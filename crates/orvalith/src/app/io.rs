//! Abrir, salvar, exportar e recuperar documentos (somente arquivos locais).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use orvalith_core::{fsio, imageio, orva, Document};

use super::{ExportForm, OrvalithApp};
use crate::canvas::OpenDoc;

/// Tamanho máximo de arquivo aceito na leitura (verificado antes de ler para a memória).
const MAX_FILE_BYTES: u64 = 3 << 30;

struct Loaded {
    doc: Document,
    warnings: Vec<String>,
    project: bool,
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Sem título".into())
}

fn load_file(path: &Path) -> Result<Loaded, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_FILE_BYTES {
        return Err("arquivo grande demais".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "orva" => {
            let mut doc = orva::read(&bytes).map_err(|e| e.to_string())?;
            doc.name = stem(path);
            Ok(Loaded {
                doc,
                warnings: vec![],
                project: true,
            })
        }
        "png" | "jpg" | "jpeg" | "jpe" | "jfif" => {
            let imp = imageio::import(&bytes, &stem(path))?;
            Ok(Loaded {
                doc: imp.document,
                warnings: imp.warnings,
                project: false,
            })
        }
        _ => Err("formato ainda não suportado (nesta versão: .orva, PNG e JPEG)".into()),
    }
}

fn with_extension(mut p: PathBuf, ext: &str) -> PathBuf {
    let has = p
        .extension()
        .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(ext));
    if !has {
        let mut name = p.file_name().unwrap_or_default().to_os_string();
        name.push(".");
        name.push(ext);
        p.set_file_name(name);
    }
    p
}

fn recovery_file(dir: &Path, doc_id: u64) -> PathBuf {
    dir.join(format!("recovery-{}-{doc_id}.orva", std::process::id()))
}

/// Cópias de recuperação deixadas por outras sessões (processos) na pasta.
pub(super) fn scan_recovery(dir: Option<PathBuf>) -> Vec<PathBuf> {
    let Some(dir) = dir else { return vec![] };
    let own = format!("recovery-{}-", std::process::id());
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    rd.filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.starts_with("recovery-") && n.ends_with(".orva") && !n.starts_with(&own)
            })
        })
        .collect()
}

impl OrvalithApp {
    pub(super) fn show_message(&mut self, title: &str, text: String) {
        self.message = Some((title.to_string(), text));
    }

    pub(super) fn open_dialog(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter(
                "Projetos e imagens",
                &["orva", "png", "jpg", "jpeg", "jpe", "jfif"],
            )
            .add_filter("Projeto Orvalith", &["orva"])
            .add_filter("Imagens PNG/JPEG", &["png", "jpg", "jpeg", "jpe", "jfif"])
            .pick_file();
        if let Some(p) = picked {
            self.open_path(p);
        }
    }

    pub(super) fn open_path(&mut self, path: PathBuf) {
        if let Some(i) = self
            .docs
            .iter()
            .position(|d| d.path.as_deref() == Some(path.as_path()))
        {
            self.active = i;
            return;
        }
        match load_file(&path) {
            Ok(l) => {
                let mut open = OpenDoc::new(l.doc);
                if l.project {
                    open.path = Some(path.clone());
                }
                self.docs.push(open);
                self.active = self.docs.len() - 1;
                self.prefs.add_recent(&path);
                self.prefs.save();
                if !l.warnings.is_empty() {
                    self.show_message("Aviso de importação", l.warnings.join("\n"));
                }
            }
            Err(e) => self.show_message(
                "Não foi possível abrir",
                format!("{}\n\n{e}", path.display()),
            ),
        }
    }

    /// Salva o projeto `.orva` do documento `idx`. Retorna `true` se foi gravado.
    pub(super) fn save_doc(&mut self, idx: usize, save_as: bool) -> bool {
        let Some(open) = self.docs.get(idx) else {
            return false;
        };
        let path = match (&open.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let mut dlg = rfd::FileDialog::new()
                    .add_filter("Projeto Orvalith", &["orva"])
                    .set_file_name(format!("{}.orva", open.doc.name));
                if let Some(dir) = open.path.as_deref().and_then(Path::parent) {
                    dlg = dlg.set_directory(dir);
                }
                match dlg.save_file() {
                    Some(p) => with_extension(p, "orva"),
                    None => return false,
                }
            }
        };
        let bytes = match orva::write(&open.doc) {
            Ok(b) => b,
            Err(e) => {
                self.show_message("Erro ao salvar", e.to_string());
                return false;
            }
        };
        if let Err(e) = fsio::atomic_write(&path, &bytes) {
            self.show_message(
                "Erro ao salvar",
                format!(
                    "{}\n\n{e}\n\nO arquivo anterior (se existia) foi preservado.",
                    path.display()
                ),
            );
            return false;
        }
        let id = {
            let open = &mut self.docs[idx];
            open.doc.modified = false;
            open.doc.name = stem(&path);
            open.path = Some(path.clone());
            open.doc.id.0
        };
        self.remove_recovery(id);
        self.prefs.add_recent(&path);
        self.prefs.save();
        true
    }

    /// Exporta a imagem composta (PNG/JPEG) sem alterar o projeto nem o estado "modificado".
    pub(super) fn export_doc(&mut self, form: &ExportForm) {
        let Some(open) = self.docs.get(self.active) else {
            return;
        };
        let (ext, label) = if form.jpeg {
            ("jpg", "JPEG")
        } else {
            ("png", "PNG")
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                label,
                if form.jpeg {
                    &["jpg", "jpeg"]
                } else {
                    &["png"]
                },
            )
            .set_file_name(format!("{}.{ext}", open.doc.name))
            .save_file()
        else {
            return;
        };
        let path = if form.jpeg
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("jpeg"))
        {
            path
        } else {
            with_extension(path, ext)
        };
        let result = if form.jpeg {
            imageio::export_jpeg(&open.doc, form.quality, form.background)
        } else {
            imageio::export_png(&open.doc)
        };
        match result.and_then(|b| fsio::atomic_write(&path, &b).map_err(|e| e.to_string())) {
            Ok(()) => self.show_message("Exportação concluída", path.display().to_string()),
            Err(e) => self.show_message("Erro ao exportar", format!("{}\n\n{e}", path.display())),
        }
    }

    // ---- Autosave e recuperação (opcionais; desligados por padrão) ----

    pub(super) fn remove_recovery(&self, doc_id: u64) {
        if let Some(dir) = self.prefs.recovery_path() {
            let _ = std::fs::remove_file(recovery_file(&dir, doc_id));
        }
    }

    pub(super) fn remove_all_own_recovery(&self) {
        for open in &self.docs {
            self.remove_recovery(open.doc.id.0);
        }
    }

    /// Apaga todas as cópias de recuperação (desta e de outras sessões).
    pub(super) fn clear_recovery_files(&mut self) -> usize {
        let Some(dir) = self.prefs.recovery_path() else {
            return 0;
        };
        let Ok(rd) = std::fs::read_dir(dir) else {
            return 0;
        };
        let mut n = 0;
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("recovery-")
                && name.ends_with(".orva")
                && std::fs::remove_file(e.path()).is_ok()
            {
                n += 1;
            }
        }
        self.recovery_found.clear();
        n
    }

    pub(super) fn autosave_tick(&mut self, ctx: &egui::Context) {
        while let Ok(err) = self.autosave_rx.try_recv() {
            if self.message.is_none() {
                self.show_message("Falha no autosave", err);
            }
        }
        if !self.prefs.autosave_enabled {
            return;
        }
        ctx.request_repaint_after(Duration::from_secs(5));
        if self.last_autosave.elapsed() < Duration::from_secs(u64::from(self.prefs.autosave_secs)) {
            return;
        }
        self.last_autosave = Instant::now();
        let Some(dir) = self.prefs.recovery_path() else {
            return;
        };
        for open in self.docs.iter_mut() {
            if !open.doc.modified || open.autosaved_rev == Some(open.doc.revision) {
                continue;
            }
            open.autosaved_rev = Some(open.doc.revision);
            let doc = open.doc.clone();
            let file = recovery_file(&dir, doc.id.0);
            let dir = dir.clone();
            let tx = self.autosave_tx.clone();
            std::thread::spawn(move || {
                let result = std::fs::create_dir_all(&dir)
                    .map_err(|e| e.to_string())
                    .and_then(|()| orva::write(&doc).map_err(|e| e.to_string()))
                    .and_then(|b| fsio::atomic_write(&file, &b).map_err(|e| e.to_string()));
                if let Err(e) = result {
                    let _ = tx.send(e);
                }
            });
        }
    }

    /// Reabre as cópias de recuperação como documentos não salvos e remove os arquivos.
    pub(super) fn restore_recovery(&mut self) {
        let files = std::mem::take(&mut self.recovery_found);
        let mut failed = Vec::new();
        for f in files {
            match std::fs::read(&f)
                .map_err(|e| e.to_string())
                .and_then(|b| orva::read(&b).map_err(|e| e.to_string()))
            {
                Ok(mut doc) => {
                    doc.name = format!("{} (recuperado)", doc.name);
                    doc.modified = true;
                    self.docs.push(OpenDoc::new(doc));
                    self.active = self.docs.len() - 1;
                    let _ = std::fs::remove_file(&f);
                }
                Err(e) => failed.push(format!("{}: {e}", f.display())),
            }
        }
        if !failed.is_empty() {
            self.show_message("Recuperação parcial", failed.join("\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_is_appended_only_when_missing() {
        assert_eq!(
            with_extension("a/b".into(), "orva"),
            PathBuf::from("a/b.orva")
        );
        assert_eq!(
            with_extension("a/b.ORVA".into(), "orva"),
            PathBuf::from("a/b.ORVA")
        );
        assert_eq!(
            with_extension("a/b.png".into(), "orva"),
            PathBuf::from("a/b.png.orva")
        );
    }

    #[test]
    fn scan_recovery_ignores_own_session_and_other_files() {
        let dir = std::env::temp_dir().join(format!("orvalith-rec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("recovery-1-5.orva"), b"x").unwrap();
        std::fs::write(recovery_file(&dir, 9), b"x").unwrap();
        std::fs::write(dir.join("nota.txt"), b"x").unwrap();
        let found = scan_recovery(Some(dir.clone()));
        assert_eq!(found, vec![dir.join("recovery-1-5.orva")]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn load_file_rejects_unknown_extension_and_missing_file() {
        assert!(load_file(Path::new("nao-existe.png")).is_err());
        let p = std::env::temp_dir().join(format!("orvalith-x-{}.xyz", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        assert!(load_file(&p).is_err());
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn project_save_and_load_roundtrip_through_disk() {
        let mut doc = Document::new("d", 3, 2, Some([9, 8, 7, 255])).unwrap();
        doc.layers[0].set_pixel(1, 1, [1, 2, 3, 4]);
        let p = std::env::temp_dir().join(format!("orvalith-rt-{}.orva", std::process::id()));
        fsio::atomic_write(&p, &orva::write(&doc).unwrap()).unwrap();
        let l = load_file(&p).unwrap();
        assert!(l.project);
        assert_eq!(l.doc.layers[0].pixels, doc.layers[0].pixels);
        assert_eq!(l.doc.name, stem(&p));
        let _ = std::fs::remove_file(p);
    }
}
