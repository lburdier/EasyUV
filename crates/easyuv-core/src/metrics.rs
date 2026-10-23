//! Mesures de qualité d'une carte UV indépendantes de l'algorithme (banc d'essai).

use crate::math::V2;
use crate::mesh::Mesh;

/// Part (0..1) d'un îlot qui se recouvre lui-même (repli de l'îlot sur lui-même), mesurée sur
/// une grille de `res` cellules selon son plus grand côté. Les centres de cellules situés
/// exactement sur une arête ne sont comptés pour aucun des deux triangles (inégalités strictes),
/// pour que des triangles voisins ne soient jamais pris pour un chevauchement.
pub fn chart_self_overlap(tris: &[[u32; 3]], uv: &[V2], res: usize) -> f64 {
    if tris.is_empty() || uv.is_empty() {
        return 0.0;
    }
    let (mut lo, mut hi) = (V2::new(f64::MAX, f64::MAX), V2::new(f64::MIN, f64::MIN));
    for p in uv {
        lo = V2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = V2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let side = (hi.x - lo.x).max(hi.y - lo.y);
    if side <= 0.0 {
        return 0.0;
    }
    let k = res as f64 / side;
    let (w, h) = (((hi.x - lo.x) * k).ceil() as usize + 1, ((hi.y - lo.y) * k).ceil() as usize + 1);
    let mut count = vec![0u8; w * h];
    for t in tris {
        let p: [[f64; 2]; 3] = std::array::from_fn(|j| {
            let q = uv[t[j] as usize];
            [(q.x - lo.x) * k, (q.y - lo.y) * k]
        });
        let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
        if area.abs() < 1e-12 {
            continue;
        }
        let s = area.signum();
        let x0 = p.iter().map(|q| q[0]).fold(f64::MAX, f64::min).floor().max(0.0) as usize;
        let x1 = (p.iter().map(|q| q[0]).fold(f64::MIN, f64::max).ceil() as usize).min(w - 1);
        let y0 = p.iter().map(|q| q[1]).fold(f64::MAX, f64::min).floor().max(0.0) as usize;
        let y1 = (p.iter().map(|q| q[1]).fold(f64::MIN, f64::max).ceil() as usize).min(h - 1);
        let edge = |a: [f64; 2], b: [f64; 2], x: f64, y: f64| (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                let (x, y) = (cx as f64 + 0.5, cy as f64 + 0.5);
                if s * edge(p[0], p[1], x, y) > 0.0 && s * edge(p[1], p[2], x, y) > 0.0 && s * edge(p[2], p[0], x, y) > 0.0 {
                    let c = &mut count[cy * w + cx];
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    let covered = count.iter().filter(|&&c| c >= 1).count();
    let twice = count.iter().filter(|&&c| c >= 2).count();
    if covered == 0 {
        0.0
    } else {
        twice as f64 / covered as f64
    }
}

/// Part (0..1) de la surface UV couverte au moins deux fois, mesurée sur une grille de `res`
/// cellules par tuile (centres de cellules). 0 = aucun chevauchement, ni entre îlots ni à
/// l'intérieur d'un îlot replié. Chaque tuile UDIM est comptée séparément.
pub fn overlap_ratio(mesh: &Mesh, corner_uv: &[[f32; 2]], res: usize) -> f64 {
    let mut count: std::collections::HashMap<(i64, i64), u8> = std::collections::HashMap::new();
    let r = res as f64;
    for f in 0..mesh.face_count() {
        let range = mesh.face_range(f);
        for t in mesh.fan(f) {
            let p: [[f64; 2]; 3] = std::array::from_fn(|k| {
                let c = corner_uv[range.start + t[k]];
                [c[0] as f64 * r, c[1] as f64 * r]
            });
            let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
            if area.abs() < 1e-12 {
                continue;
            }
            let (x0, x1) = (p.iter().map(|q| q[0]).fold(f64::MAX, f64::min), p.iter().map(|q| q[0]).fold(f64::MIN, f64::max));
            let (y0, y1) = (p.iter().map(|q| q[1]).fold(f64::MAX, f64::min), p.iter().map(|q| q[1]).fold(f64::MIN, f64::max));
            let edge = |a: [f64; 2], b: [f64; 2], x: f64, y: f64| (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
            for cy in (y0.floor() as i64)..=(y1.ceil() as i64) {
                for cx in (x0.floor() as i64)..=(x1.ceil() as i64) {
                    let (x, y) = (cx as f64 + 0.5, cy as f64 + 0.5);
                    let (e0, e1, e2) = (edge(p[0], p[1], x, y), edge(p[1], p[2], x, y), edge(p[2], p[0], x, y));
                    let inside = if area > 0.0 { e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0 } else { e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0 };
                    if inside {
                        let c = count.entry((cx, cy)).or_insert(0);
                        *c = c.saturating_add(1);
                    }
                }
            }
        }
    }
    let covered = count.len();
    let twice = count.values().filter(|&&c| c >= 2).count();
    if covered == 0 {
        0.0
    } else {
        twice as f64 / covered as f64
    }
}

/// Score d'une carte UV, de 0 à 100, et ses composantes (chacune de 0 à 1).
#[derive(Clone, Debug, Default)]
pub struct UvScore {
    pub total: f64,
    /// Distorsion moyenne (angles et aires, à l'échelle de chaque îlot) : 0 = parfaite.
    pub distortion: f64,
    /// Part de la surface UV recouverte deux fois.
    pub overlap: f64,
    /// Faces retournées (sens opposé à celui de leur îlot).
    pub flipped: usize,
    /// Part de la tuile 0-1 couverte.
    pub coverage: f64,
    /// Part de la surface UV hors de la tuile 0-1.
    pub outside: f64,
    /// Écart relatif des densités de texels entre îlots (0 = toutes égales).
    pub density_spread: f64,
    /// Longueur des coutures rapportée à la racine de l'aire.
    pub seam_length: f64,
    /// (nom, note 0-1, poids sur 100) de chaque critère, dans l'ordre d'affichage.
    pub parts: Vec<(&'static str, f64, f64)>,
}

impl UvScore {
    /// Critère le plus pénalisant et le conseil qui va avec.
    pub fn advice(&self) -> Option<&'static str> {
        let worst = self.parts.iter().max_by(|a, b| ((1.0 - a.1) * a.2).partial_cmp(&((1.0 - b.1) * b.2)).unwrap_or(std::cmp::Ordering::Equal))?;
        if (1.0 - worst.1) * worst.2 < 1.0 {
            return None;
        }
        Some(match worst.0 {
            "Distorsion" => "Distorsion : pinceau Détendre, Optimiser, ou une couture de plus",
            "Chevauchements" => "Chevauchements : Ranger (Ctrl+P)",
            "Faces retournées" => "Faces retournées : Optimiser ou Redéplier l'îlot",
            "Remplissage" => "Remplissage : Ranger (Ctrl+P) avec une marge plus petite, ou Pivoter les îlots",
            "Dans la tuile" => "UV hors de la tuile 0-1 : Ranger (Ctrl+P) ou Remplir la tuile",
            "Densité égale" => "Densités différentes : Égaliser la densité",
            _ => "Coutures longues : Coudre les îlots voisins",
        })
    }
}

/// Note la carte UV : distorsion, chevauchements, faces retournées, remplissage de la tuile,
/// UV hors tuile, densité de texels égale entre îlots, longueur des coutures.
/// `face_chart` : îlot de chaque face ; `seam_length` : longueur des coutures normalisée.
pub fn uv_score(mesh: &Mesh, corner_uv: &[[f32; 2]], face_chart: &[u32], seam_length: f64, overlap_res: usize) -> UvScore {
    let n_isl = face_chart.iter().map(|&c| c as usize + 1).max().unwrap_or(0);
    // aires par îlot (3D et UV signée) pour l'échelle et le sens de chaque îlot
    let (mut a3, mut au, mut signed) = (vec![0.0; n_isl], vec![0.0; n_isl], vec![0.0; n_isl]);
    struct Tri {
        isl: usize,
        a3: f64,
        au: f64,
        j: [f64; 4],
        out: bool,
    }
    let mut tris = Vec::new();
    for f in 0..mesh.face_count() {
        let r = mesh.face_range(f);
        let isl = face_chart.get(f).copied().unwrap_or(0) as usize;
        for t in mesh.fan(f) {
            let p = |k: usize| mesh.positions[mesh.corners[r.start + t[k]] as usize];
            let u = |k: usize| {
                let c = corner_uv[r.start + t[k]];
                V2::new(c[0] as f64, c[1] as f64)
            };
            let (e1, e2) = (p(1) - p(0), p(2) - p(0));
            let n = e1.cross(e2);
            let l1 = e1.length();
            let area3 = 0.5 * n.length();
            if area3 <= 1e-20 || l1 <= 1e-20 {
                continue;
            }
            let ax = e1 * (1.0 / l1);
            let ay = n.normalized().cross(ax);
            // repère local du triangle 3D : X = [[l1, x2], [0, y2]]
            let (x2, y2) = (e2.dot(ax), e2.dot(ay));
            let (du1, du2) = (u(1) - u(0), u(2) - u(0));
            // J = U · X⁻¹, X⁻¹ = [[1/l1, -x2/(l1 y2)], [0, 1/y2]]
            let j = [du1.x / l1, (du2.x - du1.x * x2 / l1) / y2, du1.y / l1, (du2.y - du1.y * x2 / l1) / y2];
            let sa = 0.5 * du1.cross(du2);
            let c = (u(0) + u(1) + u(2)) * (1.0 / 3.0);
            let out = c.x < 0.0 || c.y < 0.0 || c.x > 1.0 || c.y > 1.0;
            if isl < n_isl {
                a3[isl] += area3;
                au[isl] += sa.abs();
                signed[isl] += sa;
            }
            tris.push(Tri { isl, a3: area3, au: sa, j, out });
        }
    }
    let total3: f64 = a3.iter().sum();
    let totalu: f64 = au.iter().sum();
    let mut s = UvScore { seam_length, ..Default::default() };
    if total3 <= 0.0 || totalu <= 0.0 {
        return s;
    }
    let (mut dist, mut outside) = (0.0, 0.0);
    for t in &tris {
        let scale = (au[t.isl] / a3[t.isl].max(1e-300)).sqrt().max(1e-300);
        let (s1, s2) = crate::math::singular_values_2x2(t.j[0] / scale, t.j[1] / scale, t.j[2] / scale, t.j[3] / scale);
        let flip = t.au * signed[t.isl] < 0.0;
        if flip {
            s.flipped += 1;
        }
        // angles (s1/s2) et aires (s1·s2) ; un triangle écrasé ou retourné compte au maximum
        let e = if s2 <= 1e-9 || flip { 2.0 } else { (0.5 * (s1 / s2).ln() + 0.5 * (s1 * s2).ln().abs()).min(2.0) };
        dist += e * t.a3;
        if t.out {
            outside += t.au.abs();
        }
    }
    s.distortion = dist / total3;
    s.outside = outside / totalu;
    s.coverage = (totalu - outside).min(1.0);
    s.overlap = overlap_ratio(mesh, corner_uv, overlap_res);
    let dens: Vec<(f64, f64)> = (0..n_isl).filter(|&i| a3[i] > 0.0 && au[i] > 0.0).map(|i| ((au[i] / a3[i]).sqrt(), a3[i])).collect();
    let w: f64 = dens.iter().map(|d| d.1).sum();
    let mean = dens.iter().map(|d| d.0 * d.1).sum::<f64>() / w.max(1e-300);
    let var = dens.iter().map(|d| (d.0 - mean).powi(2) * d.1).sum::<f64>() / w.max(1e-300);
    s.density_spread = if mean > 0.0 { var.sqrt() / mean } else { 0.0 };
    let nt = tris.len().max(1) as f64;
    s.parts = vec![
        ("Distorsion", (-4.0 * s.distortion).exp(), 30.0),
        ("Chevauchements", (1.0 - s.overlap * 10.0).clamp(0.0, 1.0), 20.0),
        ("Faces retournées", (1.0 - s.flipped as f64 / nt * 20.0).clamp(0.0, 1.0), 15.0),
        ("Remplissage", (s.coverage / 0.65).clamp(0.0, 1.0), 15.0),
        ("Dans la tuile", (1.0 - s.outside * 4.0).clamp(0.0, 1.0), 5.0),
        ("Densité égale", (-3.0 * s.density_spread).exp(), 10.0),
        ("Coutures", (-seam_length / 12.0).exp(), 5.0),
    ];
    s.total = s.parts.iter().map(|p| p.1 * p.2).sum();
    s
}

#[cfg(test)]
mod score_tests {
    use super::*;
    use crate::{layout::uv_islands, primitives, smart_unwrap, Params};

    #[test]
    fn good_unwrap_scores_high_and_damage_lowers_it() {
        let m = primitives::cylinder(1.0, 2.0, 24, 4);
        let r = smart_unwrap(&m, &Params::default(), &[]);
        let (fi, _) = uv_islands(&m, &r.corner_uv);
        let good = uv_score(&m, &r.corner_uv, &fi, r.stats.seam_length, 256);
        assert!(good.total > 70.0, "score {:.1} {:?}", good.total, good.parts);
        // tuile presque vide, puis une face retournée : le score baisse à chaque fois
        let small: Vec<[f32; 2]> = r.corner_uv.iter().map(|c| [c[0] * 0.2, c[1] * 0.2]).collect();
        let s_small = uv_score(&m, &small, &fi, r.stats.seam_length, 256);
        assert!(s_small.total < good.total - 5.0, "{:.1} vs {:.1}", s_small.total, good.total);
        let mut flipped = r.corner_uv.clone();
        let c0 = m.face_range(0).start;
        flipped.swap(c0, c0 + 1);
        let s_flip = uv_score(&m, &flipped, &fi, r.stats.seam_length, 256);
        assert!(s_flip.flipped >= 1 && s_flip.total < good.total, "{:?}", s_flip.parts);
    }
}
