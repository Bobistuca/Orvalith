//! Registro de formatos de arquivo, detecção por conteúdo, importação e exportação.
//!
//! "Suportado" só vale para o subconjunto verificado em testes (ver docs/FORMAT_SUPPORT.md).
//! Detectar uma extensão não é suporte: cada formato abaixo tem codec e teste próprios.

use std::io::{Cursor, Read, Write};

use image::{ExtendedColorType, ImageEncoder};

use crate::blend::BlendMode;
use crate::compose;
use crate::document::{Document, Layer, LayerId, LayerKind};
use crate::imageio::{self, ColorOut, Imported};
use crate::layerops::normalize;
use crate::transform::{self, Interp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Orva,
    Png,
    Jpeg,
    Webp,
    Gif,
    Bmp,
    Tiff,
    Tga,
    Ico,
    Qoi,
    Pnm,
    Hdr,
    Exr,
    Svg,
    Ora,
    Psd,
    Psb,
    Pdf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Yes,
    Partial,
    No,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Yes => "sim",
            Level::Partial => "parcial",
            Level::No => "não",
        }
    }
}

pub struct FormatInfo {
    pub format: Format,
    pub name: &'static str,
    pub exts: &'static [&'static str],
    pub read: Level,
    pub write: Level,
    pub note: &'static str,
}

/// Tabela real de suporte (a mesma que docs/FORMAT_SUPPORT.md descreve).
pub const FORMATS: &[FormatInfo] = &[
    FormatInfo { format: Format::Orva, name: "Projeto Orvalith", exts: &["orva"], read: Level::Yes, write: Level::Yes, note: "formato nativo com todas as camadas e recursos editáveis" },
    FormatInfo { format: Format::Png, name: "PNG", exts: &["png"], read: Level::Partial, write: Level::Partial, note: "8 bits com alfa e perfil ICC; 16 bits são reduzidos a 8; APNG: só o 1º quadro" },
    FormatInfo { format: Format::Jpeg, name: "JPEG", exts: &["jpg", "jpeg", "jpe", "jfif"], read: Level::Partial, write: Level::Partial, note: "qualidade 1–100, orientação EXIF na leitura, perfil ICC; sem EXIF na saída; sem alfa (composto sobre um fundo)" },
    FormatInfo { format: Format::Webp, name: "WebP", exts: &["webp"], read: Level::Partial, write: Level::Partial, note: "lê com e sem perdas e alfa; grava só sem perdas; animação: só o 1º quadro" },
    FormatInfo { format: Format::Gif, name: "GIF", exts: &["gif"], read: Level::Partial, write: Level::Partial, note: "estático: lê o 1º quadro; grava 256 cores e transparência binária" },
    FormatInfo { format: Format::Bmp, name: "BMP", exts: &["bmp"], read: Level::Partial, write: Level::Partial, note: "32 bits com alfa na gravação; leitura do subconjunto do codec" },
    FormatInfo { format: Format::Tiff, name: "TIFF", exts: &["tif", "tiff"], read: Level::Partial, write: Level::Partial, note: "8 bits RGBA sem compressão na gravação; uma página; sem camadas" },
    FormatInfo { format: Format::Tga, name: "TGA", exts: &["tga"], read: Level::Partial, write: Level::Partial, note: "subconjunto do codec; detectado só pela extensão" },
    FormatInfo { format: Format::Ico, name: "ICO", exts: &["ico"], read: Level::Partial, write: Level::Partial, note: "lê a maior imagem; grava uma imagem de até 256×256" },
    FormatInfo { format: Format::Qoi, name: "QOI", exts: &["qoi"], read: Level::Yes, write: Level::Yes, note: "8 bits com alfa" },
    FormatInfo { format: Format::Pnm, name: "PNM (PBM/PGM/PPM)", exts: &["pnm", "pbm", "pgm", "ppm"], read: Level::Partial, write: Level::Partial, note: "grava PPM binário sem alfa (fundo composto)" },
    FormatInfo { format: Format::Hdr, name: "Radiance HDR", exts: &["hdr"], read: Level::Partial, write: Level::Partial, note: "lê convertendo para sRGB de 8 bits; grava a imagem de 8 bits como linear f32 (sem faixa dinâmica extra)" },
    FormatInfo { format: Format::Exr, name: "OpenEXR", exts: &["exr"], read: Level::Partial, write: Level::Partial, note: "idem HDR; uma camada de imagem; sem canais extras" },
    FormatInfo { format: Format::Svg, name: "SVG", exts: &["svg"], read: Level::Partial, write: Level::No, note: "importa rasterizando (vira camada raster, com aviso); sem exportação vetorial" },
    FormatInfo { format: Format::Ora, name: "OpenRaster", exts: &["ora"], read: Level::Partial, write: Level::Partial, note: "camadas, grupos, visibilidade, opacidade, modos básicos; sem máscaras, estilos ou ajustes" },
    FormatInfo { format: Format::Psd, name: "Photoshop PSD", exts: &["psd"], read: Level::No, write: Level::No, note: "subetapa 12.3" },
    FormatInfo { format: Format::Psb, name: "Photoshop PSB", exts: &["psb"], read: Level::No, write: Level::No, note: "subetapa 12.3" },
    FormatInfo { format: Format::Pdf, name: "PDF", exts: &["pdf"], read: Level::Partial, write: Level::Partial, note: "importa páginas rasterizadas (Arquivo → Importar PDF) e exporta PDF raster multipágina (Arquivo → Exportar PDF); texto e vetores não ficam editáveis" },
];

/// Extensões que não têm suporte e o motivo (aparece nas mensagens e na documentação).
pub const UNSUPPORTED: &[(&str, &str)] = &[
    (
        "avif",
        "exige decodificador/codificador AV1 externo (dav1d/rav1e); não incluído",
    ),
    (
        "heic",
        "exige libheif/HEVC (licenciamento e dependência nativa); não incluído",
    ),
    (
        "heif",
        "exige libheif/HEVC (licenciamento e dependência nativa); não incluído",
    ),
    (
        "jxl",
        "o codec JPEG XL de referência é nativo e extenso; não incluído",
    ),
    (
        "jp2",
        "o codec JPEG 2000 não tem uma opção leve verificada; não incluído",
    ),
    (
        "dng",
        "revelação de RAW exige biblioteca própria por câmera; não incluído",
    ),
    (
        "cr2",
        "revelação de RAW exige biblioteca própria por câmera; não incluído",
    ),
    (
        "cr3",
        "revelação de RAW exige biblioteca própria por câmera; não incluído",
    ),
    (
        "nef",
        "revelação de RAW exige biblioteca própria por câmera; não incluído",
    ),
    (
        "arw",
        "revelação de RAW exige biblioteca própria por câmera; não incluído",
    ),
    (
        "raf",
        "revelação de RAW exige biblioteca própria por câmera; não incluído",
    ),
    (
        "orf",
        "revelação de RAW exige biblioteca própria por câmera; não incluído",
    ),
    (
        "rw2",
        "revelação de RAW exige biblioteca própria por câmera; não incluído",
    ),
    (
        "pcraft",
        "não há especificação pública verificável do formato; ver docs/FORMAT_SUPPORT.md",
    ),
];

pub fn info(f: Format) -> &'static FormatInfo {
    FORMATS
        .iter()
        .find(|i| i.format == f)
        .expect("formato registrado")
}

impl Format {
    pub fn from_ext(ext: &str) -> Option<Format> {
        let e = ext.trim_start_matches('.').to_lowercase();
        FORMATS
            .iter()
            .find(|i| i.exts.contains(&e.as_str()))
            .map(|i| i.format)
    }

    pub fn can_read(self) -> bool {
        info(self).read != Level::No
    }

    pub fn can_write(self) -> bool {
        info(self).write != Level::No
    }

    pub fn default_ext(self) -> &'static str {
        info(self).exts[0]
    }

    fn image_format(self) -> Option<image::ImageFormat> {
        use image::ImageFormat as I;
        Some(match self {
            Format::Png => I::Png,
            Format::Jpeg => I::Jpeg,
            Format::Webp => I::WebP,
            Format::Gif => I::Gif,
            Format::Bmp => I::Bmp,
            Format::Tiff => I::Tiff,
            Format::Tga => I::Tga,
            Format::Ico => I::Ico,
            Format::Qoi => I::Qoi,
            Format::Pnm => I::Pnm,
            Format::Hdr => I::Hdr,
            Format::Exr => I::OpenExr,
            _ => return None,
        })
    }

    /// Formatos que a janela de exportação oferece.
    pub fn exportable() -> Vec<Format> {
        FORMATS
            .iter()
            .filter(|i| i.write != Level::No && i.format != Format::Orva)
            .map(|i| i.format)
            .collect()
    }
}

/// Reconhece o formato pelo conteúdo (assinatura); a extensão só decide TGA, que não tem assinatura.
pub fn detect(bytes: &[u8], ext: &str) -> Option<Format> {
    let b = bytes;
    let starts = |p: &[u8]| b.starts_with(p);
    if starts(b"\x89PNG\r\n\x1a\n") {
        return Some(Format::Png);
    }
    if starts(&[0xFF, 0xD8, 0xFF]) {
        return Some(Format::Jpeg);
    }
    if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        return Some(Format::Webp);
    }
    if starts(b"GIF87a") || starts(b"GIF89a") {
        return Some(Format::Gif);
    }
    if starts(b"BM") && Format::from_ext(ext) != Some(Format::Pnm) {
        return Some(Format::Bmp);
    }
    if starts(b"II*\0") || starts(b"MM\0*") || starts(b"II+\0") || starts(b"MM\0+") {
        return Some(Format::Tiff);
    }
    if starts(&[0, 0, 1, 0]) {
        return Some(Format::Ico);
    }
    if starts(b"qoif") {
        return Some(Format::Qoi);
    }
    if b.len() > 2 && b[0] == b'P' && (b'1'..=b'6').contains(&b[1]) && b[2].is_ascii_whitespace() {
        return Some(Format::Pnm);
    }
    if starts(b"#?RADIANCE") || starts(b"#?RGBE") {
        return Some(Format::Hdr);
    }
    if starts(&[0x76, 0x2F, 0x31, 0x01]) {
        return Some(Format::Exr);
    }
    if starts(b"8BPS") {
        return Some(if b.len() > 5 && b[5] == 2 {
            Format::Psb
        } else {
            Format::Psd
        });
    }
    if starts(b"%PDF") {
        return Some(Format::Pdf);
    }
    if starts(b"ORVA") || starts(b"ORVALITH") {
        return Some(Format::Orva);
    }
    if starts(b"PK\x03\x04") && Format::from_ext(ext) == Some(Format::Ora) {
        return Some(Format::Ora);
    }
    let head = String::from_utf8_lossy(&b[..b.len().min(2048)]).to_lowercase();
    if head.contains("<svg") {
        return Some(Format::Svg);
    }
    match Format::from_ext(ext) {
        Some(Format::Tga) => Some(Format::Tga),
        _ => None,
    }
}

/// Mensagem para uma extensão sem suporte (ou `None` se o formato é conhecido).
pub fn unsupported_reason(ext: &str) -> Option<String> {
    let e = ext.trim_start_matches('.').to_lowercase();
    UNSUPPORTED
        .iter()
        .find(|(x, _)| *x == e)
        .map(|(x, why)| format!("Formato .{x} não suportado: {why}."))
}

// ---------------------------------------------------------------------------------------
// Importação

/// Abre um arquivo de imagem/projeto de camadas pelo conteúdo. `.orva` e PDF têm fluxos próprios.
pub fn import(bytes: &[u8], name: &str, ext: &str) -> Result<Imported, String> {
    if let Some(msg) = unsupported_reason(ext) {
        return Err(msg);
    }
    let Some(f) = detect(bytes, ext) else {
        return Err(format!(
            "Formato não reconhecido (.{ext}). Veja docs/FORMAT_SUPPORT.md."
        ));
    };
    match f {
        Format::Svg => import_svg(bytes, name),
        Format::Ora => import_ora(bytes, name),
        Format::Pdf => {
            Err("PDF: use Arquivo → Importar PDF… (escolha de páginas e resolução).".into())
        }
        Format::Orva => Err("Projeto Orvalith: abra com Arquivo → Abrir.".into()),
        Format::Psd | Format::Psb => Err("PSD/PSB ainda não suportado nesta versão.".into()),
        raster => {
            let fmt = raster.image_format().ok_or("Formato sem codec.")?;
            imageio::import_as(bytes, name, Some(fmt))
        }
    }
}

fn import_svg(bytes: &[u8], name: &str) -> Result<Imported, String> {
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree =
        resvg::usvg::Tree::from_data(bytes, &opt).map_err(|e| format!("SVG inválido: {e}"))?;
    let size = tree.size();
    let (mut w, mut h) = (size.width().max(1.0), size.height().max(1.0));
    let max = 8192.0f32;
    let mut scale = 1.0f32;
    if w.max(h) > max {
        scale = max / w.max(h);
    } else if w.max(h) < 256.0 {
        scale = (256.0 / w.max(h)).min(8.0); // vetores pequenos ganham resolução útil
    }
    w = (w * scale).round().max(1.0);
    h = (h * scale).round().max(1.0);
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(w as u32, h as u32).ok_or("Tamanho de SVG inválido.")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut px = Vec::with_capacity(pixmap.data().len());
    for p in pixmap.pixels() {
        let c = p.demultiply();
        px.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let layer = Layer::from_pixels(name, w as u32, h as u32, px);
    let doc = Document::from_parts(name.to_string(), w as u32, h as u32, 72.0, vec![layer]);
    Ok(Imported {
        document: doc,
        warnings: vec![
            "SVG rasterizado: o desenho virou uma camada raster (os vetores não ficam editáveis); recursos fora do subconjunto do renderizador podem faltar.".into(),
        ],
    })
}

// ---------------------------------------------------------------------------------------
// OpenRaster

const ORA_OPS: &[(BlendMode, &str)] = &[
    (BlendMode::Normal, "svg:src-over"),
    (BlendMode::Multiply, "svg:multiply"),
    (BlendMode::Screen, "svg:screen"),
    (BlendMode::Overlay, "svg:overlay"),
    (BlendMode::Darken, "svg:darken"),
    (BlendMode::Lighten, "svg:lighten"),
    (BlendMode::ColorDodge, "svg:color-dodge"),
    (BlendMode::ColorBurn, "svg:color-burn"),
    (BlendMode::HardLight, "svg:hard-light"),
    (BlendMode::SoftLight, "svg:soft-light"),
    (BlendMode::Difference, "svg:difference"),
    (BlendMode::Exclusion, "svg:exclusion"),
    (BlendMode::LinearDodge, "svg:plus"),
    (BlendMode::Hue, "svg:hue"),
    (BlendMode::Saturation, "svg:saturation"),
    (BlendMode::Color, "svg:color"),
    (BlendMode::Luminosity, "svg:luminosity"),
];

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn png_bytes(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(rgba, w, h, ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Pixels da camada com a máscara aplicada ao alfa e o preenchimento embutido.
fn baked_pixels(doc: &Document, l: &Layer) -> Vec<u8> {
    let mut px = l.pixels.as_ref().clone();
    let mask = l
        .mask
        .as_ref()
        .filter(|m| m.enabled)
        .map(|m| m.effective(doc.width, doc.height));
    for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let mut a = f32::from(p[3]) * l.fill;
        if let Some(m) = &mask {
            a *= f32::from(m[i]) / 255.0;
        }
        p[3] = a.round().clamp(0.0, 255.0) as u8;
    }
    px
}

fn ora_stack(
    doc: &Document,
    parent: Option<LayerId>,
    depth: usize,
    files: &mut Vec<(String, Vec<u8>)>,
    warnings: &mut Vec<String>,
    xml: &mut String,
) -> Result<(), String> {
    let kids: Vec<&Layer> = doc.layers.iter().filter(|l| l.parent == parent).collect();
    for l in kids.iter().rev() {
        let pad = "  ".repeat(depth + 2);
        let op = ORA_OPS
            .iter()
            .find(|(m, _)| *m == l.blend)
            .map(|(_, s)| *s)
            .unwrap_or_else(|| {
                if l.blend != BlendMode::PassThrough {
                    warnings.push(format!("Modo de mesclagem de \"{}\" não existe no OpenRaster: gravado como Normal.", l.name));
                }
                "svg:src-over"
            });
        let vis = if l.visible { "visible" } else { "hidden" };
        if l.kind == LayerKind::Group {
            xml.push_str(&format!(
                "{pad}<stack name=\"{}\" visibility=\"{vis}\" opacity=\"{:.4}\" composite-op=\"{op}\">\n",
                xml_escape(&l.name),
                l.opacity
            ));
            ora_stack(doc, Some(l.id), depth + 1, files, warnings, xml)?;
            xml.push_str(&format!("{pad}</stack>\n"));
            continue;
        }
        if l.adjustment.is_some() {
            warnings.push(format!(
                "A camada de ajuste \"{}\" não existe no OpenRaster e foi omitida.",
                l.name
            ));
            continue;
        }
        if !l.styles.is_empty() {
            warnings.push(format!(
                "Os estilos de \"{}\" não são gravados (só os pixels da camada).",
                l.name
            ));
        }
        if l.clipped {
            warnings.push(format!(
                "O recorte (clipping) de \"{}\" não existe no OpenRaster.",
                l.name
            ));
        }
        let src = format!("data/layer{}.png", files.len());
        files.push((
            src.clone(),
            png_bytes(&baked_pixels(doc, l), l.width, l.height)?,
        ));
        xml.push_str(&format!(
            "{pad}<layer name=\"{}\" src=\"{src}\" x=\"0\" y=\"0\" visibility=\"{vis}\" opacity=\"{:.4}\" composite-op=\"{op}\"/>\n",
            xml_escape(&l.name),
            l.opacity
        ));
    }
    Ok(())
}

fn export_ora(doc: &Document, warnings: &mut Vec<String>) -> Result<Vec<u8>, String> {
    let mut files = Vec::new();
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<image version=\"0.0.3\" w=\"{}\" h=\"{}\">\n  <stack>\n",
        doc.width, doc.height
    );
    ora_stack(doc, None, 0, &mut files, warnings, &mut xml)?;
    xml.push_str("  </stack>\n</image>\n");
    let merged = compose::flatten(doc);
    let (tw, th) = {
        let s = 256.0 / doc.width.max(doc.height) as f32;
        let s = s.min(1.0);
        (
            ((doc.width as f32 * s).round() as u32).max(1),
            ((doc.height as f32 * s).round() as u32).max(1),
        )
    };
    let thumb = transform::resample(&merged, doc.width, doc.height, tw, th, Interp::Bilinear);
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let io = |e: zip::result::ZipError| e.to_string();
    zw.start_file("mimetype", stored).map_err(io)?;
    zw.write_all(b"image/openraster")
        .map_err(|e| e.to_string())?;
    zw.start_file("stack.xml", deflated).map_err(io)?;
    zw.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;
    for (name, data) in &files {
        zw.start_file(name.as_str(), stored).map_err(io)?;
        zw.write_all(data).map_err(|e| e.to_string())?;
    }
    zw.start_file("mergedimage.png", stored).map_err(io)?;
    zw.write_all(&png_bytes(&merged, doc.width, doc.height)?)
        .map_err(|e| e.to_string())?;
    zw.start_file("Thumbnails/thumbnail.png", stored)
        .map_err(io)?;
    zw.write_all(&png_bytes(&thumb, tw, th)?)
        .map_err(|e| e.to_string())?;
    let out = zw.finish().map_err(io)?;
    Ok(out.into_inner())
}

/// Atributos de uma tag XML simples (`chave="valor"`).
fn attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = tag.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if let Some(eq) = tag[i..].find("=\"") {
            let ks = tag[i..i + eq]
                .rsplit(|c: char| c.is_whitespace() || c == '<')
                .next()
                .unwrap_or("")
                .to_string();
            let vs = i + eq + 2;
            if let Some(len) = tag[vs..].find('"') {
                out.push((ks, xml_unescape(&tag[vs..vs + len])));
                i = vs + len + 1;
                continue;
            }
        }
        break;
    }
    out
}

fn attr<'a>(a: &'a [(String, String)], k: &str) -> Option<&'a str> {
    a.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str())
}

type Attrs = Vec<(String, String)>;

enum OraNode {
    Layer(Vec<(String, String)>),
    Stack(Vec<(String, String)>, Vec<OraNode>),
}

const ORA_MAX_ENTRIES: usize = 4000;
const ORA_MAX_ENTRY_BYTES: u64 = 1 << 30;

fn parse_ora_stack(xml: &str) -> Result<(Attrs, Vec<OraNode>), String> {
    let mut root_attrs = Vec::new();
    // O primeiro item é uma raiz fictícia; a pilha raiz do arquivo vira seu único filho.
    let mut stack: Vec<(Attrs, Vec<OraNode>)> = vec![(Vec::new(), Vec::new())];
    let mut rest = xml;
    while let Some(s) = rest.find('<') {
        let Some(e) = rest[s..].find('>') else { break };
        let tag = &rest[s + 1..s + e];
        rest = &rest[s + e + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        let closing = tag.starts_with('/');
        let selfclose = tag.ends_with('/');
        let name = tag
            .trim_start_matches('/')
            .trim_end_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("");
        match (name, closing) {
            ("image", false) => root_attrs = attrs(tag),
            ("stack", false) => {
                stack.push((attrs(tag), Vec::new()));
                if selfclose {
                    let (a, kids) = stack.pop().unwrap_or_default();
                    if let Some(top) = stack.last_mut() {
                        top.1.push(OraNode::Stack(a, kids));
                    }
                }
            }
            ("stack", true) => {
                if stack.len() < 2 {
                    return Err("stack.xml malformado.".into());
                }
                let (a, kids) = stack.pop().unwrap_or_default();
                if let Some(top) = stack.last_mut() {
                    top.1.push(OraNode::Stack(a, kids));
                }
            }
            ("layer", false) => {
                if let Some(top) = stack.last_mut() {
                    top.1.push(OraNode::Layer(attrs(tag)));
                }
            }
            _ => {}
        }
        if stack.len() > 64 {
            return Err("Grupos aninhados demais.".into());
        }
    }
    while stack.len() > 1 {
        // Pilhas não fechadas: fecha-as para não perder as camadas.
        let (a, kids) = stack.pop().unwrap_or_default();
        if let Some(top) = stack.last_mut() {
            top.1.push(OraNode::Stack(a, kids));
        }
    }
    let (_, mut top) = stack.pop().ok_or("stack.xml vazio.")?;
    let nodes = match top.pop() {
        Some(OraNode::Stack(_, kids)) if top.is_empty() => kids,
        Some(other) => {
            top.push(other);
            top
        }
        None => Vec::new(),
    };
    Ok((root_attrs, nodes))
}

fn import_ora(bytes: &[u8], name: &str) -> Result<Imported, String> {
    let mut zr =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("ORA inválido: {e}"))?;
    if zr.len() > ORA_MAX_ENTRIES {
        return Err("ORA com arquivos demais.".into());
    }
    let mut read = |n: &str| -> Result<Vec<u8>, String> {
        let f = zr.by_name(n).map_err(|e| format!("{n}: {e}"))?;
        if f.size() > ORA_MAX_ENTRY_BYTES {
            return Err(format!("{n}: grande demais."));
        }
        let mut out = Vec::new();
        f.take(ORA_MAX_ENTRY_BYTES)
            .read_to_end(&mut out)
            .map_err(|e| e.to_string())?;
        Ok(out)
    };
    let xml = String::from_utf8_lossy(&read("stack.xml")?).into_owned();
    let (root, nodes) = parse_ora_stack(&xml)?;
    let dim = |k: &str| {
        attr(&root, k)
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| (1..=Document::MAX_SIDE).contains(v))
    };
    let (w, h) = (
        dim("w").ok_or("ORA sem largura válida.")?,
        dim("h").ok_or("ORA sem altura válida.")?,
    );
    let mut warnings = Vec::new();
    let mut layers: Vec<Layer> = Vec::new();
    let mut budget: u64 = 2 << 30;
    #[allow(clippy::too_many_arguments)]
    fn walk(
        nodes: &[OraNode],
        parent: Option<LayerId>,
        w: u32,
        h: u32,
        layers: &mut Vec<Layer>,
        warnings: &mut Vec<String>,
        budget: &mut u64,
        read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
    ) -> Result<(), String> {
        for n in nodes.iter().rev() {
            let a = match n {
                OraNode::Layer(a) | OraNode::Stack(a, _) => a,
            };
            let nm = attr(a, "name")
                .filter(|s| !s.is_empty())
                .unwrap_or("Camada")
                .chars()
                .take(120)
                .collect::<String>();
            let vis = attr(a, "visibility") != Some("hidden");
            let opacity = attr(a, "opacity")
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|v| v.is_finite())
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            let blend = match attr(a, "composite-op") {
                None => BlendMode::Normal,
                Some(op) => match ORA_OPS.iter().find(|(_, s)| *s == op) {
                    Some((m, _)) => *m,
                    None => {
                        warnings.push(format!(
                            "Modo \"{op}\" de \"{nm}\" não é suportado: usado Normal."
                        ));
                        BlendMode::Normal
                    }
                },
            };
            match n {
                OraNode::Stack(_, kids) => {
                    let mut g = Layer::group(nm);
                    g.parent = parent;
                    g.visible = vis;
                    g.opacity = opacity;
                    g.blend = if blend == BlendMode::Normal {
                        BlendMode::PassThrough
                    } else {
                        blend
                    };
                    let id = g.id;
                    layers.push(g);
                    walk(kids, Some(id), w, h, layers, warnings, budget, read)?;
                }
                OraNode::Layer(_) => {
                    let Some(src) = attr(a, "src") else { continue };
                    let cost = u64::from(w) * u64::from(h) * 4;
                    if *budget < cost {
                        return Err("ORA grande demais para a memória permitida.".into());
                    }
                    *budget -= cost;
                    let data = match read(src) {
                        Ok(d) => d,
                        Err(e) => {
                            warnings.push(format!("Camada \"{nm}\" omitida: {e}"));
                            continue;
                        }
                    };
                    let img = match imageio::import_as(&data, &nm, Some(image::ImageFormat::Png)) {
                        Ok(i) => i,
                        Err(e) => {
                            warnings.push(format!("Camada \"{nm}\" omitida: {e}"));
                            continue;
                        }
                    };
                    let s = &img.document.layers[0];
                    let (ox, oy) = (
                        attr(a, "x")
                            .and_then(|v| v.parse::<i64>().ok())
                            .unwrap_or(0),
                        attr(a, "y")
                            .and_then(|v| v.parse::<i64>().ok())
                            .unwrap_or(0),
                    );
                    let mut px = vec![0u8; w as usize * h as usize * 4];
                    for y in 0..s.height as i64 {
                        let dy = y + oy;
                        if dy < 0 || dy >= i64::from(h) {
                            continue;
                        }
                        for x in 0..s.width as i64 {
                            let dx = x + ox;
                            if dx < 0 || dx >= i64::from(w) {
                                continue;
                            }
                            let si = (y as usize * s.width as usize + x as usize) * 4;
                            let di = (dy as usize * w as usize + dx as usize) * 4;
                            px[di..di + 4].copy_from_slice(&s.pixels[si..si + 4]);
                        }
                    }
                    let mut l = Layer::from_pixels(nm, w, h, px);
                    l.parent = parent;
                    l.visible = vis;
                    l.opacity = opacity;
                    l.blend = blend;
                    layers.push(l);
                }
            }
        }
        Ok(())
    }
    walk(
        &nodes,
        None,
        w,
        h,
        &mut layers,
        &mut warnings,
        &mut budget,
        &mut read,
    )?;
    if layers.is_empty() {
        return Err("O arquivo ORA não tem camadas legíveis.".into());
    }
    let layers = normalize(layers);
    warnings.push("OpenRaster: camadas, grupos, visibilidade, opacidade e modos básicos foram importados; máscaras e efeitos não existem no formato.".into());
    let doc = Document::from_parts(name.to_string(), w, h, 72.0, layers);
    Ok(Imported {
        document: doc,
        warnings,
    })
}

// ---------------------------------------------------------------------------------------
// Exportação

#[derive(Debug, Clone)]
pub struct ExportOpts {
    pub quality: u8,
    pub background: [u8; 3],
    pub color: ColorOut,
    /// Reduz a imagem exportada para este lado máximo (miniaturas); `None` = tamanho original.
    pub max_side: Option<u32>,
}

impl Default for ExportOpts {
    fn default() -> Self {
        Self {
            quality: 90,
            background: [255, 255, 255],
            color: ColorOut::Embed,
            max_side: None,
        }
    }
}

pub struct Exported {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

fn has_transparency(doc: &Document) -> bool {
    compose::flatten(doc)
        .as_chunks::<4>()
        .0
        .iter()
        .any(|p| p[3] < 255)
}

/// Avisos que se aplicam ao exportar `doc` em `fmt` (sem codificar).
pub fn export_warnings(doc: &Document, fmt: Format, opts: &ExportOpts) -> Vec<String> {
    let mut w = Vec::new();
    let i = info(fmt);
    if matches!(fmt, Format::Jpeg | Format::Pnm) && has_transparency(doc) {
        w.push(format!("{} não tem transparência: áreas transparentes serão compostas sobre o fundo escolhido.", i.name));
    }
    if fmt == Format::Gif {
        w.push("GIF: no máximo 256 cores e transparência de 1 bit (semitransparências viram opacas ou transparentes).".into());
    }
    if fmt == Format::Ico && (doc.width > 256 || doc.height > 256) {
        w.push("ICO aceita no máximo 256×256 px: reduza a imagem antes de exportar.".into());
    }
    if matches!(fmt, Format::Hdr | Format::Exr) {
        w.push("O documento tem 8 bits por canal: o arquivo HDR/EXR terá o mesmo alcance (sem informação de alto alcance dinâmico).".into());
    }
    if fmt == Format::Webp {
        w.push("WebP é gravado sem perdas (sem qualidade ajustável nesta versão).".into());
    }
    if fmt == Format::Ora {
        w.push("OpenRaster: ajustes, estilos, recortes e objetos inteligentes não existem no formato; máscaras e preenchimento são embutidos no alfa.".into());
    }
    if let Some(p) = &doc.icc {
        match (fmt, opts.color) {
            (Format::Png | Format::Jpeg, ColorOut::Embed) => {}
            (_, ColorOut::ToSrgb) => w.push(format!("Os pixels serão convertidos de \"{}\" para sRGB.", p.name)),
            (Format::Png | Format::Jpeg, ColorOut::Strip) => w.push(format!("O perfil \"{}\" não será gravado.", p.name)),
            _ => w.push(format!("Este formato não embute o perfil \"{}\": os pixels são gravados como estão e o leitor presumirá sRGB. Use \"Converter para sRGB\" se a cor importa.", p.name)),
        }
    }
    if !matches!(fmt, Format::Ora) && doc.layers.iter().filter(|l| !l.is_group()).count() > 1 {
        w.push("O formato não guarda camadas: a imagem composta será exportada. Use .orva (ou OpenRaster) para preservar as camadas.".into());
    }
    w
}

/// Exporta a imagem composta (ou as camadas, no OpenRaster) no formato escolhido.
pub fn export(doc: &Document, fmt: Format, opts: &ExportOpts) -> Result<Exported, String> {
    if let Some(m) = opts
        .max_side
        .filter(|m| *m >= 1 && *m < doc.width.max(doc.height) && fmt != Format::Ora)
    {
        let scale = m as f32 / doc.width.max(doc.height) as f32;
        let (nw, nh) = (
            ((doc.width as f32 * scale).round() as u32).max(1),
            ((doc.height as f32 * scale).round() as u32).max(1),
        );
        let px = transform::resample(
            &compose::flatten(doc),
            doc.width,
            doc.height,
            nw,
            nh,
            Interp::Bicubic,
        );
        let mut small = Document::from_parts(
            doc.name.clone(),
            nw,
            nh,
            doc.ppi,
            vec![Layer::from_pixels("Miniatura", nw, nh, px)],
        );
        small.icc = doc.icc.clone();
        let inner = ExportOpts {
            max_side: None,
            ..opts.clone()
        };
        let mut out = export(&small, fmt, &inner)?;
        out.warnings.insert(
            0,
            format!("Imagem reduzida para {nw}×{nh} px (lado máximo {m})."),
        );
        return Ok(out);
    }
    let mut warnings = export_warnings(doc, fmt, opts);
    let (w, h) = (doc.width, doc.height);
    let bytes = match fmt {
        Format::Png => imageio::export_png_with(doc, opts.color)?,
        Format::Jpeg => imageio::export_jpeg_with(doc, opts.quality, opts.background, opts.color)?,
        Format::Ora => export_ora(doc, &mut warnings)?,
        Format::Orva | Format::Svg | Format::Psd | Format::Psb | Format::Pdf => {
            return Err(format!(
                "Exportação em {} não está disponível aqui.",
                info(fmt).name
            ));
        }
        raster => {
            let (rgba, icc) = imageio::flatten_for_export(doc, opts.color)?;
            let mut out = Vec::new();
            encode_raster(raster, &rgba, w, h, icc, opts, &mut out, &mut warnings)?;
            out
        }
    };
    Ok(Exported { bytes, warnings })
}

#[allow(clippy::too_many_arguments)]
fn encode_raster(
    fmt: Format,
    rgba: &[u8],
    w: u32,
    h: u32,
    icc: Option<Vec<u8>>,
    opts: &ExportOpts,
    out: &mut Vec<u8>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    use image::codecs::{
        bmp::BmpEncoder,
        gif::GifEncoder,
        hdr::HdrEncoder,
        ico::IcoEncoder,
        openexr::OpenExrEncoder,
        pnm::{PnmEncoder, PnmSubtype, SampleEncoding},
        qoi::QoiEncoder,
        tga::TgaEncoder,
        tiff::TiffEncoder,
        webp::WebPEncoder,
    };
    let e = |x: image::ImageError| x.to_string();
    let mut icc_note = |r: Result<(), image::error::UnsupportedError>, name: &str| {
        if r.is_err() {
            warnings.push(format!(
                "{name} não embute perfil de cor neste codec: os pixels foram gravados como estão."
            ));
        }
    };
    match fmt {
        Format::Webp => {
            let mut enc = WebPEncoder::new_lossless(&mut *out);
            if let Some(p) = icc {
                icc_note(enc.set_icc_profile(p), "WebP");
            }
            enc.write_image(rgba, w, h, ExtendedColorType::Rgba8)
                .map_err(e)?;
        }
        Format::Gif => {
            let mut px = rgba.to_vec();
            for p in px.as_chunks_mut::<4>().0 {
                p[3] = if p[3] >= 128 { 255 } else { 0 };
            }
            let img = image::RgbaImage::from_raw(w, h, px).ok_or("Falha ao montar a imagem.")?;
            let mut enc = GifEncoder::new_with_speed(&mut *out, 10);
            enc.encode_frame(image::Frame::new(img)).map_err(e)?;
        }
        Format::Bmp => {
            let mut enc = BmpEncoder::new(out);
            enc.encode(rgba, w, h, ExtendedColorType::Rgba8)
                .map_err(e)?;
        }
        Format::Tiff => {
            let mut cur = Cursor::new(Vec::new());
            {
                let mut enc = TiffEncoder::new(&mut cur);
                if let Some(p) = icc {
                    icc_note(enc.set_icc_profile(p), "TIFF");
                }
                enc.write_image(rgba, w, h, ExtendedColorType::Rgba8)
                    .map_err(e)?;
            }
            *out = cur.into_inner();
        }
        Format::Tga => {
            TgaEncoder::new(&mut *out)
                .encode(rgba, w, h, ExtendedColorType::Rgba8)
                .map_err(e)?;
        }
        Format::Ico => {
            if w > 256 || h > 256 {
                return Err("ICO aceita no máximo 256×256 px.".into());
            }
            IcoEncoder::new(&mut *out)
                .write_image(rgba, w, h, ExtendedColorType::Rgba8)
                .map_err(e)?;
        }
        Format::Qoi => {
            QoiEncoder::new(&mut *out)
                .write_image(rgba, w, h, ExtendedColorType::Rgba8)
                .map_err(e)?;
        }
        Format::Pnm => {
            let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
            for p in rgba.as_chunks::<4>().0 {
                let a = u32::from(p[3]);
                for (v, bg) in p[..3].iter().zip(opts.background) {
                    rgb.push(((u32::from(*v) * a + u32::from(bg) * (255 - a) + 127) / 255) as u8);
                }
            }
            PnmEncoder::new(&mut *out)
                .with_subtype(PnmSubtype::Pixmap(SampleEncoding::Binary))
                .write_image(&rgb, w, h, ExtendedColorType::Rgb8)
                .map_err(e)?;
        }
        Format::Hdr => {
            let lin = |v: u8| {
                let c = f32::from(v) / 255.0;
                if c <= 0.040_45 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            let px: Vec<image::Rgb<f32>> = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| {
                    let a = f32::from(p[3]) / 255.0;
                    image::Rgb([lin(p[0]) * a, lin(p[1]) * a, lin(p[2]) * a])
                })
                .collect();
            warnings.push("Radiance HDR não tem alfa: a cor foi multiplicada pelo alfa.".into());
            HdrEncoder::new(&mut *out)
                .encode(&px, w as usize, h as usize)
                .map_err(e)?;
        }
        Format::Exr => {
            let lin = |v: u8| {
                let c = f32::from(v) / 255.0;
                if c <= 0.040_45 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            let mut raw = Vec::with_capacity(rgba.len() * 4);
            for p in rgba.as_chunks::<4>().0 {
                for v in [lin(p[0]), lin(p[1]), lin(p[2]), f32::from(p[3]) / 255.0] {
                    raw.extend_from_slice(&v.to_ne_bytes());
                }
            }
            let mut cur = Cursor::new(Vec::new());
            OpenExrEncoder::new(&mut cur)
                .write_image(&raw, w, h, ExtendedColorType::Rgba32F)
                .map_err(e)?;
            *out = cur.into_inner();
        }
        other => return Err(format!("Sem codificador para {}.", info(other).name)),
    }
    Ok(())
}

/// Folha de sprites: cada camada visível (já com máscara) ocupa uma célula do tamanho do
/// documento numa grade de `cols` colunas, com `pad` pixels entre as células.
pub fn sprite_sheet(doc: &Document, cols: u32, pad: u32) -> Result<Document, String> {
    let sprites: Vec<&Layer> = doc
        .layers
        .iter()
        .filter(|l| !l.is_group() && l.visible && l.adjustment.is_none())
        .collect();
    if sprites.is_empty() {
        return Err("Não há camadas visíveis para a folha de sprites.".into());
    }
    let cols = cols.clamp(1, sprites.len() as u32);
    let rows = (sprites.len() as u32).div_ceil(cols);
    let (cw, ch) = (doc.width + pad, doc.height + pad);
    let (tw, th) = (cols * cw - pad, rows * ch - pad);
    if tw > 16384 || th > 16384 {
        return Err(format!("A folha teria {tw}×{th} px (máximo 16384)."));
    }
    let mut px = vec![0u8; tw as usize * th as usize * 4];
    for (i, l) in sprites.iter().enumerate() {
        let (ox, oy) = ((i as u32 % cols) * cw, (i as u32 / cols) * ch);
        let src = baked_pixels(doc, l);
        for y in 0..doc.height as usize {
            let d = ((oy as usize + y) * tw as usize + ox as usize) * 4;
            let s = y * doc.width as usize * 4;
            px[d..d + doc.width as usize * 4].copy_from_slice(&src[s..s + doc.width as usize * 4]);
        }
    }
    let mut out = Document::from_parts(
        format!("{} (sprites)", doc.name),
        tw,
        th,
        doc.ppi,
        vec![Layer::from_pixels("Sprites", tw, th, px)],
    );
    out.icc = doc.icc.clone();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_and_sprite_sheets() {
        let mut d = Document::new("s", 8, 8, None).unwrap();
        d.layers[0].set_pixel(0, 0, [255, 0, 0, 255]);
        let mut b = Layer::transparent("b", 8, 8);
        b.set_pixel(7, 7, [0, 0, 255, 255]);
        let mut hidden = Layer::filled("h", 8, 8, [9, 9, 9, 255]);
        hidden.visible = false;
        d.layers.extend([b, hidden]);
        let sheet = sprite_sheet(&d, 2, 2).unwrap();
        assert_eq!((sheet.width, sheet.height), (18, 8));
        assert_eq!(sheet.layers[0].pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(
            sheet.layers[0].pixel(10 + 7, 7),
            Some([0, 0, 255, 255]),
            "2ª célula, deslocada de 8+2"
        );
        assert_eq!(
            sheet.layers[0].pixel(8, 3).unwrap()[3],
            0,
            "o espaçamento é transparente"
        );
        assert!(sprite_sheet(&Document::new("e", 4, 4, None).unwrap(), 1, 0).is_ok());
        let big = Document::new("g", 1000, 1000, None).unwrap();
        let mut many = big.clone();
        many.layers
            .extend((0..30).map(|i| Layer::transparent(format!("l{i}"), 1000, 1000)));
        assert!(sprite_sheet(&many, 31, 0).is_err(), "limite de tamanho");
        let thumb = export(
            &Document::new("t", 64, 32, Some([10, 20, 30, 255])).unwrap(),
            Format::Png,
            &ExportOpts {
                max_side: Some(16),
                ..ExportOpts::default()
            },
        )
        .unwrap();
        let back = import(&thumb.bytes, "t", "png").unwrap().document;
        assert_eq!((back.width, back.height), (16, 8));
        assert!(thumb.warnings[0].contains("reduzida"));
    }

    fn doc() -> Document {
        let mut d = Document::new("t", 16, 12, Some([30, 90, 200, 255])).unwrap();
        d.layers[0].set_pixel(2, 3, [250, 10, 10, 255]);
        d.layers[0].set_pixel(15, 11, [10, 250, 10, 255]);
        d
    }

    fn translucent() -> Document {
        let mut d = Document::new("t", 8, 8, None).unwrap();
        for y in 0..8 {
            for x in 0..4 {
                d.layers[0].set_pixel(x, y, [200, 40, 40, 255]);
            }
        }
        d
    }

    #[test]
    fn lossless_formats_roundtrip_exact_pixels_and_alpha() {
        let d = translucent();
        let want = compose::flatten(&d);
        for f in [
            Format::Png,
            Format::Webp,
            Format::Bmp,
            Format::Tiff,
            Format::Tga,
            Format::Qoi,
        ] {
            let out = export(&d, f, &ExportOpts::default()).unwrap();
            let back = import(&out.bytes, "x", f.default_ext()).unwrap().document;
            assert_eq!((back.width, back.height), (8, 8), "{f:?}");
            assert_eq!(
                *back.layers[0].pixels, want,
                "{f:?}: pixels e alfa preservados"
            );
            assert_eq!(
                detect(&out.bytes, f.default_ext()),
                Some(f),
                "{f:?}: detecção por conteúdo"
            );
        }
    }

    #[test]
    fn jpeg_pnm_gif_ico_hdr_exr_have_documented_behaviour() {
        let d = doc();
        for (f, tol) in [
            (Format::Jpeg, 40),
            (Format::Pnm, 0),
            (Format::Gif, 24),
            (Format::Hdr, 3),
            (Format::Exr, 2),
        ] {
            let out = export(&d, f, &ExportOpts::default()).unwrap();
            let back = import(&out.bytes, "x", f.default_ext()).unwrap().document;
            assert_eq!((back.width, back.height), (16, 12), "{f:?}");
            let p = back.layers[0].pixel(8, 6).unwrap();
            assert!(
                p[0].abs_diff(30) as i32 <= tol && p[2].abs_diff(200) as i32 <= tol && p[3] == 255,
                "{f:?}: {p:?}"
            );
        }
        let ico = export(
            &Document::new("i", 32, 32, Some([9, 9, 9, 255])).unwrap(),
            Format::Ico,
            &ExportOpts::default(),
        )
        .unwrap();
        let back = import(&ico.bytes, "i", "ico").unwrap().document;
        assert_eq!((back.width, back.height), (32, 32));
        assert!(export(
            &Document::new("g", 300, 10, None).unwrap(),
            Format::Ico,
            &ExportOpts::default()
        )
        .is_err());
    }

    #[test]
    fn warnings_describe_losses() {
        let t = translucent();
        let o = ExportOpts::default();
        assert!(export_warnings(&t, Format::Jpeg, &o)
            .iter()
            .any(|w| w.contains("transparência")));
        assert!(export_warnings(&t, Format::Gif, &o)
            .iter()
            .any(|w| w.contains("256")));
        assert!(!export_warnings(&t, Format::Png, &o)
            .iter()
            .any(|w| w.contains("transparência")));
        let mut multi = doc();
        multi.layers.push(Layer::transparent("b", 16, 12));
        assert!(export_warnings(&multi, Format::Png, &o)
            .iter()
            .any(|w| w.contains("camadas")));
        assert!(!export_warnings(&multi, Format::Ora, &o)
            .iter()
            .any(|w| w.contains("não guarda camadas")));
        let mut p = doc();
        p.icc = crate::color::builtin_profiles().into_iter().nth(2);
        assert!(export_warnings(&p, Format::Bmp, &o)
            .iter()
            .any(|w| w.contains("Display P3")));
    }

    #[test]
    fn animated_gif_and_webp_import_first_frame_with_a_warning() {
        use image::codecs::gif::GifEncoder;
        let mut gif = Vec::new();
        {
            let mut enc = GifEncoder::new(&mut gif);
            for c in [[255u8, 0, 0, 255], [0, 0, 255, 255]] {
                let img = image::RgbaImage::from_pixel(6, 6, image::Rgba(c));
                enc.encode_frame(image::Frame::new(img)).unwrap();
            }
        }
        let imp = import(&gif, "a", "gif").unwrap();
        assert!(
            imp.warnings.iter().any(|w| w.contains("animada")),
            "{:?}",
            imp.warnings
        );
        assert_eq!(imp.document.layers[0].pixel(0, 0).unwrap()[0], 255);
    }

    #[test]
    fn detection_and_unsupported_reasons() {
        assert_eq!(detect(b"\x89PNG\r\n\x1a\nxxxx", ""), Some(Format::Png));
        assert_eq!(detect(b"8BPS\0\x01rest", "psd"), Some(Format::Psd));
        assert_eq!(detect(b"8BPS\0\x02rest", "psb"), Some(Format::Psb));
        assert_eq!(detect(b"%PDF-1.7", ""), Some(Format::Pdf));
        assert_eq!(
            detect(b"<?xml version='1.0'?><svg xmlns='x'/>", "svg"),
            Some(Format::Svg)
        );
        assert_eq!(detect(b"lixo", "tga"), Some(Format::Tga));
        assert_eq!(detect(b"lixo", "png"), None, "extensão não basta");
        assert!(import(b"x", "a", "avif").unwrap_err().contains("AV1"));
        assert!(import(b"x", "a", "cr2").unwrap_err().contains("RAW"));
        assert!(import(b"x", "a", "pcraft")
            .unwrap_err()
            .contains("especificação"));
        assert!(import(b"nada", "a", "xyz").is_err());
        assert_eq!(Format::from_ext(".JPG"), Some(Format::Jpeg));
    }

    #[test]
    fn svg_is_rasterized_with_a_warning_and_alpha() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48"><rect x="0" y="0" width="32" height="48" fill="#ff0000"/></svg>"##;
        let imp = import(svg, "s", "svg").unwrap();
        assert!(imp.warnings.iter().any(|w| w.contains("rasterizado")));
        let l = &imp.document.layers[0];
        assert!(l.width >= 64 && l.height >= 48);
        let (cx, cy) = (l.width / 4, l.height / 2);
        assert_eq!(l.pixel(cx, cy), Some([255, 0, 0, 255]));
        assert_eq!(
            l.pixel(l.width - 2, cy).unwrap()[3],
            0,
            "fora do retângulo é transparente"
        );
        assert!(import(b"<svg", "s", "svg").is_err());
    }

    #[test]
    fn openraster_roundtrip_keeps_layers_groups_and_properties() {
        let mut d = Document::new("o", 20, 10, Some([255, 255, 255, 255])).unwrap();
        let mut a = Layer::transparent("Vermelho", 20, 10);
        for y in 2..6 {
            for x in 3..9 {
                a.set_pixel(x, y, [255, 0, 0, 255]);
            }
        }
        a.opacity = 0.5;
        a.blend = BlendMode::Multiply;
        let g = Layer::group("Grupo & <teste>");
        let gid = g.id;
        let mut b = Layer::filled("Azul", 20, 10, [0, 0, 255, 255]);
        b.parent = Some(gid);
        b.visible = false;
        d.layers.extend([a, g, b]);
        let out = export(&d, Format::Ora, &ExportOpts::default()).unwrap();
        let back = import(&out.bytes, "o", "ora").unwrap().document;
        assert_eq!((back.width, back.height), (20, 10));
        assert_eq!(back.layers.len(), d.layers.len());
        let names: Vec<&str> = back.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["Fundo", "Vermelho", "Grupo & <teste>", "Azul"]);
        let red = &back.layers[1];
        assert!((red.opacity - 0.5).abs() < 1e-3 && red.blend == BlendMode::Multiply);
        assert_eq!(red.pixels, d.layers[1].pixels);
        assert!(
            back.layers[2].is_group()
                && back.layers[3].parent == Some(back.layers[2].id)
                && !back.layers[3].visible
        );
        assert_eq!(
            compose::flatten(&back),
            compose::flatten(&d),
            "composição idêntica após reabrir"
        );
        // Estrutura do contêiner: mimetype primeiro e sem compressão.
        let mut z = zip::ZipArchive::new(Cursor::new(&out.bytes)).unwrap();
        assert_eq!(
            z.by_index(0).unwrap().name().unwrap_or_default(),
            "mimetype"
        );
        let mut mt = String::new();
        z.by_name("mimetype")
            .unwrap()
            .read_to_string(&mut mt)
            .unwrap();
        assert_eq!(mt, "image/openraster");
        assert!(
            z.by_name("mergedimage.png").is_ok() && z.by_name("Thumbnails/thumbnail.png").is_ok()
        );
    }

    #[test]
    fn openraster_hostile_inputs_are_rejected_or_tolerated() {
        assert!(import(b"PK\x03\x04lixo", "x", "ora").is_err());
        let mk = |xml: &str| {
            let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let o = zip::write::SimpleFileOptions::default();
            zw.start_file("stack.xml", o).unwrap();
            zw.write_all(xml.as_bytes()).unwrap();
            zw.finish().unwrap().into_inner()
        };
        assert!(import(
            &mk("<image w=\"99999\" h=\"5\"><stack/></image>"),
            "x",
            "ora"
        )
        .is_err());
        assert!(import(&mk("<image w=\"4\" h=\"4\"><stack><layer name=\"x\" src=\"data/falta.png\"/></stack></image>"), "x", "ora").is_err(), "sem camadas legíveis");
        let deep = format!("<image w=\"4\" h=\"4\">{}</image>", "<stack>".repeat(100));
        assert!(import(&mk(&deep), "x", "ora").is_err());
    }
}
