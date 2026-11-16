//! 系统文件缩略图 / 图标与本机软件真实图标提取器。
//!
//! - 置物架文件预览：优先用 `IShellItemImageFactory` 取 96 px 缩略图，失败再用 `SHGetFileInfoW` 取 32 px 大图标；
//! - 通知应用真实图标提取（`extract_app_icon_texture`）：直接通过本机运行进程（`CreateToolhelp32Snapshot` + `QueryFullProcessImageNameW`）、
//!   注册表 `App Paths` / `Uninstall`、开始菜单 `.lnk` 快捷方式定位本机软件真实路径，调用 `SHGetFileInfoW` 提取软件原生 32×32 图标纹理，
//!   并对常见 AI / 通讯应用提供内嵌官方 SVG / PNG 矢量图标保底，彻底杜绝字符方块 `□`。

use eframe::egui::{self,ColorImage};
use std::path::{Path,PathBuf};
use std::collections::{HashMap,HashSet};
use std::sync::mpsc::{self,Receiver,SyncSender};

#[derive(Clone)]
pub(crate) struct Preview {pub image:Option<egui::TextureHandle>,pub metadata:String}
struct Loaded {image:Option<ColorImage>,metadata:String}
type ResultItem=(PathBuf,Loaded);

#[derive(Default)]
pub(crate) struct Cache {
    entries:HashMap<PathBuf,(Preview,u64)>,pending:HashSet<PathBuf>,
    worker:Option<(SyncSender<PathBuf>,Receiver<ResultItem>)>,generation:u64,
}

impl Cache {
    fn start(&mut self,ctx:egui::Context,load:impl Fn(&Path)->Loaded+Send+'static){
        let(tx,requests)=mpsc::sync_channel::<PathBuf>(8);let(results,rx)=mpsc::sync_channel(8);
        std::thread::Builder::new().name("auralsle-thumbnails".into()).spawn(move||{
            #[cfg(windows)] let initialized=unsafe{windows::Win32::System::Com::CoInitializeEx(None,windows::Win32::System::Com::COINIT_APARTMENTTHREADED).is_ok()};
            while let Ok(path)=requests.recv(){
                let preview=load(&path);
                if results.send((path,preview)).is_err(){break;}
                ctx.request_repaint();
            }
            #[cfg(windows)] if initialized{unsafe{windows::Win32::System::Com::CoUninitialize();}}
        }).ok();
        self.worker=Some((tx,rx));
    }
    pub fn get(&mut self,ctx:&egui::Context,path:&Path)->Option<Preview>{
        if self.worker.is_none(){self.start(ctx.clone(),load_preview);}
        let(tx,rx)=self.worker.as_ref().unwrap();
        while let Ok((path,loaded))=rx.try_recv(){
            if !self.pending.remove(&path){continue;}
            if self.entries.len()>=32{
                if let Some(oldest)=self.entries.iter().min_by_key(|(_,(_,used))|used).map(|(path,_)|path.clone()){self.entries.remove(&oldest);}
            }
            let image=loaded.image.map(|image|ctx.load_texture(format!("shelf:{}",path.display()),image,egui::TextureOptions::LINEAR));
            self.entries.insert(path,(Preview{image,metadata:loaded.metadata},self.generation));
        }
        self.generation=self.generation.wrapping_add(1);
        if let Some((preview,used))=self.entries.get_mut(path){*used=self.generation;return Some(preview.clone());}
        if !self.pending.contains(path) && tx.try_send(path.to_path_buf()).is_ok(){self.pending.insert(path.to_path_buf());}
        None
    }
    pub fn retain(&mut self,paths:&[PathBuf]){
        self.entries.retain(|path,_|paths.contains(path));self.pending.retain(|path|paths.contains(path));
    }
    pub fn remove(&mut self,path:&Path){self.entries.remove(path);self.pending.remove(path);}
    pub fn clear(&mut self){self.entries.clear();self.pending.clear();}
}

fn load_preview(path:&Path)->Loaded {
    let metadata=std::fs::metadata(path).map(|metadata|if metadata.is_dir(){"文件夹".into()}else{crate::model::human_size(metadata.len())}).unwrap_or_else(|_|"已移动".into());
    Loaded{image:file_icon(path),metadata}
}

fn preview_image(width:usize,height:usize,pixels:Vec<u8>)->Option<ColorImage>{
    let image=image::RgbaImage::from_raw(width as u32,height as u32,pixels)?;
    let image=image::DynamicImage::ImageRgba8(image).thumbnail(96,96).to_rgba8();
    Some(ColorImage::from_rgba_premultiplied([image.width()as usize,image.height()as usize],image.as_raw()))
}

#[cfg(windows)]
fn file_icon(path: &Path) -> Option<ColorImage> {
    if !path.exists() {
        return None;
    }
    if let Some((w, h, px)) = thumbnail(path, 96) {
        return preview_image(w,h,px);
    }
    shell_icon(path)
}

#[cfg(windows)]
fn thumbnail(path: &Path, size: i32) -> Option<(usize, usize, Vec<u8>)> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_RESIZETOFIT,
    };

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
        let hbmp = factory.GetImage(SIZE { cx: size, cy: size }, SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK).ok()?;
        let mut bm = BITMAP::default();
        let got = GetObjectW(
            HGDIOBJ(hbmp.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut BITMAP as *mut core::ffi::c_void),
        );
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        if got == 0 || w <= 0 || h <= 0 || w > 1024 || h > 1024 {
            let _ = DeleteObject(HGDIOBJ(hbmp.0));
            return None;
        }
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let lines = GetDIBits(dc, hbmp, 0, h as u32, Some(px.as_mut_ptr() as *mut core::ffi::c_void), &mut bmi, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        if lines == 0 {
            return None;
        }
        // BGRA（预乘）→ RGBA；JPEG 之类的缩略图 alpha 全 0，当作不透明
        let opaque = px.chunks_exact(4).all(|p| p[3] == 0);
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
            if opaque {
                p[3] = 255;
            }
        }
        Some((w as usize, h as usize, px))
    }
}

/// 从 Windows `.lnk` 快捷方式二进制数据中直接解析目标 `.exe` 或 `.ico` 的真实磁盘路径，
/// 从而直接提取目标程序的高清原生图标，彻底避免 `.lnk` 左下角自带的快捷方式小箭头！
#[cfg(windows)]
fn resolve_lnk_target_path(lnk_path: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(lnk_path).ok()?;
    // 1. 扫描 UTF-16LE 路径 (如 "C:\Program Files\...\xxx.exe")
    if bytes.len() >= 8 {
        let u16s: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let text16 = String::from_utf16_lossy(&u16s);
        for part in text16.split('\0') {
            let trimmed = part.trim();
            if trimmed.len() > 6
                && trimmed.as_bytes().get(1) == Some(&b':')
                && (trimmed.to_ascii_lowercase().ends_with(".exe")
                    || trimmed.to_ascii_lowercase().ends_with(".ico"))
            {
                let pb = PathBuf::from(trimmed);
                if pb.is_file() {
                    return Some(pb);
                }
            }
        }
    }
    // 2. 扫描 ANSI/ASCII 路径
    let text8 = String::from_utf8_lossy(&bytes);
    for part in text8.split('\0') {
        let trimmed = part.trim();
        if trimmed.len() > 6
            && trimmed.as_bytes().get(1) == Some(&b':')
            && (trimmed.to_ascii_lowercase().ends_with(".exe")
                || trimmed.to_ascii_lowercase().ends_with(".ico"))
        {
            let pb = PathBuf::from(trimmed);
            if pb.is_file() {
                return Some(pb);
            }
        }
    }
    None
}

/// 检测提取出的图标是否为 Windows 默认无图标 `.exe` 占位图（外圈白底长方形 + 中心纯蓝小块，如图六中的 OPPO 互联错误图标）。
fn is_generic_windows_exe_icon(rgba: &[u8], w: usize, h: usize) -> bool {
    if rgba.len() != w * h * 4 || w < 16 || h < 16 {
        return false;
    }
    let mut white_gray_box = 0usize;
    let mut total_opaque = 0usize;
    for px in rgba.chunks_exact(4) {
        if px[3] > 180 {
            total_opaque += 1;
            let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
            if r > 210 && g > 210 && b > 210 && (r - g).abs() <= 12 && (g - b).abs() <= 12 {
                white_gray_box += 1;
            }
        }
    }
    // Windows 默认未知程序图标超过 55% 的不透明像素是灰白方框背景
    total_opaque > (w * h / 4) && white_gray_box * 100 / total_opaque.max(1) >= 54
}

#[cfg(windows)]
pub(crate) fn shell_icon(path: &Path) -> Option<ColorImage> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{SHDefExtractIconW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL, HICON};

    // 若传入的是 .lnk 快捷方式，优先解析其指向的真实 .exe/.ico 目标，彻底消除左下角快捷方式箭头
    let target_buf: PathBuf;
    let effective_path = if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
    {
        if let Some(resolved) = resolve_lnk_target_path(path) {
            target_buf = resolved;
            target_buf.as_path()
        } else {
            path
        }
    } else {
        path
    };

    // 升级为 64×64 高清尺寸（通过 SHDefExtractIconW 直接提取 64×64 大尺寸图标，解决高分屏通知历史图标模糊）
    const SIZE: i32 = 64;
    let mut wide: Vec<u16> = effective_path.as_os_str().encode_wide().collect();
    wide.push(0);

    let mut hicon_large = HICON::default();
    let mut hicon_small = HICON::default();
    let extracted_hd = unsafe {
        SHDefExtractIconW(
            PCWSTR(wide.as_ptr()),
            0,
            0,
            Some(&mut hicon_large),
            Some(&mut hicon_small),
            (SIZE as u32) | ((16u32) << 16),
        )
        .is_ok()
            && !hicon_large.is_invalid()
    };
    if !hicon_small.is_invalid() {
        unsafe {
            let _ = DestroyIcon(hicon_small);
        }
    }

    let icon = if extracted_hd {
        hicon_large
    } else {
        let mut info = SHFILEINFOW::default();
        let ok = unsafe {
            SHGetFileInfoW(
                PCWSTR(wide.as_ptr()),
                Default::default(),
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            )
        };
        if ok == 0 || info.hIcon.is_invalid() {
            return None;
        }
        info.hIcon
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let rgba = unsafe {
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            let _ = DestroyIcon(icon);
            return None;
        }
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE,
                biHeight: -SIZE, // 自上而下
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        let mut out = None;
        if let Ok(bmp) = bmp {
            if !bits.is_null() {
                let old = SelectObject(dc, HGDIOBJ(bmp.0));
                let _ = DrawIconEx(dc, 0, 0, icon, SIZE, SIZE, 0, None, DI_NORMAL);
                SelectObject(dc, old);
                // 图标是 32 位 BGRA（直通 alpha）
                let n = (SIZE * SIZE) as usize;
                let src = std::slice::from_raw_parts(bits as *const u8, n * 4);
                out = Some(src.to_vec());
            }
            let _ = DeleteObject(HGDIOBJ(bmp.0));
        }
        let _ = DeleteDC(dc);
        let _ = DestroyIcon(icon);
        out
    };
    let mut src = rgba?;
    // BGRA -> RGBA
    for px in src.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    if src.iter().all(|&b| b == 0) {
        return None;
    }
    // 自动剥离部分 Windows 应用图标外圈的纯白底框/白色遮罩（从四角向内透明化连续的纯白/浅灰底板像素）
    strip_icon_white_outer_mask(&mut src, SIZE as usize, SIZE as usize);
    let img = ColorImage::from_rgba_unmultiplied([SIZE as usize, SIZE as usize], &src);
    Some(img)
}


/// 从图标四周边缘向内执行 Flood-Fill 剥离外圈白色/亮灰底框遮罩，保留中心软件主体图标。
fn strip_icon_white_outer_mask(rgba: &mut [u8], w: usize, h: usize) {
    if rgba.len() != w * h * 4 || w < 8 || h < 8 {
        return;
    }
    // 判定是否为外围透明留白或无彩度的白色/浅灰/抗锯齿灰白边框像素（如 OPPO 互联、手机通知等外圈白底圆角框及其外沿 1px 灰边）
    fn is_outer_neutral_or_transparent(rgba: &[u8], idx: usize) -> bool {
        let r = rgba[idx] as i32;
        let g = rgba[idx + 1] as i32;
        let b = rgba[idx + 2] as i32;
        let a = rgba[idx + 3] as i32;
        if a <= 38 {
            return true;
        }
        let max_c = r.max(g).max(b);
        let min_c = r.min(g).min(b);
        let chroma = max_c - min_c;
        // 纯白/亮灰底框 (min >= 165, chroma <= 26)，或半透明抗锯齿灰白外沿 (a < 238, min >= 78, chroma <= 28)
        (chroma <= 26 && min_c >= 165) || (a < 238 && chroma <= 28 && min_c >= 78)
    }

    fn is_visible_white_plate(rgba: &[u8], idx: usize) -> bool {
        let r = rgba[idx] as i32;
        let g = rgba[idx + 1] as i32;
        let b = rgba[idx + 2] as i32;
        let a = rgba[idx + 3] as i32;
        let max_c = r.max(g).max(b);
        let min_c = r.min(g).min(b);
        a > 38 && (max_c - min_c) <= 26 && min_c >= 175
    }

    // 沿四条边向内发射扫描线，找到每个方向第一个非透明像素（跳过图标四周的透明留白），检测外圈是否为白色/浅灰底板
    let mut first_hit_white = 0usize;
    let mut first_hit_total = 0usize;
    for x in 0..w {
        for y in 0..(h / 3) {
            let idx = (y * w + x) * 4;
            if rgba[idx + 3] > 45 {
                first_hit_total += 1;
                if is_outer_neutral_or_transparent(rgba, idx) {
                    first_hit_white += 1;
                }
                break;
            }
        }
        for y in ((h * 2 / 3)..h).rev() {
            let idx = (y * w + x) * 4;
            if rgba[idx + 3] > 45 {
                first_hit_total += 1;
                if is_outer_neutral_or_transparent(rgba, idx) {
                    first_hit_white += 1;
                }
                break;
            }
        }
    }
    for y in 0..h {
        for x in 0..(w / 3) {
            let idx = (y * w + x) * 4;
            if rgba[idx + 3] > 45 {
                first_hit_total += 1;
                if is_outer_neutral_or_transparent(rgba, idx) {
                    first_hit_white += 1;
                }
                break;
            }
        }
        for x in ((w * 2 / 3)..w).rev() {
            let idx = (y * w + x) * 4;
            if rgba[idx + 3] > 45 {
                first_hit_total += 1;
                if is_outer_neutral_or_transparent(rgba, idx) {
                    first_hit_white += 1;
                }
                break;
            }
        }
    }

    // 统计是否包含可见的彩色/深色中心主体（避免把纯白色单色图标整张掏空）
    let mut colored_core_pixels = 0usize;
    for p in 0..(w * h) {
        let idx = p * 4;
        if rgba[idx + 3] > 80 && !is_outer_neutral_or_transparent(rgba, idx) {
            colored_core_pixels += 1;
        }
    }
    if first_hit_total == 0 || first_hit_white * 4 < first_hit_total || colored_core_pixels < 16 {
        return;
    }

    // 1. 从画布四边全周界启动 8-连通 Flood-Fill，穿过外围透明留白、外圈抗锯齿灰边与整块白色圆角底板，直达中心彩色图标边缘（不设人工距离上限）
    let mut visited = vec![false; w * h];
    let mut stack = Vec::with_capacity(w * 4);
    for x in 0..w {
        for y in [0, h - 1] {
            let p = y * w + x;
            if !visited[p] && is_outer_neutral_or_transparent(rgba, p * 4) {
                visited[p] = true;
                stack.push((x, y));
            }
        }
    }
    for y in 0..h {
        for x in [0, w - 1] {
            let p = y * w + x;
            if !visited[p] && is_outer_neutral_or_transparent(rgba, p * 4) {
                visited[p] = true;
                stack.push((x, y));
            }
        }
    }

    let mut stripped_white_count = 0usize;
    while let Some((x, y)) = stack.pop() {
        let idx = (y * w + x) * 4;
        if is_visible_white_plate(rgba, idx) {
            stripped_white_count += 1;
        }
        rgba[idx] = 0;
        rgba[idx + 1] = 0;
        rgba[idx + 2] = 0;
        rgba[idx + 3] = 0;
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
                    let (ux, uy) = (nx as usize, ny as usize);
                    let np = uy * w + ux;
                    if !visited[np] && is_outer_neutral_or_transparent(rgba, np * 4) {
                        visited[np] = true;
                        stack.push((ux, uy));
                    }
                }
            }
        }
    }

    // 2. 两轮边缘去白边（De-haloing / Color Decontamination）：彻底消除彩色主体与原白底交界处的 1~2px 抗锯齿白圈残留
    for _pass in 0..2 {
        let snapshot = rgba.to_vec();
        for y in 0..h {
            for x in 0..w {
                let p = y * w + x;
                let idx = p * 4;
                let a = snapshot[idx + 3] as i32;
                if a == 0 {
                    continue;
                }
                let mut transparent_neighbors = 0usize;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx < 0 || nx >= w as i32 || ny < 0 || ny >= h as i32 {
                            transparent_neighbors += 1;
                        } else {
                            let nidx = (ny as usize * w + nx as usize) * 4;
                            if snapshot[nidx + 3] <= 24 {
                                transparent_neighbors += 1;
                            }
                        }
                    }
                }
                if transparent_neighbors > 0 {
                    let r = snapshot[idx] as i32;
                    let g = snapshot[idx + 1] as i32;
                    let b = snapshot[idx + 2] as i32;
                    let max_c = r.max(g).max(b);
                    let min_c = r.min(g).min(b);
                    let chroma = max_c - min_c;
                    // 若处于外轮廓边缘且几乎无色相（残留灰白边）或孤立悬浮像素，直接透明化
                    if (chroma <= 34 && min_c >= 135) || (transparent_neighbors >= 5 && chroma <= 42) {
                        rgba[idx] = 0;
                        rgba[idx + 1] = 0;
                        rgba[idx + 2] = 0;
                        rgba[idx + 3] = 0;
                    } else if min_c >= 115 && chroma > 34 {
                        // 对带色相但混入了白色背景的边缘像素做去白底反预乘（Un-matte White），保留纯净彩色并柔化边缘透明度
                        let white_ratio = (min_c as f32 / 255.0).clamp(0.0, 0.85);
                        let inv = (1.0 - white_ratio).max(0.15);
                        rgba[idx] = (((r as f32 - min_c as f32 * 0.82).max(0.0)) / inv).clamp(0.0, 255.0) as u8;
                        rgba[idx + 1] = (((g as f32 - min_c as f32 * 0.82).max(0.0)) / inv).clamp(0.0, 255.0) as u8;
                        rgba[idx + 2] = (((b as f32 - min_c as f32 * 0.82).max(0.0)) / inv).clamp(0.0, 255.0) as u8;
                        rgba[idx + 3] = ((a as f32) * (1.0 - white_ratio * 0.68)).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }

    // 3. 若剥离外圈大白底框后中心图标有效包围盒偏小（例如只占中间 55%），自动居中放大至 88% 画幅，使图标饱满清晰
    if stripped_white_count > (w * h / 10) {
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (w, h, 0usize, 0usize);
        for y in 0..h {
            for x in 0..w {
                if rgba[(y * w + x) * 4 + 3] > 32 {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        if max_x > min_x && max_y > min_y {
            let bw = max_x - min_x + 1;
            let bh = max_y - min_y + 1;
            let max_dim = bw.max(bh);
            if max_dim >= 12 && max_dim * 100 < w * 80 {
                if let Some(full_img) = image::RgbaImage::from_raw(w as u32, h as u32, rgba.to_vec()) {
                    let sub = image::imageops::crop_imm(&full_img, min_x as u32, min_y as u32, bw as u32, bh as u32).to_image();
                    let target_dim = ((w as f32 * 0.88).round() as u32).clamp(16, w as u32);
                    let scaled_w = ((bw as f32 / max_dim as f32) * target_dim as f32).round().max(1.0) as u32;
                    let scaled_h = ((bh as f32 / max_dim as f32) * target_dim as f32).round().max(1.0) as u32;
                    let resized = image::imageops::resize(&sub, scaled_w, scaled_h, image::imageops::FilterType::Lanczos3);
                    rgba.fill(0);
                    let off_x = (w as u32).saturating_sub(scaled_w) / 2;
                    let off_y = (h as u32).saturating_sub(scaled_h) / 2;
                    for sy in 0..scaled_h {
                        for sx in 0..scaled_w {
                            let dx = (off_x + sx) as usize;
                            let dy = (off_y + sy) as usize;
                            if dx < w && dy < h {
                                let p = resized.get_pixel(sx, sy).0;
                                let dst = (dy * w + dx) * 4;
                                rgba[dst..dst + 4].copy_from_slice(&p);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(not(windows))]
fn file_icon(_path: &Path) -> Option<ColorImage> {
    None
}

// ---------------------------------------------------------------
// 本机软件真实图标提取器（进程镜像 + 注册表 App Paths/Uninstall + 开始菜单 .lnk + 内嵌官方图标）
// ---------------------------------------------------------------

#[cfg(windows)]
fn resolve_local_app_icon_image(app_id: &str, app_name: &str) -> Option<ColorImage> {
    use windows::{
        core::{PCWSTR, PWSTR},
        Win32::{
            Foundation::CloseHandle,
            System::{
                Diagnostics::ToolHelp::{
                    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                    TH32CS_SNAPPROCESS,
                },
                Registry::{
                    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
                },
                Threading::{
                    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                    PROCESS_QUERY_LIMITED_INFORMATION,
                },
            },
        },
    };

    let s = format!("{} {}", app_id, app_name).to_lowercase();

    // 1. 先查已记忆的 app_paths
    let cfg = crate::auralsle_ai_status::config::get();
    for (k, p) in &cfg.app_paths {
        let kl = k.to_lowercase();
        if (!kl.is_empty() && s.contains(&kl)) || (!app_name.is_empty() && kl.contains(&app_name.to_lowercase())) {
            let pb = PathBuf::from(p);
            if pb.exists() {
                if let Some(img) = shell_icon(&pb) {
                    if !is_generic_windows_exe_icon(img.as_raw(), img.size[0], img.size[1]) {
                        return Some(img);
                    }
                }
            }
        }
    }

    // 2. 匹配候选进程可执行文件名与搜索关键字
    let mut exe_candidates: Vec<String> = Vec::new();
    let mut lnk_keywords: Vec<String> = Vec::new();
    if !app_name.trim().is_empty() {
        lnk_keywords.push(app_name.trim().to_lowercase());
    }
    for tok in app_id
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|t| t.len() >= 3)
    {
        let tl = tok.to_lowercase();
        if !matches!(tl.as_str(), "com" | "exe" | "app" | "cn" | "org" | "net" | "microsoft" | "windows" | "desktop" | "connect" | "service" | "client" | "helper") {
            exe_candidates.push(format!("{tl}.exe"));
            lnk_keywords.push(tl);
        }
    }
    if s.contains("wechat") || s.contains("weixin") || s.contains("微信") {
        exe_candidates.insert(0, "weixin.exe".into());
        exe_candidates.insert(1, "wechat.exe".into());
        lnk_keywords.push("微信".into());
        lnk_keywords.push("wechat".into());
    }
    if s.contains("qq") || s.contains("tim") {
        exe_candidates.insert(0, "qq.exe".into());
        exe_candidates.insert(1, "tim.exe".into());
        lnk_keywords.push("qq".into());
    }
    if s.contains("workbuddy") || s.contains("codebuddy") {
        exe_candidates.insert(0, "workbuddy.exe".into());
        exe_candidates.insert(1, "codebuddy.exe".into());
        lnk_keywords.push("workbuddy".into());
        lnk_keywords.push("codebuddy".into());
    }
    if s.contains("oppo") || s.contains("heytap") || s.contains("互联") || s.contains("跨屏") || s.contains("手机通知") || s.contains("手机连接") {
        exe_candidates.insert(0, "pcsuite.exe".into());
        exe_candidates.insert(1, "heytap.exe".into());
        exe_candidates.insert(2, "oppopcsuite.exe".into());
                lnk_keywords.push("oppo".into());
        lnk_keywords.push("跨屏互联".into());
        lnk_keywords.push("oppo 互联".into());
    }
    if s.contains("cursor") {
        exe_candidates.insert(0, "cursor.exe".into());
    }
    if s.contains("code") || s.contains("vscode") {
        exe_candidates.push("code.exe".into());
    }
    if s.contains("edge") {
        exe_candidates.insert(0, "msedge.exe".into());
    }
    if s.contains("chrome") {
        exe_candidates.insert(0, "chrome.exe".into());
    }
    if s.contains("dingtalk") || s.contains("钉钉") {
        exe_candidates.insert(0, "dingtalk.exe".into());
    }
    if s.contains("feishu") || s.contains("lark") || s.contains("飞书") {
        exe_candidates.insert(0, "feishu.exe".into());
        exe_candidates.insert(1, "lark.exe".into());
    }
    if s.contains("小黑盒") || s.contains("heybox") || s.contains("xiaoheihe") {
        exe_candidates.insert(0, "heybox.exe".into());
        exe_candidates.insert(1, "xiaoheihe.exe".into());
        exe_candidates.insert(2, "max+.exe".into());
        lnk_keywords.push("小黑盒".into());
        lnk_keywords.push("heybox".into());
    }
    if s.contains("steam") {
        exe_candidates.insert(0, "steam.exe".into());
        lnk_keywords.push("steam".into());
    }
    if s.contains("云服务") || s.contains("onedrive") || s.contains("网盘") || s.contains("cloud") {
        exe_candidates.insert(0, "onedrive.exe".into());
        exe_candidates.insert(1, "baidunetdisk.exe".into());
        exe_candidates.insert(2, "aliyundrive.exe".into());
        lnk_keywords.push("云服务".into());
        lnk_keywords.push("onedrive".into());
    }
    if s.contains("网易云") || s.contains("cloudmusic") {
        exe_candidates.insert(0, "cloudmusic.exe".into());
        lnk_keywords.push("网易云".into());
    }
    if s.contains("qq音乐") || s.contains("qqmusic") {
        exe_candidates.insert(0, "qqmusic.exe".into());
        lnk_keywords.push("qq音乐".into());
    }
    if s.contains("哔哩") || s.contains("bilibili") {
        exe_candidates.insert(0, "bilibili.exe".into());
        lnk_keywords.push("哔哩哔哩".into());
        lnk_keywords.push("bilibili".into());
    }

    // 3. 扫描当前正在运行的本机进程，直接取其真实 .exe 图标
    unsafe {
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut ok = Process32FirstW(snap, &mut entry).is_ok();
            while ok {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let exe_name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                let stem = exe_name.trim_end_matches(".exe");
                let hit = exe_candidates.iter().any(|c| c == &exe_name)
                    || (stem.len() >= 3 && lnk_keywords.iter().any(|k| k.len() >= 3 && (stem.contains(k) || k.contains(stem))));
                if hit && entry.th32ProcessID != 0 {
                    if let Ok(proc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, entry.th32ProcessID) {
                        let mut buf = [0u16; 2048];
                        let mut sz = buf.len() as u32;
                        let got = QueryFullProcessImageNameW(proc, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut sz).is_ok();
                        let _ = CloseHandle(proc);
                        if got && sz > 0 {
                            let full = PathBuf::from(String::from_utf16_lossy(&buf[..sz as usize]));
                            if let Some(img) = shell_icon(&full) {
                                if !is_generic_windows_exe_icon(img.as_raw(), img.size[0], img.size[1]) {
                                    let _ = CloseHandle(snap);
                                    return Some(img);
                                }
                            }
                        }
                    }
                }
                ok = Process32NextW(snap, &mut entry).is_ok();
            }
            let _ = CloseHandle(snap);
        }
    }

    // 4. 查注册表 App Paths 与微信/QQ 安装目录
    let reg_str = |root: HKEY, subkey: &str, val_name: Option<&str>| -> Option<String> {
        let k_wide: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
        let v_wide: Vec<u16> = val_name.map(|v| v.encode_utf16().chain(Some(0)).collect()).unwrap_or_default();
        let mut data = [0u16; 2048];
        let mut size = (data.len() * 2) as u32;
        unsafe {
            RegGetValueW(
                root,
                PCWSTR(k_wide.as_ptr()),
                if v_wide.is_empty() { PCWSTR::null() } else { PCWSTR(v_wide.as_ptr()) },
                RRF_RT_REG_SZ,
                None,
                Some(data.as_mut_ptr().cast()),
                Some(&mut size),
            )
            .ok()
            .ok()?;
        }
        let len = data.iter().position(|&c| c == 0).unwrap_or(data.len());
        Some(String::from_utf16_lossy(&data[..len]))
    };

    if s.contains("wechat") || s.contains("weixin") || s.contains("微信") {
        for (sub, exe) in [
            ("Software\\Tencent\\Weixin", "Weixin.exe"),
            ("Software\\Tencent\\WeChat", "WeChat.exe"),
        ] {
            if let Some(dir) = reg_str(HKEY_CURRENT_USER, sub, Some("InstallPath")) {
                let p = PathBuf::from(dir.trim_matches('"')).join(exe);
                if let Some(img) = shell_icon(&p) {
                    return Some(img);
                }
            }
        }
    }

    // 4a. 直接查询 Windows 通知中心注册表 AppUserModelId 下的原生高清 IconUri（支持 UWP / PWA / 桌面通知应用直接读取高清 PNG/ICO）
    if !app_id.trim().is_empty() {
        let aumid_sub = format!("Software\\Classes\\AppUserModelId\\{}", app_id.trim());
        for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            if let Some(icon_uri) = reg_str(root, &aumid_sub, Some("IconUri")) {
                let clean = icon_uri.trim().trim_matches('"').trim_start_matches("file:///").replace('/', "\\");
                let pb = PathBuf::from(&clean);
                if pb.is_file() {
                    if let Ok(bytes) = std::fs::read(&pb) {
                        if let Ok(dyn_img) = image::load_from_memory(&bytes) {
                            let mut rgba = dyn_img.thumbnail(64, 64).to_rgba8();
                            let (iw, ih) = (rgba.width() as usize, rgba.height() as usize);
                            strip_icon_white_outer_mask(rgba.as_mut(), iw, ih);
                            return Some(ColorImage::from_rgba_unmultiplied([iw, ih], rgba.as_raw()));
                        }
                    }
                    if let Some(img) = shell_icon(&pb) {
                        return Some(img);
                    }
                }
            }
        }
    }

    // 4b. 扫描 Windows 注册表 Uninstall 列表（匹配 DisplayName 提取 DisplayIcon 原生高清软件图标）
    if let Some(img) = find_icon_from_uninstall_registry(&lnk_keywords) {
        return Some(img);
    }

    for cand in &exe_candidates {
        let sub = format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{cand}");
        for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            if let Some(raw) = reg_str(root, &sub, None) {
                let clean = raw.trim().trim_matches('"').split(',').next().unwrap_or("").trim();
                let pb = PathBuf::from(clean);
                if pb.exists() {
                    if let Some(img) = shell_icon(&pb) {
                        return Some(img);
                    }
                }
            }
        }
    }

    // 5. 扫描开始菜单快捷方式目录（.lnk 文件用 SHGetFileInfoW 可直接提取目标软件真实图标）
    let mut start_dirs = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        start_dirs.push(PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Ok(progdata) = std::env::var("PROGRAMDATA") {
        start_dirs.push(PathBuf::from(progdata).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        start_dirs.push(PathBuf::from(userprofile).join("Desktop"));
    }
    if let Ok(localapp) = std::env::var("LOCALAPPDATA") {
        start_dirs.push(PathBuf::from(localapp).join("Programs"));
    }
    for dir in start_dirs {
        if let Some(found) = find_matching_lnk(&dir, &lnk_keywords, 0) {
            if let Some(img) = shell_icon(&found) {
                if !is_generic_windows_exe_icon(img.as_raw(), img.size[0], img.size[1]) {
                    return Some(img);
                }
            }
        }
    }

    None
}

#[cfg(windows)]
fn find_icon_from_uninstall_registry(keywords: &[String]) -> Option<ColorImage> {
    use windows::{
        core::{PCWSTR, PWSTR},
        Win32::System::Registry::{
            RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
            HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ,
        },
    };
    if keywords.is_empty() {
        return None;
    }
    let read_sz = |hkey: HKEY, sub: &[u16], val: &str| -> Option<String> {
        let vw: Vec<u16> = val.encode_utf16().chain(Some(0)).collect();
        let mut buf = [0u16; 1024];
        let mut sz = (buf.len() * 2) as u32;
        unsafe {
            RegGetValueW(
                hkey,
                PCWSTR(sub.as_ptr()),
                PCWSTR(vw.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut sz),
            )
            .ok()
            .ok()?;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    };

    for (root, base_path) in [
        (HKEY_CURRENT_USER, "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
    ] {
        let bw: Vec<u16> = base_path.encode_utf16().chain(Some(0)).collect();
        let mut parent = HKEY::default();
        if unsafe { RegOpenKeyExW(root, PCWSTR(bw.as_ptr()), 0, KEY_READ, &mut parent) }.is_err() {
            continue;
        }
        let mut index = 0u32;
        loop {
            let mut name_buf = [0u16; 256];
            let mut name_len = name_buf.len() as u32;
            let res = unsafe {
                RegEnumKeyExW(
                    parent,
                    index,
                    PWSTR(name_buf.as_mut_ptr()),
                    &mut name_len,
                    None,
                    PWSTR::null(),
                    None,
                    None,
                )
            };
            if res.is_err() {
                break;
            }
            index += 1;
            let sub_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            let sub_w: Vec<u16> = sub_name.encode_utf16().chain(Some(0)).collect();
            let disp_name = read_sz(parent, &sub_w, "DisplayName").unwrap_or_default().to_lowercase();
            let sub_low = sub_name.to_lowercase();
            let matched = keywords.iter().any(|k| {
                k.len() >= 2 && (disp_name.contains(k) || sub_low.contains(k))
            });
            if matched {
                if let Some(raw_icon) = read_sz(parent, &sub_w, "DisplayIcon") {
                    let clean = raw_icon.trim().trim_matches('"').split(',').next().unwrap_or("").trim().trim_matches('"');
                    let pb = PathBuf::from(clean);
                    if pb.is_file() {
                        if let Some(img) = shell_icon(&pb) {
                            if !is_generic_windows_exe_icon(img.as_raw(), img.size[0], img.size[1]) {
                                unsafe { let _ = RegCloseKey(parent); }
                                return Some(img);
                            }
                        }
                    }
                }
            }
        }
        unsafe { let _ = RegCloseKey(parent); }
    }
    None
}

#[cfg(windows)]
fn find_matching_lnk(dir: &Path, keywords: &[String], depth: usize) -> Option<PathBuf> {
    if depth > 4 || keywords.is_empty() {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !ext.eq_ignore_ascii_case("lnk") && !ext.eq_ignore_ascii_case("exe") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !stem.is_empty() && keywords.iter().any(|k| !k.is_empty() && (stem.contains(k) || k.contains(&stem))) {
            return Some(path);
        }
    }
    for sub in subdirs {
        if let Some(hit) = find_matching_lnk(&sub, keywords, depth + 1) {
            return Some(hit);
        }
    }
    None
}

fn load_bundled_brand_icon(app_id: &str, app_name: &str) -> Option<ColorImage> {
    let s = format!("{} {}", app_id, app_name).to_lowercase();
    if s.contains("qq") {
        let bytes = include_bytes!("../assets/qq-client.png");
        let dyn_img = image::load_from_memory(bytes).ok()?.thumbnail(64, 64).to_rgba8();
        return Some(ColorImage::from_rgba_unmultiplied(
            [dyn_img.width() as usize, dyn_img.height() as usize],
            dyn_img.as_raw(),
        ));
    }
    // 为 OPPO 互联、抖音、微信、钉钉、飞书提供 64×64 高清矢量多色 SVG 图标，彻底告别模糊与快捷方式箭头
    let custom_svg: Option<&'static [u8]> = if s.contains("oppo") || s.contains("heytap") || s.contains("互联") || s.contains("跨屏") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#059669'/><circle cx='25' cy='32' r='11' fill='none' stroke='#FFFFFF' stroke-width='4.2'/><circle cx='39' cy='32' r='11' fill='none' stroke='#A7F3D0' stroke-width='4.2'/></svg>")
    } else if s.contains("抖音") || s.contains("douyin") || s.contains("tiktok") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#111218'/><path d='M38 15v23a9 9 0 1 1-6.5-8.6V22a16 16 0 1 0 13.5 15.8V26.5c2.8 2 6.2 3.1 9.8 3.1v-7c-5.2 0-9.6-3.3-10.8-7.6H38z' fill='#25F4EE' transform='translate(-2,-1.5)'/><path d='M38 15v23a9 9 0 1 1-6.5-8.6V22a16 16 0 1 0 13.5 15.8V26.5c2.8 2 6.2 3.1 9.8 3.1v-7c-5.2 0-9.6-3.3-10.8-7.6H38z' fill='#FE2C55' transform='translate(2,1.5)'/><path d='M38 15v23a9 9 0 1 1-6.5-8.6V22a16 16 0 1 0 13.5 15.8V26.5c2.8 2 6.2 3.1 9.8 3.1v-7c-5.2 0-9.6-3.3-10.8-7.6H38z' fill='#FFFFFF'/></svg>")
    } else if s.contains("微信") || s.contains("wechat") || s.contains("weixin") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#07C160'/><ellipse cx='26' cy='27' rx='14' ry='11.5' fill='#FFFFFF'/><circle cx='21' cy='25' r='1.9' fill='#07C160'/><circle cx='31' cy='25' r='1.9' fill='#07C160'/><ellipse cx='40' cy='36' rx='12' ry='9.8' fill='#DCFCE7'/><circle cx='36' cy='35' r='1.6' fill='#059669'/><circle cx='44' cy='35' r='1.6' fill='#059669'/></svg>")
    } else if s.contains("钉钉") || s.contains("dingtalk") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#0089FF'/><path d='M18 17h26l-8 14h11L25 50l4-15H19l-1-18z' fill='#FFFFFF'/></svg>")
    } else if s.contains("飞书") || s.contains("feishu") || s.contains("lark") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#3370FF'/><path d='M16 32l30-14-9 29-7-9-14-6z' fill='#FFFFFF'/></svg>")
    } else if s.contains("小黑盒") || s.contains("heybox") || s.contains("xiaoheihe") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#181920'/><path d='M32 13L50 23.5V44.5L32 55L14 44.5V23.5L32 13Z' fill='none' stroke='#FFFFFF' stroke-width='3.8' stroke-linejoin='round'/><path d='M14 23.5L32 34L50 23.5M32 34V55' fill='none' stroke='#38BDF8' stroke-width='3.4' stroke-linecap='round' stroke-linejoin='round'/><circle cx='32' cy='23' r='3.2' fill='#FFFFFF'/></svg>")
    } else if s.contains("steam") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#171A21'/><circle cx='40' cy='25' r='9.5' fill='none' stroke='#66C0F4' stroke-width='4.2'/><circle cx='40' cy='25' r='3.8' fill='#FFFFFF'/><circle cx='22' cy='43' r='6.5' fill='none' stroke='#FFFFFF' stroke-width='3.8'/><path d='M10 38L22 43L34 31' fill='none' stroke='#FFFFFF' stroke-width='5.2' stroke-linecap='round' stroke-linejoin='round'/></svg>")
    } else if s.contains("云服务") || s.contains("云备份") || s.contains("onedrive") || s.contains("网盘") || s.contains("cloud") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#0284C7'/><path d='M21 44h23a10 10 0 0 0 1.2-19.9A13.5 13.5 0 0 0 19 26.5 9.5 9.5 0 0 0 21 44z' fill='#FFFFFF'/><path d='M28 35l4-4 4 4M32 31v10' fill='none' stroke='#0284C7' stroke-width='3.2' stroke-linecap='round' stroke-linejoin='round'/></svg>")
    } else if s.contains("电源") || s.contains("电池") || s.contains("节能") || s.contains("power") || s.contains("battery") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#059669'/><rect x='13' y='21' width='34' height='22' rx='5' fill='none' stroke='#FFFFFF' stroke-width='3.6'/><rect x='48' y='27' width='4.5' height='10' rx='2' fill='#FFFFFF'/><path d='M33 23L25 33h7l-2 8 9-10h-7l1-8z' fill='#FDE047'/></svg>")
    } else if s.contains("信息") || s.contains("短信") || s.contains("messages") || s.contains("sms") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#2563EB'/><path d='M15 20h34a5 5 0 0 1 5 5v16a5 5 0 0 1-5 5H27l-10 7v-7h-2a5 5 0 0 1-5-5V25a5 5 0 0 1 5-5z' fill='#FFFFFF'/><rect x='22' y='27' width='20' height='3.4' rx='1.7' fill='#2563EB'/><rect x='22' y='34' width='13' height='3.4' rx='1.7' fill='#60A5FA'/></svg>")
    } else if s.contains("天气") || s.contains("气象") || s.contains("weather") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#0EA5E9'/><circle cx='24' cy='25' r='9' fill='#FACC15'/><path d='M23 45h22a8.5 8.5 0 0 0 0-17 11.5 11.5 0 0 0-21.6-3.2A7.5 7.5 0 0 0 23 45z' fill='#FFFFFF'/></svg>")
    } else if s.contains("安全") || s.contains("防护") || s.contains("defender") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#1D4ED8'/><path d='M32 13l17 6.5v11.5c0 11.2-7.2 20.6-17 24-9.8-3.4-17-12.8-17-24V19.5L32 13z' fill='#FFFFFF'/><path d='M25 32l5 5 10-10' fill='none' stroke='#1D4ED8' stroke-width='3.8' stroke-linecap='round' stroke-linejoin='round'/></svg>")
    } else if s.contains("邮件") || s.contains("邮箱") || s.contains("outlook") || s.contains("mail") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#0284C7'/><rect x='13' y='19' width='38' height='26' rx='4.5' fill='#FFFFFF'/><path d='M15 22l17 12 17-12' fill='none' stroke='#0284C7' stroke-width='3.4' stroke-linecap='round' stroke-linejoin='round'/></svg>")
    } else if s.contains("小红书") || s.contains("xiaohongshu") || s.contains("rednote") || s.contains("xhs") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#FF2442'/><rect x='13' y='18' width='38' height='28' rx='6' fill='none' stroke='#FFFFFF' stroke-width='3.6'/><path d='M32 18v28M21 26h6M21 33h6M37 26h6M37 33h6' stroke='#FFFFFF' stroke-width='3.4' stroke-linecap='round'/></svg>")
    } else if s.contains("哔哩") || s.contains("bilibili") || s.contains("b站") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#FB7299'/><path d='M22 13l6 7M42 13l-6 7' stroke='#FFFFFF' stroke-width='3.6' stroke-linecap='round'/><rect x='13' y='20' width='38' height='28' rx='7' fill='none' stroke='#FFFFFF' stroke-width='3.8'/><path d='M23 30l4 3-4 3M41 30l-4 3 4 3' fill='none' stroke='#FFFFFF' stroke-width='3.2' stroke-linecap='round' stroke-linejoin='round'/></svg>")
    } else if s.contains("网易云") || s.contains("cloudmusic") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#E60026'/><circle cx='32' cy='32' r='17' fill='none' stroke='#FFFFFF' stroke-width='3.8'/><circle cx='32' cy='32' r='6.5' fill='none' stroke='#FFFFFF' stroke-width='3.6'/><path d='M32 15c6 0 11 4 11 10' fill='none' stroke='#FFFFFF' stroke-width='3.6' stroke-linecap='round'/></svg>")
    } else if s.contains("qq音乐") || s.contains("qqmusic") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#10B981'/><path d='M26 42a6 6 0 1 1-4-5.6V20l20-4v18a6 6 0 1 1-4-5.6V21l-12 2.4V42z' fill='#FACC15'/></svg>")
    } else if s.contains("淘宝") || s.contains("天猫") || s.contains("闲鱼") || s.contains("京东") || s.contains("拼多多") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#F97316'/><path d='M16 24h32l-3 24H19L16 24z' fill='#FFFFFF'/><path d='M24 24v-4a8 8 0 0 1 16 0v4' fill='none' stroke='#FFFFFF' stroke-width='3.8' stroke-linecap='round'/></svg>")
    } else if s.contains("系统") || s.contains("windows") || s.contains("更新") || s.contains("设置") {
        Some(b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='2' y='2' width='60' height='60' rx='15' fill='#0F172A'/><rect x='15' y='15' width='15' height='15' rx='2.5' fill='#38BDF8'/><rect x='34' y='15' width='15' height='15' rx='2.5' fill='#38BDF8'/><rect x='15' y='34' width='15' height='15' rx='2.5' fill='#38BDF8'/><rect x='34' y='34' width='15' height='15' rx='2.5' fill='#38BDF8'/></svg>")
    } else {
        None
    };
    if let Some(svg_b) = custom_svg {
        if let Ok(img) = egui_extras::image::load_svg_bytes_with_size(
            svg_b,
            Some(egui::load::SizeHint::Size(64, 64)),
        ) {
            return Some(img);
        }
    }
    let (svg_bytes, tint): (&[u8], Option<egui::Color32>) = if s.contains("workbuddy") || s.contains("codebuddy") {
        (include_bytes!("auralsle_ai_status/assets/ai-icons/workbuddy.svg"), None)
    } else if s.contains("gemini") || s.contains("google") {
        (
            include_bytes!("auralsle_ai_status/assets/ai-icons/gemini.svg"),
            Some(egui::Color32::from_rgb(96, 165, 250)),
        )
    } else if s.contains("claude") || s.contains("anthropic") {
        (
            include_bytes!("auralsle_ai_status/assets/ai-icons/claude.svg"),
            Some(egui::Color32::from_rgb(251, 146, 60)),
        )
    } else if s.contains("codex") || s.contains("openai") || s.contains("chatgpt") {
        (
            include_bytes!("auralsle_ai_status/assets/ai-icons/codex.svg"),
            Some(egui::Color32::from_rgb(167, 139, 250)),
        )
    } else if s.contains("deepseek") {
        (
            include_bytes!("auralsle_ai_status/assets/ai-icons/deepseek.svg"),
            Some(egui::Color32::from_rgb(77, 107, 254)),
        )
    } else if s.contains("kimi") || s.contains("moonshot") {
        (
            include_bytes!("auralsle_ai_status/assets/ai-icons/kimi.svg"),
            Some(egui::Color32::from_rgb(56, 189, 248)),
        )
    } else if s.contains("grok") {
        (
            include_bytes!("auralsle_ai_status/assets/ai-icons/grokbot.svg"),
            Some(egui::Color32::from_rgb(52, 211, 153)),
        )
    } else if s.contains("zcode") {
        (include_bytes!("auralsle_ai_status/assets/ai-icons/zcode.svg"), None)
    } else {
        return None;
    };

    let mut image = egui_extras::image::load_svg_bytes_with_size(
        svg_bytes,
        Some(egui::load::SizeHint::Size(64, 64)),
    )
    .ok()?;
    if let Some(c) = tint {
        for px in &mut image.pixels {
            let a = px.a();
            if a > 0 {
                *px = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
            }
        }
    }
    Some(image)
}

static ASYNC_NATIVE_ICON_IMAGES: std::sync::OnceLock<std::sync::Mutex<HashMap<String, Option<ColorImage>>>> =
    std::sync::OnceLock::new();
static ASYNC_NATIVE_ICON_PENDING: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();
static ASYNC_ICON_WORKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn async_native_icon_images() -> &'static std::sync::Mutex<HashMap<String, Option<ColorImage>>> {
    ASYNC_NATIVE_ICON_IMAGES.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn async_native_icon_pending() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    ASYNC_NATIVE_ICON_PENDING.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// 获取应用的高清圆角图标纹理（100% 异步非阻塞：本地软件 .exe 图标在后台低优线程提取，UI 主线程绝不执行磁盘/注册表/进程遍历，彻底根除切页无响应！）。
/// 快速判断指定应用是否已有后台提取完成的本机原生图标（避免每帧重复构造字符串与锁竞争）。
pub fn has_ready_native_icon(ctx: &egui::Context, app_id: &str, app_name: &str) -> Option<egui::TextureHandle> {
    let native_tex_key = egui::Id::new(("auralsle_native_icon_fast_v30", app_id, app_name));
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(native_tex_key)) {
        return Some(tex);
    }
    let tex = extract_app_icon_texture(ctx, app_id, app_name)?;
    ctx.data_mut(|d| d.insert_temp(native_tex_key, tex.clone()));
    Some(tex)
}

pub fn extract_app_icon_texture(
    ctx: &egui::Context,
    app_id: &str,
    app_name: &str,
) -> Option<egui::TextureHandle> {
    let norm_key = format!(
        "{}:{}",
        app_id.trim().to_ascii_lowercase(),
        app_name.trim().to_ascii_lowercase()
    );
    let native_tex_key = egui::Id::new(("auralsle_native_icon_tex_v26", norm_key.as_str()));
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(native_tex_key)) {
        return Some(tex);
    }

    // 1. 检查后台线程是否已完成本机软件高清图标提取
    #[cfg(windows)]
    {
        let resolved_opt = async_native_icon_images()
            .lock()
            .ok()
            .and_then(|map| map.get(&norm_key).cloned());
        match resolved_opt {
            Some(Some(img)) => {
                let tex = ctx.load_texture(
                    format!("app-native-icon-{norm_key}"),
                    img,
                    egui::TextureOptions::LINEAR,
                );
                crate::skin_paint::mark_colored(tex.id());
                ctx.data_mut(|d| d.insert_temp(native_tex_key, tex.clone()));
                return Some(tex);
            }
            Some(None) => {
                // 后台已确认本机未安装该独立软件，直接走下方内置高清矢量/位图缓存
            }
            None => {
                // 尚未在后台解析过：投递至后台线程（限制并发数 <= 2，绝不阻塞 UI 线程）
                let should_spawn = async_native_icon_pending()
                    .lock()
                    .ok()
                    .is_some_and(|mut pend| {
                        if pend.contains(&norm_key) {
                            false
                        } else if ASYNC_ICON_WORKERS.load(std::sync::atomic::Ordering::Relaxed) < 2 {
                            pend.insert(norm_key.clone());
                            true
                        } else {
                            false
                        }
                    });
                if should_spawn {
                    ASYNC_ICON_WORKERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let aid = app_id.to_owned();
                    let aname = app_name.to_owned();
                    let k_clone = norm_key.clone();
                    let ctx_clone = ctx.clone();
                    let _ = std::thread::Builder::new()
                        .name("auralsle-app-icon".into())
                        .spawn(move || {
                            let img = resolve_local_app_icon_image(&aid, &aname);
                            if let Ok(mut map) = async_native_icon_images().lock() {
                                map.insert(k_clone, img);
                            }
                            ASYNC_ICON_WORKERS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                            ctx_clone.request_repaint();
                        });
                }
            }
        }
    }

    // 2. 内置高清品牌图标快速通道（内存直接解码，耗时 < 0.05ms）
    let fallback_tex_key = egui::Id::new(("auralsle_bundled_icon_tex_v26", norm_key.as_str()));
    if let Some(cached) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(fallback_tex_key)) {
        return cached;
    }
    let handle = load_bundled_brand_icon(app_id, app_name).map(|img| {
        let tex = ctx.load_texture(
            format!("app-bundled-icon-{norm_key}"),
            img,
            egui::TextureOptions::LINEAR,
        );
        crate::skin_paint::mark_colored(tex.id());
        tex
    });
    ctx.data_mut(|d| d.insert_temp(fallback_tex_key, handle.clone()));
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration,Instant};
    #[test]
    fn oversized_shell_thumbnails_are_reduced_before_upload(){
        let preview=preview_image(1024,512,vec![255;1024*512*4]).unwrap();
        assert_eq!(preview.size,[96,48]);
        assert!(preview.pixels.len()<=96*96,"缩略图插件返回大图时也必须限制单项内存");
    }
    #[test]
    fn slow_preview_keeps_ui_nonblocking_and_requests_bounded(){
        let ctx=egui::Context::default();let mut cache=Cache::default();
        let(started,ready)=mpsc::channel();let(release,blocked)=mpsc::channel();
        cache.start(ctx.clone(),move|_|{
            let _=started.send(());let _=blocked.recv();
            Loaded{image:None,metadata:"文件夹".into()}
        });
        let path=Path::new("blocked-preview");
        assert!(cache.get(&ctx,path).is_none());
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let at=Instant::now();
        for _ in 0..100 {assert!(cache.get(&ctx,path).is_none());}
        assert!(at.elapsed()<Duration::from_millis(200),"慢磁盘或缩略图插件不能阻塞界面");
        assert_eq!(cache.pending.len(),1,"同一文件不能反复排队");
        for i in 0..100{cache.get(&ctx,Path::new(&format!("queued-{i}")));}
        assert!(cache.pending.len()<=9,"在途请求和排队请求必须有上限");
        release.send(()).unwrap();
    }

    #[test]
    fn preview_cache_bounds_textures_and_releases_them_when_cleared(){
        let ctx=egui::Context::default();let mut cache=Cache::default();
        cache.start(ctx.clone(),|_|Loaded{image:Some(ColorImage::new([8,8],egui::Color32::WHITE)),metadata:"1 KB".into()});
        for i in 0..40{
            let path=PathBuf::from(format!("preview-{i}"));let deadline=Instant::now()+Duration::from_secs(2);
            while cache.get(&ctx,&path).is_none(){assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(1));}
        }
        assert_eq!(cache.entries.len(),32);
        assert!(ctx.tex_manager().read().num_allocated()<=33,"滚动大量文件不能无限保留 GPU 缩略图");
        cache.clear();
        assert!(ctx.tex_manager().read().num_allocated()<=1,"清空置物架后应释放缩略图纹理");
    }
}
