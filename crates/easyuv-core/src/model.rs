//! Scène multi-objets : tous les objets sont fusionnés dans un seul maillage (en espace monde)
//! avec des plages par objet. Le dépliage peut ranger les îlots ensemble ou objet par objet.

use crate::math::V3;
use crate::mesh::Mesh;
use crate::{smart_unwrap, Params, Stats, UvResult};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct ObjRange {
    pub name: String,
    pub vert_start: usize,
    pub vert_end: usize,
    pub face_start: usize,
    pub face_end: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub mesh: Mesh,
    pub objects: Vec<ObjRange>,
}

impl Model {
    pub fn new() -> Self {
        Model { mesh: Mesh::new(), objects: Vec::new() }
    }

    pub fn single(name: impl Into<String>, mesh: Mesh) -> Self {
        let mut m = Model::new();
        m.push(name, &mesh);
        m
    }

    /// Ajoute un objet (ses sommets sont recopiés à la suite).
    pub fn push(&mut self, name: impl Into<String>, mesh: &Mesh) {
        let vs = self.mesh.positions.len();
        let fs = self.mesh.face_count();
        self.mesh.positions.extend_from_slice(&mesh.positions);
        for f in 0..mesh.face_count() {
            let face: Vec<u32> = mesh.face(f).iter().map(|&v| v + vs as u32).collect();
            self.mesh.add_face(&face);
        }
        self.objects.push(ObjRange {
            name: name.into(),
            vert_start: vs,
            vert_end: self.mesh.positions.len(),
            face_start: fs,
            face_end: self.mesh.face_count(),
        });
    }

    pub fn append(&mut self, other: &Model) {
        for (i, o) in other.objects.iter().enumerate() {
            self.push(o.name.clone(), &other.object_mesh(i));
        }
    }

    /// Maillage d'un objet, indices locaux. L'ordre des faces et des coins est conservé.
    pub fn object_mesh(&self, i: usize) -> Mesh {
        let o = &self.objects[i];
        let mut m = Mesh::new();
        m.positions = self.mesh.positions[o.vert_start..o.vert_end].to_vec();
        for f in o.face_start..o.face_end {
            let face: Vec<u32> = self.mesh.face(f).iter().map(|&v| v - o.vert_start as u32).collect();
            m.add_face(&face);
        }
        m
    }

    /// Plage de coins (dans `mesh.corners`) d'un objet.
    pub fn corner_range(&self, i: usize) -> std::ops::Range<usize> {
        let o = &self.objects[i];
        self.mesh.face_start[o.face_start]..self.mesh.face_start[o.face_end]
    }

    /// Déplace un objet (pour poser côte à côte les primitives ajoutées).
    pub fn translate_object(&mut self, i: usize, d: V3) {
        let o = &self.objects[i];
        for p in &mut self.mesh.positions[o.vert_start..o.vert_end] {
            *p = *p + d;
        }
    }
}

/// Dépliage d'une scène. `together` : tous les îlots partagent la même tuile 0-1
/// (densité de texels commune à tous les objets) ; sinon chaque objet a sa propre tuile.
pub fn unwrap_model(model: &Model, params: &Params, together: bool) -> UvResult {
    if together || model.objects.len() <= 1 {
        return smart_unwrap(&model.mesh, params, &[]);
    }
    let t0 = std::time::Instant::now();
    let mut out = UvResult {
        corner_uv: vec![[0.0; 2]; model.mesh.corners.len()],
        face_chart: vec![0; model.mesh.face_count()],
        seams: Vec::new(),
        stats: Stats::default(),
    };
    let mut chart_offset = 0u32;
    let (mut area_sum, mut dist_sum, mut dens_sum, mut cov_sum) = (0.0, 0.0, 0.0, 0.0);
    for i in 0..model.objects.len() {
        let o = &model.objects[i];
        let sub = model.object_mesh(i);
        if sub.face_count() == 0 {
            continue;
        }
        let r = smart_unwrap(&sub, params, &[]);
        let cr = model.corner_range(i);
        out.corner_uv[cr].copy_from_slice(&r.corner_uv);
        for (k, &c) in r.face_chart.iter().enumerate() {
            out.face_chart[o.face_start + k] = c + chart_offset;
        }
        chart_offset += r.stats.charts as u32 + 1;
        out.seams.extend(r.seams.iter().map(|&(a, b)| (a + o.vert_start as u32, b + o.vert_start as u32)));
        let a = sub.surface_area();
        let s = &r.stats;
        out.stats.charts += s.charts;
        out.stats.flipped += s.flipped;
        out.stats.seams += s.seams;
        for k in 0..3 {
            out.stats.shapes[k] += s.shapes[k];
        }
        out.stats.seam_length += s.seam_length * a.sqrt();
        out.stats.overflow |= s.overflow;
        out.stats.max_distortion = out.stats.max_distortion.max(s.max_distortion);
        area_sum += a;
        dist_sum += s.mean_distortion * a;
        dens_sum += s.density_px_per_unit * a;
        cov_sum += s.coverage;
    }
    let n = model.objects.len() as f64;
    if area_sum > 0.0 {
        out.stats.mean_distortion = dist_sum / area_sum;
        out.stats.density_px_per_unit = dens_sum / area_sum;
        out.stats.seam_length /= area_sum.sqrt();
    }
    out.stats.coverage = cov_sum / n;
    out.stats.time_ms = t0.elapsed().as_secs_f64() * 1000.0;
    out
}

/// Soude les sommets confondus (les formats glTF/FBX dupliquent les sommets aux coutures).
pub fn weld(mesh: &Mesh) -> Mesh {
    let (lo, hi) = mesh.bounds();
    let q = ((hi - lo).length() * 1e-6).max(1e-9);
    let mut map: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut remap = Vec::with_capacity(mesh.positions.len());
    let mut out = Mesh::new();
    for p in &mesh.positions {
        let k = ((p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64);
        let id = *map.entry(k).or_insert_with(|| {
            out.positions.push(*p);
            (out.positions.len() - 1) as u32
        });
        remap.push(id);
    }
    let mut face = Vec::new();
    for f in 0..mesh.face_count() {
        face.clear();
        for &v in mesh.face(f) {
            let r = remap[v as usize];
            if face.last() != Some(&r) {
                face.push(r);
            }
        }
        if face.len() > 1 && face[0] == *face.last().unwrap() {
            face.pop();
        }
        if face.len() >= 3 {
            out.add_face(&face);
        }
    }
    out
}

/// Reconstruit des quads à partir de paires de triangles quasi coplanaires (formats triangulés).
/// La diagonale supprimée doit être l'arête la plus longue d'au moins un des triangles et le quad convexe.
pub fn tris_to_quads(mesh: &Mesh) -> Mesh {
    let nf = mesh.face_count();
    let mut edge_tris: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for f in 0..nf {
        let v = mesh.face(f);
        if v.len() != 3 {
            continue;
        }
        for i in 0..3 {
            let (a, b) = (v[i], v[(i + 1) % 3]);
            edge_tris.entry(if a < b { (a, b) } else { (b, a) }).or_default().push(f);
        }
    }
    let p = |i: u32| mesh.positions[i as usize];
    let longest = |f: usize| -> (u32, u32) {
        let v = mesh.face(f);
        let mut best = (0.0, (v[0], v[1]));
        for i in 0..3 {
            let (a, b) = (v[i], v[(i + 1) % 3]);
            let l = (p(b) - p(a)).length();
            if l > best.0 {
                best = (l, if a < b { (a, b) } else { (b, a) });
            }
        }
        best.1
    };
    let mut candidates: Vec<(f64, usize, usize, (u32, u32))> = Vec::new();
    for (&key, fs) in &edge_tris {
        if fs.len() != 2 {
            continue;
        }
        let (f, g) = (fs[0], fs[1]);
        if longest(f) != key && longest(g) != key {
            continue;
        }
        let (nf_, ng) = (mesh.face_normal(f), mesh.face_normal(g));
        let d = nf_.dot(ng);
        if d < 0.966 {
            continue; // > 15° : vrai pli, on garde les triangles
        }
        candidates.push((d, f, g, key));
    }
    candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut used = vec![false; nf];
    let mut quads: Vec<(usize, Vec<u32>)> = Vec::new();
    for (_, f, g, (a, b)) in candidates {
        if used[f] || used[g] {
            continue;
        }
        // quad : depuis f, insérer le sommet opposé de g entre a et b
        let vf = mesh.face(f);
        let opp_g = *mesh.face(g).iter().find(|&&v| v != a && v != b).unwrap();
        let i = (0..3).find(|&i| {
            let (x, y) = (vf[i], vf[(i + 1) % 3]);
            (x == a && y == b) || (x == b && y == a)
        });
        let Some(i) = i else { continue };
        let quad = vec![vf[i], opp_g, vf[(i + 1) % 3], vf[(i + 2) % 3]];
        // convexité
        let n = mesh.face_normal(f);
        let convex = (0..4).all(|k| {
            let (q0, q1, q2) = (p(quad[k]), p(quad[(k + 1) % 4]), p(quad[(k + 2) % 4]));
            (q1 - q0).cross(q2 - q1).dot(n) > 0.0
        });
        if !convex {
            continue;
        }
        used[f] = true;
        used[g] = true;
        quads.push((f.min(g), quad));
    }
    let mut quad_at: HashMap<usize, Vec<u32>> = quads.into_iter().collect();
    let mut out = Mesh::new();
    out.positions = mesh.positions.clone();
    for f in 0..nf {
        if let Some(q) = quad_at.remove(&f) {
            out.add_face(&q);
        } else if !used[f] {
            out.add_face(mesh.face(f));
        }
    }
    out
}
