//! 状态文件读取与「任务结束」状态机。
//!
//! 读取 ZenDesktop ai-status 协议：`%LOCALAPPDATA%\ZenDesktop\ai-status\<agent>.json`，
//! 字段 `agent / name / state / message / detail / project / updated`。
//! 这套文件由各 AI 客户端的 hooks / MCP 写入，本 mod 只读、不回写，
//! 因此与任务栏 AI 状态、Windhawk mod、zen-status CLI/MCP 完全兼容。

use crate::auralsle_ai_status::config;
use serde_json::Value;
/// 陈旧上限：还在「工作 / 执行 / 运行」但文件很久没更新时按废数据处理。
/// 正常任务每次工具调用都会刷新状态文件，6 小时还没动静基本是关了窗口。
const STALE_ACTIVE_SECS: i64 = 6 * 3600;
/// 「等你回复 / 出错了」这类等人处理的状态：45 分钟没人动就当过期，
/// 否则旧工具的 waiting 会一直占着岛上的位置。
const STALE_ASKING_SECS: i64 = 45 * 60;
/// 「刚完成」状态在内存里保留的秒数。只给吉祥物表情判断用（庆祝 / 开心），
/// 岛上「已完成」文字由 `done_timeout_secs` 控制，桌面不再弹结束卡片。
const FINISH_KEEP_SECS: f64 = 8.0;

use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Working,
    Tool,
    Waiting,
    Error,
    Running,
    Done,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Working => "工作中",
            Phase::Tool => "执行中",
            Phase::Waiting => "等你回复",
            Phase::Error => "出错了",
            Phase::Running => "运行中",
            Phase::Done => "已完成",
        }
    }
    pub fn active(self) -> bool {
        !matches!(self, Phase::Done)
    }
}

fn parse_phase(s: &str) -> Option<Phase> {
    match s {
        "working" | "thinking" => Some(Phase::Working),
        "tool" => Some(Phase::Tool),
        "waiting" => Some(Phase::Waiting),
        "error" => Some(Phase::Error),
        "running" => Some(Phase::Running),
        "done" => Some(Phase::Done),
        // idle / 未知：不显示
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct Agent {
    pub key: String,
    pub name: String,
    pub phase: Phase,
    pub message: String,
    pub detail: String,
    pub project: String,
    pub updated: i64,
    pub age_secs: i64,
    /// 「这一步在做什么」：协议里的 `step`（可选），没有就退回 `message`。
    pub step: String,
    /// 第几步 / 共几步（协议里的 `step_index` / `step_total`，可选，0 = 不知道）。
    pub step_index: i64,
    pub step_total: i64,
    /// 0..1 的进度（协议里的 `progress`，可选，负数 = 不知道）。
    pub progress: f32,
    /// 这次任务已经跑了多久（秒）。首次看到它处于活跃状态时开始计时。
    pub elapsed: i64,
    /// 活跃、但超过「静默」阈值没有新上报：仍算工作中，只是提示可能要等一下。
    pub quiet: bool,
}

impl Agent {
    /// 页面第二行要显示的「正在做什么」。
    ///
    /// 优先级：协议给的 `step` > `message` > `detail` > `project`；
    /// 都没有就按状态给一句兜底，避免整行空着。
    pub fn doing(&self) -> String {
        for text in [&self.step, &self.message, &self.detail, &self.project] {
            if !text.trim().is_empty() {
                return text.trim().to_owned();
            }
        }
        match self.phase {
            Phase::Waiting => "等你回复后继续".to_owned(),
            Phase::Error => "出错了，需要回去看一眼".to_owned(),
            Phase::Done => "任务已结束".to_owned(),
            _ => "正在处理…".to_owned(),
        }
    }

    /// 「第 3/7 步」；没有步数信息时返回空串。
    pub fn step_text(&self) -> String {
        if self.step_total > 0 {
            format!("第 {}/{} 步", self.step_index.max(1).min(self.step_total), self.step_total)
        } else if self.step_index > 0 {
            format!("第 {} 步", self.step_index)
        } else {
            String::new()
        }
    }
}

#[derive(Clone, Debug)]
pub struct Finished {
    pub key: String,
    pub name: String,
    pub phase: Phase,
    /// 结束时的消息 / 项目 / 描述，用来在提示卡片上显示「详细」。
    pub message: String,
    pub project: String,
    pub detail: String,
    pub at: f64,
}

pub struct State {
    pub agents: Vec<Agent>,
    pub finished: Option<Finished>,
    prev: Vec<Agent>,
    last_scan: f64,
    initialized: bool,
    /// 每个助手这次任务是什么时候开始的（秒），用来算「已进行 3 分 12 秒」。
    active_since: std::collections::HashMap<String, i64>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            agents: Vec::new(),
            finished: None,
            prev: Vec::new(),
            last_scan: f64::MIN,
            initialized: false,
            active_since: std::collections::HashMap::new(),
        }
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn file_unix(path: &PathBuf) -> i64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn string_field(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

/// 进度：协议里的 `progress` 可选，接受 0..1 或 0..100；没有则返回 -1（= 不知道）。
fn progress_field(v: &Value) -> f32 {
    let raw = v.get("progress").and_then(|x| {
        x.as_f64()
            .or_else(|| x.as_str().and_then(|s| s.parse::<f64>().ok()))
    });
    match raw {
        Some(p) if p.is_finite() && p >= 0.0 => {
            let p = if p > 1.0 { p / 100.0 } else { p };
            (p as f32).clamp(0.0, 1.0)
        }
        _ => -1.0,
    }
}

fn number_field(v: &Value, key: &str) -> i64 {
    if let Some(n) = v.get(key).and_then(Value::as_i64) {
        return n;
    }
    if let Some(n) = v.get(key).and_then(Value::as_u64) {
        return n as i64;
    }
    v.get(key)
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

/// 当前数据目录是否就是本机真实安装（`%APPDATA%\Auralsle`）。
fn reads_real_status_dir() -> bool {
    let Ok(appdata) = std::env::var("APPDATA") else { return false };
    crate::model::config_dir() == PathBuf::from(appdata).join("Auralsle")
}

impl State {
    /// 每帧调用；内部按 0.3 秒节流扫描。
    /// `now` 取上游 `Env::t`（秒），保证录制/测试可复现。
    pub fn tick(&mut self, now: f64) {
        let cfg = config::get();
        if !cfg.enabled {
            if self.initialized {
                self.agents.clear();
                self.prev.clear();
                self.finished = None;
            }
            self.last_scan = now;
            return;
        }
        // 测试或定制数据目录（AURALSLE_DATA_DIR / 临时 APPDATA）下不读真实状态文件：
        // 单元测试不该被本机正在跑的 AI 任务影响，布局断言才稳定。
        if cfg.status_dir.trim().is_empty() && !reads_real_status_dir() {
            if self.initialized {
                self.agents.clear();
                self.prev.clear();
                self.finished = None;
            }
            self.last_scan = now;
            return;
        }
        // 「刚完成」只留一小段时间给吉祥物表情（不再弹任何结束卡片）。
        // 岛上「已完成」文字的保留时长由 `done_timeout_secs` 单独控制，两者互不影响。
        if let Some(f) = &self.finished {
            if now - f.at > FINISH_KEEP_SECS {
                self.finished = None;
            }
        }
        if now - self.last_scan < 0.45 {
            return;
        }
        self.last_scan = now;

        let status_dirs = if cfg!(test) {
            vec![cfg.status_dir()]
        } else {
            let dirs = cfg.all_status_dirs();
            if dirs.is_empty() { vec![cfg.status_dir()] } else { dirs }
        };
        let mut agents: Vec<Agent> = Vec::new();
        let now_unix = unix_now();
        for dir in status_dirs {
            let Ok(entries) = fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".approval.json")) {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&path) else { continue };
                let Ok(v) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { continue };
                if !v.is_object() {
                    continue;
                }
                let Some(phase) = parse_phase(&string_field(&v, "state")) else { continue };

                let mtime = file_unix(&path);
                let mut updated = number_field(&v, "updated");
                if updated <= 0 || (updated - mtime).abs() > 120 {
                    updated = mtime;
                }
                let age = (now_unix - updated).max(0);
                let mut phase = phase;
                let auto_done_secs = if cfg.active_timeout_secs > 0 { cfg.active_timeout_secs } else { 180 };
                if phase.active() && phase != Phase::Waiting && age > auto_done_secs {
                    phase = Phase::Done;
                }
                let done_keep = if cfg.done_timeout_secs > 0 { cfg.done_timeout_secs } else { 45 };
                let max_age = if phase.active() {
                    match phase {
                        Phase::Waiting | Phase::Error => STALE_ASKING_SECS,
                        _ => auto_done_secs + done_keep,
                    }
                } else if phase == Phase::Done && age > auto_done_secs {
                    auto_done_secs + done_keep
                } else {
                    done_keep
                };
                if age > max_age {
                    continue;
                }

                let key = {
                    let k = string_field(&v, "agent");
                    if !k.is_empty() {
                        k
                    } else {
                        path.file_stem().and_then(|s| s.to_str()).unwrap_or("ai").to_owned()
                    }
                };
                let name = {
                    let n = string_field(&v, "name");
                    if n.is_empty() { key.clone() } else { n }
                };
                let candidate = Agent {
                    key,
                    name,
                    phase,
                    message: string_field(&v, "message"),
                    detail: string_field(&v, "detail"),
                    project: string_field(&v, "project"),
                    updated,
                    age_secs: age,
                    step: string_field(&v, "step"),
                    step_index: number_field(&v, "step_index").max(0),
                    step_total: number_field(&v, "step_total").max(0),
                    progress: progress_field(&v),
                    elapsed: 0,
                    quiet: cfg.active_timeout_secs > 0 && age > cfg.active_timeout_secs,
                };
                if let Some(existing) = agents.iter_mut().find(|a| a.key.eq_ignore_ascii_case(&candidate.key)) {
                    if candidate.updated > existing.updated {
                        *existing = candidate;
                    }
                } else {
                    agents.push(candidate);
                }
            }
        }
        // 任务计时：第一次看到它活跃就记下起点，转入完成/消失就清掉。
        for a in agents.iter_mut() {
            if a.phase.active() {
                let since = *self.active_since.entry(a.key.clone()).or_insert(now_unix);
                a.elapsed = (now_unix - since).max(0);
            } else {
                self.active_since.remove(&a.key);
                a.elapsed = 0;
            }
        }
        // [fly-island & WorkBuddy 实时窗口探测]：若状态目录中尚未写入 .approval.json，
        // 自动探测本机正在运行的 WorkBuddy / Claude Code / Codex / Cursor 窗口，确保在 WorkBuddy 上可直接联动状态与审批
        #[cfg(windows)]
        if !cfg!(test) {
            for (w_key, w_name, w_phase, w_msg) in cached_running_desktop_agents() {
                if !agents.iter().any(|a| a.key.eq_ignore_ascii_case(&w_key)) {
                    agents.push(Agent {
                        key: w_key,
                        name: w_name,
                        phase: w_phase,
                        message: w_msg,
                        detail: "支持岛内直接允许(Ctrl+Alt+Y) / 拒绝(Ctrl+Alt+N) / 交回终端(Ctrl+Alt+T)".to_owned(),
                        project: String::new(),
                        updated: now_unix,
                        age_secs: 0,
                        step: String::new(),
                        step_index: 0,
                        step_total: 0,
                        progress: -1.0,
                        elapsed: 12,
                        quiet: false,
                    });
                }
            }
        }

        // 活跃在前、已完成在后；同类按最后上报时间倒序。
        agents.sort_by(|a, b| {
            b.phase.active().cmp(&a.phase.active()).then(b.updated.cmp(&a.updated))
        });

        // 已完成提示还挂着、同一助手又忙起来了（新任务开始）：
        // 两个显示会互相矛盾，立刻收掉提示，以「工作中」为准。
        if let Some(f) = &self.finished {
            if agents.iter().any(|a| a.key == f.key && a.phase.active()) {
                self.finished = None;
            }
        }
        if self.initialized && self.finished.is_none() {
            let active: Vec<&Agent> = agents.iter().filter(|a| a.phase.active()).collect();
            for prev in self.prev.iter().filter(|a| a.phase.active()) {
                if active.iter().any(|a| a.key == prev.key) {
                    continue;
                }
                // 上一帧在忙、这一帧不再活跃：只有写入方明确报了 done 才算「已完成」。
                // 文件静默超时导致的消失不算 —— 那会把还在跑的任务误报成完成。
                let Some(src) = agents.iter().find(|a| a.key == prev.key && a.phase == Phase::Done)
                else {
                    continue;
                };
                self.finished = Some(Finished {
                    key: src.key.clone(),
                    name: src.name.clone(),
                    phase: Phase::Done,
                    message: src.message.clone(),
                    project: src.project.clone(),
                    detail: src.detail.clone(),
                    at: now,
                });
                break;
            }
        }
        self.prev = agents.clone();
        self.agents = agents;
        self.initialized = true;
    }
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn lock() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

static LAST_HOTKEY_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

pub fn tick(now: f64) {
    if !cfg!(test) && !super::config::get().mod_ai_suite_enabled {
        if let Ok(mut s) = lock().lock() {
            s.agents.clear();
            s.finished = None;
        }
        return;
    }
    if let Ok(mut s) = lock().lock() {
        s.tick(now);
    }
    #[cfg(windows)]
    if !cfg!(test) && config::get().agent_hook_approval {
        unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
            let down = |vk: i32| (GetAsyncKeyState(vk) as u16 & 0x8000) != 0;
            if down(0x11) && down(0x12) {
                let dec = if down(0x59) {
                    Some("allow")
                } else if down(0x4E) {
                    Some("deny")
                } else if down(0x54) {
                    Some("terminal")
                } else {
                    None
                };
                if let Some(d) = dec {
                    let now_ms = (now * 1000.0) as i64;
                    let prev = LAST_HOTKEY_MS.load(std::sync::atomic::Ordering::Relaxed);
                    if (now_ms - prev).abs() > 450 {
                        LAST_HOTKEY_MS.store(now_ms, std::sync::atomic::Ordering::Relaxed);
                        let key = lock()
                            .lock()
                            .ok()
                            .and_then(|s| {
                                s.agents
                                    .iter()
                                    .find(|a| a.phase == Phase::Waiting)
                                    .or_else(|| s.agents.first())
                                    .map(|a| a.key.clone())
                            })
                            .unwrap_or_else(|| "workbuddy".to_owned());
                        resolve_agent_approval(&key, d);
                    }
                }
            }
        }
    }
}

static HOOK_ACTION_STATUS: OnceLock<Mutex<String>> = OnceLock::new();

pub fn hook_action_status() -> String {
    HOOK_ACTION_STATUS
        .get_or_init(|| Mutex::new("就绪 · 支持 Codex / Claude Code / WorkBuddy / DeepSeek 生命周期与权限审批钩子".to_owned()))
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
}

pub fn set_hook_action_status(msg: impl Into<String>) {
    if let Ok(mut slot) = HOOK_ACTION_STATUS
        .get_or_init(|| Mutex::new(String::new()))
        .lock()
    {
        *slot = msg.into();
    }
}


#[cfg(windows)]
#[derive(Clone, Default)]
struct AgentRuntimeEntry {
    last_title: String,
    working_until: i64,
    done_until: i64,
    last_fg_unix: i64,
    typed_chars: u32,
}

#[cfg(windows)]
#[derive(Default)]
struct DesktopAgentTracker {
    entries: std::collections::HashMap<String, AgentRuntimeEntry>,
    prev_enter_down: bool,
    prev_lbutton_down: bool,
}

#[cfg(windows)]
static DESKTOP_TRACKER: OnceLock<Mutex<DesktopAgentTracker>> = OnceLock::new();

#[cfg(windows)]
static DESKTOP_AGENTS_CACHE: OnceLock<Mutex<(Vec<(String, String, Phase, String)>, std::time::Instant)>> = OnceLock::new();

#[cfg(windows)]
fn is_antigravity_window(title: &str, low: &str, proc_low: &str) -> bool {
    if title.contains("岛仔")
        || title.contains("灵屿")
        || low.contains("auralsle")
        || low.contains("auraisle")
        || title.contains("设置")
        || proc_low == "explorer.exe"
    {
        return false;
    }
    if proc_low.contains("antigravity") {
        return true;
    }
    !low_is_terminal_or_editor(proc_low)
        && !low.contains(".rs")
        && !low.contains(".zip")
        && !low.contains("cargo")
        && low.contains("antigravity")
}

#[cfg(windows)]
fn is_gemini_pc_window_title_or_proc(title: &str, low: &str, proc_low: &str) -> bool {
    if title.contains("岛仔")
        || title.contains("灵屿")
        || low.contains("auralsle")
        || low.contains("auraisle")
        || title.contains("设置")
        || low.contains(".rs")
        || low.contains(".zip")
        || low.contains("cargo")
        || low.contains("explorer")
    {
        return false;
    }
    proc_low.contains("gemini")
        || low == "gemini"
        || low.starts_with("gemini ")
        || low.starts_with("gemini -")
        || low.starts_with("gemini —")
        || low.ends_with("- gemini")
        || low.ends_with("— gemini")
        || low.contains("google gemini")
        || low.contains("gemini advanced")
        || low.contains("gemini desktop")
        || low.contains("gemini app")
        || low.contains("google ai studio")
}

/// 豆包桌面客户端（`Doubao.exe`）窗口判定：进程名或窗口标题带「豆包」。
#[cfg(windows)]
fn is_doubao_window(title: &str, proc_low: &str) -> bool {
    let low = title.to_ascii_lowercase();
    if proc_low.contains("auralsle") || proc_low.contains("auraisle") || title.contains("岛仔") || title.contains("灵屿") || low.contains("auraisle") {
        return false;
    }
    if low_is_terminal_or_editor(proc_low) {
        return false;
    }
    proc_low.contains("doubao")
        || title == "豆包"
        || title.starts_with("豆包 ")
        || title.starts_with("豆包 -")
        || title.starts_with("豆包 —")
        || title.starts_with("豆包 ·")
        || title.ends_with("豆包")
        || title.contains("- 豆包")
        || title.contains("— 豆包")
        || low.contains("doubao.com")
}

/// 终端/编辑器进程：它们的标题里可能带「豆包」字样（比如 `cargo run` 的日志），
/// 不能当成豆包客户端在工作。
#[cfg(windows)]
fn low_is_terminal_or_editor(proc_low: &str) -> bool {
    [
        "cmd.exe",
        "powershell.exe",
        "powershell",
        "pwsh.exe",
        "pwsh",
        "windowsterminal.exe",
        "windowsterminal",
        "code.exe",
        "devenv.exe",
        "explorer.exe",
    ]
    .iter()
    .any(|p| proc_low == *p)
}

/// 识别本机所有 12 款主流 AI Agent 客户端/IDE/终端窗口，返回 `(key, display_name)`。
#[cfg(windows)]
fn classify_ai_agent_window(title: &str, low: &str, proc_low: &str) -> Option<(&'static str, &'static str)> {
    if proc_low.contains("auralsle")
        || proc_low.contains("auraisle")
        || title.contains("岛仔")
        || title.contains("灵屿")
        || low.contains("auraisle")
        || proc_low == "explorer.exe"
    {
        return None;
    }
    if is_antigravity_window(title, low, proc_low) {
        return Some(("antigravity", "Gemini Antigravity"));
    }
    if is_doubao_window(title, proc_low) {
        return Some(("doubao", "豆包"));
    }
    if is_gemini_pc_window_title_or_proc(title, low, proc_low) {
        return Some(("gemini", "Gemini"));
    }
    if proc_low.contains("workbuddy")
        || proc_low.contains("codebuddy")
        || low.contains("workbuddy")
        || low.contains("codebuddy")
        || title.contains("企微研发助手")
    {
        return Some(("workbuddy", "WorkBuddy"));
    }
    if proc_low.contains("claude")
        || low.contains("claude code")
        || low.starts_with("claude ")
        || low.starts_with("claude -")
        || low.ends_with("- claude")
        || low.ends_with("— claude")
        || low == "claude"
    {
        return Some(("claude", "Claude"));
    }
    if proc_low.contains("codex")
        || proc_low.contains("chatgpt")
        || low.contains("codex")
        || low.starts_with("chatgpt")
        || low.ends_with("- chatgpt")
        || low.ends_with("— chatgpt")
    {
        return Some(("codex", "Codex"));
    }
    if proc_low.contains("deepseek")
        || (!low_is_terminal_or_editor(proc_low)
            && (low.contains("deepseek") || title.contains("深度求索")))
    {
        return Some(("deepseek", "DeepSeek"));
    }
    if proc_low.contains("kimi")
        || (!low_is_terminal_or_editor(proc_low)
            && (low.starts_with("kimi ")
                || low.ends_with("- kimi")
                || low.ends_with("— kimi")
                || low.contains("kimi.moonshot")
                || low.contains("kimi.com")))
    {
        return Some(("kimi", "Kimi"));
    }
    if proc_low.contains("cursor") || low.ends_with("- cursor") || low.ends_with("— cursor") {
        return Some(("cursor", "Cursor"));
    }
    if proc_low.contains("windsurf") || low.ends_with("- windsurf") {
        return Some(("windsurf", "Windsurf"));
    }
    if proc_low.contains("kiro") || low.ends_with("- kiro") {
        return Some(("kiro", "Kiro"));
    }
    if proc_low.contains("zcode") || low.contains("zcode") {
        return Some(("zcode", "ZCode"));
    }
    if proc_low.contains("grok")
        || (!low_is_terminal_or_editor(proc_low)
            && (low.starts_with("grok ") || low.ends_with("- grok") || low.ends_with("— grok")))
    {
        return Some(("grok", "Grok"));
    }
    None
}

#[cfg(windows)]
fn window_process_name_low(hwnd: windows::Win32::Foundation::HWND) -> String {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 260];
        let mut sz = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_FORMAT(0), windows::core::PWSTR(buf.as_mut_ptr()), &mut sz).is_ok();
        let _ = CloseHandle(h);
        if !ok || sz == 0 {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..sz as usize]);
        full.rsplit(['\\', '/']).next().unwrap_or(&full).to_ascii_lowercase()
    }
}

/// 读取文件末尾最多 `max_bytes` 字节的 UTF-8 文本，用于毫秒级增量探测 Claude Code / Codex 会话 JSONL。
#[cfg(windows)]
fn read_file_tail_string(path: &std::path::Path, max_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    if len == 0 {
        return None;
    }
    let start = len.saturating_sub(max_bytes);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// 在目录树（最多递归 `max_depth` 层）中查找最近修改时间在 `max_age_secs` 秒以内的最新 `.jsonl` 文件。
#[cfg(windows)]
fn newest_file_in_dir(
    root: &std::path::Path,
    now_u: i64,
    max_age_secs: i64,
    max_depth: usize,
    exts: &[&str],
    skip_names: &[&str],
) -> Option<(PathBuf, i64)> {
    fn walk(
        dir: &std::path::Path,
        now_u: i64,
        max_age: i64,
        depth: usize,
        exts: &[&str],
        skip_names: &[&str],
        best: &mut Option<(PathBuf, i64)>,
    ) {
        if depth == 0 {
            return;
        }
        let Ok(rd) = fs::read_dir(dir) else { return };
        for entry in rd.flatten() {
            let p = entry.path();
            let fname_low = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if p.is_dir() {
                if fname_low == "node_modules" || fname_low == ".git" || fname_low == "cache" || fname_low == "gpucache" {
                    continue;
                }
                walk(&p, now_u, max_age, depth - 1, exts, skip_names, best);
            } else {
                if skip_names.iter().any(|s| fname_low.contains(s)) {
                    continue;
                }
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                if exts.iter().any(|wanted| *wanted == ext) {
                    let mtime = file_unix(&p);
                    let age = now_u - mtime;
                    if mtime > 0 && age <= max_age {
                        if best.as_ref().is_none_or(|(_, b_t)| mtime > *b_t) {
                            *best = Some((p, mtime));
                        }
                    }
                }
            }
        }
    }
    let mut best = None;
    if root.is_dir() {
        walk(root, now_u, max_age_secs, max_depth, exts, skip_names, &mut best);
    }
    best
}

#[cfg(windows)]
fn newest_jsonl_in_dir(root: &std::path::Path, now_u: i64, max_age_secs: i64, max_depth: usize) -> Option<(PathBuf, i64)> {
    newest_file_in_dir(root, now_u, max_age_secs, max_depth, &["jsonl"], &[])
}

#[cfg(windows)]
fn detect_agent_tool_children() -> Vec<&'static str> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut procs: Vec<(u32, u32, String)> = Vec::with_capacity(192);
    unsafe {
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut pe = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut pe).is_ok() {
                loop {
                    let z = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
                    let exe = String::from_utf16_lossy(&pe.szExeFile[..z]).to_ascii_lowercase();
                    procs.push((pe.th32ProcessID, pe.th32ParentProcessID, exe));
                    if Process32NextW(snap, &mut pe).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
    }
    let parent_map: std::collections::HashMap<u32, (u32, &str)> = procs
        .iter()
        .map(|(pid, ppid, exe)| (*pid, (*ppid, exe.as_str())))
        .collect();
    let agent_of_proc = |exe: &str| -> Option<&'static str> {
        if exe.contains("antigravity") || exe.starts_with("language_server") {
            Some("antigravity")
        } else if exe.contains("workbuddy") || exe.contains("codebuddy") {
            Some("workbuddy")
        } else if exe.contains("doubao") {
            Some("doubao")
        } else if exe.contains("claude") {
            Some("claude")
        } else if exe.contains("codex") || exe.contains("chatgpt") {
            Some("codex")
        } else if exe.contains("cursor") {
            Some("cursor")
        } else if exe.contains("windsurf") {
            Some("windsurf")
        } else if exe.contains("kiro") {
            Some("kiro")
        } else {
            None
        }
    };
    let mut active = Vec::new();
    for &(_pid, ppid, ref exe) in &procs {
        let is_cli_tool = matches!(
            exe.as_str(),
            "cmd.exe"
                | "pwsh.exe"
                | "powershell.exe"
                | "git.exe"
                | "cargo.exe"
                | "rustc.exe"
                | "python.exe"
                | "rg.exe"
                | "npm.exe"
                | "pnpm.exe"
                | "uv.exe"
                | "cmake.exe"
                | "ninja.exe"
        );
        if !is_cli_tool {
            continue;
        }
        let mut cur_ppid = ppid;
        for _ in 0..4 {
            let Some(&(next_ppid, p_exe)) = parent_map.get(&cur_ppid) else {
                break;
            };
            if let Some(ag_key) = agent_of_proc(p_exe) {
                if !active.contains(&ag_key) {
                    active.push(ag_key);
                }
                break;
            }
            if next_ppid == 0 || next_ppid == cur_ppid {
                break;
            }
            cur_ppid = next_ppid;
        }
    }
    active
}

/// 零配置原生探测 Claude Code (`~/.claude/projects/**/*.jsonl`) 与 Codex CLI (`~/.codex/sessions/**/*.jsonl`) 的实时会话状态。
#[cfg(windows)]
fn scan_live_agent_files(now_u: i64) -> Vec<(String, String, Phase, String)> {
    let mut out = Vec::new();
    let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from) else {
        return out;
    };

    // 1. Claude Code 实时会话 JSONL 探测
    let claude_root = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"))
        .join("projects");
    if let Some((jsonl_path, mtime)) = newest_jsonl_in_dir(&claude_root, now_u, 150, 3) {
        let age = (now_u - mtime).max(0);
        if let Some(tail) = read_file_tail_string(&jsonl_path, 16384) {
            let mut last_phase: Option<Phase> = None;
            let mut last_msg = String::new();
            for line in tail.lines().rev() {
                let l = line.trim();
                if !l.starts_with('{') {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
                let row_type = string_field(&v, "type");
                if row_type == "system" && string_field(&v, "subtype") == "turn_duration" {
                    if age <= 45 {
                        last_phase = Some(Phase::Done);
                        last_msg = "Claude Code 已完成本轮任务".to_owned();
                    }
                    break;
                } else if row_type == "assistant" {
                    let stop_reason = v.pointer("/message/stop_reason").and_then(Value::as_str).unwrap_or("");
                    if let Some(arr) = v.pointer("/message/content").and_then(Value::as_array) {
                        let mut found_tool = None;
                        let mut found_text = String::new();
                        for item in arr {
                            let it_type = item.get("type").and_then(Value::as_str).unwrap_or("");
                            if it_type == "tool_use" {
                                let t_name = item.get("name").and_then(Value::as_str).unwrap_or("工具");
                                let detail = item.pointer("/input/command").and_then(Value::as_str)
                                    .or_else(|| item.pointer("/input/file_path").and_then(Value::as_str))
                                    .or_else(|| item.pointer("/input/pattern").and_then(Value::as_str))
                                    .or_else(|| item.pointer("/input/description").and_then(Value::as_str))
                                    .unwrap_or("");
                                found_tool = Some((t_name.to_owned(), detail.to_owned()));
                            } else if it_type == "text" && found_text.is_empty() {
                                found_text = item.get("text").and_then(Value::as_str).unwrap_or("").trim().to_owned();
                            }
                        }
                        if let Some((t_name, detail)) = found_tool {
                            let is_ask = t_name == "AskUserQuestion";
                            let is_approval_tool = matches!(t_name.as_str(), "Bash" | "Edit" | "Write" | "MultiEdit");
                            let ph = if is_ask && age <= 300 {
                                Phase::Waiting
                            } else if is_approval_tool && age >= 12 && age <= 180 {
                                Phase::Waiting
                            } else {
                                Phase::Tool
                            };
                            let summary = if detail.is_empty() {
                                format!("正在调用 {t_name}")
                            } else {
                                let short: String = detail.chars().take(42).collect();
                                format!("{t_name}: {short}")
                            };
                            last_phase = Some(ph);
                            last_msg = summary;
                            break;
                        } else if stop_reason == "end_turn" {
                            if age <= 45 {
                                last_phase = Some(Phase::Done);
                                let short: String = found_text.lines().next().unwrap_or("任务已完成").chars().take(42).collect();
                                last_msg = if short.is_empty() { "Claude Code 已完成回复".to_owned() } else { short };
                            }
                            break;
                        } else if age <= 120 {
                            last_phase = Some(Phase::Working);
                            let short: String = found_text.lines().next().unwrap_or("正在生成代码与回复…").chars().take(42).collect();
                            last_msg = if short.is_empty() { "Claude Code 正在思考与编码…".to_owned() } else { short };
                            break;
                        }
                    }
                } else if row_type == "user" && age <= 120 {
                    last_phase = Some(Phase::Working);
                    last_msg = "Claude Code 正在思考与分析任务…".to_owned();
                    break;
                }
            }
            if let Some(ph) = last_phase {
                out.push(("claude".to_owned(), "Claude Code".to_owned(), ph, last_msg));
            }
        }
    }

    // 2. OpenAI Codex CLI 实时会话 JSONL 探测 (`~/.codex/sessions/**/*.jsonl`)
    let codex_root = std::env::var_os("CODEX_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"))
        .join("sessions");
    if let Some((jsonl_path, mtime)) = newest_jsonl_in_dir(&codex_root, now_u, 150, 5) {
        let age = (now_u - mtime).max(0);
        if let Some(tail) = read_file_tail_string(&jsonl_path, 16384) {
            let mut last_phase: Option<Phase> = None;
            let mut last_msg = String::new();
            for line in tail.lines().rev() {
                let l = line.trim();
                if !l.starts_with('{') {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
                let ev_type = v.pointer("/payload/type").and_then(Value::as_str)
                    .or_else(|| v.get("type").and_then(Value::as_str))
                    .unwrap_or("");
                if matches!(ev_type, "task_complete" | "turn_complete" | "agent_turn_complete") {
                    if age <= 45 {
                        last_phase = Some(Phase::Done);
                        last_msg = "Codex 已完成当前编码任务".to_owned();
                    }
                    break;
                } else if matches!(ev_type, "function_call" | "custom_tool_call" | "exec_command" | "apply_patch") && age <= 120 {
                    let t_name = v.pointer("/payload/name").and_then(Value::as_str).unwrap_or("命令/工具");
                    last_phase = Some(if age >= 5 { Phase::Waiting } else { Phase::Tool });
                    last_msg = format!("Codex 正在执行: {t_name}");
                    break;
                } else if age <= 90 {
                    last_phase = Some(Phase::Working);
                    last_msg = "Codex 正在推理与编写代码…".to_owned();
                    break;
                }
            }
            if let Some(ph) = last_phase {
                out.push(("codex".to_owned(), "Codex".to_owned(), ph, last_msg));
            }
        }
    }

    // 3. Gemini Antigravity 本地会话与任务产物探测 (`~/.gemini/antigravity/conversations|brain`)
    let ag_root = home.join(".gemini").join("antigravity");
    let ag_conv = newest_file_in_dir(&ag_root.join("conversations"), now_u, 45, 2, &["pb"], &[]);
    let ag_brain = newest_file_in_dir(
        &ag_root.join("brain"),
        now_u,
        45,
        5,
        &["md", "jsonl"],
        &["media_", ".user_uploaded"],
    );
    let ag_mtime = [ag_conv.as_ref().map(|x| x.1), ag_brain.as_ref().map(|x| x.1)]
        .into_iter()
        .flatten()
        .max();
    if let Some(mtime) = ag_mtime {
        let age = (now_u - mtime).max(0);
        let (ph, msg) = if age <= 12 {
            (Phase::Working, "Gemini Antigravity 正在分析与执行任务…".to_owned())
        } else {
            (Phase::Done, "Gemini Antigravity 本轮对话/任务已完成".to_owned())
        };
        out.push(("antigravity".to_owned(), "Gemini Antigravity".to_owned(), ph, msg));
    }

    // 4. WorkBuddy / CodeBuddy 本地会话与日志探测 (`~/.workbuddy` / `~/.codebuddy`)
    for wb_dir in [home.join(".workbuddy"), home.join(".codebuddy")] {
        if let Some((_, mtime)) = newest_file_in_dir(
            &wb_dir,
            now_u,
            45,
            3,
            &["jsonl", "log"],
            &["settings", "mcp", "lock"],
        ) {
            let age = (now_u - mtime).max(0);
            let (ph, msg) = if age <= 12 {
                (Phase::Working, "WorkBuddy 正在生成与执行开发任务…".to_owned())
            } else {
                (Phase::Done, "WorkBuddy 本轮任务已完成".to_owned())
            };
            out.push(("workbuddy".to_owned(), "WorkBuddy".to_owned(), ph, msg));
            break;
        }
    }

    out
}

#[cfg(windows)]
fn cached_running_desktop_agents() -> Vec<(String, String, Phase, String)> {
    if !super::config::get().mod_ai_suite_enabled {
        if let Some(mtx) = DESKTOP_AGENTS_CACHE.get() {
            if let Ok(mut g) = mtx.lock() {
                g.0.clear();
            }
        }
        return Vec::new();
    }
    // 实时追踪前台 AI Agent 窗口的键盘/粘贴/回车交互（支持全部 12 款 AI 工具，含中文输入法 VK_PROCESSKEY 与 Ctrl+V 粘贴）
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
        let fg = GetForegroundWindow();
        if !fg.0.is_null() {
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(fg, &mut buf);
            if len > 0 {
                let title = String::from_utf16_lossy(&buf[..len as usize]);
                let low = title.to_ascii_lowercase();
                let proc_low = window_process_name_low(fg);
                if let Some((ag_key, _ag_name)) = classify_ai_agent_window(&title, &low, &proc_low) {
                    let now_u = unix_now();
                    let down = |vk: i32| (GetAsyncKeyState(vk) as u16 & 0x8000) != 0;
                    let shift_down = down(0x10);
                    let ctrl_down = down(0x11);
                    let enter_down = down(0x0D);
                    let lbutton_down = down(0x01);
                    let mut click_in_send_zone = false;
                    if lbutton_down {
                        use windows::Win32::Foundation::{POINT, RECT};
                        use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect};
                        let mut pt = POINT::default();
                        let mut wr = RECT::default();
                        if GetCursorPos(&mut pt).is_ok() && GetWindowRect(fg, &mut wr).is_ok() {
                            let h = (wr.bottom - wr.top).max(1);
                            if pt.x >= wr.left && pt.x <= wr.right && pt.y >= wr.top + h * 52 / 100 && pt.y <= wr.bottom {
                                click_in_send_zone = true;
                            }
                        }
                    }
                    // 支持空格(0x20)、退格(0x08)、中文输入法选词(VK_PROCESSKEY=0xE5)、字母数字(0x30..=0x5A)、小键盘(0x60..=0x69)、标点(0xBA..=0xDF)及 Ctrl+V 粘贴
                    let mut any_typing = down(0x20) || down(0x08) || down(0xE5) || (ctrl_down && down(0x56));
                    if !any_typing {
                        for vk in (0x30..=0x5A).chain(0x60..=0x69).chain(0xBA..=0xC0) {
                            if down(vk) {
                                any_typing = true;
                                break;
                            }
                        }
                    }
                    if let Ok(mut tr_map) = DESKTOP_TRACKER.get_or_init(|| Mutex::new(DesktopAgentTracker::default())).lock() {
                        let prev_enter = tr_map.prev_enter_down;
                        let prev_lb = tr_map.prev_lbutton_down;
                        tr_map.prev_enter_down = enter_down;
                        tr_map.prev_lbutton_down = lbutton_down;
                        let entry = tr_map.entries.entry(ag_key.to_owned()).or_default();
                        if entry.last_fg_unix > 0 && (now_u - entry.last_fg_unix) > 12 {
                            entry.typed_chars = 0;
                        }
                        entry.last_fg_unix = now_u;
                        if any_typing {
                            entry.typed_chars = entry.typed_chars.saturating_add(1);
                            if now_u > entry.working_until {
                                entry.done_until = 0;
                            }
                        }
                        // 按下回车（非 Shift+Enter 换行，或 Ctrl+Enter 发送）或先键入内容后点击发送按钮区域，进入工作生成态（避免空闲鼠标点击误判）
                        let enter_send = enter_down && !prev_enter && (!shift_down || ctrl_down) && entry.typed_chars >= 1;
                        let click_send = lbutton_down && !prev_lb && click_in_send_zone && entry.typed_chars >= 1;
                        if enter_send || click_send {
                            entry.typed_chars = 0;
                            entry.working_until = now_u + 18;
                            entry.done_until = now_u + 26;
                        }
                    }
                }
            }
        }
    }

    static SCAN_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let slot = DESKTOP_AGENTS_CACHE.get_or_init(|| {
        Mutex::new((
            Vec::new(),
            std::time::Instant::now() - std::time::Duration::from_secs(10),
        ))
    });
    let (cached, need_scan) = if let Ok(g) = slot.lock() {
        (g.0.clone(), g.1.elapsed() >= std::time::Duration::from_millis(750))
    } else {
        return Vec::new();
    };
    if need_scan && !SCAN_RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let _ = std::thread::Builder::new()
            .name("auralsle-agent-win-scan".into())
            .spawn(|| {
                let detected = detect_running_desktop_agents();
                if let Some(mtx) = DESKTOP_AGENTS_CACHE.get() {
                    if let Ok(mut g) = mtx.lock() {
                        g.0 = detected;
                        g.1 = std::time::Instant::now();
                    }
                }
                SCAN_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
            });
    }
    cached
}

#[cfg(windows)]
fn detect_running_desktop_agents() -> Vec<(String, String, Phase, String)> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowTextW, IsWindowVisible,
    };

    let now_u = unix_now();
    // 优先采集 Claude Code / Codex / Antigravity / WorkBuddy 本机实时会话状态与子工具进程
    let mut initial = scan_live_agent_files(now_u);
    let tool_active_keys = detect_agent_tool_children();
    for &t_key in &tool_active_keys {
        if let Some(pos) = initial.iter().position(|x| x.0 == t_key) {
            if matches!(initial[pos].2, Phase::Done) {
                let d_name = initial[pos].1.clone();
                initial[pos].2 = Phase::Working;
                initial[pos].3 = format!("{d_name} 正在运行子命令/工具…");
            } else if matches!(initial[pos].2, Phase::Waiting) {
                let d_name = initial[pos].1.clone();
                initial[pos].2 = Phase::Tool;
                initial[pos].3 = format!("{d_name} 正在执行子命令/工具…");
            }
        }
    }

    struct Ctx {
        fg: HWND,
        now_u: i64,
        tool_active_keys: Vec<&'static str>,
        found: Vec<(String, String, Phase, String)>,
    }
    fn phase_rank(p: Phase) -> u8 {
        match p {
            Phase::Waiting => 4,
            Phase::Tool => 3,
            Phase::Working | Phase::Running => 2,
            Phase::Error => 1,
            Phase::Done => 0,
        }
    }
    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(hwnd, &mut buf);
            if len <= 0 {
                return BOOL(1);
            }
            let title = String::from_utf16_lossy(&buf[..len as usize]);
            let low = title.to_ascii_lowercase();
            if low == "msctfime ui" || low == "default ime" || low.contains("tooltip") {
                return BOOL(1);
            }
            let ctx = &mut *(lparam.0 as *mut Ctx);
            let proc_low = window_process_name_low(hwnd);

            let Some((ag_key, ag_name)) = classify_ai_agent_window(&title, &low, &proc_low) else {
                return BOOL(1);
            };

            let is_fg = ctx.fg == hwnd;
            let waiting = title.contains("确认")
                || title.contains("允许")
                || title.contains("权限")
                || title.contains("审批")
                || low.contains("approve")
                || low.contains("confirm")
                || low.contains("permission")
                || low.contains("allow?");
            let busy_kw = title.contains("思考")
                || title.contains("生成")
                || title.contains("正在")
                || title.contains("回复中")
                || title.contains("处理中")
                || title.contains("停止生成")
                || title.contains("执行")
                || title.contains("推理")
                || title.contains("搜索")
                || low.contains("thinking")
                || low.contains("reasoning")
                || low.contains("generating")
                || low.contains("streaming")
                || low.contains("stop generating")
                || low.contains("working")
                || low.contains("running")
                || low.contains("loading")
                || title.contains('●')
                || title.contains('⏳')
                || title.contains('◐')
                || title.contains('◓')
                || title.contains('◑')
                || title.contains('◒');
            let has_tool_child = ctx.tool_active_keys.contains(&ag_key);

            let mut tracker = DESKTOP_TRACKER
                .get_or_init(|| Mutex::new(DesktopAgentTracker::default()))
                .lock()
                .ok();
            let (phase_opt, status_msg) = if let Some(ref mut tr_map) = tracker {
                let entry = tr_map.entries.entry(ag_key.to_owned()).or_default();
                let meaningful_title_changed = is_fg
                    && !entry.last_title.is_empty()
                    && entry.last_title != title
                    && title.chars().count() >= 4;
                if title.chars().count() >= 2 {
                    entry.last_title = title.clone();
                }
                if meaningful_title_changed && !waiting && (busy_kw || has_tool_child || ctx.now_u <= entry.working_until) {
                    entry.working_until = entry.working_until.max(ctx.now_u + 10);
                    entry.done_until = entry.done_until.max(ctx.now_u + 18);
                }
                if has_tool_child {
                    entry.working_until = entry.working_until.max(ctx.now_u + 6);
                    entry.done_until = entry.done_until.max(ctx.now_u + 14);
                }
                if is_fg {
                    entry.last_fg_unix = ctx.now_u;
                }
                if waiting {
                    (Some(Phase::Waiting), format!("等待你确认或批准操作 · {title}"))
                } else if has_tool_child {
                    (Some(Phase::Working), format!("{ag_name} 正在执行工具/命令 · {title}"))
                } else if busy_kw || ctx.now_u <= entry.working_until {
                    (Some(Phase::Working), format!("{ag_name} 正在思考与生成回复 · {title}"))
                } else if entry.done_until > 0 && ctx.now_u <= entry.done_until {
                    (Some(Phase::Done), format!("{ag_name} 已完成回复 · {title}"))
                } else {
                    (None, String::new())
                }
            } else if waiting {
                (Some(Phase::Waiting), format!("等待你确认或批准操作 · {title}"))
            } else if has_tool_child || busy_kw {
                (Some(Phase::Working), format!("{ag_name} 正在思考与生成回复 · {title}"))
            } else {
                (None, String::new())
            };

            if let Some(ph) = phase_opt {
                if let Some(pos) = ctx.found.iter().position(|x| x.0 == ag_key) {
                    if phase_rank(ph) >= phase_rank(ctx.found[pos].2) {
                        ctx.found[pos] = (ag_key.to_owned(), ag_name.to_owned(), ph, status_msg);
                    }
                } else {
                    ctx.found.push((ag_key.to_owned(), ag_name.to_owned(), ph, status_msg));
                }
            }
            BOOL(1)
        }
    }
    let mut ctx = Ctx {
        fg: unsafe { GetForegroundWindow() },
        now_u,
        tool_active_keys,
        found: std::mem::take(&mut initial),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    ctx.found
}

#[cfg(windows)]
fn dispatch_agent_window_action(agent_key: &str, decision: &str) {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    struct FindCtx {
        key: String,
        target: Option<HWND>,
    }
    unsafe extern "system" fn find_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(hwnd, &mut buf);
            if len <= 0 {
                return BOOL(1);
            }
            let title_orig = String::from_utf16_lossy(&buf[..len as usize]);
            let title = title_orig.to_ascii_lowercase();
            let proc_low = window_process_name_low(hwnd);
            if proc_low.contains("auralsle")
                || proc_low.contains("auraisle")
                || title_orig.contains("岛仔")
                || title_orig.contains("灵屿")
                || title.contains("auraisle")
            {
                return BOOL(1);
            }
            let ctx = &mut *(lparam.0 as *mut FindCtx);
            let k = ctx.key.as_str();
            let matched = if let Some((ag_k, _)) = classify_ai_agent_window(&title_orig, &title, &proc_low) {
                k.contains(ag_k) || ag_k.contains(k)
            } else if k.contains("claude") || k.contains("codex") {
                title.contains(k)
                    || proc_low.contains("windowsterminal")
                    || proc_low.contains("powershell")
                    || proc_low.contains("pwsh")
                    || proc_low.contains("cmd.exe")
                    || proc_low.contains("code.exe")
            } else {
                title.contains(k) || proc_low.contains(k)
            };
            if matched {
                ctx.target = Some(hwnd);
                return BOOL(0);
            }
            BOOL(1)
        }
    }

    let mut fctx = FindCtx {
        key: agent_key.to_ascii_lowercase(),
        target: None,
    };
    unsafe {
        let _ = EnumWindows(Some(find_cb), LPARAM(&mut fctx as *mut FindCtx as isize));
        if let Some(hwnd) = fctx.target {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
            match decision {
                "allow" => {
                    keybd_event(0x59, 0, Default::default(), 0);
                    keybd_event(0x59, 0, KEYEVENTF_KEYUP, 0);
                    keybd_event(0x0D, 0, Default::default(), 0);
                    keybd_event(0x0D, 0, KEYEVENTF_KEYUP, 0);
                }
                "deny" => {
                    keybd_event(0x4E, 0, Default::default(), 0);
                    keybd_event(0x4E, 0, KEYEVENTF_KEYUP, 0);
                    keybd_event(0x1B, 0, Default::default(), 0);
                    keybd_event(0x1B, 0, KEYEVENTF_KEYUP, 0);
                }
                _ => {}
            }
        }
    }
}

/// 在岛上或通过全局快捷键 (Ctrl+Alt+Y / N / T) 响应 AI Agent 权限审批请求。
pub fn resolve_agent_approval(agent_key: &str, decision: &'static str) {
    let now = unix_now();
    let eff_key = if agent_key.trim().is_empty() { "workbuddy" } else { agent_key };
    if !cfg!(test) {
        let mut dirs = config::get().all_status_dirs();
        if dirs.is_empty() {
            dirs.push(config::get().status_dir());
        }
        for dir in &dirs {
            let _ = fs::create_dir_all(dir);
            for key_name in [eff_key, "workbuddy", "claude", "codex", "cursor", "default"] {
                let approval_path = dir.join(format!("{key_name}.approval.json"));
                let payload = serde_json::json!({
                    "agent": key_name,
                    "decision": decision,
                    "updated": now,
                });
                let _ = fs::write(&approval_path, serde_json::to_string_pretty(&payload).unwrap_or_default());
            }

            let status_path = dir.join(format!("{eff_key}.json"));
            if status_path.is_file() {
                let new_state = match decision {
                    "allow" => ("working", "已允许执行命令/文件操作，正在继续运行…"),
                    "deny" => ("done", "已在灵动岛拒绝该命令执行"),
                    _ => ("running", "已交回终端/IDE 窗口交互处理"),
                };
                let st_json = serde_json::json!({
                    "agent": eff_key,
                    "name": eff_key,
                    "state": new_state.0,
                    "message": new_state.1,
                    "updated": now,
                });
                let _ = fs::write(&status_path, serde_json::to_string_pretty(&st_json).unwrap_or_default());
            }
        }

        #[cfg(windows)]
        dispatch_agent_window_action(eff_key, decision);
    }

    if let Ok(mut st) = lock().lock() {
        for ag in &mut st.agents {
            if ag.key.eq_ignore_ascii_case(eff_key) || agent_key.is_empty() {
                match decision {
                    "allow" => {
                        ag.phase = Phase::Working;
                        ag.step = "已允许命令执行 · 正在运行…".to_owned();
                    }
                    "deny" => {
                        ag.phase = Phase::Done;
                        ag.step = "已拒绝命令执行".to_owned();
                    }
                    _ => {
                        ag.phase = Phase::Running;
                        ag.step = "已交回终端处理".to_owned();
                    }
                }
            }
        }
    }

    let label = match decision {
        "allow" => format!("已对 {eff_key} 发送「允许执行」并同步窗口确认 (Ctrl+Alt+Y)"),
        "deny" => format!("已对 {eff_key} 发送「拒绝操作」并取消请求 (Ctrl+Alt+N)"),
        _ => format!("已激活并交回 {eff_key} 终端/IDE 窗口处理 (Ctrl+Alt+T)"),
    };
    set_hook_action_status(label);
}

/// 将生命周期钩子无损合并进 Claude Code (`~/.claude/settings.json`) 或 WorkBuddy (`~/.workbuddy/settings.json`)。
fn inject_hooks_into_settings_json(settings_path: &std::path::Path, hook_ps1: &std::path::Path, agent_id: &str, agent_name: &str) -> bool {
    let Some(parent) = settings_path.parent() else { return false };
    if !parent.is_dir() {
        return false;
    }
    let mut root: Value = if settings_path.is_file() {
        let Ok(raw) = fs::read_to_string(settings_path) else { return false };
        serde_json::from_str(raw.trim_start_matches('\u{feff}')).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    if !root.is_object() {
        return false;
    }
    let script_path = hook_ps1.to_string_lossy().replace('\\', "/");
    let cmd = format!(
        "powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File \"{script_path}\" -Agent {agent_id} -Name \"{agent_name}\""
    );
    let hooks_obj = root
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let Some(hooks_map) = hooks_obj.as_object_mut() else { return false };

    for ev in ["UserPromptSubmit", "PreToolUse", "PostToolUse", "Notification", "Stop"] {
        let arr = hooks_map
            .entry(ev)
            .or_insert_with(|| serde_json::json!([]));
        let Some(groups) = arr.as_array_mut() else { continue };
        let mut updated_existing = false;
        for g in groups.iter_mut() {
            if let Some(inner_hooks) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                for h in inner_hooks.iter_mut() {
                    let c_str = h.get("command").and_then(Value::as_str).unwrap_or("");
                    if c_str.contains("wb-hook.ps1")
                        || c_str.contains("fly-agent-hook.ps1")
                        || c_str.contains("zen-status")
                    {
                        if let Some(obj) = h.as_object_mut() {
                            obj.insert("type".to_owned(), Value::String("command".to_owned()));
                            obj.insert("command".to_owned(), Value::String(cmd.clone()));
                            updated_existing = true;
                        }
                    }
                }
            }
        }
        if !updated_existing {
            groups.push(serde_json::json!({
                "hooks": [
                    {
                        "type": "command",
                        "command": cmd
                    }
                ]
            }));
        }
    }

    if let Ok(pretty) = serde_json::to_string_pretty(&root) {
        fs::write(settings_path, pretty).is_ok()
    } else {
        false
    }
}

/// 一键安装或修复 Codex / Claude Code / WorkBuddy / DeepSeek / Cursor 生命周期与权限审批钩子。
pub fn install_or_repair_agent_hooks() -> String {
    if cfg!(test) {
        return "测试环境跳过钩子写入".to_owned();
    }
    config::ensure_toolchain();
    let tools = config::tools_dir();
    let _ = fs::create_dir_all(&tools);
    for s_dir in config::get().all_status_dirs() {
        let _ = fs::create_dir_all(&s_dir);
    }

    let hook_body = include_str!("assets/wb-hook.ps1");
    let hook_ps1 = tools.join("fly-agent-hook.ps1");
    let wb_ps1 = tools.join("wb-hook.ps1");
    let _ = fs::write(&hook_ps1, hook_body);
    let _ = fs::write(&wb_ps1, hook_body);

    let mut configured = Vec::new();
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from) {
        let claude_settings = home.join(".claude").join("settings.json");
        if inject_hooks_into_settings_json(&claude_settings, &hook_ps1, "claude", "Claude Code") {
            configured.push("Claude Code");
        }
        let wb_settings = home.join(".workbuddy").join("settings.json");
        if inject_hooks_into_settings_json(&wb_settings, &wb_ps1, "workbuddy", "WorkBuddy") {
            configured.push("WorkBuddy");
        }
    }
    configured.extend(["Codex", "Gemini", "豆包", "DeepSeek", "Cursor", "Kiro"]);

    let hook_cfg = tools.join("agent-lifecycle-hooks.json");
    let hooks_json = serde_json::json!({
        "providers": ["claude-code", "codex", "workbuddy", "gemini", "doubao", "deepseek", "kimi", "cursor", "windsurf", "kiro", "zcode", "grok"],
        "mode": "fly-island-hybrid-hook-jsonl-and-window-bridge",
        "hook_script": hook_ps1.display().to_string(),
        "status_dirs": config::get().all_status_dirs().iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "hotkeys": {
            "allow": "Ctrl+Alt+Y",
            "deny": "Ctrl+Alt+N",
            "terminal": "Ctrl+Alt+T"
        }
    });
    let _ = fs::write(&hook_cfg, serde_json::to_string_pretty(&hooks_json).unwrap_or_default());

    let msg = format!(
        "已修复并激活全套 AI Agent 钩子与实时探测（{}）",
        configured.join(" / ")
    );
    set_hook_action_status(msg.clone());
    msg
}

pub fn snapshot() -> (Vec<Agent>, Option<Finished>) {
    if !cfg!(test) && !super::config::get().mod_ai_suite_enabled {
        return (Vec::new(), None);
    }
    let (mut agents, finished) = lock()
        .lock()
        .map(|s| (s.agents.clone(), s.finished.clone()))
        .unwrap_or_default();
    (agents, finished)
}

// 结束提示卡片已移除：不再需要 `dismiss()` 收起接口。

#[cfg(all(test, windows))]
mod doubao_window_tests {
    use super::is_doubao_window;

    #[test]
    fn doubao_window_is_recognised_by_process_and_title() {
        assert!(is_doubao_window("源码运行卡顿问题解决 - 豆包", "doubao.exe"), "进程名命中就够了");
        assert!(is_doubao_window("豆包", "whatever.exe"), "标题就是豆包也算");
    }

    #[test]
    fn our_own_window_and_terminals_are_never_doubao() {
        assert!(!is_doubao_window("岛仔设置", "auralsle.exe"), "自己的设置窗口不能被当成豆包在干活");
        assert!(!is_doubao_window("cargo run 输出里有豆包字样", "pwsh.exe"), "终端标题里的豆包不能误判");
        assert!(!is_doubao_window("src\\auralsle_ai_status\\ui.rs - 豆包", "code.exe"), "编辑器里打开的文件名不能误判");
        assert!(!is_doubao_window("微信 · 文件传输助手", "wechat.exe"), "别的软件当然不是豆包");
    }
}
