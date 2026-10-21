//! API C exportée par `easyuv.dll` (utilisée par l'addon Blender via ctypes).
//! Toutes les fonctions renvoient 0 en cas de succès, une valeur négative sinon.

use crate::checker::{generate_rgba_f32_bottom_up, CheckerStyle};
use crate::math::V3;
use crate::mesh::Mesh;
use crate::{measure_density, smart_unwrap, DensityMode, Params};
use std::panic::catch_unwind;
use std::slice;

/// 2 : ajout de `euv_unwrap_learned` (auto-apprentissage).
pub const EUV_API_VERSION: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EuvParams {
    pub angle_deg: f32,
    pub max_distortion: f32,
    pub relax_iterations: u32,
    pub margin: f32,
    /// 0 = remplir la tuile, 1 = densité cible
    pub density_mode: u32,
    pub texture_size: u32,
    /// px par unité monde
    pub target_density: f32,
    pub quality: u32,
    pub rotate_islands: u32,
    /// Auto-apprentissage : 0 = moins de coutures, 1 = moins de distorsion.
    pub priority: f32,
    /// Forme : 0 = auto, 1 = hard-surface, 2 = cylindre, 3 = organique (API v3).
    pub shape: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct EuvStats {
    pub charts: u32,
    pub flipped: u32,
    pub seams: u32,
    pub overflow: u32,
    pub mean_distortion: f32,
    pub max_distortion: f32,
    pub density_px_per_unit: f32,
    pub coverage: f32,
    pub time_ms: f32,
}

unsafe fn build_mesh(positions: *const f32, nv: u32, face_sizes: *const u32, nf: u32, corners: *const u32, nc: u32) -> Option<Mesh> {
    if positions.is_null() || face_sizes.is_null() || corners.is_null() {
        return None;
    }
    let p = slice::from_raw_parts(positions, nv as usize * 3);
    let fs = slice::from_raw_parts(face_sizes, nf as usize);
    let cs = slice::from_raw_parts(corners, nc as usize);
    let mut m = Mesh::new();
    m.positions = p.chunks_exact(3).map(|c| V3::new(c[0] as f64, c[1] as f64, c[2] as f64)).collect();
    let mut k = 0usize;
    for &s in fs {
        let s = s as usize;
        if k + s > cs.len() {
            return None;
        }
        let face = &cs[k..k + s];
        if face.iter().any(|&v| v >= nv) {
            return None;
        }
        m.add_face(face);
        k += s;
    }
    if k != cs.len() {
        return None;
    }
    Some(m)
}

#[no_mangle]
pub extern "C" fn euv_api_version() -> u32 {
    EUV_API_VERSION
}

/// Déplie un maillage. `out_uv` : 2 floats par coin ; `out_face_chart` : 1 u32 par face ;
/// `seams` : paires de sommets forcées (peut être nul si `nseams` = 0).
/// `out_seams` (optionnel) reçoit jusqu'à `out_seams_cap` paires ; `out_nseams` leur nombre total.
///
/// # Safety
/// Les pointeurs doivent référencer des tableaux de la taille indiquée.
#[no_mangle]
pub unsafe extern "C" fn euv_unwrap(
    positions: *const f32,
    nv: u32,
    face_sizes: *const u32,
    nf: u32,
    corners: *const u32,
    nc: u32,
    seams: *const u32,
    nseams: u32,
    params: *const EuvParams,
    out_uv: *mut f32,
    out_face_chart: *mut u32,
    out_seams: *mut u32,
    out_seams_cap: u32,
    out_nseams: *mut u32,
    out_stats: *mut EuvStats,
) -> i32 {
    let r = catch_unwind(|| {
        let Some(mesh) = build_mesh(positions, nv, face_sizes, nf, corners, nc) else { return -1 };
        if params.is_null() || out_uv.is_null() {
            return -2;
        }
        let sp = *params;
        let p = Params {
            angle_deg: sp.angle_deg as f64,
            max_distortion: sp.max_distortion as f64,
            relax_iterations: sp.relax_iterations,
            margin: sp.margin as f64,
            density: if sp.density_mode == 1 { DensityMode::Target(sp.target_density as f64) } else { DensityMode::Fit },
            texture_size: sp.texture_size.max(1),
            quality: sp.quality.min(3),
            rotate_islands: sp.rotate_islands != 0,
            shape: crate::regions::ShapePreset::from_u32(sp.shape),
        };
        let forced: Vec<(u32, u32)> = if seams.is_null() || nseams == 0 {
            Vec::new()
        } else {
            slice::from_raw_parts(seams, nseams as usize * 2).chunks_exact(2).map(|c| (c[0], c[1])).collect()
        };
        let res = smart_unwrap(&mesh, &p, &forced);
        let ouv = slice::from_raw_parts_mut(out_uv, nc as usize * 2);
        for (i, uv) in res.corner_uv.iter().enumerate() {
            ouv[2 * i] = uv[0];
            ouv[2 * i + 1] = uv[1];
        }
        if !out_face_chart.is_null() {
            slice::from_raw_parts_mut(out_face_chart, nf as usize).copy_from_slice(&res.face_chart);
        }
        if !out_seams.is_null() {
            let os = slice::from_raw_parts_mut(out_seams, out_seams_cap as usize * 2);
            for (i, &(a, b)) in res.seams.iter().take(out_seams_cap as usize).enumerate() {
                os[2 * i] = a;
                os[2 * i + 1] = b;
            }
        }
        if !out_nseams.is_null() {
            *out_nseams = res.seams.len() as u32;
        }
        if !out_stats.is_null() {
            let s = &res.stats;
            *out_stats = EuvStats {
                charts: s.charts as u32,
                flipped: s.flipped as u32,
                seams: s.seams as u32,
                overflow: s.overflow as u32,
                mean_distortion: s.mean_distortion as f32,
                max_distortion: s.max_distortion as f32,
                density_px_per_unit: s.density_px_per_unit as f32,
                coverage: s.coverage as f32,
                time_ms: s.time_ms as f32,
            };
        }
        0
    });
    r.unwrap_or(-100)
}

/// Densité de texels moyenne (px / unité) des UV fournies (2 floats par coin). < 0 si erreur.
///
/// # Safety
/// Les pointeurs doivent référencer des tableaux de la taille indiquée.
#[no_mangle]
pub unsafe extern "C" fn euv_measure_density(
    positions: *const f32,
    nv: u32,
    face_sizes: *const u32,
    nf: u32,
    corners: *const u32,
    nc: u32,
    uvs: *const f32,
    texture_size: u32,
) -> f32 {
    catch_unwind(|| {
        let Some(mesh) = build_mesh(positions, nv, face_sizes, nf, corners, nc) else { return -1.0 };
        if uvs.is_null() {
            return -2.0;
        }
        let u = slice::from_raw_parts(uvs, nc as usize * 2);
        let cuv: Vec<[f32; 2]> = u.chunks_exact(2).map(|c| [c[0], c[1]]).collect();
        measure_density(&mesh, &cuv, texture_size) as f32
    })
    .unwrap_or(-100.0)
}

/// Remplit `out` (size*size*4 floats, lignes de bas en haut) avec un damier.
///
/// # Safety
/// `out` doit contenir size*size*4 floats.
#[no_mangle]
pub unsafe extern "C" fn euv_checker(size: u32, cells: u32, style: u32, out: *mut f32) -> i32 {
    if out.is_null() || size == 0 {
        return -1;
    }
    catch_unwind(|| {
        let o = slice::from_raw_parts_mut(out, (size * size * 4) as usize);
        generate_rgba_f32_bottom_up(size, cells, CheckerStyle::from_u32(style), o);
        0
    })
    .unwrap_or(-100)
}


/// Informations sur un dépliage appris.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct EuvLearnInfo {
    pub angle_deg: f32,
    pub max_distortion: f32,
    pub relax_iterations: u32,
    pub trials: u32,
    pub score: f32,
    pub default_score: f32,
    pub confidence: f32,
    /// Nombre total d'expériences du modèle après ce dépliage.
    pub experiences: u32,
}

/// Comme `euv_unwrap`, mais avec l'auto-apprentissage : `trials` réglages essayés en parallèle,
/// le meilleur est retenu et l'expérience est ajoutée au fichier `learn_path` (UTF-8 terminé par
/// un zéro ; nul = emplacement par défaut, partagé avec l'application EasyUV).
/// Angle, tolérance et ARAP de `params` servent de suggestion ; le reste est respecté.
///
/// # Safety
/// Mêmes exigences que `euv_unwrap` ; `learn_path` nul ou chaîne C valide.
#[no_mangle]
pub unsafe extern "C" fn euv_unwrap_learned(
    positions: *const f32,
    nv: u32,
    face_sizes: *const u32,
    nf: u32,
    corners: *const u32,
    nc: u32,
    seams: *const u32,
    nseams: u32,
    params: *const EuvParams,
    learn_path: *const std::os::raw::c_char,
    trials: u32,
    out_uv: *mut f32,
    out_face_chart: *mut u32,
    out_seams: *mut u32,
    out_seams_cap: u32,
    out_nseams: *mut u32,
    out_stats: *mut EuvStats,
    out_info: *mut EuvLearnInfo,
) -> i32 {
    let r = catch_unwind(|| {
        let Some(mesh) = build_mesh(positions, nv, face_sizes, nf, corners, nc) else { return -1 };
        if params.is_null() || out_uv.is_null() {
            return -2;
        }
        let p = to_params(&*params);
        let forced: Vec<(u32, u32)> = if seams.is_null() || nseams == 0 {
            Vec::new()
        } else {
            slice::from_raw_parts(seams, nseams as usize * 2).chunks_exact(2).map(|c| (c[0], c[1])).collect()
        };
        let path = if learn_path.is_null() {
            crate::learn::default_path()
        } else {
            match std::ffi::CStr::from_ptr(learn_path).to_str() {
                Ok(s) if !s.is_empty() => std::path::PathBuf::from(s),
                _ => crate::learn::default_path(),
            }
        };
        let mut learner = crate::learn::Learner::load(&path);
        learner.priority = ((*params).priority as f64).clamp(0.0, 1.0);
        let model = crate::model::Model::single("blender", mesh);
        let res = crate::learn::unwrap_learned_with_seams(&model, &p, true, &mut learner, trials.clamp(2, 64) as usize, &forced);
        let _ = learner.save(&path);
        write_result(&res.result, nc, nf, out_uv, out_face_chart, out_seams, out_seams_cap, out_nseams, out_stats);
        if !out_info.is_null() {
            *out_info = EuvLearnInfo {
                angle_deg: res.params.angle_deg as f32,
                max_distortion: res.params.max_distortion as f32,
                relax_iterations: res.params.relax_iterations,
                trials: res.trials as u32,
                score: res.score as f32,
                default_score: res.default_score as f32,
                confidence: res.confidence as f32,
                experiences: learner.samples.len() as u32,
            };
        }
        0
    });
    r.unwrap_or(-100)
}

fn to_params(sp: &EuvParams) -> Params {
    Params {
        angle_deg: sp.angle_deg as f64,
        max_distortion: sp.max_distortion as f64,
        relax_iterations: sp.relax_iterations,
        margin: sp.margin as f64,
        density: if sp.density_mode == 1 { DensityMode::Target(sp.target_density as f64) } else { DensityMode::Fit },
        texture_size: sp.texture_size.max(1),
        quality: sp.quality.min(3),
        rotate_islands: sp.rotate_islands != 0,
        shape: crate::regions::ShapePreset::from_u32(sp.shape),
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn write_result(
    res: &crate::UvResult,
    nc: u32,
    nf: u32,
    out_uv: *mut f32,
    out_face_chart: *mut u32,
    out_seams: *mut u32,
    out_seams_cap: u32,
    out_nseams: *mut u32,
    out_stats: *mut EuvStats,
) {
    let ouv = slice::from_raw_parts_mut(out_uv, nc as usize * 2);
    for (i, uv) in res.corner_uv.iter().enumerate() {
        ouv[2 * i] = uv[0];
        ouv[2 * i + 1] = uv[1];
    }
    if !out_face_chart.is_null() {
        slice::from_raw_parts_mut(out_face_chart, nf as usize).copy_from_slice(&res.face_chart);
    }
    if !out_seams.is_null() {
        let os = slice::from_raw_parts_mut(out_seams, out_seams_cap as usize * 2);
        for (i, &(a, b)) in res.seams.iter().take(out_seams_cap as usize).enumerate() {
            os[2 * i] = a;
            os[2 * i + 1] = b;
        }
    }
    if !out_nseams.is_null() {
        *out_nseams = res.seams.len() as u32;
    }
    if !out_stats.is_null() {
        let s = &res.stats;
        *out_stats = EuvStats {
            charts: s.charts as u32,
            flipped: s.flipped as u32,
            seams: s.seams as u32,
            overflow: s.overflow as u32,
            mean_distortion: s.mean_distortion as f32,
            max_distortion: s.max_distortion as f32,
            density_px_per_unit: s.density_px_per_unit as f32,
            coverage: s.coverage as f32,
            time_ms: s.time_ms as f32,
        };
    }
}