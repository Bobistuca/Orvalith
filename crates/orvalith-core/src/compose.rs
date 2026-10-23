//! Compositor CPU (referência de comportamento). Normal + opacidade, alfa não pré-multiplicado.

use crate::document::Document;
use crate::geom::Rect;

/// Compõe as camadas visíveis em um buffer RGBA8 (sRGB, alfa não pré-multiplicado).
pub fn flatten(doc: &Document) -> Vec<u8> {
    flatten_region(doc, Rect::full(doc.width, doc.height))
}

/// Compõe apenas `region` (recortada ao documento); o buffer tem `region.w × region.h` pixels.
pub fn flatten_region(doc: &Document, region: Rect) -> Vec<u8> {
    let r = region.clamp_to(doc.width, doc.height);
    let (rw, rh) = (r.w as usize, r.h as usize);
    let mut out = vec![0u8; rw * rh * 4];
    for layer in doc.layers.iter().filter(|l| l.visible) {
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
                let dst = [out[di], out[di + 1], out[di + 2], out[di + 3]];
                out[di..di + 4].copy_from_slice(&over(src, dst, layer.opacity));
            }
        }
    }
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
        let mut top = crate::Layer::filled("topo", 2, 2, [0, 0, 255, 255]);
        top.visible = false;
        doc.layers.push(top);
        assert_eq!(&flatten(&doc)[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn region_matches_full_flatten_and_pixel_at() {
        let mut doc = Document::new("t", 6, 5, Some([10, 20, 30, 255])).unwrap();
        let mut top = crate::Layer::transparent("topo", 6, 5);
        top.set_pixel(3, 2, [200, 0, 0, 128]);
        top.opacity = 0.5;
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
}
