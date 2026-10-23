//! Gerenciamento de cor (perfis ICC) sobre RGBA8.
//!
//! O documento trabalha em RGB 8 bits. `Document.icc` é o perfil **atribuído** aos pixels
//! (`None` = sRGB presumido). *Atribuir* só troca a etiqueta; *converter* recalcula os pixels.
//! Suporte declarado: perfis RGB de matriz/LUT lidos pelo `moxcms`; perfis de cinza e CMYK
//! não são aceitos como perfil do documento (a imagem é lida como RGB e o perfil é descartado
//! com aviso).

use std::sync::Arc;

use moxcms::{ColorProfile, DataColorSpace, Layout, ProfileText, TransformOptions};

use crate::document::{Document, Layer};
use crate::history::Command;

/// Perfil ICC embutido no documento.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IccProfile {
    pub name: String,
    pub data: Arc<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileKind {
    Rgb,
    Gray,
    Cmyk,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileInfo {
    pub description: String,
    pub kind: ProfileKind,
    pub is_srgb: bool,
}

/// Limite de tamanho aceito para um perfil (evita arquivos hostis).
pub const MAX_PROFILE_BYTES: usize = 16 * 1024 * 1024;

fn text_of(t: &ProfileText) -> String {
    match t {
        ProfileText::PlainString(s) => s.clone(),
        ProfileText::Description(d) => {
            if d.ascii_string.is_empty() {
                d.unicode_string.clone()
            } else {
                d.ascii_string.clone()
            }
        }
        ProfileText::Localizable(v) => v.first().map(|l| l.value.clone()).unwrap_or_default(),
    }
}

fn parse(data: &[u8]) -> Result<ColorProfile, String> {
    if data.len() > MAX_PROFILE_BYTES {
        return Err("Perfil ICC grande demais.".into());
    }
    ColorProfile::new_from_slice(data).map_err(|e| format!("Perfil ICC inválido: {e:?}"))
}

/// Lê o essencial de um perfil ICC.
pub fn inspect(data: &[u8]) -> Result<ProfileInfo, String> {
    let p = parse(data)?;
    let description = p.description.as_ref().map(text_of).unwrap_or_default();
    let kind = match p.color_space {
        DataColorSpace::Rgb => ProfileKind::Rgb,
        DataColorSpace::Gray => ProfileKind::Gray,
        DataColorSpace::Cmyk => ProfileKind::Cmyk,
        _ => ProfileKind::Other,
    };
    let is_srgb = description.to_lowercase().contains("srgb");
    Ok(ProfileInfo {
        description,
        kind,
        is_srgb,
    })
}

/// Perfis embutidos oferecidos no app (nome, bytes ICC).
pub fn builtin_profiles() -> Vec<IccProfile> {
    let make = |name: &str, p: ColorProfile| {
        p.encode().ok().map(|d| IccProfile {
            name: name.to_string(),
            data: Arc::new(d),
        })
    };
    [
        ("sRGB", ColorProfile::new_srgb()),
        ("Adobe RGB (1998)", ColorProfile::new_adobe_rgb()),
        ("Display P3", ColorProfile::new_display_p3()),
        ("Rec. 2020", ColorProfile::new_bt2020()),
    ]
    .into_iter()
    .filter_map(|(n, p)| make(n, p))
    .collect()
}

/// Cria um perfil de documento a partir de bytes ICC; só aceita RGB.
pub fn profile_from_bytes(data: &[u8]) -> Result<IccProfile, String> {
    let info = inspect(data)?;
    if info.kind != ProfileKind::Rgb {
        return Err(format!(
            "O perfil \"{}\" não é RGB; apenas perfis RGB são suportados.",
            info.description
        ));
    }
    let name = if info.description.is_empty() {
        "Perfil sem nome".to_string()
    } else {
        info.description
    };
    Ok(IccProfile {
        name,
        data: Arc::new(data.to_vec()),
    })
}

/// `None` = sRGB presumido. Perfis cuja descrição contém "sRGB" não exigem conversão na tela.
pub fn is_srgb(icc: Option<&IccProfile>) -> bool {
    match icc {
        None => true,
        Some(p) => p.name.to_lowercase().contains("srgb"),
    }
}

fn load(icc: Option<&IccProfile>) -> Result<ColorProfile, String> {
    match icc {
        None => Ok(ColorProfile::new_srgb()),
        Some(p) => parse(&p.data),
    }
}

/// Converte um buffer RGBA8 (alfa reto) entre perfis. O alfa não muda.
pub fn convert_rgba(
    buf: &mut [u8],
    from: Option<&IccProfile>,
    to: Option<&IccProfile>,
) -> Result<(), String> {
    let (src, dst) = (load(from)?, load(to)?);
    let t = src
        .create_transform_8bit(
            Layout::Rgba,
            &dst,
            Layout::Rgba,
            TransformOptions::default(),
        )
        .map_err(|e| format!("Não foi possível criar a conversão de cor: {e:?}"))?;
    let input = buf.to_vec();
    t.transform(&input, buf)
        .map_err(|e| format!("Falha na conversão de cor: {e:?}"))?;
    // Preserva o alfa exatamente, independentemente do motor.
    for (o, i) in buf
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(input.as_chunks::<4>().0)
    {
        o[3] = i[3];
    }
    Ok(())
}

/// Transformação reutilizável para a prévia (documento → sRGB da tela).
pub struct Preview {
    t: Arc<moxcms::Transform8BitExecutor>,
}

impl Preview {
    /// `None` quando o documento já está em sRGB (nada a fazer).
    pub fn new(icc: Option<&IccProfile>) -> Result<Option<Self>, String> {
        if is_srgb(icc) {
            return Ok(None);
        }
        let src = load(icc)?;
        let t = src
            .create_transform_8bit(
                Layout::Rgba,
                &ColorProfile::new_srgb(),
                Layout::Rgba,
                TransformOptions::default(),
            )
            .map_err(|e| format!("{e:?}"))?;
        Ok(Some(Self { t }))
    }

    pub fn apply(&self, buf: &mut [u8]) {
        let input = buf.to_vec();
        if self.t.transform(&input, buf).is_err() {
            buf.copy_from_slice(&input);
            return;
        }
        for (o, i) in buf
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(input.as_chunks::<4>().0)
        {
            o[3] = i[3];
        }
    }
}

/// Resultado de converter o documento para outro perfil.
pub struct Converted {
    pub layers: Vec<Layer>,
    /// Camadas de texto/forma que perderam a origem editável (os pixels foram convertidos).
    pub rasterized: usize,
}

/// Converte os pixels de todas as camadas raster de `doc.icc` (ou sRGB) para `target`.
/// Camadas de texto/forma são rasterizadas; camadas de ajuste e máscaras não mudam.
pub fn convert_document(doc: &Document, target: Option<&IccProfile>) -> Result<Converted, String> {
    let mut layers = doc.layers.clone();
    let mut rasterized = 0;
    for l in &mut layers {
        if l.is_group() || l.adjustment.is_some() {
            continue;
        }
        let mut px = l.pixels.as_ref().clone();
        convert_rgba(&mut px, doc.icc.as_ref(), target)?;
        l.pixels = Arc::new(px);
        if l.source.take().is_some() {
            rasterized += 1;
        }
    }
    Ok(Converted { layers, rasterized })
}

/// Troca o perfil e/ou as camadas numa única ação de histórico.
pub struct ColorEdit {
    label: String,
    before_layers: Vec<Layer>,
    after_layers: Vec<Layer>,
    before_icc: Option<IccProfile>,
    after_icc: Option<IccProfile>,
}

impl ColorEdit {
    pub fn new(
        label: &str,
        before_layers: Vec<Layer>,
        after_layers: Vec<Layer>,
        before_icc: Option<IccProfile>,
        after_icc: Option<IccProfile>,
    ) -> Self {
        Self {
            label: label.to_string(),
            before_layers,
            after_layers,
            before_icc,
            after_icc,
        }
    }
}

impl Command for ColorEdit {
    fn label(&self) -> &str {
        &self.label
    }
    fn apply(&mut self, doc: &mut Document) {
        doc.layers = self.after_layers.clone();
        doc.icc = self.after_icc.clone();
    }
    fn revert(&mut self, doc: &mut Document) {
        doc.layers = self.before_layers.clone();
        doc.icc = self.before_icc.clone();
    }
    fn bytes(&self) -> usize {
        let unique = |a: &[Layer], b: &[Layer]| -> usize {
            a.iter()
                .filter(|l| !b.iter().any(|o| Arc::ptr_eq(&o.pixels, &l.pixels)))
                .map(|l| l.pixels.len())
                .sum()
        };
        unique(&self.after_layers, &self.before_layers)
            + unique(&self.before_layers, &self.after_layers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p3() -> IccProfile {
        builtin_profiles()
            .into_iter()
            .find(|p| p.name == "Display P3")
            .unwrap()
    }

    fn adobe() -> IccProfile {
        builtin_profiles()
            .into_iter()
            .find(|p| p.name.starts_with("Adobe"))
            .unwrap()
    }

    #[test]
    fn builtin_profiles_are_inspectable() {
        let all = builtin_profiles();
        assert_eq!(all.len(), 4);
        for p in &all {
            let info = inspect(&p.data).unwrap();
            assert_eq!(info.kind, ProfileKind::Rgb, "{}", p.name);
        }
        assert!(inspect(&all[0].data).unwrap().is_srgb || is_srgb(Some(&all[0])));
        assert!(inspect(b"lixo").is_err());
        assert!(profile_from_bytes(&[0u8; 200]).is_err());
    }

    #[test]
    fn srgb_to_srgb_is_identity_and_alpha_is_kept() {
        let mut px = vec![10, 20, 30, 255, 200, 100, 50, 77, 0, 0, 0, 0];
        let orig = px.clone();
        let srgb = builtin_profiles().remove(0);
        convert_rgba(&mut px, Some(&srgb), None).unwrap();
        for (a, b) in px.iter().zip(&orig) {
            assert!((i32::from(*a) - i32::from(*b)).abs() <= 1);
        }
        for i in 0..3 {
            assert_eq!(px[i * 4 + 3], orig[i * 4 + 3]);
        }
    }

    #[test]
    fn known_conversion_srgb_red_to_display_p3() {
        // sRGB (255,0,0) em Display P3 ≈ (234, 51, 35) (valor de referência público).
        let mut px = vec![255, 0, 0, 255];
        convert_rgba(&mut px, None, Some(&p3())).unwrap();
        let tol = |a: u8, b: i32| (i32::from(a) - b).abs() <= 4;
        assert!(
            tol(px[0], 234) && tol(px[1], 51) && tol(px[2], 35),
            "{px:?}"
        );
        assert_eq!(px[3], 255);
    }

    #[test]
    fn roundtrip_through_wide_gamut_is_close_for_in_gamut_colors() {
        let mut px = vec![120, 90, 60, 255, 30, 200, 90, 128, 128, 128, 128, 255];
        let orig = px.clone();
        let a = adobe();
        convert_rgba(&mut px, None, Some(&a)).unwrap();
        assert_ne!(px, orig);
        convert_rgba(&mut px, Some(&a), None).unwrap();
        for (x, y) in px.iter().zip(&orig) {
            assert!(
                (i32::from(*x) - i32::from(*y)).abs() <= 3,
                "{px:?} vs {orig:?}"
            );
        }
    }

    #[test]
    fn preview_is_none_for_srgb_and_converts_for_wide_gamut() {
        assert!(Preview::new(None).unwrap().is_none());
        let pv = Preview::new(Some(&p3())).unwrap().unwrap();
        let mut px = vec![234, 51, 35, 255];
        pv.apply(&mut px);
        assert!(px[0] >= 250 && px[1] <= 10 && px[2] <= 10, "{px:?}");
    }

    #[test]
    fn convert_document_rasterizes_sources_and_undo_restores() {
        use crate::vector::{Geom, ShapeSpec};
        use crate::LayerSource;
        let mut doc = Document::new("c", 4, 4, Some([200, 30, 30, 255])).unwrap();
        let spec = ShapeSpec::solid(
            Geom::Rect {
                x: 0.0,
                y: 0.0,
                w: 2.0,
                h: 2.0,
                radius: 0.0,
            },
            [0, 0, 255, 255],
        );
        let mut l = Layer::from_pixels("forma", 4, 4, crate::vector::render_shape(&spec, 4, 4));
        l.source = Some(LayerSource::Shape(spec));
        doc.layers.push(l);
        let before = doc.layers.clone();
        let out = convert_document(&doc, Some(&p3())).unwrap();
        assert_eq!(out.rasterized, 1);
        assert!(out.layers[1].source.is_none());
        assert_ne!(out.layers[0].pixels, before[0].pixels);
        let mut hist = crate::History::default();
        hist.execute(
            &mut doc,
            Box::new(ColorEdit::new(
                "Converter",
                before.clone(),
                out.layers,
                None,
                Some(p3()),
            )),
        );
        assert!(doc.icc.is_some());
        hist.undo(&mut doc);
        assert!(doc.icc.is_none());
        assert_eq!(doc.layers[0].pixels, before[0].pixels);
        assert!(doc.layers[1].source.is_some());
    }
}
