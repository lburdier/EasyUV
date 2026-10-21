//! Empaquetage des îlots : rotation de boîte minimale + skyline (bas-gauche) avec rotation 90°.

use crate::math::V2;

fn convex_hull(pts: &[V2]) -> Vec<V2> {
    let mut p: Vec<V2> = pts.to_vec();
    p.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap().then(a.y.partial_cmp(&b.y).unwrap()));
    p.dedup_by(|a, b| (a.x - b.x).abs() < 1e-15 && (a.y - b.y).abs() < 1e-15);
    if p.len() < 3 {
        return p;
    }
    let mut h: Vec<V2> = Vec::with_capacity(p.len() * 2);
    for pass in 0..2 {
        let start = h.len();
        let iter: Box<dyn Iterator<Item = &V2>> = if pass == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for &q in iter {
            while h.len() >= start + 2 && (h[h.len() - 1] - h[h.len() - 2]).cross(q - h[h.len() - 2]) <= 0.0 {
                h.pop();
            }
            h.push(q);
        }
        h.pop();
    }
    h
}

/// Oriente l'îlot selon sa boîte englobante d'aire minimale (côté long horizontal)
/// et le ramène à l'origine. Retourne (largeur, hauteur).
pub fn orient_min_box(uv: &mut [V2]) -> (f64, f64) {
    if uv.is_empty() {
        return (0.0, 0.0);
    }
    let hull = convex_hull(uv);
    let mut best = (f64::MAX, 1.0, 0.0);
    let n = hull.len();
    for i in 0..n.max(1) {
        let (c, s) = if n >= 2 {
            let d = hull[(i + 1) % n] - hull[i];
            let l = (d.x * d.x + d.y * d.y).sqrt();
            if l < 1e-15 {
                continue;
            }
            (d.x / l, -d.y / l) // rotation qui ramène l'arête à l'horizontale
        } else {
            (1.0, 0.0)
        };
        let (mut lo, mut hi) = (V2::new(f64::MAX, f64::MAX), V2::new(f64::MIN, f64::MIN));
        for &p in &hull {
            let r = p.rotated(c, s);
            lo = V2::new(lo.x.min(r.x), lo.y.min(r.y));
            hi = V2::new(hi.x.max(r.x), hi.y.max(r.y));
        }
        let area = (hi.x - lo.x) * (hi.y - lo.y);
        if area < best.0 - 1e-12 {
            best = (area, c, s);
        }
    }
    let (_, mut c, mut s) = best;
    if best.0 == f64::MAX {
        c = 1.0;
        s = 0.0;
    }
    for p in uv.iter_mut() {
        *p = p.rotated(c, s);
    }
    let (w, h) = bounds_to_origin(uv);
    if h > w {
        for p in uv.iter_mut() {
            *p = V2::new(p.y, -p.x);
        }
        let (w, h) = bounds_to_origin(uv);
        return (w, h);
    }
    (w, h)
}

fn bounds_to_origin(uv: &mut [V2]) -> (f64, f64) {
    let (mut lo, mut hi) = (V2::new(f64::MAX, f64::MAX), V2::new(f64::MIN, f64::MIN));
    for p in uv.iter() {
        lo = V2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = V2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    for p in uv.iter_mut() {
        *p = *p - lo;
    }
    (hi.x - lo.x, hi.y - lo.y)
}

#[derive(Clone, Copy)]
struct Node {
    x: f64,
    y: f64,
    w: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub rotated: bool,
}

struct Skyline {
    width: f64,
    nodes: Vec<Node>,
}

impl Skyline {
    fn new(width: f64) -> Self {
        Skyline { width, nodes: vec![Node { x: 0.0, y: 0.0, w: width }] }
    }
    fn fit(&self, i: usize, w: f64) -> Option<f64> {
        let x = self.nodes[i].x;
        if x + w > self.width + 1e-9 {
            return None;
        }
        let mut left = w;
        let mut y: f64 = 0.0;
        let mut j = i;
        while left > 1e-12 {
            if j >= self.nodes.len() {
                return None;
            }
            y = y.max(self.nodes[j].y);
            left -= self.nodes[j].w;
            j += 1;
        }
        Some(y)
    }
    fn add(&mut self, i: usize, x: f64, y: f64, w: f64, h: f64) {
        self.nodes.insert(i, Node { x, y: y + h, w });
        let j = i + 1;
        while j < self.nodes.len() {
            let prev_end = self.nodes[j - 1].x + self.nodes[j - 1].w;
            if self.nodes[j].x < prev_end {
                let shrink = prev_end - self.nodes[j].x;
                self.nodes[j].x += shrink;
                self.nodes[j].w -= shrink;
                if self.nodes[j].w <= 1e-12 {
                    self.nodes.remove(j);
                    continue;
                }
            }
            break;
        }
        let mut k = 0;
        while k + 1 < self.nodes.len() {
            if (self.nodes[k].y - self.nodes[k + 1].y).abs() < 1e-12 {
                self.nodes[k].w += self.nodes[k + 1].w;
                self.nodes.remove(k + 1);
            } else {
                k += 1;
            }
        }
    }
    /// Place un rectangle ; retourne la position (bas-gauche) et si la rotation est utilisée.
    fn place(&mut self, w: f64, h: f64, allow_rot: bool) -> Option<Placement> {
        let mut best: Option<(f64, f64, usize, bool)> = None; // (haut, x, index, rot)
        for i in 0..self.nodes.len() {
            for rot in [false, true] {
                if rot && !allow_rot {
                    continue;
                }
                let (rw, rh) = if rot { (h, w) } else { (w, h) };
                if let Some(y) = self.fit(i, rw) {
                    let top = y + rh;
                    let better = match best {
                        None => true,
                        Some((bt, bx, _, _)) => top < bt - 1e-12 || ((top - bt).abs() <= 1e-12 && self.nodes[i].x < bx),
                    };
                    if better {
                        best = Some((top, self.nodes[i].x, i, rot));
                    }
                }
            }
        }
        let (top, x, i, rot) = best?;
        let (rw, rh) = if rot { (h, w) } else { (w, h) };
        self.add(i, x, top - rh, rw, rh);
        Some(Placement { x, y: top - rh, rotated: rot })
    }
    fn height(&self) -> f64 {
        self.nodes.iter().map(|n| n.y).fold(0.0, f64::max)
    }
}

/// Empaquette des rectangles (w, h) avec une marge `pad` dans une bande de largeur `width`.
/// Retourne les placements (coin bas-gauche du contenu) et l'étendue utilisée (w, h).
fn pack_strip(rects: &[(f64, f64)], order: &[usize], width: f64, pad: f64, allow_rot: bool) -> Option<(Vec<Placement>, f64, f64)> {
    let mut sky = Skyline::new(width);
    let mut out = vec![Placement { x: 0.0, y: 0.0, rotated: false }; rects.len()];
    let mut used_w: f64 = 0.0;
    for &i in order {
        let (w, h) = rects[i];
        let p = sky.place(w + pad, h + pad, allow_rot)?;
        let (rw, _) = if p.rotated { (h + pad, w + pad) } else { (w + pad, h + pad) };
        used_w = used_w.max(p.x + rw);
        out[i] = Placement { x: p.x + pad * 0.5, y: p.y + pad * 0.5, rotated: p.rotated };
    }
    Some((out, used_w, sky.height()))
}

pub struct PackOutput {
    pub placements: Vec<Placement>,
    /// Facteur d'échelle monde -> UV.
    pub scale: f64,
    pub overflow: bool,
}

/// `margin` est exprimée en fraction de la tuile UV [0, 1].
/// `fixed_scale` : Some(s) en mode densité de texels cible (UV par unité monde).
pub fn pack(rects: &[(f64, f64)], margin: f64, fixed_scale: Option<f64>, allow_rot: bool) -> PackOutput {
    if rects.is_empty() {
        return PackOutput { placements: vec![], scale: 1.0, overflow: false };
    }
    let mut order: Vec<usize> = (0..rects.len()).collect();
    order.sort_by(|&a, &b| {
        let ka = rects[a].1.max(rects[a].0 * 0.5) * rects[a].0;
        let kb = rects[b].1.max(rects[b].0 * 0.5) * rects[b].0;
        rects[b].1.partial_cmp(&rects[a].1).unwrap().then(kb.partial_cmp(&ka).unwrap())
    });

    if let Some(s) = fixed_scale {
        let tile = 1.0 / s;
        let pad = margin * tile;
        let min_w = rects.iter().map(|r| if allow_rot { r.0.min(r.1) } else { r.0 }).fold(0.0, f64::max) + pad;
        let width = tile.max(min_w);
        let (pl, w, h) = pack_strip(rects, &order, width, pad, allow_rot).expect("largeur suffisante");
        let overflow = w > tile * 1.0001 || h > tile * 1.0001;
        // centrage dans la tuile si ça rentre
        let (ox, oy) = if overflow { (0.0, 0.0) } else { ((tile - w) * 0.5, (tile - h) * 0.5) };
        let placements = pl.into_iter().map(|p| Placement { x: p.x + ox, y: p.y + oy, ..p }).collect();
        return PackOutput { placements, scale: s, overflow };
    }

    let area: f64 = rects.iter().map(|r| r.0 * r.1).sum::<f64>().max(1e-300);
    let mut side = area.sqrt() * 1.2;
    let mut best: Option<(Vec<Placement>, f64, f64, f64)> = None;
    // Deux passes : la marge dépend de l'échelle finale.
    for _ in 0..3 {
        let pad = margin * side;
        let padded_area: f64 = rects.iter().map(|r| (r.0 + pad) * (r.1 + pad)).sum();
        let min_w = rects.iter().map(|r| if allow_rot { r.0.min(r.1) } else { r.0 }).fold(0.0, f64::max) + pad;
        let base = padded_area.sqrt();
        best = None;
        for k in 0..24 {
            let width = (base * (0.85 + 0.03 * k as f64)).max(min_w);
            if let Some((pl, w, h)) = pack_strip(rects, &order, width, pad, allow_rot) {
                let s = w.max(h);
                if best.as_ref().map_or(true, |b| s < b.1) {
                    best = Some((pl, s, w, h));
                }
            }
        }
        let new_side = best.as_ref().unwrap().1;
        if (new_side - side).abs() < 1e-6 * side {
            break;
        }
        side = new_side;
    }
    let (pl, s, w, h) = best.unwrap();
    let scale = 1.0 / s.max(1e-300);
    let (ox, oy) = ((s - w) * 0.5, (s - h) * 0.5);
    let placements = pl.into_iter().map(|p| Placement { x: p.x + ox, y: p.y + oy, ..p }).collect();
    PackOutput { placements, scale, overflow: false }
}
