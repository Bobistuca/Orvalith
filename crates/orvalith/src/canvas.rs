//! Documento aberto e canvas: xadrez de transparência, zoom, pan e navegação.

use eframe::egui::{self, Color32, CursorIcon, PointerButton, Rect, Sense};
use orvalith_core::view::View;
use orvalith_core::{compose, Document, History};

/// Maior lado suportado pela textura de visualização nesta fase (sem tiles ainda).
const MAX_TEXTURE_SIDE: u32 = 8192;
const FIT_MARGIN: f32 = 16.0;
const STEP: f32 = 1.25;

/// Documento aberto com histórico e estado de visualização.
pub struct OpenDoc {
    pub doc: Document,
    #[allow(dead_code)] // ligado à UI no Estágio 2
    pub history: History,
    texture: Option<(u64, egui::TextureHandle)>,
    view: Option<View>,
    /// Enquanto verdadeiro, o documento é reajustado à janela quando ela muda de tamanho.
    fit_mode: bool,
    /// Zoom exibido em porcentagem de pixels físicos (para a barra de status).
    pub zoom_percent: f32,
}

impl OpenDoc {
    pub fn new(doc: Document) -> Self {
        Self {
            doc,
            history: History::new(),
            texture: None,
            view: None,
            fit_mode: true,
            zoom_percent: 100.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewCmd {
    Fit,
    Actual,
    ZoomIn,
    ZoomOut,
}

/// Desenha o canvas e trata navegação. Retorna a posição do ponteiro em coordenadas do documento.
pub fn show(
    ui: &mut egui::Ui,
    open: &mut OpenDoc,
    cmd: Option<ViewCmd>,
    space_down: bool,
) -> Option<[f32; 2]> {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let ppp = ui.ctx().pixels_per_point();
    let vp = [rect.width(), rect.height()];
    let dims = [open.doc.width, open.doc.height];

    match cmd {
        Some(ViewCmd::Fit) => open.fit_mode = true,
        Some(ViewCmd::Actual) => {
            open.view = Some(View::actual_size(dims, vp, ppp));
            open.fit_mode = false;
        }
        Some(c @ (ViewCmd::ZoomIn | ViewCmd::ZoomOut)) => {
            let f = if c == ViewCmd::ZoomIn {
                STEP
            } else {
                1.0 / STEP
            };
            if let Some(v) = open.view.as_mut() {
                v.zoom_at([vp[0] / 2.0, vp[1] / 2.0], f);
                open.fit_mode = false;
            }
        }
        None => {}
    }
    if open.fit_mode || open.view.is_none() {
        open.view = Some(View::fit(dims, vp, FIT_MARGIN, 1.0 / ppp));
    }
    let mut view = open
        .view
        .unwrap_or_else(|| View::fit(dims, vp, FIT_MARGIN, 1.0));

    // Zoom com roda/pinça ancorado no ponteiro.
    if resp.hovered() {
        let (scroll, zd, ptr) = ui.input(|i| {
            (
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                i.pointer.hover_pos(),
            )
        });
        let factor = zd * (scroll * 0.0015).exp();
        if (factor - 1.0).abs() > 1e-4 {
            if let Some(p) = ptr {
                view.zoom_at([p.x - rect.min.x, p.y - rect.min.y], factor);
                open.fit_mode = false;
            }
        }
    }
    // Pan: botão do meio, ou Espaço + arrastar.
    let panning = resp.dragged_by(PointerButton::Middle)
        || (space_down && resp.dragged_by(PointerButton::Primary));
    if panning {
        let d = resp.drag_delta();
        view.pan([d.x, d.y]);
        open.fit_mode = false;
    }
    if panning {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if resp.hovered() && space_down {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    open.view = Some(view);
    open.zoom_percent = view.zoom * ppp * 100.0;

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let img_rect = Rect::from_min_size(
        rect.min + egui::vec2(view.offset[0], view.offset[1]),
        egui::vec2(dims[0] as f32 * view.zoom, dims[1] as f32 * view.zoom),
    );
    draw_checkerboard(&painter, img_rect, rect);

    if dims[0].max(dims[1]) > MAX_TEXTURE_SIDE {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Documento grande demais para visualização nesta versão (limite 8192 px por lado).",
            egui::FontId::proportional(14.0),
            Color32::LIGHT_RED,
        );
    } else {
        if open.texture.as_ref().map(|(r, _)| *r) != Some(open.doc.revision) {
            let rgba = compose::flatten(&open.doc);
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [dims[0] as usize, dims[1] as usize],
                &rgba,
            );
            let opts = egui::TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Linear,
                ..Default::default()
            };
            let tex = ui.ctx().load_texture("doc", image, opts);
            open.texture = Some((open.doc.revision, tex));
        }
        if let Some((_, tex)) = &open.texture {
            painter.image(
                tex.id(),
                img_rect,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    resp.hover_pos()
        .map(|p| view.view_to_doc([p.x - rect.min.x, p.y - rect.min.y]))
}

/// Xadrez de 8 pontos desenhado só na parte visível do documento.
fn draw_checkerboard(painter: &egui::Painter, img: Rect, viewport: Rect) {
    let vis = img.intersect(viewport);
    if vis.width() <= 0.0 || vis.height() <= 0.0 {
        return;
    }
    let cell: f32 = 8.0;
    painter.rect_filled(vis, 0.0, Color32::from_gray(200));
    let dark = Color32::from_gray(150);
    let c0 = ((vis.min.x - img.min.x) / cell).floor() as i32;
    let c1 = ((vis.max.x - img.min.x) / cell).ceil() as i32;
    let r0 = ((vis.min.y - img.min.y) / cell).floor() as i32;
    let r1 = ((vis.max.y - img.min.y) / cell).ceil() as i32;
    for r in r0..r1 {
        for c in c0..c1 {
            if (r + c).rem_euclid(2) == 1 {
                let min = img.min + egui::vec2(c as f32 * cell, r as f32 * cell);
                let cell_rect = Rect::from_min_size(min, egui::vec2(cell, cell)).intersect(vis);
                if cell_rect.width() > 0.0 && cell_rect.height() > 0.0 {
                    painter.rect_filled(cell_rect, 0.0, dark);
                }
            }
        }
    }
}
