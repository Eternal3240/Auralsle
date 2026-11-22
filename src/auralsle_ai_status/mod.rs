//! # 岛仔 · AI 工作状态 mod（独立模块）
//!
//! 本目录是**自包含**的：数据读取、状态机、界面、设置持久化、AI 品牌图标都在这里。
//! 上游只需要少量极小挂载点（见 `patch/mounts.md`），不改动任何既有功能逻辑。
//!
//! 数据源沿用 ZenDesktop ai-status 协议的状态文件，与任务栏 AI 状态、
//! Windhawk `zen-ai-status` mod、`zen-status` CLI / MCP 共用，不重复造轮子。
//!
//! 与 AI 余额的关系：额度功能读 `ai-quota.json`，本 mod 读 `ai-status\*.json`，
//! 两者数据、配置、界面区域完全独立，互不覆盖。

pub mod config;
pub mod course;
pub mod course_import;
pub mod course_ui;
pub mod aora_shapes_data;
pub mod aora_face;
pub mod aora_ui;
pub mod islo_face;
pub mod plugins;
pub mod shot;
pub mod state;
pub mod sysmon;
pub mod ui;
pub mod xlsx;

use eframe::egui;

/// 岛上新增的第 9 个页面：AI 工作（`PAGE_NAMES[AI_PAGE]`）。
pub const AI_PAGE: usize = 8;

// ---- 紧凑状态（时间右侧）的尺寸与基线 ----
/// AI 工具图标边长。
///
/// **所有**紧凑控件里的图标都用这个尺寸：时间右侧的「AI 额度」logo
/// （`quota_ui.rs` 里也引用本常量）与「AI 工作」图标必须一样大，
/// 否则一个 22、一个 13，并排在药丸里非常显眼。
pub const ICON: f32 = 13.0;
/// 紧凑状态条里「AI 工作」图标的边长：比 `ICON` 略大，和 13.5 号粗体状态文字
/// 的墨迹高度齐平。SVG 留白较多，13 看起来明显小于文字；`ICON` 本身不动，
/// 免得牵连 `quota_ui.rs` 的常驻额度图标与一堆布局宽度。
pub const COMPACT_ICON: f32 = 16.0;
/// 控件之间的唯一间距。
///
/// 取上游 `bar_layout::Layout::new` 里 `place()` 的 12，再加上文字两侧约 2 的字面留白 ——
/// 视觉上就正好等于「时间 → AI 额度图标」那段间距（实测约 16）。药丸里所有相邻控件
/// （时间 / 额度图标 / 额度数字 / AI 工作图标 / 状态文字）都用它。
pub const GAP: f32 = 14.0;
/// 图标与它自己的文字之间留多少：比 `GAP` 小，读起来是一整块；
/// 块与块之间仍然按 `GAP` 排，所以项目间距不变。
pub const ICON_TEXT_GAP: f32 = 6.0;
/// 主岛并排展示多个 AI 状态时的块间间距。
pub const COMPACT_ITEM_GAP: f32 = 10.0;
/// 岛外元素与岛体边缘的间距（课程卡、任务结束提示共用）。
pub const OUT_GAP: f32 = 8.0;
/// 岛外未读消息图标的基准边长。
pub const BALL_ICON: f32 = 20.0;
/// 展开态未读图标的垂直中心；微信胶囊里的图标按这个常量下移对齐。
pub const BALL_CY: f32 = 28.0;
/// 微信气泡几何中心比锚点低约 1.5 个缩放单位，未读球贴图严格居中。
/// 把这个差值从锚点里扣掉，视觉重心才真正齐平，而不只是锚点数值相等。
pub const MESSENGER_OPTICAL_Y: f32 = 1.6;

/// 未读消息图标统一缩放：微信气泡本体 17 高、QQ 贴图 19 高，
/// 用 19/17 补偿微信，让两种来源与旁边的 AI 图标是同一视觉尺寸。
/// `compact=true` 对齐药丸里的 `COMPACT_ICON`，否则对齐岛外 `BALL_ICON`。
pub fn unread_icon_scale(wechat: bool, compact: bool) -> f32 {
    let side = if compact { COMPACT_ICON } else { BALL_ICON };
    let base = side / 19.0;
    if wechat { base * 19.0 / 17.0 } else { base }
}
/// 收起态药丸内两球的垂直中心，与 `ICON_CY` 对齐（微信入口原本 14.0 偏上）。
pub const BALL_CY_COMPACT: f32 = 15.8;
/// 展开面板宽度（上游 `app::PW`）。课程模块按这个宽度定位，
/// 岛体收起时卡片不会跟着左右滑动。
pub const PANEL_W: f32 = 419.0;
/// 药丸内容区右端的内边距：上游 `Layout::new` 里是 16（左右各 16）。
/// 紧凑状态贴着它收尾，看起来才和左边「电池 / 时间」的留白对称。
pub const RIGHT_PAD: f32 = 16.0;
/// 收起态未读入口（微信 / QQ 图标）占的尾部宽度。
///
/// 图标原来挂在药丸右侧**外面**（`panel.right()-2`），视觉上右端几乎没有留白，
/// 和左端的 16 内边距不对称。现在把这段宽度提前从布局里留出来，
/// 图标落在药丸自己的 16 内边距里，两端留白一致。
pub const COMPACT_UNREAD_W: f32 = 21.0;
/// 未读图标与左侧「AI 状态」之间的间距。
///
/// 改成与全局 `GAP` 一致：原来 8 会让「已完成」和微信图标挤在一起，
/// 右侧这一组的留白比左侧（时间→状态）小一截，看着不平衡。
pub const COMPACT_UNREAD_GAP: f32 = GAP;
/// 收起态图标之间的横向步长（多来源时叠放）。
pub const COMPACT_UNREAD_STEP: f32 = 22.0;
/// 右端实际要让出的宽度：`16 内边距 + 未读区`。
pub fn right_pad() -> f32 { RIGHT_PAD + compact_tail() }
/// 每帧由 `app.rs` 写入当前未读区宽度（无未读时为 0）。
static COMPACT_TAIL_BITS: AtomicU32 = AtomicU32::new(0);
pub fn set_compact_tail(width: f32) {
    COMPACT_TAIL_BITS.store(width.max(0.0).to_bits(), Ordering::Relaxed);
}
pub fn compact_tail() -> f32 { f32::from_bits(COMPACT_TAIL_BITS.load(Ordering::Relaxed)) }
/// 未读与健康徽章区需要的宽度（含与左侧控件的间距）。
pub fn compact_tail_width(icons: usize) -> f32 {
    if icons == 0 { 0.0 } else { COMPACT_UNREAD_W + COMPACT_UNREAD_GAP + COMPACT_UNREAD_STEP * (icons as f32 - 1.0) }
}
/// 图标中心 y：与状态文字的光学中心对齐。
pub const ICON_CY: f32 = 15.8;
/// 状态文字基线：与药丸时钟一致（`paint::clock` 传 21.0），字号小两号。
pub const TEXT_BASE: f32 = 21.0;

/// 每帧刷新状态。上游只需在 `App::frame` 开头调用一次。
pub fn tick(now: f64) {
    shot::tick();
    state::tick(now);
    // 每帧先清掉上一帧的课程卡矩形：没画就不会留下看不见的模糊/可点区域。
    record_course_card_rect(None);
    record_device_pill_rects(Vec::new());
}

/// 主界面该显示的助手。
pub(crate) struct Shown {
    pub agent: state::Agent,
}

/// 这些状态意味着「人该回去看一眼 AI 工具了」。
pub(crate) fn needs_return(p: state::Phase) -> bool {
    matches!(p, state::Phase::Done | state::Phase::Waiting | state::Phase::Error)
}

/// 主界面当前展示的助手列表（最多 N 个）：优先活跃，其次仍在保留窗口内的「已完成」。
/// 没有可展示的就返回空 —— 岛屿随之收回，不会一直挂着。
pub(crate) fn shown_list() -> Vec<Shown> {
    let cfg = config::get();
    if !cfg.enabled || !cfg.compact {
        return Vec::new();
    }
    let limit = cfg.compact_max_count.clamp(1, 5) as usize;
    let (agents, _) = state::snapshot();
    agents
        .into_iter()
        .take(limit)
        .map(|agent| Shown { agent })
        .collect()
}

/// 主界面当前展示的主助手（兼容单条读取点）。
pub(crate) fn shown() -> Option<Shown> {
    shown_list().into_iter().next()
}

/// 状态字号：比药丸时钟小两号（15.5 → 13.5）。
/// 同样字号下中文字面比数字大，取齐时钟会显得偏大，所以按视觉大小回收两号。
pub(crate) fn label_size() -> f32 {
    crate::theme::size::PILL_CLOCK - 2.0
}

/// 估算文字宽度（CJK 按 1em、ASCII 按 0.56em）。
/// 布局留位在 `extend_layout` 里算，那里拿不到 `Card`，所以统一用估算；
/// 绘制时再用实测宽度画，最多差 1~2pt，不影响右对齐。
pub(crate) fn text_width(t: &str) -> f32 {
    t.chars()
        .map(|ch| if ch.is_ascii() { 0.56 } else { 1.0 })
        .sum::<f32>()
        * label_size()
}

/// 紧凑状态的宽度：只有「图标 + 状态文字」。
/// 「回到 AI」由任务结束提示卡片提供，不再占用岛体宽度。
pub(crate) fn compact_width(label: &str) -> f32 {
    ICON + ICON_TEXT_GAP + text_width(label)
}

/// 多个紧凑状态并排占用的总宽度。
pub(crate) fn compact_list_width_for(items: &[Shown]) -> f32 {
    if items.is_empty() {
        return 0.0;
    }
    let sum: f32 = items
        .iter()
        .map(|s| compact_width(s.agent.phase.label()))
        .sum();
    sum + COMPACT_ITEM_GAP * (items.len().saturating_sub(1) as f32)
}

pub(crate) fn compact_list_width() -> f32 {
    let items = shown_list();
    compact_list_width_for(&items)
}

/// 本帧所有独立卡片的矩形（绝对坐标）：课程卡（若有）+ 每一枚设备药丸。
/// 两者分开登记，课程卡消失时药丸依然在，窗口可见区域与悬停判定不会丢。
pub fn course_glass_rects() -> Vec<egui::Rect> {
    let mut v = Vec::new();
    if let Ok(slot) = course_card_slot().lock() {
        if let Some(r) = *slot { v.push(r); }
    }
    if let Ok(slot) = device_pill_slot().lock() {
        v.extend(slot.iter().copied());
    }
    v
}

/// 课程卡 + 设备药丸的整体外包矩形，供窗口可见区域与悬停判定使用。
/// 只取合并矩形做「窗口要覆盖多大」，绝不拿它去铺系统毛玻璃。
pub fn course_card_rect() -> Option<egui::Rect> {
    let mut it = course_glass_rects().into_iter();
    let first = it.next()?;
    Some(it.fold(first, |a, b| a.union(b)))
}

pub(crate) fn record_course_card_rect(rect: Option<egui::Rect>) {
    if let Ok(mut slot) = course_card_slot().lock() {
        *slot = rect;
    }
}

pub(crate) fn record_device_pill_rects(rects: Vec<egui::Rect>) {
    if let Ok(mut slot) = device_pill_slot().lock() {
        *slot = rects;
    }
}

fn course_card_slot() -> &'static std::sync::Mutex<Option<egui::Rect>> {
    static SLOT: std::sync::OnceLock<std::sync::Mutex<Option<egui::Rect>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(None))
}

fn device_pill_slot() -> &'static std::sync::Mutex<Vec<egui::Rect>> {
    static SLOT: std::sync::OnceLock<std::sync::Mutex<Vec<egui::Rect>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// 设置页第 14 页（课程表）是否处于打开状态，以及是否处于「编辑课程岛」模式。
/// 仅在编辑课程岛外观时固定显示课程岛，绝不强制展开或锁定主岛！
static COURSE_EDITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static COURSE_ISLAND_PIN_PREVIEW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_course_editing(v: bool) {
    COURSE_EDITING.store(v, std::sync::atomic::Ordering::Relaxed);
    if !v {
        COURSE_ISLAND_PIN_PREVIEW.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

pub fn set_course_island_pin_preview(v: bool) {
    COURSE_ISLAND_PIN_PREVIEW.store(v, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_course_island_editing() -> bool {
    COURSE_EDITING.load(std::sync::atomic::Ordering::Relaxed)
        && COURSE_ISLAND_PIN_PREVIEW.load(std::sync::atomic::Ordering::Relaxed)
}

/// 收起态布局：给时间右侧的紧凑状态留出空间。
///
/// 只调整 `width` 与既有组件坐标，不写入 `x[]` 的新槽位，
/// 因此不会影响 `reorder()` / `clearance()` 与任何既有布局测试。
pub(crate) fn extend_layout(layout: &mut crate::bar_layout::Layout) {
    let tail = compact_tail();
    let cfg = config::get();
    let ai_w = if cfg.enabled && cfg.compact {
        compact_list_width()
    } else {
        0.0
    };
    let ai_extra = if ai_w > 0.0 {
        ai_w + GAP + course_slot_width()
    } else {
        0.0
    };
    // 即使当前没有 AI 紧凑状态（ai_extra == 0），只要收起态右侧有微信/QQ未读图标（tail > 0），
    // 也必须把药丸加宽并将内部音律波形等控件左移 tail / 2.0，防止未读图标与播放音律条重叠（修复图三）！
    let extra = ai_extra + tail;
    if extra <= 0.0 {
        return;
    }
    for value in layout.x.iter_mut().flatten() {
        *value -= extra / 2.0;
    }
    layout.width += extra;
}

/// 紧凑状态的左端 x（药丸局部坐标，右端对齐 `RIGHT_PAD`）。
pub(crate) fn compact_x(layout: &crate::bar_layout::Layout) -> f32 {
    let w = compact_list_width();
    layout.width / 2.0 - right_pad() - course_slot_width() - w
}

/// 课程提示在收起态药丸里占的宽度（图标 + 间距）；不在提醒窗口内为 0。
pub(crate) fn course_slot_width() -> f32 {
    // [ai-status-mod] 收起态药丸不再显示「课」图标（课程信息由岛左侧日程卡片呈现）。
    0.0
}

/// 额度跟随「正在运行的 AI 工具」：识别到工具时只显示对应额度；
/// 一个都没识别到（比如都是网页版）就按配置全部显示。
pub fn quota_visible(_provider: crate::quota::Provider) -> bool {
    // 已启用且勾选「常驻岛上」的 AI 额度账号始终在顶栏显示，不再因为后台运行了某一个 AI 进程就隐藏其它全部 AI 额度工具。
    true
}

/// 收起态课程提示图标的实际矩形（绝对坐标），展开时从这里滑出去。
pub(crate) fn record_course_icon(rect: Option<egui::Rect>) {
    if let Ok(mut slot) = course_icon_slot().lock() {
        *slot = rect;
    }
}
pub(crate) fn course_icon_rect() -> Option<egui::Rect> {
    course_icon_slot().lock().ok().and_then(|slot| *slot)
}
fn course_icon_slot() -> &'static std::sync::Mutex<Option<egui::Rect>> {
    static SLOT: std::sync::OnceLock<std::sync::Mutex<Option<egui::Rect>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(None))
}

/// 岛边缘描边开关：0.6.39 控制 `shape::fill` 的轮廓羽化带，
/// 0.6.63 控制 `App::glass_rim()` 返回的毛玻璃边缘高光。
/// 默认 false = 取消描边，其余视觉样式不变。
pub fn outline_enabled() -> bool {
    config::get().outline
}

/// [venu] 设置页点击「Preview（预览）」按钮时触发一次 5 秒的实机岛体边框光效演示。
static BORDER_PREVIEW_UNTIL_MS: AtomicI64 = AtomicI64::new(0);

pub fn trigger_border_preview() {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    BORDER_PREVIEW_UNTIL_MS.store(now_ms + 5000, Ordering::Relaxed);
}

pub fn border_preview_active() -> bool {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    BORDER_PREVIEW_UNTIL_MS.load(Ordering::Relaxed) > now_ms
}

// ---------------------------------------------------------------
// 音乐暂停自动回退：状态机（纯函数，不碰上游 App 的音乐控制）
// ---------------------------------------------------------------

/// 只在「上一帧播放 → 本帧暂停」的边沿开始计时。
/// 从未播放过（例如打开软件时音乐本来就是暂停的）不会启动计时，
/// 这样上游那些用 `playing=false` 造歌词态的测试完全不受影响。
pub fn next_pause_since(
    prev: Option<f64>,
    was_playing: bool,
    playing: bool,
    t: f64,
) -> Option<f64> {
    if playing {
        None
    } else if was_playing && prev.is_none() {
        Some(t)
    } else {
        prev
    }
}

/// 暂停是否已经超过给定秒数（0 = 永不回退）；纯函数，单测不读本机配置。
pub fn pause_expired(paused_since: Option<f64>, t: f64, secs: i64) -> bool {
    secs > 0 && paused_since.is_some_and(|s| t - s >= secs as f64)
}

/// 上游 `island_lyrics_active()` 用的入口：秒数来自用户配置。
pub fn music_pause_expired(paused_since: Option<f64>, t: f64) -> bool {
    pause_expired(paused_since, t, config::get().music_pause_return_secs)
}

// ---------------------------------------------------------------
// 课程卡「上课免打扰」：跨模块的一次性请求
// ---------------------------------------------------------------

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};

static FOCUS_REQ: AtomicBool = AtomicBool::new(false);
static DND_UNTIL: AtomicI64 = AtomicI64::new(0);

/// 请求启动岛仔自带专注（由 `app.rs::tick` 消费）。
pub fn request_focus() {
    FOCUS_REQ.store(true, Ordering::Relaxed);
}
pub fn take_focus_request() -> bool {
    FOCUS_REQ.swap(false, Ordering::Relaxed)
}

/// 仅岛内免打扰：到该时间戳前不弹 mod 自己的提示。
pub fn set_dnd_until(ts: i64) {
    DND_UNTIL.store(ts, Ordering::Relaxed);
}
pub fn dnd_active(now: i64) -> bool {
    DND_UNTIL.load(Ordering::Relaxed) > now
}

/// 系统级免打扰：只把用户带到 Windows 通知设置页，由用户自己开启。
/// 程序不读写任何系统设置，避免替用户改动通知权限。
pub fn open_notification_settings() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", "ms-settings:notifications"])
            .creation_flags(0x0800_0000)
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_timer_only_starts_on_play_to_pause_edge() {
        assert_eq!(next_pause_since(None, false, false, 1.0), None);
        assert_eq!(next_pause_since(None, true, true, 1.0), None);
        let s = next_pause_since(None, true, false, 5.0);
        assert_eq!(s, Some(5.0));
        assert_eq!(next_pause_since(s, false, false, 9.0), Some(5.0));
        assert_eq!(next_pause_since(s, false, true, 9.0), None);
    }

    #[test]
    fn pause_expiry_respects_delay_and_resume() {
        assert!(pause_expired(Some(0.0), 10.0, 10));
        assert!(!pause_expired(Some(0.0), 9.9, 10));
        assert!(!pause_expired(None, 999.0, 10));
        assert!(!pause_expired(Some(0.0), 999.0, 0), "0 秒 = 永不回退");
        assert!(pause_expired(Some(0.0), 3.0, 3), "自定义 N 秒应生效");
    }

    #[test]
    fn compact_list_width_accumulates_multiple_agents() {
        let mk = |key: &str, phase: state::Phase| Shown {
            agent: state::Agent {
                key: key.to_owned(),
                name: key.to_owned(),
                phase,
                message: String::new(),
                detail: String::new(),
                project: String::new(),
                updated: 0,
                age_secs: 0,
                step: String::new(),
                step_index: 0,
                step_total: 0,
                progress: -1.0,
                elapsed: 0,
                quiet: false,
                provider_key: key.to_owned(),
                title: String::new(),
                model: String::new(),
                effort: String::new(),
                is_cli: false,
                cwd: String::new(),
            },
        };
        let one = vec![mk("codex", state::Phase::Working)];
        let three = vec![
            mk("codex", state::Phase::Working),
            mk("claude", state::Phase::Waiting),
            mk("gemini", state::Phase::Done),
        ];
        let w1 = compact_list_width_for(&one);
        let w3 = compact_list_width_for(&three);
        assert!(w3 > w1 * 2.5, "3 个状态宽度应明显大于单个: w1={w1}, w3={w3}");
        assert_eq!(compact_list_width_for(&[]), 0.0);
    }
}
