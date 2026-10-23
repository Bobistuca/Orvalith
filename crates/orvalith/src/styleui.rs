//! Interface dos estilos de camada, predefinições e camadas de preenchimento.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use eframe::egui;
use orvalith_core::adjust::GradStop;
use orvalith_core::source::{FillSpec, FILL_KINDS};
use orvalith_core::style::{Effect, EffectKind, Pattern, StrokePos, StylePreset, KINDS};
use orvalith_core::BlendMode;

use crate::adjustui::{gradient_editor, AdjUiState};

/// Predefinições de estilo compartilhadas pela interface; o app copia de/para as preferências.
static PRESETS: Mutex<Vec<StylePreset>> = Mutex::new(Vec::new());
static DIRTY: AtomicBool = AtomicBool::new(false);

pub fn load_presets(list: Vec<StylePreset>) {
    if let Ok(mut p) = PRESETS.lock() {
        *p = list;
    }
}

/// Devolve as predefinições se mudaram desde a última chamada (para gravar nas preferências).
pub fn take_changed_presets() -> Option<Vec<StylePreset>> {
    if DIRTY.swap(false, Ordering::Relaxed) {
        PRESETS.lock().ok().map(|p| p.clone())
    } else {
        None
    }
}

#[derive(Default)]
pub struct StyleUiState {
    add_kind: usize,
    preset_name: String,
    grad: AdjUiState,
}

fn slider(
    ui: &mut egui::Ui,
    label: &str,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.add(egui::Slider::new(v, range).text(label)).changed()
}

fn pct(ui: &mut egui::Ui, label: &str, v: &mut f32) -> bool {
    let mut p = *v * 100.0;
    let ch = ui
        .add(
            egui::Slider::new(&mut p, 0.0..=100.0)
                .text(label)
                .suffix("%")
                .integer(),
        )
        .changed();
    if ch {
        *v = p / 100.0;
    }
    ch
}

fn blend_combo(ui: &mut egui::Ui, id: egui::Id, blend: &mut BlendMode) -> bool {
    let before = *blend;
    egui::ComboBox::from_id_salt(id)
        .selected_text(blend.label())
        .width(130.0)
        .show_ui(ui, |ui| {
            for m in BlendMode::LAYER_MODES {
                ui.selectable_value(blend, m, m.label());
            }
        });
    *blend != before
}

fn color(ui: &mut egui::Ui, label: &str, c: &mut [u8; 3]) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.color_edit_button_srgb(c).changed()
    })
    .inner
}

fn pattern_ui(ui: &mut egui::Ui, p: &mut Pattern) -> bool {
    let mut ch = false;
    let mut idx = p.index();
    egui::ComboBox::from_id_salt("pattern_kind")
        .selected_text(Pattern::NAMES[idx])
        .show_ui(ui, |ui| {
            for (i, n) in Pattern::NAMES.iter().enumerate() {
                ui.selectable_value(&mut idx, i, *n);
            }
        });
    if idx != p.index() {
        *p = Pattern::default_of(idx);
        ch = true;
    }
    match p {
        Pattern::Checker { a, b } | Pattern::Dots { a, b } => {
            ch |= color(ui, "Cor A", a);
            ch |= color(ui, "Cor B", b);
        }
        Pattern::Stripes { a, b, angle } => {
            ch |= color(ui, "Cor A", a);
            ch |= color(ui, "Cor B", b);
            ch |= slider(ui, "Ângulo (°)", angle, -180.0..=180.0);
        }
    }
    ch
}

fn effect_ui(ui: &mut egui::Ui, id: usize, kind: &mut EffectKind, grad: &mut AdjUiState) -> bool {
    let mut ch = false;
    let bid = ui.scope_id().with(("style_blend", id));
    match kind {
        EffectKind::DropShadow {
            color: c,
            opacity,
            angle,
            distance,
            spread,
            size,
            blend,
        }
        | EffectKind::InnerShadow {
            color: c,
            opacity,
            angle,
            distance,
            spread,
            size,
            blend,
        } => {
            ch |= color(ui, "Cor", c);
            ch |= blend_combo(ui, bid, blend);
            ch |= pct(ui, "Opacidade", opacity);
            ch |= slider(ui, "Ângulo (°)", angle, -180.0..=180.0);
            ch |= slider(ui, "Distância", distance, 0.0..=100.0);
            ch |= pct(ui, "Espalhar", spread);
            ch |= slider(ui, "Tamanho", size, 0.0..=100.0);
        }
        EffectKind::OuterGlow {
            color: c,
            opacity,
            spread,
            size,
            blend,
        }
        | EffectKind::InnerGlow {
            color: c,
            opacity,
            spread,
            size,
            blend,
        } => {
            ch |= color(ui, "Cor", c);
            ch |= blend_combo(ui, bid, blend);
            ch |= pct(ui, "Opacidade", opacity);
            ch |= pct(ui, "Espalhar", spread);
            ch |= slider(ui, "Tamanho", size, 0.0..=100.0);
        }
        EffectKind::Stroke {
            color: c,
            opacity,
            size,
            position,
            blend,
        } => {
            ch |= color(ui, "Cor", c);
            ch |= blend_combo(ui, bid, blend);
            ch |= pct(ui, "Opacidade", opacity);
            ch |= slider(ui, "Tamanho", size, 0.0..=100.0);
            ui.horizontal(|ui| {
                ch |= ui
                    .selectable_value(position, StrokePos::Outside, "Fora")
                    .changed();
                ch |= ui
                    .selectable_value(position, StrokePos::Center, "Centro")
                    .changed();
                ch |= ui
                    .selectable_value(position, StrokePos::Inside, "Dentro")
                    .changed();
            });
        }
        EffectKind::ColorOverlay {
            color: c,
            opacity,
            blend,
        } => {
            ch |= color(ui, "Cor", c);
            ch |= blend_combo(ui, bid, blend);
            ch |= pct(ui, "Opacidade", opacity);
        }
        EffectKind::GradientOverlay {
            stops,
            angle,
            reverse,
            opacity,
            blend,
        } => {
            ch |= blend_combo(ui, bid, blend);
            ch |= pct(ui, "Opacidade", opacity);
            ch |= slider(ui, "Ângulo (°)", angle, -180.0..=180.0);
            ch |= ui.checkbox(reverse, "Inverter").changed();
            ch |= gradient_editor(ui, stops, grad);
        }
        EffectKind::PatternOverlay {
            pattern,
            scale,
            opacity,
            blend,
        } => {
            ch |= blend_combo(ui, bid, blend);
            ch |= pct(ui, "Opacidade", opacity);
            ch |= slider(ui, "Escala (px)", scale, 1.0..=128.0);
            ch |= pattern_ui(ui, pattern);
        }
        EffectKind::Bevel {
            depth,
            size,
            angle,
            highlight,
            shadow,
            opacity,
        } => {
            ch |= slider(ui, "Profundidade", depth, 1.0..=100.0);
            ch |= slider(ui, "Tamanho", size, 1.0..=50.0);
            ch |= slider(ui, "Ângulo (°)", angle, -180.0..=180.0);
            ch |= color(ui, "Realce", highlight);
            ch |= color(ui, "Sombra", shadow);
            ch |= pct(ui, "Opacidade", opacity);
        }
    }
    ch
}

/// Editor da lista de estilos da camada. Retorna `true` quando algo mudou.
pub fn styles_ui(ui: &mut egui::Ui, list: &mut Vec<Effect>, st: &mut StyleUiState) -> bool {
    let mut ch = false;
    ui.strong("Estilos da camada");
    let mut remove = None;
    let mut swap = None;
    let n = list.len();
    for (i, e) in list.iter_mut().enumerate() {
        let name = e.kind.name();
        egui::CollapsingHeader::new(name)
            .id_salt(("style_hdr", i))
            .default_open(false)
            .show(ui, |ui| {
                if effect_ui(ui, i, &mut e.kind, &mut st.grad) {
                    e.kind.sanitize();
                    ch = true;
                }
            });
        ui.horizontal(|ui| {
            ch |= ui.checkbox(&mut e.enabled, "Ativo").changed();
            if ui
                .add_enabled(i > 0, egui::Button::new("▲").small())
                .clicked()
            {
                swap = Some((i, i - 1));
            }
            if ui
                .add_enabled(i + 1 < n, egui::Button::new("▼").small())
                .clicked()
            {
                swap = Some((i, i + 1));
            }
            if ui.small_button("Remover").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some((a, b)) = swap {
        list.swap(a, b);
        ch = true;
    }
    if let Some(i) = remove {
        list.remove(i);
        ch = true;
    }
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("style_add")
            .selected_text(KINDS[st.add_kind.min(8)])
            .show_ui(ui, |ui| {
                for (i, k) in KINDS.iter().enumerate() {
                    ui.selectable_value(&mut st.add_kind, i, *k);
                }
            });
        if ui.button("Adicionar").clicked() && list.len() < 16 {
            let mut k = EffectKind::neutral(st.add_kind);
            k.sanitize();
            list.push(Effect::new(k));
            ch = true;
        }
    });
    ui.small("Efeitos agem sobre o alfa da camada (com máscara e preenchimento); não valem em grupos nem em camadas recortadas.");
    // Predefinições.
    ui.separator();
    ui.label("Predefinições de estilo");
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut st.preset_name)
                .hint_text("Nome")
                .desired_width(110.0),
        );
        if ui
            .add_enabled(
                !list.is_empty() && !st.preset_name.trim().is_empty(),
                egui::Button::new("Salvar"),
            )
            .clicked()
        {
            if let Ok(mut p) = PRESETS.lock() {
                let name: String = st.preset_name.trim().chars().take(60).collect();
                p.retain(|x| x.name != name);
                p.push(StylePreset {
                    name,
                    effects: list.clone(),
                });
                p.truncate(100);
                DIRTY.store(true, Ordering::Relaxed);
            }
        }
    });
    let presets: Vec<StylePreset> = PRESETS.lock().map(|p| p.clone()).unwrap_or_default();
    let mut delete = None;
    for (i, p) in presets.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui
                .button(&p.name)
                .on_hover_text("Substitui os estilos da camada")
                .clicked()
            {
                *list = p.effects.clone();
                ch = true;
            }
            if ui
                .small_button("✕")
                .on_hover_text("Excluir predefinição")
                .clicked()
            {
                delete = Some(i);
            }
        });
    }
    if let Some(i) = delete {
        if let Ok(mut p) = PRESETS.lock() {
            if i < p.len() {
                p.remove(i);
                DIRTY.store(true, Ordering::Relaxed);
            }
        }
    }
    ch
}

/// Editor de uma camada de preenchimento.
pub fn fill_ui(ui: &mut egui::Ui, f: &mut FillSpec, st: &mut AdjUiState) -> bool {
    let mut ch = false;
    ui.strong("Preenchimento");
    let mut k = f.kind_index();
    ui.horizontal(|ui| {
        for (i, n) in FILL_KINDS.iter().enumerate() {
            ui.selectable_value(&mut k, i, *n);
        }
    });
    if k != f.kind_index() {
        *f = FillSpec::neutral(k);
        ch = true;
    }
    match f {
        FillSpec::Solid { color: c } => {
            let mut col = egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            if ui.color_edit_button_srgba(&mut col).changed() {
                *c = col.to_srgba_unmultiplied();
                ch = true;
            }
        }
        FillSpec::Gradient {
            stops,
            angle,
            radial,
            reverse,
        } => {
            ch |= slider(ui, "Ângulo (°)", angle, -180.0..=180.0);
            ch |= ui.checkbox(radial, "Radial").changed();
            ch |= ui.checkbox(reverse, "Inverter").changed();
            let before: Vec<GradStop> = stops.clone();
            ch |= gradient_editor(ui, stops, st);
            if stops.len() < 2 {
                *stops = before;
            }
        }
        FillSpec::Pattern { pattern, scale } => {
            ch |= slider(ui, "Escala (px)", scale, 1.0..=256.0);
            ch |= pattern_ui(ui, pattern);
        }
    }
    ch
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::source::FillSpec;

    #[test]
    fn presets_are_shared_and_reported_once_when_changed() {
        let _ = take_changed_presets();
        load_presets(vec![StylePreset {
            name: "a".into(),
            effects: vec![Effect::new(EffectKind::neutral(0))],
        }]);
        assert!(
            take_changed_presets().is_none(),
            "carregar não conta como mudança"
        );
        DIRTY.store(true, Ordering::Relaxed);
        let got = take_changed_presets().unwrap();
        assert_eq!(got.len(), 1);
        assert!(take_changed_presets().is_none());
    }

    #[test]
    fn fill_defaults_render_full_canvas() {
        for k in 0..3 {
            let px = FillSpec::neutral(k).render(6, 4);
            assert_eq!(px.len(), 6 * 4 * 4);
            assert!(px.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        }
    }
}
