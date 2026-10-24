//! Estado da aplicação e interface: menus, abas, camadas, diálogos, atalhos e status.

mod actionsui;
mod commands;
mod geocmds;
mod io;
mod pdfs;
mod selops;
mod timeline;
mod videoui;

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use eframe::egui::{self, Key};
use orvalith_core::{Document, Layer};

use crate::canvas::OpenDoc;
use geocmds::GeoCmd;
use selops::SelCmd;

use crate::adjustui::{self, AdjSession, ColorAction, ColorDialog, HistState};
use crate::canvas::{self, RulerUnit, ViewCmd, ViewOpts};
use crate::compsui::{self, CompsUi};
use crate::filterui::{self, FilterSession};
use crate::geometry::TMode;
use crate::layers_ui::{self, LayerCmd};
use crate::prefs::Prefs;
use crate::theme;
use crate::tools::gray;
use crate::tools::{Tool, ToolState};
use crate::vectool::VecState;
use orvalith_core::docops::{AlignKind, Orient};
use orvalith_core::maskops::MaskInit;
use orvalith_core::vector::BoolOp;

struct NewDocForm {
    width: u32,
    height: u32,
    ppi: f32,
    transparent: bool,
}

struct ExportForm {
    color: orvalith_core::imageio::ColorOut,
    format: orvalith_core::formats::Format,
    quality: u8,
    background: [u8; 3],
    /// 0 = tamanho original.
    max_side: u32,
    sprite_cols: u32,
    estimate: Option<String>,
}

impl ExportForm {
    fn opts(&self) -> orvalith_core::formats::ExportOpts {
        orvalith_core::formats::ExportOpts {
            quality: self.quality,
            background: self.background,
            color: self.color,
            max_side: (self.max_side > 0).then_some(self.max_side),
        }
    }
}

/// O que o lote exporta.
#[derive(Clone, Copy)]
pub(crate) enum BatchKind {
    AllDocs,
    Layers,
    Sprites(u32),
}

#[allow(dead_code)] // Ajuda, ações, lote, predefinições e histórico chegam nas próximas subetapas.
enum Action {
    New,
    Open,
    OpenPath(PathBuf),
    Save,
    SaveAs,
    Export,
    ImportPdf,
    ExportPdf,
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
    Geo(GeoCmd),
    AdjStart(usize),
    FilterStart(usize),
    AdjLayer(usize),
    ToggleHist,
    ToggleTimeline,
    ToggleVideo,
    FrameStep(i32),
    ToggleComps,
    PlaceObject(bool),
    ColorDlg,
    Palette,
    ShortcutsEditor,
    Help,
    Macros,
    Batch,
    Presets,
    History,
    OpenExample(usize),
}

pub struct OrvalithApp {
    docs: Vec<OpenDoc>,
    active: usize,
    new_doc: Option<NewDocForm>,
    export: Option<ExportForm>,
    pdf_import: Option<pdfs::PdfImport>,
    timeline: timeline::TimelineUi,
    cmd: commands::CmdUi,
    actions: actionsui::ActionsUi,
    batch: actionsui::BatchUi,
    keymap: commands::Keymap,
    queued: Vec<Action>,
    video: videoui::VideoUi,
    pdf_export: Option<pdfs::PdfExport>,
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
    view_opts: ViewOpts,
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
    /// Ajuste destrutivo em andamento: (id do documento, sessão).
    adj_session: Option<(u64, AdjSession)>,
    filter_session: Option<(u64, FilterSession)>,
    hist: HistState,
    comps_ui: CompsUi,
    color_dlg: ColorDialog,
}

impl OrvalithApp {
    pub fn new(prefs: Prefs) -> Self {
        let (autosave_tx, autosave_rx) = mpsc::channel();
        let recovery_found = io::scan_recovery(prefs.recovery_path());
        let tools = ToolState::new(prefs.brush, prefs.fg, prefs.bg);
        crate::styleui::load_presets(prefs.style_presets.clone());
        let prefs_keymap = prefs.keymap.clone();
        Self {
            docs: Vec::new(),
            active: 0,
            new_doc: None,
            export: None,
            pdf_import: None,
            timeline: timeline::TimelineUi::default(),
            cmd: commands::CmdUi::default(),
            actions: actionsui::ActionsUi::default(),
            batch: actionsui::BatchUi::default(),
            keymap: commands::Keymap::build(&prefs_keymap),
            queued: Vec::new(),
            video: videoui::VideoUi::default(),
            pdf_export: None,
            counter: 0,
            message: None,
            pending_close: None,
            quit_prompt: false,
            quit_ok: false,
            prefs,
            prefs_open: false,
            cursor_doc: None,
            view_opts: ViewOpts::default(),
            last_title: String::new(),
            tools,
            layer_counter: 1,
            layer_clip: Vec::new(),
            flatten_prompt: false,
            last_autosave: Instant::now(),
            autosave_tx,
            autosave_rx,
            recovery_found,
            adj_session: None,
            filter_session: None,
            hist: HistState::default(),
            comps_ui: CompsUi::default(),
            color_dlg: ColorDialog::default(),
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
                        color: orvalith_core::imageio::ColorOut::Embed,
                        format: orvalith_core::formats::Format::Png,
                        quality: 90,
                        background: [255, 255, 255],
                        max_side: 0,
                        sprite_cols: 4,
                        estimate: None,
                    });
                }
            }
            Action::ImportPdf => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("PDF", &["pdf"])
                    .pick_file()
                {
                    self.start_pdf_import(p);
                }
            }
            Action::ExportPdf => self.start_pdf_export(),
            Action::Close => self.request_close(self.active),
            Action::Layer(LayerCmd::RefineMask) => {
                if let Some(o) = self.docs.get(self.active) {
                    match o.active_idx().map(|i| &o.doc.layers[i]) {
                        Some(l) if l.mask.is_some() => {
                            self.tools.sel.refine = Some((
                                crate::select::RefineTarget::Mask(l.id),
                                orvalith_core::maskops::RefineParams::default(),
                            ));
                        }
                        _ => self.tools.notice = Some("A camada ativa não tem máscara.".into()),
                    }
                }
            }
            Action::Layer(LayerCmd::Flatten) => {
                if !self.docs.is_empty() {
                    self.flatten_prompt = true;
                }
            }
            Action::Layer(LayerCmd::RelinkObject) => self.relink_object(),
            Action::Layer(cmd) => {
                if self.tools.is_busy() {
                    return;
                }
                if let Some(open) = self.docs.get_mut(self.active) {
                    self.tools.notice =
                        layers_ui::run(open, cmd, &mut self.layer_clip, &mut self.layer_counter);
                }
            }
            Action::FilterStart(kind) => {
                if self.tools.is_busy() {
                    return;
                }
                self.end_edit_sessions();
                if let Some(o) = self.docs.get(self.active) {
                    match FilterSession::begin(o, kind) {
                        Ok(s) => self.filter_session = Some((o.doc.id.0, s)),
                        Err(e) => self.tools.notice = Some(e),
                    }
                }
            }
            Action::AdjStart(kind) => {
                if self.tools.is_busy() {
                    return;
                }
                self.end_edit_sessions();
                if let Some(o) = self.docs.get_mut(self.active) {
                    match AdjSession::begin(o, kind) {
                        Ok(s) => self.adj_session = Some((o.doc.id.0, s)),
                        Err(e) => self.tools.notice = Some(e),
                    }
                }
            }
            Action::AdjLayer(kind) => {
                if self.tools.is_busy() {
                    return;
                }
                if let Some(o) = self.docs.get_mut(self.active) {
                    adjustui::new_adjustment_layer(o, kind);
                }
            }
            Action::ToggleHist => self.hist.open = !self.hist.open,
            Action::ToggleTimeline => {
                self.timeline.open = !self.timeline.open;
                self.timeline.playing = false;
            }
            Action::ToggleVideo => self.video.open = !self.video.open,
            Action::FrameStep(d) => self.timeline_step(d),
            Action::ToggleComps => self.comps_ui.open = !self.comps_ui.open,
            Action::PlaceObject(linked) => self.place_object(linked),
            Action::ColorDlg => self.color_dlg.open = !self.color_dlg.open,
            Action::Palette => {
                self.cmd.palette = !self.cmd.palette;
            }
            Action::ShortcutsEditor => self.cmd.editor = !self.cmd.editor,
            Action::Macros => self.actions.open = !self.actions.open,
            Action::Batch => self.batch.open = !self.batch.open,
            Action::Help | Action::Presets | Action::History | Action::OpenExample(_) => {}
            Action::Select(c) => self.run_select(c),
            Action::CopySel(cut) => self.copy_cut(cut),
            Action::DeleteKey => self.delete_key(),
            Action::Fill(bg) => self.fill_selection(bg),
            Action::Geo(c) => self.run_geo(c),
            Action::Confirm => {
                if self.tools.needs_confirm() {
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
            Action::Tool(t) => {
                if let Some(o) = self.docs.get_mut(self.active) {
                    self.tools.liq.commit(o);
                    let (fg, bg) = (self.tools.fg, self.tools.bg);
                    self.tools.vec.commit_all(o, fg, bg);
                }
                self.tools.set_tool(t);
            }
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

    fn collect_shortcuts(
        ctx: &egui::Context,
        keymap: &commands::Keymap,
        actions: &mut Vec<Action>,
    ) {
        // Atalhos só valem quando nenhum campo de texto tem o foco do teclado.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        ctx.input_mut(|i| {
            actions.extend(keymap.collect(i));
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
        if !self.cmd.capturing() {
            Self::collect_shortcuts(&ctx, &self.keymap, &mut actions);
        }
        actions.append(&mut self.queued);
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
                    if item(ui, "Importar PDF…", "", true) {
                        actions.push(Action::ImportPdf);
                    }
                    if item(ui, "Exportar PDF…", "", has_doc) {
                        actions.push(Action::ExportPdf);
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
                    if ui.button("Paleta de comandos…  Ctrl+Shift+P").clicked() {
                        actions.push(Action::Palette);
                        ui.close();
                    }
                    if ui.button("Ações (macros)…").clicked() {
                        actions.push(Action::Macros);
                        ui.close();
                    }
                    if ui.button("Processamento em lote…").clicked() {
                        actions.push(Action::Batch);
                        ui.close();
                    }
                    if ui.button("Atalhos de teclado…").clicked() {
                        actions.push(Action::ShortcutsEditor);
                        ui.close();
                    }
                    if ui.button("Preferências…").clicked() {
                        self.prefs_open = true;
                        ui.close();
                    }
                });
                ui.menu_button("Selecionar", |ui| {
                    let items: [(&str, &str, SelCmd); 9] = [
                        ("Refinar borda…", "", SelCmd::Refine),
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
                    let items: [(&str, &str, LayerCmd); 17] = [
                        (
                            "Máscara: revelar tudo",
                            "",
                            LayerCmd::AddMask(MaskInit::Reveal),
                        ),
                        (
                            "Máscara: ocultar tudo",
                            "",
                            LayerCmd::AddMask(MaskInit::Hide),
                        ),
                        (
                            "Máscara: da seleção",
                            "",
                            LayerCmd::AddMask(MaskInit::FromSelection),
                        ),
                        (
                            "Recortar na camada abaixo",
                            "Ctrl+Alt+G",
                            LayerCmd::ToggleClip,
                        ),
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
                ui.menu_button("Animação", |ui| {
                    if ui
                        .add_enabled(has_doc, egui::Button::new("Linha do tempo"))
                        .clicked()
                    {
                        actions.push(Action::ToggleTimeline);
                        ui.close();
                    }
                    if ui
                        .add_enabled(has_doc, egui::Button::new("Vídeo…"))
                        .clicked()
                    {
                        actions.push(Action::ToggleVideo);
                        ui.close();
                    }
                    ui.small("Vídeo exige o FFmpeg instalado (veja docs/MEDIA.md).");
                });
                ui.menu_button("Objetos", |ui| {
                    if ui
                        .add_enabled(has_doc, egui::Button::new("Composições de camadas…"))
                        .clicked()
                    {
                        actions.push(Action::ToggleComps);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            has_doc,
                            egui::Button::new("Converter camada em objeto inteligente"),
                        )
                        .clicked()
                    {
                        actions.push(Action::Layer(LayerCmd::ToObject));
                        ui.close();
                    }
                    if ui
                        .add_enabled(has_doc, egui::Button::new("Colocar imagem (embutida)…"))
                        .clicked()
                    {
                        actions.push(Action::PlaceObject(false));
                        ui.close();
                    }
                    if ui
                        .add_enabled(has_doc, egui::Button::new("Colocar imagem vinculada…"))
                        .clicked()
                    {
                        actions.push(Action::PlaceObject(true));
                        ui.close();
                    }
                    ui.separator();
                    ui.small("Transformação e filtros do objeto ficam no painel da camada.");
                });
                ui.menu_button("Preenchimento", |ui| {
                    for (i, n) in orvalith_core::source::FILL_KINDS.iter().enumerate() {
                        if ui
                            .add_enabled(has_doc, egui::Button::new(format!("Nova camada: {n}")))
                            .clicked()
                        {
                            actions.push(Action::Layer(LayerCmd::NewFill(i)));
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.small("Estilos de camada ficam no painel da camada ativa.");
                });
                ui.menu_button("Texto e formas", |ui| {
                    let items: [(&str, LayerCmd); 7] = [
                        ("Rasterizar camada", LayerCmd::Rasterize),
                        ("Combinar: união", LayerCmd::Boolean(BoolOp::Union)),
                        ("Combinar: subtrair", LayerCmd::Boolean(BoolOp::Subtract)),
                        ("Combinar: interseção", LayerCmd::Boolean(BoolOp::Intersect)),
                        ("Combinar: exclusão", LayerCmd::Boolean(BoolOp::Exclude)),
                        ("Forma → seleção", LayerCmd::PathToSelection),
                        (
                            "Forma → máscara vetorial da camada abaixo",
                            LayerCmd::VectorMask,
                        ),
                    ];
                    for (label, cmd) in &items {
                        if ui.add_enabled(has_doc, egui::Button::new(*label)).clicked() {
                            actions.push(Action::Layer(*cmd));
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.small("Combinar usa as formas selecionadas (Ctrl+clique):");
                    ui.small("a mais baixa é a base; a mais alta é o operando.");
                });
                ui.menu_button("Filtro", |ui| {
                    for (i, name) in orvalith_core::filter::KINDS.iter().enumerate() {
                        if ui
                            .add_enabled(has_doc, egui::Button::new(format!("{name}…")))
                            .clicked()
                        {
                            actions.push(Action::FilterStart(i));
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.small("Os filtros agem na camada ativa (e na seleção).");
                });
                ui.menu_button("Ajustes", |ui| {
                    for (i, name) in orvalith_core::adjust::KINDS.iter().enumerate() {
                        if ui
                            .add_enabled(has_doc, egui::Button::new(format!("{name}…")))
                            .clicked()
                        {
                            actions.push(Action::AdjStart(i));
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.menu_button("Nova camada de ajuste", |ui| {
                        for (i, name) in orvalith_core::adjust::KINDS.iter().enumerate() {
                            if ui.add_enabled(has_doc, egui::Button::new(*name)).clicked() {
                                actions.push(Action::AdjLayer(i));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if ui
                        .add_enabled(has_doc, egui::Button::new("Histograma"))
                        .clicked()
                    {
                        actions.push(Action::ToggleHist);
                        ui.close();
                    }
                    if ui
                        .add_enabled(has_doc, egui::Button::new("Perfil de cor…"))
                        .clicked()
                    {
                        actions.push(Action::ColorDlg);
                        ui.close();
                    }
                });
                ui.menu_button("Imagem", |ui| {
                    let items: [(&str, &str, GeoCmd); 11] = [
                        ("Tamanho da imagem…", "Ctrl+Alt+I", GeoCmd::ImageSize),
                        ("Tamanho da tela…", "Ctrl+Alt+C", GeoCmd::CanvasSize),
                        (
                            "Girar tela 90° horário",
                            "",
                            GeoCmd::OrientCanvas(Orient::Cw90),
                        ),
                        (
                            "Girar tela 90° anti-horário",
                            "",
                            GeoCmd::OrientCanvas(Orient::Ccw90),
                        ),
                        ("Girar tela 180°", "", GeoCmd::OrientCanvas(Orient::Rot180)),
                        (
                            "Espelhar tela horizontalmente",
                            "",
                            GeoCmd::OrientCanvas(Orient::FlipH),
                        ),
                        (
                            "Espelhar tela verticalmente",
                            "",
                            GeoCmd::OrientCanvas(Orient::FlipV),
                        ),
                        ("Endireitar…", "", GeoCmd::Straighten),
                        ("Recortar para a seleção", "", GeoCmd::CropToSelection),
                        ("Limpar guias", "", GeoCmd::ClearGuides),
                        (
                            "Transformação livre",
                            "Ctrl+T",
                            GeoCmd::Transform(TMode::Free),
                        ),
                    ];
                    for (label, sc, cmd) in items {
                        if ui
                            .add_enabled(has_doc, egui::Button::new(label).shortcut_text(sc))
                            .clicked()
                        {
                            actions.push(Action::Geo(cmd));
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.add_enabled_ui(has_doc, |ui| {
                        ui.menu_button("Transformar camada", |ui| {
                            let tr: [(&str, GeoCmd); 9] = [
                                ("Inclinar", GeoCmd::Transform(TMode::Skew)),
                                ("Distorcer", GeoCmd::Transform(TMode::Distort)),
                                ("Perspectiva", GeoCmd::Transform(TMode::Perspective)),
                                ("Deformar com malha", GeoCmd::Transform(TMode::Warp)),
                                ("Espelhar horizontalmente", GeoCmd::FlipH),
                                ("Espelhar verticalmente", GeoCmd::FlipV),
                                ("Girar 90° horário", GeoCmd::RotateLayer(90.0)),
                                ("Girar 90° anti-horário", GeoCmd::RotateLayer(-90.0)),
                                ("Girar 180°", GeoCmd::RotateLayer(180.0)),
                            ];
                            for (label, cmd) in tr {
                                if ui.button(label).clicked() {
                                    actions.push(Action::Geo(cmd));
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button("Alinhar camadas", |ui| {
                            let al: [(&str, GeoCmd); 8] = [
                                ("À esquerda", GeoCmd::Align(AlignKind::Left)),
                                ("Centro horizontal", GeoCmd::Align(AlignKind::HCenter)),
                                ("À direita", GeoCmd::Align(AlignKind::Right)),
                                ("Ao topo", GeoCmd::Align(AlignKind::Top)),
                                ("Centro vertical", GeoCmd::Align(AlignKind::VCenter)),
                                ("À base", GeoCmd::Align(AlignKind::Bottom)),
                                ("Distribuir na horizontal", GeoCmd::Distribute(true)),
                                ("Distribuir na vertical", GeoCmd::Distribute(false)),
                            ];
                            for (label, cmd) in al {
                                if ui.button(label).clicked() {
                                    actions.push(Action::Geo(cmd));
                                    ui.close();
                                }
                            }
                        });
                    });
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
                    ui.separator();
                    let vo = self.view_opts;
                    for (label, sc, on, cmd) in [
                        ("Réguas", "Ctrl+R", vo.rulers, GeoCmd::ToggleRulers),
                        ("Grade", "Ctrl+'", vo.grid, GeoCmd::ToggleGrid),
                        ("Guias", "Ctrl+;", vo.guides, GeoCmd::ToggleGuides),
                        ("Encaixar", "Ctrl+Shift+;", vo.snap, GeoCmd::ToggleSnap),
                    ] {
                        let text = format!("{} {label}", if on { "✓" } else { "  " });
                        if ui.add(egui::Button::new(text).shortcut_text(sc)).clicked() {
                            actions.push(Action::Geo(cmd));
                            ui.close();
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Grade (px)");
                        ui.add(
                            egui::DragValue::new(&mut self.view_opts.snap_opts.grid_size)
                                .range(2.0..=1000.0),
                        );
                    });
                    ui.menu_button("Unidade da régua", |ui| {
                        for u in [RulerUnit::Px, RulerUnit::In, RulerUnit::Cm] {
                            if ui
                                .selectable_label(self.view_opts.unit == u, u.label())
                                .clicked()
                            {
                                actions.push(Action::Geo(GeoCmd::SetUnit(u)));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    for (label, sc, cmd) in [
                        ("Girar visão 15° horário", "Alt+→", ViewCmd::Rotate(15.0)),
                        (
                            "Girar visão 15° anti-horário",
                            "Alt+←",
                            ViewCmd::Rotate(-15.0),
                        ),
                        (
                            "Redefinir rotação da visão",
                            "Alt+↑",
                            ViewCmd::ResetRotation,
                        ),
                    ] {
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
                        if let Some(p) = &d.doc.icc {
                            ui.separator();
                            ui.label(format!("Perfil: {}", p.name));
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

            self.timeline_tick(&ctx);
            self.timeline_panel(root);

            egui::Panel::top("options").show(root, |ui| {
                if let (Some(t), Some(open)) = (
                    self.tools.geo.transform.as_mut(),
                    self.docs.get_mut(self.active),
                ) {
                    let (changed, apply, cancel) = t.options_ui(ui);
                    if changed {
                        t.refresh(open);
                    }
                    if apply {
                        actions.push(Action::Confirm);
                    }
                    if cancel {
                        actions.push(Action::CancelTool);
                    }
                } else if self.tools.options_ui(
                    ui,
                    &mut self.prefs.brush_presets,
                    self.docs.get_mut(self.active),
                ) {
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
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(open) = self.docs.get_mut(self.active) {
                            for c in layers_ui::show(ui, open, busy) {
                                actions.push(Action::Layer(c));
                            }
                        }
                    });
                });
        }

        egui::CentralPanel::default().show(root, |ui| {
            if let Some(open) = self.docs.get_mut(self.active) {
                let cmd = actions.iter().rev().find_map(|a| match a {
                    Action::View(c) => Some(*c),
                    _ => None,
                });
                let mut overlay = self.tools.overlay(self.cursor_doc, Some(&*open));
                overlay.view = self.view_opts;
                let out = canvas::show(ui, open, cmd, space_down, &overlay);
                self.cursor_doc = out.hover;
                if self.tools.vec.text_editing().is_some() {
                    text_editor_ui(&ctx, &mut self.tools.vec, open, &out);
                }
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
            if let Some(n) = self.tools.geo.show_dialogs(&ctx, open, self.tools.bg) {
                self.tools.notice = Some(n);
            }
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
            use orvalith_core::formats::{self, Format};
            let mut go = false;
            let mut cancel = false;
            let mut batch = None;
            let mut estimate = false;
            egui::Window::new("Exportar imagem")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    let before = form.format;
                    egui::ComboBox::from_label("Formato")
                        .selected_text(formats::info(form.format).name)
                        .show_ui(ui, |ui| {
                            for f in Format::exportable() {
                                ui.selectable_value(&mut form.format, f, formats::info(f).name);
                            }
                        });
                    if form.format != before {
                        form.estimate = None;
                    }
                    ui.small(formats::info(form.format).note);
                    if matches!(form.format, Format::Jpeg) {
                        ui.horizontal(|ui| {
                            ui.label("Qualidade");
                            ui.add(egui::Slider::new(&mut form.quality, 1..=100));
                        });
                    }
                    if matches!(form.format, Format::Jpeg | Format::Pnm) {
                        ui.horizontal(|ui| {
                            ui.label("Fundo");
                            ui.color_edit_button_srgb(&mut form.background);
                        });
                    }
                    if form.format != Format::Ora {
                        ui.horizontal(|ui| {
                            ui.label("Lado máximo (px, 0 = original)");
                            ui.add(egui::DragValue::new(&mut form.max_side).range(0..=16384).speed(8.0));
                        });
                    }
                    if let Some(open) = self.docs.get(self.active) {
                        let d = &open.doc;
                        ui.small(format!(
                            "Dimensões: {} × {} px · {:.0} ppi · perfil: {}",
                            d.width,
                            d.height,
                            d.ppi,
                            d.icc.as_ref().map_or("sRGB (sem perfil)", |p| p.name.as_str())
                        ));
                        if let Some(p) = d.icc.as_ref() {
                            use orvalith_core::imageio::ColorOut;
                            ui.separator();
                            ui.label(format!("Perfil do documento: {}", p.name));
                            ui.radio_value(&mut form.color, ColorOut::Embed, "Embutir o perfil (pixels intactos)");
                            ui.radio_value(&mut form.color, ColorOut::ToSrgb, "Converter para sRGB");
                            ui.radio_value(&mut form.color, ColorOut::Strip, "Gravar sem perfil (sem converter)");
                        }
                        let w = formats::export_warnings(d, form.format, &form.opts());
                        if !w.is_empty() {
                            ui.separator();
                            for line in w {
                                ui.colored_label(ui.visuals().warn_fg_color, format!("• {line}"));
                            }
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        estimate = ui.button("Estimar tamanho").clicked();
                        if let Some(t) = &form.estimate {
                            ui.label(t);
                        }
                    });
                    ui.small("Exportar não altera o projeto nem o arquivo original; arquivos existentes não são sobrescritos sem confirmação do sistema.");
                    ui.separator();
                    ui.label("Lote (usa o formato e as opções acima)");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Todos os documentos abertos…").clicked() {
                            batch = Some(BatchKind::AllDocs);
                        }
                        if ui.button("Cada camada…").clicked() {
                            batch = Some(BatchKind::Layers);
                        }
                        if ui.button("Folha de sprites…").clicked() {
                            batch = Some(BatchKind::Sprites(form.sprite_cols));
                        }
                        ui.add(egui::DragValue::new(&mut form.sprite_cols).range(1..=64).prefix("colunas "));
                    });
                    ui.horizontal(|ui| {
                        go = ui.button("Escolher destino…").clicked();
                        cancel = ui.button("Fechar").clicked();
                    });
                });
            if estimate {
                form.estimate = Some(match self.docs.get(self.active) {
                    Some(o) => match formats::export(&o.doc, form.format, &form.opts()) {
                        Ok(e) => format!("≈ {:.1} KiB", e.bytes.len() as f64 / 1024.0),
                        Err(e) => format!("indisponível: {e}"),
                    },
                    None => String::new(),
                });
            }
            if let Some(kind) = batch {
                self.export_batch(&form, kind);
                self.export = Some(form);
            } else if go {
                self.export_doc(&form);
            } else if !cancel {
                self.export = Some(form);
            }
        }

        self.adjust_windows(ctx);
        // Orçamentos de memória das preferências.
        let hb = (self.prefs.history_budget_mb as usize) << 20;
        for d in &mut self.docs {
            if d.history.budget() != Some(hb) {
                d.history.set_budget(hb);
            }
        }
        self.video.set_cache(self.prefs.frame_cache as usize);
        self.drain_recorded();
        self.batch_poll(ctx);
        self.actions_window(ctx);
        self.batch_window(ctx);
        self.command_palette(ctx);
        self.shortcuts_editor(ctx);
        self.video_poll(ctx);
        self.video_window(ctx);
        self.pdf_import_window(ctx);
        self.pdf_export_window(ctx);

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
                ui.checkbox(
                    &mut self.prefs.high_contrast,
                    "Alto contraste (acessibilidade)",
                );
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
                ui.label("Memória e cache");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.prefs.history_budget_mb)
                            .range(16..=8192)
                            .suffix(" MiB")
                            .prefix("histórico por documento "),
                    );
                });
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.prefs.snapshot_budget_mb)
                            .range(16..=8192)
                            .suffix(" MiB")
                            .prefix("instantâneos "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.prefs.frame_cache)
                            .range(4..=1024)
                            .prefix("cache de vídeo (quadros) "),
                    );
                });
                ui.small("O histórico descarta as ações mais antigas ao passar do orçamento.");
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
            theme::apply(ctx, self.prefs.dark_theme, self.prefs.high_contrast);
        }
        if self.prefs.high_contrast != before.high_contrast {
            theme::apply(ctx, self.prefs.dark_theme, self.prefs.high_contrast);
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

/// Editor de texto flutuante sobre o canvas (prévia ao vivo no documento).
fn text_editor_ui(
    ctx: &egui::Context,
    vec: &mut VecState,
    open: &mut OpenDoc,
    out: &canvas::CanvasOut,
) {
    let Some(view) = out.view else { return };
    let Some(current) = vec.current_text(open) else {
        return;
    };
    let Some(id) = vec.text_editing() else { return };
    let Some(orvalith_core::LayerSource::Text(spec)) =
        open.doc.layer(id).and_then(|l| l.source.clone())
    else {
        return;
    };
    let p = view.doc_to_view([spec.x, spec.y]);
    let pos = out.origin + egui::vec2(p[0], p[1]) - egui::vec2(0.0, 6.0);
    let mut text = current.clone();
    let (mut ok, mut cancel) = (false, false);
    egui::Area::new(egui::Id::unique("orvalith_text_editor"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let r = ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .desired_rows(3)
                        .desired_width(280.0)
                        .hint_text("Digite o texto…"),
                );
                if vec.text_focus_pending {
                    r.request_focus();
                    vec.text_focus_pending = false;
                }
                if r.has_focus() {
                    let (esc, ctrl_enter) = ui.input(|i| {
                        (
                            i.key_pressed(egui::Key::Escape),
                            i.modifiers.command && i.key_pressed(egui::Key::Enter),
                        )
                    });
                    cancel |= esc;
                    ok |= ctrl_enter;
                }
                ui.horizontal(|ui| {
                    ok |= ui.button("OK (Ctrl+Enter)").clicked();
                    cancel |= ui.button("Cancelar (Esc)").clicked();
                });
            });
        });
    if text != current {
        vec.edit_text_spec(open, |t| t.text = text);
    }
    if cancel {
        vec.cancel_text(open);
    } else if ok {
        vec.commit_text(open);
    }
}

impl OrvalithApp {
    /// Encerra (cancelando) qualquer ajuste ou filtro destrutivo em andamento.
    fn end_edit_sessions(&mut self) {
        if let Some((did, mut s)) = self.adj_session.take() {
            if let Some(o) = self.docs.iter_mut().find(|d| d.doc.id.0 == did) {
                adjustui::cancel_session(&mut s, o);
            }
        }
        if let Some((did, mut s)) = self.filter_session.take() {
            if let Some(o) = self.docs.iter_mut().find(|d| d.doc.id.0 == did) {
                filterui::cancel_session(&mut s, o);
            }
        }
    }

    /// Janelas de ajuste destrutivo, filtros, histograma e perfil de cor.
    fn adjust_windows(&mut self, ctx: &egui::Context) {
        if let Some(p) = crate::styleui::take_changed_presets() {
            self.prefs.style_presets = p;
            self.prefs.save();
        }
        let active_id = self.docs.get(self.active).map(|d| d.doc.id.0);
        // Uma sessão destrutiva pertence a um documento: trocar de aba a cancela.
        if let Some((did, _)) = &self.adj_session {
            if Some(*did) != active_id {
                if let Some((did, mut s)) = self.adj_session.take() {
                    if let Some(o) = self.docs.iter_mut().find(|d| d.doc.id.0 == did) {
                        adjustui::cancel_session(&mut s, o);
                    }
                }
            }
        }
        if let Some((did, _)) = &self.filter_session {
            if Some(*did) != active_id {
                if let Some((did, mut s)) = self.filter_session.take() {
                    if let Some(o) = self.docs.iter_mut().find(|d| d.doc.id.0 == did) {
                        filterui::cancel_session(&mut s, o);
                    }
                }
            }
        }
        if let (Some((_, s)), Some(open)) =
            (self.filter_session.as_mut(), self.docs.get_mut(self.active))
        {
            if filterui::session_window(ctx, s, open) {
                if s.applied {
                    self.actions
                        .record(orvalith_core::actions::Step::Filter(s.filter.clone()));
                }
                self.filter_session = None;
            }
        }
        if let (Some((_, s)), Some(open)) =
            (self.adj_session.as_mut(), self.docs.get_mut(self.active))
        {
            if adjustui::session_window(ctx, s, open) {
                if s.applied {
                    self.actions
                        .record(orvalith_core::actions::Step::Adjust(s.adj.clone()));
                }
                self.adj_session = None;
            }
        }
        let mut comps_notice = None;
        if let Some(open) = self.docs.get_mut(self.active) {
            comps_notice = compsui::comps_window(ctx, &mut self.comps_ui, open);
        }
        if let Some(n) = comps_notice {
            self.tools.notice = Some(n);
        }
        if let Some(open) = self.docs.get(self.active) {
            adjustui::histogram_window(ctx, &mut self.hist, open);
        }
        let mut msg = None;
        if let Some(open) = self.docs.get(self.active) {
            if let Some(action) = adjustui::color_window(ctx, &mut self.color_dlg, open) {
                match action {
                    ColorAction::OpenFile => {
                        let picked = rfd::FileDialog::new()
                            .add_filter("Perfil ICC", &["icc", "icm"])
                            .pick_file();
                        if let Some(path) = picked {
                            match std::fs::read(&path)
                                .map_err(|e| e.to_string())
                                .and_then(|b| orvalith_core::color::profile_from_bytes(&b))
                            {
                                Ok(p) => self.color_dlg.add_profile(p),
                                Err(e) => msg = Some(e),
                            }
                        }
                    }
                    other => {
                        if let Some(o) = self.docs.get_mut(self.active) {
                            if let Some(m) = adjustui::run_color_action(o, other) {
                                msg = Some(m);
                            }
                        }
                    }
                }
            }
        }
        if let Some(m) = msg {
            self.show_message("Perfil de cor", m);
        }
    }
}

impl OrvalithApp {
    /// Coloca uma imagem PNG/JPEG como objeto inteligente (embutido ou vinculado).
    fn place_object(&mut self, linked: bool) {
        if self.tools.is_busy() || self.docs.is_empty() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Imagens",
                &[
                    "png", "jpg", "jpeg", "jpe", "jfif", "webp", "gif", "bmp", "tif", "tiff",
                    "tga", "ico", "qoi",
                ],
            )
            .pick_file()
        else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                return self
                    .show_message("Erro ao ler a imagem", format!("{}\n\n{e}", path.display()))
            }
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Objeto".into());
        let link = linked.then(|| path.to_string_lossy().into_owned());
        let Some(open) = self.docs.get_mut(self.active) else {
            return;
        };
        let above = open.active_idx().map(|i| open.doc.layers[i].id);
        match orvalith_core::object::place_image(&open.doc, &bytes, &name, link, above) {
            Ok(o) => open.apply_outcome("Colocar objeto inteligente", o),
            Err(e) => self.show_message("Erro ao colocar a imagem", e),
        }
    }

    /// Escolhe o arquivo para (re)vincular o objeto da camada ativa.
    fn relink_object(&mut self) {
        if self.tools.is_busy() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Imagens",
                &[
                    "png", "jpg", "jpeg", "jpe", "jfif", "webp", "gif", "bmp", "tif", "tiff",
                    "tga", "ico", "qoi",
                ],
            )
            .pick_file()
        else {
            return;
        };
        let Some(open) = self.docs.get_mut(self.active) else {
            return;
        };
        let Some(id) = open.active_idx().map(|i| open.doc.layers[i].id) else {
            return;
        };
        match orvalith_core::object::relink(&open.doc, id, &path.to_string_lossy()) {
            Ok(layers) => open.edit_layers("Reencontrar vínculo", |ls| *ls = layers),
            Err(e) => self.show_message("Erro ao vincular", e),
        }
    }
}
