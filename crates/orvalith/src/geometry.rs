//! Transformações, recorte e geometria do documento (interface). A matemática está em
//! `orvalith_core::{transform, docops, snap}`; aqui ficam a sessão de transformação, a
//! ferramenta de recorte e os diálogos de tamanho.

use std::sync::Arc;

use eframe::egui;
use orvalith_core::compose::over;
use orvalith_core::docops::{self, AlignKind, DocEdit, DocState, Orient};
use orvalith_core::edit::LayersEdit;
use orvalith_core::geom::Rect;
use orvalith_core::layerops::movable_rasters;
use orvalith_core::selection::{self as sel, nonempty};
use orvalith_core::transform::{self, Affine, Interp};
use orvalith_core::{DocumentId, LayerId, Selection};

use crate::canvas::{CanvasOut, OpenDoc, Shape};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TMode {
    Free,
    Skew,
    Distort,
    Perspective,
    Warp,
}

impl TMode {
    pub fn label(self) -> &'static str {
        match self {
            TMode::Free => "Livre (mover/escalar/girar)",
            TMode::Skew => "Inclinar",
            TMode::Distort => "Distorcer",
            TMode::Perspective => "Perspectiva",
            TMode::Warp => "Malha (4×4)",
        }
    }
}

const MESH: usize = 4;

type Pt = (f32, f32);

enum Drag {
    Move {
        start: Pt,
        t0: Pt,
    },
    Scale {
        idx: usize,
        quad0: [Pt; 4],
        p0: (f32, f32, f32, f32),
    },
    Rotate {
        a0: f32,
        rot0: f32,
    },
    Quad(usize),
    Mesh(usize),
}

pub struct TransformSession {
    doc: DocumentId,
    before: DocState,
    originals: Vec<(LayerId, Arc<Vec<u8>>)>,
    mask: Option<Arc<Vec<u8>>>,
    bounds: Rect,
    pub mode: TMode,
    pub tx: f32,
    pub ty: f32,
    pub sx_pct: f32,
    pub sy_pct: f32,
    pub lock_aspect: bool,
    /// Âncora do pivô dentro dos limites (0, 0,5 ou 1 em cada eixo; padrão: centro).
    pub pivot_anchor: (f32, f32),
    pub rot: f32,
    pub skx: f32,
    pub sky: f32,
    quad: [Pt; 4],
    mesh: Vec<Pt>,
    pub interp: Interp,
    drag: Option<Drag>,
    changed: bool,
    pub notice: Option<String>,
}

fn rect_corners(r: Rect) -> [Pt; 4] {
    let (x0, y0, x1, y1) = (r.x as f32, r.y as f32, r.right() as f32, r.bottom() as f32);
    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
}

fn dot(a: Pt, b: Pt) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

fn norm(a: Pt) -> Pt {
    let l = a.0.hypot(a.1).max(1e-6);
    (a.0 / l, a.1 / l)
}

/// Índice do ponto mais próximo de `p` dentro do raio `r`.
fn nearest_within(pts: &[Pt], p: Pt, r: f32) -> Option<usize> {
    pts.iter()
        .enumerate()
        .map(|(i, c)| (i, (c.0 - p.0).hypot(c.1 - p.1)))
        .filter(|(_, d)| *d <= r)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

fn point_in_quad(q: &[Pt; 4], p: Pt) -> bool {
    let mut inside = false;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    inside
}

impl TransformSession {
    /// Inicia uma sessão sobre as camadas selecionadas (e a seleção, se houver).
    pub fn start(open: &OpenDoc, mode: TMode) -> Result<Self, String> {
        let (w, h) = (open.doc.width, open.doc.height);
        let ids: Vec<LayerId> = open
            .selection_ids()
            .into_iter()
            .filter(|id| open.doc.layer(*id).is_some_and(|l| !l.lock_position))
            .collect();
        let targets: Vec<LayerId> = movable_rasters(&open.doc, &ids)
            .into_iter()
            .filter(|id| {
                open.doc.layer(*id).is_some_and(|l| {
                    !l.lock_pixels
                        && !l.lock_position
                        && l.pixels.len() == w as usize * h as usize * 4
                })
            })
            .collect();
        if targets.is_empty() {
            return Err(
                "Nenhuma camada transformável (bloqueada, vazia ou grupo sem camadas).".into(),
            );
        }
        let mask = open.doc.selection.mask.clone();
        let bounds = match &mask {
            Some(m) => sel::bounds(m, w, h),
            None => targets
                .iter()
                .filter_map(|id| open.doc.layer(*id).and_then(docops::content_bounds))
                .reduce(Rect::union),
        }
        .unwrap_or(Rect::full(w, h));
        let originals = targets
            .iter()
            .filter_map(|id| open.doc.layer(*id).map(|l| (*id, l.pixels.clone())))
            .collect();
        let quad = rect_corners(bounds);
        let mut s = Self {
            doc: open.doc.id,
            before: DocState::of(&open.doc),
            originals,
            mask,
            bounds,
            mode,
            tx: 0.0,
            ty: 0.0,
            sx_pct: 100.0,
            sy_pct: 100.0,
            lock_aspect: true,
            pivot_anchor: (0.5, 0.5),
            rot: 0.0,
            skx: 0.0,
            sky: 0.0,
            quad,
            mesh: Vec::new(),
            interp: Interp::Bicubic,
            drag: None,
            changed: false,
            notice: None,
        };
        s.set_mode(mode);
        Ok(s)
    }

    fn pivot(&self) -> Pt {
        (
            self.bounds.x as f32 + self.bounds.w as f32 * self.pivot_anchor.0,
            self.bounds.y as f32 + self.bounds.h as f32 * self.pivot_anchor.1,
        )
    }

    /// Muda o pivô sem alterar o resultado atual (compensa o deslocamento).
    pub fn set_pivot_anchor(&mut self, anchor: (f32, f32)) {
        let probe = (self.bounds.x as f32, self.bounds.y as f32);
        let old = self.affine().apply(probe.0, probe.1);
        self.pivot_anchor = anchor;
        let new = self.affine().apply(probe.0, probe.1);
        self.tx += old.0 - new.0;
        self.ty += old.1 - new.1;
    }

    fn affine(&self) -> Affine {
        let p = self.pivot();
        Affine::scale(self.sx_pct / 100.0, self.sy_pct / 100.0, p)
            .then(Affine::skew(self.skx, self.sky, p))
            .then(Affine::rotate(self.rot, p))
            .then(Affine::translate(self.tx, self.ty))
    }

    fn params_quad(&self) -> [Pt; 4] {
        let a = self.affine();
        rect_corners(self.bounds).map(|c| a.apply(c.0, c.1))
    }

    /// Quadrilátero atual (destino) conforme o modo.
    fn current_quad(&self) -> [Pt; 4] {
        match self.mode {
            TMode::Free | TMode::Skew => self.params_quad(),
            _ => self.quad,
        }
    }

    fn grid_src(&self) -> Vec<Pt> {
        let b = self.bounds;
        (0..MESH)
            .flat_map(|j| {
                (0..MESH).map(move |i| {
                    (
                        b.x as f32 + b.w as f32 * i as f32 / (MESH - 1) as f32,
                        b.y as f32 + b.h as f32 * j as f32 / (MESH - 1) as f32,
                    )
                })
            })
            .collect()
    }

    pub fn set_mode(&mut self, mode: TMode) {
        let q = self.current_quad();
        let from_free = matches!(self.mode, TMode::Free | TMode::Skew);
        self.mode = mode;
        match mode {
            TMode::Free | TMode::Skew => {
                if !from_free {
                    self.tx = 0.0;
                    self.ty = 0.0;
                    self.sx_pct = 100.0;
                    self.sy_pct = 100.0;
                    self.rot = 0.0;
                    self.skx = 0.0;
                    self.sky = 0.0;
                    self.quad = rect_corners(self.bounds);
                }
            }
            TMode::Distort | TMode::Perspective => self.quad = q,
            TMode::Warp => {
                self.quad = q;
                // Malha uniforme dentro do quadrilátero atual (interpolação bilinear).
                self.mesh = (0..MESH)
                    .flat_map(|j| {
                        (0..MESH).map(move |i| {
                            (i as f32 / (MESH - 1) as f32, j as f32 / (MESH - 1) as f32)
                        })
                    })
                    .map(|(u, v)| {
                        let top = (
                            q[0].0 + (q[1].0 - q[0].0) * u,
                            q[0].1 + (q[1].1 - q[0].1) * u,
                        );
                        let bot = (
                            q[3].0 + (q[2].0 - q[3].0) * u,
                            q[3].1 + (q[2].1 - q[3].1) * u,
                        );
                        (top.0 + (bot.0 - top.0) * v, top.1 + (bot.1 - top.1) * v)
                    })
                    .collect();
            }
        }
    }

    /// Pixels RGBA da imagem de `w×h` transformados conforme o modo atual.
    fn warp(&self, rgba: &[u8], w: u32, h: u32, interp: Interp) -> Option<Vec<u8>> {
        match self.mode {
            TMode::Free | TMode::Skew => {
                transform::warp_affine(rgba, w, h, &self.affine(), w, h, interp)
            }
            TMode::Distort | TMode::Perspective => {
                transform::warp_quad(rgba, w, h, rect_corners(self.bounds), self.quad, interp)
            }
            TMode::Warp => {
                transform::warp_mesh(rgba, w, h, MESH, MESH, &self.grid_src(), &self.mesh, interp)
            }
        }
    }

    fn render(&self, orig: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
        match &self.mask {
            None => self.warp(orig, w, h, self.interp),
            Some(m) => {
                let mut lifted = orig.to_vec();
                let mut rest = orig.to_vec();
                for ((l, r), &mv) in lifted
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(rest.as_chunks_mut::<4>().0.iter_mut())
                    .zip(m.iter())
                {
                    l[3] = ((u32::from(l[3]) * u32::from(mv) + 127) / 255) as u8;
                    r[3] = ((u32::from(r[3]) * (255 - u32::from(mv)) + 127) / 255) as u8;
                }
                let mut warped = self.warp(&lifted, w, h, self.interp)?;
                for (wp, rp) in warped
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(rest.as_chunks::<4>().0)
                {
                    *wp = over(*wp, *rp, 1.0);
                }
                Some(warped)
            }
        }
    }

    /// Recalcula a prévia nas camadas (e na seleção) do documento.
    fn apply(&mut self, open: &mut OpenDoc) {
        let (w, h) = (open.doc.width, open.doc.height);
        let mut results = Vec::new();
        for (id, orig) in &self.originals {
            match self.render(orig, w, h) {
                Some(px) => results.push((*id, px)),
                None => {
                    self.notice = Some("Transformação degenerada (área zero).".into());
                    return;
                }
            }
        }
        self.notice = None;
        for (id, px) in results {
            if let Some(l) = open.doc.layer_mut(id) {
                l.pixels = Arc::new(px);
            }
        }
        if let Some(m) = &self.mask {
            let rgba: Vec<u8> = m.iter().flat_map(|&v| [v, v, v, 255]).collect();
            let moved = self.warp(&rgba, w, h, Interp::Bilinear).map(|o| {
                o.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| p[0])
                    .collect::<Vec<u8>>()
            });
            open.doc.selection = Selection::from_mask(moved.and_then(nonempty));
        }
        self.changed = true;
        open.mark_dirty(Rect::full(w, h));
    }

    pub fn cancel(self, open: &mut OpenDoc) {
        self.before.apply_to(&mut open.doc);
        open.mark_dirty(Rect::full(open.doc.width, open.doc.height));
        open.doc.bump_view();
    }

    /// Confirma: uma única ação de histórico (camadas + seleção).
    pub fn commit(self, open: &mut OpenDoc, label: &str) {
        if !self.changed {
            return;
        }
        open.record_state(label, self.before);
    }

    pub fn points(&self) -> Vec<Pt> {
        match self.mode {
            TMode::Warp => self.mesh.clone(),
            _ => self.current_quad().to_vec(),
        }
    }

    pub fn shape(&self) -> Shape {
        match self.mode {
            TMode::Warp => Shape::Mesh(MESH, MESH, self.mesh.clone()),
            _ => {
                let q = self.current_quad();
                Shape::Poly(vec![q[0], q[1], q[2], q[3], q[0]])
            }
        }
    }

    pub fn handle(&mut self, open: &mut OpenDoc, out: &CanvasOut) {
        if open.doc.id != self.doc {
            return;
        }
        let zoom = out.zoom.max(0.001);
        let hit = 10.0 / zoom;
        if let Some(p) = out.pressed {
            let p = (p[0], p[1]);
            let pts = self.points();
            let near = nearest_within(&pts, p, hit);
            self.drag = match (self.mode, near) {
                (TMode::Warp, Some(i)) => Some(Drag::Mesh(i)),
                (TMode::Distort | TMode::Perspective, Some(i)) => Some(Drag::Quad(i)),
                (TMode::Free, Some(i)) => Some(Drag::Scale {
                    idx: i,
                    quad0: self.params_quad(),
                    p0: (self.sx_pct, self.sy_pct, self.tx, self.ty),
                }),
                _ => {
                    if point_in_quad(&self.current_quad(), p) {
                        Some(Drag::Move {
                            start: p,
                            t0: (self.tx, self.ty),
                        })
                    } else if matches!(self.mode, TMode::Free) {
                        let pv = self.pivot();
                        let c = (pv.0 + self.tx, pv.1 + self.ty);
                        Some(Drag::Rotate {
                            a0: (p.1 - c.1).atan2(p.0 - c.0),
                            rot0: self.rot,
                        })
                    } else {
                        None
                    }
                }
            };
        }
        let Some(m) = out.moves.last().map(|m| (m[0], m[1])) else {
            if out.released || !out.down {
                self.drag = None;
            }
            return;
        };
        let shift = out.shift;
        let mut moved = false;
        match &self.drag {
            Some(Drag::Move { start, t0 }) => {
                if matches!(self.mode, TMode::Free | TMode::Skew) {
                    self.tx = t0.0 + (m.0 - start.0).round();
                    self.ty = t0.1 + (m.1 - start.1).round();
                } else {
                    let (dx, dy) = ((m.0 - start.0).round(), (m.1 - start.1).round());
                    // Move todos os pontos de controle de uma vez.
                    let q0 = self.quad;
                    let _ = q0;
                    for q in &mut self.quad {
                        q.0 += dx;
                        q.1 += dy;
                    }
                    for q in &mut self.mesh {
                        q.0 += dx;
                        q.1 += dy;
                    }
                    self.drag = Some(Drag::Move { start: m, t0: *t0 });
                }
                moved = true;
            }
            Some(Drag::Scale { idx, quad0, p0 }) => {
                let (i, q0, (sx0, sy0, tx0, ty0)) = (*idx, *quad0, *p0);
                let opp = q0[(i + 2) % 4];
                let u = norm((q0[1].0 - q0[0].0, q0[1].1 - q0[0].1));
                let v = norm((q0[3].0 - q0[0].0, q0[3].1 - q0[0].1));
                let c0 = (q0[i].0 - opp.0, q0[i].1 - opp.1);
                let d = (m.0 - opp.0, m.1 - opp.1);
                let (du0, dv0) = (dot(c0, u), dot(c0, v));
                if du0.abs() > 1e-3 && dv0.abs() > 1e-3 {
                    let (mut su, mut sv) = (dot(d, u) / du0, dot(d, v) / dv0);
                    if self.lock_aspect || shift {
                        let s = (su.abs() + sv.abs()) / 2.0;
                        su = s * su.signum();
                        sv = s * sv.signum();
                    }
                    self.sx_pct = sx0 * su;
                    self.sy_pct = sy0 * sv;
                    self.tx = tx0;
                    self.ty = ty0;
                    let new_opp = self.params_quad()[(i + 2) % 4];
                    self.tx += opp.0 - new_opp.0;
                    self.ty += opp.1 - new_opp.1;
                    moved = true;
                }
            }
            Some(Drag::Rotate { a0, rot0 }) => {
                let pv = self.pivot();
                let c = (pv.0 + self.tx, pv.1 + self.ty);
                let a = (m.1 - c.1).atan2(m.0 - c.0);
                let mut r = rot0 + (a - a0).to_degrees();
                if shift {
                    r = (r / 15.0).round() * 15.0;
                }
                self.rot = r;
                moved = true;
            }
            Some(Drag::Quad(i)) => {
                let i = *i;
                let old = self.quad[i];
                self.quad[i] = m;
                if self.mode == TMode::Perspective {
                    let j = i ^ 1;
                    let cx = (self.quad[i].0 + self.quad[j].0) / 2.0;
                    let _ = (old, cx);
                    let mid = (old.0 + self.quad[j].0) / 2.0;
                    self.quad[j] = (2.0 * mid - m.0, m.1);
                }
                moved = true;
            }
            Some(Drag::Mesh(i)) => {
                if let Some(p) = self.mesh.get_mut(*i) {
                    *p = m;
                    moved = true;
                }
            }
            None => {}
        }
        if moved {
            self.apply(open);
        }
        if out.released || !out.down {
            self.drag = None;
        }
    }

    /// Campos numéricos da barra de opções. Retorna `true` se algo mudou.
    pub fn options_ui(&mut self, ui: &mut egui::Ui) -> (bool, bool, bool) {
        let mut changed = false;
        let (mut apply, mut cancel) = (false, false);
        ui.horizontal_wrapped(|ui| {
            ui.strong("Transformar");
            let before_mode = self.mode;
            egui::ComboBox::from_id_salt("tmode")
                .selected_text(self.mode.label())
                .show_ui(ui, |ui| {
                    for m in [
                        TMode::Free,
                        TMode::Skew,
                        TMode::Distort,
                        TMode::Perspective,
                        TMode::Warp,
                    ] {
                        ui.selectable_value(&mut self.mode, m, m.label());
                    }
                });
            if self.mode != before_mode {
                let new_mode = self.mode;
                self.mode = before_mode;
                self.set_mode(new_mode);
                changed = true;
            }
            if matches!(self.mode, TMode::Free | TMode::Skew) {
                let names = ["↖", "↑", "↗", "←", "•", "→", "↙", "↓", "↘"];
                let cur = (self.pivot_anchor.1 * 2.0).round() as usize * 3
                    + (self.pivot_anchor.0 * 2.0).round() as usize;
                egui::ComboBox::from_id_salt("tpivot")
                    .selected_text(format!("Pivô {}", names[cur.min(8)]))
                    .width(70.0)
                    .show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            if ui.selectable_label(i == cur, *n).clicked() {
                                self.set_pivot_anchor(((i % 3) as f32 / 2.0, (i / 3) as f32 / 2.0));
                                changed = true;
                            }
                        }
                    });
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut self.tx)
                            .prefix("X ")
                            .suffix(" px"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut self.ty)
                            .prefix("Y ")
                            .suffix(" px"),
                    )
                    .changed();
                if self.mode == TMode::Free {
                    let sx0 = self.sx_pct;
                    if ui
                        .add(
                            egui::DragValue::new(&mut self.sx_pct)
                                .prefix("L ")
                                .suffix("%")
                                .speed(0.5),
                        )
                        .changed()
                    {
                        if self.lock_aspect && sx0.abs() > 1e-3 {
                            self.sy_pct *= self.sx_pct / sx0;
                        }
                        changed = true;
                    }
                    let sy0 = self.sy_pct;
                    if ui
                        .add(
                            egui::DragValue::new(&mut self.sy_pct)
                                .prefix("A ")
                                .suffix("%")
                                .speed(0.5),
                        )
                        .changed()
                    {
                        if self.lock_aspect && sy0.abs() > 1e-3 {
                            self.sx_pct *= self.sy_pct / sy0;
                        }
                        changed = true;
                    }
                    ui.checkbox(&mut self.lock_aspect, "Proporção");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut self.rot)
                                .prefix("Giro ")
                                .suffix("°"),
                        )
                        .changed();
                } else {
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut self.skx)
                                .prefix("Incl. H ")
                                .suffix("°")
                                .range(-80.0..=80.0),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut self.sky)
                                .prefix("Incl. V ")
                                .suffix("°")
                                .range(-80.0..=80.0),
                        )
                        .changed();
                }
            } else {
                ui.label("Arraste os pontos de controle (Shift: sem efeito).");
            }
            egui::ComboBox::from_id_salt("tinterp")
                .selected_text(self.interp.label())
                .show_ui(ui, |ui| {
                    for i in Interp::ALL {
                        changed |= ui
                            .selectable_value(&mut self.interp, i, i.label())
                            .changed();
                    }
                });
            apply = ui.button("Aplicar (Enter)").clicked();
            cancel = ui.button("Cancelar (Esc)").clicked();
            if let Some(n) = &self.notice {
                ui.colored_label(crate::theme::COPPER, n);
            }
        });
        (changed, apply, cancel)
    }

    /// Reaplica a prévia após edição numérica.
    pub fn refresh(&mut self, open: &mut OpenDoc) {
        self.apply(open);
    }
}

// ---- Recorte ----

#[derive(Clone, Copy)]
enum CropDrag {
    New((f32, f32)),
    Move((f32, f32), [f32; 4]),
    Handle(usize, [f32; 4]),
}

#[derive(Default)]
pub struct CropState {
    rect: Option<[f32; 4]>, // x0, y0, x1, y1 normalizados
    drag: Option<CropDrag>,
}

impl CropState {
    pub fn active(&self) -> bool {
        self.rect.is_some()
    }

    fn norm_rect(&self) -> Option<[f32; 4]> {
        self.rect.map(|r| {
            [
                r[0].min(r[2]),
                r[1].min(r[3]),
                r[0].max(r[2]),
                r[1].max(r[3]),
            ]
        })
    }

    pub fn shape(&self) -> Option<Shape> {
        let r = self.norm_rect()?;
        Some(Shape::Rect((r[0], r[1]), (r[2], r[3])))
    }

    pub fn points(&self) -> Vec<Pt> {
        let Some(r) = self.norm_rect() else {
            return Vec::new();
        };
        let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
        vec![
            (r[0], r[1]),
            (cx, r[1]),
            (r[2], r[1]),
            (r[2], cy),
            (r[2], r[3]),
            (cx, r[3]),
            (r[0], r[3]),
            (r[0], cy),
        ]
    }

    pub fn cancel(&mut self) {
        *self = Self::default();
    }

    /// Retângulo inteiro (x, y, w, h) a recortar.
    pub fn int_rect(&self) -> Option<(i32, i32, u32, u32)> {
        let r = self.norm_rect()?;
        let (x0, y0, x1, y1) = (
            r[0].round() as i32,
            r[1].round() as i32,
            r[2].round() as i32,
            r[3].round() as i32,
        );
        (x1 > x0 && y1 > y0).then_some((x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    pub fn handle(&mut self, out: &CanvasOut) {
        let hit = 10.0 / out.zoom.max(0.001);
        if let Some(p) = out.pressed {
            let p = (p[0], p[1]);
            let pts = self.points();
            let near = nearest_within(&pts, p, hit);
            self.drag = match (near, self.norm_rect()) {
                (Some(i), Some(r)) => Some(CropDrag::Handle(i, r)),
                (None, Some(r)) if p.0 >= r[0] && p.0 <= r[2] && p.1 >= r[1] && p.1 <= r[3] => {
                    Some(CropDrag::Move(p, r))
                }
                _ => {
                    self.rect = Some([p.0, p.1, p.0, p.1]);
                    Some(CropDrag::New(p))
                }
            };
        }
        let Some(m) = out.moves.last().map(|m| (m[0].round(), m[1].round())) else {
            if out.released || !out.down {
                self.drag = None;
            }
            return;
        };
        match self.drag {
            Some(CropDrag::New(s)) => self.rect = Some([s.0, s.1, m.0, m.1]),
            Some(CropDrag::Move(s, r)) => {
                let (dx, dy) = (m.0 - s.0, m.1 - s.1);
                self.rect = Some([r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy]);
            }
            Some(CropDrag::Handle(i, r)) => {
                let mut n = r;
                // 0 TL,1 T,2 TR,3 R,4 BR,5 B,6 BL,7 L
                if matches!(i, 0 | 6 | 7) {
                    n[0] = m.0;
                }
                if (2..=4).contains(&i) {
                    n[2] = m.0;
                }
                if (0..=2).contains(&i) {
                    n[1] = m.1;
                }
                if (4..=6).contains(&i) {
                    n[3] = m.1;
                }
                self.rect = Some(n);
            }
            None => {}
        }
        if out.released || !out.down {
            self.drag = None;
        }
    }
}

// ---- Diálogos ----

pub struct ImageSizeDialog {
    pub w: u32,
    pub h: u32,
    pub lock: bool,
    ratio: f32,
    pub interp: Interp,
    pub keep_physical: bool,
}

pub struct CanvasSizeDialog {
    pub w: u32,
    pub h: u32,
    pub anchor: (f32, f32),
    pub fill_bg: bool,
}

pub struct StraightenDialog {
    pub angle: f32,
    pub interp: Interp,
}

#[derive(Default)]
pub struct GeoState {
    pub transform: Option<TransformSession>,
    pub crop: CropState,
    pub image_size: Option<ImageSizeDialog>,
    pub canvas_size: Option<CanvasSizeDialog>,
    pub straighten: Option<StraightenDialog>,
}

impl GeoState {
    pub fn busy(&self) -> bool {
        self.transform.is_some()
    }

    pub fn cancel(&mut self, open: &mut OpenDoc) {
        if let Some(t) = self.transform.take() {
            t.cancel(open);
        }
        self.crop.cancel();
    }

    pub fn start_transform(&mut self, open: &mut OpenDoc, mode: TMode) -> Option<String> {
        if self.transform.is_some() {
            return None;
        }
        match TransformSession::start(open, mode) {
            Ok(s) => {
                self.transform = Some(s);
                None
            }
            Err(e) => Some(e),
        }
    }

    pub fn commit(&mut self, open: &mut OpenDoc) {
        if let Some(t) = self.transform.take() {
            t.commit(open, "Transformar");
        }
    }

    /// Operações imediatas de uma ação só: espelhar/girar o conteúdo das camadas.
    pub fn immediate(
        &mut self,
        open: &mut OpenDoc,
        f: impl FnOnce(&mut TransformSession),
    ) -> Option<String> {
        let mut s = match TransformSession::start(open, TMode::Free) {
            Ok(s) => s,
            Err(e) => return Some(e),
        };
        s.interp = Interp::Nearest;
        f(&mut s);
        s.apply(open);
        if s.notice.is_some() {
            let n = s.notice.clone();
            s.cancel(open);
            return n;
        }
        s.commit(open, "Transformar");
        None
    }

    pub fn flip(&mut self, open: &mut OpenDoc, horizontal: bool) -> Option<String> {
        self.immediate(open, |s| {
            if horizontal {
                s.sx_pct = -100.0;
            } else {
                s.sy_pct = -100.0;
            }
        })
    }

    pub fn rotate_layer(&mut self, open: &mut OpenDoc, deg: f32) -> Option<String> {
        self.immediate(open, |s| s.rot = deg)
    }

    pub fn open_image_size(&mut self, open: &OpenDoc) {
        self.image_size = Some(ImageSizeDialog {
            w: open.doc.width,
            h: open.doc.height,
            lock: true,
            ratio: open.doc.width as f32 / open.doc.height.max(1) as f32,
            interp: Interp::Bicubic,
            keep_physical: false,
        });
    }

    pub fn open_canvas_size(&mut self, open: &OpenDoc) {
        self.canvas_size = Some(CanvasSizeDialog {
            w: open.doc.width,
            h: open.doc.height,
            anchor: (0.5, 0.5),
            fill_bg: false,
        });
    }

    pub fn show_dialogs(
        &mut self,
        ctx: &egui::Context,
        open: &mut OpenDoc,
        bg: [u8; 4],
    ) -> Option<String> {
        let mut notice = None;
        if let Some(mut d) = self.image_size.take() {
            let (mut ok, mut cancel) = (false, false);
            egui::Window::new("Tamanho da imagem")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Atual: {} × {} px",
                        open.doc.width, open.doc.height
                    ));
                    let (w0, h0) = (d.w, d.h);
                    ui.horizontal(|ui| {
                        ui.label("Largura");
                        ui.add(
                            egui::DragValue::new(&mut d.w)
                                .range(1..=orvalith_core::Document::MAX_SIDE)
                                .suffix(" px"),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Altura");
                        ui.add(
                            egui::DragValue::new(&mut d.h)
                                .range(1..=orvalith_core::Document::MAX_SIDE)
                                .suffix(" px"),
                        );
                    });
                    if d.lock {
                        if d.w != w0 {
                            d.h = ((d.w as f32 / d.ratio).round() as u32).max(1);
                        } else if d.h != h0 {
                            d.w = ((d.h as f32 * d.ratio).round() as u32).max(1);
                        }
                    }
                    ui.checkbox(&mut d.lock, "Manter proporção");
                    ui.checkbox(
                        &mut d.keep_physical,
                        "Manter o tamanho físico (ajusta os ppi)",
                    );
                    egui::ComboBox::from_id_salt("isz_interp")
                        .selected_text(d.interp.label())
                        .show_ui(ui, |ui| {
                            for i in Interp::ALL {
                                ui.selectable_value(&mut d.interp, i, i.label());
                            }
                        });
                    ui.small("Pixel art: use vizinho mais próximo. Fotos: bicúbica.");
                    ui.horizontal(|ui| {
                        ok = ui.button("Aplicar").clicked();
                        cancel = ui.button("Cancelar").clicked();
                    });
                });
            if ok {
                let ppi = d
                    .keep_physical
                    .then(|| open.doc.ppi * d.w as f32 / open.doc.width as f32);
                match docops::resize_image(&open.doc, d.w, d.h, d.interp, ppi) {
                    Ok(st) => open.apply_state("Tamanho da imagem", st),
                    Err(e) => notice = Some(e),
                }
            } else if !cancel {
                self.image_size = Some(d);
            }
        }
        if let Some(mut d) = self.canvas_size.take() {
            let (mut ok, mut cancel) = (false, false);
            egui::Window::new("Tamanho da tela")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Atual: {} × {} px",
                        open.doc.width, open.doc.height
                    ));
                    ui.horizontal(|ui| {
                        ui.label("Largura");
                        ui.add(
                            egui::DragValue::new(&mut d.w)
                                .range(1..=orvalith_core::Document::MAX_SIDE)
                                .suffix(" px"),
                        );
                        ui.label("Altura");
                        ui.add(
                            egui::DragValue::new(&mut d.h)
                                .range(1..=orvalith_core::Document::MAX_SIDE)
                                .suffix(" px"),
                        );
                    });
                    ui.label("Âncora");
                    egui::Grid::new("anchor").show(ui, |ui| {
                        for ay in [0.0, 0.5, 1.0] {
                            for ax in [0.0, 0.5, 1.0] {
                                let on = (d.anchor.0 - ax).abs() < 0.01
                                    && (d.anchor.1 - ay).abs() < 0.01;
                                if ui
                                    .selectable_label(on, if on { "●" } else { "○" })
                                    .clicked()
                                {
                                    d.anchor = (ax, ay);
                                }
                            }
                            ui.end_row();
                        }
                    });
                    ui.checkbox(
                        &mut d.fill_bg,
                        "Preencher a área nova da camada inferior com a cor de fundo",
                    );
                    ui.small("Camadas ficam recortadas à tela: o que ficar fora é descartado.");
                    ui.horizontal(|ui| {
                        ok = ui.button("Aplicar").clicked();
                        cancel = ui.button("Cancelar").clicked();
                    });
                });
            if ok {
                let fill = d.fill_bg.then_some(bg);
                match docops::canvas_size(&open.doc, d.w, d.h, d.anchor, fill) {
                    Ok(st) => open.apply_state("Tamanho da tela", st),
                    Err(e) => notice = Some(e),
                }
            } else if !cancel {
                self.canvas_size = Some(d);
            }
        }
        if let Some(mut d) = self.straighten.take() {
            let (mut ok, mut cancel) = (false, false);
            egui::Window::new("Endireitar")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.add(egui::Slider::new(&mut d.angle, -45.0..=45.0).text("Ângulo °"));
                    ui.label("Gira o conteúdo e recorta ao maior retângulo sem cantos vazios.");
                    egui::ComboBox::from_id_salt("str_interp")
                        .selected_text(d.interp.label())
                        .show_ui(ui, |ui| {
                            for i in Interp::ALL {
                                ui.selectable_value(&mut d.interp, i, i.label());
                            }
                        });
                    ui.horizontal(|ui| {
                        ok = ui.button("Aplicar").clicked();
                        cancel = ui.button("Cancelar").clicked();
                    });
                });
            if ok {
                match docops::straighten(&open.doc, d.angle, d.interp) {
                    Ok(st) => open.apply_state("Endireitar", st),
                    Err(e) => notice = Some(e),
                }
            } else if !cancel {
                self.straighten = Some(d);
            }
        }
        notice
    }
}

// ---- Comandos diretos ----

pub fn orient(open: &mut OpenDoc, o: Orient, label: &str) -> Option<String> {
    match docops::orient_canvas(&open.doc, o) {
        Ok(st) => {
            open.apply_state(label, st);
            None
        }
        Err(e) => Some(e),
    }
}

pub fn crop_to_selection(open: &mut OpenDoc) -> Option<String> {
    let mask = open.doc.selection.mask.clone()?;
    let b = sel::bounds(&mask, open.doc.width, open.doc.height)?;
    match docops::crop(&open.doc, b.x as i32, b.y as i32, b.w, b.h) {
        Ok(st) => {
            open.apply_state("Recortar para a seleção", st);
            None
        }
        Err(e) => Some(e),
    }
}

pub fn apply_crop(open: &mut OpenDoc, c: &CropState) -> Option<String> {
    let (x, y, w, h) = c.int_rect()?;
    match docops::crop(&open.doc, x, y, w, h) {
        Ok(st) => {
            open.apply_state("Recortar", st);
            None
        }
        Err(e) => Some(e),
    }
}

pub fn align(open: &mut OpenDoc, kind: AlignKind) -> Option<String> {
    let reference = open
        .doc
        .selection
        .mask
        .as_ref()
        .and_then(|m| sel::bounds(m, open.doc.width, open.doc.height))
        .unwrap_or(Rect::full(open.doc.width, open.doc.height));
    let ids = open.selection_ids();
    match docops::align(&open.doc, &ids, kind, reference) {
        Some(layers) => {
            open.edit_layers("Alinhar", |ls| *ls = layers);
            None
        }
        None => Some("Nada a alinhar (já alinhado, bloqueado ou vazio).".into()),
    }
}

pub fn distribute(open: &mut OpenDoc, horizontal: bool) -> Option<String> {
    let ids = open.selection_ids();
    match docops::distribute(&open.doc, &ids, horizontal) {
        Some(layers) => {
            open.edit_layers("Distribuir", |ls| *ls = layers);
            None
        }
        None => Some("Selecione 3 ou mais camadas (Ctrl+clique) com conteúdo.".into()),
    }
}

pub fn clear_guides(open: &mut OpenDoc) {
    if open.doc.guides.is_empty() {
        return;
    }
    let before = DocState::of(&open.doc);
    let mut after = before.clone();
    after.guides.clear();
    open.execute_nonpixel(Box::new(DocEdit::new("Limpar guias", before, after)));
    open.doc.modified = true;
}

/// Registra uma troca de camadas já aplicada (usado pelo painel/Alinhar).
#[allow(dead_code)]
pub fn layers_edit(
    label: &str,
    before: Vec<orvalith_core::Layer>,
    after: Vec<orvalith_core::Layer>,
) -> LayersEdit {
    LayersEdit::new(label, before, after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::Document;

    fn open() -> OpenDoc {
        let mut d = Document::new("t", 20, 12, None).unwrap();
        for y in 2..6 {
            for x in 3..9 {
                d.layers[0].set_pixel(x, y, [255, 0, 0, 255]);
            }
        }
        OpenDoc::new(d)
    }

    fn red_count(o: &OpenDoc) -> usize {
        o.doc.layers[0]
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 0)
            .count()
    }

    #[test]
    fn flip_horizontal_is_one_exact_undoable_action() {
        let mut o = open();
        let original = o.doc.layers[0].pixels.clone();
        let mut g = GeoState::default();
        assert!(g.flip(&mut o, true).is_none());
        // Conteúdo em x 3..9 (centro 6) espelha para 3..9: simétrico no próprio intervalo.
        assert_eq!(o.doc.layers[0].pixels, original);
        assert_eq!(o.history.labels(), vec!["Transformar"]);
        // Mover antes para quebrar a simetria.
        o.doc.layers[0].set_pixel(8, 5, [0, 255, 0, 255]);
        let before = o.doc.layers[0].pixels.clone();
        assert!(g.flip(&mut o, true).is_none());
        assert_eq!(o.doc.layers[0].pixel(3, 5), Some([0, 255, 0, 255]));
        assert_eq!(o.doc.layers[0].pixel(8, 5), Some([255, 0, 0, 255]));
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[0].pixels, before);
        o.history.redo(&mut o.doc);
        assert_eq!(o.doc.layers[0].pixel(3, 5), Some([0, 255, 0, 255]));
    }

    #[test]
    fn rotate_layer_180_and_alpha_edges_stay_clean() {
        let mut o = open();
        let mut g = GeoState::default();
        o.doc.layers[0].set_pixel(8, 5, [0, 255, 0, 255]);
        let n0 = red_count(&o);
        assert!(g.rotate_layer(&mut o, 180.0).is_none());
        assert_eq!(red_count(&o), n0);
        // Conteúdo 3..9 × 2..6 gira em torno de (6, 4): o pixel verde de (8,5) vai para (3,2).
        assert_eq!(o.doc.layers[0].pixel(3, 2), Some([0, 255, 0, 255]));
        o.doc.layers[0].set_pixel(3, 2, [255, 0, 0, 255]);
        // Rotação livre bilinear: todas as cores com alfa > 0 continuam vermelhas puras (sem halo).
        let mut s = TransformSession::start(&o, TMode::Free).unwrap();
        s.interp = Interp::Bilinear;
        s.rot = 20.0;
        s.tx = 0.5;
        s.apply(&mut o);
        for p in o.doc.layers[0].pixels.as_chunks::<4>().0 {
            if p[3] > 0 {
                let pure_red = p[0] >= 254 && p[1] <= 1 && p[2] <= 1;
                let pure_green = p[1] >= 254 && p[0] <= 1 && p[2] <= 1;
                assert!(pure_red || pure_green, "{p:?}");
            }
        }
        s.cancel(&mut o);
    }

    #[test]
    fn session_cancel_restores_and_commit_is_single_action() {
        let mut o = open();
        let original = o.doc.layers[0].pixels.clone();
        let mut g = GeoState::default();
        assert!(g.start_transform(&mut o, TMode::Free).is_none());
        {
            let t = g.transform.as_mut().unwrap();
            t.tx = 4.0;
            t.ty = 3.0;
            t.interp = Interp::Nearest;
            t.refresh(&mut o);
        }
        assert_ne!(o.doc.layers[0].pixels, original);
        g.cancel(&mut o);
        assert_eq!(o.doc.layers[0].pixels, original);
        assert!(o.history.labels().is_empty());
        // Confirmar: um único item de histórico, deslocamento exato e negativo recortando.
        assert!(g.start_transform(&mut o, TMode::Free).is_none());
        {
            let t = g.transform.as_mut().unwrap();
            t.tx = -5.0;
            t.interp = Interp::Nearest;
            t.refresh(&mut o);
        }
        g.commit(&mut o);
        assert_eq!(o.history.labels(), vec!["Transformar"]);
        assert_eq!(o.doc.layers[0].pixel(0, 2), Some([255, 0, 0, 255]));
        assert_eq!(o.doc.layers[0].pixel(4, 2).unwrap()[3], 0);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[0].pixels, original);
    }

    #[test]
    fn transform_with_selection_moves_only_selected_pixels_and_selection() {
        let mut o = open();
        o.doc.layers[0].set_pixel(15, 8, [0, 0, 255, 255]);
        o.doc.selection =
            Selection::from_mask(Some(sel::rect_mask(20, 12, (3.0, 2.0), (9.0, 6.0), true)));
        let mut g = GeoState::default();
        assert!(g.start_transform(&mut o, TMode::Free).is_none());
        {
            let t = g.transform.as_mut().unwrap();
            t.tx = 4.0;
            t.interp = Interp::Nearest;
            t.refresh(&mut o);
        }
        g.commit(&mut o);
        // Pixel azul fora da seleção não se mexe; o bloco vermelho andou 4 px.
        assert_eq!(o.doc.layers[0].pixel(15, 8), Some([0, 0, 255, 255]));
        assert_eq!(o.doc.layers[0].pixel(3, 2).unwrap()[3], 0);
        assert_eq!(o.doc.layers[0].pixel(7, 2), Some([255, 0, 0, 255]));
        let b = sel::bounds(o.doc.selection.mask.as_ref().unwrap(), 20, 12).unwrap();
        assert_eq!((b.x, b.w), (7, 6));
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[0].pixel(3, 2), Some([255, 0, 0, 255]));
        let b = sel::bounds(o.doc.selection.mask.as_ref().unwrap(), 20, 12).unwrap();
        assert_eq!(b.x, 3);
    }

    #[test]
    fn locked_layers_and_degenerate_transforms_are_refused() {
        let mut o = open();
        o.doc.layers[0].lock_position = true;
        assert!(TransformSession::start(&o, TMode::Free).is_err());
        o.doc.layers[0].lock_position = false;
        o.doc.layers[0].lock_pixels = true;
        assert!(TransformSession::start(&o, TMode::Free).is_err());
        o.doc.layers[0].lock_pixels = false;
        let mut s = TransformSession::start(&o, TMode::Free).unwrap();
        let before = o.doc.layers[0].pixels.clone();
        s.sx_pct = 0.0;
        s.apply(&mut o);
        assert!(s.notice.is_some());
        assert_eq!(o.doc.layers[0].pixels, before);
    }

    #[test]
    fn distort_perspective_and_warp_modes_render() {
        let mut o = open();
        let original = o.doc.layers[0].pixels.clone();
        for mode in [TMode::Distort, TMode::Perspective, TMode::Warp] {
            let mut s = TransformSession::start(&o, mode).unwrap();
            s.interp = Interp::Bilinear;
            // Estado inicial = identidade.
            s.apply(&mut o);
            assert_eq!(o.doc.layers[0].pixels, original, "{mode:?} identidade");
            match mode {
                TMode::Warp => s.mesh[5].0 += 2.0,
                _ => s.quad[0].0 += 2.0,
            }
            s.apply(&mut o);
            assert_ne!(o.doc.layers[0].pixels, original, "{mode:?} alterou");
            s.cancel(&mut o);
            assert_eq!(o.doc.layers[0].pixels, original);
        }
    }

    #[test]
    fn corner_drag_scales_around_opposite_corner() {
        let mut o = open();
        let mut s = TransformSession::start(&o, TMode::Free).unwrap();
        s.interp = Interp::Nearest;
        let b = s.bounds;
        let (x0, y0) = (b.x as f32, b.y as f32);
        let br = ((b.right()) as f32, (b.bottom()) as f32);
        // Arrasta o canto inferior direito para dobrar a largura e a altura.
        let out = CanvasOut {
            pressed: Some([br.0, br.1]),
            zoom: 4.0,
            down: true,
            pressure: 1.0,
            ..Default::default()
        };
        s.handle(&mut o, &out);
        let out = CanvasOut {
            moves: vec![[x0 + 2.0 * b.w as f32, y0 + 2.0 * b.h as f32]],
            zoom: 4.0,
            down: true,
            pressure: 1.0,
            ..Default::default()
        };
        s.handle(&mut o, &out);
        assert!(
            (s.sx_pct - 200.0).abs() < 0.5 && (s.sy_pct - 200.0).abs() < 0.5,
            "{} {}",
            s.sx_pct,
            s.sy_pct
        );
        // O canto superior esquerdo (oposto) ficou parado.
        let q = s.params_quad();
        assert!((q[0].0 - x0).abs() < 0.1 && (q[0].1 - y0).abs() < 0.1);
        s.cancel(&mut o);
    }

    #[test]
    fn crop_tool_drag_move_handles_and_apply() {
        let mut o = open();
        let mut c = CropState::default();
        let mk = |p: Option<[f32; 2]>, moves: Vec<[f32; 2]>, released: bool| CanvasOut {
            pressed: p,
            moves,
            down: !released,
            released,
            zoom: 4.0,
            pressure: 1.0,
            ..Default::default()
        };
        c.handle(&mk(Some([2.0, 1.0]), vec![], false));
        c.handle(&mk(None, vec![[12.0, 9.0]], false));
        c.handle(&mk(None, vec![], true));
        assert_eq!(c.int_rect(), Some((2, 1, 10, 8)));
        // Arrasta a alça direita (índice 3) para 14.
        c.handle(&mk(Some([12.0, 5.0]), vec![], false));
        c.handle(&mk(None, vec![[14.0, 5.0]], false));
        c.handle(&mk(None, vec![], true));
        assert_eq!(c.int_rect(), Some((2, 1, 12, 8)));
        assert!(apply_crop(&mut o, &c).is_none());
        assert_eq!((o.doc.width, o.doc.height), (12, 8));
        assert_eq!(o.doc.layers[0].pixel(1, 1), Some([255, 0, 0, 255]));
        o.history.undo(&mut o.doc);
        assert_eq!((o.doc.width, o.doc.height), (20, 12));
    }

    #[test]
    fn image_and_canvas_size_ops_are_undoable_actions() {
        let mut o = open();
        let st = docops::resize_image(&o.doc, 40, 24, Interp::Nearest, None).unwrap();
        o.apply_state("Tamanho da imagem", st);
        assert_eq!((o.doc.width, o.doc.height), (40, 24));
        assert_eq!(o.doc.layers[0].pixels.len(), 40 * 24 * 4);
        assert!(o.doc.modified);
        assert!(orient(&mut o, Orient::Cw90, "Girar tela").is_none());
        assert_eq!((o.doc.width, o.doc.height), (24, 40));
        o.history.undo(&mut o.doc);
        o.history.undo(&mut o.doc);
        assert_eq!((o.doc.width, o.doc.height), (20, 12));
        assert_eq!(o.history.labels().len(), 0);
    }

    #[test]
    fn align_and_distribute_commands_use_selection_and_report_noops() {
        let mut o = open();
        assert!(align(&mut o, AlignKind::Right).is_none());
        let b = docops::content_bounds(&o.doc.layers[0]).unwrap();
        assert_eq!(b.right(), 20);
        assert!(align(&mut o, AlignKind::Right).is_some(), "já alinhado");
        assert!(distribute(&mut o, true).is_some(), "precisa de 3 camadas");
        o.history.undo(&mut o.doc);
        assert_eq!(docops::content_bounds(&o.doc.layers[0]).unwrap().x, 3);
    }

    #[test]
    fn changing_pivot_keeps_the_current_result() {
        let o = open();
        let mut s = TransformSession::start(&o, TMode::Free).unwrap();
        s.rot = 30.0;
        s.sx_pct = 150.0;
        s.tx = 2.0;
        let before = s.params_quad();
        for anchor in [(0.0, 0.0), (1.0, 1.0), (0.5, 0.0), (0.5, 0.5)] {
            s.set_pivot_anchor(anchor);
            let q = s.params_quad();
            for (a, b) in before.iter().zip(&q) {
                assert!(
                    (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3,
                    "{anchor:?}"
                );
            }
        }
        // Com pivô no canto, escalar mantém o canto fixo.
        s.rot = 0.0;
        s.sx_pct = 100.0;
        s.sy_pct = 100.0;
        s.tx = 0.0;
        s.ty = 0.0;
        s.set_pivot_anchor((0.0, 0.0));
        s.sx_pct = 200.0;
        s.sy_pct = 200.0;
        let q = s.params_quad();
        assert!(
            (q[0].0 - s.bounds.x as f32).abs() < 1e-3 && (q[0].1 - s.bounds.y as f32).abs() < 1e-3
        );
        assert!((q[2].0 - (s.bounds.x as f32 + 2.0 * s.bounds.w as f32)).abs() < 1e-3);
    }

    #[test]
    fn crop_survives_save_and_reopen_with_final_dimensions() {
        let mut o = open();
        o.doc.guides = vec![orvalith_core::Guide {
            vertical: true,
            pos: 10.0,
        }];
        let st = docops::crop(&o.doc, -2, 1, 16, 9).unwrap();
        o.apply_state("Recortar", st);
        let back = orvalith_core::orva::read(&orvalith_core::orva::write(&o.doc).unwrap()).unwrap();
        assert_eq!((back.width, back.height), (16, 9));
        assert!(back
            .layers
            .iter()
            .all(|l| l.width == 16 && l.height == 9 && l.pixels.len() == 16 * 9 * 4));
        assert_eq!(back.layers[0].pixel(5, 1), Some([255, 0, 0, 255]));
        assert_eq!(back.layers[0].pixel(0, 0).unwrap()[3], 0);
        assert_eq!(back.guides[0].pos, 12.0);
    }

    #[test]
    fn guides_clear_is_undoable_and_marks_modified() {
        let mut o = open();
        o.doc.guides = vec![orvalith_core::Guide {
            vertical: true,
            pos: 5.0,
        }];
        clear_guides(&mut o);
        assert!(o.doc.guides.is_empty() && o.doc.modified);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.guides.len(), 1);
    }
}
