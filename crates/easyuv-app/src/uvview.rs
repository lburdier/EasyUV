//! Éditeur UV 2D : tuile 0-1 avec damier, îlots colorés et arêtes.

use crate::scene::Scene;
use crate::theme;
use crate::viewport::{island_color, stretch_color, Shading, WHITE_UV};
use eframe::egui::{self, epaint::Vertex, Color32, Pos2, Rect, Shape, Stroke, TextureId};

pub struct UvCamera {
    pub center: egui::Vec2,
    pub zoom: f32,
}

impl Default for UvCamera {
    fn default() -> Self {
        UvCamera { center: egui::vec2(0.5, 0.5), zoom: 0.88 }
    }
}

pub fn handle_input(ui: &egui::Ui, resp: &egui::Response, cam: &mut UvCamera, rect: Rect) {
    let side = rect.width().min(rect.height());
    if resp.dragged() {
        let d = resp.drag_delta() / (side * cam.zoom);
        cam.center -= egui::vec2(d.x, -d.y);
    }
    if resp.hovered() {
        let s = ui.input(|i| i.smooth_scroll_delta.y);
        if s != 0.0 {
            cam.zoom = (cam.zoom * (s * 0.0018).exp()).clamp(0.05, 200.0);
        }
    }
}

pub fn draw(ui: &mut egui::Ui, rect: Rect, scene: Option<&Scene>, cam: &UvCamera, shading: Shading, tex: TextureId, tiling: f32) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, theme::BG_UV);
    let side = rect.width().min(rect.height()) * cam.zoom;
    let c = rect.center();
    let to_screen = |u: f32, v: f32| Pos2::new(c.x + (u - cam.center.x) * side, c.y - (v - cam.center.y) * side);

    // Tuile 0-1 texturée
    let tile = Rect::from_min_max(to_screen(0.0, 1.0), to_screen(1.0, 0.0));
    let tint = if matches!(shading, Shading::Checker) { Color32::from_gray(150) } else { Color32::from_gray(70) };
    painter.image(tex, tile, Rect::from_min_max(Pos2::ZERO, Pos2::new(tiling, tiling)), tint);
    painter.rect_stroke(tile, 0.0, Stroke::new(1.0, Color32::from_gray(110)), egui::StrokeKind::Outside);
    // Graduations 0.1
    for i in 1..10 {
        let t = i as f32 / 10.0;
        let col = Color32::from_white_alpha(if i == 5 { 28 } else { 12 });
        painter.line_segment([to_screen(t, 0.0), to_screen(t, 1.0)], Stroke::new(1.0, col));
        painter.line_segment([to_screen(0.0, t), to_screen(1.0, t)], Stroke::new(1.0, col));
    }

    let Some(scene) = scene else {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Aucun objet", egui::FontId::proportional(14.0), theme::TEXT_DIM);
        return;
    };
    let Some(uv) = &scene.uv else {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Pas encore d'UV — appuyez sur U", egui::FontId::proportional(14.0), theme::TEXT_DIM);
        return;
    };
    let m = &scene.model.mesh;
    let mut fill = egui::Mesh::with_texture(tex);
    let mut lines = egui::Mesh::with_texture(tex);
    let nf = m.face_count();
    let draw_edges = nf < 150_000;
    for f in 0..nf {
        let r = m.face_range(f);
        let col = match shading {
            Shading::Stretch => stretch_color(scene.face_stretch[f]).gamma_multiply(0.75),
            _ => island_color(uv.face_chart[f]).gamma_multiply(0.30),
        };
        let i0 = fill.vertices.len() as u32;
        for ci in r.clone() {
            let t = uv.corner_uv[ci];
            fill.vertices.push(Vertex { pos: to_screen(t[0], t[1]), uv: WHITE_UV, color: col });
        }
        for t in m.fan(f) {
            fill.indices.extend_from_slice(&[i0 + t[0] as u32, i0 + t[1] as u32, i0 + t[2] as u32]);
        }
        if draw_edges {
            let n = r.len();
            for i in 0..n {
                let a = uv.corner_uv[r.start + i];
                let b = uv.corner_uv[r.start + (i + 1) % n];
                let (pa, pb) = (to_screen(a[0], a[1]), to_screen(b[0], b[1]));
                let d = pb - pa;
                let len = d.length();
                if len < 0.3 {
                    continue;
                }
                let nrm = egui::vec2(-d.y, d.x) / len * 0.5;
                let k = lines.vertices.len() as u32;
                let lc = Color32::from_white_alpha(150);
                for p in [pa + nrm, pb + nrm, pb - nrm, pa - nrm] {
                    lines.vertices.push(Vertex { pos: p, uv: WHITE_UV, color: lc });
                }
                lines.indices.extend_from_slice(&[k, k + 1, k + 2, k, k + 2, k + 3]);
            }
        }
    }
    painter.add(Shape::mesh(fill));
    painter.add(Shape::mesh(lines));
}
