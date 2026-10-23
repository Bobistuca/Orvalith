//! Máscaras de camada: criar, aplicar, excluir, inverter e refinar a borda.
//!
//! Funções puras: devolvem a nova lista de camadas (aplicada por `edit::LayersEdit`).
//! A máscara nunca altera os pixels da camada até ser **aplicada** explicitamente.

use std::sync::Arc;

use crate::document::{Document, Layer, LayerId, LayerMask};
use crate::selection::{self, nonempty};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskInit {
    /// Tudo visível.
    Reveal,
    /// Tudo oculto.
    Hide,
    /// Visível só dentro da seleção.
    FromSelection,
    /// Oculto dentro da seleção.
    FromSelectionInverse,
}

fn doc_len(doc: &Document) -> usize {
    doc.width as usize * doc.height as usize
}

fn edit_layer(
    doc: &Document,
    id: LayerId,
    f: impl FnOnce(&mut Layer) -> Result<(), String>,
) -> Result<Vec<Layer>, String> {
    let mut layers = doc.layers.clone();
    let l = layers
        .iter_mut()
        .find(|l| l.id == id)
        .ok_or("Camada não encontrada.")?;
    f(l)?;
    Ok(layers)
}

/// Cria a máscara da camada `id`.
pub fn add_mask(doc: &Document, id: LayerId, init: MaskInit) -> Result<Vec<Layer>, String> {
    let n = doc_len(doc);
    let data = match init {
        MaskInit::Reveal => vec![255; n],
        MaskInit::Hide => vec![0; n],
        MaskInit::FromSelection | MaskInit::FromSelectionInverse => {
            let sel = doc
                .selection
                .mask
                .as_ref()
                .ok_or("Faça uma seleção primeiro.")?;
            if init == MaskInit::FromSelection {
                sel.as_ref().clone()
            } else {
                sel.iter().map(|&v| 255 - v).collect()
            }
        }
    };
    edit_layer(doc, id, |l| {
        if l.mask.is_some() {
            return Err("A camada já tem máscara.".into());
        }
        l.mask = Some(LayerMask::new(data));
        Ok(())
    })
}

pub fn delete_mask(doc: &Document, id: LayerId) -> Result<Vec<Layer>, String> {
    edit_layer(doc, id, |l| {
        l.mask
            .take()
            .map(|_| ())
            .ok_or_else(|| "A camada não tem máscara.".into())
    })
}

/// Aplica a máscara efetiva ao alfa dos pixels e a remove (só camadas raster).
pub fn apply_mask(doc: &Document, id: LayerId) -> Result<Vec<Layer>, String> {
    let (w, h) = (doc.width, doc.height);
    edit_layer(doc, id, |l| {
        if l.is_group() {
            return Err(
                "Grupos não podem ter a máscara aplicada (os filhos não são alterados).".into(),
            );
        }
        let m = l.mask.take().ok_or("A camada não tem máscara.")?;
        if m.enabled {
            let eff = m.effective(w, h);
            if eff.len() == l.pixels.len() / 4 {
                let px = l.pixels_mut();
                for (p, &mv) in px.as_chunks_mut::<4>().0.iter_mut().zip(eff.iter()) {
                    p[3] = ((u32::from(p[3]) * u32::from(mv) + 127) / 255) as u8;
                }
            }
        }
        Ok(())
    })
}

pub fn invert_mask(doc: &Document, id: LayerId) -> Result<Vec<Layer>, String> {
    edit_layer(doc, id, |l| {
        let m = l.mask.as_mut().ok_or("A camada não tem máscara.")?;
        for v in m.data_mut().iter_mut() {
            *v = 255 - *v;
        }
        Ok(())
    })
}

/// Seleção equivalente à máscara efetiva (None se vazia ou sem máscara).
pub fn mask_to_selection(doc: &Document, id: LayerId) -> Option<Vec<u8>> {
    let m = doc.layer(id)?.mask.as_ref()?;
    let e = m.effective(doc.width, doc.height);
    nonempty(e.as_ref().clone())
}

/// Parâmetros do refinamento de borda (algoritmos clássicos, sem IA).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RefineParams {
    /// Suavizar: raio do arredondamento de cantos/ruído (px).
    pub smooth: u32,
    /// Pena: desfoque da transição (px).
    pub feather: f32,
    /// Contraste da borda 0..=1 (endurece a transição suavizada).
    pub contrast: f32,
    /// Deslocar borda: positivo expande, negativo contrai (px).
    pub shift: i32,
}

impl Default for RefineParams {
    fn default() -> Self {
        Self {
            smooth: 0,
            feather: 0.0,
            contrast: 0.0,
            shift: 0,
        }
    }
}

/// Refina uma máscara/seleção: suavizar → deslocar borda → pena → contraste.
pub fn refine_edge(mask: &[u8], w: u32, h: u32, p: &RefineParams) -> Vec<u8> {
    let mut m = mask.to_vec();
    if p.smooth > 0 {
        let b = selection::blur_mask(&m, w, h, p.smooth.min(100) as f32);
        m = b
            .iter()
            .map(|&v| {
                ((f32::from(v) - 127.5) * 6.0 + 127.5)
                    .clamp(0.0, 255.0)
                    .round() as u8
            })
            .collect();
    }
    if p.shift != 0 {
        m = selection::morph_mask(&m, w, h, p.shift.unsigned_abs().min(100), p.shift > 0);
    }
    if p.feather > 0.0 {
        m = selection::blur_mask(&m, w, h, p.feather);
    }
    if p.contrast > 0.0 {
        let k = 1.0 + p.contrast.clamp(0.0, 1.0) * 9.0;
        for v in &mut m {
            *v = ((f32::from(*v) - 127.5) * k + 127.5)
                .clamp(0.0, 255.0)
                .round() as u8;
        }
    }
    m
}

/// Refina a máscara de uma camada (substitui os dados pintados).
pub fn refine_layer_mask(
    doc: &Document,
    id: LayerId,
    p: &RefineParams,
) -> Result<Vec<Layer>, String> {
    let (w, h) = (doc.width, doc.height);
    edit_layer(doc, id, |l| {
        let m = l.mask.as_mut().ok_or("A camada não tem máscara.")?;
        let refined = refine_edge(m.data(), w, h, p);
        m.set_data(Arc::new(refined));
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::{flatten, pixel_at};
    use crate::selection::rect_mask;
    use crate::{BlendMode, History, Selection};

    fn doc() -> Document {
        let mut d = Document::new("t", 8, 6, Some([0, 0, 0, 255])).unwrap();
        d.layers.push(Layer::filled("A", 8, 6, [200, 100, 50, 255]));
        d
    }

    fn top(d: &Document) -> LayerId {
        d.layers.last().unwrap().id
    }

    #[test]
    fn reveal_hide_and_selection_masks_change_the_composite_only() {
        let mut d = doc();
        let id = top(&d);
        let px = d.layers[1].pixels.clone();
        d.layers = add_mask(&d, id, MaskInit::Hide).unwrap();
        assert_eq!(pixel_at(&d, 2, 2), Some([0, 0, 0, 255]));
        assert_eq!(d.layers[1].pixels, px, "pixels originais preservados");
        d.layers = delete_mask(&d, id).unwrap();
        assert_eq!(pixel_at(&d, 2, 2), Some([200, 100, 50, 255]));
        d.selection = Selection::from_mask(Some(rect_mask(8, 6, (2.0, 1.0), (5.0, 4.0), true)));
        d.layers = add_mask(&d, id, MaskInit::FromSelection).unwrap();
        assert_eq!(pixel_at(&d, 3, 2), Some([200, 100, 50, 255]));
        assert_eq!(pixel_at(&d, 6, 5), Some([0, 0, 0, 255]));
        assert!(
            add_mask(&d, id, MaskInit::Reveal).is_err(),
            "já tem máscara"
        );
        d.layers = invert_mask(&d, id).unwrap();
        assert_eq!(pixel_at(&d, 3, 2), Some([0, 0, 0, 255]));
        assert_eq!(pixel_at(&d, 6, 5), Some([200, 100, 50, 255]));
        let mut none = doc();
        assert!(add_mask(&none, top(&none), MaskInit::FromSelection).is_err());
        none.layers = add_mask(&none, top(&none), MaskInit::Reveal).unwrap();
        assert_eq!(flatten(&none), flatten(&doc()));
    }

    #[test]
    fn enabled_density_and_feather_control_the_effect() {
        let mut d = doc();
        let id = top(&d);
        d.layers = add_mask(&d, id, MaskInit::Hide).unwrap();
        d.layers[1].mask.as_mut().unwrap().enabled = false;
        assert_eq!(
            pixel_at(&d, 1, 1),
            Some([200, 100, 50, 255]),
            "desativada = sem efeito"
        );
        let m = d.layers[1].mask.as_mut().unwrap();
        m.enabled = true;
        m.set_density(0.5);
        let p = pixel_at(&d, 1, 1).unwrap();
        // Densidade 50% de uma máscara totalmente oculta: metade visível.
        assert!((99..=101).contains(&p[0]), "{p:?}");
        let m = d.layers[1].mask.as_mut().unwrap();
        m.set_density(1.0);
        let mut data = vec![0u8; 48];
        for y in 0..6 {
            for x in 4..8 {
                data[y * 8 + x] = 255;
            }
        }
        m.set_data(Arc::new(data));
        assert_eq!(pixel_at(&d, 3, 2).unwrap()[0], 0);
        assert_eq!(pixel_at(&d, 5, 2).unwrap()[0], 200);
        d.layers[1].mask.as_mut().unwrap().set_feather(3.0);
        let edge = pixel_at(&d, 3, 2).unwrap()[0];
        assert!(edge > 5 && edge < 195, "pena suaviza a transição ({edge})");
    }

    #[test]
    fn apply_bakes_alpha_and_delete_discards_mask() {
        let mut d = doc();
        let id = top(&d);
        d.selection = Selection::from_mask(Some(rect_mask(8, 6, (0.0, 0.0), (4.0, 6.0), true)));
        d.layers = add_mask(&d, id, MaskInit::FromSelection).unwrap();
        let before = flatten(&d);
        d.layers = apply_mask(&d, id).unwrap();
        assert!(d.layers[1].mask.is_none());
        assert_eq!(d.layers[1].pixel(1, 1).unwrap()[3], 255);
        assert_eq!(d.layers[1].pixel(6, 1).unwrap()[3], 0);
        assert_eq!(flatten(&d), before, "aplicar não muda o resultado");
        assert!(apply_mask(&d, id).is_err());
        // Grupos não aplicam.
        let g = Layer::group("G");
        let gid = g.id;
        d.layers.push(g);
        d.layers = add_mask(&d, gid, MaskInit::Reveal).unwrap();
        assert!(apply_mask(&d, gid).is_err());
    }

    #[test]
    fn mask_on_group_hides_children_and_isolates_passthrough() {
        let mut d = Document::new("t", 4, 4, Some([10, 10, 10, 255])).unwrap();
        let g = Layer::group("G");
        let gid = g.id;
        d.layers.push(g);
        let mut child = Layer::filled("c", 4, 4, [200, 0, 0, 255]);
        child.parent = Some(gid);
        d.layers.push(child);
        d.layers = add_mask(&d, gid, MaskInit::Hide).unwrap();
        assert_eq!(pixel_at(&d, 1, 1), Some([10, 10, 10, 255]));
        d.layers = invert_mask(&d, gid).unwrap();
        assert_eq!(pixel_at(&d, 1, 1), Some([200, 0, 0, 255]));
    }

    #[test]
    fn mask_to_selection_uses_effective_mask() {
        let mut d = doc();
        let id = top(&d);
        assert!(mask_to_selection(&d, id).is_none());
        d.layers = add_mask(&d, id, MaskInit::Reveal).unwrap();
        assert_eq!(mask_to_selection(&d, id).unwrap(), vec![255; 48]);
        d.layers = add_mask(&doc(), id, MaskInit::Hide).unwrap_or_default();
    }

    #[test]
    fn mask_history_is_one_action_each() {
        let mut d = doc();
        let id = top(&d);
        let before = d.layers.clone();
        let after = add_mask(&d, id, MaskInit::Hide).unwrap();
        let mut h = History::new();
        h.execute(
            &mut d,
            Box::new(crate::edit::LayersEdit::new(
                "Máscara",
                before.clone(),
                after,
            )),
        );
        assert_eq!(pixel_at(&d, 0, 0), Some([0, 0, 0, 255]));
        h.undo(&mut d);
        assert!(d.layers[1].mask.is_none());
        assert_eq!(pixel_at(&d, 0, 0), Some([200, 100, 50, 255]));
    }

    #[test]
    fn mask_patch_undo_redo_restores_painted_bytes() {
        let mut d = doc();
        let id = top(&d);
        d.layers = add_mask(&d, id, MaskInit::Reveal).unwrap();
        let before: Vec<u8> = vec![255; 4];
        let patch = crate::edit::MaskPatch::new(
            "Pintar máscara",
            id,
            crate::Rect::new(2, 1, 2, 2),
            before,
            vec![0; 4],
        );
        let mut h = History::new();
        // Aplicada ao documento e registrada, como no fluxo de pintura.
        let mut cmd = patch;
        crate::history::Command::apply(&mut cmd, &mut d);
        h.record(&mut d, Box::new(cmd));
        assert_eq!(pixel_at(&d, 2, 1), Some([0, 0, 0, 255]));
        assert_eq!(pixel_at(&d, 4, 1), Some([200, 100, 50, 255]));
        h.undo(&mut d);
        assert_eq!(pixel_at(&d, 2, 1), Some([200, 100, 50, 255]));
        h.redo(&mut d);
        assert_eq!(pixel_at(&d, 3, 2), Some([0, 0, 0, 255]));
    }

    #[test]
    fn refine_edge_shift_feather_smooth_contrast() {
        let m = rect_mask(60, 60, (20.0, 20.0), (40.0, 40.0), true);
        let base = RefineParams::default();
        assert_eq!(
            refine_edge(&m, 60, 60, &base),
            m,
            "parâmetros zero não alteram"
        );
        let grown = refine_edge(&m, 60, 60, &RefineParams { shift: 3, ..base });
        assert_eq!(grown[30 * 60 + 18], 255);
        let shrunk = refine_edge(&m, 60, 60, &RefineParams { shift: -3, ..base });
        assert_eq!(shrunk[30 * 60 + 21], 0);
        assert_eq!(shrunk[30 * 60 + 30], 255);
        let soft = refine_edge(
            &m,
            60,
            60,
            &RefineParams {
                feather: 6.0,
                ..base
            },
        );
        assert!(soft[30 * 60 + 19] > 0 && soft[30 * 60 + 19] < 255);
        // Contraste endurece a transição suave.
        let hard = refine_edge(
            &soft,
            60,
            60,
            &RefineParams {
                contrast: 1.0,
                ..base
            },
        );
        let transitional = |v: &[u8]| v.iter().filter(|&&x| x > 10 && x < 245).count();
        assert!(transitional(&hard) < transitional(&soft));
        // Suavizar remove ruído isolado e arredonda cantos.
        let mut noisy = m.clone();
        noisy[5 * 60 + 5] = 255;
        let sm = refine_edge(&noisy, 60, 60, &RefineParams { smooth: 3, ..base });
        assert_eq!(sm[5 * 60 + 5], 0);
        assert!(sm[20 * 60 + 20] < 255);
        assert_eq!(sm[30 * 60 + 30], 255);
        let d = Document::new("r", 60, 60, Some([1, 1, 1, 255])).unwrap();
        let _ = (d, BlendMode::Normal);
    }

    #[test]
    fn refine_layer_mask_replaces_data() {
        let mut d = doc();
        let id = top(&d);
        d.selection = Selection::from_mask(Some(rect_mask(8, 6, (2.0, 2.0), (6.0, 5.0), true)));
        d.layers = add_mask(&d, id, MaskInit::FromSelection).unwrap();
        let before = d.layers[1].mask.as_ref().unwrap().data().clone();
        d.layers = refine_layer_mask(
            &d,
            id,
            &RefineParams {
                shift: 1,
                ..RefineParams::default()
            },
        )
        .unwrap();
        let after = d.layers[1].mask.as_ref().unwrap().data().clone();
        assert!(
            after.iter().filter(|&&v| v == 255).count()
                > before.iter().filter(|&&v| v == 255).count()
        );
        assert!(refine_layer_mask(&doc(), id, &RefineParams::default()).is_err());
    }
}
