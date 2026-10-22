//! Éditeur UV 2D : tuile 0-1 avec damier, îlots colorés et arêtes, et édition des îlots
//! à la manière de Blender (clic, rectangle, G / R / S, X / Y, Ctrl, Échap).

use crate::scene::Scene;
use crate::theme;
use crate::viewport::{island_color, stretch_color, Shading, WHITE_UV};
use eframe::egui::{self, epaint::Vertex, Color32, Pos2, Rect, Shape, Stroke, TextureId};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub struct UvCamera {
    pub center: egui::Vec2,
    pub zoom: f32,
}

impl Default for UvCamera {
    fn default() -> Self {
        UvCamera { center: egui::vec2(0.5, 0.5), zoom: 0.88 }
    }
}

/// Conversion écran <-> UV pour une zone et une caméra.
#[derive(Clone, Copy)]
struct View {
    c: Pos2,
    side: f32,
    center: egui::Vec2,
}

impl View {
    fn new(rect: Rect, cam: &UvCamera) -> Self {
        View { c: rect.center(), side: rect.width().min(rect.height()) * cam.zoom, center: cam.center }
    }
    fn to_screen(self, u: f32, v: f32) -> Pos2 {
        Pos2::new(self.c.x + (u - self.center.x) * self.side, self.c.y - (v - self.center.y) * self.side)
    }
    fn to_uv(self, p: Pos2) -> [f32; 2] {
        [(p.x - self.c.x) / self.side + self.center.x, -(p.y - self.c.y) / self.side + self.center.y]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Grab,
    Rotate,
    Scale,
}

/// Transformation en cours (G / R / S au clavier, ou glisser un îlot).
struct Modal {
    kind: Kind,
    /// UV de tous les coins au début (pour annuler et pour l'annulation Ctrl+Z).
    orig: Vec<[f32; 2]>,
    corners: Vec<u32>,
    start: Pos2,
    pivot: [f32; 2],
    /// 0 = libre, 1 = X seul, 2 = Y seul.
    axis: u8,
    /// Lancée en glissant à la souris : se valide au relâchement.
    drag: bool,
    info: String,
}

/// État de l'outil d'édition UV.
#[derive(Default)]
pub struct UvEditor {
    modal: Option<Modal>,
    /// Sélection au rectangle : (origine écran, mode 0 / 1 / -1).
    boxsel: Option<(Pos2, i32)>,
    /// Ignore le clic qui termine une transformation (il ne doit pas resélectionner).
    swallow_primary: bool,
    swallow_secondary: bool,
}

/// Ce que l'application doit faire après l'interaction.
pub enum UvEvent {
    /// Transformation validée : UV d'avant, pour l'historique d'annulation.
    Commit(Vec<[f32; 2]>),
    Status(String),
    /// Maj+W : rendre droits les îlots sélectionnés.
    Straighten,
}

impl UvEditor {
    pub fn busy(&self) -> bool {
        self.modal.is_some()
    }

    /// Texte d'aide de la transformation en cours.
    pub fn info(&self) -> Option<&str> {
        self.modal.as_ref().map(|m| m.info.as_str())
    }

    fn begin(&mut self, kind: Kind, scene: &Scene, start: Pos2, drag: bool) -> bool {
        let corners = scene.selected_corners();
        let (Some((lo, hi)), Some(uv)) = (scene.corners_bounds(&corners), scene.uv.as_ref()) else { return false };
        self.modal = Some(Modal {
            kind,
            orig: uv.corner_uv.clone(),
            corners,
            start,
            pivot: [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5],
            axis: 0,
            drag,
            info: String::new(),
        });
        true
    }
}

/// Molette : zoom centré sur le pointeur. Clic molette ou clic droit glissé : déplacer la vue.
fn camera_input(ui: &egui::Ui, resp: &egui::Response, cam: &mut UvCamera, rect: Rect) {
    let side = rect.width().min(rect.height());
    if resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) {
        let d = resp.drag_delta() / (side * cam.zoom);
        cam.center -= egui::vec2(d.x, -d.y);
    }
    if resp.hovered() {
        let s = ui.input(|i| i.smooth_scroll_delta.y);
        if s != 0.0 {
            let before = ui.input(|i| i.pointer.hover_pos()).map(|p| View::new(rect, cam).to_uv(p));
            cam.zoom = (cam.zoom * (s * 0.0018).exp()).clamp(0.05, 200.0);
            if let Some(b) = before {
                let after = View::new(rect, cam).to_uv(ui.input(|i| i.pointer.hover_pos()).unwrap());
                cam.center += egui::vec2(b[0] - after[0], b[1] - after[1]);
            }
        }
    }
}

/// Interaction avec l'éditeur UV : vue, sélection et transformations des îlots.
pub fn interact(ui: &egui::Ui, resp: &egui::Response, cam: &mut UvCamera, rect: Rect, scene: Option<&mut Scene>, ed: &mut UvEditor) -> Option<UvEvent> {
    camera_input(ui, resp, cam, rect);
    let view = View::new(rect, cam);
    let scene = scene?;
    scene.uv.as_ref()?;
    let (pointer, shift, ctrl, alt) = ui.input(|i| (i.pointer.latest_pos(), i.modifiers.shift, i.modifiers.command, i.modifiers.alt));
    let keys = !ui.ctx().egui_wants_keyboard_input() && (resp.hovered() || ed.modal.is_some());
    let key = |k: egui::Key| keys && ui.input(|i| i.key_pressed(k));
    let (primary_pressed, secondary_pressed, primary_released, secondary_released) = ui.input(|i| {
        (i.pointer.primary_pressed(), i.pointer.secondary_pressed(), i.pointer.primary_released(), i.pointer.button_released(egui::PointerButton::Secondary))
    });

    // --- transformation en cours ---
    if let Some(m) = &mut ed.modal {
        if key(egui::Key::X) {
            m.axis = if m.axis == 1 { 0 } else { 1 };
        }
        if key(egui::Key::Y) {
            m.axis = if m.axis == 2 { 0 } else { 2 };
        }
        let cancel = key(egui::Key::Escape) || (!m.drag && secondary_pressed);
        let confirm = key(egui::Key::Enter) || key(egui::Key::Space) || (!m.drag && primary_pressed) || (m.drag && !resp.dragged() && primary_released);
        if cancel {
            let m = ed.modal.take().unwrap();
            let orig = m.orig;
            scene.transform_corners(&orig, &m.corners, |p| p);
            ed.swallow_secondary = secondary_pressed;
            return Some(UvEvent::Status("Transformation annulée".into()));
        }
        let p = pointer.unwrap_or(m.start);
        let (a, b) = (view.to_uv(m.start), view.to_uv(p));
        let pivot = m.pivot;
        let axis = m.axis;
        let axis_txt = match axis {
            1 => "  [axe X]",
            2 => "  [axe Y]",
            _ => "",
        };
        match m.kind {
            Kind::Grab => {
                let mut d = [b[0] - a[0], b[1] - a[1]];
                if ctrl {
                    // pas de 1/64 de tuile
                    d = d.map(|x| (x * 64.0).round() / 64.0);
                }
                match axis {
                    1 => d[1] = 0.0,
                    2 => d[0] = 0.0,
                    _ => {}
                }
                m.info = format!("Déplacer  Δu {:+.4}  Δv {:+.4}{axis_txt}   ·   X / Y : axe   Ctrl : pas   Clic / Entrée : valider   Échap / clic droit : annuler", d[0], d[1]);
                scene.transform_corners(&m.orig, &m.corners, |q| [q[0] + d[0], q[1] + d[1]]);
            }
            Kind::Rotate => {
                let a0 = (a[1] - pivot[1]).atan2(a[0] - pivot[0]);
                let a1 = (b[1] - pivot[1]).atan2(b[0] - pivot[0]);
                let mut ang = (a1 - a0).to_degrees();
                if ctrl {
                    ang = (ang / 15.0).round() * 15.0;
                }
                m.info = format!("Pivoter  {ang:+.1}°   ·   Ctrl : pas de 15°   Clic / Entrée : valider   Échap / clic droit : annuler");
                let (s, c) = ang.to_radians().sin_cos();
                scene.transform_corners(&m.orig, &m.corners, |q| {
                    let (x, y) = (q[0] - pivot[0], q[1] - pivot[1]);
                    [pivot[0] + c * x - s * y, pivot[1] + s * x + c * y]
                });
            }
            Kind::Scale => {
                let d0 = ((a[0] - pivot[0]).powi(2) + (a[1] - pivot[1]).powi(2)).sqrt().max(1e-6);
                let d1 = ((b[0] - pivot[0]).powi(2) + (b[1] - pivot[1]).powi(2)).sqrt();
                let mut k = d1 / d0;
                if ctrl {
                    k = (k * 10.0).round() / 10.0;
                }
                let (kx, ky) = match axis {
                    1 => (k, 1.0),
                    2 => (1.0, k),
                    _ => (k, k),
                };
                m.info = format!("Échelle  ×{k:.3}{axis_txt}   ·   X / Y : axe   Ctrl : pas de 0,1   Clic / Entrée : valider   Échap / clic droit : annuler");
                scene.transform_corners(&m.orig, &m.corners, |q| [pivot[0] + (q[0] - pivot[0]) * kx, pivot[1] + (q[1] - pivot[1]) * ky]);
            }
        }
        if confirm {
            let m = ed.modal.take().unwrap();
            ed.swallow_primary = primary_pressed;
            return Some(UvEvent::Commit(m.orig));
        }
        ui.ctx().request_repaint();
        return None;
    }
    if primary_released && !primary_pressed && ed.swallow_primary && !resp.clicked() {
        ed.swallow_primary = false;
    }
    if secondary_released && ed.swallow_secondary && !resp.secondary_clicked() {
        ed.swallow_secondary = false;
    }

    // --- raccourcis ---
    if key(egui::Key::A) {
        scene.select_all_islands(!alt);
    }
    if ctrl && key(egui::Key::I) {
        scene.invert_islands();
    }
    if shift && key(egui::Key::W) {
        return Some(UvEvent::Straighten);
    }
    let start = pointer.filter(|p| rect.contains(*p));
    for (k, kind) in [(egui::Key::G, Kind::Grab), (egui::Key::R, Kind::Rotate), (egui::Key::S, Kind::Scale)] {
        if !ctrl && key(k) {
            if let Some(p) = start {
                if !ed.begin(kind, scene, p, false) {
                    return Some(UvEvent::Status("Sélectionnez d'abord des îlots (clic, A ou rectangle)".into()));
                }
                return None;
            }
        }
    }

    // --- souris : clic, glisser un îlot, rectangle ---
    if resp.drag_started_by(egui::PointerButton::Primary) {
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or_default();
        match scene.pick_island(view.to_uv(origin)) {
            Some(i) => {
                if !scene.island_selected[i] {
                    scene.click_island(Some(i), shift);
                }
                ed.begin(Kind::Grab, scene, origin, true);
                if let Some(m) = &mut ed.modal {
                    m.info = "Déplacer (glisser)".into();
                }
            }
            None => ed.boxsel = Some((origin, if shift { 1 } else if ctrl { -1 } else { 0 })),
        }
    }
    if let Some((o, mode)) = ed.boxsel {
        if !resp.dragged_by(egui::PointerButton::Primary) {
            ed.boxsel = None;
            if let Some(p) = pointer {
                let (a, b) = (view.to_uv(o), view.to_uv(p));
                scene.box_select_islands([a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])], mode);
            }
        }
    }
    if resp.clicked() {
        if ed.swallow_primary {
            ed.swallow_primary = false;
        } else if let Some(p) = resp.interact_pointer_pos() {
            let hit = scene.pick_island(view.to_uv(p));
            scene.click_island(hit, shift);
        }
    }
    None
}

/// Le menu contextuel ne doit pas s'ouvrir sur le clic droit qui annule une transformation.
pub fn context_menu_allowed(ed: &UvEditor) -> bool {
    ed.modal.is_none() && !ed.swallow_secondary
}

#[allow(clippy::too_many_arguments)]
pub fn draw(ui: &mut egui::Ui, rect: Rect, scene: Option<&Scene>, cam: &UvCamera, shading: Shading, tex: TextureId, tiling: f32, cache: &mut UvCache, ed: &UvEditor) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, theme::BG_UV);
    let view = View::new(rect, cam);
    let to_screen = |u: f32, v: f32| view.to_screen(u, v);

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
    // Les maillages de l'éditeur ne sont reconstruits que si la vue ou les UV changent :
    // les reconstruire à chaque image faisait saccader l'application sur les gros modèles.
    let mut hs = DefaultHasher::new();
    (scene.generation, scene as *const Scene as usize, shading as u8, rect.min.x.to_bits(), rect.min.y.to_bits(), rect.max.x.to_bits(), rect.max.y.to_bits()).hash(&mut hs);
    (cam.center.x.to_bits(), cam.center.y.to_bits(), cam.zoom.to_bits()).hash(&mut hs);
    let key = hs.finish();
    if key != cache.key || cache.fill.is_none() {
        let (fill, lines) = build_meshes(scene, uv, shading, tex, &to_screen);
        cache.fill = Some(Arc::new(fill));
        cache.lines = Some(Arc::new(lines));
        cache.key = key;
    }
    if let (Some(f), Some(l)) = (&cache.fill, &cache.lines) {
        painter.add(Shape::Mesh(f.clone()));
        painter.add(Shape::Mesh(l.clone()));
    }

    // Rectangle de sélection
    if let (Some((o, mode)), Some(p)) = (ed.boxsel, ui.input(|i| i.pointer.latest_pos())) {
        let r = Rect::from_two_pos(o, p);
        let col = if mode < 0 { theme::SEAM_RED } else { Color32::WHITE };
        painter.rect_filled(r, 0.0, Color32::from_white_alpha(10));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, col), egui::StrokeKind::Inside);
    }
    // Aide de la transformation en cours
    if let Some(info) = ed.info() {
        let pos = rect.left_bottom() + egui::vec2(10.0, -10.0);
        let galley = painter.layout_no_wrap(info.to_string(), egui::FontId::proportional(12.5), theme::TEXT);
        let bg = Rect::from_min_size(pos - egui::vec2(6.0, galley.size().y + 4.0), galley.size() + egui::vec2(12.0, 8.0));
        painter.rect_filled(bg, 4.0, Color32::from_black_alpha(170));
        painter.galley(pos - egui::vec2(0.0, galley.size().y), galley, theme::TEXT);
    }
}

/// Maillages mis en cache de l'éditeur UV.
#[derive(Default)]
pub struct UvCache {
    key: u64,
    fill: Option<Arc<egui::Mesh>>,
    lines: Option<Arc<egui::Mesh>>,
}

fn build_meshes(scene: &Scene, uv: &easyuv::UvResult, shading: Shading, tex: TextureId, to_screen: &dyn Fn(f32, f32) -> Pos2) -> (egui::Mesh, egui::Mesh) {
    let m = &scene.model.mesh;
    let mut fill = egui::Mesh::with_texture(tex);
    let mut lines = egui::Mesh::with_texture(tex);
    // arêtes des îlots sélectionnés, dessinées en dernier (par-dessus)
    let mut sel_lines = egui::Mesh::with_texture(tex);
    let nf = m.face_count();
    let draw_edges = nf < 150_000;
    let has_sel = scene.has_selection();
    let island_sel = |f: usize| scene.face_island.get(f).is_some_and(|&i| scene.island_selected.get(i as usize).copied().unwrap_or(false));
    for f in 0..nf {
        let o = scene.face_object[f] as usize;
        if scene.hidden[o] {
            continue;
        }
        let picked = island_sel(f);
        // comme Blender : avec une sélection, les autres objets sont estompés
        let dim = has_sel && !scene.selected[o] && !picked;
        let r = m.face_range(f);
        let col = if picked {
            Color32::from_rgba_unmultiplied(0xff, 0xa0, 0x28, 80)
        } else {
            match shading {
                Shading::Stretch => stretch_color(scene.face_stretch[f]).gamma_multiply(if dim { 0.2 } else { 0.75 }),
                _ => island_color(uv.face_chart[f]).gamma_multiply(if dim { 0.08 } else { 0.30 }),
            }
        };
        let i0 = fill.vertices.len() as u32;
        for ci in r.clone() {
            let t = uv.corner_uv[ci];
            fill.vertices.push(Vertex { pos: to_screen(t[0], t[1]), uv: WHITE_UV, color: col });
        }
        for t in m.fan(f) {
            fill.indices.extend_from_slice(&[i0 + t[0] as u32, i0 + t[1] as u32, i0 + t[2] as u32]);
        }
        if draw_edges || picked {
            let (lc, out) = if picked { (theme::ACCENT_ORANGE, &mut sel_lines) } else { (Color32::from_white_alpha(if dim { 35 } else { 150 }), &mut lines) };
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
                let k = out.vertices.len() as u32;
                for p in [pa + nrm, pb + nrm, pb - nrm, pa - nrm] {
                    out.vertices.push(Vertex { pos: p, uv: WHITE_UV, color: lc });
                }
                out.indices.extend_from_slice(&[k, k + 1, k + 2, k, k + 2, k + 3]);
            }
        }
    }
    lines.append(sel_lines);
    (fill, lines)
}
