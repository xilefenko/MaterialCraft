//! Layer import: a Painter texture set's layer stack ([`crate::model::PainterTextureSet`]) as our
//! layers ([`materialcraft::paint::Layer`]), and how exactly they reproduce Painter.
//!
//! - [`blend_table`]: the one mapping of Painter's blend values to ours, and Painter's defaults.
//! - [`blending`]: the blend mode and opacity a channel gets from a stored list.
//! - [`channels`]: declared channels on both sides; Painter's linear values in our encoding.
//! - [`content`] and [`layer_content`]: a layer's effect stack → per-channel values (what is computable).
//! - [`effect_map`] and [`effect_data`]: Painter's effects → our base content and content effects, one to one.
//! - [`mask`]: Painter masks → our mask stacks and their computability; [`images`]: bitmap resources.
//! - [`stack_builder`]: the tree walk; [`verdict`] and [`computability`]: what is exact, per channel/layer.
//! - [`cache_score`] and [`cache_agreement`]: a fully computable stack measured against Painter's cache, on
//!   the texels of the mesh ([`island_texels`]).
//! - [`strategy`]: the per-set decision between the real stack and Painter's flattened result.
//! - [`correction`]: Painter's flattened result only on the texels the imported layers don't reproduce.
//! - [`strokes`]: Painter's brush strokes → stroke records next to the paint pixels.
//! - [`replay`]: those strokes painted again by our brush pipeline (painted masks become real paint).
//! - [`decal`]: Painter's polygon-fill hits resolved on the set's mesh into picked triangles.
//! - [`set_surface`]: the set's surface map and scene frame, for mask bitmaps projected from a volume.
//! - [`anchors`]: how exactly what each anchor holds is computable, for the effects that read it.
//! - [`report`]: the import report lines.

pub mod anchors;
pub mod blend_table;
pub mod blending;
pub mod cache_agreement;
pub mod cache_score;
pub mod channels;
pub mod computability;
pub mod content;
pub mod correction;
pub mod decal;
pub mod effect_data;
pub mod effect_map;
pub mod images;
pub mod island_texels;
pub mod layer_content;
pub mod mask;
pub mod projection_map;
pub mod replay;
pub mod report;
pub mod set_surface;
pub mod stack_builder;
pub mod strategy;
pub mod strokes;
#[cfg(test)]
mod tests;
pub mod verdict;

pub use blend_table::{BlendTable, Confidence};
pub use channels::SetChannels;
pub use computability::Computability;
pub use images::{ImageSource, NoImages, ProjectImages};
pub use projection_map::{HardnessCurve, ProjectionTable, VolumeConvention};
pub use set_surface::{LazySurface, SurfaceSource};
pub use stack_builder::ImportedStack;
pub use strategy::{PAINTER_RESULT_NAME, REFERENCE_NAME, StackParts, StackStrategy};
pub use verdict::StackVerdict;

use crate::model::PainterTextureSet;
use crate::translate::replacements::ProceduralPlan;
use anchors::AnchorTable;
use decal::geometry::SetGeometry;
use replay::{ReplayCache, ReplayChoice, StrokeReplay};
use stack_builder::StackBuilder;

/// Imports layer stacks with one blend table, one image source, the user's choice of procedural
/// replacements and whether strokes are replayed.
pub struct LayerImport<'a> {
    pub table: &'a BlendTable,
    /// How Painter's projection settings map to ours.
    pub projections: &'a ProjectionTable,
    pub images: &'a dyn ImageSource,
    pub plan: &'a ProceduralPlan,
    /// Replay painted masks and paint layers (needs the mesh: [`Self::import_on`] with geometry).
    pub replay: ReplayChoice,
    /// Replays kept across imports of the same set (else each import replays on its own).
    pub replay_cache: Option<&'a ReplayCache>,
}

impl LayerImport<'_> {
    /// The set's layers for the declared `channels` at `resolution`, without the mesh (polygon fills are then
    /// not computable).
    pub fn import(&self, set: &PainterTextureSet, channels: &SetChannels, resolution: (u32, u32)) -> ImportedStack {
        self.import_on(set, channels, resolution, None, None)
    }

    /// [`Self::import`] with the set's mesh, which polygon-fill picks are resolved on, and its surface at
    /// `resolution`, which mask bitmaps in a volume projection are laid on over (else in texture space). Strokes
    /// are replayed on the mesh.
    pub fn import_on(
        &self,
        set: &PainterTextureSet,
        channels: &SetChannels,
        resolution: (u32, u32),
        geometry: Option<&SetGeometry>,
        surface: Option<&dyn SurfaceSource>,
    ) -> ImportedStack {
        let replay = match (geometry, self.replay) {
            (Some(g), ReplayChoice::Replay) => Some(StrokeReplay::new(g, self.images, resolution, self.replay_cache)),
            _ => None,
        };
        if let Some(r) = &replay {
            r.prefetch(set, channels);
        }
        let builder = |anchors| StackBuilder {
            channels,
            table: self.table,
            projections: self.projections,
            images: self.images,
            plan: self.plan,
            resolution,
            geometry,
            surface,
            replay: replay.as_ref(),
            anchors,
        };
        // anchors read through other anchors need a second pass
        let none = AnchorTable::default();
        let first = builder(&none).anchor_table(&set.layers);
        let anchors = builder(&first).anchor_table(&set.layers);
        builder(&anchors).build(&set.layers)
    }
}
