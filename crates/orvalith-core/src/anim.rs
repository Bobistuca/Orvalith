//! Animação: linha do tempo com quadros (estados visuais das camadas) e quadros-chave de
//! posição, escala, rotação e opacidade, mais exportação em GIF e sequência PNG.
//!
//! Dois modos que se combinam: **quadro a quadro** (`frames`: cada quadro guarda uma
//! composição de camadas e quantos instantes da linha do tempo ele dura) e **quadros-chave**
//! (`tracks`: valores animados por camada com interpolação constante, linear ou suavizada).
//! Limitações declaradas: transformações agem em camadas raster (a máscara não acompanha o
//! movimento); a rotação/escala usa o centro do conteúdo visível da camada como âncora.

use std::io::Write;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::compose;
use crate::comps::{self, Comp};
use crate::document::{Document, Layer};
use crate::history::Command;
use crate::transform::{self, Affine, Interp};

pub const MAX_FRAMES: usize = 2000;
pub const MAX_TRACKS: usize = 2000;
pub const MAX_KEYS: usize = 5000;
pub const MAX_LENGTH: u32 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Prop {
    X,
    Y,
    ScaleX,
    ScaleY,
    Rotation,
    Opacity,
}

impl Prop {
    pub const ALL: [Prop; 6] = [
        Prop::X,
        Prop::Y,
        Prop::ScaleX,
        Prop::ScaleY,
        Prop::Rotation,
        Prop::Opacity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Prop::X => "Posição X",
            Prop::Y => "Posição Y",
            Prop::ScaleX => "Escala X",
            Prop::ScaleY => "Escala Y",
            Prop::Rotation => "Rotação (°)",
            Prop::Opacity => "Opacidade",
        }
    }

    /// Valor quando não há trilha.
    pub fn neutral(self) -> f32 {
        match self {
            Prop::X | Prop::Y | Prop::Rotation => 0.0,
            Prop::ScaleX | Prop::ScaleY | Prop::Opacity => 1.0,
        }
    }
}

/// Interpolação do quadro-chave até o próximo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ease {
    /// Constante: mantém o valor até o próximo quadro-chave.
    Hold,
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl Ease {
    pub const ALL: [Ease; 5] = [
        Ease::Hold,
        Ease::Linear,
        Ease::EaseIn,
        Ease::EaseOut,
        Ease::EaseInOut,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Ease::Hold => "Constante",
            Ease::Linear => "Linear",
            Ease::EaseIn => "Aceleração",
            Ease::EaseOut => "Desaceleração",
            Ease::EaseInOut => "Suave",
        }
    }

    fn apply(self, u: f32) -> f32 {
        let u = u.clamp(0.0, 1.0);
        match self {
            Ease::Hold => 0.0,
            Ease::Linear => u,
            Ease::EaseIn => u * u,
            Ease::EaseOut => 1.0 - (1.0 - u) * (1.0 - u),
            Ease::EaseInOut => u * u * (3.0 - 2.0 * u),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub frame: u32,
    pub value: f32,
    pub ease: Ease,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub layer: u64,
    pub prop: Prop,
    pub keys: Vec<Key>,
}

/// Um quadro da animação quadro a quadro: o estado visual das camadas e a duração.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimFrame {
    pub comp: Comp,
    /// Duração em instantes da linha do tempo (1 = um quadro na taxa `fps`).
    pub hold: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub fps: f32,
    /// Duração total em quadros da linha do tempo.
    pub length: u32,
    /// Repetições do GIF exportado (0 = infinito).
    pub loop_count: u32,
    pub frames: Vec<AnimFrame>,
    pub tracks: Vec<Track>,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            fps: 12.0,
            length: 24,
            loop_count: 0,
            frames: Vec::new(),
            tracks: Vec::new(),
        }
    }
}

impl Timeline {
    /// Limita todos os valores a faixas válidas (entrada de arquivo hostil incluída).
    pub fn sanitize(&mut self) {
        self.fps = if self.fps.is_finite() {
            self.fps.clamp(1.0, 120.0)
        } else {
            12.0
        };
        self.length = self.length.clamp(1, MAX_LENGTH);
        self.frames.truncate(MAX_FRAMES);
        for f in &mut self.frames {
            f.hold = f.hold.clamp(1, 10_000);
        }
        self.tracks.truncate(MAX_TRACKS);
        for t in &mut self.tracks {
            t.keys.truncate(MAX_KEYS);
            for k in &mut t.keys {
                k.value = if k.value.is_finite() {
                    k.value.clamp(-1.0e6, 1.0e6)
                } else {
                    t.prop.neutral()
                };
                k.frame = k.frame.min(MAX_LENGTH * 10);
            }
            t.keys.sort_by_key(|k| k.frame);
            t.keys.dedup_by_key(|k| k.frame);
        }
        self.tracks.retain(|t| !t.keys.is_empty());
    }

    /// Duração em quadros: a dos quadros (se houver) ou o campo `length`.
    pub fn total(&self) -> u32 {
        if self.frames.is_empty() {
            self.length.max(1)
        } else {
            self.frames.iter().map(|f| f.hold).sum::<u32>().max(1)
        }
    }

    pub fn duration_secs(&self) -> f32 {
        self.total() as f32 / self.fps
    }

    /// Índice do quadro (quadro a quadro) exibido no instante `t`.
    pub fn frame_index_at(&self, t: u32) -> Option<usize> {
        if self.frames.is_empty() {
            return None;
        }
        let mut acc = 0;
        for (i, f) in self.frames.iter().enumerate() {
            acc += f.hold;
            if t < acc {
                return Some(i);
            }
        }
        Some(self.frames.len() - 1)
    }

    /// Instante em que o quadro `i` começa.
    pub fn frame_start(&self, i: usize) -> u32 {
        self.frames.iter().take(i).map(|f| f.hold).sum()
    }

    pub fn track(&self, layer: u64, prop: Prop) -> Option<&Track> {
        self.tracks
            .iter()
            .find(|t| t.layer == layer && t.prop == prop)
    }

    /// Valor animado no instante `t`, ou `None` se a camada não tem trilha dessa propriedade.
    pub fn value_at(&self, layer: u64, prop: Prop, t: u32) -> Option<f32> {
        let tr = self.track(layer, prop)?;
        let k = &tr.keys;
        let (first, last) = (k.first()?, k.last()?);
        if t <= first.frame {
            return Some(first.value);
        }
        if t >= last.frame {
            return Some(last.value);
        }
        let i = k.iter().rposition(|x| x.frame <= t)?;
        let (a, b) = (&k[i], &k[i + 1]);
        let u = (t - a.frame) as f32 / (b.frame - a.frame).max(1) as f32;
        Some(a.value + (b.value - a.value) * a.ease.apply(u))
    }

    /// Cria ou atualiza o quadro-chave em `frame`.
    pub fn set_key(&mut self, layer: u64, prop: Prop, frame: u32, value: f32, ease: Option<Ease>) {
        let idx = match self
            .tracks
            .iter()
            .position(|t| t.layer == layer && t.prop == prop)
        {
            Some(i) => i,
            None => {
                self.tracks.push(Track {
                    layer,
                    prop,
                    keys: Vec::new(),
                });
                self.tracks.len() - 1
            }
        };
        let keys = &mut self.tracks[idx].keys;
        match keys.iter_mut().find(|k| k.frame == frame) {
            Some(k) => {
                k.value = value;
                if let Some(e) = ease {
                    k.ease = e;
                }
            }
            None => {
                keys.push(Key {
                    frame,
                    value,
                    ease: ease.unwrap_or(Ease::Linear),
                });
                keys.sort_by_key(|k| k.frame);
            }
        }
    }

    pub fn remove_key(&mut self, layer: u64, prop: Prop, frame: u32) -> bool {
        let Some(tr) = self
            .tracks
            .iter_mut()
            .find(|t| t.layer == layer && t.prop == prop)
        else {
            return false;
        };
        let before = tr.keys.len();
        tr.keys.retain(|k| k.frame != frame);
        let removed = tr.keys.len() != before;
        self.tracks.retain(|t| !t.keys.is_empty());
        removed
    }

    pub fn key_at(&self, layer: u64, prop: Prop, frame: u32) -> Option<&Key> {
        self.track(layer, prop)?
            .keys
            .iter()
            .find(|k| k.frame == frame)
    }
}

fn content_center(l: &Layer, fallback: (f32, f32)) -> (f32, f32) {
    let (w, h) = (l.width as usize, l.height as usize);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            if l.pixels[(y * w + x) * 4 + 3] > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 > x0 {
        ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0)
    } else {
        fallback
    }
}

/// Camadas do documento como aparecem no instante `t` (estado do quadro + valores animados).
pub fn layers_at(doc: &Document, tl: &Timeline, t: u32) -> Vec<Layer> {
    let mut layers = match tl.frame_index_at(t) {
        Some(i) => comps::apply(doc, &tl.frames[i].comp),
        None => doc.layers.clone(),
    };
    for l in &mut layers {
        let id = l.id.0;
        if let Some(op) = tl.value_at(id, Prop::Opacity, t) {
            l.opacity = (l.opacity * op).clamp(0.0, 1.0);
        }
        if l.is_group() || l.adjustment.is_some() {
            continue;
        }
        let v = |p: Prop| tl.value_at(id, p, t);
        let (dx, dy, sx, sy, rot) = (
            v(Prop::X),
            v(Prop::Y),
            v(Prop::ScaleX),
            v(Prop::ScaleY),
            v(Prop::Rotation),
        );
        if dx.is_none() && dy.is_none() && sx.is_none() && sy.is_none() && rot.is_none() {
            continue;
        }
        let c = content_center(l, (doc.width as f32 / 2.0, doc.height as f32 / 2.0));
        let m = Affine::scale(sx.unwrap_or(1.0), sy.unwrap_or(1.0), c)
            .then(Affine::rotate(rot.unwrap_or(0.0), c))
            .then(Affine::translate(dx.unwrap_or(0.0), dy.unwrap_or(0.0)));
        if let Some(px) = transform::warp_affine(
            &l.pixels,
            l.width,
            l.height,
            &m,
            l.width,
            l.height,
            Interp::Bilinear,
        ) {
            l.pixels = std::sync::Arc::new(px);
        }
    }
    layers
}

/// Renderiza o documento no instante `t` (RGBA reto, tamanho do documento).
pub fn render_frame(doc: &Document, tl: &Timeline, t: u32) -> Vec<u8> {
    let mut d = doc.clone();
    d.layers = layers_at(doc, tl, t);
    compose::flatten(&d)
}

/// Instantes vizinhos usados pela pele de cebola (nos modos quadro a quadro e por quadros-chave).
pub fn onion_times(tl: &Timeline, t: u32, before: u32, after: u32) -> Vec<(u32, u32)> {
    let last = tl.total().saturating_sub(1);
    let mut out = Vec::new();
    match tl.frame_index_at(t) {
        Some(i) => {
            for k in 1..=before as usize {
                if i >= k {
                    out.push((tl.frame_start(i - k), k as u32));
                }
            }
            for k in 1..=after as usize {
                if i + k < tl.frames.len() {
                    out.push((tl.frame_start(i + k), k as u32));
                }
            }
        }
        None => {
            for k in 1..=before {
                if t >= k {
                    out.push((t - k, k));
                }
            }
            for k in 1..=after {
                if t + k <= last {
                    out.push((t + k, k));
                }
            }
        }
    }
    out
}

/// Quadro atual com a pele de cebola: os vizinhos são sobrepostos ao quadro atual com
/// `opacity` dividida pela distância (os mais distantes primeiro).
pub fn render_onion(
    doc: &Document,
    tl: &Timeline,
    t: u32,
    before: u32,
    after: u32,
    opacity: f32,
) -> Vec<u8> {
    let mut out = render_frame(doc, tl, t);
    let mut ghosts = onion_times(tl, t, before, after);
    ghosts.sort_by_key(|g| std::cmp::Reverse(g.1));
    for (gt, dist) in ghosts {
        let g = render_frame(doc, tl, gt);
        let a = (opacity / dist as f32).clamp(0.0, 1.0);
        for (o, p) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(g.as_chunks::<4>().0)
        {
            *o = compose::over(*p, *o, a);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// Exportação

#[derive(Debug, Clone)]
pub struct GifOpts {
    /// 1 = melhor qualidade (lento) … 30 = mais rápido.
    pub speed: i32,
    /// Alfa abaixo deste valor vira transparente (GIF tem transparência de 1 bit).
    pub alpha_threshold: u8,
    /// Compõe sobre esta cor em vez de manter transparência.
    pub background: Option<[u8; 3]>,
}

impl Default for GifOpts {
    fn default() -> Self {
        Self {
            speed: 10,
            alpha_threshold: 128,
            background: None,
        }
    }
}

pub struct GifResult {
    pub bytes: Vec<u8>,
    /// Quadros gravados (iguais consecutivos são fundidos num só, somando a duração).
    pub frames: usize,
    pub warnings: Vec<String>,
}

/// Exporta a animação como GIF animado (quantização por NeuQuant, transparência, tempo e repetição).
pub fn export_gif(doc: &Document, tl: &Timeline, o: &GifOpts) -> Result<GifResult, String> {
    let mut warnings = Vec::new();
    if tl.fps > 50.0 {
        warnings.push("GIF mede o tempo em centésimos de segundo (mínimo prático 20 ms): taxas acima de 50 fps são limitadas.".into());
    }
    warnings.push("GIF: no máximo 256 cores por quadro e transparência de 1 bit.".into());
    let total = tl.total();
    let ms_per = 1000.0 / tl.fps;
    // Renderiza e funde quadros idênticos consecutivos.
    let mut runs: Vec<(Vec<u8>, u32)> = Vec::new();
    for t in 0..total {
        let mut px = render_frame(doc, tl, t);
        for p in px.as_chunks_mut::<4>().0 {
            match o.background {
                Some(bg) => {
                    let a = u32::from(p[3]);
                    for (v, b) in p[..3].iter_mut().zip(bg) {
                        *v = ((u32::from(*v) * a + u32::from(b) * (255 - a) + 127) / 255) as u8;
                    }
                    p[3] = 255;
                }
                None => {
                    if p[3] >= o.alpha_threshold {
                        p[3] = 255;
                    } else {
                        *p = [0, 0, 0, 0];
                    }
                }
            }
        }
        match runs.last_mut() {
            Some((prev, n)) if *prev == px => *n += 1,
            _ => runs.push((px, 1)),
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new_with_speed(&mut out, o.speed.clamp(1, 30));
        enc.set_repeat(if tl.loop_count == 0 {
            Repeat::Infinite
        } else {
            Repeat::Finite(tl.loop_count.min(65535) as u16)
        })
        .map_err(|e| e.to_string())?;
        for (px, n) in &runs {
            let img = RgbaImage::from_raw(doc.width, doc.height, px.clone())
                .ok_or("Falha ao montar o quadro.")?;
            // Duração em centésimos de segundo (arredondada), no mínimo 2.
            let cs = ((*n as f32 * ms_per / 10.0).round() as u32).max(2);
            let frame = Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(cs * 10, 1));
            enc.encode_frame(frame).map_err(|e| e.to_string())?;
        }
    }
    let _ = Write::flush(&mut std::io::sink());
    Ok(GifResult {
        bytes: out,
        frames: runs.len(),
        warnings,
    })
}

/// Sequência PNG: `(nome, bytes)` de cada quadro (`base_0001.png`, …).
pub fn png_sequence(
    doc: &Document,
    tl: &Timeline,
    base: &str,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    use image::{ExtendedColorType, ImageEncoder};
    let total = tl.total();
    let digits = total.to_string().len().max(4);
    let mut out = Vec::with_capacity(total as usize);
    for t in 0..total {
        let px = render_frame(doc, tl, t);
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&px, doc.width, doc.height, ExtendedColorType::Rgba8)
            .map_err(|e| e.to_string())?;
        out.push((format!("{base}_{:0digits$}.png", t + 1), png));
    }
    Ok(out)
}

/// Troca a linha do tempo do documento numa única ação de histórico.
pub struct TimelineEdit {
    label: String,
    before: Option<Timeline>,
    after: Option<Timeline>,
}

impl TimelineEdit {
    pub fn new(label: &str, before: Option<Timeline>, after: Option<Timeline>) -> Self {
        Self {
            label: label.to_string(),
            before,
            after,
        }
    }
}

impl Command for TimelineEdit {
    fn label(&self) -> &str {
        &self.label
    }
    fn apply(&mut self, doc: &mut Document) {
        doc.timeline = self.after.clone();
    }
    fn revert(&mut self, doc: &mut Document) {
        doc.timeline = self.before.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::GifDecoder;
    use image::AnimationDecoder;
    use std::io::Cursor;

    /// Fundo azul + "título" (retângulo vermelho) que desliza para a direita e aparece.
    fn doc() -> (Document, u64) {
        let mut d = Document::new("anim", 40, 20, Some([0, 0, 120, 255])).unwrap();
        let mut title = Layer::transparent("Título", 40, 20);
        for y in 8..12 {
            for x in 2..8 {
                title.set_pixel(x, y, [255, 0, 0, 255]);
            }
        }
        let id = title.id.0;
        d.layers.push(title);
        (d, id)
    }

    fn slide(id: u64) -> Timeline {
        let mut tl = Timeline {
            fps: 10.0,
            length: 5,
            ..Timeline::default()
        };
        tl.set_key(id, Prop::X, 0, 0.0, Some(Ease::Linear));
        tl.set_key(id, Prop::X, 4, 20.0, None);
        tl.set_key(id, Prop::Opacity, 0, 0.0, Some(Ease::Linear));
        tl.set_key(id, Prop::Opacity, 2, 1.0, None);
        tl
    }

    #[test]
    fn interpolation_hold_linear_and_eased_curves() {
        let mut tl = Timeline::default();
        tl.set_key(1, Prop::X, 0, 0.0, Some(Ease::Linear));
        tl.set_key(1, Prop::X, 10, 100.0, Some(Ease::Hold));
        tl.set_key(1, Prop::X, 20, 200.0, None);
        assert_eq!(tl.value_at(1, Prop::X, 0), Some(0.0));
        assert_eq!(tl.value_at(1, Prop::X, 5), Some(50.0));
        assert_eq!(tl.value_at(1, Prop::X, 10), Some(100.0));
        assert_eq!(
            tl.value_at(1, Prop::X, 15),
            Some(100.0),
            "constante até o próximo"
        );
        assert_eq!(
            tl.value_at(1, Prop::X, 99),
            Some(200.0),
            "depois do último fica no último"
        );
        assert_eq!(tl.value_at(1, Prop::Y, 0), None);
        for (e, mid) in [
            (Ease::EaseIn, 25.0),
            (Ease::EaseOut, 75.0),
            (Ease::EaseInOut, 50.0),
        ] {
            let mut t = Timeline::default();
            t.set_key(2, Prop::Y, 0, 0.0, Some(e));
            t.set_key(2, Prop::Y, 10, 100.0, None);
            assert!(
                (t.value_at(2, Prop::Y, 5).unwrap() - mid).abs() < 1e-3,
                "{e:?}"
            );
            assert_eq!(t.value_at(2, Prop::Y, 10), Some(100.0));
        }
        assert!(tl.remove_key(1, Prop::X, 10) && !tl.remove_key(1, Prop::X, 10));
        assert_eq!(tl.key_at(1, Prop::X, 0).map(|k| k.value), Some(0.0));
    }

    #[test]
    fn rendering_moves_the_layer_and_fades_it_in() {
        let (d, id) = doc();
        let tl = slide(id);
        let f0 = render_frame(&d, &tl, 0);
        let f2 = render_frame(&d, &tl, 2);
        let f4 = render_frame(&d, &tl, 4);
        let px = |b: &[u8], x: usize, y: usize| {
            [
                b[(y * 40 + x) * 4],
                b[(y * 40 + x) * 4 + 1],
                b[(y * 40 + x) * 4 + 2],
            ]
        };
        assert_eq!(px(&f0, 4, 9), [0, 0, 120], "opacidade 0: só o fundo");
        assert_eq!(
            px(&f2, 4 + 10, 9),
            [255, 0, 0],
            "no meio: deslocado 10 px e opaco"
        );
        assert_eq!(px(&f2, 4, 9), [0, 0, 120], "posição original vazia");
        assert_eq!(px(&f4, 4 + 20, 9), [255, 0, 0]);
        // Sem trilhas, o quadro é idêntico à composição do documento.
        assert_eq!(
            render_frame(&d, &Timeline::default(), 3),
            compose::flatten(&d)
        );
    }

    #[test]
    fn scale_and_rotation_keyframes_transform_around_the_content_center() {
        let (d, id) = doc();
        let mut tl = Timeline::default();
        tl.set_key(id, Prop::ScaleX, 0, 3.0, None);
        let wide = render_frame(&d, &tl, 0);
        let cols = |b: &[u8]| {
            (0..40)
                .filter(|x| b[(9 * 40 + x) * 4] == 255 && b[(9 * 40 + x) * 4 + 1] == 0)
                .count()
        };
        assert!(
            cols(&wide) >= 12,
            "6 px × 3 (cortado na borda): {}",
            cols(&wide)
        );
        let mut r = Timeline::default();
        r.set_key(id, Prop::Rotation, 0, 90.0, None);
        let rot = render_frame(&d, &r, 0);
        let rows = (0..20)
            .filter(|y| rot[(y * 40 + 5) * 4] == 255 && rot[(y * 40 + 5) * 4 + 1] == 0)
            .count();
        assert!(rows >= 5, "retângulo 6×4 girado 90° fica 4×6: {rows}");
    }

    #[test]
    fn frame_by_frame_uses_compositions_durations_and_onion_skin() {
        let (mut d, id) = doc();
        let mut tl = Timeline {
            fps: 10.0,
            ..Timeline::default()
        };
        let on = comps::capture(&d, "a");
        d.layers[1].visible = false;
        let off = comps::capture(&d, "b");
        d.layers[1].visible = true;
        tl.frames = vec![
            AnimFrame { comp: on, hold: 2 },
            AnimFrame { comp: off, hold: 3 },
        ];
        assert_eq!(tl.total(), 5);
        assert_eq!(
            (
                tl.frame_index_at(0),
                tl.frame_index_at(1),
                tl.frame_index_at(2),
                tl.frame_index_at(4),
                tl.frame_index_at(99)
            ),
            (Some(0), Some(0), Some(1), Some(1), Some(1))
        );
        assert_eq!(tl.frame_start(1), 2);
        assert!((tl.duration_secs() - 0.5).abs() < 1e-6);
        let shown = render_frame(&d, &tl, 0);
        let hidden = render_frame(&d, &tl, 2);
        assert_eq!(&shown[(9 * 40 + 4) * 4..][..3], &[255, 0, 0]);
        assert_eq!(&hidden[(9 * 40 + 4) * 4..][..3], &[0, 0, 120]);
        // Cebola: no quadro 2 (oculto) o quadro anterior aparece esmaecido.
        let onion = render_onion(&d, &tl, 2, 1, 0, 0.5);
        let p = &onion[(9 * 40 + 4) * 4..][..3];
        assert!(
            p[0] > 60 && p[0] < 200,
            "vermelho esmaecido sobre o azul: {p:?}"
        );
        assert_eq!(onion_times(&tl, 2, 1, 1), vec![(0, 1)]);
        let _ = id;
    }

    #[test]
    fn gif_export_has_right_frames_delays_repeat_and_transparency() {
        let (mut d, id) = doc();
        d.layers[0].visible = false; // fundo escondido: transparência no GIF
        let tl = slide(id);
        let g = export_gif(&d, &tl, &GifOpts::default()).unwrap();
        assert!(g.bytes.starts_with(b"GIF89a"));
        assert!(g.frames >= 4, "{}", g.frames);
        let dec = GifDecoder::new(Cursor::new(&g.bytes)).unwrap();
        let frames: Vec<_> = dec.into_frames().collect::<Result<_, _>>().unwrap();
        assert_eq!(frames.len(), g.frames);
        // 5 quadros a 10 fps = 0,5 s no total.
        let total_ms: u32 = frames.iter().map(|f| f.delay().numer_denom_ms().0).sum();
        assert_eq!(total_ms, 500, "duração total preservada");
        // Repetição infinita (extensão NETSCAPE2.0 com contador 0).
        let pos = g
            .bytes
            .windows(11)
            .position(|w| w == b"NETSCAPE2.0")
            .expect("extensão de repetição");
        assert_eq!(&g.bytes[pos + 11..pos + 16], &[3, 1, 0, 0, 0]);
        // Último quadro: título em x+20; o fundo é transparente.
        let last = frames.last().unwrap().buffer();
        assert_eq!(last.get_pixel(0, 0)[3], 0, "fundo transparente");
        let hit = last.get_pixel(4 + 20, 9);
        assert!(hit[3] == 255 && hit[0] > 200 && hit[1] < 60, "{hit:?}");
        // Repetição finita e fundo composto.
        let mut tl2 = slide(id);
        tl2.loop_count = 3;
        let g2 = export_gif(
            &d,
            &tl2,
            &GifOpts {
                background: Some([10, 20, 30]),
                ..GifOpts::default()
            },
        )
        .unwrap();
        let pos = g2
            .bytes
            .windows(11)
            .position(|w| w == b"NETSCAPE2.0")
            .unwrap();
        assert_eq!(&g2.bytes[pos + 11..pos + 16], &[3, 1, 3, 0, 0]);
        let f: Vec<_> = GifDecoder::new(Cursor::new(&g2.bytes))
            .unwrap()
            .into_frames()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(f[0].buffer().get_pixel(0, 0)[3], 255);
    }

    #[test]
    fn identical_frames_merge_and_fast_rates_warn() {
        let (d, _) = doc();
        let tl = Timeline {
            fps: 60.0,
            length: 12,
            ..Timeline::default()
        };
        let g = export_gif(&d, &tl, &GifOpts::default()).unwrap();
        assert_eq!(
            g.frames, 1,
            "12 quadros iguais viram 1 com a duração somada"
        );
        assert!(g.warnings.iter().any(|w| w.contains("50 fps")));
        let f: Vec<_> = GifDecoder::new(Cursor::new(&g.bytes))
            .unwrap()
            .into_frames()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            f[0].delay().numer_denom_ms().0,
            200,
            "12 × 16,7 ms ≈ 200 ms"
        );
    }

    #[test]
    fn png_sequence_has_ordered_names_and_correct_frame_contents() {
        let (d, id) = doc();
        let tl = slide(id);
        let seq = png_sequence(&d, &tl, "titulo").unwrap();
        assert_eq!(seq.len(), 5);
        assert_eq!(seq[0].0, "titulo_0001.png");
        assert_eq!(seq[4].0, "titulo_0005.png");
        for (t, (_, bytes)) in seq.iter().enumerate() {
            let img = image::load_from_memory(bytes).unwrap().to_rgba8();
            assert_eq!(img.as_raw(), &render_frame(&d, &tl, t as u32), "quadro {t}");
        }
    }

    #[test]
    fn hostile_timelines_are_sanitized_and_edits_are_undoable() {
        let mut tl = Timeline {
            fps: f32::NAN,
            length: 0,
            loop_count: 5,
            frames: vec![
                AnimFrame {
                    comp: Comp {
                        name: "x".into(),
                        states: vec![]
                    },
                    hold: 0
                };
                3000
            ],
            tracks: vec![
                Track {
                    layer: 1,
                    prop: Prop::X,
                    keys: vec![
                        Key {
                            frame: 5,
                            value: f32::INFINITY,
                            ease: Ease::Linear,
                        },
                        Key {
                            frame: 5,
                            value: 1.0,
                            ease: Ease::Hold,
                        },
                        Key {
                            frame: 1,
                            value: 2.0,
                            ease: Ease::Linear,
                        },
                    ],
                },
                Track {
                    layer: 2,
                    prop: Prop::Y,
                    keys: vec![],
                },
            ],
        };
        tl.sanitize();
        assert_eq!((tl.fps, tl.length, tl.frames.len()), (12.0, 1, MAX_FRAMES));
        assert!(tl.frames.iter().all(|f| f.hold == 1));
        assert_eq!(tl.tracks.len(), 1);
        assert_eq!(
            tl.tracks[0]
                .keys
                .iter()
                .map(|k| k.frame)
                .collect::<Vec<_>>(),
            vec![1, 5]
        );
        assert!(tl.tracks[0].keys.iter().all(|k| k.value.is_finite()));
        let (mut d, _) = doc();
        let mut h = crate::History::default();
        h.execute(
            &mut d,
            Box::new(TimelineEdit::new(
                "Criar animação",
                None,
                Some(Timeline::default()),
            )),
        );
        assert!(d.timeline.is_some());
        h.undo(&mut d);
        assert!(d.timeline.is_none());
    }
}
