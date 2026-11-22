//! OPPO 互联（O+ Connect）静默后台传输、连接状态探测与进度追踪模块。
//!
//! 集成自 `oppo-connect-bridge`，零新增外部依赖：
//! - **连接状态探测**：结合 `%APPDATA%\O+Connect\config.json`、`netstat -ano` ESTABLISHED 链路与 `*_main.log` 最近连接事件，准确区分已连接 / 重连中 / 未连接。
//! - **纯后台静默传输**：通过本地 HTTP Bridge（`http://127.0.0.1:45231/oclaw/v1` + 持久化 `GA_WS_TOKEN`）静默发送文件，不弹出 O+ Connect 前台窗口；若当前 O+ Connect 实例未带 token，则自动静默拉起带 token 的后台实例。
//! - **实时进度追踪**：优先解析 `devicespace\oshare\ovmsocketlog.log` 真实进度事件，同时结合历史吞吐速率（`%APPDATA%\oppo-connect-bridge\rate.txt`）平滑推进环形进度条。

use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const NO_WINDOW: u32 = 0x0800_0000;

pub const DEFAULT_BASE: &str = "http://127.0.0.1:45231/oclaw/v1";
pub const TOKEN_ENV: &str = "GA_WS_TOKEN";
pub const SHARE_MENU_KEY: &str =
    r"HKCU\Software\Classes\*\shell\OppoConnectShare\shell\ShareToDevice\command";
const DEFAULT_BPS: f64 = 3.5 * 1024.0 * 1024.0;

/// 内嵌灵屿自有的白色互传图标（非第三方图标，128×128 透明底 RGBA PNG）
const TRANSFER_ICON_PNG: &[u8] = include_bytes!("../assets/auralsle-transfer-icon.png");

static OPPO_ICON_IMAGE: OnceLock<Option<eframe::egui::ColorImage>> = OnceLock::new();

/// 解码并缓存灵屿互联/互传图标为 `egui::ColorImage`。
pub fn icon_color_image() -> Option<&'static eframe::egui::ColorImage> {
    OPPO_ICON_IMAGE
        .get_or_init(|| {
            let img = image::load_from_memory(TRANSFER_ICON_PNG).ok()?.to_rgba8();
            let (w, h) = (img.width() as usize, img.height() as usize);
            Some(eframe::egui::ColorImage::from_rgba_unmultiplied([w, h], img.as_raw()))
        })
        .as_ref()
}

// ───────────────────────── 设备读取（config.json） ─────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub device_name: String,
    #[serde(default)]
    pub connect_type: Option<i64>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub device_type: Option<i64>,
    #[serde(default)]
    pub is_same_account: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub devices: Vec<Device>,
    pub current_id: Option<String>,
    pub local_ip: Option<String>,
    pub local_device_id: Option<String>,
}

impl Snapshot {
    pub fn current(&self) -> Option<&Device> {
        if let Some(id) = &self.current_id {
            if let Some(d) = self.devices.iter().find(|d| &d.device_id == id) {
                return Some(d);
            }
        }
        self.devices.first()
    }
}

pub fn config_path() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(PathBuf::from(appdata).join("O+Connect").join("config.json"))
}

pub fn read_config() -> Snapshot {
    let Some(path) = config_path() else {
        return Snapshot::default();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Snapshot::default();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Snapshot::default();
    };

    let mut devices: Vec<Device> = Vec::new();
    if let Some(arr) = v.get("devicesList").and_then(|x| x.as_array()) {
        for item in arr {
            if let Ok(d) = serde_json::from_value::<Device>(item.clone()) {
                if !d.device_id.is_empty() {
                    devices.push(d);
                }
            }
        }
    }
    if let Some(arr) = v.get("connectedDeviceHistoryList").and_then(|x| x.as_array()) {
        for item in arr {
            if let Ok(d) = serde_json::from_value::<Device>(item.clone()) {
                if !d.device_id.is_empty() && !devices.iter().any(|x| x.device_id == d.device_id) {
                    devices.push(d);
                }
            }
        }
    }

    let current_id = v
        .get("CurrentConnectedDevice")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    if let Some(id) = &current_id {
        if !devices.iter().any(|d| &d.device_id == id) {
            devices.insert(
                0,
                Device {
                    device_id: id.clone(),
                    device_name: String::new(),
                    ..Default::default()
                },
            );
        }
        devices.sort_by_key(|d| if &d.device_id == id { 0 } else { 1 });
    }

    Snapshot {
        devices,
        current_id,
        local_ip: v
            .get("local_address")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        local_device_id: v
            .get("deviceIdCodeData")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
    }
}

// ───────────────────────── O+Connect.exe 定位 ─────────────────────────

#[cfg(windows)]
fn reg_exe() -> PathBuf {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    PathBuf::from(root).join("System32").join("reg.exe")
}

#[cfg(windows)]
fn decode_console(bytes: &[u8]) -> String {
    let n = bytes.len();
    if n >= 16 {
        let zeros_odd = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
        if zeros_odd * 2 >= n / 2 {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            return String::from_utf16_lossy(&units);
        }
    }
    String::from_utf8_lossy(bytes).to_string()
}

fn extract_exe(s: &str) -> Option<String> {
    if let Some(a) = s.find('"') {
        if let Some(b) = s[a + 1..].find('"') {
            let cand = &s[a + 1..a + 1 + b];
            if cand.to_ascii_lowercase().ends_with(".exe") {
                return Some(cand.to_string());
            }
        }
    }
    for tok in s.split_whitespace() {
        let t = tok.trim_matches('"');
        if t.to_ascii_lowercase().ends_with(".exe") {
            return Some(t.to_string());
        }
    }
    None
}

#[cfg(windows)]
pub fn registered_command() -> Option<String> {
    let out = Command::new(reg_exe())
        .args(["query", SHARE_MENU_KEY, "/ve"])
        .creation_flags(NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = decode_console(&out.stdout);
    let idx = text.find("REG_SZ")?;
    Some(text[idx + "REG_SZ".len()..].trim().to_string())
}

#[cfg(not(windows))]
pub fn registered_command() -> Option<String> {
    None
}

pub fn exe_from_registry() -> Option<PathBuf> {
    let cmd = registered_command()?;
    let exe = extract_exe(&cmd)?;
    let p = PathBuf::from(exe);
    p.is_file().then_some(p)
}

fn probe(p: &str) -> Option<PathBuf> {
    let pb = PathBuf::from(p);
    pb.is_file().then_some(pb)
}

fn search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for env in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Ok(v) = std::env::var(env) {
            roots.push(PathBuf::from(v));
        }
    }
    roots
}

fn shallow_find(dir: &Path, depth: u8) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let p = e.path();
        if p.is_file() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.eq_ignore_ascii_case("O+Connect.exe") {
                    return Some(p);
                }
            }
        } else if depth > 0 && p.is_dir() {
            if let Some(f) = shallow_find(&p, depth - 1) {
                return Some(f);
            }
        }
    }
    None
}

pub fn locate_exe() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("OPPO_CONNECT_EXE") {
        if let Some(p) = probe(&v) {
            return Some(p);
        }
    }
    if let Some(p) = exe_from_registry() {
        return Some(p);
    }
    for cand in [
        r"D:\AMicSoftEXE\Oppo Connect\O+Connect.exe",
        r"C:\Program Files\OPPO Connect\O+Connect.exe",
        r"C:\Program Files\O+Connect\O+Connect.exe",
        r"C:\Program Files\OPPO\O+Connect\O+Connect.exe",
    ] {
        if let Some(p) = probe(cand) {
            return Some(p);
        }
    }
    for root in search_roots() {
        for sub in ["OPPO Connect", "O+Connect", "OPPO", "OppoConnect"] {
            if let Some(p) = shallow_find(&root.join(sub), 2) {
                return Some(p);
            }
        }
    }
    None
}

// ───────────────────────── 日志与连接状态探测 ─────────────────────────

pub fn devicespace_dir() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(PathBuf::from(appdata).join("O+Connect").join("devicespace"))
}

fn newest_with_suffix(dir: &Path, suffix: &str) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let name = p.file_name()?.to_string_lossy().to_string();
        if !name.ends_with(suffix) || name.ends_with(".log.1") {
            continue;
        }
        if let Ok(m) = e.metadata() {
            if let Ok(t) = m.modified() {
                if best.as_ref().map(|(bt, _)| t > *bt).unwrap_or(true) {
                    best = Some((t, p));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conn {
    Connected,
    Reconnecting,
    Disconnected,
}

#[derive(Debug, Clone)]
pub struct LinkStatus {
    pub conn: Conn,
    pub device_name: Option<String>,
    pub device_id: Option<String>,
    pub detail: String,
}

impl Default for LinkStatus {
    fn default() -> Self {
        Self {
            conn: Conn::Disconnected,
            device_name: None,
            device_id: None,
            detail: "未连接".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecentEvent {
    Connected,
    Disconnected,
}

#[cfg(windows)]
fn established_to(host: &str) -> usize {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    let ns = PathBuf::from(root).join("System32").join("netstat.exe");
    let Ok(out) = Command::new(ns)
        .arg("-ano")
        .creation_flags(NO_WINDOW)
        .output()
    else {
        return 0;
    };
    let text = decode_console(&out.stdout);
    let needle = format!("{}:", host);
    let mut n = 0;
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() >= 4
            && f[0].eq_ignore_ascii_case("TCP")
            && f[3].eq_ignore_ascii_case("ESTABLISHED")
            && f[2].starts_with(&needle)
        {
            n += 1;
        }
    }
    n
}

#[cfg(not(windows))]
fn established_to(_host: &str) -> usize {
    0
}

fn last_conn_event() -> Option<RecentEvent> {
    let dir = devicespace_dir()?;
    let path = newest_with_suffix(&dir, "_main.log")?;
    let mut f = std::fs::File::open(&path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(64 * 1024);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = String::new();
    f.read_to_string(&mut buf).ok()?;

    for line in buf.lines().rev() {
        if line.contains("event\":\"onDisconnected") || line.contains("callback onDisconnected") {
            return Some(RecentEvent::Disconnected);
        }
        if line.contains("event\":\"onConnected") || line.contains("js onConnect") {
            return Some(RecentEvent::Connected);
        }
    }
    None
}

pub fn probe_link() -> LinkStatus {
    let snap = read_config();
    let dev = snap.current().cloned();
    let device_id = dev
        .as_ref()
        .map(|d| d.device_id.clone())
        .filter(|s| !s.is_empty());
    let device_name = dev
        .as_ref()
        .map(|d| d.device_name.trim().to_string())
        .filter(|s| !s.is_empty());

    let host = dev
        .as_ref()
        .and_then(|d| d.ip.clone())
        .and_then(|ip| ip.rsplit_once(':').map(|(h, _)| h.to_string()))
        .or_else(|| snap.local_ip.clone());

    let links = host.as_deref().map(established_to).unwrap_or(0);
    let evt = last_conn_event();

    let (conn, detail) = if links > 0 && evt != Some(RecentEvent::Disconnected) {
        (
            Conn::Connected,
            match device_name.as_deref() {
                Some(name) => format!("已连接 · {}", name),
                None => "已连接".to_string(),
            },
        )
    } else if links > 0 {
        (Conn::Reconnecting, "重连中".to_string())
    } else {
        (Conn::Disconnected, "未连接".to_string())
    };

    LinkStatus {
        conn,
        device_name,
        device_id,
        detail,
    }
}

/// 异步探测 OPPO 互联设备连接状态（避免在 UI 线程调用 `netstat`）。
pub fn probe_link_async() -> mpsc::Receiver<LinkStatus> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("oppo-connect-probe".into())
        .spawn(move || {
            let _ = tx.send(probe_link());
        });
    rx
}

// ───────────────────────── 进度估算与日志 Tailer ─────────────────────────

fn state_dir() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(PathBuf::from(appdata).join("oppo-connect-bridge"))
}

fn rate_path() -> Option<PathBuf> {
    Some(state_dir()?.join("rate.txt"))
}

fn load_rate_bps() -> f64 {
    rate_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|v| *v > 50_000.0)
        .unwrap_or(DEFAULT_BPS)
}

fn record_transfer(bytes: u64, secs: f64) {
    if secs <= 0.05 || bytes == 0 {
        return;
    }
    let measured = bytes as f64 / secs;
    if !(50_000.0..=2_000_000_000.0).contains(&measured) {
        return;
    }
    let prev = load_rate_bps();
    let smoothed = prev * 0.6 + measured * 0.4;
    if let Some(dir) = state_dir() {
        let _ = std::fs::create_dir_all(&dir);
        if let Some(p) = rate_path() {
            let _ = std::fs::write(p, format!("{:.0}", smoothed));
        }
    }
}

fn estimate_progress(bytes: u64, elapsed_secs: f64, rate_bps: f64) -> f32 {
    if bytes == 0 || rate_bps <= 0.0 {
        return (elapsed_secs as f32 * 0.25).clamp(0.05, 0.92);
    }
    let eta = (bytes as f64 / rate_bps).max(0.6);
    ((elapsed_secs / eta) as f32).clamp(0.05, 0.95)
}

fn path_bytes(path: &Path, depth: u8) -> u64 {
    let Ok(meta) = std::fs::metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if meta.is_dir() && depth < 6 {
        let mut sum = 0u64;
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                sum = sum.saturating_add(path_bytes(&e.path(), depth + 1));
            }
        }
        return sum;
    }
    0
}

struct OshareTailer {
    path: PathBuf,
    pos: u64,
}

impl OshareTailer {
    fn open() -> Option<Self> {
        let path = devicespace_dir()?.join("oshare").join("ovmsocketlog.log");
        if !path.is_file() {
            return None;
        }
        let pos = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Some(Self { path, pos })
    }

    fn poll_progress(&mut self) -> Option<f32> {
        let mut f = std::fs::File::open(&self.path).ok()?;
        let len = f.metadata().ok()?.len();
        if len < self.pos {
            self.pos = 0;
        }
        f.seek(SeekFrom::Start(self.pos)).ok()?;
        let mut buf = String::new();
        f.read_to_string(&mut buf).ok()?;
        self.pos = f.stream_position().unwrap_or(len);

        let mut latest = None;
        for line in buf.lines() {
            if !line.contains("contractedContent") {
                continue;
            }
            if let (Some(i), Some(j)) = (line.find("\"max\":"), line.find("\"value\":")) {
                let max_s: String = line[i + 6..]
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                let val_s: String = line[j + 8..]
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if let (Ok(max), Ok(val)) = (max_s.parse::<u32>(), val_s.parse::<u32>()) {
                    if max > 0 {
                        latest = Some((val.min(max) as f32 / max as f32).clamp(0.0, 0.99));
                    }
                }
            }
        }
        latest
    }
}

// ───────────────────────── 本地 HTTP Bridge（纯后台静默通道） ─────────────────────────

pub fn token_path() -> Option<PathBuf> {
    Some(state_dir()?.join("ga_ws_token.txt"))
}

fn random_token() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut out = String::with_capacity(64);
    for round in 0..4u64 {
        let mut h = DefaultHasher::new();
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
            .hash(&mut h);
        std::process::id().hash(&mut h);
        round.hash(&mut h);
        (&out as *const String as usize).hash(&mut h);
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

pub fn load_or_create_token() -> String {
    if let Ok(t) = std::env::var(TOKEN_ENV) {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }
    if let Some(p) = token_path() {
        if let Ok(s) = std::fs::read_to_string(&p) {
            let s = s.trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
        let t = random_token();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&p, &t);
        return t;
    }
    random_token()
}

#[cfg(windows)]
pub(crate) fn kill_all_oconnect() {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    let tk = PathBuf::from(root).join("System32").join("taskkill.exe");
    let _ = Command::new(tk)
        .args(["/IM", "O+Connect.exe", "/F", "/T"])
        .creation_flags(NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(not(windows))]
pub(crate) fn kill_all_oconnect() {}

fn launch_silent_with_token(exe: &Path, token: &str) -> std::io::Result<()> {
    let mut cmd = Command::new(exe);
    cmd.env(TOKEN_ENV, token)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(NO_WINDOW);
    cmd.spawn().map(|_| ())
}

fn restart_with_token(exe: &Path, token: &str, wait: Duration) -> Result<(), String> {
    kill_all_oconnect();
    std::thread::sleep(Duration::from_millis(1800));
    launch_silent_with_token(exe, token).map_err(|e| format!("后台拉起失败: {}", e))?;
    let client = BridgeClient::new(DEFAULT_BASE, token);
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if client.health().is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    Err("OPPO 互联静默通道未在预期时间内就绪".to_string())
}

pub struct BridgeClient {
    base: String,
    token: String,
    agent: ureq::Agent,
}

impl BridgeClient {
    pub fn new(base: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            token: token.into(),
            agent: ureq::agent(),
        }
    }

    fn get(&self, path: &str, timeout: Duration) -> Result<serde_json::Value, String> {
        let url = format!("{}{}", self.base, path);
        let resp = self
            .agent
            .get(&url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .timeout(timeout)
            .call();
        match resp {
            Ok(r) => {
                let text = r.into_string().map_err(|e| e.to_string())?;
                serde_json::from_str(&text).map_err(|e| e.to_string())
            }
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                Err(format!("HTTP {} {}", code, body))
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn post(
        &self,
        path: &str,
        body: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        let url = format!("{}{}", self.base, path);
        let payload = serde_json::to_string(&body).map_err(|e| e.to_string())?;
        let resp = self
            .agent
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/json")
            .timeout(timeout)
            .send_string(&payload);
        match resp {
            Ok(r) => {
                let text = r.into_string().map_err(|e| e.to_string())?;
                serde_json::from_str(&text).map_err(|e| e.to_string())
            }
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                Err(format!("HTTP {} {}", code, body))
            }
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn health(&self) -> Result<serde_json::Value, String> {
        self.get("/health", Duration::from_millis(1200))
    }

    pub fn transfer_file(
        &self,
        file: &Path,
        device_id: Option<&str>,
        timeout_ms: Option<u64>,
    ) -> Result<serde_json::Value, String> {
        let mut body = serde_json::Map::new();
        body.insert(
            "file".into(),
            serde_json::Value::String(file.to_string_lossy().to_string()),
        );
        if let Some(id) = device_id {
            body.insert("deviceId".into(), serde_json::Value::String(id.to_string()));
        }
        if let Some(t) = timeout_ms {
            body.insert("timeoutMs".into(), serde_json::Value::from(t));
        }
        self.post(
            "/transfer-file",
            serde_json::Value::Object(body),
            Duration::from_secs(180),
        )
    }

    pub fn transfer_via_remote_control(
        &self,
        files: &[PathBuf],
        device_id: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let list: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let mut body = serde_json::Map::new();
        body.insert("files".into(), serde_json::Value::from(list));
        if let Some(id) = device_id {
            body.insert("deviceId".into(), serde_json::Value::String(id.to_string()));
        }
        self.post(
            "/transfer-file-via-remote-control",
            serde_json::Value::Object(body),
            Duration::from_secs(180),
        )
    }
}

pub fn filter_paths(paths: &[PathBuf], exe: Option<&Path>) -> (Vec<PathBuf>, usize) {
    let exe_lc = exe.map(|p| p.to_string_lossy().to_ascii_lowercase());
    let mut ok = Vec::new();
    let mut rejected = 0usize;
    let mut seen = Vec::new();

    for p in paths {
        let s = p.to_string_lossy().to_string();
        if s.trim().is_empty() {
            continue;
        }
        let Ok(meta) = std::fs::metadata(p) else {
            rejected += 1;
            continue;
        };
        if !(meta.is_file() || meta.is_dir()) {
            rejected += 1;
            continue;
        }
        let lc = s.to_ascii_lowercase();
        if exe_lc.as_deref() == Some(lc.as_str()) {
            rejected += 1;
            continue;
        }
        if seen.contains(&lc) {
            continue;
        }
        seen.push(lc);
        ok.push(p.clone());
    }
    (ok, rejected)
}

// ───────────────────────── 灵动岛异步静默传输入口 ─────────────────────────

#[derive(Debug, Clone)]
pub enum TransferEvent {
    Progress(f32),
    Done { ok: bool, message: String },
}

fn friendly_bridge_error(raw: &str) -> String {
    if raw.contains("NO_CONNECTED_DEVICE") {
        "手机未连接，请先保持 OPPO 互联在线".to_string()
    } else if raw.contains("UPLOAD_TIMEOUT") || raw.contains("UPLOAD_STALLED") {
        "传输超时或连接中断".to_string()
    } else {
        raw.chars().take(48).collect()
    }
}

/// 异步通过本地静默 HTTP Bridge 发送文件（全程后台静默，不弹出 O+ Connect 窗口），
/// 并实时推送进度 `0.0..=1.0`。
pub fn send_files_async(paths: Vec<PathBuf>) -> mpsc::Receiver<TransferEvent> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("oppo-connect-send".into())
        .spawn(move || {
            let exe = locate_exe();
            let (valid_paths, rejected) = filter_paths(&paths, exe.as_deref());
            if valid_paths.is_empty() {
                let _ = tx.send(TransferEvent::Done {
                    ok: false,
                    message: format!("OPPO 互联：无可用文件（跳过 {} 项）", rejected),
                });
                return;
            }

            let snap = read_config();
            let device = snap.current().cloned();
            let device_id = device
                .as_ref()
                .map(|d| d.device_id.clone())
                .filter(|s| !s.is_empty());
            let device_name = device
                .as_ref()
                .map(|d| d.device_name.trim().to_string())
                .filter(|s| !s.is_empty());
            let target_suffix = device_name
                .as_deref()
                .map(|name| format!("至 {}", name))
                .unwrap_or_default();

            let token = load_or_create_token();
            let client = BridgeClient::new(DEFAULT_BASE, token.clone());

            let _ = tx.send(TransferEvent::Progress(0.04));

            // 若当前运行的 O+ Connect 未携带我们的 token，则静默后台重启并注入 token（绝不弹出主窗口）
            if client.health().is_err() {
                let Some(exe_path) = exe else {
                    let _ = tx.send(TransferEvent::Done {
                        ok: false,
                        message: "OPPO 互联发送失败：未找到 O+Connect.exe".to_string(),
                    });
                    return;
                };
                if let Err(e) = restart_with_token(&exe_path, &token, Duration::from_secs(14)) {
                    let _ = tx.send(TransferEvent::Done {
                        ok: false,
                        message: format!("OPPO 互联后台启动失败：{}", e),
                    });
                    return;
                }
                // 给手机链路一点回连缓冲时间
                for _ in 0..6 {
                    if probe_link().conn == Conn::Connected {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            }

            let total_bytes: u64 = valid_paths.iter().map(|p| path_bytes(p, 0)).sum();
            let rate_bps = load_rate_bps();
            let done_flag = Arc::new(AtomicBool::new(false));
            let started = Instant::now();

            // 后台进度汇报线程：优先读取 oshare 日志真实进度，其次平滑按速率估算
            let prog_done = done_flag.clone();
            let prog_tx = tx.clone();
            let _ = std::thread::Builder::new()
                .name("oppo-connect-progress".into())
                .spawn(move || {
                    let mut tailer = OshareTailer::open();
                    let mut best = 0.06_f32;
                    while !prog_done.load(Ordering::Relaxed) {
                        let log_p = tailer.as_mut().and_then(|t| t.poll_progress());
                        let est_p = estimate_progress(
                            total_bytes,
                            started.elapsed().as_secs_f64(),
                            rate_bps,
                        );
                        best = best.max(log_p.unwrap_or(est_p)).clamp(0.06, 0.96);
                        let _ = prog_tx.send(TransferEvent::Progress(best));
                        std::thread::sleep(Duration::from_millis(70));
                    }
                });

            let has_dir = valid_paths.iter().any(|p| p.is_dir());
            let mut last_err: Option<String> = None;
            let mut resp_dev_name: Option<String> = None;
            let mut sent_ok = 0usize;

            if has_dir {
                match client.transfer_via_remote_control(&valid_paths, device_id.as_deref()) {
                    Ok(val) if val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) => {
                        sent_ok = valid_paths.len();
                    }
                    Ok(val) => {
                        let msg = val
                            .get("error")
                            .or_else(|| val.get("message"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("传输被拒绝");
                        last_err = Some(friendly_bridge_error(msg));
                    }
                    Err(e) => last_err = Some(friendly_bridge_error(&e)),
                }
            } else {
                for p in &valid_paths {
                    match client.transfer_file(p, device_id.as_deref(), Some(180_000)) {
                        Ok(val) if val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) => {
                            sent_ok += 1;
                            if resp_dev_name.is_none() {
                                resp_dev_name = val
                                    .get("device")
                                    .and_then(|d| d.get("device_name"))
                                    .and_then(|s| s.as_str())
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty());
                            }
                        }
                        Ok(val) => {
                            let msg = val
                                .get("upload")
                                .and_then(|u| u.get("message"))
                                .or_else(|| val.get("error"))
                                .or_else(|| val.get("code"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("传输失败");
                            last_err = Some(friendly_bridge_error(msg));
                        }
                        Err(e) => last_err = Some(friendly_bridge_error(&e)),
                    }
                }
            }

            done_flag.store(true, Ordering::Relaxed);
            let elapsed = started.elapsed().as_secs_f64();

            if sent_ok > 0 && last_err.is_none() {
                record_transfer(total_bytes, elapsed);
                let _ = tx.send(TransferEvent::Progress(1.0));
                let dev_sfx = resp_dev_name
                    .as_deref()
                    .map(|n| format!("至 {}", n))
                    .unwrap_or(target_suffix);
                let _ = tx.send(TransferEvent::Done {
                    ok: true,
                    message: format!("OPPO 互联已静默发送 {} 个文件{}", sent_ok, dev_sfx),
                });
            } else if sent_ok > 0 {
                let _ = tx.send(TransferEvent::Progress(1.0));
                let _ = tx.send(TransferEvent::Done {
                    ok: false,
                    message: format!(
                        "OPPO 互联部分发送成功（{}/{}）：{}",
                        sent_ok,
                        valid_paths.len(),
                        last_err.unwrap_or_default()
                    ),
                });
            } else {
                let _ = tx.send(TransferEvent::Done {
                    ok: false,
                    message: format!(
                        "OPPO 互联发送失败：{}",
                        last_err.unwrap_or_else(|| "连接中断".to_string())
                    ),
                });
            }
        });
    rx
}
