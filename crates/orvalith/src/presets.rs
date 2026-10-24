//! Predefinições do usuário: documento, exportação, gradiente e espaços de trabalho.
//!
//! Tudo é gravado nas preferências locais e pode ser levado a outro computador num pacote JSON
//! (`.orvapreset`). Entradas vindas de arquivo passam por `sanitize`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use orvalith_core::adjust::GradStop;
use serde::{Deserialize, Serialize};

pub const MAX_PRESETS: usize = 100;
pub const MAX_BUNDLE_BYTES: u64 = 2 * 1024 * 1024;

fn clean_name(s: &str) -> String {
    s.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(60)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DocPreset {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub ppi: f32,
    pub transparent: bool,
}

impl Default for DocPreset {
    fn default() -> Self {
        Self {
            name: String::new(),
            width: 1920,
            height: 1080,
            ppi: 72.0,
            transparent: false,
        }
    }
}

impl DocPreset {
    fn sanitize(&mut self) {
        self.name = clean_name(&self.name);
        self.width = self.width.clamp(1, orvalith_core::Document::MAX_SIDE);
        self.height = self.height.clamp(1, orvalith_core::Document::MAX_SIDE);
        self.ppi = if self.ppi.is_finite() {
            self.ppi.clamp(1.0, 2400.0)
        } else {
            72.0
        };
    }
}

/// Predefinições embutidas (não editáveis, sempre disponíveis).
pub fn builtin_doc_presets() -> Vec<DocPreset> {
    let p = |name: &str, width, height, ppi| DocPreset {
        name: name.into(),
        width,
        height,
        ppi,
        transparent: false,
    };
    vec![
        p("Full HD 1920×1080", 1920, 1080, 72.0),
        p("4K UHD 3840×2160", 3840, 2160, 72.0),
        p("A4 retrato 300 ppi", 2480, 3508, 300.0),
        p("Quadrado 1080×1080", 1080, 1080, 72.0),
        p("Miniatura de vídeo 1280×720", 1280, 720, 72.0),
        p("Ícone 512×512", 512, 512, 72.0),
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportPreset {
    pub name: String,
    /// Extensão do formato ("png", "jpg"…); formato desconhecido faz a entrada ser descartada.
    pub ext: String,
    pub quality: u8,
    /// 0 = tamanho original.
    pub max_side: u32,
    pub background: [u8; 3],
    /// 0 = embutir perfil, 1 = converter para sRGB, 2 = gravar sem perfil.
    pub color: u8,
}

impl Default for ExportPreset {
    fn default() -> Self {
        Self {
            name: String::new(),
            ext: "png".into(),
            quality: 90,
            max_side: 0,
            background: [255, 255, 255],
            color: 0,
        }
    }
}

impl ExportPreset {
    fn valid(&mut self) -> bool {
        self.name = clean_name(&self.name);
        self.quality = self.quality.clamp(1, 100);
        self.max_side = self.max_side.min(16_384);
        self.color = self.color.min(2);
        !self.name.is_empty()
            && orvalith_core::formats::Format::from_ext(&self.ext).is_some_and(|f| {
                f.can_write() && orvalith_core::formats::Format::exportable().contains(&f)
            })
    }
}

pub fn builtin_export_presets() -> Vec<ExportPreset> {
    let p = |name: &str, ext: &str, quality, max_side| ExportPreset {
        name: name.into(),
        ext: ext.into(),
        quality,
        max_side,
        ..ExportPreset::default()
    };
    vec![
        p("Web leve (JPEG 80, 1920)", "jpg", 80, 1920),
        p("Alta qualidade (JPEG 95)", "jpg", 95, 0),
        p("Sem perdas (PNG)", "png", 90, 0),
    ]
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GradPreset {
    pub name: String,
    pub stops: Vec<GradStop>,
}

impl GradPreset {
    fn valid(&mut self) -> bool {
        self.name = clean_name(&self.name);
        self.stops.truncate(64);
        for s in &mut self.stops {
            s.pos = if s.pos.is_finite() {
                s.pos.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        self.stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        !self.name.is_empty() && self.stops.len() >= 2
    }
}

/// Quais painéis estão visíveis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub tools: bool,
    pub layers: bool,
    pub options: bool,
    pub status: bool,
    pub timeline: bool,
    pub histogram: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            tools: true,
            layers: true,
            options: true,
            status: true,
            timeline: false,
            histogram: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub name: String,
    pub layout: Layout,
}

pub fn builtin_workspaces() -> Vec<Workspace> {
    let w = |name: &str, layout| Workspace {
        name: name.into(),
        layout,
    };
    vec![
        w("Pintura", Layout::default()),
        w(
            "Fotografia",
            Layout {
                histogram: true,
                ..Layout::default()
            },
        ),
        w(
            "Animação",
            Layout {
                timeline: true,
                ..Layout::default()
            },
        ),
        w(
            "Mínimo",
            Layout {
                tools: true,
                layers: false,
                options: false,
                status: false,
                timeline: false,
                histogram: false,
            },
        ),
    ]
}

/// Sanitiza listas lidas das preferências ou de um pacote.
pub fn sanitize_docs(v: &mut Vec<DocPreset>) {
    v.truncate(MAX_PRESETS);
    for p in v.iter_mut() {
        p.sanitize();
    }
    v.retain(|p| !p.name.is_empty());
}

pub fn sanitize_exports(v: &mut Vec<ExportPreset>) {
    v.truncate(MAX_PRESETS);
    v.retain_mut(ExportPreset::valid);
}

pub fn sanitize_grads(v: &mut Vec<GradPreset>) {
    v.truncate(MAX_PRESETS);
    v.retain_mut(GradPreset::valid);
}

pub fn sanitize_workspaces(v: &mut Vec<Workspace>) {
    v.truncate(MAX_PRESETS);
    for w in v.iter_mut() {
        w.name = clean_name(&w.name);
    }
    v.retain(|w| !w.name.is_empty());
}

/// Pacote de predefinições para levar a outro computador.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bundle {
    pub format: String,
    pub docs: Vec<DocPreset>,
    pub exports: Vec<ExportPreset>,
    pub gradients: Vec<GradPreset>,
    pub workspaces: Vec<Workspace>,
}

pub const BUNDLE_TAG: &str = "orvalith-presets-1";

pub fn bundle_to_json(b: &Bundle) -> String {
    let mut b = b.clone();
    b.format = BUNDLE_TAG.into();
    serde_json::to_string_pretty(&b).unwrap_or_default()
}

pub fn bundle_from_json(text: &str) -> Result<Bundle, String> {
    if text.len() as u64 > MAX_BUNDLE_BYTES {
        return Err("Pacote grande demais".into());
    }
    let mut b: Bundle =
        serde_json::from_str(text).map_err(|e| format!("Pacote de predefinições inválido: {e}"))?;
    if b.format != BUNDLE_TAG {
        return Err("Não é um pacote de predefinições do Orvalith".into());
    }
    sanitize_docs(&mut b.docs);
    sanitize_exports(&mut b.exports);
    sanitize_grads(&mut b.gradients);
    sanitize_workspaces(&mut b.workspaces);
    Ok(b)
}

/// Insere ou substitui por nome, respeitando o limite.
pub fn upsert<T>(list: &mut Vec<T>, item: T, same: impl Fn(&T) -> bool) {
    list.retain(|x| !same(x));
    list.push(item);
    if list.len() > MAX_PRESETS {
        list.remove(0);
    }
}

// Gradientes compartilhados com o editor de gradiente (que não recebe as preferências).
static GRADS: Mutex<Vec<GradPreset>> = Mutex::new(Vec::new());
static GRADS_DIRTY: AtomicBool = AtomicBool::new(false);

pub fn load_gradients(list: Vec<GradPreset>) {
    if let Ok(mut g) = GRADS.lock() {
        *g = list;
    }
}

pub fn gradients() -> Vec<GradPreset> {
    GRADS.lock().map(|g| g.clone()).unwrap_or_default()
}

pub fn save_gradient(name: &str, stops: &[GradStop]) -> bool {
    let mut p = GradPreset {
        name: name.into(),
        stops: stops.to_vec(),
    };
    if !p.valid() {
        return false;
    }
    if let Ok(mut g) = GRADS.lock() {
        let n = p.name.clone();
        upsert(&mut g, p, |x| x.name == n);
        GRADS_DIRTY.store(true, Ordering::Relaxed);
        return true;
    }
    false
}

pub fn remove_gradient(index: usize) {
    if let Ok(mut g) = GRADS.lock() {
        if index < g.len() {
            g.remove(index);
            GRADS_DIRTY.store(true, Ordering::Relaxed);
        }
    }
}

/// Devolve os gradientes se mudaram desde a última chamada.
pub fn take_changed_gradients() -> Option<Vec<GradPreset>> {
    if GRADS_DIRTY.swap(false, Ordering::Relaxed) {
        GRADS.lock().ok().map(|g| g.clone())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_valid() {
        let mut d = builtin_doc_presets();
        let n = d.len();
        sanitize_docs(&mut d);
        assert_eq!(d.len(), n);
        let mut e = builtin_export_presets();
        let n = e.len();
        sanitize_exports(&mut e);
        assert_eq!(e.len(), n, "todos os formatos embutidos exportam");
        assert_eq!(builtin_workspaces().len(), 4);
    }

    #[test]
    fn bundle_roundtrip_and_hostile_values() {
        let b = Bundle {
            docs: vec![DocPreset {
                name: "  Meu\u{7}  ".into(),
                width: 0,
                height: 999_999,
                ppi: 99_999.0,
                transparent: true,
            }],
            exports: vec![
                ExportPreset {
                    name: "x".into(),
                    ext: "exe".into(),
                    ..ExportPreset::default()
                },
                ExportPreset {
                    name: "ok".into(),
                    ext: "jpg".into(),
                    quality: 0,
                    max_side: u32::MAX,
                    color: 9,
                    ..ExportPreset::default()
                },
            ],
            gradients: vec![
                GradPreset {
                    name: "um".into(),
                    stops: vec![GradStop {
                        pos: 0.0,
                        color: [0; 3],
                    }],
                },
                GradPreset {
                    name: "dois".into(),
                    stops: vec![
                        GradStop {
                            pos: -3.0,
                            color: [1; 3],
                        },
                        GradStop {
                            pos: 7.0,
                            color: [2; 3],
                        },
                    ],
                },
            ],
            workspaces: vec![Workspace {
                name: "".into(),
                layout: Layout::default(),
            }],
            ..Bundle::default()
        };
        let back = bundle_from_json(&bundle_to_json(&b)).unwrap();
        assert_eq!(back.docs.len(), 1);
        assert_eq!(back.docs[0].name, "Meu");
        assert_eq!(back.docs[0].width, 1);
        assert_eq!(back.docs[0].height, orvalith_core::Document::MAX_SIDE);
        assert_eq!(back.docs[0].ppi, 2400.0);
        assert_eq!(back.exports.len(), 1, "formato desconhecido descartado");
        assert_eq!(back.exports[0].quality, 1);
        assert_eq!(back.exports[0].max_side, 16_384);
        assert_eq!(back.exports[0].color, 2);
        assert_eq!(back.gradients.len(), 1);
        assert!(back.gradients[0]
            .stops
            .iter()
            .all(|s| (0.0..=1.0).contains(&s.pos)));
        assert!(back.workspaces.is_empty());
    }

    #[test]
    fn bundle_rejects_foreign_or_huge_text() {
        assert!(bundle_from_json("{}").is_err());
        assert!(bundle_from_json("lixo").is_err());
        assert!(bundle_from_json(&" ".repeat(MAX_BUNDLE_BYTES as usize + 1)).is_err());
    }

    #[test]
    fn upsert_replaces_by_name_and_caps_size() {
        let mut v = vec![DocPreset {
            name: "a".into(),
            ..DocPreset::default()
        }];
        upsert(
            &mut v,
            DocPreset {
                name: "a".into(),
                width: 5,
                ..DocPreset::default()
            },
            |x| x.name == "a",
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].width, 5);
        for i in 0..150 {
            let n = format!("p{i}");
            upsert(
                &mut v,
                DocPreset {
                    name: n.clone(),
                    ..DocPreset::default()
                },
                |x| x.name == n,
            );
        }
        assert_eq!(v.len(), MAX_PRESETS);
    }

    #[test]
    fn gradient_store_reports_changes_once() {
        let _ = take_changed_gradients();
        load_gradients(Vec::new());
        let stops = [
            GradStop {
                pos: 0.0,
                color: [0; 3],
            },
            GradStop {
                pos: 1.0,
                color: [255; 3],
            },
        ];
        assert!(!save_gradient("   ", &stops));
        assert!(save_gradient("PB", &stops));
        assert_eq!(take_changed_gradients().unwrap().len(), 1);
        assert!(take_changed_gradients().is_none());
        remove_gradient(0);
        assert!(take_changed_gradients().unwrap().is_empty());
    }
}
