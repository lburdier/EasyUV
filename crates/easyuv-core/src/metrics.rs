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

/// Part (0..1) de la surface UV couverte au moins deux fois, mesurée sur une grille
/// `res`×`res` (centres de cellules). 0 = aucun chevauchement, ni entre îlots ni à l'intérieur
/// d'un îlot qui se replie sur lui-même. Les UV hors de la tuile 0-1 sont ramenées par modulo.
pub fn overlap_ratio(mesh: &Mesh, corner_uv: &[[f32; 2]], res: usize) -> f64 {
    let mut count = vec![0u8; res * res];
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
                        let (ix, iy) = (cx.rem_euclid(res as i64) as usize, cy.rem_euclid(res as i64) as usize);
                        let c = &mut count[iy * res + ix];
                        *c = c.saturating_add(1);
                    }
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
