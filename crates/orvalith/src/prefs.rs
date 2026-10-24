//! Preferências locais (tema, escala da UI, tamanho da janela) em JSON.
//!
//! Gravadas apenas em `%APPDATA%\Orvalith\config.json` (ou `~/.config/orvalith/`),
//! de forma atômica. Nunca contêm documentos, senhas ou dados pessoais.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use orvalith_core::actions::Macro;
use orvalith_core::paint::{BrushParams, BrushPreset};
use orvalith_core::style::StylePreset;
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
    /// Último pincel e cores usados, presets e amostras do usuário.
    pub brush: BrushParams,
    pub fg: [u8; 4],
    pub bg: [u8; 4],
    pub brush_presets: Vec<BrushPreset>,
    pub swatches: Vec<[u8; 4]>,
    /// Predefinições de estilos de camada.
    pub style_presets: Vec<StylePreset>,
    /// Atalhos personalizados: id do comando → "Ctrl+Shift+K" (texto vazio = sem atalho).
    pub keymap: BTreeMap<String, String>,
    /// Ações (macros) salvas.
    pub macros: Vec<Macro>,
    /// Orçamento de memória do histórico de cada documento (MiB).
    pub history_budget_mb: u32,
    /// Orçamento de memória dos instantâneos (MiB).
    pub snapshot_budget_mb: u32,
    /// Quadros de vídeo guardados no cache de prévia.
    pub frame_cache: u32,
    /// Tema de alto contraste (acessibilidade).
    pub high_contrast: bool,
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
            brush: BrushParams::default(),
            fg: [0, 0, 0, 255],
            bg: [255, 255, 255, 255],
            brush_presets: Vec::new(),
            swatches: Vec::new(),
            style_presets: Vec::new(),
            keymap: BTreeMap::new(),
            macros: Vec::new(),
            history_budget_mb: 256,
            snapshot_budget_mb: 256,
            frame_cache: 48,
            high_contrast: false,
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
        self.brush = self.brush.sanitized();
        self.brush_presets.truncate(200);
        self.swatches.truncate(200);
        self.style_presets.truncate(100);
        self.macros.truncate(100);
        for m in &mut self.macros {
            m.sanitize();
        }
        self.history_budget_mb = self.history_budget_mb.clamp(16, 8192);
        self.snapshot_budget_mb = self.snapshot_budget_mb.clamp(16, 8192);
        self.frame_cache = self.frame_cache.clamp(4, 1024);
        self.keymap.retain(|k, v| k.len() <= 80 && v.len() <= 40);
        if self.keymap.len() > 500 {
            self.keymap.clear();
        }
        self.style_presets
            .retain(|p| orvalith_core::style::effects_sane(&p.effects));
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
            brush: BrushParams {
                size: 33.0,
                hardness: 0.4,
                ..BrushParams::default()
            },
            fg: [10, 20, 30, 255],
            brush_presets: vec![BrushPreset {
                name: "Meu pincel".into(),
                params: BrushParams::default(),
            }],
            swatches: vec![[1, 2, 3, 255]],
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
    #[test]
    fn macros_shortcuts_and_budgets_persist_and_hostile_values_are_sanitized() {
        use orvalith_core::actions::{Macro, Resample, Step};
        let dir = std::env::temp_dir().join(format!("orvalith-prefs15-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let mut p = Prefs::default();
        p.macros.push(Macro {
            name: "miniatura".into(),
            steps: vec![Step::FitWithin {
                max_side: 256,
                resample: Resample::Bilinear,
            }],
        });
        p.keymap.insert("file.save".into(), "Ctrl+Alt+S".into());
        p.history_budget_mb = 512;
        p.high_contrast = true;
        p.save_to(&path).unwrap();
        let back = Prefs::load_from(&path);
        assert_eq!(back.macros, p.macros);
        assert_eq!(
            back.keymap.get("file.save").map(String::as_str),
            Some("Ctrl+Alt+S")
        );
        assert_eq!((back.history_budget_mb, back.high_contrast), (512, true));
        // Arquivo editado com valores absurdos.
        let mut bad = p.clone();
        bad.history_budget_mb = 0;
        bad.frame_cache = 99999;
        bad.macros[0].steps = vec![
            Step::FitWithin {
                max_side: 0,
                resample: Resample::Nearest
            };
            9999
        ];
        bad.save_to(&path).unwrap();
        let fixed = Prefs::load_from(&path);
        assert_eq!((fixed.history_budget_mb, fixed.frame_cache), (16, 1024));
        assert!(fixed.macros[0].steps.len() <= orvalith_core::actions::MAX_STEPS);
        let _ = std::fs::remove_dir_all(dir);
    }
}
