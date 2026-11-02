//! A paint effect's brush as a brush preset for the user's library: the settings through the replay's one mapping
//! table ([`brush_map`]), the tip from the effect's alpha. Painter's `Shape` stamp becomes our round tip (its
//! `hardness`, its `size_ratio` as roundness), a uniform alpha a hard round tip, a bitmap alpha an alpha-image tip
//! whose image is offered (and written) with the brush. Substance alphas (`.sbsar` stamps other than `Shape`) have no
//! tip of ours, so their brushes are not offered.

use super::origin::Origin;
use crate::model::{PaintEffect, PainterSource};
use crate::translate::layers::replay::brush_map;
use crate::translate::layers::replay::tip::ShapeTip;
use materialcraft::paint::brush::{BrushPreset, Tip};

/// A brush preset offered from one paint effect.
#[derive(Debug, Clone, PartialEq)]
pub struct BrushOffer {
    /// The preset (name not set yet). An alpha-image tip is [`Tip::Alpha`] with an empty reference until it is written.
    pub preset: BrushPreset,
    /// The URL of the bitmap alpha its tip is.
    pub alpha_url: Option<String>,
    /// Where the brush came from: the project's own settings, or its alpha's shelf.
    pub origin: Origin,
}

impl BrushOffer {
    /// The brush of `effect`, or why it is not offered.
    pub fn of(effect: &PaintEffect) -> Result<BrushOffer, String> {
        let mapped = brush_map::map(effect).map_err(|b| b.text)?;
        let mut preset = BrushPreset {
            name: String::new(),
            ..mapped.mesh.preset
        };
        let (alpha_url, origin) = match effect.alpha.first() {
            None => {
                preset.tip = Tip::Round;
                preset.hardness = ShapeTip::default().hardness;
                (None, Origin::Project)
            }
            Some(PainterSource::Procedural(p)) => {
                let procedural = p.procedural.as_ref().ok_or("a brush alpha without a procedural")?;
                if procedural.name() != "shape" {
                    return Err(format!("Substance brush alpha \"{}\"", procedural.name()));
                }
                let shape = ShapeTip::from_tweaks(&procedural.tweaks).map_err(|b| b.text)?;
                preset.tip = Tip::Round;
                preset.hardness = shape.hardness;
                preset.roundness = shape.size_ratio;
                (None, Origin::Project)
            }
            Some(PainterSource::Uniform(_)) => {
                preset.tip = Tip::Round;
                preset.hardness = 1.0;
                (None, Origin::Project)
            }
            Some(PainterSource::Bitmap(b)) => {
                let origin =
                    Origin::of_url(&b.url).ok_or_else(|| format!("brush alpha {:?} is no stored resource", b.url))?;
                preset.tip = Tip::Alpha(String::new());
                (Some(b.url.clone()), Origin::Project.stricter(origin))
            }
            Some(other) => return Err(format!("brush alpha source {}", other.kind())),
        };
        Ok(BrushOffer {
            preset,
            alpha_url,
            origin,
        })
    }

    /// The exact preset: two effects with the same key painted with the same settings.
    pub fn key(&self) -> String {
        format!(
            "{}|{}",
            self.preset.to_file_text(),
            self.alpha_url.as_deref().unwrap_or_default()
        )
    }

    /// The brush's character without what a painter changes from stroke to stroke (size, flow, opacity): effects
    /// with the same character are one brush, offered once.
    pub fn character(&self) -> String {
        let preset = BrushPreset {
            size: 0.0,
            flow: 0.0,
            opacity: 0.0,
            ..self.preset.clone()
        };
        format!(
            "{}|{}",
            preset.to_file_text(),
            self.alpha_url.as_deref().unwrap_or_default()
        )
    }

    /// A short description of the tip, for names and descriptions.
    pub fn tip_description(&self) -> String {
        match &self.preset.tip {
            Tip::Alpha(_) => "alpha tip".to_string(),
            _ if self.preset.hardness >= 0.75 => "hard round tip".to_string(),
            _ if self.preset.hardness <= 0.25 => "soft round tip".to_string(),
            _ => "round tip".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        BitmapSource, ChannelMask, EffectHeader, PaintTool, PainterBrush, Procedural, ProceduralSource, Tweak,
        TweakValue,
    };

    fn effect(alpha: Vec<PainterSource>) -> PaintEffect {
        PaintEffect {
            header: EffectHeader {
                uid: None,
                enabled: true,
                label: String::new(),
                channels: ChannelMask(0),
                blending: vec![],
            },
            tool: PaintTool::Brush,
            opacity: 0.75,
            strokes: vec![],
            path_strokes: 1,
            brush_uid: None,
            brush: Some(PainterBrush {
                size: 0.2,
                flow: 0.5,
                ..PainterBrush::default()
            }),
            stamp_blending: 0,
            colors: vec![],
            alpha,
            stencil: None,
        }
    }

    fn shape(url: &str, hardness: f32) -> PainterSource {
        PainterSource::Procedural(ProceduralSource {
            uid: None,
            channels: ChannelMask(0),
            mode: 0,
            procedural: Some(Procedural {
                uid: None,
                url: url.into(),
                tweaks: vec![Tweak {
                    identifier: "hardness".into(),
                    value: TweakValue::Floats(vec![hardness]),
                }],
                outputs: vec![],
                inputs: vec![],
            }),
        })
    }

    #[test]
    fn the_shape_stamp_becomes_our_round_tip_and_stays_the_users() {
        let offer = BrushOffer::of(&effect(vec![shape(
            "resource://allegorithmic/shape/shape?version=f2.sbsar",
            0.9,
        )]))
        .unwrap();
        assert_eq!(
            (
                offer.preset.tip.clone(),
                offer.preset.hardness,
                offer.preset.flow,
                offer.preset.opacity
            ),
            (Tip::Round, 0.9, 0.5, 0.75)
        );
        assert_eq!(
            (offer.alpha_url.clone(), offer.origin.clone()),
            (None, Origin::Project),
            "the tip is drawn by MaterialCraft: no vendor content"
        );
        assert_eq!(offer.tip_description(), "hard round tip");
        let other = BrushOffer::of(&effect(vec![shape(
            "resource://allegorithmic/shape/shape?version=f2.sbsar",
            0.1,
        )]))
        .unwrap();
        assert_ne!(offer.character(), other.character());
        let mut bigger = effect(vec![shape(
            "resource://allegorithmic/shape/shape?version=f2.sbsar",
            0.9,
        )]);
        bigger.opacity = 0.2;
        let bigger = BrushOffer::of(&bigger).unwrap();
        assert_ne!(offer.key(), bigger.key());
        assert_eq!(
            offer.character(),
            bigger.character(),
            "the same brush at another opacity"
        );
    }

    #[test]
    fn a_bitmap_alpha_is_brought_along_and_decides_the_origin() {
        let bitmap = |url: &str| {
            PainterSource::Bitmap(BitmapSource {
                uid: None,
                channels: ChannelMask(0),
                url: url.into(),
            })
        };
        let own = BrushOffer::of(&effect(vec![bitmap("/Scratch?version=ab")])).unwrap();
        assert_eq!(
            (own.alpha_url.as_deref(), own.origin.clone(), own.preset.tip.clone()),
            (Some("/Scratch?version=ab"), Origin::Project, Tip::Alpha(String::new()))
        );
        let vendor = BrushOffer::of(&effect(vec![bitmap("resource://starter_assets/Dots?version=1.image")])).unwrap();
        assert!(!vendor.origin.shareable());
        assert!(
            BrushOffer::of(&effect(vec![shape(
                "resource://allegorithmic/alpha_stroke_02/alpha_stroke_02?version=1.sbsar",
                0.5
            )]))
            .is_err()
        );
        assert!(BrushOffer::of(&effect(vec![bitmap("resourcealias:x")])).is_err());
    }
}
