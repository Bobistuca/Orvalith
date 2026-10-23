//! Importar e exportar PDF (somente local): janelas de páginas, senha, resolução e página.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use orvalith_core::pdf::{
    self, Compression, OpenError, Orientation, PageSize, PdfSetup, RenderOpts,
};
use orvalith_core::{fsio, Document};

use super::OrvalithApp;
use crate::canvas::OpenDoc;

const MAX_PDF_BYTES: u64 = 1 << 30;
const MM_TO_PT: f32 = 72.0 / 25.4;

pub(super) struct PdfImport {
    name: String,
    bytes: Arc<Vec<u8>>,
    /// Só existe na memória enquanto a janela está aberta; nunca é gravada.
    password: String,
    needs_password: bool,
    wrong_password: bool,
    sizes: Vec<(f32, f32)>,
    selected: Vec<bool>,
    dpi: f32,
    /// 0 = transparente, 1 = branco, 2 = cor.
    bg: u8,
    color: [u8; 3],
    trim_mm: [f32; 4],
    thumbs: Vec<Option<egui::TextureHandle>>,
    next_thumb: usize,
}

impl PdfImport {
    fn opts(&self) -> RenderOpts {
        RenderOpts {
            dpi: self.dpi,
            background: match self.bg {
                0 => None,
                1 => Some([255, 255, 255]),
                _ => Some(self.color),
            },
            trim_pt: self.trim_mm.map(|v| v * MM_TO_PT),
        }
    }

    /// Tenta abrir com a senha atual; `Err` para PDF inválido.
    fn try_open(&mut self) -> Result<(), String> {
        match pdf::page_sizes(&self.bytes, &self.password) {
            Ok(sizes) => {
                self.needs_password = false;
                self.wrong_password = false;
                self.selected = vec![true; sizes.len()];
                self.thumbs = (0..sizes.len()).map(|_| None).collect();
                self.sizes = sizes;
                Ok(())
            }
            Err(OpenError::NeedsPassword) => {
                self.needs_password = true;
                Ok(())
            }
            Err(OpenError::WrongPassword) => {
                self.needs_password = true;
                self.wrong_password = true;
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

pub(super) struct PdfExport {
    selected: Vec<bool>,
    setup: PdfSetup,
    size_idx: usize,
    custom_mm: [f32; 2],
    orientation: usize,
    jpeg: bool,
    quality: u8,
}

const SIZE_NAMES: [&str; 7] = [
    "Tamanho da imagem",
    "A3",
    "A4",
    "A5",
    "Carta (Letter)",
    "Ofício (Legal)",
    "Personalizado",
];

fn size_of(idx: usize, custom: [f32; 2]) -> PageSize {
    match idx {
        0 => PageSize::FitImage,
        1 => PageSize::A3,
        2 => PageSize::A4,
        3 => PageSize::A5,
        4 => PageSize::Letter,
        5 => PageSize::Legal,
        _ => PageSize::Custom(custom[0].clamp(20.0, 5000.0), custom[1].clamp(20.0, 5000.0)),
    }
}

impl OrvalithApp {
    pub(super) fn start_pdf_import(&mut self, path: PathBuf) {
        match std::fs::metadata(&path) {
            Ok(m) if m.len() > MAX_PDF_BYTES => {
                return self.show_message("PDF grande demais", path.display().to_string());
            }
            Err(e) => {
                return self.show_message(
                    "Não foi possível abrir",
                    format!("{}\n\n{e}", path.display()),
                )
            }
            _ => {}
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                return self.show_message(
                    "Não foi possível abrir",
                    format!("{}\n\n{e}", path.display()),
                )
            }
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "PDF".into());
        let mut st = PdfImport {
            name,
            bytes: Arc::new(bytes),
            password: String::new(),
            needs_password: false,
            wrong_password: false,
            sizes: Vec::new(),
            selected: Vec::new(),
            dpi: 150.0,
            bg: 1,
            color: [255, 255, 255],
            trim_mm: [0.0; 4],
            thumbs: Vec::new(),
            next_thumb: 0,
        };
        match st.try_open() {
            Ok(()) => self.pdf_import = Some(st),
            Err(e) => self.show_message("PDF inválido", format!("{}\n\n{e}", path.display())),
        }
    }

    pub(super) fn pdf_import_window(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.pdf_import.take() else {
            return;
        };
        let (mut close, mut do_import, mut retry) = (false, false, false);
        egui::Window::new(format!("Importar PDF: {}", st.name))
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                if st.needs_password {
                    ui.label("Este PDF é protegido por senha. Digite-a (usada só localmente e nunca gravada).");
                    if st.wrong_password {
                        ui.colored_label(ui.visuals().warn_fg_color, "Senha incorreta ou criptografia não suportada.");
                    }
                    let r = ui.add(egui::TextEdit::singleline(&mut st.password).password(true).desired_width(220.0));
                    ui.horizontal(|ui| {
                        retry = ui.button("Abrir").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                        close = ui.button("Cancelar").clicked();
                    });
                    return;
                }
                ui.label(format!("{} página(s). As páginas são rasterizadas: o texto importado não fica editável.", st.sizes.len()));
                ui.horizontal(|ui| {
                    if ui.small_button("Todas").clicked() {
                        st.selected.iter_mut().for_each(|s| *s = true);
                    }
                    if ui.small_button("Nenhuma").clicked() {
                        st.selected.iter_mut().for_each(|s| *s = false);
                    }
                });
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for i in 0..st.sizes.len() {
                            ui.vertical(|ui| {
                                match &st.thumbs[i] {
                                    Some(t) => {
                                        ui.add(egui::Image::new(t).max_height(110.0));
                                    }
                                    None => {
                                        ui.allocate_space(egui::vec2(80.0, 110.0));
                                    }
                                }
                                let (w, h) = st.sizes[i];
                                ui.checkbox(&mut st.selected[i], format!("{}", i + 1));
                                ui.small(format!("{:.0}×{:.0} mm", w / MM_TO_PT, h / MM_TO_PT));
                            });
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Resolução");
                    ui.add(egui::DragValue::new(&mut st.dpi).range(30.0..=1200.0).suffix(" ppi"));
                });
                if let Some(first) = st.selected.iter().position(|s| *s) {
                    let (w, h) = st.sizes[first];
                    let scale = st.dpi / 72.0;
                    ui.small(format!(
                        "Tamanho físico {:.0}×{:.0} mm → {}×{} px na página {}",
                        w / MM_TO_PT,
                        h / MM_TO_PT,
                        (w * scale).round(),
                        (h * scale).round(),
                        first + 1
                    ));
                }
                ui.horizontal(|ui| {
                    ui.label("Fundo");
                    ui.selectable_value(&mut st.bg, 0, "Transparente");
                    ui.selectable_value(&mut st.bg, 1, "Branco");
                    ui.selectable_value(&mut st.bg, 2, "Cor");
                    if st.bg == 2 {
                        ui.color_edit_button_srgb(&mut st.color);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Recortar (mm)");
                    for (label, v) in ["esq.", "topo", "dir.", "base"].iter().zip(st.trim_mm.iter_mut()) {
                        ui.add(egui::DragValue::new(v).range(0.0..=500.0).prefix(format!("{label} ")));
                    }
                });
                ui.horizontal(|ui| {
                    do_import = ui
                        .add_enabled(st.selected.iter().any(|s| *s), egui::Button::new("Importar páginas selecionadas"))
                        .clicked();
                    close = ui.button("Cancelar").clicked();
                });
            });
        if retry {
            if let Err(e) = st.try_open() {
                self.show_message("PDF inválido", e);
                return;
            }
        }
        // Uma miniatura por quadro (leve) enquanto houver páginas sem prévia.
        if !st.needs_password && st.next_thumb < st.sizes.len() {
            let i = st.next_thumb;
            st.next_thumb += 1;
            let o = RenderOpts {
                dpi: 28.0,
                background: Some([255, 255, 255]),
                trim_pt: [0.0; 4],
            };
            if let Ok((px, w, h)) = pdf::render_page(&st.bytes, &st.password, i, &o) {
                let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
                st.thumbs[i] = Some(ctx.load_texture(
                    format!("pdfthumb{i}"),
                    img,
                    egui::TextureOptions::LINEAR,
                ));
            }
            ctx.request_repaint();
        }
        if do_import {
            let pages: Vec<usize> = st
                .selected
                .iter()
                .enumerate()
                .filter(|(_, s)| **s)
                .map(|(i, _)| i)
                .collect();
            match pdf::import_pages(&st.bytes, &st.password, &pages, &st.name, &st.opts()) {
                Ok(docs) => {
                    let n = docs.len();
                    for d in docs {
                        self.docs.push(OpenDoc::new(d));
                    }
                    self.active = self.docs.len() - 1;
                    self.show_message(
                        "PDF importado",
                        format!("{n} página(s) importada(s) como imagens. O texto e os vetores foram rasterizados e não ficam editáveis."),
                    );
                }
                Err(e) => self.show_message("Erro ao importar o PDF", e),
            }
            return; // a senha e os dados saem da memória aqui
        }
        if close {
            return;
        }
        self.pdf_import = Some(st);
    }

    pub(super) fn start_pdf_export(&mut self) {
        if self.docs.is_empty() || self.pdf_export.is_some() {
            return;
        }
        let mut selected = vec![false; self.docs.len()];
        let last = selected.len() - 1;
        selected[self.active.min(last)] = true;
        self.pdf_export = Some(PdfExport {
            selected,
            setup: PdfSetup::default(),
            size_idx: 2,
            custom_mm: [210.0, 297.0],
            orientation: 0,
            jpeg: false,
            quality: 85,
        });
    }

    pub(super) fn pdf_export_window(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.pdf_export.take() else {
            return;
        };
        st.selected.resize(self.docs.len(), false);
        let (mut go, mut close) = (false, false);
        egui::Window::new("Exportar PDF")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Documentos (uma página por documento, na ordem das abas):");
                for (i, o) in self.docs.iter().enumerate() {
                    ui.checkbox(&mut st.selected[i], format!("{}  ({}×{} px)", o.doc.name, o.doc.width, o.doc.height));
                }
                ui.separator();
                egui::ComboBox::from_label("Página")
                    .selected_text(SIZE_NAMES[st.size_idx.min(6)])
                    .show_ui(ui, |ui| {
                        for (i, n) in SIZE_NAMES.iter().enumerate() {
                            ui.selectable_value(&mut st.size_idx, i, *n);
                        }
                    });
                if st.size_idx == 6 {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut st.custom_mm[0]).prefix("L ").suffix(" mm").range(20.0..=5000.0));
                        ui.add(egui::DragValue::new(&mut st.custom_mm[1]).prefix("A ").suffix(" mm").range(20.0..=5000.0));
                    });
                }
                if st.size_idx == 0 {
                    ui.horizontal(|ui| {
                        ui.label("Resolução da imagem");
                        ui.add(egui::DragValue::new(&mut st.setup.dpi).range(30.0..=1200.0).suffix(" ppi"));
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.label("Orientação");
                        ui.selectable_value(&mut st.orientation, 0, "Automática");
                        ui.selectable_value(&mut st.orientation, 1, "Retrato");
                        ui.selectable_value(&mut st.orientation, 2, "Paisagem");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Margens");
                        ui.add(egui::DragValue::new(&mut st.setup.margin_mm).range(0.0..=100.0).suffix(" mm"));
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Compressão");
                    ui.selectable_value(&mut st.jpeg, false, "Sem perdas (com alfa)");
                    ui.selectable_value(&mut st.jpeg, true, "JPEG");
                    if st.jpeg {
                        ui.add(egui::Slider::new(&mut st.quality, 1..=100));
                    }
                });
                st.setup.size = size_of(st.size_idx, st.custom_mm);
                st.setup.orientation = [Orientation::Auto, Orientation::Portrait, Orientation::Landscape][st.orientation.min(2)];
                st.setup.compression = if st.jpeg { Compression::Jpeg(st.quality) } else { Compression::Lossless };
                if let Some(i) = st.selected.iter().position(|s| *s) {
                    let d = &self.docs[i].doc;
                    let l = pdf::layout(d.width, d.height, &st.setup);
                    ui.small(format!(
                        "Página {:.0}×{:.0} mm · imagem com {:.0} ppi efetivos",
                        l.page_w / MM_TO_PT,
                        l.page_h / MM_TO_PT,
                        l.effective_dpi
                    ));
                }
                ui.small("O PDF contém imagens (raster): texto e vetores viram pixels e não ficam editáveis. Um PDF não substitui o projeto .orva com camadas.");
                if st.jpeg {
                    ui.small("JPEG não tem transparência: áreas transparentes ficam brancas.");
                }
                ui.horizontal(|ui| {
                    go = ui
                        .add_enabled(st.selected.iter().any(|s| *s), egui::Button::new("Escolher destino…"))
                        .clicked();
                    close = ui.button("Fechar").clicked();
                });
            });
        if go {
            let docs: Vec<&Document> = self
                .docs
                .iter()
                .zip(&st.selected)
                .filter(|(_, s)| **s)
                .map(|(o, _)| &o.doc)
                .collect();
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .set_file_name(format!(
                    "{}.pdf",
                    docs.first().map_or("documento", |d| d.name.as_str())
                ))
                .save_file()
            {
                let path = super::io::with_extension(path, "pdf");
                match pdf::export_pdf(&docs, &st.setup)
                    .and_then(|b| fsio::atomic_write(&path, &b).map_err(|e| e.to_string()))
                {
                    Ok(()) => self.show_message(
                        "PDF exportado",
                        format!("{}\n\n{} página(s), raster.", path.display(), docs.len()),
                    ),
                    Err(e) => self.show_message(
                        "Erro ao exportar o PDF",
                        format!("{}\n\n{e}", path.display()),
                    ),
                }
            }
        }
        if !close {
            self.pdf_export = Some(st);
        }
    }
}
