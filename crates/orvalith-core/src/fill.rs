//! Balde de tinta (preenchimento por tolerância) e gradientes.

use crate::compose::over;
use crate::document::Layer;
use crate::edit::PixelPatch;
use crate::geom::Rect;

fn px(layer: &Layer, x: u32, y: u32) -> [u8; 4] {
    let i = (y as usize * layer.width as usize + x as usize) * 4;
    [
        layer.pixels[i],
        layer.pixels[i + 1],
        layer.pixels[i + 2],
        layer.pixels[i + 3],
    ]
}

/// Dois pixels são "iguais" se todos os canais diferem em no máximo `tol`.
/// Pixels totalmente transparentes são iguais entre si, qualquer que seja o RGB.
fn similar(a: [u8; 4], b: [u8; 4], tol: u8) -> bool {
    if a[3] == 0 && b[3] == 0 {
        return true;
    }
    a.iter().zip(&b).all(|(&p, &q)| p.abs_diff(q) <= tol)
}

/// Preenche a região parecida com o pixel (x, y) com `color` (opacidade `opacity`).
/// `contiguous`: só pixels conectados (4 vizinhos); senão, todos os pixels da camada.
/// Retorna o retalho de histórico, ou `None` se (x, y) está fora ou nada mudou.
#[allow(clippy::too_many_arguments)]
pub fn bucket_fill(
    layer: &mut Layer,
    x: i64,
    y: i64,
    tolerance: u8,
    contiguous: bool,
    color: [u8; 4],
    opacity: f32,
    lock_alpha: bool,
) -> Option<PixelPatch> {
    if x < 0 || y < 0 || x >= i64::from(layer.width) || y >= i64::from(layer.height) {
        return None;
    }
    let (x, y) = (x as u32, y as u32);
    let (w, h) = (layer.width as usize, layer.height as usize);
    let seed = px(layer, x, y);
    let mut mask = vec![false; w * h];
    if contiguous {
        let mut visited = vec![false; w * h];
        let mut stack = vec![(x, y)];
        while let Some((sx, sy)) = stack.pop() {
            if visited[sy as usize * w + sx as usize] {
                continue;
            }
            if !similar(px(layer, sx, sy), seed, tolerance) {
                continue;
            }
            // Expande a linha horizontal e empilha vizinhos acima e abaixo.
            let mut l = sx;
            while l > 0 && similar(px(layer, l - 1, sy), seed, tolerance) {
                l -= 1;
            }
            let mut r = sx;
            while r + 1 < layer.width && similar(px(layer, r + 1, sy), seed, tolerance) {
                r += 1;
            }
            for cx in l..=r {
                let idx = sy as usize * w + cx as usize;
                visited[idx] = true;
                mask[idx] = true;
                if sy > 0 && !visited[(sy as usize - 1) * w + cx as usize] {
                    stack.push((cx, sy - 1));
                }
                if sy + 1 < layer.height && !visited[(sy as usize + 1) * w + cx as usize] {
                    stack.push((cx, sy + 1));
                }
            }
        }
    } else {
        for yy in 0..layer.height {
            for xx in 0..layer.width {
                if similar(px(layer, xx, yy), seed, tolerance) {
                    mask[yy as usize * w + xx as usize] = true;
                }
            }
        }
    }
    let before = layer.pixels.clone();
    let alpha = (f32::from(color[3]) * opacity.clamp(0.0, 1.0))
        .round()
        .clamp(0.0, 255.0) as u8;
    let src = [color[0], color[1], color[2], alpha];
    let mut bounds: Option<Rect> = None;
    let pixels = layer.pixels_mut();
    for yy in 0..h {
        for xx in 0..w {
            if mask[yy * w + xx] {
                let i = (yy * w + xx) * 4;
                let dst = [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]];
                let mut out = over(src, dst, 1.0);
                if lock_alpha {
                    out = if dst[3] == 0 {
                        dst
                    } else {
                        [out[0], out[1], out[2], dst[3]]
                    };
                }
                if out != dst {
                    pixels[i..i + 4].copy_from_slice(&out);
                    let r = Rect::new(xx as u32, yy as u32, 1, 1);
                    bounds = Some(bounds.map_or(r, |b| b.union(r)));
                }
            }
        }
    }
    PixelPatch::from_diff("Balde de tinta", layer, &before, bounds?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientKind {
    Linear,
    Radial,
}

/// Gradiente de `from` para `to`; `to` pode ter alfa 0 (cor → transparente).
#[derive(Debug, Clone, Copy)]
pub struct Gradient {
    pub kind: GradientKind,
    pub start: (f32, f32),
    pub end: (f32, f32),
    pub from: [u8; 4],
    pub to: [u8; 4],
    pub opacity: f32,
}

impl Gradient {
    fn t_at(&self, x: f32, y: f32) -> f32 {
        let (dx, dy) = (self.end.0 - self.start.0, self.end.1 - self.start.1);
        let len2 = dx * dx + dy * dy;
        match self.kind {
            GradientKind::Linear => {
                (((x - self.start.0) * dx + (y - self.start.1) * dy) / len2).clamp(0.0, 1.0)
            }
            GradientKind::Radial => {
                ((x - self.start.0).hypot(y - self.start.1) / len2.sqrt()).clamp(0.0, 1.0)
            }
        }
    }

    /// Cor do gradiente na posição t∈[0,1]. Com destino transparente, o RGB de origem é mantido.
    pub fn color_at(&self, t: f32) -> [u8; 4] {
        let to_rgb = if self.to[3] == 0 { self.from } else { self.to };
        let l = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        [
            l(self.from[0], to_rgb[0]),
            l(self.from[1], to_rgb[1]),
            l(self.from[2], to_rgb[2]),
            l(self.from[3], self.to[3]),
        ]
    }

    /// Desenha o gradiente sobre `original` (pixels da camada antes do gradiente) escrevendo em
    /// `layer`. Retorna `false` (sem alterar nada) se início e fim coincidem.
    pub fn render(&self, layer: &mut Layer, original: &[u8], lock_alpha: bool) -> bool {
        let len2 = (self.end.0 - self.start.0).powi(2) + (self.end.1 - self.start.1).powi(2);
        if len2.is_nan() || len2 <= 1e-6 || original.len() != layer.pixels.len() {
            return false;
        }
        let w = layer.width as usize;
        let height = layer.height as usize;
        let pixels = layer.pixels_mut();
        for y in 0..height {
            for x in 0..w {
                let t = self.t_at(x as f32 + 0.5, y as f32 + 0.5);
                let c = self.color_at(t);
                let i = (y * w + x) * 4;
                let dst = [
                    original[i],
                    original[i + 1],
                    original[i + 2],
                    original[i + 3],
                ];
                let mut out = over(c, dst, self.opacity);
                if lock_alpha {
                    out = if dst[3] == 0 {
                        dst
                    } else {
                        [out[0], out[1], out[2], dst[3]]
                    };
                }
                pixels[i..i + 4].copy_from_slice(&out);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::History;

    fn two_rooms() -> Layer {
        // Duas regiões brancas separadas por uma parede preta na coluna 5.
        let mut l = Layer::filled("l", 10, 6, [255, 255, 255, 255]);
        for y in 0..6 {
            l.set_pixel(5, y, [0, 0, 0, 255]);
        }
        l
    }

    #[test]
    fn contiguous_fill_stops_at_walls() {
        let mut l = two_rooms();
        let p = bucket_fill(&mut l, 1, 1, 0, true, [255, 0, 0, 255], 1.0, false).unwrap();
        assert_eq!(l.pixel(0, 0).unwrap(), [255, 0, 0, 255]);
        assert_eq!(l.pixel(4, 5).unwrap(), [255, 0, 0, 255]);
        assert_eq!(l.pixel(5, 3).unwrap(), [0, 0, 0, 255]);
        assert_eq!(l.pixel(6, 0).unwrap(), [255, 255, 255, 255]);
        assert_eq!(p.rect(), Rect::new(0, 0, 5, 6));
    }

    #[test]
    fn global_fill_reaches_disconnected_regions() {
        let mut l = two_rooms();
        bucket_fill(&mut l, 1, 1, 0, false, [255, 0, 0, 255], 1.0, false).unwrap();
        assert_eq!(l.pixel(9, 5).unwrap(), [255, 0, 0, 255]);
        assert_eq!(l.pixel(5, 0).unwrap(), [0, 0, 0, 255]);
    }

    #[test]
    fn tolerance_includes_similar_colors() {
        let mut l = Layer::filled("l", 4, 1, [100, 100, 100, 255]);
        l.set_pixel(1, 0, [110, 100, 100, 255]);
        l.set_pixel(2, 0, [160, 100, 100, 255]);
        let mut a = l.clone();
        bucket_fill(&mut a, 0, 0, 0, true, [0, 0, 255, 255], 1.0, false).unwrap();
        assert_eq!(a.pixel(1, 0).unwrap(), [110, 100, 100, 255]);
        let mut b = l.clone();
        bucket_fill(&mut b, 0, 0, 20, true, [0, 0, 255, 255], 1.0, false).unwrap();
        assert_eq!(b.pixel(1, 0).unwrap(), [0, 0, 255, 255]);
        assert_eq!(b.pixel(2, 0).unwrap(), [160, 100, 100, 255]);
    }

    #[test]
    fn fills_transparent_area_and_respects_opacity() {
        let mut l = Layer::transparent("l", 3, 3);
        bucket_fill(&mut l, 1, 1, 0, true, [10, 20, 30, 255], 0.5, false).unwrap();
        assert_eq!(l.pixel(2, 2).unwrap(), [10, 20, 30, 128]);
    }

    #[test]
    fn fill_outside_or_noop_gives_none_and_undo_restores() {
        let mut l = two_rooms();
        assert!(bucket_fill(&mut l, -1, 0, 0, true, [1, 2, 3, 255], 1.0, false).is_none());
        assert!(bucket_fill(&mut l, 10, 0, 0, true, [1, 2, 3, 255], 1.0, false).is_none());
        // Preencher com a mesma cor não muda nada.
        assert!(bucket_fill(&mut l, 1, 1, 0, true, [255, 255, 255, 255], 1.0, false).is_none());
        let mut doc = crate::Document::new("t", 10, 6, None).unwrap();
        doc.layers[0] = two_rooms();
        let original = doc.layers[0].pixels.clone();
        let mut layer = doc.layers[0].clone();
        let patch = bucket_fill(&mut layer, 8, 2, 0, true, [0, 255, 0, 255], 1.0, false).unwrap();
        doc.layers[0] = layer;
        let mut h = History::new();
        h.record(&mut doc, Box::new(patch));
        h.undo(&mut doc);
        assert_eq!(doc.layers[0].pixels, original);
    }

    fn grad(kind: GradientKind, to: [u8; 4]) -> Gradient {
        Gradient {
            kind,
            start: (0.0, 5.0),
            end: (10.0, 5.0),
            from: [255, 0, 0, 255],
            to,
            opacity: 1.0,
        }
    }

    #[test]
    fn linear_gradient_endpoints_and_midpoint() {
        let mut l = Layer::filled("l", 10, 10, [0, 0, 0, 255]);
        let orig = l.pixels.clone();
        assert!(grad(GradientKind::Linear, [0, 0, 255, 255]).render(&mut l, &orig, false));
        let first = l.pixel(0, 5).unwrap();
        let last = l.pixel(9, 5).unwrap();
        assert!(first[0] > 240 && first[2] < 15, "{first:?}");
        assert!(last[2] > 225 && last[0] < 30, "{last:?}");
        let mid = l.pixel(5, 5).unwrap();
        assert!(
            (110..=145).contains(&mid[0]) && (110..=145).contains(&mid[2]),
            "{mid:?}"
        );
    }

    #[test]
    fn gradient_to_transparent_keeps_original_at_far_end() {
        let mut l = Layer::filled("l", 10, 10, [0, 255, 0, 255]);
        let orig = l.pixels.clone();
        grad(GradientKind::Linear, [0, 0, 0, 0]).render(&mut l, &orig, false);
        let far = l.pixel(9, 5).unwrap();
        assert!(far[0] < 20 && far[1] > 235 && far[3] == 255, "{far:?}");
        let near = l.pixel(0, 5).unwrap();
        assert!(near[0] > 200 && near[1] < 60, "{near:?}");
    }

    #[test]
    fn radial_gradient_is_symmetric_and_degenerate_is_rejected() {
        let mut l = Layer::transparent("l", 21, 21);
        let orig = l.pixels.clone();
        let g = Gradient {
            kind: GradientKind::Radial,
            start: (10.5, 10.5),
            end: (20.5, 10.5),
            from: [255, 255, 255, 255],
            to: [0, 0, 0, 255],
            opacity: 1.0,
        };
        assert!(g.render(&mut l, &orig, false));
        assert_eq!(l.pixel(10, 5), l.pixel(10, 15));
        assert_eq!(l.pixel(5, 10), l.pixel(15, 10));
        assert!(l.pixel(10, 10).unwrap()[0] > l.pixel(18, 10).unwrap()[0]);
        let mut d = g;
        d.end = d.start;
        let before = l.pixels.clone();
        assert!(!d.render(&mut l, &orig, false));
        assert_eq!(l.pixels, before);
    }
}
