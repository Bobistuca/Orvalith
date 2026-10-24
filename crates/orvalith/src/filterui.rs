//! Interface dos filtros: janela com prévia calculada em segundo plano e cancelável.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

use eframe::egui;
use orvalith_core::adjops::destructive_blocked;
use orvalith_core::filter::{self, Filter, KINDS};
use orvalith_core::geom::Rect as PixRect;
use orvalith_core::LayerId;

use crate::canvas::OpenDoc;

struct Job {
    rx: Receiver<Option<Vec<u8>>>,
    cancel: Arc<AtomicBool>,
    filter: Filter,
}

pub struct FilterSession {
    pub applied: bool,
    layer: LayerId,
    original: Arc<Vec<u8>>,
    pub filter: Filter,
    /// Revisão logo após a última prévia; mudança externa encerra a sessão.
    rev: u64,
    was_modified: bool,
    job: Option<Job>,
    /// Parâmetros cujo resultado está na camada agora.
    shown: Option<Filter>,
}

fn slider(
    ui: &mut egui::Ui,
    label: &str,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.add(egui::Slider::new(v, range).text(label)).changed()
}

/// Editor dos parâmetros de um filtro. Retorna `true` quando algo mudou.
pub fn filter_ui(ui: &mut egui::Ui, f: &mut Filter) -> bool {
    let mut ch = false;
    match f {
        Filter::GaussianBlur { radius } | Filter::BoxBlur { radius } => {
            ch |= slider(ui, "Raio (px)", radius, 0.0..=100.0);
        }
        Filter::MotionBlur { angle, distance } => {
            ch |= slider(ui, "Ângulo (°)", angle, -180.0..=180.0);
            ch |= slider(ui, "Distância (px)", distance, 1.0..=200.0);
        }
        Filter::UnsharpMask {
            amount,
            radius,
            threshold,
        } => {
            ch |= slider(ui, "Quantidade (%)", amount, 0.0..=500.0);
            ch |= slider(ui, "Raio (px)", radius, 0.1..=50.0);
            ch |= slider(ui, "Limiar", threshold, 0.0..=255.0);
        }
        Filter::Sharpen => {
            ui.label("Realça detalhes com um núcleo fixo 3×3 (sem parâmetros).");
        }
        Filter::NoiseReduction { strength } => {
            ch |= slider(ui, "Força (%)", strength, 0.0..=100.0);
            ui.small("Filtro bilateral 5×5: suaviza o grão e preserva bordas fortes.");
        }
        Filter::Median { radius } => {
            let mut r = *radius as f32;
            if slider(ui, "Raio (px)", &mut r, 1.0..=8.0) {
                *radius = r.round() as u32;
                ch = true;
            }
        }
        Filter::AddNoise { amount, mono, seed } => {
            ch |= slider(ui, "Quantidade (%)", amount, 0.0..=100.0);
            ch |= ui.checkbox(mono, "Monocromático").changed();
            let mut s = *seed as f32;
            if slider(ui, "Semente", &mut s, 0.0..=9999.0) {
                *seed = s as u32;
                ch = true;
            }
            ui.small("A mesma semente gera sempre o mesmo ruído.");
        }
        Filter::Pixelate { size } => {
            let mut s = *size as f32;
            if slider(ui, "Tamanho do bloco (px)", &mut s, 1.0..=128.0) {
                *size = s.round() as u32;
                ch = true;
            }
        }
        Filter::Emboss { angle, height } => {
            ch |= slider(ui, "Ângulo (°)", angle, -180.0..=180.0);
            ch |= slider(ui, "Altura", height, 0.1..=20.0);
            ui.small("O resultado é cinza (os tons de cor são descartados).");
        }
        Filter::EdgeDetect => {
            ui.label("Bordas claras sobre fundo escuro (Sobel, sem parâmetros).");
        }
        Filter::Twirl { angle } => {
            ch |= slider(ui, "Ângulo (°)", angle, -720.0..=720.0);
            ui.small("Gira a partir do centro da camada.");
        }
        Filter::Ripple {
            amplitude,
            wavelength,
        } => {
            ch |= slider(ui, "Amplitude (px)", amplitude, 0.0..=100.0);
            ch |= slider(ui, "Comprimento de onda (px)", wavelength, 4.0..=400.0);
        }
        Filter::Spherize { amount } => {
            ch |= slider(ui, "Quantidade (%)", amount, -100.0..=100.0);
        }
    }
    ch
}

impl FilterSession {
    pub fn begin(open: &OpenDoc, kind: usize) -> Result<Self, String> {
        let idx = open
            .active_idx()
            .ok_or("Abra ou selecione uma camada primeiro.")?;
        let l = &open.doc.layers[idx];
        if l.adjustment.is_some() {
            return Err(
                "A camada ativa é uma camada de ajuste; filtros não se aplicam a ela.".into(),
            );
        }
        if let Some(msg) = destructive_blocked(l) {
            return Err(msg.replace("ajustar", "filtrar"));
        }
        Ok(Self {
            applied: false,
            layer: l.id,
            original: l.pixels.clone(),
            filter: Filter::neutral(kind),
            rev: open.doc.revision,
            was_modified: open.doc.modified,
            job: None,
            shown: None,
        })
    }

    fn start_job(&mut self, open: &OpenDoc, ctx: &egui::Context) {
        self.cancel_job();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (f, orig, sel) = (
            self.filter.clone(),
            self.original.clone(),
            open.doc.selection.mask.clone(),
        );
        let (w, h) = (open.doc.width, open.doc.height);
        let (c2, ctx2, f2) = (cancel.clone(), ctx.clone(), f.clone());
        std::thread::spawn(move || {
            let r =
                filter::apply_selected(&f2, &orig, w, h, sel.as_ref().map(|m| m.as_slice()), &c2);
            let _ = tx.send(r);
            ctx2.request_repaint();
        });
        self.job = Some(Job {
            rx,
            cancel,
            filter: f,
        });
    }

    fn cancel_job(&mut self) {
        if let Some(j) = self.job.take() {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Recolhe o resultado do cálculo em segundo plano e inicia outro se os parâmetros mudaram.
    pub fn poll(&mut self, open: &mut OpenDoc, ctx: &egui::Context) {
        if let Some(j) = &self.job {
            match j.rx.try_recv() {
                Ok(Some(px)) => {
                    let f = j.filter.clone();
                    self.job = None;
                    let (w, h) = (open.doc.width, open.doc.height);
                    if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == self.layer) {
                        l.pixels = Arc::new(px);
                    }
                    open.mark_dirty(PixRect::full(w, h));
                    self.rev = open.doc.revision;
                    self.shown = Some(f);
                }
                Ok(None) | Err(TryRecvError::Disconnected) => self.job = None,
                Err(TryRecvError::Empty) => {
                    if j.filter != self.filter {
                        self.start_job(open, ctx);
                    } else {
                        ctx.request_repaint();
                    }
                }
            }
        }
        if self.job.is_none() && self.shown.as_ref() != Some(&self.filter) {
            self.start_job(open, ctx);
        }
    }

    /// Há um resultado pronto que corresponde aos parâmetros atuais.
    pub fn ready(&self) -> bool {
        self.job.is_none() && self.shown.as_ref() == Some(&self.filter)
    }

    pub fn restore(&mut self, open: &mut OpenDoc) {
        self.cancel_job();
        let (w, h) = (open.doc.width, open.doc.height);
        if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == self.layer) {
            l.pixels = self.original.clone();
        }
        open.mark_dirty(PixRect::full(w, h));
        open.doc.modified = self.was_modified;
        self.rev = open.doc.revision;
    }

    pub fn commit(&mut self, open: &mut OpenDoc) {
        let (w, h) = (open.doc.width, open.doc.height);
        let mut before = open.doc.layers.clone();
        if let Some(l) = before.iter_mut().find(|l| l.id == self.layer) {
            l.pixels = self.original.clone();
        }
        open.record_layers(
            &format!("Filtro: {}", self.filter.name()),
            before,
            PixRect::full(w, h),
        );
    }
}

pub fn cancel_session(s: &mut FilterSession, open: &mut OpenDoc) {
    if open.doc.revision == s.rev {
        s.restore(open);
    } else {
        s.cancel_job();
    }
}

/// Desenha a janela. Retorna `true` quando a sessão terminou.
pub fn session_window(ctx: &egui::Context, s: &mut FilterSession, open: &mut OpenDoc) -> bool {
    if open.doc.revision != s.rev {
        s.cancel_job();
        return true;
    }
    s.poll(open, ctx);
    let (mut ok, mut cancel, mut reset) = (false, false, false);
    let kind = s.filter.kind_index();
    egui::Window::new(format!("Filtro: {}", KINDS[kind]))
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            if filter_ui(ui, &mut s.filter) {
                s.filter.sanitize();
            }
            if !open.doc.selection.is_empty() {
                ui.small("O filtro vale somente dentro da seleção.");
            }
            ui.horizontal(|ui| {
                if s.job.is_some() {
                    ui.spinner();
                    ui.label("Calculando…");
                }
            });
            ui.horizontal(|ui| {
                ok = ui
                    .add_enabled(s.ready(), egui::Button::new("Aplicar"))
                    .clicked();
                reset = ui.button("Redefinir").clicked();
                cancel = ui.button("Cancelar").clicked();
            });
        });
    if reset {
        s.filter = Filter::neutral(kind);
    }
    if cancel {
        s.restore(open);
        return true;
    }
    if ok && s.ready() {
        s.commit(open);
        s.applied = true;
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::Document;

    fn open() -> OpenDoc {
        let mut d = Document::new("t", 16, 16, Some([255, 255, 255, 255])).unwrap();
        let mut top = orvalith_core::Layer::filled("L2", 16, 16, [0, 0, 0, 0]);
        for y in 4..12 {
            for x in 4..12 {
                top.set_pixel(x, y, [255, 0, 0, 255]);
            }
        }
        d.layers.push(top);
        OpenDoc::new(d)
    }

    /// Espera o cálculo em segundo plano terminar (sem interface).
    fn settle(s: &mut FilterSession, o: &mut OpenDoc, ctx: &egui::Context) {
        for _ in 0..500 {
            s.poll(o, ctx);
            if s.ready() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("filtro não terminou");
    }

    #[test]
    fn preview_runs_in_background_commits_once_and_undoes() {
        let ctx = egui::Context::default();
        let mut o = open();
        let orig = o.doc.layers[1].pixels.clone();
        let n0 = o.history.labels().len();
        let mut s = FilterSession::begin(&o, 0).unwrap();
        settle(&mut s, &mut o, &ctx);
        assert_ne!(o.doc.layers[1].pixels, orig, "prévia aplicada à camada");
        s.commit(&mut o);
        assert_eq!(o.history.labels().len(), n0 + 1);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[1].pixels, orig);
    }

    #[test]
    fn changing_parameters_restarts_and_cancel_leaves_document_untouched() {
        let ctx = egui::Context::default();
        let mut o = open();
        let orig = o.doc.layers[1].pixels.clone();
        let was = o.doc.modified;
        let mut s = FilterSession::begin(&o, 0).unwrap();
        s.filter = Filter::BoxBlur { radius: 1.0 };
        settle(&mut s, &mut o, &ctx);
        assert_eq!(s.shown, Some(Filter::BoxBlur { radius: 1.0 }));
        s.restore(&mut o);
        assert_eq!(o.doc.layers[1].pixels, orig);
        assert_eq!(o.doc.modified, was);
        assert_eq!(o.history.labels().len(), 0);
    }

    #[test]
    fn selection_limits_the_filter_and_blocked_layers_are_refused() {
        let ctx = egui::Context::default();
        let mut o = open();
        o.doc.selection = orvalith_core::Selection::from_mask(Some(vec![0; 256]));
        let orig = o.doc.layers[1].pixels.clone();
        let mut s = FilterSession::begin(&o, 8).unwrap(); // pixelar
        settle(&mut s, &mut o, &ctx);
        assert_eq!(o.doc.layers[1].pixels, orig, "seleção vazia = nada muda");
        o.doc.layers[1].lock_pixels = true;
        assert!(FilterSession::begin(&o, 0).is_err());
    }
}
