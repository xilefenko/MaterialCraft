//! [`StampStroke::apply_all`] gives the same bits as applying the dabs one by one, however they are batched:
//! texture space and the 3D view in every placement, with and without symmetry, painting, erasing and cloning.

use super::tests::{camera, disc, image, mesh_of, surface};
use super::*;
use crate::mesh::test_util::quad;
use crate::paint::TiledImage;
use crate::paint::brush::Placement;
use crate::projection::{ViewProjector, ViewScene};

/// A wavy line of overlapping dabs of varying size, flow and opacity, crossing the tile edges at 128.
fn dabs(origin: [f32; 2], scale: f32) -> Vec<DabSample> {
    (0..40)
        .map(|i| {
            let t = i as f32 / 39.0;
            let c = [origin[0] + (t - 0.5) * 150.0 * scale, origin[1] + (t * 9.0).sin() * 30.0 * scale];
            disc(c, (6.0 + 5.0 * (t * 5.0).cos()) * scale, i, 0.3 + 0.5 * t, 0.6 + 0.4 * (1.0 - t))
        })
        .collect()
}

fn target(img: &mut TiledImage, mode: StampMode) -> ChannelTarget<'_> {
    ChannelTarget { image: img, value: [0.9, 0.2, 0.1, 1.0], mode, srgb: false, source: None }
}

/// The image after painting `dabs` in batches of `chunk`, the tiles reported, and the stroke's touched tiles.
/// The image's bytes, the tiles reported, the stroke's touched tiles.
type Painted = (Vec<u8>, Vec<(u32, u32)>, Vec<(u32, u32)>);

fn paint(space: StrokeSpace, spec: SymmetrySpec, mode: StampMode, dabs: &[DabSample], chunk: usize) -> Painted {
    let mesh = mesh_of(vec![quad()]);
    let (map, index) = surface(&mesh);
    // something under the stroke, so erasing and cloning have pixels to work on
    let mut img = image();
    let all = img.all_tiles();
    img.update_tiles(&all, |r, px| px.iter_mut().enumerate().for_each(|(i, p)| *p = [(r.x as f32 + (i % 128) as f32) / 256.0, 0.5, 0.25, 1.0]));
    let ctx = StampContext { surface: &map, index: &index, space, factors: &[] };
    let mut stroke = StampStroke::new(spec, &[target(&mut img, mode)]).unwrap();
    let mut reported = Vec::new();
    for part in dabs.chunks(chunk) {
        reported.extend(stroke.apply_all(&ctx, part, &mut [target(&mut img, mode)]).unwrap());
    }
    reported.sort_unstable();
    reported.dedup();
    (img.to_raw(), reported, stroke.touched())
}

fn same_however_batched(space: StrokeSpace, origin: [f32; 2], scale: f32) {
    let list = dabs(origin, scale);
    let mirror = SymmetrySpec::Mirror { axis: 0, offset: 0.0 };
    for spec in [SymmetrySpec::None, mirror] {
        for mode in [StampMode::Paint, StampMode::Erase, StampMode::Clone { offset: [7, -5] }] {
            let one = paint(space, spec, mode, &list, 1);
            assert!(!one.1.is_empty(), "the stroke paints something");
            for chunk in [2, 7, 16, list.len()] {
                let batched = paint(space, spec, mode, &list, chunk);
                assert!(one.0 == batched.0, "pixels differ in batches of {chunk} ({spec:?}, {mode:?})");
                assert_eq!(one.1, batched.1, "reported tiles differ in batches of {chunk}");
                assert_eq!(one.2, batched.2, "touched tiles differ in batches of {chunk}");
            }
        }
    }
}

#[test]
fn texels_the_stroke_did_not_reach_keep_their_bits() {
    let mesh = mesh_of(vec![quad()]);
    let (map, index) = surface(&mesh);
    let mut img = image();
    let all = img.all_tiles();
    img.update_tiles(&all, |r, px| px.iter_mut().enumerate().for_each(|(i, p)| *p = [(r.y as f32 + (i / 128) as f32) / 300.0, 0.1, 0.7, 0.8]));
    let before = img.clone();
    let ctx = StampContext { surface: &map, index: &index, space: StrokeSpace::Texture, factors: &[] };
    let mut stroke = StampStroke::new(SymmetrySpec::None, &[target(&mut img, StampMode::Paint)]).unwrap();
    for part in dabs([128.0, 128.0], 1.0).chunks(5) {
        stroke.apply_all(&ctx, part, &mut [target(&mut img, StampMode::Paint)]).unwrap();
    }
    let mut reached = 0;
    for y in 0..256 {
        for x in 0..256 {
            if stroke.coverage(x as usize, y as usize) > 0.0 {
                reached += 1;
                assert_ne!(img.get(x, y), before.get(x, y));
            } else {
                assert_eq!(img.get(x, y).map(f32::to_bits), before.get(x, y).map(f32::to_bits), "({x}, {y})");
            }
        }
    }
    assert!(reached > 1000);
}

#[test]
fn texture_space_dabs_give_the_same_bits_however_batched() {
    same_however_batched(StrokeSpace::Texture, [128.0, 128.0], 1.0);
}

#[test]
fn view_dabs_give_the_same_bits_however_batched() {
    let mesh = mesh_of(vec![quad()]);
    let scene = ViewScene::new(&mesh, mesh.fit());
    let projector = scene.projector(&mesh, camera([0.3, 0.2, 1.0], [0.0; 3])).unwrap();
    let centre = projector.to_screen([0.0; 3]).unwrap().pos;
    for placement in [Placement::FacingView, Placement::Wrapped, Placement::Flat, Placement::UvAligned] {
        let space = StrokeSpace::View(ViewSpace { projector: &projector, depth: None, placement, max_angle: 85.0 });
        same_however_batched(space, centre, 0.6);
    }
}
