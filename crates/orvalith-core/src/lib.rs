//! Núcleo do Orvalith: modelo de documento, camadas, histórico e composição.
//!
//! Convenções: coordenadas em pixels do documento (independentes de zoom/DPI);
//! pixels RGBA8 sRGB com alfa **não pré-multiplicado** (ver docs/ARCHITECTURE.md).

pub mod compose;
pub mod document;
pub mod history;
pub mod view;

pub use document::{Document, DocumentId, Layer, LayerId};
pub use history::{Command, History};
