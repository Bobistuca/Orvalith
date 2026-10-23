//! Seleções: máscaras de 8 bits (255 = totalmente selecionado) do tamanho do documento.
//!
//! Formas são recortadas ao canvas (seleções que passam das bordas são válidas). Todas as
//! funções são puras; a aplicação ao documento passa por `edit::SelectionEdit` (histórico).
//! Quando não há seleção o documento guarda `None` e as operações valem para a camada inteira.

use std::collections::VecDeque;

use crate::compose::{lerp_px, over};
use crate::document::Layer;
use crate::edit::PixelPatch;
use crate::fill::flood_mask;
use crate::geom::Rect;

/// Como uma nova forma se combina com a seleção existente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combine {
    Replace,
    Add,
    Subtract,
    Intersect,
}

/// `None` quando a máscara resultante é vazia (nada selecionado).
pub fn nonempty(mask: Vec<u8>) -> Option<Vec<u8>> {
    mask.iter().any(|&v| v > 0).then_some(mask)
}

/// Combina a seleção atual com uma forma nova. Adicionar = máximo, interseção = mínimo,
/// subtrair = `atual × (1 − nova)`.
pub fn combine(current: Option<&[u8]>, new: &[u8], mode: Combine) -> Option<Vec<u8>> {
    match (current, mode) {
        (_, Combine::Replace) | (None, Combine::Add) => nonempty(new.to_vec()),
        (None, _) => None,
        (Some(a), Combine::Add) => nonempty(a.iter().zip(new).map(|(&a, &b)| a.max(b)).collect()),
        (Some(a), Combine::Intersect) => {
            nonempty(a.iter().zip(new).map(|(&a, &b)| a.min(b)).collect())
        }
        (Some(a), Combine::Subtract) => nonempty(
            a.iter()
                .zip(new)
                .map(|(&a, &b)| ((u32::from(a) * (255 - u32::from(b)) + 127) / 255) as u8)
                .collect(),
        ),
    }
}

pub fn select_all(w: u32, h: u32) -> Vec<u8> {
    vec![255; w as usize * h as usize]
}

pub fn invert(mask: &[u8]) -> Option<Vec<u8>> {
    nonempty(mask.iter().map(|&v| 255 - v).collect())
}

/// Retângulo envolvente dos pixels com valor > 0.
pub fn bounds(mask: &[u8], w: u32, h: u32) -> Option<Rect> {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for y in 0..h {
        let row = &mask[y as usize * w as usize..(y as usize + 1) * w as usize];
        if let Some(first) = row.iter().position(|&v| v > 0) {
            let last = row.iter().rposition(|&v| v > 0).unwrap_or(first);
            x0 = x0.min(first as u32);
            x1 = x1.max(last as u32 + 1);
            y0 = y0.min(y);
            y1 = y1.max(y + 1);
        }
    }
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

fn overlap(p: f32, a: f32, b: f32) -> f32 {
    ((p + 1.0).min(b) - p.max(a)).clamp(0.0, 1.0)
}

/// Retângulo (coordenadas contínuas do documento). Com `aa`, bordas fracionárias dão cobertura parcial.
pub fn rect_mask(w: u32, h: u32, p0: (f32, f32), p1: (f32, f32), aa: bool) -> Vec<u8> {
    let (mut x0, mut x1) = (p0.0.min(p1.0), p0.0.max(p1.0));
    let (mut y0, mut y1) = (p0.1.min(p1.1), p0.1.max(p1.1));
    if !aa {
        x0 = x0.round();
        x1 = x1.round();
        y0 = y0.round();
        y1 = y1.round();
    }
    let mut out = vec![0u8; w as usize * h as usize];
    let xs = x0.floor().max(0.0) as u32;
    let xe = (x1.ceil().max(0.0) as u32).min(w);
    let ys = y0.floor().max(0.0) as u32;
    let ye = (y1.ceil().max(0.0) as u32).min(h);
    for y in ys..ye {
        let cy = overlap(y as f32, y0, y1);
        for x in xs..xe {
            let c = overlap(x as f32, x0, x1) * cy;
            out[y as usize * w as usize + x as usize] = (c * 255.0).round() as u8;
        }
    }
    out
}

/// Elipse inscrita no retângulo definido por `p0`–`p1`.
pub fn ellipse_mask(w: u32, h: u32, p0: (f32, f32), p1: (f32, f32), aa: bool) -> Vec<u8> {
    let (x0, x1) = (p0.0.min(p1.0), p0.0.max(p1.0));
    let (y0, y1) = (p0.1.min(p1.1), p0.1.max(p1.1));
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let mut out = vec![0u8; w as usize * h as usize];
    if rx <= 0.0 || ry <= 0.0 {
        return out;
    }
    let margin = 1.5 / rx.min(ry).max(1.0);
    let xs = x0.floor().max(0.0) as u32;
    let xe = (x1.ceil().max(0.0) as u32).min(w);
    let ys = y0.floor().max(0.0) as u32;
    let ye = (y1.ceil().max(0.0) as u32).min(h);
    let inside = |x: f32, y: f32| ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0;
    for y in ys..ye {
        for x in xs..xe {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let nd = (((px - cx) / rx).powi(2) + ((py - cy) / ry).powi(2)).sqrt();
            let v = if !aa {
                if inside(px, py) {
                    255
                } else {
                    0
                }
            } else if nd < 1.0 - margin {
                255
            } else if nd > 1.0 + margin {
                0
            } else {
                let mut n = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        if inside(
                            x as f32 + (sx as f32 + 0.5) / 4.0,
                            y as f32 + (sy as f32 + 0.5) / 4.0,
                        ) {
                            n += 1;
                        }
                    }
                }
                (n * 255 + 8) / 16
            };
            out[y as usize * w as usize + x as usize] = v as u8;
        }
    }
    out
}

/// Polígono preenchido (regra par-ímpar). Menos de 3 pontos resulta em máscara vazia.
pub fn polygon_mask(w: u32, h: u32, pts: &[(f32, f32)], aa: bool) -> Vec<u8> {
    let mut out = vec![0u8; w as usize * h as usize];
    if pts.len() < 3 {
        return out;
    }
    let ymin = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
    let ymax = pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
    let ys = ymin.floor().max(0.0) as u32;
    let ye = (ymax.ceil().max(0.0) as u32).min(h);
    let sub = if aa { 4 } else { 1 };
    let mut acc = vec![0f32; w as usize];
    let mut xs: Vec<f32> = Vec::new();
    for y in ys..ye {
        acc.iter_mut().for_each(|v| *v = 0.0);
        for s in 0..sub {
            let sy = y as f32 + (s as f32 + 0.5) / sub as f32;
            xs.clear();
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                if a.1 == b.1 {
                    continue;
                }
                let (lo, hi) = if a.1 < b.1 { (a, b) } else { (b, a) };
                if sy >= lo.1 && sy < hi.1 {
                    xs.push(lo.0 + (sy - lo.1) * (hi.0 - lo.0) / (hi.1 - lo.1));
                }
            }
            xs.sort_by(f32::total_cmp);
            for pair in xs.as_chunks::<2>().0 {
                let (xa, xb) = (pair[0].max(0.0), pair[1].min(w as f32));
                if xb <= xa {
                    continue;
                }
                if aa {
                    for px in xa.floor() as u32..(xb.ceil() as u32).min(w) {
                        acc[px as usize] += overlap(px as f32, xa, xb) / sub as f32;
                    }
                } else {
                    for px in 0..w {
                        let c = px as f32 + 0.5;
                        if c >= xa && c < xb {
                            acc[px as usize] += 1.0;
                        }
                    }
                }
            }
        }
        for x in 0..w as usize {
            out[y as usize * w as usize + x] = (acc[x].min(1.0) * 255.0).round() as u8;
        }
    }
    out
}

/// Varinha mágica: região parecida com o pixel (x, y) dos `pixels` RGBA.
pub fn magic_wand(
    pixels: &[u8],
    w: u32,
    h: u32,
    x: u32,
    y: u32,
    tolerance: u8,
    contiguous: bool,
) -> Vec<u8> {
    flood_mask(pixels, w, h, x, y, tolerance, contiguous)
        .into_iter()
        .map(|b| if b { 255 } else { 0 })
        .collect()
}

/// Faixa de cores: seleção suave de tudo que se aproxima de `target` (RGB), com `fuzziness`
/// (0..=255) como distância máxima por canal. Pixels transparentes não entram.
pub fn color_range(pixels: &[u8], target: [u8; 3], fuzziness: u8) -> Vec<u8> {
    let f = f32::from(fuzziness.max(1));
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let d = (0..3)
                .map(|c| p[c].abs_diff(target[c]))
                .max()
                .unwrap_or(255);
            let closeness = (1.0 - f32::from(d) / f).clamp(0.0, 1.0);
            let exact = fuzziness == 0 && d == 0;
            let v = if exact { 1.0 } else { closeness };
            (v * f32::from(p[3])).round() as u8
        })
        .collect()
}

/// Extremo (máximo/mínimo) em janela deslizante de meia-largura `half`, com janela recortada
/// nas bordas (a borda do canvas não "encolhe" nem "expande" a seleção).
fn sliding(src: &[u8], out: &mut [u8], half: usize, max: bool) {
    let n = src.len();
    let mut dq: VecDeque<usize> = VecDeque::new();
    let better = |a: u8, b: u8| if max { a >= b } else { a <= b };
    let mut next = 0;
    for i in 0..n {
        let hi = (i + half).min(n - 1);
        while next <= hi {
            while dq.back().is_some_and(|&b| better(src[next], src[b])) {
                dq.pop_back();
            }
            dq.push_back(next);
            next += 1;
        }
        let lo = i.saturating_sub(half);
        while dq.front().is_some_and(|&f| f < lo) {
            dq.pop_front();
        }
        out[i] = src[*dq.front().unwrap_or(&i)];
    }
}

/// Dilatação (`max`) ou erosão com disco de raio `r` pixels.
fn morph(mask: &[u8], w: u32, h: u32, r: u32, max: bool) -> Vec<u8> {
    let (w, h, r) = (w as usize, h as usize, r.min(100) as i64);
    if r == 0 {
        return mask.to_vec();
    }
    let mut out = mask.to_vec();
    let mut tmp = vec![0u8; w * h];
    let mut row_out = vec![0u8; w];
    // Para cada meia-largura distinta do disco, uma passada horizontal.
    let mut widths: Vec<(i64, Vec<i64>)> = Vec::new();
    for dy in -r..=r {
        let hw = (((r * r - dy * dy) as f64).sqrt()).floor() as i64;
        match widths.iter_mut().find(|(k, _)| *k == hw) {
            Some((_, dys)) => dys.push(dy),
            None => widths.push((hw, vec![dy])),
        }
    }
    for (hw, dys) in widths {
        for y in 0..h {
            sliding(&mask[y * w..(y + 1) * w], &mut row_out, hw as usize, max);
            tmp[y * w..(y + 1) * w].copy_from_slice(&row_out);
        }
        for dy in dys {
            for y in 0..h as i64 {
                let sy = y + dy;
                if sy < 0 || sy >= h as i64 {
                    continue;
                }
                let (o, t) = (y as usize * w, sy as usize * w);
                for x in 0..w {
                    let v = tmp[t + x];
                    out[o + x] = if max {
                        out[o + x].max(v)
                    } else {
                        out[o + x].min(v)
                    };
                }
            }
        }
    }
    out
}

/// Expande a seleção em `r` pixels.
pub fn expand(mask: &[u8], w: u32, h: u32, r: u32) -> Option<Vec<u8>> {
    nonempty(morph(mask, w, h, r, true))
}

/// Contrai a seleção em `r` pixels (pode esvaziá-la).
pub fn contract(mask: &[u8], w: u32, h: u32, r: u32) -> Option<Vec<u8>> {
    nonempty(morph(mask, w, h, r, false))
}

/// Desfoque por caixa separável com extensão das bordas do canvas.
fn box_blur(src: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    if r == 0 {
        return src.to_vec();
    }
    let div = (2 * r + 1) as u32;
    let mut tmp = vec![0u8; w * h];
    let line = |get: &dyn Fn(usize) -> u8, n: usize, set: &mut dyn FnMut(usize, u8)| {
        let clamp = |j: i64| j.clamp(0, n as i64 - 1) as usize;
        let mut sum: u32 = (-(r as i64)..=r as i64)
            .map(|j| u32::from(get(clamp(j))))
            .sum();
        for i in 0..n {
            set(i, ((sum + div / 2) / div) as u8);
            sum += u32::from(get(clamp(i as i64 + r as i64 + 1)));
            sum -= u32::from(get(clamp(i as i64 - r as i64)));
        }
    };
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let mut res = vec![0u8; w];
        line(&|i| row[i], w, &mut |i, v| res[i] = v);
        tmp[y * w..(y + 1) * w].copy_from_slice(&res);
    }
    let mut out = vec![0u8; w * h];
    for x in 0..w {
        let mut res = vec![0u8; h];
        line(&|i| tmp[i * w + x], h, &mut |i, v| res[i] = v);
        for y in 0..h {
            out[y * w + x] = res[y];
        }
    }
    out
}

fn gaussian_like(mask: &[u8], w: u32, h: u32, sigma: f32) -> Vec<u8> {
    // Três passadas de caixa aproximam uma gaussiana.
    let width = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let r = (((width - 1.0) / 2.0).round() as usize).max(1);
    let mut m = mask.to_vec();
    for _ in 0..3 {
        m = box_blur(&m, w as usize, h as usize, r);
    }
    m
}

/// Desfoque gaussiano aproximado de uma máscara (raio em pixels), sem normalizar para `Option`.
pub fn blur_mask(mask: &[u8], w: u32, h: u32, radius: f32) -> Vec<u8> {
    if radius <= 0.0 {
        return mask.to_vec();
    }
    gaussian_like(mask, w, h, (radius / 2.0).min(100.0))
}

/// Desloca uma máscara por inteiros preenchendo o vazio com `fill`.
pub fn shift_mask(mask: &[u8], w: u32, h: u32, dx: i32, dy: i32, fill: u8) -> Vec<u8> {
    let (wi, hi) = (i64::from(w), i64::from(h));
    let mut out = vec![fill; mask.len()];
    for y in 0..hi {
        let ty = y + i64::from(dy);
        if ty < 0 || ty >= hi {
            continue;
        }
        let x0 = i64::from(dx).max(0);
        let x1 = (wi + i64::from(dx)).min(wi);
        if x0 >= x1 {
            continue;
        }
        let len = (x1 - x0) as usize;
        let s = (y * wi + x0 - i64::from(dx)) as usize;
        let d = (ty * wi + x0) as usize;
        out[d..d + len].copy_from_slice(&mask[s..s + len]);
    }
    out
}

/// Dilatação/erosão (sem normalizar para `Option`): `grow` expande, senão contrai.
pub fn morph_mask(mask: &[u8], w: u32, h: u32, r: u32, grow: bool) -> Vec<u8> {
    morph(mask, w, h, r, grow)
}

/// Pena: suaviza a borda numa transição de aproximadamente `radius` pixels.
pub fn feather(mask: &[u8], w: u32, h: u32, radius: f32) -> Option<Vec<u8>> {
    if radius <= 0.0 {
        return nonempty(mask.to_vec());
    }
    nonempty(gaussian_like(mask, w, h, (radius / 2.0).min(100.0)))
}

/// Suavizar: arredonda cantos e remove ruído da borda (desfoque seguido de contraste).
pub fn smooth(mask: &[u8], w: u32, h: u32, radius: u32) -> Option<Vec<u8>> {
    if radius == 0 {
        return nonempty(mask.to_vec());
    }
    let blurred = gaussian_like(mask, w, h, radius.min(100) as f32 / 2.0);
    nonempty(
        blurred
            .iter()
            .map(|&v| {
                ((f32::from(v) - 127.5) * 6.0 + 127.5)
                    .clamp(0.0, 255.0)
                    .round() as u8
            })
            .collect(),
    )
}

/// Desloca a máscara por inteiros, preenchendo com zero.
pub fn translate(mask: &[u8], w: u32, h: u32, dx: i32, dy: i32) -> Option<Vec<u8>> {
    let (wi, hi) = (i64::from(w), i64::from(h));
    let mut out = vec![0u8; mask.len()];
    for y in 0..hi {
        let ty = y + i64::from(dy);
        if ty < 0 || ty >= hi {
            continue;
        }
        let x0 = i64::from(dx).max(0);
        let x1 = (wi + i64::from(dx)).min(wi);
        if x0 >= x1 {
            continue;
        }
        let len = (x1 - x0) as usize;
        let s = (y * wi + x0 - i64::from(dx)) as usize;
        let d = (ty * wi + x0) as usize;
        out[d..d + len].copy_from_slice(&mask[s..s + len]);
    }
    nonempty(out)
}

/// Transforma a máscara (escala, rotação em graus, deslocamento) em torno do centro de seus
/// limites, com reamostragem bilinear.
pub fn transform(
    mask: &[u8],
    w: u32,
    h: u32,
    scale: (f32, f32),
    angle_deg: f32,
    delta: (f32, f32),
) -> Option<Vec<u8>> {
    let b = bounds(mask, w, h)?;
    let (cx, cy) = (b.x as f32 + b.w as f32 / 2.0, b.y as f32 + b.h as f32 / 2.0);
    let (sx, sy) = (scale.0.max(0.01), scale.1.max(0.01));
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let mut out = vec![0u8; mask.len()];
    let sample = |x: f32, y: f32| -> f32 {
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let get = |xx: f32, yy: f32| -> f32 {
            if xx < 0.0 || yy < 0.0 || xx >= w as f32 || yy >= h as f32 {
                0.0
            } else {
                f32::from(mask[yy as usize * w as usize + xx as usize])
            }
        };
        get(x0, y0) * (1.0 - fx) * (1.0 - fy)
            + get(x0 + 1.0, y0) * fx * (1.0 - fy)
            + get(x0, y0 + 1.0) * (1.0 - fx) * fy
            + get(x0 + 1.0, y0 + 1.0) * fx * fy
    };
    for y in 0..h {
        for x in 0..w {
            // Mapeamento inverso: destino -> origem.
            let (dx, dy) = (x as f32 + 0.5 - cx - delta.0, y as f32 + 0.5 - cy - delta.1);
            let (rx, ry) = (dx * cos + dy * sin, -dx * sin + dy * cos);
            let v = sample(rx / sx + cx, ry / sy + cy);
            out[y as usize * w as usize + x as usize] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    nonempty(out)
}

// ---- Aplicação da seleção às camadas ----

fn masked_apply(
    layer: &mut Layer,
    mask: Option<&[u8]>,
    label: &str,
    f: impl Fn([u8; 4], u8) -> [u8; 4],
) -> Option<PixelPatch> {
    if layer.is_group() || mask.is_some_and(|m| m.len() != layer.pixels.len() / 4) {
        return None;
    }
    let area = match mask {
        Some(m) => bounds(m, layer.width, layer.height)?,
        None => Rect::full(layer.width, layer.height),
    };
    let before = layer.pixels.clone();
    let w = layer.width as usize;
    let mut changed: Option<Rect> = None;
    let px = layer.pixels_mut();
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let m = mask.map_or(255, |m| m[y as usize * w + x as usize]);
            if m == 0 {
                continue;
            }
            let i = (y as usize * w + x as usize) * 4;
            let old = [px[i], px[i + 1], px[i + 2], px[i + 3]];
            let new = f(old, m);
            if new != old {
                px[i..i + 4].copy_from_slice(&new);
                let r = Rect::new(x, y, 1, 1);
                changed = Some(changed.map_or(r, |c| c.union(r)));
            }
        }
    }
    PixelPatch::from_diff(label, layer, &before, changed?)
}

/// Apaga (alfa → 0) os pixels selecionados, proporcionalmente à máscara.
pub fn clear_selected(layer: &mut Layer, mask: Option<&[u8]>) -> Option<PixelPatch> {
    masked_apply(layer, mask, "Apagar seleção", |p, m| {
        let a = (u32::from(p[3]) * (255 - u32::from(m)) + 127) / 255;
        [p[0], p[1], p[2], a as u8]
    })
}

/// Preenche a seleção com `color` (opacidade extra `opacity`); `lock_alpha` preserva o alfa.
pub fn fill_selected(
    layer: &mut Layer,
    mask: Option<&[u8]>,
    color: [u8; 4],
    opacity: f32,
    lock_alpha: bool,
) -> Option<PixelPatch> {
    let a = (f32::from(color[3]) * opacity.clamp(0.0, 1.0)).round() as u8;
    masked_apply(layer, mask, "Preencher seleção", |p, m| {
        let filled = over([color[0], color[1], color[2], a], p, 1.0);
        let out = lerp_px(p, filled, f32::from(m) / 255.0);
        if lock_alpha {
            if p[3] == 0 {
                p
            } else {
                [out[0], out[1], out[2], p[3]]
            }
        } else {
            out
        }
    })
}

/// Copia os pixels selecionados para uma nova camada do tamanho do documento
/// (alfa multiplicado pela máscara). Sem máscara, copia a camada inteira.
pub fn extract_selected(layer: &Layer, mask: Option<&[u8]>, name: &str) -> Layer {
    let mut px = layer.pixels.as_ref().clone();
    if let Some(m) = mask {
        for (p, &mv) in px.as_chunks_mut::<4>().0.iter_mut().zip(m) {
            p[3] = ((u32::from(p[3]) * u32::from(mv) + 127) / 255) as u8;
        }
    }
    Layer::from_pixels(name, layer.width, layer.height, px)
}

/// Aplica a seleção como opacidade por pixel: mistura `original` e `modified` conforme a máscara.
pub fn restrict_to_mask(original: &[u8], modified: &mut [u8], mask: &[u8]) {
    for ((o, n), &m) in original
        .as_chunks::<4>()
        .0
        .iter()
        .zip(modified.as_chunks_mut::<4>().0.iter_mut())
        .zip(mask)
    {
        if m == 255 || o == n {
            continue;
        }
        *n = if m == 0 {
            *o
        } else {
            lerp_px(*o, *n, f32::from(m) / 255.0)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, History};

    const W: u32 = 20;
    const H: u32 = 20;

    fn at(m: &[u8], x: u32, y: u32) -> u8 {
        m[(y * W + x) as usize]
    }

    fn count(m: &[u8]) -> usize {
        m.iter().filter(|&&v| v > 0).count()
    }

    #[test]
    fn rect_hard_and_fractional_edges() {
        let m = rect_mask(W, H, (2.0, 3.0), (6.0, 8.0), true);
        assert_eq!(count(&m), 4 * 5);
        assert_eq!(at(&m, 2, 3), 255);
        assert_eq!(at(&m, 6, 3), 0);
        let f = rect_mask(W, H, (2.5, 3.0), (6.0, 8.0), true);
        assert_eq!(at(&f, 2, 4), 128);
        assert_eq!(at(&f, 3, 4), 255);
        let snapped = rect_mask(W, H, (2.4, 3.0), (6.0, 8.0), false);
        assert_eq!(at(&snapped, 2, 4), 255);
        assert_eq!(
            rect_mask(W, H, (6.0, 8.0), (2.0, 3.0), true),
            rect_mask(W, H, (2.0, 3.0), (6.0, 8.0), true)
        );
    }

    #[test]
    fn shapes_partially_outside_canvas_are_clipped() {
        let m = rect_mask(W, H, (-5.0, -5.0), (5.0, 5.0), true);
        assert_eq!(count(&m), 25);
        assert_eq!(m.len(), (W * H) as usize);
        let e = ellipse_mask(W, H, (15.0, 15.0), (30.0, 30.0), true);
        assert!(count(&e) > 0 && at(&e, 19, 19) == 255 && at(&e, 15, 15) < 255);
        let p = polygon_mask(W, H, &[(-10.0, 10.0), (30.0, 5.0), (30.0, 15.0)], true);
        assert!(at(&p, 10, 10) == 255 && at(&p, 10, 0) == 0);
        assert!(count(&rect_mask(W, H, (40.0, 40.0), (50.0, 50.0), true)) == 0);
    }

    #[test]
    fn ellipse_area_and_antialiasing() {
        let m = ellipse_mask(100, 100, (10.0, 10.0), (90.0, 90.0), true);
        let area: f32 = m.iter().map(|&v| f32::from(v) / 255.0).sum();
        let expect = std::f32::consts::PI * 40.0 * 40.0;
        assert!((area - expect).abs() / expect < 0.01, "{area} vs {expect}");
        assert!(m.iter().any(|&v| v > 0 && v < 255), "bordas suaves");
        let hard = ellipse_mask(100, 100, (10.0, 10.0), (90.0, 90.0), false);
        assert!(hard.iter().all(|&v| v == 0 || v == 255));
        assert_eq!(m[50 * 100 + 50], 255);
        assert_eq!(m[0], 0);
    }

    #[test]
    fn polygon_triangle_area_and_evenodd() {
        let tri = polygon_mask(100, 100, &[(10.0, 10.0), (90.0, 10.0), (10.0, 90.0)], true);
        let area: f32 = tri.iter().map(|&v| f32::from(v) / 255.0).sum();
        assert!((area - 3200.0).abs() < 60.0, "{area}");
        assert!(polygon_mask(W, H, &[(1.0, 1.0), (5.0, 5.0)], true)
            .iter()
            .all(|&v| v == 0));
    }

    #[test]
    fn combine_modes() {
        let a = rect_mask(W, H, (0.0, 0.0), (10.0, 10.0), true);
        let b = rect_mask(W, H, (5.0, 5.0), (15.0, 15.0), true);
        assert_eq!(
            count(&combine(Some(&a), &b, Combine::Add).unwrap()),
            100 + 100 - 25
        );
        assert_eq!(
            count(&combine(Some(&a), &b, Combine::Intersect).unwrap()),
            25
        );
        assert_eq!(
            count(&combine(Some(&a), &b, Combine::Subtract).unwrap()),
            100 - 25
        );
        assert_eq!(combine(Some(&a), &b, Combine::Replace).unwrap(), b);
        assert_eq!(count(&combine(None, &b, Combine::Add).unwrap()), 100);
        assert!(combine(None, &b, Combine::Subtract).is_none());
        let disjoint = rect_mask(W, H, (15.0, 15.0), (19.0, 19.0), true);
        assert!(combine(Some(&a), &disjoint, Combine::Intersect).is_none());
        // Subtrair com borda suave é proporcional.
        let half = vec![128u8; (W * H) as usize];
        let s = combine(Some(&a), &half, Combine::Subtract).unwrap();
        assert!((126..=129).contains(&at(&s, 1, 1)));
    }

    #[test]
    fn invert_and_bounds() {
        let a = rect_mask(W, H, (3.0, 4.0), (8.0, 9.0), true);
        assert_eq!(bounds(&a, W, H), Some(Rect::new(3, 4, 5, 5)));
        let inv = invert(&a).unwrap();
        assert_eq!(count(&inv), (W * H) as usize - 25);
        assert_eq!(invert(&inv).unwrap(), a);
        assert!(invert(&select_all(W, H)).is_none());
        assert!(bounds(&vec![0; (W * H) as usize], W, H).is_none());
    }

    #[test]
    fn expand_contract_round_trip_on_disc() {
        let a = rect_mask(W, H, (6.0, 6.0), (14.0, 14.0), true);
        let e = expand(&a, W, H, 2).unwrap();
        assert_eq!(bounds(&e, W, H), Some(Rect::new(4, 4, 12, 12)));
        // Cantos arredondados (disco), não quadrados.
        assert_eq!(at(&e, 4, 4), 0);
        assert_eq!(at(&e, 5, 10), 255);
        let c = contract(&a, W, H, 2).unwrap();
        assert_eq!(bounds(&c, W, H), Some(Rect::new(8, 8, 4, 4)));
        assert!(contract(&a, W, H, 5).is_none(), "contrair demais esvazia");
        assert_eq!(expand(&a, W, H, 0).unwrap(), a);
    }

    #[test]
    fn canvas_edge_does_not_erode_or_fade() {
        let all = select_all(W, H);
        assert_eq!(contract(&all, W, H, 3).unwrap(), all);
        assert_eq!(feather(&all, W, H, 6.0).unwrap(), all);
    }

    #[test]
    fn feather_makes_soft_edge_and_preserves_core() {
        let a = rect_mask(60, 60, (15.0, 15.0), (45.0, 45.0), true);
        let f = feather(&a, 60, 60, 8.0).unwrap();
        assert_eq!(f[30 * 60 + 30], 255);
        let edge = f[30 * 60 + 15];
        assert!(edge > 60 && edge < 200, "{edge}");
        assert!(f[30 * 60 + 10] > 0, "alcança fora da borda");
        assert!(f[30 * 60 + 4] < 30);
        // Pena 0 não altera.
        assert_eq!(feather(&a, 60, 60, 0.0).unwrap(), a);
    }

    #[test]
    fn smooth_rounds_corners_and_removes_specks() {
        let mut a = rect_mask(40, 40, (10.0, 10.0), (30.0, 30.0), true);
        a[5 * 40 + 5] = 255; // ruído isolado
        let s = smooth(&a, 40, 40, 3).unwrap();
        assert_eq!(s[5 * 40 + 5], 0);
        assert!(s[10 * 40 + 10] < 255, "canto arredondado");
        assert_eq!(s[20 * 40 + 20], 255);
    }

    #[test]
    fn translate_and_transform() {
        let a = rect_mask(W, H, (2.0, 2.0), (6.0, 6.0), true);
        let t = translate(&a, W, H, 5, 3).unwrap();
        assert_eq!(bounds(&t, W, H), Some(Rect::new(7, 5, 4, 4)));
        assert!(translate(&a, W, H, -10, 0).is_none());
        let id = transform(&a, W, H, (1.0, 1.0), 0.0, (0.0, 0.0)).unwrap();
        assert_eq!(id, a);
        let moved = transform(&a, W, H, (1.0, 1.0), 0.0, (4.0, 2.0)).unwrap();
        assert_eq!(bounds(&moved, W, H), Some(Rect::new(6, 4, 4, 4)));
        let big = transform(&a, W, H, (2.0, 2.0), 0.0, (0.0, 0.0)).unwrap();
        let area: f32 = big.iter().map(|&v| f32::from(v) / 255.0).sum();
        assert!((area - 64.0).abs() < 4.0, "{area}");
        let rot = transform(&a, W, H, (1.0, 1.0), 90.0, (0.0, 0.0)).unwrap();
        assert_eq!(bounds(&rot, W, H), bounds(&a, W, H));
    }

    #[test]
    fn wand_and_color_range() {
        let mut px = vec![255u8; (W * H * 4) as usize];
        for y in 0..H {
            for x in 10..W {
                let i = ((y * W + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[0, 0, 255, 255]);
            }
        }
        let wand = magic_wand(&px, W, H, 2, 2, 0, true);
        assert_eq!(count(&wand), 10 * 20);
        let range = color_range(&px, [0, 0, 255], 0);
        assert_eq!(count(&range), 10 * 20);
        // Faixa suave: pixels próximos entram parcialmente.
        let soft = color_range(&px, [0, 0, 200], 100);
        assert!(at(&soft, 15, 5) > 100 && at(&soft, 15, 5) < 255);
        assert_eq!(at(&soft, 2, 2), 0);
        // Transparente não entra.
        let clear = vec![0u8; (W * H * 4) as usize];
        assert_eq!(count(&color_range(&clear, [0, 0, 0], 50)), 0);
    }

    fn layer_and_sel() -> (Layer, Vec<u8>) {
        let l = Layer::filled("l", W, H, [10, 20, 30, 255]);
        (l, rect_mask(W, H, (5.0, 5.0), (10.0, 10.0), true))
    }

    #[test]
    fn clear_and_fill_do_not_touch_pixels_outside() {
        let (mut l, sel) = layer_and_sel();
        let original = l.pixels.clone();
        let p = clear_selected(&mut l, Some(&sel)).unwrap();
        assert_eq!(l.pixel(6, 6).unwrap()[3], 0);
        assert_eq!(l.pixel(4, 6).unwrap(), [10, 20, 30, 255]);
        for y in 0..H {
            for x in 0..W {
                if at(&sel, x, y) == 0 {
                    assert_eq!(l.pixel(x, y), Some([10, 20, 30, 255]));
                }
            }
        }
        assert_eq!(p.rect(), Rect::new(5, 5, 5, 5));
        let mut l2 = Layer::from_pixels("x", W, H, original.as_ref().clone());
        fill_selected(&mut l2, Some(&sel), [255, 0, 0, 255], 1.0, false).unwrap();
        assert_eq!(l2.pixel(7, 7).unwrap(), [255, 0, 0, 255]);
        assert_eq!(l2.pixel(10, 7).unwrap(), [10, 20, 30, 255]);
    }

    #[test]
    fn soft_selection_gives_partial_effect_and_undo_restores() {
        let mut doc = Document::new("t", W, H, Some([0, 0, 0, 255])).unwrap();
        let original = doc.layers[0].pixels.clone();
        let sel = vec![128u8; (W * H) as usize];
        let mut layer = doc.layers[0].clone();
        let patch =
            fill_selected(&mut layer, Some(&sel), [255, 255, 255, 255], 1.0, false).unwrap();
        let v = layer.pixel(3, 3).unwrap();
        assert!((126..=129).contains(&v[0]), "{v:?}");
        doc.layers[0] = layer;
        let mut h = History::new();
        h.record(&mut doc, Box::new(patch));
        h.undo(&mut doc);
        assert_eq!(doc.layers[0].pixels, original);
    }

    #[test]
    fn fill_without_selection_fills_all_and_alpha_lock_keeps_alpha() {
        let mut l = Layer::transparent("l", 4, 4);
        l.set_pixel(1, 1, [0, 0, 0, 100]);
        let mut a = l.clone();
        fill_selected(&mut a, None, [255, 0, 0, 255], 1.0, false).unwrap();
        assert_eq!(a.pixel(0, 0).unwrap(), [255, 0, 0, 255]);
        fill_selected(&mut l, None, [255, 0, 0, 255], 1.0, true).unwrap();
        assert_eq!(l.pixel(1, 1).unwrap(), [255, 0, 0, 100]);
        assert_eq!(l.pixel(0, 0).unwrap()[3], 0);
    }

    #[test]
    fn extract_uses_mask_as_alpha() {
        let (l, sel) = layer_and_sel();
        let e = extract_selected(&l, Some(&sel), "Seleção");
        assert_eq!(e.pixel(6, 6).unwrap(), [10, 20, 30, 255]);
        assert_eq!(e.pixel(0, 0).unwrap()[3], 0);
        assert_eq!(extract_selected(&l, None, "x").pixel(0, 0).unwrap()[3], 255);
    }

    #[test]
    fn restrict_to_mask_blends() {
        let orig = vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255];
        let mut modified = vec![200, 200, 200, 255, 200, 200, 200, 255, 200, 200, 200, 255];
        restrict_to_mask(&orig, &mut modified, &[0, 128, 255]);
        assert_eq!(modified[0], 0);
        assert!((99..=101).contains(&modified[4]));
        assert_eq!(modified[8], 200);
    }
}
