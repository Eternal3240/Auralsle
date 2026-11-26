//! 配色、字体与字号。
//!
//! 字体：拉丁/数字用 Segoe UI（Windows 原生 UI 字体，与原版 DirectWrite 渲染同源），
//! 中文回退到微软雅黑 UI，韩文回退到 Malgun Gothic。三档字重：常规 / 半粗 / 粗。
//! 非 Windows（开发机、录屏）按 `AURALSLE_FONT_DIR` → 常见系统路径依次查找
//! 度量兼容的 Selawik 与 Noto Sans SC。

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---------- 颜色（原版逐像素取色，少量为审美微调） ----------
pub const RED: Color32 = Color32::from_rgb(242, 48, 70);
pub const RED_TOP: Color32 = Color32::from_rgb(253, 104, 92);
pub const BLUE: Color32 = Color32::from_rgb(62, 172, 248);
/// “已连接”蓝：蓝牙连接提醒和无电量设备的连接环共用，不在电量的绿黄橙红里。
pub const LINK: Color32 = Color32::from_rgb(10, 132, 255);
pub const LINK_SOFT: Color32 = Color32::from_rgb(90, 169, 255);
pub const BLUE_DEEP: Color32 = Color32::from_rgb(13, 69, 128);
pub const CYAN: Color32 = Color32::from_rgb(102, 219, 255);
pub const SOFT_GREEN: Color32 = Color32::from_rgb(118, 196, 151);
pub const GREEN: Color32 = Color32::from_rgb(48, 209, 88);
/// 健康页「久坐」模式的主色（与喝水的蓝成对）。
pub const SIT_GREEN: Color32 = Color32::from_rgb(70, 200, 125);
pub const CHARGE: Color32 = Color32::from_rgb(33, 235, 84);
pub const WHITE: Color32 = Color32::WHITE;
pub const GRAY: Color32 = Color32::from_rgb(178, 178, 186);
pub const DIM: Color32 = Color32::from_rgb(142, 142, 153);
pub const BTN: Color32 = Color32::from_rgb(40, 40, 43);
pub const BTN_HOVER: Color32 = Color32::from_rgb(56, 56, 60);
pub const CARD: Color32 = Color32::from_rgb(22, 22, 24);
pub const TRACK: Color32 = Color32::from_rgb(36, 36, 39);
pub const ARROW_BG: Color32 = Color32::from_rgb(48, 48, 51);
pub const WATER_TRACK: Color32 = Color32::from_rgb(15, 38, 64);
pub const YELLOW: Color32 = Color32::from_rgb(255, 196, 61);
/// 无线设备电量 10–19%：介于黄色“一般”和红色“即将耗尽”之间。
pub const ORANGE: Color32 = Color32::from_rgb(255, 138, 61);

pub fn alpha_blue(a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(BLUE.r(), BLUE.g(), BLUE.b(), (255.0 * a) as u8)
}

/// 字重档位。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum W {
    Regular,
    Semibold,
    Bold,
}

pub fn font(size: f32, w: W) -> FontId {
    match w {
        W::Regular => FontId::proportional(size),
        W::Semibold => FontId::new(size, FontFamily::Name("semibold".into())),
        W::Bold => FontId::new(size, FontFamily::Name("bold".into())),
    }
}

/// Windows 11 原生 Segoe Fluent Icons / Segoe MDL2 Assets 图标字体（对应 Twinkle Tray `.icon`）。
pub fn fluent_icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("fluent_icons".into()))
}

/// 字号表（逻辑点）。汉字天然比拉丁字母「显大」，中文字号单独一套，
/// 让两种文字的视觉重量一致，而不是硬把墨高压到拉丁大写高度。
pub mod size {
    pub const TITLE: f32 = 14.5; // 页标题（中文）
    pub const DATE: f32 = 11.0; // 顶栏日期
    pub const CLOCK: f32 = 12.5; // 顶栏时钟
    pub const PILL_CLOCK: f32 = 15.5; // 药丸时钟
    pub const HERO: f32 = 29.0; // 大数字（25:00 / 0.1 / 0/0）
    pub const BODY: f32 = 11.5; // 正文、按钮
    pub const SMALL: f32 = 11.0; // 说明
    pub const TINY: f32 = 10.0; // 脚注、坐标轴
}

struct Face {
    path: PathBuf,
    index: u32,
}

fn find(cands: &[(&str, u32)], dirs: &[PathBuf]) -> Option<Face> {
    for (name, index) in cands {
        let p = Path::new(name);
        if p.is_absolute() && p.exists() {
            return Some(Face { path: p.to_path_buf(), index: *index });
        }
        for d in dirs {
            let q = d.join(name);
            if q.exists() {
                return Some(Face { path: q, index: *index });
            }
        }
    }
    None
}

fn font_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(d) = std::env::var("AURALSLE_FONT_DIR") {
        v.push(PathBuf::from(d));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            v.push(d.join("fonts"));
        }
    }
    v.push(PathBuf::from("assets/fonts"));
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    v.push(PathBuf::from(windir).join("Fonts"));
    for d in [
        "/usr/share/fonts/google-noto-cjk",
        "/usr/share/fonts/opentype/noto",
        "/usr/share/fonts/noto-cjk",
        "/usr/share/fonts/truetype",
    ] {
        v.push(PathBuf::from(d));
    }
    v
}

/// 装载三档字重。默认内置 MiSans Latin + 微软雅黑 / 正黑 / Yu Gothic / Malgun / Leelawadee / Nirmala / Tahoma 全语种零乱码回退。
pub fn load_fonts(ctx: &egui::Context) {
    ctx.set_fonts(shared_fonts().clone());
}

/// 已取消自定义字体切换，保持全局默认 MiSans Latin + 全语种只读系统映射回退。
pub fn sync_custom_font(_ctx: &egui::Context) {}

/// 净化通知标题与正文中的私用区字符 (PUA) 与数学花体字母，彻底消除拼多多/微信特殊网名乱码方块 `□`。
pub fn sanitize_notification_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        let code = ch as u32;
        // 过滤必出 □ 的 Unicode 私用区 (PUA)、变体选择符与替换字符
        if (0xE000..=0xF8FF).contains(&code)
            || (0xF0000..=0x10FFFF).contains(&code)
            || (0xFE00..=0xFE0F).contains(&code)
            || (0xE0100..=0xE01EF).contains(&code)
            || (0x200B..=0x200F).contains(&code)
            || ch == '\u{FFFD}'
            || ch == '\u{FFFC}'
        {
            continue;
        }
        // 将 Unicode 数学花体/粗体/斜体/双线体字母与数字 (U+1D400..=U+1D7FF) 归一化为标准可见字符
        let mapped = match code {
            0x1D400..=0x1D419 => char::from_u32(b'A' as u32 + (code - 0x1D400)),
            0x1D41A..=0x1D433 => char::from_u32(b'a' as u32 + (code - 0x1D41A)),
            0x1D434..=0x1D44D => char::from_u32(b'A' as u32 + (code - 0x1D434)),
            0x1D44E..=0x1D467 => char::from_u32(b'a' as u32 + (code - 0x1D44E)),
            0x1D468..=0x1D481 => char::from_u32(b'A' as u32 + (code - 0x1D468)),
            0x1D482..=0x1D49B => char::from_u32(b'a' as u32 + (code - 0x1D482)),
            0x1D49C..=0x1D4B5 => char::from_u32(b'A' as u32 + (code - 0x1D49C)),
            0x1D4B6..=0x1D4CF => char::from_u32(b'a' as u32 + (code - 0x1D4B6)),
            0x1D4D0..=0x1D4E9 => char::from_u32(b'A' as u32 + (code - 0x1D4D0)),
            0x1D4EA..=0x1D503 => char::from_u32(b'a' as u32 + (code - 0x1D4EA)),
            0x1D504..=0x1D51D => char::from_u32(b'A' as u32 + (code - 0x1D504)),
            0x1D51E..=0x1D537 => char::from_u32(b'a' as u32 + (code - 0x1D51E)),
            0x1D538..=0x1D551 => char::from_u32(b'A' as u32 + (code - 0x1D538)),
            0x1D552..=0x1D56B => char::from_u32(b'a' as u32 + (code - 0x1D552)),
            0x1D56C..=0x1D585 => char::from_u32(b'A' as u32 + (code - 0x1D56C)),
            0x1D586..=0x1D59F => char::from_u32(b'a' as u32 + (code - 0x1D586)),
            0x1D5A0..=0x1D5B9 => char::from_u32(b'A' as u32 + (code - 0x1D5A0)),
            0x1D5BA..=0x1D5D3 => char::from_u32(b'a' as u32 + (code - 0x1D5BA)),
            0x1D5D4..=0x1D5ED => char::from_u32(b'A' as u32 + (code - 0x1D5D4)),
            0x1D5EE..=0x1D607 => char::from_u32(b'a' as u32 + (code - 0x1D5EE)),
            0x1D608..=0x1D621 => char::from_u32(b'A' as u32 + (code - 0x1D608)),
            0x1D622..=0x1D63B => char::from_u32(b'a' as u32 + (code - 0x1D622)),
            0x1D63C..=0x1D655 => char::from_u32(b'A' as u32 + (code - 0x1D63C)),
            0x1D656..=0x1D66F => char::from_u32(b'a' as u32 + (code - 0x1D656)),
            0x1D670..=0x1D689 => char::from_u32(b'A' as u32 + (code - 0x1D670)),
            0x1D68A..=0x1D6A3 => char::from_u32(b'a' as u32 + (code - 0x1D68A)),
            0x1D7CE..=0x1D7D7 => char::from_u32(b'0' as u32 + (code - 0x1D7CE)),
            0x1D7D8..=0x1D7E1 => char::from_u32(b'0' as u32 + (code - 0x1D7D8)),
            0x1D7E2..=0x1D7EB => char::from_u32(b'0' as u32 + (code - 0x1D7E2)),
            0x1D7EC..=0x1D7F5 => char::from_u32(b'0' as u32 + (code - 0x1D7EC)),
            0x1D7F6..=0x1D7FF => char::from_u32(b'0' as u32 + (code - 0x1D7F6)),
            _ => Some(ch),
        };
        if let Some(c) = mapped {
            out.push(c);
        }
    }
    out
}

fn shared_fonts()->&'static FontDefinitions {
    static FONTS:std::sync::OnceLock<FontDefinitions>=std::sync::OnceLock::new();
    FONTS.get_or_init(build_fonts)
}

#[cfg(windows)]
struct MappedFont {
    _source:std::fs::File,
    bytes:memmap2::Mmap,
}

#[cfg(windows)]
impl MappedFont {
    fn open(path:&Path)->std::io::Result<Self>{
        use std::os::windows::fs::OpenOptionsExt;
        // 保留文件句柄并禁止原地写入；允许系统以替换文件的方式更新字体。
        let source=std::fs::OpenOptions::new().read(true).share_mode(0x1|0x4).open(path)?;
        let bytes=unsafe{memmap2::Mmap::map(&source)?};
        Ok(Self{_source:source,bytes})
    }
}

fn font_bytes(path:&Path)->Option<&'static [u8]>{
    #[cfg(windows)]
    if let Ok(mapped)=MappedFont::open(path){
        // 所有字体由进程级 OnceLock 持有，映射和只读句柄与排版器同寿命。
        let mapped=Box::leak(Box::new(mapped));return Some(&mapped.bytes);
    }
    Some(Box::leak(std::fs::read(path).ok()?.into_boxed_slice()))
}

fn build_fonts()->FontDefinitions {
    build_fonts_with_custom("")
}

static MISANS_LATIN_FONTS: [&[u8]; 3] = [
    include_bytes!("../assets/fonts/MiSansLatin-Regular.ttf"),
    include_bytes!("../assets/fonts/MiSansLatin-Semibold.ttf"),
    include_bytes!("../assets/fonts/MiSansLatin-Bold.ttf"),
];

pub fn build_fonts_with_custom(_custom_path: &str)->FontDefinitions {
    let dirs = font_dirs();
    let latin = [
        find(&[("SegUIVar.ttf", 0), ("segoeui.ttf", 0), ("selawk.ttf", 0)], &dirs),
        find(&[("seguisb.ttf", 0), ("SegUIVar.ttf", 0), ("selawksb.ttf", 0), ("segoeuib.ttf", 0)], &dirs),
        find(&[("segoeuib.ttf", 0), ("selawkb.ttf", 0)], &dirs),
    ];
    let fluent_icon_faces = [
        find(&[("SegoeIcons.ttf", 0)], &dirs),
        find(&[("segmdl2.ttf", 0)], &dirs),
    ];
    let cjk = [
        find(&[("msyh.ttc", 1), ("msyh.ttc", 0), ("NotoSansSC-Regular.otf", 0), ("NotoSansCJK-Regular.ttc", 2)], &dirs),
        find(&[("msyhbd.ttc", 1), ("msyhbd.ttc", 0), ("NotoSansSC-Medium.otf", 0), ("NotoSansCJK-Medium.ttc", 2)], &dirs),
        find(&[("msyhbd.ttc", 1), ("msyhbd.ttc", 0), ("NotoSansSC-Bold.otf", 0), ("NotoSansCJK-Bold.ttc", 2)], &dirs),
    ];
    let tc = [
        find(&[("msjh.ttc", 1), ("msjh.ttc", 0)], &dirs),
        find(&[("msjhbd.ttc", 1), ("msjhbd.ttc", 0)], &dirs),
        find(&[("msjhbd.ttc", 1), ("msjhbd.ttc", 0)], &dirs),
    ];
    let jp = [
        find(&[("YuGothR.ttc", 0), ("YuGothM.ttc", 0), ("msgothic.ttc", 0)], &dirs),
        find(&[("YuGothM.ttc", 0), ("YuGothB.ttc", 0), ("msgothic.ttc", 0)], &dirs),
        find(&[("YuGothB.ttc", 0), ("YuGothM.ttc", 0), ("msgothic.ttc", 0)], &dirs),
    ];

    let korean = [
        find(&[("malgun.ttf", 0), ("NotoSansKR-Regular.otf", 0), ("NotoSansCJK-Regular.ttc", 1)], &dirs),
    ];

    // 多语种零乱码回退（均采用只读文件映射不占私有堆内存）：
    // 藏文(Himalaya)、蒙古文(Baiti)、缅甸文(Myanmar Text)、彝文(Yi)、印加/加拿大原住民/切罗基(Gadugi)、
    // 非洲/埃塞俄比亚语系(Ebrima)、爪哇(Javanese)、傣文(Tai Le / New Tai Lue)、八思巴文(PhagsPa)、
    // 格鲁吉亚/亚美尼亚(Sylfaen)、塔纳(MV Boli)、泰/老挝/高棉(LeelawUI)、南亚印度(Nirmala)、
    // 阿拉伯/希伯来/西里尔/希腊/越南(Tahoma / Arial / Lucida Sans Unicode)、生僻汉字 Ext-B/Ext-G(SimSun-ExtB/G/PMingLiU)、符号与 Emoji
    let fallbacks = [
        find(&[("LeelawUI.ttf", 0)], &dirs),
        find(&[("Nirmala.ttc", 0)], &dirs),
        find(&[("tahoma.ttf", 0)], &dirs),
        find(&[("arial.ttf", 0)], &dirs),
        find(&[("himalaya.ttf", 0)], &dirs),
        find(&[("mmrtext.ttf", 0)], &dirs),
        find(&[("monbaiti.ttf", 0)], &dirs),
        find(&[("msyi.ttf", 0)], &dirs),
        find(&[("gadugi.ttf", 0)], &dirs),
        find(&[("ebrima.ttf", 0)], &dirs),
        find(&[("javatext.ttf", 0)], &dirs),
        find(&[("taile.ttf", 0)], &dirs),
        find(&[("ntailu.ttf", 0)], &dirs),
        find(&[("phagspa.ttf", 0)], &dirs),
        find(&[("sylfaen.ttf", 0)], &dirs),
        find(&[("mvboli.ttf", 0)], &dirs),
        find(&[("l_10646.ttf", 0)], &dirs),
        find(&[("seguihis.ttf", 0)], &dirs),
        find(&[("simsun.ttc", 0)], &dirs),
        find(&[("simsunb.ttf", 0)], &dirs),
        find(&[("SimsunExtG.ttf", 0)], &dirs),
        find(&[("mingliub.ttc", 0)], &dirs),
        find(&[("seguisym.ttf", 0)], &dirs),
        find(&[("seguiemj.ttf", 0)], &dirs),
    ];

    let mut fonts = FontDefinitions::default();
    let mut cache: Vec<(PathBuf, u32, String)> = Vec::new();
    let mut add = |fonts: &mut FontDefinitions, face: &Option<Face>, key: String| -> Option<String> {
        let face = face.as_ref()?;
        if let Some((_, _, k)) = cache.iter().find(|(p, i, _)| *p == face.path && *i == face.index) {
            return Some(k.clone());
        }
        let mut d = FontData::from_static(font_bytes(&face.path)?);
        d.index = face.index;
        fonts.font_data.insert(key.clone(), Arc::new(d));
        cache.push((face.path.clone(), face.index, key.clone()));
        Some(key)
    };

    let builtin = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let families = [
        FontFamily::Proportional,
        FontFamily::Name("semibold".into()),
        FontFamily::Name("bold".into()),
    ];
    for (i, fam) in families.iter().enumerate() {
        let ms_key = format!("misans_latin{i}");
        fonts.font_data.insert(ms_key.clone(), Arc::new(FontData::from_static(MISANS_LATIN_FONTS[i])));
        let mut list = vec![ms_key];
        if let Some(k) = add(&mut fonts, &latin[i], format!("latin{i}")) {
            list.push(k);
        }
        if let Some(k) = add(&mut fonts, &cjk[i], format!("cjk{i}")) {
            list.push(k);
        }
        if let Some(k) = add(&mut fonts, &tc[i], format!("tc{i}")) {
            list.push(k);
        }
        if let Some(k) = add(&mut fonts, &jp[i], format!("jp{i}")) {
            list.push(k);
        }
        if let Some(k) = add(&mut fonts, &korean[0], "korean0".to_owned()) {
            list.push(k);
        }
        for (fb_i, fb_face) in fallbacks.iter().enumerate() {
            if let Some(k) = add(&mut fonts, fb_face, format!("fallback_{fb_i}")) {
                list.push(k);
            }
        }
        list.extend(builtin.iter().cloned());
        fonts.families.insert(fam.clone(), list);
    }
    let mut icon_list = Vec::new();
    for (idx, f_face) in fluent_icon_faces.iter().enumerate() {
        if let Some(k) = add(&mut fonts, f_face, format!("fluent_icon_{idx}")) {
            icon_list.push(k);
        }
    }
    icon_list.extend(builtin.iter().cloned());
    fonts.families.insert(FontFamily::Name("fluent_icons".into()), icon_list);
    fonts
}

/// 各个原生窗口统一使用岛仔品牌图标，避免回落到框架默认图标。
pub fn app_icon()->std::sync::Arc<eframe::egui::IconData>{
    static ICON:std::sync::OnceLock<std::sync::Arc<eframe::egui::IconData>>=std::sync::OnceLock::new();
    ICON.get_or_init(||{
        let image=image::load_from_memory(include_bytes!("../assets/auralsle-logo.png")).expect("岛仔图标资源应有效").thumbnail(128,128).to_rgba8();
        std::sync::Arc::new(eframe::egui::IconData{width:image.width(),height:image.height(),rgba:image.into_raw()})
    }).clone()
}

/// 缓存并返回灵屿自有品牌 Logo 纹理（用于灵屿自身发出的通知身份图标）。
pub fn logo_texture(ctx: &eframe::egui::Context) -> eframe::egui::TextureHandle {
    let key = eframe::egui::Id::new("auralsle_self_logo_tex_v1");
    if let Some(tex) = ctx.data(|d| d.get_temp::<eframe::egui::TextureHandle>(key)) {
        return tex;
    }
    let icon = app_icon();
    let img = eframe::egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    let tex = ctx.load_texture("auralsle_self_logo", img, eframe::egui::TextureOptions::LINEAR);
    crate::skin_paint::mark_colored(tex.id());
    ctx.data_mut(|d| d.insert_temp(key, tex.clone()));
    tex
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn korean_titles_and_lyrics_have_glyphs_in_all_weights(){
        let ctx=eframe::egui::Context::default();
        super::load_fonts(&ctx);
        let _=ctx.run(Default::default(),|ctx|{
            for weight in [super::W::Regular,super::W::Semibold,super::W::Bold]{
                let font=super::font(14.0,weight);
                for ch in "눈, 코, 입 향기로운 숨을 담은 너의 코 사랑해 眼鼻嘴 TAEYANG ㄱㄴㅏ".chars().filter(|c|!c.is_whitespace()){
                    assert!(ctx.fonts(|f|f.has_glyph(&font,ch)),"缺少字形：{ch}，字体：{font:?}");
                }
            }
        });
    }

    #[cfg(windows)]
    #[test]
    fn uyghur_and_tibetan_lyrics_have_glyphs_in_all_weights(){
        let ctx=eframe::egui::Context::default();
        super::load_fonts(&ctx);
        // 覆盖维语全部字母：每个字母在词首、词中、词尾、独立各出现一次。
        let uyghur="سوي گۈ دىگەن شۇنداق بولامدۇ يايرىم سېنى سېغىندىم شۇنچە ئا ئە ب پ ت ج چ خ د ر ز ژ س ش غ ف ق ك گ ڭ ل م ن ھ و ۇ ۆ ۈ ۋ ې ى ي ئ لا بلا بەبە بۆبۆ بڭب بھب بېب بىب";
        let shown=crate::rtl_text::display(uyghur).into_owned();
        let tibetan="བཀྲ་ཤིས་བདེ་ལེགས། སྐྱིད་པོ། གྲོགས་པོ། ཁྱེད་རང་།";
        let _=ctx.run(Default::default(),|ctx|{
            for weight in [super::W::Regular,super::W::Semibold,super::W::Bold]{
                let font=super::font(14.0,weight);
                for ch in shown.chars().chain(tibetan.chars()).filter(|c|!c.is_whitespace()){
                    assert!(ctx.fonts(|f|f.has_glyph(&font,ch)),"缺少字形：U+{:04X}，字体：{font:?}",ch as u32);
                }
            }
        });
    }

    #[cfg(windows)]
    #[test]
    fn world_scripts_have_glyphs_without_tofu(){
        let ctx=eframe::egui::Context::default();
        super::load_fonts(&ctx);
        let samples = [
            // 藏文（实际反馈图二歌词）
            "འགྲོ་ཡ། རྩྭ་ཐང་སྟེང་ལ་འགྲོ། གངས་རི་བསྐོར་བར་འགྲོ",
            // 日文 / 韩文 / 泰文 / 缅甸文 / 蒙古文 / 阿拉伯文 / 希伯来文 / 越南文 / 俄文 / 希腊文 / 印地文 / 生僻字 Ext-B
            "さくらんぼ 日本語 한국어 สวัสดี မြန်မာ العربية שלום Tiếng Việt Привет Ελλάδα नमस्ते 𠮷",
        ];
        let _=ctx.run(Default::default(),|ctx|{
            for weight in [super::W::Regular,super::W::Semibold,super::W::Bold]{
                let font=super::font(14.0,weight);
                for text in samples {
                    for ch in text.chars().filter(|c|!c.is_whitespace()){
                        assert!(ctx.fonts(|f|f.has_glyph(&font,ch)),"缺少字形：{ch} (U+{:04X})，字体：{font:?}", ch as u32);
                    }
                }
            }
        });
    }

    #[cfg(windows)]
    #[test]
    fn mapped_fonts_prevent_in_place_writes_and_keep_shared_reads(){
        let path=std::env::temp_dir().join(format!("auralsle-font-map-{}-{}.bin",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&path,b"readonly font fixture").unwrap();
        let mapped=super::MappedFont::open(&path).unwrap();
        assert_eq!(&mapped.bytes[..],b"readonly font fixture");
        assert_eq!(std::fs::read(&path).unwrap(),b"readonly font fixture");
        assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err(),"映射存活期间不能允许原地改写字体");
        drop(mapped);std::fs::remove_file(path).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn system_font_bytes_use_read_only_file_mapping(){
        #[link(name="kernel32")]
        unsafe extern "system"{
            fn K32GetMappedFileNameW(process:*mut core::ffi::c_void,address:*const core::ffi::c_void,buffer:*mut u16,size:u32)->u32;
        }
        let fonts=super::shared_fonts();let font=&fonts.font_data["cjk0"];
        let mut name=[0u16;1024];
        let count=unsafe{K32GetMappedFileNameW(windows::Win32::System::Threading::GetCurrentProcess().0,font.font.as_ptr().cast(),name.as_mut_ptr(),name.len()as u32)};
        assert!(count>0,"中文字体仍占用完整的私有堆副本，没有复用系统字体文件页");
        let name=String::from_utf16_lossy(&name[..count as usize]).to_lowercase();
        assert!([".ttc",".ttf",".otf"].iter().any(|suffix|name.ends_with(suffix)),"映射必须来自实际字体文件");
    }

    #[test]
    fn font_data_is_shared_across_contexts_without_owned_byte_copies(){
        let fonts=super::shared_fonts();
        assert!(fonts.font_data.values().all(|font|matches!(font.font,std::borrow::Cow::Borrowed(_))),"字体不能在每个排版上下文中产生完整副本");
        let cloned=fonts.clone();
        for (name,font) in &fonts.font_data {assert!(std::sync::Arc::ptr_eq(font,&cloned.font_data[name]));}
    }
}
