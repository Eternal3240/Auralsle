//! OPPO 互联一键适配与官方文件恢复模块（纯 Rust 实现，无 Python 依赖）。
//!
//! - 支持版本白名单校验（官方原始 SHA256 与已补丁 SHA256）；
//! - 自动检测 / 一键适配 / 恢复官方文件；
//! - 所有子进程均使用 `CREATE_NO_WINDOW`，绝不闪现控制台窗口。

pub mod detect;
pub mod manifest;
pub mod patch;
pub mod restore;

use std::sync::{Mutex, OnceLock};

pub use detect::{CompatStatus, DetectReport};
pub use manifest::{PATCHED_ASAR_SHA256, ORIG_ASAR_SHA256};

#[derive(Debug, Clone, Default)]
pub struct CompatUiState {
    pub initialized: bool,
    pub busy: bool,
    pub confirm_patch: bool,
    pub confirm_restore: bool,
    pub report: DetectReport,
    pub last_message: String,
    pub last_error: String,
}

static STATE: OnceLock<Mutex<CompatUiState>> = OnceLock::new();

fn lock_state() -> std::sync::MutexGuard<'static, CompatUiState> {
    STATE
        .get_or_init(|| Mutex::new(CompatUiState::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub fn snapshot() -> CompatUiState {
    lock_state().clone()
}

pub fn set_confirm_patch(v: bool) {
    let mut g = lock_state();
    g.confirm_patch = v;
    if v {
        g.confirm_restore = false;
    }
}

pub fn set_confirm_restore(v: bool) {
    let mut g = lock_state();
    g.confirm_restore = v;
    if v {
        g.confirm_patch = false;
    }
}

pub fn ensure_initial_detect(ctx: &eframe::egui::Context) {
    {
        let mut g = lock_state();
        if g.initialized || g.busy {
            return;
        }
        g.initialized = true;
    }
    trigger_detect(ctx.clone());
}

pub fn trigger_detect(ctx: eframe::egui::Context) {
    {
        let mut g = lock_state();
        if g.busy {
            return;
        }
        g.busy = true;
        g.confirm_patch = false;
        g.confirm_restore = false;
        g.last_error.clear();
        g.last_message = "正在检测 OPPO 互联版本与完整性…".to_owned();
    }
    ctx.request_repaint();
    let _ = std::thread::Builder::new()
        .name("auralsle-oppo-compat-detect".into())
        .spawn(move || {
            let report = detect::detect_now();
            let mut g = lock_state();
            g.busy = false;
            g.last_message = report.detail.clone();
            g.report = report;
            ctx.request_repaint();
        });
}

pub fn trigger_adapt(ctx: eframe::egui::Context) {
    let (exe, asar) = {
        let mut g = lock_state();
        if g.busy {
            return;
        }
        let Some(exe) = g.report.exe_path.clone() else {
            g.last_error = "未检测到 O+Connect.exe 路径".to_owned();
            return;
        };
        let Some(asar) = g.report.asar_path.clone() else {
            g.last_error = "未检测到 app.asar 路径".to_owned();
            return;
        };
        g.busy = true;
        g.confirm_patch = false;
        g.confirm_restore = false;
        g.last_error.clear();
        g.last_message = "正在关闭 OPPO 互联、备份并执行安全适配…".to_owned();
        (exe, asar)
    };
    ctx.request_repaint();
    let _ = std::thread::Builder::new()
        .name("auralsle-oppo-compat-patch".into())
        .spawn(move || {
            let res = patch::apply_adapt(&exe, &asar);
            let mut g = lock_state();
            g.busy = false;
            match res {
                Ok(report) => {
                    g.last_error.clear();
                    g.last_message = "一键适配完成，OPPO 互联已静默重启。".to_owned();
                    g.report = report;
                }
                Err(fail) => {
                    g.last_error = fail.error;
                    g.last_message.clear();
                    g.report = fail.report;
                }
            }
            ctx.request_repaint();
        });
}

pub fn trigger_restore(ctx: eframe::egui::Context) {
    let (exe, asar) = {
        let mut g = lock_state();
        if g.busy {
            return;
        }
        let Some(exe) = g.report.exe_path.clone() else {
            g.last_error = "未检测到 O+Connect.exe 路径".to_owned();
            return;
        };
        let Some(asar) = g.report.asar_path.clone() else {
            g.last_error = "未检测到 app.asar 路径".to_owned();
            return;
        };
        g.busy = true;
        g.confirm_patch = false;
        g.confirm_restore = false;
        g.last_error.clear();
        g.last_message = "正在恢复官方文件并重启 OPPO 互联…".to_owned();
        (exe, asar)
    };
    ctx.request_repaint();
    let _ = std::thread::Builder::new()
        .name("auralsle-oppo-compat-restore".into())
        .spawn(move || {
            let res = restore::restore_official(&exe, &asar);
            let mut g = lock_state();
            g.busy = false;
            match res {
                Ok(report) => {
                    g.last_error.clear();
                    g.last_message = "已从 app.asar.bak 恢复官方文件。".to_owned();
                    g.report = report;
                }
                Err(e) => {
                    g.last_error = e;
                    g.last_message.clear();
                    g.report = detect::detect_now();
                }
            }
            ctx.request_repaint();
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn whitelist_and_status_classification_covers_all_cases() {
        let rec = manifest::CompatRecord {
            oplus_exe: "O+Connect.exe".into(),
            oplus_version: "17.20.3.0".into(),
            asar_path: "app.asar".into(),
            backup_path: "app.asar.bak".into(),
            original_sha256: ORIG_ASAR_SHA256.into(),
            patched_sha256: PATCHED_ASAR_SHA256.into(),
            adapted_at_unix: 100,
        };

        let (st, can_patch, can_restore, _) =
            detect::classify_state(ORIG_ASAR_SHA256, true, false, false, None);
        assert_eq!(st, CompatStatus::SupportedUnpatched);
        assert!(can_patch);
        assert!(!can_restore);

        let (st, can_patch, can_restore, _) =
            detect::classify_state(PATCHED_ASAR_SHA256, true, true, true, Some(&rec));
        assert_eq!(st, CompatStatus::Adapted);
        assert!(!can_patch);
        assert!(can_restore);

        let (st, can_patch, _, _) =
            detect::classify_state("DEADBEEF".repeat(8).as_str(), true, true, true, None);
        assert_eq!(st, CompatStatus::Unsupported);
        assert!(!can_patch);

        // O+ 升级后哈希变成新版本，且记录里旧 patched 哈希不匹配
        let (st, can_patch, _, _) =
            detect::classify_state("CAFEBABE".repeat(8).as_str(), true, true, true, Some(&rec));
        assert_eq!(st, CompatStatus::NeedsReadaptAfterUpdate);
        assert!(!can_patch);

        // 即使覆盖回了已知可适配的原版，若有旧记录仍明确提示需重新适配（禁止静默自动修改）
        let (st, can_patch, _, _) =
            detect::classify_state(ORIG_ASAR_SHA256, true, true, true, Some(&rec));
        assert_eq!(st, CompatStatus::NeedsReadaptAfterUpdate);
        assert!(can_patch);
    }

    #[test]
    fn synthetic_asar_patch_verify_and_restore_roundtrip() {
        let dir = std::env::temp_dir().join(format!("auralsle-compat-synth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let bg = b"function receiveNotificationAdd(e){if(e.hasOwnProperty(\"userIdentifier\")&&x!=e.userIdentifier)return(\"createNotification different users return\");return ok;}".to_vec();
        let bg_hash = manifest::sha256_bytes_lower(&bg);
        let header_json = format!(
            "{{\"files\":{{\"background.js\":{{\"size\":{},\"offset\":\"0\",\"integrity\":{{\"algorithm\":\"SHA256\",\"hash\":\"{}\",\"blockSize\":4194304,\"blocks\":[\"{}\"]}}}}}}}}",
            bg.len(),
            bg_hash,
            bg_hash
        );
        let mut raw_asar = Vec::new();
        // 16 字节简化 pickle 头（首个 '{' 在下标 16）
        raw_asar.extend_from_slice(&[0u8; 16]);
        raw_asar.extend_from_slice(header_json.as_bytes());
        while (raw_asar.len() & 3) != 0 {
            raw_asar.push(0);
        }
        raw_asar.extend_from_slice(&bg);

        let asar_path = dir.join("app.asar");
        let bak_path = dir.join("app.asar.bak");
        let tmp_path = dir.join("app.asar.tmp");
        std::fs::write(&asar_path, &raw_asar).unwrap();
        std::fs::copy(&asar_path, &bak_path).unwrap();
        let orig_sha = manifest::sha256_file(&asar_path).unwrap();

        assert!(patch::verify_asar_integrity(&asar_path).unwrap());
        let p = patch::patch_asar_to_temp(&asar_path, &tmp_path).unwrap();
        assert_eq!(p.hit_expr, "x!=");
        assert_eq!(p.replaced_with, "0&&");

        patch::atomic_swap_file(&asar_path, &tmp_path, &bak_path).unwrap();
        assert!(patch::verify_asar_integrity(&asar_path).unwrap());
        assert_ne!(manifest::sha256_file(&asar_path).unwrap(), orig_sha);

        // 从临时备份恢复
        let restored_sha = restore::restore_asar_from_backup(&asar_path, &bak_path).unwrap();
        assert_eq!(restored_sha, orig_sha);
        assert_eq!(manifest::sha256_file(&asar_path).unwrap(), orig_sha);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn real_oplus_backup_temp_copy_matches_exact_whitelist_hashes() {
        let real_exe = PathBuf::from(r"D:\AMicSoftEXE\Oppo Connect\O+Connect.exe");
        let real_bak = PathBuf::from(r"D:\AMicSoftEXE\Oppo Connect\resources\app.asar.bak");
        if !real_exe.is_file() || !real_bak.is_file() {
            return;
        }
        assert_eq!(detect::read_pe_file_version(&real_exe).as_deref(), Some("17.20.3.0"));
        let bak_sha = manifest::sha256_file(&real_bak).unwrap();
        assert_eq!(bak_sha, ORIG_ASAR_SHA256);

        let dir = std::env::temp_dir().join(format!("auralsle-compat-real-tmp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let copy_src = dir.join("app.asar");
        let copy_bak = dir.join("app.asar.bak");
        let tmp_out = dir.join("app.asar.tmp");

        std::fs::copy(&real_bak, &copy_src).unwrap();
        std::fs::copy(&real_bak, &copy_bak).unwrap();

        let patched = patch::patch_asar_to_temp(&copy_src, &tmp_out).unwrap();
        assert_eq!(
            patched.patched_sha256, PATCHED_ASAR_SHA256,
            "纯 Rust 原地补丁必须与白名单已补丁 SHA256 逐字节完全一致"
        );
        patch::atomic_swap_file(&copy_src, &tmp_out, &copy_bak).unwrap();
        assert_eq!(manifest::sha256_file(&copy_src).unwrap(), PATCHED_ASAR_SHA256);

        let restored = restore::restore_asar_from_backup(&copy_src, &copy_bak).unwrap();
        assert_eq!(restored, ORIG_ASAR_SHA256);
        assert_eq!(manifest::sha256_file(&copy_src).unwrap(), ORIG_ASAR_SHA256);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn real_oplus_detect_reports_adapted_without_modifying_files() {
        let real_exe = PathBuf::from(r"D:\AMicSoftEXE\Oppo Connect\O+Connect.exe");
        let real_asar = PathBuf::from(r"D:\AMicSoftEXE\Oppo Connect\resources\app.asar");
        if !real_exe.is_file() || !real_asar.is_file() {
            return;
        }
        let rec_tmp = std::env::temp_dir().join(format!("auralsle-compat-rec-test-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&rec_tmp);
        let rep = detect::detect_with_paths(Some(&real_exe), Some(&real_asar), &rec_tmp);
        assert_eq!(rep.status, CompatStatus::Adapted);
        assert_eq!(rep.version, "17.20.3.0");
        assert_eq!(rep.current_sha256, PATCHED_ASAR_SHA256);
        assert!(!rep.can_patch);
        assert!(rep.can_restore);
        let _ = std::fs::remove_file(&rec_tmp);
    }
}
