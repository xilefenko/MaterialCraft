//! Texture-space stamp application (ADR 0006 Q-PT-10 pull, Q-PT-11 CPU first): brush dabs made in a flat frame
//! (the screen for 3D-view strokes, texels for 2D-view strokes) land in a texture set's images by pulling every
//! texel near a dab into the dab's frame and sampling its coverage there.
//!
//! Pulling through the surface map gives, from one mechanism: both sides of a UV seam get paint (they are texels
//! near the same surface point), gutter texels get paint (the surface map extends the surface into them),
//! occlusion and facing tests per texel, symmetry (the texel's position transformed first), and stencils and quick
//! masks as per-texel factors.
//!
//! - [`DabSample`], [`Footprint`]: a dab and its coverage grid, as the brush engine hands them over (the core never
//!   sees an engine type).
//! - [`TexelIndex`]: cell bounds of a surface map, to find texels near a dab.
//! - [`DepthMap`]: the view's depth on the CPU, drawn once per stroke, for the occlusion test.
//! - [`StampStroke`]: one stroke into a set of [`ChannelTarget`]s: placement modes (screen, wrapped, flat, UV),
//!   occlusion, facing limit, symmetry ([`SymmetrySpec`], from [`crate::symmetry`], for 3D-view and 2D-view
//!   strokes alike), factors ([`TexelFactor`]: [`QuickMask`], [`Stencil`]),
//!   flow building up under the stroke's opacity ceiling, [`StampMode::Paint`] / [`StampMode::Erase`] /
//!   [`StampMode::Clone`], or [`StampMode::Local`] kernels ([`LocalKernel`], [`BlurKernel`]) for smudge and blur; it reports
//!   the image tiles each dab changed (the caller records them in the history).
//! - [`ProjectedPaint`]: projection painting (PJ-26), an image or material over the view painted through a stroke:
//!   a per-texel value for each [`ChannelTarget`] ([`TexelValue`]) and a factor that keeps paint inside the image.
//!
//! Design: `MaterialCraft-docs/requirements/painting-tools.md` section 10.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(test)]
mod batch_tests;
mod dab;
mod factors;
mod frame;
mod index;
mod local;
mod occlusion;
mod plan;
mod projected;
#[cfg(test)]
mod projected_tests;
#[cfg(test)]
mod retouch_tests;
mod stroke;
#[cfg(test)]
mod symmetry_tests;
#[cfg(test)]
mod tests;
mod through_mesh;
mod write;

pub use crate::symmetry::{CopyTransform, SymmetrySpec};
pub use dab::{DabSample, Footprint};
pub use factors::{QuickMask, Stencil, TexelFactor};
pub use frame::screen_scale_at;
pub use index::{Aabb, CELL, TexelIndex};
pub use local::{BlurKernel, LocalGrid, LocalKernel};
pub use occlusion::DepthMap;
pub use projected::{ProjectedChannel, ProjectedPaint};
pub use stroke::{StampContext, StampError, StampStroke, StrokeSpace, ViewSpace};
pub use write::{ChannelTarget, StampMode, TexelValue};
