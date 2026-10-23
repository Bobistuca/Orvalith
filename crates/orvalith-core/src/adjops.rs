//! Operações de documento para ajustes: camadas de ajuste e aplicação destrutiva.

use std::sync::Arc;

use crate::adjust::Adjustment;
use crate::document::{Document, Layer, LayerId};
use crate::layerops::{normalize, Outcome};

/// Nova camada de ajuste (pixels transparentes) logo acima de `above` ou no topo.
pub fn new_adjustment_layer(doc: &Document, adj: Adjustment, above: Option<LayerId>) -> Outcome {
    let mut adj = adj;
    adj.sanitize();
    let mut layer = Layer::transparent(adj.name(), doc.width, doc.height);
    layer.adjustment = Some(adj);
    let (parent, at) = match above.and_then(|id| doc.layer(id)) {
        Some(a) => (
            a.parent,
            doc.index_of(a.id).unwrap_or(0) + 1 + doc.descendants(a.id).len(),
        ),
        None => (None, doc.layers.len()),
    };
    layer.parent = parent;
    let id = layer.id;
    let mut layers = doc.layers.clone();
    layers.insert(at.min(layers.len()), layer);
    Outcome {
        layers: normalize(layers),
        select: vec![id],
        active: Some(id),
    }
}

/// Pixels ajustados; `sel` (1 byte por pixel) escolhe quanto do efeito vale em cada ponto.
pub fn adjusted_pixels(orig: &[u8], adj: &Adjustment, sel: Option<&[u8]>) -> Vec<u8> {
    let mut out = orig.to_vec();
    adj.prepare().apply_rgba(&mut out);
    if let Some(m) = sel {
        for (i, (o, src)) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(orig.as_chunks::<4>().0)
            .enumerate()
        {
            let w = u32::from(m.get(i).copied().unwrap_or(0));
            if w == 255 {
                continue;
            }
            for c in 0..3 {
                o[c] = ((u32::from(o[c]) * w + u32::from(src[c]) * (255 - w) + 127) / 255) as u8;
            }
        }
    }
    out
}

/// Motivo pelo qual um ajuste destrutivo não pode ser aplicado à camada, se houver.
pub fn destructive_blocked(l: &Layer) -> Option<&'static str> {
    if l.is_group() {
        Some("Selecione uma camada raster (não um grupo).")
    } else if l.adjustment.is_some() {
        Some("A camada ativa já é uma camada de ajuste; edite os parâmetros dela.")
    } else if l.source.is_some() {
        Some("Camada de texto/forma: use Camada → Rasterizar antes de ajustar.")
    } else if l.lock_pixels {
        Some("Os pixels da camada ativa estão bloqueados.")
    } else {
        None
    }
}

/// Aplica o ajuste de forma destrutiva à camada `id`, respeitando a seleção.
pub fn apply_to_layer(doc: &Document, id: LayerId, adj: &Adjustment) -> Result<Vec<Layer>, String> {
    let mut layers = doc.layers.clone();
    let l = layers
        .iter_mut()
        .find(|l| l.id == id)
        .ok_or("Camada não encontrada.")?;
    if let Some(msg) = destructive_blocked(l) {
        return Err(msg.into());
    }
    let sel = doc.selection.mask.as_ref().map(|m| m.as_slice());
    l.pixels = Arc::new(adjusted_pixels(&l.pixels, adj, sel));
    Ok(layers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose;

    fn doc() -> Document {
        let mut d = Document::new("a", 4, 1, Some([200, 100, 50, 255])).unwrap();
        d.layers[0].set_pixel(3, 0, [10, 20, 30, 128]);
        d
    }

    #[test]
    fn adjustment_layer_changes_composite_and_is_reversible_by_removal() {
        let d = doc();
        let before = compose::flatten(&d);
        let o = new_adjustment_layer(&d, Adjustment::Invert, None);
        let mut d2 = d.clone();
        d2.layers = o.layers;
        assert_eq!(d2.layers.len(), 2);
        let after = compose::flatten(&d2);
        assert_eq!(&after[0..4], &[55, 155, 205, 255]);
        assert_eq!(after[15], before[15], "alfa preservado");
        // Esconder a camada restaura exatamente o original.
        d2.layers[1].visible = false;
        assert_eq!(compose::flatten(&d2), before);
        // Pixels da camada de baixo nunca foram tocados.
        assert_eq!(d2.layers[0].pixels, d.layers[0].pixels);
    }

    #[test]
    fn adjustment_layer_respects_opacity_mask_clipping_and_blend() {
        let d = doc();
        let o = new_adjustment_layer(&d, Adjustment::Invert, None);
        let mut d2 = d.clone();
        d2.layers = o.layers;
        d2.layers[1].opacity = 0.5;
        let px = compose::pixel_at(&d2, 0, 0).unwrap();
        assert!((i32::from(px[0]) - 128).abs() <= 1, "{px:?}"); // meio caminho 200 → 55
                                                                // Máscara: oculta nos dois últimos pixels.
        d2.layers[1].opacity = 1.0;
        let mut mask = vec![255u8; 4];
        mask[1] = 0;
        d2.layers[1].mask = Some(crate::LayerMask::new(mask));
        assert_eq!(compose::pixel_at(&d2, 1, 0), Some([200, 100, 50, 255]));
        assert_eq!(compose::pixel_at(&d2, 0, 0), Some([55, 155, 205, 255]));
        // Recortada: age somente sobre a camada logo abaixo.
        d2.layers[1].mask = None;
        let base = Layer::filled("outra", 4, 1, [0, 0, 0, 255]);
        d2.layers.insert(1, base);
        d2.layers[2].clipped = true;
        assert_eq!(compose::pixel_at(&d2, 0, 0), Some([255, 255, 255, 255]));
        // Modo Multiplicar com o ajuste "Inverter" escurece em vez de substituir.
        d2.layers[2].clipped = false;
        d2.layers[1] = Layer::filled("base", 4, 1, [100, 100, 100, 255]);
        d2.layers[2].blend = crate::BlendMode::Multiply;
        let m = compose::pixel_at(&d2, 0, 0).unwrap();
        assert!(m[0] < 100, "{m:?}");
    }

    #[test]
    fn destructive_apply_respects_selection_and_blocks_special_layers() {
        let mut d = doc();
        d.selection = crate::Selection::from_mask(Some(vec![255, 255, 0, 0]));
        let id = d.layers[0].id;
        let layers = apply_to_layer(&d, id, &Adjustment::Invert).unwrap();
        assert_eq!(layers[0].pixel(0, 0), Some([55, 155, 205, 255]));
        assert_eq!(layers[0].pixel(2, 0), Some([200, 100, 50, 255]));
        assert_eq!(layers[0].pixel(3, 0).unwrap()[3], 128);
        // Camada de ajuste e camada bloqueada não aceitam ajuste destrutivo.
        let o = new_adjustment_layer(&d, Adjustment::Invert, None);
        let mut d2 = d.clone();
        d2.layers = o.layers;
        let aid = d2.layers[1].id;
        assert!(apply_to_layer(&d2, aid, &Adjustment::Invert).is_err());
        d2.layers[0].lock_pixels = true;
        assert!(apply_to_layer(&d2, id, &Adjustment::Invert).is_err());
    }

    #[test]
    fn partial_selection_blends_between_original_and_adjusted() {
        let orig = vec![200, 100, 50, 255];
        let out = adjusted_pixels(&orig, &Adjustment::Invert, Some(&[128]));
        assert!((i32::from(out[0]) - 127).abs() <= 2, "{out:?}");
    }
    #[test]
    fn adjustments_stay_editable_after_save_and_reopen() {
        let d = doc();
        let o = new_adjustment_layer(&d, Adjustment::neutral(4), None);
        let mut d2 = d.clone();
        d2.layers = o.layers;
        if let Some(Adjustment::HueSaturation { saturation, .. }) = d2.layers[1].adjustment.as_mut()
        {
            *saturation = -100.0;
        }
        let expect = compose::flatten(&d2);
        let reopened = crate::orva::read(&crate::orva::write(&d2).unwrap()).unwrap();
        assert_eq!(compose::flatten(&reopened), expect);
        // Depois de reabrir, ainda dá para editar o parâmetro e o resultado muda.
        let mut again = reopened;
        if let Some(Adjustment::HueSaturation { saturation, .. }) =
            again.layers[1].adjustment.as_mut()
        {
            *saturation = 0.0;
        }
        assert_eq!(compose::flatten(&again), compose::flatten(&d));
    }

    #[test]
    fn merging_an_adjustment_layer_bakes_it_into_the_layer_below() {
        let d = doc();
        let o = new_adjustment_layer(&d, Adjustment::Invert, None);
        let mut d2 = d.clone();
        d2.layers = o.layers;
        let expect = compose::flatten(&d2);
        let ids = [d2.layers[0].id, d2.layers[1].id];
        let merged = crate::layerops::merge_selected(&d2, &ids).unwrap();
        let mut d3 = d2.clone();
        d3.layers = merged.layers;
        assert_eq!(d3.layers.len(), 1);
        assert_eq!(compose::flatten(&d3), expect);
    }
}
