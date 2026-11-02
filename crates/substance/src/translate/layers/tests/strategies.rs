//! The per-set strategy: arranging Painter's result, defaults, cache disagreement and hidden masks.

use super::fixtures::*;
use crate::translate::layers::cache_agreement::CacheAgreement;
use crate::translate::layers::cache_score::Score;
use crate::translate::layers::strategy::provides_base;
use crate::translate::layers::{REFERENCE_NAME, StackParts, StackStrategy};
use materialcraft::paint::{Channel, Fill, Layer, LayerKind};

fn plain(id: &str) -> Layer {
    Layer::new(id, id, LayerKind::Fill(Fill::uniform(vec![])))
}

#[test]
fn strategies_arrange_the_painter_result_and_defaults() {
    let parts = || StackParts {
        imported: vec![plain("a")],
        painter_result: Some(plain("result")),
        defaults: plain("defaults"),
        uncomputable_masks: vec![],
    };
    let ids = |layers: &[Layer]| layers.iter().map(|l| l.id.clone()).collect::<Vec<_>>();
    for strategy in [
        StackStrategy::RealStack,
        StackStrategy::Replacements,
        StackStrategy::PainterResultOnTop,
        StackStrategy::CacheDisagrees,
    ] {
        let layers = strategy.assemble(parts(), &channels());
        assert_eq!(ids(&layers), ["defaults", "a", "result"], "{strategy:?}");
        assert_eq!(
            (layers[2].name.as_str(), layers[2].visible),
            (REFERENCE_NAME, false),
            "{strategy:?}: Painter's result is the hidden reference"
        );
    }
    assert!(StackStrategy::PainterResultOnTop.corrects() && StackStrategy::CacheDisagrees.corrects());
    assert!(!StackStrategy::RealStack.corrects() && !StackStrategy::Replacements.corrects());
    assert_eq!(
        ids(&StackStrategy::BestEffort.assemble(parts(), &channels())),
        ["defaults", "a"]
    );
}

#[test]
fn a_full_uniform_base_replaces_the_defaults() {
    let all = vec![
        uniform(BASE, [0.5; 3]),
        uniform(HEIGHT, [0.0; 3]),
        uniform(ROUGH, [0.5; 3]),
    ];
    let stack = import(vec![layer(1, "Base", vec![fill(all)], vec![blend(HEIGHT, 2, 1.0)])]);
    assert!(provides_base(&stack.layers, &channels()));
    let parts = StackParts {
        imported: stack.layers,
        painter_result: None,
        defaults: plain("defaults"),
        uncomputable_masks: vec![],
    };
    let layers = StackStrategy::RealStack.assemble(parts, &channels());
    assert_eq!(layers.len(), 1);
}

#[test]
fn a_disagreeing_cache_is_corrected() {
    let stack = import(vec![layer(
        1,
        "Base",
        vec![fill(vec![uniform(BASE, [0.5; 3])])],
        vec![],
    )]);
    let stale = CacheAgreement {
        disagreeing: vec![(
            Channel::BaseColor,
            Score {
                mean: 147.0,
                max: 188.0,
            },
        )],
    };
    let strategy = StackStrategy::choose(&stack.verdict, true, &stale);
    assert_eq!(strategy, StackStrategy::CacheDisagrees);
    assert!(strategy.corrects());
    let parts = StackParts {
        imported: stack.layers,
        painter_result: Some(plain("result")),
        defaults: plain("defaults"),
        uncomputable_masks: vec![],
    };
    let layers = strategy.assemble(parts, &channels());
    let top = layers.last().unwrap();
    assert_eq!((top.name.as_str(), top.visible), (REFERENCE_NAME, false));
}

#[test]
fn layers_with_uncomputable_masks_are_hidden_and_locked() {
    let mut masked = layer(2, "Dirt", vec![fill(vec![uniform(BASE, [0.1; 3])])], vec![]);
    masked.mask = Some(crate::model::PainterMask {
        enabled: true,
        initial: crate::model::MaskInitial::Black,
        effects: vec![fill(vec![procedural(0, "mg_dirt")])],
    });
    let inner = folder(3, "Folder", vec![masked], vec![]);
    let stack = import(vec![
        layer(1, "Base", vec![fill(vec![uniform(BASE, [0.5; 3])])], vec![]),
        inner,
    ]);
    assert_eq!(stack.verdict.mask_failures.len(), 1);
    assert_eq!(stack.verdict.mask_failures[0].path, "Folder / Dirt");
    let parts = |layers: Vec<Layer>| StackParts {
        uncomputable_masks: stack.verdict.mask_failures.iter().map(|f| f.id.clone()).collect(),
        imported: layers,
        painter_result: Some(plain("result")),
        defaults: plain("defaults"),
    };
    for strategy in [StackStrategy::BestEffort, StackStrategy::PainterResultOnTop] {
        let layers = strategy.assemble(parts(stack.layers.clone()), &channels());
        let dirt = &layers[2].children()[0];
        assert!(!dirt.visible, "{strategy:?}: it would cover the whole set");
        assert!(dirt.lock.is_some(), "{strategy:?}: locked");
    }
}
