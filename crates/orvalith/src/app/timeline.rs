//! Painel da linha do tempo: reprodução, quadros, quadros-chave, pele de cebola e exportação.

use std::time::{Duration, Instant};

use eframe::egui;
use orvalith_core::anim::{self, AnimFrame, Ease, GifOpts, Prop, Timeline, TimelineEdit};
use orvalith_core::comps;
use orvalith_core::fsio;

use super::OrvalithApp;
use crate::canvas::OpenDoc;

pub(super) struct TimelineUi {
    pub open: bool,
    pub playing: bool,
    /// Mostrar a animação no canvas (desligue para editar as camadas livremente).
    pub preview: bool,
    pub loop_play: bool,
    pub onion: bool,
    onion_before: u32,
    onion_after: u32,
    onion_opacity: f32,
    last_tick: Option<Instant>,
    accum: f32,
    window_start: Option<Instant>,
    shown: u32,
    achieved: Option<f32>,
    gif_speed: i32,
    gif_threshold: u8,
    gif_bg: bool,
    gif_bg_color: [u8; 3],
}

impl Default for TimelineUi {
    fn default() -> Self {
        Self {
            open: false,
            playing: false,
            preview: true,
            loop_play: true,
            onion: false,
            onion_before: 1,
            onion_after: 0,
            onion_opacity: 0.4,
            last_tick: None,
            accum: 0.0,
            window_start: None,
            shown: 0,
            achieved: None,
            gif_speed: 10,
            gif_threshold: 128,
            gif_bg: false,
            gif_bg_color: [255, 255, 255],
        }
    }
}

/// Aplica uma alteração à linha do tempo como uma ação de desfazer.
pub(super) fn edit_timeline(open: &mut OpenDoc, label: &str, f: impl FnOnce(&mut Timeline)) {
    let before = open.doc.timeline.clone();
    let mut after = before.clone().unwrap_or_default();
    f(&mut after);
    after.sanitize();
    open.history.execute(
        &mut open.doc,
        Box::new(TimelineEdit::new(label, before, Some(after))),
    );
}

/// Edição contínua (arrastar valores): aplica já e registra uma ação ao soltar.
fn live_edit(open: &mut OpenDoc, f: impl FnOnce(&mut Timeline)) {
    if open.tl_drag.is_none() {
        open.tl_drag = Some(open.doc.timeline.clone());
    }
    let mut t = open.doc.timeline.clone().unwrap_or_default();
    f(&mut t);
    t.sanitize();
    open.doc.timeline = Some(t);
    open.doc.touch();
}

fn end_live_edit(open: &mut OpenDoc) {
    if let Some(before) = open.tl_drag.take() {
        if before != open.doc.timeline {
            let after = open.doc.timeline.clone();
            open.history.record(
                &mut open.doc,
                Box::new(TimelineEdit::new("Editar animação", before, after)),
            );
        }
    }
}

impl OrvalithApp {
    pub(super) fn timeline_step(&mut self, delta: i32) {
        if !self.timeline.open {
            return;
        }
        if let Some(o) = self.docs.get_mut(self.active) {
            let total = o.doc.timeline.as_ref().map_or(1, Timeline::total) as i32;
            let t = (o.playhead as i32 + delta).rem_euclid(total);
            o.playhead = t as u32;
        }
    }

    /// Atualiza reprodução e o que o canvas deve mostrar (chamado a cada quadro da interface).
    pub(super) fn timeline_tick(&mut self, ctx: &egui::Context) {
        let ui_state = &mut self.timeline;
        let Some(open) = self.docs.get_mut(self.active) else {
            return;
        };
        let on = ui_state.open && ui_state.preview && open.doc.timeline.is_some();
        open.anim_frame = on.then_some(open.playhead);
        open.anim_onion = (on && ui_state.onion).then_some((
            ui_state.onion_before,
            ui_state.onion_after,
            (ui_state.onion_opacity * 100.0) as u32,
        ));
        if !ui_state.open || !ui_state.playing {
            ui_state.last_tick = None;
            ui_state.window_start = None;
            ui_state.shown = 0;
            return;
        }
        let Some(tl) = open.doc.timeline.as_ref() else {
            ui_state.playing = false;
            return;
        };
        let (fps, total) = (tl.fps, tl.total());
        let now = Instant::now();
        let last = ui_state.last_tick.replace(now);
        if let Some(prev) = last {
            ui_state.accum += (now - prev).as_secs_f32();
            let steps = (ui_state.accum * fps).floor() as u32;
            if steps > 0 {
                ui_state.accum -= steps as f32 / fps;
                let mut t = open.playhead + steps;
                if t >= total {
                    if ui_state.loop_play {
                        t %= total;
                    } else {
                        t = total - 1;
                        ui_state.playing = false;
                    }
                }
                open.playhead = t;
                ui_state.shown += 1;
            }
        }
        let ws = *ui_state.window_start.get_or_insert(now);
        if (now - ws).as_secs_f32() >= 1.0 {
            ui_state.achieved = Some(ui_state.shown as f32 / (now - ws).as_secs_f32());
            ui_state.shown = 0;
            ui_state.window_start = Some(now);
        }
        ctx.request_repaint_after(Duration::from_secs_f32(1.0 / fps.max(1.0)));
    }

    pub(super) fn timeline_panel(&mut self, root: &mut egui::Ui) {
        if !self.timeline.open {
            return;
        }
        let mut export_gif = false;
        let mut export_png = false;
        egui::Panel::bottom("timeline").resizable(true).show(root, |ui| {
            let Some(open) = self.docs.get_mut(self.active) else {
                ui.label("Abra um documento para animar.");
                return;
            };
            if open.doc.timeline.is_none() {
                ui.horizontal(|ui| {
                    ui.label("Este documento ainda não tem animação.");
                    if ui.button("Criar animação").clicked() {
                        edit_timeline(open, "Criar animação", |_| {});
                    }
                });
                return;
            }
            let tl = open.doc.timeline.clone().unwrap_or_default();
            let total = tl.total();
            let st = &mut self.timeline;
            ui.horizontal(|ui| {
                if ui.button("⏮").on_hover_text("Início").clicked() {
                    open.playhead = 0;
                }
                if ui.button("◀").on_hover_text("Quadro anterior (,)").clicked() {
                    open.playhead = (open.playhead + total - 1) % total;
                }
                let label = if st.playing { "⏸" } else { "▶" };
                if ui.button(label).on_hover_text("Reproduzir / pausar").clicked() {
                    st.playing = !st.playing;
                    st.achieved = None;
                    if st.playing && !st.preview {
                        st.preview = true;
                    }
                }
                if ui.button("▶|").on_hover_text("Próximo quadro (.)").clicked() {
                    open.playhead = (open.playhead + 1) % total;
                }
                ui.checkbox(&mut st.loop_play, "Repetir");
                ui.checkbox(&mut st.preview, "Prévia no canvas");
                ui.label(format!(
                    "{:.2} s / {:.2} s",
                    open.playhead as f32 / tl.fps,
                    tl.duration_secs()
                ));
            });
            if let Some(a) = st.achieved.filter(|_| st.playing) {
                if a < tl.fps * 0.9 {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!("A prévia mostra {a:.1} quadros/s; a taxa configurada é {:.0} (o computador não acompanha).", tl.fps),
                    );
                } else {
                    ui.small(format!("Prévia a {a:.1} quadros/s (alvo {:.0}).", tl.fps));
                }
            }
            let mut head = open.playhead as f32;
            if ui
                .add(egui::Slider::new(&mut head, 0.0..=(total - 1) as f32).integer().text("Quadro"))
                .changed()
            {
                open.playhead = head as u32;
            }
            ui.horizontal(|ui| {
                let mut fps = tl.fps;
                if ui.add(egui::DragValue::new(&mut fps).range(1.0..=120.0).suffix(" fps")).changed() {
                    live_edit(open, |t| t.fps = fps);
                }
                if tl.frames.is_empty() {
                    let mut len = tl.length;
                    if ui.add(egui::DragValue::new(&mut len).range(1..=anim::MAX_LENGTH).prefix("duração ").suffix(" quadros")).changed() {
                        live_edit(open, |t| t.length = len);
                    }
                } else {
                    ui.label(format!("duração {total} quadros (soma dos quadros)"));
                }
            });
            ui.separator();
            // ---- Quadro a quadro ----
            ui.label("Quadro a quadro (cada quadro guarda o estado visual das camadas)");
            ui.horizontal_wrapped(|ui| {
                let cur = tl.frame_index_at(open.playhead);
                for (i, f) in tl.frames.iter().enumerate() {
                    let text = format!("{} ({}×)", i + 1, f.hold);
                    if ui.selectable_label(cur == Some(i), text).clicked() {
                        open.playhead = tl.frame_start(i);
                    }
                }
            });
            let cur = tl.frame_index_at(open.playhead);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Novo quadro (estado atual)").clicked() {
                    let comp = comps::capture(&open.doc, &format!("Quadro {}", tl.frames.len() + 1));
                    edit_timeline(open, "Novo quadro", |t| t.frames.push(AnimFrame { comp, hold: 1 }));
                }
                if let Some(i) = cur {
                    if ui.button("Duplicar").clicked() {
                        edit_timeline(open, "Duplicar quadro", |t| {
                            let f = t.frames[i].clone();
                            t.frames.insert(i + 1, f);
                        });
                    }
                    if ui.button("Atualizar com o estado atual").clicked() {
                        let comp = comps::capture(&open.doc, &tl.frames[i].comp.name);
                        edit_timeline(open, "Atualizar quadro", |t| t.frames[i].comp = comp);
                    }
                    if ui.button("Excluir").clicked() {
                        edit_timeline(open, "Excluir quadro", |t| {
                            t.frames.remove(i);
                        });
                    }
                    let mut hold = tl.frames[i].hold;
                    if ui.add(egui::DragValue::new(&mut hold).range(1..=10_000).prefix("duração ").suffix(" quadros")).changed() {
                        live_edit(open, |t| t.frames[i].hold = hold);
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.onion, "Pele de cebola");
                if st.onion {
                    ui.add(egui::DragValue::new(&mut st.onion_before).range(0..=5).prefix("antes "));
                    ui.add(egui::DragValue::new(&mut st.onion_after).range(0..=5).prefix("depois "));
                    ui.add(egui::Slider::new(&mut st.onion_opacity, 0.05..=1.0).text("opacidade"));
                }
            });
            ui.separator();
            // ---- Quadros-chave da camada ativa ----
            let layer = open.active_idx().map(|i| (open.doc.layers[i].id.0, open.doc.layers[i].name.clone(), open.doc.layers[i].is_group()));
            match layer {
                None => {
                    ui.label("Selecione uma camada para animar.");
                }
                Some((lid, lname, is_group)) => {
                    ui.label(format!("Quadros-chave de \"{lname}\" no quadro {}", open.playhead));
                    if is_group {
                        ui.small("Em grupos só a opacidade é animada.");
                    }
                    let t = open.playhead;
                    for prop in Prop::ALL {
                        if is_group && prop != Prop::Opacity {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            let animated = tl.track(lid, prop).is_some();
                            let mut v = tl.value_at(lid, prop, t).unwrap_or(prop.neutral());
                            let speed = if matches!(prop, Prop::Opacity | Prop::ScaleX | Prop::ScaleY) { 0.01 } else { 0.5 };
                            ui.label(format!("{}{}", prop.label(), if animated { " ◆" } else { "" }));
                            if ui.add(egui::DragValue::new(&mut v).speed(speed)).changed() {
                                let ease = tl.key_at(lid, prop, t).map(|k| k.ease);
                                live_edit(open, |tl| tl.set_key(lid, prop, t, v, ease));
                            }
                            let key = tl.key_at(lid, prop, t);
                            if key.is_some() {
                                if ui.small_button("Remover chave").clicked() {
                                    edit_timeline(open, "Remover quadro-chave", |tl| {
                                        tl.remove_key(lid, prop, t);
                                    });
                                }
                                let mut e = key.map_or(Ease::Linear, |k| k.ease);
                                egui::ComboBox::from_id_salt(("ease", prop.label()))
                                    .selected_text(e.label())
                                    .width(110.0)
                                    .show_ui(ui, |ui| {
                                        for x in Ease::ALL {
                                            ui.selectable_value(&mut e, x, x.label());
                                        }
                                    });
                                if Some(e) != key.map(|k| k.ease) {
                                    edit_timeline(open, "Interpolação", |tl| tl.set_key(lid, prop, t, v, Some(e)));
                                }
                            } else if ui.small_button("◆ Adicionar chave").clicked() {
                                edit_timeline(open, "Quadro-chave", |tl| tl.set_key(lid, prop, t, v, None));
                            }
                            if let Some(track) = tl.track(lid, prop) {
                                for k in &track.keys {
                                    if ui.small_button(format!("{}", k.frame)).on_hover_text("Ir para este quadro-chave").clicked() {
                                        open.playhead = k.frame.min(total - 1);
                                    }
                                }
                            }
                        });
                    }
                    ui.small("Transformações agem em camadas raster em torno do centro do conteúdo; a máscara não acompanha o movimento.");
                }
            }
            if !ui.input(|i| i.pointer.any_down()) && !ui.ctx().egui_wants_keyboard_input() {
                end_live_edit(open);
            }
            ui.separator();
            // ---- Exportação ----
            ui.horizontal_wrapped(|ui| {
                let mut lc = tl.loop_count;
                if ui.add(egui::DragValue::new(&mut lc).range(0..=65535).prefix("repetições (0 = infinito) ")).changed() {
                    live_edit(open, |t| t.loop_count = lc);
                }
                ui.add(egui::Slider::new(&mut st.gif_speed, 1..=30).text("velocidade da quantização"));
                ui.add(egui::DragValue::new(&mut st.gif_threshold).range(1..=255).prefix("alfa mínimo "));
                ui.checkbox(&mut st.gif_bg, "Fundo sólido");
                if st.gif_bg {
                    ui.color_edit_button_srgb(&mut st.gif_bg_color);
                }
                export_gif = ui.button("Exportar GIF…").clicked();
                export_png = ui.button("Exportar sequência PNG…").clicked();
            });
        });
        if export_gif {
            self.timeline_export_gif();
        }
        if export_png {
            self.timeline_export_png();
        }
    }

    fn timeline_export_gif(&mut self) {
        let Some(open) = self.docs.get(self.active) else {
            return;
        };
        let Some(tl) = open.doc.timeline.clone() else {
            return;
        };
        let st = &self.timeline;
        let opts = GifOpts {
            speed: st.gif_speed,
            alpha_threshold: st.gif_threshold,
            background: st.gif_bg.then_some(st.gif_bg_color),
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("GIF animado", &["gif"])
            .set_file_name(format!("{}.gif", open.doc.name))
            .save_file()
        else {
            return;
        };
        let path = super::io::with_extension(path, "gif");
        match anim::export_gif(&open.doc, &tl, &opts).and_then(|g| {
            fsio::atomic_write(&path, &g.bytes).map_err(|e| e.to_string())?;
            Ok(g)
        }) {
            Ok(g) => self.show_message(
                "GIF exportado",
                format!(
                    "{}\n\n{} quadro(s) gravado(s) para {:.2} s a {:.0} fps ({}).\n- {}",
                    path.display(),
                    g.frames,
                    tl.duration_secs(),
                    tl.fps,
                    if tl.loop_count == 0 {
                        "repetição infinita".to_string()
                    } else {
                        format!("{} repetição(ões)", tl.loop_count)
                    },
                    g.warnings.join("\n- ")
                ),
            ),
            Err(e) => self.show_message(
                "Erro ao exportar o GIF",
                format!("{}\n\n{e}", path.display()),
            ),
        }
    }

    fn timeline_export_png(&mut self) {
        let Some(open) = self.docs.get(self.active) else {
            return;
        };
        let Some(tl) = open.doc.timeline.clone() else {
            return;
        };
        let Some(dir) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        let seq = match anim::png_sequence(
            &open.doc,
            &tl,
            &open
                .doc
                .name
                .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_"),
        ) {
            Ok(s) => s,
            Err(e) => return self.show_message("Erro ao exportar a sequência", e),
        };
        let mut written = 0;
        let mut errors = Vec::new();
        for (name, bytes) in &seq {
            let target = dir.join(name);
            let target = if target.exists() {
                // Nunca sobrescreve: acrescenta " (n)".
                let stem = name.trim_end_matches(".png");
                Self::free_path(&dir, stem, "png")
            } else {
                target
            };
            match fsio::atomic_write(&target, bytes) {
                Ok(()) => written += 1,
                Err(e) => errors.push(format!("{name}: {e}")),
            }
        }
        let mut text = format!(
            "{written} de {} quadro(s) gravado(s) em {} ({:.0} fps).",
            seq.len(),
            dir.display(),
            tl.fps
        );
        if !errors.is_empty() {
            text.push_str("\n\nFalhas:\n- ");
            text.push_str(&errors.join("\n- "));
        }
        self.show_message("Sequência PNG exportada", text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::Document;

    fn open() -> OpenDoc {
        let mut d = Document::new("t", 10, 10, Some([0, 0, 0, 255])).unwrap();
        d.layers
            .push(orvalith_core::Layer::filled("L", 10, 10, [200, 0, 0, 255]));
        OpenDoc::new(d)
    }

    #[test]
    fn edits_are_undoable_and_live_edits_coalesce_into_one_action() {
        let mut o = open();
        edit_timeline(&mut o, "Criar animação", |_| {});
        assert!(o.doc.timeline.is_some());
        let n = o.history.labels().len();
        let id = o.doc.layers[1].id.0;
        live_edit(&mut o, |t| t.set_key(id, Prop::X, 0, 1.0, None));
        live_edit(&mut o, |t| t.set_key(id, Prop::X, 0, 5.0, None));
        live_edit(&mut o, |t| t.set_key(id, Prop::X, 0, 9.0, None));
        end_live_edit(&mut o);
        assert_eq!(o.history.labels().len(), n + 1, "arrastar = uma ação");
        assert_eq!(
            o.doc.timeline.as_ref().unwrap().value_at(id, Prop::X, 0),
            Some(9.0)
        );
        o.history.undo(&mut o.doc);
        assert!(o.doc.timeline.as_ref().unwrap().tracks.is_empty());
        edit_timeline(&mut o, "Novo quadro", |t| {
            t.frames.push(AnimFrame {
                comp: comps::capture(&o_doc_stub(), "a"),
                hold: 2,
            })
        });
        assert_eq!(o.doc.timeline.as_ref().unwrap().total(), 2);
        end_live_edit(&mut o); // sem edição pendente: nada acontece
    }

    fn o_doc_stub() -> Document {
        Document::new("s", 2, 2, None).unwrap()
    }
}
