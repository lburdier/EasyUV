//! Éditeur UV 2D : tuile 0-1 avec damier, îlots colorés et arêtes, et édition des îlots
//! à la manière de Blender (clic, rectangle, G / R / S, X / Y, Ctrl, Échap).

use crate::scene::{Scene, SelectMode};
use crate::theme;
use crate::viewport::{island_color, stretch_color, Shading, WHITE_UV};
use easyuv::math::V2;
use easyuv::uvtools::{self, Brush, Islands, UvGraph};
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

impl UvCamera {
    /// Cadre une boîte UV (îlots sélectionnés), avec un peu de marge.
    pub fn frame(&mut self, lo: [f32; 2], hi: [f32; 2]) {
        let ext = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-4);
        self.center = egui::vec2((lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5);
        self.zoom = (0.88 / ext).clamp(0.05, 200.0);
    }
}

/// Message posé sur l'éditeur, sur un fond sombre pour rester lisible au-dessus du damier.
pub fn overlay_text(painter: &egui::Painter, pos: Pos2, text: &str) {
    let galley = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(14.0), theme::TEXT);
    let bg = Rect::from_center_size(pos, galley.size() + egui::vec2(20.0, 10.0));
    painter.rect_filled(bg, 6.0, Color32::from_black_alpha(190));
    painter.galley(pos - galley.size() * 0.5, galley, theme::TEXT);
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

/// Outil actif de l'éditeur UV.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UvTool {
    /// Sélection et transformation des îlots (G / R / S).
    #[default]
    Select,
    Grab,
    Relax,
    Straighten,
    Pinch,
    /// Dessiner le rectangle dans lequel aplatir l'îlot.
    Rect,
}

impl UvTool {
    pub const ALL: [UvTool; 6] = [UvTool::Select, UvTool::Grab, UvTool::Relax, UvTool::Straighten, UvTool::Pinch, UvTool::Rect];

    pub fn label(self) -> &'static str {
        match self {
            UvTool::Select => "Sélection",
            UvTool::Grab => "Déplacer",
            UvTool::Relax => "Détendre",
            UvTool::Straighten => "Redresser",
            UvTool::Pinch => "Pincer",
            UvTool::Rect => "Rectangle",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            UvTool::Select => "Q",
            UvTool::Grab => "Maj+1",
            UvTool::Relax => "Maj+2",
            UvTool::Straighten => "Maj+3",
            UvTool::Pinch => "Maj+4",
            UvTool::Rect => "Maj+5",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            UvTool::Select => "Clic, Maj+clic ou rectangle : îlots · glisser : déplacer · G R S : transformer",
            UvTool::Grab => "Glisser : tirer les UV sous le pinceau, l'effet s'atténue vers le bord",
            UvTool::Relax => "Peindre : les arêtes reprennent leur longueur 3D (moins d'étirement)",
            UvTool::Straighten => "Peindre : les arêtes s'alignent sur l'horizontale ou la verticale la plus proche",
            UvTool::Pinch => "Peindre : resserre les UV vers le centre · Maj : les écarte",
            UvTool::Rect => "Dessiner un rectangle : l'îlot sélectionné y est rendu droit et étiré pour le remplir",
        }
    }

    pub fn brush(self) -> Option<Brush> {
        match self {
            UvTool::Grab => Some(Brush::Grab),
            UvTool::Relax => Some(Brush::Relax),
            UvTool::Straighten => Some(Brush::Straighten),
            UvTool::Pinch => Some(Brush::Pinch),
            _ => None,
        }
    }
}

/// Forme de la sélection par zone (W pour passer de l'une à l'autre, comme Blender).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SelShape {
    #[default]
    Box,
    Lasso,
    Circle,
}

impl SelShape {
    pub const ALL: [SelShape; 3] = [SelShape::Box, SelShape::Lasso, SelShape::Circle];

    pub fn label(self) -> &'static str {
        match self {
            SelShape::Box => "Rectangle de sélection",
            SelShape::Lasso => "Lasso",
            SelShape::Circle => "Cercle",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            SelShape::Box => "Glisser dans le vide : tout ce qui est dans le rectangle. Maj : ajouter, Ctrl : retirer.",
            SelShape::Lasso => "Glisser : tracez la zone à main levée, tout ce qui est dedans est choisi. Maj : ajouter, Ctrl : retirer.",
            SelShape::Circle => "Glisser : peignez la sélection avec le cercle ([ ] ou Ctrl+molette : rayon). Ctrl : retirer.",
        }
    }
}

/// Coup de pinceau en cours.
struct BrushStroke {
    /// UV d'avant le geste (Échap, Ctrl+Z).
    orig: Vec<[f32; 2]>,
    graph: UvGraph,
    start: Vec<V2>,
    pts: Vec<V2>,
    /// Poids figés au début du geste (pinceau Déplacer).
    weights: Vec<f64>,
    start_uv: V2,
}

/// État de l'outil d'édition UV.
pub struct UvEditor {
    modal: Option<Modal>,
    /// Sélection au rectangle : (origine écran, mode 0 / 1 / -1).
    boxsel: Option<(Pos2, i32)>,
    /// Ignore le clic qui termine une transformation (il ne doit pas resélectionner).
    swallow_primary: bool,
    swallow_secondary: bool,
    pub tool: UvTool,
    /// Rayon du pinceau, en pixels d'écran.
    pub radius: f32,
    /// Force du pinceau (0-1).
    pub strength: f32,
    stroke: Option<BrushStroke>,
    /// Outil Rectangle : origine écran du rectangle en cours de tracé.
    rect: Option<Pos2>,
    /// Forme de la sélection par zone.
    pub sel_shape: SelShape,
    /// Tracé du lasso en cours (écran).
    lasso: Vec<Pos2>,
}

impl Default for UvEditor {
    fn default() -> Self {
        UvEditor { modal: None, boxsel: None, swallow_primary: false, swallow_secondary: false, tool: UvTool::Select, radius: 60.0, strength: 0.5, stroke: None, rect: None, sel_shape: SelShape::Box, lasso: Vec::new() }
    }
}

/// Ce que l'application doit faire après l'interaction.
pub enum UvEvent {
    /// Transformation validée : UV d'avant, pour l'historique d'annulation.
    Commit(Vec<[f32; 2]>),
    Status(String),
    /// Maj+W : rendre droits les îlots sélectionnés.
    Straighten,
    /// Outil Rectangle : aplatir les îlots sélectionnés dans [lo, hi] (UV).
    FlattenRect([f32; 2], [f32; 2]),
    /// Maj+G : Grille parfaite.
    SnapGrid,
}

impl UvEditor {
    pub fn busy(&self) -> bool {
        self.modal.is_some() || self.stroke.is_some() || self.rect.is_some()
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
    // pinceau : Ctrl+molette règle le rayon au lieu du zoom
    let zoom = ui.input(|i| i.zoom_delta());
    let circle = ed.tool == UvTool::Select && ed.sel_shape == SelShape::Circle;
    if (ed.tool.brush().is_some() || circle) && resp.hovered() && zoom != 1.0 {
        ed.radius = (ed.radius * zoom).clamp(4.0, 600.0);
    } else {
        camera_input(ui, resp, cam, rect);
    }
    let view = View::new(rect, cam);
    let scene = scene?;
    scene.uv.as_ref()?;
    let (pointer, shift, ctrl, alt) = ui.input(|i| (i.pointer.latest_pos(), i.modifiers.shift, i.modifiers.command, i.modifiers.alt));
    // îlot survolé : la vue 3D montre à quelle partie du modèle il correspond
    let moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    if !resp.hovered() {
        scene.hover_island = None;
    } else if moved && ed.modal.is_none() && ed.stroke.is_none() {
        scene.hover_island = resp.hover_pos().and_then(|p| scene.pick_island(view.to_uv(p)));
    }
    let keys = !ui.ctx().egui_wants_keyboard_input() && (resp.hovered() || ed.modal.is_some() || ed.stroke.is_some());
    let key = |k: egui::Key| keys && ui.input(|i| i.key_pressed(k));

    // --- mode de sélection (1 à 4, comme Blender), outils (Q, Maj+1 à Maj+5), rayon ([ ]) ---
    if ed.modal.is_none() && ed.stroke.is_none() && !ctrl && !alt {
        let digits = [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4, egui::Key::Num5];
        if shift {
            for (k, t) in digits.into_iter().zip([UvTool::Grab, UvTool::Relax, UvTool::Straighten, UvTool::Pinch, UvTool::Rect]) {
                if key(k) {
                    ed.tool = t;
                    ed.rect = None;
                }
            }
        } else {
            for (k, m) in digits.into_iter().zip(SelectMode::ALL) {
                if key(k) {
                    scene.set_select_mode(m);
                }
            }
        }
        if key(egui::Key::Q) {
            ed.tool = UvTool::Select;
            ed.rect = None;
        }
    }
    if key(egui::Key::OpenBracket) {
        ed.radius = (ed.radius / 1.15).max(4.0);
    }
    if key(egui::Key::CloseBracket) {
        ed.radius = (ed.radius * 1.15).min(600.0);
    }
    if ed.tool != UvTool::Select {
        if let Some(ev) = tool_interact(ui, resp, view, scene, ed, pointer, shift, &key) {
            return Some(ev);
        }
        // un simple clic choisit l'élément visé (pour limiter le pinceau à son îlot)
        if resp.clicked() && ed.stroke.is_none() {
            if let Some(p) = resp.interact_pointer_pos() {
                select_at(scene, view, p, shift);
            }
        }
        return None;
    }
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
        scene.select_all_uv(!alt);
    }
    if ctrl && key(egui::Key::I) {
        scene.invert_uv();
    }
    if shift && key(egui::Key::W) {
        return Some(UvEvent::Straighten);
    }
    if !shift && !ctrl && key(egui::Key::W) {
        let i = SelShape::ALL.iter().position(|&x| x == ed.sel_shape).unwrap_or(0);
        ed.sel_shape = SelShape::ALL[(i + 1) % SelShape::ALL.len()];
        return Some(UvEvent::Status(format!("Sélection : {}", ed.sel_shape.label())));
    }
    if shift && key(egui::Key::G) {
        return Some(UvEvent::SnapGrid);
    }
    let start = pointer.filter(|p| rect.contains(*p));
    for (k, kind) in [(egui::Key::G, Kind::Grab), (egui::Key::R, Kind::Rotate), (egui::Key::S, Kind::Scale)] {
        if !ctrl && !shift && key(k) {
            if let Some(p) = start {
                if !ed.begin(kind, scene, p, false) {
                    return Some(UvEvent::Status("Sélectionnez d'abord des éléments (clic, A ou rectangle)".into()));
                }
                return None;
            }
        }
    }

    // --- souris : clic, glisser la sélection, rectangle ---
    if resp.drag_started_by(egui::PointerButton::Primary) {
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or_default();
        let mode = if shift { 1 } else if ctrl { -1 } else { 0 };
        // Maj ou Ctrl maintenu : toujours une sélection par zone (ajouter / retirer), même en
        // partant d'un îlot, comme Blender ; sans touche, glisser un élément le déplace
        let grab = if shift || ctrl || ed.sel_shape != SelShape::Box { None } else { under_pointer(scene, view, origin) };
        match grab {
            Some(selected) => {
                // glisser un élément : il est d'abord choisi s'il ne l'était pas (comme Blender)
                if !selected {
                    select_at(scene, view, origin, shift);
                }
                ed.begin(Kind::Grab, scene, origin, true);
                if let Some(m) = &mut ed.modal {
                    m.info = "Déplacer (glisser)".into();
                }
            }
            None => {
                ed.boxsel = Some((origin, mode));
                ed.lasso = vec![origin];
                // cercle sans touche : on repart d'une sélection vide, puis on peint
                if ed.sel_shape == SelShape::Circle && mode == 0 {
                    scene.select_all_uv(false);
                }
            }
        }
    }
    if let Some((o, mode)) = ed.boxsel {
        if resp.dragged_by(egui::PointerButton::Primary) {
            if let Some(p) = pointer {
                match ed.sel_shape {
                    SelShape::Lasso if ed.lasso.last().is_none_or(|l| (*l - p).length() > 3.0) => ed.lasso.push(p),
                    SelShape::Circle => {
                        let (c, r) = (view.to_uv(p), ed.radius / view.side);
                        scene.select_where(|q| (q[0] - c[0]).powi(2) + (q[1] - c[1]).powi(2) <= r * r, if mode < 0 { -1 } else { 1 });
                    }
                    _ => {}
                }
            }
            ui.ctx().request_repaint();
        } else {
            ed.boxsel = None;
            match ed.sel_shape {
                SelShape::Box => {
                    if let Some(p) = pointer {
                        let (a, b) = (view.to_uv(o), view.to_uv(p));
                        scene.box_select([a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])], mode);
                    }
                }
                SelShape::Lasso => {
                    let poly: Vec<[f32; 2]> = ed.lasso.iter().map(|p| view.to_uv(*p)).collect();
                    if poly.len() >= 3 {
                        scene.select_where(|q| point_in_polygon(q, &poly), mode);
                    }
                }
                SelShape::Circle => {}
            }
            ed.lasso.clear();
        }
    }
    if resp.double_clicked() && scene.sel_mode == SelectMode::Edge {
        // double-clic sur une arête : toute la boucle d'arêtes
        if let Some(p) = resp.interact_pointer_pos() {
            let n = scene.select_loop(view.to_uv(p), PICK_PX / view.side, shift);
            if n > 0 {
                return Some(UvEvent::Status(format!("Boucle de {n} arête(s) sélectionnée")));
            }
        }
    } else if resp.clicked() {
        if ed.swallow_primary {
            ed.swallow_primary = false;
        } else if let Some(p) = resp.interact_pointer_pos() {
            select_at(scene, view, p, shift);
        }
    }
    None
}

/// Point dans un polygone (règle pair-impair).
fn point_in_polygon(q: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
    }
    inside
}

/// Distance de prise d'un sommet ou d'une arête, en pixels d'écran.
const PICK_PX: f32 = 8.0;

/// Clic de sélection, selon le mode (îlots, ou sommets / arêtes / faces).
fn select_at(scene: &mut Scene, view: View, p: Pos2, shift: bool) -> bool {
    let q = view.to_uv(p);
    if scene.sel_mode == SelectMode::Island {
        let hit = scene.pick_island(q);
        scene.click_island(hit, shift);
        hit.is_some()
    } else {
        scene.click_component(q, PICK_PX / view.side, shift)
    }
}

/// Élément sous le pointeur : None s'il n'y a rien, sinon s'il est déjà sélectionné.
fn under_pointer(scene: &Scene, view: View, p: Pos2) -> Option<bool> {
    let (q, tol) = (view.to_uv(p), PICK_PX / view.side);
    let sel = |c: usize| scene.corner_sel.get(c).copied().unwrap_or(false);
    match scene.sel_mode {
        SelectMode::Island => scene.pick_island(q).map(|i| scene.island_selected[i]),
        SelectMode::Vertex => scene.pick_corner(q, tol).map(sel),
        SelectMode::Edge => scene.pick_edge(q, tol).map(|(a, b)| sel(a) && sel(b)),
        SelectMode::Face => scene.pick_face(q).map(|f| scene.face_selected(f)),
    }
}

fn v2(p: [f32; 2]) -> V2 {
    V2::new(p[0] as f64, p[1] as f64)
}

/// Pinceaux et outil Rectangle.
#[allow(clippy::too_many_arguments)]
fn tool_interact(ui: &egui::Ui, resp: &egui::Response, view: View, scene: &mut Scene, ed: &mut UvEditor, pointer: Option<Pos2>, shift: bool, key: &dyn Fn(egui::Key) -> bool) -> Option<UvEvent> {
    let origin = ui.input(|i| i.pointer.press_origin());
    // --- outil Rectangle ---
    if ed.tool == UvTool::Rect {
        if resp.drag_started_by(egui::PointerButton::Primary) {
            ed.rect = origin;
        }
        if let Some(o) = ed.rect {
            if key(egui::Key::Escape) {
                ed.rect = None;
                return None;
            }
            if !resp.dragged_by(egui::PointerButton::Primary) {
                ed.rect = None;
                let p = pointer?;
                if (p - o).length() < 6.0 {
                    return None;
                }
                let (a, b) = (view.to_uv(o), view.to_uv(p));
                let (lo, hi) = ([a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])]);
                if !scene.has_island_selection() {
                    // sans sélection : l'îlot sous le rectangle (centre, puis départ du geste)
                    let hit = scene.pick_island([(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5]).or_else(|| scene.pick_island(a));
                    match hit {
                        Some(i) => scene.select_island_only(i),
                        None => return Some(UvEvent::Status("Sélectionnez l'îlot à aplatir (clic), puis dessinez le rectangle".into())),
                    }
                }
                return Some(UvEvent::FlattenRect(lo, hi));
            }
            ui.ctx().request_repaint();
        }
        return None;
    }
    // --- pinceaux ---
    let brush = ed.tool.brush()?;
    let radius_uv = (ed.radius / view.side) as f64;
    if resp.drag_started_by(egui::PointerButton::Primary) {
        let ids = scene.brush_islands();
        if ids.is_empty() {
            return Some(UvEvent::Status("Aucun îlot visible à modifier".into()));
        }
        let uv = &scene.uv.as_ref()?.corner_uv;
        let isl = Islands::new(&scene.model.mesh, &scene.face_island, &ids);
        let graph = UvGraph::build(&scene.model.mesh, uv, &isl);
        let pts = graph.positions(uv);
        let start_uv = v2(view.to_uv(origin.or(pointer)?));
        let weights = uvtools::brush_weights(&pts, start_uv, radius_uv);
        ed.stroke = Some(BrushStroke { orig: uv.clone(), graph, start: pts.clone(), pts, weights, start_uv });
    }
    let st = ed.stroke.as_mut()?;
    if key(egui::Key::Escape) {
        let st = ed.stroke.take()?;
        scene.uv.as_mut()?.corner_uv = st.orig;
        scene.touch();
        return Some(UvEvent::Status("Coup de pinceau annulé".into()));
    }
    if resp.dragged_by(egui::PointerButton::Primary) {
        let c = v2(view.to_uv(pointer?));
        match brush {
            Brush::Grab => uvtools::brush_grab(&mut st.pts, &st.start, &st.weights, c - st.start_uv),
            b => uvtools::brush_step(&st.graph, &mut st.pts, b, c, radius_uv, ed.strength as f64 * 0.35, shift),
        }
        st.graph.write(&st.pts, &mut scene.uv.as_mut()?.corner_uv);
        scene.touch();
        // le pinceau agit tant que le bouton est enfoncé, même sans bouger
        ui.ctx().request_repaint();
        return None;
    }
    let st = ed.stroke.take()?;
    Some(UvEvent::Commit(st.orig))
}

/// Le menu contextuel ne doit pas s'ouvrir sur le clic droit qui annule une transformation.
pub fn context_menu_allowed(ed: &UvEditor) -> bool {
    ed.modal.is_none() && !ed.swallow_secondary
}

#[allow(clippy::too_many_arguments)]
pub fn draw(ui: &mut egui::Ui, rect: Rect, scene: Option<&Scene>, cam: &UvCamera, shading: Shading, tex: TextureId, tiling: f32, cache: &mut UvCache, ed: &UvEditor, busy: bool) {
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
        overlay_text(&painter, rect.center(), "Aucun objet — Fichier > Ouvrir (Ctrl+O) ou glisser un fichier");
        return;
    };
    let Some(uv) = &scene.uv else {
        // pendant le premier dépliage, l'application affiche sa propre progression
        if !busy {
            overlay_text(&painter, rect.center(), "Pas encore d'UV — appuyez sur U");
        }
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

    // Sélection par zone : rectangle, lasso ou cercle
    if let (Some((o, mode)), Some(p)) = (ed.boxsel, ui.input(|i| i.pointer.latest_pos())) {
        let col = if mode < 0 { theme::SEAM_RED } else { Color32::WHITE };
        match ed.sel_shape {
            SelShape::Box => {
                let r = Rect::from_two_pos(o, p);
                painter.rect_filled(r, 0.0, Color32::from_white_alpha(10));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, col), egui::StrokeKind::Inside);
            }
            SelShape::Lasso => {
                let mut pts = ed.lasso.clone();
                pts.push(p);
                painter.add(Shape::closed_line(pts, Stroke::new(1.0, col)));
            }
            SelShape::Circle => {}
        }
    }
    if ed.tool == UvTool::Select && ed.sel_shape == SelShape::Circle {
        if let Some(p) = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)) {
            let col = if ed.boxsel.is_some_and(|b| b.1 < 0) { theme::SEAM_RED } else { Color32::from_white_alpha(200) };
            painter.circle_stroke(p, ed.radius, Stroke::new(1.0, col));
        }
    }
    // Outil Rectangle : le rectangle en cours de tracé
    if let (Some(o), Some(p)) = (ed.rect, ui.input(|i| i.pointer.latest_pos())) {
        let r = Rect::from_two_pos(o, p);
        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(0xff, 0xa0, 0x28, 24));
        painter.rect_stroke(r, 0.0, Stroke::new(1.5, theme::ACCENT_ORANGE), egui::StrokeKind::Inside);
    }
    // Pinceau : cercle d'influence sous le pointeur
    if ed.tool.brush().is_some() {
        if let Some(p) = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)) {
            let col = if ed.stroke.is_some() { theme::ACCENT_ORANGE } else { Color32::from_white_alpha(200) };
            painter.circle_stroke(p, ed.radius, Stroke::new(1.5, col));
            painter.circle_stroke(p, ed.radius * 0.5, Stroke::new(1.0, Color32::from_white_alpha(60)));
            painter.circle_filled(p, 1.5, col);
        }
    }
    // Aide de l'outil actif (hors sélection)
    if ed.tool != UvTool::Select && ed.info().is_none() {
        let txt = match ed.tool.brush() {
            Some(_) => format!(
                "{}  ·  rayon {:.0} px  ·  force {:.0} %   —   {}   ·   [ ] ou Ctrl+molette : rayon   ·   clic : limiter à un îlot   ·   Échap : annuler",
                ed.tool.label(),
                ed.radius,
                ed.strength * 100.0,
                ed.tool.hint()
            ),
            None => format!("{}   —   {}   ·   Échap : annuler", ed.tool.label(), ed.tool.hint()),
        };
        let pos = rect.left_bottom() + egui::vec2(10.0, -10.0);
        let galley = painter.layout(txt, egui::FontId::proportional(12.0), theme::TEXT, rect.width() - 30.0);
        let bg = Rect::from_min_size(pos - egui::vec2(6.0, galley.size().y + 4.0), galley.size() + egui::vec2(12.0, 8.0));
        painter.rect_filled(bg, 4.0, Color32::from_black_alpha(170));
        painter.galley(pos - egui::vec2(0.0, galley.size().y), galley, theme::TEXT);
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
    let mode = scene.sel_mode;
    let island_sel = |f: usize| match mode {
        SelectMode::Island => scene.face_island.get(f).is_some_and(|&i| scene.island_selected.get(i as usize).copied().unwrap_or(false)),
        SelectMode::Face => scene.face_selected(f),
        _ => false,
    };
    let csel = |c: usize| mode != SelectMode::Island && scene.corner_sel.get(c).copied().unwrap_or(false);
    let marked = &scene.marked_seams;
    // mode sommets : un point par coin (les sélectionnés en orange, dessinés en dernier)
    let mut dots = egui::Mesh::with_texture(tex);
    let mut sel_dots = egui::Mesh::with_texture(tex);
    let draw_dots = mode == SelectMode::Vertex && m.corners.len() < 200_000;
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
        if draw_dots {
            for c in r.clone() {
                let q = uv.corner_uv[c];
                let pt = to_screen(q[0], q[1]);
                let (out, col, half) = if csel(c) { (&mut sel_dots, theme::ACCENT_ORANGE, 2.5) } else { (&mut dots, Color32::from_white_alpha(if dim { 60 } else { 200 }), 1.5) };
                let k = out.vertices.len() as u32;
                for d in [egui::vec2(-half, -half), egui::vec2(half, -half), egui::vec2(half, half), egui::vec2(-half, half)] {
                    out.vertices.push(Vertex { pos: pt + d, uv: WHITE_UV, color: col });
                }
                out.indices.extend_from_slice(&[k, k + 1, k + 2, k, k + 2, k + 3]);
            }
        }
        if draw_edges || picked || mode != SelectMode::Island {
            let n = r.len();
            for i in 0..n {
                let (ca, cb) = (r.start + i, r.start + (i + 1) % n);
                let (va, vb) = (m.corners[ca], m.corners[cb]);
                let is_marked = marked.contains(&(va.min(vb), va.max(vb)));
                let edge_sel = csel(ca) && csel(cb);
                if !(draw_edges || picked || edge_sel || is_marked) {
                    continue;
                }
                let (lc, out, half) = if picked || edge_sel {
                    (theme::ACCENT_ORANGE, &mut sel_lines, if edge_sel { 1.2 } else { 0.5 })
                } else if is_marked {
                    (theme::SEAM_MARK, &mut sel_lines, 1.0)
                } else {
                    (Color32::from_white_alpha(if dim { 35 } else { 150 }), &mut lines, 0.5)
                };
                let a = uv.corner_uv[ca];
                let b = uv.corner_uv[cb];
                let (pa, pb) = (to_screen(a[0], a[1]), to_screen(b[0], b[1]));
                let d = pb - pa;
                let len = d.length();
                if len < 0.3 {
                    continue;
                }
                let nrm = egui::vec2(-d.y, d.x) / len * half;
                let k = out.vertices.len() as u32;
                for p in [pa + nrm, pb + nrm, pb - nrm, pa - nrm] {
                    out.vertices.push(Vertex { pos: p, uv: WHITE_UV, color: lc });
                }
                out.indices.extend_from_slice(&[k, k + 1, k + 2, k, k + 2, k + 3]);
            }
        }
    }
    lines.append(sel_lines);
    lines.append(dots);
    lines.append(sel_dots);
    (fill, lines)
}
