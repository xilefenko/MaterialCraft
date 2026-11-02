use super::*;
use crate::paint::effect::{EffectKind, GreyBlend, MaskEffect, MaskPixels, Procedural};
use crate::paint::layer::{LayerKind, PaintContent};
use crate::paint::{BlendMode, Channel, PixelFormat, TiledImage};

const RES: (u32, u32) = (200, 140);

fn img() -> TiledImage {
    TiledImage::new(RES.0, RES.1, PixelFormat::from_id("graya8").unwrap()).unwrap()
}

fn paint_layer(id: &str) -> Layer {
    let mut p = PaintContent::default();
    p.set_image(Channel::Roughness, img());
    Layer::new(id, id, LayerKind::Paint(p)).with_blend(Channel::Roughness, BlendMode::Normal, 1.0)
}

fn doc() -> Document {
    let mut set = TextureSet::new("s", "S", RES).with_channel(Channel::Roughness);
    let group = Layer::new("g", "G", LayerKind::Group(vec![paint_layer("a")]));
    set.layers = vec![paint_layer("base"), group, Layer::new("f", "F", LayerKind::Fill(Fill::uniform(vec![(Channel::Roughness, [0.3; 4])])))];
    let mut d = Document::default();
    d.texture_sets.push(set);
    d
}

fn lr(id: &str) -> LayerRef {
    LayerRef::new("s", id)
}

fn roughness(id: &str) -> ImageRef {
    ImageRef::new(lr(id), LayerImage::Paint(Channel::Roughness))
}

/// Applies `edit`, checks it changed the document, then applies the inverse and checks it is back.
fn round_trip(d: &mut Document, edit: Edit) -> Edit {
    let before = d.clone();
    let inverse = edit.clone().apply(d).unwrap_or_else(|e| panic!("{edit:?}: {e}"));
    assert_ne!(*d, before, "{edit:?} changed nothing");
    let after = d.clone();
    let redo = inverse.clone().apply(d).unwrap();
    assert_eq!(*d, before, "inverse of {edit:?}");
    redo.apply(d).unwrap();
    assert_eq!(*d, after, "redo of {edit:?}");
    after.validate().unwrap();
    inverse
}

fn ids(d: &Document) -> Vec<String> {
    d.texture_sets[0].walk_layers().map(|(depth, l)| format!("{depth}{}", l.id)).collect()
}

#[test]
fn structural_edits_invert() {
    let mut d = doc();
    round_trip(&mut d, Edit::InsertLayer { set: "s".into(), at: LayerSlot::in_group("g", 1), layer: Box::new(paint_layer("b")) });
    assert_eq!(ids(&d), ["0base", "0g", "1a", "1b", "0f"]);
    round_trip(&mut d, Edit::MoveLayer { layer: lr("base"), to: LayerSlot::in_group("g", 1) });
    round_trip(&mut d, Edit::MoveLayer { layer: lr("a"), to: LayerSlot::top(2) });
    round_trip(&mut d, Edit::MoveLayer { layer: lr("f"), to: LayerSlot::top(0) });
    assert_eq!(ids(&d), ["0f", "0g", "1base", "1b", "0a"]);
    round_trip(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::Opacity(0.25) });
    round_trip(&mut d, Edit::SetLayer { layer: lr("g"), property: LayerProperty::Name("Group".into()) });
    round_trip(&mut d, Edit::SetLayer { layer: lr("f"), property: LayerProperty::Fill(Fill::uniform(vec![])) });
    round_trip(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::Mask(Some(Mask::from_image(img()))) });
    round_trip(&mut d, Edit::RemoveLayer(lr("b")));
    round_trip(&mut d, Edit::RemoveLayer(lr("g")));
    assert_eq!(ids(&d), ["0f", "0a"]);
    round_trip(&mut d, Edit::SetHeightFilter(crate::paint::height_normal::HeightFilter::Sobel));
    let mut set2 = TextureSet::new("t", "T", (64, 64));
    round_trip(&mut d, Edit::InsertTextureSet { index: 0, set: Box::new(set2.clone()) });
    round_trip(&mut d, Edit::RemoveTextureSet { set: "s".into() });
    set2.name = "Renamed".into();
    round_trip(&mut d, Edit::ReplaceTextureSet(Box::new(set2)));
    let mut mesh = d.mesh.clone();
    mesh.materials.push("m".into());
    round_trip(&mut d, Edit::SetMesh(Box::new(mesh)));
}

#[test]
fn effect_edits_invert() {
    let mut d = doc();
    let generator = |id: &str| ContentEffect::new(Vec::new(), EffectKind::Generator(Procedural::new(id)));
    round_trip(&mut d, Edit::InsertEffect { layer: lr("a"), index: 0, effect: StackEffect::Content(generator("x")) });
    round_trip(&mut d, Edit::InsertEffect { layer: lr("a"), index: 1, effect: StackEffect::Content(generator("y")) });
    round_trip(&mut d, Edit::MoveEffect { layer: lr("a"), stack: StackId::Content, from: 0, to: 1 });
    round_trip(&mut d, Edit::ReplaceEffect { layer: lr("a"), index: 1, effect: StackEffect::Content(generator("z")) });
    round_trip(&mut d, Edit::RemoveEffect { layer: lr("a"), stack: StackId::Content, index: 0 });
    let mask_paint = StackEffect::Mask(MaskEffect::new(GreyBlend::default(), EffectKind::Paint(MaskPixels::new(img()))));
    assert_eq!(
        Edit::InsertEffect { layer: lr("a"), index: 0, effect: mask_paint.clone() }.apply(&mut d),
        Err(EditError::NoMask { set: "s".into(), layer: "a".into() })
    );
    Edit::SetLayer { layer: lr("a"), property: LayerProperty::Mask(Some(Mask::new(1.0))) }.apply(&mut d).unwrap();
    round_trip(&mut d, Edit::InsertEffect { layer: lr("a"), index: 0, effect: mask_paint });
    round_trip(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::MaskEnabled(false) });
    let mask_image = ImageRef::new(lr("a"), LayerImage::Mask { effect: 0 });
    round_trip(&mut d, Edit::WritePixels { image: mask_image, rect: Rect { x: 0, y: 0, w: 2, h: 2 }, pixels: vec![[1.0; 4]; 4] });
}

#[test]
fn bad_edits_leave_the_document_alone() {
    let mut d = doc();
    let before = d.clone();
    let bad = [
        Edit::InsertLayer { set: "s".into(), at: LayerSlot::top(0), layer: Box::new(paint_layer("a")) },
        Edit::InsertLayer { set: "s".into(), at: LayerSlot::top(9), layer: Box::new(paint_layer("new")) },
        Edit::InsertLayer { set: "s".into(), at: LayerSlot::in_group("f", 0), layer: Box::new(paint_layer("new")) },
        Edit::InsertLayer { set: "nope".into(), at: LayerSlot::top(0), layer: Box::new(paint_layer("new")) },
        Edit::RemoveLayer(lr("nope")),
        Edit::MoveLayer { layer: lr("g"), to: LayerSlot::in_group("a", 0) },
        Edit::MoveLayer { layer: lr("g"), to: LayerSlot::in_group("g", 0) },
        Edit::MoveLayer { layer: lr("base"), to: LayerSlot::top(3) },
        Edit::SetLayer { layer: lr("a"), property: LayerProperty::Fill(Fill::uniform(vec![])) },
        Edit::SetLayer { layer: lr("a"), property: LayerProperty::MaskEnabled(true) },
        Edit::RemoveEffect { layer: lr("a"), stack: StackId::Content, index: 0 },
        Edit::MoveEffect { layer: lr("a"), stack: StackId::Content, from: 0, to: 0 },
        Edit::WritePixels { image: roughness("a"), rect: Rect { x: 199, y: 0, w: 2, h: 1 }, pixels: vec![[1.0; 4]; 2] },
        Edit::WritePixels { image: roughness("a"), rect: Rect { x: 0, y: 0, w: 2, h: 1 }, pixels: vec![[1.0; 4]; 3] },
        Edit::WritePixels { image: roughness("f"), rect: Rect { x: 0, y: 0, w: 1, h: 1 }, pixels: vec![[1.0; 4]] },
        Edit::SwapTiles { image: roughness("a"), tiles: vec![TileSwap { tx: 0, ty: 0, tile: None }, TileSwap { tx: 7, ty: 0, tile: None }] },
        Edit::InsertTextureSet { index: 0, set: Box::new(TextureSet::new("s", "dup", RES)) },
        Edit::InsertTextureSet { index: 5, set: Box::new(TextureSet::new("t", "T", RES)) },
        Edit::ReplaceTextureSet(Box::new(TextureSet::new("t", "T", RES))),
    ];
    for edit in bad {
        assert!(edit.clone().apply(&mut d).is_err(), "{edit:?}");
        assert_eq!(d, before, "{edit:?}");
    }
}

#[test]
fn pixel_writes_swap_only_touched_tiles() {
    let mut d = doc();
    let edit = Edit::WritePixels { image: roughness("a"), rect: Rect { x: 120, y: 10, w: 20, h: 3 }, pixels: vec![[1.0; 4]; 60] };
    let inverse = round_trip(&mut d, edit);
    let Edit::SwapTiles { tiles, .. } = &inverse else { panic!("{inverse:?}") };
    let coords: Vec<_> = tiles.iter().map(|t| (t.tx, t.ty)).collect();
    assert_eq!(coords, [(0, 0), (1, 0)]);
    assert!(tiles.iter().all(|t| t.tile.is_none()), "both tiles were unallocated");
    let set = d.texture_set("s").unwrap();
    let a = set.layer("a").unwrap().images()[0].1;
    assert_eq!(a.get(127, 10), [1.0; 4]);
    assert_eq!(a.get(119, 10), [0.0; 4]);
    let mut count = 0;
    inverse.for_each_tile(&mut |_| count += 1);
    assert_eq!(count, 0);
}

#[test]
fn capture_records_direct_painting() {
    let mut d = doc();
    Edit::WritePixels { image: roughness("a"), rect: Rect { x: 0, y: 0, w: 1, h: 1 }, pixels: vec![[0.2; 4]] }.apply(&mut d).unwrap();
    let before = d.clone();
    let capture = TileCapture::begin(&d, roughness("a")).unwrap();
    let image = d.texture_set_mut("s").unwrap().layer_mut("a").unwrap().image_mut(LayerImage::Paint(Channel::Roughness)).unwrap();
    image.set(3, 3, [0.9; 4]);
    image.set(150, 130, [0.9; 4]);
    let inverse = capture.finish(&d).unwrap();
    let mut held = Vec::new();
    inverse.for_each_tile(&mut |t| held.push(t.clone()));
    assert_eq!(held.len(), 1, "the old tile (0, 0); tile (1, 1) was unallocated");
    inverse.apply(&mut d).unwrap();
    assert_eq!(d, before);
    assert!(TileCapture::begin(&d, roughness("f")).is_err());
}

#[test]
fn absorb_keeps_the_oldest_state() {
    let mut d = doc();
    let mut first = Edit::SetLayer { layer: lr("a"), property: LayerProperty::Opacity(0.9) }.apply(&mut d).unwrap();
    let second = Edit::SetLayer { layer: lr("a"), property: LayerProperty::Opacity(0.5) }.apply(&mut d).unwrap();
    assert_eq!(first.absorb(second), None);
    let other = Edit::SetLayer { layer: lr("a"), property: LayerProperty::Visible(false) }.apply(&mut d).unwrap();
    assert!(first.absorb(other.clone()).is_some());
    other.apply(&mut d).unwrap();
    first.apply(&mut d).unwrap();
    assert_eq!(d, doc());

    let dab = |x: u32| Edit::WritePixels { image: roughness("a"), rect: Rect { x, y: 0, w: 1, h: 1 }, pixels: vec![[1.0; 4]] };
    let mut stroke = dab(0).apply(&mut d).unwrap();
    for x in [5, 130, 10] {
        assert_eq!(stroke.absorb(dab(x).apply(&mut d).unwrap()), None);
    }
    let Edit::SwapTiles { tiles, .. } = &stroke else { panic!() };
    assert_eq!(tiles.len(), 2);
    stroke.apply(&mut d).unwrap();
    assert_eq!(d, doc());
}

fn locked(d: &mut Document, id: &str) {
    let lock = crate::paint::LayerLock::new("test import", vec!["procedural".into()]);
    round_trip(d, Edit::SetLayer { layer: lr(id), property: LayerProperty::Lock(Some(lock)) });
}

fn refused_as_locked(d: &mut Document, edit: Edit) {
    let before = d.clone();
    match edit.clone().apply(d) {
        Err(EditError::Locked { why, .. }) => assert!(why.contains("test import"), "{why}"),
        other => panic!("{edit:?} on a locked layer: {other:?}"),
    }
    assert_eq!(*d, before, "{edit:?} left the document as it was");
}

#[test]
fn a_locked_layer_refuses_content_edits() {
    let mut d = doc();
    round_trip(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::Mask(Some(Mask::from_image(img()))) });
    locked(&mut d, "a");
    locked(&mut d, "f");
    let px = Edit::WritePixels { image: roughness("a"), rect: Rect { x: 0, y: 0, w: 1, h: 1 }, pixels: vec![[1.0; 4]] };
    refused_as_locked(&mut d, px);
    refused_as_locked(&mut d, Edit::SwapTiles { image: roughness("a"), tiles: Vec::new() });
    refused_as_locked(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::Mask(None) });
    refused_as_locked(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::MaskInitial(0.5) });
    refused_as_locked(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::GeometryMask(None) });
    refused_as_locked(&mut d, Edit::SetLayer { layer: lr("f"), property: LayerProperty::Fill(Fill::uniform(vec![])) });
    refused_as_locked(&mut d, Edit::RemoveEffect { layer: lr("a"), stack: StackId::Content, index: 0 });
    refused_as_locked(&mut d, Edit::MoveEffect { layer: lr("a"), stack: StackId::Mask, from: 0, to: 0 });
}

#[test]
fn a_locked_layer_can_still_be_toggled_arranged_and_unlocked() {
    let mut d = doc();
    round_trip(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::Mask(Some(Mask::from_image(img()))) });
    locked(&mut d, "a");
    round_trip(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::Visible(false) });
    round_trip(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::Name("Imported".into()) });
    round_trip(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::Opacity(0.5) });
    round_trip(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::Blend(Vec::new()) });
    round_trip(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::MaskEnabled(false) });
    round_trip(&mut d, Edit::MoveLayer { layer: lr("a"), to: LayerSlot::top(0) });
    // Unlocking is an edit like any other; then the content can change again.
    let relock = round_trip(&mut d, Edit::SetLayer { layer: lr("a"), property: LayerProperty::Lock(None) });
    assert!(matches!(relock, Edit::SetLayer { property: LayerProperty::Lock(Some(_)), .. }));
    round_trip(&mut d, Edit::WritePixels { image: roughness("a"), rect: Rect { x: 0, y: 0, w: 1, h: 1 }, pixels: vec![[1.0; 4]] });
    round_trip(&mut d, Edit::RemoveLayer(lr("a")));
}
