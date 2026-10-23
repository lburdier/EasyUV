#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod icons;
mod io;
mod news;
mod raster;
mod scene;
mod theme;
mod update;
mod uvview;
mod viewport;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(format!("EasyUV {} — dépliage UV intelligent", env!("CARGO_PKG_VERSION")))
            .with_inner_size([1440.0, 880.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true),
        multisampling: 4,
        ..Default::default()
    };
    eframe::run_native("EasyUV", options, Box::new(|cc| Ok(Box::new(app::EasyUvApp::new(cc)))))
}
