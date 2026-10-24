//! Widgets reutilizáveis do visual redesenhado: botões, itens de navegação, cartões e texto truncado.
//! Estados: normal, hover, pressionado, selecionado, desabilitado e foco de teclado.

use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Galley, Rect, Response, Role, Sense, Stroke,
    StrokeKind, TextWrapMode, Ui, Vec2, WidgetInfo,
};

use crate::icons::{self, Icon};
use crate::theme::{metrics as m, Palette};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Ação principal da tela (preenchida com o acento).
    Primary,
    /// Ação secundária (superfície elevada com borda).
    Secondary,
    /// Ação discreta (sem fundo até o hover).
    Ghost,
}

/// Texto numa linha só, cortado com "…" quando passa de `max_w`.
pub fn fit_text(ui: &Ui, text: &str, font: FontId, color: Color32, max_w: f32) -> Arc<Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping {
        max_width: max_w.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// Anel de foco de teclado (2 px, cor de acento) ao redor de `rect`.
pub fn focus_ring(ui: &Ui, rect: Rect, radius: f32) {
    let pal = Palette::of(ui);
    ui.painter().rect_stroke(
        rect.expand(1.0),
        radius + 1.0,
        Stroke::new(m::FOCUS_STROKE, pal.accent),
        StrokeKind::Outside,
    );
}

fn keyboard_focused(ui: &Ui, r: &Response) -> bool {
    r.has_focus() && ui.memory(|mem| mem.focused() == Some(r.id))
}

/// Botão com ícone opcional, rótulo e dica de atalho à direita. `width` 0 = largura disponível.
pub fn button(
    ui: &mut Ui,
    kind: Kind,
    icon: Option<Icon>,
    label: &str,
    hint: &str,
    width: f32,
    enabled: bool,
) -> Response {
    let pal = Palette::of(ui);
    let w = if width > 0.0 {
        width
    } else {
        ui.available_width()
    };
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, r) = ui.allocate_exact_size(Vec2::new(w, m::CONTROL_H), sense);
    r.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    let down = enabled && r.is_pointer_button_down_on();
    let (fill, border, fg, hint_c) = match (kind, enabled) {
        (_, false) => (
            pal.raised.gamma_multiply(0.5),
            pal.border.gamma_multiply(0.6),
            pal.faint,
            pal.faint,
        ),
        (Kind::Primary, _) => {
            let f = if down {
                pal.accent_pressed
            } else if r.hovered() {
                pal.accent_hover
            } else {
                pal.accent
            };
            (f, f, pal.on_accent, pal.on_accent)
        }
        (Kind::Secondary, _) => {
            let f = if down {
                pal.pressed
            } else if r.hovered() {
                pal.hover
            } else {
                pal.raised
            };
            (f, pal.border, pal.text, pal.muted)
        }
        (Kind::Ghost, _) => {
            let f = if down {
                pal.pressed
            } else if r.hovered() {
                pal.hover
            } else {
                Color32::TRANSPARENT
            };
            (f, Color32::TRANSPARENT, pal.text, pal.muted)
        }
    };
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, m::RADIUS, fill);
        if border != Color32::TRANSPARENT && border != fill {
            p.rect_stroke(
                rect,
                m::RADIUS,
                Stroke::new(m::STROKE, border),
                StrokeKind::Inside,
            );
        }
        let mut x = rect.left() + m::SPACE_MD;
        if let Some(i) = icon {
            let ir = Rect::from_center_size(
                egui::pos2(x + m::ICON / 2.0, rect.center().y),
                Vec2::splat(m::ICON),
            );
            icons::paint(p, ir, i, fg);
            x += m::ICON + m::SPACE_SM;
        }
        let mut right = rect.right() - m::SPACE_MD;
        if !hint.is_empty() {
            let g = p.layout_no_wrap(hint.to_owned(), FontId::proportional(m::FONT_SMALL), hint_c);
            let hw = g.size().x;
            p.galley(
                egui::pos2(right - hw, rect.center().y - g.size().y / 2.0),
                g,
                hint_c,
            );
            right -= hw + m::SPACE_SM;
        }
        let g = fit_text(ui, label, FontId::proportional(m::FONT_BODY), fg, right - x);
        ui.painter()
            .galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, fg);
        if keyboard_focused(ui, &r) {
            focus_ring(ui, rect, m::RADIUS);
        }
    }
    if enabled {
        r.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        r
    }
}

/// Item de navegação lateral (ícone + rótulo); `selected` destaca a seção atual.
pub fn nav_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let pal = Palette::of(ui);
    let (rect, r) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), m::CONTROL_H - 2.0),
        Sense::click(),
    );
    r.widget_info(|| WidgetInfo::selected(Role::Tab, true, selected, label));
    let down = r.is_pointer_button_down_on();
    let fill = if selected {
        pal.raised
    } else if down {
        pal.pressed
    } else if r.hovered() {
        pal.hover
    } else {
        Color32::TRANSPARENT
    };
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, m::RADIUS, fill);
        if selected {
            // Marca de seleção: barra de acento à esquerda.
            p.rect_filled(
                Rect::from_min_size(
                    rect.left_top() + Vec2::new(0.0, 7.0),
                    Vec2::new(3.0, rect.height() - 14.0),
                ),
                2.0,
                pal.accent,
            );
        }
        let fg = if selected { pal.text } else { pal.muted };
        let ir = Rect::from_center_size(
            egui::pos2(rect.left() + m::SPACE_MD + m::ICON / 2.0, rect.center().y),
            Vec2::splat(m::ICON),
        );
        icons::paint(p, ir, icon, if selected { pal.accent } else { fg });
        let x = ir.right() + m::SPACE_SM + 2.0;
        let g = fit_text(
            ui,
            label,
            FontId::proportional(m::FONT_BODY),
            fg,
            rect.right() - x - m::SPACE_SM,
        );
        ui.painter()
            .galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, fg);
        if keyboard_focused(ui, &r) {
            focus_ring(ui, rect, m::RADIUS);
        }
    }
    r.on_hover_cursor(CursorIcon::PointingHand)
}

/// Título de seção sem espaço abaixo (para usar numa linha com ações à direita).
pub fn section_heading_inline(ui: &mut Ui, text: &str) {
    let pal = Palette::of(ui);
    ui.label(
        egui::RichText::new(text)
            .size(m::FONT_HEADING)
            .strong()
            .color(pal.text),
    );
}

/// Texto secundário (cinza, corpo pequeno).
pub fn muted(ui: &mut Ui, text: &str) {
    let pal = Palette::of(ui);
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(m::FONT_SMALL)
                .color(pal.muted),
        )
        .wrap_mode(TextWrapMode::Wrap),
    );
}

/// Superfície de cartão clicável: devolve a resposta e o retângulo para desenhar o conteúdo.
pub fn card(ui: &mut Ui, size: Vec2, label: &str) -> (Response, Rect, bool) {
    let pal = Palette::of(ui);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    r.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
    let down = r.is_pointer_button_down_on();
    let hovered = r.hovered() || keyboard_focused(ui, &r);
    if ui.is_rect_visible(rect) {
        let fill = if down {
            pal.pressed
        } else if hovered {
            pal.hover
        } else {
            pal.raised
        };
        let border = if hovered { pal.accent } else { pal.border };
        let p = ui.painter();
        p.rect_filled(rect, m::RADIUS_LG, fill);
        p.rect_stroke(
            rect,
            m::RADIUS_LG,
            Stroke::new(m::STROKE, border),
            StrokeKind::Inside,
        );
        if keyboard_focused(ui, &r) {
            focus_ring(ui, rect, m::RADIUS_LG);
        }
    }
    (r.on_hover_cursor(CursorIcon::PointingHand), rect, hovered)
}

/// Texto alinhado numa posição (ajuda para cartões e linhas).
pub fn put_text(
    ui: &Ui,
    pos: egui::Pos2,
    anchor: Align2,
    text: &str,
    size: f32,
    color: Color32,
    max_w: f32,
) {
    let g = fit_text(ui, text, FontId::proportional(size), color, max_w);
    let r = anchor.anchor_size(pos, g.size());
    ui.painter().galley(r.min, g, color);
}
