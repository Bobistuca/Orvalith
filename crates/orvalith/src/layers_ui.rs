//! Painel de camadas: árvore com grupos, miniaturas, propriedades e operações.

use eframe::egui;
use orvalith_core::edit::{LayersEdit, SetVisible};
use orvalith_core::layerops::{self, Outcome};
use orvalith_core::maskops::{self, MaskInit};
use orvalith_core::vecops;
use orvalith_core::vector::BoolOp;
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
    AddMask(MaskInit),
    DeleteMask,
    ApplyMask,
    InvertMask,
    MaskToSelection,
    RefineMask,
    ToggleClip,
    ToggleMaskView,
    Rasterize,
    ToObject,
    EmbedObject,
    UpdateLink,
    RelinkObject,
    FilterMaskFromSelection,
    FilterMaskClear,
    NewFill(usize),
    Boolean(BoolOp),
    VectorMask,
    PathToSelection,
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

    pub(crate) fn apply_outcome(&mut self, label: &str, o: Outcome) {
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
        LayerCmd::AddMask(init) => match active {
            Some(id) => match maskops::add_mask(&open.doc, id, init) {
                Ok(layers) => {
                    open.edit_layers("Adicionar máscara", |ls| *ls = layers);
                    open.edit_mask = true;
                }
                Err(e) => return Some(e),
            },
            None => return nothing(),
        },
        LayerCmd::DeleteMask | LayerCmd::ApplyMask | LayerCmd::InvertMask => {
            let Some(id) = active else { return nothing() };
            let (label, result) = match cmd {
                LayerCmd::DeleteMask => ("Excluir máscara", maskops::delete_mask(&open.doc, id)),
                LayerCmd::ApplyMask => ("Aplicar máscara", maskops::apply_mask(&open.doc, id)),
                _ => ("Inverter máscara", maskops::invert_mask(&open.doc, id)),
            };
            match result {
                Ok(layers) => {
                    open.edit_layers(label, |ls| *ls = layers);
                    if cmd != LayerCmd::InvertMask {
                        open.edit_mask = false;
                        open.mask_view = None;
                        open.doc.bump_view();
                    }
                }
                Err(e) => return Some(e),
            }
        }
        LayerCmd::MaskToSelection => {
            let Some(id) = active else { return nothing() };
            match maskops::mask_to_selection(&open.doc, id) {
                Some(m) => {
                    crate::select::set_selection(open, "Máscara para seleção", Some(m));
                }
                None => return Some("A camada não tem máscara (ou ela está vazia).".into()),
            }
        }
        LayerCmd::RefineMask => {} // tratado pelo aplicativo (abre o diálogo)
        LayerCmd::ToggleClip => {
            let Some(idx) = open.active_idx() else {
                return nothing();
            };
            let clipped = open.doc.layers[idx].clipped;
            open.edit_layers(
                if clipped {
                    "Soltar recorte"
                } else {
                    "Recortar na camada abaixo"
                },
                |ls| ls[idx].clipped = !clipped,
            );
        }
        LayerCmd::ToObject => {
            let Some(id) = active else { return nothing() };
            match orvalith_core::object::convert_to_object(&open.doc, id) {
                Ok(layers) => {
                    open.edit_layers("Converter em objeto inteligente", |ls| *ls = layers)
                }
                Err(e) => return Some(e),
            }
        }
        LayerCmd::EmbedObject
        | LayerCmd::UpdateLink
        | LayerCmd::FilterMaskFromSelection
        | LayerCmd::FilterMaskClear => {
            let Some(id) = active else { return nothing() };
            let (label, res) = match cmd {
                LayerCmd::EmbedObject => (
                    "Embutir objeto",
                    orvalith_core::object::embed(&open.doc, id),
                ),
                LayerCmd::UpdateLink => (
                    "Atualizar vínculo",
                    orvalith_core::object::update_link(&open.doc, id),
                ),
                LayerCmd::FilterMaskFromSelection => (
                    "Máscara dos filtros",
                    orvalith_core::object::set_filter_mask(&open.doc, id, true),
                ),
                _ => (
                    "Remover máscara dos filtros",
                    orvalith_core::object::set_filter_mask(&open.doc, id, false),
                ),
            };
            match res {
                Ok(layers) => open.edit_layers(label, |ls| *ls = layers),
                Err(e) => return Some(e),
            }
        }
        LayerCmd::RelinkObject => {
            // O diálogo de arquivo é aberto pelo aplicativo (ver Action::RelinkObject).
            return Some("Escolha o arquivo no diálogo.".into());
        }
        LayerCmd::NewFill(kind) => {
            let above = active;
            let o = vecops::new_source_layer(
                &open.doc,
                orvalith_core::LayerSource::Fill(orvalith_core::source::FillSpec::neutral(kind)),
                above,
            );
            open.apply_outcome("Nova camada de preenchimento", o);
        }
        LayerCmd::Rasterize => {
            let Some(id) = active else { return nothing() };
            match vecops::rasterize(&open.doc, id) {
                Ok(layers) => open.edit_layers("Rasterizar camada", |ls| *ls = layers),
                Err(e) => return Some(e),
            }
        }
        LayerCmd::Boolean(op) => {
            // A base é a camada mais baixa da seleção; a mais alta é o operando.
            let mut ids = sel.clone();
            ids.sort_by_key(|id| open.doc.index_of(*id));
            let (Some(&a), Some(&b)) = (ids.first(), ids.last()) else {
                return nothing();
            };
            match vecops::boolean_combine(&open.doc, a, b, op) {
                Ok(o) => open.apply_outcome("Combinar formas", o),
                Err(e) => return Some(e),
            }
        }
        LayerCmd::VectorMask => {
            let Some(id) = active else { return nothing() };
            let Some(idx) = open.doc.index_of(id) else {
                return nothing();
            };
            let parent = open.doc.layers[idx].parent;
            let below = open.doc.layers[..idx]
                .iter()
                .rev()
                .find(|l| l.parent == parent)
                .map(|l| l.id);
            match below {
                Some(t) => match vecops::vector_mask_from_shape(&open.doc, id, t) {
                    Ok(layers) => open.edit_layers("Máscara vetorial", |ls| *ls = layers),
                    Err(e) => return Some(e),
                },
                None => return Some("Não há camada abaixo da forma.".into()),
            }
        }
        LayerCmd::PathToSelection => {
            let Some(id) = active else { return nothing() };
            match vecops::shape_to_selection(&open.doc, id) {
                Some(m) => {
                    crate::select::set_selection(open, "Forma para seleção", Some(m));
                }
                None => return Some("A camada ativa não é uma forma editável.".into()),
            }
        }
        LayerCmd::ToggleMaskView => {
            let Some(id) = active else { return nothing() };
            if open.doc.layer(id).is_some_and(|l| l.mask.is_some()) {
                open.mask_view = if open.mask_view == Some(id) {
                    None
                } else {
                    Some(id)
                };
                open.doc.bump_view();
            } else {
                return Some("A camada não tem máscara.".into());
            }
        }
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
    open.mask_thumbs.retain(|id, _| live.contains(id));
    let opts = egui::TextureOptions::NEAREST;
    for l in &open.doc.layers {
        if let Some(img) = thumbnail(l) {
            match open.thumbs.get_mut(&l.id) {
                Some(tex) => tex.set(img, opts),
                None => {
                    let tex = ctx.load_texture(format!("thumb{}", l.id.0), img, opts);
                    open.thumbs.insert(l.id, tex);
                }
            }
        }
        match l
            .mask
            .as_ref()
            .and_then(|m| mask_thumbnail(m.data(), open.doc.width, open.doc.height))
        {
            Some(img) => match open.mask_thumbs.get_mut(&l.id) {
                Some(tex) => tex.set(img, opts),
                None => {
                    let tex = ctx.load_texture(format!("mthumb{}", l.id.0), img, opts);
                    open.mask_thumbs.insert(l.id, tex);
                }
            },
            None => {
                open.mask_thumbs.remove(&l.id);
            }
        }
    }
}

/// Miniatura em tons de cinza de uma máscara.
fn mask_thumbnail(mask: &[u8], w: u32, h: u32) -> Option<egui::ColorImage> {
    if w == 0 || h == 0 || mask.len() != w as usize * h as usize {
        return None;
    }
    let (wu, hu) = (w as usize, h as usize);
    let scale = (wu.max(hu) as f32 / THUMB as f32).max(1.0);
    let (tw, th) = (
        ((wu as f32 / scale).round() as usize).max(1),
        ((hu as f32 / scale).round() as usize).max(1),
    );
    let mut px = Vec::with_capacity(tw * th);
    for y in 0..th {
        for x in 0..tw {
            let sx = (((x as f32 + 0.5) * scale) as usize).min(wu - 1);
            let sy = (((y as f32 + 0.5) * scale) as usize).min(hu - 1);
            px.push(mask[sy * wu + sx]);
        }
    }
    Some(egui::ColorImage::from_gray([tw, th], &px))
}

fn begin_prop_drag(open: &mut OpenDoc, label: &str) {
    if open.prop_drag.is_none() {
        open.prop_drag = Some((label.to_string(), open.doc.layers.clone()));
    }
}

fn end_prop_drag(open: &mut OpenDoc) {
    if let Some((label, before)) = open.prop_drag.take() {
        let changed = before.iter().zip(&open.doc.layers).any(|(a, b)| {
            let mask_changed = match (&a.mask, &b.mask) {
                (Some(x), Some(y)) => {
                    (x.density() - y.density()).abs() > 1e-6
                        || (x.feather() - y.feather()).abs() > 1e-6
                }
                _ => false,
            };
            (a.opacity - b.opacity).abs() > 1e-6
                || (a.fill - b.fill).abs() > 1e-6
                || mask_changed
                || a.source != b.source
                || a.adjustment != b.adjustment
                || a.styles != b.styles
                || a.object != b.object
        });
        if changed {
            let after = open.doc.layers.clone();
            open.history.record(
                &mut open.doc,
                Box::new(LayersEdit::new(&label, before, after)),
            );
        }
    }
}

fn props_ui(ui: &mut egui::Ui, open: &mut OpenDoc) -> Vec<LayerCmd> {
    let mut cmds = Vec::new();
    let Some(idx) = open.active_idx() else {
        return cmds;
    };
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
    let mut clipped = open.doc.layers[idx].clipped;
    if ui
        .checkbox(&mut clipped, "Recortar na camada abaixo")
        .on_hover_text("O conteúdo só aparece onde a camada de baixo tem pixels (Ctrl+Alt+G)")
        .changed()
    {
        cmds.push(LayerCmd::ToggleClip);
    }
    // ---- Origem editável (texto / forma) ----
    if let Some(src) = open.doc.layers[idx].source.clone() {
        ui.separator();
        let mut s = src;
        let mut filter = std::mem::take(&mut open.font_filter);
        let changed = crate::vectool::source_props_ui(ui, &mut s, &mut filter);
        open.font_filter = filter;
        if changed {
            begin_prop_drag(open, "Editar propriedades");
            let (w, h) = (open.doc.width, open.doc.height);
            let l = &mut open.doc.layers[idx];
            if l.source.as_ref().map(|x| x.default_name()).as_deref() == Some(l.name.as_str()) {
                l.name = s.default_name();
            }
            l.source = Some(s);
            l.rerender(w, h);
            open.doc.touch();
        }
        if ui
            .small_button("Rasterizar")
            .on_hover_text("Descarta a edição vetorial/texto e mantém os pixels")
            .clicked()
        {
            cmds.push(LayerCmd::Rasterize);
        }
    }
    // ---- Objeto inteligente ----
    if let Some(obj) = open.doc.layers[idx].object.clone() {
        ui.separator();
        let mut o = obj;
        let mut st = std::mem::take(&mut open.obj_ui);
        let (changed, extra) = crate::objectui::object_ui(ui, &mut o, &mut st);
        open.obj_ui = st;
        cmds.extend(extra);
        if changed {
            begin_prop_drag(open, "Editar objeto inteligente");
            o.sanitize();
            let (w, h) = (open.doc.width, open.doc.height);
            let l = &mut open.doc.layers[idx];
            l.object = Some(o);
            l.rerender(w, h);
            open.doc.touch();
        }
    }
    // ---- Camada de preenchimento ----
    if let Some(orvalith_core::LayerSource::Fill(f)) = open.doc.layers[idx].source.clone() {
        ui.separator();
        let mut f = f;
        let mut st = std::mem::take(&mut open.adj_ui);
        let changed = crate::styleui::fill_ui(ui, &mut f, &mut st);
        open.adj_ui = st;
        if changed {
            begin_prop_drag(open, "Editar preenchimento");
            let (w, h) = (open.doc.width, open.doc.height);
            let l = &mut open.doc.layers[idx];
            if l.source.as_ref().map(|x| x.default_name()).as_deref() == Some(l.name.as_str()) {
                l.name = orvalith_core::LayerSource::Fill(f.clone()).default_name();
            }
            l.source = Some(orvalith_core::LayerSource::Fill(f));
            l.rerender(w, h);
            open.doc.touch();
        }
    }
    // ---- Estilos de camada ----
    if open.doc.layers[idx].adjustment.is_none() && !open.doc.layers[idx].is_group() {
        ui.separator();
        let mut list = open.doc.layers[idx].styles.clone();
        let mut st = std::mem::take(&mut open.style_ui);
        let changed = crate::styleui::styles_ui(ui, &mut list, &mut st);
        open.style_ui = st;
        if changed {
            begin_prop_drag(open, "Editar estilos");
            open.doc.layers[idx].styles = list;
            open.doc.touch();
        }
    }
    // ---- Camada de ajuste (não destrutiva) ----
    if let Some(adj) = open.doc.layers[idx].adjustment.clone() {
        ui.separator();
        ui.label(format!("Ajuste: {}", adj.name()));
        let mut a = adj;
        let mut st = std::mem::take(&mut open.adj_ui);
        let changed = crate::adjustui::adjust_ui(ui, &mut a, &mut st);
        open.adj_ui = st;
        if changed {
            begin_prop_drag(open, "Editar ajuste");
            a.sanitize();
            open.doc.layers[idx].adjustment = Some(a);
            open.doc.touch();
        }
        ui.small("Age sobre as camadas abaixo; use opacidade, modo, máscara e recorte como em outras camadas.");
    }
    ui.separator();
    // ---- Máscara de camada ----
    if let Some((enabled, linked, density, feather)) = open.doc.layers[idx]
        .mask
        .as_ref()
        .map(|m| (m.enabled, m.linked, m.density(), m.feather()))
    {
        ui.strong("Máscara");
        let (mut en, mut ln) = (enabled, linked);
        ui.horizontal(|ui| {
            ui.checkbox(&mut en, "Ativa");
            ui.checkbox(&mut ln, "Vinculada")
                .on_hover_text("Mover/transformar a camada move a máscara junto");
        });
        if en != enabled || ln != linked {
            open.edit_layers("Alterar máscara", |ls| {
                if let Some(m) = ls[idx].mask.as_mut() {
                    m.enabled = en;
                    m.linked = ln;
                }
            });
        }
        for (label, is_feather) in [("Densidade", false), ("Pena", true)] {
            let mut v = if is_feather { feather } else { density * 100.0 };
            let r = ui.add(if is_feather {
                egui::Slider::new(&mut v, 0.0..=100.0)
                    .text(label)
                    .suffix(" px")
            } else {
                egui::Slider::new(&mut v, 0.0..=100.0)
                    .text(label)
                    .suffix("%")
                    .integer()
            });
            if r.changed() {
                begin_prop_drag(
                    open,
                    if is_feather {
                        "Pena da máscara"
                    } else {
                        "Densidade da máscara"
                    },
                );
                if let Some(m) = open.doc.layers[idx].mask.as_mut() {
                    if is_feather {
                        m.set_feather(v);
                    } else {
                        m.set_density(v / 100.0);
                    }
                }
                open.doc.touch();
            }
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                end_prop_drag(open);
            }
        }
        ui.horizontal_wrapped(|ui| {
            for (label, cmd, tip) in [
                (
                    "Aplicar",
                    LayerCmd::ApplyMask,
                    "Aplica ao alfa dos pixels e remove a máscara",
                ),
                (
                    "Excluir",
                    LayerCmd::DeleteMask,
                    "Remove a máscara sem alterar os pixels",
                ),
                ("Inverter", LayerCmd::InvertMask, "Inverte a máscara"),
                (
                    "→ Seleção",
                    LayerCmd::MaskToSelection,
                    "Cria uma seleção a partir da máscara",
                ),
                (
                    "Refinar…",
                    LayerCmd::RefineMask,
                    "Suavizar, deslocar borda, pena e contraste",
                ),
                (
                    "Ver máscara",
                    LayerCmd::ToggleMaskView,
                    "Alterna a visão isolada da máscara em cinza",
                ),
            ] {
                if ui.small_button(label).on_hover_text(tip).clicked() {
                    cmds.push(cmd);
                }
            }
        });
        let mut target_mask = open.edit_mask;
        ui.horizontal(|ui| {
            ui.label("Pintar em:");
            ui.radio_value(&mut target_mask, false, "Camada");
            ui.radio_value(&mut target_mask, true, "Máscara");
        });
        open.edit_mask = target_mask;
    } else {
        open.edit_mask = false;
        ui.horizontal_wrapped(|ui| {
            ui.label("Máscara:");
            for (label, init, tip) in [
                (
                    "+ Revelar tudo",
                    MaskInit::Reveal,
                    "Máscara branca (tudo visível)",
                ),
                (
                    "+ Ocultar tudo",
                    MaskInit::Hide,
                    "Máscara preta (tudo oculto)",
                ),
                (
                    "+ Da seleção",
                    MaskInit::FromSelection,
                    "Visível só dentro da seleção",
                ),
                (
                    "+ Da seleção invertida",
                    MaskInit::FromSelectionInverse,
                    "Oculta o que está dentro da seleção",
                ),
            ] {
                if ui.small_button(label).on_hover_text(tip).clicked() {
                    cmds.push(LayerCmd::AddMask(init));
                }
            }
        });
    }
    cmds
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
    /// Clique numa miniatura: (camada, é a máscara?, modificadores).
    PickTarget(LayerId, bool, egui::Modifiers),
}

fn list_ui(ui: &mut egui::Ui, open: &mut OpenDoc) -> Vec<LayerCmd> {
    let rows = visible_rows(open);
    let mut events: Vec<RowEvent> = Vec::new();
    ui.scope(|ui| {
        for (id, depth) in &rows {
            let Some(l) = open.doc.layer(*id) else {
                continue;
            };
            let (name, visible, group, expanded) =
                (l.name.clone(), l.visible, l.is_group(), l.expanded);
            let locked = l.lock_pixels || l.lock_alpha || l.lock_position;
            let clipped = l.clipped;
            let tag = match &l.source {
                Some(orvalith_core::LayerSource::Text(_)) => "T ",
                Some(orvalith_core::LayerSource::Shape(_)) => "F ",
                Some(orvalith_core::LayerSource::Fill(_)) => "P ",
                None if l.object.is_some() => "O ",
                None if l.adjustment.is_some() => "A ",
                None if !l.styles.is_empty() => "fx ",
                None => "",
            };
            let has_mask = l.mask.is_some();
            let mask_enabled = l.mask.as_ref().is_some_and(|m| m.enabled);
            let selected = open.selected.contains(id);
            let is_active = open.active_layer == *id;
            ui.horizontal(|ui| {
                ui.add_space(*depth as f32 * 14.0 + if clipped { 10.0 } else { 0.0 });
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
                let accent = crate::theme::ACCENT;
                let thumb = |ui: &mut egui::Ui, tex: Option<&egui::TextureHandle>, picked: bool| {
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
                    ui.painter()
                        .rect_filled(rect, 2.0, egui::Color32::from_gray(90));
                    if let Some(tex) = tex {
                        let sz = tex.size_vec2();
                        let fit = egui::Rect::from_center_size(rect.center(), sz.min(rect.size()));
                        ui.painter().image(
                            tex.id(),
                            fit,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    if picked {
                        ui.painter().rect_stroke(
                            rect,
                            2.0,
                            egui::Stroke::new(2.0, accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    resp
                };
                let tr = thumb(ui, open.thumbs.get(id), is_active && !open.edit_mask);
                if tr.clicked() {
                    events.push(RowEvent::PickTarget(*id, false, ui.input(|i| i.modifiers)));
                }
                if has_mask {
                    let mr = thumb(ui, open.mask_thumbs.get(id), is_active && open.edit_mask);
                    if !mask_enabled {
                        let r = mr.rect;
                        let red = egui::Stroke::new(2.0, egui::Color32::from_rgb(220, 60, 60));
                        ui.painter()
                            .line_segment([r.left_top(), r.right_bottom()], red);
                        ui.painter()
                            .line_segment([r.right_top(), r.left_bottom()], red);
                    }
                    if mr.clicked() {
                        events.push(RowEvent::PickTarget(*id, true, ui.input(|i| i.modifiers)));
                    }
                    mr.on_hover_text(
                        "Máscara (Alt+clique: ver em cinza · Ctrl+clique: máscara → seleção)",
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
                    let mut label = if clipped {
                        format!("> {tag}{name}")
                    } else {
                        format!("{tag}{name}")
                    };
                    if locked {
                        label.push_str("  (bloq.)");
                    }
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
    let mut cmds = Vec::new();
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
            RowEvent::PickTarget(id, mask, m) => {
                open.selected = vec![id];
                open.active_layer = id;
                open.edit_mask = mask;
                if mask && m.alt {
                    open.mask_view = if open.mask_view == Some(id) {
                        None
                    } else {
                        Some(id)
                    };
                    open.doc.bump_view();
                } else if mask && m.command {
                    cmds.push(LayerCmd::MaskToSelection);
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
    cmds
}

/// Painel de canais: canais de cor (visualização) e seleções salvas como canais alfa.
pub fn channels_ui(ui: &mut egui::Ui, open: &mut OpenDoc) {
    let mut ch_changed = false;
    ui.horizontal(|ui| {
        for (i, label) in ["R", "G", "B"].iter().enumerate() {
            ch_changed |= ui.checkbox(&mut open.view_channels[i], *label).changed();
        }
    });
    if ch_changed {
        // Só afeta a visualização; força o redesenho.
        open.doc.bump_view();
    }
    if open.view_channels != [true; 3] {
        ui.small("Mostrando só alguns canais (a exportação não é afetada).");
    }
    ui.separator();
    if open.doc.channels.is_empty() {
        ui.small("Nenhum canal alfa salvo. Use Selecionar → Canais de seleção.");
    }
    let mut load: Option<usize> = None;
    let mut delete: Option<usize> = None;
    let mut edit: Option<u64> = None;
    let mut rename: Option<(u64, String)> = None;
    let mut view_toggle: Option<u64> = None;
    let chans: Vec<(u64, String)> = open
        .doc
        .channels
        .iter()
        .map(|c| (c.id, c.name.clone()))
        .collect();
    for (i, (id, name)) in chans.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut shown = open.channel_view == Some(*id);
            if ui
                .checkbox(&mut shown, "")
                .on_hover_text("Ver o canal em cinza")
                .changed()
            {
                view_toggle = Some(*id);
            }
            if open
                .renaming_channel
                .as_ref()
                .is_some_and(|(rid, _)| rid == id)
            {
                if let Some((_, text)) = open.renaming_channel.as_mut() {
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(90.0));
                    r.request_focus();
                    if r.lost_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            rename = Some((*id, text.clone()));
                        } else {
                            open.renaming_channel = None;
                        }
                    }
                }
            } else {
                let r = ui.label(name);
                if r.double_clicked() {
                    open.renaming_channel = Some((*id, name.clone()));
                }
                r.on_hover_text("Duplo clique renomeia");
            }
            if ui.small_button("Selecionar").clicked() {
                load = Some(i);
            }
            if ui
                .small_button("Editar")
                .on_hover_text("Edita o canal na máscara rápida")
                .clicked()
            {
                edit = Some(*id);
            }
            if ui.small_button("Excluir").clicked() {
                delete = Some(i);
            }
        });
    }
    if let Some(id) = view_toggle {
        open.channel_view = if open.channel_view == Some(id) {
            None
        } else {
            Some(id)
        };
        open.doc.bump_view();
    }
    if let Some(i) = load {
        crate::select::channel_load(open, i, orvalith_core::selection::Combine::Replace);
    }
    if let Some(i) = delete {
        crate::select::channel_delete(open, i);
    }
    if let Some((id, name)) = rename {
        open.renaming_channel = None;
        crate::select::channel_rename(open, id, name.trim());
    }
    if let Some(id) = edit {
        if open.qm.is_none() {
            open.enter_quick_mask_for_channel(id);
        }
    }
}

/// Desenha o painel e devolve as operações pedidas pelos botões.
pub fn show(ui: &mut egui::Ui, open: &mut OpenDoc, busy: bool) -> Vec<LayerCmd> {
    update_thumbs(ui.ctx(), open, busy);
    let mut cmds = props_ui(ui, open);
    // Edições contínuas (campos numéricos, cores, digitação) viram uma ação ao soltar o mouse/sair do campo.
    if open.prop_drag.is_some()
        && !ui.input(|i| i.pointer.any_down())
        && !ui.ctx().egui_wants_keyboard_input()
    {
        end_prop_drag(open);
    }
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
    if let Some(mv) = open.mask_view {
        let name = open
            .doc
            .layer(mv)
            .map(|l| l.name.clone())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.colored_label(crate::theme::COPPER, format!("Vendo a máscara de: {name}"));
            if ui.small_button("Voltar").clicked() {
                open.mask_view = None;
                open.doc.bump_view();
            }
        });
    }
    ui.separator();
    cmds.extend(list_ui(ui, open));
    ui.separator();
    egui::CollapsingHeader::new("Canais")
        .default_open(false)
        .show(ui, |ui| channels_ui(ui, open));
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
    #[test]
    fn mask_commands_are_single_undoable_actions_and_keep_pixels() {
        use orvalith_core::compose::pixel_at;
        let mut o = open_with_two();
        let original = o.doc.layers[1].pixels.clone();
        assert!(run1(&mut o, LayerCmd::AddMask(MaskInit::Hide)).is_none());
        assert!(o.edit_mask && o.doc.layers[1].mask.is_some());
        assert_eq!(pixel_at(&o.doc, 1, 1), Some([10, 10, 10, 255]));
        assert_eq!(
            o.doc.layers[1].pixels, original,
            "pixels intactos sob a máscara"
        );
        assert!(
            run1(&mut o, LayerCmd::AddMask(MaskInit::Reveal)).is_some(),
            "já tem máscara"
        );
        assert!(run1(&mut o, LayerCmd::InvertMask).is_none());
        assert_eq!(pixel_at(&o.doc, 1, 1), Some([200, 0, 0, 255]));
        assert!(run1(&mut o, LayerCmd::ToggleMaskView).is_none());
        assert_eq!(o.mask_view, Some(o.doc.layers[1].id));
        assert!(run1(&mut o, LayerCmd::ApplyMask).is_none());
        assert!(o.doc.layers[1].mask.is_none() && !o.edit_mask && o.mask_view.is_none());
        assert_eq!(
            o.history.labels(),
            vec!["Adicionar máscara", "Inverter máscara", "Aplicar máscara"]
        );
        o.history.undo(&mut o.doc);
        o.history.undo(&mut o.doc);
        assert_eq!(pixel_at(&o.doc, 1, 1), Some([10, 10, 10, 255]));
        o.history.undo(&mut o.doc);
        assert!(o.doc.layers[1].mask.is_none());
        assert!(run1(&mut o, LayerCmd::DeleteMask).is_some(), "sem máscara");
    }

    #[test]
    fn clip_toggle_and_density_drag_record_one_action_each() {
        use orvalith_core::compose::pixel_at;
        let mut o = open_with_two();
        // Camada clipada na de baixo (fundo opaco): continua visível por inteiro.
        assert!(run1(&mut o, LayerCmd::ToggleClip).is_none());
        assert!(o.doc.layers[1].clipped);
        assert_eq!(pixel_at(&o.doc, 0, 0), Some([200, 0, 0, 255]));
        o.history.undo(&mut o.doc);
        assert!(!o.doc.layers[1].clipped);
        run1(&mut o, LayerCmd::AddMask(MaskInit::Hide));
        let before = o.history.labels().len();
        begin_prop_drag(&mut o, "Densidade da máscara");
        for d in [0.9, 0.6, 0.3] {
            o.doc.layers[1].mask.as_mut().unwrap().set_density(d);
            o.doc.touch();
        }
        end_prop_drag(&mut o);
        assert_eq!(o.history.labels().len(), before + 1);
        assert_eq!(o.doc.layers[1].mask.as_ref().unwrap().density(), 0.3);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[1].mask.as_ref().unwrap().density(), 1.0);
    }

    #[test]
    fn mask_thumbnail_is_gray_and_sized() {
        let t = mask_thumbnail(&vec![128u8; 200 * 100], 200, 100).unwrap();
        assert_eq!(t.size, [32, 16]);
        assert!(mask_thumbnail(&[0u8; 3], 4, 4).is_none());
    }
    #[test]
    fn shape_layer_commands_rasterize_combine_and_convert() {
        use orvalith_core::vector::{Geom, ShapeSpec};
        use orvalith_core::{compose::pixel_at, LayerSource};
        let mut o = open_with_two();
        let rect = |x: f32| {
            LayerSource::Shape(ShapeSpec::solid(
                Geom::Rect {
                    x,
                    y: 0.0,
                    w: 3.0,
                    h: 3.0,
                    radius: 0.0,
                },
                [0, 200, 0, 255],
            ))
        };
        crate::vectool::VecState::add_layer(&mut o, rect(0.0), "Forma");
        crate::vectool::VecState::add_layer(&mut o, rect(2.0), "Forma");
        let (a, b) = (o.doc.layers[2].id, o.doc.layers[3].id);
        o.selected = vec![b, a];
        // Combinar: base = mais baixa; vira uma forma booleana e a outra some.
        assert!(run1(&mut o, LayerCmd::Boolean(BoolOp::Subtract)).is_none());
        assert_eq!(o.doc.layers.len(), 3);
        assert_eq!(pixel_at(&o.doc, 0, 0), Some([0, 200, 0, 255]));
        assert_eq!(
            pixel_at(&o.doc, 2, 1),
            Some([200, 0, 0, 255]),
            "interseção subtraída"
        );
        // Forma → seleção e máscara vetorial na camada de baixo.
        o.active_layer = a;
        assert!(run1(&mut o, LayerCmd::PathToSelection).is_none());
        assert!(o.doc.selection.mask.is_some());
        assert!(run1(&mut o, LayerCmd::VectorMask).is_none());
        let below = o.doc.layers[1].mask.as_ref().unwrap();
        assert!(below.vector_source().is_some());
        // Rasterizar uma vez; depois avisa que já é raster.
        o.active_layer = a;
        assert!(run1(&mut o, LayerCmd::Rasterize).is_none());
        assert!(o.doc.layer(a).unwrap().source.is_none());
        assert!(run1(&mut o, LayerCmd::Rasterize).is_some());
        assert!(
            run1(&mut o, LayerCmd::PathToSelection).is_some(),
            "não é mais forma"
        );
        o.history.undo(&mut o.doc);
        assert!(o.doc.layer(a).unwrap().source.is_some());
    }

    #[test]
    fn source_edits_from_the_panel_are_one_undoable_action() {
        use orvalith_core::vector::{Geom, ShapeSpec};
        use orvalith_core::LayerSource;
        let mut o = open_with_two();
        crate::vectool::VecState::add_layer(
            &mut o,
            LayerSource::Shape(ShapeSpec::solid(
                Geom::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 2.0,
                    h: 2.0,
                    radius: 0.0,
                },
                [0, 0, 255, 255],
            )),
            "Forma",
        );
        let idx = o.doc.layers.len() - 1;
        let before = o.history.labels().len();
        begin_prop_drag(&mut o, "Editar propriedades");
        for w in [3.0, 4.0] {
            let (dw, dh) = (o.doc.width, o.doc.height);
            if let Some(LayerSource::Shape(s)) = o.doc.layers[idx].source.as_mut() {
                s.geom = Geom::Rect {
                    x: 0.0,
                    y: 0.0,
                    w,
                    h: 2.0,
                    radius: 0.0,
                };
            }
            o.doc.layers[idx].rerender(dw, dh);
            o.doc.touch();
        }
        end_prop_drag(&mut o);
        assert_eq!(o.history.labels().len(), before + 1);
        o.history.undo(&mut o.doc);
        match &o.doc.layers[idx].source {
            Some(LayerSource::Shape(ShapeSpec {
                geom: Geom::Rect { w, .. },
                ..
            })) => assert_eq!(*w, 2.0),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn fill_layers_and_style_edits_are_undoable() {
        use orvalith_core::style::{Effect, EffectKind};
        let mut o = open_with_two();
        let n = o.doc.layers.len();
        assert!(run1(&mut o, LayerCmd::NewFill(1)).is_none());
        assert_eq!(o.doc.layers.len(), n + 1);
        let id = o.doc.layers[n].id;
        assert!(matches!(
            o.doc.layers[n].source,
            Some(orvalith_core::LayerSource::Fill(_))
        ));
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers.len(), n);
        // Estilos: editar e desfazer em uma ação.
        let before = o.history.labels().len();
        begin_prop_drag(&mut o, "Editar estilos");
        o.doc.layers[0].styles = vec![Effect::new(EffectKind::neutral(0))];
        o.doc.touch();
        end_prop_drag(&mut o);
        assert_eq!(o.history.labels().len(), before + 1);
        o.history.undo(&mut o.doc);
        assert!(o.doc.layers[0].styles.is_empty());
        let _ = id;
    }
    #[test]
    fn smart_object_commands_and_edits_are_undoable() {
        let mut o = open_with_two();
        let top = o.doc.layers.len() - 1;
        o.doc.layers[top].set_pixel(1, 1, [200, 10, 10, 255]);
        o.doc.layers[top].set_pixel(2, 1, [10, 200, 10, 255]);
        let before_px = o.doc.layers[top].pixels.clone();
        assert!(run1(&mut o, LayerCmd::ToObject).is_none());
        assert!(o.doc.layers[top].object.is_some());
        assert_eq!(o.doc.layers[top].pixels, before_px, "aparência idêntica");
        // Editar a transformação como no painel: uma ação; desfazer volta.
        let n = o.history.labels().len();
        begin_prop_drag(&mut o, "Editar objeto inteligente");
        let (w, h) = (o.doc.width, o.doc.height);
        o.doc.layers[top]
            .object
            .as_mut()
            .unwrap()
            .translate(1.0, 0.0);
        o.doc.layers[top].rerender(w, h);
        o.doc.touch();
        end_prop_drag(&mut o);
        assert_eq!(o.history.labels().len(), n + 1);
        assert_eq!(o.doc.layers[top].pixel(2, 1), Some([200, 10, 10, 255]));
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[top].pixels, before_px);
        // Rasterizar remove o objeto.
        assert!(run1(&mut o, LayerCmd::Rasterize).is_none());
        assert!(o.doc.layers[top].object.is_none());
        assert!(
            run1(&mut o, LayerCmd::EmbedObject).is_some(),
            "sem objeto: aviso"
        );
    }
}
