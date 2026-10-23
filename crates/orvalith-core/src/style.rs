//! Estilos de camada (sombra, brilho, traço, sobreposições, chanfro) e padrões.
//!
//! Os efeitos são calculados a partir do alfa do conteúdo da camada (já com máscara e
//! preenchimento) numa região com margem, e mesclados com o modo e a opacidade de cada um.
//! Aproximações documentadas: bordas pelo limiar de 0,5 do alfa (traço), chanfro por campo de
//! altura suavizado, desfoque por três caixas.

use serde::{Deserialize, Serialize};

use crate::adjust::{gradient_at, GradStop};
use crate::blend::BlendMode;
use crate::compose::composite;

/// Padrões procedurais (sem arquivos externos).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Pattern {
    Checker { a: [u8; 3], b: [u8; 3] },
    Stripes { a: [u8; 3], b: [u8; 3], angle: f32 },
    Dots { a: [u8; 3], b: [u8; 3] },
}

impl Pattern {
    pub const NAMES: [&'static str; 3] = ["Xadrez", "Listras", "Pontos"];

    pub fn index(&self) -> usize {
        match self {
            Pattern::Checker { .. } => 0,
            Pattern::Stripes { .. } => 1,
            Pattern::Dots { .. } => 2,
        }
    }

    pub fn default_of(i: usize) -> Self {
        let (a, b) = ([30, 30, 30], [230, 230, 230]);
        match i {
            0 => Pattern::Checker { a, b },
            1 => Pattern::Stripes { a, b, angle: 45.0 },
            _ => Pattern::Dots { a, b },
        }
    }

    /// Cor no ponto (x, y) do documento; `scale` é o tamanho da célula em pixels.
    pub fn color_at(&self, x: f32, y: f32, scale: f32) -> [u8; 3] {
        let s = scale.max(1.0);
        match *self {
            Pattern::Checker { a, b } => {
                if ((x / s).floor() as i64 + (y / s).floor() as i64) % 2 == 0 {
                    a
                } else {
                    b
                }
            }
            Pattern::Stripes { a, b, angle } => {
                let (sn, cs) = angle.to_radians().sin_cos();
                let t = x * cs + y * sn;
                if ((t / s).floor() as i64) % 2 == 0 {
                    a
                } else {
                    b
                }
            }
            Pattern::Dots { a, b } => {
                let (fx, fy) = (x / s - (x / s).floor() - 0.5, y / s - (y / s).floor() - 0.5);
                if fx * fx + fy * fy <= 0.16 {
                    a
                } else {
                    b
                }
            }
        }
    }
}

/// Posição do traço em relação à borda.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokePos {
    Outside,
    Inside,
    Center,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EffectKind {
    DropShadow {
        color: [u8; 3],
        opacity: f32,
        angle: f32,
        distance: f32,
        spread: f32,
        size: f32,
        blend: BlendMode,
    },
    InnerShadow {
        color: [u8; 3],
        opacity: f32,
        angle: f32,
        distance: f32,
        spread: f32,
        size: f32,
        blend: BlendMode,
    },
    OuterGlow {
        color: [u8; 3],
        opacity: f32,
        spread: f32,
        size: f32,
        blend: BlendMode,
    },
    InnerGlow {
        color: [u8; 3],
        opacity: f32,
        spread: f32,
        size: f32,
        blend: BlendMode,
    },
    Stroke {
        color: [u8; 3],
        opacity: f32,
        size: f32,
        position: StrokePos,
        blend: BlendMode,
    },
    ColorOverlay {
        color: [u8; 3],
        opacity: f32,
        blend: BlendMode,
    },
    GradientOverlay {
        stops: Vec<GradStop>,
        angle: f32,
        reverse: bool,
        opacity: f32,
        blend: BlendMode,
    },
    PatternOverlay {
        pattern: Pattern,
        scale: f32,
        opacity: f32,
        blend: BlendMode,
    },
    Bevel {
        depth: f32,
        size: f32,
        angle: f32,
        highlight: [u8; 3],
        shadow: [u8; 3],
        opacity: f32,
    },
}

pub const KINDS: [&str; 9] = [
    "Sombra projetada",
    "Sombra interna",
    "Brilho externo",
    "Brilho interno",
    "Traço",
    "Sobreposição de cor",
    "Sobreposição de gradiente",
    "Sobreposição de padrão",
    "Chanfro e relevo",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub enabled: bool,
    pub kind: EffectKind,
}

impl EffectKind {
    pub fn neutral(kind: usize) -> Self {
        let n = BlendMode::Normal;
        match kind {
            0 => Self::DropShadow {
                color: [0, 0, 0],
                opacity: 0.75,
                angle: 120.0,
                distance: 8.0,
                spread: 0.0,
                size: 8.0,
                blend: BlendMode::Multiply,
            },
            1 => Self::InnerShadow {
                color: [0, 0, 0],
                opacity: 0.75,
                angle: 120.0,
                distance: 5.0,
                spread: 0.0,
                size: 6.0,
                blend: BlendMode::Multiply,
            },
            2 => Self::OuterGlow {
                color: [255, 240, 150],
                opacity: 0.75,
                spread: 0.0,
                size: 12.0,
                blend: BlendMode::Screen,
            },
            3 => Self::InnerGlow {
                color: [255, 240, 150],
                opacity: 0.75,
                spread: 0.0,
                size: 10.0,
                blend: BlendMode::Screen,
            },
            4 => Self::Stroke {
                color: [255, 0, 0],
                opacity: 1.0,
                size: 3.0,
                position: StrokePos::Outside,
                blend: n,
            },
            5 => Self::ColorOverlay {
                color: [255, 0, 0],
                opacity: 1.0,
                blend: n,
            },
            6 => Self::GradientOverlay {
                stops: vec![
                    GradStop {
                        pos: 0.0,
                        color: [0, 0, 0],
                    },
                    GradStop {
                        pos: 1.0,
                        color: [255, 255, 255],
                    },
                ],
                angle: 90.0,
                reverse: false,
                opacity: 1.0,
                blend: n,
            },
            7 => Self::PatternOverlay {
                pattern: Pattern::default_of(0),
                scale: 8.0,
                opacity: 1.0,
                blend: n,
            },
            _ => Self::Bevel {
                depth: 60.0,
                size: 6.0,
                angle: 120.0,
                highlight: [255, 255, 255],
                shadow: [0, 0, 0],
                opacity: 0.75,
            },
        }
    }

    pub fn kind_index(&self) -> usize {
        match self {
            Self::DropShadow { .. } => 0,
            Self::InnerShadow { .. } => 1,
            Self::OuterGlow { .. } => 2,
            Self::InnerGlow { .. } => 3,
            Self::Stroke { .. } => 4,
            Self::ColorOverlay { .. } => 5,
            Self::GradientOverlay { .. } => 6,
            Self::PatternOverlay { .. } => 7,
            Self::Bevel { .. } => 8,
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
            Self::DropShadow {
                opacity,
                angle,
                distance,
                spread,
                size,
                ..
            }
            | Self::InnerShadow {
                opacity,
                angle,
                distance,
                spread,
                size,
                ..
            } => {
                lim(opacity, 0.0, 1.0, 0.75);
                lim(angle, -360.0, 360.0, 120.0);
                lim(distance, 0.0, 200.0, 5.0);
                lim(spread, 0.0, 1.0, 0.0);
                lim(size, 0.0, 100.0, 5.0);
            }
            Self::OuterGlow {
                opacity,
                spread,
                size,
                ..
            }
            | Self::InnerGlow {
                opacity,
                spread,
                size,
                ..
            } => {
                lim(opacity, 0.0, 1.0, 0.75);
                lim(spread, 0.0, 1.0, 0.0);
                lim(size, 0.0, 100.0, 10.0);
            }
            Self::Stroke { opacity, size, .. } => {
                lim(opacity, 0.0, 1.0, 1.0);
                lim(size, 0.0, 100.0, 3.0);
            }
            Self::ColorOverlay { opacity, .. } => lim(opacity, 0.0, 1.0, 1.0),
            Self::GradientOverlay {
                stops,
                angle,
                opacity,
                ..
            } => {
                lim(opacity, 0.0, 1.0, 1.0);
                lim(angle, -360.0, 360.0, 90.0);
                stops.truncate(32);
                for s in stops.iter_mut() {
                    lim(&mut s.pos, 0.0, 1.0, 0.0);
                }
                stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
                if stops.len() < 2 {
                    *stops = vec![
                        GradStop {
                            pos: 0.0,
                            color: [0, 0, 0],
                        },
                        GradStop {
                            pos: 1.0,
                            color: [255, 255, 255],
                        },
                    ];
                }
            }
            Self::PatternOverlay {
                pattern,
                scale,
                opacity,
                ..
            } => {
                lim(opacity, 0.0, 1.0, 1.0);
                lim(scale, 1.0, 512.0, 8.0);
                if let Pattern::Stripes { angle, .. } = pattern {
                    lim(angle, -360.0, 360.0, 45.0);
                }
            }
            Self::Bevel {
                depth,
                size,
                angle,
                opacity,
                ..
            } => {
                lim(depth, 1.0, 100.0, 60.0);
                lim(size, 1.0, 50.0, 6.0);
                lim(angle, -360.0, 360.0, 120.0);
                lim(opacity, 0.0, 1.0, 0.75);
            }
        }
    }

    /// Alcance do efeito além da borda do conteúdo, em pixels.
    fn reach(&self) -> f32 {
        match self {
            Self::DropShadow { distance, size, .. } | Self::InnerShadow { distance, size, .. } => {
                distance + size + 2.0
            }
            Self::OuterGlow { size, .. }
            | Self::InnerGlow { size, .. }
            | Self::Stroke { size, .. }
            | Self::Bevel { size, .. } => size + 2.0,
            _ => 0.0,
        }
    }
}

impl Effect {
    pub fn new(kind: EffectKind) -> Self {
        Self {
            enabled: true,
            kind,
        }
    }
}

/// Conjunto de estilos com nome (predefinição do usuário).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StylePreset {
    pub name: String,
    pub effects: Vec<Effect>,
}

/// Os parâmetros respeitam os limites (leitura de arquivo).
pub fn effects_sane(list: &[Effect]) -> bool {
    list.len() <= 16
        && list.iter().all(|e| {
            let mut k = e.kind.clone();
            k.sanitize();
            k == e.kind
        })
}

/// Margem (px) a somar ao redor de uma região para que os efeitos fiquem corretos nela.
pub fn reach(list: &[Effect]) -> u32 {
    list.iter()
        .filter(|e| e.enabled)
        .map(|e| e.kind.reach())
        .fold(0.0f32, f32::max)
        .ceil() as u32
}

/// Algum efeito precisa da caixa envolvente do conteúdo (gradiente)?
pub fn needs_bounds(list: &[Effect]) -> bool {
    list.iter()
        .any(|e| e.enabled && matches!(e.kind, EffectKind::GradientOverlay { .. }))
}

// ---------------------------------------------------------------------------------------
// Primitivas sobre máscaras f32

fn blur_pass(buf: &mut [f32], w: usize, h: usize, r: usize, horiz: bool) {
    if r == 0 {
        return;
    }
    let (lines, len, ls, es) = if horiz { (h, w, w, 1) } else { (w, h, 1, w) };
    let mut tmp = vec![0f32; len];
    let win = (2 * r + 1) as f32;
    let ri = r as isize;
    for l in 0..lines {
        for (i, t) in tmp.iter_mut().enumerate() {
            *t = buf[l * ls + i * es];
        }
        let at = |j: isize| tmp[j.clamp(0, len as isize - 1) as usize];
        let mut sum: f32 = (-ri..=ri).map(at).sum();
        for i in 0..len {
            buf[l * ls + i * es] = sum / win;
            sum += at(i as isize + ri + 1) - at(i as isize - ri);
        }
    }
}

/// Desfoque gaussiano aproximado por três caixas (bordas replicadas).
pub fn blur_f32(buf: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma <= 0.3 {
        return;
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
        let r = ((size - 1) / 2) as usize;
        blur_pass(buf, w, h, r, true);
        blur_pass(buf, w, h, r, false);
    }
}

fn shifted(src: &[f32], w: usize, h: usize, dx: i32, dy: i32) -> Vec<f32> {
    if dx == 0 && dy == 0 {
        return src.to_vec();
    }
    let mut out = vec![0f32; src.len()];
    for y in 0..h {
        let sy = (y as i32 - dy).clamp(0, h as i32 - 1) as usize;
        for x in 0..w {
            let sx = (x as i32 - dx).clamp(0, w as i32 - 1) as usize;
            out[y * w + x] = src[sy * w + sx];
        }
    }
    out
}

/// "Espalhar/contrair": curva que alarga o halo suave (0 = nada, 1 = borda dura).
fn spread_curve(v: f32, spread: f32) -> f32 {
    let t = (0.5 * (1.0 - spread)).max(0.01);
    (v / (2.0 * t)).clamp(0.0, 1.0)
}

/// Distância (chanfro 3-4) até o pixel `true` mais próximo; 0 nos próprios pixels `true`.
fn distance_to(set: &[bool], w: usize, h: usize) -> Vec<f32> {
    let inf = 1e9f32;
    let mut d: Vec<f32> = set.iter().map(|&s| if s { 0.0 } else { inf }).collect();
    let (a, b) = (1.0f32, std::f32::consts::SQRT_2);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1] + a);
            }
            if y > 0 {
                v = v.min(d[i - w] + a);
                if x > 0 {
                    v = v.min(d[i - w - 1] + b);
                }
                if x + 1 < w {
                    v = v.min(d[i - w + 1] + b);
                }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut v = d[i];
            if x + 1 < w {
                v = v.min(d[i + 1] + a);
            }
            if y + 1 < h {
                v = v.min(d[i + w] + a);
                if x + 1 < w {
                    v = v.min(d[i + w + 1] + b);
                }
                if x > 0 {
                    v = v.min(d[i + w - 1] + b);
                }
            }
            d[i] = v;
        }
    }
    d
}

/// Parâmetro de gradiente (0..=1) de um ponto em relação a um retângulo e ângulo.
pub fn gradient_t(x: f32, y: f32, rect: (f32, f32, f32, f32), angle: f32, radial: bool) -> f32 {
    let (rx, ry, rw, rh) = rect;
    let (cx, cy) = (rx + rw / 2.0, ry + rh / 2.0);
    if radial {
        let r = ((rw * rw + rh * rh).sqrt() / 2.0).max(1e-3);
        return (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / r).clamp(0.0, 1.0);
    }
    let (s, c) = angle.to_radians().sin_cos();
    let len = (rw * c.abs() + rh * s.abs()).max(1e-3);
    (((x - cx) * c + (y - cy) * s) / len + 0.5).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------------------
// Composição dos efeitos

/// Região de trabalho: `(x, y)` no documento e tamanho.
#[derive(Clone, Copy)]
pub struct Work {
    pub x: u32,
    pub y: u32,
    pub w: usize,
    pub h: usize,
}

fn blend_cov(
    canvas: &mut [u8],
    work: Work,
    cov: &[f32],
    color: impl Fn(usize, usize) -> [u8; 3],
    mode: BlendMode,
    opacity: f32,
    clip: Option<&[f32]>,
) {
    for y in 0..work.h {
        for x in 0..work.w {
            let c = cov[y * work.w + x];
            if c <= 0.002 {
                continue;
            }
            let rgb = color(x, y);
            let src = [
                rgb[0],
                rgb[1],
                rgb[2],
                (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            ];
            let i = (y * work.w + x) * 4;
            let dst = [canvas[i], canvas[i + 1], canvas[i + 2], canvas[i + 3]];
            if clip.is_some_and(|a| a[y * work.w + x] <= 0.0) {
                continue;
            }
            let mut out = composite(
                dst,
                src,
                mode,
                opacity,
                work.x + x as u32,
                work.y + y as u32,
            );
            if clip.is_some() {
                out[3] = dst[3]; // efeito recortado ao conteúdo: o alfa não cresce
            }
            canvas[i..i + 4].copy_from_slice(&out);
        }
    }
}

/// Aplica os estilos ao conteúdo `content` (RGBA reto da região de trabalho) e devolve a
/// região com efeitos + conteúdo. `bounds` é a caixa do conteúdo no documento (para gradientes).
pub fn apply(list: &[Effect], content: &[u8], work: Work, bounds: (f32, f32, f32, f32)) -> Vec<u8> {
    let (w, h) = (work.w, work.h);
    let alpha: Vec<f32> = content
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| f32::from(p[3]) / 255.0)
        .collect();
    let mut canvas = vec![0u8; content.len()];
    let ones = vec![1.0f32; w * h];
    let active: Vec<&Effect> = list.iter().filter(|e| e.enabled).collect();
    let below = |k: &EffectKind| {
        matches!(
            k,
            EffectKind::DropShadow { .. } | EffectKind::OuterGlow { .. }
        )
    };
    // 1. efeitos por baixo do conteúdo
    for e in active.iter().filter(|e| below(&e.kind)) {
        match &e.kind {
            EffectKind::DropShadow {
                color,
                opacity,
                angle,
                distance,
                spread,
                size,
                blend,
            } => {
                let (s, c) = angle.to_radians().sin_cos();
                let mut cov = shifted(
                    &alpha,
                    w,
                    h,
                    (-c * distance).round() as i32,
                    (s * distance).round() as i32,
                );
                blur_f32(&mut cov, w, h, size / 2.0);
                for (v, a) in cov.iter_mut().zip(&alpha) {
                    *v = spread_curve(*v, *spread) * (1.0 - a); // o conteúdo "recorta" a sombra
                }
                blend_cov(
                    &mut canvas,
                    work,
                    &cov,
                    |_, _| *color,
                    *blend,
                    *opacity,
                    None,
                );
            }
            EffectKind::OuterGlow {
                color,
                opacity,
                spread,
                size,
                blend,
            } => {
                let mut cov = alpha.clone();
                blur_f32(&mut cov, w, h, size / 2.0);
                for (v, a) in cov.iter_mut().zip(&alpha) {
                    *v = spread_curve(*v, *spread) * (1.0 - a);
                }
                blend_cov(
                    &mut canvas,
                    work,
                    &cov,
                    |_, _| *color,
                    *blend,
                    *opacity,
                    None,
                );
            }
            _ => {}
        }
    }
    // 2. o conteúdo
    for (i, p) in content.as_chunks::<4>().0.iter().enumerate() {
        if p[3] == 0 {
            continue;
        }
        let dst = [
            canvas[i * 4],
            canvas[i * 4 + 1],
            canvas[i * 4 + 2],
            canvas[i * 4 + 3],
        ];
        let out = composite(dst, *p, BlendMode::Normal, 1.0, 0, 0);
        canvas[i * 4..i * 4 + 4].copy_from_slice(&out);
    }
    // 3. efeitos por cima (recortados ao alfa do conteúdo, exceto o traço)
    for e in active.iter().filter(|e| !below(&e.kind)) {
        match &e.kind {
            EffectKind::InnerShadow {
                color,
                opacity,
                angle,
                distance,
                spread,
                size,
                blend,
            } => {
                let inv: Vec<f32> = alpha.iter().map(|a| 1.0 - a).collect();
                let (s, c) = angle.to_radians().sin_cos();
                let mut cov = shifted(
                    &inv,
                    w,
                    h,
                    (c * distance).round() as i32,
                    (-s * distance).round() as i32,
                );
                blur_f32(&mut cov, w, h, size / 2.0);
                for v in cov.iter_mut() {
                    *v = spread_curve(*v, *spread);
                }
                blend_cov(
                    &mut canvas,
                    work,
                    &cov,
                    |_, _| *color,
                    *blend,
                    *opacity,
                    Some(&alpha),
                );
            }
            EffectKind::InnerGlow {
                color,
                opacity,
                spread,
                size,
                blend,
            } => {
                let mut cov: Vec<f32> = alpha.iter().map(|a| 1.0 - a).collect();
                blur_f32(&mut cov, w, h, size / 2.0);
                for v in cov.iter_mut() {
                    *v = spread_curve(*v, *spread);
                }
                blend_cov(
                    &mut canvas,
                    work,
                    &cov,
                    |_, _| *color,
                    *blend,
                    *opacity,
                    Some(&alpha),
                );
            }
            EffectKind::Stroke {
                color,
                opacity,
                size,
                position,
                blend,
            } => {
                let inside: Vec<bool> = alpha.iter().map(|a| *a >= 0.5).collect();
                let outside: Vec<bool> = inside.iter().map(|b| !b).collect();
                let d_out = distance_to(&inside, w, h); // distância a partir da forma
                let d_in = distance_to(&outside, w, h); // distância a partir do fundo
                let (so, si) = match position {
                    StrokePos::Outside => (*size, 0.0),
                    StrokePos::Inside => (0.0, *size),
                    StrokePos::Center => (size / 2.0, size / 2.0),
                };
                let cov: Vec<f32> = (0..w * h)
                    .map(|i| {
                        if inside[i] {
                            if si > 0.0 {
                                (si - d_in[i] + 1.0).clamp(0.0, 1.0)
                            } else {
                                0.0
                            }
                        } else if so > 0.0 {
                            (so - d_out[i] + 1.0).clamp(0.0, 1.0)
                        } else {
                            0.0
                        }
                    })
                    .collect();
                blend_cov(
                    &mut canvas,
                    work,
                    &cov,
                    |_, _| *color,
                    *blend,
                    *opacity,
                    None,
                );
            }
            EffectKind::ColorOverlay {
                color,
                opacity,
                blend,
            } => blend_cov(
                &mut canvas,
                work,
                &ones,
                |_, _| *color,
                *blend,
                *opacity,
                Some(&alpha),
            ),
            EffectKind::GradientOverlay {
                stops,
                angle,
                reverse,
                opacity,
                blend,
            } => {
                let color = |x: usize, y: usize| {
                    let t = gradient_t(
                        (work.x as usize + x) as f32 + 0.5,
                        (work.y as usize + y) as f32 + 0.5,
                        bounds,
                        *angle,
                        false,
                    );
                    let c = gradient_at(stops, if *reverse { 1.0 - t } else { t });
                    c.map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
                };
                blend_cov(
                    &mut canvas,
                    work,
                    &ones,
                    color,
                    *blend,
                    *opacity,
                    Some(&alpha),
                );
            }
            EffectKind::PatternOverlay {
                pattern,
                scale,
                opacity,
                blend,
            } => {
                let color = |x: usize, y: usize| {
                    pattern.color_at(
                        (work.x as usize + x) as f32 + 0.5,
                        (work.y as usize + y) as f32 + 0.5,
                        *scale,
                    )
                };
                blend_cov(
                    &mut canvas,
                    work,
                    &ones,
                    color,
                    *blend,
                    *opacity,
                    Some(&alpha),
                );
            }
            EffectKind::Bevel {
                depth,
                size,
                angle,
                highlight,
                shadow,
                opacity,
            } => {
                let mut hgt = alpha.clone();
                blur_f32(&mut hgt, w, h, size / 2.0);
                let (s, c) = angle.to_radians().sin_cos();
                let (lx, ly) = (-c, s); // direção da luz (de onde vem), no espaço da imagem
                let k = depth / 100.0 * 3.0;
                let mut hl = vec![0f32; w * h];
                let mut sh = vec![0f32; w * h];
                for y in 0..h {
                    for x in 0..w {
                        let at = |xx: isize, yy: isize| {
                            hgt[yy.clamp(0, h as isize - 1) as usize * w
                                + xx.clamp(0, w as isize - 1) as usize]
                        };
                        let gx = at(x as isize + 1, y as isize) - at(x as isize - 1, y as isize);
                        let gy = at(x as isize, y as isize + 1) - at(x as isize, y as isize - 1);
                        // Face voltada para a luz clareia; a oposta escurece.
                        let lit = -(gx * lx + gy * ly) * k * size.max(1.0) / 2.0;
                        let i = y * w + x;
                        hl[i] = lit.clamp(0.0, 1.0);
                        sh[i] = (-lit).clamp(0.0, 1.0);
                    }
                }
                blend_cov(
                    &mut canvas,
                    work,
                    &hl,
                    |_, _| *highlight,
                    BlendMode::Normal,
                    *opacity,
                    Some(&alpha),
                );
                blend_cov(
                    &mut canvas,
                    work,
                    &sh,
                    |_, _| *shadow,
                    BlendMode::Normal,
                    *opacity,
                    Some(&alpha),
                );
            }
            _ => {}
        }
    }
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quadrado opaco 10×10 no centro de uma região 40×40.
    fn square() -> (Vec<u8>, Work) {
        let (w, h) = (40usize, 40usize);
        let mut c = vec![0u8; w * h * 4];
        for y in 15..25 {
            for x in 15..25 {
                c[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[200, 100, 50, 255]);
            }
        }
        (c, Work { x: 0, y: 0, w, h })
    }

    fn px(buf: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * w + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    fn one(kind: usize) -> Vec<Effect> {
        let mut k = EffectKind::neutral(kind);
        k.sanitize();
        vec![Effect::new(k)]
    }

    #[test]
    fn no_effects_returns_the_content() {
        let (c, w) = square();
        assert_eq!(apply(&[], &c, w, (0.0, 0.0, 40.0, 40.0)), c);
        let mut off = one(0);
        off[0].enabled = false;
        assert_eq!(apply(&off, &c, w, (0.0, 0.0, 40.0, 40.0)), c);
    }

    #[test]
    fn drop_shadow_falls_down_right_outside_and_never_over_the_content() {
        let (c, w) = square();
        let out = apply(&one(0), &c, w, (15.0, 15.0, 10.0, 10.0));
        // Conteúdo intacto.
        assert_eq!(px(&out, 40, 20, 20), [200, 100, 50, 255]);
        // Sombra à direita e abaixo; nada à esquerda/acima, longe.
        let right = px(&out, 40, 27, 22);
        assert!(right[3] > 40 && right[0] < 60, "{right:?}");
        assert_eq!(px(&out, 40, 10, 10)[3], 0);
        let below = px(&out, 40, 22, 28);
        assert!(below[3] > 40);
    }

    #[test]
    fn outer_glow_surrounds_and_inner_glow_stays_inside() {
        let (c, w) = square();
        let og = apply(&one(2), &c, w, (15.0, 15.0, 10.0, 10.0));
        assert!(px(&og, 40, 13, 20)[3] > 30 && px(&og, 40, 26, 20)[3] > 30);
        assert_eq!(px(&og, 40, 20, 20), [200, 100, 50, 255]);
        let ig = apply(&one(3), &c, w, (15.0, 15.0, 10.0, 10.0));
        assert_eq!(px(&ig, 40, 12, 20)[3], 0, "nada fora do conteúdo");
        let edge = px(&ig, 40, 15, 20);
        assert!(
            edge[0] > 200 && edge[3] == 255,
            "borda clareada pelo brilho: {edge:?}"
        );
    }

    #[test]
    fn stroke_positions_follow_the_edge() {
        let (c, w) = square();
        let mut s = one(4);
        let b = (15.0, 15.0, 10.0, 10.0);
        if let EffectKind::Stroke { position, size, .. } = &mut s[0].kind {
            *position = StrokePos::Outside;
            *size = 3.0;
        }
        let out = apply(&s, &c, w, b);
        assert_eq!(px(&out, 40, 14, 20), [255, 0, 0, 255], "logo fora da borda");
        assert_eq!(px(&out, 40, 12, 20)[3], 255);
        assert_eq!(px(&out, 40, 11, 20)[3], 0, "além do tamanho do traço");
        assert_eq!(px(&out, 40, 17, 20), [200, 100, 50, 255], "dentro intacto");
        if let EffectKind::Stroke { position, .. } = &mut s[0].kind {
            *position = StrokePos::Inside;
        }
        let ins = apply(&s, &c, w, b);
        assert_eq!(px(&ins, 40, 15, 20), [255, 0, 0, 255]);
        assert_eq!(px(&ins, 40, 14, 20)[3], 0);
        assert_eq!(px(&ins, 40, 20, 20), [200, 100, 50, 255]);
    }

    #[test]
    fn overlays_are_clipped_to_the_content_alpha() {
        let (c, w) = square();
        let b = (15.0, 15.0, 10.0, 10.0);
        let co = apply(&one(5), &c, w, b);
        assert_eq!(px(&co, 40, 20, 20), [255, 0, 0, 255]);
        assert_eq!(px(&co, 40, 5, 5)[3], 0);
        // Gradiente de preto (topo) a branco (base) no ângulo 90°.
        let go = apply(&one(6), &c, w, b);
        assert!(px(&go, 40, 20, 15)[0] < 60 && px(&go, 40, 20, 24)[0] > 190);
        // Padrão xadrez com células de 8 px usa as duas cores.
        let po = apply(&one(7), &c, w, b);
        let colors: std::collections::HashSet<[u8; 4]> =
            (15..25).map(|x| px(&po, 40, x, 20)).collect();
        assert!(colors.len() >= 2);
    }

    #[test]
    fn inner_shadow_and_bevel_only_touch_the_inside() {
        let (c, w) = square();
        let b = (15.0, 15.0, 10.0, 10.0);
        for kind in [1usize, 8] {
            let out = apply(&one(kind), &c, w, b);
            assert_eq!(px(&out, 40, 10, 20)[3], 0, "{}: fora intacto", KINDS[kind]);
            let changed = (15..25).any(|x| px(&out, 40, x, 20) != [200, 100, 50, 255]);
            assert!(changed, "{} deve alterar a borda interna", KINDS[kind]);
        }
        // Chanfro: lados opostos recebem luz e sombra diferentes.
        let out = apply(&one(8), &c, w, b);
        assert_ne!(px(&out, 40, 15, 20), px(&out, 40, 24, 20));
    }

    #[test]
    fn semi_transparent_content_keeps_its_color_without_halos() {
        let (mut c, w) = square();
        for p in c.as_chunks_mut::<4>().0 {
            if p[3] > 0 {
                *p = [255, 0, 0, 128];
            }
        }
        let out = apply(&one(5), &c, w, (15.0, 15.0, 10.0, 10.0));
        let p = px(&out, 40, 20, 20);
        assert!(p[3] == 128 && p[0] >= 250, "{p:?}");
        // A sombra de um conteúdo translúcido só aparece na proporção do que falta de alfa.
        let ds = apply(&one(0), &c, w, (15.0, 15.0, 10.0, 10.0));
        let q = px(&ds, 40, 20, 20);
        assert!(q[3] >= 128 && q[3] < 255, "{q:?}");
    }

    #[test]
    fn reach_and_sanity() {
        let list = one(0);
        assert!(reach(&list) >= 16);
        assert_eq!(reach(&[]), 0);
        assert!(effects_sane(&list));
        let mut bad = list.clone();
        if let EffectKind::DropShadow { size, .. } = &mut bad[0].kind {
            *size = f32::NAN;
        }
        assert!(!effects_sane(&bad));
        assert!(!effects_sane(&vec![list[0].clone(); 17]));
        assert!(needs_bounds(&one(6)) && !needs_bounds(&list));
    }

    #[test]
    fn patterns_are_deterministic_and_periodic() {
        let p = Pattern::default_of(0);
        assert_eq!(
            p.color_at(1.0, 1.0, 8.0),
            p.color_at(17.0, 1.0, 8.0).map(|v| v),
            "mesmo par de células"
        );
        assert_ne!(p.color_at(1.0, 1.0, 8.0), p.color_at(9.0, 1.0, 8.0));
        let s = Pattern::default_of(1);
        assert_eq!(s.color_at(3.0, 3.0, 4.0), s.color_at(3.0, 3.0, 4.0));
    }
}
