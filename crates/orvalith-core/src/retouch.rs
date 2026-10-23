//! Retoque clássico: carimbo de clone, correção (separação de frequências), correção pontual,
//! remendo, olhos vermelhos e pincéis de desfocar/nitidez/borrar/subexpor/superexpor/esponja.
//!
//! Todas as operações trabalham sobre um buffer RGBA8 reto, são limitadas pela seleção e
//! misturam em alfa pré-multiplicado (sem halos em áreas transparentes). Sem IA.

use std::sync::atomic::AtomicBool;

use crate::filter::{self, mix_px, Filter};

/// Parâmetros do pincel de retoque.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Brush {
    pub radius: f32,
    /// 0 = borda suave desde o centro, 1 = borda dura.
    pub hardness: f32,
}

impl Brush {
    /// Cobertura (0..=1) a `d` pixels do centro.
    pub fn falloff(&self, d: f32) -> f32 {
        let r = self.radius.max(0.5);
        if d >= r {
            return 0.0;
        }
        let inner = r * self.hardness.clamp(0.0, 1.0);
        if d <= inner {
            return 1.0;
        }
        let t = (d - inner) / (r - inner).max(1e-3);
        let s = 1.0 - t;
        s * s * (3.0 - 2.0 * s)
    }

    fn bbox(&self, cx: f32, cy: f32, w: usize, h: usize) -> (usize, usize, usize, usize) {
        let r = self.radius.ceil() + 1.0;
        (
            (cx - r).floor().max(0.0) as usize,
            (cy - r).floor().max(0.0) as usize,
            ((cx + r).ceil().max(0.0) as usize + 1).min(w),
            ((cy + r).ceil().max(0.0) as usize + 1).min(h),
        )
    }
}

/// Gera os centros dos toques ao longo de um traço, espaçados de `spacing` pixels.
pub struct Walker {
    last: [f32; 2],
    left: f32,
}

impl Walker {
    pub fn new(start: [f32; 2]) -> Self {
        Self {
            last: start,
            left: 0.0,
        }
    }

    pub fn advance(&mut self, to: [f32; 2], spacing: f32) -> Vec<[f32; 2]> {
        let spacing = spacing.max(0.5);
        let (dx, dy) = (to[0] - self.last[0], to[1] - self.last[1]);
        let len = (dx * dx + dy * dy).sqrt();
        let mut out = Vec::new();
        if len <= 0.0 {
            return out;
        }
        let mut d = spacing - self.left;
        while d <= len {
            let t = d / len;
            out.push([self.last[0] + dx * t, self.last[1] + dy * t]);
            d += spacing;
        }
        self.left = (self.left + len) % spacing;
        self.last = to;
        out
    }
}

fn sel_w(sel: Option<&[u8]>, i: usize) -> f32 {
    sel.map_or(1.0, |m| f32::from(m.get(i).copied().unwrap_or(0)) / 255.0)
}

fn px_at(buf: &[u8], i: usize) -> [u8; 4] {
    [buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], buf[i * 4 + 3]]
}

fn put(buf: &mut [u8], i: usize, p: [u8; 4]) {
    buf[i * 4..i * 4 + 4].copy_from_slice(&p);
}

// ---------------------------------------------------------------------------------------
// Clone e correção

/// Um toque do carimbo de clone: copia de `snapshot` deslocado por `offset` (origem = ponto + offset).
#[allow(clippy::too_many_arguments)]
pub fn clone_dab(
    dst: &mut [u8],
    snapshot: &[u8],
    w: usize,
    h: usize,
    center: [f32; 2],
    offset: [i32; 2],
    brush: Brush,
    opacity: f32,
    sel: Option<&[u8]>,
) {
    let (x0, y0, x1, y1) = brush.bbox(center[0], center[1], w, h);
    for y in y0..y1 {
        for x in x0..x1 {
            let a = brush.falloff(
                ((x as f32 + 0.5 - center[0]).powi(2) + (y as f32 + 0.5 - center[1]).powi(2))
                    .sqrt(),
            ) * opacity
                * sel_w(sel, y * w + x);
            if a <= 0.0 {
                continue;
            }
            let (sx, sy) = (x as i32 + offset[0], y as i32 + offset[1]);
            if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                continue;
            }
            let s = px_at(snapshot, sy as usize * w + sx as usize);
            let i = y * w + x;
            put(dst, i, mix_px(px_at(dst, i), s, a));
        }
    }
}

/// Passa-baixas usado pela correção (desfoque gaussiano com alfa correto).
pub fn lowpass(px: &[u8], w: usize, h: usize, sigma: f32) -> Vec<u8> {
    let no = AtomicBool::new(false);
    filter::apply(
        &Filter::GaussianBlur {
            radius: sigma.max(0.5),
        },
        px,
        w as u32,
        h as u32,
        &no,
    )
    .unwrap_or_else(|| px.to_vec())
}

/// Cor média (RGB reto ponderado pelo alfa) do anel entre `r0` e `r1` ao redor de `c`.
fn ring_mean(px: &[u8], w: usize, h: usize, c: [f32; 2], r0: f32, r1: f32) -> Option<[f32; 3]> {
    let mut acc = [0f32; 3];
    let mut wsum = 0f32;
    for k in 0..48 {
        for rr in [r0, (r0 + r1) / 2.0, r1] {
            let a = k as f32 / 48.0 * std::f32::consts::TAU;
            let (x, y) = (
                (c[0] + rr * a.cos()).round() as i32,
                (c[1] + rr * a.sin()).round() as i32,
            );
            if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                continue;
            }
            let p = px_at(px, y as usize * w + x as usize);
            let wt = f32::from(p[3]);
            for ch in 0..3 {
                acc[ch] += f32::from(p[ch]) * wt;
            }
            wsum += wt;
        }
    }
    (wsum > 0.0).then(|| acc.map(|v| v / wsum))
}

/// Um toque do pincel de correção: textura da origem + cor/luz do local (a média do anel ao
/// redor do toque substitui a do anel ao redor da origem).
#[allow(clippy::too_many_arguments)]
pub fn heal_dab(
    dst: &mut [u8],
    snapshot: &[u8],
    w: usize,
    h: usize,
    center: [f32; 2],
    offset: [i32; 2],
    brush: Brush,
    opacity: f32,
    sel: Option<&[u8]>,
) {
    let (r0, r1) = (brush.radius, brush.radius * 1.5 + 1.0);
    let src_c = [center[0] + offset[0] as f32, center[1] + offset[1] as f32];
    let shift = match (
        ring_mean(snapshot, w, h, center, r0, r1),
        ring_mean(snapshot, w, h, src_c, r0, r1),
    ) {
        (Some(d), Some(s)) => [d[0] - s[0], d[1] - s[1], d[2] - s[2]],
        _ => [0.0; 3],
    };
    let (x0, y0, x1, y1) = brush.bbox(center[0], center[1], w, h);
    for y in y0..y1 {
        for x in x0..x1 {
            let a = brush.falloff(
                ((x as f32 + 0.5 - center[0]).powi(2) + (y as f32 + 0.5 - center[1]).powi(2))
                    .sqrt(),
            ) * opacity
                * sel_w(sel, y * w + x);
            if a <= 0.0 {
                continue;
            }
            let (sx, sy) = (x as i32 + offset[0], y as i32 + offset[1]);
            if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                continue;
            }
            let s = px_at(snapshot, sy as usize * w + sx as usize);
            if s[3] == 0 {
                continue;
            }
            let healed = [
                (f32::from(s[0]) + shift[0]).round().clamp(0.0, 255.0) as u8,
                (f32::from(s[1]) + shift[1]).round().clamp(0.0, 255.0) as u8,
                (f32::from(s[2]) + shift[2]).round().clamp(0.0, 255.0) as u8,
                s[3],
            ];
            let i = y * w + x;
            put(dst, i, mix_px(px_at(dst, i), healed, a));
        }
    }
}

/// Escolhe automaticamente a melhor origem para a correção pontual: o deslocamento cujo anel
/// de vizinhança mais se parece com o anel ao redor do ponto. `None` se nada couber na imagem.
pub fn best_source_offset(
    px: &[u8],
    w: usize,
    h: usize,
    center: [f32; 2],
    radius: f32,
) -> Option<[i32; 2]> {
    let ring_pts: Vec<[f32; 2]> = (0..2)
        .flat_map(|k| {
            let rr = radius * (1.15 + 0.2 * k as f32);
            (0..24).map(move |i| {
                let a = i as f32 / 24.0 * std::f32::consts::TAU;
                [rr * a.cos(), rr * a.sin()]
            })
        })
        .collect();
    let at = |x: i32, y: i32| -> Option<[f32; 3]> {
        if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
            return None;
        }
        let p = px_at(px, y as usize * w + x as usize);
        (p[3] > 0).then(|| [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])])
    };
    let target: Vec<Option<[f32; 3]>> = ring_pts
        .iter()
        .map(|o| {
            at(
                (center[0] + o[0]).round() as i32,
                (center[1] + o[1]).round() as i32,
            )
        })
        .collect();
    let mut best: Option<(f32, [i32; 2])> = None;
    for scale in [2.0f32, 2.8, 3.8, 5.0] {
        for i in 0..16 {
            let a = i as f32 / 16.0 * std::f32::consts::TAU;
            let off = [
                (scale * radius * a.cos()).round() as i32,
                (scale * radius * a.sin()).round() as i32,
            ];
            let mut cost = 0.0;
            let mut n = 0;
            let mut ok = true;
            for (o, t) in ring_pts.iter().zip(&target) {
                let Some(t) = t else { continue };
                let (sx, sy) = (
                    (center[0] + o[0]).round() as i32 + off[0],
                    (center[1] + o[1]).round() as i32 + off[1],
                );
                match at(sx, sy) {
                    Some(s) => {
                        cost += (0..3).map(|c| (s[c] - t[c]).powi(2)).sum::<f32>();
                        n += 1;
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            // O disco inteiro da origem deve caber na imagem.
            let (cx, cy) = (center[0] as i32 + off[0], center[1] as i32 + off[1]);
            let r = radius.ceil() as i32;
            if !ok || n < 8 || cx - r < 0 || cy - r < 0 || cx + r >= w as i32 || cy + r >= h as i32
            {
                continue;
            }
            let cost = cost / n as f32;
            if best.is_none_or(|(b, _)| cost < b) {
                best = Some((cost, off));
            }
        }
    }
    best.map(|(_, o)| o)
}

/// Correção pontual: um clique remove a imperfeição usando a melhor origem encontrada.
pub fn spot_heal(
    px: &mut [u8],
    w: usize,
    h: usize,
    center: [f32; 2],
    radius: f32,
    sel: Option<&[u8]>,
) -> bool {
    let Some(off) = best_source_offset(px, w, h, center, radius) else {
        return false;
    };
    let snapshot = px.to_vec();
    let brush = Brush {
        radius: radius * 1.15,
        hardness: 0.6,
    };
    heal_dab(px, &snapshot, w, h, center, off, brush, 1.0, sel);
    true
}

/// Remendo: a área da seleção é refeita com a textura de outra região (`offset`), mantendo a
/// cor e a luz do local (separação de frequências). Retorna quantos pixels mudaram.
pub fn patch(px: &mut [u8], w: usize, h: usize, sel: &[u8], offset: [i32; 2]) -> usize {
    let snapshot = px.to_vec();
    let (mut minx, mut miny, mut maxx, mut maxy) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if sel[y * w + x] > 0 {
                minx = minx.min(x);
                miny = miny.min(y);
                maxx = maxx.max(x);
                maxy = maxy.max(y);
            }
        }
    }
    if minx > maxx {
        return 0;
    }
    let span = ((maxx - minx).min(maxy - miny) as f32).max(4.0);
    let sigma = (span / 2.0).clamp(3.0, 20.0);
    let low_src = lowpass(&snapshot, w, h, sigma);
    // Destino: só a vizinhança fora da seleção conta (convolução normalizada).
    let mut outside = snapshot.clone();
    for (i, p) in outside.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        p[3] = (u32::from(p[3]) * (255 - u32::from(sel[i])) / 255) as u8;
    }
    let low_dst = lowpass(&outside, w, h, sigma);
    let mut changed = 0;
    for y in miny..=maxy {
        for x in minx..=maxx {
            let i = y * w + x;
            let a = f32::from(sel[i]) / 255.0;
            if a <= 0.0 {
                continue;
            }
            let (sx, sy) = (x as i32 + offset[0], y as i32 + offset[1]);
            if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                continue;
            }
            let si = sy as usize * w + sx as usize;
            let s = px_at(&snapshot, si);
            if s[3] == 0 {
                continue;
            }
            let (ld, ls) = (px_at(&low_dst, i), px_at(&low_src, si));
            let ld = if ld[3] < 8 { px_at(&low_src, i) } else { ld };
            let mut healed = [0u8; 4];
            for c in 0..3 {
                healed[c] =
                    (i32::from(ld[c]) + i32::from(s[c]) - i32::from(ls[c])).clamp(0, 255) as u8;
            }
            healed[3] = s[3];
            let new = mix_px(px_at(px, i), healed, a);
            if new != px_at(px, i) {
                changed += 1;
            }
            put(px, i, new);
        }
    }
    changed
}

/// Olhos vermelhos: escurece o vermelho dominante dentro do círculo. Retorna os pixels alterados.
pub fn red_eye(
    px: &mut [u8],
    w: usize,
    h: usize,
    center: [f32; 2],
    radius: f32,
    amount: f32,
    sel: Option<&[u8]>,
) -> usize {
    let brush = Brush {
        radius,
        hardness: 0.7,
    };
    let (x0, y0, x1, y1) = brush.bbox(center[0], center[1], w, h);
    let mut changed = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let f = brush.falloff(
                ((x as f32 + 0.5 - center[0]).powi(2) + (y as f32 + 0.5 - center[1]).powi(2))
                    .sqrt(),
            ) * sel_w(sel, y * w + x);
            if f <= 0.0 {
                continue;
            }
            let i = (y * w + x) * 4;
            let (r, g, b) = (f32::from(px[i]), f32::from(px[i + 1]), f32::from(px[i + 2]));
            if px[i + 3] == 0 || r < 60.0 {
                continue;
            }
            let other = g.max(b).max(1.0);
            let redness = ((r / other - 1.8) / 1.0).clamp(0.0, 1.0);
            if redness <= 0.0 {
                continue;
            }
            let target = ((g + b) / 2.0 * 0.85).min(r);
            let t = (redness * f * amount.clamp(0.0, 1.0)).clamp(0.0, 1.0);
            let nr = (r + (target - r) * t).round().clamp(0.0, 255.0) as u8;
            if nr != px[i] {
                px[i] = nr;
                changed += 1;
            }
        }
    }
    changed
}

// ---------------------------------------------------------------------------------------
// Pincéis de efeito

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Shadows,
    Midtones,
    Highlights,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Blur,
    Sharpen,
    Smudge,
    Dodge(Range),
    Burn(Range),
    Sponge { saturate: bool },
}

impl Op {
    pub const NAMES: [&'static str; 6] = [
        "Desfocar",
        "Nitidez",
        "Borrar",
        "Subexpor",
        "Superexpor",
        "Esponja",
    ];
}

fn range_weight(l: f32, r: Range) -> f32 {
    let t = 2.0 * l - 1.0;
    match r {
        Range::Shadows => (-t).max(0.0),
        Range::Midtones => 1.0 - t.abs(),
        Range::Highlights => t.max(0.0),
    }
}

/// Estado do borrar entre toques: o que o "dedo" carrega (alfa pré-multiplicado).
#[derive(Default)]
pub struct Smudge {
    size: usize,
    buf: Vec<[f32; 4]>,
    origin: (i32, i32),
}

fn pm(p: [u8; 4]) -> [f32; 4] {
    let k = f32::from(p[3]) / 255.0;
    [
        f32::from(p[0]) * k,
        f32::from(p[1]) * k,
        f32::from(p[2]) * k,
        f32::from(p[3]),
    ]
}

fn unpm(p: [f32; 4]) -> [u8; 4] {
    if p[3] < 0.5 {
        return [0; 4];
    }
    let k = 255.0 / p[3];
    [
        (p[0] * k + 0.5).clamp(0.0, 255.0) as u8,
        (p[1] * k + 0.5).clamp(0.0, 255.0) as u8,
        (p[2] * k + 0.5).clamp(0.0, 255.0) as u8,
        (p[3] + 0.5).clamp(0.0, 255.0) as u8,
    ]
}

/// Um toque de desfocar/nitidez/borrar/subexpor/superexpor/esponja sobre `work`.
#[allow(clippy::too_many_arguments)]
pub fn effect_dab(
    op: Op,
    work: &mut [u8],
    w: usize,
    h: usize,
    center: [f32; 2],
    brush: Brush,
    strength: f32,
    sel: Option<&[u8]>,
    smudge: &mut Smudge,
) {
    let (x0, y0, x1, y1) = brush.bbox(center[0], center[1], w, h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let cover = |x: usize, y: usize| {
        brush.falloff(
            ((x as f32 + 0.5 - center[0]).powi(2) + (y as f32 + 0.5 - center[1]).powi(2)).sqrt(),
        ) * sel_w(sel, y * w + x)
            * strength
    };
    match op {
        Op::Blur | Op::Sharpen => {
            // Média 5×5 lida de uma cópia da região (com margem), escrita no buffer de trabalho.
            let (rx0, ry0) = (x0.saturating_sub(2), y0.saturating_sub(2));
            let (rx1, ry1) = ((x1 + 2).min(w), (y1 + 2).min(h));
            let reg: Vec<[f32; 4]> = (ry0..ry1)
                .flat_map(|y| (rx0..rx1).map(move |x| (x, y)))
                .map(|(x, y)| pm(px_at(work, y * w + x)))
                .collect();
            let rw = rx1 - rx0;
            for y in y0..y1 {
                for x in x0..x1 {
                    let a = cover(x, y);
                    if a <= 0.0 {
                        continue;
                    }
                    let mut mean = [0f32; 4];
                    let mut n = 0.0;
                    for dy in -2i32..=2 {
                        for dx in -2i32..=2 {
                            let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                            if xx < rx0 as i32
                                || yy < ry0 as i32
                                || xx >= rx1 as i32
                                || yy >= ry1 as i32
                            {
                                continue;
                            }
                            let p = reg[(yy as usize - ry0) * rw + (xx as usize - rx0)];
                            for c in 0..4 {
                                mean[c] += p[c];
                            }
                            n += 1.0;
                        }
                    }
                    let cur = reg[(y - ry0) * rw + (x - rx0)];
                    let mut out = cur;
                    for c in 0..4 {
                        let m = mean[c] / n;
                        out[c] = if op == Op::Blur {
                            cur[c] + (m - cur[c]) * a
                        } else if c < 3 {
                            cur[c] + (cur[c] - m) * a * 1.5
                        } else {
                            cur[c]
                        };
                    }
                    if op == Op::Sharpen {
                        for c in 0..3 {
                            out[c] = out[c].clamp(0.0, out[3]);
                        }
                    }
                    put(work, y * w + x, unpm(out));
                }
            }
        }
        Op::Smudge => {
            let size = (brush.radius.ceil() as usize) * 2 + 1;
            let origin = (
                center[0].round() as i32 - size as i32 / 2,
                center[1].round() as i32 - size as i32 / 2,
            );
            if smudge.size != size || smudge.buf.is_empty() {
                smudge.size = size;
                smudge.buf = vec![[0.0; 4]; size * size];
                for ly in 0..size {
                    for lx in 0..size {
                        let (x, y) = (origin.0 + lx as i32, origin.1 + ly as i32);
                        if x >= 0 && y >= 0 && x < w as i32 && y < h as i32 {
                            smudge.buf[ly * size + lx] =
                                pm(px_at(work, y as usize * w + x as usize));
                        }
                    }
                }
                smudge.origin = origin;
                return; // o primeiro toque só "pega" a tinta
            }
            let mut next = smudge.buf.clone();
            for y in y0..y1 {
                for x in x0..x1 {
                    let a = cover(x, y).min(1.0);
                    let (lx, ly) = (x as i32 - origin.0, y as i32 - origin.1);
                    if a <= 0.0 || lx < 0 || ly < 0 || lx >= size as i32 || ly >= size as i32 {
                        continue;
                    }
                    let i = y * w + x;
                    let cur = pm(px_at(work, i));
                    let carried = smudge.buf[ly as usize * size + lx as usize];
                    let mut out = cur;
                    for c in 0..4 {
                        out[c] = cur[c] + (carried[c] - cur[c]) * a;
                    }
                    put(work, i, unpm(out));
                    for c in 0..4 {
                        next[ly as usize * size + lx as usize][c] =
                            carried[c] + (out[c] - carried[c]) * (1.0 - a * 0.5);
                    }
                }
            }
            smudge.buf = next;
            smudge.origin = origin;
        }
        Op::Dodge(range) | Op::Burn(range) => {
            for y in y0..y1 {
                for x in x0..x1 {
                    let a = cover(x, y);
                    if a <= 0.0 {
                        continue;
                    }
                    let i = (y * w + x) * 4;
                    if work[i + 3] == 0 {
                        continue;
                    }
                    let l = (0.299 * f32::from(work[i])
                        + 0.587 * f32::from(work[i + 1])
                        + 0.114 * f32::from(work[i + 2]))
                        / 255.0;
                    let e = (a * range_weight(l, range)).clamp(0.0, 1.0);
                    for c in 0..3 {
                        let v = f32::from(work[i + c]);
                        let n = if matches!(op, Op::Dodge(_)) {
                            v + (255.0 - v) * e
                        } else {
                            v * (1.0 - e)
                        };
                        work[i + c] = n.round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
        Op::Sponge { saturate } => {
            for y in y0..y1 {
                for x in x0..x1 {
                    let a = cover(x, y).min(1.0);
                    if a <= 0.0 {
                        continue;
                    }
                    let i = (y * w + x) * 4;
                    if work[i + 3] == 0 {
                        continue;
                    }
                    let gray = 0.299 * f32::from(work[i])
                        + 0.587 * f32::from(work[i + 1])
                        + 0.114 * f32::from(work[i + 2]);
                    let k = if saturate { 1.0 + a } else { 1.0 - a };
                    for c in 0..3 {
                        let v = f32::from(work[i + c]);
                        work[i + c] = (gray + (v - gray) * k).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BR: Brush = Brush {
        radius: 6.0,
        hardness: 1.0,
    };

    fn flat(w: usize, h: usize, c: [u8; 4]) -> Vec<u8> {
        (0..w * h).flat_map(|_| c).collect()
    }

    fn set(buf: &mut [u8], w: usize, x: usize, y: usize, c: [u8; 4]) {
        buf[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&c);
    }

    #[test]
    fn walker_spaces_dabs_evenly_across_segments() {
        let mut wk = Walker::new([0.0, 0.0]);
        let a = wk.advance([10.0, 0.0], 4.0);
        let b = wk.advance([20.0, 0.0], 4.0);
        let xs: Vec<f32> = a.iter().chain(&b).map(|p| p[0]).collect();
        assert_eq!(xs, vec![4.0, 8.0, 12.0, 16.0, 20.0]);
        assert!(Walker::new([1.0, 1.0]).advance([1.0, 1.0], 3.0).is_empty());
    }

    #[test]
    fn clone_copies_from_offset_with_brush_falloff_and_selection() {
        let w = 40;
        let mut px = flat(w, 20, [10, 10, 10, 255]);
        for y in 0..20 {
            for x in 25..40 {
                set(&mut px, w, x, y, [200, 100, 50, 255]);
            }
        }
        let snap = px.clone();
        // Origem em x=30 (clara): o centro do toque passa a ser clara.
        let mut dst2 = snap.clone();
        clone_dab(
            &mut dst2,
            &snap,
            w,
            20,
            [10.0, 10.0],
            [20, 0],
            BR,
            1.0,
            None,
        );
        let c = (10 * w + 10) * 4;
        assert_eq!(&dst2[c..c + 4], &[200, 100, 50, 255]);
        assert_eq!(
            &dst2[(10 * w + 25) * 4..(10 * w + 25) * 4 + 4],
            &snap[(10 * w + 25) * 4..(10 * w + 25) * 4 + 4],
            "fora do pincel não muda"
        );
        // Seleção vazia bloqueia; opacidade 0.5 mistura.
        let zero = vec![0u8; w * 20];
        let mut d3 = snap.clone();
        clone_dab(
            &mut d3,
            &snap,
            w,
            20,
            [10.0, 10.0],
            [20, 0],
            BR,
            1.0,
            Some(&zero),
        );
        assert_eq!(d3, snap);
        let mut d4 = snap.clone();
        clone_dab(&mut d4, &snap, w, 20, [10.0, 10.0], [20, 0], BR, 0.5, None);
        assert!((i32::from(d4[c]) - 105).abs() <= 2);
    }

    #[test]
    fn clone_does_not_create_halos_on_transparency() {
        let w = 20;
        let mut px = flat(w, 10, [0, 0, 0, 0]);
        for y in 0..10 {
            for x in 12..20 {
                set(&mut px, w, x, y, [255, 0, 0, 255]);
            }
        }
        let snap = px.clone();
        let mut dst = px.clone();
        clone_dab(
            &mut dst,
            &snap,
            w,
            10,
            [5.0, 5.0],
            [10, 0],
            Brush {
                radius: 4.0,
                hardness: 0.3,
            },
            1.0,
            None,
        );
        for p in dst.as_chunks::<4>().0 {
            if p[3] > 0 {
                assert_eq!(&p[..3], &[255, 0, 0], "{p:?}");
            }
        }
        assert!(dst
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0 && p[3] < 255));
    }

    #[test]
    fn heal_keeps_destination_tone_but_takes_source_texture() {
        // Fundo cinza 100 à esquerda, 160 à direita; textura fina (xadrez ±20) só à direita.
        let (w, h) = (60usize, 30usize);
        let mut px = flat(w, h, [100, 100, 100, 255]);
        for y in 0..h {
            for x in 30..w {
                let v = if (x + y) % 2 == 0 { 140 } else { 180 };
                set(&mut px, w, x, y, [v, v, v, 255]);
            }
        }
        let snap = px.clone();
        let mut dst = snap.clone();
        heal_dab(
            &mut dst,
            &snap,
            w,
            h,
            [10.0, 15.0],
            [30, 0],
            Brush {
                radius: 8.0,
                hardness: 1.0,
            },
            1.0,
            None,
        );
        let vals: Vec<u8> = (8..13).map(|x| dst[(15 * w + x) * 4]).collect();
        let mean = vals.iter().map(|&v| f32::from(v)).sum::<f32>() / 5.0;
        assert!(
            (mean - 100.0).abs() < 12.0,
            "tom do destino mantido: {vals:?}"
        );
        assert!(
            vals.iter().any(|&v| v != vals[0]),
            "textura da origem presente: {vals:?}"
        );
        // Clone puro, em contraste, copiaria o tom 160.
        let mut cl = snap.clone();
        clone_dab(
            &mut cl,
            &snap,
            w,
            h,
            [10.0, 15.0],
            [30, 0],
            Brush {
                radius: 8.0,
                hardness: 1.0,
            },
            1.0,
            None,
        );
        assert!(cl[(15 * w + 10) * 4] > 130);
    }

    #[test]
    fn spot_heal_removes_a_blemish_on_a_gradient() {
        let (w, h) = (80usize, 60usize);
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                set(
                    &mut px,
                    w,
                    x,
                    y,
                    [(60 + x) as u8, (60 + x) as u8, (60 + x) as u8, 255],
                );
            }
        }
        for y in 26..34 {
            for x in 36..44 {
                set(&mut px, w, x, y, [10, 10, 10, 255]);
            }
        }
        assert!(spot_heal(&mut px, w, h, [40.0, 30.0], 8.0, None));
        let c = px[(30 * w + 40) * 4];
        assert!((i32::from(c) - 100).abs() <= 12, "centro refeito: {c}");
        // Imagem minúscula: sem origem possível.
        let mut tiny = flat(6, 6, [5, 5, 5, 255]);
        assert!(!spot_heal(&mut tiny, 6, 6, [3.0, 3.0], 8.0, None));
    }

    #[test]
    fn patch_replaces_selection_with_shifted_texture_and_respects_partial_mask() {
        let (w, h) = (40usize, 20usize);
        let mut px = flat(w, h, [120, 120, 120, 255]);
        for y in 5..15 {
            for x in 5..10 {
                set(&mut px, w, x, y, [0, 0, 0, 255]); // defeito
            }
        }
        let mut sel = vec![0u8; w * h];
        for y in 5..15 {
            for x in 5..10 {
                sel[y * w + x] = 255;
            }
        }
        let n = patch(&mut px, w, h, &sel, [20, 0]);
        assert!(n > 0);
        assert!((i32::from(px[(10 * w + 7) * 4]) - 120).abs() <= 6);
        assert_eq!(px[(10 * w + 20) * 4], 120, "fora da seleção intacto");
        assert_eq!(patch(&mut px, w, h, &vec![0; w * h], [1, 1]), 0);
    }

    #[test]
    fn red_eye_darkens_only_red_pupils() {
        let (w, h) = (30usize, 30usize);
        let mut px = flat(w, h, [90, 60, 40, 255]); // íris marrom
        for y in 0..h {
            for x in 0..w {
                let d = ((x as f32 - 15.0).powi(2) + (y as f32 - 15.0).powi(2)).sqrt();
                if d < 4.0 {
                    set(&mut px, w, x, y, [210, 30, 30, 255]);
                }
            }
        }
        let n = red_eye(&mut px, w, h, [15.0, 15.0], 8.0, 1.0, None);
        assert!(n > 20);
        let c = (15 * w + 15) * 4;
        assert!(
            px[c] <= 40 && px[c + 1] == 30 && px[c + 2] == 30,
            "{:?}",
            &px[c..c + 4]
        );
        assert_eq!(&px[0..4], &[90, 60, 40, 255]);
        // A íris marrom (90,60,40) não é "vermelha" o bastante e não muda.
        let iris = (15 * w + 21) * 4;
        assert_eq!(&px[iris..iris + 4], &[90, 60, 40, 255]);
    }

    #[test]
    fn effect_brushes_do_what_they_say() {
        let (w, h) = (20usize, 20usize);
        let mut sm = Smudge::default();
        // Desfocar suaviza uma linha branca em fundo preto.
        let mut px = flat(w, h, [0, 0, 0, 255]);
        for y in 0..h {
            set(&mut px, w, 10, y, [255, 255, 255, 255]);
        }
        effect_dab(
            Op::Blur,
            &mut px,
            w,
            h,
            [10.0, 10.0],
            BR,
            1.0,
            None,
            &mut sm,
        );
        assert!(px[(10 * w + 10) * 4] < 255 && px[(10 * w + 9) * 4] > 0);
        // Nitidez aumenta o contraste de uma borda.
        let mut e = flat(w, h, [100, 100, 100, 255]);
        for y in 0..h {
            for x in 10..w {
                set(&mut e, w, x, y, [160, 160, 160, 255]);
            }
        }
        effect_dab(
            Op::Sharpen,
            &mut e,
            w,
            h,
            [10.0, 10.0],
            BR,
            1.0,
            None,
            &mut sm,
        );
        assert!(e[(10 * w + 9) * 4] < 100 && e[(10 * w + 10) * 4] > 160);
        // Subexpor clareia; superexpor escurece; esponja dessatura sem mexer no alfa.
        let base = flat(w, h, [120, 100, 80, 200]);
        let mut d = base.clone();
        effect_dab(
            Op::Dodge(Range::Midtones),
            &mut d,
            w,
            h,
            [10.0, 10.0],
            BR,
            1.0,
            None,
            &mut sm,
        );
        assert!(d[(10 * w + 10) * 4] > 120);
        let mut b = base.clone();
        effect_dab(
            Op::Burn(Range::Midtones),
            &mut b,
            w,
            h,
            [10.0, 10.0],
            BR,
            1.0,
            None,
            &mut sm,
        );
        assert!(b[(10 * w + 10) * 4] < 120);
        let mut s = base.clone();
        effect_dab(
            Op::Sponge { saturate: false },
            &mut s,
            w,
            h,
            [10.0, 10.0],
            BR,
            1.0,
            None,
            &mut sm,
        );
        let i = (10 * w + 10) * 4;
        assert!(s[i] == s[i + 1] && s[i + 1] == s[i + 2] && s[i + 3] == 200);
        // Faixa errada não age: realces não mexem em um tom escuro.
        let dark = flat(w, h, [20, 20, 20, 255]);
        let mut dd = dark.clone();
        effect_dab(
            Op::Dodge(Range::Highlights),
            &mut dd,
            w,
            h,
            [10.0, 10.0],
            BR,
            1.0,
            None,
            &mut sm,
        );
        assert_eq!(dd, dark);
    }

    #[test]
    fn smudge_drags_color_along_the_stroke() {
        let (w, h) = (40usize, 20usize);
        let mut px = flat(w, h, [0, 0, 0, 255]);
        for y in 0..h {
            for x in 0..12 {
                set(&mut px, w, x, y, [255, 255, 255, 255]);
            }
        }
        let br = Brush {
            radius: 5.0,
            hardness: 1.0,
        };
        let mut sm = Smudge::default();
        for x in [8.0f32, 11.0, 14.0, 17.0, 20.0, 23.0] {
            effect_dab(Op::Smudge, &mut px, w, h, [x, 10.0], br, 0.9, None, &mut sm);
        }
        assert!(px[(10 * w + 18) * 4] > 60, "cor arrastada para a direita");
        assert_eq!(px[(10 * w + 38) * 4], 0, "longe do traço não muda");
    }
}
