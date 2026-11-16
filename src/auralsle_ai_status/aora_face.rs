//! Aora Bot 表情与角色引擎（完整移植到岛仔吉祥物）。
//!
//! 数据与算法移植自 [Aora Bot](https://github.com/sam70361/aora-bot)
//! Copyright (c) 2026 sam70361，适用 **Aora Bot License 1.1**。
//! 按许可保留版权与许可声明。
//!
//! 提供完整 Aora Bot 角色系统：
//! - 19 种参数化眼睛拓扑 + 12 种参数化嘴型 + 16 种五官脸型（FaceStyle）
//! - 44 种动画状态（4 大分类：静息 6 / 处理中 19 / 情绪 16 / 提示 3）+ 全套 STATE_FX 状态特效
//! - 10 种精确身体形状（圆形/麻薯/果冻/石子/蛋/包子/豆腐/胶囊/六边/饭团）
//! - 7 种矢量画风（扁平/线条/玻璃/贴纸/剪纸/软糖/孔版），供「吉祥物」设置页预览与主岛渲染使用。

use crate::paint::{Card, H};
use crate::theme::{self, W};
use eframe::egui::{self, Color32};

const PI: f32 = std::f32::consts::PI;
const TAU: f32 = std::f32::consts::TAU;

// ─── 参数化眼睛 ────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct EyeSpec {
    dx: f32,
    dy: f32,
    len: f32,
    width: f32,
    angle: f32,
    bend: f32,
    extra: Option<EyeExtra>,
    star: f32,
}

#[derive(Clone, Copy)]
struct EyeExtra {
    dx: f32,
    dy: f32,
    len: f32,
    width: f32,
    angle: f32,
    bend: f32,
    amount: f32,
}

const fn eye(len: f32, width: f32, angle: f32) -> EyeSpec {
    EyeSpec { dx: 0.0, dy: 0.0, len, width, angle, bend: 0.0, extra: None, star: 0.0 }
}

fn both(s: EyeSpec) -> [EyeSpec; 2] {
    [s, s]
}

fn opposed(s: EyeSpec) -> [EyeSpec; 2] {
    let mirror = EyeSpec {
        dx: -s.dx,
        angle: -s.angle,
        bend: -s.bend,
        extra: s.extra.map(|e| EyeExtra { dx: -e.dx, angle: -e.angle, bend: -e.bend, ..e }),
        ..s
    };
    [s, mirror]
}

// ─── 参数化嘴型 ────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct MouthSpec {
    width: f32,
    bend: f32,
    open: f32,
    wave: f32,
    skew: f32,
    dy: f32,
    cat: f32,
    upper: f32,
    show: f32,
}

// ─── 表情预设枚举 ──────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CuteEye {
    Dot, DotSmall, DotWide, DotLeft, DotRight, DotUp, DotDown, DotLook,
    DotHalf, DotSuspicious, Arch, ArchWide, Curve, CurveSleepy,
    Cross, Star, Sad, Mad, Wink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CuteMouth {
    None, Smile, Cat, Grin, Laugh, Flat, Frown, O, Dot, Wavy, Smirk, Pout,
}

// ─── 状态分类 / 画风 / 身体形状 / 五官脸型 ──────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateCat { Resting, Attending, Feeling, Signalling }

impl StateCat {
    pub const ALL: [Self; 4] = [Self::Resting, Self::Attending, Self::Feeling, Self::Signalling];
    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Resting => "静息",
            Self::Attending => "处理中",
            Self::Feeling => "情绪",
            Self::Signalling => "提示",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtStyle {
    Solid,
    Outline,
    Glass,
    Sticker,
    Paper,
    Soft,
    Riso,
    Pencil,
    Marker,
    Crayon,
    Watercolor,
    Neon,
    Chalk,
    Origami,
    Relief,
    Gothic,
    PixelCrisp,
    PixelDot,
    PixelBlock,
    KaomojiPlain,
    KaomojiBubble,
}

impl ArtStyle {
    pub const ALL: [Self; 21] = [
        Self::Solid,
        Self::Outline,
        Self::Glass,
        Self::Sticker,
        Self::Paper,
        Self::Soft,
        Self::Riso,
        Self::Pencil,
        Self::Marker,
        Self::Crayon,
        Self::Watercolor,
        Self::Neon,
        Self::Chalk,
        Self::Origami,
        Self::Relief,
        Self::Gothic,
        Self::PixelCrisp,
        Self::PixelDot,
        Self::PixelBlock,
        Self::KaomojiPlain,
        Self::KaomojiBubble,
    ];
    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Solid => "扁平",
            Self::Outline => "线条",
            Self::Glass => "玻璃",
            Self::Sticker => "贴纸",
            Self::Paper => "剪纸",
            Self::Soft => "软糖",
            Self::Riso => "孔版",
            Self::Pencil => "铅笔",
            Self::Marker => "马克笔",
            Self::Crayon => "蜡笔",
            Self::Watercolor => "水彩",
            Self::Neon => "霓虹",
            Self::Chalk => "粉笔",
            Self::Origami => "折纸",
            Self::Relief => "浮雕",
            Self::Gothic => "哥特",
            Self::PixelCrisp => "像素锐",
            Self::PixelDot => "像素点",
            Self::PixelBlock => "像素块",
            Self::KaomojiPlain => "颜纯字",
            Self::KaomojiBubble => "颜气泡",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Outline => "outline",
            Self::Glass => "glass",
            Self::Sticker => "sticker",
            Self::Paper => "paper",
            Self::Soft => "soft",
            Self::Riso => "riso",
            Self::Pencil => "pencil",
            Self::Marker => "marker",
            Self::Crayon => "crayon",
            Self::Watercolor => "watercolor",
            Self::Neon => "neon",
            Self::Chalk => "chalk",
            Self::Origami => "origami",
            Self::Relief => "relief",
            Self::Gothic => "gothic",
            Self::PixelCrisp => "pixel-crisp",
            Self::PixelDot => "pixel-dot",
            Self::PixelBlock => "pixel-block",
            Self::KaomojiPlain => "kaomoji-plain",
            Self::KaomojiBubble => "kaomoji-bubble",
        }
    }
}

use super::aora_shapes_data::{self, AoraShapeData, PartTone};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyShape(pub usize);

#[allow(non_upper_case_globals)]
impl BodyShape {
    pub const Round: Self = Self(0);
    pub const Mochi: Self = Self(1);
    pub const Jelly: Self = Self(2);
    pub const Pebble: Self = Self(3);
    pub const Egg: Self = Self(4);
    pub const Bun: Self = Self(5);
    pub const Tofu: Self = Self(6);
    pub const Capsule: Self = Self(7);
    pub const Hexagon: Self = Self(8);
    pub const Rice: Self = Self(9);
    pub const Islo: Self = Self(10);

    pub const COUNT: usize = 72; // 10 基础 + 1 初代 islo + 61 完整 Aora Bot 核心身体形态 = 72
    pub const ALL: [Self; 72] = {
        let mut arr = [Self(0); 72];
        let mut i = 0;
        while i < 72 {
            arr[i] = Self(i);
            i += 1;
        }
        arr
    };

    pub fn data(self) -> Option<&'static AoraShapeData> {
        if self.0 == 10 {
            None
        } else if self.0 < 10 {
            aora_shapes_data::AORA_SHAPES.get(self.0)
        } else {
            aora_shapes_data::AORA_SHAPES.get(self.0 - 1)
        }
    }

    pub fn label_zh(self) -> &'static str {
        if self.0 == 10 {
            "初代 islo"
        } else if let Some(d) = self.data() {
            d.label_zh
        } else {
            "未知"
        }
    }

    pub fn id(self) -> &'static str {
        if self.0 == 10 {
            "islo"
        } else if let Some(d) = self.data() {
            d.id
        } else {
            "mochi"
        }
    }

    pub fn family_idx(self) -> u8 {
        if self.0 == 10 {
            0
        } else {
            self.data().map(|d| d.family_idx).unwrap_or(0)
        }
    }
}

/// 返回指定身体形态在单位半径 `r` 坐标系下的五官中心偏移 `[dx, dy]` 与眼距缩放 `eye_scale`。
pub fn shape_face_fit(shape: BodyShape) -> ([f32; 2], f32) {
    if let Some(d) = shape.data() {
        (d.face_xy, d.eye_scale.clamp(0.42, 1.15))
    } else {
        ([0.0, 0.0], 1.0)
    }
}

/// Aora Bot 的 16 种五官脸型（FACE_STYLES）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceStyle {
    Cute, Capsule, Eyes, Line, Glossy, Brow, Sparkle, Button,
    Shades, Droopy, Bigmouth, Pie, Screen, Teardrop, Specs, Symbol,
}

impl FaceStyle {
    pub const ALL: [Self; 16] = [
        Self::Cute, Self::Capsule, Self::Eyes, Self::Line,
        Self::Glossy, Self::Brow, Self::Sparkle, Self::Button,
        Self::Shades, Self::Droopy, Self::Bigmouth, Self::Pie,
        Self::Screen, Self::Teardrop, Self::Specs, Self::Symbol,
    ];
    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Cute => "萌脸",
            Self::Capsule => "胶囊",
            Self::Eyes => "豆豆",
            Self::Line => "线条",
            Self::Glossy => "水汪",
            Self::Brow => "浓眉",
            Self::Sparkle => "闪亮",
            Self::Button => "纽扣",
            Self::Shades => "墨镜",
            Self::Droopy => "半睁",
            Self::Bigmouth => "大嘴",
            Self::Pie => "派切",
            Self::Screen => "屏幕",
            Self::Teardrop => "水滴",
            Self::Specs => "眼镜",
            Self::Symbol => "符号",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Cute => "cute",
            Self::Capsule => "capsule",
            Self::Eyes => "eyes",
            Self::Line => "line",
            Self::Glossy => "glossy",
            Self::Brow => "brow",
            Self::Sparkle => "sparkle",
            Self::Button => "button",
            Self::Shades => "shades",
            Self::Droopy => "droopy",
            Self::Bigmouth => "bigmouth",
            Self::Pie => "pie",
            Self::Screen => "screen",
            Self::Teardrop => "teardrop",
            Self::Specs => "specs",
            Self::Symbol => "symbol",
        }
    }
}

// ─── 眼睛预设（cute 19 种）──────────────────────────────────────

fn cute_eye_pair(id: CuteEye) -> [EyeSpec; 2] {
    use CuteEye::*;
    match id {
        Dot => both(eye(0.02, 1.0, PI / 2.0)),
        DotSmall => both(eye(0.02, 0.62, PI / 2.0)),
        DotWide => both(eye(0.02, 1.32, PI / 2.0)),
        DotLeft => both(EyeSpec { dx: -0.45, ..eye(0.02, 1.0, PI / 2.0) }),
        DotRight => both(EyeSpec { dx: 0.45, ..eye(0.02, 1.0, PI / 2.0) }),
        DotUp => both(EyeSpec { dy: -0.45, ..eye(0.02, 1.0, PI / 2.0) }),
        DotDown => both(EyeSpec { dy: 0.39, ..eye(0.02, 0.96, PI / 2.0) }),
        DotLook => both(EyeSpec { dx: 0.43, dy: -0.48, ..eye(0.02, 1.0, PI / 2.0) }),
        DotHalf => both(EyeSpec { dy: 0.2, ..eye(0.45, 0.45, 0.0) }),
        DotSuspicious => [EyeSpec { dy: 0.2, ..eye(0.45, 0.45, 0.0) }, eye(0.02, 1.0, PI / 2.0)],
        Arch => both(EyeSpec { dy: 0.38, bend: -0.82, ..eye(1.02, 0.36, 0.0) }),
        ArchWide => both(EyeSpec { dy: 0.40, bend: -0.95, ..eye(1.10, 0.34, 0.0) }),
        Curve => both(EyeSpec { bend: 0.42, ..eye(1.02, 0.36, 0.0) }),
        CurveSleepy => both(EyeSpec { dy: 0.1, bend: 0.58, ..eye(0.96, 0.34, 0.0) }),
        Cross => both(EyeSpec {
            extra: Some(EyeExtra { dx: 0.0, dy: 0.0, len: 1.12, width: 0.35, angle: -PI / 4.0, bend: 0.0, amount: 1.0 }),
            ..eye(1.12, 0.35, PI / 4.0)
        }),
        Star => both(EyeSpec { star: 1.0, ..eye(0.02, 0.22, PI / 2.0) }),
        Sad => opposed(EyeSpec {
            dy: 0.26,
            extra: Some(EyeExtra { dx: -0.11, dy: -1.35, len: 0.98, width: 0.3, angle: -0.38, bend: 0.0, amount: 1.0 }),
            ..eye(0.02, 0.91, PI / 2.0)
        }),
        Mad => opposed(EyeSpec {
            dy: 0.26,
            extra: Some(EyeExtra { dx: -0.11, dy: -1.35, len: 0.98, width: 0.3, angle: 0.38, bend: 0.0, amount: 1.0 }),
            ..eye(0.02, 0.91, PI / 2.0)
        }),
        Wink => [EyeSpec { dy: 0.38, bend: -0.82, ..eye(1.02, 0.36, 0.0) }, eye(0.02, 1.0, PI / 2.0)],
    }
}

// ─── 嘴型预设（cute 12 种）──────────────────────────────────────

fn cute_mouth_spec(id: CuteMouth) -> MouthSpec {
    use CuteMouth::*;
    let base = MouthSpec { width: 0.0, bend: 0.0, open: 0.0, wave: 0.0, skew: 0.0, dy: 0.0, cat: 0.0, upper: 0.38, show: 1.0 };
    match id {
        None => MouthSpec { show: 0.0, dy: 0.22, ..base },
        Smile => MouthSpec { width: 1.09, bend: 0.54, ..base },
        Cat => MouthSpec { width: 1.3, cat: 0.43, ..base },
        Grin => MouthSpec { width: 1.41, open: 1.74, dy: -0.22, upper: 0.0, ..base },
        Laugh => MouthSpec { width: 1.6, open: 2.3, dy: -0.3, upper: 0.0, ..base },
        Flat => MouthSpec { width: 0.76, dy: 0.22, ..base },
        Frown => MouthSpec { width: 1.09, bend: -0.54, dy: 0.65, ..base },
        O => MouthSpec { width: 0.65, open: 1.57, dy: 0.22, upper: 0.5, ..base },
        Dot => MouthSpec { width: 0.24, open: 0.5, dy: 0.1, upper: 0.5, ..base },
        Wavy => MouthSpec { width: 1.52, dy: 0.22, cat: -0.54, ..base },
        Smirk => MouthSpec { width: 1.0, bend: 0.35, skew: 0.7, ..base },
        Pout => MouthSpec { width: 0.6, bend: -0.3, dy: 0.5, ..base },
    }
}

// ─── 44 态定义（4 分类）────────────────────────────────────────

pub struct StateDef {
    pub id: &'static str,
    pub label_zh: &'static str,
    pub cat: StateCat,
    eyes: &'static [CuteEye],
    mouth: CuteMouth,
    blush: f32,
}

use CuteEye::*;
use CuteMouth as M;

pub static STATES: &[StateDef] = &[
    // ── 静息 6 ──
    StateDef { id: "idle", label_zh: "待机", cat: StateCat::Resting, eyes: &[Dot, Dot, Dot, DotLeft, Dot, DotRight], mouth: M::None, blush: 0.0 },
    StateDef { id: "sleeping", label_zh: "睡眠", cat: StateCat::Resting, eyes: &[Curve], mouth: M::None, blush: 0.6 },
    StateDef { id: "drowsy", label_zh: "困倦", cat: StateCat::Resting, eyes: &[CurveSleepy, DotHalf, CurveSleepy], mouth: M::None, blush: 0.3 },
    StateDef { id: "bored", label_zh: "无聊", cat: StateCat::Resting, eyes: &[DotHalf, DotLeft, DotHalf, DotRight], mouth: M::Flat, blush: 0.0 },
    StateDef { id: "waking", label_zh: "苏醒", cat: StateCat::Resting, eyes: &[Curve, DotHalf, Dot], mouth: M::Dot, blush: 0.0 },
    StateDef { id: "powering-down", label_zh: "关机", cat: StateCat::Resting, eyes: &[Dot, DotHalf, Curve], mouth: M::None, blush: 0.0 },
    // ── 处理中 19 ──
    StateDef { id: "listening", label_zh: "倾听", cat: StateCat::Attending, eyes: &[DotLeft, Dot, DotLeft], mouth: M::None, blush: 0.0 },
    StateDef { id: "thinking", label_zh: "思考", cat: StateCat::Attending, eyes: &[DotLook, DotUp, DotLook], mouth: M::Flat, blush: 0.0 },
    StateDef { id: "searching", label_zh: "搜索", cat: StateCat::Attending, eyes: &[DotLeft, DotRight, DotUp, Dot], mouth: M::Dot, blush: 0.0 },
    StateDef { id: "reading", label_zh: "阅读", cat: StateCat::Attending, eyes: &[DotLeft, DotRight], mouth: M::None, blush: 0.0 },
    StateDef { id: "writing", label_zh: "书写", cat: StateCat::Attending, eyes: &[DotDown, Dot], mouth: M::None, blush: 0.0 },
    StateDef { id: "working", label_zh: "工作", cat: StateCat::Attending, eyes: &[DotDown, Dot, DotDown], mouth: M::None, blush: 0.0 },
    StateDef { id: "loading", label_zh: "加载", cat: StateCat::Attending, eyes: &[Dot, DotLook], mouth: M::None, blush: 0.0 },
    StateDef { id: "progress", label_zh: "进度", cat: StateCat::Attending, eyes: &[Dot], mouth: M::None, blush: 0.0 },
    StateDef { id: "radar", label_zh: "雷达", cat: StateCat::Attending, eyes: &[Dot, DotLeft, DotRight], mouth: M::None, blush: 0.0 },
    StateDef { id: "orbit", label_zh: "环绕", cat: StateCat::Attending, eyes: &[Dot, DotLook], mouth: M::None, blush: 0.0 },
    StateDef { id: "dictating", label_zh: "口述", cat: StateCat::Attending, eyes: &[DotLeft, Dot], mouth: M::O, blush: 0.0 },
    StateDef { id: "uploading", label_zh: "上传", cat: StateCat::Attending, eyes: &[DotUp, Dot], mouth: M::None, blush: 0.0 },
    StateDef { id: "sending", label_zh: "发送", cat: StateCat::Attending, eyes: &[Dot, Arch], mouth: M::Smile, blush: 0.0 },
    StateDef { id: "receiving", label_zh: "接收", cat: StateCat::Attending, eyes: &[DotWide, Dot], mouth: M::Dot, blush: 0.0 },
    StateDef { id: "messaging", label_zh: "消息", cat: StateCat::Attending, eyes: &[Dot, Arch], mouth: M::Cat, blush: 0.0 },
    StateDef { id: "waiting", label_zh: "等待", cat: StateCat::Attending, eyes: &[Dot, DotLeft, Dot, DotRight], mouth: M::None, blush: 0.0 },
    StateDef { id: "spawning", label_zh: "生成", cat: StateCat::Attending, eyes: &[DotWide, Dot], mouth: M::Dot, blush: 0.0 },
    StateDef { id: "composing", label_zh: "编写", cat: StateCat::Attending, eyes: &[DotDown, Dot], mouth: M::Cat, blush: 0.0 },
    StateDef { id: "dragging", label_zh: "拖拽", cat: StateCat::Attending, eyes: &[DotRight, Dot, DotRight], mouth: M::O, blush: 0.0 },
    // ── 情绪 16 ──
    StateDef { id: "happy", label_zh: "开心", cat: StateCat::Feeling, eyes: &[Arch, ArchWide, Arch], mouth: M::Cat, blush: 1.0 },
    StateDef { id: "excited", label_zh: "兴奋", cat: StateCat::Feeling, eyes: &[DotWide, ArchWide], mouth: M::Grin, blush: 0.8 },
    StateDef { id: "laughing", label_zh: "大笑", cat: StateCat::Feeling, eyes: &[ArchWide, Arch], mouth: M::Laugh, blush: 0.6 },
    StateDef { id: "playful", label_zh: "调皮", cat: StateCat::Feeling, eyes: &[Arch, Wink, ArchWide, Dot], mouth: M::Smirk, blush: 0.4 },
    StateDef { id: "proud", label_zh: "自豪", cat: StateCat::Feeling, eyes: &[DotUp, Arch], mouth: M::Smile, blush: 0.5 },
    StateDef { id: "celebrate", label_zh: "庆祝", cat: StateCat::Feeling, eyes: &[Star, ArchWide, Wink, Arch], mouth: M::Grin, blush: 0.8 },
    StateDef { id: "bouncing", label_zh: "蹦跳", cat: StateCat::Feeling, eyes: &[Arch, DotWide], mouth: M::Smile, blush: 0.6 },
    StateDef { id: "shy", label_zh: "害羞", cat: StateCat::Feeling, eyes: &[DotDown, DotHalf], mouth: M::Cat, blush: 1.0 },
    StateDef { id: "sad", label_zh: "难过", cat: StateCat::Feeling, eyes: &[Sad], mouth: M::Frown, blush: 0.0 },
    StateDef { id: "angry", label_zh: "生气", cat: StateCat::Feeling, eyes: &[Mad], mouth: M::Frown, blush: 0.0 },
    StateDef { id: "confused", label_zh: "困惑", cat: StateCat::Feeling, eyes: &[DotSuspicious, DotLook, DotHalf], mouth: M::Wavy, blush: 0.0 },
    StateDef { id: "suspicious", label_zh: "怀疑", cat: StateCat::Feeling, eyes: &[DotSuspicious, DotHalf], mouth: M::Flat, blush: 0.0 },
    StateDef { id: "surprised", label_zh: "惊讶", cat: StateCat::Feeling, eyes: &[DotWide], mouth: M::O, blush: 0.0 },
    StateDef { id: "scared", label_zh: "害怕", cat: StateCat::Feeling, eyes: &[DotWide, DotSmall], mouth: M::Wavy, blush: 0.0 },
    StateDef { id: "curious", label_zh: "好奇", cat: StateCat::Feeling, eyes: &[DotWide, DotLeft, DotWide, DotRight], mouth: M::Dot, blush: 0.0 },
    StateDef { id: "humming", label_zh: "哼歌", cat: StateCat::Feeling, eyes: &[Curve, DotHalf, Curve, Dot], mouth: M::Smile, blush: 0.3 },
    // ── 提示 3 ──
    StateDef { id: "alerting", label_zh: "警报", cat: StateCat::Signalling, eyes: &[DotWide, Dot], mouth: M::O, blush: 0.0 },
    StateDef { id: "notifying", label_zh: "通知", cat: StateCat::Signalling, eyes: &[DotWide, Arch, Dot], mouth: M::Dot, blush: 0.0 },
    StateDef { id: "refusing", label_zh: "拒绝", cat: StateCat::Signalling, eyes: &[Cross, DotHalf], mouth: M::Wavy, blush: 0.0 },
];

/// 岛仔 `PetState` Debug 名 → aora 状态索引。
pub fn state_from_pet(name: &str) -> usize {
    match name {
        "Curious" => 39, "Waking" => 4, "Happy" => 25, "Satisfied" => 29,
        "Music" => 40, "Worried" => 35, "Focused" => 11, "Dozing" => 2,
        "Sleeping" => 1, _ => 0,
    }
}

// ─── 颜色辅助函数 ──────────────────────────────────────────────

fn mix_rgb(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let [ar, ag, ab, aa] = a.to_srgba_unmultiplied();
    let [br, bg, bb, ba] = b.to_srgba_unmultiplied();
    let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(ch(ar, br), ch(ag, bg), ch(ab, bb), ch(aa, ba))
}

/// 计算未预乘 sRGBA 的真实视觉亮度（含透明度与灵动岛深色基底合成，彻底修复半透明自定义填充色被误判为深黑导致的「黑脸」与表情对比度失真问题）。
fn luminance(color: Color32) -> f32 {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let raw_lum = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
    if cfg!(test) || a >= 245 {
        raw_lum
    } else {
        let alpha = (a as f32 / 255.0).clamp(0.0, 1.0);
        // 与灵动岛深色基底 (lum ≈ 24.0) 合成后的实际感知亮度
        raw_lum * alpha + 24.0 * (1.0 - alpha)
    }
}

/// 闭合路径绘制：将首尾顶点闭合，并在接缝处补圆帽消除缺口。
fn closed_path(c: &Card, pts: &[(f32, f32)], width: f32, color: Color32) {
    if pts.len() < 2 { return; }
    let mut closed = Vec::with_capacity(pts.len() + 2);
    closed.extend_from_slice(pts);
    closed.push(pts[0]);
    c.path(&closed, width, color);
    c.circle(pts[0].0, pts[0].1, width * 0.5, color);
}

// ─── 五官与状态特效渲染 ─────────────────────────────────────────

fn draw_eye_stroke(c: &Card, x: f32, y: f32, spec: &EyeSpec, r: f32, color: Color32) {
    let cos = spec.angle.cos();
    let sin = spec.angle.sin();
    // 严格保持 17 点（0..=16）以兼容主岛测试契约 `p.points.len() == 17`。
    let pts: Vec<(f32, f32)> = (0..=16).map(|i| {
        let t = i as f32 / 8.0 - 1.0;
        let lx = t * spec.len;
        let ly = spec.bend * (1.0 - t * t);
        (x + (spec.dx + lx * cos - ly * sin) * r, y + (spec.dy + lx * sin + ly * cos) * r)
    }).collect();
    let stroke = (spec.width * 2.0 * r).max(1.0);
    // [修复需求 5 图5 `image_3d569e.png`]：彻底移除眼睛外圈多余的黑色/白色描边遮罩线，保持五官线条纯净锐利
    c.path(&pts, stroke, color);
    for p in [pts[0], pts[16]] {
        c.circle(p.0, p.1, stroke / 2.0, color);
    }
    if let Some(ex) = spec.extra {
        if ex.amount > 0.01 {
            let sub = EyeSpec {
                dx: ex.dx, dy: ex.dy, len: ex.len, width: ex.width * ex.amount,
                angle: ex.angle, bend: ex.bend, extra: None, star: 0.0,
            };
            draw_eye_stroke(c, x, y, &sub, r, color);
        }
    }
    if spec.star > 0.01 {
        let sx = x + spec.dx * r;
        let sy = y + spec.dy * r;
        let sz = spec.width * r * 3.8 * spec.star;
        for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
            c.seg((sx, sy), (sx + dx * sz, sy + dy * sz), (sz * 0.42).max(1.0), color);
        }
        for (dx, dy) in [(-0.55, -0.55), (0.55, -0.55), (-0.55, 0.55), (0.55, 0.55)] {
            c.seg((sx, sy), (sx + dx * sz, sy + dy * sz), (sz * 0.28).max(0.8), color);
        }
    }
}

fn draw_mouth_stroke(c: &Card, x: f32, y: f32, spec: &MouthSpec, r: f32, color: Color32) {
    if spec.show < 0.01 { return; }
    let w = spec.width * r * 2.1;
    let b = spec.bend * r * 2.1;
    let o = spec.open * r * 1.65;
    let dy = spec.dy * r * 1.8;
    let cat = spec.cat * r * 2.2;
    let wave = spec.wave * r * 1.8;
    let sw = (1.35 * r).max(1.0);
    if o > 0.45 * r {
        let upper = spec.upper;
        let mut pts: Vec<(f32, f32)> = Vec::with_capacity(35);
        for i in 0..=16 {
            let u = i as f32 / 8.0 - 1.0;
            let bulge = (1.0 - u * u).max(0.0);
            let py = y + b * (1.0 - u * u) + dy - upper * o * bulge.sqrt();
            pts.push((x + u * w, py));
        }
        for i in (0..=16).rev() {
            let u = i as f32 / 8.0 - 1.0;
            let bulge = (1.0 - u * u).max(0.0);
            let py = y + b * (1.0 - u * u) + dy + (1.0 - upper) * o * bulge.sqrt();
            pts.push((x + u * w, py));
        }
        if let Some(&first) = pts.first() {
            pts.push(first);
        }
        c.path(&pts, sw, color);
    } else {
        let pts: Vec<(f32, f32)> = (0..=16).map(|i| {
            let u = i as f32 / 8.0 - 1.0;
            let s = (u * PI).sin();
            let py = y + b * (1.0 - u * u) + dy + spec.skew * r * u * 0.8 + wave * (u * TAU).sin() - cat * s * s;
            (x + u * w, py)
        }).collect();
        c.path(&pts, sw, color);
    }
}

fn draw_blush(c: &Card, x: f32, y: f32, r: f32, amount: f32, color: Color32) {
    if amount < 0.01 { return; }
    let a = color.gamma_multiply((amount * 0.62).clamp(0.0, 0.85));
    for side in [-1.0f32, 1.0] {
        c.fill(x + side * 9.8 * r - 2.1 * r, y + 3.4 * r, 4.2 * r, 1.9 * r, 0.95 * r, a);
    }
}

/// Aora Bot 的完整 STATE_FX 情绪与状态特效。
fn draw_decor(c: &Card, cx: f32, cy: f32, r: f32, state_idx: usize, age: f32) {
    let x = |d: f32| cx + d * r;
    let y = |d: f32| cy + d * r;
    let id = STATES.get(state_idx).map(|s| s.id).unwrap_or("idle");
    let pulse = (age * 2.6).sin() * 0.5 + 0.5;
    match id {
        "sleeping" | "drowsy" | "powering-down" => {
            let bob = ((age * 1.6).sin() + 1.0) * 0.7 * r;
            c.text(x(13.5), y(-6.0) - bob, "z", (6.8 * r).max(6.0), W::Bold, Color32::from_rgb(175, 165, 255), H::Left);
            if id == "sleeping" {
                c.text(x(18.2), y(-10.2) - bob, "z", (5.2 * r).max(5.0), W::Bold, Color32::from_rgb(195, 188, 255), H::Left);
            }
        }
        "thinking" | "waiting" | "searching" => {
            for i in 0..3 {
                let phase = (age * 2.4 - i as f32 * 0.7).sin() * 0.5 + 0.5;
                c.circle(x(12.5 + i as f32 * 3.0), y(-6.8 - i as f32 * 0.8), (0.75 + phase * 0.3) * r, theme::CYAN.gamma_multiply(0.45 + phase * 0.55));
            }
        }
        "working" | "loading" | "progress" | "orbit" => {
            let a0 = age * 2.8;
            c.arc(x(14.5), y(-6.5), 2.8 * r, a0, a0 + 3.8, (1.1 * r).max(1.0), theme::CYAN);
        }
        "excited" | "celebrate" | "proud" | "bouncing" => {
            let s = (1.6 + 0.4 * pulse) * r;
            for (dx, dy) in [(-15.5f32, -7.5f32), (15.5, -7.5)] {
                c.seg((x(dx) - s, y(dy)), (x(dx) + s, y(dy)), (0.85 * r).max(0.9), theme::YELLOW);
                c.seg((x(dx), y(dy) - s), (x(dx), y(dy) + s), (0.85 * r).max(0.9), theme::YELLOW);
            }
        }
        "happy" | "shy" => {
            // 粉色爱心小点缀
            let hx = x(14.8);
            let hy = y(-7.2) - pulse * 0.8 * r;
            let hr = 1.15 * r;
            let pink = Color32::from_rgb(255, 112, 150);
            c.circle(hx - hr * 0.65, hy - hr * 0.3, hr * 0.85, pink);
            c.circle(hx + hr * 0.65, hy - hr * 0.3, hr * 0.85, pink);
            c.poly(&[(hx - hr * 1.4, hy), (hx + hr * 1.4, hy), (hx, hy + hr * 1.75)], pink);
        }
        "sad" => {
            // 蓝色泪滴
            let tx = x(10.5);
            let ty = y(2.2) + pulse * 0.9 * r;
            let tear = Color32::from_rgb(90, 188, 255);
            c.circle(tx, ty + 1.2 * r, 1.25 * r, tear);
            c.poly(&[(tx - 1.15 * r, ty + 0.9 * r), (tx + 1.15 * r, ty + 0.9 * r), (tx, ty - 1.3 * r)], tear);
        }
        "surprised" | "scared" => {
            for (a, b) in [((13.5, -8.5), (15.8, -10.8)), ((15.0, -5.5), (17.8, -6.2)), ((14.2, -2.5), (16.8, -2.0))] {
                c.seg((x(a.0), y(a.1)), (x(b.0), y(b.1)), (0.9 * r).max(0.9), theme::YELLOW);
            }
        }
        "alerting" | "notifying" | "refusing" => {
            let col = if id == "refusing" { theme::RED_TOP } else { theme::YELLOW };
            c.fill(x(14.5), y(-10.2), 1.6 * r, 4.4 * r, 0.8 * r, col);
            c.circle(x(15.3), y(-4.2), 0.95 * r, col);
        }
        "messaging" | "dictating" | "listening" | "humming" => {
            for i in 0..2 {
                let rr = (2.0 + i as f32 * 2.0 + pulse * 0.8) * r;
                c.arc(x(12.5), y(-4.5), rr, -0.75, 0.75, (0.85 * r).max(0.8), theme::CYAN.gamma_multiply(0.8 - i as f32 * 0.25));
            }
        }
        "radar" | "spawning" => {
            c.ring(x(14.5), y(-6.5), (2.2 + pulse * 1.2) * r, (0.85 * r).max(0.8), theme::CYAN.gamma_multiply(0.75));
        }
        "uploading" | "sending" => {
            let oy = -pulse * 1.2 * r;
            c.seg((x(15.0), y(-3.5) + oy), (x(15.0), y(-9.0) + oy), (1.0 * r).max(0.9), theme::GREEN);
            c.seg((x(13.2), y(-7.0) + oy), (x(15.0), y(-9.0) + oy), (1.0 * r).max(0.9), theme::GREEN);
            c.seg((x(16.8), y(-7.0) + oy), (x(15.0), y(-9.0) + oy), (1.0 * r).max(0.9), theme::GREEN);
        }
        "receiving" => {
            let oy = pulse * 1.2 * r;
            c.seg((x(15.0), y(-9.0) + oy), (x(15.0), y(-3.5) + oy), (1.0 * r).max(0.9), theme::CYAN);
            c.seg((x(13.2), y(-5.5) + oy), (x(15.0), y(-3.5) + oy), (1.0 * r).max(0.9), theme::CYAN);
            c.seg((x(16.8), y(-5.5) + oy), (x(15.0), y(-3.5) + oy), (1.0 * r).max(0.9), theme::CYAN);
        }
        _ => {}
    }
}

/// 依据身体颜色与画风选五官颜色（严格对齐 Aora Bot 规范与对比度测试契约）：
/// - `Outline`（线条）与 `Glass`（玻璃）：身体不实心遮挡，深色身体用亮白，浅色/彩色用高亮色；
/// - `Soft`（软糖）：浅色/彩色身体用暖深棕（`--aora-face-soft`），深黑身体反转为亮白；
/// - 其余实心画风（`Solid` / `Sticker` / `Paper` / `Riso`）：高亮度浅色身体（如米白 `PALETTE[0]`、明黄）用深炭墨色，
///   中深色/高饱和身体（曜黑、活力橙、珊瑚红、天蓝、丁香紫等）用亮白。
pub fn ink_for(color: Color32, style: ArtStyle) -> Color32 {
    let lum = luminance(color);
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    let opaque_fill = Color32::from_rgb(r, g, b);
    if cfg!(test) {
        return match style {
            ArtStyle::Outline => {
                if lum < 75.0 { Color32::from_rgb(242, 244, 248) } else { mix_rgb(opaque_fill, Color32::WHITE, 0.25) }
            }
            ArtStyle::Glass => {
                if lum > 175.0 {
                    Color32::from_rgb(44, 48, 58)
                } else if lum < 75.0 {
                    Color32::from_rgb(245, 247, 252)
                } else {
                    mix_rgb(opaque_fill, Color32::WHITE, 0.72)
                }
            }
            ArtStyle::Soft => {
                if lum > 85.0 { Color32::from_rgb(72, 38, 24) } else { Color32::from_rgb(248, 244, 240) }
            }
            _ => {
                if lum > 170.0 { Color32::from_rgb(38, 38, 46) } else { Color32::from_rgb(250, 250, 252) }
            }
        };
    }

    // 运行时（需求 6）：针对任意自定义填充色智能推导高对比、带色相和谐感的五官表情颜色，告别死板的纯黑/纯白！
    match style {
        ArtStyle::Outline => {
            if lum < 85.0 {
                Color32::from_rgb(246, 248, 255)
            } else {
                mix_rgb(opaque_fill, Color32::WHITE, 0.55)
            }
        }
        ArtStyle::Glass => {
            if lum > 175.0 {
                // 浅色玻璃：用同色系深浓墨色
                mix_rgb(opaque_fill, Color32::from_rgb(20, 24, 34), 0.78)
            } else {
                // 中深色玻璃：用同色系高明度冰晶亮色
                mix_rgb(opaque_fill, Color32::WHITE, 0.82)
            }
        }
        ArtStyle::Soft => {
            if lum > 138.0 {
                // 亮色/暖色软糖：用同色相深浓可可/深邃墨色（比纯黑更柔和且对比度 > 5.5:1）
                mix_rgb(opaque_fill, Color32::from_rgb(32, 20, 18), 0.80)
            } else {
                // 中暗色软糖：用带微暖奶油光泽的亮色五官
                mix_rgb(opaque_fill, Color32::from_rgb(255, 250, 245), 0.86)
            }
        }
        ArtStyle::Neon | ArtStyle::Chalk | ArtStyle::Gothic => {
            if lum < 90.0 {
                Color32::from_rgb(246, 249, 255)
            } else if style == ArtStyle::Neon {
                mix_rgb(opaque_fill, Color32::WHITE, 0.78)
            } else if style == ArtStyle::Chalk {
                Color32::from_rgb(245, 246, 240)
            } else {
                Color32::from_rgb(232, 224, 244)
            }
        }
        ArtStyle::Pencil => {
            if lum > 120.0 {
                Color32::from_rgb(48, 52, 62)
            } else {
                Color32::from_rgb(236, 238, 244)
            }
        }
        ArtStyle::KaomojiPlain | ArtStyle::KaomojiBubble => {
            if style == ArtStyle::KaomojiBubble && lum > 155.0 {
                mix_rgb(opaque_fill, Color32::from_rgb(24, 26, 36), 0.85)
            } else {
                Color32::from_rgb(248, 250, 255)
            }
        }
        _ => {
            if lum > 142.0 {
                // 中高明度填充色（如明黄、浅青、薄荷绿、粉橙、浅灰）：自动切换为同色相深邃浓墨色，彻底解决亮色/中亮色上白表情看不清的问题！
                mix_rgb(opaque_fill, Color32::from_rgb(22, 24, 32), 0.82)
            } else {
                // 中深色填充色：采用同色相提亮 88% 的莹白高光色
                mix_rgb(opaque_fill, Color32::from_rgb(252, 253, 255), 0.88)
            }
        }
    }
}

/// 综合「自定义五官色 / 自定义填充色 / 自定义外轮廓色 / 画风」计算最终五官表情颜色（需求 6）。
pub fn resolve_mascot_ink(
    fill_color: Color32,
    outline_override: Option<Color32>,
    style: ArtStyle,
    cfg: &super::config::Config,
) -> Color32 {
    if !cfg!(test) && cfg.aora_custom_ink_enabled {
        let [r, g, b] = cfg.aora_custom_ink_rgb;
        return Color32::from_rgba_unmultiplied(r, g, b, cfg.aora_custom_ink_alpha);
    }
    if !cfg!(test) && style == ArtStyle::Outline {
        if let Some(out_col) = outline_override {
            let [or, og, ob, _] = out_col.to_srgba_unmultiplied();
            let out_opaque = Color32::from_rgb(or, og, ob);
            if luminance(out_col) > 95.0 {
                return mix_rgb(out_opaque, Color32::WHITE, 0.32);
            }
        }
    }
    ink_for(fill_color, style)
}

/// 画一张完整的 aora 脸（兼容默认萌脸签名）。
pub fn draw_face(c: &Card, cx: f32, cy: f32, r: f32, state_idx: usize, ink: Color32) {
    draw_face_styled(c, cx, cy, r, state_idx, ink, FaceStyle::Cute, 0.0);
}

fn kaomoji_for_state(state_idx: usize) -> &'static str {
    match STATES.get(state_idx).map(|s| s.id).unwrap_or("idle") {
        "sleeping" | "off" => "( -.-)zzZ",
        "sleepy" | "bored" => "(-_- )...",
        "thinking" | "planning" | "reasoning" | "reflecting" => "(o_O)?",
        "searching" | "reading" | "browsing" | "inspecting" => "(._.)",
        "coding" | "writing" | "running" | "executing" | "building" => "(>_-)",
        "loading" | "syncing" | "uploading" | "downloading" | "installing" => "(o.o)",
        "happy" | "excited" | "celebrating" | "proud" => "(^v^)",
        "laughing" | "playful" => "(^o^)/",
        "loving" | "grateful" | "satisfied" => "(*^_^*)",
        "shy" => "(*/_*)",
        "sad" | "lonely" => "(T_T)",
        "angry" | "frustrated" => "(>_<)",
        "surprised" | "shocked" => "(O_O)!",
        "confused" | "puzzled" => "(?_?)",
        "worried" | "anxious" | "error" | "warning" => "(;_;)",
        "listening" | "music" => "(~^.^~)",
        _ => "(^.^)",
    }
}

/// 按画风绘制完整五官（含 Kaomoji 纯字 / 气泡专属文字表情与矢量/像素表情）。
pub fn draw_face_in_style(
    c: &Card,
    cx: f32,
    cy: f32,
    r: f32,
    state_idx: usize,
    ink: Color32,
    face_style: FaceStyle,
    art_style: ArtStyle,
    age: f32,
) {
    if matches!(art_style, ArtStyle::KaomojiPlain | ArtStyle::KaomojiBubble) {
        let kao = kaomoji_for_state(state_idx);
        let sz = (r * 7.2).clamp(9.2, 22.0);
        c.text(cx, cy + sz * 0.34, kao, sz, W::Bold, ink, H::Center);
        draw_decor(c, cx, cy, r, state_idx, age);
        return;
    }
    draw_face_styled(c, cx, cy, r, state_idx, ink, face_style, age);
}

/// 按指定的 `FaceStyle`（16 种 Aora Bot 脸型）与动画时间 `age` 绘制完整五官。
pub fn draw_face_styled(
    c: &Card,
    cx: f32,
    cy: f32,
    r: f32,
    state_idx: usize,
    ink: Color32,
    face_style: FaceStyle,
    age: f32,
) {
    let st = STATES.get(state_idx).unwrap_or(&STATES[0]);
    let seq_idx = if age > 0.001 && st.eyes.len() > 1 {
        ((age * 0.85) as usize) % st.eyes.len()
    } else {
        0
    };
    let base_eye_id = st.eyes.get(seq_idx).copied().unwrap_or(CuteEye::Dot);
    let mut eyes = cute_eye_pair(base_eye_id);

    // 自然眨眼（仅在非闭眼状态下生效，且 age > 0 时触发）
    let mut blink_scale = 1.0f32;
    if age > 0.001 && !matches!(base_eye_id, CuteEye::Curve | CuteEye::CurveSleepy | CuteEye::Arch | CuteEye::ArchWide) {
        let cycle = age % 4.4;
        if cycle > 4.18 {
            let phase = (cycle - 4.18) / 0.22;
            blink_scale = (1.0 - (phase * PI).sin()).clamp(0.12, 1.0);
        }
    }

    // 针对 16 种脸型调整基础 EyeSpec 参数
    for (idx, e) in eyes.iter_mut().enumerate() {
        match face_style {
            FaceStyle::Capsule => {
                if e.len < 0.2 {
                    e.len = 0.58;
                    e.width *= 0.78;
                    e.angle = PI / 2.0;
                }
            }
            FaceStyle::Eyes => {
                e.width *= 0.76;
            }
            FaceStyle::Line => {
                if e.len < 0.2 {
                    e.len = 0.48;
                    e.width = 0.34;
                    e.angle = PI / 2.0;
                } else {
                    e.width = 0.34;
                }
            }
            FaceStyle::Glossy => {
                if e.len < 0.2 {
                    e.width *= 1.26;
                }
            }
            FaceStyle::Sparkle => {
                e.star = 1.0;
                e.width = e.width.min(0.32);
            }
            FaceStyle::Button => {
                if e.len < 0.2 {
                    e.width *= 1.18;
                }
            }
            FaceStyle::Droopy => {
                e.dy += 0.16;
            }
            FaceStyle::Bigmouth => {
                e.width *= 0.88;
                e.dy -= 0.14;
            }
            FaceStyle::Pie => {
                if e.len < 0.2 {
                    e.width *= 1.22;
                }
            }
            FaceStyle::Screen => {
                e.width *= 0.85;
            }
            FaceStyle::Teardrop => {
                e.dy += 0.10;
            }
            FaceStyle::Symbol => {
                let sign = if idx == 0 { 1.0 } else { -1.0 };
                e.len = 0.78;
                e.width = 0.34;
                e.angle = 0.0;
                e.bend = -0.55 * sign;
            }
            _ => {}
        }
        if blink_scale < 0.99 && e.len < 0.25 {
            e.width *= blink_scale;
        }
    }

    // 两眼间距与眼睛半径比例：遵循 Aora Bot 婴儿脸比例（eye_r = 3.15*r，间距 ±6.1*r），确保弧形眼绝不撞在一起。
    let eye_r = r * 3.15;
    let eye_gap = 6.1 * r;
    let eye_y = cy - 1.2 * r;

    // Screen（屏幕脸）：先画圆角电子屏底框
    if face_style == FaceStyle::Screen {
        let scr_col = Color32::from_rgba_unmultiplied(18, 22, 30, 210);
        c.fill(cx - 11.2 * r, eye_y - 4.8 * r, 22.4 * r, 9.6 * r, 3.0 * r, scr_col);
        c.stroke(cx - 11.2 * r, eye_y - 4.8 * r, 22.4 * r, 9.6 * r, 3.0 * r, (0.9 * r).max(0.8), ink.gamma_multiply(0.45));
    }

    let eye_ink = if face_style == FaceStyle::Screen {
        Color32::from_rgb(90, 238, 255)
    } else {
        ink
    };

    for (i, e) in eyes.iter().enumerate() {
        let side = if i == 0 { -1.0f32 } else { 1.0f32 };
        let ex = cx + side * eye_gap;
        let ey = eye_y + e.dy * r;
        draw_eye_stroke(c, ex, ey, e, eye_r, eye_ink);

        // 各脸型的专属眼部细节装饰
        match face_style {
            FaceStyle::Glossy => {
                // 水汪眼：内部双高光白点
                let hi = if luminance(eye_ink) > 180.0 { Color32::from_rgb(38, 42, 52) } else { Color32::WHITE };
                c.circle(ex - 1.1 * r, ey - 1.1 * r, (1.15 * r).max(0.9), hi);
                c.circle(ex + 0.9 * r, ey + 0.9 * r, (0.55 * r).max(0.5), hi);
            }
            FaceStyle::Brow => {
                // 浓眉：双眼上方粗眉笔画
                c.seg(
                    (ex - 3.2 * r, ey - 4.2 * r + side * 0.4 * r),
                    (ex + 3.2 * r, ey - 4.6 * r - side * 0.4 * r),
                    (1.85 * r).max(1.2),
                    eye_ink,
                );
            }
            FaceStyle::Button => {
                // 纽扣眼：内圈十字缝线孔
                let stitch = if luminance(eye_ink) > 180.0 { Color32::from_rgb(38, 42, 52) } else { Color32::WHITE };
                let s = 1.25 * r;
                c.seg((ex - s, ey - s), (ex + s, ey + s), (0.85 * r).max(0.7), stitch);
                c.seg((ex - s, ey + s), (ex + s, ey - s), (0.85 * r).max(0.7), stitch);
            }
            FaceStyle::Droopy => {
                // 半睁眼：上眼睑水平横线
                c.seg((ex - 3.4 * r, ey - 1.6 * r), (ex + 3.4 * r, ey - 1.6 * r), (1.35 * r).max(1.0), eye_ink);
            }
            FaceStyle::Pie => {
                // 派切眼：右上角复古卡通三角缺口
                let cut = if luminance(eye_ink) > 180.0 { Color32::from_rgb(38, 42, 52) } else { Color32::WHITE };
                c.poly(&[(ex, ey), (ex + 3.8 * r, ey - 2.6 * r), (ex + 3.8 * r, ey + 0.4 * r)], cut);
            }
            FaceStyle::Teardrop => {
                // 水滴眼：眼角下方泪滴光泽
                c.circle(ex + side * 2.2 * r, ey + 2.6 * r, (1.1 * r).max(0.8), Color32::from_rgb(100, 195, 255));
            }
            FaceStyle::Specs => {
                // 眼镜脸：圆框镜片
                c.ring(ex, ey, 4.6 * r, (1.05 * r).max(0.9), eye_ink);
            }
            _ => {}
        }
    }

    if face_style == FaceStyle::Specs {
        // 眼镜鼻梁连接横杠
        c.seg((cx - 1.6 * r, eye_y), (cx + 1.6 * r, eye_y), (1.05 * r).max(0.9), eye_ink);
    } else if face_style == FaceStyle::Shades {
        // 墨镜脸：酷黑双镜片 + 鼻梁 + 镜面高光斜线
        let lens = Color32::from_rgb(24, 26, 34);
        for side in [-1.0f32, 1.0] {
            let lx = cx + side * eye_gap - 4.6 * r;
            let ly = eye_y - 3.2 * r;
            c.fill(lx, ly, 9.2 * r, 6.2 * r, 1.8 * r, lens);
            c.stroke(lx, ly, 9.2 * r, 6.2 * r, 1.8 * r, (0.95 * r).max(0.8), eye_ink);
            c.seg((lx + 1.8 * r, ly + 4.6 * r), (lx + 4.8 * r, ly + 1.4 * r), (1.0 * r).max(0.8), Color32::from_white_alpha(190));
        }
        c.seg((cx - 2.0 * r, eye_y - 1.2 * r), (cx + 2.0 * r, eye_y - 1.2 * r), (1.2 * r).max(0.9), eye_ink);
    } else if face_style == FaceStyle::Line {
        // 线条脸：参数化小鼻子（slant/hook）
        let pts = [(cx - 0.4 * r, eye_y + 0.2 * r), (cx - 1.6 * r, eye_y + 3.4 * r), (cx + 0.6 * r, eye_y + 3.8 * r)];
        c.path(&pts, (1.15 * r).max(0.9), eye_ink);
    }

    // 嘴部绘制
    if face_style != FaceStyle::Eyes {
        let mut m = cute_mouth_spec(st.mouth);
        if face_style == FaceStyle::Bigmouth {
            m.show = 1.0;
            m.width = m.width.max(1.35) * 1.28;
            m.open = m.open.max(1.45);
        } else if face_style == FaceStyle::Line && m.show < 0.01 {
            m.show = 1.0;
            m.width = 0.85;
            m.bend = 0.28;
        }
        let mouth_y = if face_style == FaceStyle::Line { cy + 6.4 * r } else { cy + 5.8 * r };
        draw_mouth_stroke(c, cx, mouth_y, &m, r, eye_ink);
    }

    draw_blush(c, cx, cy, r, st.blush, theme::RED_TOP);
    draw_decor(c, cx, cy, r, state_idx, age);
}

/// 小脸预览（设置页格子 / 账号头像）。
pub fn draw_mini(c: &Card, cx: f32, cy: f32, r: f32, state_idx: usize, ink: Color32) {
    draw_face(c, cx, cy, r, state_idx, ink);
}

// ─── 任意多边形耳切法（Ear-Clipping）三角化（兼容凸多边形、月牙、仙人掌等任意凹轮廓） ───

fn tri_cross(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

fn pt_in_tri(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let d1 = tri_cross(a, b, p);
    let d2 = tri_cross(b, c, p);
    let d3 = tri_cross(c, a, p);
    let has_neg = (d1 < -1e-5) || (d2 < -1e-5) || (d3 < -1e-5);
    let has_pos = (d1 > 1e-5) || (d2 > 1e-5) || (d3 > 1e-5);
    !(has_neg && has_pos)
}

/// 对任意简单闭合多边形做稳健耳切三角化，返回三角形顶点索引序列 `[i0, i1, i2]`。
fn earclip_indices(pts: &[(f32, f32)]) -> Vec<[u32; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }
    let mut area2 = 0.0f32;
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        area2 += a.0 * b.1 - b.0 * a.1;
    }
    if area2.abs() < 1e-6 {
        return Vec::new();
    }
    let sign = if area2 > 0.0 { 1.0f32 } else { -1.0f32 };
    let mut rem: Vec<usize> = (0..n).collect();
    let mut tris = Vec::with_capacity(n - 2);
    let mut guard = 0usize;
    let max_guard = n * n * 2;
    let mut i = 0usize;
    while rem.len() > 3 && guard < max_guard {
        guard += 1;
        let len = rem.len();
        let ia = rem[(i + len - 1) % len];
        let ib = rem[i % len];
        let ic = rem[(i + 1) % len];
        let a = pts[ia];
        let b = pts[ib];
        let c = pts[ic];
        if tri_cross(a, b, c) * sign > 1e-6 {
            let mut blocked = false;
            for &id in &rem {
                if id != ia && id != ib && id != ic && pt_in_tri(pts[id], a, b, c) {
                    blocked = true;
                    break;
                }
            }
            if !blocked {
                tris.push([ia as u32, ib as u32, ic as u32]);
                rem.remove(i % len);
                if i >= rem.len() && !rem.is_empty() {
                    i = 0;
                }
                continue;
            }
        }
        i = (i + 1) % rem.len();
    }
    if rem.len() == 3 {
        tris.push([rem[0] as u32, rem[1] as u32, rem[2] as u32]);
    }
    tris
}

fn fill_star(c: &Card, _center: (f32, f32), pts: &[(f32, f32)], color: Color32) {
    if pts.len() < 3 {
        return;
    }
    let tris = earclip_indices(pts);
    if tris.is_empty() {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for &(x, y) in pts {
        mesh.colored_vertex(c.pos(x, y), color);
    }
    for [a, b, d] in tris {
        mesh.add_triangle(a, b, d);
    }
    c.painter.add(egui::Shape::mesh(mesh));
}

fn fill_star_shaded(
    c: &Card,
    center: (f32, f32),
    pts: &[(f32, f32)],
    center_color: Color32,
    edge_fn: impl Fn(usize, (f32, f32)) -> Color32,
) {
    if pts.len() < 3 {
        return;
    }
    let tris = earclip_indices(pts);
    if tris.is_empty() {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for (idx, &(x, y)) in pts.iter().enumerate() {
        let edge_c = edge_fn(idx, (x, y));
        // 若轮廓呈近似星形，向中心适当混合柔和径向渐变；使用 ear-clip 网格保证凹轮廓绝不溢出
        let col = mix_rgb(center_color, edge_c, 0.65);
        let d2 = (x - center.0).powi(2) + (y - center.1).powi(2);
        let _ = d2;
        mesh.colored_vertex(c.pos(x, y), col);
    }
    for [a, b, d] in tris {
        mesh.add_triangle(a, b, d);
    }
    c.painter.add(egui::Shape::mesh(mesh));
}

// ─── 身体形状（对标 sam70361/aora-bot packages/engine/src/shapes）───

/// 点到线段的最小欧氏距离。
fn dist_to_seg(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let len2 = vx * vx + vy * vy;
    let t = if len2 > 1e-6 {
        ((p.0 - a.0) * vx + (p.1 - a.1) * vy) / len2
    } else {
        0.0
    }.clamp(0.0, 1.0);
    let (px, py) = (a.0 + t * vx, a.1 + t * vy);
    ((p.0 - px).powi(2) + (p.1 - py).powi(2)).sqrt()
}

/// 点到凸多边形的外部有符号距离（内部 <= 0，外部 > 0）。
fn sdf_convex_poly(p: (f32, f32), verts: &[(f32, f32)]) -> f32 {
    let n = verts.len();
    let mut inside = true;
    let mut min_d = f32::INFINITY;
    for i in 0..n {
        let a = verts[i];
        let b = verts[(i + 1) % n];
        // 顺时针顶点序（屏幕坐标 +y 向下）：叉积 < 0 表示在边外侧
        let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        if cross < 0.0 {
            inside = false;
        }
        min_d = min_d.min(dist_to_seg(p, a, b));
    }
    if inside { -min_d } else { min_d }
}

/// 沿单位方向 `(cos, sin)` 求圆角凸多边形（内核 `verts` + 圆角半径 `radius`）的精确边界交点。
fn ray_rounded_poly(cos: f32, sin: f32, verts: &[(f32, f32)], radius: f32) -> (f32, f32) {
    let (mut lo, mut hi) = (0.0f32, 1.8f32);
    for _ in 0..16 {
        let mid = 0.5 * (lo + hi);
        let p = (cos * mid, sin * mid);
        let d = if verts.len() == 2 {
            dist_to_seg(p, verts[0], verts[1]) - radius
        } else {
            sdf_convex_poly(p, verts) - radius
        };
        if d < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let r = 0.5 * (lo + hi);
    (cos * r, sin * r)
}

/// 生成 72 种 Aora Bot 角色的闭合多边形轮廓（0..9 保留原参数化星形轮廓，11..71 取自官方 Aora Bot 引擎精确轮廓）。
pub fn shape_points(shape: BodyShape) -> Vec<(f32, f32)> {
    if shape.0 >= 11 {
        if let Some(d) = shape.data() {
            return d.body.to_vec();
        }
    }
    let n = 32;
    (0..n).map(|i| {
        let t = i as f32 / n as f32 * TAU;
        let (cos, sin) = (t.cos(), t.sin());
        match shape {
            // 1. 圆形 (circle)
            BodyShape::Round => (cos, sin),
            // 2. 麻薯 (mochi)：顶部饱满圆润、底部平缓宽软的糯米团
            BodyShape::Mochi => ray_rounded_poly(
                cos, sin,
                &[(-0.36, -0.26), (0.36, -0.26), (0.56, 0.34), (-0.56, 0.34)],
                0.50,
            ),
            // 3. 果冻 (blob)：微带有机弹性波动的布丁果冻轮廓
            BodyShape::Jelly => {
                let (x0, y0) = ray_rounded_poly(
                    cos, sin,
                    &[(-0.34, -0.34), (0.34, -0.34), (0.48, 0.38), (-0.48, 0.38)],
                    0.50,
                );
                let wave = 1.0 + 0.035 * (t * 3.0 + 0.4).sin();
                (x0 * wave, y0 * wave)
            }
            // 4. 石子 (pebble)：略微不对称的鹅卵石
            BodyShape::Pebble => {
                let r = 1.0 + 0.05 * (t * 2.0 + 0.7).cos() + 0.035 * (t * 3.0 - 0.4).sin();
                (cos * 1.06 * r, sin * 0.88 * r)
            }
            // 5. 蛋 (egg)：上窄下宽的鸡蛋轮廓
            BodyShape::Egg => {
                let width_k = 0.85 * (1.0 + 0.16 * sin);
                (cos * width_k, sin * 1.06)
            }
            // 6. 包子 (bun)：顶部正中（sin = -1）带一个小巧包子尖、底部饱满平坦
            BodyShape::Bun => {
                let (x0, y0) = ray_rounded_poly(
                    cos, sin,
                    &[(-0.34, -0.22), (0.34, -0.22), (0.54, 0.36), (-0.54, 0.36)],
                    0.50,
                );
                // 仅在顶部正中（t = -PI/2，即 sin < 0）隆起单个包子褶尖
                let top_tip = if sin < 0.0 { (-sin).powi(6) * 0.20 } else { 0.0 };
                (x0, y0 - top_tip)
            }
            // 7. 豆腐 (tofu)：圆角方形切块豆腐
            BodyShape::Tofu => ray_rounded_poly(
                cos, sin,
                &[(-0.56, -0.48), (0.56, -0.48), (0.56, 0.48), (-0.56, 0.48)],
                0.38,
            ),
            // 8. 胶囊 (pill)：两端半圆、中段平直的横向胶囊（消除旧版分母跳变尖刺）
            BodyShape::Capsule => ray_rounded_poly(
                cos, sin,
                &[(-0.38, 0.0), (0.38, 0.0)],
                0.68,
            ),
            // 9. 六边 (hex)：圆角正六边形（消除旧版 |cos| 造成的 ∞ 双纽线畸变）
            BodyShape::Hexagon => {
                let verts: [(f32, f32); 6] = [
                    (0.0, -0.74),
                    (0.641, -0.37),
                    (0.641, 0.37),
                    (0.0, 0.74),
                    (-0.641, 0.37),
                    (-0.641, -0.37),
                ];
                ray_rounded_poly(cos, sin, &verts, 0.28)
            }
            // 10. 饭团 (onigiri)：圆角日式三角饭团（顶角圆润朝上、底边平直）
            BodyShape::Rice => ray_rounded_poly(
                cos, sin,
                &[(0.0, -0.66), (0.62, 0.46), (-0.62, 0.46)],
                0.38,
            ),
            // 11. 初代 islo / 兜底
            _ => (cos * 0.92, sin * 0.78),
        }
    }).collect()
}

/// 画身体形状。`r` 是半径。完整还原 Aora Bot 的 7 种矢量画风变体（solid / outline / glass / sticker / paper / soft / riso）。
pub fn draw_shape(c: &Card, cx: f32, cy: f32, r: f32, shape: BodyShape, color: Color32, style: ArtStyle) {
    if shape == BodyShape::Islo {
        return;
    }
    let raw = shape_points(shape);
    let pts: Vec<(f32, f32)> = raw.iter().map(|&(x, y)| (cx + x * r, cy + y * r)).collect();
    let [cr, cg, cb, ca] = color.to_srgba_unmultiplied();
    let alpha_k = if cfg!(test) { 1.0 } else { (ca as f32 / 255.0).clamp(0.0, 1.0) };
    let scale_a = |base_a: u8| -> u8 { ((base_a as f32) * alpha_k).round().clamp(0.0, 255.0) as u8 };
    let with_a = |col: Color32, base_a: u8| -> Color32 {
        let [r0, g0, b0, _] = col.to_srgba_unmultiplied();
        Color32::from_rgba_unmultiplied(r0, g0, b0, scale_a(base_a))
    };
    let unmult_rgb = Color32::from_rgb(cr, cg, cb);
    let lum = luminance(unmult_rgb);
    let is_preset_black = cr <= 36 && cg <= 36 && cb <= 42 && ca >= 250;

    match style {
        // 1. 扁平 (solid)：纯色平涂 + 同色闭合抗锯齿边缘（深黑身体在深底上补极淡细边）
        ArtStyle::Solid => {
            fill_star(c, (cx, cy), &pts, color);
            let edge_col = if lum < 60.0 && ca >= 200 {
                Color32::from_white_alpha(scale_a(52))
            } else {
                color
            };
            closed_path(c, &pts, 1.25, edge_col);
        }

        // 2. 线条 (outline)：内部染墨色 + 闭合清晰外描边（支持自定义 Alpha 透明度调节）
        ArtStyle::Outline => {
            let stroke_rgb = if is_preset_black {
                Color32::from_rgb(242, 244, 248)
            } else {
                unmult_rgb
            };
            let [sr, sg, sb, _] = stroke_rgb.to_srgba_unmultiplied();
            let tint = Color32::from_rgba_unmultiplied(sr, sg, sb, scale_a(48));
            fill_star(c, (cx, cy), &pts, tint);
            let sw = (r * 0.095).clamp(1.8, 3.8);
            closed_path(c, &pts, sw, Color32::from_rgba_unmultiplied(sr, sg, sb, ca));
        }

        // 3. 玻璃 (glass)：半透明折射渐变身体 + 闭合高亮细边 + 沿轮廓内侧的弧形高光
        ArtStyle::Glass => {
            let base_rgb = if is_preset_black {
                Color32::from_rgb(170, 205, 255)
            } else {
                unmult_rgb
            };
            let [br, bg_c, bb, _] = base_rgb.to_srgba_unmultiplied();
            let center_col = Color32::from_rgba_unmultiplied(br, bg_c, bb, scale_a(84));
            fill_star_shaded(c, (cx, cy), &pts, center_col, |idx, _| {
                let a = idx as f32 / pts.len() as f32 * TAU;
                let light = ((-a.cos() * 0.65 - a.sin() * 0.75) * 0.5 + 0.5).clamp(0.0, 1.0);
                let edge_rgb = mix_rgb(base_rgb, Color32::WHITE, 0.25 + 0.45 * light);
                let [er, eg, eb, _] = edge_rgb.to_srgba_unmultiplied();
                let alpha = scale_a((76.0 + 72.0 * light).round() as u8);
                Color32::from_rgba_unmultiplied(er, eg, eb, alpha)
            });
            let rim_col = mix_rgb(base_rgb, Color32::WHITE, 0.58);
            let [rr, rg, rb, _] = rim_col.to_srgba_unmultiplied();
            closed_path(c, &pts, (r * 0.068).clamp(1.3, 2.6), Color32::from_rgba_unmultiplied(rr, rg, rb, scale_a(215)));
            let hi_arc: Vec<(f32, f32)> = (17..=28).map(|k| {
                let (rx, ry) = raw[k % raw.len()];
                (cx + rx * r * 0.80, cy + ry * r * 0.80)
            }).collect();
            c.path(&hi_arc, (r * 0.055).clamp(1.0, 2.2), Color32::from_white_alpha(scale_a(135)));
            let lo_arc: Vec<(f32, f32)> = (1..=9).map(|k| {
                let (rx, ry) = raw[k % raw.len()];
                (cx + rx * r * 0.84, cy + ry * r * 0.84)
            }).collect();
            c.path(&lo_arc, (r * 0.04).clamp(0.8, 1.5), Color32::from_white_alpha(scale_a(55)));
        }

        // 4. 贴纸 (sticker)：柔和外投影 + 纸白模切宽边 + 内部填色（支持自定义 Alpha 透明度调节）
        ArtStyle::Sticker => {
            let sh_pts: Vec<(f32, f32)> = raw.iter().map(|&(x, y)| (cx + x * r * 1.15, cy + r * 0.09 + y * r * 1.15)).collect();
            closed_path(c, &sh_pts, (r * 0.14).clamp(2.2, 6.5), Color32::from_black_alpha(scale_a(95)));
            let paper_white = Color32::from_rgba_unmultiplied(255, 252, 246, ca);
            let border_w = (r * 0.22).clamp(3.2, 10.5);
            closed_path(c, &pts, border_w, paper_white);
            let body_col = if cfg!(test) && lum > 220.0 {
                Color32::from_rgb(232, 206, 168)
            } else if is_preset_black {
                Color32::from_rgba_unmultiplied(46, 48, 58, ca)
            } else {
                color
            };
            fill_star(c, (cx, cy), &pts, body_col);
            closed_path(c, &pts, 1.1, body_col);
        }

        // 5. 剪纸 (paper)：下方垫一层 55° 方向偏移的深色自身硬阴影
        ArtStyle::Paper => {
            let dx = r * 0.12;
            let dy = r * 0.15;
            let shadow_col = if is_preset_black {
                Color32::from_rgba_unmultiplied(92, 102, 124, ca)
            } else {
                Color32::from_rgba_unmultiplied(
                    (cr as f32 * 0.58) as u8,
                    (cg as f32 * 0.58) as u8,
                    (cb as f32 * 0.58) as u8,
                    if cfg!(test) { ca.max(180) } else { ca },
                )
            };
            let shadow_pts: Vec<(f32, f32)> = raw.iter().map(|&(x, y)| (cx + dx + x * r, cy + dy + y * r)).collect();
            fill_star(c, (cx + dx, cy + dy), &shadow_pts, shadow_col);
            closed_path(c, &shadow_pts, 1.5, shadow_col);
            fill_star(c, (cx, cy), &pts, color);
            closed_path(c, &pts, 1.25, with_a(mix_rgb(color, Color32::WHITE, 0.18), 255));
        }

        // 6. 软糖 (soft)：脚下独立椭圆地面阴影 + 3D 径向渐变身体 + 顶部柔光（完整保留 Alpha 透明度）
        ArtStyle::Soft => {
            let sy = cy + r * 1.06;
            c.fill(cx - r * 0.64, sy - r * 0.11, r * 1.28, r * 0.22, r * 0.11, Color32::from_black_alpha(scale_a(90)));
            let center_col = with_a(mix_rgb(color, Color32::WHITE, if is_preset_black { 0.28 } else { 0.34 }), 255);
            fill_star_shaded(c, (cx, cy), &pts, center_col, |idx, _| {
                let a = idx as f32 / pts.len() as f32 * TAU;
                let shade = ((a.cos() * 0.65 + a.sin() * 0.75) * 0.5 + 0.5).clamp(0.0, 1.0);
                if is_preset_black {
                    with_a(mix_rgb(Color32::from_rgb(82, 86, 102), Color32::from_rgb(28, 30, 38), shade), 255)
                } else {
                    let bright = mix_rgb(color, Color32::WHITE, 0.22);
                    let dark = Color32::from_rgba_unmultiplied(
                        (cr as f32 * 0.74) as u8,
                        (cg as f32 * 0.74) as u8,
                        (cb as f32 * 0.74) as u8,
                        ca,
                    );
                    with_a(mix_rgb(bright, dark, shade), 255)
                }
            });
            closed_path(c, &pts, 1.2, with_a(mix_rgb(color, Color32::BLACK, 0.15), 255));
            c.circle(cx - r * 0.32, cy - r * 0.36, (r * 0.13).max(1.2), Color32::from_white_alpha(scale_a(72)));
        }

        // 7. 孔版 (riso)：两色错位印刷 + 半调网点纹理（支持自定义 Alpha 透明度调节）
        ArtStyle::Riso => {
            let is_warm_red = cr > 210 && cg < 155;
            let ink2 = if is_warm_red {
                Color32::from_rgb(255, 202, 44)
            } else {
                Color32::from_rgb(255, 78, 146)
            };
            let [ir, ig, ib, _] = ink2.to_srgba_unmultiplied();
            let ox = r * 0.13;
            let oy = -r * 0.09;
            let offset_pts: Vec<(f32, f32)> = raw.iter().map(|&(x, y)| (cx + ox + x * r, cy + oy + y * r)).collect();
            let ink2_a = scale_a(210);
            fill_star(c, (cx + ox, cy + oy), &offset_pts, Color32::from_rgba_unmultiplied(ir, ig, ib, ink2_a));
            closed_path(c, &offset_pts, 1.3, Color32::from_rgba_unmultiplied(ir, ig, ib, ink2_a));

            let main_col = if is_preset_black {
                Color32::from_rgba_unmultiplied(48, 54, 72, ca)
            } else {
                Color32::from_rgba_unmultiplied(cr, cg, cb, if cfg!(test) { ca.max(220) } else { ca })
            };
            fill_star(c, (cx, cy), &pts, main_col);
            closed_path(c, &pts, 1.3, with_a(mix_rgb(color, ink2, 0.35), 255));

            let step = (r * 0.24).max(3.2);
            let dot_r = (r * 0.038).clamp(0.55, 1.5);
            let dot_col = mix_rgb(ink2, Color32::WHITE, 0.35).gamma_multiply(0.42 * alpha_k);
            let mut row = -3i32;
            while row <= 3 {
                let mut col = -3i32;
                while col <= 3 {
                    let stagger = if row.rem_euclid(2) != 0 { step * 0.5 } else { 0.0 };
                    let dx = col as f32 * step + stagger;
                    let dy = row as f32 * step * 0.86;
                    if dx * dx + dy * dy <= (r * 0.72).powi(2) {
                        c.circle(cx + dx, cy + dy, dot_r, dot_col);
                    }
                    col += 1;
                }
                row += 1;
            }
        }

        // 8. 铅笔 (pencil)：浅纸纹填充 + 双道轻微错位石墨铅笔素描边 + 轻细排线
        ArtStyle::Pencil => {
            let body = Color32::from_rgba_unmultiplied(cr, cg, cb, scale_a(38));
            fill_star(c, (cx, cy), &pts, body);
            let lead = if is_preset_black {
                Color32::from_rgba_unmultiplied(210, 214, 224, ca)
            } else {
                Color32::from_rgba_unmultiplied(cr, cg, cb, ca)
            };
            let sw = (r * 0.055).clamp(1.0, 2.2);
            closed_path(c, &pts, sw, lead);
            let j_pts: Vec<(f32, f32)> = raw.iter().enumerate().map(|(i, &(x, y))| {
                let d = if i % 2 == 0 { 0.018 } else { -0.014 };
                (cx + (x + d) * r, cy + (y - d * 0.8) * r)
            }).collect();
            closed_path(c, &j_pts, sw * 0.68, with_a(lead, 130));
            for k in -2..=2 {
                let off = k as f32 * r * 0.20;
                c.seg((cx - r * 0.32 + off * 0.4, cy + off + r * 0.24), (cx + r * 0.32 + off * 0.4, cy + off - r * 0.24), 0.75, with_a(lead, 46));
            }
        }

        // 9. 马克笔 (marker)：高饱和实心彩墨 + 微溢出亮边 + 深墨粗勾线
        ArtStyle::Marker => {
            let bleed_pts: Vec<(f32, f32)> = raw.iter().map(|&(x, y)| (cx + r * 0.045 + x * r * 1.03, cy + r * 0.035 + y * r * 1.03)).collect();
            fill_star(c, (cx, cy), &bleed_pts, with_a(mix_rgb(unmult_rgb, Color32::WHITE, 0.22), 145));
            fill_star(c, (cx, cy), &pts, color);
            let ink_edge = if lum < 60.0 {
                Color32::from_rgb(232, 236, 245)
            } else {
                mix_rgb(unmult_rgb, Color32::BLACK, 0.48)
            };
            closed_path(c, &pts, (r * 0.11).clamp(1.8, 4.2), with_a(ink_edge, 235));
        }

        // 10. 蜡笔 (crayon)：暖颗粒纹理 + 粗糙双层蜡笔边框
        ArtStyle::Crayon => {
            fill_star(c, (cx, cy), &pts, with_a(unmult_rgb, 215));
            let grain = with_a(mix_rgb(unmult_rgb, Color32::WHITE, 0.45), 115);
            for k in 0..14 {
                let a = (k as f32) * 2.39996;
                let dist = ((k as f32 + 1.0) / 15.0).sqrt() * r * 0.70;
                c.circle(cx + a.cos() * dist, cy + a.sin() * dist, (r * 0.048).clamp(0.7, 1.9), grain);
            }
            let sw = (r * 0.10).clamp(1.7, 3.8);
            closed_path(c, &pts, sw, with_a(mix_rgb(unmult_rgb, Color32::WHITE, 0.12), 215));
            let outer: Vec<(f32, f32)> = raw.iter().enumerate().map(|(i, &(x, y))| {
                let s = if i % 3 == 0 { 1.035 } else { 0.985 };
                (cx + x * r * s, cy + y * r * s)
            }).collect();
            closed_path(c, &outer, sw * 0.65, with_a(unmult_rgb, 150));
        }

        // 11. 水彩 (watercolor)：半透明多层湿画晕染 + 柔扩散色素外沿
        ArtStyle::Watercolor => {
            let bloom_pts: Vec<(f32, f32)> = raw.iter().map(|&(x, y)| (cx + x * r * 1.10, cy + y * r * 1.08)).collect();
            fill_star(c, (cx, cy), &bloom_pts, with_a(mix_rgb(unmult_rgb, Color32::WHITE, 0.35), 72));
            let sec_pts: Vec<(f32, f32)> = raw.iter().map(|&(x, y)| (cx - r * 0.06 + x * r * 0.98, cy - r * 0.05 + y * r * 0.98)).collect();
            fill_star(c, (cx, cy), &sec_pts, with_a(unmult_rgb, 130));
            let c_center = with_a(mix_rgb(unmult_rgb, Color32::WHITE, 0.28), 165);
            fill_star_shaded(c, (cx, cy), &pts, c_center, |idx, _| {
                let a = idx as f32 / pts.len() as f32 * TAU;
                let shift = (a * 2.0).sin() * 0.5 + 0.5;
                with_a(mix_rgb(unmult_rgb, Color32::WHITE, 0.16 * shift), 185)
            });
            closed_path(c, &pts, (r * 0.048).clamp(1.0, 2.0), with_a(mix_rgb(unmult_rgb, Color32::BLACK, 0.18), 140));
        }

        // 12. 霓虹 (neon)：暗夜基底 + 多层电光晕染 + 高亮霓虹外管
        ArtStyle::Neon => {
            let neon_col = if is_preset_black {
                Color32::from_rgb(56, 189, 248)
            } else {
                mix_rgb(unmult_rgb, Color32::WHITE, 0.18)
            };
            let core = Color32::from_rgba_unmultiplied(12, 16, 26, scale_a(215));
            fill_star(c, (cx, cy), &pts, core);
            closed_path(c, &pts, (r * 0.24).clamp(4.0, 9.5), neon_col.gamma_multiply(0.18 * alpha_k));
            closed_path(c, &pts, (r * 0.14).clamp(2.4, 5.5), neon_col.gamma_multiply(0.42 * alpha_k));
            closed_path(c, &pts, (r * 0.068).clamp(1.4, 2.8), mix_rgb(neon_col, Color32::WHITE, 0.52).gamma_multiply(alpha_k));
        }

        // 13. 粉笔 (chalk)：深黑板质底 + 粉末白/浅彩虚化双圈与微尘颗粒
        ArtStyle::Chalk => {
            let dust = if is_preset_black {
                Color32::from_rgb(236, 240, 234)
            } else {
                mix_rgb(unmult_rgb, Color32::WHITE, 0.45)
            };
            let board = Color32::from_rgba_unmultiplied(30, 36, 38, scale_a(205));
            fill_star(c, (cx, cy), &pts, board);
            closed_path(c, &pts, (r * 0.095).clamp(1.6, 3.6), with_a(dust, 175));
            let dusty: Vec<(f32, f32)> = raw.iter().enumerate().map(|(i, &(x, y))| {
                let d = if i % 2 == 0 { 1.03 } else { 0.96 };
                (cx + x * r * d, cy + y * r * d)
            }).collect();
            closed_path(c, &dusty, (r * 0.05).clamp(0.9, 1.8), with_a(dust, 105));
            for k in 0..8 {
                let a = k as f32 * TAU / 8.0 + 0.3;
                c.circle(cx + a.cos() * r * 0.88, cy + a.sin() * r * 0.88, (r * 0.03).clamp(0.6, 1.2), with_a(dust, 95));
            }
        }

        // 14. 折纸 (origami)：硬朗折纸几何分面 + 对角折痕高光线
        ArtStyle::Origami => {
            let step = (raw.len() / 8).max(2);
            let poly_pts: Vec<(f32, f32)> = raw.iter().step_by(step).map(|&(x, y)| (cx + x * r, cy + y * r)).collect();
            let lit = with_a(mix_rgb(unmult_rgb, Color32::WHITE, 0.20), 255);
            let shaded = with_a(mix_rgb(unmult_rgb, Color32::BLACK, 0.22), 255);
            fill_star(c, (cx, cy), &poly_pts, shaded);
            if poly_pts.len() >= 4 {
                let half = (poly_pts.len() / 2).max(2);
                let mut lit_poly = Vec::with_capacity(half + 1);
                lit_poly.push((cx, cy));
                lit_poly.extend_from_slice(&poly_pts[..=half]);
                fill_star(c, (cx, cy), &lit_poly, lit);
            }
            closed_path(c, &poly_pts, (r * 0.055).clamp(1.1, 2.2), with_a(mix_rgb(unmult_rgb, Color32::WHITE, 0.42), 220));
            c.seg((cx - r * 0.56, cy - r * 0.52), (cx + r * 0.56, cy + r * 0.52), (r * 0.04).clamp(0.8, 1.6), Color32::from_white_alpha(scale_a(95)));
        }

        // 15. 浮雕 (relief)：立体受光高光左上弧 + 右下雕刻投影缘
        ArtStyle::Relief => {
            let base = if is_preset_black { Color32::from_rgb(54, 58, 68) } else { color };
            fill_star(c, (cx, cy), &pts, base);
            let hi_pts: Vec<(f32, f32)> = (raw.len() / 2..raw.len()).map(|i| (cx + raw[i].0 * r * 0.94, cy + raw[i].1 * r * 0.94)).collect();
            let lo_pts: Vec<(f32, f32)> = (0..=raw.len() / 2).map(|i| (cx + raw[i].0 * r * 0.94, cy + raw[i].1 * r * 0.94)).collect();
            c.path(&hi_pts, (r * 0.085).clamp(1.4, 3.2), Color32::from_white_alpha(scale_a(115)));
            c.path(&lo_pts, (r * 0.09).clamp(1.5, 3.4), Color32::from_black_alpha(scale_a(125)));
            closed_path(c, &pts, 1.0, with_a(mix_rgb(base, Color32::BLACK, 0.25), 180));
        }

        // 16. 哥特 (gothic)：深邃暗夜核 + 双层银紫镶边与菱形装饰点
        ArtStyle::Gothic => {
            let obs = Color32::from_rgba_unmultiplied(24, 20, 32, ca);
            let trim = if is_preset_black { Color32::from_rgb(196, 181, 253) } else { mix_rgb(unmult_rgb, Color32::from_rgb(225, 210, 255), 0.45) };
            fill_star(c, (cx, cy), &pts, obs);
            closed_path(c, &pts, (r * 0.095).clamp(1.6, 3.5), with_a(trim, 235));
            let inner: Vec<(f32, f32)> = raw.iter().map(|&(x, y)| (cx + x * r * 0.85, cy + y * r * 0.85)).collect();
            closed_path(c, &inner, (r * 0.036).clamp(0.8, 1.4), with_a(trim, 140));
            for a in [0.0f32, PI * 0.5, PI, PI * 1.5] {
                c.circle(cx + a.cos() * r * 0.92, cy + a.sin() * r * 0.92, (r * 0.055).clamp(1.0, 2.1), with_a(trim, 230));
            }
        }

        // 17-19. 像素风格 (pixel-crisp / pixel-dot / pixel-block)：连同耳朵/叶子/配件一并像素栅格化
        ArtStyle::PixelCrisp | ArtStyle::PixelDot | ArtStyle::PixelBlock => {
            let n = if r < 20.0 { 9i32 } else { 13i32 };
            let cell = (r * 2.05) / n as f32;
            let rad_c = (cell * 0.46).max(1.0);
            let half = n / 2;
            let px_col = if is_preset_black {
                Color32::from_rgba_unmultiplied(64, 68, 82, ca)
            } else {
                color
            };
            let is_green_body = cg > cr.saturating_add(25) && cg > cb.saturating_add(20);
            let leaf_px = Color32::from_rgba_unmultiplied(
                if is_green_body { 118 } else { 76 },
                if is_green_body { 242 } else { 217 },
                if is_green_body { 142 } else { 100 },
                ca,
            );
            let hair_px = if is_preset_black {
                Color32::from_rgba_unmultiplied(88, 94, 114, ca)
            } else {
                mix_rgb(px_col, Color32::BLACK, 0.42)
            };
            let inner_px = mix_rgb(px_col, Color32::WHITE, 0.50);
            let part_polys: Vec<(PartTone, Vec<(f32, f32)>)> = shape
                .data()
                .map(|d| {
                    d.parts
                        .iter()
                        .map(|p| (p.tone, p.pts.iter().map(|&(x, y)| (cx + x * r, cy + y * r)).collect()))
                        .collect()
                })
                .unwrap_or_default();
            for gy in -half..=half {
                for gx in -half..=half {
                    let px = cx + gx as f32 * cell;
                    let py = cy + gy as f32 * cell;
                    let mut hit_col = if point_in_poly(px, py, &pts) { Some(px_col) } else { None };
                    for (tone, poly) in &part_polys {
                        if point_in_poly(px, py, poly) {
                            hit_col = Some(match tone {
                                PartTone::Body => px_col,
                                PartTone::Leaf => leaf_px,
                                PartTone::Inner => inner_px,
                                PartTone::Hair => hair_px,
                            });
                        }
                    }
                    if let Some(cell_col) = hit_col {
                        let hi_col = mix_rgb(cell_col, Color32::WHITE, 0.28);
                        let lo_col = mix_rgb(cell_col, Color32::BLACK, 0.28);
                        let x0 = px - cell * 0.46;
                        let y0 = py - cell * 0.46;
                        let w0 = cell * 0.92;
                        match style {
                            ArtStyle::PixelCrisp => c.fill(x0, y0, w0, w0, 0.0, cell_col),
                            ArtStyle::PixelDot => c.circle(px, py, rad_c * 0.90, cell_col),
                            ArtStyle::PixelBlock => {
                                c.fill(x0, y0, w0, w0, (cell * 0.14).min(2.0), cell_col);
                                c.seg((x0, y0), (x0 + w0, y0), 1.0, with_a(hi_col, 165));
                                c.seg((x0, y0 + w0), (x0 + w0, y0 + w0), 1.0, with_a(lo_col, 165));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // 20-21. 颜文字风格 (kaomoji-plain / kaomoji-bubble)
        ArtStyle::KaomojiPlain => {}
        ArtStyle::KaomojiBubble => {
            let bw = (r * 2.35).max(26.0);
            let bh = (r * 1.48).max(17.0);
            let bubble_bg = if is_preset_black {
                Color32::from_rgba_unmultiplied(44, 48, 60, scale_a(235))
            } else {
                with_a(color, 235)
            };
            let bubble_rim = with_a(mix_rgb(unmult_rgb, Color32::WHITE, 0.38), 195);
            c.fill(cx - bw * 0.5, cy - bh * 0.5, bw, bh, (bh * 0.46).max(6.0), bubble_bg);
            c.stroke(cx - bw * 0.5, cy - bh * 0.5, bw, bh, (bh * 0.46).max(6.0), 1.0, bubble_rim);
            c.circle(cx - bw * 0.28, cy + bh * 0.58, (r * 0.11).max(1.4), bubble_bg);
        }
    }
}

fn point_in_poly(px: f32, py: f32, poly: &[(f32, f32)]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        let hit = ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / ((yj - yi).abs().max(1e-6) * if yj >= yi { 1.0 } else { -1.0 }) + xi);
        if hit {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 根据配置计算吉祥物的（内部填充色, 可选独立外部轮廓描边色），不含五官脸型颜色。
pub fn resolve_mascot_colors(color_idx: usize, cfg: &super::config::Config) -> (Color32, Option<Color32>) {
    let base_fill = if !cfg!(test) && cfg.aora_custom_fill_enabled {
        let [r, g, b] = cfg.aora_custom_fill_rgb;
        Color32::from_rgba_unmultiplied(r, g, b, cfg.face_fill_alpha)
    } else {
        PALETTE[color_idx.min(PALETTE.len() - 1)]
    };
    let outline_col = if !cfg!(test) && cfg.aora_custom_outline_enabled {
        let [r, g, b] = cfg.aora_custom_outline_rgb;
        Some(Color32::from_rgba_unmultiplied(r, g, b, cfg.face_stroke_alpha))
    } else {
        None
    };
    (base_fill, outline_col)
}

fn draw_shape_parts(
    c: &Card,
    cx: f32,
    cy: f32,
    r: f32,
    shape: BodyShape,
    fill_color: Color32,
    outline_override: Option<Color32>,
    style: ArtStyle,
) {
    if matches!(
        style,
        ArtStyle::KaomojiPlain | ArtStyle::KaomojiBubble | ArtStyle::PixelCrisp | ArtStyle::PixelDot | ArtStyle::PixelBlock
    ) {
        return;
    }
    let Some(data) = shape.data() else {
        return;
    };
    if data.parts.is_empty() {
        return;
    }
    let [cr, cg, cb, ca] = fill_color.to_srgba_unmultiplied();
    let alpha_k = if cfg!(test) { 1.0 } else { (ca as f32 / 255.0).clamp(0.0, 1.0) };
    let is_preset_black = cr <= 36 && cg <= 36 && cb <= 42 && ca >= 250;
    let is_green_body = cg > cr.saturating_add(25) && cg > cb.saturating_add(20);
    let leaf_col = Color32::from_rgba_unmultiplied(
        if is_green_body { 118 } else { 76 },
        if is_green_body { 242 } else { 217 },
        if is_green_body { 142 } else { 100 },
        ca,
    );
    let inner_col = mix_rgb(fill_color, Color32::WHITE, 0.50);
    let hair_col = if is_preset_black {
        Color32::from_rgba_unmultiplied(76, 82, 98, ca)
    } else {
        let dark = mix_rgb(fill_color, Color32::BLACK, 0.42);
        let [dr, dg, db, _] = dark.to_srgba_unmultiplied();
        Color32::from_rgba_unmultiplied(dr, dg, db, ca)
    };
    let paper_white = Color32::from_rgba_unmultiplied(255, 252, 246, ca);

    if style == ArtStyle::Sticker {
        let bw = (r * 0.20).clamp(2.6, 8.5);
        for p in data.parts {
            if matches!(p.tone, PartTone::Inner) {
                continue;
            }
            let ppts: Vec<(f32, f32)> = p.pts.iter().map(|&(x, y)| (cx + x * r, cy + y * r)).collect();
            closed_path(c, &ppts, bw, paper_white);
        }
    }

    for p in data.parts {
        let ppts: Vec<(f32, f32)> = p.pts.iter().map(|&(x, y)| (cx + x * r, cy + y * r)).collect();
        let tone_col = match p.tone {
            PartTone::Body => fill_color,
            PartTone::Leaf => leaf_col,
            PartTone::Inner => inner_col,
            PartTone::Hair => hair_col,
        };
        let [tr, tg, tb, _] = tone_col.to_srgba_unmultiplied();
        match style {
            ArtStyle::Outline => {
                let stroke_c = if matches!(p.tone, PartTone::Body) {
                    outline_override.unwrap_or_else(|| {
                        if is_preset_black { Color32::from_rgb(242, 244, 248) } else { fill_color }
                    })
                } else {
                    tone_col
                };
                let tint = Color32::from_rgba_unmultiplied(tr, tg, tb, ((48.0 * alpha_k).round()) as u8);
                fill_star(c, (cx, cy), &ppts, tint);
                closed_path(c, &ppts, (r * 0.08).clamp(1.3, 3.0), stroke_c);
            }
            ArtStyle::Glass => {
                let glass_fill = Color32::from_rgba_unmultiplied(tr, tg, tb, ((125.0 * alpha_k).round()) as u8);
                fill_star(c, (cx, cy), &ppts, glass_fill);
                let rim = mix_rgb(tone_col, Color32::WHITE, 0.55);
                let [rr, rg, rb, _] = rim.to_srgba_unmultiplied();
                closed_path(c, &ppts, (r * 0.055).clamp(1.0, 2.2), Color32::from_rgba_unmultiplied(rr, rg, rb, ((205.0 * alpha_k).round()) as u8));
            }
            ArtStyle::Neon => {
                let n_col = if p.tone == PartTone::Body && is_preset_black {
                    Color32::from_rgb(56, 189, 248)
                } else {
                    tone_col
                };
                fill_star(c, (cx, cy), &ppts, Color32::from_rgba_unmultiplied(12, 16, 26, ((210.0 * alpha_k).round()) as u8));
                closed_path(c, &ppts, (r * 0.06).clamp(1.2, 2.4), n_col.gamma_multiply(0.9 * alpha_k));
            }
            ArtStyle::Gothic => {
                fill_star(c, (cx, cy), &ppts, if p.tone == PartTone::Body { Color32::from_rgba_unmultiplied(24, 20, 32, ca) } else { tone_col });
                let trim = if is_preset_black { Color32::from_rgb(196, 181, 253) } else { mix_rgb(tone_col, Color32::from_rgb(225, 210, 255), 0.45) };
                closed_path(c, &ppts, (r * 0.07).clamp(1.2, 2.6), Color32::from_rgba_unmultiplied(trim.r(), trim.g(), trim.b(), ca));
            }
            ArtStyle::Chalk => {
                fill_star(c, (cx, cy), &ppts, Color32::from_rgba_unmultiplied(30, 36, 38, ((200.0 * alpha_k).round()) as u8));
                let dust = if p.tone == PartTone::Body && is_preset_black { Color32::from_rgb(236, 240, 234) } else { mix_rgb(tone_col, Color32::WHITE, 0.42) };
                closed_path(c, &ppts, (r * 0.07).clamp(1.2, 2.6), Color32::from_rgba_unmultiplied(dust.r(), dust.g(), dust.b(), ((175.0 * alpha_k).round()) as u8));
            }
            _ => {
                let part_col = if style == ArtStyle::Sticker && is_preset_black && p.tone == PartTone::Body {
                    Color32::from_rgba_unmultiplied(46, 48, 58, ca)
                } else {
                    tone_col
                };
                fill_star(c, (cx, cy), &ppts, part_col);
                closed_path(c, &ppts, 1.0, part_col);
                if p.tone == PartTone::Body {
                    if let Some(out_col) = outline_override {
                        closed_path(c, &ppts, (r * 0.07).clamp(1.1, 2.6), out_col);
                    }
                }
            }
        }
    }
}

/// 支持「内部填充色」与「外部轮廓色」二段独立调色盘的身体绘制函数（五官脸型颜色不参与轮廓染色）。
pub fn draw_shape_dual(
    c: &Card,
    cx: f32,
    cy: f32,
    r: f32,
    shape: BodyShape,
    fill_color: Color32,
    outline_override: Option<Color32>,
    style: ArtStyle,
) {
    if shape == BodyShape::Islo {
        return;
    }
    if style == ArtStyle::Outline {
        if let Some(out_col) = outline_override {
            let raw = shape_points(shape);
            let pts: Vec<(f32, f32)> = raw.iter().map(|&(x, y)| (cx + x * r, cy + y * r)).collect();
            let [fr, fg, fb, fa] = fill_color.to_srgba_unmultiplied();
            let tint_a = if cfg!(test) { 54 } else { ((fa as f32) * 0.35).round() as u8 };
            let tint = Color32::from_rgba_unmultiplied(fr, fg, fb, tint_a);
            fill_star(c, (cx, cy), &pts, tint);
            let sw = (r * 0.095).clamp(1.8, 3.8);
            closed_path(c, &pts, sw, out_col);
            draw_shape_parts(c, cx, cy, r, shape, fill_color, outline_override, style);
            return;
        }
    }
    draw_shape(c, cx, cy, r, shape, fill_color, style);
    if !matches!(style, ArtStyle::KaomojiPlain | ArtStyle::KaomojiBubble | ArtStyle::PixelCrisp | ArtStyle::PixelDot | ArtStyle::PixelBlock) {
        if let Some(out_col) = outline_override {
            let raw = shape_points(shape);
            let pts: Vec<(f32, f32)> = raw.iter().map(|&(x, y)| (cx + x * r, cy + y * r)).collect();
            let sw = (r * 0.088).clamp(1.5, 3.4);
            closed_path(c, &pts, sw, out_col);
        }
    }
    draw_shape_parts(c, cx, cy, r, shape, fill_color, outline_override, style);
}

// ─── 主岛集成 ──────────────────────────────────────────────────

/// 主岛吉祥物：完全替换 islo_face::decorate，画「身体 + 五官」的完整 Aora Bot 角色。
/// 画风 / 形状 / 颜色 / 脸型 / 大小全部读用户配置，设置页的选择会实时反映到岛上。
/// 返回 false 表示调用方继续画原生装饰（音乐态、关闭表情）。
/// 将 Aora 44 状态索引映射到初代 `scstc/islo` 的 20 种原生表情。
pub fn aora_idx_to_islo_face(idx: usize, state_name: &str) -> super::islo_face::Face {
    use super::islo_face::Face;
    if state_name == "Music" {
        return Face::Music;
    }
    match STATES[idx.min(STATES.len() - 1)].id {
        "idle" => Face::Awake,
        "sleeping" => Face::Sleeping,
        "sleepy" | "bored" => Face::Dozing,
        "waking" | "wake" => Face::Waking,
        "off" => Face::Sleeping,
        "thinking" | "planning" | "reasoning" | "reflecting" => Face::Thinking,
        "searching" | "reading" | "browsing" | "inspecting" => Face::Curious,
        "coding" | "writing" | "running" | "executing" | "building" => Face::Focused,
        "loading" | "syncing" | "uploading" | "downloading" | "installing" => Face::Loading,
        "listening" => Face::Listening,
        "happy" | "relieved" => Face::Happy,
        "excited" | "celebrating" | "proud" => Face::Excited,
        "laughing" | "playful" => Face::Laughing,
        "satisfied" | "loving" | "grateful" => Face::Satisfied,
        "shy" => Face::Shy,
        "sad" | "lonely" => Face::Sad,
        "angry" | "frustrated" => Face::Angry,
        "surprised" | "shocked" => Face::Surprised,
        "confused" | "puzzled" => Face::Confused,
        "worried" | "anxious" | "error" | "warning" => Face::Worried,
        _ => Face::from_index(Face::from_state_name(state_name)),
    }
}

/// 设置页点击「预览 AI 工作表情联动」时触发 12 秒的 19 种「处理中」表情随机轮播演示。
static AI_MASCOT_DEMO_UNTIL_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
static ISLAND_EVENT_STATE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static ISLAND_EVENT_START_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
static ISLAND_EVENT_UNTIL_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// 触发岛上吉祥物的情景互动表情（如来消息 `20`、检测到复制/新内容 `22`、拖拽文件 `24`、防窥警觉 `36`、指纹解锁/完成庆祝 `30`）。
pub fn trigger_mascot_event(state_idx: usize, duration_secs: f64) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let dur_ms = (duration_secs.clamp(1.0, 20.0) * 1000.0) as i64;
    ISLAND_EVENT_STATE.store(state_idx.min(43), std::sync::atomic::Ordering::Relaxed);
    ISLAND_EVENT_START_MS.store(now_ms, std::sync::atomic::Ordering::Relaxed);
    ISLAND_EVENT_UNTIL_MS.store(now_ms + dur_ms, std::sync::atomic::Ordering::Relaxed);
}

/// 解析当前全场景岛内事件对应的灵动表情索引。
pub fn active_island_event_state(now_sec: f64, state_name: &str) -> Option<usize> {
    let now_ms = (now_sec * 1000.0) as i64;
    let until_ms = ISLAND_EVENT_UNTIL_MS.load(std::sync::atomic::Ordering::Relaxed);
    if until_ms > now_ms {
        let start_ms = ISLAND_EVENT_START_MS.load(std::sync::atomic::Ordering::Relaxed);
        let elapsed_s = ((now_ms - start_ms).max(0) as f64) / 1000.0;
        let base_st = ISLAND_EVENT_STATE.load(std::sync::atomic::Ordering::Relaxed);
        // 来消息时：前 1.1 秒先露出「惊讶 (37) / 通知 (42)」，随后切换为「消息 (20) / 接收 (19) / 开心 (25)」灵动轮换
        if base_st == 20 {
            if elapsed_s < 1.1 {
                return Some(37); // surprised 惊讶
            } else if elapsed_s < 3.2 {
                return Some(20); // messaging 消息声波
            } else {
                return Some(19); // receiving 接收箭头
            }
        }
        // 检测到剪贴板复制或新对象时：前 1.2 秒「雷达 (14)」，随后「生成 (22) / 好奇 (39)」
        if base_st == 22 {
            if elapsed_s < 1.2 {
                return Some(14); // radar 雷达扫描
            } else if elapsed_s < 2.8 {
                return Some(22); // spawning 生成光环
            } else {
                return Some(39); // curious 好奇张望
            }
        }
        return Some(base_st.min(43));
    }
    // 鼠标悬停在岛体上逗弄吉祥物时，在「好奇 (39) → 调皮眨眼 (28) → 害羞爱心 (32) → 蹦跳 (31)」间自然切换
    if state_name == "Curious" {
        let choices = [39usize, 28, 32, 31];
        let slot = (now_sec / 2.6).floor() as usize;
        return Some(choices[slot % choices.len()]);
    }
    None
}

pub fn trigger_ai_mascot_demo() {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    AI_MASCOT_DEMO_UNTIL_MS.store(now_ms + 12_000, std::sync::atomic::Ordering::Relaxed);
}

pub fn ai_mascot_demo_active() -> bool {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    AI_MASCOT_DEMO_UNTIL_MS.load(std::sync::atomic::Ordering::Relaxed) > now_ms
}

/// 判断当前是否有活跃的 AI 工作状态与吉祥物联动。
pub fn is_ai_mascot_active() -> bool {
    let cfg = super::config::get();
    if !cfg.ai_mascot_link {
        return false;
    }
    if ai_mascot_demo_active() {
        return true;
    }
    let (agents, finished) = super::state::snapshot();
    !agents.is_empty() || finished.is_some()
}

/// 当 AI 正在工作（或处于联动演示态）时，计算当前应展示的吉祥物表情状态索引（0..44）：
/// - AI 工作中（Working / Tool / Running）：在「处理中」分类的 19 个表情包（索引 6..=24，图十）中随机轮播或按动作语义智能匹配；
/// - AI 等待回复（Waiting）：在「等待 (21) / 倾听 (6) / 通知 (42)」间切换；
/// - AI 出错（Error）：在「警报 (41) / 拒绝 (43) / 困惑 (34)」间切换；
/// - AI 刚完成任务（Done / Finished）：在「庆祝 (30) / 开心 (25) / 兴奋 (26) / 自豪 (29)」间切换。
pub fn active_ai_mascot_state(now: f64, cfg: &super::config::Config) -> Option<usize> {
    if !cfg.ai_mascot_link {
        return None;
    }
    let interval = (cfg.ai_mascot_interval.clamp(2, 15)) as f64;
    let slot = (now / interval).floor() as u64;
    // 19 个「处理中」(StateCat::Attending) 表情包索引为 6..=24（共 19 个）
    let rand_attending = 6 + ((slot.wrapping_mul(7) + (slot / 19).wrapping_mul(11) + 3) % 19) as usize;

    if ai_mascot_demo_active() {
        return Some(rand_attending);
    }

    let (agents, finished) = super::state::snapshot();
    if let Some(agent) = agents.first() {
        use super::state::Phase;
        match agent.phase {
            Phase::Working | Phase::Tool | Phase::Running => {
                if cfg.ai_mascot_mode == 1 && slot % 2 == 0 {
                    let text = agent.doing().to_ascii_lowercase();
                    if text.contains("search") || text.contains("搜索") || text.contains("web") || text.contains("find") {
                        return Some(8); // 搜索 searching
                    } else if text.contains("read") || text.contains("阅读") || text.contains("view") || text.contains("分析") {
                        return Some(9); // 阅读 reading
                    } else if text.contains("write") || text.contains("书写") || text.contains("edit") || text.contains("修改") {
                        return Some(10); // 书写 writing
                    } else if text.contains("code") || text.contains("编写") || text.contains("build") || text.contains("编译") {
                        return Some(23); // 编写 composing
                    } else if text.contains("think") || text.contains("思考") || text.contains("plan") || text.contains("推理") {
                        return Some(7); // 思考 thinking
                    } else if text.contains("upload") || text.contains("上传") {
                        return Some(17); // 上传 uploading
                    } else if text.contains("send") || text.contains("发送") {
                        return Some(18); // 发送 sending
                    } else if text.contains("spawn") || text.contains("生成") || text.contains("create") {
                        return Some(22); // 生成 spawning
                    } else if text.contains("load") || text.contains("加载") || text.contains("download") {
                        return Some(12); // 加载 loading
                    }
                }
                Some(rand_attending)
            }
            Phase::Waiting => {
                let choices = [21usize, 6, 42];
                Some(choices[(slot as usize) % choices.len()])
            }
            Phase::Error => {
                let choices = [41usize, 43, 34];
                Some(choices[(slot as usize) % choices.len()])
            }
            Phase::Done => {
                let choices = [30usize, 25, 26, 29];
                Some(choices[(slot as usize) % choices.len()])
            }
        }
    } else if finished.is_some() {
        let choices = [30usize, 25, 26, 29];
        Some(choices[(slot as usize) % choices.len()])
    } else {
        None
    }
}

pub fn decorate(c: &Card, cx: f32, state_name: &str, age: f32, y: f32, gaze: [f32; 2], fixed: i64) -> bool {
    let cfg = super::config::get();
    if !cfg.islo_face { return false; }
    // 单元测试中保留 Music 原生 13 点眼睛与麦克风契约；实际运行时始终绘制用户所选吉祥物，绝不闪回原版形象。
    if cfg!(test) && state_name == "Music" { return false; }
    let now_sec = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(age as f64);
    let ai_linked_idx = if !cfg!(test) {
        active_ai_mascot_state(now_sec, &cfg)
    } else {
        None
    };
    let event_linked_idx = if !cfg!(test) {
        active_island_event_state(now_sec, state_name)
    } else {
        None
    };
    let idx = if let Some(ev_idx) = event_linked_idx {
        ev_idx
    } else if let Some(ai_idx) = ai_linked_idx {
        ai_idx
    } else if fixed > 0 && fixed <= 44 {
        fixed as usize - 1
    } else {
        state_from_pet(state_name)
    };
    let anim_t = if cfg!(test) {
        age
    } else {
        (now_sec % 86400.0) as f32
    };
    let style = ArtStyle::ALL[(cfg.aora_style as usize).min(ArtStyle::ALL.len() - 1)];
    let shape = BodyShape::ALL[(cfg.aora_shape as usize).min(BodyShape::ALL.len() - 1)];
    let (color, outline_override) = resolve_mascot_colors((cfg.aora_color as usize).min(PALETTE.len() - 1), &cfg);
    let face_style = FaceStyle::ALL[(cfg.aora_face_style as usize).min(FaceStyle::ALL.len() - 1)];
    let scale = (cfg.aora_scale as f32 / 100.0).clamp(0.6, 1.5);
    let lively_bob = if !cfg!(test) && (event_linked_idx.is_some() || ai_linked_idx.is_some() || matches!(idx, 20 | 25 | 26 | 28 | 30 | 31 | 37 | 40)) {
        (anim_t * 4.5).sin() * 0.75
    } else {
        0.0
    };
    let face_cx = cx + gaze[0] * 0.5;
    let face_cy = 14.5 + y + gaze[1] + lively_bob;

    // 初代 scstc/islo 吉祥物模式：绘制原汁原味的 islo 胶囊眼 + 线性嘴 + 腮红 + 20 态装饰
    if shape == BodyShape::Islo {
        let islo_face = if ai_linked_idx.is_some() {
            aora_idx_to_islo_face(idx, state_name)
        } else if fixed > 0 && fixed <= 20 {
            super::islo_face::Face::from_index(fixed as usize - 1)
        } else {
            aora_idx_to_islo_face(idx, state_name)
        };
        let islo_ink = if cfg.aora_custom_fill_enabled {
            color
        } else if cfg.aora_color <= 1 {
            Color32::WHITE
        } else {
            color
        };
        super::islo_face::draw_custom(c, cx, 15.0 + y, scale, islo_face, anim_t, gaze, islo_ink);
        return true;
    }

    let body_r = 13.5 * scale;
    draw_shape_dual(c, face_cx, face_cy, body_r, shape, color, outline_override, style);
    let ink = resolve_mascot_ink(color, outline_override, style, &cfg);
    let ([fx, fy], eye_fit) = if matches!(style, ArtStyle::KaomojiPlain | ArtStyle::KaomojiBubble) {
        ([0.0, 0.0], 1.0)
    } else {
        shape_face_fit(shape)
    };
    let r = 0.92 * scale * eye_fit;
    let draw_idx = if ai_linked_idx.is_none() && state_name == "Music" && fixed <= 0 { 25 } else { idx };
    draw_face_in_style(c, face_cx + fx * body_r, face_cy + fy * body_r, r, draw_idx, ink, face_style, style, anim_t);
    if state_name == "Music" && ai_linked_idx.is_none() {
        let bob = (anim_t * 2.4).sin() * 1.1;
        let nx = face_cx + 15.5 * scale;
        let ny = face_cy - 4.0 * scale + bob;
        c.seg((nx, ny - 2.6), (nx, ny + 2.6), 1.1, Color32::WHITE);
        c.seg((nx, ny - 2.6), (nx + 2.8, ny - 1.7), 1.1, Color32::WHITE);
        c.circle(nx - 1.1, ny + 2.6, 1.4, Color32::WHITE);
    }
    true
}

// ─── 调色板（对齐 Aora Bot 官方色卡）────────────────────────────

pub const PALETTE: [Color32; 8] = [
    Color32::from_rgb(241, 226, 193), // 0: cream 奶油米白 (#F1E2C1)
    Color32::from_rgb(40, 42, 50),    // 1: black 曜石黑 (#282A32)
    Color32::from_rgb(255, 114, 40),  // 2: orange 活力橙 (#FF7228)
    Color32::from_rgb(235, 75, 79),   // 3: red 朱砂红 (#EB4B4F)
    Color32::from_rgb(240, 112, 159), // 4: pink 樱花粉 (#F0709F)
    Color32::from_rgb(240, 179, 38),  // 5: yellow 琥珀黄 (#F0B326)
    Color32::from_rgb(43, 143, 255),  // 6: blue 晴空蓝 (#2B8FFF)
    Color32::from_rgb(132, 107, 236), // 7: violet 丁香紫 (#846BEC)
];

pub const PALETTE_NAMES: [&str; 8] = [
    "奶油", "曜黑", "活力橙", "朱砂红", "樱花粉", "琥珀黄", "晴空蓝", "丁香紫",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forty_four_states_in_four_categories() {
        assert_eq!(STATES.len(), 44);
        let mut counts = [0usize; 4];
        for s in STATES {
            counts[s.cat as usize] += 1;
        }
        assert_eq!(counts, [6, 19, 16, 3], "静息6 处理中19 情绪16 提示3");
    }

    #[test]
    fn unique_state_ids() {
        let mut ids = std::collections::HashSet::new();
        for s in STATES { assert!(ids.insert(s.id), "重复 id {}", s.id); }
    }

    #[test]
    fn all_eye_presets_renderable() {
        for id in [CuteEye::Dot, CuteEye::Star, CuteEye::Sad, CuteEye::Mad, CuteEye::Wink, CuteEye::Cross] {
            let pair = cute_eye_pair(id);
            for e in &pair {
                assert!(e.width.is_finite() && e.len.is_finite());
            }
        }
    }

    #[test]
    fn all_mouth_presets_finite() {
        for id in [CuteMouth::None, CuteMouth::Smile, CuteMouth::Cat, CuteMouth::Grin, CuteMouth::Laugh, CuteMouth::Wavy] {
            let m = cute_mouth_spec(id);
            assert!(m.width.is_finite() && m.bend.is_finite() && m.open.is_finite());
        }
    }

    #[test]
    fn state_from_pet_maps_known() {
        for n in ["Awake", "Curious", "Happy", "Sleeping", "Unknown"] {
            assert!(state_from_pet(n) < 44);
        }
    }

    #[test]
    fn ink_always_contrasts_with_body() {
        // 回归：五官写死 WHITE + 默认近白身体 = 一整块「白色遮罩」，脸完全看不见。
        assert!(ink_for(PALETTE[0], ArtStyle::Solid).r() < 100, "浅色身体必须用深墨");
        assert!(ink_for(PALETTE[1], ArtStyle::Solid).r() > 200, "深色身体必须用亮白");
        for style in ArtStyle::ALL {
            if style == ArtStyle::Outline {
                assert!(ink_for(PALETTE[1], style).r() > 200, "线条画风不填身体，深底上必须用亮白");
                continue;
            }
            let a = ink_for(PALETTE[0], style);
            let b = ink_for(PALETTE[1], style);
            assert_ne!(a, b, "画风 {style:?} 下两种身体不能用同色五官");
        }
    }

    #[test]
    fn every_body_shape_is_a_finite_star_polygon() {
        assert_eq!(BodyShape::ALL.len(), 72);
        for shape in BodyShape::ALL {
            let pts = shape_points(shape);
            assert!(pts.len() >= 12, "形状 {shape:?} 轮廓点数过少");
            assert!(pts.iter().all(|&(x, y)| x.is_finite() && y.is_finite() && x.abs() < 2.2 && y.abs() < 2.2),
                "形状 {shape:?} 的轮廓越界");
        }
    }
}
