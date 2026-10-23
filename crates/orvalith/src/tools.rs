//! Ferramentas de pintura e cor: estado, tratamento de entrada e interface de opções.
//!
//! Toda a lógica de pixels está em `orvalith-core`; aqui apenas se interpreta a entrada do
//! ponteiro e se conectam traço, balde e gradiente ao histórico do documento.

use std::sync::Arc;

use eframe::egui;
use orvalith_core::edit::{MaskPatch, PixelPatch};
use orvalith_core::fill::{bucket_fill, Gradient, GradientKind};
use orvalith_core::geom::Rect;
use orvalith_core::layerops;
use orvalith_core::paint::{builtin_presets, BrushParams, BrushPreset, StrokeMode, StrokeSession};
use orvalith_core::{compose, DocumentId, Layer, LayerId};

use crate::canvas::{CanvasOut, OpenDoc, Overlay};
use crate::geometry::{self, GeoState};
use crate::select::{is_selection_tool, SelectState};
use crate::vectool::VecState;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Move,
    Text,
    Shape,
    Pen,
    Node,
    Crop,
    Marquee,
    Ellipse,
    Lasso,
    PolyLasso,
    Wand,
    Brush,
    Pencil,
    Eraser,
    Eyedropper,
    Bucket,
    Gradient,
}

impl Tool {
    pub const ALL: [Tool; 17] = [
        Tool::Move,
        Tool::Text,
        Tool::Shape,
        Tool::Pen,
        Tool::Node,
        Tool::Crop,
        Tool::Marquee,
        Tool::Ellipse,
        Tool::Lasso,
        Tool::PolyLasso,
        Tool::Wand,
        Tool::Brush,
        Tool::Pencil,
        Tool::Eraser,
        Tool::Eyedropper,
        Tool::Bucket,
        Tool::Gradient,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Move => "Mover",
            Tool::Text => "Texto",
            Tool::Shape => "Forma",
            Tool::Pen => "Caneta",
            Tool::Node => "Seleção de nós",
            Tool::Crop => "Recortar",
            Tool::Marquee => "Seleção retangular",
            Tool::Ellipse => "Seleção elíptica",
            Tool::Lasso => "Laço",
            Tool::PolyLasso => "Laço poligonal",
            Tool::Wand => "Varinha mágica",
            Tool::Brush => "Pincel",
            Tool::Pencil => "Lápis",
            Tool::Eraser => "Borracha",
            Tool::Eyedropper => "Conta-gotas",
            Tool::Bucket => "Balde de tinta",
            Tool::Gradient => "Gradiente",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Tool::Move => "V",
            Tool::Text => "T",
            Tool::Shape => "U",
            Tool::Pen => "P",
            Tool::Node => "A",
            Tool::Crop => "C",
            Tool::Marquee => "M",
            Tool::Ellipse => "Shift+M",
            Tool::Lasso => "L",
            Tool::PolyLasso => "Shift+L",
            Tool::Wand => "W",
            Tool::Brush => "B",
            Tool::Pencil => "N",
            Tool::Eraser => "E",
            Tool::Eyedropper => "I",
            Tool::Bucket => "G",
            Tool::Gradient => "Shift+G",
        }
    }

    fn paints_strokes(self) -> bool {
        matches!(self, Tool::Brush | Tool::Pencil | Tool::Eraser)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GradientFill {
    ForegroundToBackground,
    ForegroundToTransparent,
}

enum Active {
    Stroke {
        session: Box<StrokeSession>,
        doc: DocumentId,
        layer: LayerId,
    },
    Gradient {
        doc: DocumentId,
        layer: LayerId,
        original: Arc<Vec<u8>>,
        start: (f32, f32),
        drawn: bool,
    },
    Move {
        doc: DocumentId,
        before: Vec<Layer>,
        targets: Vec<LayerId>,
        start: (f32, f32),
        delta: (i32, i32),
    },
    /// Pintura na máscara da camada: uma camada-sombra cinza recebe as ferramentas.
    MaskEdit {
        doc: DocumentId,
        layer: LayerId,
        temp: Box<Layer>,
        original: Arc<Vec<u8>>,
        stroke: Option<Box<StrokeSession>>,
        grad_start: Option<(f32, f32)>,
    },
}

pub struct ToolState {
    pub tool: Tool,
    pub brush: BrushParams,
    pub fg: [u8; 4],
    pub bg: [u8; 4],
    pub tolerance: u8,
    pub contiguous: bool,
    pub fill_opacity: f32,
    pub gradient_kind: GradientKind,
    pub gradient_fill: GradientFill,
    pub gradient_reverse: bool,
    pub gradient_opacity: f32,
    pub sample_composite: bool,
    pub pressure_size: bool,
    pub notice: Option<String>,
    pub sel: SelectState,
    pub geo: GeoState,
    pub vec: VecState,
    preset_name: String,
    active: Option<Active>,
}

const PALETTE: [[u8; 4]; 16] = [
    [0, 0, 0, 255],
    [255, 255, 255, 255],
    [128, 128, 128, 255],
    [200, 200, 200, 255],
    [220, 40, 40, 255],
    [240, 140, 30, 255],
    [245, 220, 50, 255],
    [60, 170, 70, 255],
    [47, 184, 166, 255],
    [60, 190, 230, 255],
    [50, 90, 220, 255],
    [130, 70, 200, 255],
    [220, 70, 170, 255],
    [201, 123, 74, 255],
    [120, 80, 50, 255],
    [30, 40, 60, 255],
];

/// Converte uma cor para cinza (luminância) mantendo o alfa.
pub fn gray(c: [u8; 4]) -> [u8; 4] {
    let l = (0.3 * f32::from(c[0]) + 0.59 * f32::from(c[1]) + 0.11 * f32::from(c[2])).round() as u8;
    [l, l, l, c[3]]
}

/// Motivo pelo qual a camada não pode receber pintura, se houver.
pub(crate) fn paint_blocked(l: &Layer) -> Option<String> {
    if l.is_group() {
        Some("Selecione uma camada raster (não um grupo) para pintar.".into())
    } else if l.source.is_some() {
        Some("Camada de texto/forma: use Camada → Rasterizar antes de pintar.".into())
    } else if !l.visible {
        Some("A camada ativa está oculta.".into())
    } else if l.lock_pixels {
        Some("Os pixels da camada ativa estão bloqueados.".into())
    } else {
        None
    }
}

fn to32(c: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn pct_slider(ui: &mut egui::Ui, label: &str, v: &mut f32, min_pct: f32) {
    let mut pct = *v * 100.0;
    ui.label(label);
    if ui
        .add(
            egui::Slider::new(&mut pct, min_pct..=100.0)
                .suffix("%")
                .integer(),
        )
        .changed()
    {
        *v = pct / 100.0;
    }
}

impl ToolState {
    pub fn new(brush: BrushParams, fg: [u8; 4], bg: [u8; 4]) -> Self {
        Self {
            tool: Tool::Brush,
            brush: brush.sanitized(),
            fg,
            bg,
            tolerance: 32,
            contiguous: true,
            fill_opacity: 1.0,
            gradient_kind: GradientKind::Linear,
            gradient_fill: GradientFill::ForegroundToBackground,
            gradient_reverse: false,
            gradient_opacity: 1.0,
            sample_composite: true,
            pressure_size: true,
            notice: None,
            sel: SelectState::default(),
            geo: GeoState::default(),
            vec: VecState::default(),
            preset_name: String::new(),
            active: None,
        }
    }

    /// Há um traço ou gradiente em andamento (troca de ferramenta/aba é adiada).
    pub fn is_busy(&self) -> bool {
        self.active.is_some() || self.geo.busy() || self.vec.busy()
    }

    /// Enter tem algo a confirmar (transformação, recorte ou laço poligonal).
    pub fn needs_confirm(&self) -> bool {
        self.geo.busy() || self.geo.crop.active() || self.sel.active() || self.vec.busy()
    }

    pub fn set_tool(&mut self, tool: Tool) {
        if !self.is_busy() {
            self.tool = tool;
        }
    }

    pub fn swap_colors(&mut self) {
        std::mem::swap(&mut self.fg, &mut self.bg);
    }

    pub fn default_colors(&mut self) {
        self.fg = [0, 0, 0, 255];
        self.bg = [255, 255, 255, 255];
    }

    pub fn resize_brush(&mut self, factor: f32) {
        let s = self.brush.size;
        let step = if factor > 1.0 {
            (s * 0.1).max(1.0)
        } else {
            -(s * 0.1).max(1.0)
        };
        self.brush.size = (s + step).clamp(1.0, 2000.0);
    }

    pub fn overlay(&self, hover: Option<[f32; 2]>, open_for_overlay: Option<&OpenDoc>) -> Overlay {
        if let Some(t) = &self.geo.transform {
            return Overlay {
                shape: Some(t.shape()),
                points: t.points(),
                tool_cursor: true,
                snap_pointer: true,
                ..Overlay::default()
            };
        }
        let vec_overlay = match self.tool {
            Tool::Text => open_for_overlay
                .and_then(|o| self.vec.text_overlay(o))
                .map(|s| (s, Vec::new())),
            Tool::Shape => self
                .vec
                .shape_preview(self.fg, self.bg, false)
                .map(|s| (s, Vec::new())),
            Tool::Pen => self.vec.pen_shape(),
            Tool::Node => open_for_overlay.and_then(|o| self.vec.node_overlay(o)),
            _ => None,
        };
        if let Some((shape, points)) = vec_overlay {
            return Overlay {
                shape: Some(shape),
                points,
                tool_cursor: true,
                snap_pointer: matches!(self.tool, Tool::Shape | Tool::Pen),
                ..Overlay::default()
            };
        }
        if self.tool == Tool::Crop {
            return Overlay {
                shape: self.geo.crop.shape(),
                points: self.geo.crop.points(),
                tool_cursor: true,
                snap_pointer: true,
                ..Overlay::default()
            };
        }
        Overlay {
            shape: self.sel.shape(self.tool, hover),
            tool_cursor: true,
            snap_pointer: is_selection_tool(self.tool),
            brush_radius_doc: self.tool.paints_strokes().then_some(self.brush.size / 2.0),
            ..Overlay::default()
        }
    }

    /// Cancela um gradiente em andamento restaurando os pixels originais (tecla Esc).
    pub fn cancel(&mut self, open: &mut OpenDoc) {
        self.vec.cancel_text(open);
        self.vec.cancel_pen();
        self.geo.cancel(open);
        self.sel.cancel(open);
        match self.active.take() {
            Some(Active::Gradient {
                layer, original, ..
            }) => {
                if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == layer) {
                    if l.pixels.len() == original.len() {
                        l.pixels = original;
                        let r = Rect::full(l.width, l.height);
                        open.mark_dirty(r);
                    }
                }
            }
            Some(Active::Move { before, .. }) => {
                open.doc.layers = before;
                let r = Rect::full(open.doc.width, open.doc.height);
                open.mark_dirty(r);
            }
            Some(Active::MaskEdit {
                layer, original, ..
            }) => {
                // Restaura a máscara a partir do instantâneo gerado no início do gesto.
                if let Some(m) = open.doc.layer_mut(layer).and_then(|l| l.mask.as_mut()) {
                    let orig: Vec<u8> = original.as_chunks::<4>().0.iter().map(|p| p[0]).collect();
                    if orig.len() == m.data().len() {
                        m.set_data(Arc::new(orig));
                    }
                }
                let r = Rect::full(open.doc.width, open.doc.height);
                open.mark_dirty(r);
            }
            other => self.active = other,
        }
    }

    /// Confirma a operação em andamento (Enter): fecha o laço poligonal.
    pub fn confirm(&mut self, open: &mut OpenDoc) {
        if self.vec.busy() {
            let (fg, bg) = (self.fg, self.bg);
            self.vec.commit_all(open, fg, bg);
        } else if self.geo.busy() {
            self.geo.commit(open);
        } else if self.geo.crop.active() {
            self.notice = geometry::apply_crop(open, &self.geo.crop);
            self.geo.crop.cancel();
        } else {
            self.sel.confirm(open);
        }
    }

    /// Interpreta os eventos de ponteiro do quadro para a ferramenta atual.
    pub fn handle(&mut self, open: &mut OpenDoc, out: &CanvasOut) {
        let doc_id = open.doc.id;
        let foreign = match &self.active {
            Some(Active::Stroke { doc, .. })
            | Some(Active::Gradient { doc, .. })
            | Some(Active::Move { doc, .. })
            | Some(Active::MaskEdit { doc, .. }) => *doc != doc_id,
            None => false,
        };
        if foreign {
            self.active = None;
        }
        if let Some(t) = self.geo.transform.as_mut() {
            t.handle(open, out);
            return;
        }
        let mask_tools = matches!(
            self.tool,
            Tool::Brush | Tool::Pencil | Tool::Eraser | Tool::Bucket | Tool::Gradient
        );
        if mask_tools
            && (self.mask_editing(open) || matches!(self.active, Some(Active::MaskEdit { .. })))
        {
            self.handle_mask_edit(open, out);
            return;
        }
        match self.tool {
            Tool::Crop => self.geo.crop.handle(out),
            Tool::Text => self.vec.handle_text(open, out, self.fg),
            Tool::Shape => self.vec.handle_shape(open, out, self.fg, self.bg),
            Tool::Pen => self.vec.handle_pen(open, out, self.fg, self.bg),
            Tool::Node => self.vec.handle_node(open, out),
            Tool::Marquee | Tool::Ellipse | Tool::Lasso | Tool::PolyLasso | Tool::Wand => {
                self.sel.handle(self.tool, open, out)
            }
            Tool::Move => self.handle_move(open, out),
            Tool::Brush | Tool::Pencil | Tool::Eraser => self.handle_stroke(open, out),
            Tool::Eyedropper => self.handle_eyedropper(open, out),
            Tool::Bucket => self.handle_bucket(open, out),
            Tool::Gradient => self.handle_gradient(open, out),
        }
    }

    fn handle_move(&mut self, open: &mut OpenDoc, out: &CanvasOut) {
        let (open_w, open_h) = (open.doc.width, open.doc.height);
        if let Some(p) = out.pressed {
            let sel = open.selection_ids();
            let unlocked: Vec<LayerId> = sel
                .into_iter()
                .filter(|id| open.doc.layer(*id).is_some_and(|l| !l.lock_position))
                .collect();
            let targets: Vec<LayerId> = layerops::movable_rasters(&open.doc, &unlocked)
                .into_iter()
                .filter(|id| {
                    open.doc
                        .layer(*id)
                        .is_some_and(|l| !l.lock_position && !l.lock_pixels)
                })
                .collect();
            if targets.is_empty() {
                self.notice =
                    Some("Nenhuma camada movível (bloqueada, vazia ou grupo sem camadas).".into());
                return;
            }
            self.notice = None;
            self.active = Some(Active::Move {
                doc: open.doc.id,
                before: open.doc.layers.clone(),
                targets,
                start: (p[0], p[1]),
                delta: (0, 0),
            });
        }
        if let Some(Active::Move {
            before,
            targets,
            start,
            delta,
            ..
        }) = self.active.as_mut()
        {
            if let Some(m) = out.moves.last() {
                let d = (
                    (m[0] - start.0).round() as i32,
                    (m[1] - start.1).round() as i32,
                );
                if d != *delta {
                    *delta = d;
                    for id in targets.iter() {
                        let (Some(orig), Some(cur)) =
                            (before.iter().find(|l| l.id == *id), open.doc.layer_mut(*id))
                        else {
                            continue;
                        };
                        if orig.source.is_some() {
                            cur.source = orig.source.clone();
                            orvalith_core::vecops::translate_source(
                                cur, d.0 as f32, d.1 as f32, open_w, open_h,
                            );
                        } else {
                            cur.pixels = Arc::new(layerops::shifted_pixels(orig, d.0, d.1));
                        }
                        if let (Some(cm), Some(om)) = (cur.mask.as_mut(), orig.mask.as_ref()) {
                            if cm.linked {
                                let nd = orvalith_core::selection::shift_mask(
                                    om.data(),
                                    open_w,
                                    open_h,
                                    d.0,
                                    d.1,
                                    255,
                                );
                                cm.set_data(Arc::new(nd));
                            }
                        }
                    }
                    let r = Rect::full(open.doc.width, open.doc.height);
                    open.mark_dirty(r);
                }
            }
        }
        if (out.released || !out.down) && matches!(self.active, Some(Active::Move { .. })) {
            if let Some(Active::Move { before, delta, .. }) = self.active.take() {
                if delta == (0, 0) {
                    open.doc.layers = before;
                } else {
                    let r = Rect::full(open.doc.width, open.doc.height);
                    open.record_layers("Mover camada", before, r);
                }
            }
        }
    }

    /// A pintura vai para a máscara da camada ativa?
    fn mask_editing(&self, open: &OpenDoc) -> bool {
        open.edit_mask
            && open
                .active_idx()
                .is_some_and(|i| open.doc.layers[i].mask.is_some())
    }

    /// Copia a área `rect` da camada-sombra cinza para a máscara real (prévia ao vivo).
    fn sync_mask(open: &mut OpenDoc, layer: LayerId, temp: &Layer, rect: Rect) {
        let w = temp.width as usize;
        let rect = rect.clamp_to(temp.width, temp.height);
        if let Some(m) = open.doc.layer_mut(layer).and_then(|l| l.mask.as_mut()) {
            if m.data().len() == w * temp.height as usize {
                let buf = m.data_mut();
                for y in rect.y..rect.bottom() {
                    for x in rect.x..rect.right() {
                        buf[y as usize * w + x as usize] =
                            temp.pixels[(y as usize * w + x as usize) * 4];
                    }
                }
            }
        }
        open.mark_dirty(rect);
    }

    /// Ferramentas de pintura aplicadas à máscara: preto oculta, branco revela.
    fn handle_mask_edit(&mut self, open: &mut OpenDoc, out: &CanvasOut) {
        let pressure = if self.pressure_size {
            out.pressure
        } else {
            1.0
        };
        let (w, h) = (open.doc.width, open.doc.height);
        if let Some(p) = out.pressed {
            let Some(idx) = open.active_idx() else { return };
            let layer_id = open.doc.layers[idx].id;
            let Some(data) = open.doc.layers[idx].mask.as_ref().map(|m| m.data().clone()) else {
                return;
            };
            if data.len() != w as usize * h as usize {
                return;
            }
            self.notice = None;
            let gray_px: Vec<u8> = data.iter().flat_map(|&v| [v, v, v, 255]).collect();
            let mut temp = Layer::from_pixels("máscara", w, h, gray_px);
            let original = temp.pixels.clone();
            let sel = open.doc.selection.mask.clone();
            let (fg, bg) = (gray(self.fg), gray(self.bg));
            match self.tool {
                Tool::Bucket => {
                    let patch = bucket_fill(
                        &mut temp,
                        p[0].floor() as i64,
                        p[1].floor() as i64,
                        self.tolerance,
                        self.contiguous,
                        fg,
                        self.fill_opacity,
                        false,
                        sel.as_ref().map(|m| m.as_slice()),
                    );
                    if let Some(patch) = patch {
                        let rect = patch.rect();
                        Self::sync_mask(open, layer_id, &temp, rect);
                        let cmd = MaskPatch::from_gray_patch("Balde na máscara", layer_id, &patch);
                        open.record_cmd(Box::new(cmd), rect);
                    } else {
                        self.notice = Some("Nada a preencher neste ponto da máscara.".into());
                    }
                }
                Tool::Gradient => {
                    self.active = Some(Active::MaskEdit {
                        doc: open.doc.id,
                        layer: layer_id,
                        temp: Box::new(temp),
                        original,
                        stroke: None,
                        grad_start: Some((p[0], p[1])),
                    });
                }
                _ => {
                    let (color, pencil, label) = match self.tool {
                        Tool::Pencil => (fg, true, "Lápis na máscara"),
                        Tool::Eraser => (bg, false, "Borracha na máscara"),
                        _ => (fg, false, "Pincel na máscara"),
                    };
                    let mut session = Box::new(
                        StrokeSession::begin(
                            &temp,
                            self.brush,
                            color,
                            StrokeMode::Paint,
                            pencil,
                            label,
                        )
                        .with_selection(sel),
                    );
                    if let Some(r) = session.add_point(&mut temp, p[0], p[1], pressure) {
                        Self::sync_mask(open, layer_id, &temp, r);
                    }
                    self.active = Some(Active::MaskEdit {
                        doc: open.doc.id,
                        layer: layer_id,
                        temp: Box::new(temp),
                        original,
                        stroke: Some(session),
                        grad_start: None,
                    });
                }
            }
        } else if matches!(self.active, Some(Active::MaskEdit { .. })) {
            let grad_colors = (gray(self.fg), gray(self.bg));
            let (old_fg, old_bg) = (self.fg, self.bg);
            self.fg = grad_colors.0;
            self.bg = grad_colors.1;
            let g = match &self.active {
                Some(Active::MaskEdit {
                    grad_start: Some(s),
                    ..
                }) => out.moves.last().map(|m| self.gradient(*s, (m[0], m[1]))),
                _ => None,
            };
            self.fg = old_fg;
            self.bg = old_bg;
            let sel = open.doc.selection.mask.clone();
            if let Some(Active::MaskEdit {
                layer,
                temp,
                original,
                stroke,
                ..
            }) = self.active.as_mut()
            {
                if let Some(session) = stroke.as_mut() {
                    let mut dirty: Option<Rect> = None;
                    for m in &out.moves {
                        if let Some(r) = session.add_point(temp, m[0], m[1], pressure) {
                            dirty = Some(dirty.map_or(r, |d| d.union(r)));
                        }
                    }
                    if let Some(r) = dirty {
                        Self::sync_mask(open, *layer, temp, r);
                    }
                } else if let Some(g) = g {
                    if g.render(temp, original, false, sel.as_ref().map(|m| m.as_slice())) {
                        Self::sync_mask(open, *layer, temp, Rect::full(w, h));
                    }
                }
            }
        }
        if (out.released || !out.down) && matches!(self.active, Some(Active::MaskEdit { .. })) {
            if let Some(Active::MaskEdit {
                layer,
                mut temp,
                original,
                stroke,
                ..
            }) = self.active.take()
            {
                let (patch, label) = match stroke {
                    Some(session) => (session.finish(&mut temp), "Pintar máscara"),
                    None => (
                        PixelPatch::from_diff("Gradiente", &temp, &original, Rect::full(w, h)),
                        "Gradiente na máscara",
                    ),
                };
                if let Some(patch) = patch {
                    let rect = patch.rect();
                    Self::sync_mask(open, layer, &temp, rect);
                    let cmd = MaskPatch::from_gray_patch(label, layer, &patch);
                    open.record_cmd(Box::new(cmd), rect);
                }
            }
        }
    }

    fn handle_stroke(&mut self, open: &mut OpenDoc, out: &CanvasOut) {
        let pressure = if self.pressure_size {
            out.pressure
        } else {
            1.0
        };
        if let Some(p) = out.pressed {
            let Some(idx) = open.active_idx() else { return };
            if let Some(msg) = paint_blocked(&open.doc.layers[idx]) {
                self.notice = Some(msg);
                return;
            }
            self.notice = None;
            let (mode, pencil, label) = match self.tool {
                Tool::Pencil => (StrokeMode::Paint, true, "Lápis"),
                Tool::Eraser => (StrokeMode::Erase, false, "Borracha"),
                _ => (StrokeMode::Paint, false, "Pincel"),
            };
            let mut session = Box::new(
                StrokeSession::begin(
                    &open.doc.layers[idx],
                    self.brush,
                    self.fg,
                    mode,
                    pencil,
                    label,
                )
                .with_alpha_lock(open.doc.layers[idx].lock_alpha)
                .with_selection(open.doc.selection.mask.clone()),
            );

            let layer = &mut open.doc.layers[idx];
            let layer_id = layer.id;
            let r = session.add_point(layer, p[0], p[1], pressure);
            if let Some(r) = r {
                open.mark_dirty(r);
            }
            self.active = Some(Active::Stroke {
                session,
                doc: open.doc.id,
                layer: layer_id,
            });
        } else if let Some(Active::Stroke { session, layer, .. }) = self.active.as_mut() {
            if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == *layer) {
                let mut dirty: Option<Rect> = None;
                for m in &out.moves {
                    if let Some(r) = session.add_point(l, m[0], m[1], pressure) {
                        dirty = Some(dirty.map_or(r, |d| d.union(r)));
                    }
                }
                if let Some(r) = dirty {
                    open.mark_dirty(r);
                }
            }
        }
        if (out.released || !out.down) && matches!(self.active, Some(Active::Stroke { .. })) {
            if let Some(Active::Stroke { session, layer, .. }) = self.active.take() {
                if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == layer) {
                    if let Some(patch) = session.finish(l) {
                        open.mark_dirty(patch.rect());
                        open.record_patch(patch);
                    }
                }
            }
        }
    }

    fn handle_eyedropper(&mut self, open: &mut OpenDoc, out: &CanvasOut) {
        let Some(p) = out.pressed.or(if out.down {
            out.moves.last().copied()
        } else {
            None
        }) else {
            return;
        };
        if p[0] < 0.0 || p[1] < 0.0 {
            return;
        }
        let (x, y) = (p[0] as u32, p[1] as u32);
        let sampled = if self.sample_composite {
            compose::pixel_at(&open.doc, x, y)
        } else {
            open.active_idx()
                .and_then(|i| open.doc.layers[i].pixel(x, y))
        };
        match sampled {
            Some(c) if c[3] > 0 => {
                if out.alt {
                    self.bg = c;
                } else {
                    self.fg = c;
                }
                self.notice = None;
            }
            Some(_) => self.notice = Some("Pixel transparente: nenhuma cor amostrada.".into()),
            None => {}
        }
    }

    fn handle_bucket(&mut self, open: &mut OpenDoc, out: &CanvasOut) {
        let Some(p) = out.pressed else { return };
        let Some(idx) = open.active_idx() else { return };
        if let Some(msg) = paint_blocked(&open.doc.layers[idx]) {
            self.notice = Some(msg);
            return;
        }
        let lock_alpha = open.doc.layers[idx].lock_alpha;
        let sel_mask = open.doc.selection.mask.clone();
        let patch: Option<PixelPatch> = bucket_fill(
            &mut open.doc.layers[idx],
            p[0].floor() as i64,
            p[1].floor() as i64,
            self.tolerance,
            self.contiguous,
            self.fg,
            self.fill_opacity,
            lock_alpha,
            sel_mask.as_ref().map(|m| m.as_slice()),
        );
        match patch {
            Some(patch) => {
                self.notice = None;
                open.mark_dirty(patch.rect());
                open.record_patch(patch);
            }
            None => self.notice = Some("Nada a preencher neste ponto.".into()),
        }
    }

    fn gradient(&self, start: (f32, f32), end: (f32, f32)) -> Gradient {
        let (mut from, mut to) = match self.gradient_fill {
            GradientFill::ForegroundToBackground => (self.fg, self.bg),
            GradientFill::ForegroundToTransparent => {
                (self.fg, [self.fg[0], self.fg[1], self.fg[2], 0])
            }
        };
        if self.gradient_reverse {
            std::mem::swap(&mut from, &mut to);
        }
        Gradient {
            kind: self.gradient_kind,
            start,
            end,
            from,
            to,
            opacity: self.gradient_opacity,
        }
    }

    fn handle_gradient(&mut self, open: &mut OpenDoc, out: &CanvasOut) {
        if let Some(p) = out.pressed {
            let Some(idx) = open.active_idx() else { return };
            if let Some(msg) = paint_blocked(&open.doc.layers[idx]) {
                self.notice = Some(msg);
                return;
            }
            self.notice = None;
            self.active = Some(Active::Gradient {
                doc: open.doc.id,
                layer: open.doc.layers[idx].id,
                original: open.doc.layers[idx].pixels.clone(),
                start: (p[0], p[1]),
                drawn: false,
            });
        }
        let Some(last) = out.moves.last().copied() else {
            self.finish_gradient(open, out);
            return;
        };
        let sel_mask = open.doc.selection.mask.clone();
        let preview = match &self.active {
            Some(Active::Gradient { start, .. }) => Some(self.gradient(*start, (last[0], last[1]))),
            _ => None,
        };
        if let (
            Some(g),
            Some(Active::Gradient {
                layer,
                original,
                drawn,
                ..
            }),
        ) = (preview, self.active.as_mut())
        {
            if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == *layer) {
                if g.render(
                    l,
                    original,
                    l.lock_alpha,
                    sel_mask.as_ref().map(|m| m.as_slice()),
                ) {
                    *drawn = true;
                    let r = Rect::full(l.width, l.height);
                    open.mark_dirty(r);
                }
            }
        }
        self.finish_gradient(open, out);
    }

    fn finish_gradient(&mut self, open: &mut OpenDoc, out: &CanvasOut) {
        if !(out.released || !out.down) {
            return;
        }
        if let Some(Active::Gradient {
            layer,
            original,
            drawn,
            ..
        }) = self.active.take()
        {
            if !drawn {
                return;
            }
            if let Some(l) = open.doc.layers.iter().find(|l| l.id == layer) {
                let rect = Rect::full(l.width, l.height);
                if let Some(patch) = PixelPatch::from_diff("Gradiente", l, &original, rect) {
                    open.record_patch(patch);
                }
            }
        }
    }

    // ---- Interface ----

    /// Barra de opções da ferramenta atual. Retorna `true` se presets mudaram.
    pub fn options_ui(
        &mut self,
        ui: &mut egui::Ui,
        user_presets: &mut Vec<BrushPreset>,
        open_for_text: Option<&mut OpenDoc>,
    ) -> bool {
        let mut presets_changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.strong(self.tool.label());
            ui.separator();
            match self.tool {
                Tool::Text => self.vec.text_options_ui(ui, open_for_text),
                Tool::Shape => self.vec.shape_options_ui(ui),
                Tool::Pen => self.vec.pen_options_ui(ui),
                Tool::Node => VecState::node_options_ui(ui),
                Tool::Crop => {
                    ui.label("Arraste a área; alças ajustam; Enter recorta (a área pode passar das bordas); Esc cancela. Camadas ficam recortadas à tela.");
                }
                Tool::Marquee | Tool::Ellipse | Tool::Lasso | Tool::PolyLasso | Tool::Wand => {
                    self.sel.options_ui(ui, self.tool)
                }
                Tool::Move => {
                    ui.label("Arraste para mover a(s) camada(s) selecionada(s). Pixels que saem da imagem são descartados; bloqueio de posição impede o movimento.");
                }
                Tool::Brush | Tool::Pencil | Tool::Eraser => {
                    ui.label("Tamanho");
                    ui.add(egui::Slider::new(&mut self.brush.size, 1.0..=500.0).logarithmic(true).suffix(" px"));
                    if self.tool != Tool::Pencil {
                        pct_slider(ui, "Dureza", &mut self.brush.hardness, 0.0);
                    }
                    pct_slider(ui, "Opacidade", &mut self.brush.opacity, 1.0);
                    if self.tool != Tool::Pencil {
                        pct_slider(ui, "Fluxo", &mut self.brush.flow, 1.0);
                    }
                    pct_slider(ui, "Espaçamento", &mut self.brush.spacing, 1.0);
                    let mut sm = self.brush.smoothing / 0.95;
                    pct_slider(ui, "Suavização", &mut sm, 0.0);
                    self.brush.smoothing = sm * 0.95;
                    ui.checkbox(&mut self.pressure_size, "Pressão controla tamanho")
                        .on_hover_text("Só tem efeito se o sistema informar a pressão da caneta; com mouse nada muda.");
                    ui.separator();
                    presets_changed = self.preset_ui(ui, user_presets);
                }
                Tool::Eyedropper => {
                    ui.radio_value(&mut self.sample_composite, true, "Imagem composta");
                    ui.radio_value(&mut self.sample_composite, false, "Camada ativa");
                    ui.label("Alt+clique define a cor de fundo.");
                }
                Tool::Bucket => {
                    ui.label("Tolerância");
                    ui.add(egui::Slider::new(&mut self.tolerance, 0..=255));
                    ui.checkbox(&mut self.contiguous, "Contíguo");
                    pct_slider(ui, "Opacidade", &mut self.fill_opacity, 1.0);
                }
                Tool::Gradient => {
                    ui.radio_value(&mut self.gradient_kind, GradientKind::Linear, "Linear");
                    ui.radio_value(&mut self.gradient_kind, GradientKind::Radial, "Radial");
                    ui.separator();
                    ui.radio_value(
                        &mut self.gradient_fill,
                        GradientFill::ForegroundToBackground,
                        "Frente → Fundo",
                    );
                    ui.radio_value(
                        &mut self.gradient_fill,
                        GradientFill::ForegroundToTransparent,
                        "Frente → Transparente",
                    );
                    ui.checkbox(&mut self.gradient_reverse, "Inverter");
                    pct_slider(ui, "Opacidade", &mut self.gradient_opacity, 1.0);
                }
            }
        });
        self.brush = self.brush.sanitized();
        presets_changed
    }

    fn preset_ui(&mut self, ui: &mut egui::Ui, user: &mut Vec<BrushPreset>) -> bool {
        let mut changed = false;
        egui::ComboBox::from_label("Preset")
            .selected_text("Escolher…")
            .show_ui(ui, |ui| {
                for p in builtin_presets().iter().chain(user.iter()) {
                    if ui.selectable_label(false, &p.name).clicked() {
                        self.brush = p.params;
                    }
                }
            });
        ui.add(
            egui::TextEdit::singleline(&mut self.preset_name)
                .hint_text("Nome do preset")
                .desired_width(110.0),
        );
        if ui
            .add_enabled(
                !self.preset_name.trim().is_empty(),
                egui::Button::new("Salvar preset"),
            )
            .clicked()
        {
            let name = self.preset_name.trim().to_string();
            user.retain(|p| p.name != name);
            user.push(BrushPreset {
                name,
                params: self.brush,
            });
            self.preset_name.clear();
            changed = true;
        }
        if !user.is_empty() && ui.button("Apagar presets salvos").clicked() {
            user.clear();
            changed = true;
        }
        changed
    }

    /// Cores de frente/fundo e amostras. Retorna `true` se as amostras mudaram.
    pub fn colors_ui(&mut self, ui: &mut egui::Ui, swatches: &mut Vec<[u8; 4]>) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            for (label, c) in [("Frente", &mut self.fg), ("Fundo", &mut self.bg)] {
                ui.vertical(|ui| {
                    let mut c32 = to32(*c);
                    if ui.color_edit_button_srgba(&mut c32).changed() {
                        *c = c32.to_srgba_unmultiplied();
                    }
                    ui.small(label);
                });
            }
        });
        ui.horizontal(|ui| {
            if ui.small_button("Trocar (X)").clicked() {
                self.swap_colors();
            }
            if ui.small_button("Padrão (D)").clicked() {
                self.default_colors();
            }
        });
        ui.small("Clique: cor de frente · Shift+clique: cor de fundo");
        let mut clicked = None;
        let all: Vec<[u8; 4]> = PALETTE.iter().chain(swatches.iter()).copied().collect();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
            for c in &all {
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                ui.painter().rect_filled(rect, 2.0, to32(*c));
                ui.painter().rect_stroke(
                    rect,
                    2.0,
                    ui.visuals().widgets.noninteractive.bg_stroke,
                    egui::StrokeKind::Inside,
                );
                if resp.clicked() {
                    clicked = Some((*c, ui.input(|i| i.modifiers.shift)));
                }
            }
            if ui
                .small_button("+")
                .on_hover_text("Adicionar a cor de frente às amostras")
                .clicked()
                && !swatches.contains(&self.fg)
                && !PALETTE.contains(&self.fg)
            {
                swatches.push(self.fg);
                changed = true;
            }
        });
        if let Some((c, shift)) = clicked {
            if shift {
                self.bg = c;
            } else {
                self.fg = c;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_and_defaults() {
        let mut t = ToolState::new(BrushParams::default(), [1, 2, 3, 255], [4, 5, 6, 255]);
        t.swap_colors();
        assert_eq!((t.fg, t.bg), ([4, 5, 6, 255], [1, 2, 3, 255]));
        t.default_colors();
        assert_eq!((t.fg, t.bg), ([0, 0, 0, 255], [255, 255, 255, 255]));
    }

    #[test]
    fn resize_brush_is_clamped_and_monotonic() {
        let mut t = ToolState::new(BrushParams::default(), [0; 4], [0; 4]);
        t.brush.size = 20.0;
        t.resize_brush(1.1);
        assert!(t.brush.size > 20.0);
        t.brush.size = 1.0;
        t.resize_brush(0.9);
        assert_eq!(t.brush.size, 1.0);
    }

    #[test]
    fn tool_keys_are_unique() {
        for (i, a) in Tool::ALL.iter().enumerate() {
            for b in &Tool::ALL[i + 1..] {
                assert_ne!(a.shortcut(), b.shortcut());
            }
        }
    }
    use orvalith_core::Document;

    fn open_doc() -> OpenDoc {
        OpenDoc::new(Document::new("t", 40, 30, Some([255, 255, 255, 255])).unwrap())
    }

    fn ev() -> CanvasOut {
        CanvasOut {
            pressure: 1.0,
            down: true,
            ..Default::default()
        }
    }

    fn drag(t: &mut ToolState, o: &mut OpenDoc, pts: &[[f32; 2]]) {
        t.handle(
            o,
            &CanvasOut {
                pressed: Some(pts[0]),
                ..ev()
            },
        );
        t.handle(
            o,
            &CanvasOut {
                moves: pts[1..].to_vec(),
                ..ev()
            },
        );
        t.handle(
            o,
            &CanvasOut {
                released: true,
                down: false,
                pressure: 1.0,
                ..Default::default()
            },
        );
    }

    #[test]
    fn brush_drag_is_one_history_action_and_undo_restores() {
        let mut o = open_doc();
        let original = o.doc.layers[0].pixels.clone();
        let mut t = ToolState::new(
            BrushParams {
                size: 4.0,
                hardness: 1.0,
                ..BrushParams::default()
            },
            [255, 0, 0, 255],
            [0, 0, 0, 255],
        );
        drag(&mut t, &mut o, &[[2.0, 5.0], [20.0, 5.0], [36.0, 20.0]]);
        assert!(!t.is_busy());
        assert_eq!(o.history.labels(), vec!["Pincel"]);
        assert_eq!(o.doc.layers[0].pixel(20, 5).unwrap(), [255, 0, 0, 255]);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[0].pixels, original);
    }

    #[test]
    fn tool_cannot_change_mid_stroke_and_hidden_layer_blocks_painting() {
        let mut o = open_doc();
        let mut t = ToolState::new(BrushParams::default(), [1, 2, 3, 255], [0; 4]);
        t.handle(
            &mut o,
            &CanvasOut {
                pressed: Some([5.0, 5.0]),
                ..ev()
            },
        );
        assert!(t.is_busy());
        t.set_tool(Tool::Bucket);
        assert_eq!(t.tool, Tool::Brush);
        t.handle(
            &mut o,
            &CanvasOut {
                released: true,
                down: false,
                ..Default::default()
            },
        );
        assert!(!t.is_busy());
        o.doc.layers[0].visible = false;
        let before = o.history.labels().len();
        t.handle(
            &mut o,
            &CanvasOut {
                pressed: Some([9.0, 9.0]),
                ..ev()
            },
        );
        assert!(!t.is_busy() && t.notice.is_some());
        assert_eq!(o.history.labels().len(), before);
    }

    #[test]
    fn eyedropper_samples_composite_alt_sets_background() {
        let mut o = open_doc();
        o.doc.layers[0].set_pixel(3, 3, [10, 20, 30, 255]);
        let mut t = ToolState::new(BrushParams::default(), [0; 4], [0; 4]);
        t.tool = Tool::Eyedropper;
        t.handle(
            &mut o,
            &CanvasOut {
                pressed: Some([3.4, 3.9]),
                ..ev()
            },
        );
        assert_eq!(t.fg, [10, 20, 30, 255]);
        t.handle(
            &mut o,
            &CanvasOut {
                pressed: Some([0.5, 0.5]),
                alt: true,
                ..ev()
            },
        );
        assert_eq!(t.bg, [255, 255, 255, 255]);
    }

    #[test]
    fn bucket_click_fills_and_undoes() {
        let mut o = open_doc();
        let original = o.doc.layers[0].pixels.clone();
        let mut t = ToolState::new(BrushParams::default(), [0, 0, 255, 255], [0; 4]);
        t.tool = Tool::Bucket;
        t.handle(
            &mut o,
            &CanvasOut {
                pressed: Some([10.0, 10.0]),
                released: true,
                down: false,
                pressure: 1.0,
                ..Default::default()
            },
        );
        assert_eq!(o.doc.layers[0].pixel(39, 29).unwrap(), [0, 0, 255, 255]);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[0].pixels, original);
    }

    #[test]
    fn gradient_drag_is_one_action_and_escape_restores() {
        let mut o = open_doc();
        let original = o.doc.layers[0].pixels.clone();
        let mut t = ToolState::new(BrushParams::default(), [255, 0, 0, 255], [0, 0, 255, 255]);
        t.tool = Tool::Gradient;
        drag(&mut t, &mut o, &[[0.0, 15.0], [10.0, 15.0], [39.0, 15.0]]);
        assert_eq!(o.history.labels(), vec!["Gradiente"]);
        assert_ne!(o.doc.layers[0].pixels, original);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[0].pixels, original);
        // Cancelar com Esc no meio do arrasto.
        t.handle(
            &mut o,
            &CanvasOut {
                pressed: Some([0.0, 15.0]),
                ..ev()
            },
        );
        t.handle(
            &mut o,
            &CanvasOut {
                moves: vec![[30.0, 15.0]],
                ..ev()
            },
        );
        assert_ne!(o.doc.layers[0].pixels, original);
        t.cancel(&mut o);
        assert_eq!(o.doc.layers[0].pixels, original);
        assert!(!t.is_busy());
    }
    #[test]
    fn move_tool_shifts_selected_layer_as_one_undoable_action() {
        let mut o = open_doc();
        o.doc.layers[0].set_pixel(0, 0, [9, 9, 9, 255]);
        let original = o.doc.layers.clone();
        let mut t = ToolState::new(BrushParams::default(), [0; 4], [0; 4]);
        t.tool = Tool::Move;
        drag(&mut t, &mut o, &[[5.0, 5.0], [8.0, 7.0], [10.2, 8.1]]);
        assert_eq!(o.history.labels(), vec!["Mover camada"]);
        assert_eq!(o.doc.layers[0].pixel(5, 3).unwrap(), [9, 9, 9, 255]);
        assert_eq!(o.doc.layers[0].pixel(0, 0).unwrap()[3], 0);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[0].pixels, original[0].pixels);
    }

    #[test]
    fn position_lock_blocks_move_and_pixel_lock_blocks_paint() {
        let mut o = open_doc();
        o.doc.layers[0].lock_position = true;
        let mut t = ToolState::new(BrushParams::default(), [1, 2, 3, 255], [0; 4]);
        t.tool = Tool::Move;
        drag(&mut t, &mut o, &[[5.0, 5.0], [9.0, 9.0]]);
        assert!(o.history.labels().is_empty() && t.notice.is_some());
        o.doc.layers[0].lock_position = false;
        o.doc.layers[0].lock_pixels = true;
        t.tool = Tool::Brush;
        drag(&mut t, &mut o, &[[5.0, 5.0], [9.0, 9.0]]);
        assert!(o.history.labels().is_empty());
        t.tool = Tool::Bucket;
        drag(&mut t, &mut o, &[[5.0, 5.0], [5.0, 5.0]]);
        assert!(o.history.labels().is_empty());
    }

    #[test]
    fn painting_on_a_group_is_refused() {
        let mut o = open_doc();
        let g = Layer::group("G");
        let gid = g.id;
        o.doc.layers.push(g);
        o.active_layer = gid;
        let mut t = ToolState::new(BrushParams::default(), [1, 2, 3, 255], [0; 4]);
        drag(&mut t, &mut o, &[[5.0, 5.0], [9.0, 9.0]]);
        assert!(t.notice.as_deref().is_some_and(|n| n.contains("grupo")));
        assert!(o.history.labels().is_empty());
    }

    #[test]
    fn alpha_lock_keeps_transparent_pixels_transparent_with_bucket() {
        let mut o = OpenDoc::new(Document::new("t", 10, 10, None).unwrap());
        o.doc.layers[0].set_pixel(2, 2, [0, 0, 0, 200]);
        o.doc.layers[0].lock_alpha = true;
        let mut t = ToolState::new(BrushParams::default(), [255, 0, 0, 255], [0; 4]);
        t.tool = Tool::Bucket;
        t.contiguous = false;
        t.tolerance = 255;
        drag(&mut t, &mut o, &[[2.0, 2.0], [2.0, 2.0]]);
        assert_eq!(o.doc.layers[0].pixel(2, 2).unwrap(), [255, 0, 0, 200]);
        assert_eq!(o.doc.layers[0].pixel(5, 5).unwrap()[3], 0);
    }
    fn with_selection(o: &mut OpenDoc, x0: f32, y0: f32, x1: f32, y1: f32) {
        let m = orvalith_core::selection::rect_mask(
            o.doc.width,
            o.doc.height,
            (x0, y0),
            (x1, y1),
            true,
        );
        o.doc.selection = orvalith_core::Selection::from_mask(Some(m));
    }

    #[test]
    fn tools_never_touch_pixels_outside_the_selection() {
        let original = open_doc().doc.layers[0].pixels.clone();
        let outside = |o: &OpenDoc| {
            (0..30u32).all(|y| {
                (0..40u32).all(|x| {
                    let inside = (10..20).contains(&x) && (5..15).contains(&y);
                    inside || o.doc.layers[0].pixel(x, y).unwrap() == [255, 255, 255, 255]
                })
            })
        };
        for tool in [
            Tool::Brush,
            Tool::Pencil,
            Tool::Eraser,
            Tool::Bucket,
            Tool::Gradient,
        ] {
            let mut o = open_doc();
            with_selection(&mut o, 10.0, 5.0, 20.0, 15.0);
            let mut t = ToolState::new(
                BrushParams {
                    size: 12.0,
                    hardness: 1.0,
                    ..BrushParams::default()
                },
                [255, 0, 0, 255],
                [0, 0, 255, 255],
            );
            t.tool = tool;
            t.tolerance = 255;
            drag(&mut t, &mut o, &[[0.0, 2.0], [39.0, 20.0], [5.0, 28.0]]);
            assert!(outside(&o), "{tool:?} vazou para fora da seleção");
            assert_ne!(
                o.doc.layers[0].pixels, original,
                "{tool:?} não pintou dentro"
            );
            o.history.undo(&mut o.doc);
            assert_eq!(o.doc.layers[0].pixels, original, "{tool:?}");
        }
    }

    #[test]
    fn soft_selection_edge_gives_partial_bucket_effect() {
        let mut o = open_doc();
        let m = vec![128u8; 40 * 30];
        o.doc.selection = orvalith_core::Selection::from_mask(Some(m));
        let mut t = ToolState::new(BrushParams::default(), [0, 0, 0, 255], [0; 4]);
        t.tool = Tool::Bucket;
        drag(&mut t, &mut o, &[[5.0, 5.0], [5.0, 5.0]]);
        let v = o.doc.layers[0].pixel(5, 5).unwrap();
        assert!((126..=129).contains(&v[0]), "{v:?}");
    }
    fn mask_doc() -> OpenDoc {
        let mut o = OpenDoc::new(Document::new("t", 40, 30, Some([255, 255, 255, 255])).unwrap());
        o.doc
            .layers
            .push(Layer::filled("A", 40, 30, [200, 0, 0, 255]));
        let id = o.doc.layers[1].id;
        o.active_layer = id;
        o.selected = vec![id];
        o.doc.layers[1].mask = Some(orvalith_core::LayerMask::new(vec![255; 40 * 30]));
        o.edit_mask = true;
        o
    }

    #[test]
    fn brush_on_mask_hides_without_touching_pixels_and_is_one_action() {
        use orvalith_core::compose::pixel_at;
        let mut o = mask_doc();
        let pixels = o.doc.layers[1].pixels.clone();
        let mut t = ToolState::new(
            BrushParams {
                size: 6.0,
                hardness: 1.0,
                ..BrushParams::default()
            },
            [0, 0, 0, 255],
            [255, 255, 255, 255],
        );
        drag(&mut t, &mut o, &[[5.0, 15.0], [35.0, 15.0]]);
        assert_eq!(o.history.labels(), vec!["Pintar máscara"]);
        assert_eq!(
            pixel_at(&o.doc, 20, 15),
            Some([255, 255, 255, 255]),
            "mascarado: aparece o fundo"
        );
        assert_eq!(pixel_at(&o.doc, 20, 2), Some([200, 0, 0, 255]));
        assert_eq!(
            o.doc.layers[1].pixels, pixels,
            "pixels da camada preservados"
        );
        // Borracha pinta com a cor de fundo (branco): revela de volta.
        t.tool = Tool::Eraser;
        drag(&mut t, &mut o, &[[5.0, 15.0], [35.0, 15.0]]);
        assert_eq!(pixel_at(&o.doc, 20, 15), Some([200, 0, 0, 255]));
        o.history.undo(&mut o.doc);
        assert_eq!(pixel_at(&o.doc, 20, 15), Some([255, 255, 255, 255]));
        o.history.undo(&mut o.doc);
        assert_eq!(pixel_at(&o.doc, 20, 15), Some([200, 0, 0, 255]));
        assert!(!t.is_busy());
    }

    #[test]
    fn bucket_and_gradient_on_mask() {
        use orvalith_core::compose::pixel_at;
        let mut o = mask_doc();
        let mut t = ToolState::new(BrushParams::default(), [0, 0, 0, 255], [255, 255, 255, 255]);
        t.tool = Tool::Bucket;
        t.tolerance = 0;
        drag(&mut t, &mut o, &[[3.0, 3.0], [3.0, 3.0]]);
        assert_eq!(
            pixel_at(&o.doc, 10, 10),
            Some([255, 255, 255, 255]),
            "máscara toda preta"
        );
        o.history.undo(&mut o.doc);
        // Gradiente preto→branco da esquerda para a direita: esquerda oculta, direita visível.
        t.tool = Tool::Gradient;
        drag(&mut t, &mut o, &[[0.0, 15.0], [20.0, 15.0], [39.0, 15.0]]);
        assert_eq!(o.history.labels(), vec!["Gradiente na máscara"]);
        let left = pixel_at(&o.doc, 1, 15).unwrap();
        let right = pixel_at(&o.doc, 38, 15).unwrap();
        assert!(left[1] > 200, "esquerda quase oculta: {left:?}");
        assert!(right[1] < 20, "direita quase visível: {right:?}");
        o.history.undo(&mut o.doc);
        assert!(o.doc.layers[1]
            .mask
            .as_ref()
            .unwrap()
            .data()
            .iter()
            .all(|&v| v == 255));
    }

    #[test]
    fn painting_target_follows_edit_mask_flag() {
        let mut o = mask_doc();
        o.edit_mask = false;
        let mut t = ToolState::new(
            BrushParams {
                size: 6.0,
                hardness: 1.0,
                ..BrushParams::default()
            },
            [0, 0, 255, 255],
            [255, 255, 255, 255],
        );
        drag(&mut t, &mut o, &[[5.0, 5.0], [30.0, 5.0]]);
        assert_eq!(o.history.labels(), vec!["Pincel"]);
        assert!(o.doc.layers[1]
            .mask
            .as_ref()
            .unwrap()
            .data()
            .iter()
            .all(|&v| v == 255));
        assert_eq!(o.doc.layers[1].pixel(20, 5), Some([0, 0, 255, 255]));
    }

    #[test]
    fn move_tool_carries_linked_mask_and_leaves_unlinked() {
        for linked in [true, false] {
            let mut o = mask_doc();
            o.edit_mask = false;
            o.doc.layers[1].mask.as_mut().unwrap().data_mut()[5 * 40 + 5] = 0;
            o.doc.layers[1].mask.as_mut().unwrap().linked = linked;
            let mut t = ToolState::new(BrushParams::default(), [0; 4], [0; 4]);
            t.tool = Tool::Move;
            drag(&mut t, &mut o, &[[10.0, 10.0], [14.0, 12.0], [15.0, 13.0]]);
            let md = o.doc.layers[1].mask.as_ref().unwrap().data().clone();
            if linked {
                assert_eq!(md[8 * 40 + 10], 0, "marca andou (5,3)");
                assert_eq!(md[5 * 40 + 5], 255);
            } else {
                assert_eq!(md[5 * 40 + 5], 0);
            }
            o.history.undo(&mut o.doc);
            assert_eq!(o.doc.layers[1].mask.as_ref().unwrap().data()[5 * 40 + 5], 0);
        }
    }
}
