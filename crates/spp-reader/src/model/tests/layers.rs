//! Layer kinds, nesting, ordering, blending and robustness of the builder.

use super::doc::*;
use crate::model::*;

fn build(bytes: &[u8]) -> (PainterDocument, ModelReport) {
    PainterDocument::from_bytes(bytes).expect("model")
}

#[test]
fn reads_the_texture_set() {
    let (doc, report) = build(&single_set(vec![]));
    assert_eq!(doc.format_version, 78);
    let set = &doc.texture_sets[0];
    assert_eq!(
        (set.name.as_str(), set.material_uid, set.uid),
        ("Vase", Some(77), Some(2))
    );
    assert_eq!(set.channels[0].channel, PainterChannel::BaseColor);
    assert!(set.layers.is_empty());
    assert_eq!(report.raw(), 0);
    assert_eq!(report.not_modelled(), 0);
}

#[test]
fn layer_kinds_follow_the_first_content_effect() {
    let paint_group = obj(
        "DataActionGroup",
        vec![
            ("enabled", V::Bool(true)),
            ("perChannelBlending", V::List(vec![blending(u64::MAX, 25, 1.0)])),
            ("subStack", stack("DataStackActions", 41, vec![])),
        ],
    );
    let (doc, _) = build(&single_set(vec![
        layer(10, "Paint", vec![paint_group], vec![], None),
        layer(20, "Fill", vec![fill(25, vec![uniform(1, [0.5; 3])])], vec![], None),
        layer(30, "Empty", vec![], vec![], None),
    ]));
    let layers = &doc.texture_sets[0].layers;
    let kinds: Vec<&str> = layers.iter().map(|l| l.kind.name()).collect();
    assert_eq!(kinds, ["paint", "fill", "paint"]);
    assert_eq!(layers[0].name, "Paint", "stored order (bottom → top) is kept");
    let PainterEffect::Group(g) = &layers[0].content[0] else {
        panic!("group")
    };
    assert_eq!(g.header.blending[0].mode, PainterBlendMode::Passthrough);
    assert!(layers[1].mask.is_none());
}

#[test]
fn folders_nest_with_their_children() {
    let inner = folder(100, "Inner", vec![layer(110, "Deep", vec![], vec![], None)]);
    let outer = folder(200, "Outer", vec![layer(210, "A", vec![], vec![], None), inner]);
    let (doc, _) = build(&single_set(vec![outer]));
    let set = &doc.texture_sets[0];
    let outer = &set.layers[0];
    assert!(!outer.visible);
    assert_eq!(outer.children().len(), 2);
    assert_eq!(outer.children()[1].children()[0].name, "Deep");
    let names: Vec<&str> = set.all_layers().iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Outer", "A", "Inner", "Deep"]);
    let summary = StackSummary::of(&doc);
    assert_eq!((summary.layers, summary.folders, summary.paint_layers), (4, 2, 2));
}

#[test]
fn blending_entries_keep_modes_opacity_and_masks() {
    let blend = vec![
        blending(1 | 16777216, 4, 0.5),
        blending(2, 11, 1.0),
        blending(4194304, 27, 1.0),
    ];
    let (doc, _) = build(&single_set(vec![layer(10, "L", vec![], blend, None)]));
    let b = &doc.texture_sets[0].layers[0].blending;
    assert_eq!(b.len(), 3);
    assert_eq!(b[0].mode, PainterBlendMode::Multiply);
    assert_eq!(b[0].opacity, 0.5);
    assert!(b[0].channels.contains(PainterChannel::Diffuse));
    assert_eq!(b[1].mode, PainterBlendMode::Unknown(11));
    assert!(!b[1].is_default());
    assert!(b[2].is_default());
    assert_eq!(
        blending_for(b, PainterChannel::Height).map(|e| e.mode),
        Some(PainterBlendMode::Unknown(11))
    );
}

#[test]
fn unknown_classes_are_kept_raw_and_counted() {
    let future_layer = obj("DataLayerInstance", vec![("label", V::Str("I")), ("uid", V::Uid(5))]);
    let future_effect = obj("DataActionHologram", vec![("uid", V::Uid(6))]);
    let future_source = obj("DataSourceVideo", vec![("uid", V::Uid(7))]);
    let (doc, report) = build(&single_set(vec![
        future_layer,
        layer(
            10,
            "L",
            vec![future_effect, fill(20, vec![future_source])],
            vec![],
            None,
        ),
    ]));
    let layers = &doc.texture_sets[0].layers;
    assert_eq!(layers[0].kind, LayerKind::Other("DataLayerInstance".into()));
    assert_eq!(layers[0].name, "I");
    let PainterEffect::Other(raw) = &layers[1].content[0] else {
        panic!("raw effect")
    };
    assert_eq!((raw.class.as_str(), raw.uid), ("DataActionHologram", Some(6)));
    let PainterEffect::Fill(f) = &layers[1].content[1] else {
        panic!("fill")
    };
    assert!(matches!(&f.sources[0], PainterSource::Other(r) if r.class == "DataSourceVideo"));
    assert_eq!(report.raw(), 3);
    let raw_classes: Vec<&str> = report.raw_classes().iter().map(|(c, _)| *c).collect();
    assert_eq!(
        raw_classes,
        ["DataActionHologram", "DataLayerInstance", "DataSourceVideo"]
    );
}

#[test]
fn another_root_class_gives_an_empty_model() {
    let bytes = document(2, obj("DataLayersStackState", vec![("uid", V::Uid(1))]));
    let (doc, report) = build(&bytes);
    assert!(doc.texture_sets.is_empty());
    assert_eq!(report.raw(), 1);
}

#[test]
fn deep_folder_nesting_is_cut_not_followed() {
    let mut v = layer(1, "bottom", vec![], vec![], None);
    for _ in 0..70 {
        v = folder(9, "f", vec![v]);
    }
    let (doc, report) = build(&single_set(vec![v]));
    assert_eq!(report.depth_limited, 1);
    let deepest = doc.texture_sets[0].all_layers().len();
    assert_eq!(deepest, 65, "64 nested folders plus the one kept raw");
}

#[test]
fn every_truncation_is_an_error_not_a_panic() {
    let bytes = single_set(vec![layer(
        10,
        "L",
        vec![fill(20, vec![uniform(1, [0.1, 0.2, 0.3])])],
        vec![],
        None,
    )]);
    for len in 0..bytes.len() {
        assert!(PainterDocument::from_bytes(&bytes[..len]).is_err(), "length {len}");
    }
    assert!(PainterDocument::from_bytes(&bytes).is_ok());
}
