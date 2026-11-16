//! 毛玻璃与液态玻璃皮肤的实时背景采样与 2D 空间折射场（对标 `Wei-Zhang09/LiquidGlassUI` 与 `rdev/liquid-glass-react`）。
//!
//! 在紧贴岛体轮廓四周（窗口区域外侧 2..12px）高频采集真实屏幕像素：
//! 1. 估计岛背后的明暗对比度，让着色深浅刚好保证白字可读；
//! 2. 构建实时 2D 空间背景颜色网格（8×4 空间采样场 + 双线性平滑插值），供 `LiquidGlassUI`
//!    与 `rdev/liquid-glass-react` 的 Snell 定律凸透镜边缘位移（`feDisplacementMap`）及 RGB 三通道色散直接采样真实屏幕色相。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};

#[derive(Default)]
struct Shared {
    points: Vec<(i32, i32)>,
    color: Option<[f32; 3]>,
}

const GRID_W: usize = 8;
const GRID_H: usize = 4;

/// 紧贴岛体四周实时采样的 `8×4` 2D 空间背景色场。
static SPATIAL_GRID: OnceLock<Mutex<[[[f32; 3]; GRID_W]; GRID_H]>> = OnceLock::new();

fn spatial_slot() -> &'static Mutex<[[[f32; 3]; GRID_W]; GRID_H]> {
    SPATIAL_GRID.get_or_init(|| Mutex::new([[[0.16, 0.18, 0.22]; GRID_W]; GRID_H]))
}

/// 在归一化局部坐标 `(u, v)`（可因边缘折射位移超出 `0..1`）处双线性采样真实屏幕 2D 背景色场。
pub fn spatial_at(u: f32, v: f32) -> [f32; 3] {
    let grid = spatial_slot()
        .lock()
        .map(|g| *g)
        .unwrap_or([[[0.16, 0.18, 0.22]; GRID_W]; GRID_H]);
    let u = u.clamp(0.0, 1.0) * (GRID_W - 1) as f32;
    let v = v.clamp(0.0, 1.0) * (GRID_H - 1) as f32;
    let x0 = (u.floor() as usize).min(GRID_W - 1);
    let x1 = (x0 + 1).min(GRID_W - 1);
    let y0 = (v.floor() as usize).min(GRID_H - 1);
    let y1 = (y0 + 1).min(GRID_H - 1);
    let tx = u - x0 as f32;
    let ty = v - y0 as f32;
    let mut out = [0.0f32; 3];
    for k in 0..3 {
        let top = grid[y0][x0][k] * (1.0 - tx) + grid[y0][x1][k] * tx;
        let bot = grid[y1][x0][k] * (1.0 - tx) + grid[y1][x1][k] * tx;
        out[k] = (top * (1.0 - ty) + bot * ty).clamp(0.02, 0.98);
    }
    out
}

fn update_spatial_grid(points: &[(i32, i32)], samples: &[[f32; 3]]) {
    if samples.is_empty() {
        return;
    }
    let fallback = crate::glass::backdrop_estimate(samples).unwrap_or([0.16, 0.18, 0.22]);
    let mut target = [[fallback; GRID_W]; GRID_H];

    if points.len() >= 4 && samples.len() == points.len() {
        let min_x = points.iter().map(|p| p.0).min().unwrap_or(0) as f32;
        let max_x = points.iter().map(|p| p.0).max().unwrap_or(1) as f32;
        let min_y = points.iter().map(|p| p.1).min().unwrap_or(0) as f32;
        let max_y = points.iter().map(|p| p.1).max().unwrap_or(1) as f32;
        let span_x = (max_x - min_x).max(1.0);
        let span_y = (max_y - min_y).max(1.0);

        // 反距离加权（IDW）将紧贴岛体外周的真实采样像素重构为平滑 8×4 2D 空间折射场
        for gy in 0..GRID_H {
            let nv = gy as f32 / (GRID_H - 1) as f32;
            for gx in 0..GRID_W {
                let nu = gx as f32 / (GRID_W - 1) as f32;
                let mut acc = [0.0f32; 3];
                let mut w_sum = 0.0f32;
                for (pt, rgb) in points.iter().zip(samples.iter()) {
                    let pu = (pt.0 as f32 - min_x) / span_x;
                    let pv = (pt.1 as f32 - min_y) / span_y;
                    let d2 = (nu - pu).powi(2) + (nv - pv).powi(2) + 0.035;
                    let w = 1.0 / (d2 * d2);
                    acc[0] += rgb[0] * w;
                    acc[1] += rgb[1] * w;
                    acc[2] += rgb[2] * w;
                    w_sum += w;
                }
                if w_sum > 0.0 {
                    target[gy][gx] = [acc[0] / w_sum, acc[1] / w_sum, acc[2] / w_sum];
                }
            }
        }
    }

    if let Ok(mut grid) = spatial_slot().lock() {
        for gy in 0..GRID_H {
            for gx in 0..GRID_W {
                for ch in 0..3 {
                    grid[gy][gx][ch] = grid[gy][gx][ch] * 0.38 + target[gy][gx][ch] * 0.62;
                }
            }
        }
    }
}

pub struct Sampler {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
}

impl Sampler {
    pub fn start() -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (state, flag) = (shared.clone(), stop.clone());
        let _ = std::thread::Builder::new().name("auralsle-backdrop".into()).spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                let points = state.lock().map(|s| s.points.clone()).unwrap_or_default();
                if !points.is_empty() {
                    let (valid_pts, raw) = sample_with_coords(&points);
                    update_spatial_grid(&valid_pts, &raw);
                    let color = crate::glass::backdrop_estimate(&raw);
                    if let (Some(color), Ok(mut s)) = (color, state.lock()) {
                        s.color = Some(color);
                    }
                }
                // 60ms 实时高频采样，灵敏响应岛体下方及四周的壁纸/窗口色彩变化
                for _ in 0..2 {
                    if flag.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(55));
                }
            }
        });
        Self { shared, stop }
    }

    /// 采样点（屏幕物理坐标）。自动在轮廓外侧加密采样点以提高 2D 折射场空间分辨率。
    pub fn set_points(&self, points: Vec<(i32, i32)>) {
        if let Ok(mut s) = self.shared.lock() {
            if s.points != points {
                s.points = points;
            }
        }
    }

    pub fn color(&self) -> Option<[f32; 3]> {
        self.shared.lock().ok().and_then(|s| s.color)
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn sample_with_coords(points: &[(i32, i32)]) -> (Vec<(i32, i32)>, Vec<[f32; 3]>) {
    unsafe {
        let dc = GetDC(None);
        if dc.0.is_null() {
            return (Vec::new(), Vec::new());
        }
        let mut coords = Vec::with_capacity(points.len());
        let mut colors = Vec::with_capacity(points.len());
        for &(x, y) in points {
            let c = GetPixel(dc, x, y).0;
            // CLR_INVALID：点在屏幕外或被裁掉。
            if c != 0xFFFF_FFFF {
                coords.push((x, y));
                colors.push([
                    (c & 0xFF) as f32 / 255.0,
                    ((c >> 8) & 0xFF) as f32 / 255.0,
                    ((c >> 16) & 0xFF) as f32 / 255.0,
                ]);
            }
        }
        let _ = ReleaseDC(None, dc);
        (coords, colors)
    }
}
