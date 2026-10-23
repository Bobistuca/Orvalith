//! Texto editável: layout e rasterização com `cosmic-text` (Unicode, bidi, fallback de fontes).
//!
//! Usa apenas as fontes instaladas no sistema (nada é baixado). Uma fonte ausente ao
//! reabrir não falha: o texto é desenhado com a fonte substituta e o aplicativo avisa
//! (`font_installed`).

use std::sync::{Mutex, OnceLock};

use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight,
};
use serde::{Deserialize, Serialize};

use crate::compose::over;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSpec {
    pub text: String,
    /// Família da fonte; vazio = sans-serif padrão do sistema.
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    /// Tamanho em pixels do documento.
    pub size: f32,
    pub color: [u8; 4],
    pub align: TextAlign,
    /// Entrelinha como múltiplo do tamanho (1,2 = padrão).
    pub line_height: f32,
    /// Espaçamento entre caracteres em pixels.
    pub letter_spacing: f32,
    /// Canto superior esquerdo da caixa de texto no documento.
    pub x: f32,
    pub y: f32,
    /// Largura da caixa: `Some` = texto de parágrafo (quebra de linha); `None` = texto de ponto.
    pub box_width: Option<f32>,
}

impl TextSpec {
    pub fn point(text: impl Into<String>, x: f32, y: f32, color: [u8; 4]) -> Self {
        Self {
            text: text.into(),
            family: String::new(),
            bold: false,
            italic: false,
            size: 32.0,
            color,
            align: TextAlign::Left,
            line_height: 1.2,
            letter_spacing: 0.0,
            x,
            y,
            box_width: None,
        }
    }

    /// Valores seguros para layout (arquivos editados ou corrompidos).
    fn sane(&self) -> (f32, f32, f32) {
        let ok = |v: f32, lo: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
        (
            ok(self.size, 1.0, 2000.0, 32.0),
            ok(self.line_height, 0.5, 5.0, 1.2),
            ok(self.letter_spacing, -50.0, 200.0, 0.0),
        )
    }
}

struct Fonts {
    system: FontSystem,
    cache: SwashCache,
}

fn fonts() -> &'static Mutex<Fonts> {
    static FONTS: OnceLock<Mutex<Fonts>> = OnceLock::new();
    FONTS.get_or_init(|| {
        Mutex::new(Fonts {
            system: FontSystem::new(),
            cache: SwashCache::new(),
        })
    })
}

/// Famílias de fonte instaladas, sem repetição e em ordem alfabética.
pub fn list_fonts() -> Vec<String> {
    let f = fonts()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut v: Vec<String> = f
        .system
        .db()
        .faces()
        .filter_map(|face| face.families.first().map(|(n, _)| n.clone()))
        .collect();
    v.sort_by_key(|n| n.to_lowercase());
    v.dedup();
    v
}

/// A família está instalada? (vazio conta como a fonte padrão.)
pub fn font_installed(family: &str) -> bool {
    family.is_empty() || list_fonts().iter().any(|f| f.eq_ignore_ascii_case(family))
}

fn build(f: &mut Fonts, spec: &TextSpec) -> Buffer {
    let (size, lh, ls) = spec.sane();
    let mut buffer = Buffer::new(&mut f.system, Metrics::new(size, size * lh));
    buffer.set_size(spec.box_width.filter(|w| w.is_finite() && *w > 1.0), None);
    let family = if spec.family.is_empty() {
        Family::SansSerif
    } else {
        Family::Name(&spec.family)
    };
    let attrs = Attrs::new()
        .family(family)
        .weight(if spec.bold {
            Weight::BOLD
        } else {
            Weight::NORMAL
        })
        .style(if spec.italic {
            Style::Italic
        } else {
            Style::Normal
        })
        .letter_spacing(ls / size);
    let align = match spec.align {
        TextAlign::Left => Align::Left,
        TextAlign::Center => Align::Center,
        TextAlign::Right => Align::Right,
    };
    buffer.set_text(&spec.text, &attrs, Shaping::Advanced, Some(align));
    buffer.shape_until_scroll(&mut f.system, false);
    buffer
}

/// Tamanho do texto laid out (largura da linha mais larga, altura total).
pub fn text_size(spec: &TextSpec) -> (f32, f32) {
    let mut guard = fonts()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let buffer = build(&mut guard, spec);
    let (mut w, mut h) = (0f32, 0f32);
    for run in buffer.layout_runs() {
        w = w.max(run.line_w);
        h = h.max(run.line_top + run.line_height);
    }
    (w, h)
}

/// Caixa envolvente (x, y, largura, altura) do texto no documento.
pub fn text_bounds(spec: &TextSpec) -> (f32, f32, f32, f32) {
    let (w, h) = text_size(spec);
    let w = spec
        .box_width
        .filter(|b| b.is_finite() && *b > 1.0)
        .unwrap_or(w);
    (spec.x, spec.y, w, h)
}

/// Renderiza o texto em RGBA8 (não pré-multiplicado) do tamanho `w × h` do documento.
pub fn render_text(spec: &TextSpec, w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; w as usize * h as usize * 4];
    if spec.text.is_empty() || !spec.x.is_finite() || !spec.y.is_finite() {
        return out;
    }
    let mut guard = fonts()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = &mut *guard;
    let mut buffer = build(f, spec);
    let c = spec.color;
    let color = Color::rgba(c[0], c[1], c[2], c[3]);
    let (ox, oy) = (spec.x.round() as i64, spec.y.round() as i64);
    buffer.draw(&mut f.system, &mut f.cache, color, |x, y, gw, gh, col| {
        let a = col.a();
        if a == 0 {
            return;
        }
        for yy in 0..gh as i64 {
            for xx in 0..gw as i64 {
                let (px, py) = (ox + i64::from(x) + xx, oy + i64::from(y) + yy);
                if px < 0 || py < 0 || px >= i64::from(w) || py >= i64::from(h) {
                    continue;
                }
                let i = (py as usize * w as usize + px as usize) * 4;
                let dst = [out[i], out[i + 1], out[i + 2], out[i + 3]];
                let src = [col.r(), col.g(), col.b(), a];
                out[i..i + 4].copy_from_slice(&over(src, dst, 1.0));
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have_fonts() -> bool {
        let ok = !list_fonts().is_empty();
        if !ok {
            eprintln!("(sem fontes no sistema: teste de texto ignorado)");
        }
        ok
    }

    fn bbox(px: &[u8], w: u32) -> Option<(u32, u32, u32, u32)> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for (i, p) in px.as_chunks::<4>().0.iter().enumerate() {
            if p[3] > 0 {
                let (x, y) = (i as u32 % w, i as u32 / w);
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        (x0 != u32::MAX).then_some((x0, y0, x1, y1))
    }

    const W: u32 = 400;
    const H: u32 = 200;

    #[test]
    fn renders_accented_text_inside_bounds_without_color_halos() {
        if !have_fonts() {
            return;
        }
        let spec = TextSpec::point("Ação ÁÉÍÓÚ çãõ", 20.0, 30.0, [200, 40, 40, 255]);
        let px = render_text(&spec, W, H);
        let b = bbox(&px, W).expect("algo foi desenhado");
        assert!(b.0 >= 20 && b.1 >= 30, "{b:?}");
        let (tw, th) = text_size(&spec);
        assert!(tw > 50.0 && th >= 32.0, "{tw} {th}");
        for p in px.as_chunks::<4>().0 {
            if p[3] > 0 {
                assert_eq!(&p[..3], &[200, 40, 40], "cor pura, sem halo");
            }
        }
        assert_eq!(
            render_text(&TextSpec::point("", 0.0, 0.0, [0; 4]), 4, 4),
            vec![0; 64]
        );
    }

    #[test]
    fn size_bold_letter_spacing_and_line_height_change_layout() {
        if !have_fonts() {
            return;
        }
        let base = TextSpec::point("Orvalith", 0.0, 0.0, [0, 0, 0, 255]);
        let (w0, _) = text_size(&base);
        let big = TextSpec {
            size: 64.0,
            ..base.clone()
        };
        assert!(text_size(&big).0 > w0 * 1.7);
        let spaced = TextSpec {
            letter_spacing: 10.0,
            ..base.clone()
        };
        assert!(text_size(&spaced).0 > w0 + 40.0);
        let two = TextSpec {
            text: "Orva\nlith".into(),
            ..base.clone()
        };
        let (_, h1) = text_size(&base);
        let (_, h2) = text_size(&two);
        assert!(h2 > h1 * 1.8);
        let loose = TextSpec {
            line_height: 2.0,
            ..two.clone()
        };
        assert!(text_size(&loose).1 > h2 * 1.4);
        // Valores absurdos não derrubam o layout.
        let crazy = TextSpec {
            size: f32::NAN,
            line_height: -3.0,
            letter_spacing: f32::INFINITY,
            ..base.clone()
        };
        assert!(text_size(&crazy).0 > 0.0);
    }

    #[test]
    fn paragraph_wraps_and_alignment_moves_ink() {
        if !have_fonts() {
            return;
        }
        let text = "uma frase longa o bastante para quebrar em várias linhas dentro da caixa";
        let point = TextSpec::point(text, 10.0, 10.0, [0, 0, 0, 255]);
        let para = TextSpec {
            box_width: Some(160.0),
            ..point.clone()
        };
        let (_, hp) = text_size(&point);
        let (wq, hq) = text_size(&para);
        assert!(hq > hp * 2.0 && wq <= 161.0, "{wq} {hq} {hp}");
        let left = bbox(&render_text(&para, W, H), W).unwrap();
        let right = bbox(
            &render_text(
                &TextSpec {
                    align: TextAlign::Right,
                    ..para.clone()
                },
                W,
                H,
            ),
            W,
        )
        .unwrap();
        let center = bbox(
            &render_text(
                &TextSpec {
                    align: TextAlign::Center,
                    ..para.clone()
                },
                W,
                H,
            ),
            W,
        )
        .unwrap();
        assert!(
            right.2 >= center.2 && center.2 >= left.2,
            "{left:?} {center:?} {right:?}"
        );
        assert!(right.2 <= 170, "dentro da caixa (10 + 160)");
    }

    #[test]
    fn missing_font_is_detected_and_text_still_renders() {
        if !have_fonts() {
            return;
        }
        assert!(font_installed(""));
        assert!(!font_installed("Fonte Que Nao Existe 123"));
        let spec = TextSpec {
            family: "Fonte Que Nao Existe 123".into(),
            ..TextSpec::point("Olá", 5.0, 5.0, [0, 0, 0, 255])
        };
        assert!(
            bbox(&render_text(&spec, W, H), W).is_some(),
            "usa a fonte substituta"
        );
        let first = list_fonts().into_iter().next().unwrap();
        assert!(font_installed(&first.to_uppercase()));
    }

    #[test]
    fn text_is_clipped_to_the_canvas_and_json_roundtrips() {
        if !have_fonts() {
            return;
        }
        let spec = TextSpec::point("Cortado nas bordas", -30.0, -10.0, [0, 0, 0, 255]);
        let px = render_text(&spec, 100, 40);
        assert_eq!(px.len(), 100 * 40 * 4);
        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(serde_json::from_str::<TextSpec>(&json).unwrap(), spec);
    }
}
