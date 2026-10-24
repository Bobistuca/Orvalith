//! Ícones vetoriais próprios (traço de 1,5 px numa grade de 16 px), desenhados com o `Painter`.
//! Não dependem de fontes nem de arquivos externos e escalam com o DPI.

use eframe::egui::{self, pos2, Color32, Painter, Pos2, Rect, Stroke};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    NewDoc,
    Folder,
    Clock,
    Home,
    Grid,
    Help,
    Sliders,
    Shield,
    Image,
    Alert,
    Close,
    Plus,
}

/// Desenha `icon` dentro de `rect` (quadrado) com a cor dada.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let k = rect.width().min(rect.height()) / 16.0;
    let o = rect.center() - egui::vec2(8.0 * k, 8.0 * k);
    let pt = |x: f32, y: f32| pos2(o.x + x * k, o.y + y * k);
    let st = Stroke::new((1.5 * k).max(1.0), color);
    let line = |a: Pos2, b: Pos2| {
        p.line_segment([a, b], st);
    };
    let poly = |pts: &[Pos2], closed: bool| {
        p.add(egui::Shape::line(pts.to_vec(), st));
        if closed {
            if let (Some(f), Some(l)) = (pts.first(), pts.last()) {
                p.line_segment([*l, *f], st);
            }
        }
    };
    match icon {
        Icon::NewDoc => {
            poly(
                &[
                    pt(3.5, 1.5),
                    pt(9.5, 1.5),
                    pt(12.5, 4.5),
                    pt(12.5, 14.5),
                    pt(3.5, 14.5),
                ],
                true,
            );
            line(pt(9.5, 1.5), pt(9.5, 4.5));
            line(pt(9.5, 4.5), pt(12.5, 4.5));
            line(pt(8.0, 7.5), pt(8.0, 12.0));
            line(pt(5.75, 9.75), pt(10.25, 9.75));
        }
        Icon::Folder => {
            poly(
                &[
                    pt(1.5, 3.5),
                    pt(6.0, 3.5),
                    pt(7.5, 5.0),
                    pt(14.5, 5.0),
                    pt(14.5, 13.0),
                    pt(1.5, 13.0),
                ],
                true,
            );
        }
        Icon::Clock => {
            p.circle_stroke(pt(8.0, 8.0), 6.25 * k, st);
            poly(&[pt(8.0, 4.0), pt(8.0, 8.0), pt(10.75, 9.75)], false);
        }
        Icon::Home => {
            poly(&[pt(1.5, 8.0), pt(8.0, 2.0), pt(14.5, 8.0)], false);
            poly(
                &[pt(3.5, 7.0), pt(3.5, 14.0), pt(12.5, 14.0), pt(12.5, 7.0)],
                false,
            );
            line(pt(6.75, 14.0), pt(6.75, 10.0));
            line(pt(9.25, 14.0), pt(9.25, 10.0));
            line(pt(6.75, 10.0), pt(9.25, 10.0));
        }
        Icon::Grid => {
            for (x, y) in [(2.0, 2.0), (9.0, 2.0), (2.0, 9.0), (9.0, 9.0)] {
                p.rect_stroke(
                    Rect::from_min_size(pt(x, y), egui::vec2(5.0 * k, 5.0 * k)),
                    1.0 * k,
                    st,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Icon::Help => {
            p.circle_stroke(pt(8.0, 8.0), 6.25 * k, st);
            poly(
                &[
                    pt(6.0, 6.5),
                    pt(6.6, 5.2),
                    pt(8.0, 4.7),
                    pt(9.4, 5.2),
                    pt(9.9, 6.5),
                    pt(8.0, 8.2),
                    pt(8.0, 9.3),
                ],
                false,
            );
            p.circle_filled(pt(8.0, 11.5), 0.9 * k, color);
        }
        Icon::Sliders => {
            for (y, x) in [(4.0, 10.0), (8.0, 5.0), (12.0, 11.0)] {
                line(pt(2.0, y), pt(14.0, y));
                p.circle_filled(pt(x, y), 1.9 * k, color);
            }
        }
        Icon::Shield => {
            poly(
                &[
                    pt(8.0, 1.5),
                    pt(13.5, 3.5),
                    pt(13.0, 9.0),
                    pt(8.0, 14.5),
                    pt(3.0, 9.0),
                    pt(2.5, 3.5),
                ],
                true,
            );
            poly(&[pt(5.5, 8.0), pt(7.4, 9.9), pt(10.7, 6.3)], false);
        }
        Icon::Image => {
            p.rect_stroke(
                Rect::from_min_max(pt(1.5, 2.5), pt(14.5, 13.5)),
                1.5 * k,
                st,
                egui::StrokeKind::Inside,
            );
            poly(
                &[
                    pt(2.5, 12.0),
                    pt(6.0, 8.0),
                    pt(8.5, 10.5),
                    pt(10.5, 8.5),
                    pt(13.5, 12.0),
                ],
                false,
            );
            p.circle_filled(pt(10.75, 5.5), 1.2 * k, color);
        }
        Icon::Alert => {
            poly(&[pt(8.0, 2.0), pt(14.5, 13.5), pt(1.5, 13.5)], true);
            line(pt(8.0, 6.5), pt(8.0, 9.5));
            p.circle_filled(pt(8.0, 11.6), 0.85 * k, color);
        }
        Icon::Close => {
            line(pt(4.0, 4.0), pt(12.0, 12.0));
            line(pt(12.0, 4.0), pt(4.0, 12.0));
        }
        Icon::Plus => {
            line(pt(8.0, 3.0), pt(8.0, 13.0));
            line(pt(3.0, 8.0), pt(13.0, 8.0));
        }
    }
}
