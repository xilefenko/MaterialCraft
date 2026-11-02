//! Mapping tables from Painter's channel bits, channel formats and baked-map usages to ours.
//! The values were read off real projects; anything else is reported, not guessed.

use materialcraft::paint::{BakedKind, Channel, ChannelSpec, ColorSpace, PixelFormat, PixelLayout, SampleType};

/// Painter channel bits (texture-cache leaf names and `DataChannel.type`) with our channel and Painter's label.
const CHANNELS: &[(u64, Channel, &str)] = &[
    (1, Channel::BaseColor, "Base Color"),
    (2, Channel::Height, "Height"),
    (8, Channel::Opacity, "Opacity"),
    (16, Channel::Emissive, "Emissive"),
    (128, Channel::Roughness, "Roughness"),
    (8192, Channel::Metallic, "Metallic"),
    (4194304, Channel::Normal, "Normal"),
];

/// Our channel for a Painter channel bit.
pub fn channel_for_bit(bit: u64) -> Option<Channel> {
    CHANNELS.iter().find(|(b, _, _)| *b == bit).map(|(_, c, _)| *c)
}

/// Painter's name for a channel bit, for the report.
pub fn painter_channel_name(bit: u64) -> String {
    CHANNELS
        .iter()
        .find(|(b, _, _)| *b == bit)
        .map_or_else(|| format!("channel bit {bit}"), |(_, _, name)| (*name).to_string())
}

/// The storage spec for a channel, from Painter's `DataChannel.format` when known: 0 = sRGB8, 1 = L8,
/// 5 = L16F, 6 = RGB16F. Normals keep our 16-bit encoded format (Painter's values are encoded 0..1 too).
pub fn channel_spec(channel: Channel, painter_format: Option<u64>) -> ChannelSpec {
    let mut spec = ChannelSpec::new(channel);
    let gray_or_rgb = |sample| match channel.is_color() {
        true => PixelFormat::new(PixelLayout::Rgb, sample),
        false => PixelFormat::new(PixelLayout::Gray, sample),
    };
    match painter_format {
        Some(0) => {
            spec.format = gray_or_rgb(SampleType::U8);
            spec.color_space = ColorSpace::Srgb;
        }
        Some(1) => spec.format = gray_or_rgb(SampleType::U8),
        Some(5) | Some(6) if channel != Channel::Normal => spec.format = gray_or_rgb(SampleType::F16),
        _ => {}
    }
    spec
}

/// What Painter shows where no layer covers a channel, in our stored encoding (Verified on partially covered
/// texels of Hörbuch: base colour 0.5 linear = 0.735 sRGB, roughness 0.3, metallic 0; flat normal, zero height).
pub fn painter_default(channel: Channel) -> [f32; 4] {
    match channel {
        Channel::BaseColor => [0.735_357, 0.735_357, 0.735_357, 1.0],
        Channel::Roughness => [0.3, 0.3, 0.3, 1.0],
        Channel::Metallic | Channel::Height | Channel::Emissive | Channel::User(_) => [0.0, 0.0, 0.0, 1.0],
        Channel::Normal => [0.5, 0.5, 1.0, 1.0],
        Channel::Opacity | Channel::AmbientOcclusion => [1.0; 4],
    }
}

/// The kind of a baked map from `DataAdditionalMap.usage` (`None` = the material's `normalMapBitmap`).
pub fn baked_kind(usage: Option<u64>) -> BakedKind {
    match usage {
        None => BakedKind::Normal,
        Some(0) => BakedKind::Id,
        Some(1) => BakedKind::AmbientOcclusion,
        Some(2) => BakedKind::Curvature,
        Some(3) => BakedKind::WorldSpaceNormal,
        Some(4) => BakedKind::Position,
        Some(5) => BakedKind::Thickness,
        Some(n) => BakedKind::Other(format!("painter_usage_{n}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_bits_and_formats() {
        assert_eq!(channel_for_bit(4194304), Some(Channel::Normal));
        assert_eq!(channel_for_bit(4), None);
        assert_eq!(painter_channel_name(4), "channel bit 4");
        let base = channel_spec(Channel::BaseColor, Some(0));
        assert_eq!((base.format.id(), base.color_space), ("rgb8".into(), ColorSpace::Srgb));
        assert_eq!(channel_spec(Channel::Height, Some(5)).format.id(), "gray16f");
        assert_eq!(channel_spec(Channel::Normal, Some(6)).format.id(), "rgb16");
        assert_eq!(
            channel_spec(Channel::Roughness, None),
            ChannelSpec::new(Channel::Roughness)
        );
        assert_eq!(baked_kind(Some(9)), BakedKind::Other("painter_usage_9".into()));
        assert_eq!(painter_default(Channel::Roughness)[0], 0.3);
    }
}
