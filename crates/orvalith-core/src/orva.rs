//! Formato de projeto `.orva` (versão 1). Especificação em docs/ORVA_FORMAT.md.
//!
//! Layout: `"ORVALITH"` (8) · versão u32 LE · tamanho do manifesto u32 LE ·
//! CRC32 do manifesto u32 LE · manifesto JSON UTF-8 · blob de recursos.
//! Cada camada é um recurso zlib de pixels RGBA8 (sRGB, alfa não pré-multiplicado)
//! com CRC32 dos bytes descomprimidos. Tudo é validado antes de alocar memória.

use std::collections::HashSet;
use std::fmt;
use std::io::{Read, Write};

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};

use crate::document::{Document, Layer, LayerId};

pub const MAGIC: &[u8; 8] = b"ORVALITH";
pub const CURRENT_VERSION: u32 = 1;
const HEADER_LEN: usize = 20;
const MAX_MANIFEST: usize = 64 * 1024 * 1024;
const MAX_LAYERS: usize = 10_000;
/// Limite de memória descomprimida de um projeto (bytes RGBA somados).
pub const MAX_TOTAL_PIXEL_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug)]
pub enum OrvaError {
    NotOrva,
    UnsupportedVersion(u32),
    Corrupt(String),
    TooLarge(String),
}

impl fmt::Display for OrvaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotOrva => write!(f, "O arquivo não é um projeto Orvalith (.orva)."),
            Self::UnsupportedVersion(v) => write!(
                f,
                "Versão {v} do formato .orva não é suportada (esta versão lê até {CURRENT_VERSION}). Atualize o Orvalith."
            ),
            Self::Corrupt(m) => write!(f, "Arquivo .orva corrompido: {m}"),
            Self::TooLarge(m) => write!(f, "Projeto grande demais para abrir: {m}"),
        }
    }
}

impl std::error::Error for OrvaError {}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    app_version: String,
    name: String,
    width: u32,
    height: u32,
    #[serde(default = "default_ppi")]
    ppi: f32,
    layers: Vec<LayerEntry>,
}

fn default_ppi() -> f32 {
    72.0
}

#[derive(Serialize, Deserialize)]
struct LayerEntry {
    id: u64,
    name: String,
    visible: bool,
    opacity: f32,
    width: u32,
    height: u32,
    offset: u64,
    stored_len: u64,
    raw_len: u64,
    crc32: u32,
}

/// Serializa o documento (camadas, propriedades e pixels) para bytes `.orva`.
pub fn write(doc: &Document) -> Result<Vec<u8>, OrvaError> {
    let mut blob = Vec::new();
    let mut entries = Vec::with_capacity(doc.layers.len());
    for l in &doc.layers {
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(3));
        enc.write_all(&l.pixels)
            .map_err(|e| OrvaError::Corrupt(e.to_string()))?;
        let packed = enc
            .finish()
            .map_err(|e| OrvaError::Corrupt(e.to_string()))?;
        entries.push(LayerEntry {
            id: l.id.0,
            name: l.name.clone(),
            visible: l.visible,
            opacity: l.opacity,
            width: l.width,
            height: l.height,
            offset: blob.len() as u64,
            stored_len: packed.len() as u64,
            raw_len: l.pixels.len() as u64,
            crc32: crc32fast::hash(&l.pixels),
        });
        blob.extend_from_slice(&packed);
    }
    let manifest = Manifest {
        format: "orvalith-project".into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        name: doc.name.clone(),
        width: doc.width,
        height: doc.height,
        ppi: doc.ppi,
        layers: entries,
    };
    let json = serde_json::to_vec(&manifest).map_err(|e| OrvaError::Corrupt(e.to_string()))?;
    let mut out = Vec::with_capacity(HEADER_LEN + json.len() + blob.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&CURRENT_VERSION.to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(&json).to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&blob);
    Ok(out)
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Lê e valida bytes `.orva`. Nenhuma alocação proporcional a valores do arquivo
/// ocorre antes de a consistência dos tamanhos ser verificada.
pub fn read(bytes: &[u8]) -> Result<Document, OrvaError> {
    if bytes.len() < 8 || &bytes[..8] != MAGIC {
        return Err(OrvaError::NotOrva);
    }
    if bytes.len() < HEADER_LEN {
        return Err(OrvaError::Corrupt("cabeçalho incompleto".into()));
    }
    let version = u32_at(bytes, 8);
    if version == 0 || version > CURRENT_VERSION {
        return Err(OrvaError::UnsupportedVersion(version));
    }
    let manifest_len = u32_at(bytes, 12) as usize;
    let manifest_crc = u32_at(bytes, 16);
    if manifest_len > MAX_MANIFEST {
        return Err(OrvaError::TooLarge("manifesto".into()));
    }
    let manifest_end = HEADER_LEN
        .checked_add(manifest_len)
        .filter(|&e| e <= bytes.len())
        .ok_or_else(|| OrvaError::Corrupt("manifesto truncado".into()))?;
    let manifest_bytes = &bytes[HEADER_LEN..manifest_end];
    if crc32fast::hash(manifest_bytes) != manifest_crc {
        return Err(OrvaError::Corrupt(
            "soma de verificação do manifesto".into(),
        ));
    }
    let manifest: Manifest = serde_json::from_slice(manifest_bytes)
        .map_err(|e| OrvaError::Corrupt(format!("manifesto inválido ({e})")))?;
    if manifest.format != "orvalith-project" {
        return Err(OrvaError::NotOrva);
    }
    let side_ok = |v: u32| (1..=Document::MAX_SIDE).contains(&v);
    if !side_ok(manifest.width) || !side_ok(manifest.height) {
        return Err(OrvaError::Corrupt(
            "dimensões do documento inválidas".into(),
        ));
    }
    if manifest.layers.len() > MAX_LAYERS {
        return Err(OrvaError::TooLarge("camadas demais".into()));
    }
    if !manifest.ppi.is_finite() || manifest.ppi <= 0.0 {
        return Err(OrvaError::Corrupt("resolução inválida".into()));
    }

    let blob = &bytes[manifest_end..];
    let mut total: u64 = 0;
    let mut seen = HashSet::new();
    for e in &manifest.layers {
        if !side_ok(e.width) || !side_ok(e.height) {
            return Err(OrvaError::Corrupt(format!(
                "dimensões da camada \"{}\"",
                e.name
            )));
        }
        let expect = u64::from(e.width) * u64::from(e.height) * 4;
        if e.raw_len != expect {
            return Err(OrvaError::Corrupt(format!(
                "tamanho da camada \"{}\"",
                e.name
            )));
        }
        total = total.saturating_add(e.raw_len);
        if total > MAX_TOTAL_PIXEL_BYTES {
            return Err(OrvaError::TooLarge(format!(
                "mais de {} MiB de pixels",
                MAX_TOTAL_PIXEL_BYTES >> 20
            )));
        }
        let end = e.offset.checked_add(e.stored_len);
        if end.is_none_or(|end| end > blob.len() as u64) {
            return Err(OrvaError::Corrupt(format!(
                "recurso fora do arquivo: \"{}\"",
                e.name
            )));
        }
        if !seen.insert(e.id) {
            return Err(OrvaError::Corrupt("IDs de camada duplicados".into()));
        }
        if !e.opacity.is_finite() {
            return Err(OrvaError::Corrupt("opacidade inválida".into()));
        }
    }

    let mut layers = Vec::with_capacity(manifest.layers.len());
    for e in &manifest.layers {
        let start = e.offset as usize;
        let packed = &blob[start..start + e.stored_len as usize];
        let mut pixels = Vec::with_capacity(e.raw_len as usize);
        ZlibDecoder::new(packed)
            .take(e.raw_len + 1)
            .read_to_end(&mut pixels)
            .map_err(|err| OrvaError::Corrupt(format!("camada \"{}\": {err}", e.name)))?;
        if pixels.len() as u64 != e.raw_len {
            return Err(OrvaError::Corrupt(format!(
                "camada \"{}\" com tamanho errado",
                e.name
            )));
        }
        if crc32fast::hash(&pixels) != e.crc32 {
            return Err(OrvaError::Corrupt(format!(
                "soma de verificação da camada \"{}\"",
                e.name
            )));
        }
        layers.push(Layer {
            id: LayerId(e.id),
            name: e.name.clone(),
            visible: e.visible,
            opacity: e.opacity.clamp(0.0, 1.0),
            width: e.width,
            height: e.height,
            pixels,
        });
    }
    Ok(Document::from_parts(
        manifest.name,
        manifest.width,
        manifest.height,
        manifest.ppi,
        layers,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Document {
        let mut doc = Document::new("Projeto Ação", 5, 3, Some([10, 20, 30, 255])).unwrap();
        doc.ppi = 300.0;
        doc.layers[0].set_pixel(2, 1, [200, 100, 50, 128]);
        let mut top = Layer::transparent("Topo", 5, 3);
        top.visible = false;
        top.opacity = 0.4;
        top.set_pixel(0, 0, [1, 2, 3, 4]);
        doc.layers.push(top);
        doc
    }

    #[test]
    fn roundtrip_preserves_pixels_and_properties() {
        let doc = sample();
        let back = read(&write(&doc).unwrap()).unwrap();
        assert_eq!(back.name, doc.name);
        assert_eq!((back.width, back.height, back.ppi), (5, 3, 300.0));
        assert_eq!(back.layers.len(), 2);
        for (a, b) in doc.layers.iter().zip(&back.layers) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.name, b.name);
            assert_eq!(a.visible, b.visible);
            assert_eq!(a.opacity, b.opacity);
            assert_eq!(a.pixels, b.pixels);
        }
        assert!(!back.modified);
    }

    #[test]
    fn new_layer_ids_do_not_collide_after_load() {
        let back = read(&write(&sample()).unwrap()).unwrap();
        let max = back.layers.iter().map(|l| l.id.0).max().unwrap();
        assert!(Layer::transparent("x", 1, 1).id.0 > max);
    }

    #[test]
    fn rejects_foreign_and_truncated_files() {
        assert!(matches!(read(b"PNG..."), Err(OrvaError::NotOrva)));
        let bytes = write(&sample()).unwrap();
        for cut in [10, HEADER_LEN, HEADER_LEN + 5, bytes.len() - 3] {
            assert!(read(&bytes[..cut]).is_err(), "cut {cut}");
        }
    }

    #[test]
    fn detects_corrupted_pixels_and_manifest() {
        let bytes = write(&sample()).unwrap();
        let mut bad = bytes.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xFF;
        assert!(read(&bad).is_err());
        let mut bad = bytes;
        bad[HEADER_LEN + 3] ^= 0x01;
        assert!(matches!(read(&bad), Err(OrvaError::Corrupt(_))));
    }

    #[test]
    fn rejects_future_version() {
        let mut bytes = write(&sample()).unwrap();
        bytes[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert!(matches!(
            read(&bytes),
            Err(OrvaError::UnsupportedVersion(99))
        ));
    }

    /// Manifesto que declara camadas gigantes deve falhar antes de alocar.
    #[test]
    fn rejects_allocation_bomb_before_allocating() {
        let manifest = serde_json::json!({
            "format": "orvalith-project", "app_version": "x", "name": "bomba",
            "width": 32768, "height": 32768, "ppi": 72.0,
            "layers": [
                {"id":1,"name":"a","visible":true,"opacity":1.0,"width":32768,"height":32768,
                 "offset":0,"stored_len":0,"raw_len": 4294967296u64,"crc32":0},
                {"id":2,"name":"b","visible":true,"opacity":1.0,"width":32768,"height":32768,
                 "offset":0,"stored_len":0,"raw_len": 4294967296u64,"crc32":0}
            ]
        });
        let json = serde_json::to_vec(&manifest).unwrap();
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&crc32fast::hash(&json).to_le_bytes());
        out.extend_from_slice(&json);
        assert!(matches!(read(&out), Err(OrvaError::TooLarge(_))));
    }
}
