//! OPPO 互联「手机通知镜像」运行时：
//! 增量跟踪 `ovmsocketlog.log`、`key` 去重、解码来源 App 图标，并及时唤醒 UI 线程。

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::model::Notification;
use super::registry::{self, OPPO_AUMID};

#[derive(Debug, Clone)]
pub struct MirrorEvent {
    pub kind: MirrorEventKind,
}

#[derive(Debug, Clone)]
pub enum MirrorEventKind {
    Started {
        log: PathBuf,
    },
    /// 收到一条手机通知；`is_update = true` 表示同 `key` 更新替换
    Notification {
        notification: Notification,
        is_update: bool,
        icon_png: Option<Arc<Vec<u8>>>,
    },
    /// O+ 通知开关抑制结果
    OppoSuppressed {
        ok: bool,
        setting: String,
        detail: String,
    },
    Waiting {
        detail: String,
    },
}

#[derive(Debug, Clone)]
pub struct MirrorConfig {
    pub suppress_oppo: bool,
    pub max_items: usize,
    pub poll: Duration,
    pub icon_dir: PathBuf,
    pub log_path: Option<PathBuf>,
}

impl Default for MirrorConfig {
    fn default() -> Self {
        Self {
            suppress_oppo: true,
            max_items: 40,
            poll: Duration::from_millis(220),
            icon_dir: default_icon_dir(),
            log_path: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MirrorStatus {
    pub active: bool,
    pub log_found: bool,
    pub detail: String,
    pub oppo_setting: String,
    pub mirrored_count: u64,
}

static MIRROR_STATUS: OnceLock<Mutex<MirrorStatus>> = OnceLock::new();
static SUPPRESS_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_suppress_active(active: bool) {
    SUPPRESS_ACTIVE.store(active, Ordering::SeqCst);
}

pub fn is_suppress_active() -> bool {
    SUPPRESS_ACTIVE.load(Ordering::SeqCst)
}

/// 刷新当前 OPPO 原生通知系统状态快照。
pub fn refresh_oppo_setting_status() -> String {
    let actual = registry::app_setting(OPPO_AUMID).unwrap_or_else(|_| "Unknown".into());
    update_status(|s| s.oppo_setting = actual.clone());
    actual
}

/// 立即取消静默抑制（先将 `oppo_suppress_native` 设为 `false` 并落盘，再恢复注册表原值）。
/// 调用后即使 5 秒轮询继续运行，也不会重新写回 0。
pub fn restore_oppo_native_now() -> Result<String, String> {
    set_suppress_active(false);
    crate::auralsle_ai_status::config::update(|c| {
        c.oppo_suppress_native = false;
    });
    let msg = registry::restore_app_notifications(OPPO_AUMID)?;
    refresh_oppo_setting_status();
    Ok(msg)
}

/// 正常退出灵屿时调用：仅当当前 0 是灵屿写入且未被用户手动改过时，恢复注册表原值。
pub fn restore_on_normal_exit() {
    set_suppress_active(false);
    let _ = registry::restore_if_managed(OPPO_AUMID);
}

/// 启动时调用：若存在上次崩溃遗留的备份记录且当前配置不再抑制，则先恢复一次原值。
pub fn restore_crash_leftover_if_needed(currently_suppressing: bool) {
    if !currently_suppressing && registry::has_leftover_backup(OPPO_AUMID) {
        let _ = registry::restore_if_managed(OPPO_AUMID);
    }
}

pub fn status_snapshot() -> MirrorStatus {
    MIRROR_STATUS
        .get_or_init(|| Mutex::new(MirrorStatus::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn update_status(f: impl FnOnce(&mut MirrorStatus)) {
    let mut g = MIRROR_STATUS
        .get_or_init(|| Mutex::new(MirrorStatus::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    f(&mut g);
}

pub fn default_icon_dir() -> PathBuf {
    let base = std::env::var("APPDATA")
        .or_else(|_| std::env::var("LOCALAPPDATA"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("Auralsle").join("icon-cache")
}

/// 读取某个手机包名最近缓存的图标 PNG 字节（供通知历史列表直接显示源 App 图标；绝不使用 OPPO 官方图标）。
pub fn cached_icon_png(pkg_name: &str) -> Option<Vec<u8>> {
    if pkg_name.trim().eq_ignore_ascii_case(OPPO_AUMID) {
        return None;
    }
    let clean: String = pkg_name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .take(48)
        .collect();
    if clean.is_empty() {
        return None;
    }
    let dir = default_icon_dir();
    let latest = dir.join(format!("pkg-{clean}.png"));
    std::fs::read(latest).ok()
}

pub struct MirrorHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl MirrorHandle {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for MirrorHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn spawn(cfg: MirrorConfig, ui_ctx: Option<eframe::egui::Context>) -> (MirrorHandle, Receiver<MirrorEvent>) {
    set_suppress_active(cfg.suppress_oppo);
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let join = std::thread::Builder::new()
        .name("auralsle-oppo-notify-mirror".into())
        .spawn(move || run(cfg, tx, stop2, ui_ctx))
        .ok();
    (MirrorHandle { stop, join }, rx)
}

fn send(tx: &Sender<MirrorEvent>, ui_ctx: Option<&eframe::egui::Context>, kind: MirrorEventKind) {
    let _ = tx.send(MirrorEvent { kind });
    if let Some(ctx) = ui_ctx {
        ctx.request_repaint();
    }
}

fn run(
    cfg: MirrorConfig,
    tx: Sender<MirrorEvent>,
    stop: Arc<AtomicBool>,
    ui_ctx: Option<eframe::egui::Context>,
) {
    update_status(|s| {
        s.active = true;
        s.detail = "正在等待 OPPO 互联通知日志…".into();
        s.oppo_setting = registry::app_setting(OPPO_AUMID).unwrap_or_else(|_| "Unknown".into());
    });

    if cfg.suppress_oppo && is_suppress_active() {
        let before = registry::app_setting(OPPO_AUMID).unwrap_or_default();
        let reg_val = registry::app_notifications_enabled(OPPO_AUMID);
        let need_write = before != "DisabledForApplication" || reg_val.is_none();
        let res = if !need_write {
            Ok(())
        } else {
            registry::suppress_with_backup(OPPO_AUMID)
        };
        let actual = registry::app_setting(OPPO_AUMID).unwrap_or_else(|e| format!("<{e}>"));
        let ok = res.is_ok() && actual == "DisabledForApplication";
        let detail = match (&res, !need_write) {
            (Err(e), _) => format!("抑制失败: {e}"),
            (_, true) => "已关闭 OPPO 原生通知".to_owned(),
            (_, false) => "已开启原生通知静默".to_owned(),
        };
        update_status(|s| {
            s.oppo_setting = actual.clone();
        });
        send(
            &tx,
            ui_ctx.as_ref(),
            MirrorEventKind::OppoSuppressed {
                ok,
                setting: actual,
                detail,
            },
        );
    }

    // 等待 O+ Connect 日志就绪（避免启动时 O+ 还没创建日志导致线程提前退出）
    let mut watcher: Option<Watcher> = None;
    let mut missing_reported = false;
    while !stop.load(Ordering::Relaxed) {
        let candidate = cfg.log_path.clone().or_else(log_path);
        if let Some(p) = candidate {
            update_status(|s| {
                s.log_found = true;
                s.detail = "已连接 OPPO 互联通知日志".into();
            });
            send(&tx, ui_ctx.as_ref(), MirrorEventKind::Started { log: p.clone() });
            // 启动前已存在的日志从文件末尾开始尾随，避免历史陈旧通知涌入；若是刚新建的文件则从 0 读取
            watcher = Some(if missing_reported {
                Watcher::new_from_start(p)
            } else {
                Watcher::new(p)
            });
            break;
        }
        if !missing_reported {
            missing_reported = true;
            update_status(|s| {
                s.log_found = false;
                s.detail = "等待 OPPO 互联启动".into();
            });
            send(
                &tx,
                ui_ctx.as_ref(),
                MirrorEventKind::Waiting {
                    detail: "等待 OPPO 互联启动".into(),
                },
            );
        }
        std::thread::sleep(Duration::from_millis(1200));
    }

    let Some(mut watcher) = watcher else {
        update_status(|s| s.active = false);
        return;
    };

    let mut seen: HashMap<String, Instant> = HashMap::new();
    let mut last_setting_poll = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        for line in watcher.drain() {
            let Some(n) = Notification::from_log_line(&line) else {
                continue;
            };
            if n.pkg_name.trim().starts_with("android.") {
                continue;
            }
            let key = n.dedupe_key();
            let is_update = seen.contains_key(&key);
            seen.insert(key, Instant::now());
            if seen.len() > cfg.max_items {
                if let Some(oldest) = seen.iter().min_by_key(|(_, &at)| at).map(|(k, _)| k.clone()) {
                    seen.remove(&oldest);
                }
            }

            let skip_cache = n.pkg_name.trim().eq_ignore_ascii_case(OPPO_AUMID);
            let icon_png = if skip_cache {
                None
            } else {
                n.icon_b64().and_then(|b64| {
                    let bytes = b64_decode(b64)?;
                    if bytes.len() < 8 {
                        return None;
                    }
                    cache_icon_bytes(&n.pkg_name, &bytes, &cfg.icon_dir);
                    Some(Arc::new(bytes))
                })
            };

            update_status(|s| s.mirrored_count = s.mirrored_count.saturating_add(1));
            send(
                &tx,
                ui_ctx.as_ref(),
                MirrorEventKind::Notification {
                    notification: n,
                    is_update,
                    icon_png,
                },
            );
        }

        if last_setting_poll.elapsed() >= Duration::from_secs(5) {
            last_setting_poll = Instant::now();
            if let Ok(actual) = registry::app_setting(OPPO_AUMID) {
                update_status(|s| s.oppo_setting = actual.clone());
                // O+ 重启或用户在系统设置里重新打开后，启动时写入的 0 会失效；
                // 运行期间仅在抑制开关处于激活态时才纠正。
                if cfg.suppress_oppo
                    && is_suppress_active()
                    && (actual != "DisabledForApplication"
                        || registry::app_notifications_enabled(OPPO_AUMID).is_none())
                {
                    let _ = registry::suppress_with_backup(OPPO_AUMID);
                }
            }
        }
        std::thread::sleep(cfg.poll);
    }

    update_status(|s| {
        s.active = false;
        s.detail = "已停止".into();
    });
}

pub fn log_path() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    let p = PathBuf::from(appdata)
        .join("O+Connect")
        .join("devicespace")
        .join("oshare")
        .join("ovmsocketlog.log");
    p.is_file().then_some(p)
}

/// 增量读取，支持文件轮转/截断与半行缓冲（若末尾尚未写完 `\n`，下次再继续读）。
pub struct Watcher {
    path: PathBuf,
    pos: u64,
    carry: String,
}

impl Watcher {
    pub fn new(path: PathBuf) -> Self {
        let pos = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Self { path, pos, carry: String::new() }
    }

    pub fn new_from_start(path: PathBuf) -> Self {
        Self { path, pos: 0, carry: String::new() }
    }

    pub fn drain(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            return out;
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.pos {
            self.pos = 0;
            self.carry.clear();
        }
        if len == self.pos {
            return out;
        }
        if f.seek(SeekFrom::Start(self.pos)).is_err() {
            return out;
        }
        let mut raw = Vec::new();
        if f.read_to_end(&mut raw).is_err() {
            self.pos = len;
            return out;
        }
        self.pos += raw.len() as u64;
        self.carry.push_str(&String::from_utf8_lossy(&raw));

        if let Some(last_nl) = self.carry.rfind('\n') {
            let complete = self.carry[..last_nl].to_owned();
            let rest = self.carry[last_nl + 1..].to_owned();
            self.carry = rest;
            for line in complete.lines() {
                if line.contains("ShowToast Data received:") {
                    out.push(line.to_string());
                }
            }
        }
        out
    }
}

fn cache_icon_bytes(pkg_name: &str, bytes: &[u8], dir: &Path) {
    let safe: String = pkg_name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .take(48)
        .collect();
    if safe.is_empty() {
        return;
    }
    let pkg_path = dir.join(format!("pkg-{safe}.png"));
    if std::fs::read(&pkg_path).ok().as_deref() == Some(bytes) {
        return;
    }
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let _ = std::fs::write(&pkg_path, bytes);
    let hash = fnv1a(bytes);
    let hashed = dir.join(format!("logo-{safe}-{hash:016x}.png"));
    if !hashed.is_file() {
        let _ = std::fs::write(&hashed, bytes);
        prune(dir, 256);
    }
}

fn prune(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if !p.is_file() {
                return None;
            }
            let name = p.file_name()?.to_string_lossy();
            if !name.starts_with("logo-") {
                return None;
            }
            let t = e.metadata().ok()?.modified().ok()?;
            Some((t, p))
        })
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(p);
    }
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(s.len() / 4 * 3 + 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in s.chars() {
        let v = match c {
            'A'..='Z' => (c as u32) - ('A' as u32),
            'a'..='z' => (c as u32) - ('a' as u32) + 26,
            '0'..='9' => (c as u32) - ('0' as u32) + 52,
            '+' => 62,
            '/' => 63,
            '=' => break,
            c if c.is_whitespace() => continue,
            _ => return None,
        };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buf.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watcher_handles_partial_line_and_rotation() {
        let dir = std::env::temp_dir().join(format!("auralsle-oppo-watcher-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("ovmsocketlog.log");
        std::fs::write(&path, "").unwrap();
        let mut w = Watcher::new_from_start(path.clone());
        std::fs::write(&path, "2026-11-21 INFO - ShowToast Data received:{\"key\":\"1\"").unwrap();
        assert!(w.drain().is_empty(), "半行尚未换行时不提前切分");
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(f, ",\"pkgName\":\"com.twitter.android\"}}").unwrap();
        let got = w.drain();
        assert_eq!(got.len(), 1);
        std::fs::write(&path, "2026-11-21 INFO - ShowToast Data received:{\"key\":\"2\"}\n").unwrap();
        let got2 = w.drain();
        assert_eq!(got2.len(), 1);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn suppress_active_flag_and_icon_cache_dir_are_valid() {
        set_suppress_active(true);
        assert!(is_suppress_active());
        set_suppress_active(false);
        assert!(!is_suppress_active());
        assert!(default_icon_dir().ends_with("icon-cache"));
    }
}
