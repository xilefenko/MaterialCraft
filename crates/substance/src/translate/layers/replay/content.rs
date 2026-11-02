//! What one paint effect of a paint layer's brush work does to each declared channel ([`ContentPlan`]): the brush
//! paints its `sourceColor` uniform per channel (Painter's linear values in our stored encoding); the eraser lowers
//! coverage on every channel it works on. Brushes painting a material or an image are not replayable.

use super::super::channels::SetChannels;
use super::refused;
use crate::model::{PaintEffect, PaintTool, PainterSource};
use crate::translate::blockers::Blocker;
use materialcraft::paint::{Channel, ColorSpace, TiledImage};
use materialcraft::stamp::{ChannelTarget, StampMode};

/// One effect's work per channel.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentPlan {
    /// Channel, value in its stored encoding, and whether it is stored sRGB.
    values: Vec<(Channel, [f32; 4], bool)>,
    mode: StampMode,
}

impl ContentPlan {
    pub fn paints(&self, channel: Channel) -> bool {
        self.values.iter().any(|(c, _, _)| *c == channel)
    }

    /// The target for `channel`'s image, if this effect works on the channel.
    pub fn target<'a>(&self, channel: Channel, image: &'a mut TiledImage) -> Option<ChannelTarget<'a>> {
        let (_, value, srgb) = self.values.iter().find(|(c, _, _)| *c == channel)?;
        Some(ChannelTarget {
            image,
            value: *value,
            mode: self.mode,
            srgb: *srgb,
            source: None,
        })
    }
}

/// The plan of `effect` over the declared `channels`.
pub fn values(effect: &PaintEffect, channels: &SetChannels) -> Result<ContentPlan, Blocker> {
    let srgb = |c: Channel| channels.spec(c).is_some_and(|s| s.color_space == ColorSpace::Srgb);
    let worked = channels.all().filter(|c| channels.mask_has(effect.header.channels, *c));
    match effect.tool {
        PaintTool::Eraser => Ok(ContentPlan {
            values: worked.map(|c| (c, [0.0; 4], srgb(c))).collect(),
            mode: StampMode::Erase,
        }),
        _ => {
            let mut values = Vec::new();
            for c in worked {
                let painter = channels.painter(c);
                match effect.colors.iter().find(|s| s.channels().contains(painter)) {
                    None => {}
                    Some(PainterSource::Uniform(u)) => {
                        let mut v = channels.stored_value(c, u.color);
                        v[3] = u.opacity.clamp(0.0, 1.0);
                        values.push((c, v, srgb(c)));
                    }
                    Some(other) => {
                        return Err(refused(
                            &format!("paints-{}", other.kind()),
                            format!("brush paints a {} source", other.kind()),
                        ));
                    }
                }
            }
            Ok(ContentPlan {
                values,
                mode: StampMode::Paint,
            })
        }
    }
}
