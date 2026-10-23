//! Motor de traço: pincel, lápis e borracha.
//!
//! Um traço contínuo é uma `StrokeSession`: carimbos ("dabs") são espaçados ao longo do
//! caminho, interpolando entre as amostras do mouse (sem lacunas em movimentos rápidos).
//! A cobertura do traço é acumulada em um buffer próprio com o **fluxo** de cada carimbo;
//! a **opacidade** é aplicada uma única vez sobre o estado original da camada, de modo que
//! sobrepor carimbos no mesmo traço nunca excede a opacidade configurada.
//! Ao terminar, o traço vira um único `PixelPatch` (uma ação de histórico).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::compose::over;
use crate::document::Layer;
use crate::edit::PixelPatch;
use crate::geom::Rect;

/// Parâmetros de pincel (também usados em presets).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushParams {
    /// Diâmetro em pixels do documento.
    pub size: f32,
    /// 0 = bordas bem suaves, 1 = bordas duras (com antisserrilhado).
    pub hardness: f32,
    /// Opacidade máxima do traço.
    pub opacity: f32,
    /// Acúmulo por carimbo.
    pub flow: f32,
    /// Distância entre carimbos como fração do diâmetro.
    pub spacing: f32,
    /// Estabilização do traço (0 = sem suavização).
    pub smoothing: f32,
}

impl Default for BrushParams {
    fn default() -> Self {
        Self {
            size: 20.0,
            hardness: 0.8,
            opacity: 1.0,
            flow: 1.0,
            spacing: 0.15,
            smoothing: 0.0,
        }
    }
}

impl BrushParams {
    /// Força os valores a faixas válidas (inclusive vindos de arquivos editados).
    pub fn sanitized(self) -> Self {
        let d = Self::default();
        let f = |v: f32, def: f32, lo: f32, hi: f32| {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                def
            }
        };
        Self {
            size: f(self.size, d.size, 1.0, 2000.0),
            hardness: f(self.hardness, d.hardness, 0.0, 1.0),
            opacity: f(self.opacity, d.opacity, 0.0, 1.0),
            flow: f(self.flow, d.flow, 0.01, 1.0),
            spacing: f(self.spacing, d.spacing, 0.01, 2.0),
            smoothing: f(self.smoothing, d.smoothing, 0.0, 0.95),
        }
    }
}

/// Preset nomeado de pincel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrushPreset {
    pub name: String,
    pub params: BrushParams,
}

/// Presets básicos incluídos.
pub fn builtin_presets() -> Vec<BrushPreset> {
    let p = |name: &str, size, hardness, opacity, flow, spacing| BrushPreset {
        name: name.to_string(),
        params: BrushParams {
            size,
            hardness,
            opacity,
            flow,
            spacing,
            smoothing: 0.0,
        },
    };
    vec![
        p("Redondo duro", 20.0, 1.0, 1.0, 1.0, 0.1),
        p("Redondo suave", 40.0, 0.2, 1.0, 1.0, 0.12),
        p("Aerógrafo", 60.0, 0.0, 1.0, 0.15, 0.08),
        p("Marcador translúcido", 30.0, 0.9, 0.5, 1.0, 0.1),
        p("Fino", 3.0, 1.0, 1.0, 1.0, 0.1),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeMode {
    Paint,
    Erase,
}

/// Traço em andamento sobre uma camada.
pub struct StrokeSession {
    params: BrushParams,
    color: [u8; 4],
    mode: StrokeMode,
    pencil: bool,
    lock_alpha: bool,
    label: &'static str,
    w: u32,
    h: u32,
    snapshot: Arc<Vec<u8>>,
    coverage: Vec<f32>,
    dirty: Option<Rect>,
    /// Último ponto (suavizado) já percorrido pelo caminho.
    reached: Option<(f32, f32)>,
    /// Distância até o próximo carimbo ao longo do caminho.
    next_in: f32,
    target: (f32, f32),
    pressure: f32,
}

impl StrokeSession {
    pub fn begin(
        layer: &Layer,
        params: BrushParams,
        color: [u8; 4],
        mode: StrokeMode,
        pencil: bool,
        label: &'static str,
    ) -> Self {
        Self {
            params: params.sanitized(),
            color,
            mode,
            pencil,
            lock_alpha: false,
            label,
            w: layer.width,
            h: layer.height,
            snapshot: layer.pixels.clone(),
            coverage: vec![0.0; layer.width as usize * layer.height as usize],
            dirty: None,
            reached: None,
            next_in: 0.0,
            target: (0.0, 0.0),
            pressure: 1.0,
        }
    }

    /// Bloqueio de transparência: pinta só onde já há pixels, sem mudar o alfa; borracha não age.
    pub fn with_alpha_lock(mut self, lock: bool) -> Self {
        self.lock_alpha = lock;
        self
    }

    /// Adiciona uma amostra (coordenadas do documento; `pressure` em 0..=1) e pinta na
    /// camada. Retorna a área alterada por esta chamada.
    pub fn add_point(&mut self, layer: &mut Layer, x: f32, y: f32, pressure: f32) -> Option<Rect> {
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        self.target = (x, y);
        self.pressure = pressure.clamp(0.05, 1.0);
        let to = match self.reached {
            None => (x, y),
            Some(prev) => {
                let k = 1.0 - self.params.smoothing;
                (prev.0 + (x - prev.0) * k, prev.1 + (y - prev.1) * k)
            }
        };
        self.advance(layer, to)
    }

    fn spacing_px(&self) -> f32 {
        let s = self.params.size * self.pressure * self.params.spacing;
        if self.pencil {
            s.clamp(0.5, 1.0)
        } else {
            s.max(0.5)
        }
    }

    fn advance(&mut self, layer: &mut Layer, to: (f32, f32)) -> Option<Rect> {
        let mut changed: Option<Rect> = None;
        let mut merge = |r: Option<Rect>| {
            if let Some(r) = r {
                changed = Some(changed.map_or(r, |c| c.union(r)));
            }
        };
        match self.reached {
            None => {
                merge(self.stamp(layer, to.0, to.1));
                self.next_in = self.spacing_px();
                self.reached = Some(to);
            }
            Some(from) => {
                let (dx, dy) = (to.0 - from.0, to.1 - from.1);
                let len = dx.hypot(dy);
                if len < 1e-4 {
                    return None;
                }
                let mut d = self.next_in;
                while d <= len {
                    let t = d / len;
                    merge(self.stamp(layer, from.0 + dx * t, from.1 + dy * t));
                    d += self.spacing_px();
                }
                self.next_in = d - len;
                self.reached = Some(to);
            }
        }
        changed
    }

    /// Carimba um disco em (cx, cy) e recompõe os pixels afetados a partir do original.
    fn stamp(&mut self, layer: &mut Layer, cx: f32, cy: f32) -> Option<Rect> {
        let (cx, cy) = if self.pencil {
            (cx.floor() + 0.5, cy.floor() + 0.5)
        } else {
            (cx, cy)
        };
        let r = (self.params.size * self.pressure / 2.0).max(0.5);
        let x0 = (cx - r - 1.0).floor().max(0.0) as i64;
        let y0 = (cy - r - 1.0).floor().max(0.0) as i64;
        let x1 = ((cx + r + 1.0).ceil() as i64).min(i64::from(self.w));
        let y1 = ((cy + r + 1.0).ceil() as i64).min(i64::from(self.h));
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let pixels = layer.pixels_mut();
        let flow = if self.pencil { 1.0 } else { self.params.flow };
        let opacity = self.params.opacity;
        for py in y0..y1 {
            for px in x0..x1 {
                let dist = (px as f32 + 0.5 - cx).hypot(py as f32 + 0.5 - cy);
                let cov = dab_coverage(dist, r, self.params.hardness, self.pencil);
                if cov <= 0.0 {
                    continue;
                }
                let i = py as usize * self.w as usize + px as usize;
                let a = cov * flow;
                self.coverage[i] = 1.0 - (1.0 - self.coverage[i]) * (1.0 - a);
                let c = self.coverage[i] * opacity;
                let p = i * 4;
                let dst = [
                    self.snapshot[p],
                    self.snapshot[p + 1],
                    self.snapshot[p + 2],
                    self.snapshot[p + 3],
                ];
                let out = match self.mode {
                    StrokeMode::Paint if self.lock_alpha => {
                        let t = c * f32::from(self.color[3]) / 255.0;
                        let mix = |d: u8, s: u8| {
                            (f32::from(d) + (f32::from(s) - f32::from(d)) * t)
                                .round()
                                .clamp(0.0, 255.0) as u8
                        };
                        if dst[3] == 0 {
                            dst
                        } else {
                            [
                                mix(dst[0], self.color[0]),
                                mix(dst[1], self.color[1]),
                                mix(dst[2], self.color[2]),
                                dst[3],
                            ]
                        }
                    }
                    StrokeMode::Erase if self.lock_alpha => dst,
                    StrokeMode::Paint => {
                        let alpha = (c * f32::from(self.color[3])).round().clamp(0.0, 255.0) as u8;
                        over(
                            [self.color[0], self.color[1], self.color[2], alpha],
                            dst,
                            1.0,
                        )
                    }
                    StrokeMode::Erase => [
                        dst[0],
                        dst[1],
                        dst[2],
                        (f32::from(dst[3]) * (1.0 - c)).round().clamp(0.0, 255.0) as u8,
                    ],
                };
                pixels[p..p + 4].copy_from_slice(&out);
            }
        }
        let rect = Rect::new(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
        self.dirty = Some(self.dirty.map_or(rect, |d| d.union(rect)));
        Some(rect)
    }

    /// Conclui o traço: completa o caminho até o último ponto e devolve o retalho de
    /// histórico (uma única ação) mais a área final alterada.
    pub fn finish(mut self, layer: &mut Layer) -> Option<PixelPatch> {
        if self.reached.is_some_and(|r| r != self.target) {
            let t = self.target;
            self.advance(layer, t);
        }
        let rect = self.dirty?;
        PixelPatch::from_diff(self.label, layer, &self.snapshot, rect)
    }

    pub fn dirty(&self) -> Option<Rect> {
        self.dirty
    }
}

fn dab_coverage(dist: f32, r: f32, hardness: f32, pencil: bool) -> f32 {
    if pencil {
        return if dist <= r { 1.0 } else { 0.0 };
    }
    let edge = (r + 0.5 - dist).clamp(0.0, 1.0);
    let inner = if hardness >= 0.999 {
        1.0
    } else {
        let t = ((dist / r - hardness) / (1.0 - hardness)).clamp(0.0, 1.0);
        1.0 - t * t * (3.0 - 2.0 * t)
    };
    edge * inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::View;
    use crate::{Document, History};

    fn hard(size: f32) -> BrushParams {
        BrushParams {
            size,
            hardness: 1.0,
            ..BrushParams::default()
        }
    }

    fn red() -> [u8; 4] {
        [255, 0, 0, 255]
    }

    fn stroke(layer: &mut Layer, p: BrushParams, pts: &[(f32, f32)]) -> Option<PixelPatch> {
        let mut s = StrokeSession::begin(layer, p, red(), StrokeMode::Paint, false, "Pincel");
        for &(x, y) in pts {
            s.add_point(layer, x, y, 1.0);
        }
        s.finish(layer)
    }

    #[test]
    fn fast_stroke_leaves_no_gaps() {
        let mut layer = Layer::transparent("l", 100, 20);
        // Apenas duas amostras muito distantes, como um movimento rápido do mouse.
        stroke(&mut layer, hard(4.0), &[(2.0, 10.0), (97.0, 10.0)]);
        for x in 3..=96 {
            assert_eq!(layer.pixel(x, 10).unwrap()[3], 255, "lacuna em x={x}");
        }
    }

    #[test]
    fn opacity_caps_overlapping_dabs_within_one_stroke() {
        let mut layer = Layer::transparent("l", 40, 40);
        let p = BrushParams {
            opacity: 0.5,
            ..hard(10.0)
        };
        // Vai e volta sobre a mesma região.
        stroke(
            &mut layer,
            p,
            &[(10.0, 20.0), (30.0, 20.0), (10.0, 20.0), (30.0, 20.0)],
        );
        let a = layer.pixel(20, 20).unwrap()[3];
        assert!((127..=129).contains(&a), "alfa {a}");
    }

    #[test]
    fn flow_accumulates_per_dab() {
        let mut layer = Layer::transparent("l", 40, 40);
        let p = BrushParams {
            flow: 0.2,
            ..hard(10.0)
        };
        stroke(&mut layer, p, &[(10.0, 20.0), (30.0, 20.0)]);
        let a = layer.pixel(20, 20).unwrap()[3];
        assert!(a > 51 && a < 255, "alfa {a}");
    }

    #[test]
    fn soft_brush_fades_to_edge() {
        let mut layer = Layer::transparent("l", 60, 60);
        let p = BrushParams {
            size: 40.0,
            hardness: 0.0,
            ..BrushParams::default()
        };
        stroke(&mut layer, p, &[(30.0, 30.0)]);
        let center = layer.pixel(30, 30).unwrap()[3];
        let mid = layer.pixel(40, 30).unwrap()[3];
        let edge = layer.pixel(49, 30).unwrap()[3];
        assert!(
            center > 240 && mid < center && edge < mid,
            "{center} {mid} {edge}"
        );
    }

    #[test]
    fn pencil_has_no_antialiasing() {
        let mut layer = Layer::transparent("l", 30, 30);
        let mut s =
            StrokeSession::begin(&layer, hard(7.0), red(), StrokeMode::Paint, true, "Lápis");
        s.add_point(&mut layer, 10.3, 10.7, 1.0);
        s.add_point(&mut layer, 20.0, 15.0, 1.0);
        s.finish(&mut layer);
        assert!(layer
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] == 0 || p[3] == 255));
        assert_eq!(layer.pixel(10, 10).unwrap()[3], 255);
    }

    #[test]
    fn eraser_reduces_alpha_and_leaves_transparent_untouched() {
        let mut layer = Layer::filled("l", 20, 20, [0, 255, 0, 255]);
        layer.set_pixel(0, 0, [0, 0, 0, 0]);
        let mut s = StrokeSession::begin(
            &layer,
            BrushParams {
                opacity: 0.5,
                ..hard(30.0)
            },
            red(),
            StrokeMode::Erase,
            false,
            "Borracha",
        );
        s.add_point(&mut layer, 10.0, 10.0, 1.0);
        s.finish(&mut layer);
        let a = layer.pixel(10, 10).unwrap()[3];
        assert!((127..=129).contains(&a), "alfa {a}");
        assert_eq!(layer.pixel(0, 0).unwrap()[3], 0);
    }

    #[test]
    fn alpha_lock_paints_only_existing_pixels_and_keeps_alpha() {
        let mut layer = Layer::transparent("l", 20, 10);
        for x in 0..10 {
            layer.set_pixel(x, 5, [0, 0, 255, 128]);
        }
        let mut s = StrokeSession::begin(&layer, hard(30.0), red(), StrokeMode::Paint, false, "x")
            .with_alpha_lock(true);
        s.add_point(&mut layer, 10.0, 5.0, 1.0);
        s.finish(&mut layer);
        assert_eq!(layer.pixel(3, 5).unwrap(), [255, 0, 0, 128]);
        assert_eq!(layer.pixel(15, 5).unwrap()[3], 0);
        assert_eq!(layer.pixel(3, 4).unwrap()[3], 0);
        let mut e = StrokeSession::begin(&layer, hard(30.0), red(), StrokeMode::Erase, false, "x")
            .with_alpha_lock(true);
        let before = layer.pixels.clone();
        e.add_point(&mut layer, 10.0, 5.0, 1.0);
        e.finish(&mut layer);
        assert_eq!(layer.pixels, before);
    }

    #[test]
    fn whole_stroke_is_one_undo_step_and_restores_exactly() {
        let mut doc = Document::new("t", 50, 50, Some([255, 255, 255, 255])).unwrap();
        let original = doc.layers[0].pixels.clone();
        let mut layer = doc.layers[0].clone();
        let patch = stroke(
            &mut layer,
            hard(6.0),
            &[(5.0, 5.0), (25.0, 30.0), (45.0, 10.0)],
        )
        .unwrap();
        doc.layers[0] = layer;
        let painted = doc.layers[0].pixels.clone();
        assert_ne!(painted, original);
        let mut h = History::new();
        h.record(&mut doc, Box::new(patch));
        assert_eq!(h.labels().len(), 1);
        h.undo(&mut doc);
        assert_eq!(doc.layers[0].pixels, original);
        h.redo(&mut doc);
        assert_eq!(doc.layers[0].pixels, painted);
    }

    #[test]
    fn stroke_partially_outside_canvas_is_clipped() {
        let mut layer = Layer::transparent("l", 20, 20);
        stroke(
            &mut layer,
            hard(10.0),
            &[(-5.0, 10.0), (3.0, 10.0), (30.0, 10.0)],
        );
        assert_eq!(layer.pixel(0, 10).unwrap()[3], 255);
        assert_eq!(layer.pixel(19, 10).unwrap()[3], 255);
        // Totalmente fora: nada acontece e nenhum retalho é gerado.
        let mut other = Layer::transparent("o", 20, 20);
        assert!(stroke(&mut other, hard(4.0), &[(-50.0, -50.0), (-40.0, -40.0)]).is_none());
    }

    #[test]
    fn smoothing_still_reaches_final_point_on_finish() {
        let mut layer = Layer::transparent("l", 60, 20);
        let p = BrushParams {
            smoothing: 0.9,
            ..hard(4.0)
        };
        stroke(&mut layer, p, &[(5.0, 10.0), (50.0, 10.0)]);
        assert_eq!(layer.pixel(50, 10).unwrap()[3], 255);
        assert_eq!(layer.pixel(30, 10).unwrap()[3], 255);
    }

    #[test]
    fn same_document_point_paints_same_pixel_at_any_zoom() {
        for zoom in [0.25_f32, 1.0, 3.0, 17.0] {
            let v = View::centered([100, 100], [800.0, 600.0], zoom);
            let screen = v.doc_to_view([50.5, 40.5]);
            let doc = v.view_to_doc(screen);
            let mut layer = Layer::transparent("l", 100, 100);
            let mut s =
                StrokeSession::begin(&layer, hard(1.0), red(), StrokeMode::Paint, true, "x");
            s.add_point(&mut layer, doc[0], doc[1], 1.0);
            s.finish(&mut layer);
            assert_eq!(layer.pixel(50, 40).unwrap()[3], 255, "zoom {zoom}");
            let painted = layer
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] > 0)
                .count();
            assert_eq!(painted, 1, "zoom {zoom}");
        }
    }

    #[test]
    fn painted_pixels_survive_project_roundtrip() {
        let mut doc = Document::new("t", 30, 30, None).unwrap();
        let mut layer = doc.layers[0].clone();
        stroke(&mut layer, hard(5.0), &[(5.0, 5.0), (20.0, 20.0)]);
        doc.layers[0] = layer;
        let back = crate::orva::read(&crate::orva::write(&doc).unwrap()).unwrap();
        assert_eq!(back.layers[0].pixels, doc.layers[0].pixels);
    }

    #[test]
    fn sanitize_rejects_nan_and_out_of_range() {
        let p = BrushParams {
            size: f32::NAN,
            hardness: 5.0,
            opacity: -1.0,
            flow: 0.0,
            spacing: 0.0,
            smoothing: 7.0,
        }
        .sanitized();
        assert_eq!(p.size, BrushParams::default().size);
        assert_eq!(
            (p.hardness, p.opacity, p.flow, p.spacing, p.smoothing),
            (1.0, 0.0, 0.01, 0.01, 0.95)
        );
    }

    #[test]
    fn presets_serialize_roundtrip() {
        let presets = builtin_presets();
        let json = serde_json::to_string(&presets).unwrap();
        let back: Vec<BrushPreset> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, presets);
    }
}
