//! Adaptador de **leitura** do formato nativo `.pcraft` do PhotoCraft (projeto separado do `.orva`).
//!
//! Base: a documentação pública do repositório PhotoCraft (`docs/architecture.md` §9) e o modelo
//! serde do manifesto (`crates/format/src/manifest.rs`, `FORMAT_VERSION = 1`):
//! um zip (modo "store") com `manifest.json` e blocos `tiles/<blake3>.zst` (ladrilhos de 256×256,
//! amostras intercaladas little-endian, zstd; o nome é o hash BLAKE3 do conteúdo sem compressão).
//!
//! Subconjunto verificado: camadas raster e grupos RGB/cinza de 8 e 16 bits, visibilidade, opacidade,
//! preenchimento, modo de mesclagem, recorte, máscara e perfil ICC. Texto/formas/objetos inteligentes/
//! preenchimentos entram como pixels quando o arquivo traz o cache renderizado; ajustes e efeitos
//! são omitidos com aviso. **Escrita não está implementada**: o manifesto exige muitos campos sem
//! esquema público estável e não há como validá-los contra o aplicativo. A validação cruzada com o
//! PhotoCraft real está pendente (os testes usam arquivos sintéticos montados pela especificação).

use std::io::{Cursor, Read};

use serde_json::Value;

use crate::blend::BlendMode;
use crate::document::{Document, Layer, LayerMask};
use crate::imageio::Imported;
use crate::layerops::normalize;

const TILE: usize = 256;
const MAX_MANIFEST: u64 = 64 << 20;
const MAX_ENTRIES: usize = 2_000_000;
const MAX_TOTAL_BYTES: u64 = 2 << 30;
const MAX_LAYERS: usize = 2000;
const MAX_DEPTH: usize = 16;
/// Única versão do manifesto cujo esquema foi verificado.
pub const SUPPORTED_VERSION: u64 = 1;

const BLEND_NAMES: &[(&str, BlendMode)] = &[
    ("PassThrough", BlendMode::PassThrough),
    ("Normal", BlendMode::Normal),
    ("Dissolve", BlendMode::Dissolve),
    ("Darken", BlendMode::Darken),
    ("Multiply", BlendMode::Multiply),
    ("ColorBurn", BlendMode::ColorBurn),
    ("LinearBurn", BlendMode::LinearBurn),
    ("Lighten", BlendMode::Lighten),
    ("Screen", BlendMode::Screen),
    ("ColorDodge", BlendMode::ColorDodge),
    ("LinearDodge", BlendMode::LinearDodge),
    ("Overlay", BlendMode::Overlay),
    ("SoftLight", BlendMode::SoftLight),
    ("HardLight", BlendMode::HardLight),
    ("VividLight", BlendMode::VividLight),
    ("LinearLight", BlendMode::LinearLight),
    ("PinLight", BlendMode::PinLight),
    ("HardMix", BlendMode::HardMix),
    ("Difference", BlendMode::Difference),
    ("Exclusion", BlendMode::Exclusion),
    ("Subtract", BlendMode::Subtract),
    ("Divide", BlendMode::Divide),
    ("Hue", BlendMode::Hue),
    ("Saturation", BlendMode::Saturation),
    ("Color", BlendMode::Color),
    ("Luminosity", BlendMode::Luminosity),
];

type Res<T> = Result<T, String>;

struct Ctx<'a> {
    zip: zip::ZipArchive<Cursor<&'a [u8]>>,
    w: u32,
    h: u32,
    budget: u64,
    layers: Vec<Layer>,
    warnings: Vec<String>,
    notes: Vec<String>,
}

#[derive(Clone, Copy)]
struct Fmt {
    gray: bool,
    sixteen: bool,
    alpha: bool,
}

impl Fmt {
    fn parse(v: &Value) -> Res<Fmt> {
        let mode = v.get("mode").and_then(Value::as_str).unwrap_or("");
        let sample = v.get("sample").and_then(Value::as_str).unwrap_or("");
        let alpha = v.get("alpha").and_then(Value::as_bool).unwrap_or(false);
        let gray = match mode {
            "Rgb" => false,
            "Grayscale" => true,
            other => return Err(format!("modo de cor {other} não suportado")),
        };
        let sixteen = match sample {
            "U8" => false,
            "U16" => true,
            other => return Err(format!("amostra {other} não suportada")),
        };
        Ok(Fmt {
            gray,
            sixteen,
            alpha,
        })
    }

    fn bpp(self) -> usize {
        let ch = if self.gray { 1 } else { 3 } + usize::from(self.alpha);
        ch * if self.sixteen { 2 } else { 1 }
    }

    /// Converte um pixel (bytes little-endian intercalados) em RGBA8.
    fn px(self, b: &[u8]) -> [u8; 4] {
        let step = if self.sixteen { 2 } else { 1 };
        let s = |i: usize| b[i * step + step - 1]; // 16 bits: byte alto (little-endian)
        let n = if self.gray { 1 } else { 3 };
        let (r, g, bl) = if self.gray {
            (s(0), s(0), s(0))
        } else {
            (s(0), s(1), s(2))
        };
        [r, g, bl, if self.alpha { s(n) } else { 255 }]
    }
}

fn read_entry(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str, limit: u64) -> Res<Vec<u8>> {
    let f = zip.by_name(name).map_err(|e| format!("{name}: {e}"))?;
    if f.size() > limit {
        return Err(format!("{name}: grande demais."));
    }
    let mut out = Vec::new();
    f.take(limit)
        .read_to_end(&mut out)
        .map_err(|e| format!("{name}: {e}"))?;
    Ok(out)
}

fn hash_ok(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Lê e valida um recurso `<dir>/<hash>.zst`: descomprime (com limite) e confere o BLAKE3.
fn load_blob(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    dir: &str,
    hash: &str,
    max_raw: usize,
) -> Res<Vec<u8>> {
    if !hash_ok(hash) {
        return Err(format!(
            "hash inválido \"{}\"",
            hash.chars().take(20).collect::<String>()
        ));
    }
    let packed = read_entry(zip, &format!("{dir}/{}.zst", hash.to_lowercase()), 64 << 20)?;
    let mut dec =
        ruzstd::decoding::StreamingDecoder::new_with_max_window_size(packed.as_slice(), 64 << 20)
            .map_err(|e| format!("zstd: {e:?}"))?;
    let mut raw = Vec::new();
    std::io::Read::take(&mut dec, max_raw as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| format!("zstd: {e}"))?;
    if raw.len() > max_raw {
        return Err("recurso maior que o esperado".into());
    }
    if blake3::hash(&raw).to_hex().as_str() != hash.to_lowercase() {
        return Err(format!(
            "o hash BLAKE3 de {dir}/{hash} não confere (arquivo corrompido)"
        ));
    }
    Ok(raw)
}

/// Monta uma superfície (ladrilhos) em RGBA8 do tamanho do documento.
fn surface(c: &mut Ctx, v: &Value) -> Res<Vec<u8>> {
    let fmt = Fmt::parse(v.get("format").ok_or("superfície sem formato")?)?;
    let bpp = fmt.bpp();
    let cost = u64::from(c.w) * u64::from(c.h) * 4;
    if c.budget < cost {
        return Err("memória permitida excedida (cada camada tem o tamanho do documento)".into());
    }
    c.budget -= cost;
    let def_hex = v.get("default").and_then(Value::as_str).unwrap_or("");
    let def_bytes: Vec<u8> = (0..def_hex.len() / 2)
        .filter_map(|i| u8::from_str_radix(def_hex.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect();
    let def = if def_bytes.len() >= bpp {
        fmt.px(&def_bytes)
    } else {
        [0, 0, 0, 0]
    };
    let (w, h) = (c.w as usize, c.h as usize);
    let mut out = Vec::with_capacity(w * h * 4);
    for _ in 0..w * h {
        out.extend_from_slice(&def);
    }
    let tiles = v
        .get("tiles")
        .and_then(Value::as_array)
        .ok_or("superfície sem ladrilhos")?;
    if tiles.len() > MAX_ENTRIES {
        return Err("ladrilhos demais".into());
    }
    for t in tiles {
        let (tx, ty) = (
            t.get("tx")
                .and_then(Value::as_i64)
                .ok_or("ladrilho sem tx")?,
            t.get("ty")
                .and_then(Value::as_i64)
                .ok_or("ladrilho sem ty")?,
        );
        let hash = t
            .get("hash")
            .and_then(Value::as_str)
            .ok_or("ladrilho sem hash")?;
        // Ladrilhos fora do documento não precisam ser lidos.
        let (ox, oy) = (tx * TILE as i64, ty * TILE as i64);
        if ox >= w as i64 || oy >= h as i64 || ox + TILE as i64 <= 0 || oy + TILE as i64 <= 0 {
            continue;
        }
        let raw = load_blob(&mut c.zip, "tiles", hash, TILE * TILE * bpp)?;
        if raw.len() != TILE * TILE * bpp {
            return Err("ladrilho com tamanho inesperado".into());
        }
        for y in 0..TILE {
            let dy = oy + y as i64;
            if dy < 0 || dy >= h as i64 {
                continue;
            }
            for x in 0..TILE {
                let dx = ox + x as i64;
                if dx < 0 || dx >= w as i64 {
                    continue;
                }
                let p = fmt.px(&raw[(y * TILE + x) * bpp..(y * TILE + x + 1) * bpp]);
                let d = (dy as usize * w + dx as usize) * 4;
                out[d..d + 4].copy_from_slice(&p);
            }
        }
    }
    Ok(out)
}

fn walk(
    c: &mut Ctx,
    layers: &[Value],
    parent: Option<crate::document::LayerId>,
    depth: usize,
) -> Res<()> {
    if depth > MAX_DEPTH {
        return Err("grupos aninhados demais".into());
    }
    for l in layers {
        if c.layers.len() >= MAX_LAYERS {
            return Err("camadas demais".into());
        }
        let name: String = l
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Camada")
            .chars()
            .take(120)
            .collect();
        let content = l
            .get("content")
            .ok_or_else(|| format!("camada \"{name}\" sem conteúdo"))?;
        let kind = content.get("kind").and_then(Value::as_str).unwrap_or("");
        let blend_name = l.get("blend").and_then(Value::as_str).unwrap_or("Normal");
        let blend = match BLEND_NAMES.iter().find(|(n, _)| *n == blend_name) {
            Some((_, b)) => *b,
            None => {
                c.notes.push(format!(
                    "modo \"{blend_name}\" de \"{name}\" não existe aqui (usado Normal)"
                ));
                BlendMode::Normal
            }
        };
        let num = |k: &str, d: f64| {
            l.get(k)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
                .unwrap_or(d)
        };
        let apply = |ly: &mut Layer| {
            ly.visible = l.get("visible").and_then(Value::as_bool).unwrap_or(true);
            ly.opacity = num("opacity", 1.0).clamp(0.0, 1.0) as f32;
            ly.fill = num("fill_opacity", 1.0).clamp(0.0, 1.0) as f32;
            ly.clipped = l.get("clipped").and_then(Value::as_bool).unwrap_or(false);
            ly.parent = parent;
        };
        if l.pointer("/effects/items")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
        {
            c.notes
                .push(format!("efeitos de camada de \"{name}\" não importados"));
        }
        if kind == "group" {
            let mut g = Layer::group(name);
            apply(&mut g);
            g.clipped = false;
            g.expanded = content
                .get("expanded")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            g.blend = if blend_name == "PassThrough" {
                BlendMode::PassThrough
            } else {
                blend
            };
            let gid = g.id;
            c.layers.push(g);
            let kids = content
                .get("children")
                .and_then(Value::as_array)
                .ok_or("grupo sem filhos")?;
            walk(c, kids, Some(gid), depth + 1)?;
            continue;
        }
        // Fonte dos pixels: a superfície da camada raster ou o cache renderizado dos demais tipos.
        let (surf, note): (Option<&Value>, Option<&str>) = match kind {
            "raster" => (content.get("surface"), None),
            "text" => (
                content.get("cache").filter(|v| v.get("tiles").is_some()),
                Some("texto"),
            ),
            "shape" => (
                content.get("cache").filter(|v| v.get("tiles").is_some()),
                Some("forma"),
            ),
            "smart" => (
                content.get("cache").filter(|v| v.get("tiles").is_some()),
                Some("objeto inteligente"),
            ),
            "fill" => (
                l.get("fill_cache").filter(|v| v.get("tiles").is_some()),
                Some("preenchimento"),
            ),
            "adjustment" => {
                c.notes
                    .push(format!("camada de ajuste \"{name}\" omitida (sem pixels)"));
                continue;
            }
            other => {
                c.notes
                    .push(format!("camada \"{name}\" de tipo \"{other}\" omitida"));
                continue;
            }
        };
        let Some(sv) = surf else {
            c.notes
                .push(format!("camada \"{name}\" sem pixels legíveis omitida"));
            continue;
        };
        let px = match surface(c, sv) {
            Ok(p) => p,
            Err(e) if note.is_some() => {
                c.notes.push(format!(
                    "camada \"{name}\" ({}) omitida: {e}",
                    note.unwrap_or("")
                ));
                continue;
            }
            Err(e) => return Err(format!("camada \"{name}\": {e}")),
        };
        if let Some(n) = note {
            c.notes.push(format!(
                "\"{name}\": {n} importado como pixels (não editável)"
            ));
        }
        let mut ly = Layer::from_pixels(name.clone(), c.w, c.h, px);
        apply(&mut ly);
        ly.blend = if blend == BlendMode::PassThrough {
            BlendMode::Normal
        } else {
            blend
        };
        if let Some(m) = l.get("mask").filter(|m| !m.is_null()) {
            if let Some(ms) = m.get("surface") {
                match surface(c, ms) {
                    Ok(mp) => {
                        let data: Vec<u8> = mp.as_chunks::<4>().0.iter().map(|p| p[0]).collect();
                        let mut lm = LayerMask::new(data);
                        lm.enabled = m.get("enabled").and_then(Value::as_bool).unwrap_or(true);
                        lm.linked = m.get("linked").and_then(Value::as_bool).unwrap_or(true);
                        lm.set_density(
                            m.get("density").and_then(Value::as_f64).unwrap_or(1.0) as f32
                        );
                        lm.set_feather(
                            m.get("feather").and_then(Value::as_f64).unwrap_or(0.0) as f32
                        );
                        ly.mask = Some(lm);
                    }
                    Err(e) => c.notes.push(format!("máscara de \"{name}\" omitida: {e}")),
                }
            }
        }
        c.layers.push(ly);
    }
    Ok(())
}

/// Lê um `.pcraft` (somente leitura; subconjunto documentado no módulo).
pub fn read(bytes: &[u8], name: &str) -> Result<Imported, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("Não é um .pcraft válido (zip ilegível: {e})."))?;
    let manifest_bytes = read_entry(&mut zip, "manifest.json", MAX_MANIFEST)
        .map_err(|e| format!("Não é um .pcraft válido: {e}"))?;
    let m: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| format!("Manifesto .pcraft inválido: {e}"))?;
    let version = m
        .get("format_version")
        .and_then(Value::as_u64)
        .ok_or("Manifesto sem format_version.")?;
    if version != SUPPORTED_VERSION {
        return Err(format!(
            "Versão {version} do .pcraft não suportada: só a versão {SUPPORTED_VERSION} do manifesto foi verificada (o formato está em desenvolvimento no PhotoCraft)."
        ));
    }
    let d = m.get("document").ok_or("Manifesto sem documento.")?;
    let dim = |k: &str| {
        d.pointer(&format!("/size/{k}"))
            .and_then(Value::as_u64)
            .filter(|v| (1..=u64::from(Document::MAX_SIDE)).contains(v))
            .map(|v| v as u32)
    };
    let (w, h) = (
        dim("width").ok_or("Largura do documento inválida.")?,
        dim("height").ok_or("Altura do documento inválida.")?,
    );
    let mode = d.get("mode").and_then(Value::as_str).unwrap_or("Rgb");
    if !matches!(mode, "Rgb" | "Grayscale") {
        return Err(format!(
            "Modo de cor {mode} do .pcraft não suportado (apenas RGB e tons de cinza)."
        ));
    }
    let depth = d.get("depth").and_then(Value::as_str).unwrap_or("U8");
    let mut c = Ctx {
        zip,
        w,
        h,
        budget: MAX_TOTAL_BYTES,
        layers: Vec::new(),
        warnings: Vec::new(),
        notes: Vec::new(),
    };
    if depth == "U16" {
        c.warnings.push(
            "O documento tem 16 bits por canal: convertido para 8 bits (precisão reduzida).".into(),
        );
    } else if depth != "U8" {
        return Err(format!(
            "Profundidade {depth} do .pcraft não suportada (apenas 8 e 16 bits)."
        ));
    }
    let layers = d
        .get("layers")
        .and_then(Value::as_array)
        .ok_or("Documento sem camadas.")?;
    walk(&mut c, layers, None, 0)?;
    if c.layers.is_empty() {
        return Err("O .pcraft não tem camadas legíveis.".into());
    }
    let icc = d
        .get("icc_profile")
        .and_then(Value::as_str)
        .filter(|h| !h.is_empty())
        .map(str::to_string);
    let doc_name = d
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(name)
        .chars()
        .take(120)
        .collect::<String>();
    let ppi = d
        .get("resolution_dpi")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 1.0)
        .unwrap_or(72.0) as f32;
    let layers = std::mem::take(&mut c.layers);
    let mut doc = Document::from_parts(doc_name, w, h, ppi, normalize(layers));
    let mut warnings = std::mem::take(&mut c.warnings);
    if let Some(hash) = icc {
        match load_blob(&mut c.zip, "blobs", &hash, 16 << 20)
            .and_then(|b| crate::color::profile_from_bytes(&b))
        {
            Ok(p) => doc.icc = Some(p),
            Err(e) => warnings.push(format!(
                "Perfil de cor do .pcraft não importado ({e}); a imagem é tratada como sRGB."
            )),
        }
    }
    if !c.notes.is_empty() {
        c.notes.sort();
        c.notes.dedup();
        warnings.push(format!(
            "Recursos do PhotoCraft não suportados ou limitados: {}. Salve como .orva para manter o trabalho; o .pcraft original não é alterado.",
            c.notes.join("; ")
        ));
    }
    warnings.push("Leitura de .pcraft (somente leitura, manifesto versão 1): camadas raster, grupos, máscaras, opacidade e modos de mesclagem. Validação cruzada com o PhotoCraft real pendente.".into());
    Ok(Imported {
        document: doc,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Constrói um `.pcraft` sintético conforme a especificação pública (versão 1).
    struct Builder {
        zw: zip::ZipWriter<Cursor<Vec<u8>>>,
        added: std::collections::HashSet<String>,
    }

    impl Builder {
        fn new() -> Self {
            Self {
                zw: zip::ZipWriter::new(Cursor::new(Vec::new())),
                added: Default::default(),
            }
        }
        fn put(&mut self, path: &str, data: &[u8]) {
            if self.added.insert(path.to_string()) {
                let o = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored);
                self.zw.start_file(path, o).unwrap();
                self.zw.write_all(data).unwrap();
            }
        }
        /// Grava um ladrilho e devolve o hash.
        fn tile(&mut self, raw: &[u8]) -> String {
            let hash = blake3::hash(raw).to_hex().to_string();
            let z =
                ruzstd::encoding::compress_to_vec(raw, ruzstd::encoding::CompressionLevel::Fastest);
            self.put(&format!("tiles/{hash}.zst"), &z);
            hash
        }
        fn finish(mut self, manifest: &Value) -> Vec<u8> {
            self.put("manifest.json", manifest.to_string().as_bytes());
            self.zw.finish().unwrap().into_inner()
        }
    }

    fn rgba8() -> Value {
        serde_json::json!({"mode": "Rgb", "sample": "U8", "alpha": true})
    }

    /// Ladrilho 256×256 RGBA8 com um retângulo colorido em (x0..x1, y0..y1).
    fn tile_with(rect: (usize, usize, usize, usize), color: [u8; 4]) -> Vec<u8> {
        let mut raw = vec![0u8; TILE * TILE * 4];
        for y in rect.1..rect.3 {
            for x in rect.0..rect.2 {
                raw[(y * TILE + x) * 4..(y * TILE + x) * 4 + 4].copy_from_slice(&color);
            }
        }
        raw
    }

    fn layer(id: u64, name: &str, content: Value) -> Value {
        serde_json::json!({
            "id": id, "name": name, "visible": true, "blend": "Normal", "opacity": 1.0,
            "fill_opacity": 1.0, "clipped": false, "mask": null, "content": content,
            "effects": {"enabled": true, "items": []}
        })
    }

    fn sample() -> Vec<u8> {
        let mut b = Builder::new();
        let red = b.tile(&tile_with((10, 5, 30, 25), [220, 20, 20, 255]));
        let blue = b.tile(&tile_with((0, 0, 256, 256), [20, 20, 220, 255]));
        let mut mask_raw = vec![0u8; TILE * TILE];
        for y in 0..TILE {
            for x in 0..20 {
                mask_raw[y * TILE + x] = 255;
            }
        }
        let mask = b.tile(&mask_raw);
        let surf = |hash: &str| serde_json::json!({"format": rgba8(), "default": "00000000", "tiles": [{"tx": 0, "ty": 0, "hash": hash}]});
        let mut red_layer = layer(
            1,
            "Vermelho",
            serde_json::json!({"kind": "raster", "surface": surf(&red)}),
        );
        red_layer["opacity"] = serde_json::json!(0.5);
        red_layer["blend"] = serde_json::json!("Multiply");
        red_layer["mask"] = serde_json::json!({
            "surface": {"format": {"mode": "Grayscale", "sample": "U8", "alpha": false}, "default": "00", "tiles": [{"tx": 0, "ty": 0, "hash": mask}]},
            "enabled": true, "linked": true, "density": 1.0, "feather": 0.0
        });
        let blue_layer = layer(
            2,
            "Azul",
            serde_json::json!({"kind": "raster", "surface": surf(&blue)}),
        );
        let mut hidden = layer(
            3,
            "Oculta",
            serde_json::json!({"kind": "raster", "surface": surf(&red)}),
        );
        hidden["visible"] = serde_json::json!(false);
        let group = layer(
            4,
            "Grupo",
            serde_json::json!({"kind": "group", "children": [hidden], "expanded": true}),
        );
        let adj = layer(
            5,
            "Ajuste",
            serde_json::json!({"kind": "adjustment", "adjustment": {}}),
        );
        let manifest = serde_json::json!({
            "format_version": 1, "generator": "teste",
            "document": {"id": 1, "name": "Cartaz", "size": {"width": 40, "height": 30}, "resolution_dpi": 150.0,
                "mode": "Rgb", "depth": "U8", "icc_profile": null, "layers": [blue_layer, red_layer, group, adj]}
        });
        b.finish(&manifest)
    }

    #[test]
    fn reads_layers_groups_masks_blend_and_reports_unsupported_features() {
        let imp = read(&sample(), "x").unwrap();
        let d = &imp.document;
        assert_eq!((d.width, d.height, d.name.as_str()), (40, 30, "Cartaz"));
        assert_eq!(d.ppi, 150.0);
        let names: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["Azul", "Vermelho", "Grupo", "Oculta"]);
        assert_eq!(
            d.layers[0].pixel(39, 29),
            Some([20, 20, 220, 255]),
            "ladrilho cobre o documento recortado"
        );
        let red = &d.layers[1];
        assert_eq!(red.pixel(15, 10), Some([220, 20, 20, 255]));
        assert_eq!(
            red.pixel(5, 5),
            Some([0, 0, 0, 0]),
            "pixel padrão fora do retângulo"
        );
        assert!((red.opacity - 0.5).abs() < 1e-6 && red.blend == BlendMode::Multiply);
        let m = red.mask.as_ref().unwrap();
        assert_eq!((m.data()[0], m.data()[25]), (255, 0));
        assert!(
            d.layers[2].is_group()
                && d.layers[3].parent == Some(d.layers[2].id)
                && !d.layers[3].visible
        );
        assert!(
            imp.warnings
                .iter()
                .any(|w| w.contains("ajuste") && w.contains("Ajuste")),
            "{:?}",
            imp.warnings
        );
        assert!(imp.warnings.iter().any(|w| w.contains("pendente")));
    }

    #[test]
    fn text_cache_becomes_pixels_and_sixteen_bit_is_reduced() {
        let mut b = Builder::new();
        // RGBA16 little-endian: valor 0x7F00 → byte alto 0x7F.
        let mut raw = vec![0u8; TILE * TILE * 8];
        for p in raw.as_chunks_mut::<8>().0 {
            p.copy_from_slice(&[0x00, 0x7F, 0x00, 0x40, 0x00, 0x20, 0x00, 0xFF]);
        }
        let hash = b.tile(&raw);
        let fmt16 = serde_json::json!({"mode": "Rgb", "sample": "U16", "alpha": true});
        let cache = serde_json::json!({"format": fmt16, "default": "0000000000000000", "tiles": [{"tx": 0, "ty": 0, "hash": hash}]});
        let text = layer(
            1,
            "Título",
            serde_json::json!({"kind": "text", "text": "Oi", "cache": cache}),
        );
        let manifest = serde_json::json!({"format_version": 1, "generator": "t",
            "document": {"id": 1, "name": "T", "size": {"width": 8, "height": 8}, "resolution_dpi": 72.0, "mode": "Rgb", "depth": "U16", "icc_profile": null, "layers": [text]}});
        let imp = read(&b.finish(&manifest), "x").unwrap();
        assert_eq!(
            imp.document.layers[0].pixel(3, 3),
            Some([0x7F, 0x40, 0x20, 0xFF])
        );
        assert!(imp.warnings.iter().any(|w| w.contains("16 bits")));
        assert!(imp
            .warnings
            .iter()
            .any(|w| w.contains("texto importado como pixels")));
    }

    #[test]
    fn hostile_and_unsupported_files_fail_cleanly() {
        assert!(read(b"nao e zip", "x").is_err());
        let mk = |m: Value| Builder::new().finish(&m);
        let base =
            |v: Value| serde_json::json!({"format_version": 1, "generator": "t", "document": v});
        assert!(read(
            &mk(serde_json::json!({"format_version": 2, "document": {}})),
            "x"
        )
        .unwrap_err()
        .contains("versão"));
        let e = read(&mk(base(serde_json::json!({"size": {"width": 4, "height": 4}, "mode": "Cmyk", "depth": "U8", "layers": []}))), "x").unwrap_err();
        assert!(e.contains("Cmyk"));
        let e = read(&mk(base(serde_json::json!({"size": {"width": 99999, "height": 4}, "mode": "Rgb", "depth": "U8", "layers": []}))), "x").unwrap_err();
        assert!(e.contains("Largura"));
        // Ladrilho adulterado: o hash BLAKE3 não confere.
        let mut b = Builder::new();
        let good = tile_with((0, 0, 4, 4), [1, 2, 3, 255]);
        let hash = blake3::hash(&good).to_hex().to_string();
        let mut other = good.clone();
        other[0] ^= 1;
        b.put(
            &format!("tiles/{hash}.zst"),
            &ruzstd::encoding::compress_to_vec(
                other.as_slice(),
                ruzstd::encoding::CompressionLevel::Fastest,
            ),
        );
        let surf = serde_json::json!({"format": rgba8(), "default": "00000000", "tiles": [{"tx": 0, "ty": 0, "hash": hash}]});
        let m = base(
            serde_json::json!({"size": {"width": 8, "height": 8}, "mode": "Rgb", "depth": "U8",
            "layers": [layer(1, "a", serde_json::json!({"kind": "raster", "surface": surf}))]}),
        );
        let e = read(&b.finish(&m), "x").unwrap_err();
        assert!(e.contains("BLAKE3"), "{e}");
        // Ladrilho ausente e hash malformado.
        let surf = serde_json::json!({"format": rgba8(), "default": "00000000", "tiles": [{"tx": 0, "ty": 0, "hash": "xyz"}]});
        let m = base(
            serde_json::json!({"size": {"width": 8, "height": 8}, "mode": "Rgb", "depth": "U8",
            "layers": [layer(1, "a", serde_json::json!({"kind": "raster", "surface": surf}))]}),
        );
        assert!(read(&mk(m), "x").is_err());
        // Aninhamento absurdo de grupos.
        let mut l = layer(
            1,
            "x",
            serde_json::json!({"kind": "group", "children": [], "expanded": true}),
        );
        for i in 0..24 {
            l = layer(
                i + 2,
                "g",
                serde_json::json!({"kind": "group", "children": [l], "expanded": true}),
            );
        }
        let m = base(
            serde_json::json!({"size": {"width": 4, "height": 4}, "mode": "Rgb", "depth": "U8", "layers": [l]}),
        );
        assert!(read(&mk(m), "x").unwrap_err().contains("aninhados"));
        // Truncar o zip em qualquer ponto não causa pânico.
        let good = sample();
        for cut in (0..good.len()).step_by(97) {
            let _ = read(&good[..cut], "x");
        }
    }
}
