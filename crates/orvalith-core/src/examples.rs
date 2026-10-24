//! Exemplos originais gerados por código (sem imagens de terceiros): miniatura, pôster,
//! retoque e animação. Servem de ponto de partida e de material para a ajuda local.

use crate::anim::{Ease, Prop, Timeline};
use crate::document::{Document, Layer};

/// Nome e descrição de cada exemplo, na ordem de [`build`].
pub const EXAMPLES: [(&str, &str); 4] = [
    (
        "Miniatura de vídeo",
        "1280×720 com fundo em degradê e camadas separadas para editar.",
    ),
    (
        "Pôster",
        "600×900 com faixas de cor e formas sobrepostas em camadas.",
    ),
    (
        "Retoque",
        "Fundo liso com manchas: pratique Carimbo, Pincel de correção e Remendo.",
    ),
    (
        "Animação",
        "Uma bola que desliza e aparece, com quadros-chave na linha do tempo.",
    ),
];

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

fn vertical_gradient(l: &mut Layer, w: u32, h: u32, top: [u8; 3], bottom: [u8; 3]) {
    for y in 0..h {
        let t = y as f32 / (h.max(2) - 1) as f32;
        let c = [
            lerp(top[0], bottom[0], t),
            lerp(top[1], bottom[1], t),
            lerp(top[2], bottom[2], t),
            255,
        ];
        for x in 0..w {
            l.set_pixel(x, y, c);
        }
    }
}

fn rect(l: &mut Layer, x0: u32, y0: u32, w: u32, h: u32, c: [u8; 4]) {
    for y in y0..(y0 + h).min(l.height) {
        for x in x0..(x0 + w).min(l.width) {
            l.set_pixel(x, y, c);
        }
    }
}

fn disc(l: &mut Layer, cx: i32, cy: i32, r: i32, c: [u8; 4]) {
    for y in (cy - r).max(0)..=(cy + r).min(l.height as i32 - 1) {
        for x in (cx - r).max(0)..=(cx + r).min(l.width as i32 - 1) {
            if (x - cx).pow(2) + (y - cy).pow(2) <= r * r {
                l.set_pixel(x as u32, y as u32, c);
            }
        }
    }
}

/// Constrói o exemplo `index` (0..4). Índice inválido é erro.
pub fn build(index: usize) -> Result<Document, String> {
    let (name, _) = *EXAMPLES.get(index).ok_or("Exemplo inexistente")?;
    match index {
        0 => thumbnail(name),
        1 => poster(name),
        2 => retouch(name),
        _ => animation(name),
    }
}

fn thumbnail(name: &str) -> Result<Document, String> {
    let (w, h) = (1280, 720);
    let mut d = Document::new(name, w, h, Some([20, 24, 40, 255]))?;
    vertical_gradient(&mut d.layers[0], w, h, [30, 60, 140], [10, 10, 30]);
    let mut band = Layer::transparent("Faixa", w, h);
    rect(&mut band, 0, 520, w, 140, [0, 0, 0, 170]);
    let mut mark = Layer::transparent("Destaque", w, h);
    disc(&mut mark, 1050, 260, 150, [255, 200, 40, 255]);
    disc(&mut mark, 1050, 260, 100, [255, 120, 20, 255]);
    d.layers.push(mark);
    d.layers.push(band);
    Ok(d)
}

fn poster(name: &str) -> Result<Document, String> {
    let (w, h) = (600, 900);
    let mut d = Document::new(name, w, h, Some([245, 238, 225, 255]))?;
    let mut stripes = Layer::transparent("Faixas", w, h);
    for (i, c) in [[200, 60, 50], [230, 160, 40], [40, 120, 160]]
        .into_iter()
        .enumerate()
    {
        rect(
            &mut stripes,
            0,
            80 + i as u32 * 70,
            w,
            40,
            [c[0], c[1], c[2], 255],
        );
    }
    let mut shapes = Layer::transparent("Formas", w, h);
    disc(&mut shapes, 300, 560, 180, [30, 50, 90, 255]);
    disc(&mut shapes, 300, 560, 110, [245, 238, 225, 255]);
    rect(&mut shapes, 60, 800, 480, 12, [30, 50, 90, 255]);
    d.layers.push(stripes);
    d.layers.push(shapes);
    d.ppi = 150.0;
    Ok(d)
}

fn retouch(name: &str) -> Result<Document, String> {
    let (w, h) = (640, 480);
    let mut d = Document::new(name, w, h, Some([200, 160, 140, 255]))?;
    vertical_gradient(&mut d.layers[0], w, h, [214, 176, 156], [184, 142, 122]);
    // Manchas a remover.
    for (x, y, r) in [(180, 160, 9), (300, 250, 6), (450, 330, 12), (240, 380, 5)] {
        disc(&mut d.layers[0], x, y, r, [110, 60, 50, 255]);
    }
    Ok(d)
}

fn animation(name: &str) -> Result<Document, String> {
    let (w, h) = (320, 180);
    let mut d = Document::new(name, w, h, Some([24, 32, 56, 255]))?;
    let mut ball = Layer::transparent("Bola", w, h);
    disc(&mut ball, 40, 90, 24, [255, 120, 60, 255]);
    let id = ball.id.0;
    d.layers.push(ball);
    let mut tl = Timeline {
        fps: 12.0,
        length: 24,
        ..Timeline::default()
    };
    tl.set_key(id, Prop::X, 0, 0.0, Some(Ease::EaseInOut));
    tl.set_key(id, Prop::X, 23, 220.0, None);
    tl.set_key(id, Prop::Opacity, 0, 0.0, Some(Ease::Linear));
    tl.set_key(id, Prop::Opacity, 6, 1.0, None);
    d.timeline = Some(tl);
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_example_builds_with_expected_content() {
        for (i, ex) in EXAMPLES.iter().enumerate() {
            let d = build(i).unwrap();
            assert_eq!(d.name, ex.0);
            assert!(!d.layers.is_empty());
            assert!(!d.modified, "exemplo recém-criado não está modificado");
        }
        assert_eq!(build(0).unwrap().width, 1280);
        assert!(build(0).unwrap().layers.len() >= 3);
        assert_eq!(build(1).unwrap().ppi, 150.0);
        assert!(build(9).is_err());
    }

    #[test]
    fn retouch_example_has_blemishes_and_animation_has_keys() {
        let d = build(2).unwrap();
        let p = d.layers[0].pixel(180, 160).unwrap();
        assert!(p[0] < 140, "mancha escura presente");
        let a = build(3).unwrap();
        let tl = a.timeline.as_ref().unwrap();
        assert_eq!(tl.tracks.len(), 2);
        let id = a.layers[1].id.0;
        assert_eq!(tl.value_at(id, Prop::X, 23), Some(220.0));
    }
}
