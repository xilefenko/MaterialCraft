//! Painter fill sources that are a procedural texture (one grey output feeding channels) replaced by one of
//! MaterialCraft's textures: when the plan replaces the texture, the channels the source feeds
//! become a [`TextureFill`] with our texture and its translated parameters, laid on by the fill's projection, and
//! the Painter procedural is kept as the fill's [`Fill::original`]. A Substance material (several outputs, see
//! [`super::materials`]) never takes this path.

use super::materials::is_material;
use super::{ProceduralPlan, Replacement, replace};
use crate::model::ProceduralSource;
use crate::translate::layers::effect_data;
use materialcraft::paint::effect::Procedural;
use materialcraft::paint::{Channel, Fill, FillSource, TextureFill};
use materialcraft::projection::Projection;

/// A replaced texture source: the table entry, our texture (without its own placement: the fill places it) and
/// the imported Painter procedural it stands in for.
pub struct ChannelTexture {
    pub replacement: &'static Replacement,
    pub ours: Procedural,
    pub original: Procedural,
}

/// Our texture for `source` under `plan`, if it is a texture (not a material) the plan replaces. Painter also
/// marks a texture graph as a material (`mode` 1) when it was dropped in as one; with a single output feeding the
/// channels it is still the one grey texture, so it counts as a texture (a material the user picked one of ours
/// for is decided before, see [`super::materials::chosen`]).
pub fn chosen(source: &ProceduralSource, plan: &ProceduralPlan) -> Option<ChannelTexture> {
    let single_output = source.procedural.as_ref().is_some_and(|p| p.outputs.len() == 1);
    if is_material(source) && !single_output {
        return None;
    }
    let original = effect_data::procedural(source.procedural.as_ref());
    let (replacement, mut ours) = replace(&original, plan).filter(|(r, _)| r.is_texture())?;
    ours.original = None;
    Some(ChannelTexture {
        replacement,
        ours,
        original,
    })
}

impl ChannelTexture {
    /// The texture fill writing `channels`, laid on by `projection`.
    pub fn fill(&self, channels: &[Channel], projection: Projection) -> Fill {
        Fill::new(FillSource::Texture(TextureFill::on(channels, &self.ours)))
            .with_projection(projection)
            .replacing(self.original.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ChannelMask, Procedural as PainterProcedural, ProceduralOutput, Tweak, TweakValue};
    use crate::translate::replacements::Choice;

    fn source(name: &str, mode: u64) -> ProceduralSource {
        ProceduralSource {
            uid: None,
            channels: ChannelMask::default(),
            mode,
            procedural: Some(PainterProcedural {
                uid: None,
                url: format!("resource://shelf/{name}/{name}?version=1.sbsar"),
                tweaks: vec![Tweak {
                    identifier: "scale".into(),
                    value: TweakValue::Floats(vec![32.0]),
                }],
                outputs: vec![],
                inputs: vec![],
            }),
        }
    }

    #[test]
    fn replaced_textures_become_texture_fills() {
        let replace = ProceduralPlan::all(Choice::Replace);
        let t = chosen(&source("ratio_perlin_noise", 0), &replace).unwrap();
        assert_eq!(t.ours.id, "perlin");
        // Painter's lattice cells → our multiple of 8 cells
        assert_eq!(t.ours.param("scale"), Some("4"));
        assert_eq!(t.original.id, "substance:ratio_perlin_noise");
        let fill = t.fill(&[Channel::Roughness, Channel::Height], Projection::default());
        let FillSource::Texture(textures) = &fill.source else {
            panic!("a texture fill")
        };
        assert_eq!(textures.channels(), vec![Channel::Roughness, Channel::Height]);
        assert_eq!(textures.texture(Channel::Height), Some(&t.ours));
        assert!(t.ours.original.is_none(), "the original is the fill's");
        assert_eq!(
            fill.original.map(|o| o.id),
            Some("substance:ratio_perlin_noise".to_string())
        );
    }

    #[test]
    fn kept_unknown_generators_and_materials_stay() {
        let replace = ProceduralPlan::all(Choice::Replace);
        assert!(
            chosen(&source("ratio_perlin_noise", 0), &ProceduralPlan::default()).is_none(),
            "kept by the plan"
        );
        let keep_perlin = replace.clone().with("ratio_perlin_noise", Choice::Keep);
        assert!(chosen(&source("ratio_perlin_noise", 0), &keep_perlin).is_none());
        assert!(chosen(&source("fish_scales", 0), &replace).is_none(), "no replacement");
        assert!(
            chosen(&source("mg_dirt", 0), &replace).is_none(),
            "a mask generator, not a texture"
        );
        assert!(
            chosen(&source("ratio_perlin_noise", 1), &replace).is_none(),
            "a material"
        );
    }

    #[test]
    fn a_texture_dropped_in_as_a_material_is_still_a_texture() {
        let replace = ProceduralPlan::all(Choice::Replace);
        let output = |id: &str| ProceduralOutput {
            identifier: id.into(),
            channels: ChannelMask(1),
        };
        let mut one = source("gaussian_spots_2", 1);
        if let Some(p) = one.procedural.as_mut() {
            p.outputs = vec![output("output")];
        }
        assert_eq!(
            chosen(&one, &replace).map(|t| t.ours.id),
            Some("gaussian_spots_2".to_string())
        );
        if let Some(p) = one.procedural.as_mut() {
            p.outputs.push(output("normal"));
        }
        assert!(chosen(&one, &replace).is_none(), "several outputs: a material");
    }
}
