//! Maillage polygonal et topologie des arêtes.

use crate::math::V3;
use std::collections::HashMap;

/// Maillage polygonal (quads / n-gones acceptés).
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub positions: Vec<V3>,
    /// Indices de sommets, aplatis. La face `f` occupe `corners[face_start[f]..face_start[f+1]]`.
    pub corners: Vec<u32>,
    pub face_start: Vec<usize>,
}

impl Mesh {
    pub fn new() -> Self {
        Mesh { positions: Vec::new(), corners: Vec::new(), face_start: vec![0] }
    }

    pub fn from_faces(positions: Vec<V3>, faces: &[Vec<u32>]) -> Self {
        let mut m = Mesh { positions, corners: Vec::new(), face_start: vec![0] };
        for f in faces {
            m.add_face(f);
        }
        m
    }

    pub fn add_face(&mut self, verts: &[u32]) {
        self.corners.extend_from_slice(verts);
        self.face_start.push(self.corners.len());
    }

    pub fn face_count(&self) -> usize {
        self.face_start.len() - 1
    }

    pub fn face(&self, f: usize) -> &[u32] {
        &self.corners[self.face_start[f]..self.face_start[f + 1]]
    }

    pub fn face_range(&self, f: usize) -> std::ops::Range<usize> {
        self.face_start[f]..self.face_start[f + 1]
    }

    /// Normale de Newell (non normalisée : sa norme vaut 2 x l'aire).
    pub fn face_newell(&self, f: usize) -> V3 {
        let vs = self.face(f);
        let mut n = V3::ZERO;
        for i in 0..vs.len() {
            let a = self.positions[vs[i] as usize];
            let b = self.positions[vs[(i + 1) % vs.len()] as usize];
            n.x += (a.y - b.y) * (a.z + b.z);
            n.y += (a.z - b.z) * (a.x + b.x);
            n.z += (a.x - b.x) * (a.y + b.y);
        }
        n
    }

    pub fn face_normal(&self, f: usize) -> V3 {
        self.face_newell(f).normalized()
    }

    pub fn face_area(&self, f: usize) -> f64 {
        self.face_newell(f).length() * 0.5
    }

    pub fn face_centroid(&self, f: usize) -> V3 {
        let vs = self.face(f);
        let mut c = V3::ZERO;
        for &v in vs {
            c = c + self.positions[v as usize];
        }
        c / vs.len().max(1) as f64
    }

    /// Triangulation en éventail : triplets de *coins locaux* de la face.
    pub fn fan(&self, f: usize) -> impl Iterator<Item = [usize; 3]> {
        let n = self.face_start[f + 1] - self.face_start[f];
        (1..n.saturating_sub(1)).map(|i| [0, i, i + 1])
    }

    pub fn triangle_count(&self) -> usize {
        (0..self.face_count()).map(|f| (self.face(f).len()).saturating_sub(2)).sum()
    }

    pub fn bounds(&self) -> (V3, V3) {
        let mut lo = V3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = V3::new(f64::MIN, f64::MIN, f64::MIN);
        for p in &self.positions {
            lo = V3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = V3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
        if self.positions.is_empty() {
            (V3::ZERO, V3::ZERO)
        } else {
            (lo, hi)
        }
    }

    pub fn surface_area(&self) -> f64 {
        (0..self.face_count()).map(|f| self.face_area(f)).sum()
    }
}

/// Une face incidente à une arête : `ca` = coin global portant le sommet `key.0`,
/// `cb` = coin global portant le sommet `key.1`.
#[derive(Clone, Copy, Debug)]
pub struct EdgeFace {
    pub face: u32,
    pub ca: u32,
    pub cb: u32,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub key: (u32, u32),
    pub faces: Vec<EdgeFace>,
    pub length: f64,
    /// Angle entre normales des faces adjacentes, en radians (0 = plat). PI si bord.
    pub dihedral: f64,
}

/// Topologie d'arêtes partagée par toutes les étapes.
pub struct Topology {
    pub edges: Vec<Edge>,
    /// Pour chaque coin global `c`, l'arête allant de `c` au coin suivant de la face.
    pub corner_edge: Vec<u32>,
    pub corner_face: Vec<u32>,
    pub face_normals: Vec<V3>,
    pub face_areas: Vec<f64>,
    pub edge_map: HashMap<(u32, u32), u32>,
}

pub fn edge_key(a: u32, b: u32) -> (u32, u32) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

impl Topology {
    pub fn build(mesh: &Mesh) -> Self {
        let nf = mesh.face_count();
        let mut edge_map: HashMap<(u32, u32), u32> = HashMap::with_capacity(mesh.corners.len());
        let mut edges: Vec<Edge> = Vec::new();
        let mut corner_edge = vec![0u32; mesh.corners.len()];
        let mut corner_face = vec![0u32; mesh.corners.len()];
        for f in 0..nf {
            let r = mesh.face_range(f);
            let n = r.len();
            for i in 0..n {
                let c0 = r.start + i;
                let c1 = r.start + (i + 1) % n;
                corner_face[c0] = f as u32;
                let a = mesh.corners[c0];
                let b = mesh.corners[c1];
                if a == b {
                    // arête dégénérée : on lui donne quand même un identifiant isolé
                    let id = edges.len() as u32;
                    edges.push(Edge { key: (a, b), faces: vec![], length: 0.0, dihedral: 0.0 });
                    corner_edge[c0] = id;
                    continue;
                }
                let key = edge_key(a, b);
                let id = *edge_map.entry(key).or_insert_with(|| {
                    edges.push(Edge { key, faces: Vec::with_capacity(2), length: 0.0, dihedral: 0.0 });
                    (edges.len() - 1) as u32
                });
                let (ca, cb) = if a == key.0 { (c0, c1) } else { (c1, c0) };
                edges[id as usize].faces.push(EdgeFace { face: f as u32, ca: ca as u32, cb: cb as u32 });
                corner_edge[c0] = id;
            }
        }
        let face_normals: Vec<V3> = (0..nf).map(|f| mesh.face_normal(f)).collect();
        let face_areas: Vec<f64> = (0..nf).map(|f| mesh.face_area(f)).collect();
        for e in edges.iter_mut() {
            let pa = mesh.positions[e.key.0 as usize];
            let pb = mesh.positions[e.key.1 as usize];
            e.length = (pb - pa).length();
            e.dihedral = if e.faces.len() == 2 {
                let n0 = face_normals[e.faces[0].face as usize];
                let n1 = face_normals[e.faces[1].face as usize];
                // Si les deux faces ont un enroulement opposé, les normales sont inversées.
                let consistent = Self::consistent_winding(mesh, &e.faces[0], &e.faces[1]);
                let d = if consistent { n0.dot(n1) } else { -n0.dot(n1) };
                d.clamp(-1.0, 1.0).acos()
            } else {
                std::f64::consts::PI
            };
        }
        Topology { edges, corner_edge, corner_face, face_normals, face_areas, edge_map }
    }

    /// Vrai si les deux faces parcourent l'arête dans des sens opposés (orientation cohérente).
    fn consistent_winding(mesh: &Mesh, f0: &EdgeFace, f1: &EdgeFace) -> bool {
        let dir = |ef: &EdgeFace| -> bool {
            // vrai si ca -> cb suit l'ordre de la face
            let r = mesh.face_range(ef.face as usize);
            let n = r.len();
            let next = r.start + ((ef.ca as usize - r.start) + 1) % n;
            next == ef.cb as usize
        };
        dir(f0) != dir(f1)
    }

    /// Autre face partageant l'arête `e`, si l'arête est manifold.
    pub fn opposite(&self, e: u32, f: u32) -> Option<u32> {
        let ed = &self.edges[e as usize];
        if ed.faces.len() != 2 {
            return None;
        }
        if ed.faces[0].face == f {
            Some(ed.faces[1].face)
        } else if ed.faces[1].face == f {
            Some(ed.faces[0].face)
        } else {
            None
        }
    }
}
