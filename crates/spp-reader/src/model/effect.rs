//! [`PainterEffect`]: one entry of an effect stack (`DataStackActions.items`). A layer's content and its
//! mask are both such stacks, listed bottom → top like the layers themselves.

use super::blending::ChannelBlending;
use super::channel::{ChannelMask, PainterChannel};
use super::decal::DecalHit;
use super::fill::FillEffect;
use super::paint::PaintEffect;
use super::procedural::Procedural;
use super::raw_entry::RawEntry;

#[derive(Debug, Clone, PartialEq)]
pub enum PainterEffect {
    /// `DataActionFill`: fill or generator (sources per channel, projection).
    Fill(FillEffect),
    /// `DataActionPaint`, `…PaintProj`, `…Eraser`, `…Smudge`: brush strokes.
    Paint(PaintEffect),
    /// `DataActionGroup`: a nested effect stack. A paint layer's strokes live in one (blend mode
    /// passthrough), so do the strokes of a painted mask.
    Group(EffectGroup),
    /// `DataActionFilterProcedural`: a filter `.sbsar` applied to what is below.
    Filter(FilterEffect),
    /// `DataActionFilterLevels`.
    Levels(LevelsEffect),
    /// `DataActionFilterColorSelection`: a mask from the colours of a bitmap (ID map).
    ColorSelection(ColorSelectionEffect),
    /// `DataActionAnchor`: an anchor point other effects can reference by uid.
    Anchor(EffectHeader),
    /// `DataActionDecal`: polygon fill (picked triangles/polygons/meshes/UV islands, Inferred).
    PolygonFill(PolygonFillEffect),
    /// A class the model does not know.
    Other(RawEntry),
}

/// Fields every effect class has.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EffectHeader {
    pub uid: Option<u64>,
    pub enabled: bool,
    /// `label`; empty except for anchors and a handful of effects.
    pub label: String,
    /// `channelTypes`: the channels the effect writes (0 in masks).
    pub channels: ChannelMask,
    /// `perChannelBlending` as stored (may be empty).
    pub blending: Vec<ChannelBlending>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectGroup {
    pub header: EffectHeader,
    pub effects: Vec<PainterEffect>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilterEffect {
    pub header: EffectHeader,
    pub procedural: Option<Procedural>,
    /// `inputs`: the filter's image inputs and the channel each reads (`input` for most filters).
    pub inputs: Vec<(String, PainterChannel)>,
}

/// Levels (`DataLevels`), per RGB component.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Levels {
    pub input_minimum: [f32; 3],
    pub input_maximum: [f32; 3],
    pub gamma: [f32; 3],
    pub output_minimum: [f32; 3],
    pub output_maximum: [f32; 3],
    /// `clamp` enum (0 ×811, 1 ×265 in the corpus).
    pub clamp: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LevelsEffect {
    pub header: EffectHeader,
    pub levels: Option<Levels>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColorSelectionEffect {
    pub header: EffectHeader,
    pub colors: Vec<[f32; 3]>,
    pub tolerance: f32,
    pub hardness: f32,
    /// The bitmap the colours are picked from (usually the baked ID map).
    pub bitmap_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PolygonFillEffect {
    pub header: EffectHeader,
    /// The picks (`DataDecalHitRect` / `DataDecalHitPunctual`), in order: later ones paint over earlier ones.
    pub hits: Vec<DecalHit>,
    /// Hits that could not be read (unknown class or missing position).
    pub unread: usize,
}

impl PainterEffect {
    /// The common fields, if the class is known.
    pub fn header(&self) -> Option<&EffectHeader> {
        Some(match self {
            PainterEffect::Fill(e) => &e.header,
            PainterEffect::Paint(e) => &e.header,
            PainterEffect::Group(e) => &e.header,
            PainterEffect::Filter(e) => &e.header,
            PainterEffect::Levels(e) => &e.header,
            PainterEffect::ColorSelection(e) => &e.header,
            PainterEffect::Anchor(h) => h,
            PainterEffect::PolygonFill(e) => &e.header,
            PainterEffect::Other(_) => return None,
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.header().is_some_and(|h| h.enabled)
    }

    /// Short kind name for summaries.
    pub fn kind(&self) -> &'static str {
        match self {
            PainterEffect::Fill(_) => "fill",
            PainterEffect::Paint(_) => "paint",
            PainterEffect::Group(_) => "group",
            PainterEffect::Filter(_) => "filter",
            PainterEffect::Levels(_) => "levels",
            PainterEffect::ColorSelection(_) => "color selection",
            PainterEffect::Anchor(_) => "anchor",
            PainterEffect::PolygonFill(_) => "polygon fill",
            PainterEffect::Other(_) => "other",
        }
    }
}
