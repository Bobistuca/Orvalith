//! Interface dos ajustes de cor: editor de parâmetros (compartilhado pela janela destrutiva e
//! pelo painel da camada de ajuste), sessão destrutiva com prévia, histograma e perfil de cor.

use std::sync::Arc;

use eframe::egui;
use orvalith_core::adjops;
use orvalith_core::adjust::{eval_curve, Adjustment, GradStop, BW_NAMES, KINDS, SELECTIVE_RANGES};
use orvalith_core::color::{self, IccProfile};
use orvalith_core::geom::Rect as PixRect;
use orvalith_core::histogram::{self, Channel, Histogram};
use orvalith_core::{compose, LayerId};

use crate::canvas::OpenDoc;

/// Estado de seleção dos controles do editor (qual canal, faixa, parada…).
#[derive(Default)]
pub struct AdjUiState {
    chan: usize,
    tone: usize,
    range: usize,
    stop: usize,
    curve_drag: Option<usize>,
}

fn slider(
    ui: &mut egui::Ui,
    label: &str,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.add(egui::Slider::new(v, range).text(label)).changed()
}

const CHANNELS: [&str; 4] = ["Principal", "Vermelho", "Verde", "Azul"];

fn curve_editor(ui: &mut egui::Ui, pts: &mut Vec<[f32; 2]>, drag: &mut Option<usize>) -> bool {
    let size = egui::vec2(220.0, 220.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let faint = ui.visuals().widgets.noninteractive.bg_stroke.color;
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    for i in 1..4 {
        let t = i as f32 / 4.0;
        painter.line_segment(
            [
                egui::pos2(rect.left() + t * size.x, rect.top()),
                egui::pos2(rect.left() + t * size.x, rect.bottom()),
            ],
            egui::Stroke::new(1.0, faint),
        );
        painter.line_segment(
            [
                egui::pos2(rect.left(), rect.top() + t * size.y),
                egui::pos2(rect.right(), rect.top() + t * size.y),
            ],
            egui::Stroke::new(1.0, faint),
        );
    }
    let to_screen =
        |p: [f32; 2]| egui::pos2(rect.left() + p[0] * size.x, rect.bottom() - p[1] * size.y);
    let line: Vec<egui::Pos2> = (0..=64)
        .map(|i| {
            let x = i as f32 / 64.0;
            to_screen([x, eval_curve(pts, x)])
        })
        .collect();
    painter.add(egui::Shape::line(
        line,
        egui::Stroke::new(1.5, ui.visuals().text_color()),
    ));
    for p in pts.iter() {
        painter.circle_filled(to_screen(*p), 4.0, ui.visuals().selection.bg_fill);
    }
    let from_screen = |pos: egui::Pos2| {
        [
            ((pos.x - rect.left()) / size.x).clamp(0.0, 1.0),
            ((rect.bottom() - pos.y) / size.y).clamp(0.0, 1.0),
        ]
    };
    let nearest = |pos: egui::Pos2, pts: &[[f32; 2]]| {
        pts.iter()
            .enumerate()
            .map(|(i, p)| (i, to_screen(*p).distance(pos)))
            .filter(|(_, d)| *d <= 10.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let mut changed = false;
    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            *drag = nearest(pos, pts).or_else(|| {
                let p = from_screen(pos);
                let at = pts.iter().position(|q| q[0] > p[0]).unwrap_or(pts.len());
                if pts.len() < 16 {
                    pts.insert(at, p);
                    changed = true;
                    Some(at)
                } else {
                    None
                }
            });
        }
    }
    if let (true, Some(i), Some(pos)) = (resp.dragged(), *drag, resp.interact_pointer_pos()) {
        if i < pts.len() {
            let mut p = from_screen(pos);
            let last = pts.len() - 1;
            if i == 0 || i == last {
                p[0] = pts[i][0]; // as pontas só se movem na vertical
            } else {
                p[0] = p[0].clamp(pts[i - 1][0] + 0.01, pts[i + 1][0] - 0.01);
            }
            if pts[i] != p {
                pts[i] = p;
                changed = true;
            }
        }
    }
    if resp.drag_stopped() {
        *drag = None;
    }
    if resp.secondary_clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if let Some(i) = nearest(pos, pts) {
                if i != 0 && i != pts.len() - 1 {
                    pts.remove(i);
                    changed = true;
                }
            }
        }
    }
    ui.small("Arraste para criar/mover pontos; clique direito remove.");
    changed
}

fn tone_picker(ui: &mut egui::Ui, sel: &mut usize, names: &[&str]) {
    ui.horizontal_wrapped(|ui| {
        for (i, n) in names.iter().enumerate() {
            ui.selectable_value(sel, i, *n);
        }
    });
}

/// Editor dos parâmetros de um ajuste. Retorna `true` quando algo mudou.
pub fn adjust_ui(ui: &mut egui::Ui, adj: &mut Adjustment, st: &mut AdjUiState) -> bool {
    let mut ch = false;
    match adj {
        Adjustment::BrightnessContrast {
            brightness,
            contrast,
        } => {
            ch |= slider(ui, "Brilho", brightness, -100.0..=100.0);
            ch |= slider(ui, "Contraste", contrast, -100.0..=100.0);
        }
        Adjustment::Exposure {
            exposure,
            offset,
            gamma,
        } => {
            ch |= slider(ui, "Exposição (stops)", exposure, -5.0..=5.0);
            ch |= slider(ui, "Deslocamento", offset, -0.5..=0.5);
            ch |= slider(ui, "Gama", gamma, 0.1..=3.0);
            ui.small("Calculado em luz linear (sRGB decodificado).");
        }
        Adjustment::Levels { master, r, g, b } => {
            tone_picker(ui, &mut st.chan, &CHANNELS);
            let c = match st.chan {
                1 => r,
                2 => g,
                3 => b,
                _ => master,
            };
            ch |= slider(ui, "Entrada: preto", &mut c.in_black, 0.0..=254.0);
            ch |= slider(ui, "Gama", &mut c.gamma, 0.1..=9.99);
            ch |= slider(ui, "Entrada: branco", &mut c.in_white, 1.0..=255.0);
            ch |= slider(ui, "Saída: preto", &mut c.out_black, 0.0..=255.0);
            ch |= slider(ui, "Saída: branco", &mut c.out_white, 0.0..=255.0);
        }
        Adjustment::Curves { master, r, g, b } => {
            tone_picker(ui, &mut st.chan, &CHANNELS);
            let pts = match st.chan {
                1 => r,
                2 => g,
                3 => b,
                _ => master,
            };
            ch |= curve_editor(ui, pts, &mut st.curve_drag);
            if ui.small_button("Redefinir curva").clicked() {
                *pts = orvalith_core::adjust::identity_curve();
                ch = true;
            }
        }
        Adjustment::HueSaturation {
            hue,
            saturation,
            lightness,
        } => {
            ch |= slider(ui, "Matiz", hue, -180.0..=180.0);
            ch |= slider(ui, "Saturação", saturation, -100.0..=100.0);
            ch |= slider(ui, "Luminosidade", lightness, -100.0..=100.0);
        }
        Adjustment::Vibrance {
            vibrance,
            saturation,
        } => {
            ch |= slider(ui, "Vibração", vibrance, -100.0..=100.0);
            ch |= slider(ui, "Saturação", saturation, -100.0..=100.0);
        }
        Adjustment::ColorBalance {
            shadows,
            midtones,
            highlights,
            preserve_luminosity,
        } => {
            tone_picker(ui, &mut st.tone, &["Sombras", "Meios-tons", "Realces"]);
            let t = match st.tone {
                0 => shadows,
                2 => highlights,
                _ => midtones,
            };
            ch |= slider(ui, "Ciano ↔ Vermelho", &mut t[0], -100.0..=100.0);
            ch |= slider(ui, "Magenta ↔ Verde", &mut t[1], -100.0..=100.0);
            ch |= slider(ui, "Amarelo ↔ Azul", &mut t[2], -100.0..=100.0);
            ch |= ui
                .checkbox(preserve_luminosity, "Preservar luminosidade")
                .changed();
        }
        Adjustment::BlackWhite { weights, tint } => {
            for (i, n) in BW_NAMES.iter().enumerate() {
                ch |= slider(ui, n, &mut weights[i], -100.0..=200.0);
            }
            let mut on = tint.is_some();
            if ui.checkbox(&mut on, "Matiz de tonalização").changed() {
                *tint = on.then_some([225, 195, 150]);
                ch = true;
            }
            if let Some(t) = tint {
                ch |= ui.color_edit_button_srgb(t).changed();
            }
        }
        Adjustment::PhotoFilter {
            color,
            density,
            preserve_luminosity,
        } => {
            ui.horizontal(|ui| {
                ui.label("Cor");
                ch |= ui.color_edit_button_srgb(color).changed();
            });
            ch |= slider(ui, "Densidade", density, 0.0..=100.0);
            ch |= ui
                .checkbox(preserve_luminosity, "Preservar luminosidade")
                .changed();
        }
        Adjustment::ChannelMixer {
            matrix,
            constant,
            monochrome,
        } => {
            tone_picker(
                ui,
                &mut st.chan,
                &["Saída vermelho", "Saída verde", "Saída azul"],
            );
            let o = st.chan.min(2);
            for (i, n) in ["Vermelho", "Verde", "Azul"].iter().enumerate() {
                ch |= slider(ui, n, &mut matrix[o][i], -200.0..=200.0);
            }
            ch |= slider(ui, "Constante", &mut constant[o], -100.0..=100.0);
            ch |= ui
                .checkbox(monochrome, "Monocromático (usa a saída vermelho)")
                .changed();
        }
        Adjustment::SelectiveColor { ranges, relative } => {
            egui::ComboBox::from_id_salt("sel_range")
                .selected_text(SELECTIVE_RANGES[st.range.min(8)])
                .show_ui(ui, |ui| {
                    for (i, n) in SELECTIVE_RANGES.iter().enumerate() {
                        ui.selectable_value(&mut st.range, i, *n);
                    }
                });
            let r = &mut ranges[st.range.min(8)];
            for (i, n) in ["Ciano", "Magenta", "Amarelo", "Preto"].iter().enumerate() {
                ch |= slider(ui, n, &mut r[i], -100.0..=100.0);
            }
            ch |= ui.checkbox(relative, "Relativo").changed();
            ui.small("Modelo clássico aproximado; difere de outros editores.");
        }
        Adjustment::GradientMap { stops, reverse } => {
            ch |= gradient_editor(ui, stops, st);
            ch |= ui.checkbox(reverse, "Inverter").changed();
        }
        Adjustment::Invert => {
            ui.label("Inverte as cores (sem parâmetros).");
        }
        Adjustment::Posterize { levels } => {
            let mut v = *levels as f32;
            if slider(ui, "Níveis", &mut v, 2.0..=64.0) {
                *levels = v.round() as u32;
                ch = true;
            }
        }
        Adjustment::Threshold { level } => {
            ch |= slider(ui, "Limiar", level, 0.0..=255.0);
        }
        Adjustment::Grayscale => {
            ui.label("Converte para tons de cinza pela luminância (Rec. 601).");
            ui.small("O documento continua RGB de 8 bits; não há modo de cinza real.");
        }
    }
    ch
}

pub fn gradient_editor(ui: &mut egui::Ui, stops: &mut Vec<GradStop>, st: &mut AdjUiState) -> bool {
    let mut ch = false;
    st.stop = st.stop.min(stops.len().saturating_sub(1));
    // Faixa de pré-visualização.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(220.0, 16.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let n = 44;
    for i in 0..n {
        let t = i as f32 / (n - 1) as f32;
        let c = orvalith_core::adjust::Adjustment::GradientMap {
            stops: stops.clone(),
            reverse: false,
        }
        .prepare()
        .apply([(t * 255.0) as u8; 3]);
        let x0 = rect.left() + rect.width() * i as f32 / n as f32;
        let x1 = rect.left() + rect.width() * (i + 1) as f32 / n as f32;
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom())),
            0.0,
            egui::Color32::from_rgb(c[0], c[1], c[2]),
        );
    }
    let mut remove = None;
    for (i, s) in stops.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ch |= ui.color_edit_button_srgb(&mut s.color).changed();
            ch |= ui
                .add(egui::Slider::new(&mut s.pos, 0.0..=1.0).fixed_decimals(2))
                .changed();
            if ui
                .small_button("✕")
                .on_hover_text("Remover parada")
                .clicked()
            {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        if stops.len() > 2 {
            stops.remove(i);
            ch = true;
        }
    }
    ui.horizontal(|ui| {
        if ui.small_button("+ Parada").clicked() && stops.len() < 32 {
            stops.push(GradStop {
                pos: 0.5,
                color: [128, 128, 128],
            });
            ch = true;
        }
        for (name, a, b) in [
            ("Sépia", [40, 20, 10], [255, 235, 200]),
            ("Azul→Laranja", [10, 30, 120], [255, 160, 40]),
        ] {
            if ui.small_button(name).clicked() {
                *stops = vec![
                    GradStop { pos: 0.0, color: a },
                    GradStop { pos: 1.0, color: b },
                ];
                ch = true;
            }
        }
    });
    if ch {
        stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    }
    ch
}

// ---------------------------------------------------------------------------------------
// Sessão destrutiva: janela de ajuste com prévia ao vivo na camada ativa.

pub struct AdjSession {
    pub layer: LayerId,
    original: Arc<Vec<u8>>,
    pub adj: Adjustment,
    /// Revisão do documento logo após a última prévia; mudança externa cancela a sessão.
    rev: u64,
    was_modified: bool,
    pub ui: AdjUiState,
}

impl AdjSession {
    /// Inicia a sessão na camada ativa; `Err` se a camada não aceitar ajuste destrutivo.
    pub fn begin(open: &OpenDoc, kind: usize) -> Result<Self, String> {
        let idx = open
            .active_idx()
            .ok_or("Abra ou selecione uma camada primeiro.")?;
        let l = &open.doc.layers[idx];
        if let Some(msg) = adjops::destructive_blocked(l) {
            return Err(msg.into());
        }
        Ok(Self {
            layer: l.id,
            original: l.pixels.clone(),
            adj: Adjustment::neutral(kind),
            rev: open.doc.revision,
            was_modified: open.doc.modified,
            ui: AdjUiState::default(),
        })
    }

    fn preview(&mut self, open: &mut OpenDoc) {
        let sel = open.doc.selection.mask.clone();
        let px = adjops::adjusted_pixels(
            &self.original,
            &self.adj,
            sel.as_ref().map(|m| m.as_slice()),
        );
        let (w, h) = (open.doc.width, open.doc.height);
        if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == self.layer) {
            l.pixels = Arc::new(px);
        }
        open.mark_dirty(PixRect::full(w, h));
        self.rev = open.doc.revision;
    }

    fn restore(&mut self, open: &mut OpenDoc) {
        let (w, h) = (open.doc.width, open.doc.height);
        if let Some(l) = open.doc.layers.iter_mut().find(|l| l.id == self.layer) {
            l.pixels = self.original.clone();
        }
        open.mark_dirty(PixRect::full(w, h));
        open.doc.modified = self.was_modified;
        self.rev = open.doc.revision;
    }

    fn commit(&mut self, open: &mut OpenDoc) {
        let (w, h) = (open.doc.width, open.doc.height);
        let mut before = open.doc.layers.clone();
        if let Some(l) = before.iter_mut().find(|l| l.id == self.layer) {
            l.pixels = self.original.clone();
        }
        open.record_layers(
            &format!("Ajuste: {}", self.adj.name()),
            before,
            PixRect::full(w, h),
        );
    }
}

/// Cancela a sessão restaurando os pixels originais.
pub fn cancel_session(s: &mut AdjSession, open: &mut OpenDoc) {
    if open.doc.revision == s.rev {
        s.restore(open);
    }
}

/// Desenha a janela da sessão destrutiva. Retorna `true` quando a sessão terminou.
pub fn session_window(ctx: &egui::Context, s: &mut AdjSession, open: &mut OpenDoc) -> bool {
    if open.doc.revision != s.rev {
        return true; // o documento mudou por outro caminho (desfazer etc.): encerra sem mexer
    }
    let (mut ok, mut cancel, mut changed, mut reset) = (false, false, false, false);
    let kind = s.adj.kind_index();
    egui::Window::new(format!("Ajuste: {}", KINDS[kind]))
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            changed = adjust_ui(ui, &mut s.adj, &mut s.ui);
            if !open.doc.selection.is_empty() {
                ui.small("O ajuste vale somente dentro da seleção.");
            }
            ui.horizontal(|ui| {
                ok = ui.button("Aplicar").clicked();
                reset = ui.button("Redefinir").clicked();
                cancel = ui.button("Cancelar").clicked();
            });
        });
    if reset {
        s.adj = Adjustment::neutral(kind);
        changed = true;
    }
    if changed {
        s.adj.sanitize();
        s.preview(open);
    }
    if cancel {
        s.restore(open);
        return true;
    }
    if ok {
        s.preview(open);
        s.commit(open);
        return true;
    }
    false
}

// ---------------------------------------------------------------------------------------
// Histograma

type HistKey = (u64, u64, LayerId, bool, bool);

#[derive(Default)]
pub struct HistState {
    pub open: bool,
    pub active_layer_only: bool,
    pub selection_only: bool,
    chan: usize,
    cache: Option<(HistKey, Histogram)>,
}

const HIST_CHANNELS: [(&str, Option<Channel>); 5] = [
    ("Luminosidade", Some(Channel::Luma)),
    ("Vermelho", Some(Channel::Red)),
    ("Verde", Some(Channel::Green)),
    ("Azul", Some(Channel::Blue)),
    ("RGB", None),
];

pub fn histogram_window(ctx: &egui::Context, h: &mut HistState, open: &OpenDoc) {
    if !h.open {
        return;
    }
    let mut is_open = true;
    egui::Window::new("Histograma")
        .open(&mut is_open)
        .resizable(false)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (i, (n, _)) in HIST_CHANNELS.iter().enumerate() {
                    ui.selectable_value(&mut h.chan, i, *n);
                }
            });
            ui.checkbox(&mut h.active_layer_only, "Somente a camada ativa");
            ui.checkbox(&mut h.selection_only, "Somente a seleção");
            let key = (
                open.doc.id.0,
                open.doc.revision,
                open.active_layer,
                h.active_layer_only,
                h.selection_only,
            );
            if h.cache.as_ref().map(|c| c.0) != Some(key) {
                let rgba: Option<Vec<u8>> = if h.active_layer_only {
                    open.active_idx()
                        .map(|i| &open.doc.layers[i])
                        .filter(|l| !l.is_group() && l.adjustment.is_none())
                        .map(|l| l.pixels.as_ref().clone())
                } else {
                    Some(compose::flatten(&open.doc))
                };
                let mask = if h.selection_only {
                    open.doc.selection.mask.clone()
                } else {
                    None
                };
                let hist = histogram::compute(
                    &rgba.unwrap_or_default(),
                    mask.as_ref().map(|m| m.as_slice()),
                );
                h.cache = Some((key, hist));
            }
            let Some((_, hist)) = &h.cache else { return };
            draw_histogram(ui, hist, HIST_CHANNELS[h.chan.min(4)].1);
            match HIST_CHANNELS[h.chan.min(4)].1 {
                Some(c) => match hist.stats(c) {
                    Some(s) => {
                        ui.small(format!(
                            "Média {:.1} · Mediana {} · Desvio {:.1} · Mín {} · Máx {} · {} px",
                            s.mean, s.median, s.std_dev, s.min, s.max, hist.pixels
                        ));
                    }
                    None => {
                        ui.small("Sem pixels visíveis.");
                    }
                },
                None => {
                    ui.small(format!("{} px", hist.pixels));
                }
            }
        });
    h.open = is_open;
}

fn draw_histogram(ui: &mut egui::Ui, hist: &Histogram, ch: Option<Channel>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(256.0, 100.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let series: Vec<(&[u32; 256], egui::Color32)> = match ch {
        Some(c) => {
            let col = match c {
                Channel::Luma => ui.visuals().text_color(),
                Channel::Red => egui::Color32::from_rgb(230, 70, 70),
                Channel::Green => egui::Color32::from_rgb(70, 200, 90),
                Channel::Blue => egui::Color32::from_rgb(90, 130, 240),
            };
            vec![(hist.bins(c), col)]
        }
        None => vec![
            (
                &hist.r,
                egui::Color32::from_rgba_unmultiplied(230, 70, 70, 160),
            ),
            (
                &hist.g,
                egui::Color32::from_rgba_unmultiplied(70, 200, 90, 160),
            ),
            (
                &hist.b,
                egui::Color32::from_rgba_unmultiplied(90, 130, 240, 160),
            ),
        ],
    };
    // Escala pelo maior bin (ignora os extremos 0/255 que costumam dominar).
    let max = series
        .iter()
        .flat_map(|(b, _)| b[1..255].iter().copied())
        .max()
        .unwrap_or(0)
        .max(1) as f32;
    for (bins, col) in series {
        for (i, &n) in bins.iter().enumerate() {
            let hgt = (n as f32 / max).min(1.0) * rect.height();
            if hgt > 0.0 {
                let x = rect.left() + i as f32;
                painter.line_segment(
                    [
                        egui::pos2(x, rect.bottom()),
                        egui::pos2(x, rect.bottom() - hgt),
                    ],
                    egui::Stroke::new(1.0, col),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Perfil de cor

pub struct ColorDialog {
    pub open: bool,
    profiles: Vec<IccProfile>,
    pick: usize,
}

impl Default for ColorDialog {
    fn default() -> Self {
        Self {
            open: false,
            profiles: color::builtin_profiles(),
            pick: 0,
        }
    }
}

/// Resultado de uma ação no diálogo de perfil.
pub enum ColorAction {
    Assign(Option<IccProfile>),
    Convert(Option<IccProfile>),
    ToggleProof,
    OpenFile,
}

pub fn color_window(
    ctx: &egui::Context,
    d: &mut ColorDialog,
    open: &OpenDoc,
) -> Option<ColorAction> {
    if !d.open {
        return None;
    }
    let mut action = None;
    let mut is_open = true;
    egui::Window::new("Perfil de cor")
        .open(&mut is_open)
        .resizable(false)
        .show(ctx, |ui| {
            match &open.doc.icc {
                Some(p) => {
                    ui.label(format!("Perfil atribuído: {}", p.name));
                    if let Err(e) = color::inspect(&p.data) {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            format!("Este perfil não pôde ser lido ({e}); os bytes foram preservados, mas conversões e a prévia ficam indisponíveis."),
                        );
                    }
                }
                None => {
                    ui.label("Sem perfil: os pixels são tratados como sRGB.");
                }
            }
            let mut proof = open.proof;
            if ui
                .checkbox(&mut proof, "Prévia na tela convertida para sRGB")
                .on_hover_text("Só afeta a visualização; não altera os pixels.")
                .changed()
            {
                action = Some(ColorAction::ToggleProof);
            }
            ui.separator();
            let name = d.profiles.get(d.pick).map_or("—", |p| p.name.as_str());
            egui::ComboBox::from_label("Perfil de destino")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    for (i, p) in d.profiles.iter().enumerate() {
                        ui.selectable_value(&mut d.pick, i, &p.name);
                    }
                });
            if ui.button("Carregar perfil de um arquivo .icc…").clicked() {
                action = Some(ColorAction::OpenFile);
            }
            let target = d.profiles.get(d.pick).cloned();
            ui.horizontal(|ui| {
                if ui
                    .button("Atribuir")
                    .on_hover_text("Só troca a etiqueta: os pixels NÃO mudam, a aparência muda.")
                    .clicked()
                {
                    action = Some(ColorAction::Assign(target.clone()));
                }
                if ui
                    .button("Converter")
                    .on_hover_text("Recalcula os pixels para manter a aparência no novo perfil.")
                    .clicked()
                {
                    action = Some(ColorAction::Convert(target.clone()));
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Remover perfil (tratar como sRGB)").clicked() {
                    action = Some(ColorAction::Assign(None));
                }
                if ui.button("Converter para sRGB").clicked() {
                    action = Some(ColorAction::Convert(None));
                }
            });
            ui.small("Suporte: perfis RGB de 8 bits. Sem CMYK, Lab, cinza, 16 bits ou prova de cores.");
        });
    d.open = is_open;
    action
}

impl ColorDialog {
    /// Adiciona um perfil carregado de arquivo e o seleciona.
    pub fn add_profile(&mut self, p: IccProfile) {
        self.profiles.push(p);
        self.pick = self.profiles.len() - 1;
    }
}

/// Aplica uma ação de perfil ao documento (uma ação de histórico). Retorna uma mensagem.
pub fn run_color_action(open: &mut OpenDoc, action: ColorAction) -> Option<String> {
    use orvalith_core::color::ColorEdit;
    match action {
        ColorAction::ToggleProof => {
            open.proof = !open.proof;
            None
        }
        ColorAction::OpenFile => None,
        ColorAction::Assign(p) => {
            if p == open.doc.icc {
                return Some("O documento já tem esse perfil.".into());
            }
            let layers = open.doc.layers.clone();
            let cmd = ColorEdit::new(
                "Atribuir perfil",
                layers.clone(),
                layers,
                open.doc.icc.clone(),
                p,
            );
            open.history.execute(&mut open.doc, Box::new(cmd));
            Some("Perfil atribuído: os valores dos pixels não mudaram.".into())
        }
        ColorAction::Convert(p) => {
            if p == open.doc.icc
                || (color::is_srgb(p.as_ref()) && color::is_srgb(open.doc.icc.as_ref()))
            {
                return Some("O documento já está nesse perfil.".into());
            }
            match color::convert_document(&open.doc, p.as_ref()) {
                Ok(c) => {
                    let before = open.doc.layers.clone();
                    let cmd = ColorEdit::new(
                        "Converter perfil",
                        before,
                        c.layers,
                        open.doc.icc.clone(),
                        p,
                    );
                    open.history.execute(&mut open.doc, Box::new(cmd));
                    Some(if c.rasterized > 0 {
                        format!("Convertido. {} camada(s) de texto/forma foram rasterizadas. Camadas de ajuste e máscaras não foram convertidas.", c.rasterized)
                    } else {
                        "Convertido. Camadas de ajuste e máscaras não foram convertidas.".into()
                    })
                }
                Err(e) => Some(e),
            }
        }
    }
}

/// Cria uma camada de ajuste do tipo `kind` acima da camada ativa.
pub fn new_adjustment_layer(open: &mut OpenDoc, kind: usize) {
    let above = open.active_idx().map(|i| open.doc.layers[i].id);
    let o = adjops::new_adjustment_layer(&open.doc, Adjustment::neutral(kind), above);
    let before = open.doc.layers.clone();
    open.history.execute(
        &mut open.doc,
        Box::new(orvalith_core::edit::LayersEdit::new(
            &format!("Camada de ajuste: {}", KINDS[kind.min(15)]),
            before,
            o.layers,
        )),
    );
    open.selected = o.select;
    if let Some(a) = o.active {
        open.active_layer = a;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvalith_core::Document;

    fn open() -> OpenDoc {
        let mut d = Document::new("t", 4, 4, Some([200, 100, 50, 255])).unwrap();
        let top = orvalith_core::Layer::filled("L2", 4, 4, [10, 20, 30, 255]);
        d.layers.push(top);
        OpenDoc::new(d)
    }

    #[test]
    fn destructive_session_previews_commits_and_undoes_as_one_action() {
        let mut o = open();
        let n0 = o.history.labels().len();
        let orig = o.doc.layers[1].pixels.clone();
        let mut s = AdjSession::begin(&o, 12).unwrap(); // Inverter
        s.preview(&mut o);
        assert_eq!(o.doc.layers[1].pixel(0, 0), Some([245, 235, 225, 255]));
        s.commit(&mut o);
        assert_eq!(o.history.labels().len(), n0 + 1);
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers[1].pixels, orig);
        o.history.redo(&mut o.doc);
        assert_eq!(o.doc.layers[1].pixel(0, 0), Some([245, 235, 225, 255]));
    }

    #[test]
    fn cancel_restores_pixels_and_modified_flag() {
        let mut o = open();
        let orig = o.doc.layers[1].pixels.clone();
        let was = o.doc.modified;
        let mut s = AdjSession::begin(&o, 12).unwrap();
        s.preview(&mut o);
        assert_ne!(o.doc.layers[1].pixels, orig);
        s.restore(&mut o);
        assert_eq!(o.doc.layers[1].pixels, orig);
        assert_eq!(o.doc.modified, was);
        assert_eq!(o.history.labels().len(), 0);
    }

    #[test]
    fn session_refuses_groups_and_adjustment_layers() {
        let mut o = open();
        new_adjustment_layer(&mut o, 0);
        assert!(AdjSession::begin(&o, 0).is_err());
    }

    #[test]
    fn adjustment_layer_creation_is_one_undoable_action() {
        let mut o = open();
        let n = o.doc.layers.len();
        new_adjustment_layer(&mut o, 3);
        assert_eq!(o.doc.layers.len(), n + 1);
        assert!(o.doc.layers.last().unwrap().adjustment.is_some());
        o.history.undo(&mut o.doc);
        assert_eq!(o.doc.layers.len(), n);
    }

    #[test]
    fn color_actions_assign_convert_and_undo() {
        let mut o = open();
        let before = o.doc.layers[0].pixels.clone();
        let p3 = color::builtin_profiles().remove(2);
        // Atribuir não muda os pixels.
        run_color_action(&mut o, ColorAction::Assign(Some(p3.clone())));
        assert_eq!(
            o.doc.icc.as_ref().map(|p| p.name.clone()),
            Some(p3.name.clone())
        );
        assert_eq!(o.doc.layers[0].pixels, before);
        // Converter de volta para sRGB muda os pixels.
        let msg = run_color_action(&mut o, ColorAction::Convert(None));
        assert!(msg.is_some());
        assert!(o.doc.icc.is_none());
        assert_ne!(o.doc.layers[0].pixels, before);
        o.history.undo(&mut o.doc);
        assert!(o.doc.icc.is_some());
        assert_eq!(o.doc.layers[0].pixels, before);
        // Repetir o mesmo perfil avisa e não cria ação.
        let n = o.history.labels().len();
        assert!(run_color_action(&mut o, ColorAction::Assign(Some(p3))).is_some());
        assert_eq!(o.history.labels().len(), n);
    }
}
