//! Comandos de edição: propriedades de camada, edições estruturais e retalhos de pixels.

use std::sync::Arc;

use crate::document::{Channel, Document, Layer, LayerId, Selection};
use crate::geom::Rect;
use crate::history::Command;

fn layer_mut(doc: &mut Document, id: LayerId) -> Option<&mut Layer> {
    doc.layers.iter_mut().find(|l| l.id == id)
}

/// Alterna a visibilidade de uma camada.
pub struct SetVisible {
    pub layer: LayerId,
    pub old: bool,
    pub new: bool,
}

impl Command for SetVisible {
    fn label(&self) -> &str {
        "Alterar visibilidade"
    }
    fn apply(&mut self, doc: &mut Document) {
        if let Some(l) = layer_mut(doc, self.layer) {
            l.visible = self.new;
        }
    }
    fn revert(&mut self, doc: &mut Document) {
        if let Some(l) = layer_mut(doc, self.layer) {
            l.visible = self.old;
        }
    }
}

/// Altera a opacidade de uma camada (um arrasto de controle = um comando).
pub struct SetOpacity {
    pub layer: LayerId,
    pub old: f32,
    pub new: f32,
}

impl Command for SetOpacity {
    fn label(&self) -> &str {
        "Alterar opacidade"
    }
    fn apply(&mut self, doc: &mut Document) {
        if let Some(l) = layer_mut(doc, self.layer) {
            l.opacity = self.new;
        }
    }
    fn revert(&mut self, doc: &mut Document) {
        if let Some(l) = layer_mut(doc, self.layer) {
            l.opacity = self.old;
        }
    }
}

/// Troca a lista inteira de camadas (estrutura, propriedades, mesclagem, mover…).
/// Os pixels são compartilhados por `Arc`, então o instantâneo é barato e só buffers
/// realmente novos contam para o orçamento do histórico.
pub struct LayersEdit {
    label: String,
    before: Vec<Layer>,
    after: Vec<Layer>,
}

impl LayersEdit {
    pub fn new(label: &str, before: Vec<Layer>, after: Vec<Layer>) -> Self {
        Self {
            label: label.to_string(),
            before,
            after,
        }
    }
}

impl Command for LayersEdit {
    fn label(&self) -> &str {
        &self.label
    }
    fn apply(&mut self, doc: &mut Document) {
        doc.layers = self.after.clone();
    }
    fn revert(&mut self, doc: &mut Document) {
        doc.layers = self.before.clone();
    }
    fn bytes(&self) -> usize {
        let unique = |a: &[Layer], b: &[Layer]| -> usize {
            a.iter()
                .filter(|l| !b.iter().any(|o| Arc::ptr_eq(&o.pixels, &l.pixels)))
                .map(|l| l.pixels.len())
                .sum()
        };
        unique(&self.after, &self.before) + unique(&self.before, &self.after)
    }
}

/// Estado de seleção e canais, para desfazer/refazer alterações de seleção.
#[derive(Clone, Default)]
pub struct SelectionState {
    pub selection: Selection,
    pub channels: Vec<Channel>,
}

impl SelectionState {
    pub fn of(doc: &Document) -> Self {
        Self {
            selection: doc.selection.clone(),
            channels: doc.channels.clone(),
        }
    }
}

/// Altera a seleção atual e/ou os canais salvos (o `Arc` evita copiar máscaras).
pub struct SelectionEdit {
    label: String,
    before: SelectionState,
    after: SelectionState,
}

impl SelectionEdit {
    pub fn new(label: &str, before: SelectionState, after: SelectionState) -> Self {
        Self {
            label: label.to_string(),
            before,
            after,
        }
    }
}

impl Command for SelectionEdit {
    fn label(&self) -> &str {
        &self.label
    }
    fn apply(&mut self, doc: &mut Document) {
        doc.selection = self.after.selection.clone();
        doc.channels = self.after.channels.clone();
    }
    fn revert(&mut self, doc: &mut Document) {
        doc.selection = self.before.selection.clone();
        doc.channels = self.before.channels.clone();
    }
    fn bytes(&self) -> usize {
        let m = |s: &SelectionState| s.selection.mask.as_ref().map_or(0, |m| m.len());
        m(&self.before) + m(&self.after)
    }
}

/// Troca os pixels de um retângulo de uma camada (traço, balde, gradiente…).
/// Guarda apenas a área alterada, antes e depois.
pub struct PixelPatch {
    label: String,
    layer: LayerId,
    rect: Rect,
    before: Vec<u8>,
    after: Vec<u8>,
}

fn copy_rect(src: &[u8], stride_w: u32, r: Rect) -> Vec<u8> {
    let mut out = Vec::with_capacity(r.w as usize * r.h as usize * 4);
    for y in r.y..r.bottom() {
        let s = (y as usize * stride_w as usize + r.x as usize) * 4;
        out.extend_from_slice(&src[s..s + r.w as usize * 4]);
    }
    out
}

fn write_rect(dst: &mut [u8], stride_w: u32, r: Rect, data: &[u8]) {
    let row = r.w as usize * 4;
    for (i, y) in (r.y..r.bottom()).enumerate() {
        let d = (y as usize * stride_w as usize + r.x as usize) * 4;
        dst[d..d + row].copy_from_slice(&data[i * row..(i + 1) * row]);
    }
}

impl PixelPatch {
    /// Monta o retalho a partir do estado anterior (`before_pixels`, camada inteira)
    /// e do estado atual da camada, restritos a `rect`.
    pub fn from_diff(label: &str, layer: &Layer, before_pixels: &[u8], rect: Rect) -> Option<Self> {
        let rect = rect.clamp_to(layer.width, layer.height);
        if rect.is_empty() || before_pixels.len() != layer.pixels.len() {
            return None;
        }
        Some(Self {
            label: label.to_string(),
            layer: layer.id,
            rect,
            before: copy_rect(before_pixels, layer.width, rect),
            after: copy_rect(&layer.pixels, layer.width, rect),
        })
    }

    fn write(&self, doc: &mut Document, data: &[u8]) {
        if let Some(l) = layer_mut(doc, self.layer) {
            if self.rect.right() <= l.width && self.rect.bottom() <= l.height {
                let w = l.width;
                write_rect(l.pixels_mut(), w, self.rect, data);
            }
        }
    }

    pub fn rect(&self) -> Rect {
        self.rect
    }
}

impl Command for PixelPatch {
    fn label(&self) -> &str {
        &self.label
    }
    fn apply(&mut self, doc: &mut Document) {
        let data = std::mem::take(&mut self.after);
        self.write(doc, &data);
        self.after = data;
    }
    fn revert(&mut self, doc: &mut Document) {
        let data = std::mem::take(&mut self.before);
        self.write(doc, &data);
        self.before = data;
    }
    fn bytes(&self) -> usize {
        self.before.len() + self.after.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::History;

    #[test]
    fn visibility_and_opacity_undo_redo() {
        let mut doc = Document::new("t", 2, 2, Some([1, 2, 3, 255])).unwrap();
        let id = doc.layers[0].id;
        let mut h = History::new();
        h.execute(
            &mut doc,
            Box::new(SetVisible {
                layer: id,
                old: true,
                new: false,
            }),
        );
        assert!(!doc.layers[0].visible);
        // Arrasto já aplicado ao documento, depois registrado como uma só ação.
        doc.layers[0].opacity = 0.25;
        h.record(
            &mut doc,
            Box::new(SetOpacity {
                layer: id,
                old: 1.0,
                new: 0.25,
            }),
        );
        assert_eq!(h.undo_label(), Some("Alterar opacidade"));
        h.undo(&mut doc);
        assert_eq!(doc.layers[0].opacity, 1.0);
        assert!(!doc.layers[0].visible);
        h.undo(&mut doc);
        assert!(doc.layers[0].visible);
        h.redo(&mut doc);
        h.redo(&mut doc);
        assert_eq!(doc.layers[0].opacity, 0.25);
        assert!(!h.can_redo());
    }

    #[test]
    fn layers_edit_counts_only_new_buffers() {
        let mut doc = Document::new("t", 4, 4, Some([1, 2, 3, 255])).unwrap();
        let before = doc.layers.clone();
        let mut after = before.clone();
        after.push(Layer::transparent("Nova", 4, 4));
        after[0].name = "Renomeada".into();
        let e = LayersEdit::new("x", before, after);
        assert_eq!(e.bytes(), 4 * 4 * 4); // só o buffer da camada nova
        let mut h = History::new();
        h.execute(&mut doc, Box::new(e));
        assert_eq!(doc.layers.len(), 2);
        h.undo(&mut doc);
        assert_eq!(doc.layers.len(), 1);
        assert_eq!(doc.layers[0].name, "Fundo");
    }

    #[test]
    fn pixel_patch_roundtrip_only_touches_its_rect() {
        let mut doc = Document::new("t", 4, 4, Some([0, 0, 0, 255])).unwrap();
        let before = doc.layers[0].pixels.clone();
        doc.layers[0].set_pixel(1, 1, [9, 9, 9, 255]);
        doc.layers[0].set_pixel(2, 2, [8, 8, 8, 255]);
        let patch =
            PixelPatch::from_diff("Teste", &doc.layers[0], &before, Rect::new(1, 1, 2, 2)).unwrap();
        let painted = doc.layers[0].pixels.clone();
        let mut h = History::new();
        h.record(&mut doc, Box::new(patch));
        h.undo(&mut doc);
        assert_eq!(doc.layers[0].pixels, before);
        h.redo(&mut doc);
        assert_eq!(doc.layers[0].pixels, painted);
    }
}
