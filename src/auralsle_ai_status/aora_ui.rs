//! 「吉祥物」设置页 — 完整还原 aora-bot.vercel.app 控制台 UI 布局与交互，并集成初代 `scstc/islo` 吉祥物。
//!
//! 全面支持随窗口大小动态缩放与自适应流式排布（窄窗口不裁切、宽窗口自适应放大舞台与网格）：
//! - 顶部分类标签（持久化切换，带「初代 islo」快速切换与「恢复默认」按钮）
//! - 宽窗三栏主控台 / 窄窗双层自适应主控台（左侧 7 种矢量画风 + 动态大预览舞台 + 8 色调色板与 16 种五官 4×4 网格）
//! - 三区独立配色定制卡片（纯 RGB 不透明色块，宽窗单行、窄窗双层自适应，彻底消除文字重叠与半黑半棋盘格）
//! - 底部 11 种吉祥物形象响应式网格（宽窗 11 列一行，中小窗 6+5 双行均衡排布）+ 表情状态响应式矩阵。

use super::aora_face::{
    self, ArtStyle, BodyShape, FaceStyle, StateCat, PALETTE, PALETTE_NAMES, STATES,
};
use super::config;
use super::islo_face::{self, Face as IsloFace};
use crate::paint::Card;
use eframe::egui;

/// 绘制「吉祥物」设置页完整内容（全面跟随设置窗口大小动态缩放，窄窗口不裁切右侧元素，统一使用标准卡片容器）。
pub fn draw_mascot_settings(ui: &mut egui::Ui) {
    let enabled = ui.is_enabled();
    if !enabled {
        ui.multiply_opacity(0.38);
    }
    let dark = ui.visuals().dark_mode;
    // [第 10 批 · 图三未选中底色] 旧版未选中芯片用的是比卡片更暗的 `(38,40,48)`，
    // 读起来像在卡上挖了一排洞，和其余页面的 `bg_ctrl #333` 控件语言不一致。
    // 统一改成「控件底色 + 悬停提亮」：未选中=普通按钮，选中才用强调蓝。
    let card_idle = if dark {
        egui::Color32::from_rgb(0x33, 0x33, 0x33)
    } else {
        egui::Color32::from_rgb(0xe9, 0xec, 0xf2)
    };
    let card_hover = if dark {
        egui::Color32::from_rgb(0x3f, 0x3f, 0x46)
    } else {
        egui::Color32::from_rgb(0xdd, 0xe2, 0xeb)
    };
    let text_idle = if dark {
        egui::Color32::from_gray(210)
    } else {
        egui::Color32::from_rgb(51, 51, 51)
    };
    let sub_text = if dark {
        egui::Color32::from_gray(145)
    } else {
        egui::Color32::from_gray(100)
    };

    let cfg = config::get();
    let mut style_idx = (cfg.aora_style as usize).min(ArtStyle::ALL.len() - 1);
    let mut shape_idx = (cfg.aora_shape as usize).min(BodyShape::ALL.len() - 1);
    let mut color_idx = (cfg.aora_color as usize).min(PALETTE.len() - 1);
    let mut face_idx = (cfg.aora_face_style as usize).min(FaceStyle::ALL.len() - 1);
    let mut fixed = cfg.islo_fixed;
    let mut scale = cfg.aora_scale;
    let mut custom_fill_on = cfg.aora_custom_fill_enabled;
    let mut custom_fill_rgb = if cfg.aora_custom_fill_rgb == [0, 0, 0] && !cfg.aora_custom_fill_enabled {
        let p = PALETTE[color_idx];
        [p.r(), p.g(), p.b()]
    } else {
        cfg.aora_custom_fill_rgb
    };
    let mut custom_fill_a = if cfg.face_fill_alpha == 0 { 255 } else { cfg.face_fill_alpha };
    let mut custom_outline_on = cfg.aora_custom_outline_enabled;
    let mut custom_outline_rgb = if cfg.aora_custom_outline_rgb == [0, 0, 0] && !cfg.aora_custom_outline_enabled {
        [56, 189, 248]
    } else {
        cfg.aora_custom_outline_rgb
    };
    let mut custom_outline_a = if cfg.face_stroke_alpha == 0 { 255 } else { cfg.face_stroke_alpha };
    let mut custom_ink_on = cfg.aora_custom_ink_enabled;
    let mut custom_ink_rgb = if cfg.aora_custom_ink_rgb == [0, 0, 0] && !cfg.aora_custom_ink_enabled {
        [248, 250, 252]
    } else {
        cfg.aora_custom_ink_rgb
    };
    let mut custom_ink_a = if cfg.aora_custom_ink_alpha == 0 { 255 } else { cfg.aora_custom_ink_alpha };
    let mut changed = false;

    // 60fps 平滑预览重绘
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs_f64() % 86400.0) as f32)
        .unwrap_or_else(|_| ui.input(|i| i.time) as f32);
    if enabled {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
    }

    // ── 持久化状态分类与预览表情索引 ──
    let cat_id = egui::Id::new("aora_selected_cat_v2");
    let prev_id = egui::Id::new("aora_preview_state_v2");

    let default_cat_idx = if fixed > 0 && (fixed as usize) <= STATES.len() {
        STATES[(fixed - 1) as usize].cat as usize
    } else {
        0
    };
    let mut selected_cat_idx: usize = ui
        .data(|d| d.get_temp::<usize>(cat_id))
        .unwrap_or(default_cat_idx)
        .min(3);
    let selected_cat = StateCat::ALL[selected_cat_idx];

    let default_preview: usize = if fixed > 0 && (fixed as usize) <= STATES.len() {
        (fixed - 1) as usize
    } else {
        STATES
            .iter()
            .position(|s| s.cat == selected_cat)
            .unwrap_or(0)
    };
    let mut preview_state: usize = ui
        .data(|d| d.get_temp::<usize>(prev_id))
        .unwrap_or(default_preview)
        .min(STATES.len() - 1);
    if STATES[preview_state].cat != selected_cat {
        preview_state = STATES
            .iter()
            .position(|s| s.cat == selected_cat)
            .unwrap_or(0);
    }

    // ════════════════════════════════════════════════════════════════════════
    // 卡片 1：角色预览主控台 + 画风/颜色/16五官 + 配色定制（统一包裹在 settings_card 中，彻底对齐左右边距）
    // ════════════════════════════════════════════════════════════════════════
    crate::extras::settings_card(ui, |ui| {
        let avail_w = crate::extras::layout_avail_w(ui).max(200.0);
        ui.set_width(avail_w);
        ui.set_max_width(avail_w);

        // 1) 顶部：状态分类标签（静息 6 / 处理中 19 / 情绪 16 / 提示 3）+ 恢复默认
        let gap_top = 4.0_f32;
        if avail_w >= 370.0 {
            let reset_w = (avail_w * 0.17).clamp(58.0, 84.0);
            let tab_btn_w = ((avail_w - reset_w - gap_top * 4.0) / 4.0).max(40.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(gap_top, 4.0);
                for (ci, cat) in StateCat::ALL.iter().enumerate() {
                    let count = STATES.iter().filter(|s| s.cat == *cat).count();
                    let label = format!("{} {}", cat.label_zh(), count);
                    let selected = selected_cat_idx == ci;
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(tab_btn_w, 28.0),
                        if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                    );
                    if resp.clicked() {
                        selected_cat_idx = ci;
                        if let Some(first) = STATES.iter().position(|s| s.cat == *cat) {
                            preview_state = first;
                            if fixed > 0 && shape_idx != 10 {
                                fixed = first as i64 + 1;
                                changed = true;
                            }
                        }
                    }
                    let fill = if selected {
                        egui::Color32::from_rgb(10, 132, 255)
                    } else if resp.hovered() {
                        card_hover
                    } else {
                        card_idle
                    };
                    ui.painter().rect_filled(rect, 7.0, fill);
                    let fsz = if tab_btn_w < 62.0 { 10.2 } else { 11.2 };
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(fsz),
                        if selected { egui::Color32::WHITE } else { text_idle },
                    );
                }

                let (reset_rect, reset_resp) = ui.allocate_exact_size(
                    egui::vec2(reset_w, 28.0),
                    if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                );
                if reset_resp.clicked() {
                    style_idx = 0;
                    shape_idx = 1; // 麻薯
                    color_idx = 0; // 奶油
                    face_idx = 0;  // 萌脸
                    scale = 100;
                    fixed = 0;
                    custom_fill_on = false;
                    custom_outline_on = false;
                    custom_ink_on = false;
                    selected_cat_idx = 0;
                    preview_state = 0;
                    changed = true;
                }
                ui.painter().rect_filled(
                    reset_rect,
                    7.0,
                    if reset_resp.hovered() { card_hover } else { card_idle },
                );
                ui.painter().text(
                    reset_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "恢复默认",
                    egui::FontId::proportional(if reset_w < 60.0 { 9.8 } else { 10.8 }),
                    text_idle,
                );
            });
        } else {
            let tab_btn_w = ((avail_w - gap_top * 3.0) / 4.0).max(40.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(gap_top, 4.0);
                for (ci, cat) in StateCat::ALL.iter().enumerate() {
                    let count = STATES.iter().filter(|s| s.cat == *cat).count();
                    let label = format!("{} {}", cat.label_zh(), count);
                    let selected = selected_cat_idx == ci;
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(tab_btn_w, 26.0),
                        if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                    );
                    if resp.clicked() {
                        selected_cat_idx = ci;
                        if let Some(first) = STATES.iter().position(|s| s.cat == *cat) {
                            preview_state = first;
                            if fixed > 0 && shape_idx != 10 {
                                fixed = first as i64 + 1;
                                changed = true;
                            }
                        }
                    }
                    let fill = if selected {
                        egui::Color32::from_rgb(10, 132, 255)
                    } else if resp.hovered() {
                        card_hover
                    } else {
                        card_idle
                    };
                    ui.painter().rect_filled(rect, 7.0, fill);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(9.8),
                        if selected { egui::Color32::WHITE } else { text_idle },
                    );
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (reset_rect, reset_resp) = ui.allocate_exact_size(
                        egui::vec2(72.0, 24.0),
                        if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                    );
                    if reset_resp.clicked() {
                        style_idx = 0;
                        shape_idx = 1;
                        color_idx = 0;
                        face_idx = 0;
                        scale = 100;
                        fixed = 0;
                        custom_fill_on = false;
                        custom_outline_on = false;
                        custom_ink_on = false;
                        selected_cat_idx = 0;
                        preview_state = 0;
                        changed = true;
                    }
                    ui.painter().rect_filled(
                        reset_rect,
                        6.0,
                        if reset_resp.hovered() { card_hover } else { card_idle },
                    );
                    ui.painter().text(
                        reset_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "恢复默认",
                        egui::FontId::proportional(10.2),
                        text_idle,
                    );
                });
            });
        }

        ui.data_mut(|d| {
            d.insert_temp(cat_id, selected_cat_idx);
            d.insert_temp(prev_id, preview_state);
        });

        ui.add_space(6.0);

        // 渲染预览舞台闭包（供宽窗三栏与窄窗双层复用）
        /// 预览舞台用的是"选择快照"：闭包不持有可变借用，
        /// 才能与调色板/五官网格在同一 UI 内并用（原代码三闭包互借冲突）。
        #[derive(Clone, Copy)]
        struct StageLook {
            color_idx: usize,
            style_idx: usize,
            shape_idx: usize,
            face_idx: usize,
            custom_fill_on: bool,
            custom_fill_rgb: [u8; 3],
        }

        let draw_stage_box = |ui: &mut egui::Ui, stage_w: f32, stage_h: f32, look: StageLook| {
            let preview_size = egui::vec2(stage_w, stage_h);
            let (rect, _) = ui.allocate_exact_size(preview_size, egui::Sense::hover());
            let body_color = if look.custom_fill_on {
                egui::Color32::from_rgba_unmultiplied(
                    look.custom_fill_rgb[0],
                    look.custom_fill_rgb[1],
                    look.custom_fill_rgb[2],
                    custom_fill_a,
                )
            } else {
                PALETTE[look.color_idx]
            };
            let outline_override = custom_outline_on.then(|| {
                egui::Color32::from_rgba_unmultiplied(
                    custom_outline_rgb[0],
                    custom_outline_rgb[1],
                    custom_outline_rgb[2],
                    custom_outline_a,
                )
            });
            let art_style = ArtStyle::ALL[look.style_idx];
            let body_shape = BodyShape::ALL[look.shape_idx];
            let face_style = FaceStyle::ALL[look.face_idx];

            ui.painter()
                .rect_filled(rect, 14.0, egui::Color32::from_rgb(32, 32, 32));
            ui.painter().rect_stroke(
                rect,
                14.0,
                egui::Stroke::new(1.0, egui::Color32::from_white_alpha(26)),
                egui::StrokeKind::Inside,
            );

            let card = Card::new(ui, rect.min, rect);
            let cx = preview_size.x * 0.5;
            let bob = (now * 2.1).sin() * 1.6;
            let cy = preview_size.y * 0.44 + bob;

            let halo_col = if body_shape == BodyShape::Islo && look.color_idx <= 1 {
                egui::Color32::from_rgba_unmultiplied(120, 175, 255, 38)
            } else {
                let [br, bg_c, bb, _] = body_color.to_srgba_unmultiplied();
                egui::Color32::from_rgba_unmultiplied(br, bg_c, bb, 40)
            };
            card.glow(cx, cy, (stage_w.min(stage_h) * 0.36).clamp(44.0, 92.0), halo_col);

            let k = (scale as f32 / 100.0).clamp(0.7, 1.4);
            let scale_fit = (stage_w.min(stage_h * 1.05) / 180.0).clamp(0.70, 1.45);
            if body_shape == BodyShape::Islo {
                let islo_f = if fixed > 0 && fixed <= 20 {
                    IsloFace::from_index(fixed as usize - 1)
                } else {
                    aora_face::aora_idx_to_islo_face(preview_state, "Awake")
                };
                let islo_ink = if look.color_idx <= 1 {
                    egui::Color32::WHITE
                } else {
                    body_color
                };
                islo_face::draw_custom(&card, cx, cy, 2.35 * k * scale_fit, islo_f, now, [0.0, 0.0], islo_ink);

                let title = format!("「{}」 · 初代 islo", islo_f.label());
                ui.painter().text(
                    egui::pos2(rect.center().x, rect.bottom() - 25.0),
                    egui::Align2::CENTER_CENTER,
                    title,
                    egui::FontId::proportional(11.5),
                    egui::Color32::WHITE,
                );
                let meta = format!(
                    "初代胶囊眼形象 · {} · 20 种经典表情",
                    if look.color_idx <= 1 { "经典纯白" } else { PALETTE_NAMES[look.color_idx] }
                );
                ui.painter().text(
                    egui::pos2(rect.center().x, rect.bottom() - 10.5),
                    egui::Align2::CENTER_CENTER,
                    meta,
                    egui::FontId::proportional(9.2),
                    egui::Color32::from_gray(155),
                );
            } else {
                let stage_state = if let Some(ai_st) = aora_face::active_ai_mascot_state(now as f64, &cfg)
                    .filter(|_| aora_face::ai_mascot_demo_active())
                {
                    ai_st
                } else {
                    preview_state
                };
                let radius = 42.0 * k * scale_fit;
                aora_face::draw_shape_dual(&card, cx, cy, radius, body_shape, body_color, outline_override, art_style);
                let ink = if custom_ink_on {
                    egui::Color32::from_rgba_unmultiplied(
                        custom_ink_rgb[0],
                        custom_ink_rgb[1],
                        custom_ink_rgb[2],
                        custom_ink_a,
                    )
                } else {
                    aora_face::resolve_mascot_ink(body_color, outline_override, art_style, &cfg)
                };
                let ([fx, fy], eye_fit) = if matches!(art_style, ArtStyle::KaomojiPlain | ArtStyle::KaomojiBubble) {
                    ([0.0, 0.0], 1.0)
                } else {
                    aora_face::shape_face_fit(body_shape)
                };
                aora_face::draw_face_in_style(
                    &card,
                    cx + fx * radius,
                    cy + fy * radius,
                    1.98 * k * scale_fit * eye_fit,
                    stage_state,
                    ink,
                    face_style,
                    art_style,
                    now,
                );

                let st = &STATES[stage_state];
                let title = format!("「{}」 · {}", st.label_zh, st.id);
                ui.painter().text(
                    egui::pos2(rect.center().x, rect.bottom() - 25.0),
                    egui::Align2::CENTER_CENTER,
                    title,
                    egui::FontId::proportional(11.5),
                    egui::Color32::WHITE,
                );
                let meta = format!(
                    "{} · {} · {} · {}",
                    art_style.label_zh(),
                    body_shape.label_zh(),
                    PALETTE_NAMES[look.color_idx],
                    face_style.label_zh()
                );
                ui.painter().text(
                    egui::pos2(rect.center().x, rect.bottom() - 10.5),
                    egui::Align2::CENTER_CENTER,
                    meta,
                    egui::FontId::proportional(9.2),
                    egui::Color32::from_gray(155),
                );
            }
        };

        // 渲染指定矩形区域内的 8 色调色板 + 16 种五官 4×4 网格（基于精确坐标布局，零累积误差，顶底与左列/舞台 100% 严丝合缝）
        let draw_palette_and_faces_in_rect = |ui: &mut egui::Ui,
                                              col_rect: egui::Rect,
                                              swatch_h: f32,
                                              cell_h: f32,
                                              color_idx: &mut usize,
                                              custom_fill_on: &mut bool,
                                              custom_fill_rgb: &mut [u8; 3],
                                              face_idx: &mut usize,
                                              shape_idx: &mut usize,
                                              changed: &mut bool| {
            let panel_w = col_rect.width();
            let top_y = col_rect.top();
            let lbl1_r = egui::Rect::from_min_size(egui::pos2(col_rect.left(), top_y), egui::vec2(panel_w, 16.0));
            ui.painter().text(
                egui::pos2(lbl1_r.left(), lbl1_r.center().y),
                egui::Align2::LEFT_CENTER,
                format!("颜色 · {}", PALETTE_NAMES[*color_idx]),
                egui::FontId::proportional(10.5),
                sub_text,
            );

            let swatch_gap = 4.0_f32;
            let swatch_w = ((panel_w - swatch_gap * 7.0) / 8.0).max(12.0);
            let swatch_y = top_y + 20.0;
            for (i, color) in PALETTE.iter().enumerate() {
                let selected = i == *color_idx;
                let sx = col_rect.left() + i as f32 * (swatch_w + swatch_gap);
                let rect = egui::Rect::from_min_size(egui::pos2(sx, swatch_y), egui::vec2(swatch_w, swatch_h));
                let resp = ui.interact(
                    rect,
                    ui.id().with(("aora_swatch", i)),
                    if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                );
                if resp.clicked() {
                    *color_idx = i;
                    *custom_fill_on = false;
                    *custom_fill_rgb = [color.r(), color.g(), color.b()];
                    *changed = true;
                }
                let draw_r = if selected { rect } else { rect.shrink(1.0) };
                ui.painter().rect_filled(draw_r, 5.0, *color);
                ui.painter().rect_stroke(
                    draw_r,
                    5.0,
                    egui::Stroke::new(
                        if selected { 2.0 } else { 1.0 },
                        if selected {
                            egui::Color32::from_rgb(10, 132, 255)
                        } else {
                            egui::Color32::from_white_alpha(55)
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
            }

            let lbl2_y = swatch_y + swatch_h + 6.0;
            let lbl2_r = egui::Rect::from_min_size(egui::pos2(col_rect.left(), lbl2_y), egui::vec2(panel_w, 16.0));
            ui.painter().text(
                egui::pos2(lbl2_r.left(), lbl2_r.center().y),
                egui::Align2::LEFT_CENTER,
                format!("脸型 · {} ({})", FaceStyle::ALL[*face_idx].label_zh(), FaceStyle::ALL[*face_idx].id()),
                egui::FontId::proportional(10.5),
                sub_text,
            );

            let cell_gap = 4.0_f32;
            let cell_w = ((panel_w - cell_gap * 3.0) / 4.0).max(24.0);
            let grid_top = lbl2_y + 20.0;
            for row in 0..4 {
                let ry = grid_top + row as f32 * (cell_h + cell_gap);
                let row_bottom = if row == 3 {
                    col_rect.bottom()
                } else {
                    ry + cell_h
                };
                let actual_h = (row_bottom - ry).max(20.0);
                for col in 0..4 {
                    let idx = row * 4 + col;
                    let fstyle = FaceStyle::ALL[idx];
                    let selected = idx == *face_idx && *shape_idx != 10;
                    let cx = col_rect.left() + col as f32 * (cell_w + cell_gap);
                    let rect = egui::Rect::from_min_size(egui::pos2(cx, ry), egui::vec2(cell_w, actual_h));
                    let resp = ui.interact(
                        rect,
                        ui.id().with(("aora_face_cell", idx)),
                        if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                    );
                    if resp.clicked() {
                        *face_idx = idx;
                        if *shape_idx == 10 {
                            *shape_idx = 1;
                        }
                        *changed = true;
                    }
                    let fill = if selected {
                        egui::Color32::from_rgb(10, 132, 255)
                    } else if resp.hovered() {
                        card_hover
                    } else {
                        card_idle
                    };
                    ui.painter().rect_filled(rect, 6.0, fill);
                    let fcard = Card::new(ui, rect.min, rect);
                    let face_ink = if selected || dark {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::from_rgb(51, 51, 51)
                    };
                    aora_face::draw_face_styled(
                        &fcard,
                        cell_w * 0.5,
                        actual_h * 0.38,
                        (actual_h / 54.0).clamp(0.52, 0.76),
                        0,
                        face_ink,
                        fstyle,
                        0.0,
                    );
                    ui.painter().text(
                        egui::pos2(rect.center().x, rect.bottom() - 6.5),
                        egui::Align2::CENTER_CENTER,
                        fstyle.label_zh(),
                        egui::FontId::proportional(8.8),
                        if selected { egui::Color32::WHITE } else { text_idle },
                    );
                }
            }
        };

        // 2) 主控台自适应布局：宽窗口（>= 460px）三栏顶底像素级锁死平齐；小窗口（< 460px）双层布局
        let style_cols = 3usize;
        let style_rows = (ArtStyle::ALL.len() + style_cols - 1) / style_cols;
        if avail_w >= 460.0 {
            let col_gap = 8.0_f32;
            let left_w = (avail_w * 0.25).clamp(124.0, 176.0);
            let right_w = (avail_w * 0.42).clamp(160.0, 340.0);
            let stage_w = (avail_w - left_w - right_w - col_gap * 2.0).max(116.0);
            let cell_w = ((right_w - 12.0) / 4.0).clamp(32.0, 88.0);
            let cell_h = (cell_w * 0.68).clamp(30.0, 40.0).round();
            let swatch_w = ((right_w - 28.0) / 8.0).clamp(14.0, 42.0);
            let swatch_h = swatch_w.clamp(18.0, 22.0).round();
            // 主控台内容区高度（不含顶部 20px 标题带）：swatch_h + 6 + lbl2(16) + 4 + 4*cell_h + 3*4
            let body_h = swatch_h + 6.0 + 16.0 + 4.0 + cell_h * 4.0 + 12.0;
            let total_h = 20.0 + body_h;
            let style_gap = 3.5_f32;
            let style_btn_h = (body_h - ((style_rows.saturating_sub(1)) as f32) * style_gap) / style_rows as f32;
            let style_btn_w = (left_w - ((style_cols - 1) as f32) * style_gap) / style_cols as f32;

            let (console_rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, total_h), egui::Sense::hover());
            let left_x = console_rect.left();
            let stage_x = left_x + left_w + col_gap;
            let right_x = stage_x + stage_w + col_gap;
            let body_top = console_rect.top() + 20.0;

            // 左栏：顶部「画风」标题 + 21 种 Aora Bot 画风 3×7 网格（顶与舞台/色板平齐，底与舞台/脸型平齐）
            let lbl_l = egui::Rect::from_min_size(egui::pos2(left_x, console_rect.top()), egui::vec2(left_w, 16.0));
            ui.painter().text(
                egui::pos2(lbl_l.left(), lbl_l.center().y),
                egui::Align2::LEFT_CENTER,
                format!("画风 · {}", ArtStyle::ALL.len()),
                egui::FontId::proportional(10.5),
                sub_text,
            );
            for (i, style) in ArtStyle::ALL.iter().enumerate() {
                let row = i / style_cols;
                let col = i % style_cols;
                let selected = i == style_idx && shape_idx != 10;
                let bx = left_x + col as f32 * (style_btn_w + style_gap);
                let by = body_top + row as f32 * (style_btn_h + style_gap);
                let b_bottom = if row + 1 == style_rows {
                    console_rect.bottom()
                } else {
                    by + style_btn_h
                };
                let rect = egui::Rect::from_min_max(
                    egui::pos2(bx, by),
                    egui::pos2(bx + style_btn_w, b_bottom),
                );
                let resp = ui.interact(
                    rect,
                    ui.id().with(("aora_style_btn", i)),
                    if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                );
                if resp.clicked() {
                    style_idx = i;
                    if shape_idx == 10 {
                        shape_idx = 1;
                    }
                    changed = true;
                }
                let fill = if selected {
                    egui::Color32::from_rgb(10, 132, 255)
                } else if resp.hovered() {
                    card_hover
                } else {
                    card_idle
                };
                ui.painter().rect_filled(rect, 5.0, fill);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    style.label_zh(),
                    egui::FontId::proportional(if style_btn_w < 42.0 { 9.2 } else { 10.0 }),
                    if selected { egui::Color32::WHITE } else { text_idle },
                );
            }

            // 中栏：顶部「预览」标题 + 舞台框（顶对齐 y=body_top，底对齐 y=console_rect.bottom()）
            let lbl_m = egui::Rect::from_min_size(egui::pos2(stage_x, console_rect.top()), egui::vec2(stage_w, 16.0));
            ui.painter().text(
                egui::pos2(lbl_m.left(), lbl_m.center().y),
                egui::Align2::LEFT_CENTER,
                "预览 · 实时互动",
                egui::FontId::proportional(10.5),
                sub_text,
            );
            let stage_rect = egui::Rect::from_min_size(egui::pos2(stage_x, body_top), egui::vec2(stage_w, body_h));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(stage_rect), |ui| {
                draw_stage_box(
                    ui,
                    stage_w,
                    body_h,
                    StageLook {
                        color_idx,
                        style_idx,
                        shape_idx,
                        face_idx,
                        custom_fill_on,
                        custom_fill_rgb,
                    },
                );
            });

            // 右栏：顶部「颜色」标题 + 8 色块 + 「脸型」标题 + 4×4 网格
            let right_rect = egui::Rect::from_min_size(egui::pos2(right_x, console_rect.top()), egui::vec2(right_w, total_h));
            draw_palette_and_faces_in_rect(
                ui,
                right_rect,
                swatch_h,
                cell_h,
                &mut color_idx,
                &mut custom_fill_on,
                &mut custom_fill_rgb,
                &mut face_idx,
                &mut shape_idx,
                &mut changed,
            );
        } else {
            // 小窗口双层紧凑主控台：第一层「左画风 3×7 + 右舞台（精确等高）」，第二层「8 色调色板 + 16 种五官 4×4 网格」铺满宽度
            let col_gap = 8.0_f32;
            let left_w = 128.0_f32;
            let stage_w = (avail_w - left_w - col_gap).max(120.0);
            let style_gap = 3.5_f32;
            let body_h = 176.0_f32;
            let total_h = 20.0 + body_h;
            let style_btn_h = (body_h - ((style_rows.saturating_sub(1)) as f32) * style_gap) / style_rows as f32;
            let style_btn_w = (left_w - ((style_cols - 1) as f32) * style_gap) / style_cols as f32;

            let (top_rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, total_h), egui::Sense::hover());
            let left_x = top_rect.left();
            let stage_x = left_x + left_w + col_gap;
            let body_top = top_rect.top() + 20.0;

            let lbl_l = egui::Rect::from_min_size(egui::pos2(left_x, top_rect.top()), egui::vec2(left_w, 16.0));
            ui.painter().text(
                egui::pos2(lbl_l.left(), lbl_l.center().y),
                egui::Align2::LEFT_CENTER,
                format!("画风 · {}", ArtStyle::ALL.len()),
                egui::FontId::proportional(10.5),
                sub_text,
            );
            for (i, style) in ArtStyle::ALL.iter().enumerate() {
                let row = i / style_cols;
                let col = i % style_cols;
                let selected = i == style_idx && shape_idx != 10;
                let bx = left_x + col as f32 * (style_btn_w + style_gap);
                let by = body_top + row as f32 * (style_btn_h + style_gap);
                let b_bottom = if row + 1 == style_rows { top_rect.bottom() } else { by + style_btn_h };
                let rect = egui::Rect::from_min_max(egui::pos2(bx, by), egui::pos2(bx + style_btn_w, b_bottom));
                let resp = ui.interact(
                    rect,
                    ui.id().with(("aora_style_btn_sm", i)),
                    if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                );
                if resp.clicked() {
                    style_idx = i;
                    if shape_idx == 10 {
                        shape_idx = 1;
                    }
                    changed = true;
                }
                let fill = if selected {
                    egui::Color32::from_rgb(10, 132, 255)
                } else if resp.hovered() {
                    card_hover
                } else {
                    card_idle
                };
                ui.painter().rect_filled(rect, 5.0, fill);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    style.label_zh(),
                    egui::FontId::proportional(9.2),
                    if selected { egui::Color32::WHITE } else { text_idle },
                );
            }

            let lbl_m = egui::Rect::from_min_size(egui::pos2(stage_x, top_rect.top()), egui::vec2(stage_w, 16.0));
            ui.painter().text(
                egui::pos2(lbl_m.left(), lbl_m.center().y),
                egui::Align2::LEFT_CENTER,
                "预览 · 实时互动",
                egui::FontId::proportional(10.5),
                sub_text,
            );
            let stage_rect = egui::Rect::from_min_size(egui::pos2(stage_x, body_top), egui::vec2(stage_w, body_h));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(stage_rect), |ui| {
                draw_stage_box(
                    ui,
                    stage_w,
                    body_h,
                    StageLook {
                        color_idx,
                        style_idx,
                        shape_idx,
                        face_idx,
                        custom_fill_on,
                        custom_fill_rgb,
                    },
                );
            });

            ui.add_space(6.0);
            let swatch_h = 22.0_f32;
            let cell_h = 34.0_f32;
            let bot_h = 20.0 + swatch_h + 6.0 + 16.0 + 4.0 + cell_h * 4.0 + 12.0;
            let (bot_rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, bot_h), egui::Sense::hover());
            draw_palette_and_faces_in_rect(
                ui,
                bot_rect,
                swatch_h,
                cell_h,
                &mut color_idx,
                &mut custom_fill_on,
                &mut custom_fill_rgb,
                &mut face_idx,
                &mut shape_idx,
                &mut changed,
            );
        }

        ui.add_space(8.0);
        crate::extras::twinkle_card_break(ui);
        ui.add_space(4.0);

        // 3) 吉祥物三区独立配色定制（窄窗双层子卡片 / 宽窗三列清晰不截断，小窗口文字永不消失）
        let any_custom = custom_fill_on || custom_outline_on || custom_ink_on;
        let reset_custom_colors = |custom_fill_on: &mut bool,
                                   custom_outline_on: &mut bool,
                                   custom_ink_on: &mut bool,
                                   custom_fill_rgb: &mut [u8; 3],
                                   custom_fill_a: &mut u8,
                                   custom_outline_rgb: &mut [u8; 3],
                                   custom_outline_a: &mut u8,
                                   custom_ink_rgb: &mut [u8; 3],
                                   custom_ink_a: &mut u8,
                                   changed: &mut bool| {
            *custom_fill_on = false;
            *custom_outline_on = false;
            *custom_ink_on = false;
            let p = PALETTE[color_idx];
            *custom_fill_rgb = [p.r(), p.g(), p.b()];
            *custom_fill_a = 255;
            *custom_outline_rgb = [56, 189, 248];
            *custom_outline_a = 255;
            *custom_ink_rgb = [248, 250, 252];
            *custom_ink_a = 255;
            *changed = true;
        };
        let skin = crate::extras::current_settings_skin_theme(ui.ctx());
        let accent = crate::auralsle_ai_status::plugins::active_accent_color();
        let title_col = crate::settings_theme::color(
            ui.ctx(),
            egui::Color32::WHITE,
            egui::Color32::from_rgb(30, 34, 42),
        );
        ui.horizontal(|ui| {
            ui.set_max_width(avail_w);
            ui.label(
                egui::RichText::new("吉祥物配色定制")
                    .size(11.8)
                    .strong()
                    .color(title_col),
            );
            if avail_w >= 540.0 {
                ui.label(
                    egui::RichText::new("左侧选择区块，右侧公用 RGBA 调色盘实时调节色彩与透明度")
                        .size(10.2)
                        .color(sub_text),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if any_custom {
                    let rst_btn = egui::Button::new(
                        egui::RichText::new("恢复默认配色").size(10.6).color(title_col),
                    )
                    .fill(skin.bg_ctrl)
                    .stroke(egui::Stroke::new(1.0, skin.border))
                    .corner_radius(6.0);
                    if ui.add(rst_btn).clicked() {
                        reset_custom_colors(
                            &mut custom_fill_on,
                            &mut custom_outline_on,
                            &mut custom_ink_on,
                            &mut custom_fill_rgb,
                            &mut custom_fill_a,
                            &mut custom_outline_rgb,
                            &mut custom_outline_a,
                            &mut custom_ink_rgb,
                            &mut custom_ink_a,
                            &mut changed,
                        );
                    }
                }
            });
        });
        if avail_w < 540.0 {
            ui.label(
                egui::RichText::new("左侧选择区块，右侧公用 RGBA 调色盘实时调节色彩与透明度")
                    .size(10.0)
                    .color(sub_text),
            );
        }
        ui.add_space(6.0);

        let parse_hex_8_or_6 = |hex_in: &str, rgb: &mut [u8; 3], alpha: &mut u8| -> bool {
            let h = hex_in.trim().trim_start_matches('#');
            if h.len() == 8 {
                if let (Ok(r), Ok(g), Ok(b), Ok(a)) = (
                    u8::from_str_radix(&h[0..2], 16),
                    u8::from_str_radix(&h[2..4], 16),
                    u8::from_str_radix(&h[4..6], 16),
                    u8::from_str_radix(&h[6..8], 16),
                ) {
                    *rgb = [r, g, b];
                    *alpha = a;
                    return true;
                }
            } else if h.len() == 6 {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    u8::from_str_radix(&h[0..2], 16),
                    u8::from_str_radix(&h[2..4], 16),
                    u8::from_str_radix(&h[4..6], 16),
                ) {
                    *rgb = [r, g, b];
                    return true;
                }
            }
            false
        };
        let rgb_to_hsv = |rgb: [u8; 3]| -> (f32, f32, f32) {
            let r = rgb[0] as f32 / 255.0;
            let g = rgb[1] as f32 / 255.0;
            let b = rgb[2] as f32 / 255.0;
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            let d = max - min;
            let v = max;
            let s = if max > 1e-5 { d / max } else { 0.0 };
            let h = if d < 1e-5 {
                0.0
            } else if (max - r).abs() < 1e-5 {
                ((g - b) / d).rem_euclid(6.0) / 6.0
            } else if (max - g).abs() < 1e-5 {
                ((b - r) / d + 2.0) / 6.0
            } else {
                ((r - g) / d + 4.0) / 6.0
            };
            (h.clamp(0.0, 1.0), s.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
        };
        let hsv_to_rgb = |h: f32, s: f32, v: f32| -> [u8; 3] {
            let h = h.rem_euclid(1.0) * 6.0;
            let s = s.clamp(0.0, 1.0);
            let v = v.clamp(0.0, 1.0);
            let i = h.floor() as i32;
            let f = h - i as f32;
            let p = v * (1.0 - s);
            let q = v * (1.0 - s * f);
            let t = v * (1.0 - s * (1.0 - f));
            let (r, g, b) = match i.rem_euclid(6) {
                0 => (v, t, p),
                1 => (q, v, p),
                2 => (p, v, t),
                3 => (p, q, v),
                4 => (t, p, v),
                _ => (v, p, q),
            };
            [
                (r * 255.0).round().clamp(0.0, 255.0) as u8,
                (g * 255.0).round().clamp(0.0, 255.0) as u8,
                (b * 255.0).round().clamp(0.0, 255.0) as u8,
            ]
        };
        let draw_checker = |painter: &egui::Painter, rect: egui::Rect, radius: f32| {
            let clip = painter.with_clip_rect(rect);
            let c1 = if dark {
                egui::Color32::from_gray(46)
            } else {
                egui::Color32::from_gray(208)
            };
            let c2 = if dark {
                egui::Color32::from_gray(68)
            } else {
                egui::Color32::from_gray(236)
            };
            clip.rect_filled(rect, radius, c1);
            let step = 4.5_f32;
            let cols = (rect.width() / step).ceil() as usize;
            let rows = (rect.height() / step).ceil() as usize;
            for r in 0..rows {
                for c in 0..cols {
                    if (r + c) % 2 == 1 {
                        let cell = egui::Rect::from_min_size(
                            egui::pos2(rect.left() + c as f32 * step, rect.top() + r as f32 * step),
                            egui::vec2(step, step),
                        )
                        .intersect(rect);
                        if cell.width() > 0.0 && cell.height() > 0.0 {
                            clip.rect_filled(cell, 0.0, c2);
                        }
                    }
                }
            }
        };

        let active_ch_id = egui::Id::new("aora_color_active_channel");
        let mut active_ch = ui
            .ctx()
            .data(|d| d.get_temp::<usize>(active_ch_id))
            .unwrap_or(0)
            .min(2);

        let side_by_side = avail_w >= 460.0;
        let col_gap = 10.0_f32;
        let left_w = if side_by_side {
            (avail_w * 0.40).clamp(178.0, 252.0).floor()
        } else {
            avail_w.max(180.0)
        };
        let right_w = if side_by_side {
            (avail_w - left_w - col_gap).max(210.0)
        } else {
            avail_w.max(180.0)
        };
        let row_h = 48.0_f32;
        let row_gap = 6.0_f32;
        let total_left_h = row_h * 3.0 + row_gap * 2.0;

        let draw_channel_row = |ui: &mut egui::Ui,
                                ch_idx: usize,
                                label: &str,
                                active_ch: &mut usize,
                                on_flag: &mut bool,
                                rgb_val: &[u8; 3],
                                alpha_val: u8,
                                changed: &mut bool| {
            let selected = *active_ch == ch_idx;
            let (row_rect, row_resp) = ui.allocate_exact_size(
                egui::vec2(left_w, row_h),
                egui::Sense::click(),
            );
            if row_resp.clicked() {
                *active_ch = ch_idx;
            }
            let bg = if selected {
                crate::paint::mix(skin.bg_ctrl, accent, if dark { 0.16 } else { 0.10 })
            } else if row_resp.hovered() {
                crate::paint::mix(skin.bg_ctrl, title_col, 0.05)
            } else {
                skin.bg_ctrl
            };
            let stroke_col = if selected {
                accent
            } else if *on_flag {
                crate::paint::mix(skin.border, accent, 0.45)
            } else {
                skin.border
            };
            ui.painter().rect(
                row_rect,
                7.0,
                bg,
                egui::Stroke::new(if selected { 1.25 } else { 1.0 }, stroke_col),
                egui::StrokeKind::Inside,
            );
            if selected {
                let bar = egui::Rect::from_min_size(
                    egui::pos2(row_rect.left() + 1.5, row_rect.top() + 9.0),
                    egui::vec2(3.0, row_h - 18.0),
                );
                ui.painter().rect_filled(bar, 1.5, accent);
            }

            let inner = row_rect.shrink2(egui::vec2(9.0, 5.0));
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(inner)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.set_width(inner.width());
                    ui.spacing_mut().item_spacing.x = 7.0;
                    if ui.checkbox(on_flag, "").changed() {
                        *active_ch = ch_idx;
                        *changed = true;
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.5;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.label(
                                egui::RichText::new(label)
                                    .size(11.4)
                                    .strong()
                                    .color(if selected { accent } else { title_col }),
                            );
                            if *on_flag {
                                ui.label(
                                    egui::RichText::new("自定义")
                                        .size(9.2)
                                        .color(accent),
                                );
                            }
                        });
                        let alpha_pct = ((alpha_val as f32 / 255.0) * 100.0).round() as u8;
                        let meta_txt = if alpha_val < 255 {
                            format!(
                                "#{:02X}{:02X}{:02X}{:02X} · {}%",
                                rgb_val[0], rgb_val[1], rgb_val[2], alpha_val, alpha_pct
                            )
                        } else {
                            format!(
                                "#{:02X}{:02X}{:02X} · {}%",
                                rgb_val[0], rgb_val[1], rgb_val[2], alpha_pct
                            )
                        };
                        ui.label(
                            egui::RichText::new(meta_txt)
                                .size(9.8)
                                .monospace()
                                .color(sub_text),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (sw_rect, sw_resp) = ui.allocate_exact_size(
                            egui::vec2(24.0, 24.0),
                            egui::Sense::click(),
                        );
                        if sw_resp.clicked() {
                            *active_ch = ch_idx;
                        }
                        draw_checker(ui.painter(), sw_rect, 5.0);
                        let swatch = egui::Color32::from_rgba_unmultiplied(
                            rgb_val[0], rgb_val[1], rgb_val[2], alpha_val,
                        );
                        ui.painter().rect(
                            sw_rect,
                            5.0,
                            swatch,
                            egui::Stroke::new(1.0, skin.border),
                            egui::StrokeKind::Inside,
                        );
                    });
                },
            );
        };

        let draw_shared_palette = |ui: &mut egui::Ui,
                                   ch_idx: usize,
                                   label: &str,
                                   on_flag: &mut bool,
                                   rgb_val: &mut [u8; 3],
                                   alpha_val: &mut u8,
                                   panel_w: f32,
                                   panel_h: f32,
                                   changed: &mut bool| {
            let (panel_rect, _) = ui.allocate_exact_size(
                egui::vec2(panel_w, panel_h),
                egui::Sense::hover(),
            );
            ui.painter().rect(
                panel_rect,
                7.0,
                skin.bg_ctrl,
                egui::Stroke::new(1.0, skin.border),
                egui::StrokeKind::Inside,
            );
            let inner = panel_rect.shrink2(egui::vec2(10.0, 8.0));
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(inner)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    let iw = inner.width().max(140.0);
                    ui.set_width(iw);
                    ui.set_max_width(iw);
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);

                    // 顶部：当前编辑分区标识 + 快速预设 + 预览色块
                    ui.horizontal(|ui| {
                        ui.set_width(iw);
                        ui.label(
                            egui::RichText::new("公用 RGBA 调色盘")
                                .size(11.0)
                                .strong()
                                .color(title_col),
                        );
                        let badge_bg = crate::paint::mix(skin.bg_card, accent, 0.20);
                        egui::Frame::new()
                            .fill(badge_bg)
                            .stroke(egui::Stroke::new(0.8, crate::paint::mix(skin.border, accent, 0.55)))
                            .corner_radius(4.0)
                            .inner_margin(egui::Margin::symmetric(5, 1))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(label)
                                        .size(9.8)
                                        .strong()
                                        .color(accent),
                                );
                            });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let (pv_rect, _) = ui.allocate_exact_size(
                                egui::vec2(26.0, 16.0),
                                egui::Sense::hover(),
                            );
                            draw_checker(ui.painter(), pv_rect, 4.0);
                            let pv_col = egui::Color32::from_rgba_unmultiplied(
                                rgb_val[0], rgb_val[1], rgb_val[2], *alpha_val,
                            );
                            ui.painter().rect(
                                pv_rect,
                                4.0,
                                pv_col,
                                egui::Stroke::new(1.0, skin.border),
                                egui::StrokeKind::Inside,
                            );

                            if iw >= 250.0 {
                                const QUICK: [[u8; 3]; 6] = [
                                    [241, 226, 193],
                                    [40, 42, 50],
                                    [56, 189, 248],
                                    [244, 63, 94],
                                    [52, 211, 153],
                                    [255, 255, 255],
                                ];
                                for q in QUICK.iter().rev() {
                                    let (qr, qresp) = ui.allocate_exact_size(
                                        egui::vec2(13.0, 13.0),
                                        egui::Sense::click(),
                                    );
                                    let qc = egui::Color32::from_rgb(q[0], q[1], q[2]);
                                    ui.painter().rect(
                                        qr,
                                        3.5,
                                        qc,
                                        egui::Stroke::new(
                                            if *rgb_val == *q { 1.4 } else { 0.8 },
                                            if *rgb_val == *q { accent } else { skin.border },
                                        ),
                                        egui::StrokeKind::Inside,
                                    );
                                    if qresp.clicked() {
                                        *rgb_val = *q;
                                        let (nh, _, _) = rgb_to_hsv(*rgb_val);
                                        ui.ctx().data_mut(|d| {
                                            d.insert_temp(egui::Id::new(("aora_color_hue", ch_idx)), nh);
                                        });
                                        *on_flag = true;
                                        *changed = true;
                                    }
                                }
                            }
                        });
                    });

                    // 计算并保持 Hue 记忆（黑白/灰阶拖动时不丢失色相角）
                    let hue_id = egui::Id::new(("aora_color_hue", ch_idx));
                    let (cur_h, cur_s, cur_v) = rgb_to_hsv(*rgb_val);
                    let stored_h = ui.ctx().data(|d| d.get_temp::<f32>(hue_id));
                    let mut hue = if cur_s > 0.02 {
                        cur_h
                    } else {
                        stored_h.unwrap_or(cur_h)
                    };
                    let mut sat = cur_s;
                    let mut val = cur_v;

                    // 中段：2D SV 调色画布 + 色相条 + 透明度条
                    let mid_h = (inner.height() - 18.0 - 24.0 - 12.0).max(66.0);
                    let bar_w = 16.0_f32;
                    let sv_w = (iw - bar_w * 2.0 - 12.0).max(80.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;

                        // 1) 2D Saturation × Value 画布
                        let (sv_rect, sv_resp) = ui.allocate_exact_size(
                            egui::vec2(sv_w, mid_h),
                            egui::Sense::click_and_drag(),
                        );
                        if sv_resp.dragged() || sv_resp.clicked() {
                            if let Some(pos) = sv_resp.interact_pointer_pos() {
                                sat = ((pos.x - sv_rect.left()) / sv_rect.width()).clamp(0.0, 1.0);
                                val = (1.0 - (pos.y - sv_rect.top()) / sv_rect.height()).clamp(0.0, 1.0);
                                *rgb_val = hsv_to_rgb(hue, sat, val);
                                *on_flag = true;
                                *changed = true;
                            }
                        }
                        let pure_h = hsv_to_rgb(hue, 1.0, 1.0);
                        let hue_c = egui::Color32::from_rgb(pure_h[0], pure_h[1], pure_h[2]);
                        // 双层叠加渲染精确双线性 SV：先横铺 White -> PureHue，再竖叠 Transparent -> Black，消除单四边形对角插值折痕
                        let mut h_sat_mesh = egui::Mesh::default();
                        h_sat_mesh.colored_vertex(sv_rect.left_top(), egui::Color32::WHITE);
                        h_sat_mesh.colored_vertex(sv_rect.right_top(), hue_c);
                        h_sat_mesh.colored_vertex(sv_rect.right_bottom(), hue_c);
                        h_sat_mesh.colored_vertex(sv_rect.left_bottom(), egui::Color32::WHITE);
                        h_sat_mesh.add_triangle(0, 1, 2);
                        h_sat_mesh.add_triangle(0, 2, 3);
                        ui.painter().add(egui::Shape::mesh(h_sat_mesh));

                        let mut v_shade_mesh = egui::Mesh::default();
                        let clear_black = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 0);
                        v_shade_mesh.colored_vertex(sv_rect.left_top(), clear_black);
                        v_shade_mesh.colored_vertex(sv_rect.right_top(), clear_black);
                        v_shade_mesh.colored_vertex(sv_rect.right_bottom(), egui::Color32::BLACK);
                        v_shade_mesh.colored_vertex(sv_rect.left_bottom(), egui::Color32::BLACK);
                        v_shade_mesh.add_triangle(0, 1, 2);
                        v_shade_mesh.add_triangle(0, 2, 3);
                        ui.painter().add(egui::Shape::mesh(v_shade_mesh));
                        ui.painter().rect_stroke(
                            sv_rect,
                            4.0,
                            egui::Stroke::new(1.0, skin.border),
                            egui::StrokeKind::Inside,
                        );
                        let sv_pt = egui::pos2(
                            sv_rect.left() + sat * sv_rect.width(),
                            sv_rect.top() + (1.0 - val) * sv_rect.height(),
                        );
                        ui.painter().circle_stroke(
                            sv_pt,
                            5.5,
                            egui::Stroke::new(2.0, egui::Color32::BLACK),
                        );
                        ui.painter().circle_stroke(
                            sv_pt,
                            4.2,
                            egui::Stroke::new(1.6, egui::Color32::WHITE),
                        );

                        // 2) Hue 色相竖条
                        let (hue_rect, hue_resp) = ui.allocate_exact_size(
                            egui::vec2(bar_w, mid_h),
                            egui::Sense::click_and_drag(),
                        );
                        if hue_resp.dragged() || hue_resp.clicked() {
                            if let Some(pos) = hue_resp.interact_pointer_pos() {
                                hue = ((pos.y - hue_rect.top()) / hue_rect.height()).clamp(0.0, 0.9999);
                                *rgb_val = hsv_to_rgb(hue, sat, val);
                                *on_flag = true;
                                *changed = true;
                            }
                        }
                        const HUE_STOPS: [(u8, u8, u8); 7] = [
                            (255, 0, 0),
                            (255, 255, 0),
                            (0, 255, 0),
                            (0, 255, 255),
                            (0, 0, 255),
                            (255, 0, 255),
                            (255, 0, 0),
                        ];
                        let mut hmesh = egui::Mesh::default();
                        for (idx, &(r, g, b)) in HUE_STOPS.iter().enumerate() {
                            let y = hue_rect.top() + hue_rect.height() * (idx as f32 / 6.0);
                            let col = egui::Color32::from_rgb(r, g, b);
                            hmesh.colored_vertex(egui::pos2(hue_rect.left(), y), col);
                            hmesh.colored_vertex(egui::pos2(hue_rect.right(), y), col);
                            if idx > 0 {
                                let base = ((idx - 1) * 2) as u32;
                                hmesh.add_triangle(base, base + 1, base + 3);
                                hmesh.add_triangle(base, base + 3, base + 2);
                            }
                        }
                        ui.painter().add(egui::Shape::mesh(hmesh));
                        ui.painter().rect_stroke(
                            hue_rect,
                            4.0,
                            egui::Stroke::new(1.0, skin.border),
                            egui::StrokeKind::Inside,
                        );
                        let hy = hue_rect.top() + hue * hue_rect.height();
                        ui.painter().rect(
                            egui::Rect::from_center_size(
                                egui::pos2(hue_rect.center().x, hy),
                                egui::vec2(bar_w + 2.0, 5.0),
                            ),
                            2.5,
                            egui::Color32::WHITE,
                            egui::Stroke::new(1.0, egui::Color32::BLACK),
                            egui::StrokeKind::Inside,
                        );

                        // 3) Alpha 透明度竖条（上方 100% 不透明 -> 下方 0% 透明）
                        let (a_rect, a_resp) = ui.allocate_exact_size(
                            egui::vec2(bar_w, mid_h),
                            egui::Sense::click_and_drag(),
                        );
                        if a_resp.dragged() || a_resp.clicked() {
                            if let Some(pos) = a_resp.interact_pointer_pos() {
                                let t = (1.0 - (pos.y - a_rect.top()) / a_rect.height()).clamp(0.0, 1.0);
                                *alpha_val = (t * 255.0).round() as u8;
                                *on_flag = true;
                                *changed = true;
                            }
                        }
                        draw_checker(ui.painter(), a_rect, 4.0);
                        let top_c = egui::Color32::from_rgba_unmultiplied(rgb_val[0], rgb_val[1], rgb_val[2], 255);
                        let bot_c = egui::Color32::from_rgba_unmultiplied(rgb_val[0], rgb_val[1], rgb_val[2], 0);
                        let mut amesh = egui::Mesh::default();
                        amesh.colored_vertex(a_rect.left_top(), top_c);
                        amesh.colored_vertex(a_rect.right_top(), top_c);
                        amesh.colored_vertex(a_rect.right_bottom(), bot_c);
                        amesh.colored_vertex(a_rect.left_bottom(), bot_c);
                        amesh.add_triangle(0, 1, 2);
                        amesh.add_triangle(0, 2, 3);
                        ui.painter().add(egui::Shape::mesh(amesh));
                        ui.painter().rect_stroke(
                            a_rect,
                            4.0,
                            egui::Stroke::new(1.0, skin.border),
                            egui::StrokeKind::Inside,
                        );
                        let af = *alpha_val as f32 / 255.0;
                        let ay = a_rect.top() + (1.0 - af) * a_rect.height();
                        ui.painter().rect(
                            egui::Rect::from_center_size(
                                egui::pos2(a_rect.center().x, ay),
                                egui::vec2(bar_w + 2.0, 5.0),
                            ),
                            2.5,
                            egui::Color32::WHITE,
                            egui::Stroke::new(1.0, egui::Color32::BLACK),
                            egui::StrokeKind::Inside,
                        );
                    });

                    ui.ctx().data_mut(|d| {
                        d.insert_temp(hue_id, hue);
                    });

                    // 底栏：#RRGGBB(AA) 输入 + 透明度百分比 + 同步 RGBA 值展示
                    ui.horizontal(|ui| {
                        ui.set_width(iw);
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let mut hex_str = if *alpha_val < 255 {
                            format!("#{:02X}{:02X}{:02X}{:02X}", rgb_val[0], rgb_val[1], rgb_val[2], *alpha_val)
                        } else {
                            format!("#{:02X}{:02X}{:02X}", rgb_val[0], rgb_val[1], rgb_val[2])
                        };
                        let resp = ui.add_sized(
                            [82.0, 22.0],
                            egui::TextEdit::singleline(&mut hex_str)
                                .desired_width(82.0)
                                .clip_text(true)
                                .font(egui::FontId::monospace(10.8))
                                .horizontal_align(egui::Align::Center)
                                .vertical_align(egui::Align::Center),
                        );
                        if resp.changed() && parse_hex_8_or_6(&hex_str, rgb_val, alpha_val) {
                            let (nh, _, _) = rgb_to_hsv(*rgb_val);
                            ui.ctx().data_mut(|d| d.insert_temp(hue_id, nh));
                            *on_flag = true;
                            *changed = true;
                        }
                        let mut alpha_pct = ((*alpha_val as f32 / 255.0) * 100.0).round() as u8;
                        if ui
                            .add_sized(
                                [54.0, 22.0],
                                egui::DragValue::new(&mut alpha_pct)
                                    .range(0..=100)
                                    .prefix("A:")
                                    .suffix("%"),
                            )
                            .changed()
                        {
                            *alpha_val = ((alpha_pct as f32 / 100.0) * 255.0).round() as u8;
                            *on_flag = true;
                            *changed = true;
                        }
                        if iw >= 230.0 {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "RGBA({}, {}, {}, {})",
                                        rgb_val[0], rgb_val[1], rgb_val[2], *alpha_val
                                    ))
                                    .size(9.8)
                                    .monospace()
                                    .color(sub_text),
                                );
                            });
                        }
                    });
                },
            );
        };

        if side_by_side {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = col_gap;
                ui.vertical(|ui| {
                    ui.set_width(left_w);
                    ui.spacing_mut().item_spacing.y = row_gap;
                    draw_channel_row(ui, 0, "内部填充", &mut active_ch, &mut custom_fill_on, &custom_fill_rgb, custom_fill_a, &mut changed);
                    draw_channel_row(ui, 1, "外部轮廓", &mut active_ch, &mut custom_outline_on, &custom_outline_rgb, custom_outline_a, &mut changed);
                    draw_channel_row(ui, 2, "五官表情", &mut active_ch, &mut custom_ink_on, &custom_ink_rgb, custom_ink_a, &mut changed);
                });
                match active_ch {
                    0 => draw_shared_palette(ui, 0, "内部填充", &mut custom_fill_on, &mut custom_fill_rgb, &mut custom_fill_a, right_w, total_left_h, &mut changed),
                    1 => draw_shared_palette(ui, 1, "外部轮廓", &mut custom_outline_on, &mut custom_outline_rgb, &mut custom_outline_a, right_w, total_left_h, &mut changed),
                    _ => draw_shared_palette(ui, 2, "五官表情", &mut custom_ink_on, &mut custom_ink_rgb, &mut custom_ink_a, right_w, total_left_h, &mut changed),
                }
            });
        } else {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = row_gap;
                draw_channel_row(ui, 0, "内部填充", &mut active_ch, &mut custom_fill_on, &custom_fill_rgb, custom_fill_a, &mut changed);
                draw_channel_row(ui, 1, "外部轮廓", &mut active_ch, &mut custom_outline_on, &custom_outline_rgb, custom_outline_a, &mut changed);
                draw_channel_row(ui, 2, "五官表情", &mut active_ch, &mut custom_ink_on, &custom_ink_rgb, custom_ink_a, &mut changed);
            });
            ui.add_space(6.0);
            match active_ch {
                0 => draw_shared_palette(ui, 0, "内部填充", &mut custom_fill_on, &mut custom_fill_rgb, &mut custom_fill_a, right_w, 152.0, &mut changed),
                1 => draw_shared_palette(ui, 1, "外部轮廓", &mut custom_outline_on, &mut custom_outline_rgb, &mut custom_outline_a, right_w, 152.0, &mut changed),
                _ => draw_shared_palette(ui, 2, "五官表情", &mut custom_ink_on, &mut custom_ink_rgb, &mut custom_ink_a, right_w, 152.0, &mut changed),
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(active_ch_id, active_ch));
    });

    ui.add_space(6.0);

    // ════════════════════════════════════════════════════════════════════════
    // 卡片 2：岛体显示大小 + 11 种形象网格 + 当前分类状态表情矩阵（统一包裹在 settings_card 中，修复图二左边贴边与 9+2/6+1 孤立换行）
    // ════════════════════════════════════════════════════════════════════════
    crate::extras::settings_card(ui, |ui| {
        let avail_w = crate::extras::layout_avail_w(ui).max(200.0);
        ui.set_width(avail_w);
        ui.set_max_width(avail_w);

        // 1) 主岛显示大小滑块
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("岛体显示大小").size(11.2).strong());
            let mut pct = scale as f32;
            let reserved = if avail_w >= 380.0 { 195.0 } else { 105.0 };
            let slider_w = (avail_w - reserved).clamp(75.0, 340.0);
            ui.spacing_mut().slider_width = slider_w - 52.0;
            if ui
                .add_sized(
                    egui::vec2(slider_w, 20.0),
                    egui::Slider::new(&mut pct, 70.0..=140.0)
                        .suffix("%")
                        .show_value(true),
                )
                .changed()
            {
                scale = pct.round() as u8;
                changed = true;
            }
            if avail_w >= 380.0 {
                ui.label(egui::RichText::new("实时同步至灵动岛").size(10.0).color(sub_text));
            }
        });

        ui.add_space(4.0);
        crate::extras::twinkle_card_break(ui);
        ui.add_space(2.0);

        // 2) 全部 72 款 Aora Bot 官方身体形态与分类筛选网格（保留 0..=10 原序号兼容旧配置，新增 11..=71 全量形态）
        let shape_cat_id = egui::Id::new("aora_shape_family_filter");
        let mut shape_cat = ui.data(|d| d.get_temp::<usize>(shape_cat_id)).unwrap_or(0).min(6);
        let cur_shape_obj = BodyShape::ALL[shape_idx];
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(5.0, 4.0);
            ui.label(
                egui::RichText::new(format!(
                    "身体形态（{} 款 · 当前：{}）",
                    BodyShape::ALL.len(),
                    cur_shape_obj.label_zh()
                ))
                .size(11.0)
                .strong()
                .color(if dark { egui::Color32::from_gray(220) } else { egui::Color32::from_rgb(51, 51, 51) }),
            );
            for (ci, clabel) in [
                (0usize, "全部 72"),
                (1, "基础"),
                (2, "花叶/田园"),
                (3, "水果/美食"),
                (4, "造型/物件"),
                (5, "动物/海洋"),
                (6, "人物/煤球"),
            ] {
                let on = shape_cat == ci;
                let bg_c = if on {
                    egui::Color32::from_rgb(10, 132, 255)
                } else {
                    card_idle
                };
                let fg_c = if on { egui::Color32::WHITE } else { text_idle };
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(clabel).size(9.8).color(fg_c))
                            .fill(bg_c)
                            .corner_radius(egui::CornerRadius::same(5))
                            .min_size(egui::vec2(0.0, 20.0)),
                    )
                    .clicked()
                {
                    shape_cat = ci;
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(shape_cat_id, shape_cat));
        ui.add_space(3.0);

        let filtered_shapes: Vec<usize> = match shape_cat {
            0 => {
                let mut v = Vec::with_capacity(BodyShape::ALL.len());
                v.push(10); // 初代 islo 置顶展示
                for idx in 0..BodyShape::ALL.len() {
                    if idx != 10 {
                        v.push(idx);
                    }
                }
                v
            }
            1 => {
                let mut v = vec![10];
                for idx in 0..BodyShape::ALL.len() {
                    if idx != 10 && matches!(BodyShape::ALL[idx].family_idx(), 0..=2) {
                        v.push(idx);
                    }
                }
                v
            }
            2 => (0..BodyShape::ALL.len()).filter(|&i| i != 10 && matches!(BodyShape::ALL[i].family_idx(), 3 | 4)).collect(),
            3 => (0..BodyShape::ALL.len()).filter(|&i| i != 10 && matches!(BodyShape::ALL[i].family_idx(), 5 | 6)).collect(),
            4 => (0..BodyShape::ALL.len()).filter(|&i| i != 10 && matches!(BodyShape::ALL[i].family_idx(), 7 | 8)).collect(),
            5 => (0..BodyShape::ALL.len()).filter(|&i| i != 10 && matches!(BodyShape::ALL[i].family_idx(), 9 | 10)).collect(),
            _ => (0..BodyShape::ALL.len()).filter(|&i| i != 10 && matches!(BodyShape::ALL[i].family_idx(), 11 | 12)).collect(),
        };
        let shape_cols: usize = if avail_w >= 620.0 {
            9
        } else if avail_w >= 480.0 {
            8
        } else if avail_w >= 340.0 {
            6
        } else {
            4
        };
        let shape_gap = 4.5_f32;
        let shape_card_w = ((avail_w - (shape_cols as f32 - 1.0) * shape_gap) / shape_cols as f32).clamp(38.0, 82.0);
        let shape_card_h = (shape_card_w * 0.86).clamp(44.0, 56.0);

        ui.spacing_mut().item_spacing.y = shape_gap;
        for row_slice in filtered_shapes.chunks(shape_cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = shape_gap;
                for &i in row_slice {
                    let shape = BodyShape::ALL[i];
                    let selected = i == shape_idx;
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(shape_card_w, shape_card_h),
                        if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                    );
                    if resp.clicked() {
                        shape_idx = i;
                        changed = true;
                    }
                    let fill = if selected {
                        egui::Color32::from_rgb(10, 132, 255)
                    } else if resp.hovered() {
                        card_hover
                    } else {
                        card_idle
                    };
                    ui.painter().rect_filled(rect, 7.5, fill);
                    let icon_card = Card::new(ui, rect.min, rect);
                    let cur_color = PALETTE[color_idx];
                    let cur_style = ArtStyle::ALL[style_idx];
                    let mid_x = shape_card_w * 0.5;
                    let mid_y = shape_card_h * 0.37;
                    if shape == BodyShape::Islo {
                        let islo_ink = if color_idx <= 1 {
                            if selected || dark {
                                egui::Color32::WHITE
                            } else {
                                egui::Color32::from_rgb(51, 51, 51)
                            }
                        } else {
                            cur_color
                        };
                        islo_face::draw_custom(&icon_card, mid_x, mid_y, 0.74, IsloFace::Happy, 0.0, [0.0, 0.0], islo_ink);
                    } else {
                        let cur_fill = if custom_fill_on {
                            egui::Color32::from_rgba_unmultiplied(
                                custom_fill_rgb[0],
                                custom_fill_rgb[1],
                                custom_fill_rgb[2],
                                custom_fill_a,
                            )
                        } else {
                            cur_color
                        };
                        let cur_out = custom_outline_on.then(|| {
                            egui::Color32::from_rgba_unmultiplied(
                                custom_outline_rgb[0],
                                custom_outline_rgb[1],
                                custom_outline_rgb[2],
                                custom_outline_a,
                            )
                        });
                        let body_r = (shape_card_h * 0.23).clamp(9.5, 13.5);
                        aora_face::draw_shape_dual(
                            &icon_card,
                            mid_x,
                            mid_y,
                            body_r,
                            shape,
                            cur_fill,
                            cur_out,
                            cur_style,
                        );
                        let mini_ink = if custom_ink_on {
                            egui::Color32::from_rgba_unmultiplied(
                                custom_ink_rgb[0],
                                custom_ink_rgb[1],
                                custom_ink_rgb[2],
                                custom_ink_a,
                            )
                        } else {
                            aora_face::resolve_mascot_ink(cur_fill, cur_out, cur_style, &cfg)
                        };
                        let ([fx, fy], eye_fit) = aora_face::shape_face_fit(shape);
                        aora_face::draw_face_styled(
                            &icon_card,
                            mid_x + fx * body_r,
                            mid_y + fy * body_r,
                            0.50 * eye_fit,
                            0,
                            mini_ink,
                            FaceStyle::ALL[face_idx],
                            0.0,
                        );
                    }
                    ui.painter().text(
                        egui::pos2(rect.center().x, rect.bottom() - 8.0),
                        egui::Align2::CENTER_CENTER,
                        shape.label_zh(),
                        egui::FontId::proportional(8.8),
                        if selected { egui::Color32::WHITE } else { text_idle },
                    );
                }
            });
        }

        ui.add_space(6.0);
        crate::extras::twinkle_card_break(ui);
        ui.add_space(2.0);

        // 3) 当前表情动作响应式矩阵（静息 6 状态 + 自动 = 7 项时一行 7 列均分，彻底告别图二 6+1 孤立换行）
        let expr_gap = 4.5_f32;
        if shape_idx == 10 {
            let expr_cols: usize = if avail_w >= 480.0 { 7 } else { 5 };
            let expr_w = ((avail_w - (expr_cols as f32 - 1.0) * expr_gap) / expr_cols as f32).clamp(38.0, 74.0);
            let expr_h = (expr_w * 0.86).clamp(42.0, 52.0);
            ui.label(
                egui::RichText::new("初代经典形象（共 20 种表情）· 点选固定岛上表情，或选「自动」随岛内状态智能切换")
                    .size(10.5)
                    .color(sub_text),
            );
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(expr_gap, expr_gap);
                let auto_selected = fixed <= 0;
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(expr_w, expr_h),
                    if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                );
                if resp.clicked() {
                    fixed = 0;
                    changed = true;
                }
                let fill = if auto_selected {
                    egui::Color32::from_rgb(10, 132, 255)
                } else if resp.hovered() {
                    card_hover
                } else {
                    card_idle
                };
                ui.painter().rect_filled(rect, 7.5, fill);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "自动",
                    egui::FontId::proportional(10.2),
                    if auto_selected { egui::Color32::WHITE } else { text_idle },
                );

                for (fi, f) in IsloFace::ALL.iter().enumerate() {
                    let is_fixed = fixed == (fi as i64 + 1);
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(expr_w, expr_h),
                        if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                    );
                    if resp.clicked() {
                        fixed = fi as i64 + 1;
                        changed = true;
                    }
                    let fill = if is_fixed {
                        egui::Color32::from_rgb(10, 132, 255)
                    } else if resp.hovered() {
                        card_hover
                    } else {
                        card_idle
                    };
                    ui.painter().rect_filled(rect, 7.5, fill);
                    let mini_card = Card::new(ui, rect.min, rect);
                    islo_face::draw_mini(&mini_card, expr_w * 0.5, expr_h * 0.38, 0.74, *f, now);
                    ui.painter().text(
                        egui::pos2(rect.center().x, rect.bottom() - 8.0),
                        egui::Align2::CENTER_CENTER,
                        f.label(),
                        egui::FontId::proportional(8.8),
                        if is_fixed { egui::Color32::WHITE } else { text_idle },
                    );
                }
            });
        } else {
            let cat_states: Vec<usize> = STATES
                .iter()
                .enumerate()
                .filter(|(_, s)| s.cat == selected_cat)
                .map(|(i, _)| i)
                .collect();
            let total_items = cat_states.len() + 1;
            let expr_cols: usize = if total_items == 7 && avail_w >= 300.0 {
                7
            } else if total_items == 4 && avail_w >= 200.0 {
                4
            } else {
                ((avail_w + expr_gap) / 52.0).floor().clamp(4.0, 10.0) as usize
            };
            let expr_w = ((avail_w - (expr_cols as f32 - 1.0) * expr_gap) / expr_cols as f32).clamp(38.0, 74.0);
            let expr_h = (expr_w * 0.86).clamp(42.0, 52.0);

            ui.label(
                egui::RichText::new(format!(
                    "{}（{} 项）· 点击固定表情，或选「自动」随状态切换",
                    selected_cat.label_zh(),
                    cat_states.len()
                ))
                .size(10.5)
                .color(sub_text),
            );
            ui.add_space(2.0);

            let mut all_slots: Vec<Option<usize>> = Vec::with_capacity(total_items);
            all_slots.push(None);
            for &si in &cat_states {
                all_slots.push(Some(si));
            }

            ui.spacing_mut().item_spacing.y = expr_gap;
            for row_slots in all_slots.chunks(expr_cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = expr_gap;
                    for slot in row_slots {
                        match slot {
                            None => {
                                let auto_selected = fixed <= 0;
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(expr_w, expr_h),
                                    if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                                );
                                if resp.clicked() {
                                    fixed = 0;
                                    changed = true;
                                }
                                let fill = if auto_selected {
                                    egui::Color32::from_rgb(10, 132, 255)
                                } else if resp.hovered() {
                                    card_hover
                                } else {
                                    card_idle
                                };
                                ui.painter().rect_filled(rect, 7.5, fill);
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "自动",
                                    egui::FontId::proportional(10.2),
                                    if auto_selected { egui::Color32::WHITE } else { text_idle },
                                );
                            }
                            Some(si) => {
                                let si = *si;
                                let st = &STATES[si];
                                let is_fixed = fixed == (si as i64 + 1);
                                let is_preview = preview_state == si;
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(expr_w, expr_h),
                                    if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                                );
                                if resp.clicked() {
                                    fixed = si as i64 + 1;
                                    preview_state = si;
                                    ui.data_mut(|d| d.insert_temp(prev_id, preview_state));
                                    changed = true;
                                }
                                let fill = if is_fixed {
                                    egui::Color32::from_rgb(10, 132, 255)
                                } else if resp.hovered() {
                                    card_hover
                                } else {
                                    card_idle
                                };
                                ui.painter().rect_filled(rect, 7.5, fill);
                                if is_preview && !is_fixed {
                                    ui.painter().rect_stroke(
                                        rect,
                                        7.5,
                                        egui::Stroke::new(1.2, egui::Color32::from_rgb(10, 132, 255)),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                let mini_card = Card::new(ui, rect.min, rect);
                                let expr_ink = if is_fixed || dark {
                                    egui::Color32::from_gray(242)
                                } else {
                                    egui::Color32::from_rgb(51, 51, 51)
                                };
                                aora_face::draw_face_styled(
                                    &mini_card,
                                    expr_w * 0.5,
                                    expr_h * 0.38,
                                    (expr_h / 58.0).clamp(0.68, 0.92),
                                    si,
                                    expr_ink,
                                    FaceStyle::ALL[face_idx],
                                    now,
                                );
                                ui.painter().text(
                                    egui::pos2(rect.center().x, rect.bottom() - 8.0),
                                    egui::Align2::CENTER_CENTER,
                                    st.label_zh,
                                    egui::FontId::proportional(8.8),
                                    if is_fixed { egui::Color32::WHITE } else { text_idle },
                                );
                            }
                        }
                    }
                });
            }
        }
    });

    // ════════════════════════════════════════════════════════════════════════
    // 卡片 3：AI 工作状态 × 吉祥物 19 个「处理中」表情包联动设置（统一包裹在 settings_card 中）
    // ════════════════════════════════════════════════════════════════════════
    ui.add_space(6.0);
    let mut ai_link = cfg.ai_mascot_link;
    let mut ai_mode = cfg.ai_mascot_mode;
    let mut ai_intv = cfg.ai_mascot_interval;
    let mut ai_changed = false;
    crate::extras::settings_card(ui, |ui| {
        let avail_w = crate::extras::layout_avail_w(ui).max(200.0);
        ui.set_width(avail_w);
        ui.set_max_width(avail_w);

        ui.horizontal(|ui| {
            if ui.checkbox(&mut ai_link, "").changed() {
                ai_changed = true;
            }
            ui.label(
                egui::RichText::new("AI 工作状态联动吉祥物表情")
                    .size(11.8)
                    .strong()
                    .color(text_idle),
            );
            if avail_w >= 420.0 {
                ui.label(
                    egui::RichText::new("AI 运行时自动切换「处理中」动态表情")
                        .size(10.2)
                        .color(sub_text),
                );
            }
        });
        if ai_link {
            ui.add_space(4.0);
            let btn_w = ((avail_w - 76.0) * 0.5).clamp(115.0, 205.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("联动方式：").size(11.2).color(sub_text));
                for (m_val, m_label) in [
                    (0u8, "↻ 随机轮播 19 个「处理中」表情"),
                    (1u8, "✦ 按 AI 动作智能匹配 + 随机轮播"),
                ] {
                    let sel = ai_mode == m_val;
                    let b = egui::Button::new(
                        egui::RichText::new(m_label)
                            .size(10.8)
                            .color(if sel { egui::Color32::WHITE } else { text_idle }),
                    )
                    .fill(if sel {
                        egui::Color32::from_rgb(10, 132, 255)
                    } else {
                        card_hover
                    })
                    .corner_radius(6.0);
                    if ui.add_sized([btn_w, 25.0], b).clicked() && !sel {
                        ai_mode = m_val;
                        ai_changed = true;
                    }
                }
            });
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("切换间隔：").size(11.2).color(sub_text));
                if ui
                    .add(egui::DragValue::new(&mut ai_intv).range(2..=15).suffix(" 秒"))
                    .changed()
                {
                    ai_changed = true;
                }
                ui.label(
                    egui::RichText::new("平滑轮播间隔")
                        .size(10.2)
                        .color(sub_text),
                );
            });
        }
    });
    if ai_changed {
        config::update(|c| {
            c.ai_mascot_link = ai_link;
            c.ai_mascot_mode = ai_mode;
            c.ai_mascot_interval = ai_intv;
        });
    }

    // ── 保存 ──
    if changed {
        config::update(|c| {
            c.aora_style = style_idx as u8;
            c.aora_shape = shape_idx as u8;
            c.aora_color = color_idx as u8;
            c.aora_face_style = face_idx as u8;
            c.aora_scale = scale;
            c.islo_fixed = fixed;
            c.aora_custom_fill_enabled = custom_fill_on;
            c.aora_custom_fill_rgb = custom_fill_rgb;
            c.face_fill_alpha = custom_fill_a;
            c.aora_custom_outline_enabled = custom_outline_on;
            c.aora_custom_outline_rgb = custom_outline_rgb;
            c.face_stroke_alpha = custom_outline_a;
            c.aora_custom_ink_enabled = custom_ink_on;
            c.aora_custom_ink_rgb = custom_ink_rgb;
            c.aora_custom_ink_alpha = custom_ink_a;
        });
    }
}
