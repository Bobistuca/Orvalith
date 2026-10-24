//! Comandos de geometria: transformar, imagem/tela, recorte, alinhar e opções de visualização.

use orvalith_core::docops::{AlignKind, Orient};

use super::OrvalithApp;
use crate::canvas::RulerUnit;
use crate::geometry::{self, StraightenDialog, TMode};
use orvalith_core::transform::Interp;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GeoCmd {
    Transform(TMode),
    FlipH,
    FlipV,
    RotateLayer(f32),
    ImageSize,
    CanvasSize,
    Straighten,
    OrientCanvas(Orient),
    CropToSelection,
    Align(AlignKind),
    Distribute(bool),
    ClearGuides,
    ToggleRulers,
    ToggleGrid,
    ToggleGuides,
    ToggleSnap,
    SetUnit(RulerUnit),
}

impl OrvalithApp {
    pub(super) fn run_geo(&mut self, cmd: GeoCmd) {
        // Opções de visualização não dependem do documento.
        match cmd {
            GeoCmd::ToggleRulers => return self.view_opts.rulers = !self.view_opts.rulers,
            GeoCmd::ToggleGrid => return self.view_opts.grid = !self.view_opts.grid,
            GeoCmd::ToggleGuides => return self.view_opts.guides = !self.view_opts.guides,
            GeoCmd::ToggleSnap => return self.view_opts.snap = !self.view_opts.snap,
            GeoCmd::SetUnit(u) => return self.view_opts.unit = u,
            _ => {}
        }
        let Some(open) = self.docs.get_mut(self.active) else {
            return;
        };
        if open.qm.is_some() {
            self.tools.notice = Some("Saia da máscara rápida (Q) antes de transformar.".into());
            return;
        }
        if self.tools.is_busy() && !matches!(cmd, GeoCmd::ClearGuides) {
            return;
        }
        let bg = self.tools.bg;
        let _ = bg;
        let geo = &mut self.tools.geo;
        let notice = match cmd {
            GeoCmd::Transform(m) => geo.start_transform(open, m),
            GeoCmd::FlipH => geo.flip(open, true),
            GeoCmd::FlipV => geo.flip(open, false),
            GeoCmd::RotateLayer(d) => geo.rotate_layer(open, d),
            GeoCmd::ImageSize => {
                geo.open_image_size(open);
                None
            }
            GeoCmd::CanvasSize => {
                geo.open_canvas_size(open);
                None
            }
            GeoCmd::Straighten => {
                geo.straighten = Some(StraightenDialog {
                    angle: 0.0,
                    interp: Interp::Bicubic,
                });
                None
            }
            GeoCmd::OrientCanvas(o) => {
                let label = match o {
                    Orient::Cw90 => "Girar tela 90° horário",
                    Orient::Ccw90 => "Girar tela 90° anti-horário",
                    Orient::Rot180 => "Girar tela 180°",
                    Orient::FlipH => "Espelhar tela horizontalmente",
                    Orient::FlipV => "Espelhar tela verticalmente",
                };
                let r = geometry::orient(open, o, label);
                if r.is_none() {
                    self.tools
                        .geo
                        .recorded
                        .push(orvalith_core::actions::Step::Orient(match o {
                            Orient::Cw90 => orvalith_core::actions::Turn::Cw90,
                            Orient::Ccw90 => orvalith_core::actions::Turn::Ccw90,
                            Orient::Rot180 => orvalith_core::actions::Turn::Rot180,
                            Orient::FlipH => orvalith_core::actions::Turn::FlipH,
                            Orient::FlipV => orvalith_core::actions::Turn::FlipV,
                        }));
                }
                r
            }
            GeoCmd::CropToSelection => geometry::crop_to_selection(open).or_else(|| {
                open.doc
                    .selection
                    .mask
                    .is_none()
                    .then(|| "Faça uma seleção primeiro.".to_string())
            }),
            GeoCmd::Align(k) => geometry::align(open, k),
            GeoCmd::Distribute(h) => geometry::distribute(open, h),
            GeoCmd::ClearGuides => {
                geometry::clear_guides(open);
                None
            }
            _ => None,
        };
        if notice.is_some() {
            self.tools.notice = notice;
        }
    }
}
