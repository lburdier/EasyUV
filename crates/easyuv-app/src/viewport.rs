//! Vue 3D : rendu logiciel (algorithme du peintre) dans un `egui::Mesh` texturé par le damier.

use crate::scene::Scene;
use crate::theme;
use eframe::egui::{self, epaint::Vertex, Color32, Pos2, Rect, Shape, Stroke, TextureId};
use easyuv::math::V3;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shading {
    Solid,
    Checker,
    Islands,
    Stretch,
}

pub struct ViewOptions {
    pub shading: Shading,
    pub wire: bool,
    pub seams: bool,
    pub grid: bool,
    /// Répétition du damier (1 = le damier couvre la tuile 0-1).
    pub tiling: f32,
}

pub struct Camera {
    pub target: V3,
    pub yaw: f64,
    pub pitch: f64,
    pub dist: f64,
    pub fov: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { target: V3::ZERO, yaw: -0.9, pitch: 0.45, dist: 6.0, fov: 50f64.to_radians() }
    }
}

struct Basis {
    eye: V3,
    right: V3,
    up: V3,
    fwd: V3,
}

impl Camera {
    fn basis(&self) -> Basis {
        let dir = V3::new(self.pitch.cos() * self.yaw.cos(), self.pitch.cos() * self.yaw.sin(), self.pitch.sin());
        let eye = self.target + dir * self.dist;
        let fwd = (self.target - eye).normalized();
        let right = fwd.cross(V3::new(0.0, 0.0, 1.0)).normalized();
        let up = right.cross(fwd);
        Basis { eye, right, up, fwd }
    }

    pub fn frame(&mut self, lo: V3, hi: V3) {
        self.target = (lo + hi) * 0.5;
        let r = ((hi - lo).length() * 0.5).max(1e-3);
        self.dist = r / (self.fov * 0.5).sin() * 1.15;
    }
}

/// Taille de la texture damier affichée (les 2x2 texels du coin sont forcés en blanc).
pub const TEX_SIZE: usize = 1024;
/// UV d'un texel blanc pur (coin du damier) pour les éléments non texturés.
pub const WHITE_UV: Pos2 = Pos2 { x: 1.0 / TEX_SIZE as f32, y: 1.0 / TEX_SIZE as f32 };

pub fn island_color(i: u32) -> Color32 {
    let h = (i as f32 * 0.618_034).fract();
    let (r, g, b) = hsv(h, 0.45, 0.92);
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn hsv(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h6 = h.fract() * 6.0;
    let f = h6 - h6.floor();
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match h6 as i32 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

/// Palette d'étirement façon Blender : bleu (aucun) -> vert -> jaune -> rouge (fort).
pub fn stretch_color(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let stops = [(0.0, [0x1f, 0x3c, 0xd6]), (0.33, [0x1f, 0xc8, 0x5a]), (0.66, [0xf0, 0xd2, 0x1e]), (1.0, [0xe6, 0x28, 0x1e])];
    for w in stops.windows(2) {
        let (a, ca) = w[0];
        let (b, cb) = w[1];
        if t <= b {
            let k = (t - a) / (b - a);
            let m = |i: usize| (ca[i] as f32 + (cb[i] as f32 - ca[i] as f32) * k) as u8;
            return Color32::from_rgb(m(0), m(1), m(2));
        }
    }
    Color32::from_rgb(0xe6, 0x28, 0x1e)
}

fn shade(c: Color32, k: f32) -> Color32 {
    let m = |v: u8| ((v as f32) * k).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(m(c.r()), m(c.g()), m(c.b()), c.a())
}

fn push_line(mesh: &mut egui::Mesh, a: Pos2, b: Pos2, width: f32, color: Color32) {
    let d = b - a;
    let len = d.length();
    if len < 1e-3 {
        return;
    }
    let n = egui::vec2(-d.y, d.x) / len * (width * 0.5);
    let i = mesh.vertices.len() as u32;
    for p in [a + n, b + n, b - n, a - n] {
        mesh.vertices.push(Vertex { pos: p, uv: WHITE_UV, color });
    }
    mesh.indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
}

pub fn handle_input(ui: &egui::Ui, resp: &egui::Response, cam: &mut Camera, rect: Rect) {
    let (shift, scroll, mmb, lmb, rmb) = ui.input(|i| {
        (
            i.modifiers.shift,
            i.smooth_scroll_delta.y,
            i.pointer.middle_down(),
            i.pointer.primary_down(),
            i.pointer.secondary_down(),
        )
    });
    if resp.dragged() && (mmb || lmb || rmb) {
        let d = resp.drag_delta();
        if shift || rmb {
            let b = cam.basis();
            let k = cam.dist * (cam.fov * 0.5).tan() * 2.0 / rect.height().max(1.0) as f64;
            cam.target = cam.target - b.right * (d.x as f64 * k) + b.up * (d.y as f64 * k);
        } else {
            cam.yaw -= d.x as f64 * 0.008;
            cam.pitch = (cam.pitch + d.y as f64 * 0.008).clamp(-1.55, 1.55);
        }
    }
    if resp.hovered() && scroll != 0.0 {
        cam.dist = (cam.dist * (-(scroll as f64) * 0.0018).exp()).max(1e-3);
    }
}

pub fn draw(ui: &mut egui::Ui, rect: Rect, scene: Option<&Scene>, cam: &Camera, opt: &ViewOptions, tex: TextureId) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, theme::BG_VIEWPORT);
    let b = cam.basis();
    let f = (rect.height() as f64 * 0.5) / (cam.fov * 0.5).tan();
    let c = rect.center();
    let near = cam.dist * 0.01;
    let project = |p: V3| -> (Pos2, f64) {
        let d = p - b.eye;
        let z = d.dot(b.fwd);
        let zz = z.max(near);
        (Pos2::new(c.x + (d.dot(b.right) / zz * f) as f32, c.y - (d.dot(b.up) / zz * f) as f32), z)
    };

    // Grille du sol (plan XY) + axes X / Y
    if opt.grid {
        let size = scene.map(|s| s.radius).unwrap_or(2.0).max(0.1);
        let step = 10f64.powf((size * 0.5).log10().floor());
        let n = ((size * 4.0) / step).ceil() as i32;
        let ext = n as f64 * step;
        let seg = |a: V3, bb: V3, col: Color32, w: f32| {
            // découpage contre le plan proche
            let (da, db) = ((a - b.eye).dot(b.fwd), (bb - b.eye).dot(b.fwd));
            if da < near && db < near {
                return;
            }
            let (mut a, mut bb) = (a, bb);
            if da < near {
                a = a + (bb - a) * ((near - da) / (db - da));
            } else if db < near {
                bb = bb + (a - bb) * ((near - db) / (da - db));
            }
            painter.line_segment([project(a).0, project(bb).0], Stroke::new(w, col));
        };
        for i in -n..=n {
            let t = i as f64 * step;
            if i == 0 {
                continue;
            }
            seg(V3::new(t, -ext, 0.0), V3::new(t, ext, 0.0), theme::GRID, 1.0);
            seg(V3::new(-ext, t, 0.0), V3::new(ext, t, 0.0), theme::GRID, 1.0);
        }
        seg(V3::new(-ext, 0.0, 0.0), V3::new(ext, 0.0, 0.0), theme::AXIS_X.gamma_multiply(0.8), 1.5);
        seg(V3::new(0.0, -ext, 0.0), V3::new(0.0, ext, 0.0), theme::AXIS_Y.gamma_multiply(0.8), 1.5);
    }

    if let Some(scene) = scene {
        let m = &scene.model.mesh;
        let proj: Vec<(Pos2, f64)> = m.positions.iter().map(|&p| project(p)).collect();
        let nf = m.face_count();
        let mut order: Vec<(f64, u32)> = Vec::with_capacity(nf);
        for fi in 0..nf {
            let vs = m.face(fi);
            let mut z = 0.0;
            let mut behind = false;
            for &v in vs {
                let pz = proj[v as usize].1;
                behind |= pz < near;
                z += pz;
            }
            if !behind {
                order.push((z / vs.len() as f64, fi as u32));
            }
        }
        order.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

        let has_uv = scene.uv.is_some();
        let wire = opt.wire && nf < 60_000;
        let mut mesh = egui::Mesh::with_texture(tex);
        mesh.vertices.reserve(order.len() * 4);
        for &(_, fi) in &order {
            let fi = fi as usize;
            let r = m.face_range(fi);
            let n = scene.face_normals[fi];
            let to_eye = (b.eye - m.face_centroid(fi)).normalized();
            let ndv = n.dot(to_eye);
            let front = ndv >= 0.0;
            let mut k = 0.28 + 0.72 * ndv.abs() as f32;
            if !front {
                k *= 0.5;
            }
            let (base, textured) = match opt.shading {
                Shading::Checker if has_uv => (Color32::WHITE, true),
                Shading::Islands if has_uv => (island_color(scene.uv.as_ref().unwrap().face_chart[fi]), false),
                Shading::Stretch if has_uv => (stretch_color(scene.face_stretch[fi]), false),
                _ => (Color32::from_gray(204), false),
            };
            let col = shade(base, k);
            let i0 = mesh.vertices.len() as u32;
            for ci in r.clone() {
                let v = m.corners[ci] as usize;
                let uv = if textured {
                    let t = scene.uv.as_ref().unwrap().corner_uv[ci];
                    Pos2::new(t[0] * opt.tiling, 1.0 - t[1] * opt.tiling)
                } else {
                    WHITE_UV
                };
                mesh.vertices.push(Vertex { pos: proj[v].0, uv, color: col });
            }
            for t in m.fan(fi) {
                mesh.indices.extend_from_slice(&[i0 + t[0] as u32, i0 + t[1] as u32, i0 + t[2] as u32]);
            }
            if front && (wire || opt.seams) {
                let vs = m.face(fi);
                for i in 0..vs.len() {
                    let (a, bb) = (vs[i], vs[(i + 1) % vs.len()]);
                    let key = if a < bb { (a, bb) } else { (bb, a) };
                    let seam = opt.seams && scene.seam_set.contains(&key);
                    if seam {
                        push_line(&mut mesh, proj[a as usize].0, proj[bb as usize].0, 2.2, theme::SEAM_RED);
                    } else if wire {
                        push_line(&mut mesh, proj[a as usize].0, proj[bb as usize].0, 1.0, Color32::from_black_alpha(110));
                    }
                }
            }
        }
        painter.add(Shape::mesh(mesh));
    }

    draw_gizmo(&painter, rect, &b);
}

/// Gizmo d'orientation en haut à droite, comme dans Blender.
fn draw_gizmo(painter: &egui::Painter, rect: Rect, b: &Basis) {
    let c = Pos2::new(rect.right() - 52.0, rect.top() + 62.0);
    let r = 36.0f64;
    painter.circle_filled(c, r as f32 + 10.0, Color32::from_white_alpha(8));
    let axes = [
        (V3::new(1.0, 0.0, 0.0), theme::AXIS_X, "X"),
        (V3::new(0.0, 1.0, 0.0), theme::AXIS_Y, "Y"),
        (V3::new(0.0, 0.0, 1.0), theme::AXIS_Z, "Z"),
    ];
    let mut items: Vec<(f64, Pos2, Color32, Option<&str>)> = Vec::new();
    for (a, col, name) in axes {
        for sign in [1.0, -1.0] {
            let v = a * sign;
            let p = Pos2::new(c.x + (v.dot(b.right) * r) as f32, c.y - (v.dot(b.up) * r) as f32);
            items.push((v.dot(b.fwd), p, col, if sign > 0.0 { Some(name) } else { None }));
        }
    }
    items.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for (_, p, col, name) in items {
        if let Some(name) = name {
            painter.line_segment([c, p], Stroke::new(2.0, col));
            painter.circle_filled(p, 9.0, col);
            painter.text(p, egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(11.0), Color32::from_gray(20));
        } else {
            painter.circle_filled(p, 7.5, col.gamma_multiply(0.35));
            painter.circle_stroke(p, 7.5, Stroke::new(1.0, col.gamma_multiply(0.8)));
        }
    }
}
