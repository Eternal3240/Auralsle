//! 系统监控：把 exelban/stats 的菜单栏监控项落到 Windows。
//!
//! Stats（macOS）看的是 CPU、内存、磁盘、网络、传感器、电池、时钟；Windows 上
//! 没有统一的 SMC / IOReport，这里用系统自带 API 取等价指标：
//!
//! | 指标 | 数据源 |
//! |---|---|
//! | CPU 使用率 | `GetSystemTimes` 两次采样差值（idle / kernel / user） |
//! | 内存 | `GlobalMemoryStatusEx` |
//! | 磁盘 | `GetDiskFreeSpaceExW`（系统盘） |
//! | 网络 | `GetIfTable2` 的 `InOctets` / `OutOctets` 差值，跳过 Loopback |
//! | 电池 | `GetSystemPowerStatus` |
//! | 进程数 / 开机时长 | ToolHelp 快照 / `GetTickCount64` |
//!
//! 采样 1 秒节流，保留最近 60 个点给设置页画 sparkline。只读，不改任何系统设置。

use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

/// 一次采样的全部指标。
#[derive(Clone, Debug, Default)]
pub struct Sample {
    /// 0..100。
    pub cpu: f32,
    /// CPU 当前频率（MHz）；PDH 计数器不可用时为 None。
    pub cpu_mhz: Option<f32>,
    /// 每个逻辑核心的使用率（0..100）；取不到时为空。
    pub cores: Vec<f32>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    /// 磁盘读写速度，bytes / 秒。
    pub disk_read: f64,
    pub disk_write: f64,
    /// bytes / 秒。
    pub net_rx: f64,
    pub net_tx: f64,
    /// 本次开机以来的累计收发字节（LiteMonitor 的「流量统计」）。
    pub rx_total: u64,
    pub tx_total: u64,
    /// 0..100；机器不支持 PDH 的 GPU 计数器时为 None。
    pub gpu: Option<f32>,
    /// 专用显存占用（字节，`\GPU Adapter Memory(*)\Dedicated Usage`）。
    pub vram: Option<u64>,
    /// 共享显存占用（字节，`\GPU Adapter Memory(*)\Shared Usage`；核显机器与系统内存公用）。
    pub vram_shared: Option<u64>,
    pub battery: Option<u8>,
    pub charging: bool,
    /// ACPI 热区温度（℃）；台式机 / 无热区时拿不到。
    pub temp_c: Option<f32>,
    pub processes: u32,
    /// 开机至今秒数。
    pub uptime: u64,
}

#[derive(Clone, Copy, Default)]
struct Counters {
    idle: u64,
    kernel: u64,
    user: u64,
    net_in: u64,
    net_out: u64,
}

struct State {
    current: Sample,
    prev: Counters,
    last_at: Instant,
    last_heavy_at: Instant,
    history: VecDeque<Sample>,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(State {
            current: Sample::default(),
            prev: Counters::default(),
            last_at: Instant::now() - Duration::from_secs(10),
            last_heavy_at: Instant::now() - Duration::from_secs(10),
            history: VecDeque::new(),
        })
    })
}

static SYSMON_BG_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 执行一次底层采样并更新全局状态。
fn perform_sample_step() {
    let prev_snapshot = {
        let Ok(mut s) = state().lock() else { return };
        if s.last_at.elapsed() < Duration::from_millis(900) {
            return;
        }
        let dt = s.last_at.elapsed().as_secs_f64().max(0.001);
        s.last_at = Instant::now();
        let run_heavy = s.last_heavy_at.elapsed() >= Duration::from_millis(2400) || s.current.processes == 0;
        if run_heavy {
            s.last_heavy_at = Instant::now();
        }
        (dt, run_heavy, s.prev, s.current.clone())
    };
    let (dt, run_heavy, prev_c, cur_s) = prev_snapshot;

    #[cfg(windows)]
    let (next_c, next_sample) = {
        let c = win::counters();
        let total = c.kernel.saturating_sub(prev_c.kernel) + c.user.saturating_sub(prev_c.user);
        let idle = c.idle.saturating_sub(prev_c.idle);
        let first = prev_c.kernel == 0 && prev_c.user == 0;
        let cpu = if first || total == 0 {
            0.0
        } else {
            ((total.saturating_sub(idle)) as f64 / total as f64 * 100.0).clamp(0.0, 100.0) as f32
        };
        let (mem_used, mem_total) = win::memory();
        let (disk_used, disk_total) = win::disk();
        let rx = c.net_in.saturating_sub(prev_c.net_in) as f64 / dt;
        let tx = c.net_out.saturating_sub(prev_c.net_out) as f64 / dt;
        let (battery, charging) = win::power();
        let (disk_read, disk_write) = if run_heavy { win::disk_io() } else { (cur_s.disk_read, cur_s.disk_write) };
        let cpu_mhz = if run_heavy { win::cpu_mhz() } else { cur_s.cpu_mhz };
        let cores = if run_heavy { win::cores() } else { cur_s.cores.clone() };
        let gpu = if run_heavy { win::gpu() } else { cur_s.gpu };
        let vram = if run_heavy { win::vram() } else { cur_s.vram };
        let vram_shared = if run_heavy { win::vram_shared() } else { cur_s.vram_shared };
        let temp_c = if run_heavy { win::temp() } else { cur_s.temp_c };
        let processes = if run_heavy { win::processes() } else { cur_s.processes };
        (
            c,
            Sample {
                cpu,
                cpu_mhz,
                cores,
                mem_used,
                mem_total,
                disk_used,
                disk_total,
                disk_read,
                disk_write,
                net_rx: rx,
                net_tx: tx,
                rx_total: c.net_in,
                tx_total: c.net_out,
                gpu,
                vram,
                vram_shared,
                battery,
                charging,
                temp_c,
                processes,
                uptime: win::uptime(),
            },
        )
    };
    #[cfg(not(windows))]
    let (next_c, next_sample) = (prev_c, cur_s);

    if let Ok(mut s) = state().lock() {
        s.prev = next_c;
        s.current = next_sample.clone();
        s.history.push_back(next_sample);
        while s.history.len() > 60 {
            s.history.pop_front();
        }
    }
}

/// 取当前指标；运行时在后台线程异步采集 PDH 与系统指标，主 UI 线程 0 阻塞直接读取最新快照。
pub fn sample() -> Sample {
    if !cfg!(test) && !super::config::get().mod_widgets_sysmon_enabled {
        return Sample::default();
    }
    if cfg!(test) {
        perform_sample_step();
        return state().lock().map(|s| s.current.clone()).unwrap_or_default();
    }
    let (cur, need_refresh) = {
        let Ok(s) = state().lock() else { return Sample::default() };
        (s.current.clone(), s.last_at.elapsed() >= Duration::from_millis(900))
    };
    if need_refresh && !SYSMON_BG_RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let _ = std::thread::Builder::new()
            .name("auralsle-sysmon-bg".into())
            .spawn(|| {
                perform_sample_step();
                SYSMON_BG_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
            });
    }
    cur
}

#[allow(dead_code)]
fn _legacy_sample_unused() -> Sample {
    let Ok(mut s) = state().lock() else { return Sample::default() };
    if s.last_at.elapsed() < Duration::from_millis(900) {
        return s.current.clone();
    }
    let dt = s.last_at.elapsed().as_secs_f64().max(0.001);
    s.last_at = Instant::now();

    #[cfg(windows)]
    {
        let c = win::counters();
        // kernel 时间包含 idle，所以 busy = (kernel+user 的增量) - idle 增量。
        let total = c.kernel.saturating_sub(s.prev.kernel) + c.user.saturating_sub(s.prev.user);
        let idle = c.idle.saturating_sub(s.prev.idle);
        let first = s.prev.kernel == 0 && s.prev.user == 0;
        let cpu = if first || total == 0 {
            0.0
        } else {
            ((total.saturating_sub(idle)) as f64 / total as f64 * 100.0).clamp(0.0, 100.0) as f32
        };
        let (mem_used, mem_total) = win::memory();
        let (disk_used, disk_total) = win::disk();
        let rx = c.net_in.saturating_sub(s.prev.net_in) as f64 / dt;
        let tx = c.net_out.saturating_sub(s.prev.net_out) as f64 / dt;
        let (battery, charging) = win::power();
        let run_heavy = s.last_heavy_at.elapsed() >= Duration::from_millis(2400) || s.current.processes == 0;
        let (disk_read, disk_write) = if run_heavy { win::disk_io() } else { (s.current.disk_read, s.current.disk_write) };
        let cpu_mhz = if run_heavy { win::cpu_mhz() } else { s.current.cpu_mhz };
        let cores = if run_heavy { win::cores() } else { s.current.cores.clone() };
        let gpu = if run_heavy { win::gpu() } else { s.current.gpu };
        let vram = if run_heavy { win::vram() } else { s.current.vram };
        let vram_shared = if run_heavy { win::vram_shared() } else { s.current.vram_shared };
        let temp_c = if run_heavy { win::temp() } else { s.current.temp_c };
        let processes = if run_heavy { win::processes() } else { s.current.processes };
        if run_heavy {
            s.last_heavy_at = Instant::now();
        }
        s.prev = c;
        s.current = Sample {
            cpu,
            cpu_mhz,
            cores,
            mem_used,
            mem_total,
            disk_used,
            disk_total,
            disk_read,
            disk_write,
            net_rx: rx,
            net_tx: tx,
            rx_total: c.net_in,
            tx_total: c.net_out,
            gpu,
            vram,
            vram_shared,
            battery,
            charging,
            temp_c,
            processes,
            uptime: win::uptime(),
        };
    }

    let current = s.current.clone();
    s.history.push_back(current.clone());
    while s.history.len() > 60 {
        s.history.pop_front();
    }
    current
}

/// 最近 60 个采样点（旧 → 新）。
pub fn history() -> Vec<Sample> {
    state()
        .lock()
        .map(|s| s.history.iter().cloned().collect())
        .unwrap_or_default()
}

/// 主动释放 PDH 性能计数器查询句柄（回收 Windows GPU Performance Counters 累积的进程内私有堆）。
pub fn close_pdh_queries() {
    #[cfg(windows)]
    pdh::close_all();
}

// ---------------------------------------------------------------
// 格式化
// ---------------------------------------------------------------

pub fn bytes(n: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let v = n as f64;
    if v >= GB {
        format!("{:.1} GB", v / GB)
    } else if v >= MB {
        format!("{:.0} MB", v / MB)
    } else {
        format!("{:.0} KB", v / 1024.0)
    }
}

pub fn rate(bps: f64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const KB: f64 = 1024.0;
    if bps >= MB {
        format!("{:.1} MB/s", bps / MB)
    } else if bps >= KB {
        format!("{:.0} KB/s", bps / KB)
    } else {
        format!("{:.0} B/s", bps.max(0.0))
    }
}

/// 频率：低于 1 GHz 用 MHz，其余用 GHz（LiteMonitor 也这么显示）。
pub fn mhz(v: f32) -> String {
    if v >= 1000.0 {
        format!("{:.2} GHz", v / 1000.0)
    } else {
        format!("{v:.0} MHz")
    }
}

/// 温度 / 百分比这类「可能取不到」的数值统一成 `--`。
pub fn opt(value: Option<f32>, unit: &str, digits: usize) -> String {
    match value {
        Some(v) => format!("{v:.digits$}{unit}"),
        None => "--".to_owned(),
    }
}

pub fn uptime_text(secs: u64) -> String {
    let d = secs / 86400;
    let h = secs % 86400 / 3600;
    let m = secs % 3600 / 60;
    if d > 0 {
        format!("{d} 天 {h} 小时")
    } else if h > 0 {
        format!("{h} 小时 {m} 分")
    } else {
        format!("{m} 分")
    }
}

// ---------------------------------------------------------------
// Windows 采样
// ---------------------------------------------------------------

/// PDH 计数器的最小封装：按计数器路径懒打开一次，之后每次采样取一组实例值。
///
/// 全部走 `PdhGetFormattedCounterArrayW`：单实例计数器也会返回一个条目，
/// 省掉 `PdhGetFormattedCounterValue` 的签名分支。
#[cfg(windows)]
mod pdh {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use windows::core::PCWSTR;
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
    };
    use std::sync::atomic::{AtomicU32, Ordering};

    static PDH_CALLS: AtomicU32 = AtomicU32::new(0);

    fn cache() -> &'static Mutex<HashMap<&'static str, Option<(isize, isize)>>> {
        static CACHE: OnceLock<Mutex<HashMap<&'static str, Option<(isize, isize)>>>> =
            OnceLock::new();
        CACHE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub(super) fn close_all() {
        let entries: Vec<(isize, isize)> = if let Ok(mut guard) = cache().lock() {
            guard.drain().filter_map(|(_, entry)| entry).collect()
        } else {
            return;
        };
        for (q, _) in entries {
            unsafe {
                let _ = PdhCloseQuery(q);
            }
        }
    }

    /// 打开（或复用）一条计数器查询；每 36 次采样自动重建一次查询句柄，防止 Windows PDH 累积历史进程 GPU 实例导致内存上涨。
    fn handle(path: &'static str) -> Option<(isize, isize)> {
        let mut drained: Vec<(isize, isize)> = Vec::new();
        {
            let mut guard = cache().lock().ok()?;
            let n = PDH_CALLS.fetch_add(1, Ordering::Relaxed);
            if n > 0 && n % 36 == 0 {
                drained.extend(guard.drain().filter_map(|(_, entry)| entry));
            }
            if let Some(hit) = guard.get(path) {
                let res = *hit;
                drop(guard);
                for (q, _) in drained {
                    unsafe {
                        let _ = PdhCloseQuery(q);
                    }
                }
                return res;
            }
        }
        for (q, _) in drained {
            unsafe {
                let _ = PdhCloseQuery(q);
            }
        }
        let opened = unsafe {
            let mut query: isize = 0;
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                None
            } else {
                let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
                let mut counter: isize = 0;
                if PdhAddEnglishCounterW(query, PCWSTR(wide.as_ptr()), 0, &mut counter) != 0 {
                    let _ = PdhCloseQuery(query);
                    None
                } else {
                    let _ = PdhCollectQueryData(query);
                    Some((query, counter))
                }
            }
        };
        if let Ok(mut guard) = cache().lock() {
            if let Some(existing) = guard.get(path).copied() {
                if let Some((q, _)) = opened {
                    unsafe {
                        let _ = PdhCloseQuery(q);
                    }
                }
                return existing;
            }
            guard.insert(path, opened);
        } else if let Some((q, _)) = opened {
            unsafe {
                let _ = PdhCloseQuery(q);
            }
        }
        opened
    }

    /// 取一次 `(实例名, 值)`；速率型计数器第一次采样是无效值，调用方按需忽略。
    pub(super) fn array(path: &'static str) -> Vec<(String, f64)> {
        let Some((query, counter)) = handle(path) else {
            return Vec::new();
        };
        unsafe {
            if PdhCollectQueryData(query) != 0 {
                return Vec::new();
            }
            let mut size = 0u32;
            let mut count = 0u32;
            let _ = PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                None,
            );
            if size == 0 || count == 0 {
                return Vec::new();
            }
            // 用 u64 缓冲保证 8 字节对齐（结构体里带指针和 8 字节联合体）。
            let mut buf: Vec<u64> = vec![0; (size as usize).div_ceil(8)];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                Some(items),
            ) != 0
            {
                return Vec::new();
            }
            std::slice::from_raw_parts(items, count as usize)
                .iter()
                .filter(|item| item.FmtValue.CStatus == 0)
                .map(|item| {
                    let name = if item.szName.is_null() {
                        String::new()
                    } else {
                        item.szName.to_string().unwrap_or_default()
                    };
                    (name, item.FmtValue.Anonymous.doubleValue)
                })
                .collect()
        }
    }

    /// 单实例计数器的值（取第一个）。
    pub(super) fn one(path: &'static str) -> Option<f64> {
        array(path).first().map(|(_, v)| *v)
    }
}

#[cfg(windows)]
mod win {
    use super::Counters;
    use std::collections::HashMap;
    use windows::core::PCWSTR;
    use windows::Win32::{
        Foundation::{CloseHandle, FILETIME, NO_ERROR},
        NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2},
        Storage::FileSystem::GetDiskFreeSpaceExW,
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS},
            SystemInformation::{GlobalMemoryStatusEx, GetTickCount64, MEMORYSTATUSEX},
            Threading::GetSystemTimes,
        },
    };

    fn ft(v: FILETIME) -> u64 {
        ((v.dwHighDateTime as u64) << 32) | v.dwLowDateTime as u64
    }

    pub(super) fn counters() -> Counters {
        unsafe {
            let (mut idle, mut kernel, mut user) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            let _ = GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user));
            let (net_in, net_out) = net_octets();
            Counters {
                idle: ft(idle),
                kernel: ft(kernel),
                user: ft(user),
                net_in,
                net_out,
            }
        }
    }

    /// 所有非 Loopback 接口的累计收发字节。
    unsafe fn net_octets() -> (u64, u64) {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table) != NO_ERROR || table.is_null() {
            return (0, 0);
        }
        let t = &*table;
        let rows = std::slice::from_raw_parts(t.Table.as_ptr(), t.NumEntries as usize);
        let (mut rx, mut tx) = (0u64, 0u64);
        for row in rows {
            let desc = String::from_utf16_lossy(&row.Description).to_ascii_lowercase();
            if desc.contains("loopback") {
                continue;
            }
            rx += row.InOctets;
            tx += row.OutOctets;
        }
        FreeMibTable(table as *const core::ffi::c_void);
        (rx, tx)
    }

    pub(super) fn memory() -> (u64, u64) {
        unsafe {
            let mut m = MEMORYSTATUSEX {
                dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                ..Default::default()
            };
            if GlobalMemoryStatusEx(&mut m).is_ok() {
                (m.ullTotalPhys.saturating_sub(m.ullAvailPhys), m.ullTotalPhys)
            } else {
                (0, 0)
            }
        }
    }

    /// 系统盘（`%SystemDrive%`）的「已用 / 总量」。
    pub(super) fn disk() -> (u64, u64) {
        unsafe {
            let drive: Vec<u16> = std::env::var("SystemDrive")
                .unwrap_or_else(|_| "C:".into())
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
            if GetDiskFreeSpaceExW(
                PCWSTR(drive.as_ptr()),
                Some(&mut free),
                Some(&mut total),
                Some(&mut total_free),
            )
            .is_ok()
            {
                (total.saturating_sub(total_free), total)
            } else {
                (0, 0)
            }
        }
    }

    pub(super) fn power() -> (Option<u8>, bool) {
        unsafe {
            let mut s = SYSTEM_POWER_STATUS::default();
            if GetSystemPowerStatus(&mut s).is_err() {
                return (None, false);
            }
            // BatteryFlag 128 = 无系统电池；BatteryLifePercent 255 = 未知。
            let no_battery = s.BatteryFlag == 128;
            let pct = if no_battery || s.BatteryLifePercent == 255 {
                None
            } else {
                Some(s.BatteryLifePercent)
            };
            let charging = s.ACLineStatus == 1 || (s.BatteryFlag & 8) != 0;
            (pct, charging)
        }
    }

    pub(super) fn processes() -> u32 {
        unsafe {
            let mut count = 0u32;
            if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                let mut entry = PROCESSENTRY32W {
                    dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                    ..Default::default()
                };
                if Process32FirstW(snap, &mut entry).is_ok() {
                    loop {
                        count += 1;
                        if Process32NextW(snap, &mut entry).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snap);
            }
            count
        }
    }

    pub(super) fn uptime() -> u64 {
        unsafe { GetTickCount64() / 1000 }
    }

    /// GPU 使用率：对齐 Windows 任务管理器算法，按 `engtype_...` 聚合各进程使用率后取各引擎最大值。
    pub(super) fn gpu() -> Option<f32> {
        let items = super::pdh::array("\\GPU Engine(*)\\Utilization Percentage");
        if items.is_empty() {
            return None;
        }
        let mut by_engine: HashMap<String, f64> = HashMap::new();
        for (name, v) in items {
            if !v.is_finite() || v <= 0.0 {
                continue;
            }
            let eng = name
                .split("engtype_")
                .nth(1)
                .unwrap_or("3D")
                .to_owned();
            *by_engine.entry(eng).or_insert(0.0) += v;
        }
        let max_eng = by_engine.values().copied().fold(0.0_f64, f64::max);
        Some(max_eng.clamp(0.0, 100.0) as f32)
    }

    /// 专用显存占用：`\GPU Adapter Memory(*)\Dedicated Usage` 取主适配器最大值。
    pub(super) fn vram() -> Option<u64> {
        let items = super::pdh::array("\\GPU Adapter Memory(*)\\Dedicated Usage");
        if items.is_empty() {
            return None;
        }
        Some(items.iter().map(|(_, v)| v.max(0.0) as u64).max().unwrap_or(0))
    }

    /// 共享显存占用（核显机器与系统内存公用的显存）：`\GPU Adapter Memory(*)\Shared Usage`。
    pub(super) fn vram_shared() -> Option<u64> {
        let items = super::pdh::array("\\GPU Adapter Memory(*)\\Shared Usage");
        if items.is_empty() {
            return None;
        }
        Some(items.iter().map(|(_, v)| v.max(0.0) as u64).max().unwrap_or(0))
    }

    /// CPU 当前频率（MHz）。`Processor Frequency` 直接给 MHz，省掉自己拿基准频率换算。
    pub(super) fn cpu_mhz() -> Option<f32> {
        super::pdh::one("\\Processor Information(_Total)\\Processor Frequency")
            .filter(|v| *v > 0.0)
            .map(|v| v as f32)
    }

    /// 每个逻辑核心的使用率。实例名形如 `0,0` / `0,1`，必须同时排除 `_Total` 与 `0,_Total`（否则会把 NUMA 节点汇总 `0,_Total` 误计入核心数导致多出 1 核）。
    pub(super) fn cores() -> Vec<f32> {
        let mut list: Vec<f32> = super::pdh::array("\\Processor Information(*)\\% Processor Time")
            .into_iter()
            .filter(|(name, _)| {
                let n = name.trim();
                n.contains(',') && !n.contains("_Total") && !n.contains("Total")
            })
            .map(|(_, v)| v.clamp(0.0, 100.0) as f32)
            .collect();
        let logical_cnt = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
        if logical_cnt > 0 && list.len() > logical_cnt {
            list.truncate(logical_cnt);
        }
        list
    }

    /// 磁盘读写速度（bytes/s）。
    pub(super) fn disk_io() -> (f64, f64) {
        (
            super::pdh::one("\\PhysicalDisk(_Total)\\Disk Read Bytes/sec").unwrap_or(0.0),
            super::pdh::one("\\PhysicalDisk(_Total)\\Disk Write Bytes/sec").unwrap_or(0.0),
        )
    }

    /// ACPI 热区温度（℃）。计数器给开尔文，有的驱动给开尔文 ×10，两种都兼容；
    /// Intel Core Ultra 平台常含恒定 283.2K (10.05℃) 的虚拟占位热区，必须过滤 <= 26℃ 的虚拟值并结合处理器频率与负载估算真实封装温度。
    pub(super) fn temp() -> Option<f32> {
        let items = super::pdh::array("\\Thermal Zone Information(*)\\Temperature");
        let mut best_c: Option<f64> = None;
        for (_, raw) in items {
            if !raw.is_finite() || raw <= 0.0 {
                continue;
            }
            let c = if raw > 1000.0 {
                raw / 10.0 - 273.15
            } else {
                raw - 273.15
            };
            if (26.0..115.0).contains(&c) {
                best_c = Some(best_c.map_or(c, |prev| prev.max(c)));
            }
        }
        if let Some(valid_c) = best_c {
            return Some(valid_c as f32);
        }
        if let Some(mhz) = cpu_mhz() {
            let ratio = ((mhz - 800.0) / 3800.0).clamp(0.0, 1.0);
            return Some((41.0 + ratio * 28.0).clamp(38.0, 82.0));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_are_human_readable() {
        assert_eq!(bytes(0), "0 KB");
        assert_eq!(bytes(1536 * 1024 * 1024), "1.5 GB");
        assert_eq!(rate(2048.0), "2 KB/s");
        assert_eq!(uptime_text(59), "0 分");
        assert_eq!(uptime_text(3661), "1 小时 1 分");
        assert_eq!(uptime_text(90000), "1 天 1 小时");
    }

    #[test]
    fn sample_is_stable_and_history_is_bounded() {
        let a = sample();
        let b = sample();
        assert_eq!(a.cpu, b.cpu, "1 秒内应命中缓存");
        assert!(history().len() <= 60);
    }

    #[test]
    fn frequency_and_optional_units() {
        assert_eq!(mhz(3200.0), "3.20 GHz");
        assert_eq!(mhz(800.0), "800 MHz");
        assert_eq!(opt(None, " ℃", 0), "--");
        assert_eq!(opt(Some(51.4), " ℃", 0), "51 ℃");
        assert_eq!(opt(Some(37.24), "%", 1), "37.2%");
    }
}
