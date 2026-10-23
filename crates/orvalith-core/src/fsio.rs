//! Escrita atômica: grava em arquivo temporário vizinho e substitui o destino só no sucesso.

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".orvatmp");
    path.with_file_name(name)
}

/// Executa `write` sobre um arquivo temporário e, se tudo der certo, substitui `path`.
/// Em caso de erro o arquivo anterior permanece intacto e o temporário é removido.
pub fn atomic_write_with(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let tmp = temp_path(path);
    let result = (|| {
        let mut file = File::create(&tmp)?;
        write(&mut file)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write_with(path, |f| f.write_all(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("orvalith-fsio-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn replaces_existing_file() {
        let d = dir("ok");
        let p = d.join("a.bin");
        atomic_write(&p, b"antigo").unwrap();
        atomic_write(&p, b"novo").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"novo");
        assert!(!temp_path(&p).exists());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn failed_write_keeps_previous_file() {
        let d = dir("fail");
        let p = d.join("a.bin");
        atomic_write(&p, b"original").unwrap();
        let err = atomic_write_with(&p, |f| {
            f.write_all(b"parcial")?;
            Err(io::Error::other("disco cheio simulado"))
        });
        assert!(err.is_err());
        assert_eq!(std::fs::read(&p).unwrap(), b"original");
        assert!(!temp_path(&p).exists());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn unwritable_destination_reports_error() {
        let d = dir("nodir");
        let p = d.join("inexistente").join("a.bin");
        assert!(atomic_write(&p, b"x").is_err());
        let _ = std::fs::remove_dir_all(d);
    }
}
