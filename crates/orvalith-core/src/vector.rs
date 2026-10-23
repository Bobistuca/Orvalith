//! Formas e caminhos vetoriais editáveis, rasterizados com `tiny-skia`.
//!
//! Uma camada de forma guarda a especificação (`ShapeSpec`) e os pixels já renderizados;
//! editar a especificação re-renderiza. A rasterização explícita descarta a especificação.
//! Operações booleanas combinam a **cobertura** (alfa) das duas formas e mantêm as formas
//! originais como filhas editáveis. Traço em resultado booleano não é suportado.

use serde::{Deserialize, Serialize};
use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Path, PathBuilder,
    Pixmap, Point, Rect as SkRect, SpreadMode, Stroke as SkStroke, Transform,
};

pub type Pt = (f32, f32);

/// Nó de um caminho Bézier; as alças são deslocamentos relativos ao nó.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PathNode {
    pub x: f32,
    pub y: f32,
    pub hin: Pt,
    pub hout: Pt,
}

impl PathNode {
    pub fn corner(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            hin: (0.0, 0.0),
            hout: (0.0, 0.0),
        }
    }

    pub fn pos(&self) -> Pt {
        (self.x, self.y)
    }

    pub fn in_abs(&self) -> Pt {
        (self.x + self.hin.0, self.y + self.hin.1)
    }

    pub fn out_abs(&self) -> Pt {
        (self.x + self.hout.0, self.y + self.hout.1)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BezierPath {
    pub nodes: Vec<PathNode>,
    pub closed: bool,
}

fn lerp(a: Pt, b: Pt, t: f32) -> Pt {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

impl BezierPath {
    pub fn segment_count(&self) -> usize {
        match self.nodes.len() {
            0 | 1 => 0,
            n if self.closed => n,
            n => n - 1,
        }
    }

    /// Pontos de controle P0, P1, P2, P3 do segmento `i` (do nó i ao nó i+1).
    pub fn segment(&self, i: usize) -> Option<[Pt; 4]> {
        let n = self.nodes.len();
        if i >= self.segment_count() {
            return None;
        }
        let (a, b) = (&self.nodes[i], &self.nodes[(i + 1) % n]);
        Some([a.pos(), a.out_abs(), b.in_abs(), b.pos()])
    }

    /// Ponto do caminho mais próximo de `p` (segmento, t) dentro de `tol`, por amostragem.
    pub fn nearest_on_path(&self, p: Pt, tol: f32) -> Option<(usize, f32)> {
        let mut best: Option<(f32, usize, f32)> = None;
        for i in 0..self.segment_count() {
            let Some(c) = self.segment(i) else { continue };
            for k in 0..=32 {
                let t = k as f32 / 32.0;
                let q = cubic_point(c, t);
                let d = (q.0 - p.0).hypot(q.1 - p.1);
                if d <= tol && best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, i, t));
                }
            }
        }
        best.map(|(_, i, t)| (i, t))
    }

    /// Insere um nó no segmento `seg` em `t`, dividindo a curva sem alterar sua forma.
    pub fn insert_node(&mut self, seg: usize, t: f32) -> bool {
        let Some([p0, p1, p2, p3]) = self.segment(seg) else {
            return false;
        };
        let t = t.clamp(0.02, 0.98);
        if p1 == p0 && p2 == p3 {
            // Segmento reto: o novo nó é um canto sobre a reta, sem alças.
            let s = lerp(p0, p3, t);
            self.nodes.insert(seg + 1, PathNode::corner(s.0, s.1));
            return true;
        }
        let (q0, q1, q2) = (lerp(p0, p1, t), lerp(p1, p2, t), lerp(p2, p3, t));
        let (r0, r1) = (lerp(q0, q1, t), lerp(q1, q2, t));
        let s = lerp(r0, r1, t);
        let n = self.nodes.len();
        let j = (seg + 1) % n;
        self.nodes[seg].hout = (q0.0 - p0.0, q0.1 - p0.1);
        self.nodes[j].hin = (q2.0 - p3.0, q2.1 - p3.1);
        let node = PathNode {
            x: s.0,
            y: s.1,
            hin: (r0.0 - s.0, r0.1 - s.1),
            hout: (r1.0 - s.0, r1.1 - s.1),
        };
        self.nodes.insert(seg + 1, node);
        true
    }

    /// Remove o nó `i` (mantém ao menos 2 nós).
    pub fn remove_node(&mut self, i: usize) -> bool {
        if i >= self.nodes.len() || self.nodes.len() <= 2 {
            return false;
        }
        self.nodes.remove(i);
        true
    }

    /// Alterna o nó entre suave (alças alinhadas automáticas) e canto (sem alças).
    pub fn set_smooth(&mut self, i: usize, smooth: bool) {
        let n = self.nodes.len();
        if i >= n {
            return;
        }
        if !smooth {
            self.nodes[i].hin = (0.0, 0.0);
            self.nodes[i].hout = (0.0, 0.0);
            return;
        }
        let cur = self.nodes[i].pos();
        let prev = if i > 0 {
            Some(self.nodes[i - 1].pos())
        } else if self.closed {
            Some(self.nodes[n - 1].pos())
        } else {
            None
        };
        let next = if i + 1 < n {
            Some(self.nodes[i + 1].pos())
        } else if self.closed {
            Some(self.nodes[0].pos())
        } else {
            None
        };
        let dir = match (prev, next) {
            (Some(p), Some(q)) => (q.0 - p.0, q.1 - p.1),
            (None, Some(q)) => (q.0 - cur.0, q.1 - cur.1),
            (Some(p), None) => (cur.0 - p.0, cur.1 - p.1),
            (None, None) => return,
        };
        let len = dir.0.hypot(dir.1).max(1e-6);
        let u = (dir.0 / len, dir.1 / len);
        let dist = |o: Option<Pt>| o.map_or(0.0, |q| (q.0 - cur.0).hypot(q.1 - cur.1) / 3.0);
        let (lin, lout) = (dist(prev), dist(next));
        self.nodes[i].hin = (-u.0 * lin, -u.1 * lin);
        self.nodes[i].hout = (u.0 * lout, u.1 * lout);
    }

    fn to_sk(&self) -> Option<Path> {
        let n = self.nodes.len();
        if n < 2 {
            return None;
        }
        let mut pb = PathBuilder::new();
        pb.move_to(self.nodes[0].x, self.nodes[0].y);
        for i in 0..self.segment_count() {
            let (a, b) = (&self.nodes[i], &self.nodes[(i + 1) % n]);
            let (c1, c2) = (a.out_abs(), b.in_abs());
            if a.hout == (0.0, 0.0) && b.hin == (0.0, 0.0) {
                pb.line_to(b.x, b.y);
            } else {
                pb.cubic_to(c1.0, c1.1, c2.0, c2.1, b.x, b.y);
            }
        }
        if self.closed {
            pb.close();
        }
        pb.finish()
    }
}

fn cubic_point(c: [Pt; 4], t: f32) -> Pt {
    let u = 1.0 - t;
    (
        u * u * u * c[0].0
            + 3.0 * u * u * t * c[1].0
            + 3.0 * u * t * t * c[2].0
            + t * t * t * c[3].0,
        u * u * u * c[0].1
            + 3.0 * u * u * t * c[1].1
            + 3.0 * u * t * t * c[2].1
            + t * t * t * c[3].1,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
    Exclude,
}

impl BoolOp {
    pub fn label(self) -> &'static str {
        match self {
            BoolOp::Union => "União",
            BoolOp::Subtract => "Subtrair",
            BoolOp::Intersect => "Interseção",
            BoolOp::Exclude => "Exclusão",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Geom {
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
    },
    Ellipse {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    Polygon {
        cx: f32,
        cy: f32,
        radius: f32,
        sides: u32,
        rotation: f32,
    },
    Star {
        cx: f32,
        cy: f32,
        outer: f32,
        inner: f32,
        points: u32,
        rotation: f32,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    },
    Path(BezierPath),
    Boolean {
        op: BoolOp,
        a: Box<ShapeSpec>,
        b: Box<ShapeSpec>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Fill {
    None,
    Solid([u8; 4]),
    Linear {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        c0: [u8; 4],
        c1: [u8; 4],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrokeSpec {
    pub color: [u8; 4],
    pub width: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeSpec {
    pub geom: Geom,
    pub fill: Fill,
    pub stroke: Option<StrokeSpec>,
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.clamp(0.0, w.abs().min(h.abs()) / 2.0);
    if r <= 0.0 {
        return PathBuilder::from_rect(SkRect::from_xywh(x, y, w, h)?).into();
    }
    let k = 0.552_284_7 * r;
    let (x1, y1) = (x + w, y + h);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x1 - r, y);
    pb.cubic_to(x1 - r + k, y, x1, y + r - k, x1, y + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
    pb.line_to(x + r, y1);
    pb.cubic_to(x + r - k, y1, x, y1 - r + k, x, y1 - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn poly_path(pts: &[Pt]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let (first, rest) = pts.split_first()?;
    pb.move_to(first.0, first.1);
    for p in rest {
        pb.line_to(p.0, p.1);
    }
    pb.close();
    pb.finish()
}

/// Vértices de um polígono regular (ou estrela) em graus de rotação (0 = vértice para cima).
fn ring(cx: f32, cy: f32, count: u32, rot_deg: f32, radius: impl Fn(u32) -> f32) -> Vec<Pt> {
    (0..count)
        .map(|i| {
            let a = rot_deg.to_radians() - std::f32::consts::FRAC_PI_2
                + i as f32 * std::f32::consts::TAU / count as f32;
            let r = radius(i);
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// Caminho geométrico (sem booleanas).
fn geom_path(g: &Geom) -> Option<Path> {
    match g {
        Geom::Rect { x, y, w, h, radius } => rounded_rect(*x, *y, *w, *h, *radius),
        Geom::Ellipse { x, y, w, h } => PathBuilder::from_oval(SkRect::from_xywh(*x, *y, *w, *h)?),
        Geom::Polygon {
            cx,
            cy,
            radius,
            sides,
            rotation,
        } => {
            let n = (*sides).clamp(3, 64);
            poly_path(&ring(*cx, *cy, n, *rotation, |_| *radius))
        }
        Geom::Star {
            cx,
            cy,
            outer,
            inner,
            points,
            rotation,
        } => {
            let n = (*points).clamp(3, 64);
            poly_path(&ring(*cx, *cy, n * 2, *rotation, |i| {
                if i % 2 == 0 {
                    *outer
                } else {
                    *inner
                }
            }))
        }
        Geom::Line { x1, y1, x2, y2 } => {
            let mut pb = PathBuilder::new();
            pb.move_to(*x1, *y1);
            pb.line_to(*x2, *y2);
            pb.finish()
        }
        Geom::Path(p) => p.to_sk(),
        Geom::Boolean { .. } => None,
    }
}

fn solid_paint(c: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c[0], c[1], c[2], c[3]);
    p.anti_alias = true;
    p
}

fn fill_paint(f: &Fill) -> Option<Paint<'static>> {
    match f {
        Fill::None => None,
        Fill::Solid(c) => Some(solid_paint(*c)),
        Fill::Linear {
            x0,
            y0,
            x1,
            y1,
            c0,
            c1,
        } => {
            let col = |c: &[u8; 4]| Color::from_rgba8(c[0], c[1], c[2], c[3]);
            let shader = LinearGradient::new(
                Point::from_xy(*x0, *y0),
                Point::from_xy(*x1, *y1),
                vec![
                    GradientStop::new(0.0, col(c0)),
                    GradientStop::new(1.0, col(c1)),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            );
            match shader {
                Some(sh) => Some(Paint {
                    shader: sh,
                    anti_alias: true,
                    ..Paint::default()
                }),
                None => Some(solid_paint(*c0)),
            }
        }
    }
}

fn sk_stroke(width: f32) -> SkStroke {
    SkStroke {
        width: width.max(0.1),
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..SkStroke::default()
    }
}

fn pixmap_to_rgba(p: &Pixmap) -> Vec<u8> {
    let mut out = Vec::with_capacity(p.pixels().len() * 4);
    for px in p.pixels() {
        let c = px.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    out
}

/// Cobertura (alfa 0..255) do preenchimento da geometria; linhas usam o traço.
pub fn coverage(spec: &ShapeSpec, w: u32, h: u32) -> Vec<u8> {
    let n = w as usize * h as usize;
    if let Geom::Boolean { op, a, b } = &spec.geom {
        let (ca, cb) = (coverage(a, w, h), coverage(b, w, h));
        return ca
            .iter()
            .zip(&cb)
            .map(|(&x, &y)| {
                let (x, y) = (u32::from(x), u32::from(y));
                (match op {
                    BoolOp::Union => x.max(y),
                    BoolOp::Subtract => x * (255 - y) / 255,
                    BoolOp::Intersect => x.min(y),
                    BoolOp::Exclude => x.abs_diff(y),
                }) as u8
            })
            .collect();
    }
    let Some(mut pm) = Pixmap::new(w.max(1), h.max(1)) else {
        return vec![0; n];
    };
    if let Some(path) = geom_path(&spec.geom) {
        let paint = solid_paint([0, 0, 0, 255]);
        if matches!(spec.geom, Geom::Line { .. }) {
            let width = spec.stroke.map_or(1.0, |s| s.width);
            pm.stroke_path(
                &path,
                &paint,
                &sk_stroke(width),
                Transform::identity(),
                None,
            );
        } else {
            pm.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
    pm.pixels().iter().map(|p| p.alpha()).collect()
}

/// Renderiza a forma em RGBA8 (não pré-multiplicado) do tamanho `w × h`.
pub fn render_shape(spec: &ShapeSpec, w: u32, h: u32) -> Vec<u8> {
    if let Geom::Boolean { .. } = spec.geom {
        let cov = coverage(spec, w, h);
        let mut out = Vec::with_capacity(cov.len() * 4);
        for (i, &c) in cov.iter().enumerate() {
            let (x, y) = ((i % w as usize) as f32 + 0.5, (i / w as usize) as f32 + 0.5);
            let col = fill_color_at(&spec.fill, x, y);
            out.extend_from_slice(&[
                col[0],
                col[1],
                col[2],
                ((u32::from(col[3]) * u32::from(c) + 127) / 255) as u8,
            ]);
        }
        return out;
    }
    let Some(mut pm) = Pixmap::new(w.max(1), h.max(1)) else {
        return vec![0; w as usize * h as usize * 4];
    };
    if let Some(path) = geom_path(&spec.geom) {
        if !matches!(spec.geom, Geom::Line { .. }) {
            if let Some(p) = fill_paint(&spec.fill) {
                pm.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
            }
        }
        if let Some(s) = spec.stroke {
            pm.stroke_path(
                &path,
                &solid_paint(s.color),
                &sk_stroke(s.width),
                Transform::identity(),
                None,
            );
        }
    }
    pixmap_to_rgba(&pm)
}

fn fill_color_at(f: &Fill, x: f32, y: f32) -> [u8; 4] {
    match f {
        Fill::None => [0, 0, 0, 0],
        Fill::Solid(c) => *c,
        Fill::Linear {
            x0,
            y0,
            x1,
            y1,
            c0,
            c1,
        } => {
            let (dx, dy) = (x1 - x0, y1 - y0);
            let len2 = dx * dx + dy * dy;
            let t = if len2 <= 1e-6 {
                0.0
            } else {
                (((x - x0) * dx + (y - y0) * dy) / len2).clamp(0.0, 1.0)
            };
            let l = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
            [
                l(c0[0], c1[0]),
                l(c0[1], c1[1]),
                l(c0[2], c1[2]),
                l(c0[3], c1[3]),
            ]
        }
    }
}

impl ShapeSpec {
    pub fn solid(geom: Geom, color: [u8; 4]) -> Self {
        Self {
            geom,
            fill: Fill::Solid(color),
            stroke: None,
        }
    }

    /// Caixa envolvente aproximada (x, y, w, h), incluindo alças de caminhos.
    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let pad = self.stroke.map_or(0.0, |s| s.width / 2.0);
        let (x0, y0, x1, y1) = match &self.geom {
            Geom::Rect { x, y, w, h, .. } | Geom::Ellipse { x, y, w, h } => (*x, *y, x + w, y + h),
            Geom::Polygon { cx, cy, radius, .. } => {
                (cx - radius, cy - radius, cx + radius, cy + radius)
            }
            Geom::Star { cx, cy, outer, .. } => (cx - outer, cy - outer, cx + outer, cy + outer),
            Geom::Line { x1, y1, x2, y2 } => (x1.min(*x2), y1.min(*y2), x1.max(*x2), y1.max(*y2)),
            Geom::Path(p) => {
                let mut pts: Vec<Pt> = Vec::new();
                for n in &p.nodes {
                    pts.extend([n.pos(), n.in_abs(), n.out_abs()]);
                }
                let fold = |f: fn(f32, f32) -> f32, init: f32, i: usize| {
                    pts.iter()
                        .fold(init, |a, p| f(a, if i == 0 { p.0 } else { p.1 }))
                };
                if pts.is_empty() {
                    return None;
                }
                (
                    fold(f32::min, f32::INFINITY, 0),
                    fold(f32::min, f32::INFINITY, 1),
                    fold(f32::max, f32::NEG_INFINITY, 0),
                    fold(f32::max, f32::NEG_INFINITY, 1),
                )
            }
            Geom::Boolean { a, b, .. } => {
                let (ba, bb) = (a.bounds()?, b.bounds()?);
                (
                    ba.0.min(bb.0),
                    ba.1.min(bb.1),
                    (ba.0 + ba.2).max(bb.0 + bb.2),
                    (ba.1 + ba.3).max(bb.1 + bb.3),
                )
            }
        };
        Some((x0 - pad, y0 - pad, x1 - x0 + 2.0 * pad, y1 - y0 + 2.0 * pad))
    }

    /// Move a geometria (e o degradê) por (dx, dy).
    pub fn translate(&mut self, dx: f32, dy: f32) {
        match &mut self.geom {
            Geom::Rect { x, y, .. } | Geom::Ellipse { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Geom::Polygon { cx, cy, .. } | Geom::Star { cx, cy, .. } => {
                *cx += dx;
                *cy += dy;
            }
            Geom::Line { x1, y1, x2, y2 } => {
                *x1 += dx;
                *y1 += dy;
                *x2 += dx;
                *y2 += dy;
            }
            Geom::Path(p) => {
                for n in &mut p.nodes {
                    n.x += dx;
                    n.y += dy;
                }
            }
            Geom::Boolean { a, b, .. } => {
                a.translate(dx, dy);
                b.translate(dx, dy);
            }
        }
        if let Fill::Linear { x0, y0, x1, y1, .. } = &mut self.fill {
            *x0 += dx;
            *x1 += dx;
            *y0 += dy;
            *y1 += dy;
        }
    }

    /// Profundidade de booleanas (para validar arquivos).
    pub fn depth(&self) -> usize {
        match &self.geom {
            Geom::Boolean { a, b, .. } => 1 + a.depth().max(b.depth()),
            _ => 0,
        }
    }

    /// Número total de nós de caminho (para validar arquivos).
    pub fn node_count(&self) -> usize {
        match &self.geom {
            Geom::Path(p) => p.nodes.len(),
            Geom::Boolean { a, b, .. } => a.node_count() + b.node_count(),
            _ => 0,
        }
    }

    /// Combina duas formas numa booleana editável (preenchimento da base `a`).
    pub fn boolean(op: BoolOp, a: ShapeSpec, b: ShapeSpec) -> ShapeSpec {
        let fill = match a.fill {
            Fill::None => Fill::Solid([0, 0, 0, 255]),
            f => f,
        };
        ShapeSpec {
            geom: Geom::Boolean {
                op,
                a: Box::new(a),
                b: Box::new(b),
            },
            fill,
            stroke: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 40;
    const H: u32 = 30;

    fn area(rgba: &[u8]) -> f32 {
        rgba.as_chunks::<4>()
            .0
            .iter()
            .map(|p| f32::from(p[3]) / 255.0)
            .sum()
    }

    fn at(rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
        let i = ((y * W + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    fn rect(x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) -> ShapeSpec {
        ShapeSpec::solid(
            Geom::Rect {
                x,
                y,
                w,
                h,
                radius: 0.0,
            },
            color,
        )
    }

    #[test]
    fn rect_area_and_color_are_exact_and_unmultiplied() {
        let px = render_shape(&rect(5.0, 4.0, 10.0, 8.0, [200, 100, 50, 255]), W, H);
        assert!((area(&px) - 80.0).abs() < 0.5);
        assert_eq!(at(&px, 8, 6), [200, 100, 50, 255]);
        assert_eq!(at(&px, 20, 6)[3], 0);
        // Translúcido: RGB preservado (não pré-multiplicado).
        let t = render_shape(&rect(5.0, 4.0, 10.0, 8.0, [200, 100, 50, 128]), W, H);
        let p = at(&t, 8, 6);
        assert!(
            p[0] >= 199 && p[1].abs_diff(100) <= 1 && p[3] == 128,
            "{p:?}"
        );
    }

    #[test]
    fn ellipse_polygon_star_and_rounded_rect() {
        let e = render_shape(
            &ShapeSpec::solid(
                Geom::Ellipse {
                    x: 4.0,
                    y: 4.0,
                    w: 20.0,
                    h: 20.0,
                },
                [0, 0, 0, 255],
            ),
            W,
            H,
        );
        assert!(
            (area(&e) - std::f32::consts::PI * 100.0).abs() < 8.0,
            "area {}",
            area(&e)
        );
        let sq = render_shape(
            &ShapeSpec::solid(
                Geom::Polygon {
                    cx: 20.0,
                    cy: 15.0,
                    radius: 10.0,
                    sides: 4,
                    rotation: 45.0,
                },
                [0, 0, 0, 255],
            ),
            W,
            H,
        );
        // Quadrado de raio 10 (circunscrito): lado = 10·√2 → área 200.
        assert!((area(&sq) - 200.0).abs() < 8.0, "{}", area(&sq));
        let tri = render_shape(
            &ShapeSpec::solid(
                Geom::Polygon {
                    cx: 20.0,
                    cy: 15.0,
                    radius: 10.0,
                    sides: 3,
                    rotation: 0.0,
                },
                [0, 0, 0, 255],
            ),
            W,
            H,
        );
        assert_eq!(at(&tri, 20, 8)[3], 255, "vértice para cima");
        let star = render_shape(
            &ShapeSpec::solid(
                Geom::Star {
                    cx: 20.0,
                    cy: 15.0,
                    outer: 12.0,
                    inner: 5.0,
                    points: 5,
                    rotation: 0.0,
                },
                [0, 0, 0, 255],
            ),
            W,
            H,
        );
        let disc = render_shape(
            &ShapeSpec::solid(
                Geom::Polygon {
                    cx: 20.0,
                    cy: 15.0,
                    radius: 12.0,
                    sides: 64,
                    rotation: 0.0,
                },
                [0, 0, 0, 255],
            ),
            W,
            H,
        );
        assert!(area(&star) < area(&disc) * 0.6 && area(&star) > 20.0);
        let rr = render_shape(
            &ShapeSpec::solid(
                Geom::Rect {
                    x: 4.0,
                    y: 4.0,
                    w: 20.0,
                    h: 20.0,
                    radius: 8.0,
                },
                [0, 0, 0, 255],
            ),
            W,
            H,
        );
        assert_eq!(at(&rr, 4, 4)[3], 0, "canto arredondado vazio");
        assert_eq!(at(&rr, 14, 4)[3], 255);
        assert!(area(&rr) < 400.0 && area(&rr) > 330.0);
    }

    #[test]
    fn stroke_fill_none_and_line() {
        let mut s = rect(10.0, 8.0, 20.0, 14.0, [255, 0, 0, 255]);
        s.fill = Fill::None;
        s.stroke = Some(StrokeSpec {
            color: [0, 0, 255, 255],
            width: 4.0,
        });
        let px = render_shape(&s, W, H);
        assert_eq!(at(&px, 20, 15)[3], 0, "interior vazio");
        assert_eq!(at(&px, 10, 15), [0, 0, 255, 255], "traço centrado na borda");
        let line = ShapeSpec {
            geom: Geom::Line {
                x1: 2.0,
                y1: 15.0,
                x2: 38.0,
                y2: 15.0,
            },
            fill: Fill::None,
            stroke: Some(StrokeSpec {
                color: [0, 255, 0, 255],
                width: 2.0,
            }),
        };
        let l = render_shape(&line, W, H);
        assert!((area(&l) - 72.0).abs() < 6.0);
        assert_eq!(at(&l, 20, 15)[1], 255);
    }

    #[test]
    fn linear_gradient_fill_goes_from_first_to_second_color() {
        let mut s = rect(0.0, 0.0, 40.0, 30.0, [0, 0, 0, 255]);
        s.fill = Fill::Linear {
            x0: 0.0,
            y0: 0.0,
            x1: 40.0,
            y1: 0.0,
            c0: [255, 0, 0, 255],
            c1: [0, 0, 255, 255],
        };
        let px = render_shape(&s, W, H);
        let (l, r, m) = (at(&px, 1, 10), at(&px, 38, 10), at(&px, 20, 10));
        assert!(l[0] > 235 && l[2] < 20, "{l:?}");
        assert!(r[2] > 235 && r[0] < 20, "{r:?}");
        assert!(
            (100..=155).contains(&m[0]) && (100..=155).contains(&m[2]),
            "{m:?}"
        );
    }

    #[test]
    fn boolean_operations_combine_coverage() {
        let a = rect(5.0, 5.0, 16.0, 16.0, [255, 0, 0, 255]);
        let b = rect(13.0, 5.0, 16.0, 16.0, [0, 0, 255, 255]);
        let area_of = |op| {
            area(&render_shape(
                &ShapeSpec::boolean(op, a.clone(), b.clone()),
                W,
                H,
            ))
        };
        assert!((area_of(BoolOp::Union) - (256.0 + 256.0 - 128.0)).abs() < 2.0);
        assert!((area_of(BoolOp::Intersect) - 128.0).abs() < 2.0);
        assert!((area_of(BoolOp::Subtract) - 128.0).abs() < 2.0);
        assert!((area_of(BoolOp::Exclude) - 256.0).abs() < 2.0);
        // A cor vem da forma base e continua editável (as formas filhas são preservadas).
        let u = ShapeSpec::boolean(BoolOp::Union, a.clone(), b.clone());
        assert_eq!(at(&render_shape(&u, W, H), 7, 10), [255, 0, 0, 255]);
        assert_eq!(at(&render_shape(&u, W, H), 25, 10), [255, 0, 0, 255]);
        assert_eq!(u.depth(), 1);
        match u.geom {
            Geom::Boolean { a: ref ba, .. } => assert_eq!(**ba, a),
            _ => panic!(),
        }
    }

    fn curve() -> BezierPath {
        BezierPath {
            closed: true,
            nodes: vec![
                PathNode {
                    x: 5.0,
                    y: 25.0,
                    hin: (0.0, 0.0),
                    hout: (0.0, -30.0),
                },
                PathNode {
                    x: 35.0,
                    y: 25.0,
                    hin: (0.0, -30.0),
                    hout: (0.0, 0.0),
                },
            ],
        }
    }

    #[test]
    fn bezier_path_renders_curved_and_straight_segments() {
        let px = render_shape(&ShapeSpec::solid(Geom::Path(curve()), [0, 0, 0, 255]), W, H);
        // A curva sobe acima da corda (y=25) no meio.
        assert_eq!(at(&px, 20, 10)[3], 255);
        assert_eq!(at(&px, 20, 28)[3], 0);
        let tri = BezierPath {
            closed: true,
            nodes: vec![
                PathNode::corner(5.0, 25.0),
                PathNode::corner(35.0, 25.0),
                PathNode::corner(20.0, 5.0),
            ],
        };
        let t = render_shape(&ShapeSpec::solid(Geom::Path(tri), [0, 0, 0, 255]), W, H);
        assert!((area(&t) - 300.0).abs() < 4.0);
    }

    #[test]
    fn path_editing_insert_remove_smooth_and_hit_testing() {
        let mut p = curve();
        let (seg, t) = p
            .nearest_on_path((20.0, 3.0), 3.0)
            .expect("ponto sobre a curva");
        assert_eq!(seg, 0);
        let before = render_shape(
            &ShapeSpec::solid(Geom::Path(p.clone()), [0, 0, 0, 255]),
            W,
            H,
        );
        assert!(p.insert_node(seg, t));
        assert_eq!(p.nodes.len(), 3);
        // Dividir não altera a forma.
        let after = render_shape(
            &ShapeSpec::solid(Geom::Path(p.clone()), [0, 0, 0, 255]),
            W,
            H,
        );
        assert!((area(&before) - area(&after)).abs() < 3.0);
        assert!(p.nearest_on_path((20.0, 28.0), 2.0).is_none());
        assert!(p.remove_node(1));
        assert!(!p.remove_node(0), "mínimo de 2 nós");
        let mut q = BezierPath {
            closed: false,
            nodes: vec![
                PathNode::corner(0.0, 0.0),
                PathNode::corner(10.0, 10.0),
                PathNode::corner(20.0, 0.0),
            ],
        };
        q.set_smooth(1, true);
        assert!(
            q.nodes[1].hout.0 > 0.0 && q.nodes[1].hin.0 < 0.0,
            "alças alinhadas pelo vizinho"
        );
        assert!((q.nodes[1].hin.0 + q.nodes[1].hout.0).abs() < 1e-4);
        q.set_smooth(1, false);
        assert_eq!(q.nodes[1].hout, (0.0, 0.0));
        assert_eq!(q.segment_count(), 2);
        assert!(q.segment(2).is_none());
    }

    #[test]
    fn straight_segment_split_keeps_corner_nodes() {
        let mut p = BezierPath {
            closed: false,
            nodes: vec![PathNode::corner(0.0, 0.0), PathNode::corner(10.0, 0.0)],
        };
        assert!(p.insert_node(0, 0.5));
        assert_eq!(p.nodes[1].pos(), (5.0, 0.0));
        assert_eq!((p.nodes[1].hin, p.nodes[1].hout), ((0.0, 0.0), (0.0, 0.0)));
    }

    #[test]
    fn translate_bounds_and_validation_helpers() {
        let mut s = rect(5.0, 4.0, 10.0, 8.0, [1, 2, 3, 255]);
        s.stroke = Some(StrokeSpec {
            color: [0, 0, 0, 255],
            width: 2.0,
        });
        assert_eq!(s.bounds(), Some((4.0, 3.0, 12.0, 10.0)));
        s.translate(10.0, -2.0);
        assert_eq!(s.bounds(), Some((14.0, 1.0, 12.0, 10.0)));
        let mut g = rect(0.0, 0.0, 4.0, 4.0, [0, 0, 0, 255]);
        g.fill = Fill::Linear {
            x0: 0.0,
            y0: 0.0,
            x1: 4.0,
            y1: 0.0,
            c0: [0; 4],
            c1: [255; 4],
        };
        g.translate(3.0, 1.0);
        assert!(
            matches!(g.fill, Fill::Linear { x0, x1, y0, .. } if x0 == 3.0 && x1 == 7.0 && y0 == 1.0)
        );
        let c = ShapeSpec::solid(Geom::Path(curve()), [0; 4]);
        assert_eq!(c.node_count(), 2);
        let b = ShapeSpec::boolean(BoolOp::Union, c.clone(), c);
        assert_eq!((b.depth(), b.node_count()), (1, 4));
        assert!(b.bounds().is_some());
    }

    #[test]
    fn json_roundtrip_of_specs() {
        let s = ShapeSpec::boolean(
            BoolOp::Subtract,
            rect(1.0, 2.0, 3.0, 4.0, [9, 8, 7, 255]),
            ShapeSpec::solid(Geom::Path(curve()), [1, 2, 3, 255]),
        );
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<ShapeSpec>(&json).unwrap(), s);
    }

    #[test]
    fn coverage_is_usable_as_selection_mask() {
        let m = coverage(&rect(5.0, 4.0, 10.0, 8.0, [0; 4]), W, H);
        assert_eq!(m.len(), (W * H) as usize);
        assert_eq!(m[(6 * W + 8) as usize], 255);
        assert_eq!(m[0], 0);
        assert_eq!(m.iter().filter(|&&v| v == 255).count(), 80);
    }
}
