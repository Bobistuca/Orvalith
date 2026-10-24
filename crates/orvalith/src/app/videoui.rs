//! Janela de vídeo: trilhas, clipes (aparar/mover/cortar arrastando), títulos e imagens por cima,
//! transições, áudio, prévia por quadro, mídia ausente e exportação com progresso e cancelamento.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use eframe::egui;
use orvalith_core::media::{Ffmpeg, FrameCache};
use orvalith_core::video::{
    self, compose_frame, Clip, ClipSource, Codec, ExportError, ExportSettings, PreviewProvider,
    TrackKind, VideoEdit, VideoProject,
};

use super::OrvalithApp;
use crate::canvas::OpenDoc;

const ROW_H: f32 = 34.0;
const EDGE: f32 = 6.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone {
    Move,
    TrimStart,
    TrimEnd,
}

struct Drag {
    id: u64,
    zone: Zone,
    orig: VideoProject,
    press_x: f32,
}

struct Job {
    rx: Receiver<Result<String, String>>,
    progress: Arc<AtomicU32>,
    total: u32,
    cancel: Arc<AtomicBool>,
}

pub(super) struct VideoUi {
    pub open: bool,
    ffmpeg: Option<Result<Ffmpeg, String>>,
    cache: FrameCache,
    playhead: u32,
    selected: Option<u64>,
    drag: Option<Drag>,
    ppf: f32,
    preview: Option<((u64, u32), egui::TextureHandle)>,
    job: Option<Job>,
    exp_w: u32,
    exp_h: u32,
    exp_fps: u32,
    codec: usize,
    crf: u32,
    still_secs: f32,
}

impl Default for VideoUi {
    fn default() -> Self {
        Self {
            open: false,
            ffmpeg: None,
            cache: FrameCache::new(48),
            playhead: 0,
            selected: None,
            drag: None,
            ppf: 3.0,
            preview: None,
            job: None,
            exp_w: 1280,
            exp_h: 720,
            exp_fps: 30,
            codec: 0,
            crf: 23,
            still_secs: 3.0,
        }
    }
}

/// Edição contínua do projeto de vídeo: aplica já e registra uma ação ao concluir.
pub(super) fn vid_live(open: &mut OpenDoc, f: impl FnOnce(&mut VideoProject)) {
    if open.vid_drag.is_none() {
        open.vid_drag = Some(open.doc.video.clone());
    }
    let mut v = open.doc.video.clone().unwrap_or_default();
    f(&mut v);
    v.sanitize();
    open.doc.video = Some(v);
    open.doc.touch();
}

pub(super) fn vid_end(open: &mut OpenDoc) {
    if let Some(before) = open.vid_drag.take() {
        if before != open.doc.video {
            let after = open.doc.video.clone();
            open.history.record(
                &mut open.doc,
                Box::new(VideoEdit::new("Editar vídeo", before, after)),
            );
        }
    }
}

/// Edição pontual (uma ação de desfazer).
pub(super) fn vid_edit(open: &mut OpenDoc, label: &str, f: impl FnOnce(&mut VideoProject)) {
    let before = open.doc.video.clone();
    let mut after = before.clone().unwrap_or_default();
    f(&mut after);
    after.sanitize();
    open.history.execute(
        &mut open.doc,
        Box::new(VideoEdit::new(label, before, Some(after))),
    );
}

/// Probe + inserção de um arquivo de mídia na primeira trilha adequada, ao final dela (ou no cabeçote).
pub(super) fn add_media(
    open: &mut OpenDoc,
    ff: &Ffmpeg,
    path: &Path,
    at: Option<u32>,
) -> Result<(), String> {
    let info = ff.probe(path)?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Mídia".into());
    let p = path.to_string_lossy().into_owned();
    let has_video = info.video.is_some();
    let mut err = None;
    vid_edit(open, "Adicionar mídia", |v| {
        let kind = if has_video {
            TrackKind::Video
        } else {
            TrackKind::Audio
        };
        // Primeira trilha do tipo certo; vídeo vai sempre para a trilha de baixo.
        let Some(ti) = v.tracks.iter().position(|t| t.kind == kind) else {
            err = Some("Não há trilha adequada.".to_string());
            return;
        };
        let start =
            at.unwrap_or_else(|| v.tracks[ti].clips.iter().map(Clip::end).max().unwrap_or(0));
        let first = v.tracks.iter().all(|t| t.clips.is_empty());
        // O primeiro vídeo define a resolução e a taxa do projeto.
        if has_video && first {
            if let Some(vs) = &info.video {
                v.width = (vs.width & !1).max(16);
                v.height = (vs.height & !1).max(16);
                v.fps_num = vs.fps.0;
                v.fps_den = vs.fps.1;
            }
        }
        let clip = v.media_clip(&p, &name, &info, start);
        if let Err(e) = v.add_clip(ti, clip) {
            err = Some(e);
        }
    });
    match err {
        Some(e) => {
            // Desfaz a ação vazia criada acima.
            open.history.undo(&mut open.doc);
            Err(e)
        }
        None => Ok(()),
    }
}

/// Pixels do documento composto como clipe de título/imagem na trilha 2 (ou a de vídeo mais alta).
pub(super) fn add_document_as_still(open: &mut OpenDoc, at: u32, secs: f64) {
    let (w, h) = (open.doc.width, open.doc.height);
    let px = orvalith_core::compose::flatten(&open.doc);
    let name = open.doc.name.clone();
    vid_edit(open, "Adicionar título/imagem", |v| {
        let ti = v
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == TrackKind::Video)
            .map(|(i, _)| i)
            .nth(1)
            .or_else(|| v.tracks.iter().position(|t| t.kind == TrackKind::Video))
            .unwrap_or(0);
        let c = v.still_clip(&name, w, h, px, at, secs);
        let _ = v.add_clip(ti, c);
    });
}

fn px_to_frame(x: f32, left: f32, ppf: f32) -> u32 {
    ((x - left) / ppf).round().max(0.0) as u32
}

impl VideoUi {
    pub(super) fn set_cache(&mut self, n: usize) {
        self.cache.set_cap(n);
    }
}

impl OrvalithApp {
    fn ensure_ffmpeg(&mut self) {
        if self.video.ffmpeg.is_none() {
            self.video.ffmpeg = Some(Ffmpeg::detect(None));
        }
    }

    /// Recolhe o resultado de uma exportação em andamento.
    pub(super) fn video_poll(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.video.job else { return };
        match job.rx.try_recv() {
            Ok(res) => {
                self.video.job = None;
                match res {
                    Ok(text) => self.show_message("Vídeo exportado", text),
                    Err(e) => self.show_message("Exportação do vídeo", e),
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100))
            }
            Err(mpsc::TryRecvError::Disconnected) => self.video.job = None,
        }
    }

    pub(super) fn video_window(&mut self, ctx: &egui::Context) {
        if !self.video.open {
            return;
        }
        self.ensure_ffmpeg();
        let mut is_open = true;
        let mut export_clicked = false;
        let mut add_media_clicked = false;
        let mut add_doc_clicked = false;
        let mut relink: Option<String> = None;
        egui::Window::new("Vídeo")
            .open(&mut is_open)
            .default_width(760.0)
            .show(ctx, |ui| {
                let ff = self.video.ffmpeg.clone().and_then(Result::ok);
                match (&ff, &self.video.ffmpeg) {
                    (Some(f), _) => {
                        ui.small(f.summary());
                    }
                    (None, Some(Err(e))) => {
                        ui.colored_label(ui.visuals().warn_fg_color, e);
                        ui.small("O Orvalith não inclui o FFmpeg: instale-o separadamente (veja docs/MEDIA.md). Sem ele não há importação, prévia nem exportação de vídeo.");
                    }
                    _ => {}
                }
                let Some(open) = self.docs.get_mut(self.active) else {
                    ui.label("Abra um documento para criar o projeto de vídeo.");
                    return;
                };
                if open.doc.video.is_none() {
                    if ui.button("Criar projeto de vídeo neste documento").clicked() {
                        vid_edit(open, "Criar projeto de vídeo", |_| {});
                    }
                    return;
                }
                let v = open.doc.video.clone().unwrap_or_default();
                let total = v.total_frames();
                ui.horizontal(|ui| {
                    let (mut w, mut h, mut fn_, mut fd) = (v.width, v.height, v.fps_num, v.fps_den);
                    let mut ch = false;
                    ch |= ui.add(egui::DragValue::new(&mut w).range(16..=7680).prefix("L ")).changed();
                    ch |= ui.add(egui::DragValue::new(&mut h).range(16..=4320).prefix("A ")).changed();
                    ch |= ui.add(egui::DragValue::new(&mut fn_).range(1..=240).prefix("fps ")).changed();
                    ch |= ui.add(egui::DragValue::new(&mut fd).range(1..=1001).prefix("/ ")).changed();
                    if ch {
                        vid_live(open, |p| {
                            p.width = w;
                            p.height = h;
                            p.fps_num = fn_;
                            p.fps_den = fd;
                        });
                    }
                    ui.label(format!("{:.2} fps · {} quadros · {:.2} s", v.fps(), total, v.duration_secs()));
                });
                ui.horizontal_wrapped(|ui| {
                    add_media_clicked = ui.add_enabled(ff.is_some(), egui::Button::new("Adicionar mídia…")).clicked();
                    add_doc_clicked = ui.button("Documento atual como título/imagem").clicked();
                    ui.add(egui::DragValue::new(&mut self.video.still_secs).range(0.1..=600.0).suffix(" s"));
                    if let Some(id) = self.video.selected {
                        let head = self.video.playhead;
                        if ui.button("Cortar no cabeçote").clicked() {
                            vid_edit(open, "Cortar clipe", |p| {
                                p.split(id, head);
                            });
                        }
                        if ui.button("Excluir clipe").clicked() {
                            vid_edit(open, "Excluir clipe", |p| {
                                p.remove_clip(id);
                            });
                            self.video.selected = None;
                        }
                    }
                });
                // ---- Linha do tempo ----
                let ppf = self.video.ppf;
                let width = ((total + 90) as f32 * ppf).max(500.0);
                let rows = v.tracks.len();
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    let (rect, bg) = ui.allocate_exact_size(egui::vec2(width, 18.0 + rows as f32 * (ROW_H + 4.0)), egui::Sense::click());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
                    let fps = v.fps() as f32;
                    let step = (fps.round().max(1.0) * (60.0 / (fps.max(1.0) * ppf)).ceil().max(1.0)) as u32;
                    let mut f = 0;
                    while f <= total + 90 {
                        let x = rect.left() + f as f32 * ppf;
                        painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.top() + 10.0)], egui::Stroke::new(1.0, ui.visuals().weak_text_color()));
                        painter.text(egui::pos2(x + 2.0, rect.top()), egui::Align2::LEFT_TOP, format!("{:.0}s", f as f32 / fps), egui::FontId::proportional(9.0), ui.visuals().weak_text_color());
                        f += step.max(1);
                    }
                    if bg.clicked() {
                        if let Some(pos) = bg.interact_pointer_pos() {
                            self.video.playhead = px_to_frame(pos.x, rect.left(), ppf).min(total.max(1));
                        }
                    }
                    for (ti, t) in v.tracks.iter().enumerate() {
                        let top = rect.top() + 16.0 + ti as f32 * (ROW_H + 4.0);
                        painter.rect_filled(egui::Rect::from_min_size(egui::pos2(rect.left(), top), egui::vec2(width, ROW_H)), 2.0, ui.visuals().faint_bg_color);
                        painter.text(egui::pos2(rect.left() + 3.0, top + 1.0), egui::Align2::LEFT_TOP, format!("{}{}", t.name, if t.mute { " (mudo)" } else { "" }), egui::FontId::proportional(9.0), ui.visuals().weak_text_color());
                        for c in &t.clips {
                            let cr = egui::Rect::from_min_size(egui::pos2(rect.left() + c.start as f32 * ppf, top + 10.0), egui::vec2((c.duration as f32 * ppf).max(3.0), ROW_H - 12.0));
                            let id = ui.scope_id().with(("clip", c.id));
                            let resp = ui.interact(cr, id, egui::Sense::click_and_drag());
                            let sel = self.video.selected == Some(c.id);
                            let missing = c.path().is_some_and(|p| !Path::new(p).exists());
                            let col = match (&c.source, missing) {
                                (_, true) => egui::Color32::from_rgb(170, 60, 60),
                                (ClipSource::Still { .. }, _) => egui::Color32::from_rgb(190, 130, 40),
                                (_, _) if t.kind == TrackKind::Audio => egui::Color32::from_rgb(50, 140, 90),
                                _ => egui::Color32::from_rgb(60, 110, 190),
                            };
                            painter.rect_filled(cr, 3.0, col);
                            if sel {
                                painter.rect_stroke(cr, 3.0, egui::Stroke::new(2.0, egui::Color32::WHITE), egui::StrokeKind::Inside);
                            }
                            painter.text(cr.left_center() + egui::vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, &c.name, egui::FontId::proportional(10.0), egui::Color32::WHITE);
                            // Rampas (fade/dissolver) como triângulos.
                            let (fi, fo) = (c.fade_in as f32 * ppf, c.fade_out as f32 * ppf);
                            if fi > 1.0 {
                                painter.add(egui::Shape::convex_polygon(vec![cr.left_bottom(), cr.left_top(), egui::pos2(cr.left() + fi, cr.bottom())], egui::Color32::from_black_alpha(90), egui::Stroke::NONE));
                            }
                            if fo > 1.0 {
                                painter.add(egui::Shape::convex_polygon(vec![cr.right_bottom(), cr.right_top(), egui::pos2(cr.right() - fo, cr.bottom())], egui::Color32::from_black_alpha(90), egui::Stroke::NONE));
                            }
                            if let Some(pos) = resp.hover_pos() {
                                if pos.x - cr.left() < EDGE || cr.right() - pos.x < EDGE {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                                } else {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                                }
                            }
                            if resp.clicked() {
                                self.video.selected = Some(c.id);
                                if let Some(pos) = resp.interact_pointer_pos() {
                                    self.video.playhead = px_to_frame(pos.x, rect.left(), ppf);
                                }
                            }
                            if resp.drag_started() {
                                if let Some(pos) = resp.interact_pointer_pos() {
                                    let zone = if pos.x - cr.left() < EDGE {
                                        Zone::TrimStart
                                    } else if cr.right() - pos.x < EDGE {
                                        Zone::TrimEnd
                                    } else {
                                        Zone::Move
                                    };
                                    self.video.selected = Some(c.id);
                                    self.video.drag = Some(Drag {
                                        id: c.id,
                                        zone,
                                        orig: v.clone(),
                                        press_x: pos.x,
                                    });
                                }
                            }
                            if resp.dragged() {
                                if let (Some(d), Some(pos)) = (&self.video.drag, resp.interact_pointer_pos()) {
                                    if d.id == c.id {
                                        let df = ((pos.x - d.press_x) / ppf).round() as i64;
                                        let mut np = d.orig.clone();
                                        let oc = d.orig.clip(c.id).cloned();
                                        if let Some(oc) = oc {
                                            let at = |base: u32| (i64::from(base) + df).max(0) as u32;
                                            match d.zone {
                                                Zone::Move => {
                                                    np.move_clip(c.id, at(oc.start));
                                                }
                                                Zone::TrimStart => {
                                                    np.trim_start(c.id, at(oc.start));
                                                }
                                                Zone::TrimEnd => {
                                                    np.trim_end(c.id, at(oc.end()));
                                                }
                                            }
                                        }
                                        np.sanitize();
                                        if open.vid_drag.is_none() {
                                            open.vid_drag = Some(Some(d.orig.clone()));
                                        }
                                        open.doc.video = Some(np);
                                        open.doc.touch();
                                    }
                                }
                            }
                            if resp.drag_stopped() {
                                self.video.drag = None;
                                vid_end(open);
                            }
                        }
                    }
                    let hx = rect.left() + self.video.playhead as f32 * ppf;
                    painter.line_segment([egui::pos2(hx, rect.top()), egui::pos2(hx, rect.bottom())], egui::Stroke::new(1.5, egui::Color32::from_rgb(240, 80, 80)));
                });
                ui.horizontal(|ui| {
                    ui.add(egui::Slider::new(&mut self.video.ppf, 0.5..=12.0).text("zoom"));
                    let mut head = self.video.playhead as f32;
                    if ui.add(egui::Slider::new(&mut head, 0.0..=total.max(1) as f32).integer().text("quadro")).changed() {
                        self.video.playhead = head as u32;
                    }
                    ui.label(format!("{:.2} s", v.frame_to_secs(self.video.playhead)));
                });
                // ---- Trilhas ----
                ui.horizontal_wrapped(|ui| {
                    for (i, t) in v.tracks.iter().enumerate() {
                        let mut mute = t.mute;
                        if ui.checkbox(&mut mute, format!("Mudo: {}", t.name)).changed() {
                            vid_edit(open, "Silenciar trilha", |p| p.tracks[i].mute = mute);
                        }
                        let mut vol = t.volume;
                        if ui.add(egui::DragValue::new(&mut vol).range(0.0..=4.0).speed(0.01).prefix("vol ")).changed() {
                            vid_live(open, |p| p.tracks[i].volume = vol);
                        }
                    }
                });
                // ---- Clipe selecionado ----
                if let Some(c) = self.video.selected.and_then(|id| v.clip(id)).cloned() {
                    ui.separator();
                    ui.label(format!("Clipe: {} — início {} · duração {} quadros · entrada na mídia {:.2} s", c.name, c.start, c.duration, c.in_point));
                    ui.horizontal_wrapped(|ui| {
                        let (mut vol, mut op, mut x, mut y, mut sc) = (c.volume, c.opacity, c.x, c.y, c.scale);
                        let mut ch = false;
                        if c.has_audio() {
                            ch |= ui.add(egui::DragValue::new(&mut vol).range(0.0..=4.0).speed(0.01).prefix("volume ")).changed();
                        }
                        if c.has_video() {
                            ch |= ui.add(egui::DragValue::new(&mut op).range(0.0..=1.0).speed(0.01).prefix("opacidade ")).changed();
                            ch |= ui.add(egui::DragValue::new(&mut x).prefix("X ")).changed();
                            ch |= ui.add(egui::DragValue::new(&mut y).prefix("Y ")).changed();
                            ch |= ui.add(egui::DragValue::new(&mut sc).range(0.01..=50.0).speed(0.01).prefix("escala ")).changed();
                        }
                        if ch {
                            vid_live(open, |p| {
                                for cl in p.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).filter(|k| k.id == c.id) {
                                    cl.volume = vol;
                                    cl.opacity = op;
                                    cl.x = x;
                                    cl.y = y;
                                    cl.scale = sc;
                                }
                            });
                        }
                        let (mut fi, mut fo) = (c.fade_in, c.fade_out);
                        let mut fch = ui.add(egui::DragValue::new(&mut fi).range(0..=c.duration).prefix("fade entrada ")).changed();
                        fch |= ui.add(egui::DragValue::new(&mut fo).range(0..=c.duration).prefix("fade saída ")).changed();
                        if fch {
                            vid_live(open, |p| {
                                p.set_fade(c.id, fi, fo);
                            });
                        }
                        if ui.button("Dissolver com o anterior").on_hover_text("Sobrepõe o início deste clipe ao fim do anterior (10 quadros) com fade").clicked() {
                            let id = c.id;
                            let mut res = Ok(());
                            vid_edit(open, "Dissolver", |p| res = p.dissolve_with_previous(id, 10));
                            if let Err(e) = res {
                                self.tools.notice = Some(e);
                                open.history.undo(&mut open.doc);
                            }
                        }
                    });
                }
                if !ui.input(|i| i.pointer.any_down()) && !ui.ctx().egui_wants_keyboard_input() {
                    vid_end(open);
                }
                // ---- Mídia ausente ----
                let missing = v.missing_media();
                if !missing.is_empty() {
                    ui.separator();
                    ui.colored_label(ui.visuals().warn_fg_color, "Mídia ausente (o projeto guarda caminhos relativos; localize os arquivos):");
                    for m in &missing {
                        ui.horizontal(|ui| {
                            ui.label(m);
                            if ui.button("Localizar…").clicked() {
                                relink = Some(m.clone());
                            }
                        });
                    }
                }
                // ---- Prévia ----
                if let (Some(f), true) = (ff.as_ref(), missing.is_empty()) {
                    let key = (open.doc.revision, self.video.playhead);
                    if self.video.preview.as_ref().map(|p| p.0) != Some(key) && total > 0 {
                        let pw = 400u32.min(v.width);
                        let small = VideoProject {
                            width: (pw & !1).max(16),
                            height: ((pw * v.height / v.width.max(1)) & !1).max(16),
                            ..v.clone()
                        };
                        let mut prov = PreviewProvider::new(f, &mut self.video.cache);
                        let px = compose_frame(&small, self.video.playhead.min(total - 1), &mut prov);
                        let img = egui::ColorImage::from_rgba_unmultiplied([small.width as usize, small.height as usize], &px);
                        let tex = ui.ctx().load_texture("video_preview", img, egui::TextureOptions::LINEAR);
                        self.video.preview = Some((key, tex));
                    }
                    if let Some((_, t)) = &self.video.preview {
                        ui.add(egui::Image::new(t).max_width(400.0));
                    }
                    ui.small("Prévia por quadro (sem áudio em tempo real); o áudio é mixado na exportação.");
                }
                // ---- Exportação ----
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::DragValue::new(&mut self.video.exp_w).range(16..=7680).prefix("L "));
                    ui.add(egui::DragValue::new(&mut self.video.exp_h).range(16..=4320).prefix("A "));
                    ui.add(egui::DragValue::new(&mut self.video.exp_fps).range(1..=120).suffix(" fps"));
                    egui::ComboBox::from_id_salt("vcodec")
                        .selected_text(["MP4 (H.264 + AAC)", "WebM (VP9 + Opus)"][self.video.codec.min(1)])
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.video.codec, 0, "MP4 (H.264 + AAC)");
                            ui.selectable_value(&mut self.video.codec, 1, "WebM (VP9 + Opus)");
                        });
                    ui.add(egui::Slider::new(&mut self.video.crf, 10..=45).text("CRF (menor = melhor)"));
                    if let Some(j) = &self.video.job {
                        let done = j.progress.load(Ordering::Relaxed);
                        ui.add(egui::ProgressBar::new(done as f32 / j.total.max(1) as f32).text(format!("{done}/{}", j.total)).desired_width(220.0));
                        if ui.button("Cancelar").clicked() {
                            j.cancel.store(true, Ordering::Relaxed);
                        }
                    } else {
                        export_clicked = ui.add_enabled(ff.is_some() && total > 0, egui::Button::new("Exportar…")).clicked();
                    }
                });
            });
        self.video.open = is_open;
        if add_media_clicked {
            if let (Some(Ok(ff)), Some(path)) = (
                self.video.ffmpeg.clone(),
                rfd::FileDialog::new()
                    .add_filter(
                        "Vídeo e áudio",
                        &[
                            "mp4", "mov", "mkv", "webm", "avi", "m4v", "mp3", "wav", "flac", "ogg",
                            "m4a", "aac", "opus",
                        ],
                    )
                    .pick_file(),
            ) {
                if let Some(open) = self.docs.get_mut(self.active) {
                    let at = None;
                    if let Err(e) = add_media(open, &ff, &path, at) {
                        self.show_message(
                            "Não foi possível adicionar a mídia",
                            format!("{}\n\n{e}", path.display()),
                        );
                    }
                }
            }
        }
        if add_doc_clicked {
            let secs = f64::from(self.video.still_secs);
            let head = self.video.playhead;
            if let Some(open) = self.docs.get_mut(self.active) {
                add_document_as_still(open, head, secs);
            }
        }
        if let Some(old) = relink {
            if let (Some(Ok(ff)), Some(path)) = (
                self.video.ffmpeg.clone(),
                rfd::FileDialog::new().pick_file(),
            ) {
                match ff.probe(&path) {
                    Ok(info) => {
                        if let Some(open) = self.docs.get_mut(self.active) {
                            vid_edit(open, "Localizar mídia", |p| {
                                p.relink(&old, &path.to_string_lossy(), &info);
                            });
                        }
                    }
                    Err(e) => self.show_message("Arquivo inválido", e),
                }
            }
        }
        if export_clicked {
            self.video_start_export();
        }
    }

    fn video_start_export(&mut self) {
        let Some(Ok(ff)) = self.video.ffmpeg.clone() else {
            return;
        };
        let Some(open) = self.docs.get(self.active) else {
            return;
        };
        let Some(project) = open.doc.video.clone() else {
            return;
        };
        let codec = if self.video.codec == 0 {
            Codec::H264Aac
        } else {
            Codec::Vp9Opus
        };
        if !codec.available(&ff) {
            return self.show_message(
                "Codificador indisponível",
                format!(
                    "O FFmpeg instalado não tem o codificador para {}.",
                    codec.extension()
                ),
            );
        }
        let Some(out) = rfd::FileDialog::new()
            .add_filter(
                if codec == Codec::H264Aac {
                    "MP4"
                } else {
                    "WebM"
                },
                &[codec.extension()],
            )
            .set_file_name(format!("{}.{}", open.doc.name, codec.extension()))
            .save_file()
        else {
            return;
        };
        let out: PathBuf = super::io::with_extension(out, codec.extension());
        let settings = ExportSettings {
            width: self.video.exp_w & !1,
            height: self.video.exp_h & !1,
            fps_num: self.video.exp_fps,
            fps_den: 1,
            codec,
            crf: self.video.crf,
        };
        let total = (f64::from(project.total_frames())
            * (f64::from(settings.fps_num) / project.fps()))
        .round() as u32;
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let (p2, c2) = (progress.clone(), cancel.clone());
        std::thread::spawn(move || {
            let res = video::export(
                &ff,
                &project,
                &settings,
                &out,
                &mut |f, _| p2.store(f, Ordering::Relaxed),
                &c2,
            );
            let msg = match res {
                Ok(rep) => {
                    // Verificação independente do arquivo gerado com o ffprobe.
                    let check = match ff.probe(&out) {
                        Ok(i) => {
                            let v = i.video.as_ref();
                            format!(
                                "ffprobe: {:.2} s, {}×{}, {:.2} fps, áudio: {}",
                                i.duration_s,
                                v.map_or(0, |v| v.width),
                                v.map_or(0, |v| v.height),
                                v.map_or(0.0, |v| v.fps_f64()),
                                i.audio.as_ref().map_or("não".to_string(), |a| format!(
                                    "{} {} Hz",
                                    a.codec, a.sample_rate
                                ))
                            )
                        }
                        Err(e) => format!("ffprobe não pôde verificar: {e}"),
                    };
                    Ok(format!(
                        "{}\n\n{} quadros ({:.2} s). {check}",
                        out.display(),
                        rep.frames,
                        rep.duration_s
                    ))
                }
                Err(ExportError::Cancelled) => {
                    Err("Exportação cancelada; o arquivo parcial foi removido.".to_string())
                }
                Err(ExportError::Failed(m)) => Err(m),
            };
            let _ = tx.send(msg);
        });
        self.video.job = Some(Job {
            rx,
            progress,
            total,
            cancel,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::{media::Ffmpeg, Document};

    fn open() -> OpenDoc {
        let mut d = Document::new("v", 64, 36, Some([10, 20, 30, 255])).unwrap();
        d.video = Some(VideoProject {
            width: 64,
            height: 36,
            fps_num: 10,
            fps_den: 1,
            ..VideoProject::default()
        });
        OpenDoc::new(d)
    }

    #[test]
    fn live_edits_coalesce_and_documents_become_title_clips() {
        let mut o = open();
        let n = o.history.labels().len();
        vid_live(&mut o, |p| p.tracks[0].volume = 0.5);
        vid_live(&mut o, |p| p.tracks[0].volume = 0.2);
        vid_end(&mut o);
        assert_eq!(o.history.labels().len(), n + 1);
        assert_eq!(o.doc.video.as_ref().unwrap().tracks[0].volume, 0.2);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.video.as_ref().unwrap().tracks[0].volume, 1.0);
        add_document_as_still(&mut o, 5, 2.0);
        let v = o.doc.video.as_ref().unwrap();
        let c = &v.tracks[1].clips[0];
        assert_eq!(
            (c.start, c.duration),
            (5, 20),
            "título na trilha de cima, 2 s a 10 fps"
        );
        assert!(matches!(
            &c.source,
            ClipSource::Still {
                w: 64,
                h: 36,
                pixels: Some(_)
            }
        ));
        assert_eq!(px_to_frame(110.0, 10.0, 4.0), 25);
        assert_eq!(px_to_frame(0.0, 10.0, 4.0), 0);
    }

    #[test]
    fn adding_real_media_probes_it_and_takes_the_first_clip_format() {
        let Ok(ff) = Ffmpeg::detect(None) else { return };
        let dir = std::env::temp_dir().join(format!("orvalith_vui_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let clip = dir.join("c.mp4");
        let out = ff
            .command()
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=160x90:r=25:d=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=2",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ])
            .arg(&clip)
            .output()
            .unwrap();
        assert!(out.status.success());
        let mut o = open();
        add_media(&mut o, &ff, &clip, None).unwrap();
        let v = o.doc.video.as_ref().unwrap();
        assert_eq!(
            (v.width, v.height, v.fps_num),
            (160, 90, 25),
            "primeiro vídeo define o formato"
        );
        let c = &v.tracks[0].clips[0];
        assert_eq!(c.duration, 50, "2 s a 25 fps");
        assert!(c.has_audio());
        // Arquivo inválido: erro claro e nenhuma ação deixada no histórico.
        let n = o.history.labels().len();
        let junk = dir.join("lixo.mp4");
        std::fs::write(&junk, b"nao e video").unwrap();
        assert!(add_media(&mut o, &ff, &junk, None).is_err());
        assert_eq!(o.history.labels().len(), n);
        let _ = std::fs::remove_dir_all(dir);
    }
}
