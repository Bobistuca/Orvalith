//! Tema Orvalith: cinzas neutros escuros, acento turquesa mineral, detalhe cobre.

use eframe::egui::{self, Color32};

pub const ACCENT: Color32 = Color32::from_rgb(0x2f, 0xb8, 0xa6);
pub const COPPER: Color32 = Color32::from_rgb(0xc9, 0x7b, 0x4a);
const BG: Color32 = Color32::from_rgb(0x1e, 0x20, 0x22);
const PANEL: Color32 = Color32::from_rgb(0x26, 0x29, 0x2b);

/// Valores de espaçamento, raio e tipografia compartilhados pelas telas redesenhadas (px lógicos).
pub mod metrics {
    pub const SPACE_XS: f32 = 4.0;
    pub const SPACE_SM: f32 = 8.0;
    pub const SPACE_MD: f32 = 12.0;
    pub const SPACE_LG: f32 = 16.0;
    /// Margem externa das áreas de conteúdo.
    pub const SPACE_XL: f32 = 24.0;
    pub const RADIUS_SM: f32 = 4.0;
    pub const RADIUS: f32 = 6.0;
    pub const RADIUS_LG: f32 = 8.0;
    pub const SIDEBAR_W: f32 = 248.0;
    pub const CONTROL_H: f32 = 34.0;
    pub const ICON: f32 = 16.0;
    pub const FONT_TITLE: f32 = 26.0;
    pub const FONT_HEADING: f32 = 16.0;
    pub const FONT_BODY: f32 = 14.0;
    pub const FONT_SMALL: f32 = 12.0;
    pub const STROKE: f32 = 1.0;
    pub const FOCUS_STROKE: f32 = 2.0;
}

/// Cores semânticas das telas redesenhadas, nos temas escuro, claro e de alto contraste.
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub surface: Color32,
    pub raised: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_pressed: Color32,
    pub on_accent: Color32,
    pub warn: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            bg: Color32::from_rgb(0x1b, 0x1d, 0x1f),
            surface: Color32::from_rgb(0x23, 0x26, 0x28),
            raised: Color32::from_rgb(0x2b, 0x2f, 0x32),
            hover: Color32::from_rgb(0x34, 0x39, 0x3d),
            pressed: Color32::from_rgb(0x3d, 0x43, 0x48),
            border: Color32::from_rgb(0x3a, 0x3f, 0x43),
            text: Color32::from_rgb(0xe8, 0xea, 0xec),
            muted: Color32::from_rgb(0xa3, 0xaa, 0xaf),
            faint: Color32::from_rgb(0x7a, 0x82, 0x88),
            accent: ACCENT,
            accent_hover: Color32::from_rgb(0x3d, 0xc9, 0xb7),
            accent_pressed: Color32::from_rgb(0x25, 0x9b, 0x8b),
            on_accent: Color32::from_rgb(0x0b, 0x1f, 0x1c),
            warn: Color32::from_rgb(0xe0, 0xa4, 0x58),
        }
    }

    pub fn light() -> Self {
        Self {
            bg: Color32::from_rgb(0xee, 0xef, 0xf1),
            surface: Color32::from_rgb(0xf7, 0xf8, 0xf9),
            raised: Color32::WHITE,
            hover: Color32::from_rgb(0xe3, 0xe6, 0xe8),
            pressed: Color32::from_rgb(0xd5, 0xd9, 0xdc),
            border: Color32::from_rgb(0xcc, 0xd0, 0xd4),
            text: Color32::from_rgb(0x1f, 0x23, 0x26),
            muted: Color32::from_rgb(0x55, 0x5d, 0x63),
            faint: Color32::from_rgb(0x7a, 0x82, 0x88),
            accent: Color32::from_rgb(0x1c, 0x8f, 0x80),
            accent_hover: Color32::from_rgb(0x17, 0x7d, 0x70),
            accent_pressed: Color32::from_rgb(0x12, 0x68, 0x5d),
            on_accent: Color32::WHITE,
            warn: Color32::from_rgb(0xa8, 0x5f, 0x0a),
        }
    }

    pub fn high_contrast(dark: bool) -> Self {
        let (fg, bg) = if dark {
            (Color32::WHITE, Color32::BLACK)
        } else {
            (Color32::BLACK, Color32::WHITE)
        };
        let step = if dark {
            Color32::from_gray(28)
        } else {
            Color32::from_gray(232)
        };
        Self {
            bg,
            surface: bg,
            raised: step,
            hover: if dark {
                Color32::from_gray(48)
            } else {
                Color32::from_gray(210)
            },
            pressed: if dark {
                Color32::from_gray(70)
            } else {
                Color32::from_gray(190)
            },
            border: fg,
            text: fg,
            muted: fg,
            faint: fg,
            accent: Color32::from_rgb(0xff, 0xd8, 0x00),
            accent_hover: Color32::from_rgb(0xff, 0xe4, 0x4d),
            accent_pressed: Color32::from_rgb(0xd9, 0xb7, 0x00),
            on_accent: Color32::BLACK,
            warn: Color32::from_rgb(0xff, 0xb0, 0x40),
        }
    }

    /// Paleta do tema ativo (lida das visuais já aplicadas por [`apply`]).
    pub fn of(ui: &egui::Ui) -> Self {
        let v = ui.visuals();
        if v.override_text_color.is_some() {
            Self::high_contrast(v.dark_mode)
        } else if v.dark_mode {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

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
    // Segue a preferência do aplicativo, não o tema do sistema operacional.
    let theme = if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(theme);
    ctx.set_visuals_of(theme, visuals);
}
