//! Comandos de edição de propriedades de camada (visibilidade e opacidade).

use crate::document::{Document, LayerId};
use crate::history::Command;

fn layer_mut(doc: &mut Document, id: LayerId) -> Option<&mut crate::Layer> {
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
}
