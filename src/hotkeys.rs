//! 全局热键（Twinkle Tray 风格的「亮度 + / 亮度 − / 显示器静音」快捷键）。
//!
//! 关键约束：
//! - `RegisterHotKey` 必须在拥有消息循环的线程注册，且 `WM_HOTKEY` 会投递到该线程的消息队列；
//! - 岛仔主线程由 winit 接管事件循环，不能在 `monitors` 后台线程直接调 `RegisterHotKey`；
//! - 因此这里启动一条独立的 `auralsle-hotkeys` 消息循环线程，用 `PeekMessageW` / `MsgWaitForMultipleObjects`
//!   同时处理热键重新注册指令与 `WM_HOTKEY` 事件，并将冲突状态实时回写供设置页展示。

use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    BrightnessUp,
    BrightnessDown,
    ToggleMute,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HotkeyConfig {
    pub enabled: bool,
    pub step_percent: u32,
    pub up_hotkey: String,
    pub down_hotkey: String,
    pub mute_hotkey: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            step_percent: 5,
            up_hotkey: "Ctrl+Alt+Up".to_owned(),
            down_hotkey: "Ctrl+Alt+Down".to_owned(),
            mute_hotkey: "Ctrl+Alt+M".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HotkeyStatus {
    pub up_ok: bool,
    pub up_err: Option<String>,
    pub down_ok: bool,
    pub down_err: Option<String>,
    pub mute_ok: bool,
    pub mute_err: Option<String>,
}

static CFG: OnceLock<Mutex<HotkeyConfig>> = OnceLock::new();
static STATUS: OnceLock<Mutex<HotkeyStatus>> = OnceLock::new();
static TX: OnceLock<Sender<HotkeyConfig>> = OnceLock::new();

fn cfg_path() -> std::path::PathBuf {
    crate::model::config_dir().join("monitor-hotkeys.json")
}

fn load_disk() -> HotkeyConfig {
    let Ok(text) = std::fs::read_to_string(cfg_path()) else {
        return HotkeyConfig::default();
    };
    serde_json::from_str::<HotkeyConfig>(&text).unwrap_or_default()
}

fn save_disk(cfg: &HotkeyConfig) {
    let _ = std::fs::create_dir_all(crate::model::config_dir());
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(cfg_path(), json);
    }
}

fn cfg_slot() -> &'static Mutex<HotkeyConfig> {
    CFG.get_or_init(|| Mutex::new(load_disk()))
}

fn status_slot() -> &'static Mutex<HotkeyStatus> {
    STATUS.get_or_init(|| Mutex::new(HotkeyStatus::default()))
}

pub fn get_config() -> HotkeyConfig {
    start();
    cfg_slot().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn get_status() -> HotkeyStatus {
    start();
    status_slot().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn update_config(next: HotkeyConfig) {
    start();
    save_disk(&next);
    *cfg_slot().lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    if let Some(tx) = TX.get() {
        let _ = tx.send(next);
    }
}

/// 启动独立的消息循环线程并注册全局热键（幂等）。
pub fn start() {
    if TX.get().is_some() {
        return;
    }
    let initial = cfg_slot().lock().unwrap_or_else(|e| e.into_inner()).clone();
    let (tx, rx) = channel::<HotkeyConfig>();
    let spawned = std::thread::Builder::new()
        .name("auralsle-hotkeys".into())
        .spawn(move || {
            #[cfg(windows)]
            run_win_loop(initial, rx);
            #[cfg(not(windows))]
            {
                let mut cur = initial;
                loop {
                    let st = HotkeyStatus {
                        up_ok: !cur.up_hotkey.trim().is_empty(),
                        up_err: None,
                        down_ok: !cur.down_hotkey.trim().is_empty(),
                        down_err: None,
                        mute_ok: !cur.mute_hotkey.trim().is_empty(),
                        mute_err: None,
                    };
                    *status_slot().lock().unwrap_or_else(|e| e.into_inner()) = st;
                    match rx.recv() {
                        Ok(next) => cur = next,
                        Err(_) => break,
                    }
                }
            }
        });
    if spawned.is_ok() {
        let _ = TX.set(tx);
    }
}

/// 解析类似 `"Ctrl+Alt+Up"`、`"Ctrl+Shift+F1"` 的快捷键字符串为 Win32 `(fsModifiers, vk)`。
/// `MOD_ALT = 0x0001`, `MOD_CONTROL = 0x0002`, `MOD_SHIFT = 0x0004`, `MOD_WIN = 0x0008`, `MOD_NOREPEAT = 0x4000`。
pub fn parse_accelerator(accel: &str) -> Option<(u32, u32)> {
    let trimmed = accel.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut mods: u32 = 0x4000; // MOD_NOREPEAT
    let mut vk: Option<u32> = None;

    for raw_part in trimmed.split('+') {
        let part = raw_part.trim();
        if part.is_empty() {
            continue;
        }
        let upper = part.to_ascii_uppercase();
        match upper.as_str() {
            "CTRL" | "CONTROL" => mods |= 0x0002,
            "ALT" => mods |= 0x0001,
            "SHIFT" => mods |= 0x0004,
            "WIN" | "SUPER" | "META" => mods |= 0x0008,
            "UP" | "ARROWUP" => vk = Some(0x26),
            "DOWN" | "ARROWDOWN" => vk = Some(0x28),
            "LEFT" | "ARROWLEFT" => vk = Some(0x25),
            "RIGHT" | "ARROWRIGHT" => vk = Some(0x27),
            "PGUP" | "PAGEUP" => vk = Some(0x21),
            "PGDN" | "PAGEDOWN" => vk = Some(0x22),
            "HOME" => vk = Some(0x24),
            "END" => vk = Some(0x23),
            "INSERT" | "INS" => vk = Some(0x2D),
            "DELETE" | "DEL" => vk = Some(0x2E),
            "SPACE" => vk = Some(0x20),
            "TAB" => vk = Some(0x09),
            "ENTER" | "RETURN" => vk = Some(0x0D),
            "PLUS" | "=" | "+" => vk = Some(0xBB),
            "MINUS" | "-" | "_" => vk = Some(0xBD),
            s if s.starts_with('F') && s.len() >= 2 => {
                if let Ok(n) = s[1..].parse::<u32>() {
                    if (1..=24).contains(&n) {
                        vk = Some(0x70 + n - 1);
                    }
                }
            }
            s if s.len() == 1 => {
                let b = s.as_bytes()[0];
                if b.is_ascii_uppercase() || b.is_ascii_digit() {
                    vk = Some(b as u32);
                }
            }
            _ => return None,
        }
    }
    vk.map(|v| (mods, v))
}

/// 从 egui 当前帧按键事件合成快捷键字符串（用于设置页「点击录入快捷键」交互）。
pub fn capture_egui_chord(ui: &eframe::egui::Ui) -> Option<String> {
    use eframe::egui::Key;
    ui.input(|i| {
        if i.key_pressed(Key::Escape) || i.key_pressed(Key::Backspace) {
            return Some(String::new());
        }
        let key_name = [
            (Key::ArrowUp, "Up"),
            (Key::ArrowDown, "Down"),
            (Key::ArrowLeft, "Left"),
            (Key::ArrowRight, "Right"),
            (Key::PageUp, "PageUp"),
            (Key::PageDown, "PageDown"),
            (Key::Home, "Home"),
            (Key::End, "End"),
            (Key::Insert, "Insert"),
            (Key::Delete, "Delete"),
            (Key::Space, "Space"),
            (Key::Plus, "Plus"),
            (Key::Equals, "Plus"),
            (Key::Minus, "Minus"),
            (Key::F1, "F1"),
            (Key::F2, "F2"),
            (Key::F3, "F3"),
            (Key::F4, "F4"),
            (Key::F5, "F5"),
            (Key::F6, "F6"),
            (Key::F7, "F7"),
            (Key::F8, "F8"),
            (Key::F9, "F9"),
            (Key::F10, "F10"),
            (Key::F11, "F11"),
            (Key::F12, "F12"),
            (Key::A, "A"),
            (Key::B, "B"),
            (Key::C, "C"),
            (Key::D, "D"),
            (Key::E, "E"),
            (Key::F, "F"),
            (Key::G, "G"),
            (Key::H, "H"),
            (Key::I, "I"),
            (Key::J, "J"),
            (Key::K, "K"),
            (Key::L, "L"),
            (Key::M, "M"),
            (Key::N, "N"),
            (Key::O, "O"),
            (Key::P, "P"),
            (Key::Q, "Q"),
            (Key::R, "R"),
            (Key::S, "S"),
            (Key::T, "T"),
            (Key::U, "U"),
            (Key::V, "V"),
            (Key::W, "W"),
            (Key::X, "X"),
            (Key::Y, "Y"),
            (Key::Z, "Z"),
            (Key::Num0, "0"),
            (Key::Num1, "1"),
            (Key::Num2, "2"),
            (Key::Num3, "3"),
            (Key::Num4, "4"),
            (Key::Num5, "5"),
            (Key::Num6, "6"),
            (Key::Num7, "7"),
            (Key::Num8, "8"),
            (Key::Num9, "9"),
        ]
        .into_iter()
        .find_map(|(k, name)| i.key_pressed(k).then_some(name))?;

        let mut parts: Vec<&str> = Vec::new();
        if i.modifiers.ctrl || i.modifiers.command {
            parts.push("Ctrl");
        }
        if i.modifiers.alt {
            parts.push("Alt");
        }
        if i.modifiers.shift {
            parts.push("Shift");
        }
        parts.push(key_name);
        Some(parts.join("+"))
    })
}

#[cfg(windows)]
fn run_win_loop(initial: HotkeyConfig, rx: std::sync::mpsc::Receiver<HotkeyConfig>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS};
    use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, MSG, PM_REMOVE, WM_HOTKEY};

    const ID_UP: i32 = 0x7401;
    const ID_DOWN: i32 = 0x7402;
    const ID_MUTE: i32 = 0x7403;

    fn apply_registration(cfg: &HotkeyConfig) -> HotkeyStatus {
        unsafe {
            let _ = UnregisterHotKey(HWND::default(), ID_UP);
            let _ = UnregisterHotKey(HWND::default(), ID_DOWN);
            let _ = UnregisterHotKey(HWND::default(), ID_MUTE);
        }
        if !cfg.enabled {
            return HotkeyStatus::default();
        }
        let reg_one = |id: i32, text: &str| -> (bool, Option<String>) {
            let s = text.trim();
            if s.is_empty() {
                return (false, None);
            }
            let Some((mods, vk)) = parse_accelerator(s) else {
                return (false, Some(format!("快捷键格式无效：{s}")));
            };
            let ok = unsafe { RegisterHotKey(HWND::default(), id, HOT_KEY_MODIFIERS(mods), vk).is_ok() };
            if ok {
                (true, None)
            } else {
                (false, Some(format!("热键「{s}」已被系统或其他应用占用")))
            }
        };
        let (up_ok, up_err) = reg_one(ID_UP, &cfg.up_hotkey);
        let (down_ok, down_err) = reg_one(ID_DOWN, &cfg.down_hotkey);
        let (mute_ok, mute_err) = reg_one(ID_MUTE, &cfg.mute_hotkey);
        HotkeyStatus {
            up_ok,
            up_err,
            down_ok,
            down_err,
            mute_ok,
            mute_err,
        }
    }

    let mut cur_cfg = initial;
    let st = apply_registration(&cur_cfg);
    *status_slot().lock().unwrap_or_else(|e| e.into_inner()) = st;

    loop {
        while let Ok(next_cfg) = rx.try_recv() {
            cur_cfg = next_cfg;
            let st = apply_registration(&cur_cfg);
            *status_slot().lock().unwrap_or_else(|e| e.into_inner()) = st;
        }
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() } {
            if msg.message == WM_HOTKEY && cur_cfg.enabled {
                let step = (cur_cfg.step_percent.clamp(1, 50)) as i32;
                match msg.wParam.0 as i32 {
                    ID_UP => crate::monitors::step_all_brightness(step),
                    ID_DOWN => crate::monitors::step_all_brightness(-step),
                    ID_MUTE => crate::monitors::toggle_all_monitors_mute(),
                    _ => {}
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}
