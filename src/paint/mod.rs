//! Paint documents: layered, per-channel textures on a mesh (Paint mode). See the internal docs,
//! `architecture/paint-mode.md`.
//!
//! - [`pixel`]: sample types and pixel formats.
//! - [`image`]: [`TiledImage`], lazily allocated tiles with dirty tracking.
//! - [`channel`]: material channels, colour spaces, [`ChannelSpec`].
//! - [`blend`] and [`normal`]: blend modes and normal-vector maths.
//! - [`layer`], [`effect`], [`mask`], [`geometry_mask`], [`texture_set`], [`doc`], [`report`]: the document
//!   model; [`validate`] its invariants.
//! - [`instance`]: layer instances, resolved into their texture set at composite time; which sets an edit reaches.
//! - [`embedded`]: library assets a document embeds (AS-21); [`fragment`]: smart materials and smart masks.
//! - [`edit`]: [`edit::Edit`], every document change with its inverse (for the GUI and the history).
//! - [`bake_config`]: per texture set bake settings with link, copy and check commands.
//! - [`stroke`]: stroke records, the strokes behind painted pixels; [`brush`]: brush presets (our format).
//! - [`see_through`]: see-through paint content, whose strokes work on the composite below it.
//! - [`stack`]: [`Compositor`], layer stack → channel; [`layer_preview`]: layer thumbnails on a reduced grid.
//! - [`probe`]: [`probe::TexelProbe`], the composited channel values under a point of the mesh (eyedropper, material picker).
//! - [`diag_entries`]: import summaries and compositing warnings as session log entries.
//! - [`working_space`]: the space layers blend in (linear light for sRGB channels).
//! - [`output`]: [`MaterialMaps`], final PBR maps for the viewer and export.
//! - [`new_project`]: a new document from a mesh file and a template (File > New); [`project_settings`]: the
//!   choices it records (normal map format, tangent frame, fill sizing).

pub mod bake_config;
pub mod blend;
pub mod brush;
pub mod camera_import;
pub mod camera_list;
pub mod channel;
pub mod diag_entries;
pub mod doc;
pub mod edit;
pub mod effect;
pub mod embedded;
pub mod fill_image;
pub mod fill_texture;
pub mod format;
pub mod fragment;
pub mod geometry_mask;
pub mod height_normal;
pub mod image;
pub mod instance;
pub mod layer;
pub mod layer_preview;
pub mod lock;
pub mod mask;
pub mod new_project;
pub mod normal;
pub mod normal_combine;
pub mod output;
pub mod output_tiles;
pub(crate) mod par;
pub mod pixel;
pub mod probe;
pub mod project_settings;
pub mod report;
pub mod saved_camera;
pub mod saved_view;
pub mod see_through;
pub mod set_changes;
pub mod stack;
pub mod stroke;
#[cfg(test)]
pub(crate) mod test_stacks;
pub mod texture_set;
pub mod validate;
pub mod working_space;

pub use blend::BlendMode;
pub use channel::{Channel, ChannelSpec, ColorSpace};
pub use doc::{Document, ForeignFile};
pub use effect::{ContentEffect, Effect, EffectKind, EffectRegistry, GreyBlend, MaskEffect, MaskPixels};
pub use embedded::EmbeddedAsset;
pub use fill_image::{FillImage, FillImageRef, ImageFill};
pub use fill_texture::TextureFill;
pub use fragment::{Fragment, FragmentContent, FragmentKind};
pub use geometry_mask::{GeometryMask, Inclusion};
pub use image::{ImageError, Rect, TILE_SIZE, Tile, TiledImage};
pub use instance::InstanceLink;
pub use layer::{ChannelBlend, Fill, FillSource, Layer, LayerImage, LayerKind, PaintContent};
pub use lock::LayerLock;
pub use mask::Mask;
pub use output::{MaterialMaps, OutputOptions};
pub use pixel::{PixelFormat, PixelLayout, SampleType};
pub use project_settings::{NormalMapFormat, ProjectSettings};
pub use report::{Fidelity, ImportReport, ReportEntry};
pub use stack::{ChainedSources, CompositeWarning, Compositor, FillContext, FillRequest, FillSources, NO_FILL_CONTEXT, NoFillSources};
pub use texture_set::{BakedKind, BakedMap, TextureSet};
pub use validate::DocError;
