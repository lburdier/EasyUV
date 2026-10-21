//! Textures de contrôle (damier) : révèlent étirement, densité et orientation des îlots.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckerStyle {
    /// Damier gris classique.
    Mono = 0,
    /// Grille colorée : teinte par colonne, luminosité par ligne, repère « L » d'orientation.
    Color = 1,
}

impl CheckerStyle {
    pub fn from_u32(v: u32) -> Self {
        if v == 1 {
            CheckerStyle::Color
        } else {
            CheckerStyle::Mono
        }
    }
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = (h.fract() + 1.0).fract() * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Couleur RGB (0..1) du damier au point UV (u, v) dans [0, 1], v vers le haut.
pub fn sample(style: CheckerStyle, cells: u32, u: f32, v: f32) -> [f32; 3] {
    let cells = cells.max(1) as f32;
    let (cu, cv) = (u * cells, v * cells);
    let (iu, iv) = (cu.floor(), cv.floor());
    let (fu, fv) = (cu - iu, cv - iv);
    let odd = ((iu as i64 + iv as i64) & 1) == 1;
    // lignes fines de grille
    let line = fu < 0.02 || fv < 0.02 || fu > 0.98 || fv > 0.98;
    match style {
        CheckerStyle::Mono => {
            let mut c = if odd { 0.18 } else { 0.62 };
            // sous-damier 2x2 à faible contraste
            let sub = (((fu * 2.0) as i32 + (fv * 2.0) as i32) & 1) == 1;
            if sub {
                c *= 0.88;
            }
            if line {
                c = 0.92;
            }
            [c, c, c]
        }
        CheckerStyle::Color => {
            let hue = iu / cells;
            let val = 0.55 + 0.4 * (iv / cells);
            let mut c = hsv(hue, if odd { 0.55 } else { 0.35 }, if odd { val * 0.75 } else { val });
            // repère « L » asymétrique en haut à gauche : révèle miroirs et rotations
            let in_bar_h = fu > 0.15 && fu < 0.55 && fv > 0.72 && fv < 0.82;
            let in_bar_v = fu > 0.15 && fu < 0.25 && fv > 0.38 && fv < 0.82;
            if in_bar_h || in_bar_v {
                c = [0.08, 0.08, 0.08];
            }
            if line {
                c = [0.95, 0.95, 0.95];
            }
            c
        }
    }
}

/// Image RGBA8, lignes de haut en bas (convention écran).
pub fn generate_rgba8(size: u32, cells: u32, style: CheckerStyle) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        let v = 1.0 - (y as f32 + 0.5) / size as f32;
        for x in 0..size {
            let u = (x as f32 + 0.5) / size as f32;
            let c = sample(style, cells, u, v);
            out.extend_from_slice(&[(c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, 255]);
        }
    }
    out
}

/// Image RGBA flottante, lignes de bas en haut (convention Blender `Image.pixels`).
pub fn generate_rgba_f32_bottom_up(size: u32, cells: u32, style: CheckerStyle, out: &mut [f32]) {
    for y in 0..size {
        let v = (y as f32 + 0.5) / size as f32;
        for x in 0..size {
            let u = (x as f32 + 0.5) / size as f32;
            let c = sample(style, cells, u, v);
            let i = ((y * size + x) * 4) as usize;
            out[i] = c[0];
            out[i + 1] = c[1];
            out[i + 2] = c[2];
            out[i + 3] = 1.0;
        }
    }
}
