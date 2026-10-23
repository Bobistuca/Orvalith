//! Estado da aplicação e interface: menus, abas, camadas, diálogos, atalhos e status.

mod io;
mod selops;

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use eframe::egui::{self, Key, Modifiers};
use orvalith_core::{Document, Layer};

use crate::canvas::OpenDoc;
use selops::{gray, SelCmd};

use crate::canvas::{self, ViewCmd};
use crate::layers_ui::{self, LayerCmd};
use crate::prefs::Prefs;
use crate::theme;
use crate::tools::{Tool, ToolState};

struct NewDocForm {
    width: u32,
    height: u32,
    ppi: f32,
    transparent: bool,
}

struct ExportForm {
    jpeg: bool,
    quality: u8,
    background: [u8; 3],
}

/// Atalho: modificadores, tecla e a ação que dispara.
type Shortcut = (Modifiers, Key, fn() -> Action);

enum Action {
    New,
    Open,
    OpenPath(PathBuf),
    Save,
    SaveAs,
    Export,
    Close,
    Undo,
    Redo,
    View(ViewCmd),
    NextTab,
    ClearRecent,
    Tool(Tool),
    SwapColors,
    DefaultColors,
    BrushSize(f32),
    CancelTool,
    Layer(LayerCmd),
    Select(SelCmd),
    CopySel(bool),
    DeleteKey,
    Fill(bool),
    Confirm,
}

pub struct OrvalithApp {
    docs: Vec<OpenDoc>,
    active: usize,
    new_doc: Option<NewDocForm>,
    export: Option<ExportForm>,
    counter: u32,
    /// Mensagem (título, texto) para o usuário: erros, avisos e confirmações.
    message: Option<(String, String)>,
    /// Aba aguardando decisão para fechar com alterações não salvas.
    pending_close: Option<usize>,
    /// Pedido de saída do aplicativo com documentos modificados.
    quit_prompt: bool,
    quit_ok: bool,
    prefs: Prefs,
    prefs_open: bool,
    cursor_doc: Option<[f32; 2]>,
    last_title: String,
    tools: ToolState,
    layer_counter: u32,
    layer_clip: Vec<Layer>,
    flatten_prompt: bool,
    last_autosave: Instant,
    autosave_tx: mpsc::Sender<String>,
    autosave_rx: mpsc::Receiver<String>,
    /// Cópias de recuperação de sessões anteriores aguardando decisão.
    recovery_found: Vec<PathBuf>,
}

impl OrvalithApp {
    pub fn new(prefs: Prefs) -> Self {
        let (autosave_tx, autosave_rx) = mpsc::channel();
        let recovery_found = io::scan_recovery(prefs.recovery_path());
        let tools = ToolState::new(prefs.brush, prefs.fg, prefs.bg);
        Self {
            docs: Vec::new(),
            active: 0,
            new_doc: None,
            export: None,
            counter: 0,
            message: None,
            pending_close: None,
            quit_prompt: false,
            quit_ok: false,
            prefs,
            prefs_open: false,
            cursor_doc: None,
            last_title: String::new(),
            tools,
            layer_counter: 1,
            layer_clip: Vec::new(),
            flatten_prompt: false,
            last_autosave: Instant::now(),
            autosave_tx,
            autosave_rx,
            recovery_found,
        }
    }

    fn open_new_dialog(&mut self) {
        self.new_doc = Some(NewDocForm {
            width: 1920,
            height: 1080,
            ppi: 72.0,
            transparent: false,
        });
    }

    fn create_document(&mut self, form: &NewDocForm) -> Result<(), String> {
        self.counter += 1;
        let name = format!("Sem título-{}", self.counter);
        let bg = (!form.transparent).then_some([255, 255, 255, 255]);
        let mut doc = Document::new(name, form.width, form.height, bg)?;
        doc.ppi = form.ppi;
        self.docs.push(OpenDoc::new(doc));
        self.active = self.docs.len() - 1;
        Ok(())
    }

    /// Fecha a aba; pede decisão se houver alterações não salvas.
    fn request_close(&mut self, index: usize) {
        match self.docs.get(index) {
            Some(d) if d.doc.modified => self.pending_close = Some(index),
            Some(_) => self.close_now(index),
            None => {}
        }
    }

    fn close_now(&mut self, index: usize) {
        if index < self.docs.len() {
            let id = self.docs[index].doc.id.0;
            self.remove_recovery(id);
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

    fn undo_redo(&mut self, redo: bool) {
        if let Some(o) = self.docs.get_mut(self.active) {
            if let Some(qm) = o.qm.as_deref_mut() {
                if redo {
                    qm.history.redo(&mut qm.doc);
                } else {
                    qm.history.undo(&mut qm.doc);
                }
                return;
            }
            if redo {
                o.history.redo(&mut o.doc);
            } else {
                o.history.undo(&mut o.doc);
            }
        }
    }

    fn run_action(&mut self, action: Action, view_cmd: &mut Option<ViewCmd>) {
        match action {
            Action::New => {
                if self.new_doc.is_none() {
                    self.open_new_dialog();
                }
            }
            Action::Open => self.open_dialog(),
            Action::OpenPath(p) => self.open_path(p),
            Action::Save => {
                if !self.docs.is_empty() {
                    self.save_doc(self.active, false);
                }
            }
            Action::SaveAs => {
                if !self.docs.is_empty() {
                    self.save_doc(self.active, true);
                }
            }
            Action::Export => {
                if !self.docs.is_empty() && self.export.is_none() {
                    self.export = Some(ExportForm {
                        jpeg: false,
                        quality: 90,
                        background: [255, 255, 255],
                    });
                }
            }
            Action::Close => self.request_close(self.active),
            Action::Layer(LayerCmd::Flatten) => {
                if !self.docs.is_empty() {
                    self.flatten_prompt = true;
                }
            }
            Action::Layer(cmd) => {
                if self.tools.is_busy() {
                    return;
                }
                if let Some(open) = self.docs.get_mut(self.active) {
                    self.tools.notice =
                        layers_ui::run(open, cmd, &mut self.layer_clip, &mut self.layer_counter);
                }
            }
            Action::Select(c) => self.run_select(c),
            Action::CopySel(cut) => self.copy_cut(cut),
            Action::DeleteKey => self.delete_key(),
            Action::Fill(bg) => self.fill_selection(bg),
            Action::Confirm => {
                if self.tools.sel.active() {
                    if let Some(o) = self.docs.get_mut(self.active) {
                        self.tools.confirm(o);
                    }
                }
            }
            Action::Undo => self.undo_redo(false),
            Action::Redo => self.undo_redo(true),
            Action::View(c) => *view_cmd = Some(c),
            Action::NextTab => {
                if self.docs.len() > 1 {
                    self.active = (self.active + 1) % self.docs.len();
                }
            }
            Action::Tool(t) => self.tools.set_tool(t),
            Action::SwapColors => self.tools.swap_colors(),
            Action::DefaultColors => self.tools.default_colors(),
            Action::BrushSize(f) => self.tools.resize_brush(f),
            Action::CancelTool => {
                if let Some(o) = self.docs.get_mut(self.active) {
                    self.tools.cancel(o);
                }
            }
            Action::ClearRecent => {
                self.prefs.recent.clear();
                self.prefs.save();
            }
        }
    }

    fn collect_shortcuts(ctx: &egui::Context, actions: &mut Vec<Action>) {
        // Atalhos só valem quando nenhum campo de texto tem o foco do teclado.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        const C: Modifiers = Modifiers::COMMAND;
        let cs = Modifiers::COMMAND.plus(Modifiers::SHIFT);
        ctx.input_mut(|i| {
            // Variantes com Shift primeiro: Ctrl+X casaria também com Ctrl+Shift+X.
            let table: [Shortcut; 49] = [
                (cs, Key::I, || Action::Select(SelCmd::Invert)),
                (C, Key::A, || Action::Select(SelCmd::All)),
                (C, Key::D, || Action::Select(SelCmd::Deselect)),
                (Modifiers::ALT, Key::Backspace, || Action::Fill(false)),
                (C, Key::Backspace, || Action::Fill(true)),
                (Modifiers::NONE, Key::Enter, || Action::Confirm),
                (Modifiers::NONE, Key::Q, || {
                    Action::Select(SelCmd::QuickMask)
                }),
                (Modifiers::SHIFT, Key::M, || Action::Tool(Tool::Ellipse)),
                (Modifiers::NONE, Key::M, || Action::Tool(Tool::Marquee)),
                (Modifiers::SHIFT, Key::L, || Action::Tool(Tool::PolyLasso)),
                (Modifiers::NONE, Key::L, || Action::Tool(Tool::Lasso)),
                (Modifiers::NONE, Key::W, || Action::Tool(Tool::Wand)),
                (cs, Key::N, || Action::Layer(LayerCmd::New)),
                (cs, Key::G, || Action::Layer(LayerCmd::Ungroup)),
                (cs, Key::S, || Action::SaveAs),
                (cs, Key::E, || Action::Export),
                (cs, Key::Z, || Action::Redo),
                (C, Key::Y, || Action::Redo),
                (C, Key::Z, || Action::Undo),
                (C, Key::S, || Action::Save),
                (C, Key::O, || Action::Open),
                (C, Key::N, || Action::New),
                (C, Key::W, || Action::Close),
                (Modifiers::CTRL, Key::Tab, || Action::NextTab),
                (C, Key::Num0, || Action::View(ViewCmd::Fit)),
                (C, Key::Num1, || Action::View(ViewCmd::Actual)),
                (C, Key::Equals, || Action::View(ViewCmd::ZoomIn)),
                (C, Key::Plus, || Action::View(ViewCmd::ZoomIn)),
                (C, Key::Minus, || Action::View(ViewCmd::ZoomOut)),
                (C, Key::F4, || Action::Close),
                (Modifiers::NONE, Key::F5, || Action::View(ViewCmd::Fit)),
                // Ferramentas (Shift+G antes de G).
                (C, Key::J, || Action::Layer(LayerCmd::Duplicate)),
                (C, Key::G, || Action::Layer(LayerCmd::Group)),
                (C, Key::E, || Action::Layer(LayerCmd::MergeSelected)),
                (C, Key::CloseBracket, || Action::Layer(LayerCmd::Up)),
                (C, Key::OpenBracket, || Action::Layer(LayerCmd::Down)),
                (Modifiers::NONE, Key::Delete, || Action::DeleteKey),
                (Modifiers::NONE, Key::V, || Action::Tool(Tool::Move)),
                (Modifiers::SHIFT, Key::G, || Action::Tool(Tool::Gradient)),
                (Modifiers::NONE, Key::B, || Action::Tool(Tool::Brush)),
                (Modifiers::NONE, Key::N, || Action::Tool(Tool::Pencil)),
                (Modifiers::NONE, Key::E, || Action::Tool(Tool::Eraser)),
                (Modifiers::NONE, Key::I, || Action::Tool(Tool::Eyedropper)),
                (Modifiers::NONE, Key::G, || Action::Tool(Tool::Bucket)),
                (Modifiers::NONE, Key::X, || Action::SwapColors),
                (Modifiers::NONE, Key::D, || Action::DefaultColors),
                (Modifiers::NONE, Key::OpenBracket, || Action::BrushSize(0.9)),
                (Modifiers::NONE, Key::CloseBracket, || {
                    Action::BrushSize(1.1)
                }),
                (Modifiers::NONE, Key::Escape, || Action::CancelTool),
            ];
            for (m, k, make) in table {
                if i.consume_key(m, k) {
                    actions.push(make());
                }
            }
            // O egui-winit converte Ctrl+C/Ctrl+V em eventos próprios (sem evento de tecla).
            for e in &i.events {
                match e {
                    egui::Event::Copy => actions.push(Action::CopySel(false)),
                    egui::Event::Cut => actions.push(Action::CopySel(true)),
                    egui::Event::Paste(_) | egui::Event::PasteImage(_) => {
                        actions.push(Action::Layer(LayerCmd::Paste));
                    }
                    _ => {}
                }
            }
        });
    }
}

impl eframe::App for OrvalithApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let mut actions: Vec<Action> = Vec::new();

        if let Some(size) = self.window_size(&ctx) {
            self.prefs.window_size = size;
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.docs.iter().any(|d| d.doc.modified) && !self.quit_ok {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.quit_prompt = true;
            } else {
                self.remove_all_own_recovery();
                self.persist_tools();
            }
        }
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            actions.push(Action::OpenPath(f.path().to_path_buf()));
        }
        Self::collect_shortcuts(&ctx, &mut actions);
        let space_down = !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_down(Key::Space));
        let has_doc = !self.docs.is_empty();
        let (can_undo, can_redo, undo_label, redo_label) = match self.docs.get(self.active) {
            Some(d) => (
                d.qm.as_deref()
                    .map_or(&d.history, |q| &q.history)
                    .can_undo(),
                d.qm.as_deref()
                    .map_or(&d.history, |q| &q.history)
                    .can_redo(),
                d.qm.as_deref()
                    .map_or(&d.history, |q| &q.history)
                    .undo_label()
                    .map(str::to_owned),
                d.qm.as_deref()
                    .map_or(&d.history, |q| &q.history)
                    .redo_label()
                    .map(str::to_owned),
            ),
            None => (false, false, None, None),
        };

        egui::Panel::top("menu").show(root, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Arquivo", |ui| {
                    let item = |ui: &mut egui::Ui, label: &str, sc: &str, on: bool| {
                        let r = ui.add_enabled(on, egui::Button::new(label).shortcut_text(sc));
                        if r.clicked() {
                            ui.close();
                        }
                        r.clicked()
                    };
                    if item(ui, "Novo…", "Ctrl+N", true) {
                        actions.push(Action::New);
                    }
                    if item(ui, "Abrir…", "Ctrl+O", true) {
                        actions.push(Action::Open);
                    }
                    ui.add_enabled_ui(!self.prefs.recent.is_empty(), |ui| {
                        ui.menu_button("Abrir recente", |ui| {
                            for r in self.prefs.recent.clone() {
                                if ui.button(&r).clicked() {
                                    actions.push(Action::OpenPath(PathBuf::from(r)));
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui.button("Limpar lista").clicked() {
                                actions.push(Action::ClearRecent);
                                ui.close();
                            }
                        });
                    });
                    ui.separator();
                    if item(ui, "Salvar projeto", "Ctrl+S", has_doc) {
                        actions.push(Action::Save);
                    }
                    if item(ui, "Salvar projeto como…", "Ctrl+Shift+S", has_doc) {
                        actions.push(Action::SaveAs);
                    }
                    if item(ui, "Exportar imagem…", "Ctrl+Shift+E", has_doc) {
                        actions.push(Action::Export);
                    }
                    ui.separator();
                    if item(ui, "Fechar documento", "Ctrl+W", has_doc) {
                        actions.push(Action::Close);
                    }
                    ui.separator();
                    if ui.button("Sair").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Editar", |ui| {
                    let u = format!("Desfazer {}", undo_label.as_deref().unwrap_or(""));
                    if ui
                        .add_enabled(
                            can_undo,
                            egui::Button::new(u.trim()).shortcut_text("Ctrl+Z"),
                        )
                        .clicked()
                    {
                        actions.push(Action::Undo);
                        ui.close();
                    }
                    let r = format!("Refazer {}", redo_label.as_deref().unwrap_or(""));
                    if ui
                        .add_enabled(
                            can_redo,
                            egui::Button::new(r.trim()).shortcut_text("Ctrl+Shift+Z"),
                        )
                        .clicked()
                    {
                        actions.push(Action::Redo);
                        ui.close();
                    }
                    ui.separator();
                    let edit_items: [(&str, &str, Action); 6] = [
                        ("Cortar", "Ctrl+X", Action::CopySel(true)),
                        ("Copiar", "Ctrl+C", Action::CopySel(false)),
                        ("Colar", "Ctrl+V", Action::Layer(LayerCmd::Paste)),
                        ("Apagar seleção", "Delete", Action::DeleteKey),
                        (
                            "Preencher com a cor de frente",
                            "Alt+Backspace",
                            Action::Fill(false),
                        ),
                        (
                            "Preencher com a cor de fundo",
                            "Ctrl+Backspace",
                            Action::Fill(true),
                        ),
                    ];
                    for (label, sc, act) in edit_items {
                        if ui
                            .add_enabled(has_doc, egui::Button::new(label).shortcut_text(sc))
                            .clicked()
                        {
                            actions.push(act);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Preferências…").clicked() {
                        self.prefs_open = true;
                        ui.close();
                    }
                });
                ui.menu_button("Selecionar", |ui| {
                    let items: [(&str, &str, SelCmd); 8] = [
                        ("Tudo", "Ctrl+A", SelCmd::All),
                        ("Desmarcar", "Ctrl+D", SelCmd::Deselect),
                        ("Inverter", "Ctrl+Shift+I", SelCmd::Invert),
                        ("Faixa de cores…", "", SelCmd::Range),
                        (
                            "Modificar (expandir, contrair, suavizar, pena)…",
                            "",
                            SelCmd::Modify,
                        ),
                        ("Transformar seleção…", "", SelCmd::Transform),
                        ("Canais de seleção (salvar/carregar)…", "", SelCmd::Channels),
                        ("Máscara rápida", "Q", SelCmd::QuickMask),
                    ];
                    for (label, sc, cmd) in items {
                        if ui
                            .add_enabled(has_doc, egui::Button::new(label).shortcut_text(sc))
                            .clicked()
                        {
                            actions.push(Action::Select(cmd));
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Camada", |ui| {
                    let items: [(&str, &str, LayerCmd); 13] = [
                        ("Nova camada", "Ctrl+Shift+N", LayerCmd::New),
                        ("Novo grupo", "", LayerCmd::NewGroup),
                        ("Duplicar", "Ctrl+J", LayerCmd::Duplicate),
                        ("Excluir", "Delete", LayerCmd::Delete),
                        ("Agrupar seleção", "Ctrl+G", LayerCmd::Group),
                        ("Desagrupar", "Ctrl+Shift+G", LayerCmd::Ungroup),
                        ("Subir", "Ctrl+]", LayerCmd::Up),
                        ("Descer", "Ctrl+[", LayerCmd::Down),
                        ("Mesclar selecionadas", "Ctrl+E", LayerCmd::MergeSelected),
                        ("Mesclar visíveis", "", LayerCmd::MergeVisible),
                        ("Achatar imagem…", "", LayerCmd::Flatten),
                        ("Copiar camadas", "Ctrl+C", LayerCmd::Copy),
                        ("Colar camadas", "Ctrl+V", LayerCmd::Paste),
                    ];
                    for (label, sc, cmd) in &items {
                        if ui
                            .add_enabled(has_doc, egui::Button::new(*label).shortcut_text(*sc))
                            .clicked()
                        {
                            actions.push(Action::Layer(*cmd));
                            ui.close();
                        }
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
                            actions.push(Action::View(cmd));
                            ui.close();
                        }
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
                        ui.label(format!(
                            "{} × {} px · {:.0} ppi",
                            d.doc.width, d.doc.height, d.doc.ppi
                        ));
                        if let Some(p) = self.cursor_doc {
                            ui.separator();
                            ui.label(format!("x: {}  y: {}", p[0].floor(), p[1].floor()));
                        }
                        if let Some(r) = d.sel_info {
                            ui.separator();
                            ui.label(format!("Seleção: {} × {} em ({}, {})", r.w, r.h, r.x, r.y));
                        }
                        if d.qm.is_some() {
                            ui.separator();
                            ui.label(egui::RichText::new("Máscara rápida").color(theme::COPPER));
                        }
                        if let Some(n) = &self.tools.notice {
                            ui.separator();
                            ui.label(egui::RichText::new(n).color(theme::COPPER));
                        }
                    }
                });
            });

            egui::Panel::top("options").show(root, |ui| {
                if self.tools.options_ui(ui, &mut self.prefs.brush_presets) {
                    self.persist_tools();
                }
            });

            egui::Panel::left("tools")
                .resizable(false)
                .default_size(110.0)
                .show(root, |ui| {
                    for t in Tool::ALL {
                        let r = ui
                            .selectable_label(self.tools.tool == t, t.label())
                            .on_hover_text(format!("{} ({})", t.label(), t.shortcut()));
                        if r.clicked() {
                            actions.push(Action::Tool(t));
                        }
                    }
                    ui.separator();
                    if self.tools.colors_ui(ui, &mut self.prefs.swatches) {
                        self.persist_tools();
                    }
                });

            egui::Panel::right("layers")
                .resizable(true)
                .default_size(270.0)
                .show(root, |ui| {
                    ui.heading("Camadas");
                    ui.separator();
                    let busy = self.tools.is_busy();
                    if let Some(open) = self.docs.get_mut(self.active) {
                        for c in layers_ui::show(ui, open, busy) {
                            actions.push(Action::Layer(c));
                        }
                    }
                });
        }

        egui::CentralPanel::default().show(root, |ui| {
            if let Some(open) = self.docs.get_mut(self.active) {
                let cmd = actions.iter().rev().find_map(|a| match a {
                    Action::View(c) => Some(*c),
                    _ => None,
                });
                let overlay = self.tools.overlay(self.cursor_doc);
                let out = canvas::show(ui, open, cmd, space_down, &overlay);
                self.cursor_doc = out.hover;
                if self.tools.sel.picking() {
                    self.tools.sel.range_pick(open, &out);
                } else if let Some(qm) = open.qm.as_deref_mut() {
                    let t = self.tools.tool;
                    if crate::select::is_selection_tool(t) || t == Tool::Move {
                        self.tools.notice = Some(
                            "Máscara rápida: use pincel, lápis, balde ou gradiente (Q sai).".into(),
                        );
                    } else if t == Tool::Eraser {
                        self.tools.notice =
                            Some("Na máscara rápida, pinte de branco para selecionar.".into());
                    } else {
                        let (fg, bg) = (self.tools.fg, self.tools.bg);
                        self.tools.fg = gray(fg);
                        self.tools.bg = gray(bg);
                        self.tools.handle(qm, &out);
                        self.tools.fg = fg;
                        self.tools.bg = bg;
                    }
                } else {
                    self.tools.handle(open, &out);
                }
            } else {
                self.cursor_doc = None;
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.25);
                    ui.heading(egui::RichText::new("Orvalith").color(theme::ACCENT));
                    ui.label("Editor de imagens local, sem login.");
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() / 2.0 - 110.0).max(0.0));
                        if ui.button("Novo documento").clicked() {
                            actions.push(Action::New);
                        }
                        if ui.button("Abrir arquivo").clicked() {
                            actions.push(Action::Open);
                        }
                    });
                    if !self.prefs.recent.is_empty() {
                        ui.add_space(16.0);
                        ui.label("Recentes");
                        for r in self.prefs.recent.clone() {
                            if ui.link(&r).clicked() {
                                actions.push(Action::OpenPath(PathBuf::from(r)));
                            }
                        }
                    }
                });
            }
        });

        let mut view_cmd = None; // já consumido pelo canvas acima
        for a in actions {
            if !matches!(a, Action::View(_)) {
                self.run_action(a, &mut view_cmd);
            }
        }

        self.autosave_tick(&ctx);
        self.update_title(&ctx);
        self.show_dialogs(&ctx);
        if let Some(open) = self.docs.get_mut(self.active) {
            self.tools.sel.show_dialogs(&ctx, open, self.tools.fg);
        }
    }
}

impl OrvalithApp {
    /// Copia o estado das ferramentas (pincel, cores) para as preferências e grava.
    fn persist_tools(&mut self) {
        self.prefs.brush = self.tools.brush;
        self.prefs.fg = self.tools.fg;
        self.prefs.bg = self.tools.bg;
        self.prefs.save();
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.docs.get(self.active) {
            Some(d) => format!(
                "{}{} — Orvalith",
                d.doc.name,
                if d.doc.modified { " *" } else { "" }
            ),
            None => "Orvalith".to_string(),
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

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
                    ui.horizontal(|ui| {
                        ui.label("Resolução (ppi)");
                        ui.add(egui::DragValue::new(&mut form.ppi).range(1.0..=2400.0));
                    });
                    ui.checkbox(&mut form.transparent, "Fundo transparente");
                    ui.horizontal(|ui| {
                        create = ui.button("Criar").clicked();
                        cancel = ui.button("Cancelar").clicked();
                    });
                });
            if create {
                if let Err(e) = self.create_document(&form) {
                    self.show_message("Erro", e);
                }
            } else if !cancel {
                self.new_doc = Some(form);
            }
        }

        if let Some(mut form) = self.export.take() {
            let mut go = false;
            let mut cancel = false;
            egui::Window::new("Exportar imagem")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut form.jpeg, false, "PNG");
                        ui.selectable_value(&mut form.jpeg, true, "JPEG");
                    });
                    if form.jpeg {
                        ui.horizontal(|ui| {
                            ui.label("Qualidade");
                            ui.add(egui::Slider::new(&mut form.quality, 1..=100));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Fundo");
                            ui.color_edit_button_srgb(&mut form.background);
                        });
                        ui.small("JPEG não tem transparência: áreas transparentes são compostas sobre o fundo.");
                    } else {
                        ui.small("PNG preserva a transparência (8 bits por canal).");
                    }
                    ui.small("Exportar não altera o projeto nem o estado de alterações não salvas.");
                    ui.horizontal(|ui| {
                        go = ui.button("Escolher destino…").clicked();
                        cancel = ui.button("Cancelar").clicked();
                    });
                });
            if go {
                self.export_doc(&form);
            } else if !cancel {
                self.export = Some(form);
            }
        }

        if let Some(i) = self.pending_close {
            let name = self
                .docs
                .get(i)
                .map(|d| d.doc.name.clone())
                .unwrap_or_default();
            let (mut save, mut discard, mut cancel) = (false, false, false);
            egui::Window::new("Alterações não salvas")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("\"{name}\" tem alterações não salvas."));
                    ui.horizontal(|ui| {
                        save = ui.button("Salvar").clicked();
                        discard = ui.button("Descartar").clicked();
                        cancel = ui.button("Cancelar").clicked();
                    });
                });
            if save {
                if self.save_doc(i, false) {
                    self.close_now(i);
                    self.pending_close = None;
                }
            } else if discard {
                self.close_now(i);
                self.pending_close = None;
            } else if cancel {
                self.pending_close = None;
            }
        }

        if self.flatten_prompt {
            let (mut yes, mut no) = (false, false);
            egui::Window::new("Achatar imagem")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Todas as camadas visíveis serão mescladas em uma só. Camadas ocultas serão descartadas. Dá para desfazer com Ctrl+Z.");
                    ui.horizontal(|ui| {
                        yes = ui.button("Achatar").clicked();
                        no = ui.button("Cancelar").clicked();
                    });
                });
            if yes {
                if let Some(open) = self.docs.get_mut(self.active) {
                    self.tools.notice = layers_ui::run(
                        open,
                        LayerCmd::Flatten,
                        &mut self.layer_clip,
                        &mut self.layer_counter,
                    );
                }
            }
            if yes || no {
                self.flatten_prompt = false;
            }
        }

        if self.quit_prompt {
            let (mut quit, mut cancel) = (false, false);
            egui::Window::new("Sair do Orvalith")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Há documentos com alterações não salvas. Cancele para salvá-los.");
                    ui.horizontal(|ui| {
                        quit = ui.button("Sair sem salvar").clicked();
                        cancel = ui.button("Cancelar").clicked();
                    });
                });
            if quit {
                self.quit_ok = true;
                self.quit_prompt = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if cancel {
                self.quit_prompt = false;
            }
        }

        if !self.recovery_found.is_empty() {
            let (mut restore, mut discard, mut later) = (false, false, false);
            let n = self.recovery_found.len();
            egui::Window::new("Recuperação de sessão")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Foram encontradas {n} cópia(s) de recuperação de uma sessão anterior."
                    ));
                    ui.horizontal(|ui| {
                        restore = ui.button("Recuperar").clicked();
                        discard = ui.button("Descartar").clicked();
                        later = ui.button("Decidir depois").clicked();
                    });
                });
            if restore {
                self.restore_recovery();
            } else if discard {
                self.clear_recovery_files();
            } else if later {
                self.recovery_found.clear();
            }
        }

        if self.prefs_open {
            self.prefs_window(ctx);
        }

        if let Some((title, text)) = self.message.clone() {
            let mut ok = false;
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(text);
                    ok = ui.button("OK").clicked();
                });
            if ok {
                self.message = None;
            }
        }
    }

    fn prefs_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let before = self.prefs.clone();
        let mut clear_recovery = false;
        egui::Window::new("Preferências")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.checkbox(&mut self.prefs.dark_theme, "Tema escuro");
                ui.horizontal(|ui| {
                    ui.label("Escala da interface");
                    ui.add(egui::Slider::new(&mut self.prefs.ui_scale, 0.75..=2.0).step_by(0.05));
                });
                ui.separator();
                ui.checkbox(
                    &mut self.prefs.autosave_enabled,
                    "Cópias de recuperação automáticas (autosave)",
                );
                ui.add_enabled_ui(self.prefs.autosave_enabled, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Intervalo (s)");
                        ui.add(
                            egui::DragValue::new(&mut self.prefs.autosave_secs).range(15..=3600),
                        );
                    });
                });
                let dir = self
                    .prefs
                    .recovery_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                ui.small(format!("Pasta: {dir}"));
                ui.horizontal(|ui| {
                    if ui.button("Escolher pasta…").clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            self.prefs.recovery_dir = Some(p.display().to_string());
                        }
                    }
                    if ui.button("Apagar cópias de recuperação").clicked() {
                        clear_recovery = true;
                    }
                });
                ui.separator();
                ui.checkbox(&mut self.prefs.recent_enabled, "Lembrar arquivos recentes");
                if !self.prefs.recent_enabled {
                    self.prefs.recent.clear();
                }
                if ui.button("Limpar lista de recentes").clicked() {
                    self.prefs.recent.clear();
                }
                ui.small("As preferências ficam somente neste computador.");
            });
        if self.prefs.dark_theme != before.dark_theme {
            theme::apply(ctx, self.prefs.dark_theme);
        }
        if (self.prefs.ui_scale - before.ui_scale).abs() > f32::EPSILON {
            ctx.set_zoom_factor(self.prefs.ui_scale);
        }
        if clear_recovery {
            let n = self.clear_recovery_files();
            for d in &mut self.docs {
                d.autosaved_rev = None;
            }
            self.show_message(
                "Cópias de recuperação",
                format!("{n} arquivo(s) apagado(s)."),
            );
        }
        if self.prefs != before || !open {
            self.prefs.save();
        }
        self.prefs_open = open;
    }
}
