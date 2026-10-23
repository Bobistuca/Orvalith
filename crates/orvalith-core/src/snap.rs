//! Encaixe (snapping) em guias, grade e bordas do documento.

use crate::document::Guide;

#[derive(Debug, Clone, Copy)]
pub struct SnapOpts {
    pub guides: bool,
    pub grid: bool,
    pub edges: bool,
    /// Espaçamento da grade em pixels do documento.
    pub grid_size: f32,
}

impl Default for SnapOpts {
    fn default() -> Self {
        Self {
            guides: true,
            grid: false,
            edges: true,
            grid_size: 50.0,
        }
    }
}

/// Encaixa um valor num eixo. `x_axis`: o valor é uma coordenada X (guias verticais).
/// `extent` = largura (X) ou altura (Y) do documento; `threshold` em pixels do documento.
pub fn snap_axis(
    v: f32,
    x_axis: bool,
    guides: &[Guide],
    o: &SnapOpts,
    extent: f32,
    threshold: f32,
) -> f32 {
    let mut best: Option<(f32, f32)> = None; // (distância, alvo)
    let mut consider = |t: f32| {
        let d = (t - v).abs();
        if d <= threshold && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, t));
        }
    };
    if o.guides {
        for g in guides.iter().filter(|g| g.vertical == x_axis) {
            consider(g.pos);
        }
    }
    if o.edges {
        consider(0.0);
        consider(extent);
    }
    if o.grid && o.grid_size >= 1.0 {
        consider((v / o.grid_size).round() * o.grid_size);
    }
    best.map_or(v, |(_, t)| t)
}

/// Encaixa um ponto (x, y) do documento.
pub fn snap_point(
    p: (f32, f32),
    guides: &[Guide],
    o: &SnapOpts,
    doc_size: (u32, u32),
    threshold: f32,
) -> (f32, f32) {
    (
        snap_axis(p.0, true, guides, o, doc_size.0 as f32, threshold),
        snap_axis(p.1, false, guides, o, doc_size.1 as f32, threshold),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guides() -> Vec<Guide> {
        vec![
            Guide {
                vertical: true,
                pos: 100.0,
            },
            Guide {
                vertical: false,
                pos: 40.0,
            },
        ]
    }

    #[test]
    fn snaps_to_nearest_within_threshold_per_axis() {
        let o = SnapOpts::default();
        let p = snap_point((103.0, 200.0), &guides(), &o, (500, 300), 5.0);
        assert_eq!(p, (100.0, 200.0));
        let p = snap_point((300.0, 42.0), &guides(), &o, (500, 300), 5.0);
        assert_eq!(p, (300.0, 40.0));
        // Fora do limiar não encaixa.
        assert_eq!(
            snap_point((110.0, 60.0), &guides(), &o, (500, 300), 5.0),
            (110.0, 60.0)
        );
    }

    #[test]
    fn edges_grid_and_disabled_sources() {
        let mut o = SnapOpts::default();
        assert_eq!(snap_axis(2.0, true, &[], &o, 500.0, 5.0), 0.0);
        assert_eq!(snap_axis(497.0, true, &[], &o, 500.0, 5.0), 500.0);
        o.edges = false;
        assert_eq!(snap_axis(2.0, true, &[], &o, 500.0, 5.0), 2.0);
        o.grid = true;
        o.grid_size = 50.0;
        assert_eq!(snap_axis(148.0, true, &[], &o, 500.0, 5.0), 150.0);
        o.guides = false;
        assert_eq!(snap_axis(103.0, true, &guides(), &o, 500.0, 2.0), 103.0);
    }

    #[test]
    fn nearest_candidate_wins() {
        let gs = vec![
            Guide {
                vertical: true,
                pos: 100.0,
            },
            Guide {
                vertical: true,
                pos: 104.0,
            },
        ];
        let o = SnapOpts::default();
        assert_eq!(snap_axis(103.0, true, &gs, &o, 500.0, 5.0), 104.0);
    }
}
