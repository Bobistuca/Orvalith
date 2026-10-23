//! Filtros clássicos sobre um buffer RGBA8 (alfa reto).
//!
//! Desfoques, mediana e redução de ruído trabalham em alfa **pré-multiplicado** (`f32`), para
//! que as bordas de áreas transparentes não ganhem halos escuros ou claros. Cada filtro
//! respeita um `AtomicBool` de cancelamento (devolve `None`) e pode ser limitado por uma
//! máscara de seleção.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Filter {
    GaussianBlur {
        radius: f32,
    },
    BoxBlur {
        radius: f32,
    },
    /// Ângulo em graus; distância em pixels.
    MotionBlur {
        angle: f32,
        distance: f32,
    },
    /// Quantidade em %, raio em px, limiar 0..=255.
    UnsharpMask {
        amount: f32,
        radius: f32,
        threshold: f32,
    },
    Sharpen,
    /// Redução de ruído que preserva bordas (bilateral 5×5); força 0..=100.
    NoiseReduction {
        strength: f32,
    },
    Median {
        radius: u32,
    },
    /// Ruído gaussiano com semente (reprodutível); quantidade 0..=100.
    AddNoise {
        amount: f32,
        mono: bool,
        seed: u32,
    },
    Pixelate {
        size: u32,
    },
    Emboss {
        angle: f32,
        height: f32,
    },
    EdgeDetect,
    Twirl {
        angle: f32,
    },
    Ripple {
        amplitude: f32,
        wavelength: f32,
    },
    Spherize {
        amount: f32,
    },
}

pub const KINDS: [&str; 14] = [
    "Desfoque gaussiano",
    "Desfoque de caixa",
    "Desfoque de movimento",
    "Máscara de nitidez",
    "Nitidez",
    "Reduzir ruído",
    "Mediana",
    "Adicionar ruído",
    "Pixelar",
    "Relevo",
    "Detectar bordas",
    "Redemoinho",
    "Ondulação",
    "Esferizar",
];

impl Filter {
    pub fn neutral(kind: usize) -> Self {
        match kind {
            0 => Self::GaussianBlur { radius: 4.0 },
            1 => Self::BoxBlur { radius: 4.0 },
            2 => Self::MotionBlur {
                angle: 0.0,
                distance: 15.0,
            },
            3 => Self::UnsharpMask {
                amount: 100.0,
                radius: 2.0,
                threshold: 0.0,
            },
            4 => Self::Sharpen,
            5 => Self::NoiseReduction { strength: 50.0 },
            6 => Self::Median { radius: 2 },
            7 => Self::AddNoise {
                amount: 20.0,
                mono: true,
                seed: 1,
            },
            8 => Self::Pixelate { size: 8 },
            9 => Self::Emboss {
                angle: 135.0,
                height: 3.0,
            },
            10 => Self::EdgeDetect,
            11 => Self::Twirl { angle: 120.0 },
            12 => Self::Ripple {
                amplitude: 8.0,
                wavelength: 40.0,
            },
            _ => Self::Spherize { amount: 50.0 },
        }
    }

    pub fn kind_index(&self) -> usize {
        match self {
            Self::GaussianBlur { .. } => 0,
            Self::BoxBlur { .. } => 1,
            Self::MotionBlur { .. } => 2,
            Self::UnsharpMask { .. } => 3,
            Self::Sharpen => 4,
            Self::NoiseReduction { .. } => 5,
            Self::Median { .. } => 6,
            Self::AddNoise { .. } => 7,
            Self::Pixelate { .. } => 8,
            Self::Emboss { .. } => 9,
            Self::EdgeDetect => 10,
            Self::Twirl { .. } => 11,
            Self::Ripple { .. } => 12,
            Self::Spherize { .. } => 13,
        }
    }

    pub fn name(&self) -> &'static str {
        KINDS[self.kind_index()]
    }

    /// Limita os parâmetros às faixas suportadas.
    pub fn sanitize(&mut self) {
        let lim = |v: &mut f32, lo: f32, hi: f32, d: f32| {
            *v = if v.is_finite() { v.clamp(lo, hi) } else { d };
        };
        match self {
            Self::GaussianBlur { radius } => lim(radius, 0.0, 100.0, 4.0),
            Self::BoxBlur { radius } => lim(radius, 0.0, 100.0, 4.0),
            Self::MotionBlur { angle, distance } => {
                lim(angle, -360.0, 360.0, 0.0);
                lim(distance, 0.0, 200.0, 15.0);
            }
            Self::UnsharpMask {
                amount,
                radius,
                threshold,
            } => {
                lim(amount, 0.0, 500.0, 100.0);
                lim(radius, 0.1, 50.0, 2.0);
                lim(threshold, 0.0, 255.0, 0.0);
            }
            Self::NoiseReduction { strength } => lim(strength, 0.0, 100.0, 50.0),
            Self::Median { radius } => *radius = (*radius).clamp(1, 8),
            Self::AddNoise { amount, .. } => lim(amount, 0.0, 100.0, 20.0),
            Self::Pixelate { size } => *size = (*size).clamp(1, 256),
            Self::Emboss { angle, height } => {
                lim(angle, -360.0, 360.0, 135.0);
                lim(height, 0.1, 20.0, 3.0);
            }
            Self::Twirl { angle } => lim(angle, -720.0, 720.0, 0.0),
            Self::Ripple {
                amplitude,
                wavelength,
            } => {
                lim(amplitude, 0.0, 200.0, 8.0);
                lim(wavelength, 4.0, 1000.0, 40.0);
            }
            Self::Spherize { amount } => lim(amount, -100.0, 100.0, 0.0),
            Self::Sharpen | Self::EdgeDetect => {}
        }
    }
}

fn cancelled(c: &AtomicBool) -> bool {
    c.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------------------
// Conversões pré-multiplicadas

fn premul(px: &[u8]) -> Vec<f32> {
    let mut out = Vec::with_capacity(px.len());
    for p in px.as_chunks::<4>().0 {
        let a = f32::from(p[3]);
        let k = a / 255.0;
        out.extend_from_slice(&[
            f32::from(p[0]) * k,
            f32::from(p[1]) * k,
            f32::from(p[2]) * k,
            a,
        ]);
    }
    out
}

fn unpremul(buf: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(buf.len());
    for p in buf.as_chunks::<4>().0 {
        let a = p[3].clamp(0.0, 255.0);
        if a < 0.5 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let k = 255.0 / a;
        out.extend_from_slice(&[
            (p[0] * k + 0.5).clamp(0.0, 255.0) as u8,
            (p[1] * k + 0.5).clamp(0.0, 255.0) as u8,
            (p[2] * k + 0.5).clamp(0.0, 255.0) as u8,
            (a + 0.5) as u8,
        ]);
    }
    out
}

fn luma_u8(p: &[u8]) -> f32 {
    0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2])
}

// ---------------------------------------------------------------------------------------
// Desfoques

fn blur_pass(
    buf: &mut [f32],
    w: usize,
    h: usize,
    r: usize,
    horiz: bool,
    cancel: &AtomicBool,
) -> Option<()> {
    if r == 0 {
        return Some(());
    }
    let (lines, len, lstride, estride) = if horiz {
        (h, w, w * 4, 4)
    } else {
        (w, h, 4, w * 4)
    };
    let mut tmp = vec![0f32; len];
    let win = (2 * r + 1) as f32;
    for l in 0..lines {
        if l % 64 == 0 && cancelled(cancel) {
            return None;
        }
        for c in 0..4 {
            let base = l * lstride + c;
            for (i, t) in tmp.iter_mut().enumerate() {
                *t = buf[base + i * estride];
            }
            let at = |j: isize| tmp[j.clamp(0, len as isize - 1) as usize];
            let ri = r as isize;
            let mut sum: f32 = (-ri..=ri).map(at).sum();
            for i in 0..len {
                buf[base + i * estride] = sum / win;
                sum += at(i as isize + ri + 1) - at(i as isize - ri);
            }
        }
    }
    Some(())
}

fn box_blur(buf: &mut [f32], w: usize, h: usize, r: usize, cancel: &AtomicBool) -> Option<()> {
    blur_pass(buf, w, h, r, true, cancel)?;
    blur_pass(buf, w, h, r, false, cancel)
}

/// Três caixas aproximam um gaussiano de desvio `sigma`.
fn gaussian_blur(
    buf: &mut [f32],
    w: usize,
    h: usize,
    sigma: f32,
    cancel: &AtomicBool,
) -> Option<()> {
    if sigma <= 0.0 {
        return Some(());
    }
    let n = 3.0f32;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n)
        / (-4.0 * wl as f32 - 4.0))
        .round() as i32;
    for i in 0..3 {
        let size = if i < m { wl } else { wu };
        box_blur(buf, w, h, ((size - 1) / 2) as usize, cancel)?;
    }
    Some(())
}

fn motion_blur(
    src: &[f32],
    w: usize,
    h: usize,
    angle: f32,
    dist: f32,
    cancel: &AtomicBool,
) -> Option<Vec<f32>> {
    let steps = dist.round().max(1.0) as i32;
    let (s, c) = angle.to_radians().sin_cos();
    let mut out = vec![0f32; src.len()];
    for y in 0..h {
        if y % 32 == 0 && cancelled(cancel) {
            return None;
        }
        for x in 0..w {
            let mut acc = [0f32; 4];
            for k in 0..steps {
                let t = k as f32 - (steps - 1) as f32 / 2.0;
                let sx = (x as f32 + c * t).round().clamp(0.0, (w - 1) as f32) as usize;
                let sy = (y as f32 - s * t).round().clamp(0.0, (h - 1) as f32) as usize;
                let i = (sy * w + sx) * 4;
                for ch in 0..4 {
                    acc[ch] += src[i + ch];
                }
            }
            let o = (y * w + x) * 4;
            for ch in 0..4 {
                out[o + ch] = acc[ch] / steps as f32;
            }
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------
// Nitidez, mediana, ruído

fn clamp_premul(buf: &mut [f32]) {
    for p in buf.as_chunks_mut::<4>().0 {
        p[3] = p[3].clamp(0.0, 255.0);
        for c in 0..3 {
            p[c] = p[c].clamp(0.0, p[3]);
        }
    }
}

fn unsharp(
    orig: &[f32],
    w: usize,
    h: usize,
    amount: f32,
    radius: f32,
    threshold: f32,
    cancel: &AtomicBool,
) -> Option<Vec<f32>> {
    let mut blurred = orig.to_vec();
    gaussian_blur(&mut blurred, w, h, radius, cancel)?;
    let k = amount / 100.0;
    let mut out = orig.to_vec();
    for (o, b) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(blurred.as_chunks::<4>().0)
    {
        for c in 0..3 {
            let d = o[c] - b[c];
            if d.abs() >= threshold {
                o[c] += k * d;
            }
        }
    }
    clamp_premul(&mut out);
    Some(out)
}

fn sharpen_kernel(src: &[f32], w: usize, h: usize, cancel: &AtomicBool) -> Option<Vec<f32>> {
    let mut out = src.to_vec();
    for y in 0..h {
        if y % 64 == 0 && cancelled(cancel) {
            return None;
        }
        for x in 0..w {
            let at = |dx: isize, dy: isize, c: usize| {
                let xx = (x as isize + dx).clamp(0, w as isize - 1) as usize;
                let yy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
                src[(yy * w + xx) * 4 + c]
            };
            for c in 0..3 {
                out[(y * w + x) * 4 + c] =
                    5.0 * at(0, 0, c) - at(-1, 0, c) - at(1, 0, c) - at(0, -1, c) - at(0, 1, c);
            }
        }
    }
    clamp_premul(&mut out);
    Some(out)
}

fn median(src: &[f32], w: usize, h: usize, r: usize, cancel: &AtomicBool) -> Option<Vec<f32>> {
    let mut out = src.to_vec();
    let mut win: Vec<f32> = Vec::with_capacity((2 * r + 1) * (2 * r + 1));
    for y in 0..h {
        if y % 8 == 0 && cancelled(cancel) {
            return None;
        }
        for x in 0..w {
            for c in 0..4 {
                win.clear();
                for dy in -(r as isize)..=r as isize {
                    let yy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
                    for dx in -(r as isize)..=r as isize {
                        let xx = (x as isize + dx).clamp(0, w as isize - 1) as usize;
                        win.push(src[(yy * w + xx) * 4 + c]);
                    }
                }
                let mid = win.len() / 2;
                win.select_nth_unstable_by(mid, |a, b| a.total_cmp(b));
                out[(y * w + x) * 4 + c] = win[mid];
            }
        }
    }
    Some(out)
}

fn bilateral(
    src: &[f32],
    w: usize,
    h: usize,
    strength: f32,
    cancel: &AtomicBool,
) -> Option<Vec<f32>> {
    let mut out = src.to_vec();
    let sigma_c = 4.0 + strength * 0.8; // desvio de cor, em níveis 0..255
    let inv_c = 1.0 / (2.0 * sigma_c * sigma_c);
    let inv_s = 1.0 / (2.0 * 1.5 * 1.5);
    for y in 0..h {
        if y % 32 == 0 && cancelled(cancel) {
            return None;
        }
        for x in 0..w {
            let ci = (y * w + x) * 4;
            let mut acc = [0f32; 3];
            let mut wsum = 0f32;
            for dy in -2isize..=2 {
                let yy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
                for dx in -2isize..=2 {
                    let xx = (x as isize + dx).clamp(0, w as isize - 1) as usize;
                    let ni = (yy * w + xx) * 4;
                    let d2: f32 = (0..3).map(|c| (src[ni + c] - src[ci + c]).powi(2)).sum();
                    let wt = (-(d2 * inv_c) - ((dx * dx + dy * dy) as f32) * inv_s).exp();
                    for c in 0..3 {
                        acc[c] += wt * src[ni + c];
                    }
                    wsum += wt;
                }
            }
            for c in 0..3 {
                out[ci + c] = acc[c] / wsum;
            }
        }
    }
    clamp_premul(&mut out);
    Some(out)
}

struct Rng(u64);

impl Rng {
    fn new(seed: u32) -> Self {
        Self(u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 40) as f32 / (1u64 << 24) as f32
    }
    /// Aproximação gaussiana (soma de 4 uniformes), média 0, desvio ~1.
    fn gauss(&mut self) -> f32 {
        (self.next() + self.next() + self.next() + self.next() - 2.0) * 1.732
    }
}

fn add_noise(px: &[u8], amount: f32, mono: bool, seed: u32) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    let sigma = amount / 100.0 * 80.0;
    let mut out = px.to_vec();
    for p in out.as_chunks_mut::<4>().0 {
        if p[3] == 0 {
            continue;
        }
        let g = [rng.gauss(), rng.gauss(), rng.gauss()];
        for c in 0..3 {
            let n = if mono { g[0] } else { g[c] } * sigma;
            p[c] = (f32::from(p[c]) + n).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn pixelate(src: &[f32], w: usize, h: usize, size: usize) -> Vec<f32> {
    let mut out = src.to_vec();
    for by in (0..h).step_by(size) {
        for bx in (0..w).step_by(size) {
            let (ex, ey) = ((bx + size).min(w), (by + size).min(h));
            let mut acc = [0f32; 4];
            for y in by..ey {
                for x in bx..ex {
                    for c in 0..4 {
                        acc[c] += src[(y * w + x) * 4 + c];
                    }
                }
            }
            let n = ((ex - bx) * (ey - by)) as f32;
            for y in by..ey {
                for x in bx..ex {
                    for c in 0..4 {
                        out[(y * w + x) * 4 + c] = acc[c] / n;
                    }
                }
            }
        }
    }
    out
}

fn luma_at(px: &[u8], w: usize, h: usize, x: isize, y: isize) -> f32 {
    let xx = x.clamp(0, w as isize - 1) as usize;
    let yy = y.clamp(0, h as isize - 1) as usize;
    luma_u8(&px[(yy * w + xx) * 4..])
}

fn emboss(
    px: &[u8],
    w: usize,
    h: usize,
    angle: f32,
    height: f32,
    cancel: &AtomicBool,
) -> Option<Vec<u8>> {
    let (s, c) = angle.to_radians().sin_cos();
    let (dx, dy) = (c.round() as isize, -s.round() as isize);
    let mut out = px.to_vec();
    for y in 0..h {
        if y % 64 == 0 && cancelled(cancel) {
            return None;
        }
        for x in 0..w {
            let d = luma_at(px, w, h, x as isize + dx, y as isize + dy)
                - luma_at(px, w, h, x as isize - dx, y as isize - dy);
            let v = (128.0 + d * height * 0.5).round().clamp(0.0, 255.0) as u8;
            let i = (y * w + x) * 4;
            out[i..i + 3].copy_from_slice(&[v, v, v]);
        }
    }
    Some(out)
}

fn edge_detect(px: &[u8], w: usize, h: usize, cancel: &AtomicBool) -> Option<Vec<u8>> {
    let mut out = px.to_vec();
    for y in 0..h {
        if y % 64 == 0 && cancelled(cancel) {
            return None;
        }
        for x in 0..w {
            let l = |dx: isize, dy: isize| luma_at(px, w, h, x as isize + dx, y as isize + dy);
            let gx = -l(-1, -1) - 2.0 * l(-1, 0) - l(-1, 1) + l(1, -1) + 2.0 * l(1, 0) + l(1, 1);
            let gy = -l(-1, -1) - 2.0 * l(0, -1) - l(1, -1) + l(-1, 1) + 2.0 * l(0, 1) + l(1, 1);
            let v = (gx * gx + gy * gy).sqrt().round().clamp(0.0, 255.0) as u8;
            let i = (y * w + x) * 4;
            out[i..i + 3].copy_from_slice(&[v, v, v]);
        }
    }
    Some(out)
}

/// Amostra bilinear em alfa pré-multiplicado; fora da imagem = transparente.
fn sample(src: &[f32], w: usize, h: usize, x: f32, y: f32) -> [f32; 4] {
    if x < -0.5 || y < -0.5 || x > w as f32 - 0.5 || y > h as f32 - 0.5 {
        return [0.0; 4];
    }
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let get = |xx: f32, yy: f32| {
        let xi = xx.clamp(0.0, (w - 1) as f32) as usize;
        let yi = yy.clamp(0.0, (h - 1) as f32) as usize;
        let i = (yi * w + xi) * 4;
        [src[i], src[i + 1], src[i + 2], src[i + 3]]
    };
    let (a, b, c, d) = (
        get(x0, y0),
        get(x0 + 1.0, y0),
        get(x0, y0 + 1.0),
        get(x0 + 1.0, y0 + 1.0),
    );
    let mut o = [0f32; 4];
    for i in 0..4 {
        o[i] = (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy;
    }
    o
}

fn warp(
    src: &[f32],
    w: usize,
    h: usize,
    cancel: &AtomicBool,
    map: impl Fn(f32, f32) -> (f32, f32),
) -> Option<Vec<f32>> {
    let mut out = vec![0f32; src.len()];
    for y in 0..h {
        if y % 64 == 0 && cancelled(cancel) {
            return None;
        }
        for x in 0..w {
            let (sx, sy) = map(x as f32, y as f32);
            let p = sample(src, w, h, sx, sy);
            out[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&p);
        }
    }
    Some(out)
}

/// Aplica o filtro a `px` (`w × h`). `None` se o cancelamento foi pedido.
pub fn apply(f: &Filter, px: &[u8], w: u32, h: u32, cancel: &AtomicBool) -> Option<Vec<u8>> {
    let mut f = f.clone();
    f.sanitize();
    let (w, h) = (w as usize, h as usize);
    if px.len() != w * h * 4 || w == 0 || h == 0 {
        return Some(px.to_vec());
    }
    match f {
        Filter::AddNoise { amount, mono, seed } => Some(add_noise(px, amount, mono, seed)),
        Filter::Emboss { angle, height } => emboss(px, w, h, angle, height, cancel),
        Filter::EdgeDetect => edge_detect(px, w, h, cancel),
        other => {
            let src = premul(px);
            let out = match other {
                Filter::GaussianBlur { radius } => {
                    let mut b = src;
                    gaussian_blur(&mut b, w, h, radius, cancel)?;
                    b
                }
                Filter::BoxBlur { radius } => {
                    let mut b = src;
                    box_blur(&mut b, w, h, radius.round() as usize, cancel)?;
                    b
                }
                Filter::MotionBlur { angle, distance } => {
                    motion_blur(&src, w, h, angle, distance, cancel)?
                }
                Filter::UnsharpMask {
                    amount,
                    radius,
                    threshold,
                } => unsharp(&src, w, h, amount, radius, threshold, cancel)?,
                Filter::Sharpen => sharpen_kernel(&src, w, h, cancel)?,
                Filter::NoiseReduction { strength } => bilateral(&src, w, h, strength, cancel)?,
                Filter::Median { radius } => median(&src, w, h, radius as usize, cancel)?,
                Filter::Pixelate { size } => pixelate(&src, w, h, size as usize),
                Filter::Twirl { angle } => {
                    let (cx, cy) = ((w as f32 - 1.0) / 2.0, (h as f32 - 1.0) / 2.0);
                    let r = (w.min(h) as f32) / 2.0;
                    let a = angle.to_radians();
                    warp(&src, w, h, cancel, |x, y| {
                        let (dx, dy) = (x - cx, y - cy);
                        let d = (dx * dx + dy * dy).sqrt();
                        if d >= r {
                            return (x, y);
                        }
                        let t = a * (1.0 - d / r).powi(2);
                        let (s, c) = t.sin_cos();
                        (cx + dx * c - dy * s, cy + dx * s + dy * c)
                    })?
                }
                Filter::Ripple {
                    amplitude,
                    wavelength,
                } => warp(&src, w, h, cancel, |x, y| {
                    (
                        x + amplitude * (std::f32::consts::TAU * y / wavelength).sin(),
                        y,
                    )
                })?,
                Filter::Spherize { amount } => {
                    let (cx, cy) = ((w as f32 - 1.0) / 2.0, (h as f32 - 1.0) / 2.0);
                    let r = (w.min(h) as f32) / 2.0;
                    let k = amount / 100.0;
                    warp(&src, w, h, cancel, |x, y| {
                        let (dx, dy) = (x - cx, y - cy);
                        let d = (dx * dx + dy * dy).sqrt() / r;
                        if d >= 1.0 || d == 0.0 {
                            return (x, y);
                        }
                        // k > 0 estufa (amplia o centro); k < 0 afunda.
                        let nd = if k >= 0.0 {
                            d.powf(1.0 + k)
                        } else {
                            d.powf(1.0 / (1.0 - k))
                        };
                        (cx + dx / d * nd, cy + dy / d * nd)
                    })?
                }
                _ => src,
            };
            Some(unpremul(&out))
        }
    }
}

/// Como [`apply`], mas `sel` (1 byte por pixel) limita o efeito (parcial = mistura).
pub fn apply_selected(
    f: &Filter,
    px: &[u8],
    w: u32,
    h: u32,
    sel: Option<&[u8]>,
    cancel: &AtomicBool,
) -> Option<Vec<u8>> {
    let mut out = apply(f, px, w, h, cancel)?;
    if let Some(m) = sel {
        for (i, (o, s)) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(px.as_chunks::<4>().0)
            .enumerate()
        {
            let wt = u32::from(m.get(i).copied().unwrap_or(0));
            if wt == 255 {
                continue;
            }
            if wt == 0 {
                *o = *s;
                continue;
            }
            *o = mix_px(*s, *o, wt as f32 / 255.0);
        }
    }
    Some(out)
}

/// Mistura dois pixels retos de forma correta para o alfa (via pré-multiplicado).
pub fn mix_px(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let (aa, ba) = (f32::from(a[3]) / 255.0, f32::from(b[3]) / 255.0);
    let oa = aa + (ba - aa) * t;
    if oa <= 0.0 {
        return [0, 0, 0, 0];
    }
    let mut o = [0u8; 4];
    for c in 0..3 {
        let pa = f32::from(a[c]) * aa;
        let pb = f32::from(b[c]) * ba;
        o[c] = ((pa + (pb - pa) * t) / oa + 0.5).clamp(0.0, 255.0) as u8;
    }
    o[3] = (oa * 255.0 + 0.5) as u8;
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    static NO: AtomicBool = AtomicBool::new(false);

    fn flat(w: u32, h: u32, c: [u8; 4]) -> Vec<u8> {
        (0..w * h).flat_map(|_| c).collect()
    }

    /// Quadrado vermelho opaco no centro de uma tela transparente.
    fn red_square() -> Vec<u8> {
        let mut v = flat(16, 16, [0, 0, 0, 0]);
        for y in 4..12 {
            for x in 4..12 {
                let i = (y * 16 + x) * 4;
                v[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
            }
        }
        v
    }

    fn all_kinds() -> Vec<Filter> {
        (0..KINDS.len()).map(Filter::neutral).collect()
    }

    #[test]
    fn flat_opaque_image_is_stable_under_smoothing_filters() {
        let px = flat(12, 9, [90, 140, 200, 255]);
        for f in [
            Filter::GaussianBlur { radius: 3.0 },
            Filter::BoxBlur { radius: 2.0 },
            Filter::MotionBlur {
                angle: 30.0,
                distance: 9.0,
            },
            Filter::UnsharpMask {
                amount: 150.0,
                radius: 2.0,
                threshold: 0.0,
            },
            Filter::Sharpen,
            Filter::NoiseReduction { strength: 80.0 },
            Filter::Median { radius: 2 },
            Filter::Pixelate { size: 5 },
        ] {
            let out = apply(&f, &px, 12, 9, &NO).unwrap();
            for (a, b) in out.iter().zip(&px) {
                assert!(
                    (i32::from(*a) - i32::from(*b)).abs() <= 1,
                    "{}: {a} vs {b}",
                    f.name()
                );
            }
        }
    }

    #[test]
    fn blurring_over_transparency_makes_no_dark_or_light_halos() {
        let px = red_square();
        for f in [
            Filter::GaussianBlur { radius: 2.0 },
            Filter::BoxBlur { radius: 2.0 },
            Filter::MotionBlur {
                angle: 0.0,
                distance: 5.0,
            },
            Filter::Median { radius: 1 },
        ] {
            let out = apply(&f, &px, 16, 16, &NO).unwrap();
            let mut partial = 0;
            for p in out.as_chunks::<4>().0 {
                if p[3] > 8 {
                    // A cor continua vermelha pura onde há alfa significativo (sem halo).
                    assert!(p[0] >= 250 && p[1] <= 4 && p[2] <= 4, "{}: {p:?}", f.name());
                }
                if p[3] > 0 && p[3] < 255 {
                    partial += 1;
                }
            }
            if !matches!(f, Filter::Median { .. }) {
                assert!(partial > 0, "{} deveria suavizar a borda", f.name());
            }
        }
    }

    #[test]
    fn gaussian_blur_preserves_total_energy_and_known_impulse_spread() {
        let mut px = flat(21, 21, [0, 0, 0, 255]);
        let c = (10 * 21 + 10) * 4;
        px[c..c + 3].copy_from_slice(&[255, 255, 255]);
        let out = apply(&Filter::GaussianBlur { radius: 2.0 }, &px, 21, 21, &NO).unwrap();
        let sum: u32 = out.as_chunks::<4>().0.iter().map(|p| u32::from(p[0])).sum();
        assert!((i64::from(sum) - 255).abs() <= 20, "{sum}");
        assert!(out[c] < 255 && out[c] > out[c + 4 * 3]);
        // Simétrico em x.
        assert!((i32::from(out[c - 8]) - i32::from(out[c + 8])).abs() <= 1);
    }

    #[test]
    fn unsharp_and_sharpen_increase_edge_contrast_and_keep_alpha() {
        let mut px = flat(16, 8, [100, 100, 100, 255]);
        for y in 0..8 {
            for x in 8..16 {
                let i = (y * 16 + x) * 4;
                px[i..i + 3].copy_from_slice(&[160, 160, 160]);
            }
        }
        for f in [
            Filter::UnsharpMask {
                amount: 200.0,
                radius: 2.0,
                threshold: 0.0,
            },
            Filter::Sharpen,
        ] {
            let out = apply(&f, &px, 16, 8, &NO).unwrap();
            let (dark, bright) = (out[(3 * 16 + 7) * 4], out[(3 * 16 + 8) * 4]);
            assert!(dark < 100 && bright > 160, "{}: {dark} {bright}", f.name());
            assert!(out.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        }
    }

    #[test]
    fn median_removes_salt_noise_and_noise_is_seeded() {
        let mut px = flat(9, 9, [50, 50, 50, 255]);
        let c = (4 * 9 + 4) * 4;
        px[c..c + 3].copy_from_slice(&[255, 255, 255]);
        let out = apply(&Filter::Median { radius: 1 }, &px, 9, 9, &NO).unwrap();
        assert_eq!(&out[c..c + 3], &[50, 50, 50]);
        let n = Filter::AddNoise {
            amount: 30.0,
            mono: false,
            seed: 7,
        };
        let a = apply(&n, &px, 9, 9, &NO).unwrap();
        let b = apply(&n, &px, 9, 9, &NO).unwrap();
        assert_eq!(a, b, "mesma semente = mesmo ruído");
        let other = apply(
            &Filter::AddNoise {
                amount: 30.0,
                mono: false,
                seed: 8,
            },
            &px,
            9,
            9,
            &NO,
        )
        .unwrap();
        assert_ne!(a, other);
        // Pixels transparentes não recebem ruído (sem halos).
        let t = flat(4, 4, [0, 0, 0, 0]);
        assert_eq!(apply(&n, &t, 4, 4, &NO).unwrap(), t);
    }

    #[test]
    fn noise_reduction_smooths_grain_but_keeps_a_strong_edge() {
        let mut px = flat(16, 8, [60, 60, 60, 255]);
        for y in 0..8 {
            for x in 8..16 {
                px[(y * 16 + x) * 4..(y * 16 + x) * 4 + 3].copy_from_slice(&[200, 200, 200]);
            }
        }
        let noisy = apply(
            &Filter::AddNoise {
                amount: 15.0,
                mono: true,
                seed: 3,
            },
            &px,
            16,
            8,
            &NO,
        )
        .unwrap();
        let var = |b: &[u8]| -> f32 {
            let v: Vec<f32> = (0..8)
                .flat_map(|y| (1..7).map(move |x| (y, x)))
                .map(|(y, x)| f32::from(b[(y * 16 + x) * 4]))
                .collect();
            let m = v.iter().sum::<f32>() / v.len() as f32;
            v.iter().map(|a| (a - m).powi(2)).sum::<f32>() / v.len() as f32
        };
        let den = apply(
            &Filter::NoiseReduction { strength: 60.0 },
            &noisy,
            16,
            8,
            &NO,
        )
        .unwrap();
        assert!(var(&den) < var(&noisy));
        assert!(
            den[(3 * 16 + 5) * 4] < 120 && den[(3 * 16 + 10) * 4] > 140,
            "borda preservada"
        );
    }

    #[test]
    fn pixelate_edge_emboss_and_distortions() {
        let mut px = flat(8, 8, [0, 0, 0, 255]);
        px[..4].copy_from_slice(&[255, 255, 255, 255]);
        let p = apply(&Filter::Pixelate { size: 4 }, &px, 8, 8, &NO).unwrap();
        assert_eq!(&p[0..3], &p[4 * 3..4 * 3 + 3]);
        assert!((i32::from(p[0]) - 16).abs() <= 1); // 255 / 16 pixels
        let flat_px = flat(8, 8, [90, 90, 90, 255]);
        let e = apply(&Filter::EdgeDetect, &flat_px, 8, 8, &NO).unwrap();
        assert!(e.as_chunks::<4>().0.iter().all(|q| q[0] == 0));
        let em = apply(
            &Filter::Emboss {
                angle: 135.0,
                height: 3.0,
            },
            &flat_px,
            8,
            8,
            &NO,
        )
        .unwrap();
        assert!(em.as_chunks::<4>().0.iter().all(|q| q[0] == 128));
        let ident = flat(9, 9, [10, 20, 30, 255]);
        for f in [
            Filter::Twirl { angle: 0.0 },
            Filter::Ripple {
                amplitude: 0.0,
                wavelength: 20.0,
            },
            Filter::Spherize { amount: 0.0 },
        ] {
            assert_eq!(apply(&f, &ident, 9, 9, &NO).unwrap(), ident, "{}", f.name());
        }
        let rotated = apply(&Filter::Twirl { angle: 90.0 }, &red_square(), 16, 16, &NO).unwrap();
        assert_ne!(rotated, red_square());
    }

    #[test]
    fn cancel_returns_none_and_selection_limits_the_effect() {
        let stop = AtomicBool::new(true);
        let px = red_square();
        for f in all_kinds() {
            if matches!(f, Filter::AddNoise { .. } | Filter::Pixelate { .. }) {
                continue; // curtos/sem pontos de cancelamento próprios
            }
            assert!(apply(&f, &px, 16, 16, &stop).is_none(), "{}", f.name());
        }
        let mut sel = vec![0u8; 256];
        for y in 0..16 {
            for x in 0..8 {
                sel[y * 16 + x] = 255;
            }
        }
        let f = Filter::Pixelate { size: 8 };
        let base = apply(&f, &px, 16, 16, &NO).unwrap();
        let part = apply_selected(&f, &px, 16, 16, Some(&sel), &NO).unwrap();
        for y in 0..16 {
            for x in 0..16 {
                let i = (y * 16 + x) * 4;
                let want = if x < 8 {
                    &base[i..i + 4]
                } else {
                    &px[i..i + 4]
                };
                assert_eq!(&part[i..i + 4], want);
            }
        }
    }

    #[test]
    fn hostile_parameters_are_sanitized() {
        let px = flat(6, 6, [1, 2, 3, 255]);
        for f in [
            Filter::GaussianBlur { radius: f32::NAN },
            Filter::Median { radius: 9999 },
            Filter::Pixelate { size: 0 },
            Filter::MotionBlur {
                angle: f32::INFINITY,
                distance: 1e9,
            },
        ] {
            assert_eq!(apply(&f, &px, 6, 6, &NO).unwrap().len(), px.len());
        }
        assert_eq!(
            apply(&Filter::Sharpen, &px[..8], 6, 6, &NO).unwrap(),
            px[..8].to_vec()
        );
    }
}
