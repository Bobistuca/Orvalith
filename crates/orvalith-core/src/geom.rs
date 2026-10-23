//! Geometria de pixels do documento.

/// Retângulo de pixels inteiro (origem no canto superior esquerdo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub fn full(w: u32, h: u32) -> Self {
        Self::new(0, 0, w, h)
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    pub fn right(&self) -> u32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }

    pub fn union(self, o: Rect) -> Rect {
        if self.is_empty() {
            return o;
        }
        if o.is_empty() {
            return self;
        }
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(
            x,
            y,
            self.right().max(o.right()) - x,
            self.bottom().max(o.bottom()) - y,
        )
    }

    /// Recorta o retângulo aos limites `w × h`.
    pub fn clamp_to(self, w: u32, h: u32) -> Rect {
        let x = self.x.min(w);
        let y = self.y.min(h);
        Rect::new(x, y, self.right().min(w) - x, self.bottom().min(h) - y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_and_clamp() {
        let a = Rect::new(2, 2, 3, 3);
        let b = Rect::new(10, 1, 2, 2);
        assert_eq!(a.union(b), Rect::new(2, 1, 10, 4));
        assert_eq!(a.union(Rect::new(0, 0, 0, 0)), a);
        assert_eq!(
            Rect::new(8, 8, 10, 10).clamp_to(10, 12),
            Rect::new(8, 8, 2, 4)
        );
        assert!(Rect::new(20, 20, 5, 5).clamp_to(10, 10).is_empty());
    }
}
