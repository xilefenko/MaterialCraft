use super::*;
use crate::paint::pixel::{PixelLayout, SampleType};
use crate::tex::Tex;

fn fmt(l: PixelLayout, s: SampleType) -> PixelFormat {
    PixelFormat::new(l, s)
}

#[test]
fn tiles_are_lazy_and_read_as_zero() {
    let mut img = TiledImage::new(300, 200, fmt(PixelLayout::Rgba, SampleType::U8)).unwrap();
    assert_eq!((img.tiles_x(), img.tiles_y(), img.tile_count()), (3, 2, 6));
    assert_eq!(img.allocated_tiles(), 0);
    assert_eq!(img.memory_bytes(), 0);
    assert_eq!(img.get(10, 10), [0.0; 4]);
    let rgb = TiledImage::new(4, 4, fmt(PixelLayout::Rgb, SampleType::U8)).unwrap();
    assert_eq!(rgb.get(1, 1), [0.0, 0.0, 0.0, 1.0]);
    assert!(img.set(299, 199, [1.0, 0.0, 0.0, 1.0]));
    assert_eq!(img.allocated_tiles(), 1);
    assert!(img.tile_allocated(2, 1));
    assert_eq!(img.memory_bytes(), 128 * 128 * 4);
    assert_eq!(img.get(299, 199), [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn edges_and_out_of_bounds() {
    let mut img = TiledImage::new(130, 129, fmt(PixelLayout::Gray, SampleType::F32)).unwrap();
    assert_eq!(img.tile_rect(1, 1), Some(Rect { x: 128, y: 128, w: 2, h: 1 }));
    assert_eq!(img.tile_rect(2, 0), None);
    assert!(!img.set(130, 0, [1.0; 4]));
    assert!(!img.write_tile(5, 5, &[]));
    assert_eq!(img.get(500, 500), [0.0, 0.0, 0.0, 1.0]);
    assert!(TiledImage::new(MAX_DIMENSION + 1, 1, fmt(PixelLayout::Gray, SampleType::U8)).is_err());
    let empty = TiledImage::new(0, 0, fmt(PixelLayout::Gray, SampleType::U8)).unwrap();
    assert_eq!(empty.to_raw(), Vec::<u8>::new());
    assert_eq!(empty.to_tex().px.len(), 0);
    img.set(129, 128, [0.25; 4]);
    let mut out = vec![[9.0; 4]; 4];
    img.read_region(Rect { x: 128, y: 128, w: 2, h: 2 }, &mut out);
    assert_eq!(out[1][0], 0.25);
    assert_eq!(out[2], [0.0, 0.0, 0.0, 1.0]); // below the image
}

#[test]
fn raw_round_trip_every_format_with_odd_size() {
    let (w, h) = (131u32, 257u32);
    for l in PixelLayout::ALL {
        for s in SampleType::ALL {
            let f = fmt(l, s);
            let mut raw = vec![0u8; (w * h) as usize * f.bytes_per_pixel()];
            for (i, b) in raw.iter_mut().enumerate() {
                *b = (i * 7 % 251) as u8;
            }
            if s.is_float() {
                // keep floats finite and canonical: only write small half / float values
                let mut tmp = TiledImage::new(w, h, f).unwrap();
                for y in 0..h {
                    for x in 0..w {
                        tmp.set(x, y, [x as f32 / 256.0, y as f32 / 512.0, 0.5, 0.75]);
                    }
                }
                raw = tmp.to_raw();
            }
            let img = TiledImage::from_raw(w, h, f, &raw).unwrap();
            assert_eq!(img.to_raw(), raw, "{}", f.id());
        }
    }
    let f = fmt(PixelLayout::Gray, SampleType::U8);
    assert!(matches!(TiledImage::from_raw(2, 2, f, &[0; 3]), Err(ImageError::WrongDataSize { expected: 4, found: 3 })));
}

#[test]
fn zero_tiles_stay_unallocated_on_import() {
    let f = fmt(PixelLayout::Gray, SampleType::U8);
    let mut raw = vec![0u8; 256 * 256];
    raw[256 * 200 + 200] = 9;
    let img = TiledImage::from_raw(256, 256, f, &raw).unwrap();
    assert_eq!(img.allocated_tiles(), 1);
    assert!(img.region_allocated(Rect { x: 150, y: 150, w: 10, h: 10 }));
    assert!(!img.region_allocated(Rect { x: 0, y: 0, w: 128, h: 128 }));
}

#[test]
fn tex_round_trip() {
    let tex = Tex::from_fn(200, 150, |u, v| [u, v, 0.5, 1.0 - u]);
    let img = TiledImage::from_tex(&tex, fmt(PixelLayout::Rgba, SampleType::F32)).unwrap();
    let back = img.to_tex();
    assert_eq!((back.w, back.h), (200, 150));
    assert_eq!(back.px, tex.px);
    let img8 = TiledImage::from_tex(&tex, fmt(PixelLayout::Rgba, SampleType::U8)).unwrap();
    assert!(img8.to_tex().px.iter().zip(&tex.px).all(|(a, b)| (0..4).all(|c| (a[c] - b[c]).abs() <= 0.5 / 255.0 + 1e-6)));
    let bad = Tex { w: 3, h: 3, px: vec![[0.0; 4]; 2] };
    assert!(TiledImage::from_tex(&bad, fmt(PixelLayout::Rgba, SampleType::U8)).is_err());
}

#[test]
fn dirty_tracking() {
    let mut img = TiledImage::new(512, 256, fmt(PixelLayout::GrayAlpha, SampleType::U16)).unwrap();
    assert!(img.dirty_tiles().is_empty());
    img.set(5, 5, [1.0; 4]);
    img.set(300, 200, [1.0; 4]);
    assert_eq!(img.dirty_tiles(), vec![(0, 0), (2, 1)]);
    img.clear_dirty();
    assert!(img.dirty_tiles().is_empty());
    img.update_tiles(&[(3, 0), (3, 0), (9, 9)], |r, px| {
        assert_eq!(r, Rect { x: 384, y: 0, w: 128, h: 128 });
        px.fill([0.5, 0.5, 0.5, 1.0]);
    });
    assert_eq!(img.dirty_tiles(), vec![(3, 0)]);
    assert!((img.get(400, 3)[0] - 0.5).abs() < 1e-4);
    img.clear();
    assert_eq!(img.allocated_tiles(), 0);
    assert_eq!(img.dirty_tiles().len(), 8);
}

#[test]
fn update_regions_touches_only_their_pixels() {
    for f in [fmt(PixelLayout::Rgba, SampleType::U8), fmt(PixelLayout::Rgba, SampleType::F16), fmt(PixelLayout::GrayAlpha, SampleType::F32)] {
        let mut img = TiledImage::new(300, 200, f).unwrap();
        img.update_tiles(&[(0, 0)], |_, px| px.iter_mut().enumerate().for_each(|(i, p)| *p = [(i % 7) as f32 / 7.0, 0.5, 0.25, 1.0]));
        img.clear_dirty();
        let before = img.clone();
        // one region inside tile (0, 0), one cut to tile (2, 1) (it starts there), one outside the image
        let regions = [Rect { x: 10, y: 20, w: 5, h: 3 }, Rect { x: 290, y: 190, w: 40, h: 40 }, Rect { x: 900, y: 0, w: 4, h: 4 }];
        let mut seen = std::sync::Mutex::new(Vec::new());
        img.update_regions(&regions, |r, px| {
            assert_eq!(px.len(), r.area());
            seen.lock().unwrap().push(r);
            px.iter_mut().for_each(|p| *p = [1.0, 0.0, 0.0, 1.0]);
        });
        let mut seen = std::mem::take(seen.get_mut().unwrap());
        seen.sort_by_key(|r| (r.y, r.x));
        assert_eq!(seen, vec![Rect { x: 10, y: 20, w: 5, h: 3 }, Rect { x: 290, y: 190, w: 10, h: 10 }]);
        assert_eq!(img.dirty_tiles(), vec![(0, 0), (2, 1)]);
        for y in 0..200 {
            for x in 0..300 {
                let inside = (10..15).contains(&x) && (20..23).contains(&y) || x >= 290 && y >= 190;
                if inside {
                    assert_eq!(img.get(x, y)[0], 1.0, "{f:?} ({x}, {y})");
                } else {
                    assert_eq!(img.get(x, y), before.get(x, y), "{f:?} ({x}, {y}) unchanged");
                }
            }
        }
        // the bytes outside the regions too
        let raw = |i: &TiledImage| i.tiles.first().and_then(|t| t.as_ref()).map(|t| t.bytes().to_vec()).unwrap();
        let (a, b) = (raw(&img), raw(&before));
        let bpp = f.bytes_per_pixel();
        let differ: Vec<usize> = a.chunks(bpp).zip(b.chunks(bpp)).enumerate().filter(|(_, (p, q))| p != q).map(|(i, _)| i).collect();
        assert!(differ.iter().all(|i| (10..15).contains(&(i % 128)) && (20..23).contains(&(i / 128))), "{f:?}");
    }
}

#[test]
fn update_tiles_sees_current_pixels_and_convert_keeps_them() {
    let mut img = TiledImage::new(140, 140, fmt(PixelLayout::Rgba, SampleType::F16)).unwrap();
    img.fill([0.25, 0.5, 0.75, 1.0]);
    img.update_tiles(&img.all_tiles(), |_, px| px.iter_mut().for_each(|p| p[0] *= 2.0));
    assert_eq!(img.get(139, 139), [0.5, 0.5, 0.75, 1.0]);
    let c = img.converted(fmt(PixelLayout::Rgb, SampleType::F32));
    assert_eq!(c.get(0, 0), [0.5, 0.5, 0.75, 1.0]);
    assert_eq!(c.converted(img.format()), img);
}

#[test]
fn large_image_round_trip_is_fast_enough() {
    let tex = Tex::solid(2048, 2048, [0.3, 0.6, 0.9, 1.0]);
    let start = std::time::Instant::now();
    let img = TiledImage::from_tex(&tex, fmt(PixelLayout::Rgba, SampleType::U8)).unwrap();
    let back = img.to_tex();
    assert_eq!(back.px.len(), tex.px.len());
    assert!(start.elapsed().as_secs_f32() < 10.0);
}

#[test]
fn clones_share_tiles_until_written() {
    let mut img = TiledImage::new(300, 200, fmt(PixelLayout::Rgba, SampleType::U8)).unwrap();
    img.set(1, 1, [1.0; 4]);
    img.set(200, 150, [0.5; 4]);
    let before = img.clone();
    assert!(img.tile(0, 0).unwrap().same(before.tile(0, 0).unwrap()));
    assert!(img.changed_tiles(&before).is_empty());
    img.set(2, 2, [0.0, 1.0, 0.0, 1.0]);
    img.set(290, 10, [1.0; 4]);
    assert_eq!(before.get(2, 2), [0.0; 4], "the clone keeps its pixels");
    assert!(!img.tile(0, 0).unwrap().same(before.tile(0, 0).unwrap()));
    assert!(img.tile(1, 1).unwrap().same(before.tile(1, 1).unwrap()));
    assert_eq!(img.changed_tiles(&before), vec![(0, 0), (2, 0)]);
    assert_eq!(img.tiles().map(|(c, _)| c).collect::<Vec<_>>(), vec![(0, 0), (2, 0), (1, 1)]);
    let other = TiledImage::new(10, 10, fmt(PixelLayout::Rgba, SampleType::U8)).unwrap();
    assert_eq!(img.changed_tiles(&other).len(), img.tile_count());
}

#[test]
fn replacing_tiles_restores_bytes() {
    let mut img = TiledImage::new(300, 200, fmt(PixelLayout::Gray, SampleType::U16)).unwrap();
    img.set(5, 5, [0.25; 4]);
    let before = img.clone();
    img.set(5, 5, [0.75; 4]);
    img.set(140, 5, [0.75; 4]);
    img.clear_dirty();
    for (tx, ty) in img.changed_tiles(&before) {
        img.replace_tile(tx, ty, before.tile(tx, ty).cloned()).unwrap();
    }
    assert_eq!(img, before);
    assert_eq!(img.to_raw(), before.to_raw());
    assert_eq!(img.dirty_tiles(), vec![(0, 0), (1, 0)]);
    assert_eq!(img.replace_tile(9, 0, None), Err(ImageError::NoSuchTile { tx: 9, ty: 0 }));
    let mut wide = TiledImage::new(4, 4, fmt(PixelLayout::Rgba, SampleType::F32)).unwrap();
    wide.set(0, 0, [1.0; 4]);
    assert!(matches!(img.replace_tile(0, 0, wide.tile(0, 0).cloned()), Err(ImageError::WrongDataSize { .. })));
}
