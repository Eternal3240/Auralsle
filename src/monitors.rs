//! 显示器亮度与 DDC/CI 多功能控制（移植自 Twinkle Tray 1.17.2）。
//!
//! 承接 Twinkle Tray 的核心能力：
//! - 逐屏读数、逐屏调亮度、启动时恢复上次亮度；
//! - VCP 码扩展：对比度 `0x12`、输入源 `0x60`、扬声器音量 `0x62`（及静音切换）；
//! - 每屏规则 `%APPDATA%\Auralsle\monitor-rules.json`（自定义命名、隐藏、量程重映射、开机默认值、自定义亮度 VCP 码與 `skipReapply`）；
//! - 定时 / 日出日落（SunCalc 算法）调度；
//! - 锁屏 / 会话切换时禁用写入（`WTSRegisterSessionNotification` + `WM_WTSSESSION_CHANGE` + `screen_locked` 双保险）。
//!
//! DDC/CI 的每次调用都是几十毫秒的系统调用，一律放到后台线程里跑，
//! 界面只读缓存快照、只发指令，绝不阻塞绘制。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// MCCS v2.2a 常用 VCP 码（对应 `twinkle-asar/src/vcp-codes.js`）。
pub const VCP_LUMINANCE: u8 = 0x10;
pub const VCP_CONTRAST: u8 = 0x12;
pub const VCP_INPUT_SOURCE: u8 = 0x60;
pub const VCP_AUDIO_VOLUME: u8 = 0x62;
pub const VCP_AUDIO_MUTE: u8 = 0x8D;
pub const VCP_POWER_MODE: u8 = 0xD6;
/// 笔记本/一体机内置面板虚拟槽位（走 `\\.\LCD` + `powrprof.dll`，不与 DDC/CI 0..N 冲突）。
pub(crate) const INTERNAL_PANEL_SLOT: usize = 99;

/// MCCS 标准输入源枚举（`0x60`）。
pub const INPUT_SOURCES: &[(u16, &str)] = &[
    (1, "VGA-1 (0x01)"),
    (3, "DVI-1 (0x03)"),
    (4, "DVI-2 (0x04)"),
    (15, "DisplayPort-1 (0x0F)"),
    (16, "DisplayPort-2 (0x10)"),
    (17, "HDMI-1 (0x11)"),
    (18, "HDMI-2 (0x12)"),
    (27, "USB-C / DP Alt (0x1B)"),
];

pub fn input_source_name(val: u16) -> &'static str {
    INPUT_SOURCES
        .iter()
        .find_map(|(code, name)| (*code == val).then_some(*name))
        .unwrap_or("其他输入源")
}

/// 一块屏幕的 DDC/CI 状态（含亮度 0x10、对比度 0x12、输入源 0x60、音量 0x62）。
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorInfo {
    pub slot: usize,
    pub label: String,
    pub raw_label: String,
    pub cur: u32,
    pub min: u32,
    pub max: u32,
    pub brightness_vcp: u8,
    /// 对比度 `0x12` `(cur, min, max)`
    pub contrast: Option<(u16, u16, u16)>,
    /// 扬声器音量 `0x62` `(cur, min, max)`
    pub volume: Option<(u16, u16, u16)>,
    /// 输入源 `0x60` 当前值
    pub input: Option<u16>,
}

impl MonitorInfo {
    pub fn percent(&self) -> f32 {
        if self.max > self.min {
            (self.cur.saturating_sub(self.min) as f32 / (self.max - self.min) as f32).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }
    pub fn value_from_percent(&self, p: f32) -> u32 {
        let p = if p.is_finite() { p.clamp(0.0, 1.0) } else { 1.0 };
        self.min + ((self.max - self.min) as f32 * p).round() as u32
    }
    pub fn contrast_percent(&self) -> Option<f32> {
        let (cur, min, max) = self.contrast?;
        if max > min {
            Some(((cur.saturating_sub(min)) as f32 / (max - min) as f32 * 100.0).clamp(0.0, 100.0))
        } else {
            Some(cur as f32)
        }
    }
    pub fn contrast_from_percent(&self, pct: f32) -> u16 {
        let (_, min, max) = self.contrast.unwrap_or((50, 0, 100));
        let p = (pct / 100.0).clamp(0.0, 1.0);
        min + ((max.saturating_sub(min)) as f32 * p).round() as u16
    }
    pub fn volume_percent(&self) -> Option<f32> {
        let (cur, min, max) = self.volume?;
        if max > min {
            Some(((cur.saturating_sub(min)) as f32 / (max - min) as f32 * 100.0).clamp(0.0, 100.0))
        } else {
            Some(cur as f32)
        }
    }
    pub fn volume_from_percent(&self, pct: f32) -> u16 {
        let (_, min, max) = self.volume.unwrap_or((50, 0, 100));
        let p = (pct / 100.0).clamp(0.0, 1.0);
        min + ((max.saturating_sub(min)) as f32 * p).round() as u16
    }
}

// ---------------------------------------------------------------
// P1-2 每屏规则 `monitor-rules.json`
// ---------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MonitorRuleEntry {
    /// 匹配关键字：可写显示器序号（如 `"0"`、`"显示器 1"`）或型号子串（如 `"DEL"`、`"LG"`、`"TMA"`）
    #[serde(default)]
    pub match_label: String,
    /// 自定义显示名称（非空时替换卡片标题）
    #[serde(default)]
    pub custom_name: String,
    /// 是否在列表中隐藏该显示器
    #[serde(default)]
    pub hidden: bool,
    /// 自定义最小原始亮度值
    #[serde(default)]
    pub min: Option<u32>,
    /// 自定义最大原始亮度值
    #[serde(default)]
    pub max: Option<u32>,
    /// 启动时强制应用的默认亮度百分比（0..=100）
    #[serde(default)]
    pub startup_brightness: Option<u32>,
    /// 自定义亮度 VCP 码（默认 `0x10`）
    #[serde(default)]
    pub brightness_vcp: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MonitorRulesFile {
    #[serde(default = "default_rules_comment")]
    pub _comment: String,
    #[serde(default = "default_ddc_brightness_codes")]
    pub ddc_brightness_codes: HashMap<String, u8>,
    #[serde(default = "default_skip_reapply")]
    pub skip_reapply: Vec<String>,
    #[serde(default)]
    pub rules: Vec<MonitorRuleEntry>,
}

fn default_rules_comment() -> String {
    "灵屿 AuraIsle × Twinkle Tray 每屏规则：可按 match_label（型号子串或序号）自定义屏幕名称(custom_name)、隐藏(hidden)、亮度量程(min/max)与开机亮度(startup_brightness)。".to_owned()
}

fn default_ddc_brightness_codes() -> HashMap<String, u8> {
    let mut m = HashMap::new();
    m.insert("FUS087C".to_owned(), 107);
    m.insert("FUS06AB".to_owned(), 19);
    m
}

fn default_skip_reapply() -> Vec<String> {
    vec!["DEL41D9".to_owned()]
}

impl Default for MonitorRulesFile {
    fn default() -> Self {
        Self {
            _comment: default_rules_comment(),
            ddc_brightness_codes: default_ddc_brightness_codes(),
            skip_reapply: default_skip_reapply(),
            rules: vec![MonitorRuleEntry {
                match_label: "EXAMPLE_MONITOR".to_owned(),
                custom_name: "主工作台显示器".to_owned(),
                hidden: false,
                min: Some(0),
                max: Some(100),
                startup_brightness: None,
                brightness_vcp: Some(0x10),
            }],
        }
    }
}

pub fn rules_path() -> std::path::PathBuf {
    crate::model::config_dir().join("monitor-rules.json")
}

/// 读取 `%APPDATA%\Auralsle\monitor-rules.json`；若不存在则自动生成带注释样例，若 JSON 写坏则安全回退默认且不崩。
pub fn load_monitor_rules() -> MonitorRulesFile {
    let path = rules_path();
    if !path.exists() {
        let def = MonitorRulesFile::default();
        let _ = std::fs::create_dir_all(crate::model::config_dir());
        if let Ok(txt) = serde_json::to_string_pretty(&def) {
            let _ = std::fs::write(&path, txt);
        }
        return def;
    }
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str::<MonitorRulesFile>(&raw).unwrap_or_default(),
        Err(_) => MonitorRulesFile::default(),
    }
}

pub fn save_monitor_rules(rules: &MonitorRulesFile) {
    let _ = std::fs::create_dir_all(crate::model::config_dir());
    if let Ok(txt) = serde_json::to_string_pretty(rules) {
        let _ = std::fs::write(rules_path(), txt);
    }
}

fn find_matching_rule<'a>(
    rules: &'a MonitorRulesFile,
    slot: usize,
    raw_label: &str,
) -> Option<&'a MonitorRuleEntry> {
    let slot_str = slot.to_string();
    let display_idx_str = format!("显示器 {}", slot + 1);
    let raw_low = raw_label.to_ascii_lowercase();
    rules.rules.iter().find(|r| {
        let pat = r.match_label.trim();
        if pat.is_empty() || pat == "EXAMPLE_MONITOR" {
            return false;
        }
        pat == slot_str
            || pat == display_idx_str
            || raw_low.contains(&pat.to_ascii_lowercase())
    })
}

// ---------------------------------------------------------------
// P1-4 定时 / 日出日落调度 & P1-5 锁屏禁用设置
// ---------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ScheduleTrigger {
    FixedTime,
    Sunrise,
    Sunset,
}

impl ScheduleTrigger {
    pub fn label(self) -> &'static str {
        match self {
            Self::FixedTime => "固定时刻",
            Self::Sunrise => "日出 (SunCalc)",
            Self::Sunset => "日落 (SunCalc)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScheduleRule {
    pub enabled: bool,
    pub trigger: ScheduleTrigger,
    pub hour: u8,
    pub minute: u8,
    pub target_percent: u8,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TwinkleConfig {
    pub restore_on_startup: bool,
    pub disable_on_lock_screen: bool,
    pub schedule_enabled: bool,
    pub latitude: f64,
    pub longitude: f64,
    pub schedules: Vec<ScheduleRule>,
}

impl Default for TwinkleConfig {
    fn default() -> Self {
        Self {
            restore_on_startup: false,
            disable_on_lock_screen: true,
            schedule_enabled: false,
            latitude: 31.23,
            longitude: 121.47,
            schedules: vec![
                ScheduleRule {
                    enabled: true,
                    trigger: ScheduleTrigger::Sunrise,
                    hour: 7,
                    minute: 30,
                    target_percent: 85,
                },
                ScheduleRule {
                    enabled: true,
                    trigger: ScheduleTrigger::Sunset,
                    hour: 18,
                    minute: 30,
                    target_percent: 45,
                },
                ScheduleRule {
                    enabled: false,
                    trigger: ScheduleTrigger::FixedTime,
                    hour: 22,
                    minute: 30,
                    target_percent: 20,
                },
            ],
        }
    }
}

static TW_CFG: OnceLock<Mutex<TwinkleConfig>> = OnceLock::new();
static SESSION_LOCKED: AtomicBool = AtomicBool::new(false);
static LAST_SCHEDULE_FIRED_KEY: OnceLock<Mutex<Option<(i32, u32, u32, usize, u8, u8)>>> = OnceLock::new();

fn twinkle_cfg_path() -> std::path::PathBuf {
    crate::model::config_dir().join("twinkle-settings.json")
}

fn load_twinkle_cfg() -> TwinkleConfig {
    let Ok(txt) = std::fs::read_to_string(twinkle_cfg_path()) else {
        return TwinkleConfig::default();
    };
    let mut cfg = serde_json::from_str::<TwinkleConfig>(&txt).unwrap_or_default();
    // 启动或重新打开灵屿时始终只读取当前硬件屏幕亮度，绝不反向覆盖现有屏幕亮度
    cfg.restore_on_startup = false;
    cfg
}

pub fn get_twinkle_config() -> TwinkleConfig {
    TW_CFG
        .get_or_init(|| Mutex::new(load_twinkle_cfg()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub fn update_twinkle_config(next: TwinkleConfig) {
    let _ = std::fs::create_dir_all(crate::model::config_dir());
    if let Ok(txt) = serde_json::to_string_pretty(&next) {
        let _ = std::fs::write(twinkle_cfg_path(), txt);
    }
    // 先取一份种子给 lazy init 用，避免 next 在闭包里被借用后又移动。（E0382 修复）
    let seed = next.clone();
    *TW_CFG
        .get_or_init(|| Mutex::new(seed))
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = next;
    // 修改调度配置后清除当日已触发标记，使新设定的 1 分钟后目标值能够准时触发。
    *LAST_SCHEDULE_FIRED_KEY
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn reload_twinkle_config() {
    let loaded = load_twinkle_cfg();
    let seed = loaded.clone();
    *TW_CFG
        .get_or_init(|| Mutex::new(seed))
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = loaded;
    *LAST_SCHEDULE_FIRED_KEY
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
}

/// 基于 `twinkle-asar/node_modules/suncalc/suncalc.js` 的日出日落时刻计算，返回本地时间 `(日出 (时, 分), 日落 (时, 分))`。
pub fn compute_suncalc_times(lat: f64, lon: f64) -> ((u8, u8), (u8, u8)) {
    use chrono::{Datelike, Local, Timelike};
    let now = Local::now();
    let day_of_year = now.ordinal() as f64;
    let lat_rad = lat.clamp(-65.0, 65.0).to_radians();

    // 太阳赤纬与均时差近似（NOAA / Jean Meeus 公式）
    let gamma = 2.0 * std::f64::consts::PI / 365.0 * (day_of_year - 1.0);
    let eq_time = 229.18
        * (0.000075 + 0.001868 * gamma.cos()
            - 0.032077 * gamma.sin()
            - 0.014615 * (2.0 * gamma).cos()
            - 0.040849 * (2.0 * gamma).sin());
    let decl = 0.006918 - 0.399912 * gamma.cos() + 0.070257 * gamma.sin()
        - 0.006758 * (2.0 * gamma).cos()
        + 0.000907 * (2.0 * gamma).sin()
        - 0.002697 * (3.0 * gamma).cos()
        + 0.00148 * (3.0 * gamma).sin();

    let zenith = 90.833_f64.to_radians();
    let cos_ha = ((zenith.cos() / (lat_rad.cos() * decl.cos())) - lat_rad.tan() * decl.tan())
        .clamp(-1.0, 1.0);
    let ha_deg = cos_ha.acos().to_degrees();

    let tz_offset_hours = (now.offset().local_minus_utc() as f64) / 3600.0;
    let solar_noon_min = 720.0 - 4.0 * lon - eq_time + tz_offset_hours * 60.0;
    let sunrise_min = (solar_noon_min - ha_deg * 4.0).rem_euclid(1440.0);
    let sunset_min = (solar_noon_min + ha_deg * 4.0).rem_euclid(1440.0);

    let to_hm = |m: f64| -> (u8, u8) {
        let total = m.round() as u32 % 1440;
        ((total / 60) as u8, (total % 60) as u8)
    };
    let _ = now.hour();
    (to_hm(sunrise_min), to_hm(sunset_min))
}

/// 当前是否因锁屏或会话切换而暂停 DDC/CI 写入（P1-5）。
pub fn is_write_blocked_by_lock() -> bool {
    let cfg = get_twinkle_config();
    if !cfg.disable_on_lock_screen {
        return false;
    }
    if SESSION_LOCKED.load(Ordering::Relaxed) {
        return true;
    }
    #[cfg(windows)]
    {
        if crate::system::screen_locked() {
            return true;
        }
    }
    false
}

/// 启动 `WTSRegisterSessionNotification` 会话锁屏监听线程（P1-5）。
#[cfg(windows)]
fn start_session_lock_watcher() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("auralsle-wts-session".into())
        .spawn(|| unsafe {
            use windows::core::w;
            use windows::Win32::System::RemoteDesktop::{
                WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
            };
            use windows::Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, GetMessageW, TranslateMessage, DispatchMessageW, HWND_MESSAGE,
                MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK,
                WTS_SESSION_LOGOFF, WTS_SESSION_UNLOCK, WTS_CONSOLE_DISCONNECT,
                WTS_CONSOLE_CONNECT,
            };

            let Ok(hwnd) = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("AuralsleWtsSessionWatcher"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                None,
                None,
                None,
            ) else {
                return;
            };
            let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_WTSSESSION_CHANGE {
                    let code = msg.wParam.0 as u32;
                    if code == WTS_SESSION_LOCK
                        || code == WTS_SESSION_LOGOFF
                        || code == WTS_CONSOLE_DISCONNECT
                    {
                        SESSION_LOCKED.store(true, Ordering::Relaxed);
                    } else if code == WTS_SESSION_UNLOCK || code == WTS_CONSOLE_CONNECT {
                        SESSION_LOCKED.store(false, Ordering::Relaxed);
                    }
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
}

enum Cmd {
    Refresh,
    SetVcp { slot: usize, code: u8, value: u16 },
    StepAll { delta_pct: i32 },
    ToggleAllMute,
}

static LIST: OnceLock<Mutex<Vec<MonitorInfo>>> = OnceLock::new();
static NOTE: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static TX: OnceLock<Sender<Cmd>> = OnceLock::new();
static MUTE_SAVED_VOL: OnceLock<Mutex<HashMap<usize, u16>>> = OnceLock::new();
static MOD_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn is_module_enabled() -> bool {
    MOD_ENABLED.load(Ordering::Relaxed)
}

/// 由「功能模块」开关调用：停用时清理显示器缓存并休眠后台调度；启用时立即重新探测。
pub fn set_module_enabled(enabled: bool) {
    let prev = MOD_ENABLED.swap(enabled, Ordering::Relaxed);
    if !enabled {
        list_slot().lock().unwrap_or_else(|e| e.into_inner()).clear();
        MUTE_SAVED_VOL.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap_or_else(|e| e.into_inner()).clear();
        *note_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some("显示器与蓝牙外设模块已停用".to_owned());
    } else if !prev {
        *note_slot().lock().unwrap_or_else(|e| e.into_inner()) = None;
        request_refresh();
    }
}

fn list_slot() -> &'static Mutex<Vec<MonitorInfo>> {
    LIST.get_or_init(|| Mutex::new(Vec::new()))
}
fn note_slot() -> &'static Mutex<Option<String>> {
    NOTE.get_or_init(|| Mutex::new(None))
}

fn saved_path() -> std::path::PathBuf {
    crate::model::config_dir().join("monitor-brightness.txt")
}

/// 上次退出时的亮度（`slot 亮度值` 每行一条），用于启动时恢复。
fn load_saved() -> Vec<(usize, u32)> {
    let text = std::fs::read_to_string(saved_path()).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let slot = it.next()?.parse::<usize>().ok()?;
            let value = it.next()?.parse::<u32>().ok()?;
            Some((slot, value))
        })
        .collect()
}

fn save_saved(list: &[MonitorInfo]) {
    let text: String = list.iter().map(|m| format!("{} {}\n", m.slot, m.cur)).collect();
    let _ = std::fs::create_dir_all(crate::model::config_dir());
    let _ = std::fs::write(saved_path(), text);
}

/// 启动后台线程（幂等）。首轮先把上次保存的亮度写回屏幕，随后每 30 秒检查一次定时/日出日落调度。
pub fn start() {
    if !MOD_ENABLED.load(Ordering::Relaxed) || TX.get().is_some() {
        return;
    }
    #[cfg(windows)]
    start_session_lock_watcher();
    crate::hotkeys::start();

    let (tx, rx) = channel::<Cmd>();
    let spawned = std::thread::Builder::new()
        .name("auraisle-monitors".into())
        .spawn(move || {
            let _ = load_monitor_rules();
            if MOD_ENABLED.load(Ordering::Relaxed) {
                // 重新打开灵屿时仅读取当前屏幕硬件亮度并同步缓存，不改写当前亮度
                refresh_now();
            }
            let mut last_sched_check = Instant::now();
            let mut dirty_save_at: Option<Instant> = None;
            let mut dirty_power_pct: Option<u16> = None;

            let flush_idle = |dirty_save_at: &mut Option<Instant>, dirty_power_pct: &mut Option<u16>| {
                if dirty_power_pct.take().is_some() {
                    #[cfg(windows)]
                    if let Some(pct) = dirty_power_pct_last() {
                        write_power_brightness(pct as u32, is_on_battery_win());
                    }
                }
                if dirty_save_at.take().is_some() {
                    let cur_list = list_slot().lock().unwrap_or_else(|e| e.into_inner()).clone();
                    save_saved(&cur_list);
                }
            };

            loop {
                if !MOD_ENABLED.load(Ordering::Relaxed) {
                    dirty_save_at = None;
                    dirty_power_pct = None;
                    match rx.recv_timeout(Duration::from_secs(30)) {
                        Ok(Cmd::Refresh) if MOD_ENABLED.load(Ordering::Relaxed) => refresh_now(),
                        Err(RecvTimeoutError::Disconnected) => break,
                        _ => {}
                    }
                    last_sched_check = Instant::now();
                    continue;
                }
                let wait_dur = if dirty_save_at.is_some() || dirty_power_pct.is_some() {
                    Duration::from_millis(260)
                } else {
                    Duration::from_secs(15)
                };
                let first = match rx.recv_timeout(wait_dur) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => {
                        flush_idle(&mut dirty_save_at, &mut dirty_power_pct);
                        None
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        flush_idle(&mut dirty_save_at, &mut dirty_power_pct);
                        break;
                    }
                };
                if let Some(cmd0) = first {
                    // 合并队列中所有挂起的 SetVcp（拖动滑条时只写最新值，彻底消除百毫秒级 I2C/IOCTL 积压延迟）
                    let mut batch = vec![cmd0];
                    while let Ok(more) = rx.try_recv() {
                        batch.push(more);
                    }
                    let mut latest_vcp: Vec<((usize, u8), u16)> = Vec::new();
                    let mut need_refresh = false;
                    let mut net_step_delta = 0i32;
                    let mut toggle_mute_cnt = 0u32;
                    for item in batch {
                        match item {
                            Cmd::Refresh => need_refresh = true,
                            Cmd::SetVcp { slot, code, value } => {
                                if let Some(prev) = latest_vcp.iter_mut().find(|((s, c), _)| *s == slot && *c == code) {
                                    prev.1 = value;
                                } else {
                                    latest_vcp.push(((slot, code), value));
                                }
                            }
                            Cmd::StepAll { delta_pct } => net_step_delta += delta_pct,
                            Cmd::ToggleAllMute => toggle_mute_cnt += 1,
                        }
                    }
                    if !is_write_blocked_by_lock() {
                        for ((slot, code), value) in latest_vcp {
                            if slot == INTERNAL_PANEL_SLOT && code == VCP_LUMINANCE {
                                #[cfg(windows)]
                                {
                                    write_internal_brightness_win_fast(value);
                                    record_power_pct_last(value);
                                }
                                dirty_power_pct = Some(value);
                            } else {
                                write_vcp_now(slot, code, value);
                            }
                            if code == VCP_LUMINANCE {
                                dirty_save_at = Some(Instant::now());
                            }
                        }
                        if net_step_delta != 0 {
                            step_all_now(net_step_delta);
                        }
                        if toggle_mute_cnt % 2 == 1 {
                            toggle_mute_now();
                        }
                    }
                    if need_refresh {
                        refresh_now();
                    }
                    // 若超过 600ms 持续拖动仍未空闲，顺便做一次防抖落盘；否则交由停止拖动后的 260ms 超时一次性写入
                    if dirty_save_at.is_some_and(|t| t.elapsed() >= Duration::from_millis(600)) {
                        flush_idle(&mut dirty_save_at, &mut dirty_power_pct);
                    }
                }
                if MOD_ENABLED.load(Ordering::Relaxed) && last_sched_check.elapsed() >= Duration::from_secs(20) {
                    last_sched_check = Instant::now();
                    check_schedule_tick();
                }
            }
        });
    if spawned.is_ok() {
        let _ = TX.set(tx);
    } else {
        *note_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some("亮度后台线程启动失败".to_owned());
    }
}

fn send(cmd: Cmd) {
    if !MOD_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    start();
    if let Some(tx) = TX.get() {
        let _ = tx.send(cmd);
    }
}

pub fn request_refresh() {
    send(Cmd::Refresh);
}

/// 泛化 VCP 写入入口（P1-1）：支持亮度 `0x10`、对比度 `0x12`、输入源 `0x60`、音量 `0x62`。
pub fn set_vcp(slot: usize, code: u8, value: u16) {
    if is_write_blocked_by_lock() {
        return;
    }
    // 乐观更新内存快照，滑动条不回弹、不阻塞 UI。
    {
        let mut list = list_slot().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(m) = list.iter_mut().find(|m| m.slot == slot) {
            if code == VCP_LUMINANCE || code == m.brightness_vcp {
                m.cur = (value as u32).clamp(m.min, m.max);
            } else if code == VCP_CONTRAST {
                if let Some((cur, min, max)) = &mut m.contrast {
                    *cur = value.clamp(*min, *max);
                }
            } else if code == VCP_AUDIO_VOLUME {
                if let Some((cur, min, max)) = &mut m.volume {
                    *cur = value.clamp(*min, *max);
                }
            } else if code == VCP_INPUT_SOURCE {
                m.input = Some(value);
            }
        }
    }
    send(Cmd::SetVcp { slot, code, value });
}

/// 保持向后兼容的亮度写入接口。
pub fn set_value(slot: usize, value: u32) {
    let code = list_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|m| m.slot == slot)
        .map(|m| m.brightness_vcp)
        .unwrap_or(VCP_LUMINANCE);
    set_vcp(slot, code, value.min(u16::MAX as u32) as u16);
}

/// 全局热键步进所有屏幕亮度百分比（如 `+5` 或 `-5`）。
pub fn step_all_brightness(delta_pct: i32) {
    send(Cmd::StepAll { delta_pct });
}

/// 全局热键切换所有支持音量控制的显示器静音状态。
pub fn toggle_all_monitors_mute() {
    send(Cmd::ToggleAllMute);
}

pub fn snapshot() -> Vec<MonitorInfo> {
    if !MOD_ENABLED.load(Ordering::Relaxed) {
        return Vec::new();
    }
    start();
    list_slot().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn note() -> Option<String> {
    note_slot().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn store(list: Vec<MonitorInfo>, note: Option<String>) {
    *list_slot().lock().unwrap_or_else(|e| e.into_inner()) = list;
    *note_slot().lock().unwrap_or_else(|e| e.into_inner()) = note;
}

fn refresh_now() {
    let (list, note) = probe();
    save_saved(&list);
    store(list, note);
}

fn step_all_now(delta_pct: i32) {
    let mut list = list_slot().lock().unwrap_or_else(|e| e.into_inner()).clone();
    if list.is_empty() {
        return;
    }
    for m in &mut list {
        let cur_pct = (m.percent() * 100.0).round() as i32;
        let next_pct = (cur_pct + delta_pct).clamp(0, 100) as f32 / 100.0;
        let next_val = m.value_from_percent(next_pct);
        m.cur = next_val;
        write_vcp_now(m.slot, m.brightness_vcp, next_val as u16);
    }
    save_saved(&list);
    store(list, None);
}

fn toggle_mute_now() {
    let mut list = list_slot().lock().unwrap_or_else(|e| e.into_inner()).clone();
    let saved_map = MUTE_SAVED_VOL.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = saved_map.lock().unwrap_or_else(|e| e.into_inner());
    let mut any_vol = false;
    for m in &mut list {
        if let Some((cur, min, max)) = &mut m.volume {
            any_vol = true;
            if *cur > *min {
                map.insert(m.slot, *cur);
                *cur = *min;
                write_vcp_now(m.slot, VCP_AUDIO_VOLUME, *min);
                write_vcp_now(m.slot, VCP_AUDIO_MUTE, 1);
            } else {
                let restore = map.remove(&m.slot).unwrap_or((*min + *max) / 2).clamp(*min, *max);
                *cur = restore;
                write_vcp_now(m.slot, VCP_AUDIO_MUTE, 2);
                write_vcp_now(m.slot, VCP_AUDIO_VOLUME, restore);
            }
        }
    }
    if any_vol {
        store(list, None);
    } else {
        #[cfg(windows)]
        {
            let _ = crate::system::toggle_mute();
        }
    }
}

fn check_schedule_tick() {
    use chrono::{Datelike, Local, Timelike};
    if is_write_blocked_by_lock() {
        return;
    }
    let cfg = get_twinkle_config();
    if !cfg.schedule_enabled || cfg.schedules.is_empty() {
        return;
    }
    let now = Local::now();
    let cur_h = now.hour() as u8;
    let cur_m = now.minute() as u8;
    let (sunrise_hm, sunset_hm) = compute_suncalc_times(cfg.latitude, cfg.longitude);

    for (idx, rule) in cfg.schedules.iter().enumerate() {
        if !rule.enabled {
            continue;
        }
        let (target_h, target_m) = match rule.trigger {
            ScheduleTrigger::FixedTime => (rule.hour, rule.minute),
            ScheduleTrigger::Sunrise => sunrise_hm,
            ScheduleTrigger::Sunset => sunset_hm,
        };
        if cur_h == target_h && cur_m == target_m {
            let fire_key = (now.year(), now.month(), now.day(), idx, target_h, target_m);
            let mut last_slot = LAST_SCHEDULE_FIRED_KEY
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            // 同一天同一条规则只触发一次，机器休眠唤醒后绝不重复疯狂写。
            if *last_slot == Some(fire_key) {
                continue;
            }
            *last_slot = Some(fire_key);
            drop(last_slot);

            let rules_file = load_monitor_rules();
            let mut list = list_slot().lock().unwrap_or_else(|e| e.into_inner()).clone();
            let pct = (rule.target_percent.min(100) as f32) / 100.0;
            for m in &mut list {
                if rules_file
                    .skip_reapply
                    .iter()
                    .any(|skip| !skip.is_empty() && m.raw_label.contains(skip))
                {
                    continue;
                }
                let val = m.value_from_percent(pct);
                m.cur = val;
                write_vcp_now(m.slot, m.brightness_vcp, val as u16);
            }
            save_saved(&list);
            store(list, None);
            break;
        }
    }
}

#[allow(dead_code)]
fn restore_on_startup() {
    // 仅读取当前硬件屏幕亮度并同步状态，不在软件启动时覆盖现有屏幕亮度
    refresh_now();
}

fn write_vcp_now(slot: usize, code: u8, value: u16) {
    if !MOD_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    #[cfg(windows)]
    write_vcp_win(slot, code, value);
    #[cfg(not(windows))]
    {
        let _ = (slot, code, value);
    }
}

fn probe() -> (Vec<MonitorInfo>, Option<String>) {
    if !MOD_ENABLED.load(Ordering::Relaxed) {
        return (Vec::new(), Some("显示器与蓝牙外设模块已停用".to_owned()));
    }
    #[cfg(windows)]
    {
        probe_win()
    }
    #[cfg(not(windows))]
    {
        (Vec::new(), Some("当前系统不支持 DDC/CI".to_owned()))
    }
}

#[cfg(windows)]
#[link(name = "dxva2")]
extern "system" {
    fn GetVCPFeatureAndVCPFeatureReply(
        hmonitor: isize,
        bcode: u8,
        pvct: *mut i32,
        pdwcurrentvalue: *mut u32,
        pdwmaximumvalue: *mut u32,
    ) -> i32;
    fn SetVCPFeature(hmonitor: isize, bcode: u8, dwnewvalue: u32) -> i32;
    fn GetMonitorContrast(
        hmonitor: isize,
        pdwminimumcontrast: *mut u32,
        pdwcurrentcontrast: *mut u32,
        pdwmaximumcontrast: *mut u32,
    ) -> i32;
    fn SetMonitorContrast(hmonitor: isize, dwnewcontrast: u32) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(
        lp_file_name: *const u16,
        dw_desired_access: u32,
        dw_share_mode: u32,
        lp_security_attributes: *const std::ffi::c_void,
        dw_creation_disposition: u32,
        dw_flags_and_attributes: u32,
        h_template_file: isize,
    ) -> isize;
    fn DeviceIoControl(
        h_device: isize,
        dw_io_control_code: u32,
        lp_in_buffer: *const std::ffi::c_void,
        n_in_buffer_size: u32,
        lp_out_buffer: *mut std::ffi::c_void,
        n_out_buffer_size: u32,
        lp_bytes_returned: *mut u32,
        lp_overlapped: *mut std::ffi::c_void,
    ) -> i32;
    fn CloseHandle(h_object: *mut std::ffi::c_void) -> i32;
    fn LocalFree(h_mem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn GetSystemPowerStatus(lp_system_power_status: *mut SystemPowerStatus) -> i32;
}

#[cfg(windows)]
#[link(name = "powrprof")]
extern "system" {
    fn PowerGetActiveScheme(
        user_root_power_key: *const std::ffi::c_void,
        active_policy_guid: *mut *mut WinGuid,
    ) -> u32;
    fn PowerReadACValueIndex(
        root_power_key: *const std::ffi::c_void,
        scheme_guid: *const WinGuid,
        sub_group_of_power_settings_guid: *const WinGuid,
        power_setting_guid: *const WinGuid,
        ac_value_index: *mut u32,
    ) -> u32;
    fn PowerReadDCValueIndex(
        root_power_key: *const std::ffi::c_void,
        scheme_guid: *const WinGuid,
        sub_group_of_power_settings_guid: *const WinGuid,
        power_setting_guid: *const WinGuid,
        dc_value_index: *mut u32,
    ) -> u32;
    fn PowerWriteACValueIndex(
        root_power_key: *const std::ffi::c_void,
        scheme_guid: *const WinGuid,
        sub_group_of_power_settings_guid: *const WinGuid,
        power_setting_guid: *const WinGuid,
        ac_value_index: u32,
    ) -> u32;
    fn PowerWriteDCValueIndex(
        root_power_key: *const std::ffi::c_void,
        scheme_guid: *const WinGuid,
        sub_group_of_power_settings_guid: *const WinGuid,
        power_setting_guid: *const WinGuid,
        dc_value_index: u32,
    ) -> u32;
    fn PowerSetActiveScheme(
        user_root_power_key: *const std::ffi::c_void,
        scheme_guid: *const WinGuid,
    ) -> u32;
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct WinGuid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct SystemPowerStatus {
    ac_line_status: u8,
    battery_flag: u8,
    battery_life_percent: u8,
    system_status_flag: u8,
    battery_life_time: u32,
    battery_full_life_time: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DisplayBrightness {
    uc_display_policy: u8,
    uc_ac_brightness: u8,
    uc_dc_brightness: u8,
}

#[cfg(windows)]
const IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS: u32 = 0x0023_0494;
#[cfg(windows)]
const IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS: u32 = 0x0023_0498;
#[cfg(windows)]
const IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS: u32 = 0x0023_049C;

#[cfg(windows)]
const GUID_VIDEO_SUBGROUP: WinGuid = WinGuid {
    data1: 0x7516_b95f,
    data2: 0xf776,
    data3: 0x4464,
    data4: [0x8c, 0x53, 0x06, 0x16, 0x7f, 0x40, 0xcc, 0x99],
};

#[cfg(windows)]
const GUID_VIDEO_BRIGHTNESS: WinGuid = WinGuid {
    data1: 0xaded_5e82,
    data2: 0xb909,
    data3: 0x4619,
    data4: [0x99, 0x49, 0xf5, 0xd7, 0x1d, 0xac, 0x0b, 0xcb],
};

#[cfg(windows)]
fn is_on_battery_win() -> bool {
    let mut sps = SystemPowerStatus::default();
    let ok = unsafe { GetSystemPowerStatus(&mut sps) };
    ok != 0 && sps.ac_line_status == 0
}

#[cfg(windows)]
fn open_lcd_handle(for_write: bool) -> Option<isize> {
    const INVALID_HANDLE: isize = -1;
    let path: [u16; 8] = [b'\\' as u16, b'\\' as u16, b'.' as u16, b'\\' as u16, b'L' as u16, b'C' as u16, b'D' as u16, 0];
    let accesses: [u32; 3] = if for_write {
        [0xC000_0000, 0x4000_0000, 0]
    } else {
        [0x8000_0000, 0, 0xC000_0000]
    };
    for access in accesses {
        let h = unsafe {
            CreateFileW(
                path.as_ptr(),
                access,
                3, // FILE_SHARE_READ | FILE_SHARE_WRITE
                std::ptr::null(),
                3, // OPEN_EXISTING
                0,
                0,
            )
        };
        if h != 0 && h != INVALID_HANDLE {
            return Some(h);
        }
    }
    None
}

#[cfg(windows)]
fn read_supported_lcd_levels(h: isize) -> Vec<u8> {
    let mut buf = [0u8; 256];
    let mut returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS,
            std::ptr::null(),
            0,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok != 0 && returned > 0 {
        buf[..(returned as usize).min(buf.len())].to_vec()
    } else {
        Vec::new()
    }
}

#[cfg(windows)]
fn read_power_brightness(is_dc: bool) -> Option<u32> {
    let mut scheme_ptr: *mut WinGuid = std::ptr::null_mut();
    let res = unsafe { PowerGetActiveScheme(std::ptr::null(), &mut scheme_ptr) };
    if res != 0 || scheme_ptr.is_null() {
        return None;
    }
    let mut val = 0u32;
    let read_res = unsafe {
        if is_dc {
            PowerReadDCValueIndex(
                std::ptr::null(),
                scheme_ptr,
                &GUID_VIDEO_SUBGROUP,
                &GUID_VIDEO_BRIGHTNESS,
                &mut val,
            )
        } else {
            PowerReadACValueIndex(
                std::ptr::null(),
                scheme_ptr,
                &GUID_VIDEO_SUBGROUP,
                &GUID_VIDEO_BRIGHTNESS,
                &mut val,
            )
        }
    };
    unsafe {
        let _ = LocalFree(scheme_ptr.cast());
    }
    (read_res == 0).then(|| val.min(100))
}

#[cfg(windows)]
fn write_power_brightness(pct: u32, is_dc: bool) {
    let pct = pct.min(100);
    let mut scheme_ptr: *mut WinGuid = std::ptr::null_mut();
    let res = unsafe { PowerGetActiveScheme(std::ptr::null(), &mut scheme_ptr) };
    if res != 0 || scheme_ptr.is_null() {
        return;
    }
    unsafe {
        if is_dc {
            let _ = PowerWriteDCValueIndex(
                std::ptr::null(),
                scheme_ptr,
                &GUID_VIDEO_SUBGROUP,
                &GUID_VIDEO_BRIGHTNESS,
                pct,
            );
        } else {
            let _ = PowerWriteACValueIndex(
                std::ptr::null(),
                scheme_ptr,
                &GUID_VIDEO_SUBGROUP,
                &GUID_VIDEO_BRIGHTNESS,
                pct,
            );
        }
        let _ = PowerSetActiveScheme(std::ptr::null(), scheme_ptr);
        let _ = LocalFree(scheme_ptr.cast());
    }
}

/// 探测笔记本/一体机内置面板（必须 `\\.\LCD` + `IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS` 返回 >0 档位才认定存在）。
#[cfg(windows)]
fn probe_internal_panel(rules_file: &MonitorRulesFile) -> Option<MonitorInfo> {
    let h = open_lcd_handle(false)?;
    let supported = read_supported_lcd_levels(h);
    if supported.is_empty() {
        unsafe {
            let _ = CloseHandle(h as *mut std::ffi::c_void);
        }
        return None;
    }

    let is_dc = is_on_battery_win();
    let mut db = DisplayBrightness::default();
    let mut returned = 0u32;
    let query_ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS,
            std::ptr::null(),
            0,
            (&mut db as *mut DisplayBrightness).cast(),
            std::mem::size_of::<DisplayBrightness>() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    unsafe {
        let _ = CloseHandle(h as *mut std::ffi::c_void);
    }

    let cur_raw = if query_ok != 0 && returned >= std::mem::size_of::<DisplayBrightness>() as u32 {
        let val = if is_dc { db.uc_dc_brightness } else { db.uc_ac_brightness };
        (val as u32).min(100)
    } else {
        read_power_brightness(is_dc).unwrap_or_else(|| {
            let mid = supported[supported.len() / 2];
            mid as u32
        })
    };

    let raw_label = "内置屏幕 · LCD".to_owned();
    let matched_rule = find_matching_rule(rules_file, INTERNAL_PANEL_SLOT, &raw_label);
    if matched_rule.is_some_and(|r| r.hidden) {
        return None;
    }
    let mut min = 0u32;
    let mut max = 100u32;
    if let Some(rule) = matched_rule {
        if let Some(r_min) = rule.min {
            min = r_min.min(100);
        }
        if let Some(r_max) = rule.max {
            if r_max > min {
                max = r_max.min(100);
            }
        }
    }
    let cur = cur_raw.clamp(min, max);
    let label = matched_rule
        .filter(|r| !r.custom_name.trim().is_empty())
        .map(|r| r.custom_name.trim().to_owned())
        .unwrap_or_else(|| raw_label.clone());

    Some(MonitorInfo {
        slot: INTERNAL_PANEL_SLOT,
        label,
        raw_label,
        cur,
        min,
        max,
        brightness_vcp: VCP_LUMINANCE,
        contrast: None,
        volume: None,
        input: None,
    })
}

#[cfg(windows)]
static LAST_POWER_PCT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(u16::MAX);

#[cfg(windows)]
fn record_power_pct_last(value: u16) {
    LAST_POWER_PCT.store(value.min(100), std::sync::atomic::Ordering::Relaxed);
}

#[cfg(windows)]
fn dirty_power_pct_last() -> Option<u16> {
    let v = LAST_POWER_PCT.swap(u16::MAX, std::sync::atomic::Ordering::Relaxed);
    (v <= 100).then_some(v)
}

#[cfg(windows)]
fn cached_supported_lcd_levels() -> &'static Vec<u8> {
    static LEVELS: OnceLock<Vec<u8>> = OnceLock::new();
    LEVELS.get_or_init(|| {
        if let Some(h) = open_lcd_handle(false) {
            let levels = read_supported_lcd_levels(h);
            unsafe {
                let _ = CloseHandle(h as *mut std::ffi::c_void);
            }
            levels
        } else {
            Vec::new()
        }
    })
}

#[cfg(windows)]
fn write_internal_brightness_win_fast(value: u16) {
    let pct = (value as u32).min(100) as u8;
    if let Some(h) = open_lcd_handle(true) {
        let supported = cached_supported_lcd_levels();
        let hw_level = if !supported.is_empty() {
            supported
                .iter()
                .copied()
                .min_by_key(|&lv| (lv as i16 - pct as i16).abs())
                .unwrap_or(pct)
        } else {
            pct
        };
        let mut db = DisplayBrightness {
            uc_display_policy: 3, // DISPLAYPOLICY_BOTH: immediate apply
            uc_ac_brightness: hw_level,
            uc_dc_brightness: hw_level,
        };
        let mut returned = 0u32;
        let _ = unsafe {
            DeviceIoControl(
                h,
                IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS,
                (&mut db as *mut DisplayBrightness).cast(),
                std::mem::size_of::<DisplayBrightness>() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        unsafe {
            let _ = CloseHandle(h as *mut std::ffi::c_void);
        }
    }
}

#[cfg(windows)]
fn write_internal_brightness_win(value: u16) {
    write_internal_brightness_win_fast(value);
    write_power_brightness((value as u32).min(100), is_on_battery_win());
}

#[cfg(windows)]
fn read_vcp_feature(handle_raw: isize, code: u8) -> Option<(u16, u16, u16)> {
    let mut vct: i32 = 0;
    let mut cur: u32 = 0;
    let mut max: u32 = 0;
    let ok = unsafe { GetVCPFeatureAndVCPFeatureReply(handle_raw, code, &mut vct, &mut cur, &mut max) != 0 };
    if ok {
        let max_u16 = (max.min(65535) as u16).max(1);
        let cur_u16 = (cur.min(65535) as u16).min(max_u16);
        Some((cur_u16, 0, max_u16))
    } else {
        None
    }
}

#[cfg(windows)]
fn probe_win() -> (Vec<MonitorInfo>, Option<String>) {
    use windows::Win32::Devices::Display::{
        DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
        GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR,
    };
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
    };

    struct HMon {
        handle: HMONITOR,
        label: String,
    }
    fn text(value: &[u16]) -> String {
        String::from_utf16_lossy(&value[..value.iter().position(|c| *c == 0).unwrap_or(value.len())])
    }
    unsafe extern "system" fn collect(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(monitor, &mut info.monitorInfo).as_bool() {
            let device = text(&info.szDevice);
            let number = device.trim_start_matches(r"\\.\DISPLAY").to_owned();
            let primary = info.monitorInfo.dwFlags & 1 != 0;
            let out = &mut *(data.0 as *mut Vec<HMon>);
            out.push(HMon {
                handle: monitor,
                label: format!("显示器 {}{}", number, if primary { " · 主屏" } else { "" }),
            });
        }
        BOOL(1)
    }

    let rules_file = load_monitor_rules();
    let mut hmons: Vec<HMon> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut hmons as *mut Vec<HMon> as isize));
    }

    let mut list = Vec::new();
    let mut note: Option<String> = None;
    let mut slot = 0usize;
    for hmon in &hmons {
        let mut count = 0u32;
        if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hmon.handle, &mut count) }.is_err() || count == 0 {
            continue;
        }
        let mut handles = vec![PHYSICAL_MONITOR::default(); count as usize];
        if unsafe { GetPhysicalMonitorsFromHMONITOR(hmon.handle, &mut handles) }.is_err() {
            continue;
        }
        for pm in &handles {
            let raw = pm.szPhysicalMonitorDescription;
            let desc = text(&raw);
            let raw_label = if desc.is_empty() {
                hmon.label.clone()
            } else {
                format!("{} · {}", hmon.label, desc)
            };

            let matched_rule = find_matching_rule(&rules_file, slot, &raw_label);
            if matched_rule.is_some_and(|r| r.hidden) {
                slot += 1;
                continue;
            }

            let h_raw = pm.hPhysicalMonitor.0 as isize;
            let mut brightness_vcp = matched_rule
                .and_then(|r| r.brightness_vcp)
                .unwrap_or(VCP_LUMINANCE);
            for (k, &vcp_c) in &rules_file.ddc_brightness_codes {
                if !k.is_empty() && raw_label.to_ascii_lowercase().contains(&k.to_ascii_lowercase()) {
                    brightness_vcp = vcp_c;
                    break;
                }
            }

            let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
            let mut bright_ok = unsafe {
                GetMonitorBrightness(pm.hPhysicalMonitor, &mut min, &mut cur, &mut max) != 0
            } && max > min;

            if !bright_ok {
                if let Some((v_cur, v_min, v_max)) = read_vcp_feature(h_raw, brightness_vcp) {
                    if v_max > v_min {
                        min = v_min as u32;
                        cur = v_cur as u32;
                        max = v_max as u32;
                        bright_ok = true;
                    }
                }
            }

            if bright_ok {
                if let Some(rule) = matched_rule {
                    if let Some(r_min) = rule.min {
                        min = r_min;
                    }
                    if let Some(r_max) = rule.max {
                        if r_max > min {
                            max = r_max;
                        }
                    }
                    cur = cur.clamp(min, max);
                }

                let display_label = matched_rule
                    .filter(|r| !r.custom_name.trim().is_empty())
                    .map(|r| r.custom_name.trim().to_owned())
                    .unwrap_or_else(|| raw_label.clone());

                // 读取扩展 VCP：对比度 0x12、音量 0x62、输入源 0x60
                let contrast = {
                    let (mut c_min, mut c_cur, mut c_max) = (0u32, 0u32, 0u32);
                    let hl_ok = unsafe {
                        GetMonitorContrast(h_raw, &mut c_min, &mut c_cur, &mut c_max) != 0
                    } && c_max > c_min;
                    if hl_ok {
                        Some((c_cur as u16, c_min as u16, c_max as u16))
                    } else {
                        read_vcp_feature(h_raw, VCP_CONTRAST)
                    }
                };
                let volume = read_vcp_feature(h_raw, VCP_AUDIO_VOLUME);
                let input = read_vcp_feature(h_raw, VCP_INPUT_SOURCE).map(|(c, _, _)| c);

                list.push(MonitorInfo {
                    slot,
                    label: display_label,
                    raw_label,
                    cur,
                    min,
                    max,
                    brightness_vcp,
                    contrast,
                    volume,
                    input,
                });
                slot += 1;
            }
        }
        unsafe {
            let _ = DestroyPhysicalMonitors(&handles);
        }
    }
    if let Some(internal) = probe_internal_panel(&rules_file) {
        list.insert(0, internal);
    }
    if list.is_empty() {
        note = Some("没有读到可调亮度的屏幕：外接屏需在 OSD 开启 DDC/CI，台式机如未接支持 DDC 的显示器也无法读写亮度。".to_owned());
    }
    (list, note)
}

#[cfg(windows)]
fn write_vcp_win(slot: usize, code: u8, value: u16) {
    if !MOD_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if slot == INTERNAL_PANEL_SLOT {
        if code == VCP_LUMINANCE {
            write_internal_brightness_win(value);
        }
        return;
    }
    use windows::Win32::Devices::Display::{
        DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
        GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
    };
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};

    unsafe extern "system" fn collect(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let out = &mut *(data.0 as *mut Vec<HMONITOR>);
        out.push(monitor);
        BOOL(1)
    }
    let mut hmons: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut hmons as *mut Vec<HMONITOR> as isize));
    }
    let mut index = 0usize;
    for hmon in hmons {
        let mut count = 0u32;
        if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count) }.is_err() || count == 0 {
            continue;
        }
        let mut handles = vec![PHYSICAL_MONITOR::default(); count as usize];
        if unsafe { GetPhysicalMonitorsFromHMONITOR(hmon, &mut handles) }.is_err() {
            continue;
        }
        for pm in &handles {
            let h_raw = pm.hPhysicalMonitor.0 as isize;
            let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
            let ok = unsafe { GetMonitorBrightness(pm.hPhysicalMonitor, &mut min, &mut cur, &mut max) != 0 }
                && max > min
                || read_vcp_feature(h_raw, VCP_LUMINANCE).is_some_and(|(_, vmin, vmax)| vmax > vmin);
            if ok {
                if index == slot {
                    if code == VCP_LUMINANCE && max > min {
                        let target = (value as u32).clamp(min, max);
                        unsafe {
                            if SetMonitorBrightness(pm.hPhysicalMonitor, target) == 0 {
                                let _ = SetVCPFeature(h_raw, code, target);
                            }
                        }
                    } else if code == VCP_CONTRAST {
                        unsafe {
                            if SetMonitorContrast(h_raw, value as u32) == 0 {
                                let _ = SetVCPFeature(h_raw, code, value as u32);
                            }
                        }
                    } else {
                        unsafe {
                            let _ = SetVCPFeature(h_raw, code, value as u32);
                        }
                    }
                    unsafe {
                        let _ = DestroyPhysicalMonitors(&handles);
                    }
                    return;
                }
                index += 1;
            }
        }
        unsafe {
            let _ = DestroyPhysicalMonitors(&handles);
        }
    }
}

// ---------------------------------------------------------------
// 设置页「显示器亮度」区块（放在系统监控页顶部）
// ---------------------------------------------------------------

pub fn draw_settings(ui: &mut eframe::egui::Ui) {
    use crate::extras::{settings_card, settings_hint, settings_label};

    settings_label(ui, "显示器亮度");
    let list = snapshot();
    settings_card(ui, |ui| {
        if list.is_empty() {
            settings_hint(ui, note().unwrap_or_else(|| "正在检测显示器…".to_owned()).as_str());
            if ui.small_button("重新检测").clicked() {
                request_refresh();
            }
            return;
        }
        for m in &list {
            let mut pct = m.percent() * 100.0;
            let label = if m.label.chars().count() > 20 {
                format!("{}…", m.label.chars().take(19).collect::<String>())
            } else {
                m.label.clone()
            };
            ui.horizontal(|ui| {
                ui.label(
                    eframe::egui::RichText::new(label)
                        .size(10.5)
                        .color(eframe::egui::Color32::from_gray(178)),
                );
                ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                    let r = ui.add(
                        eframe::egui::Slider::new(&mut pct, 0.0..=100.0)
                            .suffix("%")
                            .show_value(true)
                            .max_decimals(0),
                    );
                    if r.changed() {
                        set_value(m.slot, m.value_from_percent(pct / 100.0));
                    }
                });
            });
            ui.add_space(2.0);
        }
        settings_hint(ui, "通过 DDC/CI 直接控制屏幕亮度；内置笔记本屏通常不支持，外接屏需在 OSD 里打开 DDC/CI。");
    });
}
