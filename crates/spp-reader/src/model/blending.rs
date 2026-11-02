//! [`ChannelBlending`]: one `DataBlending` entry (blend mode and opacity for a set of channels) and
//! [`PainterBlendMode`], Painter's `blendingMode` enum.
//!
//! Only values with evidence are named (see `interop/painter-document-catalogue.md` for the table and
//! confidence of each); everything else stays [`PainterBlendMode::Unknown`] with the stored number.

use super::channel::{ChannelMask, PainterChannel};
use std::fmt;

/// Painter's blend mode as stored (`DataBlending.blendingMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PainterBlendMode {
    /// 0: the channel is left out of the layer (Observed: "Dirt" layers switching off height/metallic).
    Disable,
    /// 1 (Inferred: used on height/normal of material layers).
    Replace,
    /// 2: the default for colour-like channels (Observed).
    Normal,
    /// 4: the default of the ambient-occlusion channel and of the material's AO map (Observed).
    Multiply,
    /// 10: the default of the height channel (Inferred).
    LinearDodge,
    /// 25: the default of effect groups; filter-only layers (Observed).
    Passthrough,
    /// 26: `additionalNormalMapBlending`; Painter's combine of baked and painted normals is RNM (Inferred).
    NormalMapDetail,
    /// Any other value (27 is the normal channel's default, a normal-map mode not yet identified).
    Unknown(u64),
}

impl PainterBlendMode {
    pub fn from_raw(value: u64) -> PainterBlendMode {
        match value {
            0 => PainterBlendMode::Disable,
            1 => PainterBlendMode::Replace,
            2 => PainterBlendMode::Normal,
            4 => PainterBlendMode::Multiply,
            10 => PainterBlendMode::LinearDodge,
            25 => PainterBlendMode::Passthrough,
            26 => PainterBlendMode::NormalMapDetail,
            n => PainterBlendMode::Unknown(n),
        }
    }

    /// The stored number.
    pub fn raw(self) -> u64 {
        match self {
            PainterBlendMode::Disable => 0,
            PainterBlendMode::Replace => 1,
            PainterBlendMode::Normal => 2,
            PainterBlendMode::Multiply => 4,
            PainterBlendMode::LinearDodge => 10,
            PainterBlendMode::Passthrough => 25,
            PainterBlendMode::NormalMapDetail => 26,
            PainterBlendMode::Unknown(n) => n,
        }
    }
}

impl fmt::Display for PainterBlendMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PainterBlendMode::Disable => f.write_str("disable"),
            PainterBlendMode::Replace => f.write_str("replace"),
            PainterBlendMode::Normal => f.write_str("normal"),
            PainterBlendMode::Multiply => f.write_str("multiply"),
            PainterBlendMode::LinearDodge => f.write_str("linear dodge"),
            PainterBlendMode::Passthrough => f.write_str("passthrough"),
            PainterBlendMode::NormalMapDetail => f.write_str("normal map detail"),
            PainterBlendMode::Unknown(n) => write!(f, "mode {n}"),
        }
    }
}

/// One `DataBlending`: how the channels in `channels` blend onto what is below.
///
/// Format ≤ 59 stores one entry per channel bit (28–34 entries, defaults included); format ≥ 61 stores
/// only entries that differ from the defaults, often for several channels at once (Observed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelBlending {
    pub channels: ChannelMask,
    pub mode: PainterBlendMode,
    pub opacity: f32,
}

impl ChannelBlending {
    /// True for what Painter stores when nothing was changed: full opacity and the channel's usual mode.
    pub fn is_default(&self) -> bool {
        if self.opacity != 1.0 {
            return false;
        }
        let usual = |c: PainterChannel| match c {
            PainterChannel::Height => self.mode == PainterBlendMode::LinearDodge,
            PainterChannel::Normal => self.mode == PainterBlendMode::Unknown(27),
            PainterChannel::AmbientOcclusion => self.mode == PainterBlendMode::Multiply,
            _ => self.mode == PainterBlendMode::Normal,
        };
        self.channels.channels().all(usual)
    }
}

/// The entry that covers `channel`, if any.
pub fn blending_for(entries: &[ChannelBlending], channel: PainterChannel) -> Option<&ChannelBlending> {
    entries.iter().find(|b| b.channels.contains(channel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_keep_their_numbers() {
        for raw in 0..40 {
            assert_eq!(PainterBlendMode::from_raw(raw).raw(), raw);
        }
        assert_eq!(PainterBlendMode::from_raw(25), PainterBlendMode::Passthrough);
        assert_eq!(PainterBlendMode::from_raw(11).to_string(), "mode 11");
    }

    #[test]
    fn defaults_depend_on_the_channel() {
        let height = |mode| ChannelBlending {
            channels: ChannelMask(2),
            mode,
            opacity: 1.0,
        };
        assert!(height(PainterBlendMode::LinearDodge).is_default());
        assert!(!height(PainterBlendMode::Normal).is_default());
        let entries = [height(PainterBlendMode::Normal)];
        assert!(blending_for(&entries, PainterChannel::Height).is_some());
        assert!(blending_for(&entries, PainterChannel::BaseColor).is_none());
    }
}
