//! A paint effect's brush alpha (`sourceTransparent`) as a sampled tip ([`TipAlpha`]): Painter's parametric
//! `Shape` stamp drawn by our own profile ([`shape_tip`], a smooth fall-off of our own, not fitted yet: no measurable
//! case), an embedded bitmap alpha read from the project, or a uniform alpha (a solid square). Other
//! Substance alphas (`.sbsar` stamps) are not computable.

use super::super::images::{ImageSource, grey_values};
use super::{failed, refused};
use crate::model::{PainterSource, Tweak, TweakValue};
use crate::translate::blockers::{Blocker, BlockerKey, BlockerKind};
use materialcraft::paint::TiledImage;
use materialcraft_brush::TipAlpha;

/// Side of the drawn shape tips, and the longest side bitmap alphas are reduced to.
pub const TIP_SIDE: u32 = 256;

/// How a tip was made, for the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipKind {
    Shape,
    Bitmap,
    Uniform,
}

/// The tip of `alpha` (the effect's `sourceTransparent` list; the first entry is the alpha), or why not.
pub fn tip(alpha: &[PainterSource], images: &dyn ImageSource) -> Result<(TipAlpha, TipKind), Blocker> {
    let Some(source) = alpha.first() else {
        let shape = ShapeTip::default();
        return Ok((
            shape_tip(&shape).ok_or_else(|| failed("tip could not be drawn"))?,
            TipKind::Shape,
        ));
    };
    match source {
        PainterSource::Procedural(p) => {
            let procedural = p
                .procedural
                .as_ref()
                .ok_or_else(|| refused("alpha-without-procedural", "brush alpha without a procedural"))?;
            match procedural.name() {
                "shape" => {
                    let shape = ShapeTip::from_tweaks(&procedural.tweaks)?;
                    Ok((
                        shape_tip(&shape).ok_or_else(|| failed("tip could not be drawn"))?,
                        TipKind::Shape,
                    ))
                }
                other => Err(Blocker::new(
                    BlockerKey::new(BlockerKind::BrushAlpha, other),
                    format!("brush alpha \"{other}\""),
                )),
            }
        }
        PainterSource::Bitmap(b) => {
            let loaded = images
                .load(&b.url)
                .map_err(|e| refused("alpha-bitmap-unreadable", format!("brush alpha {e}")))?;
            Ok((
                bitmap_tip(&loaded.image)
                    .ok_or_else(|| refused("alpha-bitmap-empty", "brush alpha bitmap is empty"))?,
                TipKind::Bitmap,
            ))
        }
        PainterSource::Uniform(u) => {
            let v = (u.color[0] * u.opacity).clamp(0.0, 1.0);
            let tip = TipAlpha::new(8, 8, vec![v; 64]).ok_or_else(|| failed("tip could not be drawn"))?;
            Ok((tip, TipKind::Uniform))
        }
        other => Err(refused(
            &format!("alpha-source-{}", other.kind()),
            format!("brush alpha source {}", other.kind()),
        )),
    }
}

/// Painter's `Shape` stamp parameters (`hardness`; newer files also `shape` and `size_ratio`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeTip {
    /// 0 soft … 1 hard.
    pub hardness: f32,
    /// The `shape` choice (absent: the round default).
    pub shape: Option<i32>,
    /// Height over width (1 = round).
    pub size_ratio: f32,
}

impl Default for ShapeTip {
    fn default() -> ShapeTip {
        ShapeTip {
            hardness: 0.0,
            shape: None,
            size_ratio: 1.0,
        }
    }
}

impl ShapeTip {
    /// The parameters from the `.sbsar` tweaks; an unknown parameter that is not at its default is an error.
    pub fn from_tweaks(tweaks: &[Tweak]) -> Result<ShapeTip, Blocker> {
        let mut out = ShapeTip::default();
        for t in tweaks {
            match (t.identifier.as_str(), &t.value) {
                ("hardness", TweakValue::Floats(v)) => out.hardness = v.first().copied().unwrap_or(0.0).clamp(0.0, 1.0),
                ("size_ratio", TweakValue::Floats(v)) => {
                    out.size_ratio = v.first().copied().unwrap_or(1.0).clamp(0.01, 1.0)
                }
                ("shape", TweakValue::Int(n)) => out.shape = Some(*n),
                (id, _) if id.starts_with('$') => {}
                (id, v) => {
                    return Err(refused(
                        &format!("shape-tip-parameter-{id}"),
                        format!("shape tip parameter {id} = {v:?}"),
                    ));
                }
            }
        }
        match out.shape {
            None => Ok(out),
            Some(n) => Err(refused(
                &format!("shape-tip-variant-{n}"),
                format!("shape tip variant {n}"),
            )),
        }
    }
}

/// The coverage of a round shape tip at normalised radius `r` (0 centre, 1 rim) for `hardness`: full up to
/// the hard core, then a smooth fall to 0 at the rim (our own curve, not fitted).
pub fn shape_profile(r: f32, hardness: f32) -> f32 {
    // not below the rim (NaN included): nothing
    if r.is_nan() || r >= 1.0 {
        return 0.0;
    }
    let core = hardness.clamp(0.0, 1.0) * 0.98;
    if r <= core {
        return 1.0;
    }
    let t = ((r - core) / (1.0 - core)).clamp(0.0, 1.0);
    // smoothstep from 1 to 0
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// The shape tip drawn [`TIP_SIDE`] square (the stamp's height scaled by `size_ratio`).
pub fn shape_tip(shape: &ShapeTip) -> Option<TipAlpha> {
    let n = TIP_SIDE as usize;
    let ratio = shape.size_ratio.clamp(0.01, 1.0);
    let mut values = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let v = ((y as f32 + 0.5) / n as f32 * 2.0 - 1.0) / ratio;
            values.push(shape_profile((u * u + v * v).sqrt(), shape.hardness));
        }
    }
    TipAlpha::new(TIP_SIDE, TIP_SIDE, values)
}

/// A bitmap alpha (grey × alpha) as a tip, box-filtered down to at most [`TIP_SIDE`] on its longer side.
pub fn bitmap_tip(image: &TiledImage) -> Option<TipAlpha> {
    let (w, h) = image.size();
    if w == 0 || h == 0 {
        return None;
    }
    let grey = grey_values(image);
    let step = w.max(h).div_ceil(TIP_SIDE).max(1);
    let (tw, th) = (w.div_ceil(step), h.div_ceil(step));
    let mut values = Vec::with_capacity(tw as usize * th as usize);
    for ty in 0..th {
        for tx in 0..tw {
            let (mut sum, mut n) = (0.0, 0.0);
            for y in ty * step..((ty + 1) * step).min(h) {
                for x in tx * step..((tx + 1) * step).min(w) {
                    sum += grey.get(y as usize * w as usize + x as usize).copied().unwrap_or(0.0);
                    n += 1.0;
                }
            }
            values.push(if n > 0.0 { sum / n } else { 0.0 });
        }
    }
    TipAlpha::new(tw, th, values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use materialcraft::paint::{PixelFormat, PixelLayout, SampleType};

    fn tweak(id: &str, value: TweakValue) -> Tweak {
        Tweak {
            identifier: id.into(),
            value,
        }
    }

    #[test]
    fn the_shape_profile_falls_from_the_core_to_the_rim() {
        for hardness in [0.0, 0.5, 1.0] {
            let v: Vec<f32> = (0..=20).map(|i| shape_profile(i as f32 / 20.0, hardness)).collect();
            assert!(v.windows(2).all(|w| w[1] <= w[0] + 1e-6), "falls: {v:?}");
            assert_eq!((v[0], v[20]), (1.0, 0.0));
        }
        // harder tips stay full further out
        assert!(shape_profile(0.7, 1.0) > shape_profile(0.7, 0.5) && shape_profile(0.7, 0.5) > shape_profile(0.7, 0.0));
    }

    #[test]
    fn shape_parameters_come_from_the_tweaks() {
        let tweaks = vec![
            tweak("hardness", TweakValue::Floats(vec![0.8])),
            tweak("$outputsize", TweakValue::Int2([8, 8])),
        ];
        let shape = ShapeTip::from_tweaks(&tweaks).unwrap();
        assert_eq!((shape.hardness, shape.size_ratio, shape.shape), (0.8, 1.0, None));
        assert!(
            ShapeTip::from_tweaks(&[tweak("angle", TweakValue::Floats(vec![1.0]))]).is_err(),
            "unknown parameter"
        );
        let tip = shape_tip(&ShapeTip {
            size_ratio: 0.5,
            ..shape
        })
        .unwrap();
        let at = |x: u32, y: u32| tip.values[(y * tip.width + x) as usize];
        let c = TIP_SIDE / 2;
        // squashed to half its height: full across, empty a third of the way up
        assert!(at(c, c) > 0.99 && at(c / 4, c) > 0.5 && at(c, c / 3) < 0.01);
    }

    #[test]
    fn bitmap_alphas_are_reduced_to_the_tip_size() {
        let mut image = TiledImage::new(1024, 512, PixelFormat::new(PixelLayout::GrayAlpha, SampleType::U8)).unwrap();
        image.fill([1.0, 1.0, 1.0, 1.0]);
        let tip = bitmap_tip(&image).unwrap();
        assert_eq!((tip.width, tip.height), (256, 128));
        assert!(tip.values.iter().all(|v| (v - 1.0).abs() < 1e-4));
    }
}
