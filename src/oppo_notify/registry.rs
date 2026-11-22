//! OPPO 互联（`com.oplus.devicespace`）Windows 通知开关读写、原值备份与恢复。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// O+ Connect 在 Windows 注册的 AUMID
pub const OPPO_AUMID: &str = "com.oplus.devicespace";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeNotifyOriginalRecord {
    pub aumid: String,
    pub original_enabled: Option<u32>,
    pub written_at_unix: u64,
    pub managed_by_auralsle: bool,
}

pub fn original_record_path() -> PathBuf {
    let appdata = std::env::var("APPDATA")
        .or_else(|_| std::env::var("LOCALAPPDATA"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(appdata)
        .join("Auralsle")
        .join("oppo-native-notify-original.json")
}

#[cfg(windows)]
fn settings_subkey(aumid: &str) -> String {
    format!(r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\{aumid}")
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_dword_at_subkey(subkey: &str) -> Option<u32> {
    #[cfg(windows)]
    unsafe {
        use windows::core::PCWSTR;
        use windows::Win32::System::Registry::{
            RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
            REG_VALUE_TYPE,
        };
        let sub = wide(subkey);
        let mut hk = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), 0, KEY_READ, &mut hk).is_err() {
            return None;
        }
        let name = wide("Enabled");
        let mut ty = REG_VALUE_TYPE::default();
        let mut buf = [0u8; 4];
        let mut cb = 4u32;
        let ok = RegQueryValueExW(
            hk,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut ty),
            Some(buf.as_mut_ptr()),
            Some(&mut cb),
        )
        .is_ok();
        let _ = RegCloseKey(hk);
        if ok && cb >= 4 {
            Some(u32::from_ne_bytes(buf))
        } else {
            None
        }
    }
    #[cfg(not(windows))]
    {
        let _ = subkey;
        None
    }
}

fn write_dword_at_subkey(subkey: &str, val: u32) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        use windows::core::PCWSTR;
        use windows::Win32::System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
            REG_DWORD, REG_OPTION_NON_VOLATILE,
        };
        let sub = wide(subkey);
        let mut hk = HKEY::default();
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hk,
            None,
        );
        if rc.is_err() {
            return Err(format!("RegCreateKeyExW failed: {:?}", rc));
        }
        let name = wide("Enabled");
        let rc2 = RegSetValueExW(hk, PCWSTR(name.as_ptr()), 0, REG_DWORD, Some(&val.to_ne_bytes()));
        let _ = RegCloseKey(hk);
        if rc2.is_err() {
            return Err(format!("RegSetValueExW failed: {:?}", rc2));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (subkey, val);
        Ok(())
    }
}

fn delete_enabled_at_subkey(subkey: &str) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        use windows::Win32::System::Registry::{
            RegCloseKey, RegDeleteValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
        };
        let sub = wide(subkey);
        let mut hk = HKEY::default();
        let rc = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            0,
            KEY_SET_VALUE,
            &mut hk,
        );
        if rc == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if rc.is_err() {
            return Err(format!("RegOpenKeyExW failed: {:?}", rc));
        }
        let name = wide("Enabled");
        let rc2 = RegDeleteValueW(hk, PCWSTR(name.as_ptr()));
        let _ = RegCloseKey(hk);
        if rc2 == ERROR_FILE_NOT_FOUND || rc2.is_ok() {
            Ok(())
        } else {
            Err(format!("RegDeleteValueW failed: {:?}", rc2))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = subkey;
        Ok(())
    }
}

/// 读取通知设置注册表 `Enabled` 值；`None` 表示尚未写入过（系统默认开启）。
pub fn app_notifications_enabled(aumid: &str) -> Option<u32> {
    #[cfg(windows)]
    {
        read_dword_at_subkey(&settings_subkey(aumid))
    }
    #[cfg(not(windows))]
    {
        let _ = aumid;
        None
    }
}

/// 通过注册表写入 `Enabled = 0 / 1`。
pub fn set_app_notifications_enabled(aumid: &str, enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        write_dword_at_subkey(&settings_subkey(aumid), if enabled { 1 } else { 0 })
    }
    #[cfg(not(windows))]
    {
        let _ = (aumid, enabled);
        Ok(())
    }
}

/// 删除注册表里的 `Enabled` 键（恢复系统默认通知行为）。
pub fn delete_app_notifications_enabled(aumid: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        delete_enabled_at_subkey(&settings_subkey(aumid))
    }
    #[cfg(not(windows))]
    {
        let _ = aumid;
        Ok(())
    }
}

fn load_record_from(path: &Path) -> Option<NativeNotifyOriginalRecord> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save_record_to(path: &Path, rec: &NativeNotifyOriginalRecord) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(rec).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

fn suppress_with_backup_inner(aumid: &str, subkey: &str, rec_path: &Path) -> Result<(), String> {
    let current = read_dword_at_subkey(subkey);
    if current == Some(0) {
        return Ok(());
    }
    if !rec_path.is_file() {
        let rec = NativeNotifyOriginalRecord {
            aumid: aumid.to_owned(),
            original_enabled: current,
            written_at_unix: unix_now(),
            managed_by_auralsle: true,
        };
        save_record_to(rec_path, &rec)?;
    }
    write_dword_at_subkey(subkey, 0)
}

fn restore_value_at_subkey(subkey: &str, original: Option<u32>) -> Result<(), String> {
    match original {
        Some(v) => write_dword_at_subkey(subkey, v),
        None => delete_enabled_at_subkey(subkey),
    }
}

fn restore_explicit_inner(aumid: &str, subkey: &str, rec_path: &Path) -> Result<String, String> {
    let rec = load_record_from(rec_path).filter(|r| r.aumid == aumid);
    let target_original = rec.as_ref().and_then(|r| r.original_enabled);
    restore_value_at_subkey(subkey, target_original)?;
    let _ = std::fs::remove_file(rec_path);
    Ok(match target_original {
        Some(0) => "已按原始记录恢复系统通知设置（原值为关闭）".to_owned(),
        Some(_) => "已恢复 OPPO 原生通知开关（已写回开启状态）".to_owned(),
        None => "已恢复 OPPO 原生通知默认设置".to_owned(),
    })
}

fn restore_if_managed_inner(aumid: &str, subkey: &str, rec_path: &Path) -> Result<bool, String> {
    let Some(rec) = load_record_from(rec_path) else {
        return Ok(false);
    };
    if !rec.managed_by_auralsle || rec.aumid != aumid {
        return Ok(false);
    }
    let current = read_dword_at_subkey(subkey);
    let restored = if current == Some(0) {
        restore_value_at_subkey(subkey, rec.original_enabled)?;
        true
    } else {
        // 用户已在系统设置中手动改过，不覆盖用户设置
        false
    };
    let _ = std::fs::remove_file(rec_path);
    Ok(restored)
}

/// 第一次把 `Enabled` 改为 `0` 前，先把原值记录到 `%APPDATA%\Auralsle\oppo-native-notify-original.json`。
pub fn suppress_with_backup(aumid: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        suppress_with_backup_inner(aumid, &settings_subkey(aumid), &original_record_path())
    }
    #[cfg(not(windows))]
    {
        let _ = aumid;
        Ok(())
    }
}

/// 显式恢复 OPPO 原生通知（用户点击「恢复 OPPO 原生通知」或关闭静默开关时调用）。
pub fn restore_app_notifications(aumid: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        restore_explicit_inner(aumid, &settings_subkey(aumid), &original_record_path())
    }
    #[cfg(not(windows))]
    {
        let _ = aumid;
        Ok("非 Windows 平台无需操作".to_owned())
    }
}

/// 正常退出或启动清理崩溃残留时调用：仅当当前 `0` 仍由灵屿写入且未被用户手动改过时才恢复原值。
pub fn restore_if_managed(aumid: &str) -> Result<bool, String> {
    #[cfg(windows)]
    {
        restore_if_managed_inner(aumid, &settings_subkey(aumid), &original_record_path())
    }
    #[cfg(not(windows))]
    {
        let _ = aumid;
        Ok(false)
    }
}

pub fn has_leftover_backup(aumid: &str) -> bool {
    load_record_from(&original_record_path())
        .is_some_and(|r| r.managed_by_auralsle && r.aumid == aumid)
}

/// 查询 Windows 通知中心里该 AUMID 的真实系统状态（唯一可信判据）。
pub fn app_setting(aumid: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::UI::Notifications::{NotificationSetting, ToastNotificationManager};
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(aumid))
            .map_err(|e| format!("CreateToastNotifierWithId: {e}"))?;
        let s = notifier.Setting().map_err(|e| format!("Setting: {e}"))?;
        Ok(match s {
            NotificationSetting::Enabled => "Enabled",
            NotificationSetting::DisabledForApplication => "DisabledForApplication",
            NotificationSetting::DisabledForUser => "DisabledForUser",
            NotificationSetting::DisabledByGroupPolicy => "DisabledByGroupPolicy",
            NotificationSetting::DisabledByManifest => "DisabledByManifest",
            _ => "Unknown",
        }
        .to_owned())
    }
    #[cfg(not(windows))]
    {
        let _ = aumid;
        Ok("Enabled".to_owned())
    }
}

/// 打开 Windows「系统 > 通知」设置页，便于用户按需恢复 O+ Connect 原生通知开关。
pub fn open_system_notification_settings() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("explorer.exe")
            .arg("ms-settings:notifications")
            .creation_flags(NO_WINDOW)
            .spawn();
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn backup_and_restore_preserves_original_and_respects_user_changes() {
        let pid = std::process::id();
        let subkey = format!(r"Software\AuralsleTest\NotifyRestoreTest_{pid}");
        let rec_path = std::env::temp_dir().join(format!("auralsle-notify-orig-test-{pid}.json"));
        let _ = std::fs::remove_file(&rec_path);
        let _ = delete_enabled_at_subkey(&subkey);

        // 1. 原值为 None -> 抑制写 0 -> 恢复后删除 Enabled
        suppress_with_backup_inner("test.aumid", &subkey, &rec_path).unwrap();
        assert_eq!(read_dword_at_subkey(&subkey), Some(0));
        let rec = load_record_from(&rec_path).unwrap();
        assert_eq!(rec.original_enabled, None);
        assert!(rec.managed_by_auralsle);
        let _ = restore_explicit_inner("test.aumid", &subkey, &rec_path).unwrap();
        assert_eq!(read_dword_at_subkey(&subkey), None);
        assert!(!rec_path.exists());

        // 2. 原值为 Some(1) -> 抑制写 0 -> 托管恢复写回 Some(1)
        write_dword_at_subkey(&subkey, 1).unwrap();
        suppress_with_backup_inner("test.aumid", &subkey, &rec_path).unwrap();
        assert_eq!(read_dword_at_subkey(&subkey), Some(0));
        assert!(restore_if_managed_inner("test.aumid", &subkey, &rec_path).unwrap());
        assert_eq!(read_dword_at_subkey(&subkey), Some(1));

        // 3. 用户手动改为 Some(1) 后，正常退出不得覆盖
        suppress_with_backup_inner("test.aumid", &subkey, &rec_path).unwrap();
        assert_eq!(read_dword_at_subkey(&subkey), Some(0));
        write_dword_at_subkey(&subkey, 1).unwrap();
        assert!(!restore_if_managed_inner("test.aumid", &subkey, &rec_path).unwrap());
        assert_eq!(read_dword_at_subkey(&subkey), Some(1));

        // 清理测试注册表值
        let _ = delete_enabled_at_subkey(&subkey);
        let _ = std::fs::remove_file(&rec_path);
        unsafe {
            use windows::core::PCWSTR;
            use windows::Win32::System::Registry::{RegDeleteKeyW, HKEY_CURRENT_USER};
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(wide(&subkey).as_ptr()));
        }
    }
}
