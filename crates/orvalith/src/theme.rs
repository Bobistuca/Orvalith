//! Tema Orvalith: cinzas neutros escuros, acento turquesa mineral, detalhe cobre.

use eframe::egui::{self, Color32};

pub const ACCENT: Color32 = Color32::from_rgb(0x2f, 0xb8, 0xa6);
pub const COPPER: Color32 = Color32::from_rgb(0xc9, 0x7b, 0x4a);
const BG: Color32 = Color32::from_rgb(0x1e, 0x20, 0x22);
const PANEL: Color32 = Color32::from_rgb(0x26, 0x29, 0x2b);

pub fn apply(ctx: &egui::Context, dark: bool, high_contrast: bool) {
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
    if high_contrast {
        // Texto e bordas com contraste máximo; seleção e foco bem visíveis.
        let (fg, bg) = if dark {
            (Color32::WHITE, Color32::BLACK)
        } else {
            (Color32::BLACK, Color32::WHITE)
        };
        visuals.override_text_color = Some(fg);
        visuals.panel_fill = bg;
        visuals.window_fill = bg;
        visuals.extreme_bg_color = bg;
        visuals.faint_bg_color = if dark {
            Color32::from_gray(24)
        } else {
            Color32::from_gray(235)
        };
        visuals.selection.bg_fill = Color32::from_rgb(0xff, 0xd8, 0x00);
        visuals.selection.stroke = egui::Stroke::new(2.0, fg);
        visuals.hyperlink_color = Color32::from_rgb(0x66, 0xcc, 0xff);
        for w in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
        ] {
            w.bg_stroke = egui::Stroke::new(1.5, fg);
            w.fg_stroke = egui::Stroke::new(1.5, fg);
        }
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x30, 0x30, 0x30);
    }
    ctx.set_visuals(visuals);
}
