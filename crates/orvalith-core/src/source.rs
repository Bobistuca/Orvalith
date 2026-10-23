//! Origem editável de uma camada: texto ou forma/caminho vetorial.
//!
//! Camadas com `source` guardam também os pixels renderizados; ao reabrir um projeto os
//! pixels gravados são usados mesmo que uma fonte esteja ausente. "Rasterizar" descarta a origem.

use serde::{Deserialize, Serialize};

use crate::adjust::{gradient_at, GradStop};
use crate::style::{gradient_t, Pattern};
use crate::text::{self, TextSpec};
use crate::vector::{self, ShapeSpec};

/// Limites para validar arquivos.
pub const MAX_TEXT_BYTES: usize = 1_000_000;
pub const MAX_NODES: usize = 100_000;
pub const MAX_BOOL_DEPTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LayerSource {
    Text(TextSpec),
    Shape(ShapeSpec),
    /// Camada de preenchimento (cor, gradiente ou padrão) do tamanho do documento.
    Fill(FillSpec),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FillSpec {
    Solid {
        color: [u8; 4],
    },
    Gradient {
        stops: Vec<GradStop>,
        angle: f32,
        radial: bool,
        reverse: bool,
    },
    Pattern {
        pattern: Pattern,
        scale: f32,
    },
}

pub const FILL_KINDS: [&str; 3] = ["Cor sólida", "Gradiente", "Padrão"];

impl FillSpec {
    pub fn neutral(kind: usize) -> Self {
        match kind {
            0 => FillSpec::Solid {
                color: [128, 128, 128, 255],
            },
            1 => FillSpec::Gradient {
                stops: vec![
                    GradStop {
                        pos: 0.0,
                        color: [20, 40, 120],
                    },
                    GradStop {
                        pos: 1.0,
                        color: [250, 180, 60],
                    },
                ],
                angle: 90.0,
                radial: false,
                reverse: false,
            },
            _ => FillSpec::Pattern {
                pattern: Pattern::default_of(0),
                scale: 16.0,
            },
        }
    }

    pub fn kind_index(&self) -> usize {
        match self {
            FillSpec::Solid { .. } => 0,
            FillSpec::Gradient { .. } => 1,
            FillSpec::Pattern { .. } => 2,
        }
    }

    pub fn render(&self, w: u32, h: u32) -> Vec<u8> {
        let (wu, hu) = (w as usize, h as usize);
        let mut out = Vec::with_capacity(wu * hu * 4);
        match self {
            FillSpec::Solid { color } => {
                for _ in 0..wu * hu {
                    out.extend_from_slice(color);
                }
            }
            FillSpec::Gradient {
                stops,
                angle,
                radial,
                reverse,
            } => {
                for y in 0..hu {
                    for x in 0..wu {
                        let t = gradient_t(
                            x as f32 + 0.5,
                            y as f32 + 0.5,
                            (0.0, 0.0, w as f32, h as f32),
                            *angle,
                            *radial,
                        );
                        let c = gradient_at(stops, if *reverse { 1.0 - t } else { t });
                        out.extend_from_slice(&c.map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8));
                        out.push(255);
                    }
                }
            }
            FillSpec::Pattern { pattern, scale } => {
                for y in 0..hu {
                    for x in 0..wu {
                        out.extend_from_slice(&pattern.color_at(
                            x as f32 + 0.5,
                            y as f32 + 0.5,
                            *scale,
                        ));
                        out.push(255);
                    }
                }
            }
        }
        out
    }

    fn is_sane(&self) -> bool {
        match self {
            FillSpec::Solid { .. } => true,
            FillSpec::Gradient { stops, angle, .. } => {
                stops.len() >= 2
                    && stops.len() <= 32
                    && angle.is_finite()
                    && stops.iter().all(|s| s.pos.is_finite())
            }
            FillSpec::Pattern { scale, .. } => {
                scale.is_finite() && *scale >= 1.0 && *scale <= 4096.0
            }
        }
    }
}

impl LayerSource {
    /// Renderiza a origem em RGBA8 do tamanho do documento.
    pub fn render(&self, w: u32, h: u32) -> Vec<u8> {
        match self {
            LayerSource::Text(t) => text::render_text(t, w, h),
            LayerSource::Shape(s) => vector::render_shape(s, w, h),
            LayerSource::Fill(f) => f.render(w, h),
        }
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        match self {
            LayerSource::Text(t) => {
                t.x += dx;
                t.y += dy;
            }
            LayerSource::Shape(s) => s.translate(dx, dy),
            LayerSource::Fill(_) => {}
        }
    }

    /// Caixa envolvente (x, y, w, h) no documento.
    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        match self {
            LayerSource::Text(t) => Some(text::text_bounds(t)),
            LayerSource::Shape(s) => s.bounds(),
            LayerSource::Fill(_) => None,
        }
    }

    /// A origem respeita os limites de tamanho (evita arquivos hostis)?
    pub fn is_sane(&self) -> bool {
        match self {
            LayerSource::Text(t) => {
                t.text.len() <= MAX_TEXT_BYTES && t.x.is_finite() && t.y.is_finite()
            }
            LayerSource::Shape(s) => shape_sane(s),
            LayerSource::Fill(f) => f.is_sane(),
        }
    }

    /// Nome sugerido para a camada.
    pub fn default_name(&self) -> String {
        match self {
            LayerSource::Text(t) => {
                let first = t.text.lines().next().unwrap_or("").trim();
                let name: String = first.chars().take(24).collect();
                if name.is_empty() {
                    "Texto".to_string()
                } else {
                    name
                }
            }
            LayerSource::Fill(f) => format!("Preenchimento: {}", FILL_KINDS[f.kind_index()]),
            LayerSource::Shape(s) => match &s.geom {
                vector::Geom::Rect { radius, .. } if *radius > 0.0 => "Retângulo arredondado",
                vector::Geom::Rect { .. } => "Retângulo",
                vector::Geom::Ellipse { .. } => "Elipse",
                vector::Geom::Polygon { .. } => "Polígono",
                vector::Geom::Star { .. } => "Estrela",
                vector::Geom::Line { .. } => "Linha",
                vector::Geom::Path(_) => "Caminho",
                vector::Geom::Boolean { .. } => "Forma combinada",
            }
            .to_string(),
        }
    }
}

/// Valida uma forma: profundidade, nós e números finitos.
pub fn shape_sane(s: &ShapeSpec) -> bool {
    if s.depth() > MAX_BOOL_DEPTH || s.node_count() > MAX_NODES {
        return false;
    }
    s.bounds()
        .is_none_or(|(x, y, w, h)| x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::{BoolOp, Geom};

    #[test]
    fn sanity_limits_and_names() {
        let rect = ShapeSpec::solid(
            Geom::Rect {
                x: 0.0,
                y: 0.0,
                w: 4.0,
                h: 4.0,
                radius: 0.0,
            },
            [0; 4],
        );
        assert!(LayerSource::Shape(rect.clone()).is_sane());
        let mut deep = rect.clone();
        for _ in 0..10 {
            deep = ShapeSpec::boolean(BoolOp::Union, deep, rect.clone());
        }
        assert!(
            !LayerSource::Shape(deep).is_sane(),
            "booleanas fundo demais"
        );
        let big = TextSpec::point("x".repeat(MAX_TEXT_BYTES + 1), 0.0, 0.0, [0; 4]);
        assert!(!LayerSource::Text(big).is_sane());
        let nan = ShapeSpec::solid(
            Geom::Rect {
                x: f32::NAN,
                y: 0.0,
                w: 1.0,
                h: 1.0,
                radius: 0.0,
            },
            [0; 4],
        );
        assert!(!LayerSource::Shape(nan).is_sane());
        assert_eq!(LayerSource::Shape(rect).default_name(), "Retângulo");
        let t = TextSpec::point(
            "  Título muito comprido que passa de vinte e quatro letras  \nlinha 2",
            0.0,
            0.0,
            [0; 4],
        );
        assert_eq!(LayerSource::Text(t).default_name().chars().count(), 24);
        assert_eq!(
            LayerSource::Text(TextSpec::point("", 0.0, 0.0, [0; 4])).default_name(),
            "Texto"
        );
    }
}
