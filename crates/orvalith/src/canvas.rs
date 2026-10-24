//! Documento aberto e canvas: xadrez de transparência, zoom, pan, entrada de ferramentas.

use eframe::egui::{self, Color32, CursorIcon, PointerButton, Rect, Sense, Stroke};
use std::collections::HashMap;

use crate::adjustui::AdjUiState;
use crate::objectui::ObjUiState;
use crate::styleui::StyleUiState;
use orvalith_core::color::Preview;
use orvalith_core::compose::RenderOpts;
use orvalith_core::docops::{DocEdit, DocState};
use orvalith_core::edit::{LayersEdit, PixelPatch};
use orvalith_core::geom::Rect as PixRect;
use orvalith_core::snap::{snap_point, SnapOpts};
use orvalith_core::view::View;
use orvalith_core::Guide;
use orvalith_core::{compose, Document, History, Layer, LayerId};

/// Maior lado suportado pela textura de visualização nesta fase (sem tiles ainda).
const MAX_TEXTURE_SIDE: u32 = 8192;
const FIT_MARGIN: f32 = 16.0;
const STEP: f32 = 1.25;

fn texture_options() -> egui::TextureOptions {
    egui::TextureOptions {
        magnification: egui::TextureFilter::Nearest,
        minification: egui::TextureFilter::Linear,
        ..Default::default()
    }
}

/// Documento aberto com histórico e estado de visualização.
pub struct OpenDoc {
    pub doc: Document,
    pub history: History,
    /// Caminho do projeto `.orva` salvo (None = nunca salvo).
    pub path: Option<std::path::PathBuf>,
    /// Arrasto de opacidade/preenchimento em andamento: (rótulo, camadas antes).
    pub prop_drag: Option<(String, Vec<Layer>)>,
    /// Camadas selecionadas no painel (múltipla seleção).
    pub selected: Vec<LayerId>,
    /// Isolamento temporário de visibilidade (não entra no histórico nem no arquivo).
    pub isolated: Option<LayerId>,
    pub thumbs: HashMap<LayerId, egui::TextureHandle>,
    pub thumbs_rev: u64,
    /// Renomeação em andamento no painel.
    pub renaming: Option<(LayerId, String)>,
    /// A pintura vai para a máscara da camada ativa (em vez dos pixels).
    pub edit_mask: bool,
    /// Mostrar a máscara desta camada em cinza (visão isolada).
    pub mask_view: Option<LayerId>,
    /// Mostrar este canal salvo em cinza.
    pub channel_view: Option<u64>,
    /// Canais de cor visíveis (R, G, B): só afeta a visualização.
    pub view_channels: [bool; 3],
    /// Canal salvo sendo editado pela máscara rápida (None = edita a seleção).
    pub qm_target: Option<u64>,
    pub mask_thumbs: HashMap<LayerId, egui::TextureHandle>,
    pub renaming_channel: Option<(u64, String)>,
    /// Filtro de busca da lista de fontes (painel da camada).
    pub font_filter: String,
    /// Estado dos controles do editor de ajustes (painel da camada de ajuste).
    pub adj_ui: AdjUiState,
    pub style_ui: StyleUiState,
    pub obj_ui: ObjUiState,
    /// Prévia na tela convertida do perfil do documento para sRGB.
    pub proof: bool,
    /// Instante da linha do tempo exibido no canvas (prévia da animação) e a pele de cebola.
    pub playhead: u32,
    pub anim_frame: Option<u32>,
    pub anim_onion: Option<(u32, u32, u32)>,
    anim_key: (Option<u32>, Option<(u32, u32, u32)>),
    /// Estado da linha do tempo antes de uma edição contínua (arrastar valores).
    pub tl_drag: Option<Option<orvalith_core::anim::Timeline>>,
    pub vid_drag: Option<Option<orvalith_core::video::VideoProject>>,
    preview: Option<std::sync::Arc<Preview>>,
    preview_key: (Option<usize>, bool),
    /// Máscara rápida: um documento cinza (255 = selecionado) editado com as ferramentas de pintura.
    pub qm: Option<Box<OpenDoc>>,
    sel_edges: Option<(usize, Vec<[f32; 4]>)>,
    guide_drag: Option<GuideDrag>,
    /// Retângulo envolvente da seleção atual (para a barra de status).
    pub sel_info: Option<PixRect>,
    /// Última revisão gravada na cópia de recuperação.
    pub autosaved_rev: Option<u64>,
    /// Camada que recebe pintura.
    pub active_layer: LayerId,
    texture: Option<(u64, egui::TextureHandle)>,
    /// Região alterada desde a última sincronização da textura (atualização parcial).
    dirty: Option<PixRect>,
    /// Revisão atingida só por alterações registradas em `dirty`.
    tracked_rev: u64,
    view: Option<View>,
    /// Enquanto verdadeiro, o documento é reajustado à janela quando ela muda de tamanho.
    fit_mode: bool,
    /// Zoom exibido em porcentagem de pixels físicos (para a barra de status).
    pub zoom_percent: f32,
}

impl OpenDoc {
    pub fn new(doc: Document) -> Self {
        let active_layer = doc.layers.last().map(|l| l.id).unwrap_or(LayerId(0));
        Self {
            doc,
            history: History::new(),
            path: None,
            prop_drag: None,
            selected: vec![active_layer],
            isolated: None,
            thumbs: HashMap::new(),
            thumbs_rev: 0,
            renaming: None,
            edit_mask: false,
            mask_view: None,
            channel_view: None,
            view_channels: [true; 3],
            qm_target: None,
            mask_thumbs: HashMap::new(),
            renaming_channel: None,
            font_filter: String::new(),
            adj_ui: AdjUiState::default(),
            style_ui: StyleUiState::default(),
            obj_ui: ObjUiState::default(),
            proof: true,
            playhead: 0,
            anim_frame: None,
            anim_onion: None,
            anim_key: (None, None),
            tl_drag: None,
            vid_drag: None,
            preview: None,
            preview_key: (None, true),
            qm: None,
            sel_edges: None,
            guide_drag: None,
            sel_info: None,
            autosaved_rev: None,
            active_layer,
            texture: None,
            dirty: None,
            tracked_rev: 0,
            view: None,
            fit_mode: true,
            zoom_percent: 100.0,
        }
    }

    /// Reconstrói a conversão de prévia quando o perfil (ou a opção) muda e refaz a textura.
    pub fn ensure_preview(&mut self) {
        let akey = (self.anim_frame, self.anim_onion);
        if akey != self.anim_key {
            self.anim_key = akey;
            self.texture = None;
            self.dirty = None;
        }
        let key = (
            self.doc
                .icc
                .as_ref()
                .map(|p| std::sync::Arc::as_ptr(&p.data) as usize),
            self.proof,
        );
        if key == self.preview_key {
            return;
        }
        self.preview_key = key;
        self.preview = if self.proof {
            Preview::new(self.doc.icc.as_ref())
                .ok()
                .flatten()
                .map(std::sync::Arc::new)
        } else {
            None
        };
        self.texture = None;
        self.dirty = None;
    }

    /// Índice da camada ativa (cai para a mais alta se o ID não existir mais).
    pub fn active_idx(&self) -> Option<usize> {
        self.doc
            .layers
            .iter()
            .position(|l| l.id == self.active_layer)
            .or_else(|| self.doc.layers.len().checked_sub(1))
    }

    /// Marca uma região alterada diretamente nos pixels (pré-visualização de traço etc.):
    /// avança a revisão, mas a textura só precisa atualizar esta região.
    pub fn mark_dirty(&mut self, rect: PixRect) {
        self.doc.touch();
        self.dirty = Some(self.dirty.map_or(rect, |d| d.union(rect)));
        self.tracked_rev = self.doc.revision;
    }

    /// Executa um comando que não altera pixels (seleção, canais): não suja o documento
    /// nem força o reenvio da textura.
    pub fn execute_nonpixel(&mut self, cmd: Box<dyn orvalith_core::Command>) {
        let partial_ok = self.tracked_rev == self.doc.revision;
        let was_modified = self.doc.modified;
        self.history.execute(&mut self.doc, cmd);
        self.doc.modified = was_modified;
        if partial_ok {
            self.tracked_rev = self.doc.revision;
            self.dirty.get_or_insert(PixRect::new(0, 0, 0, 0));
        }
    }

    /// Entra na máscara rápida a partir da seleção atual (sem seleção = tudo selecionado).
    pub fn enter_quick_mask(&mut self) {
        let (w, h) = (self.doc.width, self.doc.height);
        let mut px = Vec::with_capacity(w as usize * h as usize * 4);
        for i in 0..w as usize * h as usize {
            let v = self.doc.selection.mask.as_ref().map_or(255, |m| m[i]);
            px.extend_from_slice(&[v, v, v, 255]);
        }
        if let Ok(mut d) = Document::new("Máscara rápida", w, h, None) {
            d.layers[0] = Layer::from_pixels("Máscara", w, h, px);
            self.qm = Some(Box::new(OpenDoc::new(d)));
        }
    }

    /// Entra na máscara rápida editando um canal salvo.
    pub fn enter_quick_mask_for_channel(&mut self, id: u64) {
        let Some(c) = self.doc.channels.iter().find(|c| c.id == id) else {
            return;
        };
        let (w, h) = (self.doc.width, self.doc.height);
        let mut px = Vec::with_capacity(w as usize * h as usize * 4);
        for &v in c.mask.iter() {
            px.extend_from_slice(&[v, v, v, 255]);
        }
        if let Ok(mut d) = Document::new("Canal", w, h, None) {
            d.layers[0] = Layer::from_pixels("Canal", w, h, px);
            self.qm = Some(Box::new(OpenDoc::new(d)));
            self.qm_target = Some(id);
        }
    }

    /// Sai da máscara rápida e devolve o mapa bruto de 1 byte por pixel.
    pub fn leave_quick_mask_raw(&mut self) -> Option<Vec<u8>> {
        let qm = self.qm.take()?;
        Some(
            qm.doc.layers[0]
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| p[0])
                .collect(),
        )
    }

    /// Registra um comando já aplicado, com atualização parcial da textura em `rect`.
    pub fn record_cmd(&mut self, cmd: Box<dyn orvalith_core::Command>, rect: PixRect) {
        let partial_ok = self.tracked_rev == self.doc.revision;
        self.history.record(&mut self.doc, cmd);
        if partial_ok {
            self.dirty = Some(self.dirty.map_or(rect, |d| d.union(rect)));
            self.tracked_rev = self.doc.revision;
        }
    }

    /// Sai da máscara rápida e devolve a máscara editada (None = tudo selecionado).
    pub fn leave_quick_mask(&mut self) -> Option<Option<Vec<u8>>> {
        let qm = self.qm.take()?;
        let mask: Vec<u8> = qm.doc.layers[0]
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0])
            .collect();
        Some(if mask.iter().all(|&v| v == 255) {
            None
        } else {
            orvalith_core::selection::nonempty(mask)
        })
    }

    /// Registra uma troca de camadas já aplicada ao documento (ex.: mover), exibida com
    /// atualização parcial da textura em `rect`.
    pub fn record_layers(&mut self, label: &str, before: Vec<Layer>, rect: PixRect) {
        let partial_ok = self.tracked_rev == self.doc.revision;
        let after = self.doc.layers.clone();
        self.history.record(
            &mut self.doc,
            Box::new(LayersEdit::new(label, before, after)),
        );
        if partial_ok {
            self.dirty = Some(self.dirty.map_or(rect, |d| d.union(rect)));
            self.tracked_rev = self.doc.revision;
        }
    }

    /// Aplica um novo estado geométrico (recortar, redimensionar…) como uma ação de histórico.
    pub fn apply_state(&mut self, label: &str, after: DocState) {
        let before = DocState::of(&self.doc);
        self.history
            .execute(&mut self.doc, Box::new(DocEdit::new(label, before, after)));
        self.fit_mode = true;
        self.sel_edges = None;
        self.thumbs_rev = 0;
    }

    /// Registra uma troca de estado já aplicada ao documento (transformação confirmada).
    pub fn record_state(&mut self, label: &str, before: DocState) {
        let partial_ok = self.tracked_rev == self.doc.revision;
        let after = DocState::of(&self.doc);
        self.history
            .record(&mut self.doc, Box::new(DocEdit::new(label, before, after)));
        if partial_ok {
            self.dirty = Some(PixRect::full(self.doc.width, self.doc.height));
            self.tracked_rev = self.doc.revision;
        }
        self.thumbs_rev = 0;
    }

    /// Registra no histórico uma alteração de pixels já exibida (sem reenviar a textura toda).
    pub fn record_patch(&mut self, patch: PixelPatch) {
        let partial_ok = self.tracked_rev == self.doc.revision;
        let rect = patch.rect();
        self.history.record(&mut self.doc, Box::new(patch));
        if partial_ok {
            self.dirty = Some(self.dirty.map_or(rect, |d| d.union(rect)));
            self.tracked_rev = self.doc.revision;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewCmd {
    Fit,
    Actual,
    ZoomIn,
    ZoomOut,
    /// Gira a visão (graus, horário); não altera a imagem.
    Rotate(f32),
    ResetRotation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RulerUnit {
    Px,
    In,
    Cm,
}

impl RulerUnit {
    pub fn label(self) -> &'static str {
        match self {
            RulerUnit::Px => "Pixels",
            RulerUnit::In => "Polegadas",
            RulerUnit::Cm => "Centímetros",
        }
    }

    /// Pixels do documento por unidade.
    fn px_per_unit(self, ppi: f32) -> f32 {
        match self {
            RulerUnit::Px => 1.0,
            RulerUnit::In => ppi.max(1.0),
            RulerUnit::Cm => ppi.max(1.0) / 2.54,
        }
    }
}

/// Opções de visualização compartilhadas (réguas, grade, guias, encaixe).
#[derive(Debug, Clone, Copy)]
pub struct ViewOpts {
    pub rulers: bool,
    pub grid: bool,
    pub guides: bool,
    pub snap: bool,
    pub snap_opts: SnapOpts,
    pub unit: RulerUnit,
}

impl Default for ViewOpts {
    fn default() -> Self {
        Self {
            rulers: true,
            grid: false,
            guides: true,
            snap: true,
            snap_opts: SnapOpts::default(),
            unit: RulerUnit::Px,
        }
    }
}

/// Arrasto de guia em andamento (nova, ou uma existente sendo movida/removida).
struct GuideDrag {
    index: Option<usize>,
    vertical: bool,
    pos: f32,
    remove: bool,
    before: DocState,
}

/// Forma em construção, em coordenadas do documento.
#[derive(Clone)]
pub enum Shape {
    Rect((f32, f32), (f32, f32)),
    Ellipse((f32, f32), (f32, f32)),
    Poly(Vec<(f32, f32)>),
    /// Malha `colunas × linhas` de pontos de controle (linha a linha).
    Mesh(usize, usize, Vec<(f32, f32)>),
}

/// Informações de desenho vindas da ferramenta atual.
#[derive(Default, Clone)]
pub struct Overlay {
    /// Forma de seleção/transformação em construção (contorno tracejado).
    pub shape: Option<Shape>,
    /// Alças desenhadas como quadradinhos (coordenadas do documento).
    pub points: Vec<(f32, f32)>,
    /// A ferramenta atual usa o cursor de mira.
    pub tool_cursor: bool,
    /// Raio do pincel em pixels do documento (para o contorno sob o cursor).
    pub brush_radius_doc: Option<f32>,
    /// Encaixar os pontos do ponteiro em guias/grade/bordas.
    pub snap_pointer: bool,
    pub view: ViewOpts,
}

/// Eventos de ponteiro do quadro, já em coordenadas do documento.
#[derive(Default)]
pub struct CanvasOut {
    pub hover: Option<[f32; 2]>,
    /// Botão primário pressionado neste quadro (início de traço).
    pub pressed: Option<[f32; 2]>,
    /// Amostras de movimento deste quadro (cada evento do sistema, não só a última posição).
    pub moves: Vec<[f32; 2]>,
    pub down: bool,
    pub released: bool,
    pub alt: bool,
    pub shift: bool,
    /// Duplo clique do botão primário neste quadro.
    pub double_click: bool,
    /// Pressão da caneta (0..=1) quando o sistema a informa; 1.0 com mouse.
    pub pressure: f32,
    /// Pontos de tela por pixel do documento (para tamanhos de alças).
    pub zoom: f32,
    /// Visão e origem do canvas na tela (para posicionar editores sobre o documento).
    pub view: Option<View>,
    pub origin: egui::Pos2,
}

const RULER: f32 = 20.0;

/// Desenha o canvas, trata navegação, guias e devolve os eventos de ponteiro para as ferramentas.
pub fn show(
    ui: &mut egui::Ui,
    open: &mut OpenDoc,
    cmd: Option<ViewCmd>,
    space_down: bool,
    overlay: &Overlay,
) -> CanvasOut {
    open.ensure_preview();
    let (full, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let rotated = open.view.is_some_and(|v| v.rotation_deg != 0.0);
    let ruler = if overlay.view.rulers && !rotated {
        RULER
    } else {
        0.0
    };
    let rect = Rect::from_min_max(full.min + egui::vec2(ruler, ruler), full.max);
    let resp = ui.interact(rect, ui.scope_id().with("canvas"), Sense::click_and_drag());
    let ppp = ui.ctx().pixels_per_point();
    let vp = [rect.width(), rect.height()];
    let dims = [open.doc.width, open.doc.height];
    let rotation = open.view.map_or(0.0, |v| v.rotation_deg);

    match cmd {
        Some(ViewCmd::Fit) => open.fit_mode = true,
        Some(ViewCmd::Actual) => {
            let mut v = View::actual_size(dims, vp, ppp);
            v.rotation_deg = rotation;
            open.view = Some(v);
            open.fit_mode = false;
        }
        Some(c @ (ViewCmd::ZoomIn | ViewCmd::ZoomOut)) => {
            let f = if c == ViewCmd::ZoomIn {
                STEP
            } else {
                1.0 / STEP
            };
            if let Some(v) = open.view.as_mut() {
                v.zoom_at([vp[0] / 2.0, vp[1] / 2.0], f);
                open.fit_mode = false;
            }
        }
        Some(ViewCmd::Rotate(d)) => {
            if let Some(v) = open.view.as_mut() {
                v.rotate_by(d);
            }
        }
        Some(ViewCmd::ResetRotation) => {
            if let Some(v) = open.view.as_mut() {
                let r = v.rotation_deg;
                v.rotate_by(-r);
            }
        }
        None => {}
    }
    if open.fit_mode || open.view.is_none() {
        open.view = Some(View::fit_rotated(dims, vp, FIT_MARGIN, 1.0 / ppp, rotation));
    }
    let mut view = open
        .view
        .unwrap_or_else(|| View::fit(dims, vp, FIT_MARGIN, 1.0));
    view.pivot = [vp[0] / 2.0, vp[1] / 2.0];

    // Zoom com roda/pinça ancorado no ponteiro.
    if resp.hovered() {
        let (scroll, zd, ptr) = ui.input(|i| {
            (
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                i.pointer.hover_pos(),
            )
        });
        let factor = zd * (scroll * 0.0015).exp();
        if (factor - 1.0).abs() > 1e-4 {
            if let Some(p) = ptr {
                view.zoom_at([p.x - rect.min.x, p.y - rect.min.y], factor);
                open.fit_mode = false;
            }
        }
    }
    // Pan: botão do meio, ou Espaço + arrastar.
    let panning = resp.dragged_by(PointerButton::Middle)
        || (space_down && resp.dragged_by(PointerButton::Primary));
    if panning {
        let d = resp.drag_delta();
        view.pan([d.x, d.y]);
        open.fit_mode = false;
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if resp.hovered() && space_down {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    } else if resp.hovered() && overlay.tool_cursor {
        ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
    }
    open.view = Some(view);
    open.zoom_percent = view.zoom * ppp * 100.0;

    let to_doc = |p: egui::Pos2| view.view_to_doc([p.x - rect.min.x, p.y - rect.min.y]);
    let guides_now = open.doc.guides.clone();
    let thr = 8.0 / view.zoom;
    let snap_on = overlay.snap_pointer && overlay.view.snap;
    let snap = |d: [f32; 2]| -> [f32; 2] {
        if snap_on {
            let (x, y) = snap_point(
                (d[0], d[1]),
                &guides_now,
                &overlay.view.snap_opts,
                (dims[0], dims[1]),
                thr,
            );
            [x, y]
        } else {
            d
        }
    };
    let mut out = CanvasOut {
        pressure: 1.0,
        zoom: view.zoom,
        view: Some(view),
        origin: rect.min,
        hover: resp.hover_pos().map(to_doc).map(snap),
        ..Default::default()
    };
    let inside = ui.rect_contains_pointer(rect) && resp.hovered();
    let (pressed, down, released, pts, (alt, shift, dbl), force, latest) = ui.input(|i| {
        let pts: Vec<egui::Pos2> = i
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::PointerMoved(p) => Some(*p),
                _ => None,
            })
            .collect();
        let force = i.events.iter().rev().find_map(|e| match e {
            egui::Event::Touch { force, .. } => *force,
            _ => None,
        });
        (
            i.pointer.button_pressed(PointerButton::Primary),
            i.pointer.button_down(PointerButton::Primary),
            i.pointer.button_released(PointerButton::Primary),
            pts,
            (
                i.modifiers.alt,
                i.modifiers.shift,
                i.pointer.button_double_clicked(PointerButton::Primary),
            ),
            force,
            i.pointer.latest_pos(),
        )
    });
    out.alt = alt;
    out.shift = shift;
    out.double_click = dbl;
    out.down = down;
    out.released = released;
    if let Some(f) = force {
        out.pressure = f.clamp(0.0, 1.0);
    }

    // ---- Guias: criar arrastando das réguas, mover ou remover arrastando de volta ----
    let mut guide_consumed = open.guide_drag.is_some();
    if open.guide_drag.is_none() && pressed && !space_down && !panning {
        if let Some(p) = latest {
            let in_top = ruler > 0.0 && p.y >= full.min.y && p.y < rect.min.y && p.x >= rect.min.x;
            let in_left = ruler > 0.0 && p.x >= full.min.x && p.x < rect.min.x && p.y >= rect.min.y;
            let mut start: Option<(Option<usize>, bool)> = None;
            if in_top {
                start = Some((None, false));
            } else if in_left {
                start = Some((None, true));
            } else if overlay.view.guides && inside && !rotated {
                for (i, g) in open.doc.guides.iter().enumerate() {
                    let s = view.doc_to_view([g.pos, g.pos]);
                    let d = if g.vertical {
                        (p.x - rect.min.x - s[0]).abs()
                    } else {
                        (p.y - rect.min.y - s[1]).abs()
                    };
                    if d <= 4.0 {
                        start = Some((Some(i), g.vertical));
                        break;
                    }
                }
            }
            if let Some((index, vertical)) = start {
                open.guide_drag = Some(GuideDrag {
                    index,
                    vertical,
                    pos: 0.0,
                    remove: false,
                    before: DocState::of(&open.doc),
                });
                guide_consumed = true;
            }
        }
    }
    if let Some(gd) = open.guide_drag.as_mut() {
        if let Some(p) = latest {
            let d = to_doc(p);
            gd.pos = if gd.vertical { d[0] } else { d[1] };
            let in_ruler_zone = ruler > 0.0 && (p.x < rect.min.x || p.y < rect.min.y);
            gd.remove = !rect.expand(0.0).contains(p) && !in_ruler_zone
                || (in_ruler_zone && gd.index.is_some());
            if gd.index.is_none() {
                gd.remove = !rect.contains(p);
            }
        }
        ui.ctx().set_cursor_icon(if gd.vertical {
            CursorIcon::ResizeHorizontal
        } else {
            CursorIcon::ResizeVertical
        });
        if released || !down {
            let gd = open.guide_drag.take();
            if let Some(gd) = gd {
                let mut after = gd.before.clone();
                match (gd.index, gd.remove) {
                    (Some(i), true) => {
                        if i < after.guides.len() {
                            after.guides.remove(i);
                        }
                    }
                    (Some(i), false) => {
                        if let Some(g) = after.guides.get_mut(i) {
                            g.pos = gd.pos.round();
                        }
                    }
                    (None, false) => after.guides.push(Guide {
                        vertical: gd.vertical,
                        pos: gd.pos.round(),
                    }),
                    (None, true) => {}
                }
                if after.guides != gd.before.guides {
                    open.execute_nonpixel(Box::new(DocEdit::new("Guias", gd.before, after)));
                    open.doc.modified = true;
                }
            }
        }
    } else if resp.hovered() && overlay.view.guides && !rotated && !space_down {
        if let Some(p) = resp.hover_pos() {
            for g in &open.doc.guides {
                let s = view.doc_to_view([g.pos, g.pos]);
                let d = if g.vertical {
                    (p.x - rect.min.x - s[0]).abs()
                } else {
                    (p.y - rect.min.y - s[1]).abs()
                };
                if d <= 4.0 {
                    ui.ctx().set_cursor_icon(if g.vertical {
                        CursorIcon::ResizeHorizontal
                    } else {
                        CursorIcon::ResizeVertical
                    });
                }
            }
        }
    }

    if guide_consumed {
        out.down = false;
        out.released = false;
    } else if pressed && inside && !space_down && !panning {
        out.pressed = latest.map(to_doc).map(snap);
    } else if down && !space_down {
        out.moves = pts.into_iter().map(to_doc).map(snap).collect();
        if out.moves.is_empty() {
            out.moves.extend(latest.map(to_doc).map(snap));
        }
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let ts = |p: [f32; 2]| {
        let v = view.doc_to_view(p);
        rect.min + egui::vec2(v[0], v[1])
    };
    let (dw, dh) = (dims[0] as f32, dims[1] as f32);
    let corners = [ts([0.0, 0.0]), ts([dw, 0.0]), ts([dw, dh]), ts([0.0, dh])];
    let full_uv = [
        egui::pos2(0.0, 0.0),
        egui::pos2(1.0, 0.0),
        egui::pos2(1.0, 1.0),
        egui::pos2(0.0, 1.0),
    ];
    // Xadrez de transparência: textura repetida de 16 px, ancorada na tela.
    let checker = checker_texture(ui.ctx());
    let ck_uv = corners.map(|c| egui::pos2((c.x - rect.min.x) / 16.0, (c.y - rect.min.y) / 16.0));
    draw_quad(&painter, checker.id(), corners, ck_uv);

    if dims[0].max(dims[1]) > MAX_TEXTURE_SIDE {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Documento grande demais para visualização nesta versão (limite 8192 px por lado).",
            egui::FontId::proportional(14.0),
            Color32::LIGHT_RED,
        );
    } else {
        sync_texture(ui.ctx(), open, "doc", &render_composite);
        if let Some((_, tex)) = &open.texture {
            draw_quad(&painter, tex.id(), corners, full_uv);
        }
        if let Some(qm) = open.qm.as_deref_mut() {
            sync_texture(ui.ctx(), qm, "qmask", &render_quick_mask);
            if let Some((_, tex)) = &qm.texture {
                draw_quad(&painter, tex.id(), corners, full_uv);
            }
        }
    }

    if overlay.view.grid {
        draw_grid(
            &painter,
            rect,
            &view,
            dims,
            overlay.view.snap_opts.grid_size,
        );
    }
    if overlay.view.guides {
        let drag = open.guide_drag.as_ref();
        for (i, g) in open.doc.guides.iter().enumerate() {
            if drag.is_some_and(|d| d.index == Some(i)) {
                continue;
            }
            draw_guide(&painter, &ts, g.vertical, g.pos, false);
        }
        if let Some(d) = drag.filter(|d| !d.remove) {
            draw_guide(&painter, &ts, d.vertical, d.pos, true);
        }
    }

    let phase = (ui.input(|i| i.time) * 24.0) as f32;
    let animate = open.doc.selection.mask.is_some() || overlay.shape.is_some();
    if open.qm.is_none() {
        draw_selection(&painter, open, &view, rect, phase);
    }
    if let Some(shape) = &overlay.shape {
        draw_shape(&painter, shape, &view, rect, phase);
    }
    for p in &overlay.points {
        let c = ts([p.0, p.1]);
        let r = Rect::from_center_size(c, egui::vec2(8.0, 8.0));
        painter.rect_filled(r, 1.0, Color32::WHITE);
        painter.rect_stroke(
            r,
            1.0,
            Stroke::new(1.0, Color32::BLACK),
            egui::StrokeKind::Inside,
        );
    }
    if animate {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(80));
    }

    if let (Some(r), Some(p)) = (overlay.brush_radius_doc, resp.hover_pos()) {
        let rr = r * view.zoom;
        if rr >= 2.0 && !space_down {
            painter.circle_stroke(p, rr, Stroke::new(2.0, Color32::BLACK.gamma_multiply(0.6)));
            painter.circle_stroke(p, rr, Stroke::new(1.0, Color32::WHITE));
        }
    }

    if ruler > 0.0 {
        let rp = ui.painter_at(full);
        draw_rulers(
            &rp,
            full,
            rect,
            &view,
            (open.doc.ppi, overlay.view.unit),
            resp.hover_pos()
                .map(|p| [p.x - rect.min.x, p.y - rect.min.y]),
        );
    }
    out
}

fn checker_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::unique("orvalith_checker_texture");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let mut bytes = Vec::with_capacity(256);
    for y in 0..16 {
        for x in 0..16 {
            bytes.push(if (x / 8 + y / 8) % 2 == 1 { 150 } else { 200 });
        }
    }
    let img = egui::ColorImage::from_gray([16, 16], &bytes);
    let opts = egui::TextureOptions {
        wrap_mode: egui::TextureWrapMode::Repeat,
        ..egui::TextureOptions::NEAREST
    };
    let tex = ctx.load_texture("orvalith_checker", img, opts);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

fn draw_quad(
    painter: &egui::Painter,
    tex: egui::TextureId,
    c: [egui::Pos2; 4],
    uv: [egui::Pos2; 4],
) {
    let mut mesh = egui::Mesh::with_texture(tex);
    for i in 0..4 {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: c[i],
            uv: uv[i],
            color: Color32::WHITE,
        });
    }
    mesh.indices = vec![0, 1, 2, 0, 2, 3];
    painter.add(egui::Shape::mesh(mesh));
}

fn draw_guide(
    painter: &egui::Painter,
    ts: &dyn Fn([f32; 2]) -> egui::Pos2,
    vertical: bool,
    pos: f32,
    preview: bool,
) {
    const FAR: f32 = 1.0e5;
    let (a, b) = if vertical {
        (ts([pos, -FAR]), ts([pos, FAR]))
    } else {
        (ts([-FAR, pos]), ts([FAR, pos]))
    };
    let color = if preview {
        Color32::from_rgb(0x60, 0xe0, 0xff)
    } else {
        Color32::from_rgb(0x2f, 0xb8, 0xe6)
    };
    painter.line_segment([a, b], Stroke::new(1.0, color));
}

fn draw_grid(painter: &egui::Painter, rect: Rect, view: &View, dims: [u32; 2], size: f32) {
    if size < 1.0 || size * view.zoom < 4.0 {
        return;
    }
    let stroke = Stroke::new(1.0, Color32::from_white_alpha(40));
    let to = |p: [f32; 2]| {
        let v = view.doc_to_view(p);
        rect.min + egui::vec2(v[0], v[1])
    };
    let (w, h) = (dims[0] as f32, dims[1] as f32);
    let mut x = 0.0;
    let mut n = 0;
    while x <= w && n < 600 {
        painter.line_segment([to([x, 0.0]), to([x, h])], stroke);
        x += size;
        n += 1;
    }
    let mut y = 0.0;
    n = 0;
    while y <= h && n < 600 {
        painter.line_segment([to([0.0, y]), to([w, y])], stroke);
        y += size;
        n += 1;
    }
}

/// Passo "bonito" (1, 2, 5 × 10^n) em unidades da régua para que haja ~60 pt entre rótulos.
fn nice_step(units_per_point: f32) -> f32 {
    let target = units_per_point * 60.0;
    let pow = 10f32.powf(target.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * pow >= target {
            return m * pow;
        }
    }
    10.0 * pow
}

fn draw_rulers(
    painter: &egui::Painter,
    full: Rect,
    rect: Rect,
    view: &View,
    (ppi, unit): (f32, RulerUnit),
    hover: Option<[f32; 2]>,
) {
    let bg = Color32::from_gray(45);
    let fg = Color32::from_gray(190);
    let font = egui::FontId::proportional(9.0);
    painter.rect_filled(
        Rect::from_min_max(full.min, egui::pos2(full.max.x, rect.min.y)),
        0.0,
        bg,
    );
    painter.rect_filled(
        Rect::from_min_max(full.min, egui::pos2(rect.min.x, full.max.y)),
        0.0,
        bg,
    );
    let ppu = unit.px_per_unit(ppi); // pixels do documento por unidade
    let step_units = nice_step(1.0 / (ppu * view.zoom));
    let step_px = step_units * ppu;
    for horizontal in [true, false] {
        let (origin, len, start, end) = if horizontal {
            (view.offset[0], rect.width(), 0.0, rect.width())
        } else {
            (view.offset[1], rect.height(), 0.0, rect.height())
        };
        let first = ((start - origin) / view.zoom / step_px).floor() as i64;
        let last = ((end - origin) / view.zoom / step_px).ceil() as i64;
        let _ = len;
        for k in first..=last {
            let doc = k as f32 * step_px;
            let screen = origin + doc * view.zoom;
            for sub in 0..5 {
                let pos = screen + sub as f32 * step_px * view.zoom / 5.0;
                if pos < 0.0
                    || pos
                        > if horizontal {
                            rect.width()
                        } else {
                            rect.height()
                        }
                {
                    continue;
                }
                let (a, b) = if horizontal {
                    let x = rect.min.x + pos;
                    let t = if sub == 0 { 0.0 } else { 12.0 };
                    (egui::pos2(x, full.min.y + t), egui::pos2(x, rect.min.y))
                } else {
                    let y = rect.min.y + pos;
                    let t = if sub == 0 { 0.0 } else { 12.0 };
                    (egui::pos2(full.min.x + t, y), egui::pos2(rect.min.x, y))
                };
                painter.line_segment([a, b], Stroke::new(1.0, fg));
            }
            let label = format!("{}", (k as f32 * step_units * 100.0).round() / 100.0);
            if horizontal {
                painter.text(
                    egui::pos2(rect.min.x + screen + 2.0, full.min.y),
                    egui::Align2::LEFT_TOP,
                    label,
                    font.clone(),
                    fg,
                );
            } else {
                painter.text(
                    egui::pos2(full.min.x + 1.0, rect.min.y + screen + 1.0),
                    egui::Align2::LEFT_TOP,
                    label
                        .chars()
                        .map(|c| format!("{c}\n"))
                        .collect::<String>()
                        .trim_end()
                        .to_string(),
                    font.clone(),
                    fg,
                );
            }
        }
    }
    if let Some(h) = hover {
        let c = Color32::from_rgb(0x2f, 0xb8, 0xa6);
        painter.line_segment(
            [
                egui::pos2(rect.min.x + h[0], full.min.y),
                egui::pos2(rect.min.x + h[0], rect.min.y),
            ],
            Stroke::new(1.0, c),
        );
        painter.line_segment(
            [
                egui::pos2(full.min.x, rect.min.y + h[1]),
                egui::pos2(rect.min.x, rect.min.y + h[1]),
            ],
            Stroke::new(1.0, c),
        );
    }
}

/// Atualiza a textura: só a região alterada quando possível, ou tudo se a mudança foi geral.
/// `render` produz os pixels RGBA da região pedida.
fn sync_texture(
    ctx: &egui::Context,
    open: &mut OpenDoc,
    name: &str,
    render: &dyn Fn(&OpenDoc, PixRect) -> Vec<u8>,
) {
    let rev = open.doc.revision;
    if open.texture.as_ref().map(|(r, _)| *r) == Some(rev) {
        return;
    }
    let (w, h) = (open.doc.width, open.doc.height);
    let partial = open.texture.is_some() && open.dirty.is_some() && open.tracked_rev == rev;
    if partial {
        let r = open
            .dirty
            .take()
            .unwrap_or(PixRect::full(w, h))
            .clamp_to(w, h);
        if !r.is_empty() {
            let rgba = render(open, r);
            let image =
                egui::ColorImage::from_rgba_unmultiplied([r.w as usize, r.h as usize], &rgba);
            if let Some((trev, tex)) = open.texture.as_mut() {
                tex.set_partial([r.x as usize, r.y as usize], image, texture_options());
                *trev = rev;
            }
        } else if let Some((trev, _)) = open.texture.as_mut() {
            *trev = rev;
        }
    } else {
        let rgba = render(open, PixRect::full(w, h));
        let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
        let tex = ctx.load_texture(name, image, texture_options());
        open.texture = Some((rev, tex));
        open.dirty = None;
    }
}

fn render_composite(o: &OpenDoc, r: PixRect) -> Vec<u8> {
    // Visão isolada de um canal salvo (cinza).
    if let Some(c) = o
        .channel_view
        .and_then(|id| o.doc.channels.iter().find(|c| c.id == id))
    {
        let (w, rr) = (o.doc.width as usize, r.clamp_to(o.doc.width, o.doc.height));
        let mut out = Vec::with_capacity(rr.w as usize * rr.h as usize * 4);
        for y in rr.y..rr.bottom() {
            for x in rr.x..rr.right() {
                let v = c
                    .mask
                    .get(y as usize * w + x as usize)
                    .copied()
                    .unwrap_or(0);
                out.extend_from_slice(&[v, v, v, 255]);
            }
        }
        return out;
    }
    let opts = RenderOpts {
        isolate: o.isolated,
        show_mask: o.mask_view,
    };
    let mut px = match (o.anim_frame, o.doc.timeline.as_ref()) {
        (Some(f), Some(tl)) => {
            let full = match o.anim_onion {
                Some((b, a, op)) => {
                    orvalith_core::anim::render_onion(&o.doc, tl, f, b, a, op as f32 / 100.0)
                }
                None => orvalith_core::anim::render_frame(&o.doc, tl, f),
            };
            let rr = r.clamp_to(o.doc.width, o.doc.height);
            let w = o.doc.width as usize;
            let mut out = Vec::with_capacity(rr.w as usize * rr.h as usize * 4);
            for y in rr.y..rr.bottom() {
                let s = (y as usize * w + rr.x as usize) * 4;
                out.extend_from_slice(&full[s..s + rr.w as usize * 4]);
            }
            out
        }
        _ => compose::flatten_region_with(&o.doc, r, opts),
    };
    if let Some(p) = &o.preview {
        p.apply(&mut px);
    }
    apply_channel_view(&mut px, o.view_channels);
    px
}

/// Mostra só os canais de cor escolhidos: um canal = tons de cinza; vários = os demais zerados.
pub fn apply_channel_view(rgba: &mut [u8], ch: [bool; 3]) {
    let on = ch.iter().filter(|&&c| c).count();
    if on == 3 {
        return;
    }
    for p in rgba.as_chunks_mut::<4>().0 {
        if on == 1 {
            let i = ch.iter().position(|&c| c).unwrap_or(0);
            let v = p[i];
            p[0] = v;
            p[1] = v;
            p[2] = v;
        } else {
            for (c, &show) in ch.iter().enumerate() {
                if !show {
                    p[c] = 0;
                }
            }
        }
    }
}

/// Sobreposição vermelha (50%) sobre as áreas NÃO selecionadas da máscara rápida.
fn render_quick_mask(o: &OpenDoc, r: PixRect) -> Vec<u8> {
    let layer = &o.doc.layers[0];
    let r = r.clamp_to(layer.width, layer.height);
    let mut out = Vec::with_capacity(r.w as usize * r.h as usize * 4);
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            let v = layer.pixels[(y as usize * layer.width as usize + x as usize) * 4];
            out.extend_from_slice(&[255, 0, 0, (255 - v) / 2]);
        }
    }
    out
}

/// Segmentos (x0, y0, x1, y1) da borda da seleção (limiar 50%), com trechos retos fundidos.
pub fn selection_edges(mask: &[u8], w: u32, h: u32) -> Vec<[f32; 4]> {
    const CAP: usize = 400_000;
    let inside = |x: i64, y: i64| -> bool {
        x >= 0
            && y >= 0
            && x < i64::from(w)
            && y < i64::from(h)
            && mask[y as usize * w as usize + x as usize] >= 128
    };
    let mut segs = Vec::new();
    for y in 0..=i64::from(h) {
        let mut start: Option<i64> = None;
        for x in 0..=i64::from(w) {
            let edge = x < i64::from(w) && inside(x, y - 1) != inside(x, y);
            match (edge, start) {
                (true, None) => start = Some(x),
                (false, Some(s)) => {
                    segs.push([s as f32, y as f32, x as f32, y as f32]);
                    start = None;
                }
                _ => {}
            }
        }
        if segs.len() > CAP {
            return segs;
        }
    }
    for x in 0..=i64::from(w) {
        let mut start: Option<i64> = None;
        for y in 0..=i64::from(h) {
            let edge = y < i64::from(h) && inside(x - 1, y) != inside(x, y);
            match (edge, start) {
                (true, None) => start = Some(y),
                (false, Some(s)) => {
                    segs.push([x as f32, s as f32, x as f32, y as f32]);
                    start = None;
                }
                _ => {}
            }
        }
        if segs.len() > CAP {
            return segs;
        }
    }
    segs
}

fn ants(painter: &egui::Painter, pts: &[egui::Pos2], phase: f32) {
    let shadow = Stroke::new(1.5, Color32::BLACK);
    let light = Stroke::new(1.5, Color32::WHITE);
    painter.add(egui::Shape::line(pts.to_vec(), shadow));
    painter.extend(egui::Shape::dashed_line_with_offset(
        pts,
        light,
        &[4.0],
        &[4.0],
        phase,
    ));
}

fn draw_selection(
    painter: &egui::Painter,
    open: &mut OpenDoc,
    view: &View,
    rect: Rect,
    phase: f32,
) {
    let Some(mask) = open.doc.selection.mask.clone() else {
        open.sel_info = None;
        open.sel_edges = None;
        return;
    };
    let key = std::sync::Arc::as_ptr(&mask) as usize;
    if open.sel_edges.as_ref().map(|(k, _)| *k) != Some(key) {
        let edges = selection_edges(&mask, open.doc.width, open.doc.height);
        open.sel_edges = Some((key, edges));
        open.sel_info = orvalith_core::selection::bounds(&mask, open.doc.width, open.doc.height);
    }
    let to_screen = |x: f32, y: f32| {
        let v = view.doc_to_view([x, y]);
        rect.min + egui::vec2(v[0], v[1])
    };
    if let Some((_, edges)) = &open.sel_edges {
        let mut drawn = 0;
        for e in edges {
            let (a, b) = (to_screen(e[0], e[1]), to_screen(e[2], e[3]));
            if !rect.intersects(Rect::from_two_pos(a, b).expand(1.0)) {
                continue;
            }
            ants(painter, &[a, b], phase);
            drawn += 1;
            if drawn > 20_000 {
                break;
            }
        }
    }
}

fn draw_shape(painter: &egui::Painter, shape: &Shape, view: &View, rect: Rect, phase: f32) {
    let to_screen = |p: (f32, f32)| {
        let v = view.doc_to_view([p.0, p.1]);
        rect.min + egui::vec2(v[0], v[1])
    };
    if let Shape::Mesh(cols, rows, p) = shape {
        if p.len() == cols * rows {
            for j in 0..*rows {
                let line: Vec<egui::Pos2> =
                    (0..*cols).map(|i| to_screen(p[j * cols + i])).collect();
                ants(painter, &line, phase);
            }
            for i in 0..*cols {
                let line: Vec<egui::Pos2> =
                    (0..*rows).map(|j| to_screen(p[j * cols + i])).collect();
                ants(painter, &line, phase);
            }
        }
        return;
    }
    let pts: Vec<egui::Pos2> = match shape {
        Shape::Rect(a, b) => vec![
            to_screen(*a),
            to_screen((b.0, a.1)),
            to_screen(*b),
            to_screen((a.0, b.1)),
            to_screen(*a),
        ],
        Shape::Ellipse(a, b) => {
            let (cx, cy) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let (rx, ry) = ((b.0 - a.0).abs() / 2.0, (b.1 - a.1).abs() / 2.0);
            (0..=64)
                .map(|i| {
                    let t = i as f32 / 64.0 * std::f32::consts::TAU;
                    to_screen((cx + rx * t.cos(), cy + ry * t.sin()))
                })
                .collect()
        }
        Shape::Poly(p) => p.iter().map(|q| to_screen(*q)).collect(),
        Shape::Mesh(..) => Vec::new(),
    };
    if pts.len() >= 2 {
        ants(painter, &pts, phase);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_view_shows_gray_of_one_channel_or_zeroes_others() {
        let mut px = vec![10, 20, 30, 255, 200, 100, 50, 128];
        apply_channel_view(&mut px, [true, true, true]);
        assert_eq!(px, vec![10, 20, 30, 255, 200, 100, 50, 128]);
        let mut g = px.clone();
        apply_channel_view(&mut g, [false, true, false]);
        assert_eq!(g, vec![20, 20, 20, 255, 100, 100, 100, 128]);
        let mut rb = px.clone();
        apply_channel_view(&mut rb, [true, false, true]);
        assert_eq!(rb, vec![10, 0, 30, 255, 200, 0, 50, 128]);
    }
}
