//! Preferências locais (tema, escala da UI, tamanho da janela) em JSON.
//!
//! Gravadas apenas em `%APPDATA%\Orvalith\config.json` (ou `~/.config/orvalith/`),
//! de forma atômica. Nunca contêm documentos, senhas ou dados pessoais.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub dark_theme: bool,
    pub ui_scale: f32,
    pub window_size: [f32; 2],
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            dark_theme: true,
            ui_scale: 1.0,
            window_size: [1280.0, 800.0],
        }
    }
}

impl Prefs {
    /// Corrige valores fora de faixa vindos de um arquivo editado ou corrompido.
    fn sanitized(mut self) -> Self {
        let d = Self::default();
        if !self.ui_scale.is_finite() {
            self.ui_scale = d.ui_scale;
        }
        self.ui_scale = self.ui_scale.clamp(0.5, 3.0);
        for (v, def) in self.window_size.iter_mut().zip(d.window_size) {
            if !v.is_finite() {
                *v = def;
            }
        }
        self.window_size[0] = self.window_size[0].clamp(800.0, 8000.0);
        self.window_size[1] = self.window_size[1].clamp(500.0, 8000.0);
        self
    }

    /// Lê as preferências; arquivo ausente ou inválido resulta nos padrões.
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Self>(&s).ok())
            .unwrap_or_default()
            .sanitized()
    }

    /// Grava de forma atômica (arquivo temporário + renomeação).
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    pub fn load() -> Self {
        config_path()
            .map(|p| Self::load_from(&p))
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = config_path() {
            if let Err(e) = self.save_to(&p) {
                eprintln!("Não foi possível salvar as preferências: {e}");
            }
        }
    }
}

pub fn config_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Orvalith"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(|a| PathBuf::from(a).join("orvalith"))
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/orvalith")))
    };
    base.map(|b| b.join("config.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("orvalith-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("config.json")
    }

    #[test]
    fn roundtrip() {
        let p = temp("rt");
        let prefs = Prefs {
            dark_theme: false,
            ui_scale: 1.5,
            window_size: [1400.0, 900.0],
        };
        prefs.save_to(&p).unwrap();
        assert_eq!(Prefs::load_from(&p), prefs);
        assert!(!p.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn corrupt_or_missing_gives_defaults() {
        let p = temp("bad");
        assert_eq!(Prefs::load_from(&p), Prefs::default());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{ nao e json").unwrap();
        assert_eq!(Prefs::load_from(&p), Prefs::default());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let p = temp("clamp");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, r#"{"ui_scale": 99.0, "window_size": [10.0, 1e9]}"#).unwrap();
        let l = Prefs::load_from(&p);
        assert_eq!(l.ui_scale, 3.0);
        assert_eq!(l.window_size, [800.0, 8000.0]);
        assert!(l.dark_theme);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
