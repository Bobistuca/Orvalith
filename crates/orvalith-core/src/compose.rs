//! Compositor CPU (referência de comportamento).
//!
//! Percorre a árvore de camadas: camadas raster são mescladas com seu modo, opacidade e
//! preenchimento; grupos "passar através" mesclam os filhos direto no fundo, e grupos com
//! outro modo são compostos isoladamente e depois mesclados como uma única camada.
//! Alfa não pré-multiplicado; fórmulas de mesclagem em sRGB codificado (docs/COMPOSITING.md).

use std::collections::HashSet;

use crate::blend::{blend_rgb, dissolve_noise, BlendMode};
use crate::document::{Document, Layer, LayerId, LayerKind};
use crate::geom::Rect;

/// Opções de visualização (não afetam a exportação, que usa `Default`).
#[derive(Default, Clone, Copy)]
pub struct RenderOpts {
    /// Mostrar apenas esta camada (e seus ancestrais/descendentes): isolamento temporário.
    pub isolate: Option<LayerId>,
}

/// Compõe as camadas visíveis em um buffer RGBA8 (sRGB, alfa não pré-multiplicado).
pub fn flatten(doc: &Document) -> Vec<u8> {
    flatten_region(doc, Rect::full(doc.width, doc.height))
}

pub fn flatten_region(doc: &Document, region: Rect) -> Vec<u8> {
    flatten_region_with(doc, region, RenderOpts::default())
}

/// Compõe apenas `region` (recortada ao documento); o buffer tem `region.w × region.h` pixels.
pub fn flatten_region_with(doc: &Document, region: Rect, opts: RenderOpts) -> Vec<u8> {
    let r = region.clamp_to(doc.width, doc.height);
    let mut buf = vec![0u8; r.w as usize * r.h as usize * 4];
    let show = opts.isolate.map(|id| isolation_set(doc, id));
    render_level(doc, None, r, &mut buf, show.as_ref());
    buf
}

fn isolation_set(doc: &Document, id: LayerId) -> HashSet<LayerId> {
    let mut set: HashSet<LayerId> = doc.descendants(id).into_iter().collect();
    set.insert(id);
    let mut cur = doc.layer(id).and_then(|l| l.parent);
    while let Some(p) = cur {
        if !set.insert(p) {
            break;
        }
        cur = doc.layer(p).and_then(|l| l.parent);
    }
    set
}

fn render_level(
    doc: &Document,
    parent: Option<LayerId>,
    r: Rect,
    buf: &mut [u8],
    show: Option<&HashSet<LayerId>>,
) {
    for layer in doc
        .layers
        .iter()
        .filter(|l| l.parent == parent && l.visible)
    {
        if show.is_some_and(|s| !s.contains(&layer.id)) {
            continue;
        }
        match layer.kind {
            LayerKind::Raster => blend_raster(layer, r, buf),
            LayerKind::Group => {
                let pass = layer.blend == BlendMode::PassThrough;
                if pass && layer.opacity >= 0.999 {
                    render_level(doc, Some(layer.id), r, buf, show);
                } else if pass {
                    let mut copy = buf.to_vec();
                    render_level(doc, Some(layer.id), r, &mut copy, show);
                    for (dst, src) in buf
                        .as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .zip(copy.as_chunks::<4>().0.iter())
                    {
                        let out = lerp_px(
                            [dst[0], dst[1], dst[2], dst[3]],
                            [src[0], src[1], src[2], src[3]],
                            layer.opacity,
                        );
                        dst.copy_from_slice(&out);
                    }
                } else {
                    let mut iso = vec![0u8; buf.len()];
                    render_level(doc, Some(layer.id), r, &mut iso, show);
                    blend_buffer(&iso, buf, r, layer.blend, layer.opacity * layer.fill);
                }
            }
        }
    }
}

/// Mescla uma camada raster no buffer da região.
fn blend_raster(layer: &Layer, r: Rect, buf: &mut [u8]) {
    let scale = layer.opacity * layer.fill;
    if scale <= 0.0 {
        return;
    }
    let rw = r.w as usize;
    let x_end = r.right().min(layer.width);
    let y_end = r.bottom().min(layer.height);
    for y in r.y..y_end {
        for x in r.x..x_end {
            let si = (y as usize * layer.width as usize + x as usize) * 4;
            let di = ((y - r.y) as usize * rw + (x - r.x) as usize) * 4;
            let src = [
                layer.pixels[si],
                layer.pixels[si + 1],
                layer.pixels[si + 2],
                layer.pixels[si + 3],
            ];
            if src[3] == 0 {
                continue;
            }
            let dst = [buf[di], buf[di + 1], buf[di + 2], buf[di + 3]];
            buf[di..di + 4].copy_from_slice(&composite(dst, src, layer.blend, scale, x, y));
        }
    }
}

/// Mescla um buffer de região inteiro (grupo isolado) sobre `buf`.
fn blend_buffer(src_buf: &[u8], buf: &mut [u8], r: Rect, mode: BlendMode, scale: f32) {
    let rw = r.w as usize;
    for (i, (dst, src)) in buf
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(src_buf.as_chunks::<4>().0.iter())
        .enumerate()
    {
        if src[3] == 0 {
            continue;
        }
        let x = r.x + (i % rw) as u32;
        let y = r.y + (i / rw) as u32;
        let out = composite(
            [dst[0], dst[1], dst[2], dst[3]],
            [src[0], src[1], src[2], src[3]],
            mode,
            scale,
            x,
            y,
        );
        dst.copy_from_slice(&out);
    }
}

/// Interpolação entre dois pixels em alfa pré-multiplicado.
pub fn lerp_px(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let (aa, ba) = (f32::from(a[3]) / 255.0, f32::from(b[3]) / 255.0);
    let oa = aa + (ba - aa) * t;
    if oa <= 0.0 {
        return [0, 0, 0, 0];
    }
    let mut out = [0u8; 4];
    for c in 0..3 {
        let pa = f32::from(a[c]) * aa;
        let pb = f32::from(b[c]) * ba;
        out[c] = ((pa + (pb - pa) * t) / oa).round().clamp(0.0, 255.0) as u8;
    }
    out[3] = (oa * 255.0).round() as u8;
    out
}

/// Composição de um pixel de origem sobre o destino com modo de mesclagem.
/// `scale` = opacidade × preenchimento da camada.
pub fn composite(
    dst: [u8; 4],
    src: [u8; 4],
    mode: BlendMode,
    scale: f32,
    x: u32,
    y: u32,
) -> [u8; 4] {
    let scale = scale.clamp(0.0, 1.0);
    let sa = f32::from(src[3]) / 255.0 * scale;
    if sa <= 0.0 {
        return dst;
    }
    match mode {
        BlendMode::Normal | BlendMode::PassThrough => return over(src, dst, scale),
        BlendMode::Dissolve => {
            // Alfa vira probabilidade: cada pixel é todo da origem ou nada dela.
            return if dissolve_noise(x, y) < sa {
                over([src[0], src[1], src[2], 255], dst, 1.0)
            } else {
                dst
            };
        }
        _ => {}
    }
    let ba = f32::from(dst[3]) / 255.0;
    if ba <= 0.0 {
        return [src[0], src[1], src[2], (sa * 255.0).round() as u8];
    }
    let cb = [
        f32::from(dst[0]) / 255.0,
        f32::from(dst[1]) / 255.0,
        f32::from(dst[2]) / 255.0,
    ];
    let cs = [
        f32::from(src[0]) / 255.0,
        f32::from(src[1]) / 255.0,
        f32::from(src[2]) / 255.0,
    ];
    let b = blend_rgb(mode, cb, cs);
    let ra = sa + ba * (1.0 - sa);
    let mut out = [0u8; 4];
    for c in 0..3 {
        let v = ((1.0 - sa) * ba * cb[c] + sa * (1.0 - ba) * cs[c] + sa * ba * b[c]) / ra;
        out[c] = (v * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    out[3] = (ra * 255.0).round().clamp(0.0, 255.0) as u8;
    out
}

/// Cor composta de um único pixel (todas as camadas visíveis).
pub fn pixel_at(doc: &Document, x: u32, y: u32) -> Option<[u8; 4]> {
    if x >= doc.width || y >= doc.height {
        return None;
    }
    let px = flatten_region(doc, Rect::new(x, y, 1, 1));
    Some([px[0], px[1], px[2], px[3]])
}

/// Operador "source over" com opacidade da camada, em alfa não pré-multiplicado.
pub fn over(src: [u8; 4], dst: [u8; 4], opacity: f32) -> [u8; 4] {
    let sa = f32::from(src[3]) / 255.0 * opacity.clamp(0.0, 1.0);
    let da = f32::from(dst[3]) / 255.0;
    let oa = sa + da * (1.0 - sa);
    if oa <= 0.0 {
        return [0, 0, 0, 0];
    }
    let mut out = [0u8; 4];
    for c in 0..3 {
        let s = f32::from(src[c]);
        let d = f32::from(dst[c]);
        let v = (s * sa + d * da * (1.0 - sa)) / oa;
        out[c] = v.round().clamp(0.0, 255.0) as u8;
    }
    out[3] = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_doc() -> Document {
        Document::new("t", 4, 4, Some([128, 128, 128, 255])).unwrap()
    }

    #[test]
    fn opaque_over_replaces() {
        assert_eq!(
            over([10, 20, 30, 255], [200, 200, 200, 255], 1.0),
            [10, 20, 30, 255]
        );
    }

    #[test]
    fn transparent_over_keeps_destination() {
        assert_eq!(
            over([10, 20, 30, 0], [200, 100, 50, 255], 1.0),
            [200, 100, 50, 255]
        );
    }

    #[test]
    fn half_opacity_blends_evenly() {
        let r = over([255, 255, 255, 255], [0, 0, 0, 255], 0.5);
        assert_eq!(r, [128, 128, 128, 255]);
    }

    #[test]
    fn over_empty_is_transparent() {
        assert_eq!(over([9, 9, 9, 0], [7, 7, 7, 0], 1.0), [0, 0, 0, 0]);
    }

    #[test]
    fn flatten_skips_hidden_layers() {
        let mut doc = Document::new("t", 2, 2, Some([255, 0, 0, 255])).unwrap();
        let mut top = Layer::filled("topo", 2, 2, [0, 0, 255, 255]);
        top.visible = false;
        doc.layers.push(top);
        assert_eq!(&flatten(&doc)[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn region_matches_full_flatten_and_pixel_at() {
        let mut doc = Document::new("t", 6, 5, Some([10, 20, 30, 255])).unwrap();
        let mut top = Layer::transparent("topo", 6, 5);
        top.set_pixel(3, 2, [200, 0, 0, 128]);
        top.opacity = 0.5;
        top.blend = BlendMode::Multiply;
        doc.layers.push(top);
        let full = flatten(&doc);
        let reg = flatten_region(&doc, Rect::new(2, 1, 3, 3));
        for y in 0..3usize {
            for x in 0..3usize {
                let fi = ((y + 1) * 6 + (x + 2)) * 4;
                assert_eq!(
                    &reg[(y * 3 + x) * 4..(y * 3 + x) * 4 + 4],
                    &full[fi..fi + 4]
                );
            }
        }
        assert_eq!(pixel_at(&doc, 3, 2).unwrap(), full[(2 * 6 + 3) * 4..][..4]);
        assert_eq!(pixel_at(&doc, 6, 0), None);
    }

    #[test]
    fn multiply_layer_darkens_backdrop() {
        let mut doc = gray_doc();
        let mut top = Layer::filled("m", 4, 4, [128, 128, 128, 255]);
        top.blend = BlendMode::Multiply;
        doc.layers.push(top);
        let px = pixel_at(&doc, 1, 1).unwrap();
        // 0.502 * 0.502 * 255 ≈ 64
        assert_eq!(px, [64, 64, 64, 255]);
    }

    #[test]
    fn every_mode_ignores_transparent_source_and_keeps_opaque_result() {
        for mode in BlendMode::LAYER_MODES {
            let dst = [90, 140, 200, 255];
            assert_eq!(
                composite(dst, [10, 20, 30, 0], mode, 1.0, 0, 0),
                dst,
                "{mode:?}"
            );
            let out = composite(dst, [10, 220, 30, 255], mode, 1.0, 5, 7);
            assert_eq!(out[3], 255, "{mode:?}");
        }
    }

    #[test]
    fn blend_over_transparent_backdrop_shows_source() {
        for mode in BlendMode::LAYER_MODES {
            if mode == BlendMode::Dissolve {
                continue;
            }
            let out = composite([0, 0, 0, 0], [10, 220, 30, 255], mode, 1.0, 0, 0);
            assert_eq!(out, [10, 220, 30, 255], "{mode:?}");
        }
    }

    #[test]
    fn dissolve_is_binary_and_proportional_to_alpha() {
        let dst = [0, 0, 0, 255];
        let src = [255, 255, 255, 128];
        let mut lit = 0;
        for y in 0..60 {
            for x in 0..60 {
                let o = composite(dst, src, BlendMode::Dissolve, 1.0, x, y);
                assert!(o == dst || o == [255, 255, 255, 255]);
                if o != dst {
                    lit += 1;
                }
            }
        }
        assert!((1500..2100).contains(&lit), "{lit} de 3600");
    }

    #[test]
    fn fill_scales_layer_alpha() {
        let mut doc = Document::new("t", 2, 2, Some([0, 0, 0, 255])).unwrap();
        let mut top = Layer::filled("f", 2, 2, [255, 255, 255, 255]);
        top.fill = 0.5;
        doc.layers.push(top);
        assert_eq!(pixel_at(&doc, 0, 0).unwrap(), [128, 128, 128, 255]);
    }

    fn group_with_multiply(blend: BlendMode, opacity: f32) -> Document {
        let mut doc = gray_doc();
        let mut g = Layer::group("G");
        g.blend = blend;
        g.opacity = opacity;
        let gid = g.id;
        doc.layers.push(g);
        let mut child = Layer::filled("m", 4, 4, [128, 128, 128, 255]);
        child.blend = BlendMode::Multiply;
        child.parent = Some(gid);
        doc.layers.push(child);
        doc
    }

    #[test]
    fn pass_through_group_blends_with_backdrop_but_normal_group_isolates() {
        let pass = group_with_multiply(BlendMode::PassThrough, 1.0);
        assert_eq!(pixel_at(&pass, 0, 0).unwrap(), [64, 64, 64, 255]);
        // Isolado: o Multiplicar atua sobre transparência, e o grupo (Normal) cobre o fundo.
        let iso = group_with_multiply(BlendMode::Normal, 1.0);
        assert_eq!(pixel_at(&iso, 0, 0).unwrap(), [128, 128, 128, 255]);
    }

    #[test]
    fn group_opacity_and_visibility() {
        let half = group_with_multiply(BlendMode::PassThrough, 0.5);
        let px = pixel_at(&half, 0, 0).unwrap();
        assert!((95..=97).contains(&px[0]), "{px:?}");
        let mut hidden = group_with_multiply(BlendMode::PassThrough, 1.0);
        let gi = hidden.layers.iter().position(|l| l.is_group()).unwrap();
        hidden.layers[gi].visible = false;
        assert_eq!(pixel_at(&hidden, 0, 0).unwrap(), [128, 128, 128, 255]);
        let mut iso_half = group_with_multiply(BlendMode::Normal, 0.5);
        iso_half.layers[1].opacity = 1.0;
        let p = pixel_at(&iso_half, 0, 0).unwrap();
        assert_eq!(p[3], 255);
    }

    #[test]
    fn nested_groups_render_inner_first() {
        let mut doc = Document::new("t", 2, 2, Some([0, 0, 0, 255])).unwrap();
        let outer = Layer::group("outer");
        let mut inner = Layer::group("inner");
        inner.parent = Some(outer.id);
        let mut leaf = Layer::filled("leaf", 2, 2, [200, 0, 0, 255]);
        leaf.parent = Some(inner.id);
        doc.layers.extend([outer, inner, leaf]);
        assert_eq!(pixel_at(&doc, 0, 0).unwrap(), [200, 0, 0, 255]);
    }

    #[test]
    fn isolation_shows_only_chosen_layer_chain() {
        let mut doc = Document::new("t", 2, 2, Some([10, 10, 10, 255])).unwrap();
        let top = Layer::filled("top", 2, 2, [200, 0, 0, 255]);
        let id = top.id;
        doc.layers.push(top);
        let only = flatten_region_with(
            &doc,
            Rect::full(2, 2),
            RenderOpts {
                isolate: Some(doc.layers[0].id),
            },
        );
        assert_eq!(&only[0..4], &[10, 10, 10, 255]);
        let only = flatten_region_with(&doc, Rect::full(2, 2), RenderOpts { isolate: Some(id) });
        assert_eq!(&only[0..4], &[200, 0, 0, 255]);
    }
}
