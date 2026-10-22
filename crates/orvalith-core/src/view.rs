//! Transformação entre coordenadas do documento e da viewport.
//!
//! Unidades da viewport: pontos lógicos da UI (egui). `zoom` = pontos por pixel
//! do documento, portanto independente do DPI; 100% real (1 pixel do documento =
//! 1 pixel físico) corresponde a `zoom = 1 / pixels_per_point`.

pub const MIN_ZOOM: f32 = 0.01;
pub const MAX_ZOOM: f32 = 64.0;

/// Visão de um documento: zoom e posição (na viewport) da origem do documento.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub zoom: f32,
    /// Posição, relativa ao canto da viewport, do pixel (0, 0) do documento.
    pub offset: [f32; 2],
}

impl View {
    /// Ajusta o documento inteiro à viewport (sem ampliar além de `max_zoom`), centralizado.
    pub fn fit(doc: [u32; 2], viewport: [f32; 2], margin: f32, max_zoom: f32) -> Self {
        let aw = (viewport[0] - 2.0 * margin).max(1.0);
        let ah = (viewport[1] - 2.0 * margin).max(1.0);
        let zoom = (aw / doc[0].max(1) as f32)
            .min(ah / doc[1].max(1) as f32)
            .clamp(MIN_ZOOM, max_zoom.clamp(MIN_ZOOM, MAX_ZOOM));
        Self::centered(doc, viewport, zoom)
    }

    /// Zoom dado, documento centralizado na viewport.
    pub fn centered(doc: [u32; 2], viewport: [f32; 2], zoom: f32) -> Self {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        Self {
            zoom,
            offset: [
                (viewport[0] - doc[0] as f32 * zoom) / 2.0,
                (viewport[1] - doc[1] as f32 * zoom) / 2.0,
            ],
        }
    }

    /// Tamanho real: 1 pixel do documento = 1 pixel físico da tela.
    pub fn actual_size(doc: [u32; 2], viewport: [f32; 2], pixels_per_point: f32) -> Self {
        Self::centered(doc, viewport, 1.0 / pixels_per_point.max(0.1))
    }

    pub fn doc_to_view(&self, p: [f32; 2]) -> [f32; 2] {
        [
            self.offset[0] + p[0] * self.zoom,
            self.offset[1] + p[1] * self.zoom,
        ]
    }

    pub fn view_to_doc(&self, p: [f32; 2]) -> [f32; 2] {
        [
            (p[0] - self.offset[0]) / self.zoom,
            (p[1] - self.offset[1]) / self.zoom,
        ]
    }

    /// Multiplica o zoom por `factor` mantendo fixo o ponto `anchor` (coordenadas da viewport).
    pub fn zoom_at(&mut self, anchor: [f32; 2], factor: f32) {
        let new_zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let k = new_zoom / self.zoom;
        self.offset = [
            anchor[0] - (anchor[0] - self.offset[0]) * k,
            anchor[1] - (anchor[1] - self.offset[1]) * k,
        ];
        self.zoom = new_zoom;
    }

    pub fn pan(&mut self, delta: [f32; 2]) {
        self.offset[0] += delta[0];
        self.offset[1] += delta[1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    #[test]
    fn roundtrip_at_various_zooms() {
        for zoom in [0.05, 0.5, 1.0, 3.0, 32.0] {
            let v = View::centered([1920, 1080], [1000.0, 700.0], zoom);
            for p in [[0.0, 0.0], [10.5, 99.25], [1919.0, 1079.0]] {
                assert!(close(v.view_to_doc(v.doc_to_view(p)), p), "zoom {zoom}");
            }
        }
    }

    #[test]
    fn zoom_at_keeps_anchor_fixed() {
        let mut v = View::centered([800, 600], [1000.0, 700.0], 1.0);
        let anchor = [250.0, 120.0];
        let before = v.view_to_doc(anchor);
        v.zoom_at(anchor, 4.0);
        assert!(close(v.view_to_doc(anchor), before));
        assert_eq!(v.zoom, 4.0);
    }

    #[test]
    fn zoom_is_clamped() {
        let mut v = View::centered([10, 10], [100.0, 100.0], 1.0);
        v.zoom_at([0.0, 0.0], 1e9);
        assert_eq!(v.zoom, MAX_ZOOM);
        v.zoom_at([0.0, 0.0], 1e-9);
        assert_eq!(v.zoom, MIN_ZOOM);
    }

    #[test]
    fn fit_fits_and_centers_without_upscaling() {
        let v = View::fit([2000, 1000], [1000.0, 1000.0], 0.0, 1.0);
        assert_eq!(v.zoom, 0.5);
        assert!(close(v.doc_to_view([0.0, 0.0]), [0.0, 250.0]));
        let small = View::fit([10, 10], [1000.0, 1000.0], 0.0, 1.0);
        assert_eq!(small.zoom, 1.0);
    }

    #[test]
    fn actual_size_respects_dpi_scale() {
        // Em tela 2x, 1 px do documento ocupa 0,5 ponto lógico.
        let v = View::actual_size([100, 100], [500.0, 500.0], 2.0);
        assert_eq!(v.zoom, 0.5);
        let v = View::actual_size([100, 100], [500.0, 500.0], 1.0);
        assert_eq!(v.zoom, 1.0);
    }
}
