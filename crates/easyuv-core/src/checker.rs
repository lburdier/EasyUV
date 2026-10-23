//! Textures de contrôle (damier) : révèlent étirement, densité et orientation des îlots.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckerStyle {
    /// Damier gris classique.
    Mono = 0,
    /// Grille colorée : teinte par colonne, luminosité par ligne, repère « L » d'orientation.
    Color = 1,
    /// Cases étiquetées « colonne + rangée » (A1, B2…) : retrouver une zone de l'UV sur le modèle.
    Numbered = 2,
    /// Un cercle par case : il devient ellipse là où les UV sont étirées.
    Circles = 3,
    /// Une flèche vers le haut (+V) par case : rotations et miroirs sautent aux yeux.
    Arrows = 4,
    /// Dégradé U en rouge, V en vert : position dans la tuile et coutures.
    Gradient = 5,
    /// Diagonales à 45° : un cisaillement les fait pencher ou onduler.
    Diagonal = 6,
}

impl CheckerStyle {
    pub const ALL: [CheckerStyle; 7] = [
        CheckerStyle::Color,
        CheckerStyle::Mono,
        CheckerStyle::Numbered,
        CheckerStyle::Circles,
        CheckerStyle::Arrows,
        CheckerStyle::Gradient,
        CheckerStyle::Diagonal,
    ];

    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => CheckerStyle::Color,
            2 => CheckerStyle::Numbered,
            3 => CheckerStyle::Circles,
            4 => CheckerStyle::Arrows,
            5 => CheckerStyle::Gradient,
            6 => CheckerStyle::Diagonal,
            _ => CheckerStyle::Mono,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CheckerStyle::Mono => "Mono",
            CheckerStyle::Color => "Couleur",
            CheckerStyle::Numbered => "Numéroté",
            CheckerStyle::Circles => "Cercles",
            CheckerStyle::Arrows => "Flèches",
            CheckerStyle::Gradient => "Dégradé UV",
            CheckerStyle::Diagonal => "Diagonales",
        }
    }

    /// À quoi sert ce damier.
    pub fn help(self) -> &'static str {
        match self {
            CheckerStyle::Mono => "Damier gris neutre : densité de texels et étirement.",
            CheckerStyle::Color => "Couleur par colonne et par rangée, repère « L » : orientation et position.",
            CheckerStyle::Numbered => "Chaque case porte ses coordonnées (A1, B2…) : on retrouve une zone de l'UV sur le modèle.",
            CheckerStyle::Circles => "Un cercle par case : les ellipses montrent l'étirement et sa direction.",
            CheckerStyle::Arrows => "Une flèche vers +V par case : îlots tournés ou retournés en miroir.",
            CheckerStyle::Gradient => "Rouge = U, vert = V : position dans la tuile ; une couture se voit comme une cassure.",
            CheckerStyle::Diagonal => "Lignes à 45° : un cisaillement les fait pencher ou onduler.",
        }
    }
}

/// Glyphes 3 × 5 (une rangée par élément, bit 2 = colonne de gauche), lettres puis chiffres.
const GLYPHS: [[u8; 5]; 36] = [
    [2, 5, 7, 5, 5], // A
    [6, 5, 6, 5, 6], // B
    [3, 4, 4, 4, 3], // C
    [6, 5, 5, 5, 6], // D
    [7, 4, 6, 4, 7], // E
    [7, 4, 6, 4, 4], // F
    [3, 4, 5, 5, 3], // G
    [5, 5, 7, 5, 5], // H
    [7, 2, 2, 2, 7], // I
    [1, 1, 1, 5, 2], // J
    [5, 5, 6, 5, 5], // K
    [4, 4, 4, 4, 7], // L
    [5, 7, 7, 5, 5], // M
    [6, 5, 5, 5, 5], // N
    [2, 5, 5, 5, 2], // O
    [6, 5, 6, 4, 4], // P
    [2, 5, 5, 6, 3], // Q
    [6, 5, 6, 5, 5], // R
    [3, 4, 2, 1, 6], // S
    [7, 2, 2, 2, 2], // T
    [5, 5, 5, 5, 7], // U
    [5, 5, 5, 5, 2], // V
    [5, 5, 7, 7, 5], // W
    [5, 5, 2, 5, 5], // X
    [5, 5, 2, 2, 2], // Y
    [7, 1, 2, 4, 7], // Z
    [7, 5, 5, 5, 7], // 0
    [2, 6, 2, 2, 7], // 1
    [6, 1, 2, 4, 7], // 2
    [6, 1, 2, 1, 6], // 3
    [5, 5, 7, 1, 1], // 4
    [7, 4, 6, 1, 6], // 5
    [3, 4, 6, 5, 2], // 6
    [7, 1, 2, 2, 2], // 7
    [2, 5, 2, 5, 2], // 8
    [2, 5, 3, 1, 6], // 9
];

/// Étiquette d'une case : lettres de colonne (A..Z, puis AA..) et numéro de rangée (1..).
fn cell_label(col: u32, row: u32) -> Vec<usize> {
    let mut g = Vec::new();
    if col >= 26 {
        g.push((col / 26 - 1) as usize % 26);
    }
    g.push((col % 26) as usize);
    for ch in (row + 1).to_string().bytes() {
        g.push(26 + (ch - b'0') as usize);
    }
    g
}

/// Vrai si le point (x, y) de la case (0..1, y vers le haut) tombe sur un pixel de l'étiquette,
/// écrite en haut à gauche.
fn on_label(glyphs: &[usize], x: f32, y: f32) -> bool {
    let px = 0.07;
    let (x0, y0) = (0.08, 0.92);
    let (gx, gy) = ((x - x0) / px, (y0 - y) / px);
    if gx < 0.0 || gy < 0.0 || gy >= 5.0 {
        return false;
    }
    let (k, cx) = ((gx / 4.0) as usize, gx % 4.0);
    if k >= glyphs.len() || cx >= 3.0 {
        return false;
    }
    (GLYPHS[glyphs[k]][gy as usize] >> (2 - cx as u8)) & 1 == 1
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
        CheckerStyle::Numbered => {
            let hue = iu / cells;
            let mut c = hsv(hue + iv * 0.37 / cells, 0.35, if odd { 0.55 } else { 0.78 });
            if on_label(&cell_label(iu as u32, iv as u32), fu, fv) {
                c = [0.05, 0.05, 0.05];
            }
            if line {
                c = [0.95, 0.95, 0.95];
            }
            c
        }
        CheckerStyle::Circles => {
            let mut c = if odd { [0.24, 0.27, 0.32] } else { [0.32, 0.36, 0.42] };
            let r = ((fu - 0.5).powi(2) + (fv - 0.5).powi(2)).sqrt();
            if (r - 0.36).abs() < 0.035 {
                c = [0.98, 0.8, 0.3];
            }
            if r < 0.05 {
                c = [0.95, 0.95, 0.95];
            }
            if line {
                c = [0.6, 0.62, 0.66];
            }
            c
        }
        CheckerStyle::Arrows => {
            let hue = iv / cells;
            let mut c = hsv(hue, 0.4, if odd { 0.5 } else { 0.68 });
            // flèche vers +V : hampe centrée, pointe en haut ; ergot à droite (+U) pour les miroirs
            let shaft = (fu - 0.5).abs() < 0.06 && fv > 0.2 && fv < 0.62;
            let head = (0.62..0.85).contains(&fv) && (fu - 0.5).abs() < (0.85 - fv) * 0.9;
            let tick = fv > 0.2 && fv < 0.3 && fu > 0.5 && fu < 0.72;
            if shaft || head || tick {
                c = [0.97, 0.97, 0.97];
            }
            if line {
                c = [0.15, 0.15, 0.15];
            }
            c
        }
        CheckerStyle::Gradient => {
            let mut c = [u.clamp(0.0, 1.0), v.clamp(0.0, 1.0), 0.25];
            if odd {
                c = c.map(|x| x * 0.85);
            }
            if line {
                c = [0.95, 0.95, 0.95];
            }
            c
        }
        CheckerStyle::Diagonal => {
            let mut c = if odd { [0.2, 0.2, 0.22] } else { [0.42, 0.42, 0.45] };
            if (fu + fv).fract() < 0.12 {
                c = [0.95, 0.6, 0.25];
            }
            if (fu - fv + 1.0).fract() < 0.06 {
                c = [0.55, 0.75, 0.95];
            }
            if line {
                c = [0.9, 0.9, 0.9];
            }
            c
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_draws_something_different() {
        let imgs: Vec<Vec<u8>> = CheckerStyle::ALL.iter().map(|&st| generate_rgba8(64, 8, st)).collect();
        for i in 0..imgs.len() {
            for j in i + 1..imgs.len() {
                assert_ne!(imgs[i], imgs[j], "{:?} et {:?} identiques", CheckerStyle::ALL[i], CheckerStyle::ALL[j]);
            }
        }
        for st in CheckerStyle::ALL {
            assert_eq!(CheckerStyle::from_u32(st as u32), st);
        }
    }

    #[test]
    fn labels_read_column_then_row() {
        // A1 en bas à gauche, B1 à sa droite, A2 au-dessus
        assert_eq!(cell_label(0, 0), vec![0, 27]);
        assert_eq!(cell_label(1, 0), vec![1, 27]);
        assert_eq!(cell_label(0, 9), vec![0, 27, 26]);
        assert_eq!(cell_label(27, 0), vec![0, 1, 27]);
    }
}
