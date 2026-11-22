//! 经典蓝牙与低功耗蓝牙设备的连接状态和电量。

use std::time::{Duration,Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub battery: Option<u8>,
    pub kind: DeviceKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind { Bone, Earbuds, Headphones, Speaker, Keyboard, Mouse, Gamepad, Phone, Other }

fn device_kind(name: &str, classic: bool, class: Option<u32>) -> DeviceKind {
    use DeviceKind::*;
    let name = name.to_lowercase();
    if ["shokz", "aftershokz", "openmove", "openrun", "骨传导"].iter().any(|s| name.contains(s)) { return Bone; }
    if ["earbuds", "airpods", "buds", "earphones", "入耳"].iter().any(|s| name.contains(s)) { return Earbuds; }
    if ["keyboard", "键盘"].iter().any(|s| name.contains(s)) { return Keyboard; }
    if ["mouse", "鼠标"].iter().any(|s| name.contains(s)) { return Mouse; }
    if ["gamepad", "controller", "手柄"].iter().any(|s| name.contains(s)) { return Gamepad; }
    if ["speaker", "音箱", "音响"].iter().any(|s| name.contains(s)) { return Speaker; }
    if let Some(class) = class {
        if classic {
            match (class >> 8) & 0x1f {
                2 => return Phone,
                4 => return if matches!((class >> 2) & 0x3f, 5 | 7 | 10) { Speaker } else { Headphones },
                5 => return match (class >> 6) & 3 { 1 => Keyboard, 2 => Mouse, 3 => Keyboard, _ => Other },
                _ => {}
            }
        } else {
            match (class >> 6, class & 0x3f) {
                (15, 1) => return Keyboard,
                (15, 2) => return Mouse,
                (15, 3 | 4) => return Gamepad,
                (1, _) => return Phone,
                _ => {}
            }
        }
    }
    if ["headphone", "headset", "耳机"].iter().any(|s| name.contains(s)) { Headphones } else { Other }
}

impl Device {
    /// 通知条正文：`名称 · 电量 80%`，设备没上报电量时只写名称。
    pub fn summary(&self) -> String {
        match self.battery {
            Some(b) => format!("{} · 电量 {b}%", self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Default)]
struct Cache {devices:Vec<Device>,battery_at:Option<Instant>}
impl Cache {
    fn update(&mut self,mut devices:Vec<Device>,now:Instant,read:impl FnOnce(&[Device])->Result<Vec<(String,u8)>,()>)->Vec<Device>{
        let changed=devices.len()!=self.devices.len() || devices.iter().any(|d|!self.devices.iter().any(|old|old.id==d.id));
        for device in &mut devices {device.battery=self.devices.iter().find(|old|old.id==device.id).and_then(|old|old.battery);}
        if devices.is_empty(){self.battery_at=None;}
        else if changed || self.battery_at.is_none_or(|at|now.duration_since(at)>=Duration::from_secs(60)) {
            // 失败同样限频，避免设备驱动异常时每两秒重新枚举整机。
            self.battery_at=Some(now);
            if let Ok(levels)=read(&devices){for device in &mut devices {device.battery=levels.iter().find(|(id,_)|id==&device.id).map(|(_,level)|*level);}}
        }
        self.devices=devices.clone();devices
    }
}

/// 释放服务时唤醒后台线程，停止后续扫描。
pub struct Service {
    pub rx: std::sync::mpsc::Receiver<Vec<Device>>,
    stop: std::sync::mpsc::Sender<()>,
}
impl Drop for Service {
    fn drop(&mut self) {let _=self.stop.send(());}
}

pub fn spawn() -> Service {
    // [修复需求 7 / v3.8.5+]：无界通道保留全部状态跳变，WinRT DeviceWatcher 事件驱动 + 快速发射拓扑变更，不等待慢速整机 HID/电量枚举。
    let (tx, rx) = std::sync::mpsc::channel();
    let (stop, stopped) = std::sync::mpsc::channel();
    std::thread::Builder::new().name("auralsle-bluetooth".into()).spawn(move || {
        #[cfg(windows)]
        {
            let (wake_tx, wake_rx) = std::sync::mpsc::sync_channel::<()>(1);
            let watchers = start_bt_watchers(wake_tx);
            let mut cache = Cache::default();
            let mut bus_state = BusScanState::default();
            let mut last_emitted: Option<Vec<Device>> = None;
            loop {
                if stopped.try_recv() != Err(std::sync::mpsc::TryRecvError::Empty) {
                    stop_bt_watchers(watchers);
                    return;
                }
                if let Some(fast) = query_fast(&mut bus_state, &cache) {
                    let fast_topology_changed = last_emitted
                        .as_ref()
                        .is_none_or(|prev| !same_topology(prev, &fast));
                    if fast_topology_changed {
                        emit_devices(&tx, &fast, &mut last_emitted);
                    }
                    if let Ok(full) = query_full_with_batteries(&mut cache, fast) {
                        if last_emitted.as_ref().is_none_or(|prev| *prev != full) {
                            emit_devices(&tx, &full, &mut last_emitted);
                        }
                    }
                }
                // 优先响应 WinRT DeviceWatcher 的瞬时到达事件；收到首个事件后略微合并 35ms 再扫一次（避免双模耳机经典/BLE分两批触发抖动）
                match wake_rx.recv_timeout(Duration::from_millis(450)) {
                    Ok(()) => {
                        if stopped.recv_timeout(Duration::from_millis(35))
                            != Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                        {
                            stop_bt_watchers(watchers);
                            return;
                        }
                        while wake_rx.try_recv().is_ok() {}
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        if stopped.recv_timeout(Duration::from_millis(450))
                            != Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                        {
                            stop_bt_watchers(watchers);
                            return;
                        }
                    }
                }
            }
        }
        #[cfg(not(windows))]
        loop {
            let _ = &tx;
            if stopped.recv_timeout(Duration::from_millis(650)) != Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                return;
            }
        }
    }).ok();
    Service { rx, stop }
}

#[cfg(windows)]
fn same_topology(a: &[Device], b: &[Device]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.id == y.id && x.name == y.name)
}

#[cfg(windows)]
fn emit_devices(
    tx: &std::sync::mpsc::Sender<Vec<Device>>,
    devices: &[Device],
    last_emitted: &mut Option<Vec<Device>>,
) {
    let mut snap = devices.to_vec();
    for d in connected_peripherals() {
        if d.id.starts_with("VOLTIDE_DEMO_") && !snap.iter().any(|x| x.id == d.id) {
            snap.push(d);
        }
    }
    set_connected_peripherals(snap);
    let list = devices.to_vec();
    *last_emitted = Some(list.clone());
    let _ = tx.send(list);
    wake_ui();
}

#[cfg(windows)]
fn start_bt_watchers(wake_tx: std::sync::mpsc::SyncSender<()>) -> Vec<windows::Devices::Enumeration::DeviceWatcher> {
    use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice};
    use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationUpdate, DeviceWatcher};
    use windows::Foundation::TypedEventHandler;

    let mut out = Vec::new();
    let selectors = [
        BluetoothDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected).ok(),
        BluetoothLEDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected).ok(),
    ];
    for sel in selectors.into_iter().flatten() {
        let Ok(watcher) = DeviceInformation::CreateWatcherAqsFilter(&sel) else {
            continue;
        };
        let tx_add = wake_tx.clone();
        let _ = watcher.Added(&TypedEventHandler::<DeviceWatcher, DeviceInformation>::new(move |_, _| {
            let _ = tx_add.try_send(());
            Ok(())
        }));
        let tx_upd = wake_tx.clone();
        let _ = watcher.Updated(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(move |_, _| {
            let _ = tx_upd.try_send(());
            Ok(())
        }));
        let tx_rem = wake_tx.clone();
        let _ = watcher.Removed(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(move |_, _| {
            let _ = tx_rem.try_send(());
            Ok(())
        }));
        if watcher.Start().is_ok() {
            out.push(watcher);
        }
    }
    out
}

#[cfg(windows)]
fn stop_bt_watchers(watchers: Vec<windows::Devices::Enumeration::DeviceWatcher>) {
    for w in watchers {
        let _ = w.Stop();
    }
}

#[cfg(windows)]
#[derive(Default)]
struct BusScanState {
    classic: Vec<Device>,
    le: Vec<Device>,
}

#[cfg(windows)]
fn voltide_cache() -> &'static std::sync::Mutex<(Vec<Device>, Option<Instant>)> {
    static VOLTIDE_CACHE: std::sync::OnceLock<std::sync::Mutex<(Vec<Device>, Option<Instant>)>> =
        std::sync::OnceLock::new();
    VOLTIDE_CACHE.get_or_init(|| std::sync::Mutex::new((Vec::new(), None)))
}

#[cfg(windows)]
fn scan_bus(classic: bool, cache: &Cache) -> Result<Vec<Device>, ()> {
    use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice};
    use windows::Devices::Enumeration::DeviceInformation;
    let selector = if classic {
        BluetoothDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected)
    } else {
        BluetoothLEDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected)
    }
    .map_err(|_| ())?;
    let found = DeviceInformation::FindAllAsyncAqsFilter(&selector)
        .map_err(|_| ())?
        .get()
        .map_err(|_| ())?;
    let mut out = Vec::new();
    for info in found {
        let Ok(id) = info.Id() else { continue };
        let Ok(name) = info.Name() else { continue };
        let name = name.to_string();
        if name.trim().is_empty() { continue; }
        let address = id.to_string().rsplit('-').next().unwrap_or_default().replace(':', "").to_ascii_uppercase();
        if out.iter().any(|d: &Device| d.id == address) { continue; }
        let cached = cache.devices.iter().find(|old| old.id == address && old.name == name);
        let kind = if let Some(old) = cached {
            old.kind
        } else {
            let class = if classic {
                BluetoothDevice::FromIdAsync(&id).ok().and_then(|a| a.get().ok()).and_then(|d| {
                    let class = d.ClassOfDevice().ok().and_then(|c| c.RawValue().ok());
                    let _ = d.Close();
                    class
                })
            } else {
                BluetoothLEDevice::FromIdAsync(&id).ok().and_then(|a| a.get().ok()).and_then(|d| {
                    let class = d.Appearance().ok().and_then(|c| c.RawValue().ok()).map(u32::from);
                    let _ = d.Close();
                    class
                })
            };
            device_kind(&name, classic, class)
        };
        let battery = cached.and_then(|old| old.battery);
        out.push(Device { id: address, kind, name, battery });
    }
    Ok(out)
}

#[cfg(windows)]
fn merge_voltide_devices(devices: &mut Vec<Device>, voltide_devs: Vec<Device>) {
    for vd in voltide_devs {
        if !devices.iter().any(|d| d.id == vd.id || d.name == vd.name) {
            devices.push(vd);
        }
    }
}

#[cfg(windows)]
fn query_fast(bus: &mut BusScanState, cache: &Cache) -> Option<Vec<Device>> {
    let c_res = scan_bus(true, cache);
    let l_res = scan_bus(false, cache);
    if c_res.is_err() && l_res.is_err() {
        return None;
    }
    if let Ok(c) = c_res {
        bus.classic = c;
    }
    if let Ok(l) = l_res {
        bus.le = l;
    }
    let mut devices = Vec::with_capacity(bus.classic.len() + bus.le.len());
    for d in bus.classic.iter().chain(bus.le.iter()) {
        if !devices.iter().any(|x: &Device| x.id == d.id) {
            devices.push(d.clone());
        }
    }
    if !cfg!(test) && crate::auralsle_ai_status::config::get().voltide_enabled {
        let cached_v = voltide_cache().lock().unwrap_or_else(|e| e.into_inner()).0.clone();
        merge_voltide_devices(&mut devices, cached_v);
    }
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    Some(devices)
}

#[cfg(windows)]
fn query_full_with_batteries(cache: &mut Cache, mut devices: Vec<Device>) -> Result<Vec<Device>, ()> {
    if !cfg!(test) && crate::auralsle_ai_status::config::get().voltide_enabled {
        let voltide_devs = {
            let mtx = voltide_cache();
            let mut guard = mtx.lock().unwrap_or_else(|e| e.into_inner());
            let need_rescan = guard.1.is_none_or(|t| t.elapsed() >= Duration::from_millis(1600));
            if need_rescan {
                if let Ok(v) = scan_voltide_hid_peripherals(&devices) {
                    guard.0 = v;
                }
                guard.1 = Some(Instant::now());
            }
            guard.0.clone()
        };
        merge_voltide_devices(&mut devices, voltide_devs);
    }
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(cache.update(devices, Instant::now(), read_batteries))
}

/// [sdparklab/Voltide] 枚举本机通过 2.4GHz 接收器或 HID 电池协议上报电量的无线外设（鼠标、键盘、耳机、手柄）。
#[cfg(windows)]
fn scan_voltide_hid_peripherals(existing_bt: &[Device]) -> Result<Vec<Device>, ()> {
    use windows::core::{GUID, PCWSTR};
    use windows::Win32::Devices::DeviceAndDriverInstallation::*;
    use windows::Win32::Devices::Properties::DEVPROPKEY;
    use windows::Win32::Foundation::HWND;

    const BATTERY_KEY: DEVPROPKEY = DEVPROPKEY {
        fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
        pid: 2,
    };
    let mut out = Vec::new();
    unsafe {
        let set = SetupDiGetClassDevsW(None, PCWSTR::null(), HWND::default(), DIGCF_PRESENT | DIGCF_ALLCLASSES)
            .map_err(|_| ())?;
        for i in 0..u32::MAX {
            let mut data = SP_DEVINFO_DATA {
                cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInfo(set, i, &mut data).is_err() {
                break;
            }
            let Some(level) = byte_prop(set, &data, &BATTERY_KEY).filter(|&b| b > 0 && b <= 100) else {
                continue;
            };
            let Some(instance) = instance_id(set, &data) else { continue };
            let upper = instance.to_ascii_uppercase();
            if existing_bt.iter().any(|d| upper.contains(&d.id)) {
                continue;
            }
            if !upper.starts_with("HID\\") && !upper.starts_with("USB\\") {
                continue;
            }
            let raw_name = device_friendly_name(set, &data).unwrap_or_else(|| "Voltide 2.4G 无线外设".to_owned());
            let low = raw_name.to_ascii_lowercase();
            // 过滤通用系统虚拟 HID 节点，仅保留外设或明确带电量的无线设备
            let kind = device_kind(&raw_name, false, None);
            let is_known_brand = [
                "logitech", "lightspeed", "razer", "hyperspeed", "atk", "vgn", "vxe", "mchose",
                "ajazz", "keychron", "aula", "hyperx", "steelseries", "rog", "wireless", "2.4g",
                "mouse", "keyboard", "headset", "controller", "xbox", "dualsense", "鼠标", "键盘", "耳机", "手柄",
            ]
            .iter()
            .any(|k| low.contains(k));
            if kind != DeviceKind::Other || is_known_brand {
                let id_short = format!("VOLTIDE_{}", upper.chars().filter(|c| c.is_alphanumeric()).take(16).collect::<String>());
                out.push(Device {
                    id: id_short,
                    name: raw_name,
                    battery: Some(level),
                    kind: if kind == DeviceKind::Other { DeviceKind::Mouse } else { kind },
                });
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    Ok(out)
}

#[cfg(windows)]
fn read_batteries(devices:&[Device])->Result<Vec<(String,u8)>,()> {
    use windows::core::{GUID, PCWSTR};
    use windows::Win32::Devices::DeviceAndDriverInstallation::*;
    use windows::Win32::Devices::Properties::DEVPROPKEY;
    use windows::Win32::Foundation::HWND;

    // DEVPKEY_Bluetooth_Battery {104EA319-6EE2-4701-BD47-8DDBF425BBE5}, 2
    const BATTERY: DEVPROPKEY = DEVPROPKEY {
        fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
        pid: 2,
    };
    let mut batteries: Vec<(String, u8)> = Vec::new();
    unsafe {
        let set = SetupDiGetClassDevsW(None, PCWSTR::null(), HWND::default(), DIGCF_PRESENT | DIGCF_ALLCLASSES)
            .map_err(|_| ())?;
        for i in 0..u32::MAX {
            let mut data = SP_DEVINFO_DATA {
                cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInfo(set, i, &mut data).is_err() {
                break;
            }
            let Some(instance) = instance_id(set, &data) else { continue };
            let upper = instance.to_ascii_uppercase();
            if !devices.iter().any(|device|upper.contains(&device.id)){continue;}
            if let Some(level) = byte_prop(set, &data, &BATTERY).filter(|b| *b <= 100) {
                batteries.push((upper.clone(), level));
            }
        }
        // 每次枚举后释放设备列表句柄。
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    Ok(devices.iter().filter_map(|device|batteries.iter().find(|(node,_)|node.contains(&device.id)).map(|(_,level)|(device.id.clone(),*level))).collect())
}

#[cfg(windows)]
unsafe fn instance_id(
    set: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
) -> Option<String> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDeviceInstanceIdW;
    let mut buf = [0u16; 512];
    let mut need = 0u32;
    SetupDiGetDeviceInstanceIdW(set, data, Some(&mut buf), Some(&mut need)).ok()?;
    Some(String::from_utf16_lossy(&buf[..(need as usize).saturating_sub(1)]))
}

#[cfg(windows)]
unsafe fn byte_prop(
    set: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
    key: &windows::Win32::Devices::Properties::DEVPROPKEY,
) -> Option<u8> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDevicePropertyW;
    use windows::Win32::Devices::Properties::DEVPROPTYPE;
    // DEVPROP_TYPE_BYTE
    const BYTE: DEVPROPTYPE = DEVPROPTYPE(0x3);
    let mut ty = DEVPROPTYPE::default();
    let mut buf = [0u8; 1];
    let mut size = 0u32;
    SetupDiGetDevicePropertyW(set, data, key, &mut ty, Some(&mut buf), Some(&mut size), 0).ok()?;
    (ty == BYTE && size == 1).then_some(buf[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id:&str)->Device {Device{id:id.into(),name:id.into(),battery:None,kind:DeviceKind::Mouse}}

    #[test]
    fn idle_bluetooth_does_not_rescan_all_hardware_every_two_seconds(){
        let start=std::time::Instant::now();let mut cache=Cache::default();let mut scans=0;
        for second in (0..360).step_by(2){
            let found=cache.update(vec![device("AA")],start+Duration::from_secs(second),|_|{scans+=1;Ok(vec![("AA".into(),80)])});
            assert_eq!(found[0].battery,Some(80));
        }
        assert_eq!(scans,6,"六分钟内同一设备的电量扫描应最多每分钟一次");
    }

    #[test]
    fn bluetooth_connection_change_refreshes_immediately_and_failed_scans_back_off(){
        let now=std::time::Instant::now();let mut cache=Cache::default();
        cache.update(vec![device("AA")],now,|_|Ok(vec![("AA".into(),80)]));
        let found=cache.update(vec![device("AA"),device("BB")],now+Duration::from_secs(2),|_|Ok(vec![("AA".into(),79),("BB".into(),60)]));
        assert_eq!(found[1].battery,Some(60));
        let found=cache.update(vec![device("AA"),device("BB")],now+Duration::from_secs(62),|_|Err(()));
        assert_eq!(found[0].battery,Some(79));
        cache.update(vec![device("AA"),device("BB")],now+Duration::from_secs(64),|_|panic!("失败后不能立即重复扫描"));
        assert!(cache.update(vec![],now+Duration::from_secs(66),|_|panic!("无设备不能扫描整机")).is_empty());
        cache.update(vec![device("AA")],now+Duration::from_secs(68),|_|Ok(vec![("AA".into(),78)]));
        assert_eq!(cache.devices[0].battery,Some(78));
    }

    #[test]
    fn identifies_connected_audio_and_input_devices() {
        assert_eq!(device_kind("OpenMove by Shokz", true, Some(0x240418)), DeviceKind::Bone);
        assert_eq!(device_kind("QingLong 87 Ultra 8K", false, Some(961)), DeviceKind::Keyboard);
        assert_eq!(device_kind("MX Anywhere", false, Some(962)), DeviceKind::Mouse);
        assert_eq!(device_kind("耳机", true, Some(0x0404)), DeviceKind::Headphones);
        assert_eq!(device_kind("AirPods", true, None), DeviceKind::Earbuds);
        assert_eq!(device_kind("未知设备", false, None), DeviceKind::Other);
    }

    #[test]
    fn summary_omits_missing_battery() {
        let d = Device { id: "1".into(), name: "Headset".into(), battery: None, kind: DeviceKind::Headphones };
        assert_eq!(d.summary(), "Headset");
        let d = Device { battery: Some(80), ..d };
        assert_eq!(d.summary(), "Headset · 电量 80%");
    }
}

#[cfg(test)]
mod lifecycle_tests {
    #[test]
    fn dropping_service_signals_stop_even_without_successful_scans(){
        let (_tx,rx)=std::sync::mpsc::channel();
        let (stop,stopped)=std::sync::mpsc::channel();
        drop(super::Service {rx,stop});
        assert_eq!(stopped.recv_timeout(std::time::Duration::from_millis(100)),Ok(()));
    }
}


static GLOBAL_PERIPHERALS: std::sync::OnceLock<std::sync::Mutex<Vec<Device>>> = std::sync::OnceLock::new();
static BT_UI_CTX: std::sync::OnceLock<std::sync::Mutex<Option<eframe::egui::Context>>> = std::sync::OnceLock::new();

/// 注册主界面 `egui::Context`，以便后台检测到无线外设连接/断开时第一时间唤醒 UI 线程立即刷新设备岛与设备胶囊（需求 7）。
pub fn register_ui_ctx(ctx: &eframe::egui::Context) {
    if let Ok(mut slot) = BT_UI_CTX.get_or_init(|| std::sync::Mutex::new(None)).lock() {
        if slot.is_none() {
            *slot = Some(ctx.clone());
        }
    }
}

/// 获取当前已连接的蓝牙设备与 Voltide 2.4GHz 无线外设列表快照（供灵动岛顶栏、定制组件与设置页实时展示）。
pub fn connected_peripherals() -> Vec<Device> {
    GLOBAL_PERIPHERALS
        .get_or_init(|| std::sync::Mutex::new(Vec::new()))
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

/// 主动唤醒主界面重绘（用于在收到蓝牙拓扑跳变时立即解除主循环休眠）。
#[cfg(windows)]
fn wake_ui() {
    if let Some(mtx) = BT_UI_CTX.get() {
        if let Ok(guard) = mtx.lock() {
            if let Some(ref ctx) = *guard {
                ctx.request_repaint();
            }
        }
    }
}

/// 更新全局已连接外设快照，若连接状态或电量发生变化则立即唤醒主界面重绘！
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
        if let Some(mtx) = BT_UI_CTX.get() {
            if let Ok(guard) = mtx.lock() {
                if let Some(ref ctx) = *guard {
                    ctx.request_repaint();
                }
            }
        }
    }
}

/// 一键注入/切换 Voltide 2.4GHz 无线外设与蓝牙电量演示设备（方便在未插接收器时直观预览岛上电量徽章效果）。
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
        });
        next.push(Device {
            id: "VOLTIDE_DEMO_KB".into(),
            name: "机械键盘 (Keychron Q1 Pro)".into(),
            battery: Some(68),
            kind: DeviceKind::Keyboard,
        });
        next.push(Device {
            id: "VOLTIDE_DEMO_HEADSET".into(),
            name: "无线耳机 (AirPods Pro)".into(),
            battery: Some(92),
            kind: DeviceKind::Earbuds,
        });
        next.push(Device {
            id: "VOLTIDE_DEMO_GAMEPAD".into(),
            name: "无线手柄 (Xbox Controller)".into(),
            battery: Some(16), // 低于 20% 触发 BrimDeck 低电量红色告警提醒
            kind: DeviceKind::Gamepad,
        });
        set_connected_peripherals(next);
    }
}

#[cfg(windows)]
unsafe fn device_friendly_name(
    set: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
) -> Option<String> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiGetDeviceRegistryPropertyW, SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME,
    };
    for prop in [SPDRP_FRIENDLYNAME, SPDRP_DEVICEDESC] {
        let mut buf = [0u8; 512];
        let mut req = 0u32;
        if SetupDiGetDeviceRegistryPropertyW(set, data, prop, None, Some(&mut buf), Some(&mut req)).is_ok() && req >= 2 {
            let u16s: Vec<u16> = buf[..(req as usize).min(buf.len())]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&c| c != 0)
                .collect();
            let s = String::from_utf16_lossy(&u16s).trim().to_owned();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}
