//! Orvalith: editor de imagens desktop, offline e sem login.

mod app;
mod theme;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui_viewport(),
        ..Default::default()
    };
    eframe::run_native(
        "Orvalith",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::OrvalithApp::default()))
        }),
    )
}

fn egui_viewport() -> eframe::egui::ViewportBuilder {
    eframe::egui::ViewportBuilder::default()
        .with_title("Orvalith")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([800.0, 500.0])
}
