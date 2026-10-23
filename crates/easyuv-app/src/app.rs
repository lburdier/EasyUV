use crate::icons::{self, Icon};
use crate::io;
use crate::scene::{Scene, SelectMode};
use crate::theme::{self, primary_button, section};
use crate::update::{self, Release};
use crate::uvview::{self, SelShape, UvCache, UvCamera, UvEditor, UvEvent, UvTool};
use crate::raster::Mips;
use crate::viewport::{self, Camera, RenderCache, Shading, ViewOptions, TEX_SIZE};
use easyuv::checker::{generate_rgba8, CheckerStyle};
use easyuv::learn::{self, Learner};
use easyuv::uvtools;
use easyuv::math::V3;
use easyuv::mesh::Mesh;
use easyuv::model::Model;
use easyuv::regions::ShapePreset;
use easyuv::{primitives, DensityMode, Params, UvResult};
use eframe::egui::{self, Color32, RichText, TextureHandle, TextureOptions};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

/// Résumé d'un dépliage appris, affiché dans le panneau.
struct LearnInfo {
    params: Params,
    score: f64,
    default_score: f64,
    trials: usize,
    confidence: f64,
}

/// Action demandée depuis l'outliner, appliquée après l'affichage du panneau.
enum OutlinerAction {
    Click(usize, bool),
    ToggleHidden(usize),
    StartRename(usize),
    Rename(usize, String),
    CancelRename,
    Duplicate(usize),
    Delete(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Workspace {    Layout,
    UvEditing,
    Viewport,
}

enum UpdateState {
    Idle,
    /// (réponse, vérification demandée par l'utilisateur)
    Checking(Receiver<Result<Option<Release>, String>>, bool),
    Available(Release),
    Installing(Receiver<Result<PathBuf, String>>),
    UpToDate,
    Failed(String),
}

pub struct EasyUvApp {
    scene: Option<Scene>,
    params: Params,
    target_mode: bool,
    target_density: f64,
    pack_together: bool,
    cam: Camera,
    uv_cam: UvCamera,
    view: ViewOptions,
    checker_style: CheckerStyle,
    checker_cells: u32,
    texture: Option<TextureHandle>,
    texture_dirty: bool,
    workspace: Workspace,
    job: Option<Receiver<(UvResult, Option<LearnInfo>)>>,
    /// Objets visés par le dépliage en cours.
    job_targets: Vec<usize>,
    /// Le calcul en cours est un rangement (layout) et non un dépliage.
    job_layout: bool,
    /// Rangement : remettre tous les îlots à la même densité de texels.
    pack_equalize: bool,
    uv_edit: UvEditor,
    /// Zone de l'éditeur UV (image précédente) : les raccourcis y visent les îlots.
    uv_rect: Option<egui::Rect>,
    learner: Arc<Mutex<Learner>>,
    auto_learn: bool,
    trials: usize,
    /// 0 = moins de coutures, 1 = moins de distorsion.
    priority: f64,
    last_learn: Option<LearnInfo>,
    confirm_reset: bool,
    update: UpdateState,
    show_update: bool,
    /// Fenêtre « Nouveautés » (ouverte d'elle-même après une mise à jour).
    show_news: bool,
    /// Part de la vue 3D dans les espaces Layout et UV Editing (séparateur déplaçable).
    split: [f32; 2],
    /// Outils UV : angle de « Pivoter » et passes de « Optimiser ».
    rot_angle: f64,
    optimize_iters: usize,
    /// Score UV en direct, et la génération de scène pour laquelle il a été calculé.
    score: Option<easyuv::metrics::UvScore>,
    score_gen: Option<u64>,
    score_time: f64,
    /// Seuil de la sélection par angle (degrés).
    sel_angle: f64,
    /// Vue 3D en mode Édition (Tab, comme Blender) : on y choisit sommets, arêtes et faces.
    edit_mode: bool,
    /// Dernier mode de sélection vu : passer en sommets / arêtes / faces ouvre l'Édition.
    last_sel_mode: SelectMode,
    status: String,
    error: Option<String>,
    /// Damier en mipmaps pour le rendu 3D logiciel.
    mips: Mips,
    render: RenderCache,
    uv_cache: UvCache,
    undo: Vec<Scene>,
    redo: Vec<Scene>,
    /// Objet en cours de renommage dans l'outliner.
    renaming: Option<(usize, String)>,
    /// Menu « Supprimer ? » ouvert par X, à la position du pointeur (comme Blender).
    confirm_delete: Option<egui::Pos2>,
}

const EDIT_HELP: &str = "Mode Édition (Tab) : 1 sommets, 2 arêtes, 3 faces, 4 îlots. Clic : choisir, Maj+clic : ajouter ou retirer, Alt+clic : boucle d'arêtes, A / Alt+A : tout / rien, Ctrl+I : inverser, L / Ctrl+L : partie reliée, Ctrl+ / Ctrl- : agrandir ou réduire, Ctrl+E : marquer la couture, Maj+Ctrl+E : la retirer, U : déplier la sélection. La sélection est la même que dans l'éditeur UV.";

const PROJECT_HELP: &str = "Les UV prennent exactement ce que l'on voit dans la vue 3D, sous cet angle (comme Project From View de Blender) : idéal pour un logo, une étiquette ou un décor à peindre de face. S'applique aux faces sélectionnées dans l'éditeur UV, sinon aux objets sélectionnés, sinon à tout. Le résultat est rangé dans la place libre, à la densité des autres îlots.";

/// Outils de couture du panneau « Outils UV ».
#[derive(Clone, Copy)]
enum SeamOp {
    Cut,
    Sew,
    Mark(bool),
    ClearAll,
}

fn tool_icon(t: UvTool) -> Icon {
    match t {
        UvTool::Select => Icon::Select,
        UvTool::Grab => Icon::Grab,
        UvTool::Relax => Icon::Relax,
        UvTool::Straighten => Icon::Straighten,
        UvTool::Pinch => Icon::Pinch,
        UvTool::Rect => Icon::Rect,
    }
}

fn shape_icon(s: SelShape) -> Icon {
    match s {
        SelShape::Box => Icon::BoxSel,
        SelShape::Lasso => Icon::Lasso,
        SelShape::Circle => Icon::Circle,
    }
}

fn mode_icon(m: SelectMode) -> Icon {
    match m {
        SelectMode::Vertex => Icon::Vertex,
        SelectMode::Edge => Icon::Edge,
        SelectMode::Face => Icon::Face,
        SelectMode::Island => Icon::Island,
    }
}

fn mode_help(m: SelectMode) -> &'static str {
    match m {
        SelectMode::Vertex => "Clic : un sommet UV (les deux côtés d'une couture restent séparés). Maj+clic : ajouter ou retirer. Glisser ou G R S : déplacer la sélection.",
        SelectMode::Edge => "Clic : une arête. Double-clic : toute la boucle d'arêtes. Les outils Couper, Coudre et Marquer agissent sur les arêtes choisies.",
        SelectMode::Face => "Clic : une face. Rectangle : les faces dont le centre est dedans. U déplie seulement les faces choisies.",
        SelectMode::Island => "Clic : un îlot entier. Les outils d'îlots (aligner, empiler, optimiser…) agissent sur les îlots choisis.",
    }
}

/// Vert au-dessus de 80, jaune au-dessus de 60, orange en dessous.
fn score_color(total: f64) -> Color32 {
    if total >= 80.0 {
        theme::AXIS_Y
    } else if total >= 60.0 {
        Color32::from_rgb(0xf0, 0xd0, 0x40)
    } else {
        theme::ACCENT_ORANGE
    }
}

/// Détail du score UV : une barre par critère, et le conseil le plus utile.
fn score_details(ui: &mut egui::Ui, sc: &easyuv::metrics::UvScore) {
    ui.set_min_width(260.0);
    ui.label(RichText::new(format!("Score UV : {:.0} / 100", sc.total)).strong().color(score_color(sc.total)));
    egui::Grid::new("score_parts").num_columns(3).spacing([8.0, 3.0]).show(ui, |ui| {
        for (name, v, w) in &sc.parts {
            ui.label(RichText::new(*name).small());
            let (r, p) = ui.allocate_painter(egui::vec2(110.0, 8.0), egui::Sense::hover());
            p.rect_filled(r.rect, 2.0, theme::FIELD);
            let mut fill = r.rect;
            fill.set_width(r.rect.width() * *v as f32);
            p.rect_filled(fill, 2.0, score_color(v * 100.0));
            ui.label(RichText::new(format!("{:.0}/{w:.0}", v * w)).small().color(theme::TEXT_DIM));
            ui.end_row();
        }
    });
    ui.label(
        RichText::new(format!(
            "distorsion {:.3} · chevauchement {:.1} % · {} retournée(s) · tuile {:.0} % · hors tuile {:.1} % · écart de densité {:.0} %",
            sc.distortion,
            sc.overlap * 100.0,
            sc.flipped,
            sc.coverage * 100.0,
            sc.outside * 100.0,
            sc.density_spread * 100.0
        ))
        .small()
        .color(theme::TEXT_DIM),
    );
    if let Some(a) = sc.advice() {
        ui.label(RichText::new(format!("Conseil : {a}")).small().color(theme::ACCENT_ORANGE));
    }
}

/// Outil d'îlots : (maillage, UV des coins, îlot de chaque face, îlots visés).
type UvOp = Box<dyn Fn(&easyuv::mesh::Mesh, &mut [[f32; 2]], &[u32], &[usize])>;

const UNDO_LEVELS: usize = 32;
/// Part de la vue 3D par défaut : Layout, UV Editing.
const SPLIT_DEFAULT: [f32; 2] = [0.58, 0.4];

impl EasyUvApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = EasyUvApp {
            scene: None,
            params: Params::default(),
            target_mode: false,
            target_density: 1024.0,
            pack_together: true,
            cam: Camera::default(),
            uv_cam: UvCamera::default(),
            view: ViewOptions { shading: Shading::Checker, wire: true, seams: true, grid: true, tiling: 1.0 },
            checker_style: CheckerStyle::Color,
            checker_cells: 16,
            texture: None,
            texture_dirty: true,
            workspace: Workspace::Layout,
            job: None,
            job_targets: Vec::new(),
            job_layout: false,
            pack_equalize: true,
            uv_edit: UvEditor::default(),
            uv_rect: None,
            learner: Arc::new(Mutex::new(Learner::load(&learn::default_path()))),
            auto_learn: true,
            trials: 6,
            priority: 0.5,
            last_learn: None,
            confirm_reset: false,
            update: UpdateState::Idle,
            show_update: false,
            show_news: crate::news::first_run_of_version(),
            split: SPLIT_DEFAULT,
            rot_angle: 45.0,
            optimize_iters: 10,
            score: None,
            score_gen: None,
            score_time: -1.0,
            sel_angle: 30.0,
            edit_mode: false,
            last_sel_mode: SelectMode::Island,
            status: "Prêt".into(),
            error: None,
            mips: Mips::new(1, &[255, 255, 255, 255]),
            render: RenderCache::default(),
            uv_cache: UvCache::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            renaming: None,
            confirm_delete: None,
        };
        // fichier passé en argument (double-clic / « Ouvrir avec »)
        let arg = std::env::args().nth(1).map(PathBuf::from).filter(|p| p.is_file());
        match arg {
            Some(p) => app.open_file(p, false),
            None => app.set_model("Forme organique".into(), Model::single("Forme organique", primitives::blob(1.0, 48, 24))),
        }
        if std::env::var_os("EASYUV_NO_UPDATE").is_none() {
            app.check_updates(&cc.egui_ctx, false);
        }
        app
    }

    fn set_model(&mut self, name: String, model: Model) {
        let s = Scene::new(name, model);
        let (lo, hi) = s.model.mesh.bounds();
        self.cam.frame(lo, hi);
        self.status = format!(
            "{} : {} objet(s), {} sommets, {} faces",
            s.name,
            s.model.objects.len(),
            s.model.mesh.positions.len(),
            s.model.mesh.face_count()
        );
        self.scene = Some(s);
        self.job = None;
    }

    /// Ajoute des objets à la scène courante, posés à droite des objets existants.
    fn append_model(&mut self, mut add: Model) {
        self.snapshot();
        let Some(s) = &self.scene else {
            let name = add.objects.first().map(|o| o.name.clone()).unwrap_or_default();
            self.set_model(name, add);
            return;
        };
        let (_, hi) = s.model.mesh.bounds();
        let (alo, ahi) = add.mesh.bounds();
        let dx = hi.x - alo.x + 0.25 * (ahi.x - alo.x).max(0.5);
        for i in 0..add.objects.len() {
            add.translate_object(i, V3::new(dx, 0.0, 0.0));
        }
        let mut model = s.model.clone();
        model.append(&add);
        let name = s.name.clone();
        let first_new = s.len();
        let active = model.objects.len() - 1;
        self.set_model(name, model);
        // comme Blender : les objets ajoutés sont sélectionnés, le dernier est actif
        if let Some(s) = &mut self.scene {
            for i in 0..s.len() {
                s.selected[i] = i >= first_new;
            }
            s.active = active;
            s.touch();
        }
    }

    fn effective_params(&self) -> Params {
        let mut p = self.params.clone();
        p.density = if self.target_mode { DensityMode::Target(self.target_density) } else { DensityMode::Fit };
        p
    }

    fn start_unwrap(&mut self, ctx: &egui::Context) {
        let Some(scene) = &self.scene else { return };
        if self.job.is_some() {
            return;
        }
        // comme Blender : seuls les objets sélectionnés sont dépliés (tous si aucune sélection)
        let targets = scene.unwrap_targets();
        let model = scene.sub_model(&targets);
        // coutures marquées à la main : imposées au dépliage
        let seams = scene.sub_seams(&targets);
        let n_targets = targets.len();
        let all = n_targets == scene.len();
        self.job_targets = targets;
        let params = self.effective_params();
        let together = self.pack_together;
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        let learner = self.auto_learn.then(|| self.learner.clone());
        let trials = self.trials;
        let priority = self.priority;
        std::thread::spawn(move || {
            let out = match learner {
                Some(l) => {
                    let mut l = l.lock().unwrap();
                    l.priority = priority;
                    let res = learn::unwrap_learned_with_seams(&model, &params, together, &mut l, trials, &seams);
                    // l'expérience est conservée même si l'application est fermée ensuite
                    let _ = l.save(&learn::default_path());
                    let info = LearnInfo {
                        params: res.params,
                        score: res.score,
                        default_score: res.default_score,
                        trials: res.trials,
                        confidence: res.confidence,
                    };
                    (res.result, Some(info))
                }
                None => (easyuv::model::unwrap_model_with_seams(&model, &params, together, &seams), None),
            };
            let _ = tx.send(out);
            ctx.request_repaint();
        });
        self.job = Some(rx);
        self.status = if all { "Dépliage de tous les objets…".into() } else { format!("Dépliage de {n_targets} objet(s) sélectionné(s)…") };
    }

    /// Mode rangement : range les UV existantes sans redéplier. Les îlots sélectionnés dans
    /// l'éditeur UV s'il y en a, sinon tous les îlots des objets visés.
    fn start_layout(&mut self, ctx: &egui::Context) {
        let Some(scene) = &self.scene else { return };
        if self.job.is_some() {
            return;
        }
        if scene.uv.is_none() {
            self.status = "Aucune UV à ranger : dépliez d'abord (U)".into();
            return;
        }
        let params = self.effective_params();
        let together = self.pack_together;
        let equalize = self.pack_equalize;
        let (tx, rx) = channel();
        let ctx2 = ctx.clone();
        if scene.has_island_selection() {
            let mesh = scene.model.mesh.clone();
            let uv = scene.uv.clone().unwrap();
            let face_island = scene.face_island.clone();
            let which = scene.island_selected.clone();
            let n = which.iter().filter(|&&w| w).count();
            self.job_targets = (0..scene.len()).collect();
            std::thread::spawn(move || {
                let t0 = std::time::Instant::now();
                let mut uv = uv;
                let rep = easyuv::layout::layout_islands(&mesh, &mut uv.corner_uv, &face_island, Some(&which), &params, equalize);
                uv.stats = easyuv::Stats { charts: rep.islands, coverage: rep.coverage, overflow: rep.overflow, time_ms: t0.elapsed().as_secs_f64() * 1000.0, ..Default::default() };
                let _ = tx.send((uv, None));
                ctx2.request_repaint();
            });
            self.status = format!("Rangement de {n} îlot(s) sélectionné(s)…");
        } else {
            let targets = scene.unwrap_targets();
            let model = scene.sub_model(&targets);
            let full = &scene.uv.as_ref().unwrap().corner_uv;
            let sub_uv: Vec<[f32; 2]> = targets.iter().flat_map(|&i| full[scene.model.corner_range(i)].iter().copied()).collect();
            let all = targets.len() == scene.len();
            self.status = if all { "Rangement de tous les îlots…".into() } else { format!("Rangement des îlots de {} objet(s)…", targets.len()) };
            self.job_targets = targets;
            std::thread::spawn(move || {
                let r = easyuv::model::pack_model(&model, &sub_uv, &params, together, equalize);
                let _ = tx.send((r, None));
                ctx2.request_repaint();
            });
        }
        self.job_layout = true;
        self.job = Some(rx);
    }

    fn poll_job(&mut self) {
        let Some(rx) = &self.job else { return };
        if let Ok((r, info)) = rx.try_recv() {
            if let Some(i) = &info {
                // les curseurs montrent les réglages retenus par le modèle
                self.params.angle_deg = i.params.angle_deg;
                self.params.max_distortion = i.params.max_distortion;
                self.params.relax_iterations = i.params.relax_iterations;
            }
            if !self.job_layout {
                self.last_learn = info;
            }
            let s = &r.stats;
            self.status = if self.job_layout {
                format!(
                    "Rangement : {} îlots, couverture {:.1} %, {:.0} ms{}",
                    s.charts,
                    s.coverage * 100.0,
                    s.time_ms,
                    if s.overflow { " — la densité cible dépasse la tuile 0-1 (UDIM)" } else { "" }
                )
            } else {
                format!(
                "Dépliage : {} îlots, {} coutures, distorsion {:.3}, {:.0} ms{}",
                s.charts,
                s.seams,
                s.mean_distortion,
                s.time_ms,
                    if s.overflow { " — ATTENTION : la densité cible dépasse la tuile 0-1" } else { "" }
                )
            };
            // la scène a pu changer pendant le calcul (objet supprimé, ajouté…)
            let targets = std::mem::take(&mut self.job_targets);
            let fits = self.scene.as_ref().is_some_and(|s| s.fits_partial(&targets, &r));
            if fits {
                self.snapshot();
                let ts = self.params.texture_size;
                if let Some(scene) = &mut self.scene {
                    // la sélection d'îlots survit au rangement (mêmes îlots, autre place)
                    let sel = std::mem::take(&mut scene.island_selected);
                    if targets.len() == scene.len() {
                        scene.set_uv(r, ts);
                    } else {
                        scene.merge_partial_uv(&targets, r, ts);
                    }
                    if self.job_layout && sel.len() == scene.island_selected.len() {
                        scene.island_selected = sel;
                        scene.touch();
                    }
                }
            } else {
                self.status = "La scène a changé pendant le dépliage : résultat ignoré, relancez U".into();
            }
            self.job = None;
            self.job_layout = false;
        }
    }

    // ---------- fichiers ----------

    fn open_file(&mut self, path: PathBuf, append: bool) {
        match io::import(&path) {
            Ok(m) => {
                if append {
                    self.append_model(m);
                } else {
                    let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    self.set_model(name, m);
                }
            }
            Err(e) => self.error = Some(format!("Impossible de lire {} : {e}", path.display())),
        }
    }

    fn pick_and_open(&mut self, append: bool) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Modèles 3D (OBJ, glTF, GLB, FBX)", io::IMPORT_EXT)
            .add_filter("Wavefront OBJ", &["obj"])
            .add_filter("glTF / GLB", &["gltf", "glb"])
            .add_filter("FBX", &["fbx"])
            .pick_file()
        {
            self.open_file(p, append);
        }
    }

    fn export_file(&mut self) {
        let Some(scene) = &self.scene else { return };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Wavefront OBJ", &["obj"])
            .add_filter("glTF binaire (GLB)", &["glb"])
            .add_filter("FBX (via Blender)", &["fbx"])
            .set_file_name(format!("{}_uv.obj", scene.name))
            .save_file()
        else {
            return;
        };
        match io::export(&path, &scene.model, scene.uv.as_ref().map(|u| u.corner_uv.as_slice())) {
            Ok(_) => self.status = format!("Exporté : {}", path.display()),
            Err(e) => self.error = Some(format!("Échec de l'export : {e}")),
        }
    }

    // ---------- mises à jour ----------

    fn check_updates(&mut self, ctx: &egui::Context, manual: bool) {
        if matches!(self.update, UpdateState::Checking(..) | UpdateState::Installing(_)) {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::check());
            ctx.request_repaint();
        });
        self.update = UpdateState::Checking(rx, manual);
    }

    fn poll_update(&mut self) {
        let next = match &self.update {
            UpdateState::Checking(rx, manual) => match rx.try_recv() {
                Ok(Ok(Some(rel))) => {
                    self.status = format!("Mise à jour {} disponible", rel.version);
                    Some(UpdateState::Available(rel))
                }
                Ok(Ok(None)) => Some(UpdateState::UpToDate),
                // hors ligne au démarrage : silencieux ; visible si demandé
                Ok(Err(e)) => Some(if *manual { UpdateState::Failed(e) } else { UpdateState::Idle }),
                Err(_) => None,
            },
            UpdateState::Installing(rx) => match rx.try_recv() {
                Ok(Ok(exe)) => update::restart(&exe),
                Ok(Err(e)) => {
                    self.show_update = true;
                    Some(UpdateState::Failed(e))
                }
                Err(_) => None,
            },
            _ => None,
        };
        if let Some(n) = next {
            self.update = n;
        }
    }

    fn install_update(&mut self, ctx: &egui::Context) {
        let UpdateState::Available(rel) = &self.update else { return };
        let rel = rel.clone();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::install(&rel));
            ctx.request_repaint();
        });
        self.update = UpdateState::Installing(rx);
    }

    fn update_window(&mut self, ctx: &egui::Context) {
        if !self.show_update {
            return;
        }
        let mut open = true;
        let (mut install, mut retry) = (false, false);
        egui::Window::new("Mises à jour")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_min_width(380.0);
                ui.label(format!("Version installée : {}", update::current_version()));
                ui.add_space(4.0);
                match &self.update {
                    UpdateState::Available(rel) => {
                        ui.label(RichText::new(format!("Nouvelle version : {}", rel.version)).color(theme::ACCENT_ORANGE).strong());
                        if !rel.notes.trim().is_empty() {
                            egui::Frame::new().fill(theme::FIELD).corner_radius(4).inner_margin(6).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                                    ui.label(rel.notes.trim());
                                });
                            });
                        }
                        ui.add_space(6.0);
                        install = primary_button(ui, "Installer et redémarrer").clicked();
                        ui.hyperlink_to("Voir la release sur GitHub", &rel.page);
                    }
                    UpdateState::Installing(_) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Téléchargement et vérification SHA-256…");
                        });
                    }
                    UpdateState::Checking(..) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Vérification…");
                        });
                    }
                    UpdateState::UpToDate => {
                        ui.label("EasyUV est à jour.");
                    }
                    UpdateState::Failed(e) => {
                        ui.label(RichText::new(e).color(theme::ACCENT_ORANGE));
                        retry = ui.button("Réessayer").clicked();
                    }
                    UpdateState::Idle => {
                        retry = ui.button("Vérifier maintenant").clicked();
                    }
                }
            });
        if install {
            self.install_update(ctx);
        }
        if retry {
            self.check_updates(ctx, true);
        }
        if !open {
            self.show_update = false;
        }
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        if !self.texture_dirty && self.texture.is_some() {
            return;
        }
        let mut rgba = generate_rgba8(TEX_SIZE as u32, self.checker_cells, self.checker_style);
        // 2x2 texels blancs au coin : couleur unie pour les éléments non texturés
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let i = (y * TEX_SIZE + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
        let img = egui::ColorImage::from_rgba_unmultiplied([TEX_SIZE, TEX_SIZE], &rgba);
        let opts = TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..TextureOptions::LINEAR_REPEAT };
        self.texture = Some(ctx.load_texture("checker", img, opts));
        self.mips = Mips::new(TEX_SIZE, &rgba);
        self.render.mips_gen += 1;
        self.texture_dirty = false;
    }

    // ---------- objets : sélection, suppression, annulation ----------

    /// Mémorise l'état courant avant une modification (Ctrl+Z).
    fn snapshot(&mut self) {
        if let Some(s) = &self.scene {
            self.undo.push(s.clone());
            if self.undo.len() > UNDO_LEVELS {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            if let Some(cur) = self.scene.take() {
                self.redo.push(cur);
            }
            self.scene = Some(prev);
            self.touch_scene();
            self.status = "Annuler".into();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            if let Some(cur) = self.scene.take() {
                self.undo.push(cur);
            }
            self.scene = Some(next);
            self.touch_scene();
            self.status = "Rétablir".into();
        }
    }

    /// Force le rendu après un échange de scène (la génération seule peut coïncider).
    fn touch_scene(&mut self) {
        self.render.invalidate();
        if let Some(s) = &mut self.scene {
            s.touch();
        }
    }

    fn delete_selected(&mut self) {
        let ts = self.params.texture_size;
        let n = self.scene.as_ref().map_or(0, |s| s.selection().len());
        if n == 0 {
            self.status = "Rien de sélectionné".into();
            return;
        }
        self.snapshot();
        let s = self.scene.as_mut().unwrap();
        s.delete_selected(ts);
        self.status = format!("{n} objet(s) supprimé(s) — Ctrl+Z pour annuler");
        if s.len() == 0 {
            self.scene = None;
        }
        self.renaming = None;
    }

    fn duplicate_selected(&mut self) {
        let ts = self.params.texture_size;
        if !self.scene.as_ref().is_some_and(|s| s.has_selection()) {
            return;
        }
        self.snapshot();
        let n = self.scene.as_mut().unwrap().duplicate_selected(ts);
        self.status = format!("{n} objet(s) dupliqué(s)");
    }

    fn hide_selected(&mut self) {
        if !self.scene.as_ref().is_some_and(|s| s.has_selection()) {
            return;
        }
        self.snapshot();
        let n = self.scene.as_mut().unwrap().hide_selected();
        self.status = format!("{n} objet(s) masqué(s) — Alt+H pour tout afficher");
    }

    fn reveal_all(&mut self) {
        if !self.scene.as_ref().is_some_and(|s| s.hidden.iter().any(|&h| h)) {
            return;
        }
        self.snapshot();
        self.scene.as_mut().unwrap().reveal_all();
    }

    fn frame_selection(&mut self) {
        if let Some(s) = &self.scene {
            let (lo, hi) = s.focus_bounds();
            self.cam.frame(lo, hi);
        }
        self.uv_cam = UvCamera::default();
    }

    /// Opérations d'objet communes au menu Objet, au clic droit et à l'outliner.
    fn object_menu_items(&mut self, ui: &mut egui::Ui) {
        let has_sel = self.scene.as_ref().is_some_and(|s| s.has_selection());
        let has_hidden = self.scene.as_ref().is_some_and(|s| s.hidden.iter().any(|&h| h));
        let item = |ui: &mut egui::Ui, enabled: bool, label: &str, key: &str| {
            ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(key)).clicked()
        };
        if item(ui, self.scene.is_some(), "Tout sélectionner", "A") {
            ui.close();
            if let Some(s) = &mut self.scene {
                s.select_all(true);
            }
        }
        if item(ui, has_sel, "Tout désélectionner", "Alt+A") {
            ui.close();
            if let Some(s) = &mut self.scene {
                s.select_all(false);
            }
        }
        if item(ui, self.scene.is_some(), "Inverser la sélection", "Ctrl+Maj+I") {
            ui.close();
            if let Some(s) = &mut self.scene {
                s.invert_selection();
            }
        }
        ui.separator();
        if item(ui, has_sel, "Dupliquer", "Maj+D") {
            ui.close();
            self.duplicate_selected();
        }
        if item(ui, has_sel, "Renommer l'actif", "F2") {
            ui.close();
            if let Some(s) = &self.scene {
                self.renaming = Some((s.active, s.model.objects[s.active].name.clone()));
            }
        }
        if item(ui, has_sel, "Masquer la sélection", "H") {
            ui.close();
            self.hide_selected();
        }
        if item(ui, has_hidden, "Tout afficher", "Alt+H") {
            ui.close();
            self.reveal_all();
        }
        if item(ui, has_sel, "Cadrer la sélection", "F") {
            ui.close();
            self.frame_selection();
        }
        if item(ui, self.scene.is_some(), "Projeter depuis la vue (UV)", "") {
            ui.close();
            self.project_view_op(false);
        }
        ui.menu_button("Pivoter de 90° (redresser)", |ui| {
            for (label, axis, pos) in [("X +90°", 0, true), ("X −90°", 0, false), ("Y +90°", 1, true), ("Y −90°", 1, false), ("Z +90°", 2, true), ("Z −90°", 2, false)] {
                if ui.button(label).clicked() {
                    ui.close();
                    self.rotate_objects(axis, pos);
                }
            }
        });
        ui.separator();
        if item(ui, has_sel, "Supprimer", "X / Suppr") {
            ui.close();
            self.delete_selected();
        }
    }

    fn rotate_objects(&mut self, axis: usize, positive: bool) {
        if self.scene.is_none() {
            return;
        }
        self.snapshot();
        let n = self.scene.as_mut().unwrap().rotate_objects_quarter(axis, positive);
        self.render.invalidate();
        self.status = format!("{n} objet(s) pivoté(s) de 90° — Ctrl+Z pour annuler");
    }

    /// Opérations UV communes au menu UV et au clic droit de l'éditeur UV.
    fn uv_menu_items(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let has_uv = self.scene.as_ref().is_some_and(|s| s.uv.is_some());
        let has_isl = self.scene.as_ref().is_some_and(|s| s.has_island_selection());
        let item = |ui: &mut egui::Ui, enabled: bool, label: &str, key: &str| ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(key)).clicked();
        if item(ui, self.scene.is_some(), "Dépliage intelligent", "U") {
            ui.close();
            self.start_unwrap(ctx);
        }
        if item(ui, has_uv, if has_isl { "Ranger les îlots sélectionnés" } else { "Ranger les îlots (layout)" }, "Ctrl+P") {
            ui.close();
            self.start_layout(ctx);
        }
        if item(ui, has_uv, "Appliquer la densité cible", "") {
            ui.close();
            self.apply_density();
        }
        ui.separator();
        if item(ui, has_uv, "Tout sélectionner (îlots)", "A") {
            ui.close();
            if let Some(s) = &mut self.scene {
                s.select_all_islands(true);
            }
        }
        if item(ui, has_isl, "Tout désélectionner", "Alt+A") {
            ui.close();
            if let Some(s) = &mut self.scene {
                s.select_all_islands(false);
            }
        }
        if item(ui, has_uv, "Inverser la sélection", "Ctrl+I") {
            ui.close();
            if let Some(s) = &mut self.scene {
                s.invert_islands();
            }
        }
        ui.separator();
        let ops: [(&str, fn([f32; 2]) -> [f32; 2]); 4] = [
            ("Pivoter de 90° (gauche)", |p| [-p[1], p[0]]),
            ("Pivoter de 90° (droite)", |p| [p[1], -p[0]]),
            ("Miroir horizontal", |p| [-p[0], p[1]]),
            ("Miroir vertical", |p| [p[0], -p[1]]),
        ];
        for (label, f) in ops {
            if item(ui, has_isl, label, "") {
                ui.close();
                self.island_op(|s, ts| s.per_island(f, ts), label);
            }
        }
        if item(ui, has_isl, "Rendre droit (bandes, arcs)", "Maj+W") {
            ui.close();
            self.straighten_strips();
        }
        if item(ui, self.scene.is_some(), "Projeter depuis la vue", "") {
            ui.close();
            self.project_view_op(false);
        }
        if item(ui, self.scene.is_some(), "Projeter depuis la vue (cadré)", "") {
            ui.close();
            self.project_view_op(true);
        }
        if item(ui, has_uv, "Grille parfaite", "Maj+G") {
            ui.close();
            self.snap_grid_op();
        }
        if item(ui, has_uv, "Remettre d'aplomb (d'après les arêtes)", "") {
            ui.close();
            self.orient_op();
        }
        if item(ui, has_uv, "Sélection par angle", "") {
            ui.close();
            self.select_by_angle();
        }
        if item(ui, has_isl, "Orienter sur les axes", "") {
            ui.close();
            self.island_op(|s, ts| s.straighten_islands(ts), "Orienter");
        }
        {
            use uvtools::{Align, Islands};
            ui.add_enabled_ui(has_isl, |ui| {
                ui.menu_button("Aligner", |ui| {
                    for (label, mode) in [("À gauche", Align::Left), ("Au centre (U)", Align::CenterU), ("À droite", Align::Right), ("En haut", Align::Top), ("Au centre (V)", Align::CenterV), ("En bas", Align::Bottom)] {
                        if ui.button(label).clicked() {
                            ui.close();
                            self.uv_tool_op("Aligner", move |m, uv, fi, ids| uvtools::align(uv, &Islands::new(m, fi, ids), mode));
                        }
                    }
                });
                ui.menu_button("Répartir", |ui| {
                    for (label, axis) in [("En U (horizontalement)", 0), ("En V (verticalement)", 1)] {
                        if ui.button(label).clicked() {
                            ui.close();
                            self.uv_tool_op("Répartir", move |m, uv, fi, ids| uvtools::distribute(uv, &Islands::new(m, fi, ids), axis));
                        }
                    }
                });
            });
            let it = self.optimize_iters;
            let tools: [(&str, &str, UvOp); 5] = [
                ("Empiler", "", Box::new(|m, uv, fi, ids| uvtools::stack(uv, &Islands::new(m, fi, ids)))),
                ("Égaliser la densité", "", Box::new(|m, uv, fi, ids| uvtools::equalize_density(m, uv, &Islands::new(m, fi, ids)))),
                ("Remplir la tuile", "", Box::new(|m, uv, fi, ids| uvtools::fit_to_tile(uv, &Islands::new(m, fi, ids)))),
                ("Optimiser (ARAP)", "", Box::new(move |m, uv, fi, ids| uvtools::optimize(m, uv, &Islands::new(m, fi, ids), it))),
                ("Redéplier", "", Box::new(|m, uv, fi, ids| uvtools::reunwrap(m, uv, &Islands::new(m, fi, ids)))),
            ];
            for (label, key, op) in tools {
                if item(ui, has_isl, label, key) {
                    ui.close();
                    self.uv_tool_op(label, op);
                }
            }
        }
        ui.separator();
        if item(ui, has_isl, "Déplier la sélection UV", "U") {
            ui.close();
            self.unwrap_uv_selection();
        }
        let has_edges = self.scene.as_ref().is_some_and(|s| !s.selected_mesh_edges().is_empty());
        for (label, op) in [("Couper (arêtes)", SeamOp::Cut), ("Coudre (arêtes)", SeamOp::Sew), ("Marquer la couture", SeamOp::Mark(true)), ("Retirer la marque", SeamOp::Mark(false))] {
            if item(ui, has_edges, label, "") {
                ui.close();
                self.seam_op(op);
            }
        }
        ui.menu_button("Mode de sélection", |ui| {
            let mode = self.scene.as_ref().map_or(SelectMode::Island, |s| s.sel_mode);
            for m in SelectMode::ALL {
                if ui.add(egui::Button::selectable(mode == m, m.label()).shortcut_text(m.key())).clicked() {
                    ui.close();
                    if let Some(s) = &mut self.scene {
                        s.set_select_mode(m);
                    }
                }
            }
        });
        ui.menu_button("Outil actif", |ui| {
            for t in UvTool::ALL {
                if ui.add(egui::Button::selectable(self.uv_edit.tool == t, t.label()).shortcut_text(t.key())).clicked() {
                    ui.close();
                    self.uv_edit.tool = t;
                }
            }
        });
        ui.label(RichText::new("Déplacer G · Pivoter R · Échelle S · X / Y : axe").small().color(theme::TEXT_DIM));
    }

    /// Rend droites les bandes courbes sélectionnées (grille de quads régulière).
    fn straighten_strips(&mut self) {
        if !self.scene.as_ref().is_some_and(|s| s.has_island_selection()) {
            self.status = "Sélectionnez d'abord les îlots à rendre droits".into();
            return;
        }
        self.snapshot();
        let ts = self.params.texture_size;
        let (done, skipped) = self.scene.as_mut().unwrap().straighten_strips(ts);
        if done == 0 {
            self.undo.pop();
        }
        self.status = match (done, skipped) {
            (0, _) => "Rien à rendre droit : il faut une bande, pas un couvercle. Astuce : en mode sommets (1), choisissez les 4 coins de la bande puis Maj+W".into(),
            (d, 0) => format!("{d} îlot(s) rendu(s) droit(s) — Ctrl+P pour ranger"),
            (d, k) => format!("{d} îlot(s) rendu(s) droit(s), {k} ignoré(s) : pas une bande, ou le redressement l'aurait abîmé"),
        };
    }

    /// Outil d'îlots de `easyuv::uvtools` sur la sélection, annulable.
    fn uv_tool_op(&mut self, label: &str, op: impl FnOnce(&easyuv::mesh::Mesh, &mut [[f32; 2]], &[u32], &[usize])) {
        if !self.scene.as_ref().is_some_and(|s| s.has_island_selection()) {
            self.status = format!("{label} : sélectionnez d'abord des îlots (clic, A ou rectangle)");
            return;
        }
        self.snapshot();
        let ts = self.params.texture_size;
        let n = self.scene.as_mut().unwrap().island_tool(ts, op);
        self.status = format!("{label} : {n} îlot(s) — Ctrl+Z pour annuler");
    }

    /// Dépliage intelligent des seules faces sélectionnées dans l'éditeur UV (annulable).
    fn unwrap_uv_selection(&mut self) {
        if self.job.is_some() || self.scene.is_none() {
            return;
        }
        self.snapshot();
        let params = self.effective_params();
        let ts = self.params.texture_size;
        match self.scene.as_mut().unwrap().unwrap_selection(&params, ts) {
            Ok(msg) => self.status = format!("{msg} — Ctrl+Z pour annuler"),
            Err(e) => {
                self.undo.pop();
                self.status = e;
            }
        }
    }

    /// Outils de couture sur les arêtes sélectionnées (annulables).
    fn seam_op(&mut self, op: SeamOp) {
        if self.scene.is_none() {
            return;
        }
        self.snapshot();
        let params = self.effective_params();
        let ts = self.params.texture_size;
        let s = self.scene.as_mut().unwrap();
        let res = match op {
            SeamOp::Cut => s.cut_selected(&params, ts),
            SeamOp::Sew => s.sew_selected(ts),
            SeamOp::Mark(on) => match s.mark_selected_seams(on) {
                0 => Err("Sélectionnez d'abord des arêtes (mode Arêtes, touche 2 ; double-clic : boucle)".into()),
                n if on => Ok(format!("{n} arête(s) marquée(s) comme couture : le prochain dépliage les coupera")),
                n => Ok(format!("Marque de couture retirée sur {n} arête(s)")),
            },
            SeamOp::ClearAll => {
                let n = s.marked_seams.len();
                s.marked_seams.clear();
                s.touch();
                Ok(format!("{n} marque(s) de couture effacée(s)"))
            }
        };
        match res {
            Ok(m) => self.status = m,
            Err(e) => {
                self.undo.pop();
                self.status = e;
            }
        }
    }

    /// Projection depuis la vue 3D (comme « Project From View » de Blender), annulable.
    fn project_view_op(&mut self, fit: bool) {
        if self.scene.is_none() || self.job.is_some() {
            return;
        }
        self.snapshot();
        let (right, up) = self.cam.view_axes();
        let params = self.effective_params();
        let ts = self.params.texture_size;
        let n = self.scene.as_mut().unwrap().project_from_view(right, up, &params, ts, fit);
        if n == 0 {
            self.undo.pop();
            self.status = "Projeter depuis la vue : rien à projeter".into();
            return;
        }
        self.status = format!(
            "{n} face(s) projetée(s) depuis la vue{} — Ctrl+Z pour annuler",
            if fit { ", étirées sur la tuile" } else { ", rangées dans la place libre" }
        );
    }

    /// Place de l'en-tête de la vue 3D : (titre affiché, bouton Objet / Édition à x0, en version
    /// compacte ou non). Le bouton de mode reste visible tant qu'il tient, quitte à masquer le titre.
    fn view3d_header_layout(&self, ui: &egui::Ui, r: egui::Rect, title: &str) -> (bool, Option<(f32, bool)>) {
        // les boutons d'affichage occupent la droite de l'en-tête (~390 px)
        let room = r.right() - 390.0;
        let full = if self.edit_mode { 290.0 } else { 190.0 };
        // juste après le titre, quelle que soit sa longueur (« Droite · Orthographique »…)
        let tw = ui.painter().layout_no_wrap(title.to_string(), egui::TextStyle::Body.resolve(ui.style()), theme::TEXT_DIM).size().x;
        let after_title = r.left() + 8.0 + tw + 14.0;
        let left = r.left() + 8.0;
        if after_title + full <= room {
            (true, Some((after_title, false)))
        } else if left + full <= room {
            (false, Some((left, false)))
        } else if left + 70.0 <= room {
            (false, Some((left, true)))
        } else {
            (r.width() > 540.0, None) // très étroit : Tab seulement
        }
    }

    /// En-tête de la vue 3D : Objet / Édition, puis modes de sélection et projection (sauf compact).
    fn view3d_header_extra(&mut self, ui: &mut egui::Ui, r: egui::Rect, x0: f32, compact: bool) {
        let at = egui::Rect::from_min_size(egui::pos2(x0, r.top() + 2.0), egui::vec2(300.0, 22.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let label = match (self.edit_mode, compact) {
                    (true, true) => "Édition",
                    (false, true) => "Objet",
                    (true, false) => "Édition  (Tab)",
                    (false, false) => "Objet  (Tab : Édition)",
                };
                let txt = if self.edit_mode { RichText::new(label).small().color(Color32::BLACK).strong() } else { RichText::new(label).small() };
                let b = egui::Button::new(txt).fill(if self.edit_mode { theme::ACCENT_ORANGE } else { theme::WIDGET });
                if ui.add(b).on_hover_text("Tab : mode Objet (un clic choisit un objet) ou Édition des objets sélectionnés (un clic choisit un de leurs sommets, arêtes ou faces, comme dans Blender). Choisir les modes sommets, arêtes ou faces passe d'office en Édition.").clicked() {
                    self.toggle_edit_mode();
                }
                if compact {
                    return;
                }
                if self.edit_mode {
                    let mode = self.scene.as_ref().map_or(SelectMode::Island, |s| s.sel_mode);
                    for m in SelectMode::ALL {
                        if icons::button(ui, mode_icon(m), mode == m, &format!("Sélection : {}", m.label()), m.key(), EDIT_HELP).clicked() {
                            if let Some(s) = &mut self.scene {
                                s.set_select_mode(m);
                            }
                        }
                    }
                }
                ui.add_space(4.0);
                if icons::button(ui, Icon::Project, false, "Projeter depuis la vue", "", PROJECT_HELP).clicked() {
                    self.project_view_op(false);
                }
            });
        });
    }

    /// Passe en Édition les objets sélectionnés (comme Tab dans Blender), ou revient au mode
    /// Objet. Renvoie false quand aucun objet n'est sélectionné (on reste en mode Objet).
    fn set_edit_mode(&mut self, on: bool) -> bool {
        let Some(s) = &mut self.scene else {
            self.edit_mode = false;
            return !on;
        };
        if !on {
            s.exit_edit();
            self.edit_mode = false;
            return true;
        }
        // un seul objet visible : il passe en Édition sans avoir à le choisir
        if !s.selection().iter().any(|&i| !s.hidden[i]) {
            let vis: Vec<usize> = (0..s.len()).filter(|&i| !s.hidden[i]).collect();
            if vis.len() == 1 {
                s.click(Some(vis[0]), false);
            }
        }
        self.edit_mode = s.enter_edit() > 0;
        self.edit_mode
    }

    fn toggle_edit_mode(&mut self) {
        let on = !self.edit_mode;
        self.status = if !self.set_edit_mode(on) {
            "Mode Édition : sélectionnez d'abord un objet (clic en mode Objet), puis Tab".into()
        } else if on {
            let n = self.scene.as_ref().map_or(0, |s| s.editing.iter().filter(|&&e| e).count());
            format!("Mode Édition ({n} objet(s)) : clic sur un sommet, une arête ou une face (1 2 3), Alt+clic : boucle, Maj : ajouter, Tab : mode Objet")
        } else {
            "Mode Objet".into()
        };
    }

    /// Touches de la vue 3D (pointeur dessus), comme Blender.
    fn view3d_keys(&mut self, ui: &egui::Ui, r: egui::Rect) {
        let k = |key: egui::Key| ui.input(|i| i.key_pressed(key));
        let (ctrl, shift, alt) = ui.input(|i| (i.modifiers.command, i.modifiers.shift, i.modifiers.alt));
        if k(egui::Key::Tab) {
            self.toggle_edit_mode();
        }
        // vues d'axe : 3 droite, 7 dessus (Ctrl : en face), 5 ortho ; 1 face en mode Objet
        // (en Édition, 1 à 4 choisissent le mode de sélection)
        let views: &[(egui::Key, usize, bool)] = if self.edit_mode {
            &[(egui::Key::Num7, 2, true)]
        } else {
            &[(egui::Key::Num1, 1, false), (egui::Key::Num3, 0, true), (egui::Key::Num7, 2, true)]
        };
        for &(key, axis, positive) in views {
            if k(key) {
                self.cam.set_axis_view(viewport::AxisView::from_axis(axis, positive != ctrl));
            }
        }
        if k(egui::Key::Num5) {
            self.cam.toggle_ortho();
        }
        if !self.edit_mode {
            return;
        }
        let Some(s) = &mut self.scene else { return };
        for (key, m) in [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4].into_iter().zip(SelectMode::ALL) {
            if !ctrl && k(key) {
                s.set_select_mode(m);
            }
        }
        if k(egui::Key::A) && !ctrl {
            s.select_all_uv(!alt);
        }
        if ctrl && k(egui::Key::I) {
            s.invert_uv();
        }
        if ctrl && k(egui::Key::L) {
            let n = s.select_linked_3d();
            self.status = format!("Sélection liée : {n} face(s)");
        } else if k(egui::Key::L) {
            // L : la partie reliée sous le pointeur
            if let Some(f) = ui.input(|i| i.pointer.hover_pos()).and_then(|p| self.render.pick_face(p)).filter(|&f| s.object_editable(s.face_object[f] as usize)) {
                if !shift {
                    s.select_all_uv(false);
                }
                s.model.mesh.face_range(f).for_each(|c| s.corner_sel[c] = true);
                let n = s.select_linked_3d();
                self.status = format!("Partie reliée : {n} face(s)");
            }
        }
        if ctrl && (k(egui::Key::Plus) || k(egui::Key::Equals)) {
            s.grow_shrink(true);
        }
        if ctrl && k(egui::Key::Minus) {
            s.grow_shrink(false);
        }
        if ctrl && k(egui::Key::E) {
            self.seam_op(SeamOp::Mark(!shift));
        }
        let _ = r;
    }

    /// Clic dans la vue 3D en mode Édition : sommet, arête, face ou îlot ; Alt : boucle.
    fn edit_click(&mut self, ui: &egui::Ui, resp: &egui::Response, r: egui::Rect) {
        let Some(p) = resp.interact_pointer_pos() else { return };
        let (shift, alt) = ui.input(|i| (i.modifiers.shift || i.modifiers.command, i.modifiers.alt));
        let Some(s) = &mut self.scene else { return };
        // seules les faces des objets en Édition se choisissent
        let f = self.render.pick_face(p).filter(|&f| s.object_editable(s.face_object[f] as usize));
        // positions écran des sommets de la face touchée (seuls interrogés)
        let screen: std::collections::HashMap<u32, [f32; 2]> = f
            .map(|f| s.model.mesh.face(f).iter().filter_map(|&v| viewport::screen_pos(&self.cam, r, s.model.mesh.positions[v as usize]).map(|q| (v, [q.x, q.y]))).collect())
            .unwrap_or_default();
        let at = |v: u32| screen.get(&v).copied();
        match (alt, f) {
            (true, Some(f)) => {
                let n = s.loop_3d(f, [p.x, p.y], at, shift);
                self.status = format!("Boucle de {n} arête(s) — Ctrl+E pour en faire une couture");
            }
            _ => s.click_3d(f, [p.x, p.y], at, shift),
        }
    }

    /// Grille parfaite (Maj+G) sur la sélection, ou sur tous les îlots.
    fn snap_grid_op(&mut self) {
        if self.scene.as_ref().is_none_or(|s| s.uv.is_none()) {
            return;
        }
        self.snapshot();
        let ts = self.params.texture_size;
        let (n, total) = self.scene.as_mut().unwrap().snap_grid(ts);
        if n == 0 {
            self.undo.pop();
        }
        self.status = match (n, total) {
            (0, _) => "Grille parfaite : rien à aligner sans abîmer de faces (îlot sans rangées ni colonnes, comme un couvercle)".into(),
            (n, t) if n == t => format!("Grille parfaite : {n} îlot(s), rangées et colonnes droites — Ctrl+Z pour annuler"),
            (n, t) => format!("Grille parfaite : {n} îlot(s) sur {t} ; les autres n'ont pas de grille à aligner"),
        };
    }

    /// Remet d'aplomb la sélection (ou tous les îlots) d'après leurs arêtes.
    fn orient_op(&mut self) {
        if self.scene.as_ref().is_none_or(|s| s.uv.is_none()) {
            return;
        }
        self.snapshot();
        let ts = self.params.texture_size;
        let n = self.scene.as_mut().unwrap().orient_to_edges(ts);
        self.status = format!("Remis d'aplomb : {n} îlot(s), arêtes à l'horizontale et à la verticale — Ctrl+P pour ranger");
    }

    /// Sélection par angle : arêtes vives en mode arêtes, sinon faces liées depuis la sélection.
    fn select_by_angle(&mut self) {
        let a = self.sel_angle;
        let Some(s) = self.scene.as_mut().filter(|s| s.uv.is_some()) else { return };
        self.status = if s.sel_mode == SelectMode::Edge {
            let n = s.select_sharp_edges(a, false);
            format!("{n} arête(s) vive(s) au-delà de {a:.0}° — Couper ou Marquer pour en faire des coutures")
        } else {
            match s.select_linked_by_angle(a) {
                0 => "Par angle : choisissez d'abord une face (mode faces, 3), ou passez en mode arêtes (2) pour les arêtes vives".into(),
                n => format!("{n} face(s) liées à moins de {a:.0}° — U pour ne déplier qu'elles"),
            }
        };
    }

    /// Barre d'icônes de l'éditeur UV : modes de sélection, outils, score en direct.
    fn uv_header_bar(&mut self, ui: &mut egui::Ui, header: egui::Rect) {
        let r = egui::Rect::from_min_max(egui::pos2(header.right() - 500.0, header.top() + 2.0), egui::pos2(header.right() - 6.0, header.bottom() - 2.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                self.score_badge(ui);
                ui.add_space(6.0);
                for t in UvTool::ALL.iter().rev() {
                    if icons::button(ui, tool_icon(*t), self.uv_edit.tool == *t, t.label(), t.key(), t.hint()).clicked() {
                        self.uv_edit.tool = *t;
                    }
                }
                ui.add_space(6.0);
                for sh in SelShape::ALL.iter().rev() {
                    if icons::button(ui, shape_icon(*sh), self.uv_edit.sel_shape == *sh, sh.label(), "W", sh.help()).clicked() {
                        self.uv_edit.sel_shape = *sh;
                        self.uv_edit.tool = UvTool::Select;
                    }
                }
                ui.add_space(6.0);
                let mode = self.scene.as_ref().map_or(SelectMode::Island, |s| s.sel_mode);
                for m in SelectMode::ALL.iter().rev() {
                    if icons::button(ui, mode_icon(*m), mode == *m, &format!("Sélection : {}", m.label()), m.key(), mode_help(*m)).clicked() {
                        if let Some(s) = &mut self.scene {
                            s.set_select_mode(*m);
                        }
                    }
                }
            });
        });
    }

    /// Score UV en direct (recalculé quand les UV changent, au plus 5 fois par seconde).
    fn update_score(&mut self, ctx: &egui::Context) {
        let Some(s) = &self.scene else {
            self.score = None;
            return;
        };
        let Some(uv) = &s.uv else {
            self.score = None;
            return;
        };
        let now = ctx.input(|i| i.time);
        if self.score_gen == Some(s.generation) || now - self.score_time < 0.2 {
            if self.score_gen != Some(s.generation) {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            return;
        }
        let res = if s.model.mesh.face_count() > 40_000 { 128 } else { 256 };
        self.score = Some(easyuv::metrics::uv_score(&s.model.mesh, &uv.corner_uv, &s.face_island, uv.stats.seam_length, res));
        self.score_gen = Some(s.generation);
        self.score_time = now;
    }

    fn score_badge(&mut self, ui: &mut egui::Ui) {
        let Some(sc) = &self.score else { return };
        let col = score_color(sc.total);
        let txt = RichText::new(format!("Score {:.0}", sc.total)).strong().color(Color32::BLACK);
        let resp = ui.add(egui::Button::new(txt).fill(col).min_size(egui::vec2(74.0, 20.0)));
        let sc = sc.clone();
        resp.on_hover_ui(|ui| score_details(ui, &sc));
    }

    /// Panneau et menus : outils d'îlots façon Maya UV Toolkit / RizomUV.
    fn uv_toolkit(&mut self, ui: &mut egui::Ui) {
        use uvtools::{Align, Islands};
        let has_isl = self.scene.as_ref().is_some_and(|s| s.has_island_selection());
        ui.label(RichText::new("Sélection (1 à 4) et outils (Q, Maj+1 à Maj+5) — survolez une icône pour son aperçu").small().color(theme::TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            let mode = self.scene.as_ref().map_or(SelectMode::Island, |s| s.sel_mode);
            for m in SelectMode::ALL {
                if icons::button(ui, mode_icon(m), mode == m, &format!("Sélection : {}", m.label()), m.key(), mode_help(m)).clicked() {
                    if let Some(s) = &mut self.scene {
                        s.set_select_mode(m);
                    }
                }
            }
            ui.add_space(8.0);
            for sh in SelShape::ALL {
                if icons::button(ui, shape_icon(sh), self.uv_edit.sel_shape == sh, sh.label(), "W", sh.help()).clicked() {
                    self.uv_edit.sel_shape = sh;
                    self.uv_edit.tool = UvTool::Select;
                }
            }
            ui.add_space(8.0);
            for t in UvTool::ALL {
                if icons::button(ui, tool_icon(t), self.uv_edit.tool == t, t.label(), t.key(), t.hint()).clicked() {
                    self.uv_edit.tool = t;
                }
            }
        });
        ui.horizontal(|ui| {
            if icons::button(ui, Icon::Angle, false, "Sélection par angle", "", "Mode arêtes : les arêtes vives, dont les faces font un angle au-delà du seuil (pour Couper ou Marquer). Autres modes : depuis les faces choisies, toutes les faces voisines à moins du seuil (un même pan).").clicked() {
                self.select_by_angle();
            }
            ui.label("Par angle");
            ui.add(egui::DragValue::new(&mut self.sel_angle).range(1.0..=179.0).speed(0.5).suffix("°"));
            ui.add_space(8.0);
            if icons::button(ui, Icon::Grid, false, "Grille parfaite", "Maj+G", "Chaque rangée et chaque colonne passe sur une droite, même avec triangles et jonctions en T. Sur la sélection, ou sur tous les îlots. Appliquée aussi à la fin de Rendre droit.").clicked() {
                self.snap_grid_op();
            }
            if icons::button(ui, Icon::Orient, false, "Remettre d'aplomb", "", "Chaque îlot tourne pour que ses arêtes suivent l'horizontale et la verticale (d'après ses arêtes, pas sa boîte). Sur la sélection, ou sur tous les îlots.").clicked() {
                self.orient_op();
            }
        });
        ui.label(RichText::new(format!("{} : {}", self.uv_edit.tool.label(), self.uv_edit.tool.hint())).small());
        // coutures et dépliage de la sélection
        ui.horizontal_wrapped(|ui| {
            let edges = self.scene.as_ref().map_or(0, |s| s.selected_mesh_edges().len());
            ui.label(RichText::new(format!("Coutures ({edges} arête(s))")).color(theme::TEXT_DIM));
            let items: [(Icon, &str, &str, SeamOp); 4] = [
                (Icon::Cut, "Couper", "Les arêtes sélectionnées deviennent des coutures et les îlots traversés sont redépliés.", SeamOp::Cut),
                (Icon::Sew, "Coudre", "Rapproche l'îlot voisin, soude les arêtes sélectionnées puis détend l'îlot recousu (Move and Sew de Maya).", SeamOp::Sew),
                (Icon::Mark, "Marquer la couture", "Les arêtes sélectionnées seront coupées à chaque dépliage (U), comme Mark Seam de Blender.", SeamOp::Mark(true)),
                (Icon::Edge, "Retirer la marque", "Les arêtes sélectionnées ne sont plus des coutures imposées.", SeamOp::Mark(false)),
            ];
            for (icon, title, help, op) in items {
                if icons::button(ui, icon, false, title, "", help).clicked() {
                    self.seam_op(op);
                }
            }
            let marked = self.scene.as_ref().map_or(0, |s| s.marked_seams.len());
            if marked > 0 && ui.small_button(format!("Effacer les {marked} marques")).clicked() {
                self.seam_op(SeamOp::ClearAll);
            }
            if icons::button(ui, Icon::Unwrap, false, "Déplier la sélection UV", "U", "Dépliage intelligent des seules faces sélectionnées ; le reste ne bouge pas.").clicked() {
                self.unwrap_uv_selection();
            }
        });
        if self.uv_edit.tool.brush().is_some() {
            egui::Grid::new("brush").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Rayon");
                ui.add(egui::Slider::new(&mut self.uv_edit.radius, 4.0..=600.0).logarithmic(true).suffix(" px"));
                ui.end_row();
                ui.label("Force");
                ui.add(egui::Slider::new(&mut self.uv_edit.strength, 0.05..=1.0).fixed_decimals(2));
                ui.end_row();
            });
        }
        ui.label(RichText::new(self.uv_edit.tool.hint()).small().color(theme::TEXT_DIM));
        ui.add_space(4.0);
        ui.add_enabled_ui(has_isl, |ui| {
            egui::Grid::new("toolkit").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Aligner");
                ui.horizontal_wrapped(|ui| {
                    for (label, mode) in [("Gauche", Align::Left), ("Centre U", Align::CenterU), ("Droite", Align::Right), ("Haut", Align::Top), ("Centre V", Align::CenterV), ("Bas", Align::Bottom)] {
                        if ui.small_button(label).clicked() {
                            self.uv_tool_op("Aligner", move |m, uv, fi, ids| uvtools::align(uv, &Islands::new(m, fi, ids), mode));
                        }
                    }
                });
                ui.end_row();
                ui.label("Répartir");
                ui.horizontal(|ui| {
                    for (label, axis) in [("En U", 0), ("En V", 1)] {
                        if ui.small_button(label).on_hover_text("Écarts égaux entre les îlots (au moins 3)").clicked() {
                            self.uv_tool_op("Répartir", move |m, uv, fi, ids| uvtools::distribute(uv, &Islands::new(m, fi, ids), axis));
                        }
                    }
                });
                ui.end_row();
                ui.label("Pivoter");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.rot_angle).range(-360.0..=360.0).speed(1.0).suffix("°"));
                    if ui.small_button("Appliquer").clicked() {
                        let (s, c) = (self.rot_angle as f32).to_radians().sin_cos();
                        self.island_op(move |sc, ts| sc.per_island(move |p| [c * p[0] - s * p[1], s * p[0] + c * p[1]], ts), "Pivoter");
                    }
                    for (label, a) in [("-90", -90f32), ("+90", 90.0), ("180", 180.0)] {
                        if ui.small_button(label).clicked() {
                            let (s, c) = a.to_radians().sin_cos();
                            self.island_op(move |sc, ts| sc.per_island(move |p| [c * p[0] - s * p[1], s * p[0] + c * p[1]], ts), "Pivoter");
                        }
                    }
                });
                ui.end_row();
                ui.label("Miroir / échelle");
                ui.horizontal(|ui| {
                    if ui.small_button("Miroir U").clicked() {
                        self.island_op(|s, ts| s.per_island(|p| [-p[0], p[1]], ts), "Miroir");
                    }
                    if ui.small_button("Miroir V").clicked() {
                        self.island_op(|s, ts| s.per_island(|p| [p[0], -p[1]], ts), "Miroir");
                    }
                    if ui.small_button("×2").clicked() {
                        self.island_op(|s, ts| s.per_island(|p| [p[0] * 2.0, p[1] * 2.0], ts), "Échelle");
                    }
                    if ui.small_button("×½").clicked() {
                        self.island_op(|s, ts| s.per_island(|p| [p[0] * 0.5, p[1] * 0.5], ts), "Échelle");
                    }
                });
                ui.end_row();
                ui.label("Îlots");
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button("Empiler").on_hover_text("Superpose les îlots sur le premier (zones de texture partagées)").clicked() {
                        self.uv_tool_op("Empiler", |m, uv, fi, ids| uvtools::stack(uv, &Islands::new(m, fi, ids)));
                    }
                    if ui.small_button("Égaliser la densité").on_hover_text("Même densité de texels pour tous les îlots sélectionnés").clicked() {
                        self.uv_tool_op("Égaliser la densité", |m, uv, fi, ids| uvtools::equalize_density(m, uv, &Islands::new(m, fi, ids)));
                    }
                    if ui.small_button("Remplir la tuile").on_hover_text("La sélection tient dans la tuile 0-1, proportions gardées").clicked() {
                        self.uv_tool_op("Remplir la tuile", |m, uv, fi, ids| uvtools::fit_to_tile(uv, &Islands::new(m, fi, ids)));
                    }
                });
                ui.end_row();
                ui.label("Forme");
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button("Rendre droit").on_hover_text("Bande courbe -> rectangle droit. Maj+W").clicked() {
                        self.straighten_strips();
                    }
                    if ui.small_button("Orienter").on_hover_text("Pivote chaque îlot sur sa boîte minimale").clicked() {
                        self.island_op(|s, ts| s.straighten_islands(ts), "Orienter");
                    }
                    let it = self.optimize_iters;
                    if ui.small_button("Optimiser").on_hover_text("Relaxation ARAP : moins d'étirement, même place et même orientation (Optimize de RizomUV)").clicked() {
                        self.uv_tool_op("Optimiser", move |m, uv, fi, ids| uvtools::optimize(m, uv, &Islands::new(m, fi, ids), it));
                    }
                    if ui.small_button("Redéplier").on_hover_text("Repart de la forme 3D (LSCM + ARAP) : efface les retouches de l'îlot").clicked() {
                        self.uv_tool_op("Redéplier", |m, uv, fi, ids| uvtools::reunwrap(m, uv, &Islands::new(m, fi, ids)));
                    }
                });
                ui.end_row();
                ui.label("Passes d'optimisation");
                ui.add(egui::Slider::new(&mut self.optimize_iters, 1..=50));
                ui.end_row();
            });
        });
        if !has_isl {
            ui.label(RichText::new("Sélectionnez des îlots dans l'éditeur UV pour les outils d'îlots.").small().color(theme::TEXT_DIM));
        }
    }

    /// Opération sur les îlots sélectionnés, annulable.
    fn island_op(&mut self, op: impl FnOnce(&mut Scene, u32) -> usize, label: &str) {
        if !self.scene.as_ref().is_some_and(|s| s.has_island_selection()) {
            return;
        }
        self.snapshot();
        let ts = self.params.texture_size;
        let n = op(self.scene.as_mut().unwrap(), ts);
        self.status = format!("{label} : {n} îlot(s)");
    }

    /// Raccourcis clavier de type Blender (ignorés pendant la saisie de texte).
    fn object_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (k_a, k_x, k_del, k_d, k_h, k_z, k_y, k_f2, k_inv, alt, shift, cmd, pointer) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::A),
                i.key_pressed(egui::Key::X),
                i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::D),
                i.key_pressed(egui::Key::H),
                i.key_pressed(egui::Key::Z),
                i.key_pressed(egui::Key::Y),
                i.key_pressed(egui::Key::F2),
                i.key_pressed(egui::Key::I),
                i.modifiers.alt,
                i.modifiers.shift,
                i.modifiers.command,
                i.pointer.hover_pos(),
            )
        });
        if self.uv_edit.busy() {
            return;
        }
        if cmd && k_z && !shift {
            self.undo();
        } else if (cmd && k_z && shift) || (cmd && k_y) {
            self.redo();
        }
        if cmd && k_inv && shift {
            if let Some(s) = &mut self.scene {
                s.invert_selection();
            }
        }
        if cmd || self.edit_mode {
            return;
        }
        // au-dessus de l'éditeur UV, les touches visent les îlots (comme dans Blender)
        if self.uv_edit.busy() || pointer.zip(self.uv_rect).is_some_and(|(p, r)| r.contains(p)) {
            return;
        }
        if k_a {
            if let Some(s) = &mut self.scene {
                s.select_all(!alt);
            }
        }
        if k_del {
            self.delete_selected();
        }
        if k_x && self.scene.as_ref().is_some_and(|s| s.has_selection()) {
            self.confirm_delete = pointer;
        }
        if k_d && shift {
            self.duplicate_selected();
        }
        if k_h {
            if alt {
                self.reveal_all();
            } else {
                self.hide_selected();
            }
        }
        if k_f2 {
            if let Some(s) = &self.scene {
                if s.has_selection() {
                    self.renaming = Some((s.active, s.model.objects[s.active].name.clone()));
                }
            }
        }
    }

    /// Menu de confirmation de X, comme le « Delete » de Blender.
    fn confirm_delete_popup(&mut self, ctx: &egui::Context) {
        let Some(pos) = self.confirm_delete else { return };
        let n = self.scene.as_ref().map_or(0, |s| s.selection().len());
        let mut close = false;
        let resp = egui::Area::new(egui::Id::unique("confirm_delete")).order(egui::Order::Foreground).fixed_pos(pos - egui::vec2(60.0, 14.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(format!("Supprimer {n} objet(s) ?")).color(theme::TEXT_DIM));
                if ui.add(egui::Button::new("Supprimer").min_size(egui::vec2(140.0, 22.0))).clicked() {
                    self.delete_selected();
                    close = true;
                }
            });
        });
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if close || esc || resp.response.clicked_elsewhere() {
            self.confirm_delete = None;
        }
    }

    // ---------- interface ----------

    fn top_bar(&mut self, root: &mut egui::Ui) {
        let ctx = &root.ctx().clone();
        egui::Panel::top("topbar")
            .frame(egui::Frame::new().fill(theme::BG_PANEL).inner_margin(egui::Margin::symmetric(8, 4)))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("EasyUV").color(theme::ACCENT_ORANGE).strong());
                    ui.add_space(8.0);
                    ui.menu_button("Fichier", |ui| {
                        if ui.button("Nouveau").clicked() {
                            ui.close();
                            self.scene = None;
                            self.job = None;
                        }
                        if ui.button("Ouvrir…   Ctrl+O").clicked() {
                            ui.close();
                            self.pick_and_open(false);
                        }
                        if ui.button("Importer dans la scène…   Ctrl+I").clicked() {
                            ui.close();
                            self.pick_and_open(true);
                        }
                        ui.separator();
                        if ui.button("Exporter (OBJ, GLB, FBX)…   Ctrl+S").clicked() {
                            ui.close();
                            self.export_file();
                        }
                        ui.separator();
                        if ui.button("Quitter").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.menu_button("Ajouter", |ui| {
                        let items: [(&str, fn() -> Mesh); 6] = [
                            ("Cube", || primitives::cube(2.0)),
                            ("Cylindre", || primitives::cylinder(1.0, 2.0, 32, 4)),
                            ("Pot biseauté", || primitives::beveled_cylinder(1.0, 1.6, 0.15, 4, 48, 8)),
                            ("Sphère UV", || primitives::uv_sphere(1.0, 32, 16)),
                            ("Tore", || primitives::torus(1.0, 0.35, 48, 16)),
                            ("Forme organique", || primitives::blob(1.0, 48, 24)),
                        ];
                        for (name, f) in items {
                            if ui.button(name).clicked() {
                                ui.close();
                                self.append_model(Model::single(name, f()));
                            }
                        }
                    });
                    ui.menu_button("Objet", |ui| {
                        self.object_menu_items(ui);
                    });
                    ui.menu_button("UV", |ui| {
                        self.uv_menu_items(ui, ctx);
                    });
                    ui.menu_button("Aide", |ui| {
                        if ui.button("Nouveautés…").clicked() {
                            ui.close();
                            self.show_news = true;
                        }
                        if ui.button("Vérifier les mises à jour").clicked() {
                            ui.close();
                            self.check_updates(ctx, true);
                            self.show_update = true;
                        }
                        if ui.button("Page du projet (GitHub)").clicked() {
                            ui.close();
                            let _ = std::process::Command::new("explorer").arg(format!("https://github.com/{}", update::REPO)).spawn();
                        }
                        ui.separator();
                        ui.label(RichText::new(format!("EasyUV {}", env!("CARGO_PKG_VERSION"))).color(theme::TEXT_DIM));
                    });
                    ui.add_space(24.0);
                    for (ws, label) in [(Workspace::Layout, "Layout"), (Workspace::UvEditing, "UV Editing"), (Workspace::Viewport, "Vue 3D")] {
                        let active = self.workspace == ws;
                        let txt = RichText::new(label).color(if active { Color32::WHITE } else { theme::TEXT_DIM });
                        let b = egui::Button::new(txt).fill(if active { theme::BG_SUBPANEL } else { Color32::TRANSPARENT });
                        if ui.add(b).clicked() {
                            self.workspace = ws;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // numéro de version, à droite : un clic ouvre les nouveautés
                        let v = egui::Button::new(RichText::new(format!("v{}", update::current_version())).small().color(theme::TEXT_DIM)).frame(false);
                        if ui.add(v).on_hover_text("Nouveautés de cette version").clicked() {
                            self.show_news = true;
                        }
                        if let UpdateState::Available(rel) = &self.update {
                            let txt = format!("Mise à jour {} disponible", rel.version);
                            let b = egui::Button::new(RichText::new(txt).color(Color32::BLACK).strong()).fill(theme::ACCENT_ORANGE);
                            if ui.add(b).clicked() {
                                self.show_update = true;
                            }
                        }
                    });
                });
            });
    }

    /// Le pointeur est-il au-dessus de l'éditeur UV (zone de l'image précédente) ?
    fn pointer_over_uv(&self, ctx: &egui::Context) -> bool {
        ctx.input(|i| i.pointer.hover_pos()).zip(self.uv_rect).is_some_and(|(p, r)| r.contains(p))
    }

    fn status_bar(&mut self, root: &mut egui::Ui) {
        // aide contextuelle, comme la barre d'état de Blender : elle suit la vue sous le pointeur
        let hints = if self.edit_mode && !self.pointer_over_uv(root.ctx()) {
            "Édition (Tab)   1 2 3 4 : sommets, arêtes, faces, îlots   Clic / Maj+clic : choisir   Alt+clic : boucle   A : tout   L / Ctrl+L : relié   Ctrl+E : couture   U : déplier la sélection"
        } else if self.pointer_over_uv(root.ctx()) {
            "1 2 3 4 : sommets, arêtes, faces, îlots   Clic / Maj+clic / rectangle : sélection   Glisser, G R S : transformer   U : déplier la sélection   Q, Maj+1 à 5 : outils   [ ] : rayon   Molette : zoom"
        } else {
            "Molette/Clic : orbite   Maj+glisser / Clic droit : déplacer   Molette : zoom   Clic : sélectionner   Tab : Édition   U : déplier   F : cadrer la sélection"
        };
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(theme::BG_PANEL).inner_margin(egui::Margin::symmetric(8, 3)))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(hints).color(theme::TEXT_DIM).small());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(&self.status).small());
                    });
                });
            });
    }

    fn side_panel(&mut self, root: &mut egui::Ui) {
        egui::Panel::right("props")
            .resizable(true)
            .default_size(310.0)
            .frame(egui::Frame::new().fill(theme::BG_PANEL).inner_margin(egui::Margin::same(6)))
            .show(root, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let ts = self.params.texture_size;
                    let mut act: Option<OutlinerAction> = None;
                    if let Some(s) = &self.scene {
                        let renaming = &mut self.renaming;
                        section(ui, "Outliner", true, |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} objet(s) · {} sommets · {} faces · {} tris",
                                    s.len(),
                                    s.model.mesh.positions.len(),
                                    s.model.mesh.face_count(),
                                    s.model.mesh.triangle_count()
                                ))
                                .small()
                                .color(theme::TEXT_DIM),
                            );
                            let has_uv = s.uv.is_some();
                            egui::ScrollArea::vertical().id_salt("outliner").max_height(180.0).show(ui, |ui| {
                                for i in 0..s.len() {
                                    let o = &s.model.objects[i];
                                    let (sel, hidden) = (s.selected[i], s.hidden[i]);
                                    let active = s.active == i && sel;
                                    ui.horizontal(|ui| {
                                        // œil de visibilité, dessiné (la police n'a pas d'icônes)
                                        let (r, p) = ui.allocate_painter(egui::vec2(16.0, 16.0), egui::Sense::click());
                                        let c = r.rect.center();
                                        let col = if hidden { theme::TEXT_DIM.gamma_multiply(0.5) } else if r.hovered() { Color32::WHITE } else { theme::TEXT };
                                        let eye = [c + egui::vec2(-6.0, 0.0), c + egui::vec2(0.0, -4.0), c + egui::vec2(6.0, 0.0), c + egui::vec2(0.0, 4.0)];
                                        p.add(egui::Shape::closed_line(eye.to_vec(), egui::Stroke::new(1.2, col)));
                                        if hidden {
                                            p.line_segment([c + egui::vec2(-6.0, 5.0), c + egui::vec2(6.0, -5.0)], egui::Stroke::new(1.2, col));
                                        } else {
                                            p.circle_filled(c, 2.0, col);
                                        }
                                        if r.on_hover_text(if hidden { "Afficher (Alt+H)" } else { "Masquer (H)" }).clicked() {
                                            act = Some(OutlinerAction::ToggleHidden(i));
                                        }
                                        if let Some((ri, text)) = renaming.as_mut().filter(|(ri, _)| *ri == i) {
                                            let te = ui.add(egui::TextEdit::singleline(text).desired_width(150.0));
                                            if !te.has_focus() && !te.lost_focus() {
                                                te.request_focus();
                                            }
                                            if te.lost_focus() {
                                                if ui.input(|k| k.key_pressed(egui::Key::Escape)) {
                                                    act = Some(OutlinerAction::CancelRename);
                                                } else {
                                                    act = Some(OutlinerAction::Rename(*ri, text.clone()));
                                                }
                                            }
                                        } else {
                                            // ligne sélectionnée : fond bleu, texte blanc (actif : orange clair)
                                            let name_col = if active {
                                                Color32::from_rgb(0xff, 0xd2, 0x8a)
                                            } else if sel {
                                                Color32::WHITE
                                            } else if hidden {
                                                theme::TEXT_DIM
                                            } else {
                                                theme::TEXT
                                            };
                                            let label = RichText::new(format!("{}   ({} faces)", o.name, o.face_end - o.face_start)).color(name_col);
                                            let resp = ui.selectable_label(sel, label);
                                            if resp.double_clicked() {
                                                act = Some(OutlinerAction::StartRename(i));
                                            } else if resp.clicked() {
                                                act = Some(OutlinerAction::Click(i, ui.input(|k| k.modifiers.shift || k.modifiers.command)));
                                            }
                                            resp.context_menu(|ui| {
                                                if !sel {
                                                    act = Some(OutlinerAction::Click(i, false));
                                                }
                                                if ui.button("Renommer   F2").clicked() {
                                                    ui.close();
                                                    act = Some(OutlinerAction::StartRename(i));
                                                }
                                                if ui.button("Dupliquer   Maj+D").clicked() {
                                                    ui.close();
                                                    act = Some(OutlinerAction::Duplicate(i));
                                                }
                                                if ui.button(if hidden { "Afficher" } else { "Masquer   H" }).clicked() {
                                                    ui.close();
                                                    act = Some(OutlinerAction::ToggleHidden(i));
                                                }
                                                ui.separator();
                                                if ui.button("Supprimer   X").clicked() {
                                                    ui.close();
                                                    act = Some(OutlinerAction::Delete(i));
                                                }
                                            });
                                        }
                                        if has_uv {
                                            let d = s.object_density(i, ts);
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.label(RichText::new(format!("{d:.0} px/m")).small().color(theme::TEXT_DIM));
                                            });
                                        }
                                    });
                                }
                            });
                            ui.label(RichText::new("Clic : sélection · Maj+clic : ajouter · double-clic : renommer · clic droit : menu").small().color(theme::TEXT_DIM));
                        });
                    }
                    if let Some(a) = act {
                        self.outliner_action(a);
                    }

                    section(ui, "Dépliage intelligent", true, |ui| {
                        egui::Grid::new("unwrap").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            ui.label("Forme");
                            let before = self.params.shape;
                            egui::ComboBox::from_id_salt("shape").selected_text(self.params.shape.label()).width(190.0).show_ui(ui, |ui| {
                                for p in ShapePreset::ALL {
                                    ui.selectable_value(&mut self.params.shape, p, p.label());
                                }
                            });
                            if self.params.shape != before && self.params.shape != ShapePreset::Auto {
                                // un preset choisi à la main applique aussi ses réglages conseillés
                                let (a, d, r) = self.params.shape.suggested();
                                self.params.angle_deg = a;
                                self.params.max_distortion = d;
                                self.params.relax_iterations = r;
                            }
                            ui.end_row();
                            ui.label("Angle de couture");
                            ui.add(egui::Slider::new(&mut self.params.angle_deg, 15.0..=89.0).suffix("°"));
                            ui.end_row();
                            ui.label("Tolérance distorsion");
                            ui.add(egui::Slider::new(&mut self.params.max_distortion, 0.02..=0.5).fixed_decimals(2));
                            ui.end_row();
                            ui.label("Relaxation (ARAP)");
                            ui.add(egui::Slider::new(&mut self.params.relax_iterations, 0..=20));
                            ui.end_row();
                            ui.label("Marge");
                            ui.add(egui::Slider::new(&mut self.params.margin, 0.0..=0.05).fixed_decimals(3));
                            ui.end_row();
                            ui.label("Qualité");
                            egui::ComboBox::from_id_salt("q")
                                .selected_text(["Rapide", "Normale", "Haute", "Maximale"][self.params.quality as usize])
                                .show_ui(ui, |ui| {
                                    for (i, n) in ["Rapide", "Normale", "Haute", "Maximale"].iter().enumerate() {
                                        ui.selectable_value(&mut self.params.quality, i as u32, *n);
                                    }
                                });
                            ui.end_row();
                            ui.label("Pivoter les îlots");
                            ui.checkbox(&mut self.params.rotate_islands, "");
                            ui.end_row();
                            ui.label("Auto-apprentissage");
                            ui.checkbox(&mut self.auto_learn, "").on_hover_text(
                                "EasyUV essaie plusieurs réglages, garde le meilleur et retient ce qui marche\npour chaque type de maillage. Plus il travaille, mieux il prédit.",
                            );
                            ui.end_row();
                            if self.auto_learn {
                                ui.label("Essais par dépliage");
                                ui.add(egui::Slider::new(&mut self.trials, 2..=16)).on_hover_text("Toujours au moins la prédiction et les réglages d'usine :\nle résultat n'est jamais moins bon que sans apprentissage.");
                                ui.end_row();
                                ui.label("Priorité");
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("coutures").small().color(theme::TEXT_DIM));
                                    // le curseur prend la place restante : « distorsion » ne doit pas sortir du panneau
                                    let font = egui::TextStyle::Small.resolve(ui.style());
                                    let right = ui.painter().layout_no_wrap("distorsion".into(), font, theme::TEXT_DIM).size().x + 2.0 * ui.spacing().item_spacing.x;
                                    ui.spacing_mut().slider_width = (ui.available_width() - right).clamp(40.0, 150.0);
                                    ui.add(egui::Slider::new(&mut self.priority, 0.0..=1.0).show_value(false)).on_hover_text(
                                        "Gauche : le moins de coutures et d'îlots possible.\nDroite : le moins de distorsion possible.\nS'applique immédiatement à ce que le modèle a déjà appris.",
                                    );
                                    ui.label(RichText::new("distorsion").small().color(theme::TEXT_DIM));
                                });
                                ui.end_row();
                            }
                            ui.label("Ranger ensemble");
                            ui.checkbox(&mut self.pack_together, "").on_hover_text(
                                "Coché : tous les objets partagent la tuile 0-1 et la même densité.\nDécoché : chaque objet a sa propre tuile.",
                            );
                            ui.end_row();
                        });
                        ui.add_space(6.0);
                        let busy = self.job.is_some();
                        ui.add_enabled_ui(!busy && self.scene.is_some(), |ui| {
                            let n_sel = self.scene.as_ref().map_or(0, |s| s.selection().len());
                            let uv_faces = self.scene.as_ref().filter(|s| s.uv.is_some() && s.has_island_selection()).map_or(0, |s| s.selected_faces().len());
                            let objects = if n_sel > 0 { format!("Déplier les objets sélectionnés ({n_sel})") } else { "Déplier tout".to_string() };
                            if busy {
                                primary_button(ui, "Dépliage…");
                            } else if uv_faces > 0 {
                                // une sélection dans l'éditeur UV : U ne redéplie qu'elle
                                if primary_button(ui, &format!("Déplier la sélection UV ({uv_faces} faces)  (U)")).on_hover_ui(|ui| {
                                    icons::tooltip(ui, Icon::Unwrap, "Déplier la sélection UV", "U", "Dépliage intelligent des seules faces sélectionnées (îlots ou faces). Le résultat est rangé dans la place libre, à la densité des autres îlots ; les coutures marquées sont respectées.");
                                }).clicked() {
                                    self.unwrap_uv_selection();
                                }
                                if ui.button(format!("{objects}  (Maj+U)")).clicked() {
                                    self.start_unwrap(ui.ctx());
                                }
                            } else if primary_button(ui, &format!("{objects}  (U)")).clicked() {
                                self.start_unwrap(ui.ctx());
                            }
                            ui.horizontal(|ui| {
                                if icons::button(ui, Icon::Project, false, "Projeter depuis la vue", "", PROJECT_HELP).clicked() {
                                    self.project_view_op(false);
                                }
                                if ui.button("Projeter depuis la vue").on_hover_text(PROJECT_HELP).clicked() {
                                    self.project_view_op(false);
                                }
                                if ui.small_button("Cadré").on_hover_text("Même projection, étirée pour remplir toute la tuile 0-1").clicked() {
                                    self.project_view_op(true);
                                }
                            });
                        });
                        if busy {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(if self.auto_learn { "Essais en parallèle, notation, apprentissage…" } else { "Segmentation, LSCM, ARAP, empaquetage…" });
                            });
                        }
                        if self.auto_learn {
                            ui.add_space(4.0);
                            let (total, local) = self.learner.try_lock().map(|l| (l.samples.len(), l.local_len())).unwrap_or((0, 0));
                            egui::Frame::new().fill(theme::FIELD).corner_radius(4).inner_margin(6).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(format!("Modèle : {total} expériences ({local} acquises ici)")).small().color(theme::TEXT_DIM));
                                if let Some(i) = &self.last_learn {
                                    let gain = i.score - i.default_score;
                                    ui.label(
                                        RichText::new(format!("Retenu : {:.0}°, tolérance {:.2}, ARAP {}", i.params.angle_deg, i.params.max_distortion, i.params.relax_iterations))
                                            .small(),
                                    );
                                    let col = if gain > 1e-4 { theme::AXIS_Y } else { theme::TEXT_DIM };
                                    ui.label(
                                        RichText::new(format!("Score {:+.3} ({:+.3} vs défaut) · {} essais · confiance {:.1}", i.score, gain, i.trials, i.confidence))
                                            .small()
                                            .color(col),
                                    );
                                }
                            });
                            let label = if self.confirm_reset { "Confirmer : oublier l'expérience locale" } else { "Oublier l'expérience locale" };
                            if ui.small_button(label).clicked() {
                                if self.confirm_reset {
                                    let _ = std::fs::remove_file(learn::default_path());
                                    if let Ok(mut l) = self.learner.lock() {
                                        *l = Learner::base();
                                    }
                                    self.last_learn = None;
                                    self.status = "Expérience locale effacée (modèle de base conservé)".into();
                                }
                                self.confirm_reset = !self.confirm_reset;
                            }
                        }
                    });

                    section(ui, "Outils UV", true, |ui| self.uv_toolkit(ui));

                    section(ui, "Rangement (layout)", true, |ui| {
                        ui.label(
                            RichText::new("Range les UV actuelles dans la tuile sans redéplier : après une retouche, ou pour réunir des objets dépliés séparément.")
                                .small()
                                .color(theme::TEXT_DIM),
                        );
                        egui::Grid::new("layout").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            ui.label("Égaliser la densité");
                            ui.checkbox(&mut self.pack_equalize, "").on_hover_text(
                                "Coché : chaque îlot reprend la même densité de texels (taille réelle).\nDécoché : les tailles relatives actuelles sont conservées (îlots agrandis à la main).",
                            );
                            ui.end_row();
                            ui.label("Pivoter les îlots");
                            ui.checkbox(&mut self.params.rotate_islands, "").on_hover_text("Autorise les rotations pour mieux remplir la tuile.");
                            ui.end_row();
                            ui.label("Marge");
                            ui.add(egui::Slider::new(&mut self.params.margin, 0.0..=0.05).fixed_decimals(3));
                            ui.end_row();
                            ui.label("Ranger ensemble");
                            ui.checkbox(&mut self.pack_together, "").on_hover_text("Coché : une tuile commune.\nDécoché : une tuile par objet.");
                            ui.end_row();
                        });
                        ui.add_space(4.0);
                        let (has_uv, n_isl, n_obj) = self.scene.as_ref().map_or((false, 0, 0), |s| (s.uv.is_some(), s.island_selected.iter().filter(|&&x| x).count(), s.selection().len()));
                        let label = if n_isl > 0 {
                            format!("Ranger les îlots sélectionnés ({n_isl})  (Ctrl+P)")
                        } else if n_obj > 0 {
                            format!("Ranger la sélection ({n_obj})  (Ctrl+P)")
                        } else {
                            "Ranger tout  (Ctrl+P)".to_string()
                        };
                        ui.add_enabled_ui(has_uv && self.job.is_none(), |ui| {
                            if primary_button(ui, &label).clicked() {
                                self.start_layout(ui.ctx());
                            }
                        });
                        ui.label(
                            RichText::new("Éditeur UV : clic / Maj+clic / rectangle pour choisir des îlots, glisser pour déplacer, G R S, X / Y pour l'axe, clic droit : menu.")
                                .small()
                                .color(theme::TEXT_DIM),
                        );
                    });

                    section(ui, "Densité de texels", true, |ui| {
                        egui::Grid::new("td").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            ui.label("Texture");
                            egui::ComboBox::from_id_salt("tex").selected_text(format!("{0} × {0}", self.params.texture_size)).show_ui(ui, |ui| {
                                for s in [256u32, 512, 1024, 2048, 4096, 8192] {
                                    ui.selectable_value(&mut self.params.texture_size, s, format!("{s} × {s}"));
                                }
                            });
                            ui.end_row();
                            ui.label("Mode");
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut self.target_mode, false, "Remplir 0-1");
                                ui.selectable_value(&mut self.target_mode, true, "Cible");
                            });
                            ui.end_row();
                            ui.label("Densité cible");
                            ui.add_enabled(
                                self.target_mode || self.scene.as_ref().is_some_and(|s| s.uv.is_some()),
                                egui::DragValue::new(&mut self.target_density).range(1.0..=100_000.0).speed(4.0).suffix(" px/m"),
                            );
                            ui.end_row();
                        });
                        ui.horizontal(|ui| {
                            for (lbl, v) in [("256", 256.0), ("512", 512.0), ("1024", 1024.0), ("2048", 2048.0)] {
                                if ui.small_button(lbl).clicked() {
                                    self.target_density = v;
                                }
                            }
                            ui.label(RichText::new("px/m").small().color(theme::TEXT_DIM));
                        });
                        ui.add_space(4.0);
                        let mut apply = false;
                        let mut pick = false;
                        if let Some(s) = &self.scene {
                            if s.uv.is_some() {
                                egui::Frame::new().fill(theme::FIELD).corner_radius(4).inner_margin(6).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new(format!("Actuelle : {:.1} px/m  ({:.2} px/cm)", s.density, s.density / 100.0)).strong());
                                    if s.uv_overflow() {
                                        ui.label(RichText::new("Attention : les UV dépassent la tuile 0-1").color(theme::ACCENT_ORANGE));
                                    }
                                });
                                ui.horizontal(|ui| {
                                    apply = ui.button("Appliquer la cible").clicked();
                                    pick = ui.button("Prendre l'actuelle").clicked();
                                });
                            }
                        }
                        if apply {
                            self.apply_density();
                        }
                        if pick {
                            if let Some(s) = &self.scene {
                                self.target_density = s.density.round();
                            }
                        }
                    });

                    section(ui, "Damier", true, |ui| {
                        egui::Grid::new("chk").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            ui.label("Style");
                            let before = self.checker_style;
                            egui::ComboBox::from_id_salt("checker_style").selected_text(self.checker_style.label()).width(150.0).show_ui(ui, |ui| {
                                for st in CheckerStyle::ALL {
                                    ui.selectable_value(&mut self.checker_style, st, st.label()).on_hover_text(st.help());
                                }
                            });
                            self.texture_dirty |= self.checker_style != before;
                            ui.end_row();
                            ui.label("Cellules");
                            self.texture_dirty |= ui.add(egui::Slider::new(&mut self.checker_cells, 2..=64)).changed();
                            ui.end_row();
                            ui.label("Répétition");
                            ui.add(egui::Slider::new(&mut self.view.tiling, 0.25..=16.0).logarithmic(true));
                            ui.end_row();
                        });
                        ui.label(RichText::new(self.checker_style.help()).small().color(theme::TEXT_DIM));
                        let cell_px = self.params.texture_size as f32 / (self.checker_cells as f32 * self.view.tiling);
                        ui.label(RichText::new(format!("1 case ≈ {cell_px:.0} px de texture")).small().color(theme::TEXT_DIM));
                    });

                    section(ui, "Affichage", true, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for (s, n) in [(Shading::Solid, "Solide"), (Shading::Checker, "Damier"), (Shading::Islands, "Îlots"), (Shading::Stretch, "Étirement")] {
                                ui.selectable_value(&mut self.view.shading, s, n);
                            }
                        });
                        ui.checkbox(&mut self.view.wire, "Fil de fer");
                        ui.checkbox(&mut self.view.seams, "Coutures");
                        ui.checkbox(&mut self.view.grid, "Grille du sol");
                        if self.view.shading == Shading::Stretch {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("aucun").small());
                                let (r, p) = ui.allocate_painter(egui::vec2(120.0, 10.0), egui::Sense::hover());
                                for i in 0..60 {
                                    let t = i as f32 / 59.0;
                                    let x = r.rect.left() + t * r.rect.width();
                                    p.rect_filled(egui::Rect::from_min_max(egui::pos2(x, r.rect.top()), egui::pos2(x + 2.5, r.rect.bottom())), 0.0, viewport::stretch_color(t));
                                }
                                ui.label(RichText::new("×2").small());
                            });
                        }
                    });

                    if let Some(sc) = self.score.clone() {
                        section(ui, "Score UV (en direct)", true, |ui| score_details(ui, &sc));
                    }
                    if let Some(uv) = self.scene.as_ref().and_then(|s| s.uv.as_ref()) {
                        section(ui, "Statistiques", true, |ui| {
                            let s = &uv.stats;
                            egui::Grid::new("stats").num_columns(2).spacing([12.0, 3.0]).show(ui, |ui| {
                                for (k, v) in [
                                    ("Îlots", s.charts.to_string()),
                                    (
                                        "Formes",
                                        [(s.shapes[0], "hard-surface"), (s.shapes[1], "cylindre"), (s.shapes[2], "organique")]
                                            .iter()
                                            .filter(|(n, _)| *n > 0)
                                            .map(|(n, l)| format!("{n} {l}"))
                                            .collect::<Vec<_>>()
                                            .join(" · "),
                                    ),
                                    ("Coutures", s.seams.to_string()),
                                    ("Distorsion moy.", format!("{:.3}", s.mean_distortion)),
                                    ("Distorsion max", format!("{:.3}", s.max_distortion)),
                                    ("Retournés", s.flipped.to_string()),
                                    ("Couverture 0-1", format!("{:.1} %", s.coverage * 100.0)),
                                    ("Temps", format!("{:.0} ms", s.time_ms)),
                                ] {
                                    ui.label(RichText::new(k).color(theme::TEXT_DIM));
                                    ui.label(v);
                                    ui.end_row();
                                }
                            });
                        });
                    }
                });
            });
    }

    fn outliner_action(&mut self, a: OutlinerAction) {
        match a {
            OutlinerAction::Click(i, extend) => {
                if let Some(s) = &mut self.scene {
                    s.click(Some(i), extend);
                }
            }
            OutlinerAction::ToggleHidden(i) => {
                self.snapshot();
                if let Some(s) = &mut self.scene {
                    s.toggle_hidden(i);
                }
            }
            OutlinerAction::StartRename(i) => {
                if let Some(s) = &self.scene {
                    self.renaming = Some((i, s.model.objects[i].name.clone()));
                }
            }
            OutlinerAction::Rename(i, name) => {
                self.renaming = None;
                let changed = self.scene.as_ref().is_some_and(|s| s.model.objects[i].name != name.trim() && !name.trim().is_empty());
                if changed {
                    self.snapshot();
                    if let Some(s) = &mut self.scene {
                        s.rename(i, &name);
                    }
                }
            }
            OutlinerAction::CancelRename => self.renaming = None,
            OutlinerAction::Duplicate(i) => {
                if let Some(s) = &mut self.scene {
                    if !s.selected[i] {
                        s.click(Some(i), false);
                    }
                }
                self.duplicate_selected();
            }
            OutlinerAction::Delete(i) => {
                if let Some(s) = &mut self.scene {
                    if !s.selected[i] {
                        s.click(Some(i), false);
                    }
                }
                self.delete_selected();
            }
        }
    }

    fn apply_density(&mut self) {        let ts = self.params.texture_size;
        if self.scene.as_ref().is_some_and(|s| s.uv.is_some()) {
            self.snapshot();
        }
        if let Some(s) = &mut self.scene {
            if s.apply_density(self.target_density, ts) {
                self.status = format!("Densité appliquée : {:.1} px/m", s.density);
            }
        }
    }

    fn editor_header(ui: &mut egui::Ui, rect: egui::Rect, title: &str, view: &mut ViewOptions, show_shading: bool) {
        let header = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 26.0));
        ui.painter().rect_filled(header, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, theme::BG_PANEL);
        ui.scope_builder(egui::UiBuilder::new().max_rect(header.shrink2(egui::vec2(8.0, 3.0))), |ui| {
            ui.horizontal_centered(|ui| {
                // vue étroite (séparateur déplacé) : les boutons passent avant le titre
                if !show_shading || rect.width() > 540.0 {
                    ui.label(RichText::new(title).color(theme::TEXT_DIM));
                }
                if show_shading {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        for (s, n) in [(Shading::Stretch, "Étirement"), (Shading::Islands, "Îlots"), (Shading::Checker, "Damier"), (Shading::Solid, "Solide")] {
                            ui.selectable_value(&mut view.shading, s, n);
                        }
                        ui.separator();
                        ui.toggle_value(&mut view.seams, "Coutures");
                        ui.toggle_value(&mut view.wire, "Fil");
                    });
                }
            });
        });
    }
}

impl eframe::App for EasyUvApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        self.poll_job();
        self.poll_update();
        self.ensure_texture(ctx);

        // Fichiers glissés-déposés (Maj : ajouter à la scène)
        let (dropped, shift): (Vec<PathBuf>, bool) = ctx.input(|i| (i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect(), i.modifiers.shift));
        for (k, p) in dropped.into_iter().enumerate() {
            self.open_file(p, shift || k > 0);
        }
        // Raccourcis
        let (u, f, home, o, s, imp, pk) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::U),
                i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::Home),
                i.modifiers.command && i.key_pressed(egui::Key::O),
                i.modifiers.command && i.key_pressed(egui::Key::S),
                i.modifiers.command && i.key_pressed(egui::Key::I),
                i.modifiers.command && i.key_pressed(egui::Key::P),
            )
        });
        if pk && !ctx.egui_wants_keyboard_input() {
            self.start_layout(ctx);
        }
        if !ctx.egui_wants_keyboard_input() && !self.uv_edit.busy() {
            if u {
                // U : la sélection UV s'il y en a une (comme Blender), sinon les objets ; Maj+U : les objets
                let shift = ctx.input(|i| i.modifiers.shift);
                if !shift && self.scene.as_ref().is_some_and(|s| s.uv.is_some() && s.has_island_selection()) {
                    self.unwrap_uv_selection();
                } else {
                    self.start_unwrap(ctx);
                }
            }
            // comme Blender : F cadre la sélection, Origine tout, dans la vue sous le pointeur
            if f || home {
                let over_uv = self.pointer_over_uv(ctx);
                match (&self.scene, over_uv) {
                    (Some(s), true) => match s.corners_bounds(&s.selected_corners()).filter(|_| f) {
                        Some((lo, hi)) => self.uv_cam.frame(lo, hi),
                        None => self.uv_cam = UvCamera::default(),
                    },
                    (Some(s), false) => {
                        let (lo, hi) = if f { s.focus_bounds() } else { s.model.mesh.bounds() };
                        self.cam.frame(lo, hi);
                    }
                    (None, _) => self.uv_cam = UvCamera::default(),
                }
            }
        }
        if o {
            self.pick_and_open(false);
        }
        if imp {
            self.pick_and_open(true);
        }
        if s {
            self.export_file();
        }

        self.object_shortcuts(ctx);
        self.update_score(ctx);
        // choisir sommets, arêtes ou faces (dans l'éditeur UV ou la vue 3D) passe la vue 3D en
        // Édition : un clic sur le modèle choisit alors directement ces éléments
        // l'état d'Édition vit dans la scène (annuler, supprimer, ouvrir un fichier le changent)
        if let Some(s) = &mut self.scene {
            if s.is_editing() && !(0..s.len()).any(|o| s.in_edit(o) && !s.hidden[o]) {
                s.exit_edit(); // tous les objets édités ont été masqués
            }
        }
        self.edit_mode = self.scene.as_ref().is_some_and(|s| s.is_editing());
        if let Some(mode) = self.scene.as_ref().map(|s| s.sel_mode) {
            if mode != self.last_sel_mode {
                if mode != SelectMode::Island && !self.edit_mode {
                    self.set_edit_mode(true);
                }
                self.last_sel_mode = mode;
            }
        }

        self.top_bar(root);
        self.status_bar(root);
        self.side_panel(root);

        let tex = self.texture.as_ref().unwrap().id();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG_WINDOW).inner_margin(egui::Margin::same(2)))
            .show(root, |ui| {
                let full = ui.available_rect_before_wrap();
                let split = |k: f32| {
                    let w = full.width() * k;
                    (
                        Some(egui::Rect::from_min_max(full.min, egui::pos2(full.left() + w - 1.0, full.bottom()))),
                        Some(egui::Rect::from_min_max(egui::pos2(full.left() + w + 1.0, full.top()), full.max)),
                    )
                };
                // part de la vue 3D, réglable en glissant le séparateur (une par espace de travail)
                let ratio = match self.workspace {
                    Workspace::Viewport => None,
                    Workspace::Layout => Some(self.split[0]),
                    Workspace::UvEditing => Some(self.split[1]),
                };
                let (r3d, ruv) = match ratio {
                    None => (Some(full), None),
                    Some(k) => split(k),
                };
                if let Some(r) = r3d {
                    let body = egui::Rect::from_min_max(r.min + egui::vec2(0.0, 26.0), r.max);
                    let resp = ui.interact(body, ui.scope_id().with("vp3d"), egui::Sense::click_and_drag());
                    let on_gizmo = viewport::handle_input(ui, &resp, &mut self.cam, body);
                    if resp.hovered() && !ctx.egui_wants_keyboard_input() {
                        self.view3d_keys(ui, r);
                    }
                    if self.edit_mode && resp.clicked() && !on_gizmo {
                        self.edit_click(ui, &resp, r);
                    }
                    if resp.clicked() && !on_gizmo && !self.edit_mode {
                        let hit = resp.interact_pointer_pos().and_then(|p| self.render.pick(p));
                        let extend = ui.input(|i| i.modifiers.shift || i.modifiers.command);
                        if let Some(s) = &mut self.scene {
                            s.click(hit, extend);
                        }
                    }
                    if resp.secondary_clicked() && !self.edit_mode {
                        // le clic droit sélectionne l'objet visé avant d'ouvrir le menu
                        let hit = resp.interact_pointer_pos().and_then(|p| self.render.pick(p));
                        if let (Some(s), Some(h)) = (&mut self.scene, hit) {
                            if !s.selected[h] {
                                s.click(Some(h), false);
                            }
                        }
                    }
                    resp.context_menu(|ui| self.object_menu_items(ui));
                    // en Édition, le fil de fer des objets édités est dessiné d'office (render_scene)
                    viewport::draw(ui, r, self.scene.as_ref(), &self.cam, &self.view, &self.mips, &mut self.render);
                    if self.edit_mode {
                        if let Some(s) = &self.scene {
                            viewport::draw_vertices(ui, r, s, &self.cam, &self.render, s.sel_mode == SelectMode::Vertex);
                        }
                    }
                    let title = if self.edit_mode { format!("{} · Édition", self.cam.title()) } else { self.cam.title() };
                    let (show_title, extra) = self.view3d_header_layout(ui, r, &title);
                    Self::editor_header(ui, r, if show_title { &title } else { "" }, &mut self.view, true);
                    if let Some((x0, compact)) = extra {
                        self.view3d_header_extra(ui, r, x0, compact);
                    }
                }
                if ruv.is_none() {
                    self.uv_rect = None;
                }
                if let Some(r) = ruv {
                    let body = egui::Rect::from_min_max(r.min + egui::vec2(0.0, 26.0), r.max);
                    let resp = ui.interact(body, ui.scope_id().with("uv2d"), egui::Sense::click_and_drag());
                    self.uv_rect = Some(body);
                    let busy = self.job.is_some();
                    let ev = uvview::interact(ui, &resp, &mut self.uv_cam, body, if busy { None } else { self.scene.as_mut() }, &mut self.uv_edit);
                    match ev {
                        Some(UvEvent::Commit(orig)) => {
                            // historique : l'état d'avant la transformation
                            if let Some(s) = &self.scene {
                                let mut prev = s.clone();
                                if let Some(u) = &mut prev.uv {
                                    u.corner_uv = orig;
                                }
                                self.undo.push(prev);
                                if self.undo.len() > UNDO_LEVELS {
                                    self.undo.remove(0);
                                }
                                self.redo.clear();
                            }
                            let ts = self.params.texture_size;
                            if let Some(s) = &mut self.scene {
                                s.refresh(ts);
                            }
                            self.status = "Transformation appliquée — Ctrl+Z pour annuler".into();
                        }
                        Some(UvEvent::Status(t)) => self.status = t,
                        Some(UvEvent::Straighten) => self.straighten_strips(),
                        Some(UvEvent::SnapGrid) => self.snap_grid_op(),
                        Some(UvEvent::FlattenRect(lo, hi)) => {
                            let (lo, hi) = (easyuv::math::V2::new(lo[0] as f64, lo[1] as f64), easyuv::math::V2::new(hi[0] as f64, hi[1] as f64));
                            self.uv_tool_op("Aplatir dans le rectangle", move |m, uv, fi, ids| {
                                uvtools::flatten_into_rect(m, uv, fi, ids, lo, hi);
                            });
                        }
                        None => {}
                    }
                    if uvview::context_menu_allowed(&self.uv_edit) {
                        resp.context_menu(|ui| self.uv_menu_items(ui, ctx));
                    }
                    uvview::draw(ui, body, self.scene.as_ref(), &self.uv_cam, self.view.shading, tex, self.view.tiling, &mut self.uv_cache, &self.uv_edit, busy);
                    if busy {
                        // calcul en cours : l'éditeur est figé (les îlots vont changer)
                        let p = ui.painter_at(body);
                        p.rect_filled(body, 4.0, Color32::from_black_alpha(90));
                        let c = body.center();
                        ui.put(egui::Rect::from_center_size(c - egui::vec2(0.0, 26.0), egui::vec2(24.0, 24.0)), egui::Spinner::new().size(24.0));
                        uvview::overlay_text(&p, c + egui::vec2(0.0, 10.0), if self.job_layout { "Rangement des îlots…" } else { "Dépliage en cours…" });
                    }
                    let title = match self.scene.as_ref().filter(|s| s.uv.is_some()) {
                        Some(s) => format!("Éditeur UV · {:.0} px/m @ {}px", s.density, self.params.texture_size),
                        None => "Éditeur UV".to_string(),
                    };
                    let mut dummy = ViewOptions { shading: self.view.shading, wire: false, seams: false, grid: false, tiling: 1.0 };
                    let title = if r.width() < 760.0 { String::new() } else { title };
                    Self::editor_header(ui, r, &title, &mut dummy, false);
                    self.uv_header_bar(ui, egui::Rect::from_min_size(r.min, egui::vec2(r.width(), 26.0)));
                }
                // séparateur entre les deux vues, déclaré après elles pour passer devant ;
                // double-clic : partage par défaut
                if let Some(k) = ratio {
                    let i = if self.workspace == Workspace::Layout { 0 } else { 1 };
                    let x = full.left() + full.width() * k;
                    let handle = egui::Rect::from_min_max(egui::pos2(x - 4.0, full.top()), egui::pos2(x + 4.0, full.bottom()));
                    let resp = ui.interact(handle, ui.scope_id().with("split"), egui::Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                    if resp.dragged() {
                        if let Some(p) = resp.interact_pointer_pos() {
                            // chaque vue garde au moins 220 px
                            let min = (220.0 / full.width()).min(0.45);
                            self.split[i] = ((p.x - full.left()) / full.width()).clamp(min, 1.0 - min);
                        }
                    }
                    if resp.double_clicked() {
                        self.split[i] = SPLIT_DEFAULT[i];
                    }
                    if resp.hovered() || resp.dragged() {
                        ui.painter().rect_filled(egui::Rect::from_center_size(egui::pos2(x, full.center().y), egui::vec2(2.0, full.height())), 0.0, theme::ACCENT_BLUE);
                    }
                }
            });

        if let Some(e) = self.error.clone() {
            egui::Window::new("Erreur").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(e);
                if ui.button("OK").clicked() {
                    self.error = None;
                }
            });
        }
        self.confirm_delete_popup(ctx);
        let mut check = false;
        crate::news::window(ctx, &mut self.show_news, &mut check);
        if check {
            self.show_news = false;
            self.check_updates(ctx, true);
            self.show_update = true;
        }
        self.update_window(ctx);
        if self.job.is_some() || matches!(self.update, UpdateState::Checking(..) | UpdateState::Installing(_)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}
