//! Painter fills whose channels come from a procedural texture, replaced by one of our textures.

use super::fixtures::*;
use crate::model::{PainterLayer, PainterSource};
use crate::translate::layers::{BlendTable, Computability, ImportedStack, LayerImport, NoImages};
use crate::translate::replacements::{Choice, ProceduralPlan};
use materialcraft::paint::{Channel, Compositor, EffectKind, FillSource, LayerKind, TextureSet};

fn import_planned(layers: Vec<PainterLayer>, plan: &ProceduralPlan) -> ImportedStack {
    let table = BlendTable::painter();
    LayerImport {
        table: &table,
        projections: &crate::translate::layers::ProjectionTable::painter(),
        images: &NoImages,
        plan,
        replay: Default::default(),
        replay_cache: None,
    }
    .import(&set(layers), &channels(), SIZE)
}

/// A single-output procedural (a texture, not a material) feeding `bits`.
fn texture(bits: u64, name: &str) -> PainterSource {
    let PainterSource::Procedural(mut p) = procedural(bits, name) else {
        unreachable!()
    };
    p.mode = 0;
    PainterSource::Procedural(p)
}

fn replace() -> ProceduralPlan {
    ProceduralPlan::all(Choice::Replace)
}

/// The composite of `channel` with the built-in generators.
fn composite_all(stack: &ImportedStack, channel: Channel) -> Vec<[f32; 4]> {
    let mut set = TextureSet::new("t", "T", SIZE);
    set.channels = channels().specs().to_vec();
    set.layers = stack.layers.clone();
    let c = Compositor::with_effects(&set, materialcraft::generators::builtin());
    assert!(c.warnings(channel).is_empty(), "{:?}", c.warnings(channel));
    c.composite_tex(channel).0.px
}

#[test]
fn a_replaced_texture_becomes_a_texture_fill_layer_keeping_the_original() {
    let perlin = || {
        vec![layer(
            3,
            "Noise",
            vec![fill(vec![texture(ROUGH | HEIGHT, "ratio_perlin_noise")])],
            vec![],
        )]
    };
    // kept: Painter's result
    let kept = import(perlin());
    assert!(!kept.verdict.channel(Channel::Roughness).is_computable());
    // replaced: a texture fill layer on both channels, computable as a replacement
    let stack = import_planned(perlin(), &replace());
    let LayerKind::Fill(f) = &stack.layers[0].kind else {
        panic!("fill layer expected: {:?}", stack.layers[0])
    };
    let FillSource::Texture(t) = &f.source else {
        panic!("texture fill expected: {f:?}")
    };
    assert_eq!(t.channels(), vec![Channel::Height, Channel::Roughness]);
    assert_eq!(t.texture(Channel::Roughness).map(|p| p.id.as_str()), Some("perlin"));
    assert_eq!(
        f.original.as_ref().map(|o| o.id.as_str()),
        Some("substance:ratio_perlin_noise")
    );
    assert!(stack.layers[0].effects.is_empty());
    for channel in [Channel::Roughness, Channel::Height] {
        let status = stack.verdict.channel(channel);
        assert!(matches!(status, Computability::Replaced(_)), "{channel:?}: {status:?}");
        assert!(
            status.reasons().iter().any(|r| r.contains("replaced by perlin")),
            "{status:?}"
        );
    }
    // it composites: the texture varies over the set
    let px = composite_all(&stack, Channel::Roughness);
    assert!(px.iter().any(|p| (p[0] - px[0][0]).abs() > 1e-3));
}

#[test]
fn a_replaced_texture_next_to_constants_is_a_texture_fill_effect() {
    let layers = vec![layer(
        3,
        "Dirt",
        vec![fill(vec![uniform(BASE, [0.3; 3]), texture(ROUGH, "ratio_dirt_3")])],
        vec![],
    )];
    let stack = import_planned(layers, &replace());
    let l = &stack.layers[0];
    assert!(matches!(&l.kind, LayerKind::Fill(f) if f.uniform_value(Channel::BaseColor).is_some()));
    let textures: Vec<_> = l
        .effects
        .iter()
        .filter(|e| matches!(&e.kind, EffectKind::Fill(f) if matches!(f.source, FillSource::Texture(_))))
        .collect();
    assert_eq!(textures.len(), 1, "{l:?}");
    assert_eq!(
        textures[0].blend.iter().map(|b| b.channel).collect::<Vec<_>>(),
        vec![Channel::Roughness]
    );
    assert!(stack.verdict.all_exact_or_replaced());
    composite_all(&stack, Channel::Roughness);
}

#[test]
fn textures_without_a_replacement_and_materials_stay_painters() {
    let plan = replace();
    let unknown = import_planned(
        vec![layer(
            3,
            "Fish",
            vec![fill(vec![texture(ROUGH, "fish_scales")])],
            vec![],
        )],
        &plan,
    );
    let status = unknown.verdict.channel(Channel::Roughness);
    assert!(!status.is_computable());
    assert!(
        status
            .reasons()
            .iter()
            .any(|r| r.contains("procedural fill \"fish_scales\"")),
        "{status:?}"
    );
    // a material (mode 1) of a replaceable name takes the material path: kept without a pick
    let material = import_planned(
        vec![layer(
            3,
            "M",
            vec![fill(vec![procedural(ROUGH, "ratio_perlin_noise")])],
            vec![],
        )],
        &plan,
    );
    assert!(!material.verdict.channel(Channel::Roughness).is_computable());
    assert!(
        material.layers[0]
            .effects
            .iter()
            .any(|e| matches!(&e.kind, EffectKind::Generator(_)))
    );
}
