//! OPPO 互联安装路径、PE 版本号、ASAR 哈希与状态检测。

use std::path::{Path, PathBuf};

use super::manifest::{self, CompatRecord};
use super::patch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatStatus {
    NotInstalled,
    SupportedUnpatched,
    Unsupported,
    Adapted,
    NeedsReadaptAfterUpdate,
    FailedRolledBack,
}

impl CompatStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotInstalled => "未安装",
            Self::SupportedUnpatched => "支持适配",
            Self::Unsupported => "不支持",
            Self::Adapted => "已适配",
            Self::NeedsReadaptAfterUpdate => "O+ 更新后需重新适配",
            Self::FailedRolledBack => "失败已回滚",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetectReport {
    pub status: CompatStatus,
    pub exe_path: Option<PathBuf>,
    pub asar_path: Option<PathBuf>,
    pub backup_path: Option<PathBuf>,
    pub backup_exists: bool,
    pub version: String,
    pub current_sha256: String,
    pub can_patch: bool,
    pub can_restore: bool,
    pub detail: String,
}

impl Default for DetectReport {
    fn default() -> Self {
        Self {
            status: CompatStatus::NotInstalled,
            exe_path: None,
            asar_path: None,
            backup_path: None,
            backup_exists: false,
            version: String::new(),
            current_sha256: String::new(),
            can_patch: false,
            can_restore: false,
            detail: "尚未检测".to_owned(),
        }
    }
}

/// 通过 Windows `version.dll` 读取 `O+Connect.exe` PE 文件版本号（如 `17.20.3.0`）。
#[cfg(windows)]
pub fn read_pe_file_version(path: &Path) -> Option<String> {
    #[repr(C)]
    struct VsFixedFileInfo {
        dw_signature: u32,
        dw_struc_version: u32,
        dw_file_version_ms: u32,
        dw_file_version_ls: u32,
        dw_product_version_ms: u32,
        dw_product_version_ls: u32,
        dw_file_flags_mask: u32,
        dw_file_flags: u32,
        dw_file_os: u32,
        dw_file_type: u32,
        dw_file_subtype: u32,
        dw_file_date_ms: u32,
        dw_file_date_ls: u32,
    }
    #[link(name = "version")]
    extern "system" {
        fn GetFileVersionInfoSizeW(lptstrfilename: *const u16, lpdwhandle: *mut u32) -> u32;
        fn GetFileVersionInfoW(
            lptstrfilename: *const u16,
            dwhandle: u32,
            dwlen: u32,
            lpdata: *mut core::ffi::c_void,
        ) -> i32;
        fn VerQueryValueW(
            pblock: *const core::ffi::c_void,
            lpsubblock: *const u16,
            lplpbuffer: *mut *mut core::ffi::c_void,
            pulen: *mut u32,
        ) -> i32;
    }

    let wide: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let mut handle = 0u32;
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), &mut handle);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, buf.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let sub: Vec<u16> = "\\".encode_utf16().chain(std::iter::once(0)).collect();
        let mut info_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut info_len = 0u32;
        if VerQueryValueW(buf.as_ptr().cast(), sub.as_ptr(), &mut info_ptr, &mut info_len) == 0
            || info_ptr.is_null()
            || (info_len as usize) < std::mem::size_of::<VsFixedFileInfo>()
        {
            return None;
        }
        let info = &*(info_ptr as *const VsFixedFileInfo);
        if info.dw_signature != 0xFEEF04BD {
            return None;
        }
        let major = (info.dw_file_version_ms >> 16) & 0xFFFF;
        let minor = info.dw_file_version_ms & 0xFFFF;
        let build = (info.dw_file_version_ls >> 16) & 0xFFFF;
        let rev = info.dw_file_version_ls & 0xFFFF;
        Some(format!("{major}.{minor}.{build}.{rev}"))
    }
}

#[cfg(not(windows))]
pub fn read_pe_file_version(_path: &Path) -> Option<String> {
    None
}

/// 纯逻辑状态分类器（方便单元测试覆盖全部 6 种状态，且不碰真实 O+ 安装）。
pub fn classify_state(
    current_sha256: &str,
    integrity_ok: bool,
    backup_exists: bool,
    backup_integrity_ok: bool,
    record: Option<&CompatRecord>,
) -> (CompatStatus, bool, bool, String) {
    let cur = current_sha256.trim().to_ascii_uppercase();
    let can_restore = backup_exists && backup_integrity_ok;

    if manifest::is_known_patched(&cur) && integrity_ok {
        return (
            CompatStatus::Adapted,
            false,
            can_restore,
            "已完成 OPPO 互联分身通知适配，无需重新打补丁。".to_owned(),
        );
    }

    if let Some(rec) = record {
        if !rec.patched_sha256.eq_ignore_ascii_case(&cur) {
            if manifest::is_known_original(&cur) && integrity_ok {
                return (
                    CompatStatus::NeedsReadaptAfterUpdate,
                    true,
                    can_restore,
                    "检测到 OPPO 互联已更新并覆盖补丁，当前新版本在支持白名单内，可点击「一键适配」重新适配。".to_owned(),
                );
            }
            return (
                CompatStatus::NeedsReadaptAfterUpdate,
                false,
                can_restore,
                "检测到 OPPO 互联版本已变化，但当前文件哈希不在支持白名单内，暂不支持自动修改。".to_owned(),
            );
        }
    }

    if manifest::is_known_original(&cur) {
        if integrity_ok {
            return (
                CompatStatus::SupportedUnpatched,
                true,
                can_restore,
                "检测到官方支持版本，可点击「一键适配」开启微信/QQ 分身通知放行。".to_owned(),
            );
        }
        return (
            CompatStatus::Unsupported,
            false,
            can_restore,
            "官方文件完整性校验未通过，拒绝修改。".to_owned(),
        );
    }

    (
        CompatStatus::Unsupported,
        false,
        can_restore,
        "当前 OPPO 互联版本或 app.asar 哈希不在白名单中，不支持适配（安全拒绝修改）。".to_owned(),
    )
}

pub fn detect_with_paths(
    exe_opt: Option<&Path>,
    asar_override: Option<&Path>,
    record_path: &Path,
) -> DetectReport {
    let Some(exe) = exe_opt.map(Path::to_path_buf).or_else(crate::oppo_bridge::locate_exe) else {
        return DetectReport {
            status: CompatStatus::NotInstalled,
            detail: "未检测到已安装的 O+ Connect（OPPO 互联）。".to_owned(),
            ..Default::default()
        };
    };

    let asar = asar_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| exe.parent().unwrap_or_else(|| Path::new(".")).join("resources").join("app.asar"));
    if !asar.is_file() {
        return DetectReport {
            status: CompatStatus::NotInstalled,
            exe_path: Some(exe),
            asar_path: Some(asar),
            detail: "已找到 O+Connect.exe，但缺少 resources\\app.asar。".to_owned(),
            ..Default::default()
        };
    }

    let version = read_pe_file_version(&exe).unwrap_or_default();
    let bak = PathBuf::from(format!("{}.bak", asar.display()));
    let backup_exists = bak.is_file();
    let backup_integrity_ok = if backup_exists {
        patch::verify_asar_integrity(&bak).unwrap_or(false)
    } else {
        false
    };

    let current_sha256 = match manifest::sha256_file(&asar) {
        Ok(s) => s,
        Err(e) => {
            return DetectReport {
                status: CompatStatus::Unsupported,
                exe_path: Some(exe),
                asar_path: Some(asar),
                backup_path: Some(bak),
                backup_exists,
                version,
                detail: e,
                ..Default::default()
            };
        }
    };
    let integrity_ok = patch::verify_asar_integrity(&asar).unwrap_or(false);
    let rec = manifest::load_record_at(record_path);
    let (status, can_patch, can_restore, detail) = classify_state(
        &current_sha256,
        integrity_ok,
        backup_exists,
        backup_integrity_ok,
        rec.as_ref(),
    );

    // 若本机已处于白名单已适配态且尚无记录文件，补写适配记录以供日后 O+ 升级检测比对（绝不修改 app.asar 或 .bak）。
    if status == CompatStatus::Adapted {
        let need_write = rec
            .as_ref()
            .is_none_or(|r| !r.patched_sha256.eq_ignore_ascii_case(&current_sha256));
        if need_write {
            let entry = manifest::whitelist_entry_for_patched(&current_sha256);
            let orig_sha = if backup_exists {
                manifest::sha256_file(&bak).unwrap_or_else(|_| {
                    entry.map(|e| e.orig_sha256.to_owned()).unwrap_or_default()
                })
            } else {
                entry.map(|e| e.orig_sha256.to_owned()).unwrap_or_default()
            };
            let new_rec = CompatRecord {
                oplus_exe: exe.to_string_lossy().into_owned(),
                oplus_version: if version.is_empty() {
                    entry.map(|e| e.version.to_owned()).unwrap_or_default()
                } else {
                    version.clone()
                },
                asar_path: asar.to_string_lossy().into_owned(),
                backup_path: bak.to_string_lossy().into_owned(),
                original_sha256: orig_sha,
                patched_sha256: current_sha256.clone(),
                adapted_at_unix: manifest::unix_now(),
            };
            let _ = manifest::save_record_at(record_path, &new_rec);
        }
    }

    DetectReport {
        status,
        exe_path: Some(exe),
        asar_path: Some(asar),
        backup_path: Some(bak),
        backup_exists,
        version,
        current_sha256,
        can_patch,
        can_restore,
        detail,
    }
}

pub fn detect_now() -> DetectReport {
    detect_with_paths(None, None, &manifest::record_path())
}
