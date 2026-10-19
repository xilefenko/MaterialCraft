//! Painting from the 2D view (PT-53): strokes land in texture space where the view shows them, symmetry mirrors them
//! through the mesh (ADR 0008 mc2-q-pt-04c), the polygon fill picks in UV space and the 3D-only tools refuse.

use super::*;
use crate::gui_paint::input::PointerSample;
use crate::gui_paint::view2d::View2d;
use materialcraft::mesh::{Mesh, MeshPart};
use materialcraft::paint::brush::SizeSpace;
use materialcraft::paint::edit::LayerRef;
use materialcraft::paint::{BlendMode, Channel, ChannelSpec, Document, Layer, LayerKind, PaintContent, TextureSet, TiledImage};
use materialcraft_brush::TargetPart;
use std::path::PathBuf;

const RES: u32 = 64;

/// A flat quad from x −1 to 1 whose UVs cover the 0–1 square (u along x, v down along −y), painted by set "s".
fn document() -> OpenDocument {
    let part = MeshPart {
        positions: vec![[-1.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, -1.0, 0.0]],
        triangles: vec![[0, 3, 2], [0, 2, 1]],
        uv_sets: vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]],
        ..MeshPart::default()
    };
    let mut doc = Document::new(Mesh { materials: vec!["M".into()], parts: vec![part] });
    let mut set = TextureSet::new("s", "S", (RES, RES)).with_channel(Channel::BaseColor);
    let image = |c: Channel| TiledImage::new(RES, RES, ChannelSpec::new(c).layer_format()).expect("image");
    let content = PaintContent::from_images(vec![(Channel::BaseColor, image(Channel::BaseColor))]);
    let mut mask = image(Channel::Roughness);
    mask.fill([1.0; 4]);
    let layer = Layer::new("p", "Paint", LayerKind::Paint(content)).with_blend(Channel::BaseColor, BlendMode::Normal, 1.0).with_mask(mask);
    set.layers.push(layer);
    doc.texture_sets.push(set);
    OpenDocument::new(doc, PathBuf::from("uv.mcpaint"), None)
}

fn target(part: TargetPart) -> Option<(PaintTarget, usize)> {
    Some((PaintTarget { layer: LayerRef::new("s", "p"), part }, 0))
}

/// The 2D view at one point per texel with the 0–1 square filling it.
fn map() -> UvMap {
    UvMap { view: View2d { centre: [0.5, 0.5], zoom: RES as f32 }, size: [RES as f32, RES as f32] }
}

fn painting() -> Painting {
    let mut p = Painting::default();
    p.brush.paint.size = 6.0;
    p.brush.paint.size_space = SizeSpace::Texture;
    p.brush.paint.hardness = 1.0;
    p.brush.value_mut(Channel::BaseColor).value = [1.0, 0.0, 0.0, 1.0];
    // a stroke completes within its frames (one test below doesn't wait)
    p.wait_for_stamps = true;
    p
}

/// Waits until the set's surface is built (the brush paints through it).
fn ready(p: &mut Painting, doc: &mut OpenDocument) {
    for _ in 0..500 {
        if p.surface_ready(doc, 0) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the surface was not built");
}

fn stroke(points: &[[f32; 2]]) -> Vec<StrokeInput> {
    let at = |i: usize, p: [f32; 2]| PointerSample::mouse(p, i as f64 * 0.02);
    let mut input = vec![StrokeInput::Down(at(0, points[0]))];
    input.extend(points.iter().enumerate().skip(1).map(|(i, p)| StrokeInput::Move(at(i, *p))));
    input.push(StrokeInput::Up);
    input
}

fn frame<'a>(doc: &'a mut OpenDocument, picking: &'a ViewPicking, tool: Tool, part: TargetPart) -> PaintFrame<'a> {
    PaintFrame { doc, picking, camera: None, tool, target: target(part), uv: Some(map()) }
}

fn red_at(doc: &OpenDocument, x: u32, y: u32) -> f32 {
    let set = &doc.doc().texture_sets[0];
    match &set.layers[0].kind {
        LayerKind::Paint(c) => c.images.iter().find(|(ch, _)| *ch == Channel::BaseColor).map_or(0.0, |(_, img)| img.get(x, y)[3]),
        _ => 0.0,
    }
}

#[test]
fn a_2d_stroke_paints_the_texels_under_it_as_one_history_step() {
    let (mut doc, picking, mut p) = (document(), ViewPicking::default(), painting());
    ready(&mut p, &mut doc);
    let before = doc.history().len();
    let report = p.frame(stroke(&[[10.0, 20.0], [20.0, 20.0], [30.0, 20.0]]), frame(&mut doc, &picking, Tool::Paint, TargetPart::Content));
    assert_eq!(report.message, None);
    assert_eq!(report.refresh, Some(0));
    assert_eq!(doc.history().len(), before + 1, "one stroke, one step");
    assert!(red_at(&doc, 20, 20) > 0.9, "painted under the stroke");
    assert!(red_at(&doc, 20, 26) < 0.01 && red_at(&doc, 50, 20) < 0.01, "and nowhere else");
    // a second view position maps through the zoom: two points per texel, centred the same
    let wide = UvMap { view: View2d { centre: [0.5, 0.5], zoom: 2.0 * RES as f32 }, size: [2.0 * RES as f32, 2.0 * RES as f32] };
    let f = PaintFrame { uv: Some(wide), ..frame(&mut doc, &picking, Tool::Paint, TargetPart::Content) };
    p.frame(stroke(&[[100.0, 100.0], [101.0, 100.0]]), f);
    assert!(red_at(&doc, 50, 50) > 0.9);
}

#[test]
fn symmetry_mirrors_2d_strokes_through_the_mesh() {
    let (mut doc, picking, mut p) = (document(), ViewPicking::default(), painting());
    p.brush.helpers.symmetry.on = true; // mirror across x = 0: u ↔ 1 − u on this quad
    ready(&mut p, &mut doc);
    p.frame(stroke(&[[12.0, 40.0], [13.0, 40.0]]), frame(&mut doc, &picking, Tool::Paint, TargetPart::Content));
    assert!(red_at(&doc, 12, 40) > 0.9, "the stroke");
    assert!(red_at(&doc, RES - 1 - 12, 40) > 0.9, "its mirror image");
    assert!(red_at(&doc, 32, 40) < 0.01);
}

#[test]
fn the_eraser_works_in_the_2d_view_and_3d_only_tools_say_so() {
    let (mut doc, picking, mut p) = (document(), ViewPicking::default(), painting());
    ready(&mut p, &mut doc);
    p.frame(stroke(&[[30.0, 30.0], [34.0, 30.0]]), frame(&mut doc, &picking, Tool::Paint, TargetPart::Content));
    assert!(red_at(&doc, 32, 30) > 0.9);
    p.brush.eraser.size = 10.0;
    p.brush.eraser.size_space = SizeSpace::Texture;
    p.brush.eraser.hardness = 1.0;
    p.frame(stroke(&[[32.0, 30.0], [32.5, 30.0]]), frame(&mut doc, &picking, Tool::Eraser, TargetPart::Content));
    assert!(red_at(&doc, 32, 30) < 0.01, "erased");
    let steps = doc.history().len();
    let report = p.frame(stroke(&[[10.0, 10.0], [12.0, 10.0]]), frame(&mut doc, &picking, Tool::Projection, TargetPart::Content));
    assert!(report.message.is_some_and(|m| m.contains("3D view")));
    assert_eq!(doc.history().len(), steps, "nothing painted");
}

#[test]
fn the_polygon_fill_picks_in_uv_space() {
    let (mut doc, picking, mut p) = (document(), ViewPicking::default(), painting());
    let steps = doc.history().len();
    // a click outside the 0–1 square picks nothing (V2-05)
    let outside = UvMap { view: View2d { centre: [1.5, 0.5], zoom: RES as f32 }, size: [RES as f32, RES as f32] };
    let f = PaintFrame { uv: Some(outside), ..frame(&mut doc, &picking, Tool::PolygonFill, TargetPart::Mask) };
    p.frame(stroke(&[[32.0, 32.0]]), f);
    assert_eq!(doc.history().len(), steps);
    // a click on the quad's lower-left triangle fills it into the layer's mask
    let report = p.frame(stroke(&[[8.0, 56.0]]), frame(&mut doc, &picking, Tool::PolygonFill, TargetPart::Mask));
    assert_eq!(report.message, None);
    assert_eq!(report.refresh, Some(0));
    assert_eq!(doc.history().len(), steps + 1, "one pick, one step");
}

#[test]
fn a_stroke_on_its_worker_completes_after_pen_up_with_the_same_pixels() {
    let points = [[10.0, 20.0], [14.0, 22.0], [20.0, 20.0], [26.0, 24.0], [30.0, 20.0]];
    let (mut waited, picking, mut p) = (document(), ViewPicking::default(), painting());
    ready(&mut p, &mut waited);
    p.frame(stroke(&points), frame(&mut waited, &picking, Tool::Paint, TargetPart::Content));

    let (mut doc, mut p) = (document(), painting());
    p.wait_for_stamps = false;
    ready(&mut p, &mut doc);
    let before = doc.history().len();
    let mut input = stroke(&points);
    let up = input.pop();
    // moves spread over frames, then pen up; the frames don't wait for the worker
    for ev in input {
        p.frame(vec![ev], frame(&mut doc, &picking, Tool::Paint, TargetPart::Content));
    }
    p.frame(up.into_iter().collect(), frame(&mut doc, &picking, Tool::Paint, TargetPart::Content));
    let mut refreshed = None;
    for _ in 0..2000 {
        if !p.is_stroking() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
        refreshed = p.frame(Vec::new(), frame(&mut doc, &picking, Tool::Paint, TargetPart::Content)).refresh.or(refreshed);
    }
    assert!(!p.is_stroking(), "the stroke completes");
    assert_eq!(refreshed, Some(0), "and its set is recomposited");
    assert_eq!(doc.history().len(), before + 1, "one stroke, one step");
    assert!(doc.doc() == waited.doc(), "the same document as a stroke stamped within its frames");
}
