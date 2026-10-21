//! Construction d'un îlot UV (chart) à partir d'un ensemble de faces et des coutures.

use crate::math::V3;
use crate::mesh::{Mesh, Topology};
use std::collections::HashMap;

pub struct UnionFind {
    parent: Vec<u32>,
}

impl UnionFind {
    pub fn new(n: usize) -> Self {
        UnionFind { parent: (0..n as u32).collect() }
    }
    pub fn find(&mut self, mut x: u32) -> u32 {
        while self.parent[x as usize] != x {
            let p = self.parent[x as usize];
            self.parent[x as usize] = self.parent[p as usize];
            x = p;
        }
        x
    }
    pub fn union(&mut self, a: u32, b: u32) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[ra as usize] = rb;
        }
    }
}

/// Îlot prêt à être déplié.
pub struct Chart {
    pub faces: Vec<u32>,
    /// Position 3D de chaque sommet UV local.
    pub pos: Vec<V3>,
    /// Sommet du maillage d'origine pour chaque sommet local.
    pub mesh_vert: Vec<u32>,
    /// Triangles en indices locaux.
    pub tris: Vec<[u32; 3]>,
    /// Pour chaque coin global des faces de l'îlot : (coin global, sommet local).
    pub corner_local: Vec<(u32, u32)>,
    pub euler: i64,
    pub boundary_loops: usize,
    /// Sommets locaux par boucle de bord.
    pub loops: Vec<Vec<u32>>,
    /// Arêtes intérieures (non coupées) : (sommet local a, sommet local b, arête du maillage).
    pub interior_edges: Vec<(u32, u32, u32)>,
}

impl Chart {
    pub fn is_disk(&self) -> bool {
        self.euler == 1 && self.boundary_loops == 1
    }
    /// Genre topologique (χ = 2 - 2g - b).
    pub fn genus(&self) -> i64 {
        (2 - self.boundary_loops as i64 - self.euler) / 2
    }
}

/// Construit l'îlot formé par `faces`. `cut[e]` indique une couture ; `owner[f]` vaut
/// `tag` pour les faces de l'ensemble (les arêtes vers d'autres ensembles sont des coutures).
pub fn build_chart(mesh: &Mesh, topo: &Topology, faces: &[u32], cut: &[bool], owner: &[u32], tag: u32) -> Chart {
    // Index local des coins
    let mut corner_ids: HashMap<u32, u32> = HashMap::new();
    let mut corner_list: Vec<u32> = Vec::new();
    for &f in faces {
        for c in mesh.face_range(f as usize) {
            corner_ids.insert(c as u32, corner_list.len() as u32);
            corner_list.push(c as u32);
        }
    }
    let mut uf = UnionFind::new(corner_list.len());
    for &f in faces {
        for c in mesh.face_range(f as usize) {
            let e = topo.corner_edge[c] as usize;
            if cut[e] {
                continue;
            }
            let ed = &topo.edges[e];
            if ed.faces.len() != 2 {
                continue;
            }
            let (a, b) = (&ed.faces[0], &ed.faces[1]);
            if owner[a.face as usize] != tag || owner[b.face as usize] != tag || a.face == b.face {
                continue;
            }
            uf.union(corner_ids[&a.ca], corner_ids[&b.ca]);
            uf.union(corner_ids[&a.cb], corner_ids[&b.cb]);
        }
    }
    let mut root_local: HashMap<u32, u32> = HashMap::new();
    let mut pos = Vec::new();
    let mut mesh_vert = Vec::new();
    let mut corner_local = Vec::with_capacity(corner_list.len());
    for (i, &c) in corner_list.iter().enumerate() {
        let r = uf.find(i as u32);
        let id = *root_local.entry(r).or_insert_with(|| {
            let v = mesh.corners[c as usize];
            pos.push(mesh.positions[v as usize]);
            mesh_vert.push(v);
            (pos.len() - 1) as u32
        });
        corner_local.push((c, id));
    }
    let local_of = |c: usize| corner_local[corner_ids[&(c as u32)] as usize].1;
    let mut tris = Vec::new();
    for &f in faces {
        let r = mesh.face_range(f as usize);
        for t in mesh.fan(f as usize) {
            let tri = [local_of(r.start + t[0]), local_of(r.start + t[1]), local_of(r.start + t[2])];
            if tri[0] != tri[1] && tri[1] != tri[2] && tri[0] != tri[2] {
                tris.push(tri);
            }
        }
    }
    // Topologie : arêtes uniques et bords (en considérant les arêtes de polygone, pas les diagonales)
    let mut edge_count: HashMap<(u32, u32), u32> = HashMap::new();
    let mut directed: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut poly_edges: Vec<(u32, u32)> = Vec::new();
    let mut edge_mesh_id: HashMap<(u32, u32), u32> = HashMap::new();
    for &f in faces {
        let r = mesh.face_range(f as usize);
        let n = r.len();
        for i in 0..n {
            let a = local_of(r.start + i);
            let b = local_of(r.start + (i + 1) % n);
            if a == b {
                continue;
            }
            poly_edges.push((a, b));
            edge_mesh_id.insert(if a < b { (a, b) } else { (b, a) }, topo.corner_edge[r.start + i]);
            *edge_count.entry(if a < b { (a, b) } else { (b, a) }).or_insert(0) += 1;
        }
    }
    let mut n_edges = edge_count.len() as i64;
    // les diagonales de triangulation ajoutent autant d'arêtes que de triangles en plus des faces
    let n_tris_total: i64 = faces.iter().map(|&f| mesh.face(f as usize).len() as i64 - 2).sum();
    n_edges += n_tris_total - faces.len() as i64;
    let euler = pos.len() as i64 - n_edges + n_tris_total;

    for &(a, b) in &poly_edges {
        let k = if a < b { (a, b) } else { (b, a) };
        if edge_count[&k] == 1 {
            directed.entry(a).or_default().push(b);
        }
    }
    // Boucles de bord : composantes connexes du graphe de bord
    let mut loops: Vec<Vec<u32>> = Vec::new();
    let mut visited: HashMap<u32, bool> = HashMap::new();
    let mut bverts: Vec<u32> = directed.keys().copied().collect();
    for vs in directed.values() {
        bverts.extend(vs.iter().copied());
    }
    bverts.sort_unstable();
    bverts.dedup();
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&a, bs) in &directed {
        for &b in bs {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
    }
    for &s in &bverts {
        if visited.get(&s).copied().unwrap_or(false) {
            continue;
        }
        let mut comp = Vec::new();
        let mut stack = vec![s];
        visited.insert(s, true);
        while let Some(v) = stack.pop() {
            comp.push(v);
            if let Some(ns) = adj.get(&v) {
                for &n in ns {
                    if !visited.get(&n).copied().unwrap_or(false) {
                        visited.insert(n, true);
                        stack.push(n);
                    }
                }
            }
        }
        // ordre déterministe (l'ordre d'itération des HashMap est aléatoire)
        comp.sort_unstable();
        loops.push(comp);
    }
    let mut interior_edges: Vec<(u32, u32, u32)> = edge_count
        .iter()
        .filter(|(_, &c)| c == 2)
        .map(|(&(a, b), _)| (a, b, edge_mesh_id[&(a, b)]))
        .collect();
    interior_edges.sort_unstable();
    Chart {
        interior_edges,
        faces: faces.to_vec(),
        pos,
        mesh_vert,
        tris,
        corner_local,
        euler,
        boundary_loops: loops.len(),
        loops,
    }
}
