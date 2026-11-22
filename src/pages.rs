//! 五个页面：音乐 → 专注 → 饮水 → 置物 → 待办。
//! 坐标全部是面板局部逻辑点（419×140），文字 y 是基线。

use crate::anim::{self, lerp, smoothstep, Spring};
use crate::app::{App, Cmd, Env, Phase};
use crate::mail;
use crate::paint::{self, capsule, mix, Card, H};
use crate::smtc::Transport;
use crate::theme::{self, size, W};
use eframe::egui::{self, Color32, Pos2, Rect};
use std::f32::consts::{FRAC_PI_2, TAU};

/// 剪贴板分栏一屏显示的条数。
pub(crate) const CLIP_ROWS: usize = 6;
pub(crate) const MUSIC_VOLUME_RECT: Rect = Rect::from_min_max(Pos2::new(345.0,6.0),Pos2::new(389.0,104.0));

fn mmss(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// 由标题生成稳定的封面渐变色。
fn cover_colors(title: &str) -> (Color32, Color32) {
    let mut h: u64 = 1469598103934665603;
    for b in title.bytes() {
        h = (h ^ b as u64).wrapping_mul(1099511628211);
    }
    let palette = [
        (Color32::from_rgb(255, 94, 98), Color32::from_rgb(255, 153, 102)),
        (Color32::from_rgb(86, 204, 242), Color32::from_rgb(47, 128, 237)),
        (Color32::from_rgb(168, 85, 247), Color32::from_rgb(236, 72, 153)),
        (Color32::from_rgb(52, 211, 153), Color32::from_rgb(16, 120, 200)),
        (Color32::from_rgb(251, 191, 36), Color32::from_rgb(239, 68, 68)),
    ];
    palette[(h % palette.len() as u64) as usize]
}

/// 绘制音乐岛播放/暂停按钮：统一应用全局「播放按钮主题」（礁石/涟漪/细砂/晨雾/琉璃/墨屿/双潮/留白/光环等 10 套主题）。
fn paint_music_play_button(
    c: &Card,
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
    hover: f32,
    press: f32,
    playing_t: f32,
    accent: Color32,
    icon_sz: f32,
) {
    let scale = 1.0 - 0.08 * press + 0.03 * hover;
    let p_card = c.scaled(cx, cy, scale);
    let x = cx - w * 0.5;
    let y = cy - h * 0.5;
    let from = crate::skin_paint::shape_count(p_card.ui.ctx(), p_card.painter.layer_id());
    let cfg = crate::auralsle_ai_status::config::get();
    let theme_tt = crate::auralsle_music::TransportTheme::from_setting(&cfg.auralsle_transport_theme);
    let lum = 0.299 * (accent.r() as f32) + 0.587 * (accent.g() as f32) + 0.114 * (accent.b() as f32);
    let on_accent = if lum > 148.0 { Color32::from_rgb(20, 26, 30) } else { Color32::WHITE };
    let tonal = paint::mix(accent, Color32::from_rgb(32, 36, 44), 0.62);
    let p_ink = crate::auralsle_music::draw_transport_button_surface(
        &p_card,
        4,
        crate::auralsle_music::DRect::new(x, y, w, h),
        theme_tt,
        accent,
        on_accent,
        Color32::WHITE,
        tonal,
        paint::mix(accent, Color32::WHITE, 0.22),
        1.0_f32.min(playing_t.max(0.0)),
        hover,
    );
    if playing_t >= 0.88 {
        let bar_w = (icon_sz * 0.25).clamp(2.6, 3.4);
        let bar_h = icon_sz.clamp(9.5, 13.0);
        let gap = (icon_sz * 0.26).clamp(2.8, 3.6);
        let br = bar_w * 0.48;
        p_card.fill(cx - gap * 0.5 - bar_w, cy - bar_h * 0.5, bar_w, bar_h, br, p_ink);
        p_card.fill(cx + gap * 0.5, cy - bar_h * 0.5, bar_w, bar_h, br, p_ink);
    } else {
        paint::play_pause(&p_card, cx, cy, icon_sz, playing_t, p_ink);
    }
    if !cfg!(test) {
        crate::skin_paint::tone_since(p_card.ui.ctx(), p_card.painter.layer_id(), from, |col, _| col);
    }
}

/// 统一绘制应用全局「进度条风格与游标」设置的音乐进度条（适用于所有音乐岛 UI 样式）。
fn paint_themed_music_progress_bar(
    c: &Card,
    bx: f32,
    by: f32,
    bw: f32,
    frac: f32,
    fg: Color32,
    playing: bool,
    time_secs: f64,
) {
    let cfg = crate::auralsle_ai_status::config::get();
    let bar_style = crate::auralsle_music::BarStyle::from_setting(&cfg.auralsle_bar_style);
    let thumb_style = if cfg.auralsle_thumb_enabled {
        crate::auralsle_music::ThumbStyle::from_setting(&cfg.auralsle_thumb_style)
    } else {
        crate::auralsle_music::ThumbStyle::None
    };
    let phase = if playing { ((time_secs * 0.85) % 1.0) as f32 } else { 0.0 };
    let wave = if playing { 1.0 } else { 0.0 };
    let spin = if playing && cfg.auralsle_thumb_spin { ((time_secs * 0.25) % 1.0) as f32 } else { 0.0 };
    let from = crate::skin_paint::shape_count(c.ui.ctx(), c.painter.layer_id());
    crate::auralsle_music::draw_progress_bar(
        c,
        crate::auralsle_music::DRect::new(bx, by - 6.0, bw, 12.0),
        frac,
        phase,
        0.52,
        9.0,
        wave,
        spin,
        fg,
        Color32::from_white_alpha(32),
        paint::alpha(fg, 0.72),
        bar_style,
        thumb_style,
    );
    if !cfg!(test) {
        crate::skin_paint::tone_since(c.ui.ctx(), c.painter.layer_id(), from, |col, _| col);
    }
}

impl App {
    // ================= 音乐 =================
    /// 从当前正在播放的真实专辑封面像素中提取双主色调（左上高光色 + 右下氛围色，自动提升过暗或低饱和像素的明度与饱和度；无封面时回退到歌名哈希色板，需求 13）。
    pub(crate) fn extract_artwork_colors(&self) -> (Color32, Color32) {
        static ART_COL_CACHE: std::sync::OnceLock<std::sync::Mutex<(usize, String, (Color32, Color32))>> = std::sync::OnceLock::new();
        let art_ptr = self.music_artwork.as_ref().map(|(img, _)| std::sync::Arc::as_ptr(img) as usize).unwrap_or(0);
        if !cfg!(test) {
            let mtx = ART_COL_CACHE.get_or_init(|| std::sync::Mutex::new((0, String::new(), (Color32::TRANSPARENT, Color32::TRANSPARENT))));
            if let Ok(g) = mtx.lock() {
                if g.0 == art_ptr && g.1 == self.media.title && g.2.0 != Color32::TRANSPARENT {
                    return g.2;
                }
            }
        }
        let (mut ca, mut cb) = if !self.media.title.is_empty() {
            cover_colors(&self.media.title)
        } else {
            (Color32::from_rgb(56, 189, 248), Color32::from_rgb(168, 85, 247))
        };
        if let Some((img, _)) = &self.music_artwork {
            let [w, h] = img.size;
            if w >= 8 && h >= 8 && img.pixels.len() >= w * h {
                let sample_region = |x0: usize, y0: usize, x1: usize, y1: usize| -> Color32 {
                    let (mut r_sum, mut g_sum, mut b_sum, mut cnt) = (0u64, 0u64, 0u64, 0u64);
                    let step_x = ((x1 - x0) / 6).max(1);
                    let step_y = ((y1 - y0) / 6).max(1);
                    let mut y = y0;
                    while y < y1 {
                        let mut x = x0;
                        while x < x1 {
                            let px = img.pixels[y * w + x];
                            r_sum += px.r() as u64;
                            g_sum += px.g() as u64;
                            b_sum += px.b() as u64;
                            cnt += 1;
                            x += step_x;
                        }
                        y += step_y;
                    }
                    if cnt == 0 {
                        return Color32::from_rgb(86, 204, 242);
                    }
                    let (mut r, mut g, mut b) = (
                        (r_sum / cnt) as f32,
                        (g_sum / cnt) as f32,
                        (b_sum / cnt) as f32,
                    );
                    let max_c = r.max(g).max(b);
                    let min_c = r.min(g).min(b);
                    let avg = (r + g + b) / 3.0;
                    if max_c - min_c < 28.0 {
                        r = (r - avg) * 1.65 + avg;
                        g = (g - avg) * 1.65 + avg;
                        b = (b - avg) * 1.65 + avg;
                    }
                    let boost = if max_c < 135.0 { 145.0 / max_c.max(24.0) } else { 1.0 };
                    Color32::from_rgb(
                        (r * boost).clamp(48.0, 255.0) as u8,
                        (g * boost).clamp(48.0, 255.0) as u8,
                        (b * boost).clamp(48.0, 255.0) as u8,
                    )
                };
                let p1 = sample_region(w / 8, h / 8, w / 2, h / 2);
                let p2 = sample_region(w / 2, h / 2, w * 7 / 8, h * 7 / 8);
                ca = mix(ca, p1, 0.78);
                cb = mix(cb, p2, 0.78);
            }
        }
        if !cfg!(test) {
            if let Some(mtx) = ART_COL_CACHE.get() {
                if let Ok(mut g) = mtx.lock() {
                    *g = (art_ptr, self.media.title.clone(), (ca, cb));
                }
            }
        }
        (ca, cb)
    }
    /// 根据当前播放歌曲的节奏特征、实时播放位置以及当前同步歌词音节密度，计算第 `bar_idx / total_bars` 根频谱柱的旋律律动幅度 `(0.0..=1.0)`。
    pub(crate) fn melody_spectrum_amp(&self, bar_idx: usize, total_bars: usize) -> f32 {
        let np = &self.media;
        if !np.playing || np.title.is_empty() {
            let idle_u = bar_idx as f32 / total_bars.max(1) as f32;
            return 0.12 + 0.06 * (idle_u * std::f32::consts::TAU).sin().abs();
        }
        // 1) 从歌名哈希推导稳定的曲风 BPM (104..138 BPM) 与律动相位
        let mut h: u32 = 2166136261;
        for b in np.title.bytes() {
            h = (h ^ b as u32).wrapping_mul(16777619);
        }
        let bpm = 104.0 + (h % 35) as f32;
        let beat_hz = bpm / 60.0;
        let pos_t = self.lyric_position().max(self.t as f32);

        // 2) 从当前正在演唱的歌词行提取人声能量包络与音节脉冲
        let mut vocal_energy = 0.65_f32;
        let mut syllable_freq = 4.2_f32;
        if let Some(lyrics) = &self.ly.lyrics {
            if let Some(idx) = lyrics.index_at(self.lyric_position()) {
                if let Some(line) = lyrics.lines.get(idx) {
                    let char_cnt = line.text.chars().count();
                    let next_t = lyrics
                        .lines
                        .get(idx + 1)
                        .map(|l| l.t)
                        .unwrap_or((line.t + 4.0).max(np.duration as f32));
                    let dur = (next_t - line.t).clamp(0.8, 8.0);
                    let elapsed = (self.lyric_position() - line.t).max(0.0);
                    let line_u = (elapsed / dur).clamp(0.0, 1.0);
                    // 乐句起音爆发 -> 句中跌宕 -> 句尾收束
                    let phrase_env = (1.0 - (line_u * 2.0 - 0.85).powi(2)).clamp(0.25, 1.0);
                    vocal_energy = if char_cnt == 0 {
                        0.32
                    } else {
                        (0.45 + (char_cnt.min(24) as f32 / 24.0) * 0.55) * phrase_env
                    };
                    syllable_freq = (char_cnt.clamp(3, 18) as f32 / dur).clamp(2.6, 7.5);
                }
            }
        }

        // 3) 分频段声学合成：低频鼓点 (Bass Kick) + 中频人声共振 (Vocal Formant) + 高频泛音 (Treble Shimmer)
        let u = bar_idx as f32 / (total_bars.saturating_sub(1).max(1) as f32);
        let beat_ang = pos_t * beat_hz * std::f32::consts::TAU;
        let kick = (beat_ang.sin().max(0.0)).powi(3);
        let sub_bass = ((beat_ang * 0.5 + bar_idx as f32 * 0.35).cos() * 0.5 + 0.5).powi(2);
        let vocal_wave = ((pos_t * syllable_freq * std::f32::consts::TAU + bar_idx as f32 * 0.82).sin() * 0.5 + 0.5)
            * vocal_energy;
        let harmonic = ((pos_t * (beat_hz * 2.0) * std::f32::consts::TAU - bar_idx as f32 * 1.15).sin() * 0.5 + 0.5)
            * (0.4 + 0.6 * vocal_energy);

        // 中心略高、两端灵动的自然声场曲线
        let center_bell = 1.0 - ((u - 0.45).abs() * 1.15).powi(2) * 0.42;
        let raw = if u < 0.28 {
            0.22 + 0.52 * kick + 0.26 * sub_bass
        } else if u < 0.72 {
            0.20 + 0.56 * vocal_wave + 0.24 * kick
        } else {
            0.18 + 0.50 * harmonic + 0.32 * vocal_wave
        };
        (raw * center_bell).clamp(0.14, 1.0)
    }

    /// 统一获取当前音乐页面应展示的实时歌词文本（修复图六红框显示固定占位词的问题）。
    pub(crate) fn current_music_lyric_text(&self) -> (String, f32) {
        let np = &self.media;
        if np.title.is_empty() {
            return ("♪ 播放音乐后在此处同步滚动显示歌词".to_owned(), 1.0);
        }
        if let Some(lyrics) = &self.ly.lyrics {
            if !lyrics.lines.is_empty() {
                let pos = self.lyric_position();
                let idx = if lyrics.synced {
                    lyrics.index_at(pos).unwrap_or(0)
                } else {
                    0
                };
                let anim_idx = self
                    .clipboard
                    .is_enabled()
                    .then_some(idx as f32)
                    .unwrap_or(idx as f32);
                let _ = anim_idx;
                if let Some(line) = lyrics.lines.get(idx) {
                    let t = line.text.trim();
                    if !t.is_empty() {
                        return (t.to_owned(), 1.0);
                    }
                }
                if let Some(first_non_empty) = lyrics.lines.iter().find(|l| !l.text.trim().is_empty()) {
                    return (first_non_empty.text.trim().to_owned(), 0.9);
                }
            }
        }
        if self.ly.loading {
            return ("正在同步当前歌曲歌词…".to_owned(), 0.85);
        }
        if let Some(err) = &self.ly.error {
            return (format!("{} · 点击此处重新匹配歌词", err.label()), 0.85);
        }
        if !np.artist.is_empty() {
            (format!("♪ {} — {}", np.title, np.artist), 0.92)
        } else {
            (format!("♪ {}", np.title), 0.92)
        }
    }

    /// [eIsland × React Bits] 灵动频谱模式（修复图三：取消封面外部白色遮罩与 LIVE 标签、取消底部左右两枚按钮、声波跟随音乐旋律律动）：
    fn page_music_eisland(&mut self, c: &Card, _env: &Env, cmds: &mut Vec<Cmd>) {
        let np = self.media.clone();
        let has = !np.title.is_empty();
        let ctx = c.ui.ctx().clone();
        let playing_t = ctx.animate_bool_with_time(egui::Id::new("np-playing-eisland"), np.playing, 0.24);
        let cur_local = self.cursor_local;

        let (accent_a, accent_b) = if has {
            self.extract_artwork_colors()
        } else {
            (Color32::from_rgb(56, 189, 248), Color32::from_rgb(139, 92, 246))
        };

        // 1) 自适应双径向流光环境背光（奶油主题下取消背景跟随专辑封面取色渲染）
        if cfg!(test) || self.settings.skin != crate::model::Skin::Cream {
            c.glow(76.0, 58.0, 74.0, paint::alpha(accent_a, 0.14 + 0.14 * playing_t));
            c.glow(310.0, 62.0, 88.0, paint::alpha(accent_b, 0.10 + 0.12 * playing_t));
        }

        // 2) 左舱（优化图四 `image_aa0223.jpg` 专辑图与左下布局）：
        // 80×80 高清圆角专辑封面 + 右侧微露黑胶唱盘边缘 + 下方配套「♪ 岛词」状态胶囊，使左舱与右舱底部 (y=127) 严格齐平！
        let (art_x, art_y, art_s) = (38.0, 20.0, 78.0);
        // 已移除右侧微露黑胶边，避免与右侧信息舱左缘及歌名重叠
        if let Some((image, texture)) = &self.music_artwork {
            let [w, h] = image.size;
            let (uw, vh) = if w > h { (h as f32 / w as f32, 1.0) } else { (1.0, w as f32 / h as f32) };
            let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), egui::vec2(uw, vh));
            c.painter.add(
                egui::epaint::RectShape::filled(c.rect(art_x, art_y, art_s, art_s), 14.0 * c.s, Color32::WHITE)
                    .with_texture(texture.id(), uv),
            );
        } else {
            let cc = c.clipped(art_x, art_y, art_s, art_s);
            cc.vgrad(art_x, art_y, art_s, art_s, 14.0, accent_a, accent_b);
            paint::note(&cc, art_x + art_s * 0.5, art_y + art_s * 0.5, art_s * 0.38, Color32::from_white_alpha(240));
        }
        if has && !cfg!(test) && c.hit(art_x, art_y, art_s, art_s, "eisland-art-open").on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("点击跳转到音乐软件").clicked() {
            crate::auralsle_music::focus_player_source(&np.source);
        }
        // 封面左下配套「♪ 岛词」微胶囊 (x=38, y=104, w=78, h=22)，与右侧播控坞等高对齐
        let il_on = self.settings.island_lyrics;
        let (il_r, il_c) = capsule(
            c,
            38.0,
            103.5,
            78.0,
            22.0,
            "ei-island-lyric-btn",
            if il_on { paint::alpha(accent_a, 0.24) } else { Color32::from_white_alpha(12) },
            Color32::from_white_alpha(24),
        );
        if il_on {
            c.stroke(38.0, 103.5, 78.0, 22.0, 11.0, 0.8, paint::alpha(accent_a, 0.65));
        }
        il_c.text(
            77.0,
            118.0,
            if il_on { "♪ 岛词已开" } else { "♪ 岛上歌词" },
            8.4,
            W::Semibold,
            if il_on { theme::WHITE } else { Color32::from_gray(180) },
            H::Center,
        );
        if il_r.on_hover_text("切换收起状态下的灵动岛桌面歌词").clicked() {
            self.settings.island_lyrics = !self.settings.island_lyrics;
            self.settings.save();
        }

        // 3) 右上舱：内嵌音律与旋律频谱矩阵卡 (x=122, y=20, w=258, h=54)
        let (card_x, card_y, card_w, card_h) = (122.0, 20.0, 258.0, 54.0);
        c.fill(card_x, card_y, card_w, card_h, 11.5, Color32::from_white_alpha(12));
        c.stroke(card_x, card_y, card_w, card_h, 11.5, 0.8, Color32::from_white_alpha(20));

        // 右上舱右侧：14 柱跟随音乐旋律与人声律动的频谱画布 (x=298..373)
        let eq_x0 = 298.0;
        let eq_cy = card_y + card_h * 0.5;
        for i in 0..14 {
            let target_amp = self.melody_spectrum_amp(i, 14);
            let amp = ctx.animate_value_with_time(egui::Id::new(("melody-ei-bar", i)), target_amp, 0.085);
            let bh = (36.0 * amp).clamp(4.0, 36.0);
            let bx = eq_x0 + i as f32 * 5.2;
            let bar_col = mix(accent_a, accent_b, i as f32 / 13.0);
            c.fill(bx, eq_cy - bh * 0.5, 3.2, bh, 1.6, bar_col);
            let peak_y = (eq_cy - bh * 0.5 - 2.4).max(card_y + 4.0);
            c.circle(bx + 1.6, peak_y, 1.15, paint::alpha(Color32::WHITE, 0.45 + 0.45 * amp));
        }

        // 右上舱左侧：歌名、歌手与同步歌词 (x=130..292)
        let info_x = 130.0;
        let info_w = 162.0;
        if !has {
            c.text(info_x, 38.0, "灵动频谱 · Audio Matrix", 12.8, W::Bold, theme::WHITE, H::Left);
            c.text(info_x, 52.5, "播放音乐后频谱随音乐旋律实时律动", 9.4, W::Semibold, mix(accent_a, Color32::WHITE, 0.4), H::Left);
            c.text(info_x, 66.0, "♪ 支持封面流光染色与双轨同步歌词", 8.8, W::Regular, theme::GRAY, H::Left);
        } else {
            c.text(info_x, 36.0, &c.fit(&np.title, 12.8, W::Bold, info_w), 12.8, W::Bold, theme::WHITE, H::Left);
            let artist_line = if np.artist.is_empty() { "未知艺术家".to_owned() } else { np.artist.clone() };
            c.text(info_x, 49.5, &c.fit(&artist_line, 9.4, W::Semibold, info_w), 9.4, W::Semibold, mix(accent_a, Color32::WHITE, 0.35), H::Left);
            let (lyric_line, lyric_alpha) = self.current_music_lyric_text();
            c.text(
                info_x,
                65.0,
                &c.fit(&lyric_line, 9.6, W::Bold, info_w),
                9.6,
                W::Bold,
                paint::alpha(Color32::WHITE, lyric_alpha),
                H::Left,
            );
            if self.ly.error.is_some() && c.hit(info_x, 54.0, info_w, 16.0, "ei-lyric-retry").clicked() {
                self.ly.key.clear();
                self.ly.manual_retry = true;
            }
        }

        // 4) 右下舱：流光进度轨 (x=122, y=78, w=258, h=19)
        let (bx, bw, by) = (156.0, 190.0, 87.5);
        c.fill(122.0, 78.0, 258.0, 19.0, 9.5, Color32::from_white_alpha(11));
        let bar = c.hit_drag(bx - 4.0, by - 8.0, bw + 8.0, 16.0, "m-seek-eisland");
        let seek_position = if has && np.duration > 0.0 && (bar.is_pointer_button_down_on() || bar.clicked() || bar.drag_stopped()) {
            bar.interact_pointer_pos()
                .or_else(|| ctx.input(|i| i.pointer.interact_pos()))
                .map(|pointer| ((c.to_local(pointer).x - bx) / bw).clamp(0.0, 1.0) as f64 * np.duration)
        } else {
            None
        };
        let position = seek_position.unwrap_or(np.position);
        let frac = if np.duration > 0.0 { (position / np.duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let (bh, bpress) = c.hover_t(&bar);
        let th = lerp(4.2, 6.2, if has { bh.max(bpress) } else { 0.0 });
        let _ = (bh, bpress, th);
        paint_themed_music_progress_bar(c, bx, by, bw, frac, mix(accent_a, accent_b, 0.35), np.playing, self.t);
        if bar.clicked() || bar.drag_stopped() {
            if let Some(pos) = seek_position {
                cmds.push(Cmd::Media(Transport::Seek(pos)));
            }
        }
        let tc = if has { Color32::from_gray(210) } else { Color32::from_gray(115) };
        c.text(128.0, 91.0, &mmss(position), 8.6, W::Semibold, tc, H::Left);
        let rem = if has && np.duration <= 0.0 { "--:--".into() } else { format!("-{}", mmss((np.duration - position).max(0.0))) };
        c.text(374.0, 91.0, &rem, 8.6, W::Semibold, tc, H::Right);

        // 5) 底部全宽一体式磨砂播控坞 (x=122, y=103, w=258, h=23，与上方信息舱及进度轨等宽对齐，优化图四 `image_aa0223.jpg` 播放控件)
        let y = 114.5;
        let ic = if has { theme::WHITE } else { Color32::from_rgb(108, 112, 122) };
        c.fill(122.0, y - 11.5, 258.0, 23.0, 11.5, Color32::from_white_alpha(12));
        c.stroke(122.0, y - 11.5, 258.0, 23.0, 11.5, 0.8, Color32::from_white_alpha(20));

        // 播控坞左端：歌词重匹按钮 [词] (x = 128..158)
        let lyr_r = c.hit(126.0, y - 11.0, 32.0, 22.0, "m-lyr-ei");
        let (lh, _) = c.hover_t(&lyr_r);
        if lh > 0.01 {
            c.fill(128.0, y - 8.5, 28.0, 17.0, 8.5, crate::auralsle_music::music_pause_pill_fill(accent_a, lh, false));
        }
        c.text(142.0, y + 3.6, "歌词", 8.2, W::Semibold, if self.ly.lyrics.is_some() { mix(accent_a, Color32::WHITE, 0.5) } else { theme::GRAY }, H::Center);
        if lyr_r.on_hover_text("点击重新匹配同步当前歌曲歌词").clicked() && has {
            self.ly.key.clear();
            self.ly.manual_retry = true;
        }

        // 播控坞中央：对称磁吸三联播控 [ ⏮ (194)   ⏯ (242)   ⏭ (290) ]
        let prev_r = c.hit(176.0, y - 11.5, 36.0, 23.0, "m-prev-ei");
        let (ph, pp) = c.hover_t(&prev_r);
        let pmag = crate::auralsle_ai_status::ui::magnet_offset(cur_local, Pos2::new(194.0, y), 24.0, 2.8);
        if ph > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(ph), 194.0 + pmag.x, y + pmag.y, 10.0, accent_a, ph, false); }
        paint::skip(&c.scaled(194.0 + pmag.x, y + pmag.y, 1.0 - 0.1 * pp), 194.0 + pmag.x, y + pmag.y, -1.0, ic);

        let play_resp = c.hit(216.0, y - 11.5, 52.0, 23.0, "m-play-ei");
        let (play_hov, play_press) = c.hover_t(&play_resp);
        let c_mag = crate::auralsle_ai_status::ui::magnet_offset(cur_local, Pos2::new(242.0, y), 26.0, 2.6);
        paint_music_play_button(c, 242.0 + c_mag.x, y + c_mag.y, 48.0, 19.0, play_hov, play_press, playing_t, accent_a, 11.2);

        let next_r = c.hit(272.0, y - 11.5, 36.0, 23.0, "m-next-ei");
        let (nh, np_p) = c.hover_t(&next_r);
        let nmag = crate::auralsle_ai_status::ui::magnet_offset(cur_local, Pos2::new(290.0, y), 24.0, 2.8);
        if nh > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(nh), 290.0 + nmag.x, y + nmag.y, 10.0, accent_a, nh, false); }
        paint::skip(&c.scaled(290.0 + nmag.x, y + nmag.y, 1.0 - 0.1 * np_p), 290.0 + nmag.x, y + nmag.y, 1.0, ic);

        if has {
            if prev_r.clicked() { cmds.push(Cmd::Media(Transport::Prev)); }
            if play_resp.clicked() { cmds.push(Cmd::Media(Transport::Toggle)); }
            if next_r.clicked() { cmds.push(Cmd::Media(Transport::Next)); }
        }

        // 播控坞右端：内嵌音量喇叭与百分比 (x = 334..376)
        let vol_r = c.hit(332.0, y - 11.5, 44.0, 23.0, "m-vol-ei");
        let (vh, vp) = c.hover_t(&vol_r);
        if vh > 0.01 { c.fill(334.0, y - 8.5, 40.0, 17.0, 8.5, crate::auralsle_music::music_pause_pill_fill(accent_a, vh, false)); }
        let (volume, muted) = self.music_volume_ui.unwrap_or((100, false));
        paint::speaker(&c.scaled(346.0, y, 1.0 - 0.08 * vp), 346.0, y, muted, ic);
        c.text(364.0, y + 3.4, &format!("{volume}%"), 7.6, W::Semibold, Color32::from_gray(205), H::Center);
        if vol_r.clicked() && self.music_volume_ui.is_some() {
            self.music_volume_open = !self.music_volume_open;
        }
        if vol_r.hovered() && self.music_volume_ui.is_some() {
            let delta = c.ui.ctx().input(|i| i.raw_scroll_delta.y);
            if delta.abs() > 0.5 {
                self.change_music_volume(Transport::SetVolume((volume as f32 + delta / 120.0 * 4.0).round().clamp(0.0, 100.0) as u32), cmds);
            }
        }
    }

    /// 音乐岛样式 2：黑胶唱片机 (Vinyl Studio Deck) — 修复图四：
    /// 取消外部方框遮罩与粗白圈，直接绘制与原白色圆同等大小（直径 52px，半径 26px）的大圆形专辑图 + 唱臂 + 旋律电平表 + 精简底部播控
    #[allow(dead_code)]
    fn page_music_vinyl(&mut self, c: &Card, _env: &Env, cmds: &mut Vec<Cmd>) {
        let np = self.media.clone();
        let has = !np.title.is_empty();
        let ctx = c.ui.ctx().clone();
        let playing_t = ctx.animate_bool_with_time(egui::Id::new("np-playing-vinyl"), np.playing, 0.26);
        let cur_local = self.cursor_local;
        let (accent_a, accent_b) = if has {
            cover_colors(&np.title)
        } else {
            (Color32::from_rgb(245, 158, 11), Color32::from_rgb(239, 68, 68))
        };

        if cfg!(test) || self.settings.skin != crate::model::Skin::Cream {
            c.glow(76.0, 58.0, 72.0, paint::alpha(accent_a, 0.14 + 0.14 * playing_t));
        }

        // 1) 左舱：直接绘制一个大号圆形专辑图（支持「圆形专辑图旋转」开关控制 33⅓ RPM 真纹理旋转，需求 12）
        let (vcx, vcy, art_r) = (74.0_f32, 58.0_f32, 26.0_f32);
        let rot_on = crate::auralsle_ai_status::config::get().music_round_art_rotate;
        c.circle(vcx, vcy, 31.0, Color32::from_rgb(16, 17, 22));
        c.ring(vcx, vcy, 29.2, 0.7, Color32::from_white_alpha(20));
        self.draw_circular_album_art(c, vcx, vcy, art_r, rot_on, accent_a, accent_b);

        // 精密唱臂（播放时平滑摆入唱盘外沿，暂停时归位）
        let arm_pivot = (111.0_f32, 26.0_f32);
        let arm_elbow = (lerp(114.0, 107.0, playing_t), lerp(48.0, 50.0, playing_t));
        let arm_tip = (lerp(112.0, 98.5, playing_t), lerp(66.0, 64.0, playing_t));
        c.circle(arm_pivot.0, arm_pivot.1, 4.0, Color32::from_gray(175));
        c.circle(arm_pivot.0, arm_pivot.1, 1.8, Color32::from_gray(90));
        c.seg(arm_pivot, arm_elbow, 1.8, Color32::from_gray(220));
        c.seg(arm_elbow, arm_tip, 1.6, Color32::from_gray(220));
        c.circle(arm_tip.0, arm_tip.1, 2.3, accent_a);

        // 2) 右上舱：黑胶录音室信息与同步歌词舱 (x=124, y=20, w=256, h=54)
        let (rc_x, rc_y, rc_w, rc_h) = (124.0, 20.0, 256.0, 54.0);
        c.fill(rc_x, rc_y, rc_w, rc_h, 11.5, Color32::from_white_alpha(12));
        c.stroke(rc_x, rc_y, rc_w, rc_h, 11.5, 0.8, Color32::from_white_alpha(20));

        // 右上角 33⅓ RPM 金标胶囊 + 8 柱旋律 VU 电平表
        c.fill(322.0, 24.0, 52.0, 14.0, 7.0, paint::alpha(accent_a, 0.22));
        c.stroke(322.0, 24.0, 52.0, 14.0, 7.0, 0.7, paint::alpha(accent_a, 0.60));
        c.text(348.0, 33.8, "33⅓ RPM", 7.5, W::Bold, accent_a, H::Center);
        for vi in 0..8 {
            let target_v = self.melody_spectrum_amp(vi, 8);
            let vamp = ctx.animate_value_with_time(egui::Id::new(("melody-vn-bar", vi)), target_v, 0.085);
            let vh = (20.0 * vamp).clamp(3.0, 20.0);
            let vx = 326.0 + vi as f32 * 6.0;
            c.fill(vx, 67.0 - vh, 3.6, vh, 1.5, mix(accent_a, accent_b, vi as f32 / 7.0));
        }

        let info_x = 132.0;
        let info_w = 184.0;
        if !has {
            c.text(info_x, 36.0, "黑胶唱片机 · Vinyl Studio", 12.8, W::Bold, theme::WHITE, H::Left);
            c.text(info_x, 50.0, "33⅓ RPM 圆形专辑唱盘与旋律电平", 9.4, W::Semibold, accent_a, H::Left);
            c.text(info_x, 64.5, "♪ 播放音乐后唱臂落槽并同步滚动歌词", 8.8, W::Regular, theme::GRAY, H::Left);
        } else {
            c.text(info_x, 35.5, &c.fit(&np.title, 12.8, W::Bold, info_w), 12.8, W::Bold, theme::WHITE, H::Left);
            let artist_line = if np.artist.is_empty() { "未知艺术家".to_owned() } else { np.artist.clone() };
            c.text(info_x, 49.0, &c.fit(&artist_line, 9.4, W::Semibold, info_w), 9.4, W::Semibold, accent_a, H::Left);
            let (cur_l, l_alpha) = self.current_music_lyric_text();
            c.text(info_x, 64.5, &c.fit(&cur_l, 9.6, W::Bold, info_w), 9.6, W::Bold, paint::alpha(Color32::WHITE, l_alpha), H::Left);
        }

        // 3) 右下舱：唱针进度轨 (x=124, y=78, w=256, h=19)
        c.fill(124.0, 78.0, 256.0, 19.0, 9.5, Color32::from_white_alpha(11));
        let (bx, bw, by) = (158.0, 188.0, 87.5);
        let bar = c.hit_drag(bx - 4.0, by - 8.0, bw + 8.0, 16.0, "m-seek-vinyl");
        let seek_position = if has && np.duration > 0.0 && (bar.is_pointer_button_down_on() || bar.clicked() || bar.drag_stopped()) {
            bar.interact_pointer_pos()
                .or_else(|| ctx.input(|i| i.pointer.interact_pos()))
                .map(|pointer| ((c.to_local(pointer).x - bx) / bw).clamp(0.0, 1.0) as f64 * np.duration)
        } else {
            None
        };
        let position = seek_position.unwrap_or(np.position);
        let frac = if np.duration > 0.0 { (position / np.duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let (bh, bpress) = c.hover_t(&bar);
        let th = lerp(4.0, 6.0, if has { bh.max(bpress) } else { 0.0 });
        c.fill(bx, by - th * 0.5, bw, th, th * 0.5, Color32::from_white_alpha(26));
        if frac > 0.0 {
            let pw = (bw * frac).max(th);
            c.fill(bx, by - th * 0.5, pw, th, th * 0.5, accent_a);
            c.circle(bx + pw, by, lerp(4.0, 5.6, bh.max(bpress)), Color32::WHITE);
        }
        if (bar.clicked() || bar.drag_stopped()) && seek_position.is_some() {
            cmds.push(Cmd::Media(Transport::Seek(seek_position.unwrap())));
        }
        c.text(130.0, 91.0, &mmss(position), 8.6, W::Semibold, Color32::from_gray(205), H::Left);
        let rem = if has && np.duration <= 0.0 { "--:--".into() } else { format!("-{}", mmss((np.duration - position).max(0.0))) };
        c.text(374.0, 91.0, &rem, 8.6, W::Semibold, Color32::from_gray(205), H::Right);

        // 4) 底部居中黑胶控制坞 + 音量键（已取消左右两枚冗余按钮）
        let y = 114.5;
        let ic = if has { theme::WHITE } else { Color32::from_rgb(108, 112, 122) };
        c.fill(126.0, y - 12.0, 166.0, 24.0, 12.0, Color32::from_white_alpha(12));
        let prev_r = c.hit(134.0, y - 12.0, 36.0, 24.0, "m-prev-vn");
        let (ph, pp) = c.hover_t(&prev_r);
        let pmag = crate::auralsle_ai_status::ui::magnet_offset(cur_local, Pos2::new(152.0, y), 24.0, 3.0);
        if ph > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(ph), 152.0 + pmag.x, y + pmag.y, 10.5, accent_a, ph, false); }
        paint::skip(&c.scaled(152.0 + pmag.x, y + pmag.y, 1.0 - 0.1 * pp), 152.0 + pmag.x, y + pmag.y, -1.0, ic);

        let play_r = c.hit(180.0, y - 12.0, 58.0, 24.0, "m-play-vn");
        let (play_hov, play_press) = c.hover_t(&play_r);
        let c_mag = crate::auralsle_ai_status::ui::magnet_offset(cur_local, Pos2::new(209.0, y), 26.0, 2.8);
        paint_music_play_button(c, 209.0 + c_mag.x, y + c_mag.y, 52.0, 20.0, play_hov, play_press, playing_t, accent_a, 12.5);

        let next_r = c.hit(248.0, y - 12.0, 36.0, 24.0, "m-next-vn");
        let (nh, np_p) = c.hover_t(&next_r);
        let nmag = crate::auralsle_ai_status::ui::magnet_offset(cur_local, Pos2::new(266.0, y), 24.0, 3.0);
        if nh > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(nh), 266.0 + nmag.x, y + nmag.y, 10.5, accent_a, nh, false); }
        paint::skip(&c.scaled(266.0 + nmag.x, y + nmag.y, 1.0 - 0.1 * np_p), 266.0 + nmag.x, y + nmag.y, 1.0, ic);

        if has {
            if prev_r.clicked() { cmds.push(Cmd::Media(Transport::Prev)); }
            if play_r.clicked() { cmds.push(Cmd::Media(Transport::Toggle)); }
            if next_r.clicked() { cmds.push(Cmd::Media(Transport::Next)); }
        }

        let vol_r = c.hit(355.0, y - 11.0, 24.0, 22.0, "m-vol-vn");
        let (vh, vp) = c.hover_t(&vol_r);
        if vh > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(vh), 367.0, y, 10.5, accent_a, vh, false); }
        let (_volume, muted) = self.music_volume_ui.unwrap_or((100, false));
        paint::speaker(&c.scaled(367.0, y, 1.0 - 0.08 * vp), 367.0, y, muted, ic);
        if vol_r.clicked() && self.music_volume_ui.is_some() {
            self.music_volume_open = !self.music_volume_open;
        }
    }

    /// 音乐岛样式 3：全宽旋律声波画布 (Studio Waveform) — 居中大歌词 + 贯穿式 28 柱跟随音乐旋律动态声波进度条
    #[allow(dead_code)]
    fn page_music_waveform(&mut self, c: &Card, _env: &Env, cmds: &mut Vec<Cmd>) {
        let np = self.media.clone();
        let has = !np.title.is_empty();
        let ctx = c.ui.ctx().clone();
        let playing_t = ctx.animate_bool_with_time(egui::Id::new("np-playing-wave"), np.playing, 0.22);
        let (accent_a, accent_b) = if has {
            cover_colors(&np.title)
        } else {
            (Color32::from_rgb(56, 189, 248), Color32::from_rgb(168, 85, 247))
        };

        // 1) 顶栏：纯净专辑封面（无外圈白色遮罩）+ 歌名/歌手
        let (art_x, art_y, art_s) = (44.0, 20.0, 28.0);
        if let Some((image, texture)) = &self.music_artwork {
            let [w, h] = image.size;
            let (uw, vh) = if w > h { (h as f32 / w as f32, 1.0) } else { (1.0, w as f32 / h as f32) };
            let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), egui::vec2(uw, vh));
            c.painter.add(
                egui::epaint::RectShape::filled(c.rect(art_x, art_y, art_s, art_s), 7.0 * c.s, Color32::WHITE)
                    .with_texture(texture.id(), uv),
            );
        } else {
            c.fill(art_x, art_y, art_s, art_s, 7.0, accent_a);
            paint::note(c, art_x + 14.0, art_y + 14.0, 12.0, Color32::WHITE);
        }
        if has && !cfg!(test) && c.hit(art_x, art_y, art_s, art_s, "waveform-art-open").on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("点击跳转到音乐软件").clicked() {
            crate::auralsle_music::focus_player_source(&np.source);
        }
        let header_title = if has {
            if np.artist.is_empty() {
                np.title.clone()
            } else {
                format!("{} · {}", np.title, np.artist)
            }
        } else {
            "全宽声波画布 · 暂无音乐".to_owned()
        };
        c.text(80.0, 38.0, &c.fit(&header_title, 12.2, W::Bold, 220.0), 12.2, W::Bold, theme::WHITE, H::Left);

        // 2) 中央大字高亮实时歌词
        let (lyric_line, _) = self.current_music_lyric_text();
        c.text(209.5, 60.0, &c.fit(&lyric_line, 11.8, W::Bold, 326.0), 11.8, W::Bold, mix(accent_a, Color32::WHITE, 0.45), H::Center);

        // 3) 贯穿式 28 柱跟随音乐旋律动态声波进度条
        let (wx, ww, wy_mid) = (44.0, 331.0, 84.0);
        let wave_hit = c.hit_drag(wx, wy_mid - 14.0, ww, 28.0, "m-seek-waveform");
        let seek_position = if has && np.duration > 0.0 && (wave_hit.is_pointer_button_down_on() || wave_hit.clicked() || wave_hit.drag_stopped()) {
            wave_hit
                .interact_pointer_pos()
                .or_else(|| ctx.input(|i| i.pointer.interact_pos()))
                .map(|pointer| ((c.to_local(pointer).x - wx) / ww).clamp(0.0, 1.0) as f64 * np.duration)
        } else {
            None
        };
        let position = seek_position.unwrap_or(np.position);
        let frac = if np.duration > 0.0 { (position / np.duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
        if (wave_hit.clicked() || wave_hit.drag_stopped()) && seek_position.is_some() {
            cmds.push(Cmd::Media(Transport::Seek(seek_position.unwrap())));
        }

        let bars = 28usize;
        let step = ww / bars as f32;
        for i in 0..bars {
            let u = (i as f32 + 0.5) / bars as f32;
            let target_amp = self.melody_spectrum_amp(i, bars);
            let wave_amp = ctx.animate_value_with_time(egui::Id::new(("melody-wf-bar", i)), target_amp, 0.085);
            let bh = (24.0 * wave_amp).clamp(4.0, 24.0);
            let bx = wx + i as f32 * step + (step - 5.2) * 0.5;
            let col = if u <= frac {
                mix(accent_a, accent_b, u)
            } else {
                Color32::from_white_alpha(38)
            };
            c.fill(bx, wy_mid - bh * 0.5, 4.8, bh, 2.4, col);
        }

        // 4) 底部播控与时间戳
        let y = 115.5;
        let ic = if has { theme::WHITE } else { Color32::from_rgb(96, 98, 106) };
        c.text(44.0, y + 4.0, &mmss(position), 9.2, W::Semibold, theme::GRAY, H::Left);
        let rem = if has && np.duration <= 0.0 { "--:--".into() } else { format!("-{}", mmss((np.duration - position).max(0.0))) };
        c.text(375.0, y + 4.0, &rem, 9.2, W::Semibold, theme::GRAY, H::Right);

        let prev_r = c.hit(148.0, y - 12.0, 28.0, 24.0, "m-prev-wf");
        let (ph, pp) = c.hover_t(&prev_r);
        if ph > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(ph), 162.0, y, 10.5, accent_a, ph, false); }
        if prev_r.clicked() && has {
            cmds.push(Cmd::Media(Transport::Prev));
        }
        paint::skip(&c.scaled(162.0, y, 1.0 - 0.08 * pp), 162.0, y, -1.0, ic);

        let play_r = c.hit(190.0, y - 13.0, 40.0, 26.0, "m-play-wf");
        let (p_hov, p_press) = c.hover_t(&play_r);
        paint_music_play_button(c, 210.0, y, 38.0, 22.0, p_hov, p_press, playing_t, accent_a, 12.5);
        if play_r.clicked() && has {
            cmds.push(Cmd::Media(Transport::Toggle));
        }

        let next_r = c.hit(244.0, y - 12.0, 28.0, 24.0, "m-next-wf");
        let (nh, np_p) = c.hover_t(&next_r);
        if nh > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(nh), 258.0, y, 10.5, accent_a, nh, false); }
        if next_r.clicked() && has {
            cmds.push(Cmd::Media(Transport::Next));
        }
        paint::skip(&c.scaled(258.0, y, 1.0 - 0.08 * np_p), 258.0, y, 1.0, ic);
    }

    /// 音乐岛样式 4：Fly流光卡（移植自 flyl1u/fly-island 流光极光媒体卡片，全新流光渲染架构）：
    /// - 封面色彩提取 + 动态流光极光网格底板（Dual Orbiting Aurora Glow）+ 360° 环绕流光彗星边框；
    /// - 左侧悬浮封面（随旋律律动呼吸外环，无白色遮罩）+ 右上 16 柱旋律流光频谱带；
    /// - 中央磨砂流光逐字卡拉 OK 歌词舱（主句逐字点亮 + 副句预览，点击可刷新歌词）；
    /// - 底部流光彗星进度条（带 paint::chase 渐隐光尾）与全功能磨砂播控坞。
    fn page_music_fly_island(&mut self, c: &Card, _env: &Env, cmds: &mut Vec<Cmd>) {
        let np = self.media.clone();
        let has = !np.title.is_empty();
        let ctx = c.ui.ctx().clone();
        let playing_t = ctx.animate_bool_with_time(egui::Id::new("np-playing-fly"), np.playing, 0.20);
        if np.playing {
            ctx.request_repaint();
        }
        let (accent_a, accent_b) = if has {
            self.extract_artwork_colors()
        } else {
            (Color32::from_rgb(56, 189, 248), Color32::from_rgb(168, 85, 247))
        };

        let phase = (self.t as f32) * if np.playing { 1.65 } else { 0.35 };
        let (cx_card, cy_card, cw_card, ch_card, r_card) = (36.0_f32, 8.0_f32, 348.0_f32, 126.0_f32, 15.0_f32);

        // 1) 流光卡底层深色玻璃底座 + 双极光动态流光球 (Aurora Streaming Mesh)
        c.fill(cx_card, cy_card, cw_card, ch_card, r_card, Color32::from_rgba_unmultiplied(16, 18, 26, 228));
        if cfg!(test) || self.settings.skin != crate::model::Skin::Cream {
            let inner = c.clipped(cx_card, cy_card, cw_card, ch_card);
            inner.hgrad(
                cx_card,
                cy_card,
                cw_card,
                ch_card,
                r_card,
                paint::alpha(accent_a, 0.26 + 0.08 * playing_t),
                paint::alpha(accent_b, 0.24 + 0.08 * playing_t),
            );
            let orb1_x = cx_card + 78.0 + (phase * 0.9).cos() * 46.0;
            let orb1_y = cy_card + 42.0 + (phase * 1.1).sin() * 24.0;
            let orb2_x = cx_card + 268.0 + (phase * 0.8 + 1.8).sin() * 52.0;
            let orb2_y = cy_card + 86.0 + (phase * 1.0 + 0.9).cos() * 22.0;
            inner.glow(orb1_x, orb1_y, 78.0, paint::alpha(accent_a, 0.34));
            inner.glow(orb2_x, orb2_y, 82.0, paint::alpha(accent_b, 0.32));
            // 顶部斜向镜面流光亮带
            inner.vgrad(
                cx_card + 1.0,
                cy_card + 1.0,
                cw_card - 2.0,
                34.0,
                r_card,
                Color32::from_white_alpha(26),
                Color32::TRANSPARENT,
            );
        }

        // 2) 360° 环绕流光彗星边框（Fly Streaming Ribbon Border）
        c.stroke(cx_card, cy_card, cw_card, ch_card, r_card, 1.0, Color32::from_white_alpha(28));
        {
            let segs = 40usize;
            let head_u = (phase * 0.22).rem_euclid(1.0);
            let perim_pt = |u: f32| -> (f32, f32) {
                let u = u.rem_euclid(1.0);
                let top_w = cw_card - 2.0 * r_card;
                let side_h = ch_card - 2.0 * r_card;
                let total = 2.0 * (top_w + side_h);
                let mut d = u * total;
                if d < top_w {
                    return (cx_card + r_card + d, cy_card);
                }
                d -= top_w;
                if d < side_h {
                    return (cx_card + cw_card, cy_card + r_card + d);
                }
                d -= side_h;
                if d < top_w {
                    return (cx_card + cw_card - r_card - d, cy_card + ch_card);
                }
                d -= top_w;
                (cx_card, cy_card + ch_card - r_card - d)
            };
            for i in 0..segs {
                let u0 = i as f32 / segs as f32;
                let u1 = (i + 1) as f32 / segs as f32;
                let dist = (head_u - u0).rem_euclid(1.0);
                let dist2 = (head_u + 0.5 - u0).rem_euclid(1.0);
                let tail1 = (1.0 - (dist / 0.32).clamp(0.0, 1.0)).powi(2);
                let tail2 = (1.0 - (dist2 / 0.28).clamp(0.0, 1.0)).powi(2);
                let intensity = tail1.max(tail2 * 0.85);
                if intensity > 0.05 {
                    let col = if tail1 >= tail2 {
                        mix(accent_a, Color32::WHITE, tail1 * 0.55)
                    } else {
                        mix(accent_b, Color32::WHITE, tail2 * 0.45)
                    };
                    let p0 = perim_pt(u0);
                    let p1 = perim_pt(u1);
                    c.seg(p0, p1, 1.4 + intensity * 0.9, paint::alpha(col, intensity * 0.88));
                }
            }
        }

        // 3) 左侧悬浮封面舞台（带旋律脉动光环，无白圈遮罩）
        let (art_x, art_y, art_s) = (48.0_f32, 18.0_f32, 54.0_f32);
        let beat_amp = self.melody_spectrum_amp(1, 16);
        let beat_s = ctx.animate_value_with_time(egui::Id::new("fly-art-beat"), beat_amp, 0.08);
        c.glow(
            art_x + art_s * 0.5,
            art_y + art_s * 0.5,
            art_s * (0.62 + 0.18 * beat_s),
            paint::alpha(accent_a, 0.42),
        );
        if let Some((image, texture)) = &self.music_artwork {
            let [w, h] = image.size;
            let (uw, vh) = if w > h { (h as f32 / w as f32, 1.0) } else { (1.0, w as f32 / h as f32) };
            let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), egui::vec2(uw, vh));
            c.painter.add(
                egui::epaint::RectShape::filled(c.rect(art_x, art_y, art_s, art_s), 11.0 * c.s, Color32::WHITE)
                    .with_texture(texture.id(), uv),
            );
        } else {
            let cc = c.clipped(art_x, art_y, art_s, art_s);
            cc.vgrad(art_x, art_y, art_s, art_s, 11.0, accent_a, accent_b);
            paint::note(&cc, art_x + art_s * 0.5, art_y + art_s * 0.5, 21.0, Color32::WHITE);
        }
        if has && !cfg!(test) && c.hit(art_x, art_y, art_s, art_s, "fly-art-open").on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("点击跳转到音乐软件").clicked() {
            crate::auralsle_music::focus_player_source(&np.source);
        }

        // 4) 中上：歌名、歌手流光徽章 + 右上 16 柱旋律流光频谱矩阵
        let info_x = 112.0_f32;
        let info_w = 160.0_f32;
        if has {
            c.text(info_x, 30.5, &c.fit(&np.title, 13.0, W::Bold, info_w), 13.0, W::Bold, theme::WHITE, H::Left);
            let artist = if np.artist.is_empty() { "未知艺术家" } else { np.artist.as_str() };
            let fit_art = c.fit(artist, 9.2, W::Semibold, info_w - 10.0);
            let aw = (c.text_w(&fit_art, 9.2, W::Semibold) + 12.0).min(info_w);
            c.fill(info_x, 35.0, aw, 14.5, 7.2, paint::alpha(accent_a, 0.24));
            c.text(info_x + 6.0, 45.2, &fit_art, 9.2, W::Semibold, mix(accent_a, Color32::WHITE, 0.45), H::Left);
        } else {
            c.text(info_x, 30.5, "Fly 流光音乐卡", 13.0, W::Bold, theme::WHITE, H::Left);
            c.text(info_x, 45.2, "封面流光染色 · 旋律律动频谱", 9.2, W::Semibold, mix(accent_a, Color32::WHITE, 0.45), H::Left);
        }

        // 右上 16 柱旋律流光频谱带 (x = 278..372, y = 18..46)
        let eq_bars = 16usize;
        let eq_x0 = 278.0_f32;
        let eq_w = 94.0_f32;
        let eq_step = eq_w / eq_bars as f32;
        let eq_mid_y = 33.0_f32;
        for bi in 0..eq_bars {
            let u = bi as f32 / (eq_bars - 1) as f32;
            let target_a = self.melody_spectrum_amp(bi, eq_bars);
            let amp = ctx.animate_value_with_time(egui::Id::new(("fly-aurora-eq", bi)), target_a, 0.08);
            let bh = (22.0 * amp).clamp(3.2, 22.0);
            let bx = eq_x0 + bi as f32 * eq_step + (eq_step - 3.6) * 0.5;
            let col = mix(accent_a, accent_b, u);
            c.fill(bx, eq_mid_y - bh * 0.5, 3.4, bh, 1.7, col);
        }

        // 5) 中央磨砂流光逐字卡拉 OK 歌词舱 (x = 112, y = 53, w = 260, h = 24)
        c.fill(112.0, 53.0, 260.0, 24.0, 8.0, Color32::from_white_alpha(14));
        c.stroke(112.0, 53.0, 260.0, 24.0, 8.0, 0.75, paint::alpha(accent_a, 0.35));
        let (_prev_l, cur_l, _next_l, word_prog) = self.glance_lyric_triplet();
        draw_karaoke_lyric(c, 242.0, 68.5, 246.0, &cur_l, 10.4, word_prog);
        if c.hit(112.0, 53.0, 260.0, 24.0, "fly-lyric-bar-hit")
            .on_hover_text("点击重新匹配或刷新当前歌曲同步歌词")
            .clicked()
        {
            self.show_lyrics = true;
            self.settings.lyrics_enabled = true;
            self.ly.key.clear();
            self.ly.manual_retry = true;
        }

        // 6) 贯穿式流光彗星进度轨 (x = 48..372, y = 88.0)
        let (bx, bw, by) = (48.0_f32, 324.0_f32, 88.0_f32);
        let bar = c.hit_drag(bx - 4.0, by - 7.0, bw + 8.0, 14.0, "m-seek-fly");
        let seek_position = if has && np.duration > 0.0 && (bar.is_pointer_button_down_on() || bar.clicked() || bar.drag_stopped()) {
            bar.interact_pointer_pos()
                .or_else(|| ctx.input(|i| i.pointer.interact_pos()))
                .map(|pointer| ((c.to_local(pointer).x - bx) / bw).clamp(0.0, 1.0) as f64 * np.duration)
        } else {
            None
        };
        let position = seek_position.unwrap_or(np.position);
        let frac = if np.duration > 0.0 { (position / np.duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
        c.fill(bx, by - 2.2, bw, 4.4, 2.2, Color32::from_white_alpha(26));
        if frac > 0.0 {
            let pw = (bw * frac).max(5.0);
            let head_x = bx + pw;
            c.hgrad(bx, by - 2.2, pw, 4.4, 2.2, accent_a, accent_b);
            paint::chase(c, bx, head_x, by, 4.4, mix(accent_a, Color32::WHITE, 0.35), if np.playing { 0.78 } else { 0.42 });
            c.glow(head_x, by, 9.5, paint::alpha(accent_b, 0.65));
            c.circle(head_x, by, 4.2, Color32::WHITE);
        }
        if (bar.clicked() || bar.drag_stopped()) && seek_position.is_some() {
            cmds.push(Cmd::Media(Transport::Seek(seek_position.unwrap())));
        }

        // 7) 底部时间戳与流光胶囊播控坞 (y = 112.0)
        let cy_ctrl = 112.0_f32;
        let ic = if has { theme::WHITE } else { Color32::from_gray(115) };
        c.text(48.0, cy_ctrl + 3.8, &mmss(position), 9.2, W::Semibold, mix(accent_a, Color32::WHITE, 0.4), H::Left);
        let rem = if has && np.duration <= 0.0 { "--:--".into() } else { format!("-{}", mmss((np.duration - position).max(0.0))) };
        c.text(372.0, cy_ctrl + 3.8, &rem, 9.2, W::Regular, Color32::from_gray(195), H::Right);

        c.fill(152.0, cy_ctrl - 12.5, 116.0, 25.0, 12.5, Color32::from_white_alpha(16));
        c.stroke(152.0, cy_ctrl - 12.5, 116.0, 25.0, 12.5, 0.8, Color32::from_white_alpha(28));
        let prev_fly = c.hit(156.0, cy_ctrl - 12.0, 30.0, 24.0, "m-prev-fly");
        let (pf_h, pf_p) = c.hover_t(&prev_fly);
        if pf_h > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(pf_h), 171.0, cy_ctrl, 10.5, accent_a, pf_h, false); }
        if prev_fly.clicked() && has {
            cmds.push(Cmd::Media(Transport::Prev));
        }
        paint::skip(&c.scaled(171.0, cy_ctrl, 1.0 - 0.08 * pf_p), 171.0, cy_ctrl, -0.92, ic);

        let play_r = c.hit(192.0, cy_ctrl - 12.5, 36.0, 25.0, "m-play-fly");
        let (p_hov, p_press) = c.hover_t(&play_r);
        paint_music_play_button(c, 210.0, cy_ctrl, 34.0, 20.0, p_hov, p_press, playing_t, accent_a, 11.0);
        if play_r.clicked() && has {
            cmds.push(Cmd::Media(Transport::Toggle));
        }

        let next_fly = c.hit(234.0, cy_ctrl - 12.0, 30.0, 24.0, "m-next-fly");
        let (nf_h, nf_p) = c.hover_t(&next_fly);
        if nf_h > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(nf_h), 249.0, cy_ctrl, 10.5, accent_a, nf_h, false); }
        if next_fly.clicked() && has {
            cmds.push(Cmd::Media(Transport::Next));
        }
        paint::skip(&c.scaled(249.0, cy_ctrl, 1.0 - 0.08 * nf_p), 249.0, cy_ctrl, 0.92, ic);
    }

    /// 音乐岛样式 5：Glance 灵动自适应音乐控件 (移植自 https://github.com/lulu-loopp/glance ，需求 7)：
    /// - 不写死岛体长度，整个岛体就是 Glance 音乐控件，岛体宽高随所选形态（紧凑歌词横条 / 三行歌词流光卡 / 黑胶频谱舞台）及当前歌名与歌词长度实时弹性伸缩！
    /// - 支持逐字点亮卡拉 OK 歌词、封面色彩流体烟雾背景、三形态一键无缝切换与音量滑条。
    fn page_music_glance(&mut self, c: &Card, _env: &Env, cmds: &mut Vec<Cmd>) {
        // [修复需求 2 图1 `image_3d6619.jpg` & 需求 7 图8 `image_3d007d.jpg` / 图9 `image_3d0002.jpg`]：
        // - 左右两侧各预留 36px 专属切页按钮通道 (`pad_l = x0 + 36.0`, `pad_r = x1 - 36.0`)，使 `<` 与 `>` 箭头与三种 Glance 形态完美适配、绝不压盖内容；
        // - 形态 1（卡片，图9）：左侧 88×88 专辑图 (`y = 20..108`) 与右侧「歌名胶囊头 (`y = 20..38`) + 三行歌词舱 (`y = 42..108`)」上下边缘像素级严格对齐，彻底消除歌名被歌词框裁切的问题；
        // - 形态 2（舞台，图8）：左侧黑胶唱盘 (`y = 24..108`) 与右侧「歌名 + 歌手 + 逐字歌词 + 下一句预览 + 22 柱频谱」全部严格左对齐于 `rx` (`pad_l + 94.0`)，进度条与内容区左右边界严丝合缝对齐！
        let np = self.media.clone();
        let has = !np.title.is_empty();
        let ctx = c.ui.ctx().clone();
        let playing_t = ctx.animate_bool_with_time(egui::Id::new("np-playing-glance"), np.playing, 0.22);
        let cfg = crate::auralsle_ai_status::config::get();
        let form = cfg.glance_music_form % 3;

        // [修复需求 3：调整音乐岛“Glance自适应”UI的宽度，和其他岛保持一致（标准 419.0px），并进行内容布局适配]
        let cur_w = 419.0_f32;
        let cur_h = self.h.pos.clamp(124.0, 240.0);
        let x0 = 0.0_f32;
        let x1 = 419.0_f32;
        let pad_l = 38.0_f32;
        let pad_r = 381.0_f32;
        let inner_w = 343.0_f32;

        let (accent_a, accent_b) = if has {
            self.extract_artwork_colors()
        } else {
            (Color32::from_rgb(56, 189, 248), Color32::from_rgb(168, 85, 247))
        };
        // [修复需求 12 图6 `image_3a2b44.jpg` 暂停/播放按钮右边泛白问题]：
        // 对提取出的专辑强调色进行高饱和深色收敛（限制最高亮度 <= 198，确保纯白 `||` / `▶` 图标在按钮左右两侧都具有极高对比度，绝不泛白！）
        let clamp_vivid = |col: Color32| -> Color32 {
            let (mut r, mut g, mut b) = (col.r() as f32, col.g() as f32, col.b() as f32);
            let max_c = r.max(g).max(b).max(1.0);
            let min_c = r.min(g).min(b);
            // 若颜色偏浅或饱和度不足（导致图6右侧发白），拉大色差并压低高亮通道
            if min_c > 135.0 {
                let pull = (min_c - 110.0) * 0.72;
                r = (r - pull).max(28.0);
                g = (g - pull).max(28.0);
                b = (b - pull).max(36.0);
            }
            if max_c > 205.0 {
                let scale = 205.0 / max_c;
                r *= scale;
                g *= scale;
                b *= scale;
            }
            Color32::from_rgb(r.round() as u8, g.round() as u8, b.round() as u8)
        };
        let play_col_a = clamp_vivid(accent_a);
        let play_col_b = clamp_vivid(mix(accent_a, accent_b, 0.45));

        // 1) Glance 背景节拍流光（奶油主题下取消背景跟随专辑图取色渲染，保持纯净温润奶油底色）
        if cfg!(test) || self.settings.skin != crate::model::Skin::Cream {
            let beat_phase = (self.t as f32 * if np.playing { 2.4 } else { 0.6 }).sin() * 18.0;
            c.glow(x0 + cur_w * 0.28 + beat_phase, cur_h * 0.46, cur_w * 0.34, paint::alpha(accent_a, 0.18 + 0.14 * playing_t));
            c.glow(x0 + cur_w * 0.72 - beat_phase, cur_h * 0.52, cur_w * 0.32, paint::alpha(accent_b, 0.15 + 0.12 * playing_t));
        }

        // 2) 顶栏安全区 (y = -5.5，对应岛体 y = 8.5)：Glance 三种形态平滑滑动切换胶囊 ([横条 | 卡片 | 舞台])，右缘对齐 `pad_r`
        let form_x0 = (pad_r - 78.0).max(246.0);
        let form_y0 = -5.5_f32;
        c.fill(form_x0, form_y0, 78.0, 17.5, 8.8, Color32::from_rgba_unmultiplied(24, 26, 34, 215));
        c.stroke(form_x0, form_y0, 78.0, 17.5, 8.8, 0.75, Color32::from_white_alpha(26));
        let slide_fx = ctx.animate_value_with_time(
            egui::Id::new("glance-form-slide-x"),
            form_x0 + 1.5 + (form as f32) * 25.2,
            0.18,
        );
        let pill_col = paint::alpha(paint::mix(accent_a, crate::auralsle_ai_status::plugins::active_accent_color(), 0.45), 0.90);
        c.fill(slide_fx, form_y0 + 1.5, 24.6, 14.5, 7.25, pill_col);
        for (fi, (f_val, f_lbl)) in [(0u8, "横条"), (1u8, "卡片"), (2u8, "舞台")].iter().enumerate() {
            let bx = form_x0 + 1.5 + fi as f32 * 25.2;
            let sel = form == *f_val;
            let hit = c.hit(bx, form_y0, 25.0, 17.5, ("glance-m-form", *f_val));
            c.centered_text(
                bx + 12.3,
                form_y0 + 8.75,
                f_lbl,
                7.4,
                if sel { W::Bold } else { W::Regular },
                if sel { theme::WHITE } else { Color32::from_gray(182) },
                H::Center,
            );
            if hit.clicked() && !sel {
                let nf = *f_val;
                crate::auralsle_ai_status::config::update(|c_mut| c_mut.glance_music_form = nf);
            }
        }

        // 辅助：提取当前句、上一句、下一句以及当前句逐字点亮进度 (0.0..=1.0)
        let (prev_line, cur_line, next_line, word_prog) = {
            if let Some(lyrics) = &self.ly.lyrics {
                if !lyrics.lines.is_empty() {
                    let pos = self.lyric_position();
                    let idx = if lyrics.synced {
                        lyrics.index_at(pos).unwrap_or(0)
                    } else {
                        ((pos.max(0.0) / 4.0) as usize) % lyrics.lines.len()
                    };
                    let prev_s = if idx > 0 {
                        lyrics.lines.get(idx - 1).map(|l| l.text.trim().to_owned()).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let cur_s = lyrics
                        .lines
                        .get(idx)
                        .map(|l| l.text.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| self.current_music_lyric_text().0);
                    let next_s = lyrics
                        .lines
                        .get(idx + 1)
                        .map(|l| l.text.trim().to_owned())
                        .unwrap_or_default();
                    let cur_t0 = lyrics.lines.get(idx).map(|l| l.t).unwrap_or(0.0);
                    let cur_t1 = lyrics
                        .lines
                        .get(idx + 1)
                        .map(|l| l.t)
                        .unwrap_or((cur_t0 + 3.5).max(np.duration as f32));
                    let p_word = if lyrics.synced {
                        ((pos - cur_t0) / (cur_t1 - cur_t0).max(0.4)).clamp(0.0, 1.0)
                    } else {
                        0.85
                    };
                    (prev_s, cur_s, next_s, p_word)
                } else {
                    (String::new(), self.current_music_lyric_text().0, String::new(), 1.0)
                }
            } else {
                (String::new(), self.current_music_lyric_text().0, String::new(), 1.0)
            }
        };

        // 辅助：居中逐字点亮 + 超宽平滑横向滚动歌词行
        let draw_karaoke_lyric_center = |card: &Card, lx: f32, ly: f32, max_w: f32, text: &str, fsz: f32, prog: f32| {
            let phase = if cfg!(test) { 0.0 } else { card.ui.ctx().input(|i| i.time) as f32 * 2.4 };
            let ly = ly + if cfg!(test) { 0.0 } else { phase.sin() * 0.8 };
            let raw_tw = card.text_w(text, fsz, W::Bold);
            let box_x = lx - max_w * 0.5;
            let viewport = card.clipped(box_x, ly - fsz - 4.0, max_w, fsz + 10.0);
            let (draw_x, total_tw) = if !cfg!(test) && raw_tw > max_w {
                let scroll_u = ((prog - 0.14) / 0.72).clamp(0.0, 1.0);
                let sx = card.ui.ctx().animate_value_with_time(egui::Id::new("gl-karaoke-scroll-c"), (raw_tw - max_w + 6.0) * scroll_u, 0.12);
                (box_x - sx, raw_tw)
            } else {
                (lx - raw_tw * 0.5, raw_tw)
            };
            let lit_w = (total_tw * prog.clamp(0.18, 1.0)).clamp(0.0, total_tw + 2.0);
            let clip_lit = viewport.clipped(draw_x - 2.0, ly - fsz - 4.0, lit_w + 2.0, fsz + 10.0);
            clip_lit.text(draw_x, ly, text, fsz, W::Bold, theme::WHITE, H::Left);
            if lit_w < total_tw {
                let clip_dim = viewport.clipped(draw_x + lit_w, ly - fsz - 4.0, (total_tw - lit_w) + 6.0, fsz + 10.0);
                clip_dim.text(draw_x, ly, text, fsz, W::Bold, Color32::from_white_alpha(115), H::Left);
            }
            karaoke_wave_crest(&viewport, draw_x + lit_w, ly, fsz, prog, phase, lit_w < total_tw);
        };

        // 辅助：左对齐逐字点亮 + 超宽平滑横向滚动歌词行
        let draw_karaoke_lyric_left = |card: &Card, start_x: f32, ly: f32, max_w: f32, text: &str, fsz: f32, prog: f32| {
            let phase = if cfg!(test) { 0.0 } else { card.ui.ctx().input(|i| i.time) as f32 * 2.4 };
            let ly = ly + if cfg!(test) { 0.0 } else { phase.sin() * 0.8 };
            let raw_tw = card.text_w(text, fsz, W::Bold);
            let viewport = card.clipped(start_x, ly - fsz - 4.0, max_w, fsz + 10.0);
            let draw_x = if !cfg!(test) && raw_tw > max_w {
                let scroll_u = ((prog - 0.14) / 0.72).clamp(0.0, 1.0);
                let sx = card.ui.ctx().animate_value_with_time(egui::Id::new("gl-karaoke-scroll-l"), (raw_tw - max_w + 6.0) * scroll_u, 0.12);
                start_x - sx
            } else {
                start_x
            };
            let lit_w = (raw_tw * prog.clamp(0.18, 1.0)).clamp(0.0, raw_tw + 2.0);
            let clip_lit = viewport.clipped(draw_x - 2.0, ly - fsz - 4.0, lit_w + 2.0, fsz + 10.0);
            clip_lit.text(draw_x, ly, text, fsz, W::Bold, theme::WHITE, H::Left);
            if lit_w < raw_tw {
                let clip_dim = viewport.clipped(draw_x + lit_w, ly - fsz - 4.0, (raw_tw - lit_w) + 6.0, fsz + 10.0);
                clip_dim.text(draw_x, ly, text, fsz, W::Bold, Color32::from_white_alpha(115), H::Left);
            }
            karaoke_wave_crest(&viewport, draw_x + lit_w, ly, fsz, prog, phase, lit_w < raw_tw);
        };

        if form == 0 {
            // ── 形态 0：Glance 紧凑歌词横条（图1 `image_3d6619.jpg`：严格位于 `pad_l .. pad_r` 内，绝不触碰左右切页按钮）──
            c.fill(pad_l, 18.0, inner_w, 74.0, 13.0, Color32::from_white_alpha(12));
            c.stroke(pad_l, 18.0, inner_w, 74.0, 13.0, 0.85, Color32::from_white_alpha(22));
            let art_x = pad_l + 8.0;
            let art_y = 24.0_f32;
            let art_s = 48.0_f32;
            if let Some((image, texture)) = &self.music_artwork {
                let [w, h] = image.size;
                let (uw, vh) = if w > h { (h as f32 / w as f32, 1.0) } else { (1.0, w as f32 / h as f32) };
                let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), egui::vec2(uw, vh));
                c.painter.add(
                    egui::epaint::RectShape::filled(c.rect(art_x, art_y, art_s, art_s), 10.5 * c.s, Color32::WHITE)
                        .with_texture(texture.id(), uv),
                );
            } else {
                let cc = c.clipped(art_x, art_y, art_s, art_s);
                cc.vgrad(art_x, art_y, art_s, art_s, 10.5, accent_a, accent_b);
                paint::note(&cc, art_x + art_s * 0.5, art_y + art_s * 0.5, 19.0, Color32::WHITE);
            }
            if has && !cfg!(test) && c.hit(art_x, art_y, art_s, art_s, "glance-m0-art-open").on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("点击跳转到音乐软件").clicked() {
                crate::auralsle_music::focus_player_source(&np.source);
            }

            let info_x = art_x + art_s + 10.0;
            let ctrl_w = 82.0_f32;
            let info_w = (pad_r - 10.0 - info_x - ctrl_w).max(90.0);
            let title_str = if has { np.title.as_str() } else { "Glance 音乐横条" };
            let artist_str = if has {
                if np.artist.is_empty() { "正在播放" } else { np.artist.as_str() }
            } else {
                "随歌名与歌词长度自适应岛体宽度"
            };
            c.text(info_x, 37.0, &c.fit(title_str, 12.0, W::Bold, info_w), 12.0, W::Bold, theme::WHITE, H::Left);
            c.text(info_x, 50.0, &c.fit(artist_str, 8.8, W::Semibold, info_w), 8.8, W::Semibold, mix(accent_a, Color32::WHITE, 0.4), H::Left);

            // 逐字点亮当前歌词（与歌名、歌手严格左对齐于 `info_x`）
            draw_karaoke_lyric_left(c, info_x, 66.5, info_w + ctrl_w - 6.0, &cur_line, 9.6, word_prog);

            // 右侧磨砂三联播控胶囊 (Prev / Play / Next，右缘对齐 `pad_r - 8.0`，深色高对比度播放键杜绝泛白)
            let cy_ctrl = 41.0_f32;
            let cx_ctrl = pad_r - 45.0;
            let ic = if has { theme::WHITE } else { Color32::from_gray(115) };
            c.fill(cx_ctrl - 37.0, cy_ctrl - 11.5, 76.0, 23.0, 11.5, Color32::from_rgba_unmultiplied(18, 20, 28, 210));
            c.stroke(cx_ctrl - 37.0, cy_ctrl - 11.5, 76.0, 23.0, 11.5, 0.8, Color32::from_white_alpha(24));
            let prev_gl0 = c.hit(cx_ctrl - 35.0, cy_ctrl - 11.0, 22.0, 22.0, "gl-m0-prev");
            let (p0h, p0p) = c.hover_t(&prev_gl0);
            if p0h > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(p0h), cx_ctrl - 24.0, cy_ctrl, 9.5, accent_a, p0h, false); }
            if prev_gl0.clicked() && has {
                cmds.push(Cmd::Media(Transport::Prev));
            }
            paint::skip(&c.scaled(cx_ctrl - 24.0, cy_ctrl, 1.0 - 0.08 * p0p), cx_ctrl - 24.0, cy_ctrl, -0.82, ic);
            let play_r = c.hit(cx_ctrl - 12.0, cy_ctrl - 11.0, 26.0, 22.0, "gl-m0-play");
            let (p_hov, p_press) = c.hover_t(&play_r);
            paint_music_play_button(c, cx_ctrl + 1.0, cy_ctrl, 24.0, 19.0, p_hov, p_press, playing_t, play_col_a, 10.2);
            if play_r.clicked() && has {
                cmds.push(Cmd::Media(Transport::Toggle));
            }
            let next_gl0 = c.hit(cx_ctrl + 15.0, cy_ctrl - 11.0, 22.0, 22.0, "gl-m0-next");
            let (n0h, n0p) = c.hover_t(&next_gl0);
            if n0h > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(n0h), cx_ctrl + 26.0, cy_ctrl, 9.5, accent_a, n0h, false); }
            if next_gl0.clicked() && has {
                cmds.push(Cmd::Media(Transport::Next));
            }
            paint::skip(c, cx_ctrl + 26.0, cy_ctrl, 0.82, ic);

            // 底部贯穿式流光进度轨 (y = 82.0，左右对齐卡片内边距)
            let bx = pad_l + 8.0;
            let bw = (inner_w - 16.0).max(120.0);
            let by = 82.0_f32;
            let bar = c.hit_drag(bx, by - 6.0, bw, 12.0, "gl-m0-seek");
            let seek_pos = if has && np.duration > 0.0 && (bar.is_pointer_button_down_on() || bar.clicked() || bar.drag_stopped()) {
                bar.interact_pointer_pos()
                    .or_else(|| ctx.input(|i| i.pointer.interact_pos()))
                    .map(|p| ((c.to_local(p).x - bx) / bw).clamp(0.0, 1.0) as f64 * np.duration)
            } else {
                None
            };
            let pos = seek_pos.unwrap_or(np.position);
            let frac = if np.duration > 0.0 { (pos / np.duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
            c.fill(bx, by - 1.8, bw, 3.6, 1.8, Color32::from_white_alpha(24));
            if frac > 0.0 {
                let pw = (bw * frac).max(3.6);
                c.hgrad(bx, by - 1.8, pw, 3.6, 1.8, accent_a, accent_b);
                c.circle(bx + pw, by, 3.4, Color32::WHITE);
            }
            if (bar.clicked() || bar.drag_stopped()) && seek_pos.is_some() {
                cmds.push(Cmd::Media(Transport::Seek(seek_pos.unwrap())));
            }
        } else if form == 1 {
            // ── 形态 1：Glance 三行歌词卡片（修复图9 `image_3d0002.jpg`：左专辑图与右栏顶底 y=20..108 像素级对齐，歌名与歌词舱留足间距绝不裁切！）──
            let art_x = pad_l;
            let art_y = 20.0_f32;
            let art_s = 88.0_f32;
            if let Some((image, texture)) = &self.music_artwork {
                let [w, h] = image.size;
                let (uw, vh) = if w > h { (h as f32 / w as f32, 1.0) } else { (1.0, w as f32 / h as f32) };
                let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), egui::vec2(uw, vh));
                c.painter.add(
                    egui::epaint::RectShape::filled(c.rect(art_x, art_y, art_s, art_s), 13.5 * c.s, Color32::WHITE)
                        .with_texture(texture.id(), uv),
                );
            } else {
                let cc = c.clipped(art_x, art_y, art_s, art_s);
                cc.vgrad(art_x, art_y, art_s, art_s, 13.5, accent_a, accent_b);
                paint::note(&cc, art_x + art_s * 0.5, art_y + art_s * 0.5, 28.0, Color32::WHITE);
            }
            c.stroke(art_x, art_y, art_s, art_s, 13.5, 0.8, Color32::from_white_alpha(26));
            if has && !cfg!(test) && c.hit(art_x, art_y, art_s, art_s, "glance-m1-art-open").on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("点击跳转到音乐软件").clicked() {
                crate::auralsle_music::focus_player_source(&np.source);
            }

            // 右栏 (rx = pad_l + 98.0 .. pad_r，总高 y = 20..108 = 88px，与左侧 88×88 专辑图上下边缘严格齐平！)
            let rx = art_x + art_s + 10.0;
            let rw = (pad_r - rx).max(160.0);
            let rcx = rx + rw * 0.5;
            let title_str = if has {
                if np.artist.is_empty() { np.title.clone() } else { format!("{} — {}", np.title, np.artist) }
            } else {
                "Glance 三行歌词卡片 · 自适应岛体".to_owned()
            };
            // 右上歌名标题条 (y = 20..38，基线 y = 33.2，下方留 4px 间距，绝不被 y=42 的歌词框遮挡！)
            c.fill(rx, 20.0, rw, 18.0, 6.0, Color32::from_white_alpha(10));
            c.circle(rx + 8.5, 29.0, 2.4, accent_a);
            c.text(
                rx + 15.0,
                33.2,
                &c.fit(&title_str, 10.5, W::Bold, rw - 22.0),
                10.5,
                W::Bold,
                mix(accent_a, Color32::WHITE, 0.55),
                H::Left,
            );

            // 三行歌词磨砂内舱 (y = 42..108，高度 66px，底边 108.0 与左侧专辑图底边 108.0 像素级对齐！)
            c.fill(rx, 42.0, rw, 66.0, 10.0, Color32::from_white_alpha(12));
            c.stroke(rx, 42.0, rw, 66.0, 10.0, 0.85, Color32::from_white_alpha(22));
            let prev_disp = if prev_line.is_empty() { "· · ·" } else { prev_line.as_str() };
            let next_disp = if next_line.is_empty() { "· · ·" } else { next_line.as_str() };
            c.text(rcx, 57.5, &c.fit(prev_disp, 9.0, W::Regular, rw - 16.0), 9.0, W::Regular, Color32::from_white_alpha(105), H::Center);
            draw_karaoke_lyric_center(c, rcx, 78.0, rw - 16.0, &cur_line, 11.2, word_prog);
            c.text(rcx, 98.0, &c.fit(next_disp, 9.0, W::Regular, rw - 16.0), 9.0, W::Regular, Color32::from_white_alpha(120), H::Center);

            // 进度轨 (y = 121.0)：左时间戳对齐 `pad_l` (专辑图左缘)，右时间戳对齐 `pad_r` (歌词框右缘)！
            let by = 121.0_f32;
            let bx = pad_l + 32.0;
            let bw = (inner_w - 64.0).max(140.0);
            let bar = c.hit_drag(bx - 4.0, by - 7.0, bw + 8.0, 14.0, "gl-m1-seek");
            let seek_pos = if has && np.duration > 0.0 && (bar.is_pointer_button_down_on() || bar.clicked() || bar.drag_stopped()) {
                bar.interact_pointer_pos()
                    .or_else(|| ctx.input(|i| i.pointer.interact_pos()))
                    .map(|p| ((c.to_local(p).x - bx) / bw).clamp(0.0, 1.0) as f64 * np.duration)
            } else {
                None
            };
            let pos = seek_pos.unwrap_or(np.position);
            let frac = if np.duration > 0.0 { (pos / np.duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
            c.text(pad_l, by + 3.4, &mmss(pos), 8.6, W::Semibold, Color32::from_gray(200), H::Left);
            let rem = if has && np.duration <= 0.0 { "--:--".into() } else { format!("-{}", mmss((np.duration - pos).max(0.0))) };
            c.text(pad_r, by + 3.4, &rem, 8.6, W::Semibold, Color32::from_gray(200), H::Right);
            paint_themed_music_progress_bar(c, bx, by, bw, frac, accent_a, np.playing, self.t);
            if (bar.clicked() || bar.drag_stopped()) && seek_pos.is_some() {
                cmds.push(Cmd::Media(Transport::Seek(seek_pos.unwrap())));
            }

            // 底部居中播控坞 (y = 144.0，修复图6暂停/播放键右侧泛白)
            let y_ctrl = 144.0_f32;
            let mid_x = 209.5_f32;
            let ic = if has { theme::WHITE } else { Color32::from_gray(110) };
            c.fill(mid_x - 66.0, y_ctrl - 12.0, 132.0, 24.0, 12.0, Color32::from_rgba_unmultiplied(18, 20, 28, 205));
            c.stroke(mid_x - 66.0, y_ctrl - 12.0, 132.0, 24.0, 12.0, 0.8, Color32::from_white_alpha(24));
            let prev_gl1 = c.hit(mid_x - 56.0, y_ctrl - 11.0, 28.0, 22.0, "gl-m1-prev");
            let (p1h, p1p) = c.hover_t(&prev_gl1);
            if p1h > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(p1h), mid_x - 42.0, y_ctrl, 10.0, accent_a, p1h, false); }
            if prev_gl1.clicked() && has {
                cmds.push(Cmd::Media(Transport::Prev));
            }
            paint::skip(&c.scaled(mid_x - 42.0, y_ctrl, 1.0 - 0.08 * p1p), mid_x - 42.0, y_ctrl, -0.95, ic);
            let play_r = c.hit(mid_x - 21.0, y_ctrl - 12.0, 42.0, 24.0, "gl-m1-play");
            let (p_hov, p_press) = c.hover_t(&play_r);
            paint_music_play_button(c, mid_x, y_ctrl, 40.0, 19.6, p_hov, p_press, playing_t, play_col_a, 11.0);
            if play_r.clicked() && has {
                cmds.push(Cmd::Media(Transport::Toggle));
            }
            let next_gl1 = c.hit(mid_x + 28.0, y_ctrl - 11.0, 28.0, 22.0, "gl-m1-next");
            let (n1h, n1p) = c.hover_t(&next_gl1);
            if n1h > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(n1h), mid_x + 42.0, y_ctrl, 10.0, accent_a, n1h, false); }
            if next_gl1.clicked() && has {
                cmds.push(Cmd::Media(Transport::Next));
            }
            paint::skip(&c.scaled(mid_x + 42.0, y_ctrl, 1.0 - 0.08 * n1p), mid_x + 42.0, y_ctrl, 0.95, ic);
        } else {
            // ── 形态 2：Glance 黑胶频谱舞台（修复图8 `image_3d007d.jpg`：左侧黑胶与右栏歌名/歌手/歌词/22柱频谱全部严格对齐，进度条与内容区左右边界严丝合缝对齐！）──
            let outer_r = 42.0_f32;
            let art_r = 31.0_f32;
            let vcx = pad_l + outer_r;
            let vcy = 66.0_f32;
            c.circle(vcx, vcy, outer_r, Color32::from_rgb(14, 15, 20));
            c.ring(vcx, vcy, outer_r - 2.2, 0.85, Color32::from_white_alpha(26));
            c.ring(vcx, vcy, outer_r - 6.0, 0.65, paint::alpha(accent_a, 0.35));
            self.draw_circular_album_art(c, vcx, vcy, art_r, cfg.music_round_art_rotate, accent_a, accent_b);
            c.circle(vcx, vcy, 5.4, Color32::from_rgb(18, 19, 24));
            c.circle(vcx, vcy, 2.1, Color32::from_white_alpha(190));

            // 右栏舞台 (rx = pad_l + 94.0 .. pad_r，总高 y = 24..108 = 84px，与左侧直径 84px 的黑胶唱盘顶底严格齐平！)
            let rx = pad_l + outer_r * 2.0 + 10.0;
            let rw = (pad_r - rx).max(180.0);
            let title_str = if has { np.title.as_str() } else { "Glance 黑胶频谱音乐舞台" };
            let artist_str = if has {
                if np.artist.is_empty() { "未知艺术家" } else { np.artist.as_str() }
            } else {
                "转动唱片 + 逐字点亮大字歌词 + 22 柱旋律频谱舞台"
            };
            c.text(rx, 35.0, &c.fit(title_str, 13.0, W::Bold, rw), 13.0, W::Bold, theme::WHITE, H::Left);
            c.text(rx, 49.0, &c.fit(artist_str, 9.2, W::Semibold, rw), 9.2, W::Semibold, mix(accent_a, Color32::WHITE, 0.40), H::Left);

            // 当前句逐字点亮大歌词 (y = 67.5) 与下一句预览 (y = 82.0) 严格左对齐于 `rx`（修复图8红框处歌名左对齐而歌词居中造成的错位！）
            draw_karaoke_lyric_left(c, rx, 67.5, rw, &cur_line, 11.4, word_prog);
            let next_disp = if next_line.is_empty() { prev_line.as_str() } else { next_line.as_str() };
            if !next_disp.is_empty() {
                c.text(rx, 82.0, &c.fit(next_disp, 8.8, W::Regular, rw), 8.8, W::Regular, Color32::from_white_alpha(120), H::Left);
            }

            // 22 柱旋律律动频谱矩阵 (x = rx..rx+rw, y = 88..108，底边 108.0 与黑胶唱盘底边 108.0 严格齐平！)
            let eq_bars = 22usize;
            let eq_step = rw / eq_bars as f32;
            let eq_bot = 108.0_f32;
            for bi in 0..eq_bars {
                let target_a = self.melody_spectrum_amp(bi, eq_bars);
                let amp = ctx.animate_value_with_time(egui::Id::new(("glance-st-bar", bi)), target_a, 0.08);
                let bh = (20.0 * amp).clamp(3.0, 20.0);
                let bx = rx + bi as f32 * eq_step;
                c.fill(bx, eq_bot - bh, (eq_step - 2.2).clamp(3.2, 5.2), bh, 1.8, mix(accent_a, accent_b, bi as f32 / (eq_bars - 1) as f32));
            }

            // 进度轨 (y = 125.0)：左时间戳对齐 `pad_l` (黑胶左缘)，右时间戳对齐 `pad_r` (频谱右缘)！
            let by = 125.0_f32;
            let bx = pad_l + 32.0;
            let bw = (inner_w - 64.0).max(160.0);
            let bar = c.hit_drag(bx - 4.0, by - 7.0, bw + 8.0, 14.0, "gl-m2-seek");
            let seek_pos = if has && np.duration > 0.0 && (bar.is_pointer_button_down_on() || bar.clicked() || bar.drag_stopped()) {
                bar.interact_pointer_pos()
                    .or_else(|| ctx.input(|i| i.pointer.interact_pos()))
                    .map(|p| ((c.to_local(p).x - bx) / bw).clamp(0.0, 1.0) as f64 * np.duration)
            } else {
                None
            };
            let pos = seek_pos.unwrap_or(np.position);
            let frac = if np.duration > 0.0 { (pos / np.duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
            c.text(pad_l, by + 3.4, &mmss(pos), 8.6, W::Semibold, Color32::from_gray(200), H::Left);
            let rem = if has && np.duration <= 0.0 { "--:--".into() } else { format!("-{}", mmss((np.duration - pos).max(0.0))) };
            c.text(pad_r, by + 3.4, &rem, 8.6, W::Semibold, Color32::from_gray(200), H::Right);
            paint_themed_music_progress_bar(c, bx, by, bw, frac, accent_a, np.playing, self.t);
            if (bar.clicked() || bar.drag_stopped()) && seek_pos.is_some() {
                cmds.push(Cmd::Media(Transport::Seek(seek_pos.unwrap())));
            }

            // 底部居中全功能播控坞 (y = 149.0，修复图6 `image_3a2b44.jpg` 暂停播放键右侧泛白)
            let y_ctrl = 149.0_f32;
            let mid_x = 209.5_f32;
            let ic = if has { theme::WHITE } else { Color32::from_gray(110) };
            c.fill(mid_x - 68.0, y_ctrl - 12.0, 136.0, 24.0, 12.0, Color32::from_rgba_unmultiplied(18, 20, 28, 210));
            c.stroke(mid_x - 68.0, y_ctrl - 12.0, 136.0, 24.0, 12.0, 0.85, Color32::from_white_alpha(26));
            let prev_gl2 = c.hit(mid_x - 60.0, y_ctrl - 11.0, 28.0, 22.0, "gl-m2-prev");
            let (p2h, p2p) = c.hover_t(&prev_gl2);
            if p2h > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(p2h), mid_x - 46.0, y_ctrl, 10.0, accent_a, p2h, false); }
            if prev_gl2.clicked() && has {
                cmds.push(Cmd::Media(Transport::Prev));
            }
            paint::skip(&c.scaled(mid_x - 46.0, y_ctrl, 1.0 - 0.08 * p2p), mid_x - 46.0, y_ctrl, -1.0, ic);
            let play_r = c.hit(mid_x - 22.0, y_ctrl - 12.0, 44.0, 24.0, "gl-m2-play");
            let (p_hov, p_press) = c.hover_t(&play_r);
            paint_music_play_button(c, mid_x, y_ctrl, 42.0, 19.6, p_hov, p_press, playing_t, play_col_a, 11.2);
            if play_r.clicked() && has {
                cmds.push(Cmd::Media(Transport::Toggle));
            }
            let next_gl2 = c.hit(mid_x + 32.0, y_ctrl - 11.0, 28.0, 22.0, "gl-m2-next");
            let (n2h, n2p) = c.hover_t(&next_gl2);
            if n2h > 0.01 { crate::auralsle_music::paint_music_trigger_circle(&c.faded(n2h), mid_x + 46.0, y_ctrl, 10.0, accent_a, n2h, false); }
            if next_gl2.clicked() && has {
                cmds.push(Cmd::Media(Transport::Next));
            }
            paint::skip(&c.scaled(mid_x + 46.0, y_ctrl, 1.0 - 0.08 * n2p), mid_x + 46.0, y_ctrl, 1.0, ic);
        }
    }

    /// 绘制支持平滑旋转的圆形专辑封面（仅作用于圆形专辑图：黑胶唱机与 Glance 舞台，需求 12）。
    pub(crate) fn draw_circular_album_art(
        &self,
        c: &Card,
        cx: f32,
        cy: f32,
        r: f32,
        rotate_enabled: bool,
        accent_a: Color32,
        accent_b: Color32,
    ) {
        let angle = if rotate_enabled && self.media.playing {
            (self.t as f32 * 0.65).rem_euclid(std::f32::consts::TAU)
        } else {
            0.0
        };
        if rotate_enabled && self.media.playing {
            c.ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        }
        if !self.media.title.trim().is_empty() && !cfg!(test)
            && c.hit(cx - r, cy - r, r * 2.0, r * 2.0, "glance-m2-art-open")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("点击跳转到音乐软件")
                .clicked()
        {
            crate::auralsle_music::focus_player_source(&self.media.source);
        }
        if let Some((image, texture)) = &self.music_artwork {
            let [w, h] = image.size;
            let (uw, vh) = if w > h { (h as f32 / w as f32, 1.0) } else { (1.0, w as f32 / h as f32) };
            let segs = 36usize;
            let mut mesh = egui::Mesh::with_texture(texture.id());
            let center_p = c.pos(cx, cy);
            mesh.vertices.push(egui::epaint::Vertex {
                pos: center_p,
                uv: Pos2::new(0.5, 0.5),
                color: Color32::WHITE,
            });
            for i in 0..=segs {
                let theta = (i as f32 / segs as f32) * std::f32::consts::TAU;
                let px = cx + r * theta.cos();
                let py = cy + r * theta.sin();
                let uv_theta = theta - angle;
                let u = 0.5 + 0.5 * uw * uv_theta.cos();
                let v = 0.5 + 0.5 * vh * uv_theta.sin();
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: c.pos(px, py),
                    uv: Pos2::new(u, v),
                    color: Color32::WHITE,
                });
                if i > 0 {
                    mesh.add_triangle(0, i as u32, (i + 1) as u32);
                }
            }
            c.painter.add(egui::Shape::mesh(mesh));
        } else {
            c.circle(cx, cy, r, mix(accent_a, accent_b, 0.42));
            paint::note(c, cx, cy, r * 0.72, Color32::WHITE);
        }
    }

    /// 计算 Glance 音乐控件当前形态对应的岛体宽高 `(width, height)`（需求 3：宽度统一为 `419.0`，与其他所有岛保持一致）。
    pub(crate) fn glance_music_target_dims(&self) -> (f32, f32) {
        let cfg = crate::auralsle_ai_status::config::get();
        let form = cfg.glance_music_form % 3;
        match form {
            0 => (419.0, 142.0),
            1 => (419.0, 188.0),
            _ => (419.0, 196.0),
        }
    }

    fn paint_music_ai_status_pill(&mut self, c: &Card) {
        if cfg!(test) || !crate::auralsle_ai_status::config::get().music_show_ai_status {
            return;
        }
        let Some(shown) = crate::auralsle_ai_status::shown() else {
            return;
        };
        let phase_col = match shown.agent.phase {
            crate::auralsle_ai_status::state::Phase::Waiting => Color32::from_rgb(251, 191, 36),
            crate::auralsle_ai_status::state::Phase::Error => Color32::from_rgb(248, 113, 113),
            crate::auralsle_ai_status::state::Phase::Done => Color32::from_rgb(52, 211, 153),
            _ => Color32::from_rgb(56, 189, 248),
        };
        let phase_txt = match shown.agent.phase {
            crate::auralsle_ai_status::state::Phase::Waiting => "等待确认",
            crate::auralsle_ai_status::state::Phase::Error => "异常",
            crate::auralsle_ai_status::state::Phase::Done => "已完成",
            _ => "运行中",
        };
        let label = format!("{} · {}", shown.agent.name, phase_txt);
        let txt = c.fit(&label, 8.6, W::Semibold, 118.0);
        let tw = c.text_w(&txt, 8.6, W::Semibold);
        let pw = tw + 22.0;
        let px = if self.auralsle_music_island_active() { crate::app::PW - pw - 16.0 } else { 366.0 - pw };
        let py = if self.auralsle_music_island_active() { 14.0 } else { 4.0 };
        let hit = c.hit(px, py, pw, 18.0, "music-ai-status-pill").on_hover_cursor(egui::CursorIcon::PointingHand);
        let from = crate::skin_paint::shape_count(c.ui.ctx(), c.painter.layer_id());
        c.fill(px, py, pw, 18.0, 9.0, Color32::from_rgba_unmultiplied(18, 22, 30, 188));
        c.stroke(px, py, pw, 18.0, 9.0, 0.8, paint::alpha(phase_col, 0.65));
        c.circle(px + 9.0, py + 9.0, 2.8, phase_col);
        c.text(px + 15.0, py + 12.2, &txt, 8.6, W::Semibold, theme::WHITE, H::Left);
        crate::skin_paint::tone_since(c.ui.ctx(), c.painter.layer_id(), from, |col, _| col);
        if hit.clicked() {
            let target = if crate::auralsle_ai_status::config::get().ai_island_scheme == 2 { 6 } else { 8 };
            self.show_page(target);
        }
    }

    pub(crate) fn page_music(&mut self, c: &Card, _env: &Env, cmds: &mut Vec<Cmd>) {
        if !cfg!(test) {
            match crate::auralsle_ai_status::config::get().music_ui_style {
                1 | 4 => {
                    self.page_music_eisland(c, _env, cmds);
                    return;
                }
                5 => {
                    self.page_music_glance(c, _env, cmds);
                    return;
                }
                2 | 3 | 6 => {
                    self.page_music_auralsle(c, _env, cmds);
                    return;
                }
                _ => {}
            }
        }
        let np=self.media.clone();
        let has=!np.title.is_empty();
        let with_lyrics=has && self.show_lyrics;
        let ctx=c.ui.ctx().clone();
        let playing_t=ctx.animate_bool_with_time(egui::Id::new("np-playing"),np.playing,0.22);
        let classic_tint_on = !cfg!(test) && crate::auralsle_ai_status::config::get().classic_music_cover_tint;
        let (classic_acc_a, classic_acc_b) = if has {
            self.extract_artwork_colors()
        } else {
            (theme::WHITE, theme::WHITE)
        };
        if classic_tint_on && has {
            // [需求 13]：经典分栏启用专辑封面取色双径向柔光背板与封面光晕
            c.glow(68.0, 52.0, 68.0, paint::alpha(classic_acc_a, 0.22 + 0.12 * playing_t));
            c.glow(295.0, 62.0, 84.0, paint::alpha(classic_acc_b, 0.16 + 0.10 * playing_t));
        }
        let art_size=if with_lyrics {44.0}else{62.0};
        let art_y=if with_lyrics {23.0}else{13.0};
        if let Some((image,texture))=&self.music_artwork {
            let [w,h]=image.size;
            let (uw,vh)=if w>h {(h as f32/w as f32,1.0)}else{(1.0,w as f32/h as f32)};
            let uv=Rect::from_center_size(Pos2::new(0.5,0.5),egui::vec2(uw,vh));
            c.painter.add(egui::epaint::RectShape::filled(c.rect(44.0,art_y,art_size,art_size),10.0*c.s,Color32::WHITE).with_texture(texture.id(),uv));
        } else if has {
            let (a,b)=cover_colors(&np.title);
            let cc=c.clipped(44.0,art_y,art_size,art_size);
            cc.vgrad(44.0,art_y,art_size,art_size,10.0,a,b);
            paint::note(&cc,44.0+art_size/2.0,art_y+art_size/2.0,art_size*0.39,Color32::from_white_alpha(235));
        } else {
            c.fill(44.0,13.0,62.0,62.0,13.0,Color32::from_rgb(25,25,27));
            paint::note(c,75.0,44.0,26.0,Color32::from_rgb(110,110,114));
        }
        if has && !cfg!(test) && c.hit(44.0, art_y, art_size, art_size, "classic-art-open").on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("点击跳转到音乐软件").clicked() {
            crate::auralsle_music::focus_player_source(&np.source);
        }

        if !has {
            c.text(121.0,47.0,"暂无音乐",14.0,W::Bold,theme::WHITE,H::Left);
        } else {
            let text_x=if with_lyrics {99.0}else{121.0};
            let text_width=if with_lyrics {123.0}else{220.0};
            let title_size=if with_lyrics {12.0}else{14.0};
            c.text(text_x,if with_lyrics {41.0}else{53.0},&c.fit(&np.title,title_size,W::Bold,text_width),title_size,W::Bold,theme::WHITE,H::Left);
            c.text(text_x,if with_lyrics {57.0}else{67.0},&c.fit(&np.artist,size::BODY,W::Regular,text_width),size::BODY,W::Regular,theme::GRAY,H::Left);
            if with_lyrics {self.draw_music_lyrics(c);}
        }

        let tc=if has {theme::WHITE}else{Color32::from_rgb(78,78,82)};
        let (bx,bw,by)=if with_lyrics {(44.0,176.0,82.0)}else{(96.0,221.0,92.6)};
        let bar=c.hit_drag(bx-4.0,by-7.0,bw+8.0,14.0,"m-seek");
        let seek_position=if has && np.duration>0.0 && (bar.is_pointer_button_down_on() || bar.clicked() || bar.drag_stopped()) {
            bar.interact_pointer_pos().or_else(||ctx.input(|i|i.pointer.interact_pos()))
                .map(|pointer|((c.to_local(pointer).x-bx)/bw).clamp(0.0,1.0) as f64*np.duration)
        }else{None};
        let pending_position=self.music_seek_ui.as_ref().filter(|(_,title)|*title==np.title).map(|(target,_)|*target);
        let restart=self.music_restart_at.filter(|at|self.t<at+1.5).map(|at|(self.t-at).max(0.0));
        let position=seek_position.or(pending_position).unwrap_or_else(||match restart {
            Some(fresh) if np.position>fresh+1.0=>fresh,
            _=>np.position,
        });
        let frac=if np.duration>0.0 {(position/np.duration).clamp(0.0,1.0) as f32}else{0.0};
        let (time_y,time_size,right)=if with_lyrics {(100.0,10.0,220.0)}else{(97.0,15.0,375.0)};
        c.text(44.0,time_y,&mmss(position),time_size,W::Regular,tc,H::Left);
        let remaining=if has && np.duration<=0.0{"--:--".into()}else{format!("-{}",mmss((np.duration-position).max(0.0)))};
        c.text(right,time_y,&remaining,time_size,W::Regular,tc,H::Right);
        let (bh,_)=c.hover_t(&bar);
        let bh=if has {bh}else{0.0};
        let th=lerp(3.5,5.0,bh);
        c.fill(bx,by-th/2.0,bw,th,th/2.0,if has {Color32::from_gray(90)}else{theme::TRACK});
        if self.settings.skin==crate::model::Skin::Fluid {
            let head=bx+bw*frac;
            let hot=if bar.is_pointer_button_down_on(){1.0}else if np.playing{0.62}else{0.45};
            if has && frac>0.0 {
                c.fill(bx,by-th/2.0,bw*frac,th,th/2.0,self.fluid.tint_color(0.78));
                paint::chase(c,bx,head,by,th,self.fluid.tint_color(0.30),hot);
            }
            if c.live {
                let pos=c.pos(head,by)-self.fluid_origin;
                self.fluid.set_seek((has && frac>0.0).then_some((pos.x,pos.y)),hot);
            }
        } else if !cfg!(test) {
            paint_themed_music_progress_bar(c, bx, by, bw, frac, classic_acc_a, np.playing, self.t);
        } else {
            if frac>0.0 {c.fill(bx,by-th/2.0,(bw*frac).max(th),th,th/2.0,theme::WHITE);}
            if bh>0.01 {c.faded(bh).circle(bx+bw*frac,by,4.5,theme::WHITE);}
        }
        if bar.clicked() || bar.drag_stopped() {
            if let Some(position)=seek_position {
                self.music_seek_ui=Some((position,np.title.clone()));
                self.music_seek_quiet_until=self.t+2.5;
                cmds.push(Cmd::Media(Transport::Seek(position)));
            }
        }

        let y=118.0;
        let ic=if has {theme::WHITE}else{Color32::from_rgb(88,88,92)};
        if !cfg!(test) {
            crate::auralsle_ai_status::ui::paint_glare_hover(c, 44.0, art_y, art_size, art_size, 10.0, Color32::from_rgb(244, 114, 182));
        }
        let cur_local = self.cursor_local;
        let btn=|x:f32,w:f32,id:&str| {
            let response=c.hit(x-w/2.0,y-12.0,w,24.0,id);
            let (hover,pressed)=c.hover_t(&response);
            let mag = if cfg!(test) { egui::Vec2::ZERO } else { crate::auralsle_ai_status::ui::magnet_offset(cur_local, Pos2::new(x, y), 24.0, 3.2) };
            if has && hover>0.01 {crate::auralsle_music::paint_music_trigger_circle(&c.faded(hover), x + mag.x, y + mag.y, 12.5, classic_acc_a, hover, false);}
            (response,c.shifted(mag.x, mag.y).scaled(x,y,1.0-0.1*pressed))
        };
        let (lyrics_button,lyrics_card)=btn(74.0,26.0,"m-lyr");
        lyrics_card.text(74.0,y+5.0,"词",14.0,W::Semibold,if !has {ic}else if self.show_lyrics {theme::WHITE}else{theme::GRAY},H::Center);
        let (previous,previous_card)=btn(162.0,30.0,"m-prev");
        paint::skip(&previous_card,162.0,y,-1.0,ic);
        let (play,play_card)=btn(210.5,30.0,"m-play");
        if cfg!(test) {
            paint::play_pause(&play_card,211.0,y,15.0,playing_t,ic);
        } else {
            let (p_hov, p_press) = c.hover_t(&play);
            paint_music_play_button(c, 211.0, y, 30.0, 24.0, p_hov, p_press, playing_t, classic_acc_a, 13.0);
        }
        let (next,next_card)=btn(258.0,30.0,"m-next");
        paint::skip(&next_card,258.0,y,1.0,ic);
        let (volume_button,volume_card)=btn(367.0,26.0,"m-volume");
        let (volume,muted)=self.music_volume_ui.unwrap_or((0,false));
        paint::speaker(&volume_card,367.0,y,muted,if self.music_volume_ui.is_some(){ic}else{theme::GRAY});
        let hint=if self.music_volume_ui.is_some(){format!("音乐应用音量 {volume}%{} · 点击展开 · 滚轮调节",if muted{"（已静音）"}else{""})}else{"暂无可调节的音乐音频会话".into()};
        let volume_button=volume_button.on_hover_text(hint);
        if c.live && has && self.music_volume_ui.is_some() {
            if volume_button.clicked(){self.music_volume_open=!self.music_volume_open;}
            if volume_button.hovered(){
                    let delta=c.ui.ctx().input(|i|i.raw_scroll_delta.y);
                    if delta.abs()>0.5 {
                        self.change_music_volume(Transport::SetVolume((volume as f32+delta/120.0*4.0).round().clamp(0.0,100.0) as u32),cmds);
                    }
            }
        }
        if has {
            if previous.clicked() {self.music_restart_at=Some(self.t);self.music_seek_ui=None;cmds.push(Cmd::Media(Transport::Prev));}
            if play.clicked() {cmds.push(Cmd::Media(Transport::Toggle));}
            if next.clicked() {self.music_restart_at=Some(self.t);self.music_seek_ui=None;cmds.push(Cmd::Media(Transport::Next));}
        }
        if lyrics_button.on_hover_text(if self.show_lyrics {"隐藏歌词"}else{"显示歌词"}).clicked() {
            if !has {self.say("先放一首歌吧");}
            else {
                self.show_lyrics=!self.show_lyrics;
                self.settings.lyrics_enabled=self.show_lyrics;
                self.settings.save();
            }
        }
        // 岛上歌词是横向的，只在顶部停靠时提供。
        if has && !self.side() {
            let pill_rest = if self.settings.island_lyrics {
                crate::auralsle_music::music_pause_pill_fill(classic_acc_a, 0.35, true)
            } else {
                crate::auralsle_music::music_pause_pill_fill(classic_acc_a, 0.0, true)
            };
            let pill_hov = crate::auralsle_music::music_pause_pill_fill(classic_acc_a, 1.0, true);
            let (button,button_card)=capsule(c,279.0,108.0,68.0,20.0,"m-island-lyrics",pill_rest,pill_hov);
            button_card.text(313.0,122.0,"岛上歌词",10.0,W::Semibold,if self.settings.island_lyrics {theme::WHITE}else{theme::GRAY},H::Center);
            if button.on_hover_text(if self.settings.island_lyrics {"关闭收起时的歌词"}else{"收起时显示当前歌词"}).clicked() {
                self.settings.island_lyrics=!self.settings.island_lyrics;
                self.settings.save();
            }
        }
    }

    fn change_music_volume(&mut self,command:Transport,cmds:&mut Vec<Cmd>){
        let Some((volume,_))=self.music_volume_ui else{return;};
        let next=match command{Transport::SetVolume(v)=>(v,false),Transport::SetMute(muted)=>(volume,muted),_=>return};
        if self.music_volume_ui==Some(next){return;}
        self.music_volume_ui=Some(next);
        self.music_volume_quiet_until=self.t+0.9;
        cmds.push(Cmd::MusicVolume(command,self.media.source.clone()));
    }

    /// Auralsle 灵屿音乐岛（完整复刻排版插值、自定义布局、封面磨砂取色、发光歌词、8 款进度条与 10 套播控主题）。
    fn page_music_auralsle(&mut self, c: &Card, _env: &Env, cmds: &mut Vec<Cmd>) {
        use crate::auralsle_music::{self, BarStyle, CardInput, ThumbStyle, TransportTheme};

        let ctx = c.ui.ctx().clone();
        let cfg = crate::auralsle_ai_status::config::get();
        let np = self.media.clone();
        let has = !np.title.trim().is_empty();
        let w = crate::app::PW;

        let (geometry, custom_opt) = auralsle_music::active_geometry(&cfg, w, self.auralsle_music_mix);
        let bar_style = custom_opt
            .as_ref()
            .map(|l| BarStyle::from_setting(&l.bar_style))
            .unwrap_or_else(|| BarStyle::from_setting(&cfg.auralsle_bar_style));
        let thumb_en = custom_opt
            .as_ref()
            .map(|l| l.thumb_enabled)
            .unwrap_or(cfg.auralsle_thumb_enabled);
        let thumb_style = if thumb_en {
            custom_opt
                .as_ref()
                .map(|l| ThumbStyle::from_setting(&l.thumb_style))
                .unwrap_or_else(|| ThumbStyle::from_setting(&cfg.auralsle_thumb_style))
        } else {
            ThumbStyle::None
        };
        let thumb_spin = custom_opt
            .as_ref()
            .map(|l| l.thumb_spin)
            .unwrap_or(cfg.auralsle_thumb_spin);
        let transport_theme = custom_opt
            .as_ref()
            .map(|l| TransportTheme::from_setting(&l.transport_theme))
            .unwrap_or_else(|| TransportTheme::from_setting(&cfg.auralsle_transport_theme));

        // 提取当前歌词、译文/下一句、逐字进度与换句平滑上浮 phase
        let (cur_line, sec_line, highlight, line_idx) = if has && self.show_lyrics {
            if let Some(lyrics) = &self.ly.lyrics {
                if !lyrics.lines.is_empty() {
                    let pos = self.lyric_position();
                    let idx = if lyrics.synced {
                        lyrics.index_at(pos).unwrap_or(0)
                    } else {
                        ((pos.max(0.0) / 4.0) as usize) % lyrics.lines.len()
                    };
                    let cur_s = lyrics
                        .lines
                        .get(idx)
                        .map(|l| l.text.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "···".to_owned());
                    let trans_s = if cfg.auralsle_show_translation {
                        lyrics
                            .lines
                            .get(idx)
                            .and_then(|l| l.translation.as_ref())
                            .map(|s| s.trim().to_owned())
                            .filter(|s| !s.is_empty())
                    } else {
                        None
                    };
                    let next_s = if cfg.auralsle_chinese_two_line {
                        lyrics
                            .lines
                            .iter()
                            .skip(idx + 1)
                            .find(|l| !l.text.trim().is_empty())
                            .map(|l| l.text.trim().to_owned())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let secondary = trans_s.unwrap_or(next_s);
                    let cur_t0 = lyrics.lines.get(idx).map(|l| l.t).unwrap_or(0.0);
                    let cur_t1 = lyrics
                        .lines
                        .get(idx + 1)
                        .map(|l| l.t)
                        .unwrap_or((cur_t0 + 3.5).max(np.duration as f32));
                    let prog = if lyrics.synced {
                        ((pos - cur_t0) / (cur_t1 - cur_t0).max(0.4)).clamp(0.0, 1.0)
                    } else {
                        0.88
                    };
                    (cur_s, secondary, prog, idx)
                } else {
                    ("等待歌词同步".to_owned(), String::new(), 1.0, 0)
                }
            } else {
                ("正在匹配同步歌词…".to_owned(), String::new(), 1.0, 0)
            }
        } else if has {
            ("歌词已关闭".to_owned(), "点击右下角设置或切换开启".to_owned(), 1.0, 0)
        } else {
            (String::new(), String::new(), 1.0, 0)
        };

        let age_id = egui::Id::new("auralsle-lyric-age-idx");
        let age_t0_id = egui::Id::new("auralsle-lyric-age-t0");
        let prev_idx = ctx.data(|d| d.get_temp::<usize>(age_id));
        if prev_idx != Some(line_idx) {
            ctx.data_mut(|d| {
                d.insert_temp(age_id, line_idx);
                d.insert_temp(age_t0_id, self.t);
            });
        }
        let changed_at = ctx.data(|d| d.get_temp::<f64>(age_t0_id)).unwrap_or(self.t - 1.0);
        let age_phase = ((self.t - changed_at).max(0.0) / 0.18).clamp(0.0, 1.0) as f32;

        let pending_position = self
            .music_seek_ui
            .as_ref()
            .filter(|(_, title)| *title == np.title)
            .map(|(target, _)| *target);
        let restart = self
            .music_restart_at
            .filter(|at| self.t < at + 1.5)
            .map(|at| (self.t - at).max(0.0));
        let position = pending_position.unwrap_or_else(|| match restart {
            Some(fresh) if np.position > fresh + 1.0 => fresh,
            _ => np.position,
        });

        let fallback_tint = if has {
            Some(cover_colors(&np.title).0)
        } else {
            None
        };

        let live_h = self.h.pos.max(24.0);
        let live_w = self.w.pos.max(w);
        let base_r = if self.settings.island_shape == crate::model::IslandShape::Pill { 26.0 } else { 14.0 };
        let card_radius = (live_w.min(live_h) * 0.5).min(base_r);
        let events = auralsle_music::paint_auralsle_music_card(
            c,
            &cfg,
            &mut self.auralsle_blur_cache,
            &CardInput {
                width: w,
                surface_height: live_h,
                card_radius,
                time_secs: self.t,
                title: &np.title,
                artist: &np.artist,
                has_track: has,
                playing: np.playing,
                position,
                duration: np.duration,
                lyric: &cur_line,
                secondary: &sec_line,
                highlight,
                lyric_age_phase: age_phase,
                artwork: self.music_artwork.as_ref(),
                artwork_tint: fallback_tint,
                volume_level: self.music_volume_ui,
                liked: self.auralsle_music_liked,
                shuffle_mode: self.auralsle_music_shuffle,
                geometry,
                bar_style,
                thumb_style,
                thumb_spin,
                transport_theme,
                allow_seek_and_buttons: true,
                skip_surface: !cfg!(test),
                id_salt: "auralsle-live-card",
            },
        );

        if let Some(pos_seek) = events.seek_to {
            if has && np.duration > 0.0 {
                self.music_seek_ui = Some((pos_seek, np.title.clone()));
                self.music_seek_quiet_until = self.t + 2.5;
                cmds.push(Cmd::Media(Transport::Seek(pos_seek)));
            }
        }
        if let Some(delta) = events.volume_wheel_delta {
            if let Some((vol, _)) = self.music_volume_ui {
                self.change_music_volume(
                    Transport::SetVolume((vol as f32 + delta / 120.0 * 4.0).round().clamp(0.0, 100.0) as u32),
                    cmds,
                );
            }
        }
        if events.header_clicked && has {
            auralsle_music::focus_player_source(&np.source);
        }
        if let Some(bid) = events.clicked_button {
            match bid {
                2 => {
                    self.auralsle_music_shuffle = self.auralsle_music_shuffle.wrapping_add(1) % 2;
                    self.say(if self.auralsle_music_shuffle == 0 {
                        "已切换：随机播放模式"
                    } else {
                        "已切换：顺序循环模式"
                    });
                }
                9 => {
                    self.auralsle_music_liked = !self.auralsle_music_liked;
                    self.say(if self.auralsle_music_liked {
                        "已标记喜欢"
                    } else {
                        "已取消喜欢"
                    });
                }
                16 => {
                    if self.music_volume_ui.is_some() {
                        self.music_volume_open = !self.music_volume_open;
                    } else {
                        self.say("暂无可调节的音乐音频会话");
                    }
                }
                3 if has => {
                    self.music_restart_at = Some(self.t);
                    self.music_seek_ui = None;
                    cmds.push(Cmd::Media(Transport::Prev));
                }
                4 if has => {
                    cmds.push(Cmd::Media(Transport::Toggle));
                }
                5 if has => {
                    self.music_restart_at = Some(self.t);
                    self.music_seek_ui = None;
                    cmds.push(Cmd::Media(Transport::Next));
                }
                6 => {
                    let next_tr = !cfg.auralsle_show_translation;
                    crate::auralsle_ai_status::config::update(|c| c.auralsle_show_translation = next_tr);
                }
                7 => {
                    self.settings_tab = 1;
                    self.open_settings();
                }
                _ => {}
            }
        }

        // 在 Auralsle 音量按钮上方弹出紧凑纵向音量条
        if let Some((_, vol_r)) = geometry.buttons.iter().find(|(id, _)| *id == 16) {
            if vol_r.w > 1.0 && self.music_volume_ui.is_some() {
                let a = ctx.animate_bool_with_time(
                    egui::Id::new("auralsle-music-vol-open"),
                    self.music_volume_open,
                    0.12,
                );
                let trigger = c.rect(vol_r.x, vol_r.y, vol_r.w, vol_r.h);
                let pw = 42.0;
                let ph = 98.0;
                let px = (vol_r.x + vol_r.w * 0.5 - pw * 0.5).clamp(10.0, w - pw - 10.0);
                let py = (vol_r.y - ph - 6.0).max(8.0);
                let panel_rect = c.rect(px, py, pw, ph);
                if self.music_volume_open
                    && ctx.input(|i| {
                        i.pointer.any_pressed()
                            && i.pointer
                                .interact_pos()
                                .is_some_and(|p| !panel_rect.contains(p) && !trigger.contains(p))
                    })
                {
                    self.music_volume_open = false;
                }
                if a > 0.01 {
                    let (volume, muted) = self.music_volume_ui.unwrap_or((50, false));
                    let popup = c.faded(a);
                    let popup = if self.music_volume_open { popup } else { popup.ghost() };
                    popup.hit(px, py, pw, ph, "auralsle-vol-surface");
                    popup.fill(px, py, pw, ph, 12.0, Color32::from_rgba_unmultiplied(30, 35, 42, 242));
                    popup.stroke(px, py, pw, ph, 12.0, 1.0, Color32::from_white_alpha(38));
                    let tr_top = py + 24.0;
                    let tr_h = 42.0;
                    let track = popup.hit_drag(px + 6.0, tr_top - 4.0, pw - 12.0, tr_h + 8.0, "auralsle-vol-track");
                    let mut cmd = None;
                    if popup.live && (track.clicked() || track.dragged() || track.is_pointer_button_down_on()) {
                        if let Some(pt) = track.interact_pointer_pos() {
                            let ly = popup.to_local(pt).y;
                            let pct = ((tr_top + tr_h - ly) / tr_h * 100.0).round().clamp(0.0, 100.0) as u32;
                            cmd = Some(Transport::SetVolume(pct));
                        }
                    }
                    let mute_btn = popup.hit(px + 6.0, py + ph - 24.0, pw - 12.0, 18.0, "auralsle-vol-mute");
                    if popup.live && mute_btn.clicked() {
                        cmd = Some(Transport::SetMute(!muted));
                    }
                    if let Some(command) = cmd {
                        self.change_music_volume(command, cmds);
                    }
                    let (v_now, m_now) = self.music_volume_ui.unwrap_or((volume, muted));
                    let ink = if m_now { theme::GRAY } else { Color32::from_rgb(152, 229, 194) };
                    popup.centered_text(px + pw * 0.5, py + 12.0, &format!("{v_now}%"), 10.0, W::Semibold, theme::WHITE, H::Center);
                    popup.fill(px + pw * 0.5 - 2.5, tr_top, 5.0, tr_h, 2.5, Color32::from_gray(68));
                    let head_y = tr_top + tr_h * (1.0 - v_now as f32 / 100.0);
                    if v_now > 0 {
                        popup.fill(px + pw * 0.5 - 2.5, head_y, 5.0, (tr_top + tr_h - head_y).max(3.0), 2.5, ink);
                    }
                    popup.circle(px + pw * 0.5, head_y, if track.dragged() { 5.0 } else { 4.2 }, theme::WHITE);
                    popup.fill(px + 6.0, py + ph - 24.0, pw - 12.0, 18.0, 6.0, Color32::from_white_alpha(20));
                    popup.centered_text(px + pw * 0.5, py + ph - 15.0, if m_now { "恢复" } else { "静音" }, 9.0, W::Semibold, theme::WHITE, H::Center);
                }
            }
        }
    }

    pub(crate) fn draw_music_volume(&mut self,c:&Card,cmds:&mut Vec<Cmd>){
        let ctx=c.ui.ctx();
        let bounds=MUSIC_VOLUME_RECT;
        let panel=c.rect(bounds.min.x,bounds.min.y,bounds.width(),bounds.height());
        let trigger=c.rect(354.0,106.0,26.0,24.0);
        if self.music_volume_open && ctx.input(|i|i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p|!panel.contains(p) && !trigger.contains(p))){
            self.music_volume_open=false;
        }
        let a=ctx.animate_bool_with_time(egui::Id::new("music-volume-open"),self.music_volume_open,0.12);
        let Some((volume,muted))=self.music_volume_ui else{return;};
        if a<0.01{return;}
        let popup=c.faded(a);
        let popup=if self.music_volume_open{popup}else{popup.ghost()};
        // 浮条最后绘制并占据整块命中区域，防止点击穿到后面的翻页按钮。
        popup.hit(bounds.min.x,bounds.min.y,bounds.width(),bounds.height(),"m-volume-surface");
        popup.fill(bounds.min.x,bounds.min.y,bounds.width(),bounds.height(),12.0,Color32::from_gray(36));
        popup.stroke(bounds.min.x,bounds.min.y,bounds.width(),bounds.height(),12.0,1.0,Color32::from_gray(60));
        let track=popup.hit_drag(351.0,27.0,32.0,54.0,"m-volume-track");
        let mut command=None;
        if popup.live && (track.clicked() || track.dragged() || track.is_pointer_button_down_on()) {
            if let Some(pointer)=track.interact_pointer_pos(){
                command=Some(Transport::SetVolume(((75.0-popup.to_local(pointer).y)/43.0*100.0).round().clamp(0.0,100.0) as u32));
            }
        }else if popup.live && panel.contains(ctx.pointer_hover_pos().unwrap_or(Pos2::ZERO)) {
            let delta=ctx.input(|i|i.raw_scroll_delta.y);
            if delta.abs()>0.5{command=Some(Transport::SetVolume((volume as f32+delta/120.0*4.0).round().clamp(0.0,100.0) as u32));}
        }
        let mute=popup.hit(353.0,82.0,28.0,20.0,"m-volume-mute").on_hover_text(if muted{"恢复音乐音量"}else{"静音音乐，保留当前音量"});
        let (hover,press)=popup.hover_t(&mute);
        if popup.live && mute.clicked(){command=Some(Transport::SetMute(!muted));}
        if let Some(command)=command {self.change_music_volume(command,cmds);}
        let (volume,muted)=self.music_volume_ui.unwrap_or((volume,muted));
        let ink=if muted{theme::GRAY}else{theme::WHITE};
        popup.centered_text(367.0,20.0,&format!("{volume}%"),10.5,W::Semibold,ink,H::Center);
        popup.fill(364.5,32.0,5.0,43.0,2.5,Color32::from_gray(68));
        let head=75.0-43.0*volume as f32/100.0;
        if volume>0 {popup.fill(364.5,head,5.0,(75.0-head).max(2.5),2.5,ink);}
        popup.circle(367.0,head,if track.dragged(){5.5}else{4.5},ink);
        popup.fill(353.0,82.0,28.0,20.0,6.0,paint::mix(theme::BTN,theme::BTN_HOVER,if muted{1.0}else{hover}));
        popup.scaled(367.0,92.0,1.0-0.06*press).centered_text(367.0,92.0,if muted{"恢复"}else{"静音"},9.5,W::Semibold,theme::WHITE,H::Center);
    }

    fn draw_music_lyrics(&mut self,c:&Card) {
        let col=c.clipped(243.0,28.0,136.0,74.0);
        c.seg((233.0,32.0),(233.0,98.0),1.0,Color32::from_gray(35));
        if let Some(lyrics)=&self.ly.lyrics {
            let current=if lyrics.synced {lyrics.index_at(self.lyric_position())}else{Some(0)};
            let target=current.unwrap_or(0);
            let id=egui::Id::new("music-lyrics-scroll");
            if !self.ly.scroll_animation_started {
                c.ui.ctx().animate_value_with_time(id,target as f32,0.0);
                self.ly.scroll_animation_started=true;
            }
            let position=c.ui.ctx().animate_value_with_time(id,target as f32,0.3);
            let first=(position.floor() as usize).saturating_sub(2);
            for (index,line) in lyrics.lines.iter().enumerate().skip(first).take(6) {
                let distance=index as f32-position;
                if distance.abs()>2.2 {continue;}
                let active=current==Some(index);
                let ink=if active {
                    if !cfg!(test) && crate::auralsle_ai_status::config::get().classic_music_cover_tint {
                        let (ca, _) = self.extract_artwork_colors();
                        mix(ca, theme::WHITE, 0.45)
                    } else {
                        theme::WHITE
                    }
                } else {
                    theme::GRAY
                };
                let row=col.faded((1.0-distance.abs()*0.35).clamp(0.0,1.0));
                let mut job=egui::text::LayoutJob::simple(line.text.clone(),row.font(if active {12.0}else{10.0},if active {W::Semibold}else{W::Regular}),ink,132.0*c.s);
                job.wrap.max_rows=if active {3}else{1};
                job.wrap.break_anywhere=true;
                let galley=row.painter.layout_job(job);
                let y=64.0+distance*29.0-galley.size().y/(2.0*c.s);
                row.painter.galley(row.pos(246.0,y),galley,ink);
            }
            if !lyrics.synced {col.text(246.0,39.0,"文本歌词",8.0,W::Regular,theme::GRAY,H::Left);}
        } else {
            let status=if self.ly.loading {"正在查找歌词…"}else if let Some(error)=&self.ly.error {error.label()}else{"暂无歌词"};
            let galley=col.painter.layout(status.into(),col.font(10.0,W::Regular),theme::GRAY,130.0*c.s);
            col.painter.galley(col.pos(246.0,61.0-galley.size().y/(2.0*c.s)),galley,theme::GRAY);
            if self.ly.error.is_some() {
                col.text(246.0,87.0,"点击重试",9.0,W::Regular,theme::GRAY,H::Left);
                if c.hit(243.0,28.0,136.0,74.0,"lyrics-retry").on_hover_text("重新查询当前歌曲的歌词").clicked(){self.ly.key.clear();self.ly.manual_retry=true;}
            }
        }
    }

    // ================= 专注（番茄钟） =================
    pub(crate) fn page_focus(&mut self, c: &Card, _env: &Env) {
        let set_x = if cfg!(test) {
            90.0
        } else if crate::auralsle_ai_status::config::get().show_island_page_title {
            108.0
        } else {
            40.0
        };
        if c.hit(set_x,8.0,42.0,21.0,"focus-settings").on_hover_text("专注设置").clicked(){self.open_settings_category(2);}
        c.text(set_x + 21.0,23.0,"设置",10.5,W::Regular,theme::GRAY,H::Center);
        let c_shifted = c.shifted(0.0, if cfg!(test) { 0.0 } else { 8.0 });
        let c = &c_shifted;
        let (rx, ry, rr) = (79.0, 74.0, 32.0);
        let frac = self.ring_shown.pos.clamp(0.0, 1.0);
        let run = c.ui.ctx().animate_bool_with_time(egui::Id::new("pomo-run"), self.pomo.running, 0.3);
        let resting=self.pomo.phase!=Phase::Focus;
        let (top,bottom)=if resting{(theme::SOFT_GREEN,mix(theme::SOFT_GREEN,Color32::BLACK,0.2))}else{(theme::RED_TOP,theme::RED)};
        // 外圈柔光；计时中缓慢呼吸
        let breathe = 0.5 + 0.5 * ((self.t * 2.2).sin() as f32);
        let glow = 0.30 + 0.25 * run * breathe;
        c.glow(rx, ry, 48.0, paint::alpha(bottom, glow * 0.55));
        c.ring(rx, ry, rr, 7.0, if resting{mix(theme::CARD,top,0.18)}else{Color32::from_rgb(58,22,26)});
        // 渐变进度环随当前阶段配色。
        if frac > 0.001 {
            let steps = (120.0 * frac).ceil().max(2.0) as usize;
            let a0 = -FRAC_PI_2;
            let mut prev = (rx + rr * a0.cos(), ry + rr * a0.sin());
            for i in 1..=steps {
                let a = a0 + TAU * frac * i as f32 / steps as f32;
                let next = (rx + rr * a.cos(), ry + rr * a.sin());
                let t = ((prev.1 + next.1) * 0.5 - (ry - rr)) / (2.0 * rr);
                c.seg(prev, next, 7.0, mix(top, bottom, t));
                prev = next;
            }
            // 圆头
            c.circle(rx, ry - rr, 3.5, top);
            c.circle(prev.0, prev.1, 3.5, mix(top, bottom, (prev.1 - (ry - rr)) / (2.0 * rr)));
        }
        let hand = if self.pomo.running { (self.t as f32 * 1.2) % TAU } else { 0.0 };
        if resting {
            let t=if self.pomo.running && crate::bar_layout::animations_enabled(){self.t as f32}else{0.0};
            crate::focus_bar::tea(&c.scaled(rx,ry,2.2),rx,ry,frac,t);
        }else{paint::stopwatch(c, rx, ry + 0.5, 15.5, hand * run, theme::WHITE);}

        let secs = self.pomo.remaining.max(0.0).ceil() as u32;
        c.text(264.0, 61.0, &format!("{:02}:{:02}", secs / 60, secs % 60), size::HERO + 1.0, W::Bold, if resting{theme::SOFT_GREEN}else{theme::WHITE}, H::Center);
        c.text(264.0, 79.0, self.pomo.phase.label(), size::SMALL, W::Regular, theme::GRAY, H::Center);

        // 重置：点一下图标转一圈
        let r = c.hit(317.0, 38.0, 24.0, 24.0, "f-reset");
        let (h, p) = c.hover_t(&r);
        let rc = c.scaled(329.0, 50.0, 1.0 - 0.1 * p);
        rc.circle(329.0, 50.0, 11.0, mix(Color32::from_rgb(28, 28, 30), Color32::from_rgb(50, 50, 54), h));
        paint::reset(&rc, 329.0, 50.0, -self.reset_spin.pos, Color32::from_gray(230));
        if r.clicked() {
            self.pomo.remaining = self.phase_secs(self.pomo.phase);
            self.pomo.running = false;
            self.reset_spin.target += TAU;
        }

        for (phase,label,x,width,id) in [(Phase::Focus,"专注",148.0,88.0,"f-focus"),(Phase::Rest,"休息",292.0,88.0,"f-rest")] {
            let selected=self.pomo.phase==phase;
            let accent=if phase==Phase::Focus{theme::RED_TOP}else{theme::SOFT_GREEN};
            let background=if selected{mix(theme::BTN,accent,0.22)}else{theme::BTN};
            let (response,button)=capsule(c,x,94.0,width,25.0,id,background,if selected{mix(theme::BTN_HOVER,accent,0.25)}else{theme::BTN_HOVER});
            button.text(x+width/2.0,110.0,label,size::BODY,W::Semibold,if selected{mix(accent,theme::WHITE,0.5)}else{theme::WHITE},H::Center);
            if response.clicked(){self.set_phase(phase);}
        }
        let (b2, c2) = capsule(c, 246.0, 94.0, 36.0, 25.0, "f-start", theme::RED, Color32::from_rgb(255, 82, 98));
        paint::play_pause(&c2, 264.0, 106.5, 11.0, run, theme::WHITE);
        let b2=b2.on_hover_text(if self.pomo.running{"暂停计时"}else{"开始计时"});
        if b2.clicked() {
            if self.pomo.remaining <= 0.0 {
                self.pomo.remaining = self.phase_secs(self.pomo.phase);
            }
            self.pomo.running = !self.pomo.running;
        }
    }

    fn set_phase(&mut self, ph: Phase) {
        if self.pomo.phase==ph {return;}
        self.pomo.phase = ph;
        self.pomo.remaining = self.phase_secs(ph);
        self.pomo.running = false;
    }

    // ================= 饮水 =================
    fn next_hint(&self, env: &Env) -> String {
        Self::clock_after(env, self.settings.water_minutes as f64 * 60.0 - (self.t - self.last_reminder))
    }
    /// 从现在起 secs 秒后的时钟读数（HH:MM）。
    fn clock_after(env: &Env, secs: f64) -> String {
        let total = env.hour() as u32 * 60 + env.minute() + secs.max(0.0) as u32 / 60;
        format!("{:02}:{:02}", (total / 60) % 24, total % 60)
    }

    /// 健康页：标题「健康」后接「喝水 | 久坐」分段切换。两种模式是同一套版式——
    /// 大数字、右上开关、状态行、进度条、主按钮 + 设置、右侧进度环都在同一位置，
    /// 只换内容和主色（蓝 / 绿）。设置按钮、分隔线和开关本体不随切换闪动。
    pub(crate) fn page_water(&mut self, c: &Card, env: &Env) {
        let sv = self.sit_view_t.pos.clamp(0.0, 1.0);
        let h_x = if cfg!(test) { 15.0 } else { 40.0 };
        let c_hdr = if cfg!(test) { c.clone() } else { c.shifted(0.0, -14.0) };
        self.health_tabs(&c_hdr, h_x, sv, cfg!(test));
        let c_shifted = c.shifted(0.0, if cfg!(test) { 0.0 } else { 8.0 });
        let c = &c_shifted;

        // 共用的开关：位置不变，颜色和旋钮图标随模式过渡。
        let sit = self.sit_mode;
        let sw = c.hit(160.0, 35.0, 38.0, 22.0, "w-remind")
            .on_hover_text(match (sit, if sit { self.settings.sit_remind } else { self.water.remind }) {
                (false, true) => "关闭喝水提醒", (false, false) => "打开喝水提醒",
                (true, true) => "关闭久坐提醒", (true, false) => "打开久坐提醒",
            });
        let (sh, _) = c.hover_t(&sw);
        let on = lerp(self.remind_t.pos, self.sit_t.pos, sv);
        let accent = mix(theme::BLUE, theme::SIT_GREEN, sv);
        let track = mix(Color32::from_rgb(58, 58, 62), accent, on.clamp(0.0, 1.0));
        c.fill(162.0, 36.0, 34.0, 20.0, 10.0, mix(track, theme::WHITE, sh * 0.08));
        let kx = lerp(172.0, 186.0, on);
        c.circle(kx, 46.0, 8.0, theme::WHITE);
        let ink = mix(Color32::from_gray(140), accent, on.clamp(0.0, 1.0));
        paint::bell(&c.scaled(kx, 46.0, 0.9), kx, 46.0, ink, 0.0);
        if sw.clicked() {
            if sit {
                self.settings.sit_remind = !self.settings.sit_remind;
                self.settings.save();
                self.confirm_health_reminder(crate::extras::HealthReminder::Sit);
                self.say(if self.settings.sit_remind { "久坐提醒已打开" } else { "久坐提醒已关闭" });
            } else {
                self.water.remind = !self.water.remind;
                self.water.save();
                self.confirm_health_reminder(crate::extras::HealthReminder::Water);
                self.say(if self.water.remind { "喝水提醒已打开" } else { "喝水提醒已关闭" });
            }
        }

        let (s, sc) = capsule(c, 111.0, 98.0, 66.0, 25.0, "w-set", theme::BTN, theme::BTN_HOVER);
        paint::sliders(&sc, 126.0, 110.5, theme::WHITE);
        sc.text(149.0, 114.0, "设置", size::BODY, W::Regular, theme::WHITE, H::Center);
        if s.clicked() { self.open_settings_category(1); }
        c.fill(208.0, 36.0, 1.0, 86.0, 0.5, Color32::from_rgb(30, 30, 33));

        // 内容原地交叉淡化，不滑动；不可见的一侧不接收点击。
        if sv < 0.99 {
            let wc = c.faded(1.0 - sv);
            self.water_body(&if sit { wc.ghost() } else { wc }, env);
        }
        if sv > 0.01 {
            let sc = c.faded(sv);
            self.sit_body(&if sit { sc } else { sc.ghost() }, env);
        }
    }

    /// 标题「健康提醒」（「提醒」随模式变蓝 / 绿）与紧跟其后的分段切换：水滴 = 喝水，椅子 = 久坐。
    /// 胶囊外框与标题字等高（11..27），读起来像标题的一部分。设置子页的标题也用它。
    pub(crate) fn health_tabs(&mut self, c: &Card, x: f32, sv: f32, show_title: bool) {
        let accent_now = mix(theme::BLUE, theme::SIT_GREEN, sv);
        let x0 = if show_title {
            let wa = c.text(x, 25.0, "健康", size::TITLE, W::Bold, theme::WHITE, H::Left);
            let wb = c.text(x + wa + 1.5, 25.0, "提醒", size::TITLE, W::Bold, accent_now, H::Left);
            x + wa + 1.5 + wb + 7.0
        } else {
            let wa = c.text_w("健康", size::TITLE, W::Bold);
            let wb = c.text_w("提醒", size::TITLE, W::Bold);
            x + wa + 1.5 + wb + 7.0
        };
        let (w, y, h) = (22.0, 11.0, 16.0);
        c.fill(x0, y, w * 2.0 + 3.0, h, h / 2.0, Color32::from_rgb(30, 30, 33));
        let px = lerp(x0 + 1.5, x0 + 1.5 + w, sv);
        let pill = mix(Color32::from_rgb(22, 62, 108), Color32::from_rgb(26, 84, 54), sv);
        c.fill(px, y + 1.5, w, h - 3.0, (h - 3.0) / 2.0, pill);
        for i in 0..2 {
            let ix = x0 + 1.5 + i as f32 * w;
            let r = c.hit(ix, y - 3.0, w, h + 6.0, ("health-tab", i))
                .on_hover_text(if i == 0 { "喝水提醒" } else { "久坐提醒" });
            let (hv, _) = c.hover_t(&r);
            let act = if i == 0 { 1.0 - sv } else { sv };
            let accent = if i == 0 { theme::BLUE } else { theme::SIT_GREEN };
            let icon = mix(mix(Color32::from_gray(110), Color32::from_gray(170), hv), mix(accent, theme::WHITE, 0.25), act);
            if i == 0 {
                paint::droplet(c, ix + w / 2.0, y + h / 2.0, 8.5, icon);
            } else {
                paint::chair(c, ix + w / 2.0, y + h / 2.0, 0.34, icon);
            }
            if r.clicked() { self.sit_mode = i == 1; }
        }
    }

    fn water_body(&mut self, c: &Card, env: &Env) {
        let pct = self.water.percent();
        let liters = self.water_ml / 1000.0;
        let pending = self.health_pending[crate::extras::HealthReminder::Water as usize];
        if pending {
            c.text(44.0, 59.0, "待喝水", 24.0, W::Bold, theme::BLUE, H::Left);
        } else {
            let ww = c.text(44.0, 61.0, &format!("{liters:.1}"), size::HERO, W::Bold, theme::WHITE, H::Left);
            c.text(44.0 + ww + 5.0, 60.0, &format!("/ {:.1} L", self.water.goal_ml as f32 / 1000.0), 11.0, W::Semibold, theme::GRAY, H::Left);
        }

        // 今天 · 3% · 下次 08:07
        let mut x = 44.0;
        x += c.text(x, 77.0, "今天 · ", size::SMALL, W::Regular, theme::GRAY, H::Left);
        x += c.text(x, 77.0, &format!("{pct:.0}%"), size::SMALL, W::Bold, theme::BLUE, H::Left);
        if pending {
            c.text(x, 77.0, " · 待确认", size::SMALL, W::Regular, theme::BLUE, H::Left);
        } else if self.water.percent()>=100.0 {
            c.text(x,77.0," · 已达标",size::SMALL,W::Regular,theme::GRAY,H::Left);
        } else if self.water.remind && self.lunch_now(env) {
            c.text(x, 77.0, " · 午休中", size::SMALL, W::Regular, theme::GRAY, H::Left);
        } else if self.water.remind && self.water_hours(env.hour() as u32) {
            c.text(x, 77.0, &format!(" · 下次 {}", self.next_hint(env)), size::SMALL, W::Regular, theme::GRAY, H::Left);
        }
        c.fill(44.0, 84.0, 151.0, 3.5, 1.75, Color32::from_rgb(34, 34, 37));
        let f = self.water_ring.pos.clamp(0.0, 1.0);
        if f > 0.0 {
            c.fill(44.0, 84.0, (151.0 * f).max(3.5), 3.5, 1.75, theme::BLUE);
        }

        let (d, dc) = capsule(c, 43.0, 98.0, 62.0, 25.0, "w-drink", theme::BLUE_DEEP, Color32::from_rgb(22, 92, 165));
        paint::droplet(&dc, 56.0, 110.5, 11.0, Color32::from_rgb(120, 200, 255));
        dc.text(65.0, 114.0, "喝一口", size::BODY, W::Semibold, Color32::from_rgb(220, 236, 255), H::Left);
        if d.clicked() { self.drink_water(); }
        if self.last_drink.is_some() {
            c.text(191.0,114.0,"撤销",10.5,W::Regular,theme::GRAY,H::Center);
            if c.hit(177.0,98.0,28.0,25.0,"water-undo").on_hover_text("撤销最近一次饮水记录").clicked() {
                if let Some((ml,hour))=self.last_drink.take() {
                    self.water.drunk_ml=self.water.drunk_ml.saturating_sub(ml);
                    self.water.hours[hour]=self.water.hours[hour].saturating_sub(ml);
                    self.water.save(); self.say("已撤销这次记录");
                }
            }
        }

        // 右侧：环形 ↔ 柱状图（点右侧区域或右上角两颗豆切换）
        let vt = self.water_view_t.pos;
        let area = c.hit(214.0, 30.0, 166.0, 96.0, "w-view");
        if area.clicked() {
            self.water_view = 1 - self.water_view;
        }
        for i in 0..2 {
            let on = if i == 0 { 1.0 - vt.clamp(0.0, 1.0) } else { vt.clamp(0.0, 1.0) };
            let w = lerp(3.0, 7.0, on);
            let x = 362.0 + i as f32 * 8.0 - w / 2.0 + if i == 0 { -1.0 * on } else { 1.0 * on };
            c.fill(x, 33.5, w, 3.0, 1.5, mix(Color32::from_gray(80), theme::WHITE, on));
        }

        let ring_a = 1.0 - smoothstep(0.0, 0.6, vt);
        if ring_a > 0.0 {
            let k = 1.0 + 0.22 * vt;
            let rc = c.faded(ring_a).scaled(292.0, 78.0, k);
            Self::progress_ring(&rc, 292.0, 78.0, 33.0, 6.0, f, theme::WATER_TRACK, theme::BLUE);
            rc.text(292.0, 86.0, &format!("{pct:.0}"), 26.0, W::Bold, theme::WHITE, H::Center);
            paint::droplet(&rc, 287.0, 97.0, 7.0, theme::CYAN);
            rc.text(292.5, 100.0, "%", 10.5, W::Semibold, Color32::from_gray(217), H::Left);
        }
        let chart_a = smoothstep(0.25, 1.0, vt);
        if chart_a > 0.0 {
            self.water_chart(&c.faded(chart_a), env, vt);
        }
    }

    /// 久坐：与喝水同一版式，但只讲一件事——「还有多久该起来」。
    /// 提醒后保持待处理，确认或起身后才重新计时。
    fn sit_body(&mut self, c: &Card, env: &Env) {
        let on = self.settings.sit_remind;
        let sat = self.sat_minutes();
        let goal = self.settings.sit_minutes.max(1);
        let focusing = self.pomo.running && self.pomo.phase == Phase::Focus;
        let lunch = self.lunch_now(env);
        let in_hours = self.remind_window(env);
        let left = ((self.sit_due() - self.t).max(0.0) / 60.0).ceil() as u32;
        let pending = on && self.health_pending[crate::extras::HealthReminder::Sit as usize];
        let counting = on && !pending && left > 0;
        // 待处理时进度停在满环，确认后从零开始。
        let frac = if on { (1.0 - (self.sit_due() - self.t) / (goal as f64 * 60.0)).clamp(0.0, 1.0) as f32 }
            else { (sat as f32 / goal as f32).clamp(0.0, 1.0) };
        let fill = theme::SIT_GREEN;

        if pending {
            c.text(44.0, 59.0, "待活动", 24.0, W::Bold, fill, H::Left);
        } else if counting {
            let ww = c.text(44.0, 61.0, &left.to_string(), size::HERO, W::Bold, theme::WHITE, H::Left);
            c.text(44.0 + ww + 5.0, 60.0, "分钟后提醒", 11.0, W::Semibold, theme::GRAY, H::Left);
        } else {
            c.text(44.0, 59.0, if on { "等待提醒" } else { "已关闭" }, 24.0, W::Bold, theme::GRAY, H::Left);
        }

        // 已坐 25 分钟 · 10:45 提醒
        let tail = if pending { "确认提醒或起身后重新计时".to_string() }
            else if !on { "久坐提醒已关闭".to_string() }
            else if lunch { "午休中不提醒".to_string() }
            else if !in_hours { format!("{:02}:00–{:02}:00 外不提醒", self.settings.water_start, self.settings.water_end) }
            else if !counting && focusing { "专注结束后再提醒".to_string() }
            else { format!("{} 提醒", Self::clock_after(env, self.sit_due() - self.t)) };
        let mut x = 44.0;
        if !pending && counting {
            x += c.text(x, 77.0, "已坐 ", size::SMALL, W::Regular, theme::GRAY, H::Left);
            x += c.text(x, 77.0, &format!("{sat} 分钟"), size::SMALL, W::Bold, fill, H::Left);
            x += c.text(x, 77.0, " · ", size::SMALL, W::Regular, theme::GRAY, H::Left);
        }
        c.text(x, 77.0, &tail, size::SMALL, W::Regular, theme::GRAY, H::Left);
        c.fill(44.0, 84.0, 151.0, 3.5, 1.75, Color32::from_rgb(34, 34, 37));
        if frac > 0.0 {
            c.fill(44.0, 84.0, (151.0 * frac).max(3.5), 3.5, 1.75, fill);
        }

        let (b, bc) = capsule(c, 43.0, 98.0, 62.0, 25.0, "sit-stand", Color32::from_rgb(24, 84, 52), Color32::from_rgb(30, 104, 64));
        paint::chair(&bc, 55.5, 110.5, 0.36, Color32::from_rgb(150, 230, 180));
        bc.text(64.0, 114.0, "起身了", size::BODY, W::Semibold, Color32::from_rgb(220, 244, 228), H::Left);
        if b.on_hover_text("确认已起身，重新计时").clicked() { self.stood_up(); }

        // 右侧：与喝水同一个环，环心是椅子。
        Self::progress_ring(c, 292.0, 78.0, 33.0, 6.0, frac, Color32::from_rgb(16, 48, 32), if on { fill } else { Color32::from_gray(90) });
        let (label, ink) = if !on { ("已关", theme::GRAY) }
            else if pending { ("动一动", fill) }
            else { ("坐着", Color32::from_gray(217)) };
        paint::chair(c, 292.0, 76.0, 0.75, if !on { theme::GRAY } else if pending { fill } else { theme::WHITE });
        c.text(292.0, 101.0, label, 10.5, W::Semibold, ink, H::Center);
    }

    /// 底部留缺口的进度环（约 276°），起点有圆头亮点。
    fn progress_ring(c: &Card, cx: f32, cy: f32, r: f32, thick: f32, frac: f32, track: Color32, color: Color32) {
        let start = 132.0f32.to_radians();
        let sweep = 276.0f32.to_radians();
        c.arc(cx, cy, r, start, start + sweep, thick, track);
        c.circle(cx + r * start.cos(), cy + r * start.sin(), thick * 0.5, track);
        c.circle(cx + r * (start + sweep).cos(), cy + r * (start + sweep).sin(), thick * 0.5, track);
        let end = start + sweep * frac;
        if frac > 0.002 {
            c.arc(cx, cy, r, start, end, thick, color);
        }
        c.circle(cx + r * start.cos(), cy + r * start.sin(), thick * 0.55, color);
        if frac > 0.002 {
            c.circle(cx + r * end.cos(), cy + r * end.sin(), thick * 0.5, color);
        }
    }

    /// 今日分小时柱状图：12 根柱，从当前时间往前；柱子依次升起。
    fn water_chart(&self, c: &Card, env: &Env, vt: f32) {
        let hour = env.hour();
        let first = hour.saturating_sub(8).min(12);
        let (x0, bottom, top) = (224.0, 103.0, 52.0);
        let max = self.water.hours.iter().copied().max().unwrap_or(0).max(500) as f32;
        let scale = if max > 500.0 { (max / 250.0).ceil() * 250.0 } else { 500.0 };
        for (i, label) in [(0.0, format!("{}", scale as u32)), (0.5, format!("{}", (scale / 2.0) as u32)), (1.0, "0 ml".to_string())] {
            let y = lerp(top, bottom, i);
            c.text(343.0, y + 3.0, &label, size::TINY, W::Regular, theme::DIM, H::Left);
            if i == 0.5 {
                for k in 0..28 {
                    c.fill(x0 + k as f32 * 4.0, y, 2.0, 0.8, 0.0, Color32::from_gray(48));
                }
            }
        }
        c.fill(x0 - 2.0, bottom + 1.5, 114.0, 0.8, 0.0, Color32::from_gray(48));
        for i in 0..12 {
            let hr = first + i;
            let x = x0 + i as f32 * 9.4;
            // 依次升起：每根晚 18 ms
            let rise = anim::ease_out_cubic(((vt - 0.3 - i as f32 * 0.03) / 0.5).clamp(0.0, 1.0));
            let full = bottom - top;
            c.fill(x, bottom - full * rise, 5.6, full * rise, 2.8, Color32::from_rgb(26, 26, 29));
            let v = self.water.hours.get(hr).copied().unwrap_or(0) as f32;
            if v > 0.0 {
                let hgt = (full * (v / scale)).max(5.6) * rise;
                let col = if hr == hour { theme::BLUE } else { Color32::from_rgb(40, 120, 190) };
                c.fill(x, bottom - hgt, 5.6, hgt, 2.8, col);
            } else if hr == hour {
                c.circle(x + 2.8, bottom - 2.8, 2.8 * rise, theme::BLUE);
            }
            if hr % 3 == 0 {
                c.text(x + 2.8, 117.0, &format!("{hr:02}"), size::TINY, W::Regular, theme::GRAY, H::Center);
            }
        }
    }

    // ================= 置物架 =================
    /// 标题旁的「文件 | 剪贴板」分栏，返回分栏右边缘。
    /// 页面标题处的「置物架 | 剪贴板」分栏，返回分栏右边缘。
    fn shelf_tabs(&mut self, c: &Card, x: f32) -> f32 {
        let labels = ["置物架", "剪贴板"];
        let font = 12.0;
        let widths: Vec<f32> = labels.iter().map(|l| c.text_w(l, font, W::Bold) + 14.0).collect();
        let total = widths.iter().sum::<f32>() + 4.0;
        c.fill(x, 9.5, total, 21.0, 10.5, theme::BTN);
        let mut left = x + 2.0;
        for (i, (label, w)) in labels.iter().zip(&widths).enumerate() {
            let selected = self.shelf_tab == i;
            let r = c.hit(left, 9.5, *w, 21.0, ("shelf-tab", i));
            let (h, _) = c.hover_t(&r);
            if selected {
                c.fill(left, 11.5, *w, 17.0, 8.5, Color32::from_gray(88));
            } else if h > 0.01 {
                c.fill(left, 11.5, *w, 17.0, 8.5, Color32::from_white_alpha((18.0 * h) as u8));
            }
            c.text(left + w / 2.0, 24.3, label, font, W::Bold, if selected { theme::WHITE } else { theme::GRAY }, H::Center);
            if r.clicked() && !selected {
                self.shelf_tab = i;
                self.confirm_clear = None;
                self.clip_confirm_clear = None;
            }
            left += w;
        }
        x + total
    }

    pub(crate) fn page_clipboard_standalone(&mut self, c: &Card, env: &Env, cmds: &mut Vec<Cmd>) {
        self.page_clipboard(c, env, cmds);
    }

    pub(crate) fn page_shelf(&mut self, c: &Card, env: &Env, cmds: &mut Vec<Cmd>) {
        let n = self.shelf.items.len();
        let drag = self.drag_a;
        if cfg!(test) {
            // 拖文件进来时回到「文件」分栏。
            if drag > 0.002 { self.shelf_tab = 0; }
            self.shelf_tabs(c, 13.0);
            if self.shelf_tab == 1 {
                self.page_clipboard(c, env, cmds);
                return;
            }
        }
        if cfg!(test) {
            c.text(344.0,23.0,"添加",size::SMALL,W::Regular,theme::GRAY,H::Right);
            if c.hit(311.0,8.0,38.0,22.0,"shelf-add").on_hover_text("从电脑选择文件").clicked(){cmds.push(Cmd::AddFiles);}
        } else {
            c.fill(38.0, 10.0, 168.0, 19.0, 7.0, Color32::from_white_alpha(12));
            c.text(48.0, 23.2, &format!("已收纳 {} 个文件 · 支持拖入/拖出", n), 8.2, W::Semibold, Color32::from_gray(195), H::Left);
            let (add_r, add_b) = capsule(c, 322.0, 10.0, 56.0, 19.0, "shelf-add", theme::BTN, theme::BTN_HOVER);
            add_b.text(350.0, 23.2, "＋ 添加", 8.5, W::Semibold, theme::WHITE, H::Center);
            if add_r.on_hover_text("从电脑选择文件放入置物架").clicked() { cmds.push(Cmd::AddFiles); }
        }
        if n > 5 {
            let page_y = if cfg!(test) { 193.0 } else { 218.0 };
            c.text(209.5, page_y, &format!("{}–{} / {n} · 滚轮查看更多文件", self.shelf_first + 1, (self.shelf_first + 5).min(n)), 10.5, W::Regular, theme::DIM, H::Center);
        }

        if n > 0 {
            let confirming = self.confirm_clear.is_some();
            let label = if confirming { "确认" } else { "清空" };
            let lw = c.text_w("清空", size::SMALL, W::Regular) + 16.0;
            let base = if confirming { Color32::from_rgb(120, 28, 36) } else { theme::BTN };
            let hover = if confirming { Color32::from_rgb(150, 34, 44) } else { theme::BTN_HOVER };
            // 放入次顶栏「＋ 添加」左侧（不占用下方文件展示框）
            let (x, y) = if cfg!(test) { (379.0 - lw, 177.0) } else { (316.0 - lw, 10.0) };
            let (r, bc) = capsule(&c.faded(1.0 - drag), x, y, lw, if cfg!(test) { 18.0 } else { 19.0 }, "shelf-clear", base, hover);
            let r = if confirming { r.on_hover_text("再点一次清空置物架（文件本身不动）") } else { r.on_hover_text("清空置物架") };
            bc.text(x + lw / 2.0, y + 13.0, label, if cfg!(test) { size::SMALL } else { 8.5 }, W::Semibold, theme::WHITE, H::Center);
            if r.clicked() {
                if confirming {
                    self.shelf.clear();
                    self.icons.clear();
                    self.shelf_born.clear();
                    self.shelf_x.clear();
                    self.shelf_first = 0;
                    self.confirm_clear = None;
                    self.say("置物架已清空（文件本身没动）");
                } else {
                    self.confirm_clear = Some(self.t);
                }
            }
        }

        let body = c.faded(lerp(1.0, 0.14, drag));
        if n == 0 {
            let a = 1.0 - drag;
            if a > 0.0 {
                let ec = body.faded(a);
                if cfg!(test) {
                    ec.dashed(38.0, 34.0, 343.0, 158.0, 13.0, 1.2, Color32::from_gray(70), 0.0);
                    paint::tray(&ec, 209.5, 92.0, 32.0, 0.0, Color32::from_gray(200), Color32::from_gray(80));
                    ec.text(209.5, 130.0, "把文件拖到这儿临时收纳", 13.0, W::Semibold, theme::WHITE, H::Center);
                    ec.text(209.5, 148.0, "只记本机路径，不复制原文件 · 点一下直接打开或拖出到任意软件", size::TINY, W::Regular, theme::GRAY, H::Center);
                } else {
                    // [修复需求 6 图6 `image_3d5358.jpg`]：将置物架区域完整舒展至 y = 34..222 (高 188px)，虚线框、渐变微光底与图标文字垂直居中对齐！
                    ec.fill(36.0, 34.0, 347.0, 188.0, 14.0, Color32::from_white_alpha(8));
                    ec.dashed(36.0, 34.0, 347.0, 188.0, 14.0, 1.25, Color32::from_gray(82), 0.0);
                    paint::tray(&ec, 209.5, 106.0, 36.0, 0.0, Color32::from_gray(215), Color32::from_gray(90));
                    ec.text(209.5, 148.0, "把文件拖到这儿临时收纳", 13.5, W::Bold, theme::WHITE, H::Center);
                    ec.text(209.5, 168.0, "只记本机路径，不复制原文件 · 点击直接打开或拖出到任意软件", 9.6, W::Regular, Color32::from_gray(165), H::Center);
                }
            }
        } else {
            self.shelf_row(&body, cmds);
        }
        if drag > 0.002 {
            self.drag_overlay(&c.faded(drag));
        }
    }

    fn shelf_row(&mut self, c: &Card, cmds: &mut Vec<Cmd>) {
        let n = self.shelf.items.len();
        let slot = 68.0;
        let center = 209.5;
        let visible = n.min(5) as f32;
        let scroll = self.shelf_scroll.pos;
        let row_h_clip = if cfg!(test) { 142.0 } else { 184.0 };
        let row = c.clipped(36.0, 34.0, 347.0, row_h_clip);
        let dt = self.dt;
        let items = self.shelf.items.clone();
        self.icons.retain(&items);
        self.shelf_x.retain(|p, _| items.contains(p));
        for (i, p) in items.iter().enumerate() {
            let target = center + (i as f32 - scroll - (visible - 1.0) / 2.0) * slot;
            let born = self.shelf_born.get(p).copied();
            let s = self.shelf_x.entry(p.clone()).or_insert_with(|| {
                let start = if born.is_some() { target + slot * 0.9 } else { target };
                Spring::new(start, 230.0, 0.9)
            });
            s.target = target;
            s.step(dt);
            let x = s.pos;
            if x < 0.0 || x > 420.0 {
                continue;
            }
            let appear = born.map(|b| smoothstep(0.0, 0.35, (self.t - b) as f32)).unwrap_or(1.0);
            let edge = smoothstep(26.0, 60.0, x) * smoothstep(392.0, 358.0, x);
            let a = appear * edge;
            if a <= 0.01 {
                continue;
            }
            let r = row.hit_drag(x - 32.0, 36.0, 64.0, 128.0, ("shelf", p));
            let show_hint=self.shelf_menu_rect.is_none() && !r.context_menu_opened() && !r.secondary_clicked();
            if show_hint {r.clone().on_hover_text(p.display().to_string());}
            let (h, pr) = row.hover_t(&r);
            let cc = row.faded(a).scaled(x, 96.0, 1.0 - 0.04 * pr);
            cc.fill(x - 31.0, 36.0, 62.0, 126.0, 11.0, Color32::from_white_alpha((10.0 + 18.0 * h) as u8));
            let lift = -2.5 * h + (1.0 - appear) * 8.0;
            self.file_icon_at(&cc, p, x, 68.0 + lift, 42.0);
            let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let name = cc.fit(&name, size::SMALL, W::Semibold, 58.0);
            cc.text(x, 114.0, &name, size::SMALL, W::Semibold, theme::WHITE, H::Center);
            let meta=self.icons.get(c.ui.ctx(),p).map(|preview|preview.metadata).unwrap_or_default();
            cc.text(x, 130.0, &meta, size::TINY, W::Regular, theme::GRAY, H::Center);
            let ext = p.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_else(|| "FILE".into());
            cc.text(x, 147.0, &cc.fit(&ext, 8.5, W::Semibold, 52.0), 8.5, W::Semibold, Color32::from_gray(145), H::Center);
            if r.drag_started() && p.exists() {cmds.push(Cmd::DragFile(p.clone()));}
            let remove=row.hit(x+15.0,36.0,17.0,18.0,("shelf-remove",p));
            let remove_card=row.faded(a);
            remove_card.circle(x+23.5,45.0,7.5,Color32::from_rgb(55,40,46));
            paint::close(&remove_card,x+23.5,45.0,2.4,theme::WHITE);
            let mut delete=remove.clicked();
            if show_hint {remove.on_hover_text("从置物架移除，保留原文件");}
            r.context_menu(|ui|{
                if ui.button("打开").clicked(){if p.exists(){cmds.push(Cmd::Open(p.clone()));}ui.close_menu();}
                if ui.button("在文件夹中显示").clicked(){cmds.push(Cmd::Reveal(p.clone()));ui.close_menu();}
                if ui.button("复制完整路径").clicked(){cmds.push(Cmd::Copy(p.display().to_string()));ui.close_menu();}
                if ui.button("从置物架移除").clicked(){delete=true;ui.close_menu();}
                self.shelf_menu_rect=Some(egui::Frame::menu(ui.style()).widget_rect(ui.min_rect()));
            });
            if delete { self.shelf.remove(p); self.icons.remove(p); self.shelf_x.remove(p); self.shelf_born.remove(p); self.shelf_first=self.shelf_first.min(self.shelf.items.len().saturating_sub(5)); }
            else if r.clicked() { if p.exists(){cmds.push(Cmd::Open(p.clone()));} else {self.say("文件已移动或删除，可从置物架移除");} }
        }
    }

    // ================= 剪贴板（置物架第二个分栏 · 加长高度 216 适配 6 行长文本） =================
    fn page_clipboard(&mut self, c: &Card, _env: &Env, _cmds: &mut Vec<Cmd>) {
        if !self.settings.clipboard_history {
            c.dashed(38.0, 34.0, 343.0, 158.0, 13.0, 1.2, Color32::from_gray(70), 0.0);
            c.text(209.5, 98.0, "剪贴板记录已关闭", 12.5, W::Semibold, theme::WHITE, H::Center);
            c.text(209.5, 116.0, "开启后，检测到复制动作并将鼠标移到岛上即可自动展开查看", size::TINY, W::Regular, theme::GRAY, H::Center);
            let (r, bc) = capsule(c, 179.5, 128.0, 60.0, 21.0, "clip-enable", theme::BTN, theme::BTN_HOVER);
            bc.centered_text(209.5, 138.5, "开启", size::SMALL, W::Semibold, theme::WHITE, H::Center);
            if r.clicked() { self.settings.clipboard_history = true; self.clipboard.set_enabled(true); self.settings.save(); }
            return;
        }
        static CLIP_SNAP_CACHE: std::sync::OnceLock<std::sync::Mutex<(u64, std::sync::Arc<Vec<crate::clipboard::Clip>>)>> = std::sync::OnceLock::new();
        let cur_ver = self.clipboard.lock().version;
        let items_arc = {
            let mtx = CLIP_SNAP_CACHE.get_or_init(|| std::sync::Mutex::new((u64::MAX, std::sync::Arc::new(Vec::new()))));
            let mut guard = mtx.lock().unwrap();
            if guard.0 != cur_ver {
                guard.0 = cur_ver;
                guard.1 = std::sync::Arc::new(self.clipboard.lock().items.clone());
            }
            guard.1.clone()
        };
        let items = items_arc.as_slice();
        if !cfg!(test) {
            if crate::auralsle_ai_status::config::get().clipboard_ui_mode == 0 {
                self.draw_brimdeck_clipboard_ui(c, items);
            } else {
                self.draw_fly_clipboard_message_ui(c, items);
            }
            return;
        }
        let n = items.len();
        let recent = items.iter().filter(|clip| !clip.pinned).count();
        self.clip_first = self.clip_first.min(n.saturating_sub(CLIP_ROWS));

        // 右上：条数与「清空」（只清未固定的）。
        if recent > 0 {
            let confirming = self.clip_confirm_clear.is_some_and(|at| self.t - at < 3.0);
            if !confirming { self.clip_confirm_clear = None; }
            let label = if confirming { "确认" } else { "清空" };
            let lw = c.text_w("清空", size::SMALL, W::Regular) + 16.0;
            let base = if confirming { Color32::from_rgb(120, 28, 36) } else { theme::BTN };
            let hover = if confirming { Color32::from_rgb(150, 34, 44) } else { theme::BTN_HOVER };
            let x = 351.0 - lw;
            let (r, bc) = capsule(c, x, 11.5, lw, 17.0, "clip-clear", base, hover);
            bc.text(x + lw / 2.0, 23.5, label, size::SMALL, W::Regular, theme::WHITE, H::Center);
            let r = r.on_hover_text(if confirming { "再点一次清空未固定的记录" } else { "清空未固定的记录" });
            if r.clicked() {
                if confirming {
                    self.clipboard.lock().clear_recent();
                    self.clip_first = 0;
                    self.clip_confirm_clear = None;
                    self.say("剪贴板记录已清空（固定的保留）");
                } else {
                    self.clip_confirm_clear = Some(self.t);
                }
            }
        }

        if n == 0 {
            c.dashed(38.0, 34.0, 343.0, 158.0, 13.0, 1.2, Color32::from_gray(70), 0.0);
            paint::clipboard_glyph(c, 209.5, 88.0, 25.0, Color32::from_gray(200), Color32::from_gray(80));
            c.text(209.5, 126.0, "复制的文字会出现在这里", 12.5, W::Semibold, theme::WHITE, H::Center);
            c.text(209.5, 144.0, "检测到复制后将鼠标移入灵动岛，即可自动展开此剪贴板面板", size::TINY, W::Regular, theme::GRAY, H::Center);
            return;
        }

        if !cfg!(test) {
            self.draw_fly_clipboard_message_ui(c, &items);
            return;
        }
        c.fill(38.0, 32.0, 343.0, 162.0, 12.0, theme::CARD);
        // 超过一屏时右侧细滚动条，滚轮查看。
        if n > CLIP_ROWS {
            let (top, track) = (39.0, 148.0);
            let thumb = (track * CLIP_ROWS as f32 / n as f32).max(14.0);
            let y = top + (track - thumb) * self.clip_first as f32 / (n - CLIP_ROWS) as f32;
            c.fill(375.0, top, 2.5, track, 1.25, Color32::from_white_alpha(14));
            c.fill(375.0, y, 2.5, thumb, 1.25, Color32::from_white_alpha(70));
        }
        let mut action: Option<(usize, u8)> = None; // 0 复制 1 固定 2 删除
        for (row, (index, clip)) in items.iter().enumerate().skip(self.clip_first).take(CLIP_ROWS).enumerate() {
            let y = 46.5 + row as f32 * 25.2;
            let r = c.hit(44.0, y - 11.0, 329.0, 22.5, ("clip", index, clip.text.len(), clip.at.as_str()));
            let (h, _) = c.hover_t(&r);
            if h > 0.01 {
                c.fill(44.0, y - 11.0, 329.0, 22.5, 7.5, Color32::from_white_alpha((14.0 * h) as u8));
            }
            if clip.pinned {
                paint::pin(c, 54.0, y, 9.0, theme::BLUE);
            } else {
                c.circle(54.0, y, 2.2, Color32::from_gray(110));
            }
            let text = c.fit(&crate::clipboard::preview(&clip.text), size::BODY, W::Regular, 226.0);
            c.text(64.0, y + 3.8, &text, size::BODY, W::Regular, Color32::from_gray(238), H::Left);
            c.text(324.0, y + 3.5, &clip.at, size::TINY, W::Regular, theme::DIM, H::Right);
            let tip: String = clip.text.chars().take(240).collect();
            let r = r.on_hover_text(format!("{tip}{}
点击复制", if clip.text.len() > 240 { "…" } else { "" }));
            let pin = c.hit(329.0, y - 9.0, 18.0, 18.0, ("clip-pin", index)).on_hover_text(if clip.pinned { "取消固定" } else { "固定（清空时保留）" });
            paint::pin(c, 338.0, y, 9.0, if clip.pinned || pin.hovered() { theme::BLUE } else { theme::GRAY });
            let delete = c.hit(350.0, y - 9.0, 17.0, 18.0, ("clip-delete", index)).on_hover_text("删除这条记录");
            paint::close(c, 358.5, y, 2.4, theme::GRAY);
            if delete.clicked() { action = Some((index, 2)); }
            else if pin.clicked() { action = Some((index, 1)); }
            else if r.clicked() { action = Some((index, 0)); }
        }
        match action {
            Some((index, 0)) => {
                #[cfg(windows)]
                self.say(if crate::clipboard::copy_history(&items[index].text) { "已复制" } else { "复制失败，请重试" });
                #[cfg(not(windows))]
                _cmds.push(Cmd::Copy(items[index].text.clone()));
            }
            Some((index, 1)) => {
                if !self.clipboard.lock().toggle_pin(index) { self.say(format!("最多固定 {} 条", crate::clipboard::MAX_PINNED)); }
            }
            Some((index, _)) => { self.clipboard.lock().remove(index); }
            None => {}
        }
    }

    fn file_icon_at(&mut self, c: &Card, p: &std::path::Path, cx: f32, cy: f32, sz: f32) {
        let tex=self.icons.get(c.ui.ctx(),p).and_then(|preview|preview.image);
        if let Some(tex) = tex {
            let s = tex.size_vec2();
            let k = sz / s.x.max(s.y);
            let r = Rect::from_center_size(c.pos(cx, cy), s * k * c.s);
            c.painter.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        } else {
            paint::file_glyph(c, paint::file_kind(p), cx, cy, sz);
        }
    }

    /// 拖入遮罩：流动虚线框 + 跟随光标的柔光 + 上下轻跳的托盘图标。
    fn drag_overlay(&self, c: &Card) {
        let (x, y, w, h) = if cfg!(test) { (38.0, 32.0, 343.0, 162.0) } else { (36.0, 34.0, 347.0, 188.0) };
        c.fill(x, y, w, h, 12.0, Color32::from_black_alpha(110));
        if let Some(p) = self.cursor_local {
            let px = p.x.clamp(x + 20.0, x + w - 20.0);
            let py = p.y.clamp(y + 10.0, y + h - 10.0);
            c.clipped(x, y, w, h).glow(px, py, 90.0, Color32::from_rgba_unmultiplied(120, 170, 255, 34));
        }
        c.dashed(x, y, w, h, 12.0, 1.4, Color32::from_white_alpha(230), self.t as f32 * 14.0);
        let bob = ((self.t * 5.0).sin() as f32 * 0.5 + 0.5) * 3.0;
        paint::tray(c, 209.5, 96.0, 32.0, bob, theme::WHITE, Color32::from_gray(90));
        c.text(209.5, 132.0, "松手放进置物架", 12.5, W::Semibold, theme::WHITE, H::Center);
    }

    // ================= 待办 =================
    pub(crate) fn page_todo(&mut self, c: &Card, env: &Env, _cmds: &mut Vec<Cmd>) {
        if !cfg!(test) {
            self.draw_fly_todo_page(c, env, _cmds);
            return;
        }
        // 今天未完成优先，已完成沉底；不在今天的重复待办变暗显示、不能勾选。
        let (done, total) = self.todos.progress(&env.today);
        let order = self.todos.display_order(&env.today);
        let count = order.len();
        let pop = self.todo_pop.pos.abs().min(0.4);
        let hc = c.scaled(60.0, 50.0, 1.0 + pop * 0.25);
        let w = hc.text(42.0, 61.0, &format!("{done}"), size::HERO, W::Bold, if done > 0 && done == total { theme::GREEN } else { theme::WHITE }, H::Left);
        hc.text(42.0 + w, 61.0, &format!("/{total}"), size::HERO, W::Bold, theme::WHITE, H::Left);
        c.text(42.0, 78.0, "已完成", size::SMALL, W::Regular, theme::GRAY, H::Left);

        let (add, ac) = capsule(c, 41.0, 96.0, 74.0, 26.0, "t-add", theme::BTN, theme::BTN_HOVER);
        paint::plus(&ac, 56.0, 109.0, 6.2, theme::WHITE, Color32::BLACK);
        ac.text(67.0, 112.5, "加一个", size::BODY, W::Regular, theme::WHITE, H::Left);
        let can_edit = !self.todos.items.is_empty() && !self.adding && self.editing.is_none();
        let ec = if can_edit { c.clone() } else { c.faded(0.45) };
        let (edit, edc) = capsule(&ec, 121.0, 96.0, 68.0, 26.0, "t-edit", theme::BTN, if can_edit { theme::BTN_HOVER } else { theme::BTN });
        if self.todo_choosing{paint::close(&edc,135.0,109.0,3.5,theme::WHITE);}else{paint::pencil(&edc,135.0,109.0,11.0,theme::WHITE);}
        edc.text(145.0, 112.5, if self.todo_choosing{"取消"}else{"改一下"}, size::BODY, W::Regular, theme::WHITE, H::Left);

        c.fill(196.0, 31.0, 181.0, 93.0, 12.0, theme::CARD);
        let editing = self.adding || self.editing.is_some();
        if count == 0 && !editing {
            paint::checklist(c, 286.5, 64.0, 32.0, Color32::from_gray(150), Color32::from_rgb(52, 54, 58));
            c.text(286.5, 99.0, "今天还没有安排", size::SMALL, W::Regular, theme::GRAY, H::Center);
        } else {
            let mut row = 0;
            let row_y = |r: usize| 46.0 + r as f32 * 22.0;
            if editing {
                // 输入行（文本框本身由 app::input_field 放置）
                let y = row_y(0);
                c.fill(203.0, y - 10.0, 167.0, 20.0, 7.0, Color32::from_rgb(34, 34, 38));
                c.stroke(203.0, y - 10.0, 167.0, 20.0, 7.0, 1.0, paint::alpha(theme::BLUE, 0.7));
                if self.editing.is_some() {
                    paint::pencil(c, 212.0, y, 9.0, theme::BLUE);
                } else {
                    paint::plus(c, 212.0, y, 5.0, theme::BLUE, Color32::BLACK);
                }
                row = 1;
            }
            let ctx = c.ui.ctx().clone();
            let cap = if editing { 2 } else { 3 };
            let mut shown = 0;
            for &i in order.iter().skip(self.todo_first.min(count.saturating_sub(1))) {
                if shown >= cap {
                    break;
                }
                if Some(i) == self.editing {
                    continue;
                }
                let item = self.todos.items[i].clone();
                let today_item = item.scheduled_on(&env.today);
                let y = row_y(row);
                let r = c.hit(202.0, y - 10.0, 169.0, 20.0, ("todo", i, item.text.as_str()));
                let (h, _) = c.hover_t(&r);
                if h > 0.01 {
                    c.fill(202.0, y - 10.0, 169.0, 20.0, 7.0, Color32::from_white_alpha((12.0 * h) as u8));
                }
                let checked = item.done_for(&env.today);
                let ct = ctx.animate_bool_with_time(egui::Id::new(("todo-check", item.text.as_str())), checked, 0.28);
                let rc = if today_item { c.clone() } else { c.faded(0.45) };
                if self.todo_choosing{paint::pencil(c,212.0,y,10.0,theme::BLUE);}else{
                    rc.ring(212.0, y, 5.2, 1.3, paint::alpha(Color32::from_gray(120), 1.0 - ct));
                    if ct > 0.0 {
                        c.circle(212.0, y, 6.0 * anim::ease_out_cubic(ct.min(1.0) / 0.6).min(1.0), theme::GREEN);
                        paint::check(c, 212.0, y, 6.0, smoothstep(0.3, 1.0, ct), 1.4, Color32::BLACK);
                    }
                }
                // 每周重复：文字右侧小标签（周一三五 / 工作日 / 每天）。
                let tag = crate::model::repeat_tag(item.repeat);
                let tag_w = if tag.is_empty() { 0.0 } else { c.text_w(&tag, size::TINY, W::Regular) + 6.0 };
                let text = c.fit(&item.text, size::BODY, W::Regular, 104.0 - tag_w);
                let repeat_hint = if item.is_repeating() { format!("\n{}{}", crate::model::repeat_label(item.repeat), if today_item { "" } else { " · 今天不用做" }) } else { String::new() };
                r.clone().on_hover_text(format!("{}{}{}",item.text,repeat_hint,item.reminder_text().map(|d|format!("\n提醒：{d}")).unwrap_or_default()));
                let tcol = mix(Color32::from_gray(235), Color32::from_gray(110), ct);
                let tw = rc.text(223.0, y + 3.8, &text, size::BODY, W::Regular, tcol, H::Left);
                if !tag.is_empty() {
                    rc.text(223.0 + tw + 5.0, y + 3.2, &tag, size::TINY, W::Regular, if today_item { theme::BLUE } else { theme::GRAY }, H::Left);
                }
                if ct > 0.0 {
                    c.fill(223.0, y + 0.2, tw * smoothstep(0.2, 1.0, ct), 1.0, 0.5, Color32::from_gray(110));
                }
                if self.todo_choosing{
                    c.text(366.0,y+3.8,"修改",size::TINY,W::Regular,theme::BLUE,H::Right);
                    if r.clicked(){self.todo_choosing=false;self.editing=Some(i);self.adding=false;self.buf=item.text.clone();self.todo_input_focused=false;break;}
                    row+=1;shown+=1;continue;
                }
                let delete=c.hit(353.0,y-9.0,17.0,18.0,("todo-delete",i)).on_hover_text("删除待办").clicked();
                paint::close(c,361.5,y,2.4,theme::GRAY);
                let reminder=c.hit(333.0,y-9.0,18.0,18.0,("todo-reminder",i)).on_hover_text(item.reminder_text().map(|due|format!("提醒：{due} · 点击修改")).unwrap_or_else(||"设置提醒与每周重复（可选）".into()));
                let reminder_clicked=reminder.clicked();
                let ink=if item.due.is_some()||reminder.hovered(){theme::BLUE}else{theme::GRAY};
                c.ring(342.0,y,4.7,1.1,ink);
                c.path(&[(342.0,y-2.5),(342.0,y),(344.3,y+1.0)],1.1,ink);
                if reminder_clicked {self.open_task_reminder(i);}
                let mut edit_this=false;
                r.context_menu(|ui|{
                    if ui.button("编辑与提醒…").clicked(){edit_this=true;ui.close_menu();}
                    if ui.button("取消提醒").clicked(){self.todos.items[i].due=None;self.todos.save();ui.close_menu();}
                });
                if edit_this {self.open_task_dialog(i);}
                if delete {
                    self.todos.items.remove(i);
                    if let Some(index)=self.editing.as_mut(){if i<*index{*index-=1;}}
                    self.todos.save();self.todo_first=self.todo_first.min(self.todos.items.len().saturating_sub(3));break;
                }
                if r.clicked() && !edit_this && !reminder_clicked && !today_item {
                    self.say(format!("{}的待办，今天不用做", crate::model::repeat_label(item.repeat)));
                } else if r.clicked() && !r.double_clicked() && !edit_this && !reminder_clicked {
                    let now = self.todos.items[i].done_for(&env.today);
                    self.todos.items[i].toggle(&env.today);
                    self.todos.save();
                    if !now {
                        self.todo_pop.vel += 5.0;
                        self.pet.react(crate::extras::PetState::Happy);
                    }
                }
                row += 1;
                shown += 1;
            }
            let rest = count.saturating_sub(shown + self.editing.map(|_|1).unwrap_or(0));
            if self.todo_choosing{
                c.text(370.0,120.0,if rest>0{"点选修改 · 滚轮查看"}else{"点击要修改的事项"},size::TINY,W::Regular,theme::GRAY,H::Right);
            }else if rest > 0 {
                c.text(370.0, 120.0, &format!("共 {count} 项 · 滚轮查看"), size::TINY, W::Regular, theme::DIM, H::Right);
            }
        }

        if add.clicked() {
            self.todo_choosing=false;
            self.todo_first=0;
            self.adding = true;
            self.editing = None;
            self.buf.clear();
            self.todo_input_focused=false;
        }
        if edit.clicked() && can_edit {
            if count==1 {
                let i=order[0];
                self.editing = Some(i);
                self.adding = false;
                self.buf = self.todos.items[i].text.clone();
                self.todo_input_focused=false;
            }else{self.todo_choosing=!self.todo_choosing;if self.todo_choosing{self.pending.push(Cmd::Focus);}}
        }
    }

    // ---------------------------------------------------------------
    // 邮件：有验证码大卡时分栏，否则显示紧凑单行列表。
    // ---------------------------------------------------------------
    fn visible_mail_code(&self)->Option<&mail::Item> {
        self.mail.code_item().filter(|item|!self.mail.manage.trash_open && self.settings.mail_code && !self.mail_code_hidden(item))
    }
    pub(crate) fn mail_list_bounds(&self)->Rect {
        let left=if self.visible_mail_code().is_some(){182.0}else{36.0};
        let bot = if cfg!(test) { 178.0 } else { 204.0 };
        Rect::from_min_max(Pos2::new(left,54.0),Pos2::new(380.0,bot))
    }
    pub(crate) fn page_mail(&mut self, c: &Card, _env: &Env, cmds: &mut Vec<Cmd>) {
        if !cfg!(test) {
            self.draw_fly_message_island(c, cmds);
            return;
        }
        if self.mail.accounts.is_empty() {
            c.text(42.0, 60.0, "还没有邮箱", 13.0, W::Bold, theme::WHITE, H::Left);
            let hint = c.fit("在设置 → 邮箱里添加；授权码只存进系统凭据管理器", 10.5, W::Regular, 344.0);
            c.text(42.0, 77.0, &hint, 10.5, W::Regular, theme::GRAY, H::Left);
            let (go, gc) = capsule(c, 42.0, 88.0, 84.0, 26.0, "mail-setup", theme::BTN, theme::BTN_HOVER);
            gc.text(58.0, 105.0, "去添加", size::BODY, W::Regular, theme::WHITE, H::Left);
            if go.clicked() {
                self.open_mail_settings();
            }
            return;
        }
        if !cfg!(test) {
            let msg_mode = crate::auralsle_ai_status::config::get().msg_island_filter;
            if msg_mode < 10 {
                self.draw_fly_message_island(c, cmds);
                return;
            }
        }

        self.mail_manage_tick();
        // v0.5.5：点开某封邮件 → 整块换成正文摘要，左上「返回」回到列表。
        if let Some((account, uid)) = self.mail.open_uid.clone() {
            match self.mail_rows()
                .iter()
                .find(|m| m.uid == uid && m.account.eq_ignore_ascii_case(&account))
                .cloned()
            {
                Some(item) => {
                    self.mail_detail(c, &item, cmds);
                    return;
                }
                None => self.mail.open_uid = None,
            }
        }
        let code=self.visible_mail_code().cloned();
        if let Some(raw)=&code {
            c.fill(36.0,36.0,140.0,86.0,11.0,theme::CARD);
            let hidden=self.mail_code_hidden(raw);
            let item=self.display_mail(raw);
            let value=item.code.as_deref().unwrap_or("••••••");
            c.text(46.0,51.0,"验证码",10.5,W::Regular,theme::GRAY,H::Left);
            c.text(166.0,51.0,&c.fit(&item.from,10.5,W::Semibold,78.0),10.5,W::Semibold,theme::WHITE,H::Right);
            let code_size=23.0*(120.0/c.text_w(value,23.0,W::Bold)).min(1.0);
            c.text(46.0,75.0,value,code_size,W::Bold,theme::GREEN,H::Left);
            c.text(46.0,91.0,&c.fit(&item.subject,10.5,W::Regular,120.0),10.5,W::Regular,Color32::from_gray(190),H::Left);
            c.text(46.0,108.0,if hidden{"已隐藏 · 点击查看"}else{"点击复制"},10.5,W::Regular,theme::DIM,H::Left);
            c.text(166.0,108.0,item.time.rsplit(' ').next().unwrap_or(&item.time),10.0,W::Regular,theme::GRAY,H::Right);
            c.hit(42.0,38.0,128.0,16.0,"mail-code-source").on_hover_text(format!("来自：{}\n{}\n收件邮箱：{}\n{}",item.from,item.subject,item.account,item.time));
            if c.hit(40.0,56.0,132.0,38.0,("mail-code",item.uid)).on_hover_text(format!("复制 {} 的验证码",item.from)).clicked(){if hidden{self.reveal_mail_code(raw);}else{cmds.push(Cmd::Copy(value.to_string()));self.code_copied(value);}}
        }
        use crate::mail_manage::{Action,Folder};
        let folder=self.mail.manage.folder();let trash=folder==Folder::Trash;
        let list_bounds=self.mail_list_bounds();let list_x=list_bounds.left();let row_w=list_bounds.width()-14.0;
        let compact=code.is_none();let row_h=if compact{21.5}else{34.0};
        let all_count=self.mail.recent.len();
        let unread=self.mail.recent.iter().filter(|m|self.mail_seen(m,Folder::Inbox)==Some(false)).count();
        if trash{
            c.text(list_x+6.0,47.0,&format!("{} {}",if self.settings.mail_sync_operations{"回收站"}else{"本机回收站"},self.mail.manage.trash.len()),10.5,W::Semibold,theme::WHITE,H::Left);
            c.text(375.0,47.0,"返回邮件",10.0,W::Regular,theme::BLUE,H::Right);
            if c.hit(322.0,33.0,56.0,19.0,"mail-trash-back").clicked(){self.mail_toggle_trash();return;}
        }else{
            let tab_w=if compact{52.0}else{45.0};let tab_gap=if compact{56.0}else{48.0};
            for (x,width,label,selected,id) in [
                (list_x,tab_w,format!("全部 {all_count}"),!self.mail.manage.unread_only,"mail-all"),
                (list_x+tab_gap,tab_w,format!("未读 {unread}"),self.mail.manage.unread_only,"mail-unread"),
            ]{
                if selected{c.fill(x,33.0,width,19.0,6.0,theme::BTN);}
                c.text(x+width/2.0,47.0,&label,10.0,W::Semibold,if selected{theme::WHITE}else{theme::GRAY},H::Center);
                if c.hit(x,33.0,width,19.0,id).on_hover_text("当前加载的最近邮件").clicked(){self.mail.manage.unread_only=id=="mail-unread";}
            }
            if !cfg!(test) {
                let hub_x = if compact { 262.0 } else { 278.0 };
                let hub_w = if compact { 58.0 } else { 52.0 };
                let (sw_r, sw_b) = capsule(c, hub_x, 33.0, hub_w, 19.0, "fly-msg-hub-switch", theme::BTN, theme::BTN_HOVER);
                sw_b.text(hub_x + hub_w * 0.5, 46.2, "返回消息岛", if compact { 8.2 } else { 7.8 }, W::Semibold, Color32::from_rgb(110, 231, 183), H::Center);
                if sw_r.clicked() {
                    crate::auralsle_ai_status::config::update(|cfg| cfg.msg_island_filter = 0);
                    return;
                }
            } else {
                c.text(307.0,46.0,if self.settings.mail_sync_operations{"同步"}else{"仅本机"},8.0,W::Regular,theme::DIM,H::Center);
            }
            let ready=self.mail.manage.pending.is_empty()&&self.mail.accounts.iter().any(|a|a.enabled&&self.mail.manage.data.contains_key(&(crate::mail::key(a),Folder::Inbox))&&self.mail.manage.identities.get(&crate::mail::key(a))==Some(a));
            c.text(377.0,47.0,"全部已读",10.0,W::Regular,if ready{theme::BLUE}else{theme::DIM},H::Right);
            if c.hit(333.0,33.0,47.0,19.0,"mail-read-all").on_hover_text(if self.settings.mail_sync_operations{"将已连接邮箱的未读邮件全部标为已读"}else{"仅将灵屿 AuraIsle 当前加载的邮件标为已读，不更改邮箱"}).clicked()&&ready{
                self.mail_manage_request(None,Folder::Inbox,Action::ReadAll);
            }
        }
        // 底部控件跟随列表底边：放进状态提示、同步模式与回收站入口，避免与列表及页面指示条挤在一起。
        let foot_top = if cfg!(test) { 186.0 } else { list_bounds.bottom() + 4.0 };
        let foot_text_y = foot_top + 11.5;
        if !trash{
            if !cfg!(test) {
                c.text(350.0, foot_text_y, if self.settings.mail_sync_operations{"邮箱同步"}else{"仅本机"}, 8.2, W::Regular, theme::DIM, H::Right);
            }
            let hit=c.hit(359.0,foot_top,22.0,17.0,"mail-trash").on_hover_text("回收站");
            crate::mail_manage_ui::trash_icon(c,370.0,foot_top+8.0,if hit.hovered(){theme::WHITE}else{theme::GRAY});
            if hit.clicked(){self.mail_toggle_trash();return;}
        }
        if !self.mail.manage.message.is_empty(){
            let status_x = if cfg!(test) { 42.0 } else { list_x + 4.0 };
            let status_w = if cfg!(test) { 125.0 } else { (304.0 - status_x).max(80.0) };
            c.text(status_x,foot_text_y,&c.fit(&self.mail.manage.message,8.0,W::Regular,status_w),8.0,W::Regular,theme::GRAY,H::Left);
            if c.hit(status_x-2.0,foot_top,status_w+4.0,16.0,"mail-manage-status").on_hover_text(format!("{}\n点击刷新邮件状态",self.mail.manage.message)).clicked(){self.mail_manage_request(None,folder,Action::Load);}
        }
        let cached_mail_rows = self.mail_rows().to_vec();
        let indices:Vec<_>=cached_mail_rows.iter().enumerate().filter(|(_,m)|trash||!self.mail.manage.unread_only||self.mail_seen(m,folder)==Some(false)).map(|(i,_)|i).collect();
        if indices.is_empty(){
            let message=if !self.mail.manage.pending.is_empty(){"正在同步邮件…"}else if trash{"回收站为空"}else if self.mail.manage.unread_only{"没有未读邮件"}else{"还没有收到邮件"};
            c.text(list_x+6.0,list_bounds.center().y+4.0,message,11.0,W::Regular,theme::GRAY,H::Left);return;
        }
        let mut content=egui::Ui::new(c.ui.ctx().clone(),egui::Id::new(("mail-list",c.live)),
            egui::UiBuilder::new().max_rect(c.rect(list_bounds.left(),list_bounds.top(),list_bounds.width(),list_bounds.height())).layer_id(c.painter.layer_id()).style(c.ui.style().clone()));
        content.set_clip_rect(c.rect(list_bounds.left(),list_bounds.top(),list_bounds.width(),list_bounds.height()).intersect(c.painter.clip_rect()));
        content.set_opacity(c.painter.opacity());if !c.live{content.disable();}
        content.spacing_mut().item_spacing.y=0.0;content.spacing_mut().scroll.floating=false;content.spacing_mut().scroll.bar_width=4.0*c.s;content.spacing_mut().scroll.bar_inner_margin=4.0*c.s;
        content.visuals_mut().widgets.inactive.bg_fill=Color32::from_gray(80);
        egui::ScrollArea::vertical().id_salt(("mail-list-scroll",trash,self.mail.manage.unread_only)).max_height(list_bounds.height()*c.s).auto_shrink([false,false]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible).show_rows(&mut content,row_h*c.s,indices.len(),|ui,range|{
            let rows:Vec<_>=range.map(|i|cached_mail_rows[indices[i]].clone()).collect();
            for raw in rows{
                let item=self.display_mail(&raw);let hidden=self.mail_code_hidden(&raw);let seen=self.mail_seen(&raw,folder);
                let(rect,_)=ui.allocate_exact_size(egui::vec2(row_w*c.s,row_h*c.s),egui::Sense::hover());
                let mut row=Card::new(ui,rect.min,ui.clip_rect());row.s=c.s;row.live=c.live;
                let hit=row.hit(0.0,0.0,row_w,row_h-1.0,("mail-row",item.uid,item.account.as_str()));
                let hovered=ui.ctx().pointer_hover_pos().is_some_and(|p|rect.contains(p)&&ui.clip_rect().contains(p));
                let ready=self.mail_ready(&item.account,folder);let actions=trash||hovered&&ready;
                if hovered{row.fill(0.0,0.0,row_w,row_h-1.0,5.0,Color32::from_gray(29));}
                if seen==Some(false)&&!trash{row.circle(5.0,if compact{10.5}else{8.0},2.1,theme::BLUE);}
                let ink=if seen==Some(true){theme::GRAY}else{theme::WHITE};
                if compact{
                    row.text(12.0,14.0,&row.fit(&item.from,11.0,W::Semibold,57.0),11.0,W::Semibold,ink,H::Left);
                    let subject_w=(row_w-134.0-if hidden{32.0}else{0.0}).max(10.0);
                    let width=row.text(76.0,14.0,&row.fit(&item.subject,10.5,W::Regular,subject_w),10.5,W::Regular,if seen==Some(true){theme::GRAY}else{Color32::from_gray(210)},H::Left);
                    if hidden{row.fill(80.0+width,5.0,29.0,12.0,3.0,theme::BTN);row.text(94.5+width,14.0,"已隐藏",8.0,W::Regular,theme::GRAY,H::Center);}
                }else{
                    row.text(12.0,12.0,&row.fit(&item.from,11.0,W::Semibold,row_w-65.0),11.0,W::Semibold,ink,H::Left);
                    row.text(12.0,27.0,&row.fit(&item.subject,10.0,W::Regular,row_w-20.0),10.0,W::Regular,theme::GRAY,H::Left);
                }
                let cy=if compact{10.5}else{8.5};let mut acted=false;
                if actions{
                    if trash{
                        row.text(row_w-6.0,cy+4.0,"恢复",10.0,W::Regular,if ready{theme::BLUE}else{theme::DIM},H::Right);
                        if row.hit(row_w-40.0,0.0,36.0,20.0,("mail-restore",item.uid,item.account.as_str())).clicked()&&ready{self.mail_manage_request(Some(&item.account),folder,Action::Move{uid:item.uid});acted=true;}
                    }else{
                        row.ring(row_w-34.0,cy,4.7,1.1,theme::GRAY);paint::check(&row,row_w-34.0,cy,6.0,if seen==Some(true){1.0}else{0.0},1.1,theme::GRAY);
                        if row.hit(row_w-44.0,0.0,20.0,20.0,("mail-seen",item.uid,item.account.as_str())).on_hover_text(if seen==Some(true){"标为未读"}else{"标为已读"}).clicked(){self.mail_manage_request(Some(&item.account),folder,Action::Read{uid:item.uid,seen:seen!=Some(true)});acted=true;}
                        let remove=row.hit(row_w-23.0,0.0,21.0,20.0,("mail-delete",item.uid,item.account.as_str())).on_hover_text(if self.settings.mail_sync_operations{"移到邮箱回收站"}else{"移到本机回收站，不更改邮箱"});
                        crate::mail_manage_ui::trash_icon(&row,row_w-13.0,cy,if remove.hovered(){theme::RED}else{theme::GRAY});
                        if remove.clicked(){self.mail_manage_request(Some(&item.account),folder,Action::Move{uid:item.uid});acted=true;}
                    }
                }else{row.text(row_w-6.0,cy+4.0,item.time.rsplit(' ').next().unwrap_or(&item.time),10.0,W::Regular,theme::GRAY,H::Right);}
                row.seg((6.0,row_h-0.5),(row_w-6.0,row_h-0.5),0.5,Color32::from_gray(40));
                hit.clone().on_hover_text(format!("{}\n{}\n收件邮箱：{}\n{}",item.from,item.subject,item.account,item.time));
                if hit.clicked()&&!acted{self.mail_open(&raw);}
            }
        });
    }

    /// 岛内邮件详情可滚动，也可以进入独立窗口阅读全文。
    pub(crate) fn mail_detail(&mut self, c: &Card, raw: &mail::Item, cmds: &mut Vec<Cmd>) {
        let hidden=self.mail_code_hidden(raw);
        let display=self.display_mail(raw);let item=&display;
        let body=self.mail_body_lines(item);
        c.fill(36.0, 36.0, 347.0, 168.0, 11.0, theme::CARD);
        let back = c.hit(330.0, 42.0, 44.0, 14.0, ("mail-back", item.uid));
        let bh = c.hover_t(&back).0;
        c.text(374.0, 50.0, "返回", 10.5, W::Regular, if bh > 0.5 { theme::WHITE } else { theme::GRAY }, H::Right);
        if back.clicked() {
            self.mail.open_uid = None;
            return;
        }
        let folder=self.mail.manage.folder();let ready=self.mail_ready(&item.account,folder);
        let seen=self.mail_seen(raw,folder)==Some(true);
        let status=if folder==crate::mail_manage::Folder::Trash{"恢复"}else if seen{"标为未读"}else{"标为已读"};
        c.text(215.0,50.0,status,9.0,W::Regular,if ready{theme::BLUE}else{theme::DIM},H::Right);
        if c.hit(166.0,39.0,50.0,18.0,("mail-detail-seen",item.uid)).clicked()&&ready{
            let action=if folder==crate::mail_manage::Folder::Trash{crate::mail_manage::Action::Move{uid:item.uid}}else{crate::mail_manage::Action::Read{uid:item.uid,seen:!seen}};
            self.mail_manage_request(Some(&item.account),folder,action);
        }
        if folder==crate::mail_manage::Folder::Inbox{
            c.text(252.0,50.0,"删除",9.0,W::Regular,if ready{theme::RED}else{theme::DIM},H::Right);
            if c.hit(220.0,39.0,35.0,18.0,("mail-detail-delete",item.uid)).on_hover_text(if self.settings.mail_sync_operations{"移到邮箱回收站"}else{"移到本机回收站，不更改邮箱"}).clicked()&&ready{self.mail_manage_request(Some(&item.account),folder,crate::mail_manage::Action::Move{uid:item.uid});}
        }
        let from = c.fit(&item.from, 11.5, W::Semibold, 112.0);
        c.text(46.0, 50.0, &from, 11.5, W::Semibold, theme::WHITE, H::Left);
        let read=c.hit(266.0,42.0,60.0,14.0,("mail-read",&item.account,item.uid)).on_hover_text("在独立窗口阅读完整正文");
        c.text(324.0,50.0,"阅读全文",10.5,W::Regular,if read.hovered(){theme::BLUE}else{theme::GRAY},H::Right);
        if read.clicked(){self.mail_open_reader(raw);}
        let subject = c.fit(&item.subject, 13.5, W::Semibold, 328.0);
        c.text(46.0, 73.0, &subject, 13.5, W::Semibold, theme::WHITE, H::Left);
        c.hit(46.0,58.0,328.0,20.0,("mail-subject",&item.account,item.uid)).on_hover_text(&item.subject);
        c.text(374.0, 88.0, &item.time, 10.0, W::Regular, Color32::from_gray(170), H::Right);
        let account=format!("收件邮箱：{}",item.account);
        c.text(46.0,88.0,&c.fit(&account,10.0,W::Regular,230.0),10.0,W::Regular,Color32::from_gray(175),H::Left);
        c.hit(46.0,76.0,230.0,16.0,("mail-account",&item.account,item.uid)).on_hover_text(&account);

        // 正文和链接共用滚动区域，会议与验证码操作保留在底部。
        let mut content=egui::Ui::new(c.ui.ctx().clone(),egui::Id::new(("mail-content",&item.account,item.uid,c.live)),
            egui::UiBuilder::new().max_rect(c.rect(46.0,94.0,328.0,82.0)).layer_id(c.painter.layer_id()).style(c.ui.style().clone()));
        content.set_clip_rect(c.rect(46.0,94.0,328.0,82.0).intersect(c.painter.clip_rect()));
        content.set_opacity(c.painter.opacity());
        if !c.live {content.disable();}
        content.spacing_mut().item_spacing.y=6.0*c.s;
        content.style_mut().spacing.scroll.floating=false;
        content.spacing_mut().scroll.bar_width=4.0*c.s;
        egui::ScrollArea::vertical().id_salt(("mail-body",&item.account,item.uid)).max_height(82.0*c.s).auto_shrink([false,false]).show(&mut content,|ui|{
            if !body.is_empty() {
                crate::mail_reader::body_ui(ui,body.as_ref(),11.5*c.s);
            }else if item.links.is_empty(){
                ui.label(egui::RichText::new("（这封邮件没有正文）").font(c.font(12.0,W::Regular)).color(theme::DIM));
            }
            for link in &item.links {
                let text=format!("{} ↗",c.fit(&link.label,12.0,W::Regular,295.0));
                if ui.add(egui::Link::new(egui::RichText::new(text).font(c.font(12.0,W::Regular)).color(theme::BLUE))).on_hover_text(&link.url).clicked(){
                    cmds.push(Cmd::OpenUrl(link.url.clone()));
                }
            }
        });

        if !self.mail.manage.message.is_empty(){
            c.text(374.0,172.0,&c.fit(&self.mail.manage.message,8.0,W::Regular,150.0),8.0,W::Regular,theme::GRAY,H::Right);
            c.hit(220.0,162.0,156.0,15.0,"mail-detail-status").on_hover_text(&self.mail.manage.message);
        }
        // 底部一行：会议时间（绿）优先，其次验证码（点一下复制）。
        c.seg((46.0,181.0),(374.0,181.0),0.6,Color32::from_gray(48));
        let y = 196.0;
        if hidden {
            c.text(46.0,y,"验证码已隐藏（不代表失效）",11.0,W::Regular,theme::GRAY,H::Left);
            c.text(374.0,y,"查看 15 秒",10.5,W::Regular,theme::BLUE,H::Right);
            if c.hit(290.0,y-14.0,86.0,24.0,("mail-reveal",item.uid)).clicked(){self.reveal_mail_code(raw);}
        } else if let Some(m) = &item.meeting {
            let ignored=self.mail_event_ignored(m);
            c.text(46.0,y,&format!("{} · {}",if ignored{"已忽略"}else{"日程"},m.at),10.5,W::Regular,if ignored{theme::DIM}else{theme::GREEN},H::Left);
            let label=if ignored{"恢复日程"}else{"忽略日程"};
            c.text(374.0,y,label,10.5,W::Regular,theme::GRAY,H::Right);
            if c.hit(316.0,y-14.0,60.0,24.0,("mail-ignore-event",item.uid)).on_hover_text(&m.title).clicked(){self.toggle_mail_event(m);}
        } else if let Some(code) = &item.code {
            let label = format!("验证码 {code}");
            let w = c.text(46.0, y, &label, 13.0, W::Semibold, theme::GREEN, H::Left);
            c.text(46.0 + w + 6.0, y, "点击复制", 10.5, W::Regular, theme::DIM, H::Left);
            let hit = c.hit(42.0, y - 14.0, w + 60.0, 24.0, ("mail-detail-code", item.uid));
            if hit.clicked() {
                cmds.push(Cmd::Copy(code.clone()));
                self.code_copied(code);
            }
        }
    }

    /// 完美复刻 flyl1u/fly-island 的「消息岛」UI（图七）：
    /// 顶部水平排列 `[微信 •]`、`[QQ n]`、`[邮箱 n]` 图标胶囊切换栏 + 右侧清空电源图标，
    /// 下方为高对比分割线消息流列表（加粗发送者/群名、正文摘要、右侧 HH:MM 时间戳与独立滚动条）。
    fn draw_fly_message_island(&mut self, c: &Card, cmds: &mut Vec<Cmd>) {
        let cfg_now = crate::auralsle_ai_status::config::get();
        // source_id: 0 = 邮箱（默认第一个！）, 1 = 微信, 2 = QQ
        let cur_src = (cfg_now.msg_island_filter % 3) as usize;
        let order = cfg_now.msg_hub_order;
        let wx_cnt = self.pending_messengers.iter().filter(|u| u.source == crate::app::Messenger::Wechat).count()
            + self.notification_log.items.iter().filter(|r| r.app_name.contains("微信") || r.app_id.contains("wechat") || r.app_id.contains("weixin")).count();
        let qq_cnt = self.pending_messengers.iter().filter(|u| u.source == crate::app::Messenger::Qq).count()
            + self.notification_log.items.iter().filter(|r| is_strict_qq_chat_record(r)).count();
        let mail_cnt = self.mail.recent.len();

        let ctx_msg = c.ui.ctx().clone();
        let active_pos = order.iter().position(|&id| (id as usize).min(2) == cur_src).unwrap_or(0);
        let target_tab_x = 40.0 + (active_pos as f32) * (66.0 + 7.0);
        let slide_tab_x = ctx_msg.animate_value_with_time(egui::Id::new("fly-msg-tab-slide-x"), target_tab_x, 0.18);
        if (slide_tab_x - target_tab_x).abs() > 0.2 {
            ctx_msg.request_repaint();
        }
        // [需求 11 · React Bits 丝滑滑动胶囊底座]
        c.fill(slide_tab_x, 12.0, 66.0, 24.0, 8.0, Color32::from_white_alpha(22));
        c.stroke(slide_tab_x, 12.0, 66.0, 24.0, 8.0, 0.9, crate::auralsle_ai_status::plugins::active_accent_color().gamma_multiply(0.85));

        let sw_id = egui::Id::new("fly-msg-last-src");
        let sw_at_id = egui::Id::new("fly-msg-sw-at");
        let last_src = ctx_msg.data(|d| d.get_temp::<usize>(sw_id)).unwrap_or(cur_src);
        if last_src != cur_src {
            let now_s = self.t;
            ctx_msg.data_mut(|d| {
                d.insert_temp(sw_id, cur_src);
                d.insert_temp(sw_at_id, now_s);
            });
        }
        let sw_at = ctx_msg.data(|d| d.get_temp::<f64>(sw_at_id)).unwrap_or(0.0);
        let msg_enter_t = if sw_at <= 0.0 {
            1.0_f32
        } else {
            let el = ((self.t - sw_at) as f32 / 0.22).clamp(0.0, 1.0);
            if el < 0.995 {
                ctx_msg.request_repaint();
            }
            1.0 - (1.0 - el).powi(3)
        };

        let mut tab_x = 40.0_f32;
        for &raw_id in &order {
            let src_id = (raw_id as usize).min(2);
            let (name, cnt) = match src_id {
                0 => ("邮箱", mail_cnt),
                1 => ("微信", wx_cnt),
                _ => ("QQ", qq_cnt),
            };
            let selected = cur_src == src_id;
            let pill_w = 66.0_f32;
            let pill_y = 12.0_f32;
            let pill_h = 24.0_f32;
            // 精致图标：0=矢量信封邮箱图标, 1=微信图标, 2=QQ图标
            let ic_x = tab_x + 14.0;
            let ic_y = pill_y + 12.0;
            if src_id == 0 {
                // 精致圆角信封图标（替代原本简陋的双圆点）
                c.fill(ic_x - 7.5, ic_y - 6.0, 15.0, 12.0, 3.2, Color32::from_rgb(56, 189, 248));
                c.stroke(ic_x - 5.2, ic_y - 3.8, 10.4, 7.6, 1.2, 1.05, Color32::WHITE);
                c.seg((ic_x - 4.8, ic_y - 3.2), (ic_x, ic_y + 0.6), 1.05, Color32::WHITE);
                c.seg((ic_x, ic_y + 0.6), (ic_x + 4.8, ic_y - 3.2), 1.05, Color32::WHITE);
            } else if src_id == 1 {
                crate::extras::messenger_icon(&c.scaled(ic_x, ic_y, 0.82), ic_x, ic_y, crate::app::Messenger::Wechat);
            } else {
                crate::extras::messenger_icon(&c.scaled(ic_x, ic_y, 0.82), ic_x, ic_y, crate::app::Messenger::Qq);
            }
            c.text(
                tab_x + 25.5,
                pill_y + 16.2,
                name,
                10.2,
                if selected { W::Bold } else { W::Semibold },
                if selected { theme::WHITE } else { theme::GRAY },
                H::Left,
            );
            if cnt > 0 {
                c.text(tab_x + pill_w - 7.5, pill_y + 16.0, &cnt.min(99).to_string(), 9.0, W::Semibold, theme::GRAY, H::Right);
            }
            if c.hit(tab_x, pill_y, pill_w, pill_h, ("fly-msg-tab", src_id)).clicked() {
                crate::auralsle_ai_status::config::update(|cfg| cfg.msg_island_filter = src_id as u8);
            }
            tab_x += pill_w + 7.0;
        }

        // 胶囊栏右侧：同行排列 `[排序]` + `[🗑 清空]`（已移除旧版邮箱岛入口，全面统一使用消息岛）
        let (ord_r, ord_b) = capsule(c, 292.0, 15.0, 36.0, 18.0, "fly-msg-order-btn", Color32::from_white_alpha(14), Color32::from_white_alpha(26));
        ord_b.text(310.0, 27.5, "排序", 8.0, W::Semibold, theme::GRAY, H::Center);
        if ord_r.on_hover_text("切换 邮箱 / 微信 / QQ 选项卡顺序").clicked() {
            let next_order = match order {
                [0, 1, 2] => [1, 2, 0],
                [1, 2, 0] => [0, 2, 1],
                [0, 2, 1] => [1, 0, 2],
                [1, 0, 2] => [2, 1, 0],
                _ => [0, 1, 2],
            };
            crate::auralsle_ai_status::config::update(|cfg| {
                cfg.msg_hub_order = next_order;
                cfg.msg_island_filter = next_order[0];
            });
        }

        let (clr_r, clr_b) = capsule(
            c,
            332.0,
            15.0,
            46.0,
            18.0,
            "fly-msg-clear-btn",
            Color32::from_rgba_unmultiplied(239, 68, 68, 38),
            Color32::from_rgba_unmultiplied(239, 68, 68, 76),
        );
        paint::trash_can(&clr_b, 341.5, 24.0, 9.0, Color32::from_rgb(252, 165, 165));
        clr_b.text(360.5, 27.5, "清空", 8.0, W::Semibold, Color32::from_rgb(254, 202, 202), H::Center);
        if clr_r.on_hover_text("一键清空当前分栏的历史消息与未读提醒").clicked() {
            if cur_src == 1 {
                self.notification_log.items.retain(|r| {
                    !(r.app_name.contains("微信")
                        || r.app_id.contains("wechat")
                        || r.app_id.contains("weixin")
                        || r.body.contains("微信"))
                });
                self.notification_log.save();
                self.pending_messengers.retain(|u| u.source != crate::app::Messenger::Wechat);
                self.say("已清空微信历史消息");
            } else if cur_src == 2 {
                self.notification_log.items.retain(|r| !is_strict_qq_chat_record(r));
                self.notification_log.save();
                self.pending_messengers.retain(|u| u.source != crate::app::Messenger::Qq);
                self.say("已清空 QQ 历史消息");
            } else {
                self.mail.recent.clear();
                self.say("已清空邮箱历史消息列表");
            }
        }

        let slot_filter_id = egui::Id::new(("fly-msg-slot-filter", cur_src));
        let mut slot_filter = if cur_src == 1 || cur_src == 2 {
            ctx_msg.data(|d| d.get_temp::<u8>(slot_filter_id)).unwrap_or(0).min(2)
        } else {
            0
        };
        let mut list_top = 43.0_f32;
        if cur_src == 1 || cur_src == 2 {
            let mut main_cnt = 0usize;
            let mut clone_cnt = 0usize;
            for r in &self.notification_log.items {
                let matched = if cur_src == 1 {
                    r.app_name.contains("微信") || r.app_id.contains("wechat") || r.app_id.contains("weixin") || r.body.contains("微信")
                } else {
                    is_strict_qq_chat_record(r)
                };
                if matched {
                    if is_clone_notification_record(r) {
                        clone_cnt += 1;
                    } else {
                        main_cnt += 1;
                    }
                }
            }
            let total_cnt = main_cnt + clone_cnt;
            let chips = [
                (0u8, format!("全部({total_cnt})")),
                (1u8, format!("主应用({main_cnt})")),
                (2u8, format!("分身({clone_cnt})")),
            ];
            let mut sx = 40.0_f32;
            for (fid, label) in chips {
                let cw = (c.text_w(&label, 8.6, W::Semibold) + 14.0).clamp(44.0, 96.0);
                let active = slot_filter == fid;
                let hit = c.hit(sx, 41.5, cw, 16.0, ("fly-msg-slot-chip", cur_src, fid));
                let bg = if active {
                    if fid == 2 {
                        Color32::from_rgba_unmultiplied(16, 185, 129, 64)
                    } else {
                        Color32::from_white_alpha(30)
                    }
                } else if hit.hovered() {
                    Color32::from_white_alpha(18)
                } else {
                    Color32::from_white_alpha(9)
                };
                c.fill(sx, 41.5, cw, 16.0, 6.0, bg);
                if active {
                    let bcol = if fid == 2 {
                        Color32::from_rgb(52, 211, 153)
                    } else {
                        crate::auralsle_ai_status::plugins::active_accent_color()
                    };
                    c.stroke(sx, 41.5, cw, 16.0, 6.0, 0.8, bcol.gamma_multiply(0.85));
                }
                let tcol = if active { theme::WHITE } else { theme::GRAY };
                c.text(sx + cw * 0.5, 52.3, &label, 8.4, if active { W::Bold } else { W::Semibold }, tcol, H::Center);
                if hit.clicked() && slot_filter != fid {
                    slot_filter = fid;
                    ctx_msg.data_mut(|d| d.insert_temp(slot_filter_id, fid));
                }
                sx += cw + 6.0;
            }
            list_top = 62.0;
        }

        // 收集当前分栏的消息列表（带帧间缓存：仅当通知数/邮件数/分栏/主分身筛选变化时重建，滑动列表时零分配！）
        // Row: (title, body, time_str, unread_opt, mail_opt, slot_tag: Option<(&'static str, bool)>)
        type MsgRowTuple = (String, String, String, Option<crate::app::Unread>, Option<(String, u32)>, Option<(&'static str, bool)>);
        static MSG_ROWS_CACHE: std::sync::OnceLock<std::sync::Mutex<((usize, u8, usize, i64, usize, u32), Vec<MsgRowTuple>)>> = std::sync::OnceLock::new();
        let notif_len = self.notification_log.items.len();
        let last_notif_ts = self.notification_log.items.first().map(|r| r.at).unwrap_or(0);
        let mail_len = self.mail.recent.len();
        let last_mail_uid = self.mail.recent.first().map(|m| m.uid).unwrap_or(0);
        let cache_key = (cur_src, slot_filter, notif_len, last_notif_ts, mail_len, last_mail_uid);
        let rows: Vec<MsgRowTuple> = {
            let mtx = MSG_ROWS_CACHE.get_or_init(|| std::sync::Mutex::new(((99, 0, 0, 0, 0, 0), Vec::new())));
            let mut guard = mtx.lock().unwrap();
            if guard.0 != cache_key {
                let mut built: Vec<MsgRowTuple> = Vec::new();
                if cur_src == 1 {
                    for r in &self.notification_log.items {
                        if r.app_name.contains("微信") || r.app_id.contains("wechat") || r.app_id.contains("weixin") || r.body.contains("微信") {
                            let is_clone = is_clone_notification_record(r);
                            if (slot_filter == 1 && is_clone) || (slot_filter == 2 && !is_clone) {
                                continue;
                            }
                            let t_str = chrono::DateTime::from_timestamp(r.at, 0)
                                .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
                                .unwrap_or_else(|| "刚刚".into());
                            let title = if r.title.is_empty() || r.title == "微信" || r.title == "微信分身" {
                                r.body.split_once(':').or_else(|| r.body.split_once('：')).map(|(a, _)| a.trim().to_owned()).unwrap_or_else(|| if is_clone { "微信分身".into() } else { "微信消息".into() })
                            } else {
                                r.title.strip_prefix("微信 · ").map(str::to_owned).unwrap_or_else(|| r.title.clone())
                            };
                            let body = r.body.split_once(':').or_else(|| r.body.split_once('：')).map(|(_, b)| b.trim().to_owned()).unwrap_or_else(|| r.body.clone());
                            built.push((title, body, t_str, Some(self.latest_unread(crate::app::Messenger::Wechat)), None, notification_record_slot_tag(r)));
                        }
                    }
                } else if cur_src == 2 {
                    for r in &self.notification_log.items {
                        if is_strict_qq_chat_record(r) {
                            let is_clone = is_clone_notification_record(r);
                            if (slot_filter == 1 && is_clone) || (slot_filter == 2 && !is_clone) {
                                continue;
                            }
                            let t_str = chrono::DateTime::from_timestamp(r.at, 0)
                                .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
                                .unwrap_or_else(|| "刚刚".into());
                            let title = r.title.strip_prefix("QQ · ").map(str::to_owned).unwrap_or_else(|| r.title.clone());
                            built.push((title, r.body.clone(), t_str, Some(self.latest_unread(crate::app::Messenger::Qq)), None, notification_record_slot_tag(r)));
                        }
                    }
                } else {
                    for m in self.mail_rows().into_iter().take(24) {
                        let disp = self.display_mail(&m);
                        let t_short = disp.time.rsplit(' ').next().unwrap_or(&disp.time).to_string();
                        built.push((disp.from, disp.subject, t_short, None, Some((m.account.clone(), m.uid)), None));
                    }
                }
                guard.0 = cache_key;
                guard.1 = built;
            }
            guard.1.clone()
        };

        if rows.is_empty() {
            let line_y = if cur_src == 1 || cur_src == 2 { 60.0 } else { 41.0 };
            c.seg((40.0, line_y), (376.0, line_y), 0.7, Color32::from_white_alpha(18));
            let empty_title = match (cur_src, slot_filter) {
                (1, 2) => "暂无微信分身历史消息",
                (1, 1) => "暂无微信主应用历史消息",
                (1, _) => "暂无微信历史消息",
                (2, 2) => "暂无 QQ 分身历史消息",
                (2, 1) => "暂无 QQ 主应用历史消息",
                (2, _) => "暂无 QQ 历史消息",
                _ => "暂无邮箱消息",
            };
            c.text(209.0, 122.0, empty_title, 12.0, W::Bold, Color32::from_rgb(161, 161, 170), H::Center);
            c.text(209.0, 142.0, "收到新消息后将在此处实时聚合展示", 9.5, W::Regular, theme::DIM, H::Center);
            return;
        }

        // 顶部分割线
        let line_y = if cur_src == 1 || cur_src == 2 { 60.0 } else { 41.0 };
        c.seg((40.0, line_y), (376.0, line_y), 0.7, Color32::from_white_alpha(18));

        // 列表渲染区（充分利用加长后的岛体高度，完整容纳消息与分身区分标记！）
        let msg_bottom = ((self.h.pos / c.s) - 22.0 - 25.0).max(168.0);
        let list_rect = Rect::from_min_max(Pos2::new(40.0, list_top), Pos2::new(378.0, msg_bottom));
        let row_h = 44.0_f32;
        let mut content = egui::Ui::new(
            c.ui.ctx().clone(),
            egui::Id::new(("fly-msg-hub-list", cur_src, c.live)),
            egui::UiBuilder::new()
                .max_rect(c.rect(list_rect.left(), list_rect.top(), list_rect.width(), list_rect.height()))
                .layer_id(c.painter.layer_id())
                .style(c.ui.style().clone()),
        );
        content.set_clip_rect(c.rect(list_rect.left(), list_rect.top(), list_rect.width(), list_rect.height()).intersect(c.painter.clip_rect()));
        content.set_opacity(c.painter.opacity());
        if !c.live {
            content.disable();
        }
        content.spacing_mut().item_spacing.y = 0.0;
        content.spacing_mut().scroll.floating = false;
        content.spacing_mut().scroll.bar_width = 3.5 * c.s;
        egui::ScrollArea::vertical()
            .id_salt(("fly-msg-hub-scroll", cur_src, c.live))
            .max_height(list_rect.height() * c.s)
            .auto_shrink([false, false])
            .show_rows(&mut content, row_h * c.s, rows.len(), |ui, range| {
                for idx in range {
                    let (ref title, ref body, ref time_str, unread_opt, ref mail_opt, slot_tag) = rows[idx];
                    let r_w = list_rect.width() - 10.0;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(r_w * c.s, row_h * c.s), egui::Sense::hover());
                    let stagger = ((idx.min(5) as f32) * 0.06).min(0.30);
                    let row_ease = ((msg_enter_t - stagger) / (1.0 - stagger).max(0.25)).clamp(0.0, 1.0);
                    let row_ease = 1.0 - (1.0 - row_ease).powi(3);
                    let y_lift = (1.0 - row_ease) * 8.0 * c.s;
                    let mut rc = Card::new(ui, rect.min + egui::vec2(0.0, y_lift), ui.clip_rect()).faded(0.25 + 0.75 * row_ease);
                    rc.s = c.s;
                    rc.live = c.live;
                    let hit = rc.hit(0.0, 0.0, r_w, row_h - 1.0, ("fly-msg-row", cur_src, idx));
                    if hit.hovered() {
                        rc.fill(0.0, 1.0, r_w, row_h - 2.0, 7.0, Color32::from_white_alpha(14));
                    }
                    let badge_reserve = if let Some((tag_label, _)) = slot_tag {
                        rc.text_w(tag_label, 7.8, W::Semibold) + 16.0
                    } else {
                        0.0
                    };
                    let t_fit = rc.fit(title, 11.2, W::Bold, (r_w - 52.0 - badge_reserve).max(60.0));
                    rc.text(4.0, 16.5, &t_fit, 11.2, W::Bold, theme::WHITE, H::Left);
                    if let Some((tag_label, is_clone)) = slot_tag {
                        let tw = rc.text_w(&t_fit, 11.2, W::Bold);
                        let bw = rc.text_w(tag_label, 7.8, W::Semibold) + 10.0;
                        let bx = (4.0 + tw + 6.0).min(r_w - 46.0 - bw);
                        let (bg_col, fg_col) = if is_clone {
                            (Color32::from_rgba_unmultiplied(16, 185, 129, 52), Color32::from_rgb(110, 231, 183))
                        } else {
                            (Color32::from_rgba_unmultiplied(56, 189, 248, 42), Color32::from_rgb(125, 211, 252))
                        };
                        rc.fill(bx, 6.5, bw, 13.5, 4.0, bg_col);
                        rc.centered_text(bx + bw * 0.5, 13.2, tag_label, 7.8, W::Semibold, fg_col, H::Center);
                    }
                    rc.text(r_w - 4.0, 16.0, time_str, 9.2, W::Regular, theme::GRAY, H::Right);
                    let b_fit = rc.fit(body, 10.0, W::Regular, r_w - 12.0);
                    rc.text(4.0, 34.0, &b_fit, 10.0, W::Regular, Color32::from_gray(178), H::Left);
                    rc.seg((2.0, row_h - 0.6), (r_w - 2.0, row_h - 0.6), 0.6, Color32::from_white_alpha(14));
                    if hit.clicked() {
                        if let Some(u) = unread_opt {
                            cmds.push(Cmd::ViewMessenger(u));
                        } else if let Some((acc, uid)) = mail_opt.clone() {
                            crate::auralsle_ai_status::config::update(|cfg| cfg.msg_island_filter = 10);
                            self.mail.open_uid = Some((acc, uid));
                        }
                    }
                }
            });
    }


    /// 绘制 Fly-Island 风格垃圾桶矢量图标 `🗑`
    fn draw_fly_trash_icon(c: &Card, cx: f32, cy: f32, col: Color32) {
        c.seg((cx - 4.8, cy - 3.6), (cx + 4.8, cy - 3.6), 1.2, col);
        c.seg((cx - 2.0, cy - 5.0), (cx + 2.0, cy - 5.0), 1.2, col);
        c.stroke(cx - 3.6, cy - 3.2, 7.2, 8.2, 1.2, 1.15, col);
        c.seg((cx - 1.3, cy - 1.2), (cx - 1.3, cy + 2.8), 1.0, col);
        c.seg((cx + 1.3, cy - 1.2), (cx + 1.3, cy + 2.8), 1.0, col);
    }

    /// 绘制 Fly-Island 风格五角星图标 `★`
    fn draw_fly_star_icon(c: &Card, cx: f32, cy: f32, r: f32, filled: bool, col: Color32) {
        let mut pts = Vec::with_capacity(10);
        for i in 0..10 {
            let angle = -std::f32::consts::FRAC_PI_2 + (i as f32) * std::f32::consts::PI / 5.0;
            let rad = if i % 2 == 0 { r } else { r * 0.44 };
            pts.push(c.pos(cx + angle.cos() * rad, cy + angle.sin() * rad));
        }
        if filled {
            let center = c.pos(cx, cy);
            let mut mesh = egui::Mesh::default();
            let c_idx = mesh.vertices.len() as u32;
            mesh.colored_vertex(center, col);
            for &p in &pts {
                mesh.colored_vertex(p, col);
            }
            for i in 0..10u32 {
                let next = (i + 1) % 10;
                mesh.add_triangle(c_idx, c_idx + 1 + i, c_idx + 1 + next);
            }
            c.painter.add(egui::Shape::mesh(mesh));
        } else {
            pts.push(pts[0]);
            c.painter.add(egui::Shape::line(pts, egui::Stroke::new(1.15 * c.s, col)));
        }
    }

    /// 绘制 Fly-Island 风格剪贴板小图标 `📋`
    fn draw_fly_clip_row_icon(c: &Card, cx: f32, cy: f32, col: Color32) {
        c.stroke(cx - 4.6, cy - 5.2, 9.2, 10.8, 1.8, 1.15, col);
        c.fill(cx - 2.4, cy - 6.2, 4.8, 2.2, 0.9, col);
        c.seg((cx - 2.4, cy - 1.2), (cx + 2.4, cy - 1.2), 1.0, col);
        c.seg((cx - 2.4, cy + 1.4), (cx + 2.4, cy + 1.4), 1.0, col);
        c.seg((cx - 2.4, cy + 3.6), (cx + 0.8, cy + 3.6), 1.0, col);
    }

    /// 完美移植 flyl1u/fly-island 剪贴板独立页面 UI（图一 image_fd8cd9.png）：
    /// - 顶栏：左侧 `📋 剪贴板历史  N  ★ M`，右侧 `⌨ Ctrl+Shift+V` + `🗑` 清空按钮
    /// - 次栏：左侧 `全部` / `收藏` 选项卡（带底边指示线），右侧圆角搜索框 `🔍 搜索内容或备注`
    /// - 列表：平滑滚动列表（`📋` 图标 + 单行内容 + `✎ 备注` + `★` 收藏按钮 + 细分割线）
    /// [MoFeng2223/BrimDeck 移植] 图文混合剪贴板面板（需求 6a：支持保存之前复制的文字与图片、图片缩略图预览、链接/代码/颜色专属卡片样式，点击一键再次粘贴）：
    fn draw_brimdeck_clipboard_ui(&mut self, c: &Card, items: &[crate::clipboard::Clip]) {
        let c_shifted = c.shifted(0.0, if cfg!(test) { 0.0 } else { 8.0 });
        let c = &c_shifted;
        let cfg = crate::auralsle_ai_status::config::get();
        let acc = crate::auralsle_ai_status::plugins::active_accent_color();

        // 1) 独立次顶栏（已在 render_page 下移避让状态栏，位于日期时间正下方 y=32..50，绝不遮挡顶栏日期时间）：
        // 左侧显示记录统计，右侧排列 [+ 演示图片] [⚡ 一键粘贴] [清空]
        let total_n = items.len();
        let starred_n = items.iter().filter(|it| it.pinned).count();
        Self::draw_fly_clip_row_icon(c, 44.0, 10.8, Color32::from_gray(210));
        c.text(
            54.0,
            14.2,
            &format!("共 {total_n} 条 · ★ {starred_n}"),
            9.2,
            W::Semibold,
            Color32::from_gray(182),
            H::Left,
        );

        let (img_r, img_c) = capsule(c, 204.0, 2.0, 58.0, 17.5, "bd-clip-demo-img", theme::BTN, theme::BTN_HOVER);
        img_c.text(233.0, 13.8, "＋ 演示图片", 7.8, W::Semibold, Color32::from_rgb(125, 211, 252), H::Center);
        if img_r.on_hover_text("向剪贴板历史插入一张高分辨率演示图片，体验 BrimDeck 图片卡片与一键粘贴").clicked() {
            if crate::clipboard::insert_demo_image_clip(&self.clipboard).is_some() {
                self.say("已添加演示图片至剪贴板面板");
            }
        }

        let auto_p = cfg.clipboard_auto_paste;
        let (ap_r, ap_c) = capsule(
            c,
            266.0,
            2.0,
            66.0,
            17.5,
            "bd-clip-autopaste",
            if auto_p { Color32::from_rgba_unmultiplied(16, 185, 129, 52) } else { theme::BTN },
            theme::BTN_HOVER,
        );
        ap_c.text(
            299.0,
            13.8,
            if auto_p { "⚡ 一键粘贴" } else { "仅复制" },
            7.8,
            W::Semibold,
            if auto_p { Color32::from_rgb(110, 231, 183) } else { theme::GRAY },
            H::Center,
        );
        if ap_r.on_hover_text("开启后：点击任意文字或图片卡片即刻写回系统剪贴板并自动向当前工作窗口发送 Ctrl+V 粘贴").clicked() {
            crate::auralsle_ai_status::config::update(|c_mut| c_mut.clipboard_auto_paste = !auto_p);
        }

        let (clr_r, clr_c) = capsule(c, 336.0, 2.0, 42.0, 17.5, "bd-clip-clear", Color32::from_rgba_unmultiplied(239, 68, 68, 38), Color32::from_rgba_unmultiplied(239, 68, 68, 72));
        clr_c.text(357.0, 13.8, "清空", 7.8, W::Semibold, Color32::from_rgb(252, 165, 165), H::Center);
        if clr_r.on_hover_text("清空未固定的剪贴板历史").clicked() {
            self.clipboard.lock().clear_recent();
            self.say("已清空未固定的剪贴板记录");
        }

        // 2) 次栏 (y = 26..48)：BrimDeck 分类胶囊栏 (`全部` / `收藏` / `图片` / `链接` / `代码`) —
        // [修复需求 4 图三 `image_aa5c1c.jpg`]：引入 React Bits 120Hz 丝滑滑动胶囊底座 (`bd-cat-slide-x`) + 选中缩放与列表平滑入场过渡动画！
        let filter_tab = cfg.clip_filter_tab % 5;
        let cats = [
            (0u8, "全部"),
            (1u8, "收藏"),
            (2u8, "图片"),
            (3u8, "链接"),
            (4u8, "代码"),
        ];
        let ctx_anim = c.ui.ctx().clone();
        let cw = 36.0_f32;
        let cgap = 4.0_f32;
        let target_slide_x = 38.0 + (filter_tab as f32) * (cw + cgap);
        let slide_x = ctx_anim.animate_value_with_time(egui::Id::new("bd-cat-slide-x"), target_slide_x, 0.18);
        if (slide_x - target_slide_x).abs() > 0.2 {
            ctx_anim.request_repaint();
        }
        // 统一轨道槽 + 丝滑滑动高亮胶囊
        c.fill(36.0, 24.5, 5.0 * cw + 4.0 * cgap + 4.0, 23.0, 7.5, Color32::from_white_alpha(9));
        c.fill(slide_x, 26.0, cw, 20.0, 6.0, acc.gamma_multiply(0.32));
        c.stroke(slide_x, 26.0, cw, 20.0, 6.0, 0.95, acc);
        c.fill(slide_x + 8.0, 44.2, cw - 16.0, 1.6, 0.8, theme::WHITE);

        let mut cx_cat = 38.0_f32;
        for (cat_id, cat_lbl) in cats {
            let sel = filter_tab == cat_id;
            let sel_t = ctx_anim.animate_bool_with_time(egui::Id::new(("bd-cat-sel-t", cat_id)), sel, 0.16);
            let hit = c.hit(cx_cat, 26.0, cw, 20.0, ("bd-cat-pill", cat_id));
            let (hov, _) = c.hover_t(&hit);
            if hov > 0.01 && !sel {
                c.fill(cx_cat, 26.0, cw, 20.0, 6.0, Color32::from_white_alpha((16.0 * hov) as u8));
            }
            let txt_col = mix(Color32::from_gray(168), theme::WHITE, sel_t.max(hov * 0.6));
            c.text(
                cx_cat + cw * 0.5,
                39.4 - sel_t * 0.4,
                cat_lbl,
                8.6 + sel_t * 0.4,
                if sel_t > 0.5 { W::Bold } else { W::Semibold },
                txt_col,
                H::Center,
            );
            if hit.clicked() && !sel {
                crate::auralsle_ai_status::config::update(|c_mut| c_mut.clip_filter_tab = cat_id);
            }
            cx_cat += cw + cgap;
        }

        // 右侧紧凑搜索框 (x = 240..380, y = 26..46)
        c.fill(240.0, 26.0, 140.0, 20.0, 6.0, Color32::from_white_alpha(12));
        c.stroke(240.0, 26.0, 140.0, 20.0, 6.0, 0.8, Color32::from_white_alpha(20));
        c.ring(249.5, 35.5, 3.2, 1.0, Color32::from_gray(150));
        c.seg((252.0, 38.0), (254.2, 40.2), 1.0, Color32::from_gray(150));
        if c.live {
            let mut q = cfg.clip_search_query.clone();
            let srect = c.rect(258.0, 28.0, 118.0, 16.0);
            let mut sui = egui::Ui::new(
                c.ui.ctx().clone(),
                egui::Id::new(("bd-clip-search", c.live)),
                egui::UiBuilder::new()
                    .max_rect(srect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center))
                    .layer_id(c.painter.layer_id())
                    .style(c.ui.style().clone()),
            );
            sui.set_clip_rect(srect.expand2(egui::vec2(2.0, 2.0)).intersect(c.painter.clip_rect()));
            sui.set_opacity(c.painter.opacity());
            sui.visuals_mut().text_cursor.blink = true;
            sui.visuals_mut().text_cursor.stroke = egui::Stroke::new(1.6, theme::WHITE);
            let resp = sui.add(
                egui::TextEdit::singleline(&mut q)
                    .id(egui::Id::new("bd-clip-search-in"))
                    .frame(false)
                    .margin(egui::Margin::symmetric(2, 0))
                    .text_color(theme::WHITE)
                    .font(theme::font(9.5, W::Regular))
                    .desired_width(srect.width()),
            );
            if q.is_empty() && !resp.has_focus() {
                c.text(258.0, 39.2, "搜索文字/图片", 9.2, W::Regular, Color32::from_gray(142), H::Left);
            }
            if resp.clicked() {
                self.pending.push(Cmd::Focus);
            }
            if q != cfg.clip_search_query {
                let nq = q.clone();
                crate::auralsle_ai_status::config::update(|c_mut| c_mut.clip_search_query = nq);
            }
        }

        // 3) 过滤并分类条目（识别：图片 / 链接 / 颜色 / 代码 / 文本）
        let classify = |txt: &str| -> u8 {
            let t = txt.trim();
            if crate::clipboard::is_image_clip(t) {
                2 // 图片
            } else if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("www.") {
                3 // 链接
            } else if t.starts_with('#') && (t.len() == 7 || t.len() == 9) && t[1..].chars().all(|ch| ch.is_ascii_hexdigit()) {
                5 // 颜色色值
            } else if t.contains("fn ") || t.contains("let ") || t.contains("const ") || t.contains("import ") || t.contains('{') || t.starts_with("cargo ") || t.starts_with("git ") || t.starts_with("npm ") || t.starts_with("pnpm ") {
                4 // 代码/命令
            } else {
                0 // 文本
            }
        };

        let q_low = cfg.clip_search_query.trim().to_lowercase();
        let filtered: Vec<(usize, &crate::clipboard::Clip, u8)> = items
            .iter()
            .enumerate()
            .filter_map(|(idx, clip)| {
                let kind = classify(&clip.text);
                match filter_tab {
                    1 if !clip.pinned => return None,
                    2 if kind != 2 => return None,
                    3 if kind != 3 => return None,
                    4 if kind != 4 => return None,
                    _ => {}
                }
                if !q_low.is_empty() && !clip.text.to_lowercase().contains(&q_low) {
                    return None;
                }
                Some((idx, clip, kind))
            })
            .collect();

        let box_bottom = ((self.h.pos / c.s) - 30.0 - 18.0).max(182.0);
        let box_h = (box_bottom - 54.0).max(128.0);
        if filtered.is_empty() {
            c.fill(36.0, 54.0, 344.0, box_h, 11.0, Color32::from_white_alpha(9));
            c.stroke(36.0, 54.0, 344.0, box_h, 11.0, 0.8, Color32::from_white_alpha(18));
            let empty_msg = match filter_tab {
                2 => "暂无复制的图片记录 · 可复制任意截图或点击上方「＋ 演示图片」",
                3 => "暂无复制的网页链接",
                4 => "暂无复制的代码或终端命令",
                1 => "暂无星标收藏的剪贴板记录",
                _ => "暂无匹配的剪贴板历史",
            };
            let mid_y = 54.0 + box_h * 0.5;
            c.centered_text(208.0, mid_y - 10.0, empty_msg, 10.5, W::Semibold, Color32::from_gray(175), H::Center);
            c.centered_text(208.0, mid_y + 10.0, "复制文字或图片后自动保存至此，点击任意卡片即可一键再次粘贴", 9.0, W::Regular, Color32::from_gray(130), H::Center);
            return;
        }

        // [需求 11 · React Bits AnimatedList 分类切换流光入场动画]
        let bd_sw_id = egui::Id::new("bd-clip-last-tab");
        let bd_at_id = egui::Id::new("bd-clip-sw-at");
        let last_bd_tab = ctx_anim.data(|d| d.get_temp::<u8>(bd_sw_id)).unwrap_or(filter_tab);
        if last_bd_tab != filter_tab {
            let now_s = self.t;
            ctx_anim.data_mut(|d| {
                d.insert_temp(bd_sw_id, filter_tab);
                d.insert_temp(bd_at_id, now_s);
            });
        }
        let bd_sw_at = ctx_anim.data(|d| d.get_temp::<f64>(bd_at_id)).unwrap_or(0.0);
        let bd_enter_t = if bd_sw_at <= 0.0 {
            1.0_f32
        } else {
            let el = ((self.t - bd_sw_at) as f32 / 0.22).clamp(0.0, 1.0);
            if el < 0.995 {
                ctx_anim.request_repaint();
            }
            1.0 - (1.0 - el).powi(3)
        };

        // 4) BrimDeck 双列 Bento 图文混合卡片流（左右严格对齐上方筛选轨与搜索框 x = 36..380，底边紧贴指示点上方安全区）
        let list_bottom = box_bottom;
        let list_rect = Rect::from_min_max(Pos2::new(36.0, 54.0), Pos2::new(380.0, list_bottom));
        let row_h = 47.0_f32;
        let row_count = (filtered.len() + 1) / 2;
        let mut clicked_action: Option<(usize, u8)> = None; // 0 = 一键粘贴, 1 = 收藏, 2 = 删除
        let clip_viewport_rect = c.rect(list_rect.left(), list_rect.top(), list_rect.width(), list_rect.height());

        let mut content = egui::Ui::new(
            c.ui.ctx().clone(),
            egui::Id::new(("bd-clip-grid", filter_tab, c.live)),
            egui::UiBuilder::new()
                .max_rect(clip_viewport_rect)
                .layer_id(c.painter.layer_id())
                .style(c.ui.style().clone()),
        );
        content.visuals_mut().extreme_bg_color = Color32::TRANSPARENT;
        content.set_clip_rect(clip_viewport_rect.intersect(c.painter.clip_rect()));
        content.set_opacity(c.painter.opacity());
        if !c.live {
            content.disable();
        }
        content.spacing_mut().item_spacing.y = 0.0;
        content.spacing_mut().scroll.floating = false;
        content.spacing_mut().scroll.bar_width = 4.0 * c.s;
        content.spacing_mut().scroll.bar_inner_margin = 4.0 * c.s;
        content.spacing_mut().scroll.bar_outer_margin = 2.0 * c.s;
        content.spacing_mut().scroll.foreground_color = false;

        let scroll_out = egui::ScrollArea::vertical()
            .id_salt(("bd-clip-scroll", filter_tab, c.live))
            .max_height(list_rect.height() * c.s)
            .auto_shrink([false, false])
            .show_rows(&mut content, row_h * c.s, row_count, |ui, range| {
                let vp_r = ui.clip_rect();
                let fade_zone = 20.0 * c.s;
                for r_idx in range {
                    let total_w = list_rect.width() - 14.0;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(total_w * c.s, row_h * c.s), egui::Sense::hover());
                    let top_vis = ((rect.bottom() - vp_r.top()) / fade_zone).clamp(0.0, 1.0);
                    let bot_vis = ((vp_r.bottom() - rect.top()) / fade_zone).clamp(0.0, 1.0);
                    let edge_vis = if rect.top() < vp_r.top() - 1.0 {
                        top_vis * top_vis
                    } else if rect.bottom() > vp_r.bottom() + 1.0 {
                        bot_vis * bot_vis
                    } else {
                        1.0
                    };
                    let stagger = ((r_idx.min(4) as f32) * 0.065).min(0.28);
                    let r_ease = ((bd_enter_t - stagger) / (1.0 - stagger).max(0.25)).clamp(0.0, 1.0);
                    let r_ease = 1.0 - (1.0 - r_ease).powi(3);
                    let y_lift = (1.0 - r_ease) * 7.5 * c.s;
                    let mut rc = Card::new(ui, rect.min + egui::vec2(0.0, y_lift), ui.clip_rect()).faded((0.28 + 0.72 * r_ease) * edge_vis.clamp(0.08, 1.0));
                    rc.s = c.s;
                    rc.live = c.live;

                    let card_w = (total_w - 6.0) * 0.5;
                    let card_h = row_h - 5.0;
                    for col_i in 0..2 {
                        let item_pos = r_idx * 2 + col_i;
                        let Some(&(orig_idx, clip, kind)) = filtered.get(item_pos) else { continue };
                        let cx = col_i as f32 * (card_w + 6.0);
                        let cy = 1.5_f32;

                        let hit = rc.hit(cx, cy, card_w, card_h, ("bd-clip-card", orig_idx));
                        let hov = hit.hovered();
                        let (type_badge, type_col) = match kind {
                            2 => ("图片", Color32::from_rgb(56, 189, 248)),
                            3 => ("链接", Color32::from_rgb(52, 211, 153)),
                            4 => ("代码", Color32::from_rgb(168, 85, 247)),
                            5 => ("颜色", Color32::from_rgb(244, 114, 182)),
                            _ => ("文本", Color32::from_rgb(161, 161, 170)),
                        };

                        let bg = if hov {
                            Color32::from_rgba_unmultiplied(32, 36, 48, 225)
                        } else {
                            Color32::from_rgba_unmultiplied(18, 20, 27, 195)
                        };
                        let border = if hov {
                            type_col.gamma_multiply(0.85)
                        } else {
                            Color32::from_white_alpha(20)
                        };
                        rc.fill(cx, cy, card_w, card_h, 8.5, bg);
                        rc.stroke(cx, cy, card_w, card_h, 8.5, 0.9, border);
                        rc.fill(cx + 2.2, cy + 7.0, 2.2, card_h - 14.0, 1.1, type_col);

                        // 卡片顶栏：类型胶囊 + 时间 + 右上角星标
                        rc.fill(cx + 8.0, cy + 4.5, 24.0, 12.0, 4.0, type_col.gamma_multiply(0.22));
                        rc.text(cx + 20.0, cy + 13.0, type_badge, 7.2, W::Bold, type_col, H::Center);
                        rc.text(cx + 36.0, cy + 13.0, &clip.at, 7.2, W::Regular, Color32::from_gray(142), H::Left);
                        if hov {
                            rc.text(cx + card_w - 22.0, cy + 13.0, if auto_p { "粘贴" } else { "复制" }, 7.2, W::Bold, type_col, H::Right);
                        }

                        let star_hit = rc.hit(cx + card_w - 18.0, cy + 2.0, 16.0, 16.0, ("bd-star", orig_idx));
                        if clip.pinned || hov || star_hit.hovered() {
                            Self::draw_fly_star_icon(
                                &rc,
                                cx + card_w - 10.0,
                                cy + 10.2,
                                4.4,
                                clip.pinned,
                                if clip.pinned { Color32::from_rgb(250, 204, 21) } else { Color32::from_gray(155) },
                            );
                        }

                        // 卡片主体：根据条目类型呈现专属视觉样式
                        if kind == 2 {
                            // 图片卡片：左侧渲染真实缩略图纹理 + 右侧分辨率与 PNG 标签
                            if let Some(img_prev) = crate::clipboard::get_clip_image_preview(&clip.text) {
                                let tex_id_key = egui::Id::new(("bd-clip-img-tex", orig_idx, img_prev.width, img_prev.height));
                                // [修复切页死锁 Bug]：绝不在 `ctx.data_mut` 写锁闭包内部调用 `ctx.load_texture`（否则会触发 parking_lot::RwLock 同线程重入死锁！）
                                let cached_tex = ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(tex_id_key));
                                let tex: egui::TextureHandle = if let Some(t) = cached_tex {
                                    t
                                } else {
                                    let loaded = ui.ctx().load_texture(
                                        format!("bd-clip-thumb-{orig_idx}"),
                                        img_prev.thumb.clone(),
                                        egui::TextureOptions::LINEAR,
                                    );
                                    ui.ctx().data_mut(|d| d.insert_temp(tex_id_key, loaded.clone()));
                                    loaded
                                };
                                let thumb_r = rc.rect(cx + 8.0, cy + 19.0, 46.0, 21.0);
                                let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
                                rc.painter.add(
                                    egui::epaint::RectShape::filled(thumb_r, 4.0 * rc.s, Color32::WHITE)
                                        .with_texture(tex.id(), uv),
                                );
                                rc.stroke(cx + 8.0, cy + 19.0, 46.0, 21.0, 4.0, 0.6, Color32::from_white_alpha(40));
                                rc.text(
                                    cx + 59.0,
                                    cy + 29.0,
                                    &format!("{}×{} PNG", img_prev.width, img_prev.height),
                                    8.4,
                                    W::Bold,
                                    theme::WHITE,
                                    H::Left,
                                );
                                rc.text(cx + 59.0, cy + 38.8, "点击一键粘贴图片", 7.4, W::Regular, Color32::from_gray(165), H::Left);
                            } else {
                                rc.text(cx + 8.0, cy + 33.0, &rc.fit(&clip.text, 8.8, W::Semibold, card_w - 16.0), 8.8, W::Semibold, theme::WHITE, H::Left);
                            }
                        } else if kind == 5 {
                            // 颜色卡片：左侧渲染圆角真实色块 + 右侧 HEX 值
                            let hex = clip.text.trim();
                            let swatch = crate::auralsle_ai_status::plugins::parse_hex_rgba(hex)
                                .map(|[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a))
                                .unwrap_or(type_col);
                            rc.fill(cx + 8.0, cy + 20.0, 32.0, 18.0, 4.5, swatch);
                            rc.stroke(cx + 8.0, cy + 20.0, 32.0, 18.0, 4.5, 0.8, Color32::from_white_alpha(65));
                            rc.text(cx + 46.0, cy + 33.0, &rc.fit(hex, 9.5, W::Bold, card_w - 52.0), 9.5, W::Bold, theme::WHITE, H::Left);
                        } else {
                            let one_line = crate::clipboard::preview(&clip.text);
                            rc.text(
                                cx + 8.0,
                                cy + 33.0,
                                &rc.fit(&one_line, 9.2, W::Semibold, card_w - 14.0),
                                9.2,
                                W::Semibold,
                                if kind == 3 { Color32::from_rgb(125, 211, 252) } else { theme::WHITE },
                                H::Left,
                            );
                        }

                        if star_hit.clicked() {
                            clicked_action = Some((orig_idx, 1));
                        } else if hit.secondary_clicked() {
                            clicked_action = Some((orig_idx, 2));
                        } else if hit.clicked() && clicked_action.is_none() {
                            clicked_action = Some((orig_idx, 0));
                        }
                    }
                }
            });

        let offset_y = scroll_out.state.offset.y;
        let max_scroll_y = (scroll_out.content_size.y - scroll_out.inner_rect.height()).max(0.0);
        Self::paint_clipboard_edge_fades(
            c,
            list_rect,
            list_rect.width() - 10.0,
            self.preview_fill(),
            offset_y,
            max_scroll_y,
        );
        c.seg((38.0, 53.0), (381.0, 53.0), 0.6, Color32::from_white_alpha(24));

        if let Some((index, act)) = clicked_action {
            match act {
                0 => {
                    let text = self.clipboard.lock().items.get(index).map(|c| c.text.clone());
                    if let Some(text) = text {
                        crate::clipboard::paste_clip_to_active_window(c.ui.ctx(), &text, auto_p);
                        self.clip_first = 0;
                        self.apply_clipboard_post_paste_mode(auto_p, false);
                    }
                }
                1 => {
                    if !self.clipboard.lock().toggle_pin(index) {
                        self.say(format!("最多收藏 {} 条", crate::clipboard::MAX_PINNED));
                    }
                }
                _ => {
                    self.clipboard.lock().remove(index);
                }
            }
        }
    }

    fn paint_clipboard_edge_fades(
        c: &Card,
        list_rect: Rect,
        mask_w: f32,
        base_fade: Color32,
        offset_y: f32,
        max_scroll_y: f32,
    ) {
        // 仅在纯色/半实心皮肤上补细腻微渐隐；渐变流体/液态玻璃背景上跳过硬色块遮罩，由卡片边缘透明度平滑过渡，杜绝底部黑灰断层带
        if base_fade.a() < 160 {
            return;
        }
        let top_t = if max_scroll_y > 1.0 {
            (offset_y / (6.0 * c.s.max(0.5))).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bot_t = if max_scroll_y > 1.0 {
            ((max_scroll_y - offset_y) / (6.0 * c.s.max(0.5))).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if top_t > 0.02 {
            let a = (118.0 * top_t).round() as u8;
            let solid = Color32::from_rgba_unmultiplied(base_fade.r(), base_fade.g(), base_fade.b(), a);
            c.vgrad(list_rect.left() - 1.0, list_rect.top(), mask_w, 12.0, 0.0, solid, Color32::TRANSPARENT);
        }
        if bot_t > 0.02 {
            let a = (135.0 * bot_t).round() as u8;
            let solid = Color32::from_rgba_unmultiplied(base_fade.r(), base_fade.g(), base_fade.b(), a);
            c.vgrad(
                list_rect.left() - 1.0,
                list_rect.bottom() - 14.0,
                mask_w,
                14.0,
                0.0,
                Color32::TRANSPARENT,
                solid,
            );
        }
    }

    fn apply_clipboard_post_paste_mode(&mut self, auto_p: bool, compact_msg: bool) {
        let cfg = crate::auralsle_ai_status::config::get();
        if auto_p {
            self.say(if compact_msg { "已复制并自动粘贴" } else { "已一键粘贴到当前窗口" });
        } else {
            self.say("已复制到剪贴板");
        }
        match cfg.clipboard_close_mode {
            0 => {
                // 方案 1：鼠标离开就收起
                self.clip_paste_pin_outside = false;
                self.forced_open_until = -10.0;
                self.clip_timed_close_at = -10.0;
            }
            2 => {
                // 方案 3：粘贴后 N 秒后收起
                let delay = cfg.clipboard_close_delay_secs.clamp(1, 15) as f64;
                self.clip_paste_pin_outside = false;
                self.forced_open_until = self.t + delay;
                self.clip_timed_close_at = self.t + delay;
            }
            _ => {
                // 方案 2：粘贴后点击空白区域后收起
                self.clip_paste_pin_outside = true;
                self.forced_open_until = -10.0;
                self.clip_timed_close_at = -10.0;
            }
        }
    }

    fn draw_fly_clipboard_message_ui(&mut self, c: &Card, items: &[crate::clipboard::Clip]) {
        let c_shifted = c.shifted(0.0, if cfg!(test) { 0.0 } else { 11.0 });
        let c = &c_shifted;
        let cfg = crate::auralsle_ai_status::config::get();
        let total_n = items.len();
        let starred_n = items.iter().filter(|it| it.pinned).count();

        // 1. 头部信息栏 (y = 30..48): 左侧 `📋 剪贴板历史 5 ★ 1`，右侧 `⌨ Ctrl+Shift+V  🗑`
        Self::draw_fly_clip_row_icon(c, 46.0, 39.5, Color32::from_gray(215));
        let w_title = c.text(56.0, 43.5, "剪贴板历史", 11.8, W::Bold, theme::WHITE, H::Left);
        let cnt_x = 56.0 + w_title + 7.0;
        let w_cnt = c.text(cnt_x, 43.2, &format!("{total_n}"), 10.5, W::Semibold, Color32::from_gray(172), H::Left);
        let star_x = cnt_x + w_cnt + 9.0;
        Self::draw_fly_star_icon(c, star_x, 39.2, 4.4, true, Color32::from_gray(178));
        c.text(star_x + 7.0, 43.2, &format!("{starred_n}"), 10.5, W::Semibold, Color32::from_gray(172), H::Left);

        // 右侧快捷键徽章 `⌨ Ctrl+Shift+V` 与清空垃圾桶按钮 `🗑`
        c.stroke(278.0, 33.5, 11.5, 8.5, 1.6, 1.0, Color32::from_gray(150));
        c.circle(281.2, 36.8, 0.7, Color32::from_gray(150));
        c.circle(283.8, 36.8, 0.7, Color32::from_gray(150));
        c.circle(286.4, 36.8, 0.7, Color32::from_gray(150));
        c.seg((280.8, 39.6), (286.8, 39.6), 0.9, Color32::from_gray(150));
        c.text(293.0, 42.8, "Ctrl+Shift+V", 9.8, W::Regular, Color32::from_gray(168), H::Left);

        let clear_hit = c.hit(358.0, 30.0, 20.0, 19.0, "fly-clip-clear-btn");
        let clear_col = if clear_hit.hovered() { theme::RED } else { Color32::from_gray(168) };
        Self::draw_fly_trash_icon(c, 368.0, 39.2, clear_col);
        if clear_hit.clicked() {
            self.clipboard.lock().clear_recent();
            self.clip_first = 0;
            self.say("已清空未收藏的剪贴板记录");
        }

        // 2. 次栏 (y = 52..76): 左侧 `全部` / `收藏` 选项卡 + 右侧 `🔍 搜索内容或备注` 输入框
        let filter_tab = cfg.clip_filter_tab.min(1);
        for (tab_idx, label, tx) in [(0u8, "全部", 40.0_f32), (1u8, "收藏", 74.0_f32)] {
            let active = filter_tab == tab_idx;
            let tw = c.text_w(label, 11.0, if active { W::Bold } else { W::Regular });
            let hit = c.hit(tx - 2.0, 52.0, tw + 8.0, 24.0, ("fly-clip-tab", tab_idx));
            let tcol = if active {
                theme::WHITE
            } else if hit.hovered() {
                Color32::from_gray(215)
            } else {
                Color32::from_gray(145)
            };
            c.text(tx, 67.5, label, 11.0, if active { W::Bold } else { W::Regular }, tcol, H::Left);
            if active {
                c.fill(tx, 73.5, tw, 2.0, 1.0, theme::WHITE);
            }
            if hit.clicked() && filter_tab != tab_idx {
                crate::auralsle_ai_status::config::update(|c| c.clip_filter_tab = tab_idx);
            }
        }

        // 右侧圆角搜索框 `🔍 搜索内容或备注` (x = 112..378, y = 51..75)
        c.fill(112.0, 51.0, 266.0, 24.0, 6.5, Color32::from_white_alpha(12));
        c.stroke(112.0, 51.0, 266.0, 24.0, 6.5, 0.8, Color32::from_white_alpha(20));
        c.ring(123.5, 62.2, 3.8, 1.15, Color32::from_gray(148));
        c.seg((126.4, 65.1), (129.2, 67.9), 1.2, Color32::from_gray(148));

        if c.live {
            let mut q = cfg.clip_search_query.clone();
            let search_rect = c.rect(134.0, 54.0, 236.0, 18.0);
            let mut search_ui = egui::Ui::new(
                c.ui.ctx().clone(),
                egui::Id::new(("fly-clip-search-ui", c.live)),
                egui::UiBuilder::new()
                    .max_rect(search_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center))
                    .layer_id(c.painter.layer_id())
                    .style(c.ui.style().clone()),
            );
            search_ui.set_clip_rect(search_rect.expand2(egui::vec2(2.0, 2.0)).intersect(c.painter.clip_rect()));
            search_ui.set_opacity(c.painter.opacity());
            search_ui.visuals_mut().text_cursor.blink = true;
            search_ui.visuals_mut().text_cursor.stroke = egui::Stroke::new(1.6, theme::WHITE);
            let te = egui::TextEdit::singleline(&mut q)
                .id(egui::Id::new("fly-clip-search-input"))
                .frame(false)
                .margin(egui::Margin::symmetric(2, 0))
                .text_color(theme::WHITE)
                .font(theme::font(10.5, W::Regular))
                .desired_width(search_rect.width());
            let resp = search_ui.add(te);
            if q.is_empty() && !resp.has_focus() {
                // 使用局部坐标直接绘制占位提示，确保与放大镜图标 (123.5) 严格左对齐，绝不向右偏移出界（修复图二）
                c.text(134.0, 66.8, "搜索内容或备注", 10.5, W::Regular, Color32::from_gray(142), H::Left);
            }
            if resp.clicked() {
                self.pending.push(Cmd::Focus);
            }
            if q != cfg.clip_search_query {
                let nq = q.clone();
                crate::auralsle_ai_status::config::update(|c| c.clip_search_query = nq);
            }
        } else {
            let hint_str = if cfg.clip_search_query.is_empty() {
                "搜索内容或备注"
            } else {
                cfg.clip_search_query.as_str()
            };
            c.text(134.0, 66.8, hint_str, 10.5, W::Regular, Color32::from_gray(142), H::Left);
        }

        // 3. 过滤列表数据
        let q_low = cfg.clip_search_query.trim().to_lowercase();
        let note_for = |text: &str| -> String {
            for (k, v) in &cfg.clip_notes {
                if k == text && !v.is_empty() {
                    return v.clone();
                }
            }
            let t = text.trim();
            if t.starts_with("http://") || t.starts_with("https://") {
                "官网".to_owned()
            } else if t.starts_with('#') && (t.len() == 7 || t.len() == 9) {
                "色值".to_owned()
            } else if t.starts_with("pnpm ") || t.starts_with("npm ") || t.starts_with("cargo ") || t.starts_with("git ") {
                "命令".to_owned()
            } else {
                String::new()
            }
        };

        let filtered: Vec<(usize, &crate::clipboard::Clip, String)> = items
            .iter()
            .enumerate()
            .filter(|(_, clip)| {
                if filter_tab == 1 && !clip.pinned {
                    return false;
                }
                if !q_low.is_empty() {
                    let n_str = note_for(&clip.text).to_lowercase();
                    if !clip.text.to_lowercase().contains(&q_low) && !n_str.contains(&q_low) {
                        return false;
                    }
                }
                true
            })
            .map(|(idx, clip)| {
                let n_str = note_for(&clip.text);
                (idx, clip, n_str)
            })
            .collect();

        if filtered.is_empty() {
            c.text(209.5, 134.0, if filter_tab == 1 { "暂无收藏的剪贴板记录" } else { "暂无匹配的剪贴板记录" }, 11.5, W::Semibold, Color32::from_gray(165), H::Center);
            c.text(209.5, 152.0, "复制任意文本后将自动收入此处，点击星标 ★ 可永久收藏", 9.8, W::Regular, Color32::from_gray(125), H::Center);
            return;
        }

        let list_bottom = ((self.h.pos / c.s) - 33.0 - 22.0).max(160.0);
        let list_rect = Rect::from_min_max(Pos2::new(38.0, 80.0), Pos2::new(382.0, list_bottom));
        let row_h = 31.0_f32;
        let mut action: Option<(usize, u8, String)> = None; // 0 复制 1 收藏 2 切换备注 3 删除
        let clip_viewport_rect = c.rect(list_rect.left(), list_rect.top(), list_rect.width(), list_rect.height());

        let mut content = egui::Ui::new(
            c.ui.ctx().clone(),
            egui::Id::new(("fly-clip-v17-list", filter_tab, c.live)),
            egui::UiBuilder::new()
                .max_rect(clip_viewport_rect)
                .layer_id(c.painter.layer_id())
                .style(c.ui.style().clone()),
        );
        content.visuals_mut().extreme_bg_color = Color32::TRANSPARENT;
        content.set_clip_rect(clip_viewport_rect.intersect(c.painter.clip_rect()));
        content.set_opacity(c.painter.opacity());
        if !c.live {
            content.disable();
        }
        content.spacing_mut().item_spacing.y = 0.0;
        content.spacing_mut().scroll.floating = true;
        content.spacing_mut().scroll.bar_width = 4.0 * c.s;
        content.spacing_mut().scroll.floating_width = 4.0 * c.s;
        content.spacing_mut().scroll.bar_outer_margin = 1.0 * c.s;

        let scroll_out = egui::ScrollArea::vertical()
            .id_salt(("fly-clip-v17-scroll", filter_tab, c.live))
            .max_height(list_rect.height() * c.s)
            .auto_shrink([false, false])
            .show_rows(&mut content, row_h * c.s, filtered.len(), |ui, range| {
                let vp_r = ui.clip_rect();
                let fade_zone = 16.0 * c.s;
                for row_i in range {
                    let (orig_idx, clip, ref note_tag) = filtered[row_i];
                    let r_w = list_rect.width() - 12.0;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(r_w * c.s, row_h * c.s), egui::Sense::hover());
                    let top_vis = ((rect.bottom() - vp_r.top()) / fade_zone).clamp(0.0, 1.0);
                    let bot_vis = ((vp_r.bottom() - rect.top()) / fade_zone).clamp(0.0, 1.0);
                    let edge_vis = if rect.top() < vp_r.top() - 1.0 {
                        top_vis * top_vis
                    } else if rect.bottom() > vp_r.bottom() + 1.0 {
                        bot_vis * bot_vis
                    } else {
                        1.0
                    };
                    let mut rc = Card::new(ui, rect.min, ui.clip_rect()).faded(edge_vis.clamp(0.08, 1.0));
                    rc.s = c.s;
                    rc.live = c.live;

                    let hit = rc.hit(0.0, 0.0, r_w, row_h - 1.0, ("fly-clip-r17", orig_idx, clip.at.as_str()));
                    let hovered = hit.hovered();
                    if hovered {
                        rc.fill(0.0, 1.0, r_w, row_h - 2.0, 6.0, Color32::from_white_alpha(12));
                    }

                    // 左侧 `📋` 图标
                    Self::draw_fly_clip_row_icon(&rc, 11.0, row_h * 0.5, Color32::from_gray(165));

                    // 右侧备注 `✎ 官网` 与星标 `★`
                    let star_hit = rc.hit(r_w - 24.0, 4.0, 20.0, row_h - 8.0, ("fly-clip-star", orig_idx));
                    let star_col = if clip.pinned {
                        theme::WHITE
                    } else if star_hit.hovered() {
                        Color32::from_gray(220)
                    } else {
                        Color32::from_gray(115)
                    };
                    if clip.pinned || hovered || star_hit.hovered() {
                        Self::draw_fly_star_icon(&rc, r_w - 14.0, row_h * 0.5, 5.2, clip.pinned, star_col);
                    }

                    let mut right_reserved = 30.0_f32;
                    if !note_tag.is_empty() || hovered {
                        let disp_note = if note_tag.is_empty() { "备注" } else { note_tag.as_str() };
                        let nw = rc.text_w(disp_note, 9.8, W::Regular);
                        let note_x = r_w - 28.0 - nw - 14.0;
                        let note_hit = rc.hit(note_x - 2.0, 4.0, nw + 18.0, row_h - 8.0, ("fly-clip-note", orig_idx));
                        let ncol = if note_hit.hovered() { theme::WHITE } else { Color32::from_gray(155) };
                        paint::pencil(&rc, note_x + 4.5, row_h * 0.5, 8.2, ncol);
                        rc.text(note_x + 11.5, row_h * 0.5 + 3.4, disp_note, 9.8, W::Regular, ncol, H::Left);
                        right_reserved = r_w - note_x + 8.0;
                        if note_hit.clicked() {
                            action = Some((orig_idx, 2, clip.text.clone()));
                        }
                    }

                    // 中间单行剪贴板文本（完全对齐图一）
                    let preview_one_line = crate::clipboard::preview(&clip.text);
                    let fit_txt = rc.fit(&preview_one_line, 11.0, W::Semibold, (r_w - 26.0 - right_reserved).max(80.0));
                    rc.text(24.0, row_h * 0.5 + 3.8, &fit_txt, 11.0, W::Semibold, theme::WHITE, H::Left);

                    // 底部分割细线
                    rc.seg((2.0, row_h - 0.6), (r_w - 2.0, row_h - 0.6), 0.6, Color32::from_white_alpha(14));

                    if star_hit.clicked() {
                        action = Some((orig_idx, 1, String::new()));
                    } else if hit.secondary_clicked() {
                        action = Some((orig_idx, 3, String::new()));
                    } else if hit.clicked() && action.is_none() {
                        action = Some((orig_idx, 0, String::new()));
                    }
                }
            });

        let offset_y = scroll_out.state.offset.y;
        let max_scroll_y = (scroll_out.content_size.y - scroll_out.inner_rect.height()).max(0.0);
        Self::paint_clipboard_edge_fades(
            c,
            list_rect,
            list_rect.width() - 10.0,
            self.preview_fill(),
            offset_y,
            max_scroll_y,
        );

        if let Some((index, kind, key_txt)) = action {
            match kind {
                0 => {
                    let text = self.clipboard.lock().items.get(index).map(|c| c.text.clone());
                    if let Some(text) = text {
                        let auto_p = crate::auralsle_ai_status::config::get().clipboard_auto_paste;
                        crate::clipboard::paste_clip_to_active_window(c.ui.ctx(), &text, auto_p);
                        self.clip_first = 0;
                        self.apply_clipboard_post_paste_mode(auto_p, true);
                    }
                }
                1 => {
                    if !self.clipboard.lock().toggle_pin(index) {
                        self.say(format!("最多收藏 {} 条", crate::clipboard::MAX_PINNED));
                    }
                }
                2 => {
                    let presets = ["官网", "重要", "代码", "命令", "账号", ""];
                    crate::auralsle_ai_status::config::update(|c| {
                        let cur = c.clip_notes.iter().find(|(k, _)| k == &key_txt).map(|(_, v)| v.as_str()).unwrap_or("");
                        let next_idx = presets.iter().position(|&p| p == cur).map(|i| (i + 1) % presets.len()).unwrap_or(0);
                        let next_val = presets[next_idx];
                        if let Some(entry) = c.clip_notes.iter_mut().find(|(k, _)| k == &key_txt) {
                            entry.1 = next_val.to_owned();
                        } else {
                            c.clip_notes.push((key_txt, next_val.to_owned()));
                        }
                    });
                }
                _ => {
                    self.clipboard.lock().remove(index);
                }
            }
        }
    }

    /// 完美移植 flyl1u/fly-island 待办页面 UI（图二 image_fd88c0.png）：
    /// - 顶栏：左侧三段胶囊 `[📋 待办] [📝] [📅]`，右侧蓝色进度条 + `1/3` 完成比例
    /// - 输入框：全宽圆角输入框 `+ 添加 YYYY-MM-DD 的待办` + 右侧 `📅 今天`
    /// - 待办列表：平滑滚动列表（15×15 圆角方形勾选框、已完成绿色对勾+删除线、未完成右侧 `▷ 开始` 专注按钮 + `🗑` 删除图标）
    fn draw_fly_todo_page(&mut self, c: &Card, env: &Env, cmds: &mut Vec<Cmd>) {
        let c_shifted = c.shifted(0.0, if cfg!(test) { 0.0 } else { -16.0 });
        let c = &c_shifted;
        let (done, total) = self.todos.progress(&env.today);
        let cfg = crate::auralsle_ai_status::config::get();
        let sub_tab = cfg.todo_sub_tab.min(2);

        // 1. 顶部分段栏 `[📋 待办] [📝] [📅]` (x = 38..140, y = 31..53，对应岛内 y = 37..59)
        c.fill(38.0, 31.0, 102.0, 22.0, 6.5, Color32::from_white_alpha(12));
        c.stroke(38.0, 31.0, 102.0, 22.0, 6.5, 0.8, Color32::from_white_alpha(18));

        let tab_specs: [(u8, f32, f32); 3] = [
            (0, 40.0, 50.0), // [📋 待办]
            (1, 92.0, 22.0), // [📝]
            (2, 116.0, 22.0), // [📅]
        ];
        for (tid, tx, tw) in tab_specs {
            let active = sub_tab == tid;
            let hit = c.hit(tx, 32.0, tw, 20.0, ("fly-todo-subtab", tid));
            if active {
                c.fill(tx, 32.5, tw, 19.0, 5.0, Color32::from_white_alpha(26));
            } else if hit.hovered() {
                c.fill(tx, 32.5, tw, 19.0, 5.0, Color32::from_white_alpha(10));
            }
            let col = if active { theme::WHITE } else { Color32::from_gray(172) };
            match tid {
                0 => {
                    Self::draw_fly_clip_row_icon(c, tx + 10.5, 42.0, col);
                    c.text(tx + 19.0, 45.8, "待办", 10.6, W::Bold, col, H::Left);
                }
                1 => {
                    c.stroke(tx + 6.5, 36.8, 8.5, 10.2, 1.5, 1.1, col);
                    paint::pencil(c, tx + 12.2, 41.5, 7.5, col);
                }
                _ => {
                    c.stroke(tx + 6.2, 37.2, 9.6, 9.4, 1.6, 1.1, col);
                    c.seg((tx + 6.2, 40.2), (tx + 15.8, 40.2), 1.0, col);
                    c.seg((tx + 8.8, 35.8), (tx + 8.8, 37.8), 1.1, col);
                    c.seg((tx + 13.2, 35.8), (tx + 13.2, 37.8), 1.1, col);
                }
            }
            if hit.clicked() && sub_tab != tid {
                crate::auralsle_ai_status::config::update(|c| c.todo_sub_tab = tid);
            }
        }

        // 右上角进度条与 `done/total` (x = 282..378, y = 39..45)
        let ratio = if total > 0 { (done as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
        c.fill(282.0, 40.2, 62.0, 4.6, 2.3, Color32::from_white_alpha(24));
        if ratio > 0.01 {
            c.fill(282.0, 40.2, (62.0 * ratio).max(4.6), 4.6, 2.3, Color32::from_rgb(56, 189, 248));
        }
        c.text(377.0, 45.8, &format!("{done}/{total}"), 10.6, W::Semibold, Color32::from_gray(215), H::Right);

        // 2. 全宽添加待办输入框 `+ 添加 YYYY-MM-DD 的待办   📅 今天` (x = 38..378, y = 58..85)
        c.fill(38.0, 58.0, 340.0, 27.0, 6.5, Color32::from_white_alpha(10));
        c.stroke(38.0, 58.0, 340.0, 27.0, 6.5, 0.9, Color32::from_white_alpha(22));
        c.seg((46.5, 71.5), (54.5, 71.5), 1.3, Color32::from_gray(165));
        c.seg((50.5, 67.5), (50.5, 75.5), 1.3, Color32::from_gray(165));
        let plus_hit = if c.live {
            Some(c.hit(38.0, 58.0, 20.0, 27.0, "fly-todo-plus-btn"))
        } else {
            None
        };

        // 右侧 `📅 今天` 标签按钮
        c.stroke(334.0, 66.8, 9.2, 9.0, 1.5, 1.05, Color32::from_gray(172));
        c.seg((334.0, 69.6), (343.2, 69.6), 0.9, Color32::from_gray(172));
        c.text(347.0, 75.2, "今天", 10.2, W::Semibold, Color32::from_gray(192), H::Left);
        let today_btn = c.hit(330.0, 60.0, 46.0, 23.0, "fly-todo-today-btn");
        if today_btn.clicked() && !self.todos.items.is_empty() {
            self.open_task_reminder(0);
        }

        if c.live {
            let has_text_before = !self.buf.trim().is_empty();
            let input_w = if has_text_before { 222.0 } else { 264.0 };
            let input_rect = c.rect(60.0, 62.5, input_w, 18.0);
            let mut input_ui = egui::Ui::new(
                c.ui.ctx().clone(),
                egui::Id::new(("fly-todo-input-ui", c.live)),
                egui::UiBuilder::new()
                    .max_rect(input_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center))
                    .layer_id(c.painter.layer_id())
                    .style(c.ui.style().clone()),
            );
            input_ui.set_clip_rect(input_rect.expand2(egui::vec2(2.0, 2.0)).intersect(c.painter.clip_rect()));
            input_ui.set_opacity(c.painter.opacity());
            input_ui.visuals_mut().text_cursor.blink = true;
            input_ui.visuals_mut().text_cursor.stroke = egui::Stroke::new(1.6, theme::WHITE);
            let mut buf = std::mem::take(&mut self.buf);
            let te = egui::TextEdit::singleline(&mut buf)
                .id(egui::Id::new("fly-todo-inline-input"))
                .frame(false)
                .margin(egui::Margin::symmetric(2, 0))
                .text_color(theme::WHITE)
                .font(theme::font(11.0, W::Regular))
                .desired_width(input_rect.width());
            let resp = input_ui.add(te);
            let is_empty = buf.is_empty();
            let has_text = !buf.trim().is_empty();
            self.buf = buf;

            let mut add_btn_clicked = false;
            if has_text {
                let add_hit = c.hit(286.0, 61.5, 40.0, 20.0, "fly-todo-add-btn");
                let fill_col = if add_hit.hovered() {
                    Color32::from_rgb(36, 148, 242)
                } else {
                    Color32::from_rgb(14, 118, 224)
                };
                c.fill(286.0, 61.5, 40.0, 20.0, 5.0, fill_col);
                c.centered_text(306.0, 71.5, "添加", 10.0, W::Bold, theme::WHITE, H::Center);
                if add_hit.clicked() {
                    add_btn_clicked = true;
                }
            }

            if is_empty && !resp.has_focus() {
                // 使用局部坐标直接绘制占位提示，确保紧挨左侧 `+` 号 (50.5) 右侧，绝不向右偏出输入框（修复图三）
                c.text(60.0, 75.2, &format!("添加 {} 的待办", env.today), 11.0, W::Regular, Color32::from_gray(145), H::Left);
            }
            // 不再把 `self.adding = true` 绑死到内联输入框，防止输入文字后 `is_editing()` 锁定岛体导致鼠标移出无法收起
            if resp.clicked() || resp.gained_focus() {
                cmds.push(Cmd::Focus);
            }
            let plus_clicked = plus_hit.as_ref().is_some_and(|h| h.clicked());
            if plus_clicked && !has_text {
                resp.request_focus();
                cmds.push(Cmd::Focus);
            }
            let enter_submit = (resp.lost_focus() || resp.has_focus())
                && !self.todo_ime_frame
                && c.ui.input(|i| i.key_pressed(egui::Key::Enter));
            let esc_pressed = (resp.lost_focus() || resp.has_focus())
                && !self.todo_ime_frame
                && c.ui.input(|i| i.key_pressed(egui::Key::Escape));
            let lost_window = c.ui.input(|i| {
                i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(false)))
            });

            if enter_submit || add_btn_clicked || (plus_clicked && has_text) {
                let text = self.buf.trim().to_string();
                if !text.is_empty() {
                    self.todo_first = 0;
                    self.todos.items.insert(0, crate::model::TodoItem::new(text));
                    self.todos.save();
                    self.todo_pop.vel += 6.0;
                    self.buf.clear();
                }
                resp.surrender_focus();
                self.adding = false;
                self.todo_input_focused = false;
                self.todo_ime_active = false;
            } else if esc_pressed || (!self.todo_ime_frame && (resp.clicked_elsewhere() || lost_window)) || !self.open {
                resp.surrender_focus();
                self.adding = false;
                self.todo_input_focused = false;
                self.todo_ime_active = false;
            }
        } else {
            c.text(60.0, 75.2, &format!("添加 {} 的待办", env.today), 11.0, W::Regular, Color32::from_gray(145), H::Left);
        }

        // 3. 待办滚动列表 (x = 38..378, y = 89..202)
        let order: Vec<usize> = self
            .todos
            .display_order(&env.today)
            .into_iter()
            .filter(|&i| {
                let item = &self.todos.items[i];
                match sub_tab {
                    1 => !item.done_for(&env.today),
                    2 => item.is_repeating() || item.due.is_some(),
                    _ => true,
                }
            })
            .collect();

        if order.is_empty() {
            c.text(208.0, 138.0, "今天还没有待办安排", 11.5, W::Semibold, Color32::from_gray(165), H::Center);
            c.text(208.0, 156.0, "在上方输入框输入待办事项并按回车即可快速添加", 9.8, W::Regular, Color32::from_gray(125), H::Center);
            return;
        }

        let todo_bottom = ((self.h.pos / c.s) - 6.0 - 28.0).clamp(150.0, 244.0);
        let list_rect = Rect::from_min_max(Pos2::new(38.0, 89.0), Pos2::new(378.0, todo_bottom));
        let row_h = 34.0_f32;
        let mut todo_action: Option<(usize, u8)> = None; // 0 切换完成 1 开始专注 2 删除 3 编辑提醒

        let mut content = egui::Ui::new(
            c.ui.ctx().clone(),
            egui::Id::new(("fly-todo-v17-list", sub_tab, c.live)),
            egui::UiBuilder::new()
                .max_rect(c.rect(list_rect.left(), list_rect.top(), list_rect.width(), list_rect.height()))
                .layer_id(c.painter.layer_id())
                .style(c.ui.style().clone()),
        );
        content.visuals_mut().extreme_bg_color = Color32::TRANSPARENT;
        content.set_clip_rect(c.rect(list_rect.left(), list_rect.top(), list_rect.width(), list_rect.height()).intersect(c.painter.clip_rect()));
        content.set_opacity(c.painter.opacity());
        if !c.live {
            content.disable();
        }
        content.spacing_mut().item_spacing.y = 0.0;
        content.spacing_mut().scroll.floating = true;
        content.spacing_mut().scroll.bar_width = 3.0 * c.s;

        egui::ScrollArea::vertical()
            .id_salt(("fly-todo-v17-scroll", sub_tab, c.live))
            .max_height(list_rect.height() * c.s)
            .auto_shrink([false, false])
            .show_rows(&mut content, row_h * c.s, order.len(), |ui, range| {
                for r_idx in range {
                    let i = order[r_idx];
                    let item = &self.todos.items[i];
                    let checked = item.done_for(&env.today);
                    let r_w = list_rect.width() - 4.0;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(r_w * c.s, row_h * c.s), egui::Sense::hover());
                    let mut rc = Card::new(ui, rect.min, ui.clip_rect());
                    rc.s = c.s;
                    rc.live = c.live;

                    let row_hit = rc.hit(0.0, 0.0, r_w, row_h - 1.0, ("fly-todo-r17", i, item.text.as_str()));
                    if row_hit.hovered() {
                        rc.fill(0.0, 1.0, r_w, row_h - 2.0, 6.0, Color32::from_white_alpha(10));
                    }

                    // 左侧 15×15 圆角方形复选框（图二：已完成为翠绿底+白勾，未完成为圆角灰框）
                    let box_x = 4.0_f32;
                    let box_y = (row_h - 15.0) * 0.5;
                    if checked {
                        rc.fill(box_x, box_y, 15.0, 15.0, 4.0, Color32::from_rgb(34, 197, 94));
                        paint::check(&rc, box_x + 7.5, box_y + 7.5, 6.2, 1.0, 1.5, Color32::WHITE);
                    } else {
                        rc.stroke(box_x, box_y, 15.0, 15.0, 4.0, 1.35, Color32::from_gray(135));
                    }

                    // 右侧 `▷ 开始` 按钮（未完成项显示）与 `🗑` 删除按钮
                    let del_hit = rc.hit(r_w - 22.0, 5.0, 20.0, row_h - 10.0, ("fly-todo-del17", i));
                    let del_col = if del_hit.hovered() { theme::RED } else { Color32::from_gray(148) };
                    Self::draw_fly_trash_icon(&rc, r_w - 12.0, row_h * 0.5, del_col);

                    let mut right_pad = 28.0_f32;
                    if !checked {
                        let start_hit = rc.hit(r_w - 68.0, 5.0, 42.0, row_h - 10.0, ("fly-todo-start17", i));
                        let scol = if start_hit.hovered() { theme::WHITE } else { Color32::from_gray(205) };
                        let tri_cx = r_w - 60.0;
                        let tri_cy = row_h * 0.5;
                        rc.path(&[(tri_cx - 2.8, tri_cy - 3.6), (tri_cx + 3.2, tri_cy), (tri_cx - 2.8, tri_cy + 3.6), (tri_cx - 2.8, tri_cy - 3.6)], 1.15, scol);
                        rc.text(r_w - 52.0, row_h * 0.5 + 3.6, "开始", 10.4, W::Semibold, scol, H::Left);
                        right_pad = 74.0;
                        if start_hit.clicked() {
                            todo_action = Some((i, 1));
                        }
                    }

                    // 中间待办标题 + 完成态删除线
                    let txt_col = if checked { Color32::from_gray(128) } else { theme::WHITE };
                    let fit_t = rc.fit(&item.text, 11.4, W::Semibold, (r_w - 28.0 - right_pad).max(80.0));
                    let tw = rc.text(26.0, row_h * 0.5 + 4.0, &fit_t, 11.4, W::Semibold, txt_col, H::Left);
                    if checked {
                        rc.fill(26.0, row_h * 0.5 - 0.5, tw, 1.2, 0.6, Color32::from_gray(128));
                    }

                    // 底部分割细线
                    rc.seg((2.0, row_h - 0.6), (r_w - 2.0, row_h - 0.6), 0.6, Color32::from_white_alpha(14));

                    if del_hit.clicked() {
                        todo_action = Some((i, 2));
                    } else if row_hit.secondary_clicked() {
                        todo_action = Some((i, 3));
                    } else if row_hit.clicked() && todo_action.is_none() {
                        todo_action = Some((i, 0));
                    }
                }
            });

        if let Some((i, act)) = todo_action {
            if i < self.todos.items.len() {
                match act {
                    0 => {
                        let now_done = self.todos.items[i].done_for(&env.today);
                        self.todos.items[i].toggle(&env.today);
                        self.todos.save();
                        if !now_done {
                            self.todo_pop.vel += 5.0;
                            self.pet.react(crate::extras::PetState::Happy);
                        }
                    }
                    1 => {
                        self.pomo.phase = crate::app::Phase::Focus;
                        self.pomo.remaining = self.phase_secs(crate::app::Phase::Focus);
                        self.pomo.running = true;
                        self.show_page(1);
                    }
                    2 => {
                        self.todos.items.remove(i);
                        self.todos.save();
                    }
                    _ => {
                        self.open_task_dialog(i);
                    }
                }
            }
        }
    }
}


pub(crate) fn is_clone_notification_record(r: &crate::model::NotificationRecord) -> bool {
    r.app_name.contains("分身")
        || r.app_id.contains("#u")
        || r.app_id.contains("#clone")
}

pub(crate) fn notification_record_slot_tag(r: &crate::model::NotificationRecord) -> Option<(&'static str, bool)> {
    if is_clone_notification_record(r) {
        Some(("分身", true))
    } else if r.app_id.starts_with("oppo-mirror:") {
        Some(("主应用", false))
    } else {
        None
    }
}

/// [修复需求 15 图8 `image_39d1c6.jpg`]：严格判定是否为真实腾讯 QQ / TIM 聊天消息，
/// 彻底剔除 QQ音乐 (`qqmusic`)、QQ浏览器 (`qqbrowser`)、QQ输入法 (`qqpinyin`)、QQ影音、腾讯视频、QQ游戏与播放器歌单加载状态通知！
pub(crate) fn is_strict_qq_chat_record(r: &crate::model::NotificationRecord) -> bool {
    let app_l = r.app_name.trim().to_ascii_lowercase();
    let id_l = r.app_id.trim().to_ascii_lowercase();
    let title_l = r.title.trim().to_ascii_lowercase();
    let body_l = r.body.trim().to_ascii_lowercase();
    for bad in [
        "qq音乐",
        "qqmusic",
        "qq 音乐",
        "qq浏览器",
        "qqbrowser",
        "qq输入法",
        "qqpinyin",
        "qq影音",
        "qqplayer",
        "qq游戏",
        "qqgame",
        "腾讯视频",
        "qqlive",
        "qq邮箱",
        "qqmail",
        "qq旋风",
        "qq安全",
        "qq电脑管家",
        "正在加载列表",
        "点击选择歌曲播放",
    ] {
        if app_l.contains(bad) || id_l.contains(bad) || title_l.contains(bad) || body_l.contains(bad) {
            return false;
        }
    }
    app_l == "qq"
        || app_l == "qq分身"
        || app_l.starts_with("qq ")
        || app_l == "tim"
        || app_l == "腾讯qq"
        || app_l == "手机qq"
        || id_l == "qq"
        || id_l.ends_with(".qq")
        || id_l.contains("tencent.qq")
        || id_l.contains("tencent.mobileqq")
        || id_l.contains("tencent.tim")
        || title_l.starts_with("qq ·")
}

impl crate::app::App {
    pub(crate) fn glance_lyric_triplet(&self) -> (String, String, String, f32) {
        if let Some(lyrics) = &self.ly.lyrics {
            if !lyrics.lines.is_empty() {
                let pos = self.lyric_position();
                let idx = if lyrics.synced {
                    lyrics.index_at(pos).unwrap_or(0)
                } else {
                    ((pos.max(0.0) / 4.0) as usize) % lyrics.lines.len()
                };
                let prev_s = if idx > 0 {
                    lyrics.lines.get(idx - 1).map(|l| l.text.trim().to_owned()).unwrap_or_default()
                } else {
                    String::new()
                };
                let cur_s = lyrics
                    .lines
                    .get(idx)
                    .map(|l| l.text.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| self.current_music_lyric_text().0);
                let next_s = lyrics.lines.get(idx + 1).map(|l| l.text.trim().to_owned()).unwrap_or_default();
                let cur_t0 = lyrics.lines.get(idx).map(|l| l.t).unwrap_or(0.0);
                let cur_t1 = lyrics.lines.get(idx + 1).map(|l| l.t).unwrap_or((cur_t0 + 3.5).max(self.media.duration as f32));
                let p_word = if lyrics.synced {
                    ((pos - cur_t0) / (cur_t1 - cur_t0).max(0.4)).clamp(0.0, 1.0)
                } else {
                    0.85
                };
                return (prev_s, cur_s, next_s, p_word);
            }
        }
        (String::new(), self.current_music_lyric_text().0, String::new(), 1.0)
    }
}

fn draw_karaoke_lyric(c: &Card, lx: f32, ly: f32, max_w: f32, text: &str, fsz: f32, prog: f32) {
    // [eIsland · rolling wave] 整行随节奏轻微浮动，已唱部分右端加一道呼吸光刃 + 波峰碎片，
    // 读起来是"浪头推着字点亮"，而不是一条硬边色块划过。
    let phase = if cfg!(test) { 0.0 } else { c.ui.ctx().input(|i| i.time) as f32 * 2.4 };
    let bob = if cfg!(test) { 0.0 } else { phase.sin() * 0.8 };
    let ly = ly + bob;
    let fitted = c.fit(text, fsz, W::Bold, max_w);
    let total_tw = c.text_w(&fitted, fsz, W::Bold);
    let start_x = lx - total_tw * 0.5;
    c.text(start_x, ly, &fitted, fsz, W::Bold, Color32::from_white_alpha(115), H::Left);
    let lit_w = (total_tw * prog.clamp(0.18, 1.0)).max(2.0);
    let clip_c = c.clipped(start_x - 1.0, ly - fsz - 4.0, lit_w + 2.0, fsz + 10.0);
    clip_c.text(start_x, ly, &fitted, fsz, W::Bold, theme::WHITE, H::Left);
    karaoke_wave_crest(c, start_x + lit_w, ly, fsz, prog, phase, lit_w < total_tw);
}

/// [eIsland · rolling wave] 已唱与未唱交界处的浪头：呼吸光刃 + 两条随相位平移的波峰光点。
fn karaoke_wave_crest(c: &Card, edge: f32, ly: f32, fsz: f32, prog: f32, phase: f32, mid_line: bool) {
    if cfg!(test) || !mid_line || prog >= 0.995 {
        return;
    }
    let pulse = 0.42 + 0.34 * (phase * 1.7).sin().abs();
    let topy = ly - fsz + 1.0;
    let h = fsz * 1.05;
    c.fill(edge - 1.2, topy, 2.4, h, 1.2, Color32::from_white_alpha((150.0 * pulse) as u8));
    c.fill(edge - 4.2, topy + h * 0.12, 2.6, h * 0.76, 1.3, Color32::from_white_alpha((72.0 * pulse) as u8));
    for k in 0..3 {
        let t = phase + k as f32 * 2.1;
        let dy = t.sin() * h * 0.30;
        let a = (0.52 - k as f32 * 0.13).max(0.0);
        c.circle(edge - 2.0 - k as f32 * 3.4, ly - fsz * 0.55 + dy, 1.15, Color32::from_white_alpha((255.0 * a) as u8));
    }
}
