//! 从 `app.asar.bak` 原子恢复官方文件，并静默重启 OPPO 互联。

use std::path::{Path, PathBuf};

use super::detect::{self, DetectReport};
use super::manifest;
use super::patch;

/// 从 `backup_path` 校验并原子还原到 `asar_path`（保留 `backup_path` 不删除）。
pub fn restore_asar_from_backup(asar_path: &Path, backup_path: &Path) -> Result<String, String> {
    if !backup_path.is_file() {
        return Err(format!("找不到官方备份文件: {}", backup_path.display()));
    }
    if !patch::verify_asar_integrity(backup_path)? {
        return Err("备份文件 app.asar.bak 完整性校验未通过，已停止恢复。".to_owned());
    }
    let bak_sha = manifest::sha256_file(backup_path)?;
    let res_dir = asar_path
        .parent()
        .ok_or_else(|| "app.asar 路径无效".to_owned())?;
    let tmp = res_dir.join("app.asar.auralsle.restore.tmp");
    let _ = std::fs::remove_file(&tmp);
    std::fs::copy(backup_path, &tmp)
        .map_err(|e| format!("复制官方备份到临时文件失败: {e}"))?;
    let tmp_sha = manifest::sha256_file(&tmp)?;
    if tmp_sha != bak_sha {
        let _ = std::fs::remove_file(&tmp);
        return Err("恢复临时文件校验和不一致，已取消替换。".to_owned());
    }
    patch::atomic_swap_file(asar_path, &tmp, backup_path)?;
    Ok(bak_sha)
}

/// 恢复官方文件入口：权限检查 -> 关闭 O+ -> 校验并还原 `app.asar.bak` -> 删除适配记录 -> 静默重启 O+。
pub fn restore_official(exe_path: &Path, asar_path: &Path) -> Result<DetectReport, String> {
    let res_dir = asar_path
        .parent()
        .ok_or_else(|| "无效的 app.asar 路径".to_owned())?;
    patch::probe_dir_writable(res_dir)?;
    let bak = PathBuf::from(format!("{}.bak", asar_path.display()));
    if !bak.is_file() {
        return Err("未找到 app.asar.bak，无法恢复官方文件。".to_owned());
    }

    patch::kill_oconnect_and_wait();
    if let Err(e) = restore_asar_from_backup(asar_path, &bak) {
        let _ = patch::restart_oconnect_silent(exe_path);
        return Err(e);
    }
    manifest::remove_record();
    patch::restart_oconnect_silent(exe_path)?;
    Ok(detect::detect_with_paths(
        Some(exe_path),
        Some(asar_path),
        &manifest::record_path(),
    ))
}
