//! Photoshop PSD e PSB (grande): leitura e escrita próprias, baseadas na especificação pública
//! "Adobe Photoshop File Formats Specification".
//!
//! Nível implementado: imagem composta, camadas raster (posição, nome Unicode, visibilidade,
//! opacidade, preenchimento, recorte, 25 modos de mesclagem), grupos, máscaras de camada e
//! perfil ICC/resolução; 8 bits RGB (+ alfa) na escrita; leitura de 8/16 bits RGB e cinza com
//! compressão bruta, RLE e ZIP (com e sem predição). Texto, formas, ajustes, estilos e objetos
//! inteligentes **não** são interpretados: viram pixels (texto/forma/objeto na escrita) ou são
//! omitidos/avisados (leitura). Dados desconhecidos não são preservados.

use std::io::Write;

use flate2::read::ZlibDecoder;
use std::io::Read;

use crate::blend::BlendMode;
use crate::color::IccProfile;
use crate::compose;
use crate::document::{Document, Layer, LayerId, LayerKind, LayerMask};
use crate::imageio::Imported;
use crate::layerops::normalize;

/// Limite de memória dos pixels de todas as camadas importadas (cada uma tem o tamanho do documento).
const MAX_TOTAL_BYTES: u64 = 2 << 30;
const MAX_LAYERS: usize = 2000;
const PSD_MAX_SIDE: u32 = 30_000;

const BLEND_KEYS: &[(BlendMode, &[u8; 4])] = &[
    (BlendMode::Normal, b"norm"),
    (BlendMode::Dissolve, b"diss"),
    (BlendMode::Darken, b"dark"),
    (BlendMode::Multiply, b"mul "),
    (BlendMode::ColorBurn, b"idiv"),
    (BlendMode::LinearBurn, b"lbrn"),
    (BlendMode::Lighten, b"lite"),
    (BlendMode::Screen, b"scrn"),
    (BlendMode::ColorDodge, b"div "),
    (BlendMode::LinearDodge, b"lddg"),
    (BlendMode::Overlay, b"over"),
    (BlendMode::SoftLight, b"sLit"),
    (BlendMode::HardLight, b"hLit"),
    (BlendMode::VividLight, b"vLit"),
    (BlendMode::LinearLight, b"lLit"),
    (BlendMode::PinLight, b"pLit"),
    (BlendMode::HardMix, b"hMix"),
    (BlendMode::Difference, b"diff"),
    (BlendMode::Exclusion, b"smud"),
    (BlendMode::Subtract, b"fsub"),
    (BlendMode::Divide, b"fdiv"),
    (BlendMode::Hue, b"hue "),
    (BlendMode::Saturation, b"sat "),
    (BlendMode::Color, b"colr"),
    (BlendMode::Luminosity, b"lum "),
    (BlendMode::PassThrough, b"pass"),
];

fn blend_key(m: BlendMode) -> [u8; 4] {
    BLEND_KEYS
        .iter()
        .find(|(b, _)| *b == m)
        .map_or(*b"norm", |(_, k)| **k)
}

// ---------------------------------------------------------------------------------------
// Escrita

struct W {
    b: Vec<u8>,
    psb: bool,
}

impl W {
    fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.b.extend_from_slice(&v.to_be_bytes());
    }
    fn i16(&mut self, v: i16) {
        self.b.extend_from_slice(&v.to_be_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_be_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.b.extend_from_slice(&v.to_be_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.b.extend_from_slice(&v.to_be_bytes());
    }
    /// Comprimento de 4 bytes (PSD) ou 8 bytes (PSB).
    fn len(&mut self, v: u64) {
        if self.psb {
            self.u64(v);
        } else {
            self.u32(v as u32);
        }
    }
    fn bytes(&mut self, v: &[u8]) {
        self.b.extend_from_slice(v);
    }
    fn pad_to(&mut self, base: usize, multiple: usize) {
        while !(self.b.len() - base).is_multiple_of(multiple) {
            self.b.push(0);
        }
    }
}

fn packbits(row: &[u8], out: &mut Vec<u8>) {
    let n = row.len();
    let mut i = 0;
    while i < n {
        // Sequência repetida?
        let mut run = 1;
        while i + run < n && run < 128 && row[i + run] == row[i] {
            run += 1;
        }
        if run >= 2 {
            out.push((257 - run) as u8);
            out.push(row[i]);
            i += run;
            continue;
        }
        // Literais até a próxima repetição (ou 128).
        let start = i;
        let mut len = 0;
        while i < n && len < 128 {
            if i + 1 < n && row[i] == row[i + 1] && len > 0 {
                break;
            }
            i += 1;
            len += 1;
        }
        out.push((len - 1) as u8);
        out.extend_from_slice(&row[start..start + len]);
    }
}

/// Plano de um canal (`w × h` bytes) em RLE: contagens por linha + dados.
fn rle_plane(plane: &[u8], w: usize, h: usize, psb: bool) -> Vec<u8> {
    let mut counts = Vec::new();
    let mut data = Vec::new();
    for y in 0..h {
        let before = data.len();
        packbits(&plane[y * w..(y + 1) * w], &mut data);
        let c = data.len() - before;
        if psb {
            counts.extend_from_slice(&(c as u32).to_be_bytes());
        } else {
            counts.extend_from_slice(&(c as u16).to_be_bytes());
        }
    }
    counts.extend_from_slice(&data);
    counts
}

fn baked(doc: &Document, l: &Layer) -> Vec<u8> {
    // Camada com máscara: a máscara vai como canal próprio; os pixels ficam como estão.
    let _ = doc;
    l.pixels.as_ref().clone()
}

struct Rec {
    name: String,
    kind: RecKind,
    blend: [u8; 4],
    opacity: u8,
    fill: u8,
    visible: bool,
    clipped: bool,
}

enum RecKind {
    Raster {
        rect: [i32; 4],                // topo, esquerda, base, direita
        planes: [Vec<u8>; 4],          // A, R, G, B recortados
        mask: Option<(Vec<u8>, bool)>, // dados (documento inteiro) e desativada?
    },
    GroupEnd,
    GroupStart {
        open: bool,
    },
}

fn tagged(w: &mut W, key: &[u8; 4], data: &[u8]) {
    w.bytes(b"8BIM");
    w.bytes(key);
    let padded = data.len().div_ceil(4) * 4;
    w.u32(padded as u32);
    w.bytes(data);
    for _ in data.len()..padded {
        w.u8(0);
    }
}

fn pascal_name(name: &str) -> Vec<u8> {
    let ascii: Vec<u8> = name
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_control() {
                c as u8
            } else {
                b'?'
            }
        })
        .take(255)
        .collect();
    let mut v = vec![ascii.len() as u8];
    v.extend_from_slice(&ascii);
    while v.len() % 4 != 0 {
        v.push(0);
    }
    v
}

fn collect(
    doc: &Document,
    parent: Option<LayerId>,
    out: &mut Vec<Rec>,
    warnings: &mut Vec<String>,
) {
    for l in doc.layers.iter().filter(|l| l.parent == parent) {
        let opacity = (l.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let fill = (l.fill.clamp(0.0, 1.0) * 255.0).round() as u8;
        let blend = blend_key(l.blend);
        if l.kind == LayerKind::Group {
            out.push(Rec {
                name: "</Layer group>".into(),
                kind: RecKind::GroupEnd,
                blend: *b"norm",
                opacity: 255,
                fill: 255,
                visible: true,
                clipped: false,
            });
            collect(doc, Some(l.id), out, warnings);
            out.push(Rec {
                name: l.name.clone(),
                kind: RecKind::GroupStart { open: l.expanded },
                blend: if l.blend == BlendMode::Normal {
                    *b"norm"
                } else {
                    blend
                },
                opacity,
                fill,
                visible: l.visible,
                clipped: false,
            });
            continue;
        }
        if l.adjustment.is_some() {
            warnings.push(format!("A camada de ajuste \"{}\" não é exportada para PSD (sem equivalente implementado).", l.name));
            continue;
        }
        if l.source.is_some() || l.object.is_some() {
            warnings.push(format!("\"{}\" (texto/forma/preenchimento/objeto) foi gravada como pixels: não fica editável no Photoshop.", l.name));
        }
        if !l.styles.is_empty() {
            warnings.push(format!(
                "Os estilos de \"{}\" não são gravados no PSD (só os pixels da camada).",
                l.name
            ));
        }
        let px = baked(doc, l);
        let (w, h) = (l.width as usize, l.height as usize);
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
        for y in 0..h {
            for x in 0..w {
                if px[(y * w + x) * 4 + 3] > 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        let (rect, planes) = if x1 > x0 {
            let (cw, ch) = (x1 - x0, y1 - y0);
            let mut planes: [Vec<u8>; 4] = std::array::from_fn(|_| Vec::with_capacity(cw * ch));
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * w + x) * 4;
                    planes[0].push(px[i + 3]);
                    planes[1].push(px[i]);
                    planes[2].push(px[i + 1]);
                    planes[3].push(px[i + 2]);
                }
            }
            ([y0 as i32, x0 as i32, y1 as i32, x1 as i32], planes)
        } else {
            ([0, 0, 0, 0], std::array::from_fn(|_| Vec::new()))
        };
        let mask = l.mask.as_ref().map(|m| {
            let data = if m.density() < 1.0 || m.feather() > 0.0 {
                m.effective(doc.width, doc.height).as_ref().clone()
            } else {
                m.data().as_ref().clone()
            };
            (data, !m.enabled)
        });
        if l.mask
            .as_ref()
            .is_some_and(|m| m.density() < 1.0 || m.feather() > 0.0)
        {
            warnings.push(format!(
                "A densidade/pena da máscara de \"{}\" foi embutida nos valores da máscara.",
                l.name
            ));
        }
        out.push(Rec {
            name: l.name.clone(),
            kind: RecKind::Raster { rect, planes, mask },
            blend,
            opacity,
            fill,
            visible: l.visible,
            clipped: l.clipped,
        });
    }
}

/// Grava o documento como PSD (`psb == false`) ou PSB. Retorna os bytes e os avisos de perdas.
pub fn write(doc: &Document, psb: bool) -> Result<(Vec<u8>, Vec<String>), String> {
    if !psb && (doc.width > PSD_MAX_SIDE || doc.height > PSD_MAX_SIDE) {
        return Err(format!(
            "PSD aceita no máximo {PSD_MAX_SIDE} px por lado; use PSB."
        ));
    }
    let mut warnings = Vec::new();
    let composite = compose::flatten(doc);
    let has_alpha = composite.as_chunks::<4>().0.iter().any(|p| p[3] < 255);
    let (w, h) = (doc.width as usize, doc.height as usize);
    let mut out = W { b: Vec::new(), psb };

    // Cabeçalho.
    out.bytes(b"8BPS");
    out.u16(if psb { 2 } else { 1 });
    out.bytes(&[0; 6]);
    out.u16(if has_alpha { 4 } else { 3 });
    out.u32(doc.height);
    out.u32(doc.width);
    out.u16(8);
    out.u16(3); // RGB

    // Dados do modo de cor.
    out.u32(0);

    // Recursos de imagem: resolução e perfil ICC.
    let res_start = out.b.len();
    out.u32(0);
    let body = out.b.len();
    let res = |id: u16, data: &[u8], out: &mut W| {
        out.bytes(b"8BIM");
        out.u16(id);
        out.u16(0); // nome vazio (Pascal, com preenchimento)
        out.u32(data.len() as u32);
        out.bytes(data);
        if data.len() % 2 == 1 {
            out.u8(0);
        }
    };
    let ppi = (doc.ppi.clamp(1.0, 10000.0) * 65536.0) as u32;
    let mut r = Vec::new();
    r.extend_from_slice(&ppi.to_be_bytes());
    r.extend_from_slice(&1u16.to_be_bytes());
    r.extend_from_slice(&1u16.to_be_bytes());
    r.extend_from_slice(&ppi.to_be_bytes());
    r.extend_from_slice(&1u16.to_be_bytes());
    r.extend_from_slice(&1u16.to_be_bytes());
    res(0x03ED, &r, &mut out);
    if let Some(p) = &doc.icc {
        res(0x040F, &p.data, &mut out);
    }
    let rlen = (out.b.len() - body) as u32;
    out.b[res_start..res_start + 4].copy_from_slice(&rlen.to_be_bytes());

    // Camadas e máscaras.
    let mut recs = Vec::new();
    collect(doc, None, &mut recs, &mut warnings);
    let section_len_at = out.b.len();
    out.len(0);
    let section_body = out.b.len();
    let info_len_at = out.b.len();
    out.len(0);
    let info_body = out.b.len();
    let count = recs.len() as i16;
    out.i16(if has_alpha { -count } else { count });
    // Registros.
    struct ChanData {
        id: i16,
        bytes: Vec<u8>,
    }
    let mut all_data: Vec<Vec<ChanData>> = Vec::new();
    for rec in &recs {
        let mut chans: Vec<ChanData> = Vec::new();
        let (rect, mask_rec) = match &rec.kind {
            RecKind::Raster { rect, planes, mask } => {
                let (cw, ch) = (
                    (rect[3] - rect[1]).max(0) as usize,
                    (rect[2] - rect[0]).max(0) as usize,
                );
                for (id, plane) in [-1i16, 0, 1, 2].into_iter().zip(planes.iter()) {
                    let mut b = Vec::new();
                    if cw == 0 || ch == 0 {
                        b.extend_from_slice(&0u16.to_be_bytes());
                    } else {
                        b.extend_from_slice(&1u16.to_be_bytes());
                        b.extend_from_slice(&rle_plane(plane, cw, ch, psb));
                    }
                    chans.push(ChanData { id, bytes: b });
                }
                if let Some((data, _)) = mask {
                    let mut b = 1u16.to_be_bytes().to_vec();
                    b.extend_from_slice(&rle_plane(data, w, h, psb));
                    chans.push(ChanData { id: -2, bytes: b });
                }
                (*rect, mask.as_ref())
            }
            RecKind::GroupEnd | RecKind::GroupStart { .. } => {
                for id in [-1i16, 0, 1, 2] {
                    chans.push(ChanData {
                        id,
                        bytes: 0u16.to_be_bytes().to_vec(),
                    });
                }
                ([0, 0, 0, 0], None)
            }
        };
        for v in rect {
            out.i32(v);
        }
        out.u16(chans.len() as u16);
        for c in &chans {
            out.i16(c.id);
            out.len(c.bytes.len() as u64);
        }
        out.bytes(b"8BIM");
        out.bytes(&rec.blend);
        out.u8(rec.opacity);
        out.u8(u8::from(rec.clipped));
        out.u8(if rec.visible { 0 } else { 2 } | 0x08); // bit 3: o bit 4 é válido
        out.u8(0);
        // Extra data.
        let extra_at = out.b.len();
        out.u32(0);
        let extra_body = out.b.len();
        match mask_rec {
            Some((_, disabled)) => {
                out.u32(20);
                for v in [0i32, 0, h as i32, w as i32] {
                    out.i32(v);
                }
                out.u8(255);
                out.u8(if *disabled { 2 } else { 0 });
                out.u16(0);
            }
            None => out.u32(0),
        }
        out.u32(0); // faixas de mesclagem
        out.bytes(&pascal_name(&rec.name));
        // Blocos adicionais.
        let utf16: Vec<u8> = {
            let mut v = (rec.name.encode_utf16().count() as u32)
                .to_be_bytes()
                .to_vec();
            for u in rec.name.encode_utf16() {
                v.extend_from_slice(&u.to_be_bytes());
            }
            v.extend_from_slice(&[0, 0]);
            v
        };
        tagged(&mut out, b"luni", &utf16);
        tagged(&mut out, b"iOpa", &[rec.fill]);
        match &rec.kind {
            RecKind::GroupEnd => {
                let mut d = 3u32.to_be_bytes().to_vec();
                d.extend_from_slice(b"8BIM");
                d.extend_from_slice(b"norm");
                tagged(&mut out, b"lsct", &d);
            }
            RecKind::GroupStart { open } => {
                let mut d = (if *open { 1u32 } else { 2u32 }).to_be_bytes().to_vec();
                d.extend_from_slice(b"8BIM");
                d.extend_from_slice(&rec.blend);
                tagged(&mut out, b"lsct", &d);
            }
            RecKind::Raster { .. } => {}
        }
        let elen = (out.b.len() - extra_body) as u32;
        out.b[extra_at..extra_at + 4].copy_from_slice(&elen.to_be_bytes());
        all_data.push(chans);
    }
    for chans in &all_data {
        for c in chans {
            out.bytes(&c.bytes);
        }
    }
    out.pad_to(info_body, 4);
    let info_len = (out.b.len() - info_body) as u64;
    patch_len(&mut out.b, info_len_at, info_len, psb);
    out.u32(0); // máscara global
    let sect_len = (out.b.len() - section_body) as u64;
    patch_len(&mut out.b, section_len_at, sect_len, psb);

    // Imagem composta (RLE).
    out.u16(1);
    let nch = if has_alpha { 4 } else { 3 };
    let planes: Vec<Vec<u8>> = (0..nch)
        .map(|c| composite.as_chunks::<4>().0.iter().map(|p| p[c]).collect())
        .collect();
    let mut counts = Vec::new();
    let mut data = Vec::new();
    for p in &planes {
        for y in 0..h {
            let before = data.len();
            packbits(&p[y * w..(y + 1) * w], &mut data);
            let c = data.len() - before;
            if psb {
                counts.extend_from_slice(&(c as u32).to_be_bytes());
            } else {
                counts.extend_from_slice(&(c as u16).to_be_bytes());
            }
        }
    }
    out.bytes(&counts);
    out.bytes(&data);
    if recs.is_empty() {
        warnings.push("O documento não tem camadas gravadas além da imagem composta.".into());
    }
    let _ = (&mut warnings, Write::flush(&mut std::io::sink()));
    Ok((out.b, warnings))
}

fn patch_len(buf: &mut [u8], at: usize, v: u64, psb: bool) {
    if psb {
        buf[at..at + 8].copy_from_slice(&v.to_be_bytes());
    } else {
        buf[at..at + 4].copy_from_slice(&(v as u32).to_be_bytes());
    }
}

// ---------------------------------------------------------------------------------------
// Leitura

struct R<'a> {
    b: &'a [u8],
    p: usize,
    psb: bool,
}

type Res<T> = Result<T, String>;

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let end = self
            .p
            .checked_add(n)
            .filter(|e| *e <= self.b.len())
            .ok_or("Arquivo PSD truncado ou corrompido.")?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Res<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Res<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn i16(&mut self) -> Res<i16> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Res<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Res<i32> {
        Ok(self.u32()? as i32)
    }
    fn u64(&mut self) -> Res<u64> {
        let b = self.take(8)?;
        Ok(u64::from_be_bytes(b.try_into().map_err(|_| "leitura")?))
    }
    fn len(&mut self) -> Res<u64> {
        if self.psb {
            self.u64()
        } else {
            Ok(u64::from(self.u32()?))
        }
    }
    fn skip(&mut self, n: u64) -> Res<()> {
        let n = usize::try_from(n).map_err(|_| "Tamanho inválido no PSD.")?;
        self.take(n).map(|_| ())
    }
}

fn unpackbits(src: &[u8], row_len: usize) -> Res<Vec<u8>> {
    let mut out = Vec::with_capacity(row_len);
    let mut i = 0;
    while out.len() < row_len {
        let n = *src.get(i).ok_or("Dados RLE truncados.")? as i8;
        i += 1;
        if n >= 0 {
            let cnt = n as usize + 1;
            let s = src.get(i..i + cnt).ok_or("Dados RLE truncados.")?;
            out.extend_from_slice(s);
            i += cnt;
        } else if n != -128 {
            let cnt = (1 - i16::from(n)) as usize;
            let v = *src.get(i).ok_or("Dados RLE truncados.")?;
            i += 1;
            out.extend(std::iter::repeat_n(v, cnt));
        }
    }
    out.truncate(row_len);
    Ok(out)
}

/// Desfaz a predição por deltas do ZIP (dentro de cada linha, 8 ou 16 bits).
fn unpredict(out: &mut [u8], w: usize, h: usize, bytes_per: usize) {
    let row_bytes = w * bytes_per;
    for y in 0..h {
        let row = &mut out[y * row_bytes..(y + 1) * row_bytes];
        if bytes_per == 1 {
            for x in 1..row.len() {
                row[x] = row[x].wrapping_add(row[x - 1]);
            }
        } else {
            for x in 1..w {
                let prev = u16::from_be_bytes([row[(x - 1) * 2], row[(x - 1) * 2 + 1]]);
                let cur = u16::from_be_bytes([row[x * 2], row[x * 2 + 1]]);
                let v = cur.wrapping_add(prev).to_be_bytes();
                row[x * 2] = v[0];
                row[x * 2 + 1] = v[1];
            }
        }
    }
}

/// Decodifica um canal de `w × h` (8 ou 16 bits por amostra) para bytes de 8 bits.
fn decode_channel(
    data: &[u8],
    w: usize,
    h: usize,
    depth: u16,
    psb: bool,
    comp: u16,
) -> Res<Vec<u8>> {
    let bytes_per = (depth / 8) as usize;
    let row_bytes = w * bytes_per;
    let raw: Vec<u8> = match comp {
        0 => data
            .get(..row_bytes * h)
            .ok_or("Canal bruto truncado.")?
            .to_vec(),
        1 => {
            let cnt_size = if psb { 4 } else { 2 };
            let table = data.get(..cnt_size * h).ok_or("Tabela RLE truncada.")?;
            let mut pos = cnt_size * h;
            let mut out = Vec::with_capacity(row_bytes * h);
            for y in 0..h {
                let c = if psb {
                    u32::from_be_bytes([
                        table[y * 4],
                        table[y * 4 + 1],
                        table[y * 4 + 2],
                        table[y * 4 + 3],
                    ]) as usize
                } else {
                    u16::from_be_bytes([table[y * 2], table[y * 2 + 1]]) as usize
                };
                let end = pos
                    .checked_add(c)
                    .filter(|e| *e <= data.len())
                    .ok_or("Linha RLE fora do arquivo.")?;
                out.extend_from_slice(&unpackbits(&data[pos..end], row_bytes)?);
                pos = end;
            }
            out
        }
        2 | 3 => {
            let mut out = Vec::new();
            ZlibDecoder::new(data)
                .take((row_bytes * h) as u64 + 1)
                .read_to_end(&mut out)
                .map_err(|e| format!("Canal ZIP inválido: {e}"))?;
            if out.len() < row_bytes * h {
                return Err("Canal ZIP truncado.".into());
            }
            out.truncate(row_bytes * h);
            if comp == 3 {
                unpredict(&mut out, w, h, bytes_per);
            }
            out
        }
        other => return Err(format!("Compressão PSD {other} não suportada.")),
    };
    Ok(if bytes_per == 2 {
        raw.as_chunks::<2>().0.iter().map(|c| c[0]).collect()
    } else {
        raw
    })
}

struct LayerInfo {
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    chans: Vec<(i16, u64)>,
    blend: [u8; 4],
    opacity: u8,
    clipping: u8,
    flags: u8,
    name: String,
    fill: u8,
    divider: Option<u32>,
    mask_rect: Option<[i32; 4]>,
    mask_default: u8,
    mask_disabled: bool,
    notes: Vec<String>,
}

fn parse_layer(r: &mut R, count_budget: &mut Vec<String>) -> Res<LayerInfo> {
    let (top, left, bottom, right) = (r.i32()?, r.i32()?, r.i32()?, r.i32()?);
    let nch = r.u16()? as usize;
    if nch > 56 {
        return Err("Canais demais em uma camada PSD.".into());
    }
    let mut chans = Vec::new();
    for _ in 0..nch {
        let id = r.i16()?;
        let len = r.len()?;
        chans.push((id, len));
    }
    if r.take(4)? != b"8BIM" {
        return Err("Assinatura de mesclagem inválida no PSD.".into());
    }
    let blend: [u8; 4] = r.take(4)?.try_into().map_err(|_| "leitura")?;
    let opacity = r.u8()?;
    let clipping = r.u8()?;
    let flags = r.u8()?;
    r.u8()?;
    let extra_len = u64::from(r.u32()?);
    let extra_end =
        r.p.checked_add(extra_len as usize)
            .filter(|e| *e <= r.b.len())
            .ok_or("Dados extras da camada fora do arquivo.")?;
    let mut info = LayerInfo {
        top,
        left,
        bottom,
        right,
        chans,
        blend,
        opacity,
        clipping,
        flags,
        name: String::new(),
        fill: 255,
        divider: None,
        mask_rect: None,
        mask_default: 0,
        mask_disabled: false,
        notes: Vec::new(),
    };
    // Máscara de camada.
    let mlen = r.u32()? as usize;
    if mlen >= 18 {
        let m = r.take(mlen)?;
        let i32at = |o: usize| i32::from_be_bytes([m[o], m[o + 1], m[o + 2], m[o + 3]]);
        info.mask_rect = Some([i32at(0), i32at(4), i32at(8), i32at(12)]);
        info.mask_default = m[16];
        info.mask_disabled = m[17] & 2 != 0;
    } else {
        r.skip(mlen as u64)?;
    }
    let blen = r.u32()? as usize;
    r.skip(blen as u64)?;
    // Nome Pascal (múltiplo de 4).
    let nlen = r.u8()? as usize;
    let nm = r.take(nlen)?;
    info.name = String::from_utf8_lossy(nm).into_owned();
    let used = 1 + nlen;
    r.skip(((4 - used % 4) % 4) as u64)?;
    // Blocos adicionais.
    while r.p + 12 <= extra_end {
        let sig = r.take(4)?;
        if sig != b"8BIM" && sig != b"8B64" {
            break;
        }
        let key: [u8; 4] = r.take(4)?.try_into().map_err(|_| "leitura")?;
        let wide = r.psb
            && matches!(
                &key,
                b"LMsk"
                    | b"Lr16"
                    | b"Lr32"
                    | b"Layr"
                    | b"Mt16"
                    | b"Mt32"
                    | b"Mtrn"
                    | b"Alph"
                    | b"FMsk"
                    | b"lnk2"
                    | b"FEid"
                    | b"FXid"
                    | b"PxSD"
            );
        let len = if wide { r.u64()? } else { u64::from(r.u32()?) };
        let end =
            r.p.checked_add(len as usize)
                .filter(|e| *e <= extra_end.max(r.p))
                .ok_or("Bloco adicional fora da camada.")?;
        let data = &r.b[r.p..end];
        match &key {
            b"luni" if data.len() >= 4 => {
                let n = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
                let units: Vec<u16> = data[4..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .take(n)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                info.name = String::from_utf16_lossy(&units);
            }
            b"iOpa" if !data.is_empty() => info.fill = data[0],
            b"lsct" | b"lsdk" if data.len() >= 4 => {
                info.divider = Some(u32::from_be_bytes([data[0], data[1], data[2], data[3]]));
            }
            b"TySh" | b"tySh" => info
                .notes
                .push("texto editável não é interpretado (pixels mantidos)".into()),
            b"SoCo" | b"GdFl" | b"PtFl" => info
                .notes
                .push("camada de preenchimento tratada como pixels".into()),
            b"lfx2" | b"lrFX" | b"lfxs" => {
                info.notes.push("efeitos de camada não importados".into())
            }
            b"SoLd" | b"PlLd" | b"SoLE" => info
                .notes
                .push("objeto inteligente tratado como pixels".into()),
            b"brit" | b"levl" | b"curv" | b"hue2" | b"hue " | b"blnc" | b"blwh" | b"phfl"
            | b"mixr" | b"clrL" | b"nvrt" | b"post" | b"thrs" | b"selc" | b"grdm" | b"vibA"
            | b"expA" => {
                info.notes.push(format!(
                    "camada de ajuste ({}) sem pixels",
                    String::from_utf8_lossy(&key)
                ));
            }
            _ => {}
        }
        r.p = end;
    }
    r.p = extra_end;
    let _ = count_budget;
    Ok(info)
}

fn gray_or_rgb(depth: u16, mode: u16) -> Res<()> {
    if depth != 8 && depth != 16 {
        return Err(format!(
            "Profundidade de {depth} bits não suportada (apenas 8 e 16)."
        ));
    }
    match mode {
        1 | 3 => Ok(()),
        0 => Err("Modo Bitmap não suportado.".into()),
        2 => Err("Modo Indexado não suportado.".into()),
        4 => Err("Modo CMYK não suportado (converta para RGB no Photoshop).".into()),
        7 => Err("Modo Multicanal não suportado.".into()),
        8 => Err("Modo Duotone não suportado.".into()),
        9 => Err("Modo Lab não suportado.".into()),
        other => Err(format!("Modo de cor PSD {other} não suportado.")),
    }
}

/// Importa um PSD/PSB. Camadas não representáveis são omitidas/aviso; o resto vira camadas Orvalith.
pub fn read(bytes: &[u8], name: &str) -> Result<Imported, String> {
    let mut r = R {
        b: bytes,
        p: 0,
        psb: false,
    };
    if r.take(4)? != b"8BPS" {
        return Err("Não é um arquivo Photoshop (assinatura 8BPS ausente).".into());
    }
    let version = r.u16()?;
    r.psb = match version {
        1 => false,
        2 => true,
        v => return Err(format!("Versão PSD {v} desconhecida.")),
    };
    r.skip(6)?;
    let channels = r.u16()? as usize;
    let (height, width) = (r.u32()?, r.u32()?);
    let depth = r.u16()?;
    let mode = r.u16()?;
    if width == 0 || height == 0 || width > Document::MAX_SIDE || height > Document::MAX_SIDE {
        return Err(format!(
            "Dimensões PSD {width}×{height} fora do suportado (máximo {} px).",
            Document::MAX_SIDE
        ));
    }
    gray_or_rgb(depth, mode)?;
    // Defesa contra bombas: nem o melhor ZIP (~1000:1) cabe em um arquivo menor que isto.
    if u64::from(width) * u64::from(height) > bytes.len() as u64 * 1100 {
        return Err("As dimensões do PSD são incompatíveis com o tamanho do arquivo (arquivo corrompido ou malicioso).".into());
    }
    let (w, h) = (width as usize, height as usize);
    let mut warnings = Vec::new();
    if depth == 16 {
        warnings.push(
            "O arquivo tem 16 bits por canal: convertido para 8 bits (precisão reduzida).".into(),
        );
    }
    if mode == 1 {
        warnings.push("Modo Tons de cinza convertido para RGB.".into());
    }
    let color_len = r.u32()? as u64;
    r.skip(color_len)?;
    // Recursos de imagem.
    let res_len = r.u32()? as usize;
    let res_end =
        r.p.checked_add(res_len)
            .filter(|e| *e <= bytes.len())
            .ok_or("Recursos de imagem fora do arquivo.")?;
    let mut icc: Option<Vec<u8>> = None;
    let mut ppi = 72.0f32;
    while r.p + 12 <= res_end {
        if r.take(4)? != b"8BIM" {
            break;
        }
        let id = r.u16()?;
        let nl = r.u8()? as usize;
        r.skip(((nl + 1) % 2 + nl) as u64)?;
        let dlen = r.u32()? as usize;
        let data = r.take(dlen)?;
        if dlen % 2 == 1 {
            r.skip(1)?;
        }
        match id {
            0x040F => icc = Some(data.to_vec()),
            0x03ED if data.len() >= 4 => {
                let v = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as f32 / 65536.0;
                if v.is_finite() && v >= 1.0 {
                    ppi = v;
                }
            }
            _ => {}
        }
    }
    r.p = res_end;
    // Camadas e máscaras.
    let section_len = r.len()?;
    let section_end =
        r.p.checked_add(section_len as usize)
            .filter(|e| *e <= bytes.len())
            .ok_or("Seção de camadas fora do arquivo.")?;
    let mut layers: Vec<Layer> = Vec::new();
    let mut unsupported: Vec<String> = Vec::new();
    if section_len > 0 {
        let info_len = r.len()?;
        let info_end =
            r.p.checked_add(info_len as usize)
                .filter(|e| *e <= section_end)
                .ok_or("Informações de camadas fora do arquivo.")?;
        if info_len > 0 {
            let count = r.i16()?;
            let has_merged_alpha = count < 0;
            let n = count.unsigned_abs() as usize;
            if n > MAX_LAYERS {
                return Err(format!("Camadas demais no PSD ({n})."));
            }
            let mut infos = Vec::new();
            let mut scratch = Vec::new();
            for _ in 0..n {
                infos.push(parse_layer(&mut r, &mut scratch)?);
            }
            let _ = has_merged_alpha;
            let mut budget: u64 = MAX_TOTAL_BYTES;
            // Decodifica os dados dos canais, na ordem dos registros.
            struct Decoded {
                info: LayerInfo,
                rgba: Option<Vec<u8>>, // do tamanho do documento
                mask: Option<Vec<u8>>, // do tamanho do documento
            }
            let mut decoded: Vec<Decoded> = Vec::new();
            for info in infos {
                let (lw, lh) = (
                    (info.right - info.left).max(0) as usize,
                    (info.bottom - info.top).max(0) as usize,
                );
                let mut planes: [Option<Vec<u8>>; 4] = [None, None, None, None]; // R G B A
                let mut gray: Option<Vec<u8>> = None;
                let mut mask_plane: Option<Vec<u8>> = None;
                for (id, len) in &info.chans {
                    let len = usize::try_from(*len).map_err(|_| "Canal grande demais.")?;
                    let comp_and_data = r.take(len)?;
                    if len < 2 {
                        continue;
                    }
                    let comp = u16::from_be_bytes([comp_and_data[0], comp_and_data[1]]);
                    let data = &comp_and_data[2..];
                    let (cw, ch) = match id {
                        -2 => match info.mask_rect {
                            Some(m) => {
                                ((m[3] - m[1]).max(0) as usize, (m[2] - m[0]).max(0) as usize)
                            }
                            None => (0, 0),
                        },
                        -3 => (0, 0),
                        _ => (lw, lh),
                    };
                    if cw == 0 || ch == 0 || cw.saturating_mul(ch) > 400_000_000 {
                        continue;
                    }
                    if (cw as u64) * (ch as u64) > (len as u64) * 1100 {
                        return Err(
                            "Canal PSD maior do que os dados disponíveis (arquivo corrompido)."
                                .into(),
                        );
                    }
                    let plane = decode_channel(data, cw, ch, depth, r.psb, comp)?;
                    match id {
                        0 => {
                            if mode == 1 {
                                gray = Some(plane);
                            } else {
                                planes[0] = Some(plane);
                            }
                        }
                        1 => planes[1] = Some(plane),
                        2 => planes[2] = Some(plane),
                        -1 => planes[3] = Some(plane),
                        -2 => mask_plane = Some(plane),
                        _ => {}
                    }
                }
                let is_marker = info.divider.is_some();
                let cost = (w * h * 4) as u64;
                let mut rgba = None;
                if !is_marker && lw > 0 && lh > 0 {
                    if budget < cost {
                        return Err("O PSD excede a memória permitida para as camadas (cada camada tem o tamanho do documento).".into());
                    }
                    budget -= cost;
                    let mut px = vec![0u8; w * h * 4];
                    for y in 0..lh {
                        let dy = y as i64 + i64::from(info.top);
                        if dy < 0 || dy >= h as i64 {
                            continue;
                        }
                        for x in 0..lw {
                            let dx = x as i64 + i64::from(info.left);
                            if dx < 0 || dx >= w as i64 {
                                continue;
                            }
                            let si = y * lw + x;
                            let di = (dy as usize * w + dx as usize) * 4;
                            let (rr, gg, bb) = match &gray {
                                Some(g) => (g[si], g[si], g[si]),
                                None => (
                                    planes[0].as_ref().map_or(0, |p| p[si]),
                                    planes[1].as_ref().map_or(0, |p| p[si]),
                                    planes[2].as_ref().map_or(0, |p| p[si]),
                                ),
                            };
                            px[di..di + 4].copy_from_slice(&[
                                rr,
                                gg,
                                bb,
                                planes[3].as_ref().map_or(255, |p| p[si]),
                            ]);
                        }
                    }
                    rgba = Some(px);
                }
                let mut mask = None;
                if let (Some(plane), Some(m)) = (&mask_plane, info.mask_rect) {
                    let (mw, mh) = ((m[3] - m[1]).max(0) as usize, (m[2] - m[0]).max(0) as usize);
                    let mut full = vec![info.mask_default; w * h];
                    for y in 0..mh {
                        let dy = y as i64 + i64::from(m[0]);
                        if dy < 0 || dy >= h as i64 {
                            continue;
                        }
                        for x in 0..mw {
                            let dx = x as i64 + i64::from(m[1]);
                            if dx >= 0 && dx < w as i64 {
                                full[dy as usize * w + dx as usize] = plane[y * mw + x];
                            }
                        }
                    }
                    mask = Some(full);
                }
                decoded.push(Decoded { info, rgba, mask });
            }
            r.p = info_end.max(r.p);
            // Monta a árvore: o arquivo lista de baixo para cima; o fim do grupo vem primeiro.
            let mut stack: Vec<Vec<Layer>> = vec![Vec::new()];
            for d in decoded {
                let i = d.info;
                let blend = BLEND_KEYS
                    .iter()
                    .find(|(_, k)| **k == i.blend)
                    .map(|(b, _)| *b);
                let make_common = |l: &mut Layer| {
                    l.visible = i.flags & 2 == 0;
                    l.opacity = f32::from(i.opacity) / 255.0;
                    l.fill = f32::from(i.fill) / 255.0;
                    l.clipped = i.clipping != 0;
                };
                match i.divider {
                    Some(3) => stack.push(Vec::new()),
                    Some(1) | Some(2) => {
                        let kids = if stack.len() > 1 {
                            stack.pop().unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                        let mut g = Layer::group(if i.name.is_empty() {
                            "Grupo".to_string()
                        } else {
                            i.name.clone()
                        });
                        make_common(&mut g);
                        g.expanded = i.divider == Some(1);
                        g.clipped = false;
                        g.blend = match blend {
                            Some(b) => b,
                            None => {
                                unsupported.push(format!(
                                    "modo de mesclagem \"{}\" do grupo \"{}\"",
                                    String::from_utf8_lossy(&i.blend),
                                    i.name
                                ));
                                BlendMode::PassThrough
                            }
                        };
                        let gid = g.id;
                        let mut kids = kids;
                        for k in &mut kids {
                            if k.parent.is_none() {
                                k.parent = Some(gid);
                            }
                        }
                        let top = stack.last_mut().ok_or("Estrutura de grupos inválida.")?;
                        top.push(g);
                        top.extend(kids);
                    }
                    _ => {
                        for n in &i.notes {
                            unsupported.push(format!("\"{}\": {n}", i.name));
                        }
                        let Some(px) = d.rgba else {
                            if i.notes.iter().any(|n| n.starts_with("camada de ajuste")) {
                                continue;
                            }
                            // Camada vazia: mantida como camada transparente.
                            let mut l = Layer::transparent(i.name.clone(), width, height);
                            make_common(&mut l);
                            stack.last_mut().ok_or("Estrutura inválida.")?.push(l);
                            continue;
                        };
                        let mut l = Layer::from_pixels(
                            if i.name.is_empty() {
                                "Camada".to_string()
                            } else {
                                i.name.clone()
                            },
                            width,
                            height,
                            px,
                        );
                        make_common(&mut l);
                        l.blend = match blend {
                            Some(BlendMode::PassThrough) | None => {
                                if blend.is_none() {
                                    unsupported.push(format!(
                                        "modo de mesclagem \"{}\" de \"{}\"",
                                        String::from_utf8_lossy(&i.blend),
                                        i.name
                                    ));
                                }
                                BlendMode::Normal
                            }
                            Some(b) => b,
                        };
                        if let Some(m) = d.mask {
                            let mut lm = LayerMask::new(m);
                            lm.enabled = !i.mask_disabled;
                            l.mask = Some(lm);
                        }
                        stack.last_mut().ok_or("Estrutura inválida.")?.push(l);
                    }
                }
            }
            // Grupos não fechados: junta o que sobrou.
            while stack.len() > 1 {
                let kids = stack.pop().unwrap_or_default();
                if let Some(top) = stack.last_mut() {
                    top.extend(kids);
                }
            }
            layers = stack.pop().unwrap_or_default();
        }
    }
    r.p = section_end;
    // Imagem composta (também é o fallback de arquivos sem camadas).
    let composite = read_composite(&mut r, w, h, channels, depth, mode);
    let doc_layers = if layers.is_empty() {
        match composite {
            Ok(px) => vec![Layer::from_pixels("Fundo", width, height, px)],
            Err(e) => {
                return Err(format!(
                    "O PSD não tem camadas legíveis e a imagem composta falhou: {e}"
                ))
            }
        }
    } else {
        if let Err(e) = &composite {
            warnings.push(format!(
                "Imagem composta não lida ({e}); as camadas foram usadas."
            ));
        }
        normalize(layers)
    };
    if !unsupported.is_empty() {
        unsupported.sort();
        unsupported.dedup();
        warnings.push(format!(
            "Recursos do Photoshop não suportados (viraram pixels ou foram omitidos): {}. Dados desconhecidos não são preservados; salve como .orva para manter o trabalho sem sobrescrever o PSD.",
            unsupported.join("; ")
        ));
    }
    warnings.push("PSD/PSB: camadas raster, grupos, visibilidade, opacidade, modos de mesclagem e máscaras foram importados.".into());
    let mut doc = Document::from_parts(name.to_string(), width, height, ppi, doc_layers);
    if let Some(bytes) = icc {
        match crate::color::profile_from_bytes(&bytes) {
            Ok(p) => doc.icc = Some(p),
            Err(e) => {
                warnings.push(format!("Perfil de cor do PSD não suportado ({e}); preservado apenas como aviso: a imagem é tratada como sRGB."));
                let _: Option<IccProfile> = None;
            }
        }
    }
    Ok(Imported {
        document: doc,
        warnings,
    })
}

fn read_composite(
    r: &mut R,
    w: usize,
    h: usize,
    channels: usize,
    depth: u16,
    mode: u16,
) -> Res<Vec<u8>> {
    let comp = r.u16()?;
    let rest = &r.b[r.p..];
    let want = if mode == 1 { 1 } else { 3 };
    if channels < want {
        return Err("Poucos canais na imagem composta.".into());
    }
    let n = channels.min(if mode == 1 { 2 } else { 4 });
    let bytes_per = (depth / 8) as usize;
    let mut planes: Vec<Vec<u8>> = Vec::new();
    match comp {
        0 => {
            let sz = w * h * bytes_per;
            for c in 0..n {
                let s = rest
                    .get(c * sz..(c + 1) * sz)
                    .ok_or("Imagem composta truncada.")?;
                planes.push(decode_channel(s, w, h, depth, r.psb, 0)?);
            }
        }
        1 => {
            let cnt_size = if r.psb { 4 } else { 2 };
            let table_len = cnt_size * h * channels;
            let table = rest
                .get(..table_len)
                .ok_or("Tabela RLE da imagem composta truncada.")?;
            let mut pos = table_len;
            for c in 0..n {
                let mut chan = Vec::with_capacity(w * h * bytes_per);
                // As linhas dos canais anteriores já avançaram `pos`; para canais ignorados também.
                for y in 0..h {
                    let t = (c * h + y) * cnt_size;
                    let cnt = if r.psb {
                        u32::from_be_bytes([table[t], table[t + 1], table[t + 2], table[t + 3]])
                            as usize
                    } else {
                        u16::from_be_bytes([table[t], table[t + 1]]) as usize
                    };
                    let end = pos
                        .checked_add(cnt)
                        .filter(|e| *e <= rest.len())
                        .ok_or("Linha RLE da imagem composta fora do arquivo.")?;
                    chan.extend_from_slice(&unpackbits(&rest[pos..end], w * bytes_per)?);
                    pos = end;
                }
                planes.push(if bytes_per == 2 {
                    chan.as_chunks::<2>().0.iter().map(|c| c[0]).collect()
                } else {
                    chan
                });
            }
        }
        2 | 3 => {
            let mut raw = Vec::new();
            let total = w * h * bytes_per * n;
            ZlibDecoder::new(rest)
                .take(total as u64 + 1)
                .read_to_end(&mut raw)
                .map_err(|e| format!("ZIP inválido: {e}"))?;
            if raw.len() < total {
                return Err("Imagem composta ZIP truncada.".into());
            }
            for c in 0..n {
                let mut plane = raw[c * w * h * bytes_per..(c + 1) * w * h * bytes_per].to_vec();
                if comp == 3 {
                    unpredict(&mut plane, w, h, bytes_per);
                }
                planes.push(decode_channel(&plane, w, h, depth, r.psb, 0)?);
            }
        }
        other => {
            return Err(format!(
                "Compressão {other} da imagem composta não suportada."
            ))
        }
    }
    let mut px = Vec::with_capacity(w * h * 4);
    for i in 0..w * h {
        if mode == 1 {
            let g = planes[0][i];
            px.extend_from_slice(&[g, g, g, planes.get(1).map_or(255, |p| p[i])]);
        } else {
            px.extend_from_slice(&[
                planes[0][i],
                planes[1][i],
                planes[2][i],
                planes.get(3).map_or(255, |p| p[i]),
            ]);
        }
    }
    Ok(px)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Document {
        let mut d = Document::new("psd", 24, 16, Some([240, 240, 240, 255])).unwrap();
        let mut a = Layer::transparent("Vermelho ção", 24, 16);
        for y in 3..9 {
            for x in 4..14 {
                a.set_pixel(x, y, [220, 20, 20, 255]);
            }
        }
        a.opacity = 0.5;
        a.blend = BlendMode::Multiply;
        let mut g = Layer::group("Grupo A");
        g.expanded = true;
        let gid = g.id;
        let mut b = Layer::transparent("Azul", 24, 16);
        for y in 6..14 {
            for x in 10..20 {
                b.set_pixel(x, y, [20, 20, 220, 200]);
            }
        }
        b.parent = Some(gid);
        b.visible = false;
        let mut c = Layer::filled("Com máscara", 24, 16, [10, 200, 10, 255]);
        let mut mask = vec![0u8; 24 * 16];
        for y in 0..16 {
            for x in 12..24 {
                mask[y * 24 + x] = 255;
            }
        }
        c.mask = Some(LayerMask::new(mask));
        c.parent = Some(gid);
        c.clipped = true;
        d.layers.extend([a, g, b, c]);
        d
    }

    #[test]
    fn psd_and_psb_roundtrip_preserve_layers_groups_masks_and_composite() {
        let d = sample();
        for psb in [false, true] {
            let (bytes, warnings) = write(&d, psb).unwrap();
            assert_eq!(&bytes[0..4], b"8BPS");
            assert_eq!(
                u16::from_be_bytes([bytes[4], bytes[5]]),
                if psb { 2 } else { 1 }
            );
            let back = read(&bytes, "x").unwrap().document;
            assert_eq!((back.width, back.height), (24, 16));
            let names: Vec<&str> = back.layers.iter().map(|l| l.name.as_str()).collect();
            assert_eq!(
                names,
                vec!["Fundo", "Vermelho ção", "Grupo A", "Azul", "Com máscara"],
                "psb={psb}"
            );
            let red = &back.layers[1];
            assert!((red.opacity - 0.5).abs() < 0.01 && red.blend == BlendMode::Multiply);
            assert_eq!(red.pixels, d.layers[1].pixels, "pixels idênticos");
            let g = &back.layers[2];
            assert!(g.is_group() && g.expanded);
            assert_eq!(back.layers[3].parent, Some(g.id));
            assert!(!back.layers[3].visible);
            assert_eq!(back.layers[3].pixels, d.layers[3].pixels);
            let m = back.layers[4].mask.as_ref().expect("máscara importada");
            assert_eq!(
                m.data().as_ref(),
                d.layers[4].mask.as_ref().unwrap().data().as_ref()
            );
            assert!(back.layers[4].clipped && back.layers[4].parent == Some(g.id));
            // A opacidade 0,5 vira 128/255 no arquivo: aceita diferença de arredondamento.
            let (fa, fb) = (compose::flatten(&back), compose::flatten(&d));
            assert!(
                fa.iter().zip(&fb).all(|(a, b)| a.abs_diff(*b) <= 2),
                "composição equivalente após reabrir (psb={psb})"
            );
            assert!(warnings.is_empty(), "{warnings:?}");
        }
    }

    #[test]
    fn independent_reader_agrees_with_our_psd_writer() {
        // O crate `psd` (leitor independente) lê o arquivo gravado por nós.
        let d = sample();
        let (bytes, _) = write(&d, false).unwrap();
        let p = psd::Psd::from_bytes(&bytes).expect("leitor independente abre o PSD");
        assert_eq!((p.width(), p.height()), (24, 16));
        let names: Vec<String> = p.layers().iter().map(|l| l.name().to_string()).collect();
        for want in ["Vermelho", "Azul"] {
            assert!(names.iter().any(|n| n.starts_with(want)), "{names:?}");
        }
        let comp = p.rgba();
        let ours = compose::flatten(&d);
        let diff = comp
            .iter()
            .zip(&ours)
            .filter(|(a, b)| a.abs_diff(**b) > 1)
            .count();
        assert_eq!(diff, 0, "imagem composta idêntica pelo leitor independente");
    }

    #[test]
    fn alpha_composite_icc_and_resolution_roundtrip() {
        let mut d = Document::new("a", 10, 8, None).unwrap();
        d.layers[0].set_pixel(2, 2, [255, 0, 0, 128]);
        d.ppi = 300.0;
        d.icc = crate::color::builtin_profiles().into_iter().nth(2);
        let (bytes, _) = write(&d, false).unwrap();
        let back = read(&bytes, "a").unwrap();
        assert_eq!(back.document.ppi.round(), 300.0);
        assert_eq!(
            back.document.icc.as_ref().map(|p| p.name.clone()),
            d.icc.as_ref().map(|p| p.name.clone())
        );
        assert_eq!(back.document.layers[0].pixel(2, 2), d.layers[0].pixel(2, 2));
        // O primeiro canal alfa do composto marca a transparência.
        assert_eq!(compose::flatten(&back.document), compose::flatten(&d));
    }

    #[test]
    fn unsupported_content_is_reported_and_dimensions_are_limited() {
        let mut d = sample();
        d.layers.push(Layer::transparent("aj", 24, 16));
        let last = d.layers.len() - 1;
        d.layers[last].adjustment = Some(crate::adjust::Adjustment::Invert);
        let (_, w) = write(&d, false).unwrap();
        assert!(w.iter().any(|x| x.contains("ajuste")));
        let mut t = sample();
        t.layers[1].source = Some(crate::LayerSource::Fill(crate::source::FillSpec::neutral(
            0,
        )));
        let (_, w) = write(&t, false).unwrap();
        assert!(w.iter().any(|x| x.contains("pixels")));
        let big = Document::new("g", 30_001, 2, None).unwrap();
        assert!(write(&big, false).is_err() && write(&big, true).is_ok());
    }

    /// Monta um PSD pequeno à mão com compressão ZIP e RLE e 16 bits para testar o leitor.
    fn hand_made(depth: u16, comp: u16, mode: u16) -> Vec<u8> {
        let (w, h) = (4usize, 2usize);
        let mut v = Vec::new();
        v.extend_from_slice(b"8BPS");
        v.extend_from_slice(&1u16.to_be_bytes());
        v.extend_from_slice(&[0; 6]);
        let nch: u16 = if mode == 1 { 1 } else { 3 };
        v.extend_from_slice(&nch.to_be_bytes());
        v.extend_from_slice(&(h as u32).to_be_bytes());
        v.extend_from_slice(&(w as u32).to_be_bytes());
        v.extend_from_slice(&depth.to_be_bytes());
        v.extend_from_slice(&mode.to_be_bytes());
        v.extend_from_slice(&0u32.to_be_bytes()); // modo de cor
        v.extend_from_slice(&0u32.to_be_bytes()); // recursos
        v.extend_from_slice(&0u32.to_be_bytes()); // sem camadas
        v.extend_from_slice(&comp.to_be_bytes());
        let bytes_per = (depth / 8) as usize;
        let mut planes = Vec::new();
        for c in 0..nch as usize {
            let mut p = Vec::new();
            for i in 0..w * h {
                let val = (40 * (c + 1) + i * 3) as u8;
                for _ in 0..bytes_per {
                    p.push(val);
                }
            }
            planes.push(p);
        }
        match comp {
            0 => planes.iter().for_each(|p| v.extend_from_slice(p)),
            1 => {
                let mut counts = Vec::new();
                let mut data = Vec::new();
                for p in &planes {
                    for y in 0..h {
                        let before = data.len();
                        packbits(&p[y * w * bytes_per..(y + 1) * w * bytes_per], &mut data);
                        counts.extend_from_slice(&((data.len() - before) as u16).to_be_bytes());
                    }
                }
                v.extend_from_slice(&counts);
                v.extend_from_slice(&data);
            }
            _ => {
                let all: Vec<u8> = planes.concat();
                let mut e =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(&all).unwrap();
                v.extend_from_slice(&e.finish().unwrap());
            }
        }
        v
    }

    #[test]
    fn reader_handles_raw_rle_zip_16bit_and_grayscale_composites() {
        for (depth, comp, mode) in [
            (8, 0, 3),
            (8, 1, 3),
            (8, 2, 3),
            (16, 1, 3),
            (8, 1, 1),
            (16, 0, 1),
        ] {
            let imp = read(&hand_made(depth, comp, mode), "m").unwrap();
            let l = &imp.document.layers[0];
            let want = |c: usize, i: usize| (40 * (c + 1) + i * 3) as u8;
            let p = l.pixel(1, 1).unwrap();
            let i = 5;
            if mode == 1 {
                assert_eq!(
                    p,
                    [want(0, i), want(0, i), want(0, i), 255],
                    "{depth} {comp} {mode}"
                );
            } else {
                assert_eq!(
                    p,
                    [want(0, i), want(1, i), want(2, i), 255],
                    "{depth} {comp} {mode}"
                );
            }
            assert_eq!(
                imp.warnings.iter().any(|w| w.contains("16 bits")),
                depth == 16
            );
        }
    }

    #[test]
    fn unsupported_modes_and_hostile_files_fail_cleanly_without_panics() {
        let cmyk = hand_made(8, 0, 3);
        let mut c = cmyk.clone();
        c[24..26].copy_from_slice(&4u16.to_be_bytes());
        assert!(read(&c, "x").unwrap_err().contains("CMYK"));
        assert!(read(b"nao e psd", "x").is_err());
        assert!(read(&[], "x").is_err());
        // Truncar em qualquer ponto nunca causa pânico.
        let good = write(&sample(), false).unwrap().0;
        for cut in (0..good.len()).step_by(37) {
            let _ = read(&good[..cut], "x");
        }
        // Bytes corrompidos também não.
        for i in (0..good.len()).step_by(53) {
            let mut bad = good.clone();
            bad[i] ^= 0xA5;
            let _ = read(&bad, "x");
        }
        // Dimensões absurdas são recusadas antes de alocar.
        let mut huge = hand_made(8, 0, 3);
        huge[14..18].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(read(&huge, "x").is_err());
    }
}
