//! 蓝牙与 2.4GHz 无线外设连接状态和电量监控。
mod state;
pub use state::{BatteryReading, BatterySource, Device, DeviceKind, FRESH_FOR};

mod bas;
pub mod compat;
#[cfg(windows)]
mod receivers;
#[cfg(windows)]
mod windows;

/// 识别顺序：具体型号词 → 通用名称词 → 蓝牙类别码 → 兜底名称词。
/// 短词（pen、mic、band…）按整词匹配，避免 OpenRun 里的 pen 被认成触控笔。
fn device_kind(name: &str, classic: bool, class: Option<u32>) -> DeviceKind {
    use DeviceKind::*;
    let name = name.to_lowercase();
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let word = |w: &str| words.contains(&w);
    let any = |list: &[&str]| list.iter().any(|s| name.contains(s));
    if name.contains("airpods") { return if word("max") { Headphones } else if word("pro") { EarbudsStemTips } else { EarbudsStem }; }
    if name.contains("freebuds") && word("pro") { return EarbudsStemTips; }
    if any(&["joy-con", "joycon"]) { return JoyCon; }
    // Shokz 的开放式耳机不是骨传导，要先于骨传导规则。
    if any(&["openfit", "opendots", "freeclip", "ultra open", "耳夹"]) { return OpenEar; }
    if any(&["shokz", "aftershokz", "openmove", "openrun", "openswim", "骨传导"]) { return Bone; }
    if word("wf") || any(&["earbuds", "buds", "earphones", "入耳"]) { return Earbuds; }
    if word("wh") { return Headphones; }
    if any(&["hearing", "助听"]) { return HearingAid; }
    if any(&["trackpad", "touchpad", "触控板"]) { return Trackpad; }
    if any(&["keyboard", "键盘"]) { return Keyboard; }
    if any(&["mouse", "鼠标"]) { return Mouse; }
    if any(&["stylus", "手写笔", "触控笔"]) || word("pen") || word("pencil") { return Pen; }
    if any(&["presenter", "spotlight", "翻页"]) { return Presenter; }
    if any(&["gamepad", "controller", "dualsense", "dualshock", "手柄"]) { return Gamepad; }
    if any(&["speaker", "soundlink", "音箱", "音响"]) { return Speaker; }
    if any(&["microphone", "麦克风"]) || word("mic") { return Mic; }
    if any(&["遥控"]) || word("remote") { return Remote; }
    if any(&["ipad", "tablet", "matepad", "平板"]) { return Tablet; }
    if any(&["手表"]) || words.iter().any(|w| w.starts_with("watch")) { return Watch; }
    if any(&["手环"]) || word("band") { return Band; }
    if any(&["心率"]) || word("hrm") || word("heart") { return Health; }
    if let Some(class) = class {
        if classic {
            let minor = (class >> 2) & 0x3f;
            match (class >> 8) & 0x1f {
                1 if minor == 7 => return Tablet,
                2 => return Phone,
                4 => return match minor { 4 => Mic, 5 | 7 | 10 => Speaker, _ => Headphones },
                5 => return match ((class >> 6) & 3, (class >> 2) & 0xf) {
                    (1 | 3, _) => Keyboard, (2, _) => Mouse,
                    (_, 1 | 2) => Gamepad, (_, 3) => Remote, (_, 5 | 7) => Pen, _ => Other,
                },
                7 if minor == 1 => return Watch,
                9 => return Health,
                _ => {}
            }
        } else {
            // 低功耗蓝牙外观值：高 10 位是类别，低 6 位是子类别。
            match (class >> 6, class & 0x3f) {
                (15, 1) => return Keyboard,
                (15, 2) => return Mouse,
                (15, 3 | 4) => return Gamepad,
                (15, 5 | 7) => return Pen,
                (15, 9) => return Trackpad,
                (15, 10) => return Presenter,
                (1, _) => return Phone,
                (2, 7) => return Tablet,
                (3, _) => return Watch,
                (6, _) => return Remote,
                (12 | 13 | 14 | 16 | 17 | 18 | 49 | 50, _) => return Health,
                (33, _) => return Speaker,
                (37, 1) => return Earbuds,
                (37, _) => return Headphones,
                (41, _) => return HearingAid,
                _ => {}
            }
        }
    }
    if ["headphone", "headset", "耳机"].iter().any(|s| name.contains(s)) { Headphones } else { Other }
}

/// 释放服务会唤醒后台线程并取消未完成的蓝牙读取。
pub struct Service {
    pub rx: std::sync::mpsc::Receiver<Vec<Device>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Service {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

pub fn spawn() -> Service {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(windows)]
    {
        let stopped = stop.clone();
        let _ = std::thread::Builder::new()
            .name("auralsle-bluetooth".into())
            .spawn(move || windows::run(tx, stopped));
    }
    #[cfg(not(windows))]
    let _ = tx;
    Service { rx, stop }
}

#[cfg(windows)]
pub fn spawn_receivers(store: compat::Store, wake: impl Fn() + Send + 'static) -> Service {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = stop.clone();
    let _ = std::thread::Builder::new()
        .name("auralsle-usb-receivers".into())
        .spawn(move || receivers::run(tx, stopped, store, wake));
    Service { rx, stop }
}

static GLOBAL_PERIPHERALS: std::sync::OnceLock<std::sync::Mutex<Vec<Device>>> = std::sync::OnceLock::new();
static BT_UI_CTX: std::sync::OnceLock<std::sync::Mutex<Option<eframe::egui::Context>>> = std::sync::OnceLock::new();

/// 注册主界面 `egui::Context`，以便后台检测到无线外设连接/断开时第一时间唤醒 UI 线程立即刷新。
pub fn register_ui_ctx(ctx: &eframe::egui::Context) {
    if let Ok(mut slot) = BT_UI_CTX.get_or_init(|| std::sync::Mutex::new(None)).lock() {
        if slot.is_none() {
            *slot = Some(ctx.clone());
        }
    }
}

/// 获取当前已注入的外设快照（Voltide 2.4GHz 与演示设备）。
pub fn connected_peripherals() -> Vec<Device> {
    GLOBAL_PERIPHERALS
        .get_or_init(|| std::sync::Mutex::new(Vec::new()))
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

/// 主动唤醒主界面重绘（用于在收到蓝牙拓扑跳变时立即解除主循环休眠）。
#[cfg(windows)]
pub(super) fn wake_ui() {
    if let Some(mtx) = BT_UI_CTX.get() {
        if let Ok(guard) = mtx.lock() {
            if let Some(ref ctx) = *guard {
                ctx.request_repaint();
            }
        }
    }
}

/// 更新全局已连接外设快照，若连接状态或电量发生变化则立即唤醒主界面重绘。
pub fn set_connected_peripherals(list: Vec<Device>) {
    let mut changed = false;
    if let Ok(mut g) = GLOBAL_PERIPHERALS
        .get_or_init(|| std::sync::Mutex::new(Vec::new()))
        .lock()
    {
        if *g != list {
            *g = list;
            changed = true;
        }
    }
    if changed {
        #[cfg(windows)]
        wake_ui();
    }
}

/// 一键注入/切换 Voltide 2.4GHz 无线外设与蓝牙电量演示设备。
pub fn toggle_demo_peripherals() {
    let cur = connected_peripherals();
    if cur.iter().any(|d| d.id.starts_with("VOLTIDE_DEMO_")) {
        let filtered: Vec<Device> = cur.into_iter().filter(|d| !d.id.starts_with("VOLTIDE_DEMO_")).collect();
        set_connected_peripherals(filtered);
    } else {
        let mut next = cur;
        next.push(Device {
            id: "VOLTIDE_DEMO_MOUSE".into(),
            name: "无线鼠标 (Lightspeed 2.4G)".into(),
            battery: Some(86),
            kind: DeviceKind::Mouse,
            ..Default::default()
        });
        next.push(Device {
            id: "VOLTIDE_DEMO_KB".into(),
            name: "机械键盘 (Keychron Q1 Pro)".into(),
            battery: Some(68),
            kind: DeviceKind::Keyboard,
            ..Default::default()
        });
        next.push(Device {
            id: "VOLTIDE_DEMO_HEADSET".into(),
            name: "无线耳机 (AirPods Pro)".into(),
            battery: Some(92),
            kind: DeviceKind::EarbudsStemTips,
            ..Default::default()
        });
        next.push(Device {
            id: "VOLTIDE_DEMO_GAMEPAD".into(),
            name: "无线手柄 (Xbox Controller)".into(),
            battery: Some(16),
            kind: DeviceKind::Gamepad,
            ..Default::default()
        });
        set_connected_peripherals(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifies_connected_audio_and_input_devices() {
        assert_eq!(device_kind("OpenMove by Shokz", true, Some(0x240418)), DeviceKind::Bone);
        assert_eq!(device_kind("QingLong 87 Ultra 8K", false, Some(961)), DeviceKind::Keyboard);
        assert_eq!(device_kind("MX Anywhere", false, Some(962)), DeviceKind::Mouse);
        assert_eq!(device_kind("耳机", true, Some(0x0404)), DeviceKind::Headphones);
        assert_eq!(device_kind("AirPods", true, None), DeviceKind::EarbudsStem);
        assert_eq!(device_kind("未知设备", false, None), DeviceKind::Other);
    }

    #[test]
    fn recognizes_specific_models_before_generic_words() {
        use DeviceKind::*;
        for (name, kind) in [
            ("小明的 AirPods Pro", EarbudsStemTips), ("AirPods Max", Headphones), ("AirPods", EarbudsStem),
            ("HUAWEI FreeBuds Pro 3", EarbudsStemTips), ("Galaxy Buds2 Pro", Earbuds), ("WF-1000XM5", Earbuds),
            ("WH-1000XM5", Headphones), ("Shokz OpenFit", OpenEar), ("OpenRun Pro by Shokz", Bone),
            ("Joy-Con (L)", JoyCon), ("Xbox Wireless Controller", Gamepad), ("Magic Trackpad", Trackpad),
            ("Surface Slim Pen 2", Pen), ("HUAWEI M-Pencil", Pen), ("Logi Spotlight", Presenter),
            ("DJI Mic 2", Mic), ("Galaxy Watch6", Watch), ("HUAWEI WATCH GT 4", Watch), ("Mi Smart Band 8", Band),
            ("小米手环 8", Band), ("MatePad Pro", Tablet), ("JBL Flip 6 speaker", Speaker),
        ] { assert_eq!(device_kind(name, false, None), kind, "{name}"); }
    }

    #[test]
    fn recognizes_class_and_appearance_codes() {
        use DeviceKind::*;
        assert_eq!(device_kind("X", true, Some(0x0410)), Mic);
        assert_eq!(device_kind("X", true, Some(0x011C)), Tablet);
        assert_eq!(device_kind("X", true, Some(0x0508)), Gamepad);
        assert_eq!(device_kind("X", true, Some(0x050C)), Remote);
        assert_eq!(device_kind("X", true, Some(0x051C)), Pen);
        assert_eq!(device_kind("X", true, Some(0x0704)), Watch);
        assert_eq!(device_kind("X", false, Some(15 << 6 | 9)), Trackpad);
        assert_eq!(device_kind("X", false, Some(15 << 6 | 10)), Presenter);
        assert_eq!(device_kind("X", false, Some(15 << 6 | 7)), Pen);
        assert_eq!(device_kind("X", false, Some(41 << 6)), HearingAid);
        assert_eq!(device_kind("X", false, Some(37 << 6 | 1)), Earbuds);
        assert_eq!(device_kind("X", false, Some(13 << 6)), Health);
    }

    #[test]
    fn summary_omits_missing_battery() {
        let d = Device { id: "1".into(), name: "Headset".into(), battery: None, kind: DeviceKind::Headphones, ..Default::default() };
        assert_eq!(d.summary(), "Headset");
        let d = Device { battery: Some(80), ..d };
        assert_eq!(d.summary(), "Headset · 电量 80%");
    }
}

#[cfg(test)]
mod lifecycle_tests {
    #[test]
    fn dropping_service_signals_stop_even_without_successful_scans() {
        let (_tx, rx) = std::sync::mpsc::channel();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        drop(super::Service { rx, stop });
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed));
    }
}
