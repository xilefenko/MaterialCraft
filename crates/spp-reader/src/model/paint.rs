//! [`PaintEffect`]: brush work (`DataActionPaint` and its projection, eraser and smudge variants) with its
//! strokes ([`PainterStroke`]). 3D path strokes (`DataStroke3D`, format 78) are only counted.

use super::brush::PainterBrush;
use super::effect::EffectHeader;
use super::source::PainterSource;
use super::stroke::PainterStroke;

/// Which tool made the strokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintTool {
    /// `DataActionPaint`
    Brush,
    /// `DataActionPaintProj`
    Projection,
    /// `DataActionEraser`
    Eraser,
    /// `DataActionSmudge`
    Smudge,
}

impl PaintTool {
    pub fn from_class(class: &str) -> Option<PaintTool> {
        Some(match class {
            "DataActionPaint" => PaintTool::Brush,
            "DataActionPaintProj" => PaintTool::Projection,
            "DataActionEraser" => PaintTool::Eraser,
            "DataActionSmudge" => PaintTool::Smudge,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            PaintTool::Brush => "brush",
            PaintTool::Projection => "projection",
            PaintTool::Eraser => "eraser",
            PaintTool::Smudge => "smudge",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaintEffect {
    pub header: EffectHeader,
    pub tool: PaintTool,
    /// Tool opacity (`opacity`); 1 if missing.
    pub opacity: f32,
    /// `strokes`: the `DataStroke`s, in painting order.
    pub strokes: Vec<PainterStroke>,
    /// Number of `strokes3D` (`DataStroke3D`, path strokes on the mesh); not mirrored.
    pub path_strokes: usize,
    /// `brush`: uid of the `DataBrush` (size, flow, spacing, jitter, emitter) the strokes were painted with.
    pub brush_uid: Option<u64>,
    /// `brush`: its settings, if the effect has one.
    pub brush: Option<PainterBrush>,
    /// `stampBlendingMode` (0 ×12126, 1 ×688; absent in old files): how stamps of one stroke combine.
    pub stamp_blending: u64,
    /// `sourceColor`: what the brush paints, per channel (uniform colours are linear).
    pub colors: Vec<PainterSource>,
    /// `sourceTransparent`: the brush alpha (usually the `shape.sbsar` procedural or a bitmap).
    pub alpha: Vec<PainterSource>,
    /// `sourceStencil`, if a stencil was used.
    pub stencil: Option<PainterSource>,
}

impl PaintEffect {
    /// Strokes of both kinds.
    pub fn stroke_count(&self) -> usize {
        self.strokes.len() + self.path_strokes
    }
}
