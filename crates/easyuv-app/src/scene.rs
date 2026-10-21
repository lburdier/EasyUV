//! Objet chargé + résultats UV dérivés pour l'affichage.

use easyuv::math::{V2, V3};
use easyuv::model::Model;
use easyuv::UvResult;
use std::collections::HashSet;

pub struct Scene {
    pub name: String,
    pub model: Model,
    /// Objet sélectionné dans l'outliner.
    pub active: usize,
    pub face_normals: Vec<V3>,
    pub radius: f64,
    pub uv: Option<UvResult>,
    pub seam_set: HashSet<(u32, u32)>,
    /// Étirement d'aire par face, normalisé 0 (aucun) .. 1 (x2 ou /2).
    pub face_stretch: Vec<f32>,
    /// Densité mesurée sur les UV actuelles (px / unité).
    pub density: f64,
}

impl Scene {
    pub fn new(name: String, model: Model) -> Self {
        let mesh = &model.mesh;
        let face_normals = (0..mesh.face_count()).map(|f| mesh.face_normal(f)).collect();
        let (lo, hi) = mesh.bounds();
        let radius = (hi - lo).length() * 0.5;
        Scene { name, face_normals, radius, model, active: 0, uv: None, seam_set: HashSet::new(), face_stretch: Vec::new(), density: 0.0 }
    }

    /// Densité de texels d'un objet (px/m).
    pub fn object_density(&self, i: usize, texture_size: u32) -> f64 {
        let Some(uv) = &self.uv else { return 0.0 };
        easyuv::measure_density(&self.model.object_mesh(i), &uv.corner_uv[self.model.corner_range(i)], texture_size)
    }

    pub fn set_uv(&mut self, uv: UvResult, texture_size: u32) {
        self.seam_set = uv.seams.iter().copied().collect();
        self.uv = Some(uv);
        self.refresh(texture_size);
    }

    /// Recalcule étirement et densité après modification des UV.
    pub fn refresh(&mut self, texture_size: u32) {
        let Some(uv) = &self.uv else { return };
        let m = &self.model.mesh;
        let mut ratios = Vec::with_capacity(m.face_count());
        let (mut sa3, mut sau) = (0.0, 0.0);
        for f in 0..m.face_count() {
            let r = m.face_range(f);
            let (mut a3, mut au) = (0.0, 0.0);
            for t in m.fan(f) {
                let p = |k: usize| m.positions[m.corners[r.start + t[k]] as usize];
                let u = |k: usize| {
                    let c = uv.corner_uv[r.start + t[k]];
                    V2::new(c[0] as f64, c[1] as f64)
                };
                a3 += 0.5 * (p(1) - p(0)).cross(p(2) - p(0)).length();
                au += 0.5 * (u(1) - u(0)).cross(u(2) - u(0)).abs();
            }
            sa3 += a3;
            sau += au;
            ratios.push(if a3 > 0.0 { au / a3 } else { 0.0 });
        }
        let global = if sa3 > 0.0 { sau / sa3 } else { 1.0 };
        self.face_stretch = ratios
            .iter()
            .map(|&r| if r <= 0.0 { 1.0 } else { ((r / global).ln().abs() / std::f64::consts::LN_2) as f32 })
            .collect();
        self.density = easyuv::measure_density(m, &uv.corner_uv, texture_size);
    }

    /// Remet toutes les UV à l'échelle pour atteindre la densité cible (ancrage en 0,0).
    pub fn apply_density(&mut self, target: f64, texture_size: u32) -> bool {
        if self.uv.is_none() || self.density <= 0.0 {
            return false;
        }
        let k = (target / self.density) as f32;
        let uv = self.uv.as_mut().unwrap();
        let mut lo = [f32::MAX; 2];
        for c in &uv.corner_uv {
            lo = [lo[0].min(c[0]), lo[1].min(c[1])];
        }
        for c in uv.corner_uv.iter_mut() {
            c[0] = (c[0] - lo[0]) * k + lo[0];
            c[1] = (c[1] - lo[1]) * k + lo[1];
        }
        self.refresh(texture_size);
        true
    }

    pub fn uv_overflow(&self) -> bool {
        self.uv.as_ref().is_some_and(|u| u.corner_uv.iter().any(|c| c[0] < -1e-4 || c[1] < -1e-4 || c[0] > 1.0001 || c[1] > 1.0001))
    }
}
