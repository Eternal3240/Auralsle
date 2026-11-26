//! 主岛与关联副卡片的实时桌面折射液态玻璃管线（支持 Wgpu 与 OpenGL 双后端）。

mod backdrop;
mod controls;
mod glow_pipeline;
mod pipeline;

pub use backdrop::{Backdrop, CleanPlate, Geometry, ScreenRect};
#[allow(unused_imports)]
pub(crate) use controls::{control, control_scale, ControlRole};

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Pos2, Rect};

use crate::{app::FrameOut, model::Dock};
use controls::{Controls, Registration};
use glow_pipeline::GlowPipeline;
use pipeline::{Frame, Pipeline};

pub const CAYWW_LIQUID_GLASS_SHADER: &str = include_str!("cayww_liquid_glass.wgsl");

fn log(message: &str) {
    crate::resources::append_log(
        &crate::model::config_dir().join("diagnostics"),
        "material",
        message,
    );
}

#[derive(Clone)]
pub struct Options {
    pub controls: bool,
    pub stiffness: f32,
    pub damping: f32,
    pub params: [f32; 32],
    pub shadow: Option<f32>,
    pub glass_foreground: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            controls: true,
            stiffness: 1.0,
            damping: 0.5,
            params: [0.0; 32],
            shadow: None,
            glass_foreground: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Appearance {
    pub shadow: Option<f32>,
    pub glass_foreground: bool,
    pub light: bool,
    pub controls: bool,
    pub ready: bool,
}

pub(crate) struct CardLayer {
    painter: egui::Painter,
    slot: egui::layers::ShapeIdx,
    rect: Rect,
    radius: f32,
    fade: f32,
}

pub(crate) struct Layer {
    main: Option<(egui::Painter, egui::layers::ShapeIdx)>,
    canvas: Rect,
    body: Rect,
    capture: Rect,
    radius: f32,
    expansion: f32,
    notch: Option<(f32, f32)>,
    dock: Dock,
    artwork: Option<(egui::TextureId, Arc<egui::ColorImage>)>,
    extra_cards: Vec<CardLayer>,
}

impl Layer {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        canvas: Rect,
        body: Rect,
        radius: f32,
        expansion: f32,
        notch: Option<(f32, f32)>,
        dock: Dock,
        artwork: Option<&(Arc<egui::ColorImage>, egui::TextureHandle)>,
    ) -> Self {
        let padding = if dock.is_side() {
            egui::vec2(48.0, 64.0)
        } else {
            egui::vec2(64.0, 48.0)
        };
        Self {
            main: None,
            canvas,
            body,
            capture: body.expand2(padding).intersect(canvas),
            radius,
            expansion,
            notch,
            dock,
            artwork: artwork.map(|(image, texture)| (texture.id(), Arc::clone(image))),
            extra_cards: Vec::new(),
        }
    }

    pub(crate) fn reserve_main(&mut self, painter: &egui::Painter, fallback: egui::Shape) {
        self.main = Some((painter.clone(), painter.add(fallback)));
    }

    #[allow(dead_code, clippy::too_many_arguments)]
    pub(crate) fn reserve(
        painter: &egui::Painter,
        canvas: Rect,
        body: Rect,
        radius: f32,
        expansion: f32,
        notch: Option<(f32, f32)>,
        dock: Dock,
        artwork: Option<&(Arc<egui::ColorImage>, egui::TextureHandle)>,
        fallback: egui::Shape,
    ) -> Self {
        let mut layer = Self::new(canvas, body, radius, expansion, notch, dock, artwork);
        layer.reserve_main(painter, fallback);
        layer
    }

    /// 预留副岛或悬浮胶囊的共享液态玻璃渲染槽位（与主岛共享同张 CleanPlate 与光标光照）。
    pub(crate) fn reserve_card(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        radius: f32,
        fade: f32,
        fallback: egui::Shape,
    ) {
        let slot = painter.add(fallback);
        self.extra_cards.push(CardLayer {
            painter: painter.clone(),
            slot,
            rect,
            radius,
            fade,
        });
    }
}

#[derive(Default)]
struct Cadence {
    last_activity: Option<Instant>,
    last_sample: Option<Instant>,
}

impl Cadence {
    fn interval(&mut self, now: Instant, active: bool) -> Duration {
        let last = self.last_activity.get_or_insert(now);
        if active {
            *last = now;
            Duration::from_secs_f64(1.0 / 60.0)
        } else if now.duration_since(*last) >= Duration::from_secs(120) {
            Duration::from_secs(1)
        } else {
            Duration::from_secs_f64(1.0 / 15.0)
        }
    }

    fn remaining(&self, now: Instant, interval: Duration) -> Duration {
        self.last_sample.map_or(Duration::ZERO, |at| {
            interval.saturating_sub(now.duration_since(at))
        })
    }
}

#[derive(Default)]
struct Budget(u32);

impl Budget {
    fn exceeded(&mut self, elapsed: Duration) -> bool {
        if elapsed > Duration::from_millis(4) {
            self.0 += 1;
        } else {
            self.0 = 0;
        }
        self.0 >= 120
    }
}

struct Motion {
    expansion: Option<f32>,
    light: f32,
    morph: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            expansion: None,
            light: 1.0,
            morph: 0.0,
        }
    }
}

impl Motion {
    fn sample_expansion(&mut self, expansion: f32, dt: f32) -> (f32, f32) {
        let speed = self
            .expansion
            .replace(expansion)
            .map_or(0.0, |previous| (expansion - previous) / dt.max(0.001));
        self.morph = (self.morph * 0.74).max((speed.abs() * 0.30).min(1.0));
        (speed, self.morph)
    }

    fn update_light(&mut self, brightness: f32, dt: f32) -> bool {
        let target = if brightness > 0.66 {
            1.0
        } else if brightness < 0.52 {
            0.0
        } else {
            self.light
        };
        self.light += (target - self.light) * (dt.clamp(0.0, 0.1) * 6.0);
        (target - self.light).abs() > 0.001
    }
}

enum Backend {
    Wgpu(Pipeline),
    Glow(GlowPipeline),
}

impl Backend {
    fn clear_backdrop(&mut self) {
        match self {
            Self::Wgpu(p) => p.clear_backdrop(),
            Self::Glow(g) => g.clear_backdrop(),
        }
    }

    fn callback(
        &mut self,
        card_index: usize,
        rect: Rect,
        frame: Frame,
        plate: &CleanPlate,
        dock: Dock,
    ) -> Result<egui::Shape, String> {
        match self {
            Self::Wgpu(p) => p.callback(card_index, rect, frame, plate, dock),
            Self::Glow(g) => g.callback(rect, frame, plate, dock),
        }
    }

    fn destroy_gl(&mut self, gl: &eframe::glow::Context) {
        if let Self::Glow(g) = self {
            g.destroy(gl);
        }
    }
}

pub struct Runtime {
    backend: Backend,
    options: Options,
    backdrop: Option<Backdrop>,
    controls: Controls,
    registrations: Vec<Registration>,
    cadence: Cadence,
    budget: Budget,
    last_frame: Option<Instant>,
    pointer: Option<Pos2>,
    screen: Option<ScreenRect>,
    tint: Option<(egui::TextureId, [f32; 3])>,
    motion: Motion,
    ready: bool,
    capture_ready: bool,
    has_drawn: bool,
}

impl Runtime {
    pub fn new_liquid_glass(
        wgpu_rs: Option<&eframe::egui_wgpu::RenderState>,
        gl: Option<&Arc<eframe::glow::Context>>,
        options: Options,
        msaa_samples: u32,
    ) -> Result<Self, String> {
        let backend = if let Some(rs) = wgpu_rs {
            Backend::Wgpu(Pipeline::new(rs, CAYWW_LIQUID_GLASS_SHADER, msaa_samples)?)
        } else if let Some(gl) = gl {
            Backend::Glow(GlowPipeline::new(Arc::clone(gl))?)
        } else {
            return Err("未找到可用的图形渲染后端（Wgpu / OpenGL）".into());
        };
        Ok(Self {
            backend,
            options,
            backdrop: None,
            controls: Controls::default(),
            registrations: Vec::new(),
            cadence: Cadence::default(),
            budget: Budget::default(),
            last_frame: None,
            pointer: None,
            screen: None,
            tint: None,
            motion: Motion::default(),
            ready: false,
            capture_ready: false,
            has_drawn: false,
        })
    }

    pub fn set_options(&mut self, options: Options) {
        self.options = options;
    }

    pub fn destroy_gl(&mut self, gl: &eframe::glow::Context) {
        self.backend.destroy_gl(gl);
    }

    pub fn appearance(&self) -> Appearance {
        Appearance {
            shadow: self.options.shadow,
            glass_foreground: self.options.glass_foreground,
            light: self.motion.light > 0.5,
            controls: self.options.controls,
            ready: self.ready,
        }
    }

    fn clear_backdrop(&mut self) {
        self.backdrop = None;
        self.backend.clear_backdrop();
        self.screen = None;
        self.ready = false;
    }

    pub fn draw(&mut self, draw_island: impl FnOnce() -> FrameOut) -> FrameOut {
        if self.options.controls {
            let (out, registrations) = controls::collect(draw_island);
            self.registrations = registrations;
            out
        } else {
            draw_island()
        }
    }

    /// 隐藏或锁屏期间释放桌面捕获，并在恢复后重新定位底片。
    pub fn suspend(&mut self) {
        if self.backdrop.is_some() {
            self.clear_backdrop();
            self.controls = Controls::default();
            self.registrations.clear();
            self.cadence = Cadence::default();
            self.budget = Budget::default();
            self.last_frame = None;
            self.motion.expansion = None;
            self.motion.morph = 0.0;
            self.pointer = None;
        }
    }

    pub fn finish(
        &mut self,
        ctx: &egui::Context,
        window: &mut crate::native::Window,
        out: &mut FrameOut,
        time: f64,
        screen_pointer: Option<(i32, i32)>,
    ) -> Result<Option<Duration>, String> {
        let Some(mut layer) = out.material_layer.take() else {
            return Ok(None);
        };
        if ctx.will_discard() {
            return Ok(None);
        }
        let hwnd = window.hwnd();
        if !unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd) }.as_bool() {
            return Ok(Some(Duration::from_millis(20)));
        }
        let started = Instant::now();
        if window.ensure_material_capture_exclusion()? {
            self.clear_backdrop();
        }
        self.capture_ready = true;
        let ppp = ctx.pixels_per_point();

        let mut union_capture = layer.capture;
        for card in &layer.extra_cards {
            let expanded = card.rect.expand2(egui::vec2(48.0, 48.0)).intersect(layer.canvas);
            union_capture = union_capture.union(expanded);
        }

        let Some((geometry, viewport)) =
            Geometry::for_window(hwnd, layer.body, union_capture, ppp)?
        else {
            self.ready = false;
            return Ok(Some(self.cadence.interval(started, out.animating)));
        };
        let [bx, by, bw, bh] = geometry.body;
        let body_pixels = Rect::from_min_size(egui::pos2(bx, by), egui::vec2(bw, bh));
        let pointer = screen_pointer.map(|(x, y)| {
            egui::pos2(
                (x - geometry.screen.x) as f32,
                (y - geometry.screen.y) as f32,
            )
        });
        let pointer_in_island = pointer.filter(|&p| {
            let margin = 12.0 * ppp;
            body_pixels.expand(margin).contains(p)
                || layer.extra_cards.iter().any(|card| {
                    let cap_origin = (viewport.min.to_vec2() * ppp).round().to_pos2();
                    let min = card.rect.min.to_vec2() * ppp - cap_origin.to_vec2();
                    let r = Rect::from_min_size(min.to_pos2(), card.rect.size() * ppp);
                    r.expand(margin).contains(p)
                })
        });
        let moved = self.pointer != pointer_in_island;
        self.pointer = pointer_in_island;
        let dt = self
            .last_frame
            .replace(started)
            .map_or(1.0 / 60.0, |at| (started - at).as_secs_f32());
        let (speed, morph) = self.motion.sample_expansion(layer.expansion, dt);
        let moving = out.animating
            || morph > 0.01
            || moved
            || pointer_in_island.is_some()
                && ctx.input(|i| i.pointer.any_down() || i.pointer.any_released());
        let capture_origin = (viewport.min.to_vec2() * ppp).round().to_pos2();
        let mut registrations = std::mem::take(&mut self.registrations);
        if self.options.glass_foreground && layer.expansion < 0.985 {
            registrations.clear();
        }
        let controls = self.controls.frame(
            ctx,
            registrations,
            ppp,
            capture_origin,
            dt,
            self.options.stiffness,
            self.options.damping,
        );
        let interval = self.cadence.interval(started, moving || controls.animating);
        let backdrop = self.backdrop.get_or_insert_with(Backdrop::default);
        let plate = if self.screen != Some(geometry.screen)
            || self.cadence.remaining(started, interval).is_zero()
        {
            self.cadence.last_sample = Some(started);
            self.screen = Some(geometry.screen);
            backdrop.capture(geometry).map_err(str::to_owned)?
        } else {
            backdrop.current(geometry)
        };

        let mut light_changing = false;
        let mut extra_shapes = Vec::with_capacity(layer.extra_cards.len());
        let shape = if let Some(plate) = plate {
            let mut frame = Frame::default();
            let [x, y, w, h] = [
                geometry.screen.x,
                geometry.screen.y,
                geometry.screen.width,
                geometry.screen.height,
            ];
            frame.capture = [x as f32, y as f32, w as f32, h as f32];
            frame.body = geometry.body;
            let ptr = pointer.unwrap_or(egui::pos2(-1.0, -1.0));
            frame.shape = [layer.radius * ppp, layer.expansion, ptr.x, ptr.y];
            let (flare, height) = layer.notch.unwrap_or_default();
            frame.notch = [
                flare * ppp,
                height * ppp,
                u8::from(layer.notch.is_some()) as f32,
                0.0,
            ];
            frame.env = [time as f32, plate.brightness, ppp, 1.0];
            frame.motion = [speed, self.motion.light, morph, 0.0];
            light_changing = self.motion.update_light(plate.brightness, dt);
            if let Some((id, image)) = layer.artwork {
                if !self.tint.is_some_and(|(known, _)| known == id) {
                    self.tint = Some((id, crate::fluid::Fluid::tint_from_image(&image)));
                }
                if let Some((_, [red, green, blue])) = self.tint {
                    frame.tint = [red, green, blue, 1.0];
                }
            }
            frame.count[0] = controls.count;
            frame.ctl_rect = controls.rects;
            frame.ctl_shape = controls.shapes;
            frame.ctl_spring = controls.springs;
            for (dest, source) in frame.params.iter_mut().flatten().zip(self.options.params) {
                *dest = source;
            }
            let main_shape = self.backend.callback(0, viewport, frame, plate, layer.dock)?;

            for (idx, card) in layer.extra_cards.iter().enumerate() {
                let mut cf = Frame::default();
                let card_min = card.rect.min.to_vec2() * ppp - capture_origin.to_vec2();
                let card_size = card.rect.size() * ppp;
                cf.capture = frame.capture;
                cf.body = [card_min.x, card_min.y, card_size.x, card_size.y];
                cf.shape = [card.radius * ppp, layer.expansion, ptr.x, ptr.y];
                cf.notch = [0.0; 4];
                cf.env = frame.env;
                cf.tint = frame.tint;
                cf.motion = [speed, frame.motion[1], morph, (1.0 - card.fade).clamp(0.0, 1.0)];
                cf.params = frame.params;
                let s = self
                    .backend
                    .callback(idx + 1, viewport, cf, plate, Dock::Top)?;
                extra_shapes.push(s);
            }
            Some(main_shape)
        } else {
            self.backend.clear_backdrop();
            None
        };

        if self.budget.exceeded(started.elapsed()) {
            return Err("材质通道连续 120 帧超过 4 ms".into());
        }
        self.ready = shape.is_some();
        if let Some(shape) = shape {
            if let Some((main_painter, main_slot)) = layer.main.take() {
                main_painter.set(main_slot, shape);
            }
            for (card, card_shape) in layer.extra_cards.drain(..).zip(extra_shapes) {
                card.painter.set(card.slot, card_shape);
            }
            if !self.has_drawn {
                self.has_drawn = true;
                log(&format!(
                    "材质首帧就绪（{} × {}）",
                    geometry.screen.width, geometry.screen.height
                ));
            }
        }
        let delay = self.cadence.remaining(Instant::now(), interval);
        Ok(Some(if light_changing {
            delay.min(Duration::from_secs_f64(1.0 / 60.0))
        } else {
            delay
        }))
    }

    pub fn report_failure(error: &str) {
        log(&format!("材质已停用，退回普通绘制：{error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_refresh_slows_after_two_minutes_and_wakes_on_activity() {
        let start = Instant::now();
        let mut cadence = Cadence::default();
        let idle = Duration::from_secs_f64(1.0 / 15.0);
        assert_eq!(cadence.interval(start, false), idle);
        assert_eq!(
            cadence.interval(start + Duration::from_secs(119), false),
            idle
        );
        let wake = start + Duration::from_secs(120);
        assert_eq!(cadence.interval(wake, false), Duration::from_secs(1));
        assert_eq!(
            cadence.interval(wake, true),
            Duration::from_secs_f64(1.0 / 60.0)
        );
        assert_eq!(cadence.interval(wake + Duration::from_secs(1), false), idle);
    }

    #[test]
    fn material_budget_only_trips_after_120_consecutive_slow_frames() {
        let slow = Duration::from_millis(5);
        let mut budget = Budget::default();
        for _ in 0..119 {
            assert!(!budget.exceeded(slow));
        }
        assert!(!budget.exceeded(Duration::from_millis(4)));
        for _ in 0..119 {
            assert!(!budget.exceeded(slow));
        }
        assert!(budget.exceeded(slow));
    }

    #[test]
    fn material_light_holds_within_the_hysteresis_band() {
        let mut motion = Motion::default();
        let dt = 1.0 / 60.0;
        assert!(motion.update_light(0.51, dt));
        assert!((motion.light - 0.9).abs() < 1e-5);
        let light = motion.light;
        for brightness in [0.52, 0.53, 0.65, 0.66, 0.53] {
            assert!(!motion.update_light(brightness, dt));
            assert_eq!(motion.light, light);
        }
        assert!(motion.update_light(0.67, dt));
        assert!(motion.light > light);
    }

    #[test]
    fn material_light_still_transitions_after_an_idle_frame() {
        let mut motion = Motion::default();
        assert!(motion.update_light(0.40, 1.0));
        assert!((motion.light - 0.4).abs() < 1e-5);
    }

    #[test]
    fn material_expansion_keeps_signed_speed_and_decays_morph_like_the_preview() {
        let mut motion = Motion::default();
        assert_eq!(motion.sample_expansion(0.0, 0.125), (0.0, 0.0));
        assert_eq!(motion.sample_expansion(0.25, 0.125), (2.0, 0.6));
        assert_eq!(motion.sample_expansion(0.75, 0.125), (4.0, 1.0));
        assert_eq!(motion.sample_expansion(0.5, 0.125), (-2.0, 0.74));
        assert_eq!(motion.sample_expansion(0.5, 0.125), (0.0, 0.74 * 0.74));
        for _ in 0..14 {
            assert_eq!(motion.sample_expansion(0.5, 0.125).0, 0.0);
        }
        assert!(motion.morph < 0.01);
    }
}
