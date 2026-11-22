//! OPPO 互联通知镜像的账号级判定。
//!
//! 规则：
//! - 手机主应用 = O+ `userIdentifier: 0`；
//! - 手机应用分身 = O+ `userIdentifier: 999`；
//! - PC 微信/QQ 当前登录的账号，如果映射到与手机通知相同的槽位，则屏蔽；
//! - PC 未登录、或登录账号未映射，则镜像（宁可多一条，不误吞）。
//!
//! PC 账号映射放在 `%APPDATA%\Auralsle\oppo-account-policy.json`，
//! 可以用 `oppo-account-mirror-sim map ...` 写入，也可以手改。

use serde::Deserialize;
use std::{
    fs::{self, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhoneSlot {
    Main,
    Clone,
}

fn phone_slot(user_identifier: i32) -> PhoneSlot {
    if user_identifier == 0 {
        PhoneSlot::Main
    } else {
        PhoneSlot::Clone
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PcSlot {
    Main,
    Clone,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct MessengerBinding {
    #[serde(default)]
    main: Option<String>,
    #[serde(default)]
    clone: Option<String>,
}

impl MessengerBinding {
    fn slot_for(&self, id: &str) -> Option<PcSlot> {
        let id = id.trim();
        if self.main.as_deref().is_some_and(|v| v.eq_ignore_ascii_case(id)) {
            return Some(PcSlot::Main);
        }
        if self.clone.as_deref().is_some_and(|v| v.eq_ignore_ascii_case(id)) {
            return Some(PcSlot::Clone);
        }
        None
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PolicyConfig {
    #[serde(default)]
    wechat: MessengerBinding,
    #[serde(default)]
    qq: MessengerBinding,
}

fn config_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(appdata)
        .join("Auralsle")
        .join("oppo-account-policy.json")
}

fn load_config() -> PolicyConfig {
    let path = config_path();
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Mirror,
    Suppress,
}

#[derive(Clone, Debug, Default)]
struct Accounts {
    wechat: Vec<String>,
    qq: Vec<String>,
}

fn appdata() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into()))
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn age(t: Option<SystemTime>) -> Duration {
    t.and_then(|t| SystemTime::now().duration_since(t).ok())
        .unwrap_or(Duration::MAX)
}

fn process_running(image: &str) -> bool {
    let filter = format!("IMAGENAME eq {image}");
    let mut cmd = Command::new("tasklist");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW：GUI 进程里不闪黑框
    }
    cmd.args(["/FI", &filter, "/NH"])
        .output()
        .ok()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .to_ascii_lowercase()
                .contains(&image.to_ascii_lowercase())
        })
        .unwrap_or(false)
}

fn detect_wechat() -> Vec<String> {
    let mut rows: Vec<(String, PathBuf, Option<SystemTime>)> = Vec::new();
    for root in [
        appdata().join("Tencent").join("xwechat").join("login"),
        appdata().join("Tencent").join("WeChat").join("login"),
    ] {
        let Ok(entries) = fs::read_dir(root) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.to_ascii_lowercase().starts_with("wxid_") {
                continue;
            }
            let at = modified(&path.join("key_info.dat"));
            rows.push((name, path.clone(), at));
        }
    }
    if rows.is_empty() || !(process_running("Weixin.exe") || process_running("WeChat.exe")) {
        return Vec::new();
    }
    let latest = rows.iter().filter_map(|(_, _, at)| *at).max();
    rows.into_iter()
        .filter(|(_, _, at)| {
            let d = age(*at);
            if *at == latest {
                d < Duration::from_secs(30 * 24 * 3600)
            } else {
                d < Duration::from_secs(6 * 3600)
            }
        })
        .map(|(id, _, _)| id)
        .collect()
}

fn lock_held(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        match OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(path)
        {
            Ok(_) => false,
            Err(err) => {
                matches!(err.raw_os_error(), Some(32 | 33))
                    || !matches!(err.kind(), ErrorKind::NotFound)
            }
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn detect_qq() -> Vec<String> {
    let root = appdata().join("QQ").join("Partitions");
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    let qq_running = process_running("QQ.exe");
    let mut locked = Vec::new();
    let mut recent = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(id) = name.strip_prefix("qqnt_") else { continue };
        if id.is_empty() {
            continue;
        }
        let lock = path.join("Session Storage").join("LOCK");
        let held = lock_held(&lock);
        let stamp = modified(&lock).or_else(|| modified(&path));
        if held {
            locked.push(id.to_string());
        } else if qq_running && age(stamp) < Duration::from_secs(10 * 60) {
            // 仅作为“没有 LOCK 命中”时的兜底，避免把刚写过缓存但未登录的分区当成活动账号。
            recent.push(id.to_string());
        }
    }
    if locked.is_empty() {
        recent
    } else {
        locked
    }
}

fn detect_accounts() -> Accounts {
    Accounts {
        wechat: detect_wechat(),
        qq: detect_qq(),
    }
}

fn cached_accounts() -> Accounts {
    static CACHE: OnceLock<Mutex<Option<(Instant, Accounts)>>> = OnceLock::new();
    let lock = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, accounts)) = guard.as_ref() {
        if at.elapsed() < Duration::from_secs(10) {
            return accounts.clone();
        }
    }
    let accounts = detect_accounts();
    *guard = Some((Instant::now(), accounts.clone()));
    accounts
}

fn slot_for_pkg(pkg: &str) -> Option<&'static str> {
    match pkg {
        "com.tencent.mm" => Some("wechat"),
        "com.tencent.mobileqq" => Some("qq"),
        _ => None,
    }
}

fn decide(cfg: &PolicyConfig, accounts: &Accounts, pkg: &str, user_identifier: i32) -> Decision {
    let Some(client) = slot_for_pkg(pkg) else {
        return Decision::Mirror;
    };
    let active = match client {
        "wechat" => &accounts.wechat,
        "qq" => &accounts.qq,
        _ => return Decision::Mirror,
    };
    if active.is_empty() {
        return Decision::Mirror;
    }
    let binding = match client {
        "wechat" => &cfg.wechat,
        "qq" => &cfg.qq,
        _ => return Decision::Mirror,
    };
    let phone = phone_slot(user_identifier);
    for id in active {
        let pc = binding.slot_for(id);
        let same = match phone {
            PhoneSlot::Main => pc == Some(PcSlot::Main),
            PhoneSlot::Clone => pc == Some(PcSlot::Clone),
        };
        if same {
            return Decision::Suppress;
        }
    }
    Decision::Mirror
}

/// 给 `extras.rs` 用：同账号屏蔽，其余镜像。
pub fn should_suppress(pkg: &str, user_identifier: i32) -> bool {
    let cfg = load_config();
    let accounts = cached_accounts();
    match decide(&cfg, &accounts, pkg, user_identifier) {
        Decision::Suppress => true,
        Decision::Mirror => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> PolicyConfig {
        PolicyConfig {
            wechat: MessengerBinding {
                main: Some("pc-main".into()),
                clone: Some("pc-clone".into()),
            },
            qq: MessengerBinding {
                main: Some("pc-main".into()),
                clone: Some("pc-clone".into()),
            },
        }
    }

    fn accounts(client: &str, id: &str) -> Accounts {
        Accounts {
            wechat: if client == "wechat" { vec![id.into()] } else { vec![] },
            qq: if client == "qq" { vec![id.into()] } else { vec![] },
        }
    }

    #[test]
    fn same_account_suppresses() {
        let c = cfg();
        assert_eq!(decide(&c, &accounts("wechat", "pc-main"), "com.tencent.mm", 0), Decision::Suppress);
        assert_eq!(decide(&c, &accounts("qq", "pc-clone"), "com.tencent.mobileqq", 999), Decision::Suppress);
    }

    #[test]
    fn opposite_account_mirrors() {
        let c = cfg();
        assert_eq!(decide(&c, &accounts("wechat", "pc-main"), "com.tencent.mm", 999), Decision::Mirror);
        assert_eq!(decide(&c, &accounts("qq", "pc-clone"), "com.tencent.mobileqq", 0), Decision::Mirror);
    }

    #[test]
    fn no_login_or_unmapped_mirrors() {
        let c = cfg();
        assert_eq!(decide(&c, &Accounts::default(), "com.tencent.mm", 0), Decision::Mirror);
        assert_eq!(decide(&c, &accounts("wechat", "stranger"), "com.tencent.mm", 0), Decision::Mirror);
    }
}
