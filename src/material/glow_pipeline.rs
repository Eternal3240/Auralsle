//! 轻九九九（@cayww）社区液态玻璃材质 OpenGL 3.3（`Renderer::Glow`）硬件加速管线。
//!
//! 1:1 对应 `prelude.wgsl` + `cayww_liquid_glass.wgsl`，供 AuraIsle 默认的 OpenGL/WGL 后端
//! 在不切换渲染后端的情况下直接呈现真实桌面折射、色散、霜化与控件玻璃。

use std::sync::{Arc, Mutex};

use eframe::{
    egui::{self, PaintCallback},
    egui_glow, glow,
};
use glow::HasContext as _;

use super::{pipeline, CleanPlate};
use crate::model::Dock;

const VERT_SRC: &str = r#"#version 330 core
out vec2 v_uv;
void main() {
    vec2 uv = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_uv = uv;
    gl_Position = vec4(2.0 * uv.x - 1.0, 1.0 - 2.0 * uv.y, 0.0, 1.0);
}
"#;

const FRAG_SRC: &str = r#"#version 330 core
in vec2 v_uv;
out vec4 frag_color;

uniform vec4 u_capture;
uniform vec4 u_body;
uniform vec4 u_shape;
uniform vec4 u_notch;
uniform vec4 u_env;
uniform vec4 u_tint;
uniform vec4 u_tex;
uniform vec4 u_motion;
uniform ivec4 u_count;
uniform vec4 u_ctl_rect[16];
uniform vec4 u_ctl_shape[16];
uniform vec4 u_ctl_spring[16];
uniform vec4 u_params[8];
uniform sampler2D u_backdrop;

const vec3 LUMA = vec3(0.299, 0.587, 0.114);
const vec3 NEUTRAL_TINT = vec3(0.62, 0.66, 0.74);
const uint ROLE_ISLAND_LYRICS = 7u;
const uint ROLE_SEEK = 10u;

float param(uint i) {
    vec4 v = u_params[i / 4u];
    uint k = i % 4u;
    if (k == 0u) return v.x;
    if (k == 1u) return v.y;
    if (k == 2u) return v.z;
    return v.w;
}

float rr_sdf(vec2 p, vec2 b, float r) {
    vec2 q = abs(p) - b + vec2(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - r;
}

float daozai_notch_sdf(vec2 p, vec2 b, float r, float flare, float fh) {
    float t = clamp((p.y + b.y) / fh, 0.0, 1.0);
    float wing = flare * pow(1.0 - t, 2.4);
    float hx = b.x + wing;
    if (p.y > b.y - r) {
        float qx = max(abs(p.x) - (b.x - r), 0.0);
        float qy = abs(p.y) - (b.y - r);
        return length(vec2(qx, max(qy, 0.0))) - r;
    }
    return max(abs(p.x) - hx, abs(p.y) - b.y);
}

float notch_sdf(vec2 p, vec2 b, float r, float flare, float fh) {
    return daozai_notch_sdf(p, b, r, flare, max(fh, 1.0));
}

float daozai_body_sdf(vec2 p, float inset, float minimum) {
    vec2 half_size = max(u_body.zw * 0.5 - vec2(inset), vec2(minimum));
    vec2 local = p - u_body.xy - u_body.zw * 0.5;
    float radius = max(min(u_shape.x - inset, min(half_size.x, half_size.y)), 0.0);
    if (u_notch.z > 0.5) {
        int dock = u_count.y;
        if (dock == 1) {
            local = local.yx;
            half_size = half_size.yx;
        } else if (dock == 2) {
            local = vec2(local.y, -local.x);
            half_size = half_size.yx;
        } else if (dock == 3) {
            local.y = -local.y;
        }
        if (inset == 0.0) {
            float fh = max(min(u_notch.y, half_size.y), 0.5 * u_env.z);
            return daozai_notch_sdf(local, half_size, radius, u_notch.x, max(fh, 0.0001));
        }
        return notch_sdf(local, half_size, radius, u_notch.x, u_notch.y);
    }
    return rr_sdf(local, half_size, radius);
}

float body_sdf(vec2 p) {
    return daozai_body_sdf(p, 3.0, 2.0);
}

float body_sdf_exact(vec2 p) {
    return daozai_body_sdf(p, 0.0, 0.0);
}

vec3 sample_backdrop(vec2 px) {
    vec2 valid = max(u_tex.zw, vec2(1.0));
    vec2 scale = valid / max(u_tex.xy, vec2(1.0));
    vec2 uv = clamp(px / valid, vec2(0.001), vec2(0.999)) * scale;
    return textureLod(u_backdrop, uv, 0.0).rgb;
}

float hash21(vec2 p) {
    vec2 q = fract(p * vec2(0.1031, 0.1030));
    q = q + vec2(dot(q, q.yx + vec2(33.33)));
    return fract((q.x + q.y) * q.x);
}

vec3 tap5(vec2 p, vec2 dispersion, float blur) {
    vec2 offsets[5] = vec2[5](
        vec2(0.0), vec2(blur, 0.0), vec2(-blur, 0.0),
        vec2(0.0, blur), vec2(0.0, -blur)
    );
    vec3 color = vec3(0.0);
    for (int i = 0; i < 5; i++) {
        vec2 q = p + offsets[i];
        color += vec3(
            sample_backdrop(q + dispersion).r,
            sample_backdrop(q).g,
            sample_backdrop(q - dispersion).b
        );
    }
    return color / 5.0;
}

vec2 rounded_normal(vec2 p, vec2 half_size, float radius) {
    vec2 dx = vec2(1.0, 0.0);
    vec2 dy = vec2(0.0, 1.0);
    float gx = rr_sdf(p + dx, half_size, radius) - rr_sdf(p - dx, half_size, radius);
    float gy = rr_sdf(p + dy, half_size, radius) - rr_sdf(p - dy, half_size, radius);
    return normalize(vec2(gx, gy) + vec2(1e-6));
}

float corner_weight(vec2 local, vec2 half_size, float radius) {
    vec2 corner = clamp((abs(local) - (half_size - radius)) / max(radius, 1.0), vec2(0.0), vec2(1.0));
    return mix(0.60, 1.0, corner.x * corner.y);
}

struct Optics {
    float refraction;
    float dispersion;
    float blur;
    float edge_width;
    float edge_opacity;
};

struct ControlHit {
    uint index;
    vec2 local;
    vec2 half_size;
    float radius;
    float distance;
};

ControlHit pick_control(vec2 p) {
    uint count = uint(min(max(u_count.x, 0), 16));
    ControlHit hit = ControlHit(count, vec2(0.0), vec2(1.0), 0.0, 0.0);
    for (uint index = 0u; index < count; index++) {
        vec4 ctrl = u_ctl_shape[index];
        if (ctrl.y < 0.5) continue;
        vec4 rect = u_ctl_rect[index];
        vec4 spring = u_ctl_spring[index];
        vec2 half_size = max(rect.zw * (0.5 * spring.z) - vec2(0.5), vec2(1.0));
        float radius = clamp(ctrl.x * spring.z - 0.5, 0.0, min(half_size.x, half_size.y));
        vec2 local = p - rect.xy - rect.zw * 0.5 - spring.xy;
        float dist = rr_sdf(local, half_size, radius);
        if (dist < 0.0) {
            hit = ControlHit(index, local, half_size, radius, dist);
        }
    }
    return hit;
}

vec4 control_glass(vec2 p, ControlHit hit, float mat, vec2 light, vec3 edge_bg, Optics optics) {
    vec2 local = hit.local;
    vec2 hs = hit.half_size;
    float dist = hit.distance;
    vec2 normal = rounded_normal(local, hs, hit.radius);
    float sshort = max(min(hs.x, hs.y), 1.0);
    float band = clamp(sshort * 1.30, 10.0, 72.0);
    float t = clamp(-dist / band, 0.0, 1.0);
    float k = pow(1.0 - pow(t, 0.30), 2.0);
    vec2 size = u_ctl_rect[hit.index].zw / max(u_env.z, 1e-4);
    uint role = uint(u_ctl_spring[hit.index].w);
    float lens = 1.12;
    if (role == ROLE_ISLAND_LYRICS) {
        lens = 1.06;
    } else if (role == ROLE_SEEK) {
        lens = 1.0 + 0.9 * u_ctl_shape[hit.index].w;
    }
    float short_side = min(size.x, size.y) / (role == ROLE_SEEK ? lens : 1.0);
    float refract = 0.55 * clamp(short_side / 70.0, 0.30, 1.0);
    float chroma = optics.dispersion * refract * corner_weight(local, hs, hit.radius);
    vec2 bp = p - local * (1.0 - 1.0 / lens) - normal * k * optics.refraction * refract;

    vec3 col = tap5(bp, -normal * k * chroma, optics.blur * 0.80 + k * 1.2);
    float lum = dot(col, LUMA);
    col *= 1.0 + mix(0.045, -0.035, smoothstep(0.42, 0.88, lum));
    col = mix(col, vec3(0.30), smoothstep(0.48, 0.96, dot(edge_bg, LUMA)) * 0.16 * mat);
    col = col * 1.010 + vec3(0.004, 0.006, 0.010);
    col = mix(col, col * 0.58 + vec3(0.010, 0.014, 0.020), (1.0 - mat) * 0.70);
    float top = 1.0 - smoothstep(0.0, max(hs.y * 2.0, 1.0), local.y + hs.y);
    col += vec3(0.06, 0.08, 0.11) * top * 0.16;

    float tq = (dist + 1.7) / 0.80;
    float bevel = exp(-tq * tq);
    col *= 1.0 - bevel * 0.10;
    float bleed = exp(-max(-dist, 0.0) / clamp(sshort * 0.45, 5.0, 22.0)) * (1.0 - bevel * 0.5);
    float lobe = pow(max(dot(normal, light), 0.0), 1.5);
    col += vec3(0.62, 0.86, 0.96) * bleed * (lobe * 0.15 + 0.055);

    float sigma = max(optics.edge_width, 0.5) * 0.42;
    float lq = (dist + sigma * 0.65) / sigma;
    float line = exp(-lq * lq) * mix(0.55, 1.0, pow(max(dot(normal, light), 0.0), 1.3));
    float edge = clamp(line * optics.edge_opacity, 0.0, 1.0);
    col = mix(col, vec3(1.0), edge);
    float aa = 1.0 - smoothstep(-1.0, 1.0, dist);
    return vec4(col, max(aa, edge));
}

vec4 material(vec2 p) {
    float dist = body_sdf(p);
    if (dist > 2.75 || body_sdf_exact(p) > 2.0) { return vec4(0.0); }
    vec3 edge_bg = sample_backdrop(p);

    float ppp = u_env.z;
    Optics optics = Optics(param(0u) * ppp, param(1u) * ppp, param(2u) * ppp, param(4u) * ppp, param(5u));
    float mat = clamp(u_motion.y, 0.0, 1.0);
    vec2 center = u_body.xy + u_body.zw * 0.5;
    vec2 local = p - center;
    vec2 half_size = max(u_body.zw * 0.5 - vec2(3.0), vec2(2.0));
    float radius = clamp(u_shape.x - 3.0, 0.0, min(half_size.x, half_size.y));
    vec2 normal = rounded_normal(local, half_size, radius);
    bool no_ptr = abs(u_shape.z + 1.0) < 0.01 && abs(u_shape.w + 1.0) < 0.01;
    vec2 to_pointer = no_ptr ? vec2(0.0) : (u_shape.zw - center);
    float pointer_distance = length(to_pointer);
    vec2 swung = to_pointer / max(pointer_distance, 1e-3);
    vec2 light = normalize(mix(
        normalize(vec2(-0.45, -0.65)),
        vec2(swung.x, min(swung.y, -0.18)),
        smoothstep(3.0, 48.0, pointer_distance)
    ) + vec2(1e-6));

    float morph = max(param(6u), 0.0) * clamp(u_motion.z, 0.0, 1.0);
    float band = max(param(3u) * ppp, 1.0) * (1.0 + morph * 0.08);
    float t = clamp(-dist / band, 0.0, 1.0);
    float k = pow(1.0 - pow(t, 0.30), 2.0);
    float strength = optics.refraction * (1.0 + morph * 0.15);
    float chroma = optics.dispersion * corner_weight(local, half_size, radius);
    vec3 col = tap5(p - normal * k * strength, -normal * k * chroma, optics.blur + k * 1.6);
    col = col * 1.004 + vec3(0.002, 0.003, 0.006);
    float top = 1.0 - smoothstep(0.0, max(u_body.w, 1.0), p.y - u_body.y);
    col += vec3(0.05, 0.07, 0.10) * top * 0.10;

    float lum = dot(col, LUMA);
    col *= 1.0 + mix(0.045, -0.035, smoothstep(0.42, 0.88, lum));
    col = mix(col, col * 0.58 + vec3(0.010, 0.014, 0.020), (1.0 - mat) * 0.70);
    col = mix(col, vec3(0.96, 0.97, 1.0), mat * 0.02);
    float scrim = smoothstep(0.48, 0.96, dot(edge_bg, LUMA)) * 0.16 * mat;
    col = mix(col, vec3(0.30), scrim);
    col = mix(vec3(dot(col, LUMA)), col, 1.06);

    float tq = (dist + 2.2) / 0.9;
    float bevel = exp(-tq * tq);
    col *= 1.0 - bevel * 0.08;
    float depth = max(-dist, 0.0);
    float bleed = exp(-depth / 16.0) * (1.0 - bevel * 0.55);
    float path = exp(-depth / 7.0);
    col *= mix(vec3(1.0), vec3(0.90, 1.0, 0.96), path * 0.65);
    float absorb = 1.0 - exp(-depth / 10.0);
    vec3 tint = mix(vec3(0.88, 0.93, 1.0), vec3(0.52, 0.90, 0.80), absorb);
    float lobe_a = pow(max(dot(normal, light), 0.0), 1.6);
    float lobe_b = pow(max(dot(normal, -light), 0.0), 1.2);
    col += tint * bleed * (lobe_a * 0.16 + lobe_b * 0.085 + 0.050);
    col += NEUTRAL_TINT * 0.05 * bleed;

    float sigma = max(optics.edge_width, 0.3) * 0.365;
    float lq = (dist + sigma * 0.55) / sigma;
    float line = exp(-lq * lq) * mix(0.40, 1.0, pow(max(dot(normal, light), 0.0), 1.5));
    float gq = (dist + 1.0) / max(3.2 * optics.edge_width / 1.5, 0.8);
    float glow = exp(-gq * gq) * 0.04;
    float line_weight = optics.edge_opacity * mix(0.69, 1.02, smoothstep(0.60, 0.92, lum));
    col = mix(col, vec3(1.0), clamp(line * line_weight + glow, 0.0, 1.0));

    uint max_c = uint(min(max(u_count.x, 0), 16));
    ControlHit hit = pick_control(p);
    if (hit.index < max_c) {
        vec4 control = control_glass(p, hit, mat, light, edge_bg, optics);
        col = mix(col, control.rgb, control.a);
    }

    float aa = 1.0 - smoothstep(-1.2, 1.2, dist);
    float coverage = clamp(max(aa, line * optics.edge_opacity), 0.0, 1.0);
    float dither = (hash21(p) - 0.5) * (1.7 / 255.0);
    vec3 color = mix(edge_bg, col, coverage) + vec3(dither);
    return vec4(color * coverage, coverage);
}

void main() {
    float fade = clamp(1.0 - u_motion.w, 0.0, 1.0);
    frag_color = material(v_uv * u_capture.zw) * fade;
}
"#;

struct PlateUpload {
    revision: u64,
    width: i32,
    height: i32,
    rgba: Vec<u8>,
}

struct GlowState {
    program: Option<glow::Program>,
    vao: Option<glow::VertexArray>,
    texture: Option<glow::Texture>,
    tex_size: (i32, i32),
    uploaded_rev: Option<u64>,
    u_capture: Option<glow::UniformLocation>,
    u_body: Option<glow::UniformLocation>,
    u_shape: Option<glow::UniformLocation>,
    u_notch: Option<glow::UniformLocation>,
    u_env: Option<glow::UniformLocation>,
    u_tint: Option<glow::UniformLocation>,
    u_tex: Option<glow::UniformLocation>,
    u_motion: Option<glow::UniformLocation>,
    u_count: Option<glow::UniformLocation>,
    u_ctl_rect: Option<glow::UniformLocation>,
    u_ctl_shape: Option<glow::UniformLocation>,
    u_ctl_spring: Option<glow::UniformLocation>,
    u_params: Option<glow::UniformLocation>,
    u_backdrop: Option<glow::UniformLocation>,
}

impl GlowState {
    fn compile(gl: &glow::Context) -> Result<Self, String> {
        unsafe {
            let program = gl.create_program().map_err(|e| format!("GL 程序创建：{e}"))?;
            let vs = compile_shader(gl, glow::VERTEX_SHADER, VERT_SRC)?;
            let fs = compile_shader(gl, glow::FRAGMENT_SHADER, FRAG_SRC)?;
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            gl.detach_shader(program, vs);
            gl.detach_shader(program, fs);
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            if !gl.get_program_link_status(program) {
                let info = gl.get_program_info_log(program);
                gl.delete_program(program);
                return Err(format!("GL 着色器链接失败：{info}"));
            }
            let vao = gl
                .create_vertex_array()
                .map_err(|e| format!("GL VAO 创建：{e}"))?;
            let texture = gl
                .create_texture()
                .map_err(|e| format!("GL 纹理创建：{e}"))?;
            let prev_tex = gl.get_parameter_i32(glow::TEXTURE_BINDING_2D);
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            let restore = u32::try_from(prev_tex)
                .ok()
                .and_then(std::num::NonZeroU32::new)
                .map(glow::NativeTexture);
            gl.bind_texture(glow::TEXTURE_2D, restore);

            let loc = |name: &str| gl.get_uniform_location(program, name);
            Ok(Self {
                u_capture: loc("u_capture"),
                u_body: loc("u_body"),
                u_shape: loc("u_shape"),
                u_notch: loc("u_notch"),
                u_env: loc("u_env"),
                u_tint: loc("u_tint"),
                u_tex: loc("u_tex"),
                u_motion: loc("u_motion"),
                u_count: loc("u_count"),
                u_ctl_rect: loc("u_ctl_rect"),
                u_ctl_shape: loc("u_ctl_shape"),
                u_ctl_spring: loc("u_ctl_spring"),
                u_params: loc("u_params"),
                u_backdrop: loc("u_backdrop"),
                program: Some(program),
                vao: Some(vao),
                texture: Some(texture),
                tex_size: (0, 0),
                uploaded_rev: None,
            })
        }
    }

    fn destroy(&mut self, gl: &glow::Context) {
        unsafe {
            if let Some(tex) = self.texture.take() {
                gl.delete_texture(tex);
            }
            if let Some(vao) = self.vao.take() {
                gl.delete_vertex_array(vao);
            }
            if let Some(prog) = self.program.take() {
                gl.delete_program(prog);
            }
        }
        self.uploaded_rev = None;
        self.tex_size = (0, 0);
    }
}

unsafe fn compile_shader(
    gl: &glow::Context,
    kind: u32,
    src: &str,
) -> Result<glow::Shader, String> {
    let shader = gl
        .create_shader(kind)
        .map_err(|e| format!("GL 着色器创建：{e}"))?;
    gl.shader_source(shader, src);
    gl.compile_shader(shader);
    if !gl.get_shader_compile_status(shader) {
        let info = gl.get_shader_info_log(shader);
        gl.delete_shader(shader);
        return Err(format!("GL 着色器编译失败：{info}"));
    }
    Ok(shader)
}

pub(super) struct GlowPipeline {
    gl: Arc<glow::Context>,
    state: Arc<Mutex<GlowState>>,
    cached_plate: Option<Arc<PlateUpload>>,
}

impl GlowPipeline {
    pub(super) fn new(gl: Arc<glow::Context>) -> Result<Self, String> {
        let state = GlowState::compile(&gl)?;
        Ok(Self {
            gl,
            state: Arc::new(Mutex::new(state)),
            cached_plate: None,
        })
    }

    pub(super) fn clear_backdrop(&mut self) {
        self.cached_plate = None;
    }

    pub(super) fn destroy(&mut self, gl: &glow::Context) {
        if let Ok(mut guard) = self.state.lock() {
            guard.destroy(gl);
        }
        self.cached_plate = None;
    }

    pub(super) fn callback(
        &mut self,
        rect: egui::Rect,
        mut frame: pipeline::Frame,
        plate: &CleanPlate,
        dock: Dock,
    ) -> Result<egui::Shape, String> {
        let screen = plate.geometry.screen;
        if screen.width <= 0 || screen.height <= 0 {
            return Err("底片尺寸无效".into());
        }
        let upload = if let Some(cached) = &self.cached_plate {
            if cached.revision == plate.revision
                && cached.width == screen.width
                && cached.height == screen.height
            {
                Arc::clone(cached)
            } else {
                let u = build_upload(plate);
                self.cached_plate = Some(Arc::clone(&u));
                u
            }
        } else {
            let u = build_upload(plate);
            self.cached_plate = Some(Arc::clone(&u));
            u
        };

        let [w, h] = [screen.width as f32, screen.height as f32];
        frame.tex = [w, h, w, h];
        frame.count[1] = pipeline::dock_code(dock);

        let state_arc = Arc::clone(&self.state);
        let cb = egui_glow::CallbackFn::new(move |info, painter| {
            let gl = painter.gl();
            let Ok(mut st) = state_arc.lock() else { return };
            let (Some(program), Some(vao), Some(texture)) = (st.program, st.vao, st.texture)
            else {
                return;
            };
            unsafe {
                let prev_prog = gl.get_parameter_i32(glow::CURRENT_PROGRAM);
                let prev_vao = gl.get_parameter_i32(glow::VERTEX_ARRAY_BINDING);
                let prev_active = gl.get_parameter_i32(glow::ACTIVE_TEXTURE);
                let prev_tex = gl.get_parameter_i32(glow::TEXTURE_BINDING_2D);
                let prev_blend = gl.is_enabled(glow::BLEND);
                let prev_src_rgb = gl.get_parameter_i32(glow::BLEND_SRC_RGB);
                let prev_dst_rgb = gl.get_parameter_i32(glow::BLEND_DST_RGB);
                let prev_src_a = gl.get_parameter_i32(glow::BLEND_SRC_ALPHA);
                let prev_dst_a = gl.get_parameter_i32(glow::BLEND_DST_ALPHA);
                let prev_scissor = gl.is_enabled(glow::SCISSOR_TEST);
                let mut prev_scissor_box = [0i32; 4];
                gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut prev_scissor_box);

                let clip = info.clip_rect_in_pixels();
                gl.enable(glow::SCISSOR_TEST);
                gl.scissor(
                    clip.left_px,
                    clip.from_bottom_px,
                    clip.width_px.max(0),
                    clip.height_px.max(0),
                );
                gl.enable(glow::BLEND);
                gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);

                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                if st.uploaded_rev != Some(upload.revision)
                    || st.tex_size != (upload.width, upload.height)
                {
                    let prev_align = gl.get_parameter_i32(glow::UNPACK_ALIGNMENT);
                    let prev_row = gl.get_parameter_i32(glow::UNPACK_ROW_LENGTH);
                    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
                    gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
                    if st.tex_size != (upload.width, upload.height) {
                        gl.tex_image_2d(
                            glow::TEXTURE_2D,
                            0,
                            glow::RGBA8 as i32,
                            upload.width,
                            upload.height,
                            0,
                            glow::RGBA,
                            glow::UNSIGNED_BYTE,
                            glow::PixelUnpackData::Slice(Some(&upload.rgba)),
                        );
                        st.tex_size = (upload.width, upload.height);
                    } else {
                        gl.tex_sub_image_2d(
                            glow::TEXTURE_2D,
                            0,
                            0,
                            0,
                            upload.width,
                            upload.height,
                            glow::RGBA,
                            glow::UNSIGNED_BYTE,
                            glow::PixelUnpackData::Slice(Some(&upload.rgba)),
                        );
                    }
                    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, prev_align);
                    gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, prev_row);
                    st.uploaded_rev = Some(upload.revision);
                }

                gl.use_program(Some(program));
                gl.bind_vertex_array(Some(vao));
                gl.uniform_1_i32(st.u_backdrop.as_ref(), 0);
                gl.uniform_4_f32_slice(st.u_capture.as_ref(), &frame.capture);
                gl.uniform_4_f32_slice(st.u_body.as_ref(), &frame.body);
                gl.uniform_4_f32_slice(st.u_shape.as_ref(), &frame.shape);
                gl.uniform_4_f32_slice(st.u_notch.as_ref(), &frame.notch);
                gl.uniform_4_f32_slice(st.u_env.as_ref(), &frame.env);
                gl.uniform_4_f32_slice(st.u_tint.as_ref(), &frame.tint);
                gl.uniform_4_f32_slice(st.u_tex.as_ref(), &frame.tex);
                gl.uniform_4_f32_slice(st.u_motion.as_ref(), &frame.motion);
                gl.uniform_4_i32(
                    st.u_count.as_ref(),
                    frame.count[0] as i32,
                    frame.count[1] as i32,
                    0,
                    0,
                );
                gl.uniform_4_f32_slice(st.u_ctl_rect.as_ref(), flatten16(&frame.ctl_rect));
                gl.uniform_4_f32_slice(st.u_ctl_shape.as_ref(), flatten16(&frame.ctl_shape));
                gl.uniform_4_f32_slice(st.u_ctl_spring.as_ref(), flatten16(&frame.ctl_spring));
                gl.uniform_4_f32_slice(st.u_params.as_ref(), flatten8(&frame.params));
                gl.draw_arrays(glow::TRIANGLES, 0, 3);

                gl.bind_vertex_array(
                    u32::try_from(prev_vao)
                        .ok()
                        .and_then(std::num::NonZeroU32::new)
                        .map(glow::NativeVertexArray),
                );
                gl.use_program(
                    u32::try_from(prev_prog)
                        .ok()
                        .and_then(std::num::NonZeroU32::new)
                        .map(glow::NativeProgram),
                );
                gl.bind_texture(
                    glow::TEXTURE_2D,
                    u32::try_from(prev_tex)
                        .ok()
                        .and_then(std::num::NonZeroU32::new)
                        .map(glow::NativeTexture),
                );
                gl.active_texture(prev_active as u32);
                gl.blend_func_separate(
                    prev_src_rgb as u32,
                    prev_dst_rgb as u32,
                    prev_src_a as u32,
                    prev_dst_a as u32,
                );
                if !prev_blend {
                    gl.disable(glow::BLEND);
                }
                gl.scissor(
                    prev_scissor_box[0],
                    prev_scissor_box[1],
                    prev_scissor_box[2],
                    prev_scissor_box[3],
                );
                if !prev_scissor {
                    gl.disable(glow::SCISSOR_TEST);
                }
            }
        });
        Ok(egui::Shape::Callback(PaintCallback {
            rect,
            callback: Arc::new(cb),
        }))
    }
}

impl Drop for GlowPipeline {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.state.lock() {
            guard.destroy(&self.gl);
        }
    }
}

fn flatten16(arr: &[[f32; 4]; 16]) -> &[f32] {
    unsafe { std::slice::from_raw_parts(arr.as_ptr().cast::<f32>(), 16 * 4) }
}

fn flatten8(arr: &[[f32; 4]; 8]) -> &[f32] {
    unsafe { std::slice::from_raw_parts(arr.as_ptr().cast::<f32>(), 8 * 4) }
}

fn build_upload(plate: &CleanPlate) -> Arc<PlateUpload> {
    let mut rgba = Vec::with_capacity(plate.bgra.len());
    for px in plate.bgra.chunks_exact(4) {
        rgba.push(px[2]);
        rgba.push(px[1]);
        rgba.push(px[0]);
        rgba.push(255);
    }
    Arc::new(PlateUpload {
        revision: plate.revision,
        width: plate.geometry.screen.width,
        height: plate.geometry.screen.height,
        rgba,
    })
}
