//! Ferramenta Liquify: pincel que deforma, suaviza ou reconstrói a camada ativa. A sessão
//! guarda o original e o campo de deslocamento; Enter aplica (1 ação), Esc cancela.

use std::sync::Arc;

use eframe::egui;
use orvalith_core::geom::Rect as PixRect;
use orvalith_core::liquify::Field;
use orvalith_core::retouch::Brush;
use orvalith_core::{DocumentId, Layer, LayerId};

use crate::canvas::{CanvasOut, OpenDoc};
use crate::tools::paint_blocked;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Warp,
    Smooth,
    Reconstruct,
}

struct Session {
    doc: DocumentId,
    layer: LayerId,
    original: Arc<Vec<u8>>,
    field: Field,
    before: Vec<Layer>,
    was_modified: bool,
    /// Revisão logo após o último toque; mudança externa encerra a sessão.
    rev: u64,
    last: Option<[f32; 2]>,
}

pub struct LiquifyState {
    pub mode: Mode,
    pub size: f32,
    pub hardness: f32,
    pub strength: f32,
    session: Option<Session>,
}

impl Default for LiquifyState {
    fn default() -> Self {
        Self {
            mode: Mode::Warp,
            size: 80.0,
            hardness: 0.3,
            strength: 0.8,
            session: None,
        }
    }
}

impl LiquifyState {
    pub fn busy(&self) -> bool {
        self.session.is_some()
    }

    pub fn brush_radius(&self) -> f32 {
        self.size / 2.0
    }

    fn brush(&self) -> Brush {
        Brush {
            radius: self.size / 2.0,
            hardness: self.hardness,
        }
    }

    /// Esc: restaura a camada original sem registrar histórico.
    pub fn cancel(&mut self, open: &mut OpenDoc) {
        let Some(s) = self.session.take() else { return };
        if s.doc != open.doc.id || open.doc.revision != s.rev {
            return;
        }
        if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == s.layer) {
            l.pixels = s.original.clone();
        }
        let full = PixRect::full(open.doc.width, open.doc.height);
        open.mark_dirty(full);
        open.doc.modified = s.was_modified;
    }

    /// Enter / troca de ferramenta: aplica o resultado como uma única ação.
    pub fn commit(&mut self, open: &mut OpenDoc) {
        let Some(s) = self.session.take() else { return };
        if s.doc != open.doc.id || open.doc.revision != s.rev || s.field.is_identity() {
            if s.doc == open.doc.id && open.doc.revision == s.rev && s.field.is_identity() {
                open.doc.modified = s.was_modified;
            }
            return;
        }
        let full = PixRect::full(open.doc.width, open.doc.height);
        open.record_layers("Liquify", s.before, full);
    }

    /// Volta tudo ao original mantendo a sessão aberta.
    fn reset_all(&mut self, open: &mut OpenDoc) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        s.field.clear();
        if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == s.layer) {
            l.pixels = s.original.clone();
        }
        let full = PixRect::full(open.doc.width, open.doc.height);
        open.mark_dirty(full);
        s.rev = open.doc.revision;
    }

    fn begin(&mut self, open: &OpenDoc) -> Result<(), String> {
        if open.edit_mask || open.qm.is_some() {
            return Err("O Liquify age nos pixels da camada, não na máscara.".into());
        }
        let idx = open.active_idx().ok_or("Selecione uma camada.")?;
        let l = &open.doc.layers[idx];
        if let Some(m) = paint_blocked(l) {
            return Err(m);
        }
        self.session = Some(Session {
            doc: open.doc.id,
            layer: l.id,
            original: l.pixels.clone(),
            field: Field::new(l.width as usize, l.height as usize),
            before: open.doc.layers.clone(),
            was_modified: open.doc.modified,
            rev: open.doc.revision,
            last: None,
        });
        Ok(())
    }

    fn dab(&mut self, open: &mut OpenDoc, p: [f32; 2]) {
        let brush = self.brush();
        let (mode, strength) = (self.mode, self.strength);
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let delta = s.last.map(|l| [p[0] - l[0], p[1] - l[1]]);
        s.last = Some(p);
        match mode {
            Mode::Warp => {
                let Some(d) = delta else { return };
                // O toque fica no ponto médio do passo, com o empurrão proporcional ao movimento.
                let mid = [p[0] - d[0] / 2.0, p[1] - d[1] / 2.0];
                s.field.push(mid, d, brush, strength);
            }
            Mode::Smooth => s.field.smooth(p, brush, strength),
            Mode::Reconstruct => s.field.reconstruct(p, brush, strength),
        }
        let c = delta.map_or(p, |d| [p[0] - d[0] / 2.0, p[1] - d[1] / 2.0]);
        let region = s.field.touched(
            c,
            brush.radius + delta.map_or(0.0, |d| d[0].abs().max(d[1].abs())),
        );
        let sel = open.doc.selection.mask.clone();
        if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == s.layer) {
            let px = l.pixels_mut();
            s.field
                .render_region(&s.original, px, region, sel.as_ref().map(|m| m.as_slice()));
        }
        let r = PixRect::new(
            region.0 as u32,
            region.1 as u32,
            (region.2 - region.0) as u32,
            (region.3 - region.1) as u32,
        );
        open.mark_dirty(r);
        s.rev = open.doc.revision;
    }

    pub fn handle(&mut self, open: &mut OpenDoc, out: &CanvasOut) -> Option<String> {
        if let Some(s) = &self.session {
            if s.doc != open.doc.id || s.rev != open.doc.revision {
                self.session = None; // mudança externa (desfazer etc.): a sessão não vale mais
            }
        }
        let mut notice = None;
        if let Some(p) = out.pressed {
            if self.session.is_none() {
                if let Err(e) = self.begin(open) {
                    return Some(e);
                }
            }
            if let Some(s) = self.session.as_mut() {
                s.last = Some(p);
            }
            if self.mode != Mode::Warp {
                self.dab(open, p);
            }
        }
        if out.down {
            for m in out.moves.clone() {
                if out.pressed == Some(m) {
                    continue;
                }
                self.dab(open, m);
            }
        }
        if out.released || (!out.down && out.pressed.is_none()) {
            if let Some(s) = self.session.as_mut() {
                s.last = None;
            }
        }
        if self.session.is_some() && notice.is_none() {
            notice = Some("Liquify: Enter aplica, Esc cancela.".to_string());
        }
        notice
    }

    pub fn options_ui(&mut self, ui: &mut egui::Ui, open: Option<&mut OpenDoc>) -> bool {
        let mut reset = false;
        ui.radio_value(&mut self.mode, Mode::Warp, "Deformar");
        ui.radio_value(&mut self.mode, Mode::Smooth, "Suavizar");
        ui.radio_value(&mut self.mode, Mode::Reconstruct, "Reconstruir");
        ui.separator();
        ui.label("Tamanho");
        ui.add(
            egui::Slider::new(&mut self.size, 4.0..=600.0)
                .logarithmic(true)
                .suffix(" px"),
        );
        for (label, v, min) in [
            ("Dureza", &mut self.hardness, 0.0),
            ("Intensidade", &mut self.strength, 1.0),
        ] {
            let mut p = *v * 100.0;
            ui.label(label);
            if ui
                .add(egui::Slider::new(&mut p, min..=100.0).suffix("%").integer())
                .changed()
            {
                *v = p / 100.0;
            }
        }
        if self.session.is_some() {
            ui.separator();
            if ui.button("Reconstruir tudo").clicked() {
                reset = true;
            }
            ui.label("Enter aplica · Esc cancela");
        }
        if reset {
            if let Some(o) = open {
                self.reset_all(o);
            }
        }
        reset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::Document;

    fn open() -> OpenDoc {
        let mut d = Document::new("t", 60, 30, Some([0, 0, 0, 255])).unwrap();
        for y in 0..30 {
            d.layers[0].set_pixel(30, y, [255, 255, 255, 255]);
        }
        OpenDoc::new(d)
    }

    fn press(p: [f32; 2]) -> CanvasOut {
        CanvasOut {
            pressed: Some(p),
            down: true,
            moves: vec![p],
            ..CanvasOut::default()
        }
    }

    fn drag(p: [f32; 2]) -> CanvasOut {
        CanvasOut {
            down: true,
            moves: vec![p],
            ..CanvasOut::default()
        }
    }

    fn release() -> CanvasOut {
        CanvasOut {
            released: true,
            ..CanvasOut::default()
        }
    }

    fn white_col(o: &OpenDoc) -> usize {
        (0..60)
            .max_by_key(|x| o.doc.layers[0].pixel(*x as u32, 15).unwrap()[0])
            .unwrap()
    }

    fn warp(o: &mut OpenDoc, st: &mut LiquifyState) {
        st.handle(o, &press([28.0, 15.0]));
        for x in [30.0, 32.0, 34.0, 36.0] {
            st.handle(o, &drag([x, 15.0]));
        }
        st.handle(o, &release());
    }

    #[test]
    fn warp_drag_moves_the_line_and_commit_is_one_undoable_action() {
        let mut o = open();
        let orig = o.doc.layers[0].pixels.clone();
        let mut st = LiquifyState {
            size: 20.0,
            hardness: 1.0,
            strength: 1.0,
            ..LiquifyState::default()
        };
        warp(&mut o, &mut st);
        assert!(st.busy());
        assert!(white_col(&o) > 30, "linha empurrada para a direita");
        assert_eq!(o.history.labels().len(), 0, "ainda sem histórico");
        st.commit(&mut o);
        assert!(!st.busy());
        assert_eq!(o.history.labels().len(), 1);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[0].pixels, orig);
    }

    #[test]
    fn escape_restores_and_reconstruct_returns_to_the_original() {
        let mut o = open();
        let orig = o.doc.layers[0].pixels.clone();
        let was = o.doc.modified;
        let mut st = LiquifyState {
            size: 20.0,
            hardness: 1.0,
            strength: 1.0,
            ..LiquifyState::default()
        };
        warp(&mut o, &mut st);
        st.cancel(&mut o);
        assert_eq!(o.doc.layers[0].pixels, orig);
        assert_eq!(o.doc.modified, was);
        assert!(!st.busy());
        // Reconstruir por cima do deslocamento volta ao original.
        warp(&mut o, &mut st);
        st.mode = Mode::Reconstruct;
        st.size = 60.0;
        st.handle(&mut o, &press([32.0, 15.0]));
        st.handle(&mut o, &release());
        assert_eq!(o.doc.layers[0].pixels, orig);
        // Sessão sem alterações não gera histórico.
        st.commit(&mut o);
        assert_eq!(o.history.labels().len(), 0);
    }

    #[test]
    fn locked_layers_and_external_changes_end_the_session() {
        let mut o = open();
        o.doc.layers[0].lock_pixels = true;
        let mut st = LiquifyState::default();
        assert!(st.handle(&mut o, &press([10.0, 10.0])).is_some());
        assert!(!st.busy());
        o.doc.layers[0].lock_pixels = false;
        st.handle(&mut o, &press([28.0, 15.0]));
        assert!(st.busy());
        o.doc.touch(); // alteração externa
        st.handle(&mut o, &drag([30.0, 15.0]));
        assert!(!st.busy());
    }
}
