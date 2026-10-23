//! Modos de mesclagem. Fórmulas em espaço sRGB codificado (gama), canais em 0..=1,
//! como nos editores de referência; ver docs/COMPOSITING.md para diferenças conhecidas.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    /// Só para grupos: os filhos são mesclados diretamente com o que está abaixo.
    PassThrough,
    #[default]
    Normal,
    Dissolve,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    Lighten,
    Screen,
    ColorDodge,
    LinearDodge,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    /// Modos disponíveis para camadas raster (na ordem do menu).
    pub const LAYER_MODES: [BlendMode; 25] = [
        BlendMode::Normal,
        BlendMode::Dissolve,
        BlendMode::Darken,
        BlendMode::Multiply,
        BlendMode::ColorBurn,
        BlendMode::LinearBurn,
        BlendMode::Lighten,
        BlendMode::Screen,
        BlendMode::ColorDodge,
        BlendMode::LinearDodge,
        BlendMode::Overlay,
        BlendMode::SoftLight,
        BlendMode::HardLight,
        BlendMode::VividLight,
        BlendMode::LinearLight,
        BlendMode::PinLight,
        BlendMode::HardMix,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Subtract,
        BlendMode::Divide,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BlendMode::PassThrough => "Passar através",
            BlendMode::Normal => "Normal",
            BlendMode::Dissolve => "Dissolver",
            BlendMode::Darken => "Escurecer",
            BlendMode::Multiply => "Multiplicar",
            BlendMode::ColorBurn => "Subexposição de cor",
            BlendMode::LinearBurn => "Subexposição linear",
            BlendMode::Lighten => "Clarear",
            BlendMode::Screen => "Tela",
            BlendMode::ColorDodge => "Superexposição de cor",
            BlendMode::LinearDodge => "Superexposição linear (adicionar)",
            BlendMode::Overlay => "Sobrepor",
            BlendMode::SoftLight => "Luz suave",
            BlendMode::HardLight => "Luz direta",
            BlendMode::VividLight => "Luz brilhante",
            BlendMode::LinearLight => "Luz linear",
            BlendMode::PinLight => "Luz pontual",
            BlendMode::HardMix => "Mistura dura",
            BlendMode::Difference => "Diferença",
            BlendMode::Exclusion => "Exclusão",
            BlendMode::Subtract => "Subtrair",
            BlendMode::Divide => "Dividir",
            BlendMode::Hue => "Matiz",
            BlendMode::Saturation => "Saturação",
            BlendMode::Color => "Cor",
            BlendMode::Luminosity => "Luminosidade",
        }
    }

    /// Modo não separável (depende dos três canais ao mesmo tempo).
    fn non_separable(self) -> bool {
        matches!(
            self,
            BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity
        )
    }
}

fn color_burn(cb: f32, cs: f32) -> f32 {
    if cb >= 1.0 {
        1.0
    } else if cs <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - cb) / cs).min(1.0)
    }
}

fn color_dodge(cb: f32, cs: f32) -> f32 {
    if cb <= 0.0 {
        0.0
    } else if cs >= 1.0 {
        1.0
    } else {
        (cb / (1.0 - cs)).min(1.0)
    }
}

fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        cb * 2.0 * cs
    } else {
        let s = 2.0 * cs - 1.0;
        cb + s - cb * s
    }
}

fn soft_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
    } else {
        let d = if cb <= 0.25 {
            ((16.0 * cb - 12.0) * cb + 4.0) * cb
        } else {
            cb.sqrt()
        };
        cb + (2.0 * cs - 1.0) * (d - cb)
    }
}

/// Mescla de um canal (modos separáveis). `cb` = fundo, `cs` = origem.
fn blend_channel(mode: BlendMode, cb: f32, cs: f32) -> f32 {
    let v = match mode {
        BlendMode::Darken => cb.min(cs),
        BlendMode::Multiply => cb * cs,
        BlendMode::ColorBurn => color_burn(cb, cs),
        BlendMode::LinearBurn => cb + cs - 1.0,
        BlendMode::Lighten => cb.max(cs),
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::ColorDodge => color_dodge(cb, cs),
        BlendMode::LinearDodge => cb + cs,
        BlendMode::Overlay => hard_light(cs, cb),
        BlendMode::SoftLight => soft_light(cb, cs),
        BlendMode::HardLight => hard_light(cb, cs),
        BlendMode::VividLight => {
            if cs <= 0.5 {
                color_burn(cb, 2.0 * cs)
            } else {
                color_dodge(cb, 2.0 * (cs - 0.5))
            }
        }
        BlendMode::LinearLight => cb + 2.0 * cs - 1.0,
        BlendMode::PinLight => {
            if cs < 0.5 {
                cb.min(2.0 * cs)
            } else {
                cb.max(2.0 * cs - 1.0)
            }
        }
        BlendMode::HardMix => {
            if cb + cs >= 1.0 {
                1.0
            } else {
                0.0
            }
        }
        BlendMode::Difference => (cb - cs).abs(),
        BlendMode::Exclusion => cb + cs - 2.0 * cb * cs,
        BlendMode::Subtract => cb - cs,
        BlendMode::Divide => {
            if cs <= 0.0 {
                if cb <= 0.0 {
                    0.0
                } else {
                    1.0
                }
            } else {
                cb / cs
            }
        }
        // Normal, Dissolve (tratado no compositor) e PassThrough.
        _ => cs,
    };
    v.clamp(0.0, 1.0)
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 {
        for v in &mut c {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 1.0 {
        for v in &mut c {
            *v = l + (*v - l) * (1.0 - l) / (x - l);
        }
    }
    c
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|&a, &b| c[a].total_cmp(&c[b]));
    let (min, mid, max) = (idx[0], idx[1], idx[2]);
    let mut out = [0.0f32; 3];
    if c[max] > c[min] {
        out[mid] = (c[mid] - c[min]) * s / (c[max] - c[min]);
        out[max] = s;
    }
    out
}

/// Cor mesclada (sem alfa) para `mode`, com fundo `cb` e origem `cs` em 0..=1.
pub fn blend_rgb(mode: BlendMode, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    if mode.non_separable() {
        let r = match mode {
            BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
            BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
            BlendMode::Color => set_lum(cs, lum(cb)),
            _ => set_lum(cb, lum(cs)),
        };
        return [
            r[0].clamp(0.0, 1.0),
            r[1].clamp(0.0, 1.0),
            r[2].clamp(0.0, 1.0),
        ];
    }
    [
        blend_channel(mode, cb[0], cs[0]),
        blend_channel(mode, cb[1], cs[1]),
        blend_channel(mode, cb[2], cs[2]),
    ]
}

/// Ruído determinístico em 0..1 por pixel (Dissolver); mesmo resultado em prévia e exportação.
pub fn dissolve_noise(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA6B) ^ 0xC2B2_AE35;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(mode: BlendMode, cb: f32, cs: f32) -> f32 {
        blend_channel(mode, cb, cs)
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn separable_formulas_on_known_inputs() {
        use BlendMode::*;
        assert!(near(ch(Multiply, 0.5, 0.5), 0.25));
        assert!(near(ch(Screen, 0.5, 0.5), 0.75));
        assert!(near(ch(Darken, 0.3, 0.7), 0.3));
        assert!(near(ch(Lighten, 0.3, 0.7), 0.7));
        assert!(near(ch(ColorBurn, 0.5, 0.5), 0.0));
        assert!(near(ch(ColorBurn, 1.0, 0.0), 1.0));
        assert!(near(ch(ColorBurn, 0.5, 1.0), 0.5));
        assert!(near(ch(LinearBurn, 0.5, 0.5), 0.0));
        assert!(near(ch(LinearBurn, 0.8, 0.5), 0.3));
        assert!(near(ch(ColorDodge, 0.5, 0.5), 1.0));
        assert!(near(ch(ColorDodge, 0.25, 0.5), 0.5));
        assert!(near(ch(ColorDodge, 0.0, 0.9), 0.0));
        assert!(near(ch(LinearDodge, 0.6, 0.7), 1.0));
        assert!(near(ch(LinearDodge, 0.2, 0.3), 0.5));
        assert!(near(ch(Overlay, 0.25, 0.5), 0.25));
        assert!(near(ch(Overlay, 0.25, 1.0), 0.5));
        assert!(near(ch(HardLight, 0.5, 0.25), 0.25));
        assert!(near(ch(HardLight, 0.5, 0.75), 0.75));
        assert!(near(ch(SoftLight, 0.5, 0.5), 0.5));
        assert!(near(ch(SoftLight, 0.5, 1.0), 0.5f32.sqrt()));
        assert!(near(ch(SoftLight, 0.5, 0.0), 0.25));
        assert!(near(ch(VividLight, 0.5, 0.25), 0.0));
        assert!(near(ch(VividLight, 0.25, 0.75), 0.5));
        assert!(near(ch(LinearLight, 0.5, 0.75), 1.0));
        assert!(near(ch(LinearLight, 0.5, 0.25), 0.0));
        assert!(near(ch(PinLight, 0.5, 0.2), 0.4));
        assert!(near(ch(PinLight, 0.5, 0.9), 0.8));
        assert!(near(ch(HardMix, 0.6, 0.5), 1.0));
        assert!(near(ch(HardMix, 0.4, 0.5), 0.0));
        assert!(near(ch(Difference, 0.2, 0.7), 0.5));
        assert!(near(ch(Exclusion, 0.5, 0.5), 0.5));
        assert!(near(ch(Exclusion, 1.0, 1.0), 0.0));
        assert!(near(ch(Subtract, 0.7, 0.2), 0.5));
        assert!(near(ch(Subtract, 0.2, 0.7), 0.0));
        assert!(near(ch(Divide, 0.2, 0.5), 0.4));
        assert!(near(ch(Divide, 0.8, 0.5), 1.0));
        assert!(near(ch(Divide, 0.3, 0.0), 1.0));
        assert!(near(ch(Divide, 0.0, 0.0), 0.0));
        assert!(near(ch(Normal, 0.3, 0.9), 0.9));
    }

    #[test]
    fn white_and_black_identities() {
        use BlendMode::*;
        for cb in [0.0, 0.2, 0.5, 0.9, 1.0] {
            assert!(near(ch(Multiply, cb, 1.0), cb), "multiplicar por branco");
            assert!(near(ch(Screen, cb, 0.0), cb), "tela com preto");
            assert!(near(ch(LinearDodge, cb, 0.0), cb));
            assert!(near(ch(Difference, cb, 0.0), cb));
            assert!(near(ch(Subtract, cb, 0.0), cb));
            assert!(near(ch(Darken, cb, 1.0), cb));
            assert!(near(ch(Lighten, cb, 0.0), cb));
            assert!(near(ch(SoftLight, cb, 0.5), cb), "cinza 50% não altera");
            assert!(near(ch(Overlay, cb, 0.5), cb));
        }
    }

    #[test]
    fn all_results_stay_in_range() {
        for mode in BlendMode::LAYER_MODES {
            for i in 0..=10 {
                for j in 0..=10 {
                    let (a, b) = (i as f32 / 10.0, j as f32 / 10.0);
                    let r = blend_rgb(mode, [a, b, a], [b, a, b]);
                    assert!(r.iter().all(|v| (0.0..=1.0).contains(v)), "{mode:?}");
                }
            }
        }
    }

    #[test]
    fn non_separable_properties() {
        let cb = [0.2, 0.4, 0.6];
        let cs = [1.0, 0.1, 0.3];
        let color = blend_rgb(BlendMode::Color, cb, cs);
        assert!(
            near(lum(color), lum(cb)),
            "Cor preserva a luminosidade do fundo"
        );
        let l = blend_rgb(BlendMode::Luminosity, cb, cs);
        assert!(
            near(lum(l), lum(cs)),
            "Luminosidade usa a luminosidade da origem"
        );
        let h = blend_rgb(BlendMode::Hue, cb, cs);
        assert!(near(lum(h), lum(cb)) && (sat(h) - sat(cb)).abs() < 1e-3);
        // Origem cinza: saturação zero.
        let s = blend_rgb(BlendMode::Saturation, cb, [0.5, 0.5, 0.5]);
        assert!(near(sat(s), 0.0));
        // Origem com a mesma cor do fundo: Cor e Luminosidade devolvem o fundo.
        for m in [BlendMode::Color, BlendMode::Luminosity] {
            let r = blend_rgb(m, cb, cb);
            assert!(r.iter().zip(cb).all(|(a, b)| near(*a, b)), "{m:?}");
        }
    }

    #[test]
    fn dissolve_noise_is_deterministic_and_spread() {
        assert_eq!(dissolve_noise(3, 9), dissolve_noise(3, 9));
        let below = (0..100u32)
            .flat_map(|x| (0..100u32).map(move |y| dissolve_noise(x, y)))
            .filter(|&n| n < 0.5)
            .count();
        assert!((4500..5500).contains(&below), "{below}");
    }

    #[test]
    fn serde_names_are_stable() {
        assert_eq!(
            serde_json::to_string(&BlendMode::LinearDodge).unwrap(),
            "\"linear_dodge\""
        );
        let m: BlendMode = serde_json::from_str("\"pass_through\"").unwrap();
        assert_eq!(m, BlendMode::PassThrough);
    }
}
