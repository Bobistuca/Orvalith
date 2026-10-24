//! Registro local de falhas: um pânico grava um relatório em `crash.log` (pasta de preferências).
//!
//! Nada é enviado pela rede; o arquivo é só texto, limitado em tamanho, e pode ser apagado.

use std::io::Write;
use std::path::Path;

/// Tamanho máximo do arquivo; ao passar disso ele recomeça.
pub const MAX_BYTES: u64 = 256 * 1024;

/// Texto do relatório (sem caminhos de documentos nem conteúdo de imagens).
pub fn report(version: &str, message: &str, location: &str, backtrace: &str) -> String {
    format!(
        "=== Orvalith {version} — falha ===\nmensagem: {message}\nlocal: {location}\n{backtrace}\n\n"
    )
}

/// Acrescenta `text` ao arquivo, recomeçando-o se já passou do limite.
pub fn append(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let too_big = std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(!too_big)
        .truncate(too_big)
        .open(path)?;
    f.write_all(text.as_bytes())
}

/// Instala o gancho de pânico (mantém o comportamento padrão de imprimir no terminal).
pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(sem mensagem)".into());
        let location = info
            .location()
            .map_or_else(|| "?".into(), |l| format!("{}:{}", l.file(), l.line()));
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        if let Some(cfg) = crate::prefs::config_path() {
            let text = report(env!("CARGO_PKG_VERSION"), &message, &location, &bt);
            let _ = append(&cfg.with_file_name("crash.log"), &text);
        }
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_has_version_message_and_location() {
        let r = report("1.2.3", "boom", "a.rs:7", "bt");
        assert!(r.contains("1.2.3") && r.contains("boom") && r.contains("a.rs:7"));
    }

    #[test]
    fn append_accumulates_then_restarts_when_too_big() {
        let d = std::env::temp_dir().join(format!("orvalith-crash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = d.join("crash.log");
        append(&p, "um\n").unwrap();
        append(&p, "dois\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "um\ndois\n");
        std::fs::write(&p, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        append(&p, "novo\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "novo\n");
        let _ = std::fs::remove_dir_all(&d);
    }
}
