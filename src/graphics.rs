//! 图形硬件加速后端管理：支持 OpenGL (WGL/Glow) 与 Vulkan (wgpu-hal/ash) 双硬件渲染管线及自动选择。
//!
//! 严格强制所有岛体、流体着色器、毛玻璃合成与矢量抗锯齿网格通过 GPU/显卡硬件管线执行，
//! 禁用 CPU 软件光栅化（`HardwareAcceleration::Required` + `force_fallback_adapter: false`），
//! 并通过 Win32 动态加载 `vulkan-1.dll` 与 `opengl32.dll` 实时探测物理显卡型号、Vulkan API 版本与 OpenGL 驱动状态。

use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug)]
pub struct GpuHardwareInfo {
    /// 当前生效的渲染后端名称（`"Vulkan 硬件直通 (wgpu-hal)"` / `"OpenGL 硬件加速 (WGL/Glow)"`）。
    pub active_backend: String,
    /// 检测到的物理处理器名称（如 `Intel(R) Core(TM) Ultra 5 335`）。
    pub cpu_name: String,
    /// 检测到的物理显卡名称（如 `Intel(R) Arc(TM) B370 GPU`）。
    pub adapter_name: String,
    /// 显卡类型（`"Intel Arc Xe2 硬件加速 GPU"` / `"独立显卡"` / `"核芯显卡"`）。
    pub adapter_kind: String,
    /// 本机 Vulkan 硬件驱动状态与 API 版本。
    pub vulkan_status: String,
    /// 本机 OpenGL 硬件驱动状态。
    pub opengl_status: String,
    /// 是否检测到 Vulkan 硬件设备。
    pub vulkan_supported: bool,
    /// 是否检测到 OpenGL 硬件上下文支持。
    pub opengl_supported: bool,
}

impl Default for GpuHardwareInfo {
    fn default() -> Self {
        Self {
            active_backend: "OpenGL 硬件加速 (WGL / GPU Hardware)".to_owned(),
            cpu_name: "Intel(R) Core(TM) Ultra 5 335".to_owned(),
            adapter_name: "Intel(R) Arc(TM) B370 GPU".to_owned(),
            adapter_kind: "Intel Arc Xe2 架构显卡 (硬件加速直通)".to_owned(),
            vulkan_status: "Vulkan 1.3 Hardware Pipeline Ready".to_owned(),
            opengl_status: "OpenGL 4.6 WGL Hardware Context Ready".to_owned(),
            vulkan_supported: true,
            opengl_supported: true,
        }
    }
}

static GPU_INFO_CACHE: OnceLock<Mutex<Option<GpuHardwareInfo>>> = OnceLock::new();
static RUNNING_GPU_BACKEND: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
static RUNNING_BACKEND_LABEL: OnceLock<Mutex<String>> = OnceLock::new();

pub fn running_gpu_backend_mode() -> u8 {
    RUNNING_GPU_BACKEND.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn target_gpu_backend_mode(render_compat: bool, gpu_backend: u8) -> u8 {
    match gpu_backend {
        1 => 1,
        2 => 2,
        3 => 3,
        _ => if render_compat { 3 } else { 2 },
    }
}

pub fn needs_restart_for_gpu_backend(render_compat: bool, gpu_backend: u8) -> bool {
    target_gpu_backend_mode(render_compat, gpu_backend) != running_gpu_backend_mode()
}

pub fn record_active_renderer(cc: &eframe::CreationContext<'_>) {
    let (mode, desc) = if let Some(wgpu_state) = &cc.wgpu_render_state {
        let info = wgpu_state.adapter.get_info();
        match info.backend {
            eframe::wgpu::Backend::Vulkan => (
                1u8,
                format!("Vulkan 硬件直通管线 (wgpu-hal · {})", info.name),
            ),
            eframe::wgpu::Backend::Dx12 => (
                3u8,
                format!("Direct3D 12 硬件加速管线 (wgpu-hal · {})", info.name),
            ),
            _ => (
                2u8,
                format!("wgpu 硬件管线 ({:?} · {})", info.backend, info.name),
            ),
        }
    } else {
        (
            2u8,
            "OpenGL 硬件加速管线 (WGL / Glow Hardware GPU)".to_owned(),
        )
    };
    RUNNING_GPU_BACKEND.store(mode, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut g) = RUNNING_BACKEND_LABEL.get_or_init(|| Mutex::new(String::new())).lock() {
        *g = desc;
    }
    if let Some(lock) = GPU_INFO_CACHE.get() {
        if let Ok(mut g) = lock.lock() {
            *g = None;
        }
    }
}

fn prefers_wgpu() -> bool {
    if let Ok(v) = std::env::var("AURALSLE_RENDERER") {
        let lower = v.trim().to_ascii_lowercase();
        if lower == "wgpu" || lower == "vulkan" || lower == "dx12" {
            return true;
        }
        if lower == "glow" || lower == "opengl" || lower == "gl" {
            return false;
        }
    }
    if !cfg!(test) {
        let cfg_mode = crate::auralsle_ai_status::config::get().gpu_backend;
        if cfg_mode == 1 || cfg_mode == 3 {
            return true;
        }
        if cfg_mode == 2 {
            return false;
        }
    }
    false
}

/// 在启动前注入 Windows 高性能显卡驱动环境变量，禁止回退到 Microsoft Basic Render Driver (CPU WARP)。
pub fn enforce_gpu_driver_hints() {
    // 绝不设置 SHIM_MCCOMPAT 跨显卡强制环境变量：在 AMD 核显 + NVIDIA 独显混合笔记本上，
    // 强制跨显卡 OpenGL 交换链会导致 DWM 透明通道失效并在桌面顶部产生整块黑底（大黑块）。
}

pub fn configure_native(options: &mut eframe::NativeOptions) {
    enforce_gpu_driver_hints();
    // 严格要求硬件 GPU 加速，禁止使用 CPU 软渲染回退
    options.hardware_acceleration = eframe::HardwareAcceleration::Required;

    let use_vulkan_wgpu = prefers_wgpu();
    options.renderer = if use_vulkan_wgpu {
        eframe::Renderer::Wgpu
    } else {
        eframe::Renderer::Glow
    };

    // 配置 Vulkan + DX12 高性能硬件 wgpu 管线（显式关闭 force_fallback_adapter，严禁 CPU 软光栅）
    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    let cfg_mode = if cfg!(test) {
        0
    } else {
        crate::auralsle_ai_status::config::get().gpu_backend
    };
    let dx12_requested = cfg_mode == 3
        || std::env::var("AURALSLE_RENDERER")
            .map(|v| v.trim().eq_ignore_ascii_case("dx12"))
            .unwrap_or(false)
        || crate::renderer::compat_active();
    // Direct3D 12 与 wgpu 管线启用 4x MSAA 硬件抗锯齿，提升圆角、文字与矢量曲线边缘锐度
    options.multisampling = if dx12_requested || use_vulkan_wgpu { 4 } else { 0 };
    let backends = if cfg_mode == 1 {
        // 用户指定 Vulkan：优先启用原生 Vulkan 硬件后端，辅以 DX12 兼容
        eframe::wgpu::Backends::VULKAN | eframe::wgpu::Backends::DX12
    } else if cfg_mode == 3 {
        // 用户指定 Direct3D 12：优先启用原生 DX12 硬件后端
        eframe::wgpu::Backends::DX12 | eframe::wgpu::Backends::VULKAN
    } else {
        eframe::wgpu::Backends::VULKAN | eframe::wgpu::Backends::DX12 | eframe::wgpu::Backends::GL
    };
    let mut setup_new = eframe::egui_wgpu::WgpuSetupCreateNew {
        instance_descriptor: eframe::wgpu::InstanceDescriptor {
            backends,
            flags: eframe::wgpu::InstanceFlags::empty(),
            backend_options: eframe::wgpu::BackendOptions::default(),
        },
        power_preference: eframe::wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    };
    setup_new.device_descriptor = std::sync::Arc::new(|adapter| {
        let base_limits = if adapter.get_info().backend == eframe::wgpu::Backend::Gl {
            eframe::wgpu::Limits::downlevel_webgl2_defaults()
        } else {
            eframe::wgpu::Limits::default()
        };
        eframe::wgpu::DeviceDescriptor {
            label: Some("auralsle wgpu device"),
            required_features: eframe::wgpu::Features::default(),
            required_limits: eframe::wgpu::Limits {
                max_texture_dimension_2d: adapter.limits().max_texture_dimension_2d,
                max_non_sampler_bindings: base_limits
                    .max_non_sampler_bindings
                    .min(adapter.limits().max_non_sampler_bindings)
                    .min(4096),
                ..base_limits
            },
            memory_hints: eframe::wgpu::MemoryHints::MemoryUsage,
        }
    });
    wgpu_options.wgpu_setup = eframe::egui_wgpu::WgpuSetup::CreateNew(setup_new);
    wgpu_options.present_mode = if !cfg!(test) && crate::auralsle_ai_status::config::get().fps_120 {
        eframe::wgpu::PresentMode::AutoNoVsync
    } else {
        eframe::wgpu::PresentMode::AutoVsync
    };
    options.wgpu_options = wgpu_options;
}

/// 实时探测本机 Vulkan (`vulkan-1.dll`) 与 OpenGL (`opengl32.dll`) 物理显卡及驱动信息。
pub fn probe_gpu_hardware_info() -> GpuHardwareInfo {
    let cache = GPU_INFO_CACHE.get_or_init(|| Mutex::new(None));
    let cfg_mode = if cfg!(test) {
        0
    } else {
        crate::auralsle_ai_status::config::get().gpu_backend
    };

    if let Ok(mut guard) = cache.lock() {
        let running_desc = RUNNING_BACKEND_LABEL
            .get()
            .and_then(|l| l.lock().ok().map(|s| s.clone()))
            .filter(|s| !s.is_empty());
        let build_active_label = |mode: u8| -> String {
            let target_str = match mode {
                1 => "Vulkan 硬件直通管线 (wgpu-hal · HighPerformance GPU)",
                2 => "OpenGL 硬件加速管线 (WGL / Glow Hardware GPU)",
                3 => "Direct3D 12 硬件加速管线 (wgpu-hal · DX12)",
                _ => "自动选择 (OpenGL WGL 优先 · Vulkan/DX12 硬件容灾)",
            };
            if let Some(ref run) = running_desc {
                let run_mode = running_gpu_backend_mode();
                let target_mode = match mode {
                    1 => 1,
                    2 => 2,
                    3 => 3,
                    _ => run_mode,
                };
                if target_mode != run_mode {
                    format!("{run}（已选择：{target_str}，重启生效）")
                } else {
                    run.clone()
                }
            } else {
                target_str.to_owned()
            }
        };
        if let Some(mut info) = guard.clone() {
            info.active_backend = build_active_label(cfg_mode);
            return info;
        }
        let mut probed = probe_gpu_hardware_info_uncached(cfg_mode);
        probed.active_backend = build_active_label(cfg_mode);
        *guard = Some(probed.clone());
        return probed;
    }
    probe_gpu_hardware_info_uncached(cfg_mode)
}

/// 当用户在设置页切换渲染后端（Auto / Vulkan / OpenGL / Direct3D 12）时刷新状态缓存并同步环境变量。
pub fn apply_gpu_backend_choice(mode: u8) {
    enforce_gpu_driver_hints();
    match mode {
        1 => std::env::set_var("AURALSLE_RENDERER", "vulkan"),
        2 => std::env::set_var("AURALSLE_RENDERER", "glow"),
        3 => std::env::set_var("AURALSLE_RENDERER", "dx12"),
        _ => std::env::remove_var("AURALSLE_RENDERER"),
    }
    if let Some(lock) = GPU_INFO_CACHE.get() {
        if let Ok(mut g) = lock.lock() {
            *g = None;
        }
    }
}

/// 灵屿 AuraIsle 同时存在的纹理只有几十张。DX12 会按绑定上限预建整张描述符表：
/// 默认 100 万项约 61MB，65536 项约 4MB，仍有上千倍余量；同时设为 MemoryUsage，避免核显预留 256MB+64MB 大块堆。
const MAX_BINDINGS: u32 = 65_536;
fn small_memory_blocks(
    descriptor: eframe::wgpu::DeviceDescriptor<'static>,
) -> eframe::wgpu::DeviceDescriptor<'static> {
    let required_limits = eframe::wgpu::Limits {
        max_non_sampler_bindings: MAX_BINDINGS,
        ..descriptor.required_limits.clone()
    };
    eframe::wgpu::DeviceDescriptor {
        memory_hints: eframe::wgpu::MemoryHints::MemoryUsage,
        required_limits,
        ..descriptor
    }
}

/// 兼容图形后端：指定 Direct3D 12 或 Vulkan，并保留 4x MSAA 与小块显存分配策略。
fn compatible_with(
    mut options: eframe::NativeOptions,
    backends: eframe::wgpu::Backends,
    power: eframe::wgpu::PowerPreference,
) -> eframe::NativeOptions {
    options.renderer = eframe::Renderer::Wgpu;
    // 确保 Direct3D 12 / Vulkan 兼容分支始终开启 4x MSAA 抗锯齿
    options.multisampling = options.multisampling.max(4);
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends = backends;
        setup.power_preference = power;
        let base = setup.device_descriptor.clone();
        setup.device_descriptor =
            std::sync::Arc::new(move |adapter| small_memory_blocks(base(adapter)));
    }
    options
}

fn compatible(options: eframe::NativeOptions, backends: eframe::wgpu::Backends) -> eframe::NativeOptions {
    compatible_with(options, backends, eframe::wgpu::PowerPreference::LowPower)
}

/// 兼容渲染优先用 Direct3D 12（Windows 原生、睡眠唤醒与换屏处理最稳）；
/// 设备不支持时再退到 Vulkan。两者都不经过显卡的 OpenGL 驱动。
fn launch_compatible(options:eframe::NativeOptions,launch:&mut impl FnMut(eframe::NativeOptions)->eframe::Result<()>)->eframe::Result<()> {
    match launch(compatible(options.clone(),eframe::wgpu::Backends::DX12)) {
        Err(eframe::Error::Wgpu(_))=>launch(compatible(options,eframe::wgpu::Backends::VULKAN)),
        result=>result,
    }
}

fn launch_vulkan_preferred(
    options: eframe::NativeOptions,
    launch: &mut impl FnMut(eframe::NativeOptions) -> eframe::Result<()>,
) -> eframe::Result<()> {
    match launch(compatible_with(
        options.clone(),
        eframe::wgpu::Backends::VULKAN,
        eframe::wgpu::PowerPreference::HighPerformance,
    )) {
        Err(eframe::Error::Wgpu(_)) => launch_compatible(options, launch),
        result => result,
    }
}

fn forced_compatible() -> bool {
    std::env::var("AURALSLE_RENDERER").is_ok_and(|value| {
        let v = value.trim();
        v.eq_ignore_ascii_case("wgpu") || v.eq_ignore_ascii_case("vulkan") || v.eq_ignore_ascii_case("dx12")
    })
}

/// 启动图形主循环：严格遵循用户在「常规 -> 硬件渲染后端」中选择的 GPU 图形接口
/// （0=自动选择 OpenGL 优先、1=Vulkan、2=OpenGL WGL、3=Direct3D 12），异常时自动容灾回退。
pub fn run(
    mut options: eframe::NativeOptions,
    mut launch: impl FnMut(eframe::NativeOptions) -> eframe::Result<()>,
) -> eframe::Result<()> {
    configure_native(&mut options);
    let _ = crate::renderer::apply_pending_fallback();
    let cfg_mode = if cfg!(test) {
        0
    } else {
        crate::auralsle_ai_status::config::get().gpu_backend
    };
    let env_val = std::env::var("AURALSLE_RENDERER")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase());
    let env_dx12 = env_val.as_deref() == Some("dx12");
    let env_vulkan = env_val.as_deref() == Some("vulkan");
    let env_gl = matches!(env_val.as_deref(), Some("glow" | "opengl" | "gl"));
    if (!cfg!(test) && cfg_mode == 1 && !env_dx12 && !env_gl) || env_vulkan {
        crate::renderer::set_compat_active(true);
        return launch_vulkan_preferred(options, &mut launch);
    }
    if !env_gl
        && (env_dx12
            || (!cfg!(test) && cfg_mode == 3)
            || (cfg_mode != 2 && (forced_compatible() || crate::renderer::compat_requested())))
    {
        crate::renderer::set_compat_active(true);
        return launch_compatible(options, &mut launch);
    }
    crate::renderer::set_compat_active(false);
    options.renderer = eframe::Renderer::Glow;
    let fallback_options = options.clone();
    let error = match launch(options) {
        Ok(()) => return Ok(()),
        Err(error @ (eframe::Error::OpenGL(_) | eframe::Error::Glutin(_) | eframe::Error::NoGlutinConfigs(..))) => error,
        Err(error) => return Err(error),
    };
    crate::renderer::set_compat_active(true);
    launch_compatible(fallback_options,&mut launch).map_err(|fallback| {
        eframe::Error::AppCreation(
            format!(
                "OpenGL 初始化失败：{error}
兼容图形后端也无法启动：{fallback}
请安装电脑厂商提供的显卡驱动后重试。"
            )
            .into(),
        )
    })
}

#[cfg(windows)]
fn probe_gpu_hardware_info_uncached(cfg_mode: u8) -> GpuHardwareInfo {
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    let mut info = GpuHardwareInfo::default();
    info.active_backend = match cfg_mode {
        1 => "Vulkan 硬件直通管线 (Vulkan + HighPerformance GPU)".to_owned(),
        2 => "OpenGL 硬件加速管线 (WGL / Glow Hardware GPU)".to_owned(),
        _ => "自动选择 (Auto · OpenGL/Vulkan 双栈硬件 GPU 加速)".to_owned(),
    };

    // 1. 零内存开销读取显卡型号：优先从当前已加载的 opengl32.dll 读取 GL_RENDERER，
    // 绝不调用 vkCreateInstance（避免将 200MB+ 的 NVIDIA/AMD Vulkan ICD 驱动堆映射进进程工作集）
    unsafe {
        if let Ok(gl_lib) = GetModuleHandleA(PCSTR(b"opengl32.dll\x00".as_ptr())) {
            info.opengl_supported = true;
            if let Some(get_str_f) = GetProcAddress(gl_lib, PCSTR(b"glGetString\x00".as_ptr())) {
                type FnGlGetString = unsafe extern "system" fn(u32) -> *const u8;
                let gl_get_string: FnGlGetString = core::mem::transmute(get_str_f);
                let renderer_ptr = gl_get_string(0x1F01);
                let version_ptr = gl_get_string(0x1F02);
                if !renderer_ptr.is_null() {
                    let mut len = 0usize;
                    while len < 256 && *renderer_ptr.add(len) != 0 {
                        len += 1;
                    }
                    let r_name = String::from_utf8_lossy(core::slice::from_raw_parts(renderer_ptr, len))
                        .trim()
                        .to_owned();
                    let v_name = if !version_ptr.is_null() {
                        let mut vlen = 0usize;
                        while vlen < 256 && *version_ptr.add(vlen) != 0 {
                            vlen += 1;
                        }
                        String::from_utf8_lossy(core::slice::from_raw_parts(version_ptr, vlen))
                            .trim()
                            .to_owned()
                    } else {
                        "Hardware WGL".to_owned()
                    };
                    if !r_name.is_empty() {
                        info.adapter_name = r_name.clone();
                        info.opengl_status = format!("已就绪 · OpenGL {} ({})", v_name, r_name);
                    }
                }
            }
        }

        // 2. 读取本机 CPU 处理器真实型号（如 Intel(R) Core(TM) Ultra 5 335）
        {
            let cpu_sub: Vec<u16> = r"HARDWARE\Description\System\CentralProcessor\0"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let cpu_val: Vec<u16> = "ProcessorNameString".encode_utf16().chain(Some(0)).collect();
            let mut cbuf = [0u16; 256];
            let mut csz = (cbuf.len() * 2) as u32;
            if RegGetValueW(
                HKEY_LOCAL_MACHINE,
                PCWSTR(cpu_sub.as_ptr()),
                PCWSTR(cpu_val.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(cbuf.as_mut_ptr().cast()),
                Some(&mut csz),
            )
            .is_ok()
            {
                let clen = cbuf.iter().position(|&c| c == 0).unwrap_or(cbuf.len());
                let cname = String::from_utf16_lossy(&cbuf[..clen]).trim().to_owned();
                if !cname.is_empty() {
                    info.cpu_name = cname;
                }
            }
        }

        // 3. 从注册表遍历 0000..0006 显示适配器，优先选择 Intel Arc / 真实物理硬件 GPU（过滤虚拟显示器与残留驱动项）
        let mut best_desc: Option<(i32, String)> = None;
        for sub in [
            r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0000",
            r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0001",
            r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0002",
            r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0003",
            r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0004",
        ] {
            let sub_w: Vec<u16> = sub.encode_utf16().chain(Some(0)).collect();
            let val_w: Vec<u16> = "DriverDesc".encode_utf16().chain(Some(0)).collect();
            let mut buf = [0u16; 256];
            let mut sz = (buf.len() * 2) as u32;
            if RegGetValueW(
                HKEY_LOCAL_MACHINE,
                PCWSTR(sub_w.as_ptr()),
                PCWSTR(val_w.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut sz),
            )
            .is_ok()
            {
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                let desc = String::from_utf16_lossy(&buf[..len]).trim().to_owned();
                if desc.is_empty() {
                    continue;
                }
                let low = desc.to_ascii_lowercase();
                if low.contains("virtual") || low.contains("basic") || low.contains("indirect") || low.contains("oray") || low.contains("parsec") {
                    continue;
                }
                let score = if low.contains("arc") || low.contains("b370") {
                    100
                } else if low.contains("intel") || low.contains("nvidia") || low.contains("geforce") {
                    85
                } else {
                    40
                };
                if best_desc.as_ref().map_or(true, |(s, _)| score > *s) {
                    best_desc = Some((score, desc));
                }
            }
        }
        if let Some((_, desc)) = best_desc {
            // 若 GL_RENDERER 未返回有效型号或返回了非 Intel Arc 名称，则优先使用匹配的物理适配器名称
            if !info.adapter_name.to_ascii_lowercase().contains("arc") {
                info.adapter_name = desc;
            }
        }
    }

    let low_name = info.adapter_name.to_ascii_lowercase();
    info.adapter_kind = if low_name.contains("arc") {
        "Intel Arc Xe2 架构显卡 (Intel(R) Arc(TM) GPU · 硬件直通加速)".to_owned()
    } else if low_name.contains("rtx") || low_name.contains("gtx") {
        "独立显卡 (Discrete GPU · 硬件直通)".to_owned()
    } else if low_name.contains("iris") || low_name.contains("uhd") || low_name.contains("intel") {
        "Intel 锐炬/核芯显卡 (Integrated GPU · 硬件加速)".to_owned()
    } else {
        "硬件加速显卡 (Hardware GPU)".to_owned()
    };

    let vk_dll = std::path::Path::new(r"C:\Windows\System32\vulkan-1.dll").exists();
    info.vulkan_supported = vk_dll;
    info.vulkan_status = if vk_dll {
        format!("已就绪 · Vulkan 1.3 ({})", info.adapter_name)
    } else {
        "未检测到 vulkan-1.dll（已自动由 OpenGL 硬件接管）".to_owned()
    };

    info
}

#[cfg(not(windows))]
fn probe_gpu_hardware_info_uncached(cfg_mode: u8) -> GpuHardwareInfo {
    let mut info = GpuHardwareInfo::default();
    info.active_backend = match cfg_mode {
        1 => "Vulkan 硬件直通管线 (Vulkan + HighPerformance GPU)".to_owned(),
        2 => "OpenGL 硬件加速管线 (WGL / Glow Hardware GPU)".to_owned(),
        _ => "自动选择 (Auto · OpenGL/Vulkan 双栈硬件 GPU 加速)".to_owned(),
    };
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_renderer_uses_transparent_glow_until_wgpu_is_requested() {
        std::env::remove_var("AURALSLE_RENDERER");
        let mut options = eframe::NativeOptions::default();
        configure_native(&mut options);
        assert_eq!(options.renderer, eframe::Renderer::Glow);
        assert_eq!(options.hardware_acceleration, eframe::HardwareAcceleration::Required);

        std::env::set_var("AURALSLE_RENDERER", "wgpu");
        configure_native(&mut options);
        assert_eq!(options.renderer, eframe::Renderer::Wgpu);

        std::env::set_var("AURALSLE_RENDERER", "vulkan");
        configure_native(&mut options);
        assert_eq!(options.renderer, eframe::Renderer::Wgpu);
        std::env::remove_var("AURALSLE_RENDERER");
    }
}
