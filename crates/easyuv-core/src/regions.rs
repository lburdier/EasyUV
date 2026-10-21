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
    if planar >= 0.55 {
        ShapePreset::HardSurface
    } else if developable >= 0.55 {
        ShapePreset::Cylindrical
    } else {
        ShapePreset::Organic
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
