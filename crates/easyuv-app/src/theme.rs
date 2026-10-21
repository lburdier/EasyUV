//! Direction artistique calquée sur le thème par défaut de Blender 4.x / 5.x.

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, Visuals};

pub const BG_WINDOW: Color32 = Color32::from_rgb(0x1d, 0x1d, 0x1d); // séparateurs entre zones
pub const BG_PANEL: Color32 = Color32::from_rgb(0x30, 0x30, 0x30); // régions (header, N-panel)
pub const BG_SUBPANEL: Color32 = Color32::from_rgb(0x3d, 0x3d, 0x3d); // sous-panneaux repliables
pub const BG_VIEWPORT: Color32 = Color32::from_rgb(0x39, 0x39, 0x39); // vue 3D
pub const BG_UV: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b); // éditeur UV
pub const WIDGET: Color32 = Color32::from_rgb(0x54, 0x54, 0x54);
pub const WIDGET_HOVER: Color32 = Color32::from_rgb(0x65, 0x65, 0x65);
pub const FIELD: Color32 = Color32::from_rgb(0x28, 0x28, 0x28);
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x9a);
pub const ACCENT_BLUE: Color32 = Color32::from_rgb(0x47, 0x72, 0xb3); // sélection Blender
pub const ACCENT_ORANGE: Color32 = Color32::from_rgb(0xff, 0xa0, 0x28); // objet actif
pub const SEAM_RED: Color32 = Color32::from_rgb(0xdb, 0x25, 0x12); // coutures Blender
pub const GRID: Color32 = Color32::from_rgb(0x48, 0x48, 0x48);
pub const AXIS_X: Color32 = Color32::from_rgb(0xff, 0x33, 0x52);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x8b, 0xdc, 0x00);
pub const AXIS_Z: Color32 = Color32::from_rgb(0x28, 0x90, 0xff);

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    let r = CornerRadius::same(4);
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_PANEL;
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = BG_SUBPANEL;
    v.code_bg_color = FIELD;
    v.window_stroke = Stroke::new(1.0, BG_WINDOW);
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(4);
    v.selection.bg_fill = ACCENT_BLUE;
    v.selection.stroke = Stroke::new(1.0, TEXT);
    v.hyperlink_color = ACCENT_BLUE;
    v.override_text_color = Some(TEXT);

    v.widgets.noninteractive.bg_fill = BG_PANEL;
    v.widgets.noninteractive.weak_bg_fill = BG_PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BG_WINDOW);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.noninteractive.corner_radius = r;

    v.widgets.inactive.bg_fill = WIDGET;
    v.widgets.inactive.weak_bg_fill = WIDGET;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x3d, 0x3d, 0x3d));
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.corner_radius = r;

    v.widgets.hovered.bg_fill = WIDGET_HOVER;
    v.widgets.hovered.weak_bg_fill = WIDGET_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x70, 0x70, 0x70));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.hovered.corner_radius = r;
    v.widgets.hovered.expansion = 0.0;

    v.widgets.active.bg_fill = ACCENT_BLUE;
    v.widgets.active.weak_bg_fill = ACCENT_BLUE;
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_BLUE);
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.corner_radius = r;
    v.widgets.active.expansion = 0.0;

    v.widgets.open.bg_fill = Color32::from_rgb(0x28, 0x28, 0x28);
    v.widgets.open.weak_bg_fill = Color32::from_rgb(0x28, 0x28, 0x28);
    v.widgets.open.corner_radius = r;

    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, v);

    ctx.all_styles_mut(|style| {
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(10.5)),
        (TextStyle::Body, FontId::proportional(12.5)),
        (TextStyle::Button, FontId::proportional(12.5)),
        (TextStyle::Heading, FontId::proportional(14.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 3.0);
    style.spacing.interact_size.y = 20.0;
    style.spacing.slider_width = 150.0;
    });
}

/// Bouton « primaire » bleu, comme les opérateurs mis en avant dans Blender.
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let b = egui::Button::new(egui::RichText::new(text).color(Color32::WHITE).strong())
        .fill(ACCENT_BLUE)
        .min_size(egui::vec2(ui.available_width(), 26.0));
    ui.add(b)
}

/// Sous-panneau repliable façon panneau de propriétés Blender.
pub fn section<R>(ui: &mut egui::Ui, title: &str, open: bool, add: impl FnOnce(&mut egui::Ui) -> R) {
    egui::Frame::new()
        .fill(BG_SUBPANEL)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::CollapsingHeader::new(egui::RichText::new(title).strong())
                .default_open(open)
                .show(ui, |ui| {
                    ui.add_space(2.0);
                    add(ui);
                });
        });
    ui.add_space(4.0);
}
