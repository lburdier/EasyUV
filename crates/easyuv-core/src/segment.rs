//! Segmentation intelligente en îlots :
//! 1. coutures initiales (arêtes vives, coutures forcées, non-manifold) ;
//! 2. chaque îlot non disque est découpé : tube -> couture le long du chemin le plus
//!    discret entre ses bords ; fermé / genre > 0 -> partition géodésique guidée par les plis ;
//! 3. chaque îlot déplié trop déformé (ou retourné) est re-partitionné récursivement.

use crate::chart::{build_chart, Chart};
use crate::math::V2;
use crate::mesh::{Mesh, Topology};
use crate::unwrap::{arap, lscm, normalize_scale, quality};
use crate::regions::{self, ShapePreset};
use crate::Params;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(PartialEq)]
struct HeapItem(f64, u32);
impl Eq for HeapItem {}
impl PartialOrd for HeapItem {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for HeapItem {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal)
    }
}

pub struct Unwrapped {
    pub chart: Chart,
    pub uv: Vec<V2>,
    pub distortion: f64,
    pub flipped: usize,
}

const MAX_DEPTH: u32 = 24;

pub fn segment_and_unwrap(mesh: &Mesh, topo: &Topology, params: &Params, forced_seams: &[bool]) -> (Vec<Unwrapped>, Vec<bool>, [usize; 3]) {
    let mut shapes = [0usize; 3];
    let nf = mesh.face_count();
    let angle = params.angle_deg.to_radians();
    let mut cut: Vec<bool> = topo
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| e.faces.len() != 2 || e.dihedral > angle || forced_seams.get(i).copied().unwrap_or(false))
        .collect();

    // Composantes initiales par remplissage à travers les arêtes non coupées
    let mut owner = vec![u32::MAX; nf];
    let mut next_tag = 0u32;
    let mut work: Vec<(Vec<u32>, u32, u32)> = Vec::new(); // (faces, tag, profondeur)
    for s in 0..nf {
        if owner[s] != u32::MAX || topo.face_areas[s] <= 0.0 && mesh.face(s).len() < 3 {
            continue;
        }
        let tag = next_tag;
        next_tag += 1;
        let mut faces = vec![s as u32];
        owner[s] = tag;
        let mut i = 0;
        while i < faces.len() {
            let f = faces[i] as usize;
            i += 1;
            for c in mesh.face_range(f) {
                let e = topo.corner_edge[c];
                if cut[e as usize] {
                    continue;
                }
                if let Some(g) = topo.opposite(e, f as u32) {
                    if owner[g as usize] == u32::MAX {
                        owner[g as usize] = tag;
                        faces.push(g);
                    }
                }
            }
        }
        // Stratégie selon la forme du morceau (preset choisi ou détecté)
        let preset = match params.shape {
            ShapePreset::Auto => regions::classify(mesh, topo, &faces, &cut),
            p => p,
        };
        let (dev_tol, cone) = match preset {
            ShapePreset::HardSurface => {
                shapes[0] += 1;
                (0.012, 50f64.to_radians())
            }
            ShapePreset::Cylindrical => {
                shapes[1] += 1;
                (0.03, std::f64::consts::PI)
            }
            _ => {
                shapes[2] += 1;
                work.push((faces, tag, 0));
                continue;
            }
        };
        let regs = regions::developable_regions(mesh, topo, &faces, &cut, dev_tol, cone);
        if regs.len() <= 1 {
            work.push((faces, tag, 0));
            continue;
        }
        let mut tagged = Vec::with_capacity(regs.len());
        for (k, r) in regs.into_iter().enumerate() {
            let t = if k == 0 {
                tag
            } else {
                next_tag += 1;
                next_tag - 1
            };
            for &f in &r {
                owner[f as usize] = t;
            }
            tagged.push((r, t));
        }
        // les frontières entre régions deviennent des coutures
        for &f in &faces {
            for c in mesh.face_range(f as usize) {
                let e = topo.corner_edge[c];
                if let Some(g) = topo.opposite(e, f) {
                    if owner[g as usize] != owner[f as usize] {
                        cut[e as usize] = true;
                    }
                }
            }
        }
        for (r, t) in tagged {
            work.push((r, t, 0));
        }
    }

    let solver_iter = 600 + 400 * params.quality as usize;
    let mut done: Vec<Unwrapped> = Vec::new();
    while let Some((faces, tag, depth)) = work.pop() {
        let chart = build_chart(mesh, topo, &faces, &cut, &owner, tag);
        if chart.tris.is_empty() {
            done.push(Unwrapped { uv: vec![V2::default(); chart.pos.len()], chart, distortion: 0.0, flipped: 0 });
            continue;
        }
        // un anneau plan (couvercle percé…) se déplie tel quel : inutile de le couper
        let planar = !chart.is_disk() && regions::is_planar(topo, &faces, 1.0);
        if !chart.is_disk() && !planar && faces.len() > 1 && depth < MAX_DEPTH {
            // Tube (genre 0, plusieurs bords) : une seule couture discrète suffit.
            if chart.genus() == 0 && chart.boundary_loops >= 2 && cut_between_loops(mesh, topo, &chart, &mut cut) {
                work.push((faces, tag, depth + 1));
                continue;
            }
            split_geodesic(mesh, topo, &faces, &mut cut, &mut owner, &mut next_tag, depth, &mut work);
            continue;
        }
        let mut uv = lscm(&chart, solver_iter);
        normalize_scale(&chart, &mut uv);
        let q_lscm = quality(&chart, &uv);
        let mut q = q_lscm;
        if params.relax_iterations > 0 {
            let backup = uv.clone();
            arap(&chart, &mut uv, params.relax_iterations as usize, 300 + 200 * params.quality as usize);
            normalize_scale(&chart, &mut uv);
            let q_arap = quality(&chart, &uv);
            // On garde ARAP seulement s'il n'introduit pas de retournements
            if q_arap.flipped > q.flipped || !q_arap.distortion.is_finite() {
                uv = backup;
            } else {
                q = q_arap;
            }
        }
        let bad = q.flipped > 0 || q.distortion > params.max_distortion;
        if bad && faces.len() > 1 && depth < MAX_DEPTH {
            split_geodesic(mesh, topo, &faces, &mut cut, &mut owner, &mut next_tag, depth, &mut work);
            continue;
        }
        done.push(Unwrapped { chart, uv, distortion: q.distortion, flipped: q.flipped });
    }
    (done, cut, shapes)
}

/// Coût de traversée d'une arête : on préfère couper sur les plis (moins visibles).
fn crease_factor(dihedral: f64) -> f64 {
    let s = (dihedral / std::f64::consts::PI).clamp(0.0, 1.0);
    0.25 + 0.75 * (1.0 - s).powi(2)
}

/// Ajoute une couture le long du plus court chemin (pondéré par les plis) reliant la
/// première boucle de bord aux autres.
fn cut_between_loops(mesh: &Mesh, topo: &Topology, chart: &Chart, cut: &mut [bool]) -> bool {
    let n = chart.pos.len();
    let mut loop_id = vec![usize::MAX; n];
    for (li, l) in chart.loops.iter().enumerate() {
        for &v in l {
            loop_id[v as usize] = li;
        }
    }
    // 1. « Hauteur » le long du tube : distance (longueur pure) depuis la première boucle.
    let mut len_adj: Vec<Vec<(u32, f64)>> = vec![Vec::new(); n];
    for &(a, b, e) in &chart.interior_edges {
        let l = topo.edges[e as usize].length;
        len_adj[a as usize].push((b, l));
        len_adj[b as usize].push((a, l));
    }
    let mut height = vec![f64::INFINITY; n];
    {
        let mut heap = BinaryHeap::new();
        for &v in &chart.loops[0] {
            height[v as usize] = 0.0;
            heap.push(HeapItem(0.0, v));
        }
        while let Some(HeapItem(d, v)) = heap.pop() {
            if d > height[v as usize] {
                continue;
            }
            for &(w, c) in &len_adj[v as usize] {
                if d + c < height[w as usize] {
                    height[w as usize] = d + c;
                    heap.push(HeapItem(d + c, w));
                }
            }
        }
    }
    // 2. Coût d'arête : une arête qui part « de côté » (sans gagner de hauteur) coûte
    //    beaucoup plus cher. Sans ça, le chemin longeait les rainures horizontales et la
    //    couture faisait des marches d'escalier. Les plis restent un bonus secondaire.
    let mut adj: Vec<Vec<(u32, f64, u32)>> = vec![Vec::new(); n];
    for &(a, b, e) in &chart.interior_edges {
        let ed = &topo.edges[e as usize];
        let l = ed.length.max(1e-12);
        let (ha, hb) = (height[a as usize], height[b as usize]);
        let climb = if ha.is_finite() && hb.is_finite() { ((hb - ha).abs() / l).clamp(0.0, 1.0) } else { 1.0 };
        let sideways = 1.0 - climb;
        let w = l * (1.0 + 8.0 * sideways * sideways) * (0.75 + 0.25 * crease_factor(ed.dihedral));
        adj[a as usize].push((b, w, e));
        adj[b as usize].push((a, w, e));
    }
    let mut dist = vec![f64::INFINITY; n];
    let mut prev: Vec<Option<(u32, u32)>> = vec![None; n];
    let mut heap = BinaryHeap::new();
    for &v in &chart.loops[0] {
        dist[v as usize] = 0.0;
        heap.push(HeapItem(0.0, v));
    }
    let mut target = None;
    while let Some(HeapItem(d, v)) = heap.pop() {
        if d > dist[v as usize] {
            continue;
        }
        if loop_id[v as usize] != 0 && loop_id[v as usize] != usize::MAX {
            target = Some(v);
            break;
        }
        for &(w, c, e) in &adj[v as usize] {
            let nd = d + c;
            if nd < dist[w as usize] {
                dist[w as usize] = nd;
                prev[w as usize] = Some((v, e));
                heap.push(HeapItem(nd, w));
            }
        }
    }
    let Some(mut v) = target else { return false };
    let mut any = false;
    while let Some((p, e)) = prev[v as usize] {
        cut[e as usize] = true;
        any = true;
        v = p;
    }
    let _ = mesh;
    any
}

/// Partition en deux régions connexes par Voronoï géodésique sur le graphe dual,
/// les plis rendant la traversée coûteuse (la frontière tombe donc sur les plis).
#[allow(clippy::too_many_arguments)]
fn split_geodesic(
    mesh: &Mesh,
    topo: &Topology,
    faces: &[u32],
    cut: &mut [bool],
    owner: &mut [u32],
    next_tag: &mut u32,
    depth: u32,
    work: &mut Vec<(Vec<u32>, u32, u32)>,
) {
    let tag = owner[faces[0] as usize];
    let centroids: Vec<_> = faces.iter().map(|&f| mesh.face_centroid(f as usize)).collect();
    let mut local = std::collections::HashMap::with_capacity(faces.len());
    for (i, &f) in faces.iter().enumerate() {
        local.insert(f, i as u32);
    }
    // Graphe dual « 8-connexe » : faces partageant une arête ou un sommet. La métrique obtenue
    // est bien plus proche de l'euclidienne qu'avec les seules arêtes (Manhattan sur une grille
    // de quads), ce qui évite les coutures en escalier.
    let mut vert_faces: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for (i, &f) in faces.iter().enumerate() {
        for &v in mesh.face(f as usize) {
            vert_faces.entry(v).or_default().push(i as u32);
        }
    }
    let mut adj: Vec<Vec<(u32, f64)>> = vec![Vec::new(); faces.len()];
    for (i, &f) in faces.iter().enumerate() {
        let mut nb: Vec<u32> = Vec::new();
        for c in mesh.face_range(f as usize) {
            let e = topo.corner_edge[c];
            if cut[e as usize] {
                continue;
            }
            if let Some(g) = topo.opposite(e, f) {
                if owner[g as usize] == tag {
                    nb.push(local[&g]);
                }
            }
        }
        // voisins par sommet seulement si la face a au moins un voisin par arête non coupée
        if !nb.is_empty() {
            for &v in mesh.face(f as usize) {
                nb.extend(vert_faces[&v].iter().copied());
            }
        }
        nb.sort_unstable();
        nb.dedup();
        for j in nb {
            if j as usize == i {
                continue;
            }
            let d = (centroids[i] - centroids[j as usize]).length().max(1e-12);
            let ang = topo.face_normals[f as usize].dot(topo.face_normals[faces[j as usize] as usize]).clamp(-1.0, 1.0).acos();
            let k = 1.0 + 6.0 * (ang / std::f64::consts::PI);
            adj[i].push((j, d * k));
        }
    }
    let dijkstra = |seeds: &[u32]| -> (Vec<f64>, Vec<u32>) {
        let mut dist = vec![f64::INFINITY; faces.len()];
        let mut lab = vec![u32::MAX; faces.len()];
        let mut heap = BinaryHeap::new();
        for (k, &s) in seeds.iter().enumerate() {
            dist[s as usize] = 0.0;
            lab[s as usize] = k as u32;
            heap.push(HeapItem(0.0, s));
        }
        while let Some(HeapItem(d, v)) = heap.pop() {
            if d > dist[v as usize] {
                continue;
            }
            for &(w, c) in &adj[v as usize] {
                let nd = d + c;
                if nd < dist[w as usize] {
                    dist[w as usize] = nd;
                    lab[w as usize] = lab[v as usize];
                    heap.push(HeapItem(nd, w));
                }
            }
        }
        (dist, lab)
    };
    let farthest = |dist: &[f64]| -> u32 {
        let mut best = 0;
        for i in 0..dist.len() {
            if dist[i].is_finite() && dist[i] > dist[best] {
                best = i;
            }
        }
        best as u32
    };
    let (d0, _) = dijkstra(&[0]);
    let a = farthest(&d0);
    let (da, _) = dijkstra(&[a]);
    let mut b = farthest(&da);
    if a == b {
        b = if a == 0 { 1 } else { 0 };
    }
    let (_, mut lab) = dijkstra(&[a, b]);
    // Lissage de la frontière (filtre majoritaire sur les voisins par arête) : supprime
    // les dents de scie du découpage géodésique sur les grilles de quads.
    let mut edge_nb: Vec<Vec<u32>> = vec![Vec::new(); faces.len()];
    for (i, &f) in faces.iter().enumerate() {
        for c in mesh.face_range(f as usize) {
            let e = topo.corner_edge[c];
            if cut[e as usize] {
                continue;
            }
            if let Some(g) = topo.opposite(e, f) {
                if let Some(&j) = local.get(&g) {
                    edge_nb[i].push(j);
                }
            }
        }
    }
    for _ in 0..4 {
        let mut changed = false;
        for i in 0..faces.len() {
            if i as u32 == a || i as u32 == b || lab[i] > 1 {
                continue;
            }
            let other = edge_nb[i].iter().filter(|&&j| lab[j as usize] <= 1 && lab[j as usize] != lab[i]).count();
            if other * 2 > edge_nb[i].len() {
                lab[i] = 1 - lab[i];
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut part: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
    for (i, &f) in faces.iter().enumerate() {
        let l = if lab[i] == 1 { 1 } else { 0 };
        part[l].push(f);
    }
    if part[1].is_empty() {
        // sécurité : moitié arbitraire
        let h = part[0].len() / 2;
        part[1] = part[0].split_off(h);
    }
    let tag_b = *next_tag;
    *next_tag += 1;
    for &f in &part[1] {
        owner[f as usize] = tag_b;
    }
    // Les arêtes entre les deux régions deviennent des coutures
    for &f in &part[1] {
        for c in mesh.face_range(f as usize) {
            let e = topo.corner_edge[c];
            if let Some(g) = topo.opposite(e, f) {
                if owner[g as usize] == tag {
                    cut[e as usize] = true;
                }
            }
        }
    }
    // Les morceaux peuvent être non connexes (cas « sécurité ») : on recalcule les composantes.
    for (p, t) in [(std::mem::take(&mut part[0]), tag), (std::mem::take(&mut part[1]), tag_b)] {
        for comp in components(mesh, topo, &p, cut, owner, t, next_tag) {
            work.push((comp.0, comp.1, depth + 1));
        }
    }
}

/// Composantes connexes d'un ensemble de faces (à travers les arêtes non coupées).
fn components(mesh: &Mesh, topo: &Topology, faces: &[u32], cut: &[bool], owner: &mut [u32], tag: u32, next_tag: &mut u32) -> Vec<(Vec<u32>, u32)> {
    const VISITING: u32 = u32::MAX - 1;
    let mut out = Vec::new();
    for &s in faces {
        if owner[s as usize] != tag {
            continue;
        }
        let mut comp = vec![s];
        owner[s as usize] = VISITING;
        let mut i = 0;
        while i < comp.len() {
            let f = comp[i];
            i += 1;
            for c in mesh.face_range(f as usize) {
                let e = topo.corner_edge[c];
                if cut[e as usize] {
                    continue;
                }
                if let Some(g) = topo.opposite(e, f) {
                    if owner[g as usize] == tag {
                        owner[g as usize] = VISITING;
                        comp.push(g);
                    }
                }
            }
        }
        out.push(comp);
    }
    let mut res = Vec::new();
    for (k, comp) in out.into_iter().enumerate() {
        let t = if k == 0 {
            tag
        } else {
            let t = *next_tag;
            *next_tag += 1;
            t
        };
        for &f in &comp {
            owner[f as usize] = t;
        }
        res.push((comp, t));
    }
    res
}
