//! Comandos de seleção, recortar/copiar/apagar/preencher e máscara rápida.

use orvalith_core::selection as sel;
use orvalith_core::selection::{clear_selected, extract_selected, fill_selected};

use super::OrvalithApp;
use crate::layers_ui::{self, LayerCmd};
use crate::select::{self, ModifyDialog, ModifyKind};
use crate::tools::{paint_blocked, Tool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelCmd {
    All,
    Deselect,
    Invert,
    Modify,
    Transform,
    Range,
    Channels,
    QuickMask,
    Refine,
}

impl OrvalithApp {
    fn notice(&mut self, msg: &str) {
        self.tools.notice = Some(msg.to_string());
    }

    pub(super) fn run_select(&mut self, cmd: SelCmd) {
        if cmd == SelCmd::QuickMask {
            return self.toggle_quick_mask();
        }
        let Some(open) = self.docs.get_mut(self.active) else {
            return;
        };
        if open.qm.is_some() {
            self.notice("Saia da máscara rápida (Q) para usar os comandos de seleção.");
            return;
        }
        let (w, h) = (open.doc.width, open.doc.height);
        match cmd {
            SelCmd::All => {
                select::set_selection(open, "Selecionar tudo", Some(sel::select_all(w, h)));
            }
            SelCmd::Deselect => {
                select::set_selection(open, "Desmarcar", None);
            }
            SelCmd::Invert => match open.doc.selection.mask.clone() {
                Some(m) => {
                    select::set_selection(open, "Inverter seleção", sel::invert(&m));
                }
                None => self.notice("Não há seleção para inverter."),
            },
            SelCmd::Modify => {
                if open.doc.selection.mask.is_some() {
                    self.tools.sel.modify = Some(ModifyDialog {
                        kind: ModifyKind::Expand,
                        radius: 5.0,
                    });
                } else {
                    self.notice("Faça uma seleção primeiro.");
                }
            }
            SelCmd::Transform => {
                if open.doc.selection.mask.is_some() {
                    self.tools.sel.open_transform(open);
                } else {
                    self.notice("Faça uma seleção primeiro.");
                }
            }
            SelCmd::Range => {
                let fg = self.tools.fg;
                self.tools.sel.open_range(open, fg);
            }
            SelCmd::Channels => self.tools.sel.channels_open = true,
            SelCmd::Refine => {
                if open.doc.selection.mask.is_some() {
                    self.tools.sel.refine = Some((
                        crate::select::RefineTarget::Selection,
                        orvalith_core::maskops::RefineParams::default(),
                    ));
                } else {
                    self.notice("Faça uma seleção primeiro.");
                }
            }
            SelCmd::QuickMask => {}
        }
    }

    /// Ctrl+C / Ctrl+X: com seleção copia os pixels selecionados da camada ativa como uma
    /// nova camada na área interna; sem seleção copia as camadas selecionadas.
    pub(super) fn copy_cut(&mut self, cut: bool) {
        let Some(open) = self.docs.get_mut(self.active) else {
            return;
        };
        let Some(mask) = open.doc.selection.mask.clone() else {
            if cut {
                self.notice("Cortar exige uma seleção.");
            } else {
                self.tools.notice = layers_ui::run(
                    open,
                    LayerCmd::Copy,
                    &mut self.layer_clip,
                    &mut self.layer_counter,
                );
            }
            return;
        };
        let Some(idx) = open.active_idx() else { return };
        if open.doc.layers[idx].is_group() {
            self.notice("Selecione uma camada raster (não um grupo).");
            return;
        }
        let copy = extract_selected(&open.doc.layers[idx], Some(&mask), "Seleção copiada");
        self.layer_clip = vec![copy];
        if cut {
            self.clear_selection_pixels();
        } else {
            self.notice("Seleção copiada. Cole com Ctrl+V.");
        }
    }

    fn clear_selection_pixels(&mut self) {
        let Some(open) = self.docs.get_mut(self.active) else {
            return;
        };
        let Some(idx) = open.active_idx() else { return };
        if let Some(msg) = paint_blocked(&open.doc.layers[idx]) {
            self.tools.notice = Some(msg);
            return;
        }
        if open.doc.layers[idx].lock_alpha {
            self.notice("A transparência da camada está bloqueada: não é possível apagar.");
            return;
        }
        let mask = open.doc.selection.mask.clone();
        match clear_selected(
            &mut open.doc.layers[idx],
            mask.as_ref().map(|m| m.as_slice()),
        ) {
            Some(patch) => {
                open.mark_dirty(patch.rect());
                open.record_patch(patch);
            }
            None => self.notice("Nada a apagar na seleção."),
        }
    }

    /// Tecla Delete: apaga o conteúdo da seleção; sem seleção, exclui a camada.
    pub(super) fn delete_key(&mut self) {
        if self.tools.tool == Tool::Node {
            if let Some(open) = self.docs.get_mut(self.active) {
                if self.tools.vec.delete_selected_node(open) {
                    return;
                }
            }
        }
        let has_sel = self
            .docs
            .get(self.active)
            .is_some_and(|o| o.doc.selection.mask.is_some() && o.qm.is_none());
        if has_sel {
            self.clear_selection_pixels();
        } else if let Some(open) = self.docs.get_mut(self.active) {
            self.tools.notice = layers_ui::run(
                open,
                LayerCmd::Delete,
                &mut self.layer_clip,
                &mut self.layer_counter,
            );
        }
    }

    /// Preenche a seleção (ou a camada inteira, sem seleção) com a cor de frente/fundo.
    pub(super) fn fill_selection(&mut self, background: bool) {
        let color = if background {
            self.tools.bg
        } else {
            self.tools.fg
        };
        let Some(open) = self.docs.get_mut(self.active) else {
            return;
        };
        let Some(idx) = open.active_idx() else { return };
        if let Some(msg) = paint_blocked(&open.doc.layers[idx]) {
            self.tools.notice = Some(msg);
            return;
        }
        let mask = open.doc.selection.mask.clone();
        let lock = open.doc.layers[idx].lock_alpha;
        match fill_selected(
            &mut open.doc.layers[idx],
            mask.as_ref().map(|m| m.as_slice()),
            color,
            1.0,
            lock,
        ) {
            Some(patch) => {
                open.mark_dirty(patch.rect());
                open.record_patch(patch);
            }
            None => self.notice("Nada a preencher."),
        }
    }

    pub(super) fn toggle_quick_mask(&mut self) {
        let Some(open) = self.docs.get_mut(self.active) else {
            return;
        };
        if self.tools.is_busy() {
            return;
        }
        if open.qm.is_some() {
            if let Some(id) = open.qm_target.take() {
                if let Some(raw) = open.leave_quick_mask_raw() {
                    select::channel_store(open, id, raw);
                }
            } else if let Some(result) = open.leave_quick_mask() {
                select::set_selection(open, "Máscara rápida", result);
            }
            self.notice("Saiu da máscara rápida.");
        } else {
            open.enter_quick_mask();
            if select::is_selection_tool(self.tools.tool) || self.tools.tool == Tool::Move {
                self.tools.set_tool(Tool::Brush);
            }
            self.notice(
                "Máscara rápida: pinte de preto para ocultar e de branco para selecionar (Q sai).",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::canvas::{CanvasOut, OpenDoc};
    use crate::prefs::Prefs;
    use crate::tools::gray;
    use orvalith_core::Document;

    fn app_with_selection() -> OrvalithApp {
        let mut app = OrvalithApp::new(Prefs::default());
        let mut o = OpenDoc::new(Document::new("t", 20, 20, Some([200, 200, 200, 255])).unwrap());
        select::set_selection(
            &mut o,
            "Seleção retangular",
            Some(sel::rect_mask(20, 20, (5.0, 5.0), (10.0, 10.0), true)),
        );
        app.docs.push(o);
        app.tools.fg = [255, 0, 0, 255];
        app.tools.bg = [0, 0, 255, 255];
        app
    }

    fn px(app: &OrvalithApp, x: u32, y: u32) -> [u8; 4] {
        app.docs[0].doc.layers[0].pixel(x, y).unwrap()
    }

    #[test]
    fn fill_only_inside_selection_and_undo_redo_restore_edit_and_selection() {
        let mut app = app_with_selection();
        app.fill_selection(false);
        assert_eq!(px(&app, 7, 7), [255, 0, 0, 255]);
        assert_eq!(px(&app, 4, 7), [200, 200, 200, 255]);
        assert_eq!(px(&app, 10, 7), [200, 200, 200, 255]);
        app.fill_selection(true);
        assert_eq!(px(&app, 7, 7), [0, 0, 255, 255]);
        let o = &mut app.docs[0];
        o.history.undo(&mut o.doc);
        assert_eq!(px(&app, 7, 7), [255, 0, 0, 255]);
        let o = &mut app.docs[0];
        o.history.undo(&mut o.doc);
        assert_eq!(px(&app, 7, 7), [200, 200, 200, 255]);
        let o = &mut app.docs[0];
        o.history.undo(&mut o.doc);
        assert!(app.docs[0].doc.selection.mask.is_none());
        app.fill_selection(false);
        assert_eq!(px(&app, 0, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn redo_restores_selection_then_edit() {
        let mut app = app_with_selection();
        app.fill_selection(false);
        let o = &mut app.docs[0];
        o.history.undo(&mut o.doc);
        o.history.undo(&mut o.doc);
        assert!(o.doc.selection.mask.is_none());
        o.history.redo(&mut o.doc);
        assert!(o.doc.selection.mask.is_some());
        o.history.redo(&mut o.doc);
        assert_eq!(px(&app, 7, 7), [255, 0, 0, 255]);
        assert_eq!(px(&app, 2, 2), [200, 200, 200, 255]);
    }

    #[test]
    fn cut_copies_selected_pixels_clears_them_and_paste_restores_in_other_doc() {
        let mut app = app_with_selection();
        app.copy_cut(true);
        assert_eq!(px(&app, 7, 7)[3], 0);
        assert_eq!(px(&app, 4, 4), [200, 200, 200, 255]);
        assert_eq!(app.layer_clip.len(), 1);
        let mut other = OpenDoc::new(Document::new("o", 20, 20, Some([0, 0, 0, 255])).unwrap());
        let r = layers_ui::run(&mut other, LayerCmd::Paste, &mut app.layer_clip, &mut 1);
        assert!(r.is_none());
        assert_eq!(
            other.doc.layers[1].pixel(7, 7).unwrap(),
            [200, 200, 200, 255]
        );
        assert_eq!(other.doc.layers[1].pixel(2, 2).unwrap()[3], 0);
        let mut app2 = app_with_selection();
        app2.copy_cut(false);
        assert_eq!(px(&app2, 7, 7), [200, 200, 200, 255]);
    }

    #[test]
    fn delete_key_clears_selection_or_deletes_layer() {
        let mut app = app_with_selection();
        app.delete_key();
        assert_eq!(px(&app, 6, 6)[3], 0);
        assert_eq!(app.docs[0].doc.layers.len(), 1);
        let o = &mut app.docs[0];
        select::set_selection(o, "Desmarcar", None);
        app.delete_key();
        assert!(app.docs[0].doc.layers.is_empty());
    }

    #[test]
    fn locked_layers_refuse_selection_edits() {
        let mut app = app_with_selection();
        app.docs[0].doc.layers[0].lock_pixels = true;
        app.fill_selection(false);
        assert_eq!(px(&app, 7, 7), [200, 200, 200, 255]);
        assert!(app.tools.notice.is_some());
        app.docs[0].doc.layers[0].lock_pixels = false;
        app.docs[0].doc.layers[0].lock_alpha = true;
        app.delete_key();
        assert_eq!(px(&app, 7, 7)[3], 255);
    }

    #[test]
    fn quick_mask_paint_black_removes_from_selection_and_round_trips() {
        let mut app = app_with_selection();
        app.toggle_quick_mask();
        assert!(app.docs[0].qm.is_some());
        app.tools.tool = Tool::Pencil;
        app.tools.brush.size = 1.0;
        let (fg, bg) = (app.tools.fg, app.tools.bg);
        app.tools.fg = gray([0, 0, 0, 255]);
        let out = CanvasOut {
            pressed: Some([6.5, 6.5]),
            released: true,
            pressure: 1.0,
            ..Default::default()
        };
        let qm = app.docs[0].qm.as_deref_mut().unwrap();
        app.tools.handle(qm, &out);
        app.tools.fg = fg;
        app.tools.bg = bg;
        app.toggle_quick_mask();
        assert!(app.docs[0].qm.is_none());
        let m = app.docs[0].doc.selection.mask.clone().unwrap();
        assert_eq!(m[6 * 20 + 6], 0, "pixel pintado de preto saiu da seleção");
        assert_eq!(m[7 * 20 + 7], 255);
        assert_eq!(m[2 * 20 + 2], 0);
        let o = &mut app.docs[0];
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.selection.mask.as_ref().unwrap()[6 * 20 + 6], 255);
    }

    #[test]
    fn quick_mask_without_edits_keeps_selection_and_without_selection_gives_none() {
        let mut app = app_with_selection();
        let before = app.docs[0].doc.selection.mask.clone().unwrap();
        app.toggle_quick_mask();
        app.toggle_quick_mask();
        assert_eq!(*app.docs[0].doc.selection.mask.clone().unwrap(), *before);
        let o = &mut app.docs[0];
        select::set_selection(o, "Desmarcar", None);
        app.toggle_quick_mask();
        app.toggle_quick_mask();
        assert!(app.docs[0].doc.selection.mask.is_none());
    }

    #[test]
    fn gray_uses_luminance_and_keeps_alpha() {
        assert_eq!(gray([255, 255, 255, 200]), [255, 255, 255, 200]);
        assert_eq!(gray([0, 0, 0, 255]), [0, 0, 0, 255]);
        let g = gray([255, 0, 0, 255]);
        assert_eq!(g, [77, 77, 77, 255]);
    }
    #[test]
    fn channel_edited_in_quick_mask_is_stored_with_undo() {
        let mut app = app_with_selection();
        // Salva a seleção como canal e edita esse canal na máscara rápida.
        let mask = app.docs[0].doc.selection.mask.clone().unwrap();
        let before = orvalith_core::edit::SelectionState::of(&app.docs[0].doc);
        let mut after = before.clone();
        after.channels.push(orvalith_core::Channel {
            id: 1,
            name: "Céu".into(),
            mask,
        });
        app.docs[0].execute_nonpixel(Box::new(orvalith_core::edit::SelectionEdit::new(
            "Salvar seleção",
            before,
            after,
        )));
        app.docs[0].enter_quick_mask_for_channel(1);
        app.tools.tool = Tool::Pencil;
        app.tools.brush.size = 1.0;
        let (fg, bg) = (app.tools.fg, app.tools.bg);
        app.tools.fg = gray([255, 255, 255, 255]);
        let out = CanvasOut {
            pressed: Some([15.5, 15.5]),
            released: true,
            pressure: 1.0,
            ..Default::default()
        };
        let qm = app.docs[0].qm.as_deref_mut().unwrap();
        app.tools.handle(qm, &out);
        app.tools.fg = fg;
        app.tools.bg = bg;
        app.toggle_quick_mask();
        assert!(app.docs[0].qm.is_none() && app.docs[0].qm_target.is_none());
        assert_eq!(
            app.docs[0].doc.channels[0].mask[15 * 20 + 15],
            255,
            "canal ganhou o pixel branco"
        );
        assert_eq!(app.docs[0].doc.channels[0].mask[2 * 20 + 2], 0);
        // A seleção atual não foi tocada.
        assert_eq!(
            app.docs[0].doc.selection.mask.as_ref().unwrap()[15 * 20 + 15],
            0
        );
        let o = &mut app.docs[0];
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.channels[0].mask[15 * 20 + 15], 0);
    }
}
