//! Documento aberto e canvas: xadrez de transparência, zoom, pan, entrada de ferramentas.

use eframe::egui::{self, Color32, CursorIcon, PointerButton, Rect, Sense, Stroke};
use std::collections::HashMap;

use orvalith_core::compose::RenderOpts;
use orvalith_core::edit::{LayersEdit, PixelPatch};
use orvalith_core::geom::Rect as PixRect;
use orvalith_core::view::View;
use orvalith_core::{compose, Document, History, Layer, LayerId};

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
    /// Arrasto de opacidade/preenchimento em andamento: (rótulo, camadas antes).
    pub prop_drag: Option<(String, Vec<Layer>)>,
    /// Camadas selecionadas no painel (múltipla seleção).
    pub selected: Vec<LayerId>,
    /// Isolamento temporário de visibilidade (não entra no histórico nem no arquivo).
    pub isolated: Option<LayerId>,
    pub thumbs: HashMap<LayerId, egui::TextureHandle>,
    pub thumbs_rev: u64,
    /// Renomeação em andamento no painel.
    pub renaming: Option<(LayerId, String)>,
    /// Máscara rápida: um documento cinza (255 = selecionado) editado com as ferramentas de pintura.
    pub qm: Option<Box<OpenDoc>>,
    sel_edges: Option<(usize, Vec<[f32; 4]>)>,
    /// Retângulo envolvente da seleção atual (para a barra de status).
    pub sel_info: Option<PixRect>,
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
            prop_drag: None,
            selected: vec![active_layer],
            isolated: None,
            thumbs: HashMap::new(),
            thumbs_rev: 0,
            renaming: None,
            qm: None,
            sel_edges: None,
            sel_info: None,
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

    /// Executa um comando que não altera pixels (seleção, canais): não suja o documento
    /// nem força o reenvio da textura.
    pub fn execute_nonpixel(&mut self, cmd: Box<dyn orvalith_core::Command>) {
        let partial_ok = self.tracked_rev == self.doc.revision;
        let was_modified = self.doc.modified;
        self.history.execute(&mut self.doc, cmd);
        self.doc.modified = was_modified;
        if partial_ok {
            self.tracked_rev = self.doc.revision;
            self.dirty.get_or_insert(PixRect::new(0, 0, 0, 0));
        }
    }

    /// Entra na máscara rápida a partir da seleção atual (sem seleção = tudo selecionado).
    pub fn enter_quick_mask(&mut self) {
        let (w, h) = (self.doc.width, self.doc.height);
        let mut px = Vec::with_capacity(w as usize * h as usize * 4);
        for i in 0..w as usize * h as usize {
            let v = self.doc.selection.mask.as_ref().map_or(255, |m| m[i]);
            px.extend_from_slice(&[v, v, v, 255]);
        }
        if let Ok(mut d) = Document::new("Máscara rápida", w, h, None) {
            d.layers[0] = Layer::from_pixels("Máscara", w, h, px);
            self.qm = Some(Box::new(OpenDoc::new(d)));
        }
    }

    /// Sai da máscara rápida e devolve a máscara editada (None = tudo selecionado).
    pub fn leave_quick_mask(&mut self) -> Option<Option<Vec<u8>>> {
        let qm = self.qm.take()?;
        let mask: Vec<u8> = qm.doc.layers[0]
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0])
            .collect();
        Some(if mask.iter().all(|&v| v == 255) {
            None
        } else {
            orvalith_core::selection::nonempty(mask)
        })
    }

    /// Registra uma troca de camadas já aplicada ao documento (ex.: mover), exibida com
    /// atualização parcial da textura em `rect`.
    pub fn record_layers(&mut self, label: &str, before: Vec<Layer>, rect: PixRect) {
        let partial_ok = self.tracked_rev == self.doc.revision;
        let after = self.doc.layers.clone();
        self.history.record(
            &mut self.doc,
            Box::new(LayersEdit::new(label, before, after)),
        );
        if partial_ok {
            self.dirty = Some(self.dirty.map_or(rect, |d| d.union(rect)));
            self.tracked_rev = self.doc.revision;
        }
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

/// Forma de seleção em construção, em coordenadas do documento.
#[derive(Clone)]
pub enum Shape {
    Rect((f32, f32), (f32, f32)),
    Ellipse((f32, f32), (f32, f32)),
    Poly(Vec<(f32, f32)>),
}

/// Informações de desenho vindas da ferramenta atual.
#[derive(Default, Clone)]
pub struct Overlay {
    /// Forma de seleção em construção (contorno tracejado).
    pub shape: Option<Shape>,
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
    pub shift: bool,
    /// Duplo clique do botão primário neste quadro.
    pub double_click: bool,
    /// Pressão da caneta (0..=1) quando o sistema a informa; 1.0 com mouse.
    pub pressure: f32,
}

/// Desenha o canvas, trata navegação e devolve os eventos de ponteiro para as ferramentas.
pub fn show(
    ui: &mut egui::Ui,
    open: &mut OpenDoc,
    cmd: Option<ViewCmd>,
    space_down: bool,
    overlay: &Overlay,
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
    let (pressed, down, released, pts, (alt, shift, dbl), force, latest) = ui.input(|i| {
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
            (
                i.modifiers.alt,
                i.modifiers.shift,
                i.pointer.button_double_clicked(PointerButton::Primary),
            ),
            force,
            i.pointer.latest_pos(),
        )
    });
    out.alt = alt;
    out.shift = shift;
    out.double_click = dbl;
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
        sync_texture(ui.ctx(), open, "doc", &render_composite);
        let full_uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        if let Some((_, tex)) = &open.texture {
            painter.image(tex.id(), img_rect, full_uv, Color32::WHITE);
        }
        if let Some(qm) = open.qm.as_deref_mut() {
            sync_texture(ui.ctx(), qm, "qmask", &render_quick_mask);
            if let Some((_, tex)) = &qm.texture {
                painter.image(tex.id(), img_rect, full_uv, Color32::WHITE);
            }
        }
    }

    let phase = (ui.input(|i| i.time) * 24.0) as f32;
    let animate = open.doc.selection.mask.is_some() || overlay.shape.is_some();
    if open.qm.is_none() {
        draw_selection(&painter, open, &view, rect, phase);
    }
    if let Some(shape) = &overlay.shape {
        draw_shape(&painter, shape, &view, rect, phase);
    }
    if animate {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(80));
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
/// `render` produz os pixels RGBA da região pedida.
fn sync_texture(
    ctx: &egui::Context,
    open: &mut OpenDoc,
    name: &str,
    render: &dyn Fn(&OpenDoc, PixRect) -> Vec<u8>,
) {
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
            let rgba = render(open, r);
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
        let rgba = render(open, PixRect::full(w, h));
        let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
        let tex = ctx.load_texture(name, image, texture_options());
        open.texture = Some((rev, tex));
        open.dirty = None;
    }
}

fn render_composite(o: &OpenDoc, r: PixRect) -> Vec<u8> {
    let opts = RenderOpts {
        isolate: o.isolated,
    };
    compose::flatten_region_with(&o.doc, r, opts)
}

/// Sobreposição vermelha (50%) sobre as áreas NÃO selecionadas da máscara rápida.
fn render_quick_mask(o: &OpenDoc, r: PixRect) -> Vec<u8> {
    let layer = &o.doc.layers[0];
    let r = r.clamp_to(layer.width, layer.height);
    let mut out = Vec::with_capacity(r.w as usize * r.h as usize * 4);
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            let v = layer.pixels[(y as usize * layer.width as usize + x as usize) * 4];
            out.extend_from_slice(&[255, 0, 0, (255 - v) / 2]);
        }
    }
    out
}

/// Segmentos (x0, y0, x1, y1) da borda da seleção (limiar 50%), com trechos retos fundidos.
pub fn selection_edges(mask: &[u8], w: u32, h: u32) -> Vec<[f32; 4]> {
    const CAP: usize = 400_000;
    let inside = |x: i64, y: i64| -> bool {
        x >= 0
            && y >= 0
            && x < i64::from(w)
            && y < i64::from(h)
            && mask[y as usize * w as usize + x as usize] >= 128
    };
    let mut segs = Vec::new();
    for y in 0..=i64::from(h) {
        let mut start: Option<i64> = None;
        for x in 0..=i64::from(w) {
            let edge = x < i64::from(w) && inside(x, y - 1) != inside(x, y);
            match (edge, start) {
                (true, None) => start = Some(x),
                (false, Some(s)) => {
                    segs.push([s as f32, y as f32, x as f32, y as f32]);
                    start = None;
                }
                _ => {}
            }
        }
        if segs.len() > CAP {
            return segs;
        }
    }
    for x in 0..=i64::from(w) {
        let mut start: Option<i64> = None;
        for y in 0..=i64::from(h) {
            let edge = y < i64::from(h) && inside(x - 1, y) != inside(x, y);
            match (edge, start) {
                (true, None) => start = Some(y),
                (false, Some(s)) => {
                    segs.push([x as f32, s as f32, x as f32, y as f32]);
                    start = None;
                }
                _ => {}
            }
        }
        if segs.len() > CAP {
            return segs;
        }
    }
    segs
}

fn ants(painter: &egui::Painter, pts: &[egui::Pos2], phase: f32) {
    let shadow = Stroke::new(1.5, Color32::BLACK);
    let light = Stroke::new(1.5, Color32::WHITE);
    painter.add(egui::Shape::line(pts.to_vec(), shadow));
    painter.extend(egui::Shape::dashed_line_with_offset(
        pts,
        light,
        &[4.0],
        &[4.0],
        phase,
    ));
}

fn draw_selection(
    painter: &egui::Painter,
    open: &mut OpenDoc,
    view: &View,
    rect: Rect,
    phase: f32,
) {
    let Some(mask) = open.doc.selection.mask.clone() else {
        open.sel_info = None;
        open.sel_edges = None;
        return;
    };
    let key = std::sync::Arc::as_ptr(&mask) as usize;
    if open.sel_edges.as_ref().map(|(k, _)| *k) != Some(key) {
        let edges = selection_edges(&mask, open.doc.width, open.doc.height);
        open.sel_edges = Some((key, edges));
        open.sel_info = orvalith_core::selection::bounds(&mask, open.doc.width, open.doc.height);
    }
    let to_screen = |x: f32, y: f32| {
        rect.min
            + egui::vec2(
                view.offset[0] + x * view.zoom,
                view.offset[1] + y * view.zoom,
            )
    };
    if let Some((_, edges)) = &open.sel_edges {
        let mut drawn = 0;
        for e in edges {
            let (a, b) = (to_screen(e[0], e[1]), to_screen(e[2], e[3]));
            if !rect.intersects(Rect::from_two_pos(a, b).expand(1.0)) {
                continue;
            }
            ants(painter, &[a, b], phase);
            drawn += 1;
            if drawn > 20_000 {
                break;
            }
        }
    }
}

fn draw_shape(painter: &egui::Painter, shape: &Shape, view: &View, rect: Rect, phase: f32) {
    let to_screen = |p: (f32, f32)| {
        rect.min
            + egui::vec2(
                view.offset[0] + p.0 * view.zoom,
                view.offset[1] + p.1 * view.zoom,
            )
    };
    let pts: Vec<egui::Pos2> = match shape {
        Shape::Rect(a, b) => vec![
            to_screen(*a),
            to_screen((b.0, a.1)),
            to_screen(*b),
            to_screen((a.0, b.1)),
            to_screen(*a),
        ],
        Shape::Ellipse(a, b) => {
            let (cx, cy) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let (rx, ry) = ((b.0 - a.0).abs() / 2.0, (b.1 - a.1).abs() / 2.0);
            (0..=64)
                .map(|i| {
                    let t = i as f32 / 64.0 * std::f32::consts::TAU;
                    to_screen((cx + rx * t.cos(), cy + ry * t.sin()))
                })
                .collect()
        }
        Shape::Poly(p) => p.iter().map(|q| to_screen(*q)).collect(),
    };
    if pts.len() >= 2 {
        ants(painter, &pts, phase);
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
