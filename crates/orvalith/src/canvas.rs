//! Documento aberto e canvas: xadrez de transparência, zoom, pan, entrada de ferramentas.

use eframe::egui::{self, Color32, CursorIcon, PointerButton, Rect, Sense, Stroke};
use orvalith_core::edit::PixelPatch;
use orvalith_core::geom::Rect as PixRect;
use orvalith_core::view::View;
use orvalith_core::{compose, Document, History, LayerId};

/// Maior lado suportado pela textura de visualização nesta fase (sem tiles ainda).
const MAX_TEXTURE_SIDE: u32 = 8192;
const FIT_MARGIN: f32 = 16.0;
const STEP: f32 = 1.25;

fn texture_options() -> egui::TextureOptions {
    egui::TextureOptions {
        magnification: egui::TextureFilter::Nearest,
        minification: egui::TextureFilter::Linear,
        ..Default::default()
    }
}

/// Documento aberto com histórico e estado de visualização.
pub struct OpenDoc {
    pub doc: Document,
    pub history: History,
    /// Caminho do projeto `.orva` salvo (None = nunca salvo).
    pub path: Option<std::path::PathBuf>,
    /// Arrasto de opacidade em andamento: (camada, valor original).
    pub opacity_drag: Option<(LayerId, f32)>,
    /// Última revisão gravada na cópia de recuperação.
    pub autosaved_rev: Option<u64>,
    /// Camada que recebe pintura.
    pub active_layer: LayerId,
    texture: Option<(u64, egui::TextureHandle)>,
    /// Região alterada desde a última sincronização da textura (atualização parcial).
    dirty: Option<PixRect>,
    /// Revisão atingida só por alterações registradas em `dirty`.
    tracked_rev: u64,
    view: Option<View>,
    /// Enquanto verdadeiro, o documento é reajustado à janela quando ela muda de tamanho.
    fit_mode: bool,
    /// Zoom exibido em porcentagem de pixels físicos (para a barra de status).
    pub zoom_percent: f32,
}

impl OpenDoc {
    pub fn new(doc: Document) -> Self {
        let active_layer = doc.layers.last().map(|l| l.id).unwrap_or(LayerId(0));
        Self {
            doc,
            history: History::new(),
            path: None,
            opacity_drag: None,
            autosaved_rev: None,
            active_layer,
            texture: None,
            dirty: None,
            tracked_rev: 0,
            view: None,
            fit_mode: true,
            zoom_percent: 100.0,
        }
    }

    /// Índice da camada ativa (cai para a mais alta se o ID não existir mais).
    pub fn active_idx(&self) -> Option<usize> {
        self.doc
            .layers
            .iter()
            .position(|l| l.id == self.active_layer)
            .or_else(|| self.doc.layers.len().checked_sub(1))
    }

    /// Marca uma região alterada diretamente nos pixels (pré-visualização de traço etc.):
    /// avança a revisão, mas a textura só precisa atualizar esta região.
    pub fn mark_dirty(&mut self, rect: PixRect) {
        self.doc.touch();
        self.dirty = Some(self.dirty.map_or(rect, |d| d.union(rect)));
        self.tracked_rev = self.doc.revision;
    }

    /// Registra no histórico uma alteração de pixels já exibida (sem reenviar a textura toda).
    pub fn record_patch(&mut self, patch: PixelPatch) {
        let partial_ok = self.tracked_rev == self.doc.revision;
        let rect = patch.rect();
        self.history.record(&mut self.doc, Box::new(patch));
        if partial_ok {
            self.dirty = Some(self.dirty.map_or(rect, |d| d.union(rect)));
            self.tracked_rev = self.doc.revision;
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

/// Informações de desenho vindas da ferramenta atual.
#[derive(Default, Clone, Copy)]
pub struct Overlay {
    /// A ferramenta atual usa o cursor de mira.
    pub tool_cursor: bool,
    /// Raio do pincel em pixels do documento (para o contorno sob o cursor).
    pub brush_radius_doc: Option<f32>,
}

/// Eventos de ponteiro do quadro, já em coordenadas do documento.
#[derive(Default)]
pub struct CanvasOut {
    pub hover: Option<[f32; 2]>,
    /// Botão primário pressionado neste quadro (início de traço).
    pub pressed: Option<[f32; 2]>,
    /// Amostras de movimento deste quadro (cada evento do sistema, não só a última posição).
    pub moves: Vec<[f32; 2]>,
    pub down: bool,
    pub released: bool,
    pub alt: bool,
    /// Pressão da caneta (0..=1) quando o sistema a informa; 1.0 com mouse.
    pub pressure: f32,
}

/// Desenha o canvas, trata navegação e devolve os eventos de ponteiro para as ferramentas.
pub fn show(
    ui: &mut egui::Ui,
    open: &mut OpenDoc,
    cmd: Option<ViewCmd>,
    space_down: bool,
    overlay: Overlay,
) -> CanvasOut {
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
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if resp.hovered() && space_down {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    } else if resp.hovered() && overlay.tool_cursor {
        ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
    }
    open.view = Some(view);
    open.zoom_percent = view.zoom * ppp * 100.0;

    let to_doc = |p: egui::Pos2| view.view_to_doc([p.x - rect.min.x, p.y - rect.min.y]);
    let mut out = CanvasOut {
        pressure: 1.0,
        hover: resp.hover_pos().map(to_doc),
        ..Default::default()
    };
    let inside = ui.rect_contains_pointer(rect) && resp.hovered();
    let (pressed, down, released, pts, alt, force, latest) = ui.input(|i| {
        let pts: Vec<egui::Pos2> = i
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::PointerMoved(p) => Some(*p),
                _ => None,
            })
            .collect();
        let force = i.events.iter().rev().find_map(|e| match e {
            egui::Event::Touch { force, .. } => *force,
            _ => None,
        });
        (
            i.pointer.button_pressed(PointerButton::Primary),
            i.pointer.button_down(PointerButton::Primary),
            i.pointer.button_released(PointerButton::Primary),
            pts,
            i.modifiers.alt,
            force,
            i.pointer.latest_pos(),
        )
    });
    out.alt = alt;
    out.down = down;
    out.released = released;
    if let Some(f) = force {
        out.pressure = f.clamp(0.0, 1.0);
    }
    if pressed && inside && !space_down && !panning {
        out.pressed = latest.map(to_doc);
    } else if down && !space_down {
        out.moves = pts.into_iter().map(to_doc).collect();
        if out.moves.is_empty() {
            out.moves.extend(latest.map(to_doc));
        }
    }

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
        sync_texture(ui.ctx(), open);
        if let Some((_, tex)) = &open.texture {
            painter.image(
                tex.id(),
                img_rect,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    if let (Some(r), Some(p)) = (overlay.brush_radius_doc, resp.hover_pos()) {
        let rr = r * view.zoom;
        if rr >= 2.0 && !space_down {
            painter.circle_stroke(p, rr, Stroke::new(2.0, Color32::BLACK.gamma_multiply(0.6)));
            painter.circle_stroke(p, rr, Stroke::new(1.0, Color32::WHITE));
        }
    }
    out
}

/// Atualiza a textura: só a região alterada quando possível, ou tudo se a mudança foi geral.
fn sync_texture(ctx: &egui::Context, open: &mut OpenDoc) {
    let rev = open.doc.revision;
    if open.texture.as_ref().map(|(r, _)| *r) == Some(rev) {
        return;
    }
    let (w, h) = (open.doc.width, open.doc.height);
    let partial = open.texture.is_some() && open.dirty.is_some() && open.tracked_rev == rev;
    if partial {
        let r = open
            .dirty
            .take()
            .unwrap_or(PixRect::full(w, h))
            .clamp_to(w, h);
        if !r.is_empty() {
            let rgba = compose::flatten_region(&open.doc, r);
            let image =
                egui::ColorImage::from_rgba_unmultiplied([r.w as usize, r.h as usize], &rgba);
            if let Some((trev, tex)) = open.texture.as_mut() {
                tex.set_partial([r.x as usize, r.y as usize], image, texture_options());
                *trev = rev;
            }
        } else if let Some((trev, _)) = open.texture.as_mut() {
            *trev = rev;
        }
    } else {
        let rgba = compose::flatten(&open.doc);
        let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
        let tex = ctx.load_texture("doc", image, texture_options());
        open.texture = Some((rev, tex));
        open.dirty = None;
    }
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
