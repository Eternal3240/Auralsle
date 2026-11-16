//! islo 的 20 种原生矢量表情（移植到岛仔）。
//!
//! 表情的嘴型词汇与状态映射适配自 [Aora Bot](https://github.com/sam70361/aora-bot)
//! Copyright (c) 2026 sam70361，适用 **Aora Bot License 1.1**。
//! 原实现见 islo `src/pet.rs`；本文件是面向岛仔 `paint::Card` 的独立重写，
//! 只保留面部几何与状态词汇，不含 islo 的 i18n / anim / 状态机。
//! 按许可 5.1 保留版权与许可声明；产品署名见设置页「吉祥物表情」卡片（5.2）。
//!
//! 设计约束：岛仔 `pet.rs` 的眼睛是 13 点路径 / 3.6 描边，且 `pet_expressions`
//! 图集与音乐麦克风都有上游测试锁死。因此本模块**不重画眼睛**，只在眼睛画完后
//! 叠加嘴型 / 腮红 / 状态装饰；音乐态继续走岛仔原演唱逻辑。这样 20 种表情
//! 全部可见可固定，同时主岛的既有测试契约一条都不破。

use crate::{
    paint::{Card, H},
    theme::{self, W},
};
use eframe::egui::Color32;

/// islo 的 20 种表情。前 10 个与岛仔 `PetState` 一一对应，后 10 个是 islo 新增。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Awake, Curious, Waking, Happy, Satisfied, Music, Worried, Focused, Dozing, Sleeping,
    Listening, Thinking, Loading, Excited, Laughing, Shy, Sad, Angry, Surprised, Confused,
}

impl Face {
    pub const ALL: [Self; 20] = [
        Self::Awake, Self::Curious, Self::Waking, Self::Happy, Self::Satisfied,
        Self::Music, Self::Worried, Self::Focused, Self::Dozing, Self::Sleeping,
        Self::Listening, Self::Thinking, Self::Loading, Self::Excited, Self::Laughing,
        Self::Shy, Self::Sad, Self::Angry, Self::Surprised, Self::Confused,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Awake => "待机", Self::Curious => "好奇", Self::Waking => "苏醒",
            Self::Happy => "开心", Self::Satisfied => "满足", Self::Music => "听音乐",
            Self::Worried => "担心", Self::Focused => "专注", Self::Dozing => "困倦",
            Self::Sleeping => "睡眠", Self::Listening => "倾听", Self::Thinking => "思考",
            Self::Loading => "加载", Self::Excited => "兴奋", Self::Laughing => "大笑",
            Self::Shy => "害羞", Self::Sad => "难过", Self::Angry => "生气",
            Self::Surprised => "惊讶", Self::Confused => "疑惑",
        }
    }
    /// 岛仔 `PetState` 的 Debug 名 → 表情索引。
    pub fn from_state_name(name: &str) -> usize {
        let i = match name {
            "Curious" => 1, "Waking" => 2, "Happy" => 3, "Satisfied" => 4,
            "Music" => 5, "Worried" => 6, "Focused" => 7, "Dozing" => 8,
            "Sleeping" => 9, _ => 0,
        };
        i
    }
    pub fn from_index(i: usize) -> Self { Self::ALL[i.min(19)] }
}

/// Aora Bot 的线性嘴型单位（width / bend / open / wave / skew），按 islo 的 20 态映射。
#[derive(Clone, Copy, Debug)]
struct Mouth { width: f32, bend: f32, open: f32, wave: f32, skew: f32 }

impl Mouth {
    fn target(face: Face) -> Self {
        let (w, b, o, v, s) = match face {
            Face::Happy | Face::Satisfied => (0.60, 0.22, 0.00, 0.00, 0.0),
            Face::Excited => (0.70, 0.26, 0.14, 0.00, 0.0),
            Face::Laughing => (0.50, 0.14, 0.62, 0.00, 0.0),
            Face::Shy => (0.50, 0.15, 0.00, 0.00, 0.7),
            Face::Surprised => (0.24, 0.00, 0.48, 0.00, 0.0),
            Face::Focused | Face::Curious => (0.10, 0.00, 0.20, 0.00, 0.0),
            Face::Confused | Face::Worried => (0.55, 0.00, 0.00, 0.11, 0.0),
            Face::Sad | Face::Angry => (0.50, -0.18, 0.00, 0.00, 0.0),
            Face::Dozing => (0.30, -0.10, 0.00, 0.00, 0.0),
            Face::Thinking | Face::Loading => (0.32, 0.00, 0.00, 0.00, 0.0),
            Face::Listening | Face::Sleeping => (0.50, 0.00, 0.00, 0.00, 0.0),
            _ => (0.50, 0.09, 0.00, 0.00, 0.0),
        };
        Self { width: w * 5.25, bend: b * 5.25, open: o * 5.25, wave: v * 5.25, skew: s }
    }

    fn draw_colored(self, c: &Card, cx: f32, cy: f32, ink: Color32) {
        let point = |u: f32, v: f32| {
            (cx + u * self.width,
             cy + self.bend * (1.0 - u * u) + v * self.open / 2.0
                + (u * std::f32::consts::TAU).sin() * self.wave + u * self.skew * 0.5)
        };
        let pts: Vec<(f32, f32)> = if self.open > 0.08 {
            (0..=32).map(|i| {
                let a = i as f32 / 32.0 * std::f32::consts::TAU;
                point(a.cos(), a.sin())
            }).collect()
        } else {
            (0..=16).map(|i| point(i as f32 / 8.0 - 1.0, 0.0)).collect()
        };
        c.path(&pts, 0.85, ink);
    }
}

/// 眼睛形状参数（与岛仔 `pet::Eye` 同构，仅用于预览网格；主岛眼睛仍由 pet.rs 绘制）。
#[derive(Clone, Copy)]
struct Eye { height: f32, spread: f32, curve: f32, y: f32, tilt: f32 }
impl Eye {
    fn open(h: f32) -> Self { Self { height: h, spread: 0.0, curve: 0.0, y: 0.0, tilt: 0.0 } }
    fn arc(s: f32, c: f32) -> Self { Self { spread: s, curve: c, ..Self::open(0.0) } }
}

fn eyes_for(face: Face) -> [Eye; 2] {
    match face {
        Face::Curious => { let mut e = [Eye::open(5.6), Eye::open(6.3)]; e[0].y = 0.6; e[1].y = -0.6; e }
        Face::Happy | Face::Laughing => [Eye::arc(2.6, -1.8); 2],
        Face::Satisfied => [Eye::arc(2.2, -1.2); 2],
        Face::Worried => [
            Eye { spread: -0.6, tilt: -0.7, y: 0.4, ..Eye::open(2.8) },
            Eye { spread: 0.6, tilt: 0.7, y: 1.0, ..Eye::open(1.8) },
        ],
        Face::Focused => [Eye::open(1.8); 2],
        Face::Dozing => [Eye::open(0.6); 2],
        Face::Sleeping => [Eye::arc(2.4, 1.0); 2],
        Face::Listening => { let mut e = [Eye::open(5.0); 2]; e[1].y = -0.4; e }
        Face::Thinking => [Eye::open(1.8), Eye::open(4.6)],
        Face::Loading => [Eye::open(3.8); 2],
        Face::Excited => [Eye::open(6.2); 2],
        Face::Shy => [Eye::arc(2.0, 0.8); 2],
        Face::Sad => { let mut e = [Eye::open(2.5); 2]; e[0].tilt = 0.7; e[1].tilt = -0.7; e }
        Face::Angry => { let mut e = [Eye::open(2.8); 2]; e[0].tilt = -0.7; e[1].tilt = 0.7; e }
        Face::Surprised => [Eye::open(6.8); 2],
        Face::Confused => { let mut e = [Eye::open(5.8), Eye::open(2.6)]; e[1].y = 0.5; e }
        Face::Music | Face::Waking => [Eye::arc(2.5, 0.7); 2],
        _ => [Eye::open(4.4); 2],
    }
}

fn blush_for(face: Face) -> f32 {
    if matches!(face, Face::Happy | Face::Satisfied | Face::Excited | Face::Laughing | Face::Shy) { 1.0 } else { 0.0 }
}

/// 画一张完整的小脸，用于设置页 20 格预览与额度页账号头像。
/// `cx, cy` 是脸中心（眼睛基线附近）；`k` 是整体缩放。
pub fn draw_mini(c: &Card, cx: f32, cy: f32, k: f32, face: Face, age: f32) {
    draw_custom(c, cx, cy, k, face, age, [0.0, 0.0], theme::WHITE);
}

/// 完整绘制初代 `scstc/islo` 吉祥物（胶囊/弯弧眼 + 自然眨眼 + 视线追踪 + 线性嘴型 + 腮红 + 20 态装饰）。
pub fn draw_custom(
    c: &Card,
    cx: f32,
    cy: f32,
    k: f32,
    face: Face,
    age: f32,
    gaze: [f32; 2],
    ink: Color32,
) {
    let mut eyes = eyes_for(face);
    // 待机 / 好奇 / 专注 / 思考等睁眼状态下加入自然眨眼
    if matches!(
        face,
        Face::Awake | Face::Curious | Face::Focused | Face::Worried | Face::Listening | Face::Thinking
    ) && age > 0.0
    {
        let period = 4.6;
        let phase = (age % period) - 3.8;
        if (0.0..0.18).contains(&phase) {
            let close = (phase / 0.18 * std::f32::consts::PI).sin();
            for e in &mut eyes {
                e.height *= (1.0 - close).max(0.08);
            }
        }
    }
    let gx = gaze[0] * 0.65;
    let gy = gaze[1] * 0.65;
    let eye_y = cy - 1.0 * k + gy;
    for (i, e) in eyes.iter().enumerate() {
        let x = cx + gx + if i == 0 { -5.6 } else { 5.6 } * k;
        let y = eye_y + e.y * k;
        let pts: Vec<(f32, f32)> = (0..=12)
            .map(|n| {
                let q = n as f32 / 6.0 - 1.0;
                (
                    x + q * e.spread * k,
                    y + q * (e.height / 2.0 + e.tilt) * k + (1.0 - q * q) * e.curve * k,
                )
            })
            .collect();
        let stroke = 3.6 * k;
        c.path(&pts, stroke, ink);
        for p in [pts[0], pts[12]] {
            c.circle(p.0, p.1, stroke / 2.0, ink);
        }
    }
    let m = Mouth::target(face);
    let scaled = Mouth {
        width: m.width * k,
        bend: m.bend * k,
        open: m.open * k,
        wave: m.wave * k,
        skew: m.skew * k,
    };
    scaled.draw_colored(c, cx + gx * 0.7, cy + 7.5 * k + gy * 0.7, ink);
    if blush_for(face) > 0.0 {
        for x in [cx + gx * 0.8 - 10.5 * k, cx + gx * 0.8 + 10.5 * k] {
            c.fill(
                x - 1.6 * k,
                cy + 4.1 * k + gy * 0.8,
                3.2 * k,
                1.4 * k,
                0.7 * k,
                theme::RED_TOP.gamma_multiply(0.65),
            );
        }
    }
    decor(c, cx + gx * 0.4, cy + gy * 0.4, k, face, age);
}

/// 状态装饰（z / 十字星 / 水滴 / 感叹号 / 思考点 / 问号 …），按 islo 词汇重画。
fn decor(c: &Card, cx: f32, cy: f32, k: f32, face: Face, age: f32) {
    let x = |d: f32| cx + d * k;
    let y = |d: f32| cy + d * k;
    match face {
        Face::Dozing | Face::Sleeping => {
            let bob = ((age * 1.4).sin() + 1.0) * 0.7 * k;
            c.text(x(14.0), y(-1.5) - bob, "z", 7.0 * k, W::Bold,
                   Color32::from_rgb(170, 160, 255), H::Left);
            if face == Face::Sleeping {
                c.text(x(19.0), y(-5.5) - bob, "z", 5.5 * k, W::Bold,
                       Color32::from_rgb(170, 160, 255), H::Left);
            }
        }
        Face::Happy => {
            for (dx, yy, r) in [(-18.0, -5.5, 2.1), (18.0, -6.5, 2.8)] {
                c.seg((x(dx - r), y(yy)), (x(dx + r), y(yy)), k, theme::GREEN);
                c.seg((x(dx), y(yy - r)), (x(dx), y(yy + r)), k, theme::GREEN);
            }
        }
        Face::Satisfied => crate::paint::droplet(c, x(18.0), y(-2.5), 5.0 * k, theme::CYAN),
        Face::Worried => {
            c.fill(x(17.0), y(-6.5), 1.6 * k, 4.2 * k, 0.8 * k, theme::YELLOW);
            c.circle(x(17.8), y(0.0), 0.9 * k, theme::YELLOW);
        }
        Face::Thinking | Face::Loading => {
            for i in 0..3 {
                let phase = (age * 2.0 - i as f32 * 0.7).sin() * 0.5 + 0.5;
                c.circle(x(13.0 + i as f32 * 3.0), y(-6.5),
                         (0.7 + phase * 0.25) * k,
                         theme::CYAN.gamma_multiply(0.3 + phase * 0.7));
            }
        }
        Face::Excited => {
            for dx in [-17.0, 17.0] {
                c.seg((x(dx - 1.7), y(-6.5)), (x(dx + 1.7), y(-6.5)), 0.8 * k, theme::YELLOW);
                c.seg((x(dx), y(-8.2)), (x(dx), y(-4.8)), 0.8 * k, theme::YELLOW);
            }
        }
        Face::Sad => crate::paint::droplet(c, x(12.0), y(3.5), 3.0 * k, theme::CYAN),
        Face::Angry => {
            for side in [-1.0, 1.0] {
                let ex = cx + side * 5.6 * k;
                c.seg((ex - 2.2 * k, cy - 6.5 * k - side * 0.7 * k),
                      (ex + 2.2 * k, cy - 6.5 * k + side * 0.7 * k), k, theme::WHITE);
            }
        }
        Face::Surprised => {
            c.seg((x(15.0), y(-7.5)), (x(16.5), y(-9.5)), 0.9 * k, theme::YELLOW);
            c.seg((x(17.0), y(-4.5)), (x(19.0), y(-4.9)), 0.9 * k, theme::YELLOW);
        }
        Face::Confused => { c.text(x(14.0), y(-4.5), "?", 8.0 * k, W::Semibold, theme::CYAN, H::Left); }
        _ => {}
    }
}

/// 主岛叠加层：眼睛由 `pet.rs` 画完后调用，替换原来的状态装饰。
/// `state_name` 是 `PetState` 的 Debug 名；`age` 是当前状态已持续秒数。
/// 返回 false 表示调用方应继续画岛仔自己的装饰（音乐态 / 图集已接管等）。
pub fn decorate(c: &Card, cx: f32, state_name: &str, age: f32, y: f32, gaze: [f32; 2], fixed: i64) -> bool {
    // [aora] 全面替换为 Aora Bot 表情引擎。
    super::aora_face::decorate(c, cx, state_name, age, y, gaze, fixed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn twenty_faces_and_unique_labels() {
        assert_eq!(Face::ALL.len(), 20);
        let mut labels = std::collections::HashSet::new();
        for f in Face::ALL { assert!(labels.insert(f.label()), "重复标签 {}", f.label()); }
    }
    #[test]
    fn state_names_map_into_range() {
        for n in ["Awake","Curious","Waking","Happy","Satisfied","Music","Worried","Focused","Dozing","Sleeping","Unknown"] {
            assert!(Face::from_state_name(n) < 20, "{n}");
        }
    }
    #[test]
    fn mouth_targets_are_finite() {
        for f in Face::ALL {
            let m = Mouth::target(f);
            assert!(m.width.is_finite() && m.bend.is_finite() && m.open.is_finite());
        }
    }
}
