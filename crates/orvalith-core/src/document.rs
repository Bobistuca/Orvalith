//! Documento e árvore de camadas (inicialmente uma lista plana de camadas raster).

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Garante que IDs novos não colidam com um ID restaurado de arquivo.
pub(crate) fn reserve_id(id: u64) {
    NEXT_ID.fetch_max(id.saturating_add(1), Ordering::Relaxed);
}

/// Identificador estável de documento (válido durante a sessão).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentId(pub u64);

/// Identificador estável de camada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerId(pub u64);

/// Camada raster RGBA8 (sRGB, alfa não pré-multiplicado).
#[derive(Debug, Clone)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Layer {
    /// Cria uma camada transparente.
    pub fn transparent(name: impl Into<String>, width: u32, height: u32) -> Self {
        Self::filled(name, width, height, [0, 0, 0, 0])
    }

    /// Cria uma camada preenchida com uma cor RGBA.
    pub fn filled(name: impl Into<String>, width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let count = width as usize * height as usize;
        let mut pixels = Vec::with_capacity(count * 4);
        for _ in 0..count {
            pixels.extend_from_slice(&rgba);
        }
        Self {
            id: LayerId(next_id()),
            name: name.into(),
            visible: true,
            opacity: 1.0,
            width,
            height,
            pixels,
        }
    }

    /// Lê o pixel em (x, y); `None` fora dos limites.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ])
    }

    /// Escreve o pixel em (x, y); ignora coordenadas fora dos limites.
    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x < self.width && y < self.height {
            let i = (y as usize * self.width as usize + x as usize) * 4;
            self.pixels[i..i + 4].copy_from_slice(&rgba);
        }
    }
}

/// Documento aberto no editor.
#[derive(Debug, Clone)]
pub struct Document {
    pub id: DocumentId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Resolução de impressão em pixels por polegada.
    pub ppi: f32,
    /// Camadas da mais baixa para a mais alta.
    pub layers: Vec<Layer>,
    /// Há alterações não salvas.
    pub modified: bool,
    /// Incrementa a cada edição; descarta resultados assíncronos obsoletos.
    pub revision: u64,
}

impl Document {
    /// Limite de lado por documento, para validar antes de alocar.
    pub const MAX_SIDE: u32 = 32_768;

    /// Cria um documento com uma camada de fundo. `background` `None` = transparente.
    pub fn new(
        name: impl Into<String>,
        width: u32,
        height: u32,
        background: Option<[u8; 4]>,
    ) -> Result<Self, String> {
        if width == 0 || height == 0 || width > Self::MAX_SIDE || height > Self::MAX_SIDE {
            return Err(format!(
                "Dimensões inválidas: {width}x{height} (máximo {} por lado)",
                Self::MAX_SIDE
            ));
        }
        let layer = match background {
            Some(c) => Layer::filled("Fundo", width, height, c),
            None => Layer::transparent("Camada 1", width, height),
        };
        Ok(Self {
            id: DocumentId(next_id()),
            name: name.into(),
            width,
            height,
            ppi: 72.0,
            layers: vec![layer],
            modified: false,
            revision: 0,
        })
    }

    /// Monta um documento a partir de partes já validadas (abertura de arquivo).
    pub fn from_parts(name: String, width: u32, height: u32, ppi: f32, layers: Vec<Layer>) -> Self {
        for l in &layers {
            reserve_id(l.id.0);
        }
        Self {
            id: DocumentId(next_id()),
            name,
            width,
            height,
            ppi,
            layers,
            modified: false,
            revision: 0,
        }
    }

    /// Registra uma edição: marca como modificado e avança a revisão.
    pub fn touch(&mut self) {
        self.modified = true;
        self.revision += 1;
    }
}
