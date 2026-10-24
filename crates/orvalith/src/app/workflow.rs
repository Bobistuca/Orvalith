//! Predefinições, espaços de trabalho, histórico visual, instantâneos, ajuda local e exemplos.

use std::collections::{HashMap, HashSet};

use eframe::egui;
use orvalith_core::docops::DocState;
use orvalith_core::{examples, resources};

use super::OrvalithApp;
use crate::canvas::OpenDoc;
use crate::presets::{self, Bundle, Layout};

/// Instantâneo: cópia nomeada do estado do documento (camadas, tamanho, seleção, canais, guias).
struct Snapshot {
    name: String,
    state: DocState,
}

#[derive(Default)]
pub(super) struct WorkflowUi {
    pub presets_open: bool,
    pub history_open: bool,
    pub help_open: bool,
    tab: usize,
    help_query: String,
    snap_name: String,
    doc_name: String,
    ws_name: String,
    snaps: HashMap<u64, Vec<Snapshot>>,
}

/// Bytes de pixels distintos retidos pelos estados (camadas compartilhadas contam uma vez).
pub(super) fn snapshot_bytes<'a>(states: impl Iterator<Item = &'a DocState>) -> usize {
    let mut seen: HashSet<*const Vec<u8>> = HashSet::new();
    let mut total = 0usize;
    for s in states {
        for l in &s.layers {
            if seen.insert(std::sync::Arc::as_ptr(&l.pixels)) {
                total += l.pixels.len();
            }
        }
    }
    total
}

pub(super) const HELP: [(&str, &str); 10] = [
    (
        "Primeiros passos",
        "Arquivo → Novo cria um documento (use as predefinições de tamanho). Arquivo → Abrir lê imagens, .orva, PDF, PSD e outros. Tudo fica no seu computador: não há login, rede nem telemetria.",
    ),
    (
        "Atalhos e paleta de comandos",
        "Ctrl+Shift+P abre a paleta: digite parte do nome de qualquer comando. Editar → Atalhos de teclado… permite trocar o atalho de cada comando; o conflito é avisado e o atalho antigo fica livre. Restaurar volta ao padrão. F1 abre esta ajuda.",
    ),
    (
        "Camadas, máscaras e estilos",
        "O painel da direita lista as camadas. Há máscaras, grupos, modos de mistura, estilos de camada, objetos inteligentes e camadas de ajuste não destrutivas. Desfazer e refazer valem para tudo (Ctrl+Z e Ctrl+Shift+Z).",
    ),
    (
        "Ajustes e filtros",
        "Menus Ajustes e Filtro aplicam com prévia ao vivo. 'Nova camada de ajuste' mantém a edição reversível. Gradientes de ajustes podem ser salvos como predefinição no próprio editor de gradiente.",
    ),
    (
        "Ações (macros) e lote",
        "Editar → Ações: grave ajustes, filtros, tamanho da imagem e giro/espelho e reproduza em outro documento. Editar → Processamento em lote aplica uma ação e exporta vários arquivos; cada arquivo tem seu resultado no relatório e os originais não são sobrescritos sem a opção explícita.",
    ),
    (
        "Predefinições",
        "Editar → Predefinições… reúne tamanhos de documento, exportação, gradientes e espaços de trabalho. É possível exportar/importar um pacote .orvapreset e importar paletas GIMP (.gpl) e gradientes GIMP (.ggr). Pincéis .abr e amostras .ase não são suportados.",
    ),
    (
        "Espaços de trabalho",
        "Visualizar → Espaço de trabalho muda quais painéis aparecem (Pintura, Fotografia, Animação, Mínimo) e guarda a disposição atual com um nome. A disposição também é lembrada ao reabrir o aplicativo.",
    ),
    (
        "Histórico visual e instantâneos",
        "Editar → Histórico e instantâneos lista as ações; clique numa linha para voltar (ou avançar) até ela. Instantâneos guardam o estado do documento com um nome e respeitam o orçamento de memória das Preferências. Não são gravados em disco.",
    ),
    (
        "Animação e vídeo",
        "Animação → Linha do tempo cria quadros-chave e quadro a quadro, com pele de cebola e exportação GIF/sequência PNG. A edição de vídeo usa o FFmpeg instalado no seu computador, só para as operações de vídeo.",
    ),
    (
        "Acessibilidade e privacidade",
        "Preferências → Alto contraste e escala da interface. Todos os comandos podem ser acionados pelo teclado pela paleta e podem ter atalho próprio. As preferências ficam em %APPDATA%\\Orvalith\\config.json e não contêm imagens nem dados pessoais.",
    ),
];

/// Índices das seções cujo título ou texto contém todas as palavras da busca.
pub(super) fn help_matches(query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    (0..HELP.len())
        .filter(|&i| {
            let hay = format!("{} {}", HELP[i].0, HELP[i].1).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .collect()
}

impl OrvalithApp {
    /// Espaços de trabalho disponíveis: os embutidos e depois os do usuário.
    pub(super) fn all_workspaces(&self) -> Vec<presets::Workspace> {
        let mut v = presets::builtin_workspaces();
        v.extend(self.prefs.workspaces.iter().cloned());
        v
    }

    pub(super) fn current_layout(&self) -> Layout {
        Layout {
            timeline: self.timeline.open,
            histogram: self.hist.open,
            ..self.prefs.layout
        }
    }

    pub(super) fn apply_layout(&mut self, l: Layout) {
        self.prefs.layout = l;
        self.timeline.open = l.timeline;
        self.hist.open = l.histogram;
        self.prefs.save();
    }

    pub(super) fn apply_workspace(&mut self, index: usize) {
        if let Some(w) = self.all_workspaces().get(index) {
            self.apply_layout(w.layout);
        }
    }

    /// Abre um dos exemplos originais como documento novo (não modificado, sem caminho).
    pub(super) fn open_example(&mut self, index: usize) {
        match examples::build(index) {
            Ok(doc) => {
                self.docs.push(OpenDoc::new(doc));
                self.active = self.docs.len() - 1;
            }
            Err(e) => self.show_message("Exemplo", e),
        }
    }

    /// Grava a disposição dos painéis quando ela muda (linha do tempo e histograma abrem e fecham por comandos).
    pub(super) fn layout_sync(&mut self) {
        let cur = self.current_layout();
        if cur != self.prefs.layout {
            self.prefs.layout = cur;
            self.prefs.save();
        }
    }

    fn budget_bytes(&self) -> usize {
        self.prefs.snapshot_budget_mb as usize * 1024 * 1024
    }

    fn snapshot_take(&mut self) {
        let Some(o) = self.docs.get(self.active) else {
            return;
        };
        let id = o.doc.id.0;
        let state = DocState::of(&o.doc);
        let used = snapshot_bytes(self.wf.snaps.values().flatten().map(|s| &s.state));
        let after = snapshot_bytes(
            self.wf
                .snaps
                .values()
                .flatten()
                .map(|s| &s.state)
                .chain(std::iter::once(&state)),
        );
        if after > self.budget_bytes() {
            let msg = format!(
                "Orçamento de instantâneos esgotado ({} de {} MiB). Apague instantâneos ou aumente o limite em Preferências.",
                used / (1024 * 1024),
                self.prefs.snapshot_budget_mb
            );
            self.show_message("Instantâneo", msg);
            return;
        }
        let list = self.wf.snaps.entry(id).or_default();
        let name = match self.wf.snap_name.trim() {
            "" => format!("Instantâneo {}", list.len() + 1),
            n => n.chars().take(60).collect(),
        };
        list.push(Snapshot { name, state });
        self.wf.snap_name.clear();
    }

    fn snapshot_restore(&mut self, index: usize) {
        let Some(o) = self.docs.get_mut(self.active) else {
            return;
        };
        let Some(s) = self
            .wf
            .snaps
            .get(&o.doc.id.0)
            .and_then(|l| l.get(index))
            .map(|s| (s.name.clone(), s.state.clone()))
        else {
            return;
        };
        o.apply_state(&format!("Restaurar “{}”", s.0), s.1);
    }

    /// Janela do histórico visual e dos instantâneos do documento ativo.
    pub(super) fn history_window(&mut self, ctx: &egui::Context) {
        // Instantâneos de documentos fechados são descartados.
        let live: HashSet<u64> = self.docs.iter().map(|d| d.doc.id.0).collect();
        self.wf.snaps.retain(|k, _| live.contains(k));
        if !self.wf.history_open {
            return;
        }
        let mut open = true;
        let mut jump: Option<(usize, bool)> = None;
        let mut take = false;
        let mut restore = None;
        let mut delete = None;
        let mut snap_name = std::mem::take(&mut self.wf.snap_name);
        let budget = self.budget_bytes();
        let used = snapshot_bytes(self.wf.snaps.values().flatten().map(|s| &s.state));
        egui::Window::new("Histórico e instantâneos")
            .open(&mut open)
            .default_width(340.0)
            .show(ctx, |ui| {
                let Some(o) = self.docs.get(self.active) else {
                    ui.label("Nenhum documento aberto.");
                    return;
                };
                if o.qm.is_some() {
                    ui.small("Saia da máscara rápida para navegar no histórico.");
                }
                ui.strong("Histórico");
                egui::ScrollArea::vertical()
                    .id_salt("hist-list")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        let done = o.history.labels();
                        let redo = o.history.redo_labels();
                        ui.label(
                            egui::RichText::new("Estado inicial").color(ui.visuals().weak_text_color()),
                        );
                        for (i, l) in done.iter().enumerate() {
                            let last = i + 1 == done.len();
                            if ui.selectable_label(last, *l).clicked() && !last {
                                jump = Some((done.len() - 1 - i, false));
                            }
                        }
                        for (i, l) in redo.iter().enumerate() {
                            let r = ui.selectable_label(
                                false,
                                egui::RichText::new(*l).color(ui.visuals().weak_text_color()),
                            );
                            if r.clicked() {
                                jump = Some((i + 1, true));
                            }
                        }
                    });
                ui.small("Clique numa linha para voltar ou avançar até ela. Linhas esmaecidas são refazíveis.");
                ui.separator();
                ui.strong("Instantâneos");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut snap_name)
                            .hint_text("Nome (opcional)")
                            .desired_width(150.0),
                    );
                    take = ui.button("Criar instantâneo").clicked();
                });
                ui.small(format!(
                    "Memória: {:.1} de {} MiB (ajuste em Preferências)",
                    used as f64 / (1024.0 * 1024.0),
                    budget / (1024 * 1024)
                ));
                if let Some(list) = self.wf.snaps.get(&o.doc.id.0) {
                    for (i, s) in list.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "{} · {}×{} · {} camadas",
                                s.name,
                                s.state.width,
                                s.state.height,
                                s.state.layers.len()
                            ));
                            if ui.small_button("Restaurar").clicked() {
                                restore = Some(i);
                            }
                            if ui.small_button("✕").on_hover_text("Apagar").clicked() {
                                delete = Some(i);
                            }
                        });
                    }
                }
                ui.small("Restaurar é uma ação desfazível. Linha do tempo e vídeo não fazem parte do instantâneo.");
            });
        self.wf.snap_name = snap_name;
        self.wf.history_open = open;
        if let Some((n, redo)) = jump {
            if let Some(o) = self.docs.get_mut(self.active) {
                if o.qm.is_none() {
                    for _ in 0..n {
                        let ok = if redo {
                            o.history.redo(&mut o.doc)
                        } else {
                            o.history.undo(&mut o.doc)
                        };
                        if !ok {
                            break;
                        }
                    }
                }
            }
        }
        if take {
            self.snapshot_take();
        }
        if let Some(i) = restore {
            self.snapshot_restore(i);
        }
        if let Some(i) = delete {
            if let Some(o) = self.docs.get(self.active) {
                if let Some(l) = self.wf.snaps.get_mut(&o.doc.id.0) {
                    if i < l.len() {
                        l.remove(i);
                    }
                }
            }
        }
    }

    /// Janela de ajuda local com busca e exemplos originais.
    pub(super) fn help_window(&mut self, ctx: &egui::Context) {
        if !self.wf.help_open {
            return;
        }
        let mut open = true;
        let mut example = None;
        let mut query = std::mem::take(&mut self.wf.help_query);
        egui::Window::new("Ajuda")
            .open(&mut open)
            .default_width(460.0)
            .default_height(420.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Buscar");
                    ui.add(
                        egui::TextEdit::singleline(&mut query)
                            .hint_text("palavra ou tema")
                            .desired_width(260.0),
                    );
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let hits = help_matches(&query);
                    if hits.is_empty() {
                        ui.label("Nada encontrado.");
                    }
                    for i in hits {
                        ui.collapsing(HELP[i].0, |ui| {
                            ui.label(HELP[i].1);
                        });
                    }
                    ui.separator();
                    ui.strong("Exemplos originais");
                    ui.small("Gerados pelo próprio aplicativo; abrem como documentos novos.");
                    for (i, (name, desc)) in examples::EXAMPLES.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.button(*name).clicked() {
                                example = Some(i);
                            }
                            ui.label(*desc);
                        });
                    }
                });
            });
        self.wf.help_query = query;
        self.wf.help_open = open;
        if let Some(i) = example {
            self.open_example(i);
        }
    }

    fn import_resource(&mut self) {
        let Some(p) = rfd::FileDialog::new()
            .add_filter("Recursos (paleta .gpl, gradiente .ggr)", &["gpl", "ggr"])
            .add_filter("Todos os arquivos", &["*"])
            .pick_file()
        else {
            return;
        };
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_string();
        if let Some(why) = resources::unsupported_reason(&ext) {
            self.show_message("Importar recurso", why.to_string());
            return;
        }
        let bytes = match read_limited(&p, resources::MAX_BYTES as u64) {
            Ok(b) => b,
            Err(e) => return self.show_message("Importar recurso", e),
        };
        match ext.to_ascii_lowercase().as_str() {
            "gpl" => match resources::parse_gpl(&bytes) {
                Ok(pal) => {
                    let before = self.prefs.swatches.len();
                    for c in pal.colors {
                        if !self.prefs.swatches.contains(&c) && self.prefs.swatches.len() < 200 {
                            self.prefs.swatches.push(c);
                        }
                    }
                    let n = self.prefs.swatches.len() - before;
                    self.prefs.save();
                    self.show_message(
                        "Importar recurso",
                        format!(
                            "Paleta “{}”: {n} cor(es) adicionada(s) às amostras.",
                            pal.name
                        ),
                    );
                }
                Err(e) => self.show_message("Importar recurso", e),
            },
            "ggr" => match resources::parse_ggr(&bytes) {
                Ok(g) => {
                    let ok = presets::save_gradient(&g.name, &g.stops);
                    let msg = if ok {
                        format!("Gradiente “{}” salvo nas predefinições.", g.name)
                    } else {
                        "Gradiente inválido.".to_string()
                    };
                    self.show_message("Importar recurso", msg);
                }
                Err(e) => self.show_message("Importar recurso", e),
            },
            _ => self.show_message(
                "Importar recurso",
                "Formato não suportado. Aceitos: .gpl (paleta) e .ggr (gradiente).".into(),
            ),
        }
    }

    fn bundle_export(&mut self) {
        let b = Bundle {
            docs: self.prefs.doc_presets.clone(),
            exports: self.prefs.export_presets.clone(),
            gradients: presets::gradients(),
            workspaces: self.prefs.workspaces.clone(),
            ..Bundle::default()
        };
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Predefinições do Orvalith", &["orvapreset"])
            .set_file_name("predefinicoes.orvapreset")
            .save_file()
        {
            if let Err(e) = std::fs::write(&p, presets::bundle_to_json(&b)) {
                self.show_message("Exportar pacote", format!("Falha ao gravar: {e}"));
            }
        }
    }

    fn bundle_import(&mut self) {
        let Some(p) = rfd::FileDialog::new()
            .add_filter("Predefinições do Orvalith", &["orvapreset", "json"])
            .pick_file()
        else {
            return;
        };
        let text = read_limited(&p, presets::MAX_BUNDLE_BYTES)
            .and_then(|b| String::from_utf8(b).map_err(|_| "Arquivo não é texto UTF-8".into()));
        match text.and_then(|t| presets::bundle_from_json(&t)) {
            Ok(b) => {
                let n = b.docs.len() + b.exports.len() + b.gradients.len() + b.workspaces.len();
                for d in b.docs {
                    let name = d.name.clone();
                    presets::upsert(&mut self.prefs.doc_presets, d, |x| x.name == name);
                }
                for e in b.exports {
                    let name = e.name.clone();
                    presets::upsert(&mut self.prefs.export_presets, e, |x| x.name == name);
                }
                for g in b.gradients {
                    presets::save_gradient(&g.name, &g.stops);
                }
                for w in b.workspaces {
                    let name = w.name.clone();
                    presets::upsert(&mut self.prefs.workspaces, w, |x| x.name == name);
                }
                self.prefs.save();
                self.show_message(
                    "Importar pacote",
                    format!("{n} predefinição(ões) importada(s)."),
                );
            }
            Err(e) => self.show_message("Importar pacote", e),
        }
    }

    /// Gerenciador de predefinições.
    pub(super) fn presets_window(&mut self, ctx: &egui::Context) {
        if let Some(g) = presets::take_changed_gradients() {
            self.prefs.gradient_presets = g;
            self.prefs.save();
        }
        if !self.wf.presets_open {
            return;
        }
        let mut open = true;
        let mut changed = false;
        let mut import_res = false;
        let mut b_export = false;
        let mut b_import = false;
        let mut save_ws = false;
        let mut save_doc = false;
        let mut tab = self.wf.tab;
        let mut doc_name = std::mem::take(&mut self.wf.doc_name);
        let mut ws_name = std::mem::take(&mut self.wf.ws_name);
        let mut del_grad = None;
        egui::Window::new("Predefinições")
            .open(&mut open)
            .default_width(440.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, t) in ["Documento", "Exportação", "Gradientes", "Espaços"]
                        .iter()
                        .enumerate()
                    {
                        ui.selectable_value(&mut tab, i, *t);
                    }
                });
                ui.separator();
                match tab {
                    0 => {
                        ui.small("Embutidas: fotos 3:2, 2:3 e 4:3, quadrado, retrato 4:5, 16:9, 4K, A4 e ícone (aparecem em Novo documento e na tela inicial).");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut doc_name)
                                    .hint_text("Nome")
                                    .desired_width(140.0),
                            );
                            save_doc = ui
                                .button("Salvar tamanho do documento ativo")
                                .clicked();
                        });
                        let mut del = None;
                        for (i, p) in self.prefs.doc_presets.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                changed |= ui.text_edit_singleline(&mut p.name).changed();
                                ui.label(format!("{}×{} · {:.0} ppi", p.width, p.height, p.ppi));
                                if ui.small_button("✕").on_hover_text("Apagar").clicked() {
                                    del = Some(i);
                                }
                            });
                        }
                        if let Some(i) = del {
                            self.prefs.doc_presets.remove(i);
                            changed = true;
                        }
                    }
                    1 => {
                        ui.small("Guardam formato, qualidade, lado máximo, fundo e perfil. Crie e use no diálogo Exportar imagem.");
                        let mut del = None;
                        for (i, p) in self.prefs.export_presets.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                changed |= ui.text_edit_singleline(&mut p.name).changed();
                                ui.label(format!(
                                    "{} · q{} · lado {}",
                                    p.ext,
                                    p.quality,
                                    if p.max_side == 0 {
                                        "original".to_string()
                                    } else {
                                        p.max_side.to_string()
                                    }
                                ));
                                if ui.small_button("✕").on_hover_text("Apagar").clicked() {
                                    del = Some(i);
                                }
                            });
                        }
                        if let Some(i) = del {
                            self.prefs.export_presets.remove(i);
                            changed = true;
                        }
                    }
                    2 => {
                        ui.small("Salve gradientes no editor de gradiente (Mapa de gradiente, estilos). Clique no nome ali para usar.");
                        let list = presets::gradients();
                        if list.is_empty() {
                            ui.label("Nenhum gradiente salvo.");
                        }
                        for (i, g) in list.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{} ({} paradas)", g.name, g.stops.len()));
                                if ui.small_button("✕").on_hover_text("Apagar").clicked() {
                                    del_grad = Some(i);
                                }
                            });
                        }
                    }
                    _ => {
                        ui.small("Embutidos: Pintura, Fotografia, Animação, Mínimo (menu Visualizar).");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut ws_name)
                                    .hint_text("Nome")
                                    .desired_width(140.0),
                            );
                            save_ws = ui.button("Salvar disposição atual").clicked();
                        });
                        let mut del = None;
                        for (i, w) in self.prefs.workspaces.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                changed |= ui.text_edit_singleline(&mut w.name).changed();
                                if ui.small_button("✕").on_hover_text("Apagar").clicked() {
                                    del = Some(i);
                                }
                            });
                        }
                        if let Some(i) = del {
                            self.prefs.workspaces.remove(i);
                            changed = true;
                        }
                    }
                }
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    b_export = ui.button("Exportar pacote…").clicked();
                    b_import = ui.button("Importar pacote…").clicked();
                    import_res = ui
                        .button("Importar paleta/gradiente GIMP…")
                        .on_hover_text(".gpl vira amostras; .ggr vira predefinição de gradiente")
                        .clicked();
                });
                ui.small("Não suportados: pincéis .abr, amostras .ase/.aco e gradientes .grd.");
            });
        self.wf.tab = tab;
        self.wf.doc_name = doc_name;
        self.wf.ws_name = ws_name;
        self.wf.presets_open = open;
        if let Some(i) = del_grad {
            presets::remove_gradient(i);
        }
        if save_doc {
            if let Some(o) = self.docs.get(self.active) {
                let name: String = self.wf.doc_name.trim().chars().take(60).collect();
                if !name.is_empty() {
                    let p = presets::DocPreset {
                        name: name.clone(),
                        width: o.doc.width,
                        height: o.doc.height,
                        ppi: o.doc.ppi,
                        transparent: false,
                    };
                    presets::upsert(&mut self.prefs.doc_presets, p, |x| x.name == name);
                    self.wf.doc_name.clear();
                    changed = true;
                }
            }
        }
        if save_ws {
            let name: String = self.wf.ws_name.trim().chars().take(60).collect();
            if !name.is_empty() {
                let w = presets::Workspace {
                    name: name.clone(),
                    layout: self.current_layout(),
                };
                presets::upsert(&mut self.prefs.workspaces, w, |x| x.name == name);
                self.wf.ws_name.clear();
                changed = true;
            }
        }
        if changed {
            self.prefs.save();
        }
        if import_res {
            self.import_resource();
        }
        if b_export {
            self.bundle_export();
        }
        if b_import {
            self.bundle_import();
        }
    }
}

/// Lê um arquivo recusando os maiores que `max` bytes.
fn read_limited(p: &std::path::Path, max: u64) -> Result<Vec<u8>, String> {
    let len = std::fs::metadata(p)
        .map_err(|e| format!("Não foi possível ler o arquivo: {e}"))?
        .len();
    if len > max {
        return Err(format!("Arquivo grande demais (máximo {} KiB)", max / 1024));
    }
    std::fs::read(p).map_err(|e| format!("Não foi possível ler o arquivo: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::Document;

    #[test]
    fn snapshot_bytes_counts_shared_layers_once() {
        let d = Document::new("a", 10, 10, Some([1, 2, 3, 255])).unwrap();
        let a = DocState::of(&d);
        let b = DocState::of(&d);
        let one = snapshot_bytes([&a].into_iter());
        assert_eq!(one, 10 * 10 * 4);
        assert_eq!(
            snapshot_bytes([&a, &b].into_iter()),
            one,
            "Arc compartilhado"
        );
        let mut d2 = d.clone();
        d2.layers[0].set_pixel(0, 0, [9, 9, 9, 255]);
        let c = DocState::of(&d2);
        assert_eq!(snapshot_bytes([&a, &c].into_iter()), one * 2);
    }

    #[test]
    fn help_search_matches_all_words_case_insensitively() {
        assert_eq!(help_matches("").len(), HELP.len());
        let h = help_matches("GIMP");
        assert!(h.iter().any(|&i| HELP[i].0 == "Predefinições"));
        assert!(help_matches("paleta comandos").contains(&1));
        assert!(help_matches("zzzxyz").is_empty());
    }
}
