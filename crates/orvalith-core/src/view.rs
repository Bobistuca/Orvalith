//! Transformação entre coordenadas do documento e da viewport.
//!
//! Unidades da viewport: pontos lógicos da UI (egui). `zoom` = pontos por pixel
//! do documento, portanto independente do DPI; 100% real (1 pixel do documento =
//! 1 pixel físico) corresponde a `zoom = 1 / pixels_per_point`.
//!
//! A visão pode ser girada (`rotation_deg`, sentido horário) em torno de `pivot`
//! (normalmente o centro da viewport): a imagem original não é alterada.

pub const MIN_ZOOM: f32 = 0.01;
pub const MAX_ZOOM: f32 = 64.0;

/// Visão de um documento: zoom, posição da origem e rotação.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub zoom: f32,
    /// Posição, relativa ao canto da viewport e antes da rotação, do pixel (0, 0) do documento.
    pub offset: [f32; 2],
    /// Rotação da visão em graus (horário).
    pub rotation_deg: f32,
    /// Centro da rotação, em coordenadas da viewport.
    pub pivot: [f32; 2],
}

impl View {
    /// Ajusta o documento inteiro (considerando a rotação) à viewport, centralizado.
    pub fn fit(doc: [u32; 2], viewport: [f32; 2], margin: f32, max_zoom: f32) -> Self {
        Self::fit_rotated(doc, viewport, margin, max_zoom, 0.0)
    }

    pub fn fit_rotated(
        doc: [u32; 2],
        viewport: [f32; 2],
        margin: f32,
        max_zoom: f32,
        rotation_deg: f32,
    ) -> Self {
        let aw = (viewport[0] - 2.0 * margin).max(1.0);
        let ah = (viewport[1] - 2.0 * margin).max(1.0);
        let (s, c) = rotation_deg.to_radians().sin_cos();
        let (w, h) = (doc[0].max(1) as f32, doc[1].max(1) as f32);
        let (bw, bh) = (w * c.abs() + h * s.abs(), w * s.abs() + h * c.abs());
        let zoom = (aw / bw)
            .min(ah / bh)
            .clamp(MIN_ZOOM, max_zoom.clamp(MIN_ZOOM, MAX_ZOOM));
        Self {
            rotation_deg,
            ..Self::centered(doc, viewport, zoom)
        }
    }

    /// Zoom dado, documento centralizado na viewport (sem rotação).
    pub fn centered(doc: [u32; 2], viewport: [f32; 2], zoom: f32) -> Self {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        Self {
            zoom,
            offset: [
                (viewport[0] - doc[0] as f32 * zoom) / 2.0,
                (viewport[1] - doc[1] as f32 * zoom) / 2.0,
            ],
            rotation_deg: 0.0,
            pivot: [viewport[0] / 2.0, viewport[1] / 2.0],
        }
    }

    /// Tamanho real: 1 pixel do documento = 1 pixel físico da tela.
    pub fn actual_size(doc: [u32; 2], viewport: [f32; 2], pixels_per_point: f32) -> Self {
        Self::centered(doc, viewport, 1.0 / pixels_per_point.max(0.1))
    }

    fn rotate(&self, v: [f32; 2], inverse: bool) -> [f32; 2] {
        if self.rotation_deg == 0.0 {
            return v;
        }
        let (mut s, c) = self.rotation_deg.to_radians().sin_cos();
        if inverse {
            s = -s;
        }
        [c * v[0] - s * v[1], s * v[0] + c * v[1]]
    }

    pub fn doc_to_view(&self, p: [f32; 2]) -> [f32; 2] {
        let q = [
            self.offset[0] + p[0] * self.zoom,
            self.offset[1] + p[1] * self.zoom,
        ];
        let r = self.rotate([q[0] - self.pivot[0], q[1] - self.pivot[1]], false);
        [self.pivot[0] + r[0], self.pivot[1] + r[1]]
    }

    pub fn view_to_doc(&self, p: [f32; 2]) -> [f32; 2] {
        let r = self.rotate([p[0] - self.pivot[0], p[1] - self.pivot[1]], true);
        let q = [self.pivot[0] + r[0], self.pivot[1] + r[1]];
        [
            (q[0] - self.offset[0]) / self.zoom,
            (q[1] - self.offset[1]) / self.zoom,
        ]
    }

    /// Multiplica o zoom por `factor` mantendo fixo o ponto `anchor` (coordenadas da viewport).
    pub fn zoom_at(&mut self, anchor: [f32; 2], factor: f32) {
        let doc_point = self.view_to_doc(anchor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let r = self.rotate([anchor[0] - self.pivot[0], anchor[1] - self.pivot[1]], true);
        let q = [self.pivot[0] + r[0], self.pivot[1] + r[1]];
        self.offset = [
            q[0] - doc_point[0] * self.zoom,
            q[1] - doc_point[1] * self.zoom,
        ];
    }

    /// Desloca a visão por `delta` (coordenadas da viewport, ou seja, o que o usuário vê).
    pub fn pan(&mut self, delta: [f32; 2]) {
        let d = self.rotate(delta, true);
        self.offset[0] += d[0];
        self.offset[1] += d[1];
    }

    /// Gira a visão em torno do centro da viewport mantendo o centro da imagem na tela.
    pub fn rotate_by(&mut self, delta_deg: f32) {
        let center_doc = self.view_to_doc(self.pivot);
        self.rotation_deg = (self.rotation_deg + delta_deg).rem_euclid(360.0);
        let r = self.rotate([0.0, 0.0], true);
        let q = [self.pivot[0] + r[0], self.pivot[1] + r[1]];
        self.offset = [
            q[0] - center_doc[0] * self.zoom,
            q[1] - center_doc[1] * self.zoom,
        ];
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

    #[test]
    fn rotated_view_roundtrips_and_matches_known_points() {
        let mut v = View::centered([100, 100], [400.0, 400.0], 2.0);
        v.rotation_deg = 90.0;
        // Com 90° horário, o canto superior esquerdo do documento vai para o canto superior direito.
        assert!(close(v.doc_to_view([0.0, 0.0]), [300.0, 100.0]));
        assert!(close(v.doc_to_view([100.0, 0.0]), [300.0, 300.0]));
        for rot in [0.0, 15.0, 45.0, 135.0, 270.0] {
            v.rotation_deg = rot;
            for p in [[0.0, 0.0], [33.3, 71.1], [100.0, 100.0]] {
                assert!(close(v.view_to_doc(v.doc_to_view(p)), p), "rot {rot}");
            }
        }
    }

    #[test]
    fn rotated_zoom_pan_and_rotation_keep_expected_points_fixed() {
        let mut v = View::centered([200, 100], [600.0, 400.0], 1.5);
        v.rotation_deg = 30.0;
        let anchor = [123.0, 321.0];
        let d = v.view_to_doc(anchor);
        v.zoom_at(anchor, 2.5);
        assert!(close(v.view_to_doc(anchor), d));
        // Pan arrasta a imagem exatamente o que o usuário vê.
        let before = v.doc_to_view([10.0, 10.0]);
        v.pan([20.0, -7.0]);
        let after = v.doc_to_view([10.0, 10.0]);
        assert!(close(after, [before[0] + 20.0, before[1] - 7.0]));
        // Girar mantém o ponto sob o centro da viewport.
        let center_doc = v.view_to_doc(v.pivot);
        v.rotate_by(37.0);
        assert!(close(v.view_to_doc(v.pivot), center_doc));
        assert!((v.rotation_deg - 67.0).abs() < 1e-4);
        v.rotate_by(-100.0);
        assert!((v.rotation_deg - 327.0).abs() < 1e-3);
    }

    #[test]
    fn fit_rotated_uses_rotated_bounds() {
        let v = View::fit_rotated([100, 100], [200.0, 200.0], 0.0, 10.0, 45.0);
        // Quadrado girado 45° tem diagonal ~141,4; cabe em 200 com zoom ~1,414.
        assert!((v.zoom - 200.0 / (100.0 * 2f32.sqrt())).abs() < 1e-3);
        assert!(close(v.doc_to_view([50.0, 50.0]), [100.0, 100.0]));
    }
}
