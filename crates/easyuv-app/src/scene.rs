//! Objets chargés, sélection et résultats UV dérivés pour l'affichage.

use easyuv::math::{V2, V3};
use easyuv::model::Model;
use easyuv::UvResult;
use std::collections::HashSet;

#[derive(Clone)]
pub struct Scene {
    pub name: String,
    pub model: Model,
    /// Objet actif (dernier cliqué), comme dans Blender.
    pub active: usize,
    pub selected: Vec<bool>,
    pub hidden: Vec<bool>,
    pub face_normals: Vec<V3>,
    /// Objet de chaque face.
    pub face_object: Vec<u32>,
    /// Arêtes uniques : (a, b, objet).
    pub edges: Vec<(u32, u32, u32)>,
    pub radius: f64,
    pub uv: Option<UvResult>,
    pub seam_set: HashSet<(u32, u32)>,
    /// Étirement d'aire par face, normalisé 0 (aucun) .. 1 (x2 ou /2).
    pub face_stretch: Vec<f32>,
    /// Densité mesurée sur les UV actuelles (px / unité).
    pub density: f64,
    /// Densité par objet, mise en cache (mesurée avec density_ts).
    object_densities: Vec<f64>,
    density_ts: u32,
    /// Incrémenté à chaque modification visible (cache du rendu 3D).
    pub generation: u64,
    /// Îlot UV de chaque face (déduit des UV : faces reliées par des UV continues).
    pub face_island: Vec<u32>,
    /// Coins de chaque îlot UV.
    pub island_corners: Vec<Vec<u32>>,
    /// Objet de chaque îlot UV.
    pub island_object: Vec<u32>,
    /// Îlots sélectionnés dans l'éditeur UV.
    pub island_selected: Vec<bool>,
}

/// Une entrée de reconstruction : objet source, décalage éventuel (copie), état.
struct Entry {
    src: usize,
    offset: Option<V3>,
    name: String,
    selected: bool,
    hidden: bool,
}

impl Scene {
    pub fn new(name: String, model: Model) -> Self {
        let n = model.objects.len();
        let mut s = Scene {
            name,
            model,
            active: 0,
            selected: vec![false; n],
            hidden: vec![false; n],
            face_normals: Vec::new(),
            face_object: Vec::new(),
            edges: Vec::new(),
            radius: 1.0,
            uv: None,
            seam_set: HashSet::new(),
            face_stretch: Vec::new(),
            density: 0.0,
            object_densities: Vec::new(),
            density_ts: 1,
            generation: 0,
            face_island: Vec::new(),
            island_corners: Vec::new(),
            island_object: Vec::new(),
            island_selected: Vec::new(),
        };
        s.rebuild_derived();
        s
    }

    fn rebuild_derived(&mut self) {
        let mesh = &self.model.mesh;
        self.face_normals = (0..mesh.face_count()).map(|f| mesh.face_normal(f)).collect();
        let (lo, hi) = mesh.bounds();
        self.radius = (hi - lo).length() * 0.5;
        self.face_object = vec![0; mesh.face_count()];
        let mut seen = HashSet::new();
        self.edges.clear();
        for (oi, o) in self.model.objects.iter().enumerate() {
            for f in o.face_start..o.face_end {
                self.face_object[f] = oi as u32;
                let vs = mesh.face(f);
                for i in 0..vs.len() {
                    let (a, b) = (vs[i], vs[(i + 1) % vs.len()]);
                    let k = if a < b { (a, b) } else { (b, a) };
                    if a != b && seen.insert(k) {
                        self.edges.push((k.0, k.1, oi as u32));
                    }
                }
            }
        }
        self.selected.resize(self.model.objects.len(), false);
        self.hidden.resize(self.model.objects.len(), false);
        self.active = self.active.min(self.model.objects.len().saturating_sub(1));
        self.touch();
    }

    pub fn touch(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn len(&self) -> usize {
        self.model.objects.len()
    }

    // ---------- sélection ----------

    pub fn selection(&self) -> Vec<usize> {
        (0..self.len()).filter(|&i| self.selected[i]).collect()
    }

    pub fn has_selection(&self) -> bool {
        self.selected.iter().any(|&s| s)
    }

    /// Clic simple : sélectionne uniquement `i`. Avec Maj : bascule `i`.
    pub fn click(&mut self, i: Option<usize>, extend: bool) {
        match (i, extend) {
            (Some(i), false) => {
                self.selected.iter_mut().for_each(|s| *s = false);
                self.selected[i] = true;
                self.active = i;
            }
            (Some(i), true) => {
                // comme Blender : Maj+clic sur un objet sélectionné non actif le rend actif
                if self.selected[i] && self.active != i {
                    self.active = i;
                } else {
                    self.selected[i] = !self.selected[i];
                    if self.selected[i] {
                        self.active = i;
                    }
                }
            }
            (None, false) => self.selected.iter_mut().for_each(|s| *s = false),
            (None, true) => {}
        }
        self.touch();
    }

    pub fn select_all(&mut self, on: bool) {
        for i in 0..self.len() {
            self.selected[i] = on && !self.hidden[i];
        }
        self.touch();
    }

    pub fn invert_selection(&mut self) {
        for i in 0..self.len() {
            self.selected[i] = !self.selected[i] && !self.hidden[i];
        }
        self.touch();
    }

    // ---------- visibilité ----------

    pub fn toggle_hidden(&mut self, i: usize) {
        self.hidden[i] = !self.hidden[i];
        if self.hidden[i] {
            self.selected[i] = false;
        }
        self.touch();
    }

    pub fn hide_selected(&mut self) -> usize {
        let sel = self.selection();
        for &i in &sel {
            self.hidden[i] = true;
            self.selected[i] = false;
        }
        self.touch();
        sel.len()
    }

    /// Réaffiche tout et sélectionne ce qui était masqué (comportement Blender Alt+H).
    pub fn reveal_all(&mut self) -> usize {
        let mut n = 0;
        for i in 0..self.len() {
            if self.hidden[i] {
                self.hidden[i] = false;
                self.selected[i] = true;
                n += 1;
            }
        }
        self.touch();
        n
    }

    pub fn rename(&mut self, i: usize, name: &str) {
        let name = name.trim();
        if !name.is_empty() {
            self.model.objects[i].name = name.to_string();
            self.touch();
        }
    }

    /// Boîte englobante des objets sélectionnés visibles (ou de tout si aucune sélection).
    pub fn focus_bounds(&self) -> (V3, V3) {
        let sel: Vec<usize> = self.selection().into_iter().filter(|&i| !self.hidden[i]).collect();
        if sel.is_empty() {
            return self.model.mesh.bounds();
        }
        let mut lo = V3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = V3::new(f64::MIN, f64::MIN, f64::MIN);
        for i in sel {
            let o = &self.model.objects[i];
            for p in &self.model.mesh.positions[o.vert_start..o.vert_end] {
                lo = V3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
                hi = V3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
            }
        }
        (lo, hi)
    }

    // ---------- opérations structurelles ----------

    /// Supprime les objets sélectionnés. Les UV des objets restants sont conservées.
    pub fn delete_selected(&mut self, texture_size: u32) -> usize {
        let plan: Vec<Entry> = (0..self.len())
            .filter(|&i| !self.selected[i])
            .map(|i| Entry { src: i, offset: None, name: self.model.objects[i].name.clone(), selected: false, hidden: self.hidden[i] })
            .collect();
        let removed = self.len() - plan.len();
        if removed > 0 {
            let old_active = self.active;
            let new_active = plan.iter().position(|e| e.src >= old_active).unwrap_or(plan.len().saturating_sub(1));
            self.rebuild(plan, texture_size);
            self.active = new_active.min(self.len().saturating_sub(1));
        }
        removed
    }

    /// Duplique les objets sélectionnés (décalés en X) et sélectionne les copies.
    pub fn duplicate_selected(&mut self, texture_size: u32) -> usize {
        let sel = self.selection();
        if sel.is_empty() {
            return 0;
        }
        let (lo, hi) = self.focus_bounds();
        let dx = (hi.x - lo.x).max(0.1) * 1.15;
        let mut plan: Vec<Entry> = (0..self.len())
            .map(|i| Entry { src: i, offset: None, name: self.model.objects[i].name.clone(), selected: false, hidden: self.hidden[i] })
            .collect();
        for &i in &sel {
            let name = self.unique_name(&self.model.objects[i].name);
            plan.push(Entry { src: i, offset: Some(V3::new(dx, 0.0, 0.0)), name, selected: true, hidden: false });
        }
        let n_before = self.len();
        self.rebuild(plan, texture_size);
        self.active = self.len() - 1;
        let _ = n_before;
        sel.len()
    }

    /// « Cube » -> « Cube.001 », comme Blender.
    fn unique_name(&self, base: &str) -> String {
        let stem = match base.rsplit_once('.') {
            Some((s, n)) if n.len() == 3 && n.chars().all(|c| c.is_ascii_digit()) => s,
            _ => base,
        };
        let names: HashSet<&str> = self.model.objects.iter().map(|o| o.name.as_str()).collect();
        (1..).map(|k| format!("{stem}.{k:03}")).find(|n| !names.contains(n.as_str())).unwrap()
    }

    fn rebuild(&mut self, plan: Vec<Entry>, texture_size: u32) {
        let mut model = Model::new();
        let old_uv = self.uv.as_ref();
        let chart_off = old_uv.map_or(0, |u| u.face_chart.iter().copied().max().map_or(0, |c| c + 1));
        let mut new_uv = old_uv.map(|u| UvResult { corner_uv: Vec::new(), face_chart: Vec::new(), seams: Vec::new(), stats: u.stats.clone() });
        let mut selected = Vec::new();
        let mut hidden = Vec::new();
        for e in &plan {
            let mut m = self.model.object_mesh(e.src);
            if let Some(d) = e.offset {
                for p in m.positions.iter_mut() {
                    *p = *p + d;
                }
            }
            let vs_new = model.mesh.positions.len() as u32;
            model.push(e.name.clone(), &m);
            selected.push(e.selected);
            hidden.push(e.hidden);
            if let (Some(nu), Some(ou)) = (new_uv.as_mut(), old_uv) {
                let o = &self.model.objects[e.src];
                nu.corner_uv.extend_from_slice(&ou.corner_uv[self.model.corner_range(e.src)]);
                let off = if e.offset.is_some() { chart_off } else { 0 };
                nu.face_chart.extend(ou.face_chart[o.face_start..o.face_end].iter().map(|c| c + off));
                let (v0, v1) = (o.vert_start as u32, o.vert_end as u32);
                for &(a, b) in &ou.seams {
                    if a >= v0 && a < v1 && b >= v0 && b < v1 {
                        nu.seams.push((a - v0 + vs_new, b - v0 + vs_new));
                    }
                }
            }
        }
        if let Some(nu) = new_uv.as_mut() {
            let charts: HashSet<u32> = nu.face_chart.iter().copied().collect();
            nu.stats.charts = charts.len();
            nu.stats.seams = nu.seams.len();
        }
        self.model = model;
        self.selected = selected;
        self.hidden = hidden;
        self.rebuild_derived();
        match new_uv {
            Some(u) if !self.model.objects.is_empty() => self.set_uv(u, texture_size),
            _ => {
                self.uv = None;
                self.rebuild_islands();
                self.seam_set.clear();
                self.face_stretch.clear();
                self.density = 0.0;
            }
        }
    }

    /// Objets à déplier : la sélection visible, ou tout s'il n'y a pas de sélection.
    pub fn unwrap_targets(&self) -> Vec<usize> {
        let sel: Vec<usize> = self.selection().into_iter().filter(|&i| !self.hidden[i]).collect();
        if sel.is_empty() {
            (0..self.len()).collect()
        } else {
            sel
        }
    }

    /// Sous-modèle formé des objets `targets` (dans cet ordre).
    pub fn sub_model(&self, targets: &[usize]) -> Model {
        if targets.len() == self.len() && targets.iter().enumerate().all(|(k, &i)| k == i) {
            return self.model.clone();
        }
        let mut m = Model::new();
        for &i in targets {
            m.push(self.model.objects[i].name.clone(), &self.model.object_mesh(i));
        }
        m
    }

    /// Vrai si le résultat `sub` correspond encore aux objets `targets` (scène inchangée).
    pub fn fits_partial(&self, targets: &[usize], sub: &UvResult) -> bool {
        let mut n = 0;
        for &i in targets {
            if i >= self.len() {
                return false;
            }
            n += self.model.corner_range(i).len();
        }
        n == sub.corner_uv.len()
    }

    /// Remplace les UV des seuls objets `targets` par celles de `sub` (calculées sur
    /// `sub_model(targets)`) ; les UV des autres objets sont conservées.
    pub fn merge_partial_uv(&mut self, targets: &[usize], sub: UvResult, texture_size: u32) {
        let nc = self.model.mesh.corners.len();
        let nf = self.model.mesh.face_count();
        let mut base = self.uv.take().unwrap_or_else(|| {
            // objets jamais dépliés : UV nulles, un îlot par objet
            let mut face_chart = vec![0u32; nf];
            for (oi, o) in self.model.objects.iter().enumerate() {
                face_chart[o.face_start..o.face_end].iter_mut().for_each(|c| *c = 1_000_000 + oi as u32);
            }
            UvResult { corner_uv: vec![[0.0; 2]; nc], face_chart, seams: Vec::new(), stats: Default::default() }
        });
        let chart_off = base.face_chart.iter().copied().filter(|&c| c < 1_000_000).max().map_or(0, |c| c + 1);
        let (mut sub_c, mut sub_f, mut sub_v) = (0usize, 0usize, 0u32);
        let mut keep_seams: Vec<(u32, u32)> = base.seams.clone();
        for &i in targets {
            let o = self.model.objects[i].clone();
            let cr = self.model.corner_range(i);
            let n_c = cr.len();
            base.corner_uv[cr.clone()].copy_from_slice(&sub.corner_uv[sub_c..sub_c + n_c]);
            let n_f = o.face_end - o.face_start;
            for k in 0..n_f {
                base.face_chart[o.face_start + k] = sub.face_chart[sub_f + k] + chart_off;
            }
            let (v0, v1) = (o.vert_start as u32, o.vert_end as u32);
            let n_v = v1 - v0;
            keep_seams.retain(|&(a, b)| !(a >= v0 && a < v1 && b >= v0 && b < v1));
            for &(a, b) in &sub.seams {
                if a >= sub_v && a < sub_v + n_v && b >= sub_v && b < sub_v + n_v {
                    keep_seams.push((a - sub_v + v0, b - sub_v + v0));
                }
            }
            sub_c += n_c;
            sub_f += n_f;
            sub_v += n_v;
        }
        base.seams = keep_seams;
        // statistiques du dernier dépliage (objets ciblés)
        base.stats = sub.stats;
        self.set_uv(base, texture_size);
    }

    // ---------- UV ----------

    /// Densité de texels d'un objet (px/m), depuis le cache (proportionnelle à la texture).
    pub fn object_density(&self, i: usize, texture_size: u32) -> f64 {
        self.object_densities.get(i).map_or(0.0, |d| d * texture_size as f64 / self.density_ts.max(1) as f64)
    }

    pub fn set_uv(&mut self, uv: UvResult, texture_size: u32) {
        self.seam_set = uv.seams.iter().map(|&(a, b)| if a < b { (a, b) } else { (b, a) }).collect();
        self.uv = Some(uv);
        self.rebuild_islands();
        self.refresh(texture_size);
    }

    /// Recalcule les îlots UV (après un dépliage, un rangement ou un changement d'objets).
    fn rebuild_islands(&mut self) {
        self.face_island.clear();
        self.island_corners.clear();
        self.island_object.clear();
        self.island_selected.clear();
        let Some(uv) = &self.uv else { return };
        let m = &self.model.mesh;
        let (fi, n) = easyuv::layout::uv_islands(m, &uv.corner_uv);
        self.island_corners = vec![Vec::new(); n];
        self.island_object = vec![0; n];
        for f in 0..m.face_count() {
            let i = fi[f] as usize;
            self.island_corners[i].extend(m.face_range(f).map(|c| c as u32));
            self.island_object[i] = self.face_object[f];
        }
        self.face_island = fi;
        self.island_selected = vec![false; n];
    }

    // ---------- édition UV (îlots) ----------

    /// Îlot visible sous le point `p` (coordonnées UV), le dernier dessiné l'emportant.
    pub fn pick_island(&self, p: [f32; 2]) -> Option<usize> {
        let uv = self.uv.as_ref()?;
        let m = &self.model.mesh;
        let mut hit = None;
        for f in 0..m.face_count() {
            if self.hidden[self.face_object[f] as usize] {
                continue;
            }
            let r = m.face_range(f);
            for t in m.fan(f) {
                let [a, b, c] = t.map(|k| uv.corner_uv[r.start + k]);
                let e = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
                let (e0, e1, e2) = (e(a, b), e(b, c), e(c, a));
                // triangle UV dégénéré (objet jamais déplié) : ne capte pas le clic
                if ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() < 1e-12 {
                    continue;
                }
                if (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0) || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0) {
                    hit = Some(self.face_island[f] as usize);
                }
            }
        }
        hit
    }

    pub fn has_island_selection(&self) -> bool {
        self.island_selected.iter().any(|&s| s)
    }

    pub fn island_visible(&self, i: usize) -> bool {
        !self.hidden[self.island_object[i] as usize]
    }

    /// Clic dans l'éditeur UV : sélectionne l'îlot `i` seul, ou le bascule avec Maj.
    pub fn click_island(&mut self, i: Option<usize>, extend: bool) {
        match (i, extend) {
            (Some(i), false) => {
                self.island_selected.iter_mut().for_each(|s| *s = false);
                self.island_selected[i] = true;
            }
            (Some(i), true) => self.island_selected[i] = !self.island_selected[i],
            (None, false) => self.island_selected.iter_mut().for_each(|s| *s = false),
            (None, true) => {}
        }
        self.touch();
    }

    pub fn select_all_islands(&mut self, on: bool) {
        for i in 0..self.island_selected.len() {
            self.island_selected[i] = on && self.island_visible(i);
        }
        self.touch();
    }

    pub fn invert_islands(&mut self) {
        for i in 0..self.island_selected.len() {
            self.island_selected[i] = !self.island_selected[i] && self.island_visible(i);
        }
        self.touch();
    }

    /// Sélection au rectangle : îlots dont un centre de face est dans [lo, hi].
    /// `mode` : 1 = ajouter, -1 = retirer, 0 = remplacer.
    pub fn box_select_islands(&mut self, lo: [f32; 2], hi: [f32; 2], mode: i32) {
        let Some(uv) = &self.uv else { return };
        let m = &self.model.mesh;
        let mut inside = vec![false; self.island_selected.len()];
        for f in 0..m.face_count() {
            if self.hidden[self.face_object[f] as usize] {
                continue;
            }
            let r = m.face_range(f);
            let n = r.len() as f32;
            let c = r.fold([0.0f32; 2], |a, ci| [a[0] + uv.corner_uv[ci][0] / n, a[1] + uv.corner_uv[ci][1] / n]);
            if c[0] >= lo[0] && c[0] <= hi[0] && c[1] >= lo[1] && c[1] <= hi[1] {
                inside[self.face_island[f] as usize] = true;
            }
        }
        for (s, hit) in self.island_selected.iter_mut().zip(inside) {
            *s = match mode {
                1 => *s || hit,
                -1 => *s && !hit,
                _ => hit,
            };
        }
        self.touch();
    }

    /// Îlots sélectionnés et visibles.
    pub fn selected_island_ids(&self) -> Vec<usize> {
        (0..self.island_selected.len()).filter(|&i| self.island_selected[i] && self.island_visible(i)).collect()
    }

    /// Îlots touchés par un pinceau : la sélection s'il y en a une, sinon tout ce qui est visible.
    pub fn brush_islands(&self) -> Vec<usize> {
        let sel = self.selected_island_ids();
        if !sel.is_empty() {
            return sel;
        }
        (0..self.island_selected.len()).filter(|&i| self.island_visible(i)).collect()
    }

    /// Outil d'îlots (aligner, répartir, empiler…) appliqué aux îlots sélectionnés.
    /// Renvoie le nombre d'îlots traités.
    pub fn island_tool(&mut self, texture_size: u32, op: impl FnOnce(&easyuv::mesh::Mesh, &mut [[f32; 2]], &[u32], &[usize])) -> usize {
        let ids = self.selected_island_ids();
        let Some(uv) = &mut self.uv else { return 0 };
        if ids.is_empty() {
            return 0;
        }
        op(&self.model.mesh, &mut uv.corner_uv, &self.face_island, &ids);
        self.refresh(texture_size);
        ids.len()
    }

    /// Coins des îlots sélectionnés.
    pub fn selected_corners(&self) -> Vec<u32> {
        (0..self.island_selected.len()).filter(|&i| self.island_selected[i]).flat_map(|i| self.island_corners[i].iter().copied()).collect()
    }

    /// Boîte UV (min, max) d'un ensemble de coins.
    pub fn corners_bounds(&self, corners: &[u32]) -> Option<([f32; 2], [f32; 2])> {
        let uv = self.uv.as_ref()?;
        if corners.is_empty() {
            return None;
        }
        let mut lo = [f32::MAX; 2];
        let mut hi = [f32::MIN; 2];
        for &c in corners {
            let p = uv.corner_uv[c as usize];
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        Some((lo, hi))
    }

    /// Applique `f` aux UV d'origine `orig` des coins `corners` (transformation interactive).
    pub fn transform_corners(&mut self, orig: &[[f32; 2]], corners: &[u32], f: impl Fn([f32; 2]) -> [f32; 2]) {
        let Some(uv) = &mut self.uv else { return };
        for &c in corners {
            uv.corner_uv[c as usize] = f(orig[c as usize]);
        }
        self.touch();
    }

    /// Transformation de chaque îlot sélectionné autour de son propre centre (miroir,
    /// quart de tour) : `f(p - centre)`.
    pub fn per_island(&mut self, f: impl Fn([f32; 2]) -> [f32; 2], texture_size: u32) -> usize {
        let sel: Vec<usize> = (0..self.island_selected.len()).filter(|&i| self.island_selected[i]).collect();
        for &i in &sel {
            let corners = self.island_corners[i].clone();
            let Some((lo, hi)) = self.corners_bounds(&corners) else { continue };
            let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
            let uv = self.uv.as_mut().unwrap();
            for &k in &corners {
                let p = uv.corner_uv[k as usize];
                let q = f([p[0] - c[0], p[1] - c[1]]);
                uv.corner_uv[k as usize] = [q[0] + c[0], q[1] + c[1]];
            }
        }
        if !sel.is_empty() {
            self.refresh(texture_size);
        }
        sel.len()
    }

    /// Redresse les îlots sélectionnés : boîte englobante minimale alignée sur les axes,
    /// côté long horizontal, centre conservé.
    pub fn straighten_islands(&mut self, texture_size: u32) -> usize {
        let sel: Vec<usize> = (0..self.island_selected.len()).filter(|&i| self.island_selected[i]).collect();
        for &i in &sel {
            let corners = self.island_corners[i].clone();
            let Some((lo, hi)) = self.corners_bounds(&corners) else { continue };
            let uv = self.uv.as_mut().unwrap();
            let mut pts: Vec<V2> = corners.iter().map(|&k| V2::new(uv.corner_uv[k as usize][0] as f64, uv.corner_uv[k as usize][1] as f64)).collect();
            let (w, h) = easyuv::pack::orient_min_box(&mut pts);
            let off = [(lo[0] + hi[0]) as f64 * 0.5 - w * 0.5, (lo[1] + hi[1]) as f64 * 0.5 - h * 0.5];
            for (k, p) in corners.iter().zip(pts) {
                uv.corner_uv[*k as usize] = [(p.x + off[0]) as f32, (p.y + off[1]) as f32];
            }
        }
        if !sel.is_empty() {
            self.refresh(texture_size);
        }
        sel.len()
    }

    /// Rend droites les bandes / grilles de quads sélectionnées. Retourne (redressés, ignorés).
    pub fn straighten_strips(&mut self, texture_size: u32) -> (usize, usize) {
        let Some(uv) = &mut self.uv else { return (0, 0) };
        let r = easyuv::layout::straighten_grid(&self.model.mesh, &mut uv.corner_uv, &self.face_island, &self.island_selected);
        if r.0 > 0 {
            self.refresh(texture_size);
        }
        r
    }

    // ---------- objets : orientation ----------

    /// Quart de tour des objets sélectionnés (tous si aucune sélection) : redresse un import
    /// couché. Les UV ne changent pas.
    pub fn rotate_objects_quarter(&mut self, axis: usize, positive: bool) -> usize {
        let targets = self.unwrap_targets();
        for &i in &targets {
            self.model.rotate_object_quarter(i, axis, positive);
        }
        let mesh = &self.model.mesh;
        self.face_normals = (0..mesh.face_count()).map(|f| mesh.face_normal(f)).collect();
        let (lo, hi) = mesh.bounds();
        self.radius = (hi - lo).length() * 0.5;
        self.touch();
        targets.len()
    }

    /// Recalcule étirement et densité après modification des UV.
    pub fn refresh(&mut self, texture_size: u32) {
        self.touch();
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
        self.density_ts = texture_size;
        self.object_densities = (0..self.model.objects.len())
            .map(|i| easyuv::measure_density(&self.model.object_mesh(i), &uv.corner_uv[self.model.corner_range(i)], texture_size))
            .collect();
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

#[cfg(test)]
mod tests {
    use super::*;
    use easyuv::primitives;

    fn scene() -> Scene {
        let mut m = Model::single("Cube", primitives::cube(1.0));
        m.push("Tore", &primitives::torus(1.0, 0.3, 16, 8));
        m.push("Sphère", &primitives::uv_sphere(1.0, 12, 6));
        let mut s = Scene::new("test".into(), m);
        let uv = easyuv::model::unwrap_model(&s.model, &easyuv::Params::default(), true);
        s.set_uv(uv, 2048);
        s
    }

    #[test]
    fn delete_keeps_other_uvs() {
        let mut s = scene();
        let tore_uv: Vec<[f32; 2]> = s.uv.as_ref().unwrap().corner_uv[s.model.corner_range(1)].to_vec();
        s.click(Some(0), false);
        assert_eq!(s.delete_selected(2048), 1);
        assert_eq!(s.len(), 2);
        assert_eq!(s.model.objects[0].name, "Tore");
        assert_eq!(&s.uv.as_ref().unwrap().corner_uv[s.model.corner_range(0)], tore_uv.as_slice());
        assert_eq!(s.face_object.len(), s.model.mesh.face_count());
        // les coutures restent valides (indices de sommets dans le maillage)
        let nv = s.model.mesh.positions.len() as u32;
        assert!(s.uv.as_ref().unwrap().seams.iter().all(|&(a, b)| a < nv && b < nv));
    }

    #[test]
    fn delete_all_clears_uv() {
        let mut s = scene();
        s.select_all(true);
        assert_eq!(s.delete_selected(2048), 3);
        assert_eq!(s.len(), 0);
        assert!(s.uv.is_none());
    }

    #[test]
    fn duplicate_names_and_selects_copy() {
        let mut s = scene();
        s.click(Some(1), false);
        assert_eq!(s.duplicate_selected(2048), 1);
        assert_eq!(s.len(), 4);
        assert_eq!(s.model.objects[3].name, "Tore.001");
        assert!(s.selected[3] && !s.selected[1]);
        assert_eq!(s.active, 3);
        assert_eq!(s.uv.as_ref().unwrap().corner_uv.len(), s.model.mesh.corners.len());
    }

    #[test]
    fn hide_and_reveal() {
        let mut s = scene();
        s.click(Some(2), false);
        assert_eq!(s.hide_selected(), 1);
        assert!(s.hidden[2] && !s.selected[2]);
        s.select_all(true);
        assert!(!s.selected[2], "un objet masqué n'est pas sélectionnable");
        assert_eq!(s.reveal_all(), 1);
        assert!(!s.hidden[2] && s.selected[2]);
    }
    #[test]
    fn partial_unwrap_keeps_other_objects() {
        let mut s = scene();
        let before: Vec<[f32; 2]> = s.uv.as_ref().unwrap().corner_uv[s.model.corner_range(0)].to_vec();
        s.click(Some(2), false);
        let targets = s.unwrap_targets();
        assert_eq!(targets, vec![2]);
        let sub = easyuv::model::unwrap_model(&s.sub_model(&targets), &easyuv::Params::default(), true);
        assert!(s.fits_partial(&targets, &sub));
        s.merge_partial_uv(&targets, sub, 2048);
        let uv = s.uv.as_ref().unwrap();
        assert_eq!(&uv.corner_uv[s.model.corner_range(0)], before.as_slice(), "le cube ne doit pas bouger");
        // la sphère remplit maintenant seule la tuile
        let max = uv.corner_uv[s.model.corner_range(2)].iter().fold(0.0f32, |a, c| a.max(c[0]).max(c[1]));
        assert!(max > 0.9);
        let nv = s.model.mesh.positions.len() as u32;
        assert!(uv.seams.iter().all(|&(a, b)| a < nv && b < nv));
    }

    #[test]
    fn islands_pick_select_transform() {
        let mut s = scene();
        let n = s.island_selected.len();
        assert!(n >= 6, "{n} îlots");
        // le centre d'un triangle d'une face est sur son îlot
        let f = 0;
        let uv = s.uv.as_ref().unwrap();
        let r = s.model.mesh.face_range(f);
        let t: Vec<[f32; 2]> = r.clone().take(3).map(|c| uv.corner_uv[c]).collect();
        let p = [(t[0][0] + t[1][0] + t[2][0]) / 3.0, (t[0][1] + t[1][1] + t[2][1]) / 3.0];
        let hit = s.pick_island(p).unwrap();
        assert_eq!(hit, s.face_island[f] as usize);
        s.click_island(Some(hit), false);
        assert_eq!(s.island_selected.iter().filter(|&&x| x).count(), 1);
        // déplacement : seuls les coins de l'îlot bougent
        let orig = s.uv.as_ref().unwrap().corner_uv.clone();
        let corners = s.selected_corners();
        s.transform_corners(&orig, &corners, |q| [q[0] + 2.0, q[1]]);
        let now = &s.uv.as_ref().unwrap().corner_uv;
        let moved = (0..now.len()).filter(|&c| now[c] != orig[c]).count();
        assert_eq!(moved, corners.len());
        assert!(s.pick_island(p).is_none_or(|i| i != hit));
        // miroir autour du centre de l'îlot : la boîte ne change pas
        let b0 = s.corners_bounds(&corners).unwrap();
        s.per_island(|p| [-p[0], p[1]], 2048);
        let b1 = s.corners_bounds(&corners).unwrap();
        assert!((b0.0[0] - b1.0[0]).abs() < 1e-5 && (b0.1[1] - b1.1[1]).abs() < 1e-5);
        s.select_all_islands(true);
        assert!(s.island_selected.iter().all(|&x| x));
    }

    #[test]
    fn rotate_object_keeps_it_on_the_ground() {
        let mut s = scene();
        s.click(Some(1), false);
        let o = s.model.objects[1].clone();
        let zmin = |s: &Scene| s.model.mesh.positions[o.vert_start..o.vert_end].iter().map(|p| p.z).fold(f64::MAX, f64::min);
        let before = zmin(&s);
        assert_eq!(s.rotate_objects_quarter(0, true), 1);
        assert!((zmin(&s) - before).abs() < 1e-9);
        // le tore (plat sur Z) est maintenant debout : sa hauteur devient son diamètre
        let h = s.model.mesh.positions[o.vert_start..o.vert_end].iter().map(|p| p.z).fold(f64::MIN, f64::max) - zmin(&s);
        assert!(h > 2.0, "hauteur {h}");
    }

    #[test]
    fn no_selection_unwraps_everything() {
        let s = scene();
        assert_eq!(s.unwrap_targets(), vec![0, 1, 2]);
    }
}