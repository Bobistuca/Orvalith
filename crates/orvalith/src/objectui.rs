//! Painel do objeto inteligente: transformação reversível, vínculo e filtros inteligentes.

use eframe::egui;
use orvalith_core::filter::{Filter, KINDS};
use orvalith_core::object::{SmartFilter, SmartObject, MAX_FILTERS};

use crate::filterui::filter_ui;
use crate::layers_ui::LayerCmd;

#[derive(Default)]
pub struct ObjUiState {
    add_kind: usize,
    lock_scale: bool,
}

/// Mostra os controles do objeto. Devolve se algo mudou (exige re-renderizar) e os comandos
/// que precisam do documento inteiro.
pub fn object_ui(
    ui: &mut egui::Ui,
    o: &mut SmartObject,
    st: &mut ObjUiState,
) -> (bool, Vec<LayerCmd>) {
    let mut ch = false;
    let mut cmds = Vec::new();
    ui.strong("Objeto inteligente");
    match &o.link {
        Some(p) => {
            ui.label(format!("Vinculado a: {p}"));
        }
        None => {
            ui.label("Conteúdo embutido no projeto.");
        }
    }
    ui.label(format!("Original: {} × {} px", o.orig_w, o.orig_h));
    if o.is_missing() {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "Arquivo vinculado ausente: a aparência gravada foi mantida, mas não dá para transformar nem filtrar até reencontrá-lo.",
        );
    }
    ui.horizontal(|ui| {
        if o.is_linked() {
            if ui
                .small_button("Atualizar")
                .on_hover_text("Relê o arquivo vinculado")
                .clicked()
            {
                cmds.push(LayerCmd::UpdateLink);
            }
            if ui
                .small_button("Embutir")
                .on_hover_text("Guarda o original no projeto e remove o vínculo")
                .clicked()
            {
                cmds.push(LayerCmd::EmbedObject);
            }
            if ui.small_button("Reencontrar…").clicked() {
                cmds.push(LayerCmd::RelinkObject);
            }
        }
        if ui
            .small_button("Rasterizar")
            .on_hover_text("Descarta o original e os filtros; mantém os pixels")
            .clicked()
        {
            cmds.push(LayerCmd::Rasterize);
        }
    });
    if !o.is_missing() {
        ui.separator();
        ui.label("Transformação (reversível)");
        ui.horizontal(|ui| {
            ch |= ui
                .add(egui::DragValue::new(&mut o.cx).prefix("X ").speed(0.5))
                .changed();
            ch |= ui
                .add(egui::DragValue::new(&mut o.cy).prefix("Y ").speed(0.5))
                .changed();
        });
        let (mut px, mut py) = (o.sx * 100.0, o.sy * 100.0);
        ui.horizontal(|ui| {
            let a = ui
                .add(
                    egui::DragValue::new(&mut px)
                        .prefix("L ")
                        .suffix("%")
                        .speed(0.5)
                        .range(-10000.0..=10000.0),
                )
                .changed();
            let b = ui
                .add(
                    egui::DragValue::new(&mut py)
                        .prefix("A ")
                        .suffix("%")
                        .speed(0.5)
                        .range(-10000.0..=10000.0),
                )
                .changed();
            if a {
                o.sx = px / 100.0;
                if st.lock_scale {
                    o.sy = o.sx;
                }
            }
            if b {
                o.sy = py / 100.0;
                if st.lock_scale {
                    o.sx = o.sy;
                }
            }
            ch |= a || b;
            ui.checkbox(&mut st.lock_scale, "Proporção");
        });
        ch |= ui
            .add(
                egui::DragValue::new(&mut o.rot)
                    .prefix("Giro ")
                    .suffix("°")
                    .speed(0.5),
            )
            .changed();
        if ui.small_button("Redefinir transformação").clicked() {
            o.reset_transform();
            ch = true;
        }
        ui.separator();
        ui.label("Filtros inteligentes");
        let mut remove = None;
        for (i, f) in o.filters.iter_mut().enumerate() {
            egui::CollapsingHeader::new(f.filter.name())
                .id_salt(("smart_filter", i))
                .show(ui, |ui| {
                    if filter_ui(ui, &mut f.filter) {
                        f.filter.sanitize();
                        ch = true;
                    }
                });
            ui.horizontal(|ui| {
                ch |= ui.checkbox(&mut f.enabled, "Ativo").changed();
                if ui.small_button("Remover").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            o.filters.remove(i);
            ch = true;
        }
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("smart_filter_add")
                .selected_text(KINDS[st.add_kind.min(KINDS.len() - 1)])
                .show_ui(ui, |ui| {
                    for (i, k) in KINDS.iter().enumerate() {
                        ui.selectable_value(&mut st.add_kind, i, *k);
                    }
                });
            if ui.button("Adicionar").clicked() && o.filters.len() < MAX_FILTERS {
                o.filters.push(SmartFilter {
                    enabled: true,
                    filter: Filter::neutral(st.add_kind),
                });
                ch = true;
            }
        });
        ui.horizontal(|ui| {
            if ui.small_button("Máscara dos filtros: da seleção").clicked() {
                cmds.push(LayerCmd::FilterMaskFromSelection);
            }
            if o.mask.is_some() && ui.small_button("Remover máscara").clicked() {
                cmds.push(LayerCmd::FilterMaskClear);
            }
        });
        if !o.filters.is_empty() {
            ui.small(
                "Os filtros rodam no original a cada mudança (imagens grandes podem demorar).",
            );
        }
    }
    (ch, cmds)
}
