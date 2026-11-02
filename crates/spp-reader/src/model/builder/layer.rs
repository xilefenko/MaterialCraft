//! `DataStackLayers` → [`PainterLayer`]s: layers, folders, per-channel blending and masks.

use super::{MAX_NESTING, ModelBuilder};
use crate::model::blending::{ChannelBlending, PainterBlendMode};
use crate::model::channel::ChannelMask;
use crate::model::effect::PainterEffect;
use crate::model::layer::{LayerKind, PainterLayer};
use crate::model::mask::{MaskInitial, PainterMask};
use crate::model::raw::RawObject;

impl ModelBuilder {
    /// The items of a `DataStackLayers`, bottom → top as stored.
    pub(super) fn layer_stack(&mut self, stack: &RawObject, depth: usize) -> Vec<PainterLayer> {
        self.seen(stack);
        stack.objects("items").map(|item| self.layer(item, depth)).collect()
    }

    fn layer(&mut self, layer: &RawObject, depth: usize) -> PainterLayer {
        let kind = match layer.class.as_str() {
            "DataLayerColor" => {
                self.seen(layer);
                None
            }
            "DataLayerGroup" if depth < MAX_NESTING => {
                self.seen(layer);
                let children = match layer.object("subStack") {
                    Some(sub) => self.layer_stack(sub, depth + 1),
                    None => Vec::new(),
                };
                Some(LayerKind::Group(children))
            }
            "DataLayerGroup" => Some(LayerKind::Other(self.too_deep(layer).class)),
            other => {
                self.raw(layer);
                Some(LayerKind::Other(other.to_string()))
            }
        };
        let content = match layer.object("actions") {
            Some(actions) => self.effect_stack(actions, depth),
            None => Vec::new(),
        };
        let kind = kind.unwrap_or_else(|| match content.first() {
            Some(PainterEffect::Fill(_)) => LayerKind::Fill,
            _ => LayerKind::Paint,
        });
        PainterLayer {
            uid: layer.uid(),
            name: layer.string("label").unwrap_or_default().to_string(),
            kind,
            visible: layer.bool("enabled").unwrap_or(true),
            blending: self.blending(layer),
            mask: self.mask(layer, depth),
            content,
        }
    }

    fn mask(&mut self, layer: &RawObject, depth: usize) -> Option<PainterMask> {
        let actions = layer.object("maskActions")?;
        Some(PainterMask {
            enabled: layer.bool("maskEnabled").unwrap_or(true),
            initial: MaskInitial::from_raw(layer.u64("maskInitial").unwrap_or(1)),
            effects: self.effect_stack(actions, depth),
        })
    }

    /// `perChannelBlending` of a layer, folder or effect.
    pub(super) fn blending(&mut self, holder: &RawObject) -> Vec<ChannelBlending> {
        let mut out = Vec::new();
        for entry in holder.objects("perChannelBlending") {
            if entry.class != "DataBlending" {
                self.raw(entry);
                continue;
            }
            self.seen(entry);
            out.push(ChannelBlending {
                channels: ChannelMask(entry.u64("channelTypes").unwrap_or(0)),
                mode: PainterBlendMode::from_raw(entry.u64("blendingMode").unwrap_or(2)),
                opacity: entry.f32("opacity").unwrap_or(1.0),
            });
        }
        out
    }
}
