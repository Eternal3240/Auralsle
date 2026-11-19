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
                    let kl = key.to_ascii_lowercase();
                    if !pk.is_empty() {
                        pk.to_ascii_lowercase()
                    } else if kl.contains("claude") {
                        "claude".to_owned()
                    } else if kl.contains("codex") || kl.contains("openai") {
                        "codex".to_owned()
                    } else if kl.contains("deepseek") || kl.contains("dsh") {
                        "deepseek".to_owned()
                    } else if kl.contains("antigravity") {
                        "antigravity".to_owned()
                    } else if kl.contains("workbuddy") || kl.contains("codebuddy") {
                        "workbuddy".to_owned()
                    } else if kl.contains("grok") {
                        "grokbot".to_owned()
                    } else if kl.contains("cursor") {
                        "cursor".to_owned()
                    } else if kl.contains("trae") {
                        "trae".to_owned()
                    } else if kl.contains("muse") {
                        "muse".to_owned()
                    } else if kl == "dot" || kl.starts_with("dot-") || kl.contains("dots") {
                        "dot".to_owned()
                    } else if kl.contains("doubao") || key.contains("豆包") {
                        "doubao".to_owned()
                    } else {
                        kl
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
    if proc_low.contains("trae") || low.ends_with("- trae") || low.ends_with("— trae") || low.contains("trae cn") {
        return Some(("trae", "Trae"));
    }
    if proc_low.contains("muse")
        || (!low_is_terminal_or_editor(proc_low)
            && (low == "muse" || low.starts_with("muse ") || low.ends_with("- muse") || low.contains("musework")))
    {
        return Some(("muse", "Muse"));
    }
    if proc_low == "dot.exe"
        || proc_low == "dots.exe"
        || (!low_is_terminal_or_editor(proc_low)
            && (low == "dot" || low == "dots" || low.ends_with("- dot") || low.ends_with("— dot")))
    {
        return Some(("dot", "Dot"));
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
            && (low.starts_with("grok") || low.ends_with("- grok") || low.ends_with("— grok") || low.contains("grok bot")))
    {
        return Some(("grokbot", "Grok Bot"));
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

    let procs = running_process_names_low();

    // 4. 豆包 / 豆包工作桌面端多会话实时探测 (`%LOCALAPPDATA%\Doubao\User Data\sdk_storage\log`)
    if cfg.fly_doubao_sessions {
        out.extend(scan_doubao_work_sessions(now_u, &cfg, &procs));
    }

    // 5. Gemini Antigravity 实时轨迹探测 (`~/.gemini/antigravity/brain/**/.system_generated/logs/transcript.jsonl`)
    if cfg.fly_antigravity_sessions {
        out.extend(scan_antigravity_sessions(&home, now_u, &cfg, &procs));
    }

    // 6. WorkBuddy / CodeBuddy 实时会话探测 (`~/.workbuddy/projects/**/*.jsonl`)
    if cfg.fly_workbuddy_sessions {
        out.extend(scan_workbuddy_sessions(&home, now_u, &cfg, &procs));
    }

    // 7. Cursor Agent 实时轨迹探测 (`~/.cursor/projects/**/agent-transcripts/**/*.jsonl`)
    if cfg.fly_cursor_sessions {
        out.extend(scan_cursor_sessions(&home, now_u, &cfg, &procs));
    }

    // 8. Grok Bot 实时会话与日志流探测 (`%APPDATA%\Grok Bot` / `~/.grokbot`)
    if cfg.fly_grokbot_sessions {
        out.extend(scan_grokbot_sessions(&home, now_u, &cfg, &procs));
    }

    // 9. Trae / Trae CN (SOLO / Builder) 实时会话与日志流探测 (`%APPDATA%\Trae` / `~/.trae`)
    if cfg.fly_trae_sessions {
        out.extend(scan_trae_sessions(&home, now_u, &cfg, &procs));
    }

    // 10. Muse / MuseWork 实时会话与日志流探测 (`~/.muse` / `%APPDATA%\Muse`)
    if cfg.fly_muse_sessions {
        out.extend(scan_muse_sessions(&home, now_u, &cfg, &procs));
    }

    // 11. Dot / Dots 实时会话与日志流探测 (`~/.dot` / `%APPDATA%\Dot`)
    if cfg.fly_dot_sessions {
        out.extend(scan_dot_sessions(&home, now_u, &cfg, &procs));
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

#[cfg(windows)]
fn running_process_names_low() -> std::collections::HashSet<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut set = std::collections::HashSet::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return set;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase();
                if !name.is_empty() {
                    set.insert(name);
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    set
}

#[cfg(not(windows))]
fn running_process_names_low() -> std::collections::HashSet<String> {
    std::collections::HashSet::new()
}

fn has_any_proc(procs: &std::collections::HashSet<String>, patterns: &[&str]) -> bool {
    #[cfg(not(windows))]
    {
        let _ = (procs, patterns);
        return true;
    }
    #[cfg(windows)]
    procs.iter().any(|p| patterns.iter().any(|pat| p == pat || p.contains(pat)))
}

#[cfg(windows)]
fn is_pid_running(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    if pid == 0 {
        return false;
    }
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code).is_ok() && code == 259; // STILL_ACTIVE
        let _ = CloseHandle(h);
        ok
    }
}

#[cfg(not(windows))]
fn is_pid_running(_pid: u32) -> bool {
    false
}

#[derive(Clone, Debug)]
struct DoubaoConvState {
    phase: Phase,
    started_at: i64,
    updated_at: i64,
    is_work_task: bool,
    model: String,
    effort: String,
    step_msg: String,
    in_progress: bool,
}

#[derive(Default)]
struct DoubaoRootTracker {
    frontend_path: Option<PathBuf>,
    frontend_offset: u64,
    infra_path: Option<PathBuf>,
    infra_offset: u64,
    sessions: std::collections::HashMap<String, DoubaoConvState>,
    session_to_conv: std::collections::HashMap<String, String>,
    last_active_conv: Option<String>,
}

static DOUBAO_TRACKER: OnceLock<Mutex<std::collections::HashMap<String, DoubaoRootTracker>>> = OnceLock::new();

fn parse_doubao_frontend_ts(line: &str, now_u: i64) -> Option<i64> {
    use chrono::{Datelike, Local, NaiveDate, TimeZone};
    let rest = line.strip_prefix('[')?;
    let end = rest.find(']')?;
    let header = &rest[..end];
    // 格式: PID:TID:MMDD/HHMMSS.mmm:INFO:...
    let mut parts = header.split(':');
    let _pid = parts.next()?;
    let _tid = parts.next()?;
    let dt = parts.next()?;
    let (mmdd, hhmmss) = dt.split_once('/')?;
    if mmdd.len() < 4 || hhmmss.len() < 6 {
        return None;
    }
    let month: u32 = mmdd.get(0..2)?.parse().ok()?;
    let day: u32 = mmdd.get(2..4)?.parse().ok()?;
    let hour: u32 = hhmmss.get(0..2)?.parse().ok()?;
    let min: u32 = hhmmss.get(2..4)?.parse().ok()?;
    let sec: u32 = hhmmss.get(4..6)?.parse().ok()?;

    let cur_year = Local::now().year();
    let resolve = |year: i32| -> Option<i64> {
        let naive = NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, min, sec)?;
        Local
            .from_local_datetime(&naive)
            .earliest()
            .or_else(|| Local.from_local_datetime(&naive).latest())
            .map(|d| d.timestamp())
    };
    let mut ts = resolve(cur_year)?;
    if ts - now_u > 86_400 * 30 {
        if let Some(prev) = resolve(cur_year - 1) {
            ts = prev;
        }
    } else if now_u - ts > 86_400 * 30 {
        if let Some(next) = resolve(cur_year + 1) {
            ts = next;
        }
    }
    Some(ts)
}

fn parse_doubao_infra_ts(line: &str) -> Option<i64> {
    let rest = line.strip_prefix('[')?;
    let end = rest.find(']')?;
    chrono::DateTime::parse_from_rfc3339(&rest[..end])
        .ok()
        .map(|dt| dt.timestamp())
}

fn extract_doubao_json_slice(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end_scope = s.rfind("\", source:").unwrap_or(s.len());
    let slice = &s[start..end_scope];
    let end = slice.rfind('}')?;
    Some(&slice[..=end])
}

fn doubao_effort_label(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    match t.to_ascii_lowercase().as_str() {
        "high" | "max" | "7" | "8" | "9" | "10" => "High".to_owned(),
        "medium" | "med" | "4" | "5" | "6" => "Medium".to_owned(),
        "low" | "min" | "1" | "2" | "3" => "Low".to_owned(),
        _ => String::new(),
    }
}

fn read_log_incremental(
    path: &std::path::Path,
    tracked_path: &mut Option<PathBuf>,
    offset: &mut u64,
    initial_tail_bytes: u64,
) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    #[cfg(windows)]
    use std::os::windows::fs::OpenOptionsExt;

    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(windows)]
    {
        opts.share_mode(7);
    }
    let mut f = opts.open(path).ok()?;
    let len = f.metadata().ok()?.len();

    let path_changed = tracked_path.as_deref() != Some(path);
    let mut skip_first_partial = false;
    if path_changed || len < *offset {
        *tracked_path = Some(path.to_path_buf());
        *offset = len.saturating_sub(initial_tail_bytes);
        skip_first_partial = *offset > 0;
    }
    if len <= *offset {
        return None;
    }
    let to_read = (len - *offset).min(2 * 1024 * 1024) as usize;
    f.seek(SeekFrom::Start(*offset)).ok()?;
    let mut buf = vec![0u8; to_read];
    let n = f.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    buf.truncate(n);
    // 只消费到最后一个换行符，避免半行日志在下次读取时丢失
    let valid_len = buf.iter().rposition(|&b| b == b'\n').map(|i| i + 1).unwrap_or(0);
    if valid_len == 0 {
        return None;
    }
    *offset += valid_len as u64;
    let slice = if skip_first_partial {
        buf[..valid_len]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| &buf[i + 1..valid_len])
            .unwrap_or(&[])
    } else {
        &buf[..valid_len]
    };
    Some(String::from_utf8_lossy(slice).into_owned())
}

fn newest_matching_log(dir: &std::path::Path, prefix: &str) -> Option<PathBuf> {
    let rd = fs::read_dir(dir).ok()?;
    let mut best: Option<(i64, String, PathBuf)> = None;
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(prefix) || !name.ends_with(".log") {
            continue;
        }
        let mtime = file_unix(&p);
        let replace = match &best {
            None => true,
            Some((bt, bn, _)) => mtime > *bt || (mtime == *bt && name > bn.as_str()),
        };
        if replace {
            best = Some((mtime, name.to_owned(), p));
        }
    }
    best.map(|(_, _, p)| p)
}

impl DoubaoRootTracker {
    fn ingest_frontend_chunk(&mut self, chunk: &str, now_u: i64, default_work_app: bool) {
        for line in chunk.lines() {
            // 高频心跳与非生命周期噪声快速剪枝
            if line.contains("listTasks")
                || line.contains("getWebContentsToken")
                || line.contains("rfc-request")
                || line.contains("dbStorage.set")
                || line.contains("runtimeStorage.")
                || line.contains("conversation_list_data_")
                || line.contains("presentation_sync_")
                || line.contains("recycle_finished")
                || line.contains("multi_env_transport_state")
            {
                continue;
            }
            let has_signal = line.contains("chat_input_end_button_show")
                || line.contains("input_submit_entry")
                || line.contains("input_submit_executor_entry")
                || line.contains("deep_thinking_progress_show")
                || line.contains("on_message_receiving_update")
                || line.contains("on_batch_message_receiving_end")
                || line.contains("on_message_session_end")
                || line.contains("agent_sandbox_toolcall_exec")
                || line.contains("sandbox_sse_tool_call_received")
                || line.contains("im_chat_cua_bash_escalation_trace")
                || line.contains("neotix.sandboxBash.run")
                || line.contains("neotix.aiUserAgentWorkspace.")
                || line.contains("neotix.sandboxFs.readToTempUrl");
            if !has_signal {
                continue;
            }
            let Some(ev_ts) = parse_doubao_frontend_ts(line, now_u) else {
                continue;
            };

            if let Some(pos) = line.find("[logifier-report]: ") {
                let Some(json_str) = extract_doubao_json_slice(&line[pos + 19..]) else {
                    continue;
                };
                let Ok(v) = serde_json::from_str::<Value>(json_str) else {
                    continue;
                };
                let ev_name = v.get("eventName").and_then(Value::as_str).unwrap_or("");
                let meta = v.get("meta").unwrap_or(&Value::Null);
                self.apply_logifier_event(ev_name, meta, ev_ts, default_work_app);
            } else if let Some(pos) = line.find("\"Event: ") {
                let after = &line[pos + 8..];
                let ev_name = after.split_whitespace().next().unwrap_or("");
                let Some(json_str) = extract_doubao_json_slice(after) else {
                    continue;
                };
                let Ok(v) = serde_json::from_str::<Value>(json_str) else {
                    continue;
                };
                self.apply_neotix_event(ev_name, &v, ev_ts, default_work_app);
            }
        }
    }

    fn ensure_conv(&mut self, conv_id: &str, ev_ts: i64, default_work_app: bool) -> &mut DoubaoConvState {
        self.sessions.entry(conv_id.to_owned()).or_insert_with(|| DoubaoConvState {
            phase: Phase::Working,
            started_at: ev_ts,
            updated_at: ev_ts,
            is_work_task: default_work_app,
            model: String::new(),
            effort: String::new(),
            step_msg: "正在处理任务…".to_owned(),
            in_progress: false,
        })
    }

    fn apply_logifier_event(&mut self, ev_name: &str, meta: &Value, ev_ts: i64, default_work_app: bool) {
        match ev_name {
            "input_submit_entry" | "input_submit_executor_entry" => {
                let conv_id = meta.get("conversation_id").and_then(Value::as_str).unwrap_or("");
                if conv_id.is_empty() {
                    return;
                }
                let st = self.ensure_conv(conv_id, ev_ts, default_work_app);
                if !st.in_progress {
                    st.started_at = ev_ts;
                }
                st.phase = Phase::Working;
                st.in_progress = true;
                st.updated_at = st.updated_at.max(ev_ts);
                st.step_msg = "已发送任务，等待豆包响应…".to_owned();
                self.last_active_conv = Some(conv_id.to_owned());
            }
            "chat_input_end_button_show" => {
                let conv_id = meta.get("chat_id").and_then(Value::as_str).unwrap_or("");
                if conv_id.is_empty() {
                    return;
                }
                let status = meta.get("send_message_status").and_then(Value::as_str).unwrap_or("");
                let btn = meta.get("button_type").and_then(Value::as_str).unwrap_or("");
                if matches!(status, "Submitting" | "Sending" | "Receiving") || btn == "break" {
                    let st = self.ensure_conv(conv_id, ev_ts, default_work_app);
                    if !st.in_progress {
                        st.started_at = ev_ts;
                    }
                    let keep_tool_or_wait = matches!(st.phase, Phase::Tool | Phase::Waiting)
                        && (ev_ts - st.updated_at).abs() <= 12;
                    if !keep_tool_or_wait {
                        st.phase = Phase::Working;
                        st.step_msg = if status == "Receiving" {
                            "正在思考与生成回复…".to_owned()
                        } else {
                            "正在提交任务…".to_owned()
                        };
                    }
                    st.in_progress = true;
                    st.updated_at = st.updated_at.max(ev_ts);
                    self.last_active_conv = Some(conv_id.to_owned());
                } else if status == "Default" {
                    if let Some(st) = self.sessions.get_mut(conv_id) {
                        if st.in_progress && ev_ts >= st.updated_at {
                            st.phase = Phase::Done;
                            st.in_progress = false;
                            st.updated_at = ev_ts;
                            st.step_msg = if st.is_work_task {
                                "豆包工作任务已完成".to_owned()
                            } else {
                                "已完成本轮回复".to_owned()
                            };
                        }
                    }
                }
            }
            "deep_thinking_progress_show" => {
                let conv_id = meta.get("conversation_id").and_then(Value::as_str).unwrap_or("");
                if conv_id.is_empty() {
                    return;
                }
                // 仅更新正在进行的会话，防止回看历史对话时误报工作中
                if let Some(st) = self.sessions.get_mut(conv_id) {
                    if st.in_progress && ev_ts >= st.updated_at {
                        st.updated_at = ev_ts;
                        if !matches!(st.phase, Phase::Tool | Phase::Waiting) {
                            st.phase = Phase::Working;
                            st.step_msg = "正在深度思考与规划…".to_owned();
                        }
                    }
                }
            }
            "on_message_receiving_update" => {
                let sid = meta.get("session_id").and_then(Value::as_str).unwrap_or("");
                let trigger = meta.get("report_trigger").and_then(Value::as_str).unwrap_or("");
                let summary = meta.get("event_summary").and_then(Value::as_str).unwrap_or("");
                let conv_id = if !sid.is_empty() {
                    self.session_to_conv
                        .get(sid)
                        .cloned()
                        .or_else(|| self.last_active_conv.clone())
                        .unwrap_or_else(|| format!("sess-{}", &sid[..sid.len().min(12)]))
                } else if let Some(last) = self.last_active_conv.clone() {
                    last
                } else {
                    return;
                };
                if !sid.is_empty() {
                    self.session_to_conv.insert(sid.to_owned(), conv_id.clone());
                }
                let is_end = trigger == "reply_end" || summary.contains("SSE_REPLY_END");
                if is_end {
                    if let Some(st) = self.sessions.get_mut(&conv_id) {
                        if ev_ts >= st.updated_at {
                            st.phase = Phase::Done;
                            st.in_progress = false;
                            st.updated_at = ev_ts;
                            st.step_msg = if st.is_work_task {
                                "豆包工作任务已完成".to_owned()
                            } else {
                                "已完成本轮回复".to_owned()
                            };
                        }
                    }
                } else {
                    let st = self.ensure_conv(&conv_id, ev_ts, default_work_app);
                    if !st.in_progress {
                        st.started_at = ev_ts;
                    }
                    st.in_progress = true;
                    st.updated_at = st.updated_at.max(ev_ts);
                    if !matches!(st.phase, Phase::Tool | Phase::Waiting) {
                        st.phase = Phase::Working;
                        st.step_msg = "正在流式生成与推理…".to_owned();
                    }
                    self.last_active_conv = Some(conv_id);
                }
            }
            "on_batch_message_receiving_end" | "on_message_session_end" => {
                let conv_id = meta.get("conversation_id").and_then(Value::as_str).unwrap_or("");
                let sid = meta.get("chat_session_id").and_then(Value::as_str).unwrap_or("");
                if conv_id.is_empty() {
                    return;
                }
                if !sid.is_empty() {
                    self.session_to_conv.insert(sid.to_owned(), conv_id.to_owned());
                }
                let mode_type = meta.get("mode_type").and_then(Value::as_str).unwrap_or("");
                let agent_mode = meta.get("agent_mode").and_then(Value::as_i64).unwrap_or(0);
                let model_name = meta.get("model_name").and_then(Value::as_str).unwrap_or("");
                let effort = meta.get("reasoning_effort").and_then(Value::as_str).unwrap_or("");
                let sess_ok = meta.get("session_success").and_then(Value::as_str).unwrap_or("1");
                let ag_err = meta.get("agent_error").and_then(Value::as_str).unwrap_or("0");
                let recv_err = meta.get("receive_err").and_then(Value::as_str).unwrap_or("");
                let sess_err = meta.get("session_err").and_then(Value::as_str).unwrap_or("");

                let st = self.ensure_conv(conv_id, ev_ts, default_work_app);
                if mode_type == "work_task" || agent_mode == 1 {
                    st.is_work_task = true;
                }
                if !model_name.is_empty() {
                    st.model = model_name.to_owned();
                }
                let ef = doubao_effort_label(effort);
                if !ef.is_empty() {
                    st.effort = ef;
                }
                let failed = sess_ok == "0" || ag_err == "1" || !recv_err.is_empty() || !sess_err.is_empty();
                st.phase = if failed { Phase::Error } else { Phase::Done };
                st.in_progress = false;
                st.updated_at = st.updated_at.max(ev_ts);
                st.step_msg = if failed {
                    "豆包任务执行出错，请回去查看".to_owned()
                } else if st.is_work_task {
                    "豆包工作任务已完成".to_owned()
                } else {
                    "已完成本轮回复".to_owned()
                };
            }
            "shell_api_neotix_log" => {
                let api_name = meta.get("name").and_then(Value::as_str).unwrap_or("");
                self.apply_neotix_api_call(api_name, ev_ts, default_work_app);
            }
            _ => {}
        }
    }

    fn apply_neotix_event(&mut self, ev_name: &str, v: &Value, ev_ts: i64, default_work_app: bool) {
        match ev_name {
            "agent_sandbox_toolcall_exec" => {
                let conv_id = v
                    .get("conversationId")
                    .or_else(|| v.get("conversation_id"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| self.last_active_conv.clone())
                    .unwrap_or_else(|| "work".to_owned());
                let tool = v.get("toolName").and_then(Value::as_str).unwrap_or("工具");
                let status = v.get("status").and_then(Value::as_str).unwrap_or("");
                let st = self.ensure_conv(&conv_id, ev_ts, true);
                if !st.in_progress {
                    st.started_at = ev_ts;
                }
                st.is_work_task = true;
                st.in_progress = true;
                st.updated_at = st.updated_at.max(ev_ts);
                if status == "start" || status == "started" {
                    st.phase = Phase::Tool;
                    st.step_msg = format!("正在调用 {tool}");
                } else if status == "success" {
                    st.phase = Phase::Working;
                    st.step_msg = format!("已完成 {tool}，继续推理…");
                } else if status == "failed" || status == "error" {
                    st.phase = Phase::Working;
                    st.step_msg = format!("{tool} 执行异常，正在调整…");
                }
                self.last_active_conv = Some(conv_id);
            }
            "sandbox_sse_tool_call_received" => {
                let conv_id = self.last_active_conv.clone().unwrap_or_else(|| "work".to_owned());
                let tool = v.get("toolName").and_then(Value::as_str).unwrap_or("工具");
                let st = self.ensure_conv(&conv_id, ev_ts, true);
                if !st.in_progress {
                    st.started_at = ev_ts;
                }
                st.is_work_task = true;
                st.in_progress = true;
                st.phase = Phase::Tool;
                st.updated_at = st.updated_at.max(ev_ts);
                st.step_msg = format!("正在调用 {tool}");
                self.last_active_conv = Some(conv_id);
            }
            "im_chat_cua_bash_escalation_trace" => {
                let conv_id = self.last_active_conv.clone().unwrap_or_else(|| "work".to_owned());
                let needs_auth = v.get("requires_authorization").and_then(Value::as_bool).unwrap_or(false)
                    || v.get("cmd_popup_warn").and_then(Value::as_bool).unwrap_or(false);
                let st = self.ensure_conv(&conv_id, ev_ts, true);
                if !st.in_progress {
                    st.started_at = ev_ts;
                }
                st.is_work_task = true;
                st.in_progress = true;
                st.updated_at = st.updated_at.max(ev_ts);
                if needs_auth {
                    st.phase = Phase::Waiting;
                    st.step_msg = "等待你确认执行终端命令".to_owned();
                } else {
                    st.phase = Phase::Tool;
                    st.step_msg = "正在执行终端命令 (Bash)".to_owned();
                }
                self.last_active_conv = Some(conv_id);
            }
            "shell_api_neotix_log" => {
                let api_name = v.get("name").and_then(Value::as_str).unwrap_or("");
                self.apply_neotix_api_call(api_name, ev_ts, default_work_app);
            }
            _ => {}
        }
    }

    fn apply_neotix_api_call(&mut self, api_name: &str, ev_ts: i64, default_work_app: bool) {
        let msg = if api_name == "neotix.sandboxBash.run" {
            "正在执行终端命令"
        } else if matches!(
            api_name,
            "neotix.aiUserAgentWorkspace.writeFile"
                | "neotix.aiUserAgentWorkspace.createFile"
                | "neotix.aiUserAgentWorkspace.appendFile"
                | "neotix.aiUserAgentWorkspace.deleteFile"
        ) {
            "正在修改工作区文件"
        } else if api_name == "neotix.aiUserAgentWorkspace.createDirectory" {
            "正在创建工作区目录"
        } else if api_name == "neotix.sandboxFs.readToTempUrl" {
            "正在读取沙箱文件"
        } else {
            return;
        };
        let conv_id = self.last_active_conv.clone().unwrap_or_else(|| "work".to_owned());
        if api_name == "neotix.sandboxFs.readToTempUrl" {
            if let Some(st) = self.sessions.get_mut(&conv_id) {
                if st.in_progress && ev_ts >= st.updated_at {
                    st.is_work_task = true;
                    st.phase = Phase::Tool;
                    st.updated_at = ev_ts;
                    st.step_msg = msg.to_owned();
                }
            }
            return;
        }
        let st = self.ensure_conv(&conv_id, ev_ts, default_work_app || true);
        if !st.in_progress {
            st.started_at = ev_ts;
        }
        st.is_work_task = true;
        st.in_progress = true;
        st.phase = Phase::Tool;
        st.updated_at = st.updated_at.max(ev_ts);
        st.step_msg = msg.to_owned();
        self.last_active_conv = Some(conv_id);
    }

    fn ingest_infra_chunk(&mut self, chunk: &str) {
        for line in chunk.lines() {
            if line.contains("local_runtime:shell:list_tasks") || !line.contains("method=local_runtime:") {
                continue;
            }
            let msg = if line.contains("method=local_runtime:shell:run_task") {
                "正在执行终端任务"
            } else if line.contains("method=local_runtime:shell:query_task_result") {
                "等待终端命令输出…"
            } else if line.contains("method=local_runtime:file_tool:write")
                || line.contains("method=local_runtime:file_tool:edit")
            {
                "正在修改本地文件"
            } else if line.contains("method=local_runtime:file_tool:read")
                || line.contains("method=local_runtime:file_tool:get_info")
            {
                "正在读取本地文件"
            } else {
                continue;
            };
            let Some(ev_ts) = parse_doubao_infra_ts(line) else {
                continue;
            };
            let conv_id = self.last_active_conv.clone().unwrap_or_else(|| "work".to_owned());
            if let Some(st) = self.sessions.get_mut(&conv_id) {
                // 不覆盖更晚的 Done/Waiting 状态，防止任务完成后回弹
                if ev_ts > st.updated_at {
                    if !st.in_progress {
                        st.started_at = ev_ts;
                    }
                    st.is_work_task = true;
                    st.in_progress = true;
                    st.phase = Phase::Tool;
                    st.updated_at = ev_ts;
                    st.step_msg = msg.to_owned();
                }
            } else {
                self.sessions.insert(
                    conv_id.clone(),
                    DoubaoConvState {
                        phase: Phase::Tool,
                        started_at: ev_ts,
                        updated_at: ev_ts,
                        is_work_task: true,
                        model: String::new(),
                        effort: String::new(),
                        step_msg: msg.to_owned(),
                        in_progress: true,
                    },
                );
                self.last_active_conv = Some(conv_id);
            }
        }
    }

    fn collect_agents(&self, now_u: i64, cfg: &config::Config, proc_running: bool) -> Vec<Agent> {
        let mut out = Vec::new();
        let done_keep = if cfg.done_timeout_secs > 0 { cfg.done_timeout_secs } else { 45 };
        // 豆包流式生成时每 ~60s 至少写一次 on_message_receiving_update，120s 无任何事件即视为已静默结束
        let active_timeout = 120i64;
        for (conv_id, st) in &self.sessions {
            let age = (now_u - st.updated_at).max(0);
            let mut phase = st.phase;
            if phase.active() && !proc_running {
                continue;
            }
            if phase.active() && phase != Phase::Waiting && age > active_timeout {
                phase = Phase::Done;
            }
            let max_age = match phase {
                Phase::Waiting => STALE_ASKING_SECS,
                Phase::Error => done_keep.max(90),
                Phase::Done if st.phase.active() => active_timeout + done_keep,
                Phase::Done => done_keep,
                _ => active_timeout + done_keep,
            };
            if age > max_age {
                continue;
            }
            let display_name = if st.is_work_task { "豆包工作" } else { "豆包" };
            let model_display = if !st.model.is_empty() {
                st.model.chars().take(20).collect()
            } else if st.is_work_task {
                "豆包工作".to_owned()
            } else {
                "豆包".to_owned()
            };
            let title = if st.is_work_task {
                "豆包工作任务".to_owned()
            } else {
                "豆包对话".to_owned()
            };
            let project = if st.is_work_task {
                "本地工作区".to_owned()
            } else {
                "豆包桌面端".to_owned()
            };
            let elapsed = if phase.active() {
                (now_u - st.started_at).max(1)
            } else {
                (st.updated_at - st.started_at).max(1)
            };
            let step = if phase == Phase::Done && st.phase.active() {
                if st.is_work_task {
                    "豆包工作任务已完成".to_owned()
                } else {
                    "已完成本轮回复".to_owned()
                }
            } else {
                st.step_msg.clone()
            };
            out.push(Agent {
                key: format!("doubao-{conv_id}"),
                name: display_name.to_owned(),
                phase,
                message: step.clone(),
                detail: step.clone(),
                project,
                updated: st.updated_at,
                age_secs: age,
                step,
                step_index: 0,
                step_total: 0,
                progress: if phase == Phase::Done { 1.0 } else { -1.0 },
                elapsed,
                quiet: false,
                provider_key: "doubao".to_owned(),
                title,
                model: model_display,
                effort: st.effort.clone(),
                is_cli: false,
                cwd: String::new(),
            });
        }
        out
    }
}

fn scan_doubao_work_sessions(
    now_u: i64,
    cfg: &config::Config,
    procs: &std::collections::HashSet<String>,
) -> Vec<Agent> {
    let Some(local_app) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return Vec::new();
    };
    let proc_running = has_any_proc(procs, &["doubao.exe", "doubaowork.exe"]);
    let tracker_map = DOUBAO_TRACKER.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let Ok(mut map) = tracker_map.lock() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for app in ["Doubao", "DoubaoWork"] {
        let log_dir = local_app.join(app).join("User Data").join("sdk_storage").join("log");
        if !log_dir.is_dir() {
            continue;
        }
        let default_work_app = app == "DoubaoWork";
        let tr = map.entry(app.to_owned()).or_default();
        if let Some(fe_path) = newest_matching_log(&log_dir, "saman_frontend_") {
            if let Some(chunk) = read_log_incremental(&fe_path, &mut tr.frontend_path, &mut tr.frontend_offset, 768 * 1024) {
                tr.ingest_frontend_chunk(&chunk, now_u, default_work_app);
            }
        }
        let infra_dir = log_dir.join("agent_infra");
        if infra_dir.is_dir() {
            if let Some(inf_path) = newest_matching_log(&infra_dir, "agent_infra_") {
                if let Some(chunk) = read_log_incremental(&inf_path, &mut tr.infra_path, &mut tr.infra_offset, 256 * 1024) {
                    tr.ingest_infra_chunk(&chunk);
                }
            }
        }
        // 清理超过 2 小时未活跃的旧会话，防止内存增长
        tr.sessions.retain(|_, s| (now_u - s.updated_at).abs() <= 7200);
        out.extend(tr.collect_agents(now_u, cfg, proc_running));
    }
    out
}

/// Windows 共享模式读取文件头尾，避免与正在写入日志的 AI 客户端产生文件锁冲突。
fn read_file_head_and_tail_shared(path: &std::path::Path, head_bytes: usize, tail_bytes: u64) -> Option<(String, String)> {
    use std::io::{Read, Seek, SeekFrom};
    #[cfg(windows)]
    use std::os::windows::fs::OpenOptionsExt;

    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(windows)]
    {
        opts.share_mode(7);
    }
    let mut f = opts.open(path).ok()?;
    let len = f.metadata().ok()?.len();
    if len == 0 {
        return None;
    }
    let h_len = (len as usize).min(head_bytes);
    let mut h_buf = vec![0u8; h_len];
    let n = f.read(&mut h_buf).ok()?;
    h_buf.truncate(n);
    let head_str = String::from_utf8_lossy(&h_buf).into_owned();

    let start = len.saturating_sub(tail_bytes);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut t_buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut t_buf).ok()?;
    let tail_str = String::from_utf8_lossy(&t_buf).into_owned();
    Some((head_str, tail_str))
}

fn parse_json_timestamp_secs(v: &Value, fallback: i64) -> i64 {
    for k in ["created_at", "timestamp", "updated_at", "time", "ts"] {
        if let Some(raw) = v.get(k) {
            if let Some(s) = raw.as_str() {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                    return dt.timestamp();
                }
                if let Ok(num) = s.parse::<i64>() {
                    return if num > 10_000_000_000 { num / 1000 } else { num };
                }
            } else if let Some(num) = raw.as_i64() {
                if num > 0 {
                    return if num > 10_000_000_000 { num / 1000 } else { num };
                }
            } else if let Some(num) = raw.as_u64() {
                if num > 0 {
                    let n = num as i64;
                    return if n > 10_000_000_000 { n / 1000 } else { n };
                }
            }
        }
    }
    fallback
}

fn normalize_phase_for_age(
    raw_phase: Phase,
    age: i64,
    active_timeout: i64,
    done_keep: i64,
    proc_running: bool,
) -> Option<Phase> {
    if raw_phase.active() && !proc_running {
        return None;
    }
    let mut phase = raw_phase;
    if phase.active() && phase != Phase::Waiting && age > active_timeout {
        phase = Phase::Done;
    }
    let max_age = match phase {
        Phase::Waiting => STALE_ASKING_SECS,
        Phase::Error => done_keep.max(90),
        Phase::Done if raw_phase.active() => active_timeout + done_keep,
        Phase::Done => done_keep,
        _ => active_timeout + done_keep,
    };
    (age <= max_age).then_some(phase)
}

fn parse_antigravity_transcript(
    head: &str,
    tail: &str,
    conv_id: &str,
    mtime: i64,
    now_u: i64,
    cfg: &config::Config,
    proc_running: bool,
) -> Option<Agent> {
    let mut title = String::new();
    let mut model = String::new();
    let mut last_phase: Option<Phase> = None;
    let mut last_msg = String::new();
    let mut last_ts = mtime;

    for line in head.lines() {
        let l = line.trim();
        if !l.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        if title.is_empty() && string_field(&v, "type") == "USER_INPUT" {
            let content = string_field(&v, "content");
            for seg in content.lines() {
                let s = seg.trim();
                if !s.is_empty() && !s.starts_with('<') && !s.starts_with('[') && !s.starts_with('#') {
                    title = s.chars().take(24).collect();
                    break;
                }
            }
        }
    }

    for line in tail.lines().rev() {
        let l = line.trim();
        if !l.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        if model.is_empty() {
            let m = string_field(&v, "model");
            if !m.is_empty() {
                model = m;
            }
        }
        if title.is_empty() && string_field(&v, "type") == "USER_INPUT" {
            let content = string_field(&v, "content");
            for seg in content.lines() {
                let s = seg.trim();
                if !s.is_empty() && !s.starts_with('<') && !s.starts_with('[') {
                    title = s.chars().take(24).collect();
                    break;
                }
            }
        }
        if last_phase.is_some() {
            continue;
        }
        let ev_type = string_field(&v, "type");
        let status = string_field(&v, "status");
        last_ts = parse_json_timestamp_secs(&v, mtime);
        let age = (now_u - last_ts).max(0);
        if status.eq_ignore_ascii_case("ERROR") {
            last_phase = Some(Phase::Error);
            last_msg = "执行出错，请回 Antigravity 查看".to_owned();
        } else if ev_type == "PLANNER_RESPONSE" {
            if let Some(calls) = v.get("tool_calls").and_then(Value::as_array).filter(|a| !a.is_empty()) {
                let tool_name = calls
                    .last()
                    .and_then(|c| c.get("name").and_then(Value::as_str))
                    .unwrap_or("工具");
                let is_ask = tool_name == "ask_question";
                let is_cmd = matches!(tool_name, "run_command" | "edit_file" | "write_file");
                let ph = if is_ask && age <= 600 {
                    Phase::Waiting
                } else if is_cmd && age >= 12 && age <= 300 {
                    Phase::Waiting
                } else if age <= 120 {
                    Phase::Tool
                } else {
                    Phase::Done
                };
                last_phase = Some(ph);
                last_msg = if is_ask {
                    "等待你回答确认问题".to_owned()
                } else {
                    format!("正在调用 {tool_name}")
                };
            } else {
                let content = string_field(&v, "content");
                let short: String = content.lines().next().unwrap_or("已完成回复").trim().chars().take(40).collect();
                last_phase = Some(Phase::Done);
                last_msg = if short.is_empty() { "已完成回复".to_owned() } else { short };
            }
        } else if ev_type == "USER_INPUT" {
            last_phase = Some(if age <= 120 { Phase::Working } else { Phase::Done });
            last_msg = "正在思考与分析任务…".to_owned();
        } else if ev_type == "GENERIC" {
            last_phase = Some(if age <= 120 { Phase::Working } else { Phase::Done });
            last_msg = "工具执行完毕，继续推理…".to_owned();
        }
    }

    let raw_phase = last_phase?;
    let age = (now_u - last_ts).max(0);
    let done_keep = if cfg.done_timeout_secs > 0 { cfg.done_timeout_secs } else { 45 };
    let phase = normalize_phase_for_age(raw_phase, age, 120, done_keep, proc_running)?;
    if title.is_empty() {
        title = "Antigravity 任务".to_owned();
    }
    let short_id: String = conv_id.chars().take(12).collect();
    let model_disp = if model.is_empty() {
        "Gemini".to_owned()
    } else {
        model.chars().take(20).collect()
    };
    Some(Agent {
        key: format!("antigravity-{short_id}"),
        name: "Gemini Antigravity".to_owned(),
        phase,
        message: last_msg.clone(),
        detail: last_msg.clone(),
        project: "Antigravity".to_owned(),
        updated: last_ts,
        age_secs: age,
        step: last_msg,
        step_index: 0,
        step_total: 0,
        progress: if phase == Phase::Done { 1.0 } else { -1.0 },
        elapsed: age.max(1),
        quiet: false,
        provider_key: "antigravity".to_owned(),
        title,
        model: model_disp,
        effort: String::new(),
        is_cli: false,
        cwd: String::new(),
    })
}

fn scan_antigravity_sessions(
    home: &std::path::Path,
    now_u: i64,
    cfg: &config::Config,
    procs: &std::collections::HashSet<String>,
) -> Vec<Agent> {
    let brain_root = home.join(".gemini").join("antigravity").join("brain");
    if !brain_root.is_dir() {
        return Vec::new();
    }
    let proc_running = has_any_proc(procs, &["antigravity.exe"]);
    let mut candidates: Vec<(PathBuf, String, i64)> = Vec::new();
    if let Ok(rd) = fs::read_dir(&brain_root) {
        for entry in rd.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let conv_id = dir.file_name().and_then(|s| s.to_str()).unwrap_or("").to_owned();
            if conv_id.is_empty() || conv_id == "tempmediaStorage" {
                continue;
            }
            let transcript = dir.join(".system_generated").join("logs").join("transcript.jsonl");
            if !transcript.is_file() {
                continue;
            }
            let mtime = file_unix(&transcript);
            if mtime > 0 && now_u - mtime <= 1800 {
                candidates.push((transcript, conv_id, mtime));
            }
        }
    }
    candidates.sort_by(|a, b| b.2.cmp(&a.2));
    candidates.truncate(4);
    let mut out = Vec::new();
    for (path, conv_id, mtime) in candidates {
        let Some((head, tail)) = read_file_head_and_tail_shared(&path, 4096, 20480) else {
            continue;
        };
        if let Some(ag) = parse_antigravity_transcript(&head, &tail, &conv_id, mtime, now_u, cfg, proc_running) {
            out.push(ag);
        }
    }
    out
}

fn parse_workbuddy_jsonl(
    head: &str,
    tail: &str,
    stem: &str,
    parent_dir: &str,
    mtime: i64,
    now_u: i64,
    cfg: &config::Config,
    proc_running: bool,
) -> Option<Agent> {
    let mut title = String::new();
    let mut model = String::new();
    let mut last_phase: Option<Phase> = None;
    let mut last_msg = String::new();
    let mut last_ts = mtime;

    for line in head.lines() {
        let l = line.trim();
        if !l.starts_with('{') { continue; }
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        if title.is_empty() && string_field(&v, "type") == "message" && string_field(&v, "role") == "user" {
            if let Some(txt) = v.pointer("/content/0/text").and_then(Value::as_str).or_else(|| v.get("content").and_then(Value::as_str)) {
                let clean = txt.lines().next().unwrap_or("").trim();
                if !clean.is_empty() {
                    title = clean.chars().take(24).collect();
                }
            }
        }
    }

    for line in tail.lines().rev() {
        let l = line.trim();
        if !l.starts_with('{') { continue; }
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        if model.is_empty() {
            if let Some(m) = v.get("model").and_then(Value::as_str).or_else(|| v.pointer("/providerData/model").and_then(Value::as_str)) {
                model = m.to_owned();
            }
        }
        if last_phase.is_some() {
            continue;
        }
        let row_type = string_field(&v, "type");
        last_ts = parse_json_timestamp_secs(&v, mtime);
        let age = (now_u - last_ts).max(0);
        if row_type == "function_call" || row_type == "tool_call" {
            let t_name = v.get("name").or_else(|| v.get("toolName")).and_then(Value::as_str).unwrap_or("工具");
            let is_ask = matches!(t_name, "AskUserQuestion" | "ask_user");
            let is_exec = matches!(t_name, "Bash" | "run_command" | "execute_command" | "Edit" | "Write");
            let ph = if is_ask && age <= 600 {
                Phase::Waiting
            } else if is_exec && age >= 12 && age <= 300 {
                Phase::Waiting
            } else if age <= 120 {
                Phase::Tool
            } else {
                Phase::Done
            };
            last_phase = Some(ph);
            last_msg = format!("正在调用 {t_name}");
        } else if row_type == "function_call_result" || row_type == "tool_result" {
            let t_name = v.get("name").and_then(Value::as_str).unwrap_or("工具");
            let st = string_field(&v, "status");
            if st.eq_ignore_ascii_case("error") || st.eq_ignore_ascii_case("failed") {
                last_phase = Some(if age <= 90 { Phase::Working } else { Phase::Error });
                last_msg = format!("{t_name} 返回异常，正在处理…");
            } else {
                last_phase = Some(if age <= 90 { Phase::Working } else { Phase::Done });
                last_msg = format!("已完成 {t_name}，继续推理…");
            }
        } else if row_type == "message" {
            let role = string_field(&v, "role");
            let st = string_field(&v, "status");
            if role == "user" {
                last_phase = Some(if age <= 90 { Phase::Working } else { Phase::Done });
                last_msg = "正在思考与规划任务…".to_owned();
            } else if role == "assistant" {
                if matches!(st.as_str(), "in_progress" | "streaming" | "running") && age <= 45 {
                    last_phase = Some(Phase::Working);
                    last_msg = "正在生成回复…".to_owned();
                } else {
                    last_phase = Some(Phase::Done);
                    last_msg = "已完成本轮任务".to_owned();
                }
            }
        }
    }

    let raw_phase = last_phase?;
    let age = (now_u - last_ts).max(0);
    let done_keep = if cfg.done_timeout_secs > 0 { cfg.done_timeout_secs } else { 45 };
    let phase = normalize_phase_for_age(raw_phase, age, 120, done_keep, proc_running)?;
    let project = clean_project_basename(parent_dir);
    if title.is_empty() {
        title = "WorkBuddy 任务".to_owned();
    }
    Some(Agent {
        key: format!("workbuddy-{stem}"),
        name: "WorkBuddy".to_owned(),
        phase,
        message: last_msg.clone(),
        detail: last_msg.clone(),
        project,
        updated: last_ts,
        age_secs: age,
        step: last_msg,
        step_index: 0,
        step_total: 0,
        progress: if phase == Phase::Done { 1.0 } else { -1.0 },
        elapsed: age.max(1),
        quiet: false,
        provider_key: "workbuddy".to_owned(),
        title,
        model: if model.is_empty() { "WorkBuddy".to_owned() } else { model.chars().take(20).collect() },
        effort: String::new(),
        is_cli: false,
        cwd: String::new(),
    })
}

fn scan_workbuddy_sessions(
    home: &std::path::Path,
    now_u: i64,
    cfg: &config::Config,
    procs: &std::collections::HashSet<String>,
) -> Vec<Agent> {
    let proc_running = has_any_proc(procs, &["workbuddy.exe", "codebuddy.exe"]);
    let mut out = Vec::new();
    for root in [home.join(".workbuddy").join("projects"), home.join(".codebuddy").join("projects")] {
        for (jsonl_path, mtime) in recent_jsonl_files_in_dir(&root, now_u, 1800, 3, 4) {
            let Some((head, tail)) = read_file_head_and_tail_shared(&jsonl_path, 4096, 16384) else {
                continue;
            };
            let stem = jsonl_path.file_stem().and_then(|s| s.to_str()).unwrap_or("wb");
            let parent = jsonl_path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()).unwrap_or("WorkBuddy");
            if let Some(ag) = parse_workbuddy_jsonl(&head, &tail, stem, parent, mtime, now_u, cfg, proc_running) {
                if !out.iter().any(|a: &Agent| a.key == ag.key) {
                    out.push(ag);
                }
            }
        }
    }
    out
}

struct AgentSpec {
    provider_key: &'static str,
    display_name: &'static str,
    default_title: &'static str,
    default_project: &'static str,
    default_model: &'static str,
    is_cli: bool,
}

/// 通用 AI Agent JSONL 轨迹解析器：兼容 Cursor / Grok Bot / Trae / Muse / Dot 的
/// Anthropic 风格 (`type: user|assistant|tool_use`) 与 OpenAI/Codex 风格 (`role: user|assistant|tool`, `event/type: turn_started|function_call|task_complete`)。
fn parse_generic_agent_jsonl(
    spec: &AgentSpec,
    head: &str,
    tail: &str,
    stem: &str,
    parent_dir: &str,
    mtime: i64,
    now_u: i64,
    cfg: &config::Config,
    proc_running: bool,
) -> Option<Agent> {
    let mut cwd = String::new();
    let mut title = String::new();
    let mut model = String::new();
    let mut last_phase: Option<Phase> = None;
    let mut last_msg = String::new();
    let mut last_ts = mtime;

    for line in head.lines() {
        let l = line.trim();
        if !l.starts_with('{') { continue; }
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        if cwd.is_empty() {
            cwd = v.get("cwd").or_else(|| v.pointer("/payload/cwd")).and_then(Value::as_str).unwrap_or("").to_owned();
        }
        if title.is_empty() {
            let role = string_field(&v, "role");
            let ty = string_field(&v, "type");
            if role == "user" || ty == "user" {
                if let Some(s) = v.get("content").and_then(Value::as_str)
                    .or_else(|| v.pointer("/message/content").and_then(Value::as_str))
                    .or_else(|| v.pointer("/message/content/0/text").and_then(Value::as_str))
                    .or_else(|| v.pointer("/content/0/text").and_then(Value::as_str))
                {
                    let clean = s.lines().next().unwrap_or("").trim();
                    if !clean.is_empty() && !clean.starts_with('<') {
                        title = clean.chars().take(24).collect();
                    }
                }
            } else {
                let t = string_field(&v, "title");
                if !t.is_empty() {
                    title = t.chars().take(24).collect();
                }
            }
        }
    }

    for line in tail.lines().rev() {
        let l = line.trim();
        if !l.starts_with('{') { continue; }
        // 忽略 Grok Bot / Electron 遥测心跳行
        if l.contains("heartbeatAtMs") || l.contains("bootOutcome") || l.contains("ship_failed") || l.contains("windowCrashSeen") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        if model.is_empty() {
            if let Some(m) = v.get("model").and_then(Value::as_str)
                .or_else(|| v.pointer("/message/model").and_then(Value::as_str))
                .or_else(|| v.pointer("/payload/model").and_then(Value::as_str))
            {
                model = m.to_owned();
            }
        }
        if last_phase.is_some() {
            continue;
        }
        let ev = v.pointer("/payload/type").and_then(Value::as_str)
            .or_else(|| v.get("event").and_then(Value::as_str))
            .or_else(|| v.get("type").and_then(Value::as_str))
            .unwrap_or("")
            .to_ascii_lowercase();
        let role = string_field(&v, "role").to_ascii_lowercase();
        let status = v.get("status").or_else(|| v.pointer("/payload/status")).and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
        last_ts = parse_json_timestamp_secs(&v, mtime);
        let age = (now_u - last_ts).max(0);

        if matches!(ev.as_str(), "task_complete" | "turn_complete" | "agent_turn_complete" | "done" | "completed" | "finished" | "session_end")
            || matches!(status.as_str(), "completed" | "done" | "finished")
        {
            last_phase = Some(Phase::Done);
            last_msg = "已完成本轮任务".to_owned();
        } else if matches!(ev.as_str(), "error" | "failed" | "turn_failed") || matches!(status.as_str(), "error" | "failed") {
            last_phase = Some(Phase::Error);
            last_msg = "执行出错，请回去查看".to_owned();
        } else if matches!(ev.as_str(), "waiting" | "approval" | "approval_required" | "permission_request" | "ask_user")
            || matches!(status.as_str(), "waiting" | "waiting_approval" | "pending_approval")
        {
            last_phase = Some(Phase::Waiting);
            last_msg = "等待你确认继续执行".to_owned();
        } else if matches!(ev.as_str(), "tool_use" | "tool_call" | "function_call" | "custom_tool_call" | "exec_command" | "apply_patch") {
            let t_name = v.get("name").or_else(|| v.get("toolName")).or_else(|| v.pointer("/payload/name")).and_then(Value::as_str).unwrap_or("工具");
            last_phase = Some(if age >= 12 && age <= 300 && matches!(t_name, "Bash" | "run_command" | "exec_command" | "Edit" | "Write") {
                Phase::Waiting
            } else if age <= 120 {
                Phase::Tool
            } else {
                Phase::Done
            });
            last_msg = format!("正在调用 {t_name}");
        } else if matches!(ev.as_str(), "tool_result" | "function_call_result" | "agent_reasoning" | "task_started" | "turn_started" | "thinking" | "streaming" | "working")
            || role == "tool"
        {
            last_phase = Some(if age <= 90 { Phase::Working } else { Phase::Done });
            last_msg = "正在思考与处理任务…".to_owned();
        } else if ev == "assistant" || role == "assistant" {
            let stop_reason = v.pointer("/message/stop_reason").and_then(Value::as_str)
                .or_else(|| v.get("finish_reason").and_then(Value::as_str))
                .or_else(|| v.get("stop_reason").and_then(Value::as_str))
                .unwrap_or("");
            let has_tool = v.get("tool_calls").and_then(Value::as_array).is_some_and(|a| !a.is_empty())
                || v.pointer("/message/content").and_then(Value::as_array).is_some_and(|arr| {
                    arr.iter().any(|it| it.get("type").and_then(Value::as_str) == Some("tool_use"))
                });
            if has_tool {
                last_phase = Some(if age <= 120 { Phase::Tool } else { Phase::Done });
                last_msg = "正在调用工具…".to_owned();
            } else if matches!(stop_reason, "end_turn" | "stop" | "length") || age > 30 {
                last_phase = Some(Phase::Done);
                last_msg = "已完成回复".to_owned();
            } else {
                last_phase = Some(Phase::Working);
                last_msg = "正在生成回复…".to_owned();
            }
        } else if ev == "user" || role == "user" {
            last_phase = Some(if age <= 60 { Phase::Working } else { Phase::Done });
            last_msg = "正在思考与分析任务…".to_owned();
        }
    }

    let raw_phase = last_phase?;
    let age = (now_u - last_ts).max(0);
    let done_keep = if cfg.done_timeout_secs > 0 { cfg.done_timeout_secs } else { 45 };
    let phase = normalize_phase_for_age(raw_phase, age, 120, done_keep, proc_running)?;
    let project = if !cwd.is_empty() {
        clean_project_basename(&cwd)
    } else if !parent_dir.is_empty() && parent_dir != "sessions" && parent_dir != "agent-transcripts" {
        clean_project_basename(parent_dir)
    } else {
        spec.default_project.to_owned()
    };
    if title.is_empty() {
        title = spec.default_title.to_owned();
    }
    let short_stem: String = stem.chars().take(16).collect();
    Some(Agent {
        key: format!("{}-{short_stem}", spec.provider_key),
        name: spec.display_name.to_owned(),
        phase,
        message: last_msg.clone(),
        detail: last_msg.clone(),
        project,
        updated: last_ts,
        age_secs: age,
        step: last_msg,
        step_index: 0,
        step_total: 0,
        progress: if phase == Phase::Done { 1.0 } else { -1.0 },
        elapsed: age.max(1),
        quiet: false,
        provider_key: spec.provider_key.to_owned(),
        title,
        model: if model.is_empty() { spec.default_model.to_owned() } else { model.chars().take(20).collect() },
        effort: String::new(),
        is_cli: spec.is_cli,
        cwd,
    })
}

#[derive(Default)]
struct GenericLogTracker {
    path: Option<PathBuf>,
    offset: u64,
    phase: Option<Phase>,
    started_at: i64,
    updated_at: i64,
    step_msg: String,
    in_progress: bool,
}

static GENERIC_LOG_TRACKERS: OnceLock<Mutex<std::collections::HashMap<String, GenericLogTracker>>> = OnceLock::new();

fn parse_any_log_line_ts(line: &str, now_u: i64) -> Option<i64> {
    if let Some(ts) = parse_doubao_infra_ts(line) {
        return Some(ts);
    }
    if let Some(ts) = parse_doubao_frontend_ts(line, now_u) {
        return Some(ts);
    }
    // 兼容 ISO8601 前缀或 JSON 内 "timestamp":"2026-..."
    if let Some(pos) = line.find("202") {
        let cand = &line[pos..];
        let end = cand.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | ':' | '.' | '+' | 'T' | 'Z'))).unwrap_or(cand.len());
        if end >= 19 {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&cand[..end]) {
                return Some(dt.timestamp());
            }
        }
    }
    None
}

impl GenericLogTracker {
    fn ingest_chunk(&mut self, chunk: &str, now_u: i64) {
        for line in chunk.lines() {
            let low = line.to_ascii_lowercase();
            // 严格过滤心跳 / UI 刷新噪声
            if low.contains("heartbeat")
                || low.contains("listtasks")
                || low.contains("list_tasks")
                || low.contains("getwebcontentstoken")
                || low.contains("bootoutcome")
                || low.contains("ship_failed")
                || low.contains("metrics")
            {
                continue;
            }
            let event = if low.contains("requires_authorization\":true")
                || low.contains("approval_required")
                || low.contains("waiting_approval")
                || low.contains("waiting_confirm")
                || low.contains("permission_request")
                || low.contains("cmd_popup_warn\":true")
            {
                Some((Phase::Waiting, "等待你确认继续执行"))
            } else if low.contains("on_message_session_end")
                || low.contains("task_complete")
                || low.contains("turn_complete")
                || low.contains("solo_task_end")
                || low.contains("stream_end")
                || low.contains("reply_end")
                || low.contains("agent_turn_complete")
            {
                if low.contains("\"agent_error\":\"1\"") || low.contains("\"session_success\":\"0\"") {
                    Some((Phase::Error, "任务执行出错，请回去查看"))
                } else {
                    Some((Phase::Done, "已完成本轮任务"))
                }
            } else if low.contains("agent_sandbox_toolcall_exec")
                || low.contains("tool_call")
                || low.contains("tool_use")
                || low.contains("function_call")
                || low.contains("exec_command")
                || low.contains("apply_patch")
                || low.contains("sandboxbash.run")
            {
                Some((Phase::Tool, "正在调用工具执行任务…"))
            } else if low.contains("solo_task_start")
                || low.contains("turn_started")
                || low.contains("task_started")
                || low.contains("run_started")
                || low.contains("on_message_receiving_update")
                || low.contains("deep_thinking_progress_show")
                || low.contains("stream_start")
                || low.contains("agent_reasoning")
            {
                Some((Phase::Working, "正在思考与生成回复…"))
            } else {
                None
            };
            let Some((ph, msg)) = event else { continue };
            let ev_ts = parse_any_log_line_ts(line, now_u).unwrap_or(now_u);
            if ev_ts < self.updated_at {
                continue;
            }
            if ph.active() && !self.in_progress {
                self.started_at = ev_ts;
            }
            self.in_progress = ph.active();
            self.phase = Some(ph);
            self.updated_at = ev_ts;
            self.step_msg = msg.to_owned();
        }
    }

    fn to_agent(&self, spec: &AgentSpec, now_u: i64, cfg: &config::Config, proc_running: bool) -> Option<Agent> {
        let raw_phase = self.phase?;
        let age = (now_u - self.updated_at).max(0);
        let done_keep = if cfg.done_timeout_secs > 0 { cfg.done_timeout_secs } else { 45 };
        let phase = normalize_phase_for_age(raw_phase, age, 120, done_keep, proc_running)?;
        let elapsed = if phase.active() {
            (now_u - self.started_at).max(1)
        } else {
            (self.updated_at - self.started_at).max(1)
        };
        Some(Agent {
            key: format!("{}-log", spec.provider_key),
            name: spec.display_name.to_owned(),
            phase,
            message: self.step_msg.clone(),
            detail: self.step_msg.clone(),
            project: spec.default_project.to_owned(),
            updated: self.updated_at,
            age_secs: age,
            step: self.step_msg.clone(),
            step_index: 0,
            step_total: 0,
            progress: if phase == Phase::Done { 1.0 } else { -1.0 },
            elapsed,
            quiet: false,
            provider_key: spec.provider_key.to_owned(),
            title: spec.default_title.to_owned(),
            model: spec.default_model.to_owned(),
            effort: String::new(),
            is_cli: spec.is_cli,
            cwd: String::new(),
        })
    }
}

fn newest_log_in_tree(root: &std::path::Path, max_depth: usize, now_u: i64) -> Option<PathBuf> {
    fn walk(dir: &std::path::Path, depth: usize, now_u: i64, best: &mut Option<(i64, PathBuf)>) {
        if depth == 0 { return; }
        let Ok(rd) = fs::read_dir(dir) else { return };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, depth - 1, now_u, best);
            } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("log")) {
                let mtime = file_unix(&p);
                if mtime > 0 && now_u - mtime <= 1800 {
                    if best.as_ref().map_or(true, |(bt, _)| mtime > *bt) {
                        *best = Some((mtime, p));
                    }
                }
            }
        }
    }
    let mut best = None;
    if root.is_dir() {
        walk(root, max_depth, now_u, &mut best);
    }
    best.map(|(_, p)| p)
}

fn scan_generic_jsonl_and_logs(
    spec: &AgentSpec,
    jsonl_roots: &[PathBuf],
    log_roots: &[PathBuf],
    now_u: i64,
    cfg: &config::Config,
    proc_running: bool,
) -> Vec<Agent> {
    let mut out = Vec::new();
    for root in jsonl_roots {
        for (path, mtime) in recent_jsonl_files_in_dir(root, now_u, 1800, 5, 3) {
            let Some((head, tail)) = read_file_head_and_tail_shared(&path, 4096, 16384) else {
                continue;
            };
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(spec.provider_key);
            let parent = path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()).unwrap_or("");
            // 若是 CLI 产生的极新鲜 JSONL（<=60s），即使无桌面主进程也允许显示
            let allow_active = proc_running || (now_u - mtime <= 60 && spec.is_cli);
            if let Some(ag) = parse_generic_agent_jsonl(spec, &head, &tail, stem, parent, mtime, now_u, cfg, allow_active) {
                if !out.iter().any(|a: &Agent| a.key == ag.key) {
                    out.push(ag);
                }
            }
        }
    }
    if out.is_empty() && proc_running {
        let trackers = GENERIC_LOG_TRACKERS.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
        if let Ok(mut map) = trackers.lock() {
            for root in log_roots {
                if let Some(log_path) = newest_log_in_tree(root, 3, now_u) {
                    let key = format!("{}:{}", spec.provider_key, root.display());
                    let tr = map.entry(key).or_default();
                    if let Some(chunk) = read_log_incremental(&log_path, &mut tr.path, &mut tr.offset, 256 * 1024) {
                        tr.ingest_chunk(&chunk, now_u);
                    }
                    if let Some(ag) = tr.to_agent(spec, now_u, cfg, proc_running) {
                        out.push(ag);
                        break;
                    }
                }
            }
        }
    }
    out
}

fn scan_cursor_sessions(
    home: &std::path::Path,
    now_u: i64,
    cfg: &config::Config,
    procs: &std::collections::HashSet<String>,
) -> Vec<Agent> {
    let spec = AgentSpec {
        provider_key: "cursor",
        display_name: "Cursor",
        default_title: "Cursor Agent 任务",
        default_project: "Cursor",
        default_model: "Cursor Agent",
        is_cli: false,
    };
    let mut jsonl_roots = vec![home.join(".cursor").join("projects"), home.join(".cursor").join("sessions")];
    let mut log_roots = Vec::new();
    if let Some(roaming) = std::env::var_os("APPDATA").map(PathBuf::from) {
        log_roots.push(roaming.join("Cursor").join("logs"));
        jsonl_roots.push(roaming.join("Cursor").join("User").join("globalStorage"));
    }
    let proc_running = has_any_proc(procs, &["cursor.exe"]);
    scan_generic_jsonl_and_logs(&spec, &jsonl_roots, &log_roots, now_u, cfg, proc_running)
}

fn scan_grokbot_sessions(
    home: &std::path::Path,
    now_u: i64,
    cfg: &config::Config,
    procs: &std::collections::HashSet<String>,
) -> Vec<Agent> {
    let spec = AgentSpec {
        provider_key: "grokbot",
        display_name: "Grok Bot",
        default_title: "Grok Bot 任务",
        default_project: "Grok Bot",
        default_model: "Grok",
        is_cli: false,
    };
    let mut jsonl_roots = vec![home.join(".grokbot")];
    let mut log_roots = vec![home.join(".grokbot").join("logs")];
    let mut status_pid_alive = false;
    if let Some(roaming) = std::env::var_os("APPDATA").map(PathBuf::from) {
        let gb = roaming.join("Grok Bot");
        if let Ok(text) = fs::read_to_string(gb.join("desktop-status.json")) {
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                if let Some(pid) = v.get("pid").and_then(Value::as_u64) {
                    status_pid_alive = is_pid_running(pid as u32);
                }
            }
        }
        jsonl_roots.push(gb.join("sessions"));
        jsonl_roots.push(gb.join("dune-reliability"));
        log_roots.push(gb.join("logs"));
    }
    let proc_running = status_pid_alive || has_any_proc(procs, &["grokbot.exe", "grok bot.exe", "grok.exe"]);
    scan_generic_jsonl_and_logs(&spec, &jsonl_roots, &log_roots, now_u, cfg, proc_running)
}

fn scan_trae_sessions(
    home: &std::path::Path,
    now_u: i64,
    cfg: &config::Config,
    procs: &std::collections::HashSet<String>,
) -> Vec<Agent> {
    let spec = AgentSpec {
        provider_key: "trae",
        display_name: "Trae",
        default_title: "Trae SOLO / Builder 任务",
        default_project: "Trae",
        default_model: "Trae AI",
        is_cli: false,
    };
    let mut jsonl_roots = vec![home.join(".trae"), home.join(".trae-cn")];
    let mut log_roots = Vec::new();
    if let Some(roaming) = std::env::var_os("APPDATA").map(PathBuf::from) {
        for name in ["Trae", "Trae CN"] {
            let root = roaming.join(name);
            log_roots.push(root.join("logs"));
            jsonl_roots.push(root.join("User").join("globalStorage"));
        }
    }
    let proc_running = has_any_proc(procs, &["trae.exe", "trae cn.exe"]);
    scan_generic_jsonl_and_logs(&spec, &jsonl_roots, &log_roots, now_u, cfg, proc_running)
}

fn scan_muse_sessions(
    home: &std::path::Path,
    now_u: i64,
    cfg: &config::Config,
    procs: &std::collections::HashSet<String>,
) -> Vec<Agent> {
    let spec = AgentSpec {
        provider_key: "muse",
        display_name: "Muse",
        default_title: "Muse Agent 任务",
        default_project: "Muse",
        default_model: "Muse",
        is_cli: true,
    };
    let mut jsonl_roots = vec![home.join(".muse")];
    let mut log_roots = vec![home.join(".muse").join("logs")];
    for env_key in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(env_key).map(PathBuf::from) {
            for name in ["Muse", "MuseWork", "Muse Code"] {
                let root = base.join(name);
                jsonl_roots.push(root.join("sessions"));
                jsonl_roots.push(root.join("projects"));
                log_roots.push(root.join("logs"));
            }
        }
    }
    let proc_running = has_any_proc(procs, &["muse.exe", "musework.exe", "muse-desktop.exe"]);
    scan_generic_jsonl_and_logs(&spec, &jsonl_roots, &log_roots, now_u, cfg, proc_running)
}

fn scan_dot_sessions(
    home: &std::path::Path,
    now_u: i64,
    cfg: &config::Config,
    procs: &std::collections::HashSet<String>,
) -> Vec<Agent> {
    let spec = AgentSpec {
        provider_key: "dot",
        display_name: "Dot",
        default_title: "Dot Agent 任务",
        default_project: "Dot",
        default_model: "Dot",
        is_cli: false,
    };
    let mut jsonl_roots = vec![home.join(".dot"), home.join(".dots")];
    let mut log_roots = vec![home.join(".dot").join("logs"), home.join(".dots").join("logs")];
    for env_key in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(env_key).map(PathBuf::from) {
            for name in ["Dot", "Dots"] {
                let root = base.join(name);
                jsonl_roots.push(root.join("sessions"));
                jsonl_roots.push(root.join("projects"));
                log_roots.push(root.join("logs"));
            }
        }
    }
    let proc_running = has_any_proc(procs, &["dot.exe", "dots.exe", "chatgpt.exe"]);
    scan_generic_jsonl_and_logs(&spec, &jsonl_roots, &log_roots, now_u, cfg, proc_running)
}

#[cfg(all(test, windows))]
mod doubao_window_tests {
    use super::{is_doubao_window, DoubaoRootTracker, Phase};
    use crate::auralsle_ai_status::config::Config;

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

    #[test]
    fn doubao_work_log_tracks_idle_working_tool_waiting_and_done() {
        use chrono::Local;
        let now = Local::now();
        let now_u = now.timestamp();
        let ts = now.format("%m%d/%H%M%S").to_string();
        let cfg = Config::default();
        let mut tr = DoubaoRootTracker::default();

        // 1. 仅空闲心跳不得误判为工作中
        let noise = format!(
            "[11556:7108:{ts}.100:INFO:FRONTEND(0)] [neotix] [office-api-logger] \"[webview][unknown][flow-web][logifier-report]: {{\"version\":1,\"type\":\"event\",\"level\":\"info\",\"eventName\":\"shell_api_neotix_log\",\"meta\":{{\"duration\":2,\"name\":\"neotix.sandboxBash.listTasks\"}}}}\", source:  (0)\n"
        );
        tr.ingest_frontend_chunk(&noise, now_u, false);
        assert!(tr.collect_agents(now_u, &cfg, true).is_empty(), "空闲 listTasks 心跳不能触发工作中");

        // 2. 提交任务 + 执行 Bash 工具
        let working = format!(
            "[11556:7108:{ts}.200:INFO:FRONTEND(0)] [neotix] [office-api-logger] \"[webview][unknown][flow-web][logifier-report]: {{\"version\":1,\"type\":\"event\",\"level\":\"info\",\"eventName\":\"chat_input_end_button_show\",\"meta\":{{\"button_type\":\"break\",\"chat_id\":\"38444064368880130\",\"send_message_status\":\"Receiving\"}}}}\", source:  (0)\n\
             [11556:7108:{ts}.300:INFO:FRONTEND(0)] [neotix] \"Event: agent_sandbox_toolcall_exec {{\"conversationId\":\"38444064368880130\",\"toolName\":\"Bash\",\"status\":\"start\"}}\", source:  (0)\n"
        );
        tr.ingest_frontend_chunk(&working, now_u, false);
        let agents = tr.collect_agents(now_u, &cfg, true);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "豆包工作");
        assert_eq!(agents[0].phase, Phase::Tool);
        assert!(agents[0].step.contains("Bash"));

        // 3. 切到别的空闲会话不应把正在运行的会话错标为完成
        let other_idle = format!(
            "[11556:7108:{ts}.400:INFO:FRONTEND(0)] [neotix] [office-api-logger] \"[webview][unknown][flow-web][logifier-report]: {{\"version\":1,\"type\":\"event\",\"level\":\"info\",\"eventName\":\"chat_input_end_button_show\",\"meta\":{{\"button_type\":\"voice\",\"chat_id\":\"999999\",\"send_message_status\":\"Default\"}}}}\", source:  (0)\n"
        );
        tr.ingest_frontend_chunk(&other_idle, now_u, false);
        assert_eq!(tr.collect_agents(now_u, &cfg, true)[0].phase, Phase::Tool);

        // 4. 命令需要授权 -> Waiting
        let waiting = format!(
            "[11556:7108:{ts}.500:INFO:FRONTEND(0)] [neotix] \"Event: im_chat_cua_bash_escalation_trace {{\"requires_authorization\":true,\"cmd_popup_warn\":true}}\", source:  (0)\n"
        );
        tr.ingest_frontend_chunk(&waiting, now_u, false);
        assert_eq!(tr.collect_agents(now_u, &cfg, true)[0].phase, Phase::Waiting);

        // 5. 会话结束 -> Done，并提取模型名
        let done = format!(
            "[11556:7108:{ts}.600:INFO:FRONTEND(0)] [neotix] [office-api-logger] \"[webview][unknown][flow-web][logifier-report]: {{\"version\":1,\"type\":\"event\",\"level\":\"info\",\"eventName\":\"on_message_session_end\",\"meta\":{{\"conversation_id\":\"38444064368880130\",\"mode_type\":\"work_task\",\"model_name\":\"DeepSeek V4.1 Flash\",\"session_success\":\"1\",\"agent_error\":\"0\"}}}}\", source:  (0)\n"
        );
        tr.ingest_frontend_chunk(&done, now_u, false);
        let done_agents = tr.collect_agents(now_u, &cfg, true);
        assert_eq!(done_agents[0].phase, Phase::Done);
        assert_eq!(done_agents[0].model, "DeepSeek V4.1 Flash");
    }
}
