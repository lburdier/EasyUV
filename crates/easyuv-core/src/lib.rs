//! EasyUV — dépliage UV automatique intelligent.
//!
//! Pipeline : segmentation (plis + topologie + distorsion) -> LSCM -> relaxation ARAP
//! -> mise à l'échelle monde (densité de texels uniforme) -> empaquetage.

pub mod chart;
pub mod checker;
pub mod ffi;
pub mod glb;
pub mod layout;
pub mod learn;
pub mod math;
pub mod mesh;
pub mod metrics;
pub mod model;
pub mod obj;
pub mod pack;
pub mod primitives;
pub mod regions;
pub mod segment;
pub mod shapepack;
pub mod solver;
pub mod unwrap;
pub mod uvtools;

use math::V2;
use mesh::{Mesh, Topology};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DensityMode {
    /// Remplit au mieux la tuile 0-1 ; la densité obtenue est rapportée.
    Fit,
    /// Densité cible en pixels par unité monde (px/m dans Blender).
    Target(f64),
}

#[derive(Clone, Debug)]
pub struct Params {
    /// Angle dièdre au-delà duquel une arête devient couture.
    pub angle_deg: f64,
    /// Distorsion moyenne tolérée par îlot avant re-découpage (0.05 strict … 0.4 permissif).
    pub max_distortion: f64,
    /// Itérations de relaxation ARAP (0 = LSCM pur).
    pub relax_iterations: u32,
    /// Marge entre îlots en fraction de la tuile UV.
    pub margin: f64,
    pub density: DensityMode,
    pub texture_size: u32,
    /// 0 = rapide, 1 = normal, 2 = haute qualité (itérations du solveur).
    pub quality: u32,
    pub rotate_islands: bool,
    /// Stratégie selon la forme (Auto : détectée pour chaque morceau connexe).
    pub shape: regions::ShapePreset,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            angle_deg: 66.0,
            max_distortion: 0.12,
            relax_iterations: 4,
            margin: 0.008,
            density: DensityMode::Fit,
            texture_size: 2048,
            quality: 1,
            rotate_islands: true,
            shape: regions::ShapePreset::Auto,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub charts: usize,
    pub flipped: usize,
    pub mean_distortion: f64,
    pub max_distortion: f64,
    /// Densité de texels en pixels par unité monde.
    pub density_px_per_unit: f64,
    /// Fraction de la tuile 0-1 couverte par les îlots.
    pub coverage: f64,
    /// Vrai si la densité cible fait déborder la tuile 0-1.
    pub overflow: bool,
    pub seams: usize,
    /// Longueur totale des coutures / racine de l'aire (sans unité, comparable entre maillages).
    pub seam_length: f64,
    pub time_ms: f64,
    /// Morceaux traités par stratégie : [hard-surface, cylindre, organique].
    pub shapes: [usize; 3],
}

#[derive(Clone, Debug)]
pub struct UvResult {
    /// Une coordonnée UV par coin de face (même ordre que `Mesh::corners`).
    pub corner_uv: Vec<[f32; 2]>,
    /// Îlot de chaque face.
    pub face_chart: Vec<u32>,
    /// Coutures finales (paires de sommets).
    pub seams: Vec<(u32, u32)>,
    pub stats: Stats,
}

/// Résolution de la grille d'empaquetage par forme (cellules par côté de tuile).
const PACK_RES: usize = 256;

/// Dépliage complet. `forced_seams` : paires de sommets à couper obligatoirement
/// (coutures / arêtes vives marquées dans Blender).
pub fn smart_unwrap(mesh: &Mesh, params: &Params, forced_seams: &[(u32, u32)]) -> UvResult {
    let t0 = Instant::now();
    let topo = Topology::build(mesh);
    let mut forced = vec![false; topo.edges.len()];
    for &(a, b) in forced_seams {
        if let Some(&e) = topo.edge_map.get(&mesh::edge_key(a, b)) {
            forced[e as usize] = true;
        }
    }
    segment::prof("topologie", t0);
    let t_seg = Instant::now();
    let (mut charts, cut, shapes) = segment::segment_and_unwrap(mesh, &topo, params, &forced);
    segment::prof("segmentation", t_seg);
    let t_pack = Instant::now();
    charts.sort_by_key(|c| c.chart.faces.iter().copied().min().unwrap_or(0));

    let mut uvs: Vec<Vec<V2>> = charts.iter_mut().map(|c| std::mem::take(&mut c.uv)).collect();
    let tri_refs: Vec<&[[u32; 3]]> = charts.iter().map(|c| c.chart.tris.as_slice()).collect();
    let (placed, pack_scale, overflow) = place_islands(&mut uvs, &tri_refs, params, true);
    drop(tri_refs);
    segment::prof("empaquetage", t_pack);

    let mut corner_uv = vec![[0.0f32; 2]; mesh.corners.len()];
    let mut face_chart = vec![u32::MAX; mesh.face_count()];
    let mut uv_area = 0.0;
    let mut stats = Stats { charts: charts.len(), shapes, ..Default::default() };
    for (ci, c) in charts.iter().enumerate() {
        let uv = &placed[ci];
        for &(corner, local) in &c.chart.corner_local {
            let p = uv[local as usize];
            corner_uv[corner as usize] = [p.x as f32, p.y as f32];
        }
        for &f in &c.chart.faces {
            face_chart[f as usize] = ci as u32;
        }
        for t in &c.chart.tris {
            uv_area += 0.5 * (uv[t[1] as usize] - uv[t[0] as usize]).cross(uv[t[2] as usize] - uv[t[0] as usize]).abs();
        }
        stats.flipped += c.flipped;
        stats.max_distortion = stats.max_distortion.max(c.distortion);
        stats.mean_distortion += c.distortion * c.chart.faces.iter().map(|&f| topo.face_areas[f as usize]).sum::<f64>();
    }
    let total_area: f64 = topo.face_areas.iter().sum();
    if total_area > 0.0 {
        stats.mean_distortion /= total_area;
    }
    // Faces non dépliables (dégénérées) : îlot à part, UV nulles
    for fc in face_chart.iter_mut() {
        if *fc == u32::MAX {
            *fc = charts.len() as u32;
        }
    }
    stats.density_px_per_unit = pack_scale * params.texture_size as f64;
    stats.coverage = uv_area;
    stats.overflow = overflow;

    // Coutures : arêtes coupées, ou entre deux îlots différents
    let mut seams = Vec::new();
    for (i, e) in topo.edges.iter().enumerate() {
        // les bords ouverts du maillage ne sont pas des coutures
        let is_seam = e.faces.len() > 2
            || (e.faces.len() == 2
                && cut[i]
                && (face_chart[e.faces[0].face as usize] != face_chart[e.faces[1].face as usize]
                    || corner_uv[e.faces[0].ca as usize] != corner_uv[e.faces[1].ca as usize]
                    || corner_uv[e.faces[0].cb as usize] != corner_uv[e.faces[1].cb as usize]));
        if is_seam {
            seams.push(e.key);
            stats.seam_length += e.length;
        }
    }
    stats.seams = seams.len();
    if total_area > 0.0 {
        stats.seam_length /= total_area.sqrt();
    }
    stats.time_ms = t0.elapsed().as_secs_f64() * 1000.0;
    UvResult { corner_uv, face_chart, seams, stats }
}

/// Range des îlots (UV en unités monde) dans la tuile 0-1 : orientation (si `orient`), puis
/// empaquetage par boîtes et par formes, la meilleure des deux. Retourne les UV placées,
/// l'échelle monde -> UV et le débordement (UDIM).
pub(crate) fn place_islands(uvs: &mut [Vec<V2>], tris: &[&[[u32; 3]]], params: &Params, orient: bool) -> (Vec<Vec<V2>>, f64, bool) {
    // Orientation + boîtes
    let rects: Vec<(f64, f64)> = uvs.iter_mut().map(|uv| if orient { pack::orient_min_box(uv) } else { pack::to_origin(uv) }).collect();
    let fixed = match params.density {
        DensityMode::Fit => None,
        DensityMode::Target(px) => Some(px / params.texture_size.max(1) as f64),
    };
    // Empaquetage par forme ; à défaut (cas extrêmes), par boîtes englobantes.
    let islands: Vec<shapepack::Island> = uvs.iter().zip(tris).map(|(uv, t)| shapepack::Island { uv, tris: t }).collect();
    // résolution adaptée au nombre d'îlots (coût de la recherche)
    let n = uvs.len();
    let res = std::env::var("EASYUV_PACK_RES").ok().and_then(|v| v.parse().ok()).unwrap_or(if n <= 64 {
        1024
    } else if n <= 200 {
        768
    } else {
        PACK_RES
    });
    // Deux méthodes : boîtes (skyline, rapide) puis formes, gardée seulement si elle fait mieux.
    // En densité cible, la méthode par formes est utilisée pour passer en UDIM sans chevauchement.
    let packed = pack::pack(&rects, params.margin, fixed, params.rotate_islands);
    let shape = if fixed.is_some() && !packed.overflow {
        None
    } else {
        let floor = if fixed.is_some() { 0.0 } else { packed.scale };
        shapepack::pack_shapes(&islands, params.margin, fixed, params.rotate_islands, res, floor)
    };
    drop(islands);
    match shape {
        Some(sp) => (uvs.iter().zip(&sp.xforms).map(|(uv, xf)| uv.iter().map(|&p| xf.apply(p, sp.scale)).collect()).collect(), sp.scale, sp.tiles > 1),
        None => {
            let placed = uvs
                .iter()
                .enumerate()
                .map(|(ci, uv)| {
                    let pl = packed.placements[ci];
                    let (_, h) = rects[ci];
                    uv.iter()
                        .map(|&p| {
                            let q = if pl.rotated { V2::new(h - p.y, p.x) } else { p };
                            (q + V2::new(pl.x, pl.y)) * packed.scale
                        })
                        .collect()
                })
                .collect();
            (placed, packed.scale, packed.overflow)
        }
    }
}

/// Densité de texels moyenne (px / unité monde) d'UV existantes.
pub fn measure_density(mesh: &Mesh, corner_uv: &[[f32; 2]], texture_size: u32) -> f64 {
    let mut a3 = 0.0;
    let mut au = 0.0;
    for f in 0..mesh.face_count() {
        let r = mesh.face_range(f);
        for t in mesh.fan(f) {
            let p = |k: usize| mesh.positions[mesh.corners[r.start + t[k]] as usize];
            let u = |k: usize| {
                let c = corner_uv[r.start + t[k]];
                V2::new(c[0] as f64, c[1] as f64)
            };
            a3 += 0.5 * (p(1) - p(0)).cross(p(2) - p(0)).length();
            au += 0.5 * (u(1) - u(0)).cross(u(2) - u(0)).abs();
        }
    }
    if a3 <= 0.0 {
        return 0.0;
    }
    (au / a3).sqrt() * texture_size as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(mesh: &Mesh, name: &str) -> UvResult {
        let r = smart_unwrap(mesh, &Params::default(), &[]);
        let s = &r.stats;
        println!(
            "{name}: charts={} flipped={} dist={:.3} (max {:.3}) density={:.1}px/u coverage={:.1}% seams={} {:.1}ms",
            s.charts, s.flipped, s.mean_distortion, s.max_distortion, s.density_px_per_unit, s.coverage * 100.0, s.seams, s.time_ms
        );
        for uv in &r.corner_uv {
            assert!(uv[0].is_finite() && uv[1].is_finite());
            assert!(uv[0] >= -1e-4 && uv[0] <= 1.0001 && uv[1] >= -1e-4 && uv[1] <= 1.0001, "{name}: uv hors tuile {uv:?}");
        }
        assert_eq!(s.flipped, 0, "{name}: triangles retournés");
        r
    }

    #[test]
    fn cube_six_islands() {
        let r = check(&primitives::cube(2.0), "cube");
        assert_eq!(r.stats.charts, 6);
        assert!(r.stats.mean_distortion < 1e-3);
    }

    #[test]
    fn cylinder_strip_and_caps() {
        let r = check(&primitives::cylinder(1.0, 3.0, 32, 6), "cylindre");
        assert_eq!(r.stats.charts, 3, "le tube doit rester une seule bande");
    }

    #[test]
    fn sphere_torus_blob() {
        check(&primitives::uv_sphere(1.0, 32, 16), "sphere");
        check(&primitives::torus(1.0, 0.35, 48, 16), "tore");
        check(&primitives::blob(1.0, 48, 24), "blob");
    }

    #[test]
    fn target_density() {
        let mut p = Params::default();
        p.density = DensityMode::Target(512.0);
        p.texture_size = 2048;
        let m = primitives::cube(1.0);
        let r = smart_unwrap(&m, &p, &[]);
        let d = measure_density(&m, &r.corner_uv, 2048);
        assert!((d - 512.0).abs() < 1.0, "densité mesurée {d}");
    }

    #[test]
    fn multi_object_together_vs_separate() {
        let mut m = model::Model::single("Cube", primitives::cube(2.0));
        m.push("Tore", &primitives::torus(1.0, 0.35, 32, 12));
        m.translate_object(1, math::V3::new(4.0, 0.0, 0.0));
        let p = Params::default();
        let t = model::unwrap_model(&m, &p, true);
        let s = model::unwrap_model(&m, &p, false);
        assert_eq!(t.corner_uv.len(), m.mesh.corners.len());
        // ensemble : une seule densité pour les deux objets
        let d_cube = measure_density(&m.object_mesh(0), &t.corner_uv[m.corner_range(0)], 2048);
        let d_tore = measure_density(&m.object_mesh(1), &t.corner_uv[m.corner_range(1)], 2048);
        assert!((d_cube - d_tore).abs() / d_cube < 0.02, "{d_cube} vs {d_tore}");
        // séparé : chaque objet remplit sa tuile (densité du cube plus élevée)
        let d_cube_sep = measure_density(&m.object_mesh(0), &s.corner_uv[m.corner_range(0)], 2048);
        assert!(d_cube_sep > d_cube * 1.2);
        assert_eq!(s.stats.flipped + t.stats.flipped, 0);
    }

    #[test]
    fn weld_and_quads() {
        // cube triangulé avec sommets dupliqués par face (comme un glTF)
        let c = primitives::cube(1.0);
        let mut soup = mesh::Mesh::new();
        for f in 0..c.face_count() {
            let v = c.face(f);
            let base = soup.positions.len() as u32;
            for &i in v {
                soup.positions.push(c.positions[i as usize]);
            }
            soup.add_face(&[base, base + 1, base + 2]);
            soup.add_face(&[base, base + 2, base + 3]);
        }
        let w = model::weld(&soup);
        assert_eq!(w.positions.len(), 8);
        let q = model::tris_to_quads(&w);
        assert_eq!(q.face_count(), 6);
        assert_eq!(smart_unwrap(&q, &Params::default(), &[]).stats.charts, 6);
    }

    #[test]
    fn obj_multi_and_glb() {
        let mut m = model::Model::single("A", primitives::cube(1.0));
        m.push("B", &primitives::cylinder(0.5, 1.0, 16, 2));
        let r = model::unwrap_model(&m, &Params::default(), true);
        let txt = obj::write_obj_model(&m, Some(&r.corner_uv));
        let back = obj::parse_obj_model(&txt, "x").unwrap();
        assert_eq!(back.objects.len(), 2);
        assert_eq!(back.objects[1].name, "B");
        assert_eq!(back.mesh.face_count(), m.mesh.face_count());
        let p0 = m.mesh.positions[0];
        let q0 = back.mesh.positions[0];
        assert!((p0 - q0).length() < 1e-5, "conversion des axes non reversible");
        let glb = glb::write_glb(&m, Some(&r.corner_uv));
        assert_eq!(&glb[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize, glb.len());
    }

    #[test]
    fn obj_z_up_detected() {
        // pot posé au sol, écrit en Z vers le haut (hauteur 0..2 en z) : doit rester debout
        let txt = "o Pot\nv -1 -1 0\nv 1 -1 0\nv 1 1 0\nv -1 1 0\nv -1 -1 2\nv 1 -1 2\nv 1 1 2\nv -1 1 2\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";
        let m = obj::parse_obj_model(txt, "x").unwrap();
        let (lo, hi) = m.mesh.bounds();
        assert!((lo.z - 0.0).abs() < 1e-9 && (hi.z - 2.0).abs() < 1e-9, "{lo:?} {hi:?}");
        // le même pot en Y vers le haut (standard OBJ)
        let txt_y = "o Pot\nv -1 0 -1\nv 1 0 -1\nv 1 0 1\nv -1 0 1\nv -1 2 -1\nv 1 2 -1\nv 1 2 1\nv -1 2 1\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";
        let (lo, hi) = obj::parse_obj_model(txt_y, "x").unwrap().mesh.bounds();
        assert!((lo.z - 0.0).abs() < 1e-9 && (hi.z - 2.0).abs() < 1e-9, "{lo:?} {hi:?}");
    }

    #[test]
    fn obj_roundtrip() {
        let m = primitives::cube(1.0);
        let r = smart_unwrap(&m, &Params::default(), &[]);
        let txt = obj::write_obj(&m, Some(&r.corner_uv));
        let m2 = obj::parse_obj(&txt).unwrap();
        assert_eq!(m2.face_count(), 6);
    }

    #[test]
    fn beveled_pot_gives_straight_wall_strip() {
        // aucune arête vive : l'ancien découpage géodésique faisait des escaliers
        let m = primitives::beveled_cylinder(1.0, 2.0, 0.15, 4, 48, 8);
        let topo = mesh::Topology::build(&m);
        assert!(topo.edges.iter().all(|e| e.faces.len() != 2 || e.dihedral < 66f64.to_radians()));
        let r = check(&m, "pot biseauté");
        assert_eq!(r.stats.shapes, [0, 1, 0], "doit être détecté comme cylindre");
        assert!(r.stats.charts <= 9, "{} îlots", r.stats.charts);
        assert!(r.stats.mean_distortion < 0.05, "distorsion {}", r.stats.mean_distortion);
        // la paroi (8 rangées x 48 segments, faces 4 sommets à rayon plein) est d'un seul tenant
        let wall: Vec<usize> = (0..m.face_count())
            .filter(|&f| m.face(f).len() == 4 && m.face(f).iter().all(|&v| ((m.positions[v as usize].x.powi(2) + m.positions[v as usize].y.powi(2)).sqrt() - 1.0).abs() < 1e-9))
            .collect();
        assert_eq!(wall.len(), 48 * 8);
        let chart = r.face_chart[wall[0]];
        assert!(wall.iter().all(|&f| r.face_chart[f] == chart), "paroi découpée en plusieurs îlots");
    }
}