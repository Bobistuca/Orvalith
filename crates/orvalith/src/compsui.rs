//! Janela de composições de camadas: criar, aplicar, atualizar e excluir.

use eframe::egui;
use orvalith_core::comps::{self, CompsEdit, MAX_COMPS};

use crate::canvas::OpenDoc;

#[derive(Default)]
pub struct CompsUi {
    pub open: bool,
    name: String,
}

/// Cria uma composição com o estado visual atual (uma ação de desfazer).
pub fn add_comp(open: &mut OpenDoc, name: &str) -> Result<(), String> {
    if open.doc.comps.len() >= MAX_COMPS {
        return Err("Limite de composições atingido.".into());
    }
    let name = if name.trim().is_empty() {
        format!("Composição {}", open.doc.comps.len() + 1)
    } else {
        name.to_string()
    };
    let before = open.doc.comps.clone();
    let mut after = before.clone();
    after.push(comps::capture(&open.doc, &name));
    open.history.execute(
        &mut open.doc,
        Box::new(CompsEdit::new("Nova composição", before, after)),
    );
    Ok(())
}

/// Aplica a composição `i` (uma ação de desfazer sobre as camadas).
pub fn apply_comp(open: &mut OpenDoc, i: usize) {
    let Some(c) = open.doc.comps.get(i).cloned() else {
        return;
    };
    let layers = comps::apply(&open.doc, &c);
    open.edit_layers(&format!("Aplicar composição \"{}\"", c.name), |ls| {
        *ls = layers
    });
}

/// Regrava a composição `i` com o estado atual.
pub fn update_comp(open: &mut OpenDoc, i: usize) {
    let Some(old) = open.doc.comps.get(i).cloned() else {
        return;
    };
    let before = open.doc.comps.clone();
    let mut after = before.clone();
    after[i] = comps::capture(&open.doc, &old.name);
    open.history.execute(
        &mut open.doc,
        Box::new(CompsEdit::new("Atualizar composição", before, after)),
    );
}

pub fn delete_comp(open: &mut OpenDoc, i: usize) {
    if i >= open.doc.comps.len() {
        return;
    }
    let before = open.doc.comps.clone();
    let mut after = before.clone();
    after.remove(i);
    open.history.execute(
        &mut open.doc,
        Box::new(CompsEdit::new("Excluir composição", before, after)),
    );
}

pub fn comps_window(ctx: &egui::Context, st: &mut CompsUi, open: &mut OpenDoc) -> Option<String> {
    if !st.open {
        return None;
    }
    let mut is_open = true;
    let mut notice = None;
    let mut act: Option<(char, usize)> = None;
    let mut create = false;
    egui::Window::new("Composições de camadas")
        .open(&mut is_open)
        .resizable(false)
        .show(ctx, |ui| {
            if open.doc.comps.is_empty() {
                ui.label("Nenhuma composição. Ajuste a visibilidade, a opacidade e os estilos e crie uma.");
            }
            for (i, c) in open.doc.comps.iter().enumerate() {
                ui.horizontal(|ui| {
                    let on = comps::matches(&open.doc, c);
                    let label = if on { format!("● {}", c.name) } else { c.name.clone() };
                    if ui.button(label).on_hover_text("Aplicar esta composição").clicked() {
                        act = Some(('a', i));
                    }
                    if ui.small_button("Atualizar").on_hover_text("Regrava com o estado atual").clicked() {
                        act = Some(('u', i));
                    }
                    if ui.small_button("✕").on_hover_text("Excluir").clicked() {
                        act = Some(('d', i));
                    }
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut st.name).hint_text("Nome").desired_width(140.0));
                create = ui.button("Nova composição").clicked();
            });
            ui.small("Guarda visibilidade, opacidade, preenchimento, modo e estilos ativos; não guarda posição nem pixels.");
        });
    if create {
        if let Err(e) = add_comp(open, &st.name) {
            notice = Some(e);
        } else {
            st.name.clear();
        }
    }
    match act {
        Some(('a', i)) => apply_comp(open, i),
        Some(('u', i)) => update_comp(open, i),
        Some(('d', i)) => delete_comp(open, i),
        _ => {}
    }
    st.open = is_open;
    notice
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::Document;

    fn open() -> OpenDoc {
        let mut d = Document::new("t", 4, 4, Some([255, 255, 255, 255])).unwrap();
        d.layers
            .push(orvalith_core::Layer::filled("L2", 4, 4, [0, 0, 0, 255]));
        OpenDoc::new(d)
    }

    #[test]
    fn create_apply_update_delete_are_each_undoable() {
        let mut o = open();
        add_comp(&mut o, "Claro").unwrap();
        o.doc.layers[1].visible = false;
        add_comp(&mut o, "").unwrap();
        assert_eq!(o.doc.comps.len(), 2);
        assert_eq!(o.doc.comps[1].name, "Composição 2");
        apply_comp(&mut o, 0);
        assert!(o.doc.layers[1].visible);
        let n = o.history.labels().len();
        o.history.undo(&mut o.doc);
        assert!(!o.doc.layers[1].visible, "aplicar é uma ação");
        assert_eq!(o.history.labels().len(), n - 1);
        // Atualizar com o estado atual e depois excluir.
        update_comp(&mut o, 0);
        assert!(comps::matches(&o.doc, &o.doc.comps[0].clone()));
        delete_comp(&mut o, 0);
        assert_eq!(o.doc.comps.len(), 1);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.comps.len(), 2);
        delete_comp(&mut o, 9);
    }
}
