//! Comandos de edição: propriedades de camada, camadas novas e retalhos de pixels.

use crate::document::{Document, Layer, LayerId};
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

/// Insere uma camada na posição `index` da pilha.
pub struct AddLayer {
    layer: Option<Layer>,
    id: LayerId,
    index: usize,
}

impl AddLayer {
    pub fn new(layer: Layer, index: usize) -> Self {
        Self {
            id: layer.id,
            layer: Some(layer),
            index,
        }
    }
}

impl Command for AddLayer {
    fn label(&self) -> &str {
        "Nova camada"
    }
    fn apply(&mut self, doc: &mut Document) {
        if let Some(l) = self.layer.take() {
            let at = self.index.min(doc.layers.len());
            doc.layers.insert(at, l);
        }
    }
    fn revert(&mut self, doc: &mut Document) {
        if let Some(p) = doc.layers.iter().position(|l| l.id == self.id) {
            self.layer = Some(doc.layers.remove(p));
        }
    }
    fn bytes(&self) -> usize {
        self.layer.as_ref().map_or(0, |l| l.pixels.len())
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
                write_rect(&mut l.pixels, l.width, self.rect, data);
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
    fn add_layer_undo_redo_keeps_identity() {
        let mut doc = Document::new("t", 2, 2, Some([1, 2, 3, 255])).unwrap();
        let layer = Layer::transparent("Nova", 2, 2);
        let id = layer.id;
        let mut h = History::new();
        h.execute(&mut doc, Box::new(AddLayer::new(layer, 1)));
        assert_eq!(doc.layers.len(), 2);
        h.undo(&mut doc);
        assert_eq!(doc.layers.len(), 1);
        h.redo(&mut doc);
        assert_eq!(doc.layers[1].id, id);
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
