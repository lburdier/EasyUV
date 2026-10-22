//! Presets de forme et segmentation en régions développables.
//!
//! - Détection automatique de la forme d'un morceau connexe (hard-surface, révolution,
//!   organique) à partir de la courbure de Gauss discrète et de la part de faces planes.
//! - Croissance de régions « développables » : une région accepte une face tant que les
//!   normales de ses faces restent sur un même cercle de la sphère de Gauss (plan, cylindre,
//!   cône : surfaces dépliables sans distorsion). Les frontières suivent ainsi les boucles
//!   d'arêtes, et la paroi d'un cylindre reste une seule bande.

use crate::math::V3;
use crate::mesh::{Mesh, Topology};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// Stratégie de dépliage selon la forme de l'objet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShapePreset {
    /// Détecte la forme de chaque morceau connexe.
    #[default]
    Auto,
    /// Boîtes, pièces mécaniques : grandes faces planes, coutures sur les arêtes vives.
    HardSurface,
    /// Cylindres, tuyaux, pots, bouteilles : bandes droites + couvercles.
    Cylindrical,
    /// Personnages, sculptures : découpage guidé par la distorsion.
    Organic,
}

impl ShapePreset {
    pub const ALL: [ShapePreset; 4] = [ShapePreset::Auto, ShapePreset::HardSurface, ShapePreset::Cylindrical, ShapePreset::Organic];

    pub fn label(self) -> &'static str {
        match self {
            ShapePreset::Auto => "Auto (détection)",
            ShapePreset::HardSurface => "Hard-surface (cube, boîte)",
            ShapePreset::Cylindrical => "Cylindre / révolution",
            ShapePreset::Organic => "Organique",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            ShapePreset::Auto => "auto",
            ShapePreset::HardSurface => "hard-surface",
            ShapePreset::Cylindrical => "cylindre",
            ShapePreset::Organic => "organique",
        }
    }

    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => ShapePreset::HardSurface,
            2 => ShapePreset::Cylindrical,
            3 => ShapePreset::Organic,
            _ => ShapePreset::Auto,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Some(ShapePreset::Auto),
            "hard" | "hard-surface" | "hardsurface" | "cube" | "box" | "boite" => Some(ShapePreset::HardSurface),
            "cyl" | "cylindre" | "cylinder" | "revolution" | "tube" => Some(ShapePreset::Cylindrical),
            "organic" | "organique" | "orga" => Some(ShapePreset::Organic),
            _ => None,
        }
    }

    /// Réglages conseillés (angle de couture, tolérance de distorsion, ARAP).
    pub fn suggested(self) -> (f64, f64, u32) {
        match self {
            ShapePreset::Auto => (66.0, 0.12, 4),
            ShapePreset::HardSurface => (45.0, 0.08, 2),
            ShapePreset::Cylindrical => (60.0, 0.12, 4),
            ShapePreset::Organic => (75.0, 0.15, 8),
        }
    }
}

#[derive(PartialEq)]
struct Item(f64, u32);
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal).then(o.1.cmp(&self.1))
    }
}

/// Valeurs propres (croissantes) d'une matrice symétrique 3x3 (méthode trigonométrique).
fn eigen_sym3(a: [[f64; 3]; 3]) -> [f64; 3] {
    let p1 = a[0][1] * a[0][1] + a[0][2] * a[0][2] + a[1][2] * a[1][2];
    if p1 < 1e-20 {
        let mut d = [a[0][0], a[1][1], a[2][2]];
        d.sort_by(|x, y| x.partial_cmp(y).unwrap());
        return d;
    }
    let q = (a[0][0] + a[1][1] + a[2][2]) / 3.0;
    let p2 = (a[0][0] - q).powi(2) + (a[1][1] - q).powi(2) + (a[2][2] - q).powi(2) + 2.0 * p1;
    let p = (p2 / 6.0).sqrt();
    if p < 1e-20 {
        return [q; 3];
    }
    let b = |i: usize, j: usize| (a[i][j] - if i == j { q } else { 0.0 }) / p;
    let det = b(0, 0) * (b(1, 1) * b(2, 2) - b(1, 2) * b(2, 1)) - b(0, 1) * (b(1, 0) * b(2, 2) - b(1, 2) * b(2, 0))
        + b(0, 2) * (b(1, 0) * b(2, 1) - b(1, 1) * b(2, 0));
    let r = (det / 2.0).clamp(-1.0, 1.0);
    let phi = r.acos() / 3.0;
    let l1 = q + 2.0 * p * phi.cos();
    let l3 = q + 2.0 * p * (phi + 2.0 * std::f64::consts::PI / 3.0).cos();
    [l3, 3.0 * q - l1 - l3, l1]
}

#[cfg(test)]
fn min_eigen_sym3(a: [[f64; 3]; 3]) -> f64 {
    eigen_sym3(a)[0]
}

/// Vecteur propre associé à `l` (produit vectoriel des lignes de A - lI).
fn eigvec_sym3(a: [[f64; 3]; 3], l: f64) -> V3 {
    let r = |i: usize| V3::new(a[i][0] - if i == 0 { l } else { 0.0 }, a[i][1] - if i == 1 { l } else { 0.0 }, a[i][2] - if i == 2 { l } else { 0.0 });
    let c = [r(0).cross(r(1)), r(0).cross(r(2)), r(1).cross(r(2))];
    let best = c.iter().copied().max_by(|x, y| x.length().partial_cmp(&y.length()).unwrap()).unwrap();
    if best.length() < 1e-12 {
        V3::new(0.0, 0.0, 1.0)
    } else {
        best.normalized()
    }
}
/// Moments des normales d'une région (pondérés par l'aire).
#[derive(Clone, Copy, Default)]
struct Moments {
    w: f64,
    s1: V3,
    s2: [[f64; 3]; 3],
}

impl Moments {
    fn add(&mut self, n: V3, w: f64) {
        self.w += w;
        self.s1 = self.s1 + n * w;
        let v = [n.x, n.y, n.z];
        for i in 0..3 {
            for j in 0..3 {
                self.s2[i][j] += w * v[i] * v[j];
            }
        }
    }
    fn covariance(&self) -> ([[f64; 3]; 3], V3) {
        let m = self.s1 / self.w.max(1e-300);
        let mv = [m.x, m.y, m.z];
        let mut c = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                c[i][j] = self.s2[i][j] / self.w.max(1e-300) - mv[i] * mv[j];
            }
        }
        (c, m)
    }
    /// Dispersion des normales hors du meilleur plan (0 = toutes sur un même cercle).
    fn out_of_circle(&self) -> f64 {
        if self.w <= 0.0 {
            return 0.0;
        }
        eigen_sym3(self.covariance().0)[0].max(0.0)
    }
    /// Une normale `n` prolonge-t-elle la région sans la rendre non développable ?
    fn accepts(&self, n: V3, w: f64, dev_tol: f64) -> bool {
        let (c, m) = self.covariance();
        let ev = eigen_sym3(c);
        if ev[2] < 0.004 {
            // région encore quasi plane : on n'accepte qu'un léger changement de direction,
            // sinon trois normales quelconques définiraient toujours un « cercle »
            return n.dot(m.normalized()) >= 25f64.to_radians().cos();
        }
        // cercle établi : la normale doit rester dessus (axe = plus petite valeur propre)
        let axis = eigvec_sym3(c, ev[0]);
        if (axis.dot(n) - axis.dot(m)).abs() > 0.085 {
            return false;
        }
        let mut trial = *self;
        trial.add(n, w);
        trial.out_of_circle() <= dev_tol
    }
}

/// Classe un morceau connexe : part de faces localement planes et part de surface
/// à courbure de Gauss quasi nulle (développable).
pub fn classify(mesh: &Mesh, topo: &Topology, faces: &[u32], cut: &[bool]) -> ShapePreset {
    if faces.len() <= 12 {
        return ShapePreset::HardSurface;
    }
    let mut area_total = 0.0;
    let mut flat_area = 0.0;
    let cos2 = 2f64.to_radians().cos();
    // défaut angulaire et aire par sommet
    let mut defect: HashMap<u32, f64> = HashMap::new();
    let mut varea: HashMap<u32, f64> = HashMap::new();
    let (mut lo, mut hi) = (V3::new(f64::MAX, f64::MAX, f64::MAX), V3::new(f64::MIN, f64::MIN, f64::MIN));
    for &f in faces {
        let fu = f as usize;
        let a = topo.face_areas[fu];
        area_total += a;
        let n = topo.face_normals[fu];
        let mut flat = true;
        for c in mesh.face_range(fu) {
            let e = topo.corner_edge[c];
            if cut[e as usize] {
                continue;
            }
            if let Some(g) = topo.opposite(e, f) {
                if topo.face_normals[g as usize].dot(n) < cos2 {
                    flat = false;
                }
            }
        }
        if flat {
            flat_area += a;
        }
        let vs = mesh.face(fu);
        let k = vs.len();
        for i in 0..k {
            let p = mesh.positions[vs[i] as usize];
            lo = V3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = V3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
            let pa = mesh.positions[vs[(i + k - 1) % k] as usize] - p;
            let pb = mesh.positions[vs[(i + 1) % k] as usize] - p;
            let ang = (pa.normalized().dot(pb.normalized())).clamp(-1.0, 1.0).acos();
            *defect.entry(vs[i]).or_insert(2.0 * std::f64::consts::PI) -= ang;
            *varea.entry(vs[i]).or_insert(0.0) += a / k as f64;
        }
    }
    if area_total <= 0.0 {
        return ShapePreset::Organic;
    }
    // sommets de bord : défaut non significatif
    let mut boundary: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for &f in faces {
        for c in mesh.face_range(f as usize) {
            let e = topo.corner_edge[c] as usize;
            if topo.edges[e].faces.len() != 2 {
                boundary.insert(topo.edges[e].key.0);
                boundary.insert(topo.edges[e].key.1);
            }
        }
    }
    let r2 = ((hi - lo).length() * 0.5).powi(2).max(1e-12);
    let (mut dev_area, mut a_sum) = (0.0, 0.0);
    for (v, d) in &defect {
        if boundary.contains(v) {
            continue;
        }
        let a = varea[v];
        a_sum += a;
        // courbure normalisée par la taille : ~1-3 pour une sphère, 0 pour un cylindre
        let k = d.abs() / a.max(1e-300) * r2;
        if k < 0.35 {
            dev_area += a;
        }
    }
    let planar = flat_area / area_total;
    let developable = if a_sum > 0.0 { dev_area / a_sum } else { 1.0 };
    let rev = revolution_fit(mesh, topo, faces);
    if std::env::var_os("EASYUV_PROFILE").is_some() {
        eprintln!("  [forme] faces={} plan={planar:.3} dév={developable:.3} révolution={rev:?}", faces.len());
    }
    if planar >= 0.55 {
        ShapePreset::HardSurface
    } else if developable >= 0.55 || rev.is_some_and(|r| r.is_turned()) {
        // un pot aux arêtes biseautées a peu d'aire développable (les biseaux sont courbes
        // dans les deux sens) mais reste un objet tourné : bandes droites + couvercles
        ShapePreset::Cylindrical
    } else {
        ShapePreset::Organic
    }
}

/// Valeurs et vecteurs propres d'une matrice symétrique 6x6 (Jacobi), par valeur croissante.
fn jacobi6(mut a: [[f64; 6]; 6]) -> Vec<(f64, [f64; 6])> {
    let mut v = [[0.0; 6]; 6];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for _ in 0..60 {
        let off: f64 = (0..6).flat_map(|i| (0..6).filter(move |&j| j != i).map(move |j| (i, j))).map(|(i, j)| a[i][j] * a[i][j]).sum();
        if off < 1e-24 {
            break;
        }
        for p in 0..6 {
            for q in p + 1..6 {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for row in a.iter_mut() {
                    let (akp, akq) = (row[p], row[q]);
                    row[p] = c * akp - s * akq;
                    row[q] = s * akp + c * akq;
                }
                let (rp, rq) = (a[p], a[q]);
                a[p] = std::array::from_fn(|k| c * rp[k] - s * rq[k]);
                a[q] = std::array::from_fn(|k| s * rp[k] + c * rq[k]);
                for row in v.iter_mut() {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = c * vp - s * vq;
                    row[q] = s * vp + c * vq;
                }
            }
        }
    }
    let mut out: Vec<(f64, [f64; 6])> = (0..6).map(|i| (a[i][i], std::array::from_fn(|k| v[k][i]))).collect();
    out.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(Ordering::Equal));
    out
}

/// Ajustement d'une surface de révolution (pot, couvercle, bouteille, vis…) : sur une telle
/// surface, chaque normale coupe l'axe, soit `a · (p × n) + m · n = 0` avec (a, m) les
/// coordonnées de Plücker de l'axe. On cherche la droite qui le vérifie au mieux (plus petit
/// vecteur propre), puis on mesure l'écart angulaire moyen des normales à cette droite.
/// None si aucun axe fini ne convient (surface plane, prisme).
pub fn revolution_fit(mesh: &Mesh, topo: &Topology, faces: &[u32]) -> Option<Revolution> {
    let centroid = |f: usize| {
        let vs = mesh.face(f);
        vs.iter().fold(V3::ZERO, |s, &v| s + mesh.positions[v as usize]) / vs.len().max(1) as f64
    };
    let (mut c0, mut wsum) = (V3::ZERO, 0.0);
    let (mut lo, mut hi) = (V3::new(f64::MAX, f64::MAX, f64::MAX), V3::new(f64::MIN, f64::MIN, f64::MIN));
    for &f in faces {
        let w = topo.face_areas[f as usize];
        let p = centroid(f as usize);
        c0 = c0 + p * w;
        wsum += w;
        lo = V3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = V3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    if wsum <= 0.0 {
        return None;
    }
    c0 = c0 / wsum;
    let scale = ((hi - lo).length() * 0.5).max(1e-12);
    let pts: Vec<(V3, V3, f64)> = faces.iter().map(|&f| ((centroid(f as usize) - c0) / scale, topo.face_normals[f as usize], topo.face_areas[f as usize] / wsum)).collect();
    let mut m = [[0.0; 6]; 6];
    for &(p, n, w) in &pts {
        let x = p.cross(n);
        let r = [x.x, x.y, x.z, n.x, n.y, n.z];
        for i in 0..6 {
            for j in 0..6 {
                m[i][j] += w * r[i] * r[j];
            }
        }
    }
    let eig = jacobi6(m);
    let v = eig[0].1;
    let a = V3::new(v[0], v[1], v[2]);
    let al = a.length();
    if al < 0.2 {
        // axe rejeté à l'infini : surface plane ou prismatique, pas une révolution
        return None;
    }
    let mom = V3::new(v[3], v[4], v[5]);
    let axis = a / al;
    let c = a.cross(mom) / (al * al);
    let (mut err, mut wn) = (0.0, 0.0);
    // profil : direction de la normale dans le demi-plan méridien, par pas de 2°, avec
    // l'aire et l'étendue (hauteur, rayon) des sommets de chaque direction
    let mut bins = vec![(0.0f64, [f64::MAX, f64::MIN, f64::MAX, f64::MIN]); 181];
    for (&(p, n, w), &f) in pts.iter().zip(faces) {
        let t = axis.cross(p - c);
        let r = t.length();
        let radial = if r < 0.02 { None } else { Some(t.cross(axis) / r) };
        let theta = match radial {
            Some(d) => n.dot(axis).atan2(n.dot(d)).to_degrees(),
            None => 90f64.copysign(n.dot(axis)),
        };
        let b = &mut bins[((theta + 180.0) / 2.0).round() as usize];
        b.0 += w;
        for &v in mesh.face(f as usize) {
            let q = (mesh.positions[v as usize] - c0) / scale - c;
            let (h, rho) = (q.dot(axis), axis.cross(q).length());
            b.1 = [b.1[0].min(h), b.1[1].max(h), b.1[2].min(rho), b.1[3].max(rho)];
        }
        if r < 0.02 {
            continue; // sur l'axe (pôle d'un couvercle) : direction tangente indéfinie
        }
        err += w * (n.dot(t / r)).abs();
        wn += w;
    }
    if wn <= 0.0 {
        return None;
    }
    // une paroi, un cône ou un disque concentre son aire sur une seule direction, sur une
    // bonne longueur du profil ; un tore ou une sphère l'étale, et même à facettes chaque
    // direction n'y couvre qu'une facette courte
    let straight = bins
        .iter()
        .filter(|(a, e)| *a >= 0.03 && (e[1] - e[0]).hypot(e[3] - e[2]) >= 0.15)
        .map(|(a, _)| a)
        .sum();
    // unicité : la deuxième valeur propre, ramenée à la même échelle que l'écart
    Some(Revolution { error: err / wn, uniqueness: eig[1].0.max(0.0).sqrt(), straight })
}

/// Résultat de [`revolution_fit`].
#[derive(Clone, Copy, Debug)]
pub struct Revolution {
    /// Écart moyen |n · t| des normales à l'axe (0 = révolution parfaite).
    pub error: f64,
    /// Une sphère vérifie l'équation pour tous les axes passant par son centre : ≈ 0.
    pub uniqueness: f64,
    /// Part de l'aire sur des portions droites du profil (paroi, cône, disque).
    pub straight: f64,
}

impl Revolution {
    /// Pot, couvercle, bouteille, tuyau biseauté : à déplier en bandes et couvercles.
    pub fn is_turned(&self) -> bool {
        self.error < 0.03 && self.uniqueness > 0.05 && self.straight >= 0.45
    }
}

/// Découpe un morceau connexe en régions développables. Renvoie les régions (faces).
/// `dev_tol` : dispersion maximale des normales hors cercle ; `cone` : écart angulaire
/// maximal à la normale moyenne (π = illimité).
pub fn developable_regions(mesh: &Mesh, topo: &Topology, faces: &[u32], cut: &[bool], dev_tol: f64, cone: f64) -> Vec<Vec<u32>> {
    let mut local: HashMap<u32, u32> = HashMap::with_capacity(faces.len());
    for (i, &f) in faces.iter().enumerate() {
        local.insert(f, i as u32);
    }
    let neighbors = |f: u32| -> Vec<(u32, u32)> {
        let mut out = Vec::with_capacity(4);
        for c in mesh.face_range(f as usize) {
            let e = topo.corner_edge[c];
            if cut[e as usize] {
                continue;
            }
            if let Some(g) = topo.opposite(e, f) {
                if let Some(&lg) = local.get(&g) {
                    out.push((lg, e));
                }
            }
        }
        out
    };
    let n = faces.len();
    let mut region = vec![u32::MAX; n];
    // graines : les plus grandes faces d'abord (couvercles, grands pans plats)
    let mut seeds: Vec<u32> = (0..n as u32).collect();
    seeds.sort_by(|&a, &b| topo.face_areas[faces[b as usize] as usize].partial_cmp(&topo.face_areas[faces[a as usize] as usize]).unwrap().then(a.cmp(&b)));
    let cos_cone = cone.cos();
    let mut regions: Vec<Vec<u32>> = Vec::new();
    for &s in &seeds {
        if region[s as usize] != u32::MAX {
            continue;
        }
        let rid = regions.len() as u32;
        let mut mom = Moments::default();
        let mut members = Vec::new();
        let mut heap = BinaryHeap::new();
        heap.push(Item(0.0, s));
        while let Some(Item(cost, lf)) = heap.pop() {
            if region[lf as usize] != u32::MAX {
                continue;
            }
            let f = faces[lf as usize] as usize;
            let nf = topo.face_normals[f];
            let w = topo.face_areas[f].max(1e-12);
            if !members.is_empty() {
                let mean = mom.s1.normalized();
                if cone < std::f64::consts::PI && nf.dot(mean) < cos_cone {
                    continue;
                }
                // garde-fou : un pli marqué avec la face voisine termine la région
                if cost > 1.0 - 30f64.to_radians().cos() {
                    continue;
                }
                if !mom.accepts(nf, w, dev_tol) {
                    continue;
                }
            }
            mom.add(nf, w);
            region[lf as usize] = rid;
            members.push(lf);
            for (g, _) in neighbors(faces[lf as usize]) {
                if region[g as usize] == u32::MAX {
                    // continuité lisse d'abord : la région suit les boucles d'arêtes
                    let cost = 1.0 - topo.face_normals[faces[g as usize] as usize].dot(nf);
                    heap.push(Item(cost, g));
                }
            }
        }
        regions.push(members);
    }

    // Absorption des petites régions (anneaux de biseau, confettis) par le voisin
    // avec lequel elles partagent la plus longue frontière.
    let total: f64 = faces.iter().map(|&f| topo.face_areas[f as usize]).sum();
    let min_area = total * 0.05;
    loop {
        let areas: Vec<f64> = regions.iter().map(|r| r.iter().map(|&l| topo.face_areas[faces[l as usize] as usize]).sum()).collect();
        let mut order: Vec<usize> = (0..regions.len()).filter(|&r| !regions[r].is_empty() && areas[r] < min_area).collect();
        order.sort_by(|&a, &b| areas[a].partial_cmp(&areas[b]).unwrap());
        let mut merged_any = false;
        for r in order {
            if regions[r].is_empty() {
                continue;
            }
            let mut shared: HashMap<u32, f64> = HashMap::new();
            for &l in &regions[r] {
                for (g, e) in neighbors(faces[l as usize]) {
                    let rg = region[g as usize];
                    if rg != r as u32 {
                        *shared.entry(rg).or_insert(0.0) += topo.edges[e as usize].length;
                    }
                }
            }
            let Some((&target, _)) = shared.iter().max_by(|a, b| a.1.partial_cmp(b.1).unwrap().then(b.0.cmp(a.0))) else { continue };
            let moved = std::mem::take(&mut regions[r]);
            for &l in &moved {
                region[l as usize] = target;
            }
            regions[target as usize].extend(moved);
            merged_any = true;
        }
        if !merged_any {
            break;
        }
    }
    regions.into_iter().filter(|r| !r.is_empty()).map(|r| r.into_iter().map(|l| faces[l as usize]).collect()).collect()
}

/// Vrai si toutes les faces sont dans un même plan (à `tol_deg` près).
pub fn is_planar(topo: &Topology, faces: &[u32], tol_deg: f64) -> bool {
    let mut s = V3::ZERO;
    for &f in faces {
        s = s + topo.face_normals[f as usize] * topo.face_areas[f as usize];
    }
    let m = s.normalized();
    let c = tol_deg.to_radians().cos();
    m.length() > 0.5 && faces.iter().all(|&f| topo.face_normals[f as usize].dot(m) >= c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    fn all_faces(m: &Mesh) -> Vec<u32> {
        (0..m.face_count() as u32).collect()
    }

    #[test]
    fn eigen() {
        let e = min_eigen_sym3([[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 1.0]]);
        assert!((e - 1.0).abs() < 1e-9);
        let e = min_eigen_sym3([[2.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 0.0, 5.0]]);
        assert!((e - 1.0).abs() < 1e-9, "{e}");
    }

    #[test]
    fn classification() {
        for (m, want) in [
            (primitives::cube(1.0), ShapePreset::HardSurface),
            (primitives::cylinder(1.0, 3.0, 32, 8), ShapePreset::Cylindrical),
            (primitives::uv_sphere(1.0, 32, 16), ShapePreset::Organic),
            (primitives::blob(1.0, 48, 24), ShapePreset::Organic),
            // pot aux arêtes arrondies : peu d'aire développable, mais un objet tourné
            (primitives::beveled_cylinder(1.0, 1.6, 0.15, 4, 48, 8), ShapePreset::Cylindrical),
            // révolution aussi, mais sans aucune portion droite du profil
            (primitives::torus(1.0, 0.35, 48, 16), ShapePreset::Organic),
        ] {
            let topo = Topology::build(&m);
            let cut: Vec<bool> = topo.edges.iter().map(|e| e.faces.len() != 2 || e.dihedral > 66f64.to_radians()).collect();
            assert_eq!(classify(&m, &topo, &all_faces(&m), &cut), want);
        }
    }

    #[test]
    fn cylinder_wall_is_one_region() {
        // cylindre sans arête vive (angle > 90°) : la paroi doit rester d'un seul tenant
        let m = primitives::cylinder(1.0, 3.0, 32, 8);
        let topo = Topology::build(&m);
        let cut: Vec<bool> = topo.edges.iter().map(|e| e.faces.len() != 2).collect();
        let regs = developable_regions(&m, &topo, &all_faces(&m), &cut, 0.03, std::f64::consts::PI);
        let sizes: Vec<usize> = regs.iter().map(|r| r.len()).collect();
        assert!(sizes.contains(&(32 * 8)), "régions {sizes:?}");
    }
}
