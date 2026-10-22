//! Tema Orvalith: cinzas neutros escuros, acento turquesa mineral, detalhe cobre.

use eframe::egui::{self, Color32};

pub const ACCENT: Color32 = Color32::from_rgb(0x2f, 0xb8, 0xa6);
#[allow(dead_code)] // detalhe cobre reservado para estágios seguintes
pub const COPPER: Color32 = Color32::from_rgb(0xc9, 0x7b, 0x4a);
pub const BG: Color32 = Color32::from_rgb(0x1e, 0x20, 0x22);
pub const PANEL: Color32 = Color32::from_rgb(0x26, 0x29, 0x2b);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BG;
    visuals.selection.bg_fill = ACCENT.linear_multiply(0.45);
    visuals.hyperlink_color = ACCENT;
    ctx.set_visuals(visuals);
}
