//! Substance material fills replaced by a material of ours.

use super::fixtures::*;
use crate::model::PainterLayer;
use crate::translate::layers::{BlendTable, ImportedStack, LayerImport, NoImages};
use crate::translate::replacements::ProceduralPlan;
use materialcraft::paint::{Channel, EffectKind, FillSource, LayerKind};

const IRON: &str = "materials/iron-rusted.mca";

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

fn rust_plan() -> ProceduralPlan {
    ProceduralPlan::default().with_material("rust", Some(IRON))
}

#[test]
fn a_picked_material_becomes_a_material_fill_layer_keeping_the_original() {
    let rust = || vec![layer(3, "Rust", vec![fill(vec![procedural(ALL, "rust")])], vec![])];
    // not picked: Painter's result
    let kept = import(rust());
    assert!(!kept.verdict.channel(Channel::BaseColor).is_computable());
    assert!(
        kept.layers[0]
            .effects
            .iter()
            .any(|e| matches!(&e.kind, EffectKind::Generator(p) if p.id == "substance:rust")),
        "{:?}",
        kept.layers[0]
    );
    // picked: a material fill layer, computable as a replacement
    let stack = import_planned(rust(), &rust_plan());
    let LayerKind::Fill(f) = &stack.layers[0].kind else {
        panic!("fill layer expected")
    };
    assert_eq!(
        f.source,
        FillSource::Material {
            path: IRON.into(),
            params: vec![]
        },
        "{:?}",
        stack.layers[0]
    );
    assert_eq!(f.original.as_ref().map(|o| o.id.as_str()), Some("substance:rust"));
    assert!(stack.layers[0].effects.is_empty());
    for (channel, status) in stack.verdict.channels() {
        assert!(
            matches!(status, crate::translate::layers::Computability::Replaced(_)),
            "{channel:?}: {status:?}"
        );
    }
    assert!(stack.verdict.all_exact_or_replaced());
}

#[test]
fn a_picked_material_next_to_constants_is_a_material_fill_effect() {
    // roughness from a constant, base colour from the material
    let layers = vec![layer(
        3,
        "Rust",
        vec![fill(vec![uniform(ROUGH, [0.3; 3]), procedural(BASE, "rust")])],
        vec![],
    )];
    let stack = import_planned(layers, &rust_plan());
    let l = &stack.layers[0];
    assert!(matches!(&l.kind, LayerKind::Fill(f) if f.uniform_value(Channel::Roughness).is_some()));
    let materials: Vec<_> = l
        .effects
        .iter()
        .filter(|e| matches!(&e.kind, EffectKind::Fill(f) if matches!(f.source, FillSource::Material { .. })))
        .collect();
    assert_eq!(materials.len(), 1);
    assert_eq!(
        materials[0].blend.iter().map(|b| b.channel).collect::<Vec<_>>(),
        vec![Channel::BaseColor]
    );
    assert!(stack.verdict.all_exact_or_replaced());
}
