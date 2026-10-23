//! Núcleo do Orvalith: modelo de documento, camadas, histórico e composição.
//!
//! Convenções: coordenadas em pixels do documento (independentes de zoom/DPI);
//! pixels RGBA8 sRGB com alfa **não pré-multiplicado** (ver docs/ARCHITECTURE.md).

pub mod blend;
pub mod compose;
pub mod document;
pub mod edit;
pub mod fill;
pub mod fsio;
pub mod geom;
pub mod history;
pub mod imageio;
pub mod layerops;
pub mod orva;
pub mod paint;
pub mod selection;
pub mod view;

pub use blend::BlendMode;
pub use document::{Channel, Document, DocumentId, Layer, LayerId, LayerKind, Selection};
pub use geom::Rect;
pub use history::{Command, History};
