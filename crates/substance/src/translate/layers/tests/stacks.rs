//! Layer structure, values, blending and the per-set strategy.

use super::fixtures::*;
use crate::model::LayerKind as PainterKind;
use crate::translate::layers::StackStrategy;
use crate::translate::layers::cache_agreement::CacheAgreement;
use crate::translate::layers::strategy::provides_base;
use materialcraft::paint::working_space::{linear_to_srgb, srgb_to_linear};
use materialcraft::paint::{BlendMode, Channel, LayerKind};

fn agrees() -> CacheAgreement {
    CacheAgreement::default()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 2e-3
}

#[test]
fn uniform_fills_become_fill_layers_in_stored_encoding() {
    let base = layer(
        10,
        "Base",
        vec![fill(vec![uniform(BASE, [0.5; 3]), uniform(ROUGH, [0.3; 3])])],
        vec![],
    );
    let top = layer(11, "Top", vec![fill(vec![uniform(BASE, [0.2, 0.4, 0.6])])], vec![]);
    let stack = import(vec![base, top]);
    assert_eq!(stack.layers.len(), 2);
    assert_eq!(
        (stack.layers[0].id.as_str(), stack.layers[0].name.as_str()),
        ("layer-10", "Base")
    );
    let LayerKind::Fill(f) = &stack.layers[0].kind else {
        panic!("fill")
    };
    assert!(close(
        f.uniform_value(Channel::BaseColor).unwrap()[0],
        linear_to_srgb(0.5)
    ));
    assert_eq!(f.uniform_value(Channel::Roughness).unwrap()[0], 0.3);
    assert!(
        stack.layers[1].blend_for(Channel::Roughness).is_none(),
        "top layer does not write roughness"
    );
    assert!(stack.verdict.all_exact());
    assert_eq!(
        StackStrategy::choose(&stack.verdict, true, &agrees()),
        StackStrategy::RealStack
    );
    let px = composite(&stack, Channel::BaseColor, 3, 3);
    assert!(close(px[2], linear_to_srgb(0.6)), "{px:?}");
    assert!(!provides_base(&stack.layers, &channels()), "height is not covered");
}

#[test]
fn opacity_and_identified_modes_blend_in_linear_light() {
    let base = layer(1, "Base", vec![fill(vec![uniform(BASE, [0.5; 3])])], vec![]);
    let multiply = layer(
        2,
        "Dark",
        vec![fill(vec![uniform(BASE, [0.5; 3])])],
        vec![blend(BASE, 4, 0.5)],
    );
    let stack = import(vec![base, multiply]);
    let b = stack.layers[1].blend_for(Channel::BaseColor).unwrap();
    assert_eq!((b.mode, b.opacity), (BlendMode::Multiply, 0.5));
    let px = composite(&stack, Channel::BaseColor, 0, 0);
    assert!(close(srgb_to_linear(px[0]), 0.375), "{px:?}");
}

#[test]
fn height_defaults_to_adding_and_disable_leaves_a_channel_out() {
    let a = layer(1, "A", vec![fill(vec![uniform(HEIGHT, [0.25; 3])])], vec![]);
    let b = layer(2, "B", vec![fill(vec![uniform(HEIGHT, [0.5; 3])])], vec![]);
    let off = layer(
        3,
        "Off",
        vec![fill(vec![uniform(HEIGHT, [0.9; 3])])],
        vec![blend(HEIGHT, 0, 1.0)],
    );
    let stack = import(vec![a, b, off]);
    assert!(close(composite(&stack, Channel::Height, 1, 1)[0], 0.75));
    assert_eq!(
        stack.layers[2].blend_for(Channel::Height).unwrap().mode,
        BlendMode::Disable
    );
}

#[test]
fn folders_keep_nesting_and_passthrough_or_isolation() {
    let child = layer(5, "Child", vec![fill(vec![uniform(BASE, [1.0; 3])])], vec![]);
    let pass = folder(4, "Pass", vec![child.clone()], vec![blend(ALL, 25, 1.0)]);
    let iso = folder(6, "Iso", vec![layer(7, "Child", vec![], vec![])], vec![]);
    let stack = import(vec![pass, iso]);
    assert_eq!(stack.layers[0].children()[0].id, "layer-5");
    assert_eq!(
        stack.layers[0].blend_for(Channel::BaseColor).unwrap().mode,
        BlendMode::Passthrough
    );
    assert_eq!(
        stack.layers[1].blend_for(Channel::BaseColor).unwrap().mode,
        BlendMode::Normal
    );
    assert_eq!(stack.verdict.layer_count, 4);
    assert!(close(composite(&stack, Channel::BaseColor, 0, 0)[0], 1.0));
}

#[test]
fn uncomputable_layers_are_kept_with_reasons_and_hidden_ones_do_not_count() {
    let base = layer(1, "Base", vec![fill(vec![uniform(BASE, [0.5; 3])])], vec![]);
    let mut hidden = layer(2, "Hidden rust", vec![fill(vec![procedural(ALL, "rust")])], vec![]);
    hidden.visible = false;
    let stack = import(vec![base.clone(), hidden]);
    assert!(stack.verdict.all_exact(), "hidden layers do not count");
    assert_eq!(stack.verdict.layers.len(), 1);
    assert!(!stack.verdict.layers[0].visible);
    assert!(!stack.layers[1].visible);

    let rust = layer(3, "Rust", vec![fill(vec![procedural(BASE | ROUGH, "rust")])], vec![]);
    let stack = import(vec![base, rust]);
    let base_status = stack.verdict.channel(Channel::BaseColor);
    assert!(!base_status.is_computable());
    assert_eq!(base_status.reasons(), ["procedural fill \"rust\""]);
    assert!(stack.verdict.channel(Channel::Height).is_exact());
    assert_eq!(stack.verdict.layers[0].path, "Rust");
    assert!(stack.layers[1].blend_for(Channel::Roughness).is_some(), "blending kept");
    let lock = stack.layers[1]
        .lock
        .as_ref()
        .expect("a layer that can't be computed is locked");
    assert_eq!(lock.reasons.len(), 1);
    assert_eq!(
        (lock.reasons[0].key.as_str(), lock.reasons[0].text.as_str()),
        ("substance:rust", "procedural fill \"rust\"")
    );
    assert!(stack.layers[0].lock.is_none(), "an exact layer stays editable");
    assert_eq!(
        StackStrategy::choose(&stack.verdict, true, &agrees()),
        StackStrategy::Corrected
    );
    assert_eq!(
        StackStrategy::choose(&stack.verdict, false, &agrees()),
        StackStrategy::BestEffort
    );
}

#[test]
fn guessed_modes_are_approximated_and_recorded() {
    let base = layer(1, "Base", vec![fill(vec![uniform(BASE, [0.5; 3])])], vec![]);
    let odd = layer(
        2,
        "Odd",
        vec![fill(vec![uniform(BASE, [0.5; 3])])],
        vec![blend(BASE, 11, 1.0)],
    );
    let stack = import(vec![base, odd]);
    let status = stack.verdict.channel(Channel::BaseColor);
    assert!(status.is_computable() && !status.is_exact());
    assert_eq!(
        stack.verdict.blend_values["base_color"]
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [2, 11]
    );
    assert_eq!(stack.verdict.layers.len(), 1);
    assert!(
        stack.layers.iter().all(|l| l.lock.is_none()),
        "approximated layers stay editable"
    );
}

#[test]
fn corrupt_values_and_unknown_classes_do_not_panic() {
    let mut odd = layer(
        1,
        "",
        vec![fill(vec![uniform(BASE, [f32::NAN; 3])])],
        vec![blend(BASE, 99, f32::NAN)],
    );
    odd.kind = PainterKind::Other("DataLayerFuture".into());
    let stack = import(vec![odd.clone(), odd]);
    assert_eq!(stack.layers[1].id, "layer-1-2", "ids stay unique");
    let b = stack.layers[0].blend_for(Channel::BaseColor).unwrap();
    assert_eq!((b.mode, b.opacity), (BlendMode::Normal, 1.0));
    assert!(!stack.verdict.channel(Channel::BaseColor).is_computable());
}
