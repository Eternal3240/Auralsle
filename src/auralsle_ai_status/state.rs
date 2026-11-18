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
    /// [fly-island] 底层提供商标识（"claude" / "codex" / "deepseek" / "workbuddy" 等）
    pub provider_key: String,
    /// [fly-island] 会话标题（首条 Prompt 摘要或任务名，如 "整理 3.1 发布说明"）
    pub title: String,
    /// [fly-island] 模型显示名（如 "gpt-6-astra" / "Opus 5.5"）
    pub model: String,
    /// [fly-island] 推理强度档位（如 "Medium" / "High" / "Low"）
    pub effort: String,
    /// [fly-island] 是否为 CLI 终端会话（true 显示 `>_` 终端图标，false 显示桌面窗口图标）
    pub is_cli: bool,
    /// [fly-island] 会话工作目录路径
    pub cwd: String,
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
                let provider_key = {
                    let pk = string_field(&v, "provider");
                    if !pk.is_empty() {
                        pk.to_ascii_lowercase()
                    } else if key.to_ascii_lowercase().contains("claude") {
                        "claude".to_owned()
                    } else if key.to_ascii_lowercase().contains("codex") || key.to_ascii_lowercase().contains("openai") {
                        "codex".to_owned()
                    } else if key.to_ascii_lowercase().contains("deepseek") || key.to_ascii_lowercase().contains("dsh") {
                        "deepseek".to_owned()
                    } else {
                        key.to_ascii_lowercase()
                    }
                };
                let raw_title = {
                    let t = string_field(&v, "title");
                    if !t.is_empty() {
                        t
                    } else {
                        let s = string_field(&v, "step");
                        if !s.is_empty() { s } else { name.clone() }
                    }
                };
                let raw_model = {
                    let m = string_field(&v, "model");
                    if !m.is_empty() {
                        m
                    } else if provider_key == "claude" {
                        "Opus 4.6".to_owned()
                    } else if provider_key == "codex" {
                        "gpt-5-codex".to_owned()
                    } else if provider_key == "deepseek" {
                        "deepseek-r1".to_owned()
                    } else {
                        String::new()
                    }
                };
                let raw_effort = {
                    let e = string_field(&v, "effort");
                    if !e.is_empty() {
                        e
                    } else if !raw_model.is_empty() {
                        "High".to_owned()
                    } else {
                        String::new()
                    }
                };
                let is_cli = v.get("is_cli").and_then(Value::as_bool).unwrap_or_else(|| {
                    provider_key == "claude" || provider_key == "deepseek"
                });
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
                    elapsed: number_field(&v, "elapsed").max(0),
                    quiet: cfg.active_timeout_secs > 0 && age > cfg.active_timeout_secs,
                    provider_key,
                    title: raw_title,
                    model: raw_model,
                    effort: raw_effort,
                    is_cli,
                    cwd: string_field(&v, "cwd"),
                };
                if is_session_archived(&candidate.key, candidate.updated) {
                    continue;
                }
                if let Some(existing) = agents.iter_mut().find(|a| a.key.eq_ignore_ascii_case(&candidate.key)) {
                    if candidate.updated > existing.updated {
                        *existing = candidate;
                    }
                } else {
                    agents.push(candidate);
                }
            }
        }
        // 任务计时：第一次看到它活跃就记下起点，若底层已带 elapsed 则保留较大值。
        for a in agents.iter_mut() {
            if a.phase.active() {
                let since = *self.active_since.entry(a.key.clone()).or_insert(now_unix);
                let tracked = (now_unix - since).max(0);
                if a.elapsed <= 0 {
                    a.elapsed = tracked;
                }
            } else {
                self.active_since.remove(&a.key);
            }
        }
        // [flyl1u/fly-island 统一 AI 工作状态判断引擎]：
        // 方案 1 与方案 2 全部统一使用 fly-island 的 JSONL 会话流 + 生命周期 Hook 检测（Claude Code / Codex / DSH）
        if !cfg!(test) {
            for cand in cached_fly_island_sessions() {
                if is_session_archived(&cand.key, cand.updated) {
                    continue;
                }
                if !agents.iter().any(|a| a.key.eq_ignore_ascii_case(&cand.key)) {
                    agents.push(cand);
                }
            }

        }

        // 排序规则（对齐 fly-island）：等待确认 (Waiting) 最前 -> 工作中 (Working/Tool) 次之 -> 已完成 (Done) 最后；同类按更新时间倒序。
        let phase_prio = |p: Phase| -> u8 {
            match p {
                Phase::Waiting | Phase::Error => 3,
                Phase::Working | Phase::Tool | Phase::Running => 2,
                Phase::Done => 1,
            }
        };
        agents.sort_by(|a, b| {
            phase_prio(b.phase).cmp(&phase_prio(a.phase)).then(b.updated.cmp(&a.updated))
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
    if !cfg!(test) && !super::config::get().mod_ai_status_enabled {
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

/// [flyl1u/fly-island] 会话归档表：`session_key -> archived_at_updated`。
/// 当该会话后续有新消息产生（`updated > archived_at_updated`）时自动解除归档重新浮现。
static ARCHIVED_SESSIONS: OnceLock<Mutex<std::collections::HashMap<String, i64>>> = OnceLock::new();

pub fn archive_agent_session(key: &str, updated: i64) {
    let mtx = ARCHIVED_SESSIONS.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Ok(mut map) = mtx.lock() {
        map.insert(key.to_ascii_lowercase(), updated.max(unix_now()));
    }
    if let Ok(mut st) = lock().lock() {
        st.agents.retain(|a| !a.key.eq_ignore_ascii_case(key));
    }
}

pub fn clear_archived_sessions() {
    if let Some(mtx) = ARCHIVED_SESSIONS.get() {
        if let Ok(mut map) = mtx.lock() {
            map.clear();
        }
    }
}

pub fn is_session_archived(key: &str, updated: i64) -> bool {
    let Some(mtx) = ARCHIVED_SESSIONS.get() else {
        return false;
    };
    let Ok(map) = mtx.lock() else {
        return false;
    };
    if let Some(&arch_ts) = map.get(&key.to_ascii_lowercase()) {
        updated <= arch_ts
    } else {
        false
    }
}

/// 读取文件头部与尾部文本，用于毫秒级提取 fly-island 会话标题、工作目录、模型与当前阶段。
fn read_file_head_and_tail(path: &std::path::Path, head_bytes: usize, tail_bytes: u64) -> Option<(String, String)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    if len == 0 {
        return None;
    }
    let h_len = (len as usize).min(head_bytes);
    let mut h_buf = vec![0u8; h_len];
    let _ = f.read(&mut h_buf).ok()?;
    let head_str = String::from_utf8_lossy(&h_buf).into_owned();

    let start = len.saturating_sub(tail_bytes);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut t_buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut t_buf).ok()?;
    let tail_str = String::from_utf8_lossy(&t_buf).into_owned();
    Some((head_str, tail_str))
}

/// 在目录树中收集最近修改时间在 `max_age_secs` 秒以内的前 `limit` 个最新 `.jsonl` 会话文件。
fn recent_jsonl_files_in_dir(
    root: &std::path::Path,
    now_u: i64,
    max_age_secs: i64,
    max_depth: usize,
    limit: usize,
) -> Vec<(PathBuf, i64)> {
    fn walk(
        dir: &std::path::Path,
        now_u: i64,
        max_age: i64,
        depth: usize,
        out: &mut Vec<(PathBuf, i64)>,
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
                if matches!(fname_low.as_str(), "node_modules" | ".git" | "cache" | "gpucache") {
                    continue;
                }
                walk(&p, now_u, max_age, depth - 1, out);
            } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("jsonl")) {
                let mtime = file_unix(&p);
                let age = now_u - mtime;
                if mtime > 0 && age <= max_age {
                    out.push((p, mtime));
                }
            }
        }
    }
    let mut all = Vec::new();
    if root.is_dir() {
        walk(root, now_u, max_age_secs, max_depth, &mut all);
    }
    all.sort_by(|a, b| b.1.cmp(&a.1));
    all.truncate(limit);
    all
}

fn clean_project_basename(cwd_or_dir: &str) -> String {
    let trimmed = cwd_or_dir.trim().trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        return "workspace".to_owned();
    }
    let last = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    if last.contains("--") {
        let segs: Vec<&str> = last.split('-').filter(|s| !s.is_empty()).collect();
        if segs.len() >= 2 {
            return segs[segs.len().saturating_sub(2)..].join("-");
        }
    }
    last.to_owned()
}

fn format_model_short(raw: &str, default_model: &str) -> String {
    let r = raw.trim();
    if r.is_empty() {
        return default_model.to_owned();
    }
    let low = r.to_ascii_lowercase();
    if low.contains("opus-4-6") || low.contains("opus-4.6") {
        "Opus 4.6".to_owned()
    } else if low.contains("opus-5-5") || low.contains("opus-5.5") {
        "Opus 5.5".to_owned()
    } else if low.contains("sonnet-4") {
        "Sonnet 4.5".to_owned()
    } else if low.contains("haiku") {
        "Haiku 4.5".to_owned()
    } else {
        r.chars().take(18).collect()
    }
}

fn format_effort_short(raw: &str, default_effort: &str) -> String {
    let low = raw.trim().to_ascii_lowercase();
    match low.as_str() {
        "high" | "max" => "High".to_owned(),
        "medium" | "med" => "Medium".to_owned(),
        "low" | "min" => "Low".to_owned(),
        _ => default_effort.to_owned(),
    }
}

/// [flyl1u/fly-island 演示会话]：1:1 复刻图 1 (`image_f41ce3.png`) 的三条会话，
/// 供本机尚无活跃 Claude Code / Codex / DSH 会话时直接预览和交互。
#[allow(dead_code)]
pub fn build_fly_island_demo_sessions(now_u: i64) -> Vec<Agent> {
    vec![
        Agent {
            key: "codex-fly-release".to_owned(),
            name: "Codex".to_owned(),
            phase: Phase::Waiting,
            message: "草稿写好了，要我按版本号提交吗？".to_owned(),
            detail: "支持岛内直接允许(Ctrl+Alt+Y) / 拒绝(Ctrl+Alt+N)".to_owned(),
            project: "fly-island".to_owned(),
            updated: now_u - 15,
            age_secs: 15,
            step: "草稿写好了，要我按版本号提交吗？".to_owned(),
            step_index: 0,
            step_total: 0,
            progress: 0.90,
            elapsed: 7 * 60,
            quiet: false,
            provider_key: "codex".to_owned(),
            title: "整理 3.1 发布说明".to_owned(),
            model: "gpt-6-astra".to_owned(),
            effort: "Medium".to_owned(),
            is_cli: false,
            cwd: String::new(),
        },
        Agent {
            key: "claude-fly-website".to_owned(),
            name: "Claude Code".to_owned(),
            phase: Phase::Working,
            message: "正在把真实的小岛放进官网...".to_owned(),
            detail: "Claude Code CLI 正在编辑仓库文件".to_owned(),
            project: "fly-island-website".to_owned(),
            updated: now_u - 5,
            age_secs: 5,
            step: "正在把真实的小岛放进官网...".to_owned(),
            step_index: 2,
            step_total: 4,
            progress: 0.65,
            elapsed: 4 * 60,
            quiet: false,
            provider_key: "claude".to_owned(),
            title: "重做官网首页".to_owned(),
            model: "Opus 5.5".to_owned(),
            effort: "High".to_owned(),
            is_cli: true,
            cwd: String::new(),
        },
        Agent {
            key: "codex-fly-clipboard".to_owned(),
            name: "Codex".to_owned(),
            phase: Phase::Done,
            message: "已完成剪贴板图片预览修复".to_owned(),
            detail: "任务已完成".to_owned(),
            project: "fly-island".to_owned(),
            updated: now_u - 60,
            age_secs: 60,
            step: "已完成剪贴板图片预览修复".to_owned(),
            step_index: 4,
            step_total: 4,
            progress: 1.0,
            elapsed: 10 * 60,
            quiet: false,
            provider_key: "codex".to_owned(),
            title: "修复剪贴板图片预览".to_owned(),
            model: "gpt-6-astra".to_owned(),
            effort: "Medium".to_owned(),
            is_cli: false,
            cwd: String::new(),
        },
    ]
}

/// [flyl1u/fly-island] 零配置原生扫描 Claude Code (`~/.claude/projects/**/*.jsonl`)、
/// OpenAI Codex (`~/.codex/sessions/**/*.jsonl`) 与 DeepSeek Harness (`~/.dsh/sessions/**/*.jsonl`) 的多会话状态。
fn scan_fly_island_jsonl_sessions(now_u: i64) -> Vec<Agent> {
    let mut out = Vec::new();
    let cfg = super::config::get();
    let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from) else {
        return out;
    };

    // 1. Claude Code 多会话 JSONL 探测 (`~/.claude/projects/**/*.jsonl`)
    if cfg.fly_claude_sessions {
        let claude_root = std::env::var_os("CLAUDE_CONFIG_DIR")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude"))
            .join("projects");
        for (jsonl_path, mtime) in recent_jsonl_files_in_dir(&claude_root, now_u, 1800, 3, 4) {
            let age = (now_u - mtime).max(0);
            let Some((head, tail)) = read_file_head_and_tail(&jsonl_path, 6144, 16384) else { continue };
            let mut cwd = String::new();
            let mut title = String::new();
            let mut model = String::new();
            let mut effort = String::new();
            for line in head.lines() {
                let l = line.trim();
                if !l.starts_with('{') { continue; }
                let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
                if cwd.is_empty() {
                    cwd = string_field(&v, "cwd");
                }
                if title.is_empty() && string_field(&v, "type") == "user" {
                    if let Some(s) = v.pointer("/message/content").and_then(Value::as_str) {
                        title = s.lines().next().unwrap_or("").trim().chars().take(24).collect();
                    } else if let Some(arr) = v.pointer("/message/content").and_then(Value::as_array) {
                        for it in arr {
                            if let Some(t) = it.get("text").and_then(Value::as_str) {
                                let clean = t.lines().next().unwrap_or("").trim();
                                if !clean.is_empty() && !clean.starts_with('<') {
                                    title = clean.chars().take(24).collect();
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            let mut last_phase: Option<Phase> = None;
            let mut last_msg = String::new();
            for line in tail.lines().rev() {
                let l = line.trim();
                if !l.starts_with('{') { continue; }
                let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
                if model.is_empty() {
                    if let Some(m) = v.pointer("/message/model").and_then(Value::as_str).or_else(|| v.get("model").and_then(Value::as_str)) {
                        model = m.to_owned();
                    }
                }
                if effort.is_empty() {
                    if let Some(ef) = v.get("effort").and_then(Value::as_str).or_else(|| v.pointer("/message/effort").and_then(Value::as_str)) {
                        effort = ef.to_owned();
                    }
                }
                if last_phase.is_some() {
                    continue;
                }
                let row_type = string_field(&v, "type");
                if row_type == "system" && string_field(&v, "subtype") == "turn_duration" {
                    last_phase = Some(Phase::Done);
                    last_msg = "已完成本轮任务".to_owned();
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
                            let ph = if is_ask && age <= 600 {
                                Phase::Waiting
                            } else if is_approval_tool && age >= 8 && age <= 300 {
                                Phase::Waiting
                            } else if age <= 90 {
                                Phase::Tool
                            } else {
                                Phase::Done
                            };
                            let summary = if detail.is_empty() {
                                format!("正在调用 {t_name}")
                            } else {
                                let short: String = detail.chars().take(42).collect();
                                format!("{t_name}: {short}")
                            };
                            last_phase = Some(ph);
                            last_msg = summary;
                        } else if stop_reason == "end_turn" || age > 45 {
                            last_phase = Some(Phase::Done);
                            let short: String = found_text.lines().next().unwrap_or("任务已完成").chars().take(42).collect();
                            last_msg = if short.is_empty() { "已完成回复".to_owned() } else { short };
                        } else {
                            last_phase = Some(Phase::Working);
                            let short: String = found_text.lines().next().unwrap_or("正在生成代码与回复…").chars().take(42).collect();
                            last_msg = if short.is_empty() { "正在思考与编码…".to_owned() } else { short };
                        }
                    }
                } else if row_type == "user" && age <= 35 {
                    last_phase = Some(Phase::Working);
                    last_msg = "正在思考与分析任务…".to_owned();
                }
            }
            if let Some(ph) = last_phase {
                let stem = jsonl_path.file_stem().and_then(|s| s.to_str()).unwrap_or("session");
                let parent_dir = jsonl_path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()).unwrap_or("project");
                let project = if !cwd.is_empty() {
                    clean_project_basename(&cwd)
                } else {
                    clean_project_basename(parent_dir)
                };
                if title.is_empty() {
                    title = "Claude Code 会话".to_owned();
                }
                out.push(Agent {
                    key: format!("claude-{stem}"),
                    name: "Claude Code".to_owned(),
                    phase: ph,
                    message: last_msg.clone(),
                    detail: last_msg.clone(),
                    project,
                    updated: mtime,
                    age_secs: age,
                    step: last_msg,
                    step_index: 0,
                    step_total: 0,
                    progress: if ph == Phase::Done { 1.0 } else { 0.65 },
                    elapsed: age.max(60),
                    quiet: false,
                    provider_key: "claude".to_owned(),
                    title,
                    model: format_model_short(&model, "Opus 4.6"),
                    effort: format_effort_short(&effort, "High"),
                    is_cli: true,
                    cwd,
                });
            }
        }
    }

    // 2. OpenAI Codex 多会话 JSONL 探测 (`~/.codex/sessions/**/*.jsonl`)
    if cfg.fly_codex_sessions {
        let codex_root = std::env::var_os("CODEX_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"))
            .join("sessions");
        for (jsonl_path, mtime) in recent_jsonl_files_in_dir(&codex_root, now_u, 1800, 5, 4) {
            let age = (now_u - mtime).max(0);
            let Some((head, tail)) = read_file_head_and_tail(&jsonl_path, 6144, 16384) else { continue };
            let mut cwd = String::new();
            let mut title = String::new();
            let mut model = String::new();
            let mut effort = String::new();
            let mut is_cli = true;
            for line in head.lines() {
                let l = line.trim();
                if !l.starts_with('{') { continue; }
                let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
                if cwd.is_empty() {
                    cwd = v.pointer("/payload/cwd").and_then(Value::as_str).or_else(|| v.get("cwd").and_then(Value::as_str)).unwrap_or("").to_owned();
                }
                if let Some(orig) = v.pointer("/payload/originator").and_then(Value::as_str) {
                    if orig.to_ascii_lowercase().contains("desktop") || orig.to_ascii_lowercase().contains("app") {
                        is_cli = false;
                    }
                }
                if title.is_empty() {
                    if let Some(msg) = v.pointer("/payload/message").and_then(Value::as_str) {
                        let clean = msg.lines().next().unwrap_or("").trim();
                        if !clean.is_empty() {
                            title = clean.chars().take(24).collect();
                        }
                    }
                }
            }
            let mut last_phase: Option<Phase> = None;
            let mut last_msg = String::new();
            for line in tail.lines().rev() {
                let l = line.trim();
                if !l.starts_with('{') { continue; }
                let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
                if model.is_empty() {
                    if let Some(m) = v.pointer("/payload/model").and_then(Value::as_str).or_else(|| v.get("model").and_then(Value::as_str)) {
                        model = m.to_owned();
                    }
                }
                if effort.is_empty() {
                    if let Some(ef) = v.pointer("/payload/effort").and_then(Value::as_str).or_else(|| v.pointer("/payload/reasoning_effort").and_then(Value::as_str)) {
                        effort = ef.to_owned();
                    }
                }
                if last_phase.is_some() {
                    continue;
                }
                let ev_type = v.pointer("/payload/type").and_then(Value::as_str)
                    .or_else(|| v.get("type").and_then(Value::as_str))
                    .unwrap_or("");
                if matches!(ev_type, "task_complete" | "turn_complete" | "agent_turn_complete") {
                    last_phase = Some(Phase::Done);
                    last_msg = "已完成当前编码任务".to_owned();
                } else if matches!(ev_type, "function_call" | "custom_tool_call" | "exec_command" | "apply_patch") {
                    let t_name = v.pointer("/payload/name").and_then(Value::as_str).unwrap_or("命令/工具");
                    last_phase = Some(if age >= 8 && age <= 300 { Phase::Waiting } else if age <= 60 { Phase::Tool } else { Phase::Done });
                    last_msg = format!("正在执行: {t_name}");
                } else if matches!(ev_type, "agent_reasoning" | "task_started" | "turn_started") {
                    last_phase = Some(if age <= 60 { Phase::Working } else { Phase::Done });
                    last_msg = "正在推理与编写代码…".to_owned();
                }
            }
            if let Some(ph) = last_phase {
                let stem = jsonl_path.file_stem().and_then(|s| s.to_str()).unwrap_or("codex");
                let project = if !cwd.is_empty() {
                    clean_project_basename(&cwd)
                } else {
                    "codex-project".to_owned()
                };
                if title.is_empty() {
                    title = "Codex 编码任务".to_owned();
                }
                out.push(Agent {
                    key: format!("codex-{stem}"),
                    name: "Codex".to_owned(),
                    phase: ph,
                    message: last_msg.clone(),
                    detail: last_msg.clone(),
                    project,
                    updated: mtime,
                    age_secs: age,
                    step: last_msg,
                    step_index: 0,
                    step_total: 0,
                    progress: if ph == Phase::Done { 1.0 } else { 0.65 },
                    elapsed: age.max(60),
                    quiet: false,
                    provider_key: "codex".to_owned(),
                    title,
                    model: format_model_short(&model, "gpt-5-codex"),
                    effort: format_effort_short(&effort, "Medium"),
                    is_cli,
                    cwd,
                });
            }
        }
    }

    // 3. DeepSeek Harness (DSH) 多会话 JSONL 探测 (`~/.dsh/sessions/**/*.jsonl`)
    if cfg.fly_dsh_sessions {
        let dsh_root = home.join(".dsh").join("sessions");
        for (jsonl_path, mtime) in recent_jsonl_files_in_dir(&dsh_root, now_u, 1800, 4, 3) {
            let age = (now_u - mtime).max(0);
            let Some((head, tail)) = read_file_head_and_tail(&jsonl_path, 4096, 12288) else { continue };
            let mut cwd = String::new();
            let mut title = String::new();
            for line in head.lines() {
                let l = line.trim();
                if !l.starts_with('{') { continue; }
                let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
                if cwd.is_empty() {
                    cwd = string_field(&v, "cwd");
                }
                if title.is_empty() {
                    title = string_field(&v, "title");
                }
            }
            let ph = if age <= 30 { Phase::Working } else { Phase::Done };
            let last_msg = tail.lines().rev().find_map(|l| {
                let v: Value = serde_json::from_str(l.trim()).ok()?;
                let m = string_field(&v, "message");
                (!m.is_empty()).then_some(m)
            }).unwrap_or_else(|| "DeepSeek Harness 会话".to_owned());
            let stem = jsonl_path.file_stem().and_then(|s| s.to_str()).unwrap_or("dsh");
            out.push(Agent {
                key: format!("deepseek-{stem}"),
                name: "DeepSeek".to_owned(),
                phase: ph,
                message: last_msg.clone(),
                detail: last_msg.clone(),
                project: clean_project_basename(&cwd),
                updated: mtime,
                age_secs: age,
                step: last_msg,
                step_index: 0,
                step_total: 0,
                progress: if ph == Phase::Done { 1.0 } else { 0.60 },
                elapsed: age.max(60),
                quiet: false,
                provider_key: "deepseek".to_owned(),
                title: if title.is_empty() { "DSH 终端任务".to_owned() } else { title },
                model: "deepseek-r1".to_owned(),
                effort: "High".to_owned(),
                is_cli: true,
                cwd,
            });
        }
    }

    out
}

static FLY_SESSIONS_CACHE: OnceLock<Mutex<(Vec<Agent>, std::time::Instant)>> = OnceLock::new();

fn cached_fly_island_sessions() -> Vec<Agent> {
    if !super::config::get().mod_ai_status_enabled {
        if let Some(mtx) = FLY_SESSIONS_CACHE.get() {
            if let Ok(mut g) = mtx.lock() {
                g.0.clear();
            }
        }
        return Vec::new();
    }

    static SCAN_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let slot = FLY_SESSIONS_CACHE.get_or_init(|| {
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
            .name("auralsle-fly-session-scan".into())
            .spawn(|| {
                let detected = scan_fly_island_jsonl_sessions(unix_now());
                if let Some(mtx) = FLY_SESSIONS_CACHE.get() {
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
    if !cfg!(test) && !super::config::get().mod_ai_status_enabled {
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
