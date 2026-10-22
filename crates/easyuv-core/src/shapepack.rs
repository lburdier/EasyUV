//! Empaquetage par forme (et non par boîtes englobantes).
//!
//! Chaque îlot est rastérisé de façon conservatrice sur une grille, dilaté de la marge, puis
//! placé « en bas à gauche » sur une carte d'occupation en bitsets, ce qui remplit les creux
//! des îlots concaves. Quatre rotations sont essayées par îlot. En mode « remplir », on cherche
//! la plus grande échelle qui fait tout tenir dans la tuile (plusieurs échelles évaluées en
//! parallèle à chaque tour). En mode densité cible, les îlots qui ne tiennent pas passent sur la
//! tuile suivante (UDIM : u + 1).

use crate::math::V2;

/// Îlot à placer, en coordonnées monde.
pub struct Island<'a> {
    pub uv: &'a [V2],
    pub tris: &'a [[u32; 3]],
}

/// Transformation finale d'un îlot : uv' = R(rot)·p·scale + offset.
#[derive(Clone, Copy, Debug, Default)]
pub struct Xform {
    pub rot: u8, // quarts de tour
    pub offset: V2,
}

impl Xform {
    pub fn apply(&self, p: V2, scale: f64) -> V2 {
        rotate(p, self.rot) * scale + self.offset
    }
}

pub struct ShapePack {
    pub xforms: Vec<Xform>,
    pub scale: f64,
    /// Nombre de tuiles UDIM utilisées (1 = tout tient dans 0-1).
    pub tiles: usize,
}

fn rotate(p: V2, rot: u8) -> V2 {
    match rot & 3 {
        0 => p,
        1 => V2::new(-p.y, p.x),
        2 => V2::new(-p.x, -p.y),
        _ => V2::new(p.y, -p.x),
    }
}

/// Masque binaire d'un îlot pour une rotation : lignes de bitsets.
struct Mask {
    w: usize,
    h: usize,
    words: usize,
    /// masque exact (tamponné dans la carte), même repère
    stamp: Vec<u64>,
    /// cellule (min) du repère : uv = (pos - qmin) / G + R·p·s
    qmin: (i64, i64),
    /// intervalles occupés du masque dilaté, par ligne : (début, longueur)
    runs: Vec<Vec<(u32, u32)>>,
}

fn rasterize(isl: &Island, rot: u8, k: f64, dil: usize) -> Mask {
    // k = échelle × résolution (cellules par unité monde)
    let pts: Vec<(f64, f64)> = isl.uv.iter().map(|&p| {
        let q = rotate(p, rot);
        (q.x * k, q.y * k)
    }).collect();
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in &pts {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let qmin = (x0.floor() as i64, y0.floor() as i64);
    let core_w = (x1.ceil() as i64 - qmin.0).max(1) as usize + 1;
    let core_h = (y1.ceil() as i64 - qmin.1).max(1) as usize + 1;
    let (w, h) = (core_w + 2 * dil, core_h + 2 * dil);
    let mut cells = vec![false; w * h];
    // rastérisation conservatrice : cellule marquée si son centre est à moins d'une
    // demi-diagonale du triangle (sur-ensemble des cellules touchées)
    const R: f64 = 0.7072;
    for t in isl.tris {
        let p = [pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]];
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        let s = if area >= 0.0 { 1.0 } else { -1.0 };
        let edges: [(f64, f64, f64, f64, f64); 3] = std::array::from_fn(|i| {
            let (a, b) = (p[i], p[(i + 1) % 3]);
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt().max(1e-12);
            (a.0, a.1, b.0 - a.0, b.1 - a.1, len)
        });
        let by0 = (p.iter().map(|q| q.1).fold(f64::MAX, f64::min) - 1.0).floor() as i64 - qmin.1;
        let by1 = (p.iter().map(|q| q.1).fold(f64::MIN, f64::max) + 1.0).ceil() as i64 - qmin.1;
        // balayage : pour chaque ligne de centres, intervalle en x où les 3 demi-plans
        // (dilatés de R) sont satisfaits : s·(dx·(y−ay) − dy·(x−ax)) ≥ −R·len
        for cy in by0.max(0)..=by1.min(core_h as i64 - 1) {
            let y = (cy + qmin.1) as f64 + 0.5;
            let (mut xl, mut xr) = (f64::MIN, f64::MAX);
            let mut empty = false;
            for &(ax, ay, dx, dy, len) in &edges {
                // c0 + c1·x ≥ 0
                let c1 = -s * dy;
                let c0 = s * (dx * (y - ay) + dy * ax) + R * len;
                if c1.abs() < 1e-12 {
                    if c0 < 0.0 {
                        empty = true;
                    }
                } else if c1 > 0.0 {
                    xl = xl.max(-c0 / c1);
                } else {
                    xr = xr.min(-c0 / c1);
                }
            }
            if empty || xl > xr {
                continue;
            }
            // centres de cellules x + 0.5 dans [xl, xr]
            let cx0 = ((xl - 0.5).ceil() as i64 - qmin.0).max(0);
            let cx1 = ((xr - 0.5).floor() as i64 - qmin.0).min(core_w as i64 - 1);
            let row = (cy as usize + dil) * w + dil;
            for cx in cx0..=cx1 {
                cells[row + cx as usize] = true;
            }
        }
    }    // dilatation séparable (carré de rayon `dil`)
    let mut dilated = cells.clone();
    if dil > 0 {
        let mut tmp = vec![false; w * h];
        for y in 0..h {
            let mut last: i64 = -(1 << 40);
            for x in 0..w {
                if cells[y * w + x] {
                    last = x as i64;
                }
                if x as i64 - last <= dil as i64 {
                    tmp[y * w + x] = true;
                }
            }
            let mut last: i64 = 1 << 40;
            for x in (0..w).rev() {
                if cells[y * w + x] {
                    last = x as i64;
                }
                if last - x as i64 <= dil as i64 {
                    tmp[y * w + x] = true;
                }
            }
        }
        for x in 0..w {
            let mut last: i64 = -(1 << 40);
            for y in 0..h {
                if tmp[y * w + x] {
                    last = y as i64;
                }
                dilated[y * w + x] = y as i64 - last <= dil as i64;
            }
            let mut last: i64 = 1 << 40;
            for y in (0..h).rev() {
                if tmp[y * w + x] {
                    last = y as i64;
                }
                if last - y as i64 <= dil as i64 {
                    dilated[y * w + x] = true;
                }
            }
        }
    }
    let words = w.div_ceil(64);
    let pack = |src: &[bool]| {
        let mut v = vec![0u64; words * h];
        for y in 0..h {
            for x in 0..w {
                if src[y * w + x] {
                    v[y * words + x / 64] |= 1u64 << (x % 64);
                }
            }
        }
        v
    };
    let runs = (0..h)
        .map(|y| {
            let mut v = Vec::new();
            let mut x = 0;
            while x < w {
                if dilated[y * w + x] {
                    let s = x;
                    while x < w && dilated[y * w + x] {
                        x += 1;
                    }
                    v.push((s as u32, (x - s) as u32));
                } else {
                    x += 1;
                }
            }
            v
        })
        .collect();
    Mask { w, h, words, stamp: pack(&cells), qmin: (qmin.0 - dil as i64, qmin.1 - dil as i64), runs }
}

/// Carte d'occupation `n`×`n` en bitsets.
#[derive(Clone)]
struct Board {
    n: usize,
    words: usize,
    rows: Vec<u64>,
    /// première ligne pouvant encore contenir une cellule libre
    first_free: usize,
    /// longueur de la plage libre (resp. occupée) commençant en chaque cellule
    free_run: Vec<u32>,
    occ_run: Vec<u32>,
}

impl Board {
    fn new(n: usize) -> Self {
        let words = n.div_ceil(64);
        let mut rows = vec![0u64; words * n];
        // les bits au-delà de n sont occupés
        if n % 64 != 0 {
            for y in 0..n {
                rows[y * words + words - 1] |= !0u64 << (n % 64);
            }
        }
        // carte vide : la plage libre depuis x va jusqu'au bord
        let free_run = (0..n * n).map(|i| (n - i % n) as u32).collect();
        Board { n, words, rows, first_free: 0, free_run, occ_run: vec![0; n * n] }
    }

    fn occupied(&self, y: usize, x: usize) -> bool {
        self.rows[y * self.words + x / 64] >> (x % 64) & 1 == 1
    }

    fn update_runs(&mut self, y: usize) {
        let n = self.n;
        let (mut fr, mut or) = (0u32, 0u32);
        for x in (0..n).rev() {
            if self.occupied(y, x) {
                or += 1;
                fr = 0;
            } else {
                fr += 1;
                or = 0;
            }
            self.free_run[y * n + x] = fr;
            self.occ_run[y * n + x] = or;
        }
    }

    /// Ok si le masque tient en (x, y) ; sinon la plus petite abscisse suivante possible
    /// (on saute directement après l'obstacle rencontré).
    fn fits(&self, m: &Mask, x: usize, y: usize) -> Result<(), usize> {
        let n = self.n;
        for r in 0..m.h {
            let row = (y + r) * n;
            for &(a, len) in &m.runs[r] {
                let p = x + a as usize;
                let fr = self.free_run[row + p];
                if fr >= len {
                    continue;
                }
                let skip = if fr == 0 { self.occ_run[row + p] as usize } else { fr as usize + self.occ_run[row + p + fr as usize] as usize };
                return Err(x + skip.max(1));
            }
        }
        Ok(())
    }

    fn stamp(&mut self, m: &Mask, x: usize, y: usize) {
        for r in 0..m.h {
            for k in 0..m.words {
                let mw = m.stamp[r * m.words + k];
                if mw == 0 {
                    continue;
                }
                let bx = x + 64 * k;
                let (wi, sh) = (bx / 64, bx % 64);
                let row = (y + r) * self.words;
                self.rows[row + wi] |= mw << sh;
                if sh != 0 && wi + 1 < self.words {
                    self.rows[row + wi + 1] |= mw >> (64 - sh);
                }
            }
        }
        for r in 0..m.h {
            self.update_runs(y + r);
        }
        while self.first_free < self.n && (0..self.words).all(|k| self.rows[self.first_free * self.words + k] == !0u64) {
            self.first_free += 1;
        }
    }

    /// Tamponne un masque à une position quelconque (obstacle), en ignorant ce qui sort.
    fn stamp_clipped(&mut self, m: &Mask, x0: i64, y0: i64) {
        let n = self.n as i64;
        for r in 0..m.h {
            let y = y0 + r as i64;
            if y < 0 || y >= n {
                continue;
            }
            for c in 0..m.w {
                if m.stamp[r * m.words + c / 64] >> (c % 64) & 1 == 0 {
                    continue;
                }
                let x = x0 + c as i64;
                if x >= 0 && x < n {
                    self.rows[y as usize * self.words + x as usize / 64] |= 1u64 << (x as usize % 64);
                }
            }
            self.update_runs(y as usize);
        }
        self.first_free = 0;
        while self.first_free < self.n && (0..self.words).all(|k| self.rows[self.first_free * self.words + k] == !0u64) {
            self.first_free += 1;
        }
    }

    /// Position la plus basse puis la plus à gauche où le masque tient.
    fn find(&self, m: &Mask) -> Option<(usize, usize)> {
        if m.w > self.n || m.h > self.n {
            return None;
        }
        for y in self.first_free..=self.n - m.h {
            let mut x = 0;
            while x <= self.n - m.w {
                match self.fits(m, x, y) {
                    Ok(()) => return Some((x, y)),
                    Err(nx) => x = nx,
                }
            }
        }
        None
    }
}

/// Une tentative à l'échelle `s` (UV par unité monde). Retourne les placements, ou None
/// si un îlot ne tient pas et que `udim` est faux.
#[allow(clippy::too_many_arguments)]
fn attempt(islands: &[Island], order: &[usize], s: f64, res: usize, margin: f64, rotations: u8, udim: bool, start: Option<&Board>) -> Option<(Vec<Xform>, usize)> {
    let k = s * res as f64;
    let dil = (margin * res as f64).ceil() as usize;
    let mut boards = vec![start.cloned().unwrap_or_else(|| Board::new(res))];
    let mut out = vec![Xform::default(); islands.len()];
    for &i in order {
        let masks: Vec<Mask> = (0..rotations).map(|r| rasterize(&islands[i], r, k, dil)).collect();
        let mut placed = None;
        for (ti, b) in boards.iter().enumerate() {
            // critère : le haut de l'îlot le plus bas possible, puis le plus à gauche
            let mut best: Option<(usize, usize, usize, usize)> = None; // (haut, x, rotation, y)
            for (r, m) in masks.iter().enumerate() {
                if let Some((x, y)) = b.find(m) {
                    let top = y + m.h;
                    if best.map_or(true, |(bt, bx, _, _)| (top, x) < (bt, bx)) {
                        best = Some((top, x, r, y));
                    }
                }
            }
            if let Some((_, x, r, y)) = best {
                placed = Some((ti, (y, x, r)));
                break;
            }
        }
        let (ti, (y, x, r)) = match placed {
            Some(p) => p,
            None if udim => {
                // nouvelle tuile
                boards.push(Board::new(res));
                let ti = boards.len() - 1;
                let b = &boards[ti];
                let found = masks.iter().enumerate().filter_map(|(r, m)| b.find(m).map(|(x, y)| (y, x, r))).min()?;
                (ti, found)
            }
            None => return None,
        };
        let m = &masks[r];
        boards[ti].stamp(m, x, y);
        out[i] = Xform {
            rot: r as u8,
            offset: V2::new((x as f64 - m.qmin.0 as f64) / res as f64 + ti as f64, (y as f64 - m.qmin.1 as f64) / res as f64),
        };
    }
    Some((out, boards.len()))
}

/// Empaquette les îlots. `fixed_scale` : densité cible (UV par unité monde) ; sinon la plus
/// grande échelle qui fait tout tenir dans la tuile 0-1 est recherchée.
/// min_scale : échelle déjà atteinte par une autre méthode ; seul un résultat strictement
/// meilleur est retourné (None sinon).
pub fn pack_shapes(islands: &[Island], margin: f64, fixed_scale: Option<f64>, allow_rot: bool, res: usize, min_scale: f64) -> Option<ShapePack> {
    if islands.is_empty() {
        return None;
    }
    let rotations = if allow_rot { 4 } else { 1 };
    let area: f64 = islands
        .iter()
        .map(|isl| isl.tris.iter().map(|t| 0.5 * (isl.uv[t[1] as usize] - isl.uv[t[0] as usize]).cross(isl.uv[t[2] as usize] - isl.uv[t[0] as usize]).abs()).sum::<f64>())
        .sum::<f64>()
        .max(1e-300);
    // grands îlots d'abord
    let mut order: Vec<usize> = (0..islands.len()).collect();
    let bbox_area = |isl: &Island| {
        let (mut a, mut b) = (V2::new(f64::MAX, f64::MAX), V2::new(f64::MIN, f64::MIN));
        for p in isl.uv {
            a = V2::new(a.x.min(p.x), a.y.min(p.y));
            b = V2::new(b.x.max(p.x), b.y.max(p.y));
        }
        (b.x - a.x) * (b.y - a.y)
    };
    let keys: Vec<f64> = islands.iter().map(bbox_area).collect();
    order.sort_by(|&a, &b| keys[b].partial_cmp(&keys[a]).unwrap().then(a.cmp(&b)));

    if let Some(s) = fixed_scale {
        let (xf, tiles) = attempt(islands, &order, s, res, margin, rotations, true, None)?;
        return Some(ShapePack { xforms: xf, scale: s, tiles });
    }
    // recherche de l'échelle maximale : bornes [hi·0.25, hi], hi = couverture de 100 %
    let hi0 = (1.0 / area).sqrt();
    let (mut lo, mut hi) = (min_scale.max(hi0 * 0.25).min(hi0), hi0);
    let mut best: Option<(Vec<Xform>, f64)> = None;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
    for _round in 0..3 {
        let scales: Vec<f64> = (1..=threads).map(|i| lo + (hi - lo) * i as f64 / (threads + 1) as f64).collect();
        let results: Vec<Option<Vec<Xform>>> = std::thread::scope(|sc| {
            let hs: Vec<_> = scales.iter().map(|&s| {
                let order = &order;
                sc.spawn(move || attempt(islands, order, s, res, margin, rotations, false, None).map(|r| r.0))
            }).collect();
            hs.into_iter().map(|h| h.join().ok().flatten()).collect()
        });
        // plus grande échelle réussie ; l'intervalle se resserre autour d'elle
        let mut ok_idx = None;
        for (i, r) in results.iter().enumerate() {
            if r.is_some() {
                ok_idx = Some(i);
            }
        }
        match ok_idx {
            Some(i) => {
                if best.as_ref().map_or(true, |b| scales[i] > b.1) {
                    best = Some((results[i].clone().unwrap(), scales[i]));
                }
                lo = scales[i];
                hi = if i + 1 < scales.len() { scales[i + 1] } else { hi };
            }
            None => {
                hi = scales[0];
                lo *= 0.5;
            }
        }
    }
    let (xforms, scale) = best?;
    if scale <= min_scale * 1.0005 {
        return None;
    }
    Some(ShapePack { xforms, scale, tiles: 1 })
}

/// Range `islands` dans l'espace laissé libre par `obstacles` (îlots déjà placés, en
/// coordonnées de la tuile 0-1), à l'échelle `scale` si possible, sinon à la plus grande
/// échelle inférieure qui tient. None s'il n'y a pas de place.
pub fn pack_into_free(islands: &[Island], obstacles: &[Island], margin: f64, allow_rot: bool, res: usize, scale: f64) -> Option<ShapePack> {
    if islands.is_empty() {
        return None;
    }
    let rotations = if allow_rot { 4 } else { 1 };
    let mut board = Board::new(res);
    for o in obstacles {
        let m = rasterize(o, 0, res as f64, 0);
        board.stamp_clipped(&m, m.qmin.0, m.qmin.1);
    }
    let bbox_area = |isl: &Island| {
        let (mut a, mut b) = (V2::new(f64::MAX, f64::MAX), V2::new(f64::MIN, f64::MIN));
        for p in isl.uv {
            a = V2::new(a.x.min(p.x), a.y.min(p.y));
            b = V2::new(b.x.max(p.x), b.y.max(p.y));
        }
        (b.x - a.x) * (b.y - a.y)
    };
    let keys: Vec<f64> = islands.iter().map(bbox_area).collect();
    let mut order: Vec<usize> = (0..islands.len()).collect();
    order.sort_by(|&a, &b| keys[b].partial_cmp(&keys[a]).unwrap().then(a.cmp(&b)));
    let run = |s: f64| attempt(islands, &order, s, res, margin, rotations, false, Some(&board)).map(|r| r.0);
    if let Some(xf) = run(scale) {
        return Some(ShapePack { xforms: xf, scale, tiles: 1 });
    }
    // dichotomie sur l'échelle
    let (mut lo, mut hi) = (0.0, scale);
    let mut best = None;
    for _ in 0..12 {
        let mid = 0.5 * (lo + hi);
        match run(mid) {
            Some(xf) => {
                best = Some((xf, mid));
                lo = mid;
            }
            None => hi = mid,
        }
    }
    best.map(|(xforms, scale)| ShapePack { xforms, scale, tiles: 1 })
}
