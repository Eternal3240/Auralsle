//! Twinkle Tray 1.17.2 原生 1:1 复刻设置页：显示器与亮度 (`SettingsWindow.jsx` + `SettingsOption.jsx` + `Slider.jsx`)。
//!
//! 严格对应 `xanderfrangos/twinkle-tray` 源码架构：
//! - 顶层容器 `#page` 内置滚动，首个 `.sectionTitle` (`28px`) 为「所有显示器」
//! - 子标题 `.sectionSubtitle` (`18px / 600`) 配 `Segoe Fluent Icons` `\u{E7F4}` 显示器图标
//! - 卡片 `.settings-option-elem` (`border-radius: 4px`, `#2B2B2B` 暗 / `#FBFBFB` 亮, `1px` 内描边, `margin-bottom: 6px`)
//! - 卡片内行 `.parent-panel`：左侧 `.content-area` 垂直居中 (`16px` 标题 + `12.5px` 描述)，右侧 `.input-area` 固定宽框垂直居中
//! - 滑块 `.input--range` (`slider.586709b8.css`)：`4px` 轨道 + `18px` 外圆 (`#414141`) 套 `10px` 强调色内圆 (`#4CC2FF`) + 右侧 `.val` 数值
//! - 子项展开动画 `settingsOptionChildrenIn 0.3s` 与滑条块淡入 `sliderFadeIn`

use crate::extras::{
    react_bits_combo, setting_row, setting_row_card, setting_select_card, setting_select_row,
    settings_card, settings_expand_header, settings_label, settings_subpanel, twinkle_button,
    twinkle_card_break, twinkle_parent_panel, twinkle_slider_f32,
};
use crate::monitors::{
    input_source_name, ScheduleRule, ScheduleTrigger, INPUT_SOURCES, VCP_AUDIO_VOLUME,
    VCP_CONTRAST, VCP_INPUT_SOURCE,
};
use eframe::egui;

fn text_cols(ui: &egui::Ui) -> (egui::Color32, egui::Color32) {
    let dark = ui.ctx().style().visuals.dark_mode;
    (
        if dark {
            egui::Color32::from_rgb(0xe4, 0xe4, 0xe4)
        } else {
            egui::Color32::from_rgb(24, 24, 26)
        },
        if dark {
            egui::Color32::from_rgb(0xbb, 0xbb, 0xbb)
        } else {
            egui::Color32::from_rgb(90, 92, 98)
        },
    )
}

/// Twinkle `sliderFadeIn` 关键帧映射：`0% -> 33%` 保持 opacity 0，`33% -> 100%` 从 0 线性过渡到 1。
pub fn slider_fade_in_alpha(elapsed_secs: f32) -> f32 {
    const TOTAL_DUR: f32 = 0.36;
    if !elapsed_secs.is_finite() || elapsed_secs <= 0.0 {
        return 0.0;
    }
    if elapsed_secs >= TOTAL_DUR {
        return 1.0;
    }
    let progress = (elapsed_secs / TOTAL_DUR).clamp(0.0, 1.0);
    if progress <= 0.33 {
        0.0
    } else {
        ((progress - 0.33) / 0.67).clamp(0.0, 1.0)
    }
}

/// 绘制 `Segoe Fluent Icons` `\u{E7F4}` 显示器图标（含矢量回退）
fn paint_monitor_icon(ui: &mut egui::Ui, color: egui::Color32) {
    let (ico_r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
    let icon_font = crate::theme::fluent_icon_font(18.0);
    let has_fluent = !cfg!(test) && ui.fonts(|f| f.has_glyph(&icon_font, '\u{E7F4}'));
    if has_fluent {
        ui.painter().text(
            ico_r.center(),
            egui::Align2::CENTER_CENTER,
            "\u{E7F4}",
            icon_font,
            color,
        );
    } else {
        let scr_r = egui::Rect::from_center_size(
            egui::pos2(ico_r.center().x, ico_r.center().y - 2.0),
            egui::vec2(18.0, 12.0),
        );
        ui.painter().rect_stroke(scr_r, 1.5, egui::Stroke::new(1.5, color), egui::StrokeKind::Inside);
        ui.painter().line_segment(
            [egui::pos2(scr_r.center().x, scr_r.bottom()), egui::pos2(scr_r.center().x, scr_r.bottom() + 3.5)],
            egui::Stroke::new(1.5, color),
        );
        ui.painter().line_segment(
            [egui::pos2(scr_r.center().x - 4.5, scr_r.bottom() + 3.5), egui::pos2(scr_r.center().x + 4.5, scr_r.bottom() + 3.5)],
            egui::Stroke::new(1.5, color),
        );
    }
}

pub(crate) fn draw_settings(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let (title_col, desc_col) = text_cols(ui);
    let list = crate::monitors::snapshot();
    let now_t = ctx.input(|i| i.time);

    let mut brightness_page_visible = crate::auralsle_ai_status::config::get().brightness_page_visible;
    let prev_bpv = brightness_page_visible;
    setting_row_card(
        ui,
        "在灵动岛中显示亮度控制页面",
        "开启后可在展开态灵动岛新增「亮度」分页，随时直接滑条调节外接显示器与笔记本内置屏幕亮度。",
        &mut brightness_page_visible,
    );
    if brightness_page_visible != prev_bpv {
        crate::auralsle_ai_status::config::update(|c| {
            c.brightness_page_visible = brightness_page_visible;
        });
    }

    // ── P0-2 `sliderFadeIn`：检测列表从 0 块屏变成 N 块屏（或点击「重新检测」后）触发一次整段淡入 ──
    let prev_count_id = egui::Id::new("tw_slider_prev_mon_count");
    let fade_start_id = egui::Id::new("tw_slider_fade_start_t");
    let prev_count = ui.data(|d| d.get_temp::<usize>(prev_count_id)).unwrap_or(0);
    if prev_count == 0 && !list.is_empty() {
        ui.data_mut(|d| d.insert_temp(fade_start_id, now_t));
    }
    ui.data_mut(|d| d.insert_temp(prev_count_id, list.len()));
    let fade_start = ui.data(|d| d.get_temp::<f64>(fade_start_id)).unwrap_or(0.0);
    let slider_block_alpha = if cfg!(test) || fade_start <= 0.0 {
        1.0
    } else {
        let elapsed = (now_t - fade_start).max(0.0) as f32;
        let a = slider_fade_in_alpha(elapsed);
        if elapsed < 0.365 {
            ctx.request_repaint();
        }
        a
    };

    // ── 0. Twinkle Tray 1.17.2 原版「所有显示器」信息块与 4 张核心卡片（1:1 对齐 `SettingsWindow.jsx` `getMonitors()`）──
    ui.add_space(4.0);
    let mon_title = list
        .first()
        .map(|m| m.label.as_str())
        .unwrap_or("TL160MDMP01");
    let mon_cur_br = list
        .first()
        .map(|m| (m.percent() * 100.0).round() as i32)
        .unwrap_or(78);
    let mon_comm = if list.is_empty() {
        "WMI"
    } else if list.first().is_some_and(|m| m.slot == crate::monitors::INTERNAL_PANEL_SLOT) {
        "LCD / WMI"
    } else {
        "DDC/CI"
    };
    let mon_max_str = list
        .first()
        .map(|m| m.max.to_string())
        .unwrap_or_else(|| "不支持".to_owned());

    // `.sectionSubtitle` (`18px / 600`)：`\u{E7F4}` 监视器图标 + 型号标题
    ui.horizontal(|ui| {
        paint_monitor_icon(ui, title_col);
        ui.add_space(6.0);
        ui.label(egui::RichText::new(mon_title).size(18.0).strong().color(title_col));
    });
    ui.add_space(10.0);
    let mon_cur_str = mon_cur_br.to_string();
    let left_rows: [(&str, &str, bool); 4] = [
        ("名称: ", mon_title, false),
        ("内部名称: ", "TMA2024", false),
        ("通信方式: ", mon_comm, true),
        ("当前亮度: ", &mon_cur_str, false),
    ];
    let right_rows: [(&str, &str, bool); 3] = [
        ("最大亮度: ", &mon_max_str, false),
        ("统一亮度区间: ", "0 - 100", false),
        ("HDR: ", "已激活", false),
    ];
    let draw_info_row = |ui: &mut egui::Ui, k: &str, v: &str, has_check: bool| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.label(egui::RichText::new(k).size(14.0).color(desc_col));
            ui.label(egui::RichText::new(v).size(14.0).strong().color(title_col));
            if has_check {
                ui.add_space(4.0);
                let (chk_r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                ui.painter().rect_filled(chk_r, 2.5, egui::Color32::from_rgb(16, 152, 48));
                let c = chk_r.center();
                let s = egui::Stroke::new(1.5, egui::Color32::WHITE);
                ui.painter().line_segment([egui::pos2(c.x - 3.5, c.y + 0.2), egui::pos2(c.x - 0.8, c.y + 3.0)], s);
                ui.painter().line_segment([egui::pos2(c.x - 0.8, c.y + 3.0), egui::pos2(c.x + 3.8, c.y - 2.6)], s);
            }
        });
    };
    let col_w = ((crate::extras::layout_avail_w(ui) - 24.0) * 0.5).max(160.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 24.0;
        ui.allocate_ui_with_layout(egui::vec2(col_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(col_w);
            for (k, v, has_check) in left_rows {
                draw_info_row(ui, k, v, has_check);
            }
        });
        ui.allocate_ui_with_layout(egui::vec2(col_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(col_w);
            for (k, v, has_check) in right_rows {
                draw_info_row(ui, k, v, has_check);
            }
        });
    });
    ui.add_space(18.0);

    // 卡片 1：亮度更新速度（左文右 `<select>` 下拉框 `138×40`）
    let rate_id = egui::Id::new("tw_update_rate_ms");
    let mut update_rate = ui.data(|d| d.get_temp::<u32>(rate_id)).unwrap_or(250);
    let rate_lbl = match update_rate {
        100 => "极快 (100ms)",
        250 => "快 (250ms)",
        500 => "正常 (500ms)",
        _ => "慢 (1000ms)",
    };
    if setting_select_card(
        ui,
        "亮度更新速度",
        "调整显示器的亮度值时，显示器亮度的更新速度。如果显示器闪烁，请延长亮度更新时间。",
        "tw-update-rate-select",
        rate_lbl,
        &mut update_rate,
        &[
            (100u32, "极快 (100ms)"),
            (250u32, "快 (250ms)"),
            (500u32, "正常 (500ms)"),
            (1000u32, "慢 (1000ms)"),
        ],
    ) {
        ui.data_mut(|d| d.insert_temp(rate_id, update_rate));
    }

    // 卡片 2：隐藏显示器（可展开卡片 `settingsOptionChildrenIn` 0.3s）
    settings_card(ui, |ui| {
        let hide_open_id = egui::Id::new("tw_hide_displays_open");
        let mut hide_open = ui.data(|d| d.get_temp::<bool>(hide_open_id)).unwrap_or(false);
        let hide_anim_id = egui::Id::new("tw_hide_displays_anim");
        let mut hide_anim = ui.data(|d| d.get_temp::<f32>(hide_anim_id)).unwrap_or(0.0);
        if settings_expand_header(
            ui,
            "tw-hide-displays-hdr",
            "隐藏显示器",
            "如果您想从弹出窗口中隐藏特定的显示器，您可以在此处将其关闭。",
            &mut hide_open,
        ) {
            ui.data_mut(|d| d.insert_temp(hide_open_id, hide_open));
        }
        settings_subpanel(ui, &mut hide_anim, hide_open, "tw-hide-displays-sub", |ui| {
            let mut rules = crate::monitors::load_monitor_rules();
            let mut changed_r = false;
            if list.is_empty() {
                let mut show_def = true;
                setting_row(ui, mon_title, "在亮度滑块列表中显示此显示器", &mut show_def);
            } else {
                for (i, m) in list.iter().enumerate() {
                    if i > 0 {
                        twinkle_card_break(ui);
                    }
                    let rule_idx = rules.rules.iter().position(|r| r.match_label.eq_ignore_ascii_case(&m.label));
                    let mut visible_mon = rule_idx.map(|idx| !rules.rules[idx].hidden).unwrap_or(true);
                    let prev_v = visible_mon;
                    setting_row(ui, &m.label, "在亮度滑块列表中显示此显示器", &mut visible_mon);
                    if visible_mon != prev_v {
                        if let Some(idx) = rule_idx {
                            rules.rules[idx].hidden = !visible_mon;
                        } else {
                            rules.rules.push(crate::monitors::MonitorRuleEntry {
                                match_label: m.label.clone(),
                                custom_name: String::new(),
                                hidden: !visible_mon,
                                min: None,
                                max: None,
                                startup_brightness: None,
                                brightness_vcp: None,
                            });
                        }
                        changed_r = true;
                    }
                }
            }
            if changed_r {
                crate::monitors::save_monitor_rules(&rules);
            }
        });
        ui.data_mut(|d| d.insert_temp(hide_anim_id, hide_anim));
    });

    // 卡片 3：隐藏不活动的内部显示器（Twinkle 独立开关卡片）
    let hide_int_id = egui::Id::new("tw_hide_inactive_internal");
    let mut hide_inactive_internal = ui.data(|d| d.get_temp::<bool>(hide_int_id)).unwrap_or(false);
    let prev_hi = hide_inactive_internal;
    setting_row_card(
        ui,
        "隐藏不活动的内部显示器",
        "如果显示器被合上，或者在 Windows 中禁用了内建屏幕（自带显示器），那么该显示器的亮度滑块将被隐藏。",
        &mut hide_inactive_internal,
    );
    if hide_inactive_internal != prev_hi {
        ui.data_mut(|d| d.insert_temp(hide_int_id, hide_inactive_internal));
    }

    // 卡片 4：重命名显示器（可展开卡片 `settingsOptionChildrenIn` 0.3s）
    settings_card(ui, |ui| {
        let ren_open_id = egui::Id::new("tw_rename_displays_open");
        let mut ren_open = ui.data(|d| d.get_temp::<bool>(ren_open_id)).unwrap_or(false);
        let ren_anim_id = egui::Id::new("tw_rename_displays_anim");
        let mut ren_anim = ui.data(|d| d.get_temp::<f32>(ren_anim_id)).unwrap_or(0.0);
        if settings_expand_header(
            ui,
            "tw-rename-displays-hdr",
            "重命名显示器",
            "如果您希望每个显示器使用不同的名称（例如“左显示器”、“中显示器”），可以在下面输入。不填写将恢复原始名称。",
            &mut ren_open,
        ) {
            ui.data_mut(|d| d.insert_temp(ren_open_id, ren_open));
        }
        settings_subpanel(ui, &mut ren_anim, ren_open, "tw-rename-displays-sub", |ui| {
            let alias_id = egui::Id::new("tw_mon_custom_alias");
            let mut alias = ui.data(|d| d.get_temp::<String>(alias_id)).unwrap_or_default();
            twinkle_parent_panel(ui, mon_title, "", 240.0, 32.0, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let resp = ui.add_sized(
                        [236.0, 30.0],
                        egui::TextEdit::singleline(&mut alias).hint_text("恢复原始名称"),
                    );
                    if resp.changed() {
                        ui.data_mut(|d| d.insert_temp(alias_id, alias.clone()));
                    }
                });
            });
        });
        ui.data_mut(|d| d.insert_temp(ren_anim_id, ren_anim));
    });

    // ── 1. DDC/CI 功能与屏幕亮度控制（P0-1 + P0-2 + P1-1）──
    settings_label(ui, "DDC/CI 功能");
    if list.is_empty() {
        settings_card(ui, |ui| {
            let demo_open_id = egui::Id::new("tw_vcp_preview_subpanel_open");
            let mut demo_open = ui.data(|d| d.get_temp::<bool>(demo_open_id)).unwrap_or(false);
            let demo_anim_id = egui::Id::new("tw_vcp_preview_subpanel_anim");
            let mut demo_anim = ui.data(|d| d.get_temp::<f32>(demo_anim_id)).unwrap_or(0.0);
            let note_str = crate::monitors::note()
                .unwrap_or_else(|| "启用对比度 (0x12)、音量 (0x62) 与电源状态等标准 DDC/CI VCP 扩展控制项。".to_owned());
            if settings_expand_header(
                ui,
                "tw-empty-vcp-expand",
                "启用 DDC/CI 扩展功能（亮度 0x10 · 对比度 0x12 · 输入源 0x60 · 音量 0x62）",
                &note_str,
                &mut demo_open,
            ) {
                ui.data_mut(|d| d.insert_temp(demo_open_id, demo_open));
            }
            settings_subpanel(ui, &mut demo_anim, demo_open, "tw-empty-vcp-subpanel", |ui| {
                for (i, (code_str, name_str, desc_str)) in [
                    ("0x10", "亮度 (Luminance)", "标准背光亮度滑块，支持按每屏规则重映射 min–max 量程"),
                    ("0x12", "对比度 (Contrast)", "硬件级画面对比度调节（0%–100%）"),
                    ("0x60", "输入源 (Input Source)", "一键切换 DisplayPort 1/2、HDMI 1/2、USB-C / DP Alt、DVI、VGA"),
                    ("0x62", "扬声器音量 (Audio Speaker Volume)", "内置音箱或显示器音频输出口音量调节与快捷键静音"),
                ]
                .into_iter()
                .enumerate()
                {
                    if i > 0 {
                        twinkle_card_break(ui);
                    }
                    let mut vcp_on = true;
                    setting_row(ui, &format!("{name_str} ({code_str})"), desc_str, &mut vcp_on);
                }
                twinkle_card_break(ui);
                if twinkle_button(ui, "重新检测显示器", false).clicked() {
                    ui.data_mut(|d| {
                        d.insert_temp(prev_count_id, 0usize);
                        d.insert_temp(fade_start_id, now_t);
                    });
                    crate::monitors::request_refresh();
                }
            });
            ui.data_mut(|d| d.insert_temp(demo_anim_id, demo_anim));
        });
    } else {
        // 整段滑条区块统一应用一次 `sliderFadeIn`（绝不逐卡错峰）
        ui.scope(|ui| {
            if slider_block_alpha < 0.999 {
                ui.multiply_opacity(slider_block_alpha);
            }
            for m in &list {
                let pct = m.percent() * 100.0;
                settings_card(ui, |ui| {
                    let mut value = pct;
                    twinkle_parent_panel(
                        ui,
                        &m.label,
                        &format!("DDC/CI (0x{:02X}) · 亮度量程 {}–{}", m.brightness_vcp, m.min, m.max),
                        240.0,
                        28.0,
                        |ui| {
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                if twinkle_slider_f32(ui, ("tw-mon-br", m.slot), &mut value, 0.0..=100.0, true, "%") {
                                    crate::monitors::set_value(m.slot, m.value_from_percent(value / 100.0));
                                }
                            });
                        },
                    );

                    // ── P0-1 + P1-1：每块屏「高级」折叠 -> 对比度 0x12 / 输入源 0x60 / 音量 0x62 ──
                    twinkle_card_break(ui);
                    let adv_open_id = egui::Id::new(("tw_mon_adv_open", m.slot));
                    let mut adv_open = ui.data(|d| d.get_temp::<bool>(adv_open_id)).unwrap_or(false);
                    let adv_anim_id = egui::Id::new(("tw_mon_adv_anim", m.slot));
                    let mut adv_anim = ui.data(|d| d.get_temp::<f32>(adv_anim_id)).unwrap_or(0.0);
                    let adv_summary = format!(
                        "对比度 {} · 音量 {} · 输入源 {}",
                        m.contrast_percent()
                            .map(|v| format!("{}%", v.round() as i32))
                            .unwrap_or_else(|| "默认".into()),
                        m.volume_percent()
                            .map(|v| format!("{}%", v.round() as i32))
                            .unwrap_or_else(|| "默认".into()),
                        m.input.map(input_source_name).unwrap_or("自动"),
                    );
                    if settings_expand_header(
                        ui,
                        ("tw-mon-adv-hdr", m.slot),
                        "高级 VCP 控制（对比度 / 音量 / 输入源）",
                        &adv_summary,
                        &mut adv_open,
                    ) {
                        ui.data_mut(|d| d.insert_temp(adv_open_id, adv_open));
                    }
                    settings_subpanel(
                        ui,
                        &mut adv_anim,
                        adv_open,
                        ("tw-mon-adv-sub", m.slot),
                        |ui| {
                            // 1) 对比度 0x12
                            let mut c_pct = m.contrast_percent().unwrap_or(50.0);
                            twinkle_parent_panel(ui, "对比度 (VCP 0x12)", "", 230.0, 28.0, |ui| {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    if twinkle_slider_f32(ui, ("tw-mon-con", m.slot), &mut c_pct, 0.0..=100.0, true, "%") {
                                        crate::monitors::set_vcp(
                                            m.slot,
                                            VCP_CONTRAST,
                                            m.contrast_from_percent(c_pct),
                                        );
                                    }
                                });
                            });

                            twinkle_card_break(ui);
                            // 2) 扬声器音量 0x62
                            let mut v_pct = m.volume_percent().unwrap_or(50.0);
                            twinkle_parent_panel(ui, "扬声器音量 (VCP 0x62)", "", 230.0, 28.0, |ui| {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    if twinkle_slider_f32(ui, ("tw-mon-vol", m.slot), &mut v_pct, 0.0..=100.0, true, "%") {
                                        crate::monitors::set_vcp(
                                            m.slot,
                                            VCP_AUDIO_VOLUME,
                                            m.volume_from_percent(v_pct),
                                        );
                                    }
                                });
                            });

                            twinkle_card_break(ui);
                            // 3) 输入源切换 0x60
                            let mut cur_input = m.input.unwrap_or(15);
                            let prev_input = cur_input;
                            setting_select_row(
                                ui,
                                "视频输入源 (VCP 0x60)",
                                "",
                                ("tw-mon-input-src", m.slot),
                                input_source_name(cur_input),
                                &mut cur_input,
                                INPUT_SOURCES,
                            );
                            if cur_input != prev_input {
                                crate::monitors::set_vcp(m.slot, VCP_INPUT_SOURCE, cur_input);
                            }
                        },
                    );
                    ui.data_mut(|d| d.insert_temp(adv_anim_id, adv_anim));
                });
            }
            settings_card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if twinkle_button(ui, "重新检测显示器", false).clicked() {
                        ui.data_mut(|d| {
                            d.insert_temp(prev_count_id, 0usize);
                            d.insert_temp(fade_start_id, now_t);
                        });
                        crate::monitors::request_refresh();
                    }
                    if twinkle_button(ui, "全部 100%", false).clicked() {
                        for m in crate::monitors::snapshot() {
                            crate::monitors::set_value(m.slot, m.max);
                        }
                    }
                    if twinkle_button(ui, "全部 50%", false).clicked() {
                        for m in crate::monitors::snapshot() {
                            crate::monitors::set_value(m.slot, m.value_from_percent(0.5));
                        }
                    }
                });
            });
        });
    }

    // ── 2. 定时调整（P1-4：对齐 Twinkle Tray `SETTINGS_TIME_TITLE`）──
    settings_label(ui, "定时调整");
    let mut tw_cfg = crate::monitors::get_twinkle_config();
    let prev_tw = tw_cfg.clone();
    let ((sr_h, sr_m), (ss_h, ss_m)) =
        crate::monitors::compute_suncalc_times(tw_cfg.latitude, tw_cfg.longitude);

    setting_row_card(
        ui,
        "启用自动时段亮度调度",
        "在指定时间或日出日落将显示器设置为特定亮度水平。",
        &mut tw_cfg.schedule_enabled,
    );

    if tw_cfg.schedule_enabled {
        settings_card(ui, |ui| {
            twinkle_parent_panel(
                ui,
                "日出与日落定位坐标",
                &format!("当前计算：今日日出 {:02}:{:02} · 日落 {:02}:{:02}", sr_h, sr_m, ss_h, ss_m),
                220.0,
                30.0,
                |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("纬").size(12.0).color(desc_col));
                        ui.add_sized(
                            [68.0, 28.0],
                            egui::DragValue::new(&mut tw_cfg.latitude)
                                .range(-65.0..=65.0)
                                .speed(0.1)
                                .fixed_decimals(2)
                                .suffix("°"),
                        );
                        ui.label(egui::RichText::new("经").size(12.0).color(desc_col));
                        ui.add_sized(
                            [72.0, 28.0],
                            egui::DragValue::new(&mut tw_cfg.longitude)
                                .range(-180.0..=180.0)
                                .speed(0.1)
                                .fixed_decimals(2)
                                .suffix("°"),
                        );
                    });
                },
            );
        });

        let mut remove_sched: Option<usize> = None;
        for (idx, rule) in tw_cfg.schedules.iter_mut().enumerate() {
            settings_card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut rule.enabled, "");
                    react_bits_combo(
                        ui,
                        ("tw-sched-trig", idx),
                        rule.trigger.label(),
                        140.0,
                        &mut rule.trigger,
                        &[
                            (ScheduleTrigger::FixedTime, "固定时刻"),
                            (ScheduleTrigger::Sunrise, "日出 (SunCalc)"),
                            (ScheduleTrigger::Sunset, "日落 (SunCalc)"),
                        ],
                    );
                    if rule.trigger == ScheduleTrigger::FixedTime {
                        let mut h = rule.hour as u32;
                        let mut m = rule.minute as u32;
                        if ui
                            .add_sized(
                                [54.0, 30.0],
                                egui::DragValue::new(&mut h)
                                    .range(0..=23)
                                    .custom_formatter(|v, _| format!("{:02}时", v as u32)),
                            )
                            .changed()
                        {
                            rule.hour = h as u8;
                        }
                        ui.label(":");
                        if ui
                            .add_sized(
                                [54.0, 30.0],
                                egui::DragValue::new(&mut m)
                                    .range(0..=59)
                                    .custom_formatter(|v, _| format!("{:02}分", v as u32)),
                            )
                            .changed()
                        {
                            rule.minute = m as u8;
                        }
                    } else {
                        let (eh, em) = if rule.trigger == ScheduleTrigger::Sunrise {
                            (sr_h, sr_m)
                        } else {
                            (ss_h, ss_m)
                        };
                        ui.label(
                            egui::RichText::new(format!("({:02}:{:02})", eh, em))
                                .size(13.0)
                                .monospace()
                                .color(desc_col),
                        );
                    }
                    let mut pct_f = rule.target_percent as f32;
                    if twinkle_slider_f32(ui, ("tw-sched-pct", idx), &mut pct_f, 0.0..=100.0, true, "%") {
                        rule.target_percent = pct_f.round().clamp(0.0, 100.0) as u8;
                    }
                    if twinkle_button(ui, "删除", false).clicked() {
                        remove_sched = Some(idx);
                    }
                });
            });
        }
        if let Some(rm) = remove_sched {
            if rm < tw_cfg.schedules.len() {
                tw_cfg.schedules.remove(rm);
            }
        }
        settings_card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if twinkle_button(ui, "+ 添加时间", false).clicked() {
                    use chrono::{Local, Timelike};
                    let next_dt = Local::now() + chrono::Duration::minutes(1);
                    tw_cfg.schedules.push(ScheduleRule {
                        enabled: true,
                        trigger: ScheduleTrigger::FixedTime,
                        hour: next_dt.hour() as u8,
                        minute: next_dt.minute() as u8,
                        target_percent: 60,
                    });
                }
                if twinkle_button(ui, "设为 1 分钟后测试", false).clicked() {
                    use chrono::{Local, Timelike};
                    let next_dt = Local::now() + chrono::Duration::minutes(1);
                    if let Some(first) = tw_cfg.schedules.first_mut() {
                        first.enabled = true;
                        first.trigger = ScheduleTrigger::FixedTime;
                        first.hour = next_dt.hour() as u8;
                        first.minute = next_dt.minute() as u8;
                    } else {
                        tw_cfg.schedules.push(ScheduleRule {
                            enabled: true,
                            trigger: ScheduleTrigger::FixedTime,
                            hour: next_dt.hour() as u8,
                            minute: next_dt.minute() as u8,
                            target_percent: 50,
                        });
                    }
                }
            });
        });
    }

    // ── 3. 快捷键（P1-3：对齐 Twinkle Tray `SETTINGS_HOTKEYS_TITLE`）──
    settings_label(ui, "快捷键");
    let mut hk_cfg = crate::hotkeys::get_config();
    let prev_hk = hk_cfg.clone();
    let hk_status = crate::hotkeys::get_status();
    let rec_target_id = egui::Id::new("tw_hotkey_recording_slot");
    let mut rec_slot: Option<usize> = ui.data(|d| d.get_temp::<Option<usize>>(rec_target_id)).unwrap_or(None);

    if let Some(active_slot) = rec_slot {
        if let Some(captured) = crate::hotkeys::capture_egui_chord(ui) {
            match active_slot {
                0 => hk_cfg.up_hotkey = captured,
                1 => hk_cfg.down_hotkey = captured,
                2 => hk_cfg.mute_hotkey = captured,
                _ => {}
            }
            rec_slot = None;
            ui.data_mut(|d| d.insert_temp(rec_target_id, None::<usize>));
        }
    }

    setting_row_card(
        ui,
        "启用亮度与静音全局热键",
        "在任意桌面或全屏应用中按下快捷键直接步进调节所有显示器亮度或切换静音。",
        &mut hk_cfg.enabled,
    );

    let step_lbl = match hk_cfg.step_percent {
        2 => "2%",
        10 => "10%",
        _ => "5%",
    };
    setting_select_card(
        ui,
        "快捷键调整幅度",
        "每次按下亮度增加或降低快捷键时调整亮度的百分比。",
        "tw-hotkey-step-select",
        step_lbl,
        &mut hk_cfg.step_percent,
        &[(2u32, "2%"), (5u32, "5%"), (10u32, "10%")],
    );

    for (slot_idx, row_title, row_desc, key_val, _is_ok, err_msg) in [
        (
            0usize,
            "亮度增加快捷键",
            format!("按下后所有屏幕亮度 +{}%", hk_cfg.step_percent),
            &mut hk_cfg.up_hotkey,
            hk_status.up_ok,
            &hk_status.up_err,
        ),
        (
            1usize,
            "亮度降低快捷键",
            format!("按下后所有屏幕亮度 −{}%", hk_cfg.step_percent),
            &mut hk_cfg.down_hotkey,
            hk_status.down_ok,
            &hk_status.down_err,
        ),
        (
            2usize,
            "显示器静音快捷键",
            "切换外接屏扬声器静音（无外接屏音频时回退切换系统主音量静音）".to_owned(),
            &mut hk_cfg.mute_hotkey,
            hk_status.mute_ok,
            &hk_status.mute_err,
        ),
    ] {
        settings_card(ui, |ui| {
            let is_rec = rec_slot == Some(slot_idx);
            let btn_label = if is_rec {
                "请按键… (Esc 清空)".to_owned()
            } else if key_val.trim().is_empty() {
                "点击录入快捷键".to_owned()
            } else {
                key_val.clone()
            };
            twinkle_parent_panel(ui, row_title, &row_desc, 156.0, 38.0, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if twinkle_button(ui, &btn_label, is_rec)
                        .on_hover_text("点击后直接按下组合键（如 Ctrl+Alt+Up），按 Backspace/Esc 清除")
                        .clicked()
                    {
                        rec_slot = if is_rec { None } else { Some(slot_idx) };
                        ui.data_mut(|d| d.insert_temp(rec_target_id, rec_slot));
                    }
                });
            });
            if let Some(err) = err_msg {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("⚠ {err}"))
                        .size(12.0)
                        .color(egui::Color32::from_rgb(248, 113, 113)),
                );
            }
        });
    }
    if hk_cfg != prev_hk {
        crate::hotkeys::update_config(hk_cfg);
    }

    // ── 4. 统一亮度与每屏规则（P1-2 + P1-5）──
    settings_label(ui, "统一亮度与电源规则");
    setting_row_card(
        ui,
        "启动时恢复上次亮度",
        "退出时记录每块屏的亮度，下次启动写回屏幕。",
        &mut tw_cfg.restore_on_startup,
    );
    setting_row_card(
        ui,
        "锁屏或会话切换时禁用 DDC/CI 写入",
        "监听系统锁屏事件，锁屏期间暂停写入 VCP，防止部分外接显示器在锁屏时闪烁唤醒。",
        &mut tw_cfg.disable_on_lock_screen,
    );

    settings_card(ui, |ui| {
        let rules_p = crate::monitors::rules_path();
        twinkle_parent_panel(
            ui,
            "每屏规则配置文件 (monitor-rules.json)",
            &format!("路径：{}（支持按型号自定义名称、隐藏屏幕、重映射量程与默认亮度）", rules_p.display()),
            210.0,
            38.0,
            |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if twinkle_button(ui, "重载规则", false).clicked() {
                        let _ = crate::monitors::load_monitor_rules();
                        crate::monitors::request_refresh();
                    }
                    if twinkle_button(ui, "打开目录", false).clicked() {
                        let dir = crate::model::config_dir();
                        let _ = std::fs::create_dir_all(&dir);
                        #[cfg(windows)]
                        {
                            let _ = std::process::Command::new("explorer").arg(&dir).spawn();
                        }
                    }
                });
            },
        );
    });

    if tw_cfg != prev_tw {
        crate::monitors::update_twinkle_config(tw_cfg);
    }
}
