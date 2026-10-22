//! Fenêtre « Nouveautés » : le CHANGELOG livré dans l'exécutable, version par version.

use crate::theme;
use crate::update;
use eframe::egui::{self, text::LayoutJob, Color32, RichText};
use std::path::PathBuf;

const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// Une version du CHANGELOG : numéro, date, lignes.
struct Section {
    version: &'static str,
    date: &'static str,
    lines: Vec<&'static str>,
}

fn sections() -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for line in CHANGELOG.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            // « ## [0.2.0] - 2026-10-22 »
            let version = h.split(['[', ']']).nth(1).unwrap_or(h).trim();
            let date = h.split(" - ").nth(1).unwrap_or("").trim();
            out.push(Section { version, date, lines: Vec::new() });
        } else if let Some(s) = out.last_mut() {
            s.lines.push(line);
        }
    }
    out
}

/// « 2026-10-22 » -> « 22/10/2026 ».
fn french_date(d: &str) -> String {
    let p: Vec<&str> = d.split('-').collect();
    if p.len() == 3 {
        format!("{}/{}/{}", p[2], p[1], p[0])
    } else {
        d.to_string()
    }
}

/// Fichier qui retient la dernière version dont les nouveautés ont été montrées.
fn seen_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("EasyUV").join("version-vue.txt"))
}

/// Vrai au premier lancement d'une version (après une mise à jour) ; la version est retenue.
pub fn first_run_of_version() -> bool {
    let Some(p) = seen_path() else { return false };
    let cur = update::current_version();
    if std::fs::read_to_string(&p).is_ok_and(|s| s.trim() == cur) {
        return false;
    }
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&p, &cur);
    true
}

/// Texte Markdown d'une ligne : **gras** en blanc, `code` en monospace.
fn rich_line(ui: &egui::Ui, text: &str, width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let style = ui.style();
    for (i, part) in text.split("**").enumerate() {
        for (j, bit) in part.split('`').enumerate() {
            if bit.is_empty() {
                continue;
            }
            let mut t = RichText::new(bit);
            if i % 2 == 1 {
                t = t.strong().color(Color32::WHITE);
            }
            if j % 2 == 1 {
                t = t.monospace();
            }
            t.append_to(&mut job, style, egui::FontSelection::Default, egui::Align::LEFT);
        }
    }
    job.wrap.max_width = width;
    job
}

/// Fenêtre des nouveautés. `check` passe à vrai si l'utilisateur demande une vérification.
pub fn window(ctx: &egui::Context, open: &mut bool, check: &mut bool) {
    if !*open {
        return;
    }
    let current = update::current_version();
    egui::Window::new("Nouveautés d'EasyUV")
        .collapsible(false)
        .resizable(false)
        .open(open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(560.0);
            ui.set_max_width(560.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Version installée : {current}")).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    *check = ui.button("Vérifier les mises à jour").clicked();
                });
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(460.0).auto_shrink([false, true]).show(ui, |ui| {
                for (k, s) in sections().iter().enumerate() {
                    let installed = s.version == current;
                    let date = if s.date.is_empty() { String::new() } else { format!("  ·  {}", french_date(s.date)) };
                    let title = format!("Version {}{date}{}", s.version, if installed { "  (installée)" } else { "" });
                    let col = if installed { theme::ACCENT_ORANGE } else { theme::TEXT };
                    egui::CollapsingHeader::new(RichText::new(title).color(col).strong()).id_salt(s.version).default_open(k == 0).show(ui, |ui| {
                        for line in &s.lines {
                            let w = ui.available_width();
                            if let Some(h) = line.strip_prefix("### ") {
                                ui.add_space(4.0);
                                ui.label(RichText::new(h).color(theme::ACCENT_BLUE.gamma_multiply(1.6)).strong());
                            } else if let Some(b) = line.trim_start().strip_prefix("- ") {
                                let indent = if line.starts_with(' ') { 26.0 } else { 10.0 };
                                ui.horizontal_top(|ui| {
                                    ui.add_space(indent);
                                    ui.label(RichText::new("•").color(theme::TEXT_DIM));
                                    ui.add(egui::Label::new(rich_line(ui, b, w - indent - 16.0)).wrap());
                                });
                            } else if !line.trim().is_empty() {
                                ui.add(egui::Label::new(rich_line(ui, line.trim(), w)).wrap());
                            }
                        }
                        ui.add_space(4.0);
                    });
                }
            });
            ui.add_space(4.0);
            ui.hyperlink_to("Toutes les versions sur GitHub", format!("https://github.com/{}/releases", update::REPO));
        });
}
