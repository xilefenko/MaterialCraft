//! [`PainterLayer`]: one entry of a layer stack (`DataStackLayers.items`): a layer (`DataLayerColor`) or a
//! folder (`DataLayerGroup`). Stacks are stored **bottom → top** (Verified: a full, unmasked uniform fill as
//! the last item is the final colour of `AMULET 2.spp`'s sets; layer numbers grow along the list).

use super::blending::ChannelBlending;
use super::effect::PainterEffect;
use super::mask::PainterMask;

#[derive(Debug, Clone, PartialEq)]
pub struct PainterLayer {
    pub uid: Option<u64>,
    /// `label`, the name shown in Painter.
    pub name: String,
    pub kind: LayerKind,
    /// `enabled`: the eye toggle.
    pub visible: bool,
    /// `perChannelBlending`: blend mode and opacity per channel, as stored. Painter has no separate
    /// layer opacity; the opacity slider writes these entries.
    pub blending: Vec<ChannelBlending>,
    /// `maskActions`; `None` if the layer has no mask.
    pub mask: Option<PainterMask>,
    /// `actions`: the layer's content as an effect stack, bottom → top (empty for most folders).
    pub content: Vec<PainterEffect>,
}

/// What kind of layer this is, as Painter's UI shows it.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerKind {
    /// A `DataLayerColor` whose content starts with a fill effect.
    Fill,
    /// A `DataLayerColor` whose content starts with an effect group (Painter's container for strokes),
    /// or with nothing.
    Paint,
    /// A `DataLayerGroup` (folder) with its own stack, bottom → top.
    Group(Vec<PainterLayer>),
    /// A layer class the model does not know (e.g. one from a newer Painter), by class name.
    Other(String),
}

impl LayerKind {
    pub fn name(&self) -> &str {
        match self {
            LayerKind::Fill => "fill",
            LayerKind::Paint => "paint",
            LayerKind::Group(_) => "folder",
            LayerKind::Other(class) => class,
        }
    }
}

impl PainterLayer {
    /// Child layers of a folder (empty otherwise).
    pub fn children(&self) -> &[PainterLayer] {
        match &self.kind {
            LayerKind::Group(children) => children,
            _ => &[],
        }
    }

    /// This layer and every layer below it in folders, depth first, top-level order kept.
    pub fn walk(&self) -> Vec<&PainterLayer> {
        let mut out = Vec::new();
        let mut todo = vec![self];
        while let Some(layer) = todo.pop() {
            out.push(layer);
            todo.extend(layer.children().iter().rev());
        }
        out
    }
}
