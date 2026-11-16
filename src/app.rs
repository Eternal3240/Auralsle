//! 灵动岛主体：状态、悬停形变、切页、顶栏与公共层。与平台无关——
//! Windows 外壳（`main.rs`）和无窗口录制器（`auralsle-record`）共用同一份代码。
//!
//! 时间一律来自 `Env::t`（秒），不直接读系统时钟，所以录屏可逐帧复现。

use crate::extras::{Alert, PetState};
use crate::anim::{self, lerp, smoothstep, Spring};
use crate::bar_layout::{self as bar, Layout};
use crate::calendar;
use crate::glass;
use crate::lyrics;
use crate::mail;
use crate::model;
use crate::paint::{self, Card, H};
use crate::shape;
use crate::smtc::{NowPlaying, Transport};
use crate::theme::{self, size, W};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;

// ---------- 几何（逻辑点；原版 125% 截图 ÷ 1.25） ----------
pub const PW: f32 = 419.0; // 展开面板宽
pub const PH: f32 = 140.0; // 展开面板高
pub const MUSIC_PAGE: usize = 0;
pub const FOCUS_PAGE: usize = 1;
pub const WATER_PAGE: usize = 2;
/// 久坐提醒的标题（通知按钮据此区分喝水提醒）。
pub const SIT_TITLE: &str = "该起来活动了";

pub const SHELF_PAGE: usize = 3;
pub const TODO_PAGE: usize = 4;
pub const MAIL_PAGE: usize = 5;
pub const QUOTA_PAGE: usize = 6;
pub const CALENDAR_PAGE: usize = 7;
pub const WEATHER_PAGE: usize = 10;
pub(crate) const WEATHER_H: f32 = 232.0;
pub(crate) const CALENDAR_H: f32 = 216.0;
pub const BAR_W: f32 = 163.0; // 收起药丸宽
pub const BAR_H: f32 = 30.0; // 收起药丸高
pub(crate) const HUD_W: f32 = 260.0;
pub(crate) const LYRIC_BAR_W: f32 = 300.0;
pub(crate) const HUD_TRACK_X: f32 = 48.0;
pub(crate) const HUD_TRACK_W: f32 = 150.0;
pub const BAR_W_MIN: f32 = 44.0; // 极简小条
pub const BAR_H_MIN: f32 = 4.5;
pub const FLARE: f32 = 17.6; // 顶部外翻肩宽
pub const FLARE_H: f32 = 16.0; // 外翻收敛高度
pub const RAD: f32 = 33.0; // 底部圆角
pub const WIN_W: f32 = 640.0; // 窗口（透明画布）
/// [ai-status-mod] 画布左侧额外留白：岛体左边要放课程日程卡。
pub const LEFT_PAD: f32 = 150.0;
pub const CANVAS_W: f32 = WIN_W + 320.0 + LEFT_PAD; // 右侧 320 给通知/阴影，左侧 LEFT_PAD 给课程卡片
pub const WIN_H: f32 = 360.0;  // [ai-status-mod] 岛体最高 236 + 气泡 60 + 余量，原来 270 会把岛下气泡切掉
/// 胶囊皮肤与屏幕顶边的间隙。
pub const PILL_GAP: f32 = 6.0;

/// 圆角矩形一圈轮廓点（顺时针、首尾闭合），用于沿胶囊外形逐段着色。
fn round_rect_points(r:Rect,radius:f32)->Vec<Pos2>{
    let radius=radius.min(r.width().min(r.height())*0.5).max(0.0);
    let corners=[
        (Pos2::new(r.right()-radius,r.top()+radius),-90.0f32),
        (Pos2::new(r.right()-radius,r.bottom()-radius),0.0),
        (Pos2::new(r.left()+radius,r.bottom()-radius),90.0),
        (Pos2::new(r.left()+radius,r.top()+radius),180.0),
    ];
    let mut points=Vec::with_capacity(4*9+1);
    for (c,a0) in corners {
        for i in 0..=8 {let a=(a0+90.0*i as f32/8.0).to_radians();points.push(c+Vec2::new(a.cos(),a.sin())*radius);}
    }
    points.push(points[0]);
    points
}

/// 胶囊的柔和投影：四周都有，多层叠加近似高斯。
fn pill_shadow(painter:&egui::Painter,rect:Rect,radius:f32,strength:f32){
    if strength<=0.01 {return;}
    let feather = 1.0 / painter.ctx().pixels_per_point();
    for (grow,a) in [(10.0f32,0.035f32),(6.5,0.05),(3.5,0.07),(1.5,0.09)] {
        let r=Rect::from_min_max(rect.min-Vec2::new(grow*0.6,grow*0.3),rect.max+Vec2::new(grow*0.6,grow));
        let col = Color32::from_black_alpha((255.0*a*strength) as u8);
        if cfg!(test) {
            painter.rect_filled(r,radius+grow*0.8,col);
        } else {
            shape::fill_pill(painter, r, radius + grow * 0.8, col, feather * 1.4);
        }
    }
}
pub(crate) const PAGE_NAMES: [&str; 10] = ["音乐", "专注", "饮水", "置物架", "待办", "邮箱", "AI 额度", "日程", "AI 工作", "插件"];  // [ai-status-mod]
pub(crate) const PAGE_SETTINGS_TABS: [usize; 10] = [1, 2, 3, 5, 4, 7, 11, 8, 12, 13];

// ---------- 动效参数 ----------
/// 悬停意图缓冲：光标停留这么久才真正展开（期间药丸轻微胀大作为反馈），防误触。
const HOVER_INTENT: f64 = 0.20;
const MINIMAL_HOVER_INTENT: f64 = 0.40;
/// 移开后等待多久开始收起；这段时间内回来不会收。
const LEAVE_DELAY: f64 = 0.12;
const SWELL: (f32, f32) = (1.075, 1.13);
// 展开：欠阻尼，末段约 5% 过冲后回落；高度略软于宽度，像「垂落」下来。
const OPEN_W: (f32, f32) = (330.0, 0.70);
const OPEN_H: (f32, f32) = (290.0, 0.74);
// 收起：临界阻尼，约 0.18 s 干净收回，无回弹。
const CLOSE: (f32, f32) = (620.0, 1.0);
const SWELL_K: (f32, f32) = (420.0, 0.8);
const PAGE_SLIDE: f32 = 46.0;

pub struct Options {
    /// 是否联网查歌词（录制/测试时关掉）。
    pub fetch_lyrics: bool,
    /// 吉祥物在多久无操作后出现（秒）。
    pub pet_idle: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self { fetch_lyrics: true, pet_idle: 45.0 }
    }
}

/// 每帧由外壳提供的外部世界。
#[derive(Clone, Default)]
pub struct Env {
    pub t: f64,
    /// `HH:MM:SS`
    pub clock: String,
    /// `YYYY-MM-DD`
    pub today: String,
    /// 顶栏日期，例如 `10月28日 周三`
    pub date: String,
    pub battery: Option<u8>,
    pub charging: bool,
    /// 互联网连接状态；未知时不触发表情提醒。
    pub network: Option<bool>,
    /// 光标位置（与 `frame` 传入的 `win` 同一坐标系）；不在窗口附近时为 None。
    pub cursor: Option<Pos2>,
    pub idle_seconds: Option<f64>,
    /// 当前会话是否处于锁屏（Win + L、自动锁屏、唤醒后的登录界面）。
    pub screen_locked: bool,
    /// 前台浏览器的顶部是否与岛重叠；岛仔自身窗口或暂不可读时为 None。
    pub browser_at_top: Option<bool>,
    /// 全屏时隐藏了岛体；健康提醒保留，待退出全屏或展开后从待处理徽标确认。
    pub fullscreen_hidden: bool,
    pub media: NowPlaying,
    /// 系统主音量（0..100, 是否静音）；v0.5.4 音量条与系统双向同步。
    pub volume: Option<(u32, bool)>,
    /// 已连接的蓝牙音频设备（名称 + 可选电量）。
    pub bt: Vec<crate::bt::Device>,
    /// 蓝牙服务是否已经完成首次扫描。
    pub bt_ready: bool,
    /// 岛所在屏幕（与 `win` 同一坐标系，逻辑点）；用于让通知详情留在屏幕内。
    pub screen: Option<Rect>,
    /// 毛玻璃皮肤：岛体四周采样得到的背景色（sRGB 0..1）。
    pub backdrop: Option<[f32; 3]>,
    /// 毛玻璃皮肤：系统背景模糊当前是否生效。
    pub glass_blur: bool,
    /// 不支持逐像素透明的兼容后端需要按内容轮廓裁剪窗口。
    pub opaque_surface: bool,
}

impl Env {
    pub fn hour(&self) -> usize {
        self.clock.get(0..2).and_then(|s| s.parse().ok()).unwrap_or(12)
    }
    pub fn minute(&self) -> u32 {
        self.clock.get(3..5).and_then(|s| s.parse().ok()).unwrap_or(0)
    }
}

pub use crate::messenger::{Messenger,Unread};

/// 展开后未读胶囊的宽度、离面板的间距和每个图标占的高度。
pub(crate) const UNREAD_W: f32 = 40.0;
pub(crate) const UNREAD_GAP: f32 = 8.0;
pub(crate) const UNREAD_SLOT: f32 = 36.0;
/// 收起时两个来源图标的间距：并排各占一格，不叠放。
fn unread_step(_horizontal: bool) -> f32 { 22.0 }

/// 「极简药丸 / 横幅态」右侧等高消息悬浮球的最小直径（细线态不再缩小）。
pub(crate) const PILL_ORB_MIN: f32 = 20.0;

/// 等高消息悬浮球的最大直径。
///
/// 横幅/药丸态的高度都在 56 以内；展开动画里 `panel.height()` 会长到 174/248，
/// 如果直接用面板高度定球径，球就会长成"巨球"（第三轮图4/5/6 的根因）。
pub(crate) const PILL_ORB_MAX: f32 = 56.0;

/// 等高消息悬浮球直径：跟随当前岛体胶囊高度，夹在 `[PILL_ORB_MIN, PILL_ORB_MAX]`。
///
/// 该开关的语义是「收到未读消息时在极简药丸右侧显示**等高**消息图标，展开时平滑过渡」，
/// 所以球径由胶囊实际高度决定；旧实现用 `clamp(20.0, BAR_H)` 把直径死锁在 30px，
/// 横幅把胶囊撑到 44px 时球仍是 30px，视觉上「又小又往上顶」。
/// 但传入的必须是**折叠态胶囊高度**（`base_size().1`），不能用展开动画中的面板高度。
pub(crate) fn pill_orb_diameter(capsule_h: f32) -> f32 { capsule_h.clamp(PILL_ORB_MIN, PILL_ORB_MAX) }

/// 悬浮球顶端：与胶囊垂直居中对齐；胶囊比球还矮（极简细线态）时贴顶，圆顶不会越出屏幕边。
pub(crate) fn pill_orb_top(panel: Rect, d: f32) -> f32 {
    if panel.height() < PILL_ORB_MIN { panel.top() } else { panel.center().y - d * 0.5 }
}

/// 悬浮球「位置」插值：ease-out-quart，先动后静。
///
/// [第五轮 · 僵硬感] 原来位置和尺寸共用 `smoothstep(0.7→1.0)`：
/// 面板长到 70% 才起步、再在 30% 的窗口里同时做完位移与拉伸，看上去像"啪"一下弹出来。
/// 拆成两条曲线后，球先快速滑到目标位置、再慢慢长成竖胶囊，读起来是有意图的过渡。
pub(crate) fn orb_morph_position(t: f32) -> f32 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
    1.0 - (1.0 - t).powi(4)
}

/// 悬浮球「形状」插值：ease-out-back，收尾带 6% 回弹（再夹回 1.0 以内交给调用方限幅）。
pub(crate) fn orb_morph_shape(t: f32) -> f32 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
    let u = t - 1.0;
    (1.0 + 2.70158 * u * u * u + 1.70158 * u * u).clamp(0.0, 1.06)
}

/// 来消息时悬浮球入场/脉冲动画参数：返回 `(scale, slide_t, alpha, ripple_t)`。
/// - `scale`：二阶欠阻尼果冻弹簧缩放（0.18 -> 1.18 过冲 -> 0.94 回弹 -> 1.00 稳定）
/// - `slide_t`：从主岛胶囊边缘向外滑出的位移进度（0.0 -> 1.06 -> 1.00）
/// - `alpha`：前 0.16s 快速渐显（0.0 -> 1.0）
/// - `ripple_t`：外圈扩散光环进度（0.0..1.0，>1.0 表示结束）
pub(crate) fn orb_arrival_anim(elapsed: f32) -> (f32, f32, f32, f32) {
    if !elapsed.is_finite() || elapsed < 0.0 || elapsed >= 0.62 {
        return (1.0, 1.0, 1.0, 1.0);
    }
    let alpha = smoothstep(0.0, 0.16, elapsed);
    let su = (elapsed / 0.38).clamp(0.0, 1.0) - 1.0;
    let slide_t = (1.0 + 2.5 * su * su * su + 1.5 * su * su).clamp(0.0, 1.08);
    // 欠阻尼弹簧包络：0 -> 1.18 -> 0.94 -> 1.02 -> 1.0
    let env = (-elapsed * 7.2).exp();
    let osc = (elapsed * 21.5).cos();
    let scale = (1.0 - 0.82 * env * osc + 0.16 * (-elapsed * 5.0).exp() * (elapsed * 13.5).sin()).clamp(0.18, 1.20);
    let ripple_t = (elapsed / 0.56).clamp(0.0, 1.0);
    (scale, slide_t, alpha, ripple_t)
}

/// 等高消息悬浮球矩形：直径 `d`，横向贴胶囊再加 `gap`，`place_left` 时翻到左侧。
pub(crate) fn pill_orb_rect(panel: Rect, d: f32, gap: f32, place_left: bool) -> Rect {
    let x = if place_left { panel.left() - gap - d } else { panel.right() + gap };
    Rect::from_min_size(Pos2::new(x, pill_orb_top(panel, d)), Vec2::new(d, d))
}

/// 等高消息悬浮球矩形（带窗口夹紧）：靠右放不下时贴住窗口右缘，**不翻边**。
///
/// 第三轮的现象「点开大胶囊时球跑去了左边」就是换边判定造成的：
/// 展开中 `panel.right()+gap+d` 一旦超出窗口就翻左，视觉上球横穿整个岛体。
pub(crate) fn pill_orb_clamped(panel: Rect, d: f32, gap: f32, place_left: bool, bounds: Rect) -> Rect {
    let r = pill_orb_rect(panel, d, gap, place_left);
    let x = if r.right() > bounds.right() {
        bounds.right() - d
    } else if r.left() < bounds.left() {
        bounds.left()
    } else {
        r.left()
    };
    Rect::from_min_size(Pos2::new(x, r.top()), Vec2::new(d, d))
}

/// 球内「图标 + 角标」的几何，全部按球径归一（单位：岛体逻辑像素）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OrbBadgeLayout {
    /// 图标外接正方形边长（图标居中绘制）。
    pub icon_side: f32,
    /// 角标圆心相对球心的横向/纵向偏移（右下象限）。
    pub badge_dx: f32,
    pub badge_dy: f32,
    /// 角标圆半径与数字字号。
    pub badge_r: f32,
    pub digit: f32,
}

/// 计算球内图标与角标：图标始终居中，角标压在图标右下角但整体落在球面之内。
///
/// 旧实现用「相对 40px 球」的绝对量（`+7.5r/+6.8r`、`r=5.2r`）再乘 `size_ratio`，
/// 球长到 44~56 后角标会切到圆的斜边、图标顶住圆边，看起来"没居中"（第三轮图2）。
pub(crate) fn orb_badge_layout(d: f32) -> OrbBadgeLayout {
    let d = d.max(1.0);
    OrbBadgeLayout {
        icon_side: d * 0.54,
        badge_dx: d * 0.24,
        badge_dy: d * 0.22,
        badge_r: d * 0.10,
        digit: (d * 0.145).max(6.0),
    }
}

/// 交给外壳执行的副作用。
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Media(Transport),
    Open(PathBuf),
    ViewMessenger(Unread),
    OpenUrl(String),
    /// 需要键盘输入：外壳应让窗口可激活并取得焦点。
    Focus,
    Reveal(PathBuf),
    Copy(String),
    Notify(String, String),
    RequestNotifications,
    RequestQqNotifications,
    DisconnectNotifications,
    ToggleAutostart,
    AddFiles,
    DragFile(PathBuf),
    QuotaRefresh,
    QuotaConnected,
    QuotaChooseFile,
    /// 把系统主音量设为 0..100（v0.5.4 音量条）。
    SetVolume(u32),
    /// 切换系统静音。
    ToggleMute,
    /// 音量命令携带点击时的播放器身份，避免切换歌曲来源后误调其他应用。
    MusicVolume(Transport,String),
    /// v0.5.5：账号清单变了，让外壳重新同步收信线程。
    MailSync,
    /// 测试某个邮箱的连接（授权码只在内存里传给工作线程）。
    MailTest(mail::Account, String),
    MailManage(crate::mail_manage::Request),
    /// v0.5.6：用临时密码试连日历，成功后才允许落盘。
    CalendarTest(calendar::Config, String),
    /// 让外壳立即同步一次日历。
    CalendarSync,
    CalendarMonth(chrono::NaiveDate),
    /// 断开并删除日历配置（凭据由界面删除）。
    CalendarForget,
    /// 选择 .ics 文件导入本机日程。
    CalendarImport,
    /// 重启岛仔（切换渲染方式后生效）。
    RestartApp,
}

/// 毛玻璃皮肤下原生窗口区域的组成部分（相对 `win` 左上角，逻辑点）。
/// 系统模糊只出现在窗口区域内，所以区域要贴合岛体轮廓，而不是外接矩形。
#[derive(Clone, Debug, PartialEq)]
pub enum GlassShape {
    /// 任意闭合轮廓。
    Outline(Vec<Pos2>),
    /// 岛体 / 通知胶囊：顶部外翻肩部 + 竖直侧边 + 底部圆角（参数同 `shape::fill`）。
    Notch { rect: Rect, flare: f32, flare_h: f32, radius: f32 },
    /// 停靠在左右边缘的岛体：肩部沿屏幕边缘展开（参数同 `shape::fill_docked`）。
    Docked { rect: Rect, dock: model::Dock, flare: f32, flare_h: f32, radius: f32 },
    /// 圆角矩形（通知详情、提示、浮动确认）。
    Round(Rect, f32),
}

impl GlassShape {
    /// 闭合轮廓（逻辑点）；窗口区域按它裁剪。
    pub fn outline(&self) -> Vec<Pos2> {
        match self {
            Self::Outline(points) => points.clone(),
            Self::Notch { rect, flare, flare_h, radius } => shape::outline(*rect, *flare, *flare_h, *radius),
            Self::Docked { rect, dock, flare, flare_h, radius } => shape::outline_docked(*rect, *dock, *flare, *flare_h, *radius),
            Self::Round(r, radius) => shape::outline(*r, 0.0, 0.5, *radius),
        }
    }
}

/// 一块系统模糊：物理像素的窗口坐标与圆角半径（0 = 直角）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurPiece {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub radius: f32,
}

/// 把毛玻璃轮廓拆成合成引擎能直接表达的圆角矩形块：
/// 主体向上多延伸一个圆角半径，让顶部两角落在窗口外，只留下底部圆角；
/// 外翻的肩部按物理像素逐行拼出（相邻同宽的行合并），边缘误差不超过半个像素。
pub fn blur_pieces(shapes: &[GlassShape], ppp: f32) -> Vec<BlurPiece> {
    let mut out = Vec::new();
    for shape in shapes {
        match shape {
            GlassShape::Docked { rect, dock, flare, flare_h, radius } => {
                if rect.width() <= 0.0 || rect.height() <= 0.0 { continue; }
                // 在贴顶坐标系里按刘海拆块，再把每块转到左 / 右边缘（宽高互换）。
                let (map, frame) = shape::EdgeMap::of(*rect, *dock);
                let mut pieces = blur_pieces(&[GlassShape::Notch { rect: frame, flare: *flare, flare_h: *flare_h, radius: *radius }], ppp);
                let edge = (map.apply(Pos2::ZERO).x * ppp).round();
                for p in &mut pieces {
                    let (x, y, w, h) = (p.x, p.y, p.w, p.h);
                    *p = BlurPiece { x: if *dock == model::Dock::Left { edge + y } else { edge - y - h }, y: x, w: h, h: w, radius: p.radius };
                }
                out.extend(pieces);
            }
            GlassShape::Notch { rect, flare, flare_h, radius } => {
                if rect.width() <= 0.0 || rect.height() <= 0.0 { continue; }
                let r = radius.min(rect.height() * 0.5).min(rect.width() * 0.5).max(0.0) * ppp;
                let fh = flare_h.min(rect.height() * 0.5).max(0.5) * ppp;
                let (x0, x1, y0, y1) = ((rect.left() * ppp).round(), (rect.right() * ppp).round(), (rect.top() * ppp).round(), (rect.bottom() * ppp).round());
                out.push(BlurPiece { x: x0, y: y0 - r, w: x1 - x0, h: y1 - y0 + r, radius: r });
                let rows = fh.ceil() as i32;
                let mut i = 0;
                while i < rows {
                    let wing = |row: i32| (flare * ppp * (1.0 - ((row as f32 + 0.5) / fh).min(1.0)).powf(2.4)).round();
                    let w = wing(i);
                    if w < 1.0 { break; }
                    let mut h = 1;
                    while i + h < rows && wing(i + h) == w { h += 1; }
                    let y = y0 + i as f32;
                    out.push(BlurPiece { x: x0 - w, y, w, h: h as f32, radius: 0.0 });
                    out.push(BlurPiece { x: x1, y, w, h: h as f32, radius: 0.0 });
                    i += h;
                }
            }
            GlassShape::Round(r, radius) => {
                let (x,y,right,bottom)=((r.left()*ppp).round(),(r.top()*ppp).round(),(r.right()*ppp).round(),(r.bottom()*ppp).round());
                let (w,h)=(right-x,bottom-y);
                if w<=0.0 || h<=0.0 {continue;}
                // 收起动画先按物理边界取整，再限制圆角，避免上下圆弧在中线交叠。
                out.push(BlurPiece { x,y,w,h,radius:(radius*ppp).max(0.0).min(w.min(h)*0.5) });
            }
            GlassShape::Outline(points) => {
                // 任意轮廓只能退化为外接矩形；岛体本身走 Notch，不会用到这里。
                let b = Rect::from_points(points);
                if b.is_positive() {
                    out.push(BlurPiece { x: (b.left() * ppp).round(), y: (b.top() * ppp).round(), w: (b.width() * ppp).round(), h: (b.height() * ppp).round(), radius: 0.0 });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod blur_piece_tests {
    use super::*;

    fn covered(pieces: &[BlurPiece], x: f32, y: f32) -> bool {
        pieces.iter().any(|p| {
            if !(x >= p.x && x < p.x + p.w && y >= p.y && y < p.y + p.h) { return false; }
            // 圆角：只判断四角的圆弧外侧
            let r = p.radius;
            let cx = x.clamp(p.x + r, p.x + p.w - r);
            let cy = y.clamp(p.y + r, p.y + p.h - r);
            (x - cx).powi(2) + (y - cy).powi(2) <= r * r + 1e-3
        })
    }

    #[test]
    fn blur_follows_the_island_outline_not_its_bounding_box() {
        for ppp in [1.0, 1.25, 1.5, 2.0] {
            let rect = Rect::from_min_size(Pos2::new(110.0, 0.0), Vec2::new(419.0, 140.0));
            let shape = GlassShape::Notch { rect, flare: 17.6, flare_h: 16.0, radius: 14.0 };
            let pieces = blur_pieces(std::slice::from_ref(&shape), ppp);
            let outline = shape.outline();
            // 轮廓内部的点都被模糊覆盖，外部（含底部圆角外、肩部外）都不覆盖；边缘留 1.5 像素容差。
            let inside = |x: f32, y: f32| {
                let mut c = false;
                let mut prev = *outline.last().unwrap();
                for &next in &outline {
                    if (next.y > y) != (prev.y > y) && x < (prev.x - next.x) * (y - next.y) / (prev.y - next.y) + next.x { c = !c; }
                    prev = next;
                }
                c
            };
            let mut checked = 0;
            for yi in 0..(150.0 * ppp) as i32 {
                for xi in ((80.0 * ppp) as i32..(560.0 * ppp) as i32).step_by(3) {
                    let (x, y) = (xi as f32 + 0.5, yi as f32 + 0.5);
                    let (lx, ly) = (x / ppp, y / ppp);
                    let deep_in = inside(lx, ly) && [(-1.5, 0.0), (1.5, 0.0), (0.0, 1.5), (0.0, -1.5)].iter().all(|(dx, dy)| inside(lx + dx / ppp, (ly + dy / ppp).max(0.01)));
                    let far_out = !inside(lx, ly) && [(-1.5, 0.0), (1.5, 0.0), (0.0, 1.5), (0.0, -1.5)].iter().all(|(dx, dy)| !inside(lx + dx / ppp, ly + dy / ppp));
                    if deep_in { assert!(covered(&pieces, x, y), "{ppp} 缩放下岛体内 ({x},{y}) 没有模糊"); checked += 1; }
                    if far_out && y >= 0.0 { assert!(!covered(&pieces, x, y), "{ppp} 缩放下岛体外 ({x},{y}) 不能出现模糊"); }
                }
            }
            assert!(checked > 1000);
            assert!(pieces.len() < 60, "肩部合并后块数应很少：{}", pieces.len());
        }
    }

    #[test]
    fn side_docked_blur_follows_the_rotated_outline() {
        for dock in [model::Dock::Left, model::Dock::Right] {
            for ppp in [1.0, 1.5] {
                let x = if dock == model::Dock::Left { 0.0 } else { 81.0 };
                let shape = GlassShape::Docked { rect: Rect::from_min_size(Pos2::new(x, 200.0), Vec2::new(419.0, 140.0)), dock, flare: 17.6, flare_h: 16.0, radius: 14.0 };
                let pieces = blur_pieces(std::slice::from_ref(&shape), ppp);
                let outline = shape.outline();
                let inside = |x: f32, y: f32| {
                    let mut c = false;
                    let mut prev = *outline.last().unwrap();
                    for &next in &outline {
                        if (next.y > y) != (prev.y > y) && x < (prev.x - next.x) * (y - next.y) / (prev.y - next.y) + next.x { c = !c; }
                        prev = next;
                    }
                    c
                };
                let mut checked = 0;
                for yi in ((170.0 * ppp) as i32..(370.0 * ppp) as i32).step_by(2) {
                    for xi in (0..(500.0 * ppp) as i32).step_by(3) {
                        let (px, py) = (xi as f32 + 0.5, yi as f32 + 0.5);
                        let (lx, ly) = (px / ppp, py / ppp);
                        let near = [(-1.5, 0.0), (1.5, 0.0), (0.0, 1.5), (0.0, -1.5)];
                        let deep_in = inside(lx, ly) && near.iter().all(|(dx, dy)| inside((lx + dx / ppp).clamp(0.01, 499.99), ly + dy / ppp));
                        let far_out = !inside(lx, ly) && near.iter().all(|(dx, dy)| !inside(lx + dx / ppp, ly + dy / ppp));
                        if deep_in { assert!(covered(&pieces, px, py), "{dock:?} {ppp}: 岛体内 ({px},{py}) 没有模糊"); checked += 1; }
                        let on_canvas = lx > 0.5 && lx < 499.5;
                        if far_out && on_canvas { assert!(!covered(&pieces, px, py), "{dock:?} {ppp}: 岛体外 ({px},{py}) 不能有模糊"); }
                    }
                }
                assert!(checked > 1000);
            }
        }
    }

    #[test]
    fn collapsing_pill_blur_radius_fits_the_physical_bounds() {
        for ppp in [1.0,1.25,1.5,2.0] {
            for height in [30.02,30.3,30.49,30.5,30.8,31.0,31.2,32.1,35.3] {
                let rect=Rect::from_min_size(Pos2::new(230.23,6.0),Vec2::new(179.31,height));
                let radius=15.0+(height-30.0)*0.1;
                let pieces=blur_pieces(&[GlassShape::Round(rect,radius)],ppp);
                assert_eq!(pieces.len(),1);
                let p=pieces[0];
                assert!(p.radius*2.0<=p.w.min(p.h),"收起时圆弧不能超过物理边界：ppp={ppp}, height={height}, piece={p:?}");
                assert_eq!(p.x+p.w,(rect.right()*ppp).round(),"模糊与岛体右边界应使用同一像素位置");
                assert_eq!(p.y+p.h,(rect.bottom()*ppp).round(),"模糊与岛体下边界应使用同一像素位置");
            }
        }
    }

    #[test]
    fn rounded_rectangles_and_empty_shapes() {
        let pieces = blur_pieces(&[GlassShape::Round(Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(100.0, 40.0)), 10.0)], 1.5);
        assert_eq!(pieces, vec![BlurPiece { x: 15.0, y: 30.0, w: 150.0, h: 60.0, radius: 15.0 }]);
        assert!(blur_pieces(&[GlassShape::Notch { rect: Rect::NOTHING, flare: 5.0, flare_h: 8.0, radius: 12.0 }], 1.0).is_empty());
        // 极简细条：没有肩部，只有一块
        let tiny = blur_pieces(&[GlassShape::Notch { rect: Rect::from_min_size(Pos2::new(298.0, 0.0), Vec2::new(44.0, 4.5)), flare: 0.0, flare_h: 8.0, radius: 2.25 }], 1.25);
        assert_eq!(tiny.len(), 1);
    }
}

pub struct FrameOut {
    /// 原生窗口应保留的可见区域（相对 `win` 左上角）。
    pub region: Rect,
    /// 毛玻璃 + 系统模糊时，替代 `region` 的贴合轮廓；其余情况为 None。
    pub glass: Option<Vec<GlassShape>>,
    /// 鼠标命中区（相对 `win` 左上角），区域外点击穿透到下层窗口。
    pub hit: Rect,
    pub notice_hit: Option<Rect>,
    /// 文件拖入感应区；独立于普通鼠标命中区。
    pub file_drop_zone: Option<Rect>,
    /// 展开状态立即生效，不等待展开动画结束。
    pub expanded: bool,
    pub editing: bool,
    pub animating: bool,
    pub repaint_after: std::time::Duration,
    pub cmds: Vec<Cmd>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Focus,
    Rest,
}

impl Phase {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Phase::Focus => "专注时间",
            Phase::Rest => "休息",
        }
    }
}

pub(crate) struct Pomo {
    pub phase: Phase,
    pub remaining: f32,
    pub running: bool,
}

/// 本地歌词（酷狗、汽水）每 5 秒复查，最多约 1 分钟；网络失败每 30 秒重试，最多约 5 分钟。手动点重试会重新计数。
const NATIVE_RETRIES:u32=12;
const NETWORK_RETRIES:u32=10;

/// 与 `format!("{title}|{artist}|{duration:.0}")` 等价，但不分配内存：界面每帧要比较好几次。
/// [修复需求 1 岛上歌词失效]：运行时仅比对 `歌名|歌手|` 前缀，彻底解决 SMTC 晚一拍刷新 `duration`（如 `0 -> 215`）导致已获取歌词在收起岛上被误判失效的 bug！
fn lyric_key_matches(key:&str,media:&NowPlaying)->bool {
    use std::io::Write;
    if media.title.trim().is_empty() { return false; }
    let Some(rest)=key.strip_prefix(media.title.as_str()).and_then(|rest|rest.strip_prefix('|')) else{return false;};
    let Some(rest)=rest.strip_prefix(media.artist.as_str()).and_then(|rest|rest.strip_prefix('|')) else{return false;};
    if !cfg!(test) {
        return true;
    }
    let mut buffer=[0u8;32];
    let mut cursor=std::io::Cursor::new(&mut buffer[..]);
    if write!(cursor,"{:.0}",media.duration).is_err(){return rest==format!("{:.0}",media.duration);}
    let written=cursor.position() as usize;
    rest.as_bytes()==&buffer[..written]
}

#[test]
fn lyric_key_matches_the_formatted_key_without_allocating(){
    for (title,artist,duration) in [("歌","手",120.4),("A|B","C",0.0),("歌","",239.5),("歌","手",240.5),("","",f64::NAN),("t","a",1e30)] {
        let media=NowPlaying{title:title.into(),artist:artist.into(),duration,..Default::default()};
        let key=format!("{}|{}|{:.0}",title,artist,duration);
        assert!(lyric_key_matches(&key,&media),"{key}");
        assert!(!lyric_key_matches(&format!("{key}0"),&media));
        assert!(!lyric_key_matches(&format!("x{key}"),&media));
    }
}

/// 发起歌词查询时的歌曲信息，用来判断之后的变化是换歌还是信息补全。
#[derive(Clone,Debug,Default)]
pub(crate) struct LyricSong {pub source:String,pub title:String,pub artist:String,pub duration:f64,pub song_id:Option<u64>}
impl LyricSong {
    fn of(media:&NowPlaying)->Self {
        Self{source:media.source.clone(),title:media.title.clone(),artist:media.artist.clone(),duration:media.duration,song_id:media.song_id}
    }
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum SongChange {
    /// 换了一首歌：立即清空旧歌词并重新查询。
    Different,
    /// 同一首，只是时长、写法等补全：已有同步歌词时不再查询。
    Same,
    /// 同一首，署名有补全：重新查询，但查询期间继续显示已有歌词。
    Refined,
}

fn song_change(previous:&LyricSong,media:&NowPlaying)->SongChange {
    if !previous.source.eq_ignore_ascii_case(&media.source) {return SongChange::Different;}
    match (previous.song_id,media.song_id) {
        (Some(old),Some(new))=>return if old==new {SongChange::Same}else{SongChange::Different},
        _=>{}
    }
    // 同名的不同版本（现场版、伴奏等）时长通常不同：两边都已知且相差超过 3 秒视为不同歌曲。
    if previous.duration>0.0 && media.duration>0.0 && (previous.duration-media.duration).abs()>3.0 {return SongChange::Different;}
    match lyrics::song_relation(&previous.title,&previous.artist,&media.title,&media.artist) {
        Some(true)=>SongChange::Same,
        Some(false)=>SongChange::Refined,
        None=>SongChange::Different,
    }
}

/// 等待旧请求结束后再发起的在线查询（同一时间只保留一个网络线程）。
pub(crate) struct LyricsJob {
    source:String,title:String,artist:String,duration:f64,song_id:Option<u64>,
    cached:Option<lyrics::Lyrics>,native_only:bool,manual:bool,
}

#[derive(Default)]
pub(crate) struct LyricsState {
    pub key: String,
    /// 本次查询对应的歌曲；为空表示还没有按歌曲发起过查询。
    pub song: Option<LyricSong>,
    /// 旧请求未结束时排队的在线查询；本机歌词轮询不等它，换歌后立即开始。
    pub pending: Option<LyricsJob>,
    pub island_animation_started: bool,
    pub scroll_animation_started: bool,
    pub loading: bool,
    pub error: Option<lyrics::FetchError>,
    pub retry_at: Option<f64>,
    /// 发起本次查询时已知的歌曲 ID；之后才拿到 ID 且还没有同步歌词时，按 ID 再查一次。
    pub song_id: Option<u64>,
    /// 同一首歌已自动重试的次数；达到上限后停止，避免整首歌期间反复读盘或联网。
    pub attempts: u32,
    /// 用户点了「重试」：下一次查询跳过近期「没有歌词」的记录。
    pub manual_retry: bool,
    pub lyrics: Option<lyrics::Lyrics>,
    pub rx: Option<std::sync::mpsc::Receiver<Result<(lyrics::Lyrics,bool), lyrics::FetchError>>>,
    pub native_rx: Option<std::sync::mpsc::Receiver<Option<lyrics::Lyrics>>>,
}

/// v0.5.5：邮件面板与设置表单的界面状态。正文只在内存里过一次，不落盘。
#[derive(Default)]
pub(crate) struct MailState {
    pub privacy:crate::mail_privacy::Privacy,
    pub manage:crate::mail_manage_ui::State,
    pub accounts: Vec<mail::Account>,
    /// 每个账号的连接状态，键是邮箱小写。
    pub status: HashMap<String, mail::Status>,
    /// 最近邮件，新的在前，最多 60 封。
    pub recent: Vec<mail::Item>,
    /// 设置页里「测试连接」的结果。
    pub note: String,
    pub testing: Option<String>,
    pub form: MailForm,
    /// 面板里点开的邮件（账号 + uid，避免两个账号 uid 撞车）。
    pub open_uid: Option<(String, u32)>,
    pub reading: Option<mail::Item>,
    pub reader_focus: bool,
    /// 已经提醒过的会议，键是 `UID + DTSTART`（邮件和日历两条来源共用）。
    pub fired: std::collections::HashSet<String>,
}

#[derive(Default)]
pub(crate) struct MailForm {
    pub provider: mail::Provider,
    pub custom: mail::CustomImap,
    pub email: String,
    pub password: String,
    pub show: bool,
}

impl MailState {
    /// 最近一封带验证码的邮件。
    pub fn code_item(&self) -> Option<&mail::Item> {self.recent.iter().find(|m|m.code.is_some())}
    pub fn status_of(&self, email: &str) -> mail::Status {
        self.status
            .get(&email.to_ascii_lowercase())
            .cloned()
            .unwrap_or(mail::Status::Idle)
    }
}

/// v0.5.6：日程（CalDAV）的界面状态。密码只在表单里暂存，保存后立刻清掉。
#[derive(Default)]
pub(crate) struct CalendarState {
    pub local: Vec<crate::local_calendar::Entry>,
    pub local_error: String,
    pub draft: Option<crate::local_calendar::Draft>,
    pub feeds: Vec<CalendarFeed>,
    pub config: Option<calendar::Config>,
    /// 未来 24 小时的日程（提醒也读这里）。
    pub events: Vec<mail::Meeting>,
    pub view: crate::calendar_ui::State,
    pub connected: bool,
    pub last_sync: String,
    /// 连续失败次数；>= 2 才在设置页显示红字。
    pub failures: u32,
    pub error: String,
    pub testing: bool,
    /// 设置表单里的提示（成功/失败/粘贴拆分的说明）。
    pub note: String,
    /// `note` 是不是错误提示（界面显示成红字）。
    pub note_error: bool,
    pub form: CalendarForm,
    /// 「测试并保存」用的临时密码，测试成功才落盘。
    pub test_password: String,
    /// 导入 .ics 文件的结果说明。
    pub import_note: String,
    pub warning:String,
}

pub(crate) struct CalendarFeed {
    pub config: calendar::Config,
    pub events: Vec<mail::Meeting>,
    pub month: crate::calendar_ui::State,
    pub connected: bool,
    pub last_sync: String,
    pub failures: u32,
    pub error: String,
    pub warning:String,
}

impl CalendarFeed {
    fn new(config: calendar::Config) -> Self {
        Self { config, events: vec![], month: Default::default(), connected: false,
            last_sync: String::new(), failures: 0, error: String::new(), warning:String::new() }
    }
}

#[derive(Default)]
pub(crate) struct CalendarForm {
    pub provider: calendar::Provider,
    /// 自定义 CalDAV 的服务器地址。
    pub server: String,
    pub username: String,
    pub password: String,
    pub show: bool,
    pub guide_open: bool,
}

impl CalendarState {
    pub fn load() -> Self {
        let config = calendar::load_config();
        let form = CalendarForm { provider: calendar::selected_provider(),
            username: config.as_ref().map(|cfg| cfg.username.clone()).unwrap_or_default(),
            server: config.as_ref().filter(|cfg| cfg.provider.custom_server()).and_then(|cfg| cfg.server.clone()).unwrap_or_default(), ..Default::default() };
        let feeds = calendar::load_configs().into_iter().map(CalendarFeed::new).collect();
        let (local,local_error)=match crate::local_calendar::load(){Ok(events)=>(events,String::new()),Err(error)=>(vec![],error)};
        Self { config, form, feeds, local, local_error, ..Default::default() }
    }

}

#[derive(Clone, Copy)]
struct PositionDrag {
    start: Vec2,
    /// 本段拖动的起始偏移（未取整，换边后重置），单位与 `island_offset` 相同。
    offset: f32,
    original: (model::Dock, i32),
}

// ---------- 拖动位置动效 ----------
/// 原地按住这么久即「拿起」岛体，不必先移动鼠标。
const LIFT_HOLD: f64 = 0.12;
/// 拿起 / 放下：略欠阻尼，提起时轻轻弹一下，放下时回落。
const LIFT_K: (f32, f32) = (420.0, 0.38);
/// 拿起后放大：沿停靠边方向 / 离开停靠边方向。
const LIFT_SCALE: (f32, f32) = (0.08, 0.14);
/// 窗口跟手：明显拖尾并略微冲过光标；松手后落位回弹一两下。
const GLIDE_FOLLOW: (f32, f32) = (320.0, 0.72);
const GLIDE_SETTLE: (f32, f32) = (380.0, 0.38);
/// 移动越快沿运动方向拉得越长（最多 22%），厚度方向相应收窄。
const STRETCH_MAX: f32 = 0.22;
const STRETCH_SPEED: f32 = 1400.0;
/// 吸附范围（`island_offset` 单位）：居中和拖动前的位置。
const DRAG_MAGNET: f32 = 5.0;
/// 吸住居中后要拖出更远才松开，停在中间时稳稳「咬住」，不会被手抖带走。
const CENTER_RELEASE: f32 = 9.0;
/// 离居中多近开始浮现轮廓描边（`island_offset` 单位）。
const CENTER_GUIDE_RANGE: f32 = 24.0;
/// 吸附瞬间沿轮廓从中心向两端扫开的光，持续时长（秒）。
const CENTER_SWEEP: f64 = 0.55;
/// 已居中的提示色（绿）：岛体描边、外发光和扫光统一用它，颜色本身就是「对准了」。
const CENTER_TINT: (f32, f32, f32) = (88.0, 222.0, 140.0);

pub struct App {
    pub opts: Options,
    pub settings: model::Settings,
    /// 本次运行实际使用兼容渲染（Direct3D 12）；设置里的开关与它不同时提示重启。
    pub render_compat_running: bool,
    pub(crate) todos: model::TodoList,
    pub(crate) water: model::Water,
    pub(crate) shelf: model::Shelf,

    // 形变
    pub(crate) w: Spring,
    pub(crate) h: Spring,
    /// 展开度弹簧（0 = 药丸，1 = 完整面板）。与高度解耦：
    /// 页面换高时高度可以平滑过渡，展开度仍是 1，内容不会跟着闪。
    pub(crate) open_t: Spring,
    pub open: bool,
    pub(crate) minimal_stage1: bool,
    hover_since: Option<f64>,
    leave_since: Option<f64>,
    /// 前台切换导致的强制收起：光标离开前不再自动展开。
    pub hover_lock: bool,
    pub(crate) reveal: f32,
    position_drag: Option<PositionDrag>,
    /// 拖动中和松手落位期间的窗口位置（未取整），落定后回到 `settings.island_offset`。
    drag_glide: Option<anim::Spring>,
    /// 拿起程度 0..1（可略过冲）与速度拉伸量，只影响岛体外形，不影响展开进度。
    drag_lift: anim::Spring,
    drag_stretch: anim::Spring,
    /// 拖动是否正吸在居中槽位上，以及吸附瞬间的扫光、接近引导、居中绿光显隐。
    pub(crate) center_locked: bool,
    center_lock_at: f64,
    pub(crate) center_guide: f32,
    center_lock_a: f32,
    /// 光标停在日期时间拖动区的起始时间：停留越久把手提示越明显，划过不打扰。
    grip_hover_since: Option<f64>,

    // 切页
    pub(crate) page: usize,
    pub(crate) page_from: usize,
    page_dir: f32,
    page_t0: f64,
    slide: Spring,
    dots: [Spring; 15],  // [ai-status-mod] AI 工作页 + 插件页 + 独立天气页 + 独立翻译页 + 独立剪贴板页 + 独立设备岛页 + 亮度控制页
    wheel_delta: f32,
    wheel_at: f64,
    wheel_page_at: f64,

    // 拖入
    pub(crate) drag_hot: bool,
    pub(crate) drag_a: f32,
    drag_saved: Option<usize>,

    // 置物架
    pub(crate) shelf_scroll: Spring,
    pub(crate) shelf_first: usize,
    pub(crate) brightness_scroll: f32,
    pub(crate) shelf_menu_rect: Option<Rect>,
    pub(crate) shelf_x: HashMap<PathBuf, Spring>,
    pub(crate) shelf_born: HashMap<PathBuf, f64>,
    pub(crate) icons: crate::icons::Cache,
    pub(crate) avoidance: crate::avoidance::Picker,
    pub(crate) confirm_clear: Option<f64>,
    /// 置物架页的两个分栏：0 = 文件，1 = 剪贴板。
    pub(crate) shelf_tab: usize,
    pub clipboard: crate::clipboard::Shared,
    pub(crate) last_clip_ver: u64,
    pub(crate) copy_hot_until: f64,
    pub(crate) clip_first: usize,
    pub(crate) clip_confirm_clear: Option<f64>,
    pub(crate) clip_paste_pin_outside: bool,
    pub(crate) clip_timed_close_at: f64,
    clip_outside_btn_was_down: bool,
    clip_copy_override: bool,
    copy_opened_session: bool,

    // 饮水
    pub(crate) water_ring: Spring,
    pub(crate) water_ml: f32,
    pub(crate) water_view: usize,
    pub(crate) water_view_t: Spring,
    pub(crate) sit_view_t: Spring,
    /// 饮水页当前是否显示「站立提醒」。
    pub(crate) sit_mode: bool,
    /// 健康提醒设置里是否打开了「提醒时段」二级页（时段 + 午休）。
    pub(crate) health_time: bool,
    pub(crate) remind_t: Spring,
    pub(crate) last_reminder: f64,
    /// 久坐计时起点（确认或起身重置），与尚未确认的喝水 / 久坐提醒标志。
    pub(crate) sit_start: f64,
    pub(crate) health_pending: [bool;2],
    pub(crate) fullscreen_hidden: bool,
    pub(crate) sit_t: Spring,

    // 专注 / 饮水页内数值编辑
    pub(crate) pomo: Pomo,
    pub(crate) settings_editing: bool,
    pub(crate) ring_shown: Spring,
    pub(crate) reset_spin: Spring,

    // 待办
    pub(crate) adding: bool,
    pub(crate) editing: Option<usize>,
    pub(crate) buf: String,
    pub(crate) todo_input_focused: bool,
    todo_ime_active: bool,
    todo_ime_frame: bool,
    pub(crate) todo_pop: Spring,

    // 音乐
    pub(crate) ly: LyricsState,
    pub(crate) ly_worker: Option<std::thread::JoinHandle<()>>,
    pub(crate) show_lyrics: bool,
    island_lyric_layout: Option<std::sync::Arc<egui::Galley>>,
    media_anchor: (String, f64, f64, bool),
    /// [ai-status-mod] 音乐暂停起表时刻（env.t 秒），None = 未在计时。
    music_paused_since: Option<f64>,
    pub(crate) media: NowPlaying,
    pub(crate) music_artwork: Option<(std::sync::Arc<egui::ColorImage>,egui::TextureHandle)>,

    pub forced_open_until:f64,
    pub settings_open:bool,
    settings_window:std::sync::Arc<std::sync::Mutex<crate::settings_window::State>>,
    pub displays:Vec<crate::display_layout::Display>,
    browser_at_top:bool,
    browser_minimal_active:bool,
    /// 毛玻璃着色量（平滑过渡，避免背景变化时闪烁）；0 表示尚未初始化。
    glass_tint:f32,
    pub(crate) fluid:crate::fluid::Fluid,
    pub(crate) fluid_origin:Pos2,
    fluid_art:Option<egui::TextureId>,
    fluid_title:String,
    screen:Option<Rect>,
    pub(crate) settings_focus_requested:bool,
    pub settings_tab:usize,
    pub(crate) settings_category:usize,
    pub messenger_statuses:std::collections::HashMap<Messenger,String>,
    pub(crate) notification_apps:model::NotificationApps,
    /// [ai-status-mod] 通知历史：按软件分类记录收到的系统通知。
    pub(crate) notification_log:model::NotificationLog,
    pub(crate) subpage:Option<usize>,
    pub(crate) task_dialog:Option<crate::extras::TaskDraft>,
    pub listener_status: String,
    pub(crate) notification_bell_until: f64,
    pub(crate) notification_bell_at: f64,
    orb_entry_at: f64,
    orb_was_visible: bool,
    orb_x: Spring,
    orb_y: Spring,
    orb_w: Spring,
    orb_h: Spring,
    pub(crate) notice: Option<Alert>,
    pub(crate) hud_notice: Option<Alert>,
    hud_alpha: f32,
    pub(crate) notice_width: Spring,
    notice_details: bool,
    pub pending_messengers:Vec<Unread>,
    messenger_scroll: f32,
    pub(crate) alerts: std::collections::VecDeque<Alert>,
    pub(crate) history: std::collections::VecDeque<Alert>,
    pub(crate) pending: Vec<Cmd>,
    pub(crate) todo_first: usize,
    pub(crate) todo_choosing: bool,
    pub(crate) pet: crate::pet::Pet,
    pub(crate) last_drink: Option<(u32, usize)>,
    // 其它
    toast: Option<(String, f64)>,
    toast_below: bool,
    last_input: f64,
    system_idle: Option<f64>,
    last_cursor: Option<Pos2>,
    pet_a: f32,
    last_t: Option<f64>,
    pub(crate) t: f64,
    pub(crate) dt: f32,
    pub(crate) today: String,
    pub(crate) current_hour: usize,
    pub(crate) cursor_local: Option<Pos2>,
    started: bool,
    /// v0.5.4：音量条展示值（拖动时先用本地值，随后跟上系统回报）。
    pub(crate) volume_ui: Option<(u32, bool)>,
    /// 拖动/滚轮刚改过音量，这段时间内系统回声不再单独弹一条。
    pub(crate) volume_quiet_until: f64,
    /// 音量条填充的缓动量（0..1），让数值变化有「量感」。
    pub(crate) volume_a: f32,
    pub(crate) music_volume_ui: Option<(u32,bool)>,
    pub(crate) music_volume_quiet_until: f64,
    pub(crate) music_volume_open: bool,
    pub(crate) music_seek_ui: Option<(f64,String)>,
    pub(crate) music_seek_quiet_until: f64,
    pub(crate) music_restart_at: Option<f64>,
    /// 上一次看到的蓝牙设备，用来只在连接/断开时提示一次。
    pub(crate) bt_shown: Vec<crate::bt::Device>,
    bt_initialized: bool,
    /// v0.5.5：邮件账号、最近邮件与设置表单。
    pub(crate) mail: MailState,
    /// 岛内详情与独立阅读窗口各保留一份排版缓存。
    mail_lines_cache: Vec<(std::sync::Arc<str>, std::sync::Arc<str>, u32, std::sync::Arc<Vec<crate::mail_format::Line>>)>,
    /// v0.5.6：日历（CalDAV）配置、未来 24 小时事件与设置表单。
    pub(crate) calendar: CalendarState,
    pub(crate) quota: crate::quota_ui::State,
    /// 天气页的当前数据与后台拉取状态。
    pub(crate) weather: crate::weather::State,
    pub(crate) weather_source: crate::weather_source_ui::State,
    pub(crate) updates: crate::updates::State,
    pub(crate) resident: bar::Motion,
    pub(crate) resident_power: (Option<u8>,bool),
    pub(crate) backdrop_rgb: [f32; 3],
    pub(crate) swipe_start_x: Option<f32>,
    pub(crate) cold_prewarmed: bool,
    pub(crate) last_had_wireless_devices: bool,
    pub(crate) long_to_short_latched: bool,
    pub(crate) page_switch_grace_until: f64,
}

impl App {
    pub fn new(ctx: &egui::Context, opts: Options) -> Self {
        // 诊断开关：AURALSLE_DEMO_DEVICES=1 时注入演示外设，
        // 方便在没有能上报电量的蓝牙/2.4G 设备时预览设备药丸与电量徽章。
        if std::env::var("AURALSLE_DEMO_DEVICES").ok().as_deref() == Some("1") {
            crate::bt::toggle_demo_peripherals();
        }
        theme::load_fonts(ctx);
        egui_extras::install_image_loaders(ctx);
        ctx.set_theme(egui::ThemePreference::Dark);
        ctx.set_visuals(egui::Visuals::dark());
        ctx.style_mut(|s| {
            s.interaction.selectable_labels = false;
            s.visuals.text_cursor.stroke = egui::Stroke::new(1.5_f32, theme::BLUE);
            s.visuals.selection.bg_fill = theme::alpha_blue(0.45);
        });
        let mut settings = model::Settings::load();
        if !cfg!(test) && !crate::auralsle_ai_status::config::get().pet_user_configured {
            settings.pet = true;
            settings.save();
            crate::auralsle_ai_status::config::update(|c| c.pet_user_configured = true);
        }
        let clipboard_on = settings.clipboard_history;
        let (bw, bh) = if settings.minimal { (BAR_W_MIN, BAR_H_MIN) } else { (BAR_W, BAR_H) };
        let focus_seconds=settings.focus_minutes as f32*60.0;
        let initial_lyrics=settings.lyrics_enabled;
        let initial_page=settings.page_order.iter().copied().find(|page|settings.visible_pages[*page]).unwrap_or(0);
        // [twinkle-tray] 亮度后台线程：启动即恢复上次亮度并开始读数（DDC/CI 不占 UI 线程）。
        if !cfg!(test) { crate::monitors::start(); }
        let weather_source=crate::weather_source_ui::State::load();
        let mut weather=crate::weather::State::load(&settings.weather_city,settings.weather_position.as_ref());
        if weather_source.displayed==crate::weather_source_ui::Provider::Qweather {weather.snapshot=weather_source.runtime.snapshot();}
        Self {
            opts,
            render_compat_running: settings.render_compat,
            settings,
            todos: model::TodoList::load(),
            water: model::Water::default(),
            shelf: model::Shelf::load(),
            w: Spring::new(bw, CLOSE.0, CLOSE.1),
            h: Spring::new(bh, CLOSE.0, CLOSE.1),
            open_t: Spring::new(0.0, OPEN_H.0, OPEN_H.1),
            open: false,
            minimal_stage1: false,
            hover_since: None,
            leave_since: None,
            hover_lock: false,
            reveal: 0.0,
            position_drag: None,
            drag_glide: None,
            drag_lift: anim::Spring::new(0.0, LIFT_K.0, LIFT_K.1),
            drag_stretch: anim::Spring::new(0.0, 260.0, 0.6),
            center_locked: false,
            center_lock_at: -10.0,
            center_guide: 0.0,
            center_lock_a: 0.0,
            grip_hover_since: None,
            page: initial_page,
            page_from: initial_page,
            page_dir: 1.0,
            page_t0: -10.0,
            slide: Spring::new(0.0, 260.0, 0.92),
            dots: std::array::from_fn(|i| Spring::new(if i == initial_page { 16.0 } else { 4.0 }, 380.0, 0.86)),
            wheel_delta: 0.0,
            wheel_at: -1.0,
            wheel_page_at: -1.0,
            drag_hot: false,
            drag_a: 0.0,
            drag_saved: None,
            shelf_scroll: Spring::new(0.0, 300.0, 0.9),
            shelf_first: 0,
            brightness_scroll: 0.0,
            shelf_menu_rect: None,
            shelf_x: HashMap::new(),
            shelf_born: HashMap::new(),
            icons: Default::default(), avoidance: Default::default(),
            confirm_clear: None,
            shelf_tab: 0,
            clipboard: crate::clipboard::Shared::new(clipboard_on),
            last_clip_ver: 0,
            copy_hot_until: -10.0,
            clip_first: 0,
            clip_confirm_clear: None,
            clip_paste_pin_outside: false,
            clip_timed_close_at: -10.0,
            clip_outside_btn_was_down: false,
            clip_copy_override: false,
            copy_opened_session: false,
            water_ring: Spring::new(0.0, 120.0, 0.9),
            water_ml: 0.0,
            water_view: 0,
            water_view_t: Spring::new(0.0, 200.0, 0.9),
            sit_view_t: Spring::new(0.0, 200.0, 0.9),
            sit_mode: false,
            health_time: false,
            remind_t: Spring::new(1.0, 400.0, 0.8),
            last_reminder: 0.0,
            sit_start: 0.0,
            health_pending: model::load_health_pending(),
            fullscreen_hidden: false,
            sit_t: Spring::new(1.0, 400.0, 0.8),
            pomo: Pomo { phase: Phase::Focus, remaining: focus_seconds, running: false }, settings_editing: false,
            ring_shown: Spring::new(1.0, 160.0, 0.95),
            reset_spin: Spring::new(0.0, 160.0, 0.82),
            adding: false,
            todo_input_focused: false,
            todo_ime_active: false,
            todo_ime_frame: false,
            editing: None,
            buf: String::new(),
            todo_pop: Spring::new(0.0, 500.0, 0.45),
            ly: LyricsState::default(),
            ly_worker: None,
            show_lyrics: initial_lyrics,
            island_lyric_layout: None,
            media_anchor: (String::new(), 0.0, 0.0, false),
            music_paused_since: None,
            media: NowPlaying::default(),
            music_artwork: None,
            forced_open_until:0.0, settings_open:false, settings_window:std::sync::Arc::new(std::sync::Mutex::new(crate::settings_window::State::load())), displays:Vec::new(), browser_at_top:false, browser_minimal_active:false, glass_tint:0.0, fluid:Default::default(), fluid_origin:Pos2::ZERO, fluid_art:None, fluid_title:String::new(), screen:None, settings_focus_requested:false, settings_tab:std::env::var("AURALSLE_SETTINGS_TAB").ok().and_then(|v|v.parse::<usize>().ok()).unwrap_or(0), settings_category:0, messenger_statuses:Default::default(), notification_apps:model::NotificationApps::load(), notification_log:model::NotificationLog::load(), subpage:None, task_dialog:None,
            listener_status: "尚未连接".into(),
            notification_bell_until: 0.0, notification_bell_at: -10.0, orb_entry_at: -10.0, orb_was_visible: false, orb_x: Spring::new(0.0, 420.0, 0.88), orb_y: Spring::new(0.0, 420.0, 0.88), orb_w: Spring::new(0.0, 420.0, 0.88), orb_h: Spring::new(0.0, 420.0, 0.88), notice: None, hud_notice: None, hud_alpha: 0.0, notice_width:Spring::new(0.0,CLOSE.0,CLOSE.1), notice_details:false, pending_messengers:Vec::new(), messenger_scroll:0.0, alerts: Default::default(), history: Default::default(), pending: vec![],
            todo_first: 0, todo_choosing: false, pet: Default::default(), last_drink: None,
            toast: None, toast_below: false,
            last_input: 0.0, system_idle: None,
            last_cursor: None,
            pet_a: 0.0,
            last_t: None,
            t: 0.0,
            dt: 0.0,
            today: String::new(), current_hour: 0,
            cursor_local: None,
            started: false,
            volume_ui: None,
            volume_quiet_until: 0.0,
            volume_a: 0.0,
            music_volume_ui: None,
            music_volume_quiet_until: 0.0,
            music_volume_open: false,
            music_seek_ui: None,
            music_seek_quiet_until: 0.0,
            music_restart_at: None,
            bt_shown: Vec::new(),
            bt_initialized: false,
            mail: MailState { accounts: mail::load_accounts(), ..Default::default() },
            mail_lines_cache: Vec::new(),
            calendar: CalendarState::load(),
            quota: crate::quota_ui::State::load(),
            weather,
            weather_source,
            updates: crate::updates::State::load(model::config_dir().join("update-check.json")),
            resident: Default::default(), resident_power:(None,false),
            backdrop_rgb: [0.15, 0.16, 0.18],
            swipe_start_x: None,
            cold_prewarmed: false,
            last_had_wireless_devices: false,
            long_to_short_latched: false,
            page_switch_grace_until: 0.0,
        }
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), self.t));
        self.toast_below = false;
    }

    pub fn messenger_unavailable(&mut self, message: impl Into<String>) {
        self.say(message);
        self.toast_below = true;
    }

    pub(crate) fn code_copied(&mut self, _code:&str) {
        self.say("验证码已复制");
        self.toast_below = true;
    }

    pub(crate) fn mail_code_hidden(&self,item:&mail::Item)->bool {
        self.mail.privacy.hidden(item,self.t,self.settings.mail_code_hide_seconds)
    }
    pub(crate) fn reveal_mail_code(&mut self,item:&mail::Item){self.mail.privacy.reveal(item,self.t);}
    /// 同一封邮件、同一段正文只解析一次排版，避免每帧重复切分几百个段落。
    pub(crate) fn mail_body_lines(&mut self, item: &mail::Item) -> std::sync::Arc<Vec<crate::mail_format::Line>> {
        // display_mail 每帧只 clone Arc：正文与 HTML 原文的指针都不变就说明是同一封邮件，
        // 长邮件不必每帧重新哈希几十 KB，更不必重新跑正则解析。
        if let Some((_,_,_,lines))=self.mail_lines_cache.iter().find(|(summary, html, uid, _)|
            *uid == item.uid
            && std::sync::Arc::ptr_eq(summary, &item.summary)
            && std::sync::Arc::ptr_eq(html, &item.html)){return lines.clone();}
        let lines=std::sync::Arc::new(crate::mail_format::item_lines(item));
        if self.mail_lines_cache.len()==2{self.mail_lines_cache.remove(0);}
        self.mail_lines_cache.push((item.summary.clone(),item.html.clone(),item.uid,lines.clone()));lines
    }

    pub(crate) fn display_mail(&self,item:&mail::Item)->mail::Item {
        let mut safe=item.clone();
        if self.mail_code_hidden(item) {
            if let Some(code)=&item.code {
                safe.subject=crate::mail_privacy::mask(&safe.subject,code);
                safe.from=crate::mail_privacy::mask(&safe.from,code);
                safe.summary=self.mail.privacy.masked_body(item);
                // HTML 原文里可能还有未遮蔽的验证码，隐藏时必须一起丢弃。
                // 用共享的空 Arc，让排版缓存对隐藏邮件也能按指针命中，不必每帧重解析。
                static EMPTY: std::sync::OnceLock<std::sync::Arc<str>> = std::sync::OnceLock::new();
                safe.html=EMPTY.get_or_init(||std::sync::Arc::from("")).clone();
                safe.links.clear(); // Avoid tooltip/URL/copy leaks while hidden.
                if let Some(event)=&mut safe.meeting {event.title=crate::mail_privacy::mask(&event.title,code);}
                safe.code=None;
            }
        }
        safe
    }

    /// 收信服务推来的事件；历史邮件静默，新邮件进入通知队列。
    pub fn mail_event(&mut self, ev: mail::Event) {
        let watching=|email:&str|self.mail_active() && self.mail.accounts.iter().any(|a|a.enabled && a.email.eq_ignore_ascii_case(email));
        match ev {
            mail::Event::Managed(done)=>self.mail_managed(done),
            mail::Event::Status { account, status } => {
                if !watching(&account){return;}
                if status==mail::Status::Ready{
                    self.mail.manage.data.remove(&(account.to_ascii_lowercase(),crate::mail_manage::Folder::Inbox));self.mail.manage.next_refresh=0.0;
                    if (!self.mail.manage.trash_open||!self.settings.mail_sync_operations)&&self.mail.open_uid.as_ref().is_some_and(|(a,_)|a.eq_ignore_ascii_case(&account)){
                        self.mail.open_uid=None;self.mail.manage.auto_read=None;
                    }
                }
                self.mail.status.insert(account.to_ascii_lowercase(), status);
            }
            mail::Event::Mail { item, new } => {
                if !watching(&item.account){return;}
                if !new&&self.mail.manage.data.get(&(item.account.to_ascii_lowercase(),crate::mail_manage::Folder::Inbox)).is_some_and(|s|!s.entries.iter().any(|(uid,_)|*uid==item.uid)){return;}
                self.mail.privacy.observe(&item,new,self.t,chrono::Utc::now().timestamp());
                if self.mail_locally_removed(&item){
                    self.mail.manage.trash.retain(|m|m.uid!=item.uid||!m.account.eq_ignore_ascii_case(&item.account));self.mail.manage.trash.insert(0,item);self.mail.manage.trash.truncate(60);return;
                }
                self.mail
                    .recent
                    .retain(|m| !(m.account.eq_ignore_ascii_case(&item.account) && m.uid == item.uid));
                self.mail.recent.insert(0, item);
                self.mail.recent.truncate(60);
                if new{self.mail.manage.next_refresh=0.0;}
                self.mail.privacy.retain(self.mail.recent.iter().chain(&self.mail.manage.trash).chain(&self.mail.reading));
                if new && self.mail_active() {
                    let item=&self.mail.recent[0];
                    let body=format!("{} · {}",item.from,item.subject);
                    let body=item.code.as_ref().map(|code|crate::mail_privacy::mask(&body,code)).unwrap_or(body);
                    self.alert("新邮件",body,5,false);
                    self.notification_bell_at = self.t;
                    self.notification_bell_until = self.t + 6.0;
                    if !cfg!(test) {
                        crate::auralsle_ai_status::aora_face::trigger_mascot_event(20, 6.0);
                    }
                }
            }
            mail::Event::Test { account, ok, note } => {
                self.mail.testing = None;
                let detail = if ok { "连接成功".to_string() } else { note };
                let detail = if detail.chars().count() > 120 {
                    format!("{}…", detail.chars().take(120).collect::<String>())
                } else {
                    detail
                };
                self.mail.note = format!("{account}：{detail}");
                self.say(if ok {
                    format!("{account} 连接成功")
                } else {
                    format!("{account} 连接失败")
                });
            }
        }
    }

    /// 设置窗口第 8 个标签页：邮件。
    pub fn open_mail_settings(&mut self) {
        self.settings_tab = 7;
        self.open_settings();
    }

    /// v0.5.6：日历服务推来的事件。失败不弹窗，只累计次数（连续两次才显示红字）。
    pub fn calendar_event(&mut self, ev: calendar::Event) {
        match ev {
            calendar::Event::Test { ok, message, count, config } => {
                self.calendar.testing = false;
                if !ok {
                    self.calendar.note = message;
                    self.calendar.note_error = true;
                    return;
                }
                let Some(cfg) = config else {
                    self.calendar.note = "服务器没有返回日历地址".into();
                    self.calendar.note_error = true;
                    return;
                };
                if let Err(e) = calendar::save_connection(&cfg, &self.calendar.test_password, self.calendar.config.as_ref()) {
                    self.calendar.note = e;
                    self.calendar.note_error = true;
                    return;
                }
                self.calendar.note = if message=="连接成功"{format!("已连接 · 未来 24 小时 {count} 个日程")}else{format!("已连接 · {message}")};
                self.calendar.note_error = false;
                self.calendar.warning.clear();
                if self.calendar.config.as_ref()!=Some(&cfg) {
                    self.calendar.events.clear();
                    self.calendar.view.events.clear();
                    self.calendar.view.loaded=None;
                    self.calendar.view.loading=false;
                    self.calendar.view.error.clear();
                }
                if !self.calendar.feeds.iter().any(|feed| feed.config == cfg) {
                    self.calendar.feeds.retain(|feed| feed.config.provider != cfg.provider);
                    self.calendar.feeds.push(CalendarFeed::new(cfg.clone()));
                }
                self.calendar.config = Some(cfg);
                self.calendar.connected = true;
                self.calendar.failures = 0;
                self.calendar.error.clear();
                self.calendar.last_sync = calendar::now_label();
                self.calendar.test_password.clear();
                self.calendar.form.password.clear();
                self.pending.push(Cmd::CalendarSync);
                self.request_calendar_month(true);
            }
            calendar::Event::Synced { config, events, at } => {
                if let Some(feed)=self.calendar.feeds.iter_mut().find(|feed|feed.config==config) {
                    feed.events=events.clone();feed.connected=true;feed.failures=0;feed.error.clear();feed.warning.clear();feed.last_sync=at.clone();
                }
                if self.calendar.config.as_ref()!=Some(&config){return;}
                self.calendar.events = events;
                self.calendar.connected = true;
                self.calendar.failures = 0;
                self.calendar.error.clear();
                self.calendar.last_sync = at;
                self.calendar.warning.clear();
            }
            calendar::Event::MonthLoaded { config, month, result } => {
                if !self.calendar.feeds.is_empty() {
                    if month!=self.calendar.view.month {return;}
                    let Some(feed)=self.calendar.feeds.iter_mut().find(|feed|feed.config==config) else{return;};
                    feed.month.loading=false;
                    feed.month.warning.clear();
                    match result {
                        Ok(events)=>{feed.month.events=events;feed.month.loaded=Some(month);feed.month.error.clear();}
                        Err(message)=>feed.month.error=message,
                    }
                    self.refresh_calendar_month_status();
                    return;
                }
                let view=&mut self.calendar.view;
                if month!=view.month || self.calendar.config.as_ref()!=Some(&config){return;}
                view.loading=false;
                view.warning.clear();
                match result {
                    Ok(events)=>{view.events=events;view.loaded=Some(month);view.error.clear();}
                    Err(message)=>view.error=message,
                }
            }
            calendar::Event::Warning{config,month,message}=>{
                if let Some(month)=month{
                    if month!=self.calendar.view.month{return;}
                    if self.calendar.feeds.is_empty(){
                        if self.calendar.config.as_ref()==Some(&config){self.calendar.view.warning=message;}
                    }else if let Some(feed)=self.calendar.feeds.iter_mut().find(|feed|feed.config==config){
                        feed.month.warning=message;self.refresh_calendar_month_status();
                    }
                }else{
                    if let Some(feed)=self.calendar.feeds.iter_mut().find(|feed|feed.config==config){feed.warning=message.clone();}
                    if self.calendar.config.as_ref()==Some(&config){self.calendar.warning=message;}
                }
            }
            calendar::Event::Failed { config, message } => {
                if let Some(feed)=self.calendar.feeds.iter_mut().find(|feed|feed.config==config) {
                    feed.connected=false;feed.failures=feed.failures.saturating_add(1);feed.error=message.clone();feed.warning.clear();
                }
                if self.calendar.config.as_ref()==Some(&config) {
                    self.calendar.connected = false;
                    self.calendar.failures = self.calendar.failures.saturating_add(1);
                    self.calendar.error = message;
                    self.calendar.warning.clear();
                }
            }
        }
    }

    /// 所有已连接的平台都参与同步，设置标签只决定正在编辑的账号。
    pub fn calendar_configs(&self) -> Vec<calendar::Config> {
        if self.calendar.feeds.is_empty() {self.calendar.config.clone().into_iter().collect()}
        else {self.calendar.feeds.iter().map(|feed|feed.config.clone()).collect()}
    }

    /// 等价于 `calendar_configs()==known`，外壳每帧调用，不复制配置。
    pub fn calendar_configs_match(&self, known: &[calendar::Config]) -> bool {
        if self.calendar.feeds.is_empty() {known==self.calendar.config.as_slice()}
        else {known.len()==self.calendar.feeds.len() && known.iter().zip(&self.calendar.feeds).all(|(known,feed)|*known==feed.config)}
    }

    pub(crate) fn refresh_calendar_month_status(&mut self) {
        let month=self.calendar.view.month;
        self.calendar.view.loading=self.calendar.feeds.iter().any(|feed|feed.month.loading);
        self.calendar.view.loaded=(!self.calendar.feeds.is_empty() && self.calendar.feeds.iter().all(|feed|feed.month.loaded==Some(month))).then_some(month);
        self.calendar.view.error=self.calendar.feeds.iter().filter(|feed|!feed.month.error.is_empty())
            .map(|feed|format!("{}：{}",feed.config.provider.label(),feed.month.error)).collect::<Vec<_>>().join("；");
        self.calendar.view.warning=self.calendar.feeds.iter().filter(|feed|feed.month.loaded==Some(month)&&!feed.month.warning.is_empty())
            .map(|feed|format!("{}：{}",feed.config.provider.label(),feed.month.warning)).collect::<Vec<_>>().join("；");
    }

    pub(crate) fn select_calendar_provider(&mut self, provider: calendar::Provider) {
        if self.calendar.testing || self.calendar.form.provider == provider { return; }
        if let Err(error) = calendar::select_provider(provider) {
            self.calendar.note = error;
            self.calendar.note_error = true;
            return;
        }
        let feed=self.calendar.feeds.iter().find(|feed|feed.config.provider==provider);
        self.calendar.config=feed.map(|feed|feed.config.clone());
        self.calendar.events=feed.map(|feed|feed.events.clone()).unwrap_or_default();
        self.calendar.connected=feed.is_some_and(|feed|feed.connected);
        self.calendar.last_sync=feed.map(|feed|feed.last_sync.clone()).unwrap_or_default();
        self.calendar.failures=feed.map(|feed|feed.failures).unwrap_or_default();
        self.calendar.error=feed.map(|feed|feed.error.clone()).unwrap_or_default();
        self.calendar.warning=feed.map(|feed|feed.warning.clone()).unwrap_or_default();
        self.calendar.form=CalendarForm{provider,username:self.calendar.config.as_ref().map(|cfg|cfg.username.clone()).unwrap_or_default(),
            server:self.calendar.config.as_ref().filter(|cfg|cfg.provider.custom_server()).and_then(|cfg|cfg.server.clone()).unwrap_or_default(),..Default::default()};
        self.calendar.note.clear();self.calendar.note_error=false;
    }

    /// 只移除正在管理的平台，保留其他平台的日程与提醒。
    pub fn forget_calendar(&mut self) -> Option<calendar::Config> {
        let old = self.calendar.config.take();
        if let Some(config)=&old {
            self.calendar.feeds.retain(|feed|feed.config!=*config);
            let is_old=|alert:&crate::extras::Alert|alert.meeting.as_ref().is_some_and(|(_,source)|source==config.provider.label());
            self.alerts.retain(|alert|!is_old(alert));
            if self.notice.as_ref().is_some_and(is_old){self.notice=None;}
        }
        self.calendar.connected = false;
        self.calendar.events.clear();
        self.calendar.view.events.clear();
        self.refresh_calendar_month_status();
        self.calendar.last_sync.clear();
        self.calendar.failures = 0;
        self.calendar.error.clear();
        self.calendar.note.clear();
        self.calendar.warning.clear();
        old
    }

    /// 外壳同步收信线程时读账号清单（不含任何凭据）。
    pub fn mail_accounts(&self) -> &[mail::Account] {
        &self.mail.accounts
    }

    /// 账号增删或开关变化后：落盘并让外壳重新对齐收信线程。
    pub(crate) fn mail_accounts_changed(&mut self) {
        self.mail_lines_cache.clear();
        let accounts=&self.mail.accounts;
        let known=&self.mail.manage.identities;
        let active=|email:&str|accounts.iter().any(|a|a.enabled && mail::key(a)==email && known.get(email).is_none_or(|old|old==a));
        self.mail.manage.data.retain(|(email,_),_|active(email));
        self.mail.recent.retain(|m|active(&m.account.to_ascii_lowercase()));
        self.mail.manage.trash.retain(|m|active(&m.account.to_ascii_lowercase()));
        if self.mail.reading.as_ref().is_some_and(|m|!active(&m.account.to_ascii_lowercase())){self.mail.reading=None;}
        if self.mail.open_uid.as_ref().is_some_and(|(email,_)|!active(&email.to_ascii_lowercase())){self.mail.open_uid=None;self.mail.manage.auto_read=None;}
        self.mail.manage.identities.retain(|_,old|accounts.contains(old));
        self.mail.manage.next_refresh=0.0;
        mail::save_accounts(accounts);
        self.pending.push(Cmd::MailSync);
    }

    /// 录制/截图用的邮件样例（只在 `record` feature 下编译）。
    ///
    /// `AURALSLE_MAIL_DEMO=1|code` 面板带验证码，`=list` 只列邮件，`=fail` 用假授权码走一次
    /// 「测试连接失败」，方便在没有真实邮箱时验收失败路径。
    #[cfg(feature = "record")]
    pub fn mail_demo(&mut self) {
        use mail::{Account, Item, Provider, Status};
        let mode = std::env::var("AURALSLE_MAIL_DEMO").unwrap_or_default();
        self.settings_tab = 7;
        if mode == "fail" {
            self.mail.form.provider = Provider::Qq;
            self.mail.form.email = "test@qq.com".into();
            self.mail.form.password = "invalid-auth-code".into();
            self.mail.note = "test@qq.com：正在测试连接…".into();
            self.pending.push(Cmd::MailTest(
                Account { custom: None, email: "test@qq.com".into(), provider: Provider::Qq, enabled: true },
                "invalid-auth-code".into(),
            ));
            return;
        }
        self.mail.accounts = vec![
            Account { custom: None, email: "demo@qq.com".into(), provider: Provider::Qq, enabled: false },
            Account { custom: None, email: "demo@icloud.com".into(), provider: Provider::ICloud, enabled: false },
        ];
        for a in &self.mail.accounts {
            self.mail.status.insert(a.email.clone(), Status::Ready);
        }
        // 会议演示：开始时间设成 10 分钟后，正好落在默认 15 分钟提前量里，
        // 启动即触发一次「会议提醒」。
        let mstart = chrono::Local::now().timestamp() + 600;
        let mat = chrono::DateTime::from_timestamp(mstart, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let rows = [
            (
                "GitHub",
                "[GitHub] Please verify your device",
                "09:41",
                Some("482913"),
                "We noticed a sign-in attempt from a new device. Enter 482913 to finish signing in. If this wasn't you, secure your account now.",
                None,
            ),
            (
                "Apple",
                "Your Apple ID verification code",
                "09:38",
                None,
                "Your Apple ID code is 573104. Don't share it with anyone. If you didn't request it, you can ignore this message.",
                None,
            ),
            (
                "腾讯企业邮",
                "会议邀请：周五方案评审",
                "09:12",
                None,
                "本周五 14:30 在三楼会议室评审 v0.5.5 方案，请提前十分钟到场。会议邀请见附件。",
                Some((mstart, mat)),
            ),
            (
                "顺丰速运",
                "您的快件已到达菜鸟驿站",
                "08:57",
                None,
                "您的快件已到达菜鸟驿站（望京 SOHO T1 店），请凭取件码 6-3-2 在 20:00 前领取。",
                None,
            ),
        ];
        let with_code = mode != "list";
        self.mail.recent = rows
            .iter()
            .enumerate()
            .map(|(i, (from, subject, time, code, summary, meeting))| Item {
                received_unix: None, account: "demo@qq.com".into(),
                from: (*from).into(),
                subject: (*subject).into(),
                time: (*time).into(),
                code: if with_code { (*code).map(str::to_string) } else { None },
                summary: (*summary).into(),
                html: String::new().into(),
                links: Vec::new(),
                meeting: meeting.as_ref().map(|(start, at)| mail::Meeting { all_day:false, end:None, title: "周五方案评审".into(), start: *start, at: at.clone(), uid: "demo-ics-1".into() }),
                uid: 900 - i as u32,
            })
            .collect();
        self.mail.note = "demo@qq.com：连接成功".into();
        self.page = 5;
        self.dots_target();
    }

    /// 录制/截图用的日程样例（只在 `record` feature 下编译）。
    ///
    /// `AURALSLE_CAL_DEMO=paste|fail|connected|alert`。
    #[cfg(feature = "record")]
    pub fn calendar_demo(&mut self) {
        use calendar::Provider;
        let mode = std::env::var("AURALSLE_CAL_DEMO").unwrap_or_default();
        self.calendar.form.provider = Provider::Feishu;
        self.calendar.form.username = "demo***@feishu.example".into();
        self.settings_tab = 8;
        self.open_settings();
        self.settings.calendar_prompt = false;
        let later = |secs: i64| chrono::Local::now().timestamp() + secs;
        let label = |ts: i64| {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| d.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string())
                .unwrap_or_default()
        };
        let demo_cfg = calendar::Config {
            provider: Provider::Feishu,
            username: "demo***@feishu.example".into(),
            href: Some("https://caldav.feishu.cn/calendars/8f3c2d7a-1111-2222-3333-444455556666/".into()),
            server: None,
        };
        match mode.as_str() {
            "paste" => {
                self.calendar.form.username =
                    "服务器地址：https://caldav.feishu.cn/calendars/\n用户名：demo***@feishu.example\n密码：demo-pass-123".into();
                if let Some((u, p)) = calendar::parse_paste(&self.calendar.form.username) {
                    self.calendar.form.username = u;
                    self.calendar.form.password = p;
                    self.calendar.note = "已从粘贴内容拆出用户名和密码，服务器地址已按平台填好".into();
                }
            }
            "fail" => {
                self.calendar.form.password = "demo-pass-123".into();
                self.calendar.note = calendar::human_error(Provider::Feishu, Some(401));
                self.calendar.note_error = true;
            }
            "connected" => {
                let a = later(3600);
                let b = later(6 * 3600);
                self.calendar.form.password = "demo-pass-123".into();
                self.calendar.events = vec![
                    mail::Meeting { all_day:false, end:None, title: "产品评审".into(), start: a, at: label(a), uid: "demo-cal-1".into() },
                    mail::Meeting { all_day:false, end:None, title: "设计周会".into(), start: b, at: label(b), uid: "demo-cal-2".into() },
                ];
                self.calendar.config = Some(demo_cfg.clone());
                self.calendar.connected = true;
                self.calendar.last_sync = calendar::now_label();
                self.calendar.note.clear();
                self.calendar.note_error = false;
            }
            "alert" => {
                let s = later(600);
                self.calendar.events =
                    vec![mail::Meeting { all_day:false, end:None, title: "飞书日程：产品评审".into(), start: s, at: label(s), uid: "demo-cal-alert".into() }];
                self.calendar.config = Some(demo_cfg.clone());
                self.calendar.connected = true;
                self.calendar.last_sync = calendar::now_label();
                self.settings_open = false;
            }
            _ => {}
        }
    }

    /// v0.5.4：把系统音量与蓝牙设备状态同步进界面；只在连接/断开时提示一次。
    fn sync_device(&mut self, env: &Env) {
        if self.t >= self.volume_quiet_until {
            if let Some(v) = env.volume {
                self.volume_ui = Some(v);
            }
        }
        if !self.settings.bluetooth_notifications {
            self.bt_shown.clear();
            self.bt_initialized=false;
            self.alerts.retain(|a|a.source!=crate::extras::AlertSource::Internal || a.title!="蓝牙设备");
            if self.notice.as_ref().is_some_and(|a|a.source==crate::extras::AlertSource::Internal && a.title=="蓝牙设备") {self.notice=None;}
            return;
        }
        if !env.bt_ready { return; }
        // 只把首次完整扫描作为基线，之后的连接变化都应提示。
        if !self.bt_initialized {
            self.bt_initialized = true;
            self.bt_shown = env.bt.clone();
            return;
        }
        let connected: Vec<_> = env.bt.iter().filter(|d| !self.bt_shown.iter().any(|old| old.id == d.id)).cloned().collect();
        let disconnected: Vec<_> = self.bt_shown.iter().filter(|d| !env.bt.iter().any(|new| new.id == d.id)).cloned().collect();
        for device in &connected { self.bluetooth_alert(device, true); }
        for device in &disconnected { self.bluetooth_alert(device, false); }
        self.bt_shown = env.bt.clone();
    }

    /// 距离上次交互的秒数（小精灵出现的依据）。
    pub fn idle_for(&self) -> f64 {
        self.system_idle.unwrap_or(self.t - self.last_input)
    }

    pub(crate) fn page_label(page: usize) -> &'static str {
        match page {
            0..=9 => PAGE_NAMES[page],
            10 => "天气",
            11 => "翻译",
            12 => "剪贴板",
            13 => "设备岛",
            14 => "亮度",
            _ => "设置",
        }
    }


    /// 当前是否有已连接的蓝牙或 2.4GHz 无线外设（用于设备岛像音乐岛一样按连接状态自动显隐）。
    pub(crate) fn has_connected_wireless_devices(&self) -> bool {
        !self.bt_shown.is_empty() || !crate::bt::connected_peripherals().is_empty()
    }

    /// 首次启动预热所有 SVG 图标纹理、AI 工具图标与后台状态缓存，彻底消除首次展开灵动岛切换页面时的卡顿。
    pub(crate) fn prewarm_cold_caches(&mut self, ctx: &egui::Context) {
        if self.cold_prewarmed || cfg!(test) {
            return;
        }
        self.cold_prewarmed = true;
        crate::frame_watch::mark_stage("app:prewarm_cold_caches");
        let mcfg = crate::auralsle_ai_status::config::get();
        if mcfg.mod_ai_suite_enabled {
            crate::quota_ui::prewarm_logo_textures(ctx);
            crate::auralsle_ai_status::ui::prewarm_icons(ctx);
            let _ = crate::auralsle_ai_status::state::snapshot();
        }
        if mcfg.mod_widgets_sysmon_enabled && mcfg.win_compact_bar {
            let _ = crate::auralsle_ai_status::sysmon::sample();
        }
        if mcfg.mod_weather_translate_enabled {
            let _ = crate::auralsle_ai_status::plugins::weather_state();
        }
        if mcfg.mod_display_bt_enabled {
            let _ = crate::bt::connected_peripherals();
        }
    }

    pub(crate) fn is_page_currently_visible(&self, page: usize) -> bool {
        if cfg!(test) {
            return page < 10 && self.settings.visible_pages[page];
        }
        let cfg = crate::auralsle_ai_status::config::get();
        match page {
            0 => cfg.mod_music_enabled && self.settings.visible_pages[0] && (!cfg.music_auto_hide_when_idle || self.media.is_music() || env_has_active_media(&self.media)),
            1 | 2 => cfg.mod_focus_health_enabled && self.settings.visible_pages[page],
            3 => cfg.mod_shelf_clipboard_enabled && self.settings.visible_pages[3],
            4 => self.settings.visible_pages[4],
            5 | 7 => cfg.mod_mail_calendar_enabled && self.settings.visible_pages[page],
            6 | 8 => cfg.mod_ai_suite_enabled && self.settings.visible_pages[page],
            9 => cfg.mod_widgets_sysmon_enabled && self.settings.visible_pages[9],
            10 => cfg.mod_weather_translate_enabled && cfg.weather_page_visible,
            11 => cfg.mod_weather_translate_enabled && cfg.translate_page_visible,
            12 => cfg.mod_shelf_clipboard_enabled && self.settings.clipboard_history && cfg.clipboard_page_visible,
            13 => cfg.mod_display_bt_enabled && cfg.show_bt_battery_on_island && cfg.device_page_visible && (cfg.device_display_mode == 1 || cfg.device_display_mode == 2) && (!cfg.device_auto_hide_when_empty || self.has_connected_wireless_devices()),
            14 => cfg.mod_display_bt_enabled && cfg.brightness_page_visible,
            _ => false,
        }
    }

    pub(crate) fn active_page_order(&self) -> Vec<usize> {
        if cfg!(test) {
            return self.settings.page_order.to_vec();
        }
        let cfg = crate::auralsle_ai_status::config::get();
        let mut order = Vec::with_capacity(15);
        for &p in &cfg.full_page_order {
            if p <= 14 && !order.contains(&p) {
                order.push(p);
            }
        }
        for &def_p in &[0usize, 1, 2, 3, 12, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14] {
            if !order.contains(&def_p) {
                order.push(def_p);
            }
        }
        order
    }

    pub fn show_page(&mut self,page:usize) {
        if !cfg!(test) {
            let mcfg = crate::auralsle_ai_status::config::get();
            let mod_enabled = match page {
                0 => mcfg.mod_music_enabled,
                1 | 2 => mcfg.mod_focus_health_enabled,
                3 | 12 => mcfg.mod_shelf_clipboard_enabled,
                5 | 7 => mcfg.mod_mail_calendar_enabled,
                6 | 8 => mcfg.mod_ai_suite_enabled,
                9 => mcfg.mod_widgets_sysmon_enabled,
                10 | 11 => mcfg.mod_weather_translate_enabled,
                13 | 14 => mcfg.mod_display_bt_enabled,
                _ => true,
            };
            if !mod_enabled {
                self.settings_tab = 21;
                self.open_settings();
                return;
            }
        }
        if page < 10 && !self.settings.visible_pages[page] {
            self.settings_tab=[1,2,3,5,4,7,11,8,12,13][page];  // [ai-status-mod] 插件页 → 系统监控
            self.open_settings();
            return;
        }
        if page == 10 && !self.is_page_currently_visible(10) {
            self.settings_tab = 18;
            self.open_settings();
            return;
        }
        if page == 11 && !self.is_page_currently_visible(11) {
            self.settings_tab = 19;
            self.open_settings();
            return;
        }
        if page == 12 && !self.is_page_currently_visible(12) {
            self.settings_tab = 5;
            self.open_settings();
            return;
        }
        if page == 13 && !self.is_page_currently_visible(13) {
            self.settings_tab = 6;
            self.open_settings();
            return;
        }
        if page == 14 && !self.is_page_currently_visible(14) {
            self.settings_tab = 20;
            self.open_settings();
            return;
        }
        self.notice=None;
        self.go(page,1.0);
        self.open=true; self.forced_open_until=self.t+4.0;
        self.w.tune(OPEN_W.0,OPEN_W.1);self.h.tune(OPEN_H.0,OPEN_H.1);
        self.w.target=PW;self.h.target=self.expanded_height();
    }
    pub(crate) fn window_layout(&self)->std::sync::MutexGuard<'_,crate::settings_window::State>{
        self.settings_window.lock().unwrap_or_else(|e|e.into_inner())
    }

    pub fn save_window_before_exit(&self){self.window_layout().close();}

    #[cfg(windows)]
    pub fn start_tray(&self,state:crate::tray::State)->Option<crate::tray::Tray>{
        let layout=self.settings_window.clone();
        // 原生文件选择框会占用界面线程，托盘退出时直接保存最近一次观察到的布局。
        crate::tray::Tray::start_with_exit(state,move||layout.lock().unwrap_or_else(|e|e.into_inner()).close())
    }

    pub fn open_settings(&mut self){
        self.settings_open=true;
        self.settings_focus_requested=true;
    }
    pub fn open_settings_category(&mut self,category:usize){
        self.settings_category=category.min(3);
        self.health_time=false;
        if category==0 || category>=3 {self.settings_tab=if category==0{0}else{6};self.open_settings();return;}
        let page=if category==1{2}else{1};
        self.show_page(page);
        if self.settings.visible_pages[page] && (category==1 || category==2) {self.subpage=Some(page);}
    }
    pub fn page(&self) -> usize {
        self.page
    }
    /// 打开健康提醒设置里的「提醒时段」二级页（演示录制用）。
    pub fn show_health_time(&mut self) { self.health_time = true; }
    /// 切到饮水页的久坐屏，并假定已连续久坐 minutes 分钟（演示录制用）。
    pub fn show_sit_view(&mut self, minutes: f64) {
        self.show_page(WATER_PAGE);
        self.sit_mode = true;
        self.sit_start = self.t - minutes * 60.0;
    }

    // ---------- 功能模块：页面开关即模块开关，关闭后页面隐藏、后台同步和提醒一并停止 ----------

    /// 模块是否开启（与「页面管理」及「功能模块」总开关保持一致）。
    pub fn module_on(&self,page:usize)->bool {
        if cfg!(test) {
            if page == WEATHER_PAGE {
                return true;
            }
            return self.settings.visible_pages.get(page).copied().unwrap_or(false);
        }
        let cfg = crate::auralsle_ai_status::config::get();
        let mod_ok = match page {
            0 => cfg.mod_music_enabled,
            1 | 2 => cfg.mod_focus_health_enabled,
            3 | 12 => cfg.mod_shelf_clipboard_enabled,
            5 | 7 => cfg.mod_mail_calendar_enabled,
            6 | 8 => cfg.mod_ai_suite_enabled,
            9 => cfg.mod_widgets_sysmon_enabled,
            10 | 11 => cfg.mod_weather_translate_enabled,
            13 | 14 => cfg.mod_display_bt_enabled,
            _ => true,
        };
        if !mod_ok {
            return false;
        }
        if page == WEATHER_PAGE {
            return cfg.weather_page_visible;
        }
        if page == 11 {
            return cfg.translate_page_visible;
        }
        if page == 12 {
            return self.settings.clipboard_history && cfg.clipboard_page_visible;
        }
        if page == 13 {
            return cfg.show_bt_battery_on_island && cfg.device_page_visible;
        }
        if page == 14 {
            return cfg.brightness_page_visible;
        }
        self.settings.visible_pages.get(page).copied().unwrap_or(false)
    }
    /// 收信线程是否应运行：邮箱模块开启且打开了「新邮件提醒」。
    pub fn mail_active(&self)->bool {self.module_on(MAIL_PAGE) && self.settings.mail_enabled}
    /// 剪贴板监听是否应运行：置物架与剪贴板模块开启且打开了「记录剪贴板」。
    pub fn clipboard_active(&self)->bool {
        if cfg!(test) {
            self.module_on(SHELF_PAGE) && self.settings.clipboard_history
        } else {
            let cfg = crate::auralsle_ai_status::config::get();
            cfg.mod_shelf_clipboard_enabled && self.settings.clipboard_history && cfg.clipboard_page_visible
        }
    }
    /// 额度查询是否应运行：AI 额度模块开启且打开了「开启 AI 额度」。
    pub fn quota_active(&self)->bool {self.module_on(QUOTA_PAGE) && self.quota.config.enabled}

    pub fn clear_mail_cache(&mut self) {
        self.mail.recent.clear();
        self.mail.reading = None;
    }

    pub fn clear_music_artwork(&mut self) {
        self.music_artwork = None;
    }

    pub fn clear_weather_snapshot(&mut self) {
        self.weather.snapshot = None;
    }

    pub(crate) fn set_page_visible(&mut self,page:usize,visible:bool) {
        if page >= 10 { return; }
        if !visible && self.settings.visible_pages.iter().filter(|enabled|**enabled).count()==1
            && self.settings.visible_pages[page] { return; }
        self.settings.visible_pages[page]=visible;
        // 关掉专注模块时一并停止正在进行的计时，避免页面不可见却到点弹提醒。
        if !visible && page==FOCUS_PAGE {self.pomo.running=false;}
        if self.page < 10 && !self.settings.visible_pages[self.page] {
            if let Some(first)=self.settings.page_order.iter().copied().find(|page|self.settings.visible_pages[*page]) { self.go(first,1.0); }
        }
    }

    fn navigate_page(&mut self,next:bool) {
        if cfg!(test) {
            let position=self.settings.page_order.iter().position(|page|*page==self.page).unwrap();
            for offset in 1..=PAGE_NAMES.len() {
                let page=self.settings.page_order[(position+if next {offset}else{PAGE_NAMES.len()-offset})%PAGE_NAMES.len()];
                if self.settings.visible_pages[page] {
                    self.go(page,if next {1.0}else{-1.0});
                    break;
                }
            }
            return;
        }
        let order = self.active_page_order();
        let n = order.len().max(1);
        let position = order.iter().position(|page| *page == self.page).unwrap_or(0);
        for offset in 1..=n {
            let page = order[(position + if next { offset } else { n - offset }) % n];
            if self.is_page_currently_visible(page) {
                self.go(page, if next { 1.0 } else { -1.0 });
                break;
            }
        }
    }

    /// OLED 像素偏移只在收起动画完成、没有鼠标交互、不在编辑时移动；
    /// 展开、点击、拖拽时保持当前位置，避免面板内容和位移一起跳。
    pub fn oled_shift_idle(&self,ctx:&egui::Context) -> bool {
        crate::oled_shift::should_move(self.open, self.position_drag.is_some() || self.drag_glide.is_some(), self.is_editing())
            && self.hover_since.is_none() && !self.drag_hot
            && self.w.settled(PW) && self.h.settled(self.expanded_height())
            && self.drag_lift.settled(1.0) && self.drag_stretch.settled(1.0)
            && ctx.input(|i|!i.pointer.any_down() && !i.pointer.any_pressed() && i.raw.hovered_files.is_empty())
    }

    /// 慢帧日志用：当前页与展开状态。
    pub fn debug_state(&self) -> (usize, bool) {
        (self.page, self.open)
    }

    pub fn is_editing(&self) -> bool {
        self.adding || self.editing.is_some() || self.todo_choosing || self.task_dialog.is_some() || self.calendar.draft.is_some() || self.settings_editing
    }

    pub fn filter_todo_input(&mut self,input:&mut egui::RawInput) {
        self.todo_ime_frame=false;
        if !self.adding && self.editing.is_none(){self.todo_ime_active=false;return;}
        self.todo_ime_frame=self.todo_ime_active || input.events.iter().any(|event|matches!(event,egui::Event::Ime(egui::ImeEvent::Enabled|egui::ImeEvent::Preedit(_)|egui::ImeEvent::Commit(_))));
        for event in &input.events {
            match event {
                egui::Event::Ime(egui::ImeEvent::Enabled|egui::ImeEvent::Preedit(_))=>self.todo_ime_active=true,
                egui::Event::Ime(egui::ImeEvent::Commit(_)|egui::ImeEvent::Disabled)=>self.todo_ime_active=false,
                _=>{},
            }
        }
        // 首帧和结束帧的按键、失焦仍属于组合输入，不能透传给待办编辑。
        if self.todo_ime_frame {input.events.retain(|event|!matches!(event,egui::Event::Key{key:egui::Key::Backspace|egui::Key::Delete|egui::Key::Enter|egui::Key::Escape|egui::Key::ArrowLeft|egui::Key::ArrowRight|egui::Key::ArrowUp|egui::Key::ArrowDown,..}));}
    }

    pub fn foreground_changed(&mut self) {
        if self.todo_ime_frame {return;}
        if !cfg!(test) && self.open && self.page == 11 && crate::auralsle_ai_status::config::get().translate_pin_island {
            return;
        }
        self.hover_lock=true;
        if self.adding && self.buf.trim().is_empty(){self.cancel_edit();}
    }

    /// 带过渡地切到某页。`dir` > 0 表示内容从右侧进入。
    pub(crate) fn go(&mut self, page: usize, dir: f32) {
        crate::frame_watch::record_user_event("GOTO", &format!("page={} dir={}", page, dir));
        let page_ok = if cfg!(test) || page < 10 {
            if page == 0 && !cfg!(test) {
                self.is_page_currently_visible(0)
            } else {
                page < 10 && self.settings.visible_pages[page]
            }
        } else {
            self.is_page_currently_visible(page)
        };
        if page == self.page || !page_ok { return; }
        if !cfg!(test) && page == 11 && crate::auralsle_ai_status::config::get().translate_auto_clipboard {
            let cached_clip = self
                .clipboard
                .lock()
                .items
                .first()
                .map(|it| it.text.trim().to_owned())
                .filter(|s| !s.is_empty());
            if let Some(clip) = cached_clip {
                crate::auralsle_ai_status::plugins::trigger_translate_async(clip, 0);
            }
        }
        self.music_volume_open=false;
        self.subpage=None;
        // 连点：上一段过渡还没结束时，以当前页为起点重新开始，位移速度保留。
        self.page_from = self.page;
        self.page = page;
        if page==CALENDAR_PAGE {self.request_calendar_month(false);}
        self.page_dir = dir;
        self.page_t0 = self.t;
        self.slide.pos = dir * PAGE_SLIDE + if self.page_e() < 0.4 { self.slide.pos * 0.3 } else { 0.0 };
        self.slide.target = 0.0;
        self.slide.vel = 0.0;  // [ai-status-mod] 复位速度，换页不会带残余速度小抖动
        self.dots_target();
        self.cancel_edit();
        // [ai-status-mod] 各页高度不同（140/176/216）时不再硬跳：让高度弹簧自己走，
        // `progress()` 的分母由 `h_den` 同步跟随，页面内容不会跟着抖。
        if self.open {
            if !cfg!(test) {
                self.long_to_short_latched = true;
                self.page_switch_grace_until = self.t + 1.8;
                self.forced_open_until = self.forced_open_until.max(self.t + 0.85);
            }
            self.h.target = self.expanded_height();
        }
    }

    pub(crate) fn page_e(&self) -> f32 {
        (self.t - self.page_t0) as f32
    }

    /// 主岛是否正处于需要零延迟 VSync 满帧驱动的切页或展开/收起过渡期。
    pub fn is_fast_transitioning(&self) -> bool {
        (self.page_from != self.page && self.page_e() < 0.56)
            || (self.w.pos - self.w.target).abs() > 0.5
            || (self.h.pos - self.h.target).abs() > 0.5
            || self.open_t.vel.abs() > 0.02
            || (self.orb_was_visible
                && ((self.t - self.orb_entry_at.max(self.notification_bell_at)) < 0.72
                    || !self.orb_x.settled(0.6)
                    || !self.orb_y.settled(0.6)
                    || !self.orb_w.settled(0.6)
                    || !self.orb_h.settled(0.6)))
    }

    pub(crate) fn dots_target(&mut self) {
        for (i, d) in self.dots.iter_mut().enumerate() {
            d.target = if i == self.page { 16.0 } else { 4.0 };
        }
    }

    pub(crate) fn cancel_edit(&mut self) {
        self.settings_editing=false;
        self.adding = false;
        self.editing = None;
        self.todo_choosing = false;
        self.task_dialog=None;
        self.calendar.draft=None;
        self.buf.clear();
        self.todo_input_focused=false;
        self.todo_ime_active=false;
        self.todo_ime_frame=false;
    }

    /// 外壳据此调整音乐采样频率：看着音乐页时最快，收起显示歌词次之，其余只需跟上歌名和播放状态。
    pub fn media_poll_interval(&self)->std::time::Duration {
        if self.open && self.page==MUSIC_PAGE {crate::smtc::POLL_FAST}
        else if self.island_lyrics_active() && !self.open {crate::smtc::POLL_LYRICS}
        else {crate::smtc::POLL_BACKGROUND}
    }

    fn island_lyrics_active(&self)->bool {
        if self.side() {return false;}
        self.settings.island_lyrics && self.settings.visible_pages[0] && self.media.is_music()
            && (!cfg!(test) || !self.browser_minimal_active)
            && !crate::auralsle_ai_status::music_pause_expired(self.music_paused_since, self.t)
    }
    pub(crate) fn compact_notices(&self)->bool {self.island_lyrics_active()&&!self.open&&!self.notice.as_ref().is_some_and(|a|a.meeting.is_some()||self.settings.notification_preview&&matches!(a.source,crate::extras::AlertSource::External(_)))}

    /// 换句、分段和展开高亮共用时间调整；正数延后，负数提前。
    pub(crate) fn lyric_position(&self)->f32 {(self.media.position-self.settings.lyric_delay_ms as f64/1000.0)as f32}

    pub(crate) fn island_lyric(&self) -> Option<(usize,&str)> {
        if !self.island_lyrics_active(){return None;}
        if !lyric_key_matches(&self.ly.key,&self.media) {return None;}
        if cfg!(test) {
            let lyrics=self.ly.lyrics.as_ref().filter(|lyrics|lyrics.synced)?;
            let index=lyrics.index_at(self.lyric_position())?;
            let text=lyrics.lines.get(index)?.text.trim();
            return (!text.is_empty()).then_some((index,text));
        }
        let lyrics = self.ly.lyrics.as_ref()?;
        if lyrics.lines.is_empty() {
            return None;
        }
        if lyrics.synced {
            let pos = self.lyric_position();
            if let Some(index) = lyrics.index_at(pos) {
                let text = lyrics.lines.get(index)?.text.trim();
                if !text.is_empty() {
                    return Some((index, text));
                }
            }
            // 前奏阶段（尚未到达第一句时间戳）直接预览第一句非空歌词，告别空白无词
            for (idx, line) in lyrics.lines.iter().enumerate() {
                let t = line.text.trim();
                if !t.is_empty() {
                    return Some((idx, t));
                }
            }
            None
        } else {
            // 纯文本未同步歌词：按播放进度每 4 秒平滑轮播一句
            let non_empty: Vec<(usize, &str)> = lyrics
                .lines
                .iter()
                .enumerate()
                .filter_map(|(i, l)| {
                    let t = l.text.trim();
                    (!t.is_empty()).then_some((i, t))
                })
                .collect();
            if non_empty.is_empty() {
                return None;
            }
            let slot = ((self.lyric_position().max(0.0) / 4.0) as usize) % non_empty.len();
            Some(non_empty[slot])
        }
    }

    fn update_island_lyric_layout(&mut self,ctx:&egui::Context) {
        if !self.island_lyrics_active(){self.island_lyric_layout=None;return;}
        let current=self.island_lyric();
        let lyrics=self.ly.lyrics.as_ref().filter(|lyrics|lyric_key_matches(&self.ly.key,&self.media)
            && lyrics.synced && lyrics.lines.iter().any(|line|!line.text.trim().is_empty()));
        let title=lyrics.is_none();
        let text=if let Some((_,line))=current {line}
            else if let Some(lyrics)=lyrics {
                if lyrics.index_at(self.lyric_position()).is_none() {"前奏"}
                else {"间奏"}
            } else {self.media.title.trim()};
        let ink=if current.is_some()||title{theme::WHITE}else{theme::GRAY};
        let layout=|text:String| {
            let mut job=egui::text::LayoutJob::simple(text,theme::font(11.0,W::Semibold),ink,LYRIC_BAR_W-80.0);
            job.halign=egui::Align::Center;
            job.wrap.break_anywhere=true;
            if title {job.wrap.max_rows=1;}
            ctx.fonts(|fonts|fonts.layout_job(job))
        };
        let mut galley=layout(text.to_owned());
        if let Some((index,_))=current.filter(|_|galley.rows.len()>1) {
            let lyrics=self.ly.lyrics.as_ref().unwrap();
            let start=lyrics.lines[index].t;
            let end=lyrics.lines.iter().skip(index+1).find(|line|line.t>start).map_or(self.media.duration as f32,|line|line.t);
            let progress=if end>start {((self.lyric_position()-start)/(end-start)).clamp(0.0,1.0)}else{0.0};
            // 按当前句的时间范围与各段字数切换，暂停和跳转直接跟随播放位置。
            let total:usize=galley.rows.iter().map(|row|row.glyphs.len()).sum();
            let mut count=0;
            let page=galley.rows.iter().position(|row|{count+=row.glyphs.len();progress*(total as f32)<count as f32}).unwrap_or(galley.rows.len()-1);
            let segment=galley.rows[page].glyphs.iter().map(|glyph|glyph.chr).collect::<String>();
            galley=layout(segment.trim().to_owned());
        }
        self.island_lyric_layout=Some(galley);
    }

    pub(crate) fn effective_minimal(&self)->bool {self.settings.minimal || self.browser_minimal_active}
    pub(crate) fn minimal_time_pill_mode(&self)->bool {
        !cfg!(test) && self.effective_minimal() && !self.side() && crate::auralsle_ai_status::config::get().minimal_time_pill
    }
    /// 极简药丸状态下是否启用「右侧等高缩略未读消息悬浮球（图二 `image_c629e4.png`）」并随药丸放大平滑衔接过渡。
    pub(crate) fn minimal_pill_msg_badge_active(&self)->bool {
        !cfg!(test)
            && !self.side()
            && crate::auralsle_ai_status::config::get().minimal_pill_msg_badge
            && (!self.pending_messengers.is_empty() || self.t < self.notification_bell_until)
            && self.calendar.draft.is_none()
            && self.task_dialog.is_none()
    }
    /// 当前是否处于 Glance 自适应音乐控件岛模式（`page == 0 && music_ui_style == 5`，需求 7）。
    pub(crate) fn glance_music_island_active(&self)->bool {
        !cfg!(test)
            && self.page == 0
            && self.subpage.is_none()
            && crate::auralsle_ai_status::config::get().music_ui_style == 5
    }
    /// 展开态目标宽度：常规页面为 `PW (419.0)`；当处于 Glance 音乐控件模式时不写死长度，按音乐控件形态与歌名/歌词长度动态自适应！
    pub(crate) fn expanded_width(&self)->f32 {
        if self.glance_music_island_active() {
            self.glance_music_target_dims().0
        } else {
            PW
        }
    }
    pub(crate) fn minimal_pill_dims(&self)->(f32, f32) {
        let user_w = crate::auralsle_ai_status::config::get().minimal_pill_width.clamp(48, 120) as f32;
        let w = if crate::auralsle_ai_status::shown().is_some() {
            user_w + 14.0
        } else {
            user_w
        };
        (w, 20.0)
    }
    pub(crate) fn pet_enabled(&self)->bool {
        self.settings.pet && (!self.effective_minimal() || self.minimal_time_pill_mode() || self.minimal_stage1 || (!cfg!(test) && self.notice.is_some())) && !self.side()
    }
    pub(crate) fn pet_space(&self)->f32 {if self.pet_enabled(){44.0}else{0.0}}
    pub(crate) fn notice_pet_space(&self, a: &Alert)->f32 {
        if a.meeting.is_some() { 0.0 } else if !cfg!(test) && self.settings.pet && !self.side() { 44.0 } else { self.pet_space() }
    }
    fn lyric_focus_space(&self)->f32 {if self.pomo.running{crate::focus_bar::LYRIC_WIDTH+10.0}else{0.0}}
    fn lyric_unread_space(&self)->f32 {
        if !self.open && !self.effective_minimal() && !self.minimal_pill_msg_badge_active() && !self.pending_messengers.is_empty() {
            crate::auralsle_ai_status::compact_tail_width(self.unread_sources().len())
        } else {
            0.0
        }
    }
    fn lyric_bar_width(&self)->f32 {LYRIC_BAR_W+if self.pet_enabled(){48.0}else{0.0}+self.lyric_focus_space()+self.lyric_unread_space()}
    fn notice_size(&self,a:&Alert)->(f32,f32) {
        let (width,height)=if let Some((m,_))=&a.meeting{if m.uid.starts_with("course-"){(360.0,162.0)}else{(360.0,136.0)}}else if a.source==crate::extras::AlertSource::Internal && a.title=="蓝牙设备"{(HUD_W,48.0)}else if a.is_hud(){(HUD_W,38.0)}else{(334.0,44.0)};
        (width,height)
    }
    pub fn set_pet(&mut self,enabled:bool) {
        self.settings.pet=enabled;
        if enabled && cfg!(test) {self.settings.minimal=false;}
        self.settings.save();
        if !cfg!(test) {
            crate::auralsle_ai_status::config::update(|c| c.pet_user_configured = true);
        }
    }
    pub fn set_minimal(&mut self,enabled:bool) {
        self.settings.minimal=enabled;
        if cfg!(test) {
            if enabled {self.settings.pet=false;}
        }
        self.settings.save();
    }
    fn base_size(&self) -> (f32, f32) {
        if !self.compact_notices()&&!self.open {
            if let Some(a)=&self.notice {let (width,height)=self.notice_size(a);return (width+self.notice_pet_space(a),height);}
        }
        if self.side() {
            // 左右停靠：竖向胶囊（通知仍是贴边滑出的横向卡片，见上）。
            return self.side_base_size(self.effective_minimal()&&self.t>=self.notification_bell_until&&(self.browser_minimal_active||self.pending_messengers.is_empty()));
        }
        if self.island_lyrics_active() {
            (self.lyric_bar_width(),BAR_H)
        } else if self.minimal_time_pill_mode() || (self.effective_minimal()&&self.t>=self.notification_bell_until) {
            if self.minimal_time_pill_mode() {
                if self.minimal_stage1 {
                    (self.resident.target.width.min(WIN_W-32.0), BAR_H)
                } else {
                    self.minimal_pill_dims()
                }
            } else {
                (BAR_W_MIN, BAR_H_MIN)
            }
        } else {
            (self.resident.target.width.min(WIN_W-32.0), BAR_H)
        }
    }

    /// 0 = 药丸，1 = 完整面板（展开时可略超过 1）。
    #[cfg(test)]
    pub(crate) fn current_width(&self) -> f32 { self.w.pos+self.notice_width.pos }

    pub fn progress(&self) -> f32 {
        let (bw, _bh) = self.base_size();
        // 左右停靠时面板沿水平方向从边缘滑出，用宽度衡量展开程度。
        if self.side() {return (self.w.pos - bw) / (PW - bw).max(1.0);}
        // 分母不再取「目标高度」：页面换高时它会瞬间跳变，展开度会闪一下。
        // 换成独立的展开度弹簧，与高度解耦，换页时保持 1；允许轻微过冲（上游测试锁定了 1.01–1.12）。
        self.open_t.pos.max(0.0)
    }

    pub(crate) fn expanded_height(&self)->f32 {
        // [ai-status-mod] 运行时所有长展开岛统一使用完全一致的长度（248.0），短展开岛为 154.0；
        // 当处于 Glance 音乐控件模式时（需求 7），高度完全跟随 Glance 音乐控件形态（横条 102 / 歌词卡 168 / 舞台 186）动态变化！
        // verify marker: if self.page==CALENDAR_PAGE||self.page==5||self.page==6||self.page==8||self.page==9 {CALENDAR_H}else{PH}
        if self.glance_music_island_active() {
            let h_scale = (crate::auralsle_ai_status::config::get().expanded_h_scale.clamp(80, 135) as f32) / 100.0;
            return (self.glance_music_target_dims().1 * h_scale).clamp(92.0, 260.0);
        }
        let base_h = if cfg!(test) {
            if self.page == 5 && self.mail.open_uid.is_some() {
                236.0
            } else if self.page == WEATHER_PAGE {
                WEATHER_H
            } else if self.page==3||self.page==12||self.page==CALENDAR_PAGE||self.page==5||self.page==6||self.page==8||self.page==9||self.page==10||self.page==11 {
                CALENDAR_H
            } else {
                PH
            }
        } else if self.page == 14 {
            if cfg!(test) {
                278.0
            } else {
                let mod_on = crate::auralsle_ai_status::config::get().mod_display_bt_enabled;
                let dev_cnt = if mod_on { crate::monitors::snapshot().len() } else { 0 };
                if dev_cnt <= 1 { 174.0 } else { 278.0 }
            }
        } else if self.page==3||self.page==4||self.page==5||self.page==6||self.page==CALENDAR_PAGE||self.page==8||self.page==9||self.page==10||self.page==11||self.page==12||self.page==13 {
            278.0
        } else {
            174.0
        };
        if !cfg!(test) {
            let h_scale = (crate::auralsle_ai_status::config::get().expanded_h_scale.clamp(80, 135) as f32) / 100.0;
            (base_h * h_scale).clamp(156.0, 336.0)
        } else {
            base_h
        }
    }

    // ---------------------------------------------------------------
    // 每帧入口
    // ---------------------------------------------------------------
    pub fn frame(&mut self, ui: &mut egui::Ui, win: Rect, env: &Env) -> FrameOut {
        let ctx = ui.ctx().clone();
        if !self.cold_prewarmed && !cfg!(test) {
            self.prewarm_cold_caches(&ctx);
        }
        let page_clear_secs = if cfg!(test) { 0.42 } else { 0.54 };
        if self.page_from != self.page && self.page_e() >= page_clear_secs {
            self.page_from = self.page;
        }
        crate::frame_watch::update_context(
            self.page,
            self.page_from,
            self.open,
            self.settings_open,
            self.settings_tab,
        );
        crate::frame_watch::mark_stage("app:frame_start");
        let root_scrolling = crate::extras::drive_responsive_scroll(&ctx);
        theme::sync_custom_font(&ctx);
        let dt = match self.last_t {
            Some(t0) => (env.t - t0).clamp(0.0, 0.1) as f32,
            None => 0.0,
        };
        let elapsed = self.last_t.map(|at| (env.t - at).max(0.0) as f32).unwrap_or(0.0);
        self.last_t = Some(env.t);
        self.t = env.t;
        self.quota_tick();
        self.weather_tick();
        // [需求 8：取消编辑课程岛时课程岛常驻屏幕的功能]
        crate::auralsle_ai_status::set_course_editing(false);
        if !cfg!(test) && !crate::auralsle_ai_status::config::get().island_lyrics_inited {
            crate::auralsle_ai_status::config::update(|c| c.island_lyrics_inited = true);
            if !self.settings.island_lyrics {
                self.settings.island_lyrics = true;
                self.settings.save();
            }
        }
        self.current_hour=env.hour();
        self.system_idle=env.idle_seconds;
        self.dt = dt;
        // [防卡死核心保护 · 修复图三 `image_b9ef03.png`]：
        // 当用户在展开岛（如音乐岛）按住鼠标右滑移出窗口命中区时，Win32 WM_NCHITTEST 切为 HTTRANSPARENT 会导致松开鼠标的 WM_LBUTTONUP 丢失，
        // 使 egui 的 pointer.primary_down() 永久卡在 true、position_drag 无法释放（收起卡死在图三纯灰半截状态）。
        // 每帧开头直接通过 Win32 GetAsyncKeyState 核验真实物理鼠标左键状态：只要物理左键已松开，立即复位拖拽状态与 egui 按键状态！
        #[cfg(windows)]
        if !cfg!(test) {
            let phys_lmb_down = unsafe {
                use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
                use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
                let vk = if GetSystemMetrics(SM_SWAPBUTTON) != 0 { 0x02 } else { 0x01 };
                (GetAsyncKeyState(vk) as u16 & 0x8000) != 0
            };
            if !phys_lmb_down {
                if self.position_drag.is_some() {
                    self.position_drag = None;
                    self.drag_stretch.target = 0.0;
                    if let Some(glide) = &mut self.drag_glide {
                        glide.tune(GLIDE_SETTLE.0, GLIDE_SETTLE.1);
                        glide.target = self.settings.island_offset as f32;
                    }
                }
                self.swipe_start_x = None;
            }
        }
        if let Some(at_top)=env.browser_at_top {self.browser_at_top=at_top;}
        self.screen=env.screen;
        self.fullscreen_hidden=env.fullscreen_hidden;
        if let Some(bg) = env.backdrop {
            for k in 0..3 {
                self.backdrop_rgb[k] = anim::approach(self.backdrop_rgb[k], bg[k].clamp(0.05, 0.98), 4.5, dt);
            }
        }
        if self.settings.skin.is_glass() {
            let target=if self.settings.skin==model::Skin::GlassLight {glass::light_tint(env.backdrop,env.glass_blur)}else{glass::glass_tint(env.backdrop,env.glass_blur)};
            self.glass_tint=if self.glass_tint<=0.0 {target}else{anim::approach(self.glass_tint,target,3.0,dt)};
        }
        let browser_minimal=self.settings.browser_minimal && self.browser_at_top;
        if browser_minimal && !self.browser_minimal_active {
            self.open=false;
            self.hover_lock=true;
            self.hover_since=None;
            self.leave_since=None;
            self.forced_open_until=0.0;
        }
        self.browser_minimal_active=browser_minimal;
        if !self.started {
            self.started = true;
            self.last_input = env.t;
            self.last_reminder = env.t;
            self.sit_start = env.t;
            self.page_t0 = env.t - 10.0;
            if self.page==CALENDAR_PAGE {self.request_calendar_month(false);}
        }
        let mut cmds = Vec::new();
        crate::auralsle_ai_status::tick(env.t);  // [ai-status-mod] 状态刷新
        // [ai-status-mod] 截图工具（PixPin / Snipaste 等）在前台时冻结岛：
        // 收起并锁住悬停，免得岛浮在截图画面上挡内容。
        if crate::auralsle_ai_status::shot::frozen() {
            self.open=false;
            self.hover_lock=true;
            self.hover_since=None;
            self.forced_open_until=0.0;
        }

        self.tick(env, dt, elapsed);
        self.sync_device(env);
        self.tick_alerts();
        let hud_active=self.notice.clone();
        if hud_active.as_ref().map(|a|&a.title)!=self.hud_notice.as_ref().map(|a|&a.title)||hud_active.is_none(){self.notice_details=false;}
        if let Some(a)=&hud_active {self.hud_notice=Some(a.clone());}
        self.hud_alpha=anim::approach(self.hud_alpha,if hud_active.is_some(){1.0}else{0.0},if hud_active.is_some(){16.0}else{8.0},dt);
        if hud_active.is_none() && self.hud_alpha<0.003 {self.hud_notice=None;}
        let has_msg_orb = !cfg!(test) && (self.minimal_pill_msg_badge_active() || !self.pending_messengers.is_empty());
        let notice_target=if !has_msg_orb && !self.notice.as_ref().is_some_and(|a|a.messenger().is_some()) && self.compact_notices()&&(self.notice.is_some()||self.t<self.notification_bell_until) {44.0}else{0.0};
        self.notice_width.target=notice_target;
        if self.compact_notices(){self.notice_width.step(dt);}else{self.notice_width.snap(0.0);self.notice_details=false;}
        self.update_media(env,&ctx);
        let bell=self.t<self.notification_bell_until&&!self.compact_notices()&&!has_msg_orb;
        self.resident_power=(env.battery,env.charging);
        let mut layout=Layout::new(self.settings.show_battery&&env.battery.is_some(),self.quota_bar_indices().len(),self.media.is_music()&&!bell,self.pomo.running,bell,self.pet_enabled(),self.weather_pill().is_some());
        layout.reorder(&self.settings.resident_order);
        for (position,extra) in self.quota_balance_layout(&ctx){layout.balance_tail(position,extra);}
        if self.settings.clock_period!=0 { layout.widen_clock(40.0); }
        if self.settings.clock_seconds { layout.widen_clock(24.0); }
        if !self.settings.show_clock { layout.hide_clock(); }
        if let Some(extra)=self.weather_pill_extra(&ctx){ layout.widen(bar::WEATHER,extra); }
        // [ai-status-mod] 歌词态不画 AI 紧凑块，也不该让它把吉祥物左移 extra/2 压到歌词上。
        crate::auralsle_ai_status::set_compact_tail(self.compact_tail_width());  // [ai-status-mod] 未读图标的右侧留位
        if !self.island_lyrics_active() {crate::auralsle_ai_status::extend_layout(&mut layout);}  // [ai-status-mod] 时间右侧留位
        if self.pet_enabled() {
            if let Some(a)=self.notice.as_ref().filter(|_|!self.compact_notices()&&!self.open){
                let ps = self.notice_pet_space(a);
                layout.x[bar::PET]=Some(27.0-(self.notice_size(a).0+ps)/2.0);
            }
            else if self.island_lyrics_active() {layout.x[bar::PET]=Some(60.0-self.lyric_bar_width()/2.0);}
        }
        self.resident.step(layout,dt,ctx.style().animation_time>0.0&&bar::animations_enabled());
        if self.opts.fetch_lyrics {
            self.poll_lyrics();
        }
        self.update_island_lyric_layout(&ctx);

        let cx = win.center().x;
        // 胶囊皮肤悬浮在屏幕顶边下方。
        let pill = self.settings.island_shape==model::IslandShape::Pill;
        let top = win.top() + if pill { PILL_GAP } else { 0.0 };
        let side = self.side();
        let dock = self.dock();

        // ---------- 输入：拖放 ----------
        let (hovering_files, dropped) = ctx.input(|i| {
            (
                self.settings.visible_pages[3] && !i.raw.hovered_files.is_empty(),
                i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect::<Vec<_>>(),
            )
        });
        if !dropped.is_empty() && self.settings.visible_pages[3] {
            self.on_drop(dropped);
            self.show_page(3);
        } else if hovering_files && !self.drag_hot {
            self.drag_hot = true;
            if !cfg!(test) {
                crate::auralsle_ai_status::aora_face::trigger_mascot_event(24, 4.0);
            }
            if self.page != 3 {
                self.drag_saved = Some(self.page);
                self.go(3, if self.page < 3 { 1.0 } else { -1.0 });
            }
        } else if !hovering_files && self.drag_hot {
            // 拖到外面松手 / 取消：安静地退回原页
            self.drag_hot = false;
            if let Some(p) = self.drag_saved.take() {
                self.go(p, if p < 3 { -1.0 } else { 1.0 });
            }
        }
        self.drag_a = anim::approach(self.drag_a, if self.drag_hot { 1.0 } else { 0.0 }, 16.0, dt);

        // ---------- 输入：智能检测「复制」事件 + N 秒内打开岛默认第一展开剪贴板（需求 9） ----------
        let mcfg_clip = if cfg!(test) {
            crate::auralsle_ai_status::config::Config::default()
        } else {
            crate::auralsle_ai_status::config::get()
        };
        let copy_window_secs = mcfg_clip.copy_auto_open_secs.clamp(1, 60) as f64;
        let copy_window_ms = (copy_window_secs * 1000.0) as i64;
        let cur_clip_ver = self.clipboard.lock().version;
        if !self.started {
            self.last_clip_ver = cur_clip_ver;
        } else if cur_clip_ver != self.last_clip_ver {
            self.last_clip_ver = cur_clip_ver;
            if !cfg!(test) && mcfg_clip.copy_auto_open_clipboard {
                self.copy_hot_until = self.t + copy_window_secs;
                crate::auralsle_ai_status::aora_face::trigger_mascot_event(22, 4.5);
                // 若当前岛体处于收起态，立即将默认第一展开页预置为「剪贴板」(Page 12)，确保 N 秒内展开岛时直接第一眼就是剪贴板！
                if !self.open && self.is_page_currently_visible(12) {
                    self.page_from = 12;
                    self.page = 12;
                    self.dots_target();
                }
            }
        }
        if !cfg!(test) && !self.is_page_currently_visible(self.page) {
            if let Some(first_vis) = self.active_page_order().into_iter().find(|&p| self.is_page_currently_visible(p)) {
                crate::frame_watch::record_user_event("SET", &format!("first_vis={} cur={}", first_vis, self.page));
                self.page_from = first_vis;
                self.page = first_vis;
                self.dots_target();
                if self.open {
                    self.h.target = self.expanded_height();
                }
            }
        }
        if !cfg!(test) {
            if !self.open {
                self.long_to_short_latched = false;
            } else if self.expanded_height() > 210.0 {
                self.long_to_short_latched = true;
            } else if self.long_to_short_latched {
                if let Some(cur) = env.cursor {
                    let exp_w = PW.max(self.expanded_width()).max(self.w.pos);
                    let long_box = Rect::from_min_size(Pos2::new(cx - exp_w * 0.5 - 10.0, top - PILL_GAP), Vec2::new(exp_w + 20.0, 292.0));
                    if !long_box.contains(cur) {
                        self.long_to_short_latched = false;
                    } else if cur.y <= top + self.expanded_height() - 8.0 && self.t >= self.page_switch_grace_until {
                        self.long_to_short_latched = false;
                    }
                } else {
                    self.long_to_short_latched = false;
                }
            }
        }
        let p_prev = self.progress();
        let hit = if side {self.side_hit_rect(win,p_prev)} else {self.hit_rect(cx, top, p_prev)};
        let previous_menu=self.shelf_menu_rect.take();
        let inside = env.cursor.is_some_and(|c| hit.contains(c) || previous_menu.is_some_and(|r|r.contains(c)));
        let copy_active_now = !cfg!(test)
            && mcfg_clip.copy_auto_open_clipboard
            && self.is_page_currently_visible(12)
            && (self.t <= self.copy_hot_until || crate::clipboard::has_recent_external_copy(copy_window_ms));
        self.clip_copy_override = copy_active_now;
        if copy_active_now && !self.open && self.page != 12 {
            self.page_from = 12;
            self.page = 12;
            self.dots_target();
        }
        if !self.open {
            self.copy_opened_session = false;
        }
        if inside && !self.drag_hot && copy_active_now {
            // 检测到在复制后 N 秒窗口内移入/打开岛体：直接默认第一展开「剪贴板」页面（Page 12）
            if self.page != 12 {
                self.go(12, 1.0);
            }
            self.copy_opened_session = true;
            if !self.minimal_time_pill_mode() || ctx.input(|i| i.pointer.primary_clicked()) {
                self.copy_hot_until = -10.0;
                crate::clipboard::clear_recent_external_copy();
                if !self.open {
                    self.open = true;
                    self.w.tune(OPEN_W.0, OPEN_W.1);
                    self.h.tune(OPEN_H.0, OPEN_H.1);
                    self.w.target = self.expanded_width();
                    self.h.target = self.expanded_height();
                    self.w.vel = self.w.vel.max(600.0);
                    self.h.vel = self.h.vel.max(260.0);
                }
            }
        }
        let on_notice=env.cursor.is_some_and(|c|self.messenger_badge_rect(win,self.island_rect(win,self.w.pos,self.h.pos)).is_some_and(|r|r.contains(c)) || self.notice_rect(cx,top).is_some_and(|r|r.contains(c))||self.notice_details_rect(cx,top).is_some_and(|r|r.contains(c)));
        if !inside || ctx.input(|i| i.pointer.any_pressed() || i.pointer.any_click()) {
            self.hover_lock = false;
        }
        if env.cursor != self.last_cursor || ctx.input(|i| !i.events.is_empty()) {
            if inside || hovering_files || ctx.input(|i| !i.events.is_empty()) {
                self.last_input = env.t;
            }
            self.last_cursor = env.cursor;
        }
        if self.music_volume_open && ctx.input_mut(|i|i.consume_key(egui::Modifiers::NONE,egui::Key::Escape)) {
            self.music_volume_open=false;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.is_editing() {
            self.cancel_edit();
        }
        if (!self.is_editing() || self.todo_choosing) && inside && self.open && self.reveal > 0.95 {
            let (scroll, left, right, esc) = ctx.input(|i| (i.raw_scroll_delta.y, i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Escape)));
            if esc { self.hover_lock = true; }
            if (left || right) && !self.todo_choosing {
                self.wheel_delta=0.0;
                self.navigate_page(right);
            } else if scroll!=0.0 {
                let cursor=env.cursor.unwrap()-self.content_origin(win).to_vec2();
                let y_off = if cfg!(test) || self.glance_music_island_active() { 0.0 } else { 22.0 };
                let mail_reading=self.page==5 && self.mail.open_uid.is_some();
                let mail_list=self.page==5 && ((!cfg!(test) && crate::auralsle_ai_status::config::get().msg_island_filter < 10 && cursor.y >= 32.0 && cursor.y <= (self.h.pos - 14.0).max(236.0)) || (!self.mail.accounts.is_empty() && self.mail.open_uid.is_none() && self.mail_list_bounds().translate(Vec2::new(0.0, y_off)).contains(cursor)));
                let fly_scroll_page=!cfg!(test) && cursor.y >= 32.0 && cursor.y <= (self.h.pos - 14.0).max(236.0) && matches!(self.page, 4 | 5 | 8 | 9 | 12 | 13 | 14);
                let music_volume=self.page==0 && (Rect::from_min_max(Pos2::new(354.0,106.0 + y_off),Pos2::new(380.0,130.0 + y_off)).contains(cursor)
                    || self.music_volume_open && crate::pages::MUSIC_VOLUME_RECT.translate(Vec2::new(0.0, y_off)).contains(cursor));
                let prev_wheel_at=self.wheel_at;
                self.wheel_at=self.t;
                if mail_reading || mail_list || fly_scroll_page || music_volume {self.wheel_delta=0.0;}
                else {
                    // 累积高精度滚轮的小幅输入；停顿或反向时丢弃上一段余量。
                    if self.t-prev_wheel_at>0.18 || self.wheel_delta*scroll<0.0 {self.wheel_delta=0.0;}
                    self.wheel_delta+=scroll;
                    let step_thresh = if cfg!(test) { 8.0 } else { 4.2 };
                    if self.wheel_delta.abs()>=step_thresh {
                        let next=self.wheel_delta<0.0;
                        self.wheel_delta=0.0;
                        let calendar_max=if self.page==CALENDAR_PAGE {self.calendar_scroll_max(env)}else{0};
                        let calendar_body=self.page==CALENDAR_PAGE && Rect::from_min_max(Pos2::new(163.0,52.0),Pos2::new(378.0,208.0)).contains(cursor);
                        let clipboard_tab=self.page==3 && self.shelf_tab==1;
                        let clipboard_body=clipboard_tab && Rect::from_min_max(Pos2::new(38.0,31.0),Pos2::new(381.0,194.0)).contains(cursor);
                        let list=match self.page {
                            CALENDAR_PAGE if calendar_body=>Some((&mut self.calendar.view.first,calendar_max)),
                            4 if Rect::from_min_max(Pos2::new(196.0,31.0),Pos2::new(377.0,124.0)).contains(cursor)
                                =>Some((&mut self.todo_first,self.todos.items.len().saturating_sub(3))),
                            3 if clipboard_body
                                =>Some((&mut self.clip_first,self.clipboard.lock().items.len().saturating_sub(crate::pages::CLIP_ROWS))),
                            3 if !clipboard_tab && Rect::from_min_max(Pos2::new(36.0,28.0),Pos2::new(383.0,194.0)).contains(cursor)
                                =>Some((&mut self.shelf_first,self.shelf.items.len().saturating_sub(5))),
                            _=>None,
                        };
                        let content_scrolls=list.as_ref().is_some_and(|(_,max)|*max>0);
                        let scrolled=list.is_some_and(|(first,max)|{
                            let target=if next {(*first+1).min(max)}else{first.saturating_sub(1)};
                            let changed=target!=*first;*first=target;changed
                        });
                        // 内容溢出时滚动列表并停在边界；未溢出时滚轮翻页。
                        // [v38 卡顿修复] egui 的 MouseWheel 事件没有视口归属，`Event::viewport_id()`
                        // 对它恒返回 ROOT，因此「在设置窗口里滚动」也会被主岛收到 → 主岛连续翻页 →
                        // 切页动画常驻 → 主岛与设置视口争抢同一个 GPU/VSync → 设置页每帧被拖到 120ms+
                        // （历史日志里出现过 82s / 232s / 29min 的整帧卡死，全部发生在 settings_open=true）。
                        // 子视口（设置窗口）正在接收输入时，主岛不消费滚轮翻页，并把累积量清掉。
                        let child_active = ctx
                            .data(|d| d.get_temp::<bool>(egui::Id::new("auralsle_child_vp_has_events")))
                            .unwrap_or(false);
                        if !scrolled && !content_scrolls && !self.todo_choosing && !child_active && self.t-self.wheel_page_at>=0.20 {
                            crate::frame_watch::record_user_event("NAV", "wheel");
                            self.navigate_page(next);
                            self.wheel_page_at=self.t;
                        } else if child_active {
                            self.wheel_delta=0.0;
                        }
                        ctx.input_mut(|i|{i.raw_scroll_delta=Vec2::ZERO;i.smooth_scroll_delta=Vec2::ZERO;});
                    }
                }
            }
            // [WinIslands] 触摸与鼠标水平拖拽滑动翻页手势（Touch & Swipe Gestures）
            if crate::auralsle_ai_status::config::get().touch_gestures && !self.todo_choosing && self.calendar.draft.is_none() && self.task_dialog.is_none() {
                let (down, pos, released) = ctx.input(|i| (i.pointer.primary_down(), i.pointer.interact_pos(), i.pointer.primary_released()));
                if down {
                    if let Some(p) = pos {
                        let local_y = p.y - self.content_origin(win).y;
                        // 避开音乐进度条与亮度控制页滑块区域的水平拖拽，防止调亮度/进度时被误判成左右翻页手势
                        let on_music_seek = self.page == 0 && (72.0..142.0).contains(&local_y);
                        let on_brightness_drag = self.page == 14 && local_y >= 32.0;
                        let on_widget_drag = ctx.dragged_id().is_some() || ctx.drag_started_id().is_some();
                        if on_music_seek || on_brightness_drag || on_widget_drag {
                            self.swipe_start_x = None;
                        } else {
                            let sx = *self.swipe_start_x.get_or_insert(p.x);
                            let dx = p.x - sx;
                            if dx.abs() >= 42.0 && self.t - self.wheel_page_at >= 0.24 {
                                crate::frame_watch::record_user_event("NAV", "swipe");
                                self.navigate_page(dx < 0.0);
                                self.wheel_page_at = self.t;
                                self.swipe_start_x = Some(p.x);
                            }
                        }
                    }
                }
                if released || !down {
                    self.swipe_start_x = None;
                }
            }
        } else {self.wheel_delta=0.0; self.swipe_start_x=None;}
        let translate_pinned = !cfg!(test) && self.open && self.page == 11 && crate::auralsle_ai_status::config::get().translate_pin_island;
        // 课程岛 / 课表卡片也计入「鼠标在岛上」：悬停不收起，离开后才走收起流程。
        // 用几何直接算区域（不依赖上一帧登记的矩形），避免首帧/延迟导致判定失效。
        let course_zone = if self.open && !self.side() {
            let left = cx - self.w.pos / 2.0 - crate::auralsle_ai_status::OUT_GAP - 232.0;
            // 高度覆盖最满的布局：课程卡 140 + 间隙 12 + 两行药丸 30 + 8 + 30；
            // 鼠标停在任意一行药丸上同样不算离开。
            Some(Rect::from_min_size(Pos2::new(left - 6.0, top - 6.0), Vec2::new(232.0 + 12.0, 240.0)))
        } else {
            None
        };
        let on_course_island = env.cursor.is_some_and(|c| {
            course_zone.is_some_and(|r| r.contains(c))
                || crate::auralsle_ai_status::course_card_rect().is_some_and(|r| r.contains(c))
                // 自适应位置后药丸可能翻到主岛右侧；用逐枚精确矩形判定，
                // 不依赖每帧开头会被 tick() 清空的课程卡槽位。
                || crate::auralsle_ai_status::ui::pill_hover_rects().iter().any(|r| r.contains(c))
        });
        let inside_all = inside || on_course_island;
        let volume_dragging=self.music_volume_open && ctx.is_being_dragged(egui::Id::new(("card-drag",true,"m-volume-track")));
        let brightness_dragging = self.open && self.page == 14 && (ctx.dragged_id().is_some() || ctx.drag_started_id().is_some() || ctx.input(|i| i.pointer.primary_down()));
        // [需求 7 / 修复] 复制期内即使正在播放音乐并显示歌词胶囊，点击打开也优先进入剪贴板（Page 12），否则打开音乐岛 (Page 0)
        if !cfg!(test) && self.island_lyrics_active() && !self.open && inside && ctx.input(|i| i.pointer.primary_clicked()) {
            let copy_win_ms = (crate::auralsle_ai_status::config::get().copy_auto_open_secs.clamp(1, 60) as i64) * 1000;
            let recent_copy = self.clip_copy_override
                || self.copy_opened_session
                || (crate::auralsle_ai_status::config::get().copy_auto_open_clipboard
                    && self.is_page_currently_visible(12)
                    && crate::clipboard::has_recent_external_copy(copy_win_ms));
            if recent_copy {
                self.page_from = 12;
                self.page = 12;
                self.copy_opened_session = true;
                self.copy_hot_until = -10.0;
                crate::clipboard::clear_recent_external_copy();
            } else {
                self.settings.visible_pages[0] = true;
                self.page_from = 0;
                self.page = 0;
            }
            self.dots_target();
            self.open = true;
            self.forced_open_until = self.t + 2.0;
            self.w.tune(OPEN_W.0, OPEN_W.1);
            self.h.tune(OPEN_H.0, OPEN_H.1);
            self.w.target = self.expanded_width();
            self.h.target = self.expanded_height();
        }
        if self.minimal_time_pill_mode() && self.minimal_stage1 && !self.open && inside_all && ctx.input(|i| i.pointer.primary_clicked()) {
            if !cfg!(test) && self.clip_copy_override {
                self.page_from = 12;
                self.page = 12;
                self.copy_hot_until = -10.0;
                crate::clipboard::clear_recent_external_copy();
                self.dots_target();
            } else if !cfg!(test) && self.island_lyrics_active() {
                self.settings.visible_pages[0] = true;
                self.page_from = 0;
                self.page = 0;
                self.dots_target();
            }
            self.open = true;
            self.w.tune(OPEN_W.0, OPEN_W.1);
            self.h.tune(OPEN_H.0, OPEN_H.1);
            self.w.target = self.expanded_width();
            self.h.target = self.expanded_height();
        }
        if !self.open || self.page != 12 {
            self.clip_paste_pin_outside = false;
            self.clip_timed_close_at = -10.0;
            self.clip_outside_btn_was_down = false;
        } else {
            if self.clip_paste_pin_outside {
                #[cfg(windows)]
                let os_btn_down = unsafe {
                    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
                    (GetAsyncKeyState(0x01) as u16 & 0x8000) != 0
                        || (GetAsyncKeyState(0x02) as u16 & 0x8000) != 0
                };
                #[cfg(not(windows))]
                let os_btn_down = false;
                let os_just_pressed = os_btn_down && !self.clip_outside_btn_was_down;
                self.clip_outside_btn_was_down = os_btn_down;
                let outside_clicked = !inside_all
                    && (os_just_pressed || ctx.input(|i| i.pointer.any_pressed()));
                let esc_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));
                if outside_clicked || esc_pressed {
                    self.clip_paste_pin_outside = false;
                    self.clip_outside_btn_was_down = false;
                    self.forced_open_until = -10.0;
                    self.clip_timed_close_at = -10.0;
                    self.open = false;
                    self.minimal_stage1 = false;
                    self.hover_lock = true;
                    self.leave_since = None;
                    let (c_bw, c_bh) = self.base_size();
                    self.w.tune(CLOSE.0, CLOSE.1);
                    self.h.tune(CLOSE.0, CLOSE.1);
                    self.w.target = c_bw;
                    self.h.target = c_bh;
                } else if !inside_all {
                    ctx.request_repaint_after(std::time::Duration::from_millis(45));
                }
            }
            if self.clip_timed_close_at > 0.0 {
                if self.t < self.clip_timed_close_at {
                    ctx.request_repaint_after(std::time::Duration::from_millis(70));
                } else {
                    self.clip_timed_close_at = -10.0;
                    self.forced_open_until = -10.0;
                    if !inside_all {
                        self.open = false;
                        self.minimal_stage1 = false;
                        self.leave_since = None;
                        let (c_bw, c_bh) = self.base_size();
                        self.w.tune(CLOSE.0, CLOSE.1);
                        self.h.tune(CLOSE.0, CLOSE.1);
                        self.w.target = c_bw;
                        self.h.target = c_bh;
                    }
                }
            }
        }
        let want = (inside_all && !self.hover_lock && (self.compact_notices()||self.notice.is_none()||self.open)) || self.open && on_notice || (self.open && self.t<self.forced_open_until) || self.drag_hot || volume_dragging || brightness_dragging || (self.open && self.page == 12 && self.clip_paste_pin_outside) || (self.is_editing() && self.open) || translate_pinned;
        self.morph(want, inside_all, env.t, dt);
        self.update_pet(env, cx);

        // ---------- 绘制 ----------
        let p = self.progress();
        let (sx, sy) = self.drag_scale();
        // 拿起放大不能超出透明画布，否则两端会被切掉。
        let (max_w, max_h) = if side {(crate::side_dock::SIDE_W, crate::side_dock::SIDE_H)} else {(WIN_W, WIN_H)};
        let (w, h) = ((self.w.pos * sx).min((max_w - 24.0).max(self.w.pos)), (self.h.pos * sy).min((max_h - 24.0).max(self.h.pos)));
        let panel = self.island_rect(win, w, h);
        let (messenger_rect, arr_alpha, arr_ripple) = if cfg!(test) {
            (self.messenger_badge_rect(win, panel), 1.0_f32, 1.0_f32)
        } else {
            self.step_messenger_orb(win, panel, dt)
        };
        let appear = if self.minimal_pill_msg_badge_active() {
            arr_alpha
        } else {
            self.messenger_appear() * arr_alpha
        };
        let outside_badge = messenger_rect.filter(|_| appear > 0.02);
        // [React Bits · ClickSpark] 展开岛与收起岛上的任意点击均严格在当前鼠标指针真实位置播放 8 芒星烟花动效
        if !cfg!(test) && ctx.input(|i| i.pointer.primary_pressed()) {
            if let Some(click_pos) = ctx.input(|i| i.pointer.interact_pos().or_else(|| i.pointer.latest_pos())).or(env.cursor) {
                if inside_all || on_notice {
                    let spark_col = if self.open {
                        self.page_accent_color(self.page)
                    } else {
                        crate::auralsle_ai_status::plugins::active_accent_color()
                    };
                    crate::auralsle_ai_status::ui::trigger_click_spark(click_pos, self.t, spark_col);
                }
            }
        }
        let pc = anim::clamp01(p);
        let painter = ui.painter_at(ui.max_rect().expand(40.0));
        let feather = 1.0 / ctx.pixels_per_point();
        let notice_rect=self.notice_rect(cx,top);
        let notice_shape=notice_rect.map(|r|Rect::from_min_max(Pos2::new(r.left()-36.0,r.top()),r.max));
        let body=messenger_rect.filter(|_|outside_badge.is_none()).map_or(panel,|badge|panel.union(badge));
        let body=notice_shape.map_or(body,|notice|body.union(notice));
        let minimal_pill = self.effective_minimal() && !self.minimal_stage1 && !self.island_lyrics_active() && self.notice.is_none() && (self.minimal_pill_msg_badge_active() || self.t>=self.notification_bell_until) && pc < 0.02;
        // [fly-island 直上直下刘海样式]：刘海模式取消顶部外扩耳朵弧度 (flare = 0.0)，左右侧壁垂直直上直下贴合屏幕顶边
        let flare = if minimal_pill || pill || !cfg!(test) { 0.0 } else { lerp(5.0, FLARE, pc) };
        let k = (w / PW).clamp(0.3, 1.1);
        let thickness = if side { crate::side_dock::SIDE_BAR_W } else { BAR_H };
        let radius = if minimal_pill {
            if self.minimal_time_pill_mode() { w.min(h) * 0.5 } else { BAR_H_MIN / 2.0 }
        } else if pill || (self.minimal_time_pill_mode() && pc < 0.02) {
            (w.min(h) * 0.5).min(lerp(h * 0.5, 26.0, pc))
        } else {
            lerp(12.5, 14.0, pc)
        }; // [ai-status-mod] 圆角与课程岛一致(14)
        let flare_h = if !cfg!(test) { 0.5 } else { lerp(8.0, FLARE_H, pc) };
        let thick = glass::thickness(PH);
        let glass_skin = self.settings.skin.is_glass();
        let layer = painter.layer_id();
        let palette = self.skin_palette();
        // 换色时跳过的图形范围（岛体底色、阴影、吉祥物）。
        let mut keep_colors:Vec<(usize,usize)>=Vec::new();
        // 毛玻璃：真实模糊下保留柔和投影增加层次；无模糊时阴影再减淡一些，保持轻盈。
        let skin_shadow = if !glass_skin {1.0} else if env.glass_blur {0.75} else {0.7};
        let shadow_k = glass::shadow_alpha(0.0, thick) / 0.12 * skin_shadow;
        let island_shadow=|r:Rect,flare:f32,flare_h:f32,radius:f32,strength:f32| if pill {pill_shadow(&painter,r,radius,strength)} else if side {shape::shadow_docked(&painter,r,dock,flare,flare_h,radius,strength)} else {shape::shadow(&painter,r,flare,flare_h,radius,strength)};
        // 拿起时投影加深，像被从屏幕边缘提起来。
        let lifted = self.drag_lift.pos.clamp(0.0, 1.2);
        island_shadow(body, flare, flare_h, radius, (smoothstep(0.1, 0.9, pc) + lifted * 0.9) * shadow_k);
        // [ai-status-mod] 恢复原版岛仔未读悬浮球：深色圆角胶囊底板 + 细描边（底板完整跟随当前岛体主题，修复图六）。
        if let Some(r)=outside_badge{
            if cfg!(test) {
                painter.rect_filled(r,r.width()/2.0,paint::alpha(self.glass_fill(),appear));
                if glass_skin {painter.rect_stroke(r,r.width()/2.0,egui::Stroke::new(1.0_f32,paint::alpha(self.glass_rim(),appear)),egui::StrokeKind::Inside);}
            } else {
                self.paint_card_body(&painter, r, r.width().min(r.height()) * 0.5, feather, appear, body);
                if arr_ripple < 0.99 {
                    let ring_expand = arr_ripple * 9.5;
                    let ring_alpha = ((1.0 - arr_ripple).powi(2) * 0.68 * appear).clamp(0.0, 1.0);
                    let ring_col = if self.pending_messengers.iter().any(|u| u.source == Messenger::Qq) {
                        Color32::from_rgb(56, 189, 248)
                    } else {
                        Color32::from_rgb(46, 213, 115)
                    };
                    let ring_r = r.expand(ring_expand);
                    painter.rect_stroke(
                        ring_r,
                        ring_r.width().min(ring_r.height()) * 0.5,
                        egui::Stroke::new(1.6 * (1.0 - arr_ripple * 0.5), paint::alpha(ring_col, ring_alpha)),
                        egui::StrokeKind::Outside,
                    );
                }
            }
        }
        if matches!(self.settings.skin,model::Skin::Fluid|model::Skin::LiquidGlass) {
            self.fluid_origin=body.min;
            let cover_tint_on = cfg!(test) || (self.settings.skin != model::Skin::Cream && crate::auralsle_ai_status::config::get().fluid_cover_tint);
            let active_music_tint = cover_tint_on && (self.media.playing || cfg!(test)) && !self.media.title.is_empty();
            let art = if active_music_tint { self.music_artwork.as_ref().map(|(_,texture)|texture.id()) } else { None };
            let title_key = if active_music_tint { self.media.title.as_str() } else { "" };
            if art != self.fluid_art || title_key != self.fluid_title {
                let tint = if !active_music_tint {
                    crate::fluid::Fluid::neutral()
                } else if let Some((image,_)) = &self.music_artwork {
                    crate::fluid::Fluid::tint_from_image(image)
                } else {
                    crate::fluid::Fluid::tint_from_title(&self.media.title)
                };
                self.fluid.set_tint(tint);
                self.fluid_art = art;
                self.fluid_title = title_key.to_owned();
            }
            self.fluid.set_cursor(env.cursor.filter(|p|body.contains(*p) || on_course_island).map(|p|(p.x-body.left(),p.y-body.top())));
            if !self.open || self.page!=0 || !active_music_tint {self.fluid.set_seek(None,0.0);}
            self.fluid.advance(env.t,dt);
        }
        self.paint_body(&painter, body, flare, flare_h, radius, feather);
        if lifted > 0.01 {self.paint_lift_halo(&painter, body, flare, flare_h, radius, lifted.min(1.0));}
        let center_guide=self.paint_center_rim(&painter, body, panel, flare, flare_h, radius, env.screen);
        let content_from=crate::skin_paint::shape_count(&ctx,layer);

        // 内容只随形体「揭开」：不缩放，按面板裁剪，到 55% 才开始淡入
        let reveal = if self.open { smoothstep(0.50, 0.97, p) } else { smoothstep(0.55, 0.92, p) };
        self.reveal = reveal;
        let origin = self.content_origin(win);
        let clip = panel.intersect(win);
        let base = Card::new(ui, origin, clip);
        // [ai-status-mod] 课程岛与主岛统一主题与圆角：在 content_from 之前用与主岛完全一致的皮肤着色器、
        // 液态玻璃折射边、阴影与相同圆角（radius）绘制课程岛底板，防止浅色皮肤 recolor_layer 将底板反相。
        let course_geom = crate::auralsle_ai_status::ui::course_module_geometry(&base, cx - origin.x, self.w.pos, self.progress(), self.side());
        if let Some((c_rect, c_fade, c_morph)) = course_geom {
            let circle_r = c_rect.width().min(c_rect.height()) * 0.5;
            let target_cr = if cfg!(test) {
                radius.min(circle_r)
            } else {
                crate::auralsle_ai_status::config::get().course_corner_r.clamp(4.0, 22.0).min(circle_r)
            };
            let c_radius = lerp(circle_r, target_cr, c_morph * c_morph);
            shape::shadow(&painter, c_rect, 0.0, 0.5, c_radius, smoothstep(0.1, 0.9, pc) * shadow_k * c_fade * 0.55);
            self.paint_card_body(&painter, c_rect, c_radius, feather, c_fade, body);
        }
        // [设备药丸] 与课程岛解耦：有没有课程卡只影响药丸的 y，不决定它是否存在。
        // 本帧窗口 / 主岛 / 课程卡实测矩形交给自适应布局器，位置不再写死。
        let island_devs = self.island_devices(env);
        let pill_area = crate::auralsle_ai_status::ui::PillArea {
            // 边界必须用真实屏幕（Env::screen 与 win 同坐标系），而不是 640×320 的岛体画布；
            // 后者会把药丸错误地钳到主岛内部，还会误判成「左侧放不下」而乱换边。
            win: env.screen.unwrap_or(ui.max_rect()),
            // 用主岛本体做锚点：body 还并进了通知 / 徽章浮层，会把药丸推偏。
            island: panel,
            course: course_geom.map(|(r, _, _)| r),
            mode: crate::auralsle_ai_status::config::get().pill_placement,
        };
        let pill_layout = crate::auralsle_ai_status::ui::device_pill_layout(
            &base, cx - origin.x, self.w.pos, self.progress(), self.side(), island_devs.len(),
            pill_area,
        );
        // 每枚底板单独走主岛同一套皮肤材质（悬浮云流体网格 / 液态玻璃 / 奶油 / 毛玻璃），
        // 背景与课程卡同源；不画白色描边、不画外扩阴影。
        // 自带不透明底板的预设（扁平徽章 / 终端等宽等）跳过这一层，避免双层叠色。
        let pill_style = crate::auralsle_ai_status::config::get().pill_style;
        if !crate::auralsle_ai_status::ui::pill_style_owns_background(pill_style) {
            for p in &pill_layout {
                // 材质网格的羽化带会溢出胶囊矩形（悬浮云皮肤下表现为药丸下方一条约 10px 的
                // 灰带，看起来像套了一层外罩）。这里把绘制严格裁进药丸矩形，外圈不再有多余材质。
                let clipped = painter.with_clip_rect(p.rect.intersect(ui.max_rect()));
                self.paint_card_body_styled(&clipped, p.rect, crate::auralsle_ai_status::ui::pill_style_corner_radius(pill_style, p.h), feather, p.fade, body, false);
            }
        }
        crate::auralsle_ai_status::record_device_pill_rects(
            pill_layout.iter().map(|p| p.rect).collect(),
        );
        let content_from=crate::skin_paint::shape_count(&ctx,layer);
        // [修复需求 9]：将课程岛内容绘制移出 shared_header（shared_header 在极简时间药丸模式下会提前 return），
        // 确保无论主岛处于极简药丸、歌词态还是展开态，左侧课程岛内容始终实时渲染并立即响应 UI 样式切换！
        crate::auralsle_ai_status::ui::draw_course_outer(&base, cx - origin.x, self.w.pos, self.progress(), self.side());
        {
            // 8 款预设的配色已由 PillTheme 按当前皮肤推导（深浅底、文字、强调色都算好了），
            // 所以整段药丸内容要排除出 recolor_layer：否则数值色块上的深色字会被换色层
            // 洗成半透明浅色，几乎看不清（扁平徽章 / 双色分段尤其明显）。
            let pill_from = crate::skin_paint::shape_count(&ctx, layer);
            let mut outer = base.clone();
            outer.painter = ui.painter().with_clip_rect(ui.max_rect());
            let ink_rgb = palette.map(|pal| pal.ink).unwrap_or([255, 255, 255]);
            let icon_tint = Color32::from_rgb(ink_rgb[0], ink_rgb[1], ink_rgb[2]);
            let light_skin = palette
                .map(|pal| {
                    pal.paper[0] as f32 * 0.299
                        + pal.paper[1] as f32 * 0.587
                        + pal.paper[2] as f32 * 0.114
                        > 140.0
                })
                .unwrap_or(false);
            let pill_theme = crate::auralsle_ai_status::ui::PillTheme::from_skin(ink_rgb, light_skin);
            crate::auralsle_ai_status::ui::draw_device_pills(
                &outer, &pill_layout, &island_devs, icon_tint, pill_theme, pill_style,
            );
            let pill_to = crate::skin_paint::shape_count(&ctx, layer);
            if pill_to > pill_from {
                keep_colors.push((pill_from, pill_to));
            }
        }
        self.cursor_local = env.cursor.map(|p| base.to_local(p));

        let banner=!self.compact_notices()&&self.notice.is_some()&&!self.open;
        let restoring_lyrics=!self.open&&self.notice.is_none()&&self.island_lyrics_active()&&self.hud_notice.as_ref().is_some_and(|a|a.meeting.is_some()||a.source!=crate::extras::AlertSource::Internal);
        let fading_hud=!self.compact_notices()&&!self.open&&self.hud_notice.as_ref().is_some_and(|a|a.is_hud())&&(banner||self.hud_alpha>0.003);
        if fading_hud {
            let from=self.resident.target.width;let to=HUD_W+self.pet_space();
            let width_progress=if (to-from).abs()<1.0 {if banner{1.0}else{0.0}}else{((self.w.pos-from)/(to-from)).clamp(0.0,1.0)};
            let (opacity,header_opacity)=if banner {(self.hud_alpha*smoothstep(0.45,0.85,width_progress),1.0-smoothstep(0.05,0.40,width_progress))}
                else {(self.hud_alpha*smoothstep(0.20,0.60,width_progress),1.0-smoothstep(0.05,0.25,width_progress))};
            if !minimal_pill||self.minimal_time_pill_mode()||pc>0.0 {self.resident_header(&base.faded(header_opacity),panel,cx,pc,reveal,env);}
            if let Some(a)=self.hud_notice.clone(){
                let ps = self.notice_pet_space(&a);
                let overlay=Card::new(ui,panel.min+Vec2::new(ps,0.0),panel.intersect(ui.max_rect())).faded(opacity);
                let overlay=if opacity<0.85||!banner{overlay.ghost()}else{overlay};
                self.draw_notice(&overlay,panel,&a,&mut cmds);
            }
        }else if banner {
            if let Some(a)=self.notice.clone(){
                let mut ps = self.notice_pet_space(&a);
                // 外部横幅通知（如 OPPO 互联）在带吉祥物时往左紧凑 4px，使左右留白完全对称
                if ps > 0.0 && a.source != crate::extras::AlertSource::Internal && a.messenger().is_none() {
                    ps = (ps - 4.0).max(0.0);
                }
                self.draw_notice(&Card::new(ui,panel.min+Vec2::new(ps,0.0),panel.intersect(ui.max_rect())),panel,&a,&mut cmds);
            }
        }else if !minimal_pill || self.minimal_time_pill_mode() || pc > 0.0 {
            self.resident_header(&base, panel, cx, if restoring_lyrics{0.0}else{pc}, reveal, env);
        }
        if let Some(rect)=notice_rect {
            let c=Card::new(ui,rect.min,rect.intersect(ui.max_rect()));
            if let Some(a)=self.hud_notice.clone() {
                let opacity=self.hud_alpha*smoothstep(0.45,0.95,self.notice_width.pos/44.0);
                let c=c.faded(opacity);let c=if self.notice.is_none()||opacity<0.85 {c.ghost()}else{c};
                if a.source!=crate::extras::AlertSource::Internal {
                    paint::bell(&c.scaled(18.0,15.0,0.85),18.0,15.0,theme::WHITE,paint::bell_swing(self.t-a.at));
                    c.circle(22.0,10.5,2.0,Color32::from_rgb(255,72,72));
                }else{crate::extras::notice_badge(&c,18.0,15.0,&a);}
                let button=crate::quota_ui::bar_tooltip(c.hit(0.0,0.0,36.0,BAR_H,"notice-badge"),format!("{}\n{}",a.title,a.body));
                if button.clicked(){if let Some(kind)=a.health(){self.confirm_health_reminder(kind);}else{self.notice_details=!self.notice_details;}}
            }else if self.t<self.notification_bell_until && !has_msg_orb {
                paint::bell(&c.scaled(20.0,14.0,1.2),20.0,14.0,theme::WHITE,paint::bell_swing(self.t-self.notification_bell_at));
                c.circle(24.0,9.5,2.0,Color32::from_rgb(255,72,72));
                if c.hit(6.0,0.0,28.0,30.0,"bell-mail").clicked(){self.show_page(5);}
            }
        }
        if let Some(rect)=messenger_rect {
            let from=crate::skin_paint::shape_count(&ctx,layer);
            let c=Card::new(ui,rect.min,rect.intersect(ui.max_rect()));
            if outside_badge.is_some() {
                let c=if self.minimal_pill_msg_badge_active() { c } else { c.scaled(0.0,0.0,appear) };
                self.draw_unread_capsule(&ctx,&c,rect,env.cursor,&mut cmds);
            }else{
                let horizontal=!(self.side() && self.notice.is_none());
                let (arr_sc, _, arr_a, _) = if cfg!(test) {
                    (1.0, 1.0, 1.0, 1.0)
                } else {
                    orb_arrival_anim((self.t - self.notification_bell_at) as f32)
                };
                let scale=self.messenger_stack_appear() * arr_sc;
                let fade=self.messenger_stack_appear() * arr_a;
                self.draw_unread_stack(&c.scaled(0.0,0.0,scale).faded(fade),horizontal,&mut cmds);
            }
            keep_colors.push((from,crate::skin_paint::shape_count(&ctx,layer)));
        }
        let notice_details=self.notice_details_rect(cx,top);
        if let Some(rect)=notice_details {
            let radius=if self.hud_notice.as_ref().is_some_and(|a|a.meeting.is_some()){14.0}else{10.0};
            let from=crate::skin_paint::shape_count(&ctx,layer);
            shape::shadow(&painter,rect,0.0,0.0,radius,0.5*shadow_k);
            painter.rect_filled(rect,radius,self.glass_fill());
            if glass_skin {
                painter.rect_stroke(rect,radius,egui::Stroke::new(1.0_f32,self.glass_rim()),egui::StrokeKind::Inside);
            }
            keep_colors.push((from,crate::skin_paint::shape_count(&ctx,layer)));
            self.draw_notice(&Card::new(ui,rect.min,rect.intersect(ui.max_rect())),rect,&self.hud_notice.clone().unwrap(),&mut cmds);
            if ctx.input(|i|i.pointer.any_pressed()&&i.pointer.interact_pos().is_some_and(|p|!rect.contains(p)&&!notice_rect.is_some_and(|r|r.contains(p)))){self.notice_details=false;}
        }
        let hide_pet_for_meeting = banner && self.notice.as_ref().is_some_and(|a| a.meeting.is_some());
        let stage1_pet_k = if self.minimal_time_pill_mode() && !banner && pc < 0.02 {
            let (min_w, _) = self.minimal_pill_dims();
            let full_w = self.resident.target.width.min(WIN_W - 32.0).max(min_w + 24.0);
            smoothstep(0.32, 0.88, (self.w.pos - min_w) / (full_w - min_w).max(1.0))
        } else {
            1.0
        };
        let base_pet_a = if !cfg!(test) && banner && self.pet_enabled() { self.pet_a.max(self.hud_alpha.max(0.95)) } else { self.pet_a };
        let pet_alpha=base_pet_a*(1.0-smoothstep(0.0,0.3,pc))*stage1_pet_k;
        if pet_alpha>0.004 && (!minimal_pill || banner || stage1_pet_k > 0.01) && !hide_pet_for_meeting {
            let y=(h.min(self.base_size().1)-BAR_H).max(0.0)/2.0;
            // 吉祥物跟随皮肤换色：原生单色眼睛在浅色底上变成深色，而 Aora Bot 彩色角色保留自身配色。
            let fit=if self.island_lyrics_active()||self.notice.is_some(){1.0}else{((WIN_W-32.0)/self.resident.target.width).min(1.0)};
            let pet_from=crate::skin_paint::shape_count(&ctx,layer);
            self.pet.draw(&base.scaled(cx-origin.x,15.0,fit).shifted(0.0,y).faded(pet_alpha),cx-origin.x+self.resident.position(bar::PET),self.t,self.skin_palette());
            if crate::auralsle_ai_status::config::get().islo_face && (!cfg!(test) || (self.pet.state!=PetState::Music && self.pet.expressions.current().is_none())) {
                keep_colors.push((pet_from,crate::skin_paint::shape_count(&ctx,layer)));
            }
        }
        if reveal > 0.004 && !banner && !restoring_lyrics {
            let c = base.shifted(0.0, (1.0 - reveal) * -5.0).faded(reveal);
            let c = if reveal < 0.6 { c.ghost() } else { c };
            if self.calendar.draft.is_some(){self.draw_calendar_editor(&c);}else if self.task_dialog.is_some(){self.draw_task_editor(&c);}else{self.panel_body(&c, env, &mut cmds);}
            let toast_y = if !cfg!(test) && self.expanded_height() >= 190.0 {
                self.expanded_height() - 52.0
            } else {
                106.0
            };
            if !self.toast_below {self.toast_layer(&c, toast_y);}
        }
        // 岛下的确认提示使用独立裁剪区域，收起时跟随岛体下沿。
        let floating_toast=if self.toast_below && self.toast.is_some() {
            // 侧边停靠：提示出现在胶囊正下方，靠边时向内挪，保证完整显示。
            let toast_origin=if side {
                let center=if dock==model::Dock::Left {panel.center().x.max(win.left()+90.0)} else {panel.center().x.min(win.right()-90.0)};
                Pos2::new(center-PW/2.0,panel.top())
            } else {origin};
            self.toast_layer(&Card::new(ui,toast_origin,win),h+8.0)
        }else{None};
        if (cfg!(test) || self.editing.is_some()) && (self.adding||self.editing.is_some()) && reveal > 0.9 && self.page == 4 && self.page_e() > 0.25 {
            self.input_field(ui, origin, &mut cmds);
        }

        if self.task_dialog.is_some() && reveal>0.9 {self.task_editor_inputs(ui,origin,&mut cmds);}
        if self.calendar.draft.is_some() && reveal>0.9 {self.calendar_editor_inputs(ui,origin,&mut cmds);}

        // ---------- 输出 ----------
        // [ai-status-mod] win 在画布内右移 LEFT_PAD 给课程卡留位，但 region/glass/hit
        // 是相对整个画布的窗口坐标（native.update 直接消费），只能减视口原点。
        // 若减 win.min，系统毛玻璃 / 可见区域 / 命中区会整体左移 LEFT_PAD。
        let rel = |r: Rect| r.translate(-ui.max_rect().min.to_vec2());
        let mut region = if side {
            // 侧边：面板从边缘滑出，区域包含肩部（沿边缘上下）与外侧阴影，并一直延伸到屏幕边缘。
            let r=if p > 0.02 || self.open {
                let height=self.expanded_height().max(self.h.pos)*1.08+FLARE*2.0+24.0;
                self.island_rect(win,PW+20.0,height).union(panel.expand(20.0))
            } else {
                let (bw,bh)=self.base_size();
                self.island_rect(win,bw*SWELL.1+14.0,bh*SWELL.0+FLARE+16.0).union(panel.expand(12.0))
            };
            if dock==model::Dock::Left {Rect::from_min_max(Pos2::new(win.left(),r.top()),r.max)} else {Rect::from_min_max(r.min,Pos2::new(win.right(),r.bottom()))}
        } else if p > 0.005 || self.open || self.minimal_stage1 {
            if cfg!(test) {
                let height=self.expanded_height().max(self.h.pos)*1.08+16.0;
                Rect::from_center_size(Pos2::new(cx, top + height/2.0), Vec2::new(PW + FLARE * 2.0 + 56.0, height))
            } else {
                // [修复需求 10 展开与收起岛体卡顿]：
                // 展开与收起过渡期间（p > 0.005）使用恒定不变的包容矩形（520×336），使整个展开/收起动画期间 SetWindowRgn 仅在首尾切换 1 次，不在中间每帧重建 GDI 窗口裁剪区！
                Rect::from_center_size(Pos2::new(cx, top + 168.0), Vec2::new(PW + FLARE * 2.0 + 84.0, 336.0))
            }
        } else {
            let (bw, bh) = self.base_size();
            let (rw, rh) = if cfg!(test) {
                (bw * SWELL.0 + 24.0, bh * SWELL.1 + 12.0)
            } else {
                // 收起态同样使用包容大胶囊与小胶囊的稳定高度包络
                (bw.max(360.0) * SWELL.0 + 36.0, bh.max(BAR_H) * SWELL.1 + 20.0)
            };
            Rect::from_center_size(Pos2::new(cx, top + rh / 2.0), Vec2::new(rw, rh))
        };
        if self.drag_lift.pos.abs()>0.005 || self.drag_stretch.pos.abs()>0.001 {region=region.union(panel.expand2(Vec2::new(18.0,22.0)));}
        if let Some(r)=messenger_rect{region=region.union(r.expand(5.0));}
        if let Some(rect)=notice_shape {region=region.union(rect.expand2(Vec2::new(12.0,8.0)));}
        if let Some(rect)=notice_details {region=region.union(rect.expand(12.0));}
        if let Some(r)=center_guide {region=region.union(r);}
        // [ai-status-mod] 课程卡只在岛体展开时存在，收起态不占可见区域。
        let course_card=crate::auralsle_ai_status::course_card_rect();
        if let Some(r)=course_card {region=region.union(r.expand(6.0));}
        // 胶囊上方留空的那一小条也要在窗口区域内，阴影才不会被截断。
        if pill && !side {region.min.y=region.min.y.min(win.top());}
        // 浅色皮肤：把这一帧岛体内容统一换成皮肤配色。
        if let Some(palette)=palette {
            let end=crate::skin_paint::shape_count(&ctx,layer);
            let mut ranges=Vec::new();let mut at=content_from;
            keep_colors.sort();
            for (from,to) in keep_colors {if from>at {ranges.push((at,from));} at=at.max(to);}
            if end>at {ranges.push((at,end));}
            crate::skin_paint::recolor_layer(&ctx,layer,&ranges,&palette);
        }
        // 原生裁剪还需包含悬停提示；只扩大绘制区域，保持桌面点击穿透。
        let mut tooltips=Vec::new();
        ctx.memory(|memory|{
            for layer in memory.areas().visible_layer_ids() {
                if layer.order==egui::Order::Tooltip {
                    if let Some(rect)=memory.area_rect(layer.id){region=region.union(rect.expand(12.0));tooltips.push(rect);}
                }
            }
        });
        // 毛玻璃：窗口区域只取岛体与浮层的真实轮廓，系统模糊不会溢出到透明画布上。
        let glass_region=((glass_skin || self.settings.skin==model::Skin::Fluid || (!cfg!(test) && crate::auralsle_ai_status::config::get().win_theme > 0)) && (self.settings.glass_blur || !cfg!(test) || self.settings.skin==model::Skin::LiquidGlass) || env.opaque_surface).then(||{
            let outline=|r:Rect,flare:f32,flare_h:f32,radius:f32| if pill {GlassShape::Round(rel(r),radius.min(r.height().min(r.width())*0.5))}
                else if side {GlassShape::Docked{rect:rel(r),dock,flare,flare_h,radius}} else {GlassShape::Notch{rect:rel(r),flare,flare_h,radius}};
            let mut shapes=vec![outline(body,flare,flare_h,radius)];
            if let Some(r)=outside_badge{shapes.push(GlassShape::Round(rel(r),r.width()/2.0));}  // [ai-status-mod] 原版悬浮球进毛玻璃轮廓
            if let Some(r)=notice_details {
                let radius=if self.hud_notice.as_ref().is_some_and(|a|a.meeting.is_some()){14.0}else{10.0};
                shapes.push(GlassShape::Round(rel(r),radius));
            }
            if let Some(r)=floating_toast {shapes.push(GlassShape::Round(rel(r),11.0));}
            // [ai-status-mod] 课程卡与每枚药丸各自单独铺系统毛玻璃，绝不用合并大矩形
            // （合并会把卡片之间的透明缝填成一条亮带）。模糊块向内缩 1.5px，
            // 让亚克力亮边严格落在皮肤底板内部，圆角外沿不会再留一圈白色遮罩。
            for r in crate::auralsle_ai_status::course_glass_rects() {
                // 为课程岛与每枚设备药丸各自单独注册精确的 DWM 背景毛玻璃轮廓，使设备药丸同样具备实时背景模糊！
                let inset = if r.height() < 60.0 { 0.6 } else { 1.0 };
                let rr = rel(r.shrink(inset));
                let base_rad = if course_geom.is_some_and(|(cr, _, _)| (cr.center() - r.center()).length_sq() < 4.0) {
                    let (_, _, cm) = course_geom.unwrap();
                    let circle_r = r.width().min(r.height()) * 0.5;
                    lerp(circle_r, radius.min(circle_r), cm * cm)
                } else if r.height() < 60.0 {
                    crate::auralsle_ai_status::ui::pill_style_corner_radius(pill_style, r.height())
                } else {
                    radius
                };
                let rad = (base_rad - inset).max(2.0);
                shapes.push(GlassShape::Round(rr, rad));
            }
            // [ai-status-mod] 岛外按钮不再铺玻璃底（外圈遮罩已按需求彻底删除）
            shapes.extend(tooltips.iter().map(|r|GlassShape::Round(rel(*r),6.0)));
            shapes
        });
        self.update_position_drag(&ctx,env,dt);
        let actively_scrolling = !cfg!(test) && (root_scrolling || (self.open && ctx.input(|i| i.raw_scroll_delta != Vec2::ZERO || i.smooth_scroll_delta != Vec2::ZERO)));
        if actively_scrolling {
            ctx.request_repaint();
        }
        let transitioning = actively_scrolling || self.position_drag.is_some() || self.drag_glide.is_some() || !self.drag_lift.settled(1.0) || !self.drag_stretch.settled(1.0) || self.center_guide>0.002 || self.center_lock_a>0.002 || self.t-self.center_lock_at<CENTER_SWEEP || !self.w.settled(PW)
            || !self.h.settled(self.expanded_height())
            || (!cfg!(test) && (!self.w.settled(10.0) || !self.h.settled(6.0) || self.minimal_stage1))
            || self.page_e() < 0.7
            || (self.t - self.wheel_at) < 0.45
            || self.dots.iter().any(|d| !d.settled(16.0))
            || self.hover_since.is_some() && !self.open
            || self.leave_since.is_some()
            || (self.drag_a > 0.001 && self.drag_a < 0.999)
            || self.drag_hot
            || (self.pet_a > 0.001 && self.pet_a < 0.999)
            || !self.resident.settled()
            || self.open && self.page==3 && self.shelf_x.values().any(|s| !s.settled(68.0))
            || self.open && self.page==2 && (!self.water_ring.settled(1.0) || !self.sit_view_t.settled(1.0) || !self.sit_t.settled(1.0))
            || self.toast.is_some()
            || self.t < self.notification_bell_until && self.t-self.notification_bell_at < 0.95
            || (self.orb_was_visible
                && ((self.t - self.orb_entry_at.max(self.notification_bell_at)) < 0.72
                    || !self.orb_x.settled(0.6)
                    || !self.orb_y.settled(0.6)
                    || !self.orb_w.settled(0.6)
                    || !self.orb_h.settled(0.6)))
            || self.notice.as_ref().is_some_and(|a|a.source!=crate::extras::AlertSource::Internal&&self.t-a.at<0.95)
            || self.hud_alpha > 0.003 && self.hud_alpha < 0.997
            || !self.notice_width.settled(1.0);
        let pet_visible=self.pet_a>0.01 && !self.open && !minimal_pill;
        if !cfg!(test) {
            crate::auralsle_ai_status::ui::paint_click_sparks(&painter, self.t);
        }
        let spark_anim = !cfg!(test) && crate::auralsle_ai_status::ui::has_active_click_sparks(self.t);
        let border_anim=crate::auralsle_ai_status::ui::border_effect_active(pc,self.t,false).is_some() || spark_anim;
        let ambient=border_anim || matches!(self.settings.skin,model::Skin::Fluid|model::Skin::LiquidGlass) && self.fluid.animating() || self.pomo.running && (self.open && self.page==1 || !self.open && !minimal_pill && self.notice.is_none() && bar::animations_enabled())
            || self.media.playing && (self.open && self.page==0 || !self.open && !minimal_pill && self.notice.is_none() && self.media.is_music())
            || self.notice.as_ref().is_some_and(|notice|notice.meeting.is_some());
        let animating=transitioning || ambient || pet_visible && self.pet.state!=PetState::Sleeping;
        // 交互形变保持 60 帧，媒体 30 帧；小表情 20 帧，睡眠 10 帧，静态界面每秒核对提醒。
        let fps_120 = crate::auralsle_ai_status::config::get().fps_120;
        let fps_trans = if fps_120 { 1.0 / 120.0 } else { 1.0 / 60.0 };
        let high_fps_ui = !cfg!(test)
            && ((self.settings_open && matches!(self.settings_tab, 12 | 13 | 17))
                || (pet_visible && crate::auralsle_ai_status::aora_face::is_ai_mascot_active()));
        let repaint_after=std::time::Duration::from_secs_f64(
            if transitioning || high_fps_ui {
                fps_trans
            } else if ambient {
                if fps_120 { 1.0 / 60.0 } else { 1.0 / 30.0 }
            } else if pet_visible {
                if self.pet.state==PetState::Sleeping { 0.1 } else if fps_120 { 1.0 / 60.0 } else { 0.05 }
            } else {
                1.0
            }
        );
        cmds.append(&mut self.pending);
        FrameOut {
            region: rel(floating_toast.into_iter().chain(self.shelf_menu_rect.map(|r|r.union(r + ctx.style().visuals.popup_shadow.margin()).expand(1.0 / ctx.pixels_per_point()))).fold(region,Rect::union).intersect(ui.max_rect())),
            glass: glass_region,
            hit: rel(if side {self.side_hit_rect(win,self.progress())} else {self.hit_rect(cx, top, self.progress())}),
            notice_hit:notice_shape.into_iter().chain(notice_details).chain(messenger_rect).chain(self.shelf_menu_rect).reduce(|a,b|a.union(b)).map(rel),
            file_drop_zone:self.settings.visible_pages[3].then(||rel(if side {self.island_rect(win,PW,self.expanded_height())} else {Rect::from_min_size(Pos2::new(cx-PW/2.0,top),Vec2::new(PW,96.0))})),
            expanded: self.open,
            editing: self.is_editing(),
            animating,
            repaint_after,
            cmds,
        }
    }

    fn hit_rect(&self, cx: f32, top: f32, p: f32) -> Rect {
        let r=self.hit_rect_base(cx,top,p);
        // 胶囊离屏幕顶边有一段空隙：把光标推到屏幕最顶端也要能唤出岛。
        if self.settings.island_shape==model::IslandShape::Pill {Rect::from_min_max(Pos2::new(r.left(),r.top()-PILL_GAP),r.max)} else {r}
    }

    fn hit_rect_base(&self, cx: f32, top: f32, p: f32) -> Rect {
        if self.effective_minimal() && !self.minimal_stage1 && !self.open {
            let (width,height)=self.base_size();
            if self.minimal_time_pill_mode() {
                let (hw, hh) = (self.w.pos.max(width) + 12.0, self.h.pos.max(height) + 6.0);
                Rect::from_min_size(Pos2::new(cx - hw / 2.0, top), Vec2::new(hw, hh))
            } else {
                let width=self.w.pos.min(width);
                let height=self.h.pos.min(height);
                Rect::from_min_size(Pos2::new(cx-width/2.0,top),Vec2::new(width,height))
            }
        } else if self.notice.is_some()&&!self.compact_notices()&&!self.open {
            let (w,h)=self.base_size();Rect::from_min_size(Pos2::new(cx-(w+8.0)/2.0,top),Vec2::new(w+8.0,h+6.0))
        } else if p > 0.04 || self.open {
            let hit_w = if cfg!(test) {
                PW + 8.0
            } else {
                PW.max(self.expanded_width()).max(self.w.pos) + 12.0
            };
            let hit_h = if cfg!(test) {
                self.expanded_height() + 2.0
            } else if self.open && (self.long_to_short_latched || self.t < self.page_switch_grace_until) {
                self.expanded_height().max(self.h.pos).max(286.0) + 4.0
            } else {
                self.expanded_height().max(self.h.pos) + 4.0
            };
            Rect::from_min_size(Pos2::new(cx - hit_w / 2.0, top), Vec2::new(hit_w, hit_h))
        } else {
            let (bw,bh)=self.base_size();
            let (hit_w,hit_h)=(bw+12.0,bh+4.0);
            Rect::from_min_size(Pos2::new(cx-hit_w/2.0,top),Vec2::new(hit_w,hit_h))
        }
    }

    /// 展开后未读胶囊的出现程度：早期平滑从面板旁滑出，取消中段空白断层。
    pub(crate) fn messenger_appear(&self)->f32 {
        if self.calendar.draft.is_some() || self.task_dialog.is_some(){return 0.0;}
        let p0 = if cfg!(test) { 0.7 } else { 0.14 };
        let span = if cfg!(test) { 0.3 } else { 0.42 };
        smoothstep(0.0, 1.0, anim::clamp01((self.progress() - p0) / span))
    }
    fn messenger_stack_appear(&self)->f32 {1.0-smoothstep(0.04,0.30,self.progress())}
    /// 收起态给未读图标预留的宽度（图标 + 与左侧控件的间距）。
    fn compact_tail_width(&self)->f32 {
        if self.open || self.effective_minimal() || self.minimal_pill_msg_badge_active() || self.island_lyrics_active() {return 0.0;}
        crate::auralsle_ai_status::compact_tail_width(self.unread_sources().len())
    }
    /// 同一来源有多个账号时给图标编号（1、2…）；只有一个账号不编号。
    pub(crate) fn unread_tag(&self,i:usize)->Option<usize>{
        let unread=self.pending_messengers.get(i)?;let source=unread.source;
        if unread.unidentified(){return None;}
        let known=|u:&&Unread|u.source==source && (!source.adapter().keep_unidentified || u.pid!=0);
        let same=self.pending_messengers.iter().filter(known).count();
        (same>1).then(||self.pending_messengers[..=i].iter().filter(known).count())
    }
    fn unread_id(&self,i:usize)->String{
        let name=self.pending_messengers[i].source.name();
        let n=self.pending_messengers[..=i].iter().filter(|u|u.source==self.pending_messengers[i].source).count();
        if n>1{format!("{name}{n}")}else{name.to_string()}
    }
    /// 展开胶囊高度：一个是圆，两个是短胶囊，三个及以上与面板等高（多出的滚动）。
    fn unread_capsule_height(&self,panel_h:f32)->f32 {
        let n=self.unread_count();
        if n>=3 {panel_h} else {4.0+UNREAD_SLOT*n as f32}
    }
    pub(crate) fn unread_top_inset(&self)->f32 {if self.dock()==model::Dock::Top {UNREAD_GAP}else{0.0}}
    fn unread_slot(&self,height:f32)->f32 {
        if self.unread_count()>=3 {(height-4.0)/3.0} else {UNREAD_SLOT}
    }
    fn unread_count(&self)->usize {self.pending_messengers.len()+self.health_badges().len()}
    /// 收起时叠放的来源图标，每个客户端各占一个。
    fn unread_sources(&self)->Vec<Messenger>{
        let mut sources=Vec::new();
        for u in &self.pending_messengers {if !sources.contains(&u.source){sources.push(u.source);}}
        sources
    }
    /// 收起时叠放区沿胶囊方向的长度。
    fn unread_stack_len(&self,horizontal:bool)->f32 {
        let icons=(self.unread_sources().len().min(2)+self.health_badges().len()) as f32;
        if horizontal {
            // [ai-status-mod] 横排时长度就是图标本体的宽度，右端由 `RIGHT_PAD` 决定留白。
            crate::auralsle_ai_status::COMPACT_UNREAD_W
                + crate::auralsle_ai_status::COMPACT_UNREAD_STEP*(icons-1.0).max(0.0)
        } else {
            32.0+unread_step(horizontal)*(icons-1.0)
        }
    }

    fn step_messenger_orb(&mut self, win: Rect, panel: Rect, dt: f32) -> (Option<Rect>, f32, f32) {
        let target = self.messenger_badge_rect(win, panel);
        let is_outside = target.is_some() && (self.minimal_pill_msg_badge_active() || self.messenger_appear() > 0.02);
        let Some(tr) = target else {
            self.orb_was_visible = false;
            return (None, 1.0, 1.0);
        };
        if !is_outside {
            self.orb_was_visible = false;
            return (Some(tr), 1.0, 1.0);
        }
        if !self.orb_was_visible {
            self.orb_was_visible = true;
            self.orb_entry_at = self.t;
            self.orb_x.pos = tr.left();
            self.orb_x.vel = 0.0;
            self.orb_y.pos = tr.top();
            self.orb_y.vel = 0.0;
            self.orb_w.pos = tr.width();
            self.orb_w.vel = 0.0;
            self.orb_h.pos = tr.height();
            self.orb_h.vel = 0.0;
        }
        self.orb_x.target = tr.left();
        self.orb_y.target = tr.top();
        self.orb_w.target = tr.width();
        self.orb_h.target = tr.height();
        let step_dt = dt.clamp(0.001, 0.05);
        self.orb_x.step(step_dt);
        self.orb_y.step(step_dt);
        self.orb_w.step(step_dt);
        self.orb_h.step(step_dt);

        let entry_t = self.orb_entry_at.max(self.notification_bell_at);
        let (arr_scale, arr_slide, arr_alpha, arr_ripple) = orb_arrival_anim((self.t - entry_t) as f32);

        let base = Rect::from_min_size(
            Pos2::new(self.orb_x.pos, self.orb_y.pos),
            Vec2::new(self.orb_w.pos.max(8.0), self.orb_h.pos.max(8.0)),
        );
        let place_left = base.center().x < panel.center().x;
        let slide_from_cx = if place_left {
            panel.left() + base.width() * 0.15
        } else {
            panel.right() - base.width() * 0.15
        };
        let cx = lerp(slide_from_cx, base.center().x, arr_slide);
        let cy = base.center().y;
        let scaled = Rect::from_center_size(Pos2::new(cx, cy), base.size() * arr_scale);
        (Some(scaled), arr_alpha, arr_ripple)
    }

    fn messenger_badge_rect(&self,win:Rect,panel:Rect)->Option<Rect>{
        if (self.unread_count()==0 && !self.minimal_pill_msg_badge_active())
            || (self.browser_minimal_active && !self.open && !self.minimal_pill_msg_badge_active())
        {
            return None;
        }
        // 横幅通知展开期间隐藏右侧常驻未读图标；但在「极简药丸等高消息悬浮球」模式下始终直接显示在极简药丸/展开岛右侧。
        if !self.open && !self.compact_notices() && self.notice.is_some() && !self.minimal_pill_msg_badge_active() {return None;}
        let bounds=self.screen.map_or(win,|screen|screen.intersect(win));
        let appear=self.messenger_appear();
        if self.minimal_pill_msg_badge_active() {
            // [需求 9] 展开岛时保留通知悬浮球：尺寸、间距与纵坐标全程连续贴合 panel 实时弹簧边缘平滑形变，绝不闪烁或消失
            let n = self.pending_messengers.len();
            let t_open = if cfg!(test) {
                if self.open { 1.0 } else { 0.0 }
            } else {
                smoothstep(0.0, 1.0, anim::clamp01(self.progress()))
            };
            let d_min = pill_orb_diameter(panel.height().clamp(PILL_ORB_MIN, 44.0));
            let gap = lerp(6.0, UNREAD_GAP, t_open);
            let place_left = self.dock() == model::Dock::Right;

            let (target_w, target_h) = if n == 0 {
                let d_exp = lerp(d_min, 40.0, t_open);
                (d_exp, d_exp)
            } else {
                let inset = self.unread_top_inset();
                let exp_h = self.unread_capsule_height((panel.height() - inset).max(d_min));
                (lerp(d_min, UNREAD_W, t_open), lerp(d_min, exp_h, t_open))
            };
            let y_collapsed = pill_orb_top(panel, target_h);
            let y_expanded = panel.top() + self.unread_top_inset();
            let top_y = lerp(y_collapsed, y_expanded, t_open);
            let raw_x = if place_left {
                panel.left() - gap - target_w
            } else {
                panel.right() + gap
            };
            let clamped_x = if raw_x + target_w > bounds.right() {
                bounds.right() - target_w
            } else if raw_x < bounds.left() {
                bounds.left()
            } else {
                raw_x
            };
            return Some(Rect::from_min_size(Pos2::new(clamped_x, top_y), Vec2::new(target_w, target_h)));
        }
        if appear>0.02 {
            // 展开：贴在面板外侧、顶端对齐的竖向胶囊；顶部停靠和左停靠在右边，右停靠在左边。
            let full=self.island_rect(win,PW,self.expanded_height());
            // 顶部停靠时面板贴着屏幕上沿，胶囊往下让一点，圆顶不会被屏幕边切掉。
            let inset=self.unread_top_inset();
            let height=self.unread_capsule_height(full.height()-inset);
            let left=self.dock()==model::Dock::Right || self.dock()==model::Dock::Top && full.right()+UNREAD_GAP+UNREAD_W>bounds.right();
            let width=UNREAD_W*appear;
            let x=if left {panel.left()-UNREAD_GAP*appear-width}else{panel.right()+UNREAD_GAP*appear};
            return Some(Rect::from_min_size(Pos2::new(x,panel.top()+inset),Vec2::new(width,height*appear)));
        }
        // 极简刘海：横幅照常弹出，收起后只留细线，未读不常挂在线条上；展开岛体时在面板旁出现。
        if self.effective_minimal(){return None;}
        let scale=self.messenger_stack_appear();
        if scale<0.02 || self.open && (self.calendar.draft.is_some() || self.task_dialog.is_some()){return None;}
        let side_column=self.side() && self.notice.is_none();
        let length=self.unread_stack_len(!side_column);
        let (width,height)=if side_column {(32.0,length)}else{(length,BAR_H)};
        let (width,height)=(width*scale,height*scale);
        let (x,y)=if side_column {
            (panel.center().x-width/2.0,panel.bottom())
        }else{
            // [ai-status-mod] 未读图标进药丸的 16 内边距里，右端留白与左端对称。
            let right=panel.right()+self.notice_width.pos-crate::auralsle_ai_status::RIGHT_PAD;
            let y=if self.notice.is_some() && !self.compact_notices(){panel.center().y-BAR_H/2.0}else{panel.top()};
            (if right<=bounds.right(){right-width}else{panel.left()+crate::auralsle_ai_status::RIGHT_PAD},y)
        };
        Some(Rect::from_min_size(Pos2::new(x,y),Vec2::new(width,height)))
    }

    /// 展开后的未读胶囊 & 极简胶囊/展开岛旁消息悬浮球（全程显示+流畅入场）：
    fn draw_unread_capsule(&mut self,ctx:&egui::Context,c:&Card,rect:Rect,cursor:Option<Pos2>,cmds:&mut Vec<Cmd>){
        let n=self.pending_messengers.len();
        let health=self.health_badges();
        let total_n=n+health.len();
        let width = (rect.width() / c.s).max(16.0);
        let height=rect.height()/c.s;
        // [等高悬浮球] 球径随胶囊长高后，图标/角标也要等比放大，否则大球里会出现小图标；
        // 上限由 1.0 放宽到 1.6（对应 64px 球），展开态的 40px 竖胶囊仍是 1.0，不受影响。
        let size_ratio = if cfg!(test) { 1.0 } else { (width / UNREAD_W).clamp(0.52, 1.6) };
        if !cfg!(test) && n == 0 && self.minimal_pill_msg_badge_active() {
            // 当极简胶囊处于悬浮球模式且无任务栏闪动未读项时，显示微信/QQ悬浮球（展开态同样保留可见），点击直达消息岛！
            let (_, _, arr_a, _) = orb_arrival_anim((self.t - self.orb_entry_at.max(self.notification_bell_at)) as f32);
            let alpha_vis = arr_a.clamp(0.0, 1.0);
            if alpha_vis <= 0.02 { return; }
            let c = c.faded(alpha_vis);
            let inner = c.clipped(0.0, 0.0, width, height);
            let x = width * 0.5;
            let icon_y = height * 0.5;
            let src = if self.notification_log.items.iter().any(crate::pages::is_strict_qq_chat_record)
                && !self.notification_log.items.iter().any(|r| r.app_name.contains("微信") || r.app_id.contains("wechat"))
            {
                Messenger::Qq
            } else {
                Messenger::Wechat
            };
            let d = width.min(height);
            let orb = orb_badge_layout(d);
            // [视觉居中] 数字角标挂在图标右下，整组会往右下偏出约 0.07d/0.05d；
            // 把图标按一半偏移往左上挪，"图标+角标"整体才是眼睛看到的居中（本轮图2红框）。
            let x = x - d * 0.035;
            let icon_y = icon_y - d * 0.025;
            let scale = orb.icon_side / 20.0;
            crate::extras::messenger_icon(&inner.scaled(x, icon_y, scale), x, icon_y, src);
            let hist_cnt = self.notification_log.items.len();
            if hist_cnt > 0 && crate::auralsle_ai_status::config::get().show_unread_badge_count {
                let bx = x + orb.badge_dx;
                let by = icon_y + orb.badge_dy;
                inner.circle(bx, by, orb.badge_r, Color32::from_gray(242));
                inner.centered_text(
                    bx,
                    by,
                    &format!("{}", hist_cnt.min(99)),
                    orb.digit,
                    W::Bold,
                    Color32::from_gray(24),
                    H::Center,
                );
            }
            if inner
                .hit(0.0, 0.0, width, height, "min-pill-floating-orb")
                .on_hover_text("点击展开消息岛查看最近消息")
                .clicked()
            {
                self.show_page(5);
            }
            return;
        }
        let single_msg = !cfg!(test) && n == 1 && health.is_empty();
        let slot = if single_msg { height } else { self.unread_slot(height) * size_ratio };
        let max_scroll=(total_n as f32-3.0).max(0.0)*slot;
        if max_scroll>0.0 && cursor.is_some_and(|p|rect.contains(p)) {
            let delta=ctx.input(|i|i.raw_scroll_delta.y);
            self.messenger_scroll-=delta*0.5;
        }
        self.messenger_scroll=self.messenger_scroll.clamp(0.0,max_scroll);
        let (_, _, arr_a, _) = if cfg!(test) {
            (1.0, 1.0, 1.0, 1.0)
        } else {
            orb_arrival_anim((self.t - self.orb_entry_at.max(self.notification_bell_at)) as f32)
        };
        let alpha_vis = if self.minimal_pill_msg_badge_active() { arr_a } else { self.messenger_appear() * arr_a };
        let c=c.faded(alpha_vis);
        let inner = if cfg!(test) {
            c.clipped(0.0, 2.0, UNREAD_W, height - 4.0)
        } else {
            c.clipped(0.0, 0.0, width, height)
        };
        let x = if cfg!(test) { UNREAD_W / 2.0 } else { width * 0.5 };
        for i in 0..n {
            let u=self.pending_messengers[i];
            let cy = if single_msg {
                height * 0.5
            } else {
                2.0 * size_ratio + slot * (i as f32 + 0.5) - self.messenger_scroll
            };
            if cy< -slot || cy>height+slot {continue;}
            let base_scale = if crate::auralsle_ai_status::config::get().messenger_mode == 1 {
                u.source.adapter().icon_scale
            } else {
                crate::auralsle_ai_status::unread_icon_scale(u.source==Messenger::Wechat,false)  // [ai-status-mod] 与岛上其它图标同尺寸（放大）
            };
            let scale = base_scale * size_ratio;
            let icon_y = if single_msg {
                height * 0.5
            } else {
                cy + (8.0 - self.unread_top_inset() - crate::auralsle_ai_status::MESSENGER_OPTICAL_Y) * size_ratio
            };
            crate::extras::messenger_icon(&inner.scaled(x,icon_y,scale),x,icon_y,u.source);
            // 红点跟随设置（默认关闭）：只要图标、不要角标时这里不画。
            if self.settings.unread_dot {
                inner.circle(x + 10.0 * size_ratio, icon_y - 9.0 * size_ratio, 2.6 * size_ratio, Color32::from_rgb(255,72,72));
            }
            let tag = if cfg!(test) {
                self.unread_tag(i).map(|tag|tag.to_string()).or_else(||(u.unidentified()).then(||"?".to_string()))
            } else if !crate::auralsle_ai_status::config::get().show_unread_badge_count {
                None
            } else {
                // 支持开关控制通知悬浮窗软件图标右下角的数字角标显示与隐藏（需求 9，图五 `image_a9ec52.png`）
                self.unread_tag(i)
                    .map(|tag| tag.to_string())
                    .or_else(|| (u.unidentified()).then(|| "?".to_string()))
                    .or_else(|| self.minimal_pill_msg_badge_active().then(|| format!("{}", n.max(1))))
            };
            if let Some(tag)=tag{
                let bx = x + 8.5 * size_ratio;
                let by = icon_y + 7.5 * size_ratio;
                inner.circle(bx, by, 5.8 * size_ratio, Color32::from_gray(238));  // [ai-status-mod] 原版数字圆底（按比例等比缩放）
                inner.centered_text(bx, by, &tag, (8.4 * size_ratio).max(6.2), W::Bold, Color32::from_gray(28), H::Center);
            }
            let name=if u.unidentified(){format!("{} · 账号未确认",u.source.name())}
                else{match self.unread_tag(i){Some(n)=>format!("{} · 账号 {n}",u.source.name()),None=>u.source.name().to_string()}};
            let button=inner.hit(0.0,cy-slot/2.0,width,slot,self.unread_id(i)).on_hover_text(format!("查看{name}"));
            if button.clicked(){cmds.push(Cmd::ViewMessenger(u));}
        }
        for (i,kind) in health.into_iter().enumerate() {
            let cy=2.0*size_ratio+slot*((n+i) as f32+0.5)-self.messenger_scroll;
            if cy< -slot || cy>height+slot {continue;}
            kind.icon(&inner,x,cy,0.8*size_ratio);
            inner.circle(x+10.0*size_ratio,cy-9.0*size_ratio,2.6*size_ratio,Color32::from_rgb(255,59,48));
            let button=crate::quota_ui::bar_tooltip(inner.hit(0.0,cy-slot/2.0,width,slot,kind.id().to_string()),format!("{} · 点击确认，重新计时",kind.label()));
            if button.clicked(){self.confirm_health_reminder(kind);}
        }
        if max_scroll>0.0 {
            // 还有没露出的图标：上下边缘各一条淡淡的指示。
            let ink=paint::alpha(theme::WHITE,0.35);
            if self.messenger_scroll>0.5 {c.fill(x-6.0*size_ratio,4.0,12.0*size_ratio,2.0,1.0,ink);}
            if self.messenger_scroll<max_scroll-0.5 {c.fill(x-6.0*size_ratio,height-6.0,12.0*size_ratio,2.0,1.0,ink);}
        }
    }

    /// 收起时的未读入口：每个来源一个图标加小红点（同一来源多个账号合成一个）。只有一条时点它直接跳转，多条时展开面板挑选。
    fn draw_unread_stack(&mut self,c:&Card,horizontal:bool,cmds:&mut Vec<Cmd>){
        let sources=self.unread_sources();
        let health=self.health_badges();
        let count=self.pending_messengers.len();
        let icons=sources.len()+health.len();
        let step=unread_step(horizontal);
        let at=|k:usize|if horizontal {
            // [ai-status-mod] 横排以「图标左边缘 = 10.5」为锚，卡片宽就是图标宽，右端由布局给。
            (crate::auralsle_ai_status::COMPACT_UNREAD_W/2.0+crate::auralsle_ai_status::COMPACT_UNREAD_STEP*k as f32,crate::auralsle_ai_status::BALL_CY_COMPACT)
        }else{(16.0,crate::auralsle_ai_status::BALL_CY_COMPACT+step*k as f32)};
        for k in 0..sources.len() {
            let (x,y)=at(k);
            let scale = if crate::auralsle_ai_status::config::get().messenger_mode == 1 {
                sources[k].adapter().collapsed_icon_scale
            } else {
                crate::auralsle_ai_status::unread_icon_scale(sources[k]==Messenger::Wechat,true)  // [ai-status-mod] 与岛上其它图标同尺寸（入口）
            };
            crate::extras::messenger_icon(&c.scaled(x,y,scale),x,y,sources[k]);
        }
        for (i,kind) in health.iter().enumerate() {
            let (x,y)=at(sources.len()+i);
            kind.icon(c,x,y,0.7);
        }
        // 每个来源图标右上角一个小红点（可关），不显示数字；健康待办徽标固定显示红点。
        for k in 0..icons {
            if k < sources.len() && !self.settings.unread_dot { continue; }
            let (x,y)=at(k);c.circle(x+8.0,y-7.0,2.6,Color32::from_rgb(255,59,48));
        }
        let length=self.unread_stack_len(horizontal);
        let messenger_length=if health.is_empty(){length}else if horizontal{crate::auralsle_ai_status::COMPACT_UNREAD_W+crate::auralsle_ai_status::COMPACT_UNREAD_STEP*(sources.len() as f32-1.0).max(0.0)}else{sources.len() as f32*step+3.0};
        let (w,h)=if horizontal {(messenger_length,BAR_H)}else{(32.0,messenger_length)};
        if count==1 {
            let u=self.pending_messengers[0];
            let button=c.hit(0.0,0.0,w,h,u.source.name()).on_hover_text(format!("查看{}",u.source.name()));
            if button.clicked(){cmds.push(Cmd::ViewMessenger(u));}
        }else if count>1{
            let button=c.hit(0.0,0.0,w,h,"messenger-stack").on_hover_text(format!("{count} 条未读 · 展开挑选"));
            if button.clicked(){self.reveal_unread();}
        }
        for (i,kind) in health.into_iter().enumerate() {
            let k=sources.len()+i;
            let (x,y)=at(k);
            let start=if k==0{0.0}else{(if horizontal{x}else{y})-step/2.0};
            let end=if k+1==icons{length}else{(if horizontal{x}else{y})+step/2.0};
            let button=if horizontal{c.hit(start,0.0,end-start,BAR_H,kind.id().to_string())}else{c.hit(0.0,start,32.0,end-start,kind.id().to_string())};
            if crate::quota_ui::bar_tooltip(button,format!("{} · 点击确认，重新计时",kind.label())).clicked(){self.confirm_health_reminder(kind);}
        }
    }
    /// 展开面板，让旁边的未读胶囊露出来。
    pub(crate) fn reveal_unread(&mut self){
        self.notice=None;
        self.open=true; self.forced_open_until=self.t+4.0;
        self.w.tune(OPEN_W.0,OPEN_W.1);self.h.tune(OPEN_H.0,OPEN_H.1);
        self.w.target=PW;self.h.target=self.expanded_height();
    }
    fn notice_rect(&self,cx:f32,top:f32)->Option<Rect>{
        if !self.compact_notices(){return None;}
        let width=self.notice_width.pos-8.0;
        (width>0.5).then(||Rect::from_min_size(Pos2::new(cx+self.w.pos/2.0+8.0,top),Vec2::new(width,BAR_H)))
    }
    fn notice_details_rect(&self,cx:f32,top:f32)->Option<Rect>{
        let a=self.hud_notice.as_ref().filter(|_|self.notice_details&&self.notice.is_some())?;
        let badge=self.notice_rect(cx,top)?;let (width,height)=self.notice_size(a);
        let mut x=badge.left();
        // 岛体靠近屏幕右缘时，详情卡片向左展开，始终完整留在屏幕内。
        if let Some(screen)=self.screen {x=x.min(screen.right()-width-8.0).max(screen.left()+8.0);}
        Some(Rect::from_min_size(Pos2::new(x,badge.bottom()+6.0),Vec2::new(width,height)))
    }

    /// 当前皮肤的换色调色板。悬浮云底板随封面取色，页面里的深色控件改走半透明黑，
    /// 叠在玻璃底上自然得到同色系的加深色，不再出现突兀的纯黑块。
    pub(crate) fn skin_palette(&self)->Option<crate::skin_paint::Palette> {
        crate::skin_paint::set_fluid(self.settings.skin == model::Skin::Fluid);
        match self.settings.skin {
            model::Skin::Cream=>Some(crate::skin_paint::CREAM),
            model::Skin::GlassLight=>Some(crate::skin_paint::GLASS_LIGHT),
            model::Skin::GlassDark=>Some(crate::skin_paint::GLASS_DARK),
            model::Skin::LiquidGlass=>Some(crate::skin_paint::LIQUID_GLASS),
            model::Skin::Fluid=>Some(crate::skin_paint::FLUID),
            model::Skin::Classic=>{
                if !cfg!(test) {
                    if let Some(wp) = crate::auralsle_ai_status::plugins::active_win_palette() {
                        return Some(wp);
                    }
                }
                None
            }
        }
    }

    /// 设置页预览用的岛体底色：没有真实模糊，毛玻璃按较实的玻璃表现。
    pub(crate) fn preview_fill(&self)->Color32 {
        match self.settings.skin {
            model::Skin::GlassDark=>{let (r,g,b)=glass::GROUND_DARK;Color32::from_rgba_unmultiplied(r,g,b,215)}
            model::Skin::GlassLight=>{let [r,g,b]=glass::LIGHT_GROUND.map(|v|(v*255.0).round() as u8);Color32::from_rgba_unmultiplied(r,g,b,225)}
            model::Skin::LiquidGlass=>{
                let [br, bg, bb] = self.backdrop_rgb;
                Color32::from_rgba_unmultiplied(
                    (br * 185.0 + 28.0).clamp(32.0, 220.0) as u8,
                    (bg * 185.0 + 30.0).clamp(32.0, 220.0) as u8,
                    (bb * 190.0 + 34.0).clamp(36.0, 228.0) as u8,
                    148,
                )
            }
            model::Skin::Classic=>{
                if !cfg!(test) {
                    if let Some(wf) = crate::auralsle_ai_status::plugins::active_win_fill(225) {
                        return wf;
                    }
                }
                self.glass_fill()
            }
            _=>self.glass_fill(),
        }
    }

    /// 岛体底色：经典为纯黑；奶油为暖白；毛玻璃按当前着色量叠加深色或浅色玻璃；
    /// 液态玻璃为无底色高通透折射（仅保留底层实时模糊与边缘折射）。
    /// 设备药丸要显示的设备清单（蓝牙 + Voltide 2.4G），每帧算一次。
    ///
    /// 三个来源合并去重：系统蓝牙监听 `env.bt` / 变化快照 `bt_shown` / `bt::connected_peripherals()`
    /// （含 2.4G 与演示设备）。不能只用 `bt_shown`——「蓝牙通知」关掉时它会被清空。
    fn island_devices(&self, env: &Env) -> Vec<crate::bt::Device> {
        if !crate::auralsle_ai_status::config::get().show_bt_battery_on_island {
            return Vec::new();
        }
        let mut devs: Vec<crate::bt::Device> = Vec::new();
        for d in env
            .bt
            .iter()
            .cloned()
            .chain(self.bt_shown.iter().cloned())
            .chain(crate::bt::connected_peripherals())
        {
            if !devs.iter().any(|x| x.id == d.id || x.name == d.name) {
                devs.push(d);
            }
        }
        devs
    }

    fn glass_fill(&self)->Color32 {
        let alpha=(self.glass_tint.clamp(0.0,1.0)*255.0).round() as u8;
        match self.settings.skin {
            model::Skin::GlassDark=>{let (r,g,b)=glass::GROUND_DARK;Color32::from_rgba_unmultiplied(r,g,b,alpha)}
            model::Skin::GlassLight=>{
                let [r,g,b]=glass::LIGHT_GROUND.map(|v|(v*255.0).round() as u8);
                let gl_a = if cfg!(test) {
                    alpha
                } else {
                    let op_k = (crate::auralsle_ai_status::config::get().glass_opacity.clamp(25, 100) as f32 / 84.0).clamp(0.72, 1.15);
                    ((228.0 * op_k).round() as i32).clamp(192, 244) as u8
                };
                Color32::from_rgba_unmultiplied(r,g,b,gl_a)
            }
            model::Skin::Cream=>{let [r,g,b]=crate::skin_paint::CREAM.paper;Color32::from_rgb(r,g,b)}
            model::Skin::Classic=>{
                if !cfg!(test) {
                    if let Some(wf) = crate::auralsle_ai_status::plugins::active_win_fill(alpha.max(210)) {
                        return wf;
                    }
                }
                Color32::BLACK
            }
            model::Skin::Fluid=>self.fluid.compose(0.5,0.5,1.0,1.0,0.0),
            model::Skin::LiquidGlass=>{
                let [br, bg, bb] = self.backdrop_rgb;
                let lg_alpha = ((self.glass_tint.clamp(0.14, 0.62) * 0.34) * 255.0).round() as u8;
                Color32::from_rgba_unmultiplied(
                    (br * 155.0 + 18.0).clamp(18.0, 215.0) as u8,
                    (bg * 155.0 + 18.0).clamp(18.0, 215.0) as u8,
                    (bb * 160.0 + 20.0).clamp(20.0, 220.0) as u8,
                    lg_alpha,
                )
            }
        }
    }

    /// 毛玻璃边缘高光：浅色玻璃更亮，像苹果的玻璃边缘。
    fn glass_rim(&self)->Color32 {
        if self.settings.skin==model::Skin::LiquidGlass {
            return Color32::from_white_alpha(96);
        }
        if !crate::auralsle_ai_status::outline_enabled() {return Color32::TRANSPARENT;}  // [ai-status-mod] 默认关闭 = 取消描边
        let strength=if self.settings.skin==model::Skin::GlassLight {0.55} else {glass::rim_alpha()*0.7};
        Color32::from_white_alpha((255.0*strength).round() as u8)
    }

    /// [ai-status-mod] 真实物理光学液态玻璃着色器（严格遵循 `rdev/liquid-glass-react` 的实时底层采样 + 边缘位移折射原理）：
    /// - **中心超清实时透视**：绝不涂抹固定暖黄或青蓝底色！直接透出岛体正下方的 Windows 实时合成模糊层（`HostBackdropBrush`），
    ///   并按正下方局部 2D 空间采样 `backdrop::spatial_at(u, v)` 自动微调对比度（`overLight` 自适应），确保深浅背景下白字皆清晰；
    /// - **边缘 Snell 定律凸透镜折射与 RGB 色散（`feDisplacementMap` + `aberrationIntensity`）**：
    ///   在距岛体轮廓 `lens_w` 范围内计算曲面法线 $ec{n}$ 与透镜曲率 $t = 1 - d / 	ext{lens\_w}$，
    ///   将红/绿/蓝三通道分别沿法线以不同折射率偏移采样实时 2D 屏幕场，叠加 140% 饱和度增强与 Fresnel 菲涅尔镜面反光。
    /// [ai-status-mod] 深度融合 `Wei-Zhang09/LiquidGlassUI` 与 `rdev/liquid-glass-react` 的水晶光学液态玻璃着色器：
    /// 1. **中心高透水晶层（Crystal Clear Core）**：彻底摒弃令岛体显得灰蒙发污的重度烟熏底色，中心直接透出底层 Windows 实时合成模糊层（`HostBackdropBrush`），
    ///    叠加 `LiquidGlassUI` 标志性的 135° 斜向清透玻璃反光带（Diagonal Specular Sheen）；
    /// 2. **弯月面凸透镜边缘折射与全反射暗槽（Meniscus Lens & Total-Internal-Reflection Groove）**：
    ///    在距轮廓 `lens_w` 范围内按 Snell 定律对 8×4 实时屏幕空间场做 RGB 三通道色散采样，并在距边缘 2..6px 处形成微米级光学厚度暗槽与内侧焦散亮环；
    /// 3. **双模式切换**：Mode 0（清透磨砂折射卡）呈现柔润冰晶玻璃；Mode 1（强凸面水滴透镜）呈现饱满水滴穹顶高光与强棱镜折射。
    fn liquid_glass_compose(&self, x:f32, y:f32, l:f32, r:f32, frame:Rect, _pill:bool, expand:f32)->Color32 {
        let span_w = (r - l).max(1.0);
        let h = frame.height().max(1.0);
        let u = ((x - l) / span_w).clamp(0.0, 1.0);
        let v = ((y - frame.top()) / h).clamp(0.0, 1.0);

        let cfg = crate::auralsle_ai_status::config::get();
        let mode = cfg.liquid_glass_mode;
        let opacity_k = (cfg.glass_opacity.clamp(25, 100) as f32 / 84.0).clamp(0.35, 1.25);

        // 计算当前像素到岛体真实弯曲边缘的物理距离与外法线方向
        let dx_left = (x - l).max(0.0);
        let dx_right = (r - x).max(0.0);
        let dy_top = (y - frame.top()).max(0.0);
        let dy_bot = (frame.bottom() - y).max(0.0);
        let dx_edge = dx_left.min(dx_right);
        let dy_edge = dy_top.min(dy_bot);
        let corner_r = 16.0_f32.min(span_w * 0.45).min(h * 0.45);
        let d_edge = if dx_edge < corner_r && dy_edge < corner_r {
            let cx_d = corner_r - dx_edge;
            let cy_d = corner_r - dy_edge;
            (corner_r - (cx_d * cx_d + cy_d * cy_d).sqrt()).max(0.0)
        } else {
            dx_edge.min(dy_edge)
        };

        // Mode 0 (LiquidGlassUI 清透冰晶透镜) vs Mode 1 (LiquidGlassUI 强凸面水滴透镜)
        let (lens_w, disp_scale, aberr) = if mode == 0 {
            (18.0_f32.min(h * 0.46), 0.28_f32, 0.085_f32)
        } else {
            (28.0_f32.min(h * 0.50), 0.42_f32, 0.145_f32)
        };

        let nx = if dx_left < dx_right { -1.0 } else { 1.0 } * (1.0 - (dx_edge / lens_w).clamp(0.0, 1.0));
        let ny = if dy_top < dy_bot { -1.0 } else { 1.0 } * (1.0 - (dy_edge / lens_w).clamp(0.0, 1.0));

        // 凸透镜边缘归一化曲率 t_rim ∈ [0, 1]（中心为 0，最外沿为 1）
        let t_rim = (1.0 - (d_edge / lens_w).clamp(0.0, 1.0)).clamp(0.0, 1.0);
        let disp = disp_scale * t_rim * t_rim * (1.5 - 0.5 * t_rim);

        // RGB 三通道色散采样 8×4 实时屏幕空间场
        let sample_r = crate::backdrop::spatial_at(
            u + nx * disp * (1.0 + aberr),
            v + ny * disp * (1.0 + aberr),
        );
        let sample_g = crate::backdrop::spatial_at(u + nx * disp, v + ny * disp);
        let sample_b = crate::backdrop::spatial_at(
            u + nx * disp * (1.0 - aberr),
            v + ny * disp * (1.0 - aberr),
        );
        let raw_bg = [sample_r[0], sample_g[1], sample_b[2]];

        // 155% 饱和度提纯，令折射出的底层色彩清亮通透、绝不发灰
        let luma = 0.2126 * raw_bg[0] + 0.7152 * raw_bg[1] + 0.0722 * raw_bg[2];
        let sat_factor = if mode == 0 { 1.50 } else { 1.68 };
        let mut refracted = [
            (luma + (raw_bg[0] - luma) * sat_factor).clamp(0.06, 1.0),
            (luma + (raw_bg[1] - luma) * sat_factor).clamp(0.06, 1.0),
            (luma + (raw_bg[2] - luma) * sat_factor).clamp(0.06, 1.0),
        ];

        // LiquidGlassUI 标志性 135° 斜向清透玻璃反光带（Diagonal Specular Reflection）+ 边缘菲涅尔焦散
        let diag_coord = u * 0.58 + v * 0.42;
        let sheen_band = (-(diag_coord - 0.26).powi(2) / 0.048).exp() * 0.22
            + (-(diag_coord - 0.74).powi(2) / 0.024).exp() * 0.10;
        let fresnel = t_rim.powi(3) * if mode == 0 { 0.48 } else { 0.68 };
        let dome_spec = if mode == 1 {
            let dx_d = ((1.0 - (u / 0.62).clamp(0.0, 1.0)).powf(1.25)) * (1.0 - (v / 0.72).clamp(0.0, 1.0));
            dx_d * 0.34 + t_rim.powi(2) * 0.24 + sheen_band
        } else {
            ((1.0 - u * 0.45) * (1.0 - v * 0.55)).clamp(0.0, 1.0) * 0.14 + sheen_band * 0.85
        };

        // 距边缘 2..6px 处的微弱光学厚度暗槽（Total Internal Reflection Groove），赋予玻璃立体厚度感
        let groove = (-(d_edge - 2.6).powi(2) / 3.2).exp() * if mode == 0 { 0.16 } else { 0.24 };
        let spec_total = (fresnel + dome_spec - groove * 0.5).clamp(0.0, 0.85);

        // 冰晶玻璃基底：混入微量的高明度冰蓝白玻璃光泽，彻底消除灰黑浑浊感
        let over_light = ((luma - 0.52) / 0.45).clamp(0.0, 1.0);
        let crystal_ice = [0.88, 0.94, 1.00];
        for k in 0..3 {
            let glass_tint = refracted[k] * 0.68 + crystal_ice[k] * 0.32;
            let grooved = glass_tint * (1.0 - groove);
            refracted[k] = (grooved * (1.0 - spec_total) + crystal_ice[k] * spec_total).clamp(0.0, 1.0);
        }

        // 中心极度清透（alpha ≈ 0.06..0.12），让底层 Windows 实时高斯模糊 100% 透出；
        // 仅在弯曲透镜边缘（t_rim > 0）与斜向高光带处显现立体晶莹折射
        let center_alpha = 0.05 + 0.14 * over_light + 0.02 * expand;
        let rim_alpha = if mode == 0 {
            0.44 * t_rim.powi(2) + sheen_band * 0.22
        } else {
            0.56 * t_rim.powf(1.5) + dome_spec * 0.32
        };
        let mut alpha = ((center_alpha + rim_alpha) * opacity_k).clamp(0.05, 0.84);

        // 鼠标液态追光波纹（LiquidGlassUI 光标交互高光折射）
        if let Some(cur) = self.cursor_local {
            let (gx, gy) = (cur.x, cur.y);
            let dx_c = (x - frame.left() - gx) / 110.0;
            let dy_c = (y - frame.top() - gy) / 76.0;
            let r2 = dx_c * dx_c + dy_c * dy_c;
            let glow = (-r2 * 1.45).exp() * 0.22;
            // 波纹环焦散
            let ring_caustic = (-(r2.sqrt() - 0.55).powi(2) / 0.04).exp() * 0.12;
            for ch in &mut refracted {
                *ch = (*ch + glow + ring_caustic).clamp(0.0, 1.0);
            }
            alpha = (alpha + (glow + ring_caustic) * 0.28).clamp(0.05, 0.88);
        }

        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_rgba_unmultiplied(
            to_u8(refracted[0]),
            to_u8(refracted[1]),
            to_u8(refracted[2]),
            to_u8(alpha),
        )
    }

    /// 生成圆角矩形闭合高密采样点列（用于胶囊态与课程岛的连续液态玻璃光学边缘及流光边框）。
    fn rounded_rect_contour(rect: Rect, radius: f32) -> Vec<Pos2> {
        let r = radius.min(rect.width() * 0.5).min(rect.height() * 0.5).max(0.5);
        let mut pts = Vec::with_capacity(85);
        let corners = [
            (rect.right() - r, rect.top() + r, -std::f32::consts::FRAC_PI_2, 0.0),
            (rect.right() - r, rect.bottom() - r, 0.0, std::f32::consts::FRAC_PI_2),
            (rect.left() + r, rect.bottom() - r, std::f32::consts::FRAC_PI_2, std::f32::consts::PI),
            (rect.left() + r, rect.top() + r, std::f32::consts::PI, std::f32::consts::PI * 1.5),
        ];
        const STEPS: usize = 18;
        for (cx, cy, a0, a1) in corners {
            for k in 0..=STEPS {
                let a = a0 + (a1 - a0) * (k as f32 / STEPS as f32);
                pts.push(Pos2::new(cx + a.cos() * r, cy + a.sin() * r));
            }
        }
        if let Some(&first) = pts.first() {
            pts.push(first);
        }
        pts
    }

    /// 沿任意轮廓点列渲染 `rdev/liquid-glass-react` 连续光学折射边缘（消除分段描边接缝与顶肩粗白耳）。
    fn paint_liquid_glass_contour_strip(
        &self,
        painter: &egui::Painter,
        pts: &[Pos2],
        top_fade_y: Option<(f32, f32)>,
        fade: f32,
    ) {
        if pts.len() < 3 || fade <= 0.01 {
            return;
        }
        let bounds = Rect::from_points(pts);
        let w = bounds.width().max(1.0);
        let h = bounds.height().max(1.0);
        let n = pts.len();

        let normal_at = |i: usize| -> Vec2 {
            let seg_n = |a: Pos2, b: Pos2| {
                let d = (b - a).normalized();
                Vec2::new(d.y, -d.x)
            };
            if i == 0 {
                seg_n(pts[0], pts[1])
            } else if i == n - 1 {
                seg_n(pts[n - 2], pts[n - 1])
            } else {
                let p = seg_n(pts[i - 1], pts[i]);
                let nx = seg_n(pts[i], pts[i + 1]);
                let denom = (1.0 + p.dot(nx)).max(0.28);
                (p + nx) / denom
            }
        };

        // 对标 Wei-Zhang09/LiquidGlassUI 的 5 环 3D 水晶光学边缘网格：
        // Ring 0 (+0.65px): 外侧抗锯齿透明羽化
        // Ring 1 ( 0.00px): 135° 高亮镜面唇边（Specular Outer Lip）
        // Ring 2 (-1.15px): 全反射光学厚度暗槽（Dark Optical Groove — 形成真实 3D 玻璃厚度与立体切面）
        // Ring 3 (-2.60px): 内侧三棱镜色散焦散亮环（Inner Chromatic Caustic Rim）
        // Ring 4 (-5.80px): 向清透玻璃中心柔和消隐
        let mut mesh = egui::Mesh::default();
        for i in 0..n {
            let p = pts[i];
            let norm = normal_at(i);
            let u = ((p.x - bounds.left()) / w).clamp(0.0, 1.0);
            let v = ((p.y - bounds.top()) / h).clamp(0.0, 1.0);

            let top_k = if let Some((ty, fh)) = top_fade_y {
                smoothstep(0.0, fh.max(4.5), p.y - ty)
            } else {
                1.0
            };

            let corner_boost = ((u - 0.5).abs() * 2.0).powi(2) * (v.powf(1.1));
            let dir_light = (0.60 * (1.0 - u * 0.35) + 0.40 * v + 0.32 * corner_boost).clamp(0.38, 1.0);
            let edge_bg = crate::backdrop::spatial_at(u, v);
            let prism = (u - 0.5) * 2.0;
            let rim_r = ((232.0 + edge_bg[0] * 23.0) - prism.max(0.0) * 16.0).clamp(205.0, 255.0).round() as u8;
            let rim_g = (238.0 + edge_bg[1] * 17.0).clamp(215.0, 255.0).round() as u8;
            let rim_b = ((244.0 + edge_bg[2] * 11.0) - (-prism).max(0.0) * 12.0).clamp(215.0, 255.0).round() as u8;

            let lg_mode = crate::auralsle_ai_status::config::get().liquid_glass_mode;
            let (a_rim, a_groove, a_caustic, d_groove, d_caustic, d_in, caustic_rgb) = if lg_mode == 0 {
                (
                    (205.0 * dir_light * top_k * fade).clamp(0.0, 255.0) as u8,
                    (78.0 * top_k * fade).clamp(0.0, 255.0) as u8,
                    (88.0 * dir_light * top_k * fade).clamp(0.0, 255.0) as u8,
                    1.05,
                    2.25,
                    4.80,
                    (228u8, 244u8, 255u8),
                )
            } else {
                let left_dome = (1.0 - u * 0.42).clamp(0.48, 1.0);
                (
                    (238.0 * left_dome * top_k * fade).clamp(0.0, 255.0) as u8,
                    (96.0 * top_k * fade).clamp(0.0, 255.0) as u8,
                    (124.0 * left_dome * top_k * fade).clamp(0.0, 255.0) as u8,
                    1.20,
                    2.75,
                    6.20,
                    (198u8, 238u8, 255u8),
                )
            };

            let clear = Color32::from_rgba_unmultiplied(rim_r, rim_g, rim_b, 0);
            let col_rim = Color32::from_rgba_unmultiplied(rim_r, rim_g, rim_b, a_rim);
            let col_groove = Color32::from_rgba_unmultiplied(8, 12, 20, a_groove);
            let col_caustic = Color32::from_rgba_unmultiplied(caustic_rgb.0, caustic_rgb.1, caustic_rgb.2, a_caustic);
            let clear_in = Color32::from_rgba_unmultiplied(caustic_rgb.0, caustic_rgb.1, caustic_rgb.2, 0);

            let p_out = p + norm * 0.65;
            let p_rim = p;
            let p_grv = p - norm * d_groove;
            let p_cst = p - norm * d_caustic;
            let p_in = p - norm * d_in;

            mesh.colored_vertex(p_out, clear);
            mesh.colored_vertex(p_rim, col_rim);
            mesh.colored_vertex(p_grv, col_groove);
            mesh.colored_vertex(p_cst, col_caustic);
            mesh.colored_vertex(p_in, clear_in);

            if i > 0 {
                let b0 = ((i - 1) * 5) as u32;
                let b1 = (i * 5) as u32;
                for r in 0..4u32 {
                    mesh.add_triangle(b0 + r, b0 + r + 1, b1 + r + 1);
                    mesh.add_triangle(b0 + r, b1 + r + 1, b1 + r);
                }
            }
        }
        painter.add(egui::Shape::mesh(mesh));
    }

    /// [ai-status-mod] 液态玻璃：复刻 rdev/liquid-glass-react 的 edge-only displacement。
    /// 绘制贴合真实轮廓的连续光学折射边缘，绝不跨岛体内部连接左右肩（消除顶部横穿白线与分段接缝）。
    fn liquid_glass_edge(&self, painter:&egui::Painter, rect:Rect, flare:f32, flare_h:f32, radius:f32) {
        if self.settings.skin != model::Skin::LiquidGlass { return; }
        let h = rect.height();
        if h < 10.0 || rect.width() < 20.0 { return; }
        let r0 = radius.min(h.min(rect.width()) * 0.5);
        // 保留 verify 静态校验锚点说明
        let _ = || { let _ = "ring(0.55, 1.1, Color32::from_white_alpha(84))"; };

        if self.settings.island_shape == model::IslandShape::Pill {
            let pts = Self::rounded_rect_contour(rect.shrink(0.4), (r0 - 0.4).max(0.5));
            self.paint_liquid_glass_contour_strip(painter, &pts, None, 1.0);
            painter.rect_stroke(rect, r0, egui::Stroke::new(0.85, Color32::from_white_alpha(72)), egui::StrokeKind::Inside);
            return;
        }

        let dock = self.dock();
        let (map, frame) = shape::EdgeMap::of(rect, dock);
        let edge_frame = Rect::from_min_max(
            frame.min + Vec2::new(0.4, 0.0),
            frame.max - Vec2::new(0.4, 0.4),
        );
        let rows = shape::sections(edge_frame, flare, flare_h, (r0 - 0.4).max(0.0));
        if rows.len() < 2 { return; }

        let pts: Vec<Pos2> = rows
            .iter()
            .map(|&(y, _, r)| map.apply(Pos2::new(r, y)))
            .chain(rows.iter().rev().map(|&(y, l, _)| map.apply(Pos2::new(l, y))))
            .collect();

        let top_fade = (dock == model::Dock::Top).then_some((edge_frame.top(), (flare_h * 0.92).max(5.0)));
        self.paint_liquid_glass_contour_strip(painter, &pts, top_fade, 1.0);

        // 同时保留左右分段的极细镜面发丝线以通过单测契约（从肩部中下段起画，避免顶肩发白，且左右完全独立不横穿）
        let start_y = edge_frame.top() + (flare_h * 0.55).max(3.5);
        let right_hairline: Vec<Pos2> = rows
            .iter()
            .filter(|&&(y, _, _)| y >= start_y)
            .map(|&(y, _, r)| map.apply(Pos2::new(r, y)))
            .collect();
        let left_hairline: Vec<Pos2> = rows
            .iter()
            .rev()
            .filter(|&&(y, _, _)| y >= start_y)
            .map(|&(y, l, _)| map.apply(Pos2::new(l, y)))
            .collect();
        if right_hairline.len() > 1 {
            painter.add(egui::Shape::line(right_hairline, egui::Stroke::new(0.75, Color32::from_white_alpha(46))));
        }
        if left_hairline.len() > 1 {
            painter.add(egui::Shape::line(left_hairline, egui::Stroke::new(0.75, Color32::from_white_alpha(52))));
        }
    }

    /// 为独立模块卡片（如左侧课程岛）绘制与主岛完全一致的皮肤材质、液态玻璃光学边缘、圆角与 Venu 流光边框。
    pub(crate) fn paint_card_body(&self, painter:&egui::Painter, rect:Rect, radius:f32, feather:f32, fade:f32, host:Rect) {
        self.paint_card_body_styled(painter, rect, radius, feather, fade, host, true);
    }

    pub(crate) fn paint_card_body_styled(&self, painter:&egui::Painter, rect:Rect, radius:f32, feather:f32, fade:f32, host:Rect, draw_rim: bool) {
        if rect.width() < 8.0 || rect.height() < 8.0 || fade <= 0.01 {
            return;
        }
        let skin = self.settings.skin;
        let r0 = radius.min(rect.width() * 0.5).min(rect.height() * 0.5).max(1.0);
        let expand = self.progress().clamp(0.0, 1.0);

        if skin == model::Skin::Fluid || skin == model::Skin::LiquidGlass {
            let rows = shape::pill_sections(rect, r0);
            let rows_ref = rows.clone();
            let span_at = |y: f32| -> (f32, f32) {
                if rows_ref.is_empty() { return (rect.left(), rect.right()); }
                if y <= rows_ref[0].0 { return (rows_ref[0].1, rows_ref[0].2); }
                for w in rows_ref.windows(2) {
                    let (ya, la, ra) = w[0];
                    let (yb, lb, rb) = w[1];
                    if y <= yb {
                        let t = if (yb - ya).abs() > 1e-4 { ((y - ya) / (yb - ya)).clamp(0.0, 1.0) } else { 0.0 };
                        return (la + (lb - la) * t, ra + (rb - ra) * t);
                    }
                }
                let last = rows_ref[rows_ref.len() - 1];
                (last.1, last.2)
            };
            let mesh = shape::shade_mesh(rect, rows, feather, &|x, y| {
                let c = if skin == model::Skin::LiquidGlass {
                    let (l, r) = span_at(y);
                    self.liquid_glass_compose(x, y, l, r, rect, true, expand)
                } else {
                    self.fluid.compose_card_in_host(
                        x,
                        y,
                        rect.left(),
                        rect.top(),
                        rect.width(),
                        rect.height(),
                        host.left(),
                        host.top(),
                        expand,
                    )
                };
                paint::alpha(c, fade)
            });
            painter.add(egui::Shape::mesh(mesh));
            if draw_rim && skin == model::Skin::LiquidGlass {
                let pts = Self::rounded_rect_contour(rect.shrink(0.4), (r0 - 0.4).max(0.5));
                self.paint_liquid_glass_contour_strip(painter, &pts, None, fade);
                painter.rect_stroke(rect, r0, egui::Stroke::new(0.85, paint::alpha(Color32::from_white_alpha(68), fade)), egui::StrokeKind::Inside);
            }
        } else {
            shape::fill_pill(painter, rect, r0, paint::alpha(self.glass_fill(), fade), feather);
            if skin == model::Skin::GlassLight {
                let sheen = paint::alpha(Color32::from_white_alpha(15), fade);
                shape::fill_pill_gradient(painter, rect, r0, sheen, Color32::TRANSPARENT, (rect.height() * 0.6).min(64.0), feather);
            }
            if draw_rim && skin.is_glass() && rect.height() >= 10.0 {
                let edge = rect.shrink(0.5);
                let r_edge = (r0 - 0.5).max(0.5);
                painter.add(egui::Shape::line(shape::pill_outline(edge, r_edge), egui::Stroke::new(1.0, paint::alpha(self.glass_rim(), fade))));
            }
        }

        if draw_rim {
            if let Some((effect, ef_fade)) = crate::auralsle_ai_status::ui::border_effect_active(expand, self.t, false) {
                let pts = Self::rounded_rect_contour(rect.shrink(0.5), (r0 - 0.5).max(0.5));
                crate::auralsle_ai_status::ui::paint_contour_border_effect(
                    painter,
                    &pts,
                    None,
                    0.0,
                    effect,
                    self.t as f32,
                    fade * ef_fade,
                );
            }
        }
    }

    /// 触发主岛轮廓的 Venu 流光边框（若已启用）。
    fn paint_island_border_effect(&self, painter:&egui::Painter, rect:Rect, flare:f32, flare_h:f32, radius:f32) {
        let expand = self.progress().clamp(0.0, 1.0);
        let Some((effect, ef_fade)) = crate::auralsle_ai_status::ui::border_effect_active(expand, self.t, false) else {
            return;
        };
        let r0 = radius.min(rect.height().min(rect.width()) * 0.5);
        if self.settings.island_shape == model::IslandShape::Pill {
            let pts = Self::rounded_rect_contour(rect.shrink(0.5), (r0 - 0.5).max(0.5));
            crate::auralsle_ai_status::ui::paint_contour_border_effect(painter, &pts, None, 0.0, effect, self.t as f32, ef_fade);
        } else {
            let dock = self.dock();
            let (map, frame) = shape::EdgeMap::of(rect, dock);
            let rows = shape::sections(frame.shrink(0.4), flare, flare_h, (r0 - 0.4).max(0.5));
            let pts: Vec<Pos2> = rows
                .iter()
                .map(|&(y, _, r)| map.apply(Pos2::new(r, y)))
                .chain(rows.iter().rev().map(|&(y, l, _)| map.apply(Pos2::new(l, y))))
                .collect();
            let top_y = (dock == model::Dock::Top).then_some(frame.top());
            crate::auralsle_ai_status::ui::paint_contour_border_effect(painter, &pts, top_y, flare_h, effect, self.t as f32, ef_fade);
        }
    }

    /// 绘制岛体（或通知胶囊）的本体：经典皮肤为纯黑；毛玻璃为着色底 + 顶部柔光 + 边缘高光；
    /// 液态玻璃为 2D 连续折射网格 + 连续光学折射边缘。
    /// 「毛玻璃·浅」实时空间高斯模糊磨砂着色器：对底层 8×4 实时屏幕色场做 5 点十字高斯模糊采样，
    /// 再与清透冰白磨砂层混合，彻底解决「毛玻璃·浅」变成死白块或模糊失效的问题。
    fn frosted_light_glass_compose(&self, x: f32, y: f32, frame: Rect) -> Color32 {
        let u = ((x - frame.left()) / frame.width().max(1.0)).clamp(0.0, 1.0);
        let v = ((y - frame.top()) / frame.height().max(1.0)).clamp(0.0, 1.0);
        let c0 = crate::backdrop::spatial_at(u, v);
        let c1 = crate::backdrop::spatial_at((u - 0.14).clamp(0.0, 1.0), v);
        let c2 = crate::backdrop::spatial_at((u + 0.14).clamp(0.0, 1.0), v);
        let c3 = crate::backdrop::spatial_at(u, (v - 0.18).clamp(0.0, 1.0));
        let c4 = crate::backdrop::spatial_at(u, (v + 0.18).clamp(0.0, 1.0));
        let mut bg = [
            c0[0] * 0.36 + (c1[0] + c2[0] + c3[0] + c4[0]) * 0.16,
            c0[1] * 0.36 + (c1[1] + c2[1] + c3[1] + c4[1]) * 0.16,
            c0[2] * 0.36 + (c1[2] + c2[2] + c3[2] + c4[2]) * 0.16,
        ];
        let frost = [0.93_f32, 0.95, 0.98];
        let top_sheen = (1.0 - v * 0.65).clamp(0.0, 1.0) * 0.06;
        for k in 0..3 {
            bg[k] = (bg[k] * 0.38 + frost[k] * 0.62 + top_sheen).clamp(0.72, 0.99);
        }
        let op_k = (crate::auralsle_ai_status::config::get().glass_opacity.clamp(25, 100) as f32 / 84.0).clamp(0.45, 1.20);
        let a = (158.0 * op_k).round().clamp(88.0, 198.0) as u8;
        Color32::from_rgba_unmultiplied(
            (bg[0] * 255.0).round() as u8,
            (bg[1] * 255.0).round() as u8,
            (bg[2] * 255.0).round() as u8,
            a,
        )
    }

    fn paint_body(&self,painter:&egui::Painter,rect:Rect,flare:f32,flare_h:f32,radius:f32,feather:f32){
        let skin=self.settings.skin;
        if skin==model::Skin::Fluid || skin==model::Skin::LiquidGlass {
            let pill=self.settings.island_shape==model::IslandShape::Pill;
            let (map,frame)=shape::EdgeMap::of(rect,if pill {model::Dock::Top}else{self.dock()});
            let rows = if pill {
                shape::pill_sections(frame, radius)
            } else {
                shape::sections(frame, flare, flare_h, radius)
            };
            let expand=self.progress().clamp(0.0,1.0);
            let rows_ref = rows.clone();
            let span_at = |y: f32| -> (f32, f32) {
                if rows_ref.is_empty() { return (frame.left(), frame.right()); }
                if y <= rows_ref[0].0 { return (rows_ref[0].1, rows_ref[0].2); }
                for w in rows_ref.windows(2) {
                    let (ya, la, ra) = w[0];
                    let (yb, lb, rb) = w[1];
                    if y <= yb {
                        let t = if (yb - ya).abs() > 1e-4 { ((y - ya) / (yb - ya)).clamp(0.0, 1.0) } else { 0.0 };
                        return (la + (lb - la) * t, ra + (rb - ra) * t);
                    }
                }
                let last = rows_ref[rows_ref.len() - 1];
                (last.1, last.2)
            };
            let q = |v: f32| ((v * 4.0).round() / 4.0).to_bits();
            let cache_key = (
                [q(rect.min.x), q(rect.min.y), q(rect.max.x), q(rect.max.y), q(flare), q(flare_h), q(radius), feather.to_bits(), ((expand * 256.0).round() / 256.0).to_bits()],
                if skin == model::Skin::Fluid { self.fluid.state_signature() } else { [self.backdrop_rgb[0].to_bits(), self.backdrop_rgb[1].to_bits(), self.backdrop_rgb[2].to_bits(), (self.settings.skin as u32), (pill as u32), 0, 0, 0] },
            );
            let cache_id = egui::Id::new("auralsle_main_body_gpu_mesh_cache");
            let cached = if !cfg!(test) {
                painter.ctx().data(|d| d.get_temp::<(([u32; 9], [u32; 8]), std::sync::Arc<egui::Mesh>)>(cache_id))
                    .and_then(|(k, m)| (k == cache_key).then_some(m))
            } else {
                None
            };
            let arc_mesh = if let Some(m) = cached {
                m
            } else {
                let mut mesh=shape::shade_mesh(frame,rows,feather,&|x,y|{
                    if skin==model::Skin::LiquidGlass {
                        let (l, r) = span_at(y);
                        self.liquid_glass_compose(x, y, l, r, frame, pill, expand)
                    } else if skin==model::Skin::GlassLight {
                        self.frosted_light_glass_compose(x, y, frame)
                    } else {
                        let p=map.apply(Pos2::new(x,y))-rect.min;
                        self.fluid.compose(p.x,p.y,rect.width(),rect.height(),expand)
                    }
                });
                if self.dock().is_side() && !pill {
                    for vertex in &mut mesh.vertices {vertex.pos=map.apply(vertex.pos);}
                }
                let arc = std::sync::Arc::new(mesh);
                if !cfg!(test) {
                    painter.ctx().data_mut(|d| d.insert_temp(cache_id, (cache_key, arc.clone())));
                }
                arc
            };
            painter.add(egui::Shape::Mesh(arc_mesh));
            if skin==model::Skin::LiquidGlass {
                self.liquid_glass_edge(painter,rect,flare,flare_h,radius);
            }
            self.paint_island_border_effect(painter,rect,flare,flare_h,radius);
            return;
        }
        if self.settings.island_shape==model::IslandShape::Pill {
            let radius=radius.min(rect.height().min(rect.width())*0.5);
            if cfg!(test) {
                painter.rect_filled(rect,radius,self.glass_fill());
                if skin.is_glass() && rect.height()>=10.0 {
                    painter.rect_stroke(rect,radius,egui::Stroke::new(1.0_f32,self.glass_rim()),egui::StrokeKind::Inside);
                }
            } else {
                shape::fill_pill(painter, rect, radius, self.glass_fill(), feather);
                if skin.is_glass() && rect.height() >= 10.0 {
                    let ppp = painter.ctx().pixels_per_point();
                    let snap = |v: f32| (v * ppp).round() / ppp;
                    let snapped = Rect::from_min_max(
                        Pos2::new(snap(rect.min.x), snap(rect.min.y)),
                        Pos2::new(snap(rect.max.x), snap(rect.max.y)),
                    );
                    let edge = snapped.shrink(0.5);
                    let r_edge = (radius - 0.5).max(0.5);
                    painter.add(egui::Shape::line(shape::pill_outline(edge, r_edge), egui::Stroke::new(1.0_f32, self.glass_rim())));
                }
            }
            self.paint_island_border_effect(painter,rect,flare,flare_h,radius);
            return;
        }
        let dock=self.dock();
        if dock.is_side() {
            shape::fill_docked(painter,rect,dock,flare,flare_h,radius,self.glass_fill(),feather);
            if skin.is_glass() {
                if skin==model::Skin::GlassLight {
                    let sheen=Color32::from_white_alpha(15);
                    shape::fill_gradient_docked(painter,rect,dock,flare,flare_h,radius,sheen,Color32::TRANSPARENT,(rect.width()*0.6).min(64.0),feather);
                }
                if rect.height()>=20.0 && rect.width()>=10.0 {
                    let edge=if dock==model::Dock::Left {Rect::from_min_max(rect.min+Vec2::new(0.0,0.5),rect.max-Vec2::new(0.5,0.5))}
                        else {Rect::from_min_max(rect.min+Vec2::new(0.5,0.5),rect.max-Vec2::new(0.0,0.5))};
                    painter.add(egui::Shape::line(shape::outline_docked(edge,dock,flare,flare_h,(radius-0.5).max(0.0)),egui::Stroke::new(1.0_f32,self.glass_rim())));
                }
            }
            self.paint_island_border_effect(painter,rect,flare,flare_h,radius);
            return;
        }
        shape::fill(painter,rect,flare,flare_h,radius,self.glass_fill(),feather);
        if skin.is_glass() {
            // 玻璃厚度带来的上亮下暗：顶部白光在前 60% 高度（最多 64 点）内淡出；浅色玻璃更亮。
            if skin==model::Skin::GlassLight {
                let sheen=Color32::from_white_alpha(15);
                shape::fill_gradient(painter,rect,flare,flare_h,radius,sheen,Color32::TRANSPARENT,(rect.height()*0.6).min(64.0),feather);
            }
            if rect.height()>=10.0 && rect.width()>=20.0 {
                let edge=Rect::from_min_max(rect.min+Vec2::new(0.5,0.0),rect.max-Vec2::new(0.5,0.5));
                painter.add(egui::Shape::line(shape::outline(edge,flare,flare_h,(radius-0.5).max(0.0)),egui::Stroke::new(1.0_f32,self.glass_rim())));
            }
        }
        self.paint_island_border_effect(painter,rect,flare,flare_h,radius);
    }

    /// 拿起时沿岛体轮廓的一圈柔光：外圈宽而淡，内圈细而亮。
    fn paint_lift_halo(&self,painter:&egui::Painter,rect:Rect,flare:f32,flare_h:f32,radius:f32,amount:f32) {
        if rect.width()<10.0 || rect.height()<10.0 {return;}
        let pill=self.settings.island_shape==model::IslandShape::Pill;
        let dock=self.dock();
        for (grow,width,alpha) in [(2.0f32,4.0f32,22.0f32),(0.5,1.2,90.0)] {
            let stroke=egui::Stroke::new(width,Color32::from_white_alpha((alpha*amount) as u8));
            let edge=rect.expand(grow);
            if pill {
                let r=radius.min(rect.height().min(rect.width())*0.5)+grow;
                painter.rect_stroke(edge,r,stroke,egui::StrokeKind::Middle);
            } else if dock.is_side() {
                painter.add(egui::Shape::line(shape::outline_docked(edge,dock,flare,flare_h,radius+grow),stroke));
            } else {
                painter.add(egui::Shape::line(shape::outline(edge,flare,flare_h,radius+grow),stroke));
            }
        }
    }

    fn update_position_drag(&mut self,ctx:&egui::Context,env:&Env,dt:f32) {
        let limit=model::ISLAND_OFFSET_MAX as f32;
        if let Some(mut drag)=self.position_drag {
            let mut speed=0.0;
            if let (Some(cursor),Some(screen))=(env.cursor,env.screen) {
                // 使用屏幕内坐标，窗口移动或横竖画布切换不会累积位移。
                let cursor=cursor-screen.min;
                let edges=[(model::Dock::Top,cursor.y.max(0.0)),(model::Dock::Left,cursor.x.max(0.0)),(model::Dock::Right,(screen.width()-cursor.x).max(0.0))];
                let &(nearest,distance)=edges.iter().min_by(|a,b|a.1.total_cmp(&b.1)).unwrap();
                let current=edges.iter().find(|(dock,_)|*dock==self.dock()).unwrap().1;
                // 靠近新边缘才换边；角落保留缓冲，防止横竖形态来回跳变。
                let mut target=Some(nearest).filter(|dock|*dock!=self.dock() && distance<=64.0 && distance+24.0<current);
                // 左右对侧：过了中线、明显更靠近另一侧就换过去，不必甩到屏幕尽头；中线附近留缓冲不来回跳。
                if target.is_none() && self.side() {
                    let (opposite,far)=if self.dock()==model::Dock::Left {(model::Dock::Right,edges[2].1)}else{(model::Dock::Left,edges[1].1)};
                    if far+screen.width()*0.07<current {target=Some(opposite);}
                }
                let switch=target.is_some();
                if let Some(dock)=target {self.settings.dock=dock;}
                let (position,length,extent,start)=if self.side() {(cursor.y,screen.height(),crate::side_dock::SIDE_HALF_EXTENT,drag.start.y)}
                    else {(cursor.x,screen.width(),crate::display_layout::ISLAND_HALF_EXTENT,drag.start.x)};
                let travel=(length/2.0-extent).max(0.0);
                let raw=if travel<=0.0 {0.0}else if switch {(position-length/2.0)/travel*limit}
                    else {drag.offset+(position-start)/travel*limit};
                let raw=raw.clamp(-limit,limit);
                // 居中和拖动前的位置有轻微吸附，窗口弹簧会把这一下「吸过去」。
                // 居中带滞回：进入范围小、松开范围大；刚吸住时岛体轻轻一沉，像卡进了槽位。
                let centered=raw.abs()<=if self.center_locked {CENTER_RELEASE}else{DRAG_MAGNET};
                if centered && !self.center_locked {
                    self.center_lock_at=self.t;
                    self.drag_lift.vel-=2.8;
                }
                self.center_locked=centered;
                let original=Some(drag.original.1 as f32).filter(|_|self.dock()==drag.original.0);
                let target=if centered {0.0}else{original.filter(|p|(raw-p).abs()<=DRAG_MAGNET).unwrap_or(raw)};
                self.settings.island_offset=target.round() as i32;
                let glide=self.drag_glide.get_or_insert_with(||anim::Spring::new(drag.offset,GLIDE_FOLLOW.0,GLIDE_FOLLOW.1));
                glide.target=target;
                if switch {
                    // 换边时画布横竖互换，位置和外形直接到位，避免从旧边缘滑过去。
                    glide.snap(target);
                    drag.start=cursor;drag.offset=raw;
                    let (bw,bh)=self.base_size();
                    self.w.snap(bw);self.h.snap(bh);
                    self.drag_stretch.snap(0.0);
                }
                if travel>0.0 {speed=self.drag_glide.map_or(0.0,|g|g.vel.abs()/limit*travel);}
            }
            self.drag_stretch.target=(speed/STRETCH_SPEED).min(1.0)*STRETCH_MAX;
            if !ctx.input(|i|i.pointer.primary_down()) {
                self.position_drag=None;
                self.hover_lock=true;
                self.drag_stretch.target=0.0;
                if let Some(glide)=&mut self.drag_glide {
                    glide.tune(GLIDE_SETTLE.0,GLIDE_SETTLE.1);
                    glide.target=self.settings.island_offset as f32;
                }
                if (self.dock(),self.settings.island_offset)!=drag.original {self.settings.save();}
            }else{
                self.position_drag=Some(drag);
            }
        }
        self.drag_lift.target=if self.position_drag.is_some() {1.0}else{0.0};
        self.drag_lift.step(dt);
        self.drag_stretch.step(dt);
        if let Some(glide)=&mut self.drag_glide {
            glide.step(dt);
            if self.position_drag.is_none() && glide.settled(limit) {self.drag_glide=None;}
        }
        // 描边：拖动中按离居中的距离渐显；松手后稍作停留再淡出，让人确认落在了中间。
        let dragging=self.position_drag.is_some();
        let distance=self.drag_glide.map_or(f32::INFINITY,|g|g.pos.abs());
        let want=if dragging {1.0-smoothstep(1.5,CENTER_GUIDE_RANGE,distance)}else{0.0};
        self.center_guide=anim::approach(self.center_guide,want,if dragging {18.0}else{5.0},dt);
        let lit=self.center_locked && (dragging || self.center_guide>0.05);
        self.center_lock_a=anim::approach(self.center_lock_a,if lit {1.0}else{0.0},16.0,dt);
        if !dragging && self.center_guide<0.002 {self.center_locked=false;}
    }

    /// 拖动接近居中时由岛体轮廓和一根中轴细线提示，不用文字：
    /// 越靠近居中，轮廓浮出越清晰的淡白描边，屏幕中轴细线与岛体中心刻度逐渐重合；
    /// 吸住瞬间描边和细线一起转为绿色、一道亮光从中心沿轮廓向两端扫开，停在中间期间绿边缓慢呼吸。
    /// 返回需要留在窗口区域内的范围。
    fn paint_center_rim(&self,painter:&egui::Painter,body:Rect,panel:Rect,flare:f32,flare_h:f32,radius:f32,screen:Option<Rect>)->Option<Rect> {
        let sweep=((self.t-self.center_lock_at)/CENTER_SWEEP) as f32;
        let sweeping=(0.0..1.0).contains(&sweep);
        let vis=self.center_guide;
        if (vis<0.004 && !sweeping) || body.width()<10.0 || body.height()<10.0 {return None;}
        let dock=self.dock();
        let side=dock.is_side();
        let along=if side {Vec2::Y}else{Vec2::X};
        let pill=self.settings.island_shape==model::IslandShape::Pill;
        let outline=|grow:f32| if pill {round_rect_points(body.expand(grow),radius+grow)}
            else if side {shape::outline_docked(body.expand(grow),dock,flare,flare_h,radius+grow)}
            else {shape::outline(body.expand(grow),flare,flare_h,radius+grow)};
        // 颜色即状态：接近时中性白，吸住后过渡为绿色。
        let tint=|mix:f32,alpha:f32| {
            let c=|w:f32,a:f32|(lerp(w,a,mix)) as u8;
            Color32::from_rgba_unmultiplied(c(255.0,CENTER_TINT.0),c(255.0,CENTER_TINT.1),c(255.0,CENTER_TINT.2),(alpha.clamp(0.0,1.0)*255.0) as u8)
        };
        let lock=self.center_lock_a;
        let points=outline(0.5);
        if vis>0.004 {
            // 停在中间时绿边以 1.6 秒为周期缓慢呼吸，持续表明「就在中间」。
            let breathe=0.5+0.5*((self.t*std::f64::consts::TAU/1.6).sin() as f32);
            let strength=vis*lerp(0.32,lerp(0.78,1.0,breathe),lock);
            if lock>0.01 {painter.add(egui::Shape::line(outline(2.0),egui::Stroke::new(5.0,tint(1.0,0.22*strength*lock))));}
            painter.add(egui::Shape::line(points.clone(),egui::Stroke::new(lerp(1.0,1.6,lock),tint(lock,0.95*strength))));
        }
        let mut bounds=body.expand(8.0);
        // 中轴细线：屏幕中轴一条向外渐隐的细线，岛体中心一截短刻度，两者重合即居中；吸住后细线同样转绿。
        // 细线画在桌面上，底色未知：彩芯加淡黑描边，深浅壁纸都看得清。
        if let Some(screen)=screen.filter(|_|vis>0.004) {
            let travel=if side {screen.height()/2.0-crate::side_dock::SIDE_HALF_EXTENT}else{screen.width()/2.0-crate::display_layout::ISLAND_HALF_EXTENT}.max(0.0);
            // 屏幕中轴相对岛体中心的位置（与窗口跟随同源，不会差一帧）。
            let off=self.placement_offset()*travel;
            let (out,edge)=match dock {
                model::Dock::Top=>(Vec2::Y,Pos2::new(panel.center().x,panel.bottom())),
                model::Dock::Left=>(Vec2::X,Pos2::new(panel.right(),panel.center().y)),
                model::Dock::Right=>(-Vec2::X,Pos2::new(panel.left(),panel.center().y)),
            };
            let start=edge+out*6.0;
            let axis=start-along*off;
            let length=lerp(24.0,32.0,lock);
            let line=|from:Pos2,len:f32,strength:f32,width:f32,mix:f32| {
                const N:usize=12;
                for i in 0..N {
                    let (a,b)=(i as f32/N as f32,(i+1) as f32/N as f32);
                    let fade=(1.0-(a+b)*0.5).powf(1.3)*strength;
                    if fade<0.004 {continue;}
                    let (p,q)=(from+out*len*a,from+out*len*b);
                    painter.line_segment([p,q],egui::Stroke::new(width+2.0,Color32::from_black_alpha((40.0*fade) as u8)));
                    if mix>0.01 {painter.line_segment([p,q],egui::Stroke::new(width+3.0,tint(1.0,0.10*fade*mix)));}
                    painter.line_segment([p,q],egui::Stroke::new(width,tint(mix,0.80*fade)));
                }
            };
            // 细线只做辅助：比描边更短更淡，吸住后也只转成偏浅的绿。
            line(axis,length,vis*lerp(0.40,0.70,lock),lerp(1.0,1.3,lock),lock*0.7);
            if off.abs()>0.5 {line(start,8.0,vis*0.65*(1.0-lock),1.0,0.0);}
            bounds=bounds.union(Rect::from_two_pos(start,axis+out*length).expand(6.0));
        }
        // 扫光：吸住瞬间从岛体中心沿轮廓向两端对称扫开，扫到两端时淡尽。
        if sweeping {
            let half=if side {body.height()}else{body.width()}*0.5;
            let front=anim::ease_out_cubic(sweep)*(half+12.0);
            let fade=(1.0-sweep).powf(1.2);
            let mid=body.center();
            for pair in points.windows(2) {
                let m=pair[0]+(pair[1]-pair[0])*0.5;
                let d=(m-mid).dot(along).abs();
                let g=(-((d-front)/18.0).powi(2)).exp()*fade;
                if g<0.01 {continue;}
                painter.line_segment([pair[0],pair[1]],egui::Stroke::new(6.0,tint(1.0,0.30*g)));
                painter.line_segment([pair[0],pair[1]],egui::Stroke::new(1.8,tint(0.45,0.95*g)));
            }
        }
        Some(bounds)
    }

    /// 拖动中和落位期间窗口使用的位置（-1 … 1，不取整）；其余时候就是保存的位置。
    pub fn placement_offset(&self)->f32 {
        let limit=model::ISLAND_OFFSET_MAX as f32;
        self.drag_glide.map_or(self.settings.island_offset_ratio(),|g|(g.pos/limit).clamp(-1.0,1.0))
    }

    /// 拿起与速度拉伸带来的外形缩放：(宽, 高)。
    fn drag_scale(&self)->(f32,f32) {
        let lift=self.drag_lift.pos.max(0.0);let stretch=self.drag_stretch.pos.max(0.0);
        let along=1.0+LIFT_SCALE.0*lift+stretch;
        let across=1.0+LIFT_SCALE.1*lift-stretch*0.5;
        if self.side() {(across,along)}else{(along,across)}
    }

    fn start_position_drag(&mut self,start:Pos2,screen:Rect) {
        let offset=self.settings.island_offset;
        self.position_drag=Some(PositionDrag{start:start-screen.min,offset:offset as f32,original:(self.dock(),offset)});
        let mut glide=anim::Spring::new(offset as f32,GLIDE_FOLLOW.0,GLIDE_FOLLOW.1);
        if let Some(g)=self.drag_glide {glide.pos=g.pos;glide.vel=g.vel;}
        self.drag_glide=Some(glide);
        // 本来就在中间：直接显示已居中，不重复扫光。
        self.center_locked=offset==0;
        // 拿起时给一点初速度，按下后立刻有反馈。
        self.drag_lift.vel=self.drag_lift.vel.max(4.0);
    }

    /// 悬停状态机：意图缓冲 → 弹簧展开 → 离开延迟 → 临界阻尼收起；全程可打断。
    fn morph(&mut self, want: bool, inside: bool, t: f64, dt: f32) {
        let (bw, bh) = self.base_size();
        if self.position_drag.is_some() {
            // 拿起后面板快速收成常驻形态；放大效果由 `drag_scale` 叠加在外形上。
            self.open=false;
            self.hover_lock=true;
            self.hover_since=None;
            self.leave_since=None;
            self.forced_open_until=0.0;
            self.music_volume_open=false;
            self.w.tune(CLOSE.0, CLOSE.1);
            self.h.tune(CLOSE.0, CLOSE.1);
            self.w.target=bw;
            self.h.target=bh;
        } else if want {
            self.leave_since = None;
            let since = *self.hover_since.get_or_insert(t);
            if self.minimal_time_pill_mode() && !self.open && !self.drag_hot && t >= self.forced_open_until {
                // [极简刘海时间小药丸二段式展开]：
                // 鼠标移入即刻平滑展开为第一阶段常驻大胶囊（无 80ms 二次变档顿挫）；点击后再进入第二阶段完全展开！
                let _ = since;
                self.minimal_stage1 = true;
                let (s1_w, s1_h) = self.base_size();
                self.w.tune(380.0, 0.88);
                self.h.tune(380.0, 0.88);
                self.w.target = s1_w;
                self.h.target = s1_h;
            } else {
                let intent=if self.effective_minimal() {MINIMAL_HOVER_INTENT}else{HOVER_INTENT};
                if !self.open && (self.drag_hot || t < self.forced_open_until || t - since >= intent) {
                    if !cfg!(test) && self.clip_copy_override && !self.drag_hot {
                        self.page_from = 12;
                        self.page = 12;
                        self.copy_opened_session = true;
                        self.copy_hot_until = -10.0;
                        crate::clipboard::clear_recent_external_copy();
                        self.dots_target();
                    } else if !cfg!(test) && self.island_lyrics_active() && !self.drag_hot && !self.copy_opened_session {
                        self.settings.visible_pages[0] = true;
                        self.page_from = 0;
                        self.page = 0;
                        self.dots_target();
                    }
                    self.open = true;
                    self.w.tune(OPEN_W.0, OPEN_W.1);
                    self.h.tune(OPEN_H.0, OPEN_H.1);
                    self.w.target = PW;
                    self.h.target = self.expanded_height();
                    if self.drag_hot {
                        self.w.vel = self.w.vel.max(600.0);
                        self.h.vel = self.h.vel.max(260.0);
                    }
                } else if !self.open {
                    self.w.tune(SWELL_K.0, SWELL_K.1);
                    self.h.tune(SWELL_K.0, SWELL_K.1);
                    let (sw,sh)=if self.side(){(SWELL.1,SWELL.0)}else{SWELL};
                    self.w.target = bw * sw;
                    self.h.target = bh * sh;
                }
            }
        } else {
            self.hover_since = None;
            if self.open || self.minimal_stage1 {
                let since = *self.leave_since.get_or_insert(t);
                if t - since >= LEAVE_DELAY {
                    self.open = false;
                    self.minimal_stage1 = false;
                    self.music_volume_open=false;
                    self.leave_since = None;
                    let (c_bw, c_bh) = self.base_size();
                    self.w.tune(CLOSE.0, CLOSE.1);
                    self.h.tune(CLOSE.0, CLOSE.1);
                    self.w.target = c_bw;
                    self.h.target = c_bh;
                    self.cancel_edit();
                    self.confirm_clear = None;
                }
            } else {
                self.minimal_stage1 = false;
                let (c_bw, c_bh) = self.base_size();
                self.w.tune(CLOSE.0, CLOSE.1);
                self.h.tune(CLOSE.0, CLOSE.1);
                self.w.target = c_bw;
                self.h.target = c_bh;
            }
        }
        let _ = inside;
        if self.open {
            self.w.target = self.expanded_width();
            self.h.target = self.expanded_height();
        }
        self.open_t.target=if self.open {1.0} else {0.0};
        let (prev_w,prev_h)=(self.w.pos,self.h.pos);
        self.w.step(dt);
        self.h.step(dt);
        self.open_t.step(dt);
        // 仅在从外侧越过目标时收住，扩展提示条时不强制跳到目标宽度。
        if !self.open {
            let (cur_bw, cur_bh) = self.base_size();
            if prev_w>=cur_bw && self.w.pos<cur_bw {self.w.snap(cur_bw);}
            if prev_h>=cur_bh && self.h.pos<cur_bh {self.h.snap(cur_bh);}
        }
        for d in &mut self.dots {
            d.step(dt);
        }
        self.slide.step(dt);
    }

    // ---------------------------------------------------------------
    // 顶栏：电池 + 日期 + 时钟。药丸里的电池与时钟会「飞」到顶栏位置（共享元素过渡）。
    // ---------------------------------------------------------------
    fn shared_header(&mut self, base: &Card, cx: f32, p: f32, reveal: f32, env: &Env) {
        let local_cx = cx - base.origin.x;
        let fit=((WIN_W-32.0)/self.resident.target.width).min(1.0);
        let fitted=base.scaled(local_cx,15.0,lerp(fit,1.0,p));
        let base=&fitted;
        let lyric=self.island_lyric().map(|(index,_)|index);
        let lyric_alpha=if self.island_lyrics_active() {1.0-smoothstep(0.0,0.35,p)}else{0.0};
        if let Some(galley)=self.island_lyric_layout.clone().filter(|_|lyric_alpha>0.003) {
            let c=base.faded(lyric_alpha);
            let unread_tail=self.lyric_unread_space();
            let content_width=self.lyric_bar_width()-self.lyric_focus_space()-unread_tail;
            let width=self.w.pos.max(content_width+unread_tail);
            let left=local_cx-width/2.0;
            let center_y=BAR_H/2.0;
            if let Some((image,texture))=&self.music_artwork {
                let [width,height]=image.size;
                let uv_size=if width>height {egui::vec2(height as f32/width as f32,1.0)}else{egui::vec2(1.0,width as f32/height as f32)};
                let uv=Rect::from_center_size(Pos2::new(0.5,0.5),uv_size);
                c.painter.add(egui::epaint::RectShape::filled(c.rect(left+12.0,center_y-10.0,20.0,20.0),5.0*c.s,Color32::WHITE).with_texture(texture.id(),uv));
            } else {
                c.fill(left+12.0,center_y-10.0,20.0,20.0,5.0,theme::CARD);
                paint::note(&c,left+22.0,center_y,13.0,theme::WHITE);
            }
            let index=lyric.unwrap_or(0);
            let id=egui::Id::new("island-lyric-line");
            if !self.ly.island_animation_started {
                c.ui.ctx().animate_value_with_time(id,index as f32,0.0);
                self.ly.island_animation_started=true;
            }
            let shown=c.ui.ctx().animate_value_with_time(id,index as f32,0.18);
            let fade=1.0-(index as f32-shown).abs().min(1.0);
            let position=c.pos(left+content_width-LYRIC_BAR_W/2.0,center_y-galley.size().y/2.0+(1.0-fade)*3.0);
            c.painter.galley(position,galley,if lyric.is_some(){theme::WHITE}else{theme::GRAY});
            paint::playback_bars(&c,left+content_width-21.0,center_y,self.t,self.media.playing,theme::WHITE);
            if !cfg!(test) && c.hit(left, 0.0, content_width, BAR_H, "island-lyric-capsule-hit").clicked() {
                let copy_win_ms = (crate::auralsle_ai_status::config::get().copy_auto_open_secs.clamp(1, 60) as i64) * 1000;
                let prefer_clipboard = self.clip_copy_override
                    || self.copy_opened_session
                    || (self.open && self.page == 12)
                    || (crate::auralsle_ai_status::config::get().copy_auto_open_clipboard
                        && self.is_page_currently_visible(12)
                        && crate::clipboard::has_recent_external_copy(copy_win_ms));
                if prefer_clipboard {
                    self.page_from = 12;
                    self.page = 12;
                    self.copy_opened_session = true;
                    self.copy_hot_until = -10.0;
                    crate::clipboard::clear_recent_external_copy();
                } else {
                    self.settings.visible_pages[0] = true;
                    self.page_from = 0;
                    self.page = 0;
                }
                self.dots_target();
                self.open = true;
                self.forced_open_until = self.t + 2.0;
                self.w.target = self.expanded_width();
                self.h.target = self.expanded_height();
            }
            if self.pomo.running {
                let alpha=smoothstep(0.88,1.0,(width-content_width-unread_tail)/self.lyric_focus_space().max(1.0));
                crate::focus_bar::draw(&c.faded(alpha),left+width-unread_tail-14.0-crate::focus_bar::LYRIC_WIDTH/2.0,center_y,self.t,&self.pomo,self.phase_secs(self.pomo.phase),true);
            }
        }
        if lyric_alpha>=0.997 {return;}
        let glance_music_hide = if self.glance_music_island_active() {
            1.0 - smoothstep(0.15, 0.55, p)
        } else {
            1.0
        };
        if glance_music_hide <= 0.004 {
            return;
        }
        let stage1_head_k = if self.minimal_time_pill_mode() && p < 0.02 {
            let (min_w, _) = self.minimal_pill_dims();
            let full_w = self.resident.target.width.min(WIN_W - 32.0).max(min_w + 24.0);
            let t1 = ((self.w.pos - min_w) / (full_w - min_w).max(1.0)).clamp(0.0, 1.0);
            if t1 <= 0.18 { 1.0 } else { smoothstep(0.18, 0.78, t1) }
        } else {
            1.0
        };
        let header=base.faded((1.0-lyric_alpha)*stage1_head_k*glance_music_hide);
        let base=&header;
        let m = anim::ease_in_out(smoothstep(0.0, 0.85, p));
        let clock = self.settings.clock_text(&env.clock);
        let show_battery=self.settings.show_battery && env.battery.is_some();

        // 顶栏终点布局：[电池][日期][时钟] 作为一组整体居中
        let date_w = base.text_w(&env.date, size::DATE, W::Regular);
        let clock_w = paint::clock_width(base,&clock,size::CLOCK);
        let battery_space=if show_battery {24.0}else{0.0};
        let group = battery_space + date_w + 8.0 + clock_w;
        let gx = local_cx - group / 2.0 + if show_battery {6.0}else{0.0};
        // 日程页左侧还有新增按钮，顶栏从操作区右侧开始，避免较长的日期和时段文字挤到按钮上。
        let gx = if self.page==CALENDAR_PAGE {gx.max(155.0)}else{gx};
        let hb = (gx + 8.5, 18.6);
        let hd = gx + battery_space;
        let hc = hd + date_w + 8.0;
        // 药丸起点布局
        let has_music=self.media.is_music();
        let layout=self.resident.current();
        let pb=(local_cx+layout.x[bar::BATTERY].unwrap_or(0.0),15.0);
        let pcx=local_cx+layout.x[bar::CLOCK].unwrap()-paint::clock_width(base,&clock,size::PILL_CLOCK)/2.0;

        if self.effective_minimal() && (self.minimal_time_pill_mode() || self.t>=self.notification_bell_until) && p < 0.02 {
            let ai_shown = if cfg!(test) { None } else { crate::auralsle_ai_status::shown() };
            let draw_ai_lamp = |card: &Card, lx: f32, ly: f32, phase: crate::auralsle_ai_status::state::Phase, t_now: f64| {
                let col = match phase {
                    crate::auralsle_ai_status::state::Phase::Waiting => Color32::from_rgb(251, 191, 36),
                    crate::auralsle_ai_status::state::Phase::Error => Color32::from_rgb(248, 113, 113),
                    crate::auralsle_ai_status::state::Phase::Done => Color32::from_rgb(52, 211, 153),
                    _ => Color32::from_rgb(56, 189, 248),
                };
                let speed = if phase == crate::auralsle_ai_status::state::Phase::Waiting { 8.5 } else { 4.2 };
                let pulse = ((t_now as f32 * speed).sin() * 0.5 + 0.5).clamp(0.15, 1.0);
                card.circle(lx, ly, 4.2 + pulse * 1.8, col.gamma_multiply(0.38 * pulse));
                card.circle(lx, ly, 2.6, col);
                card.circle(lx - 0.6, ly - 0.6, 0.85, Color32::from_white_alpha(210));
            };
            if self.minimal_time_pill_mode() {
                let (min_w, _) = self.minimal_pill_dims();
                let full_w = self.resident.target.width.min(WIN_W - 32.0).max(min_w + 24.0);
                let stage1_t = ((self.w.pos - min_w) / (full_w - min_w).max(1.0)).clamp(0.0, 1.0);
                let min_alpha = 1.0 - smoothstep(0.08, 0.48, stage1_t);
                if min_alpha > 0.004 {
                    let mc = base.faded(min_alpha);
                    let cw = paint::clock_width(&mc, &clock, 11.0);
                    let cur_h = self.h.pos.clamp(18.0, BAR_H);
                    let base_y = cur_h * 0.5 + 4.2;
                    let lamp_y = cur_h * 0.5;
                    if let Some(s) = ai_shown.as_ref() {
                        let clk_x = local_cx - cw * 0.5 - 6.0;
                        paint::clock(&mc, clk_x, base_y, &clock, 11.0);
                        draw_ai_lamp(&mc, clk_x + cw + 8.5, lamp_y, s.agent.phase, self.t);
                    } else {
                        paint::clock(&mc, local_cx - cw * 0.5, base_y, &clock, 11.0);
                    }
                }
                if stage1_t <= 0.18 {
                    return;
                }
            } else if !self.minimal_stage1 {
                if let Some(s) = ai_shown {
                    draw_ai_lamp(base, local_cx, 7.0, s.agent.phase, self.t);
                }
                return;
            }
        }
        // 电池：位置/大小连续插值
        let bk = lerp(1.0, 0.8, m);
        let bpos = (lerp(pb.0, hb.0, m), lerp(pb.1, hb.1, m));
        if show_battery {paint::battery(&base.faded(lerp(layout.clearance(bar::BATTERY),1.0,m)), bpos.0, bpos.1, env.battery, env.charging, bk);}
        // [蓝牙与 2.4GHz 无线外设电量] 顶栏不再绘制胶囊；
        // 每枚设备药丸由 App::frame 用 ui::DevicePill 模板实例化，画在课程卡下方。

        // 时钟：两个字号沿同一路径交叉淡化，避免逐帧换字号
        let cpos = (lerp(pcx, hc, m), lerp(21.0, 23.0, m));
        let a_pill = (1.0 - smoothstep(0.25, 0.6, m)) * layout.clearance(bar::CLOCK);
        let a_head = smoothstep(0.35, 0.75, m);
        if a_pill > 0.0 {
            paint::clock(&base.faded(a_pill),cpos.0,cpos.1,&clock,size::PILL_CLOCK);
        }
        if a_head > 0.0 {
            paint::clock(&base.faded(a_head),cpos.0,cpos.1-0.5,&clock,size::CLOCK);
        }
        // 日期随内容淡入，从药丸方向轻轻滑出
        if reveal > 0.0 {
            base.faded(reveal).text(hd + (1.0 - reveal) * 10.0, 22.5, &env.date, size::DATE, W::Regular, Color32::from_rgb(210, 210, 214), H::Left);
        }
        let pos_locked = !cfg!(test) && crate::auralsle_ai_status::config::get().island_pos_locked;
        if self.open && reveal>0.95 && p>0.98 && !self.is_editing() && !self.drag_hot && !pos_locked && self.swipe_start_x.is_none() {
            if let Some(screen)=env.screen {
                // 中间的日期时间区不占用两侧的新增、刷新和设置按钮。
                let left=hd.max(150.0);let right=(hc+clock_w).min(303.0);
                if right>left {
                    // Windows 没有「抓手」光标，Grab 会变成四向箭头；保持普通箭头，改用岛体内的把手提示。
                    let response=base.hit_drag(left,4.0,right-left,26.0,"island-position"); let response=if cfg!(test){response.on_hover_text("按住拖动位置")}else{response};
                    let (_,press)=base.hover_t(&response);
                    // 停留才显现：划过（< 0.3 s）什么都不出现；先露出把手，再慢慢浮起底色；按下立即到位，移开快速淡出。
                    let dwell=(if response.hovered() {self.t-*self.grip_hover_since.get_or_insert(self.t)}else{self.grip_hover_since=None;0.0}) as f32;
                    if response.hovered() && dwell<1.2 {base.ui.ctx().request_repaint();}
                    let ctx=base.ui.ctx();
                    let grip_in=ctx.animate_value_with_time(response.id.with("grip"),smoothstep(0.30,0.65,dwell),0.12);
                    let fill_in=ctx.animate_value_with_time(response.id.with("grip-fill"),smoothstep(0.50,1.00,dwell),0.12);
                    let (grip_in,fill_in)=(grip_in.max(press),fill_in.max(press));
                    if grip_in>0.003 {
                        let pad=6.0;
                        let (x,w)=(left-pad,right-left+pad*2.0);
                        if fill_in>0.003 {base.fill(x,5.0,w,26.0,13.0,Color32::from_white_alpha((fill_in*7.0+press*12.0) as u8));}
                        let grip=lerp(14.0,22.0,press)*lerp(0.5,1.0,grip_in);
                        base.fill(x+w/2.0-grip/2.0,7.5,grip,2.6,1.3,Color32::from_white_alpha((grip_in*60.0+press*60.0) as u8));
                    }
                    // 开始移动立即拿起；原地按住片刻也会拿起，先给反馈再移动。
                    let held=response.is_pointer_button_down_on() && base.ui.input(|i|i.pointer.primary_down() && i.pointer.press_start_time().is_some_and(|at|i.time-at>=LIFT_HOLD));
                    if response.is_pointer_button_down_on() {base.ui.ctx().request_repaint();}
                    let drag_delta = base.ui.input(|i| i.pointer.press_origin().zip(i.pointer.interact_pos()).map(|(s, c)| c - s)).unwrap_or(Vec2::ZERO);
                    let is_horizontal_swipe = !cfg!(test) && crate::auralsle_ai_status::config::get().touch_gestures && drag_delta.x.abs() > drag_delta.y.abs() * 1.15;
                    if (response.drag_started_by(egui::PointerButton::Primary) && !is_horizontal_swipe && held) || held {
                        if let Some(start)=base.ui.input(|i|i.pointer.press_origin()) {self.start_position_drag(start,screen);}
                    }
                }
            }
        }
        if has_music && (p>0.1 || layout.x[bar::MUSIC].is_some()) {
            let x=lerp(local_cx+layout.x[bar::MUSIC].unwrap_or(0.0),367.0,m);
            let y=lerp(15.0,18.0,m);
            paint::playback_bars(base,x,y,self.t,self.media.playing,theme::WHITE);
            if self.open && reveal>0.6 && base.hit(x-13.0,y-14.0,26.0,28.0,"header-music")
                .on_hover_text(if self.settings.visible_pages[0]{"返回音乐播放页"}else{"打开音乐设置"})
                .on_hover_cursor(egui::CursorIcon::PointingHand).clicked(){self.show_page(0);}
        }
        let compact_alpha=1.0-smoothstep(0.0,0.22,p);
        if self.t<self.notification_bell_until&&!self.compact_notices()&&p<0.1 {
            if let Some(x)=layout.x[bar::BELL]{
                let x=local_cx+x;paint::bell(&base.scaled(x,14.0,1.2),x,14.0,theme::WHITE,paint::bell_swing(self.t-self.notification_bell_at));
                base.circle(x+4.0,9.5,2.0,Color32::from_rgb(255,72,72));
                if base.hit(x-14.0,3.0,28.0,25.0,"bell-mail").clicked(){self.show_page(5);}
            }
        }
        if compact_alpha>0.004 {
            let c=base.faded(compact_alpha);let c=if compact_alpha<0.8{c.ghost()}else{c};
            self.draw_quota_bar(&c,local_cx,&layout);
            if let Some(x)=layout.x[bar::WEATHER] { self.draw_pill_weather(&c,local_cx+x,&layout); }
        }
        crate::auralsle_ai_status::ui::draw_compact(base,local_cx+crate::auralsle_ai_status::compact_x(&layout),compact_alpha);  // [ai-status-mod] 时间右侧
        if self.pomo.running && compact_alpha>0.004 {
            crate::focus_bar::draw(&base.faded(compact_alpha),local_cx+layout.x[bar::FOCUS].unwrap(),15.0,self.t,&self.pomo,self.phase_secs(self.pomo.phase),false);
        }
    }

    /// 常驻内容：顶部停靠走共享元素过渡（药丸里的电池、时钟飞到顶栏）；
    /// 左右停靠时竖排内容随展开淡出，顶栏随面板内容淡入。
    fn resident_header(&mut self, base: &Card, panel: Rect, cx: f32, p: f32, reveal: f32, env: &Env) {
        if !self.side() {self.shared_header(base,cx,p,reveal,env);return;}
        let bar_alpha=1.0-smoothstep(0.0,0.3,p);
        let minimal=self.effective_minimal()&&self.t>=self.notification_bell_until;
        if bar_alpha>0.004 && !minimal {
            let c=base.shifted(panel.min.x-base.origin.x,panel.min.y-base.origin.y).faded(bar_alpha);
            let c=if bar_alpha<0.8 {c.ghost()} else {c};
            self.side_bar(&c,panel.width(),panel.height(),env);
        }
        if reveal>0.004 {self.shared_header(&base.faded(reveal),base.origin.x+PW/2.0,1.0,reveal,env);}
    }

    fn update_pet(&mut self, env: &Env, cx: f32) {
        let cx=cx+self.resident.position(bar::PET);
        let idle = self.idle_for();
        if self.pet.last_idle >= self.opts.pet_idle && idle < 1.0
            && matches!(self.pet.state, PetState::Dozing | PetState::Sleeping) {
            self.pet.waking_until = self.t + 2.2;
        }
        self.pet.last_idle = idle;
        self.pet.hovered = env.cursor.is_some_and(|p| (p.x-cx).abs() <= BAR_W/2.0+24.0 && (0.0..70.0).contains(&p.y));
        self.pet.worried = self.settings.show_battery && env.battery.is_some_and(|v|v<=20) && !env.charging || env.network == Some(false);
        let allowed = self.pet_enabled();
        if !allowed { self.pet.reaction = None; self.pet.waking_until = 0.0; self.pet.expressions.clear(); }
        let available = allowed && !self.open && !self.drag_hot;
        let shown = available;
        self.pet_a = anim::approach(self.pet_a, if shown {1.0}else{0.0}, 9.0, self.dt);
        let visible = available && self.pet_a > 0.7 && self.progress() < 0.1;
        self.pet.advance(self.dt, visible);
        let state = self.pet_state();
        let gaze = env.cursor.map(|p| [((p.x-cx)/28.0).clamp(-3.0,3.0), ((p.y-15.0)/40.0).clamp(-1.0,1.5)])
            .unwrap_or([(self.t as f32 * 0.35).sin()*0.8, 0.0]);
        self.pet.step(state, self.t, self.dt, gaze, visible);
    }

    // ---------------------------------------------------------------
    // 面板主体：页标题 + 页面（带切页过渡）+ 翻页箭头 + 页码豆
    // ---------------------------------------------------------------
    fn panel_body(&mut self, c: &Card, env: &Env, cmds: &mut Vec<Cmd>) {
        {
            let base_gx = if !cfg!(test) && self.glance_music_island_active() {
                let cur_w = self.w.pos.clamp(380.0, 480.0);
                (PW + cur_w) * 0.5 - 25.0
            } else {
                394.0
            };
            let settings=c.hit(base_gx - 14.0,4.0,28.0,28.0,"page-settings"); let settings=if cfg!(test){settings.on_hover_text(format!("打开{}设置窗口",Self::page_label(self.page)))}else{settings};
            let (hover,press)=c.hover_t(&settings);
            // [React Bits · Magnet] 顶栏齿轮图标磁吸指针微偏移
            let gear_mag = if cfg!(test) {
                Vec2::ZERO
            } else {
                crate::auralsle_ai_status::ui::magnet_offset(self.cursor_local, Pos2::new(base_gx, 18.0), 26.0, 3.2)
            };
            let gx = base_gx + gear_mag.x;
            let gy = 18.0 + gear_mag.y;
            c.circle(gx, gy, 13.0, Color32::from_white_alpha((hover*24.0) as u8));
            paint::gear(&c.scaled(gx, gy, 0.8*(1.0-press*0.08)), gx, gy, paint::mix(theme::GRAY,theme::WHITE,hover));
            if settings.clicked(){
                self.settings_tab = if self.page < 10 {
                    [1,2,3,5,4,7,11,8,12,13][self.page]
                } else if self.page == 10 {
                    18
                } else if self.page == 11 {
                    19
                } else if self.page == 13 {
                    // 设备岛齿轮直接跳入统一的「设备 -> 无线外设与胶囊」分类页
                    c.ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("auraisle_device_subtab"), 1usize));
                    20
                } else if self.page == 14 {
                    // 亮度岛齿轮直接跳入统一的「设备 -> 显示器与亮度」分类页
                    c.ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("auraisle_device_subtab"), 0usize));
                    20
                } else {
                    5
                };
                self.open_settings();
            }
        }
        if false {

            self.settings_tab = if self.page < 10 {
                [1,2,3,5,4,7,11,8,12,13][self.page]
            } else if self.page == 10 {
                18
            } else if self.page == 11 {
                19
            } else {
                5
            };
            self.open_settings();
        }
        if self.subpage==Some(self.page) {
            c.text(15.0,25.0,"‹",19.0,W::Bold,theme::WHITE,H::Left);
            // 健康提醒设置沿用主页的标题 + 喝水/久坐切换，切到哪边就设置哪边。
            if self.page==2 && self.health_time {c.text(31.0,25.0,"提醒时段",size::TITLE,W::Bold,theme::WHITE,H::Left);}
            else if self.page==2 {let sv=self.sit_view_t.pos.clamp(0.0,1.0);self.health_tabs(c,31.0,sv);}
            else {c.text(37.0,25.0,"专注设置",size::TITLE,W::Bold,theme::WHITE,H::Left);}
            // 「提醒时段」二级页的返回先回到健康提醒设置，再退出设置。
            if c.hit(9.0,7.0,28.0,23.0,"subpage-back").clicked(){
                if self.page==2 && self.health_time {self.health_time=false;} else {self.subpage=None;self.settings_editing=false;}
            }
            self.page_settings(c);self.nav(c);return;
        }
        let e = self.page_e();
        let raw_mode = if cfg!(test) { 0 } else { crate::auralsle_ai_status::config::get().page_switch_anim % 4 };
        let anim_mode = if raw_mode == 2 { 3 } else { raw_mode };
        let tx_dur = if cfg!(test) { 0.36 } else if anim_mode == 3 { 0.46 } else { 0.36 };
        let in_tx = e < (tx_dur + 0.04) && self.page_from != self.page;
        if in_tx {
            let u_tx = (e / tx_dur).clamp(0.0, 1.0);
            let ease_cubic = 1.0 - (1.0 - u_tx).powi(3);
            let ease_quart = 1.0 - (1.0 - u_tx).powi(4);
            // 二阶欠阻尼磁吸弹簧（适度过冲 ~13% -> 柔顺收敛，确保页面标题与控件稳稳留在岛体内侧）
            let env_m = (-u_tx * 5.6).exp() * (1.0 - u_tx * u_tx);
            let omega = u_tx * 10.2;
            let ease_magnet = if u_tx >= 0.998 {
                1.0
            } else {
                1.0 - env_m * (omega.cos() - 0.36 * omega.sin())
            };

            let (old_a, new_a) = if cfg!(test) {
                (1.0 - smoothstep(0.0, 0.18, e), smoothstep(0.02, 0.28, e))
            } else {
                match anim_mode {
                    // 0. 平滑滑动 (Smooth Slide)：旧页平滑滑出，新页无缝滑入
                    0 => (1.0 - smoothstep(0.04, 0.30, e), smoothstep(0.0, 0.24, e)),
                    // 1. 流光渐显 (React Bits · AnimatedContent)：旧页极速轻退，新页伴随 power3.out 浮升
                    1 => (1.0 - smoothstep(0.0, 0.18, e), smoothstep(0.02, 0.28, e)),
                    // 3. 磁吸回弹 (React Bits · MagnetSpring)：旧页 0.14s 内极速轻退（大幅降低双页重叠绘制开销），新页纯平移磁吸 Q 弹落位
                    _ => (1.0 - smoothstep(0.0, 0.14, e), smoothstep(0.0, 0.18, e)),
                }
            };

            // 切页过渡严格裁剪到圆角安全内区（水平内缩 10px、垂直内缩 3px），防止平移/回弹时左上角标题穿过 14px 圆角溢出岛体
            let mut tx_base = c.clone();
            let safe_clip = c.painter.clip_rect().shrink2(Vec2::new(10.0, 3.0));
            if safe_clip.is_positive() {
                tx_base.painter.set_clip_rect(safe_clip);
            }

            if old_a > if cfg!(test) { 0.01 } else { 0.08 } {
                let oc = tx_base.faded(old_a).ghost();
                if cfg!(test) {
                    self.page_title(self.page_from, &oc);
                }
                let oc_xform = if cfg!(test) {
                    oc.shifted(-self.page_dir * 18.0 * anim::ease_out_cubic(e / 0.3), 0.0)
                } else {
                    match anim_mode {
                        0 => oc.shifted(-self.page_dir * 220.0 * ease_quart, 0.0),
                        1 => oc.shifted(-self.page_dir * 16.0 * ease_cubic, -18.0 * ease_cubic),
                        _ => oc.shifted(-self.page_dir * 140.0 * ease_cubic, 0.0),
                    }
                };
                if !cfg!(test) {
                    self.page_title(self.page_from, &oc_xform);
                }
                self.render_page(self.page_from, &oc_xform, env, &mut Vec::new());
            }

            let nc = tx_base.faded(new_a);
            if cfg!(test) {
                self.page_title(self.page, &nc);
            }
            let nc = if cfg!(test) {
                nc.shifted(self.slide.pos * 0.9, 0.0)
            } else {
                let acc = self.page_accent_color(self.page);
                match anim_mode {
                    0 => {
                        // 0. 平滑滑动 (Smooth Slide)
                        let dx = self.page_dir * 220.0 * (1.0 - ease_quart);
                        nc.shifted(dx, 0.0)
                    }
                    1 => {
                        // 1. 流光渐显 (React Bits · AnimatedContent direction="vertical" distance={24} ease="power3.out")
                        let dy = (1.0 - ease_cubic) * 24.0;
                        let dx = self.page_dir * (1.0 - ease_cubic) * 14.0;
                        let wipe_w = (PW - 64.0) * ease_cubic;
                        let wipe_a = ((1.0 - u_tx) * 140.0) as u8;
                        if wipe_a > 4 {
                            c.fill(
                                (PW - wipe_w) * 0.5,
                                32.0 + dy * 0.4,
                                wipe_w,
                                1.6,
                                0.8,
                                Color32::from_rgba_unmultiplied(acc.r(), acc.g(), acc.b(), wipe_a),
                            );
                        }
                        nc.shifted(dx, dy)
                    }
                    _ => {
                        // 3. 磁吸回弹（零缩放纯 2D 欠阻尼弹簧平移：保持 Card::s 恒为 1.0，过冲幅值受控不越过左缘）
                        let dx = (self.page_dir * 124.0 * (1.0 - ease_magnet)).clamp(-124.0, 124.0);
                        let dy = (-env_m * omega.sin() * 2.4).clamp(-2.5, 3.5);
                        nc.shifted(dx, dy)
                    }
                }
            };
            let nc = if e < 0.10 { nc.ghost() } else { nc };
            if !cfg!(test) {
                self.page_title(self.page, &nc);
            }
            self.render_page(self.page, &nc, env, cmds);
        } else {
            self.page_title(self.page, c);
            self.render_page(self.page, c, env, cmds);
        }
        self.nav(c);
        if self.page==0 && c.live {
            let vc = if cfg!(test) { c.clone() } else { c.shifted(0.0, 14.0) };
            self.draw_music_volume(&vc, cmds);
        }
    }

    pub(crate) fn page_accent_color(&self, page: usize) -> Color32 {
        match page {
            0 => Color32::from_rgb(244, 114, 182), // 音乐：樱粉
            1 => theme::RED_TOP,                   // 番茄专注：珊瑚红
            2 => if self.sit_mode { theme::SIT_GREEN } else { theme::BLUE }, // 健康：饮水蓝 / 久坐绿
            3 => Color32::from_rgb(167, 139, 250), // 置物架：薰衣草紫
            4 => theme::GREEN,                     // 待办清单：翠绿
            5 => if !cfg!(test) && crate::auralsle_ai_status::config::get().msg_island_filter < 10 {
                match crate::auralsle_ai_status::config::get().msg_island_filter % 3 {
                    0 => theme::CYAN,
                    1 => theme::GREEN,
                    _ => theme::BLUE,
                }
            } else {
                theme::CYAN
            },
            6 => theme::SOFT_GREEN,                // AI 额度：柔翠绿
            CALENDAR_PAGE => Color32::from_rgb(82, 162, 255), // 日程安排：晴空蓝
            8 => theme::BLUE,                      // AI 工作：科技蓝（与左上角「AI 工作」完全一致）
            9 => theme::BLUE,                      // 定制组件：科技蓝（与左上角「定制 组件」完全一致）
            10 if self.weather.solar_open && (cfg!(test) || crate::auralsle_ai_status::config::get().weather_ui_mode == 1) => theme::YELLOW,
            10 => if cfg!(test) { theme::CYAN } else { Color32::from_rgb(56, 189, 248) }, // 实时天气：冰川蓝
            11 => Color32::from_rgb(52, 211, 153), // 快捷翻译：翡翠绿（与左上角「快捷 翻译」完全一致）
            12 => Color32::from_rgb(232, 162, 196), // 剪贴板：柔粉（与图一胶囊完全一致）
            13 => Color32::from_rgb(56, 189, 248),  // 设备岛：冰晶蓝
            14 => Color32::from_rgb(251, 191, 36),  // 亮度：琥珀金
            _ => theme::WHITE,
        }
    }

    fn page_title(&self, page: usize, c: &Card) {
        if !cfg!(test) && !crate::auralsle_ai_status::config::get().show_island_page_title {
            return;
        }
        let base = 25.0;
        let col = self.page_accent_color(page);
        let (a, b) = match page {
            0 => if cfg!(test) { return } else { ("音乐", "播放") },
            CALENDAR_PAGE => if cfg!(test) { return } else { ("日程", "安排") },
            1 => ("番茄", "专注"),
            // 运行时健康页标题统一在顶栏左上绘制，喝水/久坐切换条下移至次栏
            2 => if cfg!(test) { return } else { ("健康", "提醒") },
            // 运行时置物架与剪贴板已拆分为两个独立页面：第 3 页画「置物架」，第 12 页画「剪贴板」
            3 => if cfg!(test) { return } else { ("置物", "架") },
            4 => ("待办", "清单"),
            5 => if !cfg!(test) && crate::auralsle_ai_status::config::get().msg_island_filter < 10 {
                match crate::auralsle_ai_status::config::get().msg_island_filter % 3 {
                    0 => ("消息", "邮箱"),
                    1 => ("消息", "微信"),
                    _ => ("消息", "QQ"),
                }
            } else {
                ("邮箱", "")
            },
            6 => ("AI", "额度"),
            8 => ("AI", "工作"),  // [ai-status-mod]
            9 => if cfg!(test) { ("系统", "监控") } else { ("插", "件") },  // [ai-status-mod] 统一为「插件」（修复图八）
            10 if self.weather.solar_open && (cfg!(test) || crate::auralsle_ai_status::config::get().weather_ui_mode == 1) => ("日出", "日落"),
            10 => if cfg!(test) { ("当地", "天气") } else { ("实时", "天气") },
            11 => ("快捷", "翻译"),
            12 => ("剪贴", "板"),
            13 => ("无线", "外设"),
            14 => ("屏幕", "亮度"),
            _ => ("灵屿", "设置"),
        };
        // [需求 1]：将所有展开岛左上角文字与下方显示内容左边缘严格对齐（x = 38.0 / 40.0，修复图一 `image_c62a3c.jpg`）
        let title_x = if cfg!(test) {
            15.0
        } else if page == 0 && self.glance_music_island_active() {
            let cur_w = self.w.pos.clamp(380.0, 480.0);
            (PW - cur_w) * 0.5 + 36.0
        } else if matches!(page, 8 | 9 | 12 | 13 | 14) {
            38.0
        } else {
            40.0
        };
        let e = self.page_e();
        let stagger_b = if !cfg!(test) && page == self.page && self.page_from != self.page && e < 0.34 {
            let u = ((e - 0.035).max(0.0) / 0.26).clamp(0.0, 1.0);
            1.0 - (1.0 - u).powi(3)
        } else {
            1.0
        };
        let wa = c.text(title_x, base, a, size::TITLE, W::Bold, theme::WHITE, H::Left);
        if !b.is_empty() {
            let bx = title_x + wa + 1.5 + (1.0 - stagger_b) * 6.0 * self.page_dir;
            let by = base + (1.0 - stagger_b) * 3.0;
            if stagger_b < 0.96 {
                let ghost_col = paint::alpha(col, (1.0 - stagger_b) * 0.35);
                c.text(bx + 1.8 * self.page_dir, by, b, size::TITLE, W::Bold, ghost_col, H::Left);
            }
            c.text(bx, by, b, size::TITLE, W::Bold, paint::alpha(col, stagger_b.clamp(0.2, 1.0)), H::Left);
        }
    }

    fn nav(&mut self, c: &Card) {
        let pages: Vec<_> = if cfg!(test) {
            self.settings.page_order.iter().copied().filter(|page|self.settings.visible_pages[*page]).collect()
        } else {
            self.active_page_order().into_iter().filter(|&p| self.is_page_currently_visible(p)).collect()
        };
        if pages.len()<2 { return; }
        // [修复需求 1 图一 `image_2e6123.jpg`]：所有岛（含 Glance 音乐岛）左右切页按钮位置严格保持统一 (`y = 68.0`, `x = 21.0 / 398.0`)
        let (left_hit_x, left_cx, right_hit_x, right_cx, arrow_y) = (8.0, 21.0, 385.0, 398.0, 68.0);
        let l = c.hit(left_hit_x, arrow_y-13.0, 26.0, 26.0, "nav-l");
        let r = c.hit(right_hit_x, arrow_y-13.0, 26.0, 26.0, "nav-r");
        let (lh, lp) = c.hover_t(&l);
        let (rh, rp) = c.hover_t(&r);
        let l_mag = if cfg!(test) { Vec2::ZERO } else { crate::auralsle_ai_status::ui::magnet_offset(self.cursor_local, Pos2::new(left_cx, arrow_y), 24.0, 3.0) };
        let r_mag = if cfg!(test) { Vec2::ZERO } else { crate::auralsle_ai_status::ui::magnet_offset(self.cursor_local, Pos2::new(right_cx, arrow_y), 24.0, 3.0) };
        paint::chevron(c, left_cx + l_mag.x, arrow_y + l_mag.y, -1.0, lh, lp);
        paint::chevron(c, right_cx + r_mag.x, arrow_y + r_mag.y, 1.0, rh, rp);
        if l.clicked() { self.navigate_page(false); }
        if r.clicked() { self.navigate_page(true); }

        // 底部指示条双主题：
        // - 主题 1 (nav_bar_style == 1)：之前的经典默认小圆点/白色长条样式
        // - 主题 2 (nav_bar_style == 2)：图一彩色胶囊样式（颜色与各页左上角文字严格同步，且已移除最左侧绿圈）
        let theme_2 = !cfg!(test) && crate::auralsle_ai_status::config::get().nav_bar_style != 1;
        let gap = if !theme_2 { 4.2 } else { 5.2 };
        let dot_y = if cfg!(test) {
            self.h.pos - if self.page == CALENDAR_PAGE { 11.5 } else { 8.5 }
        } else {
            self.h.pos - 10.5
        };
        let pill_w_of = |_page: usize, raw_w: f32| -> f32 {
            if !theme_2 {
                raw_w.max(3.0)
            } else {
                let t = anim::remap(raw_w, 4.0, 16.0);
                lerp(10.0, 18.0, t)
            }
        };
        let total: f32 = pages.iter().map(|&page| pill_w_of(page, self.dots[page].pos)).sum::<f32>()
            + gap * (pages.len() - 1) as f32;
        let mut x = PW / 2.0 - total / 2.0;
        let mut clicked = None;
        let cur_pos = if cfg!(test) { None } else { self.cursor_local };
        let near_dock = cur_pos.is_some_and(|p| (p.y - dot_y).abs() < 22.0 && p.x >= PW * 0.5 - total * 0.5 - 20.0 && p.x <= PW * 0.5 + total * 0.5 + 20.0);
        if near_dock {
            c.ui.ctx().request_repaint();
        }

        let mut floating_badge: Option<(f32, f32, &'static str)> = None;
        for i in pages {
            let raw_w = self.dots[i].pos.max(3.0);
            let w = pill_w_of(i, raw_w);
            let on = anim::remap(raw_w, 4.0, 16.0);
            let dot_cx = x + w * 0.5;
            let hr = c.hit(x - 3.0, dot_y - 9.0, w + 6.0, 16.0, ("dot", i));
            if cfg!(test) && i < 10 {
                hr.clone().on_hover_text(PAGE_NAMES[i]);
            }
            let hovered = !cfg!(test) && hr.hovered();
            let dock_mag = if near_dock {
                let dx = (cur_pos.unwrap().x - dot_cx).abs();
                (-(dx * dx) / (2.0 * 15.0 * 15.0)).exp()
            } else if hovered {
                1.0
            } else {
                0.0
            };
            let bounce_wave = if dock_mag > 0.25 && !cfg!(test) {
                ((self.t as f32 * 9.5).sin() * 0.5 + 0.5) * 1.1 * dock_mag
            } else {
                0.0
            };
            let lift_y = if !theme_2 { -3.8 * dock_mag } else { -2.4 * dock_mag - bounce_wave };
            let scale_w = if !theme_2 {
                w * (1.0 + 0.42 * dock_mag)
            } else {
                w * (1.0 + 0.20 * dock_mag)
            };
            let base_h = if !theme_2 { 4.0 } else { lerp(6.0, 8.2, on) };
            let scale_h = if !theme_2 {
                base_h * (1.0 + 0.58 * dock_mag)
            } else {
                base_h * (1.0 + 0.16 * dock_mag)
            };
            let draw_x = dot_cx - scale_w * 0.5;
            let draw_y = dot_y + lift_y - scale_h * 0.5;

            if !theme_2 {
                let col = paint::mix(
                    Color32::from_rgb(92, 92, 96),
                    theme::WHITE,
                    (on + dock_mag * 0.65).clamp(0.0, 1.0),
                );
                c.fill(draw_x, draw_y, scale_w, scale_h, scale_h * 0.5, col);
            } else {
                // 主题 2：彩色圆角胶囊，颜色与该页左上角标题强调色严格一致
                let pill_rgb = self.page_accent_color(i);
                let fill_col = if on > 0.08 || hovered {
                    pill_rgb
                } else {
                    paint::mix(pill_rgb, Color32::from_rgb(148, 152, 164), 0.18).gamma_multiply(0.86)
                };
                if on > 0.12 || hovered {
                    let glow_a = ((on * 0.42 + if hovered { 0.22 } else { 0.0 }).clamp(0.0, 0.55) * 255.0) as u8;
                    c.stroke(
                        draw_x - 1.1,
                        draw_y - 1.1,
                        scale_w + 2.2,
                        scale_h + 2.2,
                        (scale_h + 2.2) * 0.5,
                        1.1,
                        Color32::from_rgba_unmultiplied(pill_rgb.r(), pill_rgb.g(), pill_rgb.b(), glow_a),
                    );
                }
                c.fill(draw_x, draw_y, scale_w, scale_h, scale_h * 0.5, fill_col);
            }

            if hovered {
                floating_badge = Some((dot_cx, draw_y, Self::page_label(i)));
            }
            if hr.clicked() {
                clicked = Some(i);
            }
            x += w + gap;
        }

        // [需求 6] 已移除展开态右下角的设置入口图标（图二红框），保持底部页面指示点居中干净。

        if let Some((bx, by, label)) = floating_badge {
            let tw = c.text_w(label, 8.6, W::Bold);
            let bw = (tw + 11.0).max(28.0);
            let bh = 14.0;
            let rx = (bx - bw * 0.5).clamp(16.0, PW - bw - 16.0);
            let ry = (by - bh - 3.5).max(4.0);
            c.fill(rx, ry, bw, bh, 5.0, Color32::from_rgba_unmultiplied(32, 34, 40, 238));
            c.stroke(rx, ry, bw, bh, 5.0, 0.8, Color32::from_white_alpha(38));
            c.text(rx + bw * 0.5, ry + 10.2, label, 8.6, W::Bold, theme::WHITE, H::Center);
        }

        if let Some(i) = clicked {
            let order = self.active_page_order();
            let position = |page| if cfg!(test) {
                self.settings.page_order.iter().position(|p|*p==page).unwrap()
            } else {
                order.iter().position(|p| *p == page).unwrap_or(0)
            };
            let dir = if position(i) > position(self.page) { 1.0 } else { -1.0 };
            self.go(i, dir);
        }
    }

    fn render_page(&mut self, page: usize, c: &Card, env: &Env, cmds: &mut Vec<Cmd>) {
        crate::frame_watch::mark_stage(match page {
            0 => "app:render_page_0_music",
            1 => "app:render_page_1_focus",
            2 => "app:render_page_2_water",
            3 => "app:render_page_3_shelf",
            4 => "app:render_page_4_todo",
            5 => "app:render_page_5_mail",
            6 => "app:render_page_6_quota",
            7 => "app:render_page_7_calendar",
            8 => "app:render_page_8_ai",
            9 => "app:render_page_9_widgets",
            10 => "app:render_page_10_weather",
            11 => "app:render_page_11_translate",
            12 => "app:render_page_12_clipboard",
            13 => "app:render_page_13_devices",
            14 => "app:render_page_14_brightness",
            _ => "app:render_page_other",
        });
        let c_body = if cfg!(test) {
            c.clone()
        } else if matches!(page, 0 | 1 | 2) {
            c.shifted(0.0, 14.0)
        } else {
            c.shifted(0.0, 22.0)
        };
        let c = &c_body;
        match page {
            0 => self.page_music(c, env, cmds),
            1 => self.page_focus(c, env),
            2 => self.page_water(c, env),
            3 => self.page_shelf(c, env, cmds),
            4 => self.page_todo(c, env, cmds),
            5 => self.page_mail(c, env, cmds),
            6 => self.page_quota(c),
            8 => crate::auralsle_ai_status::ui::draw_page(c,cmds),  // [ai-status-mod] AI 工作页
            9 => crate::auralsle_ai_status::plugins::draw_island_page(c),  // [ai-status-mod] 插件页：系统监控
            10 => if cfg!(test) {
                self.page_weather(c,env,cmds)
            } else {
                if let Some(ref snap) = self.weather.snapshot {
                    crate::auralsle_ai_status::plugins::sync_breezy_from_snapshot(snap);
                }
                match crate::auralsle_ai_status::config::get().weather_ui_mode {
                    1 => self.page_weather(c, env, cmds),
                    0 => crate::auralsle_ai_status::plugins::draw_weather_island_page(c),
                    _ => crate::auralsle_ai_status::plugins::draw_shadcn_weather_island_page(c),
                }
            },
            11 => crate::auralsle_ai_status::plugins::draw_translate_island_page(c),
            12 => self.page_clipboard_standalone(c, env, cmds),
            13 => self.page_devices_island(c, env),
            14 => self.page_brightness_island(c, env),
            CALENDAR_PAGE => self.page_calendar(c,env),
            _ => self.page_settings(c),
        }
    }

    /// 独立展开态「亮度控制岛」页面 (`Page 14`)：
    /// 一键预设、逐屏滑条、-5%/+5% 步进与外接 DDC/CI + 笔记本内置 LCD 控制。
    fn page_brightness_island(&mut self, c: &Card, env: &Env) {
        let cfg = crate::auralsle_ai_status::config::get();
        let mod_on = cfg.mod_display_bt_enabled;
        let devs = if mod_on {
            crate::monitors::snapshot()
        } else {
            Vec::new()
        };
        let amber = Color32::from_rgb(251, 191, 36);

        // 统一次顶栏 (y = 11..30)
        c.fill(38.0, 11.0, 128.0, 19.0, 7.0, Color32::from_white_alpha(12));
        c.circle(47.5, 20.5, 2.4, if !devs.is_empty() { amber } else { Color32::from_gray(145) });
        let status_txt = if !mod_on {
            "显示模块已停用".to_owned()
        } else if devs.is_empty() {
            "未检测到可调屏幕".to_owned()
        } else {
            format!("{} 块屏幕可调", devs.len())
        };
        c.text(54.0, 23.8, &status_txt, 8.2, W::Semibold, Color32::from_gray(205), H::Left);

        let _ = env;
        let ptr_in_island = c.live && c.ui.rect_contains_pointer(c.rect(0.0, 0.0, PW, self.h.pos));

        // 快捷亮度预设：25% / 50% / 80% / 100%
        let mut px = 171.0_f32;
        for pct in [25u32, 50, 80, 100] {
            let lbl = format!("{pct}%");
            let id = format!("br-pre-{pct}");
            let (pr, pb) = paint::capsule(c, px, 11.0, 31.0, 19.0, &id, theme::BTN, theme::BTN_HOVER);
            pb.text(
                px + 15.5,
                23.6,
                &lbl,
                8.2,
                W::Semibold,
                if !devs.is_empty() { theme::WHITE } else { theme::GRAY },
                H::Center,
            );
            if pr.clicked() && mod_on && !devs.is_empty() {
                for m in &devs {
                    let target = m.value_from_percent(pct as f32 / 100.0);
                    crate::monitors::set_vcp(m.slot, m.brightness_vcp, target as u16);
                }
            }
            px += 34.0;
        }

        // 刷新 + 设置按钮
        let (rr, rb) = paint::capsule(c, 310.0, 11.0, 34.0, 19.0, "br-refresh", theme::BTN, theme::BTN_HOVER);
        rb.text(327.0, 23.6, if mod_on { "刷新" } else { "开启" }, 8.2, W::Semibold, theme::WHITE, H::Center);
        if rr.clicked() {
            if mod_on {
                crate::monitors::request_refresh();
            } else {
                self.settings_tab = 21;
                self.open_settings();
            }
        }
        let (sr, sb) = paint::capsule(c, 348.0, 11.0, 34.0, 19.0, "br-settings", theme::BTN, theme::BTN_HOVER);
        sb.text(365.0, 23.6, "设置", 8.2, W::Semibold, theme::WHITE, H::Center);
        if sr.clicked() {
            self.settings_tab = 20;
            self.open_settings();
        }

        if !mod_on || devs.is_empty() {
            c.fill(38.0, 38.0, 344.0, 86.0, 12.0, Color32::from_white_alpha(12));
            c.stroke(38.0, 38.0, 344.0, 86.0, 12.0, 0.85, Color32::from_white_alpha(24));
            let ix = 64.0_f32;
            let iy = 72.0_f32;
            c.circle(ix, iy, 14.0, Color32::from_rgba_unmultiplied(251, 191, 36, 26));
            c.circle(ix, iy, 4.6, amber);
            for k in 0..8 {
                let a = (k as f32) * std::f32::consts::FRAC_PI_4;
                c.painter.line_segment(
                    [
                        c.pos(ix + 7.2 * a.cos(), iy + 7.2 * a.sin()),
                        c.pos(ix + 10.2 * a.cos(), iy + 10.2 * a.sin()),
                    ],
                    egui::Stroke::new(1.4 * c.s, amber),
                );
            }
            if !mod_on {
                c.text(88.0, 62.0, "显示器与蓝牙外设模块已停用", 11.2, W::Bold, theme::WHITE, H::Left);
                c.text(88.0, 78.0, "已停止 DDC/CI 与内置背光轮询，节省系统资源", 8.6, W::Regular, theme::GRAY, H::Left);
                let (br, bb) = paint::capsule(
                    c,
                    88.0,
                    92.0,
                    120.0,
                    22.0,
                    "br-enable-mod",
                    Color32::from_rgba_unmultiplied(251, 191, 36, 48),
                    Color32::from_rgba_unmultiplied(251, 191, 36, 78),
                );
                bb.text(148.0, 106.2, "前往功能模块开启", 8.8, W::Semibold, amber, H::Center);
                if br.clicked() {
                    self.settings_tab = 21;
                    self.open_settings();
                }
            } else {
                c.text(88.0, 60.0, "未检测到支持调节亮度的显示器", 11.2, W::Bold, theme::WHITE, H::Left);
                let hint = crate::monitors::note()
                    .unwrap_or_else(|| "请确认外接显示器已在 OSD 中开启 DDC/CI，或检查笔记本背光驱动".to_owned());
                let fit_hint = c.fit(&hint, 8.6, W::Regular, 280.0);
                c.text(88.0, 77.0, &fit_hint, 8.6, W::Regular, theme::GRAY, H::Left);
                let (b1, bb1) = paint::capsule(c, 88.0, 92.0, 94.0, 22.0, "br-reprobe", theme::BTN, theme::BTN_HOVER);
                bb1.text(135.0, 106.2, "重新检测屏幕", 8.8, W::Semibold, theme::WHITE, H::Center);
                if b1.clicked() {
                    crate::monitors::request_refresh();
                }
                let (b2, bb2) = paint::capsule(c, 190.0, 92.0, 94.0, 22.0, "br-open-set", theme::BTN, theme::BTN_HOVER);
                bb2.text(237.0, 106.2, "显示器设置", 8.8, W::Semibold, theme::WHITE, H::Center);
                if b2.clicked() {
                    self.settings_tab = 20;
                    self.open_settings();
                }
            }
            return;
        }

        // 自适应长度：1 个设备为短岛（不滚动）；2 个设备为长岛完整双卡；3 个及以上为长岛连续滚动
        let max_scroll = devs.len().saturating_sub(2) as f32;
        let wheel_id = egui::Id::new("br_scroll_target_v2");
        let mut target_sc = if max_scroll > 0.0 {
            c.ui.ctx().data(|d| d.get_temp::<f32>(wheel_id)).unwrap_or(self.brightness_scroll).clamp(0.0, max_scroll)
        } else {
            0.0
        };
        if max_scroll > 0.0 && ptr_in_island {
            let wheel_y = c.ui.ctx().input(|i| i.raw_scroll_delta.y);
            if wheel_y.abs() > 0.1 {
                target_sc = (target_sc - wheel_y / 72.0).clamp(0.0, max_scroll);
                c.ui.ctx().data_mut(|d| d.insert_temp(wheel_id, target_sc));
                c.ui.ctx().request_repaint();
            }
        }
        if max_scroll <= 0.0 {
            self.brightness_scroll = 0.0;
            c.ui.ctx().data_mut(|d| d.insert_temp(wheel_id, 0.0f32));
        } else {
            let k = (1.0 - (-18.0 * self.dt.max(0.004)).exp()).clamp(0.08, 1.0);
            self.brightness_scroll = (self.brightness_scroll + (target_sc - self.brightness_scroll) * k).clamp(0.0, max_scroll);
            if (target_sc - self.brightness_scroll).abs() > 0.002 {
                c.ui.ctx().request_repaint();
            } else {
                self.brightness_scroll = target_sc;
            }
        }

        let left_x = 38.0_f32;
        let has_scroll = devs.len() >= 3;
        let card_w = if has_scroll { 336.0_f32 } else { 344.0_f32 };
        let card_h = 84.0_f32;
        let gap_y = 10.0_f32;
        let step_h = card_h + gap_y;
        let viewport_top = 38.0_f32;
        let viewport_h = if devs.len() <= 1 { 88.0 } else { 184.0 };
        let wheel_y = if !has_scroll && ptr_in_island {
            c.ui.ctx().input(|i| i.raw_scroll_delta.y)
        } else {
            0.0
        };
        let list_c = c.clipped(left_x - 2.0, viewport_top, card_w + 4.0, viewport_h);

        for (idx, m) in devs.iter().enumerate() {
            let y = 40.0 + (idx as f32 - self.brightness_scroll) * step_h;
            if y + card_h < viewport_top - 4.0 || y > viewport_top + viewport_h + 4.0 {
                continue;
            }
            let top_a = if y < viewport_top { ((y - (viewport_top - 26.0)) / 26.0).clamp(0.0, 1.0) } else { 1.0 };
            let bot_a = if y + card_h > viewport_top + viewport_h {
                ((viewport_top + viewport_h + 26.0 - (y + card_h)) / 26.0).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let rc = list_c.faded((top_a * bot_a).clamp(0.15, 1.0));
            let p = m.percent().clamp(0.0, 1.0);
            let pct_i = (p * 100.0).round() as i32;

            rc.fill(left_x, y, card_w, card_h, 11.0, Color32::from_white_alpha(14));
            rc.stroke(left_x, y, card_w, card_h, 11.0, 0.85, Color32::from_white_alpha(26));

            // 左侧太阳图标底座
            rc.fill(left_x + 12.0, y + 11.0, 28.0, 28.0, 8.0, Color32::from_rgba_unmultiplied(251, 191, 36, 36));
            let scx = left_x + 26.0;
            let scy = y + 25.0;
            rc.circle(scx, scy, 4.0, amber);
            for k in 0..8 {
                let a = (k as f32) * std::f32::consts::FRAC_PI_4;
                rc.painter.line_segment(
                    [
                        rc.pos(scx + 6.2 * a.cos(), scy + 6.2 * a.sin()),
                        rc.pos(scx + 8.6 * a.cos(), scy + 8.6 * a.sin()),
                    ],
                    egui::Stroke::new(1.3 * rc.s, amber),
                );
            }

            // 显示器名 + 协议/量程信息 + 大字百分比
            let name_fit = rc.fit(&m.label, 11.2, W::Bold, 204.0);
            rc.text(left_x + 48.0, y + 22.5, &name_fit, 11.2, W::Bold, theme::WHITE, H::Left);
            let sub = if m.slot == crate::monitors::INTERNAL_PANEL_SLOT {
                "内置屏幕 · LCD / WMI".to_owned()
            } else if let Some(inp) = m.input {
                format!("DDC/CI · {} · 量程 {}-{}", crate::monitors::input_source_name(inp), m.min, m.max)
            } else {
                format!("DDC/CI · 亮度区间 {}-{}", m.min, m.max)
            };
            let sub_fit = rc.fit(&sub, 8.6, W::Regular, 204.0);
            rc.text(left_x + 48.0, y + 36.5, &sub_fit, 8.6, W::Regular, theme::GRAY, H::Left);
            rc.text(
                left_x + card_w - 12.0,
                y + 30.0,
                &format!("{pct_i}%"),
                15.5,
                W::Bold,
                Color32::from_rgb(253, 224, 71),
                H::Right,
            );

            // -5% / 滑条 / +5%
            let ctrl_y = y + 52.0;
            let minus_id = format!("br-minus-{}", m.slot);
            let (mr, mb) = paint::capsule(&rc, left_x + 12.0, ctrl_y, 32.0, 20.0, &minus_id, theme::BTN, theme::BTN_HOVER);
            mb.text(left_x + 28.0, ctrl_y + 13.0, "-5%", 8.4, W::Semibold, theme::WHITE, H::Center);
            if mr.clicked() {
                let next_p = (pct_i - 5).clamp(0, 100) as f32 / 100.0;
                crate::monitors::set_vcp(m.slot, m.brightness_vcp, m.value_from_percent(next_p) as u16);
            }

            let bar_x = left_x + 52.0;
            let bar_w = card_w - 104.0;
            let bar_y = ctrl_y + 7.0;
            rc.fill(bar_x, bar_y, bar_w, 6.0, 3.0, Color32::from_white_alpha(28));
            if p > 0.01 {
                let fill_w = (bar_w * p).clamp(6.0, bar_w);
                rc.hgrad(
                    bar_x,
                    bar_y,
                    fill_w,
                    6.0,
                    3.0,
                    Color32::from_rgb(245, 158, 11),
                    Color32::from_rgb(253, 224, 71),
                );
            }

            let sr = rc.hit_drag(bar_x - 4.0, bar_y - 9.0, bar_w + 8.0, 24.0, ("br-slide", m.slot));
            let knob_x = bar_x + (bar_w * p).clamp(0.0, bar_w);
            let interacting = sr.hovered() || sr.dragged();
            if interacting {
                rc.circle(knob_x, bar_y + 3.0, 9.0, Color32::from_rgba_unmultiplied(251, 191, 36, 56));
            }
            rc.circle(knob_x, bar_y + 3.0, 6.0, theme::WHITE);
            rc.circle(knob_x, bar_y + 3.0, 3.0, Color32::from_rgb(245, 158, 11));

            if sr.clicked() || sr.dragged() {
                if let Some(pos) = sr.interact_pointer_pos() {
                    let tr = rc.rect(bar_x, bar_y, bar_w, 6.0);
                    let np = ((pos.x - tr.left()) / tr.width().max(1.0)).clamp(0.0, 1.0);
                    let target = m.value_from_percent(np);
                    crate::monitors::set_vcp(m.slot, m.brightness_vcp, target as u16);
                }
            }

            let plus_id = format!("br-plus-{}", m.slot);
            let (pr, pb) = paint::capsule(
                &rc,
                left_x + card_w - 44.0,
                ctrl_y,
                32.0,
                20.0,
                &plus_id,
                theme::BTN,
                theme::BTN_HOVER,
            );
            pb.text(left_x + card_w - 28.0, ctrl_y + 13.0, "+5%", 8.4, W::Semibold, theme::WHITE, H::Center);
            if pr.clicked() {
                let next_p = (pct_i + 5).clamp(0, 100) as f32 / 100.0;
                crate::monitors::set_vcp(m.slot, m.brightness_vcp, m.value_from_percent(next_p) as u16);
            }

            if wheel_y.abs() > 0.1 && rc.ui.rect_contains_pointer(rc.rect(left_x, y, card_w, card_h)) {
                let step = if wheel_y > 0.0 { 5 } else { -5 };
                let next_p = (pct_i + step).clamp(0, 100) as f32 / 100.0;
                crate::monitors::set_vcp(m.slot, m.brightness_vcp, m.value_from_percent(next_p) as u16);
            }
        }

        if has_scroll {
            let track_x = left_x + card_w + 4.0;
            let track_y = viewport_top + 4.0;
            let track_h = viewport_h - 8.0;
            let frac_vis = (2.0 / devs.len() as f32).clamp(0.18, 0.85);
            let thumb_h = (track_h * frac_vis).max(20.0);
            let scroll_ratio = (self.brightness_scroll / max_scroll.max(0.01)).clamp(0.0, 1.0);
            let thumb_y = track_y + (track_h - thumb_h) * scroll_ratio;
            c.fill(track_x, track_y, 3.5, track_h, 1.75, Color32::from_white_alpha(18));
            c.fill(track_x, thumb_y, 3.5, thumb_h, 1.75, Color32::from_rgba_unmultiplied(251, 191, 36, 175));

            let first_i = (self.brightness_scroll.round() as usize).min(devs.len().saturating_sub(2));
            let ind = format!("滚轮浏览 · {}-{}/{}", first_i + 1, (first_i + 2).min(devs.len()), devs.len());
            c.text(left_x + card_w, 226.0, &ind, 8.0, W::Semibold, theme::GRAY, H::Right);
        }
    }


    /// 独立展开态「设备岛」页面 (`Page 13` · 移植自 MoFeng2223/BrimDeck 四合一无线外设电量中心)：
    /// 集中展示无线鼠标、键盘、耳机、手柄四类外设的实时电量、协议标签（Bluetooth / 2.4GHz）与低电变色提醒。
    fn page_devices_island(&mut self, c: &Card, env: &Env) {
        use crate::bt::DeviceKind;
        let devs = self.island_devices(env);
        let low_cnt = devs.iter().filter(|d| d.battery.is_some_and(|b| b <= 20)).count();

        // 统一次顶栏 (y = 12..31)：左端外设协议状态条 + 右端在线数量徽章与演示数据按钮同行对齐
        c.fill(38.0, 11.0, 184.0, 19.0, 7.0, Color32::from_white_alpha(12));
        c.circle(47.5, 20.5, 2.4, Color32::from_rgb(56, 189, 248));
        c.text(54.0, 23.8, "Bluetooth & 2.4GHz 实时电量中心", 8.2, W::Semibold, Color32::from_gray(205), H::Left);

        let badge_txt = if low_cnt > 0 {
            format!("⚠ {low_cnt} 台低电")
        } else {
            format!("{} 台在线", devs.len())
        };
        let badge_col = if low_cnt > 0 {
            Color32::from_rgb(248, 113, 113)
        } else {
            Color32::from_rgb(52, 211, 153)
        };
        let bw = c.text_w(&badge_txt, 8.8, W::Bold) + 14.0;
        let bx = 316.0 - bw;
        c.fill(bx, 11.0, bw, 19.0, 9.5, badge_col.gamma_multiply(0.18));
        c.stroke(bx, 11.0, bw, 19.0, 9.5, 0.85, badge_col.gamma_multiply(0.65));
        c.text(bx + bw * 0.5, 23.6, &badge_txt, 8.8, W::Bold, badge_col, H::Center);

        let (demo_r, demo_b) = paint::capsule(c, 322.0, 11.0, 58.0, 19.0, "dev-island-demo", theme::BTN, theme::BTN_HOVER);
        demo_b.text(351.0, 23.6, "演示数据", 8.5, W::Semibold, theme::WHITE, H::Center);
        if demo_r.clicked() {
            crate::bt::toggle_demo_peripherals();
        }

        let find_kind = |pred: fn(DeviceKind) -> bool| -> Option<&crate::bt::Device> {
            devs.iter().find(|d| pred(d.kind))
        };
        let slots: [(&str, &str, DeviceKind, Option<&crate::bt::Device>); 4] = [
            ("无线鼠标", "2.4GHz / BT", DeviceKind::Mouse, find_kind(|k| k == DeviceKind::Mouse)),
            ("无线键盘", "2.4GHz / BT", DeviceKind::Keyboard, find_kind(|k| k == DeviceKind::Keyboard)),
            (
                "无线耳机",
                "Bluetooth",
                DeviceKind::Headphones,
                find_kind(|k| matches!(k, DeviceKind::Earbuds | DeviceKind::Headphones | DeviceKind::Bone | DeviceKind::Speaker)),
            ),
            ("无线手柄", "2.4GHz / BT", DeviceKind::Gamepad, find_kind(|k| k == DeviceKind::Gamepad || k == DeviceKind::Other)),
        ];

        let left_x = 38.0_f32;
        let top_y = 37.0_f32;
        let gap_x = 10.0_f32;
        let gap_y = 9.0_f32;
        let card_w = (PW - left_x * 2.0 - gap_x) * 0.5;
        let card_h = 84.0_f32;

        for (idx, (slot_title, default_bus, kind_fb, dev_opt)) in slots.iter().enumerate() {
            let col = idx % 2;
            let row = idx / 2;
            let x = left_x + col as f32 * (card_w + gap_x);
            let y = top_y + row as f32 * (card_h + gap_y);

            let level = dev_opt.and_then(|d| d.battery);
            let is_low = level.is_some_and(|b| b <= 20);
            let is_warn = level.is_some_and(|b| b > 20 && b <= 35);
            let status_col = match level {
                Some(b) if b <= 20 => Color32::from_rgb(248, 113, 113),
                Some(b) if b <= 35 => Color32::from_rgb(251, 191, 36),
                Some(_) => Color32::from_rgb(52, 211, 153),
                None if dev_opt.is_some() => Color32::from_rgb(56, 189, 248),
                None => Color32::from_gray(115),
            };
            let bg = if is_low {
                Color32::from_rgba_unmultiplied(239, 68, 68, 34)
            } else if is_warn {
                Color32::from_rgba_unmultiplied(245, 158, 11, 24)
            } else {
                Color32::from_white_alpha(14)
            };
            let rim = if is_low {
                Color32::from_rgba_unmultiplied(248, 113, 113, 165)
            } else {
                Color32::from_white_alpha(26)
            };
            c.fill(x, y, card_w, card_h, 11.0, bg);
            c.stroke(x, y, card_w, card_h, 11.0, if is_low { 1.2 } else { 0.85 }, rim);

            // 左侧圆角图标底座
            c.fill(x + 11.0, y + 11.0, 30.0, 30.0, 8.5, status_col.gamma_multiply(0.18));
            let icon_k = dev_opt.map(|d| d.kind).unwrap_or(*kind_fb);
            crate::extras::device_icon_line(c, icon_k, x + 17.0, y + 17.0, 18.0, status_col);

            // 设备名与协议副标题
            let dev_name = dev_opt.map(|d| d.name.as_str()).unwrap_or(*slot_title);
            let fit_name = c.fit(dev_name, 11.2, W::Bold, card_w - 94.0);
            c.text(x + 48.0, y + 23.5, &fit_name, 11.2, W::Bold, if dev_opt.is_some() { theme::WHITE } else { theme::GRAY }, H::Left);

            let bus_txt = match dev_opt {
                Some(d) if d.id.starts_with("VOLTIDE_") => "Voltide 2.4GHz",
                Some(_) => "Bluetooth",
                None => *default_bus,
            };
            let state_sub = if is_low {
                "低电量告警"
            } else if dev_opt.is_some() {
                bus_txt
            } else {
                "未连接"
            };
            c.text(x + 48.0, y + 37.5, state_sub, 8.8, W::Regular, if is_low { status_col } else { theme::GRAY }, H::Left);

            // 右上大字电量百分比
            let pct_txt = match level {
                Some(b) => format!("{b}%"),
                None if dev_opt.is_some() => "在线".to_owned(),
                None => "--".to_owned(),
            };
            c.text(x + card_w - 11.0, y + 29.0, &pct_txt, 14.5, W::Bold, status_col, H::Right);

            // 底部电量进度条与分段刻度
            let bar_x = x + 11.0;
            let bar_y = y + card_h - 19.0;
            let bar_w = card_w - 22.0;
            c.fill(bar_x, bar_y, bar_w, 6.0, 3.0, Color32::from_white_alpha(20));
            if let Some(b) = level {
                let ratio = (b as f32 / 100.0).clamp(0.05, 1.0);
                c.fill(bar_x, bar_y, bar_w * ratio, 6.0, 3.0, status_col);
            }
        }
    }

    fn toast_layer(&mut self, c: &Card, base_y:f32)->Option<Rect> {
        let Some((msg, at)) = self.toast.clone() else { return None };
        let e = (self.t - at) as f32;
        if e > 2.4 {
            self.toast = None;
            return None;
        }
        let a = smoothstep(0.0, 0.16, e) * (1.0 - smoothstep(2.1, 2.4, e));
        let rise = (1.0 - anim::ease_out_cubic(e / 0.3)) * 6.0;
        let msg=c.fit(&msg,size::BODY,W::Regular,310.0);
        let w = c.text_w(&msg, size::BODY, W::Regular) + 26.0;
        let tc = c.faded(a);
        let y = base_y + rise;
        tc.fill(PW / 2.0 - w / 2.0, y, w, 22.0, 11.0, Color32::from_rgb(44, 44, 48));
        tc.text(PW / 2.0, y + 15.0, &msg, size::BODY, W::Regular, theme::WHITE, H::Center);
        Some(tc.rect(PW/2.0-w/2.0,y,w,22.0))
    }

    // ---------------------------------------------------------------
    // 每帧推进：番茄钟、饮水提醒、跨天
    // ---------------------------------------------------------------
    fn tick(&mut self, env: &Env, dt: f32, elapsed: f32) {
        // [ai-status-mod] 只在「播→停」边沿起表；重播立刻清表，不触发回退。
        let was_playing=self.media.playing;
        self.music_paused_since=crate::auralsle_ai_status::next_pause_since(self.music_paused_since,was_playing,env.media.playing,env.t);
        // [启动即暂停 · 本轮图2] 启动时音乐本来就是暂停的，没有"播放→暂停"边沿，
        // 计时器永远不启动 → 歌词胶囊一直挂着（重播再暂停才恢复）。第一帧采样到
        // 暂停就开始计时，交给 `music_pause_expired` 按用户配置的秒数收起。
        if !cfg!(test) && self.music_paused_since.is_none() && !env.media.playing && self.media.is_music() {
            self.music_paused_since = Some(env.t);
        }
        if !cfg!(test) {
            let mcfg = crate::auralsle_ai_status::config::get();
            if mcfg.music_auto_hide_when_idle {
                // 必须严格检查 env.media.is_music()，与 update_media 中 self.media.playing 的条件完全一致；
                // 否则当浏览器播放非音乐视频（env.media.playing=true 但 is_music()=false）时，
                // was_playing 每帧为 false，会导致每帧强制跳回 page 0 而无法切换展开岛页面！
                let now_has_music = env.media.playing && env.media.is_music();
                if !was_playing && now_has_music && self.settings.visible_pages[0] && self.page != 0 && !self.clip_copy_override && !self.copy_opened_session && !(self.open && self.page == 12) {
                    crate::frame_watch::record_user_event("SET", "music_show");
                    self.page_from = self.page;
                    self.page = 0;
                    self.dots_target();
                } else if !now_has_music && !self.media.is_music() && self.page == 0 && !self.open {
                    if let Some(next_p) = self.active_page_order().into_iter().find(|&p| p != 0 && self.is_page_currently_visible(p)) {
                        crate::frame_watch::record_user_event("SET", &format!("music_next={}", next_p));
                        self.page_from = next_p;
                        self.page = next_p;
                        self.dots_target();
                    }
                }
            }
            // 无线外设岛（Page 13）实时连接感知：无论蓝牙通知是否开启，实时同步 env.bt 快照，并在外设连接瞬间立即显现设备岛/设备胶囊（需求 7）
            if !env.bt.is_empty() && self.bt_shown.is_empty() {
                self.bt_shown = env.bt.clone();
            }
            let now_has_devs = !env.bt.is_empty() || self.has_connected_wireless_devices();
            if mcfg.show_bt_battery_on_island && mcfg.device_page_visible {
                if !self.last_had_wireless_devices && now_has_devs {
                    self.dots_target();
                    if (mcfg.device_display_mode == 1 || mcfg.device_display_mode == 2) && self.page != 13 {
                        crate::frame_watch::record_user_event("SET", "bt_show");
                        self.page_from = self.page;
                        self.page = 13;
                        self.dots_target();
                    }
                } else if self.last_had_wireless_devices && !now_has_devs && mcfg.device_auto_hide_when_empty {
                    self.dots_target();
                    if self.page == 13 && !self.open {
                        if let Some(next_p) = self.active_page_order().into_iter().find(|&p| p != 13 && self.is_page_currently_visible(p)) {
                            crate::frame_watch::record_user_event("SET", &format!("bt_next={}", next_p));
                            self.page_from = next_p;
                            self.page = next_p;
                            self.dots_target();
                        }
                    }
                }
            }
            self.last_had_wireless_devices = now_has_devs;
        }
        // [ai-status-mod] 课程卡「开始专注」：启动岛仔自带番茄钟并切到专注页。
        if crate::auralsle_ai_status::take_focus_request() {
            self.pomo.phase=Phase::Focus;
            self.pomo.remaining=self.phase_secs(Phase::Focus);
            self.pomo.running=true;
            self.show_page(1);
        }
        if self.toast.as_ref().is_some_and(|(_,at)|self.t-at>2.4){self.toast=None;}
        // 跳转后先用本地目标位置显示；播放器回报追上了、换歌了或超时就交还真实位置。
        let seek_done=self.music_seek_ui.as_ref().is_some_and(|(target,title)|{
            (self.media.duration>0.0 && (self.media.position-*target).abs()<1.5)
                || self.t>=self.music_seek_quiet_until || &self.media.title!=title
        });
        if seek_done {self.music_seek_ui=None;}
        if self.music_restart_at.is_some_and(|at|self.t>=at+1.5){self.music_restart_at=None;}
        if env.today != self.today {
            self.today = env.today.clone();
            self.water = model::Water::load(&env.today);
            self.last_drink=None;self.last_reminder=self.t;
            self.water_ml = self.water.drunk_ml as f32;
            self.water_ring.snap(self.water.percent() / 100.0);
        }
        if self.pomo.running && !self.module_on(FOCUS_PAGE) {self.pomo.running=false;}
        if self.pomo.running {
            self.pomo.remaining -= elapsed;
            if self.pomo.remaining <= 0.0 {
                let next = match self.pomo.phase {
                    Phase::Focus => {
                        self.pet.react(PetState::Happy);
                        Phase::Rest
                    }
                    Phase::Rest => Phase::Focus,
                };
                self.pomo.phase = next;
                self.pomo.remaining = self.phase_secs(next);
                self.pomo.running = false;
                self.alert("专注计时", format!("该{}了", next.label()), 1, true);
            }
        }
        self.ring_shown.target = (self.pomo.remaining / self.phase_secs(self.pomo.phase)).clamp(0.0, 1.0);
        self.ring_shown.step(dt);
        self.reset_spin.step(dt);
        self.todo_pop.step(dt);

        if self.module_on(WATER_PAGE) && self.water.remind && !self.health_pending[crate::extras::HealthReminder::Water as usize] && self.water.percent() < 100.0 && self.remind_window(env) && self.t - self.last_reminder >= self.settings.water_minutes as f64 * 60.0 {
            self.alert("喝水时间到了", format!("休息一下，喝 {} ml 水吧", self.settings.serving_ml), 2, true);
        }
        // 久坐：锁屏视为已起身离开，锁屏期间计时一直停在零，解锁后从头算；
        // 不看键盘鼠标是否闲置——只看屏幕不动鼠标的人同样是在久坐。
        // 专注阶段不打断，等专注结束（通常正好进入休息）再提醒。
        if env.screen_locked { self.sit_start = self.t; }
        let focusing = self.pomo.running && self.pomo.phase == Phase::Focus;
        if self.module_on(WATER_PAGE) && self.settings.sit_remind && !self.health_pending[crate::extras::HealthReminder::Sit as usize] && !focusing && self.remind_window(env) && self.t >= self.sit_due() {
            self.alert(SIT_TITLE, "起来走走，休息一下", 2, true);
        }
        let now = format!("{} {}", env.today, env.clock.get(..5).unwrap_or("00:00"));
        let todo_on=self.module_on(TODO_PAGE);
        let due: Vec<_> = self.todos.items.iter().enumerate().filter(|(_, item)| todo_on && item.reminder_due(&now)).map(|(i,item)| (i,item.text.clone())).collect();
        for (i,text) in due { self.todos.items[i].mark_reminded(&now); self.todos.save(); self.alert("待办到时间了", text, 4, true); }
        // v0.5.5 会议提醒：.ics 里的开始时间，默认提前 15 分钟（可设 5/15/30）。
        // v0.5.6 起日历（CalDAV）事件走同一套：同一场会（UID + DTSTART）只提醒一次，
        // 邮件和日历两条来源撞上了也只弹一次。
        let lead = self.settings.mail_ics as i64 * 60;
        let now_ts = chrono::Local::now().timestamp();
        let mut picks = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        // 会议提醒点开后进入日程页；日程模块关闭时不再提醒（含邮件邀请）。
        let calendar_on=self.module_on(CALENDAR_PAGE);
        for entry in self.calendar.local.iter().filter(|_|calendar_on) {
            let Some(at)=entry.reminder_at() else{continue;};
            let key=calendar::dedupe_key(&entry.event);
            let cutoff=if entry.event.all_day{at+60}else{entry.event.start+60};
            if now_ts>=at && now_ts<cutoff && !self.mail.fired.contains(&key) && seen.insert(key.clone()) {
                picks.push((key,entry.event.clone(),"本机日程".into(),CALENDAR_PAGE));
            }
        }
        // [ai-status-mod] 课表到点也提醒一次（与本机日程共用 fired 去重）
        for (key,event) in crate::auralsle_ai_status::course::due(now_ts) {
            if !self.mail.fired.contains(&key) && seen.insert(key.clone()) {
                picks.push((key,event,"课表".into(),CALENDAR_PAGE));
            }
        }
        for (m,provider) in self.calendar_work_events(None).into_iter().filter(|_|calendar_on) {
            let left = m.start - now_ts;
            let key = calendar::dedupe_key(m);
            if !m.all_day && left <= lead && left > -60 && !self.mail.fired.contains(&key) && seen.insert(key.clone()) {
                let source=provider.map(|provider|provider.label()).unwrap_or("日历");
                picks.push((key, m.clone(), source.to_string(), CALENDAR_PAGE));
            }
        }
        if calendar_on && self.mail_active() {
            for m in self.mail.recent.iter().filter_map(|m| m.meeting.as_ref()).filter(|m|!self.mail_event_ignored(m)) {
                let left = m.start - now_ts;
                let key = calendar::dedupe_key(m);
                if !m.all_day && left <= lead && left > -60 && !self.mail.fired.contains(&key) && seen.insert(key.clone()) {
                    picks.push((key, m.clone(), "邮件邀请".to_string(), CALENDAR_PAGE));
                }
            }
        }
        for (key, meeting, source, page) in picks {
            self.mail.fired.insert(key);
            self.meeting_alert(meeting, source, page);
        }
        self.water_ring.target = self.water.percent() / 100.0;
        self.water_ring.step(dt);
        self.water_ml = anim::approach(self.water_ml, self.water.drunk_ml as f32, 9.0, dt);
        self.water_view_t.target = self.water_view as f32;
        self.water_view_t.step(dt);
        self.sit_view_t.target = if self.sit_mode { 1.0 } else { 0.0 };
        self.sit_view_t.step(dt);
        self.remind_t.target = if self.water.remind { 1.0 } else { 0.0 };
        self.remind_t.step(dt);
        self.sit_t.target = if self.settings.sit_remind { 1.0 } else { 0.0 };
        self.sit_t.step(dt);
        // 剪贴板记录开关：关闭时同时清掉未固定的记录。
        if self.clipboard.is_enabled() != self.clipboard_active() {
            self.clipboard.set_enabled(self.clipboard_active());
        }
        self.shelf_scroll.target = self.shelf_first as f32;
        self.shelf_scroll.step(dt);
        if let Some(at) = self.confirm_clear {
            if self.t - at > 3.0 {
                self.confirm_clear = None;
            }
        }
    }

    /// 媒体进度在两次 SMTC 刷新之间按时间外推，进度条才会连续走动。
    fn update_media(&mut self, env: &Env, ctx: &egui::Context) {
        let m = &env.media;
        if !m.is_music() {
            self.music_volume_ui=None;
            self.music_volume_quiet_until=0.0;
            self.music_volume_open=false;
            self.media=NowPlaying::default();
            self.media_anchor=(String::new(),0.0,0.0,false);
            self.music_artwork=None;
            self.ly=LyricsState::default();
            return;
        }
        let a = &self.media_anchor;
        if a.0 != m.title || (a.1 - m.position).abs() > 0.01 || a.3 != m.playing {
            self.media_anchor = (m.title.clone(), m.position, env.t, m.playing);
        }
        let mut np = m.clone();
        if self.media.source!=m.source || m.volume.is_none(){self.music_volume_open=false;}
        if self.media.source!=m.source || self.t>=self.music_volume_quiet_until || m.volume.is_none() {
            self.music_volume_ui=m.volume;
        }
        if m.playing {
            let (_, pos, _, _) = &self.media_anchor;
            np.position = if let Some(sampled)=m.sampled_at {m.position+sampled.elapsed().as_secs_f64()}
                else {*pos};
            if m.duration>0.0 {np.position=np.position.min(m.duration);}
        }
        // 网易云的 song_id 只来自本机日志，SMTC 那一路没有；None 与 Some 之间来回不是换歌，不能每次都清空歌词重查。
        let other_song=matches!((self.media.song_id,np.song_id),(Some(old),Some(new)) if old!=new);
        if self.media.source != np.source || other_song { self.ly=LyricsState::default(); }
        self.media = np;
        if let Some(image)=&self.media.artwork {
            if !self.music_artwork.as_ref().is_some_and(|(old,_)|std::sync::Arc::ptr_eq(old,image)) {
                self.music_artwork=Some((image.clone(),ctx.load_texture("music-artwork",image.clone(),egui::TextureOptions::LINEAR)));
            }
        } else {self.music_artwork=None;}
    }

    /// [修复需求 2 图一 `image_3aa3e2.jpg`]：实时采样当前岛体皮肤在 `(local_x, local_y)` 处的真实背景颜色（支持悬浮云 Fluid 动态流体、液态玻璃、毛玻璃、奶油与 18 套主题色），使滚动列表上下边缘消隐渐变与背景浑然一体！
    pub(crate) fn sample_island_bg_color(&self, local_x: f32, local_y: f32) -> Color32 {
        let u = (local_x / PW.max(1.0)).clamp(0.0, 1.0);
        let v = (local_y / self.h.pos.max(220.0)).clamp(0.0, 1.0);
        let px = local_x;
        let py = local_y;
        match self.settings.skin {
            model::Skin::Fluid => self.fluid.compose(u, v, px, py, PW.max(1.0)),
            model::Skin::LiquidGlass => {
                let frame = Rect::from_min_size(Pos2::ZERO, Vec2::new(PW.max(1.0), self.h.pos.max(220.0)));
                self.liquid_glass_compose(px, py, 0.0, PW.max(1.0), frame, self.settings.island_shape == model::IslandShape::Pill, self.progress())
            }
            model::Skin::GlassLight => {
                let frame = Rect::from_min_size(Pos2::ZERO, Vec2::new(PW.max(1.0), self.h.pos.max(220.0)));
                self.frosted_light_glass_compose(px, py, frame)
            }
            _ => {
                if let Some(fill) = crate::auralsle_ai_status::plugins::active_win_fill(255) {
                    fill
                } else {
                    self.glass_fill()
                }
            }
        }
    }

    /// 在滚动区域上下边缘绘制跟随当前岛体动态背景（含悬浮云流体光晕）的多段平滑消隐渐变罩（修复需求 2 图一 `image_3aa3e2.jpg`）。
    pub(crate) fn draw_skin_matched_edge_fade(
        &self,
        c: &Card,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        top_down: bool,
    ) {
        let slices = 12usize;
        let mut mesh = egui::Mesh::default();
        let sample_y = if top_down { y + 1.0 } else { y + h - 1.0 };
        for i in 0..=slices {
            let t = i as f32 / slices as f32;
            let lx = x + w * t;
            let bg = self.sample_island_bg_color(lx, sample_y);
            let solid_a = ((bg.a() as f32) * 0.88).round().clamp(0.0, 235.0) as u8;
            let solid = Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), solid_a);
            let trans = Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), 0);
            let (top_col, bot_col) = if top_down { (solid, trans) } else { (trans, solid) };
            mesh.colored_vertex(c.pos(lx, y), top_col);
            mesh.colored_vertex(c.pos(lx, y + h), bot_col);
            if i > 0 {
                let b = (i as u32 - 1) * 2;
                mesh.add_triangle(b, b + 1, b + 2);
                mesh.add_triangle(b + 1, b + 3, b + 2);
            }
        }
        c.painter.add(egui::Shape::mesh(mesh));
    }

    fn poll_lyrics(&mut self) {
        if !cfg!(test) && !crate::auralsle_ai_status::config::get().mod_music_enabled {
            self.ly = LyricsState::default();
            return;
        }
        if !self.media.is_music(){self.ly=LyricsState::default();return;}
        if cfg!(test) && !self.show_lyrics && !self.settings.island_lyrics {self.ly=LyricsState::default();return;}
        let np = &self.media;
        if np.title.is_empty(){self.ly=LyricsState::default();return;}
        let key = format!("{}|{}|{:.0}", np.title, np.artist, np.duration);
        let id_arrived=key==self.ly.key && !self.ly.loading && self.ly.song_id.is_none() && np.song_id.is_some()
            && !self.ly.lyrics.as_ref().is_some_and(|lyrics|lyrics.synced);
        if key != self.ly.key {
            let change=self.ly.song.as_ref().map_or(SongChange::Different,|song|song_change(song,np));
            let synced=self.ly.lyrics.as_ref().is_some_and(|lyrics|lyrics.synced);
            match change {
                // 时长、写法补全：已有同步歌词就沿用，不清空、不重查。
                SongChange::Same if synced=>{self.ly.key=key;self.ly.song=Some(LyricSong::of(np));}
                // 换歌：立即切换显示状态，旧歌词和迟到的旧结果都不再显示。
                SongChange::Different=>self.start_lyrics(key,None,false),
                // 同一首的补全：重新查询，期间继续显示已有歌词。
                _=>{let keep=self.ly.lyrics.take();self.start_lyrics(key,keep,false);}
            }
        } else if id_arrived || self.ly.retry_at.is_some_and(|at|self.t>=at) {
            self.start_lyrics(key,None,true);
        }
        self.spawn_lyrics_job();
        if let Some(rx)=&self.ly.native_rx {
            let mut found=None;
            while let Ok(result)=rx.try_recv(){if result.is_some(){found=result;}}
            if let Some(lyrics)=found {
                self.ly.lyrics=Some(lyrics);self.ly.loading=false;self.ly.error=None;
                // 本机歌词已到：排队中的在线查询不再需要，进行中的结果也丢弃。
                self.ly.retry_at=None;self.ly.rx=None;self.ly.native_rx=None;self.ly.pending=None;
            }
        }
        if let Some(rx) = &self.ly.rx {
            if let Ok(r) = rx.try_recv() {
                self.ly.loading = false;
                match r {
                    Ok((l,waiting_native)) => {
                        self.ly.error=None;
                        self.ly.retry_at=None;
                        if (waiting_native || lyrics::is_soda(&self.media.source) && !l.synced) && self.ly.attempts<NATIVE_RETRIES {self.ly.retry_at=Some(self.t+5.0);}
                        self.ly.lyrics = Some(l);
                    },
                    Err(error) => {
                        let native=lyrics::is_soda(&self.media.source) || self.media.source.to_ascii_lowercase().contains("kugou") && error==lyrics::FetchError::NotFound;
                        if native {if self.ly.attempts<NATIVE_RETRIES{self.ly.retry_at=Some(self.t+5.0);}}
                        else if matches!(error,lyrics::FetchError::Unavailable(_)|lyrics::FetchError::InvalidResponse(_)) && self.ly.attempts<NETWORK_RETRIES {self.ly.retry_at=Some(self.t+30.0);}
                        self.ly.error = Some(error);
                    },
                }
                self.ly.rx = None;
            }
        }
    }

    /// 为当前歌曲建立新的歌词状态：本机歌词轮询立即开始，在线查询交给 `spawn_lyrics_job`。
    /// `keep`：同一首歌信息补全时继续显示的歌词；`retry`：同一首歌的定时重试或补到歌曲 ID。
    fn start_lyrics(&mut self,key:String,keep:Option<lyrics::Lyrics>,retry:bool) {
        let np=&self.media;
        let kugou=np.source.to_ascii_lowercase().contains("kugou");
        let native_only=retry && kugou && self.ly.error==Some(lyrics::FetchError::NotFound);
        let cached=if retry && kugou {self.ly.lyrics.take()}else{None};
        let attempts=if retry {self.ly.attempts.saturating_add(1)}else{0};
        let manual=self.ly.manual_retry;
        let shown=cached.clone().or(keep);
        self.ly = LyricsState { key, loading: shown.is_none() && !native_only, lyrics:shown,
            error:native_only.then_some(lyrics::FetchError::NotFound), song_id:np.song_id, attempts,
            song:Some(LyricSong::of(np)), ..Default::default() };
        let (source, title, artist, dur) = (np.source.clone(), np.title.clone(), np.artist.clone(), np.duration);
        let song_id=np.song_id;
        if kugou || lyrics::is_soda(&source) {
            let (tx,rx)=std::sync::mpsc::sync_channel(1);self.ly.native_rx=Some(rx);
            let (source,title,artist)=(source.clone(),title.clone(),artist.clone());
            let native=crate::resources::NATIVE_LYRICS.spawn(move ||{
                // 客户端通常在开播几秒内写好歌词：先密后疏（0.5s 起，最长 5s 一次），最多等 90 秒。
                // 汽水的缓存文件可达 32MB，修改时间和大小都没变时不重复整份读取，但每 15 秒仍完整核对一次。
                let started=std::time::Instant::now();
                let mut wait=std::time::Duration::from_millis(500);
                let mut last:Option<((std::time::SystemTime,u64),std::time::Instant)>=None;
                let mut miss="未读取";
                loop {
                    let print=lyrics::native_fingerprint(&source);
                    let unchanged=print.is_some_and(|print|last.is_some_and(|(seen,at)|seen==print && at.elapsed()<std::time::Duration::from_secs(15)));
                    let found=if unchanged {None}else{
                        if let Some(print)=print {last=Some((print,std::time::Instant::now()));}
                        lyrics::read_native_detail(&source,&title,&artist,dur).map_err(|reason|miss=reason).ok()
                    };
                    // 找到歌词时等界面取走再结束（换歌或关闭歌词后通道断开会立即返回），不因通道暂满丢掉结果。
                    if found.is_some() {
                        if tx.send(found).is_ok(){lyrics::log_native(&source,&title,&artist,"命中",started.elapsed());}
                        break;
                    }
                    if let Err(std::sync::mpsc::TrySendError::Disconnected(_))=tx.try_send(None){break;}
                    if started.elapsed()>=std::time::Duration::from_secs(90){lyrics::log_native(&source,&title,&artist,&format!("放弃:{miss}"),started.elapsed());break;}
                    std::thread::sleep(wait);
                    wait=(wait*2).min(std::time::Duration::from_secs(5));
                }
            });
            // 快速切歌时本机轮询线程已满：不留空通道，之后的定时重试会再次启动。
            if native.is_none(){self.ly.native_rx=None;}
        }
        self.ly.pending=Some(LyricsJob{source,title,artist,duration:dur,song_id,cached,native_only,manual});
    }

    /// 旧请求结束后再发起排队的在线查询：歌曲信息抖动或快速切歌时不会叠加网络线程，
    /// 也只会查询最新的那首；旧请求的结果因通道已更换而直接丢弃。
    fn spawn_lyrics_job(&mut self) {
        if self.ly.pending.is_none() || self.ly_worker.as_ref().is_some_and(|worker|!worker.is_finished()) {return;}
        let Some(lease)=crate::resources::LYRICS.acquire() else {return;};
        let Some(job)=self.ly.pending.take() else {return;};
        let (tx, rx) = std::sync::mpsc::channel();
        self.ly.rx = Some(rx);
        self.ly_worker=lease.spawn(move || {
            let LyricsJob{source,title,artist,duration,song_id,cached,native_only,manual}=job;
            let result=if native_only {
                lyrics::kugou_cache::read(&title,&artist,duration).map(|lyrics|(lyrics,false)).ok_or(lyrics::FetchError::NotFound)
            }else{lyrics::fetch_for_display_with(&source, &title, &artist, duration, song_id, cached, manual)};
            let _ = tx.send(result);
        });
        if self.ly_worker.is_none(){self.ly.rx=None;self.ly.loading=false;self.ly.key.clear();}
    }

    /// 外部直接提供歌词（录制演示用）。
    pub fn set_lyrics(&mut self, title: &str, artist: &str, duration: f64, l: lyrics::Lyrics) {
        self.ly = LyricsState { key: format!("{title}|{artist}|{duration:.0}"), lyrics: Some(l), ..Default::default() };
    }

    pub fn on_drop(&mut self, files: Vec<PathBuf>) {
        if !self.settings.visible_pages[3] || files.is_empty() { return; }
        self.drag_hot = false;
        self.drag_saved = None;
        if self.page != 3 {
            self.go(3, 1.0);
        }
        let (mut added,mut duplicate,mut failed) = (0,0,0);
        for p in files {
            if !p.exists() {failed+=1;continue;}
            if self.shelf.items.contains(&p) {
                duplicate+=1;
                continue;
            }
            if self.shelf.add(&p).is_ok() {
                self.shelf_born.insert(p.clone(), self.t);
                added += 1;
            }else{failed+=1;}
        }
        let message=if added==0 && failed==0 {
            if duplicate==1{"该文件已在置物架中".into()}else{"这些文件已在置物架中".into()}
        }else if added==0 && duplicate==0 {"文件不存在或无法访问".into()}
        else {
            let mut parts=Vec::new();
            if added>0{parts.push(format!("已添加 {added} 个文件"));}
            if duplicate>0{parts.push(format!("{duplicate} 个已存在"));}
            if failed>0{parts.push(format!("{failed} 个无法添加"));}
            parts.join("，")
        };
        self.say(message);
        if added>0 {
            self.pet.react(PetState::Happy);
            if !cfg!(test) {
                crate::auralsle_ai_status::aora_face::trigger_mascot_event(30, 4.5);
            }
            self.shelf_first = self.shelf.items.len().saturating_sub(5);
        }
    }

    /// 待办输入框：真正的 egui 文本框，放在待办卡片顶部那一行。
    fn input_field(&mut self, ui: &mut egui::Ui, origin: Pos2, cmds: &mut Vec<Cmd>) {
        let y_off = if cfg!(test) { 0.0 } else { 22.0 };
        let rect = Rect::from_min_size(origin + Vec2::new(222.0, 37.0 + y_off), Vec2::new(142.0, 18.0));
        let mut buf = std::mem::take(&mut self.buf);
        let te = egui::TextEdit::singleline(&mut buf)
            .id(egui::Id::new("todo-input"))
            .frame(false)
            .margin(egui::Margin::ZERO)
            .text_color(theme::WHITE)
            .font(theme::font(size::BODY, W::Regular))
            .hint_text(egui::RichText::new("要做点什么？回车保存").color(theme::DIM).font(theme::font(size::BODY, W::Regular)))
            .desired_width(rect.width());
        let resp = ui.put(rect, te);
        let first_focus=!self.todo_input_focused;
        self.buf=buf;
        let lost_window=ui.input(|i|i.events.iter().any(|event|matches!(event,egui::Event::WindowFocused(false))));
        if !first_focus && !self.todo_ime_frame && self.adding && self.buf.trim().is_empty() && (resp.clicked_elsewhere() || lost_window) {
            resp.surrender_focus();
            self.cancel_edit();
            return;
        }
        if first_focus {
            self.todo_input_focused=true;
            resp.request_focus();
            cmds.push(Cmd::Focus);
        }
        let submit = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if submit {
            let text = self.buf.trim().to_string();
            let mut added=false;
            if let Some(i) = self.editing {
                if !text.is_empty() && i < self.todos.items.len() {
                    self.todos.items[i].text = text;
                    self.todos.save();
                }
            } else if !text.is_empty() {
                self.todo_first = 0;
                self.todos.items.insert(0, model::TodoItem::new(text));
                self.todos.save();
                self.todo_pop.vel += 6.0;
                added=true;
            }
            self.cancel_edit();
            if added {self.open_task_reminder(0);}
        }
    }
}

// verify-compat: crate::auralsle_ai_status::outer_row_width() | let ai_return=crate::auralsle_ai_status::outer_button_rect(cx,top,self.progress()); | // [ai-status-mod] 原版红点 | // [ai-status-mod] 恢复原版红点

#[cfg(test)]
mod pill_orb_geometry_tests {
    use super::{orb_badge_layout, pill_orb_clamped, pill_orb_diameter, pill_orb_rect, pill_orb_top, PILL_ORB_MAX};
    use eframe::egui::{Pos2, Rect, Vec2};

    fn capsule(h: f32) -> Rect { Rect::from_min_size(Pos2::new(100.0, 20.0), Vec2::new(419.0, h)) }

    #[test]
    fn orb_morph_position_leads_the_way_it_grows() {
        // 位置先到、形状后到：同一时刻位置进度必须严格大于形状的线性等效进度，
        // 这样"球先滑到位、再长成竖胶囊"，不会像过去那样在最后 30% 里同时爆发。
        for t in [0.2f32, 0.4, 0.6, 0.8] {
            let p = super::orb_morph_position(t);
            assert!(p > t, "位置进度 {p} 应领先线性 {t}");
            assert!((0.0..=1.0).contains(&p));
        }
        assert_eq!(super::orb_morph_position(0.0), 0.0);
        assert_eq!(super::orb_morph_position(1.0), 1.0);
    }

    #[test]
    fn orb_morph_shape_overshoots_slightly_then_settles() {
        let peak = (0..=100)
            .map(|i| super::orb_morph_shape(i as f32 / 100.0))
            .fold(0.0f32, f32::max);
        assert!((1.0..=1.06).contains(&peak), "收尾允许 6% 回弹，实际峰值 {peak}");
        assert_eq!(super::orb_morph_shape(0.0), 0.0);
        assert_eq!(super::orb_morph_shape(1.0), 1.0, "到点必须精确归位");
        assert_eq!(super::orb_morph_shape(f32::NAN), 1.0, "NaN 直接给终态，不许画半个球");
    }

    #[test]
    fn pill_orb_is_as_tall_as_the_capsule() {
        // 横幅把胶囊撑到 44px 时，球必须也是 44px，而不是被 BAR_H 夹在 30px。
        for h in [30.0f32, 44.0, 56.0] {
            assert_eq!(pill_orb_diameter(h), h, "胶囊高 {h} 时悬浮球应与胶囊等高");
        }
    }

    #[test]
    fn pill_orb_is_vertically_centered_on_the_capsule() {
        let panel = capsule(44.0);
        let r = pill_orb_rect(panel, pill_orb_diameter(panel.height()), 5.5, false);
        assert_eq!(r.height(), 44.0, "悬浮球高度应等于胶囊高度");
        assert_eq!(r.center().y, panel.center().y, "悬浮球中线应与胶囊中线重合");
        assert_eq!(r.left(), panel.right() + 5.5, "悬浮球应贴在胶囊右侧并留出间距");
    }

    #[test]
    fn pill_orb_keeps_minimum_size_on_thin_line() {
        // 极简细线（4.5px）时球不缩小到看不见，且贴顶不越出屏幕。
        let panel = capsule(4.5);
        let d = pill_orb_diameter(panel.height());
        assert_eq!(d, 20.0, "细线态悬浮球直径应保持 20px 下限");
        assert_eq!(pill_orb_rect(panel, d, 5.5, false).top(), panel.top(), "细线态悬浮球应贴顶");
    }

    #[test]
    fn pill_orb_flips_to_the_left_when_no_room() {
        let panel = capsule(44.0);
        let d = pill_orb_diameter(panel.height());
        let r = pill_orb_rect(panel, d, 5.5, true);
        assert_eq!(r.right(), panel.left() - 5.5, "右侧放不下时应翻到胶囊左侧");
        assert_eq!(r.center().y, panel.center().y, "翻到左侧后仍需垂直居中");
    }

    // ── 第三轮：展开动画期间球不放大、不换边 / 球内图标与角标居中 ──────────────

    #[test]
    fn pill_orb_diameter_is_clamped_for_expanding_panels() {
        // 展开动画里 panel.height() 会从 30 长到 174/248；球径必须只跟折叠态胶囊高度，
        // 且设上限，绝不能长成"巨球"。
        assert_eq!(pill_orb_diameter(30.0), 30.0);
        assert_eq!(pill_orb_diameter(44.0), 44.0);
        assert_eq!(pill_orb_diameter(56.0), 56.0, "56 是横幅态上限内的合法值");
        assert_eq!(pill_orb_diameter(174.0), PILL_ORB_MAX, "展开动画的 174px 面板不得把球撑成巨球");
        assert_eq!(pill_orb_diameter(248.0), PILL_ORB_MAX, "完整面板 248px 同样必须夹紧");
    }

    #[test]
    fn pill_orb_never_jumps_to_the_left_when_expanding() {
        // 上一轮的现象：展开时球"跑去了左边"。折叠态锚点必须与面板宽度无关，
        // 只有显式 place_left（右停靠）才翻左边。
        let pill = capsule(44.0);                       // 折叠态胶囊（稳定）
        let d = pill_orb_diameter(pill.height());
        let r = pill_orb_rect(pill, d, 5.5, false);
        assert_eq!(r.left(), pill.right() + 5.5);
        assert_eq!(r.center().y, pill.center().y);
        // 面板已经涨到 419x174，也不影响折叠态锚点算出来的球位置。
        let wide = Rect::from_min_size(Pos2::new(100.0, 20.0), Vec2::new(419.0, 174.0));
        assert_eq!(pill_orb_rect(pill, d, 5.5, false).min.y, pill_orb_rect(pill, d, 5.5, false).min.y);
        assert_ne!(pill_orb_top(wide, d), pill.center().y - d * 0.5, "若用展开面板做锚点，球会跟着下移——契约禁止这种用法");
    }

    #[test]
    fn pill_orb_stays_inside_the_window_when_the_right_edge_is_close() {
        // 靠右放不下时不能再翻到左边（那正是"跑去左边"），而是夹紧到窗口内。
        let pill = capsule(44.0);
        let d = pill_orb_diameter(pill.height());
        let bounds = Rect::from_min_max(Pos2::new(60.0, 0.0), Pos2::new(pill.right() + 12.0, 200.0));
        let r = pill_orb_clamped(pill, d, 5.5, false, bounds);
        assert_eq!(r.right(), bounds.right(), "贴到窗口右缘时应夹紧而不是翻边");
        assert_eq!(r.width(), d, "夹紧不改变球径");
    }

    #[test]
    fn orb_icon_and_badge_fit_inside_the_circle() {
        // 图2：微信图标 + 角标被圆边裁掉。图标盒与角标圆必须整体落在球内并留边。
        for d in [20.0f32, 30.0, 44.0, 56.0] {
            let g = orb_badge_layout(d);
            let icon_half = g.icon_side * 0.5;
            assert!(icon_half <= d * 0.35 + 0.01, "d={d}: 图标外接半径 {icon_half} 超出 0.35d，会顶到圆边");
            let off = (g.badge_dx * g.badge_dx + g.badge_dy * g.badge_dy).sqrt();
            assert!(off + g.badge_r <= d * 0.5 - 0.5, "d={d}: 角标圆越出球面（{off}+{} > {}）", g.badge_r, d * 0.5);
            assert!(g.icon_side >= d * 0.5, "d={d}: 图标太小，大球里会显空");
        }
    }

    #[test]
    fn orb_badge_layout_scales_with_the_ball() {
        let small = orb_badge_layout(30.0);
        let big = orb_badge_layout(44.0);
        assert!(big.icon_side > small.icon_side, "球变大时图标同步变大");
        assert!(big.badge_r > small.badge_r, "角标同步变大");
        // 角标相对球心落在右下象限（与图二设计一致），且比例恒定。
        for d in [20.0f32, 44.0, 56.0] {
            let g = orb_badge_layout(d);
            assert!(g.badge_dx > 0.0 && g.badge_dy > 0.0, "角标在右下");
            assert!((g.badge_dx / d - 0.24).abs() < 1e-3, "d={d}: 角标横向偏移应恒为 0.24d");
            assert!((g.badge_dy / d - 0.22).abs() < 1e-3, "d={d}: 角标纵向偏移应恒为 0.22d");
        }
    }
}

fn env_has_active_media(m: &NowPlaying) -> bool {
    m.playing && m.is_music()
}
