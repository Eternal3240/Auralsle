//! 设置页「课程表」独立页面 — 深度对标 [拾光课程表 (ShiGuangSchedule)](https://github.com/XingHeYuZhuan/shiguangschedule) 桌面端与移动端体验。
//!
//! 四大核心模块：
//! 1. **周课表（Week Table）**：顶部周次胶囊 + 1~20 周矩阵弹窗快速跳转（带「本周」标记）+ 星期/真实日期表头 + 今日列高亮
//!    + 上午/下午/晚间分段 + 跨节次彩色圆角课程卡片 + 非本周课程灰显开关 + 点空白格快捷建课 / 点课程块即刻编辑。
//! 2. **今日课表 & 明日预告（Today & Tomorrow）**：按时间轴展示今日全部课程进度与状态，并内置拾光课程表特色的「明日课程预告」卡片。
//! 3. **全局课程管理 & 作息（Courses & Schedule）**：全部课程可视化卡片列表（快速增删改）、1~13 节作息时间编辑器、
//!    Excel 课表识别导入、课程文本批量添加、以及通用日历 `.ics` 导出。
//! 4. **课表岛与网格个性化（Customization）**：格子高度、圆角、不透明度、周起始日（周一/周日）、节假日免打扰与实时课表岛预览。

use crate::{
    auralsle_ai_status::{config, course, plugins},
    extras::{setting_row, settings_card, settings_label},
};
use chrono::{Datelike, Duration as ChronoDuration, Local};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::sync::{Mutex, OnceLock};

const GUTTER: f32 = 60.0;
const HEADER_H: f32 = 34.0;
const PERIODS: u32 = 13;
const DAYS_MON: [(&str, u32); 7] = [
    ("一", 1),
    ("二", 2),
    ("三", 3),
    ("四", 4),
    ("五", 5),
    ("六", 6),
    ("日", 7),
];
const DAYS_SUN: [(&str, u32); 7] = [
    ("日", 7),
    ("一", 1),
    ("二", 2),
    ("三", 3),
    ("四", 4),
    ("五", 5),
    ("六", 6),
];

/// 拾光课程表精选马卡龙/高对比度课程色板（同一门课按名称稳定哈希取色）。
const COLORS: [Color32; 10] = [
    Color32::from_rgb(59, 130, 246),  // 晴空蓝
    Color32::from_rgb(16, 185, 129),  // 翡翠绿
    Color32::from_rgb(245, 158, 11),  // 琥珀橙
    Color32::from_rgb(168, 85, 247),  // 丁香紫
    Color32::from_rgb(244, 63, 94),   // 珊瑚红
    Color32::from_rgb(14, 165, 233),  // 冰川青
    Color32::from_rgb(236, 72, 153),  // 樱花粉
    Color32::from_rgb(132, 204, 22),  // 青柠绿
    Color32::from_rgb(99, 102, 241),  // 星云靛
    Color32::from_rgb(234, 88, 12),   // 落日橘
];

fn course_color(name: &str) -> Color32 {
    let h = name
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    COLORS[(h as usize) % COLORS.len()]
}

fn dim_text(ui: &egui::Ui) -> Color32 {
    if ui.ctx().style().visuals.dark_mode {
        Color32::from_gray(172)
    } else {
        Color32::from_gray(90)
    }
}

fn primary_text(ui: &egui::Ui) -> Color32 {
    if ui.ctx().style().visuals.dark_mode {
        Color32::WHITE
    } else {
        Color32::from_rgb(30, 34, 42)
    }
}

fn course_skin(ui: &egui::Ui) -> crate::extras::SettingsSkinTheme {
    crate::extras::current_settings_skin_theme(ui.ctx())
}

fn color_field_row(ui: &mut egui::Ui, label: &str, hex: &mut String) {
    let parsed = plugins::parse_hex_rgba(hex).unwrap_or([255, 255, 255, 255]);
    let mut rgba = egui::Color32::from_rgba_unmultiplied(parsed[0], parsed[1], parsed[2], parsed[3]);
    ui.allocate_ui_with_layout(
        egui::vec2(52.0, 24.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(egui::RichText::new(format!("{label}:")).size(11.5).strong());
        },
    );
    ui.allocate_ui_with_layout(
        egui::vec2(34.0, 24.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().interact_size = egui::vec2(32.0, 22.0);
            if egui::color_picker::color_edit_button_srgba(
                ui,
                &mut rgba,
                egui::color_picker::Alpha::OnlyBlend,
            )
            .changed()
            {
                *hex = if rgba.a() == 255 {
                    format!("#{:02X}{:02X}{:02X}", rgba.r(), rgba.g(), rgba.b())
                } else {
                    format!("#{:02X}{:02X}{:02X}{:02X}", rgba.r(), rgba.g(), rgba.b(), rgba.a())
                };
            }
        },
    );
    ui.allocate_ui_with_layout(
        egui::vec2(84.0, 24.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add_sized([82.0, 22.0], egui::TextEdit::singleline(hex).vertical_align(egui::Align::Center));
        },
    );
    let mut a_pct = ((rgba.a() as f32 / 255.0) * 100.0).round() as u8;
    ui.allocate_ui_with_layout(
        egui::vec2(64.0, 24.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            if ui
                .add_sized(
                    [62.0, 22.0],
                    egui::DragValue::new(&mut a_pct).range(10..=100).prefix("A:").suffix("%"),
                )
                .changed()
            {
                let na = ((a_pct as f32 / 100.0) * 255.0).round().clamp(1.0, 255.0) as u8;
                *hex = if na == 255 {
                    format!("#{:02X}{:02X}{:02X}", rgba.r(), rgba.g(), rgba.b())
                } else {
                    format!("#{:02X}{:02X}{:02X}{:02X}", rgba.r(), rgba.g(), rgba.b(), na)
                };
            }
        },
    );
    let row_avail = crate::extras::layout_avail_w(ui);
    if row_avail >= 380.0 {
    ui.allocate_ui_with_layout(
        egui::vec2(150.0, 24.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            for (p_lbl, p_hex, p_col) in [
                ("白", "#FFFFFF", Color32::from_rgb(245, 245, 250)),
                ("橙", "#FFBE5C", Color32::from_rgb(255, 190, 92)),
                ("蓝", "#57B6FF", Color32::from_rgb(87, 182, 255)),
                ("绿", "#34D399", Color32::from_rgb(52, 211, 153)),
                ("紫", "#C084FC", Color32::from_rgb(192, 132, 252)),
            ] {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 22.0), egui::Sense::click());
                if resp.clicked() {
                    *hex = p_hex.to_owned();
                }
                let is_cur = hex.trim().eq_ignore_ascii_case(p_hex);
                ui.painter().rect_filled(rect, 5.5, p_col);
                if is_cur || resp.hovered() {
                    let ring_col = if is_cur {
                        plugins::active_accent_color()
                    } else {
                        primary_text(ui)
                    };
                    ui.painter().rect_stroke(
                        rect,
                        5.5,
                        egui::Stroke::new(if is_cur { 1.5_f32 } else { 1.0_f32 }, ring_col),
                        egui::StrokeKind::Inside,
                    );
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    p_lbl,
                    egui::FontId::proportional(10.5),
                    Color32::from_rgb(32, 32, 32),
                );
            }
        },
    );
    }
}

#[allow(dead_code)]
fn size_field_unused() {}
#[allow(dead_code)]
fn color_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(format!("{label}:")).size(11.2).strong());
        crate::extras::rgba_color_picker_inline(ui, label, value);
        for (name, hex) in [
            ("白", "#FFFFFFFF"),
            ("橙", "#FFBE5CFF"),
            ("蓝", "#57B6FFFF"),
            ("绿", "#6BE2A2FF"),
            ("紫", "#C79BFFFF"),
        ] {
            if ui.small_button(name).clicked() {
                *value = hex.to_owned();
            }
        }
    });
}

#[allow(dead_code)]
fn size_field(ui: &mut egui::Ui, label: &str, value: &mut f32) {
    ui.label(egui::RichText::new(label).size(11.0));
    ui.add(egui::DragValue::new(value).range(6.0..=32.0).speed(0.5));
}

fn week_slot() -> &'static Mutex<(bool, u32)> {
    static WEEK: OnceLock<Mutex<(bool, u32)>> = OnceLock::new();
    WEEK.get_or_init(|| Mutex::new((false, 1)))
}

fn current_week() -> u32 {
    let Ok(mut w) = week_slot().lock() else { return 1 };
    if !w.0 {
        w.0 = true;
        w.1 = course::week_now();
    }
    w.1
}

fn set_week(v: u32) {
    if let Ok(mut w) = week_slot().lock() {
        w.0 = true;
        w.1 = v.max(1);
    }
}

fn ellipsis(text: &str, max_w: f32, size: f32) -> String {
    let mut out = String::new();
    let mut w = 0.0;
    for ch in text.chars() {
        let cw = if ch.is_ascii() { size * 0.56 } else { size };
        if w + cw > max_w - size * 0.6 {
            out.push('…');
            break;
        }
        out.push(ch);
        w += cw;
    }
    out
}

/// 设置页入口（拾光课程表四分栏工作台）。
pub fn draw_settings(ui: &mut egui::Ui) {
    // [最小窗口] 下限从 300 降到 200：窄窗真实可用宽度只有 237 时，300 的下限会把整页顶出面板右缘。
    let avail_w = crate::extras::layout_avail_w(ui).max(200.0);
    ui.set_width(avail_w);
    ui.set_max_width(avail_w);

    let mut cfg = config::get();
    let before = cfg.clone();
    let rows = course::rows();
    let real_week = course::week_now();
    let week = current_week();
    let acc = plugins::active_accent_color();

    // ── 顶部：拾光课程表 (ShiGuangSchedule) 状态摘要与四分栏导航 ──
    let tab_id = egui::Id::new("shiguang_course_subtab");
    let mut subtab: usize = ui.data(|d| d.get_temp::<usize>(tab_id)).unwrap_or(0).min(3);
    let pin_id = egui::Id::new("shiguang_course_pin_island");
    let mut pin_on_screen: bool = ui.data(|d| d.get_temp::<bool>(pin_id)).unwrap_or(true);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
        for (idx, title) in [
            "周课表总览",
            "今日与明日",
            "课程与作息",
            "网格外观",
        ]
        .iter()
        .enumerate()
        {
            let skin = course_skin(ui);
            let idle_txt = crate::settings_theme::color(
                ui.ctx(),
                Color32::from_gray(208),
                Color32::from_rgb(52, 58, 70),
            );
            let selected = subtab == idx;
            let btn = egui::Button::new(
                egui::RichText::new(*title)
                    .size(11.5)
                    .strong()
                    .color(if selected { Color32::WHITE } else { idle_txt }),
            )
            .fill(if selected { acc } else { skin.bg_ctrl })
            .stroke(Stroke::new(1.0, if selected { acc } else { skin.border }))
            .corner_radius(7.0);
            if ui.add_sized([((avail_w - 18.0) / 4.0).clamp(84.0, 142.0), 28.0], btn).clicked() {
                subtab = idx;
                ui.data_mut(|d| d.insert_temp(tab_id, subtab));
            }
        }
    });

    ui.add_space(4.0);

    // [修复需求 8：取消编辑课程岛时课程岛常驻屏幕的功能]
    let _ = (pin_id, pin_on_screen);
    crate::auralsle_ai_status::set_course_island_pin_preview(false);

    match subtab {
        // =========================================================================
        // 分栏 0：周课表总览（支持周次矩阵快速跳转、今日高亮、非本周灰显、点击直接编辑/新建）
        // =========================================================================
        0 => {
            settings_card(ui, |ui| {
                // [需求 4：采用 ShiGuangSchedule (拾光课程表 https://github.com/ShiGuangSchedule/shiguangschedule) 总览 UI]
                let picker_id = egui::Id::new("shiguang_week_matrix_open");
                let mut picker_open = ui.data(|d| d.get_temp::<bool>(picker_id)).unwrap_or(false);
                let today_nav = Local::now().date_naive();
                let monday_nav = today_nav
                    - ChronoDuration::days(today_nav.weekday().num_days_from_monday() as i64)
                    + ChronoDuration::days((week as i64 - real_week as i64) * 7);
                let sunday_nav = monday_nav + ChronoDuration::days(6);

                // 拾光课程表标志性顶栏：左端大号「第 N 周」+ 日期跨度 +「本周/非本周」胶囊标签，右端周次切换与周次矩阵按钮
                let skin = course_skin(ui);
                let text_pri = primary_text(ui);
                let dark = ui.ctx().style().visuals.dark_mode;
                let bar_w = crate::extras::layout_avail_w(ui);
                egui::Frame::new()
                    .fill(skin.bg_ctrl)
                    .stroke(Stroke::new(1.0, skin.border))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_width((bar_w - 26.0).max(160.0));
                        // [窄宽排版] 顶栏拆成「信息行 + 导航行」两段，窄窗下自动优雅折行。
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                egui::RichText::new(format!("第 {week} 周"))
                                    .size(16.5)
                                    .strong()
                                    .color(text_pri),
                            );
                            let status_txt = if week == real_week {
                                "本周".to_owned()
                            } else {
                                format!("非本周 · 当前第{real_week}周")
                            };
                            let status_col = if week == real_week {
                                Color32::from_rgb(16, 185, 129)
                            } else {
                                Color32::from_rgb(245, 158, 11)
                            };
                            let badge_fg = if dark {
                                status_col
                            } else {
                                crate::paint::mix(status_col, Color32::BLACK, 0.25)
                            };
                            egui::Frame::new()
                                .fill(status_col.gamma_multiply(if dark { 0.22 } else { 0.14 }))
                                .stroke(Stroke::new(0.8, badge_fg))
                                .corner_radius(5.0)
                                .inner_margin(egui::Margin::symmetric(6, 2))
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new(status_txt).size(10.2).strong().color(badge_fg));
                                });
                            ui.label(
                                egui::RichText::new(format!(
                                    "{:04}/{:02}/{:02} – {:02}/{:02}",
                                    monday_nav.year(),
                                    monday_nav.month(),
                                    monday_nav.day(),
                                    sunday_nav.month(),
                                    sunday_nav.day()
                                ))
                                .size(11.0)
                                .color(dim_text(ui)),
                            );
                        });
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                            let nav_btn = |lbl: &str, active: bool| {
                                egui::Button::new(
                                    egui::RichText::new(lbl)
                                        .size(10.5)
                                        .strong()
                                        .color(if active { Color32::WHITE } else { text_pri }),
                                )
                                .fill(if active { acc } else { skin.bg_card })
                                .stroke(Stroke::new(1.0, if active { acc } else { skin.border }))
                                .corner_radius(6.0)
                            };
                            if ui.add_sized([64.0, 22.0], nav_btn("‹ 上周", false)).clicked() {
                                set_week(week.saturating_sub(1).max(1));
                            }
                            if ui.add_sized([64.0, 22.0], nav_btn("下周 ›", false)).clicked() {
                                set_week((week + 1).min(25));
                            }
                            if week != real_week
                                && ui.add_sized([70.0, 22.0], nav_btn("回到本周", false)).clicked()
                            {
                                set_week(real_week);
                            }
                            let matrix_btn = nav_btn(
                                if picker_open { "周次矩阵 ▴" } else { "周次矩阵 ▾" },
                                picker_open,
                            );
                            if ui.add_sized([86.0, 22.0], matrix_btn).clicked() {
                                picker_open = !picker_open;
                                ui.data_mut(|d| d.insert_temp(picker_id, picker_open));
                            }
                            let mut edit_real = real_week;
                            if ui
                                .add_sized(
                                    [74.0, 22.0],
                                    egui::DragValue::new(&mut edit_real)
                                        .range(1..=25)
                                        .prefix("校周:")
                                        .suffix("周"),
                                )
                                .on_hover_text("直接拖拽或输入数字校准当前是第几教学周")
                                .changed()
                            {
                                let _ = course::set_current_week_now(edit_real);
                                set_week(edit_real);
                            }
                        });
                    });

                // 点击顶部周次矩阵按钮弹出 1~20 周快速跳转网格（标记当前所在周）
                if picker_open {
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .fill(skin.bg_ctrl)
                        .stroke(Stroke::new(1.0, skin.border))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("快速跳转周次（高亮色为本周）")
                                    .size(10.5)
                                    .color(dim_text(ui)),
                            );
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                                for w in 1..=20u32 {
                                    let is_cur = w == week;
                                    let is_real = w == real_week;
                                    let label = if is_real { format!("第{w}周★") } else { format!("第{w}周") };
                                    let fill = if is_cur {
                                        acc
                                    } else if is_real {
                                        crate::paint::mix(skin.bg_card, acc, if dark { 0.24 } else { 0.16 })
                                    } else {
                                        skin.bg_card
                                    };
                                    let stroke = if is_cur || is_real { acc } else { skin.border };
                                    let btn = egui::Button::new(
                                        egui::RichText::new(label)
                                            .size(10.5)
                                            .color(if is_cur { Color32::WHITE } else { text_pri }),
                                    )
                                    .fill(fill)
                                    .stroke(Stroke::new(1.0, stroke))
                                    .corner_radius(5.0);
                                    if ui.add_sized([54.0, 22.0], btn).clicked() {
                                        set_week(w);
                                        ui.data_mut(|d| d.insert_temp(picker_id, false));
                                    }
                                }
                            });
                        });
                }

                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut cfg.course_show_weekend, "显示周末（周六、周日）");
                    ui.checkbox(&mut cfg.course_show_non_current, "显示非本周课程（灰显）");
                    if cfg.course_show_weekend {
                        ui.checkbox(&mut cfg.course_sunday_first, "周日作为一周起始");
                    }
                    ui.label(
                        egui::RichText::new("✦ 点课程块直接编辑，点空白格在该节次新建课程")
                            .size(10.5)
                            .color(dim_text(ui)),
                    );
                });

                ui.add_space(2.0);
                if let Some(index) = draw_week_grid(ui, &rows, week) {
                    if let Ok(mut slot) = course::editor().lock() {
                        *slot = course::Editor::from_row(index, &rows[index]);
                    }
                }
            });

            settings_label(ui, "快捷课程编辑（点击上方课表格子自动联动）");
            settings_card(ui, |ui| draw_editor_form(ui, &rows));
        }

        // =========================================================================
        // 分栏 1：今日课表 & 明日预告（全宽双栏高密度卡片 + 顶部4维KPI概览 + 本周7日速览，彻底解决图七空间利用率低问题）
        // =========================================================================
        1 => {
            let today = Local::now().date_naive();
            let tomorrow = today + ChronoDuration::days(1);
            let today_wd = today.weekday().num_days_from_monday() as u32 + 1;
            let tom_wd = tomorrow.weekday().num_days_from_monday() as u32 + 1;
            let tom_week = if tom_wd == 1 { real_week + 1 } else { real_week };
            let periods = course::period_labels();
            let now_hm = Local::now().format("%H:%M").to_string();

            let mut today_list: Vec<(usize, &course::CourseRow)> = rows
                .iter()
                .enumerate()
                .filter(|(_, r)| r.weekday == today_wd && course::week_applies(&r.weeks, real_week))
                .collect();
            today_list.sort_by_key(|(_, r)| r.from);

            let mut tom_list: Vec<&course::CourseRow> = rows
                .iter()
                .filter(|r| r.weekday == tom_wd && course::week_applies(&r.weeks, tom_week))
                .collect();
            tom_list.sort_by_key(|r| r.from);

            let week_active_courses: Vec<&course::CourseRow> = rows
                .iter()
                .filter(|r| course::week_applies(&r.weeks, real_week))
                .collect();
            let week_total_periods: u32 = week_active_courses
                .iter()
                .map(|r| r.to.saturating_sub(r.from) + 1)
                .sum();

            // 1) 顶部四宫格 KPI 状态条（横向铺满整行）
            let kpis = [
                ("当前教学周", format!("第 {} 周", real_week), format!("周{} · {:02}/{:02}", DAYS_MON[(today_wd - 1) as usize].0, today.month(), today.day()), acc),
                ("☀ 今日课程", format!("{} 门课", today_list.len()), if today_list.is_empty() { "今日自由安排".into() } else { format!("首节 第{}节起", today_list[0].1.from) }, Color32::from_rgb(16, 185, 129)),
                ("明日预告", format!("{} 门课", tom_list.len()), format!("周{} · {:02}/{:02}", DAYS_MON[(tom_wd - 1) as usize].0, tomorrow.month(), tomorrow.day()), Color32::from_rgb(245, 158, 11)),
                ("本周总学时", format!("{} 节课", week_total_periods), format!("本周共 {} 门课", week_active_courses.len()), Color32::from_rgb(168, 85, 247)),
            ];
            let kpi_dark = ui.ctx().style().visuals.dark_mode;
            let kpi_skin = course_skin(ui);
            let kpi_pri = primary_text(ui);
            ui.columns(4, |cols| {
                for (i, (title, big, sub, col)) in kpis.iter().enumerate() {
                    let col_w = cols[i].available_width();
                    let label_col = if kpi_dark {
                        *col
                    } else {
                        crate::paint::mix(*col, Color32::BLACK, 0.28)
                    };
                    egui::Frame::new()
                        .fill(crate::paint::mix(kpi_skin.bg_ctrl, *col, if kpi_dark { 0.14 } else { 0.10 }))
                        .stroke(Stroke::new(1.0, crate::paint::mix(kpi_skin.border, *col, 0.42)))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(&mut cols[i], |ui| {
                            ui.set_width((col_w - 22.0).max(52.0));
                            ui.label(egui::RichText::new(*title).size(10.5).color(label_col).strong());
                            ui.label(egui::RichText::new(big).size(16.0).strong().color(kpi_pri));
                            ui.label(egui::RichText::new(sub).size(10.0).color(dim_text(ui)));
                        });
                }
            });

            ui.add_space(6.0);

            // 2) 左右双栏并排：「今日课表」与「明日课程预告」各占 50% 宽度，课程卡片内部全宽展开
            let render_course_card = |ui: &mut egui::Ui, r: &course::CourseRow, is_today: bool| {
                let st = periods.get(r.from.saturating_sub(1) as usize).map(|x| x.0.as_str()).unwrap_or("08:00");
                let et = periods.get(r.to.saturating_sub(1) as usize).map(|x| x.1.as_str()).unwrap_or("09:40");
                let (badge, bcol) = if !is_today {
                    ("明日待上", Color32::from_rgb(245, 158, 11))
                } else if now_hm.as_str() >= st && now_hm.as_str() <= et {
                    ("上课中", Color32::from_rgb(16, 185, 129))
                } else if now_hm.as_str() < st {
                    ("待上课", acc)
                } else {
                    ("已结束", Color32::from_gray(130))
                };
                let ccol = course_color(&r.name);
                let dark = ui.ctx().style().visuals.dark_mode;
                let skin = course_skin(ui);
                let header_col = if dark { ccol } else { crate::paint::mix(ccol, Color32::BLACK, 0.28) };
                let card_w = ui.available_width();
                egui::Frame::new()
                    .fill(crate::paint::mix(skin.bg_ctrl, ccol, if dark { 0.14 } else { 0.11 }))
                    .stroke(Stroke::new(1.0, crate::paint::mix(skin.border, ccol, if dark { 0.48 } else { 0.42 })))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_width((card_w - 26.0).max(110.0));
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("第 {}-{} 节 · {st}-{et}", r.from, r.to)).size(11.5).color(header_col).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new(format!("[{badge}]")).size(10.5).color(bcol).strong());
                            });
                        });
                        ui.label(egui::RichText::new(&r.name).size(14.0).strong());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}   {}   {}周",
                                    if r.room.is_empty() { "未填教室" } else { &r.room },
                                    if r.teacher.is_empty() { "未填教师" } else { &r.teacher },
                                    r.weeks
                                ))
                                .size(11.0)
                                .color(dim_text(ui)),
                            );
                        });
                    });
                ui.add_space(4.0);
            };

            if avail_w >= 480.0 {
                ui.columns(2, |cols| {
                    settings_card(&mut cols[0], |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            egui::RichText::new(format!("☀ 今日课表 · {}月{}日 (周{})", today.month(), today.day(), DAYS_MON[(today_wd - 1) as usize].0))
                                .size(12.5)
                                .strong(),
                        );
                        crate::extras::twinkle_card_break(ui);
                        if today_list.is_empty() {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("今天没有安排课程，享受自由时光吧！").size(12.5).strong());
                            ui.label(egui::RichText::new("可前往「全局课程与作息」从 Excel 导入或新增课程。").size(10.5).color(dim_text(ui)));
                            ui.add_space(6.0);
                        } else {
                            for (_, r) in &today_list {
                                render_course_card(ui, r, true);
                            }
                        }
                    });
                    settings_card(&mut cols[1], |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            egui::RichText::new(format!("明日课程预告 · {}月{}日 (周{})", tomorrow.month(), tomorrow.day(), DAYS_MON[(tom_wd - 1) as usize].0))
                                .size(12.5)
                                .strong(),
                        );
                        crate::extras::twinkle_card_break(ui);
                        if tom_list.is_empty() {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("✨ 明日无课 · 可以安心休息或自主安排复习").size(12.5).strong());
                            ui.label(egui::RichText::new("提前查看全周安排请见下方「本周 7 日课程矩阵」。").size(10.5).color(dim_text(ui)));
                            ui.add_space(6.0);
                        } else {
                            for r in &tom_list {
                                render_course_card(ui, r, false);
                            }
                        }
                    });
                });
            } else {
                settings_label(ui, &format!("今日课表 · {}月{}日", today.month(), today.day()));
                settings_card(ui, |ui| {
                    if today_list.is_empty() {
                        ui.label(egui::RichText::new("今天没有安排课程").size(12.5).strong());
                    } else {
                        for (_, r) in &today_list {
                            render_course_card(ui, r, true);
                        }
                    }
                });
                settings_label(ui, &format!("明日课程预告 · {}月{}日", tomorrow.month(), tomorrow.day()));
                settings_card(ui, |ui| {
                    if tom_list.is_empty() {
                        ui.label(egui::RichText::new("✨ 明日无课").size(12.5).strong());
                    } else {
                        for r in &tom_list {
                            render_course_card(ui, r, false);
                        }
                    }
                });
            }

            // 3) 本周（周一 ~ 周日）全宽日程速览矩阵，充分利用下方与右侧空间
            settings_label(ui, &format!("本周 7 日课程速览（第 {} 周 · 点击任意课程可跳转编辑）", real_week));
            settings_card(ui, |ui| {
                ui.set_width(ui.available_width());
                let cols_n = if avail_w >= 620.0 { 4 } else { 2 };
                let active_days: Vec<(u32, &str)> = DAYS_MON
                    .iter()
                    .map(|&(n, w)| (w, n))
                    .collect();
                for chunk in active_days.chunks(cols_n) {
                    ui.columns(cols_n, |cols| {
                        for (c_idx, &(wd, wd_name)) in chunk.iter().enumerate() {
                            let is_td = wd == today_wd;
                            let mut day_courses: Vec<(usize, &course::CourseRow)> = rows
                                .iter()
                                .enumerate()
                                .filter(|(_, r)| r.weekday == wd && course::week_applies(&r.weeks, real_week))
                                .collect();
                            day_courses.sort_by_key(|(_, r)| r.from);
                            let col_w = cols[c_idx].available_width();
                            let skin = course_skin(&cols[c_idx]);
                            let dark = cols[c_idx].ctx().style().visuals.dark_mode;
                            egui::Frame::new()
                                .fill(if is_td {
                                    crate::paint::mix(skin.bg_ctrl, acc, if dark { 0.16 } else { 0.11 })
                                } else {
                                    skin.bg_ctrl
                                })
                                .stroke(Stroke::new(1.0, if is_td { acc } else { skin.border }))
                                .corner_radius(8.0)
                                .inner_margin(egui::Margin::symmetric(8, 6))
                                .show(&mut cols[c_idx], |ui| {
                                    ui.set_width((col_w - 18.0).max(56.0));
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("周{}{}", wd_name, if is_td { " · 今天" } else { "" }))
                                                .size(11.0)
                                                .strong()
                                                .color(if is_td { acc } else { primary_text(ui) }),
                                        );
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new(format!("{}门", day_courses.len())).size(10.0).color(dim_text(ui)));
                                        });
                                    });
                                    if day_courses.is_empty() {
                                        ui.label(egui::RichText::new("无课").size(10.5).color(dim_text(ui)));
                                    } else {
                                        for (row_idx, r) in day_courses {
                                            let ccol = course_color(&r.name);
                                            let btn_txt = format!("{}-{}节 {}", r.from, r.to, ellipsis(&r.name, 96.0, 10.5));
                                            let chip_fill = crate::paint::mix(ccol, Color32::from_rgb(20, 24, 34), if dark { 0.18 } else { 0.08 });
                                            if ui
                                                .add(
                                                    egui::Button::new(egui::RichText::new(btn_txt).size(10.2).color(Color32::WHITE))
                                                        .fill(chip_fill)
                                                        .corner_radius(5.0),
                                                )
                                                .on_hover_text(format!("{} ({}周)
{}
{}
点击在全局课程中编辑", r.name, r.weeks, r.room, r.teacher))
                                                .clicked()
                                            {
                                                if let Ok(mut slot) = course::editor().lock() {
                                                    *slot = course::Editor::from_row(row_idx, r);
                                                }
                                                ui.data_mut(|d| d.insert_temp(tab_id, 2usize));
                                            }
                                        }
                                    }
                                });
                        }
                    });
                    ui.add_space(4.0);
                }
            });
        }

        // =========================================================================
        // 分栏 2：全局课程管理、作息时间表、ICS 导出与学期起始
        // =========================================================================
        2 => {
            settings_label(ui, "从 Excel 导入课表");
            settings_card(ui, |ui| draw_excel_import(ui, &mut cfg));

            settings_label(ui, "课程编辑与快速添加");
            settings_card(ui, |ui| draw_editor_form(ui, &rows));

            settings_label(ui, &format!("全局课程列表（共 {} 条上课记录）", rows.len()));
            settings_card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("导出通用日历 (.ics)").clicked() {
                        let dest = crate::model::config_dir().join("shiguang-courses.ics");
                        let mut status = course::import_status().lock().unwrap();
                        *status = match course::export_ics_to(&dest) {
                            Ok(n) => format!("已导出 {n} 节日程到 {}", dest.display()),
                            Err(e) => e,
                        };
                    }
                });
                if rows.is_empty() {
                    ui.label(egui::RichText::new("当前暂无课程：可在上方用 Excel 导入，或手动新增课程。").size(11.5).color(dim_text(ui)));
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("shiguang-global-courses")
                        .max_height(210.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let mut to_del: Option<usize> = None;
                            for (i, r) in rows.iter().enumerate() {
                                let ccol = course_color(&r.name);
                                ui.horizontal_wrapped(|ui| {
                                    let (dot_r, _) = ui.allocate_exact_size(egui::vec2(8.0, 18.0), Sense::hover());
                                    ui.painter().rect_filled(dot_r, 3.0, ccol);
                                    ui.label(egui::RichText::new(&r.name).size(12.5).strong());
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "周{} 第{}-{}节 · {}周 · {} · {}",
                                            DAYS_MON[(r.weekday.clamp(1, 7) - 1) as usize].0,
                                            r.from,
                                            r.to,
                                            r.weeks,
                                            if r.room.is_empty() { "无教室" } else { &r.room },
                                            if r.teacher.is_empty() { "无教师" } else { &r.teacher }
                                        ))
                                        .size(11.0)
                                        .color(dim_text(ui)),
                                    );
                                    if ui.small_button("编辑").clicked() {
                                        if let Ok(mut slot) = course::editor().lock() {
                                            *slot = course::Editor::from_row(i, r);
                                        }
                                    }
                                    if ui.small_button("删除").clicked() {
                                        to_del = Some(i);
                                    }
                                });
                            }
                            if let Some(di) = to_del {
                                if let Ok(msg) = course::delete_row(di) {
                                    if let Ok(mut s) = course::import_status().lock() {
                                        *s = msg;
                                    }
                                }
                            }
                        });
                }
            });

            settings_label(ui, "学期起始设置");
            settings_card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("第 1 周周一").size(12.0));
                    ui.add_sized(
                        egui::vec2(108.0, 22.0),
                        egui::TextEdit::singleline(&mut cfg.course_start).hint_text("2026-09-07"),
                    );
                    if ui.button("应用开学日期").clicked() {
                        let mut status = course::import_status().lock().unwrap();
                        *status = match course::set_semester_start(&cfg.course_start) {
                            Ok(()) => "学期起始已更新，周次立刻重算".to_owned(),
                            Err(e) => e,
                        };
                    }
                });
            });

            // 13 节作息时间表折叠编辑器
            settings_label(ui, "作息时间表（1 ~ 13 节上下课时间绑定）");
            settings_card(ui, |ui| {
                let period_id = egui::Id::new("shiguang_periods_draft");
                let mut draft = ui
                    .data(|d| d.get_temp::<Vec<(String, String)>>(period_id))
                    .unwrap_or_else(course::period_labels);
                if draft.len() < 13 {
                    draft = course::period_labels();
                }
                egui::CollapsingHeader::new("点击展开编辑第 1 ~ 13 节作息时间")
                    .default_open(false)
                    .show(ui, |ui| {
                        for (idx, (st, et)) in draft.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("第 {:>2} 节", idx + 1)).size(11.0));
                                ui.add_sized([58.0, 20.0], egui::TextEdit::singleline(st));
                                ui.label("—");
                                ui.add_sized([58.0, 20.0], egui::TextEdit::singleline(et));
                            });
                        }
                        if ui.button("保存作息时间表").clicked() {
                            let mut status = course::import_status().lock().unwrap();
                            *status = match course::save_periods(&draft) {
                                Ok(m) => m,
                                Err(e) => e,
                            };
                        }
                    });
                ui.data_mut(|d| d.insert_temp(period_id, draft));
            });
        }

        // =========================================================================
        // 分栏 3：课表岛与网格个性化（实时预览 + 拾光网格参数 + 提醒设置）
        // =========================================================================
        _ => {
            settings_label(ui, "课表提醒与免打扰");
            settings_card(ui, |ui| {
                setting_row(
                    ui,
                    "启用课表与课程岛",
                    "同步课表至日程页、月历与左侧课程岛",
                    &mut cfg.course_enabled,
                );
                crate::extras::twinkle_card_break(ui);
                setting_row(
                    ui,
                    "节假日与周末静默",
                    "节假日或无课日自动静音课前提醒",
                    &mut cfg.course_holiday_dnd,
                );
                crate::extras::twinkle_card_break(ui);
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("课前提醒").size(12.0));
                    ui.add(
                        egui::DragValue::new(&mut cfg.course_remind_minutes)
                            .range(0..=120)
                            .speed(5.0)
                            .suffix(" 分钟"),
                    );
                    ui.label(egui::RichText::new("0 为不提醒").size(11.0).color(dim_text(ui)));
                });
            });

            settings_label(ui, "课程岛样式与实时预览");
            settings_card(ui, |ui| {
                // [修复需求 9]：将 4 款课程岛 UI 样式分段选择器置于预览上方，切换后同一帧立即更新下方预览与屏幕课程岛！
                let prev_c_style = cfg.course_style;
                crate::extras::settings_control_row_right(ui, "课程岛 UI 样式", |ui| {
                    crate::extras::settings_segmented(
                        ui,
                        &mut cfg.course_style,
                        &[
                            (0u8, "经典时钟"),
                            (1u8, "灵动模板"),
                            (2u8, "圆环胶囊"),
                            (3u8, "双栏看板"),
                        ],
                        72.0,
                    );
                });
                if cfg.course_style != prev_c_style {
                    let ns = cfg.course_style;
                    config::update(|c| c.course_style = ns);
                    course::invalidate();
                    ui.ctx().request_repaint();
                }
                ui.add_space(4.0);
                crate::auralsle_ai_status::ui::draw_course_preview(ui, &cfg);
                crate::extras::twinkle_card_break(ui);
                ui.checkbox(&mut cfg.course_show_weekend, "在周课表中显示周末");
                if cfg.course_style == 1 {
                    crate::extras::twinkle_card_break(ui);
                    ui.label(egui::RichText::new("卡片排布模板").size(11.5).strong());
                    ui.label(egui::RichText::new(config::COURSE_LAYOUT_HELP).size(10.2).color(dim_text(ui)));
                    ui.add_sized(
                        egui::vec2(crate::extras::layout_avail_w(ui).max(160.0), 66.0),
                        egui::TextEdit::multiline(&mut cfg.course_layout).font(egui::TextStyle::Monospace),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("恢复默认布局").clicked() {
                            config::reset_course_style(&mut cfg);
                        }
                        ui.checkbox(&mut cfg.course_show_sep, "显示虚线分割");
                    });
                }
                crate::extras::twinkle_card_break(ui);
                // [修复需求 8 图四 `image_3a3b63.jpg` 红框处元素对齐]：
                // 采用严格列宽的 `egui::Grid` 对齐字号、圆角、不透明度与 4 行 RGBA 调色盘（标签列、色块列、HEX列、Alpha列、预设色芯片列 100% 垂直对齐）！
                let grid_avail_w = crate::extras::layout_avail_w(ui).max(180.0);
                if grid_avail_w >= 360.0 {
                    egui::Grid::new("course_size_align_grid_v30")
                        .num_columns(6)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("课程名").size(11.2));
                            ui.add_sized([60.0, 22.0], egui::DragValue::new(&mut cfg.course_title_size).range(8.0..=32.0).speed(0.5).suffix(" pt"));
                            ui.label(egui::RichText::new("地点").size(11.2));
                            ui.add_sized([60.0, 22.0], egui::DragValue::new(&mut cfg.course_place_size).range(8.0..=32.0).speed(0.5).suffix(" pt"));
                            ui.label(egui::RichText::new("时间段").size(11.2));
                            ui.add_sized([60.0, 22.0], egui::DragValue::new(&mut cfg.course_time_size).range(8.0..=28.0).speed(0.5).suffix(" pt"));
                            ui.end_row();

                            ui.label(egui::RichText::new("副文本").size(11.2));
                            ui.add_sized([60.0, 22.0], egui::DragValue::new(&mut cfg.course_small_size).range(6.0..=20.0).speed(0.5).suffix(" pt"));
                            ui.label(egui::RichText::new("字间距").size(11.2));
                            ui.add_sized([60.0, 22.0], egui::DragValue::new(&mut cfg.course_letter_spacing).range(-2.0..=8.0).speed(0.1).suffix(" pt"));
                            ui.label(egui::RichText::new("卡片圆角").size(11.2));
                            ui.add_sized([60.0, 22.0], egui::DragValue::new(&mut cfg.course_corner_r).range(4.0..=20.0).speed(0.5).suffix(" px"));
                            ui.end_row();

                            ui.label(egui::RichText::new("不透明度").size(11.2));
                            ui.add_sized([60.0, 22.0], egui::DragValue::new(&mut cfg.course_card_alpha).range(30..=100).speed(1.0).suffix("%"));
                            ui.label(egui::RichText::new("格子行高").size(11.2));
                            ui.add_sized([60.0, 22.0], egui::DragValue::new(&mut cfg.course_cell_h).range(28.0..=52.0).speed(0.5).suffix(" px"));
                            ui.label("");
                            ui.label("");
                            ui.end_row();
                        });
                } else {
                    egui::Grid::new("course_size_align_grid_narrow_v40")
                        .num_columns(4)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("课程名").size(11.0));
                            ui.add_sized([58.0, 22.0], egui::DragValue::new(&mut cfg.course_title_size).range(8.0..=32.0).speed(0.5).suffix(" pt"));
                            ui.label(egui::RichText::new("地点").size(11.0));
                            ui.add_sized([58.0, 22.0], egui::DragValue::new(&mut cfg.course_place_size).range(8.0..=32.0).speed(0.5).suffix(" pt"));
                            ui.end_row();

                            ui.label(egui::RichText::new("时间段").size(11.0));
                            ui.add_sized([58.0, 22.0], egui::DragValue::new(&mut cfg.course_time_size).range(8.0..=28.0).speed(0.5).suffix(" pt"));
                            ui.label(egui::RichText::new("副文本").size(11.0));
                            ui.add_sized([58.0, 22.0], egui::DragValue::new(&mut cfg.course_small_size).range(6.0..=20.0).speed(0.5).suffix(" pt"));
                            ui.end_row();

                            ui.label(egui::RichText::new("字间距").size(11.0));
                            ui.add_sized([58.0, 22.0], egui::DragValue::new(&mut cfg.course_letter_spacing).range(-2.0..=8.0).speed(0.1).suffix(" pt"));
                            ui.label(egui::RichText::new("卡片圆角").size(11.0));
                            ui.add_sized([58.0, 22.0], egui::DragValue::new(&mut cfg.course_corner_r).range(4.0..=20.0).speed(0.5).suffix(" px"));
                            ui.end_row();

                            ui.label(egui::RichText::new("不透明度").size(11.0));
                            ui.add_sized([58.0, 22.0], egui::DragValue::new(&mut cfg.course_card_alpha).range(30..=100).speed(1.0).suffix("%"));
                            ui.label(egui::RichText::new("格子行高").size(11.0));
                            ui.add_sized([58.0, 22.0], egui::DragValue::new(&mut cfg.course_cell_h).range(28.0..=52.0).speed(0.5).suffix(" px"));
                            ui.end_row();
                        });
                }
                crate::extras::twinkle_card_break(ui);
                egui::Grid::new("course_color_align_grid_v30")
                    .num_columns(5)
                    .spacing([10.0, 7.0])
                    .show(ui, |ui| {
                        for (label, hex_ref) in [
                            ("时间段", &mut cfg.course_time_color),
                            ("课程名", &mut cfg.course_title_color),
                            ("地点", &mut cfg.course_place_color),
                            ("强调色", &mut cfg.course_accent_color),
                        ] {
                            color_field_row(ui, label, hex_ref);
                            ui.end_row();
                        }
                    });
                crate::extras::twinkle_card_break(ui);
                crate::extras::settings_control_row_right(ui, "倒计时格式", |ui| {
                    let cur_cd = match cfg.course_countdown_style {
                        1 => "时钟 00:20:00",
                        2 => "还有 20 分钟",
                        _ => "20分钟后上课",
                    };
                    crate::extras::react_bits_combo(
                        ui,
                        "course-countdown-style-combo",
                        cur_cd,
                        165.0,
                        &mut cfg.course_countdown_style,
                        &[
                            (0u8, "20分钟后上课"),
                            (1u8, "时钟 00:20:00"),
                            (2u8, "还有 20 分钟"),
                        ],
                    );
                });
                crate::extras::settings_control_row_right(ui, "上课免打扰动作", |ui| {
                    let cur_dnd = match cfg.course_dnd_action {
                        0 => "启动番茄钟",
                        1 => "仅岛内免打扰",
                        _ => "打开系统通知设置",
                    };
                    crate::extras::react_bits_combo(
                        ui,
                        "course-dnd-action-combo",
                        cur_dnd,
                        195.0_f32.min((crate::extras::layout_avail_w(ui) * 0.48).max(120.0)),
                        &mut cfg.course_dnd_action,
                        &[
                            (0u8, "启动番茄钟"),
                            (1u8, "仅岛内免打扰"),
                            (2u8, "打开系统通知设置"),
                        ],
                    );
                });
            });
        }
    }

    if cfg != before {
        config::update(|c| *c = cfg);
    }
}

/// 一周网格（对标拾光课程表）：支持真实日期表头、今日高亮列、非本周灰显、点空白格快速建课、点色块编辑。
fn draw_week_grid(ui: &mut egui::Ui, rows: &[course::CourseRow], week: u32) -> Option<usize> {
    let cfg = config::get();
    let row_h = cfg.course_cell_h.clamp(28.0, 52.0);
    let corner_r = cfg.course_corner_r.clamp(2.0, 14.0);
    let alpha_pct = (cfg.course_card_alpha.clamp(35, 100) as f32) / 100.0;

    // [最小窗口] 下限从 240 降到 180：课程表格子在窄窗里必须能整块塞进面板。
    let width = ui.available_width().max(180.0);
    let height = HEADER_H + row_h * PERIODS as f32;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let p = ui.painter_at(rect);
    let raw_days: &[(&str, u32)] = if cfg.course_sunday_first && cfg.course_show_weekend {
        &DAYS_SUN
    } else {
        &DAYS_MON
    };
    let days: Vec<(&str, u32)> = raw_days
        .iter()
        .copied()
        .filter(|&(_, wd)| cfg.course_show_weekend || wd <= 5)
        .collect();
    let num_cols = days.len().max(1);
    let col_w = (width - GUTTER) / num_cols as f32;
    let today = Local::now().date_naive();
    let today_wd = today.weekday().num_days_from_monday() as u32 + 1;
    let real_week = course::week_now();
    let monday_this_week = today - ChronoDuration::days(today.weekday().num_days_from_monday() as i64)
        + ChronoDuration::days((week as i64 - real_week as i64) * 7);
    let acc = plugins::active_accent_color();
    let skin = course_skin(ui);
    let dark = ui.ctx().style().visuals.dark_mode;
    let text_pri = primary_text(ui);
    let text_sec = dim_text(ui);
    let grid_bg = crate::paint::mix(skin.bg_main, skin.bg_card, 0.35);

    p.rect_filled(rect, 8.0, grid_bg);
    p.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, skin.border),
        egui::StrokeKind::Inside,
    );

    // 左上角月份徽章
    let month_pill = Rect::from_center_size(
        Pos2::new(rect.left() + GUTTER * 0.5, rect.top() + HEADER_H * 0.5),
        Vec2::new(GUTTER - 8.0, HEADER_H - 8.0),
    );
    p.rect_filled(
        month_pill,
        6.0,
        Color32::from_rgba_unmultiplied(acc.r(), acc.g(), acc.b(), if dark { 28 } else { 20 }),
    );
    p.rect_stroke(
        month_pill,
        6.0,
        Stroke::new(0.8, crate::paint::mix(skin.border, acc, 0.35)),
        egui::StrokeKind::Inside,
    );
    p.text(
        Pos2::new(month_pill.center().x, month_pill.center().y - 4.5),
        Align2::CENTER_CENTER,
        format!("{}月", monday_this_week.month()),
        FontId::proportional(10.5),
        text_pri,
    );
    p.text(
        Pos2::new(month_pill.center().x, month_pill.center().y + 6.5),
        Align2::CENTER_CENTER,
        format!("W{week}"),
        FontId::proportional(8.0),
        acc,
    );

    // 7 列星期与真实日期表头 + 今日列高亮底色
    for (col_idx, &(day_name, wd)) in days.iter().enumerate() {
        let x0 = rect.left() + GUTTER + col_idx as f32 * col_w;
        let cx = x0 + col_w * 0.5;
        let col_date = monday_this_week + ChronoDuration::days(wd as i64 - 1);
        let is_today = week == real_week && wd == today_wd;

        if is_today {
            p.rect_filled(
                Rect::from_min_size(Pos2::new(x0 + 1.0, rect.top() + 2.0), Vec2::new(col_w - 2.0, height - 4.0)),
                6.0,
                Color32::from_rgba_unmultiplied(acc.r(), acc.g(), acc.b(), if dark { 18 } else { 14 }),
            );
            p.rect_filled(
                Rect::from_min_size(Pos2::new(x0 + 3.0, rect.top() + 3.0), Vec2::new(col_w - 6.0, HEADER_H - 6.0)),
                6.0,
                acc.gamma_multiply(0.92),
            );
        }

        p.text(
            Pos2::new(cx, rect.top() + 10.5),
            Align2::CENTER_CENTER,
            format!("周{day_name}"),
            FontId::proportional(10.5),
            if is_today { Color32::WHITE } else { text_pri },
        );
        p.text(
            Pos2::new(cx, rect.top() + 24.0),
            Align2::CENTER_CENTER,
            format!("{:02}/{:02}", col_date.month(), col_date.day()),
            FontId::proportional(8.8),
            if is_today { Color32::WHITE } else { text_sec },
        );
    }

    // 13 节行与时间标签
    let periods = course::period_labels();
    let odd_stripe = if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 5)
    } else {
        Color32::from_rgba_unmultiplied(15, 23, 42, 6)
    };
    let line_weak = skin.border;
    let line_break = crate::paint::mix(skin.border, text_pri, 0.24);
    let end_time_col = if dark {
        Color32::from_gray(116)
    } else {
        Color32::from_rgb(122, 130, 144)
    };
    for n in 1..=PERIODS {
        let y = rect.top() + HEADER_H + (n as f32 - 1.0) * row_h;
        if n % 2 == 1 {
            p.rect_filled(
                Rect::from_min_size(Pos2::new(rect.left() + 1.0, y), Vec2::new(width - 2.0, row_h)),
                0.0,
                odd_stripe,
            );
        }
        p.text(
            Pos2::new(rect.left() + 10.0, y + row_h * 0.5),
            Align2::CENTER_CENTER,
            format!("{n}"),
            FontId::proportional(10.0),
            text_pri,
        );
        if let Some((start, end)) = periods.get(n as usize - 1) {
            p.text(
                Pos2::new(rect.left() + 22.0, y + row_h * 0.30),
                Align2::LEFT_CENTER,
                start.as_str(),
                FontId::proportional(8.2),
                text_sec,
            );
            p.text(
                Pos2::new(rect.left() + 22.0, y + row_h * 0.72),
                Align2::LEFT_CENTER,
                end.as_str(),
                FontId::proportional(8.0),
                end_time_col,
            );
        }
        // 午休（第4节后）与晚间（第8节后）加粗分隔线
        let is_section_break = n == 5 || n == 9;
        p.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(
                if is_section_break { 1.2 } else { 0.6 },
                if is_section_break { line_break } else { line_weak },
            ),
        );
    }
    for i in 0..=num_cols {
        let x = rect.left() + GUTTER + i as f32 * col_w;
        p.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(0.6, line_weak),
        );
    }

    let click = resp.clicked().then(|| resp.interact_pointer_pos()).flatten();
    let mut hit = None;

    // 先画非本周课程（若开启灰显），再画本周课程，确保同格冲突时本周课程在最上层
    for pass in 0..2 {
        let want_active = pass == 1;
        if !want_active && !cfg.course_show_non_current {
            continue;
        }
        for (index, row) in rows.iter().enumerate() {
            if !(1..=7).contains(&row.weekday) || row.from == 0 || row.to < row.from {
                continue;
            }
            let active = course::week_applies(&row.weeks, week);
            if active != want_active {
                continue;
            }
            let Some(col_idx) = days.iter().position(|&(_, wd)| wd == row.weekday) else {
                continue;
            };
            let x = rect.left() + GUTTER + col_idx as f32 * col_w;
            let y = rect.top() + HEADER_H + (row.from - 1) as f32 * row_h;
            let h = ((row.to - row.from + 1) as f32 * row_h - 3.0).max(row_h - 3.0);
            let block = Rect::from_min_size(Pos2::new(x + 2.0, y + 1.5), Vec2::new(col_w - 4.0, h));

            let base_col = course_color(&row.name);
            let fill_col = if active {
                if dark {
                    Color32::from_rgba_unmultiplied(
                        base_col.r(),
                        base_col.g(),
                        base_col.b(),
                        (165.0 * alpha_pct).round() as u8,
                    )
                } else {
                    let tinted = crate::paint::mix(skin.bg_card, base_col, 0.22);
                    Color32::from_rgba_unmultiplied(
                        tinted.r(),
                        tinted.g(),
                        tinted.b(),
                        (242.0 * alpha_pct).round() as u8,
                    )
                }
            } else {
                let muted = skin.bg_ctrl;
                Color32::from_rgba_unmultiplied(
                    muted.r(),
                    muted.g(),
                    muted.b(),
                    (if dark { 150.0 } else { 195.0 } * alpha_pct).round() as u8,
                )
            };
            let card_border = if active {
                crate::paint::mix(skin.border, base_col, if dark { 0.45 } else { 0.35 })
            } else {
                skin.border
            };
            p.rect(
                block,
                corner_r,
                fill_col,
                Stroke::new(0.8, card_border),
                egui::StrokeKind::Inside,
            );
            p.rect_filled(
                Rect::from_min_size(block.min, Vec2::new(3.2, block.height())),
                corner_r.min(2.5),
                if active { base_col } else { text_sec },
            );

            let raw_title = if row.name.trim().is_empty() {
                "(未命名)".to_owned()
            } else if active {
                row.name.trim().to_owned()
            } else {
                format!("[非本周]{}", row.name.trim())
            };
            let title = ellipsis(&raw_title, block.width() - 9.0, 10.0);
            let block_title_col = if active {
                if dark { Color32::WHITE } else { Color32::from_rgb(24, 30, 42) }
            } else {
                text_sec
            };
            let block_room_col = if active {
                if dark {
                    Color32::from_rgb(255, 232, 168)
                } else {
                    crate::paint::mix(base_col, Color32::BLACK, 0.42)
                }
            } else {
                text_sec
            };
            let block_teacher_col = if active {
                if dark { Color32::from_white_alpha(210) } else { Color32::from_rgb(80, 90, 106) }
            } else {
                text_sec
            };
            p.text(
                Pos2::new(block.left() + 6.5, block.top() + 4.0),
                Align2::LEFT_TOP,
                title,
                FontId::proportional(10.0),
                block_title_col,
            );
            if !row.room.trim().is_empty() && block.height() > 24.0 {
                p.text(
                    Pos2::new(block.left() + 6.5, block.top() + 18.0),
                    Align2::LEFT_TOP,
                    ellipsis(&format!("@{}", row.room.trim()), block.width() - 9.0, 8.6),
                    FontId::proportional(8.6),
                    block_room_col,
                );
            }
            if !row.teacher.trim().is_empty() && block.height() > 38.0 {
                p.text(
                    Pos2::new(block.left() + 6.5, block.top() + 31.0),
                    Align2::LEFT_TOP,
                    ellipsis(row.teacher.trim(), block.width() - 9.0, 8.2),
                    FontId::proportional(8.2),
                    block_teacher_col,
                );
            }
            if click.is_some_and(|pos| block.contains(pos)) {
                hit = Some(index);
            }
        }
    }

    // 点击空白格时：自动把下方表单定位到「新增课程」并填入所点的星期与节次！
    if hit.is_none() {
        if let Some(pos) = click {
            if pos.x > rect.left() + GUTTER && pos.y > rect.top() + HEADER_H {
                let col_idx = (((pos.x - rect.left() - GUTTER) / col_w).floor() as usize).min(6);
                let period = (((pos.y - rect.top() - HEADER_H) / row_h).floor() as u32 + 1).clamp(1, PERIODS);
                let wd = days[col_idx].1;
                if let Ok(mut slot) = course::editor().lock() {
                    *slot = course::Editor {
                        index: None,
                        name: String::new(),
                        weekday: wd,
                        from: period,
                        to: (period + 1).min(PERIODS),
                        weeks: "1-16".into(),
                        room: String::new(),
                        teacher: String::new(),
                    };
                }
            }
        }
    }

    hit
}

/// Excel 导入：选文件 → 识别 → 预览 → 写入。
///
/// 识别与写盘都在本地完成；文件对话框跑在后台线程（见 `ui::pick_xlsx_async`），
/// 不会再出现点「浏览」整个界面卡住几秒的情况。
fn draw_excel_import(ui: &mut egui::Ui, cfg: &mut config::Config) {
    ui.label(
        egui::RichText::new("支持教务系统导出的 .xlsx：网格表（节次 × 星期）和明细表（一行一条）都能识别；先出预览，确认后再写进课表")
            .size(11.0)
            .color(dim_text(ui)),
    );

    // 后台对话框返回：选到文件后立即自动识别并导入到课表（同时保留预览与结果提示）。
    if let Some(path) = crate::auralsle_ai_status::ui::take_picked_xlsx() {
        if !path.is_empty() {
            cfg.course_xlsx = path.clone();
            config::update(|c| c.course_xlsx = path.clone());
            match course::import_xlsx(std::path::Path::new(&path)) {
                Ok(msg) => {
                    let _ = course::recognize_xlsx(std::path::Path::new(&path));
                    set_week(course::week_now());
                    if let Ok(mut s) = course::import_note().lock() {
                        *s = msg;
                    }
                }
                Err(e) => {
                    if let Ok(mut s) = course::import_note().lock() {
                        *s = format!("识别失败：{e}");
                    }
                }
            }
            ui.ctx().request_repaint();
        }
    }
    if crate::auralsle_ai_status::ui::picking_xlsx() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
        ui.label(egui::RichText::new("正在等待文件选择…").size(11.0).color(dim_text(ui)));
    }

    ui.horizontal_wrapped(|ui| {
        let edit_w = (crate::extras::layout_avail_w(ui) - 172.0).max(150.0);
        ui.add_sized(
            egui::vec2(edit_w, 24.0),
            egui::TextEdit::singleline(&mut cfg.course_xlsx).hint_text("E:\\下载\\课表.xlsx"),
        );
        if ui.button("选择文件…").clicked() {
            crate::auralsle_ai_status::ui::pick_xlsx_async();
        }
        if ui.button("识别").clicked() {
            recognize_xlsx_to(ui, cfg);
        }
    });

    // 识别结果预览
    let mut apply = false;
    let mut cancel = false;
    course::with_preview(|preview| {
        let Some(p) = preview else { return };
        ui.add_space(4.0);
        let preview_w = crate::extras::layout_avail_w(ui);
        let skin = course_skin(ui);
        egui::Frame::new()
            .fill(skin.bg_ctrl)
            .stroke(Stroke::new(1.0, skin.border))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width((preview_w - 20.0).max(160.0));
                ui.label(
                    egui::RichText::new(format!("{} · {} 条课程 · {}", p.file, p.rows.len(), p.kind))
                        .size(11.5)
                        .strong(),
                );
                egui::ScrollArea::vertical()
                    .id_salt("course-excel-preview")
                    .max_height(160.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for r in p.rows.iter().take(60) {
                            let room = if r.room.is_empty() { "—" } else { r.room.as_str() };
                            let teacher = if r.teacher.is_empty() { "" } else { r.teacher.as_str() };
                            ui.label(
                                egui::RichText::new(format!(
                                    "周{} 第{}-{}节 · {}周 · {} · {} {}",
                                    DAYS_MON[(r.weekday.clamp(1, 7) - 1) as usize].0,
                                    r.from,
                                    r.to,
                                    r.weeks,
                                    room,
                                    r.name,
                                    teacher
                                ))
                                .size(11.0),
                            );
                        }
                    });
                ui.horizontal_wrapped(|ui| {
                    if ui.button("✅ 导入到课表").clicked() {
                        apply = true;
                    }
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                });
            });
    });
    if apply {
        let note = match course::apply_preview() {
            Ok(msg) => {
                set_week(course::week_now());
                msg
            }
            Err(e) => format!("导入失败：{e}"),
        };
        if let Ok(mut s) = course::import_note().lock() {
            *s = note;
        }
    }
    if cancel {
        course::clear_preview();
        if let Ok(mut s) = course::import_note().lock() {
            s.clear();
        }
    }

    // 结果提示：失败标红，成功标绿，别让人猜到底有没有导入。
    let note = course::import_note()
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    if !note.is_empty() {
        let failed = note.contains("失败") || note.contains("没识别");
        ui.label(
            egui::RichText::new(note)
                .size(11.5)
                .color(if failed { Color32::from_rgb(235, 96, 96) } else { Color32::from_rgb(86, 196, 142) }),
        );
    }
}

/// 按输入框路径识别一次，结果写进提示区。
fn recognize_xlsx_to(ui: &egui::Ui, cfg: &config::Config) {
    let file = cfg.course_xlsx.trim().to_owned();
    let note = if file.is_empty() {
        "请先选择 .xlsx 文件，或把路径粘到输入框".to_owned()
    } else {
        match course::recognize_xlsx(std::path::Path::new(&file)) {
            Ok(msg) => msg,
            Err(e) => format!("识别失败：{e}"),
        }
    };
    if let Ok(mut s) = course::import_note().lock() {
        *s = note;
    }
    ui.ctx().request_repaint();
}

/// 编辑表单：下拉选课 / 新增 + 六个字段 + 保存 / 删除 / 清空。
fn draw_editor_form(ui: &mut egui::Ui, rows: &[course::CourseRow]) {
    let mut e = course::editor()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    let selected = match e.index {
        Some(i) => rows
            .get(i)
            .map(|r| format!("{} · 周{} 第{}-{}节", r.name, r.weekday, r.from, r.to))
            .unwrap_or_else(|| "＋ 新增课程".to_owned()),
        None => format!("＋ 新增课程（周{} 第{}-{}节）", e.weekday, e.from, e.to),
    };
    let w = (crate::extras::layout_avail_w(ui) - 8.0).max(160.0);
    let mut picked = e.index;
    let prev_picked = picked;
    let mut opts: Vec<(Option<usize>, String)> = Vec::with_capacity(rows.len() + 1);
    opts.push((None, "＋ 新增课程".to_owned()));
    for (i, r) in rows.iter().enumerate() {
        opts.push((
            Some(i),
            format!(
                "{} · 周{} 第{}-{}节 · {}",
                if r.name.is_empty() { "(未命名)" } else { r.name.as_str() },
                r.weekday,
                r.from,
                r.to,
                r.weeks
            ),
        ));
    }
    crate::extras::react_bits_combo(ui, "course-editor-pick", &selected, w, &mut picked, &opts);
    if picked != prev_picked {
        e = match picked {
            None => course::Editor::default(),
            Some(i) => rows.get(i).map(|r| course::Editor::from_row(i, r)).unwrap_or_default(),
        };
    }
    let ed_w = crate::extras::layout_avail_w(ui);
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(ed_w);
        ui.label(egui::RichText::new("名称").size(11.0));
        ui.add(egui::TextEdit::singleline(&mut e.name).desired_width(142.0_f32.min((ed_w - 110.0).max(80.0))));
        ui.label(egui::RichText::new("星期").size(11.0));
        ui.add(egui::DragValue::new(&mut e.weekday).range(1..=7));
    });
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(ed_w);
        ui.label(egui::RichText::new("节次").size(11.0));
        ui.add(egui::DragValue::new(&mut e.from).range(1..=13));
        ui.label(egui::RichText::new("→").size(11.0));
        ui.add(egui::DragValue::new(&mut e.to).range(1..=13));
        ui.label(egui::RichText::new("周次").size(11.0));
        ui.add(egui::TextEdit::singleline(&mut e.weeks).desired_width(68.0).hint_text("1-16 / 1-15单"));
    });
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(ed_w);
        ui.label(egui::RichText::new("教室").size(11.0));
        ui.add(egui::TextEdit::singleline(&mut e.room).desired_width(112.0_f32.min((ed_w * 0.4).max(76.0))));
        ui.label(egui::RichText::new("老师").size(11.0));
        ui.add(egui::TextEdit::singleline(&mut e.teacher).desired_width(76.0));
    });
    let mut save = false;
    let mut remove = false;
    ui.horizontal_wrapped(|ui| {
        save = ui
            .button(if e.index.is_some() { "保存修改" } else { "＋ 新增课程" })
            .clicked();
        remove = e.index.is_some() && ui.button("删除这门课").clicked();
        if ui.button("清空表单").clicked() {
            e = course::Editor::default();
        }
    });
    if let Ok(mut slot) = course::editor().lock() {
        *slot = e.clone();
    }
    if save {
        let mut status = course::import_status().lock().unwrap();
        *status = match course::save_editor() {
            Ok(m) => m,
            Err(err) => err,
        };
    }
    if remove {
        if let Some(i) = e.index {
            let mut status = course::import_status().lock().unwrap();
            *status = match course::delete_row(i) {
                Ok(m) => {
                    if let Ok(mut slot) = course::editor().lock() {
                        *slot = course::Editor::default();
                    }
                    m
                }
                Err(err) => err,
            };
        }
    }
    let status = course::import_status().lock().unwrap();
    if !status.is_empty() {
        ui.label(egui::RichText::new(status.as_str()).size(11.0).color(Color32::from_rgb(86, 196, 142)));
    }
}

