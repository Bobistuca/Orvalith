//! Compositor CPU (referência de comportamento).
//!
//! Percorre a árvore de camadas: camadas raster são mescladas com seu modo, opacidade e
//! preenchimento; grupos "passar através" mesclam os filhos direto no fundo, e grupos com
//! outro modo são compostos isoladamente e depois mesclados como uma única camada.
//! Alfa não pré-multiplicado; fórmulas de mesclagem em sRGB codificado (docs/COMPOSITING.md).

use std::collections::HashSet;
use std::sync::Arc;

use crate::adjust::Prepared;
use crate::blend::{blend_rgb, dissolve_noise, BlendMode};
use crate::document::{Document, Layer, LayerId, LayerKind};
use crate::geom::Rect;
use crate::style;

/// Opções de visualização (não afetam a exportação, que usa `Default`).
#[derive(Default, Clone, Copy)]
pub struct RenderOpts {
    /// Mostrar apenas esta camada (e seus ancestrais/descendentes): isolamento temporário.
    pub isolate: Option<LayerId>,
    /// Mostrar a máscara desta camada em tons de cinza no lugar da imagem.
    pub show_mask: Option<LayerId>,
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
    if let Some(mask) = opts
        .show_mask
        .and_then(|id| doc.layer(id))
        .and_then(|l| l.mask.as_ref())
    {
        let eff = mask.effective(doc.width, doc.height);
        if eff.len() == doc.width as usize * doc.height as usize {
            for y in r.y..r.bottom() {
                for x in r.x..r.right() {
                    let v = eff[y as usize * doc.width as usize + x as usize];
                    let i = ((y - r.y) as usize * r.w as usize + (x - r.x) as usize) * 4;
                    buf[i..i + 4].copy_from_slice(&[v, v, v, 255]);
                }
            }
            return buf;
        }
    }
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

/// Máscara efetiva da camada, se houver, estiver ativa e combinar com o tamanho do documento.
fn effective_mask(doc: &Document, layer: &Layer) -> Option<Arc<Vec<u8>>> {
    let m = layer.mask.as_ref().filter(|m| m.enabled)?;
    let e = m.effective(doc.width, doc.height);
    (e.len() == doc.width as usize * doc.height as usize).then_some(e)
}

fn render_level(
    doc: &Document,
    parent: Option<LayerId>,
    r: Rect,
    buf: &mut [u8],
    show: Option<&HashSet<LayerId>>,
) {
    let kids: Vec<&Layer> = doc.layers.iter().filter(|l| l.parent == parent).collect();
    let mut i = 0;
    while i < kids.len() {
        let base = kids[i];
        i += 1;
        // Camadas recortadas logo acima pertencem à cadeia desta base.
        let mut chain: Vec<&Layer> = Vec::new();
        while i < kids.len() && kids[i].clipped {
            chain.push(kids[i]);
            i += 1;
        }
        if !base.visible || show.is_some_and(|s| !s.contains(&base.id)) {
            continue;
        }
        if base.adjustment.is_some() {
            // Uma camada de ajuste não tem conteúdo próprio: nada a recortar nela.
            render_single(doc, base, r, buf, show);
            for c in chain {
                if c.visible && show.is_none_or(|s| s.contains(&c.id)) {
                    render_single(doc, c, r, buf, show);
                }
            }
        } else if chain.is_empty() {
            render_single(doc, base, r, buf, show);
        } else {
            render_clipped(doc, base, &chain, r, buf, show);
        }
    }
}

fn render_single(
    doc: &Document,
    layer: &Layer,
    r: Rect,
    buf: &mut [u8],
    show: Option<&HashSet<LayerId>>,
) {
    let mask = effective_mask(doc, layer);
    if let Some(adj) = &layer.adjustment {
        apply_adjustment_layer(&adj.prepare(), layer, buf, r, mask.as_deref(), doc.width);
        return;
    }
    match layer.kind {
        LayerKind::Raster if layer.styles.iter().any(|e| e.enabled) => {
            blend_styled(doc, layer, mask.as_deref(), r, buf)
        }
        LayerKind::Raster => {
            blend_raster(layer, r, buf, mask.as_deref().map(Vec::as_slice), doc.width)
        }
        LayerKind::Group => {
            // Grupo com máscara não pode "passar através": é composto de forma isolada.
            let pass = layer.blend == BlendMode::PassThrough && mask.is_none();
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
                    let out = lerp_px(*dst, *src, layer.opacity);
                    dst.copy_from_slice(&out);
                }
            } else {
                let iso = group_buffer(doc, layer, r, show, mask.as_deref().map(Vec::as_slice));
                let mode = if layer.blend == BlendMode::PassThrough {
                    BlendMode::Normal
                } else {
                    layer.blend
                };
                blend_buffer(&iso, buf, r, mode, layer.opacity * layer.fill);
            }
        }
    }
}

/// Conteúdo isolado de um grupo (com a máscara do grupo aplicada ao alfa).
fn group_buffer(
    doc: &Document,
    g: &Layer,
    r: Rect,
    show: Option<&HashSet<LayerId>>,
    mask: Option<&[u8]>,
) -> Vec<u8> {
    let mut iso = vec![0u8; r.w as usize * r.h as usize * 4];
    render_level(doc, Some(g.id), r, &mut iso, show);
    if let Some(m) = mask {
        apply_mask_to_region(&mut iso, r, m, doc.width);
    }
    iso
}

fn apply_mask_to_region(buf: &mut [u8], r: Rect, mask: &[u8], doc_w: u32) {
    let rw = r.w as usize;
    for (i, px) in buf.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (r.x as usize + i % rw, r.y as usize + i / rw);
        let m = mask[y * doc_w as usize + x];
        px[3] = ((u32::from(px[3]) * u32::from(m) + 127) / 255) as u8;
    }
}

/// Base + camadas recortadas: a cadeia é composta sobre a base (o alfa da base manda) e o
/// resultado é mesclado com o modo, opacidade e preenchimento da base.
fn render_clipped(
    doc: &Document,
    base: &Layer,
    chain: &[&Layer],
    r: Rect,
    buf: &mut [u8],
    show: Option<&HashSet<LayerId>>,
) {
    let base_mask = effective_mask(doc, base);
    let mut b = match base.kind {
        LayerKind::Raster => {
            let mut tmp = vec![0u8; r.w as usize * r.h as usize * 4];
            let mut plain = base.clone();
            plain.opacity = 1.0;
            plain.fill = 1.0;
            plain.blend = BlendMode::Normal;
            blend_raster(
                &plain,
                r,
                &mut tmp,
                base_mask.as_deref().map(Vec::as_slice),
                doc.width,
            );
            tmp
        }
        LayerKind::Group => {
            group_buffer(doc, base, r, show, base_mask.as_deref().map(Vec::as_slice))
        }
    };
    let rw = r.w as usize;
    for c in chain {
        if !c.visible || show.is_some_and(|s| !s.contains(&c.id)) {
            continue;
        }
        let cmask = effective_mask(doc, c);
        if let Some(adj) = &c.adjustment {
            // Ajuste recortado: age só sobre a base (o alfa da base não muda).
            apply_adjustment_layer(&adj.prepare(), c, &mut b, r, cmask.as_deref(), doc.width);
            continue;
        }
        // Conteúdo da camada recortada (raster ou grupo isolado) com a sua máscara.
        let src_buf = match c.kind {
            LayerKind::Raster => {
                let mut tmp = vec![0u8; r.w as usize * r.h as usize * 4];
                let mut plain = (*c).clone();
                plain.opacity = 1.0;
                plain.fill = 1.0;
                plain.blend = BlendMode::Normal;
                blend_raster(
                    &plain,
                    r,
                    &mut tmp,
                    cmask.as_deref().map(Vec::as_slice),
                    doc.width,
                );
                tmp
            }
            LayerKind::Group => group_buffer(doc, c, r, show, cmask.as_deref().map(Vec::as_slice)),
        };
        let scale = c.opacity * c.fill;
        let mode = if c.blend == BlendMode::PassThrough {
            BlendMode::Normal
        } else {
            c.blend
        };
        for (i, (dst, src)) in b
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(src_buf.as_chunks::<4>().0.iter())
            .enumerate()
        {
            let ba = dst[3];
            if ba == 0 || src[3] == 0 {
                continue;
            }
            let (x, y) = (r.x + (i % rw) as u32, r.y + (i / rw) as u32);
            let mut out = composite(*dst, *src, mode, scale * f32::from(ba) / 255.0, x, y);
            out[3] = ba; // o alfa da base não muda
            *dst = out;
        }
    }
    let mode = if base.blend == BlendMode::PassThrough {
        BlendMode::Normal
    } else {
        base.blend
    };
    blend_buffer(&b, buf, r, mode, base.opacity * base.fill);
}

/// Camada raster com estilos: conteúdo (máscara e preenchimento aplicados) + efeitos, numa
/// região com margem, mesclados depois com o modo e a opacidade da camada.
fn blend_styled(doc: &Document, layer: &Layer, mask: Option<&Vec<u8>>, r: Rect, buf: &mut [u8]) {
    let m = style::reach(&layer.styles);
    let x0 = r.x.saturating_sub(m);
    let y0 = r.y.saturating_sub(m);
    let x1 = (r.right() + m).min(doc.width.min(layer.width));
    let y1 = (r.bottom() + m).min(doc.height.min(layer.height));
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (ew, eh) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut content = vec![0u8; ew * eh * 4];
    for y in 0..eh {
        for x in 0..ew {
            let (dx, dy) = (x0 as usize + x, y0 as usize + y);
            let si = (dy * layer.width as usize + dx) * 4;
            let mut a = u32::from(layer.pixels[si + 3]);
            if a == 0 {
                continue;
            }
            if let Some(mk) = mask {
                a = (a * u32::from(mk[dy * doc.width as usize + dx]) + 127) / 255;
            }
            a = (a as f32 * layer.fill).round() as u32;
            let di = (y * ew + x) * 4;
            content[di..di + 3].copy_from_slice(&layer.pixels[si..si + 3]);
            content[di + 3] = a.min(255) as u8;
        }
    }
    let bounds = if style::needs_bounds(&layer.styles) {
        let (mut minx, mut miny, mut maxx, mut maxy) = (layer.width, layer.height, 0u32, 0u32);
        for y in 0..layer.height {
            for x in 0..layer.width {
                if layer.pixels[(y as usize * layer.width as usize + x as usize) * 4 + 3] > 0 {
                    minx = minx.min(x);
                    miny = miny.min(y);
                    maxx = maxx.max(x);
                    maxy = maxy.max(y);
                }
            }
        }
        if minx > maxx {
            (0.0, 0.0, layer.width as f32, layer.height as f32)
        } else {
            (
                minx as f32,
                miny as f32,
                (maxx - minx + 1) as f32,
                (maxy - miny + 1) as f32,
            )
        }
    } else {
        (0.0, 0.0, layer.width as f32, layer.height as f32)
    };
    let work = style::Work {
        x: x0,
        y: y0,
        w: ew,
        h: eh,
    };
    let out = style::apply(&layer.styles, &content, work, bounds);
    // Recorta para a região pedida.
    let mut region = vec![0u8; r.w as usize * r.h as usize * 4];
    for y in 0..r.h as usize {
        for x in 0..r.w as usize {
            let (ox, oy) = (r.x as usize + x, r.y as usize + y);
            if ox < x0 as usize || oy < y0 as usize || ox >= x1 as usize || oy >= y1 as usize {
                continue;
            }
            let si = ((oy - y0 as usize) * ew + (ox - x0 as usize)) * 4;
            let di = (y * r.w as usize + x) * 4;
            region[di..di + 4].copy_from_slice(&out[si..si + 4]);
        }
    }
    blend_buffer(&region, buf, r, layer.blend, layer.opacity);
}

/// Aplica uma camada de ajuste ao que já está em `buf` (região `r`): o resultado do ajuste é
/// mesclado com o modo, a opacidade e a máscara da camada; o alfa de `buf` nunca muda.
fn apply_adjustment_layer(
    prep: &Prepared,
    layer: &Layer,
    buf: &mut [u8],
    r: Rect,
    mask: Option<&Vec<u8>>,
    doc_w: u32,
) {
    let scale = layer.opacity * layer.fill;
    if scale <= 0.0 {
        return;
    }
    let mode = if layer.blend == BlendMode::PassThrough {
        BlendMode::Normal
    } else {
        layer.blend
    };
    let rw = r.w as usize;
    for (i, dst) in buf.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        if dst[3] == 0 {
            continue;
        }
        let (x, y) = (r.x + (i % rw) as u32, r.y + (i / rw) as u32);
        let m = mask.map_or(1.0, |m| {
            f32::from(m[y as usize * doc_w as usize + x as usize]) / 255.0
        });
        if m <= 0.0 {
            continue;
        }
        let adjusted = prep.apply([dst[0], dst[1], dst[2]]);
        let src = [adjusted[0], adjusted[1], adjusted[2], 255];
        let out = composite(*dst, src, mode, scale * m, x, y);
        dst[..3].copy_from_slice(&out[..3]);
    }
}

/// Mescla uma camada raster no buffer da região (`mask`: máscara efetiva do tamanho do documento).
fn blend_raster(layer: &Layer, r: Rect, buf: &mut [u8], mask: Option<&[u8]>, doc_w: u32) {
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
            let mut src = [
                layer.pixels[si],
                layer.pixels[si + 1],
                layer.pixels[si + 2],
                layer.pixels[si + 3],
            ];
            if src[3] == 0 {
                continue;
            }
            if let Some(m) = mask {
                let mv = u32::from(m[y as usize * doc_w as usize + x as usize]);
                src[3] = ((u32::from(src[3]) * mv + 127) / 255) as u8;
                if src[3] == 0 {
                    continue;
                }
            }
            let dst = [buf[di], buf[di + 1], buf[di + 2], buf[di + 3]];
            buf[di..di + 4].copy_from_slice(&composite(dst, src, layer.blend, scale, x, y));
        }
    }
}

/// Mescla um buffer de região inteiro (grupo isolado, cadeia recortada) sobre `buf`.
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
        let out = composite(*dst, *src, mode, scale, x, y);
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

    fn clip_doc() -> (Document, LayerId, LayerId) {
        // Fundo cinza; base = quadrado vermelho opaco em (1..3, 1..3); clipada = tela azul cobrindo tudo.
        let mut d = Document::new("t", 6, 6, Some([100, 100, 100, 255])).unwrap();
        let mut base = Layer::transparent("base", 6, 6);
        for y in 1..3 {
            for x in 1..3 {
                base.set_pixel(x, y, [200, 0, 0, 255]);
            }
        }
        let mut clip = Layer::filled("clip", 6, 6, [0, 0, 200, 255]);
        clip.clipped = true;
        let ids = (base.id, clip.id);
        d.layers.extend([base, clip]);
        (d, ids.0, ids.1)
    }

    #[test]
    fn clipped_layer_shows_only_where_base_has_alpha() {
        let (d, _, _) = clip_doc();
        assert_eq!(pixel_at(&d, 1, 1), Some([0, 0, 200, 255]), "dentro da base");
        assert_eq!(
            pixel_at(&d, 4, 4),
            Some([100, 100, 100, 255]),
            "fora da base: só o fundo"
        );
        // Sem recorte a tela azul cobre tudo.
        let mut free = d.clone();
        free.layers[2].clipped = false;
        assert_eq!(pixel_at(&free, 4, 4), Some([0, 0, 200, 255]));
    }

    #[test]
    fn clipping_chain_uses_base_opacity_hides_with_base_and_keeps_alpha() {
        let (mut d, base, _) = clip_doc();
        let mut third = Layer::filled("c2", 6, 6, [0, 200, 0, 255]);
        third.clipped = true;
        third.opacity = 0.5;
        d.layers.push(third);
        // Segunda clipada meio transparente sobre a primeira: mistura de azul e verde.
        let p = pixel_at(&d, 1, 1).unwrap();
        assert!(p[1] > 90 && p[1] < 110 && p[2] > 90 && p[2] < 110, "{p:?}");
        assert_eq!(pixel_at(&d, 4, 4), Some([100, 100, 100, 255]));
        // Opacidade da base vale para a cadeia inteira.
        d.layer_mut(base).unwrap().opacity = 0.5;
        let p = pixel_at(&d, 1, 1).unwrap();
        assert!(
            p[0] > 40 && p[0] < 70,
            "metade do fundo cinza aparece: {p:?}"
        );
        // Ocultar a base oculta a cadeia.
        d.layer_mut(base).unwrap().visible = false;
        assert_eq!(pixel_at(&d, 1, 1), Some([100, 100, 100, 255]));
    }

    #[test]
    fn clipped_layer_keeps_its_own_blend_mode_and_mask() {
        let (mut d, _, clip) = clip_doc();
        d.layer_mut(clip).unwrap().blend = BlendMode::Multiply;
        // Multiplicar azul (0,0,200) por vermelho (200,0,0) dentro da base = preto.
        assert_eq!(pixel_at(&d, 1, 1), Some([0, 0, 0, 255]));
        d.layer_mut(clip).unwrap().blend = BlendMode::Normal;
        // Máscara da camada recortada esconde metade da base.
        let mut m = vec![255u8; 36];
        for y in 0..6 {
            m[y * 6 + 1] = 0;
        }
        d.layer_mut(clip).unwrap().mask = Some(crate::LayerMask::new(m));
        assert_eq!(
            pixel_at(&d, 1, 1),
            Some([200, 0, 0, 255]),
            "máscara revela a base"
        );
        assert_eq!(pixel_at(&d, 2, 1), Some([0, 0, 200, 255]));
    }

    #[test]
    fn clipping_to_a_group_and_with_a_masked_base() {
        let mut d = Document::new("t", 4, 4, Some([50, 50, 50, 255])).unwrap();
        let g = Layer::group("G");
        let gid = g.id;
        d.layers.push(g);
        let mut child = Layer::transparent("c", 4, 4);
        child.set_pixel(1, 1, [255, 255, 255, 255]);
        child.parent = Some(gid);
        d.layers.push(child);
        let mut clip = Layer::filled("clip", 4, 4, [0, 255, 0, 255]);
        clip.clipped = true;
        d.layers.push(clip);
        assert_eq!(pixel_at(&d, 1, 1), Some([0, 255, 0, 255]));
        assert_eq!(pixel_at(&d, 3, 3), Some([50, 50, 50, 255]));
        // Base com máscara escondendo o pixel: a cadeia some junto.
        let mut m = vec![255u8; 16];
        m[4 + 1] = 0;
        d.layer_mut(gid).unwrap().mask = Some(crate::LayerMask::new(m));
        assert_eq!(pixel_at(&d, 1, 1), Some([50, 50, 50, 255]));
    }

    #[test]
    fn mask_isolated_view_shows_grayscale_mask() {
        let mut d = Document::new("t", 2, 1, Some([9, 9, 9, 255])).unwrap();
        d.layers[0].mask = Some(crate::LayerMask::new(vec![40, 220]));
        let id = d.layers[0].id;
        let v = flatten_region_with(
            &d,
            Rect::full(2, 1),
            RenderOpts {
                show_mask: Some(id),
                ..Default::default()
            },
        );
        assert_eq!(v, vec![40, 40, 40, 255, 220, 220, 220, 255]);
        assert_ne!(flatten(&d), v);
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
                ..Default::default()
            },
        );
        assert_eq!(&only[0..4], &[10, 10, 10, 255]);
        let only = flatten_region_with(
            &doc,
            Rect::full(2, 2),
            RenderOpts {
                isolate: Some(id),
                ..Default::default()
            },
        );
        assert_eq!(&only[0..4], &[200, 0, 0, 255]);
    }
    fn styled_doc() -> Document {
        use crate::style::{Effect, EffectKind};
        let mut d = Document::new("s", 48, 36, Some([255, 255, 255, 255])).unwrap();
        let mut l = Layer::transparent("obj", 48, 36);
        for y in 12..24 {
            for x in 14..26 {
                l.set_pixel(x, y, [30, 90, 200, 255]);
            }
        }
        let mut shadow = EffectKind::neutral(0);
        shadow.sanitize();
        let mut stroke = EffectKind::neutral(4);
        stroke.sanitize();
        l.styles = vec![
            Effect::new(shadow),
            Effect::new(stroke),
            Effect::new(EffectKind::neutral(6)),
        ];
        d.layers.push(l);
        d
    }

    #[test]
    fn styled_layer_draws_effects_and_toggling_them_restores_the_plain_result() {
        let mut d = styled_doc();
        let with = flatten(&d);
        // Sombra à direita/abaixo (cinza sobre o fundo branco) e traço vermelho fora da borda.
        let shadow_px = pixel_at(&d, 29, 20).unwrap();
        assert!(shadow_px[0] < 250, "{shadow_px:?}");
        assert_eq!(
            pixel_at(&d, 13, 18),
            Some([255, 0, 0, 255]),
            "traço externo"
        );
        for e in &mut d.layers[1].styles {
            e.enabled = false;
        }
        let plain = flatten(&d);
        assert_ne!(with, plain);
        assert_eq!(pixel_at(&d, 20, 18), Some([30, 90, 200, 255]));
        assert_eq!(pixel_at(&d, 29, 20), Some([255, 255, 255, 255]));
        d.layers[1].styles.clear();
        assert_eq!(flatten(&d), plain);
    }

    #[test]
    fn partial_region_matches_the_full_render_with_styles() {
        let d = styled_doc();
        let full = flatten(&d);
        for r in [
            Rect::new(10, 8, 20, 20),
            Rect::new(0, 0, 48, 36),
            Rect::new(26, 14, 6, 6),
            Rect::new(12, 10, 3, 3),
        ] {
            let part = flatten_region(&d, r);
            for y in 0..r.h as usize {
                for x in 0..r.w as usize {
                    let (fx, fy) = (r.x as usize + x, r.y as usize + y);
                    assert_eq!(
                        part[(y * r.w as usize + x) * 4..(y * r.w as usize + x) * 4 + 4],
                        full[(fy * 48 + fx) * 4..(fy * 48 + fx) * 4 + 4],
                        "região {r:?} em ({x},{y})"
                    );
                }
            }
        }
    }

    #[test]
    fn styles_follow_opacity_mask_and_fill() {
        let mut d = styled_doc();
        d.layers[1].opacity = 0.5;
        let half = pixel_at(&d, 13, 18).unwrap();
        assert!(half[1] > 100 && half[1] < 160, "{half:?}"); // vermelho a 50% sobre branco
        d.layers[1].opacity = 1.0;
        // Máscara que esconde a metade esquerda do objeto: o traço da borda esquerda some.
        let mut m = vec![255u8; 48 * 36];
        for y in 0..36 {
            for x in 0..20 {
                m[y * 48 + x] = 0;
            }
        }
        d.layers[1].mask = Some(crate::LayerMask::new(m));
        assert_eq!(pixel_at(&d, 13, 18), Some([255, 255, 255, 255]));
    }
}
