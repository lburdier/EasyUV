//! Outils d'édition UV façon Maya (UV Toolkit) et RizomUV : pinceaux (déplacer, détendre,
//! redresser, pincer), aplatir un îlot dans un rectangle dessiné, aligner, répartir,
//! empiler, égaliser la densité, optimiser (ARAP) et redéplier les îlots sélectionnés.
//!
//! Les UV sont stockées par coin de face ; un « sommet UV » regroupe les coins d'un même
//! sommet du maillage, dans un même îlot et à la même position (les deux côtés d'une
//! couture sont donc deux sommets UV distincts, qui bougent indépendamment).

use crate::chart::Chart;
use crate::math::{V2, V3};
use crate::mesh::Mesh;
use std::collections::HashMap;

/// Faces et coins des îlots choisis.
pub struct Islands {
    pub faces: Vec<Vec<usize>>,
    pub corners: Vec<Vec<usize>>,
}

impl Islands {
    pub fn new(mesh: &Mesh, face_island: &[u32], ids: &[usize]) -> Self {
        let slot: HashMap<usize, usize> = ids.iter().enumerate().map(|(k, &i)| (i, k)).collect();
        let mut faces = vec![Vec::new(); ids.len()];
        let mut corners = vec![Vec::new(); ids.len()];
        for f in 0..mesh.face_count() {
            if let Some(&k) = slot.get(&(face_island[f] as usize)) {
                faces[k].push(f);
                corners[k].extend(mesh.face_range(f));
            }
        }
        Islands { faces, corners }
    }

    pub fn len(&self) -> usize {
        self.faces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
}

fn to_v2(c: [f32; 2]) -> V2 {
    V2::new(c[0] as f64, c[1] as f64)
}

fn len2(v: V2) -> f64 {
    v.x.hypot(v.y)
}

/// Boîte UV d'un ensemble de coins.
pub fn bounds(corner_uv: &[[f32; 2]], corners: &[usize]) -> Option<(V2, V2)> {
    if corners.is_empty() {
        return None;
    }
    let mut lo = V2::new(f64::MAX, f64::MAX);
    let mut hi = V2::new(f64::MIN, f64::MIN);
    for &c in corners {
        let p = to_v2(corner_uv[c]);
        lo = V2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = V2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    Some((lo, hi))
}

fn apply(corner_uv: &mut [[f32; 2]], corners: &[usize], f: impl Fn(V2) -> V2) {
    for &c in corners {
        let q = f(to_v2(corner_uv[c]));
        corner_uv[c] = [q.x as f32, q.y as f32];
    }
}

// ---------------------------------------------------------------------------------------
// Graphe des sommets UV (pinceaux, relaxation)
// ---------------------------------------------------------------------------------------

/// Sommets UV d'un ensemble d'îlots et leurs arêtes, avec les longueurs 3D.
pub struct UvGraph {
    /// Coins de chaque sommet UV.
    pub corners: Vec<Vec<usize>>,
    /// Îlot (rang dans `Islands`) de chaque sommet UV.
    pub island: Vec<usize>,
    /// Voisins : (sommet UV, longueur de l'arête en 3D).
    pub nbrs: Vec<Vec<(usize, f64)>>,
    /// Sommet sur le bord de son îlot.
    pub boundary: Vec<bool>,
    /// Rapport longueur UV / longueur 3D de chaque îlot (échelle de l'îlot).
    pub scale: Vec<f64>,
}

impl UvGraph {
    pub fn build(mesh: &Mesh, corner_uv: &[[f32; 2]], isl: &Islands) -> Self {
        let mut ids: HashMap<(usize, u32, u32, u32), usize> = HashMap::new();
        let mut corners: Vec<Vec<usize>> = Vec::new();
        let mut island = Vec::new();
        let mut edge_count: HashMap<(usize, usize), (u32, f64)> = HashMap::new();
        for (k, faces) in isl.faces.iter().enumerate() {
            for &f in faces {
                let vs: Vec<usize> = mesh
                    .face_range(f)
                    .map(|c| {
                        let key = (k, mesh.corners[c], corner_uv[c][0].to_bits(), corner_uv[c][1].to_bits());
                        *ids.entry(key).or_insert_with(|| {
                            corners.push(Vec::new());
                            island.push(k);
                            corners.len() - 1
                        })
                    })
                    .collect();
                for (c, &v) in mesh.face_range(f).zip(&vs) {
                    corners[v].push(c);
                }
                let r: Vec<usize> = mesh.face_range(f).collect();
                for i in 0..vs.len() {
                    let (a, b) = (vs[i], vs[(i + 1) % vs.len()]);
                    if a == b {
                        continue;
                    }
                    let l = (mesh.positions[mesh.corners[r[i]] as usize] - mesh.positions[mesh.corners[r[(i + 1) % r.len()]] as usize]).length();
                    edge_count.entry((a.min(b), a.max(b))).or_insert((0, l)).0 += 1;
                }
            }
        }
        let n = corners.len();
        let mut nbrs = vec![Vec::new(); n];
        let mut boundary = vec![false; n];
        let (mut su, mut s3) = (vec![0.0; isl.len()], vec![0.0; isl.len()]);
        for (&(a, b), &(count, l)) in &edge_count {
            nbrs[a].push((b, l));
            nbrs[b].push((a, l));
            if count == 1 {
                boundary[a] = true;
                boundary[b] = true;
            }
            let k = island[a];
            su[k] += len2(to_v2(corner_uv[corners[a][0]]) - to_v2(corner_uv[corners[b][0]]));
            s3[k] += l;
        }
        // ordre stable des voisins (les tables de hachage ne le sont pas)
        nbrs.iter_mut().for_each(|v| v.sort_by_key(|x| x.0));
        let scale = su.iter().zip(&s3).map(|(u, t)| if *t > 0.0 { u / t } else { 1.0 }).collect();
        UvGraph { corners, island, nbrs, boundary, scale }
    }

    pub fn len(&self) -> usize {
        self.corners.len()
    }

    pub fn is_empty(&self) -> bool {
        self.corners.is_empty()
    }

    pub fn positions(&self, corner_uv: &[[f32; 2]]) -> Vec<V2> {
        self.corners.iter().map(|c| to_v2(corner_uv[c[0]])).collect()
    }

    pub fn write(&self, pts: &[V2], corner_uv: &mut [[f32; 2]]) {
        for (cs, p) in self.corners.iter().zip(pts) {
            for &c in cs {
                corner_uv[c] = [p.x as f32, p.y as f32];
            }
        }
    }
}

/// Atténuation douce du pinceau : 1 au centre, 0 au bord du rayon.
pub fn falloff(dist: f64, radius: f64) -> f64 {
    if radius <= 0.0 || dist >= radius {
        return 0.0;
    }
    let t = 1.0 - dist / radius;
    t * t * (3.0 - 2.0 * t)
}

/// Pinceaux d'édition UV.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Brush {
    /// Tire les UV sous le pinceau dans le sens du geste.
    Grab,
    /// Détend : chaque arête revient vers sa longueur 3D (moins d'étirement).
    Relax,
    /// Redresse : les arêtes s'alignent sur l'horizontale ou la verticale la plus proche.
    Straighten,
    /// Resserre les UV vers le centre du pinceau (Maj : les écarte).
    Pinch,
}

/// Poids du pinceau pour chaque sommet UV (positions `pts`).
pub fn brush_weights(pts: &[V2], center: V2, radius: f64) -> Vec<f64> {
    pts.iter().map(|p| falloff(len2(*p - center), radius)).collect()
}

/// Pinceau Déplacer : `start` et `weights` sont pris au début du geste.
pub fn brush_grab(pts: &mut [V2], start: &[V2], weights: &[f64], delta: V2) {
    for ((p, s), w) in pts.iter_mut().zip(start).zip(weights) {
        *p = *s + delta * *w;
    }
}

/// Un pas de pinceau Détendre, Redresser ou Pincer (`strength` dans 0..1).
pub fn brush_step(g: &UvGraph, pts: &mut [V2], brush: Brush, center: V2, radius: f64, strength: f64, invert: bool) {
    let w = brush_weights(pts, center, radius);
    let old = pts.to_vec();
    for i in 0..pts.len() {
        if w[i] <= 0.0 {
            continue;
        }
        let k = (w[i] * strength).clamp(0.0, 1.0);
        let target = match brush {
            Brush::Relax => relax_target(g, &old, i),
            Brush::Straighten => straighten_target(g, &old, i),
            Brush::Pinch => {
                let d = center - old[i];
                Some(old[i] + d * if invert { -0.15 } else { 0.15 })
            }
            Brush::Grab => None,
        };
        if let Some(t) = target {
            pts[i] = old[i] + (t - old[i]) * k;
        }
    }
}

/// Position qui rend à chaque arête du sommet sa longueur 3D (à l'échelle de l'îlot).
fn relax_target(g: &UvGraph, p: &[V2], i: usize) -> Option<V2> {
    let s = g.scale[g.island[i]];
    let (mut acc, mut n) = (V2::new(0.0, 0.0), 0.0);
    for &(j, l) in &g.nbrs[i] {
        let d = p[i] - p[j];
        let dl = len2(d);
        if dl < 1e-12 {
            continue;
        }
        acc = acc + p[j] + d * (l * s / dl);
        n += 1.0;
    }
    (n > 0.0).then(|| acc * (1.0 / n))
}

/// Position qui aligne les arêtes du sommet sur l'axe U ou V le plus proche. Une arête à
/// 45° (diagonale) pèse peu : elle ne sait pas de quel côté tomber.
fn straighten_target(g: &UvGraph, p: &[V2], i: usize) -> Option<V2> {
    let (mut acc, mut wsum) = (V2::new(0.0, 0.0), 0.0);
    for &(j, _) in &g.nbrs[i] {
        let d = p[i] - p[j];
        let dl = len2(d);
        if dl < 1e-12 {
            continue;
        }
        let w = ((d.x * d.x - d.y * d.y) / (dl * dl)).abs();
        let snapped = if d.x.abs() >= d.y.abs() { V2::new(dl.copysign(d.x), 0.0) } else { V2::new(0.0, dl.copysign(d.y)) };
        acc = acc + (p[j] + snapped) * w;
        wsum += w;
    }
    (wsum > 1e-9).then(|| acc * (1.0 / wsum))
}

// ---------------------------------------------------------------------------------------
// Outils d'îlots (UV Toolkit)
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    CenterU,
    Right,
    Bottom,
    CenterV,
    Top,
}

/// Aligne les îlots sur un bord (ou le centre) de leur boîte commune.
pub fn align(corner_uv: &mut [[f32; 2]], isl: &Islands, mode: Align) {
    let all: Vec<usize> = isl.corners.concat();
    let Some((lo, hi)) = bounds(corner_uv, &all) else { return };
    for cs in &isl.corners {
        let Some((a, b)) = bounds(corner_uv, cs) else { continue };
        let d = match mode {
            Align::Left => V2::new(lo.x - a.x, 0.0),
            Align::Right => V2::new(hi.x - b.x, 0.0),
            Align::CenterU => V2::new((lo.x + hi.x - a.x - b.x) * 0.5, 0.0),
            Align::Bottom => V2::new(0.0, lo.y - a.y),
            Align::Top => V2::new(0.0, hi.y - b.y),
            Align::CenterV => V2::new(0.0, (lo.y + hi.y - a.y - b.y) * 0.5),
        };
        apply(corner_uv, cs, |p| p + d);
    }
}

/// Répartit les îlots à intervalles égaux entre le premier et le dernier (axe 0 = U, 1 = V).
pub fn distribute(corner_uv: &mut [[f32; 2]], isl: &Islands, axis: usize) {
    if isl.len() < 3 {
        return;
    }
    let comp = |v: V2| if axis == 0 { v.x } else { v.y };
    let mut boxes: Vec<(usize, f64, f64)> = isl.corners.iter().enumerate().filter_map(|(k, cs)| bounds(corner_uv, cs).map(|(a, b)| (k, comp(a), comp(b)))).collect();
    boxes.sort_by(|x, y| (x.1 + x.2).partial_cmp(&(y.1 + y.2)).unwrap_or(std::cmp::Ordering::Equal));
    let (start, end) = (boxes[0].1, boxes[boxes.len() - 1].2);
    let total: f64 = boxes.iter().map(|b| b.2 - b.1).sum();
    let gap = (end - start - total) / (boxes.len() - 1) as f64;
    let mut cursor = start;
    for &(k, a, b) in &boxes {
        let d = cursor - a;
        apply(corner_uv, &isl.corners[k], |p| if axis == 0 { V2::new(p.x + d, p.y) } else { V2::new(p.x, p.y + d) });
        cursor += b - a + gap;
    }
}

/// Empile les îlots : leurs centres rejoignent celui du premier (îlots identiques superposés,
/// par exemple pour partager la même zone de texture).
pub fn stack(corner_uv: &mut [[f32; 2]], isl: &Islands) {
    let Some((a, b)) = isl.corners.first().and_then(|cs| bounds(corner_uv, cs)) else { return };
    let target = (a + b) * 0.5;
    for cs in isl.corners.iter().skip(1) {
        let Some((lo, hi)) = bounds(corner_uv, cs) else { continue };
        let d = target - (lo + hi) * 0.5;
        apply(corner_uv, cs, |p| p + d);
    }
}

/// Aire UV et aire 3D des faces d'un îlot.
fn areas(mesh: &Mesh, corner_uv: &[[f32; 2]], faces: &[usize]) -> (f64, f64) {
    let (mut au, mut a3) = (0.0, 0.0);
    for &f in faces {
        let r = mesh.face_range(f);
        for t in mesh.fan(f) {
            let p = |k: usize| mesh.positions[mesh.corners[r.start + t[k]] as usize];
            let u = |k: usize| to_v2(corner_uv[r.start + t[k]]);
            a3 += 0.5 * (p(1) - p(0)).cross(p(2) - p(0)).length();
            au += 0.5 * (u(1) - u(0)).cross(u(2) - u(0)).abs();
        }
    }
    (au, a3)
}

/// Donne à chaque îlot la même densité de texels (la moyenne de la sélection), autour de
/// son centre.
pub fn equalize_density(mesh: &Mesh, corner_uv: &mut [[f32; 2]], isl: &Islands) {
    let a: Vec<(f64, f64)> = isl.faces.iter().map(|f| areas(mesh, corner_uv, f)).collect();
    let (su, s3) = a.iter().fold((0.0, 0.0), |(x, y), (u, t)| (x + u, y + t));
    if s3 <= 0.0 || su <= 0.0 {
        return;
    }
    let target = su / s3;
    for (cs, (au, a3)) in isl.corners.iter().zip(&a) {
        if *au <= 0.0 || *a3 <= 0.0 {
            continue;
        }
        let k = (target / (au / a3)).sqrt();
        let Some((lo, hi)) = bounds(corner_uv, cs) else { continue };
        let c = (lo + hi) * 0.5;
        apply(corner_uv, cs, |p| c + (p - c) * k);
    }
}

/// Fait tenir la sélection dans la tuile 0-1 (proportions gardées).
pub fn fit_to_tile(corner_uv: &mut [[f32; 2]], isl: &Islands) {
    fit_group(corner_uv, &isl.corners.concat(), V2::new(0.0, 0.0), V2::new(1.0, 1.0), true);
}

/// Place la boîte des coins `cs` dans le rectangle [lo, hi] (proportions gardées ou non).
fn fit_group(corner_uv: &mut [[f32; 2]], cs: &[usize], lo: V2, hi: V2, keep_aspect: bool) {
    let Some((a, b)) = bounds(corner_uv, cs) else { return };
    let (w, h) = ((b.x - a.x).max(1e-12), (b.y - a.y).max(1e-12));
    let (sx, sy) = ((hi.x - lo.x) / w, (hi.y - lo.y) / h);
    let (sx, sy) = if keep_aspect { (sx.min(sy), sx.min(sy)) } else { (sx, sy) };
    let off = (lo + hi) * 0.5 - V2::new((a.x + b.x) * 0.5 * sx, (a.y + b.y) * 0.5 * sy);
    apply(corner_uv, cs, |p| V2::new(p.x * sx + off.x, p.y * sy + off.y));
}

/// Aplatit les îlots dans le rectangle dessiné : chaque îlot est d'abord rendu droit (bande
/// de quads ou arc), tourné pour que son côté long suive celui du rectangle, puis la
/// sélection est étirée pour remplir exactement le rectangle. Renvoie les îlots rendus droits.
pub fn flatten_into_rect(mesh: &Mesh, corner_uv: &mut [[f32; 2]], face_island: &[u32], ids: &[usize], lo: V2, hi: V2) -> usize {
    let n_isl = face_island.iter().map(|&i| i as usize + 1).max().unwrap_or(0);
    let mut which = vec![false; n_isl];
    for &i in ids {
        if i < n_isl {
            which[i] = true;
        }
    }
    let (done, _) = crate::layout::straighten_grid(mesh, corner_uv, face_island, &which);
    let isl = Islands::new(mesh, face_island, ids);
    let wide = hi.x - lo.x >= hi.y - lo.y;
    for cs in &isl.corners {
        let Some((a, b)) = bounds(corner_uv, cs) else { continue };
        if (b.x - a.x >= b.y - a.y) != wide {
            let c = (a + b) * 0.5;
            apply(corner_uv, cs, |p| c + V2::new(-(p.y - c.y), p.x - c.x));
        }
    }
    fit_group(corner_uv, &isl.corners.concat(), lo, hi, false);
    done
}

/// Îlot sous forme de `Chart` minimal (positions 3D et triangles) pour LSCM / ARAP.
fn island_chart(mesh: &Mesh, corner_uv: &[[f32; 2]], faces: &[usize]) -> (Chart, Vec<(usize, usize)>, Vec<V2>) {
    let mut ids: HashMap<(u32, u32, u32), usize> = HashMap::new();
    let mut pos: Vec<V3> = Vec::new();
    let mut init: Vec<V2> = Vec::new();
    let mut corner_local = Vec::new();
    let mut tris = Vec::new();
    for &f in faces {
        let r = mesh.face_range(f);
        let vs: Vec<usize> = r
            .clone()
            .map(|c| {
                let key = (mesh.corners[c], corner_uv[c][0].to_bits(), corner_uv[c][1].to_bits());
                *ids.entry(key).or_insert_with(|| {
                    pos.push(mesh.positions[mesh.corners[c] as usize]);
                    init.push(to_v2(corner_uv[c]));
                    pos.len() - 1
                })
            })
            .collect();
        for (c, &v) in r.zip(&vs) {
            corner_local.push((c, v));
        }
        for t in mesh.fan(f) {
            tris.push([vs[t[0]] as u32, vs[t[1]] as u32, vs[t[2]] as u32]);
        }
    }
    let n = pos.len();
    let chart = Chart {
        faces: faces.iter().map(|&f| f as u32).collect(),
        pos,
        mesh_vert: Vec::new(),
        tris,
        corner_local: Vec::new(),
        euler: 0,
        boundary_loops: 0,
        loops: Vec::new(),
        interior_edges: Vec::new(),
    };
    debug_assert_eq!(init.len(), n);
    (chart, corner_local, init)
}

/// Remet `uv` dans le repère de `orig` : même sens (pas de miroir), meilleure rotation
/// (Procuste), même aire et même centre.
fn match_frame(tris: &[[u32; 3]], orig: &[V2], uv: &mut [V2]) {
    let area = |p: &[V2]| -> f64 { tris.iter().map(|t| 0.5 * (p[t[1] as usize] - p[t[0] as usize]).cross(p[t[2] as usize] - p[t[0] as usize])).sum() };
    if area(orig) * area(uv) < 0.0 {
        uv.iter_mut().for_each(|p| p.x = -p.x);
    }
    let n = orig.len().max(1) as f64;
    let co = orig.iter().fold(V2::new(0.0, 0.0), |s, p| s + *p) * (1.0 / n);
    let cu = uv.iter().fold(V2::new(0.0, 0.0), |s, p| s + *p) * (1.0 / n);
    let (mut sd, mut sc) = (0.0, 0.0);
    for (o, u) in orig.iter().zip(uv.iter()) {
        let (a, b) = (*u - cu, *o - co);
        sd += a.dot(b);
        sc += a.cross(b);
    }
    let ang = sc.atan2(sd);
    let (s, c) = ang.sin_cos();
    let k = (area(orig).abs() / area(uv).abs().max(1e-300)).sqrt();
    for p in uv.iter_mut() {
        *p = co + (*p - cu).rotated(c, s) * k;
    }
}

/// Optimise les îlots (relaxation ARAP, comme « Optimize » de RizomUV) : moins d'étirement,
/// même place, même orientation, même aire.
pub fn optimize(mesh: &Mesh, corner_uv: &mut [[f32; 2]], isl: &Islands, iterations: usize) {
    for faces in &isl.faces {
        let (chart, corner_local, orig) = island_chart(mesh, corner_uv, faces);
        let mut uv = orig.clone();
        crate::unwrap::arap(&chart, &mut uv, iterations, 200);
        if uv.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            continue;
        }
        match_frame(&chart.tris, &orig, &mut uv);
        for (c, v) in corner_local {
            corner_uv[c] = [uv[v].x as f32, uv[v].y as f32];
        }
    }
}

/// Redéplie les îlots (LSCM puis ARAP) en gardant leur place, leur orientation et leur aire :
/// efface les retouches et repart de la forme 3D.
pub fn reunwrap(mesh: &Mesh, corner_uv: &mut [[f32; 2]], isl: &Islands) {
    for faces in &isl.faces {
        let (chart, corner_local, orig) = island_chart(mesh, corner_uv, faces);
        if chart.pos.len() < 3 {
            continue;
        }
        let mut uv = crate::unwrap::lscm(&chart, 2000);
        crate::unwrap::arap(&chart, &mut uv, 6, 200);
        if uv.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            continue;
        }
        match_frame(&chart.tris, &orig, &mut uv);
        for (c, v) in corner_local {
            corner_uv[c] = [uv[v].x as f32, uv[v].y as f32];
        }
    }
}

// ---------------------------------------------------------------------------------------
// Coutures : boucles d'arêtes, dépliage partiel, couper, coudre
// ---------------------------------------------------------------------------------------

/// Boucle d'arêtes passant par l'arête (a, b), comme Alt+clic dans Blender : à chaque sommet
/// de valence 4, on continue par l'arête opposée (qui ne touche aucune des faces de l'arête
/// courante). S'arrête aux pôles, aux bords et quand la boucle se referme.
pub fn edge_loop(mesh: &Mesh, a: u32, b: u32) -> Vec<(u32, u32)> {
    let key = |x: u32, y: u32| if x < y { (x, y) } else { (y, x) };
    let mut edge_faces: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    let mut vert_edges: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
    for f in 0..mesh.face_count() {
        let vs = mesh.face(f);
        for i in 0..vs.len() {
            let k = key(vs[i], vs[(i + 1) % vs.len()]);
            let e = edge_faces.entry(k).or_default();
            if e.is_empty() {
                vert_edges.entry(k.0).or_default().push(k);
                vert_edges.entry(k.1).or_default().push(k);
            }
            e.push(f);
        }
    }
    let start = key(a, b);
    if !edge_faces.contains_key(&start) {
        return Vec::new();
    }
    let mut out = vec![start];
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::from([start]);
    for (from, mut v) in [(a, b), (b, a)] {
        let mut prev = key(from, v);
        loop {
            let edges = &vert_edges[&v];
            if edges.len() != 4 {
                break;
            }
            let faces = &edge_faces[&prev];
            let next = edges.iter().copied().filter(|e| *e != prev && !edge_faces[e].iter().any(|f| faces.contains(f))).collect::<Vec<_>>();
            let [n] = next[..] else { break };
            if !seen.insert(n) {
                break;
            }
            out.push(n);
            prev = n;
            v = if n.0 == v { n.1 } else { n.0 };
        }
    }
    out
}

/// Dépliage intelligent des seules faces `faces` (îlots sélectionnés, coupe) : renvoie les UV
/// de leurs coins, dans l'ordre des faces, rangées dans la tuile 0-1. `seams` : arêtes à
/// couper (paires de sommets du maillage) ; le bord de la sélection est toujours une couture.
pub fn unwrap_faces(mesh: &Mesh, faces: &[usize], seams: &[(u32, u32)], params: &crate::Params) -> crate::UvResult {
    let mut remap: HashMap<u32, u32> = HashMap::new();
    let mut pos = Vec::new();
    let sub_faces: Vec<Vec<u32>> = faces
        .iter()
        .map(|&f| {
            mesh.face(f)
                .iter()
                .map(|&v| {
                    *remap.entry(v).or_insert_with(|| {
                        pos.push(mesh.positions[v as usize]);
                        pos.len() as u32 - 1
                    })
                })
                .collect()
        })
        .collect();
    let sub = Mesh::from_faces(pos, &sub_faces);
    let sub_seams: Vec<(u32, u32)> = seams.iter().filter_map(|(a, b)| Some((*remap.get(a)?, *remap.get(b)?))).collect();
    let mut p = params.clone();
    p.density = crate::DensityMode::Fit;
    crate::smart_unwrap(&sub, &p, &sub_seams)
}

/// Côté d'une arête : (face, coin du premier sommet de la clé, coin du second).
type Side = (usize, usize, usize);

/// Coud les arêtes `edges` (paires de sommets) : l'îlot le plus petit de chaque paire
/// d'îlots est tourné et déplacé pour que ses arêtes rejoignent celles de l'autre (comme
/// « Move and Sew » de Maya), puis les sommets cousus sont soudés. Deux bords d'un même îlot
/// se rejoignent à mi-chemin. Renvoie le nombre de paires d'îlots cousues.
pub fn sew(mesh: &Mesh, corner_uv: &mut [[f32; 2]], face_island: &[u32], edges: &[(u32, u32)]) -> usize {
    let key = |x: u32, y: u32| if x < y { (x, y) } else { (y, x) };
    let wanted: std::collections::HashSet<(u32, u32)> = edges.iter().map(|&(a, b)| key(a, b)).collect();
    // côtés de chaque arête voulue : (face, coin du premier sommet de la clé, coin du second)
    let mut sides: HashMap<(u32, u32), Vec<Side>> = HashMap::new();
    for f in 0..mesh.face_count() {
        let r: Vec<usize> = mesh.face_range(f).collect();
        for i in 0..r.len() {
            let (ca, cb) = (r[i], r[(i + 1) % r.len()]);
            let (va, vb) = (mesh.corners[ca], mesh.corners[cb]);
            let k = key(va, vb);
            if wanted.contains(&k) {
                sides.entry(k).or_default().push(if va == k.0 { (f, ca, cb) } else { (f, cb, ca) });
            }
        }
    }
    // paires d'îlots -> correspondances (coin côté A, coin côté B)
    let mut pairs: HashMap<(u32, u32), Vec<(usize, usize)>> = HashMap::new();
    for s in sides.values() {
        let [(f1, a1, b1), (f2, a2, b2)] = s[..] else { continue };
        if corner_uv[a1] == corner_uv[a2] && corner_uv[b1] == corner_uv[b2] {
            continue; // déjà cousue
        }
        let (i1, i2) = (face_island[f1], face_island[f2]);
        let list = pairs.entry((i1.min(i2), i1.max(i2))).or_default();
        if i1 <= i2 {
            list.extend([(a1, a2), (b1, b2)]);
        } else {
            list.extend([(a2, a1), (b2, b1)]);
        }
    }
    let island_corners = |i: u32| -> Vec<usize> { (0..mesh.face_count()).filter(|&f| face_island[f] == i).flat_map(|f| mesh.face_range(f)).collect() };
    let mut done = 0;
    let mut keys: Vec<(u32, u32)> = pairs.keys().copied().collect();
    keys.sort();
    for k in keys {
        let matches = &pairs[&k];
        if k.0 == k.1 {
            // fente dans un même îlot : chaque paire de coins rejoint son milieu
            for &(ca, cb) in matches {
                let m = (to_v2(corner_uv[ca]) + to_v2(corner_uv[cb])) * 0.5;
                weld(mesh, corner_uv, &island_corners(k.0), &[(ca, m), (cb, m)]);
            }
            done += 1;
            continue;
        }
        // l'îlot qui bouge : celui qui a le moins de coins
        let (ca_all, cb_all) = (island_corners(k.0), island_corners(k.1));
        let moving_is_b = cb_all.len() <= ca_all.len();
        let (fixed_pts, moving_pts): (Vec<V2>, Vec<V2>) = matches.iter().map(|&(a, b)| if moving_is_b { (to_v2(corner_uv[a]), to_v2(corner_uv[b])) } else { (to_v2(corner_uv[b]), to_v2(corner_uv[a])) }).unzip();
        let moving = if moving_is_b { &cb_all } else { &ca_all };
        let Some(t) = rigid_fit(&moving_pts, &fixed_pts) else { continue };
        apply(corner_uv, moving, &t);
        // soudure : les coins cousus de l'îlot déplacé prennent la position de l'autre côté
        let targets: Vec<(usize, V2)> = matches.iter().map(|&(a, b)| if moving_is_b { (b, to_v2(corner_uv[a])) } else { (a, to_v2(corner_uv[b])) }).collect();
        weld(mesh, corner_uv, moving, &targets);
        done += 1;
    }
    done
}

/// Place chaque coin `c` en `p`, et avec lui les coins de `pool` du même sommet qui étaient
/// au même endroit (le reste de l'îlot autour de ce sommet).
fn weld(mesh: &Mesh, corner_uv: &mut [[f32; 2]], pool: &[usize], targets: &[(usize, V2)]) {
    for &(c, p) in targets {
        let (v, old) = (mesh.corners[c], corner_uv[c]);
        for &k in pool {
            if mesh.corners[k] == v && corner_uv[k] == old {
                corner_uv[k] = [p.x as f32, p.y as f32];
            }
        }
        corner_uv[c] = [p.x as f32, p.y as f32];
    }
}

/// Meilleure transformation rigide (rotation, éventuel miroir, translation) qui amène
/// `from` sur `to`.
fn rigid_fit(from: &[V2], to: &[V2]) -> Option<impl Fn(V2) -> V2> {
    let n = from.len();
    if n == 0 || n != to.len() {
        return None;
    }
    let cf = from.iter().fold(V2::new(0.0, 0.0), |s, p| s + *p) * (1.0 / n as f64);
    let ct = to.iter().fold(V2::new(0.0, 0.0), |s, p| s + *p) * (1.0 / n as f64);
    let fit = |mirror: bool| {
        let (mut sd, mut sc) = (0.0, 0.0);
        for (f, t) in from.iter().zip(to) {
            let mut a = *f - cf;
            if mirror {
                a.x = -a.x;
            }
            let b = *t - ct;
            sd += a.dot(b);
            sc += a.cross(b);
        }
        let (s, c) = sc.atan2(sd).sin_cos();
        let err: f64 = from
            .iter()
            .zip(to)
            .map(|(f, t)| {
                let mut a = *f - cf;
                if mirror {
                    a.x = -a.x;
                }
                len2(a.rotated(c, s) + ct - *t)
            })
            .sum();
        (err, c, s)
    };
    let (e0, c0, s0) = fit(false);
    let (e1, c1, s1) = fit(true);
    // un seul segment ne dit rien du sens : pas de miroir sans gain net
    let (mirror, c, s) = if n > 2 && e1 < e0 * 0.5 { (true, c1, s1) } else { (false, c0, s0) };
    Some(move |p: V2| {
        let mut a = p - cf;
        if mirror {
            a.x = -a.x;
        }
        a.rotated(c, s) + ct
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{layout::uv_islands, primitives, smart_unwrap, Params};

    #[test]
    fn edge_loop_goes_around_cylinder() {
        // tore soudé de 16 × 8 quads : toute boucle en fait le tour (16 ou 8 arêtes)
        let m = crate::model::weld(&primitives::torus(1.0, 0.3, 16, 8));
        let vs = m.face(0).to_vec();
        for k in 0..2 {
            let l = edge_loop(&m, vs[k], vs[k + 1]);
            assert!(l.len() == 16 || l.len() == 8, "boucle de {} arêtes", l.len());
        }
    }

    #[test]
    fn cut_then_sew_restores_one_island() {
        // grille de 4 x 2 quads, un seul îlot ; on coupe au milieu puis on recoud
        let (nx, ny) = (4usize, 2usize);
        let mut pos = Vec::new();
        for j in 0..=ny {
            for i in 0..=nx {
                pos.push(V3::new(i as f64, j as f64, 0.0));
            }
        }
        let w = nx as u32 + 1;
        let faces: Vec<Vec<u32>> = (0..ny as u32).flat_map(|j| (0..nx as u32).map(move |i| vec![j * w + i, j * w + i + 1, (j + 1) * w + i + 1, (j + 1) * w + i])).collect();
        let m = Mesh::from_faces(pos, &faces);
        let cut: Vec<(u32, u32)> = (0..ny as u32).map(|j| (j * w + 2, (j + 1) * w + 2)).collect();
        let all: Vec<usize> = (0..m.face_count()).collect();
        let r = unwrap_faces(&m, &all, &cut, &Params::default());
        let (fi, n) = uv_islands(&m, &r.corner_uv);
        assert_eq!(n, 2, "la coupe sépare la grille en deux");
        let mut uv = r.corner_uv.clone();
        assert_eq!(sew(&m, &mut uv, &fi, &cut), 1);
        let (_, n2) = uv_islands(&m, &uv);
        assert_eq!(n2, 1, "recousue, la grille ne fait plus qu'un îlot");
    }

    fn setup() -> (Mesh, Vec<[f32; 2]>, Vec<u32>, usize) {
        let m = primitives::cylinder(1.0, 2.0, 24, 4);
        let r = smart_unwrap(&m, &Params::default(), &[]);
        let (fi, n) = uv_islands(&m, &r.corner_uv);
        (m, r.corner_uv, fi, n)
    }

    #[test]
    fn align_left_and_distribute() {
        let (m, mut uv, fi, n) = setup();
        let ids: Vec<usize> = (0..n).collect();
        let isl = Islands::new(&m, &fi, &ids);
        align(&mut uv, &isl, Align::Left);
        let lefts: Vec<f64> = isl.corners.iter().map(|cs| bounds(&uv, cs).unwrap().0.x).collect();
        assert!(lefts.iter().all(|&x| (x - lefts[0]).abs() < 1e-5), "{lefts:?}");
        if n >= 3 {
            distribute(&mut uv, &isl, 1);
            let mut b: Vec<(f64, f64)> = isl.corners.iter().map(|cs| bounds(&uv, cs).map(|(a, b)| (a.y, b.y)).unwrap()).collect();
            // même ordre que l'outil : par centre
            b.sort_by(|x, y| (x.0 + x.1).partial_cmp(&(y.0 + y.1)).unwrap());
            let gaps: Vec<f64> = b.windows(2).map(|w| w[1].0 - w[0].1).collect();
            assert!(gaps.iter().all(|g| (g - gaps[0]).abs() < 1e-4), "{gaps:?}");
        }
    }

    #[test]
    fn equalize_gives_same_density() {
        let (m, mut uv, fi, n) = setup();
        let ids: Vec<usize> = (0..n).collect();
        let isl = Islands::new(&m, &fi, &ids);
        // on grossit le premier îlot : l'égalisation doit le ramener
        apply(&mut uv, &isl.corners[0], |p| p * 3.0);
        equalize_density(&m, &mut uv, &isl);
        let d: Vec<f64> = isl.faces.iter().map(|f| {
            let (a, b) = areas(&m, &uv, f);
            a / b
        }).collect();
        assert!(d.iter().all(|x| (x / d[0] - 1.0).abs() < 1e-3), "{d:?}");
    }

    #[test]
    fn flatten_into_drawn_rectangle() {
        let (m, mut uv, fi, n) = setup();
        // la paroi du cylindre : l'îlot qui a le plus de faces
        let wall = (0..n).max_by_key(|&i| fi.iter().filter(|&&x| x as usize == i).count()).unwrap();
        let (lo, hi) = (V2::new(0.1, 0.4), V2::new(0.9, 0.6));
        flatten_into_rect(&m, &mut uv, &fi, &[wall], lo, hi);
        let isl = Islands::new(&m, &fi, &[wall]);
        let (a, b) = bounds(&uv, &isl.corners[0]).unwrap();
        assert!((a.x - lo.x).abs() < 1e-4 && (a.y - lo.y).abs() < 1e-4 && (b.x - hi.x).abs() < 1e-4 && (b.y - hi.y).abs() < 1e-4, "{a:?} {b:?}");
    }

    #[test]
    fn straighten_brush_aligns_edges() {
        // grille de quads penchée de 20° : le pinceau Redresser la remet d'aplomb
        let (nx, ny) = (6usize, 3usize);
        let mut pos = Vec::new();
        for j in 0..=ny {
            for i in 0..=nx {
                pos.push(V3::new(i as f64 * 0.1, j as f64 * 0.1, 0.0));
            }
        }
        let w = nx as u32 + 1;
        let faces: Vec<Vec<u32>> = (0..ny as u32).flat_map(|j| (0..nx as u32).map(move |i| vec![j * w + i, j * w + i + 1, (j + 1) * w + i + 1, (j + 1) * w + i])).collect();
        let m = Mesh::from_faces(pos, &faces);
        let (s, c) = 20f64.to_radians().sin_cos();
        let mut uv: Vec<[f32; 2]> = m.corners.iter().map(|&v| {
            let p = m.positions[v as usize];
            let q = V2::new(p.x, p.y).rotated(c, s);
            [q.x as f32 + 0.3, q.y as f32 + 0.3]
        }).collect();
        let fi = vec![0u32; m.face_count()];
        let isl = Islands::new(&m, &fi, &[0]);
        let g = UvGraph::build(&m, &uv, &isl);
        let mut pts = g.positions(&uv);
        let center = pts.iter().fold(V2::new(0.0, 0.0), |a, p| a + *p) * (1.0 / pts.len() as f64);
        let tilt = |pts: &[V2]| -> f64 {
            g.nbrs.iter().enumerate().flat_map(|(i, n)| n.iter().map(move |&(j, _)| (i, j))).map(|(i, j)| {
                let d = pts[i] - pts[j];
                (d.x.abs().min(d.y.abs())) / len2(d).max(1e-12)
            }).fold(0.0, f64::max)
        };
        let before = tilt(&pts);
        for _ in 0..200 {
            brush_step(&g, &mut pts, Brush::Straighten, center, 1.0, 0.5, false);
        }
        let after = tilt(&pts);
        assert!(before > 0.3 && after < 0.02, "inclinaison {before:.3} -> {after:.3}");
        g.write(&pts, &mut uv);
    }

    #[test]
    fn relax_brush_reduces_stretch() {
        let (m, mut uv, fi, n) = setup();
        let ids: Vec<usize> = (0..n).collect();
        let isl = Islands::new(&m, &fi, &ids);
        let g = UvGraph::build(&m, &uv, &isl);
        let mut pts = g.positions(&uv);
        let err = |pts: &[V2]| -> f64 {
            let mut e = 0.0;
            for (i, nb) in g.nbrs.iter().enumerate() {
                for &(j, l) in nb {
                    e += (len2(pts[i] - pts[j]) / (l * g.scale[g.island[i]]) - 1.0).abs();
                }
            }
            e
        };
        // on abîme la zone sous le pinceau, puis on la détend
        let center = pts[0];
        let start = pts.clone();
        let wts = brush_weights(&start, center, 0.15);
        brush_grab(&mut pts, &start, &wts, V2::new(0.05, 0.03));
        let damaged = err(&pts);
        for _ in 0..100 {
            brush_step(&g, &mut pts, Brush::Relax, center, 0.3, 0.5, false);
        }
        assert!(err(&pts) < damaged * 0.5, "{} -> {}", damaged, err(&pts));
        g.write(&pts, &mut uv);
    }

    #[test]
    fn optimize_keeps_place_and_area() {
        let (m, mut uv, fi, n) = setup();
        let ids: Vec<usize> = (0..n).collect();
        let isl = Islands::new(&m, &fi, &ids);
        let before: Vec<(V2, V2)> = isl.corners.iter().map(|cs| bounds(&uv, cs).unwrap()).collect();
        optimize(&m, &mut uv, &isl, 4);
        reunwrap(&m, &mut uv, &isl);
        for (cs, (a, b)) in isl.corners.iter().zip(before) {
            let (c, d) = bounds(&uv, cs).unwrap();
            let (ca, cb) = ((a + b) * 0.5, (c + d) * 0.5);
            assert!(len2(ca - cb) < 0.05 * len2(b - a), "centre déplacé");
        }
    }
}
