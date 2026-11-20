//! mod 的全部界面：主界面紧凑状态、AI 工作页、任务结束提示、设置页。
//!
//! 只使用上游 `paint::Card` 的公开绘制方法和 `extras` 的设置控件，
//! 不引用上游私有状态，因而上游重构时本文件基本不用改。

use crate::{
    app::Cmd,
    auralsle_ai_status::{
        self, config, state, GAP, ICON, ICON_CY, OUT_GAP, TEXT_BASE,
    },
    extras::{setting_row, settings_card, settings_label},
    paint::{self, Card, H},
    theme::{self, size, W},
};
use eframe::egui::{self, Color32};
use std::path::PathBuf;
use std::sync::Mutex;

/// 0..1 的平滑插值（去硬边），用于淡入淡出。
fn ramp(v: f32, lo: f32, hi: f32) -> f32 {
    let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 在裁剪框里画一行文字；超宽时两份首尾相接循环滚动（每秒 26px）。
///
/// `x/y/w/h` 是裁剪区，`base` 是文字基线。坐标都是 `Card` 的局部坐标。
/// 字间距宽度：逐字量宽再累加额外间距（spacing = 0 时退化为普通量宽）。
fn spaced_text_w(c: &Card, t: &str, size: f32, w: W, spacing: f32) -> f32 {
    if spacing.abs() < 0.01 {
        return c.text_w(t, size, w);
    }
    let chars: Vec<char> = t.chars().collect();
    if chars.is_empty() {
        return 0.0;
    }
    chars
        .iter()
        .map(|ch| c.text_w(&ch.to_string(), size, w))
        .sum::<f32>()
        + spacing * (chars.len() - 1) as f32
}

/// 逐字绘制并附加字间距。spacing = 0 时直接走 `Card::text`。
fn spaced_text(
    c: &Card,
    x: f32,
    base: f32,
    t: &str,
    size: f32,
    w: W,
    color: Color32,
    h: H,
    spacing: f32,
) -> f32 {
    let total = spaced_text_w(c, t, size, w, spacing);
    if spacing.abs() < 0.01 {
        c.text(x, base, t, size, w, color, h);
        return total;
    }
    let mut cx = match h {
        H::Left => x,
        H::Center => x - total / 2.0,
        H::Right => x - total,
    };
    for ch in t.chars() {
        let s = ch.to_string();
        cx += c.text(cx, base, &s, size, w, color, H::Left);
        cx += spacing;
    }
    total
}

fn marquee(
    c: &Card,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    base: f32,
    text: &str,
    size: f32,
    weight: W,
    color: Color32,
    spacing: f32,
) {
    if text.is_empty() || w <= 0.0 {
        return;
    }
    let tw = spaced_text_w(c, text, size, weight, spacing);
    let clip = c.clipped(x, y, w, h);
    if tw <= w {
        spaced_text(&clip, x, base, text, size, weight, color, H::Left, spacing);
        return;
    }
    let clock = c.ui.input(|i| i.time) as f32;
    let cycle = tw + 36.0;
    let offset = (clock * 26.0) % cycle;
    for k in 0..2 {
        spaced_text(
            &clip,
            x - offset + k as f32 * cycle,
            base,
            text,
            size,
            weight,
            color,
            H::Left,
            spacing,
        );
    }
    c.ui.ctx().request_repaint();
}

fn phase_color(p: state::Phase) -> Color32 {
    match p {
        state::Phase::Working => theme::BLUE,
        state::Phase::Tool => theme::CYAN,
        state::Phase::Waiting => theme::YELLOW,
        state::Phase::Error => theme::RED,
        state::Phase::Running => theme::SOFT_GREEN,
        state::Phase::Done => theme::GREEN,
    }
}

/// 打开 AI 工具：像微信的未读消息图标一样，**优先回到本机应用**。
///
/// 顺序：设置里手填的目标 → 已在运行的应用（切到前台，最小化则还原）
/// → 本机已安装的应用（启动）→ 官方网页。
/// 以前直接开网页，点「回到 AI」跳到的是浏览器而不是桌面客户端。
fn expand_env_vars(raw: &str) -> String {
    let mut s = raw.trim().trim_matches(['"', '\'', ' ']).to_owned();
    for var in ["LOCALAPPDATA", "APPDATA", "USERPROFILE", "PROGRAMFILES", "PROGRAMFILES(X86)"] {
        let token = format!("%{var}%");
        if s.to_ascii_uppercase().contains(&token) {
            if let Ok(val) = std::env::var(var) {
                let idx_opt = s.to_ascii_uppercase().find(&token);
                if let Some(idx) = idx_opt {
                    s.replace_range(idx..idx + token.len(), &val);
                }
            }
        }
    }
    s
}

/// 直接按指定的 `key` 唤醒或启动对应的 AI 工具（用于点击具体 AI 工具芯片或条目时，不被全局 `open_target` 覆盖）。
fn launch_specific_agent(ctx: &egui::Context, key: &str) {
    if let Some(exe) = activate_app(key) {
        if !exe.is_empty() {
            remember_app(key, &exe);
        }
        return;
    }
    if spawn_app(key) {
        return;
    }
    let url = config::default_open_url(key);
    if url.starts_with("http://") || url.starts_with("https://") {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    } else {
        let _ = std::process::Command::new("explorer").arg(url).spawn();
    }
}

fn launch(ctx: &egui::Context, key: &str) {
    let raw = config::get().open_target;
    let t = expand_env_vars(&raw);
    if !t.is_empty() {
        if t.starts_with("http://") || t.starts_with("https://") {
            ctx.open_url(egui::OpenUrl::new_tab(t));
        } else {
            let stem = t
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or("")
                .trim_end_matches(".exe")
                .trim_end_matches(".lnk")
                .to_ascii_lowercase();
            if !stem.is_empty() && !t.contains("://") {
                if let Some(exe) = activate_app(&stem) {
                    if !exe.is_empty() {
                        remember_app(key, &exe);
                    }
                    return;
                }
            }
            let _ = std::process::Command::new("explorer").arg(&t).spawn();
        }
        return;
    }
    launch_specific_agent(ctx, key);
}

// ---------------------------------------------------------------
// 回到本机应用（微信式）
// ---------------------------------------------------------------

/// 各 AI 客户端的进程名 / 快捷方式关键词（小写）。
fn app_keywords(key: &str) -> &'static [&'static str] {
    let k = key.to_ascii_lowercase();
    if k.contains("doubao") || key.contains("豆包") {
        &["doubao", "豆包"]
    } else if k.contains("workbuddy") || k.contains("codebuddy") {
        &["workbuddy", "codebuddy", "企微研发助手"]
    } else if k.contains("claude") || k.contains("anthropic") {
        &["claude"]
    } else if k.contains("codex") || k.contains("openai") || k.contains("chatgpt") {
        &["codex", "chatgpt"]
    } else if k.contains("deepseek") || k == "dsh" {
        &["deepseek", "深度求索"]
    } else if k.contains("antigravity") {
        &["antigravity"]
    } else if k.contains("gemini") {
        &["gemini", "ai studio"]
    } else if k.contains("kimi") {
        &["kimi"]
    } else if k.contains("grok") {
        &["grok"]
    } else if k.contains("zcode") || k.contains("glm") {
        &["zcode"]
    } else if k.contains("cursor") {
        &["cursor"]
    } else if k.contains("trae") {
        &["trae"]
    } else if k.contains("muse") {
        &["muse", "musework"]
    } else if k == "dot" || k.starts_with("dot-") || k.contains("dots") {
        &["dot", "dots"]
    } else if k.contains("windsurf") {
        &["windsurf"]
    } else if k.contains("kiro") {
        &["kiro"]
    } else {
        &[]
    }
}

/// 把已在运行的 AI 客户端切到前台。返回它的 exe 路径（记下来，下次没开着也能启动）。
#[cfg(windows)]
fn activate_app(key: &str) -> Option<String> {
    let kw = app_keywords(key);
    if kw.is_empty() {
        return None;
    }
    app_win::activate(kw)
}

/// 启动本机安装的 AI 客户端：先试上次激活时记下的路径，再找常见安装位置与开始菜单。
#[cfg(windows)]
fn spawn_app(key: &str) -> bool {
    let saved = config::get().app_path(key).map(|p| p.to_owned());
    if let Some(path) = saved {
        let exe = PathBuf::from(path);
        if exe.is_file() {
            let _ = std::process::Command::new("explorer").arg(&exe).spawn();
            return true;
        }
    }
    let kw = app_keywords(key);
    if kw.is_empty() {
        return false;
    }
    app_win::launch_installed(kw)
}

#[cfg(not(windows))]
fn activate_app(_key: &str) -> Option<String> {
    None
}

#[cfg(not(windows))]
fn spawn_app(_key: &str) -> bool {
    false
}

/// 记住这次激活成功的程序路径：应用关掉后再点图标能直接把它启动起来。
fn remember_app(key: &str, exe: &str) {
    if config::get().app_path(key) == Some(exe) {
        return; // 没变化就不写盘
    }
    config::update(|c| c.set_app_path(key, exe));
}

/// 本机应用的窗口发现与启动。用上游同一套 `windows` crate（微信窗口激活也是这么做的），
/// 不额外链接 user32，避免和上游的 extern 声明打架。
#[cfg(windows)]
mod app_win {
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};
    use windows::core::PWSTR;
    use windows::Win32::{
        Foundation::{BOOL, CloseHandle, HWND, LPARAM},
        System::Threading::{
            AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
            PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
            SetForegroundWindow, ShowWindowAsync, SW_RESTORE, SW_SHOW,
        },
    };

    thread_local! {
        static WANT: RefCell<Vec<String>> = RefCell::new(Vec::new());
        static HIT: Cell<isize> = Cell::new(0);
        static HIT_PATH: RefCell<String> = RefCell::new(String::new());
    }

    /// 窗口所属进程的完整 exe 路径。
    unsafe fn process_image(hwnd: HWND) -> Option<String> {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
        if pid == 0 {
            return None;
        }
        // PROCESS_QUERY_LIMITED_INFORMATION：权限最低，够读映像路径。
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, BOOL(0), pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let res = QueryFullProcessImageNameW(
            proc,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(proc);
        res.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        if path.is_empty() {
            None
        } else {
            Some(path)
        }
    }

    unsafe extern "system" fn probe(hwnd: HWND, lp: LPARAM) -> BOOL {
        use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        let Some(path) = process_image(hwnd) else {
            return true.into();
        };
        let file = path
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let stem = match file.strip_suffix(".exe") {
            Some(s) => s.to_owned(),
            None => file,
        };
        if stem.contains("auralsle") || stem.contains("auraisle") || stem == "explorer" {
            return true.into();
        }
        if lp.0 == 0 {
            // 第一遍：精确匹配独立桌面客户端进程名 (.exe)
            let hit = WANT.with(|w| w.borrow().iter().any(|kw| stem.contains(kw.as_str())));
            if hit {
                HIT.with(|c| c.set(hwnd.0 as isize));
                HIT_PATH.with(|p| *p.borrow_mut() = path);
                return false.into();
            }
        } else {
            // 第二遍：匹配终端（Windows Terminal / PowerShell / VS Code）或浏览器/PWA 独立窗口标题
            let mut tbuf = [0u16; 256];
            let tlen = GetWindowTextW(hwnd, &mut tbuf);
            if tlen > 0 {
                let title = String::from_utf16_lossy(&tbuf[..tlen as usize]);
                if !title.contains("岛仔") && !title.contains("灵屿") && !title.to_ascii_lowercase().contains("auraisle") {
                    let t_low = title.to_ascii_lowercase();
                    let hit = WANT.with(|w| w.borrow().iter().any(|kw| t_low.contains(kw.as_str())));
                    if hit {
                        HIT.with(|c| c.set(hwnd.0 as isize));
                        // 仅当是独立客户端而非浏览器/终端壳时记录 exe 路径
                        let is_shell = ["chrome", "msedge", "firefox", "windowsterminal", "powershell", "pwsh", "cmd", "code"]
                            .iter()
                            .any(|s| stem.contains(s));
                        HIT_PATH.with(|p| *p.borrow_mut() = if is_shell { String::new() } else { path });
                        return false.into();
                    }
                }
            }
        }
        true.into()
    }

    /// 找窗口 → 还原 → 前台。返回目标 exe 路径（若为浏览器/终端宿主则返回空串）。
    pub fn activate(keywords: &[&str]) -> Option<String> {
        WANT.with(|w| {
            let mut w = w.borrow_mut();
            w.clear();
            w.extend(keywords.iter().map(|s| s.to_string()));
        });
        HIT.with(|c| c.set(0));
        HIT_PATH.with(|p| p.borrow_mut().clear());
        let _ = unsafe { EnumWindows(Some(probe), LPARAM(0)) };
        if HIT.with(|c| c.get()) == 0 {
            let _ = unsafe { EnumWindows(Some(probe), LPARAM(1)) };
        }
        let h = HIT.with(|c| c.get());
        if h == 0 {
            return None;
        }
        focus(HWND(h as *mut _));
        Some(HIT_PATH.with(|p| p.borrow().clone()))
    }

    fn focus(hwnd: HWND) {
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindowAsync(hwnd, SW_RESTORE);
            } else if !IsWindowVisible(hwnd).as_bool() {
                let _ = ShowWindowAsync(hwnd, SW_SHOW);
            }
            if SetForegroundWindow(hwnd).as_bool() {
                return;
            }
            // 前台锁：把自己的线程挂到当前前台窗口的输入队列上，再抢一次。
            let fg = GetForegroundWindow();
            let mut pid = 0u32;
            let fg_tid = if fg.0.is_null() {
                0
            } else {
                GetWindowThreadProcessId(fg, Some(&mut pid as *mut u32))
            };
            let me = GetCurrentThreadId();
            if fg_tid != 0 && fg_tid != me {
                let _ = AttachThreadInput(me, fg_tid, BOOL(1));
                let _ = SetForegroundWindow(hwnd);
                let _ = AttachThreadInput(me, fg_tid, BOOL(0));
            }
        }
    }

    fn open_path(p: &Path) {
        // 和双击等价：交给资源管理器（exe 直接跑，lnk 走它自己的启动参数）。
        let _ = std::process::Command::new("explorer").arg(p).spawn();
    }

    /// 常见安装位置：`%LOCALAPPDATA%\Programs\<Name>\<Name>.exe` 等。
    fn installed_exe(keywords: &[&str]) -> Option<PathBuf> {
        let mut bases: Vec<PathBuf> = Vec::new();
        for var in ["LOCALAPPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)", "APPDATA"] {
            if let Ok(v) = std::env::var(var) {
                let b = PathBuf::from(v);
                bases.push(b.join("Programs"));
                bases.push(b.clone());
            }
        }
        for kw in keywords {
            let file = format!("{kw}.exe");
            for b in &bases {
                for c in [
                    b.join(kw).join(&file),
                    b.join(kw).join(kw).join(&file),
                    b.join(&file),
                ] {
                    if c.is_file() {
                        return Some(c);
                    }
                }
            }
        }
        None
    }

    fn start_menus() -> Vec<PathBuf> {
        let mut v = Vec::new();
        if let Ok(a) = std::env::var("APPDATA") {
            v.push(PathBuf::from(a).join("Microsoft/Windows/Start Menu/Programs"));
        }
        if let Ok(p) = std::env::var("PROGRAMDATA") {
            v.push(PathBuf::from(p).join("Microsoft/Windows/Start Menu/Programs"));
        }
        v
    }

    /// 开始菜单里按名字找快捷方式（最多四层，几十个文件，只在没找到进程时才扫）。
    fn find_lnk(dir: &Path, kw: &str, depth: usize) -> Option<PathBuf> {
        if depth > 4 {
            return None;
        }
        let rd = std::fs::read_dir(dir).ok()?;
        let mut dirs = Vec::new();
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
                continue;
            }
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".lnk") && name.contains(kw) {
                return Some(p);
            }
        }
        dirs.iter().find_map(|d| find_lnk(d, kw, depth + 1))
    }

    pub fn launch_installed(keywords: &[&str]) -> bool {
        if let Some(exe) = installed_exe(keywords) {
            open_path(&exe);
            return true;
        }
        for kw in keywords {
            for menu in start_menus() {
                if let Some(lnk) = find_lnk(&menu, kw, 0) {
                    open_path(&lnk);
                    return true;
                }
            }
        }
        false
    }
}

// ---------------------------------------------------------------
// AI 工具图标
// ---------------------------------------------------------------

/// 各 AI 客户端的品牌图标，随 mod 一起分发，不依赖上游素材版本。
fn icon_bytes(key: &str) -> Option<&'static [u8]> {
    let k = key.to_ascii_lowercase();
    let bytes: &'static [u8] = if k.contains("doubao") || key.contains("豆包") {
        include_bytes!("assets/ai-icons/doubao.svg")
    } else if k.contains("workbuddy") || k.contains("codebuddy") {
        include_bytes!("assets/ai-icons/workbuddy.svg")
    } else if k.contains("claude") || k.contains("anthropic") {
        include_bytes!("assets/ai-icons/claude.svg")
    } else if k.contains("codex") || k.contains("openai") || k.contains("chatgpt") {
        include_bytes!("assets/ai-icons/codex.svg")
    } else if k.contains("deepseek") || k == "dsh" {
        include_bytes!("assets/ai-icons/deepseek.svg")
    } else if k.contains("gemini") || k.contains("antigravity") {
        include_bytes!("assets/ai-icons/gemini.svg")
    } else if k.contains("kimi") {
        include_bytes!("assets/ai-icons/kimi.svg")
    } else if k.contains("grok") || k.contains("cursor") || k.contains("windsurf") {
        include_bytes!("assets/ai-icons/grokbot.svg")
    } else if k.contains("zcode") || k.contains("glm") {
        include_bytes!("assets/ai-icons/zcode.svg")
    } else if k.contains("kiro") {
        include_bytes!("../../assets/ai-icons/kiro.svg")
    } else if k.contains("minimax") {
        include_bytes!("../../assets/ai-icons/minimax.svg")
    } else if k.contains("opencode") {
        include_bytes!("../../assets/ai-icons/opencode.svg")
    } else {
        return None;
    };
    Some(bytes)
}

fn icon_texture(ctx: &egui::Context, key: &str, px: u32) -> Option<egui::TextureHandle> {
    let id = egui::Id::new(("ai-status-mod-icon-v42", key, px));
    if let Some(t) = ctx.data_mut(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    let k = key.to_ascii_lowercase();
    let render_px = px.max(32);
    let mut image = if k.contains("doubao") || key.contains("豆包") {
        if let Ok(dyn_img) = image::load_from_memory(include_bytes!("../../assets/ai-icons/doubao.png")) {
            let rgba = dyn_img
                .resize_exact(render_px, render_px, image::imageops::FilterType::Lanczos3)
                .to_rgba8();
            egui::ColorImage::from_rgba_unmultiplied([render_px as usize, render_px as usize], rgba.as_raw())
        } else {
            let bytes = icon_bytes(key)?;
            egui_extras::image::load_svg_bytes_with_size(
                bytes,
                Some(egui::load::SizeHint::Size(render_px, render_px)),
            )
            .ok()?
        }
    } else {
        let bytes = icon_bytes(key)?;
        egui_extras::image::load_svg_bytes_with_size(
            bytes,
            Some(egui::load::SizeHint::Size(render_px, render_px)),
        )
        .ok()?
    };
    // 为单色 SVG 注入鲜明品牌色（特别是 deepseek.svg 的 currentColor 原本为全黑）
    if k.contains("deepseek") || k == "dsh" {
        for p in &mut image.pixels {
            if p.a() > 0 {
                *p = Color32::from_rgba_unmultiplied(77, 107, 254, p.a());
            }
        }
    } else if k.contains("claude") || k.contains("anthropic") {
        for p in &mut image.pixels {
            if p.a() > 0 {
                *p = Color32::from_rgba_unmultiplied(217, 119, 87, p.a());
            }
        }
    } else if k.contains("gemini") || k.contains("antigravity") {
        for p in &mut image.pixels {
            if p.a() > 0 {
                *p = Color32::from_rgba_unmultiplied(78, 149, 255, p.a());
            }
        }
    } else if k.contains("codex") || k.contains("openai") || k.contains("chatgpt") {
        for p in &mut image.pixels {
            if p.a() > 0 {
                *p = Color32::from_rgba_unmultiplied(16, 163, 127, p.a());
            }
        }
    } else if k.contains("kimi") {
        for p in &mut image.pixels {
            if p.a() > 0 {
                *p = Color32::from_rgba_unmultiplied(56, 189, 248, p.a());
            }
        }
    }
    let texture = ctx.load_texture(
        format!("ai-status-mod-{key}-{render_px}"),
        image,
        egui::TextureOptions::LINEAR,
    );
    crate::skin_paint::mark_colored(texture.id());
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

/// 画 AI 工具自己的图标；没有对应品牌图时退回「AI」圆环。
fn draw_icon(c: &Card, key: &str, cx: f32, cy: f32, side: f32, color: Color32) {
    let ctx = c.ui.ctx();
    let ppp = ctx.pixels_per_point();
    let px = (side * c.s * ppp).round().max(1.0) as u32;
    if let Some(texture) = icon_texture(ctx, key, px) {
        let half = px as f32 / 2.0;
        let center = c.pos(cx, cy);
        let min = egui::pos2(
            (center.x * ppp - half).round() / ppp,
            (center.y * ppp - half).round() / ppp,
        );
        let rect = egui::Rect::from_min_size(min, egui::Vec2::splat(px as f32 / ppp));
        c.painter.image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        c.ring(cx, cy, side / 2.0, 1.7, color);
        c.text(cx, cy + 2.2, "AI", side * 0.44, W::Bold, theme::WHITE, H::Center);
    }
}

// ---------------------------------------------------------------
// 主界面：时间右侧的紧凑状态
// ---------------------------------------------------------------

/// 与药丸时钟基线一致、字号小两号；左边是 AI 工具自己的图标。
/// 「回到 AI」不画在这里 —— 鼠标移到岛上就会展开，岛内的按钮根本点不到。
/// `left` 是整块的左端（`compact_x` 给出），`alpha` 与收起动画同步。
/// 仅绘制主岛同款 AI 客户端矢量/品牌图标 + 右上角呼吸状态灯（用于收起态歌词岛，与主岛图标完全一致）。
pub fn draw_compact_icon_only(c: &Card, cx: f32, cy: f32, alpha: f32) {
    let cfg = config::get();
    if !cfg.enabled {
        return;
    }
    let Some(s) = auralsle_ai_status::shown() else {
        return;
    };
    if alpha <= 0.004 {
        return;
    }
    let c = c.faded(alpha.clamp(0.0, 1.0));
    let color = phase_color(s.agent.phase);
    let icon_side = 19.5_f32;
    draw_icon(&c, &s.agent.key, cx, cy, icon_side, color);
    if !cfg!(test) && cfg.agent_status_lamp {
        let t_sec = c.ui.input(|i| i.time) as f32;
        let pulse = match s.agent.phase {
            state::Phase::Waiting => ((t_sec * 8.0).sin() * 0.5 + 0.5),
            state::Phase::Working | state::Phase::Tool | state::Phase::Running => ((t_sec * 3.8).sin() * 0.5 + 0.5),
            _ => 0.85,
        };
        let lamp_x = cx + ICON * 0.5 - 1.0;
        let lamp_y = cy - 6.2;
        c.circle(lamp_x, lamp_y, 3.8 + pulse * 1.4, color.gamma_multiply(0.32 * pulse));
        c.circle(lamp_x, lamp_y, 2.3, color);
    }
}

pub fn draw_compact(c: &Card, left: f32, alpha: f32) {
    let cfg = config::get();
    if !cfg.enabled || !cfg.compact {
        return;
    }
    let Some(s) = auralsle_ai_status::shown() else {
        return;
    };
    if alpha <= 0.004 {
        return;
    }
    let c = c.faded(alpha.clamp(0.0, 1.0));
    let color = phase_color(s.agent.phase);
    // 写入方还写着 working 就照实说「工作中」；静默只在展开页用颜色提示，
    // 绝不因为超时改口说「已完成」。
    let label = s.agent.phase.label();
    // 图标放大到与 13.5 号粗体状态文字等视觉高度；`ICON` 常量与布局宽度不动，
    // 只改实际贴图边长，文字起点仍是 `left + ICON + ICON_TEXT_GAP`。
    let icon_side = auralsle_ai_status::COMPACT_ICON;
    draw_icon(&c, &s.agent.key, left + ICON / 2.0, ICON_CY, icon_side, color);
    // [Fly-Island AgentAI 状态灯]：根据生命周期阶段在图标右上角渲染呼吸/闪烁状态灯（工作中呼吸蓝、待审批快闪琥珀金、完成翠绿、出错红）
    if !cfg!(test) && cfg.agent_status_lamp {
        let t_sec = c.ui.input(|i| i.time) as f32;
        let pulse = match s.agent.phase {
            state::Phase::Waiting => ((t_sec * 8.0).sin() * 0.5 + 0.5),
            state::Phase::Working | state::Phase::Tool | state::Phase::Running => ((t_sec * 3.8).sin() * 0.5 + 0.5),
            _ => 0.85,
        };
        let lamp_x = left + ICON - 1.0;
        let lamp_y = ICON_CY - 6.2;
        c.circle(lamp_x, lamp_y, 3.8 + pulse * 1.4, color.gamma_multiply(0.32 * pulse));
        c.circle(lamp_x, lamp_y, 2.3, color);
    }
    c.text(
        left + ICON + auralsle_ai_status::ICON_TEXT_GAP,
        TEXT_BASE,
        label,
        auralsle_ai_status::label_size(),
        W::Bold,
        theme::WHITE,
        H::Left,
    );
    let width = auralsle_ai_status::compact_width(label);
    // 课程提示图标（要上课的提醒窗口内才出现）：展开了就从这里滑出去。
    let cw = auralsle_ai_status::course_slot_width();
    if cw > 0.0 {
        let ix = left + width + GAP + ICON / 2.0;
        c.fill(ix - ICON / 2.0, ICON_CY - ICON / 2.0, ICON, ICON, ICON / 2.0, Color32::from_rgb(255, 190, 92));
        c.text(ix, ICON_CY + 3.2, "课", 8.5, W::Bold, Color32::from_rgb(30, 24, 12), H::Center);
        auralsle_ai_status::record_course_icon(Some(c.rect(ix - ICON / 2.0, ICON_CY - ICON / 2.0, ICON, ICON)));
    } else {
        auralsle_ai_status::record_course_icon(None);
    }
    let hint = format!(
        "{}\n状态：{}\n最近更新：{} 秒前",
        s.agent.name,
        label,
        s.agent.age_secs
    );
    let _ = hint;
    c.hit(left, 3.0, width, 24.0, "ai-status-compact");
}

/// 岛外课程卡：只绘制课程模块。
/// 卡片矩形由 `draw_course_module` 自行登记，供窗口可见区域取用。
/// 设备药丸与课程岛解耦，由 `App::frame` 独立布局 / 绘制 / 登记。
pub fn draw_course_outer(
    c: &Card,
    cx: f32,
    island_w: f32,
    progress: f32,
    side: bool,
) {
    let _ = draw_course_module(c, cx, island_w, progress, side);
}

// ---------------------------------------------------------------
// 课程卡：开放模板（文字卡片）+ 经典样式
// ---------------------------------------------------------------

/// `#RRGGBB` → 颜色；写错就回退，不让一张卡片因为一个手滑的十六进制整块消失。
pub fn hex_color(s: &str, fallback: Color32) -> Color32 {
    let s = s.trim().trim_start_matches('#');
    if s.len() == 8 {
        if let (Ok(r), Ok(g), Ok(b), Ok(a)) = (
            u8::from_str_radix(&s[0..2], 16),
            u8::from_str_radix(&s[2..4], 16),
            u8::from_str_radix(&s[4..6], 16),
            u8::from_str_radix(&s[6..8], 16),
        ) {
            return Color32::from_rgba_unmultiplied(r, g, b, a);
        }
    } else if s.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&s[0..2], 16),
            u8::from_str_radix(&s[2..4], 16),
            u8::from_str_radix(&s[4..6], 16),
        ) {
            return Color32::from_rgb(r, g, b);
        }
    }
    fallback
}

#[derive(Clone, Copy, PartialEq)]
enum PieceKind {
    Text,
    Button,
    Progress,
}

#[derive(Clone)]
struct Piece {
    text: String,
    size: f32,
    color: Color32,
    kind: PieceKind,
}

struct LayoutLine {
    left: Vec<Piece>,
    right: Vec<Piece>,
    h: f32,
    dash: bool,
}

struct CourseLooks {
    time: Color32,
    title: Color32,
    place: Color32,
    accent: Color32,
    title_size: f32,
    place_size: f32,
    time_size: f32,
    small: f32,
}

fn course_looks(cfg: &config::Config) -> CourseLooks {
    CourseLooks {
        time: hex_color(&cfg.course_time_color, theme::WHITE),
        title: hex_color(&cfg.course_title_color, theme::WHITE),
        place: hex_color(&cfg.course_place_color, Color32::from_rgb(255, 190, 92)),
        accent: hex_color(&cfg.course_accent_color, Color32::from_rgb(87, 182, 255)),
        title_size: cfg.course_title_size,
        place_size: cfg.course_place_size,
        time_size: cfg.course_time_size,
        small: cfg.course_small_size,
    }
}

fn piece_width(c: &Card, p: &Piece) -> f32 {
    match p.kind {
        PieceKind::Text => c.text_w(&p.text, p.size, W::Semibold) + 4.0,
        PieceKind::Button => c.text_w(&p.text, 9.5, W::Semibold) + 22.0,
        PieceKind::Progress => 120.0,
    }
}

/// 把一截模板切成占位符 + 字面文本。`{time}` / `{title}` / … 之外的内容原样保留，
/// 所以用户可以在模板里直接写中文标签、空格、破折号。
fn parse_pieces(s: &str, a: &auralsle_ai_status::course::Alert, l: &CourseLooks, cfg: &config::Config) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut literal = String::new();
    let mut chars = s.chars().peekable();
    let push_literal = |out: &mut Vec<Piece>, literal: &mut String, l: &CourseLooks| {
        if !literal.is_empty() {
            out.push(Piece {
                text: std::mem::take(literal),
                size: l.small,
                color: theme::GRAY,
                kind: PieceKind::Text,
            });
        }
    };
    while let Some(ch) = chars.next() {
        if ch != '{' {
            literal.push(ch);
            continue;
        }
        let mut key = String::new();
        while let Some(&n) = chars.peek() {
            chars.next();
            if n == '}' {
                break;
            }
            key.push(n);
        }
        push_literal(&mut out, &mut literal, l);
        let (text, size, color, kind) = match key.as_str() {
            "time" => (a.time_range.clone(), l.time_size, l.time, PieceKind::Text),
            "place" => (a.place.clone(), l.place_size, l.place, PieceKind::Text),
            "title" => (a.title.clone(), l.title_size, l.title, PieceKind::Text),
            "teacher" => (a.teacher.clone(), l.small, theme::GRAY, PieceKind::Text),
            "info" => (a.info.clone(), l.small, l.place, PieceKind::Text),
            "date" => (a.date.clone(), l.small, theme::BLUE, PieceKind::Text),
            "status" => (a.status.to_owned(), l.small, theme::BLUE, PieceKind::Text),
            "end" => (format!("{} 结束", a.end), l.small, theme::GRAY, PieceKind::Text),
            "countdown" => (a.countdown.clone(), l.small + 2.0, l.accent, PieceKind::Text),
            "dnd" => {
                let auto = match cfg.course_dnd_action {
                    0 => "开始专注",
                    1 if auralsle_ai_status::dnd_active(chrono::Local::now().timestamp()) => "免打扰中",
                    1 => "上课免打扰",
                    _ => "系统免打扰",
                };
                let label = if cfg.course_dnd_label.trim().is_empty() {
                    auto
                } else {
                    cfg.course_dnd_label.trim()
                };
                (label.to_owned(), 9.5, theme::WHITE, PieceKind::Button)
            }
            "progress" => (String::new(), 0.0, l.accent, PieceKind::Progress),
            _ => (String::new(), 0.0, theme::GRAY, PieceKind::Text),
        };
        if kind == PieceKind::Text && text.is_empty() {
            continue; // 空字段（比如没填教师）直接不占位
        }
        out.push(Piece { text, size, color, kind });
    }
    push_literal(&mut out, &mut literal, l);
    out
}

/// 模板 → 行布局；用户删占位符就是隐藏字段，换行/换顺序就是重排。
fn parse_layout(
    a: &auralsle_ai_status::course::Alert,
    cfg: &config::Config,
    l: &CourseLooks,
) -> Vec<LayoutLine> {
    let mut lines = Vec::new();
    for raw in cfg.course_layout.lines() {
        let s = raw.trim();
        if s.is_empty() {
            lines.push(LayoutLine { left: vec![], right: vec![], h: 6.0, dash: false });
        } else if s == "---" {
            if cfg.course_show_sep {
                lines.push(LayoutLine { left: vec![], right: vec![], h: 12.0, dash: true });
            }
        } else {
            let (left_s, right_s) = s.split_once('|').unwrap_or((s, ""));
            let left = parse_pieces(left_s, a, l, cfg);
            let right = parse_pieces(right_s, a, l, cfg);
            let max_size = left
                .iter()
                .chain(right.iter())
                .map(|p| match p.kind {
                    PieceKind::Text => p.size,
                    PieceKind::Button => 20.0,
                    PieceKind::Progress => 8.0,
                })
                .fold(0.0f32, f32::max);
            let h = if left.iter().chain(right.iter()).any(|p| p.kind == PieceKind::Button) {
                26.0
            } else {
                (max_size * 1.6 + 7.0).max(18.0)
            };
            lines.push(LayoutLine { left, right, h, dash: false });
        }
    }
    if lines.is_empty() {
        lines.push(LayoutLine {
            left: parse_pieces("{title}", a, l, cfg),
            right: vec![],
            h: 28.0,
            dash: false,
        });
    }
    lines
}

fn text_card_height(a: &auralsle_ai_status::course::Alert, cfg: &config::Config, l: &CourseLooks) -> f32 {
    let body: f32 = parse_layout(a, cfg, l).iter().map(|ln| ln.h).sum();
    (14.0 + body + 12.0).max(118.0)
}

/// 点击「上课免打扰」：三选一，配置决定动作。
fn course_dnd_click(a: &auralsle_ai_status::course::Alert, cfg: &config::Config) {
    match cfg.course_dnd_action {
        0 => auralsle_ai_status::request_focus(),
        1 => auralsle_ai_status::set_dnd_until(a.end_ts),
        _ => auralsle_ai_status::open_notification_settings(),
    }
}

/// 样式 1：灵动模板卡片（修复需求 8：垂直居中均衡排版 + 顶栏时间/教室自动左右分列 + 课程名大字舞台 + 底部进度轨与操作栏，彻底消除行距挤迫与文字撞车！）
fn draw_course_text(
    c: &Card,
    bx: f32,
    by: f32,
    w: f32,
    h: f32,
    a: &auralsle_ai_status::course::Alert,
    cfg: &config::Config,
) -> egui::Rect {
    let l = course_looks(cfg);
    let lines = parse_layout(a, cfg, &l);
    let total_lines_h: f32 = lines.iter().map(|ln| ln.h).sum();
    let avail_h = (h - 24.0).max(80.0);
    // 自动在 140px 高度内垂直均衡分布各行，并在底部预留课堂进度细轨
    let extra_gap = if lines.len() > 1 && total_lines_h < avail_h - 10.0 {
        ((avail_h - 10.0 - total_lines_h) / (lines.len() as f32 - 1.0)).clamp(0.0, 10.0)
    } else {
        0.0
    };
    let mut y = by + ((h - 8.0 - (total_lines_h + extra_gap * (lines.len().saturating_sub(1) as f32))) * 0.5).clamp(10.0, 18.0);

    for line in &lines {
        let cy = y + line.h / 2.0;
        if line.dash {
            let mut x = bx + 14.0;
            let end = bx + w - 14.0;
            while x < end {
                c.seg((x, cy), ((x + 3.0).min(end), cy), 1.0, Color32::from_white_alpha(45));
                x += 6.5;
            }
        } else {
            let mut x = bx + 14.0;
            let right_total: f32 = line.right.iter().map(|p| piece_width(c, p)).sum();
            let left_max_w = if right_total > 0.0 {
                (w - 28.0 - right_total - 8.0).max(60.0)
            } else {
                w - 28.0
            };
            let left_end = bx + 14.0 + left_max_w;
            for (pi, p) in line.left.iter().enumerate() {
                let tail_need: f32 = line.left[pi + 1..]
                    .iter()
                    .map(|tp| piece_width(c, tp) + 6.0)
                    .sum();
                let avail_here = (left_end - x).max(0.0);
                if avail_here < 10.0 {
                    break;
                }
                let rem_w = if tail_need > 0.0 {
                    (avail_here - tail_need.min(avail_here * 0.45)).max(44.0).min(avail_here)
                } else {
                    avail_here
                };
                draw_piece(c, p, x, cy, rem_w, a, cfg);
                x += piece_width(c, p).min(rem_w) + if pi + 1 < line.left.len() { 6.0 } else { 0.0 };
            }
            let mut rx = (bx + w - 14.0 - right_total).max(x + 6.0);
            for p in &line.right {
                let rem_w = (bx + w - 14.0 - rx).max(0.0);
                if rem_w < 10.0 {
                    break;
                }
                draw_piece(c, p, rx, cy, rem_w, a, cfg);
                rx += piece_width(c, p).min(rem_w);
            }
        }
        y += line.h + extra_gap;
    }
    // 底部贯穿式课堂进度细轨
    let bar_y = by + h - 8.0;
    c.fill(bx + 14.0, bar_y, w - 28.0, 2.4, 1.2, Color32::from_white_alpha(20));
    c.fill(
        bx + 14.0,
        bar_y,
        (w - 28.0) * a.progress.clamp(0.04, 1.0),
        2.4,
        1.2,
        l.accent,
    );
    c.rect(bx, by, w, h)
}

fn draw_piece(
    c: &Card,
    p: &Piece,
    x: f32,
    cy: f32,
    max_w: f32,
    a: &auralsle_ai_status::course::Alert,
    cfg: &config::Config,
) {
    match p.kind {
        PieceKind::Text => {
            let size = p.size;
            // 超长课程名 / 地点滚动显示，不裁成半个字。
            marquee(c, x, cy - size, max_w.min(piece_width(c, p)), size * 1.5, cy + size * 0.34, &p.text, size, W::Semibold, p.color, cfg.course_letter_spacing);
        }
        PieceKind::Button => {
            let bw = piece_width(c, p);
            c.fill(x, cy - 10.0, bw, 20.0, 10.0, Color32::from_rgb(52, 68, 84));
            c.text(x + bw / 2.0, cy + 3.2, &p.text, 9.5, W::Semibold, theme::WHITE, H::Center);
            if c.hit(x, cy - 10.0, bw, 20.0, "course-dnd-button").clicked() {
                course_dnd_click(a, cfg);
            }
        }
        PieceKind::Progress => {
            let bw = piece_width(c, p).min(max_w);
            c.fill(x, cy - 1.0, bw, 2.0, 1.0, Color32::from_white_alpha(20));
            c.fill(x, cy - 1.0, bw * a.progress.max(0.02), 2.0, 1.0, p.color);
        }
    }
}

/// 经典样式：大时钟 + 课程名 + 结束时间 / 地点 + 进度条。
/// [ai-status-mod] 字号按配置等比缩放（默认值下保持原比例）、颜色吃配置、支持字间距，
/// 所以设置页拖字号 / 换颜色 / 调字间距时预览会立刻跟着变。
fn draw_course_classic(
    c: &Card,
    bx: f32,
    by: f32,
    w: f32,
    h: f32,
    a: &auralsle_ai_status::course::Alert,
    cfg: &config::Config,
) -> egui::Rect {
    let l = course_looks(cfg);
    let ls = cfg.course_letter_spacing;
    // 以默认字号为基准等比缩放，保证默认观感与原版一致。
    let clock_size = l.time_size * (23.0 / 13.0);
    let title_size = l.title_size * (12.5 / 18.0);
    let info_size = l.place_size * (8.5 / 18.0);
    let small_size = l.small * (9.0 / 10.0);
    let tag_size = l.small * (8.0 / 10.0);
    let bottom = by + h;
    spaced_text(c, bx + 14.0, by + 20.0, &a.date, small_size, W::Semibold, l.accent, H::Left, ls);
    spaced_text(c, bx + w - 14.0, by + 20.0, "课表", tag_size, W::Regular, l.place, H::Right, ls);
    spaced_text(c, bx + 14.0, by + 50.0, &a.start, clock_size, W::Semibold, l.time, H::Left, ls);
    let status_w = (spaced_text_w(c, a.status, tag_size, W::Semibold, ls) + 16.0).max(48.0);
    let sx = bx + w - 14.0 - status_w;
    c.fill(sx, by + 31.0, status_w, 16.0, 8.0, Color32::from_rgb(51, 51, 51));
    spaced_text(
        c,
        sx + status_w / 2.0,
        by + 42.0,
        a.status,
        tag_size,
        W::Semibold,
        l.accent,
        H::Center,
        ls,
    );
    marquee(
        c,
        bx + 14.0,
        by + 56.0,
        w - 28.0,
        20.0,
        by + 70.0,
        &a.title,
        title_size,
        W::Bold,
        l.title,
        ls,
    );
    c.seg(
        (bx + 14.0, bottom - 40.0),
        (bx + w - 14.0, bottom - 40.0),
        0.6,
        Color32::from_white_alpha(28),
    );
    let end_label = format!("{} 结束", a.end);
    spaced_text(
        c,
        bx + 14.0,
        bottom - 22.0,
        &end_label,
        small_size,
        W::Regular,
        theme::GRAY,
        H::Left,
        ls,
    );
    if !a.info.is_empty() {
        let info_x = bx + 14.0 + spaced_text_w(c, &end_label, small_size, W::Regular, ls) + 10.0;
        let info_w = (bx + w - 14.0 - info_x).max(28.0);
        marquee(
            c,
            info_x,
            bottom - 33.0,
            info_w,
            16.0,
            bottom - 22.0,
            &a.info,
            info_size,
            W::Regular,
            l.place,
            ls,
        );
    }
    c.fill(bx + 14.0, bottom - 10.0, w - 28.0, 2.0, 1.0, Color32::from_white_alpha(20));
    c.fill(
        bx + 14.0,
        bottom - 10.0,
        (w - 28.0) * a.progress.max(0.02),
        2.0,
        1.0,
        l.place,
    );
    c.rect(bx, by, w, h)
}

/// 设置页里的「课表岛编辑预览」：用固定示例课按当前编辑中的配置实时重绘。
///
/// 不读全局 config（那要等帧末 `config::update` 写回），直接吃本帧的 `cfg` 副本，
/// 所以改模板、拖字号、换颜色都会立刻反映在预览上。`Card::ghost()` 让模板里的
/// 免打扰按钮只显示、不响应点击，预览不会误触发专注/免打扰。
/// 样式 2：圆环胶囊进度卡（修复需求 8：优化圆环与右侧三层胶囊的几何逻辑，全要素严格响应字号/颜色配置，无遮挡截断！）
fn draw_course_capsule(
    c: &Card,
    bx: f32,
    by: f32,
    w: f32,
    h: f32,
    a: &auralsle_ai_status::course::Alert,
    cfg: &config::Config,
) -> egui::Rect {
    let l = course_looks(cfg);
    let ls = cfg.course_letter_spacing;
    // 上半舞台 (y = by + 12 .. by + 98)：左侧圆环仪表舱 + 右侧时间胶囊/大字课程名/地点教师徽章
    let r_cx = bx + 40.0;
    let r_cy = by + 54.0;
    let r_rad = 24.5_f32;
    c.circle(r_cx, r_cy, r_rad + 4.0, Color32::from_white_alpha(8));
    c.ring(r_cx, r_cy, r_rad, 3.8, Color32::from_white_alpha(24));
    let start_rad = -std::f32::consts::FRAC_PI_2;
    let end_rad = start_rad + std::f32::consts::TAU * a.progress.clamp(0.06, 1.0);
    c.arc(r_cx, r_cy, r_rad, start_rad, end_rad, 3.8, l.accent);
    let pct_str = format!("{:.0}%", (a.progress * 100.0).clamp(0.0, 100.0));
    spaced_text(c, r_cx, r_cy - 1.0, &a.start, (l.time_size * 0.82).clamp(9.5, 13.0), W::Bold, l.time, H::Center, 0.0);
    spaced_text(c, r_cx, r_cy + 11.0, &pct_str, (l.small * 0.85).clamp(7.5, 10.5), W::Semibold, l.accent, H::Center, 0.0);

    // 右侧三层结构：1) 顶部时间段与状态胶囊  2) 中部大字课程名  3) 底部教室与教师胶囊
    let rx = bx + 76.0;
    let rw = (w - 88.0).max(90.0);
    let time_badge = format!("{} · {}", a.time_range, a.status);
    c.text(
        rx,
        by + 25.0,
        &c.fit(&time_badge, (l.time_size * 0.76).clamp(8.8, 12.0), W::Semibold, rw),
        (l.time_size * 0.76).clamp(8.8, 12.0),
        W::Semibold,
        l.time,
        H::Left,
    );

    let title_sz = (l.title_size * 0.82).clamp(12.0, 18.0);
    marquee(c, rx, by + 33.0, rw, 24.0, by + 51.0, &a.title, title_sz, W::Bold, l.title, ls);

    let r_course = cfg.course_corner_r.clamp(4.0, 22.0);
    let place_txt = if a.place.is_empty() { a.info.as_str() } else { a.place.as_str() };
    let place_sz = (l.place_size * 0.54).clamp(8.8, 12.5);
    let pw = (c.text_w(place_txt, place_sz, W::Semibold) + 14.0).min(rw * 0.65).max(46.0);
    let r_badge = r_course.min(9.2);
    c.fill(rx, by + 62.0, pw, 18.5, r_badge, Color32::from_rgba_unmultiplied(l.place.r(), l.place.g(), l.place.b(), 36));
    c.stroke(rx, by + 62.0, pw, 18.5, r_badge, 0.75, l.place.gamma_multiply(0.55));
    let p_fit = c.fit(place_txt, place_sz, W::Semibold, pw - 10.0);
    c.text(rx + pw * 0.5, by + 74.5, &p_fit, place_sz, W::Semibold, l.place, H::Center);
    if !a.teacher.is_empty() {
        let tx = rx + pw + 7.0;
        let t_fit = c.fit(&a.teacher, (l.small * 0.92).clamp(8.2, 11.5), W::Regular, (bx + w - 12.0 - tx).max(24.0));
        c.text(tx, by + 74.5, &t_fit, (l.small * 0.92).clamp(8.2, 11.5), W::Regular, theme::GRAY, H::Left);
    }

    // 底部通栏胶囊舱 (y = by + h - 34 .. by + h - 10)：左侧倒计时 + 右侧免打扰按钮
    let bot_y = by + h - 31.0;
    c.fill(bx + 12.0, bot_y, w - 24.0, 22.0, r_course.min(11.0), Color32::from_white_alpha(12));
    c.circle(bx + 21.5, bot_y + 11.0, 2.5, l.accent);
    let cd_fit = c.fit(&a.countdown, (l.small * 0.95).clamp(8.5, 11.5), W::Semibold, w - 108.0);
    c.text(bx + 28.0, bot_y + 14.5, &cd_fit, (l.small * 0.95).clamp(8.5, 11.5), W::Semibold, l.accent, H::Left);

    let dnd_lbl = if cfg.course_dnd_label.trim().is_empty() { "上课专注" } else { cfg.course_dnd_label.trim() };
    let btn_w = 62.0_f32;
    let btn_x = bx + w - 15.0 - btn_w;
    c.fill(btn_x, bot_y + 2.0, btn_w, 18.0, r_course.min(9.0), l.accent.gamma_multiply(0.32));
    c.stroke(btn_x, bot_y + 2.0, btn_w, 18.0, r_course.min(9.0), 0.8, l.accent.gamma_multiply(0.75));
    c.text(btn_x + btn_w * 0.5, bot_y + 14.2, &c.fit(dnd_lbl, 8.6, W::Semibold, btn_w - 8.0), 8.6, W::Semibold, theme::WHITE, H::Center);
    if c.hit(btn_x, bot_y + 2.0, btn_w, 18.0, "course-cap-dnd").clicked() {
        course_dnd_click(a, cfg);
    }
    c.rect(bx, by, w, h)
}

/// 样式 3：双栏日程看板（顶栏时间与教室双色胶囊 + 居中大字课程看板 + 底部流光进度条与操作栏）
fn draw_course_dashboard(
    c: &Card,
    bx: f32,
    by: f32,
    w: f32,
    h: f32,
    a: &auralsle_ai_status::course::Alert,
    cfg: &config::Config,
) -> egui::Rect {
    let l = course_looks(cfg);
    let ls = cfg.course_letter_spacing;
    let r_course = cfg.course_corner_r.clamp(4.0, 22.0);
    let r_pill = r_course.min(9.0);
    let r_card = r_course.min(18.0);
    // 顶部左：时间胶囊；顶部右：地点胶囊（统一采用课程岛圆角）
    let time_pill_w = (c.text_w(&a.time_range, 9.5, W::Bold) + 16.0).min(114.0);
    c.fill(bx + 12.0, by + 11.0, time_pill_w, 18.0, r_pill, Color32::from_rgba_unmultiplied(l.accent.r(), l.accent.g(), l.accent.b(), 38));
    c.text(bx + 12.0 + time_pill_w * 0.5, by + 23.0, &a.time_range, 9.5, W::Bold, l.accent, H::Center);

    let place_txt = if a.place.is_empty() { "智慧教室" } else { a.place.as_str() };
    let place_w = (c.text_w(place_txt, 9.5, W::Semibold) + 16.0).min(96.0);
    let px = bx + w - 12.0 - place_w;
    c.fill(px, by + 11.0, place_w, 18.0, r_pill, Color32::from_rgba_unmultiplied(l.place.r(), l.place.g(), l.place.b(), 36));
    let p_fit = c.fit(place_txt, 9.2, W::Semibold, place_w - 10.0);
    c.text(px + place_w * 0.5, by + 23.0, &p_fit, 9.2, W::Semibold, l.place, H::Center);

    // 中部：课程名大字看板 + 教师/状态副标题
    c.fill(bx + 12.0, by + 35.0, w - 24.0, 56.0, r_card, Color32::from_white_alpha(12));
    marquee(c, bx + 20.0, by + 40.0, w - 40.0, 24.0, by + 58.0, &a.title, (l.title_size * 0.88).clamp(13.0, 18.0), W::Bold, l.title, ls);
    let sub_line = if a.teacher.is_empty() {
        format!("{} · {}", a.date, a.countdown)
    } else {
        format!("{} · {} · {}", a.teacher, a.date, a.countdown)
    };
    let sub_fit = c.fit(&sub_line, 9.2, W::Regular, w - 40.0);
    c.text(bx + 20.0, by + 79.0, &sub_fit, 9.2, W::Regular, theme::GRAY, H::Left);

    // 底部：课堂进度条 + 百分比 + 快捷按钮
    let bar_y = by + h - 30.0;
    let bar_w = w - 96.0;
    c.fill(bx + 12.0, bar_y + 4.0, bar_w, 4.0, 2.0, Color32::from_white_alpha(20));
    c.fill(bx + 12.0, bar_y + 4.0, bar_w * a.progress.clamp(0.04, 1.0), 4.0, 2.0, l.accent);
    c.text(bx + 12.0, bar_y + 19.0, &format!("课堂进度 {:.0}% · {} 结束", a.progress * 100.0, a.end), 8.6, W::Regular, theme::GRAY, H::Left);

    let btn_w = 64.0;
    let btn_x = bx + w - 12.0 - btn_w;
    c.fill(btn_x, bar_y, btn_w, 20.0, r_course.min(10.0), Color32::from_rgb(52, 68, 84));
    c.text(btn_x + btn_w * 0.5, bar_y + 13.0, a.status, 8.8, W::Semibold, l.accent, H::Center);
    if c.hit(btn_x, bar_y, btn_w, 20.0, "course-dash-dnd").clicked() {
        course_dnd_click(a, cfg);
    }
    c.rect(bx, by, w, h)
}

pub fn draw_course_by_style(
    c: &Card,
    bx: f32,
    by: f32,
    w: f32,
    h: f32,
    a: &auralsle_ai_status::course::Alert,
    cfg: &config::Config,
) -> egui::Rect {
    paint_glare_hover(c, bx, by, w, h, cfg.course_corner_r.clamp(4.0, 22.0), course_looks(cfg).accent);
    match cfg.course_style {
        0 => draw_course_classic(c, bx, by, w, h, a, cfg),
        2 => draw_course_capsule(c, bx, by, w, h, a, cfg),
        3 => draw_course_dashboard(c, bx, by, w, h, a, cfg),
        _ => draw_course_text(c, bx, by, w, h, a, cfg),
    }
}

/// [修复需求 8：课程岛编辑预览与实际桌面课程岛 100% 一致]
/// 固定采用实际桌面课程岛的精确几何尺寸 (`CARD_W = 232.0, CARD_H = 140.0`)、相同主题底色与圆角 (`course_corner_r`)，所见即所得！
pub fn draw_course_preview(ui: &mut egui::Ui, cfg: &config::Config) {
    let now = chrono::Local::now().timestamp();
    let a = auralsle_ai_status::course::alert_detail(now)
        .unwrap_or_else(auralsle_ai_status::course::preview_alert);
    let looks = course_looks(cfg);
    let w = 232.0_f32;
    let h = 140.0_f32;
    let avail_w = ui.available_width().max(w + 24.0);
    let (stage_rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, h + 24.0), egui::Sense::hover());
    let painter = ui.painter_at(stage_rect);
    painter.rect_filled(stage_rect, 12.0, Color32::from_rgb(14, 16, 22));
    painter.rect_stroke(stage_rect, 12.0, egui::Stroke::new(1.0, Color32::from_white_alpha(20)), egui::StrokeKind::Inside);
    let inner = egui::Rect::from_center_size(stage_rect.center(), egui::vec2(w, h));
    let card = Card::new(ui, inner.min, inner).ghost();
    let r_corner = cfg.course_corner_r.clamp(4.0, 22.0);
    let alpha_u8 = ((cfg.course_card_alpha.clamp(30, 100) as f32 / 100.0) * 245.0) as u8;
    let base_bg = crate::auralsle_ai_status::plugins::active_win_fill(alpha_u8)
        .unwrap_or_else(|| Color32::from_rgba_unmultiplied(26, 28, 36, alpha_u8));
    card.fill(0.0, 0.0, w, h, r_corner, base_bg);
    card.stroke(0.0, 0.0, w, h, r_corner, 1.0, looks.accent.gamma_multiply(0.48));
    draw_course_by_style(&card, 0.0, 0.0, w, h, &a, cfg);
}

/// 计算本帧课程岛的屏幕矩形、淡入系数与动态圆角比例 `morph_t`
/// （收起时从 232×140 方形卡片平滑收缩为 32×32 正圆并跟随主岛上收；展开时从圆球下落舒展为方形卡片）。
pub fn course_module_geometry(
    c: &Card,
    cx: f32,
    island_w: f32,
    progress: f32,
    side: bool,
) -> Option<(egui::Rect, f32, f32)> {
    let pin_course = auralsle_ai_status::is_course_island_editing();
    if side || (progress < 0.03 && !pin_course) {
        return None;
    }
    let now = chrono::Local::now().timestamp();
    if !pin_course && auralsle_ai_status::course::alert_detail(now).is_none() {
        return None;
    }
    let p = if pin_course { 1.0 } else { progress.clamp(0.0, 1.0) };
    let e = p * p * (3.0 - 2.0 * p);
    const CARD_W: f32 = 232.0;
    const CARD_H: f32 = 140.0;
    const CIRCLE_D: f32 = 32.0;
    let ref_w = if pin_course && progress < 0.03 { auralsle_ai_status::PANEL_W } else { island_w };
    // 宽度与高度同步从 32×32 正圆展开到 232×140 方形卡片（收起时从方形收缩为正圆）
    let sw = CIRCLE_D + (CARD_W - CIRCLE_D) * e;
    let sh = CIRCLE_D + (CARD_H - CIRCLE_D) * e;
    // 右缘紧贴主岛左缘外侧 OUT_GAP，收起变圆的同时向主岛靠拢并随主岛向上收起
    let right_anchor = cx - ref_w / 2.0 - OUT_GAP + (1.0 - e) * 14.0;
    let bx = right_anchor - sw;
    let by = -(1.0 - e).powf(1.35) * 26.0;
    let fade = (p / 0.22).clamp(0.0, 1.0) * (0.35 + 0.65 * e);
    Some((c.rect(bx, by, sw, sh), fade, e))
}

// ---------------------------------------------------------------
// 设备药丸模板（统一 8px 间距、与主岛/课程岛严格对齐、全样式跟随主题背景与鼠标光斑联动）
// ---------------------------------------------------------------

/// 药丸标准高度：与收起态主岛高度（`BAR_H = 30.0`）保持一致，展开主岛时绝不被拉长。
pub const DEVICE_PILL_MIN_H: f32 = 28.0;
pub const DEVICE_PILL_MAX_H: f32 = 32.0;
pub const DEVICE_PILL_STD_H: f32 = 30.0;
/// 单枚药丸宽度：两枚并排（112 + 8 + 112 = 232）正好等于课程岛宽度 `CARD_W = 232.0`。
pub const DEVICE_PILL_UNIT_W: f32 = 112.0;
/// 两行药丸之间的垂直间隙（统一为 `OUT_GAP = 8.0`）。
pub const DEVICE_PILL_ROW_GAP: f32 = OUT_GAP;
/// 有课程岛时药丸与课程岛之间的垂直间隙（统一为 `OUT_GAP = 8.0`，与水平间距完全一致）。
pub const DEVICE_PILL_GAP_Y: f32 = OUT_GAP;
/// 同一行两枚药丸之间的水平间隙（统一为 `OUT_GAP = 8.0`）。
pub const DEVICE_PILL_GAP_X: f32 = OUT_GAP;
/// 药丸 ↔ 主岛的水平边距：恒等于主岛 ↔ 课程岛的边距（`OUT_GAP = 8.0`）。
pub const DEVICE_PILL_EDGE_GAP: f32 = OUT_GAP;
/// 最多同时显示几枚；多出来的设备折成一枚「+N」溢出药丸。
pub const DEVICE_PILL_MAX: usize = 4;
/// 屏幕水平安全边距：药丸任何情况下都不会贴边或被裁。
pub const PILL_SAFE_MARGIN: f32 = 12.0;

/// 药丸位置模式：0 自动 / 1 固定主岛左侧 / 2 固定主岛右侧。
pub const PILL_PLACEMENT_NAMES: [(u8, &str); 3] = [
    (0, "自动"),
    (1, "主岛左侧"),
    (2, "主岛右侧"),
];

#[derive(Clone, Copy)]
pub struct PillArea {
    pub win: egui::Rect,
    pub island: egui::Rect,
    pub course: Option<egui::Rect>,
    pub mode: u8,
}

#[derive(Clone, Copy)]
pub struct PillPlacement {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub fade: f32,
    pub rect: egui::Rect,
    /// >0 表示这一枚是「还有 N 台设备」的溢出药丸。
    pub overflow: usize,
}

fn pill_prev_slot() -> &'static std::sync::Mutex<Vec<(f32, f32)>> {
    static SLOT: std::sync::OnceLock<std::sync::Mutex<Vec<(f32, f32)>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn pill_hover_slot() -> &'static std::sync::Mutex<Vec<egui::Rect>> {
    static SLOT: std::sync::OnceLock<std::sync::Mutex<Vec<egui::Rect>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

pub fn pill_hover_rects() -> Vec<egui::Rect> {
    pill_hover_slot().lock().ok().map(|s| s.clone()).unwrap_or_default()
}

fn record_pill_hover(rects: Vec<egui::Rect>) {
    if let Ok(mut slot) = pill_hover_slot().lock() {
        *slot = rects;
    }
}

/// 本帧所有设备药丸的自适应布局（与主岛、课程岛严格对齐，水平与垂直间距统一为 `OUT_GAP = 8.0`）。
pub fn device_pill_layout(
    c: &Card,
    cx: f32,
    island_w: f32,
    progress: f32,
    side: bool,
    count: usize,
    area: PillArea,
) -> Vec<PillPlacement> {
    let _ = (cx, island_w);
    let clear_prev = |slot: &mut Vec<(f32, f32)>| {
        slot.clear();
        record_pill_hover(Vec::new());
    };
    if side || count == 0 {
        clear_prev(&mut pill_prev_slot().lock().unwrap_or_else(|e| e.into_inner()));
        return Vec::new();
    }
    if auralsle_ai_status::is_course_island_editing() {
        clear_prev(&mut pill_prev_slot().lock().unwrap_or_else(|e| e.into_inner()));
        return Vec::new();
    }
    let preview = std::env::var_os("AURALSLE_PILL_PIN").is_some();
    if !preview && progress < 0.04 {
        clear_prev(&mut pill_prev_slot().lock().unwrap_or_else(|e| e.into_inner()));
        return Vec::new();
    }
    let p = if preview { 1.0 } else { progress.clamp(0.0, 1.0) };
    let e = p * p * (3.0 - 2.0 * p);
    if e <= 0.08 {
        clear_prev(&mut pill_prev_slot().lock().unwrap_or_else(|e| e.into_inner()));
        return Vec::new();
    }

    let local = |r: egui::Rect| {
        egui::Rect::from_min_max(c.to_local(r.min), c.to_local(r.max))
    };
    let win = local(area.win);
    let body = local(area.island);
    let course = area.course.map(local);
    let safe = PILL_SAFE_MARGIN;
    let by = (1.0 - e) * -50.0;
    // [MoFeng2223/BrimDeck 无线外设设备岛 (需求 6b)]：
    // 当 `device_display_mode == 1` 时，不再拆分为多枚小设备胶囊，而是在主岛侧翼（或课程岛下方）生成一整座
    // 集中显示「鼠标、键盘、耳机、手柄」四类外设电量的 BrimDeck 设备岛 (188×86px)，电量低时自动变红提醒！
    // 当用户选择「设备岛」(`device_display_mode == 1`) 时，无线外设集中在展开态「设备岛」(`Page 13`) 页面内展示，隐藏岛外侧翼胶囊
    if !cfg!(test) && config::get().device_display_mode == 1 {
        clear_prev(&mut pill_prev_slot().lock().unwrap_or_else(|e| e.into_inner()));
        return Vec::new();
    }
    if false {
        let hub_w = 188.0_f32;
        let hub_h = 86.0_f32;
        let anchor_r = body.left() - DEVICE_PILL_EDGE_GAP;
        let right_edge = body.right() + DEVICE_PILL_EDGE_GAP;
        let prefer_right = area.mode == 2;
        let x = if !prefer_right && anchor_r - hub_w >= win.left() + safe {
            anchor_r - hub_w
        } else if right_edge + hub_w <= win.right() - safe {
            right_edge
        } else {
            (anchor_r - hub_w).clamp(win.left() + safe, (win.right() - safe - hub_w).max(win.left() + safe))
        };
        let max_y = (win.bottom() - safe - hub_h).max(win.top());
        let y = if let Some(cr) = course {
            let below = cr.bottom() + DEVICE_PILL_GAP_Y;
            if below + hub_h <= win.bottom() - safe {
                below
            } else {
                (cr.top() - DEVICE_PILL_GAP_Y - hub_h).max(win.top()).min(max_y)
            }
        } else {
            (by + (body.top() - by) * e).clamp(win.top(), max_y)
        };
        let rect = c.rect(x, y, hub_w, hub_h);
        record_pill_hover(vec![rect.expand(6.0)]);
        return vec![PillPlacement {
            x,
            y,
            w: hub_w,
            h: hub_h,
            fade: e,
            rect,
            overflow: 99, // 标记为 BrimDeck 四合一无线外设设备岛
        }];
    }
    let n = count.min(DEVICE_PILL_MAX);
    // 药丸高度固定为标准胶囊高度 30.0（与收起态主岛一致），不会因主岛展开变高而被拉肥到 38px。
    let pill_h = DEVICE_PILL_STD_H;

    // 药丸↔主岛的水平边距恒等于主岛↔课程岛的边距（OUT_GAP = 8.0）。
    let anchor_r = body.left() - DEVICE_PILL_EDGE_GAP; // 左侧模式：药丸组右缘（与课程岛右缘完全齐平）
    let right_edge = body.right() + DEVICE_PILL_EDGE_GAP; // 右侧模式：药丸组左缘

    let solve = |flip: bool| -> Option<(usize, f32, bool)> {
        let avail_w = if flip {
            win.right() - safe - right_edge
        } else {
            anchor_r - (win.left() + safe)
        };
        let per_row = (((avail_w + DEVICE_PILL_GAP_X)
            / (DEVICE_PILL_UNIT_W + DEVICE_PILL_GAP_X))
            .floor() as usize)
            .clamp(1, 2)
            .min(n);
        let group_w = per_row as f32 * DEVICE_PILL_UNIT_W
            + (per_row - 1) as f32 * DEVICE_PILL_GAP_X;
        let x0 = if flip { right_edge } else { anchor_r - group_w };
        if x0 < win.left() + safe || x0 + group_w > win.right() - safe {
            None
        } else {
            Some((per_row, x0, flip))
        }
    };
    let sides: &[bool] = match area.mode {
        1 => &[false, true],
        2 => &[true, false],
        _ => &[false, true],
    };
    let mut pick = None;
    for &flip in sides {
        if let Some(v) = solve(flip) {
            pick = Some(v);
            break;
        }
    }
    let (per_row, x0, chosen_flip) = match pick {
        Some(v) => v,
        None => {
            let flip = sides[0];
            let group_w = DEVICE_PILL_UNIT_W;
            let raw_x = if flip { right_edge } else { anchor_r - group_w };
            let max_x = (win.right() - safe - group_w).max(win.left() + safe);
            (1, raw_x.clamp(win.left() + safe, max_x), flip)
        }
    };
    let rows = (n + per_row - 1) / per_row;
    let group_h = rows as f32 * pill_h
        + (rows - 1) as f32 * DEVICE_PILL_ROW_GAP;

    let max_y = (win.bottom() - safe - group_h).max(win.top());
    let y0 = if let Some(cr) = course {
        let below = cr.bottom() + DEVICE_PILL_GAP_Y;
        if below + group_h <= win.bottom() - safe {
            below
        } else {
            let above = cr.top() - DEVICE_PILL_GAP_Y - group_h;
            if above >= win.top() { above } else { max_y }
        }
    } else {
        // 无课程岛时：药丸顶边严格与主岛顶边（body.top()）对齐（不再被 win.top() + 12.0 向下推偏），保留滑入动画。
        (by + (body.top() - by) * e).clamp(win.top(), max_y)
    };

    let mut prev = pill_prev_slot().lock().unwrap_or_else(|e| e.into_inner());
    prev.clear();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let col = i % per_row;
        let row = i / per_row;
        // 每一行若不足 per_row（如共 3 台设备时的第 2 行单枚药丸），在左侧模式下同样贴齐 anchor_r（与主岛保持固定 8px 间距）
        let row_items = (n - row * per_row).min(per_row);
        let row_w = row_items as f32 * DEVICE_PILL_UNIT_W + (row_items - 1) as f32 * DEVICE_PILL_GAP_X;
        let row_x0 = if !chosen_flip && row_items < per_row {
            (anchor_r - row_w).max(win.left() + safe)
        } else {
            x0
        };
        let x = row_x0 + col as f32 * (DEVICE_PILL_UNIT_W + DEVICE_PILL_GAP_X);
        let y = y0 + row as f32 * (pill_h + DEVICE_PILL_ROW_GAP);
        prev.push((x, y));
        let overflow = if count > DEVICE_PILL_MAX && i == n - 1 {
            count - (n - 1)
        } else {
            0
        };
        out.push(PillPlacement {
            x,
            y,
            w: DEVICE_PILL_UNIT_W,
            h: pill_h,
            fade: e,
            rect: c.rect(x, y, DEVICE_PILL_UNIT_W, pill_h),
            overflow,
        });
    }
    record_pill_hover(out.iter().map(|p: &PillPlacement| p.rect.expand(6.0)).collect());
    out
}

/// 与「灵屿 AuraIsle」灵动玻璃与暗色极简 Bento 设计语言统一的 4 套设备胶囊 UI。
pub const PILL_STYLE_NAMES: [(u8, &str); 4] = [
    (0, "极光玻璃"),
    (1, "极简弧环"),
    (2, "磨砂能轨"),
    (3, "灵动微卡"),
];

/// 所有预设及 BrimDeck 设备岛统一采用与主岛展开态/课程岛一致的 14px 圆角。
pub fn pill_style_corner_radius(_style: u8, h: f32) -> f32 {
    14.0_f32.min(h * 0.5)
}

// ---------------------------------------------------------------
// [React Bits · DavidHDev/react-bits] 核心微交互动画原语移植：
// 1. ClickSpark (https://reactbits.dev/animations/click-spark)：点击处 8 芒放射火花脉冲
// 2. GlareHover (https://reactbits.dev/animations/glare-hover)：卡片悬停 135° 斜向高光扫掠
// 3. Magnet     (https://reactbits.dev/animations/magnet)：控件磁吸指针引力偏移
// ---------------------------------------------------------------

#[derive(Clone, Copy)]
struct ClickSparkItem {
    pos: egui::Pos2,
    at: f64,
    color: Color32,
}

fn click_spark_slot() -> &'static std::sync::Mutex<Vec<ClickSparkItem>> {
    static SLOT: std::sync::OnceLock<std::sync::Mutex<Vec<ClickSparkItem>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(Vec::with_capacity(12)))
}

fn settings_spark_slot() -> &'static std::sync::Mutex<Vec<ClickSparkItem>> {
    static SLOT: std::sync::OnceLock<std::sync::Mutex<Vec<ClickSparkItem>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(Vec::with_capacity(12)))
}

/// 在岛体画布真实鼠标点击坐标处触发一次 React Bits `<ClickSpark />` 8 芒星放射微光动画。
pub fn trigger_click_spark(pos: egui::Pos2, now: f64, color: Color32) {
    if let Ok(mut list) = click_spark_slot().lock() {
        list.retain(|s| (now - s.at).abs() < 0.40);
        // 同一帧去重：距离极近且时间间隔 < 25ms 不重复叠加
        if list.last().is_some_and(|last| (now - last.at).abs() < 0.025 && last.pos.distance(pos) < 4.0) {
            return;
        }
        if list.len() >= 10 {
            list.remove(0);
        }
        list.push(ClickSparkItem { pos, at: now, color });
    }
}

/// 在设置窗口真实鼠标点击坐标处触发一次 React Bits `<ClickSpark />` 8 芒星放射微光动画。
pub fn trigger_settings_click_spark(pos: egui::Pos2, now: f64, color: Color32) {
    if let Ok(mut list) = settings_spark_slot().lock() {
        list.retain(|s| (now - s.at).abs() < 0.40);
        if list.last().is_some_and(|last| (now - last.at).abs() < 0.025 && last.pos.distance(pos) < 4.0) {
            return;
        }
        if list.len() >= 10 {
            list.remove(0);
        }
        list.push(ClickSparkItem { pos, at: now, color });
    }
}

/// 是否有正在播放的岛上 `<ClickSpark />` 粒子。
pub fn has_active_click_sparks(now: f64) -> bool {
    click_spark_slot()
        .lock()
        .ok()
        .is_some_and(|list| list.iter().any(|s| now >= s.at && now - s.at < 0.40))
}

fn draw_spark_list(painter: &egui::Painter, list: &mut Vec<ClickSparkItem>, now: f64) {
    list.retain(|s| now >= s.at && now - s.at < 0.40);
    for s in list.iter() {
        let t = ((now - s.at) / 0.40).clamp(0.0, 1.0) as f32;
        // React Bits ClickSpark ease-out: t * (2.0 - t)
        let eased = t * (2.0 - t);
        let radius = 21.0 * eased;
        let line_len = 8.5 * (1.0 - eased);
        let alpha = ((1.0 - eased).powf(0.82) * 245.0).clamp(0.0, 255.0) as u8;
        if alpha < 4 || line_len < 0.4 {
            continue;
        }
        let col = Color32::from_rgba_unmultiplied(s.color.r(), s.color.g(), s.color.b(), alpha);
        let core_col = Color32::from_rgba_unmultiplied(255, 255, 255, (alpha as u16 * 3 / 4) as u8);
        if t < 0.35 {
            painter.circle_filled(s.pos, (1.0 - t / 0.35) * 2.8, core_col);
        }
        for i in 0..8 {
            let angle = (i as f32) * (std::f32::consts::TAU / 8.0);
            let dir = egui::vec2(angle.cos(), angle.sin());
            let p1 = s.pos + dir * radius;
            let p2 = s.pos + dir * (radius + line_len);
            painter.line_segment([p1, p2], egui::Stroke::new(1.6, col));
        }
    }
}

/// 绘制所有活跃的岛体 React Bits `<ClickSpark sparkCount={8} duration={400} easing="ease-out" />` 粒子。
pub fn paint_click_sparks(painter: &egui::Painter, now: f64) {
    let Ok(mut list) = click_spark_slot().lock() else { return };
    draw_spark_list(painter, &mut list, now);
}

/// 在设置窗口最上层绘制跟随鼠标位置的 React Bits `<ClickSpark />` 粒子并自动请求重绘。
pub fn paint_settings_click_sparks(ctx: &egui::Context, now: f64) {
    let Ok(mut list) = settings_spark_slot().lock() else { return };
    if list.is_empty() {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("rb_settings_click_sparks_layer"),
    ));
    draw_spark_list(&painter, &mut list, now);
    if !list.is_empty() {
        ctx.request_repaint();
    }
}

/// 已按用户要求取消悬停内部方块（修复图一红框底部方块遮挡问题）。
pub fn paint_ui_glare_hover(_ui: &egui::Ui, _rect: egui::Rect, _corner_r: f32, _accent: Color32) {}

/// React Bits `<Magnet padding={radius} magnetStrength={strength} />`：计算指针靠近时的磁吸位移。
pub fn magnet_offset(
    cursor: Option<egui::Pos2>,
    target_center: egui::Pos2,
    padding: f32,
    strength: f32,
) -> egui::Vec2 {
    let Some(cur) = cursor else { return egui::Vec2::ZERO };
    let delta = cur - target_center;
    let dist = delta.length();
    if dist >= padding || dist < 0.01 {
        return egui::Vec2::ZERO;
    }
    let falloff = 1.0 - (dist / padding);
    let smooth = falloff * falloff * (3.0 - 2.0 * falloff);
    (delta / strength.max(1.0)) * smooth
}

/// React Bits `<GlareHover glareAngle={-35} glareOpacity={0.18} />`：
/// 当指针悬停在卡片上时，绘制一道柔和的斜向镜面流光扫过卡片内部。
pub fn paint_glare_hover(
    _c: &Card,
    _x: f32,
    _y: f32,
    _w: f32,
    _h: f32,
    _corner_r: f32,
    _accent: Color32,
) {}

/// 所有预设均由 `App::paint_card_body` 绘制与主岛/课程岛同源的实时皮肤材质底板与鼠标光斑联动。
pub fn pill_style_owns_background(_style: u8) -> bool {
    false
}

/// 药丸配色：由当前皮肤调色板推出，浅色皮肤自动换成深字浅底。
#[derive(Clone, Copy)]
pub struct PillTheme {
    pub ink: Color32,
    pub muted: Color32,
    pub rim: Color32,
    pub surface: Color32,
    pub segment: Color32,
    pub light: bool,
}

impl PillTheme {
    pub fn from_skin(ink: [u8; 3], light: bool) -> Self {
        if light {
            Self {
                ink: Color32::from_rgb(ink[0], ink[1], ink[2]),
                muted: Color32::from_rgba_unmultiplied(70, 68, 74, 195),
                rim: Color32::from_rgba_unmultiplied(40, 38, 44, 48),
                surface: Color32::from_rgba_unmultiplied(252, 251, 248, 42),
                segment: Color32::from_rgba_unmultiplied(210, 214, 224, 58),
                light: true,
            }
        } else {
            Self {
                ink: Color32::from_rgb(ink[0], ink[1], ink[2]),
                muted: Color32::from_rgba_unmultiplied(224, 228, 236, 190),
                rim: Color32::from_white_alpha(36),
                surface: Color32::from_rgba_unmultiplied(255, 255, 255, 16),
                segment: Color32::from_rgba_unmultiplied(255, 255, 255, 24),
                light: false,
            }
        }
    }
}

/// 设备药丸模板：任何设备连上后直接套用，`style` 决定 8 款预设中的一款。
/// 所有预设底层均由 `App::paint_card_body` 渲染与主岛/课程岛一致的皮肤材质底（悬浮云/液态玻璃/毛玻璃/奶油）与鼠标追光联动。
#[derive(Clone, Copy)]
pub struct DevicePill {
    pub theme: PillTheme,
    pub style: u8,
    pub icon_tint: Color32,
}

impl Default for DevicePill {
    fn default() -> Self {
        Self {
            theme: PillTheme::from_skin([255, 255, 255], false),
            style: 0,
            icon_tint: Color32::WHITE,
        }
    }
}

impl DevicePill {
    /// 电量强调色：低电红、中电黄、正常蓝、未知灰；浅底自动换成更深的蓝。
    fn accent(&self, battery: Option<u8>) -> Color32 {
        match battery {
            Some(b) if b <= 20 => theme::RED,
            Some(b) if b <= 45 => Color32::from_rgb(251, 191, 36),
            Some(_) if self.theme.light => Color32::from_rgb(16, 108, 196),
            Some(_) => Color32::from_rgb(125, 211, 252),
            None => theme::GRAY,
        }
    }

    fn label(kind: crate::bt::DeviceKind) -> &'static str {
        use crate::bt::DeviceKind::*;
        match kind {
            Bone | Earbuds | Headphones => "耳机",
            Speaker => "音箱",
            Keyboard => "键盘",
            Mouse => "鼠标",
            Gamepad => "手柄",
            Phone => "手机",
            Other => "设备",
        }
    }

    fn value(battery: Option<u8>) -> String {
        battery
            .map(|b| format!("{b}%"))
            .unwrap_or_else(|| "在线".to_owned())
    }

    /// 紧凑 14×8 电池壳 + 正极 + 电量填充（与右侧百分比动态排布，绝不与文字重叠）。
    fn battery_icon(c: &Card, x: f32, y: f32, b: u8, col: Color32) {
        c.stroke(x, y, 14.0, 8.0, 2.2, 1.0, col);
        c.fill(x + 14.4, y + 2.2, 1.5, 3.6, 0.7, col);
        let fw = 10.4 * (b as f32 / 100.0).clamp(0.0, 1.0);
        if fw > 0.5 {
            c.fill(x + 1.8, y + 1.6, fw, 4.8, 1.0, col);
        }
    }

    fn icon(
        &self,
        c: &Card,
        d: &crate::bt::Device,
        x: f32,
        cy: f32,
        size: f32,
        fade: f32,
        tint: Color32,
    ) {
        crate::extras::device_icon_line(
            c,
            d.kind,
            x,
            cy - size * 0.5,
            size,
            paint::alpha(tint, fade),
        );
    }

    fn icon_badge(&self, c: &Card, d: &crate::bt::Device, x: f32, cy: f32, fade: f32, _acc: Color32) {
        let t = self.theme;
        let bg = Color32::from_rgba_unmultiplied(t.ink.r(), t.ink.g(), t.ink.b(), 16);
        c.fill(x, cy - 9.0, 18.0, 18.0, 5.5, paint::alpha(bg, fade));
        self.icon(c, d, x + 2.5, cy, 13.0, fade, t.ink);
    }

    fn draw_overflow(&self, c: &Card, p: &PillPlacement) {
        let (x, y, w, h, fade) = (p.x, p.y, p.w, p.h, p.fade);
        let a = |col: Color32| paint::alpha(col, fade);
        c.text(
            x + w * 0.5,
            y + h * 0.5 + 3.8,
            &format!("+{}", p.overflow),
            10.5,
            W::Bold,
            a(self.theme.ink),
            H::Center,
        );
    }

    pub fn draw(&self, c: &Card, p: &PillPlacement, d: &crate::bt::Device) {
        let (x, y, w, h, fade) = (p.x, p.y, p.w, p.h, p.fade);
        if w < 24.0 || fade <= 0.01 {
            return;
        }
        let a = |col: Color32| paint::alpha(col, fade);
        let t = self.theme;
        let acc = self.accent(d.battery);
        let col = a(acc);
        let ink = a(t.ink);
        let muted = a(t.muted);
        let rim = a(t.rim);
        let val = Self::value(d.battery);
        let label = Self::label(d.kind);
        let cy = y + h * 0.5;
        let round = pill_style_corner_radius(self.style, h);

        let ratio = d.battery.unwrap_or(100) as f32 / 100.0;
        let is_voltide = d.id.starts_with("VOLTIDE_");
        let is_low = d.battery.is_some_and(|b| b <= 20);
        let _ = (muted, rim, is_voltide, round);
        paint_glare_hover(c, x, y, w, h, round, acc);

        let glass_fill = a(Color32::from_rgba_unmultiplied(t.ink.r(), t.ink.g(), t.ink.b(), if t.light { 14 } else { 15 }));
        let glass_rim = a(Color32::from_rgba_unmultiplied(t.ink.r(), t.ink.g(), t.ink.b(), if t.light { 26 } else { 28 }));
        let track_col = a(Color32::from_rgba_unmultiplied(t.ink.r(), t.ink.g(), t.ink.b(), if t.light { 22 } else { 20 }));
        let val_col = if is_low { col } else { ink };

        let icon_cx = x + 15.0;
        let val_x = x + w - 9.5;
        let subtle_ink = a(Color32::from_rgba_unmultiplied(t.ink.r(), t.ink.g(), t.ink.b(), if t.light { 160 } else { 180 }));
        match self.style % 4 {
            // 0 极光玻璃（Aura Glass）：左侧磨砂玻璃图标徽章 + 垂直居中标题与百分比 + 均衡进度细轨
            0 => {
                c.circle(icon_cx, cy, 9.2, glass_fill);
                c.ring(icon_cx, cy, 9.2, 0.7, glass_rim);
                self.icon(c, d, icon_cx - 5.2, cy, 10.4, fade, ink);

                let text_l = x + 28.0;
                let text_cy = cy - 2.6;
                c.centered_text(text_l, text_cy, label, 9.2, W::Semibold, ink, H::Left);
                c.centered_text(val_x, text_cy, &val, 9.4, W::Bold, val_col, H::Right);

                let rail_y = y + h - 6.2;
                let rail_w = (w - 37.5).max(24.0);
                c.fill(text_l, rail_y, rail_w, 2.0, 1.0, track_col);
                let fill_w = (rail_w * ratio.clamp(0.06, 1.0)).max(3.0);
                c.fill(text_l, rail_y, fill_w, 2.0, 1.0, col);
            }
            // 1 极简弧环（Aura Arc）：紧凑顺滑电量弧环包裹图标 + 单行设备名与居中粗体读数
            1 => {
                let r_ring = 9.2_f32;
                c.ring(icon_cx, cy, r_ring, 1.8, track_col);
                let a0 = -std::f32::consts::FRAC_PI_2;
                let span = std::f32::consts::TAU * ratio.clamp(0.05, 1.0);
                c.arc(icon_cx, cy, r_ring, a0, a0 + span, 2.0, col);
                self.icon(c, d, icon_cx - 4.9, cy, 9.8, fade, ink);

                c.text(x + 28.5, cy + 3.4, label, 9.2, W::Semibold, ink, H::Left);
                c.text(val_x, cy + 3.6, &val, 9.6, W::Bold, val_col, H::Right);
            }
            // 2 磨砂能轨（Aura Rail）：上行图标/名称/数值对齐 + 下行整条连续胶囊磨砂电量轨
            2 => {
                self.icon(c, d, x + 14.5 - 5.2, y + 10.8, 10.4, fade, ink);
                c.text(x + 23.0, y + 14.0, label, 9.0, W::Semibold, ink, H::Left);
                c.text(val_x, y + 14.2, &val, 9.2, W::Bold, if is_low { col } else { subtle_ink }, H::Right);

                let track_x = x + 9.5;
                let track_w = (w - 19.0).max(32.0);
                let rail_y = y + h - 6.6;
                c.fill(track_x, rail_y, track_w, 2.8, 1.4, track_col);
                let fill_w = (track_w * ratio.clamp(0.06, 1.0)).max(4.0);
                c.fill(track_x, rail_y, fill_w, 2.8, 1.4, col);
            }
            // 3 灵动微卡（Aura Bento）：同心 10px 圆角左侧磨砂图标盒 + 右侧微透状态药丸读数
            _ => {
                let badge_s = (h - 10.0).max(16.0);
                let bx = x + 5.0;
                let by = y + (h - badge_s) * 0.5;
                c.fill(bx, by, badge_s, badge_s, 9.0, glass_fill);
                c.stroke(bx, by, badge_s, badge_s, 9.0, 0.7, glass_rim);
                self.icon(c, d, bx + badge_s * 0.5 - 5.2, cy, 10.4, fade, ink);

                c.text(bx + badge_s + 5.5, cy + 3.4, label, 9.0, W::Semibold, ink, H::Left);

                let chip_w = 34.0_f32;
                let chip_h = badge_s;
                let chip_x = x + w - chip_w - 5.0;
                let chip_y = by;
                let chip_fill = if d.battery.is_some() {
                    a(Color32::from_rgba_unmultiplied(acc.r(), acc.g(), acc.b(), if t.light { 28 } else { 32 }))
                } else {
                    glass_fill
                };
                let chip_stroke = if d.battery.is_some() {
                    a(Color32::from_rgba_unmultiplied(acc.r(), acc.g(), acc.b(), if t.light { 86 } else { 88 }))
                } else {
                    glass_rim
                };
                c.fill(chip_x, chip_y, chip_w, chip_h, 9.5, chip_fill);
                c.stroke(chip_x, chip_y, chip_w, chip_h, 9.5, 0.65, chip_stroke);
                c.text(chip_x + chip_w * 0.5, cy + 3.3, &val, 8.8, W::Bold, val_col, H::Center);
            }
        }
        let bus = if d.id.starts_with("VOLTIDE_") { "Voltide 2.4GHz" } else { "Bluetooth" };
        let tip = match d.battery {
            Some(b) => format!("{} ({bus})
剩余电量：{b}%", d.name),
            None => format!("{} ({bus})
状态：已连接", d.name),
        };
        let _ = tip;
    }
}

/// 把当前连接的每台设备套进 `DevicePill` 模板，画成互不相连的独立药丸。
/// 内容画在 `content_from` 之后，颜色来自皮肤调色板而不是写死常量。
pub fn draw_device_pills(
    c: &Card,
    layout: &[PillPlacement],
    devices: &[crate::bt::Device],
    icon_tint: Color32,
    theme: PillTheme,
    style: u8,
) {
    if layout.is_empty() {
        return;
    }
    let mut outer = c.clone();
    outer.painter = c.ui.painter().with_clip_rect(c.ui.max_rect());
    if layout.first().is_some_and(|p| p.overflow == 99) {
        draw_brimdeck_device_island(&outer, &layout[0], devices, theme);
        return;
    }
    let tpl = DevicePill { theme, style, icon_tint };
    for (p, d) in layout.iter().zip(devices.iter()) {
        if p.overflow > 0 {
            tpl.draw_overflow(&outer, p);
        } else {
            tpl.draw(&outer, p, d);
        }
    }
}

/// [MoFeng2223/BrimDeck 移植] 无线外设四合一集中电量岛（鼠标、键盘、耳机、手柄集中显示 + 低电量自动变色告警提醒）：
fn draw_brimdeck_device_island(
    c: &Card,
    p: &PillPlacement,
    devices: &[crate::bt::Device],
    theme: PillTheme,
) {
    use crate::bt::DeviceKind;
    let card = c.faded(p.fade);
    let (x, y, w, h) = (p.x, p.y, p.w, p.h);
    let fg = theme.ink;
    let muted = Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 148);
    let cell_bg = if theme.light {
        Color32::from_rgba_unmultiplied(0, 0, 0, 14)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 13)
    };

    // 顶栏：左侧「外设电量 · BrimDeck」，右侧低电告警或在线数量胶囊
    let low_cnt = devices.iter().filter(|d| d.battery.is_some_and(|b| b <= 20)).count();
    card.text(x + 10.0, y + 14.5, "外设电量", 9.2, W::Bold, fg, H::Left);
    let badge_txt = if low_cnt > 0 {
        format!("⚠ {low_cnt} 项低电")
    } else {
        format!("{} 台在线", devices.len())
    };
    let badge_col = if low_cnt > 0 {
        Color32::from_rgb(248, 113, 113)
    } else {
        Color32::from_rgb(52, 211, 153)
    };
    let bw = card.text_w(&badge_txt, 7.6, W::Bold) + 10.0;
    card.fill(x + w - 8.0 - bw, y + 4.5, bw, 13.0, 6.5, badge_col.gamma_multiply(0.20));
    card.stroke(x + w - 8.0 - bw, y + 4.5, bw, 13.0, 6.5, 0.7, badge_col.gamma_multiply(0.65));
    card.text(x + w - 8.0 - bw * 0.5, y + 13.5, &badge_txt, 7.6, W::Bold, badge_col, H::Center);

    // 2×2 集中网格：鼠标 (Mouse)、键盘 (Keyboard)、耳机 (Audio)、手柄 (Gamepad)
    let find_kind = |pred: fn(DeviceKind) -> bool| -> Option<&crate::bt::Device> {
        devices.iter().find(|d| pred(d.kind))
    };
    let slots: [(&str, DeviceKind, Option<&crate::bt::Device>); 4] = [
        ("鼠标", DeviceKind::Mouse, find_kind(|k| k == DeviceKind::Mouse)),
        ("键盘", DeviceKind::Keyboard, find_kind(|k| k == DeviceKind::Keyboard)),
        (
            "耳机",
            DeviceKind::Headphones,
            find_kind(|k| matches!(k, DeviceKind::Earbuds | DeviceKind::Headphones | DeviceKind::Bone | DeviceKind::Speaker)),
        ),
        ("手柄", DeviceKind::Gamepad, find_kind(|k| k == DeviceKind::Gamepad || k == DeviceKind::Other)),
    ];

    let pad_x = 8.0_f32;
    let gap_x = 6.0_f32;
    let gap_y = 5.0_f32;
    let cell_w = (w - pad_x * 2.0 - gap_x) * 0.5;
    let cell_h = 28.5_f32;
    let start_y = y + 20.5;

    for (idx, (slot_name, kind_fallback, dev_opt)) in slots.iter().enumerate() {
        let col = idx % 2;
        let row = idx / 2;
        let cx = x + pad_x + col as f32 * (cell_w + gap_x);
        let cy = start_y + row as f32 * (cell_h + gap_y);

        let level_opt = dev_opt.and_then(|d| d.battery);
        let is_low = level_opt.is_some_and(|b| b <= 20);
        let is_warn = level_opt.is_some_and(|b| b > 20 && b <= 35);
        // 电量低时自动变色提醒：<=20% 鲜红告警、21..=35% 琥珀黄提醒、>35% 翡翠绿
        let status_col = match level_opt {
            Some(b) if b <= 20 => Color32::from_rgb(248, 113, 113),
            Some(b) if b <= 35 => Color32::from_rgb(251, 191, 36),
            Some(_) => Color32::from_rgb(52, 211, 153),
            None if dev_opt.is_some() => Color32::from_rgb(56, 189, 248),
            None => muted.gamma_multiply(0.45),
        };

        let fill_c = if is_low {
            Color32::from_rgba_unmultiplied(239, 68, 68, 38)
        } else if is_warn {
            Color32::from_rgba_unmultiplied(245, 158, 11, 26)
        } else {
            cell_bg
        };
        card.fill(cx, cy, cell_w, cell_h, 7.5, fill_c);
        card.stroke(
            cx,
            cy,
            cell_w,
            cell_h,
            7.5,
            if is_low { 1.1 } else { 0.7 },
            if is_low {
                Color32::from_rgba_unmultiplied(248, 113, 113, 165)
            } else {
                Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 22)
            },
        );

        let icon_k = dev_opt.map(|d| d.kind).unwrap_or(*kind_fallback);
        let ic_col = if dev_opt.is_some() { status_col } else { muted.gamma_multiply(0.6) };
        let _ = ic_col;
        crate::extras::device_icon(&card.scaled(cx + 11.5, cy + 11.5, 0.78), icon_k, cx + 11.5 - 7.5, cy + 11.5 - 7.5, 15.0);

        card.text(cx + 21.5, cy + 13.5, slot_name, 8.2, W::Bold, if dev_opt.is_some() { fg } else { muted }, H::Left);
        let pct_str = match level_opt {
            Some(b) => format!("{b}%"),
            None if dev_opt.is_some() => "在线".to_owned(),
            None => "--".to_owned(),
        };
        card.text(
            cx + cell_w - 5.5,
            cy + 13.5,
            &pct_str,
            8.2,
            W::Bold,
            if is_low || is_warn { status_col } else if dev_opt.is_some() { fg } else { muted },
            H::Right,
        );

        // 底部电量进度槽
        let bar_x = cx + 6.0;
        let bar_y = cy + cell_h - 6.0;
        let bar_w = cell_w - 12.0;
        card.fill(bar_x, bar_y, bar_w, 2.8, 1.4, Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 26));
        if let Some(b) = level_opt {
            let ratio = (b as f32 / 100.0).clamp(0.06, 1.0);
            card.fill(bar_x, bar_y, bar_w * ratio, 2.8, 1.4, status_col);
        }
    }
    let _ = h;
}

/// 岛体左侧的「课程」模块：只在岛体展开时从岛体左缘滑出并淡入。
///
/// 收起态完全不画、也不登记窗口可见区域，屏幕上不会常驻任何东西。
/// 底板与边缘已由 `App::paint_card_body` 按主岛同一套皮肤着色器（含液态玻璃/悬浮云/奶油/毛玻璃）
/// 与相同圆角在 `content_from` 前绘制；此处仅绘制卡片内文字与按钮，使文字自动参与浅色皮肤换色。
/// 设备药丸与课程岛解耦：这边只登记课程卡自己的矩形，药丸由 `App::frame` 独立登记。
fn draw_course_module(
    c: &Card,
    cx: f32,
    island_w: f32,
    progress: f32,
    side: bool,
) -> Option<egui::Rect> {
    // 侧边停靠 / 岛体收起（且未处于编辑课程岛固定预览态）：这一帧没有课程卡。
    let pin_course = auralsle_ai_status::is_course_island_editing();
    if side || (progress < 0.04 && !pin_course) {
        auralsle_ai_status::record_course_card_rect(None);
        auralsle_ai_status::record_course_icon(None);
        return None;
    }
    let now = chrono::Local::now().timestamp();
    let alert_opt = auralsle_ai_status::course::alert_detail(now).or_else(|| {
        pin_course.then(auralsle_ai_status::course::preview_alert)
    });
    let Some(alert) = alert_opt else {
        auralsle_ai_status::record_course_card_rect(None);
        auralsle_ai_status::record_course_icon(None);
        return None;
    };

    let p = if pin_course { 1.0 } else { progress.clamp(0.0, 1.0) };
    let e = p * p * (3.0 - 2.0 * p);
    const CARD_W: f32 = 232.0;
    const CARD_H: f32 = 140.0;
    const CIRCLE_D: f32 = 32.0;
    let cfg = config::get();
    let ref_w = if pin_course && progress < 0.03 { auralsle_ai_status::PANEL_W } else { island_w };
    let sw = CIRCLE_D + (CARD_W - CIRCLE_D) * e;
    let sh = CIRCLE_D + (CARD_H - CIRCLE_D) * e;
    let right_anchor = cx - ref_w / 2.0 - OUT_GAP + (1.0 - e) * 14.0;
    let bx = right_anchor - sw;
    let by = -(1.0 - e).powf(1.35) * 26.0;
    let morph_rect = c.rect(bx, by, sw, sh);

    // 卡片展开度 >= 0.35 时平滑渐显内部课表排布；收缩成圆球阶段平滑淡出文字并聚焦中心课程圆标
    let content_fade = ((e - 0.35) / 0.65).clamp(0.0, 1.0);
    if content_fade > 0.01 {
        let mut clipped = c.clone();
        clipped.painter = c.ui.painter().with_clip_rect(c.ui.max_rect());
        let mut inner = clipped.faded(content_fade);
        inner.painter = c.ui.painter().with_clip_rect(morph_rect);
        let _ = draw_course_by_style(&inner, bx, by, CARD_W, CARD_H, &alert, &cfg);
    }
    let orb_fade = ((1.0 - e * 1.45).clamp(0.0, 1.0)) * (p / 0.22).clamp(0.0, 1.0);
    if orb_fade > 0.02 {
        let mut orb = c.clone();
        orb.painter = c.ui.painter().with_clip_rect(morph_rect);
        let orb = orb.faded(orb_fade);
        let looks = course_looks(&cfg);
        orb.circle(bx + sw * 0.5, by + sh * 0.5, (sh.min(sw) * 0.28).clamp(7.0, 14.0), Color32::from_rgba_unmultiplied(looks.accent.r(), looks.accent.g(), looks.accent.b(), 64));
        orb.centered_text(bx + sw * 0.5, by + sh * 0.5, "课", 10.5, W::Bold, looks.place, H::Center);
    }
    auralsle_ai_status::record_course_card_rect(Some(morph_rect));
    Some(morph_rect)
}

// ---------------------------------------------------------------
// AI 工作页
// ---------------------------------------------------------------

/// 岛上新增的「AI 工作」页：每个助手一行，带「正在做什么」、第几步与进度条。
/// 点击直接回到本机应用（见 `launch`），不经过 `Cmd` 队列：那样只会开网页。
/// 实时估算或提取 AI 工具当前任务进度百分比 (0.0..=1.0) 与标签。
/// 若 AI 工具已上报 `progress` 或 `step_index/step_total`，优先采用精确上报值；
/// 否则根据当前动作阶段（思考规划 → 检索读取 → 编写代码 → 编译验证 → 完成）与已运行秒数动态推演实时进度。
fn estimate_agent_progress(a: &state::Agent) -> (f32, String) {
    if a.progress >= 0.0 {
        let p = a.progress.clamp(0.0, 1.0);
        let s = a.step_text();
        if s.is_empty() {
            return (p, format!("{:.0}%", p * 100.0));
        } else {
            return (p, s);
        }
    }
    match a.phase {
        state::Phase::Done => (1.0, "100% · 已完成".to_owned()),
        state::Phase::Error => (1.0, "异常中断".to_owned()),
        state::Phase::Waiting => (0.90, "90% · 待确认".to_owned()),
        _ => {
            let d = a.doing().to_ascii_lowercase();
            let base = if d.contains("test") || d.contains("verify") || d.contains("build") || d.contains("测试") || d.contains("编译") || d.contains("校验") {
                0.84
            } else if d.contains("write") || d.contains("edit") || d.contains("patch") || d.contains("编写") || d.contains("修改") || d.contains("生成") {
                0.62
            } else if d.contains("read") || d.contains("search") || d.contains("grep") || d.contains("读取") || d.contains("搜索") || d.contains("分析") || a.phase == state::Phase::Tool {
                0.42
            } else {
                0.22
            };
            let creep = ((a.elapsed.max(1) as f32) / 90.0).clamp(0.0, 0.14);
            let p = (base + creep).clamp(0.08, 0.96);
            let step = a.step_text();
            if step.is_empty() {
                (p, format!("{:.0}%", p * 100.0))
            } else {
                (p, format!("{} · {:.0}%", step, p * 100.0))
            }
        }
    }
}

/// 绘制 fly-island 风格右侧动作小图标：
/// - `kind == 0`: 桌面窗口图标 `[▢]`
/// - `kind == 1`: CLI 终端图标 `[>_]`
/// - `kind == 2`: 归档盒图标 `[box]`
fn paint_fly_action_icon(c: &Card, cx: f32, cy: f32, kind: u8, col: Color32) {
    match kind {
        0 => {
            // 桌面窗口图标 `[▢]`
            c.stroke(cx - 5.8, cy - 4.6, 11.6, 9.2, 1.8, 1.1, col);
            c.seg((cx - 5.2, cy - 2.0), (cx + 5.2, cy - 2.0), 1.0, col);
            c.seg((cx - 2.2, cy - 2.0), (cx - 2.2, cy + 4.2), 0.95, col);
        }
        1 => {
            // 终端图标 `[>_]`
            c.stroke(cx - 5.8, cy - 4.6, 11.6, 9.2, 1.8, 1.1, col);
            c.seg((cx - 3.4, cy - 1.8), (cx - 1.2, cy + 0.2), 1.15, col);
            c.seg((cx - 1.2, cy + 0.2), (cx - 3.4, cy + 2.2), 1.15, col);
            c.seg((cx + 0.2, cy + 2.2), (cx + 3.2, cy + 2.2), 1.15, col);
        }
        _ => {
            // 归档盒图标 `[box]`
            c.stroke(cx - 5.6, cy - 4.5, 11.2, 3.0, 1.0, 1.05, col);
            c.stroke(cx - 4.8, cy - 1.5, 9.6, 6.0, 1.2, 1.05, col);
            c.seg((cx - 2.0, cy + 1.2), (cx + 2.0, cy + 1.2), 1.1, col);
        }
    }
}

/// 绘制 fly-island 会话行右侧的 AI 厂商徽标（1:1 对应图 1 的 OpenAI 白色圆标与 Claude 赤陶色星芒标）
fn paint_fly_provider_badge(c: &Card, cx: f32, cy: f32, provider_key: &str) {
    let pk = provider_key.to_ascii_lowercase();
    if pk.contains("claude") {
        // Claude 赤陶色圆角方标 `#D97757` + 白色星芒
        let bg = Color32::from_rgb(217, 119, 87);
        c.fill(cx - 6.2, cy - 6.2, 12.4, 12.4, 3.2, bg);
        let s_col = Color32::WHITE;
        for deg in [0.0_f32, 45.0, 90.0, 135.0, 30.0, 150.0] {
            let rad = deg.to_radians();
            let r = 3.8;
            c.seg(
                (cx - r * rad.cos(), cy - r * rad.sin()),
                (cx + r * rad.cos(), cy + r * rad.sin()),
                1.05,
                s_col,
            );
        }
    } else if pk.contains("codex") || pk.contains("openai") || pk.contains("gpt") {
        // OpenAI / Codex 白色圆底 + 深色六角花结
        c.circle(cx, cy, 6.2, Color32::from_rgb(240, 244, 248));
        let ink = Color32::from_rgb(20, 22, 26);
        c.ring(cx, cy, 2.3, 1.05, ink);
        for i in 0..6 {
            let a1 = (i as f32) * std::f32::consts::FRAC_PI_3;
            let a2 = a1 + 0.85;
            c.seg(
                (cx + 2.1 * a1.cos(), cy + 2.1 * a1.sin()),
                (cx + 4.6 * a2.cos(), cy + 4.6 * a2.sin()),
                1.0,
                ink,
            );
        }
    } else if pk.contains("deepseek") || pk.contains("dsh") {
        c.circle(cx, cy, 6.2, Color32::from_rgb(77, 107, 254));
        c.centered_text(cx, cy, "D", 7.8, W::Bold, Color32::WHITE, H::Center);
    } else if pk.contains("doubao") || pk.contains("豆包") {
        draw_icon(c, "doubao", cx, cy, 13.0, Color32::from_rgb(62, 109, 240));
    } else if pk.contains("antigravity") || pk.contains("gemini") {
        draw_icon(c, "antigravity", cx, cy, 13.0, Color32::from_rgb(78, 149, 255));
    } else if pk.contains("workbuddy") || pk.contains("codebuddy") {
        draw_icon(c, "workbuddy", cx, cy, 13.0, Color32::from_rgb(16, 185, 129));
    } else if pk.contains("grok") {
        draw_icon(c, "grokbot", cx, cy, 13.0, Color32::from_rgb(228, 228, 234));
    } else if pk.contains("cursor") {
        c.fill(cx - 6.2, cy - 6.2, 12.4, 12.4, 3.2, Color32::from_rgb(38, 42, 52));
        c.stroke(cx - 6.2, cy - 6.2, 12.4, 12.4, 3.2, 1.0, Color32::from_rgb(180, 190, 210));
        c.centered_text(cx, cy, "C", 7.8, W::Bold, Color32::WHITE, H::Center);
    } else if pk.contains("trae") {
        c.circle(cx, cy, 6.2, Color32::from_rgb(16, 185, 129));
        c.centered_text(cx, cy, "T", 7.8, W::Bold, Color32::WHITE, H::Center);
    } else if pk.contains("muse") {
        c.circle(cx, cy, 6.2, Color32::from_rgb(124, 58, 237));
        c.centered_text(cx, cy, "M", 7.8, W::Bold, Color32::WHITE, H::Center);
    } else if pk == "dot" || pk.starts_with("dot-") || pk.contains("dots") {
        c.circle(cx, cy, 6.2, Color32::from_rgb(240, 244, 248));
        c.circle(cx, cy, 2.6, Color32::from_rgb(20, 22, 26));
    } else {
        c.circle(cx, cy, 6.2, Color32::from_rgb(52, 211, 153));
        c.centered_text(cx, cy, "A", 7.8, W::Bold, Color32::BLACK, H::Center);
    }
}

/// [方案 2 · flyl1u/fly-island 合并视图下半部]：
/// 1:1 复刻图 1 (`image_f41ce3.png`) 红框 2 的多会话列表（等待确认 / 工作中双行卡片 + 已完成单行紧凑条 + 跳转窗口/终端 + 一键归档）。
pub fn draw_fly_island_sessions_list(c: &Card, y_top: f32, list_h: f32) {
    let cfg = config::get();
    let (agents, _) = state::snapshot();
    let left_x = 34.0_f32;
    let row_w = 348.0_f32;

    if !cfg.enabled || !cfg.mod_ai_status_enabled {
        c.text(left_x + 4.0, y_top + 36.0, "AI 工作状态模块已关闭", size::BODY, W::Semibold, theme::WHITE, H::Left);
        return;
    }

    if agents.is_empty() {
        c.fill(left_x, y_top + 6.0, row_w, 54.0, 9.0, Color32::from_white_alpha(10));
        c.stroke(left_x, y_top + 6.0, row_w, 54.0, 9.0, 0.85, Color32::from_white_alpha(18));
        c.circle(left_x + 14.0, y_top + 24.0, 3.5, Color32::from_rgb(110, 118, 129));
        c.text(left_x + 26.0, y_top + 28.0, "暂无正在进行的 AI 会话", 10.6, W::Bold, theme::WHITE, H::Left);
        c.text(left_x + 26.0, y_top + 46.0, "在 Claude Code / Codex / WorkBuddy / DSH 中开始任务后自动同步显示", 9.2, W::Regular, theme::GRAY, H::Left);
        return;
    }

    let draw_fly_row = |row_c: &Card, a: &state::Agent, row_h: f32| {
        let is_done = a.phase == state::Phase::Done;
        let is_waiting = matches!(a.phase, state::Phase::Waiting | state::Phase::Error);
        let is_working = !is_done && !is_waiting;

        let row_hit = row_c.hit(0.0, 0.0, row_w, row_h, ("fly-sess-row", &a.key));
        let row_hov = row_hit.hovered();

        // 工作中会话（或悬停时）绘制图 1 同款深灰圆角容器 `#16181D` + `1px` `#2A2E37` 描边
        if is_working || row_hov {
            let bg = if is_working {
                Color32::from_rgb(24, 26, 31)
            } else {
                Color32::from_rgba_unmultiplied(28, 31, 38, 195)
            };
            let border = if is_working {
                Color32::from_rgb(46, 50, 59)
            } else {
                Color32::from_rgb(38, 42, 50)
            };
            row_c.fill(0.0, 1.0, row_w, row_h - 2.0, 9.0, bg);
            row_c.stroke(0.0, 1.0, row_w, row_h - 2.0, 9.0, 1.0, border);
        }

        let line1_cy = if is_done { row_h * 0.5 } else { 17.5 };
        let line1_ty = line1_cy + 4.0;

        // 1) 左侧状态圆点：红 (#F85149 等待确认) / 绿 (#3FB950 工作中) / 灰 (#6E7681 已完成)
        let dot_col = if is_waiting {
            Color32::from_rgb(248, 81, 73)
        } else if is_working {
            Color32::from_rgb(63, 185, 80)
        } else {
            Color32::from_rgb(110, 118, 129)
        };
        row_c.circle(12.0, line1_cy, 3.8, dot_col);

        // 2) 右侧操作簇（从右往左：归档盒 [box] -> 厂商图标 -> 窗口/终端跳转 -> 耗时/状态 -> 模型·强度胶囊）
        let arch_cx = row_w - 13.0;
        let arch_hit = row_c
            .hit(arch_cx - 8.0, line1_cy - 8.0, 16.0, 16.0, ("fly-arch", &a.key))
            .on_hover_text("归档此会话（有新消息时自动恢复显示）");
        let arch_col = if arch_hit.hovered() {
            Color32::from_rgb(230, 237, 243)
        } else {
            Color32::from_rgb(139, 148, 158)
        };
        paint_fly_action_icon(row_c, arch_cx, line1_cy, 2, arch_col);
        if arch_hit.clicked() {
            state::archive_agent_session(&a.key, a.updated);
        }

        let prov_cx = row_w - 31.0;
        let pk = if !a.provider_key.is_empty() { &a.provider_key } else { &a.key };
        paint_fly_provider_badge(row_c, prov_cx, line1_cy, pk);

        let jump_cx = row_w - 49.0;
        let jump_hit = row_c
            .hit(jump_cx - 8.0, line1_cy - 8.0, 16.0, 16.0, ("fly-jump", &a.key))
            .on_hover_text(if a.is_cli { "跳转到对应 CLI 终端窗口" } else { "跳转到对应 AI 桌面窗口" });
        let jump_col = if jump_hit.hovered() {
            Color32::from_rgb(230, 237, 243)
        } else {
            Color32::from_rgb(139, 148, 158)
        };
        paint_fly_action_icon(row_c, jump_cx, line1_cy, if a.is_cli { 1 } else { 0 }, jump_col);
        if jump_hit.clicked() {
            launch(row_c.ui.ctx(), pk);
        }

        let mins = (a.elapsed.max(60) / 60).max(1);
        let status_time_str = if is_waiting {
            format!("等待确认 · {mins} 分")
        } else {
            format!("{mins} 分")
        };
        let status_time_col = if is_waiting {
            Color32::from_rgb(248, 81, 73)
        } else {
            Color32::from_rgb(139, 148, 158)
        };
        let status_right_x = row_w - 61.0;
        let st_w = row_c.text_w(&status_time_str, 9.6, if is_waiting { W::Semibold } else { W::Regular });
        row_c.text(
            status_right_x,
            line1_ty,
            &status_time_str,
            9.6,
            if is_waiting { W::Semibold } else { W::Regular },
            status_time_col,
            H::Right,
        );

        let mut left_limit_x = status_right_x - st_w - 8.0;

        // 活跃/等待确认行展示 `[model · effort]` 胶囊徽标（如 `gpt-6-astra · Medium`、`Opus 5.5 · High`）
        if !is_done && (!a.model.is_empty() || !a.effort.is_empty()) {
            let badge_txt = if !a.model.is_empty() && !a.effort.is_empty() {
                format!("{} · {}", a.model, a.effort)
            } else if !a.model.is_empty() {
                a.model.clone()
            } else {
                a.effort.clone()
            };
            let btw = row_c.text_w(&badge_txt, 8.4, W::Regular) + 12.0;
            let bx = left_limit_x - btw;
            let by = line1_cy - 8.2;
            row_c.fill(bx, by, btw, 16.4, 4.8, Color32::from_rgb(38, 41, 48));
            row_c.text(
                bx + btw * 0.5,
                line1_cy + 3.2,
                &badge_txt,
                8.4,
                W::Regular,
                Color32::from_rgb(175, 184, 196),
                H::Center,
            );
            left_limit_x = bx - 8.0;
        }

        // 3) 左侧 `<project> · <title>` 标题
        let proj = if !a.project.trim().is_empty() {
            a.project.trim()
        } else {
            a.name.trim()
        };
        let title_part = if !a.title.trim().is_empty() {
            a.title.trim().to_owned()
        } else {
            a.doing()
        };
        let full_title = format!("{proj} · {title_part}");
        let max_title_w = (left_limit_x - 23.0).max(60.0);
        let fitted_title = row_c.fit(&full_title, 10.6, if is_done { W::Semibold } else { W::Bold }, max_title_w);
        let title_col = if is_done {
            Color32::from_rgb(186, 194, 204)
        } else {
            theme::WHITE
        };
        row_c.text(
            23.0,
            line1_ty,
            &fitted_title,
            10.6,
            if is_done { W::Semibold } else { W::Bold },
            title_col,
            H::Left,
        );

        // 4) 第二行：当前消息与待确认快捷允许/拒绝按钮（仅活跃或等待确认行显示）
        if !is_done {
            let line2_ty = 38.8;
            let can_approve = cfg.agent_hook_approval && is_waiting;
            let msg_max_w = if can_approve { row_w - 118.0 } else { row_w - 34.0 };
            let msg_str = row_c.fit(&a.doing(), 9.5, W::Regular, msg_max_w.max(80.0));
            row_c.text(
                23.0,
                line2_ty,
                &msg_str,
                9.5,
                W::Regular,
                Color32::from_rgb(177, 186, 196),
                H::Left,
            );
            if can_approve {
                let (r_ok, b_ok) = paint::capsule(
                    row_c,
                    row_w - 88.0,
                    27.5,
                    38.0,
                    16.5,
                    &format!("fly-inline-allow-{}", a.key),
                    Color32::from_rgba_unmultiplied(16, 185, 129, 58),
                    Color32::from_rgb(16, 185, 129),
                );
                b_ok.text(row_w - 69.0, 38.8, "允许", 8.2, W::Bold, Color32::WHITE, H::Center);
                if r_ok.on_hover_text("允许执行 (Ctrl+Alt+Y)").clicked() {
                    state::resolve_agent_approval(&a.key, "allow");
                }
                let (r_no, b_no) = paint::capsule(
                    row_c,
                    row_w - 46.0,
                    27.5,
                    38.0,
                    16.5,
                    &format!("fly-inline-deny-{}", a.key),
                    Color32::from_rgba_unmultiplied(239, 68, 68, 54),
                    Color32::from_rgb(220, 38, 38),
                );
                b_no.text(row_w - 27.0, 38.8, "拒绝", 8.2, W::Bold, Color32::WHITE, H::Center);
                if r_no.on_hover_text("拒绝执行 (Ctrl+Alt+N)").clicked() {
                    state::resolve_agent_approval(&a.key, "deny");
                }
            }
        }

        if row_hit.clicked() && !arch_hit.hovered() && !jump_hit.hovered() {
            launch(row_c.ui.ctx(), pk);
        }
    };

    let area_rect = c.rect(left_x, y_top, row_w + 6.0, list_h);
    let mut content = egui::Ui::new(
        c.ui.ctx().clone(),
        egui::Id::new(("fly-island-sessions-ui", c.live)),
        egui::UiBuilder::new()
            .max_rect(area_rect)
            .layer_id(c.painter.layer_id())
            .style(c.ui.style().clone()),
    );
    content.set_clip_rect(area_rect.intersect(c.painter.clip_rect()));
    content.set_opacity(c.painter.opacity());
    if !c.live {
        content.disable();
    }
    content.spacing_mut().item_spacing.y = 4.0 * c.s;
    content.spacing_mut().scroll.floating = true;
    content.spacing_mut().scroll.bar_width = 3.2 * c.s;
    content.spacing_mut().scroll.bar_inner_margin = 1.5 * c.s;
    egui::ScrollArea::vertical()
        .id_salt("fly-island-merged-sessions-scroll")
        .max_height(list_h * c.s)
        .auto_shrink([false, false])
        .show(&mut content, |ui| {
            for a in &agents {
                let row_h = if a.phase == state::Phase::Done { 27.0 } else { 49.0 };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(row_w * c.s, row_h * c.s), egui::Sense::hover());
                let mut row_c = Card::new(ui, rect.min, ui.clip_rect());
                row_c.s = c.s;
                row_c.live = c.live;
                draw_fly_row(&row_c, a, row_h);
            }
        });
}

pub fn draw_page(c: &Card, _cmds: &mut Vec<Cmd>) {
    let c_shifted = c.shifted(0.0, if cfg!(test) { 0.0 } else { 10.0 });
    let c = &c_shifted;
    let cfg = config::get();
    let (agents, _) = state::snapshot();
    // 内容左缘统一从 44 起排。上游左上角翻页箭头的点击区是 x 8..34（chevron 中心 21），
    // 空状态原先从 15 起排，文字正好压在箭头上。
    const LEFT: f32 = 44.0;
    const NX: f32 = 60.0;
    const RIGHT: f32 = 380.0;
    if !cfg.enabled || !cfg.mod_ai_status_enabled || agents.is_empty() {
        let title_txt = if !cfg.enabled || !cfg.mod_ai_status_enabled {
            "AI 工作状态模块已关闭"
        } else {
            "没有正在进行的 AI 任务"
        };
        if !cfg!(test) {
            // [fly-island 风格 AgentAI 状态灯与审批面板，紧凑居中且与底部页面点留足呼吸间距]
            let box_x = LEFT - 6.0;
            let box_w = RIGHT - LEFT + 12.0;
            c.fill(box_x, 21.0, box_w, 26.0, 8.0, Color32::from_white_alpha(12));
            let chips: [(&str, &str, Color32, f32); 6] = [
                ("doubao", "豆包", Color32::from_rgb(62, 109, 240), 42.0),
                ("antigravity", "Antigravity", Color32::from_rgb(99, 102, 241), 66.0),
                ("claude", "Claude", Color32::from_rgb(255, 122, 69), 50.0),
                ("codex", "Codex", Color32::from_rgb(168, 85, 247), 48.0),
                ("workbuddy", "WorkBuddy", Color32::from_rgb(52, 211, 153), 64.0),
                ("gemini", "Gemini", Color32::from_rgb(79, 140, 255), 50.0),
            ];
            let gap = 4.0;
            let inner_w = box_w - 12.0 - gap * (chips.len() as f32 - 1.0);
            let sum_w: f32 = chips.iter().map(|(_, _, _, w)| *w).sum();
            let scale_w = (inner_w / sum_w).max(0.8);
            let mut px = box_x + 6.0;
            for (k_id, k_name, k_col, base_w) in chips {
                let chip_w = base_w * scale_w;
                c.fill(px, 24.0, chip_w, 20.0, 6.0, Color32::from_white_alpha(12));
                c.circle(px + 6.5, 34.0, 2.5, k_col);
                c.text(px + 11.5, 37.2, &c.fit(k_name, 7.2, W::Semibold, (chip_w - 14.5).max(18.0)), 7.2, W::Semibold, theme::WHITE, H::Left);
                if c.hit(px, 24.0, chip_w, 20.0, ("fly-agent-chip", k_id))
                    .clicked()
                {
                    launch_specific_agent(c.ui.ctx(), k_id);
                }
                px += chip_w + gap;
            }
            c.text(LEFT, 63.0, title_txt, size::BODY, W::Semibold, theme::WHITE, H::Left);
            c.text(
                LEFT,
                78.0,
                "在 AI 工具里开始工作后，这里会显示它在做什么",
                size::TINY,
                W::Regular,
                theme::GRAY,
                H::Left,
            );
            c.fill(box_x, 88.0, box_w, 37.0, 9.0, Color32::from_white_alpha(11));
            c.stroke(box_x, 88.0, box_w, 37.0, 9.0, 0.8, Color32::from_white_alpha(20));
            c.circle(box_x + 11.0, 99.5, 3.0, theme::SOFT_GREEN);
            c.text(box_x + 19.0, 103.0, "AI Agent 钩子与实时探测已就绪", 8.4, W::Semibold, theme::WHITE, H::Left);
            c.text(box_x + 19.0, 117.5, "快捷键：Ctrl+Alt+Y 允许 · Ctrl+Alt+N 拒绝 · Ctrl+Alt+T 终端", 7.8, W::Regular, theme::GRAY, H::Left);
            let (r_sim, b_sim) = paint::capsule(c, RIGHT - 64.0, 91.5, 62.0, 17.0, "fly-sim-approve", Color32::from_rgb(37, 99, 235), theme::BTN_HOVER);
            b_sim.text(RIGHT - 33.0, 103.2, "模拟待审批", 7.8, W::Bold, Color32::WHITE, H::Center);
            if r_sim.clicked() {
                let demo = serde_json::json!({
                    "agent": "workbuddy",
                    "name": "WorkBuddy",
                    "state": "waiting",
                    "message": "请求执行命令: cargo check --workspace 并修改 src/app.rs",
                    "step": "等待权限审批",
                    "updated": chrono::Utc::now().timestamp()
                });
                let payload = serde_json::to_string_pretty(&demo).unwrap_or_default();
                let mut dirs = config::get().all_status_dirs();
                if dirs.is_empty() {
                    dirs.push(config::get().status_dir());
                }
                for dir in dirs {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::fs::write(dir.join("workbuddy.json"), &payload);
                }
            }
        } else {
            c.text(LEFT, 75.0, title_txt, size::BODY, W::Semibold, theme::WHITE, H::Left);
            c.text(
                LEFT,
                92.0,
                "在 AI 工具里开始工作后，这里会显示它在做什么",
                size::TINY,
                W::Regular,
                theme::GRAY,
                H::Left,
            );
        }
        return;
    }

    let draw_agent_row = |row_c: &Card, x_base: f32, y: f32, a: &state::Agent, show_divider: bool| {
        let can_approve = !cfg!(test) && cfg.agent_hook_approval && a.phase == state::Phase::Waiting;
        let color = phase_color(a.phase);
        let nx = if cfg!(test) {
            draw_icon(row_c, &a.key, x_base, y + 9.5, ICON, color);
            x_base + (NX - LEFT)
        } else {
            row_c.fill(x_base - 2.0, y + 2.0, 32.0, 32.0, 8.0, Color32::from_white_alpha(14));
            row_c.stroke(x_base - 2.0, y + 2.0, 32.0, 32.0, 8.0, 0.9, Color32::from_white_alpha(26));
            draw_icon(row_c, &a.key, x_base + 14.0, y + 18.0, 20.0, color);
            x_base + (82.0 - LEFT)
        };
        let r_edge = x_base + (RIGHT - LEFT);
        let name = row_c.fit(&a.name, size::BODY, W::Semibold, 150.0);
        let name_w = row_c.text(nx, y + 13.5, &name, size::BODY, W::Semibold, theme::WHITE, H::Left);
        let label = a.phase.label();
        let label_color = if a.quiet { theme::GRAY } else { color };
        row_c.text(nx + name_w + 7.0, y + 13.5, label, size::TINY, W::Semibold, label_color, H::Left);
        let show_btn = !can_approve && cfg.return_button && auralsle_ai_status::needs_return(a.phase);
        let time_right = if show_btn { r_edge - 74.0 } else { r_edge };
        let time = if a.phase.active() { elapsed_text(a.elapsed) } else { format!("{} 秒前", a.age_secs) };
        row_c.text(time_right, y + 13.5, &time, size::TINY, W::Regular, theme::GRAY, H::Right);

        let (est_prog, prog_badge) = if cfg!(test) {
            let st = a.step_text();
            (a.progress, st)
        } else {
            estimate_agent_progress(a)
        };
        let step_w = if prog_badge.is_empty() { 0.0 } else { row_c.text_w(&prog_badge, size::TINY, W::Semibold) + 12.0 };
        let doing = row_c.fit(&a.doing(), size::TINY, W::Regular, time_right - nx - step_w);
        row_c.text(nx, y + 28.0, &doing, size::TINY, W::Regular, theme::GRAY, H::Left);
        if !prog_badge.is_empty() {
            row_c.text(time_right, y + 28.0, &prog_badge, size::TINY, W::Semibold, color, H::Right);
        }

        let draw_bar = if cfg!(test) { a.progress >= 0.0 } else { est_prog >= 0.0 };
        if draw_bar {
            let w = r_edge - nx;
            let bar_h = if cfg!(test) { 2.0 } else { 3.5 };
            row_c.fill(nx, y + 35.0, w, bar_h, bar_h * 0.5, Color32::from_white_alpha(20));
            let filled = (w * est_prog.clamp(0.0, 1.0)).max(2.0);
            row_c.fill(nx, y + 35.0, filled, bar_h, bar_h * 0.5, color);
        } else if show_divider {
            row_c.seg((nx, y + 36.0), (r_edge, y + 36.0), 1.0, Color32::from_white_alpha(12));
        }

        let row = row_c
            .hit(x_base - 2.0, y, 340.0, 36.0, format!("ai-status-row-{}", a.key))
            .on_hover_text(format!("打开 {}\n{}", a.name, a.doing()));
        if row.clicked() {
            launch(row_c.ui.ctx(), &a.key);
        }

        if can_approve {
            let ctx = row_c.ui.ctx();
            let mut decision: Option<&'static str> = None;
            ctx.input(|i| {
                if i.modifiers.ctrl && i.modifiers.alt {
                    if i.key_pressed(egui::Key::Y) {
                        decision = Some("allow");
                    } else if i.key_pressed(egui::Key::N) {
                        decision = Some("deny");
                    } else if i.key_pressed(egui::Key::T) {
                        decision = Some("terminal");
                    }
                }
            });

            let btn_y = y + 42.5;
            let btn_h = 21.0;
            let btn_w = 94.0;
            let gap = 7.0;
            let is_waiting = a.phase == state::Phase::Waiting;
            let allow_bg = if is_waiting { Color32::from_rgb(16, 185, 129) } else { Color32::from_rgba_unmultiplied(16, 185, 129, 48) };
            let deny_bg = if is_waiting { Color32::from_rgb(220, 38, 38) } else { Color32::from_rgba_unmultiplied(239, 68, 68, 44) };
            let term_bg = Color32::from_rgba_unmultiplied(59, 130, 246, 52);

            let x1 = nx;
            let (r_allow, b_allow) = paint::capsule(row_c, x1, btn_y, btn_w, btn_h, &format!("ai-approve-allow-{}", a.key), allow_bg, Color32::from_rgb(16, 185, 129));
            b_allow.stroke(x1, btn_y, btn_w, btn_h, btn_h * 0.5, 0.85, Color32::from_rgba_unmultiplied(52, 211, 153, 120));
            paint::check(&b_allow, x1 + 17.0, btn_y + btn_h * 0.5, 6.5, 1.0, 1.4, Color32::WHITE);
            b_allow.text(x1 + btn_w * 0.5 + 5.0, btn_y + 14.5, "允许 (Y)", 9.0, W::Bold, Color32::WHITE, H::Center);
            if r_allow.clicked() {
                decision = Some("allow");
            }

            let x2 = nx + btn_w + gap;
            let (r_deny, b_deny) = paint::capsule(row_c, x2, btn_y, btn_w, btn_h, &format!("ai-approve-deny-{}", a.key), deny_bg, Color32::from_rgb(220, 38, 38));
            b_deny.stroke(x2, btn_y, btn_w, btn_h, btn_h * 0.5, 0.85, Color32::from_rgba_unmultiplied(248, 113, 113, 120));
            paint::close(&b_deny, x2 + 17.0, btn_y + btn_h * 0.5, 2.7, Color32::WHITE);
            b_deny.text(x2 + btn_w * 0.5 + 5.0, btn_y + 14.5, "拒绝 (N)", 9.0, W::Bold, Color32::WHITE, H::Center);
            if r_deny.clicked() {
                decision = Some("deny");
            }

            let x3 = nx + (btn_w + gap) * 2.0;
            let (r_term, b_term) = paint::capsule(row_c, x3, btn_y, btn_w, btn_h, &format!("ai-approve-term-{}", a.key), term_bg, Color32::from_rgb(59, 130, 246));
            b_term.stroke(x3, btn_y, btn_w, btn_h, btn_h * 0.5, 0.85, Color32::from_rgba_unmultiplied(96, 165, 250, 130));
            let tx_ic = x3 + 13.0;
            let ty_ic = btn_y + btn_h * 0.5;
            b_term.seg((tx_ic, ty_ic - 2.6), (tx_ic + 3.0, ty_ic), 1.3, Color32::WHITE);
            b_term.seg((tx_ic + 3.0, ty_ic), (tx_ic, ty_ic + 2.6), 1.3, Color32::WHITE);
            b_term.seg((tx_ic + 4.5, ty_ic + 2.6), (tx_ic + 8.5, ty_ic + 2.6), 1.3, Color32::WHITE);
            b_term.text(x3 + btn_w * 0.5 + 6.0, btn_y + 14.5, "交回终端", 9.0, W::Bold, Color32::WHITE, H::Center);
            if r_term.clicked() {
                decision = Some("terminal");
                launch(row_c.ui.ctx(), &a.key);
            }

            if let Some(dec) = decision {
                state::resolve_agent_approval(&a.key, dec);
            }
        } else if show_btn {
            let (r, b) = paint::capsule(
                row_c,
                r_edge - 66.0,
                y + 1.0,
                64.0,
                20.0,
                &format!("ai-status-page-return-{}", a.key),
                theme::BTN,
                theme::BTN_HOVER,
            );
            b.text(r_edge - 34.0, y + 14.5, "回到 AI", size::TINY, W::Semibold, theme::WHITE, H::Center);
            if r.on_hover_text(format!("回到 {}", a.name)).clicked() {
                launch(row_c.ui.ctx(), &a.key);
            }
        }
    };

    if !cfg!(test) && agents.len() >= 3 {
        let list_x = LEFT - 6.0;
        let list_y = 38.0;
        let list_w = (RIGHT - LEFT) + 16.0;
        let list_h = 200.0;
        let area_rect = c.rect(list_x, list_y, list_w, list_h);
        let mut content = egui::Ui::new(
            c.ui.ctx().clone(),
            egui::Id::new(("ai-agents-list", c.live)),
            egui::UiBuilder::new()
                .max_rect(area_rect)
                .layer_id(c.painter.layer_id())
                .style(c.ui.style().clone()),
        );
        content.set_clip_rect(area_rect.intersect(c.painter.clip_rect()));
        content.set_opacity(c.painter.opacity());
        if !c.live {
            content.disable();
        }
        content.spacing_mut().item_spacing.y = 4.0;
        content.spacing_mut().scroll.floating = true;
        content.spacing_mut().scroll.bar_width = 3.5 * c.s;
        content.spacing_mut().scroll.bar_inner_margin = 2.0 * c.s;
        egui::ScrollArea::vertical()
            .id_salt("ai-status-agents-scroll")
            .max_height(list_h * c.s)
            .auto_shrink([false, false])
            .show(&mut content, |ui| {
                for (idx, a) in agents.iter().enumerate() {
                    let can_approve = cfg.agent_hook_approval && a.phase == state::Phase::Waiting;
                    let row_h = if can_approve { 68.0 } else { 44.0 };
                    let (rect, _) = ui.allocate_exact_size(egui::vec2((RIGHT - LEFT + 8.0) * c.s, row_h * c.s), egui::Sense::hover());
                    let mut row_c = Card::new(ui, rect.min, ui.clip_rect());
                    row_c.s = c.s;
                    row_c.live = c.live;
                    draw_agent_row(&row_c, 6.0, 0.0, a, idx + 1 < agents.len());
                }
            });
        return;
    }

    let any_waiting = !cfg!(test) && cfg.agent_hook_approval && agents.iter().any(|a| a.phase == state::Phase::Waiting);
    let rows = if any_waiting { agents.len().min(2) } else { agents.len().min(3) };
    let mut y_cursor = 42.0_f32;
    for (index, a) in agents.iter().take(rows).enumerate() {
        let can_approve = !cfg!(test) && cfg.agent_hook_approval && a.phase == state::Phase::Waiting;
        draw_agent_row(c, LEFT, y_cursor, a, index + 1 < rows);
        y_cursor += if can_approve { 68.0 } else { 44.0 };
    }
    if cfg!(test) && agents.len() > rows {
        c.text(
            NX,
            44.0 + rows as f32 * 42.0 + 2.0,
            &format!("还有 {} 个助手在忙", agents.len() - rows),
            size::TINY,
            W::Regular,
            theme::GRAY,
            H::Left,
        );
    }
}

/// 「已 3 分 12 秒」；不足一秒显示「刚刚」。
fn elapsed_text(secs: i64) -> String {
    match secs {
        s if s <= 0 => "刚刚开始".to_owned(),
        s if s < 60 => format!("已 {s} 秒"),
        s if s < 3600 => format!("已 {} 分 {:02} 秒", s / 60, s % 60),
        s => format!("已 {} 小时 {} 分", s / 3600, s % 3600 / 60),
    }
}

// 任务结束提示卡片已按需求移除：任务完成只在岛上显示状态文字（`done_timeout_secs`），
// 不再在岛下方弹出「已完成 / 取消 / 回到 AI」的独立卡片。

// ---------------------------------------------------------------
// 文件选择（课表导入）
// ---------------------------------------------------------------

/// 正在等待文件对话框返回的路径；`None` = 没在选。
static PICKED_XLSX: Mutex<Option<String>> = Mutex::new(None);
/// 对话框是否还没结束（避免连点开出好几个窗口）。
static PICKING_XLSX: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 打开系统文件对话框选 .xlsx，**在后台线程里跑**。
///
/// 原来的实现直接在 UI 线程 `Command::output()` 等对话框关掉，
/// 点「浏览」整个界面就冻住几秒（Windows 还会因为 `-STA` 的隐藏窗口卡顿）。
/// 现在只投递一个线程，UI 每帧用 `take_picked_xlsx` 收结果。
pub(crate) fn pick_xlsx_async() {
    use std::sync::atomic::Ordering;
    if PICKING_XLSX.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        let picked = system_pick_xlsx();
        if let Ok(mut slot) = PICKED_XLSX.lock() {
            *slot = Some(picked);
        }
        PICKING_XLSX.store(false, Ordering::SeqCst);
    });
}

/// 还在等对话框时为 true（设置页据此保持重绘，别让界面像卡死）。
pub(crate) fn picking_xlsx() -> bool {
    PICKING_XLSX.load(std::sync::atomic::Ordering::Relaxed)
}

/// 取一次对话框结果：`Some(路径)` 表示选了文件，`Some("")` 表示取消了。
pub(crate) fn take_picked_xlsx() -> Option<String> {
    PICKED_XLSX.lock().ok().and_then(|mut slot| slot.take())
}

static PICKED_TARGET_EXE: Mutex<Option<String>> = Mutex::new(None);
static PICKING_TARGET_EXE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn pick_target_exe_async() {
    use std::sync::atomic::Ordering;
    if PICKING_TARGET_EXE.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        let picked = system_pick_target_exe();
        if let Ok(mut slot) = PICKED_TARGET_EXE.lock() {
            *slot = Some(picked);
        }
        PICKING_TARGET_EXE.store(false, Ordering::SeqCst);
    });
}

pub(crate) fn picking_target_exe() -> bool {
    PICKING_TARGET_EXE.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn take_picked_target_exe() -> Option<String> {
    PICKED_TARGET_EXE.lock().ok().and_then(|mut slot| slot.take())
}

#[cfg(windows)]
mod win_file_dialog {
    #[repr(C)]
    struct OpenFileNameW {
        l_struct_size: u32,
        hwnd_owner: isize,
        h_instance: isize,
        lpstr_filter: *const u16,
        lpstr_custom_filter: *mut u16,
        n_max_cust_filter: u32,
        n_filter_index: u32,
        lpstr_file: *mut u16,
        n_max_file: u32,
        lpstr_file_title: *mut u16,
        n_max_file_title: u32,
        lpstr_initial_dir: *const u16,
        lpstr_title: *const u16,
        flags: u32,
        n_file_offset: u16,
        n_file_extension: u16,
        lpstr_def_ext: *const u16,
        l_cust_data: isize,
        lpfn_hook: isize,
        lp_template_name: *const u16,
        pv_reserved: *mut core::ffi::c_void,
        dw_reserved: u32,
        flags_ex: u32,
    }

    #[link(name = "comdlg32")]
    extern "system" {
        fn GetOpenFileNameW(lpofn: *mut OpenFileNameW) -> i32;
    }

    pub fn open_file_dialog(filter_NullSeparated: &str, title: &str, def_ext: &str) -> Option<String> {
        let filter_w: Vec<u16> = filter_NullSeparated.encode_utf16().collect();
        let title_w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let ext_w: Vec<u16> = def_ext.encode_utf16().chain(std::iter::once(0)).collect();
        let mut file_buf = vec![0u16; 1024];
        let owner = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow().0 as isize };
        // OFN_EXPLORER (0x00080000) | OFN_FILEMUSTEXIST (0x00001000) | OFN_PATHMUSTEXIST (0x00000800) | OFN_NOCHANGEDIR (0x00000008)
        let mut ofn = OpenFileNameW {
            l_struct_size: core::mem::size_of::<OpenFileNameW>() as u32,
            hwnd_owner: owner,
            h_instance: 0,
            lpstr_filter: filter_w.as_ptr(),
            lpstr_custom_filter: core::ptr::null_mut(),
            n_max_cust_filter: 0,
            n_filter_index: 1,
            lpstr_file: file_buf.as_mut_ptr(),
            n_max_file: file_buf.len() as u32,
            lpstr_file_title: core::ptr::null_mut(),
            n_max_file_title: 0,
            lpstr_initial_dir: core::ptr::null(),
            lpstr_title: title_w.as_ptr(),
            flags: 0x0008_0000 | 0x0000_1000 | 0x0000_0800 | 0x0000_0008,
            n_file_offset: 0,
            n_file_extension: 0,
            lpstr_def_ext: ext_w.as_ptr(),
            l_cust_data: 0,
            lpfn_hook: 0,
            lp_template_name: core::ptr::null(),
            pv_reserved: core::ptr::null_mut(),
            dw_reserved: 0,
            flags_ex: 0,
        };
        let ok = unsafe { GetOpenFileNameW(&mut ofn) };
        if ok != 0 {
            let end = file_buf.iter().position(|&c| c == 0).unwrap_or(file_buf.len());
            Some(String::from_utf16_lossy(&file_buf[..end]).trim().to_owned())
        } else {
            None
        }
    }
}

#[cfg(windows)]
fn system_pick_xlsx() -> String {
    // 优先调用原生 Win32 comdlg32::GetOpenFileNameW，瞬时弹出，不依赖 PowerShell 进程
    if let Some(p) = win_file_dialog::open_file_dialog(
        "Excel 课表 (*.xlsx)\0*.xlsx\0所有文件 (*.*)\0*.*\0\0",
        "选择教务系统导出的课表 (.xlsx)",
        "xlsx",
    ) {
        return p;
    }
    String::new()
}

#[cfg(not(windows))]
fn system_pick_xlsx() -> String {
    String::new()
}

#[cfg(windows)]
fn system_pick_target_exe() -> String {
    if let Some(p) = win_file_dialog::open_file_dialog(
        "应用程序与快捷方式 (*.exe;*.lnk;*.bat;*.cmd)\0*.exe;*.lnk;*.bat;*.cmd\0所有文件 (*.*)\0*.*\0\0",
        "选择点击「回到 AI」时要唤醒或打开的程序 / 快捷方式",
        "exe",
    ) {
        return p;
    }
    String::new()
}

#[cfg(not(windows))]
fn system_pick_target_exe() -> String {
    String::new()
}

// ---------------------------------------------------------------
// 设置页
// ---------------------------------------------------------------

/// 设置页次要文字颜色。上游设置页自己会按亮/暗主题切换，
/// 这里跟随 `visuals.dark_mode`，避免硬编码白字在浅色主题下看不见。
fn dim_text(ui: &egui::Ui) -> Color32 {
    if ui.ctx().style().visuals.dark_mode {
        Color32::from_gray(180)
    } else {
        Color32::from_gray(75)
    }
}

/// 设备药丸外观（8 款预设 + 位置）。
///
/// 药丸本质上是蓝牙 / 2.4G 外设的状态展示，因此设置项挂在
/// 「通知 → 蓝牙设备与 Voltide」分组里，而不是「AI 工作状态」页。
/// 外观与位置改动立即写回本 mod 的 JSON，桌面上的药丸下一帧就跟着变。
pub fn draw_pill_settings(ui: &mut egui::Ui) {
    let mut cfg = config::get();
    cfg.pill_style %= 4;
    let before = cfg.clone();
    settings_card(ui, |ui| {
        crate::extras::settings_control_row_right(ui, "设备胶囊视觉方案（4 套 UI 可选）", |ui| {
            crate::extras::settings_segmented(ui, &mut cfg.pill_style, &PILL_STYLE_NAMES, 78.0);
        });
        ui.add_space(6.0);
        let avail_w = crate::extras::layout_avail_w(ui).max(120.0);
        let gap = 10.0_f32;
        let per_row: usize = if avail_w >= 400.0 { 2 } else { 1 };
        let card_w = if per_row == 2 {
            ((avail_w - gap) * 0.5).floor().max(160.0)
        } else {
            avail_w
        };
        let card_h = 90.0_f32;
        let rows = (4usize + per_row - 1) / per_row;
        let stage_h = rows as f32 * card_h + (rows as f32 - 1.0) * gap;
        let (stage_r, _) = ui.allocate_exact_size(egui::vec2(avail_w, stage_h), egui::Sense::hover());
        let p_theme = PillTheme::from_skin([255, 255, 255], false);
        let demo_devs = [
            crate::bt::Device {
                id: "DEMO_EARBUDS".into(),
                name: "AirPods Pro".into(),
                kind: crate::bt::DeviceKind::Earbuds,
                battery: Some(92),
            },
            crate::bt::Device {
                id: "VOLTIDE_MOUSE".into(),
                name: "Voltide 鼠标".into(),
                kind: crate::bt::DeviceKind::Mouse,
                battery: Some(76),
            },
            crate::bt::Device {
                id: "DEMO_KB".into(),
                name: "无线键盘".into(),
                kind: crate::bt::DeviceKind::Keyboard,
                battery: Some(64),
            },
            crate::bt::Device {
                id: "DEMO_PAD".into(),
                name: "无线手柄".into(),
                kind: crate::bt::DeviceKind::Gamepad,
                battery: Some(88),
            },
        ];
        let style_descs = [
            "微透玻璃图标圈 · 底部纤细进度条",
            "顺滑电量外弧环 · 居中单行排版",
            "上下分栏对齐 · 连续磨砂胶囊轨",
            "同心 14px 微卡 · 嵌合状态小药丸",
        ];
        let dark = ui.ctx().style().visuals.dark_mode;
        let acc = crate::auralsle_ai_status::plugins::active_accent_color();
        let stage_card = Card::new(ui, stage_r.min, stage_r);
        for (st_id, st_name) in PILL_STYLE_NAMES {
            let idx = st_id as usize;
            let col = idx % per_row;
            let row = idx / per_row;
            let cx = col as f32 * (card_w + gap);
            let cy = row as f32 * (card_h + gap);
            let selected = cfg.pill_style == st_id;
            let hit = stage_card.hit(cx, cy, card_w, card_h, ("pill-style-prev", st_id));
            if hit.clicked() {
                cfg.pill_style = st_id;
            }
            let hovered = hit.hovered();
            let card_bg = if dark {
                if selected {
                    Color32::from_rgb(0x2B, 0x33, 0x42)
                } else if hovered {
                    Color32::from_rgb(0x2B, 0x2C, 0x31)
                } else {
                    Color32::from_rgb(0x23, 0x24, 0x28)
                }
            } else if selected {
                Color32::from_rgb(0xEA, 0xF1, 0xFD)
            } else if hovered {
                Color32::from_rgb(0xF5, 0xF6, 0xFA)
            } else {
                Color32::from_rgb(0xFA, 0xFB, 0xFE)
            };
            let card_stroke = if selected {
                acc
            } else if dark {
                if hovered { Color32::from_gray(96) } else { Color32::from_rgb(0x36, 0x37, 0x3F) }
            } else if hovered {
                Color32::from_rgb(180, 188, 204)
            } else {
                Color32::from_rgb(220, 224, 232)
            };
            let title_col = if dark { Color32::WHITE } else { Color32::from_rgb(24, 26, 32) };
            let sub_col = if dark { Color32::from_gray(165) } else { Color32::from_gray(95) };
            let hint_col = if dark { Color32::from_gray(160) } else { Color32::from_gray(105) };
            stage_card.fill(cx, cy, card_w, card_h, 8.0, card_bg);
            stage_card.stroke(cx, cy, card_w, card_h, 8.0, if selected { 1.5 } else { 1.0 }, card_stroke);

            // 头部样式名 + 统一胶囊徽标 + 描述
            stage_card.text(cx + 12.0, cy + 20.0, st_name, 12.0, W::Bold, title_col, H::Left);
            let tag_w = 44.0_f32;
            let tag_x = cx + card_w - tag_w - 10.0;
            let tag_bg = if selected {
                acc
            } else if dark {
                Color32::from_rgb(0x33, 0x35, 0x3C)
            } else {
                Color32::from_rgb(0xE8, 0xEB, 0xF2)
            };
            let tag_fg = if selected { Color32::WHITE } else { hint_col };
            stage_card.fill(tag_x, cy + 8.5, tag_w, 16.0, 8.0, tag_bg);
            let tag_txt = if selected { "当前".to_owned() } else { format!("方案{}", idx + 1) };
            stage_card.text(
                tag_x + tag_w * 0.5,
                cy + 19.5,
                &tag_txt,
                9.0,
                if selected { W::Bold } else { W::Regular },
                tag_fg,
                H::Center,
            );
            let desc = style_descs[idx];
            let fit_desc = stage_card.fit(desc, 10.0, W::Regular, (card_w - 24.0).max(80.0));
            stage_card.text(cx + 12.0, cy + 36.0, &fit_desc, 10.0, W::Regular, sub_col, H::Left);

            // 下半部真实桌面胶囊预览（Fluent 居中舞台槽）
            let pill_w = (card_w - 24.0).min(132.0).max(96.0);
            let px = cx + (card_w - pill_w) * 0.5;
            let py = cy + 48.0;
            let pr = pill_style_corner_radius(st_id, DEVICE_PILL_STD_H);
            stage_card.fill(px, py, pill_w, DEVICE_PILL_STD_H, pr, Color32::from_rgba_unmultiplied(20, 23, 30, 235));
            stage_card.vgrad(
                px + 1.0,
                py + 1.0,
                pill_w - 2.0,
                DEVICE_PILL_STD_H * 0.55,
                (pr - 1.0).max(4.0),
                Color32::from_white_alpha(14),
                Color32::TRANSPARENT,
            );
            let tpl = DevicePill {
                theme: p_theme,
                style: st_id,
                icon_tint: Color32::WHITE,
            };
            let place = PillPlacement {
                x: px,
                y: py,
                w: pill_w,
                h: DEVICE_PILL_STD_H,
                fade: 1.0,
                rect: stage_card.rect(px, py, pill_w, DEVICE_PILL_STD_H),
                overflow: 0,
            };
            tpl.draw(&stage_card, &place, &demo_devs[idx]);
        }
        crate::extras::twinkle_card_break(ui);
        crate::extras::settings_control_row_right(ui, "显示位置", |ui| {
            crate::extras::settings_segmented(ui, &mut cfg.pill_placement, &PILL_PLACEMENT_NAMES, 78.0);
        });
        crate::extras::settings_hint(
            ui,
            "底色与圆角自动跟随当前岛体皮肤与实时背景模糊；自动模式下与主岛、课程岛保持统一 8px 间距，空间不足时智能换边。",
        );
    });
    if cfg != before {
        config::update(|c| *c = cfg);
    }
}

/// 设置页中独立的一项。沿用上游 `settings_label / settings_card / setting_row`，
/// 配置写入本 mod 自己的 JSON，不碰 `settings.txt`。
pub fn draw_settings(ui: &mut egui::Ui) {
    let mut cfg = config::get();
    let before = cfg.clone();
    // [全页等宽] 直接采用 ScrollArea 规范可用宽度，不再从包含滚动条排水沟的 clip_rect() 推算。
    let real_w = crate::extras::layout_avail_w(ui).min(ui.available_width()).max(160.0);
    ui.set_max_width(real_w);

    settings_label(ui, "显示与 AI 状态灯");
    settings_card(ui, |ui| {
        setting_row(ui, "启用 AI 工作状态", "读取状态文件，在主界面与「AI 工作」页显示当前状态", &mut cfg.enabled);
        crate::extras::twinkle_card_break(ui);
        setting_row(ui, "主界面常驻", "收起时在时间右侧显示「AI 图标 + 工作状态」，图标大小与间距跟额度图标一致", &mut cfg.compact);
        crate::extras::twinkle_card_break(ui);
        setting_row(
            ui,
            "AgentAI 呼吸状态灯",
            "通过 Codex / Claude Code / Gemini Antigravity / 豆包 / WorkBuddy / DeepSeek 生命周期与进程检测在岛上显示呼吸状态灯（运行蓝灯 / 待审批闪烁琥珀灯 / 完成绿灯）",
            &mut cfg.agent_status_lamp,
        );
        crate::extras::twinkle_card_break(ui);
        setting_row(
            ui,
            "岛内直接审批 Agent 命令与文件操作 (Ctrl+Alt+Y / N / T)",
            "当 AI Agent 请求执行命令或修改文件时，在岛上直接点击「允许 / 拒绝 / 终端」或按全局快捷键 Ctrl+Alt+Y（允许）、Ctrl+Alt+N（拒绝）、Ctrl+Alt+T（交回终端）",
            &mut cfg.agent_hook_approval,
        );
    });

    // [第 10 批 · 图八] 标题只留功能名，括注里的四个工具名已在下文说明里出现，删掉避免标题撑成两行。
    settings_label(ui, "AI 工作状态检测源（两套方案共用）");
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "Claude Code 多会话实时探测 (~/.claude/projects)",
            "读取 Claude Code 本机 JSONL 会话流与生命周期 Hook，实时提取项目名、任务标题、模型档位与等待确认状态",
            &mut cfg.fly_claude_sessions,
        );
        crate::extras::twinkle_card_break(ui);
        setting_row(
            ui,
            "OpenAI Codex 多会话实时探测 (~/.codex/sessions)",
            "读取 Codex CLI 与桌面端本机 JSONL 会话流与生命周期 Hook，同步推理进度与命令确认",
            &mut cfg.fly_codex_sessions,
        );
        crate::extras::twinkle_card_break(ui);
        setting_row(
            ui,
            "DeepSeek Harness (DSH) 会话实时探测 (~/.dsh/sessions)",
            "读取 DSH 终端会话状态与生命周期 Hook",
            &mut cfg.fly_dsh_sessions,
        );
        crate::extras::twinkle_card_break(ui);
        setting_row(
            ui,
            "豆包 / 豆包工作实时探测 (%LOCALAPPDATA%\\Doubao)",
            "实时读取豆包与豆包工作桌面端日志流（saman_frontend / agent_infra），精准检测思考、工具执行、命令确认与任务完成状态",
            &mut cfg.fly_doubao_sessions,
        );
        crate::extras::twinkle_card_break(ui);
        setting_row(
            ui,
            "Gemini Antigravity 实时探测 (~/.gemini/antigravity)",
            "读取 Antigravity 本机对话轨迹（transcript.jsonl），精准检测模型思考、工具调用、等待确认与完成状态",
            &mut cfg.fly_antigravity_sessions,
        );
        crate::extras::twinkle_card_break(ui);
        setting_row(
            ui,
            "WorkBuddy / CodeBuddy 实时探测 (~/.workbuddy/projects)",
            "读取 WorkBuddy 本机会话 JSONL 流，实时追踪函数调用、思考与任务完成",
            &mut cfg.fly_workbuddy_sessions,
        );
        crate::extras::twinkle_card_break(ui);
        setting_row(
            ui,
            "Cursor Agent 实时探测 (~/.cursor/projects)",
            "读取 Cursor Agent 轨迹（agent-transcripts/*.jsonl），同步编码、工具调用与完成状态",
            &mut cfg.fly_cursor_sessions,
        );
        crate::extras::twinkle_card_break(ui);
        setting_row(
            ui,
            "Grok Bot 实时探测 (%APPDATA%\\Grok Bot / ~/.grokbot)",
            "读取 Grok Bot 本机会话与结构化日志流，过滤心跳噪声并检测 Agent 运行与完成",
            &mut cfg.fly_grokbot_sessions,
        );
        crate::extras::twinkle_card_break(ui);
        setting_row(
            ui,
            "Trae / Trae CN 实时探测 (%APPDATA%\\Trae / ~/.trae)",
            "读取 Trae / Trae CN 的 SOLO 与 Builder 会话及日志流，检测工具执行、等待确认与完成",
            &mut cfg.fly_trae_sessions,
        );
        crate::extras::twinkle_card_break(ui);
        setting_row(
            ui,
            "Muse / MuseWork 实时探测 (~/.muse / %APPDATA%\\Muse)",
            "读取 Muse / Muse Code 本机会话 JSONL 与运行日志，实时同步 Agent 状态",
            &mut cfg.fly_muse_sessions,
        );
        crate::extras::twinkle_card_break(ui);
        setting_row(
            ui,
            "Dot / Dots 实时探测 (~/.dot / %APPDATA%\\Dot)",
            "读取 Dot / Dots Agent 本机会话与日志流，实时同步任务状态与命令确认",
            &mut cfg.fly_dot_sessions,
        );
        crate::extras::twinkle_card_break(ui);
        ui.horizontal_wrapped(|ui| {
            if ui.button("恢复所有已归档会话").clicked() {
                state::clear_archived_sessions();
            }
        });
    });

    settings_label(ui, "AI Agent 生命周期钩子管理");
    settings_card(ui, |ui| {
        ui.label(
            egui::RichText::new("一键安装或修复各 AI 编程助手的生命周期与权限审批钩子脚本，实时将任务运行、完成与命令审批同步到灵动岛。")
                .size(11.5)
                .color(dim_text(ui)),
        );
        ui.horizontal_wrapped(|ui| {
            let install_btn = egui::Button::new(
                egui::RichText::new("⚡ 一键安装 / 修复生命周期钩子")
                    .size(11.5)
                    .color(Color32::WHITE),
            )
            .fill(Color32::from_rgb(10, 132, 255))
            .corner_radius(6.0);
            if ui.add(install_btn).clicked() {
                state::install_or_repair_agent_hooks();
            }

        });
        ui.label(
            egui::RichText::new(state::hook_action_status())
                .size(11.0)
                .color(Color32::from_rgb(56, 189, 248)),
        );
    });

    settings_label(ui, "吉祥物表情联动");
    settings_card(ui, |ui| {
        setting_row(
            ui,
            "AI 工作时联动吉祥物表情",
            "思考、调用工具或生成代码时自动切换「处理中」动态表情",
            &mut cfg.ai_mascot_link,
        );
        if cfg.ai_mascot_link {
            crate::extras::twinkle_card_break(ui);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("表情切换策略：").size(11.5));
                for (m_val, m_label) in [
                    (0u8, "↻ 随机轮播 19 个「处理中」表情"),
                    (1u8, "按 AI 动作智能匹配 + 随机轮播"),
                ] {
                    let sel = cfg.ai_mascot_mode == m_val;
                    let btn = egui::Button::new(
                        egui::RichText::new(m_label)
                            .size(11.0)
                            .color(if sel { Color32::WHITE } else { Color32::from_gray(195) }),
                    )
                    .fill(if sel {
                        Color32::from_rgb(10, 132, 255)
                    } else {
                        Color32::from_rgb(51, 51, 51)
                    })
                    .corner_radius(6.0);
                    if ui.add(btn).clicked() {
                        cfg.ai_mascot_mode = m_val;
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("切换间隔：").size(11.5));
                ui.add(egui::DragValue::new(&mut cfg.ai_mascot_interval).range(2..=15).suffix(" 秒"));
            });
        }
    });

    settings_label(ui, "回到 AI");
    settings_card(ui, |ui| {
        if let Some(picked) = take_picked_target_exe() {
            if !picked.is_empty() {
                cfg.open_target = picked;
            }
        }
        if picking_target_exe() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
        }
        setting_row(ui, "显示「回到 AI」按钮", "任务完成 / 等待回复 / 出错时，在状态旁提供一键返回对应 AI 工具", &mut cfg.return_button);
        crate::extras::twinkle_card_break(ui);
        ui.label(egui::RichText::new("按钮目标：http(s) 网址或本地 exe / lnk 路径").size(12.0).strong());
        ui.horizontal_wrapped(|ui| {
            let input_w = (crate::extras::layout_avail_w(ui) - 210.0).min(ui.available_width()).max(120.0);
            ui.add_sized(
                egui::vec2(input_w, 24.0),
                egui::TextEdit::singleline(&mut cfg.open_target)
                    .hint_text("留空 = 自动探测并唤起本机 AI 客户端，或填入 exe/lnk/网址"),
            );
            if ui.button("选择程序…").on_hover_text("从本机选择 .exe 程序或桌面 .lnk 快捷方式").clicked() {
                pick_target_exe_async();
            }
            if ui.button("立即测试").on_hover_text("立即按当前填写的目标执行一次「回到 AI」跳转").clicked() {
                config::update(|c| c.open_target = cfg.open_target.clone());
                let active_key = state::snapshot()
                    .0
                    .first()
                    .map(|a| a.key.clone())
                    .unwrap_or_else(|| "doubao".to_owned());
                launch(ui.ctx(), &active_key);
            }
            if !cfg.open_target.is_empty() && ui.small_button("清空").clicked() {
                cfg.open_target.clear();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("快捷填入：").size(11.0).color(dim_text(ui)));
            for (label, target) in [
                ("自动探测本机客户端 (默认)", ""),
                ("Claude 网页", "https://claude.ai/new"),
                ("ChatGPT 网页", "https://chatgpt.com/"),
                ("DeepSeek 网页", "https://chat.deepseek.com/"),
                ("Gemini 网页", "https://gemini.google.com/app"),
                ("Kimi 网页", "https://kimi.moonshot.cn/"),
                ("Cursor 编辑器", "cursor://"),
                ("VS Code", "vscode://"),
            ] {
                let active = cfg.open_target.trim() == target;
                if ui.selectable_label(active, egui::RichText::new(label).size(11.0)).clicked() {
                    cfg.open_target = target.to_owned();
                }
            }
        });
        ui.add_space(4.0);
        let note_frame_w = crate::extras::layout_avail_w(ui).min(ui.available_width()).max(160.0);
        ui.allocate_ui_with_layout(
            egui::vec2(note_frame_w, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_width(note_frame_w);
                ui.set_max_width(note_frame_w);
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(10, 132, 255, 16))
                    .stroke(egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(10, 132, 255, 52)))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(12, 9))
                    .show(ui, |ui| {
                        let inner_nw = (note_frame_w - 26.0).max(120.0);
                        ui.set_width(inner_nw);
                        ui.set_max_width(inner_nw);
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.label(egui::RichText::new("功能作用与使用说明").size(11.5).strong().color(Color32::from_rgb(94, 178, 255)));
                        for line in [
                            "1. 触发位置：当岛内「AI 工作」页或任务结束弹窗出现「回到 AI」按钮（或直接点击岛内 AI 任务行）时触发。",
                            "2. 留空（推荐）：像微信未读消息一样自动在后台匹配当前 AI 客户端进程（如 Claude / Cursor / WorkBuddy / Codex），已运行则直接切到最前台（最小化自动还原），未运行则从本机安装目录或开始菜单启动。",
                            "3. 指定本地程序：点击上方「选择程序…」选取任意 .exe 或桌面 .lnk 快捷方式（支持带双引号路径与 %LOCALAPPDATA% 环境变量），点击「回到 AI」即固定唤醒该程序。",
                            "4. 指定网页或协议：填入 https://... 网址或 cursor:// 等协议后，点击「回到 AI」将在浏览器或对应 IDE 中直接打开；可随时点「立即测试」验证效果。",
                        ] {
                            ui.label(egui::RichText::new(line).size(11.0).color(dim_text(ui)));
                        }
                    });
            },
        );
    });

    settings_label(ui, "状态来源");
    settings_card(ui, |ui| {
        ui.label(egui::RichText::new("状态文件目录（留空使用默认位置）").size(12.0));
        let dir_w = crate::extras::layout_avail_w(ui).min(ui.available_width()).max(140.0);
        ui.add_sized(egui::vec2(dir_w, 24.0), egui::TextEdit::singleline(&mut cfg.status_dir));
        let default_status = config::install_dir().join("ai-status");
        ui.label(
            egui::RichText::new(format!("默认 {}（软件安装目录）；旧位置 %LOCALAPPDATA%\\ZenDesktop\\ai-status 自动兼容", default_status.display()))
                .size(11.0)
                .color(dim_text(ui)),
        );
        ui.label(
            egui::RichText::new("「AI 工作」页第二行按 step → message → detail → project 取；文件里带 step_index / step_total / progress 时还会显示「第几步」与进度条")
                .size(11.0)
                .color(dim_text(ui)),
        );
        ui.horizontal(|ui| {
            let dir = config::tools_dir();
            ui.label(
                egui::RichText::new("MCP 模板与钩子脚本保存在软件安装目录")
                    .size(11.0)
                    .color(dim_text(ui)),
            ).on_hover_text(dir.display().to_string());
            if ui.small_button("打开").on_hover_text(dir.display().to_string()).clicked() {
                let _ = std::process::Command::new("explorer").arg(&dir).spawn();
            }
        });
        crate::extras::twinkle_card_break(ui);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("静默多久算掉线").size(12.0));
            ui.add(egui::DragValue::new(&mut cfg.active_timeout_secs).range(0..=86400).speed(30.0).suffix(" 秒"));
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("「已完成」保留").size(12.0));
            ui.add(egui::DragValue::new(&mut cfg.done_timeout_secs).range(5..=600).speed(5.0).suffix(" 秒"));
        });
        ui.label(
            egui::RichText::new("超过这个时间没有新上报，只把状态文字颜色调暗（仍然显示「工作中」），不会再误报「已完成」；0 = 不标记")
                .size(11.0)
                .color(dim_text(ui)),
        );
        ui.label(
            egui::RichText::new("「已完成」只在 AI 工具自己上报 done 时出现")
                .size(11.0)
                .color(dim_text(ui)),
        );
    });



    if cfg != before {
        config::update(|c| *c = cfg);
    }
    // [ai-status-mod] 表情设置页已删除，默认使用 islo 表情。
}

// ---------------------------------------------------------------
// 设置页：系统监控（参考 exelban/stats）
// ---------------------------------------------------------------

fn meter_bar(ui: &mut egui::Ui, ratio: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(40.0), 6.0),
        egui::Sense::hover(),
    );
    let p = ui.painter();
    p.rect_filled(rect, 3.0, Color32::from_white_alpha(18));
    let mut fill = rect;
    fill.set_width(rect.width() * ratio.clamp(0.0, 1.0));
    if fill.width() > 1.0 {
        p.rect_filled(fill, 3.0, color);
    }
}

fn spark_line(ui: &mut egui::Ui, values: &[f32], max: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(40.0), 30.0),
        egui::Sense::hover(),
    );
    let p = ui.painter();
    p.rect_filled(rect, 6.0, Color32::from_black_alpha(30));
    if values.len() < 2 {
        return;
    }
    let max = max.max(1.0);
    let step = rect.width() / (values.len() - 1) as f32;
    let pts: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let y = rect.bottom() - 3.0 - (v / max).clamp(0.0, 1.0) * (rect.height() - 6.0);
            egui::pos2(rect.left() + i as f32 * step, y)
        })
        .collect();
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.4, color)));
}

/// 设置页「系统监控」：LiteMonitor 风格的卡片网格。
///
/// 显示哪些卡片、采样多快都来自「功能」页的自定义项；这里只负责画。
/// [ai-status-mod] 第二十二轮：整页实时卡片和岛上「系统监控」页重复，已停用。
/// 保留实现供以后需要画廊视图时取用；当前设置页见下方 draw_sysmon_settings。
#[allow(dead_code)]
fn draw_sysmon_full_gallery(ui: &mut egui::Ui) {
    use crate::auralsle_ai_status::sysmon;
    let cfg = config::get();
    let interval = cfg.monitor_interval.clamp(250, 3000);
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(interval as u64 / 2));

    ui.label(
        egui::RichText::new(
            "Diorser/LiteMonitor 移植：CPU / GPU / 内存 / 磁盘 / 网络 / 电池，采样间隔与显示项在「功能」页里调。",
        )
        .size(11.0)
        .color(dim_text(ui)),
    );
    if !cfg.monitor_plugin {
        ui.label(
            egui::RichText::new("插件已停用：到「功能」页把「系统监控」重新打开")
                .size(11.0)
                .color(dim_text(ui)),
        );
        return;
    }

    let s = sysmon::sample();
    let hist = sysmon::history();
    let cpu_hist: Vec<f32> = hist.iter().map(|h| h.cpu).collect();
    let ratio_of = |used: u64, total: u64| if total > 0 { used as f32 / total as f32 } else { 0.0 };
    let mem_hist: Vec<f32> = hist
        .iter()
        .map(|h| ratio_of(h.mem_used, h.mem_total) * 100.0)
        .collect();
    let disk_hist: Vec<f32> = hist
        .iter()
        .map(|h| ratio_of(h.disk_used, h.disk_total) * 100.0)
        .collect();
    let net_hist: Vec<f32> = hist
        .iter()
        .map(|h| ((h.net_rx + h.net_tx) / 1024.0 / 1024.0) as f32)
        .collect();
    let gpu_hist: Vec<f32> = hist.iter().filter_map(|h| h.gpu).collect();
    let net_max = net_hist.iter().copied().fold(1.0_f32, f32::max);
    let mem_ratio = ratio_of(s.mem_used, s.mem_total);
    let disk_ratio = ratio_of(s.disk_used, s.disk_total);
    let cores = s.cores.len().max(1);

    let blue = theme::BLUE;
    let green = theme::SOFT_GREEN;
    let amber = Color32::from_rgb(255, 190, 92);
    let purple = Color32::from_rgb(180, 128, 210);

    let mut panels: Vec<Panel> = Vec::new();
    if cfg.monitor_cpu {
        let freq = s.cpu_mhz.map(sysmon::mhz).unwrap_or_else(|| "--".to_owned());
        panels.push(Panel {
            title: "CPU".to_owned(),
            badge: format!("{cores} 核"),
            value: format!("{:.0}%", s.cpu),
            ratio: Some(s.cpu / 100.0),
            color: blue,
            sparks: if cfg.monitor_charts { cpu_hist } else { Vec::new() },
            max: 100.0,
            footer: format!("{freq} · 进程 {}", s.processes),
        });
    }
    if cfg.monitor_mem {
        panels.push(Panel {
            title: "内存".to_owned(),
            badge: format!("/ {}", sysmon::bytes(s.mem_total)),
            value: sysmon::bytes(s.mem_used),
            ratio: Some(mem_ratio),
            color: green,
            sparks: if cfg.monitor_charts { mem_hist } else { Vec::new() },
            max: 100.0,
            footer: format!("占用 {:.0}% · 开机 {}", mem_ratio * 100.0, sysmon::uptime_text(s.uptime)),
        });
    }
    if cfg.monitor_disk {
        panels.push(Panel {
            title: "磁盘".to_owned(),
            badge: format!("/ {}", sysmon::bytes(s.disk_total)),
            value: sysmon::bytes(s.disk_used),
            ratio: Some(disk_ratio),
            color: amber,
            sparks: if cfg.monitor_charts { disk_hist } else { Vec::new() },
            max: 100.0,
            footer: format!(
                "读 {} · 写 {}",
                sysmon::rate(s.disk_read),
                sysmon::rate(s.disk_write)
            ),
        });
    }
    if cfg.monitor_net {
        panels.push(Panel {
            title: "网络".to_owned(),
            badge: format!("↑ {}", sysmon::rate(s.net_tx)),
            value: format!("↓ {}", sysmon::rate(s.net_rx)),
            ratio: None,
            color: theme::CYAN,
            sparks: if cfg.monitor_charts { net_hist } else { Vec::new() },
            max: net_max,
            footer: format!(
                "本次开机 ↓ {} · ↑ {}",
                sysmon::bytes(s.rx_total),
                sysmon::bytes(s.tx_total)
            ),
        });
    }
    if cfg.monitor_gpu {
        let vram = s
            .vram
            .map(sysmon::bytes)
            .unwrap_or_else(|| "--".to_owned());
        panels.push(Panel {
            title: "GPU".to_owned(),
            badge: format!("显存 {vram}"),
            value: s
                .gpu
                .map(|g| format!("{g:.0}%"))
                .unwrap_or_else(|| "--".to_owned()),
            ratio: s.gpu.map(|g| g / 100.0),
            color: purple,
            sparks: if cfg.monitor_charts { gpu_hist } else { Vec::new() },
            max: 100.0,
            footer: if s.gpu.is_some() {
                "PDH GPU Engine 实时".to_owned()
            } else {
                "本机不支持 PDH GPU 计数器".to_owned()
            },
        });
    }
    if cfg.monitor_battery {
        let battery_color = if s.charging {
            theme::GREEN
        } else if s.battery.is_some_and(|p| p <= 20) {
            theme::RED
        } else {
            theme::GRAY
        };
        panels.push(Panel {
            title: "电池".to_owned(),
            badge: if s.charging { "充电中" } else { "" }.to_owned(),
            value: s
                .battery
                .map(|p| format!("{p}%"))
                .unwrap_or_else(|| "--".to_owned()),
            ratio: s.battery.map(|p| p as f32 / 100.0),
            color: battery_color,
            sparks: Vec::new(),
            max: 100.0,
            footer: format!("温度 {}", sysmon::opt(s.temp_c, " ℃", 0)),
        });
    }

    for pair in panels.chunks(2) {
        ui.add_space(8.0);
        ui.columns(2, |cols| {
            for (index, panel) in pair.iter().enumerate() {
                metric_card(
                    &mut cols[index],
                    &panel.title,
                    &panel.badge,
                    &panel.value,
                    panel.ratio,
                    panel.color,
                    &panel.sparks,
                    panel.max,
                    &panel.footer,
                );
            }
        });
    }

    // 每核占用：一行小方柱，核数多的时候按比例缩窄。
    if cfg.monitor_cores && !s.cores.is_empty() {
        ui.add_space(8.0);
        core_strip(ui, &s.cores, blue);
    }

    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(
            "温度取自 ACPI 热区；风扇 / 功耗 / 频率上限需要厂商 SMC 或 LibreHardwareMonitor，Windows 通用 API 拿不到。",
        )
        .size(10.5)
        .color(dim_text(ui)),
    );
}

/// 设置页「吉祥物表情」：islo / Aora Bot 的 20 种表情，色板式网格 + 实时预览。
///
/// 每个格子既是按钮也是缩略图（用 `Card` 直接画在 egui 的分配区里），
/// 选中项用主题蓝描边，和 eIsland 设置页的 accent colour 色板是同一种交互。
pub fn draw_face_settings(ui: &mut egui::Ui) {
    use crate::auralsle_ai_status::islo_face::{self, Face};
    let mut cfg = config::get();
    let before = cfg.clone();

    settings_label(ui, "吉祥物表情");
    settings_card(ui, |ui| {
        ui.checkbox(&mut cfg.islo_face, "使用 islo 表情（关闭则回到灵屿 AuraIsle 原版）");
        ui.label(
            egui::RichText::new("自动 = 随待机 / 专注 / 开心 / 提醒等状态切换；点格子可固定一种表情，重启后保留。")
                .size(10.5)
                .color(dim_text(ui)),
        );
        ui.add_space(4.0);

        let t = ui.input(|i| i.time) as f32;
        let cell = egui::vec2(78.0, 46.0);
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(560.0);
            for (i, face) in Face::ALL.iter().enumerate() {
                let selected = cfg.islo_fixed == i as i64 + 1;
                let (rect, resp) = ui.allocate_exact_size(cell, egui::Sense::click());
                let p = ui.painter_at(rect);
                let bg = if selected {
                    Color32::from_rgb(38, 62, 84)
                } else if resp.hovered() {
                    Color32::from_rgb(51, 51, 51)
                } else {
                    Color32::from_rgb(43, 43, 43)
                };
                p.rect_filled(rect, 7.0, bg);
                if selected {
                    p.rect_stroke(rect, 7.0, egui::Stroke::new(1.2, theme::BLUE), egui::StrokeKind::Inside);
                }
                let card = Card::new(ui, egui::pos2(rect.center().x, rect.min.y + 20.0), rect);
                islo_face::draw_mini(&card, 0.0, 0.0, 0.72, *face, t + i as f32 * 0.7);
                p.text(
                    egui::pos2(rect.center().x, rect.max.y - 9.0),
                    egui::Align2::CENTER_CENTER,
                    face.label(),
                    egui::FontId::proportional(9.0),
                    if selected { theme::WHITE } else { theme::GRAY },
                );
                if resp.clicked() {
                    cfg.islo_fixed = if selected { 0 } else { i as i64 + 1 };
                    cfg.islo_face = true;
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("自动（跟随状态）").clicked() { cfg.islo_fixed = 0; }
            let label = if cfg.islo_fixed > 0 {
                format!("当前固定：{}", Face::from_index(cfg.islo_fixed as usize - 1).label())
            } else {
                "当前：自动".to_owned()
            };
            ui.label(egui::RichText::new(label).size(11.0).color(dim_text(ui)));
        });
        crate::extras::twinkle_card_break(ui);
        ui.label(
            egui::RichText::new(
                "表情词汇适配自 Aora Bot (github.com/sam70361/aora-bot)，Copyright (c) 2026 sam70361，\
                 适用 Aora Bot License 1.1；矢量绘制移植自 islo (github.com/scstc/islo)。",
            )
            .size(9.5)
            .color(dim_text(ui)),
        );
    });

    if cfg != before {
        config::update(|c| *c = cfg);
    }
}

/// 「系统监控」设置页：启用开关 + 显示项 + 采样周期 + 一行实时摘要。
///
/// 原来这里铺满六张实时卡片，与岛上「系统监控」页和「功能」页的系统监控卡片重复；
/// 「功能」页删除后，它的配置控件整体迁到本页，一页管完开关与显示项。
pub fn draw_sysmon_settings(ui: &mut egui::Ui) {
    // [已按需求 10 删除图五 `image_b9e037.jpg` 底部的「⚙ 底层硬件采样频率与多核监控附加项」折叠界面]
    auralsle_ai_status::plugins::draw_win_widgets_settings(ui);
}

/// 一张监控卡片的数据。
struct Panel {
    title: String,
    badge: String,
    value: String,
    ratio: Option<f32>,
    color: Color32,
    sparks: Vec<f32>,
    max: f32,
    footer: String,
}

/// 每个逻辑核心一根小竖条，最高的那根标数值。
fn core_strip(ui: &mut egui::Ui, cores: &[f32], color: Color32) {
    let width = ui.available_width().max(120.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 8.0, Color32::from_white_alpha(10));
    let n = cores.len() as f32;
    let gap = 2.0;
    let bar_w = ((rect.width() - 16.0 - gap * (n - 1.0)) / n).max(2.0);
    for (i, value) in cores.iter().enumerate() {
        let h = (rect.height() - 12.0) * (value / 100.0).clamp(0.0, 1.0) + 2.0;
        let x = rect.left() + 8.0 + i as f32 * (bar_w + gap);
        let bar = egui::Rect::from_min_size(
            egui::pos2(x, rect.bottom() - 5.0 - h),
            egui::vec2(bar_w, h),
        );
        p.rect_filled(bar, 1.5, color.gamma_multiply(0.35 + 0.65 * value / 100.0));
    }
}

/// 一张监控卡片：标题 + 右上角小字 + 大数字 + 进度条 + 折线 + 底部说明。
fn metric_card(
    ui: &mut egui::Ui,
    title: &str,
    badge: &str,
    value: &str,
    ratio: Option<f32>,
    color: Color32,
    spark: &[f32],
    spark_max: f32,
    footer: &str,
) {
    let dark = ui.ctx().style().visuals.dark_mode;
    let fill = if dark {
        Color32::from_rgb(33, 33, 36)
    } else {
        Color32::from_rgb(238, 240, 244)
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(10.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).size(11.0).color(dim_text(ui)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(badge).size(10.0).color(dim_text(ui)));
                });
            });
            ui.label(egui::RichText::new(value).size(22.0).strong());
            if let Some(r) = ratio {
                meter_bar(ui, r, color);
            } else {
                ui.add_space(6.0);
            }
            ui.add_space(2.0);
            spark_line(ui, spark, spark_max, color);
            ui.label(egui::RichText::new(footer).size(9.5).color(dim_text(ui)));
        });
}


// ---------------------------------------------------------------
// [venu] 灵动岛边框光效与 Appearance / Border effects 设置面板
// 移植自 https://github.com/imnakul/venu---Dynamic-Notch-for-Windows
// ---------------------------------------------------------------

pub const BORDER_EFFECT_NAMES: [(u8, &str, &str); 9] = [
    (0, "关闭 (Off)", "Off"),
    (1, "彩虹流光 (Rainbow border)", "Rainbow border"),
    (2, "玻璃反光 (Glass reflection)", "Glass reflection"),
    (3, "色彩扫光 · 左→右 (Colour sweep, left to right)", "Colour sweep, left to right"),
    (4, "色彩扫光 · 右→左 (Colour sweep, right to left)", "Colour sweep, right to left"),
    (5, "色彩往返扫光 (Colour sweep, there and back)", "Colour sweep, there and back"),
    (6, "霓虹灯牌 (Neon sign)", "Neon sign"),
    (7, "极光律动 (Aurora)", "Aurora"),
    (8, "烈焰流火 (Fire)", "Fire"),
];

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(1.0) * 6.0;
    let i = h.floor() as i32;
    let f = h - h.floor();
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    (
        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (b.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

/// 判断当前时刻边框光效是否应当激活（遵循 Plays on 与 Plays 间隔设置，预览时强制激活）。
pub fn border_effect_active(open_progress: f32, now: f64, force_preview: bool) -> Option<(u8, f32)> {
    let cfg = config::get();
    if cfg.border_effect == 0 {
        return None;
    }
    if !force_preview && !auralsle_ai_status::border_preview_active() {
        match cfg.border_plays_on {
            1 if open_progress > 0.35 => return None, // Pill only
            2 if open_progress < 0.15 => return None, // Open panel only
            _ => {}
        }
        let period = match cfg.border_interval {
            1 => 15.0,
            2 => 30.0,
            3 => 60.0,
            _ => 0.0,
        };
        if period > 0.0 {
            let cycle_pos = now.rem_euclid(period);
            let run_dur = 3.6;
            if cycle_pos > run_dur {
                return None;
            }
            let env_fade = ((cycle_pos / 0.45).min(1.0) * ((run_dur - cycle_pos) / 0.55).min(1.0)) as f32;
            return Some((cfg.border_effect, env_fade.clamp(0.0, 1.0)));
        }
    }
    Some((cfg.border_effect, 1.0))
}

/// 计算边框光效在归一化位置 `u ∈ [0, 1]`（0 = 左肩顶，0.5 = 底部中心，1 = 右肩顶）与横坐标 `nx ∈ [0, 1]` 处的颜色。
pub fn evaluate_border_effect_color(effect: u8, u: f32, nx: f32, idx: usize, t: f32, fade: f32) -> Color32 {
    let alpha_scale = fade.clamp(0.0, 1.0);
    match effect {
        // 1. Rainbow border（彩虹流光）
        1 => {
            let hue = (nx * 0.85 + u * 0.35 - t * 0.38).rem_euclid(1.0);
            let (r, g, b) = hsv_to_rgb(hue, 0.88, 1.0);
            Color32::from_rgba_unmultiplied(r, g, b, (235.0 * alpha_scale) as u8)
        }
        // 2. Glass reflection（玻璃反光）
        2 => {
            let pos = (t * 0.52).rem_euclid(1.4) - 0.2;
            let d1 = ((nx - pos).abs() / 0.18).min(1.0);
            let d2 = ((nx - (pos - 0.22)).abs() / 0.10).min(1.0);
            let glint = (1.0 - d1 * d1).powi(2) * 0.85 + (1.0 - d2 * d2).powi(2) * 0.45;
            let base_a = (38.0 + glint * 210.0).clamp(0.0, 255.0) * alpha_scale;
            Color32::from_rgba_unmultiplied(225, 242, 255, base_a as u8)
        }
        // 3. Colour sweep, left to right（左→右色彩扫光）
        3 => {
            let center = (t * 0.55).rem_euclid(1.5) - 0.25;
            let dist = ((nx - center).abs() / 0.28).min(1.0);
            let wave = (1.0 - dist * dist).powi(2);
            let hue = (0.52 + (nx - center) * 0.9 + t * 0.12).rem_euclid(1.0);
            let (r, g, b) = hsv_to_rgb(hue, 0.82, 1.0);
            let a = (24.0 + wave * 225.0).clamp(0.0, 255.0) * alpha_scale;
            Color32::from_rgba_unmultiplied(r, g, b, a as u8)
        }
        // 4. Colour sweep, right to left（右→左色彩扫光）
        4 => {
            let center = 1.25 - (t * 0.55).rem_euclid(1.5);
            let dist = ((nx - center).abs() / 0.28).min(1.0);
            let wave = (1.0 - dist * dist).powi(2);
            let hue = (0.82 + (center - nx) * 0.9 + t * 0.12).rem_euclid(1.0);
            let (r, g, b) = hsv_to_rgb(hue, 0.82, 1.0);
            let a = (24.0 + wave * 225.0).clamp(0.0, 255.0) * alpha_scale;
            Color32::from_rgba_unmultiplied(r, g, b, a as u8)
        }
        // 5. Colour sweep, there and back（色彩往返扫光）
        5 => {
            let center = 0.5 + 0.58 * (t * 1.65).sin();
            let dist = ((nx - center).abs() / 0.30).min(1.0);
            let wave = (1.0 - dist * dist).powi(2);
            let hue = (0.58 + 0.35 * (t * 0.8).cos() + (nx - 0.5) * 0.45).rem_euclid(1.0);
            let (r, g, b) = hsv_to_rgb(hue, 0.85, 1.0);
            let a = (28.0 + wave * 222.0).clamp(0.0, 255.0) * alpha_scale;
            Color32::from_rgba_unmultiplied(r, g, b, a as u8)
        }
        // 6. Neon sign（霓虹灯牌 — 复刻图六右侧电光青霓虹灯管 + 脉冲灯丝）
        6 => {
            let flicker = 0.88 + 0.12 * (t * 6.0 + u * 8.0).sin();
            let tick = if idx % 2 == 0 { 1.0 } else { 0.72 };
            let a = (238.0 * flicker * tick * alpha_scale).clamp(0.0, 255.0) as u8;
            Color32::from_rgba_unmultiplied(18, 242, 255, a)
        }
        // 7. Aurora（极光律动）
        7 => {
            let w = ((u * 4.5 - t * 1.4).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
            let hue = 0.38 + 0.34 * w;
            let (r, g, b) = hsv_to_rgb(hue, 0.84, 0.98);
            let a = (155.0 + 90.0 * ((u * 3.0 + t * 1.9).cos() * 0.5 + 0.5)) * alpha_scale;
            Color32::from_rgba_unmultiplied(r, g, b, a.clamp(0.0, 255.0) as u8)
        }
        // 8. Fire（烈焰流火）
        8 => {
            let flame = ((nx * 9.0 + t * 5.2).sin() * 0.5 + (u * 6.0 - t * 3.8).cos() * 0.5) * 0.5 + 0.5;
            let hue = (0.015 + 0.095 * flame).clamp(0.0, 0.12);
            let (r, g, b) = hsv_to_rgb(hue, 0.95, 1.0);
            let a = (175.0 + 75.0 * flame) * alpha_scale;
            Color32::from_rgba_unmultiplied(r, g, b, a.clamp(0.0, 255.0) as u8)
        }
        _ => Color32::TRANSPARENT,
    }
}

/// 沿任意轮廓点列绘制 Venu 流光边框（含外层柔和辉光带 + 核心亮边 + Neon sign 刻度灯丝）。
pub fn paint_contour_border_effect(
    painter: &egui::Painter,
    pts: &[egui::Pos2],
    top_y: Option<f32>,
    flare_h: f32,
    effect: u8,
    t: f32,
    fade: f32,
) {
    if effect == 0 || pts.len() < 3 || fade <= 0.01 {
        return;
    }
    let bounds = egui::Rect::from_points(pts);
    let span_x = bounds.width().max(1.0);
    let n = pts.len();

    let normal_at = |i: usize| -> egui::Vec2 {
        let seg_n = |a: egui::Pos2, b: egui::Pos2| {
            let d = (b - a).normalized();
            egui::vec2(d.y, -d.x)
        };
        if i == 0 {
            seg_n(pts[0], pts[1])
        } else if i == n - 1 {
            seg_n(pts[n - 2], pts[n - 1])
        } else {
            let p = seg_n(pts[i - 1], pts[i]);
            let nx = seg_n(pts[i], pts[i + 1]);
            let denom = (1.0 + p.dot(nx)).max(0.25);
            (p + nx) / denom
        }
    };

    let mut mesh = egui::Mesh::default();
    let glow_out = if effect == 6 { 4.2 } else { 3.2 };
    let core_in = 1.45;

    for i in 0..n {
        let p = pts[i];
        // pts 顺序是 右肩顶(0) -> 右下 -> 左下 -> 左肩顶(n-1)，归一化 u 使左肩顶=0, 底部=0.5, 右肩顶=1.0
        let u = 1.0 - (i as f32 / (n.saturating_sub(1).max(1) as f32));
        let nx = ((p.x - bounds.left()) / span_x).clamp(0.0, 1.0);
        let top_k = if let Some(ty) = top_y {
            let dy = ((p.y - ty) / flare_h.max(4.0)).clamp(0.0, 1.0);
            dy * dy * (3.0 - 2.0 * dy)
        } else {
            1.0
        };
        let col = evaluate_border_effect_color(effect, u, nx, i, t, fade * top_k);
        let clear = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 0);
        let glow_mid = Color32::from_rgba_unmultiplied(
            col.r(),
            col.g(),
            col.b(),
            (col.a() as f32 * if effect == 6 { 0.45 } else { 0.32 }) as u8,
        );
        let norm = normal_at(i);

        let p_out = p + norm * glow_out;
        let p_mid = p + norm * 0.9;
        let p_edge = p;
        let p_in = p - norm * core_in;

        mesh.colored_vertex(p_out, clear);
        mesh.colored_vertex(p_mid, glow_mid);
        mesh.colored_vertex(p_edge, col);
        mesh.colored_vertex(p_in, clear);

        if i > 0 {
            let b0 = ((i - 1) * 4) as u32;
            let b1 = (i * 4) as u32;
            for r in 0..3u32 {
                mesh.add_triangle(b0 + r, b0 + r + 1, b1 + r + 1);
                mesh.add_triangle(b0 + r, b1 + r + 1, b1 + r);
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));

    // Neon sign（图六右侧）：在底部与圆角内侧绘制霓虹灯丝刻度短线
    if effect == 6 {
        let mut tick_Acc = 0.0f32;
        for i in 0..n - 1 {
            let a = pts[i];
            let b = pts[i + 1];
            let len = a.distance(b);
            tick_Acc += len;
            if tick_Acc >= 3.2 {
                tick_Acc = 0.0;
                let mid = a + (b - a) * 0.5;
                if top_y.map_or(true, |ty| mid.y > ty + flare_h * 0.55) {
                    let d = (b - a).normalized();
                    let inward = egui::vec2(-d.y, d.x);
                    painter.line_segment(
                        [mid - inward * 0.4, mid + inward * 2.4],
                        egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(165, 250, 255, (185.0 * fade) as u8)),
                    );
                }
            }
        }
    }
}

/// 在设置页渲染 Venu 风格的 `LIVE PREVIEW`（支持 `Pill | Open panel` 切换）与 `BORDER EFFECTS` 完整控制面板。
pub fn draw_venu_border_effects_panel(ui: &mut egui::Ui, skin_fill: Color32, is_pill_shape: bool) {
    let mut cfg = config::get();
    let before = cfg.clone();
    let now = ui.input(|i| i.time);
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));

    let preview_mode_id = egui::Id::new("venu_live_preview_open_mode");
    let mut preview_open = ui.data(|d| d.get_temp::<bool>(preview_mode_id)).unwrap_or(false);

    let pink = Color32::from_rgb(232, 62, 98);
    let btn_idle = Color32::from_rgb(56, 58, 66);

    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("LIVE PREVIEW · 实时预览")
            .size(10.5)
            .color(Color32::from_gray(148)),
    );
    ui.add_space(2.0);

    // [Pill | Open panel] 切换胶囊
    ui.horizontal(|ui| {
        for (is_open, label) in [(false, "Pill · 收起态"), (true, "Open panel · 展开态")] {
            let selected = preview_open == is_open;
            let btn = egui::Button::new(
                egui::RichText::new(label)
                    .size(11.5)
                    .color(if selected { Color32::WHITE } else { Color32::from_gray(210) }),
            )
            .fill(if selected { pink } else { btn_idle })
            .stroke(egui::Stroke::NONE)
            .corner_radius(6.0);
            if ui.add_sized([112.0, 24.0], btn).clicked() {
                preview_open = is_open;
                ui.data_mut(|d| d.insert_temp(preview_mode_id, preview_open));
            }
        }
    });

    ui.add_space(4.0);

    // 预览舞台（复刻图六深蓝夜空桌面背景 + 贴顶灵动岛 + 实时流光边缘）
    let avail_w = ui.available_width().max(260.0);
    let stage_h = if preview_open { 132.0 } else { 108.0 };
    let (stage_rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, stage_h), egui::Sense::hover());
    let sp = ui.painter_at(stage_rect);

    // 渐变夜空桌面壁纸底
    let mut bg_mesh = egui::Mesh::default();
    let top_bg = Color32::from_rgb(43, 43, 43);
    let bot_bg = Color32::from_rgb(12, 14, 24);
    bg_mesh.colored_vertex(stage_rect.left_top(), top_bg);
    bg_mesh.colored_vertex(stage_rect.right_top(), top_bg);
    bg_mesh.colored_vertex(stage_rect.right_bottom(), bot_bg);
    bg_mesh.colored_vertex(stage_rect.left_bottom(), bot_bg);
    bg_mesh.add_triangle(0, 1, 2);
    bg_mesh.add_triangle(0, 2, 3);
    sp.add(egui::Shape::mesh(bg_mesh));
    sp.rect_stroke(
        stage_rect,
        8.0,
        egui::Stroke::new(1.0, Color32::from_white_alpha(20)),
        egui::StrokeKind::Inside,
    );

    // 在舞台顶部居中画出模拟灵动岛（Pill 或 Open panel）
    let notch_w = if preview_open {
        (avail_w * 0.58).clamp(180.0, 250.0)
    } else {
        (avail_w * 0.34).clamp(116.0, 148.0)
    };
    let notch_h = if preview_open { 74.0 } else { 28.0 };
    let top_gap = if is_pill_shape { 10.0 } else { 0.0 };
    let notch_rect = egui::Rect::from_min_size(
        egui::pos2(stage_rect.center().x - notch_w * 0.5, stage_rect.top() + top_gap),
        egui::vec2(notch_w, notch_h),
    );
    let r0 = if is_pill_shape {
        if preview_open { 16.0 } else { 14.0 }
    } else if preview_open {
        14.0
    } else {
        11.0
    };
    let flare = if is_pill_shape { 0.0 } else if preview_open { 10.0 } else { 5.0 };
    let flare_h = if preview_open { 11.0 } else { 7.0 };

    let rows = crate::shape::sections(notch_rect, flare, flare_h, r0);
    let pts: Vec<egui::Pos2> = if is_pill_shape {
        let mut v = Vec::with_capacity(64);
        let c_r = r0.min(notch_h * 0.5);
        let corners = [
            (notch_rect.right() - c_r, notch_rect.top() + c_r, -std::f32::consts::FRAC_PI_2, 0.0),
            (notch_rect.right() - c_r, notch_rect.bottom() - c_r, 0.0, std::f32::consts::FRAC_PI_2),
            (notch_rect.left() + c_r, notch_rect.bottom() - c_r, std::f32::consts::FRAC_PI_2, std::f32::consts::PI),
            (notch_rect.left() + c_r, notch_rect.top() + c_r, std::f32::consts::PI, std::f32::consts::PI * 1.5),
        ];
        for (cx0, cy0, a0, a1) in corners {
            for k in 0..=12 {
                let a = a0 + (a1 - a0) * (k as f32 / 12.0);
                v.push(egui::pos2(cx0 + a.cos() * c_r, cy0 + a.sin() * c_r));
            }
        }
        if let Some(&first) = v.first() {
            v.push(first);
        }
        v
    } else {
        rows.iter()
            .map(|&(y, _, r)| egui::pos2(r, y))
            .chain(rows.iter().rev().map(|&(y, l, _)| egui::pos2(l, y)))
            .collect()
    };

    if is_pill_shape {
        sp.rect_filled(notch_rect, r0, skin_fill);
    } else {
        crate::shape::fill(&sp, notch_rect, flare, flare_h, r0, skin_fill, 1.0);
    }

    // 舞台上实时绘制当前选中的边框光效
    let effect_to_preview = cfg.border_effect;
    if effect_to_preview > 0 {
        paint_contour_border_effect(
            &sp,
            &pts,
            (!is_pill_shape).then_some(notch_rect.top()),
            flare_h,
            effect_to_preview,
            now as f32,
            1.0,
        );
    } else {
        sp.add(egui::Shape::line(
            pts,
            egui::Stroke::new(1.0, Color32::from_white_alpha(48)),
        ));
    }

    // 舞台内灵动岛状态内容（复刻图六：左侧橙色状态点 + 右侧 9:41，展开态显示 CPU / RAM / GPU 指标）
    if !preview_open {
        sp.circle_filled(
            egui::pos2(notch_rect.left() + 14.0, notch_rect.center().y),
            2.6,
            Color32::from_rgb(255, 142, 36),
        );
        sp.text(
            egui::pos2(notch_rect.right() - 14.0, notch_rect.center().y),
            egui::Align2::RIGHT_CENTER,
            "9:41",
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );
    } else {
        sp.circle_filled(
            egui::pos2(notch_rect.left() + 16.0, notch_rect.top() + 14.0),
            2.6,
            Color32::from_rgb(255, 142, 36),
        );
        sp.text(
            egui::pos2(notch_rect.right() - 16.0, notch_rect.top() + 14.0),
            egui::Align2::RIGHT_CENTER,
            "9:41",
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );
        for (i, (k, v)) in [("CPU", "9%"), ("RAM", "78%"), ("GPU", "3%")].iter().enumerate() {
            let x = notch_rect.left() + notch_w * (0.22 + i as f32 * 0.28);
            sp.text(
                egui::pos2(x, notch_rect.top() + 38.0),
                egui::Align2::CENTER_CENTER,
                *k,
                egui::FontId::proportional(9.0),
                Color32::from_gray(155),
            );
            sp.text(
                egui::pos2(x, notch_rect.top() + 54.0),
                egui::Align2::CENTER_CENTER,
                *v,
                egui::FontId::proportional(12.5),
                Color32::WHITE,
            );
        }
    }

    ui.add_space(10.0);
    ui.label(
        egui::RichText::new("BORDER EFFECTS · 边框光效")
            .size(10.5)
            .color(Color32::from_gray(148)),
    );
    ui.add_space(4.0);

    // Effect 行：左侧标签 + 右侧 Preview 按钮 + 光效下拉框
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("光效样式 (Effect)").size(12.5));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let cur_name = BORDER_EFFECT_NAMES
                .iter()
                .find(|(id, _, _)| *id == cfg.border_effect)
                .map(|(_, zh, _)| *zh)
                .unwrap_or("关闭 (Off)");
            let opts: Vec<(u8, &str)> = BORDER_EFFECT_NAMES.iter().map(|(id, zh, _)| (*id, *zh)).collect();
            crate::extras::react_bits_combo(
                ui,
                "venu-border-effect-combo",
                cur_name,
                185.0_f32.min((ui.available_width() - 72.0).max(110.0)),
                &mut cfg.border_effect,
                &opts,
            );
            let prev_btn = egui::Button::new(egui::RichText::new("预览 (Preview)").size(11.5))
                .fill(btn_idle)
                .corner_radius(6.0);
            if ui.add_sized([84.0, 26.0], prev_btn).clicked() {
                if cfg.border_effect == 0 {
                    cfg.border_effect = 1;
                }
                auralsle_ai_status::trigger_border_preview();
            }
        });
    });

    ui.add_space(6.0);
    crate::extras::twinkle_card_break(ui);
    ui.add_space(4.0);

    // Plays on 行
    ui.label(egui::RichText::new("生效范围 (Plays on)").size(12.5));
    ui.label(
        egui::RichText::new("让展开面板保持沉静，或仅在收起胶囊态展示流光。")
            .size(11.0)
            .color(Color32::from_gray(150)),
    );
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        for (val, label) in [
            (0u8, "始终 (Always)"),
            (1u8, "仅收起态 (Pill only)"),
            (2u8, "仅展开态 (Open panel only)"),
        ] {
            let selected = cfg.border_plays_on == val;
            let btn = egui::Button::new(
                egui::RichText::new(label)
                    .size(11.5)
                    .color(if selected { Color32::WHITE } else { Color32::from_gray(210) }),
            )
            .fill(if selected { pink } else { btn_idle })
            .stroke(egui::Stroke::NONE)
            .corner_radius(6.0);
            if ui.add(btn).clicked() {
                cfg.border_plays_on = val;
            }
        }
    });

    ui.add_space(6.0);
    crate::extras::twinkle_card_break(ui);
    ui.add_space(4.0);

    // Plays 间隔行
    ui.label(egui::RichText::new("播放频率 (Plays)").size(12.5));
    ui.label(
        egui::RichText::new("按周期触发一次或持续流转；未触发时完全不占用绘制。")
            .size(11.0)
            .color(Color32::from_gray(150)),
    );
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        for (val, label) in [
            (0u8, "持续常亮"),
            (1u8, "每 15 秒"),
            (2u8, "每 30 秒"),
            (3u8, "每 60 秒"),
        ] {
            let selected = cfg.border_interval == val;
            let btn = egui::Button::new(
                egui::RichText::new(label)
                    .size(11.5)
                    .color(if selected { Color32::WHITE } else { Color32::from_gray(210) }),
            )
            .fill(if selected { pink } else { btn_idle })
            .stroke(egui::Stroke::NONE)
            .corner_radius(6.0);
            if ui.add(btn).clicked() {
                cfg.border_interval = val;
            }
        }
    });

    if cfg != before {
        config::update(|c| *c = cfg);
    }
}

/// 预热 AI 工作状态常用缓存（消除首次切页卡顿）。
pub fn prewarm_icons(_ctx: &egui::Context) {
    let _ = state::snapshot();
}
