//! Every layer panel request applied to an open document: one history step each, undone to the identical
//! document; a slider drag is one step.

use super::*;
use materialcraft::history::CoalesceKey;
use materialcraft::paint::edit::ops::{AssetEffect, DropPayload, DropTarget, EffectAsset, MaskStart, NewEffect, NewLayer};
use materialcraft::paint::edit::{LayerSlot, StackId};
use materialcraft::paint::effect::Procedural;
use materialcraft::paint::{Document, Fill, GeometryMask, LayerKind, PaintContent, TextureSet};
use std::path::PathBuf;

fn open() -> OpenDocument {
    let l = |id: &str| Layer::new(id, id, LayerKind::Paint(PaintContent::default())).with_blend(Channel::Roughness, BlendMode::Normal, 1.0);
    let mut set = TextureSet::new("s", "S", (64, 64)).with_channel(Channel::Roughness);
    set.layers = vec![l("base"), Layer::new("g", "G", LayerKind::Group(vec![l("a")])).with_blend(Channel::Roughness, BlendMode::Normal, 1.0), l("top")];
    set.layers.push(Layer::new("f", "F", LayerKind::Fill(Fill::uniform(vec![(Channel::Roughness, [0.2; 4])]))).with_blend(
        Channel::Roughness,
        BlendMode::Normal,
        1.0,
    ));
    let mut doc = Document::default();
    doc.texture_sets.push(set);
    OpenDocument::new(doc, PathBuf::from("test.mcpaint"), None)
}

fn lr(id: &str) -> LayerRef {
    LayerRef::new("s", id)
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn every_panel_request_is_one_undoable_step() {
    let mut d = open();
    let blur = DropPayload::Effect(EffectAsset { name: "Blur".into(), kind: AssetEffect::Filter(Procedural::new("blur")) });
    let events = vec![
        LayerEvent::Create { set: "s".into(), what: NewLayer::Paint, above: Some("a".into()) },
        LayerEvent::Create { set: "s".into(), what: NewLayer::Fill, above: None },
        LayerEvent::Create { set: "s".into(), what: NewLayer::Folder, above: None },
        LayerEvent::Duplicate { set: "s".into(), ids: ids(&["g", "top"]) },
        LayerEvent::Delete { set: "s".into(), ids: ids(&["f"]) },
        LayerEvent::Group { set: "s".into(), ids: ids(&["base", "top"]) },
        LayerEvent::Move { set: "s".into(), ids: ids(&["top", "base"]), to: LayerSlot::in_group("g", 0), copy: false },
        LayerEvent::Move { set: "s".into(), ids: ids(&["top"]), to: LayerSlot::top(0), copy: true },
        LayerEvent::Rename(lr("a"), "Scratches".into()),
        LayerEvent::SetVisible(lr("a"), false),
        LayerEvent::SetOpacity { layer: lr("a"), channel: Channel::Roughness, opacity: 0.4, key: CoalesceKey(1) },
        LayerEvent::SetBlendMode { layer: lr("a"), channel: Channel::Roughness, mode: BlendMode::Multiply },
        LayerEvent::AddEffect { layer: lr("a"), stack: StackId::Content, what: NewEffect::Levels },
        LayerEvent::AddMask(lr("a"), MaskStart::Black),
        LayerEvent::SetGeometryMask(lr("a"), Some(GeometryMask::Parts(ids(&["body"])))),
        LayerEvent::SetGeometryMask(lr("g"), Some(GeometryMask::nothing())),
        LayerEvent::Drop { target: DropTarget::Between { set: "s".into(), at: LayerSlot::top(1) }, payload: blur.clone(), replace: false },
        LayerEvent::Drop { target: DropTarget::OntoLayer(lr("top")), payload: blur, replace: true },
    ];
    for ev in events {
        let before = d.doc().clone();
        let steps = d.history().len();
        let applied = apply(&mut d, ev.clone()).unwrap_or_else(|e| panic!("{ev:?}: {e}"));
        assert_eq!(applied.set, Some(0), "{ev:?}");
        assert_eq!(d.history().len(), steps + 1, "{ev:?} is one step");
        d.close_step();
        let after = d.doc().clone();
        assert_ne!(after, before, "{ev:?} changed nothing");
        assert!(d.undo().unwrap());
        assert_eq!(*d.doc(), before, "undo of {ev:?}");
        assert!(d.redo().unwrap());
        assert_eq!(*d.doc(), after, "redo of {ev:?}");
        after.validate().unwrap();
    }
    // Effects and masks on a layer that has them.
    let with_mask = vec![
        LayerEvent::SetEffectEnabled { layer: lr("a"), stack: StackId::Content, index: 0, enabled: false },
        LayerEvent::AddEffect { layer: lr("a"), stack: StackId::Mask, what: NewEffect::Paint },
        LayerEvent::RemoveEffect { layer: lr("a"), stack: StackId::Mask, index: 0 },
        LayerEvent::SetMaskEnabled(lr("a"), false),
        LayerEvent::InvertMask(lr("a")),
        LayerEvent::RemoveMask(lr("a")),
    ];
    for ev in with_mask {
        let before = d.doc().clone();
        apply(&mut d, ev.clone()).unwrap_or_else(|e| panic!("{ev:?}: {e}"));
        assert!(d.undo().unwrap());
        assert_eq!(*d.doc(), before, "undo of {ev:?}");
        d.redo().unwrap();
    }
    // Going all the way back gives the opened document.
    d.jump_to(0).unwrap();
    assert_eq!(*d.doc(), *open().doc());
}

#[test]
fn an_opacity_drag_is_one_step_and_failures_record_nothing() {
    let mut d = open();
    let before = d.doc().clone();
    for v in [0.9, 0.7, 0.3] {
        apply(&mut d, LayerEvent::SetOpacity { layer: lr("top"), channel: Channel::Roughness, opacity: v, key: CoalesceKey(7) }).unwrap();
    }
    apply(&mut d, LayerEvent::EndGesture).unwrap();
    assert_eq!(d.history().len(), 1);
    assert_eq!(d.doc().texture_sets[0].layer("top").unwrap().blend[0].opacity, 0.3);
    d.undo().unwrap();
    assert_eq!(*d.doc(), before);
    // A channel the layer does not affect, a move into itself: refused, nothing recorded.
    assert!(apply(&mut d, LayerEvent::SetOpacity { layer: lr("top"), channel: Channel::Height, opacity: 0.1, key: CoalesceKey(8) }).is_err());
    assert!(apply(&mut d, LayerEvent::Move { set: "s".into(), ids: ids(&["g"]), to: LayerSlot::in_group("g", 0), copy: false }).is_err());
    assert_eq!(*d.doc(), before);
    assert_eq!(d.history().done(), 0);
    // A drop where the layers already are is no step.
    apply(&mut d, LayerEvent::Move { set: "s".into(), ids: ids(&["top"]), to: LayerSlot::top(2), copy: false }).unwrap();
    assert_eq!(d.history().done(), 0);
    assert_eq!(apply(&mut d, LayerEvent::Isolate(Some(lr("a")))).unwrap(), Applied::default());
}

#[test]
fn each_geometry_mask_change_is_its_own_step() {
    let mut d = open();
    let masks = [Some(GeometryMask::Parts(ids(&["body", "trim"]))), Some(GeometryMask::Parts(ids(&["body"]))), None];
    for m in masks.iter().cloned() {
        apply(&mut d, LayerEvent::SetGeometryMask(lr("top"), m)).unwrap();
    }
    assert_eq!(d.history().len(), 3, "toggling parts one after another is not coalesced");
    d.undo().unwrap();
    assert_eq!(d.doc().texture_sets[0].layer("top").unwrap().geometry_mask, masks[1]);
    assert_eq!(apply(&mut d, LayerEvent::EditGeometryMask(Some(lr("top")))).unwrap(), Applied::default(), "edit mode is view state");
}

#[test]
fn a_locked_layer_toggles_refuses_content_changes_and_unlocks_in_one_undoable_step() {
    let mut doc = open().doc().clone();
    if let Some(l) = doc.texture_sets[0].layer_mut("a") {
        l.lock = Some(materialcraft::paint::LayerLock::new("test import", vec!["procedural".into()]));
    }
    let mut d = OpenDocument::new(doc, PathBuf::from("test.mcpaint"), None);
    apply(&mut d, LayerEvent::SetVisible(lr("a"), false)).expect("a locked layer can be hidden");
    let before = d.doc().clone();
    let refused = apply(&mut d, LayerEvent::AddMask(lr("a"), MaskStart::Black));
    assert!(matches!(refused, Err(EditError::Locked { .. })), "{refused:?}");
    assert_eq!(*d.doc(), before, "a refused change leaves the document as it was");
    apply(&mut d, LayerEvent::Unlock(lr("a"))).unwrap();
    assert!(d.doc().texture_sets[0].layer("a").is_some_and(|l| l.lock.is_none()));
    apply(&mut d, LayerEvent::AddMask(lr("a"), MaskStart::Black)).expect("unlocked, the content can change");
    d.undo().unwrap();
    d.undo().unwrap();
    assert_eq!(*d.doc(), before, "Undo locks it again");
}
