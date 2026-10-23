//! Icônes des outils (dessinées : la police n'en a pas) et aperçus animés qui expliquent
//! chaque outil quand le pointeur reste dessus, comme les infobulles vidéo de Blender.

use crate::theme;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Vertex,
    Edge,
    Face,
    Island,
    Select,
    Grab,
    Relax,
    Straighten,
    Pinch,
    Rect,
    Cut,
    Sew,
    Mark,
    Unwrap,
}

/// Dessine l'icône dans `r`.
pub fn paint(p: &egui::Painter, r: Rect, icon: Icon, col: Color32) {
    let c = r.center();
    let s = r.width().min(r.height()) * 0.5 - 2.0;
    let st = Stroke::new(1.5, col);
    let thin = Stroke::new(1.0, col.gamma_multiply(0.55));
    let at = |x: f32, y: f32| c + egui::vec2(x * s, y * s);
    // petit carré de grille, base de plusieurs icônes
    let square = |stroke: Stroke| {
        p.rect_stroke(Rect::from_min_max(at(-0.7, -0.7), at(0.7, 0.7)), 1.0, stroke, egui::StrokeKind::Middle);
    };
    match icon {
        Icon::Vertex => {
            square(thin);
            for (x, y) in [(-0.7, -0.7), (0.7, -0.7), (0.7, 0.7)] {
                p.circle_filled(at(x, y), 1.6, col.gamma_multiply(0.6));
            }
            p.circle_filled(at(-0.7, 0.7), 3.0, theme::ACCENT_ORANGE);
        }
        Icon::Edge => {
            square(thin);
            p.line_segment([at(-0.7, 0.7), at(0.7, 0.7)], Stroke::new(2.5, theme::ACCENT_ORANGE));
        }
        Icon::Face => {
            p.rect_filled(Rect::from_min_max(at(-0.7, -0.7), at(0.7, 0.7)), 1.0, theme::ACCENT_ORANGE.gamma_multiply(0.7));
            square(st);
        }
        Icon::Island => {
            let pts = vec![at(-0.8, -0.2), at(-0.3, -0.8), at(0.5, -0.6), at(0.8, 0.2), at(0.2, 0.8), at(-0.6, 0.5)];
            p.add(egui::Shape::convex_polygon(pts.clone(), theme::ACCENT_ORANGE.gamma_multiply(0.5), st));
        }
        Icon::Select => {
            let pts = vec![at(-0.6, -0.9), at(-0.6, 0.6), at(-0.2, 0.25), at(0.1, 0.9), at(0.35, 0.8), at(0.05, 0.15), at(0.55, 0.15)];
            p.add(egui::Shape::closed_line(pts, st));
        }
        Icon::Grab => {
            p.circle_stroke(c, s * 0.85, thin);
            let tip = at(0.75, 0.0);
            p.line_segment([at(-0.6, 0.0), tip], st);
            p.line_segment([tip, at(0.4, -0.3)], st);
            p.line_segment([tip, at(0.4, 0.3)], st);
        }
        Icon::Relax => {
            p.circle_stroke(c, s * 0.85, thin);
            let wave: Vec<Pos2> = (0..=12).map(|i| {
                let x = -0.7 + 1.4 * i as f32 / 12.0;
                at(x, -0.35 + 0.25 * (x * 9.0).sin())
            }).collect();
            p.add(egui::Shape::line(wave, thin));
            p.line_segment([at(-0.7, 0.35), at(0.7, 0.35)], st);
        }
        Icon::Straighten => {
            let zig = vec![at(-0.8, -0.2), at(-0.4, -0.6), at(0.0, -0.2), at(0.4, -0.6), at(0.8, -0.2)];
            p.add(egui::Shape::line(zig, thin));
            p.line_segment([at(-0.8, 0.45), at(0.8, 0.45)], st);
            p.line_segment([at(0.0, -0.05), at(0.0, 0.3)], thin);
        }
        Icon::Pinch => {
            p.circle_stroke(c, s * 0.85, thin);
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let (a, b) = (at(dx * 0.75, dy * 0.75), at(dx * 0.25, dy * 0.25));
                p.line_segment([a, b], st);
            }
            p.circle_filled(c, 1.8, col);
        }
        Icon::Rect => {
            let r2 = Rect::from_min_max(at(-0.85, -0.5), at(0.85, 0.5));
            dashed_rect(p, r2, Stroke::new(1.5, theme::ACCENT_ORANGE));
            p.line_segment([at(-0.55, 0.0), at(0.55, 0.0)], st);
        }
        Icon::Cut => {
            // ciseaux : deux anneaux et deux lames croisées
            p.circle_stroke(at(-0.45, 0.55), s * 0.25, st);
            p.circle_stroke(at(0.45, 0.55), s * 0.25, st);
            p.line_segment([at(-0.3, 0.35), at(0.55, -0.8)], st);
            p.line_segment([at(0.3, 0.35), at(-0.55, -0.8)], st);
        }
        Icon::Sew => {
            p.line_segment([at(-0.8, 0.0), at(0.8, 0.0)], thin);
            for i in 0..4 {
                let x = -0.6 + 0.4 * i as f32;
                p.line_segment([at(x - 0.12, -0.35), at(x + 0.12, 0.35)], st);
            }
        }
        Icon::Mark => {
            square(thin);
            p.line_segment([at(-0.7, -0.7), at(0.7, 0.7)], Stroke::new(2.5, theme::SEAM_MARK));
        }
        Icon::Unwrap => {
            // un cube qui s'ouvre en croix
            let k = 0.32;
            for (x, y) in [(0.0, -1.0), (-1.0, 0.0), (0.0, 0.0), (1.0, 0.0), (0.0, 1.0)] {
                let o = egui::vec2(x * k * 2.0, y * k * 2.0);
                let rr = Rect::from_center_size(at(o.x, o.y), Vec2::splat(s * k * 2.0 - 1.0));
                let f = if (x, y) == (0.0, 0.0) { theme::ACCENT_ORANGE.gamma_multiply(0.6) } else { Color32::TRANSPARENT };
                p.rect_filled(rr, 0.0, f);
                p.rect_stroke(rr, 0.0, st, egui::StrokeKind::Middle);
            }
        }
    }
}

fn dashed_rect(p: &egui::Painter, r: Rect, st: Stroke) {
    let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    for w in pts.windows(2) {
        p.add(egui::Shape::dashed_line(&[w[0], w[1]], st, 4.0, 3.0));
    }
}

/// Bouton-icône (sélectionnable) ; infobulle : titre, raccourci, explication et aperçu animé.
pub fn button(ui: &mut egui::Ui, icon: Icon, selected: bool, title: &str, key: &str, help: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 22.0), egui::Sense::click());
    let bg = if selected {
        theme::ACCENT_BLUE
    } else if resp.hovered() {
        theme::WIDGET_HOVER
    } else {
        theme::WIDGET
    };
    ui.painter().rect_filled(rect, 4.0, bg);
    paint(ui.painter(), rect.shrink(2.0), icon, if selected || resp.hovered() { Color32::WHITE } else { theme::TEXT });
    resp.on_hover_ui(|ui| tooltip(ui, icon, title, key, help))
}

/// Infobulle d'outil : titre, raccourci, explication, aperçu animé.
pub fn tooltip(ui: &mut egui::Ui, icon: Icon, title: &str, key: &str, help: &str) {
    ui.set_max_width(260.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong());
        if !key.is_empty() {
            ui.label(egui::RichText::new(key).small().color(theme::TEXT_DIM));
        }
    });
    ui.label(egui::RichText::new(help).small());
    ui.add_space(4.0);
    preview(ui, icon);
}

/// Aperçu animé (boucle de 3 s) de ce que fait l'outil sur une petite grille d'UV.
pub fn preview(ui: &mut egui::Ui, icon: Icon) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(240.0, 110.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, theme::BG_UV);
    let time = ui.input(|i| i.time);
    ui.ctx().request_repaint();
    // 0..1 : aller (1,2 s), pause, retour (1,2 s), pause
    let phase = (time % 3.4) as f32;
    let t = match phase {
        x if x < 0.3 => 0.0,
        x if x < 1.5 => ease((x - 0.3) / 1.2),
        x if x < 2.0 => 1.0,
        x if x < 3.2 => 1.0 - ease((x - 2.0) / 1.2),
        _ => 0.0,
    };
    let (nx, ny) = (8usize, 3usize);
    let area = rect.shrink2(egui::vec2(24.0, 22.0));
    let cell = egui::vec2(area.width() / nx as f32, area.height() / ny as f32);
    let flat = |i: usize, j: usize| area.left_top() + egui::vec2(i as f32 * cell.x, j as f32 * cell.y);
    let center = area.center();
    let jitter = |i: usize, j: usize| {
        let h = ((i * 73 + j * 151) % 97) as f32 / 97.0 - 0.5;
        let g = ((i * 37 + j * 59) % 89) as f32 / 89.0 - 0.5;
        egui::vec2(h, g) * cell.x * 0.6
    };
    let falloff = |q: Pos2, c: Pos2, r: f32| {
        let d = (q - c).length() / r;
        if d >= 1.0 { 0.0 } else { let k = 1.0 - d; k * k * (3.0 - 2.0 * k) }
    };
    let mut highlight: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut fill_cells: Vec<(usize, usize)> = Vec::new();
    let mut dots: Vec<(usize, usize)> = Vec::new();
    let mut brush: Option<(Pos2, f32)> = None;
    let mut draw_rect: Option<Rect> = None;
    let mut split_col: Option<usize> = None;
    let pos = |i: usize, j: usize| -> Pos2 {
        let f = flat(i, j);
        match icon {
            Icon::Grab => {
                let c = center + egui::vec2(-cell.x, 0.0);
                f + egui::vec2(cell.x * 1.6, -cell.y * 0.4) * (falloff(f, c, cell.x * 2.6) * t)
            }
            Icon::Relax => f + jitter(i, j) * (1.0 - t),
            Icon::Straighten => f + egui::vec2(0.0, (i as f32 * 1.3).sin() * cell.y * 0.45 * (1.0 - t)),
            Icon::Pinch => f + (center - f) * (falloff(f, center, cell.x * 3.2) * 0.45 * t),
            Icon::Rect | Icon::Unwrap => {
                // arc de cône -> bande droite
                let u = i as f32 / nx as f32 - 0.5;
                let rr = area.height() * 1.6 + (ny - j) as f32 * cell.y;
                let arc = Pos2::new(center.x, area.top() + area.height() * 1.9) + egui::vec2((u * 1.6).sin(), -(u * 1.6).cos()) * rr;
                let target = if icon == Icon::Rect { Pos2::new(area.left() + i as f32 * area.width() / nx as f32, area.top() + 8.0 + j as f32 * (area.height() - 16.0) / ny as f32) } else { f };
                arc + (target - arc) * t
            }
            // l'îlot de droite (colonnes nx/2 et au-delà, bord de coupe compris) s'écarte
            Icon::Cut => {
                let gap = if i >= nx / 2 { cell.x * 0.8 * t } else { 0.0 };
                f + egui::vec2(gap, 0.0)
            }
            Icon::Sew => {
                let gap = if i >= nx / 2 { cell.x * 0.8 * (1.0 - t) } else { 0.0 };
                f + egui::vec2(gap, if i >= nx / 2 { -cell.y * 0.5 * (1.0 - t) } else { 0.0 })
            }
            _ => f,
        }
    };
    match icon {
        Icon::Vertex => dots.push((3, 1)),
        Icon::Edge | Icon::Mark => highlight.extend((0..ny).map(|j| (4, j, 4, j + 1))),
        Icon::Face => fill_cells.extend([(2, 1), (3, 1), (3, 0)]),
        Icon::Island | Icon::Select => fill_cells.extend((0..nx).flat_map(|i| (0..ny).map(move |j| (i, j)))),
        Icon::Grab => brush = Some((center + egui::vec2(-cell.x, 0.0) + egui::vec2(cell.x * 1.6, -cell.y * 0.4) * t, cell.x * 2.6)),
        Icon::Relax | Icon::Straighten => brush = Some((Pos2::new(area.left() + area.width() * t, center.y), cell.x * 1.8)),
        Icon::Pinch => brush = Some((center, cell.x * 3.2)),
        Icon::Rect => draw_rect = Some(Rect::from_min_max(area.left_top() + egui::vec2(0.0, 8.0), Pos2::new(area.left() + area.width() * t.max(0.05), area.bottom() - 8.0))),
        Icon::Cut | Icon::Sew => split_col = Some(nx / 2),
        Icon::Unwrap => fill_cells.extend((0..nx).flat_map(|i| (0..ny).map(move |j| (i, j)))),
    }
    // faces surlignées
    for &(i, j) in &fill_cells {
        let q = [pos(i, j), pos(i + 1, j), pos(i + 1, j + 1), pos(i, j + 1)];
        p.add(egui::Shape::convex_polygon(q.to_vec(), theme::ACCENT_ORANGE.gamma_multiply(0.35), Stroke::NONE));
    }
    // grille (en deux îlots pour Couper / Coudre : la colonne de coupe existe des deux côtés)
    let line = Stroke::new(1.0, Color32::from_white_alpha(170));
    let grid = |pf: &dyn Fn(usize, usize) -> Pos2, i0: usize, i1: usize| {
        for j in 0..=ny {
            for i in i0..=i1 {
                if i < i1 {
                    p.line_segment([pf(i, j), pf(i + 1, j)], line);
                }
                if j < ny {
                    p.line_segment([pf(i, j), pf(i, j + 1)], line);
                }
            }
        }
    };
    match split_col {
        Some(sc) => {
            grid(&|i, j| if i >= sc { flat(i, j) } else { pos(i, j) }, 0, sc);
            grid(&pos, sc, nx);
        }
        None => grid(&pos, 0, nx),
    }
    // coupe / couture : les deux bords de la coupe
    if let Some(sc) = split_col {
        let left: Vec<Pos2> = (0..=ny).map(|j| flat(sc, j)).collect();
        let right: Vec<Pos2> = (0..=ny).map(|j| pos(sc, j)).collect();
        p.add(egui::Shape::line(left, Stroke::new(2.0, theme::ACCENT_ORANGE)));
        p.add(egui::Shape::line(right, Stroke::new(2.0, theme::ACCENT_ORANGE)));
    }
    for &(a, b, c, d) in &highlight {
        let colr = if icon == Icon::Mark { theme::SEAM_MARK } else { theme::ACCENT_ORANGE };
        p.line_segment([pos(a, b), pos(c, d)], Stroke::new(2.5, colr));
    }
    for &(i, j) in &dots {
        p.circle_filled(pos(i, j), 4.0, theme::ACCENT_ORANGE);
    }
    if icon == Icon::Vertex {
        for j in 0..=ny {
            for i in 0..=nx {
                p.circle_filled(pos(i, j), 1.5, Color32::from_white_alpha(200));
            }
        }
    }
    if let Some((c, r)) = brush {
        p.circle_stroke(c, r, Stroke::new(1.5, Color32::WHITE));
        p.circle_stroke(c, r * 0.5, Stroke::new(1.0, Color32::from_white_alpha(60)));
    }
    if let Some(r) = draw_rect {
        dashed_rect(&p, r, Stroke::new(1.5, theme::ACCENT_ORANGE));
    }
    let caption = match icon {
        Icon::Vertex => "clic : un sommet · Maj : ajouter",
        Icon::Edge => "clic : une arête · double-clic : la boucle",
        Icon::Face => "clic : une face · rectangle : plusieurs",
        Icon::Island => "clic : un îlot entier",
        Icon::Select => "clic, rectangle, glisser, G R S",
        Icon::Grab => "glisser : tirer les UV",
        Icon::Relax => "peindre : l'étirement disparaît",
        Icon::Straighten => "peindre : les lignes se redressent",
        Icon::Pinch => "peindre : resserrer (Maj : écarter)",
        Icon::Rect => "dessiner le rectangle voulu",
        Icon::Cut => "les arêtes choisies deviennent couture",
        Icon::Sew => "les deux bords se rejoignent",
        Icon::Mark => "couture imposée au prochain dépliage",
        Icon::Unwrap => "seule la sélection est redépliée",
    };
    p.text(rect.center_bottom() - egui::vec2(0.0, 6.0), egui::Align2::CENTER_BOTTOM, caption, egui::FontId::proportional(11.0), theme::TEXT_DIM);
}

fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
