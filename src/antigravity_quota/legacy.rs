//! Gemini Antigravity（Cloud Code Assist / Antigravity CLI）AI 额度服务。
//! 仅只读读取本机已有 OAuth 凭据或官方 CLI `statusLine` 导出的用量记录，不回写原客户端文件，
//! 也不扫描运行中进程命令行；手动 Access Token 使用 Windows DPAPI 加密存储在独立凭据目录。
use super::super::{Account, Config, Provider, Snapshot, Window};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const CLOUD_CODE_BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const DAILY_CLOUD_CODE_BASE: &str = "https://daily-cloudcode-pa.googleapis.com/v1internal";
const OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CLI_USAGE: &str = "auralsle-usage.json";
const CLI_COLLECTOR: &str = "auralsle-statusline.ps1";
const MAX_BODY: u64 = 2 * 1024 * 1024;
const LOGIN_ERROR: &str = "Antigravity 登录已过期，请在 Antigravity / Gemini CLI 或 Chrome 重新登录后刷新";
const PARSE_ERROR: &str = "Antigravity 额度数据格式暂不可读，请稍后重试";

fn sha256_hex(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

pub(crate) fn has_desktop_install() -> bool {
    let mut dirs = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|s| !s.is_empty()) {
        let base = PathBuf::from(local);
        dirs.push(base.join("Programs").join("antigravity").join("Antigravity.exe"));
        dirs.push(base.join("antigravity").join("Antigravity.exe"));
    }
    if let Some(pf) = std::env::var_os("ProgramFiles").filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(pf).join("Antigravity").join("Antigravity.exe"));
    }
    dirs.into_iter().any(|p| p.is_file())
}

pub(crate) fn has_agy_cli() -> bool {
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            for bin in ["agy.exe", "agy.cmd", "agy"] {
                if dir.join(bin).is_file() {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn default_oauth_paths() -> Vec<PathBuf> {
    let mut raw = Vec::new();
    if let Some(cli_home) = std::env::var_os("GEMINI_CLI_HOME").filter(|s| !s.is_empty()) {
        let base = PathBuf::from(cli_home);
        raw.push(base.join("oauth_creds.json"));
        raw.push(base.join(".gemini").join("oauth_creds.json"));
    }
    let home = home_dir();
    if !home.as_os_str().is_empty() {
        raw.push(home.join(".gemini").join("antigravity-cli").join("oauth_creds.json"));
        raw.push(home.join(".gemini").join("oauth_creds.json"));
        raw.push(home.join(".gemini").join("antigravity").join("oauth_creds.json"));
        raw.push(home.join(".antigravity").join("oauth_creds.json"));
        raw.push(home.join(".config").join("gcloud").join("application_default_credentials.json"));
    }
    if let Some(appdata) = std::env::var_os("APPDATA").filter(|s| !s.is_empty()) {
        let ad = PathBuf::from(appdata);
        raw.push(ad.join("Antigravity").join("oauth_creds.json"));
        raw.push(ad.join("gcloud").join("application_default_credentials.json"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|s| !s.is_empty()) {
        raw.push(PathBuf::from(local).join("Antigravity").join("oauth_creds.json"));
    }
    let mut out = Vec::new();
    for p in raw {
        if !p.is_file() {
            continue;
        }
        let canon = fs::canonicalize(&p).unwrap_or(p);
        if !out.contains(&canon) {
            out.push(canon);
        }
    }
    out
}

pub(crate) fn cli_usage_path() -> PathBuf {
    home_dir().join(".gemini").join("antigravity-cli").join(CLI_USAGE)
}

pub(crate) fn is_cli_usage_path(path: &Path) -> bool {
    path.file_name().and_then(|s| s.to_str()) == Some(CLI_USAGE)
}

fn read_json_file(path: &Path) -> Result<Value, String> {
    let raw = fs::read(path).map_err(|_| "无法读取 Antigravity 文件，请检查文件是否存在".to_string())?;
    serde_json::from_slice(raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&raw))
        .map_err(|_| "所选文件不是有效的 JSON 格式".into())
}

fn validate_token(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.len() > 65536
        || !trimmed.is_ascii()
        || trimmed.bytes().any(|b| b <= 32 || b == 127)
    {
        return Err("无效的 Antigravity Access Token".into());
    }
    Ok(trimmed.to_owned())
}

fn extract_id_identity(jwt: &str) -> Option<String> {
    let payload_b64 = jwt.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;
    let email = claims["email"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 320);
    let sub = claims["sub"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 320);
    email.or(sub).map(|s| format!("ag-user:{}", sha256_hex(s.as_bytes())))
}

struct FileOAuth {
    access_token: Option<String>,
    refresh_token: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    project: Option<String>,
    user_slot: Option<String>,
}

fn has_oauth_token_field(root: &Value) -> bool {
    root.get("access_token").is_some()
        || root.get("accessToken").is_some()
        || root.get("refresh_token").is_some()
        || root.get("refreshToken").is_some()
        || root.pointer("/tokens/access_token").is_some()
        || root.pointer("/token/access_token").is_some()
        || root.pointer("/tokens/refresh_token").is_some()
        || root.pointer("/token/refresh_token").is_some()
}

fn non_empty_str<'a>(v: Option<&'a Value>) -> Option<&'a str> {
    v.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn extract_oauth_from_value(root: &Value) -> Result<FileOAuth, String> {
    let access_token = root
        .get("access_token")
        .or_else(|| root.get("accessToken"))
        .or_else(|| root.pointer("/tokens/access_token"))
        .or_else(|| root.pointer("/token/access_token"))
        .and_then(Value::as_str)
        .map(validate_token)
        .transpose()?;
    let refresh_token = non_empty_str(
        root.get("refresh_token")
            .or_else(|| root.get("refreshToken"))
            .or_else(|| root.pointer("/tokens/refresh_token"))
            .or_else(|| root.pointer("/token/refresh_token")),
    )
    .map(str::to_owned);
    let client_id = non_empty_str(
        root.get("client_id")
            .or_else(|| root.get("clientId"))
            .or_else(|| root.pointer("/installed/client_id"))
            .or_else(|| root.pointer("/web/client_id")),
    )
    .map(str::to_owned);
    let client_secret = non_empty_str(
        root.get("client_secret")
            .or_else(|| root.get("clientSecret"))
            .or_else(|| root.pointer("/installed/client_secret"))
            .or_else(|| root.pointer("/web/client_secret")),
    )
    .map(str::to_owned);
    if access_token.is_none() && (refresh_token.is_none() || client_id.is_none()) {
        return Err("未在凭据文件中找到有效的 access_token 或可刷新凭据（refresh_token + client_id）".into());
    }
    let project = [
        "quota_project_id",
        "project_id",
        "projectId",
        "cloudaicompanionProject",
    ]
    .into_iter()
    .find_map(|k| non_empty_str(root.get(k)))
    .filter(|s| s.len() <= 256)
    .map(str::to_owned);
    let user_slot = root
        .get("id_token")
        .or_else(|| root.get("idToken"))
        .or_else(|| root.pointer("/tokens/id_token"))
        .and_then(Value::as_str)
        .and_then(extract_id_identity);
    Ok(FileOAuth {
        access_token,
        refresh_token,
        client_id,
        client_secret,
        project,
        user_slot,
    })
}

fn exchange_refresh_token(
    agent: &ureq::Agent,
    token_url: &str,
    oauth: &FileOAuth,
) -> Result<String, (String, u64)> {
    let rt = oauth
        .refresh_token
        .as_deref()
        .ok_or_else(|| (LOGIN_ERROR.to_owned(), 0))?;
    let cid = oauth
        .client_id
        .as_deref()
        .ok_or_else(|| (LOGIN_ERROR.to_owned(), 0))?;
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    ser.append_pair("grant_type", "refresh_token");
    ser.append_pair("refresh_token", rt);
    ser.append_pair("client_id", cid);
    if let Some(sec) = oauth.client_secret.as_deref().filter(|s| !s.is_empty()) {
        ser.append_pair("client_secret", sec);
    }
    let form = ser.finish();
    let resp = agent
        .post(token_url)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .set("Accept", "application/json")
        .set("User-Agent", concat!("Auralsle/", env!("CARGO_PKG_VERSION")))
        .send_string(&form)
        .map_err(|e| match e {
            ureq::Error::Status(400 | 401 | 403, _) => (LOGIN_ERROR.to_owned(), 0),
            other => status_error(other),
        })?;
    let val = read_json_response(resp)?;
    let tok = val["access_token"]
        .as_str()
        .ok_or_else(|| (LOGIN_ERROR.to_owned(), 0))?;
    validate_token(tok).map_err(|m| (m, 0))
}

fn token_slot_from_token(token: &str, owner: Option<&str>) -> String {
    let seed = owner.filter(|s| !s.trim().is_empty()).unwrap_or(token);
    format!("ag-token:{}", sha256_hex(seed.as_bytes()))
}

fn file_slot(user_slot: Option<&str>, project: Option<&str>, path: &Path) -> String {
    if let Some(u) = user_slot {
        return u.to_owned();
    }
    if let Some(p) = project.filter(|p| !p.trim().is_empty()) {
        return format!("ag-proj:{}", sha256_hex(p.trim().as_bytes()));
    }
    let norm = fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_ascii_lowercase();
    format!("ag-file:{}", sha256_hex(norm.as_bytes()))
}

fn session_hex(slot: &str) -> Result<&str, String> {
    let hex = slot
        .strip_prefix("ag-token:")
        .ok_or_else(|| "Antigravity 安全凭据标识无效，请重新连接".to_string())?;
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Antigravity 安全凭据标识无效，请重新连接".into());
    }
    Ok(hex)
}

fn session_path(slot: &str) -> Result<PathBuf, String> {
    let hex = session_hex(slot)?;
    Ok(crate::model::config_dir()
        .join("credentials")
        .join(format!("antigravity-{hex}.bin")))
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct TokenSession {
    access_token: String,
    #[serde(default)]
    project: Option<String>,
    slot: String,
}

fn save_token_session(session: &TokenSession) -> Result<(), String> {
    let path = session_path(&session.slot)?;
    let bytes = serde_json::to_vec(session).map_err(|_| "无法序列化 Antigravity 凭据")?;
    let encrypted = super::super::gemini::protect(&bytes, true)?;
    let dir = path.parent().ok_or("无法创建 Antigravity 凭据目录")?;
    fs::create_dir_all(dir).map_err(|_| "无法创建 Antigravity 凭据目录")?;
    crate::model::atomic_write(&path, &encrypted).map_err(|_| "无法保存 Antigravity 加密凭据".into())
}

fn load_token_session(account: &Account) -> Result<TokenSession, String> {
    let slot = account.slot.as_deref().ok_or(LOGIN_ERROR)?;
    let path = session_path(slot)?;
    let encrypted = fs::read(path).map_err(|_| LOGIN_ERROR)?;
    let raw = super::super::gemini::protect(&encrypted, false)?;
    let session: TokenSession = serde_json::from_slice(&raw).map_err(|_| LOGIN_ERROR)?;
    if session.slot != slot {
        return Err("Antigravity 凭据账号不匹配，请重新连接".into());
    }
    validate_token(&session.access_token)?;
    Ok(session)
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct WebSession {
    email: String,
    cookie: String,
    slot: String,
}

fn web_session_path(slot: &str) -> Result<PathBuf, String> {
    let hex = slot
        .strip_prefix("ag-web:")
        .ok_or_else(|| "Antigravity Google 会话标识无效，请重新连接".to_string())?;
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Antigravity Google 会话标识无效，请重新连接".into());
    }
    Ok(crate::model::config_dir()
        .join("credentials")
        .join(format!("antigravity-web-{hex}.bin")))
}

fn save_web_session(session: &WebSession) -> Result<(), String> {
    let path = web_session_path(&session.slot)?;
    let bytes = serde_json::to_vec(session).map_err(|_| "无法序列化 Antigravity 会话")?;
    let encrypted = super::super::gemini::protect(&bytes, true)?;
    let dir = path.parent().ok_or("无法创建 Antigravity 凭据目录")?;
    fs::create_dir_all(dir).map_err(|_| "无法创建 Antigravity 凭据目录")?;
    crate::model::atomic_write(&path, &encrypted).map_err(|_| "无法保存 Antigravity 加密会话".into())
}

fn load_web_session(account: &Account) -> Result<WebSession, String> {
    let slot = account.slot.as_deref().ok_or(LOGIN_ERROR)?;
    let path = web_session_path(slot)?;
    let encrypted = fs::read(path).map_err(|_| LOGIN_ERROR)?;
    let raw = super::super::gemini::protect(&encrypted, false)?;
    let session: WebSession = serde_json::from_slice(&raw).map_err(|_| LOGIN_ERROR)?;
    if session.slot != slot {
        return Err("Antigravity Google 会话不匹配，请重新连接".into());
    }
    Ok(session)
}

pub fn forget(account: &Account) {
    if let Some(slot) = account.slot.as_deref() {
        if let Ok(path) = session_path(slot) {
            let _ = fs::remove_file(path);
        }
        if let Ok(path) = web_session_path(slot) {
            let _ = fs::remove_file(path);
        }
    }
}

pub(crate) fn install_cli_collector(usage_path: &Path) -> Result<(), String> {
    let dir = usage_path.parent().ok_or("未找到 Antigravity CLI 配置目录")?;
    let settings_path = dir.join("settings.json");
    let mut settings = if settings_path.is_file() {
        read_json_file(&settings_path)?
    } else {
        fs::create_dir_all(dir).map_err(|_| "无法创建 Antigravity CLI 配置目录")?;
        json!({})
    };
    if !settings.is_object() {
        return Err("Antigravity CLI 的 settings.json 顶层必须是 JSON 对象，请修正后重试".into());
    }
    let script = dir.join(CLI_COLLECTOR);
    let command = format!(
        "powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File \"{}\"",
        script.to_string_lossy().replace('\\', "/")
    );
    if !settings["statusLine"].is_null() && settings["statusLine"]["command"].as_str() != Some(command.as_str()) {
        return Err("Antigravity CLI 已有自定义状态栏配置，已保留；可选择导入已有本地用量 JSON".into());
    }
    let script_content = format!("\u{feff}{}", include_str!("../../scripts/antigravity-statusline.ps1"));
    crate::model::atomic_write(&script, script_content.as_bytes())
        .map_err(|_| "无法写入 Antigravity CLI 状态栏采集脚本".to_string())?;
    if settings["statusLine"]["command"].as_str() == Some(command.as_str()) {
        return Ok(());
    }
    settings["statusLine"] = json!({"type": "command", "command": command, "enabled": true});
    let serialized = serde_json::to_vec_pretty(&settings).map_err(|_| "无法生成 Antigravity CLI 配置")?;
    crate::model::atomic_write(&settings_path, &serialized)
        .map_err(|_| "无法更新 Antigravity CLI 状态栏配置".into())
}

fn rate_limit_error(r: ureq::Response) -> (String, u64) {
    let wait = r
        .header("Retry-After")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(180)
        .clamp(30, 3600);
    ("Antigravity 请求过于频繁，稍后自动重试".into(), wait)
}

fn status_error(error: ureq::Error) -> (String, u64) {
    match error {
        ureq::Error::Status(401 | 403, _) => (LOGIN_ERROR.into(), 0),
        ureq::Error::Status(429, r) => rate_limit_error(r),
        ureq::Error::Status(code, _) => (format!("Antigravity 额度服务暂不可用（{code}）"), 60),
        _ => ("无法连接 Antigravity 额度服务，请检查网络或代理".into(), 30),
    }
}

fn read_json_response(response: ureq::Response) -> Result<Value, (String, u64)> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_BODY + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ("读取 Antigravity 额度响应失败".into(), 30))?;
    if bytes.len() as u64 > MAX_BODY {
        return Err(("Antigravity 额度响应过大".into(), 60));
    }
    serde_json::from_slice(&bytes).map_err(|_| (PARSE_ERROR.into(), 60))
}

fn normalize_tier_name(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    let low = t.to_ascii_lowercase();
    if low.contains("ultra") {
        if low.contains("20x") {
            "AI Ultra 20x".into()
        } else if low.contains("5x") {
            "AI Ultra 5x".into()
        } else {
            "AI Ultra".into()
        }
    } else if low.contains("pro") {
        "AI Pro".into()
    } else if low.contains("free") || low.contains("starter") || low.contains("standard") || low.contains("legacy") {
        "Free".into()
    } else {
        t.to_owned()
    }
}

fn extract_plan_name(v: &Value) -> String {
    for ptr in [
        "/userStatus/userTier/preferredName",
        "/userStatus/userTier/name",
        "/userStatus/planStatus/planInfo/planName",
        "/userTier/preferredName",
        "/userTier/name",
        "/response/userTier",
        "/planDetails/planName",
        "/paidTier/name",
        "/currentTier/name",
        "/plan_tier",
        "/plan",
    ] {
        if let Some(s) = v.pointer(ptr).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
            return normalize_tier_name(s);
        }
    }
    for ptr in ["/paidTier/id", "/currentTier/id", "/userStatus/userTier/id", "/planDetails/tierId"] {
        if let Some(s) = v.pointer(ptr).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
            return normalize_tier_name(s);
        }
    }
    String::new()
}

fn extract_project_id(v: &Value) -> Option<String> {
    for ptr in [
        "/cloudaicompanionProject",
        "/cloudaicompanionProject/id",
        "/project/projectId",
        "/project/id",
        "/project",
    ] {
        if let Some(s) = v.pointer(ptr).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
            return Some(s.to_owned());
        }
    }
    None
}

fn load_code_assist(
    agent: &ureq::Agent,
    base: &str,
    token: &str,
    explicit_project: Option<&str>,
) -> Result<(Option<String>, String), (String, u64)> {
    let url = format!("{base}:loadCodeAssist");
    let mut body = json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });
    if let Some(proj) = explicit_project.map(str::trim).filter(|s| !s.is_empty()) {
        body["cloudaicompanionProject"] = json!(proj);
    }
    let resp = agent
        .post(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .set("Accept", "application/json")
        .set("User-Agent", concat!("Auralsle/", env!("CARGO_PKG_VERSION")))
        .send_string(&body.to_string())
        .map_err(status_error)?;
    let value = read_json_response(resp)?;
    let proj = extract_project_id(&value).or_else(|| explicit_project.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned));
    let plan = extract_plan_name(&value);
    Ok((proj, plan))
}

fn post_cloud_quota(
    agent: &ureq::Agent,
    url: &str,
    token: &str,
    project: Option<&str>,
) -> Result<Value, (Option<u16>, (String, u64))> {
    let body = match project.map(str::trim).filter(|s| !s.is_empty()) {
        Some(p) => json!({ "project": p }),
        None => json!({}),
    };
    let resp = agent
        .post(url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .set("Accept", "application/json")
        .set("User-Agent", concat!("Auralsle/", env!("CARGO_PKG_VERSION")))
        .send_string(&body.to_string())
        .map_err(|e| match &e {
            ureq::Error::Status(code, _) => (Some(*code), status_error(e)),
            _ => (None, status_error(e)),
        })?;
    read_json_response(resp).map_err(|err| (None, err))
}

fn query_cloud_code_base(
    agent: &ureq::Agent,
    base: &str,
    token: &str,
    explicit_project: Option<&str>,
) -> Result<(Option<String>, Snapshot), (bool, (String, u64))> {
    let (project, plan) = match load_code_assist(agent, base, token, explicit_project) {
        Ok(res) => res,
        Err((msg, wait)) if msg == LOGIN_ERROR || msg.contains("频繁") => return Err((true, (msg, wait))),
        Err(_) => (
            explicit_project.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned),
            String::new(),
        ),
    };

    let mut last_err: Option<(String, u64)> = None;
    for suffix in [":retrieveUserQuota", ":fetchAvailableModels", ":retrieveUserQuotaSummary"] {
        let url = format!("{base}{suffix}");
        match post_cloud_quota(agent, &url, token, project.as_deref()) {
            Ok(val) => {
                if let Ok(snap) = parse_with_plan(&val, &plan) {
                    return Ok((project, snap));
                }
            }
            Err((Some(401 | 403 | 429), err)) => return Err((true, err)),
            Err((_, err)) => {
                last_err = Some(err);
            }
        }
    }
    Err((false, last_err.unwrap_or_else(|| (PARSE_ERROR.to_owned(), 60))))
}

fn query_cloud_code_at(
    agent: &ureq::Agent,
    base: &str,
    token: &str,
    explicit_project: Option<&str>,
) -> Result<(Option<String>, Snapshot), (String, u64)> {
    match query_cloud_code_base(agent, base, token, explicit_project) {
        Ok(res) => Ok(res),
        Err((true, err)) => Err(err),
        Err((false, err)) => {
            if base == CLOUD_CODE_BASE {
                if let Ok(res) = query_cloud_code_base(agent, DAILY_CLOUD_CODE_BASE, token, explicit_project) {
                    return Ok(res);
                }
            }
            Err(err)
        }
    }
}

#[derive(Clone)]
struct RawEntry {
    group: String,
    seconds: Option<i64>,
    cadence_tag: Option<&'static str>,
    remaining_pct: f32,
    reset: Option<i64>,
}

fn normalize_group_name(id: &str, display: Option<&str>) -> String {
    let d = display.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");
    let combined = format!("{} {}", id.to_ascii_lowercase(), d.to_ascii_lowercase());
    let has_claude = combined.contains("claude");
    let has_gpt = combined.contains("gpt")
        || combined.contains("openai")
        || combined.contains("codex")
        || combined.split(|c: char| !c.is_ascii_alphanumeric()).any(|tok| matches!(tok, "o3" | "o4"));
    if has_claude && has_gpt {
        return "Claude / GPT".into();
    }
    if has_claude {
        return "Claude".into();
    }
    if has_gpt || combined.contains("3p-") || combined.contains("third-party") || combined.contains("third_party") {
        return if combined.contains("claude") { "Claude / GPT".into() } else { "GPT / 第三方".into() };
    }
    if combined.contains("gemini") {
        if combined.contains("pro") || combined.contains("ultra") || combined.contains("thinking") {
            return "Gemini Pro".into();
        }
        if combined.contains("flash") || combined.contains("lite") {
            return "Gemini Flash".into();
        }
        return "Gemini".into();
    }
    if !d.is_empty() {
        d.to_owned()
    } else if !id.trim().is_empty() {
        id.trim().to_owned()
    } else {
        "模型额度".into()
    }
}

fn cadence_from_hints(item: &Value, raw_id: &str, default_secs: Option<i64>) -> (Option<i64>, Option<&'static str>) {
    for ptr in ["/durationSeconds", "/limitWindowSeconds", "/windowSeconds"] {
        if let Some(s) = item.pointer(ptr).and_then(|v| v.as_i64()).filter(|s| *s > 0) {
            return match s {
                18000 => (Some(18000), Some("5 小时")),
                604800 => (Some(604800), Some("本周")),
                _ => (Some(s), None),
            };
        }
    }
    let text_hint = [
        item["window"].as_str(),
        item["displayName"].as_str(),
        item["windowType"].as_str(),
        item["id"].as_str(),
        Some(raw_id),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();

    if text_hint.contains("week") || text_hint.contains("7d") || text_hint.contains("seven_day") {
        (Some(604800), Some("本周"))
    } else if text_hint.contains("5h")
        || text_hint.contains("5-hour")
        || text_hint.contains("five_hour")
        || text_hint.contains("session")
    {
        (Some(18000), Some("5 小时"))
    } else {
        let tag = match default_secs {
            Some(18000) => Some("5 小时"),
            Some(604800) => Some("本周"),
            _ => None,
        };
        (default_secs, tag)
    }
}

fn parse_reset_time(v: &Value) -> Option<i64> {
    if let Some(n) = super::super::reset(v) {
        return (n > 0 && n <= 4_102_444_800).then_some(n);
    }
    let t = super::super::number(v)?;
    let t = if t >= 100_000_000_000.0 { t / 1000.0 } else { t };
    (t > 0.0 && t <= 4_102_444_800.0).then_some(t as i64)
}

fn parse_remaining_fraction(obj: &Value, allow_omitted_zero: bool) -> Option<(f32, Option<i64>)> {
    let reset = parse_reset_time(&obj["resetTime"])
        .or_else(|| parse_reset_time(&obj["reset_time"]))
        .or_else(|| parse_reset_time(&obj["resetsAt"]))
        .or_else(|| parse_reset_time(&obj["resets_at"]))
        .or_else(|| parse_reset_time(&obj["resetAt"]))
        .or_else(|| parse_reset_time(&obj["reset_at"]))
        .or_else(|| {
            super::super::number(&obj["resetInSeconds"])
                .or_else(|| super::super::number(&obj["reset_in_seconds"]))
                .filter(|s| *s > 0.0 && *s <= 31_536_000.0)
                .map(|s| chrono::Utc::now().timestamp() + s.round() as i64)
        });

    if let Some(frac) = super::super::number(&obj["remainingFraction"])
        .or_else(|| super::super::number(&obj["remaining_fraction"]))
    {
        if !(0.0..=1.0001).contains(&frac) {
            return None;
        }
        return Some(((frac * 100.0).clamp(0.0, 100.0) as f32, reset));
    }
    if let Some(pct) = super::super::number(&obj["remainingPercent"])
        .or_else(|| super::super::number(&obj["remaining_percent"]))
    {
        if !(0.0..=100.01).contains(&pct) {
            return None;
        }
        return Some((pct.clamp(0.0, 100.0) as f32, reset));
    }
    if let Some(used_f) = super::super::number(&obj["usedFraction"])
        .or_else(|| super::super::number(&obj["used_fraction"]))
    {
        if !(0.0..=1.0001).contains(&used_f) {
            return None;
        }
        return Some((((1.0 - used_f) * 100.0).clamp(0.0, 100.0) as f32, reset));
    }
    if let Some(used_p) = super::super::number(&obj["usedPercentage"])
        .or_else(|| super::super::number(&obj["used_percentage"]))
    {
        if !(0.0..=100.01).contains(&used_p) {
            return None;
        }
        return Some(((100.0 - used_p).clamp(0.0, 100.0) as f32, reset));
    }
    if allow_omitted_zero {
        let explicit_zero = obj["remainingAmount"]
            .as_i64()
            .is_some_and(|n| n == 0)
            || obj["remainingAmount"]
                .as_str()
                .is_some_and(|s| s.trim() == "0")
            || obj["exhausted"].as_bool() == Some(true);
        if explicit_zero || reset.is_some() {
            return Some((0.0, reset));
        }
    }
    None
}

fn collect_quota_entries(v: &Value) -> Vec<RawEntry> {
    let mut entries = Vec::new();

    // 1. retrieveUserQuota: buckets[]
    if let Some(buckets) = v["buckets"].as_array() {
        for b in buckets {
            if !b.is_object() {
                continue;
            }
            let id = b["modelId"]
                .as_str()
                .or_else(|| b["id"].as_str())
                .unwrap_or_default();
            let disp = b["displayName"].as_str().or_else(|| b["label"].as_str());
            let allow_omitted = !id.trim().is_empty() || disp.is_some_and(|s| !s.trim().is_empty());
            if let Some((pct, reset)) = parse_remaining_fraction(b, allow_omitted) {
                let (seconds, cadence_tag) = cadence_from_hints(b, id, None);
                entries.push(RawEntry {
                    group: normalize_group_name(id, disp),
                    seconds,
                    cadence_tag,
                    remaining_pct: pct,
                    reset,
                });
            }
        }
    }

    // 2. GetUserStatus / RetrieveUserQuotaSummary: groups[] or response.groups[]
    for grp_ptr in ["/response/groups", "/groups"] {
        if let Some(groups) = v.pointer(grp_ptr).and_then(Value::as_array) {
            for g in groups {
                let g_id = g["id"].as_str().unwrap_or_default();
                let g_name = g["displayName"].as_str();
                for b in g["buckets"].as_array().into_iter().flatten() {
                    if let Some((pct, reset)) = parse_remaining_fraction(b, true) {
                        let b_id = b["id"].as_str().unwrap_or(g_id);
                        let (seconds, cadence_tag) = cadence_from_hints(b, b_id, None);
                        entries.push(RawEntry {
                            group: normalize_group_name(g_id, g_name),
                            seconds,
                            cadence_tag,
                            remaining_pct: pct,
                            reset,
                        });
                    }
                }
            }
        }
    }

    // 3. fetchAvailableModels: models[] or models{}
    match &v["models"] {
        Value::Array(arr) => {
            for m in arr {
                let id = m["name"]
                    .as_str()
                    .or_else(|| m["model"].as_str())
                    .or_else(|| m["modelId"].as_str())
                    .unwrap_or_default();
                let disp = m["displayName"].as_str();
                if let Some(qi) = m.get("quotaInfo").filter(|q| q.is_object()) {
                    if let Some((pct, reset)) = parse_remaining_fraction(qi, qi.get("resetTime").is_some()) {
                        let (seconds, cadence_tag) = cadence_from_hints(qi, id, Some(18000));
                        entries.push(RawEntry {
                            group: normalize_group_name(id, disp),
                            seconds,
                            cadence_tag,
                            remaining_pct: pct,
                            reset,
                        });
                    }
                }
                if let Some(wq) = m.get("weeklyQuotaInfo").filter(|q| q.is_object()) {
                    if let Some((pct, reset)) = parse_remaining_fraction(wq, wq.get("resetTime").is_some()) {
                        let (seconds, cadence_tag) = cadence_from_hints(wq, id, Some(604800));
                        entries.push(RawEntry {
                            group: normalize_group_name(id, disp),
                            seconds,
                            cadence_tag,
                            remaining_pct: pct,
                            reset,
                        });
                    }
                }
            }
        }
        Value::Object(map) => {
            for (id, m) in map {
                let disp = m["displayName"].as_str();
                if let Some(qi) = m.get("quotaInfo").filter(|q| q.is_object()) {
                    if let Some((pct, reset)) = parse_remaining_fraction(qi, qi.get("resetTime").is_some()) {
                        let (seconds, cadence_tag) = cadence_from_hints(qi, id, Some(18000));
                        entries.push(RawEntry {
                            group: normalize_group_name(id, disp),
                            seconds,
                            cadence_tag,
                            remaining_pct: pct,
                            reset,
                        });
                    }
                }
                if let Some(wq) = m.get("weeklyQuotaInfo").filter(|q| q.is_object()) {
                    if let Some((pct, reset)) = parse_remaining_fraction(wq, wq.get("resetTime").is_some()) {
                        let (seconds, cadence_tag) = cadence_from_hints(wq, id, Some(604800));
                        entries.push(RawEntry {
                            group: normalize_group_name(id, disp),
                            seconds,
                            cadence_tag,
                            remaining_pct: pct,
                            reset,
                        });
                    }
                }
            }
        }
        _ => {}
    }

    // 4. GetUserStatus / GetCommandModelConfigs fallback: clientModelConfigs[]
    for ptr in [
        "/userStatus/cascadeModelConfigData/clientModelConfigs",
        "/cascadeModelConfigData/clientModelConfigs",
        "/clientModelConfigs",
    ] {
        if let Some(cfgs) = v.pointer(ptr).and_then(Value::as_array) {
            for cfg in cfgs {
                let id = cfg["modelOrAlias"]["model"]
                    .as_str()
                    .or_else(|| cfg["modelOrAlias"]["alias"].as_str())
                    .unwrap_or_default();
                let disp = cfg["label"].as_str();
                if let Some(qi) = cfg.get("quotaInfo").filter(|q| q.is_object()) {
                    if let Some((pct, reset)) = parse_remaining_fraction(qi, qi.get("resetTime").is_some()) {
                        let (seconds, cadence_tag) = cadence_from_hints(qi, id, Some(18000));
                        entries.push(RawEntry {
                            group: normalize_group_name(id, disp),
                            seconds,
                            cadence_tag,
                            remaining_pct: pct,
                            reset,
                        });
                    }
                }
            }
        }
    }

    // 5. CLI statusLine local rate_limits format
    if let Some(rl) = v.get("rate_limits").filter(|r| r.is_object()) {
        let now = chrono::Utc::now().timestamp();
        for (key, seconds, tag) in [
            ("five_hour", 18000_i64, "5 小时"),
            ("seven_day", 604800_i64, "本周"),
        ] {
            let win = &rl[key];
            if !win.is_object() {
                continue;
            }
            let reset = parse_reset_time(&win["resets_at"]).or_else(|| parse_reset_time(&win["resetTime"]));
            if reset.is_some_and(|t| t <= now) {
                continue;
            }
            let pct = if let Some(used) = super::super::number(&win["used_percentage"]).filter(|n| (0.0..=100.0).contains(n)) {
                (100.0 - used).clamp(0.0, 100.0) as f32
            } else if let Some(rf) = super::super::number(&win["remaining_fraction"]).filter(|n| (0.0..=1.0).contains(n)) {
                (rf * 100.0).clamp(0.0, 100.0) as f32
            } else {
                continue;
            };
            entries.push(RawEntry {
                group: tag.into(),
                seconds: Some(seconds),
                cadence_tag: None,
                remaining_pct: pct,
                reset,
            });
        }
    }

    // 6. Optional `quota` container or flat `{ model_id: { remaining_fraction, ... } }` map from statusLine
    if let Some(q) = v.get("quota").and_then(Value::as_object) {
        let qv = Value::Object(q.clone());
        let nested = collect_quota_entries(&qv);
        if !nested.is_empty() {
            entries.extend(nested);
        } else {
            for (id, item) in q {
                if !item.is_object() {
                    continue;
                }
                let disp = item["displayName"]
                    .as_str()
                    .or_else(|| item["display_name"].as_str())
                    .or_else(|| item["label"].as_str());
                if let Some((pct, reset)) = parse_remaining_fraction(item, false) {
                    let (seconds, cadence_tag) = cadence_from_hints(item, id, None);
                    entries.push(RawEntry {
                        group: normalize_group_name(id, disp),
                        seconds,
                        cadence_tag,
                        remaining_pct: pct,
                        reset,
                    });
                }
            }
        }
    }

    entries
}

fn extract_credits(v: &Value) -> Option<super::super::workbuddy::Credits> {
    let remaining = [
        "/userStatus/planStatus/availablePromptCredits",
        "/planStatus/availablePromptCredits",
        "/availablePromptCredits",
        "/ai_credits/remaining",
        "/credits/remaining",
    ]
    .into_iter()
    .find_map(|ptr| super::super::number(v.pointer(ptr)?))
    .filter(|n| *n >= 0.0)?;
    let monthly = [
        "/userStatus/planStatus/monthlyPromptCredits",
        "/planStatus/monthlyPromptCredits",
        "/monthlyPromptCredits",
        "/ai_credits/total",
        "/credits/total",
    ]
    .into_iter()
    .find_map(|ptr| super::super::number(v.pointer(ptr)?))
    .filter(|n| *n > 0.0);
    let packages = monthly
        .map(|m| vec![("月度总额".to_owned(), m)])
        .unwrap_or_default();
    Some(super::super::workbuddy::Credits { remaining, packages })
}

pub fn parse(v: &Value) -> Result<Snapshot, String> {
    parse_with_plan(v, "")
}

pub(crate) fn parse_with_plan(v: &Value, fallback_plan: &str) -> Result<Snapshot, String> {
    if !v.is_object() {
        return Err(PARSE_ERROR.into());
    }
    let credits = extract_credits(v);
    let entries = collect_quota_entries(v);
    if entries.is_empty() && credits.is_none() {
        if v.get("context_window").is_some() {
            return Err("尚无有效的 Antigravity CLI 套餐额度记录，请在终端使用 /usage 后刷新".into());
        }
        return Err("Antigravity 服务未返回可用的模型配额信息".into());
    }

    // 同一模型分组 + 周期只保留最紧（剩余最低）的额度池，并保留最早有效重置时间。
    let mut grouped: BTreeMap<(String, Option<i64>), (Option<&'static str>, f32, Option<i64>)> = BTreeMap::new();
    let mut group_cadences: BTreeMap<String, Vec<Option<i64>>> = BTreeMap::new();

    for e in entries {
        group_cadences.entry(e.group.clone()).or_default();
        let cs = group_cadences.get_mut(&e.group).unwrap();
        if !cs.contains(&e.seconds) {
            cs.push(e.seconds);
        }
        let key = (e.group, e.seconds);
        match grouped.get_mut(&key) {
            Some((tag, min_pct, min_reset)) => {
                if e.remaining_pct < *min_pct {
                    *min_pct = e.remaining_pct;
                }
                if tag.is_none() {
                    *tag = e.cadence_tag;
                }
                match (*min_reset, e.reset) {
                    (None, Some(r)) => *min_reset = Some(r),
                    (Some(old), Some(new)) if new < old => *min_reset = Some(new),
                    _ => {}
                }
            }
            None => {
                grouped.insert(key, (e.cadence_tag, e.remaining_pct, e.reset));
            }
        }
    }

    let mut windows: Vec<Window> = grouped
        .into_iter()
        .map(|((group, seconds), (cadence_tag, remaining, reset))| {
            let multi_cadence = group_cadences.get(&group).is_some_and(|v| v.len() > 1);
            let label = if multi_cadence {
                if let Some(tag) = cadence_tag {
                    format!("{group} · {tag}")
                } else if seconds.is_some() {
                    format!("{group} · {}", super::super::cadence(seconds))
                } else {
                    group
                }
            } else {
                group
            };
            Window {
                label,
                seconds,
                remaining,
                reset,
                amount: None,
            }
        })
        .collect();

    windows.sort_by(|a, b| {
        a.remaining
            .total_cmp(&b.remaining)
            .then_with(|| a.seconds.unwrap_or(i64::MAX).cmp(&b.seconds.unwrap_or(i64::MAX)))
            .then_with(|| a.label.cmp(&b.label))
    });

    let extra = if windows.len() > 2 {
        windows.split_off(2)
    } else {
        Vec::new()
    };

    let plan = {
        let p = extract_plan_name(v);
        if p.is_empty() {
            fallback_plan.to_owned()
        } else {
            p
        }
    };

    Ok(Snapshot {
        plan,
        windows,
        extra,
        credits,
        at: chrono::Utc::now().timestamp(),
        ..Default::default()
    })
}

fn read_local_usage_snapshot(path: &Path) -> Result<(String, Snapshot), (String, u64)> {
    let root = read_json_file(path).map_err(|e| (e, 0))?;
    if let Some(at) = root["at"].as_i64() {
        let now = chrono::Utc::now().timestamp();
        if at <= 0 || at > now + 120 {
            return Err(("Antigravity 本地用量记录时间戳无效".into(), 0));
        }
    }
    let mut snapshot = parse(&root).map_err(|e| (e, 0))?;
    if let Some(at) = root["at"].as_i64().filter(|t| *t > 0) {
        snapshot.at = at;
    }
    let slot = if let Some(email) = root["email"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
        format!("ag-user:{}", sha256_hex(email.as_bytes()))
    } else {
        let norm = fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_ascii_lowercase();
        format!("ag-cli:{}", sha256_hex(norm.as_bytes()))
    };
    Ok((slot, snapshot))
}

fn query_file_at(
    agent: &ureq::Agent,
    base: &str,
    path: &Path,
) -> Result<(String, Option<String>, Snapshot), (String, u64)> {
    query_file_with_oauth_url(agent, base, OAUTH_TOKEN_URL, path)
}

fn query_file_with_oauth_url(
    agent: &ureq::Agent,
    base: &str,
    token_url: &str,
    path: &Path,
) -> Result<(String, Option<String>, Snapshot), (String, u64)> {
    let root = read_json_file(path).map_err(|e| (e, 0))?;
    if has_oauth_token_field(&root) {
        let oauth = extract_oauth_from_value(&root).map_err(|e| (e, 0))?;
        let mut active_token = match oauth.access_token.clone() {
            Some(tok) => tok,
            None => exchange_refresh_token(agent, token_url, &oauth)?,
        };
        let (project, snapshot) = match query_cloud_code_at(agent, base, &active_token, oauth.project.as_deref()) {
            Ok(res) => res,
            Err((msg, _)) if msg == LOGIN_ERROR && oauth.refresh_token.is_some() && oauth.client_id.is_some() => {
                active_token = exchange_refresh_token(agent, token_url, &oauth)?;
                query_cloud_code_at(agent, base, &active_token, oauth.project.as_deref())?
            }
            Err(err) => return Err(err),
        };
        let _ = &active_token;
        // 只读重校验文件凭据，确保查询期间客户端未替换账号
        let reread = read_json_file(path).and_then(|v| extract_oauth_from_value(&v)).map_err(|e| (e, 0))?;
        if reread.access_token != oauth.access_token || reread.refresh_token != oauth.refresh_token {
            return Err(("Antigravity 登录文件已变化，请重新刷新额度".into(), 0));
        }
        let slot = file_slot(oauth.user_slot.as_deref(), project.as_deref().or(oauth.project.as_deref()), path);
        Ok((slot, project, snapshot))
    } else {
        let (slot, snapshot) = read_local_usage_snapshot(path)?;
        Ok((slot, None, snapshot))
    }
}

pub(super) fn fetch(agent: &ureq::Agent, account: &Account) -> Result<Snapshot, (String, u64)> {
    if account.slot.as_deref().is_some_and(|s| s.starts_with("ag-web:")) {
        let session = load_web_session(account).map_err(|e| (e, 0))?;
        let (_email, mut snap) = super::super::gemini::query_web_session(agent, &session.email, &session.cookie)?;
        if snap.plan.is_empty() {
            snap.plan = "Google AI".into();
        }
        return Ok(snap);
    }
    if account.slot.as_deref().is_some_and(|s| s.starts_with("ag-token:")) {
        let session = load_token_session(account).map_err(|e| (e, 0))?;
        let (_project, snapshot) =
            query_cloud_code_at(agent, CLOUD_CODE_BASE, &session.access_token, session.project.as_deref())?;
        return Ok(snapshot);
    }
    if account.source.as_os_str().is_empty() {
        return Err(("请重新连接 Antigravity 账号".into(), 0));
    }
    if is_cli_usage_path(&account.source) && !account.source.is_file() {
        return Err((
            "尚无 Antigravity CLI 本地用量记录，请在终端使用 /usage 后刷新".into(),
            0,
        ));
    }
    let (actual_slot, _project, snapshot) = query_file_at(agent, CLOUD_CODE_BASE, &account.source)?;
    if let Some(expected) = account.slot.as_deref() {
        if expected.starts_with("ag-user:") && actual_slot.starts_with("ag-user:") && expected != actual_slot {
            return Err(("本机 Antigravity 登录账号已切换，请重新连接或添加新账号".into(), 0));
        }
    }
    Ok(snapshot)
}

pub(crate) fn add(config: &mut Config, path: &Path, alias: &str) -> Result<(), String> {
    if !path.is_file() {
        return Err("请选择已有的 Antigravity 登录 JSON 或本地用量文件".into());
    }
    let root = read_json_file(path)?;
    let slot = if has_oauth_token_field(&root) {
        let oauth = extract_oauth_from_value(&root)?;
        file_slot(oauth.user_slot.as_deref(), oauth.project.as_deref(), path)
    } else if is_cli_usage_path(path) {
        let norm = fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_ascii_lowercase();
        format!("ag-cli:{}", sha256_hex(norm.as_bytes()))
    } else {
        read_local_usage_snapshot(path).map_err(|e| e.0)?.0
    };
    let canon = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if config.accounts.iter().any(|a| {
        a.provider == Provider::Antigravity
            && (a.slot.as_deref() == Some(slot.as_str())
                || (!canon.as_os_str().is_empty()
                    && a.source.to_string_lossy().eq_ignore_ascii_case(&canon.to_string_lossy())))
    }) {
        return Err("这个 Antigravity 账号已经添加".into());
    }
    let next = config.accounts.iter().filter(|a| a.provider == Provider::Antigravity).count() + 1;
    let alias = if alias.trim().is_empty() {
        format!("Antigravity · 账号 {next}")
    } else {
        alias.trim().to_owned()
    };
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let id = format!("Antigravity-{nanos}");
    let pinned = config.accounts.is_empty();
    if pinned || config.selected.is_empty() {
        config.selected = id.clone();
    }
    config.accounts.push(Account {
        id,
        provider: Provider::Antigravity,
        alias,
        source: canon,
        slot: Some(slot),
        enabled: true,
        pinned,
    });
    Ok(())
}

pub(crate) fn connect_cli(config: &mut Config, alias: &str) -> Result<String, String> {
    let usage_path = cli_usage_path();
    install_cli_collector(&usage_path)?;
    let canon = fs::canonicalize(&usage_path).unwrap_or(usage_path);
    if let Some(existing) = config
        .accounts
        .iter_mut()
        .find(|a| a.provider == Provider::Antigravity && is_cli_usage_path(&a.source))
    {
        existing.enabled = true;
        if !alias.trim().is_empty() {
            existing.alias = alias.trim().to_owned();
        }
        config.selected = existing.id.clone();
        return Ok(existing.id.clone());
    }
    let norm = canon.to_string_lossy().to_ascii_lowercase();
    let slot = format!("ag-cli:{}", sha256_hex(norm.as_bytes()));
    let id = format!("Antigravity-cli-{}", &slot[7..19]);
    let label = if alias.trim().is_empty() {
        "Antigravity · CLI 本地采集".to_owned()
    } else {
        alias.trim().to_owned()
    };
    let pinned = config.pinned_indices().len() < 3 || config.accounts.is_empty();
    config.selected = id.clone();
    config.accounts.push(Account {
        id: id.clone(),
        provider: Provider::Antigravity,
        alias: label,
        source: canon,
        slot: Some(slot),
        enabled: true,
        pinned,
    });
    Ok(id)
}

pub struct Connected {
    pub source: PathBuf,
    pub slot: String,
    pub label: String,
    pub snapshot: Snapshot,
    token_session: Option<TokenSession>,
    web_session: Option<WebSession>,
}

impl Connected {
    pub fn finish(self) -> Result<Self, String> {
        if let Some(session) = &self.token_session {
            save_token_session(session)?;
        }
        if let Some(ws) = &self.web_session {
            save_web_session(ws)?;
        }
        Ok(self)
    }
}

pub struct Connect {
    rx: mpsc::Receiver<Result<Connected, String>>,
    pub target: Option<Account>,
    cancelled: Arc<AtomicBool>,
}

impl Connect {
    pub fn start_file(path: PathBuf, alias: String, target: Option<Account>) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        std::thread::spawn(move || {
            let result = (|| {
                let source = fs::canonicalize(&path).map_err(|_| "无法读取所选 Antigravity 文件")?;
                let (slot, _proj, snapshot) =
                    query_file_at(&super::super::http_agent(), CLOUD_CODE_BASE, &source).map_err(|e| e.0)?;
                let label = if alias.trim().is_empty() {
                    "Antigravity".to_owned()
                } else {
                    alias.trim().to_owned()
                };
                Ok(Connected {
                    source,
                    slot,
                    label,
                    snapshot,
                    token_session: None,
                    web_session: None,
                })
            })();
            let _ = tx.send(result);
        });
        Self { rx, target, cancelled }
    }

    pub fn start_token(alias: String, raw_token: String, raw_project: String, target: Option<Account>) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        std::thread::spawn(move || {
            let result = (|| {
                let access_token = validate_token(&raw_token)?;
                let project = Some(raw_project.trim().to_owned()).filter(|s| !s.is_empty());
                let (resolved_proj, snapshot) =
                    query_cloud_code_at(&super::super::http_agent(), CLOUD_CODE_BASE, &access_token, project.as_deref())
                        .map_err(|e| e.0)?;
                let slot = token_slot_from_token(&access_token, resolved_proj.as_deref());
                let label = if alias.trim().is_empty() {
                    if let Some(p) = resolved_proj.as_deref() {
                        format!("Antigravity · {p}")
                    } else {
                        "Antigravity".to_owned()
                    }
                } else {
                    alias.trim().to_owned()
                };
                Ok(Connected {
                    source: PathBuf::new(),
                    slot: slot.clone(),
                    label,
                    snapshot,
                    token_session: Some(TokenSession {
                        access_token,
                        project: resolved_proj,
                        slot,
                    }),
                    web_session: None,
                })
            })();
            let _ = tx.send(result);
        });
        Self { rx, target, cancelled }
    }

    pub fn start_chrome(
        alias: String,
        address: String,
        raw_cookie: Option<String>,
        target: Option<Account>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let (email, cookie) = if let Some(ck) = raw_cookie.filter(|s| !s.trim().is_empty()) {
                    let (em, _) = super::super::gemini::email_slot(&address)?;
                    (em, ck)
                } else {
                    super::super::gemini::login_chrome_session(&address, &stop)?
                };
                if stop.load(Ordering::Relaxed) {
                    return Err("已取消 Antigravity 连接".into());
                }
                let (clean_email, mut snapshot) =
                    super::super::gemini::query_web_session(&super::super::http_agent(), &email, &cookie).map_err(|e| e.0)?;
                if stop.load(Ordering::Relaxed) {
                    return Err("已取消 Antigravity 连接".into());
                }
                if snapshot.plan.is_empty() {
                    snapshot.plan = "Google AI".into();
                }
                let hex = sha256_hex(clean_email.as_bytes());
                let slot = format!("ag-web:{hex}");
                let label = if alias.trim().is_empty() {
                    format!("Antigravity · {clean_email}")
                } else {
                    alias.trim().to_owned()
                };
                Ok(Connected {
                    source: PathBuf::new(),
                    slot: slot.clone(),
                    label,
                    snapshot,
                    token_session: None,
                    web_session: Some(WebSession {
                        email: clean_email,
                        cookie,
                        slot,
                    }),
                })
            })();
            let _ = tx.send(result);
        });
        Self { rx, target, cancelled }
    }

    pub fn poll(&self) -> Option<Result<Connected, String>> {
        self.rx.try_recv().ok()
    }

    pub fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn pending(target: Option<Account>) -> (Self, mpsc::Sender<Result<Connected, String>>) {
        let (tx, rx) = mpsc::channel();
        (Self { rx, target, cancelled: Arc::new(AtomicBool::new(false)) }, tx)
    }
}

impl Drop for Connect {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn apply_connection(
    config: &mut Config,
    target: Option<&Account>,
    connected: &Connected,
) -> Result<String, String> {
    if let Some(old) = target {
        let idx = config
            .accounts
            .iter()
            .position(|a| a.id == old.id && a.provider == Provider::Antigravity)
            .ok_or("原 Antigravity 账号已变化，请重新连接")?;
        if config
            .accounts
            .iter()
            .enumerate()
            .any(|(i, a)| i != idx && a.provider == Provider::Antigravity && a.slot.as_deref() == Some(connected.slot.as_str()))
        {
            return Err("这个 Antigravity 账号已经存在，无需重复添加".into());
        }
        let a = &mut config.accounts[idx];
        a.source = connected.source.clone();
        a.slot = Some(connected.slot.clone());
        a.enabled = true;
        if !connected.label.trim().is_empty() {
            a.alias = connected.label.trim().to_owned();
        }
        let id = a.id.clone();
        config.selected = id.clone();
        return Ok(id);
    }

    if config.accounts.iter().any(|a| {
        a.provider == Provider::Antigravity
            && (a.slot.as_deref() == Some(connected.slot.as_str())
                || (!connected.source.as_os_str().is_empty()
                    && a.source.to_string_lossy().eq_ignore_ascii_case(&connected.source.to_string_lossy())))
    }) {
        return Err("这个 Antigravity 账号已经添加，请使用重新连接".into());
    }

    let next = config.accounts.iter().filter(|a| a.provider == Provider::Antigravity).count() + 1;
    let alias = if connected.label.trim().is_empty() {
        format!("Antigravity · 账号 {next}")
    } else {
        connected.label.trim().to_owned()
    };
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let id = format!("Antigravity-{nanos}");
    let pinned = config.pinned_indices().len() < 3 || config.accounts.is_empty();
    config.accounts.push(Account {
        id: id.clone(),
        provider: Provider::Antigravity,
        alias,
        source: connected.source.clone(),
        slot: Some(connected.slot.clone()),
        enabled: true,
        pinned,
    });
    config.selected = id.clone();
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "auralsle-ag-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_retrieve_user_quota_and_groups_tightest_buckets() {
        let payload = json!({
            "buckets": [
                {"modelId": "gemini-2.5-pro", "remainingFraction": 0.80, "resetTime": "2030-01-01T10:00:00Z"},
                {"modelId": "gemini-3-pro", "remainingFraction": 0.62, "resetTime": "2030-01-01T08:00:00Z"},
                {"modelId": "gemini-2.5-flash", "remainingFraction": 0.95, "resetTime": "2030-01-01T12:00:00Z"},
                {"modelId": "claude-sonnet-4-5", "remainingFraction": 0.35, "resetTime": "2030-01-01T09:00:00Z"}
            ]
        });
        let s = parse_with_plan(&payload, "AI Pro").unwrap();
        assert_eq!(s.plan, "AI Pro");
        assert_eq!(s.windows.len(), 2);
        assert_eq!(s.extra.len(), 1);
        assert_eq!(s.windows[0].label, "Claude");
        assert_eq!(s.windows[0].remaining, 35.0);
        assert_eq!(s.windows[1].label, "Gemini Pro");
        assert!((s.windows[1].remaining - 62.0).abs() < 0.01);
        assert_eq!(s.extra[0].label, "Gemini Flash");
        assert!((s.extra[0].remaining - 95.0).abs() < 0.01);
    }

    #[test]
    fn parses_fetch_available_models_object_and_array_and_exhausted_buckets() {
        let obj_payload = json!({
            "models": {
                "gemini-2.5-pro": {
                    "displayName": "Gemini 2.5 Pro",
                    "quotaInfo": {"remainingFraction": 0.45, "resetTime": "2030-01-01T08:00:00Z"},
                    "weeklyQuotaInfo": {"remainingFraction": 0.85, "resetTime": "2030-01-05T08:00:00Z"}
                },
                "claude-4-opus": {
                    "displayName": "Claude 4 Opus",
                    "quotaInfo": {"resetTime": "2030-01-01T07:00:00Z"}
                }
            }
        });
        let s = parse(&obj_payload).unwrap();
        assert_eq!(s.windows[0].label, "Claude");
        assert_eq!(s.windows[0].remaining, 0.0);
        assert_eq!(s.windows[1].label, "Gemini Pro · 5 小时");
        assert_eq!(s.windows[1].remaining, 45.0);
        assert_eq!(s.extra[0].label, "Gemini Pro · 本周");
        assert_eq!(s.extra[0].remaining, 85.0);

        let arr_payload = json!({
            "models": [
                {"name": "gemini-3-flash", "quotaInfo": {"remainingFraction": 0.9}},
                {"name": "claude-sonnet", "quotaInfo": {"remainingFraction": 0.5}}
            ]
        });
        let sa = parse(&arr_payload).unwrap();
        assert_eq!(sa.windows[0].label, "Claude");
        assert_eq!(sa.windows[1].label, "Gemini Flash");
    }

    #[test]
    fn parses_get_user_status_tier_preference_and_statusline_rate_limits() {
        let status_payload = json!({
            "userStatus": {
                "planStatus": {"planInfo": {"planName": "Pro"}},
                "userTier": {"id": "g1-ultra-tier", "preferredName": "Google AI Ultra"},
                "cascadeModelConfigData": {
                    "clientModelConfigs": [
                        {"label": "Gemini 3.8 Pro", "quotaInfo": {"remainingFraction": 0.72, "resetTime": "2030-01-02T00:00:00Z"}}
                    ]
                }
            }
        });
        let s = parse(&status_payload).unwrap();
        assert_eq!(s.plan, "AI Ultra");
        assert_eq!(s.windows[0].label, "Gemini Pro");
        assert!((s.windows[0].remaining - 72.0).abs() < 0.01);

        let future = chrono::Utc::now().timestamp() + 3600;
        let cli_payload = json!({
            "version": 1,
            "plan_tier": "AI Pro",
            "rate_limits": {
                "five_hour": {"used_percentage": 30.0, "resets_at": future},
                "seven_day": {"used_percentage": 12.0, "resets_at": future + 86400}
            }
        });
        let cs = parse(&cli_payload).unwrap();
        assert_eq!(cs.plan, "AI Pro");
        assert_eq!(cs.windows.len(), 2);
        assert_eq!(cs.windows[0].remaining, 70.0);
        assert_eq!(cs.windows[1].remaining, 88.0);
    }

    #[test]
    fn rejects_empty_invalid_or_context_only_payloads() {
        for bad in [
            json!({}),
            json!([]),
            json!({"buckets": []}),
            json!({"models": [{"name": "gemini-pro"}]}),
            json!({"context_window": {"used_percentage": 40}}),
            json!({"buckets": [{"modelId": "gemini-pro", "remainingFraction": 1.5}]}),
            json!({"buckets": [{"modelId": "gemini-pro", "remainingFraction": -0.1}]}),
        ] {
            assert!(parse(&bad).is_err(), "应拒绝无效或缺少真实配额的响应: {bad}");
        }
    }

    #[test]
    fn cloud_code_falls_back_to_fetch_available_models_and_redacts_errors() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_ref = calls.clone();
        let agent = ureq::AgentBuilder::new()
            .try_proxy_from_env(false)
            .middleware(move |req: ureq::Request, _: ureq::MiddlewareNext| {
                let n = calls_ref.fetch_add(1, Ordering::Relaxed);
                assert_eq!(req.header("Authorization"), Some("Bearer synthetic-ag-token"));
                if req.url().ends_with(":loadCodeAssist") {
                    assert!(n == 0);
                    ureq::Response::new(
                        200,
                        "OK",
                        &json!({"cloudaicompanionProject": "demo-proj-123", "currentTier": {"id": "pro-tier"}}).to_string(),
                    )
                } else if req.url().ends_with(":retrieveUserQuota") {
                    ureq::Response::new(404, "Not Found", "{}")
                } else {
                    assert!(req.url().ends_with(":fetchAvailableModels"));
                    ureq::Response::new(
                        200,
                        "OK",
                        &json!({"models": {"gemini-3-pro": {"quotaInfo": {"remainingFraction": 0.42}}}}).to_string(),
                    )
                }
            })
            .build();

        let (proj, snap) = query_cloud_code_at(&agent, "https://example.invalid/v1internal", "synthetic-ag-token", None).unwrap();
        assert_eq!(proj.as_deref(), Some("demo-proj-123"));
        assert_eq!(snap.plan, "AI Pro");
        assert!((snap.windows[0].remaining - 42.0).abs() < 0.01);

        let err_agent = ureq::AgentBuilder::new()
            .try_proxy_from_env(false)
            .middleware(|_: ureq::Request, _: ureq::MiddlewareNext| {
                ureq::Response::new(403, "Forbidden", "secret-backend-trace")
            })
            .build();
        let err = query_cloud_code_at(&err_agent, "https://example.invalid/v1internal", "synthetic-ag-token", None).unwrap_err();
        assert!(err.0.contains("重新登录"));
        assert!(!err.0.contains("secret-backend-trace"));
    }

    #[test]
    fn oauth_file_query_never_mutates_file_and_detects_live_change() {
        let dir = temp_dir("file");
        let path = dir.join("oauth_creds.json");
        let id_claims = URL_SAFE_NO_PAD.encode(json!({"email": "dev@example.com"}).to_string());
        let content = json!({"access_token": "tok-a", "id_token": format!("h.{id_claims}.s")}).to_string();
        fs::write(&path, &content).unwrap();

        let p_clone = path.clone();
        let mutated = Arc::new(AtomicUsize::new(0));
        let mutated_ref = mutated.clone();
        let agent = ureq::AgentBuilder::new()
            .try_proxy_from_env(false)
            .middleware(move |req: ureq::Request, _: ureq::MiddlewareNext| {
                if req.url().ends_with(":loadCodeAssist") {
                    ureq::Response::new(200, "OK", &json!({"cloudaicompanionProject": "proj-1"}).to_string())
                } else {
                    if mutated_ref.load(Ordering::Relaxed) > 0 {
                        let c = URL_SAFE_NO_PAD.encode(json!({"email": "other@example.com"}).to_string());
                        fs::write(&p_clone, json!({"access_token": "tok-b", "id_token": format!("h.{c}.s")}).to_string()).unwrap();
                    }
                    ureq::Response::new(200, "OK", &json!({"buckets": [{"modelId": "gemini-3-pro", "remainingFraction": 0.8}]}).to_string())
                }
            })
            .build();

        let (slot, _proj, snap) = query_file_at(&agent, "https://example.invalid/v1internal", &path).unwrap();
        assert!(slot.starts_with("ag-user:"));
        assert_eq!(snap.windows[0].remaining, 80.0);
        assert_eq!(fs::read_to_string(&path).unwrap(), content, "只读查询绝不可改写原凭据文件");

        mutated.store(1, Ordering::Relaxed);
        let err = query_file_at(&agent, "https://example.invalid/v1internal", &path).unwrap_err();
        assert!(err.0.contains("变化"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_cli_collector_preserves_existing_statusline_and_rejects_traversal() {
        let dir = temp_dir("cli");
        let usage = dir.join(CLI_USAGE);
        install_cli_collector(&usage).unwrap();
        assert!(dir.join(CLI_COLLECTOR).is_file());
        let settings: Value = serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert!(settings["statusLine"]["command"].as_str().unwrap().contains(CLI_COLLECTOR));

        fs::write(dir.join("settings.json"), json!({"statusLine": {"command": "custom-tool.exe"}}).to_string()).unwrap();
        assert!(install_cli_collector(&usage).is_err());
        let kept: Value = serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(kept["statusLine"]["command"], "custom-tool.exe");
        assert!(session_path("../escape").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn token_dpapi_storage_is_encrypted_and_forget_deletes_it() {
        let _g = crate::ui_tests::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir("dpapi");
        let old_dir = std::env::var_os("AURALSLE_DATA_DIR");
        std::env::set_var("AURALSLE_DATA_DIR", &dir);

        let slot = token_slot_from_token("synthetic-secret-token", Some("proj-x"));
        let session = TokenSession {
            access_token: "synthetic-secret-token".into(),
            project: Some("proj-x".into()),
            slot: slot.clone(),
        };
        save_token_session(&session).unwrap();
        let bin_path = session_path(&slot).unwrap();
        let raw_bin = fs::read(&bin_path).unwrap();
        assert!(!raw_bin.windows(12).any(|w| w == b"synthetic-se"));

        let account = Account {
            id: "ag-test".into(),
            provider: Provider::Antigravity,
            alias: "AG".into(),
            source: PathBuf::new(),
            slot: Some(slot),
            enabled: true,
            pinned: true,
        };
        let loaded = load_token_session(&account).unwrap();
        assert_eq!(loaded.access_token, "synthetic-secret-token");
        forget(&account);
        assert!(!bin_path.exists());

        let (mut pending_conn, _tx) = Connect::pending(Some(account));
        assert!(!pending_conn.cancelled());
        pending_conn.cancel();
        assert!(pending_conn.cancelled());

        match old_dir {
            Some(v) => std::env::set_var("AURALSLE_DATA_DIR", v),
            None => std::env::remove_var("AURALSLE_DATA_DIR"),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_snake_case_quota_map_and_prompt_credits() {
        let payload = json!({
            "plan_tier": "ultra",
            "availablePromptCredits": 450,
            "monthlyPromptCredits": 1000,
            "quota": {
                "gemini-2.5-pro": {
                    "remaining_fraction": 0.72,
                    "reset_in_seconds": 3600
                },
                "claude-sonnet-4-5": {
                    "used_percentage": 20.0,
                    "resets_at": "2030-01-01T12:00:00Z"
                }
            }
        });
        let snap = parse(&payload).unwrap();
        assert_eq!(snap.plan, "AI Ultra");
        assert_eq!(snap.credits.as_ref().unwrap().remaining, 450.0);
        assert!(snap.windows.iter().any(|w| w.label == "Gemini Pro" && (w.remaining - 72.0).abs() < 0.01));
        assert!(snap.windows.iter().any(|w| w.label == "Claude" && (w.remaining - 80.0).abs() < 0.01));
    }

    #[test]
    fn oauth_refresh_token_exchanges_and_retries_without_mutating_file() {
        let dir = temp_dir("refresh");
        let path = dir.join("application_default_credentials.json");
        let raw_cfg = json!({
            "client_id": "synthetic-client-id",
            "client_secret": "synthetic-client-secret",
            "refresh_token": "synthetic-refresh-token",
            "quota_project_id": "adc-proj-1"
        })
        .to_string();
        fs::write(&path, &raw_cfg).unwrap();

        let agent = ureq::AgentBuilder::new()
            .try_proxy_from_env(false)
            .middleware(|req: ureq::Request, _: ureq::MiddlewareNext| {
                if req.url() == "https://token.invalid/token" {
                    ureq::Response::new(200, "OK", &json!({"access_token": "fresh-ya29-token"}).to_string())
                } else if req.url().ends_with(":loadCodeAssist") {
                    assert_eq!(req.header("Authorization"), Some("Bearer fresh-ya29-token"));
                    ureq::Response::new(200, "OK", &json!({"cloudaicompanionProject": "adc-proj-1"}).to_string())
                } else {
                    ureq::Response::new(
                        200,
                        "OK",
                        &json!({"buckets": [{"modelId": "gemini-3-pro", "remainingFraction": 0.64}]}).to_string(),
                    )
                }
            })
            .build();

        let (_slot, proj, snap) = query_file_with_oauth_url(
            &agent,
            "https://example.invalid/v1internal",
            "https://token.invalid/token",
            &path,
        )
        .unwrap();
        assert_eq!(proj.as_deref(), Some("adc-proj-1"));
        assert!((snap.windows[0].remaining - 64.0).abs() < 0.01);
        assert_eq!(fs::read_to_string(&path).unwrap(), raw_cfg, "只读刷新绝不改写 ADC/OAuth 原文件");
        let _ = fs::remove_dir_all(&dir);
    }
}
