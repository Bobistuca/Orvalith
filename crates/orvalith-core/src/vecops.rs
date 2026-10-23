//! Operações de camadas de texto e formas: criar, editar, combinar, rasterizar e converter
//! caminhos em seleção/máscara vetorial. Funções puras (devolvem a nova lista de camadas).

use crate::document::{Document, Layer, LayerId, LayerMask};
use crate::layerops::{normalize, Outcome};
use crate::source::LayerSource;
use crate::vector::{self, BoolOp, ShapeSpec};

/// Insere `layer` logo acima de `above` (mesmo pai) ou no topo.
fn insert_above(doc: &Document, above: Option<LayerId>, mut layer: Layer) -> Outcome {
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

/// Nova camada editável (texto ou forma) com a origem dada.
pub fn new_source_layer(doc: &Document, source: LayerSource, above: Option<LayerId>) -> Outcome {
    let px = source.render(doc.width, doc.height);
    let mut layer = Layer::from_pixels(source.default_name(), doc.width, doc.height, px);
    layer.source = Some(source);
    insert_above(doc, above, layer)
}

/// Troca a origem de uma camada editável e re-renderiza (o nome só muda se ainda for o automático).
pub fn update_source(
    doc: &Document,
    id: LayerId,
    source: LayerSource,
) -> Result<Vec<Layer>, String> {
    let mut layers = doc.layers.clone();
    let l = layers
        .iter_mut()
        .find(|l| l.id == id)
        .ok_or("Camada não encontrada.")?;
    if l.source.is_none() {
        return Err("A camada foi rasterizada e não é mais editável.".into());
    }
    let auto_name = l.source.as_ref().map(LayerSource::default_name);
    if auto_name.as_deref() == Some(l.name.as_str()) {
        l.name = source.default_name();
    }
    l.source = Some(source);
    l.rerender(doc.width, doc.height);
    Ok(layers)
}

/// Rasteriza explicitamente: mantém os pixels e descarta a origem editável.
pub fn rasterize(doc: &Document, id: LayerId) -> Result<Vec<Layer>, String> {
    let mut layers = doc.layers.clone();
    let l = layers
        .iter_mut()
        .find(|l| l.id == id)
        .ok_or("Camada não encontrada.")?;
    if l.source.take().is_none() {
        return Err("A camada já é raster.".into());
    }
    Ok(layers)
}

/// Move a origem editável (sem perda: a camada é re-renderizada na nova posição).
pub fn translate_source(layer: &mut Layer, dx: f32, dy: f32, w: u32, h: u32) {
    if let Some(src) = layer.source.as_mut() {
        src.translate(dx, dy);
        layer.rerender(w, h);
    }
}

fn shape_of(doc: &Document, id: LayerId) -> Result<ShapeSpec, String> {
    match doc.layer(id).and_then(|l| l.source.as_ref()) {
        Some(LayerSource::Shape(s)) => Ok(s.clone()),
        _ => Err("Selecione camadas de forma (não rasterizadas).".into()),
    }
}

/// Combina duas camadas de forma (`a` = base, `b` = operando) numa forma booleana editável,
/// na posição de `a`; `b` é removida. Preserva as duas formas como filhas editáveis.
pub fn boolean_combine(
    doc: &Document,
    a: LayerId,
    b: LayerId,
    op: BoolOp,
) -> Result<Outcome, String> {
    if a == b {
        return Err("Escolha duas camadas diferentes.".into());
    }
    let (sa, sb) = (shape_of(doc, a)?, shape_of(doc, b)?);
    let combined = ShapeSpec::boolean(op, sa, sb);
    let mut layers = doc.layers.clone();
    let la = layers
        .iter_mut()
        .find(|l| l.id == a)
        .ok_or("Camada não encontrada.")?;
    la.name = format!(
        "{} ({})",
        LayerSource::Shape(combined.clone()).default_name(),
        op.label()
    );
    la.source = Some(LayerSource::Shape(combined));
    la.rerender(doc.width, doc.height);
    layers.retain(|l| l.id != b);
    Ok(Outcome {
        layers: normalize(layers),
        select: vec![a],
        active: Some(a),
    })
}

/// Seleção (cobertura) equivalente à forma da camada.
pub fn shape_to_selection(doc: &Document, id: LayerId) -> Option<Vec<u8>> {
    let spec = shape_of(doc, id).ok()?;
    crate::selection::nonempty(vector::coverage(&spec, doc.width, doc.height))
}

/// Usa a forma da camada `shape` como máscara vetorial de `target`; a camada de forma fica
/// oculta (mas editável) para ser reutilizada.
pub fn vector_mask_from_shape(
    doc: &Document,
    shape: LayerId,
    target: LayerId,
) -> Result<Vec<Layer>, String> {
    if shape == target {
        return Err("Escolha uma camada de destino diferente da forma.".into());
    }
    let spec = shape_of(doc, shape)?;
    let data = vector::coverage(&spec, doc.width, doc.height);
    let mut layers = doc.layers.clone();
    {
        let t = layers
            .iter_mut()
            .find(|l| l.id == target)
            .ok_or("Camada de destino não encontrada.")?;
        if t.mask.is_some() {
            return Err("A camada de destino já tem máscara.".into());
        }
        let mut m = LayerMask::new(data);
        m.set_vector(spec, m.data().as_ref().clone());
        t.mask = Some(m);
    }
    if let Some(s) = layers.iter_mut().find(|l| l.id == shape) {
        s.visible = false;
    }
    Ok(layers)
}

/// Famílias de fonte usadas por camadas de texto que não estão instaladas.
pub fn missing_fonts(doc: &Document) -> Vec<String> {
    let mut out: Vec<String> = doc
        .layers
        .iter()
        .filter_map(|l| match &l.source {
            Some(LayerSource::Text(t))
                if !t.family.is_empty() && !crate::text::font_installed(&t.family) =>
            {
                Some(t.family.clone())
            }
            _ => None,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::{flatten, pixel_at};
    use crate::text::{self, TextAlign, TextSpec};
    use crate::vector::{BezierPath, Fill, Geom, PathNode, StrokeSpec};
    use crate::{orva, History};

    fn rect(x: f32, y: f32, w: f32, h: f32, c: [u8; 4]) -> ShapeSpec {
        ShapeSpec::solid(
            Geom::Rect {
                x,
                y,
                w,
                h,
                radius: 0.0,
            },
            c,
        )
    }

    fn doc() -> Document {
        Document::new("t", 80, 60, Some([255, 255, 255, 255])).unwrap()
    }

    fn add(d: &mut Document, src: LayerSource) -> LayerId {
        let o = new_source_layer(d, src, d.layers.last().map(|l| l.id));
        d.layers = o.layers;
        o.active.unwrap()
    }

    #[test]
    fn shape_layer_edits_rerender_and_keep_editability_until_rasterized() {
        let mut d = doc();
        let id = add(
            &mut d,
            LayerSource::Shape(rect(10.0, 10.0, 20.0, 20.0, [255, 0, 0, 255])),
        );
        assert_eq!(d.layer(id).unwrap().name, "Retângulo");
        assert_eq!(pixel_at(&d, 15, 15), Some([255, 0, 0, 255]));
        d.layers = update_source(
            &d,
            id,
            LayerSource::Shape(rect(40.0, 10.0, 20.0, 20.0, [0, 0, 255, 255])),
        )
        .unwrap();
        assert_eq!(pixel_at(&d, 15, 15), Some([255, 255, 255, 255]));
        assert_eq!(pixel_at(&d, 45, 15), Some([0, 0, 255, 255]));
        d.layers = rasterize(&d, id).unwrap();
        assert!(d.layer(id).unwrap().source.is_none());
        assert_eq!(
            pixel_at(&d, 45, 15),
            Some([0, 0, 255, 255]),
            "pixels preservados"
        );
        assert!(
            update_source(&d, id, LayerSource::Shape(rect(0.0, 0.0, 1.0, 1.0, [0; 4]))).is_err()
        );
        assert!(rasterize(&d, id).is_err());
    }

    #[test]
    fn translate_source_moves_without_losing_content_off_canvas() {
        let mut d = doc();
        let id = add(
            &mut d,
            LayerSource::Shape(rect(60.0, 10.0, 30.0, 10.0, [0, 128, 0, 255])),
        );
        // Metade da forma está fora da tela; mover para dentro revela tudo (sem perda).
        let (w, h) = (d.width, d.height);
        let l = d.layer_mut(id).unwrap();
        translate_source(l, -30.0, 0.0, w, h);
        assert_eq!(pixel_at(&d, 45, 15), Some([0, 128, 0, 255]));
        assert_eq!(pixel_at(&d, 58, 15), Some([0, 128, 0, 255]));
    }

    #[test]
    fn boolean_combine_keeps_children_and_removes_operand() {
        let mut d = doc();
        let a = add(
            &mut d,
            LayerSource::Shape(rect(10.0, 10.0, 30.0, 30.0, [255, 0, 0, 255])),
        );
        let b = add(
            &mut d,
            LayerSource::Shape(rect(25.0, 10.0, 30.0, 30.0, [0, 0, 255, 255])),
        );
        let o = boolean_combine(&d, a, b, BoolOp::Subtract).unwrap();
        d.layers = o.layers;
        assert!(d.layer(b).is_none() && d.layers.len() == 2);
        assert_eq!(pixel_at(&d, 15, 20), Some([255, 0, 0, 255]));
        assert_eq!(
            pixel_at(&d, 30, 20),
            Some([255, 255, 255, 255]),
            "interseção subtraída"
        );
        match &d.layer(a).unwrap().source {
            Some(LayerSource::Shape(s)) => assert!(matches!(
                s.geom,
                Geom::Boolean {
                    op: BoolOp::Subtract,
                    ..
                }
            )),
            other => panic!("{other:?}"),
        }
        assert!(boolean_combine(&d, a, a, BoolOp::Union).is_err());
        let text = add(
            &mut d,
            LayerSource::Text(TextSpec::point("x", 0.0, 0.0, [0; 4])),
        );
        assert!(
            boolean_combine(&d, a, text, BoolOp::Union).is_err(),
            "só formas"
        );
    }

    #[test]
    fn path_to_selection_and_vector_mask() {
        let mut d = doc();
        let base = Layer::filled("Foto", 80, 60, [200, 100, 50, 255]);
        let base_id = base.id;
        d.layers.push(base);
        let tri = BezierPath {
            closed: true,
            nodes: vec![
                PathNode::corner(10.0, 50.0),
                PathNode::corner(70.0, 50.0),
                PathNode::corner(40.0, 5.0),
            ],
        };
        let shape = add(
            &mut d,
            LayerSource::Shape(ShapeSpec::solid(Geom::Path(tri), [0, 0, 0, 255])),
        );
        let sel = shape_to_selection(&d, shape).unwrap();
        assert_eq!(sel[(30 * 80 + 40) as usize], 255);
        assert_eq!(sel[0], 0);
        d.layers = vector_mask_from_shape(&d, shape, base_id).unwrap();
        let m = d.layer(base_id).unwrap().mask.as_ref().unwrap();
        assert!(m.vector_source().is_some());
        assert!(
            !d.layer(shape).unwrap().visible,
            "a forma fica oculta mas editável"
        );
        assert_eq!(pixel_at(&d, 40, 30), Some([200, 100, 50, 255]));
        assert_eq!(
            pixel_at(&d, 5, 5),
            Some([255, 255, 255, 255]),
            "fora do triângulo: fundo branco"
        );
        assert!(
            vector_mask_from_shape(&d, shape, base_id).is_err(),
            "já tem máscara"
        );
        // Pintar na máscara converte em raster (perde a origem vetorial).
        d.layer_mut(base_id)
            .unwrap()
            .mask
            .as_mut()
            .unwrap()
            .data_mut()[0] = 0;
        assert!(d
            .layer(base_id)
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .vector_source()
            .is_none());
    }

    /// Cartaz: imagem, título com acentos, parágrafo, formas e caminho; reabrir e editar.
    #[test]
    fn poster_roundtrip_keeps_text_and_vectors_editable() {
        if text::list_fonts().is_empty() {
            eprintln!("(sem fontes: teste do cartaz ignorado)");
            return;
        }
        let mut d = Document::new("cartaz", 300, 200, Some([250, 245, 235, 255])).unwrap();
        let mut photo = Layer::transparent("Imagem", 300, 200);
        for y in 120..200 {
            for x in 0..300 {
                photo.set_pixel(x, y, [(x / 2) as u8, 120, (y) as u8, 255]);
            }
        }
        d.layers.push(photo);
        let title = TextSpec {
            size: 28.0,
            bold: true,
            ..TextSpec::point("Festa da Colheita — Ação!", 12.0, 10.0, [30, 30, 90, 255])
        };
        let para = TextSpec {
            size: 12.0,
            box_width: Some(160.0),
            align: TextAlign::Left,
            ..TextSpec::point(
                "Venha celebrar com música, comida e muita alegria no coração da cidade.",
                12.0,
                50.0,
                [20, 20, 20, 255],
            )
        };
        let t_id = add(&mut d, LayerSource::Text(title.clone()));
        let p_id = add(&mut d, LayerSource::Text(para.clone()));
        let star = ShapeSpec {
            geom: Geom::Star {
                cx: 240.0,
                cy: 60.0,
                outer: 40.0,
                inner: 18.0,
                points: 5,
                rotation: 0.0,
            },
            fill: Fill::Linear {
                x0: 200.0,
                y0: 20.0,
                x1: 280.0,
                y1: 100.0,
                c0: [255, 200, 0, 255],
                c1: [220, 40, 40, 255],
            },
            stroke: Some(StrokeSpec {
                color: [90, 40, 0, 255],
                width: 2.0,
            }),
        };
        let s_id = add(&mut d, LayerSource::Shape(star));
        let curve = BezierPath {
            closed: false,
            nodes: vec![
                PathNode {
                    x: 20.0,
                    y: 110.0,
                    hin: (0.0, 0.0),
                    hout: (30.0, -25.0),
                },
                PathNode {
                    x: 150.0,
                    y: 110.0,
                    hin: (-30.0, -25.0),
                    hout: (0.0, 0.0),
                },
            ],
        };
        let c_id = add(
            &mut d,
            LayerSource::Shape(ShapeSpec {
                geom: Geom::Path(curve),
                fill: Fill::None,
                stroke: Some(StrokeSpec {
                    color: [0, 120, 80, 255],
                    width: 4.0,
                }),
            }),
        );
        let reference = flatten(&d);
        let back = orva::read(&orva::write(&d).unwrap()).unwrap();
        assert_eq!(flatten(&back), reference, "reabrir não muda a aparência");
        for id in [t_id, p_id, s_id, c_id] {
            assert_eq!(
                back.layer(id).unwrap().source,
                d.layer(id).unwrap().source,
                "origem preservada"
            );
        }
        // Editar o título e a estrela depois de reabrir.
        let mut back = back;
        let new_title = TextSpec {
            text: "Festa Junina".into(),
            ..title
        };
        back.layers = update_source(&back, t_id, LayerSource::Text(new_title.clone())).unwrap();
        assert_ne!(flatten(&back), reference);
        match &back.layer(t_id).unwrap().source {
            Some(LayerSource::Text(t)) => assert_eq!(t.text, "Festa Junina"),
            other => panic!("{other:?}"),
        }
        let mut star2 = match &back.layer(s_id).unwrap().source {
            Some(LayerSource::Shape(s)) => s.clone(),
            _ => panic!(),
        };
        star2.fill = Fill::Solid([0, 0, 0, 255]);
        back.layers = update_source(&back, s_id, LayerSource::Shape(star2)).unwrap();
        assert!(back.layer(s_id).unwrap().source.is_some());
        // Tudo isso também é desfazível como uma ação.
        let mut h = History::new();
        let before = back.layers.clone();
        let after = update_source(
            &back,
            t_id,
            LayerSource::Text(TextSpec {
                text: "Outro".into(),
                ..new_title
            }),
        )
        .unwrap();
        h.execute(
            &mut back,
            Box::new(crate::edit::LayersEdit::new("Editar texto", before, after)),
        );
        h.undo(&mut back);
        match &back.layer(t_id).unwrap().source {
            Some(LayerSource::Text(t)) => assert_eq!(t.text, "Festa Junina"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn missing_fonts_are_listed_and_pixels_survive_reopen() {
        if text::list_fonts().is_empty() {
            return;
        }
        let mut d = doc();
        let spec = TextSpec {
            family: "Fonte Inexistente XYZ".into(),
            ..TextSpec::point("Olá", 5.0, 5.0, [0, 0, 0, 255])
        };
        let id = add(&mut d, LayerSource::Text(spec));
        assert_eq!(missing_fonts(&d), vec!["Fonte Inexistente XYZ".to_string()]);
        let back = orva::read(&orva::write(&d).unwrap()).unwrap();
        assert_eq!(back.layer(id).unwrap().pixels, d.layer(id).unwrap().pixels);
        assert_eq!(missing_fonts(&back).len(), 1);
    }
}
