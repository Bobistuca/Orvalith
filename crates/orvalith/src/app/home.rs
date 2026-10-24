//! Tela inicial (sem documento aberto): navegação lateral, recentes reais e predefinições.
//!
//! Tudo aqui é local: os metadados dos arquivos recentes são lidos num fio em segundo plano e
//! nenhuma miniatura é inventada (veja docs/KNOWN_ISSUES.md).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align, Align2, FontId, Layout, Rect, Sense, Stroke, StrokeKind, Vec2};
use orvalith_core::examples;

use super::commands;
use super::{Action, OrvalithApp};
use crate::icons::{self, Icon};
use crate::kit::{self, Kind};
use crate::presets;
use crate::theme::{metrics as m, Palette};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Section {
    #[default]
    Start,
    Recent,
    Presets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileInfo {
    pub exists: bool,
    pub size: u64,
    /// Segundos desde 1970 (UTC).
    pub modified: Option<u64>,
}

#[derive(Default)]
pub(super) struct HomeUi {
    pub section: Section,
    infos: HashMap<String, FileInfo>,
    rx: Option<Receiver<(String, FileInfo)>>,
    requested: Vec<String>,
    refresh: bool,
    was_focused: bool,
    /// Dicas de atalho (id do comando → texto), refeitas quando o mapa de atalhos muda.
    pub hints: Option<HashMap<&'static str, String>>,
}

// ---------------------------------------------------------------------------------------
// Funções puras (testadas)

/// Tamanho legível com vírgula decimal ("1,5 MiB").
pub(super) fn format_size(bytes: u64) -> String {
    const U: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", U[i]).replace('.', ",")
    }
}

/// Data civil (UTC) a partir de dias desde 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if mo <= 2 { y + 1 } else { y }, mo, d)
}

/// "agora", "há 5 min", "há 3 h", "ontem", "há 12 dias" ou a data (UTC) quando for antigo.
pub(super) fn relative_age(now: u64, then: u64) -> String {
    if then >= now || now - then < 60 {
        return "agora".into();
    }
    let d = now - then;
    match d {
        0..=3599 => format!("há {} min", d / 60),
        3600..=86_399 => format!("há {} h", d / 3600),
        86_400..=172_799 => "ontem".into(),
        172_800..=2_591_999 => format!("há {} dias", d / 86_400),
        _ => {
            let (y, mo, da) = civil_from_days((then / 86_400) as i64);
            format!("{da:02}/{mo:02}/{y}")
        }
    }
}

/// Encurta o meio de um texto longo ("C:\Users\…\Fotos") sem cortar no meio de um caractere.
pub(super) fn shorten_middle(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max || max < 5 {
        return s.to_string();
    }
    let keep = max - 1;
    let head = keep / 2;
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(chars[chars.len() - tail..].iter());
    out
}

/// Retângulo (largura, altura) proporcional a `w:h` que cabe em `max_w × max_h`.
pub(super) fn fit_aspect(w: u32, h: u32, max_w: f32, max_h: f32) -> (f32, f32) {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let s = (max_w / w).min(max_h / h);
    ((w * s).max(6.0), (h * s).max(6.0))
}

/// Texto do selo de tipo de arquivo (extensão em maiúsculas, até 4 letras).
pub(super) fn type_badge(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.chars().take(4).collect::<String>().to_uppercase())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "ARQ".into())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn file_info(path: &str) -> FileInfo {
    match std::fs::metadata(path) {
        Ok(md) if md.is_file() => FileInfo {
            exists: true,
            size: md.len(),
            modified: md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
        },
        _ => FileInfo {
            exists: false,
            size: 0,
            modified: None,
        },
    }
}

const CARD_W: f32 = 168.0;
const CARD_H: f32 = 116.0;
const ROW_H: f32 = 56.0;
const START_RECENT_ROWS: usize = 5;

impl OrvalithApp {
    /// Lê em segundo plano os metadados dos recentes (sem travar a interface).
    fn home_poll(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focused && !self.home.was_focused {
            self.home.refresh = true;
        }
        self.home.was_focused = focused;
        if let Some(rx) = &self.home.rx {
            let mut got = false;
            while let Ok((p, info)) = rx.try_recv() {
                self.home.infos.insert(p, info);
                got = true;
            }
            if got {
                ctx.request_repaint();
            }
        }
        let list: Vec<String> = if self.prefs.recent_enabled {
            self.prefs.recent.clone()
        } else {
            Vec::new()
        };
        if list != self.home.requested || (self.home.refresh && !list.is_empty()) {
            self.home.requested = list.clone();
            self.home.refresh = false;
            self.home.infos.retain(|k, _| list.contains(k));
            if !list.is_empty() {
                let (tx, rx) = mpsc::channel();
                self.home.rx = Some(rx);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    for p in list {
                        let info = file_info(&p);
                        if tx.send((p, info)).is_err() {
                            return;
                        }
                    }
                    ctx.request_repaint();
                });
            }
        }
    }

    fn hint(&mut self, id: &'static str) -> String {
        if self.home.hints.is_none() {
            let mut map = HashMap::new();
            for c in commands::registry() {
                if let Some(b) = commands::effective(&c, &self.prefs.keymap) {
                    for want in ["file.new", "file.open", "app.help"] {
                        if c.id == want {
                            map.insert(want, b.text());
                        }
                    }
                }
            }
            self.home.hints = Some(map);
        }
        self.home
            .hints
            .as_ref()
            .and_then(|h| h.get(id).cloned())
            .unwrap_or_default()
    }

    /// Painel lateral compacto da tela inicial.
    pub(super) fn home_sidebar(&mut self, root: &mut egui::Ui, actions: &mut Vec<Action>) {
        self.home_poll(&root.ctx().clone());
        let pal = Palette::of(root);
        let hint_new = self.hint("file.new");
        let hint_open = self.hint("file.open");
        let hint_help = self.hint("app.help");
        egui::Panel::left("home_nav")
            .resizable(false)
            .exact_size(m::SIDEBAR_W)
            .frame(
                egui::Frame::new()
                    .fill(pal.surface)
                    .stroke(Stroke::new(m::STROKE, pal.border))
                    .inner_margin(m::SPACE_LG),
            )
            .show(root, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(m::SPACE_SM, m::SPACE_SM);
                // Marca
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(r, m::RADIUS, pal.accent);
                    p.circle_stroke(r.center(), 8.0, Stroke::new(2.5, pal.on_accent));
                    p.circle_filled(r.center() + Vec2::new(3.0, -3.0), 2.2, pal.on_accent);
                    ui.add_space(m::SPACE_XS);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(
                            egui::RichText::new("Orvalith")
                                .size(18.0)
                                .strong()
                                .color(pal.text),
                        );
                        ui.label(
                            egui::RichText::new(concat!("versão ", env!("CARGO_PKG_VERSION")))
                                .size(m::FONT_SMALL)
                                .color(pal.faint),
                        );
                    });
                });
                ui.add_space(m::SPACE_MD);
                if kit::button(
                    ui,
                    Kind::Primary,
                    Some(Icon::NewDoc),
                    "Novo documento",
                    &hint_new,
                    0.0,
                    true,
                )
                .clicked()
                {
                    actions.push(Action::New);
                }
                if kit::button(
                    ui,
                    Kind::Secondary,
                    Some(Icon::Folder),
                    "Abrir arquivo",
                    &hint_open,
                    0.0,
                    true,
                )
                .clicked()
                {
                    actions.push(Action::Open);
                }
                ui.add_space(m::SPACE_MD);
                for (sec, icon, label) in [
                    (Section::Start, Icon::Home, "Início"),
                    (Section::Recent, Icon::Clock, "Recentes"),
                    (Section::Presets, Icon::Grid, "Predefinições"),
                ] {
                    if kit::nav_item(ui, icon, label, self.home.section == sec).clicked() {
                        self.home.section = sec;
                    }
                }
                // Rodapé fixo na base do painel.
                let bottom_h = 2.0 * m::CONTROL_H + 3.0 * m::SPACE_SM + 46.0;
                ui.add_space((ui.available_height() - bottom_h).max(m::SPACE_MD));
                if kit::button(
                    ui,
                    Kind::Ghost,
                    Some(Icon::Help),
                    "Ajuda",
                    &hint_help,
                    0.0,
                    true,
                )
                .clicked()
                {
                    actions.push(Action::Help);
                }
                if kit::button(
                    ui,
                    Kind::Ghost,
                    Some(Icon::Sliders),
                    "Preferências",
                    "",
                    0.0,
                    true,
                )
                .clicked()
                {
                    actions.push(Action::Preferences);
                }
                ui.horizontal_top(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(m::ICON), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Shield, pal.faint);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new("Seus arquivos ficam neste computador.")
                                .size(m::FONT_SMALL)
                                .color(pal.faint),
                        )
                        .wrap(),
                    );
                });
            });
    }

    /// Área principal da tela inicial.
    pub(super) fn home_content(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let pal = Palette::of(ui);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Frame::new().inner_margin(m::SPACE_XL).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(m::SPACE_MD, m::SPACE_MD);
                    let (title, sub) = match self.home.section {
                        Section::Start => {
                            ("Início", "Crie um documento ou continue de onde parou.")
                        }
                        Section::Recent => (
                            "Recentes",
                            "Arquivos que você abriu ou salvou neste computador.",
                        ),
                        Section::Presets => (
                            "Predefinições",
                            "Tamanhos prontos para começar um documento novo.",
                        ),
                    };
                    ui.label(
                        egui::RichText::new(title)
                            .size(m::FONT_TITLE)
                            .strong()
                            .color(pal.text),
                    );
                    kit::muted(ui, sub);
                    ui.add_space(m::SPACE_SM);
                    match self.home.section {
                        Section::Start => {
                            self.home_recent(ui, actions, Some(START_RECENT_ROWS));
                            ui.add_space(m::SPACE_MD);
                            self.home_presets(ui, actions);
                        }
                        Section::Recent => self.home_recent(ui, actions, None),
                        Section::Presets => self.home_presets(ui, actions),
                    }
                });
            });
    }

    fn home_recent(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>, limit: Option<usize>) {
        let pal = Palette::of(ui);
        ui.horizontal(|ui| {
            kit::section_heading_inline(ui, "Recentes");
            if self.prefs.recent_enabled && !self.prefs.recent.is_empty() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Limpar lista")
                                    .size(m::FONT_SMALL)
                                    .color(pal.muted),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Esquece todos os arquivos recentes (não apaga os arquivos)")
                        .clicked()
                    {
                        actions.push(Action::ClearRecent);
                    }
                    if let Some(n) = limit {
                        if self.prefs.recent.len() > n
                            && ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(format!(
                                            "Ver todos ({})",
                                            self.prefs.recent.len()
                                        ))
                                        .size(m::FONT_SMALL)
                                        .color(pal.accent),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                        {
                            self.home.section = Section::Recent;
                        }
                    }
                });
            }
        });
        if !self.prefs.recent_enabled {
            self.home_empty(
                ui,
                actions,
                "O histórico de recentes está desligado",
                "Nenhum caminho de arquivo é guardado. Você pode ligar o histórico quando quiser e limpá-lo a qualquer momento.",
                true,
            );
            return;
        }
        if self.prefs.recent.is_empty() {
            self.home_empty(
                ui,
                actions,
                "Nenhum arquivo recente ainda",
                "Os arquivos que você abrir ou salvar aparecem aqui. A lista fica só neste computador.",
                false,
            );
            return;
        }
        let list: Vec<String> = self
            .prefs
            .recent
            .iter()
            .take(limit.unwrap_or(usize::MAX))
            .cloned()
            .collect();
        let now = now_secs();
        ui.spacing_mut().item_spacing.y = m::SPACE_SM;
        for path in list {
            let info = self.home.infos.get(&path).copied();
            self.recent_row(ui, actions, &path, info, now);
        }
    }

    fn recent_row(
        &mut self,
        ui: &mut egui::Ui,
        actions: &mut Vec<Action>,
        path: &str,
        info: Option<FileInfo>,
        now: u64,
    ) {
        let pal = Palette::of(ui);
        let missing = info.is_some_and(|i| !i.exists);
        let name = Path::new(path)
            .file_name()
            .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
        let folder = Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (r, rect, _) = kit::card(ui, Vec2::new(ui.available_width(), ROW_H), &name);
        let r = if missing {
            r.on_hover_text("Arquivo não encontrado. Use ✕ para remover da lista.")
        } else {
            r.on_hover_text(path)
        };
        let p = ui.painter().clone();
        let fg = if missing { pal.faint } else { pal.text };
        // Selo do tipo de arquivo
        let chip = Rect::from_center_size(
            egui::pos2(rect.left() + m::SPACE_MD + 20.0, rect.center().y),
            Vec2::splat(40.0),
        );
        p.rect_filled(chip, m::RADIUS, pal.bg);
        p.rect_stroke(
            chip,
            m::RADIUS,
            Stroke::new(m::STROKE, pal.border),
            StrokeKind::Inside,
        );
        if missing {
            icons::paint(
                &p,
                Rect::from_center_size(chip.center(), Vec2::splat(m::ICON)),
                Icon::Alert,
                pal.warn,
            );
        } else {
            kit::put_text(
                ui,
                chip.center(),
                Align2::CENTER_CENTER,
                &type_badge(path),
                10.5,
                pal.accent,
                chip.width() - 4.0,
            );
        }
        // Colunas à direita: remover, tamanho, idade.
        let rm = Rect::from_center_size(
            egui::pos2(rect.right() - m::SPACE_MD - 12.0, rect.center().y),
            Vec2::splat(24.0),
        );
        let meta_right = rm.left() - m::SPACE_SM;
        let meta = match info {
            Some(i) if i.exists => {
                let age = i.modified.map(|t| relative_age(now, t)).unwrap_or_default();
                if age.is_empty() {
                    format_size(i.size)
                } else {
                    format!("{age} · {}", format_size(i.size))
                }
            }
            Some(_) => "Arquivo não encontrado".to_string(),
            None => String::new(),
        };
        let meta_color = if missing { pal.warn } else { pal.muted };
        let mut text_right = meta_right;
        if !meta.is_empty() {
            let g =
                ui.painter()
                    .layout_no_wrap(meta, FontId::proportional(m::FONT_SMALL), meta_color);
            let w = g.size().x;
            if rect.width() > 460.0 {
                p.galley(
                    egui::pos2(meta_right - w, rect.center().y - g.size().y / 2.0),
                    g,
                    meta_color,
                );
                text_right = meta_right - w - m::SPACE_MD;
            }
        }
        let tx = chip.right() + m::SPACE_MD;
        let max_w = (text_right - tx).max(40.0);
        kit::put_text(
            ui,
            egui::pos2(tx, rect.center().y - 9.0),
            Align2::LEFT_CENTER,
            &name,
            m::FONT_BODY,
            fg,
            max_w,
        );
        kit::put_text(
            ui,
            egui::pos2(tx, rect.center().y + 10.0),
            Align2::LEFT_CENTER,
            &shorten_middle(&folder, 70),
            m::FONT_SMALL,
            pal.faint,
            max_w,
        );
        // Botão de remover (por cima do cartão).
        let rr = ui.interact(rm, r.id.with("rm"), Sense::click());
        if rr.hovered() || r.hovered() {
            if rr.hovered() {
                p.rect_filled(rm, m::RADIUS_SM, pal.pressed);
            }
            icons::paint(
                &p,
                rm.shrink(5.0),
                Icon::Close,
                if rr.hovered() { pal.text } else { pal.muted },
            );
        }
        let rr = rr.on_hover_text("Remover da lista");
        if rr.clicked() {
            actions.push(Action::ForgetRecent(path.to_string()));
        } else if r.clicked() && !missing {
            actions.push(Action::OpenPath(PathBuf::from(path)));
        }
    }

    /// Estado vazio dos recentes (lista vazia ou histórico desligado).
    fn home_empty(
        &mut self,
        ui: &mut egui::Ui,
        actions: &mut Vec<Action>,
        title: &str,
        body: &str,
        offer_enable: bool,
    ) {
        let pal = Palette::of(ui);
        egui::Frame::new()
            .fill(pal.surface)
            .stroke(Stroke::new(m::STROKE, pal.border))
            .corner_radius(m::RADIUS_LG)
            .inner_margin(m::SPACE_XL)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Image, pal.faint);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = m::SPACE_SM;
                        ui.label(
                            egui::RichText::new(title)
                                .size(m::FONT_BODY)
                                .strong()
                                .color(pal.text),
                        );
                        kit::muted(ui, body);
                        ui.add_space(m::SPACE_XS);
                        ui.horizontal_wrapped(|ui| {
                            if offer_enable {
                                if kit::button(
                                    ui,
                                    Kind::Secondary,
                                    None,
                                    "Ligar histórico de recentes",
                                    "",
                                    220.0,
                                    true,
                                )
                                .clicked()
                                {
                                    actions.push(Action::EnableRecent);
                                }
                            } else {
                                if kit::button(
                                    ui,
                                    Kind::Secondary,
                                    Some(Icon::Folder),
                                    "Abrir arquivo",
                                    "",
                                    160.0,
                                    true,
                                )
                                .clicked()
                                {
                                    actions.push(Action::Open);
                                }
                                ui.label(
                                    egui::RichText::new("ou abra um exemplo:")
                                        .size(m::FONT_SMALL)
                                        .color(pal.muted),
                                );
                                for (i, (name, _)) in examples::EXAMPLES.iter().enumerate() {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(*name)
                                                    .size(m::FONT_SMALL)
                                                    .color(pal.accent),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        actions.push(Action::OpenExample(i));
                                    }
                                }
                            }
                        });
                    });
                });
            });
    }

    fn home_presets(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let pal = Palette::of(ui);
        kit::section_heading_inline(ui, "Novo a partir de uma predefinição");
        let mut all = presets::builtin_doc_presets();
        all.extend(self.prefs.doc_presets.iter().cloned());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(m::SPACE_MD);
            for pr in &all {
                let label = format!("{}, {} por {} pixels", pr.name, pr.width, pr.height);
                let (r, rect, hovered) = kit::card(ui, Vec2::new(CARD_W, CARD_H), &label);
                // Prévia: retângulo na proporção real do documento.
                let (bw, bh) = fit_aspect(pr.width, pr.height, 56.0, 44.0);
                let box_rect = Rect::from_center_size(
                    egui::pos2(rect.center().x, rect.top() + 14.0 + 22.0),
                    Vec2::new(bw, bh),
                );
                let p = ui.painter();
                p.rect_filled(box_rect, 2.0, pal.bg);
                p.rect_stroke(
                    box_rect,
                    2.0,
                    Stroke::new(1.5, if hovered { pal.accent } else { pal.faint }),
                    StrokeKind::Inside,
                );
                let x = rect.left() + m::SPACE_MD;
                let w = rect.width() - 2.0 * m::SPACE_MD;
                kit::put_text(
                    ui,
                    egui::pos2(x, rect.top() + 74.0),
                    Align2::LEFT_CENTER,
                    &pr.name,
                    m::FONT_BODY,
                    pal.text,
                    w,
                );
                let dims = format!("{} × {} px · {:.0} ppi", pr.width, pr.height, pr.ppi);
                kit::put_text(
                    ui,
                    egui::pos2(x, rect.top() + 94.0),
                    Align2::LEFT_CENTER,
                    &dims,
                    m::FONT_SMALL,
                    pal.muted,
                    w,
                );
                if r.clicked() {
                    actions.push(Action::NewSized {
                        width: pr.width,
                        height: pr.height,
                        ppi: pr.ppi,
                        transparent: pr.transparent,
                    });
                }
            }
            // Cartão final: diálogo completo.
            let (r, rect, hovered) = kit::card(ui, Vec2::new(CARD_W, CARD_H), "Personalizado");
            let c = egui::pos2(rect.center().x, rect.top() + 36.0);
            icons::paint(
                ui.painter(),
                Rect::from_center_size(c, Vec2::splat(24.0)),
                Icon::Plus,
                if hovered { pal.accent } else { pal.muted },
            );
            let x = rect.left() + m::SPACE_MD;
            let w = rect.width() - 2.0 * m::SPACE_MD;
            kit::put_text(
                ui,
                egui::pos2(x, rect.top() + 74.0),
                Align2::LEFT_CENTER,
                "Personalizado…",
                m::FONT_BODY,
                pal.text,
                w,
            );
            kit::put_text(
                ui,
                egui::pos2(x, rect.top() + 94.0),
                Align2::LEFT_CENTER,
                "Medidas à sua escolha",
                m::FONT_SMALL,
                pal.muted,
                w,
            );
            if r.clicked() {
                actions.push(Action::New);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_binary_units_and_decimal_comma() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1536), "1,5 KiB");
        assert_eq!(format_size(5 * 1024 * 1024), "5,0 MiB");
        assert!(format_size(u64::MAX).ends_with("GiB"));
    }

    #[test]
    fn relative_age_buckets_and_old_dates() {
        let now = 1_800_000_000;
        assert_eq!(relative_age(now, now), "agora");
        assert_eq!(relative_age(now, now + 500), "agora", "futuro não quebra");
        assert_eq!(relative_age(now, now - 300), "há 5 min");
        assert_eq!(relative_age(now, now - 7200), "há 2 h");
        assert_eq!(relative_age(now, now - 100_000), "ontem");
        assert_eq!(relative_age(now, now - 5 * 86_400), "há 5 dias");
        // 2000-03-01 00:00:00 UTC = 951_868_800
        assert_eq!(relative_age(now, 951_868_800), "01/03/2000");
        // 1970-01-01
        assert_eq!(relative_age(now, 0), "01/01/1970");
    }

    #[test]
    fn civil_dates_match_known_values() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }

    #[test]
    fn middle_ellipsis_is_char_safe() {
        assert_eq!(shorten_middle("curto", 10), "curto");
        let s = shorten_middle("C:\\Usuários\\João\\Fotos\\Férias 2026\\Praia", 20);
        assert_eq!(s.chars().count(), 20);
        assert!(s.contains('…') && s.starts_with("C:\\") && s.ends_with("Praia"));
        assert_eq!(
            shorten_middle("abcdefghij", 3),
            "abcdefghij",
            "limite pequeno demais: mantém"
        );
    }

    #[test]
    fn aspect_fits_the_box_and_keeps_ratio() {
        let (w, h) = fit_aspect(1920, 1080, 56.0, 44.0);
        assert!(w <= 56.0 + 0.01 && h <= 44.0 + 0.01);
        assert!(((w / h) - (1920.0 / 1080.0)).abs() < 0.01);
        let (w, h) = fit_aspect(1080, 1350, 56.0, 44.0);
        assert!(h > w, "retrato");
        let (w, h) = fit_aspect(0, 0, 56.0, 44.0);
        assert!(w >= 6.0 && h >= 6.0);
    }

    #[test]
    fn badge_comes_from_the_extension() {
        assert_eq!(type_badge("a/b/foto.png"), "PNG");
        assert_eq!(type_badge("x.orva"), "ORVA");
        assert_eq!(type_badge("x.webpxyz"), "WEBP");
        assert_eq!(type_badge("semextensao"), "ARQ");
    }

    #[test]
    fn file_info_reports_missing_and_existing_files() {
        let d = std::env::temp_dir().join(format!("orvalith-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.png");
        std::fs::write(&f, b"12345").unwrap();
        let ok = file_info(f.to_str().unwrap());
        assert!(ok.exists && ok.size == 5 && ok.modified.is_some());
        assert!(!file_info(d.join("nao.png").to_str().unwrap()).exists);
        assert!(
            !file_info(d.to_str().unwrap()).exists,
            "pasta não é arquivo"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    // ---- Interação por teclado (equivale a clicar: Tab foca, Enter ativa) ----

    fn run_frame(
        app: &mut OrvalithApp,
        ctx: &egui::Context,
        size: Vec2,
        events: Vec<egui::Event>,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            focused: true,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.home_sidebar(ui, &mut actions);
            egui::CentralPanel::default().show(ui, |ui| app.home_content(ui, &mut actions));
        });
        out.textures_delta.clear();
        actions
    }

    fn key(k: egui::Key) -> egui::Event {
        egui::Event::Key {
            key: k,
            physical_key: Some(k),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Percorre os widgets com Tab, ativando cada um com Enter; devolve todas as ações alcançadas.
    fn reach_by_keyboard(app: &mut OrvalithApp, size: Vec2, ppp: f32) -> Vec<Action> {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(ppp);
        run_frame(app, &ctx, size, vec![]);
        let mut found = Vec::new();
        for _ in 0..40 {
            run_frame(app, &ctx, size, vec![key(egui::Key::Tab)]);
            run_frame(app, &ctx, size, vec![]);
            found.extend(run_frame(app, &ctx, size, vec![key(egui::Key::Enter)]));
            // Itens de navegação mudam de seção; volta ao início para continuar percorrendo.
            app.home.section = Section::Start;
            run_frame(app, &ctx, size, vec![]);
        }
        found
    }

    fn app_with(prefs: crate::prefs::Prefs) -> OrvalithApp {
        OrvalithApp::new(prefs)
    }

    #[test]
    fn every_home_action_is_reachable_and_wired_by_keyboard() {
        let mut app = app_with(crate::prefs::Prefs::default());
        let found = reach_by_keyboard(&mut app, Vec2::new(1280.0, 720.0), 1.0);
        let has = |f: &dyn Fn(&Action) -> bool| found.iter().any(f);
        assert!(has(&|a| matches!(a, Action::New)), "Novo documento");
        assert!(has(&|a| matches!(a, Action::Open)), "Abrir arquivo");
        assert!(has(&|a| matches!(a, Action::Help)), "Ajuda");
        assert!(has(&|a| matches!(a, Action::Preferences)), "Preferências");
        assert!(has(&|a| matches!(a, Action::OpenExample(0))), "exemplo");
        let first = presets::builtin_doc_presets();
        for p in &first {
            assert!(
                has(
                    &|a| matches!(a, Action::NewSized { width, height, .. } if *width == p.width && *height == p.height)
                ),
                "predefinição {}",
                p.name
            );
        }
    }

    #[test]
    fn recent_rows_open_real_files_and_ignore_missing_ones() {
        let d = std::env::temp_dir().join(format!("orvalith-home-kb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let real = d.join("real.png");
        std::fs::write(&real, b"x").unwrap();
        let gone = d.join("sumiu.orva");
        let prefs = crate::prefs::Prefs {
            recent: vec![
                real.to_string_lossy().into_owned(),
                gone.to_string_lossy().into_owned(),
            ],
            recent_enabled: true,
            ..Default::default()
        };
        let mut app = app_with(prefs);
        // Deixa o fio em segundo plano ler os metadados e repinta algumas vezes.
        let ctx = egui::Context::default();
        let size = Vec2::new(1280.0, 720.0);
        for _ in 0..5 {
            run_frame(&mut app, &ctx, size, vec![]);
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        assert_eq!(app.home.infos.len(), 2);
        assert!(app.home.infos[&real.to_string_lossy().into_owned()].exists);
        assert!(!app.home.infos[&gone.to_string_lossy().into_owned()].exists);
        let found = reach_by_keyboard(&mut app, size, 1.0);
        assert!(found
            .iter()
            .any(|a| matches!(a, Action::OpenPath(p) if *p == real)));
        assert!(
            !found
                .iter()
                .any(|a| matches!(a, Action::OpenPath(p) if *p == gone)),
            "arquivo ausente não abre"
        );
        assert!(found
            .iter()
            .any(|a| matches!(a, Action::ForgetRecent(p) if *p == gone.to_string_lossy())));
        assert!(found.iter().any(|a| matches!(a, Action::ClearRecent)));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn disabled_history_offers_to_enable_and_lists_nothing() {
        let prefs = crate::prefs::Prefs {
            recent_enabled: false,
            recent: vec!["C:/nao/deve/aparecer.png".into()],
            ..Default::default()
        };
        let mut app = app_with(prefs);
        let found = reach_by_keyboard(&mut app, Vec2::new(1280.0, 720.0), 1.0);
        assert!(found.iter().any(|a| matches!(a, Action::EnableRecent)));
        assert!(!found.iter().any(|a| matches!(a, Action::OpenPath(_))));
        assert!(
            app.home.requested.is_empty(),
            "nenhum metadado lido com histórico desligado"
        );
    }

    #[test]
    fn layout_survives_minimum_and_large_windows_at_several_scales() {
        let mut app = app_with(crate::prefs::Prefs::default());
        for (w, h) in [
            (800.0, 500.0),
            (1280.0, 720.0),
            (1920.0, 1080.0),
            (3840.0, 2160.0),
        ] {
            for ppp in [1.0, 1.25, 1.5, 2.0] {
                let ctx = egui::Context::default();
                ctx.set_pixels_per_point(ppp);
                for sec in [Section::Start, Section::Recent, Section::Presets] {
                    app.home.section = sec;
                    run_frame(&mut app, &ctx, Vec2::new(w, h), vec![]);
                }
            }
        }
    }

    #[test]
    fn preset_creates_a_real_document_and_closing_it_returns_home() {
        let mut app = app_with(crate::prefs::Prefs::default());
        let mut view = None;
        app.run_action(
            Action::NewSized {
                width: 1080,
                height: 1350,
                ppi: 72.0,
                transparent: false,
            },
            &mut view,
        );
        assert_eq!(app.docs.len(), 1);
        let d = &app.docs[0].doc;
        assert_eq!((d.width, d.height), (1080, 1350));
        assert!(!d.modified, "documento novo não está modificado");
        // Fechar o último documento volta à tela inicial (a que aparece quando `docs` está vazio).
        app.close_now(0);
        assert!(app.docs.is_empty());
        // "Novo documento" abre o diálogo completo; nada é criado até confirmar.
        app.run_action(Action::New, &mut view);
        assert!(app.new_doc.is_some() && app.docs.is_empty());
    }
}
