//! PDF totalmente local: importar páginas (rasterizadas, com `hayro`) e exportar PDF raster
//! (com `pdf-writer`). Nada é enviado para fora; senhas só existem em memória durante a abertura.
//!
//! Limitações declaradas: páginas importadas viram imagens (o texto não fica editável); a
//! exportação grava imagens (sem texto nem vetores preservados); recursos de PDF não
//! interpretados são ignorados.

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression as ZLevel;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::vello_cpu::color::palette::css::TRANSPARENT;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};
use pdf_writer::{Content, Filter, Finish, Name, Rect, Ref, TextStr};

use crate::compose;
use crate::document::{Document, Layer};

/// Maior lado (px) de uma página rasterizada na importação.
pub const MAX_RENDER_SIDE: u32 = 12_000;
/// Limite de páginas que a importação lista.
pub const MAX_PAGES: usize = 5000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// O arquivo é protegido e a senha está ausente.
    NeedsPassword,
    /// Senha incorreta (ou criptografia não suportada).
    WrongPassword,
    Invalid,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::NeedsPassword => write!(f, "O PDF é protegido por senha."),
            OpenError::WrongPassword => write!(f, "Senha incorreta ou criptografia não suportada."),
            OpenError::Invalid => write!(f, "PDF inválido ou corrompido."),
        }
    }
}

fn open(bytes: &[u8], password: &str) -> Result<Pdf, OpenError> {
    Pdf::new_with_password(bytes.to_vec(), password).map_err(|e| match e {
        LoadPdfError::Decryption(_) if password.is_empty() => OpenError::NeedsPassword,
        LoadPdfError::Decryption(_) => OpenError::WrongPassword,
        LoadPdfError::Invalid => OpenError::Invalid,
    })
}

/// Tamanho (largura, altura) em pontos de cada página.
pub fn page_sizes(bytes: &[u8], password: &str) -> Result<Vec<(f32, f32)>, OpenError> {
    let pdf = open(bytes, password)?;
    Ok(pdf
        .pages()
        .iter()
        .take(MAX_PAGES)
        .map(|p| p.render_dimensions())
        .collect())
}

/// Parâmetros da rasterização de uma página.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderOpts {
    pub dpi: f32,
    /// `None` = fundo transparente.
    pub background: Option<[u8; 3]>,
    /// Recorte em pontos (esquerda, topo, direita, base) retirado de cada lado.
    pub trim_pt: [f32; 4],
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            dpi: 150.0,
            background: Some([255, 255, 255]),
            trim_pt: [0.0; 4],
        }
    }
}

/// Rasteriza a página `index` (RGBA reto). Devolve `(pixels, largura, altura)`.
pub fn render_page(
    bytes: &[u8],
    password: &str,
    index: usize,
    o: &RenderOpts,
) -> Result<(Vec<u8>, u32, u32), String> {
    let pdf = open(bytes, password).map_err(|e| e.to_string())?;
    let pages = pdf.pages();
    let page = pages.get(index).ok_or("Página inexistente.")?;
    let (pw, ph) = page.render_dimensions();
    if !(pw.is_finite() && ph.is_finite()) || pw <= 0.0 || ph <= 0.0 {
        return Err("Página com dimensões inválidas.".into());
    }
    let mut scale = o.dpi.clamp(10.0, 1200.0) / 72.0;
    let longest = pw.max(ph) * scale;
    if longest > MAX_RENDER_SIDE as f32 {
        scale *= MAX_RENDER_SIDE as f32 / longest;
    }
    let pix = render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: TRANSPARENT,
        },
    );
    let (w, h) = (u32::from(pix.width()), u32::from(pix.height()));
    let src = pix.data_as_u8_slice();
    let mut px = Vec::with_capacity(src.len());
    for p in src.as_chunks::<4>().0 {
        let a = u32::from(p[3]);
        if a == 0 {
            px.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let un = |c: u8| ((u32::from(c) * 255 + a / 2) / a).min(255) as u8;
        px.extend_from_slice(&[un(p[0]), un(p[1]), un(p[2]), p[3]]);
    }
    // Fundo: compõe sobre a cor escolhida (o renderizador entrega alfa; sem fundo = transparente).
    if let Some(bgc) = o.background {
        for p in px.as_chunks_mut::<4>().0 {
            let a = u32::from(p[3]);
            for (v, bg) in p[..3].iter_mut().zip(bgc) {
                *v = ((u32::from(*v) * a + u32::from(bg) * (255 - a) + 127) / 255) as u8;
            }
            p[3] = 255;
        }
    }
    // Recorte.
    let t = o.trim_pt.map(|v| (v.max(0.0) * scale).round() as u32);
    if t.iter().any(|v| *v > 0) {
        let (x0, y0) = (t[0].min(w.saturating_sub(1)), t[1].min(h.saturating_sub(1)));
        let (x1, y1) = (
            w.saturating_sub(t[2]).max(x0 + 1),
            h.saturating_sub(t[3]).max(y0 + 1),
        );
        let (nw, nh) = (x1 - x0, y1 - y0);
        let mut out = Vec::with_capacity(nw as usize * nh as usize * 4);
        for y in y0..y1 {
            let s = (y as usize * w as usize + x0 as usize) * 4;
            out.extend_from_slice(&px[s..s + nw as usize * 4]);
        }
        return Ok((out, nw, nh));
    }
    Ok((px, w, h))
}

/// Importa as páginas escolhidas como documentos independentes (camada raster única).
pub fn import_pages(
    bytes: &[u8],
    password: &str,
    pages: &[usize],
    base_name: &str,
    o: &RenderOpts,
) -> Result<Vec<Document>, String> {
    let mut docs = Vec::new();
    for &i in pages {
        let (px, w, h) = render_page(bytes, password, i, o)?;
        let name = format!("{base_name} - p. {}", i + 1);
        let layer = Layer::from_pixels(format!("Página {}", i + 1), w, h, px);
        docs.push(Document::from_parts(
            name,
            w,
            h,
            o.dpi.clamp(10.0, 1200.0),
            vec![layer],
        ));
    }
    Ok(docs)
}

// ---------------------------------------------------------------------------------------
// Exportação

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageSize {
    /// A página tem o tamanho da imagem na resolução escolhida (sem margens).
    FitImage,
    A3,
    A4,
    A5,
    Letter,
    Legal,
    /// Milímetros (largura, altura) em retrato.
    Custom(f32, f32),
}

impl PageSize {
    pub fn mm(self) -> Option<(f32, f32)> {
        Some(match self {
            PageSize::FitImage => return None,
            PageSize::A3 => (297.0, 420.0),
            PageSize::A4 => (210.0, 297.0),
            PageSize::A5 => (148.0, 210.0),
            PageSize::Letter => (215.9, 279.4),
            PageSize::Legal => (215.9, 355.6),
            PageSize::Custom(w, h) => (w, h),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Escolhe retrato ou paisagem conforme a imagem.
    Auto,
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    /// Flate (sem perdas), com máscara de alfa.
    Lossless,
    /// JPEG (com perdas, sem alfa: composto sobre branco).
    Jpeg(u8),
}

#[derive(Debug, Clone)]
pub struct PdfSetup {
    pub size: PageSize,
    pub orientation: Orientation,
    pub margin_mm: f32,
    /// Resolução usada em `FitImage` (define o tamanho físico da página).
    pub dpi: f32,
    pub compression: Compression,
    pub title: String,
}

impl Default for PdfSetup {
    fn default() -> Self {
        Self {
            size: PageSize::A4,
            orientation: Orientation::Auto,
            margin_mm: 10.0,
            dpi: 150.0,
            compression: Compression::Lossless,
            title: String::new(),
        }
    }
}

const PT_PER_MM: f32 = 72.0 / 25.4;

/// Geometria de uma página: tamanho em pontos e posição/tamanho da imagem.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageLayout {
    pub page_w: f32,
    pub page_h: f32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Resolução efetiva da imagem na página (pixels por polegada).
    pub effective_dpi: f32,
}

pub fn layout(img_w: u32, img_h: u32, s: &PdfSetup) -> PageLayout {
    let (iw, ih) = (img_w.max(1) as f32, img_h.max(1) as f32);
    match s.size.mm() {
        None => {
            let dpi = s.dpi.clamp(10.0, 2400.0);
            let (w, h) = (iw * 72.0 / dpi, ih * 72.0 / dpi);
            PageLayout {
                page_w: w,
                page_h: h,
                x: 0.0,
                y: 0.0,
                w,
                h,
                effective_dpi: dpi,
            }
        }
        Some((mw, mh)) => {
            let landscape = match s.orientation {
                Orientation::Portrait => false,
                Orientation::Landscape => true,
                Orientation::Auto => iw > ih,
            };
            let (pw, ph) = if landscape { (mh, mw) } else { (mw, mh) };
            let (page_w, page_h) = (pw * PT_PER_MM, ph * PT_PER_MM);
            let m = (s.margin_mm.max(0.0) * PT_PER_MM).min(page_w.min(page_h) / 2.5);
            let (aw, ah) = (page_w - 2.0 * m, page_h - 2.0 * m);
            let k = (aw / iw).min(ah / ih);
            let (w, h) = (iw * k, ih * k);
            PageLayout {
                page_w,
                page_h,
                x: (page_w - w) / 2.0,
                y: (page_h - h) / 2.0,
                w,
                h,
                effective_dpi: iw / (w / 72.0),
            }
        }
    }
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), ZLevel::new(6));
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

/// Exporta documentos como um PDF de várias páginas (uma imagem por página).
pub fn export_pdf(docs: &[&Document], setup: &PdfSetup) -> Result<Vec<u8>, String> {
    if docs.is_empty() {
        return Err("Escolha ao menos um documento.".into());
    }
    let mut pdf = pdf_writer::Pdf::new();
    let catalog = Ref::new(1);
    let tree = Ref::new(2);
    let info = Ref::new(3);
    let mut next = 4;
    let mut alloc = || {
        let r = Ref::new(next);
        next += 1;
        r
    };
    let mut kids = Vec::new();
    for doc in docs {
        let rgba = compose::flatten(doc);
        let (w, h) = (doc.width, doc.height);
        let lay = layout(w, h, setup);
        let (page, content, image) = (alloc(), alloc(), alloc());
        kids.push(page);
        let smask = alloc();
        let has_alpha = rgba.as_chunks::<4>().0.iter().any(|p| p[3] < 255);
        let name = Name(b"Im0");
        let mut resources_ok = true;
        match setup.compression {
            Compression::Lossless => {
                let rgb: Vec<u8> = rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| [p[0], p[1], p[2]])
                    .collect();
                let z = deflate(&rgb);
                let mut img = pdf.image_xobject(image, &z);
                img.filter(Filter::FlateDecode);
                img.width(w as i32);
                img.height(h as i32);
                img.color_space().device_rgb();
                img.bits_per_component(8);
                if has_alpha {
                    img.s_mask(smask);
                }
                img.finish();
                if has_alpha {
                    let a: Vec<u8> = rgba.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
                    let z = deflate(&a);
                    let mut m = pdf.image_xobject(smask, &z);
                    m.filter(Filter::FlateDecode);
                    m.width(w as i32);
                    m.height(h as i32);
                    m.color_space().device_gray();
                    m.bits_per_component(8);
                    m.finish();
                }
            }
            Compression::Jpeg(q) => {
                let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
                for p in rgba.as_chunks::<4>().0 {
                    let a = u32::from(p[3]);
                    for v in &p[..3] {
                        rgb.push(((u32::from(*v) * a + 255 * (255 - a) + 127) / 255) as u8);
                    }
                }
                let mut jpg = Vec::new();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, q.clamp(1, 100))
                    .encode(&rgb, w, h, image::ExtendedColorType::Rgb8)
                    .map_err(|e| e.to_string())?;
                let mut img = pdf.image_xobject(image, &jpg);
                img.filter(Filter::DctDecode);
                img.width(w as i32);
                img.height(h as i32);
                img.color_space().device_rgb();
                img.bits_per_component(8);
                img.finish();
                resources_ok = !has_alpha || resources_ok;
            }
        }
        let _ = resources_ok;
        let mut c = Content::new();
        c.save_state();
        c.transform([lay.w, 0.0, 0.0, lay.h, lay.x, lay.y]);
        c.x_object(name);
        c.restore_state();
        pdf.stream(content, &c.finish());
        let mut p = pdf.page(page);
        p.media_box(Rect::new(0.0, 0.0, lay.page_w, lay.page_h));
        p.parent(tree);
        p.contents(content);
        p.resources().x_objects().pair(name, image);
        p.finish();
    }
    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree)
        .kids(kids.iter().copied())
        .count(kids.len() as i32);
    let title = if setup.title.trim().is_empty() {
        docs[0].name.clone()
    } else {
        setup.title.clone()
    };
    pdf.document_info(info)
        .title(TextStr(&title))
        .producer(TextStr("Orvalith"));
    Ok(pdf.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(w: u32, h: u32, c: [u8; 4]) -> Document {
        Document::new("d", w, h, Some(c)).unwrap()
    }

    #[test]
    fn layout_fits_with_margins_orientation_and_effective_dpi() {
        let s = PdfSetup::default();
        let l = layout(1000, 500, &s); // paisagem automática
        assert!(l.page_w > l.page_h);
        assert!((l.page_w - 297.0 * PT_PER_MM).abs() < 0.01);
        assert!(
            (l.w - (l.page_w - 20.0 * PT_PER_MM)).abs() < 0.01,
            "limitado pela largura útil"
        );
        assert!((l.x - 10.0 * PT_PER_MM).abs() < 0.01);
        assert!((l.effective_dpi - 1000.0 / (l.w / 72.0)).abs() < 0.01);
        let portrait = layout(
            1000,
            500,
            &PdfSetup {
                orientation: Orientation::Portrait,
                ..s.clone()
            },
        );
        assert!(portrait.page_h > portrait.page_w);
        let fit = layout(
            300,
            150,
            &PdfSetup {
                size: PageSize::FitImage,
                dpi: 300.0,
                ..s
            },
        );
        assert!(
            (fit.page_w - 72.0).abs() < 1e-3 && (fit.page_h - 36.0).abs() < 1e-3,
            "300 px a 300 ppi = 1 polegada"
        );
    }

    #[test]
    fn exported_pdf_is_valid_multipage_and_rereads_with_independent_tools() {
        let a = doc(40, 30, [200, 30, 30, 255]);
        let mut b = doc(30, 40, [30, 30, 200, 255]);
        b.layers[0].set_pixel(0, 0, [0, 0, 0, 0]); // alfa → máscara
        let bytes = export_pdf(
            &[&a, &b],
            &PdfSetup {
                title: "Teste".into(),
                ..PdfSetup::default()
            },
        )
        .unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        // Leitor independente 1: lopdf conta páginas, tamanho e imagens.
        let l = lopdf::Document::load_mem(&bytes).unwrap();
        assert_eq!(l.get_pages().len(), 2);
        let first = *l.get_pages().get(&1).unwrap();
        let mb = l
            .get_object(first)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        let w = mb[2].as_float().unwrap();
        assert!(
            (w - 297.0 * PT_PER_MM).abs() < 0.5,
            "A4 retrato para a imagem 40×30? paisagem auto: {w}"
        );
        let images = l.objects.values().filter(|o| matches!(o, lopdf::Object::Stream(s) if s.dict.get(b"Subtype").ok().and_then(|v| v.as_name().ok()) == Some(b"Image"))).count();
        assert_eq!(images, 3, "2 imagens + 1 máscara de alfa");
        // Leitor independente 2: o renderizador (hayro) abre e rasteriza as páginas.
        assert_eq!(page_sizes(&bytes, "").unwrap().len(), 2);
        let (px, rw, rh) = render_page(
            &bytes,
            "",
            0,
            &RenderOpts {
                dpi: 72.0,
                ..RenderOpts::default()
            },
        )
        .unwrap();
        assert_eq!(px.len(), (rw * rh * 4) as usize);
        // A imagem vermelha aparece no centro da página; a margem é branca.
        let c = ((rh / 2) * rw + rw / 2) as usize * 4;
        assert!(
            px[c] > 150 && px[c + 1] < 80 && px[c + 2] < 80,
            "{:?}",
            &px[c..c + 4]
        );
        assert_eq!(&px[0..4], &[255, 255, 255, 255], "margem branca");
    }

    #[test]
    fn jpeg_mode_and_fit_image_and_errors() {
        let d = doc(64, 48, [10, 120, 60, 255]);
        let lossless = export_pdf(
            &[&d],
            &PdfSetup {
                size: PageSize::FitImage,
                dpi: 96.0,
                ..PdfSetup::default()
            },
        )
        .unwrap();
        let jpeg = export_pdf(
            &[&d],
            &PdfSetup {
                size: PageSize::FitImage,
                dpi: 96.0,
                compression: Compression::Jpeg(70),
                ..PdfSetup::default()
            },
        )
        .unwrap();
        for bytes in [&lossless, &jpeg] {
            let (px, w, h) = render_page(
                bytes,
                "",
                0,
                &RenderOpts {
                    dpi: 96.0,
                    ..RenderOpts::default()
                },
            )
            .unwrap();
            assert_eq!((w, h), (64, 48));
            let p = &px[((24 * w + 32) * 4) as usize..][..4];
            assert!(
                p[0].abs_diff(10) < 12 && p[1].abs_diff(120) < 12 && p[2].abs_diff(60) < 12,
                "{p:?}"
            );
        }
        assert!(export_pdf(&[], &PdfSetup::default()).is_err());
        assert_eq!(
            page_sizes(b"isto nao e pdf", "").unwrap_err(),
            OpenError::Invalid
        );
        assert!(render_page(&lossless, "", 9, &RenderOpts::default()).is_err());
    }

    #[test]
    fn import_pages_honours_dpi_background_and_trim() {
        let d = doc(72, 36, [255, 0, 0, 255]);
        let bytes = export_pdf(
            &[&d],
            &PdfSetup {
                size: PageSize::FitImage,
                dpi: 72.0,
                ..PdfSetup::default()
            },
        )
        .unwrap();
        let o = RenderOpts {
            dpi: 144.0,
            ..RenderOpts::default()
        };
        let docs = import_pages(&bytes, "", &[0], "arq", &o).unwrap();
        assert_eq!(
            (docs[0].width, docs[0].height, docs[0].ppi),
            (144, 72, 144.0)
        );
        assert_eq!(docs[0].name, "arq - p. 1");
        // Recorte de 10 pt à esquerda e 6 pt em cima, a 144 dpi = 20 px e 12 px.
        let t = import_pages(
            &bytes,
            "",
            &[0],
            "a",
            &RenderOpts {
                trim_pt: [10.0, 6.0, 0.0, 0.0],
                ..o
            },
        )
        .unwrap();
        assert_eq!((t[0].width, t[0].height), (144 - 20, 72 - 12));
        // Fundo transparente x branco: a imagem cobre tudo, então testa pelo recorte de uma página maior.
        let big = export_pdf(&[&d], &PdfSetup::default()).unwrap();
        let tr = import_pages(
            &big,
            "",
            &[0],
            "a",
            &RenderOpts {
                background: None,
                dpi: 36.0,
                ..RenderOpts::default()
            },
        )
        .unwrap();
        assert_eq!(
            tr[0].layers[0].pixel(0, 0).unwrap()[3],
            0,
            "margem transparente"
        );
        let wh = import_pages(
            &big,
            "",
            &[0],
            "a",
            &RenderOpts {
                dpi: 36.0,
                ..RenderOpts::default()
            },
        )
        .unwrap();
        assert_eq!(wh[0].layers[0].pixel(0, 0), Some([255, 255, 255, 255]));
        let col = import_pages(
            &big,
            "",
            &[0],
            "a",
            &RenderOpts {
                background: Some([10, 20, 30]),
                dpi: 36.0,
                ..RenderOpts::default()
            },
        )
        .unwrap();
        assert_eq!(col[0].layers[0].pixel(0, 0), Some([10, 20, 30, 255]));
    }

    #[test]
    fn huge_requests_are_capped() {
        let d = doc(10, 10, [0, 0, 0, 255]);
        let bytes = export_pdf(
            &[&d],
            &PdfSetup {
                size: PageSize::A3,
                ..PdfSetup::default()
            },
        )
        .unwrap();
        let (_, w, h) = render_page(
            &bytes,
            "",
            0,
            &RenderOpts {
                dpi: 1200.0,
                ..RenderOpts::default()
            },
        )
        .unwrap();
        assert!(w.max(h) <= MAX_RENDER_SIDE + 1, "{w}x{h}");
    }
    #[test]
    fn password_protected_pdfs_ask_for_a_password_and_reject_wrong_ones() {
        use lopdf::encryption::{EncryptionState, EncryptionVersion, Permissions};
        let d = doc(20, 20, [0, 90, 200, 255]);
        let plain = export_pdf(
            &[&d],
            &PdfSetup {
                size: PageSize::FitImage,
                dpi: 72.0,
                ..PdfSetup::default()
            },
        )
        .unwrap();
        let mut l = lopdf::Document::load_mem(&plain).unwrap();
        l.trailer.set(
            "ID",
            lopdf::Object::Array(vec![
                lopdf::Object::string_literal("0123456789abcdef"),
                lopdf::Object::string_literal("0123456789abcdef"),
            ]),
        );
        let version = EncryptionVersion::V2 {
            document: &l,
            owner_password: "dono",
            user_password: "segredo",
            key_length: 128,
            permissions: Permissions::all(),
        };
        let state = EncryptionState::try_from(version).unwrap();
        l.encrypt(&state).unwrap();
        let mut locked = Vec::new();
        l.save_to(&mut locked).unwrap();
        assert_eq!(
            page_sizes(&locked, "").unwrap_err(),
            OpenError::NeedsPassword
        );
        assert_eq!(
            page_sizes(&locked, "errada").unwrap_err(),
            OpenError::WrongPassword
        );
        assert_eq!(page_sizes(&locked, "segredo").unwrap().len(), 1);
        let (px, w, h) = render_page(
            &locked,
            "segredo",
            0,
            &RenderOpts {
                dpi: 72.0,
                ..RenderOpts::default()
            },
        )
        .unwrap();
        let c = ((h / 2) * w + w / 2) as usize * 4;
        assert!(
            px[c + 2] > 150 && px[c] < 60,
            "conteúdo decifrado: {:?}",
            &px[c..c + 4]
        );
    }
}
