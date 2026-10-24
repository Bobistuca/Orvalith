//! Acesso isolado ao FFmpeg/ffprobe (processos externos, com argumentos estruturados).
//!
//! O Orvalith **não embute nem vincula** o FFmpeg: ele só executa os programas `ffmpeg` e
//! `ffprobe` instalados no computador (procurados em `ORVALITH_FFMPEG_DIR`, numa pasta
//! configurada ou no `PATH`). A licença do FFmpeg (LGPL/GPL conforme a compilação instalada)
//! vale para o programa externo, não para o Orvalith; veja docs/MEDIA.md. Nada é enviado à rede.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde::Deserialize;

/// Limite do que se lê do `ffprobe` (JSON) para evitar saídas absurdas.
const MAX_PROBE_BYTES: usize = 8 << 20;

#[cfg(windows)]
fn quiet(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
}

#[cfg(not(windows))]
fn quiet(cmd: &mut Command) -> &mut Command {
    cmd
}

/// Instalação detectada do FFmpeg.
#[derive(Debug, Clone)]
pub struct Ffmpeg {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub version: String,
    /// Compilação com GPL/nonfree habilitado (informativo para a licença).
    pub gpl: bool,
    pub h264: bool,
    pub aac: bool,
    pub vp9: bool,
    pub opus: bool,
}

impl Ffmpeg {
    /// Procura o FFmpeg: `ORVALITH_FFMPEG_DIR`, a pasta `extra` e o `PATH`.
    pub fn detect(extra: Option<&Path>) -> Result<Ffmpeg, String> {
        let mut candidates: Vec<(PathBuf, PathBuf)> = Vec::new();
        let exe = |n: &str| {
            if cfg!(windows) {
                format!("{n}.exe")
            } else {
                n.to_string()
            }
        };
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Ok(d) = std::env::var("ORVALITH_FFMPEG_DIR") {
            dirs.push(PathBuf::from(d));
        }
        if let Some(d) = extra {
            dirs.push(d.to_path_buf());
        }
        for d in dirs {
            candidates.push((d.join(exe("ffmpeg")), d.join(exe("ffprobe"))));
        }
        candidates.push((PathBuf::from("ffmpeg"), PathBuf::from("ffprobe")));
        let mut last_err = "FFmpeg não encontrado.".to_string();
        for (ff, fp) in candidates {
            match Self::probe_install(&ff, &fp) {
                Ok(f) => return Ok(f),
                Err(e) => last_err = e,
            }
        }
        Err(format!(
            "{last_err} Instale o FFmpeg (por exemplo: winget install Gyan.FFmpeg) ou defina ORVALITH_FFMPEG_DIR; veja docs/MEDIA.md."
        ))
    }

    fn probe_install(ff: &Path, fp: &Path) -> Result<Ffmpeg, String> {
        let out =
            run_capture(ff, &["-hide_banner", "-version"]).map_err(|e| format!("ffmpeg: {e}"))?;
        let first = out.lines().next().unwrap_or("");
        let version = first.split_whitespace().nth(2).unwrap_or("?").to_string();
        let gpl = out.contains("--enable-gpl") || out.contains("--enable-nonfree");
        run_capture(fp, &["-hide_banner", "-version"]).map_err(|e| format!("ffprobe: {e}"))?;
        let enc = run_capture(ff, &["-hide_banner", "-encoders"]).unwrap_or_default();
        let has = |name: &str| {
            enc.lines()
                .any(|l| l.split_whitespace().nth(1) == Some(name))
        };
        Ok(Ffmpeg {
            ffmpeg: ff.to_path_buf(),
            ffprobe: fp.to_path_buf(),
            version,
            gpl,
            h264: has("libx264"),
            aac: has("aac"),
            vp9: has("libvpx-vp9"),
            opus: has("libopus"),
        })
    }

    pub fn summary(&self) -> String {
        format!(
            "FFmpeg {} ({}) — H.264 {}, AAC {}, VP9 {}, Opus {}",
            self.version,
            if self.gpl {
                "compilação GPL"
            } else {
                "compilação LGPL"
            },
            yes(self.h264),
            yes(self.aac),
            yes(self.vp9),
            yes(self.opus)
        )
    }

    /// Cria um comando `ffmpeg` com argumentos estruturados (sem shell).
    pub fn command(&self) -> Command {
        let mut c = Command::new(&self.ffmpeg);
        c.args(["-hide_banner", "-nostdin", "-v", "error"]);
        quiet(&mut c);
        c
    }
}

fn yes(b: bool) -> &'static str {
    if b {
        "sim"
    } else {
        "não"
    }
}

fn run_capture(prog: &Path, args: &[&str]) -> Result<String, String> {
    let mut c = Command::new(prog);
    c.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    quiet(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Metadados reais lidos do `ffprobe`.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    pub duration_s: f64,
    pub format: String,
    pub video: Option<VideoStream>,
    pub audio: Option<AudioStream>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoStream {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    /// Taxa de quadros média (numerador, denominador).
    pub fps: (u32, u32),
    pub frames: Option<u64>,
    pub pix_fmt: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioStream {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub duration_s: f64,
}

impl VideoStream {
    pub fn fps_f64(&self) -> f64 {
        if self.fps.1 == 0 {
            0.0
        } else {
            f64::from(self.fps.0) / f64::from(self.fps.1)
        }
    }
}

#[derive(Deserialize)]
struct ProbeOut {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    format_name: Option<String>,
    duration: Option<String>,
}

#[derive(Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    nb_frames: Option<String>,
    pix_fmt: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    duration: Option<String>,
}

fn parse_rate(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_once('/')?;
    let (a, b): (u32, u32) = (a.parse().ok()?, b.parse().ok()?);
    (a > 0 && b > 0).then_some((a, b))
}

impl Ffmpeg {
    /// Lê os metadados reais de um arquivo (duração, codecs, dimensões, taxa de quadros, áudio).
    pub fn probe(&self, path: &Path) -> Result<MediaInfo, String> {
        let mut c = Command::new(&self.ffprobe);
        c.args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        quiet(&mut c);
        let out = c.output().map_err(|e| format!("ffprobe: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "ffprobe falhou: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        if out.stdout.len() > MAX_PROBE_BYTES {
            return Err("Saída do ffprobe grande demais.".into());
        }
        parse_probe(&out.stdout)
    }

    /// Extrai um quadro (RGBA) no instante `t_s` do arquivo, ajustado a `w × h` (com transparência nas bordas).
    pub fn frame_at(&self, path: &Path, t_s: f64, w: u32, h: u32) -> Result<Vec<u8>, String> {
        let vf = scale_filter(w, h);
        let mut c = self.command();
        c.args(["-ss", &format!("{:.6}", t_s.max(0.0)), "-i"])
            .arg(path)
            .args([
                "-frames:v",
                "1",
                "-an",
                "-vf",
                &vf,
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let out = c.output().map_err(|e| format!("ffmpeg: {e}"))?;
        let need = w as usize * h as usize * 4;
        if out.stdout.len() < need {
            return Err(format!(
                "Não foi possível extrair o quadro: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(out.stdout[..need].to_vec())
    }
}

/// Filtro que encaixa o vídeo em `w × h` mantendo a proporção, com bordas transparentes.
pub fn scale_filter(w: u32, h: u32) -> String {
    format!("scale={w}:{h}:force_original_aspect_ratio=decrease,format=rgba,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black@0")
}

pub fn parse_probe(json: &[u8]) -> Result<MediaInfo, String> {
    let p: ProbeOut =
        serde_json::from_slice(json).map_err(|e| format!("Saída do ffprobe inválida: {e}"))?;
    let num = |s: &Option<String>| {
        s.as_deref()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite())
    };
    let video = p
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"))
        .and_then(|s| {
            // Imagens de capa embutidas (attached_pic) não contam, mas o ffprobe as lista como vídeo de 1 quadro.
            Some(VideoStream {
                codec: s.codec_name.clone()?,
                width: s.width.filter(|v| *v > 0)?,
                height: s.height.filter(|v| *v > 0)?,
                fps: s
                    .avg_frame_rate
                    .as_deref()
                    .and_then(parse_rate)
                    .or_else(|| s.r_frame_rate.as_deref().and_then(parse_rate))
                    .unwrap_or((30, 1)),
                frames: s.nb_frames.as_deref().and_then(|v| v.parse().ok()),
                pix_fmt: s.pix_fmt.clone().unwrap_or_default(),
            })
        });
    let audio = p
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"))
        .and_then(|s| {
            Some(AudioStream {
                codec: s.codec_name.clone()?,
                sample_rate: s
                    .sample_rate
                    .as_deref()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                channels: s.channels.unwrap_or(0),
                duration_s: num(&s.duration).unwrap_or(0.0),
            })
        });
    if video.is_none() && audio.is_none() {
        return Err("O arquivo não tem fluxo de vídeo nem de áudio reconhecível.".into());
    }
    let duration_s = p
        .format
        .as_ref()
        .and_then(|f| num(&f.duration))
        .or_else(|| audio.as_ref().map(|a| a.duration_s))
        .unwrap_or(0.0);
    Ok(MediaInfo {
        duration_s,
        format: p.format.and_then(|f| f.format_name).unwrap_or_default(),
        video,
        audio,
    })
}

/// Cache simples de quadros extraídos (chave: caminho + instante em milissegundos + tamanho).
pub struct FrameCache {
    cap: usize,
    map: HashMap<(PathBuf, u64, u32, u32), Arc<Vec<u8>>>,
    order: VecDeque<(PathBuf, u64, u32, u32)>,
}

impl FrameCache {
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Ajusta a capacidade, descartando os quadros mais antigos se necessário.
    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap.max(1);
        while self.map.len() > self.cap {
            match self.order.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                }
                None => break,
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Devolve o quadro do cache ou o extrai (e guarda), descartando o mais antigo se estiver cheio.
    pub fn get(
        &mut self,
        ff: &Ffmpeg,
        path: &Path,
        t_s: f64,
        w: u32,
        h: u32,
    ) -> Result<Arc<Vec<u8>>, String> {
        let key = (
            path.to_path_buf(),
            (t_s * 1000.0).round().max(0.0) as u64,
            w,
            h,
        );
        if let Some(v) = self.map.get(&key) {
            return Ok(v.clone());
        }
        let frame = Arc::new(ff.frame_at(path, t_s, w, h)?);
        if self.map.len() >= self.cap {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, frame.clone());
        Ok(frame)
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// FFmpeg para testes; `None` (e os testes são pulados) se não estiver instalado.
    pub fn ffmpeg() -> Option<Ffmpeg> {
        match Ffmpeg::detect(None) {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!("FFmpeg ausente, teste pulado: {e}");
                None
            }
        }
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("orvalith_{name}_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    /// Gera um clipe de teste (cor sólida + tom senoidal opcional).
    #[allow(clippy::too_many_arguments)]
    pub fn make_clip(
        ff: &Ffmpeg,
        path: &Path,
        color: &str,
        secs: f32,
        w: u32,
        h: u32,
        fps: u32,
        audio_hz: Option<u32>,
    ) {
        let mut c = ff.command();
        c.args(["-y", "-f", "lavfi", "-i"])
            .arg(format!("color=c={color}:s={w}x{h}:r={fps}:d={secs}"));
        if let Some(hz) = audio_hz {
            c.args(["-f", "lavfi", "-i"]).arg(format!(
                "sine=frequency={hz}:sample_rate=48000:duration={secs}"
            ));
        }
        c.args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "5"]);
        if audio_hz.is_some() {
            c.args(["-c:a", "aac", "-b:a", "128k"]);
        }
        c.arg(path);
        let out = c.output().expect("ffmpeg executa");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    #[test]
    fn detects_the_install_and_reports_codecs_and_license_class() {
        let Some(ff) = ffmpeg() else { return };
        assert!(!ff.version.is_empty() && ff.version != "?");
        assert!(ff.summary().contains("FFmpeg"));
        assert!(
            Ffmpeg::detect(Some(Path::new("Z:/nao/existe"))).is_ok(),
            "cai no PATH"
        );
    }

    #[test]
    fn probe_reads_real_metadata_and_frames_are_extracted_and_cached() {
        let Some(ff) = ffmpeg() else { return };
        let dir = temp_dir("probe");
        let clip = dir.join("a.mp4");
        make_clip(&ff, &clip, "red", 2.0, 160, 90, 25, Some(440));
        let info = ff.probe(&clip).unwrap();
        assert!((info.duration_s - 2.0).abs() < 0.15, "{}", info.duration_s);
        let v = info.video.as_ref().unwrap();
        assert_eq!(
            (v.width, v.height, v.fps, v.codec.as_str()),
            (160, 90, (25, 1), "h264")
        );
        let a = info.audio.as_ref().unwrap();
        assert_eq!(
            (a.sample_rate, a.channels, a.codec.as_str()),
            (48000, 1, "aac")
        );
        let f = ff.frame_at(&clip, 1.0, 80, 45).unwrap();
        assert_eq!(f.len(), 80 * 45 * 4);
        let p = &f[(22 * 80 + 40) * 4..][..4];
        assert!(p[0] > 200 && p[1] < 60 && p[2] < 60 && p[3] == 255, "{p:?}");
        // Proporção diferente: bordas transparentes (letterbox).
        let boxed = ff.frame_at(&clip, 0.5, 90, 90).unwrap();
        assert_eq!(boxed[3], 0, "topo transparente");
        // Cache: o mesmo pedido não cria entrada nova; capacidade respeitada.
        let mut cache = FrameCache::new(2);
        for t in [0.0, 0.0, 0.5, 1.0] {
            cache.get(&ff, &clip, t, 32, 18).unwrap();
        }
        assert_eq!(cache.len(), 2);
        // Arquivo inexistente / lixo: erros claros.
        assert!(ff.probe(&dir.join("falta.mp4")).is_err());
        let junk = dir.join("lixo.mp4");
        std::fs::write(&junk, b"isto nao e video").unwrap();
        assert!(ff.probe(&junk).is_err());
        assert!(ff.frame_at(&junk, 0.0, 16, 16).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn probe_json_parsing_handles_audio_only_and_garbage() {
        let j = br#"{"streams":[{"codec_type":"audio","codec_name":"mp3","sample_rate":"44100","channels":2,"duration":"3.5"}],"format":{"format_name":"mp3","duration":"3.52"}}"#;
        let i = parse_probe(j).unwrap();
        assert!(i.video.is_none() && i.audio.is_some());
        assert!((i.duration_s - 3.52).abs() < 1e-9);
        assert!(parse_probe(b"{}").is_err());
        assert!(parse_probe(b"nao json").is_err());
        assert_eq!(parse_rate("30000/1001"), Some((30000, 1001)));
        assert_eq!(parse_rate("0/0"), None);
    }
}
