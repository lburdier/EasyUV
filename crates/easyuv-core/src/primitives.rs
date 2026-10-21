//! Primitives de démonstration (mêmes conventions que Blender : Z vers le haut).

use crate::math::V3;
use crate::mesh::Mesh;
use std::f64::consts::PI;

pub fn cube(size: f64) -> Mesh {
    let h = size * 0.5;
    let p = |x: f64, y: f64, z: f64| V3::new(x * h, y * h, z * h);
    let pos = vec![
        p(-1., -1., -1.), p(1., -1., -1.), p(1., 1., -1.), p(-1., 1., -1.),
        p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.),
    ];
    let faces = vec![
        vec![0, 3, 2, 1], vec![4, 5, 6, 7], vec![0, 1, 5, 4],
        vec![1, 2, 6, 5], vec![2, 3, 7, 6], vec![3, 0, 4, 7],
    ];
    Mesh::from_faces(pos, &faces)
}

pub fn cylinder(radius: f64, height: f64, seg: u32, rings: u32) -> Mesh {
    let mut pos = Vec::new();
    for r in 0..=rings {
        let z = -height * 0.5 + height * r as f64 / rings as f64;
        for s in 0..seg {
            let a = 2.0 * PI * s as f64 / seg as f64;
            pos.push(V3::new(radius * a.cos(), radius * a.sin(), z));
        }
    }
    let idx = |r: u32, s: u32| r * seg + (s % seg);
    let mut faces = Vec::new();
    for r in 0..rings {
        for s in 0..seg {
            faces.push(vec![idx(r, s), idx(r, s + 1), idx(r + 1, s + 1), idx(r + 1, s)]);
        }
    }
    faces.push((0..seg).rev().map(|s| idx(0, s)).collect());
    faces.push((0..seg).map(|s| idx(rings, s)).collect());
    Mesh::from_faces(pos, &faces)
}

pub fn uv_sphere(radius: f64, seg: u32, rings: u32) -> Mesh {
    let mut pos = vec![V3::new(0.0, 0.0, -radius)];
    for r in 1..rings {
        let t = PI * r as f64 / rings as f64 - PI * 0.5;
        for s in 0..seg {
            let a = 2.0 * PI * s as f64 / seg as f64;
            pos.push(V3::new(radius * t.cos() * a.cos(), radius * t.cos() * a.sin(), radius * t.sin()));
        }
    }
    pos.push(V3::new(0.0, 0.0, radius));
    let top = pos.len() as u32 - 1;
    let idx = |r: u32, s: u32| 1 + (r - 1) * seg + (s % seg);
    let mut faces = Vec::new();
    for s in 0..seg {
        faces.push(vec![0, idx(1, s + 1), idx(1, s)]);
    }
    for r in 1..rings - 1 {
        for s in 0..seg {
            faces.push(vec![idx(r, s), idx(r, s + 1), idx(r + 1, s + 1), idx(r + 1, s)]);
        }
    }
    for s in 0..seg {
        faces.push(vec![idx(rings - 1, s), idx(rings - 1, s + 1), top]);
    }
    Mesh::from_faces(pos, &faces)
}

pub fn torus(major: f64, minor: f64, seg: u32, sides: u32) -> Mesh {
    let mut pos = Vec::new();
    for i in 0..seg {
        let a = 2.0 * PI * i as f64 / seg as f64;
        for j in 0..sides {
            let b = 2.0 * PI * j as f64 / sides as f64;
            let r = major + minor * b.cos();
            pos.push(V3::new(r * a.cos(), r * a.sin(), minor * b.sin()));
        }
    }
    let idx = |i: u32, j: u32| (i % seg) * sides + (j % sides);
    let mut faces = Vec::new();
    for i in 0..seg {
        for j in 0..sides {
            faces.push(vec![idx(i, j), idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1)]);
        }
    }
    Mesh::from_faces(pos, &faces)
}

/// Forme organique (sphère déformée) pour tester la segmentation par distorsion.
pub fn blob(radius: f64, seg: u32, rings: u32) -> Mesh {
    let mut m = uv_sphere(radius, seg, rings);
    for p in m.positions.iter_mut() {
        let n = p.normalized();
        let k = 1.0 + 0.25 * (3.0 * n.x).sin() * (2.0 * n.y).cos() + 0.35 * n.z.max(0.0).powi(3);
        *p = n * (radius * k);
    }
    m
}

/// Surface de révolution autour de Z à partir d'un profil (r, z) ; les points r = 0
/// deviennent des pôles (éventails de triangles).
pub fn lathe(profile: &[(f64, f64)], seg: u32) -> Mesh {
    let mut pos = Vec::new();
    let mut ring_start = Vec::new();
    for &(r, z) in profile {
        ring_start.push(pos.len() as u32);
        if r <= 1e-9 {
            pos.push(V3::new(0.0, 0.0, z));
        } else {
            for s in 0..seg {
                let a = 2.0 * PI * s as f64 / seg as f64;
                pos.push(V3::new(r * a.cos(), r * a.sin(), z));
            }
        }
    }
    let idx = |k: usize, s: u32| -> u32 { if profile[k].0 <= 1e-9 { ring_start[k] } else { ring_start[k] + s % seg } };
    let mut faces = Vec::new();
    for k in 0..profile.len() - 1 {
        let (pole_a, pole_b) = (profile[k].0 <= 1e-9, profile[k + 1].0 <= 1e-9);
        for s in 0..seg {
            let (a, b, c, d) = (idx(k, s), idx(k, s + 1), idx(k + 1, s + 1), idx(k + 1, s));
            match (pole_a, pole_b) {
                (true, true) => {}
                (true, false) => faces.push(vec![a, c, d]),
                (false, true) => faces.push(vec![a, b, d]),
                _ => faces.push(vec![a, b, c, d]),
            }
        }
    }
    Mesh::from_faces(pos, &faces)
}

/// Pot / cylindre aux arêtes arrondies (biseaux sur `bevel_steps` segments) : aucune
/// arête vive, comme beaucoup de modèles de production.
pub fn beveled_cylinder(radius: f64, height: f64, bevel: f64, bevel_steps: u32, seg: u32, rows: u32) -> Mesh {
    let mut prof = vec![(0.0, 0.0)];
    // fond plat (anneaux concentriques)
    for i in 1..=3 {
        prof.push(((radius - bevel) * i as f64 / 3.0, 0.0));
    }
    for i in 1..=bevel_steps {
        let t = 0.5 * PI * i as f64 / bevel_steps as f64;
        prof.push((radius - bevel + bevel * t.sin(), bevel - bevel * t.cos()));
    }
    for i in 1..rows {
        prof.push((radius, bevel + (height - 2.0 * bevel) * i as f64 / rows as f64));
    }
    for i in 0..=bevel_steps {
        let t = 0.5 * PI * i as f64 / bevel_steps as f64;
        prof.push((radius - bevel + bevel * t.cos(), height - bevel + bevel * t.sin()));
    }
    for i in (0..3).rev() {
        prof.push(((radius - bevel) * i as f64 / 3.0, height));
    }
    lathe(&prof, seg)
}
