//! Registro de comandos: atalhos personalizáveis, paleta de comandos e editor de atalhos.
//!
//! Todo comando tem um id estável (ex.: `tool.Text`, `layer.ToggleClip`, `filter.3`), um rótulo
//! e um atalho padrão opcional. As preferências guardam só as diferenças (`keymap`: id → atalho,
//! texto vazio = sem atalho). A tabela de atalhos usada pelo teclado é montada a partir deste registro.

use std::collections::BTreeMap;
use std::sync::Arc;

use eframe::egui::{self, Key, Modifiers};
use orvalith_core::{adjust, filter};

use super::geocmds::GeoCmd;
use super::selops::SelCmd;
use super::{Action, OrvalithApp};
use crate::geometry::TMode;
use crate::layers_ui::LayerCmd;
use crate::tools::Tool;

/// Atalho: Ctrl (Comando no macOS), Shift, Alt e uma tecla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl Binding {
    pub fn new(m: Modifiers, key: Key) -> Self {
        Self {
            ctrl: m.command || m.ctrl,
            shift: m.shift,
            alt: m.alt,
            key,
        }
    }

    pub fn modifiers(&self) -> Modifiers {
        let mut m = if self.ctrl {
            Modifiers::COMMAND
        } else {
            Modifiers::NONE
        };
        if self.shift {
            m = m.plus(Modifiers::SHIFT);
        }
        if self.alt {
            m = m.plus(Modifiers::ALT);
        }
        m
    }

    /// Texto estável, como "Ctrl+Shift+K".
    pub fn text(&self) -> String {
        let mut p: Vec<&str> = Vec::new();
        if self.ctrl {
            p.push("Ctrl");
        }
        if self.shift {
            p.push("Shift");
        }
        if self.alt {
            p.push("Alt");
        }
        let k = self.key.name();
        let mut s = p.join("+");
        if !s.is_empty() {
            s.push('+');
        }
        s.push_str(k);
        s
    }

    pub fn parse(text: &str) -> Option<Binding> {
        let mut b = Binding {
            ctrl: false,
            shift: false,
            alt: false,
            key: Key::A,
        };
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (key_part, mods) = parts.split_last()?;
        for m in mods {
            match m.to_lowercase().as_str() {
                "ctrl" | "cmd" | "comando" => b.ctrl = true,
                "shift" => b.shift = true,
                "alt" => b.alt = true,
                _ => return None,
            }
        }
        b.key = Key::from_name(key_part)?;
        Some(b)
    }

    fn weight(&self) -> u8 {
        u8::from(self.ctrl) + u8::from(self.shift) + u8::from(self.alt)
    }
}

pub struct CmdDef {
    pub id: String,
    pub label: String,
    pub default: Option<Binding>,
    pub make: Arc<dyn Fn() -> Action + Send + Sync>,
}

fn humanize(dbg: &str) -> String {
    // "ToggleClip" → "Toggle Clip"; mantém parênteses/valores.
    let mut out = String::new();
    for (i, c) in dbg.chars().enumerate() {
        if c.is_uppercase() && i > 0 && !out.ends_with(' ') && !out.ends_with('(') {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

pub fn action_id(a: &Action) -> String {
    match a {
        Action::New => "file.new".into(),
        Action::Open => "file.open".into(),
        Action::OpenPath(_) => "file.openpath".into(),
        Action::Save => "file.save".into(),
        Action::SaveAs => "file.saveas".into(),
        Action::Export => "file.export".into(),
        Action::ImportPdf => "file.importpdf".into(),
        Action::ExportPdf => "file.exportpdf".into(),
        Action::Close => "file.close".into(),
        Action::Undo => "edit.undo".into(),
        Action::Redo => "edit.redo".into(),
        Action::View(v) => format!("view.{v:?}"),
        Action::NextTab => "view.nexttab".into(),
        Action::ClearRecent => "file.clearrecent".into(),
        Action::Tool(t) => format!("tool.{t:?}"),
        Action::SwapColors => "color.swap".into(),
        Action::DefaultColors => "color.default".into(),
        Action::BrushSize(f) => format!("brush.size.{f}"),
        Action::CancelTool => "tool.cancel".into(),
        Action::Layer(c) => format!("layer.{c:?}"),
        Action::Select(c) => format!("select.{c:?}"),
        Action::CopySel(cut) => format!("edit.{}", if *cut { "cut" } else { "copy" }),
        Action::DeleteKey => "edit.delete".into(),
        Action::Fill(bg) => format!("edit.fill.{}", if *bg { "bg" } else { "fg" }),
        Action::Confirm => "edit.confirm".into(),
        Action::Geo(c) => format!("geo.{c:?}"),
        Action::AdjStart(i) => format!("adjust.{i}"),
        Action::FilterStart(i) => format!("filter.{i}"),
        Action::AdjLayer(i) => format!("adjust.layer.{i}"),
        Action::ToggleHist => "window.histogram".into(),
        Action::ToggleTimeline => "window.timeline".into(),
        Action::ToggleVideo => "window.video".into(),
        Action::FrameStep(d) => format!("timeline.step.{d}"),
        Action::ToggleComps => "window.comps".into(),
        Action::PlaceObject(l) => {
            format!("object.place.{}", if *l { "linked" } else { "embedded" })
        }
        Action::ColorDlg => "window.colorprofile".into(),
        Action::Palette => "app.palette".into(),
        Action::ShortcutsEditor => "app.shortcuts".into(),
        Action::Help => "app.help".into(),
        Action::Macros => "window.macros".into(),
        Action::Batch => "window.batch".into(),
        Action::Presets => "window.presets".into(),
        Action::History => "window.history".into(),
        Action::OpenExample(i) => format!("help.example.{i}"),
        Action::Workspace(i) => format!("workspace.{i}"),
        Action::TogglePanel(k) => format!("panel.{k}"),
    }
}

pub fn action_label(a: &Action) -> String {
    let adj = |i: usize| adjust::KINDS.get(i).copied().unwrap_or("?");
    match a {
        Action::New => "Arquivo: Novo documento".into(),
        Action::Open => "Arquivo: Abrir".into(),
        Action::OpenPath(_) => "Arquivo: Abrir caminho".into(),
        Action::Save => "Arquivo: Salvar projeto".into(),
        Action::SaveAs => "Arquivo: Salvar projeto como".into(),
        Action::Export => "Arquivo: Exportar imagem".into(),
        Action::ImportPdf => "Arquivo: Importar PDF".into(),
        Action::ExportPdf => "Arquivo: Exportar PDF".into(),
        Action::Close => "Arquivo: Fechar documento".into(),
        Action::Undo => "Editar: Desfazer".into(),
        Action::Redo => "Editar: Refazer".into(),
        Action::View(v) => match v {
            crate::canvas::ViewCmd::Fit => "Exibir: Ajustar à janela".into(),
            crate::canvas::ViewCmd::Actual => "Exibir: 100%".into(),
            crate::canvas::ViewCmd::ZoomIn => "Exibir: Ampliar".into(),
            crate::canvas::ViewCmd::ZoomOut => "Exibir: Reduzir".into(),
            crate::canvas::ViewCmd::Rotate(d) => format!("Exibir: Girar visão {d:+.0}°"),
            crate::canvas::ViewCmd::ResetRotation => "Exibir: Redefinir rotação da visão".into(),
        },
        Action::NextTab => "Exibir: Próxima aba".into(),
        Action::ClearRecent => "Arquivo: Limpar recentes".into(),
        Action::Tool(t) => format!("Ferramenta: {}", t.label()),
        Action::SwapColors => "Cores: Trocar frente e fundo".into(),
        Action::DefaultColors => "Cores: Padrão (preto e branco)".into(),
        Action::BrushSize(f) => format!(
            "Pincel: {}",
            if *f < 1.0 {
                "Diminuir tamanho"
            } else {
                "Aumentar tamanho"
            }
        ),
        Action::CancelTool => "Ferramenta: Cancelar operação".into(),
        Action::Layer(c) => match c {
            LayerCmd::New => "Camada: Nova".into(),
            LayerCmd::Duplicate => "Camada: Duplicar".into(),
            LayerCmd::Group => "Camada: Agrupar".into(),
            LayerCmd::Ungroup => "Camada: Desagrupar".into(),
            LayerCmd::MergeSelected => "Camada: Mesclar selecionadas".into(),
            LayerCmd::Up => "Camada: Subir".into(),
            LayerCmd::Down => "Camada: Descer".into(),
            LayerCmd::ToggleClip => "Camada: Alternar máscara de recorte".into(),
            LayerCmd::Rasterize => "Camada: Rasterizar".into(),
            LayerCmd::ToObject => "Camada: Converter em objeto inteligente".into(),
            other => format!("Camada: {}", humanize(&format!("{other:?}"))),
        },
        Action::Select(c) => match c {
            SelCmd::All => "Seleção: Selecionar tudo".into(),
            SelCmd::Deselect => "Seleção: Desmarcar".into(),
            SelCmd::Invert => "Seleção: Inverter".into(),
            SelCmd::QuickMask => "Seleção: Máscara rápida".into(),
            other => format!("Seleção: {}", humanize(&format!("{other:?}"))),
        },
        Action::CopySel(cut) => if *cut {
            "Editar: Recortar"
        } else {
            "Editar: Copiar"
        }
        .into(),
        Action::DeleteKey => "Editar: Apagar".into(),
        Action::Fill(bg) => if *bg {
            "Editar: Preencher com a cor de fundo"
        } else {
            "Editar: Preencher com a cor de frente"
        }
        .into(),
        Action::Confirm => "Editar: Confirmar operação".into(),
        Action::Geo(c) => match c {
            GeoCmd::ImageSize => "Imagem: Tamanho da imagem".into(),
            GeoCmd::CanvasSize => "Imagem: Tamanho da tela".into(),
            GeoCmd::Transform(TMode::Free) => "Editar: Transformação livre".into(),
            GeoCmd::ToggleRulers => "Exibir: Réguas".into(),
            GeoCmd::ToggleGrid => "Exibir: Grade".into(),
            GeoCmd::ToggleGuides => "Exibir: Guias".into(),
            GeoCmd::ToggleSnap => "Exibir: Encaixe".into(),
            other => format!("Imagem: {}", humanize(&format!("{other:?}"))),
        },
        Action::AdjStart(i) => format!("Ajuste: {}…", adj(*i)),
        Action::FilterStart(i) => {
            format!("Filtro: {}…", filter::KINDS.get(*i).copied().unwrap_or("?"))
        }
        Action::AdjLayer(i) => format!("Nova camada de ajuste: {}", adj(*i)),
        Action::ToggleHist => "Janela: Histograma".into(),
        Action::ToggleTimeline => "Janela: Linha do tempo".into(),
        Action::ToggleVideo => "Janela: Vídeo".into(),
        Action::FrameStep(d) => if *d < 0 {
            "Animação: Quadro anterior"
        } else {
            "Animação: Próximo quadro"
        }
        .into(),
        Action::ToggleComps => "Janela: Composições de camadas".into(),
        Action::PlaceObject(l) => if *l {
            "Objetos: Colocar imagem vinculada"
        } else {
            "Objetos: Colocar imagem embutida"
        }
        .into(),
        Action::ColorDlg => "Janela: Perfil de cor".into(),
        Action::Palette => "Aplicativo: Paleta de comandos".into(),
        Action::ShortcutsEditor => "Aplicativo: Atalhos de teclado".into(),
        Action::Help => "Ajuda: Ajuda e exemplos".into(),
        Action::Macros => "Janela: Ações (macros)".into(),
        Action::Batch => "Janela: Processamento em lote".into(),
        Action::Presets => "Janela: Predefinições".into(),
        Action::History => "Janela: Histórico e instantâneos".into(),
        Action::OpenExample(i) => format!("Ajuda: Abrir exemplo {}", i + 1),
        Action::Workspace(i) => crate::presets::builtin_workspaces().get(*i).map_or_else(
            || format!("Espaço de trabalho: {}", i + 1),
            |w| format!("Espaço de trabalho: {}", w.name),
        ),
        Action::TogglePanel(k) => format!(
            "Painel: alternar {}",
            ["ferramentas", "camadas", "opções", "status"]
                .get(*k as usize)
                .copied()
                .unwrap_or("?")
        ),
    }
}

/// Atalhos padrão (a tabela clássica do aplicativo).
type DefaultEntry = (Modifiers, Key, fn() -> Action);

fn default_table() -> Vec<DefaultEntry> {
    const C: Modifiers = Modifiers::COMMAND;
    let cs = Modifiers::COMMAND.plus(Modifiers::SHIFT);
    let ca = Modifiers::COMMAND.plus(Modifiers::ALT);
    vec![
        (Modifiers::NONE, Key::Comma, || Action::FrameStep(-1)),
        (Modifiers::NONE, Key::Period, || Action::FrameStep(1)),
        (cs, Key::P, || Action::Palette),
        (Modifiers::NONE, Key::F1, || Action::Help),
        (Modifiers::SHIFT, Key::R, || Action::Tool(Tool::Liquify)),
        (Modifiers::NONE, Key::S, || Action::Tool(Tool::Clone)),
        (Modifiers::NONE, Key::J, || Action::Tool(Tool::SpotHeal)),
        (Modifiers::NONE, Key::H, || Action::Tool(Tool::Patch)),
        (Modifiers::NONE, Key::Y, || Action::Tool(Tool::RedEye)),
        (Modifiers::NONE, Key::R, || Action::Tool(Tool::Retouch)),
        (Modifiers::NONE, Key::T, || Action::Tool(Tool::Text)),
        (Modifiers::NONE, Key::U, || Action::Tool(Tool::Shape)),
        (Modifiers::NONE, Key::P, || Action::Tool(Tool::Pen)),
        (Modifiers::NONE, Key::A, || Action::Tool(Tool::Node)),
        (ca, Key::G, || Action::Layer(LayerCmd::ToggleClip)),
        (cs, Key::Semicolon, || Action::Geo(GeoCmd::ToggleSnap)),
        (ca, Key::I, || Action::Geo(GeoCmd::ImageSize)),
        (ca, Key::C, || Action::Geo(GeoCmd::CanvasSize)),
        (C, Key::T, || Action::Geo(GeoCmd::Transform(TMode::Free))),
        (C, Key::R, || Action::Geo(GeoCmd::ToggleRulers)),
        (C, Key::Quote, || Action::Geo(GeoCmd::ToggleGrid)),
        (C, Key::Semicolon, || Action::Geo(GeoCmd::ToggleGuides)),
        (Modifiers::NONE, Key::C, || Action::Tool(Tool::Crop)),
        (Modifiers::ALT, Key::ArrowLeft, || {
            Action::View(crate::canvas::ViewCmd::Rotate(-15.0))
        }),
        (Modifiers::ALT, Key::ArrowRight, || {
            Action::View(crate::canvas::ViewCmd::Rotate(15.0))
        }),
        (Modifiers::ALT, Key::ArrowUp, || {
            Action::View(crate::canvas::ViewCmd::ResetRotation)
        }),
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
        (C, Key::Num0, || Action::View(crate::canvas::ViewCmd::Fit)),
        (C, Key::Num1, || {
            Action::View(crate::canvas::ViewCmd::Actual)
        }),
        (C, Key::Equals, || {
            Action::View(crate::canvas::ViewCmd::ZoomIn)
        }),
        (C, Key::Plus, || {
            Action::View(crate::canvas::ViewCmd::ZoomIn)
        }),
        (C, Key::Minus, || {
            Action::View(crate::canvas::ViewCmd::ZoomOut)
        }),
        (C, Key::F4, || Action::Close),
        (Modifiers::NONE, Key::F5, || {
            Action::View(crate::canvas::ViewCmd::Fit)
        }),
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
    ]
}

/// Todos os comandos: os com atalho padrão e os que só existem nos menus/paleta.
pub fn registry() -> Vec<CmdDef> {
    let mut out: Vec<CmdDef> = Vec::new();
    let mut push = |make: Arc<dyn Fn() -> Action + Send + Sync>, default: Option<Binding>| {
        let a = make();
        let id = action_id(&a);
        if out.iter().any(|c| c.id == id) {
            // Mesma ação com outro atalho padrão (ex.: Ctrl+= e Ctrl++): o primeiro define o comando.
            return;
        }
        out.push(CmdDef {
            label: action_label(&a),
            id,
            default,
            make,
        });
    };
    for (m, k, f) in default_table() {
        push(Arc::new(f), Some(Binding::new(m, k)));
    }
    for t in Tool::ALL {
        push(Arc::new(move || Action::Tool(t)), None);
    }
    for i in 0..filter::KINDS.len() {
        push(Arc::new(move || Action::FilterStart(i)), None);
    }
    for i in 0..adjust::KINDS.len() {
        push(Arc::new(move || Action::AdjStart(i)), None);
        push(Arc::new(move || Action::AdjLayer(i)), None);
    }
    let simple: [fn() -> Action; 18] = [
        || Action::ToggleHist,
        || Action::ToggleTimeline,
        || Action::ToggleVideo,
        || Action::ToggleComps,
        || Action::ColorDlg,
        || Action::ImportPdf,
        || Action::ExportPdf,
        || Action::ShortcutsEditor,
        || Action::Macros,
        || Action::Batch,
        || Action::Help,
        || Action::Presets,
        || Action::History,
        || Action::PlaceObject(false),
        || Action::PlaceObject(true),
        || Action::Layer(LayerCmd::Rasterize),
        || Action::Layer(LayerCmd::ToObject),
        || Action::Layer(LayerCmd::Flatten),
    ];
    for f in simple {
        push(Arc::new(f), None);
    }
    for i in 0..crate::presets::builtin_workspaces().len() {
        push(Arc::new(move || Action::Workspace(i)), None);
    }
    for i in 0..orvalith_core::examples::EXAMPLES.len() {
        push(Arc::new(move || Action::OpenExample(i)), None);
    }
    for k in 0..4u8 {
        push(Arc::new(move || Action::TogglePanel(k)), None);
    }
    out
}

/// Atalho efetivo de um comando: o personalizado ou, na falta dele, o padrão.
pub fn effective(c: &CmdDef, overrides: &BTreeMap<String, String>) -> Option<Binding> {
    match overrides.get(&c.id) {
        Some(t) if t.is_empty() => None,
        Some(t) => Binding::parse(t).or(c.default),
        None => c.default,
    }
}

/// Define (ou limpa) o atalho de `id`; quem já usava o mesmo atalho fica sem atalho. Devolve o rótulo desse comando.
pub fn rebind(
    overrides: &mut BTreeMap<String, String>,
    cmds: &[CmdDef],
    id: &str,
    b: Option<Binding>,
) -> Option<String> {
    let mut displaced = None;
    if let Some(nb) = b {
        for c in cmds.iter().filter(|c| c.id != id) {
            if effective(c, overrides) == Some(nb) {
                overrides.insert(c.id.clone(), String::new());
                displaced = Some(c.label.clone());
            }
        }
    }
    overrides.insert(id.to_string(), b.map(|x| x.text()).unwrap_or_default());
    // Remove entradas redundantes (iguais ao padrão).
    if let Some(c) = cmds.iter().find(|c| c.id == id) {
        if overrides.get(id).map(String::as_str)
            == Some(&c.default.map(|d| d.text()).unwrap_or_default())
        {
            overrides.remove(id);
        }
    }
    displaced
}

/// Tabela de teclas ativas: ordenada de modo que Ctrl+Shift+X venha antes de Ctrl+X.
pub struct Keymap {
    pub cmds: Vec<CmdDef>,
    entries: Vec<(Binding, usize)>,
}

impl Keymap {
    pub fn build(overrides: &BTreeMap<String, String>) -> Self {
        let cmds = registry();
        let mut entries: Vec<(Binding, usize)> = Vec::new();
        for (i, c) in cmds.iter().enumerate() {
            if let Some(b) = effective(c, overrides) {
                entries.push((b, i));
            }
        }
        // Entradas padrão duplicadas (Ctrl+= / Ctrl++) viram o mesmo comando; mantém as extras.
        for (m, k, f) in default_table() {
            let a = f();
            let id = action_id(&a);
            if let Some(i) = cmds.iter().position(|c| c.id == id) {
                let b = Binding::new(m, k);
                if Some(b) != cmds[i].default && overrides.get(&id).is_none() {
                    entries.push((b, i));
                }
            }
        }
        entries.sort_by_key(|(b, _)| std::cmp::Reverse(b.weight()));
        Self { cmds, entries }
    }

    /// Ações disparadas pelas teclas deste quadro.
    pub fn collect(&self, i: &mut egui::InputState) -> Vec<Action> {
        let mut out = Vec::new();
        for (b, idx) in &self.entries {
            if i.consume_key(b.modifiers(), b.key) {
                out.push((self.cmds[*idx].make)());
            }
        }
        out
    }
}

/// Pontuação de busca (maior = melhor); `None` se não casa.
pub fn score(query: &str, c: &CmdDef) -> Option<u32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(1);
    }
    let hay = format!("{} {}", c.label.to_lowercase(), c.id.to_lowercase());
    let mut total = 0;
    for w in q.split_whitespace() {
        let pos = hay.find(w)?;
        total += 100u32.saturating_sub(pos as u32);
        if c.label.to_lowercase().starts_with(w) {
            total += 50;
        }
    }
    Some(total)
}

pub fn search<'a>(cmds: &'a [CmdDef], query: &str) -> Vec<&'a CmdDef> {
    let mut v: Vec<(u32, &CmdDef)> = cmds
        .iter()
        .filter_map(|c| score(query, c).map(|s| (s, c)))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.label.cmp(&b.1.label)));
    v.into_iter().map(|(_, c)| c).collect()
}

#[derive(Default)]
pub struct CmdUi {
    pub palette: bool,
    pub editor: bool,
    query: String,
    sel: usize,
    filter: String,
    capturing: Option<String>,
    notice: Option<String>,
    focus: bool,
}

impl CmdUi {
    pub fn capturing(&self) -> bool {
        self.capturing.is_some()
    }
}

impl OrvalithApp {
    pub(super) fn rebuild_keymap(&mut self) {
        self.keymap = Keymap::build(&self.prefs.keymap);
    }

    pub(super) fn command_palette(&mut self, ctx: &egui::Context) {
        if !self.cmd.palette {
            return;
        }
        let mut run: Option<usize> = None;
        let mut close = false;
        let matches: Vec<usize> = {
            let found = search(&self.keymap.cmds, &self.cmd.query);
            found
                .iter()
                .take(12)
                .filter_map(|c| self.keymap.cmds.iter().position(|x| x.id == c.id))
                .collect()
        };
        self.cmd.sel = self.cmd.sel.min(matches.len().saturating_sub(1));
        egui::Window::new("Paleta de comandos")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0))
            .show(ctx, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.cmd.query)
                        .hint_text("Digite um comando…")
                        .desired_width(420.0),
                );
                if !self.cmd.focus {
                    r.request_focus();
                    self.cmd.focus = true;
                }
                let (down, up, enter, esc) = ui.input(|i| {
                    (
                        i.key_pressed(egui::Key::ArrowDown),
                        i.key_pressed(egui::Key::ArrowUp),
                        i.key_pressed(egui::Key::Enter),
                        i.key_pressed(egui::Key::Escape),
                    )
                });
                if down && self.cmd.sel + 1 < matches.len() {
                    self.cmd.sel += 1;
                }
                if up && self.cmd.sel > 0 {
                    self.cmd.sel -= 1;
                }
                if enter {
                    run = matches.get(self.cmd.sel).copied();
                }
                close |= esc;
                for (n, &i) in matches.iter().enumerate() {
                    let c = &self.keymap.cmds[i];
                    let key = effective(c, &self.prefs.keymap)
                        .map(|b| b.text())
                        .unwrap_or_default();
                    let text = if key.is_empty() {
                        c.label.clone()
                    } else {
                        format!("{}    [{key}]", c.label)
                    };
                    if ui.selectable_label(n == self.cmd.sel, text).clicked() {
                        run = Some(i);
                    }
                }
                if matches.is_empty() {
                    ui.label("Nenhum comando encontrado.");
                }
            });
        if let Some(i) = run {
            let a = (self.keymap.cmds[i].make)();
            self.queued.push(a);
            close = true;
        }
        if close {
            self.cmd.palette = false;
            self.cmd.query.clear();
            self.cmd.focus = false;
            self.cmd.sel = 0;
        }
    }

    pub(super) fn shortcuts_editor(&mut self, ctx: &egui::Context) {
        if !self.cmd.editor {
            return;
        }
        // Captura da próxima combinação de teclas.
        if let Some(id) = self.cmd.capturing.clone() {
            let ev = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        repeat: false,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some((key, m)) = ev {
                if key == Key::Escape {
                    self.cmd.capturing = None;
                } else {
                    let b = Binding::new(m, key);
                    let displaced = rebind(&mut self.prefs.keymap, &self.keymap.cmds, &id, Some(b));
                    self.cmd.notice = displaced
                        .map(|d| format!("O atalho {} foi retirado de \"{d}\".", b.text()));
                    self.cmd.capturing = None;
                    self.prefs.save();
                    self.rebuild_keymap();
                }
            }
        }
        let mut is_open = true;
        let mut reset_all = false;
        let mut set: Option<String> = None;
        let mut clear: Option<String> = None;
        let mut default: Option<String> = None;
        egui::Window::new("Atalhos de teclado")
            .open(&mut is_open)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.cmd.filter).hint_text("Buscar comando…").desired_width(300.0));
                if let Some(n) = &self.cmd.notice {
                    ui.colored_label(ui.visuals().warn_fg_color, n);
                }
                if let Some(id) = &self.cmd.capturing {
                    let label = self.keymap.cmds.iter().find(|c| &c.id == id).map_or("", |c| c.label.as_str());
                    ui.colored_label(ui.visuals().warn_fg_color, format!("Pressione a nova combinação para \"{label}\" (Esc cancela)…"));
                }
                egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                    for c in search(&self.keymap.cmds, &self.cmd.filter) {
                        ui.horizontal(|ui| {
                            let cur = effective(c, &self.prefs.keymap);
                            let changed = self.prefs.keymap.contains_key(&c.id);
                            ui.label(&c.label);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Padrão").on_hover_text("Restaurar o atalho padrão").clicked() {
                                    default = Some(c.id.clone());
                                }
                                if ui.small_button("Limpar").clicked() {
                                    clear = Some(c.id.clone());
                                }
                                if ui.small_button("Definir…").clicked() {
                                    set = Some(c.id.clone());
                                }
                                let text = cur.map_or("—".to_string(), |b| b.text());
                                ui.label(if changed { egui::RichText::new(text).strong() } else { egui::RichText::new(text) });
                            });
                        });
                    }
                });
                ui.separator();
                if ui.button("Restaurar todos os atalhos padrão").clicked() {
                    reset_all = true;
                }
                ui.small("Atalhos são salvos nas preferências locais. Teclas sem Ctrl valem só quando nenhum campo de texto tem o foco.");
            });
        if let Some(id) = set {
            self.cmd.capturing = Some(id);
            self.cmd.notice = None;
        }
        let mut dirty = false;
        if let Some(id) = clear {
            rebind(&mut self.prefs.keymap, &self.keymap.cmds, &id, None);
            dirty = true;
        }
        if let Some(id) = default {
            self.prefs.keymap.remove(&id);
            dirty = true;
        }
        if reset_all {
            self.prefs.keymap.clear();
            dirty = true;
        }
        if dirty {
            self.prefs.save();
            self.rebuild_keymap();
        }
        self.cmd.editor = is_open;
        if !is_open {
            self.cmd.capturing = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_roundtrip_through_text() {
        for (m, k) in [
            (Modifiers::COMMAND, Key::S),
            (Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z),
            (Modifiers::ALT, Key::ArrowLeft),
            (Modifiers::NONE, Key::Num0),
            (Modifiers::NONE, Key::F5),
            (Modifiers::COMMAND.plus(Modifiers::ALT), Key::Comma),
        ] {
            let b = Binding::new(m, k);
            assert_eq!(Binding::parse(&b.text()), Some(b), "{}", b.text());
        }
        assert_eq!(
            Binding::parse("Ctrl+Shift+K").map(|b| (b.ctrl, b.shift, b.alt)),
            Some((true, true, false))
        );
        assert_eq!(Binding::parse("Ctrl+Nada"), None);
        assert_eq!(Binding::parse("Foo+K"), None);
        assert_eq!(Binding::parse(""), None);
    }

    #[test]
    fn registry_has_unique_ids_labels_and_conflict_free_defaults() {
        let r = registry();
        let mut ids: Vec<&str> = r.iter().map(|c| c.id.as_str()).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "ids únicos");
        assert!(r.len() > 100, "{}", r.len());
        assert!(
            r.iter()
                .all(|c| !c.label.is_empty() && !c.label.contains("{")),
            "rótulos legíveis"
        );
        let mut keys: Vec<String> = r
            .iter()
            .filter_map(|c| c.default)
            .map(|b| b.text())
            .collect();
        let m = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), m, "atalhos padrão sem conflito");
        // Ids estáveis para comandos conhecidos.
        for want in [
            "tool.Text",
            "file.save",
            "edit.undo",
            "filter.0",
            "adjust.layer.3",
        ] {
            assert!(r.iter().any(|c| c.id == want), "{want}");
        }
        assert!(r
            .iter()
            .find(|c| c.id == "file.save")
            .unwrap()
            .label
            .contains("Salvar"));
    }

    #[test]
    fn rebinding_displaces_the_previous_owner_and_resets_cleanly() {
        let cmds = registry();
        let mut ov = BTreeMap::new();
        let save = Binding::parse("Ctrl+S").unwrap();
        assert_eq!(
            effective(cmds.iter().find(|c| c.id == "file.save").unwrap(), &ov),
            Some(save)
        );
        // Atribui Ctrl+S ao Exportar: o Salvar perde o atalho.
        let displaced = rebind(&mut ov, &cmds, "file.export", Some(save));
        assert!(displaced.unwrap().contains("Salvar"));
        assert_eq!(
            effective(cmds.iter().find(|c| c.id == "file.export").unwrap(), &ov),
            Some(save)
        );
        assert_eq!(
            effective(cmds.iter().find(|c| c.id == "file.save").unwrap(), &ov),
            None
        );
        // O teclado reage ao mapa novo.
        let km = Keymap::build(&ov);
        let ctrl_s: Vec<&str> = km
            .entries
            .iter()
            .filter(|(b, _)| *b == save)
            .map(|(_, i)| km.cmds[*i].id.as_str())
            .collect();
        assert_eq!(ctrl_s, vec!["file.export"]);
        // Reatribuir o padrão remove a entrada redundante.
        let export_default = cmds.iter().find(|c| c.id == "file.export").unwrap().default;
        rebind(&mut ov, &cmds, "file.export", export_default);
        assert!(!ov.contains_key("file.export"));
        // Limpar deixa sem atalho; texto inválido volta ao padrão.
        rebind(&mut ov, &cmds, "tool.Text", None);
        assert_eq!(
            effective(cmds.iter().find(|c| c.id == "tool.Text").unwrap(), &ov),
            None
        );
        ov.insert("tool.Text".into(), "lixo+++".into());
        assert!(effective(cmds.iter().find(|c| c.id == "tool.Text").unwrap(), &ov).is_some());
    }

    #[test]
    fn modifier_heavy_bindings_are_checked_before_plain_ones() {
        let km = Keymap::build(&BTreeMap::new());
        let w: Vec<u8> = km.entries.iter().map(|(b, _)| b.weight()).collect();
        assert!(w.windows(2).all(|p| p[0] >= p[1]));
        // Ctrl+Shift+Z (refazer) é testado antes de Ctrl+Z (desfazer).
        let pos = |t: &str| km.entries.iter().position(|(b, _)| b.text() == t).unwrap();
        assert!(pos("Ctrl+Shift+Z") < pos("Ctrl+Z"));
    }

    #[test]
    fn search_ranks_and_filters_commands() {
        let cmds = registry();
        let r = search(&cmds, "desfoque gaussiano");
        assert_eq!(r.first().map(|c| c.id.as_str()), Some("filter.0"));
        assert!(search(&cmds, "zzzzxyz").is_empty());
        let all = search(&cmds, "");
        assert_eq!(all.len(), cmds.len());
        let r = search(&cmds, "salvar");
        assert!(r.iter().take(3).any(|c| c.id == "file.save"));
        assert!(search(&cmds, "FILTRO mediana")
            .iter()
            .any(|c| c.id == "filter.6"));
    }
}
