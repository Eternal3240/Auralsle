//! Auralsle 灵屿音乐岛完整引擎：
//! 多布局几何插值、自定义模块/坐标编辑器、封面动态取色、高斯模糊背板、
//! 发光逐字歌词与双语翻译、8 款进度条、16 款游标、10 套播控主题、外围呼吸/律动光晕。
//!
//! 移植自 Heiamer/Auralsle (MIT License) 并深度适配 AuraIsle `egui` 管线。

use crate::paint::{self, Card, H};
use crate::theme::W;
use eframe::egui::{self, Color32, CornerRadius, Mesh, Pos2, Rect, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ============================================================================
// 1. 共享几何与布局平滑插值 (ported from music_layout.rs)
// ============================================================================

#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct DRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl DRect {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn to_egui(self) -> Rect {
        Rect::from_min_size(Pos2::new(self.x, self.y), Vec2::new(self.w, self.h))
    }
}

fn lerp_f(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_drect(a: DRect, b: DRect, t: f32) -> DRect {
    DRect::new(
        lerp_f(a.x, b.x, t),
        lerp_f(a.y, b.y, t),
        lerp_f(a.w, b.w, t),
        lerp_f(a.h, b.h, t),
    )
}

pub fn cover_corner_radius(card_radius: f32, width: f32, height: f32) -> f32 {
    let radius = if card_radius.is_finite() { card_radius.max(0.0) } else { 30.0 };
    (radius * 0.5).min(width.max(0.0).min(height.max(0.0)) * 0.5)
}

fn buttons_at(width: f32, center_y: f32) -> [(i32, DRect); 8] {
    let snap = |v: f32| v.round();
    let center = snap(width * 0.5);
    let y = snap(center_y);
    [
        (2, DRect::new(36.0, y - 14.0, 28.0, 28.0)),
        (9, DRect::new(68.0, y - 14.0, 28.0, 28.0)),
        (3, DRect::new(center - 58.0, y - 16.0, 32.0, 32.0)),
        (4, DRect::new(center - 20.0, y - 20.0, 40.0, 40.0)),
        (5, DRect::new(center + 26.0, y - 16.0, 32.0, 32.0)),
        (6, DRect::new(width - 96.0, y - 14.0, 28.0, 28.0)),
        (7, DRect::new(width - 64.0, y - 14.0, 28.0, 28.0)),
        (16, DRect::new(100.0, y - 14.0, 28.0, 28.0)),
    ]
}

pub fn music_height(mix: f32, font: f32) -> f32 {
    220.0 + 16.0 * (mix * 2.0).clamp(0.0, 1.0) + (font + 8.0 - 24.0).max(0.0)
}

pub fn music_layout_mix(from: f32, target: f32, elapsed_seconds: f64) -> f32 {
    let t = (elapsed_seconds / 0.23).clamp(0.0, 1.0) as f32;
    if t >= 1.0 {
        return target;
    }
    let e = t * t * (3.0 - 2.0 * t);
    from + (target - from) * e
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusicLayout {
    SideBySide,
    Separated,
    LyricFocus,
    LyricRight,
    Immersive,
}

impl MusicLayout {
    pub fn position(self) -> f32 {
        match self {
            Self::SideBySide => 0.0,
            Self::Separated => 1.0,
            Self::LyricFocus => 2.0,
            Self::LyricRight => 3.0,
            Self::Immersive => 4.0,
        }
    }
    pub fn from_setting(value: &str) -> Self {
        match value {
            "Separated" => Self::Separated,
            "LyricFocus" => Self::LyricFocus,
            "LyricRight" => Self::LyricRight,
            "Immersive" => Self::Immersive,
            _ => Self::SideBySide,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MusicGeometry {
    pub cover: DRect,
    pub title: DRect,
    pub artist: DRect,
    pub lyric: DRect,
    pub secondary: DRect,
    pub status_dot: DRect,
    pub status_text: DRect,
    pub track: DRect,
    pub elapsed: DRect,
    pub total: DRect,
    pub buttons: [(i32, DRect); 8],
    pub height: f32,
    pub progress_hit: DRect,
    pub text_hit: DRect,
    pub split: f32,
    pub align: f32,
}

impl MusicGeometry {
    pub fn between(a: &Self, b: &Self, t: f32) -> Self {
        let mut buttons = a.buttons;
        for (slot, other) in buttons.iter_mut().zip(b.buttons) {
            slot.1 = lerp_drect(slot.1, other.1, t);
        }
        Self {
            cover: lerp_drect(a.cover, b.cover, t),
            title: lerp_drect(a.title, b.title, t),
            artist: lerp_drect(a.artist, b.artist, t),
            lyric: lerp_drect(a.lyric, b.lyric, t),
            secondary: lerp_drect(a.secondary, b.secondary, t),
            status_dot: lerp_drect(a.status_dot, b.status_dot, t),
            status_text: lerp_drect(a.status_text, b.status_text, t),
            track: lerp_drect(a.track, b.track, t),
            elapsed: lerp_drect(a.elapsed, b.elapsed, t),
            total: lerp_drect(a.total, b.total, t),
            buttons,
            height: lerp_f(a.height, b.height, t),
            progress_hit: lerp_drect(a.progress_hit, b.progress_hit, t),
            text_hit: lerp_drect(a.text_hit, b.text_hit, t),
            split: lerp_f(a.split, b.split, t),
            align: lerp_f(a.align, b.align, t),
        }
    }
}

pub fn music_geometry(width: f32, _layout: MusicLayout, mix: f32, font: f32) -> MusicGeometry {
    let extra = (font + 8.0 - 24.0).max(0.0);
    let line = 24.0 + extra;
    let side_cov = (92.0 + extra).clamp(88.0, 100.0);
    let side_tx = 36.0 + side_cov + 14.0;
    let side = MusicGeometry {
        cover: DRect::new(36.0, 18.0, side_cov, side_cov),
        title: DRect::new(side_tx, 18.0, (width - side_tx - 36.0).max(80.0), 24.0),
        artist: DRect::new(side_tx, 45.0, (width - side_tx - 36.0).max(80.0), 18.0),
        lyric: DRect::new(side_tx, 68.0, (width - side_tx - 36.0).max(80.0), line),
        secondary: DRect::new(side_tx, 92.0 + extra, (width - side_tx - 36.0).max(80.0), 18.0),
        status_dot: DRect::ZERO,
        status_text: DRect::ZERO,
        track: DRect::new(36.0, 134.0 + extra, (width - 72.0).max(80.0), 4.0),
        elapsed: DRect::new(36.0, 144.0 + extra, 70.0, 14.0),
        total: DRect::new(width - 106.0, 144.0 + extra, 70.0, 14.0),
        buttons: buttons_at(width, 185.0 + extra),
        height: music_height(0.0, font),
        progress_hit: DRect::new(32.0, 126.0 + extra, (width - 64.0).max(80.0), 18.0),
        text_hit: DRect::new(side_tx, 18.0, (width - side_tx - 36.0).max(80.0), 46.0),
        split: 0.0,
        align: 0.0,
    };
    let separated = MusicGeometry {
        cover: DRect::new(36.0, 18.0, 56.0, 56.0),
        title: DRect::new(106.0, 23.5, (width - 142.0).max(80.0), 24.0),
        artist: DRect::new(106.0, 50.5, (width - 142.0).max(80.0), 18.0),
        lyric: DRect::new(36.0, 94.0, (width - 72.0).max(80.0), line),
        secondary: DRect::new(36.0, 118.0 + extra, (width - 72.0).max(80.0), 18.0),
        status_dot: DRect::ZERO,
        status_text: DRect::ZERO,
        track: DRect::new(36.0, 150.0 + extra, (width - 72.0).max(80.0), 4.0),
        elapsed: DRect::new(36.0, 160.0 + extra, 70.0, 14.0),
        total: DRect::new(width - 106.0, 160.0 + extra, 70.0, 14.0),
        buttons: buttons_at(width, 201.0 + extra),
        height: music_height(1.0, font),
        progress_hit: DRect::new(32.0, 142.0 + extra, (width - 64.0).max(80.0), 18.0),
        text_hit: DRect::new(106.0, 23.5, (width - 142.0).max(80.0), 45.0),
        split: 1.0,
        align: 0.0,
    };
    let mut focus = separated;
    let focus_w = (width - 224.0).max(80.0);
    let focus_x = (width - focus_w) * 0.5;
    focus.cover = DRect::new(36.0, 18.0, 68.0, 68.0);
    focus.title = DRect::new(focus_x, 22.0, focus_w, 24.0);
    focus.artist = DRect::new(focus_x, 50.0, focus_w, 18.0);
    focus.status_dot = DRect::ZERO;
    focus.status_text = DRect::ZERO;
    focus.align = 0.5;
    focus.lyric.y = 92.0;
    focus.secondary.y = 116.0 + extra;
    focus.text_hit = DRect::new(focus_x, 20.0, focus_w, 48.0);

    let mut right = side;
    right.cover = DRect::new(width - 104.0, 20.0, 68.0, 68.0);
    right.title = DRect::new(36.0, 18.0, (width - 154.0).max(80.0), 24.0);
    right.artist = DRect::new(36.0, 45.0, (width - 154.0).max(80.0), 18.0);
    right.lyric = DRect::new(36.0, 68.0, (width - 154.0).max(80.0), line);
    right.secondary = DRect::new(36.0, 94.0 + extra, (width - 154.0).max(80.0), 18.0);
    right.text_hit = DRect::new(36.0, 18.0, (width - 154.0).max(80.0), 46.0);
    right.align = 0.5;
    right.split = 3.0;
    right.height = music_height(3.0, font);
    right.buttons = buttons_at(width, 201.0 + extra);
    right.status_dot = DRect::ZERO;
    right.status_text = DRect::ZERO;

    if mix <= 1.0 {
        let mut stacked = separated;
        stacked.lyric.x = side.lyric.x;
        stacked.lyric.w = side.lyric.w;
        stacked.secondary.x = side.secondary.x;
        stacked.secondary.w = side.secondary.w;
        let t = mix.clamp(0.0, 1.0);
        if t <= 0.5 {
            MusicGeometry::between(&side, &stacked, t * 2.0)
        } else {
            MusicGeometry::between(&stacked, &separated, (t - 0.5) * 2.0)
        }
    } else if mix <= 2.0 {
        MusicGeometry::between(&separated, &focus, (mix - 1.0).clamp(0.0, 1.0))
    } else {
        let t = (mix - 2.0).clamp(0.0, 1.0);
        let mut g = MusicGeometry::between(&focus, &right, t);
        let (anchor, scale) = if t < 0.5 {
            (focus.cover, 1.0 - t * 2.0)
        } else {
            (right.cover, (t - 0.5) * 2.0)
        };
        let cx = anchor.x + anchor.w * 0.5;
        let cy = anchor.y + anchor.h * 0.5;
        g.cover = DRect::new(
            cx - anchor.w * scale * 0.5,
            cy - anchor.h * scale * 0.5,
            anchor.w * scale,
            anchor.h * scale,
        );
        g
    }
}

// ============================================================================
// 2. 自定义布局模型与编辑器元素定位 (ported from custom_layout.rs)
// ============================================================================

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Module {
    Header,
    Lyrics,
    Translation,
    Status,
    Progress,
    Controls,
}

impl Module {
    pub const ALL: [Self; 6] = [
        Self::Header,
        Self::Lyrics,
        Self::Translation,
        Self::Status,
        Self::Progress,
        Self::Controls,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Header => "封面与歌曲信息",
            Self::Lyrics => "当前歌词",
            Self::Translation => "翻译与下一句",
            Self::Status => "播放状态",
            Self::Progress => "播放进度",
            Self::Controls => "播放控制",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub kind: Module,
    pub visible: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub element: u8,
    pub x: f32,
    pub y: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
}

pub const ALL_ELEMENT_IDS: [u8; 17] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 34, 41, 48, 35, 36, 37, 38, 39];

pub fn element_label(id: u8) -> &'static str {
    match id {
        0 => "专辑封面",
        1 => "歌曲标题",
        2 => "歌手",
        3 => "当前歌词",
        4 => "翻译与下一句",
        5 => "播放状态",
        6 => "播放进度条",
        7 => "当前时间",
        8 => "歌曲时长",
        34 => "播放模式",
        41 => "喜欢",
        35 => "上一首",
        36 => "播放 / 暂停",
        37 => "下一首",
        38 => "翻译按钮",
        39 => "设置按钮",
        48 => "音量",
        _ => "元素",
    }
}

pub fn resizable(id: u8) -> bool {
    [1, 2, 3, 4, 5, 6].contains(&id)
}

pub fn elements_of(g: &MusicGeometry) -> Vec<(u8, DRect)> {
    let mut out = vec![
        (0, g.cover),
        (1, g.title),
        (2, g.artist),
        (3, g.lyric),
        (4, g.secondary),
        (
            5,
            DRect::new(
                g.status_dot.x,
                g.status_text.y,
                (g.status_text.x + g.status_text.w - g.status_dot.x).max(0.0),
                g.status_text.h,
            ),
        ),
        (6, g.progress_hit),
        (7, g.elapsed),
        (8, g.total),
    ];
    out.extend(g.buttons.iter().map(|(id, r)| ((32 + *id) as u8, *r)));
    out.retain(|(_, r)| r.w > 0.0 && r.h > 0.0);
    out
}

fn translate_rect(r: &mut DRect, dx: f32, dy: f32) {
    if r.w > 0.0 && r.h > 0.0 {
        r.x += dx;
        r.y += dy;
    }
}

fn apply_placement(g: &mut MusicGeometry, p: &Placement, width: f32) {
    let Some((_, old)) = elements_of(g).into_iter().find(|(id, _)| *id == p.element) else {
        return;
    };
    let span = width / 400.0;
    let target_w = if resizable(p.element) {
        p.width.map(|w| w * span).unwrap_or(old.w).clamp(24.0, width - 24.0)
    } else {
        old.w
    };
    let x = (p.x * span).clamp(12.0, (width - target_w - 12.0).max(12.0));
    let y = p.y.clamp(12.0, 290.0);
    let (dx, dy) = (x - old.x, y - old.y);
    match p.element {
        0 => translate_rect(&mut g.cover, dx, dy),
        1 => {
            translate_rect(&mut g.title, dx, dy);
            g.title.w = target_w;
        }
        2 => {
            translate_rect(&mut g.artist, dx, dy);
            g.artist.w = target_w;
        }
        3 => {
            translate_rect(&mut g.lyric, dx, dy);
            g.lyric.w = target_w;
        }
        4 => {
            translate_rect(&mut g.secondary, dx, dy);
            g.secondary.w = target_w;
        }
        5 => {
            translate_rect(&mut g.status_dot, dx, dy);
            translate_rect(&mut g.status_text, dx, dy);
            g.status_text.w = (target_w - 10.0).max(14.0);
        }
        6 => {
            translate_rect(&mut g.track, dx, dy);
            translate_rect(&mut g.progress_hit, dx, dy);
            g.progress_hit.w = target_w;
            g.track.w = (target_w - 8.0).max(1.0);
        }
        7 => translate_rect(&mut g.elapsed, dx, dy),
        8 => translate_rect(&mut g.total, dx, dy),
        id => {
            if let Some((_, r)) = g.buttons.iter_mut().find(|(n, _)| 32 + *n == id as i32) {
                translate_rect(r, dx, dy);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CustomLayout {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<Placement>,
    pub version: u8,
    pub enabled: bool,
    pub modules: Vec<Slot>,
    pub alignment: u8,
    pub gap: f32,
    pub cover_size: f32,
    pub show_cover: bool,
    pub controls: Vec<i32>,
    pub transport_theme: String,
    pub bar_style: String,
    pub thumb_style: String,
    pub thumb_enabled: bool,
    pub thumb_spin: bool,
}

impl Default for CustomLayout {
    fn default() -> Self {
        Self {
            elements: Vec::new(),
            version: 1,
            enabled: false,
            modules: Module::ALL.map(|kind| Slot { kind, visible: true }).to_vec(),
            alignment: 0,
            gap: 8.0,
            cover_size: 56.0,
            show_cover: true,
            controls: vec![2, 9, 16, 3, 4, 5, 6, 7],
            transport_theme: "Reef".into(),
            bar_style: "Wavy".into(),
            thumb_style: "Pill".into(),
            thumb_enabled: false,
            thumb_spin: false,
        }
    }
}

impl CustomLayout {
    pub fn from_json_str(raw: &str) -> Self {
        if raw.trim().is_empty() {
            return Self::default();
        }
        serde_json::from_str::<Self>(raw)
            .ok()
            .filter(|l| l.validate().is_ok())
            .unwrap_or_default()
    }

    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.alignment > 2
            || !self.gap.is_finite()
            || !(4.0..=16.0).contains(&self.gap)
            || !self.cover_size.is_finite()
            || !(40.0..=80.0).contains(&self.cover_size)
        {
            return Err("布局版本、间距或封面尺寸无效".into());
        }
        if self.modules.len() != 6
            || Module::ALL
                .iter()
                .any(|kind| self.modules.iter().filter(|s| s.kind == *kind).count() != 1)
        {
            return Err("模块配置无效，请恢复推荐布局".into());
        }
        let valid_controls = [2, 9, 16, 3, 4, 5, 6, 7];
        if !self.controls.contains(&4)
            || self.controls.iter().any(|id| !valid_controls.contains(id))
        {
            return Err("播控按钮必须保留播放/暂停".into());
        }
        for i in 0..self.controls.len() {
            if self.controls[i + 1..].contains(&self.controls[i]) {
                return Err("播控按钮列表存在重复项".into());
            }
        }
        if self.elements.len() > 24 {
            return Err("元素数量异常".into());
        }
        for p in &self.elements {
            if !ALL_ELEMENT_IDS.contains(&p.element) || !p.x.is_finite() || !p.y.is_finite() {
                return Err("元素坐标无效".into());
            }
        }
        Ok(())
    }

    pub fn set_element(&mut self, element: u8, x: f32, y: f32, width: Option<f32>) {
        let sx = (x / 4.0).round() * 4.0;
        let sy = (y / 4.0).round() * 4.0;
        let sw = width.filter(|_| resizable(element)).map(|w| ((w / 4.0).round() * 4.0).clamp(24.0, 376.0));
        if let Some(slot) = self.elements.iter_mut().find(|p| p.element == element) {
            slot.x = sx.clamp(12.0, 388.0);
            slot.y = sy.clamp(12.0, 290.0);
            if sw.is_some() {
                slot.width = sw;
            }
        } else {
            self.elements.push(Placement {
                element,
                x: sx.clamp(12.0, 388.0),
                y: sy.clamp(12.0, 290.0),
                width: sw,
            });
        }
        self.enabled = true;
    }

    pub fn move_module(&mut self, from: usize, to: usize) {
        if from < self.modules.len() && to < self.modules.len() && from != to {
            self.modules.swap(from, to);
        }
    }

    pub fn geometry(&self, width: f32, font: f32) -> MusicGeometry {
        let zero = DRect::ZERO;
        let mut g = MusicGeometry {
            cover: zero,
            title: zero,
            artist: zero,
            lyric: zero,
            secondary: zero,
            status_dot: zero,
            status_text: zero,
            track: zero,
            elapsed: zero,
            total: zero,
            buttons: [(2, zero), (9, zero), (3, zero), (4, zero), (5, zero), (6, zero), (7, zero), (16, zero)],
            height: 0.0,
            progress_hit: zero,
            text_hit: zero,
            split: 0.0,
            align: self.alignment as f32 * 0.5,
        };
        let mut y = 18.0;
        let align = g.align;
        for slot in self.modules.iter().filter(|s| s.visible) {
            match slot.kind {
                Module::Header => {
                    let size = if self.show_cover { self.cover_size } else { 0.0 };
                    let tx = if size > 0.0 { size + 36.0 } else { 20.0 };
                    let tw = (width - tx - 20.0).max(1.0);
                    if size > 0.0 {
                        g.cover = DRect::new(20.0, y, size, size);
                    }
                    let h = size.max(48.0);
                    g.title = DRect::new(tx, y + (h - 46.0) * 0.5, tw, 24.0);
                    g.artist = DRect::new(tx, g.title.y + 28.0, tw, 18.0);
                    g.text_hit = DRect::new(tx, y, tw, h);
                    y += h;
                }
                Module::Lyrics => {
                    let h = font + 12.0;
                    g.lyric = DRect::new(20.0, y, width - 40.0, h);
                    y += h;
                }
                Module::Translation => {
                    g.secondary = DRect::new(20.0, y, width - 40.0, 18.0);
                    y += 18.0;
                }
                Module::Status => {
                    g.status_dot = DRect::new(20.0, y + 6.0, 4.0, 4.0);
                    g.status_text = DRect::new(30.0, y, width - 50.0, 16.0);
                    y += 16.0;
                }
                Module::Progress => {
                    g.track = DRect::new(20.0, y + 8.0, width - 40.0, 4.0);
                    g.progress_hit = DRect::new(16.0, y, width - 32.0, 22.0);
                    g.elapsed = DRect::new(20.0, y + 20.0, 70.0, 14.0);
                    g.total = DRect::new(width - 90.0, y + 20.0, 70.0, 14.0);
                    y += 42.0;
                }
                Module::Controls => {
                    let sizes: Vec<f32> = self
                        .controls
                        .iter()
                        .map(|id| if *id == 4 { 40.0 } else { 28.0 })
                        .collect();
                    let gap = 12.0;
                    let total = sizes.iter().sum::<f32>() + gap * (sizes.len().saturating_sub(1)) as f32;
                    let mut bx = 20.0 + (width - 40.0 - total) * align;
                    for (id, sz) in self.controls.iter().zip(sizes) {
                        if let Some((_, r)) = g.buttons.iter_mut().find(|(n, _)| n == id) {
                            *r = DRect::new(bx, y + (44.0 - sz) * 0.5, sz, sz);
                        }
                        bx += sz + gap;
                    }
                    y += 44.0;
                }
            }
            y += self.gap;
        }
        g.height = (y - self.gap + 18.0).max(110.0);
        for p in &self.elements {
            apply_placement(&mut g, p, width);
        }
        for (_, r) in elements_of(&g) {
            g.height = g.height.max(r.y + r.h + 18.0);
        }
        g.height = g.height.min(308.0);
        g.text_hit = DRect::new(
            g.title.x.min(g.artist.x),
            g.title.y.min(g.artist.y),
            (g.title.x + g.title.w).max(g.artist.x + g.artist.w) - g.title.x.min(g.artist.x),
            (g.title.y + g.title.h).max(g.artist.y + g.artist.h) - g.title.y.min(g.artist.y),
        );
        g
    }
}

pub fn active_geometry(
    cfg: &crate::auralsle_ai_status::config::Config,
    width: f32,
    live_mix: f32,
) -> (MusicGeometry, Option<CustomLayout>) {
    let custom = CustomLayout::from_json_str(&cfg.auralsle_custom_layout_json);
    if custom.enabled {
        let g = custom.geometry(width, cfg.auralsle_lyric_font_size);
        (g, Some(custom))
    } else {
        let layout = MusicLayout::from_setting(&cfg.auralsle_music_layout);
        let g = music_geometry(width, layout, live_mix, cfg.auralsle_lyric_font_size);
        (g, None)
    }
}

pub fn target_card_height(cfg: &crate::auralsle_ai_status::config::Config, live_mix: f32) -> f32 {
    let custom = CustomLayout::from_json_str(&cfg.auralsle_custom_layout_json);
    if custom.enabled {
        custom.geometry(419.0, cfg.auralsle_lyric_font_size).height
    } else {
        music_height(live_mix, cfg.auralsle_lyric_font_size)
    }
}

// ============================================================================
// 3. 动态取色与模糊封面缓存 (ported from cover_color.rs & renderer.rs)
// ============================================================================

#[derive(Clone, Copy, Debug)]
pub struct CoverPalette {
    pub background: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub text: Color32,
    pub muted: Color32,
}

impl Default for CoverPalette {
    fn default() -> Self {
        // Auralsle 标志性青薄荷中性暗色面板（匹配图一 Seventeen 单色封面视觉）
        Self {
            background: Color32::from_rgb(18, 35, 30),
            accent: Color32::from_rgb(152, 229, 194),
            on_accent: Color32::from_rgb(20, 32, 28),
            text: Color32::from_rgb(244, 246, 248),
            muted: Color32::from_rgb(174, 186, 182),
        }
    }
}

pub fn parse_hex_color(s: &str) -> Option<Color32> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color32::from_rgb(r, g, b))
}

fn luminance(c: Color32) -> f32 {
    let ch = |v: u8| {
        let f = v as f32 / 255.0;
        if f <= 0.04045 {
            f / 12.92
        } else {
            ((f + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
}

fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let la = luminance(a);
    let lb = luminance(b);
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) * 0.5;
    if (max - min).abs() < 1e-5 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let mut h = if (max - r).abs() < 1e-5 {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < 1e-5 {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h /= 6.0;
    (h * 360.0, s, l)
}

fn hsl_to_rgb(h_deg: f32, s: f32, l: f32) -> Color32 {
    let h = h_deg.rem_euclid(360.0) / 360.0;
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue2rgb = |t_in: f32| -> u8 {
        let mut t = t_in;
        if t < 0.0 { t += 1.0; }
        if t > 1.0 { t -= 1.0; }
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    Color32::from_rgb(hue2rgb(h + 1.0 / 3.0), hue2rgb(h), hue2rgb(h - 1.0 / 3.0))
}

fn palette_from_seed(seed: Color32) -> CoverPalette {
    let (h, s, _) = rgb_to_hsl(seed.r() as f32 / 255.0, seed.g() as f32 / 255.0, seed.b() as f32 / 255.0);
    if s < 0.09 {
        return CoverPalette::default();
    }
    let bg_s = (s * 0.42).clamp(0.18, 0.36);
    let background = hsl_to_rgb(h, bg_s, 0.13);
    let accent_s = (s * 0.65).clamp(0.32, 0.72);
    let mut l = 0.75;
    let mut accent = hsl_to_rgb(h, accent_s, l);
    while contrast_ratio(accent, background) < 4.5 && l < 0.94 {
        l += 0.03;
        accent = hsl_to_rgb(h, accent_s, l);
    }
    let on_accent = if contrast_ratio(accent, Color32::from_rgb(20, 26, 24)) >= contrast_ratio(accent, Color32::WHITE) {
        Color32::from_rgb(20, 26, 24)
    } else {
        Color32::WHITE
    };
    CoverPalette {
        background,
        accent,
        on_accent,
        text: Color32::from_rgb(244, 246, 248),
        muted: paint::mix(Color32::from_rgb(176, 184, 190), accent, 0.18),
    }
}

pub fn extract_cover_palette(image: &egui::ColorImage) -> CoverPalette {
    let n = image.pixels.len();
    if n == 0 {
        return CoverPalette::default();
    }
    let step = (n / 360).max(1);
    let mut hue_bins = [0.0f32; 36];
    let mut hue_r = [0.0f32; 36];
    let mut hue_g = [0.0f32; 36];
    let mut hue_b = [0.0f32; 36];
    let mut colorful_total = 0.0f32;
    let mut sampled_total = 0.0f32;

    for px in image.pixels.iter().step_by(step) {
        if px.a() < 180 {
            continue;
        }
        sampled_total += 1.0;
        let (r, g, b) = (px.r() as f32 / 255.0, px.g() as f32 / 255.0, px.b() as f32 / 255.0);
        let (h, s, l) = rgb_to_hsl(r, g, b);
        if s < 0.16 || !(0.08..=0.92).contains(&l) {
            continue;
        }
        let weight = s * (1.0 - (l - 0.5).abs() * 1.1).max(0.2);
        let bin = ((h / 10.0).floor() as usize).min(35);
        hue_bins[bin] += weight;
        hue_r[bin] += r * weight;
        hue_g[bin] += g * weight;
        hue_b[bin] += b * weight;
        colorful_total += 1.0;
    }

    // 单色/极低彩度封面（如 Seventeen 肖像）保留柔和青薄荷默认盘，避免灰度噪点误染色
    if sampled_total < 8.0 || colorful_total / sampled_total < 0.05 {
        return CoverPalette::default();
    }
    let mut best_bin = 0;
    let mut best_score = 0.0f32;
    for i in 0..36 {
        let prev = (i + 35) % 36;
        let next = (i + 1) % 36;
        let score = hue_bins[i] + 0.45 * (hue_bins[prev] + hue_bins[next]);
        if score > best_score {
            best_score = score;
            best_bin = i;
        }
    }
    let w = hue_bins[best_bin].max(1e-4);
    let seed = Color32::from_rgb(
        (hue_r[best_bin] / w * 255.0).clamp(0.0, 255.0) as u8,
        (hue_g[best_bin] / w * 255.0).clamp(0.0, 255.0) as u8,
        (hue_b[best_bin] / w * 255.0).clamp(0.0, 255.0) as u8,
    );
    palette_from_seed(seed)
}

#[cfg(windows)]
fn windows_accent_color() -> Color32 {
    use windows::{core::w, Win32::System::Registry::*};
    let mut val = 0u32;
    let mut sz = 4u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\DWM"),
            w!("AccentColor"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut val as *mut u32).cast()),
            Some(&mut sz),
        )
        .is_ok()
    };
    if ok && val != 0 {
        // DWORD is AABBGGRR
        let r = (val & 0xff) as u8;
        let g = ((val >> 8) & 0xff) as u8;
        let b = ((val >> 16) & 0xff) as u8;
        Color32::from_rgb(r, g, b)
    } else {
        Color32::from_rgb(152, 229, 194)
    }
}

#[cfg(not(windows))]
fn windows_accent_color() -> Color32 {
    Color32::from_rgb(152, 229, 194)
}

pub fn resolve_palette(
    cfg: &crate::auralsle_ai_status::config::Config,
    artwork: Option<&egui::ColorImage>,
) -> (CoverPalette, Color32) {
    let source_palette = match cfg.auralsle_color_source.as_str() {
        "System" => palette_from_seed(windows_accent_color()),
        "Custom" => palette_from_seed(
            parse_hex_color(&cfg.auralsle_dynamic_seed).unwrap_or(Color32::from_rgb(169, 230, 205)),
        ),
        _ => artwork.map(extract_cover_palette).unwrap_or_default(),
    };
    let mut palette = if cfg.auralsle_expanded_dynamic_color {
        source_palette
    } else {
        let mut fixed = CoverPalette::default();
        fixed.background = Color32::from_rgb(24, 27, 32);
        if let Some(c) = parse_hex_color(&cfg.auralsle_text_color) {
            fixed.text = c;
            fixed.muted = paint::alpha(c, 0.72);
        }
        if let Some(c) = parse_hex_color(&cfg.auralsle_component_color) {
            fixed.accent = c;
            fixed.on_accent = if contrast_ratio(c, Color32::from_rgb(20, 26, 24))
                >= contrast_ratio(c, Color32::WHITE)
            {
                Color32::from_rgb(20, 26, 24)
            } else {
                Color32::WHITE
            };
        }
        fixed
    };
    if !cfg.auralsle_expanded_dynamic_color {
        if let Some(c) = parse_hex_color(&cfg.auralsle_text_color) {
            palette.text = c;
            palette.muted = paint::alpha(c, 0.72);
        }
    }
    let lyric_highlight = if cfg.auralsle_lyric_cover_color {
        source_palette.accent
    } else {
        parse_hex_color(&cfg.auralsle_lyric_progress_color)
            .unwrap_or(Color32::from_rgb(169, 230, 205))
    };
    (palette, lyric_highlight)
}

#[derive(Default)]
pub struct CoverBlurCache {
    key: Option<(usize, u32)>,
    texture: Option<egui::TextureHandle>,
}

impl CoverBlurCache {
    pub fn texture_only(&self) -> Option<((usize, u32), &egui::TextureHandle)> {
        self.texture.as_ref().map(|t| (self.key.unwrap_or((0, 0)), t))
    }
    pub fn get_or_build(
        &mut self,
        ctx: &egui::Context,
        artwork: Option<&(Arc<egui::ColorImage>, egui::TextureHandle)>,
        frost: f32,
    ) -> Option<egui::TextureHandle> {
        let (img, _) = artwork?;
        let blur_sigma = ((frost.clamp(0.0, 100.0) * 0.24).round() as u32).clamp(2, 24);
        let ptr_key = Arc::as_ptr(img) as usize;
        let key = (ptr_key, blur_sigma);
        if self.key == Some(key) {
            return self.texture.clone();
        }
        let [w, h] = img.size;
        let raw_rgba: Vec<u8> = img
            .pixels
            .iter()
            .flat_map(|c| [c.r(), c.g(), c.b(), c.a()])
            .collect();
        let Some(rgba_img) = image::RgbaImage::from_raw(w as u32, h as u32, raw_rgba) else {
            return None;
        };
        let small = image::imageops::thumbnail(&rgba_img, 128, 128);
        let ratio = (small.width() as f32 / (w.max(1) as f32)).clamp(0.08, 1.0);
        let sigma = (blur_sigma as f32 * ratio * 1.8).clamp(1.2, 12.0);
        let blurred = image::imageops::blur(&small, sigma);
        let (bw, bh) = blurred.dimensions();
        let mut pixels = blurred.into_raw();
        for ch in pixels.chunks_exact_mut(4) {
            ch[0] = (ch[0] as u32 * 58 / 100) as u8;
            ch[1] = (ch[1] as u32 * 58 / 100) as u8;
            ch[2] = (ch[2] as u32 * 58 / 100) as u8;
        }
        let color_img = egui::ColorImage::from_rgba_unmultiplied([bw as usize, bh as usize], &pixels);
        let handle = ctx.load_texture("auralsle-blurred-cover", color_img, egui::TextureOptions::LINEAR);
        self.key = Some(key);
        self.texture = Some(handle.clone());
        Some(handle)
    }
}

pub fn paint_music_surface(
    c: &Card,
    width: f32,
    height: f32,
    radius: f32,
    palette: &CoverPalette,
    blurred_tex: Option<&egui::TextureHandle>,
    frost: f32,
    cover_overlay: bool,
    cover_opacity_pct: f32,
    is_glass: bool,
) {
    let top_col = paint::mix(palette.background, palette.accent, 0.10);
    let bot_col = paint::mix(palette.background, Color32::BLACK, 0.24);
    c.vgrad(0.0, 0.0, width, height, radius, top_col, bot_col);

    if cover_overlay {
        if let Some(tex) = blurred_tex {
            let overlay_a = (cover_opacity_pct.clamp(0.0, 100.0) / 100.0 * 0.85).clamp(0.12, 0.85);
            let card_rect = c.rect(0.0, 0.0, width, height);
            let [tw, th] = tex.size();
            let (tw, th) = (tw.max(1) as f32, th.max(1) as f32);
            let card_aspect = width / height.max(1.0);
            let tex_aspect = tw / th;
            let (uw, vh) = if tex_aspect > card_aspect {
                (card_aspect / tex_aspect, 1.0)
            } else {
                (1.0, tex_aspect / card_aspect)
            };
            let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), Vec2::new(uw, vh));
            let clipped = c.clipped(0.0, 0.0, width, height);
            let mut shape = egui::epaint::RectShape::filled(
                card_rect,
                CornerRadius::same((radius * c.s).round().clamp(0.0, 255.0) as u8),
                Color32::from_white_alpha((overlay_a * 255.0) as u8),
            );
            shape = shape.with_texture(tex.id(), uv);
            clipped.painter.add(shape);
            // 叠加暗色可读性压层（严格使用圆角填充，避免大半径 glow 溢出四角形成直角灰边）
            clipped.fill(
                0.0,
                0.0,
                width,
                height,
                radius,
                paint::alpha(paint::mix(Color32::from_rgb(14, 18, 21), palette.background, 0.42), 0.36),
            );
        } else {
            let inner_r = (height * 0.42).min(width * 0.28).max(12.0);
            let clipped = c.clipped(0.0, 0.0, width, height);
            clipped.glow(width * 0.5, height * 0.5, inner_r, paint::alpha(palette.accent, 0.10));
        }
    }

    let frost_norm = (frost.clamp(0.0, 100.0) / 100.0) * if is_glass { 0.45 } else { 1.0 };
    if frost_norm > 0.01 {
        let hi_a = (frost_norm * 14.0).round() as u8;
        c.vgrad(
            0.0,
            0.0,
            width,
            height,
            radius,
            Color32::from_white_alpha(hi_a),
            Color32::TRANSPARENT,
        );
    }

    // 已按需求移除 Auralsle 外圈白色边框
}

// ============================================================================
// 4. 8 种进度条样式 + 16 种游标图形 (ported from auralsle-render/src/progress.rs)
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarStyle {
    Normal,
    Wavy,
    Rounded,
    Thin,
    Thick,
    Gradient,
    Segmented,
    Dots,
}

impl BarStyle {
    pub const ALL: [Self; 8] = [
        Self::Normal,
        Self::Wavy,
        Self::Rounded,
        Self::Thin,
        Self::Thick,
        Self::Gradient,
        Self::Segmented,
        Self::Dots,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Wavy => "Wavy",
            Self::Rounded => "Rounded",
            Self::Thin => "Thin",
            Self::Thick => "Thick",
            Self::Gradient => "Gradient",
            Self::Segmented => "Segmented",
            Self::Dots => "Dots",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "标准",
            Self::Wavy => "波浪",
            Self::Rounded => "圆角",
            Self::Thin => "细线",
            Self::Thick => "粗线",
            Self::Gradient => "渐变",
            Self::Segmented => "分段",
            Self::Dots => "圆点",
        }
    }
    pub fn from_setting(s: &str) -> Self {
        Self::ALL.into_iter().find(|b| b.key() == s).unwrap_or(Self::Rounded)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbStyle {
    None,
    Default,
    Circle,
    Square,
    Pill,
    Diamond,
    Flower,
    Heart,
    Cookie,
    Puffy,
    Clover,
    Clover8,
    Gem,
    Triangle,
    Pentagon,
    Cookie12,
    Clam,
}

impl ThumbStyle {
    pub const ALL: [Self; 16] = [
        Self::Default,
        Self::Circle,
        Self::Square,
        Self::Pill,
        Self::Diamond,
        Self::Flower,
        Self::Heart,
        Self::Cookie,
        Self::Puffy,
        Self::Clover,
        Self::Clover8,
        Self::Gem,
        Self::Triangle,
        Self::Pentagon,
        Self::Cookie12,
        Self::Clam,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Default => "Default",
            Self::Circle => "Circle",
            Self::Square => "Square",
            Self::Pill => "Pill",
            Self::Diamond => "Diamond",
            Self::Flower => "Flower",
            Self::Heart => "Heart",
            Self::Cookie => "Cookie",
            Self::Puffy => "Puffy",
            Self::Clover => "Clover",
            Self::Clover8 => "Clover8",
            Self::Gem => "Gem",
            Self::Triangle => "Triangle",
            Self::Pentagon => "Pentagon",
            Self::Cookie12 => "Cookie12",
            Self::Clam => "Clam",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "无",
            Self::Default => "默认",
            Self::Circle => "圆",
            Self::Square => "方",
            Self::Pill => "胶囊",
            Self::Diamond => "菱形",
            Self::Flower => "花",
            Self::Heart => "爱心",
            Self::Cookie => "饼干",
            Self::Puffy => "蓬松",
            Self::Clover => "四叶",
            Self::Clover8 => "八叶",
            Self::Gem => "宝石",
            Self::Triangle => "三角",
            Self::Pentagon => "五角",
            Self::Cookie12 => "十二叶",
            Self::Clam => "贝壳",
        }
    }
    pub fn from_setting(s: &str) -> Self {
        Self::ALL.into_iter().find(|t| t.key() == s).unwrap_or(Self::Pill)
    }
}

fn fill_fan_points(c: &Card, center: (f32, f32), pts: &[(f32, f32)], spin: f32, col: Color32) {
    if pts.len() < 3 {
        return;
    }
    let rot = |p: (f32, f32)| -> Pos2 {
        let dx = p.0 - center.0;
        let dy = p.1 - center.1;
        let (s, cs) = spin.sin_cos();
        c.pos(center.0 + dx * cs - dy * s, center.1 + dx * s + dy * cs)
    };
    let mut mesh = Mesh::default();
    let cp = rot(center);
    mesh.colored_vertex(cp, col);
    for &p in pts {
        mesh.colored_vertex(rot(p), col);
    }
    let n = pts.len() as u32;
    for i in 0..n {
        let next = (i + 1) % n;
        mesh.add_triangle(0, 1 + i, 1 + next);
    }
    c.painter.add(egui::Shape::mesh(mesh));
}

fn draw_thumb(c: &Card, style: ThumbStyle, cx: f32, cy: f32, size: f32, spin: f32, fg: Color32) {
    if style == ThumbStyle::None {
        return;
    }
    let r = size * 0.5;
    let rot_pt = |dx: f32, dy: f32| -> (f32, f32) {
        let (s, cs) = spin.sin_cos();
        (cx + dx * cs - dy * s, cy + dx * s + dy * cs)
    };
    match style {
        ThumbStyle::None => {}
        ThumbStyle::Default => c.circle(cx, cy, r, fg),
        ThumbStyle::Circle => c.circle(cx, cy, r * 0.82, fg),
        ThumbStyle::Square => {
            if spin.abs() < 1e-4 {
                c.fill(cx - size * 0.4, cy - size * 0.4, size * 0.8, size * 0.8, size * 0.2, fg);
            } else {
                let hw = size * 0.4;
                let pts = vec![
                    rot_pt(-hw, -hw),
                    rot_pt(hw, -hw),
                    rot_pt(hw, hw),
                    rot_pt(-hw, hw),
                ];
                fill_fan_points(c, (cx, cy), &pts, 0.0, fg);
            }
        }
        ThumbStyle::Pill => {
            if spin.abs() < 1e-4 {
                c.fill(cx - size * 0.14, cy - size * 0.5, size * 0.28, size, size * 0.14, fg);
            } else {
                let hw = size * 0.14;
                let hh = size * 0.5;
                let pts = vec![
                    rot_pt(-hw, -hh),
                    rot_pt(hw, -hh),
                    rot_pt(hw, hh),
                    rot_pt(-hw, hh),
                ];
                fill_fan_points(c, (cx, cy), &pts, 0.0, fg);
            }
        }
        ThumbStyle::Diamond | ThumbStyle::Triangle | ThumbStyle::Pentagon => {
            let sides = match style {
                ThumbStyle::Diamond => 4,
                ThumbStyle::Triangle => 3,
                _ => 5,
            };
            let pts: Vec<(f32, f32)> = (0..sides)
                .map(|i| {
                    let a = -FRAC_PI_2 + TAU * i as f32 / sides as f32;
                    (cx + a.cos() * r, cy + a.sin() * r)
                })
                .collect();
            fill_fan_points(c, (cx, cy), &pts, spin, fg);
        }
        ThumbStyle::Flower => {
            for i in 0..8 {
                let a = -FRAC_PI_2 + TAU * i as f32 / 8.0 + spin;
                c.circle(cx + a.cos() * r * 0.5, cy + a.sin() * r * 0.5, r * 0.4, fg);
            }
            c.circle(cx, cy, r * 0.4, fg);
        }
        ThumbStyle::Clover => {
            for i in 0..4 {
                let a = -FRAC_PI_2 + TAU * i as f32 / 4.0 + spin;
                c.circle(cx + a.cos() * r * 0.42, cy + a.sin() * r * 0.42, r * 0.52, fg);
            }
            c.circle(cx, cy, r * 0.46, fg);
        }
        ThumbStyle::Clover8 => {
            for i in 0..8 {
                let a = -FRAC_PI_2 + TAU * i as f32 / 8.0 + spin;
                c.circle(cx + a.cos() * r * 0.52, cy + a.sin() * r * 0.52, r * 0.36, fg);
            }
            c.circle(cx, cy, r * 0.34, fg);
        }
        ThumbStyle::Puffy => {
            for i in 0..4 {
                let a = -FRAC_PI_2 + TAU * i as f32 / 4.0 + spin;
                c.circle(cx + a.cos() * r * 0.33, cy + a.sin() * r * 0.33, r * 0.55, fg);
            }
            c.circle(cx, cy, r * 0.5, fg);
        }
        ThumbStyle::Cookie | ThumbStyle::Cookie12 => {
            let lobes = if style == ThumbStyle::Cookie { 8 } else { 12 };
            let inner = if style == ThumbStyle::Cookie { r * 0.80 } else { r * 0.84 };
            let n = lobes * 10;
            let pts: Vec<(f32, f32)> = (0..n)
                .map(|i| {
                    let a = TAU * i as f32 / n as f32;
                    let bump = 0.5 + 0.5 * (a * lobes as f32).cos();
                    let rad = inner + (r - inner) * bump;
                    (cx + a.cos() * rad, cy + a.sin() * rad)
                })
                .collect();
            fill_fan_points(c, (cx, cy), &pts, spin, fg);
        }
        ThumbStyle::Heart => {
            let s = r * 0.95;
            let p = |x: f32, y: f32| (cx + x * s, cy + y * s);
            let c0 = p(0.0, 0.95);
            let mut pts = Vec::with_capacity(48);
            let eval = |p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), out: &mut Vec<(f32, f32)>| {
                for step in 0..10 {
                    let t = step as f32 / 10.0;
                    let u = 1.0 - t;
                    let x = u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0;
                    let y = u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1;
                    out.push((x, y));
                }
            };
            eval(c0, p(-0.18, 0.62), p(-0.98, 0.3), p(-0.95, -0.22), &mut pts);
            eval(p(-0.95, -0.22), p(-0.92, -0.78), p(-0.3, -0.95), p(0.0, -0.5), &mut pts);
            eval(p(0.0, -0.5), p(0.3, -0.95), p(0.92, -0.78), p(0.95, -0.22), &mut pts);
            eval(p(0.95, -0.22), p(0.98, 0.3), p(0.18, 0.62), c0, &mut pts);
            fill_fan_points(c, (cx, cy + 0.05 * s), &pts, spin, fg);
        }
        ThumbStyle::Gem => {
            let s = r * 0.95;
            let pts = vec![
                (cx + s, cy),
                (cx + 0.25 * s, cy - 0.68 * s),
                (cx - 0.72 * s, cy - 0.56 * s),
                (cx - 0.95 * s, cy),
                (cx - 0.72 * s, cy + 0.56 * s),
                (cx + 0.25 * s, cy + 0.68 * s),
            ];
            fill_fan_points(c, (cx, cy), &pts, spin, fg);
        }
        ThumbStyle::Clam => {
            let mut pts = Vec::new();
            let steps = 10;
            for i in 0..=steps {
                let a = PI + PI * i as f32 / steps as f32;
                let rad = if i % 2 == 0 { r } else { r * 0.87 };
                pts.push((cx + a.cos() * rad, cy + a.sin() * rad));
            }
            pts.push((cx + 0.35 * r, cy + 0.55 * r));
            pts.push((cx, cy + 0.85 * r));
            pts.push((cx - 0.35 * r, cy + 0.55 * r));
            fill_fan_points(c, (cx, cy + 0.1 * r), &pts, spin, fg);
        }
    }
}

pub fn draw_progress_bar(
    c: &Card,
    r: DRect,
    fraction: f32,
    phase: f32,
    unit: f32,
    thumb_size: f32,
    wave: f32,
    spin: f32,
    fg: Color32,
    track: Color32,
    dot: Color32,
    bar: BarStyle,
    thumb: ThumbStyle,
) {
    let frac = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    let unit = unit.max(0.2);
    let mid = r.y + r.h * 0.5;
    let end = r.x + r.w * frac;
    let wave = wave.clamp(0.0, 1.0);

    match bar {
        BarStyle::Normal | BarStyle::Rounded | BarStyle::Thick | BarStyle::Gradient => {
            let h = if bar == BarStyle::Thick { 12.0 * unit } else { 7.5 * unit };
            let corner = if bar == BarStyle::Normal { 2.0 * unit } else { h * 0.5 };
            c.fill(r.x, mid - h * 0.5, r.w, h, corner, track);
            if end > r.x + 0.5 {
                let fw = (end - r.x).max(h * 0.5);
                if bar == BarStyle::Gradient {
                    let right_col = paint::mix(fg, Color32::WHITE, 0.34);
                    c.hgrad(r.x, mid - h * 0.5, fw, h, corner, fg, right_col);
                } else {
                    c.fill(r.x, mid - h * 0.5, fw, h, corner, fg);
                }
            }
            if frac < 0.985 {
                c.circle(r.x + r.w - 2.4 * unit - 0.5, mid, 2.2 * unit, dot);
            }
        }
        BarStyle::Wavy => {
            let h = (2.2 * unit).max(1.2);
            c.fill(r.x, mid - h * 0.5, r.w, h, h * 0.5, track);
            let len = end - r.x;
            if len > 1.5 {
                let count = (len / 3.0).ceil().max(2.0) as usize;
                let mut pts = Vec::with_capacity(count + 1);
                for i in 0..=count {
                    let x = r.x + len * i as f32 / count as f32;
                    let edge = ((x - r.x) / (10.0 * unit)).min((end - x) / (10.0 * unit)).clamp(0.0, 1.0);
                    let y = mid + edge * 2.5 * unit * wave * ((x - r.x) / (26.0 * unit) * TAU - phase).sin();
                    pts.push((x, y));
                }
                c.path(&pts, 4.0 * unit, fg);
                c.circle(pts[0].0, pts[0].1, 2.0 * unit, fg);
                if let Some(&last) = pts.last() {
                    c.circle(last.0, last.1, 2.0 * unit, fg);
                }
            }
            if frac < 0.985 {
                c.circle(r.x + r.w - 2.0 * unit - 0.5, mid, 2.0 * unit, dot);
            }
        }
        BarStyle::Thin => {
            let h = (1.6 * unit).max(1.0);
            c.fill(r.x, mid - h * 0.5, r.w, h, h * 0.5, track);
            if end > r.x + 0.5 {
                let fh = 3.0 * unit;
                c.fill(r.x, mid - fh * 0.5, (end - r.x).max(fh), fh, fh * 0.5, fg);
            }
            if frac < 0.985 {
                c.circle(r.x + r.w - 2.0 * unit - 0.5, mid, 2.0 * unit, dot);
            }
        }
        BarStyle::Segmented => {
            let tick = 3.5 * unit;
            let h = 11.5 * unit;
            let period = 20.0 * unit;
            let count = (r.w / period).floor() as usize;
            for i in 0..=count {
                let x = r.x + i as f32 * period;
                if x + tick > r.x + r.w + 0.01 {
                    break;
                }
                let lit = x + tick * 0.5 <= end;
                c.fill(x, mid - h * 0.5, tick, h, tick * 0.5, if lit { fg } else { track });
            }
        }
        BarStyle::Dots => {
            let d = 8.5 * unit;
            let period = 22.0 * unit;
            let count = (r.w / period).floor() as usize;
            for i in 0..=count {
                let cx_i = r.x + d * 0.5 + i as f32 * period;
                if cx_i + d * 0.5 > r.x + r.w + 0.01 {
                    break;
                }
                c.circle(cx_i, mid, d * 0.5, if cx_i <= end { fg } else { track });
            }
        }
    }

    if thumb != ThumbStyle::None {
        draw_thumb(c, thumb, end, mid, thumb_size.max(6.0), spin, fg);
    }
}

// ============================================================================
// 5. 10 套播放按钮主题与 8 个高精矢量控制图标 (ported from transport.rs)
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportTheme {
    Reef,
    Ripple,
    Sand,
    Mist,
    Glaze,
    InkIsle,
    Tide,
    Whitespace,
    Halo,
    Layered,
}

impl TransportTheme {
    pub const ALL: [Self; 10] = [
        Self::Reef,
        Self::Ripple,
        Self::Sand,
        Self::Mist,
        Self::Glaze,
        Self::InkIsle,
        Self::Tide,
        Self::Whitespace,
        Self::Halo,
        Self::Layered,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Reef => "Reef",
            Self::Ripple => "Ripple",
            Self::Sand => "Sand",
            Self::Mist => "Mist",
            Self::Glaze => "Glaze",
            Self::InkIsle => "InkIsle",
            Self::Tide => "Tide",
            Self::Whitespace => "Whitespace",
            Self::Halo => "Halo",
            Self::Layered => "Layered",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Reef => "礁石",
            Self::Ripple => "涟漪",
            Self::Sand => "细砂",
            Self::Mist => "晨雾",
            Self::Glaze => "琉璃",
            Self::InkIsle => "墨屿",
            Self::Tide => "双潮",
            Self::Whitespace => "留白",
            Self::Halo => "光环",
            Self::Layered => "叠影",
        }
    }
    pub fn from_setting(s: &str) -> Self {
        Self::ALL.into_iter().find(|t| t.key() == s).unwrap_or(Self::Reef)
    }
}

pub fn draw_transport_glyph(
    c: &Card,
    id: i32,
    cx: f32,
    cy: f32,
    ink: Color32,
    play_morph: f32,
    liked: bool,
    shuffle_mode: u8,
    muted: bool,
) {
    match id {
        2 => {
            // 播放模式：图一左侧四向/随机交叉箭头
            if shuffle_mode % 2 == 0 {
                // 四向展开交叉箭（匹配图一左下图标）
                let arm = 5.4;
                for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    c.seg((cx, cy), (cx + dx * arm, cy + dy * arm), 1.5, ink);
                    c.seg((cx + dx * arm, cy + dy * arm), (cx + dx * (arm - 3.0), cy + dy * arm), 1.4, ink);
                    c.seg((cx + dx * arm, cy + dy * arm), (cx + dx * arm, cy + dy * (arm - 3.0)), 1.4, ink);
                }
            } else {
                // 顺序双轨循环
                c.path(&[(cx - 6.0, cy - 3.0), (cx + 5.5, cy - 3.0), (cx + 3.0, cy - 5.5)], 1.5, ink);
                c.path(&[(cx + 6.0, cy + 3.0), (cx - 5.5, cy + 3.0), (cx - 3.0, cy + 5.5)], 1.5, ink);
            }
        }
        9 => {
            // 喜欢爱心（描边 / 实心高亮）
            let s = 6.2;
            let mut pts = Vec::with_capacity(28);
            let p = |x: f32, y: f32| (cx + x * s, cy + y * s);
            let eval = |p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), out: &mut Vec<(f32, f32)>| {
                for step in 0..8 {
                    let t = step as f32 / 8.0;
                    let u = 1.0 - t;
                    let x = u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0;
                    let y = u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1;
                    out.push((x, y));
                }
            };
            eval(p(0.0, 0.90), p(-0.2, 0.58), p(-0.98, 0.25), p(-0.95, -0.25), &mut pts);
            eval(p(-0.95, -0.25), p(-0.90, -0.78), p(-0.28, -0.92), p(0.0, -0.45), &mut pts);
            eval(p(0.0, -0.45), p(0.28, -0.92), p(0.90, -0.78), p(0.95, -0.25), &mut pts);
            eval(p(0.95, -0.25), p(0.98, 0.25), p(0.2, 0.58), p(0.0, 0.90), &mut pts);
            if liked {
                fill_fan_points(c, (cx, cy + 0.05 * s), &pts, 0.0, ink);
            } else {
                pts.push(pts[0]);
                c.path(&pts, 1.55, ink);
            }
        }
        16 => {
            paint::speaker(c, cx, cy, muted, ink);
        }
        3 => {
            paint::skip(c, cx, cy, -1.0, ink);
        }
        4 => {
            paint::play_pause(c, cx, cy, 13.5, play_morph, ink);
        }
        5 => {
            paint::skip(c, cx, cy, 1.0, ink);
        }
        6 => {
            // 文A 双语翻译按钮（匹配图一右下圆底 文A 徽标）
            c.centered_text(cx - 3.6, cy - 1.2, "文", 11.0, W::Semibold, ink, H::Center);
            c.centered_text(cx + 4.4, cy + 2.8, "A", 9.0, W::Bold, ink, H::Center);
        }
        7 => {
            paint::gear(c, cx, cy, ink);
        }
        _ => {}
    }
}

pub fn draw_transport_button_surface(
    c: &Card,
    id: i32,
    r: DRect,
    theme: TransportTheme,
    accent: Color32,
    on_accent: Color32,
    control: Color32,
    tonal: Color32,
    ink_fill: Color32,
    play_morph: f32,
    hover: f32,
) -> Color32 {
    let rounded = 12.0_f32.min(r.h * 0.5);
    let hover_boost = hover * 0.14;
    match id {
        4 => match theme {
            TransportTheme::Reef => {
                // Reef 核心标志：暂停时纯圆，播放时变为柔和方圆 Squircle
                let rad = r.h * (0.5 - 0.20 * play_morph.clamp(0.0, 1.0));
                c.fill(r.x, r.y, r.w, r.h, rad, paint::mix(accent, Color32::WHITE, hover_boost));
                on_accent
            }
            TransportTheme::Ripple => {
                c.fill(r.x, r.y, r.w, r.h, r.h * 0.5, paint::mix(accent, Color32::WHITE, hover_boost));
                on_accent
            }
            TransportTheme::Sand => {
                if hover > 0.01 {
                    c.fill(r.x, r.y, r.w, r.h, rounded, paint::alpha(accent, hover * 0.16));
                }
                c.stroke(r.x, r.y, r.w, r.h, rounded, 1.2, paint::alpha(accent, 0.78));
                accent
            }
            TransportTheme::Mist => {
                c.fill(r.x, r.y, r.w, r.h, rounded, paint::alpha(accent, 0.22 + hover * 0.10));
                c.stroke(r.x, r.y, r.w, r.h, rounded, 1.0, paint::alpha(accent, 0.42));
                accent
            }
            TransportTheme::Glaze => {
                c.fill(r.x, r.y, r.w, r.h, rounded, paint::alpha(accent, 0.30 + hover * 0.12));
                c.stroke(r.x, r.y, r.w, r.h, rounded, 1.0, paint::alpha(accent, 0.65));
                accent
            }
            TransportTheme::InkIsle => {
                let tinted_ink = paint::mix(accent, ink_fill, 0.28);
                c.fill(r.x, r.y, r.w, r.h, rounded, paint::mix(tinted_ink, Color32::WHITE, hover_boost * 0.5));
                on_accent
            }
            TransportTheme::Tide => {
                c.fill(r.x, r.y, r.w, r.h, rounded, paint::mix(accent, Color32::WHITE, hover_boost));
                on_accent
            }
            TransportTheme::Whitespace => {
                if hover > 0.01 {
                    c.circle(r.x + r.w * 0.5, r.y + r.h * 0.5, r.w * 0.45, paint::alpha(accent, hover * 0.15));
                }
                accent
            }
            TransportTheme::Halo => {
                if hover > 0.01 {
                    c.circle(r.x + r.w * 0.5, r.y + r.h * 0.5, r.h * 0.5 - 2.0, paint::alpha(accent, hover * 0.15));
                }
                c.ring(r.x + r.w * 0.5, r.y + r.h * 0.5, r.h * 0.5 - 2.0, 2.2, accent);
                accent
            }
            TransportTheme::Layered => {
                c.fill(r.x, r.y + 2.2, r.w, r.h, rounded, Color32::from_black_alpha(46));
                c.fill(r.x, r.y, r.w, r.h, rounded, paint::mix(accent, Color32::WHITE, hover_boost));
                on_accent
            }
        },
        6 => {
            match theme {
                TransportTheme::Reef => {
                    c.fill(r.x, r.y, r.w, r.h, r.h * 0.5, Color32::from_black_alpha(48));
                }
                TransportTheme::Ripple
                | TransportTheme::Mist
                | TransportTheme::Glaze
                | TransportTheme::InkIsle
                | TransportTheme::Tide
                | TransportTheme::Layered => {
                    c.fill(r.x, r.y, r.w, r.h, r.h * 0.5, paint::alpha(tonal, 0.55));
                }
                _ => {}
            }
            if hover > 0.01 {
                c.circle(r.x + r.w * 0.5, r.y + r.h * 0.5, r.h * 0.5, Color32::from_white_alpha((hover * 24.0) as u8));
            }
            control
        }
        3 | 5 => {
            if theme == TransportTheme::Tide {
                c.fill(r.x, r.y, r.w, r.h, r.h * 0.5, paint::alpha(tonal, 0.45));
            }
            if hover > 0.01 {
                c.circle(r.x + r.w * 0.5, r.y + r.h * 0.5, r.h * 0.48, Color32::from_white_alpha((hover * 22.0) as u8));
            }
            control
        }
        _ => {
            if hover > 0.01 {
                c.circle(r.x + r.w * 0.5, r.y + r.h * 0.5, r.h * 0.48, Color32::from_white_alpha((hover * 22.0) as u8));
            }
            accent
        }
    }
}

// ============================================================================
// 6. 发光逐字歌词渲染 (bloom glow + sweep highlight + slide-up)
// ============================================================================

pub fn draw_glowing_lyric(
    c: &Card,
    text: &str,
    rect: DRect,
    font_size: f32,
    align: f32,
    highlight: f32,
    progress_color: Color32,
    base_ink: Color32,
    age_phase: f32,
    word_highlight: bool,
) {
    let clean = text.trim();
    if clean.is_empty() || rect.w <= 8.0 {
        return;
    }
    let phase = age_phase.clamp(0.0, 1.0);
    let dy = 4.0 * (1.0 - phase).powi(3);
    let weight = if font_size >= 18.0 { W::Bold } else { W::Semibold };
    let fitted = c.fit(clean, font_size, weight, rect.w);
    let text_w = c.text_w(&fitted, font_size, weight).min(rect.w);
    let x = rect.x + (rect.w - text_w) * align.clamp(0.0, 1.0);
    let cy = rect.y + rect.h * 0.5 + dy;
    let hi = if word_highlight { highlight.clamp(0.0, 1.0) } else { 1.0 };
    let boundary = x + text_w * hi;
    let row = c.faded(phase);

    // 已唱部分：柔和多向辉光 + 清晰强调色正文（复刻图一薄荷绿荧光歌词）
    let lit_w = (boundary - rect.x).clamp(0.0, rect.w);
    if lit_w > 0.5 {
        let clip_lit = row.clipped(rect.x - 2.0, rect.y - 4.0, lit_w + 2.0, rect.h + 8.0);
        let bloom_outer = paint::alpha(progress_color, 0.14);
        let bloom_inner = paint::alpha(progress_color, 0.24);
        for (ox, oy) in [(-1.6, 0.0), (1.6, 0.0), (0.0, -1.6), (0.0, 1.6)] {
            clip_lit.centered_text(x + ox, cy + oy, &fitted, font_size, weight, bloom_outer, H::Left);
        }
        for (ox, oy) in [(-0.7, -0.7), (0.7, -0.7), (-0.7, 0.7), (0.7, 0.7)] {
            clip_lit.centered_text(x + ox, cy + oy, &fitted, font_size, weight, bloom_inner, H::Left);
        }
        clip_lit.centered_text(x, cy, &fitted, font_size, weight, progress_color, H::Left);
    }

    // 未唱部分：半透白底（逐字扫过即点亮）
    let unlit_w = (rect.x + rect.w - boundary).clamp(0.0, rect.w);
    if unlit_w > 0.5 {
        let clip_unlit = row.clipped(boundary, rect.y - 4.0, unlit_w + 4.0, rect.h + 8.0);
        clip_unlit.centered_text(x, cy, &fitted, font_size, weight, paint::alpha(base_ink, 0.78), H::Left);
    }
}

// ============================================================================
// 7. 外围呼吸 / 律动光晕 (ported from auralsle-glow/src/engine.rs)
// ============================================================================

pub struct MusicGlowState {
    strength: f32,
    envelope: f32,
    peak: f32,
    last_sample: Instant,
    #[cfg(windows)]
    last_bind: Instant,
    #[cfg(windows)]
    meter: Option<windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation>,
}

impl Default for MusicGlowState {
    fn default() -> Self {
        Self {
            strength: 0.0,
            envelope: 0.0,
            peak: 0.0,
            last_sample: Instant::now(),
            #[cfg(windows)]
            last_bind: Instant::now() - Duration::from_secs(5),
            #[cfg(windows)]
            meter: None,
        }
    }
}

impl MusicGlowState {
    #[cfg(windows)]
    fn sample_audio_peak(&mut self) -> f32 {
        use windows::Win32::Media::Audio::{
            eMultimedia, eRender, Endpoints::IAudioMeterInformation, IMMDeviceEnumerator,
            MMDeviceEnumerator,
        };
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

        if self.last_sample.elapsed() < Duration::from_millis(25) {
            return self.peak;
        }
        self.last_sample = Instant::now();
        if self.meter.is_none() && self.last_bind.elapsed() >= Duration::from_secs(2) {
            self.last_bind = Instant::now();
            self.meter = unsafe {
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .ok()
                    .and_then(|e| e.GetDefaultAudioEndpoint(eRender, eMultimedia).ok())
                    .and_then(|ep| ep.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None).ok())
            };
        }
        self.peak = if let Some(meter) = &self.meter {
            match unsafe { meter.GetPeakValue() } {
                Ok(v) if v.is_finite() => v.clamp(0.0, 1.0),
                _ => {
                    self.meter = None;
                    0.0
                }
            }
        } else {
            0.0
        };
        self.peak
    }

    #[cfg(not(windows))]
    fn sample_audio_peak(&mut self) -> f32 {
        0.55
    }

    pub fn tick(&mut self, dt: f32, active: bool, rhythm: bool, time_secs: f64) -> f32 {
        let target = if active { 1.0 } else { 0.0 };
        self.strength = crate::anim::approach(self.strength, target, 8.0, dt);
        if self.strength < 0.002 && !active {
            #[cfg(windows)]
            {
                self.meter = None;
            }
            return 0.0;
        }
        let amount = if rhythm {
            let raw_peak = self.sample_audio_peak();
            let peak = if raw_peak > 0.015 {
                raw_peak.sqrt()
            } else {
                // 系统峰值表未返回信号时，用双谐波节拍律动兜底，确保「音乐律动」始终生动可见
                let b1 = ((time_secs * 2.15 * std::f64::consts::TAU).sin() * 0.5 + 0.5) as f32;
                let b2 = ((time_secs * 4.30 * std::f64::consts::TAU + 0.9).sin() * 0.5 + 0.5) as f32;
                (0.28 + 0.52 * b1.powi(2) + 0.20 * b2).clamp(0.0, 1.0)
            };
            self.envelope = crate::anim::approach(self.envelope, peak, 14.0, dt);
            0.28 + 0.72 * self.envelope.clamp(0.0, 1.0)
        } else {
            0.55 + 0.42 * ((time_secs / 2.2 * std::f64::consts::TAU).sin() as f32 * 0.5 + 0.5)
        };
        (self.strength * amount).clamp(0.0, 1.0)
    }
}

pub fn paint_outer_glow(painter: &egui::Painter, rect: Rect, radius: f32, accent: Color32, strength: f32) {
    if strength <= 0.01 {
        return;
    }
    for (expand, sw, alpha) in [
        (1.5, 3.2, 0.52),
        (4.5, 3.4, 0.32),
        (8.0, 3.6, 0.18),
        (12.0, 3.8, 0.09),
    ] {
        painter.rect_stroke(
            rect.expand(expand),
            radius + expand * 0.65,
            Stroke::new(sw, paint::alpha(accent, alpha * strength)),
            egui::StrokeKind::Outside,
        );
    }
}

pub fn paint_inner_glow(painter: &egui::Painter, rect: Rect, radius: f32, accent: Color32, strength: f32) {
    if strength <= 0.01 {
        return;
    }
    painter.rect_stroke(
        rect.shrink(0.5),
        (radius - 0.5).max(0.0),
        Stroke::new(2.0, paint::alpha(accent, 0.42 * strength)),
        egui::StrokeKind::Inside,
    );
}

// ============================================================================
// 8. 点击封面/标题呼出关联播放器窗口
// ============================================================================

#[cfg(windows)]
pub fn focus_player_source(source: &str) {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
        ShowWindowAsync, SW_RESTORE,
    };
    let Some(exe) = crate::music_volume::executable(source) else {
        return;
    };
    let pids = crate::smtc::player_process::pids_named(&exe);
    if pids.is_empty() {
        return;
    }
    struct Search {
        pids: Vec<u32>,
        found: Option<HWND>,
    }
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut Search);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if state.pids.contains(&pid) && IsWindowVisible(hwnd).as_bool() {
            state.found = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut search = Search { pids, found: None };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut search as *mut Search as isize));
        if let Some(hwnd) = search.found {
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(not(windows))]
pub fn focus_player_source(_source: &str) {}

// ============================================================================
// 9. 统一共享音乐卡片绘制器（同时驱动灵动岛与设置编辑器实时预览）
// ============================================================================

pub struct CardInput<'a> {
    pub width: f32,
    pub surface_height: f32,
    pub card_radius: f32,
    pub time_secs: f64,
    pub title: &'a str,
    pub artist: &'a str,
    pub has_track: bool,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub lyric: &'a str,
    pub secondary: &'a str,
    pub highlight: f32,
    pub lyric_age_phase: f32,
    pub artwork: Option<&'a (Arc<egui::ColorImage>, egui::TextureHandle)>,
    pub artwork_tint: Option<Color32>,
    pub volume_level: Option<(u32, bool)>,
    pub liked: bool,
    pub shuffle_mode: u8,
    pub geometry: MusicGeometry,
    pub bar_style: BarStyle,
    pub thumb_style: ThumbStyle,
    pub thumb_spin: bool,
    pub transport_theme: TransportTheme,
    pub allow_seek_and_buttons: bool,
    pub id_salt: &'static str,
}

#[derive(Default)]
pub struct CardEvents {
    pub seek_to: Option<f64>,
    pub clicked_button: Option<i32>,
    pub volume_wheel_delta: Option<f32>,
    pub header_clicked: bool,
    pub hovered_element: Option<u8>,
    pub dragged_element: Option<(u8, Vec2, bool)>,
    pub pressed_element: Option<u8>,
}

fn mmss_auralsle(secs: f64) -> String {
    let s = secs.max(0.0).floor() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

pub fn paint_auralsle_music_card(
    c: &Card,
    cfg: &crate::auralsle_ai_status::config::Config,
    blur_cache: &mut CoverBlurCache,
    input: &CardInput<'_>,
) -> CardEvents {
    let mut events = CardEvents::default();
    let g = &input.geometry;
    let w = input.width;
    let surface_h = input.surface_height.max(g.height);
    let ctx = c.ui.ctx();

    let (palette, lyric_accent) = resolve_palette(cfg, input.artwork.map(|(img, _)| img.as_ref()));
    let blurred_tex = blur_cache.get_or_build(ctx, input.artwork, cfg.auralsle_acrylic_frost);
    let is_glass = cfg.auralsle_card_material.eq_ignore_ascii_case("Glass");

    paint_music_surface(
        c,
        w,
        surface_h,
        input.card_radius,
        &palette,
        blurred_tex.as_ref(),
        cfg.auralsle_acrylic_frost,
        cfg.auralsle_cover_overlay,
        cfg.auralsle_acrylic_opacity,
        is_glass,
    );

    // 1) 专辑封面
    if g.cover.w > 1.0 && g.cover.h > 1.0 {
        let cr = cover_corner_radius(input.card_radius, g.cover.w, g.cover.h);
        if let Some((image, texture)) = input.artwork {
            let [iw, ih] = image.size;
            let (uw, vh) = if iw > ih {
                (ih as f32 / iw as f32, 1.0)
            } else {
                (1.0, iw as f32 / ih as f32)
            };
            let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), Vec2::new(uw, vh));
            let rect = c.rect(g.cover.x, g.cover.y, g.cover.w, g.cover.h);
            let shape = egui::epaint::RectShape::filled(
                rect,
                CornerRadius::same((cr * c.s).round().clamp(0.0, 255.0) as u8),
                Color32::WHITE,
            )
            .with_texture(texture.id(), uv);
            c.painter.add(shape);
        } else {
            let bg = input.artwork_tint.unwrap_or_else(|| Color32::from_rgb(30, 34, 38));
            c.vgrad(
                g.cover.x,
                g.cover.y,
                g.cover.w,
                g.cover.h,
                cr,
                paint::mix(bg, palette.accent, 0.25),
                bg,
            );
            paint::note(
                c,
                g.cover.x + g.cover.w * 0.5,
                g.cover.y + g.cover.h * 0.5,
                g.cover.w * 0.42,
                Color32::from_white_alpha(220),
            );
        }
        c.stroke(
            g.cover.x,
            g.cover.y,
            g.cover.w,
            g.cover.h,
            cr,
            0.75,
            Color32::from_white_alpha(28),
        );
    }

    // 2) 歌曲标题 & 歌手
    let align_h = if g.align >= 0.9 {
        H::Right
    } else if g.align >= 0.45 {
        H::Center
    } else {
        H::Left
    };
    let anchor_x = |r: DRect| match align_h {
        H::Left => r.x,
        H::Center => r.x + r.w * 0.5,
        H::Right => r.x + r.w,
    };

    if g.title.w > 4.0 {
        let t_str = if input.has_track { input.title } else { "暂无正在播放的音乐" };
        let fitted_t = c.fit(t_str, 16.0, W::Bold, g.title.w);
        c.centered_text(
            anchor_x(g.title),
            g.title.y + g.title.h * 0.5,
            &fitted_t,
            16.0,
            W::Bold,
            palette.text,
            align_h,
        );
    }
    if g.artist.w > 4.0 {
        let a_str = if input.has_track {
            if input.artist.trim().is_empty() { "未知艺术家" } else { input.artist }
        } else {
            "打开音乐播放器即可自动同步"
        };
        let fitted_a = c.fit(a_str, 10.5, W::Regular, g.artist.w);
        c.centered_text(
            anchor_x(g.artist),
            g.artist.y + g.artist.h * 0.5,
            &fitted_a,
            10.5,
            W::Regular,
            palette.muted,
            align_h,
        );
    }
    if input.allow_seek_and_buttons && g.text_hit.w > 4.0 {
        let hdr = c.hit(
            g.cover.x.min(g.text_hit.x),
            g.cover.y.min(g.text_hit.y),
            (g.cover.x + g.cover.w).max(g.text_hit.x + g.text_hit.w) - g.cover.x.min(g.text_hit.x),
            (g.cover.y + g.cover.h).max(g.text_hit.y + g.text_hit.h) - g.cover.y.min(g.text_hit.y),
            (input.id_salt, "hdr"),
        );
        if hdr.clicked() {
            events.header_clicked = true;
        }
    }

    // 3) 发光主歌词 + 副文本（翻译 / 下一句）
    if g.lyric.w > 4.0 {
        let lyric_text = if input.has_track {
            if input.lyric.trim().is_empty() {
                "···"
            } else {
                input.lyric
            }
        } else {
            "等待歌词同步"
        };
        draw_glowing_lyric(
            c,
            lyric_text,
            g.lyric,
            (cfg.auralsle_lyric_font_size + 2.0).clamp(13.0, 22.0),
            g.align,
            input.highlight,
            lyric_accent,
            palette.text,
            input.lyric_age_phase,
            cfg.auralsle_word_highlight,
        );
    }
    if g.secondary.w > 4.0 && !input.secondary.trim().is_empty() {
        let fitted_sec = c.fit(input.secondary.trim(), 10.5, W::Regular, g.secondary.w);
        c.centered_text(
            anchor_x(g.secondary),
            g.secondary.y + g.secondary.h * 0.5,
            &fitted_sec,
            10.5,
            W::Regular,
            palette.muted,
            align_h,
        );
    }

    // 4) 已按需求取消 Auralsle 灵屿音乐岛所有样式的播放状态文案显示
    if false && g.status_text.w > 4.0 {
        let status = if !input.has_track {
            "等待播放"
        } else if input.playing {
            "正在播放"
        } else {
            "已暂停"
        };
        let status_font = 9.5;
        if g.align >= 0.45 {
            let tw = c.text_w(status, status_font, W::Regular).min(g.status_text.w);
            let total = 4.0 + 6.0 + tw;
            let sx = g.status_text.x + (g.status_text.w - total) * 0.5;
            let sy = g.status_text.y + g.status_text.h * 0.5;
            c.circle(sx + 2.0, sy, 2.2, lyric_accent);
            c.centered_text(sx + 10.0, sy, status, status_font, W::Regular, palette.muted, H::Left);
        } else {
            let dot_cx = g.status_dot.x + g.status_dot.w * 0.5;
            let sy = g.status_text.y + g.status_text.h * 0.5;
            c.circle(dot_cx, sy, 2.2, lyric_accent);
            let fitted_st = c.fit(status, status_font, W::Regular, g.status_text.w);
            c.centered_text(
                g.status_text.x,
                sy,
                &fitted_st,
                status_font,
                W::Regular,
                palette.muted,
                H::Left,
            );
        }
    }

    // 5) 进度条与时间标签
    if g.track.w > 4.0 {
        let frac = if input.has_track && input.duration > 0.0 {
            (input.position / input.duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let mut active_pos = input.position;
        if input.allow_seek_and_buttons && c.live && input.has_track && input.duration > 0.0 {
            let bar_r = c.hit_drag(
                g.progress_hit.x,
                g.progress_hit.y,
                g.progress_hit.w,
                g.progress_hit.h,
                (input.id_salt, "seek"),
            );
            if bar_r.is_pointer_button_down_on() || bar_r.clicked() || bar_r.drag_stopped() {
                if let Some(pt) = bar_r.interact_pointer_pos().or_else(|| ctx.input(|i| i.pointer.interact_pos())) {
                    let local_x = c.to_local(pt).x;
                    let seek_f = ((local_x - g.track.x) / g.track.w.max(1.0)).clamp(0.0, 1.0) as f64;
                    active_pos = seek_f * input.duration;
                    if bar_r.clicked() || bar_r.drag_stopped() {
                        events.seek_to = Some(active_pos);
                    }
                }
            }
        }
        let draw_frac = if input.has_track && input.duration > 0.0 {
            (active_pos / input.duration).clamp(0.0, 1.0) as f32
        } else {
            frac
        };

        let playing_t = ctx.animate_bool_with_time(
            egui::Id::new((input.id_salt, "auralsle-play-morph")),
            input.playing,
            0.22,
        );
        let phase = (input.time_secs * 2.0) as f32;
        let spin = if input.thumb_spin { (input.time_secs * 0.5) as f32 } else { 0.0 };
        let fg_col = palette.accent;
        let track_col = paint::alpha(palette.accent, 0.28);
        let dot_col = paint::alpha(palette.accent, 0.56);

        draw_progress_bar(
            c,
            g.track,
            draw_frac,
            phase,
            0.66,
            13.5,
            playing_t,
            spin,
            fg_col,
            track_col,
            dot_col,
            input.bar_style,
            input.thumb_style,
        );

        let elapsed_s = if input.has_track { mmss_auralsle(active_pos) } else { "00:00".into() };
        let rem_s = if input.has_track && input.duration > 0.0 {
            format!("−{}", mmss_auralsle((input.duration - active_pos).max(0.0)))
        } else {
            "--:--".into()
        };
        if g.elapsed.w > 2.0 {
            c.centered_text(
                g.elapsed.x,
                g.elapsed.y + g.elapsed.h * 0.5,
                &elapsed_s,
                9.5,
                W::Regular,
                palette.muted,
                H::Left,
            );
        }
        if g.total.w > 2.0 {
            c.centered_text(
                g.total.x + g.total.w,
                g.total.y + g.total.h * 0.5,
                &rem_s,
                9.5,
                W::Regular,
                palette.muted,
                H::Right,
            );
        }
    }

    // 6) 8 个播放控制按钮
    let playing_t = ctx.animate_bool_with_time(
        egui::Id::new((input.id_salt, "auralsle-play-morph")),
        input.playing,
        0.22,
    );
    let tonal = paint::mix(palette.background, palette.accent, 0.24);
    let control_col = paint::mix(palette.accent, palette.text, 0.22);

    for (id, r) in g.buttons {
        if r.w <= 1.0 || r.h <= 1.0 {
            continue;
        }
        let disabled = ([3, 4, 5].contains(&id) && !input.has_track)
            || (id == 6 && !cfg.auralsle_translation_button);
        if id == 6 && disabled {
            continue;
        }
        let (hover, press, clicked) = if input.allow_seek_and_buttons {
            let resp = c.hit(r.x, r.y, r.w, r.h, (input.id_salt, "btn", id));
            let (h_t, p_t) = c.hover_t(&resp);
            if c.live && id == 16 && resp.hovered() {
                let d = ctx.input(|i| i.raw_scroll_delta.y);
                if d.abs() > 0.5 {
                    events.volume_wheel_delta = Some(d);
                }
            }
            if cfg.auralsle_hover_tooltip && c.live && h_t > 0.35 && (id == 16 || id == 6) {
                let tip = if id == 16 {
                    match input.volume_level {
                        Some((lvl, m)) => format!("滚轮调节音量 · {lvl}%{}", if m { " · 静音" } else { "" }),
                        None => "暂无可调节的音频会话".to_owned(),
                    }
                } else if cfg.auralsle_show_translation {
                    "隐藏翻译 · 已开启".to_owned()
                } else {
                    "显示翻译 · 已关闭".to_owned()
                };
                let tw = (c.text_w(&tip, 10.5, W::Semibold) + 18.0).min(w - 24.0);
                let tx = (r.x + r.w * 0.5 - tw * 0.5).clamp(12.0, w - tw - 12.0);
                let ty = r.y - 34.0;
                c.fill(tx, ty, tw, 24.0, 8.0, Color32::from_rgb(233, 237, 239));
                c.centered_text(tx + tw * 0.5, ty + 12.0, &tip, 10.5, W::Semibold, Color32::from_rgb(32, 39, 43), H::Center);
            }
            (h_t, p_t, c.live && resp.clicked())
        } else {
            (0.0, 0.0, false)
        };

        let acc = if disabled { paint::alpha(palette.accent, 0.30) } else { palette.accent };
        let ctrl = if disabled { paint::alpha(control_col, 0.30) } else { control_col };
        let btn_card = c.scaled(r.x + r.w * 0.5, r.y + r.h * 0.5, 1.0 - 0.06 * press);
        let glyph_ink = draw_transport_button_surface(
            &btn_card,
            id,
            r,
            input.transport_theme,
            acc,
            palette.on_accent,
            ctrl,
            tonal,
            palette.text,
            playing_t,
            hover,
        );
        let final_ink = if id == 6 && !cfg.auralsle_show_translation {
            paint::alpha(glyph_ink, 0.50)
        } else if id == 9 && input.liked {
            Color32::from_rgb(244, 114, 182)
        } else {
            glyph_ink
        };
        let muted = input.volume_level.is_some_and(|(_, m)| m);
        draw_transport_glyph(
            &btn_card,
            id,
            r.x + r.w * 0.5,
            r.y + r.h * 0.5,
            final_ink,
            playing_t,
            input.liked,
            input.shuffle_mode,
            muted,
        );
        if clicked {
            events.clicked_button = Some(id);
        }
    }

    // 7) 布局编辑器模式：可点击/拖拽全部元素
    if !input.allow_seek_and_buttons && c.live {
        for (eid, er) in elements_of(g) {
            let hr = c.hit_drag(er.x, er.y, er.w, er.h, (input.id_salt, "ed-el", eid));
            if hr.hovered() {
                events.hovered_element = Some(eid);
            }
            if hr.clicked() || hr.drag_started() || hr.is_pointer_button_down_on() {
                events.pressed_element = Some(eid);
            }
            if hr.dragged() {
                events.dragged_element = Some((eid, hr.drag_delta() / c.s, true));
            } else if hr.drag_stopped() {
                events.dragged_element = Some((eid, Vec2::ZERO, false));
            }
        }
    }

    events
}
