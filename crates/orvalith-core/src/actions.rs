//! Ações (macros) e processamento em lote.
//!
//! Uma [`Macro`] é uma lista de [`Step`]s serializáveis que o motor aplica a um documento, na
//! interface (um passo de desfazer) ou sem interface, em lote (arquivos). Os passos agem em
//! **todas as camadas raster comuns** (não grupos, texto/forma, objetos, ajustes nem camadas
//! bloqueadas) e ignoram a seleção; documentos sem camada compatível são recusados com mensagem.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::adjops;
use crate::adjust::Adjustment;
use crate::docops::{self, DocState, Orient};
use crate::document::{Document, Layer};
use crate::filter::{self, Filter};
use crate::formats::{self, ExportOpts, Format};
use crate::fsio;
use crate::layerops;
use crate::orva;
use crate::transform::Interp;

pub const MAX_STEPS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resample {
    Nearest,
    Bilinear,
    Bicubic,
}

impl Resample {
    fn interp(self) -> Interp {
        match self {
            Resample::Nearest => Interp::Nearest,
            Resample::Bilinear => Interp::Bilinear,
            Resample::Bicubic => Interp::Bicubic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Turn {
    Cw90,
    Ccw90,
    Rot180,
    FlipH,
    FlipV,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Step {
    Adjust(Adjustment),
    Filter(Filter),
    ResizeImage {
        width: u32,
        height: u32,
        resample: Resample,
    },
    /// Reduz (mantendo a proporção) se o maior lado passar de `max_side`; nunca amplia.
    FitWithin {
        max_side: u32,
        resample: Resample,
    },
    Orient(Turn),
    /// Une todas as camadas visíveis numa só.
    Flatten,
}

impl Step {
    pub fn describe(&self) -> String {
        match self {
            Step::Adjust(a) => format!("Ajuste: {}", a.name()),
            Step::Filter(f) => format!("Filtro: {}", f.name()),
            Step::ResizeImage { width, height, .. } => {
                format!("Tamanho da imagem: {width}×{height}")
            }
            Step::FitWithin { max_side, .. } => format!("Reduzir para no máximo {max_side} px"),
            Step::Orient(t) => format!(
                "Orientação: {}",
                match t {
                    Turn::Cw90 => "girar 90° horário",
                    Turn::Ccw90 => "girar 90° anti-horário",
                    Turn::Rot180 => "girar 180°",
                    Turn::FlipH => "espelhar horizontalmente",
                    Turn::FlipV => "espelhar verticalmente",
                }
            ),
            Step::Flatten => "Achatar imagem".into(),
        }
    }

    /// Parâmetros dentro de faixas seguras (macros vêm de arquivos de preferências editáveis).
    pub fn sanitize(&mut self) {
        match self {
            Step::Adjust(a) => a.sanitize(),
            Step::Filter(f) => f.sanitize(),
            Step::ResizeImage { width, height, .. } => {
                *width = (*width).clamp(1, Document::MAX_SIDE);
                *height = (*height).clamp(1, Document::MAX_SIDE);
            }
            Step::FitWithin { max_side, .. } => {
                *max_side = (*max_side).clamp(1, Document::MAX_SIDE)
            }
            Step::Orient(_) | Step::Flatten => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    pub name: String,
    pub steps: Vec<Step>,
}

impl Macro {
    pub fn sanitize(&mut self) {
        self.name = self.name.chars().take(80).collect();
        self.steps.truncate(MAX_STEPS);
        for s in &mut self.steps {
            s.sanitize();
        }
    }
}

fn eligible(l: &Layer) -> bool {
    !l.is_group() && adjops::destructive_blocked(l).is_none()
}

fn to_orient(t: Turn) -> Orient {
    match t {
        Turn::Cw90 => Orient::Cw90,
        Turn::Ccw90 => Orient::Ccw90,
        Turn::Rot180 => Orient::Rot180,
        Turn::FlipH => Orient::FlipH,
        Turn::FlipV => Orient::FlipV,
    }
}

/// Aplica um passo ao documento (sem histórico: o chamador decide como desfazer).
pub fn apply_step(doc: &mut Document, step: &Step) -> Result<(), String> {
    match step {
        Step::Adjust(a) => {
            let mut n = 0;
            for l in doc.layers.iter_mut().filter(|l| eligible(l)) {
                l.pixels = Arc::new(adjops::adjusted_pixels(&l.pixels, a, None));
                n += 1;
            }
            if n == 0 {
                return Err("Nenhuma camada raster compatível para o ajuste.".into());
            }
        }
        Step::Filter(f) => {
            let no = AtomicBool::new(false);
            let (w, h) = (doc.width, doc.height);
            let mut n = 0;
            for l in doc.layers.iter_mut().filter(|l| eligible(l)) {
                let out = filter::apply(f, &l.pixels, w, h, &no).ok_or("Filtro interrompido.")?;
                l.pixels = Arc::new(out);
                n += 1;
            }
            if n == 0 {
                return Err("Nenhuma camada raster compatível para o filtro.".into());
            }
        }
        Step::ResizeImage {
            width,
            height,
            resample,
        } => {
            let st = docops::resize_image(doc, *width, *height, resample.interp(), None)?;
            st.apply_to(doc);
        }
        Step::FitWithin { max_side, resample } => {
            let m = (*max_side).max(1);
            let big = doc.width.max(doc.height);
            if big > m {
                let k = m as f32 / big as f32;
                let (w, h) = (
                    ((doc.width as f32 * k).round() as u32).max(1),
                    ((doc.height as f32 * k).round() as u32).max(1),
                );
                let st = docops::resize_image(doc, w, h, resample.interp(), None)?;
                st.apply_to(doc);
            }
        }
        Step::Orient(t) => {
            let st: DocState = docops::orient_canvas(doc, to_orient(*t))?;
            st.apply_to(doc);
        }
        Step::Flatten => {
            if let Some(o) = layerops::flatten_all(doc) {
                doc.layers = o.layers;
            }
        }
    }
    doc.touch();
    Ok(())
}

/// Aplica todos os passos; para no primeiro erro (o documento pode ficar parcialmente alterado).
pub fn run(doc: &mut Document, m: &Macro) -> Result<(), String> {
    for (i, s) in m.steps.iter().enumerate() {
        apply_step(doc, s).map_err(|e| format!("Passo {} ({}): {e}", i + 1, s.describe()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Lote

#[derive(Debug, Clone)]
pub struct BatchOpts {
    pub out_dir: PathBuf,
    pub format: Format,
    pub export: ExportOpts,
    /// Acrescentado ao nome do arquivo de saída (antes da extensão).
    pub suffix: String,
    /// Opção explícita: permite gravar por cima de arquivos existentes (inclusive os originais).
    pub overwrite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Done(PathBuf),
    Failed(String),
    Skipped(String),
}

#[derive(Debug, Clone)]
pub struct FileReport {
    pub input: PathBuf,
    pub status: Status,
    pub warnings: Vec<String>,
    pub elapsed_ms: u128,
}

const MAX_INPUT_BYTES: u64 = 1 << 30;

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    p
}

fn process_one(path: &Path, m: Option<&Macro>, o: &BatchOpts) -> (Status, Vec<String>) {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if matches!(
        ext.as_str(),
        "pdf" | "mp4" | "mov" | "mkv" | "webm" | "avi" | "mp3" | "wav"
    ) {
        return (
            Status::Skipped(format!(
                ".{ext} tem fluxo próprio (importação de PDF/vídeo); não entra no lote."
            )),
            vec![],
        );
    }
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => return (Status::Failed(format!("não foi possível ler: {e}")), vec![]),
    };
    if meta.len() > MAX_INPUT_BYTES {
        return (Status::Failed("arquivo grande demais".into()), vec![]);
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return (Status::Failed(format!("não foi possível ler: {e}")), vec![]),
    };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "arquivo".into());
    let (mut doc, mut warnings) = if ext == "orva" {
        match orva::read(&bytes) {
            Ok(d) => (d, vec![]),
            Err(e) => return (Status::Failed(e.to_string()), vec![]),
        }
    } else {
        match formats::import(&bytes, &stem, &ext) {
            Ok(i) => (i.document, i.warnings),
            Err(e) => return (Status::Failed(e), vec![]),
        }
    };
    if let Some(m) = m {
        if let Err(e) = run(&mut doc, m) {
            return (Status::Failed(e), warnings);
        }
    }
    let out = match formats::export(&doc, o.format, &o.export) {
        Ok(x) => x,
        Err(e) => return (Status::Failed(e), warnings),
    };
    warnings.extend(out.warnings);
    let name = format!("{stem}{}", o.suffix);
    let ext_out = o.format.default_ext();
    let mut target = o.out_dir.join(format!("{name}.{ext_out}"));
    if same_file(&target, path) && !o.overwrite {
        return (Status::Failed("recusado: a saída sobrescreveria o arquivo original (ative a opção de sobrescrever ou escolha outra pasta/sufixo)".into()), warnings);
    }
    if target.exists() && !o.overwrite {
        target = free_path(&o.out_dir, &name, ext_out);
    }
    if let Err(e) = std::fs::create_dir_all(&o.out_dir) {
        return (Status::Failed(format!("pasta de saída: {e}")), warnings);
    }
    match fsio::atomic_write(&target, &out.bytes) {
        Ok(()) => (Status::Done(target), warnings),
        Err(e) => (Status::Failed(format!("gravação: {e}")), warnings),
    }
}

/// Processa os arquivos um a um; falhas ficam isoladas (relatório por arquivo) e `cancel` interrompe.
pub fn run_batch(
    files: &[PathBuf],
    m: Option<&Macro>,
    o: &BatchOpts,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize, &Path),
) -> Vec<FileReport> {
    let mut reports = Vec::with_capacity(files.len());
    for (i, f) in files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            for rest in &files[i..] {
                reports.push(FileReport {
                    input: rest.clone(),
                    status: Status::Skipped("cancelado".into()),
                    warnings: vec![],
                    elapsed_ms: 0,
                });
            }
            break;
        }
        progress(i, files.len(), f);
        let t = Instant::now();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process_one(f, m, o)));
        let (status, warnings) = res.unwrap_or_else(|_| {
            (
                Status::Failed("erro interno ao processar o arquivo".into()),
                vec![],
            )
        });
        reports.push(FileReport {
            input: f.clone(),
            status,
            warnings,
            elapsed_ms: t.elapsed().as_millis(),
        });
    }
    progress(files.len(), files.len(), Path::new(""));
    reports
}

/// Relatório em texto simples, um bloco por arquivo.
pub fn report_text(reports: &[FileReport]) -> String {
    let done = reports
        .iter()
        .filter(|r| matches!(r.status, Status::Done(_)))
        .count();
    let failed = reports
        .iter()
        .filter(|r| matches!(r.status, Status::Failed(_)))
        .count();
    let mut s = format!(
        "Lote: {} arquivo(s) — {done} concluído(s), {failed} com falha, {} ignorado(s)\n\n",
        reports.len(),
        reports.len() - done - failed
    );
    for r in reports {
        let (tag, detail) = match &r.status {
            Status::Done(p) => ("OK", format!("→ {}", p.display())),
            Status::Failed(e) => ("FALHA", e.clone()),
            Status::Skipped(e) => ("IGNORADO", e.clone()),
        };
        s.push_str(&format!(
            "[{tag}] {} ({} ms)\n    {detail}\n",
            r.input.display(),
            r.elapsed_ms
        ));
        for w in &r.warnings {
            s.push_str(&format!("    aviso: {w}\n"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imageio;

    fn doc() -> Document {
        let mut d = Document::new("t", 40, 20, Some([200, 100, 50, 255])).unwrap();
        d.layers[0].set_pixel(0, 0, [10, 20, 30, 255]);
        d
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("orvalith_batch_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn mac() -> Macro {
        Macro {
            name: "m".into(),
            steps: vec![
                Step::Adjust(Adjustment::Invert),
                Step::FitWithin {
                    max_side: 20,
                    resample: Resample::Bilinear,
                },
                Step::Orient(Turn::Cw90),
            ],
        }
    }

    #[test]
    fn steps_apply_to_eligible_layers_and_report_incompatible_documents() {
        let mut d = doc();
        run(&mut d, &mac()).unwrap();
        assert_eq!((d.width, d.height), (10, 20), "40×20 → 20×10 → girado 90°");
        let p = d.layers[0].pixel(5, 5).unwrap();
        assert_eq!(&p[..3], &[55, 155, 205], "cor invertida");
        // Ampliar não acontece com FitWithin; Flatten une camadas.
        let mut e = doc();
        e.layers.push(Layer::filled("b", 40, 20, [0, 0, 0, 255]));
        apply_step(
            &mut e,
            &Step::FitWithin {
                max_side: 500,
                resample: Resample::Nearest,
            },
        )
        .unwrap();
        assert_eq!(e.width, 40);
        apply_step(&mut e, &Step::Flatten).unwrap();
        assert_eq!(e.layers.len(), 1);
        // Documento sem camada compatível (só texto/forma/objetos) é recusado.
        let mut g = Document::new("g", 4, 4, None).unwrap();
        g.layers[0].lock_pixels = true;
        let err = run(
            &mut g,
            &Macro {
                name: "x".into(),
                steps: vec![Step::Filter(Filter::Sharpen)],
            },
        )
        .unwrap_err();
        assert!(
            err.contains("Passo 1") && err.contains("compatível"),
            "{err}"
        );
    }

    #[test]
    fn macros_serialize_and_hostile_values_are_sanitized() {
        let m = mac();
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Macro>(&json).unwrap(), m);
        let mut bad = Macro {
            name: "x".repeat(500),
            steps: vec![
                Step::ResizeImage {
                    width: 0,
                    height: 999_999,
                    resample: Resample::Bicubic
                };
                600
            ],
        };
        bad.sanitize();
        assert_eq!(bad.steps.len(), MAX_STEPS);
        assert_eq!(bad.name.len(), 80);
        assert_eq!(
            bad.steps[0],
            Step::ResizeImage {
                width: 1,
                height: Document::MAX_SIDE,
                resample: Resample::Bicubic
            }
        );
        assert!(Step::Adjust(Adjustment::Grayscale)
            .describe()
            .contains("Tons"));
    }

    #[test]
    fn batch_reports_each_file_never_overwrites_originals_and_handles_failures() {
        let dir = tmp("run");
        let (inp, out) = (dir.join("in"), dir.join("out"));
        std::fs::create_dir_all(&inp).unwrap();
        let good = inp.join("a.png");
        std::fs::write(&good, imageio::export_png(&doc()).unwrap()).unwrap();
        let good_bytes = std::fs::read(&good).unwrap();
        let corrupt = inp.join("b.png");
        std::fs::write(&corrupt, b"isto nao e png").unwrap();
        let weird = inp.join("c.xyz");
        std::fs::write(&weird, b"nada").unwrap();
        let pdf = inp.join("d.pdf");
        std::fs::write(&pdf, b"%PDF-1.4").unwrap();
        let missing = inp.join("sumiu.png");
        let project = inp.join("e.orva");
        std::fs::write(&project, orva::write(&doc()).unwrap()).unwrap();
        let files = vec![
            good.clone(),
            corrupt.clone(),
            weird,
            pdf,
            missing,
            project.clone(),
        ];
        let o = BatchOpts {
            out_dir: out.clone(),
            format: Format::Png,
            export: ExportOpts::default(),
            suffix: "_x".into(),
            overwrite: false,
        };
        let mut seen = Vec::new();
        let rep = run_batch(
            &files,
            Some(&mac()),
            &o,
            &AtomicBool::new(false),
            &mut |i, n, _| seen.push((i, n)),
        );
        assert_eq!(rep.len(), 6);
        assert!(matches!(&rep[0].status, Status::Done(p) if p.ends_with("a_x.png")));
        assert!(
            matches!(rep[1].status, Status::Failed(_)),
            "{:?}",
            rep[1].status
        );
        assert!(matches!(rep[2].status, Status::Failed(_)));
        assert!(matches!(rep[3].status, Status::Skipped(_)));
        assert!(matches!(rep[4].status, Status::Failed(_)));
        assert!(matches!(&rep[5].status, Status::Done(p) if p.ends_with("e_x.png")));
        assert_eq!(seen.last(), Some(&(6, 6)));
        // Saída: conteúdo transformado; original intacto.
        let outimg = imageio::import(&std::fs::read(out.join("a_x.png")).unwrap(), "o")
            .unwrap()
            .document;
        assert_eq!((outimg.width, outimg.height), (10, 20));
        assert_eq!(
            std::fs::read(&good).unwrap(),
            good_bytes,
            "original preservado"
        );
        // Rodar de novo não sobrescreve a saída anterior: cria " (2)".
        let rep2 = run_batch(
            std::slice::from_ref(&good),
            Some(&mac()),
            &o,
            &AtomicBool::new(false),
            &mut |_, _, _| {},
        );
        assert!(
            matches!(&rep2[0].status, Status::Done(p) if p.ends_with("a_x (2).png")),
            "{:?}",
            rep2[0].status
        );
        // Saída na mesma pasta, mesmo nome e mesmo formato: recusa sobrescrever o original.
        let same = BatchOpts {
            out_dir: inp.clone(),
            suffix: String::new(),
            ..o.clone()
        };
        let rep3 = run_batch(
            std::slice::from_ref(&good),
            None,
            &same,
            &AtomicBool::new(false),
            &mut |_, _, _| {},
        );
        assert!(
            matches!(&rep3[0].status, Status::Failed(m) if m.contains("original")),
            "{:?}",
            rep3[0].status
        );
        assert_eq!(std::fs::read(&good).unwrap(), good_bytes);
        // Com a opção explícita, sobrescreve.
        let force = BatchOpts {
            overwrite: true,
            ..same
        };
        let rep4 = run_batch(
            std::slice::from_ref(&good),
            Some(&mac()),
            &force,
            &AtomicBool::new(false),
            &mut |_, _, _| {},
        );
        assert!(matches!(rep4[0].status, Status::Done(_)));
        assert_ne!(std::fs::read(&good).unwrap(), good_bytes);
        // Cancelar marca o restante como ignorado.
        let stop = AtomicBool::new(true);
        let rep5 = run_batch(&files, None, &o, &stop, &mut |_, _, _| {});
        assert!(rep5
            .iter()
            .all(|r| matches!(&r.status, Status::Skipped(m) if m == "cancelado")));
        let text = report_text(&rep);
        assert!(
            text.contains("[OK]")
                && text.contains("[FALHA]")
                && text.contains("[IGNORADO]")
                && text.contains("2 concluído")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
