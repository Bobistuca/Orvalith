//! Liquify: deformação por campo de deslocamento (deformar, suavizar, reconstruir).
//!
//! O campo guarda, por pixel, quanto o conteúdo foi empurrado. A imagem original nunca é
//! alterada: o resultado é sempre `render(original, campo)` (mapeamento inverso com
//! interpolação bilinear em alfa pré-multiplicado), então reconstruir volta ao original.

use crate::filter::mix_px;
use crate::retouch::Brush;

pub struct Field {
    w: usize,
    h: usize,
    dx: Vec<f32>,
    dy: Vec<f32>,
}

/// Limite do deslocamento por pixel, em pixels (evita campos absurdos).
const MAX_DISP: f32 = 4096.0;

impl Field {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            dx: vec![0.0; w * h],
            dy: vec![0.0; w * h],
        }
    }

    pub fn is_identity(&self) -> bool {
        self.dx.iter().chain(&self.dy).all(|v| *v == 0.0)
    }

    pub fn clear(&mut self) {
        self.dx.fill(0.0);
        self.dy.fill(0.0);
    }

    pub fn displacement(&self, x: usize, y: usize) -> [f32; 2] {
        let i = y * self.w + x;
        [self.dx[i], self.dy[i]]
    }

    fn bbox(&self, c: [f32; 2], r: f32) -> (usize, usize, usize, usize) {
        let m = r.ceil() + 1.0;
        (
            (c[0] - m).floor().max(0.0) as usize,
            (c[1] - m).floor().max(0.0) as usize,
            ((c[0] + m).ceil().max(0.0) as usize + 1).min(self.w),
            ((c[1] + m).ceil().max(0.0) as usize + 1).min(self.h),
        )
    }

    /// Retângulo (x0, y0, x1, y1) afetado por um toque, para renderização parcial.
    pub fn touched(&self, c: [f32; 2], r: f32) -> (usize, usize, usize, usize) {
        self.bbox(c, r)
    }

    fn cover(&self, brush: Brush, c: [f32; 2], x: usize, y: usize) -> f32 {
        brush.falloff(((x as f32 + 0.5 - c[0]).powi(2) + (y as f32 + 0.5 - c[1]).powi(2)).sqrt())
    }

    /// Empurra o conteúdo na direção `v` (pixels) em volta de `c`.
    pub fn push(&mut self, c: [f32; 2], v: [f32; 2], brush: Brush, strength: f32) {
        let (x0, y0, x1, y1) = self.bbox(c, brush.radius);
        for y in y0..y1 {
            for x in x0..x1 {
                let f = self.cover(brush, c, x, y) * strength;
                if f <= 0.0 {
                    continue;
                }
                let i = y * self.w + x;
                self.dx[i] = (self.dx[i] + v[0] * f).clamp(-MAX_DISP, MAX_DISP);
                self.dy[i] = (self.dy[i] + v[1] * f).clamp(-MAX_DISP, MAX_DISP);
            }
        }
    }

    /// Reconstruir: reduz o deslocamento em direção a zero.
    pub fn reconstruct(&mut self, c: [f32; 2], brush: Brush, strength: f32) {
        let (x0, y0, x1, y1) = self.bbox(c, brush.radius);
        for y in y0..y1 {
            for x in x0..x1 {
                let k = 1.0 - (self.cover(brush, c, x, y) * strength).clamp(0.0, 1.0);
                let i = y * self.w + x;
                self.dx[i] *= k;
                self.dy[i] *= k;
            }
        }
    }

    /// Suavizar: aproxima cada deslocamento da média da vizinhança.
    pub fn smooth(&mut self, c: [f32; 2], brush: Brush, strength: f32) {
        let (x0, y0, x1, y1) = self.bbox(c, brush.radius);
        let (w, h) = (self.w, self.h);
        let avg = |buf: &[f32], x: usize, y: usize| {
            let mut acc = 0.0;
            let mut n = 0.0;
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                    if xx >= 0 && yy >= 0 && xx < w as i32 && yy < h as i32 {
                        acc += buf[yy as usize * w + xx as usize];
                        n += 1.0;
                    }
                }
            }
            acc / n
        };
        let mut nx = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                nx.push((avg(&self.dx, x, y), avg(&self.dy, x, y)));
            }
        }
        let mut k = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let f = (self.cover(brush, c, x, y) * strength).clamp(0.0, 1.0);
                let i = y * w + x;
                self.dx[i] += (nx[k].0 - self.dx[i]) * f;
                self.dy[i] += (nx[k].1 - self.dy[i]) * f;
                k += 1;
            }
        }
    }

    fn sample(&self, src: &[u8], x: f32, y: f32) -> [f32; 4] {
        let x = x.clamp(0.0, (self.w - 1) as f32);
        let y = y.clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let pm = |xx: usize, yy: usize| {
            let i = (yy * self.w + xx) * 4;
            let k = f32::from(src[i + 3]) / 255.0;
            [
                f32::from(src[i]) * k,
                f32::from(src[i + 1]) * k,
                f32::from(src[i + 2]) * k,
                f32::from(src[i + 3]),
            ]
        };
        let (a, b, c, d) = (pm(x0, y0), pm(x1, y0), pm(x0, y1), pm(x1, y1));
        let mut o = [0f32; 4];
        for i in 0..4 {
            o[i] =
                (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy;
        }
        o
    }

    /// Renderiza a região `(x0, y0, x1, y1)` de `src` deformada em `out` (mesmo tamanho de
    /// `src`). `sel` limita o efeito: fora dela vale o original.
    pub fn render_region(
        &self,
        src: &[u8],
        out: &mut [u8],
        region: (usize, usize, usize, usize),
        sel: Option<&[u8]>,
    ) {
        let (x0, y0, x1, y1) = region;
        for y in y0..y1.min(self.h) {
            for x in x0..x1.min(self.w) {
                let i = y * self.w + x;
                let o = [src[i * 4], src[i * 4 + 1], src[i * 4 + 2], src[i * 4 + 3]];
                let wt = sel.map_or(1.0, |m| f32::from(m.get(i).copied().unwrap_or(0)) / 255.0);
                let (dx, dy) = (self.dx[i], self.dy[i]);
                let p = if wt <= 0.0 || (dx == 0.0 && dy == 0.0) {
                    o
                } else {
                    let s = self.sample(src, x as f32 - dx, y as f32 - dy);
                    let warped = if s[3] < 0.5 {
                        [0, 0, 0, 0]
                    } else {
                        let k = 255.0 / s[3];
                        [
                            (s[0] * k + 0.5).clamp(0.0, 255.0) as u8,
                            (s[1] * k + 0.5).clamp(0.0, 255.0) as u8,
                            (s[2] * k + 0.5).clamp(0.0, 255.0) as u8,
                            (s[3] + 0.5).clamp(0.0, 255.0) as u8,
                        ]
                    };
                    if wt >= 1.0 {
                        warped
                    } else {
                        mix_px(o, warped, wt)
                    }
                };
                out[i * 4..i * 4 + 4].copy_from_slice(&p);
            }
        }
    }

    pub fn render(&self, src: &[u8], sel: Option<&[u8]>) -> Vec<u8> {
        let mut out = src.to_vec();
        self.render_region(src, &mut out, (0, 0, self.w, self.h), sel);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BR: Brush = Brush {
        radius: 8.0,
        hardness: 1.0,
    };

    fn line_image(w: usize, h: usize, col: usize) -> Vec<u8> {
        let mut v = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let c = if x == col {
                    [255, 255, 255, 255]
                } else {
                    [0, 0, 0, 255]
                };
                v[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&c);
            }
        }
        v
    }

    fn col_of_white(img: &[u8], w: usize, y: usize) -> usize {
        (0..w).max_by_key(|x| img[(y * w + x) * 4]).unwrap()
    }

    #[test]
    fn identity_field_renders_the_source_exactly() {
        let img = line_image(20, 10, 7);
        let f = Field::new(20, 10);
        assert!(f.is_identity());
        assert_eq!(f.render(&img, None), img);
    }

    #[test]
    fn push_moves_content_in_the_drag_direction_and_leaves_far_areas_alone() {
        let (w, h) = (40, 20);
        let img = line_image(w, h, 14);
        let mut f = Field::new(w, h);
        f.push([14.0, 10.0], [4.0, 0.0], BR, 1.0);
        let out = f.render(&img, None);
        assert_eq!(
            col_of_white(&out, w, 10),
            18,
            "linha deslocada 4 px para a direita"
        );
        assert_eq!(col_of_white(&out, w, 0), 14, "fora do pincel não muda");
        assert!(!f.is_identity());
    }

    #[test]
    fn reconstruct_restores_the_original_and_never_touches_the_source() {
        let (w, h) = (40, 20);
        let img = line_image(w, h, 14);
        let backup = img.clone();
        let mut f = Field::new(w, h);
        f.push([14.0, 10.0], [4.0, 0.0], BR, 1.0);
        f.reconstruct(
            [14.0, 10.0],
            Brush {
                radius: 12.0,
                hardness: 1.0,
            },
            1.0,
        );
        assert!(f.is_identity());
        assert_eq!(f.render(&img, None), backup);
        // Reconstrução parcial reduz o deslocamento.
        f.push([14.0, 10.0], [6.0, 0.0], BR, 1.0);
        f.reconstruct([14.0, 10.0], BR, 0.5);
        assert!((f.displacement(14, 10)[0] - 3.0).abs() < 0.2);
    }

    #[test]
    fn smooth_reduces_roughness_of_the_field() {
        let mut f = Field::new(30, 30);
        f.push(
            [15.0, 15.0],
            [6.0, 0.0],
            Brush {
                radius: 3.0,
                hardness: 1.0,
            },
            1.0,
        );
        let before = f.displacement(15, 15)[0];
        let neighbour_before = f.displacement(19, 15)[0];
        f.smooth(
            [15.0, 15.0],
            Brush {
                radius: 10.0,
                hardness: 1.0,
            },
            1.0,
        );
        let after = f.displacement(15, 15)[0];
        assert!(after < before, "{after} < {before}");
        assert!(
            f.displacement(19, 15)[0] > neighbour_before,
            "espalha para os vizinhos"
        );
    }

    #[test]
    fn selection_limits_the_warp_and_partial_region_matches_full_render() {
        let (w, h) = (40, 20);
        let img = line_image(w, h, 14);
        let mut f = Field::new(w, h);
        f.push([14.0, 10.0], [4.0, 0.0], BR, 1.0);
        let none = vec![0u8; w * h];
        assert_eq!(f.render(&img, Some(&none)), img);
        let full = f.render(&img, None);
        let mut part = img.clone();
        f.render_region(&img, &mut part, f.touched([14.0, 10.0], 8.0), None);
        assert_eq!(part, full);
    }

    #[test]
    fn warping_over_transparency_makes_no_halos() {
        let (w, h) = (30, 10);
        let mut img = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 10..14 {
                img[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[255, 0, 0, 255]);
            }
        }
        let mut f = Field::new(w, h);
        f.push(
            [12.0, 5.0],
            [1.5, 0.0],
            Brush {
                radius: 6.0,
                hardness: 0.3,
            },
            1.0,
        );
        let out = f.render(&img, None);
        for p in out.as_chunks::<4>().0 {
            if p[3] > 8 {
                assert!(p[0] >= 250 && p[1] <= 4 && p[2] <= 4, "{p:?}");
            }
        }
        assert!(out
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0 && p[3] < 255));
    }
}
