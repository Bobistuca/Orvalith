//! Orvalith: editor de imagens desktop, offline e sem login.

mod app;
mod canvas;
mod layers_ui;
mod prefs;
mod theme;
mod tools;

fn main() -> eframe::Result<()> {
    let prefs = prefs::Prefs::load();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Orvalith")
            .with_inner_size(prefs.window_size)
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Orvalith",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, prefs.dark_theme);
            cc.egui_ctx.set_zoom_factor(prefs.ui_scale);
            Ok(Box::new(app::OrvalithApp::new(prefs)))
        }),
    )
}
