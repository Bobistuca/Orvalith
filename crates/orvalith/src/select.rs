//! Ferramentas e diálogos de seleção. A lógica de máscaras está em `orvalith_core::selection`;
//! aqui só se interpreta o ponteiro e se registram as alterações no histórico.

use std::sync::Arc;

use eframe::egui;
use orvalith_core::edit::{SelectionEdit, SelectionState};
use orvalith_core::selection::{self as sel, Combine};
use orvalith_core::{compose, Channel, DocumentId, Selection};

use crate::canvas::{CanvasOut, OpenDoc, Shape};
use crate::tools::Tool;

enum Drag {
    Shape {
        start: (f32, f32),
        cur: (f32, f32),
        mode: Combine,
    },
    Free {
        pts: Vec<(f32, f32)>,
        mode: Combine,
    },
    MoveSel {
        start: (f32, f32),
        delta: (i32, i32),
        original: Arc<Vec<u8>>,
        before: SelectionState,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModifyKind {
    Expand,
    Contract,
    Smooth,
    Feather,
}

pub struct ModifyDialog {
    pub kind: ModifyKind,
    pub radius: f32,
}

pub struct TransformDialog {
    before: SelectionState,
    pub dx: f32,
    pub dy: f32,
    pub scale_pct: f32,
    pub angle: f32,
}

pub struct RangeDialog {
    before: SelectionState,
    pub color: [u8; 3],
    pub fuzziness: u8,
    pub picking: bool,
    pub sample_all: bool,
}

pub struct SelectState {
    pub mode: Combine,
    pub feather: f32,
    pub antialias: bool,
    pub square: bool,
    pub tolerance: u8,
    pub contiguous: bool,
    pub sample_all: bool,
    drag: Option<Drag>,
    poly: Vec<(f32, f32)>,
    poly_doc: Option<DocumentId>,
    pub modify: Option<ModifyDialog>,
    pub transform: Option<TransformDialog>,
    pub range: Option<RangeDialog>,
    pub channels_open: bool,
    channel_name: String,
    channel_mode: Combine,
}

impl Default for SelectState {
    fn default() -> Self {
        Self {
            mode: Combine::Replace,
            feather: 0.0,
            antialias: true,
            square: false,
            tolerance: 32,
            contiguous: true,
            sample_all: false,
            drag: None,
            poly: Vec::new(),
            poly_doc: None,
            modify: None,
            transform: None,
            range: None,
            channels_open: false,
            channel_name: String::new(),
            channel_mode: Combine::Replace,
        }
    }
}

pub fn is_selection_tool(t: Tool) -> bool {
    matches!(
        t,
        Tool::Marquee | Tool::Ellipse | Tool::Lasso | Tool::PolyLasso | Tool::Wand
    )
}

/// Troca a seleção como uma ação de histórico (sem marcar o documento como modificado).
/// Retorna `false` se nada mudou.
pub fn set_selection(open: &mut OpenDoc, label: &str, mask: Option<Vec<u8>>) -> bool {
    let before = SelectionState::of(&open.doc);
    let same = match (&before.selection.mask, &mask) {
        (None, None) => true,
        (Some(a), Some(b)) => **a == *b,
        _ => false,
    };
    if same {
        return false;
    }
    let mut after = before.clone();
    after.selection = Selection::from_mask(mask);
    open.execute_nonpixel(Box::new(SelectionEdit::new(label, before, after)));
    true
}

fn constrain(start: (f32, f32), cur: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (cur.0 - start.0, cur.1 - start.1);
    let m = dx.abs().max(dy.abs());
    (start.0 + m * dx.signum(), start.1 + m * dy.signum())
}

impl SelectState {
    /// Há uma operação de seleção em andamento que aceita Esc/Enter.
    pub fn active(&self) -> bool {
        self.drag.is_some() || !self.poly.is_empty()
    }

    /// Forma em construção para o contorno tracejado do canvas.
    pub fn shape(&self, tool: Tool, hover: Option<[f32; 2]>) -> Option<Shape> {
        match (&self.drag, tool) {
            (Some(Drag::Shape { start, cur, .. }), Tool::Ellipse) => {
                let c = if self.square {
                    constrain(*start, *cur)
                } else {
                    *cur
                };
                Some(Shape::Ellipse(*start, c))
            }
            (Some(Drag::Shape { start, cur, .. }), _) => {
                let c = if self.square {
                    constrain(*start, *cur)
                } else {
                    *cur
                };
                Some(Shape::Rect(*start, c))
            }
            (Some(Drag::Free { pts, .. }), _) => Some(Shape::Poly(pts.clone())),
            _ if !self.poly.is_empty() => {
                let mut pts = self.poly.clone();
                if let Some(h) = hover {
                    pts.push((h[0], h[1]));
                }
                Some(Shape::Poly(pts))
            }
            _ => None,
        }
    }

    pub fn picking(&self) -> bool {
        self.range.as_ref().is_some_and(|r| r.picking)
    }

    /// Cancela a operação em andamento (Esc).
    pub fn cancel(&mut self, open: &mut OpenDoc) {
        if let Some(Drag::MoveSel { before, .. }) = self.drag.take() {
            open.doc.selection = before.selection;
            open.doc.bump_view();
        }
        self.drag = None;
        self.poly.clear();
    }

    /// Fecha o laço poligonal em andamento (Enter).
    pub fn confirm(&mut self, open: &mut OpenDoc) {
        if self.poly.len() >= 3 {
            let pts = std::mem::take(&mut self.poly);
            self.commit_polygon(open, &pts, self.mode, "Laço poligonal");
        } else {
            self.poly.clear();
        }
    }

    fn finish(&mut self, open: &mut OpenDoc, shape: Vec<u8>, mode: Combine, label: &str) {
        let (w, h) = (open.doc.width, open.doc.height);
        let shape = if self.feather > 0.0 {
            sel::feather(&shape, w, h, self.feather).unwrap_or_else(|| vec![0; shape.len()])
        } else {
            shape
        };
        let result = sel::combine(
            open.doc.selection.mask.as_deref().map(Vec::as_slice),
            &shape,
            mode,
        );
        set_selection(open, label, result);
    }

    fn commit_polygon(
        &mut self,
        open: &mut OpenDoc,
        pts: &[(f32, f32)],
        mode: Combine,
        label: &str,
    ) {
        let m = sel::polygon_mask(open.doc.width, open.doc.height, pts, self.antialias);
        self.finish(open, m, mode, label);
    }

    pub fn handle(&mut self, tool: Tool, open: &mut OpenDoc, out: &CanvasOut) {
        if self.poly_doc != Some(open.doc.id) {
            self.poly.clear();
            self.drag = None;
            self.poly_doc = Some(open.doc.id);
        }
        let mode_for = |base: Combine| match (out.shift, out.alt) {
            (true, true) => Combine::Intersect,
            (true, false) => Combine::Add,
            (false, true) => Combine::Subtract,
            _ => base,
        };
        let released = out.released || !out.down;
        match tool {
            Tool::Marquee | Tool::Ellipse => {
                if let Some(p) = out.pressed {
                    let mode = mode_for(self.mode);
                    let inside = open.doc.selection.mask.as_ref().is_some_and(|m| {
                        p[0] >= 0.0
                            && p[1] >= 0.0
                            && (p[0] as u32) < open.doc.width
                            && (p[1] as u32) < open.doc.height
                            && m[p[1] as usize * open.doc.width as usize + p[0] as usize] > 127
                    });
                    self.drag = Some(match (&open.doc.selection.mask, mode, inside) {
                        (Some(m), Combine::Replace, true) => Drag::MoveSel {
                            start: (p[0], p[1]),
                            delta: (0, 0),
                            original: m.clone(),
                            before: SelectionState::of(&open.doc),
                        },
                        _ => Drag::Shape {
                            start: (p[0], p[1]),
                            cur: (p[0], p[1]),
                            mode,
                        },
                    });
                }
                if let Some(m) = out.moves.last() {
                    match self.drag.as_mut() {
                        Some(Drag::Shape { cur, .. }) => *cur = (m[0], m[1]),
                        Some(Drag::MoveSel {
                            start,
                            delta,
                            original,
                            ..
                        }) => {
                            let d = (
                                (m[0] - start.0).round() as i32,
                                (m[1] - start.1).round() as i32,
                            );
                            if d != *delta {
                                *delta = d;
                                let moved = sel::translate(
                                    original,
                                    open.doc.width,
                                    open.doc.height,
                                    d.0,
                                    d.1,
                                );
                                open.doc.selection = Selection::from_mask(moved);
                                open.doc.bump_view();
                            }
                        }
                        _ => {}
                    }
                }
                if released {
                    match self.drag.take() {
                        Some(Drag::Shape { start, cur, mode }) => {
                            let end = if self.square {
                                constrain(start, cur)
                            } else {
                                cur
                            };
                            let (w, h) = (open.doc.width, open.doc.height);
                            let m = if tool == Tool::Ellipse {
                                sel::ellipse_mask(w, h, start, end, self.antialias)
                            } else {
                                sel::rect_mask(w, h, start, end, self.antialias)
                            };
                            let label = if tool == Tool::Ellipse {
                                "Seleção elíptica"
                            } else {
                                "Seleção retangular"
                            };
                            self.finish(open, m, mode, label);
                        }
                        Some(Drag::MoveSel {
                            delta,
                            original,
                            before,
                            ..
                        }) => {
                            open.doc.selection = before.selection.clone();
                            if delta != (0, 0) {
                                let moved = sel::translate(
                                    &original,
                                    open.doc.width,
                                    open.doc.height,
                                    delta.0,
                                    delta.1,
                                );
                                set_selection(open, "Mover seleção", moved);
                            }
                            open.doc.bump_view();
                        }
                        other => self.drag = other,
                    }
                }
            }
            Tool::Lasso => {
                if let Some(p) = out.pressed {
                    self.drag = Some(Drag::Free {
                        pts: vec![(p[0], p[1])],
                        mode: mode_for(self.mode),
                    });
                }
                if let Some(Drag::Free { pts, .. }) = self.drag.as_mut() {
                    for m in &out.moves {
                        let last = pts.last().copied().unwrap_or((m[0], m[1]));
                        if (m[0] - last.0).hypot(m[1] - last.1) >= 1.0 {
                            pts.push((m[0], m[1]));
                        }
                    }
                }
                if released {
                    if let Some(Drag::Free { pts, mode }) = self.drag.take() {
                        self.commit_polygon(open, &pts, mode, "Laço");
                    }
                }
            }
            Tool::PolyLasso => {
                if let Some(p) = out.pressed {
                    let close = self.poly.len() >= 3
                        && (p[0] - self.poly[0].0).hypot(p[1] - self.poly[0].1) < 4.0;
                    if close || (out.double_click && self.poly.len() >= 3) {
                        let pts = std::mem::take(&mut self.poly);
                        self.commit_polygon(open, &pts, mode_for(self.mode), "Laço poligonal");
                    } else {
                        self.poly.push((p[0], p[1]));
                    }
                }
            }
            Tool::Wand => {
                if let Some(p) = out.pressed {
                    if p[0] < 0.0 || p[1] < 0.0 {
                        return;
                    }
                    let (w, h) = (open.doc.width, open.doc.height);
                    let (x, y) = (p[0] as u32, p[1] as u32);
                    if x >= w || y >= h {
                        return;
                    }
                    let layer_px = open
                        .active_idx()
                        .map(|i| &open.doc.layers[i])
                        .filter(|l| !l.is_group() && !self.sample_all);
                    let pixels: Arc<Vec<u8>> = match layer_px {
                        Some(l) => l.pixels.clone(),
                        None => Arc::new(compose::flatten(&open.doc)),
                    };
                    let mut m =
                        sel::magic_wand(&pixels, w, h, x, y, self.tolerance, self.contiguous);
                    if self.antialias {
                        if let Some(s) = sel::smooth(&m, w, h, 1) {
                            m = s;
                        }
                    }
                    self.finish(open, m, mode_for(self.mode), "Varinha mágica");
                }
            }
            _ => {}
        }
    }

    /// Amostragem da cor do diálogo "Faixa de cores" clicando no canvas.
    pub fn range_pick(&mut self, open: &mut OpenDoc, out: &CanvasOut) {
        let Some(r) = self.range.as_mut() else { return };
        let Some(p) = out.pressed else { return };
        if p[0] < 0.0 || p[1] < 0.0 {
            return;
        }
        if let Some(c) = compose::pixel_at(&open.doc, p[0] as u32, p[1] as u32) {
            r.color = [c[0], c[1], c[2]];
            r.picking = false;
            Self::preview_range(open, r);
        }
    }

    fn preview_range(open: &mut OpenDoc, r: &RangeDialog) {
        let pixels = if r.sample_all {
            compose::flatten(&open.doc)
        } else {
            open.active_idx()
                .map(|i| open.doc.layers[i].pixels.as_ref().clone())
                .filter(|p| p.len() == open.doc.width as usize * open.doc.height as usize * 4)
                .unwrap_or_else(|| compose::flatten(&open.doc))
        };
        let m = sel::nonempty(sel::color_range(&pixels, r.color, r.fuzziness));
        open.doc.selection = Selection::from_mask(m);
        open.doc.bump_view();
    }

    // ---- Opções da barra superior ----

    pub fn options_ui(&mut self, ui: &mut egui::Ui, tool: Tool) {
        ui.label("Modo");
        for (m, label) in [
            (Combine::Replace, "Nova"),
            (Combine::Add, "Adicionar"),
            (Combine::Subtract, "Subtrair"),
            (Combine::Intersect, "Interseção"),
        ] {
            ui.radio_value(&mut self.mode, m, label);
        }
        ui.separator();
        if tool != Tool::PolyLasso {
            ui.label("Pena");
            ui.add(egui::Slider::new(&mut self.feather, 0.0..=100.0).suffix(" px"));
        }
        ui.checkbox(&mut self.antialias, "Suavizar bordas");
        match tool {
            Tool::Marquee | Tool::Ellipse => {
                ui.checkbox(&mut self.square, "Proporção 1:1");
            }
            Tool::Wand => {
                ui.label("Tolerância");
                ui.add(egui::Slider::new(&mut self.tolerance, 0..=255));
                ui.checkbox(&mut self.contiguous, "Contíguo");
                ui.checkbox(&mut self.sample_all, "Todas as camadas");
            }
            Tool::PolyLasso => {
                ui.label("Clique para pontos · duplo clique/Enter fecha · Esc cancela");
            }
            _ => {}
        }
        ui.label("Shift: adicionar · Alt: subtrair · Shift+Alt: interseção");
    }

    // ---- Diálogos ----

    pub fn show_dialogs(&mut self, ctx: &egui::Context, open: &mut OpenDoc, fg: [u8; 4]) {
        self.modify_dialog(ctx, open);
        self.transform_dialog(ctx, open);
        self.range_dialog(ctx, open);
        self.channels_dialog(ctx, open, fg);
    }

    pub fn open_range(&mut self, open: &mut OpenDoc, fg: [u8; 4]) {
        let r = RangeDialog {
            before: SelectionState::of(&open.doc),
            color: [fg[0], fg[1], fg[2]],
            fuzziness: 40,
            picking: false,
            sample_all: false,
        };
        Self::preview_range(open, &r);
        self.range = Some(r);
    }

    pub fn open_transform(&mut self, open: &OpenDoc) {
        if open.doc.selection.mask.is_some() {
            self.transform = Some(TransformDialog {
                before: SelectionState::of(&open.doc),
                dx: 0.0,
                dy: 0.0,
                scale_pct: 100.0,
                angle: 0.0,
            });
        }
    }

    fn modify_dialog(&mut self, ctx: &egui::Context, open: &mut OpenDoc) {
        let Some(mut d) = self.modify.take() else {
            return;
        };
        let (mut apply, mut cancel) = (false, false);
        egui::Window::new("Modificar seleção")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.kind, ModifyKind::Expand, "Expandir");
                    ui.radio_value(&mut d.kind, ModifyKind::Contract, "Contrair");
                    ui.radio_value(&mut d.kind, ModifyKind::Smooth, "Suavizar");
                    ui.radio_value(&mut d.kind, ModifyKind::Feather, "Pena");
                });
                ui.add(egui::Slider::new(&mut d.radius, 1.0..=100.0).text("Raio (px)"));
                ui.small(
                    "Expandir/contrair usam um disco; a borda do canvas não encolhe a seleção.",
                );
                ui.horizontal(|ui| {
                    apply = ui.button("Aplicar").clicked();
                    cancel = ui.button("Cancelar").clicked();
                });
            });
        if apply {
            if let Some(cur) = open.doc.selection.mask.clone() {
                let (w, h, r) = (open.doc.width, open.doc.height, d.radius.round() as u32);
                let (label, result) = match d.kind {
                    ModifyKind::Expand => ("Expandir seleção", sel::expand(&cur, w, h, r)),
                    ModifyKind::Contract => ("Contrair seleção", sel::contract(&cur, w, h, r)),
                    ModifyKind::Smooth => ("Suavizar seleção", sel::smooth(&cur, w, h, r)),
                    ModifyKind::Feather => ("Pena da seleção", sel::feather(&cur, w, h, d.radius)),
                };
                set_selection(open, label, result);
            }
        } else if !cancel {
            self.modify = Some(d);
        }
    }

    fn transform_dialog(&mut self, ctx: &egui::Context, open: &mut OpenDoc) {
        let Some(mut d) = self.transform.take() else {
            return;
        };
        let (mut apply, mut cancel, mut changed) = (false, false, false);
        egui::Window::new("Transformar seleção")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                changed |= ui
                    .add(egui::DragValue::new(&mut d.dx).prefix("X ").suffix(" px"))
                    .changed();
                changed |= ui
                    .add(egui::DragValue::new(&mut d.dy).prefix("Y ").suffix(" px"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut d.scale_pct, 5.0..=400.0).text("Escala %"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut d.angle, -180.0..=180.0).text("Rotação °"))
                    .changed();
                ui.small("Atua só na máscara; os pixels não mudam.");
                ui.horizontal(|ui| {
                    apply = ui.button("Aplicar").clicked();
                    cancel = ui.button("Cancelar").clicked();
                });
            });
        let (w, h) = (open.doc.width, open.doc.height);
        let original = d.before.selection.mask.clone();
        if changed {
            if let Some(m) = &original {
                let s = d.scale_pct / 100.0;
                let t = sel::transform(m, w, h, (s, s), d.angle, (d.dx, d.dy));
                open.doc.selection = Selection::from_mask(t);
                open.doc.bump_view();
            }
        }
        if apply {
            let result = open.doc.selection.mask.as_ref().map(|m| m.as_ref().clone());
            open.doc.selection = d.before.selection.clone();
            set_selection(open, "Transformar seleção", result);
            open.doc.bump_view();
        } else if cancel {
            open.doc.selection = d.before.selection.clone();
            open.doc.bump_view();
        } else {
            self.transform = Some(d);
        }
    }

    fn range_dialog(&mut self, ctx: &egui::Context, open: &mut OpenDoc) {
        let Some(mut d) = self.range.take() else {
            return;
        };
        let (mut apply, mut cancel, mut changed) = (false, false, false);
        egui::Window::new("Faixa de cores")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Cor");
                    changed |= ui.color_edit_button_srgb(&mut d.color).changed();
                    if ui
                        .selectable_label(d.picking, "Amostrar no canvas")
                        .on_hover_text("Clique na imagem para escolher a cor")
                        .clicked()
                    {
                        d.picking = !d.picking;
                    }
                });
                changed |= ui
                    .add(egui::Slider::new(&mut d.fuzziness, 0..=200).text("Tolerância"))
                    .changed();
                changed |= ui.checkbox(&mut d.sample_all, "Todas as camadas").changed();
                ui.horizontal(|ui| {
                    apply = ui.button("Aplicar").clicked();
                    cancel = ui.button("Cancelar").clicked();
                });
            });
        if changed {
            Self::preview_range(open, &d);
        }
        if apply {
            let result = open.doc.selection.mask.as_ref().map(|m| m.as_ref().clone());
            open.doc.selection = d.before.selection.clone();
            set_selection(open, "Faixa de cores", result);
            open.doc.bump_view();
        } else if cancel {
            open.doc.selection = d.before.selection.clone();
            open.doc.bump_view();
        } else {
            self.range = Some(d);
        }
    }

    fn channels_dialog(&mut self, ctx: &egui::Context, open: &mut OpenDoc, _fg: [u8; 4]) {
        if !self.channels_open {
            return;
        }
        let mut keep = true;
        let mut save = false;
        let mut load: Option<usize> = None;
        let mut delete: Option<usize> = None;
        let has_sel = open.doc.selection.mask.is_some();
        egui::Window::new("Canais de seleção")
            .open(&mut keep)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.channel_name)
                            .hint_text("Nome do canal")
                            .desired_width(130.0),
                    );
                    save = ui
                        .add_enabled(has_sel, egui::Button::new("Salvar seleção atual"))
                        .clicked();
                });
                ui.horizontal(|ui| {
                    ui.label("Ao carregar:");
                    for (m, l) in [
                        (Combine::Replace, "Nova"),
                        (Combine::Add, "Adicionar"),
                        (Combine::Subtract, "Subtrair"),
                        (Combine::Intersect, "Interseção"),
                    ] {
                        ui.radio_value(&mut self.channel_mode, m, l);
                    }
                });
                ui.separator();
                if open.doc.channels.is_empty() {
                    ui.label("Nenhum canal salvo.");
                }
                for (i, c) in open.doc.channels.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(&c.name);
                        if ui.small_button("Carregar").clicked() {
                            load = Some(i);
                        }
                        if ui.small_button("Excluir").clicked() {
                            delete = Some(i);
                        }
                    });
                }
            });
        self.channels_open = keep;
        let before = SelectionState::of(&open.doc);
        if save {
            if let Some(mask) = open.doc.selection.mask.clone() {
                let name = if self.channel_name.trim().is_empty() {
                    format!("Canal {}", open.doc.channels.len() + 1)
                } else {
                    self.channel_name.trim().to_string()
                };
                let mut after = before.clone();
                after.channels.push(Channel {
                    id: open.doc.new_channel_id(),
                    name,
                    mask,
                });
                open.execute_nonpixel(Box::new(SelectionEdit::new(
                    "Salvar seleção",
                    before.clone(),
                    after,
                )));
                self.channel_name.clear();
            }
        } else if let Some(i) = load {
            let ch = open.doc.channels[i].mask.clone();
            let result = sel::combine(
                open.doc.selection.mask.as_deref().map(Vec::as_slice),
                &ch,
                self.channel_mode,
            );
            set_selection(open, "Carregar seleção", result);
        } else if let Some(i) = delete {
            let mut after = before.clone();
            after.channels.remove(i);
            open.execute_nonpixel(Box::new(SelectionEdit::new("Excluir canal", before, after)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::Document;

    fn open() -> OpenDoc {
        OpenDoc::new(Document::new("t", 40, 30, Some([200, 200, 200, 255])).unwrap())
    }

    fn drag(
        s: &mut SelectState,
        tool: Tool,
        o: &mut OpenDoc,
        pts: &[[f32; 2]],
        shift: bool,
        alt: bool,
    ) {
        let base = CanvasOut {
            pressure: 1.0,
            down: true,
            shift,
            alt,
            ..Default::default()
        };
        s.handle(
            tool,
            o,
            &CanvasOut {
                pressed: Some(pts[0]),
                ..clone_out(&base)
            },
        );
        s.handle(
            tool,
            o,
            &CanvasOut {
                moves: pts[1..].to_vec(),
                ..clone_out(&base)
            },
        );
        s.handle(
            tool,
            o,
            &CanvasOut {
                released: true,
                down: false,
                shift,
                alt,
                pressure: 1.0,
                ..Default::default()
            },
        );
    }

    fn clone_out(b: &CanvasOut) -> CanvasOut {
        CanvasOut {
            pressure: b.pressure,
            down: b.down,
            shift: b.shift,
            alt: b.alt,
            ..Default::default()
        }
    }

    fn selected(o: &OpenDoc) -> usize {
        o.doc
            .selection
            .mask
            .as_ref()
            .map_or(0, |m| m.iter().filter(|&&v| v == 255).count())
    }

    #[test]
    fn rectangle_selection_modes_and_undo() {
        let mut o = open();
        let mut s = SelectState::default();
        drag(
            &mut s,
            Tool::Marquee,
            &mut o,
            &[[2.0, 2.0], [12.0, 12.0]],
            false,
            false,
        );
        assert_eq!(selected(&o), 100);
        // Shift adiciona uma segunda área.
        drag(
            &mut s,
            Tool::Marquee,
            &mut o,
            &[[20.0, 2.0], [30.0, 12.0]],
            true,
            false,
        );
        assert_eq!(selected(&o), 200);
        // Alt subtrai.
        drag(
            &mut s,
            Tool::Marquee,
            &mut o,
            &[[20.0, 2.0], [25.0, 12.0]],
            false,
            true,
        );
        assert_eq!(selected(&o), 150);
        // Shift+Alt faz interseção.
        drag(
            &mut s,
            Tool::Marquee,
            &mut o,
            &[[0.0, 0.0], [12.0, 6.0]],
            true,
            true,
        );
        assert_eq!(selected(&o), 40);
        assert_eq!(o.history.labels().len(), 4);
        o.history.undo(&mut o.doc);
        assert_eq!(selected(&o), 150);
        o.history.undo(&mut o.doc);
        o.history.undo(&mut o.doc);
        o.history.undo(&mut o.doc);
        assert_eq!(selected(&o), 0);
        o.history.redo(&mut o.doc);
        assert_eq!(selected(&o), 100);
    }

    #[test]
    fn selection_does_not_mark_document_modified() {
        let mut o = open();
        let mut s = SelectState::default();
        drag(
            &mut s,
            Tool::Ellipse,
            &mut o,
            &[[2.0, 2.0], [20.0, 20.0]],
            false,
            false,
        );
        assert!(selected(&o) > 0);
        assert!(!o.doc.modified);
    }

    #[test]
    fn click_outside_deselects_and_click_inside_moves() {
        let mut o = open();
        let mut s = SelectState::default();
        drag(
            &mut s,
            Tool::Marquee,
            &mut o,
            &[[5.0, 5.0], [15.0, 15.0]],
            false,
            false,
        );
        // Arrastar de dentro move a seleção.
        drag(
            &mut s,
            Tool::Marquee,
            &mut o,
            &[[10.0, 10.0], [20.0, 12.0]],
            false,
            false,
        );
        let b = sel::bounds(o.doc.selection.mask.as_ref().unwrap(), 40, 30).unwrap();
        assert_eq!((b.x, b.y), (15, 7));
        assert_eq!(o.history.labels().last().copied(), Some("Mover seleção"));
        // Clique fora (sem arrastar) desmarca.
        drag(
            &mut s,
            Tool::Marquee,
            &mut o,
            &[[1.0, 1.0], [1.0, 1.0]],
            false,
            false,
        );
        assert!(o.doc.selection.mask.is_none());
    }

    #[test]
    fn lasso_polygon_and_wand() {
        let mut o = open();
        let mut s = SelectState::default();
        drag(
            &mut s,
            Tool::Lasso,
            &mut o,
            &[[5.0, 5.0], [25.0, 5.0], [25.0, 20.0], [5.0, 20.0]],
            false,
            false,
        );
        assert_eq!(selected(&o), 20 * 15);
        // Laço poligonal: três cliques e Enter.
        let click = |s: &mut SelectState, o: &mut OpenDoc, p: [f32; 2]| {
            s.handle(
                Tool::PolyLasso,
                o,
                &CanvasOut {
                    pressed: Some(p),
                    down: true,
                    pressure: 1.0,
                    ..Default::default()
                },
            );
        };
        click(&mut s, &mut o, [2.0, 2.0]);
        click(&mut s, &mut o, [30.0, 2.0]);
        click(&mut s, &mut o, [2.0, 25.0]);
        assert!(s.active());
        s.confirm(&mut o);
        assert!(selected(&o) > 250 && !s.active());
        // Varinha: documento uniforme seleciona tudo.
        drag(
            &mut s,
            Tool::Wand,
            &mut o,
            &[[3.0, 3.0], [3.0, 3.0]],
            false,
            false,
        );
        assert_eq!(selected(&o), 40 * 30);
    }

    #[test]
    fn wand_respects_layer_content_and_tolerance() {
        let mut o = open();
        for y in 0..30 {
            for x in 20..40 {
                o.doc.layers[0].set_pixel(x, y, [0, 0, 255, 255]);
            }
        }
        let mut s = SelectState {
            antialias: false,
            ..SelectState::default()
        };
        drag(
            &mut s,
            Tool::Wand,
            &mut o,
            &[[30.0, 10.0], [30.0, 10.0]],
            false,
            false,
        );
        assert_eq!(selected(&o), 20 * 30);
        s.tolerance = 255;
        drag(
            &mut s,
            Tool::Wand,
            &mut o,
            &[[30.0, 10.0], [30.0, 10.0]],
            false,
            false,
        );
        assert_eq!(selected(&o), 40 * 30);
    }

    #[test]
    fn shape_feather_is_applied_before_combining() {
        let mut o = OpenDoc::new(Document::new("t", 80, 80, Some([1, 1, 1, 255])).unwrap());
        let mut s = SelectState {
            feather: 10.0,
            ..SelectState::default()
        };
        drag(
            &mut s,
            Tool::Marquee,
            &mut o,
            &[[20.0, 20.0], [60.0, 60.0]],
            false,
            false,
        );
        let m = o.doc.selection.mask.clone().unwrap();
        assert_eq!(m[40 * 80 + 40], 255);
        let edge = m[40 * 80 + 20];
        assert!(edge > 40 && edge < 220, "{edge}");
    }

    #[test]
    fn channels_save_load_delete_are_undoable() {
        let mut o = open();
        let mut s = SelectState::default();
        drag(
            &mut s,
            Tool::Marquee,
            &mut o,
            &[[2.0, 2.0], [12.0, 12.0]],
            false,
            false,
        );
        s.channels_dialog_for_test(&mut o);
        assert_eq!(o.doc.channels.len(), 1);
        set_selection(&mut o, "Desmarcar", None);
        let ch = o.doc.channels[0].mask.clone();
        set_selection(&mut o, "Carregar", Some(ch.as_ref().clone()));
        assert_eq!(selected(&o), 100);
        o.history.undo(&mut o.doc);
        o.history.undo(&mut o.doc);
        o.history.undo(&mut o.doc);
        assert!(o.doc.channels.is_empty());
    }

    impl SelectState {
        fn channels_dialog_for_test(&mut self, open: &mut OpenDoc) {
            let before = SelectionState::of(&open.doc);
            let mut after = before.clone();
            after.channels.push(Channel {
                id: open.doc.new_channel_id(),
                name: "Teste".into(),
                mask: open.doc.selection.mask.clone().unwrap(),
            });
            open.execute_nonpixel(Box::new(SelectionEdit::new(
                "Salvar seleção",
                before,
                after,
            )));
        }
    }

    #[test]
    fn color_range_preview_and_cancel_restores_selection() {
        let mut o = open();
        let mut s = SelectState::default();
        s.open_range(&mut o, [200, 200, 200, 255]);
        assert_eq!(selected(&o), 40 * 30);
        let before_rev = o.history.labels().len();
        let r = s.range.take().unwrap();
        o.doc.selection = r.before.selection.clone();
        assert!(o.doc.selection.mask.is_none());
        assert_eq!(o.history.labels().len(), before_rev);
    }
}
