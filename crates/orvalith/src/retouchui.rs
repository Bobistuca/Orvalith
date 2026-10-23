//! Ferramentas de retoque: carimbo de clone/correção, correção pontual, remendo, olhos
//! vermelhos e pincéis de desfocar/nitidez/borrar/subexpor/superexpor/esponja.

use eframe::egui;
use orvalith_core::geom::Rect as PixRect;
use orvalith_core::retouch::{self, Brush, Op, Range, Smudge, Walker};
use orvalith_core::{compose, DocumentId, Layer, LayerId};

use crate::canvas::{CanvasOut, OpenDoc};
use crate::tools::{paint_blocked, Tool};

#[derive(Clone, Copy)]
enum Kind {
    Clone { heal: bool },
    Fx(Op),
}

struct Stroke {
    doc: DocumentId,
    layer: LayerId,
    before: Vec<Layer>,
    was_modified: bool,
    snapshot: Vec<u8>,
    walker: Walker,
    offset: [i32; 2],
    smudge: Smudge,
    kind: Kind,
    dirty: Option<PixRect>,
    label: &'static str,
}

#[derive(Clone, Copy)]
struct Params {
    brush: Brush,
    strength: f32,
}

pub struct RetouchState {
    pub size: f32,
    pub hardness: f32,
    /// Opacidade do carimbo / intensidade dos pincéis de efeito (0..=1).
    pub strength: f32,
    pub spacing: f32,
    pub aligned: bool,
    pub sample_all: bool,
    /// Carimbo no modo "correção" (mantém a cor/luz do local).
    pub heal: bool,
    pub op: usize,
    pub range: Range,
    pub saturate: bool,
    pub red_amount: f32,
    src: Option<[f32; 2]>,
    offset: Option<[i32; 2]>,
    stroke: Option<Stroke>,
    patch_start: Option<(DocumentId, [f32; 2])>,
}

impl Default for RetouchState {
    fn default() -> Self {
        Self {
            size: 40.0,
            hardness: 0.5,
            strength: 1.0,
            spacing: 0.2,
            aligned: true,
            sample_all: false,
            heal: false,
            op: 0,
            range: Range::Midtones,
            saturate: false,
            red_amount: 1.0,
            src: None,
            offset: None,
            stroke: None,
            patch_start: None,
        }
    }
}

fn op_of(s: &RetouchState) -> Op {
    match s.op {
        0 => Op::Blur,
        1 => Op::Sharpen,
        2 => Op::Smudge,
        3 => Op::Dodge(s.range),
        4 => Op::Burn(s.range),
        _ => Op::Sponge {
            saturate: s.saturate,
        },
    }
}

fn dirty_rect(p: [f32; 2], r: f32, w: u32, h: u32) -> PixRect {
    let m = r + 3.0;
    let x0 = (p[0] - m).floor().max(0.0) as u32;
    let y0 = (p[1] - m).floor().max(0.0) as u32;
    let x1 = ((p[0] + m).ceil().max(0.0) as u32).min(w);
    let y1 = ((p[1] + m).ceil().max(0.0) as u32).min(h);
    PixRect::new(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}

fn blocked(open: &OpenDoc) -> Option<String> {
    if open.edit_mask || open.qm.is_some() {
        return Some("As ferramentas de retoque agem nos pixels da camada, não na máscara.".into());
    }
    let idx = open.active_idx()?;
    paint_blocked(&open.doc.layers[idx])
}

impl RetouchState {
    pub fn busy(&self) -> bool {
        self.stroke.is_some() || self.patch_start.is_some()
    }

    /// Raio do contorno sob o cursor para as ferramentas de pincel.
    pub fn brush_radius(&self, tool: Tool) -> Option<f32> {
        matches!(
            tool,
            Tool::Clone | Tool::SpotHeal | Tool::RedEye | Tool::Retouch
        )
        .then_some(self.size / 2.0)
    }

    /// Ponto de origem do carimbo (alça desenhada no canvas).
    pub fn source_marker(&self, tool: Tool) -> Vec<(f32, f32)> {
        match (tool, self.src) {
            (Tool::Clone, Some(s)) => vec![(s[0], s[1])],
            _ => Vec::new(),
        }
    }

    /// Esc: desfaz o traço em andamento sem registrar histórico.
    pub fn cancel(&mut self, open: &mut OpenDoc) {
        self.patch_start = None;
        if let Some(st) = self.stroke.take() {
            if st.doc == open.doc.id && open.doc.layers.len() == st.before.len() {
                open.doc.layers = st.before;
                let full = PixRect::full(open.doc.width, open.doc.height);
                open.mark_dirty(full);
                open.doc.modified = st.was_modified;
            }
        }
    }

    pub fn handle(&mut self, tool: Tool, open: &mut OpenDoc, out: &CanvasOut) -> Option<String> {
        if self.stroke.as_ref().is_some_and(|s| s.doc != open.doc.id) {
            self.stroke = None;
        }
        if self.patch_start.is_some_and(|(d, _)| d != open.doc.id) {
            self.patch_start = None;
        }
        match tool {
            Tool::Clone | Tool::Retouch => self.handle_stroke(tool, open, out),
            Tool::SpotHeal => self.handle_spot(open, out),
            Tool::RedEye => self.handle_red_eye(open, out),
            Tool::Patch => self.handle_patch(open, out),
            _ => None,
        }
    }

    fn params(&self) -> Params {
        Params {
            brush: Brush {
                radius: self.size / 2.0,
                hardness: self.hardness,
            },
            strength: self.strength,
        }
    }

    fn handle_stroke(&mut self, tool: Tool, open: &mut OpenDoc, out: &CanvasOut) -> Option<String> {
        let mut notice = None;
        if let Some(p) = out.pressed {
            if tool == Tool::Clone && out.alt {
                self.src = Some(p);
                self.offset = None;
                return Some("Origem do carimbo definida.".into());
            }
            if let Some(m) = blocked(open) {
                return Some(m);
            }
            let idx = open.active_idx()?;
            let mut offset = [0, 0];
            let kind = if tool == Tool::Clone {
                let Some(src) = self.src else {
                    return Some("Alt+clique define a origem do carimbo.".into());
                };
                offset = match (self.aligned, self.offset) {
                    (true, Some(o)) => o,
                    _ => {
                        let o = [
                            (src[0] - p[0]).round() as i32,
                            (src[1] - p[1]).round() as i32,
                        ];
                        self.offset = Some(o);
                        o
                    }
                };
                Kind::Clone { heal: self.heal }
            } else {
                Kind::Fx(op_of(self))
            };
            let snapshot = match kind {
                Kind::Clone { .. } if self.sample_all => compose::flatten(&open.doc),
                Kind::Clone { .. } => open.doc.layers[idx].pixels.as_ref().clone(),
                Kind::Fx(_) => Vec::new(),
            };
            let label = match kind {
                Kind::Clone { heal: true } => "Pincel de correção",
                Kind::Clone { heal: false } => "Carimbo de clone",
                Kind::Fx(_) => "Pincel de retoque",
            };
            self.stroke = Some(Stroke {
                doc: open.doc.id,
                layer: open.doc.layers[idx].id,
                before: open.doc.layers.clone(),
                was_modified: open.doc.modified,
                snapshot,
                walker: Walker::new(p),
                offset,
                smudge: Smudge::default(),
                kind,
                dirty: None,
                label,
            });
            notice = self.dab(open, p);
        }
        let spacing = (self.size * self.spacing).max(1.0);
        let moves: Vec<[f32; 2]> = if out.down {
            out.moves.clone()
        } else {
            Vec::new()
        };
        for m in moves {
            let pts = match self.stroke.as_mut() {
                Some(s) => s.walker.advance(m, spacing),
                None => break,
            };
            for p in pts {
                self.dab(open, p);
            }
        }
        if self.stroke.is_some() && (out.released || !out.down) && out.pressed.is_none() {
            self.finish(open);
        }
        notice
    }

    fn dab(&mut self, open: &mut OpenDoc, p: [f32; 2]) -> Option<String> {
        let params = self.params();
        let st = self.stroke.as_mut()?;
        let sel = open.doc.selection.mask.clone();
        let (w, h) = (open.doc.width, open.doc.height);
        let layer = open.doc.layers.iter_mut().find(|l| l.id == st.layer)?;
        let (lw, lh) = (layer.width as usize, layer.height as usize);
        let px = layer.pixels_mut();
        let sel = sel.as_ref().map(|m| m.as_slice());
        match st.kind {
            Kind::Clone { heal: false } => {
                retouch::clone_dab(
                    px,
                    &st.snapshot,
                    lw,
                    lh,
                    p,
                    st.offset,
                    params.brush,
                    params.strength,
                    sel,
                );
            }
            Kind::Clone { heal: true } => {
                retouch::heal_dab(
                    px,
                    &st.snapshot,
                    lw,
                    lh,
                    p,
                    st.offset,
                    params.brush,
                    params.strength,
                    sel,
                );
            }
            Kind::Fx(op) => {
                retouch::effect_dab(
                    op,
                    px,
                    lw,
                    lh,
                    p,
                    params.brush,
                    params.strength,
                    sel,
                    &mut st.smudge,
                );
            }
        }
        let r = dirty_rect(p, params.brush.radius, w, h);
        st.dirty = Some(st.dirty.map_or(r, |d| d.union(r)));
        open.mark_dirty(r);
        None
    }

    fn finish(&mut self, open: &mut OpenDoc) {
        let Some(st) = self.stroke.take() else { return };
        let Some(rect) = st.dirty else { return };
        // Camada com alfa bloqueado: o alfa original é restaurado ao fim do traço.
        if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == st.layer) {
            if l.lock_alpha {
                if let Some(old) = st.before.iter().find(|b| b.id == st.layer) {
                    let a: Vec<u8> = old.pixels.iter().skip(3).step_by(4).copied().collect();
                    for (p, a) in l.pixels_mut().as_chunks_mut::<4>().0.iter_mut().zip(a) {
                        p[3] = a;
                    }
                }
            }
        }
        let rect = if open
            .doc
            .layers
            .iter()
            .any(|l| l.id == st.layer && l.lock_alpha)
        {
            PixRect::full(open.doc.width, open.doc.height)
        } else {
            rect
        };
        open.record_layers(st.label, st.before, rect);
    }

    fn one_shot(
        &mut self,
        open: &mut OpenDoc,
        label: &str,
        f: impl FnOnce(&mut Vec<u8>, usize, usize, Option<&[u8]>) -> bool,
    ) -> Option<String> {
        if let Some(m) = blocked(open) {
            return Some(m);
        }
        let idx = open.active_idx()?;
        let before = open.doc.layers.clone();
        let sel = open.doc.selection.mask.clone();
        let (w, h) = (open.doc.width, open.doc.height);
        let layer = &mut open.doc.layers[idx];
        let (lw, lh) = (layer.width as usize, layer.height as usize);
        let mut px = layer.pixels.as_ref().clone();
        if !f(&mut px, lw, lh, sel.as_ref().map(|m| m.as_slice())) {
            return Some("Nada a corrigir nesse ponto.".into());
        }
        if layer.lock_alpha {
            for (p, o) in px
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(layer.pixels.as_chunks::<4>().0)
            {
                p[3] = o[3];
            }
        }
        layer.pixels = std::sync::Arc::new(px);
        let full = PixRect::full(w, h);
        let was = open.doc.modified;
        open.mark_dirty(full);
        open.doc.modified = was;
        open.record_layers(label, before, full);
        None
    }

    fn handle_spot(&mut self, open: &mut OpenDoc, out: &CanvasOut) -> Option<String> {
        let p = out.pressed?;
        let radius = self.size / 2.0;
        self.one_shot(open, "Correção pontual", |px, w, h, sel| {
            retouch::spot_heal(px, w, h, p, radius, sel)
        })
        .map(|m| {
            if m == "Nada a corrigir nesse ponto." {
                "Não há região de origem adequada perto desse ponto.".to_string()
            } else {
                m
            }
        })
    }

    fn handle_red_eye(&mut self, open: &mut OpenDoc, out: &CanvasOut) -> Option<String> {
        let p = out.pressed?;
        let (radius, amount) = (self.size / 2.0, self.red_amount);
        self.one_shot(open, "Olhos vermelhos", |px, w, h, sel| {
            retouch::red_eye(px, w, h, p, radius, amount, sel) > 0
        })
        .map(|m| {
            if m == "Nada a corrigir nesse ponto." {
                "Nenhum vermelho de olho encontrado nesse círculo.".to_string()
            } else {
                m
            }
        })
    }

    fn handle_patch(&mut self, open: &mut OpenDoc, out: &CanvasOut) -> Option<String> {
        if let Some(p) = out.pressed {
            if open.doc.selection.is_empty() {
                return Some("Remendo: faça uma seleção sobre a área a corrigir primeiro.".into());
            }
            self.patch_start = Some((open.doc.id, p));
        }
        if out.released {
            let (_, start) = self.patch_start.take()?;
            let end = out.moves.last().copied().or(out.hover).unwrap_or(start);
            let off = [
                (end[0] - start[0]).round() as i32,
                (end[1] - start[1]).round() as i32,
            ];
            if off == [0, 0] {
                return None;
            }
            let mask = open.doc.selection.mask.clone()?;
            return self
                .one_shot(open, "Remendo", |px, w, h, _| {
                    retouch::patch(px, w, h, &mask, off) > 0
                })
                .map(|m| {
                    if m == "Nada a corrigir nesse ponto." {
                        "O remendo não alterou nada (origem fora da imagem?).".to_string()
                    } else {
                        m
                    }
                });
        }
        None
    }

    pub fn options_ui(&mut self, ui: &mut egui::Ui, tool: Tool) {
        let pct = |ui: &mut egui::Ui, label: &str, v: &mut f32, min: f32| {
            let mut p = *v * 100.0;
            ui.label(label);
            if ui
                .add(egui::Slider::new(&mut p, min..=100.0).suffix("%").integer())
                .changed()
            {
                *v = p / 100.0;
            }
        };
        match tool {
            Tool::Clone => {
                ui.label("Alt+clique define a origem; arraste para carimbar.");
                ui.checkbox(&mut self.aligned, "Alinhado");
                ui.checkbox(&mut self.heal, "Correção (mantém cor e luz)");
                ui.radio_value(&mut self.sample_all, false, "Camada ativa");
                ui.radio_value(&mut self.sample_all, true, "Imagem composta");
            }
            Tool::SpotHeal => {
                ui.label("Clique sobre a imperfeição: a origem é escolhida automaticamente.");
            }
            Tool::Patch => {
                ui.label("Com uma seleção ativa, arraste até a região de origem e solte.");
            }
            Tool::RedEye => {
                pct(ui, "Intensidade", &mut self.red_amount, 5.0);
                ui.label("Clique no centro do olho; o tamanho define o raio.");
            }
            Tool::Retouch => {
                egui::ComboBox::from_id_salt("retouch_op")
                    .selected_text(Op::NAMES[self.op.min(5)])
                    .show_ui(ui, |ui| {
                        for (i, n) in Op::NAMES.iter().enumerate() {
                            ui.selectable_value(&mut self.op, i, *n);
                        }
                    });
                if matches!(self.op, 3 | 4) {
                    ui.selectable_value(&mut self.range, Range::Shadows, "Sombras");
                    ui.selectable_value(&mut self.range, Range::Midtones, "Meios-tons");
                    ui.selectable_value(&mut self.range, Range::Highlights, "Realces");
                }
                if self.op == 5 {
                    ui.radio_value(&mut self.saturate, false, "Dessaturar");
                    ui.radio_value(&mut self.saturate, true, "Saturar");
                }
            }
            _ => {}
        }
        ui.label("Tamanho");
        ui.add(
            egui::Slider::new(&mut self.size, 2.0..=500.0)
                .logarithmic(true)
                .suffix(" px"),
        );
        if matches!(tool, Tool::Clone | Tool::Retouch) {
            pct(ui, "Dureza", &mut self.hardness, 0.0);
            pct(
                ui,
                if tool == Tool::Clone {
                    "Opacidade"
                } else {
                    "Intensidade"
                },
                &mut self.strength,
                1.0,
            );
            pct(ui, "Espaçamento", &mut self.spacing, 5.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::Document;

    fn open() -> OpenDoc {
        let mut d = Document::new("t", 60, 30, Some([100, 100, 100, 255])).unwrap();
        for y in 0..30 {
            for x in 40..60 {
                d.layers[0].set_pixel(x, y, [220, 120, 60, 255]);
            }
        }
        OpenDoc::new(d)
    }

    fn press(p: [f32; 2], alt: bool) -> CanvasOut {
        CanvasOut {
            pressed: Some(p),
            down: true,
            alt,
            moves: vec![p],
            ..CanvasOut::default()
        }
    }

    fn drag(p: [f32; 2]) -> CanvasOut {
        CanvasOut {
            down: true,
            moves: vec![p],
            ..CanvasOut::default()
        }
    }

    fn release() -> CanvasOut {
        CanvasOut {
            released: true,
            ..CanvasOut::default()
        }
    }

    #[test]
    fn clone_stroke_needs_a_source_then_is_one_undoable_action() {
        let mut o = open();
        let mut st = RetouchState {
            size: 12.0,
            hardness: 1.0,
            ..RetouchState::default()
        };
        let n0 = o.history.labels().len();
        let orig = o.doc.layers[0].pixels.clone();
        // Sem origem: avisa e não pinta.
        assert!(st
            .handle(Tool::Clone, &mut o, &press([10.0, 10.0], false))
            .is_some());
        assert!(!st.busy());
        // Alt+clique define a origem em x=50 (laranja); o traço em x=10 copia dali.
        st.handle(Tool::Clone, &mut o, &press([50.0, 15.0], true));
        st.handle(Tool::Clone, &mut o, &press([10.0, 15.0], false));
        assert!(st.busy());
        st.handle(Tool::Clone, &mut o, &drag([14.0, 15.0]));
        st.handle(Tool::Clone, &mut o, &release());
        assert!(!st.busy());
        assert_eq!(o.doc.layers[0].pixel(10, 15), Some([220, 120, 60, 255]));
        assert_eq!(o.history.labels().len(), n0 + 1);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[0].pixels, orig);
    }

    #[test]
    fn escape_cancels_a_stroke_without_history() {
        let mut o = open();
        let orig = o.doc.layers[0].pixels.clone();
        let mut st = RetouchState::default();
        st.handle(Tool::Retouch, &mut o, &press([45.0, 15.0], false));
        st.handle(Tool::Retouch, &mut o, &drag([20.0, 15.0]));
        st.cancel(&mut o);
        assert_eq!(o.doc.layers[0].pixels, orig);
        assert_eq!(o.history.labels().len(), 0);
        assert!(!st.busy());
    }

    #[test]
    fn spot_heal_red_eye_and_patch_are_single_actions_and_respect_blocks() {
        let mut o = open();
        let mut st = RetouchState {
            size: 12.0,
            ..RetouchState::default()
        };
        // Mancha escura na área cinza.
        for y in 12..18 {
            for x in 12..18 {
                o.doc.layers[0].set_pixel(x, y, [0, 0, 0, 255]);
            }
        }
        st.handle(Tool::SpotHeal, &mut o, &press([15.0, 15.0], false));
        assert!(o.doc.layers[0].pixel(15, 15).unwrap()[0] > 60);
        assert_eq!(o.history.labels().len(), 1);
        // Olho vermelho.
        for y in 20..24 {
            for x in 20..24 {
                o.doc.layers[0].set_pixel(x, y, [230, 20, 20, 255]);
            }
        }
        st.handle(Tool::RedEye, &mut o, &press([22.0, 22.0], false));
        assert!(o.doc.layers[0].pixel(22, 22).unwrap()[0] < 120);
        // Nada vermelho: avisa.
        assert!(st
            .handle(Tool::RedEye, &mut o, &press([5.0, 5.0], false))
            .is_some());
        // Remendo exige seleção.
        assert!(st
            .handle(Tool::Patch, &mut o, &press([15.0, 15.0], false))
            .is_some());
        // Camada bloqueada: nenhuma ferramenta age.
        o.doc.layers[0].lock_pixels = true;
        assert!(st
            .handle(Tool::SpotHeal, &mut o, &press([15.0, 15.0], false))
            .is_some());
    }

    #[test]
    fn patch_drag_uses_selection_and_offset() {
        let mut o = open();
        for y in 10..20 {
            for x in 5..10 {
                o.doc.layers[0].set_pixel(x, y, [0, 0, 0, 255]);
            }
        }
        let mut mask = vec![0u8; 60 * 30];
        for y in 10..20 {
            for x in 5..10 {
                mask[y * 60 + x] = 255;
            }
        }
        o.doc.selection = orvalith_core::Selection::from_mask(Some(mask));
        let mut st = RetouchState::default();
        st.handle(Tool::Patch, &mut o, &press([7.0, 15.0], false));
        st.handle(Tool::Patch, &mut o, &drag([27.0, 15.0]));
        let mut rel = release();
        rel.moves = vec![[27.0, 15.0]];
        st.handle(Tool::Patch, &mut o, &rel);
        assert!(o.doc.layers[0].pixel(7, 15).unwrap()[0] > 70);
        assert_eq!(o.history.labels().len(), 1);
    }
}
