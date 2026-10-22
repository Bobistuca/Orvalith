//! Estado da aplicação e interface (Estágio 0: janela, estado vazio, novo documento).

use eframe::egui;
use orvalith_core::{compose, Document, History};

use crate::theme;

/// Documento aberto com seu histórico e estado de visualização.
struct OpenDoc {
    doc: Document,
    #[allow(dead_code)]
    history: History,
    texture: Option<(u64, egui::TextureHandle)>,
}

#[derive(Default)]
pub struct OrvalithApp {
    docs: Vec<OpenDoc>,
    active: usize,
    new_doc_open: bool,
    new_w: u32,
    new_h: u32,
    new_transparent: bool,
    counter: u32,
    error: Option<String>,
}

impl OrvalithApp {
    fn create_document(&mut self) {
        self.counter += 1;
        let name = format!("Sem título-{}", self.counter);
        let bg = (!self.new_transparent).then_some([255, 255, 255, 255]);
        match Document::new(name, self.new_w, self.new_h, bg) {
            Ok(doc) => {
                self.docs.push(OpenDoc {
                    doc,
                    history: History::new(),
                    texture: None,
                });
                self.active = self.docs.len() - 1;
                self.new_doc_open = false;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn open_new_dialog(&mut self) {
        self.new_w = 1920;
        self.new_h = 1080;
        self.new_transparent = false;
        self.new_doc_open = true;
    }
}

impl eframe::App for OrvalithApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        // Atalho só quando nenhum campo de texto tem foco.
        if !ctx.egui_wants_keyboard_input()
            && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N))
        {
            self.open_new_dialog();
        }

        egui::Panel::top("menu").show(root, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Arquivo", |ui| {
                    if ui.button("Novo… (Ctrl+N)").clicked() {
                        self.open_new_dialog();
                        ui.close();
                    }
                    ui.add_enabled(false, egui::Button::new("Abrir… (Ctrl+O)"))
                        .on_disabled_hover_text("Disponível no Estágio 2");
                    ui.separator();
                    if ui.button("Sair").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });

        if !self.docs.is_empty() {
            egui::Panel::top("tabs").show(root, |ui| {
                ui.horizontal(|ui| {
                    for (i, d) in self.docs.iter().enumerate() {
                        let mark = if d.doc.modified { "● " } else { "" };
                        let label = format!("{mark}{}", d.doc.name);
                        if ui.selectable_label(i == self.active, label).clicked() {
                            self.active = i;
                        }
                    }
                });
            });
        }

        egui::CentralPanel::default().show(root, |ui| {
            if let Some(open) = self.docs.get_mut(self.active) {
                show_canvas(ui, open);
            } else {
                let mut want_new = false;
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.3);
                    ui.heading(egui::RichText::new("Orvalith").color(theme::ACCENT));
                    ui.label("Editor de imagens local, sem login.");
                    ui.add_space(12.0);
                    if ui.button("Novo documento").clicked() {
                        want_new = true;
                    }
                });
                if want_new {
                    self.open_new_dialog();
                }
            }
        });

        if self.new_doc_open {
            let mut create = false;
            let mut cancel = false;
            egui::Window::new("Novo documento")
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Largura (px)");
                        ui.add(egui::DragValue::new(&mut self.new_w).range(1..=Document::MAX_SIDE));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Altura (px)");
                        ui.add(egui::DragValue::new(&mut self.new_h).range(1..=Document::MAX_SIDE));
                    });
                    ui.checkbox(&mut self.new_transparent, "Fundo transparente");
                    ui.horizontal(|ui| {
                        create = ui.button("Criar").clicked();
                        cancel = ui.button("Cancelar").clicked();
                    });
                });
            if create {
                self.create_document();
            }
            if cancel {
                self.new_doc_open = false;
            }
        }

        if let Some(msg) = self.error.clone() {
            egui::Window::new("Erro").show(&ctx, |ui| {
                ui.label(msg);
                if ui.button("OK").clicked() {
                    self.error = None;
                }
            });
        }
    }
}

/// Desenha o documento centralizado com xadrez de transparência (zoom ajustado à janela).
fn show_canvas(ui: &mut egui::Ui, open: &mut OpenDoc) {
    let avail = ui.available_size();
    let doc = &open.doc;
    let zoom = (avail.x / doc.width as f32)
        .min(avail.y / doc.height as f32)
        .clamp(0.01, 1.0);
    let size = egui::vec2(doc.width as f32 * zoom, doc.height as f32 * zoom);
    let (rect, _) = ui.allocate_exact_size(avail, egui::Sense::hover());
    let img_rect = egui::Rect::from_center_size(rect.center(), size);
    let painter = ui.painter_at(rect);

    let cell: f32 = 8.0;
    let (light, dark) = (egui::Color32::from_gray(200), egui::Color32::from_gray(150));
    let mut y = 0.0;
    let mut row = 0;
    while y < size.y {
        let mut x = 0.0;
        let mut col = 0;
        while x < size.x {
            let r = egui::Rect::from_min_size(
                img_rect.min + egui::vec2(x, y),
                egui::vec2(cell.min(size.x - x), cell.min(size.y - y)),
            );
            painter.rect_filled(r, 0.0, if (row + col) % 2 == 0 { light } else { dark });
            x += cell;
            col += 1;
        }
        y += cell;
        row += 1;
    }

    let rev = doc.revision;
    if open.texture.as_ref().map(|(r, _)| *r) != Some(rev) {
        let rgba = compose::flatten(doc);
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [doc.width as usize, doc.height as usize],
            &rgba,
        );
        let tex = ui
            .ctx()
            .load_texture("doc", image, egui::TextureOptions::LINEAR);
        open.texture = Some((rev, tex));
    }
    if let Some((_, tex)) = &open.texture {
        painter.image(
            tex.id(),
            img_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}
