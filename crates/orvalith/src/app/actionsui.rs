//! Janelas de ações (gravar e reproduzir macros) e de processamento em lote.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use eframe::egui;
use orvalith_core::actions::{self, BatchOpts, FileReport, Macro, Status, Step};
use orvalith_core::docops::DocState;
use orvalith_core::formats::{self, ExportOpts, Format};
use orvalith_core::fsio;

use super::OrvalithApp;

#[derive(Default)]
pub(super) struct ActionsUi {
    pub open: bool,
    pub recording: bool,
    pub steps: Vec<Step>,
    name: String,
    expanded: Option<usize>,
}

struct Job {
    rx: Receiver<Vec<FileReport>>,
    progress: Arc<AtomicUsize>,
    total: usize,
    cancel: Arc<AtomicBool>,
}

pub(super) struct BatchUi {
    pub open: bool,
    files: Vec<PathBuf>,
    out_dir: Option<PathBuf>,
    macro_idx: Option<usize>,
    format: Format,
    quality: u8,
    max_side: u32,
    suffix: String,
    overwrite: bool,
    job: Option<Job>,
    reports: Vec<FileReport>,
}

impl Default for BatchUi {
    fn default() -> Self {
        Self {
            open: false,
            files: Vec::new(),
            out_dir: None,
            macro_idx: None,
            format: Format::Png,
            quality: 90,
            max_side: 0,
            suffix: "_lote".into(),
            overwrite: false,
            job: None,
            reports: Vec::new(),
        }
    }
}

impl ActionsUi {
    /// Registra um passo se estiver gravando.
    pub fn record(&mut self, s: Step) {
        if self.recording {
            self.steps.push(s);
        }
    }
}

impl OrvalithApp {
    /// Passos gerados pela geometria (tamanho da imagem, orientação) entram na gravação.
    pub(super) fn drain_recorded(&mut self) {
        let taken = std::mem::take(&mut self.tools.geo.recorded);
        for s in taken {
            self.actions.record(s);
        }
    }

    /// Reproduz uma ação no documento ativo como um único passo de desfazer.
    pub(super) fn play_macro(&mut self, idx: usize) {
        let Some(m) = self.prefs.macros.get(idx).cloned() else {
            return;
        };
        let was = self.actions.recording;
        self.actions.recording = false; // reproduzir nunca grava
        let Some(open) = self.docs.get_mut(self.active) else {
            self.actions.recording = was;
            return;
        };
        let mut d = open.doc.clone();
        match actions::run(&mut d, &m) {
            Ok(()) => open.apply_state(&format!("Ação: {}", m.name), DocState::of(&d)),
            Err(e) => {
                self.actions.recording = was;
                return self.show_message(
                    "Não foi possível reproduzir a ação",
                    format!("\"{}\"\n\n{e}\n\nO documento não foi alterado.", m.name),
                );
            }
        }
        self.actions.recording = was;
    }

    pub(super) fn actions_window(&mut self, ctx: &egui::Context) {
        if !self.actions.open {
            return;
        }
        let mut is_open = true;
        let mut play: Option<usize> = None;
        let mut to_batch: Option<usize> = None;
        let mut delete: Option<usize> = None;
        let mut save = false;
        egui::Window::new("Ações (macros)")
            .open(&mut is_open)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.actions.recording {
                        if ui.button("⏹ Parar gravação").clicked() {
                            self.actions.recording = false;
                        }
                        ui.colored_label(ui.visuals().warn_fg_color, "● gravando");
                    } else if ui.button("⏺ Gravar").clicked() {
                        self.actions.recording = true;
                        self.actions.steps.clear();
                    }
                });
                ui.small("Grava ajustes e filtros aplicados (Aplicar), tamanho da imagem e giro/espelho da tela. Os passos agem em todas as camadas raster comuns.");
                let mut remove = None;
                for (i, s) in self.actions.steps.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}. {}", i + 1, s.describe()));
                        if ui.small_button("✕").on_hover_text("Remover passo").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    self.actions.steps.remove(i);
                }
                if !self.actions.recording && !self.actions.steps.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.actions.name).hint_text("Nome da ação").desired_width(180.0));
                        save = ui.add_enabled(!self.actions.name.trim().is_empty(), egui::Button::new("Salvar ação")).clicked();
                    });
                }
                ui.separator();
                ui.label("Ações salvas");
                if self.prefs.macros.is_empty() {
                    ui.small("Nenhuma ação salva ainda.");
                }
                for (i, m) in self.prefs.macros.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let open = self.actions.expanded == Some(i);
                        if ui.small_button(if open { "▾" } else { "▸" }).clicked() {
                            self.actions.expanded = if open { None } else { Some(i) };
                        }
                        ui.label(format!("{} ({} passos)", m.name, m.steps.len()));
                        if ui.button("Reproduzir").on_hover_text("Aplica ao documento ativo (um passo de desfazer)").clicked() {
                            play = Some(i);
                        }
                        if ui.button("Lote…").clicked() {
                            to_batch = Some(i);
                        }
                        if ui.small_button("✕").on_hover_text("Excluir ação").clicked() {
                            delete = Some(i);
                        }
                    });
                    if self.actions.expanded == Some(i) {
                        for (n, s) in m.steps.iter().enumerate() {
                            ui.small(format!("   {}. {}", n + 1, s.describe()));
                        }
                    }
                }
            });
        self.actions.open = is_open;
        if save {
            let mut m = Macro {
                name: self.actions.name.trim().to_string(),
                steps: std::mem::take(&mut self.actions.steps),
            };
            m.sanitize();
            self.prefs.macros.retain(|x| x.name != m.name);
            self.prefs.macros.push(m);
            self.prefs.macros.truncate(100);
            self.actions.name.clear();
            self.prefs.save();
        }
        if let Some(i) = delete {
            if i < self.prefs.macros.len() {
                self.prefs.macros.remove(i);
                self.actions.expanded = None;
                self.prefs.save();
            }
        }
        if let Some(i) = play {
            self.play_macro(i);
        }
        if let Some(i) = to_batch {
            self.batch.macro_idx = Some(i);
            self.batch.open = true;
        }
    }

    pub(super) fn batch_poll(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.batch.job else { return };
        match job.rx.try_recv() {
            Ok(r) => {
                self.batch.reports = r;
                self.batch.job = None;
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100))
            }
            Err(mpsc::TryRecvError::Disconnected) => self.batch.job = None,
        }
    }

    pub(super) fn batch_window(&mut self, ctx: &egui::Context) {
        if !self.batch.open {
            return;
        }
        let mut is_open = true;
        let mut run = false;
        let mut save_report = false;
        egui::Window::new("Processamento em lote")
            .open(&mut is_open)
            .default_width(560.0)
            .show(ctx, |ui| {
                let b = &mut self.batch;
                ui.horizontal(|ui| {
                    if ui.button("Escolher arquivos…").clicked() {
                        if let Some(f) = rfd::FileDialog::new().pick_files() {
                            b.files = f;
                        }
                    }
                    ui.label(format!("{} arquivo(s)", b.files.len()));
                    if ui.small_button("Limpar").clicked() {
                        b.files.clear();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Pasta de saída…").clicked() {
                        b.out_dir = rfd::FileDialog::new().pick_folder();
                    }
                    ui.label(b.out_dir.as_ref().map_or("(nenhuma)".to_string(), |d| d.display().to_string()));
                });
                egui::ComboBox::from_label("Ação")
                    .selected_text(b.macro_idx.and_then(|i| self.prefs.macros.get(i)).map_or("(nenhuma: só converter)", |m| m.name.as_str()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut b.macro_idx, None, "(nenhuma: só converter)");
                        for (i, m) in self.prefs.macros.iter().enumerate() {
                            ui.selectable_value(&mut b.macro_idx, Some(i), &m.name);
                        }
                    });
                egui::ComboBox::from_label("Formato de saída")
                    .selected_text(formats::info(b.format).name)
                    .show_ui(ui, |ui| {
                        for f in Format::exportable() {
                            ui.selectable_value(&mut b.format, f, formats::info(f).name);
                        }
                    });
                if b.format == Format::Jpeg {
                    ui.add(egui::Slider::new(&mut b.quality, 1..=100).text("qualidade"));
                }
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut b.max_side).range(0..=16384).prefix("lado máximo ").suffix(" px (0 = original)"));
                    ui.add(egui::TextEdit::singleline(&mut b.suffix).hint_text("sufixo").desired_width(90.0));
                });
                ui.checkbox(&mut b.overwrite, "Permitir sobrescrever arquivos existentes (inclusive os originais)");
                if b.overwrite {
                    ui.colored_label(ui.visuals().warn_fg_color, "Atenção: arquivos de mesmo nome na pasta de saída serão substituídos.");
                } else {
                    ui.small("Sem esta opção, nenhum arquivo é sobrescrito: nomes repetidos ganham \" (2)\" e a saída que apagaria o original é recusada.");
                }
                ui.horizontal(|ui| {
                    if let Some(j) = &b.job {
                        let done = j.progress.load(Ordering::Relaxed);
                        ui.add(egui::ProgressBar::new(done as f32 / j.total.max(1) as f32).text(format!("{done}/{}", j.total)).desired_width(240.0));
                        if ui.button("Cancelar").clicked() {
                            j.cancel.store(true, Ordering::Relaxed);
                        }
                    } else {
                        run = ui
                            .add_enabled(!b.files.is_empty() && b.out_dir.is_some(), egui::Button::new("Processar"))
                            .clicked();
                    }
                    if !b.reports.is_empty() && ui.button("Salvar relatório…").clicked() {
                        save_report = true;
                    }
                });
                if !b.reports.is_empty() {
                    ui.separator();
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for r in &b.reports {
                            let (tag, color, detail) = match &r.status {
                                Status::Done(p) => ("OK", egui::Color32::from_rgb(70, 190, 110), format!("→ {}", p.display())),
                                Status::Failed(e) => ("FALHA", egui::Color32::from_rgb(230, 90, 80), e.clone()),
                                Status::Skipped(e) => ("IGNORADO", egui::Color32::from_rgb(220, 180, 60), e.clone()),
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(color, format!("[{tag}]"));
                                ui.label(r.input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                                ui.small(detail);
                            });
                            for w in &r.warnings {
                                ui.small(format!("      aviso: {w}"));
                            }
                        }
                    });
                }
            });
        self.batch.open = is_open;
        if run {
            self.batch_start();
        }
        if save_report {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Texto", &["txt"])
                .set_file_name("relatorio-lote.txt")
                .save_file()
            {
                let text = actions::report_text(&self.batch.reports);
                match fsio::atomic_write(&path, text.as_bytes()) {
                    Ok(()) => self.show_message("Relatório salvo", path.display().to_string()),
                    Err(e) => self.show_message("Erro ao salvar o relatório", e.to_string()),
                }
            }
        }
    }

    fn batch_start(&mut self) {
        let b = &self.batch;
        let Some(out_dir) = b.out_dir.clone() else {
            return;
        };
        let m = b.macro_idx.and_then(|i| self.prefs.macros.get(i)).cloned();
        let opts = BatchOpts {
            out_dir,
            format: b.format,
            export: ExportOpts {
                quality: b.quality,
                max_side: (b.max_side > 0).then_some(b.max_side),
                ..ExportOpts::default()
            },
            suffix: b
                .suffix
                .chars()
                .filter(|c| !"\\/:*?\"<>|".contains(*c))
                .collect(),
            overwrite: b.overwrite,
        };
        let files = b.files.clone();
        let total = files.len();
        let progress = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p2, c2) = (progress.clone(), cancel.clone());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = actions::run_batch(&files, m.as_ref(), &opts, &c2, &mut |i, _, _| {
                p2.store(i, Ordering::Relaxed)
            });
            let _ = tx.send(r);
        });
        self.batch.reports.clear();
        self.batch.job = Some(Job {
            rx,
            progress,
            total,
            cancel,
        });
    }
}
