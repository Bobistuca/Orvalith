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
    /// Autosave/recuperação local: desligado por padrão.
    pub autosave_enabled: bool,
    pub autosave_secs: u32,
    /// Pasta das cópias de recuperação; `None` usa a pasta padrão do aplicativo.
    pub recovery_dir: Option<String>,
    /// Lista de arquivos recentes (opcional e apagável).
    pub recent_enabled: bool,
    pub recent: Vec<String>,
}

pub const MAX_RECENT: usize = 8;

impl Default for Prefs {
    fn default() -> Self {
        Self {
            dark_theme: true,
            ui_scale: 1.0,
            window_size: [1280.0, 800.0],
            autosave_enabled: false,
            autosave_secs: 120,
            recovery_dir: None,
            recent_enabled: true,
            recent: Vec::new(),
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
        self.autosave_secs = self.autosave_secs.clamp(15, 3600);
        self.recent.truncate(MAX_RECENT);
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

    /// Registra um arquivo recente (se a lista estiver habilitada).
    pub fn add_recent(&mut self, path: &Path) {
        if !self.recent_enabled {
            return;
        }
        let p = path.to_string_lossy().into_owned();
        self.recent.retain(|r| *r != p);
        self.recent.insert(0, p);
        self.recent.truncate(MAX_RECENT);
    }

    /// Pasta efetiva das cópias de recuperação.
    pub fn recovery_path(&self) -> Option<PathBuf> {
        match &self.recovery_dir {
            Some(d) if !d.is_empty() => Some(PathBuf::from(d)),
            _ => config_path().and_then(|c| c.parent().map(|p| p.join("recovery"))),
        }
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
            autosave_enabled: true,
            autosave_secs: 60,
            recent: vec!["a.orva".into()],
            ..Prefs::default()
        };
        prefs.save_to(&p).unwrap();
        assert_eq!(Prefs::load_from(&p), prefs);
        assert!(!p.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn autosave_is_off_by_default_and_recent_is_bounded() {
        let mut p = Prefs::default();
        assert!(!p.autosave_enabled);
        for i in 0..20 {
            p.add_recent(Path::new(&format!("f{i}.orva")));
        }
        p.add_recent(Path::new("f15.orva"));
        assert_eq!(p.recent.len(), MAX_RECENT);
        assert_eq!(p.recent[0], "f15.orva");
        assert_eq!(p.recent.iter().filter(|r| *r == "f15.orva").count(), 1);
        p.recent_enabled = false;
        p.recent.clear();
        p.add_recent(Path::new("x.orva"));
        assert!(p.recent.is_empty());
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
