//! Geometria do documento: recortar, tamanho da tela/imagem, girar/espelhar, endireitar,
//! alinhar e distribuir. Funções puras: devolvem o novo estado, aplicado por `DocEdit`.

use crate::document::{Channel, Document, Guide, Layer, LayerId, Selection};
use crate::geom::Rect;
use crate::history::Command;
use crate::layerops::{movable_rasters, shifted_pixels};
use crate::transform::{self, Affine, Interp};

/// Limite de memória de pixels de camadas após uma operação geométrica.
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Estado do documento afetado por operações geométricas.
#[derive(Clone)]
pub struct DocState {
    pub width: u32,
    pub height: u32,
    pub ppi: f32,
    pub layers: Vec<Layer>,
    pub selection: Selection,
    pub channels: Vec<Channel>,
    pub guides: Vec<Guide>,
}

impl DocState {
    pub fn of(doc: &Document) -> Self {
        Self {
            width: doc.width,
            height: doc.height,
            ppi: doc.ppi,
            layers: doc.layers.clone(),
            selection: doc.selection.clone(),
            channels: doc.channels.clone(),
            guides: doc.guides.clone(),
        }
    }

    pub fn apply_to(&self, doc: &mut Document) {
        doc.width = self.width;
        doc.height = self.height;
        doc.ppi = self.ppi;
        doc.layers = self.layers.clone();
        doc.selection = self.selection.clone();
        doc.channels = self.channels.clone();
        doc.guides = self.guides.clone();
    }
}

/// Comando de histórico que troca o estado geométrico inteiro (recortar, redimensionar,
/// transformar camadas…). Os pixels são compartilhados por `Arc`.
pub struct DocEdit {
    label: String,
    before: DocState,
    after: DocState,
}

impl DocEdit {
    pub fn new(label: &str, before: DocState, after: DocState) -> Self {
        Self {
            label: label.to_string(),
            before,
            after,
        }
    }
}

impl Command for DocEdit {
    fn label(&self) -> &str {
        &self.label
    }
    fn apply(&mut self, doc: &mut Document) {
        self.after.apply_to(doc);
    }
    fn revert(&mut self, doc: &mut Document) {
        self.before.apply_to(doc);
    }
    fn bytes(&self) -> usize {
        let unique = |a: &DocState, b: &DocState| -> usize {
            a.layers
                .iter()
                .filter(|l| {
                    !b.layers
                        .iter()
                        .any(|o| std::sync::Arc::ptr_eq(&o.pixels, &l.pixels))
                })
                .map(|l| l.pixels.len())
                .sum::<usize>()
                + a.selection.mask.as_ref().map_or(0, |m| m.len())
        };
        unique(&self.after, &self.before) + unique(&self.before, &self.after)
    }
}

fn check_size(doc: &Document, nw: u32, nh: u32) -> Result<(), String> {
    if nw == 0 || nh == 0 || nw > Document::MAX_SIDE || nh > Document::MAX_SIDE {
        return Err(format!(
            "Dimensões inválidas: {nw}×{nh} (máximo {} por lado)",
            Document::MAX_SIDE
        ));
    }
    let rasters = doc.layers.iter().filter(|l| !l.is_group()).count() as u64;
    if u64::from(nw) * u64::from(nh) * 4 * rasters.max(1) > MAX_BYTES {
        return Err("O novo tamanho exigiria memória demais para as camadas.".into());
    }
    Ok(())
}

/// Copia `src` (sw×sh, `bpp` bytes por pixel) para um novo buffer nw×nh com a origem antiga
/// em (ox, oy); o resto é preenchido com `fill`.
#[allow(clippy::too_many_arguments)]
fn reframe_buf(
    src: &[u8],
    sw: u32,
    sh: u32,
    nw: u32,
    nh: u32,
    ox: i32,
    oy: i32,
    bpp: usize,
    fill: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(nw as usize * nh as usize * bpp);
    for _ in 0..nw as usize * nh as usize {
        out.extend_from_slice(fill);
    }
    let x0 = i64::from(ox).max(0);
    let x1 = (i64::from(ox) + i64::from(sw)).min(i64::from(nw));
    if x0 >= x1 {
        return out;
    }
    for y in 0..i64::from(nh) {
        let sy = y - i64::from(oy);
        if sy < 0 || sy >= i64::from(sh) {
            continue;
        }
        let len = (x1 - x0) as usize * bpp;
        let s = (sy as usize * sw as usize + (x0 - i64::from(ox)) as usize) * bpp;
        let d = (y as usize * nw as usize + x0 as usize) * bpp;
        out[d..d + len].copy_from_slice(&src[s..s + len]);
    }
    out
}

/// Nova tela `nw × nh` com a origem antiga em (ox, oy) (pode ser negativa: recorte).
/// `fill_bottom`: cor para preencher a área nova da camada raster mais baixa.
pub fn reframe(
    doc: &Document,
    nw: u32,
    nh: u32,
    ox: i32,
    oy: i32,
    fill_bottom: Option<[u8; 4]>,
) -> Result<DocState, String> {
    check_size(doc, nw, nh)?;
    let bottom = doc.layers.iter().position(|l| !l.is_group());
    let mut st = DocState::of(doc);
    st.width = nw;
    st.height = nh;
    for (i, l) in st.layers.iter_mut().enumerate() {
        if l.is_group() {
            continue;
        }
        if l.source.is_some() {
            crate::vecops::translate_source(l, ox as f32, oy as f32, nw, nh);
            if l.source.is_some() {
                continue;
            }
        }
        let fill = if Some(i) == bottom {
            fill_bottom.unwrap_or([0; 4])
        } else {
            [0; 4]
        };
        let px = reframe_buf(&l.pixels, l.width, l.height, nw, nh, ox, oy, 4, &fill);
        l.pixels = std::sync::Arc::new(px);
        l.width = nw;
        l.height = nh;
    }
    for l in &mut st.layers {
        if let Some(m) = l.mask.as_mut() {
            let nd = reframe_buf(m.data(), doc.width, doc.height, nw, nh, ox, oy, 1, &[255]);
            m.set_data(std::sync::Arc::new(nd));
        }
    }
    let remap = |m: &std::sync::Arc<Vec<u8>>| {
        std::sync::Arc::new(reframe_buf(
            m,
            doc.width,
            doc.height,
            nw,
            nh,
            ox,
            oy,
            1,
            &[0],
        ))
    };
    st.selection = Selection {
        mask: st
            .selection
            .mask
            .as_ref()
            .map(remap)
            .filter(|m| m.iter().any(|&v| v > 0)),
    };
    for c in &mut st.channels {
        c.mask = remap(&c.mask);
    }
    st.guides = st
        .guides
        .iter()
        .map(|g| Guide {
            vertical: g.vertical,
            pos: g.pos + if g.vertical { ox as f32 } else { oy as f32 },
        })
        .filter(|g| g.pos >= 0.0 && g.pos <= if g.vertical { nw as f32 } else { nh as f32 })
        .collect();
    Ok(st)
}

/// Recorta para o retângulo (x, y, w, h); pode passar das bordas (a área nova fica transparente).
pub fn crop(doc: &Document, x: i32, y: i32, w: u32, h: u32) -> Result<DocState, String> {
    reframe(doc, w, h, -x, -y, None)
}

/// Tamanho da tela com âncora (ax, ay ∈ {0, 0,5, 1}): 0 = esquerda/topo, 1 = direita/base.
pub fn canvas_size(
    doc: &Document,
    nw: u32,
    nh: u32,
    anchor: (f32, f32),
    fill_bottom: Option<[u8; 4]>,
) -> Result<DocState, String> {
    let ox = ((nw as f32 - doc.width as f32) * anchor.0).round() as i32;
    let oy = ((nh as f32 - doc.height as f32) * anchor.1).round() as i32;
    reframe(doc, nw, nh, ox, oy, fill_bottom)
}

/// Redimensiona a imagem inteira (camadas, seleção e canais) com reamostragem.
/// `new_ppi`: se informado, atualiza a resolução (para manter o tamanho físico).
pub fn resize_image(
    doc: &Document,
    nw: u32,
    nh: u32,
    interp: Interp,
    new_ppi: Option<f32>,
) -> Result<DocState, String> {
    check_size(doc, nw, nh)?;
    let mut st = DocState::of(doc);
    st.width = nw;
    st.height = nh;
    if let Some(p) = new_ppi.filter(|p| p.is_finite() && *p > 0.0) {
        st.ppi = p;
    }
    for l in st.layers.iter_mut().filter(|l| !l.is_group()) {
        l.source = None; // reamostrado: a origem vetorial não vale mais
        let px = transform::resample(&l.pixels, l.width, l.height, nw, nh, interp);
        l.pixels = std::sync::Arc::new(px);
        l.width = nw;
        l.height = nh;
    }
    for l in &mut st.layers {
        if let Some(m) = l.mask.as_mut() {
            let nd = transform::resample_mask(m.data(), doc.width, doc.height, nw, nh);
            m.set_data(std::sync::Arc::new(nd));
        }
    }
    let rs = |m: &std::sync::Arc<Vec<u8>>| {
        std::sync::Arc::new(transform::resample_mask(m, doc.width, doc.height, nw, nh))
    };
    st.selection = Selection {
        mask: st
            .selection
            .mask
            .as_ref()
            .map(rs)
            .filter(|m| m.iter().any(|&v| v > 0)),
    };
    for c in &mut st.channels {
        c.mask = rs(&c.mask);
    }
    let (sx, sy) = (nw as f32 / doc.width as f32, nh as f32 / doc.height as f32);
    for g in &mut st.guides {
        g.pos *= if g.vertical { sx } else { sy };
    }
    Ok(st)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orient {
    Cw90,
    Ccw90,
    Rot180,
    FlipH,
    FlipV,
}

/// Novo (x, y) do pixel (x, y) de uma imagem w×h.
fn orient_pixel(o: Orient, x: u32, y: u32, w: u32, h: u32) -> (u32, u32) {
    match o {
        Orient::Cw90 => (h - 1 - y, x),
        Orient::Ccw90 => (y, w - 1 - x),
        Orient::Rot180 => (w - 1 - x, h - 1 - y),
        Orient::FlipH => (w - 1 - x, y),
        Orient::FlipV => (x, h - 1 - y),
    }
}

fn orient_buf(src: &[u8], w: u32, h: u32, bpp: usize, o: Orient) -> Vec<u8> {
    let (nw, _nh) = match o {
        Orient::Cw90 | Orient::Ccw90 => (h, w),
        _ => (w, h),
    };
    let mut out = vec![0u8; src.len()];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = orient_pixel(o, x, y, w, h);
            let s = (y as usize * w as usize + x as usize) * bpp;
            let d = (ny as usize * nw as usize + nx as usize) * bpp;
            out[d..d + bpp].copy_from_slice(&src[s..s + bpp]);
        }
    }
    out
}

/// Gira ou espelha a tela inteira (exato, sem reamostragem).
pub fn orient_canvas(doc: &Document, o: Orient) -> Result<DocState, String> {
    let (w, h) = (doc.width, doc.height);
    let (nw, nh) = match o {
        Orient::Cw90 | Orient::Ccw90 => (h, w),
        _ => (w, h),
    };
    check_size(doc, nw, nh)?;
    let mut st = DocState::of(doc);
    st.width = nw;
    st.height = nh;
    for l in st.layers.iter_mut().filter(|l| !l.is_group()) {
        l.source = None;
        l.pixels = std::sync::Arc::new(orient_buf(&l.pixels, l.width, l.height, 4, o));
        l.width = nw;
        l.height = nh;
    }
    for l in &mut st.layers {
        if let Some(m) = l.mask.as_mut() {
            m.set_data(std::sync::Arc::new(orient_buf(m.data(), w, h, 1, o)));
        }
    }
    let om = |m: &std::sync::Arc<Vec<u8>>| std::sync::Arc::new(orient_buf(m, w, h, 1, o));
    st.selection.mask = st.selection.mask.as_ref().map(om);
    for c in &mut st.channels {
        c.mask = om(&c.mask);
    }
    let (wf, hf) = (w as f32, h as f32);
    st.guides = doc
        .guides
        .iter()
        .map(|g| {
            let c = g.pos;
            match (o, g.vertical) {
                (Orient::Cw90, true) => Guide {
                    vertical: false,
                    pos: c,
                },
                (Orient::Cw90, false) => Guide {
                    vertical: true,
                    pos: hf - c,
                },
                (Orient::Ccw90, true) => Guide {
                    vertical: false,
                    pos: wf - c,
                },
                (Orient::Ccw90, false) => Guide {
                    vertical: true,
                    pos: c,
                },
                (Orient::Rot180, true) | (Orient::FlipH, true) => Guide {
                    vertical: true,
                    pos: wf - c,
                },
                (Orient::Rot180, false) | (Orient::FlipV, false) => Guide {
                    vertical: false,
                    pos: hf - c,
                },
                (Orient::FlipH, false) => *g,
                (Orient::FlipV, true) => *g,
            }
        })
        .collect();
    Ok(st)
}

/// Maior retângulo alinhado aos eixos, centralizado, inscrito num retângulo `w×h` girado.
pub fn largest_rotated_rect(w: f32, h: f32, angle_deg: f32) -> (f32, f32) {
    if w <= 0.0 || h <= 0.0 {
        return (0.0, 0.0);
    }
    let a = angle_deg.to_radians();
    let (sin_a, cos_a) = (a.sin().abs(), a.cos().abs());
    if sin_a < 1e-9 {
        return (w, h);
    }
    let width_is_longer = w >= h;
    let (long, short) = if width_is_longer { (w, h) } else { (h, w) };
    if short <= 2.0 * sin_a * cos_a * long || (sin_a - cos_a).abs() < 1e-9 {
        let x = 0.5 * short;
        if width_is_longer {
            (x / sin_a, x / cos_a)
        } else {
            (x / cos_a, x / sin_a)
        }
    } else {
        let cos_2a = cos_a * cos_a - sin_a * sin_a;
        (
            (w * cos_a - h * sin_a) / cos_2a,
            (h * cos_a - w * sin_a) / cos_2a,
        )
    }
}

/// Endireitar: gira todo o conteúdo `angle_deg` (horário) em torno do centro e recorta ao maior
/// retângulo inscrito, para não restarem cantos vazios.
pub fn straighten(doc: &Document, angle_deg: f32, interp: Interp) -> Result<DocState, String> {
    let (iw, ih) = largest_rotated_rect(doc.width as f32, doc.height as f32, angle_deg);
    let (nw, nh) = ((iw.floor() as u32).max(1), (ih.floor() as u32).max(1));
    check_size(doc, nw, nh)?;
    let c_old = (doc.width as f32 / 2.0, doc.height as f32 / 2.0);
    let c_new = (nw as f32 / 2.0, nh as f32 / 2.0);
    let m = Affine::translate(-c_old.0, -c_old.1)
        .then(Affine::rotate(angle_deg, (0.0, 0.0)))
        .then(Affine::translate(c_new.0, c_new.1));
    let mut st = DocState::of(doc);
    st.width = nw;
    st.height = nh;
    for l in st.layers.iter_mut().filter(|l| !l.is_group()) {
        l.source = None;
        let px = transform::warp_affine(&l.pixels, l.width, l.height, &m, nw, nh, interp)
            .ok_or("Ângulo inválido.")?;
        l.pixels = std::sync::Arc::new(px);
        l.width = nw;
        l.height = nh;
    }
    let warp_mask = |mask: &std::sync::Arc<Vec<u8>>| {
        let rgba: Vec<u8> = mask.iter().flat_map(|&v| [v, v, v, 255]).collect();
        transform::warp_affine(&rgba, doc.width, doc.height, &m, nw, nh, Interp::Bilinear)
            .map(|o| {
                std::sync::Arc::new(
                    o.as_chunks::<4>()
                        .0
                        .iter()
                        .map(|p| p[0])
                        .collect::<Vec<u8>>(),
                )
            })
            .unwrap_or_else(|| std::sync::Arc::new(vec![0; nw as usize * nh as usize]))
    };
    for l in &mut st.layers {
        if let Some(m) = l.mask.as_mut() {
            let nd = warp_mask(m.data());
            m.set_data(nd);
        }
    }
    st.selection.mask = st
        .selection
        .mask
        .as_ref()
        .map(warp_mask)
        .filter(|m| m.iter().any(|&v| v > 0));
    for c in &mut st.channels {
        c.mask = warp_mask(&c.mask);
    }
    st.guides.clear(); // guias não acompanham a rotação
    Ok(st)
}

/// Retângulo envolvente dos pixels com alfa > 0 de uma camada raster.
pub fn content_bounds(layer: &Layer) -> Option<Rect> {
    if layer.is_group() {
        return None;
    }
    let (w, h) = (layer.width as usize, layer.height as usize);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    for y in 0..h {
        let row = &layer.pixels[y * w * 4..(y + 1) * w * 4];
        let first = row.as_chunks::<4>().0.iter().position(|p| p[3] > 0);
        if let Some(f) = first {
            let last = row
                .as_chunks::<4>()
                .0
                .iter()
                .rposition(|p| p[3] > 0)
                .unwrap_or(f);
            x0 = x0.min(f);
            x1 = x1.max(last + 1);
            y0 = y0.min(y);
            y1 = y1.max(y + 1);
        }
    }
    (x1 > x0).then(|| Rect::new(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignKind {
    Left,
    HCenter,
    Right,
    Top,
    VCenter,
    Bottom,
}

/// Retângulo do conteúdo de um nó (camada ou grupo com descendentes).
fn node_bounds(doc: &Document, id: LayerId) -> Option<Rect> {
    movable_rasters(doc, &[id])
        .into_iter()
        .filter_map(|r| doc.layer(r).and_then(content_bounds))
        .reduce(Rect::union)
}

/// Desloca os pixels de um nó; devolve `true` se alguma camada se moveu (não bloqueada).
fn shift_node(layers: &mut [Layer], doc: &Document, id: LayerId, dx: i32, dy: i32) -> bool {
    if dx == 0 && dy == 0 {
        return false;
    }
    let mut moved = false;
    for r in movable_rasters(doc, &[id]) {
        if let Some(l) = layers.iter_mut().find(|l| l.id == r) {
            if !l.lock_position {
                if l.source.is_some() {
                    let (w, h) = (l.width, l.height);
                    crate::vecops::translate_source(l, dx as f32, dy as f32, w, h);
                } else {
                    l.pixels = std::sync::Arc::new(shifted_pixels(l, dx, dy));
                }
                if let Some(m) = l.mask.as_mut().filter(|m| m.linked) {
                    let nd = crate::selection::shift_mask(m.data(), l.width, l.height, dx, dy, 255);
                    m.set_data(std::sync::Arc::new(nd));
                }
                moved = true;
            }
        }
    }
    moved
}

/// Alinha os nós `ids` ao retângulo `reference` (tela ou seleção). Nós bloqueados não se movem.
pub fn align(
    doc: &Document,
    ids: &[LayerId],
    kind: AlignKind,
    reference: Rect,
) -> Option<Vec<Layer>> {
    let mut layers = doc.layers.clone();
    let mut moved = false;
    for &id in ids {
        let Some(b) = node_bounds(doc, id) else {
            continue;
        };
        let (dx, dy) = match kind {
            AlignKind::Left => (reference.x as i32 - b.x as i32, 0),
            AlignKind::Right => (reference.right() as i32 - b.right() as i32, 0),
            AlignKind::HCenter => (
                (reference.x as i32 * 2 + reference.w as i32) / 2
                    - (b.x as i32 * 2 + b.w as i32) / 2,
                0,
            ),
            AlignKind::Top => (0, reference.y as i32 - b.y as i32),
            AlignKind::Bottom => (0, reference.bottom() as i32 - b.bottom() as i32),
            AlignKind::VCenter => (
                0,
                (reference.y as i32 * 2 + reference.h as i32) / 2
                    - (b.y as i32 * 2 + b.h as i32) / 2,
            ),
        };
        moved |= shift_node(&mut layers, doc, id, dx, dy);
    }
    moved.then_some(layers)
}

/// Distribui espaços iguais entre 3 ou mais nós ao longo de um eixo (o primeiro e o último
/// ficam onde estão).
pub fn distribute(doc: &Document, ids: &[LayerId], horizontal: bool) -> Option<Vec<Layer>> {
    let mut items: Vec<(LayerId, Rect)> = ids
        .iter()
        .filter_map(|&id| node_bounds(doc, id).map(|b| (id, b)))
        .collect();
    if items.len() < 3 {
        return None;
    }
    let pos = |r: &Rect| if horizontal { r.x } else { r.y };
    let len = |r: &Rect| if horizontal { r.w } else { r.h };
    items.sort_by_key(|(_, r)| pos(r) + len(r) / 2);
    let first = items[0].1;
    let last = items[items.len() - 1].1;
    let span = i64::from(pos(&last) + len(&last)) - i64::from(pos(&first));
    let total: i64 = items.iter().map(|(_, r)| i64::from(len(r))).sum();
    let gap = (span - total) as f64 / (items.len() - 1) as f64;
    let mut layers = doc.layers.clone();
    let mut cursor = f64::from(pos(&first));
    let mut moved = false;
    for (id, r) in &items {
        let target = cursor.round() as i64;
        let delta = (target - i64::from(pos(r))) as i32;
        if delta != 0 {
            let (dx, dy) = if horizontal { (delta, 0) } else { (0, delta) };
            moved |= shift_node(&mut layers, doc, *id, dx, dy);
        }
        cursor += f64::from(len(r)) + gap;
    }
    moved.then_some(layers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_marks() -> Document {
        let mut d = Document::new("t", 8, 6, None).unwrap();
        d.layers[0].set_pixel(1, 2, [255, 0, 0, 255]);
        d.layers[0].set_pixel(6, 4, [0, 255, 0, 255]);
        d
    }

    fn apply(d: &mut Document, st: DocState) {
        st.apply_to(d);
    }

    #[test]
    fn crop_inside_and_beyond_canvas() {
        let d = doc_with_marks();
        let st = crop(&d, 1, 2, 3, 2).unwrap();
        assert_eq!((st.width, st.height), (3, 2));
        let mut d2 = d.clone();
        apply(&mut d2, st);
        assert_eq!(d2.layers[0].pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(d2.layers[0].width, 3);
        // Recorte além das bordas: área nova transparente e conteúdo deslocado.
        let st = crop(&d, -2, -1, 12, 9).unwrap();
        let mut d3 = d.clone();
        apply(&mut d3, st);
        assert_eq!((d3.width, d3.height), (12, 9));
        assert_eq!(d3.layers[0].pixel(3, 3), Some([255, 0, 0, 255]));
        assert_eq!(d3.layers[0].pixel(0, 0).unwrap()[3], 0);
        assert!(crop(&d, 0, 0, 0, 5).is_err());
        assert!(crop(&d, 0, 0, 40_000, 5).is_err());
    }

    #[test]
    fn canvas_size_anchors_and_background_fill() {
        let d = doc_with_marks();
        let st = canvas_size(&d, 10, 8, (1.0, 1.0), None).unwrap();
        let mut d2 = d.clone();
        apply(&mut d2, st);
        // Âncora inferior direita: conteúdo vai para (+2, +2).
        assert_eq!(d2.layers[0].pixel(3, 4), Some([255, 0, 0, 255]));
        let st = canvas_size(&d, 10, 8, (0.5, 0.5), Some([9, 9, 9, 255])).unwrap();
        let mut d3 = d.clone();
        apply(&mut d3, st);
        assert_eq!(d3.layers[0].pixel(0, 0), Some([9, 9, 9, 255]));
        assert_eq!(d3.layers[0].pixel(2, 3), Some([255, 0, 0, 255]));
        // Reduzir com âncora no centro corta igual dos dois lados.
        let st = canvas_size(&d, 4, 2, (0.5, 0.5), None).unwrap();
        assert_eq!((st.width, st.height), (4, 2));
    }

    #[test]
    fn selection_channels_and_guides_follow_reframe() {
        let mut d = doc_with_marks();
        d.selection = Selection::from_mask(Some(crate::selection::rect_mask(
            8,
            6,
            (1.0, 1.0),
            (3.0, 3.0),
            true,
        )));
        d.channels.push(Channel {
            id: 1,
            name: "c".into(),
            mask: std::sync::Arc::new(vec![255; 48]),
        });
        d.guides = vec![
            Guide {
                vertical: true,
                pos: 4.0,
            },
            Guide {
                vertical: false,
                pos: 1.0,
            },
        ];
        let st = crop(&d, 2, 1, 4, 4).unwrap();
        assert_eq!(st.channels[0].mask.len(), 16);
        assert_eq!(st.selection.mask.as_ref().unwrap().len(), 16);
        // Guia vertical 4 -> 2; horizontal 1 -> 0.
        assert_eq!(
            st.guides,
            vec![
                Guide {
                    vertical: true,
                    pos: 2.0
                },
                Guide {
                    vertical: false,
                    pos: 0.0
                }
            ]
        );
        // Recorte que exclui a seleção a esvazia.
        let st = crop(&d, 5, 4, 3, 2).unwrap();
        assert!(st.selection.mask.is_none());
    }

    #[test]
    fn resize_image_scales_layers_masks_and_keeps_ppi_when_asked() {
        let mut d = Document::new("t", 8, 6, Some([10, 20, 30, 255])).unwrap();
        d.guides = vec![Guide {
            vertical: true,
            pos: 4.0,
        }];
        d.selection = Selection::from_mask(Some(vec![255; 48]));
        let st = resize_image(&d, 16, 12, Interp::Bicubic, Some(144.0)).unwrap();
        assert_eq!((st.width, st.height, st.ppi), (16, 12, 144.0));
        assert_eq!(st.layers[0].pixels.len(), 16 * 12 * 4);
        assert_eq!(st.layers[0].pixel(5, 5), Some([10, 20, 30, 255]));
        assert_eq!(st.selection.mask.as_ref().unwrap().len(), 192);
        assert_eq!(st.guides[0].pos, 8.0);
        let st = resize_image(&d, 4, 3, Interp::Bilinear, None).unwrap();
        assert_eq!(st.ppi, 72.0);
        assert!(resize_image(&d, 0, 3, Interp::Bilinear, None).is_err());
    }

    #[test]
    fn rotate_and_flip_canvas_are_exact_and_invertible() {
        let d = doc_with_marks();
        let mut d1 = d.clone();
        apply(&mut d1, orient_canvas(&d, Orient::Cw90).unwrap());
        assert_eq!((d1.width, d1.height), (6, 8));
        // (1,2) em 8×6 vai para (h-1-y, x) = (3, 1).
        assert_eq!(d1.layers[0].pixel(3, 1), Some([255, 0, 0, 255]));
        let mut back = d1.clone();
        apply(&mut back, orient_canvas(&d1, Orient::Ccw90).unwrap());
        assert_eq!(back.layers[0].pixels, d.layers[0].pixels);
        for o in [Orient::Rot180, Orient::FlipH, Orient::FlipV] {
            let mut x = d.clone();
            apply(&mut x, orient_canvas(&d, o).unwrap());
            let mut again = x.clone();
            apply(&mut again, orient_canvas(&x, o).unwrap());
            assert_eq!(again.layers[0].pixels, d.layers[0].pixels, "{o:?}");
        }
        let mut fh = d.clone();
        apply(&mut fh, orient_canvas(&d, Orient::FlipH).unwrap());
        assert_eq!(fh.layers[0].pixel(6, 2), Some([255, 0, 0, 255]));
        // Quatro giros horários voltam ao original.
        let mut r = d.clone();
        for _ in 0..4 {
            let st = orient_canvas(&r, Orient::Cw90).unwrap();
            apply(&mut r, st);
        }
        assert_eq!(r.layers[0].pixels, d.layers[0].pixels);
    }

    #[test]
    fn guides_follow_orientation() {
        let mut d = Document::new("t", 8, 6, None).unwrap();
        d.guides = vec![
            Guide {
                vertical: true,
                pos: 2.0,
            },
            Guide {
                vertical: false,
                pos: 1.0,
            },
        ];
        let st = orient_canvas(&d, Orient::Cw90).unwrap();
        assert_eq!(
            st.guides,
            vec![
                Guide {
                    vertical: false,
                    pos: 2.0
                },
                Guide {
                    vertical: true,
                    pos: 5.0
                }
            ]
        );
        let st = orient_canvas(&d, Orient::FlipH).unwrap();
        assert_eq!(
            st.guides,
            vec![
                Guide {
                    vertical: true,
                    pos: 6.0
                },
                Guide {
                    vertical: false,
                    pos: 1.0
                }
            ]
        );
    }

    #[test]
    fn inscribed_rect_fits_inside_rotated_original() {
        assert_eq!(largest_rotated_rect(100.0, 60.0, 0.0), (100.0, 60.0));
        for (w, h, a) in [
            (100.0, 100.0, 5.0),
            (200.0, 100.0, 10.0),
            (100.0, 200.0, -20.0),
            (300.0, 100.0, 40.0),
        ] {
            let (iw, ih) = largest_rotated_rect(w, h, a);
            assert!(
                iw > 0.0 && ih > 0.0 && iw <= w && ih <= h,
                "{w}x{h}@{a}: {iw}x{ih}"
            );
            // Os quatro cantos do retângulo inscrito, desfazendo a rotação, ficam dentro do original.
            let (s, c) = a.to_radians().sin_cos();
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let (px, py) = (sx * iw / 2.0, sy * ih / 2.0);
                let (ox, oy) = (px * c + py * s, -px * s + py * c);
                assert!(
                    ox.abs() <= w / 2.0 + 1e-2 && oy.abs() <= h / 2.0 + 1e-2,
                    "{w}x{h}@{a}"
                );
            }
        }
    }

    #[test]
    fn straighten_gives_no_empty_corners_and_correct_size() {
        let d = Document::new("t", 120, 80, Some([50, 100, 150, 255])).unwrap();
        let st = straighten(&d, 7.0, Interp::Bilinear).unwrap();
        let (iw, ih) = largest_rotated_rect(120.0, 80.0, 7.0);
        assert_eq!(
            (st.width, st.height),
            (iw.floor() as u32, ih.floor() as u32)
        );
        let l = &st.layers[0];
        for (x, y) in [
            (0, 0),
            (l.width - 1, 0),
            (0, l.height - 1),
            (l.width - 1, l.height - 1),
        ] {
            assert_eq!(l.pixel(x, y).unwrap()[3], 255, "canto ({x},{y}) vazio");
        }
        assert_eq!(straighten(&d, 0.0, Interp::Nearest).unwrap().width, 120);
    }

    #[test]
    fn content_bounds_and_alignment() {
        let mut d = Document::new("t", 20, 10, None).unwrap();
        d.layers[0].set_pixel(2, 3, [1, 1, 1, 255]);
        d.layers[0].set_pixel(4, 5, [1, 1, 1, 255]);
        assert_eq!(content_bounds(&d.layers[0]), Some(Rect::new(2, 3, 3, 3)));
        assert!(content_bounds(&Layer::transparent("v", 4, 4)).is_none());
        let id = d.layers[0].id;
        let canvas = Rect::full(20, 10);
        let l = align(&d, &[id], AlignKind::Right, canvas).unwrap();
        assert_eq!(content_bounds(&l[0]), Some(Rect::new(17, 3, 3, 3)));
        let l = align(&d, &[id], AlignKind::Bottom, canvas).unwrap();
        assert_eq!(content_bounds(&l[0]).unwrap().bottom(), 10);
        let l = align(&d, &[id], AlignKind::HCenter, canvas).unwrap();
        let b = content_bounds(&l[0]).unwrap();
        assert!(
            (b.x * 2 + b.w).abs_diff(20) <= 1,
            "centralizado (largura ímpar: ±meio pixel)"
        );
        assert!(
            align(&d, &[id], AlignKind::Left, Rect::new(2, 0, 5, 5)).is_none(),
            "já alinhado"
        );
        // Bloqueio de posição impede o movimento.
        d.layers[0].lock_position = true;
        assert!(align(&d, &[id], AlignKind::Right, canvas).is_none());
    }

    #[test]
    fn distribute_equalises_gaps_and_needs_three_items() {
        let mut d = Document::new("t", 40, 4, None).unwrap();
        let mut ids = Vec::new();
        d.layers.clear();
        for (x, name) in [(0u32, "a"), (7, "b"), (30, "c")] {
            let mut l = Layer::transparent(name, 40, 4);
            for dx in 0..4 {
                l.set_pixel(x + dx, 1, [255, 255, 255, 255]);
            }
            ids.push(l.id);
            d.layers.push(l);
        }
        let out = distribute(&d, &ids, true).unwrap();
        let xs: Vec<u32> = out.iter().map(|l| content_bounds(l).unwrap().x).collect();
        // Vão total 34 - 12 = 22 -> 11 entre cada par.
        assert_eq!(xs, vec![0, 15, 30]);
        assert!(distribute(&d, &ids[..2], true).is_none());
    }

    #[test]
    fn layer_masks_follow_reframe_resize_orient_and_linked_shift() {
        use crate::LayerMask;
        let mut d = Document::new("t", 8, 6, Some([5, 5, 5, 255])).unwrap();
        let mut m = vec![255u8; 48];
        m[2 * 8 + 1] = 0; // marca em (1, 2)
        d.layers[0].mask = Some(LayerMask::new(m));
        let cm = |st: &DocState| st.layers[0].mask.as_ref().unwrap().data().clone();
        let st = crop(&d, 1, 1, 4, 3).unwrap();
        assert_eq!(cm(&st).len(), 12);
        assert_eq!(cm(&st)[4], 0, "marca em (0, 1) após o recorte");
        let st = resize_image(&d, 16, 12, Interp::Nearest, None).unwrap();
        assert_eq!(cm(&st).len(), 192);
        let st = orient_canvas(&d, Orient::Cw90).unwrap();
        // (1, 2) em 8x6 -> (h-1-y, x) = (3, 1) em 6x8.
        assert_eq!(cm(&st)[6 + 3], 0);
        assert_eq!(cm(&st).len(), 48);
        let st = straighten(&d, 3.0, Interp::Bilinear).unwrap();
        assert_eq!(cm(&st).len() as u32, st.width * st.height);
        // Deslocamento vinculado leva a máscara junto; desvinculado, não.
        let mut e = Document::new("t", 8, 6, None).unwrap();
        e.layers[0].set_pixel(1, 1, [1, 1, 1, 255]);
        let mut mk = vec![255u8; 48];
        mk[8 + 1] = 0;
        e.layers[0].mask = Some(LayerMask::new(mk));
        let id = e.layers[0].id;
        let l = align(&e, &[id], AlignKind::Right, Rect::full(8, 6)).unwrap();
        let md = l[0].mask.as_ref().unwrap().data();
        assert_eq!(md[8 + 7], 0, "marca acompanhou o conteúdo para a direita");
        assert_eq!(md[8 + 1], 255);
        e.layers[0].mask.as_mut().unwrap().linked = false;
        let l = align(&e, &[id], AlignKind::Right, Rect::full(8, 6)).unwrap();
        assert_eq!(
            l[0].mask.as_ref().unwrap().data()[8 + 1],
            0,
            "desvinculada fica no lugar"
        );
    }

    #[test]
    fn doc_edit_undo_redo_restores_everything() {
        let mut d = doc_with_marks();
        d.guides = vec![Guide {
            vertical: true,
            pos: 3.0,
        }];
        let before = DocState::of(&d);
        let after = crop(&d, 1, 1, 5, 4).unwrap();
        let mut h = crate::History::new();
        h.execute(&mut d, Box::new(DocEdit::new("Recortar", before, after)));
        assert_eq!((d.width, d.height), (5, 4));
        h.undo(&mut d);
        assert_eq!((d.width, d.height), (8, 6));
        assert_eq!(d.layers[0].pixels.len(), 8 * 6 * 4);
        assert_eq!(d.guides[0].pos, 3.0);
        h.redo(&mut d);
        assert_eq!(d.layers[0].width, 5);
    }
}
