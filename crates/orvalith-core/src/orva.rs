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

use crate::blend::BlendMode;
use crate::document::{Channel, Document, Guide, Layer, LayerId, LayerKind, LayerMask};
use crate::layerops::normalize;
use crate::source::{shape_sane, LayerSource};
use crate::vector::ShapeSpec;

pub const MAGIC: &[u8; 8] = b"ORVALITH";
pub const CURRENT_VERSION: u32 = 6;
const MAX_CHANNELS: usize = 256;
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
    /// Versão 3: canais alfa auxiliares (seleções salvas).
    #[serde(default)]
    channels: Vec<ChannelEntry>,
    /// Versão 4: guias de régua.
    #[serde(default)]
    guides: Vec<GuideEntry>,
}

#[derive(Serialize, Deserialize)]
struct GuideEntry {
    vertical: bool,
    pos: f32,
}

#[derive(Serialize, Deserialize)]
struct ChannelEntry {
    id: u64,
    name: String,
    offset: u64,
    stored_len: u64,
    raw_len: u64,
    crc32: u32,
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
    // Campos da versão 2 (ausentes em arquivos v1: valores padrão).
    #[serde(default)]
    kind: EntryKind,
    #[serde(default)]
    parent: Option<u64>,
    #[serde(default)]
    blend: BlendMode,
    #[serde(default = "one")]
    fill: f32,
    #[serde(default)]
    lock_pixels: bool,
    #[serde(default)]
    lock_alpha: bool,
    #[serde(default)]
    lock_position: bool,
    #[serde(default = "yes")]
    expanded: bool,
    // Versão 5.
    #[serde(default)]
    clipped: bool,
    #[serde(default)]
    mask: Option<MaskEntry>,
    /// Versão 6: origem editável (texto/forma) como JSON; lida de forma tolerante.
    #[serde(default)]
    source: Option<serde_json::Value>,
}

/// Máscara de camada (versão 5): recurso zlib de 1 byte por pixel do documento.
#[derive(Serialize, Deserialize)]
struct MaskEntry {
    offset: u64,
    stored_len: u64,
    raw_len: u64,
    crc32: u32,
    enabled: bool,
    linked: bool,
    density: f32,
    feather: f32,
    /// Versão 6: forma de origem de uma máscara vetorial.
    #[serde(default)]
    vector: Option<serde_json::Value>,
}

fn pack(bytes: &[u8], blob: &mut Vec<u8>) -> Result<(u64, u64, u32), OrvaError> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(3));
    enc.write_all(bytes)
        .map_err(|e| OrvaError::Corrupt(e.to_string()))?;
    let packed = enc
        .finish()
        .map_err(|e| OrvaError::Corrupt(e.to_string()))?;
    let offset = blob.len() as u64;
    blob.extend_from_slice(&packed);
    Ok((offset, packed.len() as u64, crc32fast::hash(bytes)))
}

fn one() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
enum EntryKind {
    #[default]
    Raster,
    Group,
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
            kind: if l.is_group() {
                EntryKind::Group
            } else {
                EntryKind::Raster
            },
            parent: l.parent.map(|p| p.0),
            blend: l.blend,
            fill: l.fill,
            lock_pixels: l.lock_pixels,
            lock_alpha: l.lock_alpha,
            lock_position: l.lock_position,
            expanded: l.expanded,
            clipped: l.clipped,
            mask: None,
            source: l.source.as_ref().and_then(|s| serde_json::to_value(s).ok()),
        });
        blob.extend_from_slice(&packed);
        if let Some(m) = &l.mask {
            let (offset, stored_len, crc32) = pack(m.data(), &mut blob)?;
            if let Some(last) = entries.last_mut() {
                last.mask = Some(MaskEntry {
                    offset,
                    stored_len,
                    raw_len: m.data().len() as u64,
                    crc32,
                    enabled: m.enabled,
                    linked: m.linked,
                    density: m.density(),
                    feather: m.feather(),
                    vector: m.vector_source().and_then(|s| serde_json::to_value(s).ok()),
                });
            }
        }
    }
    let mut channel_entries = Vec::with_capacity(doc.channels.len());
    for c in &doc.channels {
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(3));
        enc.write_all(&c.mask)
            .map_err(|e| OrvaError::Corrupt(e.to_string()))?;
        let packed = enc
            .finish()
            .map_err(|e| OrvaError::Corrupt(e.to_string()))?;
        channel_entries.push(ChannelEntry {
            id: c.id,
            name: c.name.clone(),
            offset: blob.len() as u64,
            stored_len: packed.len() as u64,
            raw_len: c.mask.len() as u64,
            crc32: crc32fast::hash(&c.mask),
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
        channels: channel_entries,
        guides: doc
            .guides
            .iter()
            .map(|g| GuideEntry {
                vertical: g.vertical,
                pos: g.pos,
            })
            .collect(),
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

    if manifest.channels.len() > MAX_CHANNELS {
        return Err(OrvaError::TooLarge("canais demais".into()));
    }
    let blob = &bytes[manifest_end..];
    let mut total: u64 = 0;
    let channel_len = u64::from(manifest.width) * u64::from(manifest.height);
    for c in &manifest.channels {
        if c.raw_len != channel_len {
            return Err(OrvaError::Corrupt(format!(
                "tamanho do canal \"{}\"",
                c.name
            )));
        }
        total = total.saturating_add(c.raw_len);
        if total > MAX_TOTAL_PIXEL_BYTES {
            return Err(OrvaError::TooLarge(
                "canais e camadas grandes demais".into(),
            ));
        }
        if c.offset
            .checked_add(c.stored_len)
            .is_none_or(|end| end > blob.len() as u64)
        {
            return Err(OrvaError::Corrupt(format!(
                "canal fora do arquivo: \"{}\"",
                c.name
            )));
        }
    }
    let mut seen = HashSet::new();
    for e in &manifest.layers {
        let is_group = e.kind == EntryKind::Group;
        let dims_ok = if is_group {
            e.width == 0 && e.height == 0
        } else {
            side_ok(e.width) && side_ok(e.height)
        };
        if !dims_ok {
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
        if !e.opacity.is_finite() || !e.fill.is_finite() {
            return Err(OrvaError::Corrupt("opacidade inválida".into()));
        }
        if let Some(me) = &e.mask {
            if me.raw_len != channel_len {
                return Err(OrvaError::Corrupt(format!(
                    "tamanho da máscara de \"{}\"",
                    e.name
                )));
            }
            total = total.saturating_add(me.raw_len);
            if total > MAX_TOTAL_PIXEL_BYTES {
                return Err(OrvaError::TooLarge(
                    "máscaras e camadas grandes demais".into(),
                ));
            }
            if me
                .offset
                .checked_add(me.stored_len)
                .is_none_or(|end| end > blob.len() as u64)
                || !me.density.is_finite()
                || !me.feather.is_finite()
            {
                return Err(OrvaError::Corrupt(format!(
                    "máscara inválida em \"{}\"",
                    e.name
                )));
            }
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
        let group = e.kind == EntryKind::Group;
        let mut layer = if group {
            Layer::group(e.name.clone())
        } else {
            Layer::from_pixels(e.name.clone(), e.width, e.height, pixels)
        };
        layer.id = LayerId(e.id);
        layer.kind = if group {
            LayerKind::Group
        } else {
            LayerKind::Raster
        };
        layer.parent = e.parent.map(LayerId);
        layer.visible = e.visible;
        layer.opacity = e.opacity.clamp(0.0, 1.0);
        layer.fill = e.fill.clamp(0.0, 1.0);
        layer.blend = if group || e.blend != BlendMode::PassThrough {
            e.blend
        } else {
            BlendMode::Normal
        };
        layer.lock_pixels = e.lock_pixels;
        layer.lock_alpha = e.lock_alpha;
        layer.lock_position = e.lock_position;
        layer.expanded = e.expanded;
        layer.clipped = e.clipped;
        // Origem inválida ou hostil é ignorada: a camada fica raster com os pixels gravados.
        layer.source = e
            .source
            .as_ref()
            .and_then(|v| serde_json::from_value::<LayerSource>(v.clone()).ok())
            .filter(LayerSource::is_sane);
        if let Some(me) = &e.mask {
            let packed = &blob[me.offset as usize..(me.offset + me.stored_len) as usize];
            let mut data = Vec::with_capacity(me.raw_len as usize);
            ZlibDecoder::new(packed)
                .take(me.raw_len + 1)
                .read_to_end(&mut data)
                .map_err(|err| OrvaError::Corrupt(format!("máscara de \"{}\": {err}", e.name)))?;
            if data.len() as u64 != me.raw_len || crc32fast::hash(&data) != me.crc32 {
                return Err(OrvaError::Corrupt(format!("máscara de \"{}\"", e.name)));
            }
            let mut m = LayerMask::new(data);
            m.enabled = me.enabled;
            m.linked = me.linked;
            m.set_density(me.density);
            m.set_feather(me.feather);
            if let Some(spec) = me
                .vector
                .as_ref()
                .and_then(|v| serde_json::from_value::<ShapeSpec>(v.clone()).ok())
                .filter(shape_sane)
            {
                let data = m.data().as_ref().clone();
                m.set_vector(spec, data);
            }
            layer.mask = Some(m);
        }
        layers.push(layer);
    }
    let layers = normalize(layers);
    let manifest_guides = manifest.guides;
    let mut channels = Vec::with_capacity(manifest.channels.len());
    for c in &manifest.channels {
        let packed = &blob[c.offset as usize..(c.offset + c.stored_len) as usize];
        let mut mask = Vec::with_capacity(c.raw_len as usize);
        ZlibDecoder::new(packed)
            .take(c.raw_len + 1)
            .read_to_end(&mut mask)
            .map_err(|err| OrvaError::Corrupt(format!("canal \"{}\": {err}", c.name)))?;
        if mask.len() as u64 != c.raw_len || crc32fast::hash(&mask) != c.crc32 {
            return Err(OrvaError::Corrupt(format!("canal \"{}\"", c.name)));
        }
        channels.push(Channel {
            id: c.id,
            name: c.name.clone(),
            mask: std::sync::Arc::new(mask),
        });
    }
    let mut doc = Document::from_parts(
        manifest.name,
        manifest.width,
        manifest.height,
        manifest.ppi,
        layers,
    );
    doc.channels = channels;
    doc.guides = manifest_guides
        .iter()
        .filter(|g| g.pos.is_finite())
        .take(10_000)
        .map(|g| Guide {
            vertical: g.vertical,
            pos: g.pos,
        })
        .collect();
    Ok(doc)
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
            assert_eq!((a.fill, a.blend), (b.fill, b.blend));
        }
        assert!(!back.modified);
    }

    #[test]
    fn groups_blend_locks_roundtrip() {
        let mut doc = Document::new("g", 3, 3, Some([1, 2, 3, 255])).unwrap();
        let mut g = Layer::group("Meu grupo");
        g.opacity = 0.7;
        g.expanded = false;
        let gid = g.id;
        let mut child = Layer::filled("filho", 3, 3, [9, 9, 9, 255]);
        child.parent = Some(gid);
        child.blend = BlendMode::Overlay;
        child.fill = 0.25;
        child.lock_alpha = true;
        child.lock_position = true;
        doc.layers.extend([g, child]);
        let back = read(&write(&doc).unwrap()).unwrap();
        assert_eq!(back.layers.len(), 3);
        let (bg, bc) = (&back.layers[1], &back.layers[2]);
        assert!(bg.is_group() && !bg.expanded && bg.opacity == 0.7);
        assert_eq!(bg.blend, BlendMode::PassThrough);
        assert_eq!(bc.parent, Some(gid));
        assert_eq!(bc.blend, BlendMode::Overlay);
        assert_eq!(bc.fill, 0.25);
        assert!(bc.lock_alpha && bc.lock_position && !bc.lock_pixels);
        assert_eq!(
            crate::compose::flatten(&back),
            crate::compose::flatten(&doc)
        );
    }

    #[test]
    fn reads_version_1_files_without_new_fields() {
        let px = vec![5u8, 6, 7, 255];
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(3));
        enc.write_all(&px).unwrap();
        let packed = enc.finish().unwrap();
        let manifest = serde_json::json!({
            "format": "orvalith-project", "app_version": "0.0.1", "name": "antigo",
            "width": 1, "height": 1, "ppi": 72.0,
            "layers": [{"id": 77, "name": "Fundo", "visible": true, "opacity": 0.5,
                "width": 1, "height": 1, "offset": 0, "stored_len": packed.len(),
                "raw_len": 4, "crc32": crc32fast::hash(&px)}]
        });
        let json = serde_json::to_vec(&manifest).unwrap();
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&crc32fast::hash(&json).to_le_bytes());
        out.extend_from_slice(&json);
        out.extend_from_slice(&packed);
        let doc = read(&out).unwrap();
        let l = &doc.layers[0];
        assert_eq!(
            (l.opacity, l.fill, l.blend, l.visible),
            (0.5, 1.0, BlendMode::Normal, true)
        );
        assert_eq!(l.pixel(0, 0), Some([5, 6, 7, 255]));
    }

    #[test]
    fn saved_selection_channels_roundtrip_and_are_validated() {
        let mut doc = Document::new("c", 5, 4, Some([1, 2, 3, 255])).unwrap();
        let mask: Vec<u8> = (0..20).map(|i| (i * 12) as u8).collect();
        doc.channels.push(Channel {
            id: 7,
            name: "Céu".into(),
            mask: std::sync::Arc::new(mask.clone()),
        });
        let bytes = write(&doc).unwrap();
        let back = read(&bytes).unwrap();
        assert_eq!(back.channels.len(), 1);
        assert_eq!(
            (back.channels[0].id, back.channels[0].name.as_str()),
            (7, "Céu")
        );
        assert_eq!(*back.channels[0].mask, mask);
        assert_eq!(back.new_channel_id(), 8);
        // Corrupção no final do blob (canal) é detectada.
        let mut bad = bytes.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xFF;
        assert!(read(&bad).is_err());
    }

    #[test]
    fn masks_clipping_roundtrip_and_validation() {
        let mut doc = Document::new("m", 4, 3, Some([1, 2, 3, 255])).unwrap();
        let mut top = Layer::filled("topo", 4, 3, [9, 9, 9, 255]);
        let mut m = LayerMask::new((0..12).map(|i| (i * 20) as u8).collect());
        m.enabled = false;
        m.linked = false;
        m.set_density(0.5);
        m.set_feather(2.0);
        top.mask = Some(m);
        top.clipped = true;
        doc.layers.push(top);
        let bytes = write(&doc).unwrap();
        let back = read(&bytes).unwrap();
        assert!(back.layers[0].mask.is_none() && !back.layers[0].clipped);
        let t = &back.layers[1];
        assert!(t.clipped);
        let bm = t.mask.as_ref().unwrap();
        assert_eq!(
            **bm.data(),
            (0..12).map(|i| (i * 20) as u8).collect::<Vec<u8>>()
        );
        assert!(!bm.enabled && !bm.linked);
        assert_eq!((bm.density(), bm.feather()), (0.5, 2.0));
        assert_eq!(
            crate::compose::flatten(&back),
            crate::compose::flatten(&doc)
        );
        // Corrupção do último recurso (máscara) é detectada.
        let mut bad = bytes;
        let last = bad.len() - 1;
        bad[last] ^= 0xFF;
        assert!(read(&bad).is_err());
    }

    #[test]
    fn guides_roundtrip_and_bad_values_are_dropped() {
        let mut doc = Document::new("g", 5, 4, Some([1, 2, 3, 255])).unwrap();
        doc.guides = vec![
            Guide {
                vertical: true,
                pos: 2.5,
            },
            Guide {
                vertical: false,
                pos: 1.0,
            },
        ];
        let back = read(&write(&doc).unwrap()).unwrap();
        assert_eq!(back.guides, doc.guides);
        let none = read(&write(&Document::new("g", 5, 4, None).unwrap()).unwrap()).unwrap();
        assert!(none.guides.is_empty());
    }

    #[test]
    fn repairs_unknown_parent_on_load() {
        let mut doc = Document::new("g", 2, 2, Some([1, 2, 3, 255])).unwrap();
        doc.layers[0].parent = Some(LayerId(424242));
        let back = read(&write(&doc).unwrap()).unwrap();
        assert_eq!(back.layers[0].parent, None);
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
    #[test]
    fn editable_sources_roundtrip_and_hostile_ones_are_ignored() {
        use crate::vector::{Geom, ShapeSpec};
        let mut doc = Document::new("v", 6, 6, Some([1, 2, 3, 255])).unwrap();
        let spec = ShapeSpec::solid(
            Geom::Rect {
                x: 1.0,
                y: 1.0,
                w: 3.0,
                h: 3.0,
                radius: 0.0,
            },
            [9, 9, 9, 255],
        );
        let mut l = Layer::from_pixels("forma", 6, 6, crate::vector::render_shape(&spec, 6, 6));
        l.source = Some(LayerSource::Shape(spec.clone()));
        doc.layers.push(l);
        let back = read(&write(&doc).unwrap()).unwrap();
        assert_eq!(back.layers[1].source, Some(LayerSource::Shape(spec)));
        assert_eq!(back.layers[1].pixels, doc.layers[1].pixels);
        // Origem desconhecida/corrompida no manifesto: a camada vira raster, sem erro.
        let mut hostile = Document::new("v", 6, 6, Some([1, 2, 3, 255])).unwrap();
        let bytes = write(&hostile).unwrap();
        let text = String::from_utf8_lossy(&bytes[HEADER_LEN..]).to_string();
        assert!(text.contains("\"source\":null"));
        hostile.layers[0].source = None;
        let man_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        let man = &bytes[HEADER_LEN..HEADER_LEN + man_len];
        let patched = String::from_utf8(man.to_vec()).unwrap().replacen(
            "\"source\":null",
            "\"source\":{\"Texto\":{\"x\":1}}",
            1,
        );
        let mut out = bytes[..12].to_vec();
        out.extend_from_slice(&(patched.len() as u32).to_le_bytes());
        out.extend_from_slice(&crc32fast::hash(patched.as_bytes()).to_le_bytes());
        out.extend_from_slice(patched.as_bytes());
        out.extend_from_slice(&bytes[HEADER_LEN + man_len..]);
        let loaded = read(&out).unwrap();
        assert!(loaded.layers[0].source.is_none());
        assert_eq!(loaded.layers[0].pixels, hostile.layers[0].pixels);
    }
}
