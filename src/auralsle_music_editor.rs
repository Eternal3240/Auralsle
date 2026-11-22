//! Auralsle 灵屿音乐岛设置工作台与 400×236 实时拖拽布局编辑器。
//!
//! 复刻 Auralsle 5 个音乐设置子页：
//! - 0: 尺寸与排版（三排版示意图卡 + 胶囊尺寸滑块 + 歌词开关）
//! - 1: 动态取色（专辑封面 / 系统强调色 / 自选主色 + 固定色）
//! - 2: 交互与提示（闲时隐藏 / 点击呼出播放器 / 悬停提示 / 暂停自动返回）
//! - 3: 光效与动效（呼吸/律动光晕 + 8 种进度条实时预览 + 16 种游标 + 10 套按钮主题）
//! - 4: 布局编辑器（左：400×H 网格实时预览 + 拖拽；右：元素/布局/模块/按钮/样式检查器）

use crate::auralsle_music::{
    self, BarStyle, CardInput, CoverBlurCache, CustomLayout, ThumbStyle, TransportTheme,
    ALL_ELEMENT_IDS,
};
use crate::paint::{self, Card};
use crate::settings_theme::color as settings_color;
use crate::smtc::NowPlaying;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
struct EditorState {
    draft: CustomLayout,
    baseline: CustomLayout,
    undo: Vec<CustomLayout>,
    redo: Vec<CustomLayout>,
    selected: u8,
    inspector_tab: usize,
    drag_before: Option<CustomLayout>,
    drag_id: Option<u8>,
    drag_pos: (f32, f32),
    last_cfg_rev: u64,
}

impl EditorState {
    fn from_config(cfg: &crate::auralsle_ai_status::config::Config) -> Self {
        let mut draft = CustomLayout::from_json_str(&cfg.auralsle_custom_layout_json);
        // 编辑器与主配置同步样式初值（未独立覆盖时保持一致）
        if cfg.auralsle_custom_layout_json.trim().is_empty() {
            draft.bar_style = cfg.auralsle_bar_style.clone();
            draft.thumb_style = cfg.auralsle_thumb_style.clone();
            draft.thumb_enabled = cfg.auralsle_thumb_enabled;
            draft.thumb_spin = cfg.auralsle_thumb_spin;
            draft.transport_theme = cfg.auralsle_transport_theme.clone();
        }
        Self {
            baseline: draft.clone(),
            draft,
            undo: Vec::new(),
            redo: Vec::new(),
            selected: 6, // 默认选中「播放进度条」，匹配图四
            inspector_tab: 0,
            drag_before: None,
            drag_id: None,
            drag_pos: (0.0, 0.0),
            last_cfg_rev: crate::auralsle_ai_status::config::revision(),
        }
    }

    fn mutate(&mut self, f: impl FnOnce(&mut CustomLayout)) {
        let before = self.draft.clone();
        f(&mut self.draft);
        if self.draft != before {
            self.undo.push(before);
            if self.undo.len() > 80 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            let cur = std::mem::replace(&mut self.draft, prev);
            self.redo.push(cur);
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            let cur = std::mem::replace(&mut self.draft, next);
            self.undo.push(cur);
        }
    }
}

fn auralsle_slider(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    title: &str,
    value: &mut f32,
    min: f32,
    max: f32,
    step: f32,
    suffix: &str,
    precision: usize,
) -> bool {
    let ctx = ui.ctx();
    let avail_w = crate::extras::layout_avail_w(ui).max(180.0);
    let accent = Color32::from_rgb(10, 132, 255);
    let track_bg = settings_color(ctx, Color32::from_rgb(64, 64, 64), Color32::from_rgb(218, 222, 230));
    let title_col = settings_color(ctx, Color32::WHITE, Color32::from_rgb(24, 24, 28));
    let prev = *value;

    ui.horizontal(|ui| {
        ui.set_width(avail_w);
        ui.label(egui::RichText::new(title).size(14.0).color(title_col));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let val_txt = if precision == 0 {
                format!("{}{}", (*value).round() as i32, suffix)
            } else {
                format!("{:.*} {suffix}", precision, *value).trim().to_owned()
            };
            ui.label(egui::RichText::new(val_txt).size(13.5).strong().color(accent));
        });
    });
    ui.add_space(6.0);

    let h = 24.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(avail_w, h), egui::Sense::click_and_drag());
    let span = (max - min).max(1e-4);
    let mut frac = ((*value - min) / span).clamp(0.0, 1.0);
    if resp.is_pointer_button_down_on() || resp.clicked() || resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let f = ((pos.x - (rect.left() + 10.0)) / (rect.width() - 20.0).max(1.0)).clamp(0.0, 1.0);
            let raw = min + f * span;
            let snapped = if step > 0.0 {
                ((raw / step).round() * step).clamp(min, max)
            } else {
                raw.clamp(min, max)
            };
            *value = snapped;
            frac = ((snapped - min) / span).clamp(0.0, 1.0);
        }
    }

    let _ = &id_salt;
    let track_rect = Rect::from_min_max(
        egui::pos2(rect.left() + 4.0, rect.center().y - 2.2),
        egui::pos2(rect.right() - 4.0, rect.center().y + 2.2),
    );
    ui.painter().rect_filled(track_rect, 2.2, track_bg);
    let fill_w = (track_rect.width() * frac).max(4.4);
    let fill_rect = Rect::from_min_size(track_rect.min, egui::vec2(fill_w.min(track_rect.width()), 4.4));
    ui.painter().rect_filled(fill_rect, 2.2, accent);

    let thumb_x = (track_rect.left() + 7.0 + (track_rect.width() - 14.0) * frac)
        .clamp(track_rect.left() + 7.0, track_rect.right() - 7.0);
    let thumb_c = egui::pos2(thumb_x, track_rect.center().y);
    ui.painter().circle_filled(thumb_c, 7.2, Color32::WHITE);
    ui.painter().circle_stroke(thumb_c, 7.2, Stroke::new(1.2, accent));

    (*value - prev).abs() > 1e-4
}

#[allow(dead_code)]
fn auralsle_pill_tabs(ui: &mut egui::Ui, labels: &[&str], active: &mut usize) {
    let ctx = ui.ctx();
    let dark = ctx.style().visuals.dark_mode;
    let avail = crate::extras::layout_avail_w(ui).max(200.0);
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(avail);
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
        for (idx, lbl) in labels.iter().enumerate() {
            let on = *active == idx;
            let tw = ui.fonts(|f| {
                f.layout_no_wrap((*lbl).to_owned(), egui::FontId::proportional(13.0), Color32::WHITE)
                    .size()
                    .x
            });
            let (r, resp) = ui.allocate_exact_size(egui::vec2(tw + 24.0, 30.0), egui::Sense::click());
            let bg = if on {
                Color32::from_rgb(50, 56, 68)
            } else if resp.hovered() {
                if dark { Color32::from_rgb(38, 42, 49) } else { Color32::from_rgb(228, 232, 238) }
            } else if dark {
                Color32::from_rgb(30, 33, 39)
            } else {
                Color32::from_rgb(240, 242, 246)
            };
            let stroke_col = if on {
                if dark { Color32::from_rgb(84, 92, 108) } else { Color32::from_rgb(180, 188, 200) }
            } else {
                Color32::TRANSPARENT
            };
            let text_col = if on {
                if dark { Color32::WHITE } else { Color32::from_rgb(20, 24, 30) }
            } else if dark {
                Color32::from_rgb(174, 180, 192)
            } else {
                Color32::from_rgb(88, 94, 104)
            };
            ui.painter().rect_filled(r, 8.0, bg);
            if on {
                ui.painter().rect_stroke(r, 8.0, Stroke::new(1.0, stroke_col), egui::StrokeKind::Inside);
            }
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                *lbl,
                egui::FontId::proportional(13.0),
                text_col,
            );
            if resp.clicked() {
                *active = idx;
            }
        }
    });
}

fn color_row_hex(ui: &mut egui::Ui, title: &str, hex: &mut String, default_hex: &str) -> bool {
    let mut changed = false;
    crate::extras::settings_card(ui, |ui| {
        let avail_w = crate::extras::layout_avail_w(ui).max(200.0);
        let parsed = auralsle_music::parse_hex_color(hex)
            .unwrap_or_else(|| auralsle_music::parse_hex_color(default_hex).unwrap_or(Color32::WHITE));
        let mut rgb = [parsed.r(), parsed.g(), parsed.b()];
        ui.horizontal(|ui| {
            ui.set_width(avail_w);
            ui.label(egui::RichText::new(title).size(14.5));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::extras::twinkle_button(ui, "重置", false).clicked() {
                    *hex = default_hex.to_owned();
                    changed = true;
                }
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    *hex = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
                    changed = true;
                }
                let edit = egui::TextEdit::singleline(hex)
                    .desired_width(82.0)
                    .font(egui::TextStyle::Monospace);
                if ui.add(edit).changed() {
                    changed = true;
                }
            });
        });
    });
    changed
}

fn draw_wireframe_layout_cards(ui: &mut egui::Ui, selected: &mut String) -> bool {
    let ctx = ui.ctx().clone();
    let dark = ctx.style().visuals.dark_mode;
    let avail = crate::extras::layout_avail_w(ui).max(260.0);
    let gap = 10.0;
    let cw = ((avail - gap * 2.0) / 3.0).max(80.0);
    let ch = 152.0;
    let blue = Color32::from_rgb(10, 132, 255);
    let mut changed = false;

    let options = [
        ("并排", "SideBySide", "封面与歌词并排"),
        ("分离", "Separated", "信息与歌词上下分区"),
        ("歌词居中", "LyricFocus", "居中的独立歌词区"),
    ];

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (title, key, desc) in options {
            let on = selected == key;
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(cw, ch), egui::Sense::click());
            let bg = if on {
                if dark { Color32::from_rgb(19, 24, 34) } else { Color32::from_rgb(232, 241, 255) }
            } else if dark {
                Color32::from_rgb(43, 43, 43)
            } else {
                Color32::from_rgb(245, 246, 249)
            };
            let border = if on {
                Stroke::new(2.0, blue)
            } else if resp.hovered() {
                Stroke::new(1.0, Color32::from_gray(if dark { 90 } else { 185 }))
            } else {
                Stroke::new(1.0, Color32::from_gray(if dark { 56 } else { 218 }))
            };
            ui.painter().rect_filled(rect, 14.0, bg);
            ui.painter().rect_stroke(rect, 14.0, border, egui::StrokeKind::Inside);

            let p = ui.painter();
            let ink_bar = if dark { Color32::WHITE } else { Color32::from_rgb(30, 34, 42) };
            let dim_bar = if dark { Color32::from_rgb(56, 61, 70) } else { Color32::from_rgb(210, 215, 224) };
            let box_col = if dark { Color32::from_rgb(50, 55, 65) } else { Color32::from_rgb(222, 227, 236) };

            match key {
                "SideBySide" => {
                    p.rect_filled(Rect::from_min_size(rect.min + egui::vec2(12.0, 14.0), egui::vec2(34.0, 34.0)), 7.0, box_col);
                    p.rect_filled(Rect::from_min_size(rect.min + egui::vec2(54.0, 18.0), egui::vec2((cw - 68.0).max(20.0), 6.0)), 3.0, ink_bar);
                    p.rect_filled(Rect::from_min_size(rect.min + egui::vec2(54.0, 30.0), egui::vec2((cw - 84.0).max(14.0), 5.0)), 2.5, dim_bar);
                    p.rect_filled(Rect::from_min_size(rect.min + egui::vec2(54.0, 52.0), egui::vec2((cw - 68.0).max(20.0), 6.0)), 3.0, ink_bar);
                }
                "Separated" => {
                    p.rect_filled(Rect::from_min_size(rect.min + egui::vec2(14.0, 14.0), egui::vec2((cw - 28.0).max(20.0), 7.0)), 3.5, ink_bar);
                    p.rect_filled(Rect::from_min_size(rect.min + egui::vec2(14.0, 48.0), egui::vec2((cw - 28.0).max(20.0), 7.0)), 3.5, ink_bar);
                    p.rect_filled(Rect::from_min_size(rect.min + egui::vec2(14.0, 76.0), egui::vec2((cw - 56.0).max(16.0), 5.0)), 2.5, dim_bar);
                }
                _ => {
                    p.rect_filled(Rect::from_min_size(egui::pos2(rect.center().x - 28.0, rect.top() + 12.0), egui::vec2(56.0, 26.0)), 6.0, box_col);
                    p.rect_filled(Rect::from_min_size(rect.min + egui::vec2(16.0, 54.0), egui::vec2((cw - 32.0).max(20.0), 7.0)), 3.5, ink_bar);
                    p.rect_filled(Rect::from_min_size(rect.min + egui::vec2(16.0, 72.0), egui::vec2((cw - 60.0).max(16.0), 5.0)), 2.5, dim_bar);
                }
            }

            let title_col = settings_color(&ctx, Color32::WHITE, Color32::from_rgb(20, 24, 30));
            let desc_col = settings_color(&ctx, Color32::from_rgb(162, 168, 178), Color32::from_rgb(95, 102, 114));
            p.text(
                egui::pos2(rect.left() + 14.0, rect.bottom() - 42.0),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional(13.5),
                title_col,
            );
            p.text(
                egui::pos2(rect.left() + 14.0, rect.bottom() - 22.0),
                egui::Align2::LEFT_CENTER,
                desc,
                egui::FontId::proportional(11.0),
                desc_col,
            );

            if resp.clicked() {
                *selected = key.to_owned();
                changed = true;
            }
        }
    });
    changed
}

fn draw_bar_style_strip_preview(
    ui: &mut egui::Ui,
    bar_style: BarStyle,
    thumb_style: ThumbStyle,
    thumb_on: bool,
    spin: bool,
    live_accent: Option<Color32>,
) {
    let ctx = ui.ctx().clone();
    let avail = crate::extras::layout_avail_w(ui).max(200.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(avail, 48.0), egui::Sense::hover());
    let bg = settings_color(&ctx, Color32::from_rgb(32, 32, 32), Color32::from_rgb(235, 239, 246));
    ui.painter().rect_filled(rect, 12.0, bg);
    let card = Card::new(ui, rect.min, rect);
    let accent = live_accent.unwrap_or(Color32::from_rgb(10, 132, 255));
    let track = settings_color(&ctx, Color32::from_rgb(66, 72, 82), Color32::from_rgb(196, 204, 216));
    let t = ui.input(|i| i.time);
    let th = if thumb_on { thumb_style } else { ThumbStyle::None };
    auralsle_music::draw_progress_bar(
        &card,
        auralsle_music::DRect::new(20.0, 10.0, avail - 40.0, 28.0),
        0.42,
        (t * 2.2) as f32,
        0.95,
        18.0,
        1.0,
        if spin { (t * 0.6) as f32 } else { 0.0 },
        accent,
        track,
        Color32::from_gray(145),
        bar_style,
        th,
    );
}

fn draw_inspector_slider(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    label: &str,
    value: &mut f32,
    min: f32,
    max: f32,
    step: f32,
    suffix: &str,
    avail_w: f32,
) -> bool {
    let ctx = ui.ctx();
    let w = avail_w.max(120.0);
    let accent = Color32::from_rgb(10, 132, 255);
    let track_bg = settings_color(ctx, Color32::from_rgb(64, 64, 64), Color32::from_rgb(218, 222, 230));
    let title_col = settings_color(ctx, Color32::from_rgb(225, 230, 238), Color32::from_rgb(30, 34, 42));
    let prev = *value;

    ui.horizontal(|ui| {
        ui.set_width(w);
        ui.label(egui::RichText::new(label).size(12.5).color(title_col));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let val_txt = format!("{}{}", (*value).round() as i32, suffix);
            ui.label(egui::RichText::new(val_txt).size(12.5).strong().color(accent));
        });
    });
    ui.add_space(2.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 20.0), egui::Sense::click_and_drag());
    let span = (max - min).max(1e-4);
    let mut frac = ((*value - min) / span).clamp(0.0, 1.0);
    if resp.is_pointer_button_down_on() || resp.clicked() || resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let f = ((pos.x - (rect.left() + 8.0)) / (rect.width() - 16.0).max(1.0)).clamp(0.0, 1.0);
            let raw = min + f * span;
            let snapped = if step > 0.0 {
                ((raw / step).round() * step).clamp(min, max)
            } else {
                raw.clamp(min, max)
            };
            *value = snapped;
            frac = ((*value - min) / span).clamp(0.0, 1.0);
        }
    }
    let _ = id_salt;
    let cy = rect.center().y;
    let x0 = rect.left() + 8.0;
    let x1 = rect.right() - 8.0;
    let tw = (x1 - x0).max(1.0);
    let kx = x0 + tw * frac;
    let p = ui.painter();
    p.rect_filled(Rect::from_min_max(Pos2::new(x0, cy - 2.0), Pos2::new(x1, cy + 2.0)), 2.0, track_bg);
    if kx > x0 + 0.5 {
        p.rect_filled(Rect::from_min_max(Pos2::new(x0, cy - 2.0), Pos2::new(kx, cy + 2.0)), 2.0, accent);
    }
    let r = if resp.hovered() || resp.dragged() { 7.0 } else { 6.0 };
    p.circle_filled(Pos2::new(kx, cy), r, Color32::WHITE);
    p.circle_stroke(Pos2::new(kx, cy), r, Stroke::new(1.5, accent));
    ui.add_space(4.0);
    (*value - prev).abs() > 1e-4
}

fn draw_inspector_chip(ui: &mut egui::Ui, w: f32, h: f32, label: &str, selected: bool) -> bool {
    let ctx = ui.ctx().clone();
    let dark = ctx.style().visuals.dark_mode;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let blue = Color32::from_rgb(10, 132, 255);
    let bg = if selected {
        if dark { Color32::from_rgb(48, 54, 64) } else { Color32::from_rgb(224, 237, 255) }
    } else if resp.hovered() {
        if dark { Color32::from_rgb(64, 64, 64) } else { Color32::from_rgb(234, 238, 245) }
    } else if dark {
        Color32::from_rgb(38, 41, 47)
    } else {
        Color32::from_rgb(242, 244, 248)
    };
    ui.painter().rect_filled(rect, (h * 0.45).min(14.0), bg);
    if selected {
        ui.painter().rect_stroke(rect, (h * 0.45).min(14.0), Stroke::new(1.2, blue), egui::StrokeKind::Inside);
    }
    let text_col = if selected {
        settings_color(&ctx, Color32::WHITE, Color32::from_rgb(12, 70, 150))
    } else {
        settings_color(&ctx, Color32::from_rgb(200, 206, 218), Color32::from_rgb(65, 70, 82))
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.0),
        text_col,
    );
    resp.clicked()
}

pub(crate) fn show_auralsle_music_settings(
    ui: &mut egui::Ui,
    app: &mut crate::app::App,
    mcfg: &mut crate::auralsle_ai_status::config::Config,
) {
    let ctx = ui.ctx().clone();
    let tab_id = egui::Id::new("auralsle_music_subtab");
    let mut subtab = ctx.data(|d| d.get_temp::<usize>(tab_id)).unwrap_or(0).min(4);

    crate::extras::settings_segmented(
        ui,
        &mut subtab,
        &[
            (0usize, "尺寸与排版"),
            (1usize, "动态取色"),
            (2usize, "交互与提示"),
            (3usize, "光效与动效"),
            (4usize, "布局编辑器"),
        ],
        86.0,
    );
    ctx.data_mut(|d| d.insert_temp(tab_id, subtab));
    ui.add_space(12.0);

    match subtab {
        0 => show_tab_size_layout(ui, app, mcfg),
        1 => show_tab_material(ui, mcfg),
        2 => show_tab_interactions(ui, app, mcfg),
        3 => show_tab_effects_motion(ui, app, mcfg),
        _ => show_tab_layout_editor(ui, app, mcfg),
    }

    if !cfg!(test) && !ctx.data(|d| d.get_temp::<bool>(egui::Id::new("auralsle_settings_resizing")).unwrap_or(false)) {
        ctx.request_repaint_after(Duration::from_millis(66));
    }
}

fn show_tab_size_layout(
    ui: &mut egui::Ui,
    app: &mut crate::app::App,
    mcfg: &mut crate::auralsle_ai_status::config::Config,
) {
    crate::extras::settings_label(ui, "音乐展开排版");
    ui.label(
        egui::RichText::new("选择一种布局；经典与 Mini 共用，同一首音乐会连续切换。")
            .size(12.0)
            .color(settings_color(&ui.ctx(), Color32::from_gray(165), Color32::from_gray(95))),
    );
    ui.add_space(8.0);
    if draw_wireframe_layout_cards(ui, &mut mcfg.auralsle_music_layout) {
        let nl = mcfg.auralsle_music_layout.clone();
        crate::auralsle_ai_status::config::update(|c| c.auralsle_music_layout = nl);
    }

    crate::extras::settings_label(ui, "音乐岛实时预览");
    crate::extras::settings_card(ui, |ui| {
        let avail_w = crate::extras::layout_avail_w(ui).max(240.0);
        let card_w = crate::app::PW;
        let (geom, custom_opt) = auralsle_music::active_geometry(mcfg, card_w, app.auralsle_music_mix);
        let card_h = (geom.height + 14.0).clamp(196.0, 290.0);
        let scale_k = ((avail_w - 8.0) / card_w).clamp(0.56, 1.05);
        let draw_w = card_w * scale_k;
        let draw_h = card_h * scale_k;
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, draw_h + 10.0), egui::Sense::hover());
            let island_r = Rect::from_center_size(rect.center(), egui::vec2(draw_w, draw_h));
            let pcard = Card::new(ui, island_r.min, island_r).scaled(0.0, 0.0, scale_k);
            let bar_style = custom_opt
                .as_ref()
                .map(|l| BarStyle::from_setting(&l.bar_style))
                .unwrap_or_else(|| BarStyle::from_setting(&mcfg.auralsle_bar_style));
            let thumb_en = custom_opt.as_ref().map(|l| l.thumb_enabled).unwrap_or(mcfg.auralsle_thumb_enabled);
            let thumb_style = if thumb_en {
                custom_opt
                    .as_ref()
                    .map(|l| ThumbStyle::from_setting(&l.thumb_style))
                    .unwrap_or_else(|| ThumbStyle::from_setting(&mcfg.auralsle_thumb_style))
            } else {
                ThumbStyle::None
            };
            let thumb_spin = custom_opt.as_ref().map(|l| l.thumb_spin).unwrap_or(mcfg.auralsle_thumb_spin);
            let transport_theme = custom_opt
                .as_ref()
                .map(|l| TransportTheme::from_setting(&l.transport_theme))
                .unwrap_or_else(|| TransportTheme::from_setting(&mcfg.auralsle_transport_theme));
            let has = !app.media.title.trim().is_empty();
            let title_s = if has { app.media.title.as_str() } else { "我还在树下等一个人" };
            let artist_s = if has { app.media.artist.as_str() } else { "陆虎" };
            let lyr_s: String = if has {
                app.island_lyric().map(|(_, s)| s.to_string()).unwrap_or_else(|| "树下伊人怡人永久".to_string())
            } else {
                "树下伊人怡人永久".to_string()
            };
            let card_radius = if app.settings.island_shape == crate::model::IslandShape::Pill { 24.0 } else { 14.0 };
            let _ = auralsle_music::paint_auralsle_music_card(
                &pcard,
                mcfg,
                &mut app.auralsle_blur_cache,
                &auralsle_music::CardInput {
                    width: card_w,
                    surface_height: card_h,
                    card_radius,
                    time_secs: app.t,
                    title: title_s,
                    artist: artist_s,
                    has_track: true,
                    playing: if has { app.media.playing } else { true },
                    position: if has { app.media.position } else { 132.0 },
                    duration: if has && app.media.duration > 0.0 { app.media.duration } else { 224.0 },
                    lyric: &lyr_s,
                    secondary: "那树下守一人 朝朝暮暮相守",
                    highlight: 0.58,
                    lyric_age_phase: 1.0,
                    artwork: app.music_artwork.as_ref(),
                    artwork_tint: Some(Color32::from_rgb(56, 120, 210)),
                    volume_level: app.music_volume_ui.or(Some((80, false))),
                    liked: app.auralsle_music_liked,
                    shuffle_mode: app.auralsle_music_shuffle,
                    geometry: geom,
                    bar_style,
                    thumb_style,
                    thumb_spin,
                    transport_theme,
                    allow_seek_and_buttons: false,
                    skip_surface: false,
                    id_salt: "auralsle-live-preview-card",
                },
            );
        });
    });
}

fn show_tab_material(
    ui: &mut egui::Ui,
    mcfg: &mut crate::auralsle_ai_status::config::Config,
) {
    crate::extras::settings_label(ui, "动态色彩");
    crate::extras::settings_card(ui, |ui| {
        ui.label(egui::RichText::new("灵动岛颜色来源").size(14.5).strong());
        ui.add_space(6.0);
        let mut src_idx = match mcfg.auralsle_color_source.as_str() {
            "System" => 1usize,
            "Custom" => 2usize,
            _ => 0usize,
        };
        if crate::extras::settings_segmented(
            ui,
            &mut src_idx,
            &[(0usize, "专辑封面"), (1usize, "系统强调色"), (2usize, "自选主色")],
            96.0,
        ) {
            let s = match src_idx {
                1 => "System",
                2 => "Custom",
                _ => "Content",
            };
            mcfg.auralsle_color_source = s.to_owned();
            crate::auralsle_ai_status::config::update(|c| c.auralsle_color_source = s.to_owned());
        }
    });
    if mcfg.auralsle_color_source == "Custom" {
        if color_row_hex(ui, "自选灵动岛动态主色", &mut mcfg.auralsle_dynamic_seed, "#A9E6CD") {
            let v = mcfg.auralsle_dynamic_seed.clone();
            crate::auralsle_ai_status::config::update(|c| c.auralsle_dynamic_seed = v);
        }
    }

    let mut cap_dyn = mcfg.auralsle_capsule_dynamic_color;
    crate::extras::setting_row_card(ui, "胶囊与收起态动态色彩", "收起形态的独立氛围取色开关，与展开卡片互不影响", &mut cap_dyn);
    if cap_dyn != mcfg.auralsle_capsule_dynamic_color {
        mcfg.auralsle_capsule_dynamic_color = cap_dyn;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_capsule_dynamic_color = cap_dyn);
    }

    let mut exp_dyn = mcfg.auralsle_expanded_dynamic_color;
    crate::extras::setting_row_card(ui, "展开卡片动态色彩", "从专辑封面 / 系统强调色生成背景、进度条与组件配色", &mut exp_dyn);
    if exp_dyn != mcfg.auralsle_expanded_dynamic_color {
        mcfg.auralsle_expanded_dynamic_color = exp_dyn;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_expanded_dynamic_color = exp_dyn);
    }

    let mut lyr_dyn = mcfg.auralsle_lyric_cover_color;
    crate::extras::setting_row_card(ui, "歌词填色动态色彩", "跟随动态光源自动计算高对比荧光歌词高亮色；关闭后使用下方固定颜色", &mut lyr_dyn);
    if lyr_dyn != mcfg.auralsle_lyric_cover_color {
        mcfg.auralsle_lyric_cover_color = lyr_dyn;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_lyric_cover_color = lyr_dyn);
    }

    let mut overlay = mcfg.auralsle_cover_overlay;
    crate::extras::setting_row_card(ui, "封面磨砂叠层", "在亚克力展开卡片背景中叠加柔和的高斯模糊专辑封面", &mut overlay);
    if overlay != mcfg.auralsle_cover_overlay {
        mcfg.auralsle_cover_overlay = overlay;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_cover_overlay = overlay);
    }

    let mut flow_bg = mcfg.salt_flow_bg;
    crate::extras::setting_row_card(ui, "音乐岛背流光动效", "参考 Salt Player 流体网格算法，从专辑封面取色并生成多极柔和流动极光背景", &mut flow_bg);
    if flow_bg != mcfg.salt_flow_bg {
        mcfg.salt_flow_bg = flow_bg;
        crate::auralsle_ai_status::config::update(|c| c.salt_flow_bg = flow_bg);
    }

    crate::extras::settings_label(ui, "固定颜色（关闭动态色彩后生效）");
    if color_row_hex(ui, "展开卡片文字", &mut mcfg.auralsle_text_color, "#F4F6F8") {
        let v = mcfg.auralsle_text_color.clone();
        crate::auralsle_ai_status::config::update(|c| c.auralsle_text_color = v);
    }
    if color_row_hex(ui, "展开卡片组件", &mut mcfg.auralsle_component_color, "#B1E8D1") {
        let v = mcfg.auralsle_component_color.clone();
        crate::auralsle_ai_status::config::update(|c| c.auralsle_component_color = v);
    }
    if color_row_hex(ui, "歌词进度高亮", &mut mcfg.auralsle_lyric_progress_color, "#A9E6CD") {
        let v = mcfg.auralsle_lyric_progress_color.clone();
        crate::auralsle_ai_status::config::update(|c| c.auralsle_lyric_progress_color = v);
    }

}

fn show_tab_interactions(
    ui: &mut egui::Ui,
    app: &mut crate::app::App,
    mcfg: &mut crate::auralsle_ai_status::config::Config,
) {
    let _ = app;
    let mut click_open = mcfg.auralsle_click_open_player;
    crate::extras::setting_row_card(ui, "点击封面或歌名打开播放器", "在展开卡片中点击专辑封面或歌曲标题时呼出正在播放的音乐软件窗口", &mut click_open);
    if click_open != mcfg.auralsle_click_open_player {
        mcfg.auralsle_click_open_player = click_open;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_click_open_player = click_open);
    }

    let mut tip = mcfg.auralsle_hover_tooltip;
    crate::extras::setting_row_card(ui, "按钮悬停提示气泡", "悬停音量或翻译按钮时在上方展示操作与百分比提示胶囊", &mut tip);
    if tip != mcfg.auralsle_hover_tooltip {
        mcfg.auralsle_hover_tooltip = tip;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_hover_tooltip = tip);
    }

    crate::extras::settings_label(ui, "灵屿歌词扩展");
    let mut auto_w = mcfg.auralsle_auto_compact_width;
    crate::extras::setting_row_card(ui, "随歌词调整灵动岛宽度", "根据当前歌词和译文自动伸缩；关闭后使用固定宽度", &mut auto_w);
    if auto_w != mcfg.auralsle_auto_compact_width {
        mcfg.auralsle_auto_compact_width = auto_w;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_auto_compact_width = auto_w);
    }

    let mut wh = mcfg.auralsle_word_highlight;
    crate::extras::setting_row_card(ui, "逐字高亮与发光", "跟随演唱进度点亮并辉光渲染当前歌词原文", &mut wh);
    if wh != mcfg.auralsle_word_highlight {
        mcfg.auralsle_word_highlight = wh;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_word_highlight = wh);
    }

    let mut ct = mcfg.auralsle_compact_translation;
    crate::extras::setting_row_card(ui, "收起胶囊显示翻译", "收起时有双语译文则附带较小译文/下一句", &mut ct);
    if ct != mcfg.auralsle_compact_translation {
        mcfg.auralsle_compact_translation = ct;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_compact_translation = ct);
    }

    let mut c2 = mcfg.auralsle_chinese_two_line;
    crate::extras::setting_row_card(ui, "中文双行（下一句提示）", "没有外语翻译时，在第二行柔和预告下一句歌词", &mut c2);
    if c2 != mcfg.auralsle_chinese_two_line {
        mcfg.auralsle_chinese_two_line = c2;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_chinese_two_line = c2);
    }

    let mut st = mcfg.auralsle_show_translation;
    crate::extras::setting_row_card(ui, "展开卡片显示翻译", "展开后在主歌词下方优先显示可用中文翻译", &mut st);
    if st != mcfg.auralsle_show_translation {
        mcfg.auralsle_show_translation = st;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_show_translation = st);
    }

    let mut tb = mcfg.auralsle_translation_button;
    crate::extras::setting_row_card(ui, "显示译按钮（文A）", "在展开卡片右下角显示翻译语言切换按钮", &mut tb);
    if tb != mcfg.auralsle_translation_button {
        mcfg.auralsle_translation_button = tb;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_translation_button = tb);
    }

}

fn show_tab_effects_motion(
    ui: &mut egui::Ui,
    app: &crate::app::App,
    mcfg: &mut crate::auralsle_ai_status::config::Config,
) {
    crate::extras::settings_label(ui, "岛周围的光效");
    let mut glow_on = mcfg.auralsle_music_glow;
    crate::extras::setting_row_card(ui, "音乐外围光晕", "跟随播放柔和呼吸或读取系统音频峰值实时律动发光", &mut glow_on);
    if glow_on != mcfg.auralsle_music_glow {
        mcfg.auralsle_music_glow = glow_on;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_music_glow = glow_on);
    }
    crate::extras::settings_card(ui, |ui| {
        ui.label(egui::RichText::new("音乐光晕方式").size(14.0).strong());
        ui.add_space(6.0);
        let mut gm = if mcfg.auralsle_music_glow_mode == "Rhythm" { 1usize } else { 0usize };
        if crate::extras::settings_segmented(ui, &mut gm, &[(0usize, "柔和呼吸"), (1usize, "音乐律动")], 120.0) {
            let mode = if gm == 1 { "Rhythm" } else { "Breath" };
            mcfg.auralsle_music_glow_mode = mode.to_owned();
            crate::auralsle_ai_status::config::update(|c| c.auralsle_music_glow_mode = mode.to_owned());
        }
    });

    let _ = app;
}

/// 进度条风格与游标 + 播放按钮主题（已推广应用至全部音乐岛 UI 样式）。
pub(crate) fn show_music_bar_and_transport_settings(
    ui: &mut egui::Ui,
    mcfg: &mut crate::auralsle_ai_status::config::Config,
    live_accent: Option<Color32>,
) {
    crate::extras::settings_label(ui, "进度条风格与游标");
    crate::extras::settings_card(ui, |ui| {
        let avail = crate::extras::layout_avail_w(ui).max(200.0);
        let cw = ((avail - 24.0) / 4.0).max(52.0);
        for row in BarStyle::ALL.chunks(4) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for &bs in row {
                    if draw_inspector_chip(ui, cw, 34.0, bs.label(), mcfg.auralsle_bar_style == bs.key()) {
                        let k = bs.key().to_owned();
                        mcfg.auralsle_bar_style = k.clone();
                        crate::auralsle_ai_status::config::update(|c| {
                            c.auralsle_bar_style = k.clone();
                            let mut cl = CustomLayout::from_json_str(&c.auralsle_custom_layout_json);
                            cl.bar_style = k;
                            c.auralsle_custom_layout_json = cl.to_json_string();
                        });
                    }
                }
            });
            ui.add_space(6.0);
        }
        draw_bar_style_strip_preview(
            ui,
            BarStyle::from_setting(&mcfg.auralsle_bar_style),
            ThumbStyle::from_setting(&mcfg.auralsle_thumb_style),
            mcfg.auralsle_thumb_enabled,
            mcfg.auralsle_thumb_spin,
            live_accent,
        );
    });

    let mut th_en = mcfg.auralsle_thumb_enabled;
    crate::extras::setting_row_card(ui, "显示进度游标", "开启后在进度条末端绘制可自定义的游标形状", &mut th_en);
    if th_en != mcfg.auralsle_thumb_enabled {
        mcfg.auralsle_thumb_enabled = th_en;
        crate::auralsle_ai_status::config::update(|c| c.auralsle_thumb_enabled = th_en);
    }
    if mcfg.auralsle_thumb_enabled {
        let mut spin = mcfg.auralsle_thumb_spin;
        crate::extras::setting_row_card(ui, "游标缓慢旋转", "播放期间旋转花瓣、四叶、饼干或三角游标", &mut spin);
        if spin != mcfg.auralsle_thumb_spin {
            mcfg.auralsle_thumb_spin = spin;
            crate::auralsle_ai_status::config::update(|c| c.auralsle_thumb_spin = spin);
        }
        crate::extras::settings_card(ui, |ui| {
            let avail = crate::extras::layout_avail_w(ui).max(200.0);
            let cw = ((avail - 24.0) / 4.0).max(52.0);
            for row in ThumbStyle::ALL.chunks(4) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for &ts in row {
                        if draw_inspector_chip(ui, cw, 32.0, ts.label(), mcfg.auralsle_thumb_style == ts.key()) {
                            let k = ts.key().to_owned();
                            mcfg.auralsle_thumb_style = k.clone();
                            crate::auralsle_ai_status::config::update(|c| c.auralsle_thumb_style = k);
                        }
                    }
                });
                ui.add_space(6.0);
            }
        });
    }

    crate::extras::settings_label(ui, "播放按钮主题");
    crate::extras::settings_card(ui, |ui| {
        let ctx = ui.ctx().clone();
        let dark = ctx.style().visuals.dark_mode;
        let avail = crate::extras::layout_avail_w(ui).max(220.0);
        let gap = 10.0;
        let cw = ((avail - gap * 2.0) / 3.0).max(70.0);
        let ch = 92.0;
        let blue = Color32::from_rgb(10, 132, 255);
        for chunk in TransportTheme::ALL.chunks(3) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for &tt in chunk {
                    let on = mcfg.auralsle_transport_theme == tt.key();
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(cw, ch), egui::Sense::click());
                    let bg = if on {
                        if dark { Color32::from_rgb(30, 40, 56) } else { Color32::from_rgb(232, 242, 255) }
                    } else if dark {
                        Color32::from_rgb(43, 43, 43)
                    } else {
                        Color32::from_rgb(245, 246, 249)
                    };
                    ui.painter().rect_filled(r, 12.0, bg);
                    ui.painter().rect_stroke(
                        r,
                        12.0,
                        if on { Stroke::new(2.0, blue) } else { Stroke::new(1.0, Color32::from_gray(if dark { 58 } else { 215 })) },
                        egui::StrokeKind::Inside,
                    );
                    let card = Card::new(ui, r.min, r);
                    let accent = live_accent.unwrap_or(Color32::from_rgb(152, 229, 194));
                    let lum = 0.299 * (accent.r() as f32) + 0.587 * (accent.g() as f32) + 0.114 * (accent.b() as f32);
                    let on_acc = if lum > 148.0 { Color32::from_rgb(20, 26, 30) } else { Color32::WHITE };
                    let ink = if dark { Color32::WHITE } else { Color32::from_rgb(30, 34, 42) };
                    let tonal = paint::mix(
                        accent,
                        if dark { Color32::from_rgb(32, 36, 44) } else { Color32::from_rgb(232, 236, 242) },
                        0.62,
                    );
                    paint::skip(&card, cw * 0.5 - 32.0, 34.0, -1.0, ink);
                    let play_r = auralsle_music::DRect::new(cw * 0.5 - 16.0, 18.0, 32.0, 32.0);
                    let p_ink = auralsle_music::draw_transport_button_surface(
                        &card,
                        4,
                        play_r,
                        tt,
                        accent,
                        on_acc,
                        ink,
                        tonal,
                        paint::mix(accent, Color32::WHITE, 0.22),
                        1.0,
                        0.0,
                    );
                    paint::play_pause(&card, cw * 0.5, 34.0, 12.0, 1.0, p_ink);
                    paint::skip(&card, cw * 0.5 + 32.0, 34.0, 1.0, ink);
                    ui.painter().text(
                        egui::pos2(r.center().x, r.bottom() - 18.0),
                        egui::Align2::CENTER_CENTER,
                        tt.label(),
                        egui::FontId::proportional(12.5),
                        settings_color(&ctx, Color32::WHITE, Color32::from_rgb(24, 24, 28)),
                    );
                    if resp.clicked() {
                        let k = tt.key().to_owned();
                        mcfg.auralsle_transport_theme = k.clone();
                        crate::auralsle_ai_status::config::update(|c| c.auralsle_transport_theme = k);
                    }
                }
            });
            ui.add_space(8.0);
        }
    });
}

fn show_tab_layout_editor(
    ui: &mut egui::Ui,
    app: &mut crate::app::App,
    mcfg: &mut crate::auralsle_ai_status::config::Config,
) {
    let ctx = ui.ctx().clone();
    let dark = ctx.style().visuals.dark_mode;
    let state_id = egui::Id::new("auralsle_layout_editor_state");
    let rev_now = crate::auralsle_ai_status::config::revision();
    let mut ed = ctx
        .data(|d| d.get_temp::<EditorState>(state_id))
        .unwrap_or_else(|| EditorState::from_config(mcfg));
    if ed.last_cfg_rev != rev_now {
        ed.last_cfg_rev = rev_now;
    }

    let avail = crate::extras::layout_avail_w(ui).max(360.0);
    // 顶部工具栏：自定义布局开关 | 撤销 | 重做 | ✓ 应用到灵动岛
    crate::extras::settings_card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.set_width(crate::extras::layout_avail_w(ui).max(200.0));
            ui.label(egui::RichText::new("自定义布局").size(13.5).strong());
            let mut en = ed.draft.enabled;
            if crate::extras::switch(ui, &mut en) {
                ed.mutate(|l| l.enabled = en);
            }
            ui.add_space(6.0);
            if ui.add_enabled(!ed.undo.is_empty(), egui::Button::new("撤销")).clicked() {
                ed.undo();
            }
            if ui.add_enabled(!ed.redo.is_empty(), egui::Button::new("重做")).clicked() {
                ed.redo();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let apply_btn = egui::Button::new(egui::RichText::new("✓ 应用到灵动岛").color(Color32::WHITE).strong())
                    .fill(Color32::from_rgb(10, 132, 255))
                    .corner_radius(16.0);
                if ui.add_sized([128.0, 30.0], apply_btn).clicked() {
                    if ed.draft.validate().is_ok() {
                        let json_str = ed.draft.to_json_string();
                        let bar_s = ed.draft.bar_style.clone();
                        let thumb_s = ed.draft.thumb_style.clone();
                        let thumb_e = ed.draft.thumb_enabled;
                        let thumb_sp = ed.draft.thumb_spin;
                        let tr_t = ed.draft.transport_theme.clone();
                        mcfg.auralsle_custom_layout_json = json_str.clone();
                        mcfg.auralsle_bar_style = bar_s.clone();
                        mcfg.auralsle_thumb_style = thumb_s.clone();
                        mcfg.auralsle_thumb_enabled = thumb_e;
                        mcfg.auralsle_thumb_spin = thumb_sp;
                        mcfg.auralsle_transport_theme = tr_t.clone();
                        ed.baseline = ed.draft.clone();
                        crate::auralsle_ai_status::config::update(|c| {
                            c.auralsle_custom_layout_json = json_str;
                            c.auralsle_bar_style = bar_s;
                            c.auralsle_thumb_style = thumb_s;
                            c.auralsle_thumb_enabled = thumb_e;
                            c.auralsle_thumb_spin = thumb_sp;
                            c.auralsle_transport_theme = tr_t;
                        });
                        app.say("已应用自定义音乐岛布局");
                    }
                }
            });
        });
    });
    ui.add_space(10.0);

    let g = ed.draft.geometry(400.0, mcfg.auralsle_lyric_font_size);
    let col_gap = 12.0;
    let left_w = ((avail - col_gap) * 0.52).max(200.0);
    let right_w = (avail - col_gap - left_w).max(180.0);

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = col_gap;
        // 左侧：实时预览网格画布
        ui.vertical(|ui| {
            ui.set_width(left_w);
            let card_bg = if dark { Color32::from_rgb(43, 43, 43) } else { Color32::WHITE };
            let border = if dark { Color32::from_rgb(56, 56, 56) } else { Color32::from_rgb(225, 225, 225) };
            egui::Frame::new()
                .fill(card_bg)
                .stroke(Stroke::new(1.0, border))
                .corner_radius(14.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    let inner_w = (left_w - 24.0).max(180.0);
                    ui.horizontal(|ui| {
                        ui.set_width(inner_w);
                        ui.label(egui::RichText::new("实时预览").size(13.5).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format!("400 × {:.0}", g.height))
                                    .size(12.0)
                                    .color(settings_color(&ctx, Color32::from_gray(160), Color32::from_gray(100))),
                            );
                        });
                    });
                    ui.add_space(8.0);

                    let scale = (inner_w - 16.0) / 400.0;
                    let canvas_h = (g.height * scale + 56.0).clamp(210.0, 290.0);
                    let (canvas_rect, _) = ui.allocate_exact_size(egui::vec2(inner_w, canvas_h), egui::Sense::hover());
                    let grid_bg = if dark { Color32::from_rgb(32, 32, 32) } else { Color32::from_rgb(243, 243, 243) };
                    ui.painter().rect_filled(canvas_rect, 10.0, grid_bg);
                    let grid_col = if dark { Color32::from_white_alpha(12) } else { Color32::from_black_alpha(12) };
                    let mut gx = canvas_rect.left() + 16.0;
                    while gx < canvas_rect.right() - 2.0 {
                        ui.painter().line_segment([Pos2::new(gx, canvas_rect.top()), Pos2::new(gx, canvas_rect.bottom())], Stroke::new(1.0, grid_col));
                        gx += 16.0;
                    }
                    let mut gy = canvas_rect.top() + 16.0;
                    while gy < canvas_rect.bottom() - 2.0 {
                        ui.painter().line_segment([Pos2::new(canvas_rect.left(), gy), Pos2::new(canvas_rect.right(), gy)], Stroke::new(1.0, grid_col));
                        gy += 16.0;
                    }

                    let card_size = Vec2::new(400.0 * scale, g.height * scale);
                    let card_rect = Rect::from_center_size(canvas_rect.center(), card_size);
                    let base_card = Card::new(ui, card_rect.min, canvas_rect).scaled(0.0, 0.0, scale);

                    // 创建叠加 Draft 样式的临时配置供所见即所得绘制
                    let mut prev_cfg = mcfg.clone();
                    prev_cfg.auralsle_bar_style = ed.draft.bar_style.clone();
                    prev_cfg.auralsle_thumb_style = ed.draft.thumb_style.clone();
                    prev_cfg.auralsle_thumb_enabled = ed.draft.thumb_enabled;
                    prev_cfg.auralsle_thumb_spin = ed.draft.thumb_spin;
                    prev_cfg.auralsle_transport_theme = ed.draft.transport_theme.clone();

                    let has_live = !app.media.title.trim().is_empty();
                    let demo = NowPlaying {
                        title: "Seventeen".to_owned(),
                        artist: "Troye Sivan".to_owned(),
                        playing: true,
                        position: 157.0,
                        duration: 218.0,
                        ..Default::default()
                    };
                    let np = if has_live { &app.media } else { &demo };
                    let lyric_str = if has_live {
                        app.ly
                            .lyrics
                            .as_ref()
                            .and_then(|l| l.index_at(app.lyric_position()).and_then(|i| l.lines.get(i)))
                            .map(|l| l.text.as_str())
                            .filter(|s| !s.is_empty())
                            .unwrap_or("And in the heat of the night saw things I'd nev...")
                    } else {
                        "And in the heat of the night saw things I'd nev..."
                    };
                    let sec_str = if has_live {
                        app.ly
                            .lyrics
                            .as_ref()
                            .and_then(|l| l.index_at(app.lyric_position()).and_then(|i| l.lines.get(i)))
                            .and_then(|l| l.translation.as_deref())
                            .unwrap_or("Na oh oh oh ah ah seventeen")
                    } else {
                        "Na oh oh oh ah ah seventeen"
                    };

                    let blur_cache_id = egui::Id::new("auralsle_editor_blur_cache");
                    let blur_cache = ctx
                        .data(|d| d.get_temp::<Arc<std::sync::Mutex<CoverBlurCache>>>(blur_cache_id))
                        .unwrap_or_else(|| {
                            let arc = Arc::new(std::sync::Mutex::new(CoverBlurCache::default()));
                            ctx.data_mut(|d| d.insert_temp(blur_cache_id, arc.clone()));
                            arc
                        });
                    let mut guard = blur_cache.lock().unwrap_or_else(|e| e.into_inner());
                    let events = auralsle_music::paint_auralsle_music_card(
                        &base_card,
                        &prev_cfg,
                        &mut guard,
                        &CardInput {
                            width: 400.0,
                            surface_height: g.height,
                            card_radius: 26.0,
                            time_secs: ui.input(|i| i.time),
                            title: &np.title,
                            artist: &np.artist,
                            has_track: true,
                            playing: np.playing,
                            position: np.position,
                            duration: np.duration,
                            lyric: lyric_str,
                            secondary: sec_str,
                            highlight: 0.68,
                            lyric_age_phase: 1.0,
                            artwork: app.music_artwork.as_ref(),
                            artwork_tint: None,
                            volume_level: app.music_volume_ui,
                            liked: app.auralsle_music_liked,
                            shuffle_mode: app.auralsle_music_shuffle,
                            geometry: g,
                            bar_style: BarStyle::from_setting(&ed.draft.bar_style),
                            thumb_style: if ed.draft.thumb_enabled {
                                ThumbStyle::from_setting(&ed.draft.thumb_style)
                            } else {
                                ThumbStyle::None
                            },
                            thumb_spin: ed.draft.thumb_spin,
                            transport_theme: TransportTheme::from_setting(&ed.draft.transport_theme),
                            allow_seek_and_buttons: false,
                            skip_surface: false,
                            id_salt: "auralsle-editor-preview",
                        },
                    );
                    drop(guard);

                    if let Some(id) = events.pressed_element {
                        ed.selected = id;
                    }
                    if let Some((eid, delta, active)) = events.dragged_element {
                        if active && (delta.x != 0.0 || delta.y != 0.0) {
                            if ed.drag_id != Some(eid) {
                                if let Some((_, cur_r)) = auralsle_music::elements_of(&g).into_iter().find(|(i, _)| *i == eid) {
                                    ed.drag_id = Some(eid);
                                    ed.drag_before = Some(ed.draft.clone());
                                    ed.drag_pos = (cur_r.x, cur_r.y);
                                }
                            }
                            if ed.drag_id == Some(eid) {
                                ed.drag_pos.0 += delta.x;
                                ed.drag_pos.1 += delta.y;
                                ed.draft.set_element(eid, ed.drag_pos.0, ed.drag_pos.1, None);
                            }
                        } else if !active && ed.drag_id == Some(eid) {
                            if let Some(before) = ed.drag_before.take() {
                                if ed.draft != before {
                                    ed.undo.push(before);
                                    ed.redo.clear();
                                }
                            }
                            ed.drag_id = None;
                        }
                    }

                    // 绘制当前选中元素的蓝色虚线高亮框（直观指示当前正在编辑哪个部件）
                    if let Some((_, sel_r)) = auralsle_music::elements_of(&g).into_iter().find(|(i, _)| *i == ed.selected) {
                        base_card.stroke(
                            sel_r.x - 2.0,
                            sel_r.y - 2.0,
                            sel_r.w + 4.0,
                            sel_r.h + 4.0,
                            4.0,
                            1.5,
                            Color32::from_rgb(56, 168, 255),
                        );
                    }

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let bw = ((inner_w - 8.0) * 0.5).max(80.0);
                        if ui.add_sized([bw, 32.0], egui::Button::new("还原已保存")).clicked() {
                            let saved = ed.baseline.clone();
                            ed.mutate(|l| *l = saved);
                        }
                        if ui.add_sized([bw, 32.0], egui::Button::new("恢复推荐")).clicked() {
                            ed.mutate(|l| *l = CustomLayout::default());
                        }
                    });
                });
        });

        // 右侧：检查器（元素 / 布局 / 模块 / 按钮 / 样式）
        ui.vertical(|ui| {
            ui.set_width(right_w);
            let card_bg = if dark { Color32::from_rgb(43, 43, 43) } else { Color32::WHITE };
            let border = if dark { Color32::from_rgb(56, 56, 56) } else { Color32::from_rgb(225, 225, 225) };
            egui::Frame::new()
                .fill(card_bg)
                .stroke(Stroke::new(1.0, border))
                .corner_radius(14.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    let iw = (right_w - 24.0).max(160.0);
                    let tabs = ["元素", "布局", "模块", "按钮", "样式"];
                    let tw = ((iw - 16.0) / 5.0).max(28.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (idx, lbl) in tabs.iter().enumerate() {
                            if draw_inspector_chip(ui, tw, 28.0, lbl, ed.inspector_tab == idx) {
                                ed.inspector_tab = idx;
                            }
                        }
                    });
                    ui.add_space(10.0);

                    match ed.inspector_tab {
                        0 => {
                            let sel_id = ed.selected;
                            let cur_rect = auralsle_music::elements_of(&g)
                                .into_iter()
                                .find(|(i, _)| *i == sel_id)
                                .map(|(_, r)| r);
                            ui.label(egui::RichText::new(auralsle_music::element_label(sel_id)).size(14.0).strong());
                            ui.add_space(6.0);
                            if let Some(er) = cur_rect {
                                let mut px = er.x;
                                let mut py = er.y;
                                let mut pw = er.w;
                                if draw_inspector_slider(ui, ("el-x", sel_id), "横向位置", &mut px, 12.0, 388.0, 4.0, " px", iw) {
                                    ed.mutate(|l| l.set_element(sel_id, px, py, None));
                                }
                                if draw_inspector_slider(ui, ("el-y", sel_id), "纵向位置", &mut py, 12.0, 280.0, 4.0, " px", iw) {
                                    ed.mutate(|l| l.set_element(sel_id, px, py, None));
                                }
                                if auralsle_music::resizable(sel_id) {
                                    if draw_inspector_slider(ui, ("el-w", sel_id), "元素宽度", &mut pw, 24.0, 376.0, 4.0, " px", iw) {
                                        ed.mutate(|l| l.set_element(sel_id, px, py, Some(pw)));
                                    }
                                }
                            } else {
                                ui.label(egui::RichText::new("当前模块已隐藏，可在「模块」或「按钮」页开启").size(12.0));
                            }
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("选择元素").size(13.0).strong());
                            ui.add_space(6.0);
                            let cw = ((iw - 8.0) * 0.5).max(70.0);
                            for pair in ALL_ELEMENT_IDS.chunks(2) {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    for &eid in pair {
                                        if draw_inspector_chip(ui, cw, 28.0, auralsle_music::element_label(eid), ed.selected == eid) {
                                            ed.selected = eid;
                                        }
                                    }
                                });
                                ui.add_space(5.0);
                            }
                        }
                        1 => {
                            ui.label(egui::RichText::new("文字对齐方式").size(13.0).strong());
                            ui.add_space(6.0);
                            let cw = ((iw - 12.0) / 3.0).max(46.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for (idx, lbl) in [(0u8, "左对齐"), (1u8, "居中"), (2u8, "右对齐")] {
                                    if draw_inspector_chip(ui, cw, 28.0, lbl, ed.draft.alignment == idx) {
                                        ed.mutate(|l| l.alignment = idx);
                                    }
                                }
                            });
                            ui.add_space(10.0);
                            let mut gap_v = ed.draft.gap;
                            if draw_inspector_slider(ui, "ly-gap", "模块间距", &mut gap_v, 4.0, 16.0, 1.0, " px", iw) {
                                ed.mutate(|l| l.gap = gap_v);
                            }
                            let mut cov_sz = ed.draft.cover_size;
                            if draw_inspector_slider(ui, "ly-cov", "封面尺寸", &mut cov_sz, 40.0, 80.0, 2.0, " px", iw) {
                                ed.mutate(|l| l.cover_size = cov_sz);
                            }
                            ui.add_space(6.0);
                            let mut show_c = ed.draft.show_cover;
                            ui.horizontal(|ui| {
                                ui.label("显示专辑封面");
                                if crate::extras::switch(ui, &mut show_c) {
                                    ed.mutate(|l| l.show_cover = show_c);
                                }
                            });
                        }
                        2 => {
                            ui.label(egui::RichText::new("模块显隐与顺序").size(13.0).strong());
                            ui.add_space(6.0);
                            for idx in 0..ed.draft.modules.len() {
                                let kind = ed.draft.modules[idx].kind;
                                let mut vis = ed.draft.modules[idx].visible;
                                ui.horizontal(|ui| {
                                    ui.set_width(iw);
                                    if ui.checkbox(&mut vis, kind.label()).changed() {
                                        ed.mutate(|l| l.modules[idx].visible = vis);
                                    }
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.add_enabled(idx + 1 < ed.draft.modules.len(), egui::Button::new("↓")).clicked() {
                                            ed.mutate(|l| l.move_module(idx, idx + 1));
                                        }
                                        if ui.add_enabled(idx > 0, egui::Button::new("↑")).clicked() {
                                            ed.mutate(|l| l.move_module(idx, idx - 1));
                                        }
                                    });
                                });
                                ui.add_space(4.0);
                            }
                        }
                        3 => {
                            ui.label(egui::RichText::new("底部播控栏按钮").size(13.0).strong());
                            ui.add_space(6.0);
                            let mut remove_idx = None;
                            let mut swap_pair = None;
                            let len = ed.draft.controls.len();
                            for (i, &cid) in ed.draft.controls.iter().enumerate() {
                                let cname = auralsle_music::element_label((32 + cid) as u8);
                                ui.horizontal(|ui| {
                                    ui.set_width(iw);
                                    ui.label(cname);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.add_enabled(cid != 4, egui::Button::new("移除")).clicked() {
                                            remove_idx = Some(i);
                                        }
                                        if ui.add_enabled(i + 1 < len, egui::Button::new("↓")).clicked() {
                                            swap_pair = Some((i, i + 1));
                                        }
                                        if ui.add_enabled(i > 0, egui::Button::new("↑")).clicked() {
                                            swap_pair = Some((i, i - 1));
                                        }
                                    });
                                });
                            }
                            if let Some(idx) = remove_idx {
                                ed.mutate(|l| {
                                    l.controls.remove(idx);
                                });
                            }
                            if let Some((a, b)) = swap_pair {
                                ed.mutate(|l| l.controls.swap(a, b));
                            }
                            ui.add_space(8.0);
                            for cid in [2, 9, 16, 3, 4, 5, 6, 7] {
                                if !ed.draft.controls.contains(&cid) {
                                    let cname = auralsle_music::element_label((32 + cid) as u8);
                                    if ui.button(format!("＋ 添加 {cname}")).clicked() {
                                        ed.mutate(|l| l.controls.push(cid));
                                    }
                                }
                            }
                        }
                        _ => {
                            let cw = ((iw - 12.0) / 3.0).max(46.0);
                            ui.label(egui::RichText::new("按钮风格").size(13.0).strong());
                            ui.add_space(4.0);
                            for chunk in TransportTheme::ALL.chunks(3) {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    for &tt in chunk {
                                        if draw_inspector_chip(ui, cw, 28.0, tt.label(), ed.draft.transport_theme == tt.key()) {
                                            ed.mutate(|l| l.transport_theme = tt.key().to_owned());
                                        }
                                    }
                                });
                                ui.add_space(4.0);
                            }
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("进度条风格").size(13.0).strong());
                            ui.add_space(4.0);
                            for chunk in BarStyle::ALL.chunks(3) {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    for &bs in chunk {
                                        if draw_inspector_chip(ui, cw, 28.0, bs.label(), ed.draft.bar_style == bs.key()) {
                                            ed.mutate(|l| l.bar_style = bs.key().to_owned());
                                        }
                                    }
                                });
                                ui.add_space(4.0);
                            }
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("进度游标").size(13.0).strong());
                            ui.add_space(4.0);
                            for chunk in ThumbStyle::ALL.chunks(3) {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    for &ts in chunk {
                                        if draw_inspector_chip(ui, cw, 26.0, ts.label(), ed.draft.thumb_style == ts.key()) {
                                            ed.mutate(|l| l.thumb_style = ts.key().to_owned());
                                        }
                                    }
                                });
                                ui.add_space(4.0);
                            }
                            ui.add_space(6.0);
                            let mut th_en = ed.draft.thumb_enabled;
                            ui.horizontal(|ui| {
                                ui.label("显示进度游标");
                                if crate::extras::switch(ui, &mut th_en) {
                                    ed.mutate(|l| l.thumb_enabled = th_en);
                                }
                            });
                            let mut th_sp = ed.draft.thumb_spin;
                            ui.horizontal(|ui| {
                                ui.label("游标缓慢旋转");
                                if crate::extras::switch(ui, &mut th_sp) {
                                    ed.mutate(|l| l.thumb_spin = th_sp);
                                }
                            });
                        }
                    }
                });
        });
    });

    ctx.data_mut(|d| d.insert_temp(state_id, ed));
}
