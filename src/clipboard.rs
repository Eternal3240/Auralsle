//! 剪贴板记录：最近复制的文字，在置物架页的「剪贴板」里查看、再次复制、固定。
//!
//! - 只记文字；图片和文件不记录。
//! - 最近记录与固定条目都保存在本机 `clipboard.json`。
//! - 密码管理器等标记为「不进剪贴板历史」的内容一律跳过
//!   （`ExcludeClipboardContentFromMonitorProcessing` / `CanIncludeInClipboardHistory = 0`）。
//! - Windows 上用 `AddClipboardFormatListener` 等系统通知，没有轮询；
//!   监听线程只在剪贴板真正变化时醒来。

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

/// 记录最近一次外部程序发生「复制」（WM_CLIPBOARDUPDATE）的毫秒时间戳，供鼠标移入岛体时智能展开剪贴板页。
static LAST_EXTERNAL_COPY_MS: AtomicI64 = AtomicI64::new(0);

fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn mark_external_copy() {
    LAST_EXTERNAL_COPY_MS.store(now_epoch_ms(), Ordering::Relaxed);
}

pub fn has_recent_external_copy(within_ms: i64) -> bool {
    let last = LAST_EXTERNAL_COPY_MS.load(Ordering::Relaxed);
    last > 0 && (now_epoch_ms() - last) <= within_ms
}

pub fn clear_recent_external_copy() {
    LAST_EXTERNAL_COPY_MS.store(0, Ordering::Relaxed);
}

/// 未固定条目的上限；固定的不占这个名额。
pub const MAX_RECENT: usize = 100;
/// 单条最长字符数，超过的不记录（大段日志、整份文件内容）。
pub const MAX_CHARS: usize = 20_000;
pub const MAX_PINNED: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub text: String,
    /// 本地时间 `HH:MM`。
    pub at: String,
    pub pinned: bool,
}

#[derive(Debug, Default)]
pub struct History {
    pub items: Vec<Clip>,
    /// 每次变化 +1，界面据此判断是否需要重绘。
    pub version: u64,
    path: Option<std::path::PathBuf>,
}

impl History {
    /// 新内容放最前；同样的文字只留一条（移到最前、保留固定状态）。
    pub fn push(&mut self, text: &str, at: &str) -> bool {
        if text.trim().is_empty() || text.chars().count() > MAX_CHARS { return false; }
        let pinned = match self.items.iter().position(|c| c.text == text) {
            Some(0) => {
                if !cfg!(test) {
                    self.items[0].at = at.to_string();
                    self.changed();
                    return true;
                }
                return false;
            }
            Some(i) => self.items.remove(i).pinned,
            None => false,
        };
        self.items.insert(0, Clip { text: text.to_string(), at: at.to_string(), pinned });
        let mut recent = 0;
        self.items.retain(|c| c.pinned || { recent += 1; recent <= MAX_RECENT });
        self.changed();
        true
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
            self.changed();
        }
    }

    /// 清空未固定的条目，固定的保留。
    pub fn clear_recent(&mut self) {
        self.items.retain(|c| c.pinned);
        self.changed();
    }

    pub fn toggle_pin(&mut self, index: usize) -> bool {
        let pinned_count = self.items.iter().filter(|c| c.pinned).count();
        let Some(item) = self.items.get_mut(index) else { return false; };
        if !item.pinned && pinned_count >= MAX_PINNED { return false; }
        item.pinned = !item.pinned;
        self.changed();
        true
    }

    fn to_json(&self) -> Vec<u8> {
        let rows: Vec<_> = self.items.iter().map(|c| serde_json::json!({"pinned": c.pinned,"text": c.text, "at": c.at})).collect();
        serde_json::to_vec(&rows).unwrap_or_default()
    }

    pub fn from_json(raw: &[u8]) -> Self {
        let rows: Vec<serde_json::Value> = serde_json::from_slice(raw).unwrap_or_default();
        let items = rows.iter().filter_map(|row| Some(Clip {
            text: row.get("text")?.as_str()?.to_string(),
            at: row.get("at").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            pinned: row.get("pinned").and_then(|v| v.as_bool()).unwrap_or(true),
        })).filter(|c| !c.text.is_empty()).take(MAX_PINNED + MAX_RECENT).collect();
        Self { items, ..Self::default() }
    }

    fn path() -> std::path::PathBuf { crate::model::config_dir().join("clipboard.json") }

    pub fn load() -> Self {
        Self::load_from(Self::path())
    }

    fn load_from(path: std::path::PathBuf) -> Self {
        let mut history = std::fs::read(&path).map(|raw| Self::from_json(&raw)).unwrap_or_default();
        history.path = Some(path);
        history
    }

    fn changed(&mut self) {
        self.version += 1;
        if let Some(path) = &self.path {
            let bytes = self.to_json();
            if cfg!(test) {
                if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
                let _ = crate::model::atomic_write(path, &bytes);
            } else {
                let p = path.clone();
                std::thread::spawn(move || {
                    if let Some(parent) = p.parent() { let _ = std::fs::create_dir_all(parent); }
                    let _ = crate::model::atomic_write(&p, &bytes);
                });
            }
        }
    }
}

/// 界面与监听线程共用。
#[derive(Clone)]
pub struct Shared {
    pub history: Arc<Mutex<History>>,
    pub enabled: Arc<AtomicBool>,
}

impl Shared {
    pub fn new(enabled: bool) -> Self {
        Self { history: Arc::new(Mutex::new(History::load())), enabled: Arc::new(AtomicBool::new(enabled)) }
    }
    pub fn set_enabled(&self, on: bool) { self.enabled.store(on, Ordering::Relaxed); }
    pub fn is_enabled(&self) -> bool { self.enabled.load(Ordering::Relaxed) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, History> {
        self.history.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn reload_from_disk(&self) {
        let mut loaded = History::load();
        {
            let mut cur = self.lock();
            loaded.version = cur.version.saturating_add(1);
            *cur = loaded;
        }
        if let Ok(mut c) = clip_image_cache().lock() {
            c.clear();
        }
    }
    fn record_update(&self, text: &str, from_history: bool) -> bool {
        if !from_history && !text.trim().is_empty() {
            mark_external_copy();
        }
        !from_history && self.record(text)
    }
    pub fn record(&self, text: &str) -> bool {
        if !self.is_enabled() { return false; }
        let at = chrono::Local::now().format("%H:%M").to_string();
        self.lock().push(text, &at)
    }
}

impl Default for Shared {
    fn default() -> Self { Self::new(true) }
}

/// 列表里的一行预览：换行与连续空白压成一个空格。
pub fn preview(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------- Windows 监听

/// 剪贴板监听的持有者；销毁时让监听线程退出并注销系统监听，不等待线程结束。
pub struct Watcher {
    #[cfg(windows)]
    stop: Arc<win::Stop>,
}

#[cfg(windows)]
impl Drop for Watcher {
    fn drop(&mut self) { self.stop.request(); }
}

#[cfg(windows)]
pub fn watch(shared: Shared, repaint: impl Fn() + Send + 'static) -> Option<Watcher> {
    let stop = Arc::new(win::Stop::default());
    let thread_stop = stop.clone();
    std::thread::Builder::new().name("auralsle-clipboard".into()).spawn(move || {
        // 启动失败（极少见）时剪贴板页只是没有新记录，不影响其他功能。
        let _ = win::run(shared, Box::new(repaint), &thread_stop);
    }).ok()?;
    Some(Watcher { stop })
}

#[cfg(not(windows))]
pub fn watch(_shared: Shared, _repaint: impl Fn() + Send + 'static) -> Option<Watcher> { Some(Watcher {}) }

#[cfg(windows)]
const HISTORY_COPY_FORMAT: &str = "Auralsle.ClipboardHistoryCopy";

#[cfg(windows)]
pub fn copy_history(text: &str) -> bool {
    use clipboard_win::{Clipboard, raw, register_format};
    let Some(format) = register_format(HISTORY_COPY_FORMAT) else { return false };
    let Ok(_clipboard) = Clipboard::new_attempts(10) else { return false };
    raw::set_string(text).is_ok() && raw::set_without_clear(format.get(), &[1]).is_ok()
}

#[cfg(windows)]
mod win {
    use super::Shared;
    use std::cell::RefCell;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::System::Ole::CF_UNICODETEXT;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const WM_CLIPBOARDUPDATE: u32 = 0x031D;

    struct State { shared: Shared, repaint: Box<dyn Fn() + Send> }

    /// 监听线程建好消息窗口后登记线程号；持有者销毁时先置位再读线程号，两边总有一方看到对方。
    #[derive(Default)]
    pub struct Stop { requested: std::sync::atomic::AtomicBool, thread: std::sync::atomic::AtomicU32 }
    impl Stop {
        pub fn request(&self) {
            use std::sync::atomic::Ordering::SeqCst;
            self.requested.store(true, SeqCst);
            let thread = self.thread.load(SeqCst);
            if thread != 0 { unsafe { let _ = PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)); } }
        }
        fn register(&self) -> bool {
            use std::sync::atomic::Ordering::SeqCst;
            self.thread.store(unsafe { windows::Win32::System::Threading::GetCurrentThreadId() }, SeqCst);
            !self.requested.load(SeqCst)
        }
    }
    thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_CLIPBOARDUPDATE {
            STATE.with(|state| {
                if let Some(state) = state.borrow().as_ref() {
                    if state.shared.is_enabled() {
                        if let Some((text, from_history)) = read_text(hwnd) {
                            let updated = state.shared.record_update(&text, from_history);
                            if updated || !from_history {
                                (state.repaint)();
                            }
                        } else if let Some((img_token, from_history)) = read_dib_image(hwnd) {
                            let updated = state.shared.record_update(&img_token, from_history);
                            if updated || !from_history {
                                (state.repaint)();
                            }
                        }
                    }
                }
            });
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }

    /// 密码管理器、远程桌面等会给敏感内容加上这些格式，表示不要进入剪贴板历史。
    unsafe fn excluded() -> bool {
        let exclude = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let ignore = RegisterClipboardFormatW(w!("Clipboard Viewer Ignore"));
        if (exclude != 0 && IsClipboardFormatAvailable(exclude).is_ok()) || (ignore != 0 && IsClipboardFormatAvailable(ignore).is_ok()) {
            return true;
        }
        // 值为 DWORD 0 表示不允许进入历史。
        let history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if history == 0 || IsClipboardFormatAvailable(history).is_err() { return false; }
        let Ok(handle) = GetClipboardData(history) else { return true; };
        let memory = HGLOBAL(handle.0);
        let ptr = GlobalLock(memory) as *const u32;
        if ptr.is_null() { return true; }
        let allowed = GlobalSize(memory) >= 4 && *ptr != 0;
        let _ = GlobalUnlock(memory);
        !allowed
    }

    unsafe fn read_text(hwnd: HWND) -> Option<(String, bool)> {
        if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_err() { return None; }
        // 其他程序可能正占用剪贴板，稍等重试几次。
        let mut opened = false;
        for _ in 0..5 {
            if OpenClipboard(hwnd).is_ok() { opened = true; break; }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !opened { return None; }
        let text = (|| {
            if excluded() { return None; }
            let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
            let memory = HGLOBAL(handle.0);
            let ptr = GlobalLock(memory) as *const u16;
            if ptr.is_null() { return None; }
            let max = (GlobalSize(memory) / 2).min(super::MAX_CHARS * 2 + 1);
            let mut len = 0;
            while len < max && *ptr.add(len) != 0 { len += 1; }
            let text = (len <= super::MAX_CHARS * 2).then(|| String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len)));
            let _ = GlobalUnlock(memory);
            let from_history = clipboard_win::register_format(super::HISTORY_COPY_FORMAT)
                .is_some_and(|format| IsClipboardFormatAvailable(format.get()).is_ok());
            text.map(|text| (text, from_history))
        })();
        let _ = CloseClipboard();
        text
    }

    /// [MoFeng2223/BrimDeck 移植] 读取 Windows 剪贴板中的复制图片 (`CF_DIB = 8`) 并生成缩略图缓存与本地 PNG 快照。
    unsafe fn read_dib_image(hwnd: HWND) -> Option<(String, bool)> {
        const CF_DIB_FMT: u32 = 8;
        if IsClipboardFormatAvailable(CF_DIB_FMT).is_err() {
            return None;
        }
        let mut opened = false;
        for _ in 0..5 {
            if OpenClipboard(hwnd).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !opened {
            return None;
        }
        let out = (|| {
            if excluded() {
                return None;
            }
            let from_history = clipboard_win::register_format(super::HISTORY_COPY_FORMAT)
                .is_some_and(|format| IsClipboardFormatAvailable(format.get()).is_ok());
            if from_history {
                return None;
            }
            let handle = GetClipboardData(CF_DIB_FMT).ok()?;
            let memory = HGLOBAL(handle.0);
            let ptr = GlobalLock(memory) as *const u8;
            let sz = GlobalSize(memory);
            if ptr.is_null() || sz < 40 {
                if !ptr.is_null() {
                    let _ = GlobalUnlock(memory);
                }
                return None;
            }
            let slice = std::slice::from_raw_parts(ptr, sz);
            let bi_size = u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as usize;
            let width = i32::from_le_bytes([slice[4], slice[5], slice[6], slice[7]]);
            let height_raw = i32::from_le_bytes([slice[8], slice[9], slice[10], slice[11]]);
            let bpp = u16::from_le_bytes([slice[14], slice[15]]);
            let compression = u32::from_le_bytes([slice[16], slice[17], slice[18], slice[19]]);
            let top_down = height_raw < 0;
            let height = height_raw.unsigned_abs();
            if width <= 0 || width > 4096 || height == 0 || height > 4096 || (bpp != 24 && bpp != 32) {
                let _ = GlobalUnlock(memory);
                return None;
            }
            let w = width as usize;
            let h = height as usize;
            let offset = bi_size + if compression == 3 && bpp == 32 && bi_size == 40 { 12 } else { 0 };
            let stride = ((w * (bpp as usize) + 31) / 32) * 4;
            if offset + stride * h > sz {
                let _ = GlobalUnlock(memory);
                return None;
            }
            let mut rgba = vec![255u8; w * h * 4];
            let mut hash: u64 = 1469598103934665603;
            for y in 0..h {
                let src_y = if top_down { y } else { h - 1 - y };
                let row = &slice[offset + src_y * stride..offset + (src_y + 1) * stride];
                for x in 0..w {
                    let si = x * (bpp as usize / 8);
                    let di = (y * w + x) * 4;
                    let b = row[si];
                    let g = row[si + 1];
                    let r = row[si + 2];
                    let a = if bpp == 32 {
                        let raw_a = row[si + 3];
                        if raw_a == 0 { 255 } else { raw_a }
                    } else {
                        255
                    };
                    rgba[di] = r;
                    rgba[di + 1] = g;
                    rgba[di + 2] = b;
                    rgba[di + 3] = a;
                    if (x + y) % 17 == 0 {
                        hash = (hash ^ (r as u64)).wrapping_mul(1099511628211);
                        hash = (hash ^ (g as u64)).wrapping_mul(1099511628211);
                        hash = (hash ^ (b as u64)).wrapping_mul(1099511628211);
                    }
                }
            }
            let _ = GlobalUnlock(memory);
            let token = super::store_clipboard_rgba_image(w as u32, h as u32, rgba, hash)?;
            Some((token, false))
        })();
        let _ = CloseClipboard();
        out
    }

    pub fn run(shared: Shared, repaint: Box<dyn Fn() + Send>, stop: &Stop) -> windows::core::Result<()> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = w!("AuralsleClipboardWatcher");
            let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), class, PCWSTR::null(), WINDOW_STYLE(0), 0, 0, 0, 0, HWND_MESSAGE, None, instance, None)?;
            // 消息窗口建好后线程已有消息队列，此后投递的 WM_QUIT 不会丢。
            if !stop.register() { let _ = DestroyWindow(hwnd); return Ok(()); }
            STATE.with(|state| *state.borrow_mut() = Some(State { shared, repaint }));
            AddClipboardFormatListener(hwnd)?;
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let _ = RemoveClipboardFormatListener(hwnd);
            let _ = DestroyWindow(hwnd);
            STATE.with(|state| *state.borrow_mut() = None);
        }
        Ok(())
    }
}


// ---------------------------------------------------------------- [MoFeng2223/BrimDeck] 图文剪贴板图片缓存与一键再次粘贴
#[derive(Clone)]
pub struct ClipImagePreview {
    pub width: u32,
    pub height: u32,
    pub thumb: eframe::egui::ColorImage,
    pub png_path: std::path::PathBuf,
}

static CLIP_IMAGES: std::sync::OnceLock<Mutex<std::collections::HashMap<String, ClipImagePreview>>> = std::sync::OnceLock::new();

fn clip_image_cache() -> &'static Mutex<std::collections::HashMap<String, ClipImagePreview>> {
    CLIP_IMAGES.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// 判断剪贴板条目是否为图片记录（格式：`[图片 WxH] <path>`）。
pub fn is_image_clip(text: &str) -> bool {
    text.starts_with("[图片 ") && text.contains("] ")
}

/// 解析图片条目的尺寸与磁盘路径。
pub fn parse_image_clip(text: &str) -> Option<(u32, u32, std::path::PathBuf)> {
    let rest = text.strip_prefix("[图片 ")?;
    let (dim_str, path_str) = rest.split_once("] ")?;
    let (w_s, h_s) = dim_str.split_once('×').or_else(|| dim_str.split_once('x'))?;
    let w = w_s.trim().parse::<u32>().ok()?;
    let h = h_s.trim().parse::<u32>().ok()?;
    let mut path = std::path::PathBuf::from(path_str.trim());
    if let Some(name) = path.file_name() {
        let local = crate::model::config_dir().join("clip_images").join(name);
        if local.exists() {
            path = local;
        }
    }
    Some((w, h, path))
}

/// 将 RGBA 像素保存为本地剪贴板图片并生成内存缩略图，返回条目文本标识。
pub fn store_clipboard_rgba_image(w: u32, h: u32, rgba: Vec<u8>, hash: u64) -> Option<String> {
    let dir = crate::model::config_dir().join("clip_images");
    let _ = std::fs::create_dir_all(&dir);
    let file_path = dir.join(format!("clip_{w}x{h}_{hash:016x}.png"));
    let img_buf = image::RgbaImage::from_raw(w, h, rgba)?;
    if !file_path.exists() {
        let _ = img_buf.save(&file_path);
    }
    let thumb_w = w.min(180);
    let thumb_h = ((h as f32 * (thumb_w as f32 / w.max(1) as f32)).round() as u32).clamp(24, 110);
    let resized = image::imageops::resize(&img_buf, thumb_w, thumb_h, image::imageops::FilterType::Triangle);
    let color_img = eframe::egui::ColorImage::from_rgba_unmultiplied(
        [thumb_w as usize, thumb_h as usize],
        resized.as_raw(),
    );
    let token = format!("[图片 {w}×{h}] {}", file_path.display());
    if let Ok(mut map) = clip_image_cache().lock() {
        map.insert(
            token.clone(),
            ClipImagePreview {
                width: w,
                height: h,
                thumb: color_img,
                png_path: file_path,
            },
        );
    }
    Some(token)
}

static CLIP_IMAGE_PENDING: std::sync::OnceLock<Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();

fn clip_image_pending() -> &'static Mutex<std::collections::HashSet<String>> {
    CLIP_IMAGE_PENDING.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// 获取图片剪贴板条目的缩略图（100% 内存非阻塞读取；若重启后首次访问则触发后台线程异步解码 PNG，绝不阻塞 UI 线程）。
pub fn get_clip_image_preview(text: &str) -> Option<ClipImagePreview> {
    if !is_image_clip(text) {
        return None;
    }
    if let Ok(map) = clip_image_cache().lock() {
        if let Some(found) = map.get(text) {
            return Some(found.clone());
        }
    }
    let (w, h, path) = parse_image_clip(text)?;
    if let Ok(mut pend) = clip_image_pending().lock() {
        if pend.contains(text) {
            return None;
        }
        pend.insert(text.to_owned());
    }
    let key_owned = text.to_owned();
    let _ = std::thread::Builder::new()
        .name("auralsle-clip-thumb".into())
        .spawn(move || {
            if let Ok(img) = image::open(&path) {
                let dyn_img = img.to_rgba8();
                let thumb_w = w.min(180).max(1);
                let thumb_h = ((h as f32 * (thumb_w as f32 / w.max(1) as f32)).round() as u32).clamp(24, 110);
                let resized = image::imageops::resize(&dyn_img, thumb_w, thumb_h, image::imageops::FilterType::Triangle);
                let color_img = eframe::egui::ColorImage::from_rgba_unmultiplied(
                    [thumb_w as usize, thumb_h as usize],
                    resized.as_raw(),
                );
                let prev = ClipImagePreview {
                    width: w,
                    height: h,
                    thumb: color_img,
                    png_path: path,
                };
                if let Ok(mut map) = clip_image_cache().lock() {
                    map.insert(key_owned, prev);
                }
            }
        });
    None
}

/// 生成一张高质感演示图片并存入剪贴板历史（方便用户在无外部截图工具时一键体验 BrimDeck 图片剪贴板卡片与一键粘贴）。
pub fn insert_demo_image_clip(shared: &Shared) -> Option<String> {
    let (w, h) = (320u32, 180u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let seed = (now_epoch_ms() as u64) & 0xFFFF;
    for y in 0..h {
        let v = y as f32 / h as f32;
        for x in 0..w {
            let u = x as f32 / w as f32;
            let idx = ((y * w + x) * 4) as usize;
            let r = (28.0 + u * 165.0 + (v * 6.28).sin() * 24.0).clamp(0.0, 255.0) as u8;
            let g = (42.0 + (1.0 - v) * 145.0 + (u * 6.28).cos() * 22.0).clamp(0.0, 255.0) as u8;
            let b = (118.0 + u * 95.0 + v * 40.0).clamp(0.0, 255.0) as u8;
            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = 255;
        }
    }
    let token = store_clipboard_rgba_image(w, h, rgba, 0xB81ADE0000 ^ seed)?;
    shared.record(&token);
    Some(token)
}

/// [MoFeng2223/BrimDeck 移植] 将文字或图片条目写回系统剪贴板，并在开启 `auto_paste` 时自动向当前活动窗口发送 `Ctrl+V` 直接完成粘贴！
pub fn paste_clip_to_active_window(ctx: &eframe::egui::Context, text: &str, auto_paste: bool) {
    if is_image_clip(text) {
        #[cfg(windows)]
        {
            if let Some((_, _, path)) = parse_image_clip(text) {
                if let Ok(img) = image::open(&path) {
                    let rgba = img.to_rgba8();
                    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
                    let _ = write_rgba_to_windows_clipboard(w, h, rgba.as_raw());
                }
            }
        }
        #[cfg(not(windows))]
        {
            ctx.copy_text(text.to_owned());
        }
    } else {
        #[cfg(windows)]
        {
            if !copy_history(text) {
                ctx.copy_text(text.to_owned());
            }
        }
        #[cfg(not(windows))]
        {
            ctx.copy_text(text.to_owned());
        }
    }
    if auto_paste {
        #[cfg(windows)]
        {
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(65));
                unsafe {
                    use windows::Win32::UI::Input::KeyboardAndMouse::{
                        keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL,
                    };
                    keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
                    keybd_event(b'V', 0, KEYBD_EVENT_FLAGS(0), 0);
                    keybd_event(b'V', 0, KEYEVENTF_KEYUP, 0);
                    keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
                }
            });
        }
    }
}

#[cfg(windows)]
fn write_rgba_to_windows_clipboard(w: usize, h: usize, rgba: &[u8]) -> bool {
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    const CF_DIB_FMT: u32 = 8;
    let dib_size = 40 + w * h * 4;
    unsafe {
        if OpenClipboard(HWND::default()).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, dib_size) else {
            let _ = CloseClipboard();
            return false;
        };
        let ptr = GlobalLock(hmem) as *mut u8;
        if ptr.is_null() {
            let _ = CloseClipboard();
            return false;
        }
        let dst = std::slice::from_raw_parts_mut(ptr, dib_size);
        dst[0..4].copy_from_slice(&(40u32).to_le_bytes());
        dst[4..8].copy_from_slice(&(w as i32).to_le_bytes());
        dst[8..12].copy_from_slice(&(h as i32).to_le_bytes()); // bottom-up DIB
        dst[12..14].copy_from_slice(&(1u16).to_le_bytes());
        dst[14..16].copy_from_slice(&(32u16).to_le_bytes());
        dst[16..20].copy_from_slice(&(0u32).to_le_bytes()); // BI_RGB
        dst[20..24].copy_from_slice(&((w * h * 4) as u32).to_le_bytes());
        for y in 0..h {
            let src_y = h - 1 - y;
            for x in 0..w {
                let si = (src_y * w + x) * 4;
                let di = 40 + (y * w + x) * 4;
                dst[di] = rgba[si + 2];     // B
                dst[di + 1] = rgba[si + 1]; // G
                dst[di + 2] = rgba[si];     // R
                dst[di + 3] = rgba[si + 3]; // A
            }
        }
        let _ = GlobalUnlock(hmem);
        let ok = SetClipboardData(CF_DIB_FMT, HANDLE(hmem.0)).is_ok();
        if let Some(fmt) = clipboard_win::register_format(HISTORY_COPY_FORMAT) {
            let _ = clipboard_win::raw::set_without_clear(fmt.get(), &[1]);
        }
        let _ = CloseClipboard();
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn dropping_the_watcher_ends_the_listener_thread(){
        // 先停后启和启动后再停两种顺序都必须让线程退出，不能留下常驻的监听窗口。
        for stop_first in [true,false] {
            let stop=Arc::new(win::Stop::default());let thread_stop=stop.clone();
            let shared=Shared{history:Default::default(),enabled:Arc::new(AtomicBool::new(false))};
            if stop_first {stop.request();}
            let (done,finished)=std::sync::mpsc::channel();
            std::thread::spawn(move||{let _=win::run(shared,Box::new(||{}),&thread_stop);let _=done.send(());});
            if !stop_first {std::thread::sleep(std::time::Duration::from_millis(200));stop.request();}
            finished.recv_timeout(std::time::Duration::from_secs(3)).expect("监听线程应在停止后退出");
        }
    }

    #[test]
    fn history_copy_preserves_order_but_external_copy_promotes() {
        let shared = Shared { history: Default::default(), enabled: Arc::new(AtomicBool::new(true)) };
        shared.lock().items.clear();
        shared.record("较早的一条");
        shared.record("最近的一条");
        let before = shared.lock().items.clone();
        assert!(!shared.record_update("较早的一条", true));
        assert_eq!(shared.lock().items, before);
        assert!(shared.record_update("较早的一条", false));
        assert_eq!(shared.lock().items[0].text, "较早的一条");
    }
    #[test]
    fn history_dedupes_caps_and_keeps_pins() {
        let mut h = History::default();
        assert!(!h.push("   \n", "09:00"), "空白不记录");
        assert!(h.push("a", "09:00") && h.push("b", "09:01"));
        assert!(!h.push("b", "09:02"), "与最新一条相同不重复记录");
        assert!(h.push("a", "09:03"));
        assert_eq!(h.items.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(h.items[0].at, "09:03");
        h.items[1].pinned = true; // b
        for i in 0..(MAX_RECENT + 5) { h.push(&format!("n{i}"), "10:00"); }
        assert_eq!(h.items.iter().filter(|c| !c.pinned).count(), MAX_RECENT);
        assert!(h.items.iter().any(|c| c.text == "b" && c.pinned), "固定的不会被挤掉");
        h.clear_recent();
        assert_eq!(h.items.len(), 1);
        assert!(!h.push(&"x".repeat(MAX_CHARS + 1), "10:00"), "超长内容不记录");
    }

    #[test]
    fn reopening_restores_history_and_respects_manual_cleanup() {
        let path = std::env::temp_dir().join(format!("auralsle-clip-{}.json", std::process::id()));
        std::fs::write(&path, br#"[{"text":"old","at":"08:00"}]"#).unwrap();
        let mut history = History::load_from(path.clone());
        assert!(history.items[0].pinned);
        history.push("保留", "09:00");
        history.push("删除", "09:01");
        history.remove(0);
        let mut reopened = History::load_from(path.clone());
        assert_eq!(reopened.items, history.items);
        reopened.clear_recent();
        let final_history = History::load_from(path.clone());
        assert_eq!(final_history.items.len(), 1);
        assert_eq!(final_history.items[0].text, "old");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn pinned_round_trip_and_preview() {
        let mut h = History::default();
        h.push("第一行\n第二行  末尾", "08:00");
        h.push("临时", "08:01");
        h.items[1].pinned = true;
        let back = History::from_json(&h.to_json());
        assert_eq!(back.items, h.items);
        assert_eq!(preview(&back.items[1].text), "第一行 第二行 末尾");
        assert!(History::from_json(b"not json").items.is_empty());
    }
}
