//! Dépliage d'un îlot disque : LSCM (conforme) puis relaxation ARAP (préserve les aires).

use crate::chart::Chart;
use crate::math::{closest_rotation_2x2, singular_values_2x2, V2, V3};
use crate::solver::{cgls, pcg, CsrBuilder, SymSparse};

/// Coordonnées 2D isométriques d'un triangle 3D dans son propre plan.
fn local_frame(p0: V3, p1: V3, p2: V3) -> Option<[V2; 3]> {
    let e1 = p1 - p0;
    let e2 = p2 - p0;
    let n = e1.cross(e2);
    let l1 = e1.length();
    if n.length() < 1e-14 * (l1 * l1).max(1e-300) || l1 < 1e-14 {
        return None;
    }
    let ax = e1 / l1;
    let ay = n.normalized().cross(ax);
    Some([V2::new(0.0, 0.0), V2::new(l1, 0.0), V2::new(e2.dot(ax), e2.dot(ay))])
}

pub struct UnwrapQuality {
    /// Distorsion moyenne pondérée par l'aire : 0 = isométrie parfaite.
    pub distortion: f64,
    pub flipped: usize,
    pub area3d: f64,
}

/// Plus grande paire de sommets éloignés parmi un ensemble (heuristique en 2 passes).
fn far_pair(pos: &[V3], cand: &[u32]) -> (u32, u32) {
    let start = cand[0];
    let far = |from: u32| -> u32 {
        let p = pos[from as usize];
        *cand
            .iter()
            .max_by(|&&a, &&b| {
                let da = (pos[a as usize] - p).length();
                let db = (pos[b as usize] - p).length();
                da.partial_cmp(&db).unwrap()
            })
            .unwrap()
    };
    let a = far(start);
    let b = far(a);
    (a, b)
}

/// Projection planaire sur le plan moyen (estimation initiale).
fn planar_projection(chart: &Chart) -> Vec<V2> {
    let mut n = V3::ZERO;
    for t in &chart.tris {
        let p = [chart.pos[t[0] as usize], chart.pos[t[1] as usize], chart.pos[t[2] as usize]];
        n = n + (p[1] - p[0]).cross(p[2] - p[0]);
    }
    let n = if n.length() > 1e-12 { n.normalized() } else { V3::new(0.0, 0.0, 1.0) };
    let u = n.any_orthogonal();
    let v = n.cross(u);
    chart.pos.iter().map(|p| V2::new(p.dot(u), p.dot(v))).collect()
}

/// LSCM (Lévy et al. 2002) avec deux sommets épinglés.
pub fn lscm(chart: &Chart, max_iter: usize) -> Vec<V2> {
    let n = chart.pos.len();
    let init = planar_projection(chart);
    if n < 3 || chart.tris.is_empty() {
        return init;
    }
    let cand: Vec<u32> = if let Some(l) = chart.loops.first() { l.clone() } else { (0..n as u32).collect() };
    let (pa, pb) = far_pair(&chart.pos, &cand);
    if pa == pb {
        return init;
    }
    // Épingles : on garde leur position projetée pour une initialisation cohérente.
    let mut fixed = vec![None; 2 * n];
    fixed[pa as usize] = Some(init[pa as usize].x);
    fixed[n + pa as usize] = Some(init[pa as usize].y);
    fixed[pb as usize] = Some(init[pb as usize].x);
    fixed[n + pb as usize] = Some(init[pb as usize].y);
    let mut free_idx = vec![u32::MAX; 2 * n];
    let mut nfree = 0usize;
    for i in 0..2 * n {
        if fixed[i].is_none() {
            free_idx[i] = nfree as u32;
            nfree += 1;
        }
    }
    let mut a = CsrBuilder::new(nfree);
    let mut rhs = Vec::with_capacity(chart.tris.len() * 2);
    for t in &chart.tris {
        let p = [chart.pos[t[0] as usize], chart.pos[t[1] as usize], chart.pos[t[2] as usize]];
        let Some(x) = local_frame(p[0], p[1], p[2]) else { continue };
        let area2 = (x[1] - x[0]).cross(x[2] - x[0]);
        if area2 <= 0.0 {
            continue;
        }
        let w = 1.0 / area2.sqrt();
        // W_j = (x_{j+2} - x_{j+1}) (complexe)
        let coef: [(f64, f64); 3] = std::array::from_fn(|j| {
            let d = x[(j + 2) % 3] - x[(j + 1) % 3];
            (d.x * w, d.y * w)
        });
        // Ligne réelle : sum (a u - b v) ; ligne imaginaire : sum (b u + a v)
        for part in 0..2 {
            let mut b = 0.0;
            for j in 0..3 {
                let vi = t[j] as usize;
                let (ca, cb) = coef[j];
                let (cu, cv) = if part == 0 { (ca, -cb) } else { (cb, ca) };
                for (var, c) in [(vi, cu), (n + vi, cv)] {
                    if let Some(val) = fixed[var] {
                        b -= c * val;
                    } else {
                        a.push(free_idx[var] as usize, c);
                    }
                }
            }
            a.end_row();
            rhs.push(b);
        }
    }
    let a = a.build();
    let mut x = vec![0.0; nfree];
    for i in 0..2 * n {
        if fixed[i].is_none() {
            let v = if i < n { init[i].x } else { init[i - n].y };
            x[free_idx[i] as usize] = v;
        }
    }
    cgls(&a, &rhs, &mut x, max_iter, 1e-10);
    let get = |i: usize| fixed[i].unwrap_or_else(|| x[free_idx[i] as usize]);
    let mut uv: Vec<V2> = (0..n).map(|i| V2::new(get(i), get(n + i))).collect();
    orient_positive(chart, &mut uv);
    uv
}

/// Corrige un dépliage miroir (aire signée totale négative).
fn orient_positive(chart: &Chart, uv: &mut [V2]) {
    let signed: f64 = chart
        .tris
        .iter()
        .map(|t| (uv[t[1] as usize] - uv[t[0] as usize]).cross(uv[t[2] as usize] - uv[t[0] as usize]))
        .sum();
    if signed < 0.0 {
        for p in uv.iter_mut() {
            p.x = -p.x;
        }
    }
}

/// Relaxation ARAP local/global (Liu et al. 2008) : réduit l'étirement d'aire.
pub fn arap(chart: &Chart, uv: &mut [V2], iterations: usize, max_cg: usize) {
    let n = chart.pos.len();
    if iterations == 0 || n < 3 {
        return;
    }
    struct Tri {
        v: [usize; 3],
        x: [V2; 3],
        cot: [f64; 3], // cot[k] = poids de l'arête opposée au sommet k (k+1, k+2)
    }
    let mut tris = Vec::with_capacity(chart.tris.len());
    for t in &chart.tris {
        let p = [chart.pos[t[0] as usize], chart.pos[t[1] as usize], chart.pos[t[2] as usize]];
        let Some(x) = local_frame(p[0], p[1], p[2]) else { continue };
        let cot = std::array::from_fn(|k| {
            let a = x[(k + 1) % 3] - x[k];
            let b = x[(k + 2) % 3] - x[k];
            let c = a.dot(b) / a.cross(b).abs().max(1e-300);
            (0.5 * c).clamp(1e-4, 1e4)
        });
        tris.push(Tri { v: [t[0] as usize, t[1] as usize, t[2] as usize], x, cot });
    }
    if tris.is_empty() {
        return;
    }
    // Laplacien cotangent
    let mut lap = SymSparse { diag: vec![0.0; n], nbr: vec![Vec::new(); n] };
    {
        let mut acc: std::collections::HashMap<(usize, usize), f64> = std::collections::HashMap::new();
        for t in &tris {
            for k in 0..3 {
                let i = t.v[(k + 1) % 3];
                let j = t.v[(k + 2) % 3];
                let key = if i < j { (i, j) } else { (j, i) };
                *acc.entry(key).or_insert(0.0) += t.cot[k];
            }
        }
        for (&(i, j), &w) in &acc {
            lap.diag[i] += w;
            lap.diag[j] += w;
            lap.nbr[i].push((j as u32, -w));
            lap.nbr[j].push((i as u32, -w));
        }
    }
    // Épingle douce sur le sommet 0 (supprime l'invariance par translation)
    let anchor = 0usize;
    let lam = lap.diag.iter().sum::<f64>() / n as f64;
    lap.diag[anchor] += lam;

    let mut bu = vec![0.0; n];
    let mut bv = vec![0.0; n];
    let mut xu: Vec<f64> = uv.iter().map(|p| p.x).collect();
    let mut xv: Vec<f64> = uv.iter().map(|p| p.y).collect();
    for _ in 0..iterations {
        bu.iter_mut().for_each(|v| *v = 0.0);
        bv.iter_mut().for_each(|v| *v = 0.0);
        for t in &tris {
            // Étape locale : meilleure rotation
            let (mut a, mut b, mut c, mut d) = (0.0, 0.0, 0.0, 0.0);
            for k in 0..3 {
                let i = t.v[(k + 1) % 3];
                let j = t.v[(k + 2) % 3];
                let dx = t.x[(k + 1) % 3] - t.x[(k + 2) % 3];
                let du = V2::new(xu[i] - xu[j], xv[i] - xv[j]);
                let w = t.cot[k];
                a += w * du.x * dx.x;
                b += w * du.x * dx.y;
                c += w * du.y * dx.x;
                d += w * du.y * dx.y;
            }
            let (cs, sn) = closest_rotation_2x2(a, b, c, d);
            // Second membre de l'étape globale
            for k in 0..3 {
                let i = t.v[(k + 1) % 3];
                let j = t.v[(k + 2) % 3];
                let dx = (t.x[(k + 1) % 3] - t.x[(k + 2) % 3]).rotated(cs, sn) * t.cot[k];
                bu[i] += dx.x;
                bv[i] += dx.y;
                bu[j] -= dx.x;
                bv[j] -= dx.y;
            }
        }
        bu[anchor] += lam * xu[anchor];
        bv[anchor] += lam * xv[anchor];
        pcg(&lap, &bu, &mut xu, max_cg, 1e-8);
        pcg(&lap, &bv, &mut xv, max_cg, 1e-8);
    }
    for i in 0..n {
        uv[i] = V2::new(xu[i], xv[i]);
    }
}

/// Mesure la qualité d'un dépliage après normalisation d'échelle.
pub fn quality(chart: &Chart, uv: &[V2]) -> UnwrapQuality {
    let mut area3 = 0.0;
    let mut area_uv = 0.0;
    let mut items = Vec::with_capacity(chart.tris.len());
    for t in &chart.tris {
        let p = [chart.pos[t[0] as usize], chart.pos[t[1] as usize], chart.pos[t[2] as usize]];
        let Some(x) = local_frame(p[0], p[1], p[2]) else { continue };
        let u = [uv[t[0] as usize], uv[t[1] as usize], uv[t[2] as usize]];
        let a3 = 0.5 * (x[1] - x[0]).cross(x[2] - x[0]);
        let au = 0.5 * (u[1] - u[0]).cross(u[2] - u[0]);
        area3 += a3;
        area_uv += au.abs();
        items.push((x, u, a3, au));
    }
    let scale = if area_uv > 1e-300 { (area3 / area_uv).sqrt() } else { 1.0 };
    let mut dist = 0.0;
    let mut flipped = 0;
    for (x, u, a3, au) in items {
        if au <= 0.0 {
            flipped += 1;
            dist += a3 * 2.0;
            continue;
        }
        // J = [du1 du2] * inv([dx1 dx2])
        let (dx1, dx2) = (x[1] - x[0], x[2] - x[0]);
        let (du1, du2) = ((u[1] - u[0]) * scale, (u[2] - u[0]) * scale);
        let det = dx1.cross(dx2);
        // inv([[dx1.x, dx2.x],[dx1.y, dx2.y]]) = 1/det * [[dx2.y, -dx2.x],[-dx1.y, dx1.x]]
        let ja = (du1.x * dx2.y - du2.x * dx1.y) / det;
        let jb = (-du1.x * dx2.x + du2.x * dx1.x) / det;
        let jc = (du1.y * dx2.y - du2.y * dx1.y) / det;
        let jd = (-du1.y * dx2.x + du2.y * dx1.x) / det;
        let (s1, s2) = singular_values_2x2(ja, jb, jc, jd);
        let d = 0.5 * (s1.max(1e-9).ln().abs() + s2.max(1e-9).ln().abs());
        dist += a3 * d.min(2.0);
    }
    UnwrapQuality { distortion: if area3 > 0.0 { dist / area3 } else { 0.0 }, flipped, area3d: area3 }
}

/// Met l'îlot à l'échelle monde (aire UV = aire 3D) : base d'une densité de texels uniforme.
pub fn normalize_scale(chart: &Chart, uv: &mut [V2]) {
    let q = quality(chart, uv);
    let area_uv: f64 = chart
        .tris
        .iter()
        .map(|t| 0.5 * (uv[t[1] as usize] - uv[t[0] as usize]).cross(uv[t[2] as usize] - uv[t[0] as usize]).abs())
        .sum();
    if area_uv > 1e-300 && q.area3d > 0.0 {
        let s = (q.area3d / area_uv).sqrt();
        for p in uv.iter_mut() {
            *p = *p * s;
        }
    }
}
