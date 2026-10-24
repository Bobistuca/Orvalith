//! Projeto de vídeo básico: trilhas, clipes, corte/aparo/movimento, títulos e imagens por cima,
//! transições (fade/dissolver), mixagem de áudio e exportação com progresso e cancelamento.
//!
//! **Três tempos que não se misturam:**
//! * [`ProjectFrame`] — índice de quadro da linha do tempo do projeto (na taxa do projeto);
//! * [`MediaSeconds`] — carimbo de tempo dentro do arquivo de mídia (segundos);
//! * o índice de quadro de origem nunca é guardado: calcula-se a partir de `MediaSeconds`.
//!
//! O trabalho de mídia (decodificar, codificar) é feito pelo FFmpeg externo via [`crate::media`].
//! Limitações declaradas: sem reprodução em tempo real com som dentro do aplicativo (a prévia é
//! por quadro); velocidade de clipe fixa em 1×; sem efeitos de movimento avançados.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::adjust::Adjustment;
use crate::media::{scale_filter, Ffmpeg, FrameCache, MediaInfo};
use crate::transform::{self, Affine, Interp};

/// Índice de quadro na linha do tempo do projeto.
pub type ProjectFrame = u32;
/// Carimbo de tempo dentro de um arquivo de mídia.
pub type MediaSeconds = f64;

pub const MAX_TRACKS: usize = 64;
pub const MAX_CLIPS: usize = 2000;
pub const MAX_FRAMES: u32 = 24 * 3600 * 4; // 4 horas a 24 fps

/// Dados mínimos da mídia guardados no projeto (para aparar sem reabrir o arquivo).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaMeta {
    pub duration_s: MediaSeconds,
    pub has_video: bool,
    pub has_audio: bool,
    pub width: u32,
    pub height: u32,
}

impl MediaMeta {
    pub fn from_info(i: &MediaInfo) -> Self {
        Self {
            duration_s: i.duration_s,
            has_video: i.video.is_some(),
            has_audio: i.audio.is_some(),
            width: i.video.as_ref().map_or(0, |v| v.width),
            height: i.video.as_ref().map_or(0, |v| v.height),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClipSource {
    /// Arquivo de vídeo/áudio. `rel` é o caminho relativo ao projeto (gravado ao salvar).
    Media {
        path: String,
        rel: Option<String>,
        meta: MediaMeta,
    },
    /// Imagem/título: pixels RGBA gravados no projeto (vêm de um documento do Orvalith).
    Still {
        w: u32,
        h: u32,
        #[serde(skip)]
        pixels: Option<Arc<Vec<u8>>>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: u64,
    pub name: String,
    pub source: ClipSource,
    /// Início na linha do tempo do projeto.
    pub start: ProjectFrame,
    /// Duração em quadros do projeto.
    pub duration: u32,
    /// Ponto de entrada dentro da mídia.
    pub in_point: MediaSeconds,
    pub volume: f32,
    pub opacity: f32,
    /// Rampas (quadros do projeto) de opacidade/volume no começo e no fim: fade e dissolver.
    pub fade_in: u32,
    pub fade_out: u32,
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub adjust: Option<Adjustment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Video,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub kind: TrackKind,
    pub name: String,
    pub mute: bool,
    pub volume: f32,
    pub clips: Vec<Clip>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoProject {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub tracks: Vec<Track>,
    pub next_id: u64,
}

impl Default for VideoProject {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fps_num: 30,
            fps_den: 1,
            tracks: vec![
                Track {
                    kind: TrackKind::Video,
                    name: "Vídeo 1".into(),
                    mute: false,
                    volume: 1.0,
                    clips: vec![],
                },
                Track {
                    kind: TrackKind::Video,
                    name: "Vídeo 2 (títulos)".into(),
                    mute: false,
                    volume: 1.0,
                    clips: vec![],
                },
                Track {
                    kind: TrackKind::Audio,
                    name: "Áudio 1".into(),
                    mute: false,
                    volume: 1.0,
                    clips: vec![],
                },
            ],
            next_id: 1,
        }
    }
}

impl Clip {
    pub fn end(&self) -> ProjectFrame {
        self.start + self.duration
    }

    pub fn is_media(&self) -> bool {
        matches!(self.source, ClipSource::Media { .. })
    }

    pub fn has_audio(&self) -> bool {
        matches!(&self.source, ClipSource::Media { meta, .. } if meta.has_audio)
    }

    pub fn has_video(&self) -> bool {
        match &self.source {
            ClipSource::Media { meta, .. } => meta.has_video,
            ClipSource::Still { .. } => true,
        }
    }

    pub fn path(&self) -> Option<&str> {
        match &self.source {
            ClipSource::Media { path, .. } => Some(path),
            ClipSource::Still { .. } => None,
        }
    }

    /// Duração máxima possível a partir do ponto de entrada, em quadros (ilimitada para imagens).
    fn max_duration(&self, fps: f64) -> u32 {
        match &self.source {
            ClipSource::Media { meta, .. } => {
                (((meta.duration_s - self.in_point) * fps).floor().max(1.0) as u32).min(MAX_FRAMES)
            }
            ClipSource::Still { .. } => MAX_FRAMES,
        }
    }
}

impl VideoProject {
    pub fn fps(&self) -> f64 {
        f64::from(self.fps_num) / f64::from(self.fps_den.max(1))
    }

    /// Duração total: o fim do último clipe.
    pub fn total_frames(&self) -> ProjectFrame {
        self.tracks
            .iter()
            .flat_map(|t| &t.clips)
            .map(Clip::end)
            .max()
            .unwrap_or(0)
    }

    pub fn duration_secs(&self) -> f64 {
        f64::from(self.total_frames()) / self.fps()
    }

    /// Converte um quadro do projeto no carimbo de tempo dentro da mídia do clipe.
    pub fn source_time(&self, clip: &Clip, frame: ProjectFrame) -> MediaSeconds {
        clip.in_point + f64::from(frame.saturating_sub(clip.start)) / self.fps()
    }

    pub fn frame_to_secs(&self, frame: ProjectFrame) -> f64 {
        f64::from(frame) / self.fps()
    }

    pub fn secs_to_frame(&self, secs: f64) -> ProjectFrame {
        (secs * self.fps()).round().max(0.0) as ProjectFrame
    }

    fn new_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Cria um clipe de mídia (ainda não inserido) com duração igual à da mídia.
    pub fn media_clip(
        &mut self,
        path: &str,
        name: &str,
        info: &MediaInfo,
        start: ProjectFrame,
    ) -> Clip {
        let meta = MediaMeta::from_info(info);
        let duration = ((meta.duration_s * self.fps()).floor().max(1.0) as u32).min(MAX_FRAMES);
        Clip {
            id: self.new_id(),
            name: name.chars().take(80).collect(),
            source: ClipSource::Media {
                path: path.to_string(),
                rel: None,
                meta,
            },
            start,
            duration,
            in_point: 0.0,
            volume: 1.0,
            opacity: 1.0,
            fade_in: 0,
            fade_out: 0,
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            adjust: None,
        }
    }

    /// Cria um clipe de imagem/título a partir de pixels RGBA (`w × h`).
    pub fn still_clip(
        &mut self,
        name: &str,
        w: u32,
        h: u32,
        pixels: Vec<u8>,
        start: ProjectFrame,
        secs: f64,
    ) -> Clip {
        Clip {
            id: self.new_id(),
            name: name.chars().take(80).collect(),
            source: ClipSource::Still {
                w,
                h,
                pixels: Some(Arc::new(pixels)),
            },
            start,
            duration: self.secs_to_frame(secs).max(1),
            in_point: 0.0,
            volume: 1.0,
            opacity: 1.0,
            fade_in: 0,
            fade_out: 0,
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            adjust: None,
        }
    }

    pub fn add_clip(&mut self, track: usize, clip: Clip) -> Result<(), String> {
        let n: usize = self.tracks.iter().map(|t| t.clips.len()).sum();
        if n >= MAX_CLIPS {
            return Err("Clipes demais no projeto.".into());
        }
        let tr = self.tracks.get_mut(track).ok_or("Trilha inexistente.")?;
        match (tr.kind, clip.has_video(), clip.has_audio()) {
            (TrackKind::Video, true, _) | (TrackKind::Audio, _, true) => {}
            (TrackKind::Video, false, _) => {
                return Err("Esta mídia não tem vídeo: coloque-a numa trilha de áudio.".into())
            }
            (TrackKind::Audio, _, false) => return Err("Esta mídia não tem áudio.".into()),
        }
        tr.clips.push(clip);
        tr.clips.sort_by_key(|c| c.start);
        Ok(())
    }

    fn find(&mut self, id: u64) -> Option<(usize, usize)> {
        for (ti, t) in self.tracks.iter().enumerate() {
            if let Some(ci) = t.clips.iter().position(|c| c.id == id) {
                return Some((ti, ci));
            }
        }
        None
    }

    pub fn clip(&self, id: u64) -> Option<&Clip> {
        self.tracks
            .iter()
            .flat_map(|t| &t.clips)
            .find(|c| c.id == id)
    }

    /// Move o clipe para `new_start` (nunca antes de 0).
    pub fn move_clip(&mut self, id: u64, new_start: ProjectFrame) -> bool {
        let Some((t, c)) = self.find(id) else {
            return false;
        };
        self.tracks[t].clips[c].start = new_start.min(MAX_FRAMES);
        self.tracks[t].clips.sort_by_key(|c| c.start);
        true
    }

    /// Aparar o início: o novo início fica entre o começo da mídia e o último quadro do clipe.
    pub fn trim_start(&mut self, id: u64, new_start: ProjectFrame) -> bool {
        let fps = self.fps();
        let Some((t, c)) = self.find(id) else {
            return false;
        };
        let clip = &mut self.tracks[t].clips[c];
        let end = clip.end();
        let lowest = match clip.source {
            ClipSource::Media { .. } => clip
                .start
                .saturating_sub((clip.in_point * fps).floor() as u32),
            ClipSource::Still { .. } => 0,
        };
        let ns = new_start.clamp(lowest, end.saturating_sub(1));
        let delta = i64::from(ns) - i64::from(clip.start);
        clip.in_point = (clip.in_point + delta as f64 / fps).max(0.0);
        clip.duration = end - ns;
        clip.start = ns;
        clip.fade_in = clip.fade_in.min(clip.duration);
        clip.fade_out = clip.fade_out.min(clip.duration);
        self.tracks[t].clips.sort_by_key(|c| c.start);
        true
    }

    /// Aparar o fim: limitado ao que resta de mídia.
    pub fn trim_end(&mut self, id: u64, new_end: ProjectFrame) -> bool {
        let fps = self.fps();
        let Some((t, c)) = self.find(id) else {
            return false;
        };
        let clip = &mut self.tracks[t].clips[c];
        let max = clip.max_duration(fps);
        clip.duration = new_end.saturating_sub(clip.start).clamp(1, max);
        clip.fade_in = clip.fade_in.min(clip.duration);
        clip.fade_out = clip.fade_out.min(clip.duration);
        true
    }

    /// Corta o clipe em `frame` (dois clipes consecutivos). Devolve o id do clipe da direita.
    pub fn split(&mut self, id: u64, frame: ProjectFrame) -> Option<u64> {
        let fps = self.fps();
        let (t, c) = self.find(id)?;
        let orig = self.tracks[t].clips[c].clone();
        if frame <= orig.start || frame >= orig.end() {
            return None;
        }
        let new_id = self.new_id();
        let left = &mut self.tracks[t].clips[c];
        left.duration = frame - orig.start;
        left.fade_out = 0;
        left.fade_in = left.fade_in.min(left.duration);
        let mut right = orig.clone();
        right.id = new_id;
        right.start = frame;
        right.duration = orig.end() - frame;
        right.in_point = orig.in_point + f64::from(frame - orig.start) / fps;
        right.fade_in = 0;
        right.fade_out = right.fade_out.min(right.duration);
        self.tracks[t].clips.push(right);
        self.tracks[t].clips.sort_by_key(|c| c.start);
        Some(new_id)
    }

    pub fn remove_clip(&mut self, id: u64) -> bool {
        match self.find(id) {
            Some((t, c)) => {
                self.tracks[t].clips.remove(c);
                true
            }
            None => false,
        }
    }

    /// Fade de entrada (de/para o fundo preto ou o que estiver abaixo) com `frames` quadros.
    pub fn set_fade(&mut self, id: u64, fade_in: u32, fade_out: u32) -> bool {
        let Some((t, c)) = self.find(id) else {
            return false;
        };
        let clip = &mut self.tracks[t].clips[c];
        clip.fade_in = fade_in.min(clip.duration);
        clip.fade_out = fade_out.min(clip.duration);
        true
    }

    /// Dissolver: o clipe passa a começar `frames` quadros antes do fim do anterior da mesma
    /// trilha e entra com fade (a sobreposição vira o cruzamento).
    pub fn dissolve_with_previous(&mut self, id: u64, frames: u32) -> Result<(), String> {
        let (t, c) = self.find(id).ok_or("Clipe inexistente.")?;
        let clips = &self.tracks[t].clips;
        let prev_end = clips[..c]
            .iter()
            .map(Clip::end)
            .max()
            .ok_or("Não há clipe anterior nesta trilha.")?;
        let cur = &clips[c];
        let frames = frames.clamp(1, cur.duration.saturating_sub(1).max(1));
        let ns = prev_end.saturating_sub(frames);
        let cur_id = cur.id;
        self.move_clip(cur_id, ns);
        self.set_fade(cur_id, frames, self.clip(cur_id).map_or(0, |c| c.fade_out));
        Ok(())
    }

    /// Ajusta valores inválidos (arquivo hostil) a faixas seguras.
    pub fn sanitize(&mut self) {
        let f = |v: f32, d: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
        self.width = (self.width.clamp(16, 7680)) & !1;
        self.height = (self.height.clamp(16, 4320)) & !1;
        self.fps_num = self.fps_num.clamp(1, 240);
        self.fps_den = self.fps_den.clamp(1, 1001);
        self.tracks.truncate(MAX_TRACKS);
        let mut budget = MAX_CLIPS;
        let mut max_id = self.next_id;
        for t in &mut self.tracks {
            t.volume = f(t.volume, 1.0, 0.0, 4.0);
            t.name = t.name.chars().take(80).collect();
            t.clips.truncate(budget);
            budget -= t.clips.len();
            for c in &mut t.clips {
                c.start = c.start.min(MAX_FRAMES);
                c.duration = c.duration.clamp(1, MAX_FRAMES);
                c.in_point = if c.in_point.is_finite() {
                    c.in_point.clamp(0.0, 86_400.0)
                } else {
                    0.0
                };
                c.volume = f(c.volume, 1.0, 0.0, 4.0);
                c.opacity = f(c.opacity, 1.0, 0.0, 1.0);
                c.x = f(c.x, 0.0, -20_000.0, 20_000.0);
                c.y = f(c.y, 0.0, -20_000.0, 20_000.0);
                c.scale = f(c.scale, 1.0, 0.01, 50.0);
                c.fade_in = c.fade_in.min(c.duration);
                c.fade_out = c.fade_out.min(c.duration);
                c.name = c.name.chars().take(80).collect();
                if let Some(a) = c.adjust.as_mut() {
                    a.sanitize();
                }
                max_id = max_id.max(c.id.saturating_add(1));
            }
            t.clips.sort_by_key(|c| c.start);
        }
        self.next_id = max_id;
    }

    // ---- Mídia ausente e caminhos relativos ----

    /// Preenche `rel` (relativo à pasta do projeto) para cada mídia, antes de salvar.
    pub fn relativize(&mut self, base_dir: &Path) {
        for c in self.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()) {
            if let ClipSource::Media { path, rel, .. } = &mut c.source {
                *rel = relative_to(Path::new(path), base_dir);
            }
        }
    }

    /// Ao abrir: se o caminho absoluto não existe mas o relativo existe, usa-o. Devolve os caminhos ainda ausentes.
    pub fn resolve_media(&mut self, base_dir: Option<&Path>) -> Vec<String> {
        let mut missing = Vec::new();
        for c in self.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()) {
            if let ClipSource::Media { path, rel, .. } = &mut c.source {
                if Path::new(path).exists() {
                    continue;
                }
                if let (Some(base), Some(r)) = (base_dir, rel.as_deref()) {
                    let cand = base.join(r);
                    if cand.exists() {
                        *path = cand.to_string_lossy().into_owned();
                        continue;
                    }
                }
                if !missing.contains(path) {
                    missing.push(path.clone());
                }
            }
        }
        missing
    }

    /// Caminhos de mídia que não existem no disco.
    pub fn missing_media(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in self.tracks.iter().flat_map(|t| &t.clips) {
            if let Some(p) = c.path() {
                if !Path::new(p).exists() && !out.iter().any(|x| x == p) {
                    out.push(p.to_string());
                }
            }
        }
        out
    }

    /// Reaponta todos os clipes de `old` para `new` e atualiza os metadados.
    pub fn relink(&mut self, old: &str, new: &str, info: &MediaInfo) -> usize {
        let mut n = 0;
        for c in self.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()) {
            if let ClipSource::Media { path, rel, meta } = &mut c.source {
                if path == old {
                    *path = new.to_string();
                    *rel = None;
                    *meta = MediaMeta::from_info(info);
                    n += 1;
                }
            }
        }
        n
    }
}

/// Caminho de `path` relativo a `base` (com `/`), ou `None` se estiverem em unidades diferentes.
pub fn relative_to(path: &Path, base: &Path) -> Option<String> {
    use std::path::Component;
    let p: Vec<Component> = path.components().collect();
    let b: Vec<Component> = base.components().collect();
    if p.first() != b.first() {
        return None;
    }
    let common = p.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..b.len() {
        parts.push("..".into());
    }
    for c in &p[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

// ---------------------------------------------------------------------------------------
// Composição de quadros

/// Fornece a imagem RGBA (`largura × altura` do projeto) de um clipe num quadro do projeto.
pub trait FrameProvider {
    fn clip_image(
        &mut self,
        p: &VideoProject,
        clip: &Clip,
        frame: ProjectFrame,
    ) -> Option<Arc<Vec<u8>>>;
}

/// Rampa de opacidade/volume (0..=1) de um clipe num quadro, por fade de entrada e saída.
pub fn ramp(clip: &Clip, frame: ProjectFrame) -> f32 {
    let off = frame.saturating_sub(clip.start);
    let mut a = 1.0f32;
    if clip.fade_in > 0 && off < clip.fade_in {
        a = a.min((off + 1) as f32 / (clip.fade_in + 1) as f32);
    }
    let left = clip.end().saturating_sub(frame); // quadros restantes, contando o atual
    if clip.fade_out > 0 && left <= clip.fade_out {
        a = a.min(left as f32 / (clip.fade_out + 1) as f32);
    }
    a
}

fn over_into(base: &mut [u8], img: &[u8], alpha: f32) {
    for (b, s) in base
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(img.as_chunks::<4>().0)
    {
        if s[3] == 0 {
            continue;
        }
        *b = crate::compose::over(*s, *b, alpha);
    }
}

/// Compõe o quadro `frame` do projeto (RGBA opaco sobre preto).
pub fn compose_frame(
    p: &VideoProject,
    frame: ProjectFrame,
    provider: &mut dyn FrameProvider,
) -> Vec<u8> {
    let (w, h) = (p.width as usize, p.height as usize);
    let mut base = vec![0u8; w * h * 4];
    for px in base.as_chunks_mut::<4>().0 {
        *px = [0, 0, 0, 255];
    }
    for t in p.tracks.iter().filter(|t| t.kind == TrackKind::Video) {
        for c in &t.clips {
            if frame < c.start || frame >= c.end() {
                continue;
            }
            let Some(img) = provider.clip_image(p, c, frame) else {
                continue;
            };
            if img.len() != w * h * 4 {
                continue;
            }
            let mut img = img.as_ref().clone();
            if let Some(a) = &c.adjust {
                crate::adjust::apply_rgba(a, &mut img);
            }
            if c.scale != 1.0 || c.x != 0.0 || c.y != 0.0 {
                let ctr = (w as f32 / 2.0, h as f32 / 2.0);
                let m = Affine::scale(c.scale, c.scale, ctr).then(Affine::translate(c.x, c.y));
                if let Some(moved) = transform::warp_affine(
                    &img,
                    w as u32,
                    h as u32,
                    &m,
                    w as u32,
                    h as u32,
                    Interp::Bilinear,
                ) {
                    img = moved;
                }
            }
            over_into(
                &mut base,
                &img,
                (c.opacity * ramp(c, frame)).clamp(0.0, 1.0),
            );
        }
    }
    base
}

/// Fornecedor de prévia: extrai quadros por tempo com cache; imagens são encaixadas uma vez.
pub struct PreviewProvider<'a> {
    pub ff: &'a Ffmpeg,
    pub cache: &'a mut FrameCache,
    stills: HashMap<u64, Arc<Vec<u8>>>,
}

impl<'a> PreviewProvider<'a> {
    pub fn new(ff: &'a Ffmpeg, cache: &'a mut FrameCache) -> Self {
        Self {
            ff,
            cache,
            stills: HashMap::new(),
        }
    }
}

/// Encaixa uma imagem em `w × h` (contém, centralizada, bordas transparentes).
pub fn fit_still(src: &[u8], sw: u32, sh: u32, w: u32, h: u32) -> Vec<u8> {
    if (sw, sh) == (w, h) {
        return src.to_vec();
    }
    let k = (w as f32 / sw as f32).min(h as f32 / sh as f32);
    let (nw, nh) = (
        ((sw as f32 * k).round() as u32).max(1),
        ((sh as f32 * k).round() as u32).max(1),
    );
    let scaled = transform::resample(src, sw, sh, nw, nh, Interp::Bilinear);
    let mut out = vec![0u8; w as usize * h as usize * 4];
    let (ox, oy) = ((w - nw.min(w)) / 2, (h - nh.min(h)) / 2);
    for y in 0..nh.min(h) {
        let d = ((oy + y) as usize * w as usize + ox as usize) * 4;
        let s = y as usize * nw as usize * 4;
        let n = nw.min(w) as usize * 4;
        out[d..d + n].copy_from_slice(&scaled[s..s + n]);
    }
    out
}

impl FrameProvider for PreviewProvider<'_> {
    fn clip_image(
        &mut self,
        p: &VideoProject,
        clip: &Clip,
        frame: ProjectFrame,
    ) -> Option<Arc<Vec<u8>>> {
        match &clip.source {
            ClipSource::Still { w, h, pixels } => {
                if let Some(v) = self.stills.get(&clip.id) {
                    return Some(v.clone());
                }
                let fitted = Arc::new(fit_still(pixels.as_ref()?, *w, *h, p.width, p.height));
                self.stills.insert(clip.id, fitted.clone());
                Some(fitted)
            }
            ClipSource::Media { path, meta, .. } if meta.has_video => {
                let t = p
                    .source_time(clip, frame)
                    .min((meta.duration_s - 0.001).max(0.0));
                self.cache
                    .get(self.ff, Path::new(path), t, p.width, p.height)
                    .ok()
            }
            ClipSource::Media { .. } => None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// Áudio

/// Plano de mixagem: argumentos do FFmpeg (entradas + filtro) e a duração total.
pub struct AudioPlan {
    pub inputs: Vec<OsString>,
    pub filter: String,
    pub count: usize,
}

/// Monta as entradas e o `filter_complex` de áudio (volume, fades, atraso, mistura). `None` sem áudio.
pub fn audio_plan(p: &VideoProject) -> Option<AudioPlan> {
    let fps = p.fps();
    let mut inputs: Vec<OsString> = Vec::new();
    let mut chains: Vec<String> = Vec::new();
    let mut labels = String::new();
    for t in p.tracks.iter().filter(|t| !t.mute) {
        for c in t.clips.iter().filter(|c| c.has_audio()) {
            let Some(path) = c.path() else { continue };
            let i = chains.len();
            let dur_s = f64::from(c.duration) / fps;
            inputs.extend([
                "-ss".into(),
                format!("{:.6}", c.in_point).into(),
                "-t".into(),
                format!("{dur_s:.6}").into(),
                "-i".into(),
                path.into(),
            ]);
            let vol = (t.volume * c.volume).max(0.0);
            let mut ch =
                format!("[{i}:a]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo");
            if (vol - 1.0).abs() > 1e-4 {
                ch.push_str(&format!(",volume={vol:.4}"));
            }
            if c.fade_in > 0 {
                ch.push_str(&format!(
                    ",afade=t=in:st=0:d={:.6}",
                    f64::from(c.fade_in) / fps
                ));
            }
            if c.fade_out > 0 {
                let d = f64::from(c.fade_out) / fps;
                ch.push_str(&format!(
                    ",afade=t=out:st={:.6}:d={d:.6}",
                    (dur_s - d).max(0.0)
                ));
            }
            let delay_ms = (f64::from(c.start) / fps * 1000.0).round() as u64;
            ch.push_str(&format!(",adelay={delay_ms}|{delay_ms}[a{i}]"));
            chains.push(ch);
            labels.push_str(&format!("[a{i}]"));
        }
    }
    if chains.is_empty() {
        return None;
    }
    let n = chains.len();
    let filter = format!(
        "{};{labels}amix=inputs={n}:normalize=0:duration=longest,apad[aout]",
        chains.join(";")
    );
    Some(AudioPlan {
        inputs,
        filter,
        count: n,
    })
}

// ---------------------------------------------------------------------------------------
// Exportação

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// MP4 com H.264 + AAC.
    H264Aac,
    /// WebM com VP9 + Opus (alternativa de licenciamento aberto).
    Vp9Opus,
}

impl Codec {
    pub fn extension(self) -> &'static str {
        match self {
            Codec::H264Aac => "mp4",
            Codec::Vp9Opus => "webm",
        }
    }

    pub fn available(self, ff: &Ffmpeg) -> bool {
        match self {
            Codec::H264Aac => ff.h264 && ff.aac,
            Codec::Vp9Opus => ff.vp9 && ff.opus,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExportSettings {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub codec: Codec,
    /// Qualidade: CRF (menor = melhor; 18 ≈ alta, 23 padrão, 28 baixa).
    pub crf: u32,
}

impl ExportSettings {
    pub fn from_project(p: &VideoProject) -> Self {
        Self {
            width: p.width,
            height: p.height,
            fps_num: p.fps_num,
            fps_den: p.fps_den,
            codec: Codec::H264Aac,
            crf: 23,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Cancelled => write!(f, "Exportação cancelada."),
            ExportError::Failed(m) => write!(f, "{m}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExportReport {
    pub frames: u32,
    pub duration_s: f64,
    pub has_audio: bool,
}

struct Decoder {
    child: Child,
    out: ChildStdout,
    last: Option<Arc<Vec<u8>>>,
    done: bool,
}

impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct ExportProvider<'a> {
    ff: &'a Ffmpeg,
    w: u32,
    h: u32,
    decoders: HashMap<u64, Decoder>,
    stills: HashMap<u64, Arc<Vec<u8>>>,
    error: Option<String>,
}

impl ExportProvider<'_> {
    fn spawn(&self, p: &VideoProject, clip: &Clip, path: &str) -> Result<Decoder, String> {
        let fps = format!("{}/{}", p.fps_num, p.fps_den);
        let vf = format!("fps={fps},{}", scale_filter(self.w, self.h));
        let dur = f64::from(clip.duration) / p.fps() + 0.5;
        let mut c = self.ff.command();
        c.args([
            "-ss",
            &format!("{:.6}", clip.in_point),
            "-t",
            &format!("{dur:.6}"),
            "-i",
        ])
        .arg(path)
        .args(["-an", "-vf", &vf, "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        let mut child = c
            .spawn()
            .map_err(|e| format!("Não foi possível iniciar o decodificador: {e}"))?;
        let out = child.stdout.take().ok_or("Sem saída do decodificador.")?;
        Ok(Decoder {
            child,
            out,
            last: None,
            done: false,
        })
    }
}

impl FrameProvider for ExportProvider<'_> {
    fn clip_image(
        &mut self,
        p: &VideoProject,
        clip: &Clip,
        _frame: ProjectFrame,
    ) -> Option<Arc<Vec<u8>>> {
        match &clip.source {
            ClipSource::Still { w, h, pixels } => {
                if let Some(v) = self.stills.get(&clip.id) {
                    return Some(v.clone());
                }
                let f = Arc::new(fit_still(pixels.as_ref()?, *w, *h, self.w, self.h));
                self.stills.insert(clip.id, f.clone());
                Some(f)
            }
            ClipSource::Media { path, meta, .. } if meta.has_video => {
                if !self.decoders.contains_key(&clip.id) {
                    match self.spawn(p, clip, path) {
                        Ok(d) => {
                            self.decoders.insert(clip.id, d);
                        }
                        Err(e) => {
                            self.error = Some(e);
                            return None;
                        }
                    }
                }
                let size = self.w as usize * self.h as usize * 4;
                let d = self.decoders.get_mut(&clip.id)?;
                if !d.done {
                    let mut buf = vec![0u8; size];
                    match d.out.read_exact(&mut buf) {
                        Ok(()) => d.last = Some(Arc::new(buf)),
                        Err(_) => d.done = true, // fim da mídia: repete o último quadro
                    }
                }
                d.last.clone()
            }
            ClipSource::Media { .. } => None,
        }
    }
}

/// Troca o projeto de vídeo do documento numa única ação de histórico.
pub struct VideoEdit {
    label: String,
    before: Option<VideoProject>,
    after: Option<VideoProject>,
}

impl VideoEdit {
    pub fn new(label: &str, before: Option<VideoProject>, after: Option<VideoProject>) -> Self {
        Self {
            label: label.to_string(),
            before,
            after,
        }
    }
}

impl crate::history::Command for VideoEdit {
    fn label(&self) -> &str {
        &self.label
    }
    fn apply(&mut self, doc: &mut crate::document::Document) {
        doc.video = self.after.clone();
    }
    fn revert(&mut self, doc: &mut crate::document::Document) {
        doc.video = self.before.clone();
    }
    fn bytes(&self) -> usize {
        let still = |p: &Option<VideoProject>| -> usize {
            p.iter()
                .flat_map(|p| p.tracks.iter().flat_map(|t| &t.clips))
                .map(|c| match &c.source {
                    ClipSource::Still {
                        pixels: Some(px), ..
                    } => px.len(),
                    _ => 0,
                })
                .sum()
        };
        still(&self.before) + still(&self.after)
    }
}

fn wait_cancellable(
    child: &mut Child,
    cancel: &AtomicBool,
) -> Result<std::process::ExitStatus, ExportError> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ExportError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(s)) => return Ok(s),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(15)),
            Err(e) => return Err(ExportError::Failed(e.to_string())),
        }
    }
}

fn read_log(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .chars()
        .take(1500)
        .collect()
}

/// Exporta o projeto. `progress(quadro, total)` é chamado a cada quadro; `cancel` interrompe e
/// remove o arquivo parcial. O áudio é misturado em um WAV temporário e multiplexado ao vídeo.
pub fn export(
    ff: &Ffmpeg,
    p: &VideoProject,
    s: &ExportSettings,
    out: &Path,
    progress: &mut dyn FnMut(u32, u32),
    cancel: &AtomicBool,
) -> Result<ExportReport, ExportError> {
    let fail = |m: String| ExportError::Failed(m);
    if !s.codec.available(ff) {
        return Err(fail(format!(
            "O FFmpeg instalado não tem o codificador para {}.",
            s.codec.extension()
        )));
    }
    if s.width < 16
        || s.height < 16
        || !s.width.is_multiple_of(2)
        || !s.height.is_multiple_of(2)
        || s.width > 7680
        || s.height > 4320
    {
        return Err(fail(
            "A resolução deve ter largura e altura pares entre 16 e 7680×4320.".into(),
        ));
    }
    if s.fps_num == 0 || s.fps_den == 0 || s.fps_num > 240 * s.fps_den {
        return Err(fail("Taxa de quadros inválida.".into()));
    }
    let missing = p.missing_media();
    if !missing.is_empty() {
        return Err(fail(format!(
            "Mídia ausente: {}. Localize os arquivos antes de exportar.",
            missing.join(", ")
        )));
    }
    // Exporta com a taxa e o tamanho escolhidos (os tempos dos clipes em segundos são preservados).
    let mut proj = p.clone();
    let scale_t = (f64::from(s.fps_num) / f64::from(s.fps_den)) / p.fps();
    if (scale_t - 1.0).abs() > 1e-9 {
        for c in proj.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()) {
            c.start = (f64::from(c.start) * scale_t).round() as u32;
            c.duration = ((f64::from(c.duration) * scale_t).round() as u32).max(1);
            c.fade_in = (f64::from(c.fade_in) * scale_t).round() as u32;
            c.fade_out = (f64::from(c.fade_out) * scale_t).round() as u32;
        }
        proj.fps_num = s.fps_num;
        proj.fps_den = s.fps_den;
    }
    proj.width = s.width;
    proj.height = s.height;
    let total = proj.total_frames();
    if total == 0 {
        return Err(fail("O projeto está vazio.".into()));
    }
    let total_secs = proj.duration_secs();
    let tmp =
        std::env::temp_dir().join(format!("orvalith_export_{}_{}", std::process::id(), total));
    std::fs::create_dir_all(&tmp).map_err(|e| fail(e.to_string()))?;
    let cleanup = |tmp: &Path| {
        let _ = std::fs::remove_dir_all(tmp);
    };
    let wav = tmp.join("audio.wav");
    let log = tmp.join("encoder.log");
    // 1) Áudio.
    let mut has_audio = false;
    if let Some(plan) = audio_plan(&proj) {
        let alog = tmp.join("audio.log");
        let f = std::fs::File::create(&alog).map_err(|e| fail(e.to_string()))?;
        let mut c = ff.command();
        c.arg("-y")
            .args(&plan.inputs)
            .args([
                "-filter_complex",
                &plan.filter,
                "-map",
                "[aout]",
                "-t",
                &format!("{total_secs:.6}"),
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&wav);
        c.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(f));
        let mut child = c.spawn().map_err(|e| fail(format!("ffmpeg: {e}")))?;
        let st = wait_cancellable(&mut child, cancel).inspect_err(|_| cleanup(&tmp))?;
        if !st.success() {
            let msg = read_log(&alog);
            cleanup(&tmp);
            return Err(fail(format!("Falha ao mixar o áudio: {msg}")));
        }
        has_audio = true;
    }
    // 2) Codificador.
    let lf = std::fs::File::create(&log).map_err(|e| fail(e.to_string()))?;
    let mut c = ff.command();
    c.arg("-y").args([
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-s",
        &format!("{}x{}", s.width, s.height),
        "-r",
        &format!("{}/{}", s.fps_num, s.fps_den),
        "-i",
        "-",
    ]);
    if has_audio {
        c.arg("-i").arg(&wav);
    }
    c.args(["-map", "0:v"]);
    if has_audio {
        c.args(["-map", "1:a"]);
    }
    match s.codec {
        Codec::H264Aac => {
            c.args([
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                &s.crf.clamp(0, 51).to_string(),
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
            ]);
            if has_audio {
                c.args(["-c:a", "aac", "-b:a", "160k"]);
            }
        }
        Codec::Vp9Opus => {
            c.args([
                "-c:v",
                "libvpx-vp9",
                "-crf",
                &s.crf.clamp(0, 63).to_string(),
                "-b:v",
                "0",
                "-pix_fmt",
                "yuv420p",
            ]);
            if has_audio {
                c.args(["-c:a", "libopus", "-b:a", "128k"]);
            }
        }
    }
    c.args(["-t", &format!("{total_secs:.6}")]).arg(out);
    c.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(lf));
    let mut enc = c.spawn().map_err(|e| fail(format!("ffmpeg: {e}")))?;
    let mut stdin = enc
        .stdin
        .take()
        .ok_or_else(|| fail("Sem entrada no codificador.".into()))?;
    let mut prov = ExportProvider {
        ff,
        w: s.width,
        h: s.height,
        decoders: HashMap::new(),
        stills: HashMap::new(),
        error: None,
    };
    let mut result: Result<(), ExportError> = Ok(());
    for f in 0..total {
        if cancel.load(Ordering::Relaxed) {
            result = Err(ExportError::Cancelled);
            break;
        }
        let frame = compose_frame(&proj, f, &mut prov);
        if let Some(e) = prov.error.take() {
            result = Err(fail(e));
            break;
        }
        // Libera decodificadores de clipes que já terminaram.
        let active: Vec<u64> = proj
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| f + 1 < c.end())
            .map(|c| c.id)
            .collect();
        prov.decoders.retain(|id, _| active.contains(id));
        if stdin.write_all(&frame).is_err() {
            result = Err(fail(format!(
                "O codificador encerrou antes do fim: {}",
                read_log(&log)
            )));
            break;
        }
        progress(f + 1, total);
    }
    drop(stdin);
    prov.decoders.clear();
    let status = match &result {
        Ok(()) => wait_cancellable(&mut enc, cancel),
        Err(_) => {
            let _ = enc.kill();
            let _ = enc.wait();
            Ok(std::process::ExitStatus::default())
        }
    };
    let final_result = match (result, status) {
        (Err(e), _) | (Ok(()), Err(e)) => Err(e),
        (Ok(()), Ok(st)) if !st.success() => {
            Err(fail(format!("Falha na codificação: {}", read_log(&log))))
        }
        _ => Ok(()),
    };
    cleanup(&tmp);
    match final_result {
        Ok(()) => Ok(ExportReport {
            frames: total,
            duration_s: total_secs,
            has_audio,
        }),
        Err(e) => {
            let _ = std::fs::remove_file(out);
            Err(e)
        }
    }
}

/// Caminho de saída livre (nunca sobrescreve): `nome.ext`, `nome (2).ext`…
pub fn free_output(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::testutil::*;

    fn project() -> VideoProject {
        VideoProject {
            width: 160,
            height: 90,
            fps_num: 10,
            fps_den: 1,
            ..VideoProject::default()
        }
    }

    fn meta(secs: f64) -> MediaInfo {
        MediaInfo {
            duration_s: secs,
            format: "mp4".into(),
            video: Some(crate::media::VideoStream {
                codec: "h264".into(),
                width: 160,
                height: 90,
                fps: (10, 1),
                frames: None,
                pix_fmt: "yuv420p".into(),
            }),
            audio: Some(crate::media::AudioStream {
                codec: "aac".into(),
                sample_rate: 48000,
                channels: 1,
                duration_s: secs,
            }),
        }
    }

    #[test]
    fn project_time_media_time_and_frame_conversions_are_separate_things() {
        let mut p = project();
        let mut c = p.media_clip("a.mp4", "a", &meta(5.0), 20);
        c.in_point = 1.5;
        assert_eq!(c.duration, 50);
        // Quadro 25 do projeto = 0,5 s depois do início do clipe = 2,0 s no carimbo da mídia.
        assert!((p.source_time(&c, 25) - 2.0).abs() < 1e-9);
        assert_eq!(p.secs_to_frame(2.5), 25);
        assert!((p.frame_to_secs(25) - 2.5).abs() < 1e-9);
        assert_eq!(c.end(), 70);
        let mut ntsc = VideoProject {
            fps_num: 30000,
            fps_den: 1001,
            ..p.clone()
        };
        assert!((ntsc.fps() - 29.97).abs() < 0.001);
        ntsc.sanitize();
        assert_eq!(ntsc.fps_den, 1001);
    }

    #[test]
    fn trimming_moving_splitting_respect_media_limits() {
        let mut p = project();
        let c = p.media_clip("a.mp4", "a", &meta(5.0), 10); // 50 quadros, [10,60)
        let id = c.id;
        p.add_clip(0, c).unwrap();
        // Aparar o fim não passa do que resta de mídia.
        p.trim_end(id, 400);
        assert_eq!(p.clip(id).unwrap().duration, 50);
        p.trim_end(id, 40);
        assert_eq!(p.clip(id).unwrap().end(), 40);
        // Aparar o início avança o ponto de entrada.
        p.trim_start(id, 20);
        let c = p.clip(id).unwrap();
        assert_eq!((c.start, c.duration), (20, 20));
        assert!((c.in_point - 1.0).abs() < 1e-9, "10 quadros a 10 fps = 1 s");
        // Estender para trás só até o começo da mídia (in_point 1 s = 10 quadros).
        p.trim_start(id, 0);
        let c = p.clip(id).unwrap();
        assert_eq!((c.start, c.in_point), (10, 0.0));
        // Cortar em dois clipes contíguos.
        let right = p.split(id, 25).unwrap();
        let (l, r) = (p.clip(id).unwrap().clone(), p.clip(right).unwrap().clone());
        assert_eq!((l.start, l.end(), r.start, r.end()), (10, 25, 25, 40));
        assert!((r.in_point - 1.5).abs() < 1e-9);
        assert!(p.split(id, 10).is_none() && p.split(id, 999).is_none());
        // Mover.
        p.move_clip(right, 100);
        assert_eq!(p.clip(right).unwrap().start, 100);
        assert_eq!(p.total_frames(), 115);
        assert!(p.remove_clip(right) && !p.remove_clip(right));
        // Imagens podem ser estendidas livremente.
        let s = p.still_clip("t", 4, 4, vec![255; 64], 0, 2.0);
        let sid = s.id;
        p.add_clip(1, s).unwrap();
        p.trim_end(sid, 500);
        assert_eq!(p.clip(sid).unwrap().duration, 500);
        // Trilhas aceitam só o tipo certo de mídia.
        let mut silent = meta(1.0);
        silent.audio = None;
        let sc = p.media_clip("s.mp4", "s", &silent, 0);
        assert!(p.add_clip(2, sc).is_err());
    }

    #[test]
    fn fades_dissolve_and_ramps() {
        let mut p = project();
        let a = p.media_clip("a.mp4", "a", &meta(3.0), 0); // [0,30)
        let b = p.media_clip("b.mp4", "b", &meta(3.0), 40);
        let (ia, ib) = (a.id, b.id);
        p.add_clip(0, a).unwrap();
        p.add_clip(0, b).unwrap();
        p.dissolve_with_previous(ib, 10).unwrap();
        let (a, b) = (p.clip(ia).unwrap(), p.clip(ib).unwrap());
        assert_eq!(b.start, 20, "começa 10 quadros antes do fim de A");
        assert_eq!(b.fade_in, 10);
        assert!(ramp(b, 20) < ramp(b, 25) && ramp(b, 25) < ramp(b, 30));
        assert_eq!(ramp(b, 31), 1.0);
        assert_eq!(ramp(a, 5), 1.0);
        p.set_fade(ia, 0, 5);
        let a = p.clip(ia).unwrap();
        assert!(ramp(a, 29) < ramp(a, 26) && ramp(a, 20) == 1.0);
        assert!(
            p.dissolve_with_previous(ia, 5).is_err(),
            "o primeiro clipe não tem anterior"
        );
    }

    #[test]
    fn compose_layers_titles_over_video_with_opacity_adjustment_and_transform() {
        struct Fake;
        impl FrameProvider for Fake {
            fn clip_image(
                &mut self,
                p: &VideoProject,
                c: &Clip,
                _f: ProjectFrame,
            ) -> Option<Arc<Vec<u8>>> {
                let col = if c.name == "fundo" {
                    [0, 0, 200, 255]
                } else {
                    [255, 255, 0, 255]
                };
                let mut v = Vec::new();
                for y in 0..p.height {
                    for x in 0..p.width {
                        // O título só ocupa o canto superior esquerdo; o resto transparente.
                        let on = c.name == "fundo" || (x < 20 && y < 10);
                        v.extend_from_slice(&if on { col } else { [0, 0, 0, 0] });
                    }
                }
                Some(Arc::new(v))
            }
        }
        let mut p = project();
        let mut bg = p.still_clip("fundo", 1, 1, vec![0, 0, 200, 255], 0, 2.0);
        bg.name = "fundo".into();
        let mut title = p.still_clip("titulo", 1, 1, vec![255, 255, 0, 255], 0, 2.0);
        title.opacity = 1.0;
        let tid = title.id;
        p.add_clip(0, bg).unwrap();
        p.add_clip(1, title).unwrap();
        let px = |b: &[u8], x: usize, y: usize| {
            [
                b[(y * 160 + x) * 4],
                b[(y * 160 + x) * 4 + 1],
                b[(y * 160 + x) * 4 + 2],
            ]
        };
        let f = compose_frame(&p, 0, &mut Fake);
        assert_eq!(px(&f, 5, 5), [255, 255, 0], "título por cima");
        assert_eq!(px(&f, 100, 50), [0, 0, 200], "vídeo embaixo");
        // Fora do intervalo: só o preto.
        assert_eq!(px(&compose_frame(&p, 500, &mut Fake), 5, 5), [0, 0, 0]);
        // Opacidade 50%: mistura título e fundo.
        p.tracks[1].clips[0].opacity = 0.5;
        let m = px(&compose_frame(&p, 0, &mut Fake), 5, 5);
        assert!(m[0] > 100 && m[0] < 160 && m[2] > 80 && m[2] < 120, "{m:?}");
        // Ajuste de cor (inverter) e transformação (mover 40 px à direita).
        p.tracks[1].clips[0].opacity = 1.0;
        p.tracks[1].clips[0].adjust = Some(Adjustment::Invert);
        assert_eq!(px(&compose_frame(&p, 0, &mut Fake), 5, 5), [0, 0, 255]);
        p.tracks[1].clips[0].adjust = None;
        p.tracks[1].clips[0].x = 40.0;
        let f = compose_frame(&p, 0, &mut Fake);
        assert_eq!(px(&f, 5, 5), [0, 0, 200], "saiu do lugar");
        assert_eq!(px(&f, 45, 5), [255, 255, 0]);
        let _ = tid;
    }

    #[test]
    fn audio_plan_builds_structured_filter_with_mute_volume_fades_and_delay() {
        let mut p = project();
        let mut a = p.media_clip("um.mp4", "a", &meta(2.0), 0);
        a.volume = 0.5;
        a.fade_in = 5;
        a.fade_out = 5;
        let mut b = p.media_clip("dois.mp4", "b", &meta(2.0), 30);
        b.in_point = 0.5;
        b.duration = 10;
        p.add_clip(0, a).unwrap();
        p.add_clip(0, b).unwrap();
        let plan = audio_plan(&p).unwrap();
        assert_eq!(plan.count, 2);
        let args: Vec<String> = plan
            .inputs
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert_eq!(&args[..4], &["-ss", "0.000000", "-t", "2.000000"]);
        assert!(args.contains(&"dois.mp4".to_string()) && args.contains(&"0.500000".to_string()));
        assert!(
            plan.filter.contains("volume=0.5000")
                && plan.filter.contains("afade=t=in:st=0:d=0.500000")
        );
        assert!(plan.filter.contains("afade=t=out:st=1.500000:d=0.500000"));
        assert!(
            plan.filter.contains("adelay=3000|3000[a1]"),
            "{}",
            plan.filter
        );
        assert!(plan.filter.contains("amix=inputs=2:normalize=0"));
        p.tracks[0].mute = true;
        assert!(audio_plan(&p).is_none(), "trilha silenciada");
    }

    #[test]
    fn relative_paths_relink_and_sanitize() {
        let base = Path::new("C:/proj/sub");
        assert_eq!(
            relative_to(Path::new("C:/proj/sub/midia/a.mp4"), base).as_deref(),
            Some("midia/a.mp4")
        );
        assert_eq!(
            relative_to(Path::new("C:/proj/outra/a.mp4"), base).as_deref(),
            Some("../outra/a.mp4")
        );
        assert_eq!(relative_to(Path::new("D:/x/a.mp4"), base), None);
        let dir = std::env::temp_dir().join(format!("orvalith_rel_{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("media"));
        std::fs::write(dir.join("media/a.mp4"), b"x").unwrap();
        let mut p = project();
        let c = p.media_clip(
            &dir.join("media/a.mp4").to_string_lossy(),
            "a",
            &meta(1.0),
            0,
        );
        p.add_clip(0, c).unwrap();
        p.relativize(&dir);
        assert!(
            matches!(&p.tracks[0].clips[0].source, ClipSource::Media { rel: Some(r), .. } if r == "media/a.mp4")
        );
        // Projeto "movido": o caminho absoluto some, o relativo resolve na nova pasta.
        let moved = std::env::temp_dir().join(format!("orvalith_rel_moved_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&moved);
        std::fs::rename(&dir, &moved).unwrap();
        assert_eq!(p.missing_media().len(), 1);
        assert!(p.resolve_media(Some(&moved)).is_empty());
        assert!(Path::new(p.tracks[0].clips[0].path().unwrap()).exists());
        // Mídia realmente ausente + relink.
        std::fs::remove_dir_all(&moved).unwrap();
        let missing = p.resolve_media(Some(&moved));
        assert_eq!(missing.len(), 1);
        let new = std::env::temp_dir().join(format!("orvalith_relink_{}.mp4", std::process::id()));
        std::fs::write(&new, b"y").unwrap();
        assert_eq!(p.relink(&missing[0], &new.to_string_lossy(), &meta(3.0)), 1);
        assert!(p.missing_media().is_empty());
        let _ = std::fs::remove_file(new);
        // Valores hostis.
        p.width = 7;
        p.fps_num = 0;
        p.tracks[0].clips[0].scale = f32::NAN;
        p.tracks[0].clips[0].duration = 0;
        p.sanitize();
        assert_eq!((p.width, p.fps_num), (16, 1));
        assert_eq!(p.tracks[0].clips[0].scale, 1.0);
        assert_eq!(p.tracks[0].clips[0].duration, 1);
    }

    #[test]
    fn acceptance_two_clips_trim_title_dissolve_and_audio_export_verified_with_ffprobe() {
        let Some(ff) = ffmpeg() else { return };
        if !Codec::H264Aac.available(&ff) {
            return;
        }
        let dir = temp_dir("export");
        let (ra, rb) = (dir.join("vermelho.mp4"), dir.join("azul.mp4"));
        make_clip(&ff, &ra, "red", 3.0, 320, 180, 25, Some(440));
        make_clip(&ff, &rb, "blue", 3.0, 320, 180, 25, Some(880));
        let mut p = VideoProject {
            width: 320,
            height: 180,
            fps_num: 25,
            fps_den: 1,
            ..VideoProject::default()
        };
        let ia = ff.probe(&ra).unwrap();
        let ib = ff.probe(&rb).unwrap();
        let a = p.media_clip(&ra.to_string_lossy(), "vermelho", &ia, 0);
        let b = p.media_clip(&rb.to_string_lossy(), "azul", &ib, 0);
        let (ida, idb) = (a.id, b.id);
        p.add_clip(0, a).unwrap();
        p.add_clip(0, b).unwrap();
        // Aparar arrastando: A fica com 2 s (do início), B com 2 s a partir de 0,5 s.
        p.trim_end(ida, 50);
        p.trim_end(idb, 50);
        p.move_clip(idb, 50);
        p.trim_start(idb, 62); // entrada em 0,48 s
        p.trim_end(idb, 112);
        // Dissolver de 10 quadros e título (retângulo branco) por cima do segundo 1 ao 2.
        p.dissolve_with_previous(idb, 10).unwrap();
        let mut px = vec![0u8; 320 * 180 * 4];
        for y in 10..40 {
            for x in 10..110 {
                px[(y * 320 + x) * 4..(y * 320 + x) * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let title = p.still_clip("Título", 320, 180, px, 25, 1.0);
        p.add_clip(1, title).unwrap();
        let total = p.total_frames();
        assert_eq!(
            total, 90,
            "A [0,50) + B [40,90) com dissolver de 10 quadros: {total}"
        );
        let out = dir.join("saida.mp4");
        let mut seen = 0;
        let cancel = AtomicBool::new(false);
        let rep = export(
            &ff,
            &p,
            &ExportSettings::from_project(&p),
            &out,
            &mut |f, t| {
                seen = f;
                assert!(f <= t);
            },
            &cancel,
        )
        .unwrap();
        assert_eq!((seen, rep.frames), (total, total));
        assert!(rep.has_audio);
        // Verificação independente com o ffprobe.
        let info = ff.probe(&out).unwrap();
        let v = info.video.as_ref().unwrap();
        assert_eq!((v.width, v.height, v.fps), (320, 180, (25, 1)));
        assert!(
            (info.duration_s - 3.6).abs() < 0.12,
            "duração {}",
            info.duration_s
        );
        let au = info.audio.as_ref().expect("áudio presente");
        assert!(
            (au.duration_s - info.duration_s).abs() < 0.12,
            "áudio {} vs vídeo {}",
            au.duration_s,
            info.duration_s
        );
        // Conteúdo dos quadros: vermelho no início, azul no fim, título no meio, mistura no dissolver.
        let at = |t: f64, x: usize, y: usize| {
            let f = ff.frame_at(&out, t, 320, 180).unwrap();
            let i = (y * 320 + x) * 4;
            [f[i], f[i + 1], f[i + 2]]
        };
        let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(&b).all(|(x, y)| x.abs_diff(*y) < 40);
        assert!(
            near(at(0.2, 200, 120), [255, 0, 0]),
            "{:?}",
            at(0.2, 200, 120)
        );
        assert!(
            near(at(3.0, 200, 120), [0, 0, 255]),
            "{:?}",
            at(3.0, 200, 120)
        );
        assert!(
            near(at(1.2, 50, 25), [255, 255, 255]),
            "título visível: {:?}",
            at(1.2, 50, 25)
        );
        assert!(
            !near(at(0.4, 50, 25), [255, 255, 255]),
            "título ausente antes do tempo"
        );
        let mid = at(1.8, 200, 120); // dentro do dissolver [1,6 s; 2,0 s)
        assert!(mid[0] > 40 && mid[2] > 40, "mistura vermelho+azul: {mid:?}");
        // Cancelar remove o arquivo parcial.
        let out2 = dir.join("cancelado.mp4");
        let stop = AtomicBool::new(false);
        let r = export(
            &ff,
            &p,
            &ExportSettings::from_project(&p),
            &out2,
            &mut |f, _| {
                if f == 5 {
                    stop.store(true, Ordering::Relaxed);
                }
            },
            &stop,
        );
        assert_eq!(r.unwrap_err(), ExportError::Cancelled);
        assert!(!out2.exists(), "arquivo parcial removido");
        // Mídia ausente e resolução inválida são recusadas com mensagem clara.
        let mut broken = p.clone();
        broken.tracks[0].clips[0].source = ClipSource::Media {
            path: dir.join("sumiu.mp4").to_string_lossy().into_owned(),
            rel: None,
            meta: MediaMeta::from_info(&ia),
        };
        let e = export(
            &ff,
            &broken,
            &ExportSettings::from_project(&broken),
            &dir.join("x.mp4"),
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(e.to_string().contains("ausente"));
        let mut bad = ExportSettings::from_project(&p);
        bad.width = 321;
        assert!(export(
            &ff,
            &p,
            &bad,
            &dir.join("y.mp4"),
            &mut |_, _| {},
            &AtomicBool::new(false)
        )
        .is_err());
        // Alternativa aberta: WebM VP9 + Opus, com outra resolução e taxa.
        if Codec::Vp9Opus.available(&ff) {
            let webm = dir.join("saida.webm");
            let s = ExportSettings {
                width: 160,
                height: 90,
                fps_num: 10,
                fps_den: 1,
                codec: Codec::Vp9Opus,
                crf: 40,
            };
            export(&ff, &p, &s, &webm, &mut |_, _| {}, &AtomicBool::new(false)).unwrap();
            let wi = ff.probe(&webm).unwrap();
            assert_eq!(
                (
                    wi.video.as_ref().unwrap().width,
                    wi.video.as_ref().unwrap().fps
                ),
                (160, (10, 1))
            );
            assert!((wi.duration_s - 3.6).abs() < 0.3, "{}", wi.duration_s);
            assert!(wi.audio.is_some());
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
