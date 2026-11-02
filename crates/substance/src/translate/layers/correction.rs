//! [`Correction`]: Painter's flattened result only where the imported layers do not reproduce it.
//!
//! The set's final stack (imported layers over Painter's defaults; the hidden reference contributes nothing) is
//! composited per channel and compared texel by texel with the cache as the final look ([`final_look`]: the cache
//! over Painter's defaults). Every texel that differs by more than [`TEXEL_STEPS`] gets Painter's value in the
//! correction layer at full coverage; every other texel is left empty (coverage 0, no tile allocated), so the
//! layers below show through. With the correction on top the set looks as Painter saved it; switched off, it shows
//! what the layers reproduce, and how much it covers is how far the import still is from editable.
//!
//! The correction is Painter's pixels, not something to paint on: it is locked, and its lock lists the share of
//! texels it covers per channel.

use super::cache_score::final_look;
use super::island_texels::IslandTexels;
use materialcraft::generators;
use materialcraft::paint::{
    BlendMode, Channel, Compositor, FillContext, Layer, LayerKind, LayerLock, PaintContent, SampleType, TextureSet,
    TiledImage,
};
use materialcraft::tex::Tex;

pub const CORRECTION_ID: &str = "painter-correction";
pub const CORRECTION_NAME: &str = "Painter correction";

/// A texel differing by more than this many steps of its channel's storage (any component) is corrected: above the 1
/// step of rounding either way, so constants rounded differently don't count. Steps are 1/255 for 8-bit channels,
/// 1/2048 for half floats and 1/65535 for 16-bit and float ones (height feeds the normal map through its slope, so a
/// few 8-bit steps of height are a visible tilt).
pub const TEXEL_STEPS: f32 = 1.5;

/// The correction layer and how much of each channel it covers.
pub struct Correction {
    pub layer: Layer,
    /// Per channel the cache has: the share (0..1) of the set's texels on the mesh that are corrected.
    pub coverage: Vec<(Channel, f32)>,
}

impl Correction {
    /// The correction of `set` (its layers as they will be saved, without the correction) against the cache layer
    /// `result`, with fills laid on by `fills` (the editor's: the set's surface map in the scene's frame).
    /// `islands` limits the counted texels to the mesh. `None` when `result` holds no images.
    pub fn build(
        set: &TextureSet,
        result: &Layer,
        fills: &FillContext,
        islands: Option<&IslandTexels>,
        source: &str,
    ) -> Option<Correction> {
        let LayerKind::Paint(cache) = &result.kind else {
            return None;
        };
        // Composited as the editor shows it: our generators and filters evaluated.
        let compositor = Compositor::with_effects(set, generators::builtin()).with_fills(fills);
        let mut layer = Layer::new(
            CORRECTION_ID,
            CORRECTION_NAME,
            LayerKind::Paint(PaintContent::default()),
        );
        let mut content = PaintContent::default();
        let mut coverage = Vec::new();
        for (channel, image) in &cache.images {
            let Some(spec) = set.channel_spec(*channel) else {
                continue;
            };
            let (ours, _) = compositor.composite_tex(*channel);
            let look = final_look(image, spec);
            let step = match image.format().sample {
                SampleType::U8 => 1.0 / 255.0,
                // a half float's step around 0.5 … 1, where height and normals live
                SampleType::F16 => 1.0 / 2048.0,
                SampleType::U16 | SampleType::F32 => 1.0 / 65535.0,
            };
            let Some((pixels, share)) = corrected(
                &ours,
                &look,
                image,
                spec.channel.is_color(),
                TEXEL_STEPS * step,
                islands,
            ) else {
                continue;
            };
            content.set_image(*channel, pixels);
            layer.set_blend(*channel, BlendMode::Normal, 1.0);
            coverage.push((*channel, share));
        }
        if coverage.is_empty() {
            return None;
        }
        layer.kind = LayerKind::Paint(content);
        layer.lock = Some(LayerLock::new(source, vec![describe(&coverage)]));
        Some(Correction { layer, coverage })
    }

    /// True when no texel of any channel needed correcting.
    pub fn is_empty(&self) -> bool {
        self.coverage.iter().all(|(_, share)| *share == 0.0)
    }
}

/// `look` wherever it differs from `ours` by more than `tolerance`, in an image of `cache`'s format (alpha 1 there, 0 elsewhere), and the
/// share of on-mesh texels that differ. Texels off the mesh are corrected too (they feed export padding) but not
/// counted. `None` when the sizes don't match.
fn corrected(
    ours: &Tex,
    look: &Tex,
    cache: &TiledImage,
    color: bool,
    tolerance: f32,
    islands: Option<&IslandTexels>,
) -> Option<(TiledImage, f32)> {
    let (w, h) = (look.w, look.h);
    if ours.w != w || ours.h != h {
        return None;
    }
    let islands = islands.filter(|m| m.size() == (w, h));
    let components = if color { 3 } else { 1 };
    let mut out = TiledImage::new(w as u32, h as u32, cache.format()).ok()?;
    let (mut on_mesh, mut differ) = (0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let (o, p) = (ours.px.get(i)?, look.px.get(i)?);
            let off = (0..components).any(|c| {
                let d = (o[c] - p[c]).abs();
                !d.is_finite() || d > tolerance
            });
            let counted = islands.is_none_or(|m| m.contains(x, y));
            on_mesh += usize::from(counted);
            if off {
                differ += usize::from(counted);
                out.set(x as u32, y as u32, [p[0], p[1], p[2], 1.0]);
            }
        }
    }
    let share = if on_mesh == 0 {
        0.0
    } else {
        differ as f32 / on_mesh as f32
    };
    Some((out, share))
}

/// `covers base color 12.5%, roughness 0%` for the lock and the report.
pub fn describe(coverage: &[(Channel, f32)]) -> String {
    let parts: Vec<String> = coverage
        .iter()
        .map(|(c, share)| format!("{} {}", c.id().replace('_', " "), percent(*share)))
        .collect();
    format!("Painter's result where the layers differ: {}", parts.join(", "))
}

fn percent(share: f32) -> String {
    let p = share * 100.0;
    if p > 0.0 && p < 0.1 {
        "<0.1%".to_string()
    } else {
        format!("{p:.1}%")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::channel_table::channel_spec;
    use materialcraft::paint::{Fill, NO_FILL_CONTEXT};

    /// A 4×4 set with roughness, a uniform fill of `ours`, and a cache layer holding `cache` per texel.
    fn case(ours: f32, cache: impl Fn(u32, u32) -> f32) -> (TextureSet, Layer) {
        let mut set = TextureSet::new("s", "S", (4, 4));
        set.channels.push(channel_spec(Channel::Roughness, Some(1)));
        let fill = Layer::new(
            "f",
            "F",
            LayerKind::Fill(Fill::uniform(vec![(Channel::Roughness, [ours; 4])])),
        )
        .with_blend(Channel::Roughness, BlendMode::Normal, 1.0);
        set.layers.push(fill);
        let mut image = TiledImage::new(4, 4, set.channels[0].layer_format()).unwrap();
        for y in 0..4 {
            for x in 0..4 {
                let v = cache(x, y);
                image.set(x, y, [v, v, v, 1.0]);
            }
        }
        let mut content = PaintContent::default();
        content.set_image(Channel::Roughness, image);
        (set, Layer::new("r", "R", LayerKind::Paint(content)))
    }

    fn composite(set: &TextureSet) -> Vec<f32> {
        Compositor::new(set)
            .composite_tex(Channel::Roughness)
            .0
            .px
            .iter()
            .map(|p| p[0])
            .collect()
    }

    #[test]
    fn only_differing_texels_are_corrected_and_the_result_is_painters() {
        // Painter shows 0.3 everywhere except one texel at 0.9.
        let (mut set, result) = case(0.3, |x, y| if (x, y) == (2, 1) { 0.9 } else { 0.3 });
        let c = Correction::build(&set, &result, &NO_FILL_CONTEXT, None, "test").unwrap();
        assert!((c.coverage[0].1 - 1.0 / 16.0).abs() < 1e-6, "{:?}", c.coverage);
        assert!(
            c.layer.lock.as_ref().is_some_and(|l| l.reasons[0].contains("6.2%")),
            "{:?}",
            c.layer.lock
        );
        let LayerKind::Paint(p) = &c.layer.kind else { panic!() };
        let img = p.image(Channel::Roughness).unwrap();
        assert_eq!(img.get(0, 0)[3], 0.0, "agreeing texels are left to the layers below");
        assert_eq!(img.get(2, 1)[3], 1.0);
        set.layers.push(c.layer);
        let final_look = composite(&set);
        assert!((final_look[1 * 4 + 2] - 0.9).abs() < 2.0 / 255.0);
        assert!(
            final_look
                .iter()
                .enumerate()
                .all(|(i, v)| i == 6 || (v - 0.3).abs() < 1.0 / 255.0)
        );
    }

    #[test]
    fn a_stack_that_reproduces_painter_needs_no_correction() {
        let (set, result) = case(0.3, |_, _| 77.0 / 255.0);
        let c = Correction::build(&set, &result, &NO_FILL_CONTEXT, None, "test").unwrap();
        assert!(c.is_empty());
    }
}
