//! Núcleo do Orvalith: modelo de documento, camadas, histórico e composição.
//!
//! Convenções: coordenadas em pixels do documento (independentes de zoom/DPI);
//! pixels RGBA8 sRGB com alfa **não pré-multiplicado** (ver docs/ARCHITECTURE.md).

pub mod blend;
pub mod compose;
pub mod docops;
pub mod document;
pub mod edit;
pub mod fill;
pub mod fsio;
pub mod geom;
pub mod history;
pub mod imageio;
pub mod layerops;
pub mod maskops;
pub mod orva;
pub mod paint;
pub mod selection;
pub mod snap;
pub mod source;
pub mod text;
pub mod transform;
pub mod vecops;
pub mod vector;
pub mod view;

pub use blend::BlendMode;
pub use document::{
    Channel, Document, DocumentId, Guide, Layer, LayerId, LayerKind, LayerMask, Selection,
};
pub use geom::Rect;
pub use history::{Command, History};
pub use source::LayerSource;
