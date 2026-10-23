//! Histogramas de luminância e de canais (8 bits).

/// Canal exibido/consultado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Luma,
    Red,
    Green,
    Blue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Histogram {
    pub luma: [u32; 256],
    pub r: [u32; 256],
    pub g: [u32; 256],
    pub b: [u32; 256],
    /// Pixels contados (alfa > 0 e dentro da máscara, quando houver).
    pub pixels: u64,
}

/// Estatísticas de um canal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub mean: f32,
    pub median: u8,
    pub std_dev: f32,
    pub min: u8,
    pub max: u8,
}

impl Histogram {
    pub fn bins(&self, ch: Channel) -> &[u32; 256] {
        match ch {
            Channel::Luma => &self.luma,
            Channel::Red => &self.r,
            Channel::Green => &self.g,
            Channel::Blue => &self.b,
        }
    }

    /// `None` se não houver pixels.
    pub fn stats(&self, ch: Channel) -> Option<Stats> {
        let bins = self.bins(ch);
        let total: u64 = bins.iter().map(|&n| u64::from(n)).sum();
        if total == 0 {
            return None;
        }
        let sum: f64 = bins
            .iter()
            .enumerate()
            .map(|(i, &n)| i as f64 * f64::from(n))
            .sum();
        let mean = sum / total as f64;
        let var: f64 = bins
            .iter()
            .enumerate()
            .map(|(i, &n)| (i as f64 - mean).powi(2) * f64::from(n))
            .sum::<f64>()
            / total as f64;
        let mut acc = 0u64;
        let mut median = 0u8;
        for (i, &n) in bins.iter().enumerate() {
            acc += u64::from(n);
            if acc * 2 >= total {
                median = i as u8;
                break;
            }
        }
        let min = bins.iter().position(|&n| n > 0).unwrap_or(0) as u8;
        let max = bins.iter().rposition(|&n| n > 0).unwrap_or(0) as u8;
        Some(Stats {
            mean: mean as f32,
            median,
            std_dev: var.sqrt() as f32,
            min,
            max,
        })
    }
}

/// Calcula o histograma de um buffer RGBA; `mask` (1 byte por pixel) restringe a contagem
/// aos pixels com valor > 0.
pub fn compute(rgba: &[u8], mask: Option<&[u8]>) -> Histogram {
    let mut h = Histogram {
        luma: [0; 256],
        r: [0; 256],
        g: [0; 256],
        b: [0; 256],
        pixels: 0,
    };
    for (i, p) in rgba.as_chunks::<4>().0.iter().enumerate() {
        if p[3] == 0 || mask.is_some_and(|m| m.get(i).copied().unwrap_or(0) == 0) {
            continue;
        }
        h.r[p[0] as usize] += 1;
        h.g[p[1] as usize] += 1;
        h.b[p[2] as usize] += 1;
        let y =
            (u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114 + 500) / 1000;
        h.luma[y.min(255) as usize] += 1;
        h.pixels += 1;
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_ignore_transparent_and_masked_pixels() {
        let rgba = [
            255, 0, 0, 255, // vermelho
            0, 255, 0, 0, // transparente
            0, 0, 255, 255, // azul
            10, 10, 10, 255, // fora da máscara
        ];
        let h = compute(&rgba, Some(&[255, 255, 255, 0]));
        assert_eq!(h.pixels, 2);
        assert_eq!(h.r[255], 1);
        assert_eq!(h.b[255], 1);
        assert_eq!(h.g[255], 0);
        assert_eq!(h.luma.iter().sum::<u32>(), 2);
        let all = compute(&rgba, None);
        assert_eq!(all.pixels, 3);
    }

    #[test]
    fn stats_of_a_flat_image() {
        let rgba: Vec<u8> = (0..16).flat_map(|_| [100, 100, 100, 255]).collect();
        let s = compute(&rgba, None).stats(Channel::Luma).unwrap();
        assert_eq!((s.median, s.min, s.max), (100, 100, 100));
        assert!((s.mean - 100.0).abs() < 1e-3 && s.std_dev < 1e-3);
        assert!(compute(&[], None).stats(Channel::Red).is_none());
    }

    #[test]
    fn luma_of_primaries() {
        let h = compute(&[255, 255, 255, 255, 0, 0, 0, 255], None);
        assert_eq!(h.luma[255], 1);
        assert_eq!(h.luma[0], 1);
    }
}
