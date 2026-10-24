//! Vue 3D : rendu logiciel avec tampon de profondeur (voir `raster`), grille et gizmo.

use crate::scene::Scene;
use crate::theme;
use crate::raster::{self, FaceStyle, Geometry, Line, Mips, View};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, TextureHandle, TextureOptions};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use easyuv::math::V3;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shading {
    Solid,
    Checker,
    Islands,
    Stretch,
}

#[derive(Clone, Copy)]
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
    /// Vue orthographique (pavé 5, ou vue d'axe).
    pub ortho: bool,
    /// Vue d'axe en cours (Droite, Dessus…), et si l'orthographique a été mis d'office :
    /// tourner la vue rend alors la perspective, comme Blender.
    pub axis_view: Option<AxisView>,
    auto_ortho: bool,
}

/// Vues d'axe, comme le pavé numérique de Blender.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisView {
    Front,
    Back,
    Right,
    Left,
    Top,
    Bottom,
}

impl AxisView {
    pub fn label(self) -> &'static str {
        match self {
            AxisView::Front => "Face",
            AxisView::Back => "Arrière",
            AxisView::Right => "Droite",
            AxisView::Left => "Gauche",
            AxisView::Top => "Dessus",
            AxisView::Bottom => "Dessous",
        }
    }

    /// Vue depuis le côté `+axe` (signe 1) ou `-axe` (signe -1).
    pub fn from_axis(axis: usize, positive: bool) -> Self {
        match (axis, positive) {
            (0, true) => AxisView::Right,
            (0, false) => AxisView::Left,
            (1, true) => AxisView::Back,
            (1, false) => AxisView::Front,
            (_, true) => AxisView::Top,
            (_, false) => AxisView::Bottom,
        }
    }

    /// (lacet, tangage) : l'œil est du côté de l'axe, X à droite et Z en haut pour les vues
    /// de côté, X à droite et Y en haut vu de dessus.
    fn angles(self) -> (f64, f64) {
        use std::f64::consts::{FRAC_PI_2, PI};
        let top = FRAC_PI_2 - 1e-4;
        match self {
            AxisView::Front => (-FRAC_PI_2, 0.0),
            AxisView::Back => (FRAC_PI_2, 0.0),
            AxisView::Right => (0.0, 0.0),
            AxisView::Left => (PI, 0.0),
            AxisView::Top => (-FRAC_PI_2, top),
            AxisView::Bottom => (-FRAC_PI_2, -top),
        }
    }
}

/// Recul et focale multipliés d'autant en orthographique : une caméra très lointaine et très
/// serrée donne la même image qu'une vraie projection orthographique.
const ORTHO_K: f64 = 60.0;

impl Default for Camera {
    fn default() -> Self {
        Camera { target: V3::ZERO, yaw: -0.9, pitch: 0.45, dist: 6.0, fov: 50f64.to_radians(), ortho: false, axis_view: None, auto_ortho: false }
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
        let eye = self.target + dir * self.dist * self.focal_scale();
        let fwd = (self.target - eye).normalized();
        let right = fwd.cross(V3::new(0.0, 0.0, 1.0)).normalized();
        let up = right.cross(fwd);
        Basis { eye, right, up, fwd }
    }

    /// Facteur de focale (et de recul) : 1 en perspective, ORTHO_K en orthographique.
    fn focal_scale(&self) -> f64 {
        if self.ortho {
            ORTHO_K
        } else {
            1.0
        }
    }

    /// Passe en vue d'axe orthographique (clic sur le gizmo, pavé 1 / 3 / 7).
    pub fn set_axis_view(&mut self, v: AxisView) {
        let (yaw, pitch) = v.angles();
        self.yaw = yaw;
        self.pitch = pitch;
        self.axis_view = Some(v);
        if !self.ortho {
            self.ortho = true;
            self.auto_ortho = true;
        }
    }

    /// Pavé 5 : bascule perspective / orthographique.
    pub fn toggle_ortho(&mut self) {
        self.ortho = !self.ortho;
        self.auto_ortho = false;
    }

    /// Nom de la vue pour l'en-tête : « Droite · Orthographique », « Perspective »…
    pub fn title(&self) -> String {
        let proj = if self.ortho { "Orthographique" } else { "Perspective" };
        match self.axis_view {
            Some(v) => format!("Vue 3D · {} · {proj}", v.label()),
            None => format!("Vue 3D · {proj}"),
        }
    }

    /// Axes de l'écran dans le monde (droite, haut), pour projeter depuis la vue.
    pub fn view_axes(&self) -> (V3, V3) {
        let b = self.basis();
        (b.right, b.up)
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

fn rgb(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

/// Image 3D rendue en dernier + tampon d'identifiants (sélection au clic).
pub struct RenderCache {
    tex: Option<TextureHandle>,
    key: u64,
    ids: Vec<u32>,
    faces: Vec<u32>,
    w: usize,
    h: usize,
    rect: Rect,
    /// Génération de la texture damier (pour invalider le cache).
    pub mips_gen: u64,
    /// Coût mesuré d'une image, en nanosecondes par pixel (moyenne glissante) : décide de la
    /// résolution pendant les déplacements.
    ns_per_px: f64,
}

impl Default for RenderCache {
    fn default() -> Self {
        RenderCache { tex: None, key: 0, ids: Vec::new(), faces: Vec::new(), w: 0, h: 0, rect: Rect::NOTHING, mips_gen: 0, ns_per_px: 0.0 }
    }
}

impl RenderCache {
    /// Force un nouveau rendu à la prochaine image.
    pub fn invalidate(&mut self) {
        self.key = 0;
        self.tex = None;
    }

    /// Objet sous le pointeur, d'après le dernier rendu.
    pub fn pick(&self, pos: Pos2) -> Option<usize> {
        if self.w == 0 || !self.rect.contains(pos) {
            return None;
        }
        let x = ((pos.x - self.rect.left()) / self.rect.width() * self.w as f32) as usize;
        let y = ((pos.y - self.rect.top()) / self.rect.height() * self.h as f32) as usize;
        let id = *self.ids.get(y.min(self.h - 1) * self.w + x.min(self.w - 1))?;
        (id > 0).then(|| id as usize - 1)
    }

    /// Identifiants (objets ou faces, selon `faces`) visibles dans la zone : pixels du dernier
    /// rendu compris dans `area` et pour lesquels `inside` est vrai.
    fn ids_in(&self, faces: bool, area: Rect, inside: &dyn Fn(Pos2) -> bool) -> Vec<usize> {
        if self.w == 0 {
            return Vec::new();
        }
        let buf = if faces { &self.faces } else { &self.ids };
        let (sx, sy) = (self.rect.width() / self.w as f32, self.rect.height() / self.h as f32);
        let area = area.intersect(self.rect);
        let x0 = ((area.left() - self.rect.left()) / sx).floor().max(0.0) as usize;
        let x1 = (((area.right() - self.rect.left()) / sx).ceil() as usize).min(self.w);
        let y0 = ((area.top() - self.rect.top()) / sy).floor().max(0.0) as usize;
        let y1 = (((area.bottom() - self.rect.top()) / sy).ceil() as usize).min(self.h);
        let mut seen = std::collections::HashSet::new();
        for y in y0..y1 {
            for x in x0..x1 {
                let id = buf[y * self.w + x];
                if id > 0 && !seen.contains(&id) && inside(Pos2::new(self.rect.left() + (x as f32 + 0.5) * sx, self.rect.top() + (y as f32 + 0.5) * sy)) {
                    seen.insert(id);
                }
            }
        }
        let mut v: Vec<usize> = seen.into_iter().map(|id| id as usize - 1).collect();
        v.sort_unstable();
        v
    }

    /// Objets visibles dans la zone (sélection par zone en mode Objet).
    pub fn objects_in(&self, area: Rect, inside: &dyn Fn(Pos2) -> bool) -> Vec<usize> {
        self.ids_in(false, area, inside)
    }

    /// Faces visibles dans la zone.
    pub fn faces_in(&self, area: Rect, inside: &dyn Fn(Pos2) -> bool) -> Vec<usize> {
        self.ids_in(true, area, inside)
    }

    /// Face visible sous le point écran `pos`.
    pub fn pick_face(&self, pos: Pos2) -> Option<usize> {
        if self.w == 0 || !self.rect.contains(pos) {
            return None;
        }
        let x = ((pos.x - self.rect.left()) / self.rect.width() * self.w as f32) as usize;
        let y = ((pos.y - self.rect.top()) / self.rect.height() * self.h as f32) as usize;
        let f = *self.faces.get(y.min(self.h - 1) * self.w + x.min(self.w - 1))?;
        (f > 0).then(|| f as usize - 1)
    }
}

/// Position écran d'un point 3D dans la vue `rect` (None derrière la caméra).
pub fn screen_pos(cam: &Camera, rect: Rect, p: V3) -> Option<Pos2> {
    let b = cam.basis();
    let f = (rect.height() as f64 * 0.5) / (cam.fov * 0.5).tan() * cam.focal_scale();
    let d = p - b.eye;
    let z = d.dot(b.fwd);
    if z <= cam.dist * cam.focal_scale() * 0.002 {
        return None;
    }
    let c = rect.center();
    Some(Pos2::new(c.x + (d.dot(b.right) / z * f) as f32, c.y - (d.dot(b.up) / z * f) as f32))
}

/// Sommets visibles d'objets en Édition dont la position écran est dans la zone.
pub fn vertices_in(rect: Rect, scene: &Scene, cam: &Camera, cache: &RenderCache, area: Rect, inside: &dyn Fn(Pos2) -> bool) -> Vec<u32> {
    let m = &scene.model.mesh;
    let mut out = Vec::new();
    for (v, p) in m.positions.iter().enumerate() {
        let Some(q) = screen_pos(cam, rect, *p) else { continue };
        if !area.contains(q) || !rect.contains(q) || !inside(q) {
            continue;
        }
        // caché derrière une autre face : non (comme les points dessinés en Édition)
        if cache.pick_face(q).is_some_and(|f| m.face(f).contains(&(v as u32)) && scene.object_editable(scene.face_object[f] as usize)) {
            out.push(v as u32);
        }
    }
    out
}

/// Mode Édition : points des sommets (orange s'ils sont sélectionnés), seulement ceux qui
/// sont visibles (la face sous le point les contient).
pub fn draw_vertices(ui: &egui::Ui, rect: Rect, scene: &Scene, cam: &Camera, cache: &RenderCache, all: bool) {
    let painter = ui.painter_at(rect);
    let m = &scene.model.mesh;
    let mut sel = vec![false; m.positions.len()];
    for (c, &s) in scene.corner_sel.iter().enumerate() {
        if s {
            sel[m.corners[c] as usize] = true;
        }
    }
    if !all && !sel.iter().any(|&s| s) {
        return;
    }
    if m.positions.len() > 60_000 && all {
        return; // trop de points : on ne montre que la sélection
    }
    for (v, p) in m.positions.iter().enumerate() {
        if !all && !sel[v] {
            continue;
        }
        let Some(q) = screen_pos(cam, rect, *p) else { continue };
        if !rect.contains(q) {
            continue;
        }
        // sommet visible d'un objet en Édition
        let visible = cache.pick_face(q).is_some_and(|f| m.face(f).contains(&(v as u32)) && scene.object_editable(scene.face_object[f] as usize));
        if !visible {
            continue;
        }
        if sel[v] {
            painter.circle_filled(q, 3.0, theme::ACCENT_ORANGE);
        } else {
            painter.circle_filled(q, 1.6, Color32::from_gray(20));
        }
    }
}

/// Centre du gizmo d'axes, dans la zone de la vue (sous l'en-tête).
fn gizmo_center(body: Rect) -> Pos2 {
    Pos2::new(body.right() - 52.0, body.top() + 36.0)
}

/// Boule du gizmo sous le pointeur : (axe 0 / 1 / 2, côté positif), la plus proche de l'œil
/// l'emportant.
fn gizmo_hit(body: Rect, b: &Basis, pos: Pos2) -> Option<(usize, bool)> {
    let c = gizmo_center(body);
    let r = 36.0f64;
    let mut best: Option<(f64, (usize, bool))> = None;
    for axis in 0..3 {
        for positive in [true, false] {
            let mut v = V3::ZERO;
            match axis {
                0 => v.x = 1.0,
                1 => v.y = 1.0,
                _ => v.z = 1.0,
            }
            let v = if positive { v } else { v * -1.0 };
            let p = Pos2::new(c.x + (v.dot(b.right) * r) as f32, c.y - (v.dot(b.up) * r) as f32);
            if (p - pos).length() <= 10.0 {
                let depth = v.dot(b.fwd);
                if best.is_none_or(|(d, _)| depth < d) {
                    best = Some((depth, (axis, positive)));
                }
            }
        }
    }
    best.map(|b| b.1)
}

/// Navigation de la vue 3D. Renvoie vrai si le clic a servi au gizmo (vue d'axe) : il ne
/// doit alors pas sélectionner l'objet derrière.
pub fn handle_input(ui: &egui::Ui, resp: &egui::Response, cam: &mut Camera, rect: Rect) -> bool {
    if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            if let Some((axis, positive)) = gizmo_hit(rect, &cam.basis(), p) {
                cam.set_axis_view(AxisView::from_axis(axis, positive));
                return true;
            }
        }
    }
    let (shift, scroll, mmb, lmb, rmb) = ui.input(|i| {
        (
            i.modifiers.shift,
            i.smooth_scroll_delta.y,
            i.pointer.middle_down(),
            i.pointer.primary_down(),
            i.pointer.secondary_down(),
        )
    });
    // le clic gauche glissé sélectionne par zone ; Alt+glisser ou la molette cliquée tourne
    let alt = ui.input(|i| i.modifiers.alt);
    if resp.dragged() && (mmb || (lmb && alt) || rmb) {
        let d = resp.drag_delta();
        if (shift && !lmb) || rmb {
            let b = cam.basis();
            let k = cam.dist * (cam.fov * 0.5).tan() * 2.0 / rect.height().max(1.0) as f64;
            cam.target = cam.target - b.right * (d.x as f64 * k) + b.up * (d.y as f64 * k);
        } else {
            cam.yaw -= d.x as f64 * 0.008;
            cam.pitch = (cam.pitch + d.y as f64 * 0.008).clamp(-1.55, 1.55);
            // tourner quitte la vue d'axe ; l'orthographique mis d'office redevient perspective
            if cam.axis_view.take().is_some() && cam.auto_ortho {
                cam.ortho = false;
                cam.auto_ortho = false;
            }
        }
    }
    if resp.hovered() && scroll != 0.0 {
        cam.dist = (cam.dist * (-(scroll as f64) * 0.0018).exp()).max(1e-3);
    }
    false
}

#[allow(clippy::too_many_arguments)]
pub fn draw(ui: &mut egui::Ui, rect: Rect, scene: Option<&Scene>, cam: &Camera, opt: &ViewOptions, mips: &Mips, cache: &mut RenderCache) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, theme::BG_VIEWPORT);
    let b = cam.basis();
    let f = (rect.height() as f64 * 0.5) / (cam.fov * 0.5).tan() * cam.focal_scale();
    let c = rect.center();
    let near = cam.dist * cam.focal_scale() * 0.002;
    let project = |p: V3| -> (Pos2, f64) {
        let d = p - b.eye;
        let z = d.dot(b.fwd);
        let zz = z.max(near);
        (Pos2::new(c.x + (d.dot(b.right) / zz * f) as f32, c.y - (d.dot(b.up) / zz * f) as f32), z)
    };

    // Grille du sol (plan XY) + axes X / Y, sous l'image 3D (fond transparent)
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
        // en mouvement (orbite, zoom, déplacement) : demi-résolution, puis pleine résolution à l'arrêt
        let moving = ui.input(|i| i.pointer.any_down() || i.smooth_scroll_delta != egui::Vec2::ZERO);
        if ui.input(|i| i.smooth_scroll_delta != egui::Vec2::ZERO) {
            // garantit une dernière image (pleine résolution) à la fin du zoom
            ui.ctx().request_repaint();
        }
        // en mouvement, la résolution ne baisse que si une image pleine résolution dépasserait
        // ~14 ms (plus de 60 images par seconde) : les modèles courants restent nets
        let full = ui.ctx().pixels_per_point();
        let full_px = (rect.width() * full) as f64 * (rect.height() * full) as f64;
        let scale = if moving && cache.ns_per_px > 0.0 {
            let predicted = cache.ns_per_px * full_px;
            let s = (14.0e6 / predicted.max(1.0)).sqrt().clamp(0.5, 1.0);
            // par paliers : la texture n'est pas réallouée à chaque image
            (s * 8.0).floor() / 8.0
        } else {
            1.0
        };
        let ppp = full * scale as f32;
        let (w, h) = (((rect.width() * ppp) as usize).max(1), ((rect.height() * ppp) as usize).max(1));
        // clé du cache : tout ce qui change l'image
        let mut hs = DefaultHasher::new();
        for v in [cam.target.x, cam.target.y, cam.target.z, cam.yaw, cam.pitch, cam.dist, cam.fov] {
            v.to_bits().hash(&mut hs);
        }
        cam.ortho.hash(&mut hs);
        (w, h, scene.generation, scene as *const Scene as usize, cache.mips_gen).hash(&mut hs);
        (opt.shading as u8, opt.wire, opt.seams, opt.tiling.to_bits(), scene.hover_island).hash(&mut hs);
        let key = hs.finish();
        if key != cache.key || cache.tex.is_none() {
            let t0 = std::time::Instant::now();
            let frame = render_scene(scene, &b, f * ppp as f64, near, w, h, opt, mips, ppp);
            let ns = t0.elapsed().as_nanos() as f64 / (w * h).max(1) as f64;
            cache.ns_per_px = if cache.ns_per_px > 0.0 { cache.ns_per_px * 0.7 + ns * 0.3 } else { ns };
            let img = egui::ColorImage::from_rgba_unmultiplied([frame.width, frame.height], &frame.rgba);
            match &mut cache.tex {
                Some(t) => t.set(img, if scale < 1.0 { TextureOptions::LINEAR } else { TextureOptions::NEAREST }),
                None => cache.tex = Some(ui.ctx().load_texture("viewport3d", img, TextureOptions::NEAREST)),
            }
            cache.ids = frame.ids;
            cache.faces = frame.faces;
            cache.w = frame.width;
            cache.h = frame.height;
            cache.key = key;
        }
        cache.rect = rect;
        if let Some(t) = &cache.tex {
            painter.image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
    } else {
        cache.w = 0;
    }

    let hover = ui.input(|i| i.pointer.hover_pos());
    draw_gizmo(&painter, rect, &b, hover);
}

#[allow(clippy::too_many_arguments)]
fn render_scene(scene: &Scene, b: &Basis, focal: f64, near: f64, w: usize, h: usize, opt: &ViewOptions, mips: &Mips, ppp: f32) -> raster::Frame {
    let m = &scene.model.mesh;
    let uv = scene.uv.as_ref();
    let styles: Vec<Option<FaceStyle>> = (0..m.face_count())
        .map(|fi| {
            let o = scene.face_object[fi] as usize;
            if scene.hidden[o] {
                return None;
            }
            let (mut color, textured) = match (opt.shading, uv) {
                (Shading::Checker, Some(_)) => ([255, 255, 255], true),
                (Shading::Islands, Some(u)) => (rgb(island_color(u.face_chart[fi])), false),
                (Shading::Stretch, Some(_)) => (rgb(stretch_color(scene.face_stretch[fi])), false),
                _ => ([204, 204, 204], false),
            };
            // sélection de l'éditeur UV sur le modèle : îlots (mode îlots) ou faces choisies en
            // orange, îlot survolé en jaune clair
            let isl = scene.face_island.get(fi).map(|&i| i as usize);
            // en Édition, seuls les objets édités montrent leur sélection d'éléments
            let picked = scene.object_editable(o)
                && match scene.sel_mode {
                    crate::scene::SelectMode::Island => isl.is_some_and(|i| scene.island_selected.get(i).copied().unwrap_or(false)),
                    _ => scene.face_selected(fi),
                };
            let mix = |c: [u8; 3], t: [u8; 3], k: f32| -> [u8; 3] { std::array::from_fn(|j| (c[j] as f32 * (1.0 - k) + t[j] as f32 * k) as u8) };
            if picked {
                color = mix(color, [255, 150, 30], 0.6);
            } else if isl.is_some() && isl == scene.hover_island {
                color = mix(color, [255, 240, 170], 0.45);
            }
            Some(FaceStyle { color, textured, id: o as u32 + 1 })
        })
        .collect();
    let mut lines = Vec::new();
    let wire = opt.wire && scene.edges.len() < 400_000;
    // arêtes choisies dans l'éditeur UV et coutures marquées à la main, toujours visibles
    let picked: std::collections::HashSet<(u32, u32)> = scene.selected_mesh_edges().into_iter().collect();
    let lw = ppp.round().max(1.0) as u8;
    // en Édition, comme dans Blender : fil de fer et sélection sur les seuls objets édités
    let editing = scene.is_editing() && scene.edges.len() < 400_000;
    if wire || editing || opt.seams || !picked.is_empty() || !scene.marked_seams.is_empty() {
        for &(a, bb, o) in &scene.edges {
            if scene.hidden[o as usize] {
                continue;
            }
            let edit_o = scene.in_edit(o as usize);
            let wire = wire || (editing && edit_o);
            let seam = opt.seams && scene.seam_set.contains(&(a, bb));
            let col = |c: Color32| {
                let c = rgb(c);
                [c[0], c[1], c[2], 255]
            };
            if picked.contains(&(a, bb)) && scene.object_editable(o as usize) {
                lines.push(Line { a: m.positions[a as usize], b: m.positions[bb as usize], color: col(theme::ACCENT_ORANGE), width: lw * 2 });
            } else if scene.marked_seams.contains(&(a, bb)) {
                lines.push(Line { a: m.positions[a as usize], b: m.positions[bb as usize], color: col(theme::SEAM_MARK), width: lw * 2 });
            } else if seam {
                lines.push(Line { a: m.positions[a as usize], b: m.positions[bb as usize], color: col(theme::SEAM_RED), width: lw * 2 });
            } else if wire {
                let color = if edit_o { [0, 0, 0, 150] } else { [0, 0, 0, 90] };
                lines.push(Line { a: m.positions[a as usize], b: m.positions[bb as usize], color, width: lw });
            }
        }
    }
    let view = View { eye: b.eye, right: b.right, up: b.up, fwd: b.fwd, focal, near, width: w, height: h };
    let geo = Geometry {
        positions: &m.positions,
        corners: &m.corners,
        face_start: &m.face_start,
        face_normals: &scene.face_normals,
        corner_uv: uv.map(|u| u.corner_uv.as_slice()),
        styles: &styles,
    };
    // en Édition, pas de contour d'objet : la sélection est celle des éléments
    let none;
    let (selected, active) = if scene.is_editing() {
        none = vec![false; scene.selected.len()];
        (&none, None)
    } else {
        (&scene.selected, scene.has_selection().then_some(scene.active))
    };
    raster::render(&view, &geo, &lines, mips, opt.tiling, selected, active, (1.5 * ppp).round().max(1.0) as usize)
}

/// Gizmo d'orientation en haut à droite, comme dans Blender.
fn draw_gizmo(painter: &egui::Painter, rect: Rect, b: &Basis, hover: Option<Pos2>) {
    let body = Rect::from_min_max(rect.min + egui::vec2(0.0, 26.0), rect.max);
    let c = gizmo_center(body);
    // boule survolée : cliquable, comme dans Blender
    let hot = hover.and_then(|p| gizmo_hit(body, b, p));
    let r = 36.0f64;
    painter.circle_filled(c, r as f32 + 10.0, Color32::from_white_alpha(8));
    let axes = [
        (V3::new(1.0, 0.0, 0.0), theme::AXIS_X, "X"),
        (V3::new(0.0, 1.0, 0.0), theme::AXIS_Y, "Y"),
        (V3::new(0.0, 0.0, 1.0), theme::AXIS_Z, "Z"),
    ];
    let mut items: Vec<(f64, Pos2, Color32, Option<&str>, bool)> = Vec::new();
    for (k, (a, col, name)) in axes.into_iter().enumerate() {
        for sign in [1.0, -1.0] {
            let v = a * sign;
            let p = Pos2::new(c.x + (v.dot(b.right) * r) as f32, c.y - (v.dot(b.up) * r) as f32);
            items.push((v.dot(b.fwd), p, col, if sign > 0.0 { Some(name) } else { None }, hot == Some((k, sign > 0.0))));
        }
    }
    items.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    if hot.is_some() {
        painter.circle_filled(c, r as f32 + 10.0, Color32::from_white_alpha(14));
    }
    for (_, p, col, name, is_hot) in items {
        if is_hot {
            painter.circle_stroke(p, 11.0, Stroke::new(1.5, Color32::WHITE));
        }
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
