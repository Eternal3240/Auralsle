//! 截图工具检测。
//!
//! PixPin / Snipaste 这类工具在前台时会盖住整块屏幕，岛如果还浮在上面（而且还能被
//! 悬停展开），用户就看不到要截的那块内容。这里按「前台窗口属于哪个进程」判断，
//! 命中就把岛冻结：收起、不响应悬停，直到截图工具退出前台。
//!
//! 只认专业截图工具的可执行名 —— 微信 / QQ 的截图是主程序里的功能，按进程名判断
//! 会把整个聊天窗口误判成截图。

use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

/// 小写可执行名；命中即冻结。
const TOOLS: [&str; 6] = [
    "pixpin.exe",
    "snipaste.exe",
    "snippingtool.exe",
    "screensketch.exe",
    "faststonescreenrecorder.exe",
    "screentogif.exe",
];

#[cfg(windows)]
fn foreground_exe() -> Option<String> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    };
    unsafe {
        let window = GetForegroundWindow();
        if window.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut name = [0u16; 1024];
        let mut length = name.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(name.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        let path = String::from_utf16_lossy(&name[..length as usize]);
        let file = path.rsplit(['\\', '/']).next()?;
        Some(file.to_ascii_lowercase())
    }
}

#[cfg(not(windows))]
fn foreground_exe() -> Option<String> {
    None
}

static CACHE: OnceLock<Mutex<(bool, Instant)>> = OnceLock::new();

fn cache() -> &'static Mutex<(bool, Instant)> {
    CACHE.get_or_init(|| Mutex::new((false, Instant::now() - Duration::from_secs(10))))
}

/// 每帧调一次也行：内部 200ms 才真的去查一次前台进程。
pub fn tick() {
    let Ok(mut c) = cache().lock() else { return };
    if c.1.elapsed() < Duration::from_millis(200) {
        return;
    }
    c.1 = Instant::now();
    c.0 = foreground_exe()
        .map(|name| TOOLS.contains(&name.as_str()))
        .unwrap_or(false);
}

/// 截图工具是否在前台。
pub fn frozen() -> bool {
    cache().lock().map(|c| c.0).unwrap_or(false)
}

/// 正在运行的 AI 工具（额度跟随显示用）：进程名 → Provider。
/// 没识别到任何已知 AI 工具时返回空，调用方退回「显示全部额度」。
/// 结果缓存 1 秒：每帧扫进程表太浪费。
pub fn running_ai_providers() -> Vec<crate::quota::Provider> {
    static CACHE: OnceLock<Mutex<(Vec<crate::quota::Provider>, Instant)>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new((Vec::new(), Instant::now() - Duration::from_secs(10))));
    if let Ok(mut c) = cache.lock() {
        if c.1.elapsed() < Duration::from_secs(1) {
            return c.0.clone();
        }
        c.0 = scan_ai_providers();
        c.1 = Instant::now();
        return c.0.clone();
    }
    Vec::new()
}

fn scan_ai_providers() -> Vec<crate::quota::Provider> {
    use crate::quota::Provider;
    let mut out: Vec<Provider> = Vec::new();
    #[cfg(windows)]
    {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        unsafe {
            if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                let mut entry = PROCESSENTRY32W {
                    dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                    ..Default::default()
                };
                if Process32FirstW(snap, &mut entry).is_ok() {
                    loop {
                        let name = String::from_utf16_lossy(&entry.szExeFile);
                        let name = name.to_ascii_lowercase();
                        let provider = if name.contains("workbuddy") || name.contains("codebuddy") {
                            Some(Provider::WorkBuddy)
                        } else if name.contains("doubao") {
                            Some(Provider::Doubao)
                        } else if name.contains("deepseek") {
                            Some(Provider::DeepSeek)
                        } else if name.contains("claude") {
                            Some(Provider::Claude)
                        } else if name.contains("codex") || name.contains("chatgpt") {
                            Some(Provider::Codex)
                        } else if name.contains("kimi") {
                            Some(Provider::Kimi)
                        } else if name.contains("zcode") {
                            Some(Provider::ZCode)
                        } else if name.contains("kiro") {
                            Some(Provider::Kiro)
                        } else if name.contains("grok") || name.contains("cursor") {
                            Some(Provider::GrokBot)
                        } else if name.contains("antigravity") {
                            Some(Provider::Antigravity)
                        } else if name.contains("gemini") {
                            Some(Provider::Gemini)
                        } else {
                            None
                        };
                        if let Some(p) = provider {
                            if !out.contains(&p) {
                                out.push(p);
                            }
                        }
                        if !Process32NextW(snap, &mut entry).is_ok() {
                            break;
                        }
                    }
                }
                let _ = windows::Win32::Foundation::CloseHandle(snap);
            }
        }
    }
    out
}
