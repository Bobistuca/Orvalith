//! Ferramentas de pintura e cor: estado, tratamento de entrada e interface de opções.
//!
//! Toda a lógica de pixels está em `orvalith-core`; aqui apenas se interpreta a entrada do
//! ponteiro e se conectam traço, balde e gradiente ao histórico do documento.

use eframe::egui;
use orvalith_core::edit::PixelPatch;
use orvalith_core::fill::{bucket_fill, Gradient, GradientKind};
use orvalith_core::geom::Rect;
use orvalith_core::paint::{builtin_presets, BrushParams, BrushPreset, StrokeMode, StrokeSession};
use orvalith_core::{compose, DocumentId, LayerId};

use crate::canvas::{CanvasOut, OpenDoc, Overlay};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Brush,
    Pencil,
    Eraser,
    Eyedropper,
    Bucket,
    Gradient,
}

impl Tool {
    pub const ALL: [Tool; 6] = [
        Tool::Brush,
        Tool::Pencil,
        Tool::Eraser,
        Tool::Eyedropper,
        Tool::Bucket,
        Tool::Gradient,
    ];

    pub fn label(self) -> &'static str {
        match self {
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
        original: Vec<u8>,
        start: (f32, f32),
        drawn: bool,
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
            preset_name: String::new(),
            active: None,
        }
    }

    /// Há um traço ou gradiente em andamento (troca de ferramenta/aba é adiada).
    pub fn is_busy(&self) -> bool {
        self.active.is_some()
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

    pub fn overlay(&self) -> Overlay {
        Overlay {
            tool_cursor: true,
            brush_radius_doc: self.tool.paints_strokes().then_some(self.brush.size / 2.0),
        }
    }

    /// Cancela um gradiente em andamento restaurando os pixels originais (tecla Esc).
    pub fn cancel(&mut self, open: &mut OpenDoc) {
        if let Some(Active::Gradient {
            layer, original, ..
        }) = self.active.take()
        {
            if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == layer) {
                if l.pixels.len() == original.len() {
                    l.pixels = original;
                    let r = Rect::full(l.width, l.height);
                    open.mark_dirty(r);
                }
            }
        }
    }

    /// Interpreta os eventos de ponteiro do quadro para a ferramenta atual.
    pub fn handle(&mut self, open: &mut OpenDoc, out: &CanvasOut) {
        let doc_id = open.doc.id;
        let foreign = match &self.active {
            Some(Active::Stroke { doc, .. }) | Some(Active::Gradient { doc, .. }) => *doc != doc_id,
            None => false,
        };
        if foreign {
            self.active = None;
        }
        match self.tool {
            Tool::Brush | Tool::Pencil | Tool::Eraser => self.handle_stroke(open, out),
            Tool::Eyedropper => self.handle_eyedropper(open, out),
            Tool::Bucket => self.handle_bucket(open, out),
            Tool::Gradient => self.handle_gradient(open, out),
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
            if !open.doc.layers[idx].visible {
                self.notice = Some("A camada ativa está oculta.".into());
                return;
            }
            self.notice = None;
            let (mode, pencil, label) = match self.tool {
                Tool::Pencil => (StrokeMode::Paint, true, "Lápis"),
                Tool::Eraser => (StrokeMode::Erase, false, "Borracha"),
                _ => (StrokeMode::Paint, false, "Pincel"),
            };
            let mut session = Box::new(StrokeSession::begin(
                &open.doc.layers[idx],
                self.brush,
                self.fg,
                mode,
                pencil,
                label,
            ));
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
        if !open.doc.layers[idx].visible {
            self.notice = Some("A camada ativa está oculta.".into());
            return;
        }
        let patch: Option<PixelPatch> = bucket_fill(
            &mut open.doc.layers[idx],
            p[0].floor() as i64,
            p[1].floor() as i64,
            self.tolerance,
            self.contiguous,
            self.fg,
            self.fill_opacity,
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
            if !open.doc.layers[idx].visible {
                self.notice = Some("A camada ativa está oculta.".into());
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
                if g.render(l, original) {
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
    pub fn options_ui(&mut self, ui: &mut egui::Ui, user_presets: &mut Vec<BrushPreset>) -> bool {
        let mut presets_changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.strong(self.tool.label());
            ui.separator();
            match self.tool {
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
}
