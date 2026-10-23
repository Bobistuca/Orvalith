//! Núcleo do Orvalith: modelo de documento, camadas, histórico e composição.
//!
//! Convenções: coordenadas em pixels do documento (independentes de zoom/DPI);
//! pixels RGBA8 sRGB com alfa **não pré-multiplicado** (ver docs/ARCHITECTURE.md).

pub mod compose;
pub mod document;
pub mod edit;
pub mod fill;
pub mod fsio;
pub mod geom;
pub mod history;
pub mod imageio;
pub mod orva;
pub mod paint;
pub mod view;

pub use document::{Document, DocumentId, Layer, LayerId};
pub use geom::Rect;
pub use history::{Command, History};
