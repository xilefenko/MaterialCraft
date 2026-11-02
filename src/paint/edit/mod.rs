//! The paint edit API: every change to a [`Document`] as an [`Edit`] that, applied, returns its inverse.
//!
//! The GUI and the history call this one API. An edit names what it changes by ids (texture set, layer) and
//! positions, never by reference, so the inverse stays valid however many other edits came in between and
//! were undone. Applying an edit either changes the document and returns the edit that puts it back, or
//! fails with an [`EditError`] and leaves the document as it was.
//!
//! Pixel changes are tile swaps: images share their tiles copy-on-write ([`crate::paint::Tile`]), so an
//! inverse holds the old tiles of the area that changed, not a copy of the image. Painting that writes into
//! an image directly records its inverse with a [`TileCapture`].
//!
//! - [`tree`]: layer insert, remove, move and property changes;
//! - [`effects`]: content and mask effect stacks;
//! - [`pixels`]: tile swaps, region writes and [`TileCapture`];
//! - [`strokes`]: appending painted strokes to the stroke record of the pixels they painted;
//! - [`document`]: texture sets, the mesh, the height filter and the embedded assets;
//! - [`bake`]: bake configurations, link flags and baked maps;
//! - [`tiles`]: the tiles an edit or a document holds (for memory accounting);
//! - [`ids`]: fresh layer and anchor ids, and re-iding a copied layer tree;
//! - [`Transaction`]: several edits applied one after another, each planned on the result of the one
//!   before, that either all stay or are all rolled back;
//! - [`ops`]: the layer panel's operations (create, duplicate, delete, group, move, effects, masks, drops)
//!   built from edits inside a transaction.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod bake;
mod document;
mod effects;
mod error;
pub mod ids;
pub mod ops;
mod pixels;
mod strokes;
mod tiles;
mod transaction;
mod tree;

#[cfg(test)]
mod tests;

pub use error::EditError;
pub use pixels::{TileCapture, TileSwap};
pub use tiles::document_tiles;
pub use transaction::Transaction;

use super::bake_config::{BakeLinks, SetBakeConfig};
use super::doc::Document;
use super::effect::{ContentEffect, MaskEffect};
use super::embedded::EmbeddedAsset;
use super::geometry_mask::GeometryMask;
use super::height_normal::HeightFilter;
use super::image::Rect;
use super::layer::{ChannelBlend, Fill, Layer, LayerImage};
use super::lock::LayerLock;
use super::mask::Mask;
use super::stroke::{Stroke, StrokeOwner};
use super::texture_set::{BakedMap, TextureSet};
use crate::mesh::Mesh;

/// A layer by texture set id and layer id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LayerRef {
    pub set: String,
    pub layer: String,
}

impl LayerRef {
    pub fn new(set: impl Into<String>, layer: impl Into<String>) -> LayerRef {
        LayerRef { set: set.into(), layer: layer.into() }
    }
}

/// One image of a layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageRef {
    pub layer: LayerRef,
    pub image: LayerImage,
}

impl ImageRef {
    pub fn new(layer: LayerRef, image: LayerImage) -> ImageRef {
        ImageRef { layer, image }
    }
}

/// A place in a layer tree: the group the layer sits in (`None` for the texture set's top level) and its index
/// among that group's children, bottom to top. When inserting, `index` may equal the number of children.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LayerSlot {
    pub parent: Option<String>,
    pub index: usize,
}

impl LayerSlot {
    /// Position `index` at the top level of the stack.
    pub fn top(index: usize) -> LayerSlot {
        LayerSlot { parent: None, index }
    }

    /// Position `index` inside group `parent`.
    pub fn in_group(parent: impl Into<String>, index: usize) -> LayerSlot {
        LayerSlot { parent: Some(parent.into()), index }
    }
}

/// Which effect stack of a layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StackId {
    /// The content effects on top of the layer's base.
    Content,
    /// The effects of the layer's mask (the layer must have a mask).
    Mask,
}

/// An effect of either stack.
#[derive(Debug, Clone, PartialEq)]
pub enum StackEffect {
    Content(ContentEffect),
    Mask(MaskEffect),
}

impl StackEffect {
    /// The stack this effect belongs in.
    pub fn stack(&self) -> StackId {
        match self {
            StackEffect::Content(_) => StackId::Content,
            StackEffect::Mask(_) => StackId::Mask,
        }
    }
}

/// A property of a layer that [`Edit::SetLayer`] replaces as a whole.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerProperty {
    Name(String),
    Visible(bool),
    Opacity(f32),
    Blend(Vec<ChannelBlend>),
    /// Adds, replaces or (with `None`) removes the mask with its effects.
    Mask(Option<Mask>),
    /// Switches the mask on or off (the layer must have one).
    MaskEnabled(bool),
    /// The value the mask starts from, 0 (black) … 1 (white), under its effects (the layer must have a mask).
    MaskInitial(f32),
    GeometryMask(Option<GeometryMask>),
    /// The fill of a fill layer.
    Fill(Fill),
    /// Locks the layer's content against edits or (with `None`) unlocks it (see [`crate::paint::lock`]).
    Lock(Option<LayerLock>),
}

/// One reversible change to a [`Document`]. See the module docs.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// Inserts `layer` (with its children, if a group) at `at`. Its ids must be new in the texture set.
    InsertLayer {
        set: String,
        at: LayerSlot,
        layer: Box<Layer>,
    },
    RemoveLayer(LayerRef),
    /// Moves a layer to `to`; `to.index` counts the destination's children without the moved layer.
    MoveLayer {
        layer: LayerRef,
        to: LayerSlot,
    },
    SetLayer {
        layer: LayerRef,
        property: LayerProperty,
    },
    /// Inserts `effect` into its stack at `index` (bottom to top; may equal the stack's length).
    InsertEffect {
        layer: LayerRef,
        index: usize,
        effect: StackEffect,
    },
    RemoveEffect {
        layer: LayerRef,
        stack: StackId,
        index: usize,
    },
    /// Replaces the effect at `index` of `effect`'s stack (enable, label, blend, parameters).
    ReplaceEffect {
        layer: LayerRef,
        index: usize,
        effect: StackEffect,
    },
    /// Moves the effect at `from` to `to` (an index into the stack without it).
    MoveEffect {
        layer: LayerRef,
        stack: StackId,
        from: usize,
        to: usize,
    },
    /// Puts the given tiles into an image; the inverse holds the tiles that were there.
    SwapTiles {
        image: ImageRef,
        tiles: Vec<TileSwap>,
    },
    /// Writes `pixels` (row by row, `rect`'s size, inside the image) into an image.
    WritePixels {
        image: ImageRef,
        rect: Rect,
        pixels: Vec<[f32; 4]>,
    },
    /// Inserts a texture set at `index` (its id must be new).
    InsertTextureSet {
        index: usize,
        set: Box<TextureSet>,
    },
    RemoveTextureSet {
        set: String,
    },
    /// Replaces the texture set with the same id as a whole: a resolution change (every image resampled),
    /// new channels, re-baked maps.
    ReplaceTextureSet(Box<TextureSet>),
    /// Replaces the mesh (a re-import).
    SetMesh(Box<Mesh>),
    /// Replaces the document's embedded assets (AS-21; a dropped smart material embeds itself and what it uses).
    SetAssets(Vec<EmbeddedAsset>),
    SetHeightFilter(HeightFilter),
    /// Replaces the bake configuration of a texture set.
    SetBakeConfig {
        set: String,
        config: Box<SetBakeConfig>,
    },
    /// Replaces the document's bake link flags.
    SetBakeLinks(BakeLinks),
    /// Replaces the baked maps of a texture set (a bake's results).
    SetBaked {
        set: String,
        maps: Vec<BakedMap>,
    },
    /// Appends a stroke to the stroke record of a layer's paint pixels (creating the record if there is none).
    AppendStroke {
        layer: LayerRef,
        owner: StrokeOwner,
        stroke: Box<Stroke>,
    },
    /// Removes the last stroke of the record; with `drop_record`, a record left empty is removed too (it did not
    /// exist before the stroke was appended).
    RemoveLastStroke {
        layer: LayerRef,
        owner: StrokeOwner,
        drop_record: bool,
    },
}

impl Edit {
    /// Applies the edit to `doc` and returns its inverse. On error `doc` is unchanged.
    pub fn apply(self, doc: &mut Document) -> Result<Edit, EditError> {
        match self {
            Edit::InsertLayer { set, at, layer } => tree::insert(doc, &set, at, *layer),
            Edit::RemoveLayer(layer) => tree::remove(doc, layer),
            Edit::MoveLayer { layer, to } => tree::move_to(doc, layer, to),
            Edit::SetLayer { layer, property } => tree::set_property(doc, layer, property),
            Edit::InsertEffect { layer, index, effect } => effects::insert(doc, layer, index, effect),
            Edit::RemoveEffect { layer, stack, index } => effects::remove(doc, layer, stack, index),
            Edit::ReplaceEffect { layer, index, effect } => effects::replace(doc, layer, index, effect),
            Edit::MoveEffect { layer, stack, from, to } => effects::move_to(doc, layer, stack, from, to),
            Edit::SwapTiles { image, tiles } => pixels::swap(doc, image, tiles),
            Edit::WritePixels { image, rect, pixels } => pixels::write(doc, image, rect, &pixels),
            Edit::InsertTextureSet { index, set } => document::insert_set(doc, index, *set),
            Edit::RemoveTextureSet { set } => document::remove_set(doc, &set),
            Edit::ReplaceTextureSet(set) => document::replace_set(doc, *set),
            Edit::SetMesh(mesh) => Ok(Edit::SetMesh(Box::new(std::mem::replace(&mut doc.mesh, *mesh)))),
            Edit::SetAssets(assets) => Ok(Edit::SetAssets(std::mem::replace(&mut doc.assets, assets))),
            Edit::SetHeightFilter(f) => Ok(Edit::SetHeightFilter(std::mem::replace(&mut doc.height_filter, f))),
            Edit::SetBakeConfig { set, config } => bake::set_config(doc, set, *config),
            Edit::SetBakeLinks(links) => Ok(Edit::SetBakeLinks(std::mem::replace(&mut doc.bake_links, links))),
            Edit::SetBaked { set, maps } => bake::set_baked(doc, set, maps),
            Edit::AppendStroke { layer, owner, stroke } => strokes::append(doc, layer, owner, *stroke),
            Edit::RemoveLastStroke { layer, owner, drop_record } => strokes::remove_last(doc, layer, owner, drop_record),
        }
    }

    /// Folds `later`, applied right after `self`, into `self` where one edit can stand for both; returns `later`
    /// unchanged otherwise. Used on inverses: `self` is the older inverse, which wins (it restores the state
    /// before both), e.g. the first value of a slider drag or the first old tile of a stroke.
    pub fn absorb(&mut self, later: Edit) -> Option<Edit> {
        match (self, later) {
            (Edit::SetLayer { layer, property }, Edit::SetLayer { layer: l2, property: p2 }) if *layer == l2 && same_property(property, &p2) => None,
            (Edit::ReplaceEffect { layer, index, effect }, Edit::ReplaceEffect { layer: l2, index: i2, effect: e2 })
                if *layer == l2 && *index == i2 && effect.stack() == e2.stack() =>
            {
                None
            }
            (Edit::SetHeightFilter(_), Edit::SetHeightFilter(_)) | (Edit::SetBakeLinks(_), Edit::SetBakeLinks(_)) => None,
            (Edit::SetBakeConfig { set, .. }, Edit::SetBakeConfig { set: s2, config }) => {
                if *set == s2 {
                    None
                } else {
                    Some(Edit::SetBakeConfig { set: s2, config })
                }
            }
            (Edit::SwapTiles { image, tiles }, Edit::SwapTiles { image: i2, tiles: t2 }) if *image == i2 => {
                pixels::merge_older(tiles, t2);
                None
            }
            (_, later) => Some(later),
        }
    }
}

fn same_property(a: &LayerProperty, b: &LayerProperty) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}
