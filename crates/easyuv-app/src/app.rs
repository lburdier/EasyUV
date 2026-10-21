use crate::io;
use crate::scene::Scene;
use crate::theme::{self, primary_button, section};
use crate::update::{self, Release};
use crate::uvview::{self, UvCamera};
use crate::viewport::{self, Camera, Shading, ViewOptions, TEX_SIZE};
use easyuv::checker::{generate_rgba8, CheckerStyle};
use easyuv::math::V3;
use easyuv::mesh::Mesh;
use easyuv::model::Model;
use easyuv::{primitives, DensityMode, Params, UvResult};
use eframe::egui::{self, Color32, RichText, TextureHandle, TextureOptions};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Workspace {
    Layout,
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
    job: Option<Receiver<UvResult>>,
    update: UpdateState,
    show_update: bool,
    status: String,
    error: Option<String>,
}

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
            update: UpdateState::Idle,
            show_update: false,
            status: "Prêt".into(),
            error: None,
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
        let active = model.objects.len() - 1;
        self.set_model(name, model);
        if let Some(s) = &mut self.scene {
            s.active = active;
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
        let model = scene.model.clone();
        let params = self.effective_params();
        let together = self.pack_together;
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = easyuv::model::unwrap_model(&model, &params, together);
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.job = Some(rx);
        self.status = "Dépliage en cours…".into();
    }

    fn poll_job(&mut self) {
        let Some(rx) = &self.job else { return };
        if let Ok(r) = rx.try_recv() {
            let s = &r.stats;
            self.status = format!(
                "Dépliage : {} îlots, {} coutures, distorsion {:.3}, {:.0} ms{}",
                s.charts,
                s.seams,
                s.mean_distortion,
                s.time_ms,
                if s.overflow { " — ATTENTION : la densité cible dépasse la tuile 0-1" } else { "" }
            );
            if let Some(scene) = &mut self.scene {
                scene.set_uv(r, self.params.texture_size);
            }
            self.job = None;
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
        self.texture_dirty = false;
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
                        let items: [(&str, fn() -> Mesh); 5] = [
                            ("Cube", || primitives::cube(2.0)),
                            ("Cylindre", || primitives::cylinder(1.0, 2.0, 32, 4)),
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
                    ui.menu_button("UV", |ui| {
                        if ui.button("Dépliage intelligent   U").clicked() {
                            ui.close();
                            self.start_unwrap(ctx);
                        }
                        if ui.button("Appliquer la densité cible").clicked() {
                            ui.close();
                            self.apply_density();
                        }
                    });
                    ui.menu_button("Aide", |ui| {
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
                    if let UpdateState::Available(rel) = &self.update {
                        let txt = format!("Mise à jour {} disponible", rel.version);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let b = egui::Button::new(RichText::new(txt).color(Color32::BLACK).strong()).fill(theme::ACCENT_ORANGE);
                            if ui.add(b).clicked() {
                                self.show_update = true;
                            }
                        });
                    }
                });
            });
    }

    fn status_bar(&mut self, root: &mut egui::Ui) {
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(theme::BG_PANEL).inner_margin(egui::Margin::symmetric(8, 3)))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Molette/Clic : orbite   Maj+glisser / Clic droit : déplacer   Molette : zoom   U : déplier   F : cadrer").color(theme::TEXT_DIM).small());
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
                    if let Some(s) = &mut self.scene {
                        section(ui, "Outliner", true, |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} sommets · {} faces · {} tris",
                                    s.model.mesh.positions.len(),
                                    s.model.mesh.face_count(),
                                    s.model.mesh.triangle_count()
                                ))
                                .small()
                                .color(theme::TEXT_DIM),
                            );
                            let has_uv = s.uv.is_some();
                            egui::ScrollArea::vertical().id_salt("outliner").max_height(150.0).show(ui, |ui| {
                                for i in 0..s.model.objects.len() {
                                    let o = &s.model.objects[i];
                                    let active = s.active == i;
                                    let label = format!("{}   ({} faces)", o.name, o.face_end - o.face_start);
                                    let col = if active { theme::ACCENT_ORANGE } else { theme::TEXT };
                                    let density = if has_uv { Some(s.object_density(i, ts)) } else { None };
                                    ui.horizontal(|ui| {
                                        if ui.selectable_label(active, RichText::new(label).color(col)).clicked() {
                                            s.active = i;
                                        }
                                        if let Some(d) = density {
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.label(RichText::new(format!("{d:.0} px/m")).small().color(theme::TEXT_DIM));
                                            });
                                        }
                                    });
                                }
                            });
                        });
                    }

                    section(ui, "Dépliage intelligent", true, |ui| {
                        egui::Grid::new("unwrap").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
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
                            ui.label("Ranger ensemble");
                            ui.checkbox(&mut self.pack_together, "").on_hover_text(
                                "Coché : tous les objets partagent la tuile 0-1 et la même densité.\nDécoché : chaque objet a sa propre tuile.",
                            );
                            ui.end_row();
                        });
                        ui.add_space(6.0);
                        let busy = self.job.is_some();
                        ui.add_enabled_ui(!busy && self.scene.is_some(), |ui| {
                            if primary_button(ui, if busy { "Dépliage…" } else { "Déplier  (U)" }).clicked() {
                                self.start_unwrap(ui.ctx());
                            }
                        });
                        if busy {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Segmentation, LSCM, ARAP, empaquetage…");
                            });
                        }
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
                            ui.horizontal(|ui| {
                                self.texture_dirty |= ui.selectable_value(&mut self.checker_style, CheckerStyle::Color, "Couleur").changed();
                                self.texture_dirty |= ui.selectable_value(&mut self.checker_style, CheckerStyle::Mono, "Mono").changed();
                            });
                            ui.end_row();
                            ui.label("Cellules");
                            self.texture_dirty |= ui.add(egui::Slider::new(&mut self.checker_cells, 2..=64)).changed();
                            ui.end_row();
                            ui.label("Répétition");
                            ui.add(egui::Slider::new(&mut self.view.tiling, 0.25..=16.0).logarithmic(true));
                            ui.end_row();
                        });
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

                    if let Some(uv) = self.scene.as_ref().and_then(|s| s.uv.as_ref()) {
                        section(ui, "Statistiques", true, |ui| {
                            let s = &uv.stats;
                            egui::Grid::new("stats").num_columns(2).spacing([12.0, 3.0]).show(ui, |ui| {
                                for (k, v) in [
                                    ("Îlots", s.charts.to_string()),
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

    fn apply_density(&mut self) {
        let ts = self.params.texture_size;
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
                ui.label(RichText::new(title).color(theme::TEXT_DIM));
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
        let (u, f, o, s, imp) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::U),
                i.key_pressed(egui::Key::F) || i.key_pressed(egui::Key::Home),
                i.modifiers.command && i.key_pressed(egui::Key::O),
                i.modifiers.command && i.key_pressed(egui::Key::S),
                i.modifiers.command && i.key_pressed(egui::Key::I),
            )
        });
        if !ctx.egui_wants_keyboard_input() {
            if u {
                self.start_unwrap(ctx);
            }
            if f {
                if let Some(s) = &self.scene {
                    let (lo, hi) = s.model.mesh.bounds();
                    self.cam.frame(lo, hi);
                }
                self.uv_cam = UvCamera::default();
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
                let (r3d, ruv) = match self.workspace {
                    Workspace::Viewport => (Some(full), None),
                    Workspace::Layout => split(0.58),
                    Workspace::UvEditing => split(0.4),
                };
                if let Some(r) = r3d {
                    let body = egui::Rect::from_min_max(r.min + egui::vec2(0.0, 26.0), r.max);
                    let resp = ui.interact(body, ui.scope_id().with("vp3d"), egui::Sense::click_and_drag());
                    viewport::handle_input(ui, &resp, &mut self.cam, body);
                    viewport::draw(ui, r, self.scene.as_ref(), &self.cam, &self.view, tex);
                    Self::editor_header(ui, r, "Vue 3D · Perspective", &mut self.view, true);
                }
                if let Some(r) = ruv {
                    let body = egui::Rect::from_min_max(r.min + egui::vec2(0.0, 26.0), r.max);
                    let resp = ui.interact(body, ui.scope_id().with("uv2d"), egui::Sense::click_and_drag());
                    uvview::handle_input(ui, &resp, &mut self.uv_cam, body);
                    uvview::draw(ui, body, self.scene.as_ref(), &self.uv_cam, self.view.shading, tex, self.view.tiling);
                    let title = match self.scene.as_ref().filter(|s| s.uv.is_some()) {
                        Some(s) => format!("Éditeur UV · {:.0} px/m @ {}px", s.density, self.params.texture_size),
                        None => "Éditeur UV".to_string(),
                    };
                    let mut dummy = ViewOptions { shading: self.view.shading, wire: false, seams: false, grid: false, tiling: 1.0 };
                    Self::editor_header(ui, r, &title, &mut dummy, false);
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
        self.update_window(ctx);
        if self.job.is_some() || matches!(self.update, UpdateState::Checking(..) | UpdateState::Installing(_)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}
