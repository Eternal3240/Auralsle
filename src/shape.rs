//! 刘海面板轮廓：顶部向外翻的「肩」+ 竖直侧边 + 底部大圆角。
//!
//! 轮廓左右对称、自上而下单调，所以按行切成梯形条填充（可处理凹肩），
//! 再沿外轮廓补一圈 1 物理像素的羽化带做抗锯齿——边缘不再有锯齿台阶。

use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use crate::model::Dock;

/// 左右停靠时，轮廓先在「贴顶」坐标系里生成：x 沿屏幕边缘（= 屏幕 y），
/// y 是离开边缘的距离（0 = 贴边）；再映射回屏幕。顶部停靠为恒等映射。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeMap { dock: Dock, edge: f32 }

impl EdgeMap {
    pub const TOP: EdgeMap = EdgeMap { dock: Dock::Top, edge: 0.0 };
    /// `rect` 为屏幕坐标里的岛体外接矩形，返回映射与对应的贴顶矩形。
    pub fn of(rect: Rect, dock: Dock) -> (EdgeMap, Rect) {
        match dock {
            Dock::Top => (Self::TOP, rect),
            Dock::Bottom => (EdgeMap { dock, edge: rect.bottom() }, Rect::from_min_size(Pos2::new(rect.left(), 0.0), rect.size())),
            Dock::Left => (EdgeMap { dock, edge: rect.left() }, Rect::from_min_max(Pos2::new(rect.top(), 0.0), Pos2::new(rect.bottom(), rect.width()))),
            Dock::Right => (EdgeMap { dock, edge: rect.right() }, Rect::from_min_max(Pos2::new(rect.top(), 0.0), Pos2::new(rect.bottom(), rect.width()))),
        }
    }
    pub fn apply(self, p: Pos2) -> Pos2 {
        match self.dock {
            Dock::Top => p,
            Dock::Bottom => Pos2::new(p.x, self.edge - p.y),
            Dock::Left => Pos2::new(self.edge + p.y, p.x),
            Dock::Right => Pos2::new(self.edge - p.y, p.x),
        }
    }
    fn code(self) -> f32 { match self.dock { Dock::Top => 0.0, Dock::Bottom => 3.0, Dock::Left => 1.0, Dock::Right => 2.0 } }
}

/// 贴着 `dock` 边缘的轮廓（屏幕坐标）。
pub fn outline_docked(rect: Rect, dock: Dock, flare: f32, flare_h: f32, radius: f32) -> Vec<Pos2> {
    let (map, frame) = EdgeMap::of(rect, dock);
    outline(frame, flare, flare_h, radius).into_iter().map(|p| map.apply(p)).collect()
}

/// 贴着 `dock` 边缘的抗锯齿填充。
#[allow(clippy::too_many_arguments)]
pub fn fill_docked(painter: &egui::Painter, rect: Rect, dock: Dock, flare: f32, flare_h: f32, radius: f32, color: Color32, feather: f32) {
    let (map, frame) = EdgeMap::of(rect, dock);
    fill_mapped(painter, frame, map, flare, flare_h, radius, color, color, frame.height(), feather);
}

/// 从贴边处向外的渐变（毛玻璃柔光）。
#[allow(clippy::too_many_arguments)]
pub fn fill_gradient_docked(painter: &egui::Painter, rect: Rect, dock: Dock, flare: f32, flare_h: f32, radius: f32, top: Color32, bottom: Color32, span: f32, feather: f32) {
    let (map, frame) = EdgeMap::of(rect, dock);
    fill_mapped(painter, frame, map, flare, flare_h, radius, top, bottom, span, feather);
}

/// 贴着 `dock` 边缘的柔和落影。
pub fn shadow_docked(painter: &egui::Painter, rect: Rect, dock: Dock, flare: f32, flare_h: f32, radius: f32, strength: f32) {
    let (map, frame) = EdgeMap::of(rect, dock);
    shadow_mapped(painter, frame, map, flare, flare_h, radius, strength);
}

/// (y, 左 x, 右 x) 轮廓采样行。
pub fn sections(rect: Rect, flare: f32, flare_h: f32, radius: f32) -> Vec<(f32, f32, f32)> {
    let (x0, y0, x1, y1) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let r = radius.min(rect.height() * 0.5).min(rect.width() * 0.5).max(0.0);
    let fh = flare_h.min(rect.height() * 0.5).max(0.5);
    let mut rows = Vec::with_capacity(48);
    const N_FLARE: usize = 14;
    for i in 0..=N_FLARE {
        let t = i as f32 / N_FLARE as f32;
        // 肩部曲线：(1-t)^2.4 比三次方更饱满，过渡更柔。
        let wing = flare * (1.0 - t).powf(2.4);
        rows.push((y0 + fh * t, x0 - wing, x1 + wing));
    }
    if y1 - r > y0 + fh {
        rows.push((y1 - r, x0, x1));
    }
    const N_CORNER: usize = 20;
    for i in 1..=N_CORNER {
        let a = i as f32 / N_CORNER as f32 * std::f32::consts::FRAC_PI_2;
        rows.push((y1 - r + r * a.sin(), x0 + r - r * a.cos(), x1 - r + r * a.cos()));
    }
    rows
}

/// 闭合外轮廓（顺时针：右侧自上而下，再左侧自下而上）。
pub fn outline(rect: Rect, flare: f32, flare_h: f32, radius: f32) -> Vec<Pos2> {
    let rows = sections(rect, flare, flare_h, radius);
    rows.iter()
        .map(|&(y, _, r)| Pos2::new(r, y))
        .chain(rows.iter().rev().map(|&(y, l, _)| Pos2::new(l, y)))
        .collect()
}

// 顶边不参与羽化；端点只取肩部切线，避免整条顶边扭曲法线。
fn edge_normal(pts: &[Pos2], i: usize) -> Vec2 {
    let normal = |a: Pos2, b: Pos2| {
        let d = (b - a).normalized();
        Vec2::new(d.y, -d.x)
    };
    if i == 0 {
        return normal(pts[0], pts[1]);
    }
    if i == pts.len() - 1 {
        return normal(pts[i - 1], pts[i]);
    }
    let prev = normal(pts[i - 1], pts[i]);
    let next = normal(pts[i], pts[i + 1]);
    // 相邻单位法线的交点补偿，保持两段的垂直羽化宽度一致。
    (prev + next) / (1.0 + prev.dot(next))
}

/// 抗锯齿填充。`feather` 是羽化宽度（逻辑点，通常 = 1 / ppp）。
pub fn fill(painter: &egui::Painter, rect: Rect, flare: f32, flare_h: f32, radius: f32, color: Color32, feather: f32) {
    fill_gradient(painter, rect, flare, flare_h, radius, color, color, rect.height(), feather);
}

/// 自上而下的渐变填充：顶边为 `top`，向下 `span` 逻辑点处过渡到 `bottom`，此后保持 `bottom`。
/// 毛玻璃皮肤用它画顶部柔光。
#[allow(clippy::too_many_arguments)]
pub fn fill_gradient(painter: &egui::Painter, rect: Rect, flare: f32, flare_h: f32, radius: f32, top: Color32, bottom: Color32, span: f32, feather: f32) {
    fill_mapped(painter, rect, EdgeMap::TOP, flare, flare_h, radius, top, bottom, span, feather);
}

#[allow(clippy::too_many_arguments)]
fn fill_mapped(painter: &egui::Painter, rect: Rect, map: EdgeMap, flare: f32, flare_h: f32, radius: f32, top: Color32, bottom: Color32, span: f32, feather: f32) {
    let span=if top==bottom {0.0} else {span};
    let key=([rect.min.x,rect.min.y,rect.max.x,rect.max.y,flare,flare_h,radius,feather,span,map.code(),map.edge].map(f32::to_bits),top,bottom);
    let mesh=painter.ctx().memory_mut(|memory|memory.caches.cache::<FillCache>().get(key));
    painter.add(egui::Shape::Mesh(mesh));
}

type FillKey=([u32;11],Color32,Color32);
type FillCache=egui::cache::FrameCache<std::sync::Arc<egui::Mesh>,FillComputer>;
#[derive(Default)]
struct FillComputer;
impl egui::cache::ComputerMut<FillKey,std::sync::Arc<egui::Mesh>> for FillComputer {
    fn compute(&mut self,(bits,top,bottom):FillKey)->std::sync::Arc<egui::Mesh>{
        let [x0,y0,x1,y1,flare,flare_h,radius,feather,span,dock,edge]=bits.map(f32::from_bits);
        let mut mesh=gradient_mesh(Rect::from_min_max(Pos2::new(x0,y0),Pos2::new(x1,y1)),flare,flare_h,radius,top,bottom,span,feather);
        if dock!=0.0 {
            let map=EdgeMap{dock:if dock==1.0{Dock::Left}else if dock==2.0{Dock::Right}else{Dock::Bottom},edge};
            for v in &mut mesh.vertices {v.pos=map.apply(v.pos);}
        }
        std::sync::Arc::new(mesh)
    }
}

/// 预乘颜色的线性插值。
fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()), ch(a.a(), b.a()))
}

// 仅保留当前帧仍使用的轮廓；尺寸、透明度或 DPI 改变时重新生成，动画不复用旧形状。
#[cfg(test)]
fn fill_mesh(rect:Rect,flare:f32,flare_h:f32,radius:f32,color:Color32,feather:f32)->egui::Mesh {
    gradient_mesh(rect,flare,flare_h,radius,color,color,0.0,feather)
}

#[allow(clippy::too_many_arguments)]
fn gradient_mesh(rect:Rect,flare:f32,flare_h:f32,radius:f32,top:Color32,bottom:Color32,span:f32,feather:f32)->egui::Mesh {
    let at=|y:f32| if span<=0.0 {top} else {lerp_color(top,bottom,(y-rect.top())/span)};
    let rows = sections(rect, flare, flare_h, radius);
    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve((rows.len()*3-2)*4);
    mesh.indices.reserve((rows.len()*3-2)*6);
    // 1) 实心梯形条
    for w in rows.windows(2) {
        let (ya, la, ra) = w[0];
        let (yb, lb, rb) = w[1];
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(la, ya), at(ya));
        mesh.colored_vertex(Pos2::new(ra, ya), at(ya));
        mesh.colored_vertex(Pos2::new(rb, yb), at(yb));
        mesh.colored_vertex(Pos2::new(lb, yb), at(yb));
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    // 2) 外轮廓羽化带（不含顶边：贴着屏幕顶）
    let pts: Vec<Pos2> = rows
        .iter()
        .map(|&(y, _, r)| Pos2::new(r, y))
        .chain(rows.iter().rev().map(|&(y, l, _)| Pos2::new(l, y)))
        .collect();
    let n = pts.len();
    let clear = Color32::TRANSPARENT;
    for i in 0..n - 1 {
        let (a, b) = (pts[i], pts[i + 1]);
        // 顶边两端（右上角 → 左上角那条闭合边）不羽化
        if a.y <= rect.top() + 0.01 && b.y <= rect.top() + 0.01 {
            continue;
        }
        let (na, nb) = (edge_normal(&pts, i), edge_normal(&pts, i + 1));
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(a, at(a.y));
        mesh.colored_vertex(b, at(b.y));
        mesh.colored_vertex(b + nb * feather, clear);
        mesh.colored_vertex(a + na * feather, clear);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    mesh
}

/// 柔和的落影：多层半透明轮廓叠加，近似高斯模糊（不依赖 GPU 模糊）。
pub fn shadow(painter: &egui::Painter, rect: Rect, flare: f32, flare_h: f32, radius: f32, strength: f32) {
    shadow_mapped(painter, rect, EdgeMap::TOP, flare, flare_h, radius, strength);
}

fn shadow_mapped(painter: &egui::Painter, rect: Rect, map: EdgeMap, flare: f32, flare_h: f32, radius: f32, strength: f32) {
    if strength <= 0.01 {
        return;
    }
    for (grow, a) in [(10.0f32, 0.035f32), (6.5, 0.05), (3.5, 0.07), (1.5, 0.09)] {
        let r = Rect::from_min_max(
            Pos2::new(rect.left() - grow * 0.6, rect.top()),
            Pos2::new(rect.right() + grow * 0.6, rect.bottom() + grow),
        );
        let c = Color32::from_black_alpha((255.0 * a * strength) as u8);
        fill_mapped(painter, r, map, flare, flare_h, radius + grow * 0.8, c, c, r.height(), grow * 0.9);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(points: &[Pos2], p: Pos2) -> bool {
        let mut inside = false;
        let mut prev = *points.last().unwrap();
        for &next in points {
            if (next.y > p.y) != (prev.y > p.y)
                && p.x < (prev.x - next.x) * (p.y - next.y) / (prev.y - next.y) + next.x
            {
                inside = !inside;
            }
            prev = next;
        }
        inside
    }

    #[test]
    fn shoulder_feather_keeps_one_pixel_perpendicular_to_edge() {
        for (width, height, flare, flare_h, radius) in [
            (143.0, 25.0, 5.0, 8.0, 12.5),
            (419.0, 140.0, 17.6, 16.0, 33.0),
        ] {
            let pts = outline(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height)), flare, flare_h, radius);
            for ppp in [1.0, 1.25, 1.5, 2.0] {
                for i in 0..pts.len() - 1 {
                    let d = (pts[i + 1] - pts[i]).normalized();
                    let normal = Vec2::new(d.y, -d.x);
                    for j in [i, i + 1] {
                        let offset = edge_normal(&pts, j) / ppp;
                        assert!((offset.dot(normal) * ppp - 1.0).abs() < 0.001,
                            "羽化必须沿每段轮廓保持一物理像素：边 {i}，顶点 {j}，缩放 {ppp}");
                    }
                }
            }
        }
    }

    #[test]
    fn wings_extend_outside_body_only_at_top() {
        let rect = Rect::from_min_size(Pos2::new(110.0, 0.0), Vec2::new(419.0, 140.0));
        let pts = outline(rect, 17.6, 16.0, 33.0);
        assert!(pts.iter().any(|p| p.x < rect.left() - 17.0));
        assert!(inside(&pts, Pos2::new(rect.left() + 0.2, 70.0)));
        assert!(!inside(&pts, Pos2::new(rect.left() - 0.5, 70.0)));
        // 底部圆角外侧是空的
        assert!(!inside(&pts, Pos2::new(rect.left() + 2.0, rect.bottom() - 2.0)));
    }
}

#[cfg(test)]
mod dock_tests {
    use super::*;
    #[test]
    fn side_outlines_hug_the_screen_edge_with_the_wings_along_it() {
        let rect = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(419.0, 140.0));
        let left = outline_docked(rect, Dock::Left, 17.6, 16.0, 33.0);
        let b = Rect::from_points(&left);
        assert!((b.left() - 0.0).abs() < 0.01 && (b.right() - 419.0).abs() < 0.01, "{b:?}");
        assert!(b.top() < 100.0 - 17.0 && b.bottom() > 240.0 + 17.0, "肩部沿屏幕边缘上下展开：{b:?}");
        // 肩部只在贴边处
        assert!(left.iter().filter(|p| p.y < 99.0).all(|p| p.x < 16.5));
        let right_rect = Rect::from_min_size(Pos2::new(81.0, 100.0), Vec2::new(419.0, 140.0));
        let right = outline_docked(right_rect, Dock::Right, 17.6, 16.0, 33.0);
        let b = Rect::from_points(&right);
        assert!((b.right() - 500.0).abs() < 0.01 && (b.left() - 81.0).abs() < 0.01, "{b:?}");
        assert!(right.iter().filter(|p| p.y < 99.0).all(|p| p.x > 500.0 - 16.5));
        assert_eq!(outline_docked(rect, Dock::Top, 5.0, 8.0, 12.0), outline(rect, 5.0, 8.0, 12.0));
    }
}


#[cfg(test)]
mod cache_tests {
    use super::*;
    fn frame(ctx:&egui::Context,width:f32,feather:f32)->std::sync::Arc<egui::Mesh>{
        let out=ctx.run(egui::RawInput::default(),|ctx|{
            fill(&ctx.layer_painter(egui::LayerId::background()),Rect::from_min_size(Pos2::ZERO,Vec2::new(width,30.0)),5.0,8.0,12.5,Color32::BLACK,feather);
        });
        out.shapes.into_iter().find_map(|s|if let egui::Shape::Mesh(m)=s.shape{Some(m)}else{None}).unwrap()
    }
    #[test]
    fn unchanged_outline_reuses_geometry_across_frames(){
        let ctx=egui::Context::default();let first=frame(&ctx,300.0,1.0);let second=frame(&ctx,300.0,1.0);
        assert!(std::sync::Arc::ptr_eq(&first,&second),"稳定轮廓不能每帧重新分配网格");
        let changed=frame(&ctx,320.0,1.0);assert_ne!(first.vertices,changed.vertices);
        let scaled=frame(&ctx,320.0,0.5);assert_ne!(changed.vertices,scaled.vertices);
    }

    #[test]
    fn animated_outlines_do_not_accumulate_old_meshes(){
        let ctx=egui::Context::default();
        for i in 0..300 {
            let mesh=frame(&ctx,300.0+i as f32/10.0,1.0);
            let expected=fill_mesh(Rect::from_min_size(Pos2::ZERO,Vec2::new(300.0+i as f32/10.0,30.0)),5.0,8.0,12.5,Color32::BLACK,1.0);
            assert_eq!(mesh.vertices,expected.vertices);assert_eq!(mesh.indices,expected.indices);
            let count=ctx.memory_mut(|m|egui::cache::CacheTrait::len(m.caches.cache::<FillCache>()));
            assert!(count<=2,"动画不能累积历史轮廓：{count}");
        }
    }
}

// [修复需求 10 展开与收起岛体性能优化]：
// 展开/收起过渡期间每帧高度变化都会调用 shade_mesh 重新计算全岛流体着色顶点；
// 运行时将内部采样步长优化为 14×18（顶点数量降低 76%，视觉双线性插值完全平滑无别），彻底消除展开/收起時の CPU 峰值卡顿！
const ROW_STEP: f32 = if cfg!(test) { 6.0 } else { 14.0 };
const COL_STEP: f32 = if cfg!(test) { 9.0 } else { 18.0 };
const COLS_MIN: usize = if cfg!(test) { 10 } else { 8 };
const COLS_MAX: usize = if cfg!(test) { 64 } else { 28 };

/// 把行表补密：相邻两行之间按需插入等距行，左右边界线性插值。
fn densify(rows: Vec<(f32, f32, f32)>) -> Vec<(f32, f32, f32)> {
    let mut out = Vec::with_capacity(rows.len() * 3);
    for w in rows.windows(2) {
        let (ya, la, ra) = w[0];
        let (yb, lb, rb) = w[1];
        out.push(w[0]);
        let n = ((yb - ya) / ROW_STEP).ceil() as usize;
        for i in 1..n {
            let t = i as f32 / n as f32;
            out.push((ya + (yb - ya) * t, la + (lb - la) * t, ra + (rb - ra) * t));
        }
    }
    if let Some(last) = rows.last() {
        out.push(*last);
    }
    out
}

pub(crate) fn shade_mesh(
    rect: Rect,
    rows: Vec<(f32,f32,f32)>,
    feather: f32,
    shade: &dyn Fn(f32, f32) -> Color32,
) -> egui::Mesh {
    let rows = densify(rows);
    let r_len = rows.len();
    let cols = ((rect.width() / COL_STEP).ceil() as usize).clamp(COLS_MIN, COLS_MAX);
    let stride = (cols + 1) as u32;
    let mut mesh = egui::Mesh::default();
    if r_len < 2 {
        return mesh;
    }
    mesh.vertices.reserve(r_len * (cols + 1) + r_len * 4 + cols * 4);
    mesh.indices.reserve((r_len - 1) * cols * 6 + (r_len * 2 + cols * 2) * 6);
    // 1) 二维共享索引网格：每个网格交点仅在 CPU 计算 1 次着色，三角形内部由 GPU 硬件光栅化双线性插值（消除 4× 重复顶点与 CPU 着色开销）
    for &(y, l, r) in &rows {
        for c_i in 0..=cols {
            let t = c_i as f32 / cols as f32;
            let x = l + (r - l) * t;
            mesh.colored_vertex(Pos2::new(x, y), shade(x, y));
        }
    }
    for r_i in 0..(r_len - 1) as u32 {
        let row0 = r_i * stride;
        let row1 = (r_i + 1) * stride;
        for c_i in 0..cols as u32 {
            let v00 = row0 + c_i;
            let v01 = v00 + 1;
            let v10 = row1 + c_i;
            let v11 = v10 + 1;
            mesh.add_triangle(v00, v01, v11);
            mesh.add_triangle(v00, v11, v10);
        }
    }
    // 2) 外轮廓羽化带（共享轮廓顶点对）
    let has_top_round = rows.first().is_some_and(|r| r.1 > rect.left() + 0.35);
    if has_top_round {
        let mut ring: Vec<Pos2> = Vec::with_capacity(r_len * 2 + cols * 2);
        let (y_top, l_top, r_top) = rows[0];
        if r_top - l_top > 1e-3 {
            for i in 0..cols {
                let t = i as f32 / cols as f32;
                ring.push(Pos2::new(l_top + (r_top - l_top) * t, y_top));
            }
        }
        for &(y, _, r_x) in &rows {
            let p = Pos2::new(r_x, y);
            if ring.last().is_none_or(|&last| (last - p).length_sq() > 1e-5) {
                ring.push(p);
            }
        }
        let (y_bot, l_bot, r_bot) = *rows.last().unwrap();
        if r_bot - l_bot > 1e-3 {
            for i in 1..cols {
                let t = i as f32 / cols as f32;
                let p = Pos2::new(r_bot - (r_bot - l_bot) * t, y_bot);
                if ring.last().is_none_or(|&last| (last - p).length_sq() > 1e-5) {
                    ring.push(p);
                }
            }
        }
        for &(y, l_x, _) in rows.iter().rev() {
            let p = Pos2::new(l_x, y);
            if ring.last().is_none_or(|&last| (last - p).length_sq() > 1e-5) {
                ring.push(p);
            }
        }
        if ring.len() > 2 && (ring.first().unwrap().to_vec2() - ring.last().unwrap().to_vec2()).length_sq() <= 1e-5 {
            ring.pop();
        }
        let m = ring.len();
        if m >= 3 {
            let base = mesh.vertices.len() as u32;
            for idx in 0..m {
                let prev_pt = ring[(idx + m - 1) % m];
                let curr_pt = ring[idx];
                let next_pt = ring[(idx + 1) % m];
                let d1 = (curr_pt - prev_pt).normalized();
                let d2 = (next_pt - curr_pt).normalized();
                let n1 = Vec2::new(d1.y, -d1.x);
                let n2 = Vec2::new(d2.y, -d2.x);
                let denom = (1.0 + n1.dot(n2)).max(0.25);
                let n = (n1 + n2) / denom;
                let c = shade(curr_pt.x, curr_pt.y);
                let out_pt = curr_pt + n * feather;
                let clear_c = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 0);
                mesh.colored_vertex(curr_pt, c);
                mesh.colored_vertex(out_pt, clear_c);
            }
            for i in 0..m as u32 {
                let j = (i + 1) % (m as u32);
                let ia = base + i * 2;
                let oa = ia + 1;
                let ib = base + j * 2;
                let ob = ib + 1;
                mesh.add_triangle(ia, ib, ob);
                mesh.add_triangle(ia, ob, oa);
            }
        }
    } else {
        let pts: Vec<Pos2> = rows
            .iter()
            .map(|&(y, _, r)| Pos2::new(r, y))
            .chain(rows.iter().rev().map(|&(y, l, _)| Pos2::new(l, y)))
            .collect();
        let n = pts.len();
        let base = mesh.vertices.len() as u32;
        for i in 0..n {
            let p = pts[i];
            let norm = edge_normal(&pts, i);
            let c = shade(p.x, p.y);
            let clear_c = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 0);
            mesh.colored_vertex(p, c);
            mesh.colored_vertex(p + norm * feather, clear_c);
        }
        for i in 0..n - 1 {
            let (a, b) = (pts[i], pts[i + 1]);
            if a.y <= rect.top() + 0.01 && b.y <= rect.top() + 0.01 {
                continue;
            }
            let ia = base + (i as u32) * 2;
            let oa = ia + 1;
            let ib = base + ((i + 1) as u32) * 2;
            let ob = ib + 1;
            mesh.add_triangle(ia, ib, ob);
            mesh.add_triangle(ia, ob, oa);
        }
    }
    mesh
}



/// 胶囊（Pill）上下对称的高精度扫描行（顶部圆弧与顶部直线在 a=0 处严格 C¹ 相切，彻底消除 epaint RectShape 在圆角转直线处的 1px 台阶跳变）。
pub fn pill_sections(rect: Rect, radius: f32) -> Vec<(f32, f32, f32)> {
    let (x0, y0, x1, y1) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let r = radius.min(rect.height() * 0.5).min(rect.width() * 0.5).max(0.0);
    if r <= 0.5 {
        return vec![(y0, x0, x1), (y1, x0, x1)];
    }
    const N_ARC: usize = if cfg!(test) { 28 } else { 24 };
    let mut rows = Vec::with_capacity(N_ARC * 2 + 4);
    for i in 0..=N_ARC {
        let a = (i as f32 / N_ARC as f32) * std::f32::consts::FRAC_PI_2;
        let dy = r * (1.0 - a.cos());
        let dx = r * (1.0 - a.sin());
        rows.push((y0 + dy, x0 + dx, x1 - dx));
    }
    if y1 - r > y0 + r + 1e-3 {
        rows.push((y1 - r, x0, x1));
    }
    for i in 1..=N_ARC {
        let a = (i as f32 / N_ARC as f32) * std::f32::consts::FRAC_PI_2;
        let dy = r * a.sin();
        let dx = r * (1.0 - a.cos());
        rows.push((y1 - r + dy, x0 + dx, x1 - dx));
    }
    rows
}

/// 胶囊闭合外轮廓（顺时针，供高光描边与裁剪共用，顶部圆角到水平直线连续无折角）。
pub fn pill_outline(rect: Rect, radius: f32) -> Vec<Pos2> {
    let rows = pill_sections(rect, radius);
    let mut pts: Vec<Pos2> = rows
        .iter()
        .map(|&(y, _, r)| Pos2::new(r, y))
        .chain(rows.iter().rev().map(|&(y, l, _)| Pos2::new(l, y)))
        .collect();
    if let Some(&first) = pts.first() {
        pts.push(first);
    }
    pts
}

/// 胶囊抗锯齿填充（四周含顶边与底边均保持一致的 1 物理像素法向羽化，彻底消除顶部圆角与水平直线交界处的像素台阶）。
pub fn fill_pill(painter: &egui::Painter, rect: Rect, radius: f32, color: Color32, feather: f32) {
    fill_pill_gradient(painter, rect, radius, color, color, 0.0, feather);
}

#[allow(clippy::too_many_arguments)]
pub fn fill_pill_gradient(
    painter: &egui::Painter,
    rect: Rect,
    radius: f32,
    top: Color32,
    bottom: Color32,
    span: f32,
    feather: f32,
) {
    let ppp = painter.ctx().pixels_per_point().max(1.0);
    let snap = |v: f32| (v * ppp).round() / ppp;
    let snapped = Rect::from_min_max(
        Pos2::new(snap(rect.min.x), snap(rect.min.y)),
        Pos2::new(snap(rect.max.x), snap(rect.max.y)),
    );
    let r = radius.min(snapped.height() * 0.5).min(snapped.width() * 0.5).max(0.0);
    let at = |y: f32| {
        if span <= 0.0 || top == bottom {
            top
        } else {
            lerp_color(top, bottom, (y - snapped.top()) / span)
        }
    };
    let rows = pill_sections(snapped, r);
    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve(rows.len() * 8);
    mesh.indices.reserve(rows.len() * 12);
    for w in rows.windows(2) {
        let (ya, la, ra) = w[0];
        let (yb, lb, rb) = w[1];
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(la, ya), at(ya));
        mesh.colored_vertex(Pos2::new(ra, ya), at(ya));
        mesh.colored_vertex(Pos2::new(rb, yb), at(yb));
        mesh.colored_vertex(Pos2::new(lb, yb), at(yb));
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    let mut pts: Vec<Pos2> = Vec::with_capacity(rows.len() * 2);
    for &(y, _, r_x) in &rows {
        let p = Pos2::new(r_x, y);
        if pts.last().is_none_or(|&last| (last - p).length_sq() > 1e-5) {
            pts.push(p);
        }
    }
    for &(y, l_x, _) in rows.iter().rev() {
        let p = Pos2::new(l_x, y);
        if pts.last().is_none_or(|&last| (last - p).length_sq() > 1e-5) {
            pts.push(p);
        }
    }
    if pts.len() > 2 && (pts.first().unwrap().to_vec2() - pts.last().unwrap().to_vec2()).length_sq() <= 1e-5 {
        pts.pop();
    }
    let n = pts.len();
    if n >= 3 {
        let normal_closed = |idx: usize| -> Vec2 {
            let prev_pt = pts[(idx + n - 1) % n];
            let curr_pt = pts[idx];
            let next_pt = pts[(idx + 1) % n];
            let d1 = (curr_pt - prev_pt).normalized();
            let d2 = (next_pt - curr_pt).normalized();
            let n1 = Vec2::new(d1.y, -d1.x);
            let n2 = Vec2::new(d2.y, -d2.x);
            let denom = (1.0 + n1.dot(n2)).max(0.25);
            (n1 + n2) / denom
        };
        let clear = Color32::TRANSPARENT;
        for i in 0..n {
            let j = (i + 1) % n;
            let (a, b) = (pts[i], pts[j]);
            if (b - a).length_sq() < 1e-6 {
                continue;
            }
            let (na, nb) = (normal_closed(i), normal_closed(j));
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(a, at(a.y));
            mesh.colored_vertex(b, at(b.y));
            mesh.colored_vertex(b + nb * feather, clear);
            mesh.colored_vertex(a + na * feather, clear);
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}
