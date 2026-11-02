//! [`StackVerdict`]: how exactly an imported stack reproduces Painter, per channel (only visible layers that
//! contribute count) and per layer (every layer whose content, mask or blending is not exact).

use super::computability::Computability;
use materialcraft::paint::Channel;
use std::collections::{BTreeMap, BTreeSet};

/// One layer that is not imported exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerNote {
    /// Folder path and name, e.g. `Rust / Fill layer 3`.
    pub path: String,
    pub visible: bool,
    pub status: Computability,
}

/// A layer shown in Painter whose mask cannot be computed yet.
#[derive(Debug, Clone, PartialEq)]
pub struct MaskFailure {
    /// Our layer id.
    pub id: String,
    pub path: String,
    pub reasons: Vec<String>,
}

/// Per channel while building: the plain status, the guessed normal-map modes, and whether every normal
/// value seen is flat (then any normal-map mode leaves the result unchanged and the guesses do not matter).
#[derive(Debug, Clone)]
struct ChannelTally {
    status: Computability,
    normal_map_guesses: Computability,
    flat: bool,
}

impl Default for ChannelTally {
    fn default() -> Self {
        ChannelTally {
            status: Computability::Exact,
            normal_map_guesses: Computability::Exact,
            flat: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StackVerdict {
    channels: BTreeMap<String, (Channel, ChannelTally)>,
    pub layers: Vec<LayerNote>,
    /// Painter blend values of the contributing visible layers and folders, per channel id (for fits).
    pub blend_values: BTreeMap<String, BTreeSet<u64>>,
    /// Number of layers (folders included) imported.
    pub layer_count: usize,
    /// Layers (and folders) Painter shows whose mask is not computable; they are imported without a mask.
    pub mask_failures: Vec<MaskFailure>,
}

impl StackVerdict {
    pub fn new(channels: impl Iterator<Item = Channel>) -> StackVerdict {
        let mut v = StackVerdict::default();
        for c in channels {
            v.channels.insert(c.id(), (c, ChannelTally::default()));
        }
        v
    }

    fn tally(&mut self, channel: Channel) -> Option<&mut ChannelTally> {
        self.channels.get_mut(&channel.id()).map(|(_, t)| t)
    }

    /// A visible contribution to `channel`.
    pub fn contribute(&mut self, channel: Channel, status: &Computability) {
        if let Some(t) = self.tally(channel) {
            t.status.and(status);
        }
    }

    /// A guessed normal-map mode on `channel`, harmless while all normal values are flat.
    pub fn normal_map_guess(&mut self, channel: Channel, status: &Computability) {
        if let Some(t) = self.tally(channel) {
            t.normal_map_guesses.and(status);
        }
    }

    /// A normal value that is not flat was imported for `channel`.
    pub fn not_flat(&mut self, channel: Channel) {
        if let Some(t) = self.tally(channel) {
            t.flat = false;
        }
    }

    /// A Painter blend value used by a contributing layer or folder.
    pub fn blend_value(&mut self, channel: Channel, value: u64) {
        self.blend_values.entry(channel.id()).or_default().insert(value);
    }

    pub fn note(&mut self, note: LayerNote) {
        self.layers.push(note);
    }

    /// The final status of one channel (Exact for undeclared ones).
    pub fn channel(&self, channel: Channel) -> Computability {
        let Some((_, t)) = self.channels.get(&channel.id()) else {
            return Computability::Exact;
        };
        let mut status = t.status.clone();
        if !t.flat {
            status.and(&t.normal_map_guesses);
        }
        status
    }

    /// Every declared channel with its final status, in id order.
    pub fn channels(&self) -> Vec<(Channel, Computability)> {
        self.channels.values().map(|(c, _)| (*c, self.channel(*c))).collect()
    }

    /// True when every channel is reproduced exactly.
    pub fn all_exact(&self) -> bool {
        self.channels().iter().all(|(_, s)| s.is_exact())
    }

    /// True when every channel is exact apart from chosen replacements.
    pub fn all_exact_or_replaced(&self) -> bool {
        self.channels().iter().all(|(_, s)| s.is_exact_or_replaced())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_map_guesses_only_count_with_non_flat_normals() {
        let mut v = StackVerdict::new([Channel::Normal, Channel::BaseColor].into_iter());
        v.normal_map_guess(Channel::Normal, &Computability::approximated("mode 27"));
        assert!(v.all_exact());
        v.not_flat(Channel::Normal);
        assert!(!v.channel(Channel::Normal).is_exact());
        v.contribute(
            Channel::BaseColor,
            &Computability::not_computable(crate::translate::blockers::Blocker::feature("strokes", "strokes")),
        );
        assert!(!v.channel(Channel::BaseColor).is_computable());
        assert!(v.channel(Channel::Height).is_exact(), "undeclared");
        assert_eq!(v.channels().len(), 2);
    }
}
