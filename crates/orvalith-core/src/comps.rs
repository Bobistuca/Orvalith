//! Composições de camadas: instantâneos nomeados do estado visual (visibilidade, opacidade,
//! preenchimento, modo de mesclagem e quais estilos estão ativos) para alternar entre variações.
//!
//! Não guardam posição nem pixels: mover ou pintar camadas não faz parte da composição.

use serde::{Deserialize, Serialize};

use crate::blend::BlendMode;
use crate::document::{Document, Layer, LayerId};
use crate::history::Command;

pub const MAX_COMPS: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerState {
    pub layer: u64,
    pub visible: bool,
    pub opacity: f32,
    pub fill: f32,
    pub blend: BlendMode,
    /// Estado ativo/inativo de cada estilo, na ordem da lista.
    pub styles: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comp {
    pub name: String,
    pub states: Vec<LayerState>,
}

fn state_of(l: &Layer) -> LayerState {
    LayerState {
        layer: l.id.0,
        visible: l.visible,
        opacity: l.opacity,
        fill: l.fill,
        blend: l.blend,
        styles: l.styles.iter().map(|e| e.enabled).collect(),
    }
}

/// Fotografa o estado visual atual de todas as camadas.
pub fn capture(doc: &Document, name: &str) -> Comp {
    Comp {
        name: name.trim().chars().take(80).collect(),
        states: doc.layers.iter().map(state_of).collect(),
    }
}

/// Aplica a composição: camadas que não existem mais são ignoradas; camadas novas ficam como estão.
pub fn apply(doc: &Document, comp: &Comp) -> Vec<Layer> {
    let mut layers = doc.layers.clone();
    for s in &comp.states {
        let Some(l) = layers.iter_mut().find(|l| l.id.0 == s.layer) else {
            continue;
        };
        l.visible = s.visible;
        l.opacity = if s.opacity.is_finite() {
            s.opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        l.fill = if s.fill.is_finite() {
            s.fill.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if l.is_group() || s.blend != BlendMode::PassThrough {
            l.blend = s.blend;
        }
        for (e, on) in l.styles.iter_mut().zip(&s.styles) {
            e.enabled = *on;
        }
    }
    layers
}

/// O estado atual coincide com a composição (nas camadas que ela conhece)?
pub fn matches(doc: &Document, comp: &Comp) -> bool {
    comp.states
        .iter()
        .all(|s| match doc.layer(LayerId(s.layer)) {
            Some(l) => {
                let cur = state_of(l);
                cur.visible == s.visible
                    && (cur.opacity - s.opacity).abs() < 1e-4
                    && (cur.fill - s.fill).abs() < 1e-4
                    && cur.blend == s.blend
                    && s.styles.iter().zip(&cur.styles).all(|(a, b)| a == b)
            }
            None => true,
        })
}

/// Valida composições lidas de arquivo (limites de quantidade e de camadas).
pub fn comps_sane(list: &[Comp], max_layers: usize) -> bool {
    list.len() <= MAX_COMPS
        && list.iter().all(|c| {
            c.states.len() <= max_layers.max(1) * 2 && c.states.iter().all(|s| s.styles.len() <= 16)
        })
}

/// Troca a lista de composições (adicionar, atualizar, renomear, excluir) em uma ação.
pub struct CompsEdit {
    label: String,
    before: Vec<Comp>,
    after: Vec<Comp>,
}

impl CompsEdit {
    pub fn new(label: &str, before: Vec<Comp>, after: Vec<Comp>) -> Self {
        Self {
            label: label.to_string(),
            before,
            after,
        }
    }
}

impl Command for CompsEdit {
    fn label(&self) -> &str {
        &self.label
    }
    fn apply(&mut self, doc: &mut Document) {
        doc.comps = self.after.clone();
    }
    fn revert(&mut self, doc: &mut Document) {
        doc.comps = self.before.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Effect, EffectKind};

    fn doc() -> Document {
        let mut d = Document::new("c", 6, 6, Some([255, 255, 255, 255])).unwrap();
        for (i, c) in [[255, 0, 0, 255], [0, 255, 0, 255]].into_iter().enumerate() {
            let mut l = Layer::filled(format!("L{i}"), 6, 6, c);
            l.styles = vec![Effect::new(EffectKind::neutral(5))];
            d.layers.push(l);
        }
        d
    }

    #[test]
    fn capture_apply_roundtrip_restores_the_visual_state() {
        let mut d = doc();
        let a = capture(&d, "  Versão A ");
        assert_eq!(a.name, "Versão A");
        d.layers[1].visible = false;
        d.layers[2].opacity = 0.4;
        d.layers[2].blend = BlendMode::Multiply;
        d.layers[2].styles[0].enabled = false;
        let b = capture(&d, "B");
        assert!(matches(&d, &b) && !matches(&d, &a));
        d.layers = apply(&d, &a);
        assert!(matches(&d, &a) && !matches(&d, &b));
        assert!(d.layers[1].visible && d.layers[2].opacity == 1.0 && d.layers[2].styles[0].enabled);
        d.layers = apply(&d, &b);
        assert!(!d.layers[1].visible && d.layers[2].blend == BlendMode::Multiply);
    }

    #[test]
    fn deleted_and_new_layers_are_tolerated_and_pixels_never_change() {
        let mut d = doc();
        let comp = capture(&d, "x");
        let px = d.layers[1].pixels.clone();
        d.layers.push(Layer::transparent("nova", 6, 6));
        d.layers[3].visible = false;
        let removed = d.layers.remove(1);
        let out = apply(&d, &comp);
        assert!(!out[2].visible, "camada nova fica como estava");
        assert_eq!(removed.pixels, px);
        assert!(matches(
            &Document {
                layers: out,
                ..d.clone()
            },
            &comp
        ));
    }

    #[test]
    fn comps_edit_is_undoable_and_hostile_values_are_clamped() {
        let mut d = doc();
        let mut h = crate::History::default();
        let c = capture(&d, "um");
        h.execute(
            &mut d,
            Box::new(CompsEdit::new("Nova composição", vec![], vec![c.clone()])),
        );
        assert_eq!(d.comps.len(), 1);
        h.undo(&mut d);
        assert!(d.comps.is_empty());
        let mut bad = c;
        bad.states[1].opacity = f32::NAN;
        bad.states[1].fill = 9.0;
        let out = apply(&d, &bad);
        assert_eq!((out[1].opacity, out[1].fill), (1.0, 1.0));
        assert!(!comps_sane(&vec![bad.clone(); 65], 3));
        assert!(comps_sane(&[bad], 3));
    }
}
