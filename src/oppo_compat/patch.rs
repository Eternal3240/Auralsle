//! 纯 Rust ASAR 解析、完整性校验、原地保留键序的补丁写入与失败自动回滚。

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::detect::{self, CompatStatus, DetectReport};
use super::manifest::{self, CompatRecord};

const MARKER: &[u8] = b"createNotification different users return";
const HAS_OWN: &[u8] = b"hasOwnProperty(\"userIdentifier\")";
const CMP: &[u8] = b"!=e.userIdentifier";

#[derive(Debug, Clone)]
pub struct PatchResult {
    pub patched_sha256: String,
    pub hit_expr: String,
    pub replaced_with: String,
}

#[derive(Debug, Clone)]
pub struct AdaptFailure {
    pub report: DetectReport,
    pub error: String,
}

struct ParsedAsar {
    header_start: u64,
    data_start: u64,
    header_raw: Vec<u8>,
    bg_offset: u64,
    bg_size: u64,
    block_size: usize,
    old_hash: String,
    old_blocks: Vec<String>,
}

fn is_js_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

fn find_bytes(haystack: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    if needle.is_empty() || end <= start || end - start < needle.len() || end > haystack.len() {
        return None;
    }
    haystack[start..end]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|rel| start + rel)
}

fn rfind_bytes(haystack: &[u8], needle: &[u8], end: usize) -> Option<usize> {
    if needle.is_empty() || end < needle.len() || end > haystack.len() {
        return None;
    }
    haystack[..end]
        .windows(needle.len())
        .rposition(|w| w == needle)
}

/// 严格按 Electron ASAR JSON 边界与 4 字节对齐规则解析 header，并读取 `files.background.js` 元数据。
fn parse_asar(path: &Path) -> Result<ParsedAsar, String> {
    let mut f =
        std::fs::File::open(path).map_err(|e| format!("打开 {} 失败: {e}", path.display()))?;
    let file_len = f
        .metadata()
        .map_err(|e| format!("读取文件大小失败 {}: {e}", path.display()))?
        .len();
    let head_cap = (file_len as usize).min(4 * 1024 * 1024);
    let mut head = vec![0u8; head_cap];
    let n = f
        .read(&mut head)
        .map_err(|e| format!("读取 ASAR 头失败: {e}"))?;
    head.truncate(n);

    let start = head
        .iter()
        .position(|&b| b == b'{')
        .ok_or_else(|| "app.asar 里找不到 JSON header".to_owned())?;

    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut end = None;
    for (i, &c) in head.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    let header_end = end.ok_or_else(|| "app.asar JSON header 不完整".to_owned())?;
    let header_raw = head[start..header_end].to_vec();
    let data_start = ((header_end as u64) + 3) & !3;

    let root: Value =
        serde_json::from_slice(&header_raw).map_err(|e| format!("解析 ASAR JSON 失败: {e}"))?;
    let bg = root
        .get("files")
        .and_then(|v| v.get("background.js"))
        .ok_or_else(|| "app.asar 中缺少 files.background.js".to_owned())?;

    let bg_offset = match bg.get("offset") {
        Some(Value::String(s)) => s
            .parse::<u64>()
            .map_err(|_| "background.js offset 无效".to_owned())?,
        Some(Value::Number(n)) => n
            .as_u64()
            .ok_or_else(|| "background.js offset 无效".to_owned())?,
        _ => return Err("background.js 缺少 offset".to_owned()),
    };
    let bg_size = bg
        .get("size")
        .and_then(Value::as_u64)
        .ok_or_else(|| "background.js 缺少 size".to_owned())?;

    let integrity = bg
        .get("integrity")
        .ok_or_else(|| "background.js 缺少 integrity 字段".to_owned())?;
    let block_size = integrity
        .get("blockSize")
        .and_then(Value::as_u64)
        .unwrap_or(4 * 1024 * 1024) as usize;
    let old_hash = integrity
        .get("hash")
        .and_then(Value::as_str)
        .ok_or_else(|| "background.js integrity 缺少 hash".to_owned())?
        .to_owned();
    let old_blocks = integrity
        .get("blocks")
        .and_then(Value::as_array)
        .ok_or_else(|| "background.js integrity 缺少 blocks".to_owned())?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect::<Vec<_>>();

    if block_size == 0 || old_hash.len() != 64 || old_blocks.is_empty() {
        return Err("background.js integrity 格式不符合预期".to_owned());
    }

    Ok(ParsedAsar {
        header_start: start as u64,
        data_start,
        header_raw,
        bg_offset,
        bg_size,
        block_size,
        old_hash,
        old_blocks,
    })
}

fn read_bg_bytes(path: &Path, meta: &ParsedAsar) -> Result<Vec<u8>, String> {
    let mut f =
        std::fs::File::open(path).map_err(|e| format!("打开 {} 失败: {e}", path.display()))?;
    f.seek(SeekFrom::Start(meta.data_start + meta.bg_offset))
        .map_err(|e| format!("定位 background.js 失败: {e}"))?;
    let mut data = vec![0u8; meta.bg_size as usize];
    f.read_exact(&mut data)
        .map_err(|e| format!("读取 background.js 内容失败: {e}"))?;
    Ok(data)
}

fn verify_bg_bytes(data: &[u8], meta: &ParsedAsar) -> bool {
    if manifest::sha256_bytes_lower(data) != meta.old_hash {
        return false;
    }
    let chunks: Vec<String> = data
        .chunks(meta.block_size)
        .map(manifest::sha256_bytes_lower)
        .collect();
    chunks == meta.old_blocks
}

/// 校验 `app.asar` 中 `files.background.js` 的 SHA256 与分块完整性。
pub fn verify_asar_integrity(path: &Path) -> Result<bool, String> {
    let meta = parse_asar(path)?;
    let data = read_bg_bytes(path, &meta)?;
    Ok(verify_bg_bytes(&data, &meta))
}

/// 在 `background.js` 字节中查找分身拦截判断（形如 `z!=`，紧跟 `e.userIdentifier`），若已打补丁则返回 `Ok(None)`。
fn find_user_identifier_filter(data: &[u8]) -> Result<Option<(usize, usize)>, String> {
    let Some(p) = find_bytes(data, MARKER, 0, data.len()) else {
        return Err("找不到 createNotification different users return（O+ 版本可能已变化）".into());
    };
    if find_bytes(data, MARKER, p + 1, data.len()).is_some() {
        return Err("createNotification different users return 不唯一，拒绝修改".into());
    }
    let Some(anchor) = rfind_bytes(data, HAS_OWN, p) else {
        return Err("找不到 hasOwnProperty(userIdentifier) 判断".into());
    };
    let Some(cmp_pos) = find_bytes(data, CMP, anchor, p) else {
        return Ok(None);
    };
    let mut s = cmp_pos;
    while s > 0 && is_js_ident(data[s - 1]) {
        s -= 1;
    }
    let e = cmp_pos + 2; // 只替换 `<ident>!=`，保留后续 `e.userIdentifier` 以保持与官方 `0&&e.userIdentifier` 补丁完全一致
    if e <= s || (e - s) < 3 {
        return Err("判断表达式过短，拒绝修改".into());
    }
    Ok(Some((s, e)))
}

/// 定位 JSON header 中 `"background.js":{...}` 的精确切片区间，仅原地替换 64 字节小写 SHA256 hex，
/// 保持原有键序与字节长度一字不差（避免 `BTreeMap` 重排键序改变整个 ASAR 的 SHA256）。
fn patch_header_hashes_in_place(
    header_raw: &[u8],
    old_hash: &str,
    new_hash: &str,
    old_blocks: &[String],
    new_blocks: &[String],
) -> Result<Vec<u8>, String> {
    if old_hash.len() != 64 || new_hash.len() != 64 || old_blocks.len() != new_blocks.len() {
        return Err("完整性哈希长度不一致，拒绝修改".into());
    }
    let key_pos = find_bytes(header_raw, b"\"background.js\":", 0, header_raw.len())
        .ok_or_else(|| "header 中找不到 background.js 键".to_owned())?;
    let obj_start = find_bytes(header_raw, b"{", key_pos, header_raw.len())
        .ok_or_else(|| "background.js 缺少对象起始花括号".to_owned())?;

    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut obj_end = None;
    for (i, &c) in header_raw.iter().enumerate().skip(obj_start) {
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        obj_end = Some(i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    let obj_end = obj_end.ok_or_else(|| "background.js JSON 对象未闭合".to_owned())?;

    let mut out = header_raw.to_vec();
    let replace_64 = |buf: &mut [u8], span_s: usize, span_e: usize, old_s: &str, new_s: &str| -> Result<(), String> {
        let mut quoted_old = Vec::with_capacity(66);
        quoted_old.push(b'"');
        quoted_old.extend_from_slice(old_s.as_bytes());
        quoted_old.push(b'"');
        let pos = find_bytes(buf, &quoted_old, span_s, span_e)
            .ok_or_else(|| format!("在 background.js header 中找不到哈希 {}", old_s))?;
        buf[pos + 1..pos + 65].copy_from_slice(new_s.as_bytes());
        Ok(())
    };

    replace_64(&mut out, obj_start, obj_end, old_hash, new_hash)?;
    for (ob, nb) in old_blocks.iter().zip(new_blocks.iter()) {
        if ob.len() != 64 || nb.len() != 64 {
            return Err("块哈希长度不是 64 字节".into());
        }
        replace_64(&mut out, obj_start, obj_end, ob, nb)?;
    }

    if out.len() != header_raw.len() {
        return Err("header 长度发生意外变化".into());
    }
    Ok(out)
}

/// 将 `src` 打补丁输出到临时文件 `tmp` 并校验完整性。
pub fn patch_asar_to_temp(src: &Path, tmp: &Path) -> Result<PatchResult, String> {
    let meta = parse_asar(src)?;
    let orig_bg = read_bg_bytes(src, &meta)?;
    if !verify_bg_bytes(&orig_bg, &meta) {
        return Err("补丁前 app.asar 完整性校验失败：文件可能已被改过或版本不匹配".into());
    }

    let Some((s, e)) = find_user_identifier_filter(&orig_bg)? else {
        return Err("当前文件已包含分身放行补丁，无需重复打补丁".into());
    };
    let hit_expr = String::from_utf8_lossy(&orig_bg[s..e]).to_string();
    let span_len = e - s;
    let mut repl = vec![b' '; span_len];
    repl[span_len - 3..span_len].copy_from_slice(b"0&&");
    let replaced_with = String::from_utf8_lossy(&repl).to_string();

    let mut new_bg = orig_bg;
    new_bg[s..e].copy_from_slice(&repl);

    let new_hash = manifest::sha256_bytes_lower(&new_bg);
    let new_blocks: Vec<String> = new_bg
        .chunks(meta.block_size)
        .map(manifest::sha256_bytes_lower)
        .collect();
    let new_header = patch_header_hashes_in_place(
        &meta.header_raw,
        &meta.old_hash,
        &new_hash,
        &meta.old_blocks,
        &new_blocks,
    )?;

    std::fs::copy(src, tmp).map_err(|e| format!("复制临时文件失败: {e}"))?;
    {
        let mut tf = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(tmp)
            .map_err(|e| format!("打开临时文件写入失败: {e}"))?;
        tf.seek(SeekFrom::Start(meta.header_start))
            .map_err(|e| format!("定位 header 失败: {e}"))?;
        tf.write_all(&new_header)
            .map_err(|e| format!("写入新 header 失败: {e}"))?;
        tf.seek(SeekFrom::Start(meta.data_start + meta.bg_offset))
            .map_err(|e| format!("定位 background.js 失败: {e}"))?;
        tf.write_all(&new_bg)
            .map_err(|e| format!("写入新 background.js 失败: {e}"))?;
        tf.sync_all().map_err(|e| format!("刷盘失败: {e}"))?;
    }

    if !verify_asar_integrity(tmp)? {
        let _ = std::fs::remove_file(tmp);
        return Err("补丁后临时文件 ASAR 完整性复验失败".into());
    }
    let post_meta = parse_asar(tmp)?;
    let post_bg = read_bg_bytes(tmp, &post_meta)?;
    if find_user_identifier_filter(&post_bg)?.is_some() {
        let _ = std::fs::remove_file(tmp);
        return Err("补丁复验未通过：分身过滤判断仍存在".into());
    }
    let patched_sha256 = manifest::sha256_file(tmp)?;

    Ok(PatchResult {
        patched_sha256,
        hit_expr,
        replaced_with,
    })
}

pub(crate) fn probe_dir_writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(".auralsle_write_test_{}.tmp", std::process::id()));
    match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe)
    {
        Ok(f) => {
            drop(f);
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Err(format!(
            "没有管理员权限，无法写入 OPPO 互联目录（{}）。请以管理员身份运行灵屿后重试。",
            dir.display()
        )),
        Err(e) => Err(format!("无法写入目录 {}: {e}", dir.display())),
    }
}

pub(crate) fn kill_oconnect_and_wait() {
    crate::oppo_bridge::kill_all_oconnect();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        const NO_WINDOW: u32 = 0x0800_0000;
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        let tasklist = PathBuf::from(root).join("System32").join("tasklist.exe");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            let out = Command::new(&tasklist)
                .args(["/FI", "IMAGENAME eq O+Connect.exe", "/FO", "CSV", "/NH"])
                .creation_flags(NO_WINDOW)
                .output();
            let alive = out.is_ok_and(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .to_ascii_lowercase()
                    .contains("o+connect.exe")
            });
            if !alive {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
}

pub(crate) fn restart_oconnect_silent(exe_path: &Path) -> Result<(), String> {
    let token = crate::oppo_bridge::load_or_create_token();
    let mut cmd = std::process::Command::new(exe_path);
    cmd.arg("--openAsHidden")
        .env(crate::oppo_bridge::TOKEN_ENV, token)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(NO_WINDOW);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("重启 OPPO 互联失败: {e}"))
}

/// 原子替换目标文件，若替换阶段任意一步出错，自动还原到原文件或 `.bak`。
pub(crate) fn atomic_swap_file(target: &Path, new_tmp: &Path, fallback_bak: &Path) -> Result<(), String> {
    let dir = target
        .parent()
        .ok_or_else(|| "目标路径缺少父目录".to_owned())?;
    let swap = dir.join("app.asar.auralsle.swap");
    let _ = std::fs::remove_file(&swap);

    if let Err(e) = std::fs::rename(target, &swap) {
        return Err(format!("备份暂存旧 app.asar 失败（{}）: {e}", target.display()));
    }
    if let Err(e) = std::fs::rename(new_tmp, target) {
        // 自动回滚到暂存原文件或 .bak
        if std::fs::rename(&swap, target).is_err() && fallback_bak.is_file() {
            let _ = std::fs::copy(fallback_bak, target);
        }
        let _ = std::fs::remove_file(new_tmp);
        return Err(format!("原子写入新 app.asar 失败，已自动回滚: {e}"));
    }
    let _ = std::fs::remove_file(&swap);
    Ok(())
}

/// 一键适配入口：白名单校验 -> 权限校验 -> 关闭所有 O+ -> 备份 `.bak`（不覆盖已有）-> 临时打补丁并校验 -> 原子替换 -> 静默重启 O+ -> 失败自动回滚。
pub fn apply_adapt(exe_path: &Path, asar_path: &Path) -> Result<DetectReport, AdaptFailure> {
    let make_err = |msg: String| -> AdaptFailure {
        let mut rep = detect::detect_with_paths(Some(exe_path), Some(asar_path), &manifest::record_path());
        rep.status = CompatStatus::FailedRolledBack;
        rep.detail = msg.clone();
        AdaptFailure {
            report: rep,
            error: msg,
        }
    };

    let current_sha = manifest::sha256_file(asar_path).map_err(&make_err)?;
    let Some(entry) = manifest::whitelist_entry_for_original(&current_sha) else {
        let rep = detect::detect_with_paths(Some(exe_path), Some(asar_path), &manifest::record_path());
        let err = if manifest::is_known_patched(&current_sha) {
            "当前 OPPO 互联已完成适配，无需重复操作。".to_owned()
        } else {
            format!("当前版本不在支持白名单内（SHA256: {}），已拒绝修改。", current_sha)
        };
        return Err(AdaptFailure { report: rep, error: err });
    };

    let res_dir = asar_path
        .parent()
        .ok_or_else(|| make_err("无效的 app.asar 路径".to_owned()))?;
    probe_dir_writable(res_dir).map_err(&make_err)?;

    kill_oconnect_and_wait();

    let bak_path = PathBuf::from(format!("{}.bak", asar_path.display()));
    if !bak_path.exists() {
        if let Err(e) = std::fs::copy(asar_path, &bak_path) {
            let _ = restart_oconnect_silent(exe_path);
            return Err(make_err(format!("创建官方备份 app.asar.bak 失败: {e}")));
        }
    }

    let tmp_path = res_dir.join("app.asar.auralsle.tmp");
    let _ = std::fs::remove_file(&tmp_path);

    let patched = match patch_asar_to_temp(asar_path, &tmp_path) {
        Ok(p) => p,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            let _ = restart_oconnect_silent(exe_path);
            return Err(make_err(e));
        }
    };

    if patched.patched_sha256 != entry.patched_sha256 {
        let _ = std::fs::remove_file(&tmp_path);
        let _ = restart_oconnect_silent(exe_path);
        return Err(make_err(format!(
            "补丁后校验和与白名单不匹配（期望 {}，实际 {}），已中止并保持官方原文件。",
            entry.patched_sha256, patched.patched_sha256
        )));
    }

    if let Err(e) = atomic_swap_file(asar_path, &tmp_path, &bak_path) {
        let _ = std::fs::remove_file(&tmp_path);
        let _ = restart_oconnect_silent(exe_path);
        return Err(make_err(e));
    }

    let version = detect::read_pe_file_version(exe_path)
        .unwrap_or_else(|| entry.version.to_owned());
    let rec = CompatRecord {
        oplus_exe: exe_path.to_string_lossy().into_owned(),
        oplus_version: version,
        asar_path: asar_path.to_string_lossy().into_owned(),
        backup_path: bak_path.to_string_lossy().into_owned(),
        original_sha256: current_sha,
        patched_sha256: patched.patched_sha256,
        adapted_at_unix: manifest::unix_now(),
    };
    if let Err(e) = manifest::save_record(&rec) {
        // 回滚还原官方备份
        let _ = super::restore::restore_asar_from_backup(asar_path, &bak_path);
        let _ = restart_oconnect_silent(exe_path);
        return Err(make_err(format!("写入适配记录失败，已自动回滚: {e}")));
    }

    if let Err(e) = restart_oconnect_silent(exe_path) {
        return Err(AdaptFailure {
            report: detect::detect_with_paths(Some(exe_path), Some(asar_path), &manifest::record_path()),
            error: format!("补丁已完成，但静默重启 OPPO 互联失败: {e}"),
        });
    }

    Ok(detect::detect_with_paths(
        Some(exe_path),
        Some(asar_path),
        &manifest::record_path(),
    ))
}
