//! Núcleo do Orvalith: modelo de documento, camadas, histórico e composição.
//!
//! Convenções: coordenadas em pixels do documento (independentes de zoom/DPI);
//! pixels RGBA8 sRGB com alfa **não pré-multiplicado** (ver docs/ARCHITECTURE.md).

pub mod actions;
pub mod adjops;
pub mod adjust;
pub mod anim;
pub mod blend;
pub mod color;
pub mod compose;
pub mod comps;
pub mod docops;
pub mod document;
pub mod edit;
pub mod examples;
pub mod fill;
pub mod filter;
pub mod formats;
pub mod fsio;
pub mod geom;
pub mod histogram;
pub mod history;
pub mod imageio;
pub mod layerops;
pub mod liquify;
pub mod maskops;
pub mod media;
pub mod object;
pub mod orva;
pub mod paint;
pub mod pcraft;
pub mod pdf;
pub mod psd;
pub mod resources;
pub mod retouch;
#[cfg(test)]
mod robustness;
pub mod selection;
pub mod snap;
pub mod source;
pub mod style;
pub mod text;
pub mod transform;
pub mod vecops;
pub mod vector;
pub mod video;
pub mod view;

pub use adjust::Adjustment;
pub use blend::BlendMode;
pub use color::IccProfile;
pub use document::{
    Channel, Document, DocumentId, Guide, Layer, LayerId, LayerKind, LayerMask, Selection,
};
pub use geom::Rect;
pub use history::{Command, History};
pub use source::LayerSource;
