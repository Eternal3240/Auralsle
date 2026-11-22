//! Auralsle 验证码提取与独立浮窗胶囊（带一键复制按钮）。
//!
//! 移植自 Heiamer/Auralsle (`verification_code.rs` & `renderer.rs`，MIT License)。

use crate::paint::{self, Card, H};
use crate::theme::W;
use eframe::egui::{self, Color32, Stroke};

pub const WIDTH: f32 = 384.0;
pub const HEIGHT: f32 = 84.0;

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub body: egui::Rect,
    pub icon: egui::Rect,
    pub source: egui::Rect,
    pub digits: egui::Rect,
    pub action: egui::Rect,
    pub close: egui::Rect,
    pub compact_action: bool,
}

pub fn layout(panel_w: f32, panel_h: f32) -> Layout {
    let w = panel_w.max(140.0);
    let h = panel_h.max(HEIGHT);
    let body = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
    let compact_action = w < 336.0;
    let action_width = if compact_action { 28.0 } else { 84.0 };
    let x = 60.0;
    let action = egui::Rect::from_min_size(egui::pos2(w - 20.0 - action_width, 38.0), egui::vec2(action_width, 28.0));
    Layout {
        body,
        icon: egui::Rect::from_min_size(egui::pos2(18.0, 27.0), egui::vec2(30.0, 30.0)),
        source: egui::Rect::from_min_size(egui::pos2(x, 13.0), egui::vec2((w - 102.0).max(40.0), 18.0)),
        digits: egui::Rect::from_min_size(egui::pos2(x, 34.0), egui::vec2((action.left() - x - 14.0).max(1.0), 36.0)),
        action,
        close: egui::Rect::from_min_size(egui::pos2(w - 28.0, 9.0), egui::vec2(18.0, 18.0)),
        compact_action,
    }
}

pub fn valid_code(code: &str) -> bool {
    (4..=8).contains(&code.len())
        && code.bytes().all(|c| c.is_ascii_alphanumeric())
        && code.bytes().any(|c| c.is_ascii_digit())
}

/// 从通知/消息标题与正文中保守提取验证码（支持全角数字、中英文关键词及常见干扰项过滤）。
pub fn extract(title: &str, body: &str) -> Option<String> {
    let text = format!("{title}\n{body}")
        .chars()
        .map(|c| {
            if ('０'..='９').contains(&c) {
                char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c)
            } else {
                c
            }
        })
        .collect::<Vec<_>>();
    let matching = text.iter().map(char::to_ascii_lowercase).collect::<Vec<_>>();
    let mut markers = Vec::new();
    for word in [
        "验证码",
        "验证代码",
        "登录代码",
        "安全代码",
        "登录码",
        "安全码",
        "校验码",
        "动态码",
        "动态密码",
        "短信码",
        "确认码",
        "一次性密码",
        "取件码",
        "verification code",
        "security code",
        "one-time code",
        "one time code",
        "sign-in code",
        "login code",
        "otp",
    ] {
        let key = word.chars().collect::<Vec<_>>();
        if key.is_empty() || text.len() < key.len() {
            continue;
        }
        for i in 0..=text.len() - key.len() {
            if matching.get(i..i + key.len()) == Some(key.as_slice()) {
                if word == "otp"
                    && ((i > 0 && text[i - 1].is_ascii_alphanumeric())
                        || text.get(i + key.len()).is_some_and(char::is_ascii_alphanumeric))
                {
                    continue;
                }
                markers.push((i, i + key.len()));
            }
        }
    }
    if markers.is_empty() {
        return None;
    }
    let mut candidates = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if !text[i].is_ascii_alphanumeric() {
            i += 1;
            continue;
        }
        let start = i;
        while i < text.len() && text[i].is_ascii_alphanumeric() {
            i += 1;
        }
        let candidate: String = text[start..i].iter().collect();
        if !valid_code(&candidate)
            || (start > 0 && text[start - 1].is_ascii_alphanumeric())
            || text.get(i).is_some_and(char::is_ascii_alphanumeric)
        {
            continue;
        }
        if text[i..]
            .iter()
            .find(|c| !c.is_whitespace())
            .is_some_and(|c| "年月日分秒元".contains(*c))
        {
            continue;
        }
        let context: String = matching[start.saturating_sub(8)..start].iter().collect();
        if ["订单", "快递", "单号", "尾号", "电话", "手机号", "金额", "余额", "order", "tracking"]
            .iter()
            .any(|w| context.contains(w))
        {
            continue;
        }
        let distance = markers
            .iter()
            .map(|(a, b)| {
                if start >= *b {
                    start - b
                } else if i <= *a {
                    a - i
                } else {
                    0
                }
            })
            .min()?;
        if distance <= 24 {
            candidates.push((distance, candidate));
        }
    }
    candidates.sort_by_key(|(d, _)| *d);
    if candidates
        .get(1)
        .is_some_and(|c| c.0 == candidates[0].0 && c.1 != candidates[0].1)
    {
        return None;
    }
    candidates.first().map(|(_, c)| c.clone())
}

#[cfg(windows)]
pub fn copy_code(code: &str) -> Result<(), String> {
    if !valid_code(code) {
        return Err("验证码无效".into());
    }
    clipboard_win::set_clipboard_string(code).map_err(|_| "剪贴板正忙，请重试".to_owned())
}

#[cfg(not(windows))]
pub fn copy_code(code: &str) -> Result<(), String> {
    if !valid_code(code) {
        return Err("验证码无效".into());
    }
    Ok(())
}

fn draw_copy_glyph(c: &Card, x: f32, y: f32, size: f32, ink: Color32) {
    let bx = x + size * 0.16;
    let by = y + size * 0.12;
    c.seg((bx + size * 0.48, by), (bx, by), 1.5, ink);
    c.seg((bx, by), (bx, by + size * 0.62), 1.5, ink);
    c.stroke(
        x + size * 0.35,
        y + size * 0.30,
        size * 0.52,
        size * 0.58,
        1.5,
        1.5,
        ink,
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CapsuleAction {
    None,
    Copy,
    Dismiss,
}

/// 在验证码胶囊内绘制 Auralsle 原生视觉（图标盒 + 来源标题 + 大号验证码 + 右侧即刻复制按钮）。
pub fn draw_capsule(
    c: &Card,
    panel_w: f32,
    panel_h: f32,
    corner_r: f32,
    source: &str,
    code: &str,
    copied: bool,
) -> CapsuleAction {
    let g = layout(panel_w, panel_h);
    let ink = Color32::from_rgb(245, 245, 247);
    let blue = Color32::from_rgb(117, 180, 255);
    let muted = Color32::from_rgb(174, 180, 192);

    // 胶囊底板与 1px 高光描边（不跟随外部皮肤反相）
    c.fill(0.0, 0.0, panel_w, panel_h, corner_r, Color32::from_rgba_unmultiplied(18, 21, 27, 245));
    c.stroke(0.5, 0.5, panel_w - 1.0, panel_h - 1.0, (corner_r - 0.5).max(0.0), 1.0, Color32::from_white_alpha(26));

    // 左侧图标容器 (30x30, r=10)
    c.fill(
        g.icon.left(),
        g.icon.top(),
        g.icon.width(),
        g.icon.height(),
        10.0,
        Color32::from_rgb(23, 37, 54),
    );
    if copied {
        paint::check(c, g.icon.center().x, g.icon.center().y, 13.0, 1.0, 2.0, blue);
    } else {
        draw_copy_glyph(c, g.icon.left() + 6.0, g.icon.top() + 6.0, 18.0, blue);
    }

    // 来源名称
    let src_label = if source.trim().is_empty() { "验证码" } else { source.trim() };
    let fitted_src = c.fit(src_label, 12.0, W::Semibold, g.source.width());
    c.centered_text(
        g.source.left(),
        g.source.center().y,
        &fitted_src,
        12.0,
        W::Semibold,
        muted,
        H::Left,
    );

    // 自适应大号验证码（绝不截断）
    let natural_w = c.text_w(code, 27.0, W::Bold).max(1.0);
    let digit_size = (27.0 * (g.digits.width() / natural_w).min(1.0)).clamp(15.0, 27.0);
    c.centered_text(
        g.digits.left(),
        g.digits.center().y,
        code,
        digit_size,
        W::Bold,
        ink,
        H::Left,
    );

    // 右上角轻量关闭按钮
    let close_r = c.hit(
        g.close.left(),
        g.close.top(),
        g.close.width(),
        g.close.height(),
        "otp-capsule-close",
    );
    let (ch, _) = c.hover_t(&close_r);
    if ch > 0.02 {
        c.circle(
            g.close.center().x,
            g.close.center().y,
            8.5,
            Color32::from_white_alpha((ch * 34.0) as u8),
        );
    }
    paint::close(
        c,
        g.close.center().x,
        g.close.center().y,
        3.6,
        paint::mix(Color32::from_rgb(130, 136, 148), ink, ch),
    );
    if close_r.on_hover_text("关闭验证码提示").clicked() {
        return CapsuleAction::Dismiss;
    }

    // 右侧即刻复制按钮（及整段数字区域点击即复制）
    let btn_r = c.hit(
        g.action.left(),
        g.action.top(),
        g.action.width(),
        g.action.height(),
        "otp-capsule-copy",
    );
    let digits_r = c.hit(
        g.digits.left(),
        g.digits.top(),
        g.digits.width(),
        g.digits.height(),
        "otp-capsule-digits",
    );
    let (bh, bp) = c.hover_t(&btn_r);
    let btn_card = c.scaled(g.action.center().x, g.action.center().y, 1.0 - 0.05 * bp);
    let btn_bg = if copied {
        Color32::from_rgb(25, 53, 77)
    } else {
        paint::mix(Color32::from_rgb(36, 40, 48), Color32::from_rgb(48, 55, 68), bh)
    };
    let btn_stroke = if copied {
        paint::alpha(blue, 0.55)
    } else {
        Color32::from_white_alpha((24.0 + 26.0 * bh) as u8)
    };
    btn_card.fill(
        g.action.left(),
        g.action.top(),
        g.action.width(),
        g.action.height(),
        14.0,
        btn_bg,
    );
    btn_card.painter.rect_stroke(
        btn_card.rect(g.action.left(), g.action.top(), g.action.width(), g.action.height()),
        14.0 * btn_card.s,
        Stroke::new(0.8 * btn_card.s, btn_stroke),
        egui::StrokeKind::Inside,
    );
    if g.compact_action {
        if copied {
            paint::check(&btn_card, g.action.center().x, g.action.center().y, 11.0, 1.0, 1.8, blue);
        } else {
            draw_copy_glyph(&btn_card, g.action.left() + 6.0, g.action.top() + 6.0, 16.0, blue);
        }
    } else {
        let label = if copied { "已复制 ✓" } else { "复制" };
        btn_card.centered_text(
            g.action.center().x,
            g.action.center().y,
            label,
            11.5,
            W::Semibold,
            if copied { blue } else { ink },
            H::Center,
        );
    }

    if btn_r.clicked() || digits_r.clicked() {
        return CapsuleAction::Copy;
    }
    CapsuleAction::None
}
