//! `DataMaterial` → [`PainterTextureSet`]: identity, resolution, declared channels and the first stack.

use super::ModelBuilder;
use crate::model::channel::PainterChannel;
use crate::model::raw::RawObject;
use crate::model::texture_set::{ChannelDecl, PainterTextureSet};

impl ModelBuilder {
    pub(super) fn texture_set(&mut self, material: &RawObject) -> PainterTextureSet {
        self.seen(material);
        let mut stacks = material.objects("stacks");
        let first = stacks.next();
        self.report.extra_stacks += stacks.count();
        let mut channels = Vec::new();
        let mut layers = Vec::new();
        if let Some(stack) = first {
            self.seen(stack);
            channels = stack.objects("channels").map(|c| self.channel(c)).collect();
            if let Some(items) = stack.object("stack") {
                layers = self.layer_stack(items, 0);
            }
        }
        PainterTextureSet {
            uid: material.uid(),
            material_uid: material.u64("sceneMaterialUID"),
            name: material.string("sceneMaterialName").unwrap_or_default().to_string(),
            resolution_log2: material.ivec2("resolutionLog2"),
            channels,
            layers,
        }
    }

    fn channel(&mut self, channel: &RawObject) -> ChannelDecl {
        self.seen(channel);
        ChannelDecl {
            channel: PainterChannel::from_bit(channel.u64("type").unwrap_or(0)),
            format: channel.u64("format").unwrap_or(0),
            user_name: channel.string("userName").unwrap_or_default().to_string(),
        }
    }
}
