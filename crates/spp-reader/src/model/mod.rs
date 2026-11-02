//! Stage 3 of the import pipeline: Painter's own concepts as typed Rust, built from the `document.bin`
//! object tree. A faithful mirror (Painter's names, numbers and ordering); mapping to MaterialCraft's
//! paint model is `translate`'s job. Field meanings and evidence: `interop/painter-document-catalogue.md`.
//!
//! Entry point: [`PainterDocument::from_bytes`] → ([`PainterDocument`], [`ModelReport`]).
//! - [`baking`]: the baking settings (`/baking/baking.ini`), a separate object tree; [`cameras`]: the viewer and
//!   scene cameras (`/editor/camera.ini`), another one; [`viewer`]: the environment map of the viewer settings.
//! - [`raw`]: the generic tree ([`RawTree`]) the builder reads.
//! - [`PainterDocument`] → [`PainterTextureSet`]s → [`PainterLayer`]s (bottom → top), each with
//!   [`ChannelBlending`] entries, an optional [`PainterMask`] and a content stack of [`PainterEffect`]s.
//! - Effects ([`FillEffect`], [`PaintEffect`], filters, levels, …) take values from [`PainterSource`]s; paint
//!   effects hold their brush strokes ([`PainterStroke`]).
//! - Unknown classes become [`RawEntry`]s; [`ModelReport`] counts recognised, raw and unmirrored objects.

pub mod baking;
mod blending;
mod brush;
mod builder;
pub mod cameras;
mod channel;
mod decal;
mod document;
mod effect;
mod fill;
mod fill_projection;
mod layer;
mod mask;
mod paint;
mod procedural;
pub mod raw;
mod raw_entry;
mod report;
mod source;
mod stroke;
mod summary;
#[cfg(test)]
mod tests;
mod texture_set;
pub mod viewer;

pub use blending::{ChannelBlending, PainterBlendMode, blending_for};
pub use brush::{PainterBrush, PainterEmitter, PressureResponse, StrokeEmitter};
pub use channel::{ChannelMask, PainterChannel};
pub use decal::{DecalHit, HitGranularity, HitShape};
pub use document::PainterDocument;
pub use effect::{
    ColorSelectionEffect, EffectGroup, EffectHeader, FilterEffect, Levels, LevelsEffect, PainterEffect,
    PolygonFillEffect,
};
pub use fill::{FillEffect, UvTransform};
pub use fill_projection::{FillPlacement, GridDeformation, GridVertex, UvSizing};
pub use layer::{LayerKind, PainterLayer};
pub use mask::{MaskInitial, PainterMask};
pub use paint::{PaintEffect, PaintTool};
pub use procedural::{Procedural, ProceduralInput, ProceduralOutput, Tweak, TweakValue};
pub use raw::RawTree;
pub use raw_entry::RawEntry;
pub use report::{ClassTally, ModelReport};
pub use source::{BitmapSource, PainterSource, ProceduralSource, ReferenceSource, UniformSource};
pub use stroke::{PainterStroke, PainterView, StoredMat4, StrokeKey, TimelineKey};
pub use summary::{ContentKind, StackSummary};
pub use texture_set::{ChannelDecl, PainterTextureSet};
