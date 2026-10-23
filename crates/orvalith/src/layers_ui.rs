//! Painel de camadas: árvore com grupos, miniaturas, propriedades e operações.

use eframe::egui;
use orvalith_core::edit::{LayersEdit, SetVisible};
use orvalith_core::layerops::{self, Outcome};
use orvalith_core::{BlendMode, Layer, LayerId};

use crate::canvas::OpenDoc;

const THUMB: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerCmd {
    New,
    NewGroup,
    Duplicate,
    Delete,
    Group,
    Ungroup,
    Up,
    Down,
    MergeSelected,
    MergeVisible,
    Flatten,
    Copy,
    Paste,
}

impl OpenDoc {
    /// IDs sobre os quais as operações atuam: seleção, ou a camada ativa.
    pub fn selection_ids(&self) -> Vec<LayerId> {
        let valid: Vec<LayerId> = self
            .selected
            .iter()
            .copied()
            .filter(|id| self.doc.layer(*id).is_some())
            .collect();
        if valid.is_empty() {
            self.active_idx()
                .map(|i| vec![self.doc.layers[i].id])
                .unwrap_or_default()
        } else {
            valid
        }
    }

    /// Aplica uma alteração às camadas como uma única ação de histórico.
    pub fn edit_layers(&mut self, label: &str, f: impl FnOnce(&mut Vec<Layer>)) {
        let before = self.doc.layers.clone();
        let mut after = before.clone();
        f(&mut after);
        self.history.execute(
            &mut self.doc,
            Box::new(LayersEdit::new(label, before, after)),
        );
    }

    fn apply_outcome(&mut self, label: &str, o: Outcome) {
        let before = self.doc.layers.clone();
        self.history.execute(
            &mut self.doc,
            Box::new(LayersEdit::new(label, before, o.layers)),
        );
        self.selected = o.select;
        if let Some(a) = o.active {
            self.active_layer = a;
        }
    }
}

/// Executa uma operação de camada. Retorna uma mensagem quando nada pôde ser feito.
pub fn run(
    open: &mut OpenDoc,
    cmd: LayerCmd,
    clip: &mut Vec<Layer>,
    counter: &mut u32,
) -> Option<String> {
    let sel = open.selection_ids();
    let active = open.active_idx().map(|i| open.doc.layers[i].id);
    let nothing = || Some("Nada a fazer com a seleção atual.".to_string());
    match cmd {
        LayerCmd::New => {
            *counter += 1;
            let o = layerops::new_layer(&open.doc, active, format!("Camada {counter}"));
            open.apply_outcome("Nova camada", o);
        }
        LayerCmd::NewGroup => {
            let o = layerops::group(&open.doc, &[], active);
            open.apply_outcome("Novo grupo", o);
        }
        LayerCmd::Duplicate => match layerops::duplicate(&open.doc, &sel) {
            Some(o) => open.apply_outcome("Duplicar camada", o),
            None => return nothing(),
        },
        LayerCmd::Delete => match layerops::delete(&open.doc, &sel) {
            Some(o) => open.apply_outcome("Excluir camada", o),
            None => return nothing(),
        },
        LayerCmd::Group => {
            if sel.is_empty() {
                return nothing();
            }
            let o = layerops::group(&open.doc, &sel, None);
            open.apply_outcome("Agrupar camadas", o);
        }
        LayerCmd::Ungroup => {
            let Some(g) = sel
                .iter()
                .find(|id| open.doc.layer(**id).is_some_and(Layer::is_group))
            else {
                return Some("Selecione um grupo para desagrupar.".into());
            };
            match layerops::ungroup(&open.doc, *g) {
                Some(o) => open.apply_outcome("Desagrupar", o),
                None => return nothing(),
            }
        }
        LayerCmd::Up | LayerCmd::Down => {
            let Some(id) = active else { return nothing() };
            let up = cmd == LayerCmd::Up;
            match layerops::move_sibling(&open.doc, id, up) {
                Some(o) => open.apply_outcome(if up { "Subir camada" } else { "Descer camada" }, o),
                None => return Some("A camada já está no limite.".into()),
            }
        }
        LayerCmd::MergeSelected => match layerops::merge_selected(&open.doc, &sel) {
            Some(o) => open.apply_outcome("Mesclar camadas", o),
            None => return Some("Selecione ao menos duas camadas (Ctrl+clique).".into()),
        },
        LayerCmd::MergeVisible => match layerops::merge_visible(&open.doc) {
            Some(o) => open.apply_outcome("Mesclar visíveis", o),
            None => return Some("É preciso haver ao menos duas camadas visíveis.".into()),
        },
        LayerCmd::Flatten => match layerops::flatten_all(&open.doc) {
            Some(o) => open.apply_outcome("Achatar imagem", o),
            None => return Some("A imagem já está achatada.".into()),
        },
        LayerCmd::Copy => {
            *clip = layerops::copy_nodes(&open.doc, &sel);
            return Some(format!("{} item(ns) copiado(s).", clip.len()));
        }
        LayerCmd::Paste => match layerops::paste(&open.doc, clip, active) {
            Some(o) => open.apply_outcome("Colar camadas", o),
            None => return Some("Nada copiado ainda.".into()),
        },
    }
    open.isolated = open.isolated.filter(|id| open.doc.layer(*id).is_some());
    None
}

/// Amostra uma miniatura (vizinho mais próximo) de uma camada raster.
fn thumbnail(layer: &Layer) -> Option<egui::ColorImage> {
    if layer.is_group() || layer.width == 0 || layer.height == 0 {
        return None;
    }
    let (w, h) = (layer.width as usize, layer.height as usize);
    let scale = (w.max(h) as f32 / THUMB as f32).max(1.0);
    let (tw, th) = (
        ((w as f32 / scale).round() as usize).max(1),
        ((h as f32 / scale).round() as usize).max(1),
    );
    let mut rgba = Vec::with_capacity(tw * th * 4);
    for y in 0..th {
        for x in 0..tw {
            let sx = (((x as f32 + 0.5) * scale) as usize).min(w - 1);
            let sy = (((y as f32 + 0.5) * scale) as usize).min(h - 1);
            let i = (sy * w + sx) * 4;
            rgba.extend_from_slice(&layer.pixels[i..i + 4]);
        }
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([tw, th], &rgba))
}

/// Atualiza miniaturas quando o documento mudou (adiado durante um traço em andamento).
fn update_thumbs(ctx: &egui::Context, open: &mut OpenDoc, busy: bool) {
    if busy || open.thumbs_rev == open.doc.revision && !open.thumbs.is_empty() {
        return;
    }
    open.thumbs_rev = open.doc.revision;
    let live: Vec<LayerId> = open.doc.layers.iter().map(|l| l.id).collect();
    open.thumbs.retain(|id, _| live.contains(id));
    for l in &open.doc.layers {
        if let Some(img) = thumbnail(l) {
            let opts = egui::TextureOptions::NEAREST;
            match open.thumbs.get_mut(&l.id) {
                Some(tex) => tex.set(img, opts),
                None => {
                    let tex = ctx.load_texture(format!("thumb{}", l.id.0), img, opts);
                    open.thumbs.insert(l.id, tex);
                }
            }
        }
    }
}

fn begin_prop_drag(open: &mut OpenDoc, label: &str) {
    if open.prop_drag.is_none() {
        open.prop_drag = Some((label.to_string(), open.doc.layers.clone()));
    }
}

fn end_prop_drag(open: &mut OpenDoc) {
    if let Some((label, before)) = open.prop_drag.take() {
        let changed = before
            .iter()
            .zip(&open.doc.layers)
            .any(|(a, b)| (a.opacity - b.opacity).abs() > 1e-6 || (a.fill - b.fill).abs() > 1e-6);
        if changed {
            let after = open.doc.layers.clone();
            open.history.record(
                &mut open.doc,
                Box::new(LayersEdit::new(&label, before, after)),
            );
        }
    }
}

fn props_ui(ui: &mut egui::Ui, open: &mut OpenDoc) {
    let Some(idx) = open.active_idx() else { return };
    let l = &open.doc.layers[idx];
    let (is_group, mut blend, locks) = (
        l.is_group(),
        l.blend,
        (l.lock_pixels, l.lock_alpha, l.lock_position),
    );
    let mut new_locks = locks;
    ui.horizontal(|ui| {
        ui.label("Modo");
        egui::ComboBox::from_id_salt("blend_mode")
            .selected_text(blend.label())
            .width(150.0)
            .show_ui(ui, |ui| {
                if is_group {
                    ui.selectable_value(
                        &mut blend,
                        BlendMode::PassThrough,
                        BlendMode::PassThrough.label(),
                    );
                }
                for m in BlendMode::LAYER_MODES {
                    ui.selectable_value(&mut blend, m, m.label());
                }
            });
    });
    if blend != open.doc.layers[idx].blend {
        open.edit_layers("Alterar modo de mesclagem", |ls| ls[idx].blend = blend);
    }
    for (label, is_fill) in [("Opacidade", false), ("Preenchimento", true)] {
        let cur = if is_fill {
            open.doc.layers[idx].fill
        } else {
            open.doc.layers[idx].opacity
        };
        let mut pct = cur * 100.0;
        let r = ui.add(
            egui::Slider::new(&mut pct, 0.0..=100.0)
                .text(label)
                .suffix("%")
                .integer(),
        );
        if r.changed() {
            begin_prop_drag(
                open,
                if is_fill {
                    "Alterar preenchimento"
                } else {
                    "Alterar opacidade"
                },
            );
            let l = &mut open.doc.layers[idx];
            if is_fill {
                l.fill = pct / 100.0;
            } else {
                l.opacity = pct / 100.0;
            }
            open.doc.touch();
        }
        if r.drag_stopped() || (r.changed() && !r.dragged()) {
            end_prop_drag(open);
        }
    }
    ui.horizontal(|ui| {
        ui.label("Bloquear");
        ui.checkbox(&mut new_locks.0, "Pixels");
        ui.checkbox(&mut new_locks.1, "Transp.");
        ui.checkbox(&mut new_locks.2, "Posição");
    });
    if new_locks != locks {
        open.edit_layers("Alterar bloqueios", |ls| {
            ls[idx].lock_pixels = new_locks.0;
            ls[idx].lock_alpha = new_locks.1;
            ls[idx].lock_position = new_locks.2;
        });
    }
}

/// Linhas visíveis em ordem de exibição (topo primeiro) com a profundidade.
fn visible_rows(open: &OpenDoc) -> Vec<(LayerId, usize)> {
    fn walk(
        open: &OpenDoc,
        parent: Option<LayerId>,
        depth: usize,
        out: &mut Vec<(LayerId, usize)>,
    ) {
        let mut kids = open.doc.children_of(parent);
        kids.reverse();
        for i in kids {
            let l = &open.doc.layers[i];
            out.push((l.id, depth));
            if l.is_group() && l.expanded {
                walk(open, Some(l.id), depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(open, None, 0, &mut out);
    out
}

enum RowEvent {
    Expand(LayerId),
    Visible(LayerId, bool),
    Isolate(LayerId),
    Click(LayerId, egui::Modifiers),
    StartRename(LayerId),
    Rename(LayerId, String),
    CancelRename,
}

fn list_ui(ui: &mut egui::Ui, open: &mut OpenDoc) {
    let rows = visible_rows(open);
    let mut events: Vec<RowEvent> = Vec::new();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (id, depth) in &rows {
            let Some(l) = open.doc.layer(*id) else {
                continue;
            };
            let (name, visible, group, expanded) =
                (l.name.clone(), l.visible, l.is_group(), l.expanded);
            let locked = l.lock_pixels || l.lock_alpha || l.lock_position;
            let selected = open.selected.contains(id);
            ui.horizontal(|ui| {
                ui.add_space(*depth as f32 * 14.0);
                if group {
                    if ui.small_button(if expanded { "-" } else { "+" }).clicked() {
                        events.push(RowEvent::Expand(*id));
                    }
                } else {
                    ui.add_space(18.0);
                }
                let mut v = visible;
                if ui
                    .checkbox(&mut v, "")
                    .on_hover_text("Visível (Alt+clique: isolar)")
                    .changed()
                {
                    if ui.input(|i| i.modifiers.alt) {
                        events.push(RowEvent::Isolate(*id));
                    } else {
                        events.push(RowEvent::Visible(*id, v));
                    }
                }
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 2.0, egui::Color32::from_gray(90));
                if let Some(tex) = open.thumbs.get(id) {
                    let sz = tex.size_vec2();
                    let fit = egui::Rect::from_center_size(rect.center(), sz.min(rect.size()));
                    ui.painter().image(
                        tex.id(),
                        fit,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                if open.renaming.as_ref().is_some_and(|(rid, _)| rid == id) {
                    if let Some((_, text)) = open.renaming.as_mut() {
                        let r = ui.add(egui::TextEdit::singleline(text).desired_width(110.0));
                        r.request_focus();
                        if r.lost_focus() {
                            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                events.push(RowEvent::Rename(*id, text.clone()));
                            } else {
                                events.push(RowEvent::CancelRename);
                            }
                        }
                    }
                } else {
                    let label = if locked {
                        format!("{name}  (bloq.)")
                    } else {
                        name
                    };
                    let text = if group {
                        egui::RichText::new(label).strong()
                    } else {
                        egui::RichText::new(label)
                    };
                    let r = ui.selectable_label(selected, text);
                    if r.clicked() {
                        events.push(RowEvent::Click(*id, ui.input(|i| i.modifiers)));
                    }
                    if r.double_clicked() {
                        events.push(RowEvent::StartRename(*id));
                    }
                }
            });
        }
    });
    for e in events {
        match e {
            RowEvent::Expand(id) => {
                if let Some(l) = open.doc.layer_mut(id) {
                    l.expanded = !l.expanded;
                }
            }
            RowEvent::Visible(layer, new) => {
                if let Some(l) = open.doc.layer(layer) {
                    let old = l.visible;
                    open.history
                        .execute(&mut open.doc, Box::new(SetVisible { layer, old, new }));
                }
            }
            RowEvent::Isolate(id) => {
                open.isolated = if open.isolated == Some(id) {
                    None
                } else {
                    Some(id)
                };
                open.doc.bump_view();
            }
            RowEvent::Click(id, m) => {
                if m.shift {
                    let a = rows.iter().position(|(r, _)| *r == open.active_layer);
                    let b = rows.iter().position(|(r, _)| *r == id);
                    if let (Some(a), Some(b)) = (a, b) {
                        let (lo, hi) = (a.min(b), a.max(b));
                        open.selected = rows[lo..=hi].iter().map(|(r, _)| *r).collect();
                    }
                } else if m.command {
                    if let Some(p) = open.selected.iter().position(|s| *s == id) {
                        open.selected.remove(p);
                    } else {
                        open.selected.push(id);
                    }
                    open.active_layer = id;
                } else {
                    open.selected = vec![id];
                    open.active_layer = id;
                }
            }
            RowEvent::StartRename(id) => {
                if let Some(l) = open.doc.layer(id) {
                    open.renaming = Some((id, l.name.clone()));
                }
            }
            RowEvent::Rename(id, name) => {
                open.renaming = None;
                let name = name.trim().to_string();
                if !name.is_empty() {
                    if let Some(idx) = open.doc.index_of(id) {
                        if open.doc.layers[idx].name != name {
                            open.edit_layers("Renomear camada", |ls| ls[idx].name = name);
                        }
                    }
                }
            }
            RowEvent::CancelRename => open.renaming = None,
        }
    }
}

/// Desenha o painel e devolve as operações pedidas pelos botões.
pub fn show(ui: &mut egui::Ui, open: &mut OpenDoc, busy: bool) -> Vec<LayerCmd> {
    update_thumbs(ui.ctx(), open, busy);
    let mut cmds = Vec::new();
    props_ui(ui, open);
    ui.separator();
    let buttons: [(&str, LayerCmd, &str); 12] = [
        ("Nova", LayerCmd::New, "Nova camada (Ctrl+Shift+N)"),
        ("Grupo", LayerCmd::NewGroup, "Novo grupo vazio"),
        ("Duplicar", LayerCmd::Duplicate, "Duplicar (Ctrl+J)"),
        ("Excluir", LayerCmd::Delete, "Excluir (Delete)"),
        ("Agrupar", LayerCmd::Group, "Agrupar seleção (Ctrl+G)"),
        ("Desagrupar", LayerCmd::Ungroup, "Desagrupar (Ctrl+Shift+G)"),
        ("Subir", LayerCmd::Up, "Subir (Ctrl+])"),
        ("Descer", LayerCmd::Down, "Descer (Ctrl+[)"),
        (
            "Mesclar sel.",
            LayerCmd::MergeSelected,
            "Mesclar selecionadas (Ctrl+E)",
        ),
        (
            "Mesclar visíveis",
            LayerCmd::MergeVisible,
            "Mesclar camadas visíveis",
        ),
        (
            "Achatar",
            LayerCmd::Flatten,
            "Achatar imagem (descarta camadas ocultas)",
        ),
        ("Colar", LayerCmd::Paste, "Colar camadas copiadas (Ctrl+V)"),
    ];
    ui.horizontal_wrapped(|ui| {
        for (label, cmd, tip) in buttons {
            if ui.small_button(label).on_hover_text(tip).clicked() {
                cmds.push(cmd);
            }
        }
    });
    if let Some(iso) = open.isolated {
        let name = open
            .doc
            .layer(iso)
            .map(|l| l.name.clone())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.colored_label(crate::theme::COPPER, format!("Isolada: {name}"));
            if ui.small_button("Sair do isolamento").clicked() {
                open.isolated = None;
                open.doc.bump_view();
            }
        });
    }
    ui.separator();
    list_ui(ui, open);
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::Document;

    fn open_with_two() -> OpenDoc {
        let mut d = Document::new("t", 4, 4, Some([10, 10, 10, 255])).unwrap();
        d.layers.push(Layer::filled("A", 4, 4, [200, 0, 0, 255]));
        OpenDoc::new(d)
    }

    fn run1(o: &mut OpenDoc, c: LayerCmd) -> Option<String> {
        run(o, c, &mut Vec::new(), &mut 1)
    }

    #[test]
    fn group_duplicate_delete_each_one_undo_step() {
        let mut o = open_with_two();
        let ids: Vec<_> = o.doc.layers.iter().map(|l| l.id).collect();
        o.selected = ids.clone();
        assert!(run1(&mut o, LayerCmd::Group).is_none());
        assert_eq!(o.doc.layers.len(), 3);
        assert_eq!(o.history.labels(), vec!["Agrupar camadas"]);
        assert!(run1(&mut o, LayerCmd::Duplicate).is_none());
        assert_eq!(o.doc.layers.len(), 6);
        assert!(run1(&mut o, LayerCmd::Delete).is_none());
        assert_eq!(o.doc.layers.len(), 3);
        o.history.undo(&mut o.doc);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers.len(), 3);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers.len(), 2);
    }

    #[test]
    fn copy_and_paste_between_documents() {
        let mut a = open_with_two();
        let mut b = OpenDoc::new(Document::new("b", 8, 8, Some([0, 0, 0, 255])).unwrap());
        let mut clip = Vec::new();
        a.selected = vec![a.doc.layers[1].id];
        assert!(run(&mut a, LayerCmd::Copy, &mut clip, &mut 1).is_some());
        assert!(run(&mut b, LayerCmd::Paste, &mut clip, &mut 1).is_none());
        assert_eq!(b.doc.layers.len(), 2);
        assert_eq!(b.doc.layers[1].width, 8);
        assert_eq!(b.doc.layers[1].pixel(4, 4).unwrap(), [200, 0, 0, 255]);
        assert!(b.doc.modified);
    }

    #[test]
    fn messages_when_nothing_to_do() {
        let mut o = OpenDoc::new(Document::new("t", 4, 4, Some([1, 1, 1, 255])).unwrap());
        assert!(run1(&mut o, LayerCmd::MergeSelected).is_some());
        assert!(run1(&mut o, LayerCmd::MergeVisible).is_some());
        assert!(run1(&mut o, LayerCmd::Flatten).is_some());
        assert!(run1(&mut o, LayerCmd::Ungroup).is_some());
        assert!(run1(&mut o, LayerCmd::Up).is_some());
        assert!(o.history.labels().is_empty());
    }

    #[test]
    fn prop_drag_records_single_action() {
        let mut o = open_with_two();
        begin_prop_drag(&mut o, "Alterar opacidade");
        for v in [0.9, 0.7, 0.4] {
            o.doc.layers[1].opacity = v;
            o.doc.touch();
        }
        end_prop_drag(&mut o);
        assert_eq!(o.history.labels(), vec!["Alterar opacidade"]);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[1].opacity, 1.0);
    }

    #[test]
    fn rename_and_visibility_are_undoable() {
        let mut o = open_with_two();
        o.edit_layers("Renomear camada", |ls| ls[1].name = "Novo nome".into());
        assert_eq!(o.doc.layers[1].name, "Novo nome");
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[1].name, "A");
    }

    #[test]
    fn thumbnail_samples_with_aspect_and_skips_groups() {
        let l = Layer::filled("x", 200, 100, [255, 0, 0, 255]);
        let t = thumbnail(&l).unwrap();
        assert_eq!(t.size, [32, 16]);
        assert!(thumbnail(&Layer::group("g")).is_none());
    }
}
