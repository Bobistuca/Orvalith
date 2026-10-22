//! Tema Orvalith: cinzas neutros escuros, acento turquesa mineral, detalhe cobre.

use eframe::egui::{self, Color32};

pub const ACCENT: Color32 = Color32::from_rgb(0x2f, 0xb8, 0xa6);
pub const COPPER: Color32 = Color32::from_rgb(0xc9, 0x7b, 0x4a);
const BG: Color32 = Color32::from_rgb(0x1e, 0x20, 0x22);
const PANEL: Color32 = Color32::from_rgb(0x26, 0x29, 0x2b);

pub fn apply(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        let mut v = egui::Visuals::dark();
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.extreme_bg_color = BG;
        v
    } else {
        egui::Visuals::light()
    };
    visuals.selection.bg_fill = ACCENT.linear_multiply(0.45);
    visuals.hyperlink_color = ACCENT;
    ctx.set_visuals(visuals);
}
