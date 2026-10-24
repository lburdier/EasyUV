//! Objets chargés, sélection et résultats UV dérivés pour l'affichage.

use easyuv::math::{V2, V3};
use easyuv::model::Model;
use easyuv::UvResult;
use std::collections::HashSet;

/// Mode de sélection de l'éditeur UV, comme Blender (1 / 2 / 3 / 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SelectMode {
    Vertex,
    Edge,
    Face,
    #[default]
    Island,
}

impl SelectMode {
    pub const ALL: [SelectMode; 4] = [SelectMode::Vertex, SelectMode::Edge, SelectMode::Face, SelectMode::Island];

    pub fn label(self) -> &'static str {
        match self {
            SelectMode::Vertex => "Sommets",
            SelectMode::Edge => "Arêtes",
            SelectMode::Face => "Faces",
            SelectMode::Island => "Îlots",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SelectMode::Vertex => "1",
            SelectMode::Edge => "2",
            SelectMode::Face => "3",
            SelectMode::Island => "4",
        }
    }
}

#[derive(Clone)]
pub struct Scene {
    pub name: String,
    pub model: Model,
    /// Objet actif (dernier cliqué), comme dans Blender.
    pub active: usize,
    pub selected: Vec<bool>,
    pub hidden: Vec<bool>,
    /// Objets en mode Édition (vide : mode Objet). Comme dans Blender, seuls ceux-là laissent
    /// choisir leurs sommets, arêtes et faces.
    pub editing: Vec<bool>,
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
    /// Mode de sélection de l'éditeur UV.
    pub sel_mode: SelectMode,
    /// Coins UV sélectionnés (modes sommets, arêtes et faces).
    pub corner_sel: Vec<bool>,
    /// Coutures marquées à la main (paires de sommets, a < b), imposées aux dépliages.
    pub marked_seams: HashSet<(u32, u32)>,
    /// Îlot survolé dans l'éditeur UV : mis en évidence sur le modèle 3D.
    pub hover_island: Option<usize>,
    /// Zone isolée de l'éditeur UV (H) : coins libres, un booléen par coin (vide : aucune).
    /// Les pinceaux et G / R / S ne touchent qu'elle, le reste est figé.
    pub isolated: Vec<bool>,
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
            editing: Vec::new(),
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
            sel_mode: SelectMode::Island,
            corner_sel: Vec::new(),
            marked_seams: HashSet::new(),
            hover_island: None,
            isolated: Vec::new(),
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
        // la sélection de sommets / arêtes / faces existe aussi avant tout dépliage (vue 3D)
        self.corner_sel = vec![false; mesh.corners.len()];
        // les coins ont changé (objets supprimés, dupliqués…) : la zone isolée n'a plus de sens
        self.isolated.clear();
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

    // ---------- modes Objet / Édition ----------

    pub fn is_editing(&self) -> bool {
        !self.editing.is_empty()
    }

    /// L'objet `o` est en mode Édition.
    pub fn in_edit(&self, o: usize) -> bool {
        self.editing.get(o).copied().unwrap_or(false)
    }

    /// On peut choisir les éléments de l'objet `o` : visible et, en Édition, édité.
    pub fn object_editable(&self, o: usize) -> bool {
        !self.hidden[o] && (self.editing.is_empty() || self.in_edit(o))
    }

    /// Tab : passe en Édition les objets sélectionnés visibles. Renvoie leur nombre (0 : rien de
    /// sélectionné, on reste en mode Objet). La sélection d'éléments des autres objets est retirée.
    pub fn enter_edit(&mut self) -> usize {
        let ed: Vec<bool> = (0..self.len()).map(|i| self.selected[i] && !self.hidden[i]).collect();
        let n = ed.iter().filter(|&&e| e).count();
        if n == 0 {
            return 0;
        }
        let m = &self.model.mesh;
        for f in 0..m.face_count() {
            if !ed[self.face_object[f] as usize] {
                m.face_range(f).for_each(|c| self.corner_sel[c] = false);
            }
        }
        for i in 0..self.island_selected.len() {
            self.island_selected[i] &= ed[self.island_object[i] as usize];
        }
        self.editing = ed;
        self.touch();
        n
    }

    /// Retour au mode Objet (la sélection d'éléments est gardée pour l'éditeur UV).
    pub fn exit_edit(&mut self) {
        self.editing.clear();
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
        let mut marked = HashSet::new();
        for e in &plan {
            let mut m = self.model.object_mesh(e.src);
            if let Some(d) = e.offset {
                for p in m.positions.iter_mut() {
                    *p = *p + d;
                }
            }
            let vs_new = model.mesh.positions.len() as u32;
            {
                let o = &self.model.objects[e.src];
                let (v0, v1) = (o.vert_start as u32, o.vert_end as u32);
                for &(a, b) in &self.marked_seams {
                    if a >= v0 && a < v1 && b >= v0 && b < v1 {
                        marked.insert((a - v0 + vs_new, b - v0 + vs_new));
                    }
                }
            }
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
        self.editing.clear();
        self.marked_seams = marked;
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
        self.corner_sel = vec![false; self.model.mesh.corners.len()];
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
        self.corner_sel = vec![false; m.corners.len()];
        self.hover_island = None;
    }

    // ---------- édition UV (îlots) ----------

    /// Îlot visible sous le point `p` (coordonnées UV), le dernier dessiné l'emportant.
    pub fn pick_island(&self, p: [f32; 2]) -> Option<usize> {
        let uv = self.uv.as_ref()?;
        let m = &self.model.mesh;
        let mut hit = None;
        for f in 0..m.face_count() {
            if !self.object_editable(self.face_object[f] as usize) {
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

    /// Quelque chose est sélectionné dans l'éditeur UV (îlots, ou sommets / arêtes / faces).
    pub fn has_island_selection(&self) -> bool {
        match self.sel_mode {
            SelectMode::Island => self.island_selected.iter().any(|&s| s),
            _ => self.corner_sel.iter().any(|&s| s),
        }
    }

    pub fn island_visible(&self, i: usize) -> bool {
        self.object_editable(self.island_object[i] as usize)
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

    /// Îlots sélectionnés et visibles ; hors mode îlots, ceux qui contiennent la sélection.
    pub fn selected_island_ids(&self) -> Vec<usize> {
        if self.sel_mode == SelectMode::Island {
            return (0..self.island_selected.len()).filter(|&i| self.island_selected[i] && self.island_visible(i)).collect();
        }
        let mut hit = vec![false; self.island_selected.len()];
        for f in 0..self.model.mesh.face_count() {
            if self.model.mesh.face_range(f).any(|c| self.corner_sel.get(c).copied().unwrap_or(false)) {
                if let Some(&i) = self.face_island.get(f) {
                    hit[i as usize] = true;
                }
            }
        }
        (0..hit.len()).filter(|&i| hit[i] && self.island_visible(i)).collect()
    }

    /// Sélectionne l'îlot `i` seul, quel que soit le mode.
    pub fn select_island_only(&mut self, i: usize) {
        if self.sel_mode == SelectMode::Island {
            self.click_island(Some(i), false);
            return;
        }
        self.corner_sel.iter_mut().for_each(|s| *s = false);
        for &c in &self.island_corners[i] {
            self.corner_sel[c as usize] = true;
        }
        self.touch();
    }

    /// Îlots touchés par un pinceau : la sélection s'il y en a une, sinon tout ce qui est visible.
    pub fn brush_islands(&self) -> Vec<usize> {
        let sel = self.selected_island_ids();
        if !sel.is_empty() {
            return sel;
        }
        (0..self.island_selected.len()).filter(|&i| self.island_visible(i)).collect()
    }

    /// Coins libres d'après la sélection : îlots entiers, faces sélectionnées ou sommets
    /// choisis. Un sommet UV partagé avec une face hors sélection reste figé, pour que
    /// les faces voisines ne soient pas déformées.
    fn selection_free(&self) -> Vec<bool> {
        let m = &self.model.mesh;
        let nc = m.corners.len();
        let zone: Vec<bool> = match self.sel_mode {
            SelectMode::Vertex | SelectMode::Edge => {
                let mut free = self.corner_sel.clone();
                free.resize(nc, false);
                for f in 0..m.face_count() {
                    if !self.face_visible(f) {
                        m.face_range(f).for_each(|c| free[c] = false);
                    }
                }
                return free;
            }
            SelectMode::Island => (0..m.face_count()).map(|f| self.face_island.get(f).is_some_and(|&i| self.island_selected.get(i as usize).copied().unwrap_or(false))).collect(),
            SelectMode::Face => (0..m.face_count()).map(|f| self.face_selected(f)).collect(),
        };
        let Some(uv) = &self.uv else { return vec![false; nc] };
        // sommet UV (sommet, position, îlot) libre si toutes ses faces sont dans la zone
        let key = |f: usize, c: usize| (m.corners[c], uv.corner_uv[c][0].to_bits(), uv.corner_uv[c][1].to_bits(), self.face_island.get(f).copied().unwrap_or(u32::MAX));
        let mut all_in: std::collections::HashMap<(u32, u32, u32, u32), bool> = std::collections::HashMap::new();
        for f in 0..m.face_count() {
            let z = zone[f] && self.face_visible(f);
            for c in m.face_range(f) {
                *all_in.entry(key(f, c)).or_insert(true) &= z;
            }
        }
        let mut free = vec![false; nc];
        for f in 0..m.face_count() {
            for c in m.face_range(f) {
                free[c] = all_in[&key(f, c)];
            }
        }
        free
    }

    pub fn is_isolated(&self) -> bool {
        !self.isolated.is_empty()
    }

    /// Isole la sélection (H) : seuls ses coins pourront bouger. Renvoie le nombre de faces
    /// entièrement libres, ou None s'il n'y a rien de sélectionné.
    pub fn isolate_selection(&mut self) -> Option<usize> {
        if self.uv.is_none() || !self.has_island_selection() {
            return None;
        }
        let free = self.selection_free();
        if !free.iter().any(|&f| f) {
            return None;
        }
        let m = &self.model.mesh;
        let n = (0..m.face_count()).filter(|&f| m.face_range(f).all(|c| free[c])).count();
        self.isolated = free;
        self.touch();
        Some(n)
    }

    /// Libère la zone isolée (Alt+H). Renvoie vrai s'il y en avait une.
    pub fn clear_isolation(&mut self) -> bool {
        let had = self.is_isolated();
        self.isolated.clear();
        self.touch();
        had
    }

    /// Coins que les pinceaux peuvent déplacer : la zone isolée, sinon la sélection.
    /// None : rien n'est restreint, tout ce qui est visible peut bouger.
    pub fn brush_free(&self) -> Option<Vec<bool>> {
        if self.is_isolated() {
            return Some(self.isolated.clone());
        }
        self.has_island_selection().then(|| self.selection_free())
    }

    /// Zone d'un coup de pinceau : îlots concernés et coins libres (None : tout est libre).
    pub fn brush_zone(&self) -> (Vec<usize>, Option<Vec<bool>>) {
        let Some(free) = self.brush_free() else { return (self.brush_islands(), None) };
        let mut hit = vec![false; self.island_selected.len()];
        for f in 0..self.model.mesh.face_count() {
            if self.model.mesh.face_range(f).any(|c| free[c]) {
                if let Some(&i) = self.face_island.get(f) {
                    hit[i as usize] = true;
                }
            }
        }
        ((0..hit.len()).filter(|&i| hit[i] && self.island_visible(i)).collect(), Some(free))
    }

    /// Coins déplacés par G / R / S : la sélection, limitée à la zone isolée s'il y en a une.
    pub fn editable_corners(&self) -> Vec<u32> {
        let corners = self.selected_corners();
        if !self.is_isolated() {
            return corners;
        }
        corners.into_iter().filter(|&c| self.isolated[c as usize]).collect()
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

    /// Coins sélectionnés : ceux des îlots sélectionnés, ou les sommets / arêtes / faces.
    pub fn selected_corners(&self) -> Vec<u32> {
        if self.sel_mode != SelectMode::Island {
            return (0..self.corner_sel.len()).filter(|&c| self.corner_sel[c]).map(|c| c as u32).collect();
        }
        (0..self.island_selected.len()).filter(|&i| self.island_selected[i]).flat_map(|i| self.island_corners[i].iter().copied()).collect()
    }

    // ---------- sélection de sommets, d'arêtes et de faces (comme Blender) ----------

    fn face_visible(&self, f: usize) -> bool {
        self.object_editable(self.face_object[f] as usize)
    }

    /// Change de mode en convertissant la sélection (îlots <-> coins).
    pub fn set_select_mode(&mut self, mode: SelectMode) {
        if mode == self.sel_mode {
            return;
        }
        let ids = self.selected_island_ids();
        let corners = self.selected_corners();
        let was_island = self.sel_mode == SelectMode::Island;
        self.sel_mode = mode;
        if mode == SelectMode::Island {
            self.island_selected.iter_mut().for_each(|s| *s = false);
            for i in ids {
                self.island_selected[i] = true;
            }
        } else if was_island {
            self.corner_sel.iter_mut().for_each(|s| *s = false);
            for c in corners {
                self.corner_sel[c as usize] = true;
            }
        }
        self.touch();
    }

    /// Coins au même endroit que `c` : même sommet, même îlot, mêmes UV (sélection collante,
    /// comme « Shared Location » de Blender : l'autre côté d'une couture n'est pas pris).
    fn sticky(&self, c: usize) -> Vec<usize> {
        let Some(uv) = &self.uv else { return vec![c] };
        let m = &self.model.mesh;
        let (v, p) = (m.corners[c], uv.corner_uv[c]);
        let isl = self.face_island_of_corner(c);
        (0..m.corners.len()).filter(|&k| m.corners[k] == v && uv.corner_uv[k] == p && self.face_island_of_corner(k) == isl).collect()
    }

    fn face_island_of_corner(&self, c: usize) -> u32 {
        // face_start est croissant : recherche dichotomique de la face du coin
        let fs = &self.model.mesh.face_start;
        let f = fs.partition_point(|&s| s <= c).saturating_sub(1);
        self.face_island.get(f).copied().unwrap_or(u32::MAX)
    }

    /// Coin UV visible le plus proche de `p` (à moins de `tol`, en unités UV).
    pub fn pick_corner(&self, p: [f32; 2], tol: f32) -> Option<usize> {
        let uv = self.uv.as_ref()?;
        let m = &self.model.mesh;
        let mut best = (tol * tol, None);
        for f in (0..m.face_count()).filter(|&f| self.face_visible(f)) {
            for c in m.face_range(f) {
                let q = uv.corner_uv[c];
                let d = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
                if d < best.0 {
                    best = (d, Some(c));
                }
            }
        }
        best.1
    }

    /// Arête UV visible la plus proche de `p` : (coin, coin suivant de la face).
    pub fn pick_edge(&self, p: [f32; 2], tol: f32) -> Option<(usize, usize)> {
        let uv = self.uv.as_ref()?;
        let m = &self.model.mesh;
        let mut best = (tol, None);
        for f in (0..m.face_count()).filter(|&f| self.face_visible(f)) {
            let r: Vec<usize> = m.face_range(f).collect();
            for i in 0..r.len() {
                let (a, b) = (r[i], r[(i + 1) % r.len()]);
                let (pa, pb) = (uv.corner_uv[a], uv.corner_uv[b]);
                let d = [pb[0] - pa[0], pb[1] - pa[1]];
                let l2 = d[0] * d[0] + d[1] * d[1];
                let t = if l2 > 0.0 { (((p[0] - pa[0]) * d[0] + (p[1] - pa[1]) * d[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let dist = ((pa[0] + t * d[0] - p[0]).powi(2) + (pa[1] + t * d[1] - p[1]).powi(2)).sqrt();
                if dist < best.0 {
                    best = (dist, Some((a, b)));
                }
            }
        }
        best.1
    }

    /// Face visible sous `p` (la dernière dessinée l'emporte).
    pub fn pick_face(&self, p: [f32; 2]) -> Option<usize> {
        let uv = self.uv.as_ref()?;
        let m = &self.model.mesh;
        let mut hit = None;
        for f in (0..m.face_count()).filter(|&f| self.face_visible(f)) {
            let r = m.face_range(f);
            for t in m.fan(f) {
                let [a, b, c] = t.map(|k| uv.corner_uv[r.start + k]);
                let e = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
                let (e0, e1, e2) = (e(a, b), e(b, c), e(c, a));
                if (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0) || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0) {
                    hit = Some(f);
                }
            }
        }
        hit
    }

    /// Clic dans l'éditeur UV en mode sommets / arêtes / faces. Maj : ajouter ou retirer.
    /// Renvoie vrai si un élément a été touché.
    pub fn click_component(&mut self, p: [f32; 2], tol: f32, extend: bool) -> bool {
        let picked: Vec<usize> = match self.sel_mode {
            SelectMode::Vertex => self.pick_corner(p, tol).map(|c| self.sticky(c)).unwrap_or_default(),
            SelectMode::Edge => self.pick_edge(p, tol).map(|(a, b)| [self.sticky(a), self.sticky(b)].concat()).unwrap_or_default(),
            SelectMode::Face => self.pick_face(p).map(|f| self.model.mesh.face_range(f).flat_map(|c| self.sticky(c)).collect()).unwrap_or_default(),
            SelectMode::Island => return false,
        };
        let all_on = !picked.is_empty() && picked.iter().all(|&c| self.corner_sel[c]);
        if !extend {
            self.corner_sel.iter_mut().for_each(|s| *s = false);
        }
        // Maj+clic sur un élément déjà choisi : il est retiré
        let on = !(extend && all_on);
        for c in &picked {
            self.corner_sel[*c] = on;
        }
        self.touch();
        !picked.is_empty()
    }

    /// Double-clic en mode arêtes : boucle d'arêtes (dans l'îlot de l'arête cliquée).
    pub fn select_loop(&mut self, p: [f32; 2], tol: f32, extend: bool) -> usize {
        let Some((ca, cb)) = self.pick_edge(p, tol) else { return 0 };
        let m = &self.model.mesh;
        let lp: HashSet<(u32, u32)> = easyuv::uvtools::edge_loop(m, m.corners[ca], m.corners[cb]).into_iter().collect();
        let isl = self.face_island_of_corner(ca);
        if !extend {
            self.corner_sel.iter_mut().for_each(|s| *s = false);
        }
        for f in 0..m.face_count() {
            if self.face_island.get(f).copied() != Some(isl) {
                continue;
            }
            let r: Vec<usize> = m.face_range(f).collect();
            for i in 0..r.len() {
                let (a, b) = (r[i], r[(i + 1) % r.len()]);
                let (va, vb) = (m.corners[a], m.corners[b]);
                if lp.contains(&(va.min(vb), va.max(vb))) {
                    self.corner_sel[a] = true;
                    self.corner_sel[b] = true;
                }
            }
        }
        self.touch();
        lp.len()
    }

    /// Sélection au rectangle, selon le mode. `mode` : 1 = ajouter, -1 = retirer, 0 = remplacer.
    pub fn box_select(&mut self, lo: [f32; 2], hi: [f32; 2], mode: i32) {
        self.select_where(|q| q[0] >= lo[0] && q[0] <= hi[0] && q[1] >= lo[1] && q[1] <= hi[1], mode);
    }

    /// Sélection par zone (rectangle, lasso, cercle) : `inside` dit si un point UV est dans
    /// la zone. Sommets et arêtes : les coins dedans ; faces et îlots : celles dont le centre
    /// est dedans. `mode` : 1 = ajouter, -1 = retirer, 0 = remplacer.
    pub fn select_where(&mut self, inside: impl Fn([f32; 2]) -> bool, mode: i32) {
        let Some(uv) = &self.uv else { return };
        let m = &self.model.mesh;
        let merge = |s: &mut bool, h: bool| {
            *s = match mode {
                1 => *s || h,
                -1 => *s && !h,
                _ => h,
            }
        };
        let center = |f: usize| {
            let r = m.face_range(f);
            let n = r.len() as f32;
            r.fold([0.0f32; 2], |a, k| [a[0] + uv.corner_uv[k][0] / n, a[1] + uv.corner_uv[k][1] / n])
        };
        if self.sel_mode == SelectMode::Island {
            let mut hit = vec![false; self.island_selected.len()];
            for f in (0..m.face_count()).filter(|&f| self.face_visible(f)) {
                if inside(center(f)) {
                    hit[self.face_island[f] as usize] = true;
                }
            }
            for (s, h) in self.island_selected.iter_mut().zip(hit) {
                merge(s, h);
            }
        } else {
            let mut hit = vec![false; m.corners.len()];
            for f in (0..m.face_count()).filter(|&f| self.face_visible(f)) {
                let r = m.face_range(f);
                if self.sel_mode == SelectMode::Face {
                    if inside(center(f)) {
                        r.for_each(|k| hit[k] = true);
                    }
                } else {
                    for k in r {
                        hit[k] = inside(uv.corner_uv[k]);
                    }
                }
            }
            for (s, h) in self.corner_sel.iter_mut().zip(hit) {
                merge(s, h);
            }
        }
        self.touch();
    }

    // ---------- sélection dans la vue 3D (mode Édition, comme Blender) ----------

    /// Coins de sommets du maillage (tous les îlots : la vue 3D ne voit pas les coutures).
    fn select_vertices(&mut self, verts: &[u32], on: bool) {
        let set: HashSet<u32> = verts.iter().copied().collect();
        let visible: Vec<usize> = (0..self.model.mesh.face_count()).filter(|&f| self.face_visible(f)).collect();
        let m = &self.model.mesh;
        for f in visible {
            for c in m.face_range(f) {
                if set.contains(&m.corners[c]) {
                    self.corner_sel[c] = on;
                }
            }
        }
    }

    fn vertices_selected(&self, verts: &[u32]) -> bool {
        let m = &self.model.mesh;
        verts.iter().all(|&v| (0..m.corners.len()).any(|c| m.corners[c] == v && self.corner_sel[c]))
    }

    /// Clic dans la vue 3D en mode Édition : sommet, arête ou face selon le mode, sur la face
    /// `f` touchée au point écran `p` (`screen` donne la position écran d'un sommet).
    /// Maj : ajouter ou retirer. En mode îlots, l'îlot de la face.
    pub fn click_3d(&mut self, f: Option<usize>, p: [f32; 2], screen: impl Fn(u32) -> Option<[f32; 2]>, extend: bool) {
        // une face d'un objet qui n'est pas en Édition compte comme le vide
        let Some(f) = f.filter(|&f| self.face_visible(f)) else {
            if !extend {
                self.select_all_uv(false);
            }
            return;
        };
        let vs = self.model.mesh.face(f).to_vec();
        let d2 = |q: [f32; 2]| (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
        let picked: Vec<u32> = match self.sel_mode {
            SelectMode::Vertex => vs.iter().copied().min_by(|&a, &b| {
                let (qa, qb) = (screen(a).map_or(f32::MAX, d2), screen(b).map_or(f32::MAX, d2));
                qa.partial_cmp(&qb).unwrap_or(std::cmp::Ordering::Equal)
            }).into_iter().collect(),
            SelectMode::Edge => {
                let n = vs.len();
                let seg = |a: u32, b: u32| -> f32 {
                    let (Some(qa), Some(qb)) = (screen(a), screen(b)) else { return f32::MAX };
                    let d = [qb[0] - qa[0], qb[1] - qa[1]];
                    let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-9);
                    let t = (((p[0] - qa[0]) * d[0] + (p[1] - qa[1]) * d[1]) / l2).clamp(0.0, 1.0);
                    d2([qa[0] + t * d[0], qa[1] + t * d[1]])
                };
                let k = (0..n).min_by(|&i, &j| seg(vs[i], vs[(i + 1) % n]).partial_cmp(&seg(vs[j], vs[(j + 1) % n])).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(0);
                vec![vs[k], vs[(k + 1) % n]]
            }
            SelectMode::Face => vs.clone(),
            SelectMode::Island => {
                if self.uv.is_some() {
                    let i = self.face_island[f] as usize;
                    if extend {
                        self.island_selected[i] = !self.island_selected[i];
                        self.touch();
                    } else {
                        self.select_island_only(i);
                    }
                }
                return;
            }
        };
        if self.sel_mode == SelectMode::Face {
            let r = self.model.mesh.face_range(f);
            let on = !(extend && r.clone().all(|c| self.corner_sel[c]));
            if !extend {
                self.corner_sel.iter_mut().for_each(|s| *s = false);
            }
            // la face et les coins des mêmes sommets dans ses voisines (sélection de sommets)
            self.select_vertices(&vs, on);
            if !on {
                self.model.mesh.face_range(f).for_each(|c| self.corner_sel[c] = false);
            }
        } else {
            let on = !(extend && self.vertices_selected(&picked));
            if !extend {
                self.corner_sel.iter_mut().for_each(|s| *s = false);
            }
            self.select_vertices(&picked, on);
        }
        self.touch();
    }

    /// Alt+clic : boucle d'arêtes passant par l'arête la plus proche du clic, sur la face `f`.
    pub fn loop_3d(&mut self, f: usize, p: [f32; 2], screen: impl Fn(u32) -> Option<[f32; 2]>, extend: bool) -> usize {
        let vs = self.model.mesh.face(f).to_vec();
        let n = vs.len();
        let seg = |a: u32, b: u32| -> f32 {
            let (Some(qa), Some(qb)) = (screen(a), screen(b)) else { return f32::MAX };
            let d = [qb[0] - qa[0], qb[1] - qa[1]];
            let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-9);
            let t = (((p[0] - qa[0]) * d[0] + (p[1] - qa[1]) * d[1]) / l2).clamp(0.0, 1.0);
            (qa[0] + t * d[0] - p[0]).powi(2) + (qa[1] + t * d[1] - p[1]).powi(2)
        };
        let k = (0..n).min_by(|&i, &j| seg(vs[i], vs[(i + 1) % n]).partial_cmp(&seg(vs[j], vs[(j + 1) % n])).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(0);
        let lp = easyuv::uvtools::edge_loop(&self.model.mesh, vs[k], vs[(k + 1) % n]);
        if self.sel_mode == SelectMode::Vertex || self.sel_mode == SelectMode::Island {
            self.set_select_mode(SelectMode::Edge);
        }
        if !extend {
            self.corner_sel.iter_mut().for_each(|s| *s = false);
        }
        let verts: Vec<u32> = lp.iter().flat_map(|&(a, b)| [a, b]).collect();
        self.select_vertices(&verts, true);
        self.touch();
        lp.len()
    }

    /// Ctrl+L : tout ce qui est relié (par le maillage) à la sélection.
    pub fn select_linked_3d(&mut self) -> usize {
        let m = &self.model.mesh;
        let mut by_vert: std::collections::HashMap<u32, Vec<usize>> = std::collections::HashMap::new();
        for f in (0..m.face_count()).filter(|&f| self.face_visible(f)) {
            for &v in m.face(f) {
                by_vert.entry(v).or_default().push(f);
            }
        }
        let mut taken = vec![false; m.face_count()];
        let mut queue: std::collections::VecDeque<usize> = (0..m.face_count()).filter(|&f| self.face_visible(f) && m.face_range(f).any(|c| self.corner_sel[c])).collect();
        queue.iter().for_each(|&f| taken[f] = true);
        while let Some(f) = queue.pop_front() {
            for &v in m.face(f) {
                for &g in &by_vert[&v] {
                    if !taken[g] {
                        taken[g] = true;
                        queue.push_back(g);
                    }
                }
            }
        }
        let n = taken.iter().filter(|&&t| t).count();
        for f in 0..m.face_count() {
            if taken[f] {
                for c in m.face_range(f) {
                    self.corner_sel[c] = true;
                }
            }
        }
        self.touch();
        n
    }

    /// Ctrl+ / Ctrl- : agrandit ou réduit la sélection d'un anneau de faces.
    pub fn grow_shrink(&mut self, grow: bool) {
        let m = &self.model.mesh;
        let mut vsel = vec![false; m.positions.len()];
        for c in 0..m.corners.len() {
            if self.corner_sel[c] {
                vsel[m.corners[c] as usize] = true;
            }
        }
        let mut next = vsel.clone();
        for f in (0..m.face_count()).filter(|&f| self.face_visible(f)) {
            let vs = m.face(f);
            let any = vs.iter().any(|&v| vsel[v as usize]);
            let all = vs.iter().all(|&v| vsel[v as usize]);
            if grow && any {
                vs.iter().for_each(|&v| next[v as usize] = true);
            }
            if !grow && any && !all {
                vs.iter().for_each(|&v| next[v as usize] = false);
            }
        }
        for c in 0..m.corners.len() {
            self.corner_sel[c] = next[m.corners[c] as usize] && self.face_visible(self.face_island_face(c));
        }
        self.touch();
    }

    /// Face d'un coin (recherche dichotomique).
    fn face_island_face(&self, c: usize) -> usize {
        self.model.mesh.face_start.partition_point(|&s| s <= c).saturating_sub(1)
    }

    /// Arêtes vives : sélectionne (mode arêtes) les arêtes dont les deux faces font un angle
    /// supérieur à `angle_deg`, comme la sélection par angle de RizomUV. Renvoie leur nombre.
    pub fn select_sharp_edges(&mut self, angle_deg: f64, extend: bool) -> usize {
        self.set_select_mode(SelectMode::Edge);
        let m = &self.model.mesh;
        let mut faces_of: std::collections::HashMap<(u32, u32), Vec<(usize, usize, usize)>> = std::collections::HashMap::new();
        for f in (0..m.face_count()).filter(|&f| self.face_visible(f)) {
            let r: Vec<usize> = m.face_range(f).collect();
            for i in 0..r.len() {
                let (a, b) = (r[i], r[(i + 1) % r.len()]);
                let (va, vb) = (m.corners[a], m.corners[b]);
                faces_of.entry((va.min(vb), va.max(vb))).or_default().push((f, a, b));
            }
        }
        if !extend {
            self.corner_sel.iter_mut().for_each(|s| *s = false);
        }
        let cos = angle_deg.to_radians().cos();
        let mut n = 0;
        for sides in faces_of.values() {
            let [(f1, a1, b1), (f2, a2, b2)] = sides[..] else { continue };
            if self.face_normals[f1].dot(self.face_normals[f2]) < cos {
                for c in [a1, b1, a2, b2] {
                    self.corner_sel[c] = true;
                }
                n += 1;
            }
        }
        self.touch();
        n
    }

    /// Faces liées par angle : depuis les faces sélectionnées, gagne les faces voisines (par le
    /// maillage, coutures comprises) dont la normale s'écarte de moins de `angle_deg` de
    /// celle de la face d'où l'on vient (« Select Linked Flat Faces » de Blender).
    pub fn select_linked_by_angle(&mut self, angle_deg: f64) -> usize {
        let seeds = self.selected_faces();
        if seeds.is_empty() {
            return 0;
        }
        self.set_select_mode(SelectMode::Face);
        let m = &self.model.mesh;
        let mut by_edge: std::collections::HashMap<(u32, u32), Vec<usize>> = std::collections::HashMap::new();
        for f in (0..m.face_count()).filter(|&f| self.face_visible(f)) {
            let vs = m.face(f);
            for i in 0..vs.len() {
                let (a, b) = (vs[i], vs[(i + 1) % vs.len()]);
                by_edge.entry((a.min(b), a.max(b))).or_default().push(f);
            }
        }
        let cos = angle_deg.to_radians().cos();
        let mut taken = vec![false; m.face_count()];
        let mut queue: std::collections::VecDeque<usize> = seeds.iter().copied().collect();
        seeds.iter().for_each(|&f| taken[f] = true);
        while let Some(f) = queue.pop_front() {
            let vs = m.face(f);
            for i in 0..vs.len() {
                let (a, b) = (vs[i], vs[(i + 1) % vs.len()]);
                for &g in by_edge.get(&(a.min(b), a.max(b))).into_iter().flatten() {
                    if !taken[g] && self.face_normals[f].dot(self.face_normals[g]) >= cos {
                        taken[g] = true;
                        queue.push_back(g);
                    }
                }
            }
        }
        let n = taken.iter().filter(|&&t| t).count();
        for f in 0..m.face_count() {
            if taken[f] {
                for c in m.face_range(f) {
                    self.corner_sel[c] = true;
                }
            }
        }
        self.touch();
        n
    }

    /// Tout sélectionner / désélectionner (A / Alt+A), selon le mode.
    pub fn select_all_uv(&mut self, on: bool) {
        if self.sel_mode == SelectMode::Island {
            self.select_all_islands(on);
            return;
        }
        let m = &self.model.mesh;
        for f in 0..m.face_count() {
            let vis = self.face_visible(f);
            for c in m.face_range(f) {
                self.corner_sel[c] = on && vis;
            }
        }
        self.touch();
    }

    /// Inverser la sélection (Ctrl+I), selon le mode.
    pub fn invert_uv(&mut self) {
        if self.sel_mode == SelectMode::Island {
            self.invert_islands();
            return;
        }
        let m = &self.model.mesh;
        for f in 0..m.face_count() {
            let vis = self.face_visible(f);
            for c in m.face_range(f) {
                self.corner_sel[c] = !self.corner_sel[c] && vis;
            }
        }
        self.touch();
    }

    /// Une face est sélectionnée quand tous ses coins le sont.
    pub fn face_selected(&self, f: usize) -> bool {
        self.model.mesh.face_range(f).all(|c| self.corner_sel.get(c).copied().unwrap_or(false))
    }

    /// Faces visées par « Déplier la sélection » : celles des îlots sélectionnés, ou les faces
    /// entièrement sélectionnées (à défaut, les îlots touchés par la sélection).
    pub fn selected_faces(&self) -> Vec<usize> {
        let m = &self.model.mesh;
        if self.sel_mode != SelectMode::Island {
            let full: Vec<usize> = (0..m.face_count()).filter(|&f| self.face_visible(f) && self.face_selected(f)).collect();
            if !full.is_empty() {
                return full;
            }
        }
        let ids: HashSet<usize> = self.selected_island_ids().into_iter().collect();
        (0..m.face_count()).filter(|&f| self.face_island.get(f).is_some_and(|&i| ids.contains(&(i as usize)))).collect()
    }

    /// Arêtes sélectionnées (paires de sommets, a < b) : arêtes de face dont les deux coins
    /// sont sélectionnés.
    pub fn selected_mesh_edges(&self) -> Vec<(u32, u32)> {
        if self.sel_mode == SelectMode::Island {
            return Vec::new();
        }
        let m = &self.model.mesh;
        let mut out = HashSet::new();
        for f in 0..m.face_count() {
            let r: Vec<usize> = m.face_range(f).collect();
            for i in 0..r.len() {
                let (a, b) = (r[i], r[(i + 1) % r.len()]);
                if self.corner_sel[a] && self.corner_sel[b] {
                    let (va, vb) = (m.corners[a], m.corners[b]);
                    out.insert((va.min(vb), va.max(vb)));
                }
            }
        }
        let mut v: Vec<(u32, u32)> = out.into_iter().collect();
        v.sort();
        v
    }

    /// Coutures marquées, en indices du sous-modèle `sub_model(targets)`.
    pub fn sub_seams(&self, targets: &[usize]) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let mut base = 0u32;
        for &i in targets {
            let o = &self.model.objects[i];
            let (v0, v1) = (o.vert_start as u32, o.vert_end as u32);
            out.extend(self.marked_seams.iter().filter(|&&(a, b)| a >= v0 && a < v1 && b >= v0 && b < v1).map(|&(a, b)| (a - v0 + base, b - v0 + base)));
            base += v1 - v0;
        }
        out.sort();
        out
    }

    /// Marque (ou efface) les arêtes sélectionnées comme coutures pour les dépliages suivants.
    pub fn mark_selected_seams(&mut self, on: bool) -> usize {
        let edges = self.selected_mesh_edges();
        for e in &edges {
            if on {
                self.marked_seams.insert(*e);
            } else {
                self.marked_seams.remove(e);
            }
        }
        self.touch();
        edges.len()
    }

    /// Remplace les UV des faces `faces` par celles de `r` (dépliées à part), range les
    /// nouveaux îlots dans la place libre à la densité des autres, recalcule les îlots et
    /// les coutures, et sélectionne le résultat.
    fn replace_faces_uv(&mut self, faces: &[usize], r: &UvResult, params: &easyuv::Params, texture_size: u32) {
        let Some(old) = &self.uv else { return };
        let m = &self.model.mesh;
        let mut uv = old.corner_uv.clone();
        let mut k = 0;
        for &f in faces {
            for c in m.face_range(f) {
                uv[c] = r.corner_uv[k];
                k += 1;
            }
        }
        let (fi, n) = easyuv::layout::uv_islands(m, &uv);
        let mut which = vec![false; n];
        for &f in faces {
            which[fi[f] as usize] = true;
        }
        easyuv::layout::layout_islands(m, &mut uv, &fi, Some(&which), params, true);
        let (seams, seam_length) = easyuv::layout::uv_seams(m, &uv, &fi);
        let mut stats = old.stats.clone();
        stats.charts = n;
        stats.seams = seams.len();
        stats.seam_length = seam_length;
        stats.time_ms = r.stats.time_ms;
        let mode = self.sel_mode;
        self.set_uv(UvResult { corner_uv: uv, face_chart: fi, seams, stats }, texture_size);
        // le résultat reste sélectionné, dans le mode courant
        let fi = self.face_island.clone();
        for &f in faces {
            self.island_selected[fi[f] as usize] = true;
            for c in self.model.mesh.face_range(f) {
                self.corner_sel[c] = mode != SelectMode::Island;
            }
        }
        self.touch();
    }

    /// Projection depuis la vue : les faces sélectionnées dans l'éditeur UV (sinon celles des
    /// objets sélectionnés, sinon tout) prennent les UV de ce que l'on voit dans la vue 3D.
    /// Rangées dans la place libre à la densité des autres, ou étirées sur toute la tuile
    /// (`fit`). Renvoie le nombre de faces projetées.
    pub fn project_from_view(&mut self, right: V3, up: V3, params: &easyuv::Params, texture_size: u32, fit: bool) -> usize {
        let faces: Vec<usize> = if self.uv.is_some() && self.has_island_selection() {
            self.selected_faces()
        } else {
            let targets = self.unwrap_targets();
            targets.iter().flat_map(|&i| self.model.objects[i].face_start..self.model.objects[i].face_end).filter(|&f| self.face_visible(f)).collect()
        };
        if faces.is_empty() {
            return 0;
        }
        let m = &self.model.mesh;
        let mut uv = self.uv.as_ref().map(|u| u.corner_uv.clone()).unwrap_or_else(|| vec![[0.0; 2]; m.corners.len()]);
        let stats = self.uv.as_ref().map(|u| u.stats.clone()).unwrap_or_default();
        easyuv::uvtools::project_faces(m, &mut uv, &faces, right, up);
        let (fi, n) = easyuv::layout::uv_islands(m, &uv);
        let mut which = vec![false; n];
        for &f in &faces {
            which[fi[f] as usize] = true;
        }
        let ids: Vec<usize> = (0..n).filter(|&i| which[i]).collect();
        if fit {
            easyuv::uvtools::fit_to_tile(&mut uv, &easyuv::uvtools::Islands::new(m, &fi, &ids));
        } else {
            easyuv::layout::layout_islands(m, &mut uv, &fi, Some(&which), params, true);
        }
        let (seams, seam_length) = easyuv::layout::uv_seams(m, &uv, &fi);
        let mut stats = stats;
        stats.charts = n;
        stats.seams = seams.len();
        stats.seam_length = seam_length;
        let mode = self.sel_mode;
        self.set_uv(UvResult { corner_uv: uv, face_chart: fi, seams, stats }, texture_size);
        // le résultat reste sélectionné
        let fi = self.face_island.clone();
        for &f in &faces {
            self.island_selected[fi[f] as usize] = true;
            for c in self.model.mesh.face_range(f) {
                self.corner_sel[c] = mode != SelectMode::Island;
            }
        }
        self.touch();
        faces.len()
    }

    /// Dépliage intelligent des seules faces sélectionnées (îlots ou faces) ; les coutures
    /// marquées sont respectées.
    pub fn unwrap_selection(&mut self, params: &easyuv::Params, texture_size: u32) -> Result<String, String> {
        if self.uv.is_none() {
            return Err("Dépliez d'abord l'objet (U) : la sélection UV vient ensuite".into());
        }
        let faces = self.selected_faces();
        if faces.is_empty() {
            return Err("Sélectionnez d'abord des îlots ou des faces dans l'éditeur UV".into());
        }
        let seams: Vec<(u32, u32)> = self.marked_seams.iter().copied().collect();
        let r = easyuv::uvtools::unwrap_faces(&self.model.mesh, &faces, &seams, params);
        let charts = r.stats.charts;
        self.replace_faces_uv(&faces, &r, params, texture_size);
        Ok(format!("Sélection dépliée : {} faces, {charts} îlot(s), distorsion {:.3}, {:.0} ms", faces.len(), r.stats.mean_distortion, r.stats.time_ms))
    }

    /// Couper : les arêtes sélectionnées deviennent des coutures (marquées), et les îlots
    /// qu'elles traversent sont redépliés avec leurs coutures actuelles plus celles-ci.
    pub fn cut_selected(&mut self, params: &easyuv::Params, texture_size: u32) -> Result<String, String> {
        let edges = self.selected_mesh_edges();
        if edges.is_empty() {
            return Err("Couper : sélectionnez des arêtes (mode Arêtes, touche 2 ; double-clic : boucle)".into());
        }
        let Some(uv) = &self.uv else { return Err("Dépliez d'abord l'objet (U)".into()) };
        let m = &self.model.mesh;
        let set: HashSet<(u32, u32)> = edges.iter().copied().collect();
        // îlots traversés par les arêtes à couper
        let mut touched = HashSet::new();
        for f in 0..m.face_count() {
            let vs = m.face(f);
            if (0..vs.len()).any(|i| set.contains(&(vs[i].min(vs[(i + 1) % vs.len()]), vs[i].max(vs[(i + 1) % vs.len()])))) {
                touched.insert(self.face_island[f]);
            }
        }
        let faces: Vec<usize> = (0..m.face_count()).filter(|&f| touched.contains(&self.face_island[f])).collect();
        let (current, _) = easyuv::layout::uv_seams(m, &uv.corner_uv, &self.face_island);
        let mut seams: Vec<(u32, u32)> = current;
        seams.extend(edges.iter().copied());
        seams.extend(self.marked_seams.iter().copied());
        for e in &edges {
            self.marked_seams.insert(*e);
        }
        let r = easyuv::uvtools::unwrap_faces(&self.model.mesh, &faces, &seams, params);
        self.replace_faces_uv(&faces, &r, params, texture_size);
        Ok(format!("{} arête(s) coupée(s) : {} îlot(s) redéplié(s) — Ctrl+Z pour annuler", edges.len(), touched.len()))
    }

    /// Coudre : rapproche et soude les îlots le long des arêtes sélectionnées, puis détend
    /// l'îlot recousu. Les coutures marquées correspondantes sont effacées.
    pub fn sew_selected(&mut self, texture_size: u32) -> Result<String, String> {
        let edges = self.selected_mesh_edges();
        if edges.is_empty() {
            return Err("Coudre : sélectionnez des arêtes de couture (mode Arêtes, touche 2)".into());
        }
        let Some(uv) = &mut self.uv else { return Err("Dépliez d'abord l'objet (U)".into()) };
        let m = &self.model.mesh;
        let n = easyuv::uvtools::sew(m, &mut uv.corner_uv, &self.face_island, &edges);
        if n == 0 {
            return Err("Rien à coudre : choisissez des arêtes sur une couture (les deux côtés d'une couture)".into());
        }
        for e in &edges {
            self.marked_seams.remove(e);
        }
        let corner_uv = uv.corner_uv.clone();
        let (fi, nisl) = easyuv::layout::uv_islands(m, &corner_uv);
        let (seams, seam_length) = easyuv::layout::uv_seams(m, &corner_uv, &fi);
        let mut stats = uv.stats.clone();
        stats.charts = nisl;
        stats.seams = seams.len();
        stats.seam_length = seam_length;
        let set: HashSet<(u32, u32)> = edges.iter().copied().collect();
        let mut sewn = HashSet::new();
        for f in 0..m.face_count() {
            let vs = m.face(f);
            if (0..vs.len()).any(|i| set.contains(&(vs[i].min(vs[(i + 1) % vs.len()]), vs[i].max(vs[(i + 1) % vs.len()])))) {
                sewn.insert(fi[f] as usize);
            }
        }
        let mut corner_uv = corner_uv;
        let ids: Vec<usize> = sewn.into_iter().collect();
        // l'îlot recousu est détendu, pour effacer le pli le long de la couture
        easyuv::uvtools::optimize(m, &mut corner_uv, &easyuv::uvtools::Islands::new(m, &fi, &ids), 4);
        let mode = self.sel_mode;
        self.set_uv(UvResult { corner_uv, face_chart: fi, seams, stats }, texture_size);
        let _ = mode;
        Ok(format!("{n} couture(s) recousue(s) — Ctrl+Z pour annuler"))
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
        let sel = self.selected_island_ids();
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
        let sel = self.selected_island_ids();
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
    /// En mode sommets, 4 sommets choisis sur le bord d'un îlot en sont les coins imposés.
    pub fn straighten_strips(&mut self, texture_size: u32) -> (usize, usize) {
        let mut which = vec![false; self.island_selected.len()];
        for i in self.selected_island_ids() {
            which[i] = true;
        }
        let hint: Vec<usize> = if self.sel_mode == SelectMode::Vertex { self.selected_corners().into_iter().map(|c| c as usize).collect() } else { Vec::new() };
        let Some(uv) = &mut self.uv else { return (0, 0) };
        let r = easyuv::layout::straighten_with_corners(&self.model.mesh, &mut uv.corner_uv, &self.face_island, &which, &hint);
        if r.0 > 0 {
            // finition : rangées et colonnes passées à la règle (Grille parfaite)
            let ids: Vec<usize> = (0..which.len()).filter(|&i| which[i]).collect();
            easyuv::uvtools::snap_grid(&self.model.mesh, &mut uv.corner_uv, &easyuv::uvtools::Islands::new(&self.model.mesh, &self.face_island, &ids));
            self.refresh(texture_size);
        }
        r
    }

    /// Grille parfaite sur la sélection (ou tout). Renvoie (îlots alignés, îlots visés).
    pub fn snap_grid(&mut self, texture_size: u32) -> (usize, usize) {
        let ids = self.brush_islands();
        let Some(uv) = &mut self.uv else { return (0, 0) };
        let n = easyuv::uvtools::snap_grid(&self.model.mesh, &mut uv.corner_uv, &easyuv::uvtools::Islands::new(&self.model.mesh, &self.face_island, &ids));
        if n > 0 {
            self.refresh(texture_size);
        }
        (n, ids.len())
    }

    /// Remet d'aplomb la sélection (ou tous les îlots) d'après leurs arêtes.
    pub fn orient_to_edges(&mut self, texture_size: u32) -> usize {
        let ids = self.brush_islands();
        let Some(uv) = &mut self.uv else { return 0 };
        let n = easyuv::uvtools::orient_to_edges(&self.model.mesh, &mut uv.corner_uv, &easyuv::uvtools::Islands::new(&self.model.mesh, &self.face_island, &ids));
        if n > 0 {
            self.refresh(texture_size);
        }
        n
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
    fn isolation_freezes_the_rest() {
        let mut s = scene();
        assert_eq!(s.isolate_selection(), None, "rien de sélectionné : rien à isoler");
        // une face seule : ses sommets partagés avec des voisines hors zone sont figés, et
        // aucun coin d'une autre face n'est libre (les voisines ne se déforment pas)
        s.set_select_mode(SelectMode::Face);
        let f = s.model.objects[1].face_start + 3;
        for c in s.model.mesh.face_range(f) {
            s.corner_sel[c] = true;
        }
        let free = s.selection_free();
        for g in (0..s.model.mesh.face_count()).filter(|&g| g != f) {
            assert!(s.model.mesh.face_range(g).all(|c| !free[c]));
        }
        // l'îlot entier du tore : tout est libre dans l'îlot, rien ailleurs
        s.set_select_mode(SelectMode::Island);
        let i = s.face_island[f] as usize;
        s.click_island(Some(i), false);
        assert!(s.isolate_selection().unwrap() > 0);
        let (ids, free) = s.brush_zone();
        assert_eq!(ids, vec![i]);
        let free = free.unwrap();
        for g in 0..s.model.mesh.face_count() {
            let inside = s.face_island[g] as usize == i;
            assert!(s.model.mesh.face_range(g).all(|c| free[c] == inside));
        }
        // la zone tient même quand la sélection change, et G / R / S n'en sortent pas
        s.select_all_uv(true);
        assert!(s.editable_corners().iter().all(|&c| free[c as usize]));
        assert!(s.clear_isolation());
        assert_eq!(s.editable_corners().len(), s.model.mesh.corners.len());
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
    fn edit_mode_only_selected_objects() {
        let mut s = scene();
        s.select_all(false);
        assert_eq!(s.enter_edit(), 0, "rien de sélectionné : pas d'Édition");
        assert!(!s.is_editing());
        s.set_select_mode(SelectMode::Face);
        s.select_all_uv(true);
        s.click(Some(1), false);
        assert_eq!(s.enter_edit(), 1);
        assert!(s.in_edit(1) && !s.in_edit(0));
        // la sélection d'éléments des autres objets est retirée, et A ne la remet pas
        let only_1 = |s: &Scene| (0..s.model.mesh.face_count()).all(|f| s.face_selected(f) == (s.face_object[f] == 1));
        assert!(only_1(&s));
        s.select_all_uv(false);
        s.select_all_uv(true);
        assert!(only_1(&s));
        s.exit_edit();
        s.select_all_uv(true);
        assert!((0..s.model.mesh.face_count()).all(|f| s.face_selected(f)));
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

    fn cube_scene() -> Scene {
        let m = Model::single("Cube", easyuv::model::weld(&primitives::cube(2.0)));
        let mut s = Scene::new("cube".into(), m);
        let uv = easyuv::model::unwrap_model(&s.model, &easyuv::Params::default(), true);
        s.set_uv(uv, 2048);
        s
    }

    /// Sélectionne en mode arêtes l'arête (a, b) du maillage, des deux côtés.
    fn pick_edge(s: &mut Scene, a: u32, b: u32) {
        s.set_select_mode(SelectMode::Edge);
        s.corner_sel.iter_mut().for_each(|c| *c = false);
        for c in 0..s.model.mesh.corners.len() {
            let v = s.model.mesh.corners[c];
            if v == a || v == b {
                s.corner_sel[c] = true;
            }
        }
    }

    #[test]
    fn modes_convert_selection() {
        let mut s = scene();
        s.click_island(Some(0), false);
        let faces: Vec<usize> = (0..s.model.mesh.face_count()).filter(|&f| s.face_island[f] == 0).collect();
        s.set_select_mode(SelectMode::Face);
        assert_eq!(s.selected_faces(), faces);
        assert_eq!(s.selected_island_ids(), vec![0]);
        s.set_select_mode(SelectMode::Island);
        assert!(s.island_selected[0] && s.island_selected.iter().filter(|&&x| x).count() == 1);
    }

    #[test]
    fn unwrap_selection_keeps_the_rest() {
        let mut s = scene();
        s.click_island(Some(0), false);
        let sel: HashSet<usize> = s.selected_faces().into_iter().collect();
        let before = s.uv.as_ref().unwrap().corner_uv.clone();
        s.unwrap_selection(&easyuv::Params::default(), 2048).unwrap();
        let after = &s.uv.as_ref().unwrap().corner_uv;
        for f in 0..s.model.mesh.face_count() {
            if !sel.contains(&f) {
                for c in s.model.mesh.face_range(f) {
                    assert_eq!(before[c], after[c], "face {f} hors sélection modifiée");
                }
            }
        }
        assert!(s.has_island_selection(), "le résultat reste sélectionné");
    }

    #[test]
    fn sew_then_cut_cube_faces() {
        let mut s = cube_scene();
        let n0 = s.island_selected.len();
        assert_eq!(n0, 6);
        // une arête partagée par deux faces du cube
        let vs = s.model.mesh.face(0).to_vec();
        pick_edge(&mut s, vs[0], vs[1]);
        assert_eq!(s.selected_mesh_edges(), vec![(vs[0].min(vs[1]), vs[0].max(vs[1]))]);
        s.sew_selected(2048).unwrap();
        assert_eq!(s.island_selected.len(), 5, "deux faces recousues");
        pick_edge(&mut s, vs[0], vs[1]);
        s.cut_selected(&easyuv::Params::default(), 2048).unwrap();
        assert_eq!(s.island_selected.len(), 6, "recoupées");
        assert!(s.marked_seams.contains(&(vs[0].min(vs[1]), vs[0].max(vs[1]))));
    }

    #[test]
    fn edge_loop_in_uv_editor() {
        let mut s = scene();
        s.set_select_mode(SelectMode::Edge);
        // une arête du tore (objet 1), cliquée au milieu de son segment UV
        let o = s.model.objects[1].clone();
        let c = s.model.mesh.face_range(o.face_start).start;
        let uv = s.uv.as_ref().unwrap().corner_uv.clone();
        let mid = [(uv[c][0] + uv[c + 1][0]) * 0.5, (uv[c][1] + uv[c + 1][1]) * 0.5];
        let n = s.select_loop(mid, 1e-3, false);
        assert!(n >= 8, "boucle de {n} arêtes");
        assert!(s.selected_mesh_edges().len() >= 2);
    }

    #[test]
    fn project_from_view_keeps_the_rest() {
        let mut s = scene();
        // le cube seul (objet 0), vu de face
        s.click(Some(0), false);
        let other: Vec<usize> = s.model.corner_range(1).collect();
        let before: Vec<[f32; 2]> = other.iter().map(|&c| s.uv.as_ref().unwrap().corner_uv[c]).collect();
        let n = s.project_from_view(V3::new(1.0, 0.0, 0.0), V3::new(0.0, 0.0, 1.0), &easyuv::Params::default(), 2048, false);
        assert_eq!(n, s.model.objects[0].face_end - s.model.objects[0].face_start);
        let after: Vec<[f32; 2]> = other.iter().map(|&c| s.uv.as_ref().unwrap().corner_uv[c]).collect();
        assert_eq!(before, after, "le tore ne bouge pas");
        assert!(s.has_island_selection());
    }

    #[test]
    fn select_in_3d_view_before_unwrap() {
        // sans UV : on peut choisir arêtes et faces dans la vue 3D, puis marquer une couture
        let m = Model::single("Tore", easyuv::model::weld(&primitives::torus(1.0, 0.3, 16, 8)));
        let mut s = Scene::new("t".into(), m);
        assert!(s.uv.is_none());
        s.set_select_mode(SelectMode::Edge);
        let vs = s.model.mesh.face(0).to_vec();
        let n = s.loop_3d(0, [0.0, 0.0], |v| Some([s_pos(v), 0.0]), false);
        assert!(n == 16 || n == 8, "boucle de {n}");
        assert!(s.selected_mesh_edges().len() >= n);
        assert_eq!(s.mark_selected_seams(true), s.selected_mesh_edges().len());
        // face
        s.set_select_mode(SelectMode::Face);
        s.click_3d(Some(3), [0.0, 0.0], |_| None, false);
        assert!(s.face_selected(3));
        let _ = vs;
        // relié : tout le tore
        assert_eq!(s.select_linked_3d(), s.model.mesh.face_count());
    }

    /// Position écran factice : abscisse = numéro du sommet (l'arête la plus proche de 0 est
    /// celle des plus petits numéros).
    fn s_pos(v: u32) -> f32 {
        v as f32
    }

    #[test]
    fn select_by_angle_on_cube() {
        let mut s = cube_scene();
        // les 12 arêtes du cube font 90° : toutes vives au-delà de 30°, aucune au-delà de 100°
        assert_eq!(s.select_sharp_edges(30.0, false), 12);
        assert_eq!(s.selected_mesh_edges().len(), 12);
        assert_eq!(s.select_sharp_edges(100.0, false), 0);
        // faces liées : une face seule à 30°, tout le cube à 100°
        s.set_select_mode(SelectMode::Face);
        s.corner_sel.iter_mut().for_each(|c| *c = false);
        let r = s.model.mesh.face_range(0);
        r.for_each(|c| s.corner_sel[c] = true);
        assert_eq!(s.select_linked_by_angle(30.0), 1);
        assert_eq!(s.select_linked_by_angle(100.0), 6);
    }

    #[test]
    fn zone_selection_adds_and_removes() {
        let mut s = scene();
        s.set_select_mode(SelectMode::Island);
        s.select_where(|_| true, 0);
        let all = s.island_selected.iter().filter(|&&x| x).count();
        assert_eq!(all, s.island_selected.len());
        // retirer tout ce qui est à gauche de u = 0,5, puis rajouter
        s.select_where(|q| q[0] < 0.5, -1);
        let after = s.island_selected.iter().filter(|&&x| x).count();
        assert!(after < all);
        s.select_where(|q| q[0] < 0.5, 1);
        assert_eq!(s.island_selected.iter().filter(|&&x| x).count(), all);
    }
}
