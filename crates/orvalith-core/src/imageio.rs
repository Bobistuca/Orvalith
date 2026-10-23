//! Importação e exportação de imagens (PNG e JPEG) com limites de memória.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{
    ColorType, DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageReader, Limits,
};

use crate::color::{self, IccProfile, ProfileKind};
use crate::compose;
use crate::document::{Document, Layer};

/// Limite de pixels de uma imagem importada (evita bombas de descompressão).
const MAX_IMPORT_PIXELS: u64 = 400_000_000;

/// Resultado de uma importação.
pub struct Imported {
    pub document: Document,
    /// Avisos para o usuário (ex.: redução de profundidade de bits).
    pub warnings: Vec<String>,
}

fn bits_per_channel(ct: ColorType) -> u32 {
    u32::from(ct.bits_per_pixel()) / u32::from(ct.channel_count())
}

/// Decodifica PNG/JPEG de bytes, aplicando a orientação EXIF.
pub fn import(bytes: &[u8], name: &str) -> Result<Imported, String> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(Document::MAX_SIDE);
    limits.max_image_height = Some(Document::MAX_SIDE);
    limits.max_alloc = Some(2 * 1024 * 1024 * 1024);
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    reader.limits(limits);
    match reader.format() {
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg) => {}
        Some(other) => {
            return Err(format!(
                "Formato {other:?} ainda não é suportado (apenas PNG e JPEG nesta versão)."
            ))
        }
        None => return Err("Formato de imagem não reconhecido.".into()),
    }
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| format!("Imagem inválida: {e}"))?;
    let (w, h) = decoder.dimensions();
    if u64::from(w) * u64::from(h) > MAX_IMPORT_PIXELS {
        return Err(format!("Imagem grande demais ({w}×{h})."));
    }
    let color = decoder.color_type();
    let icc_bytes = decoder.icc_profile().ok().flatten();
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img =
        DynamicImage::from_decoder(decoder).map_err(|e| format!("Falha ao decodificar: {e}"))?;
    img.apply_orientation(orientation);

    let mut warnings = Vec::new();
    if bits_per_channel(color) > 8 {
        warnings.push(
            "A imagem tem mais de 8 bits por canal; foi convertida para 8 bits por canal (precisão reduzida)."
                .to_string(),
        );
    }
    if orientation != image::metadata::Orientation::NoTransforms {
        warnings.push("A orientação EXIF da imagem foi aplicada.".to_string());
    }
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let layer = Layer::from_pixels(name, w, h, rgba.into_raw());
    let mut document = Document::from_parts(name.to_string(), w, h, 72.0, vec![layer]);
    if let Some(bytes) = icc_bytes {
        match color::inspect(&bytes) {
            Ok(info) if info.kind == ProfileKind::Rgb => {
                let p = IccProfile {
                    name: if info.description.is_empty() {
                        "Perfil sem nome".into()
                    } else {
                        info.description
                    },
                    data: std::sync::Arc::new(bytes),
                };
                if !info.is_srgb {
                    warnings.push(format!(
                        "A imagem tem o perfil de cor \"{}\": foi preservado (pixels intactos) e a tela mostra uma prévia convertida para sRGB. Use Imagem → Perfil de cor para atribuir ou converter.",
                        p.name
                    ));
                }
                document.icc = Some(p);
            }
            Ok(info) => warnings.push(format!(
                "O perfil de cor \"{}\" não é RGB e não é suportado: foi descartado e a imagem é tratada como sRGB.",
                info.description
            )),
            Err(e) => warnings.push(format!(
                "O perfil de cor embutido não pôde ser lido ({e}) e foi descartado; a imagem é tratada como sRGB."
            )),
        }
    }
    Ok(Imported { document, warnings })
}

/// O que fazer com o perfil de cor na exportação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorOut {
    /// Grava os pixels como estão e embute o perfil do documento (se houver).
    #[default]
    Embed,
    /// Converte para sRGB e embute o sRGB.
    ToSrgb,
    /// Grava sem perfil (o leitor presumirá sRGB); os pixels não são convertidos.
    Strip,
}

/// Compõe e prepara o perfil a embutir conforme `mode`.
fn flatten_for_export(
    doc: &Document,
    mode: ColorOut,
) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
    let mut rgba = compose::flatten(doc);
    match mode {
        ColorOut::Embed => Ok((rgba, doc.icc.as_ref().map(|p| p.data.as_ref().clone()))),
        ColorOut::Strip => Ok((rgba, None)),
        ColorOut::ToSrgb => {
            if color::is_srgb(doc.icc.as_ref()) {
                return Ok((rgba, doc.icc.as_ref().map(|p| p.data.as_ref().clone())));
            }
            color::convert_rgba(&mut rgba, doc.icc.as_ref(), None)?;
            let srgb = color::builtin_profiles()
                .into_iter()
                .next()
                .map(|p| p.data.as_ref().clone());
            Ok((rgba, srgb))
        }
    }
}

/// Exporta o documento composto como PNG com alfa, embutindo o perfil do documento.
pub fn export_png(doc: &Document) -> Result<Vec<u8>, String> {
    export_png_with(doc, ColorOut::Embed)
}

pub fn export_png_with(doc: &Document, mode: ColorOut) -> Result<Vec<u8>, String> {
    let (rgba, icc) = flatten_for_export(doc, mode)?;
    let mut out = Vec::new();
    let mut enc = PngEncoder::new(&mut out);
    if let Some(p) = icc {
        enc.set_icc_profile(p).map_err(|e| e.to_string())?;
    }
    enc.write_image(&rgba, doc.width, doc.height, ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Exporta como JPEG (qualidade 1–100), compondo sobre `background` (RGB).
pub fn export_jpeg(doc: &Document, quality: u8, background: [u8; 3]) -> Result<Vec<u8>, String> {
    export_jpeg_with(doc, quality, background, ColorOut::Embed)
}

pub fn export_jpeg_with(
    doc: &Document,
    quality: u8,
    background: [u8; 3],
    mode: ColorOut,
) -> Result<Vec<u8>, String> {
    let (rgba, icc) = flatten_for_export(doc, mode)?;
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for px in rgba.as_chunks::<4>().0 {
        let a = u32::from(px[3]);
        for c in 0..3 {
            let v = (u32::from(px[c]) * a + u32::from(background[c]) * (255 - a) + 127) / 255;
            rgb.push(v as u8);
        }
    }
    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    if let Some(p) = icc {
        enc.set_icc_profile(p).map_err(|e| e.to_string())?;
    }
    enc.write_image(&rgb, doc.width, doc.height, ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern() -> Document {
        let mut doc = Document::new("p", 4, 2, None).unwrap();
        doc.layers[0].set_pixel(0, 0, [255, 0, 0, 255]);
        doc.layers[0].set_pixel(1, 0, [0, 255, 0, 128]);
        doc.layers[0].set_pixel(3, 1, [0, 0, 255, 255]);
        doc
    }

    #[test]
    fn png_roundtrip_preserves_alpha_exactly() {
        let doc = pattern();
        let png = export_png(&doc).unwrap();
        let back = import(&png, "p").unwrap().document;
        assert_eq!(*back.layers[0].pixels, compose::flatten(&doc));
        assert_eq!((back.width, back.height), (4, 2));
    }

    #[test]
    fn jpeg_composites_over_background() {
        let doc = Document::new("t", 8, 8, None).unwrap(); // totalmente transparente
        let jpg = export_jpeg(&doc, 90, [255, 255, 255]).unwrap();
        let back = import(&jpg, "t").unwrap().document;
        let px = back.layers[0].pixel(4, 4).unwrap();
        assert!(px[0] >= 250 && px[1] >= 250 && px[2] >= 250 && px[3] == 255);
    }

    #[test]
    fn jpeg_quality_changes_size() {
        let mut doc = Document::new("n", 64, 64, Some([0, 0, 0, 255])).unwrap();
        for y in 0..64 {
            for x in 0..64 {
                doc.layers[0].set_pixel(
                    x,
                    y,
                    [(x * 4) as u8, (y * 4) as u8, ((x * y) % 256) as u8, 255],
                );
            }
        }
        let low = export_jpeg(&doc, 10, [0; 3]).unwrap().len();
        let high = export_jpeg(&doc, 95, [0; 3]).unwrap().len();
        assert!(low < high);
    }

    #[test]
    fn exif_orientation_is_applied() {
        // JPEG 4x2 com tag EXIF Orientation=6 (girar 90° horário) inserida manualmente.
        let doc = Document::new("o", 4, 2, Some([200, 50, 50, 255])).unwrap();
        let jpg = export_jpeg(&doc, 90, [0; 3]).unwrap();
        let mut exif: Vec<u8> = b"Exif\0\0".to_vec();
        exif.extend_from_slice(&[
            0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, // cabeçalho TIFF little-endian
            0x01, 0x00, // 1 entrada
            0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00,
            0x00, // Orientation=6
            0x00, 0x00, 0x00, 0x00,
        ]);
        let mut with_exif = vec![0xFF, 0xD8, 0xFF, 0xE1];
        with_exif.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        with_exif.extend_from_slice(&exif);
        with_exif.extend_from_slice(&jpg[2..]);
        let imp = import(&with_exif, "o").unwrap();
        assert_eq!((imp.document.width, imp.document.height), (2, 4));
        assert!(imp.warnings.iter().any(|w| w.contains("EXIF")));
    }

    #[test]
    fn rejects_garbage_and_unsupported() {
        assert!(import(b"isto nao e imagem", "x").is_err());
        assert!(import(&[], "x").is_err());
    }
    #[test]
    fn profile_is_embedded_read_back_and_converted_on_request() {
        let mut doc = Document::new("p", 4, 4, Some([255, 0, 0, 255])).unwrap();
        doc.icc = crate::color::builtin_profiles()
            .into_iter()
            .find(|p| p.name == "Display P3");
        // Embutir: pixels intactos, perfil volta ao importar, com aviso.
        let png = export_png_with(&doc, ColorOut::Embed).unwrap();
        let back = import(&png, "p").unwrap();
        assert_eq!(
            back.document.icc.as_ref().map(|p| p.name.as_str()),
            Some("Display P3")
        );
        assert_eq!(back.document.layers[0].pixel(0, 0), Some([255, 0, 0, 255]));
        assert!(back.warnings.iter().any(|w| w.contains("Display P3")));
        // Converter para sRGB: o vermelho P3 satura em sRGB e o perfil gravado é sRGB.
        let conv = export_png_with(&doc, ColorOut::ToSrgb).unwrap();
        let c = import(&conv, "c").unwrap();
        assert!(c
            .document
            .icc
            .as_ref()
            .is_some_and(|p| p.name.to_lowercase().contains("srgb")));
        let px = c.document.layers[0].pixel(0, 0).unwrap();
        assert!(px[0] >= 250 && px[1] <= 3 && px[2] <= 3, "{px:?}");
        // Sem perfil.
        let stripped = import(&export_png_with(&doc, ColorOut::Strip).unwrap(), "s").unwrap();
        assert!(stripped.document.icc.is_none());
        // JPEG também carrega o perfil.
        let jpg = export_jpeg_with(&doc, 90, [0; 3], ColorOut::Embed).unwrap();
        assert!(import(&jpg, "j").unwrap().document.icc.is_some());
        // Documento sem perfil exporta sem perfil.
        let plain = Document::new("n", 2, 2, None).unwrap();
        assert!(import(&export_png(&plain).unwrap(), "n")
            .unwrap()
            .document
            .icc
            .is_none());
    }
}
