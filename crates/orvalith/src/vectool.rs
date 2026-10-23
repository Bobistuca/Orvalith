//! Ferramentas de texto, formas, caneta e seleção de nós (interface).
//!
//! A lógica de renderização e edição está em `orvalith_core::{vector, text, vecops}`; aqui
//! ficam os gestos, a edição de texto no canvas e os painéis de propriedades.

use std::sync::OnceLock;

use eframe::egui;
use orvalith_core::geom::Rect;
use orvalith_core::text::{self, TextAlign, TextSpec};
use orvalith_core::vector::{BezierPath, Fill, Geom, PathNode, ShapeSpec, StrokeSpec};
use orvalith_core::{vecops, Layer, LayerId, LayerSource};

use crate::canvas::{CanvasOut, OpenDoc, Shape};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeKind {
    Rect,
    RoundRect,
    Ellipse,
    Polygon,
    Star,
    Line,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 6] = [
        ShapeKind::Rect,
        ShapeKind::RoundRect,
        ShapeKind::Ellipse,
        ShapeKind::Polygon,
        ShapeKind::Star,
        ShapeKind::Line,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Rect => "Retângulo",
            ShapeKind::RoundRect => "Retângulo arredondado",
            ShapeKind::Ellipse => "Elipse",
            ShapeKind::Polygon => "Polígono",
            ShapeKind::Star => "Estrela",
            ShapeKind::Line => "Linha",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FillMode {
    None,
    Solid,
    Gradient,
}

/// Famílias instaladas (carregadas uma vez; a primeira chamada inicializa o sistema de fontes).
pub fn font_list() -> &'static Vec<String> {
    static FONTS: OnceLock<Vec<String>> = OnceLock::new();
    FONTS.get_or_init(text::list_fonts)
}

struct TextEditing {
    layer: LayerId,
    before: Vec<Layer>,
    created: bool,
}

struct PenSession {
    path: BezierPath,
    dragging: bool,
}

#[derive(Clone, Copy)]
enum NodeDrag {
    Anchor(usize, (f32, f32)),
    Handle(usize, bool),
}

pub struct VecState {
    pub shape_kind: ShapeKind,
    pub radius: f32,
    pub sides: u32,
    pub star_points: u32,
    pub star_inner: f32,
    pub fill_mode: FillMode,
    pub stroke_on: bool,
    pub stroke_width: f32,
    /// Padrões do próximo texto criado.
    pub text: TextSpec,
    text_edit: Option<TextEditing>,
    shape_drag: Option<((f32, f32), (f32, f32))>,
    pen: Option<PenSession>,
    node_sel: Option<usize>,
    node_drag: Option<NodeDrag>,
    node_before: Option<Vec<Layer>>,
    font_filter: String,
    /// O editor de texto deve receber o foco no próximo quadro.
    pub text_focus_pending: bool,
}

impl Default for VecState {
    fn default() -> Self {
        Self {
            shape_kind: ShapeKind::Rect,
            radius: 16.0,
            sides: 6,
            star_points: 5,
            star_inner: 0.45,
            fill_mode: FillMode::Solid,
            stroke_on: false,
            stroke_width: 4.0,
            text: TextSpec::point("", 0.0, 0.0, [0, 0, 0, 255]),
            text_edit: None,
            shape_drag: None,
            pen: None,
            node_sel: None,
            node_drag: None,
            node_before: None,
            font_filter: String::new(),
            text_focus_pending: false,
        }
    }
}

type Pt = (f32, f32);

fn dist(a: Pt, b: Pt) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn full(open: &OpenDoc) -> Rect {
    Rect::full(open.doc.width, open.doc.height)
}

/// Aplica diretamente uma nova lista de camadas (prévia viva, sem histórico).
fn set_layers_live(open: &mut OpenDoc, layers: Vec<Layer>) {
    open.doc.layers = layers;
    let r = full(open);
    open.mark_dirty(r);
}

/// Pontos de um caminho Bézier achatados em linha poligonal (para o contorno na tela).
fn flatten(path: &BezierPath) -> Vec<Pt> {
    let mut pts = Vec::new();
    for i in 0..path.segment_count() {
        if let Some(c) = path.segment(i) {
            for k in 0..=16 {
                let t = k as f32 / 16.0;
                let u = 1.0 - t;
                pts.push((
                    u * u * u * c[0].0
                        + 3.0 * u * u * t * c[1].0
                        + 3.0 * u * t * t * c[2].0
                        + t * t * t * c[3].0,
                    u * u * u * c[0].1
                        + 3.0 * u * u * t * c[1].1
                        + 3.0 * u * t * t * c[2].1
                        + t * t * t * c[3].1,
                ));
            }
        }
    }
    if pts.is_empty() {
        pts.extend(path.nodes.iter().map(PathNode::pos));
    }
    pts
}

impl VecState {
    // ---------- Fábrica de especificações ----------

    fn style(
        &self,
        area: bool,
        bounds: (f32, f32, f32, f32),
        fg: [u8; 4],
        bg: [u8; 4],
    ) -> (Fill, Option<StrokeSpec>) {
        let fill = if !area {
            Fill::None
        } else {
            match self.fill_mode {
                FillMode::None => Fill::None,
                FillMode::Solid => Fill::Solid(fg),
                FillMode::Gradient => Fill::Linear {
                    x0: bounds.0,
                    y0: bounds.1,
                    x1: bounds.0 + bounds.2,
                    y1: bounds.1 + bounds.3,
                    c0: fg,
                    c1: bg,
                },
            }
        };
        let stroke = if !area {
            Some(StrokeSpec {
                color: fg,
                width: self.stroke_width,
            })
        } else if self.stroke_on || matches!(fill, Fill::None) {
            Some(StrokeSpec {
                color: bg,
                width: self.stroke_width,
            })
        } else {
            None
        };
        (fill, stroke)
    }

    /// Forma arrastada de `a` até `b` (Shift: proporção 1:1).
    pub fn build_shape(
        &self,
        a: Pt,
        b: Pt,
        shift: bool,
        fg: [u8; 4],
        bg: [u8; 4],
    ) -> Option<ShapeSpec> {
        let (mut dx, mut dy) = (b.0 - a.0, b.1 - a.1);
        if shift
            && !matches!(
                self.shape_kind,
                ShapeKind::Line | ShapeKind::Polygon | ShapeKind::Star
            )
        {
            let m = dx.abs().max(dy.abs());
            dx = m * dx.signum();
            dy = m * dy.signum();
        }
        let (x, y, w, h) = (a.0.min(a.0 + dx), a.1.min(a.1 + dy), dx.abs(), dy.abs());
        let radial = dist(a, b);
        let geom = match self.shape_kind {
            ShapeKind::Rect => Geom::Rect {
                x,
                y,
                w,
                h,
                radius: 0.0,
            },
            ShapeKind::RoundRect => Geom::Rect {
                x,
                y,
                w,
                h,
                radius: self.radius,
            },
            ShapeKind::Ellipse => Geom::Ellipse { x, y, w, h },
            ShapeKind::Polygon => Geom::Polygon {
                cx: a.0,
                cy: a.1,
                radius: radial,
                sides: self.sides,
                rotation: 0.0,
            },
            ShapeKind::Star => Geom::Star {
                cx: a.0,
                cy: a.1,
                outer: radial,
                inner: radial * self.star_inner,
                points: self.star_points,
                rotation: 0.0,
            },
            ShapeKind::Line => Geom::Line {
                x1: a.0,
                y1: a.1,
                x2: dx + a.0,
                y2: dy + a.1,
            },
        };
        let degenerate = match self.shape_kind {
            ShapeKind::Polygon | ShapeKind::Star => radial < 1.0,
            ShapeKind::Line => radial < 1.0,
            _ => w < 1.0 || h < 1.0,
        };
        if degenerate {
            return None;
        }
        let area = !matches!(self.shape_kind, ShapeKind::Line);
        let bounds = ShapeSpec::solid(geom.clone(), [0; 4])
            .bounds()
            .unwrap_or((x, y, w, h));
        let (fill, stroke) = self.style(area, bounds, fg, bg);
        Some(ShapeSpec { geom, fill, stroke })
    }

    /// Cria uma camada de forma (uma ação de histórico).
    pub fn add_layer(open: &mut OpenDoc, source: LayerSource, label: &str) {
        let above = open.active_idx().map(|i| open.doc.layers[i].id);
        let o = vecops::new_source_layer(&open.doc, source, above);
        let before = open.doc.layers.clone();
        open.history.execute(
            &mut open.doc,
            Box::new(orvalith_core::edit::LayersEdit::new(
                label, before, o.layers,
            )),
        );
        open.selected = o.select;
        if let Some(a) = o.active {
            open.active_layer = a;
        }
    }

    // ---------- Forma ----------

    pub fn handle_shape(&mut self, open: &mut OpenDoc, out: &CanvasOut, fg: [u8; 4], bg: [u8; 4]) {
        if let Some(p) = out.pressed {
            let p = (p[0], p[1]);
            self.shape_drag = Some((p, p));
        }
        if let (Some(m), Some((_, end))) = (out.moves.last(), self.shape_drag.as_mut()) {
            *end = (m[0], m[1]);
        }
        if out.released || !out.down {
            if let Some((a, b)) = self.shape_drag.take() {
                if let Some(spec) = self.build_shape(a, b, out.shift, fg, bg) {
                    Self::add_layer(open, LayerSource::Shape(spec), "Forma");
                }
            }
        }
    }

    // ---------- Caneta ----------

    pub fn handle_pen(&mut self, open: &mut OpenDoc, out: &CanvasOut, fg: [u8; 4], bg: [u8; 4]) {
        if let Some(p) = out.pressed {
            let pos = (p[0], p[1]);
            let hit = 10.0 / out.zoom.max(0.001);
            let s = self.pen.get_or_insert_with(|| PenSession {
                path: BezierPath::default(),
                dragging: false,
            });
            if s.path.nodes.len() >= 3 && dist(pos, s.path.nodes[0].pos()) <= hit {
                s.path.closed = true;
                self.commit_pen(open, fg, bg);
                return;
            }
            s.path.nodes.push(PathNode::corner(pos.0, pos.1));
            s.dragging = true;
        } else if let Some(s) = self.pen.as_mut() {
            if s.dragging && out.down {
                if let (Some(m), Some(n)) = (out.moves.last(), s.path.nodes.last_mut()) {
                    let v = (m[0] - n.x, m[1] - n.y);
                    n.hout = v;
                    n.hin = (-v.0, -v.1);
                }
            }
            if out.released || !out.down {
                s.dragging = false;
            }
        }
    }

    /// Finaliza o caminho em criação (Enter ou clique no primeiro ponto).
    pub fn commit_pen(&mut self, open: &mut OpenDoc, fg: [u8; 4], bg: [u8; 4]) {
        let Some(s) = self.pen.take() else { return };
        if s.path.nodes.len() < 2 {
            return;
        }
        let area = s.path.closed;
        let bounds = ShapeSpec::solid(Geom::Path(s.path.clone()), [0; 4])
            .bounds()
            .unwrap_or((0.0, 0.0, 1.0, 1.0));
        let (fill, stroke) = self.style(area, bounds, fg, bg);
        let spec = ShapeSpec {
            geom: Geom::Path(s.path),
            fill,
            stroke,
        };
        Self::add_layer(open, LayerSource::Shape(spec), "Caminho");
    }

    pub fn cancel_pen(&mut self) {
        self.pen = None;
    }

    // ---------- Seleção de nós ----------

    fn active_path(open: &OpenDoc) -> Option<(usize, BezierPath)> {
        let idx = open.active_idx()?;
        match &open.doc.layers[idx].source {
            Some(LayerSource::Shape(ShapeSpec {
                geom: Geom::Path(p),
                ..
            })) => Some((idx, p.clone())),
            _ => None,
        }
    }

    fn write_path(open: &mut OpenDoc, idx: usize, path: BezierPath) {
        let (w, h) = (open.doc.width, open.doc.height);
        if let Some(LayerSource::Shape(s)) = open.doc.layers[idx].source.as_mut() {
            s.geom = Geom::Path(path);
        }
        open.doc.layers[idx].rerender(w, h);
        let r = Rect::full(w, h);
        open.mark_dirty(r);
    }

    pub fn handle_node(&mut self, open: &mut OpenDoc, out: &CanvasOut) {
        let Some((idx, mut path)) = Self::active_path(open) else {
            self.node_sel = None;
            return;
        };
        let hit = 9.0 / out.zoom.max(0.001);
        if let Some(p) = out.pressed {
            let p = (p[0], p[1]);
            self.node_before = Some(open.doc.layers.clone());
            self.node_drag = None;
            // Alças do nó selecionado têm prioridade.
            if let Some(i) = self.node_sel.filter(|i| *i < path.nodes.len()) {
                let n = path.nodes[i];
                if n.hout != (0.0, 0.0) && dist(p, n.out_abs()) <= hit {
                    self.node_drag = Some(NodeDrag::Handle(i, true));
                } else if n.hin != (0.0, 0.0) && dist(p, n.in_abs()) <= hit {
                    self.node_drag = Some(NodeDrag::Handle(i, false));
                }
            }
            if self.node_drag.is_none() {
                if let Some(i) = path.nodes.iter().position(|n| dist(p, n.pos()) <= hit) {
                    self.node_sel = Some(i);
                    if out.double_click {
                        let smooth =
                            path.nodes[i].hin == (0.0, 0.0) && path.nodes[i].hout == (0.0, 0.0);
                        path.set_smooth(i, smooth);
                        Self::write_path(open, idx, path);
                        self.finish_node(open);
                        return;
                    }
                    let n = path.nodes[i];
                    self.node_drag = Some(NodeDrag::Anchor(i, (p.0 - n.x, p.1 - n.y)));
                } else if let Some((seg, t)) = path.nearest_on_path(p, hit) {
                    if path.insert_node(seg, t) {
                        self.node_sel = Some(seg + 1);
                        let n = path.nodes[seg + 1];
                        self.node_drag = Some(NodeDrag::Anchor(seg + 1, (p.0 - n.x, p.1 - n.y)));
                        Self::write_path(open, idx, path.clone());
                    }
                } else {
                    self.node_sel = None;
                    self.node_before = None;
                }
            }
        }
        if let (Some(m), Some(drag)) = (out.moves.last(), self.node_drag) {
            let m = (m[0], m[1]);
            match drag {
                NodeDrag::Anchor(i, off) => {
                    path.nodes[i].x = m.0 - off.0;
                    path.nodes[i].y = m.1 - off.1;
                }
                NodeDrag::Handle(i, is_out) => {
                    let n = &mut path.nodes[i];
                    let v = (m.0 - n.x, m.1 - n.y);
                    let (this, other) = if is_out {
                        (&mut n.hout, &mut n.hin)
                    } else {
                        (&mut n.hin, &mut n.hout)
                    };
                    let was_smooth = {
                        let cross = this.0 * other.1 - this.1 * other.0;
                        *other != (0.0, 0.0)
                            && cross.abs()
                                < 0.05
                                    * this.0.hypot(this.1).max(1.0)
                                    * other.0.hypot(other.1).max(1.0)
                    };
                    *this = v;
                    if was_smooth && !out.alt {
                        let len = other.0.hypot(other.1);
                        let vl = v.0.hypot(v.1).max(1e-6);
                        *other = (-v.0 / vl * len, -v.1 / vl * len);
                    }
                }
            }
            Self::write_path(open, idx, path);
        }
        if (out.released || !out.down) && self.node_drag.is_some() {
            self.node_drag = None;
            self.finish_node(open);
        }
    }

    /// Registra a edição de nós como uma única ação.
    fn finish_node(&mut self, open: &mut OpenDoc) {
        if let Some(before) = self.node_before.take() {
            let changed = before
                .iter()
                .zip(&open.doc.layers)
                .any(|(a, b)| a.source != b.source);
            if changed {
                let r = full(open);
                open.record_layers("Editar caminho", before, r);
            }
        }
    }

    /// Remove o nó selecionado (tecla Delete com a ferramenta de nós).
    pub fn delete_selected_node(&mut self, open: &mut OpenDoc) -> bool {
        let (Some(i), Some((idx, mut path))) = (self.node_sel, Self::active_path(open)) else {
            return false;
        };
        let before = open.doc.layers.clone();
        if !path.remove_node(i) {
            return false;
        }
        Self::write_path(open, idx, path);
        self.node_sel = None;
        let r = full(open);
        open.record_layers("Excluir nó", before, r);
        true
    }

    // ---------- Texto ----------

    pub fn text_editing(&self) -> Option<LayerId> {
        self.text_edit.as_ref().map(|t| t.layer)
    }

    pub fn busy(&self) -> bool {
        self.text_edit.is_some() || self.pen.is_some()
    }

    fn text_spec_of(open: &OpenDoc, id: LayerId) -> Option<TextSpec> {
        match open.doc.layer(id)?.source.as_ref()? {
            LayerSource::Text(t) => Some(t.clone()),
            LayerSource::Shape(_) => None,
        }
    }

    /// Camada de texto mais alta sob o ponto.
    fn text_layer_at(open: &OpenDoc, p: Pt) -> Option<LayerId> {
        open.doc.layers.iter().rev().find_map(|l| {
            if !l.visible {
                return None;
            }
            let Some(LayerSource::Text(t)) = &l.source else {
                return None;
            };
            let (x, y, w, h) = text::text_bounds(t);
            (p.0 >= x - 2.0
                && p.0 <= x + w.max(8.0) + 2.0
                && p.1 >= y - 2.0
                && p.1 <= y + h.max(8.0) + 2.0)
                .then_some(l.id)
        })
    }

    pub fn begin_text_at(&mut self, open: &mut OpenDoc, p: Pt, color: [u8; 4]) {
        self.commit_text(open);
        let before = open.doc.layers.clone();
        let spec = TextSpec {
            x: p.0,
            y: p.1,
            color,
            text: String::new(),
            ..self.text.clone()
        };
        let above = open.active_idx().map(|i| open.doc.layers[i].id);
        let o = vecops::new_source_layer(&open.doc, LayerSource::Text(spec), above);
        let id = o.active.unwrap_or(open.active_layer);
        open.selected = o.select.clone();
        open.active_layer = id;
        set_layers_live(open, o.layers);
        self.text_edit = Some(TextEditing {
            layer: id,
            before,
            created: true,
        });
        self.text_focus_pending = true;
    }

    pub fn begin_text_edit(&mut self, open: &mut OpenDoc, id: LayerId) {
        self.commit_text(open);
        if Self::text_spec_of(open, id).is_none() {
            return;
        }
        open.selected = vec![id];
        open.active_layer = id;
        self.text_edit = Some(TextEditing {
            layer: id,
            before: open.doc.layers.clone(),
            created: false,
        });
        self.text_focus_pending = true;
    }

    /// Altera a especificação do texto em edição (prévia viva) e atualiza os padrões.
    pub fn edit_text_spec(&mut self, open: &mut OpenDoc, f: impl FnOnce(&mut TextSpec)) {
        let Some(id) = self.text_editing() else {
            return;
        };
        let Some(mut spec) = Self::text_spec_of(open, id) else {
            return;
        };
        f(&mut spec);
        self.text = TextSpec {
            text: String::new(),
            x: 0.0,
            y: 0.0,
            ..spec.clone()
        };
        if let Ok(layers) = vecops::update_source(&open.doc, id, LayerSource::Text(spec)) {
            set_layers_live(open, layers);
        }
    }

    pub fn current_text(&self, open: &OpenDoc) -> Option<String> {
        Self::text_spec_of(open, self.text_editing()?).map(|t| t.text)
    }

    /// Confirma a edição de texto como uma única ação (texto vazio em camada nova = cancela).
    pub fn commit_text(&mut self, open: &mut OpenDoc) {
        let Some(te) = self.text_edit.take() else {
            return;
        };
        let empty = Self::text_spec_of(open, te.layer).is_none_or(|t| t.text.trim().is_empty());
        if empty && te.created {
            open.doc.layers = te.before;
            open.active_layer = open.doc.layers.last().map_or(open.active_layer, |l| l.id);
            open.selected = vec![open.active_layer];
            let r = full(open);
            open.mark_dirty(r);
            return;
        }
        let changed = te.before.len() != open.doc.layers.len()
            || te
                .before
                .iter()
                .zip(&open.doc.layers)
                .any(|(a, b)| a.source != b.source);
        if changed {
            let label = if te.created { "Texto" } else { "Editar texto" };
            let r = full(open);
            open.record_layers(label, te.before, r);
        }
    }

    pub fn cancel_text(&mut self, open: &mut OpenDoc) {
        if let Some(te) = self.text_edit.take() {
            open.doc.layers = te.before;
            let r = full(open);
            open.mark_dirty(r);
        }
    }

    pub fn handle_text(&mut self, open: &mut OpenDoc, out: &CanvasOut, fg: [u8; 4]) {
        let Some(p) = out.pressed else { return };
        let p = (p[0], p[1]);
        match Self::text_layer_at(open, p) {
            Some(id) => self.begin_text_edit(open, id),
            None => self.begin_text_at(open, p, fg),
        }
    }

    /// Confirma tudo o que estiver em andamento (troca de ferramenta, salvar…).
    pub fn commit_all(&mut self, open: &mut OpenDoc, fg: [u8; 4], bg: [u8; 4]) {
        self.commit_text(open);
        self.commit_pen(open, fg, bg);
        self.node_drag = None;
    }

    // ---------- Contornos na tela ----------

    pub fn shape_preview(&self, fg: [u8; 4], bg: [u8; 4], shift: bool) -> Option<Shape> {
        if let Some((a, b)) = self.shape_drag {
            let spec = self.build_shape(a, b, shift, fg, bg)?;
            return Some(match spec.geom {
                Geom::Ellipse { x, y, w, h } => Shape::Ellipse((x, y), (x + w, y + h)),
                Geom::Rect { x, y, w, h, .. } => Shape::Rect((x, y), (x + w, y + h)),
                Geom::Line { x1, y1, x2, y2 } => Shape::Poly(vec![(x1, y1), (x2, y2)]),
                Geom::Polygon { .. } | Geom::Star { .. } => {
                    let r = dist(a, b);
                    Shape::Ellipse((a.0 - r, a.1 - r), (a.0 + r, a.1 + r))
                }
                _ => return None,
            });
        }
        None
    }

    pub fn pen_shape(&self) -> Option<(Shape, Vec<Pt>)> {
        let s = self.pen.as_ref()?;
        let mut pts: Vec<Pt> = s.path.nodes.iter().map(PathNode::pos).collect();
        for n in &s.path.nodes {
            if n.hout != (0.0, 0.0) {
                pts.push(n.out_abs());
                pts.push(n.in_abs());
            }
        }
        Some((Shape::Poly(flatten(&s.path)), pts))
    }

    pub fn node_overlay(&self, open: &OpenDoc) -> Option<(Shape, Vec<Pt>)> {
        let (_, path) = Self::active_path(open)?;
        let mut pts: Vec<Pt> = path.nodes.iter().map(PathNode::pos).collect();
        if let Some(i) = self.node_sel.filter(|i| *i < path.nodes.len()) {
            let n = path.nodes[i];
            if n.hout != (0.0, 0.0) {
                pts.push(n.out_abs());
            }
            if n.hin != (0.0, 0.0) {
                pts.push(n.in_abs());
            }
        }
        let mut poly = flatten(&path);
        if path.closed {
            if let Some(f) = poly.first().copied() {
                poly.push(f);
            }
        }
        Some((Shape::Poly(poly), pts))
    }

    pub fn text_overlay(&self, open: &OpenDoc) -> Option<Shape> {
        let t = Self::text_spec_of(open, self.text_editing()?)?;
        let (x, y, w, h) = text::text_bounds(&t);
        Some(Shape::Rect((x, y), (x + w.max(12.0), y + h.max(t.size))))
    }

    // ---------- Opções ----------

    pub fn shape_options_ui(&mut self, ui: &mut egui::Ui) {
        egui::ComboBox::from_id_salt("shape_kind")
            .selected_text(self.shape_kind.label())
            .show_ui(ui, |ui| {
                for k in ShapeKind::ALL {
                    ui.selectable_value(&mut self.shape_kind, k, k.label());
                }
            });
        match self.shape_kind {
            ShapeKind::RoundRect => {
                ui.add(
                    egui::DragValue::new(&mut self.radius)
                        .prefix("Raio ")
                        .range(0.0..=500.0),
                );
            }
            ShapeKind::Polygon => {
                ui.add(
                    egui::DragValue::new(&mut self.sides)
                        .prefix("Lados ")
                        .range(3..=64),
                );
            }
            ShapeKind::Star => {
                ui.add(
                    egui::DragValue::new(&mut self.star_points)
                        .prefix("Pontas ")
                        .range(3..=64),
                );
                ui.add(egui::Slider::new(&mut self.star_inner, 0.1..=0.95).text("Raio interno"));
            }
            _ => {}
        }
        ui.separator();
        ui.radio_value(&mut self.fill_mode, FillMode::None, "Sem preenchimento");
        ui.radio_value(&mut self.fill_mode, FillMode::Solid, "Cor de frente");
        ui.radio_value(
            &mut self.fill_mode,
            FillMode::Gradient,
            "Degradê frente→fundo",
        );
        ui.checkbox(&mut self.stroke_on, "Traço (cor de fundo)");
        ui.add(
            egui::DragValue::new(&mut self.stroke_width)
                .prefix("Largura ")
                .range(0.5..=200.0),
        );
        ui.label("Arraste no canvas · Shift: proporção 1:1 · edite depois no painel da camada.");
    }

    pub fn pen_options_ui(&mut self, ui: &mut egui::Ui) {
        ui.label("Clique para cantos, clique e arraste para curvas · Enter termina · clique no 1º ponto fecha · Esc cancela.");
        ui.radio_value(&mut self.fill_mode, FillMode::None, "Sem preenchimento");
        ui.radio_value(
            &mut self.fill_mode,
            FillMode::Solid,
            "Preencher (caminho fechado)",
        );
        ui.add(
            egui::DragValue::new(&mut self.stroke_width)
                .prefix("Traço ")
                .range(0.5..=200.0),
        );
    }

    pub fn node_options_ui(ui: &mut egui::Ui) {
        ui.label("Arraste nós e alças · clique no segmento adiciona nó · duplo clique alterna canto/suave · Delete remove o nó · Alt solta a alça oposta.");
    }

    pub fn text_options_ui(&mut self, ui: &mut egui::Ui, open: Option<&mut OpenDoc>) {
        let mut spec = self.text.clone();
        let before = spec.clone();
        text_style_ui(ui, &mut spec, &mut self.font_filter);
        if spec != before {
            match open {
                Some(o) if self.text_edit.is_some() => {
                    self.edit_text_spec(o, |t| {
                        t.family = spec.family.clone();
                        t.bold = spec.bold;
                        t.italic = spec.italic;
                        t.size = spec.size;
                        t.color = spec.color;
                        t.align = spec.align;
                        t.line_height = spec.line_height;
                        t.letter_spacing = spec.letter_spacing;
                        t.box_width = spec.box_width;
                    });
                }
                _ => self.text = spec,
            }
        }
        ui.label("Clique no canvas para escrever · clique num texto para editar · Esc cancela · Ctrl+Enter confirma.");
    }
}

/// Controles de estilo de texto (compartilhados pela barra de opções e pelo painel da camada).
pub fn text_style_ui(ui: &mut egui::Ui, t: &mut TextSpec, filter: &mut String) {
    let shown = if t.family.is_empty() {
        "Padrão (sans-serif)".to_string()
    } else {
        t.family.clone()
    };
    egui::ComboBox::from_id_salt("text_family")
        .selected_text(shown)
        .width(170.0)
        .show_ui(ui, |ui| {
            ui.add(egui::TextEdit::singleline(filter).hint_text("Buscar fonte…"));
            if ui
                .selectable_label(t.family.is_empty(), "Padrão (sans-serif)")
                .clicked()
            {
                t.family.clear();
            }
            let f = filter.to_lowercase();
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for name in font_list()
                        .iter()
                        .filter(|n| f.is_empty() || n.to_lowercase().contains(&f))
                    {
                        if ui.selectable_label(&t.family == name, name).clicked() {
                            t.family = name.clone();
                        }
                    }
                });
        });
    if !text::font_installed(&t.family) {
        ui.colored_label(crate::theme::COPPER, "fonte ausente: usando substituta");
    }
    ui.add(
        egui::DragValue::new(&mut t.size)
            .prefix("Tam. ")
            .suffix(" px")
            .range(1.0..=1000.0),
    );
    ui.checkbox(&mut t.bold, "Negrito");
    ui.checkbox(&mut t.italic, "Itálico");
    let mut c =
        egui::Color32::from_rgba_unmultiplied(t.color[0], t.color[1], t.color[2], t.color[3]);
    if ui.color_edit_button_srgba(&mut c).changed() {
        t.color = c.to_srgba_unmultiplied();
    }
    ui.selectable_value(&mut t.align, TextAlign::Left, "Esq.");
    ui.selectable_value(&mut t.align, TextAlign::Center, "Centro");
    ui.selectable_value(&mut t.align, TextAlign::Right, "Dir.");
    ui.add(
        egui::DragValue::new(&mut t.line_height)
            .prefix("Entrelinha ")
            .speed(0.01)
            .range(0.5..=4.0),
    );
    ui.add(
        egui::DragValue::new(&mut t.letter_spacing)
            .prefix("Espaç. ")
            .suffix(" px")
            .speed(0.1)
            .range(-20.0..=100.0),
    );
    let mut para = t.box_width.is_some();
    if ui.checkbox(&mut para, "Parágrafo").changed() {
        t.box_width = para.then_some(300.0);
    }
    if let Some(w) = t.box_width.as_mut() {
        ui.add(
            egui::DragValue::new(w)
                .prefix("Largura ")
                .suffix(" px")
                .range(10.0..=5000.0),
        );
    }
}

/// Propriedades editáveis de qualquer origem (usado no painel de camadas). Retorna `true` se mudou.
pub fn source_props_ui(ui: &mut egui::Ui, src: &mut LayerSource, filter: &mut String) -> bool {
    let before = src.clone();
    match src {
        LayerSource::Text(t) => {
            ui.strong("Texto");
            ui.add(
                egui::TextEdit::multiline(&mut t.text)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal_wrapped(|ui| text_style_ui(ui, t, filter));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut t.x).prefix("X "));
                ui.add(egui::DragValue::new(&mut t.y).prefix("Y "));
            });
        }
        LayerSource::Shape(s) => {
            ui.strong("Forma");
            geom_ui(ui, &mut s.geom);
            fill_ui(ui, &mut s.fill, s.geom.clone());
            let mut on = s.stroke.is_some();
            if ui.checkbox(&mut on, "Traço").changed() {
                s.stroke = on.then_some(StrokeSpec {
                    color: [0, 0, 0, 255],
                    width: 4.0,
                });
            }
            if let Some(st) = s.stroke.as_mut() {
                ui.horizontal(|ui| {
                    let mut c = egui::Color32::from_rgba_unmultiplied(
                        st.color[0],
                        st.color[1],
                        st.color[2],
                        st.color[3],
                    );
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        st.color = c.to_srgba_unmultiplied();
                    }
                    ui.add(
                        egui::DragValue::new(&mut st.width)
                            .prefix("Largura ")
                            .range(0.5..=200.0),
                    );
                });
            }
        }
    }
    *src != before
}

fn geom_ui(ui: &mut egui::Ui, g: &mut Geom) {
    match g {
        Geom::Rect { x, y, w, h, radius } => {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::DragValue::new(x).prefix("X "));
                ui.add(egui::DragValue::new(y).prefix("Y "));
                ui.add(egui::DragValue::new(w).prefix("L ").range(1.0..=100_000.0));
                ui.add(egui::DragValue::new(h).prefix("A ").range(1.0..=100_000.0));
                ui.add(
                    egui::DragValue::new(radius)
                        .prefix("Raio ")
                        .range(0.0..=5000.0),
                );
            });
        }
        Geom::Ellipse { x, y, w, h } => {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::DragValue::new(x).prefix("X "));
                ui.add(egui::DragValue::new(y).prefix("Y "));
                ui.add(egui::DragValue::new(w).prefix("L ").range(1.0..=100_000.0));
                ui.add(egui::DragValue::new(h).prefix("A ").range(1.0..=100_000.0));
            });
        }
        Geom::Polygon {
            cx,
            cy,
            radius,
            sides,
            rotation,
        } => {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::DragValue::new(cx).prefix("X "));
                ui.add(egui::DragValue::new(cy).prefix("Y "));
                ui.add(
                    egui::DragValue::new(radius)
                        .prefix("Raio ")
                        .range(1.0..=100_000.0),
                );
                ui.add(egui::DragValue::new(sides).prefix("Lados ").range(3..=64));
                ui.add(egui::DragValue::new(rotation).prefix("Giro ").suffix("°"));
            });
        }
        Geom::Star {
            cx,
            cy,
            outer,
            inner,
            points,
            rotation,
        } => {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::DragValue::new(cx).prefix("X "));
                ui.add(egui::DragValue::new(cy).prefix("Y "));
                ui.add(
                    egui::DragValue::new(outer)
                        .prefix("Raio ")
                        .range(1.0..=100_000.0),
                );
                ui.add(
                    egui::DragValue::new(inner)
                        .prefix("Interno ")
                        .range(0.5..=100_000.0),
                );
                ui.add(egui::DragValue::new(points).prefix("Pontas ").range(3..=64));
                ui.add(egui::DragValue::new(rotation).prefix("Giro ").suffix("°"));
            });
        }
        Geom::Line { x1, y1, x2, y2 } => {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::DragValue::new(x1).prefix("X1 "));
                ui.add(egui::DragValue::new(y1).prefix("Y1 "));
                ui.add(egui::DragValue::new(x2).prefix("X2 "));
                ui.add(egui::DragValue::new(y2).prefix("Y2 "));
            });
        }
        Geom::Path(p) => {
            ui.label(format!(
                "Caminho com {} nós ({}). Edite com a ferramenta de nós (A).",
                p.nodes.len(),
                if p.closed { "fechado" } else { "aberto" }
            ));
        }
        Geom::Boolean { op, .. } => {
            ui.label(format!("Forma combinada ({}): as formas originais continuam editáveis nos dados do projeto.", op.label()));
        }
    }
}

fn fill_ui(ui: &mut egui::Ui, fill: &mut Fill, geom: Geom) {
    let area = !matches!(geom, Geom::Line { .. });
    if !area {
        return;
    }
    let mode = match fill {
        Fill::None => 0,
        Fill::Solid(_) => 1,
        Fill::Linear { .. } => 2,
    };
    let mut m = mode;
    ui.horizontal(|ui| {
        ui.label("Preenchimento");
        ui.radio_value(&mut m, 0, "Nenhum");
        ui.radio_value(&mut m, 1, "Cor");
        ui.radio_value(&mut m, 2, "Degradê");
    });
    if m != mode {
        let (x, y, w, h) = ShapeSpec::solid(geom, [0; 4])
            .bounds()
            .unwrap_or((0.0, 0.0, 100.0, 100.0));
        let base = match *fill {
            Fill::Solid(c) => c,
            Fill::Linear { c0, .. } => c0,
            Fill::None => [90, 90, 90, 255],
        };
        *fill = match m {
            0 => Fill::None,
            1 => Fill::Solid(base),
            _ => Fill::Linear {
                x0: x,
                y0: y,
                x1: x + w,
                y1: y + h,
                c0: base,
                c1: [255, 255, 255, 255],
            },
        };
    }
    let col = |ui: &mut egui::Ui, c: &mut [u8; 4]| {
        let mut c32 = egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
        if ui.color_edit_button_srgba(&mut c32).changed() {
            *c = c32.to_srgba_unmultiplied();
        }
    };
    match fill {
        Fill::Solid(c) => col(ui, c),
        Fill::Linear {
            c0,
            c1,
            x0,
            y0,
            x1,
            y1,
        } => {
            ui.horizontal(|ui| {
                col(ui, c0);
                col(ui, c1);
                ui.add(egui::DragValue::new(x0).prefix("x0 "));
                ui.add(egui::DragValue::new(y0).prefix("y0 "));
                ui.add(egui::DragValue::new(x1).prefix("x1 "));
                ui.add(egui::DragValue::new(y1).prefix("y1 "));
            });
        }
        Fill::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::compose::pixel_at;
    use orvalith_core::Document;

    const FG: [u8; 4] = [200, 30, 30, 255];
    const BG: [u8; 4] = [30, 30, 200, 255];

    fn open() -> OpenDoc {
        OpenDoc::new(Document::new("t", 100, 80, Some([255, 255, 255, 255])).unwrap())
    }

    fn ev(pressed: Option<[f32; 2]>, moves: Vec<[f32; 2]>, released: bool) -> CanvasOut {
        CanvasOut {
            pressed,
            moves,
            down: !released,
            released,
            zoom: 1.0,
            pressure: 1.0,
            ..Default::default()
        }
    }

    fn drag(v: &mut VecState, o: &mut OpenDoc, a: [f32; 2], b: [f32; 2]) {
        v.handle_shape(o, &ev(Some(a), vec![], false), FG, BG);
        v.handle_shape(o, &ev(None, vec![b], false), FG, BG);
        v.handle_shape(o, &ev(None, vec![], true), FG, BG);
    }

    #[test]
    fn dragging_a_shape_creates_one_editable_layer_with_one_undo() {
        let mut o = open();
        let mut v = VecState::default();
        drag(&mut v, &mut o, [10.0, 10.0], [50.0, 40.0]);
        assert_eq!(o.history.labels(), vec!["Forma"]);
        assert_eq!(o.doc.layers.len(), 2);
        let l = o.doc.layers.last().unwrap();
        assert!(
            matches!(&l.source, Some(LayerSource::Shape(s)) if matches!(s.geom, Geom::Rect { .. }))
        );
        assert_eq!(pixel_at(&o.doc, 30, 25), Some(FG));
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers.len(), 1);
        // Arrasto de tamanho zero não cria nada; Shift força proporção 1:1.
        drag(&mut v, &mut o, [5.0, 5.0], [5.0, 5.0]);
        assert_eq!(o.doc.layers.len(), 1);
        v.handle_shape(&mut o, &ev(Some([10.0, 10.0]), vec![], false), FG, BG);
        v.handle_shape(
            &mut o,
            &CanvasOut {
                shift: true,
                ..ev(None, vec![], true)
            },
            FG,
            BG,
        );
        let mut v2 = VecState {
            shape_kind: ShapeKind::Ellipse,
            ..VecState::default()
        };
        let sq = v2
            .build_shape((10.0, 10.0), (50.0, 30.0), true, FG, BG)
            .unwrap();
        assert!(matches!(sq.geom, Geom::Ellipse { w, h, .. } if (w - h).abs() < 1e-3));
        v2.shape_kind = ShapeKind::Star;
        assert!(
            matches!(v2.build_shape((50.0, 40.0), (50.0, 20.0), false, FG, BG).unwrap().geom, Geom::Star { outer, .. } if (outer - 20.0).abs() < 1e-3)
        );
    }

    #[test]
    fn shape_styles_follow_options() {
        let mut v = VecState {
            fill_mode: FillMode::Gradient,
            stroke_on: true,
            stroke_width: 6.0,
            ..VecState::default()
        };
        let s = v
            .build_shape((0.0, 0.0), (20.0, 20.0), false, FG, BG)
            .unwrap();
        assert!(matches!(s.fill, Fill::Linear { c0, c1, .. } if c0 == FG && c1 == BG));
        assert_eq!(
            s.stroke,
            Some(StrokeSpec {
                color: BG,
                width: 6.0
            })
        );
        v.fill_mode = FillMode::None;
        v.stroke_on = false;
        let s = v
            .build_shape((0.0, 0.0), (20.0, 20.0), false, FG, BG)
            .unwrap();
        assert!(
            s.stroke.is_some(),
            "sem preenchimento e sem traço ficaria invisível"
        );
        v.shape_kind = ShapeKind::Line;
        let l = v
            .build_shape((0.0, 0.0), (20.0, 20.0), false, FG, BG)
            .unwrap();
        assert_eq!(l.fill, Fill::None);
        assert_eq!(l.stroke.unwrap().color, FG);
    }

    #[test]
    fn pen_builds_a_smooth_bezier_path_and_closes_on_first_node() {
        let mut o = open();
        let mut v = VecState {
            fill_mode: FillMode::Solid,
            ..VecState::default()
        };
        // 1º ponto: clique-e-arraste cria alças simétricas.
        v.handle_pen(&mut o, &ev(Some([10.0, 60.0]), vec![], false), FG, BG);
        v.handle_pen(&mut o, &ev(None, vec![[30.0, 40.0]], false), FG, BG);
        v.handle_pen(&mut o, &ev(None, vec![], true), FG, BG);
        v.handle_pen(&mut o, &ev(Some([60.0, 60.0]), vec![], false), FG, BG);
        v.handle_pen(&mut o, &ev(None, vec![], true), FG, BG);
        v.handle_pen(&mut o, &ev(Some([40.0, 10.0]), vec![], false), FG, BG);
        v.handle_pen(&mut o, &ev(None, vec![], true), FG, BG);
        assert!(v.busy() && o.doc.layers.len() == 1);
        assert!(v.pen_shape().is_some());
        // Fechar clicando perto do primeiro nó.
        v.handle_pen(&mut o, &ev(Some([11.0, 59.0]), vec![], false), FG, BG);
        assert!(!v.busy());
        assert_eq!(o.history.labels(), vec!["Caminho"]);
        let Some(LayerSource::Shape(s)) = &o.doc.layers.last().unwrap().source else {
            panic!()
        };
        let Geom::Path(p) = &s.geom else { panic!() };
        assert!(p.closed && p.nodes.len() == 3);
        assert_eq!(p.nodes[0].hout, (20.0, -20.0));
        assert_eq!(p.nodes[0].hin, (-20.0, 20.0));
        assert!(matches!(s.fill, Fill::Solid(c) if c == FG));
        assert_eq!(pixel_at(&o.doc, 40, 45), Some(FG));
    }

    #[test]
    fn pen_open_path_commits_with_enter_and_cancel_discards() {
        let mut o = open();
        let mut v = VecState::default();
        for p in [[10.0, 10.0], [50.0, 10.0]] {
            v.handle_pen(&mut o, &ev(Some(p), vec![], false), FG, BG);
            v.handle_pen(&mut o, &ev(None, vec![], true), FG, BG);
        }
        v.commit_pen(&mut o, FG, BG);
        let Some(LayerSource::Shape(s)) = &o.doc.layers.last().unwrap().source else {
            panic!()
        };
        assert_eq!(s.fill, Fill::None, "caminho aberto não preenche");
        assert_eq!(s.stroke.unwrap().color, FG);
        // Cancelar descarta sem tocar o documento.
        v.handle_pen(&mut o, &ev(Some([5.0, 5.0]), vec![], false), FG, BG);
        v.cancel_pen();
        assert!(!v.busy() && o.doc.layers.len() == 2);
        // Um único nó não gera caminho.
        v.handle_pen(&mut o, &ev(Some([5.0, 5.0]), vec![], false), FG, BG);
        v.commit_pen(&mut o, FG, BG);
        assert_eq!(o.doc.layers.len(), 2);
    }

    fn path_doc() -> (OpenDoc, VecState) {
        let mut o = open();
        let mut v = VecState::default();
        let spec = ShapeSpec {
            geom: Geom::Path(BezierPath {
                closed: false,
                nodes: vec![PathNode::corner(10.0, 40.0), PathNode::corner(60.0, 40.0)],
            }),
            fill: Fill::None,
            stroke: Some(StrokeSpec {
                color: [0, 0, 0, 255],
                width: 4.0,
            }),
        };
        VecState::add_layer(&mut o, LayerSource::Shape(spec), "Caminho");
        v.node_sel = None;
        (o, v)
    }

    fn nodes(o: &OpenDoc) -> Vec<PathNode> {
        match &o.doc.layers.last().unwrap().source {
            Some(LayerSource::Shape(ShapeSpec {
                geom: Geom::Path(p),
                ..
            })) => p.nodes.clone(),
            _ => panic!(),
        }
    }

    #[test]
    fn node_tool_moves_inserts_toggles_smooth_and_deletes_with_undo() {
        let (mut o, mut v) = path_doc();
        let base = o.history.labels().len();
        // Arrastar o nó 1 move a âncora; um único item de histórico.
        v.handle_node(&mut o, &ev(Some([60.0, 40.0]), vec![], false));
        v.handle_node(&mut o, &ev(None, vec![[70.0, 20.0]], false));
        v.handle_node(&mut o, &ev(None, vec![], true));
        assert_eq!(nodes(&o)[1].pos(), (70.0, 20.0));
        assert_eq!(o.history.labels().len(), base + 1);
        // Clique no segmento insere um nó.
        let seg_mid = (40.0, 30.0);
        v.handle_node(&mut o, &ev(Some([seg_mid.0, seg_mid.1]), vec![], false));
        v.handle_node(&mut o, &ev(None, vec![], true));
        assert_eq!(nodes(&o).len(), 3);
        assert_eq!(v.node_sel, Some(1));
        // Duplo clique no nó selecionado o torna suave; de novo, canto.
        let n = nodes(&o)[1];
        v.handle_node(
            &mut o,
            &CanvasOut {
                double_click: true,
                ..ev(Some([n.x, n.y]), vec![], false)
            },
        );
        assert!(nodes(&o)[1].hout != (0.0, 0.0));
        let n = nodes(&o)[1];
        v.handle_node(
            &mut o,
            &CanvasOut {
                double_click: true,
                ..ev(Some([n.x, n.y]), vec![], false)
            },
        );
        assert_eq!(nodes(&o)[1].hout, (0.0, 0.0));
        // Delete remove o nó selecionado; desfazer restaura.
        assert!(v.delete_selected_node(&mut o));
        assert_eq!(nodes(&o).len(), 2);
        o.history.undo(&mut o.doc);
        assert_eq!(nodes(&o).len(), 3);
    }

    #[test]
    fn node_handle_drag_keeps_smooth_pair_aligned_unless_alt() {
        let (mut o, mut v) = path_doc();
        v.handle_node(&mut o, &ev(Some([60.0, 40.0]), vec![], false));
        v.handle_node(&mut o, &ev(None, vec![], true));
        // Dá alças suaves ao nó 1 diretamente.
        if let Some(LayerSource::Shape(ShapeSpec {
            geom: Geom::Path(p),
            ..
        })) = o.doc.layers.last_mut().unwrap().source.as_mut()
        {
            p.nodes[1].hin = (-10.0, 0.0);
            p.nodes[1].hout = (10.0, 0.0);
        }
        v.node_sel = Some(1);
        v.handle_node(&mut o, &ev(Some([70.0, 40.0]), vec![], false));
        v.handle_node(&mut o, &ev(None, vec![[60.0, 60.0]], false));
        v.handle_node(&mut o, &ev(None, vec![], true));
        let n = nodes(&o)[1];
        assert_eq!(n.hout, (0.0, 20.0));
        assert!(
            n.hin.0.abs() < 1e-3 && n.hin.1 < 0.0,
            "alça oposta espelhada: {:?}",
            n.hin
        );
        // Com Alt a oposta fica onde está.
        let keep = nodes(&o)[1].hin;
        v.handle_node(&mut o, &ev(Some([60.0, 60.0]), vec![], false));
        v.handle_node(
            &mut o,
            &CanvasOut {
                alt: true,
                ..ev(None, vec![[80.0, 40.0]], false)
            },
        );
        v.handle_node(&mut o, &ev(None, vec![], true));
        assert_eq!(nodes(&o)[1].hin, keep);
    }

    #[test]
    fn text_editing_is_one_action_cancel_and_empty_discard() {
        if text::list_fonts().is_empty() {
            return;
        }
        let mut o = open();
        let mut v = VecState::default();
        v.text.size = 20.0;
        v.handle_text(
            &mut o,
            &ev(Some([10.0, 10.0]), vec![], false),
            [0, 0, 0, 255],
        );
        assert!(v.text_editing().is_some() && o.doc.layers.len() == 2);
        assert!(v.busy());
        v.edit_text_spec(&mut o, |t| t.text = "Olá, mundo".into());
        v.edit_text_spec(&mut o, |t| t.text = "Olá, mundo!".into());
        assert_ne!(pixel_at(&o.doc, 12, 20), Some([255, 255, 255, 255]));
        assert_eq!(v.current_text(&o).as_deref(), Some("Olá, mundo!"));
        v.commit_text(&mut o);
        assert_eq!(o.history.labels(), vec!["Texto"], "digitar tudo = uma ação");
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers.len(), 1);
        o.history.redo(&mut o.doc);
        // Texto vazio em camada nova = nada é criado.
        v.handle_text(
            &mut o,
            &ev(Some([60.0, 60.0]), vec![], false),
            [0, 0, 0, 255],
        );
        v.commit_text(&mut o);
        assert_eq!(o.doc.layers.len(), 2);
        assert_eq!(o.history.labels().len(), 1);
        // Clicar num texto existente o edita; cancelar restaura.
        v.handle_text(
            &mut o,
            &ev(Some([14.0, 18.0]), vec![], false),
            [0, 0, 0, 255],
        );
        assert_eq!(v.text_editing(), Some(o.doc.layers[1].id));
        v.edit_text_spec(&mut o, |t| t.text = "Outro".into());
        v.cancel_text(&mut o);
        assert_eq!(v.current_text(&o), None);
        match &o.doc.layers[1].source {
            Some(LayerSource::Text(t)) => assert_eq!(t.text, "Olá, mundo!"),
            other => panic!("{other:?}"),
        }
        // Editar e confirmar: segunda ação "Editar texto".
        v.handle_text(
            &mut o,
            &ev(Some([14.0, 18.0]), vec![], false),
            [0, 0, 0, 255],
        );
        v.edit_text_spec(&mut o, |t| t.bold = true);
        v.commit_text(&mut o);
        assert_eq!(o.history.labels(), vec!["Texto", "Editar texto"]);
    }

    #[test]
    fn source_layers_move_losslessly_and_block_painting_and_transform() {
        let mut o = open();
        let mut v = VecState::default();
        drag(&mut v, &mut o, [70.0, 10.0], [120.0, 40.0]); // metade fora da tela
        let id = o.doc.layers.last().unwrap().id;
        o.active_layer = id;
        o.selected = vec![id];
        // Mover pela ferramenta Mover traz o conteúdo cortado de volta, sem perda.
        let mut t = crate::tools::ToolState::new(Default::default(), FG, BG);
        t.tool = crate::tools::Tool::Move;
        t.handle(&mut o, &ev(Some([80.0, 20.0]), vec![], false));
        t.handle(&mut o, &ev(None, vec![[40.0, 20.0]], false));
        t.handle(&mut o, &ev(None, vec![], true));
        assert_eq!(pixel_at(&o.doc, 95, 25), Some([255, 255, 255, 255]));
        assert_eq!(
            pixel_at(&o.doc, 60, 25),
            Some(FG),
            "parte que estava fora agora aparece"
        );
        assert!(o.doc.layers.last().unwrap().source.is_some());
        // Pintura e transformação exigem rasterizar antes.
        assert!(crate::tools::paint_blocked(o.doc.layers.last().unwrap()).is_some());
        assert!(
            crate::geometry::TransformSession::start(&o, crate::geometry::TMode::Free).is_err()
        );
    }
}
