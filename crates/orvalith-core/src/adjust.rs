//! Ajustes de cor: funções puras sobre RGB sRGB codificado (8 bits na entrada e na saída,
//! `f32` internamente). O alfa nunca é alterado. Servem tanto para ajustes destrutivos quanto
//! para camadas de ajuste (a especificação fica guardada e pode ser editada).
//!
//! Aproximações documentadas: `Luma` usa os pesos Rec.601 sobre valores codificados;
//! "Cor seletiva" e "Equilíbrio de cores" são modelos clássicos simples, não idênticos aos de
//! outros editores.

use serde::{Deserialize, Serialize};

const LUMA: [f32; 3] = [0.299, 0.587, 0.114];

pub fn luma(c: [f32; 3]) -> f32 {
    c[0] * LUMA[0] + c[1] * LUMA[1] + c[2] * LUMA[2]
}

fn srgb_to_lin(v: f32) -> f32 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn lin_to_srgb(v: f32) -> f32 {
    let v = v.max(0.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn clamp01(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

/// Níveis de um canal (valores em 0..=255; `gamma` > 0).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LevelsChan {
    pub in_black: f32,
    pub in_white: f32,
    pub gamma: f32,
    pub out_black: f32,
    pub out_white: f32,
}

impl Default for LevelsChan {
    fn default() -> Self {
        Self {
            in_black: 0.0,
            in_white: 255.0,
            gamma: 1.0,
            out_black: 0.0,
            out_white: 255.0,
        }
    }
}

impl LevelsChan {
    fn apply(&self, v: f32) -> f32 {
        let span = (self.in_white - self.in_black).max(1.0);
        let t = clamp01((v * 255.0 - self.in_black) / span);
        let t = t.powf(1.0 / self.gamma.max(0.01));
        (self.out_black + t * (self.out_white - self.out_black)) / 255.0
    }

    fn sane(&mut self) {
        let f = |v: f32, d: f32| {
            if v.is_finite() {
                v.clamp(0.0, 255.0)
            } else {
                d
            }
        };
        self.in_black = f(self.in_black, 0.0);
        self.in_white = f(self.in_white, 255.0);
        if self.in_white <= self.in_black {
            self.in_white = (self.in_black + 1.0).min(255.0);
            self.in_black = self.in_black.min(self.in_white - 1.0).max(0.0);
        }
        self.out_black = f(self.out_black, 0.0);
        self.out_white = f(self.out_white, 255.0);
        self.gamma = if self.gamma.is_finite() {
            self.gamma.clamp(0.1, 9.99)
        } else {
            1.0
        };
    }
}

/// Curva definida por pontos (x, y) em 0..=1, interpolada de forma monotônica.
pub fn identity_curve() -> Vec<[f32; 2]> {
    vec![[0.0, 0.0], [1.0, 1.0]]
}

fn sort_curve(pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut p: Vec<[f32; 2]> = pts
        .iter()
        .filter(|q| q[0].is_finite() && q[1].is_finite())
        .map(|q| [clamp01(q[0]), clamp01(q[1])])
        .collect();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]));
    p.dedup_by(|b, a| (b[0] - a[0]).abs() < 1e-4);
    p
}

/// Avalia a curva em `x` (Fritsch–Carlson: sem oscilações entre os pontos).
pub fn eval_curve(pts: &[[f32; 2]], x: f32) -> f32 {
    let p = sort_curve(pts);
    if p.len() < 2 {
        return x;
    }
    let n = p.len();
    if x <= p[0][0] {
        return p[0][1];
    }
    if x >= p[n - 1][0] {
        return p[n - 1][1];
    }
    let d: Vec<f32> = (0..n - 1)
        .map(|i| (p[i + 1][1] - p[i][1]) / (p[i + 1][0] - p[i][0]))
        .collect();
    let mut m = vec![0.0f32; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        m[i] = if d[i - 1] * d[i] <= 0.0 {
            0.0
        } else {
            (d[i - 1] + d[i]) / 2.0
        };
    }
    for i in 0..n - 1 {
        if d[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
        } else {
            let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                m[i] = t * a * d[i];
                m[i + 1] = t * b * d[i];
            }
        }
    }
    let i = p.iter().rposition(|q| q[0] <= x).unwrap_or(0).min(n - 2);
    let h = p[i + 1][0] - p[i][0];
    let t = (x - p[i][0]) / h;
    let (t2, t3) = (t * t, t * t * t);
    let y = (2.0 * t3 - 3.0 * t2 + 1.0) * p[i][1]
        + (t3 - 2.0 * t2 + t) * h * m[i]
        + (-2.0 * t3 + 3.0 * t2) * p[i + 1][1]
        + (t3 - t2) * h * m[i + 1];
    clamp01(y)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradStop {
    pub pos: f32,
    pub color: [u8; 3],
}

/// Nomes das 9 faixas de "Cor seletiva".
pub const SELECTIVE_RANGES: [&str; 9] = [
    "Vermelhos",
    "Amarelos",
    "Verdes",
    "Cianos",
    "Azuis",
    "Magentas",
    "Brancos",
    "Neutros",
    "Pretos",
];

/// Nomes dos 6 pesos de "Preto e branco".
pub const BW_NAMES: [&str; 6] = [
    "Vermelhos",
    "Amarelos",
    "Verdes",
    "Cianos",
    "Azuis",
    "Magentas",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Adjustment {
    /// -100..=100 cada.
    BrightnessContrast {
        brightness: f32,
        contrast: f32,
    },
    /// Exposição em stops (luz linear), deslocamento e gama.
    Exposure {
        exposure: f32,
        offset: f32,
        gamma: f32,
    },
    Levels {
        master: LevelsChan,
        r: LevelsChan,
        g: LevelsChan,
        b: LevelsChan,
    },
    Curves {
        master: Vec<[f32; 2]>,
        r: Vec<[f32; 2]>,
        g: Vec<[f32; 2]>,
        b: Vec<[f32; 2]>,
    },
    /// Matiz -180..=180, saturação e luminosidade -100..=100.
    HueSaturation {
        hue: f32,
        saturation: f32,
        lightness: f32,
    },
    Vibrance {
        vibrance: f32,
        saturation: f32,
    },
    /// Cada tom: ciano↔vermelho, magenta↔verde, amarelo↔azul (-100..=100).
    ColorBalance {
        shadows: [f32; 3],
        midtones: [f32; 3],
        highlights: [f32; 3],
        preserve_luminosity: bool,
    },
    /// Pesos (%) de vermelhos, amarelos, verdes, cianos, azuis, magentas; tonalização opcional.
    BlackWhite {
        weights: [f32; 6],
        tint: Option<[u8; 3]>,
    },
    PhotoFilter {
        color: [u8; 3],
        density: f32,
        preserve_luminosity: bool,
    },
    /// Linhas = canais de saída (R, G, B); colunas = entrada (R, G, B), em %; constante em %.
    ChannelMixer {
        matrix: [[f32; 3]; 3],
        constant: [f32; 3],
        monochrome: bool,
    },
    /// 9 faixas × (ciano, magenta, amarelo, preto) em -100..=100.
    SelectiveColor {
        ranges: [[f32; 4]; 9],
        relative: bool,
    },
    GradientMap {
        stops: Vec<GradStop>,
        reverse: bool,
    },
    Invert,
    Posterize {
        levels: u32,
    },
    /// Limiar 0..=255 sobre a luminância.
    Threshold {
        level: f32,
    },
    /// Tons de cinza (luminância).
    Grayscale,
}

/// Tipos de ajuste na ordem dos menus.
pub const KINDS: [&str; 16] = [
    "Brilho/Contraste",
    "Exposição",
    "Níveis",
    "Curvas",
    "Matiz/Saturação",
    "Vibração",
    "Equilíbrio de cores",
    "Preto e branco",
    "Filtro de foto",
    "Mixagem de canais",
    "Cor seletiva",
    "Mapa de gradiente",
    "Inverter",
    "Posterizar",
    "Limiar",
    "Tons de cinza",
];

impl Adjustment {
    /// Ajuste neutro (não altera a imagem) do tipo de índice `kind` em [`KINDS`].
    pub fn neutral(kind: usize) -> Self {
        match kind {
            0 => Self::BrightnessContrast {
                brightness: 0.0,
                contrast: 0.0,
            },
            1 => Self::Exposure {
                exposure: 0.0,
                offset: 0.0,
                gamma: 1.0,
            },
            2 => Self::Levels {
                master: LevelsChan::default(),
                r: LevelsChan::default(),
                g: LevelsChan::default(),
                b: LevelsChan::default(),
            },
            3 => Self::Curves {
                master: identity_curve(),
                r: identity_curve(),
                g: identity_curve(),
                b: identity_curve(),
            },
            4 => Self::HueSaturation {
                hue: 0.0,
                saturation: 0.0,
                lightness: 0.0,
            },
            5 => Self::Vibrance {
                vibrance: 0.0,
                saturation: 0.0,
            },
            6 => Self::ColorBalance {
                shadows: [0.0; 3],
                midtones: [0.0; 3],
                highlights: [0.0; 3],
                preserve_luminosity: true,
            },
            7 => Self::BlackWhite {
                weights: [40.0, 60.0, 40.0, 60.0, 20.0, 80.0],
                tint: None,
            },
            8 => Self::PhotoFilter {
                color: [236, 138, 0],
                density: 0.0,
                preserve_luminosity: true,
            },
            9 => Self::ChannelMixer {
                matrix: [[100.0, 0.0, 0.0], [0.0, 100.0, 0.0], [0.0, 0.0, 100.0]],
                constant: [0.0; 3],
                monochrome: false,
            },
            10 => Self::SelectiveColor {
                ranges: [[0.0; 4]; 9],
                relative: true,
            },
            11 => Self::GradientMap {
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
                reverse: false,
            },
            12 => Self::Invert,
            13 => Self::Posterize { levels: 8 },
            14 => Self::Threshold { level: 128.0 },
            _ => Self::Grayscale,
        }
    }

    /// Índice do tipo em [`KINDS`].
    pub fn kind_index(&self) -> usize {
        match self {
            Self::BrightnessContrast { .. } => 0,
            Self::Exposure { .. } => 1,
            Self::Levels { .. } => 2,
            Self::Curves { .. } => 3,
            Self::HueSaturation { .. } => 4,
            Self::Vibrance { .. } => 5,
            Self::ColorBalance { .. } => 6,
            Self::BlackWhite { .. } => 7,
            Self::PhotoFilter { .. } => 8,
            Self::ChannelMixer { .. } => 9,
            Self::SelectiveColor { .. } => 10,
            Self::GradientMap { .. } => 11,
            Self::Invert => 12,
            Self::Posterize { .. } => 13,
            Self::Threshold { .. } => 14,
            Self::Grayscale => 15,
        }
    }

    pub fn name(&self) -> &'static str {
        KINDS[self.kind_index()]
    }

    /// Limita os parâmetros a faixas válidas (valores inválidos viram o neutro).
    pub fn sanitize(&mut self) {
        let lim = |v: &mut f32, lo: f32, hi: f32, d: f32| {
            *v = if v.is_finite() { v.clamp(lo, hi) } else { d };
        };
        match self {
            Self::BrightnessContrast {
                brightness,
                contrast,
            } => {
                lim(brightness, -100.0, 100.0, 0.0);
                lim(contrast, -100.0, 100.0, 0.0);
            }
            Self::Exposure {
                exposure,
                offset,
                gamma,
            } => {
                lim(exposure, -10.0, 10.0, 0.0);
                lim(offset, -0.5, 0.5, 0.0);
                lim(gamma, 0.1, 9.99, 1.0);
            }
            Self::Levels { master, r, g, b } => {
                for c in [master, r, g, b] {
                    c.sane();
                }
            }
            Self::Curves { master, r, g, b } => {
                for c in [master, r, g, b] {
                    let mut p = sort_curve(c);
                    p.truncate(16);
                    if p.len() < 2 {
                        p = identity_curve();
                    }
                    *c = p;
                }
            }
            Self::HueSaturation {
                hue,
                saturation,
                lightness,
            } => {
                lim(hue, -180.0, 180.0, 0.0);
                lim(saturation, -100.0, 100.0, 0.0);
                lim(lightness, -100.0, 100.0, 0.0);
            }
            Self::Vibrance {
                vibrance,
                saturation,
            } => {
                lim(vibrance, -100.0, 100.0, 0.0);
                lim(saturation, -100.0, 100.0, 0.0);
            }
            Self::ColorBalance {
                shadows,
                midtones,
                highlights,
                ..
            } => {
                for t in [shadows, midtones, highlights] {
                    for v in t.iter_mut() {
                        lim(v, -100.0, 100.0, 0.0);
                    }
                }
            }
            Self::BlackWhite { weights, .. } => {
                for v in weights.iter_mut() {
                    lim(v, -200.0, 300.0, 50.0);
                }
            }
            Self::PhotoFilter { density, .. } => lim(density, 0.0, 100.0, 0.0),
            Self::ChannelMixer {
                matrix, constant, ..
            } => {
                for row in matrix.iter_mut() {
                    for v in row.iter_mut() {
                        lim(v, -200.0, 200.0, 0.0);
                    }
                }
                for v in constant.iter_mut() {
                    lim(v, -100.0, 100.0, 0.0);
                }
            }
            Self::SelectiveColor { ranges, .. } => {
                for r in ranges.iter_mut() {
                    for v in r.iter_mut() {
                        lim(v, -100.0, 100.0, 0.0);
                    }
                }
            }
            Self::GradientMap { stops, .. } => {
                stops.truncate(32);
                for s in stops.iter_mut() {
                    lim(&mut s.pos, 0.0, 1.0, 0.0);
                }
                stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
                if stops.len() < 2 {
                    *stops = Self::neutral(11).stops_owned();
                }
            }
            Self::Invert | Self::Grayscale => {}
            Self::Posterize { levels } => *levels = (*levels).clamp(2, 255),
            Self::Threshold { level } => lim(level, 0.0, 255.0, 128.0),
        }
    }

    fn stops_owned(self) -> Vec<GradStop> {
        match self {
            Self::GradientMap { stops, .. } => stops,
            _ => Vec::new(),
        }
    }

    /// Verdadeiro se os parâmetros já estão dentro dos limites (leitura de arquivo).
    pub fn is_sane(&self) -> bool {
        let mut c = self.clone();
        c.sanitize();
        let json_ok = |a: &Self| serde_json::to_value(a).is_ok();
        json_ok(self) && *self == c
    }

    /// Prepara tabelas para aplicar o ajuste a muitos pixels.
    pub fn prepare(&self) -> Prepared {
        let mut adj = self.clone();
        adj.sanitize();
        let lut = adj.build_lut();
        Prepared { adj, lut }
    }

    fn lut_capable(&self) -> bool {
        matches!(
            self,
            Self::BrightnessContrast { .. }
                | Self::Exposure { .. }
                | Self::Levels { .. }
                | Self::Curves { .. }
                | Self::Invert
                | Self::Posterize { .. }
        )
    }

    fn chan(&self, ch: usize, v: f32) -> f32 {
        match self {
            Self::BrightnessContrast {
                brightness,
                contrast,
            } => {
                let slope = ((contrast / 100.0 + 1.0) * std::f32::consts::FRAC_PI_4).tan();
                (v - 0.5) * slope + 0.5 + brightness / 200.0
            }
            Self::Exposure {
                exposure,
                offset,
                gamma,
            } => {
                let lin = srgb_to_lin(v) * 2f32.powf(*exposure) + offset;
                lin_to_srgb(lin.max(0.0).powf(1.0 / gamma))
            }
            Self::Levels { master, r, g, b } => {
                let own = [r, g, b][ch].apply(v);
                master.apply(clamp01(own))
            }
            Self::Curves { master, r, g, b } => {
                let own = eval_curve([r, g, b][ch], v);
                eval_curve(master, own)
            }
            Self::Invert => 1.0 - v,
            Self::Posterize { levels } => {
                let n = (*levels - 1) as f32;
                (v * n).round() / n
            }
            _ => v,
        }
    }

    fn build_lut(&self) -> Option<Box<[[u8; 256]; 3]>> {
        if !self.lut_capable() {
            return None;
        }
        let mut lut = Box::new([[0u8; 256]; 3]);
        for ch in 0..3 {
            for i in 0..256 {
                let v = self.chan(ch, i as f32 / 255.0);
                lut[ch][i] = (clamp01(v) * 255.0 + 0.5) as u8;
            }
        }
        Some(lut)
    }

    /// Calcula o resultado de um pixel (valores 0..=1) para ajustes que não são por canal.
    fn map(&self, c: [f32; 3]) -> [f32; 3] {
        match self {
            Self::HueSaturation {
                hue,
                saturation,
                lightness,
            } => {
                let [mut h, mut s, mut l] = rgb_to_hsl(c);
                h = (h + hue / 360.0).rem_euclid(1.0);
                s = (s * (1.0 + saturation / 100.0)).min(1.0);
                let lg = lightness / 100.0;
                l = if lg >= 0.0 {
                    l + (1.0 - l) * lg
                } else {
                    l * (1.0 + lg)
                };
                hsl_to_rgb([h, s, clamp01(l)])
            }
            Self::Vibrance {
                vibrance,
                saturation,
            } => {
                let y = luma(c);
                let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
                let sat = if mx <= 0.0 { 0.0 } else { (mx - mn) / mx };
                let v = vibrance / 100.0;
                let mut f = if v >= 0.0 {
                    1.0 + v * (1.0 - sat)
                } else {
                    1.0 + v
                };
                f *= 1.0 + saturation / 100.0;
                let f = f.max(0.0);
                [0, 1, 2].map(|i| clamp01(y + (c[i] - y) * f))
            }
            Self::ColorBalance {
                shadows,
                midtones,
                highlights,
                preserve_luminosity,
            } => {
                let l = luma(c);
                let t = 2.0 * l - 1.0;
                let (ws, wh) = ((-t).max(0.0), t.max(0.0));
                let wm = 1.0 - t.abs();
                let mut out = c;
                for i in 0..3 {
                    out[i] = clamp01(
                        c[i] + 0.4 * (shadows[i] * ws + midtones[i] * wm + highlights[i] * wh)
                            / 100.0,
                    );
                }
                if *preserve_luminosity {
                    let d = l - luma(out);
                    for v in &mut out {
                        *v = clamp01(*v + d);
                    }
                }
                out
            }
            Self::BlackWhite { weights, tint } => {
                let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
                let g = if mx - mn < 1e-6 {
                    mx
                } else {
                    let h = rgb_to_hsl(c)[0] * 6.0;
                    let i = (h.floor() as usize) % 6;
                    let f = h - h.floor();
                    let w = weights[i] * (1.0 - f) + weights[(i + 1) % 6] * f;
                    mn + (mx - mn) * w / 100.0
                };
                let g = clamp01(g);
                match tint {
                    None => [g; 3],
                    Some(t) => {
                        let tc = t.map(|v| f32::from(v) / 255.0);
                        let [h, s, _] = rgb_to_hsl(tc);
                        hsl_to_rgb([h, s, g])
                    }
                }
            }
            Self::PhotoFilter {
                color,
                density,
                preserve_luminosity,
            } => {
                let cc = color.map(|v| f32::from(v) / 255.0);
                let mx = cc[0].max(cc[1]).max(cc[2]).max(1e-6);
                let d = density / 100.0;
                let mut out = [0, 1, 2].map(|i| c[i] + (c[i] * cc[i] / mx - c[i]) * d);
                if *preserve_luminosity {
                    let dl = luma(c) - luma(out);
                    for v in &mut out {
                        *v += dl;
                    }
                }
                out.map(clamp01)
            }
            Self::ChannelMixer {
                matrix,
                constant,
                monochrome,
            } => {
                let row = |i: usize| {
                    (matrix[i][0] * c[0] + matrix[i][1] * c[1] + matrix[i][2] * c[2] + constant[i])
                        / 100.0
                };
                if *monochrome {
                    [clamp01(row(0)); 3]
                } else {
                    [clamp01(row(0)), clamp01(row(1)), clamp01(row(2))]
                }
            }
            Self::SelectiveColor { ranges, relative } => selective(c, ranges, *relative),
            Self::GradientMap { stops, reverse } => {
                let g = clamp01(luma(c));
                gradient_at(stops, if *reverse { 1.0 - g } else { g })
            }
            Self::Threshold { level } => {
                if luma(c) * 255.0 >= *level {
                    [1.0; 3]
                } else {
                    [0.0; 3]
                }
            }
            Self::Grayscale => [clamp01(luma(c)); 3],
            _ => c,
        }
    }
}

fn rgb_to_hsl(c: [f32; 3]) -> [f32; 3] {
    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    let l = (mx + mn) / 2.0;
    let d = mx - mn;
    if d < 1e-6 {
        return [0.0, 0.0, l];
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-6);
    let h = if mx == c[0] {
        ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if mx == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    [h / 6.0, s.min(1.0), l]
}

fn hsl_to_rgb(hsl: [f32; 3]) -> [f32; 3] {
    let [h, s, l] = hsl;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(1.0) * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [clamp01(r + m), clamp01(g + m), clamp01(b + m)]
}

fn gradient_at(stops: &[GradStop], t: f32) -> [f32; 3] {
    let col = |s: &GradStop| s.color.map(|v| f32::from(v) / 255.0);
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
        return [t; 3];
    };
    if t <= first.pos {
        return col(first);
    }
    if t >= last.pos {
        return col(last);
    }
    for w in stops.windows(2) {
        if t >= w[0].pos && t <= w[1].pos {
            let span = (w[1].pos - w[0].pos).max(1e-6);
            let f = (t - w[0].pos) / span;
            let (a, b) = (col(&w[0]), col(&w[1]));
            return [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * f);
        }
    }
    col(last)
}

fn selective(c: [f32; 3], ranges: &[[f32; 4]; 9], relative: bool) -> [f32; 3] {
    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    let chroma = mx - mn;
    let mut w = [0.0f32; 9];
    if chroma > 1e-6 {
        let h = rgb_to_hsl(c)[0] * 360.0;
        for (k, wk) in w.iter_mut().take(6).enumerate() {
            let d = (h - 60.0 * k as f32).abs();
            let d = d.min(360.0 - d);
            *wk = (1.0 - d / 60.0).max(0.0) * chroma;
        }
    }
    w[6] = (2.0 * mn - 1.0).max(0.0);
    w[7] = (1.0 - chroma) * (1.0 - (2.0 * ((mx + mn) / 2.0) - 1.0).abs());
    w[8] = (1.0 - 2.0 * mx).max(0.0);
    let mut cmy = c.map(|v| 1.0 - v);
    let mut dk = 0.0;
    for (r, wk) in ranges.iter().zip(w) {
        if wk <= 0.0 {
            continue;
        }
        for i in 0..3 {
            let amount = r[i] / 100.0 * wk;
            cmy[i] += if relative { amount * cmy[i] } else { amount };
        }
        dk += r[3] / 100.0 * wk;
    }
    let mut out = cmy.map(|v| clamp01(1.0 - v));
    for v in &mut out {
        *v = clamp01(if dk >= 0.0 {
            *v * (1.0 - dk)
        } else {
            *v + (1.0 - *v) * -dk
        });
    }
    out
}

/// Ajuste pronto para uso em lote.
pub struct Prepared {
    adj: Adjustment,
    lut: Option<Box<[[u8; 256]; 3]>>,
}

impl Prepared {
    pub fn apply(&self, px: [u8; 3]) -> [u8; 3] {
        if let Some(l) = &self.lut {
            return [
                l[0][px[0] as usize],
                l[1][px[1] as usize],
                l[2][px[2] as usize],
            ];
        }
        let c = px.map(|v| f32::from(v) / 255.0);
        self.adj.map(c).map(|v| (clamp01(v) * 255.0 + 0.5) as u8)
    }

    /// Aplica a um buffer RGBA (o alfa não muda; pixels transparentes são ignorados).
    pub fn apply_rgba(&self, buf: &mut [u8]) {
        for p in buf.as_chunks_mut::<4>().0 {
            if p[3] == 0 {
                continue;
            }
            let o = self.apply([p[0], p[1], p[2]]);
            p[..3].copy_from_slice(&o);
        }
    }
}

/// Aplica `adj` a um buffer RGBA inteiro.
pub fn apply_rgba(adj: &Adjustment, buf: &mut [u8]) {
    adj.prepare().apply_rgba(buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Vec<u8> {
        let mut v = Vec::new();
        for r in (0..=255).step_by(51) {
            for g in (0..=255).step_by(51) {
                for b in (0..=255).step_by(51) {
                    v.extend_from_slice(&[r as u8, g as u8, b as u8, 200]);
                }
            }
        }
        v
    }

    #[test]
    fn neutral_adjustments_are_identity_with_alpha_preserved() {
        // Identidade exata onde o modelo é exato; os demais ficam a ±1 por arredondamento.
        for kind in 0..KINDS.len() {
            let adj = Adjustment::neutral(kind);
            let mut b = grid();
            let orig = b.clone();
            apply_rgba(&adj, &mut b);
            if matches!(kind, 7 | 11..=15) {
                continue; // não são identidade por definição
            }
            let tol = 1i32;
            for (a, o) in b.as_chunks::<4>().0.iter().zip(orig.as_chunks::<4>().0) {
                assert_eq!(a[3], o[3], "alfa mudou em {}", adj.name());
                for i in 0..3 {
                    assert!(
                        (i32::from(a[i]) - i32::from(o[i])).abs() <= tol,
                        "{}: {:?} -> {:?}",
                        adj.name(),
                        o,
                        a
                    );
                }
            }
        }
    }

    #[test]
    fn identity_curve_and_levels_are_exact() {
        for kind in [2usize, 3] {
            let p = Adjustment::neutral(kind).prepare();
            for v in 0..=255u8 {
                assert_eq!(p.apply([v, v, v]), [v, v, v]);
            }
        }
    }

    #[test]
    fn curves_are_monotone_and_hit_their_points() {
        let pts = vec![[0.0, 0.0], [0.25, 0.1], [0.75, 0.9], [1.0, 1.0]];
        assert!((eval_curve(&pts, 0.25) - 0.1).abs() < 1e-5);
        assert!((eval_curve(&pts, 0.75) - 0.9).abs() < 1e-5);
        let mut prev = -1.0;
        for i in 0..=100 {
            let y = eval_curve(&pts, i as f32 / 100.0);
            assert!(y >= prev - 1e-6 && (0.0..=1.0).contains(&y));
            prev = y;
        }
    }

    #[test]
    fn invert_is_involution_and_threshold_is_binary() {
        let mut b = grid();
        let orig = b.clone();
        apply_rgba(&Adjustment::Invert, &mut b);
        assert_ne!(b, orig);
        apply_rgba(&Adjustment::Invert, &mut b);
        assert_eq!(b, orig);
        let p = Adjustment::Threshold { level: 128.0 }.prepare();
        assert_eq!(p.apply([200, 200, 200]), [255, 255, 255]);
        assert_eq!(p.apply([10, 10, 10]), [0, 0, 0]);
    }

    #[test]
    fn known_values() {
        // Brilho +100 aumenta; contraste 0 mantém o cinza médio.
        let p = Adjustment::BrightnessContrast {
            brightness: 100.0,
            contrast: 0.0,
        }
        .prepare();
        assert_eq!(p.apply([0, 0, 0]), [128, 128, 128]);
        // Exposição +1 stop em luz linear: 0.5 linear → 1.0 → branco.
        let e = Adjustment::Exposure {
            exposure: 1.0,
            offset: 0.0,
            gamma: 1.0,
        }
        .prepare();
        assert_eq!(e.apply([255, 255, 255]), [255, 255, 255]);
        let mid = e.apply([128, 128, 128])[0];
        assert!(mid > 128 + 30, "{mid}");
        // Níveis: preto de entrada em 100 achata o escuro.
        let mut lv = Adjustment::neutral(2);
        if let Adjustment::Levels { master, .. } = &mut lv {
            master.in_black = 100.0;
        }
        assert_eq!(lv.prepare().apply([100, 50, 0]), [0, 0, 0]);
        // Posterizar em 2 níveis.
        let po = Adjustment::Posterize { levels: 2 }.prepare();
        assert_eq!(po.apply([120, 130, 255]), [0, 255, 255]);
        // Tons de cinza usa a luminância Rec.601.
        let g = Adjustment::Grayscale.prepare().apply([255, 0, 0]);
        assert_eq!(g, [76, 76, 76]);
    }

    #[test]
    fn hue_rotation_and_saturation_behave() {
        let hue = Adjustment::HueSaturation {
            hue: 120.0,
            saturation: 0.0,
            lightness: 0.0,
        }
        .prepare();
        let o = hue.apply([255, 0, 0]);
        assert!(o[1] > 250 && o[0] < 5 && o[2] < 5, "{o:?}");
        let desat = Adjustment::HueSaturation {
            hue: 0.0,
            saturation: -100.0,
            lightness: 0.0,
        }
        .prepare();
        let g = desat.apply([200, 50, 50]);
        assert!(g[0] == g[1] && g[1] == g[2]);
        // Cinza continua cinza ao aumentar a saturação.
        let sat = Adjustment::HueSaturation {
            hue: 0.0,
            saturation: 100.0,
            lightness: 0.0,
        }
        .prepare();
        assert_eq!(sat.apply([90, 90, 90]), [90, 90, 90]);
    }

    #[test]
    fn mixer_balance_filter_and_gradient_map() {
        let mut m = Adjustment::neutral(9);
        if let Adjustment::ChannelMixer { matrix, .. } = &mut m {
            *matrix = [[0.0, 0.0, 100.0], [0.0, 100.0, 0.0], [100.0, 0.0, 0.0]];
        }
        assert_eq!(m.prepare().apply([10, 20, 30]), [30, 20, 10]);
        let cb = Adjustment::ColorBalance {
            shadows: [0.0; 3],
            midtones: [100.0, 0.0, 0.0],
            highlights: [0.0; 3],
            preserve_luminosity: false,
        }
        .prepare();
        assert!(cb.apply([128, 128, 128])[0] > 128 + 20);
        let pf = Adjustment::PhotoFilter {
            color: [255, 128, 0],
            density: 100.0,
            preserve_luminosity: false,
        }
        .prepare();
        assert_eq!(pf.apply([200, 200, 200]), [200, 100, 0]);
        let gm = Adjustment::GradientMap {
            stops: vec![
                GradStop {
                    pos: 0.0,
                    color: [0, 0, 255],
                },
                GradStop {
                    pos: 1.0,
                    color: [255, 0, 0],
                },
            ],
            reverse: false,
        }
        .prepare();
        assert_eq!(gm.apply([0, 0, 0]), [0, 0, 255]);
        assert_eq!(gm.apply([255, 255, 255]), [255, 0, 0]);
        let bw = Adjustment::neutral(7).prepare();
        let g = bw.apply([200, 100, 50]);
        assert!(g[0] == g[1] && g[1] == g[2]);
        assert_eq!(bw.apply([77, 77, 77]), [77, 77, 77]);
        let sc = Adjustment::SelectiveColor {
            ranges: {
                let mut r = [[0.0; 4]; 9];
                r[0][2] = 100.0; // vermelhos: +amarelo
                r
            },
            relative: false,
        }
        .prepare();
        let before = [200u8, 40, 40];
        let after = sc.apply(before);
        assert!(after[2] < before[2], "{after:?}");
        assert_eq!(sc.apply([100, 100, 100]), [100, 100, 100]);
    }

    #[test]
    fn hostile_parameters_are_sanitized() {
        let mut a = Adjustment::Exposure {
            exposure: f32::NAN,
            offset: 99.0,
            gamma: -4.0,
        };
        assert!(!a.is_sane());
        a.sanitize();
        assert!(a.is_sane());
        let mut c = Adjustment::Curves {
            master: vec![[f32::NAN, 0.0]],
            r: identity_curve(),
            g: identity_curve(),
            b: identity_curve(),
        };
        c.sanitize();
        assert!(c.is_sane());
        let mut p = Adjustment::Posterize { levels: 0 };
        p.sanitize();
        assert_eq!(p, Adjustment::Posterize { levels: 2 });
        let json = serde_json::to_string(&Adjustment::neutral(10)).unwrap();
        assert_eq!(
            serde_json::from_str::<Adjustment>(&json).unwrap(),
            Adjustment::neutral(10)
        );
    }
}
