//! Importação de recursos comuns: paletas GIMP (`.gpl`) e gradientes GIMP (`.ggr`).
//!
//! Só entram formatos realmente suportados. Pincéis `.abr`, amostras `.ase` e outros formatos
//! proprietários são recusados com motivo (ver [`unsupported_reason`]). Toda leitura é
//! limitada em tamanho e em quantidade de entradas.

use crate::adjust::GradStop;

/// Tamanho máximo aceito para um arquivo de recurso.
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
/// Máximo de cores/paradas importadas por arquivo.
pub const MAX_ENTRIES: usize = 512;

/// Paleta importada.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<[u8; 4]>,
}

/// Gradiente importado, já reduzido a paradas de cor.
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub name: String,
    pub stops: Vec<GradStop>,
}

fn check_size(len: usize) -> Result<(), String> {
    if len > MAX_BYTES {
        Err(format!(
            "Arquivo grande demais ({} MiB; máximo {} MiB)",
            len / (1024 * 1024),
            MAX_BYTES / (1024 * 1024)
        ))
    } else {
        Ok(())
    }
}

/// Lê uma paleta GIMP (`GIMP Palette`). Linhas inválidas são ignoradas; sem nenhuma cor é erro.
pub fn parse_gpl(bytes: &[u8]) -> Result<Palette, String> {
    check_size(bytes.len())?;
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("GIMP Palette") {
        return Err("Não é uma paleta GIMP (.gpl)".into());
    }
    let mut name = String::from("Paleta importada");
    let mut colors = Vec::new();
    for line in lines {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some(n) = l.strip_prefix("Name:") {
            let n = n.trim();
            if !n.is_empty() {
                name = n.chars().take(80).collect();
            }
            continue;
        }
        if l.starts_with("Columns:") {
            continue;
        }
        let mut parts = l.split_whitespace();
        let mut rgb = [0u8; 3];
        let mut ok = true;
        for c in &mut rgb {
            match parts.next().and_then(|p| p.parse::<u16>().ok()) {
                Some(v) if v <= 255 => *c = v as u8,
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok && colors.len() < MAX_ENTRIES {
            colors.push([rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    if colors.is_empty() {
        return Err("A paleta não tem cores válidas".into());
    }
    Ok(Palette { name, colors })
}

/// Lê um gradiente GIMP (`GIMP Gradient`). Cada segmento contribui com sua cor esquerda e a
/// direita do último; transparência e modos de mistura não-lineares são aproximados por paradas.
pub fn parse_ggr(bytes: &[u8]) -> Result<Gradient, String> {
    check_size(bytes.len())?;
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("GIMP Gradient") {
        return Err("Não é um gradiente GIMP (.ggr)".into());
    }
    let mut name = String::from("Gradiente importado");
    let mut count: Option<usize> = None;
    let mut segs: Vec<[f32; 11]> = Vec::new();
    for line in lines {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(n) = l.strip_prefix("Name:") {
            let n = n.trim();
            if !n.is_empty() {
                name = n.chars().take(80).collect();
            }
            continue;
        }
        if count.is_none() {
            count = l.parse::<usize>().ok();
            if count.is_none() {
                return Err("Cabeçalho do gradiente inválido".into());
            }
            continue;
        }
        let v: Vec<f32> = l
            .split_whitespace()
            .take(11)
            .map(|p| p.parse::<f32>().unwrap_or(f32::NAN))
            .collect();
        if v.len() == 11 && v.iter().all(|x| x.is_finite()) && segs.len() < MAX_ENTRIES {
            let mut a = [0f32; 11];
            a.copy_from_slice(&v);
            segs.push(a);
        }
    }
    if segs.is_empty() {
        return Err("O gradiente não tem segmentos válidos".into());
    }
    let to8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut stops: Vec<GradStop> = Vec::new();
    for s in &segs {
        stops.push(GradStop {
            pos: s[0].clamp(0.0, 1.0),
            color: [to8(s[3]), to8(s[4]), to8(s[5])],
        });
    }
    let last = segs[segs.len() - 1];
    stops.push(GradStop {
        pos: last[2].clamp(0.0, 1.0),
        color: [to8(last[7]), to8(last[8]), to8(last[9])],
    });
    stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    stops.dedup_by(|a, b| (a.pos - b.pos).abs() < 1e-6);
    if stops.len() < 2 {
        return Err("O gradiente precisa de pelo menos duas paradas".into());
    }
    Ok(Gradient { name, stops })
}

/// Motivo pelo qual uma extensão conhecida não é importada (`None` = não é um recurso conhecido
/// ou é suportado).
pub fn unsupported_reason(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "abr" => Some("Pincéis .abr (formato proprietário do Photoshop) não são suportados"),
        "ase" | "aco" => Some("Amostras .ase/.aco (formato proprietário) não são suportadas"),
        "grd" => Some("Gradientes .grd (formato proprietário) não são suportados"),
        "asl" | "pat" => Some("Estilos/padrões do Photoshop não são suportados"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpl_parses_colors_and_skips_garbage() {
        let src = "GIMP Palette\nName: Teste\nColumns: 2\n#c\n255 0 0 Vermelho\n  0 128 255\nx y z\n300 0 0\n10 20\n";
        let p = parse_gpl(src.as_bytes()).unwrap();
        assert_eq!(p.name, "Teste");
        assert_eq!(p.colors, vec![[255, 0, 0, 255], [0, 128, 255, 255]]);
    }

    #[test]
    fn gpl_rejects_wrong_header_empty_and_huge() {
        assert!(parse_gpl(b"PNG").is_err());
        assert!(parse_gpl(b"GIMP Palette\nName: x\n").is_err());
        assert!(parse_gpl(&vec![b'a'; MAX_BYTES + 1]).is_err());
        let mut many = String::from("GIMP Palette\n");
        for _ in 0..2000 {
            many.push_str("1 2 3\n");
        }
        assert_eq!(
            parse_gpl(many.as_bytes()).unwrap().colors.len(),
            MAX_ENTRIES
        );
    }

    #[test]
    fn ggr_builds_ordered_stops() {
        let src = "GIMP Gradient\nName: Fogo\n2\n\
0.000000 0.250000 0.500000 1.0 0.0 0.0 1.0 1.0 1.0 0.0 1.0 0 0 0 0\n\
0.500000 0.750000 1.000000 1.0 1.0 0.0 1.0 0.0 0.0 1.0 1.0 0 0 0 0\n";
        let g = parse_ggr(src.as_bytes()).unwrap();
        assert_eq!(g.name, "Fogo");
        let pos: Vec<f32> = g.stops.iter().map(|s| s.pos).collect();
        assert_eq!(pos, vec![0.0, 0.5, 1.0]);
        assert_eq!(g.stops[0].color, [255, 0, 0]);
        assert_eq!(g.stops[2].color, [0, 0, 255]);
    }

    #[test]
    fn ggr_rejects_hostile_input() {
        assert!(parse_ggr(b"GIMP Gradient\nName: x\nabc\n").is_err());
        assert!(parse_ggr(b"GIMP Gradient\n1\n0 nan 1 0 0 0 1 1 1 1 1\n").is_err());
        assert!(parse_ggr(b"nada").is_err());
        assert!(parse_ggr(&vec![b'0'; MAX_BYTES + 1]).is_err());
    }

    #[test]
    fn proprietary_formats_are_declined_with_reason() {
        assert!(unsupported_reason("ABR").is_some());
        assert!(unsupported_reason("ase").is_some());
        assert!(unsupported_reason("gpl").is_none());
    }
}
