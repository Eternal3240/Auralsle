//! OPPO 互联（O+ Connect）兼容白名单、哈希计算与适配记录持久化。

use ring::digest::{Context, SHA256};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

/// 支持一键适配的官方原始 `app.asar` SHA256
pub const ORIG_ASAR_SHA256: &str =
    "A551993DC965FCF52082C2EEA970659A27361F92855812FCE09997EBF77CF3B2";

/// 已成功打过分身放行补丁的 `app.asar` SHA256
pub const PATCHED_ASAR_SHA256: &str =
    "C79D8E533878AF0D44EC5777EB281AC778BA6784F8FC4F3AD5584B2BAA4CB42D";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhitelistEntry {
    pub version: &'static str,
    pub orig_sha256: &'static str,
    pub patched_sha256: &'static str,
}

pub const WHITELIST: &[WhitelistEntry] = &[WhitelistEntry {
    version: "17.20.3.0",
    orig_sha256: ORIG_ASAR_SHA256,
    patched_sha256: PATCHED_ASAR_SHA256,
}];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatRecord {
    pub oplus_exe: String,
    pub oplus_version: String,
    pub asar_path: String,
    pub backup_path: String,
    pub original_sha256: String,
    pub patched_sha256: String,
    pub adapted_at_unix: u64,
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn record_path() -> PathBuf {
    let base = std::env::var("APPDATA")
        .or_else(|_| std::env::var("LOCALAPPDATA"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("Auralsle").join("oppo-compat.json")
}

pub fn load_record_at(path: &Path) -> Option<CompatRecord> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn load_record() -> Option<CompatRecord> {
    load_record_at(&record_path())
}

pub fn save_record_at(path: &Path, rec: &CompatRecord) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(rec).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub fn save_record(rec: &CompatRecord) -> Result<(), String> {
    save_record_at(&record_path(), rec)
}

pub fn remove_record_at(path: &Path) {
    let _ = std::fs::remove_file(path);
}

pub fn remove_record() {
    remove_record_at(&record_path());
}

pub fn is_known_original(sha256: &str) -> bool {
    let u = sha256.trim().to_ascii_uppercase();
    WHITELIST.iter().any(|e| e.orig_sha256 == u)
}

pub fn is_known_patched(sha256: &str) -> bool {
    let u = sha256.trim().to_ascii_uppercase();
    WHITELIST.iter().any(|e| e.patched_sha256 == u)
}

pub fn whitelist_entry_for_original(sha256: &str) -> Option<&'static WhitelistEntry> {
    let u = sha256.trim().to_ascii_uppercase();
    WHITELIST.iter().find(|e| e.orig_sha256 == u)
}

pub fn whitelist_entry_for_patched(sha256: &str) -> Option<&'static WhitelistEntry> {
    let u = sha256.trim().to_ascii_uppercase();
    WHITELIST.iter().find(|e| e.patched_sha256 == u)
}

pub fn sha256_bytes_lower(data: &[u8]) -> String {
    let digest = ring::digest::digest(&SHA256, data);
    hex_lower(digest.as_ref())
}

pub fn sha256_bytes_upper(data: &[u8]) -> String {
    let digest = ring::digest::digest(&SHA256, data);
    hex_upper(digest.as_ref())
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path)
        .map_err(|e| format!("无法打开文件 {}: {e}", path.display()))?;
    let mut ctx = Context::new(&SHA256);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("读取文件失败 {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    Ok(hex_upper(ctx.finish().as_ref()))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

fn hex_upper(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut s, "{:02X}", b);
    }
    s
}
