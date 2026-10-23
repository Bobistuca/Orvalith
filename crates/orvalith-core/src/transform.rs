//! Transformações geométricas e reamostragem de imagens RGBA8.
//!
//! Convenções: o pixel (i, j) ocupa [i, i+1) × [j, j+1) e seu centro é (i+0,5, j+0,5).
//! Toda interpolação ocorre em alfa **pré-multiplicado**, então bordas translúcidas e
//! transformações não criam halos de cor; fora da imagem a amostra é transparente
//! (as bordas ficam suavizadas, exceto com vizinho mais próximo).

/// Método de interpolação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interp {
    /// Vizinho mais próximo: sem suavização (pixel art).
    Nearest,
    Bilinear,
    /// Cúbica de Catmull-Rom (fotos).
    Bicubic,
}

impl Interp {
    pub const ALL: [Interp; 3] = [Interp::Nearest, Interp::Bilinear, Interp::Bicubic];

    pub fn label(self) -> &'static str {
        match self {
            Interp::Nearest => "Vizinho mais próximo (pixel art)",
            Interp::Bilinear => "Bilinear",
            Interp::Bicubic => "Bicúbica (fotos)",
        }
    }
}

/// Transformação afim: x' = a·x + c·y + e ; y' = b·x + d·y + f.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn translate(dx: f32, dy: f32) -> Self {
        Self {
            e: dx,
            f: dy,
            ..Self::IDENTITY
        }
    }

    /// Escala em torno de `pivot`.
    pub fn scale(sx: f32, sy: f32, pivot: (f32, f32)) -> Self {
        Self {
            a: sx,
            d: sy,
            e: pivot.0 * (1.0 - sx),
            f: pivot.1 * (1.0 - sy),
            ..Self::IDENTITY
        }
    }

    /// Rotação (graus, sentido horário na tela com y para baixo) em torno de `pivot`.
    pub fn rotate(deg: f32, pivot: (f32, f32)) -> Self {
        let (s, c) = deg.to_radians().sin_cos();
        Self {
            a: c,
            b: s,
            c: -s,
            d: c,
            e: pivot.0 - c * pivot.0 + s * pivot.1,
            f: pivot.1 - s * pivot.0 - c * pivot.1,
        }
    }

    /// Inclinação (graus) horizontal e vertical em torno de `pivot`.
    pub fn skew(kx_deg: f32, ky_deg: f32, pivot: (f32, f32)) -> Self {
        let (tx, ty) = (kx_deg.to_radians().tan(), ky_deg.to_radians().tan());
        Self {
            a: 1.0,
            b: ty,
            c: tx,
            d: 1.0,
            e: -tx * pivot.1,
            f: -ty * pivot.0,
        }
    }

    pub fn flip_horizontal(center_x: f32) -> Self {
        Self::scale(-1.0, 1.0, (center_x, 0.0))
    }

    pub fn flip_vertical(center_y: f32) -> Self {
        Self::scale(1.0, -1.0, (0.0, center_y))
    }

    /// Aplica `self` e depois `next`.
    pub fn then(self, next: Affine) -> Affine {
        Affine {
            a: next.a * self.a + next.c * self.b,
            b: next.b * self.a + next.d * self.b,
            c: next.a * self.c + next.c * self.d,
            d: next.b * self.c + next.d * self.d,
            e: next.a * self.e + next.c * self.f + next.e,
            f: next.b * self.e + next.d * self.f + next.f,
        }
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    pub fn invert(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-9 || !det.is_finite() {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Affine {
            a,
            b,
            c,
            d,
            e: -(a * self.e + c * self.f),
            f: -(b * self.e + d * self.f),
        })
    }
}

/// Transformação projetiva (3×3, linha a linha).
#[derive(Debug, Clone, Copy)]
pub struct Homography(pub [f64; 9]);

impl Homography {
    /// Homografia que leva os 4 pontos `src` nos 4 pontos `dst` (None se degenerada).
    pub fn from_quads(src: [(f32, f32); 4], dst: [(f32, f32); 4]) -> Option<Self> {
        // Sistema 8×8 pelo método DLT, resolvido por eliminação de Gauss com pivoteamento.
        let mut m = [[0f64; 9]; 8];
        for i in 0..4 {
            let (x, y) = (f64::from(src[i].0), f64::from(src[i].1));
            let (u, v) = (f64::from(dst[i].0), f64::from(dst[i].1));
            m[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            m[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        for col in 0..8 {
            let piv = (col..8).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
            if m[piv][col].abs() < 1e-9 {
                return None;
            }
            m.swap(col, piv);
            for r in 0..8 {
                if r != col {
                    let k = m[r][col] / m[col][col];
                    let pivot_row = m[col];
                    for (c, item) in m[r].iter_mut().enumerate().skip(col) {
                        *item -= k * pivot_row[c];
                    }
                }
            }
        }
        let mut h = [0f64; 9];
        for i in 0..8 {
            h[i] = m[i][8] / m[i][i];
        }
        h[8] = 1.0;
        h.iter().all(|v| v.is_finite()).then_some(Homography(h))
    }

    pub fn apply(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let h = &self.0;
        let (x, y) = (f64::from(x), f64::from(y));
        let w = h[6] * x + h[7] * y + h[8];
        if w.abs() < 1e-12 {
            return None;
        }
        Some((
            ((h[0] * x + h[1] * y + h[2]) / w) as f32,
            ((h[3] * x + h[4] * y + h[5]) / w) as f32,
        ))
    }
}

// ---- Amostragem ----

#[inline]
fn texel(src: &[u8], w: i64, h: i64, x: i64, y: i64) -> [f32; 4] {
    if x < 0 || y < 0 || x >= w || y >= h {
        return [0.0; 4];
    }
    let i = (y * w + x) as usize * 4;
    let a = f32::from(src[i + 3]) / 255.0;
    [
        f32::from(src[i]) / 255.0 * a,
        f32::from(src[i + 1]) / 255.0 * a,
        f32::from(src[i + 2]) / 255.0 * a,
        a,
    ]
}

fn cubic_weights(t: f32) -> [f32; 4] {
    let (t2, t3) = (t * t, t * t * t);
    [
        -0.5 * t3 + t2 - 0.5 * t,
        1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
        0.5 * t3 - 0.5 * t2,
    ]
}

/// Amostra em (x, y) contínuos; devolve RGBA pré-multiplicado em 0..=1.
pub fn sample(src: &[u8], w: u32, h: u32, x: f32, y: f32, interp: Interp) -> [f32; 4] {
    let (wi, hi) = (i64::from(w), i64::from(h));
    match interp {
        Interp::Nearest => texel(src, wi, hi, x.floor() as i64, y.floor() as i64),
        Interp::Bilinear => {
            let (fx, fy) = (x - 0.5, y - 0.5);
            let (x0, y0) = (fx.floor(), fy.floor());
            let (tx, ty) = (fx - x0, fy - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let mut out = [0f32; 4];
            for (dy, wy) in [(0, 1.0 - ty), (1, ty)] {
                for (dx, wx) in [(0, 1.0 - tx), (1, tx)] {
                    let t = texel(src, wi, hi, x0 + dx, y0 + dy);
                    for c in 0..4 {
                        out[c] += t[c] * wx * wy;
                    }
                }
            }
            out
        }
        Interp::Bicubic => {
            let (fx, fy) = (x - 0.5, y - 0.5);
            let (x0, y0) = (fx.floor(), fy.floor());
            let (wx, wy) = (cubic_weights(fx - x0), cubic_weights(fy - y0));
            let (x0, y0) = (x0 as i64, y0 as i64);
            let mut out = [0f32; 4];
            for (j, wyj) in wy.iter().enumerate() {
                for (i, wxi) in wx.iter().enumerate() {
                    let t = texel(src, wi, hi, x0 + i as i64 - 1, y0 + j as i64 - 1);
                    for c in 0..4 {
                        out[c] += t[c] * wxi * wyj;
                    }
                }
            }
            // Evita sobressinal: alfa em 0..1 e cor pré-multiplicada em 0..alfa.
            out[3] = out[3].clamp(0.0, 1.0);
            for c in 0..3 {
                out[c] = out[c].clamp(0.0, out[3]);
            }
            out
        }
    }
}

/// Converte RGBA pré-multiplicado (0..=1) em RGBA8 não pré-multiplicado.
pub fn to_rgba8(p: [f32; 4]) -> [u8; 4] {
    let a = p[3].clamp(0.0, 1.0);
    if a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let u = |v: f32| ((v / a).clamp(0.0, 1.0) * 255.0).round() as u8;
    [u(p[0]), u(p[1]), u(p[2]), (a * 255.0).round() as u8]
}

/// Preenche `out` em linhas, em paralelo.
fn par_rows(out: &mut [u8], row_bytes: usize, f: &(dyn Fn(usize, &mut [u8]) + Sync)) {
    if row_bytes == 0 || out.is_empty() {
        return;
    }
    let rows = out.len() / row_bytes;
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(8)
        .min(rows.max(1));
    let per = rows.div_ceil(threads);
    std::thread::scope(|s| {
        for (t, chunk) in out.chunks_mut(per * row_bytes).enumerate() {
            s.spawn(move || {
                for (r, row) in chunk.chunks_mut(row_bytes).enumerate() {
                    f(t * per + r, row);
                }
            });
        }
    });
}

/// Mapeamento inverso genérico: `inv` leva o centro de cada pixel de destino ao ponto de origem.
pub fn warp(
    src: &[u8],
    w: u32,
    h: u32,
    out_w: u32,
    out_h: u32,
    interp: Interp,
    inv: &(dyn Fn(f32, f32) -> Option<(f32, f32)> + Sync),
) -> Vec<u8> {
    let mut out = vec![0u8; out_w as usize * out_h as usize * 4];
    par_rows(&mut out, out_w as usize * 4, &|y, row| {
        for x in 0..out_w as usize {
            if let Some((sx, sy)) = inv(x as f32 + 0.5, y as f32 + 0.5) {
                let px = to_rgba8(sample(src, w, h, sx, sy, interp));
                row[x * 4..x * 4 + 4].copy_from_slice(&px);
            }
        }
    });
    out
}

/// Aplica `m` (origem → destino) à imagem; o destino tem `out_w × out_h` pixels.
pub fn warp_affine(
    src: &[u8],
    w: u32,
    h: u32,
    m: &Affine,
    out_w: u32,
    out_h: u32,
    interp: Interp,
) -> Option<Vec<u8>> {
    let inv = m.invert()?;
    Some(warp(src, w, h, out_w, out_h, interp, &move |x, y| {
        Some(inv.apply(x, y))
    }))
}

/// Leva o quadrilátero `src_quad` da imagem para `dst_quad` (distorcer/perspectiva).
pub fn warp_quad(
    src: &[u8],
    w: u32,
    h: u32,
    src_quad: [(f32, f32); 4],
    dst_quad: [(f32, f32); 4],
    interp: Interp,
) -> Option<Vec<u8>> {
    let back = Homography::from_quads(dst_quad, src_quad)?;
    Some(warp(src, w, h, w, h, interp, &move |x, y| back.apply(x, y)))
}

/// Malha de deformação: grade `cols × rows` de pontos de controle. `src_pts` e `dst_pts`
/// têm `cols*rows` pontos (linha a linha). Cada célula vira dois triângulos mapeados por
/// transformações afins (aproximação; as bordas externas não são suavizadas).
#[allow(clippy::too_many_arguments)]
pub fn warp_mesh(
    src: &[u8],
    w: u32,
    h: u32,
    cols: usize,
    rows: usize,
    src_pts: &[(f32, f32)],
    dst_pts: &[(f32, f32)],
    interp: Interp,
) -> Option<Vec<u8>> {
    if cols < 2 || rows < 2 || src_pts.len() != cols * rows || dst_pts.len() != cols * rows {
        return None;
    }
    let mut out = vec![0u8; w as usize * h as usize * 4];
    let tris = |i: usize, j: usize| {
        let (p00, p10, p01, p11) = (
            j * cols + i,
            j * cols + i + 1,
            (j + 1) * cols + i,
            (j + 1) * cols + i + 1,
        );
        [[p00, p10, p01], [p10, p11, p01]]
    };
    for j in 0..rows - 1 {
        for i in 0..cols - 1 {
            for t in tris(i, j) {
                let d = [dst_pts[t[0]], dst_pts[t[1]], dst_pts[t[2]]];
                let s = [src_pts[t[0]], src_pts[t[1]], src_pts[t[2]]];
                // Afim destino -> origem a partir dos três vértices.
                let Some(dst_to_src) = affine_from_triangles(d, s) else {
                    continue;
                };
                let (minx, maxx) = (
                    d.iter()
                        .map(|p| p.0)
                        .fold(f32::INFINITY, f32::min)
                        .floor()
                        .max(0.0) as i64,
                    (d.iter()
                        .map(|p| p.0)
                        .fold(f32::NEG_INFINITY, f32::max)
                        .ceil() as i64)
                        .min(i64::from(w)),
                );
                let (miny, maxy) = (
                    d.iter()
                        .map(|p| p.1)
                        .fold(f32::INFINITY, f32::min)
                        .floor()
                        .max(0.0) as i64,
                    (d.iter()
                        .map(|p| p.1)
                        .fold(f32::NEG_INFINITY, f32::max)
                        .ceil() as i64)
                        .min(i64::from(h)),
                );
                for y in miny..maxy {
                    for x in minx..maxx {
                        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                        if !in_triangle(d, px, py, 0.02) {
                            continue;
                        }
                        let (sx, sy) = dst_to_src.apply(px, py);
                        let c = to_rgba8(sample(src, w, h, sx, sy, interp));
                        let o = (y as usize * w as usize + x as usize) * 4;
                        out[o..o + 4].copy_from_slice(&c);
                    }
                }
            }
        }
    }
    Some(out)
}

fn affine_from_triangles(from: [(f32, f32); 3], to: [(f32, f32); 3]) -> Option<Affine> {
    // Resolve [x y 1] * M = [u v] para três vértices.
    let (x0, y0) = from[0];
    let (x1, y1) = from[1];
    let (x2, y2) = from[2];
    let det = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
    if det.abs() < 1e-9 {
        return None;
    }
    let solve = |v0: f32, v1: f32, v2: f32| {
        let a = ((v1 - v0) * (y2 - y0) - (v2 - v0) * (y1 - y0)) / det;
        let c = ((v2 - v0) * (x1 - x0) - (v1 - v0) * (x2 - x0)) / det;
        (a, c, v0 - a * x0 - c * y0)
    };
    let (a, c, e) = solve(to[0].0, to[1].0, to[2].0);
    let (b, d, f) = solve(to[0].1, to[1].1, to[2].1);
    Some(Affine { a, b, c, d, e, f })
}

fn in_triangle(t: [(f32, f32); 3], px: f32, py: f32, eps: f32) -> bool {
    let edge = |a: (f32, f32), b: (f32, f32)| (b.0 - a.0) * (py - a.1) - (b.1 - a.1) * (px - a.0);
    let (e0, e1, e2) = (edge(t[0], t[1]), edge(t[1], t[2]), edge(t[2], t[0]));
    let len = |a: (f32, f32), b: (f32, f32)| ((b.0 - a.0).hypot(b.1 - a.1)).max(1e-6);
    let (n0, n1, n2) = (
        e0 / len(t[0], t[1]),
        e1 / len(t[1], t[2]),
        e2 / len(t[2], t[0]),
    );
    (n0 >= -eps && n1 >= -eps && n2 >= -eps) || (n0 <= eps && n1 <= eps && n2 <= eps)
}

// ---- Reamostragem (redimensionar) ----

fn kernel(interp: Interp, x: f32) -> f32 {
    let x = x.abs();
    match interp {
        Interp::Nearest => {
            if x < 0.5 {
                1.0
            } else {
                0.0
            }
        }
        Interp::Bilinear => (1.0 - x).max(0.0),
        Interp::Bicubic => {
            // Catmull-Rom (a = -0,5).
            if x < 1.0 {
                1.5 * x * x * x - 2.5 * x * x + 1.0
            } else if x < 2.0 {
                -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
            } else {
                0.0
            }
        }
    }
}

/// Pesos por pixel de saída (início, pesos normalizados) com suporte alargado ao reduzir.
fn axis_weights(interp: Interp, src_len: usize, dst_len: usize) -> Vec<(usize, Vec<f32>)> {
    let scale = src_len as f32 / dst_len as f32;
    let filter_scale = scale.max(1.0);
    let support = match interp {
        Interp::Nearest => 0.5,
        Interp::Bilinear => 1.0,
        Interp::Bicubic => 2.0,
    } * filter_scale;
    (0..dst_len)
        .map(|i| {
            let center = (i as f32 + 0.5) * scale;
            let lo = ((center - support).floor() as i64).max(0) as usize;
            let hi = ((center + support).ceil() as i64).min(src_len as i64) as usize;
            let mut ws: Vec<f32> = (lo..hi)
                .map(|j| kernel(interp, (j as f32 + 0.5 - center) / filter_scale))
                .collect();
            let sum: f32 = ws.iter().sum();
            if sum.abs() < 1e-9 {
                // Nearest com janela vazia: usa o pixel mais próximo.
                let j = (center.floor() as i64).clamp(0, src_len as i64 - 1) as usize;
                return (j, vec![1.0]);
            }
            ws.iter_mut().for_each(|w| *w /= sum);
            (lo, ws)
        })
        .collect()
}

/// Redimensiona uma imagem RGBA8 (separável, alfa pré-multiplicado, filtro alargado ao reduzir).
pub fn resample(src: &[u8], w: u32, h: u32, new_w: u32, new_h: u32, interp: Interp) -> Vec<u8> {
    let (w, h, nw, nh) = (w as usize, h as usize, new_w as usize, new_h as usize);
    if w == 0 || h == 0 || nw == 0 || nh == 0 {
        return vec![0; nw * nh * 4];
    }
    let xw = axis_weights(interp, w, nw);
    let yw = axis_weights(interp, h, nh);
    // Passo horizontal: h linhas × nw colunas em float pré-multiplicado.
    let mut mid = vec![0f32; nw * h * 4];
    for y in 0..h {
        let row = &src[y * w * 4..(y + 1) * w * 4];
        for (x, (lo, ws)) in xw.iter().enumerate() {
            let mut acc = [0f32; 4];
            for (k, wt) in ws.iter().enumerate() {
                let i = (lo + k) * 4;
                let a = f32::from(row[i + 3]) / 255.0;
                acc[0] += f32::from(row[i]) / 255.0 * a * wt;
                acc[1] += f32::from(row[i + 1]) / 255.0 * a * wt;
                acc[2] += f32::from(row[i + 2]) / 255.0 * a * wt;
                acc[3] += a * wt;
            }
            mid[(y * nw + x) * 4..(y * nw + x) * 4 + 4].copy_from_slice(&acc);
        }
    }
    let mut out = vec![0u8; nw * nh * 4];
    for (y, (lo, ws)) in yw.iter().enumerate() {
        for x in 0..nw {
            let mut acc = [0f32; 4];
            for (k, wt) in ws.iter().enumerate() {
                let i = ((lo + k) * nw + x) * 4;
                for c in 0..4 {
                    acc[c] += mid[i + c] * wt;
                }
            }
            acc[3] = acc[3].clamp(0.0, 1.0);
            for c in 0..3 {
                acc[c] = acc[c].clamp(0.0, acc[3]);
            }
            out[(y * nw + x) * 4..(y * nw + x) * 4 + 4].copy_from_slice(&to_rgba8(acc));
        }
    }
    out
}

/// Reamostra uma máscara de 1 canal (seleções, canais).
pub fn resample_mask(mask: &[u8], w: u32, h: u32, new_w: u32, new_h: u32) -> Vec<u8> {
    let rgba: Vec<u8> = mask.iter().flat_map(|&v| [v, v, v, 255]).collect();
    resample(&rgba, w, h, new_w, new_h, Interp::Bilinear)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[0])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&[(x * 20) as u8, (y * 20) as u8, ((x + y) * 5) as u8, 255]);
            }
        }
        v
    }

    fn at(v: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [v[i], v[i + 1], v[i + 2], v[i + 3]]
    }

    #[test]
    fn identity_is_exact_for_every_interpolation() {
        let src = gradient(8, 6);
        for interp in Interp::ALL {
            let out = warp_affine(&src, 8, 6, &Affine::IDENTITY, 8, 6, interp).unwrap();
            assert_eq!(out, src, "{interp:?}");
        }
    }

    #[test]
    fn integer_translation_is_exact_and_clips() {
        let src = gradient(8, 6);
        for interp in Interp::ALL {
            let out = warp_affine(&src, 8, 6, &Affine::translate(3.0, 2.0), 8, 6, interp).unwrap();
            assert_eq!(at(&out, 8, 3, 2), at(&src, 8, 0, 0), "{interp:?}");
            assert_eq!(at(&out, 8, 7, 5), at(&src, 8, 4, 3));
            assert_eq!(at(&out, 8, 0, 0), [0, 0, 0, 0]);
            // Deslocamento negativo: conteúdo sai pela esquerda.
            let neg = warp_affine(&src, 8, 6, &Affine::translate(-2.0, 0.0), 8, 6, interp).unwrap();
            assert_eq!(at(&neg, 8, 0, 0), at(&src, 8, 2, 0));
            assert_eq!(at(&neg, 8, 7, 0), [0, 0, 0, 0]);
        }
    }

    #[test]
    fn flips_and_quarter_turns_are_pixel_exact() {
        let src = gradient(6, 6);
        let fh = warp_affine(
            &src,
            6,
            6,
            &Affine::flip_horizontal(3.0),
            6,
            6,
            Interp::Bilinear,
        )
        .unwrap();
        assert_eq!(at(&fh, 6, 0, 2), at(&src, 6, 5, 2));
        let fv = warp_affine(
            &src,
            6,
            6,
            &Affine::flip_vertical(3.0),
            6,
            6,
            Interp::Bicubic,
        )
        .unwrap();
        assert_eq!(at(&fv, 6, 2, 0), at(&src, 6, 2, 5));
        let r = warp_affine(
            &src,
            6,
            6,
            &Affine::rotate(90.0, (3.0, 3.0)),
            6,
            6,
            Interp::Nearest,
        )
        .unwrap();
        // Rotação horária: (x, y) -> (5-y, x).
        assert_eq!(at(&r, 6, 5, 0), at(&src, 6, 0, 0));
        assert_eq!(at(&r, 6, 0, 0), at(&src, 6, 0, 5));
        let r4 = warp_affine(
            &src,
            6,
            6,
            &Affine::rotate(90.0, (3.0, 3.0)),
            6,
            6,
            Interp::Bilinear,
        )
        .unwrap();
        assert_eq!(at(&r4, 6, 5, 0), at(&src, 6, 0, 0));
    }

    #[test]
    fn nearest_upscale_makes_blocks_and_scale_composes() {
        let src = gradient(4, 4);
        let out = warp_affine(
            &src,
            4,
            4,
            &Affine::scale(2.0, 2.0, (0.0, 0.0)),
            8,
            8,
            Interp::Nearest,
        )
        .unwrap();
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(at(&out, 8, x, y), at(&src, 4, x / 2, y / 2));
            }
        }
        let m = Affine::scale(2.0, 2.0, (0.0, 0.0)).then(Affine::translate(1.0, 0.0));
        assert_eq!(m.apply(1.0, 1.0), (3.0, 2.0));
        let inv = m.invert().unwrap();
        let (x, y) = inv.apply(3.0, 2.0);
        assert!((x - 1.0).abs() < 1e-6 && (y - 1.0).abs() < 1e-6);
        assert!(Affine::scale(0.0, 1.0, (0.0, 0.0)).invert().is_none());
    }

    #[test]
    fn translucent_edges_do_not_get_color_halos() {
        // Quadrado vermelho semi-transparente sobre fundo transparente (RGB preto nos vazios).
        let mut src = vec![0u8; 12 * 12 * 4];
        for y in 3..9 {
            for x in 3..9 {
                let i = (y * 12 + x) * 4;
                src[i..i + 4].copy_from_slice(&[255, 0, 0, 128]);
            }
        }
        for interp in [Interp::Bilinear, Interp::Bicubic] {
            let m = Affine::rotate(30.0, (6.0, 6.0)).then(Affine::translate(0.5, 0.3));
            let out = warp_affine(&src, 12, 12, &m, 12, 12, interp).unwrap();
            let mut partial = 0;
            for px in out.as_chunks::<4>().0 {
                if px[3] > 0 {
                    assert!(
                        px[0] >= 254 && px[1] <= 1 && px[2] <= 1,
                        "{interp:?} halo: {px:?}"
                    );
                    if px[3] < 128 {
                        partial += 1;
                    }
                }
            }
            assert!(partial > 5, "bordas suavizadas ({interp:?})");
        }
        // Vizinho mais próximo não cria meios-tons: só 0 ou 128.
        let out = warp_affine(
            &src,
            12,
            12,
            &Affine::rotate(30.0, (6.0, 6.0)),
            12,
            12,
            Interp::Nearest,
        )
        .unwrap();
        assert!(out
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] == 0 || p[3] == 128));
    }

    #[test]
    fn rotation_by_45_degrees_keeps_area_roughly() {
        let mut src = vec![0u8; 40 * 40 * 4];
        for y in 10..30 {
            for x in 10..30 {
                src[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4].copy_from_slice(&[0, 255, 0, 255]);
            }
        }
        let out = warp_affine(
            &src,
            40,
            40,
            &Affine::rotate(45.0, (20.0, 20.0)),
            40,
            40,
            Interp::Bilinear,
        )
        .unwrap();
        let area: f32 = out
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| f32::from(p[3]) / 255.0)
            .sum();
        assert!((area - 400.0).abs() < 12.0, "{area}");
        assert_eq!(at(&out, 40, 20, 20), [0, 255, 0, 255]);
        assert_eq!(at(&out, 40, 11, 11)[3], 0, "o canto original ficou vazio");
    }

    #[test]
    fn skew_shears_rows_proportionally() {
        let src = gradient(10, 10);
        let m = Affine::skew(45.0, 0.0, (0.0, 5.0));
        // y=5 é o pivô (sem deslocamento); y=7 desloca x em +2.
        let p = m.apply(0.0, 7.0);
        assert!((p.0 - 2.0).abs() < 1e-5 && (p.1 - 7.0).abs() < 1e-5);
        let out = warp_affine(&src, 10, 10, &m, 10, 10, Interp::Nearest).unwrap();
        assert_eq!(at(&out, 10, 5, 5), at(&src, 10, 5, 5));
        assert_eq!(at(&out, 10, 5, 7), at(&src, 10, 3, 7));
    }

    #[test]
    fn homography_identity_shift_and_perspective() {
        let q = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let h = Homography::from_quads(q, q).unwrap();
        let (x, y) = h.apply(3.0, 7.0).unwrap();
        assert!((x - 3.0).abs() < 1e-4 && (y - 7.0).abs() < 1e-4);
        let src = gradient(10, 10);
        let same = warp_quad(&src, 10, 10, q, q, Interp::Bilinear).unwrap();
        assert_eq!(same, src);
        let shifted = [(2.0, 1.0), (12.0, 1.0), (12.0, 11.0), (2.0, 11.0)];
        let s = warp_quad(&src, 10, 10, q, shifted, Interp::Nearest).unwrap();
        assert_eq!(at(&s, 10, 5, 4), at(&src, 10, 3, 3));
        // Perspectiva: topo estreito (trapézio).
        let solid = vec![255u8; 20 * 20 * 4];
        let q20 = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
        let trap = [(6.0, 0.0), (14.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
        let p = warp_quad(&solid, 20, 20, q20, trap, Interp::Bilinear).unwrap();
        assert_eq!(at(&p, 20, 10, 10)[3], 255);
        assert_eq!(
            at(&p, 20, 1, 2)[3],
            0,
            "canto superior esquerdo ficou vazio"
        );
        assert_eq!(at(&p, 20, 2, 18)[3], 255);
        // Degenerada: pontos colineares.
        let line = [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
        assert!(Homography::from_quads(q, line).is_none());
    }

    #[test]
    fn mesh_identity_and_translation() {
        let src = gradient(12, 12);
        let (cols, rows) = (4, 4);
        let pts: Vec<(f32, f32)> = (0..rows)
            .flat_map(|j| (0..cols).map(move |i| (i as f32 * 4.0, j as f32 * 4.0)))
            .collect();
        let same = warp_mesh(&src, 12, 12, cols, rows, &pts, &pts, Interp::Bilinear).unwrap();
        assert_eq!(same, src);
        let moved: Vec<(f32, f32)> = pts.iter().map(|p| (p.0 + 1.0, p.1)).collect();
        let m = warp_mesh(&src, 12, 12, cols, rows, &pts, &moved, Interp::Nearest).unwrap();
        assert_eq!(at(&m, 12, 5, 6), at(&src, 12, 4, 6));
        assert_eq!(at(&m, 12, 0, 6)[3], 0);
        assert!(warp_mesh(&src, 12, 12, 1, 4, &pts, &pts, Interp::Nearest).is_none());
    }

    #[test]
    fn resample_dimensions_averages_and_nearest() {
        // Xadrez 1×1 reduzido pela metade vira cinza médio.
        let mut chk = Vec::new();
        for y in 0..8u32 {
            for x in 0..8u32 {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                chk.extend_from_slice(&[v, v, v, 255]);
            }
        }
        for interp in [Interp::Bilinear, Interp::Bicubic] {
            let small = resample(&chk, 8, 8, 4, 4, interp);
            assert_eq!(small.len(), 4 * 4 * 4);
            let v = at(&small, 4, 1, 1)[0];
            assert!((120..=136).contains(&v), "{interp:?} {v}");
        }
        let nn = resample(&chk, 8, 8, 16, 16, Interp::Nearest);
        assert_eq!(at(&nn, 16, 0, 0), at(&chk, 8, 0, 0));
        assert_eq!(at(&nn, 16, 3, 0), at(&chk, 8, 1, 0));
        assert_eq!(resample(&chk, 8, 8, 8, 8, Interp::Bicubic), chk);
    }

    #[test]
    fn resample_does_not_bleed_color_from_transparent_pixels() {
        // Metade esquerda: preto totalmente transparente; direita: branco opaco.
        let mut src = Vec::new();
        for _y in 0..8 {
            for x in 0..8 {
                if x < 4 {
                    src.extend_from_slice(&[0, 0, 0, 0]);
                } else {
                    src.extend_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        for interp in Interp::ALL {
            let out = resample(&src, 8, 8, 4, 4, interp);
            for px in out.as_chunks::<4>().0 {
                if px[3] > 0 {
                    assert!(px[0] >= 253, "{interp:?}: {px:?}");
                }
            }
        }
    }

    #[test]
    fn resample_mask_keeps_range() {
        let m = vec![255u8; 16];
        assert!(resample_mask(&m, 4, 4, 2, 2).iter().all(|&v| v == 255));
        assert_eq!(resample_mask(&[0u8; 16], 4, 4, 8, 8).len(), 64);
    }
}
