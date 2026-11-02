//! Stage 4 of the import pipeline: an opened `.spp` → [`materialcraft::paint::Document`] with an import report.
//! The only part of the crate that knows both Painter's names and ours; the mapping tables live in
//! [`channel_table`], [`source_format`], [`normal_conventions`], [`bake_settings`] and [`replacements`]
//! (Substance procedurals → MaterialCraft's generators).
//!
//! Each texture set gets Painter's layer stack ([`layers`], M5) and the flattened channels from the texture
//! cache (M3) as one paint layer, either on top (when not every layer is computable) or as a hidden reference,
//! plus its baked mesh maps; everything not translated is kept under `foreign/substance/`.
//!
//! Beside the import, [`library`] lists what the project brings for the user's own asset library (brush presets,
//! alphas, stencils, textures, the environment map) and writes the ones the import review ticks.

pub mod bake_settings;
mod baked_maps;
pub mod blocker_summary;
pub(crate) mod cache_channels;
#[cfg(test)]
mod camera_tests;
pub mod cameras;
mod cancel;
pub mod catalog;
pub mod channel_table;
mod error;
mod foreign;
mod importer;
pub mod layers;
pub mod library;
pub mod mesh;
pub mod normal_conventions;
pub mod origin;
mod pixel_convert;
pub mod replacements;
mod report;
mod resources;
mod set_builder;
pub(crate) mod set_layers;
pub mod source_format;
mod texture_sets;

pub use baked_maps::{BakedImage, BakedMaps, BakedSource};
pub use cancel::Cancel;
pub use error::{ImportError, PartError};
pub use foreign::FOREIGN_ROOT;
pub use importer::{Importer, import_spp, import_spp_cancellable, import_spp_planned, import_spp_with};
pub use layers::{PAINTER_RESULT_NAME, REFERENCE_NAME};
pub use origin::{ImportOptions, PainterOrigin};
pub use set_layers::{DEFAULTS_LAYER_ID, DEFAULTS_LAYER_NAME, PAINTER_RESULT_ID};
pub use texture_sets::{SetBinding, SetSource};
