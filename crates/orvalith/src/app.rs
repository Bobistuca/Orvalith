//! Estado da aplicação e interface: menus, abas, diálogos, atalhos e barra de status.

use eframe::egui::{self, Key, Modifiers};
use orvalith_core::Document;

use crate::canvas::{self, OpenDoc, ViewCmd};
use crate::prefs::Prefs;
use crate::theme;

struct NewDocForm {
    width: u32,
    height: u32,
    transparent: bool,
}

pub struct OrvalithApp {
    docs: Vec<OpenDoc>,
    active: usize,
    new_doc: Option<NewDocForm>,
    counter: u32,
    error: Option<String>,
    /// Aba aguardando confirmação para fechar com alterações não salvas.
    pending_close: Option<usize>,
    prefs: Prefs,
    prefs_open: bool,
    cursor_doc: Option<[f32; 2]>,
}

impl OrvalithApp {
    pub fn new(prefs: Prefs) -> Self {
        Self {
            docs: Vec::new(),
            active: 0,
            new_doc: None,
            counter: 0,
            error: None,
            pending_close: None,
            prefs,
            prefs_open: false,
            cursor_doc: None,
        }
    }

    fn open_new_dialog(&mut self) {
        self.new_doc = Some(NewDocForm {
            width: 1920,
            height: 1080,
            transparent: false,
        });
    }

    fn create_document(&mut self, form: &NewDocForm) -> Result<(), String> {
        self.counter += 1;
        let name = format!("Sem título-{}", self.counter);
        let bg = (!form.transparent).then_some([255, 255, 255, 255]);
        let doc = Document::new(name, form.width, form.height, bg)?;
        self.docs.push(OpenDoc::new(doc));
        self.active = self.docs.len() - 1;
        Ok(())
    }

    /// Fecha a aba; pede confirmação se houver alterações não salvas.
    fn request_close(&mut self, index: usize) {
        match self.docs.get(index) {
            Some(d) if d.doc.modified => self.pending_close = Some(index),
            Some(_) => self.close_now(index),
            None => {}
        }
    }

    fn close_now(&mut self, index: usize) {
        if index < self.docs.len() {
            self.docs.remove(index);
            if self.active > index || self.active >= self.docs.len() {
                self.active = self.active.saturating_sub(1);
            }
        }
    }

    fn window_size(&self, ctx: &egui::Context) -> Option<[f32; 2]> {
        let z = ctx.zoom_factor();
        ctx.input(|i| i.viewport().inner_rect)
            .map(|r| [r.width() * z, r.height() * z])
    }
}

impl eframe::App for OrvalithApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();

        if let Some(size) = self.window_size(&ctx) {
            self.prefs.window_size = size;
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.prefs.save();
        }

        // Atalhos: só quando nenhum campo de texto tem o foco do teclado.
        let typing = ctx.egui_wants_keyboard_input();
        let mut view_cmd = None;
        let mut want_new = false;
        let mut want_close = false;
        let mut next_tab = false;
        if !typing {
            ctx.input_mut(|i| {
                want_new |= i.consume_key(Modifiers::COMMAND, Key::N);
                want_close |= i.consume_key(Modifiers::COMMAND, Key::W);
                next_tab |= i.consume_key(Modifiers::CTRL, Key::Tab);
                if i.consume_key(Modifiers::COMMAND, Key::Num0) {
                    view_cmd = Some(ViewCmd::Fit);
                }
                if i.consume_key(Modifiers::COMMAND, Key::Num1) {
                    view_cmd = Some(ViewCmd::Actual);
                }
                if i.consume_key(Modifiers::COMMAND, Key::Equals)
                    || i.consume_key(Modifiers::COMMAND, Key::Plus)
                {
                    view_cmd = Some(ViewCmd::ZoomIn);
                }
                if i.consume_key(Modifiers::COMMAND, Key::Minus) {
                    view_cmd = Some(ViewCmd::ZoomOut);
                }
            });
        }
        let space_down = !typing && ctx.input(|i| i.key_down(Key::Space));
        let has_doc = !self.docs.is_empty();

        egui::Panel::top("menu").show(root, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Arquivo", |ui| {
                    if ui
                        .add(egui::Button::new("Novo…").shortcut_text("Ctrl+N"))
                        .clicked()
                    {
                        want_new = true;
                        ui.close();
                    }
                    ui.add_enabled(false, egui::Button::new("Abrir…").shortcut_text("Ctrl+O"))
                        .on_disabled_hover_text("Disponível no Estágio 2");
                    ui.add_enabled(false, egui::Button::new("Salvar").shortcut_text("Ctrl+S"))
                        .on_disabled_hover_text("Disponível no Estágio 2");
                    ui.separator();
                    if ui
                        .add_enabled(
                            has_doc,
                            egui::Button::new("Fechar documento").shortcut_text("Ctrl+W"),
                        )
                        .clicked()
                    {
                        want_close = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Sair").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Visualizar", |ui| {
                    let items = [
                        ("Ajustar à janela", "Ctrl+0", ViewCmd::Fit),
                        ("Tamanho real (100%)", "Ctrl+1", ViewCmd::Actual),
                        ("Aproximar", "Ctrl+=", ViewCmd::ZoomIn),
                        ("Afastar", "Ctrl+-", ViewCmd::ZoomOut),
                    ];
                    for (label, sc, cmd) in items {
                        if ui
                            .add_enabled(has_doc, egui::Button::new(label).shortcut_text(sc))
                            .clicked()
                        {
                            view_cmd = Some(cmd);
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Editar", |ui| {
                    if ui.button("Preferências…").clicked() {
                        self.prefs_open = true;
                        ui.close();
                    }
                });
            });
        });

        if has_doc {
            let mut close_tab = None;
            egui::Panel::top("tabs").show(root, |ui| {
                ui.horizontal(|ui| {
                    for (i, d) in self.docs.iter().enumerate() {
                        if d.doc.modified {
                            ui.label(egui::RichText::new("•").color(theme::COPPER))
                                .on_hover_text("Alterações não salvas");
                        }
                        let r = ui.selectable_label(i == self.active, d.doc.name.clone());
                        if r.clicked() {
                            self.active = i;
                        }
                        if r.middle_clicked() {
                            close_tab = Some(i);
                        }
                        if ui
                            .small_button("×")
                            .on_hover_text("Fechar documento")
                            .clicked()
                        {
                            close_tab = Some(i);
                        }
                        ui.separator();
                    }
                });
            });
            if let Some(i) = close_tab {
                self.request_close(i);
            }

            egui::Panel::bottom("status").show(root, |ui| {
                ui.horizontal(|ui| {
                    if let Some(d) = self.docs.get(self.active) {
                        ui.label(format!("{:.0}%", d.zoom_percent));
                        ui.separator();
                        ui.label(format!("{} × {} px", d.doc.width, d.doc.height));
                        if let Some(p) = self.cursor_doc {
                            ui.separator();
                            ui.label(format!("x: {}  y: {}", p[0].floor(), p[1].floor()));
                        }
                    }
                });
            });
        }

        let mut open_new = want_new;
        egui::CentralPanel::default().show(root, |ui| {
            if let Some(open) = self.docs.get_mut(self.active) {
                self.cursor_doc = canvas::show(ui, open, view_cmd, space_down);
            } else {
                self.cursor_doc = None;
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.3);
                    ui.heading(egui::RichText::new("Orvalith").color(theme::ACCENT));
                    ui.label("Editor de imagens local, sem login.");
                    ui.add_space(12.0);
                    if ui.button("Novo documento").clicked() {
                        open_new = true;
                    }
                    ui.add_enabled(false, egui::Button::new("Abrir arquivo"))
                        .on_disabled_hover_text("Disponível no Estágio 2");
                });
            }
        });
        if open_new && self.new_doc.is_none() {
            self.open_new_dialog();
        }
        if want_close && has_doc {
            self.request_close(self.active);
        }
        if next_tab && self.docs.len() > 1 {
            self.active = (self.active + 1) % self.docs.len();
        }

        self.show_dialogs(&ctx);
    }
}

impl OrvalithApp {
    fn show_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(mut form) = self.new_doc.take() {
            let mut create = false;
            let mut cancel = false;
            egui::Window::new("Novo documento")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Largura (px)");
                        ui.add(egui::DragValue::new(&mut form.width).range(1..=Document::MAX_SIDE));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Altura (px)");
                        ui.add(
                            egui::DragValue::new(&mut form.height).range(1..=Document::MAX_SIDE),
                        );
                    });
                    ui.checkbox(&mut form.transparent, "Fundo transparente");
                    ui.horizontal(|ui| {
                        create = ui.button("Criar").clicked();
                        cancel = ui.button("Cancelar").clicked();
                    });
                });
            if create {
                if let Err(e) = self.create_document(&form) {
                    self.error = Some(e);
                }
            } else if !cancel {
                self.new_doc = Some(form);
            }
        }

        if let Some(i) = self.pending_close {
            let name = self
                .docs
                .get(i)
                .map(|d| d.doc.name.clone())
                .unwrap_or_default();
            let mut discard = false;
            let mut cancel = false;
            egui::Window::new("Alterações não salvas")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "\"{name}\" tem alterações não salvas. Salvar estará disponível no Estágio 2; por enquanto só é possível descartar."
                    ));
                    ui.horizontal(|ui| {
                        discard = ui.button("Descartar e fechar").clicked();
                        cancel = ui.button("Cancelar").clicked();
                    });
                });
            if discard {
                self.close_now(i);
            }
            if discard || cancel {
                self.pending_close = None;
            }
        }

        if self.prefs_open {
            let mut open = true;
            let before = self.prefs.clone();
            egui::Window::new("Preferências")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.checkbox(&mut self.prefs.dark_theme, "Tema escuro");
                    ui.horizontal(|ui| {
                        ui.label("Escala da interface");
                        ui.add(
                            egui::Slider::new(&mut self.prefs.ui_scale, 0.75..=2.0).step_by(0.05),
                        );
                    });
                    ui.small("As preferências ficam somente neste computador.");
                });
            if self.prefs.dark_theme != before.dark_theme {
                theme::apply(ctx, self.prefs.dark_theme);
            }
            if (self.prefs.ui_scale - before.ui_scale).abs() > f32::EPSILON {
                ctx.set_zoom_factor(self.prefs.ui_scale);
            }
            if self.prefs != before || !open {
                self.prefs.save();
            }
            self.prefs_open = open;
        }

        if let Some(msg) = self.error.clone() {
            egui::Window::new("Erro").show(ctx, |ui| {
                ui.label(msg);
                if ui.button("OK").clicked() {
                    self.error = None;
                }
            });
        }
    }
}
