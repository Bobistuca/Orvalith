//! Documento e árvore de camadas.
//!
//! As camadas ficam numa lista única (da mais baixa para a mais alta). Um grupo é uma entrada
//! sem pixels; seus filhos apontam para ele em `parent`. A ordem de empilhamento de filhos
//! é a ordem relativa na lista; a posição do grupo define onde o grupo entra entre os irmãos.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::blend::BlendMode;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Garante que IDs novos não colidam com um ID restaurado de arquivo.
pub(crate) fn reserve_id(id: u64) {
    NEXT_ID.fetch_max(id.saturating_add(1), Ordering::Relaxed);
}

/// Identificador estável de documento (válido durante a sessão).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(pub u64);

/// Identificador estável de camada ou grupo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerId(pub u64);

impl LayerId {
    /// Novo ID único (para camadas criadas por operações do núcleo).
    pub fn fresh() -> Self {
        LayerId(next_id())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Raster,
    Group,
}

/// Camada raster RGBA8 (sRGB, alfa não pré-multiplicado) ou grupo (sem pixels).
///
/// `pixels` é compartilhado (`Arc`): clonar uma camada é barato e a escrita copia o buffer
/// só se ele ainda estiver compartilhado (instantâneos de histórico).
#[derive(Debug, Clone)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub kind: LayerKind,
    pub parent: Option<LayerId>,
    pub visible: bool,
    pub opacity: f32,
    /// Preenchimento: multiplica o alfa dos pixels da própria camada.
    pub fill: f32,
    pub blend: BlendMode,
    pub lock_pixels: bool,
    pub lock_alpha: bool,
    pub lock_position: bool,
    /// Grupo expandido no painel.
    pub expanded: bool,
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<Vec<u8>>,
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
        Self::from_pixels(name, width, height, pixels)
    }

    /// Cria uma camada a partir de pixels RGBA8 (`width * height * 4` bytes).
    pub fn from_pixels(name: impl Into<String>, width: u32, height: u32, pixels: Vec<u8>) -> Self {
        debug_assert_eq!(pixels.len(), width as usize * height as usize * 4);
        Self {
            id: LayerId::fresh(),
            name: name.into(),
            kind: LayerKind::Raster,
            parent: None,
            visible: true,
            opacity: 1.0,
            fill: 1.0,
            blend: BlendMode::Normal,
            lock_pixels: false,
            lock_alpha: false,
            lock_position: false,
            expanded: true,
            width,
            height,
            pixels: Arc::new(pixels),
        }
    }

    /// Cria um grupo vazio (modo "passar através").
    pub fn group(name: impl Into<String>) -> Self {
        Self {
            kind: LayerKind::Group,
            blend: BlendMode::PassThrough,
            width: 0,
            height: 0,
            pixels: Arc::new(Vec::new()),
            ..Self::from_pixels(name, 0, 0, Vec::new())
        }
    }

    pub fn is_group(&self) -> bool {
        self.kind == LayerKind::Group
    }

    /// Acesso mutável aos pixels (copia o buffer se estiver compartilhado).
    pub fn pixels_mut(&mut self) -> &mut Vec<u8> {
        Arc::make_mut(&mut self.pixels)
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
            self.pixels_mut()[i..i + 4].copy_from_slice(&rgba);
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
    /// Camadas e grupos, da mais baixa para a mais alta.
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

    /// Avança a revisão sem marcar como modificado (mudança só de visualização).
    pub fn bump_view(&mut self) {
        self.revision += 1;
    }

    pub fn index_of(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Índices dos filhos diretos de `parent` (`None` = raiz), em ordem de empilhamento.
    pub fn children_of(&self, parent: Option<LayerId>) -> Vec<usize> {
        children_of(&self.layers, parent)
    }

    /// IDs de todos os descendentes de `id`, em ordem de lista.
    pub fn descendants(&self, id: LayerId) -> Vec<LayerId> {
        descendants(&self.layers, id)
    }
}

pub fn children_of(layers: &[Layer], parent: Option<LayerId>) -> Vec<usize> {
    layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.parent == parent)
        .map(|(i, _)| i)
        .collect()
}

pub fn descendants(layers: &[Layer], id: LayerId) -> Vec<LayerId> {
    let mut out = Vec::new();
    let mut frontier = vec![id];
    while let Some(p) = frontier.pop() {
        for l in layers.iter().filter(|l| l.parent == Some(p)) {
            out.push(l.id);
            frontier.push(l.id);
        }
    }
    out.sort_by_key(|d| layers.iter().position(|l| l.id == *d));
    out
}
