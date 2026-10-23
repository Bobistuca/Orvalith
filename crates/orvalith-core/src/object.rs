//! Objetos inteligentes: conteúdo original preservado (embutido, ou vinculado a um arquivo),
//! transformação reversível e filtros editáveis com máscara própria.
//!
//! A camada guarda os pixels renderizados (do tamanho do documento); `SmartObject` guarda o
//! que é preciso para renderizar de novo a partir do original sem perda: posição do centro,
//! escala, rotação e a lista de filtros. Mudar a escala várias vezes nunca degrada a imagem.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::document::{Document, Layer, LayerId};
use crate::filter::{self, mix_px, Filter};
use crate::geom::Rect;
use crate::imageio;
use crate::layerops::{normalize, Outcome};

/// Limite de filtros por objeto (valida arquivos).
pub const MAX_FILTERS: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartFilter {
    pub enabled: bool,
    pub filter: Filter,
}

#[derive(Debug, Clone)]
pub struct SmartObject {
    pub orig_w: u32,
    pub orig_h: u32,
    /// Pixels originais (RGBA8). `None` só quando o arquivo vinculado está ausente.
    pub pixels: Option<Arc<Vec<u8>>>,
    /// Caminho do arquivo vinculado; `None` = conteúdo embutido no projeto.
    pub link: Option<String>,
    /// Centro do objeto no documento.
    pub cx: f32,
    pub cy: f32,
    pub sx: f32,
    pub sy: f32,
    /// Graus, sentido horário (eixo y para baixo).
    pub rot: f32,
    pub filters: Vec<SmartFilter>,
    /// Máscara própria dos filtros (tamanho do documento; 255 = filtro aplicado).
    pub mask: Option<Arc<Vec<u8>>>,
}

impl PartialEq for SmartObject {
    fn eq(&self, o: &Self) -> bool {
        let same = |a: &Option<Arc<Vec<u8>>>, b: &Option<Arc<Vec<u8>>>| match (a, b) {
            (Some(x), Some(y)) => Arc::ptr_eq(x, y) || x == y,
            (None, None) => true,
            _ => false,
        };
        self.orig_w == o.orig_w
            && self.orig_h == o.orig_h
            && self.link == o.link
            && (self.cx, self.cy, self.sx, self.sy, self.rot) == (o.cx, o.cy, o.sx, o.sy, o.rot)
            && self.filters == o.filters
            && same(&self.pixels, &o.pixels)
            && same(&self.mask, &o.mask)
    }
}

/// Caminhos que nunca são seguidos: rede (UNC, URL) violaria o princípio offline.
pub fn link_allowed(path: &str) -> bool {
    let p = path.trim();
    !p.is_empty()
        && p.len() <= 4096
        && !p.starts_with("\\\\")
        && !p.starts_with("//")
        && !p.contains("://")
        && !p.contains('\0')
}

impl SmartObject {
    pub fn new(orig_w: u32, orig_h: u32, pixels: Vec<u8>, cx: f32, cy: f32) -> Self {
        Self {
            orig_w,
            orig_h,
            pixels: Some(Arc::new(pixels)),
            link: None,
            cx,
            cy,
            sx: 1.0,
            sy: 1.0,
            rot: 0.0,
            filters: Vec::new(),
            mask: None,
        }
    }

    pub fn is_linked(&self) -> bool {
        self.link.is_some()
    }

    /// O arquivo vinculado não está disponível (sem original, não dá para re-renderizar).
    pub fn is_missing(&self) -> bool {
        self.pixels.is_none()
    }

    pub fn reset_transform(&mut self) {
        self.sx = 1.0;
        self.sy = 1.0;
        self.rot = 0.0;
    }

    /// Limita os parâmetros a faixas válidas.
    pub fn sanitize(&mut self) {
        let f = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.cx = f(self.cx, 0.0).clamp(-1.0e6, 1.0e6);
        self.cy = f(self.cy, 0.0).clamp(-1.0e6, 1.0e6);
        for s in [&mut self.sx, &mut self.sy] {
            *s = f(*s, 1.0);
            if s.abs() < 0.01 {
                *s = 0.01_f32.copysign(*s);
            }
            *s = s.clamp(-100.0, 100.0);
        }
        self.rot = f(self.rot, 0.0).clamp(-3600.0, 3600.0);
        self.filters.truncate(MAX_FILTERS);
        for sf in &mut self.filters {
            sf.filter.sanitize();
        }
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.cx += dx;
        self.cy += dy;
    }

    fn warp(&self, src: &[u8], dw: u32, dh: u32) -> Vec<u8> {
        let (ow, oh) = (self.orig_w as usize, self.orig_h as usize);
        let (dwu, dhu) = (dw as usize, dh as usize);
        let mut out = vec![0u8; dwu * dhu * 4];
        let (s, c) = self.rot.to_radians().sin_cos();
        let (hw, hh) = (ow as f32 / 2.0, oh as f32 / 2.0);
        let texel = |xi: i64, yi: i64| -> [f32; 4] {
            if xi < 0 || yi < 0 || xi >= ow as i64 || yi >= oh as i64 {
                return [0.0; 4];
            }
            let i = (yi as usize * ow + xi as usize) * 4;
            let k = f32::from(src[i + 3]) / 255.0;
            [
                f32::from(src[i]) * k,
                f32::from(src[i + 1]) * k,
                f32::from(src[i + 2]) * k,
                f32::from(src[i + 3]),
            ]
        };
        for y in 0..dhu {
            for x in 0..dwu {
                let (vx, vy) = (x as f32 + 0.5 - self.cx, y as f32 + 0.5 - self.cy);
                let (ux, uy) = (c * vx + s * vy, -s * vx + c * vy);
                let u = ux / self.sx + hw - 0.5;
                let v = uy / self.sy + hh - 0.5;
                if u < -1.0 || v < -1.0 || u > ow as f32 || v > oh as f32 {
                    continue;
                }
                let (x0, y0) = (u.floor(), v.floor());
                let (fx, fy) = (u - x0, v - y0);
                let (xi, yi) = (x0 as i64, y0 as i64);
                let (a, b, cc, d) = (
                    texel(xi, yi),
                    texel(xi + 1, yi),
                    texel(xi, yi + 1),
                    texel(xi + 1, yi + 1),
                );
                let mut p = [0f32; 4];
                for k in 0..4 {
                    p[k] = (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy)
                        + (cc[k] * (1.0 - fx) + d[k] * fx) * fy;
                }
                if p[3] < 0.5 {
                    continue;
                }
                let k = 255.0 / p[3];
                let o = (y * dwu + x) * 4;
                out[o] = (p[0] * k + 0.5).clamp(0.0, 255.0) as u8;
                out[o + 1] = (p[1] * k + 0.5).clamp(0.0, 255.0) as u8;
                out[o + 2] = (p[2] * k + 0.5).clamp(0.0, 255.0) as u8;
                out[o + 3] = (p[3] + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
        out
    }

    /// Renderiza o objeto (filtros no original, depois transformação) no tamanho do documento.
    /// `None` se o original não está disponível.
    pub fn render(&self, dw: u32, dh: u32) -> Option<Vec<u8>> {
        let src = self.pixels.as_ref()?;
        let active: Vec<&SmartFilter> = self.filters.iter().filter(|f| f.enabled).collect();
        if active.is_empty() {
            return Some(self.warp(src, dw, dh));
        }
        let no = AtomicBool::new(false);
        let mut filtered = src.as_ref().clone();
        for f in active {
            filtered = filter::apply(&f.filter, &filtered, self.orig_w, self.orig_h, &no)?;
        }
        let with = self.warp(&filtered, dw, dh);
        let Some(mask) = self
            .mask
            .as_ref()
            .filter(|m| m.len() == dw as usize * dh as usize)
        else {
            return Some(with);
        };
        let without = self.warp(src, dw, dh);
        let mut out = without.clone();
        for (i, o) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let w = f32::from(mask[i]) / 255.0;
            if w >= 1.0 {
                *o = [
                    with[i * 4],
                    with[i * 4 + 1],
                    with[i * 4 + 2],
                    with[i * 4 + 3],
                ];
            } else if w > 0.0 {
                *o = mix_px(
                    *o,
                    [
                        with[i * 4],
                        with[i * 4 + 1],
                        with[i * 4 + 2],
                        with[i * 4 + 3],
                    ],
                    w,
                );
            }
        }
        Some(out)
    }
}

// ---------------------------------------------------------------------------------------
// Operações de documento

fn alpha_bbox(l: &Layer) -> Option<Rect> {
    let (w, h) = (l.width as usize, l.height as usize);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            if l.pixels[(y * w + x) * 4 + 3] > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    (x1 > x0).then(|| Rect::new(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
}

/// Converte uma camada raster comum em objeto inteligente (recorta o conteúdo visível).
pub fn convert_to_object(doc: &Document, id: LayerId) -> Result<Vec<Layer>, String> {
    let mut layers = doc.layers.clone();
    let l = layers
        .iter_mut()
        .find(|l| l.id == id)
        .ok_or("Camada não encontrada.")?;
    if l.is_group() || l.source.is_some() || l.adjustment.is_some() || l.object.is_some() {
        return Err(
            "Selecione uma camada raster comum (não grupo, texto/forma, ajuste ou objeto).".into(),
        );
    }
    let bb = alpha_bbox(l).ok_or("A camada está vazia: nada para converter.")?;
    let mut px = Vec::with_capacity(bb.w as usize * bb.h as usize * 4);
    for y in bb.y..bb.bottom() {
        let s = (y as usize * l.width as usize + bb.x as usize) * 4;
        px.extend_from_slice(&l.pixels[s..s + bb.w as usize * 4]);
    }
    let obj = SmartObject::new(
        bb.w,
        bb.h,
        px,
        bb.x as f32 + bb.w as f32 / 2.0,
        bb.y as f32 + bb.h as f32 / 2.0,
    );
    l.object = Some(obj);
    Ok(layers)
}

fn insert_above(doc: &Document, above: Option<LayerId>, mut layer: Layer) -> Outcome {
    let (parent, at) = match above.and_then(|id| doc.layer(id)) {
        Some(a) => (
            a.parent,
            doc.index_of(a.id).unwrap_or(0) + 1 + doc.descendants(a.id).len(),
        ),
        None => (None, doc.layers.len()),
    };
    layer.parent = parent;
    let id = layer.id;
    let mut layers = doc.layers.clone();
    layers.insert(at.min(layers.len()), layer);
    Outcome {
        layers: normalize(layers),
        select: vec![id],
        active: Some(id),
    }
}

/// Coloca uma imagem (PNG/JPEG em `bytes`) como objeto inteligente, centrado no documento.
/// `link` = caminho do arquivo para um objeto vinculado; `None` = embutido.
pub fn place_image(
    doc: &Document,
    bytes: &[u8],
    name: &str,
    link: Option<String>,
    above: Option<LayerId>,
) -> Result<Outcome, String> {
    if let Some(l) = &link {
        if !link_allowed(l) {
            return Err("Esse tipo de caminho (rede ou URL) não é permitido para vínculos.".into());
        }
    }
    let imp = imageio::import(bytes, name)?;
    let src = &imp.document.layers[0];
    let mut obj = SmartObject::new(
        src.width,
        src.height,
        src.pixels.as_ref().clone(),
        doc.width as f32 / 2.0,
        doc.height as f32 / 2.0,
    );
    obj.link = link;
    let px = obj
        .render(doc.width, doc.height)
        .ok_or("Falha ao renderizar o objeto.")?;
    let mut layer = Layer::from_pixels(name, doc.width, doc.height, px);
    layer.object = Some(obj);
    Ok(insert_above(doc, above, layer))
}

fn object_mut(layers: &mut [Layer], id: LayerId) -> Result<&mut Layer, String> {
    let l = layers
        .iter_mut()
        .find(|l| l.id == id)
        .ok_or("Camada não encontrada.")?;
    if l.object.is_none() {
        return Err("A camada não é um objeto inteligente.".into());
    }
    Ok(l)
}

/// Embute o conteúdo vinculado no projeto.
pub fn embed(doc: &Document, id: LayerId) -> Result<Vec<Layer>, String> {
    let mut layers = doc.layers.clone();
    let l = object_mut(&mut layers, id)?;
    let o = l.object.as_mut().ok_or("Sem objeto.")?;
    if o.link.is_none() {
        return Err("O objeto já está embutido.".into());
    }
    if o.pixels.is_none() {
        return Err("O arquivo vinculado está ausente; reencontre-o antes de embutir.".into());
    }
    o.link = None;
    Ok(layers)
}

fn read_link(path: &str) -> Result<(u32, u32, Vec<u8>), String> {
    if !link_allowed(path) {
        return Err("Caminho de vínculo não permitido (rede/URL).".into());
    }
    let bytes = std::fs::read(Path::new(path))
        .map_err(|e| format!("Não foi possível ler \"{path}\": {e}"))?;
    let imp = imageio::import(&bytes, "vinculo")?;
    let l = &imp.document.layers[0];
    Ok((l.width, l.height, l.pixels.as_ref().clone()))
}

/// (Re)vincula o objeto a um arquivo (reencontrar ou atualizar) e re-renderiza.
pub fn relink(doc: &Document, id: LayerId, path: &str) -> Result<Vec<Layer>, String> {
    let (w, h, px) = read_link(path)?;
    let mut layers = doc.layers.clone();
    let l = object_mut(&mut layers, id)?;
    let o = l.object.as_mut().ok_or("Sem objeto.")?;
    o.orig_w = w;
    o.orig_h = h;
    o.pixels = Some(Arc::new(px));
    o.link = Some(path.to_string());
    let (dw, dh) = (doc.width, doc.height);
    l.rerender(dw, dh);
    Ok(layers)
}

/// Relê o arquivo vinculado de seu caminho atual.
pub fn update_link(doc: &Document, id: LayerId) -> Result<Vec<Layer>, String> {
    let path = doc
        .layer(id)
        .and_then(|l| l.object.as_ref())
        .and_then(|o| o.link.clone())
        .ok_or("O objeto não é vinculado.")?;
    relink(doc, id, &path)
}

/// Ao abrir um projeto: relê os arquivos dos objetos vinculados. Devolve avisos (arquivos
/// ausentes ou inválidos mantêm a aparência gravada, sem permitir re-renderizar).
pub fn resolve_links(doc: &mut Document) -> Vec<String> {
    let mut warnings = Vec::new();
    for l in &mut doc.layers {
        let Some(o) = l.object.as_mut() else { continue };
        let Some(path) = o.link.clone() else { continue };
        if o.pixels.is_some() {
            continue;
        }
        match read_link(&path) {
            Ok((w, h, px)) => {
                if (w, h) != (o.orig_w, o.orig_h) {
                    warnings.push(format!("O arquivo vinculado \"{path}\" mudou de tamanho; a camada \"{}\" foi atualizada.", l.name));
                }
                o.orig_w = w;
                o.orig_h = h;
                o.pixels = Some(Arc::new(px));
            }
            Err(e) => warnings.push(format!(
                "Objeto vinculado ausente na camada \"{}\": {e}. A aparência gravada foi mantida; use \"Reencontrar vínculo\" para voltar a editar.",
                l.name
            )),
        }
    }
    warnings
}

/// Define a máscara dos filtros a partir da seleção atual (ou remove-a se `from_selection` falso).
pub fn set_filter_mask(
    doc: &Document,
    id: LayerId,
    from_selection: bool,
) -> Result<Vec<Layer>, String> {
    let mut layers = doc.layers.clone();
    let l = object_mut(&mut layers, id)?;
    let o = l.object.as_mut().ok_or("Sem objeto.")?;
    if from_selection {
        let m = doc
            .selection
            .mask
            .as_ref()
            .ok_or("Faça uma seleção primeiro.")?;
        o.mask = Some(m.clone());
    } else {
        o.mask = None;
    }
    let (dw, dh) = (doc.width, doc.height);
    l.rerender(dw, dh);
    Ok(layers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_square() -> Document {
        let mut d = Document::new("o", 40, 30, Some([255, 255, 255, 255])).unwrap();
        let mut l = Layer::transparent("img", 40, 30);
        for y in 8..20 {
            for x in 10..26 {
                l.set_pixel(x, y, [(x * 8) as u8, (y * 8) as u8, 90, 255]);
            }
        }
        d.layers.push(l);
        d
    }

    fn id1(d: &Document) -> LayerId {
        d.layers[1].id
    }

    #[test]
    fn convert_keeps_the_exact_appearance_and_crops_to_content() {
        let d = doc_with_square();
        let before = d.layers[1].pixels.clone();
        let layers = convert_to_object(&d, id1(&d)).unwrap();
        let o = layers[1].object.as_ref().unwrap();
        assert_eq!((o.orig_w, o.orig_h), (16, 12));
        assert_eq!(
            o.render(40, 30).unwrap(),
            *before,
            "render idêntico ao original"
        );
        assert!(convert_to_object(&Document::new("v", 4, 4, None).unwrap(), LayerId(1)).is_err());
    }

    #[test]
    fn transform_is_reversible_without_loss() {
        let d = doc_with_square();
        let mut layers = convert_to_object(&d, id1(&d)).unwrap();
        let original = layers[1].pixels.clone();
        {
            let l = &mut layers[1];
            let o = l.object.as_mut().unwrap();
            o.sx = 0.25;
            o.sy = 0.25;
            o.rot = 33.0;
            l.rerender(40, 30);
        }
        assert_ne!(layers[1].pixels, original);
        {
            let l = &mut layers[1];
            l.object.as_mut().unwrap().reset_transform();
            l.rerender(40, 30);
        }
        assert_eq!(
            layers[1].pixels, original,
            "voltar à escala 1 reconstrói o original exato"
        );
    }

    #[test]
    fn scale_and_rotation_move_content_as_expected() {
        let d = doc_with_square();
        let mut layers = convert_to_object(&d, id1(&d)).unwrap();
        let o = layers[1].object.as_mut().unwrap();
        o.sx = 2.0;
        o.sy = 2.0;
        let big = o.render(40, 30).unwrap();
        let area = |b: &[u8]| b.as_chunks::<4>().0.iter().filter(|p| p[3] > 128).count();
        let o2 = layers[1].object.as_ref().unwrap();
        assert!(
            area(&big) > area(&o2.clone().render_with(1.0)) * 3,
            "dobrar a escala ~ quadruplica a área"
        );
        // Rotação de 90° troca largura e altura (16×12 → 12×16).
        let mut r = layers[1].object.clone().unwrap();
        r.sx = 1.0;
        r.sy = 1.0;
        r.rot = 90.0;
        let px = r.render(40, 30).unwrap();
        let cols: Vec<usize> = (0..40)
            .filter(|x| (0..30).any(|y| px[(y * 40 + x) * 4 + 3] > 200))
            .collect();
        let rows: Vec<usize> = (0..30)
            .filter(|y| (0..40).any(|x| px[(y * 40 + x) * 4 + 3] > 200))
            .collect();
        assert_eq!((cols.len(), rows.len()), (12, 16));
    }

    #[test]
    fn translation_shifts_without_resampling() {
        let d = doc_with_square();
        let mut layers = convert_to_object(&d, id1(&d)).unwrap();
        let l = &mut layers[1];
        let original = l.pixels.clone();
        l.object.as_mut().unwrap().translate(5.0, 3.0);
        l.rerender(40, 30);
        assert_eq!(
            l.pixel(15, 11),
            Some([(10 * 8) as u8, (8 * 8) as u8, 90, 255])
        );
        assert_ne!(l.pixels, original);
    }

    #[test]
    fn smart_filters_are_editable_toggleable_and_masked() {
        let d = doc_with_square();
        let mut layers = convert_to_object(&d, id1(&d)).unwrap();
        let plain = layers[1].pixels.clone();
        {
            let l = &mut layers[1];
            l.object.as_mut().unwrap().filters.push(SmartFilter {
                enabled: true,
                filter: Filter::Pixelate { size: 6 },
            });
            l.rerender(40, 30);
        }
        let filtered = layers[1].pixels.clone();
        assert_ne!(filtered, plain);
        // Parâmetros editáveis: outro tamanho gera outro resultado.
        if let Filter::Pixelate { size } = &mut layers[1].object.as_mut().unwrap().filters[0].filter
        {
            *size = 3;
        }
        layers[1].rerender(40, 30);
        assert_ne!(layers[1].pixels, filtered);
        // Desligar o filtro restaura o original exato.
        layers[1].object.as_mut().unwrap().filters[0].enabled = false;
        layers[1].rerender(40, 30);
        assert_eq!(layers[1].pixels, plain);
        // Máscara própria: filtro só na metade esquerda.
        layers[1].object.as_mut().unwrap().filters[0].enabled = true;
        let mut m = vec![0u8; 40 * 30];
        for y in 0..30 {
            for x in 0..18 {
                m[y * 40 + x] = 255;
            }
        }
        layers[1].object.as_mut().unwrap().mask = Some(Arc::new(m));
        layers[1].rerender(40, 30);
        let px = |l: &Layer, x: u32, y: u32| l.pixel(x, y).unwrap();
        assert_eq!(
            px(&layers[1], 22, 12),
            [(22 * 8) as u8, (12 * 8) as u8, 90, 255],
            "fora da máscara = original"
        );
        assert_ne!(
            px(&layers[1], 10, 8),
            [(10 * 8) as u8, (8 * 8) as u8, 90, 255],
            "dentro da máscara = filtrado"
        );
    }

    fn temp_png(name: &str, c: [u8; 4]) -> String {
        let dir =
            std::env::temp_dir().join(format!("orvalith_obj_{}_{}", std::process::id(), name));
        let doc = Document::new("l", 10, 6, Some(c)).unwrap();
        std::fs::write(&dir, imageio::export_png(&doc).unwrap()).unwrap();
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn linked_objects_detect_missing_files_update_and_embed() {
        let d = Document::new("t", 40, 30, Some([255, 255, 255, 255])).unwrap();
        let path = temp_png("a.png", [200, 0, 0, 255]);
        let bytes = std::fs::read(&path).unwrap();
        let out = place_image(&d, &bytes, "ref", Some(path.clone()), None).unwrap();
        let mut doc = d.clone();
        doc.layers = out.layers;
        let id = doc.layers[1].id;
        assert!(doc.layers[1].object.as_ref().unwrap().is_linked());
        assert_eq!(
            doc.layers[1].pixel(20, 15),
            Some([200, 0, 0, 255]),
            "centralizado"
        );
        // O arquivo muda: atualizar relê.
        let bytes2 =
            imageio::export_png(&Document::new("l", 10, 6, Some([0, 0, 200, 255])).unwrap())
                .unwrap();
        std::fs::write(&path, bytes2).unwrap();
        doc.layers = update_link(&doc, id).unwrap();
        assert_eq!(doc.layers[1].pixel(20, 15), Some([0, 0, 200, 255]));
        // Reabrir com o arquivo ausente: aviso, aparência mantida e sem re-renderizar.
        let mut reopened = doc.clone();
        reopened.layers[1].object.as_mut().unwrap().pixels = None;
        std::fs::remove_file(&path).unwrap();
        let warnings = resolve_links(&mut reopened);
        assert_eq!(warnings.len(), 1);
        let o = reopened.layers[1].object.as_ref().unwrap();
        assert!(o.is_missing() && o.render(40, 30).is_none());
        assert_eq!(reopened.layers[1].pixel(20, 15), Some([0, 0, 200, 255]));
        assert!(
            embed(&reopened, id).is_err(),
            "sem original não dá para embutir"
        );
        // Com o original em memória, embutir remove o vínculo.
        let e = embed(&doc, id).unwrap();
        assert!(!e[1].object.as_ref().unwrap().is_linked());
        assert!(embed(
            &Document {
                layers: e,
                ..doc.clone()
            },
            id
        )
        .is_err());
    }

    #[test]
    fn network_and_url_paths_are_refused() {
        for p in [
            "\\\\servidor\\pasta\\a.png",
            "//host/a.png",
            "http://x/a.png",
            "file:///etc/x",
            "",
            "a\0b",
        ] {
            assert!(!link_allowed(p), "{p:?}");
        }
        assert!(link_allowed("C:\\fotos\\a.png") && link_allowed("/home/u/a.png"));
        let d = Document::new("t", 4, 4, None).unwrap();
        let png = imageio::export_png(&d).unwrap();
        assert!(place_image(&d, &png, "x", Some("\\\\srv\\a.png".into()), None).is_err());
    }

    #[test]
    fn document_operations_keep_or_drop_objects_as_documented() {
        use crate::docops;
        use crate::transform::Interp;
        let d = doc_with_square();
        let mut doc = d.clone();
        doc.layers = convert_to_object(&d, id1(&d)).unwrap();
        // Recortar o canvas desloca o objeto sem perda e ele continua editável.
        let st = docops::crop(&doc, 5, 4, 30, 22).unwrap();
        let o = st.layers[1].object.as_ref().expect("objeto mantido");
        assert_eq!((o.cx, o.cy), (18.0 - 5.0 + 0.0, 14.0 - 4.0 + 0.0));
        assert_eq!(
            st.layers[1].pixel(10 - 5 + 2, 8 - 4 + 2),
            doc.layers[1].pixel(12, 10)
        );
        // Redimensionar a imagem reamostra os pixels: o objeto vira raster.
        let rs = docops::resize_image(&doc, 20, 15, Interp::Bilinear, None).unwrap();
        assert!(rs.layers[1].object.is_none());
        // Rasterizar descarta o objeto e mantém os pixels.
        let r = crate::vecops::rasterize(&doc, id1(&doc)).unwrap();
        assert!(r[1].object.is_none());
        assert_eq!(r[1].pixels, doc.layers[1].pixels);
        assert!(crate::vecops::rasterize(
            &Document {
                layers: r,
                ..doc.clone()
            },
            id1(&doc)
        )
        .is_err());
    }

    #[test]
    fn hostile_parameters_are_sanitized() {
        let mut o = SmartObject::new(2, 2, vec![255; 16], f32::NAN, f32::INFINITY);
        o.sx = 0.0;
        o.sy = f32::NAN;
        o.rot = 1e9;
        o.sanitize();
        assert!(o.cx == 0.0 && o.sx.abs() >= 0.01 && o.sy == 1.0 && o.rot <= 3600.0);
        assert!(o.render(4, 4).is_some());
    }

    trait RenderWith {
        fn render_with(self, s: f32) -> Vec<u8>;
    }

    impl RenderWith for SmartObject {
        /// Auxiliar de teste: renderiza com a escala indicada.
        fn render_with(mut self, s: f32) -> Vec<u8> {
            self.sx = s;
            self.sy = s;
            self.render(40, 30).unwrap()
        }
    }
}
