//! [`PainterSource`]: where a fill, a brush or a procedural input takes its values from
//! (`DataSourceUniform`, `DataSourceBitmap`, `DataSourceProcedural`, `DataSourceReference`).

use super::channel::ChannelMask;
use super::effect::Levels;
use super::procedural::Procedural;
use super::raw_entry::RawEntry;

/// One source. `channels` says which channels it feeds (one bit per uniform value in fills; a set of
/// channels for a material `.sbsar`; 0 inside masks).
#[derive(Debug, Clone, PartialEq)]
pub enum PainterSource {
    /// A constant value. Colours are stored **linear** (Verified); grey channels repeat the value 3 times.
    Uniform(UniformSource),
    /// An image resource (`/<name>?version=<id>` or `resource://…`).
    Bitmap(BitmapSource),
    /// A Substance material, generator or brush shape (`.sbsar`) with its parameter values.
    Procedural(ProceduralSource),
    /// The content of an anchor point elsewhere in the stack.
    Reference(ReferenceSource),
    /// A class the model does not know.
    Other(RawEntry),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UniformSource {
    pub uid: Option<u64>,
    pub channels: ChannelMask,
    pub color: [f32; 3],
    pub opacity: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BitmapSource {
    pub uid: Option<u64>,
    pub channels: ChannelMask,
    /// `DataBitmap.urlToBitmapRes`; empty if the source has no bitmap.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProceduralSource {
    pub uid: Option<u64>,
    pub channels: ChannelMask,
    /// `mode` enum: 0 single output (brush shapes, grey generators), 1 material, 2 seen on generators
    /// in masks (Observed counts: 0 ×13890, 2 ×2529, 1 ×524).
    pub mode: u64,
    pub procedural: Option<Procedural>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceSource {
    pub uid: Option<u64>,
    pub channels: ChannelMask,
    /// `DataReference.source`: uid of the anchor effect (`DataActionAnchor`) this reads from.
    pub anchor_uid: Option<u64>,
    /// `DataReference.sourceLabel`, often empty.
    pub anchor_label: String,
    /// `levels`: applied to what is read.
    pub levels: Option<Levels>,
    /// `DataReference.bindings`: (`source` channel bit read from the anchor, `output` channel bit written; 0 = the
    /// mask's grey).
    pub bindings: Vec<(u64, u64)>,
    /// `mode` (2 ×430, 0 ×31, 1 ×6; meaning not known).
    pub mode: u64,
}

impl PainterSource {
    pub fn channels(&self) -> ChannelMask {
        match self {
            PainterSource::Uniform(s) => s.channels,
            PainterSource::Bitmap(s) => s.channels,
            PainterSource::Procedural(s) => s.channels,
            PainterSource::Reference(s) => s.channels,
            PainterSource::Other(_) => ChannelMask::default(),
        }
    }

    /// Short kind name for summaries ("uniform", "bitmap", …).
    pub fn kind(&self) -> &'static str {
        match self {
            PainterSource::Uniform(_) => "uniform",
            PainterSource::Bitmap(_) => "bitmap",
            PainterSource::Procedural(_) => "procedural",
            PainterSource::Reference(_) => "reference",
            PainterSource::Other(_) => "other",
        }
    }
}
