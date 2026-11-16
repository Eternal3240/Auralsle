//! 帧级卡死与掉帧看门狗诊断设施（`frame-watch.log`）：
//! - 主线程通过无锁原子变量记录当前帧序号、阶段名称、进入帧/阶段的时间戳、当前页面与设置页状态；
//! - 独立后台看门狗线程每 100ms 巡检一次：
//!   - 单帧耗时 `>= 450ms` 记录 `[WARN-SLOW]` 慢帧告警及停留阶段；
//!   - 单帧耗时 `>= 1500ms` 记录 `[FREEZE-HANG]` 主线程卡死现场快照；
//!   - 帧结束时若总耗时 `>= 120ms` 记录 `[FRAME-END-SLOW]` 耗时明细；
//! - 日志同时写入 `%APPDATA%\Auralsle\diagnostics\frame-watch.log` 与配置根目录 `frame-watch.log`，自动滚动截断防膨胀。

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static WATCHDOG_STARTED: OnceLock<()> = OnceLock::new();
static MONO_EPOCH: OnceLock<Instant> = OnceLock::new();

static USER_LOG_ACTIVE: AtomicBool = AtomicBool::new(false);
static USER_LOG_START_SECS: AtomicU64 = AtomicU64::new(0);
static USER_LOG_LINES: AtomicU64 = AtomicU64::new(0);
static USER_LOG_LAST_PAGE: AtomicUsize = AtomicUsize::new(usize::MAX);
static USER_LOG_LAST_OPEN: AtomicBool = AtomicBool::new(false);
static USER_LOG_PATH: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

pub fn is_user_log_active() -> bool {
    USER_LOG_ACTIVE.load(Ordering::Relaxed)
}

pub fn user_log_status() -> Option<(PathBuf, u64, u64)> {
    let p = USER_LOG_PATH.lock().ok().and_then(|g| g.clone())?;
    let elapsed = unix_secs().saturating_sub(USER_LOG_START_SECS.load(Ordering::Relaxed));
    let lines = USER_LOG_LINES.load(Ordering::Relaxed);
    Some((p, elapsed, lines))
}

pub fn start_user_log() -> PathBuf {
    let ts = unix_secs();
    let dir = default_log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("AuraIsle-runtime-{}.log", ts));
    if let Ok(mut g) = USER_LOG_PATH.lock() {
        *g = Some(path.clone());
    }
    USER_LOG_START_SECS.store(ts, Ordering::Relaxed);
    USER_LOG_LINES.store(0, Ordering::Relaxed);
    USER_LOG_LAST_PAGE.store(PAGE_CUR.load(Ordering::Relaxed), Ordering::Relaxed);
    USER_LOG_LAST_OPEN.store(ISLAND_OPEN.load(Ordering::Relaxed), Ordering::Relaxed);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&path) {
        let _ = writeln!(f, "\u{feff}=== AuraIsle Runtime Log Started ts={} ===", ts);
    }
    USER_LOG_ACTIVE.store(true, Ordering::Release);
    path
}

pub fn stop_user_log() -> Option<(PathBuf, u64, u64)> {
    USER_LOG_ACTIVE.store(false, Ordering::Release);
    let ts = unix_secs();
    let elapsed = ts.saturating_sub(USER_LOG_START_SECS.load(Ordering::Relaxed));
    let lines = USER_LOG_LINES.load(Ordering::Relaxed);
    let path = USER_LOG_PATH.lock().ok().and_then(|g| g.clone())?;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "=== Auralsle Runtime Log Stopped ts={} elapsed_s={} events={} ===", ts, elapsed, lines);
    }
    Some((path, elapsed, lines))
}

pub fn record_user_event(tag: &str, detail: &str) {
    if !USER_LOG_ACTIVE.load(Ordering::Relaxed) {
        // [诊断探针] 未开启用户行为日志时，事件仍然落到 frame-watch.log，
        // 这样自动化复现问题时不必先手动点开「用户日志」，也能拿到切页来源等现场信息。
        append_log_line(&format!("[PROBE] ts={} frame={} {} {}", unix_secs(), FRAME_SEQ.load(Ordering::Relaxed), tag, detail));
        return;
    }
    let Some(path) = USER_LOG_PATH.lock().ok().and_then(|g| g.clone()) else { return; };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let seq = FRAME_SEQ.load(Ordering::Relaxed);
        let _ = writeln!(f, "[{}] ts={} frame={} {}", tag, unix_secs(), seq, detail);
        USER_LOG_LINES.fetch_add(1, Ordering::Relaxed);
    }
}

static IN_FRAME: AtomicBool = AtomicBool::new(false);
static FRAME_SEQ: AtomicU64 = AtomicU64::new(0);
static FRAME_START_MS: AtomicI64 = AtomicI64::new(0);
static STAGE_START_MS: AtomicI64 = AtomicI64::new(0);
static STAGE_NAME_PTR: AtomicUsize = AtomicUsize::new(0);
static STAGE_NAME_LEN: AtomicUsize = AtomicUsize::new(0);
static SLOWEST_STAGE_MS: AtomicI64 = AtomicI64::new(0);
static SLOWEST_STAGE_PTR: AtomicUsize = AtomicUsize::new(0);
static SLOWEST_STAGE_LEN: AtomicUsize = AtomicUsize::new(0);

fn slowest_stage_name() -> &'static str {
    let ptr = SLOWEST_STAGE_PTR.load(Ordering::Acquire);
    let len = SLOWEST_STAGE_LEN.load(Ordering::Acquire);
    if ptr == 0 || len == 0 {
        return current_stage_name();
    }
    unsafe {
        let slice = std::slice::from_raw_parts(ptr as *const u8, len);
        std::str::from_utf8_unchecked(slice)
    }
}

fn commit_prev_stage_duration(now: i64) {
    let s_start = STAGE_START_MS.load(Ordering::Relaxed);
    let dur = (now - s_start).max(0);
    if dur > SLOWEST_STAGE_MS.load(Ordering::Relaxed) {
        SLOWEST_STAGE_MS.store(dur, Ordering::Relaxed);
        SLOWEST_STAGE_LEN.store(STAGE_NAME_LEN.load(Ordering::Acquire), Ordering::Release);
        SLOWEST_STAGE_PTR.store(STAGE_NAME_PTR.load(Ordering::Acquire), Ordering::Release);
    }
}

static PAGE_CUR: AtomicUsize = AtomicUsize::new(0);
static PAGE_FROM: AtomicUsize = AtomicUsize::new(0);
static ISLAND_OPEN: AtomicBool = AtomicBool::new(false);
static SETTINGS_OPEN: AtomicBool = AtomicBool::new(false);
static SETTINGS_TAB: AtomicUsize = AtomicUsize::new(0);

fn mono_ms() -> i64 {
    let ep = MONO_EPOCH.get_or_init(Instant::now);
    ep.elapsed().as_millis() as i64
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn current_stage_name() -> &'static str {
    let ptr = STAGE_NAME_PTR.load(Ordering::Acquire);
    let len = STAGE_NAME_LEN.load(Ordering::Acquire);
    if ptr == 0 || len == 0 {
        return "idle";
    }
    unsafe {
        let slice = std::slice::from_raw_parts(ptr as *const u8, len);
        std::str::from_utf8_unchecked(slice)
    }
}

/// 默认日志保存目录：优先使用软件安装目录（若可写，如便携目录），无写权限（如 Program Files 安装目录）时自动回退至 `%APPDATA%\Auralsle\diagnostics`。
pub fn default_log_dir() -> PathBuf {
    static WRITABLE_DIR: OnceLock<PathBuf> = OnceLock::new();
    WRITABLE_DIR
        .get_or_init(|| {
            let inst = crate::auralsle_ai_status::config::install_dir();
            let probe = inst.join(format!(".auraisle-write-probe-{}", std::process::id()));
            if std::fs::write(&probe, b"ok").is_ok() {
                let _ = std::fs::remove_file(&probe);
                return inst;
            }
            let diag = crate::model::config_dir().join("diagnostics");
            let _ = std::fs::create_dir_all(&diag);
            diag
        })
        .clone()
}

fn log_paths() -> [PathBuf; 3] {
    let inst_dir = default_log_dir();
    let cfg_dir = crate::model::config_dir();
    let diag_dir = cfg_dir.join("diagnostics");
    let _ = std::fs::create_dir_all(&diag_dir);
    [
        inst_dir.join("auralsle-diagnostic.log"),
        inst_dir.join("frame-watch.log"),
        diag_dir.join("frame-watch.log"),
    ]
}

/// [滚动卡顿根因] 旧版在 UI 线程里同步写 3 个文件（`metadata` + 轮转读 + `open/write`）。
/// 慢帧越多写得越多，一次滚动就能在渲染线程里排进几十次磁盘写，形成「卡 → 记日志 → 更卡」的正反馈。
/// 现在全部改成投递到后台写线程：`append_log_line` 只做一次 `send`，UI 线程零磁盘 IO。
static LOG_TX: OnceLock<std::sync::mpsc::Sender<String>> = OnceLock::new();

fn log_writer() -> &'static std::sync::mpsc::Sender<String> {
    LOG_TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let _ = std::thread::Builder::new()
            .name("auralsle-frame-watch-io".to_owned())
            .spawn(move || {
                while let Ok(line) = rx.recv() {
                    write_line_sync(&line);
                }
            });
        tx
    })
}

pub fn append_log_line(line: &str) {
    if cfg!(test) {
        return;
    }
    let _ = log_writer().send(line.to_owned());
}

fn write_line_sync(line: &str) {
    for path in log_paths() {
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() > 256 * 1024 {
                if let Ok(existing) = std::fs::read_to_string(&path) {
                    let tail: Vec<&str> = existing.lines().rev().take(200).collect();
                    let mut rebuilt = String::from("=== frame-watch.log rotated ===\n");
                    for l in tail.into_iter().rev() {
                        rebuilt.push_str(l);
                        rebuilt.push('\n');
                    }
                    let _ = std::fs::write(&path, rebuilt);
                }
            }
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// 启动后台帧级卡死看门狗线程（幂等，多次调用仅启动一次）。
pub fn install() {
    if cfg!(test) {
        return;
    }
    WATCHDOG_STARTED.get_or_init(|| {
        let _ = MONO_EPOCH.get_or_init(Instant::now);
        append_log_line(&format!(
            "[BOOT] ts={} pid={} version={} install_dir=\"{}\" frame-watch initialized",
            unix_secs(),
            std::process::id(),
            env!("CARGO_PKG_VERSION"),
            default_log_dir().display()
        ));
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let loc = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "unknown".to_owned());
            let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_owned()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "panic payload".to_owned()
            };
            let stage = current_stage_name();
            let p_cur = PAGE_CUR.load(Ordering::Relaxed);
            let s_open = SETTINGS_OPEN.load(Ordering::Relaxed);
            let s_tab = SETTINGS_TAB.load(Ordering::Relaxed);
            append_log_line(&format!(
                "[CRASH-PANIC] ts={} loc=\"{}\" stage=\"{}\" page={} settings_open={} settings_tab={} msg=\"{}\"",
                unix_secs(),
                loc,
                stage,
                p_cur,
                s_open,
                s_tab,
                msg.replace('\"', "'")
            ));
            prev_hook(info);
        }));
        let _ = std::thread::Builder::new()
            .name("auralsle-frame-watch".to_owned())
            .spawn(move || {
                let mut last_warn_seq = 0u64;
                let mut last_hang_seq = 0u64;
                loop {
                    std::thread::sleep(Duration::from_millis(100));
                    if !IN_FRAME.load(Ordering::Acquire) {
                        continue;
                    }
                    let now = mono_ms();
                    let seq = FRAME_SEQ.load(Ordering::Acquire);
                    let f_start = FRAME_START_MS.load(Ordering::Relaxed);
                    let s_start = STAGE_START_MS.load(Ordering::Relaxed);
                    let frame_elapsed = (now - f_start).max(0);
                    let stage_elapsed = (now - s_start).max(0);
                    let stage = current_stage_name();
                    let p_cur = PAGE_CUR.load(Ordering::Relaxed);
                    let p_from = PAGE_FROM.load(Ordering::Relaxed);
                    let open = ISLAND_OPEN.load(Ordering::Relaxed);
                    let s_open = SETTINGS_OPEN.load(Ordering::Relaxed);
                    let s_tab = SETTINGS_TAB.load(Ordering::Relaxed);

                    if frame_elapsed >= 1500 && last_hang_seq != seq {
                        last_hang_seq = seq;
                        append_log_line(&format!(
                            "[FREEZE-HANG] ts={} frame={} frame_ms={} stage=\"{}\" stage_ms={} page={}->{} open={} settings_open={} settings_tab={}",
                            unix_secs(),
                            seq,
                            frame_elapsed,
                            stage,
                            stage_elapsed,
                            p_from,
                            p_cur,
                            open,
                            s_open,
                            s_tab
                        ));
                    } else if frame_elapsed >= 450 && last_warn_seq != seq {
                        last_warn_seq = seq;
                        append_log_line(&format!(
                            "[WARN-SLOW] ts={} frame={} frame_ms={} stage=\"{}\" stage_ms={} page={}->{} open={} settings_open={} settings_tab={}",
                            unix_secs(),
                            seq,
                            frame_elapsed,
                            stage,
                            stage_elapsed,
                            p_from,
                            p_cur,
                            open,
                            s_open,
                            s_tab
                        ));
                    }
                }
            });
    });
}

#[inline]
pub fn begin_frame(
    page: usize,
    page_from: usize,
    open: bool,
    settings_open: bool,
    settings_tab: usize,
) {
    if cfg!(test) {
        return;
    }
    let now = mono_ms();
    PAGE_CUR.store(page, Ordering::Relaxed);
    PAGE_FROM.store(page_from, Ordering::Relaxed);
    ISLAND_OPEN.store(open, Ordering::Relaxed);
    SETTINGS_OPEN.store(settings_open, Ordering::Relaxed);
    SETTINGS_TAB.store(settings_tab, Ordering::Relaxed);
    FRAME_START_MS.store(now, Ordering::Relaxed);
    STAGE_START_MS.store(now, Ordering::Relaxed);
    let s: &'static str = "frame:begin";
    STAGE_NAME_LEN.store(s.len(), Ordering::Release);
    STAGE_NAME_PTR.store(s.as_ptr() as usize, Ordering::Release);
    SLOWEST_STAGE_MS.store(0, Ordering::Relaxed);
    SLOWEST_STAGE_LEN.store(s.len(), Ordering::Release);
    SLOWEST_STAGE_PTR.store(s.as_ptr() as usize, Ordering::Release);
    FRAME_SEQ.fetch_add(1, Ordering::AcqRel);
    IN_FRAME.store(true, Ordering::Release);
}

#[inline]
pub fn update_context(
    page: usize,
    page_from: usize,
    open: bool,
    settings_open: bool,
    settings_tab: usize,
) {
    if cfg!(test) {
        return;
    }
    PAGE_CUR.store(page, Ordering::Relaxed);
    PAGE_FROM.store(page_from, Ordering::Relaxed);
    ISLAND_OPEN.store(open, Ordering::Relaxed);
    SETTINGS_OPEN.store(settings_open, Ordering::Relaxed);
    SETTINGS_TAB.store(settings_tab, Ordering::Relaxed);
}

#[inline]
pub fn mark_stage(stage: &'static str) {
    if cfg!(test) {
        return;
    }
    let now = mono_ms();
    commit_prev_stage_duration(now);
    STAGE_START_MS.store(now, Ordering::Relaxed);
    STAGE_NAME_LEN.store(stage.len(), Ordering::Release);
    STAGE_NAME_PTR.store(stage.as_ptr() as usize, Ordering::Release);
}

#[inline]
pub fn end_frame() {
    if cfg!(test) {
        return;
    }
    let now = mono_ms();
    let f_start = FRAME_START_MS.load(Ordering::Relaxed);
    let elapsed = (now - f_start).max(0);
    commit_prev_stage_duration(now);
    let last_stage = slowest_stage_name();
    IN_FRAME.store(false, Ordering::Release);
    let seq = FRAME_SEQ.load(Ordering::Relaxed);
    let p_cur = PAGE_CUR.load(Ordering::Relaxed);
    let p_from = PAGE_FROM.load(Ordering::Relaxed);
    let open = ISLAND_OPEN.load(Ordering::Relaxed);
    let s_open = SETTINGS_OPEN.load(Ordering::Relaxed);
    let s_tab = SETTINGS_TAB.load(Ordering::Relaxed);
    if USER_LOG_ACTIVE.load(Ordering::Relaxed) {
        let prev_p = USER_LOG_LAST_PAGE.swap(p_cur, Ordering::Relaxed);
        let prev_o = USER_LOG_LAST_OPEN.swap(open, Ordering::Relaxed);
        if prev_p != p_cur || prev_o != open {
            record_user_event(
                "STATE",
                &format!("page={}->{} open={}->{} settings_open={} settings_tab={}", prev_p, p_cur, prev_o, open, s_open, s_tab),
            );
        }
        if elapsed >= 25 {
            record_user_event(
                if elapsed >= 120 { "SLOW" } else { "FRAME" },
                &format!("total_ms={} stage=\"{}\" page={}->{} open={} settings_open={}", elapsed, last_stage, p_from, p_cur, open, s_open),
            );
        }
    }
    if elapsed >= 45 {
        let tag = if elapsed >= 120 { "[FRAME-END-SLOW]" } else { "[JANK-FRAME]" };
        append_log_line(&format!(
            "{} ts={} frame={} total_ms={} last_stage=\"{}\" page={}->{} open={} settings_open={} settings_tab={}",
            tag,
            unix_secs(),
            seq,
            elapsed,
            last_stage,
            p_from,
            p_cur,
            open,
            s_open,
            s_tab
        ));
    }
}

/// 清空安装目录与配置目录下的诊断日志。
pub fn clear_all_logs() {
    for p in log_paths() {
        let _ = std::fs::write(&p, "");
    }
}

/// 提取并分析软件运行日志中的卡顿、掉帧、慢帧与崩溃原因，默认将完整报告保存在软件安装目录 `auralsle-log-report.txt`（需求 14）。
pub fn extract_and_analyze_log_report() -> (PathBuf, String) {
    let inst_dir = default_log_dir();
    let exe_dir = crate::auralsle_ai_status::config::install_dir();
    let cfg_dir = crate::model::config_dir();
    let diag_dir = cfg_dir.join("diagnostics");
    let mut candidate_files = vec![
        inst_dir.join("auralsle-diagnostic.log"),
        inst_dir.join("frame-watch.log"),
        exe_dir.join("frame-watch.log"),
        diag_dir.join("frame-watch.log"),
        diag_dir.join("frames.log"),
        diag_dir.join("frames.previous.log"),
        diag_dir.join("last-error.log"),
        diag_dir.join("last-graphics.log"),
        diag_dir.join("last-startup.log"),
        diag_dir.join("last-resource-anomaly.log"),
        diag_dir.join("resources.log"),
        diag_dir.join("lyrics.log"),
        diag_dir.join("weather-network.log"),
        diag_dir.join("auto-restart.log"),
        cfg_dir.join("frame-watch.log"),
        cfg_dir.join("startup.log"),
        cfg_dir.join("glass-diagnostics.txt"),
    ];
    candidate_files.sort();
    candidate_files.dedup();

    let mut crashes = Vec::new();
    let mut freezes = Vec::new();
    let mut slows = Vec::new();
    let mut janks = Vec::new();
    let mut errors = Vec::new();
    let mut last_graphics = String::new();
    let mut last_startup = String::new();
    let mut read_sources = 0usize;
    let mut stage_counts: std::collections::HashMap<String, (usize, i64)> = std::collections::HashMap::new();

    for path in &candidate_files {
        let Ok(content) = std::fs::read_to_string(path) else { continue };
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !content.trim().is_empty() {
            read_sources += 1;
        }
        if fname == "last-graphics.log" && !content.trim().is_empty() {
            last_graphics = content.lines().last().unwrap_or("").trim().to_owned();
            continue;
        }
        if fname == "last-startup.log" && !content.trim().is_empty() {
            last_startup = content.lines().last().unwrap_or("").trim().to_owned();
            continue;
        }
        for line in content.lines() {
            let trimmed = line.trim().trim_start_matches('\u{feff}');
            if trimmed.is_empty() {
                continue;
            }
            let stage = trimmed
                .split("stage=\"")
                .nth(1)
                .or_else(|| trimmed.split("last_stage=\"").nth(1))
                .and_then(|s| s.split('\"').next())
                .unwrap_or(fname)
                .to_owned();
            let ms = trimmed
                .split("total_ms=")
                .nth(1)
                .or_else(|| trimmed.split("frame_ms=").nth(1))
                .and_then(|s| s.split_whitespace().next())
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);

            if trimmed.contains("[CRASH-PANIC]") || trimmed.contains("panic") {
                crashes.push(trimmed.to_owned());
            } else if trimmed.contains("[FREEZE-HANG]") {
                freezes.push(trimmed.to_owned());
                let entry = stage_counts.entry(stage).or_insert((0, 0));
                entry.0 += 1;
                entry.1 = entry.1.max(ms);
            } else if trimmed.contains("[WARN-SLOW]") || trimmed.contains("[FRAME-END-SLOW]") || (fname.starts_with("frames") && trimmed.starts_with("帧 ")) {
                slows.push(trimmed.to_owned());
                let entry = stage_counts.entry(stage).or_insert((0, 0));
                entry.0 += 1;
                entry.1 = entry.1.max(ms);
            } else if trimmed.contains("[JANK-FRAME]") {
                janks.push(trimmed.to_owned());
                let entry = stage_counts.entry(stage).or_insert((0, 0));
                entry.0 += 1;
                entry.1 = entry.1.max(ms);
            } else if fname == "last-error.log" || trimmed.contains("崩溃") || trimmed.contains("失败") || trimmed.contains("ERROR") {
                errors.push(format!("[{fname}] {trimmed}"));
            }
        }
    }

    let mut stage_vec: Vec<(String, usize, i64)> = stage_counts
        .into_iter()
        .map(|(k, (cnt, max_ms))| (k, cnt, max_ms))
        .collect();
    stage_vec.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.1.cmp(&a.1)));

    let mut summary = String::new();
    summary.push_str(&format!(
        "=== 灵屿 AuraIsle Log 诊断分析报告 (v{}) ===\n",
        env!("CARGO_PKG_VERSION")
    ));
    summary.push_str(&format!("• 已扫描有效日志文件：{} 个（诊断目录：{}）\n", read_sources, diag_dir.display()));
    if !last_graphics.is_empty() {
        summary.push_str(&format!("• 图形驱动摘要：{}\n", last_graphics));
    }
    if !last_startup.is_empty() {
        summary.push_str(&format!("• 最近启动记录：{}\n", last_startup));
    }
    summary.push_str(&format!(
        "• 统计汇总：崩溃 {} 条 | 主线程卡死(>1.5s) {} 条 | 慢帧 {} 条 | 轻微掉帧 {} 条 | 错误记录 {} 条\n",
        crashes.len(),
        freezes.len(),
        slows.len(),
        janks.len(),
        errors.len()
    ));

    if crashes.is_empty() && freezes.is_empty() && slows.is_empty() && janks.is_empty() && errors.is_empty() {
        summary.push_str("• 诊断结论：当前运行状态良好，未检测到任何崩溃、主线程卡死或明显掉帧异常。\n");
    } else {
        if !stage_vec.is_empty() {
            summary.push_str("• 耗时热点阶段定位 (Top Bottlenecks)：\n");
            for (st, cnt, max_ms) in stage_vec.iter().take(5) {
                let advice = match st.as_str() {
                    s if s.contains("settings") => "设置面板控件重绘或配置落盘",
                    s if s.contains("quota") => "AI 额度页面切换或配置同步",
                    s if s.contains("window:update") || s.contains("backdrop") => "Win32 窗口裁剪区/毛玻璃合成更新",
                    s if s.contains("tick_and_ui") => "岛体展开/收起网格与页面绘制",
                    _ => "阶段耗时偏高",
                };
                summary.push_str(&format!(
                    "  - 阶段 `{}`：触发 {} 次，峰值耗时 {} ms（原因分析：{}）\n",
                    st, cnt, max_ms, advice
                ));
            }
        }
        if let Some(last_crash) = crashes.last() {
            summary.push_str(&format!("• 最近崩溃现场：{}\n", last_crash));
        }
        if let Some(last_err) = errors.last() {
            summary.push_str(&format!("• 最近错误日志：{}\n", last_err));
        }
        if let Some(last_freeze) = freezes.last() {
            summary.push_str(&format!("• 最近卡死现场：{}\n", last_freeze));
        } else if let Some(last_slow) = slows.last() {
            summary.push_str(&format!("• 最近慢帧记录：{}\n", last_slow));
        }
    }

    let mut report_path = inst_dir.join("AuraIsle-log-report.txt");
    let _ = std::fs::create_dir_all(&inst_dir);
    let bom_payload = format!("\u{feff}{summary}");
    if std::fs::write(&report_path, &bom_payload).is_err() {
        let _ = std::fs::create_dir_all(&diag_dir);
        report_path = diag_dir.join("AuraIsle-log-report.txt");
        let _ = std::fs::write(&report_path, &bom_payload);
    }
    (report_path, summary)
}
