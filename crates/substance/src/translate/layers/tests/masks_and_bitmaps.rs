//! Masks made of uniform/bitmap fills, and bitmap fills from embedded images.

use super::fixtures::*;
use crate::model::*;
use crate::translate::layers::ImportedStack;
use crate::translate::layers::images::{ImageSource, LoadedImage};
use materialcraft::paint::{
    Channel, EffectKind, FillSource, LayerKind, PixelFormat, PixelLayout, SampleType, TiledImage,
};
use materialcraft::projection::ProjectionMode;

fn mask(initial: MaskInitial, effects: Vec<PainterEffect>) -> Option<PainterMask> {
    Some(PainterMask {
        enabled: true,
        initial,
        effects,
    })
}

fn mask_fill(source: PainterSource, mode: u64, opacity: f32) -> PainterEffect {
    PainterEffect::Fill(FillEffect {
        header: header(0, vec![blend(0, mode, opacity)]),
        sources: vec![source],
        projection: 0,
        filtering: 2,
        placement: Default::default(),
        uv_transform: None,
    })
}

fn base_and(top: PainterLayer) -> Vec<PainterLayer> {
    vec![
        layer(1, "Base", vec![fill(vec![uniform(ROUGH, [0.0; 3])])], vec![]),
        top,
    ]
}

fn top(m: Option<PainterMask>) -> PainterLayer {
    let mut l = layer(2, "Top", vec![fill(vec![uniform(ROUGH, [1.0; 3])])], vec![]);
    l.mask = m;
    l
}

#[test]
fn plain_masks_keep_their_state() {
    let white = import(base_and(top(mask(MaskInitial::White, vec![]))));
    assert!(
        white.layers[1].mask.as_ref().unwrap().is_identity(),
        "an all-white mask changes nothing"
    );
    let black = import(base_and(top(mask(MaskInitial::Black, vec![]))));
    assert_eq!(black.layers[1].mask.as_ref().unwrap().constant_value(), Some(0.0));
    assert!(composite(&black, Channel::Roughness, 2, 2)[0] < 1e-3);
    let quarter = mask_fill(uniform(0, [0.25; 3]), 2, 1.0);
    let grey = import(base_and(top(mask(MaskInitial::Black, vec![quarter]))));
    assert!((composite(&grey, Channel::Roughness, 2, 2)[0] - 0.25).abs() < 0.01);
    assert!(grey.verdict.all_exact());
    let off = PainterMask {
        enabled: false,
        ..mask(MaskInitial::Black, vec![]).unwrap()
    };
    let off = import(base_and(top(Some(off))));
    let m = off.layers[1].mask.as_ref().unwrap();
    assert!(!m.enabled && m.is_identity(), "a switched-off mask is kept");
    assert!((composite(&off, Channel::Roughness, 2, 2)[0] - 1.0).abs() < 0.01);
}

#[test]
fn generator_masks_are_not_computable() {
    let generator = mask_fill(procedural(0, "mg_dirt"), 2, 1.0);
    let stack = import(base_and(top(mask(MaskInitial::Black, vec![generator]))));
    let m = stack.layers[1].mask.as_ref().unwrap();
    assert!(
        matches!(&m.effects[0].kind, EffectKind::Generator(p) if p.id == "substance:mg_dirt"),
        "{m:?}"
    );
    let status = stack.verdict.channel(Channel::Roughness);
    assert_eq!(status.reasons(), ["mask generator \"mg_dirt\""]);
}

#[test]
fn generator_masks_the_plan_replaces_become_ours() {
    use crate::translate::layers::{BlendTable, LayerImport, NoImages, StackStrategy, cache_agreement::CacheAgreement};
    use crate::translate::replacements::{Choice, ProceduralPlan};
    let generator = mask_fill(procedural(0, "mg_dirt"), 2, 1.0);
    let layers = base_and(top(mask(MaskInitial::Black, vec![generator])));
    let table = BlendTable::painter();
    let plan = ProceduralPlan::all(Choice::Replace);
    let stack = LayerImport {
        table: &table,
        projections: &crate::translate::layers::ProjectionTable::painter(),
        images: &NoImages,
        plan: &plan,
        replay: Default::default(),
        replay_cache: None,
    }
    .import(&set(layers), &channels(), SIZE);
    let m = stack.layers[1].mask.as_ref().unwrap();
    let EffectKind::Generator(p) = &m.effects[0].kind else {
        panic!("{m:?}")
    };
    assert_eq!(p.id, "dirt");
    assert_eq!(p.original.as_ref().map(|o| o.id.as_str()), Some("substance:mg_dirt"));
    let status = stack.verdict.channel(Channel::Roughness);
    assert!(status.is_exact_or_replaced() && !status.is_exact(), "{status:?}");
    let strategy = StackStrategy::choose(&stack.verdict, true, &CacheAgreement::default());
    assert_eq!(strategy, StackStrategy::Replacements);
    assert!(!strategy.corrects());
}

/// One 8×8 RGBA8 image whose left half is white.
struct HalfWhite;

impl ImageSource for HalfWhite {
    fn load(&self, url: &str) -> Result<LoadedImage, String> {
        if url != "/Logo?version=abc" {
            return Err(format!("no {url}"));
        }
        let mut image = TiledImage::new(8, 8, PixelFormat::new(PixelLayout::Rgba, SampleType::U8)).unwrap();
        image.fill([0.0, 0.0, 0.0, 1.0]);
        for y in 0..8 {
            for x in 0..4 {
                image.set(x, y, [1.0; 4]);
            }
        }
        Ok(LoadedImage { image, float: false })
    }
}

fn bitmap(bits: u64, url: &str) -> PainterSource {
    PainterSource::Bitmap(BitmapSource {
        uid: None,
        channels: ChannelMask(bits),
        url: url.into(),
    })
}

#[test]
fn bitmap_fills_become_paint_images_and_bitmap_masks_mask() {
    let logo = layer(
        3,
        "Logo",
        vec![fill(vec![bitmap(BASE, "/Logo?version=abc"), uniform(ROUGH, [0.7; 3])])],
        vec![],
    );
    let stack = import_with(vec![logo], &HalfWhite);
    let LayerKind::Fill(f) = &stack.layers[0].kind else {
        panic!("fill layer")
    };
    let FillSource::Image(images) = &f.source else {
        panic!("image fill")
    };
    assert_eq!(images.channels(), [Channel::BaseColor, Channel::Roughness]);
    assert_eq!(f.projection.mode, ProjectionMode::Uv);
    assert!(stack.verdict.all_exact());
    assert!(composite(&stack, Channel::BaseColor, 1, 1)[0] > 0.99);
    assert!(composite(&stack, Channel::BaseColor, 6, 1)[0] < 0.01);

    let masked = top(mask(
        MaskInitial::Black,
        vec![mask_fill(bitmap(0, "/Logo?version=abc"), 2, 1.0)],
    ));
    let stack = import_with(base_and(masked), &HalfWhite);
    assert!(composite(&stack, Channel::Roughness, 1, 5)[0] > 0.99);
    assert!(composite(&stack, Channel::Roughness, 7, 5)[0] < 0.01);
}

#[test]
fn placed_or_missing_bitmaps_are_reported() {
    let mut placed = FillEffect {
        header: header(ALL, vec![]),
        sources: vec![bitmap(BASE, "/Logo?version=abc")],
        projection: 6,
        filtering: 2,
        placement: Default::default(),
        uv_transform: Some(UvTransform {
            scale: [3.0, 3.0],
            translation: [0.0, 0.0],
            rotation: 0.0,
        }),
    };
    let stack = import_with(
        vec![layer(1, "Tri", vec![PainterEffect::Fill(placed.clone())], vec![])],
        &HalfWhite,
    );
    let status = stack.verdict.channel(Channel::BaseColor);
    assert!(status.is_computable() && status.reasons().len() == 1, "{status:?}");
    placed.projection = 1;
    let stack = import_with(
        vec![layer(1, "Planar", vec![PainterEffect::Fill(placed.clone())], vec![])],
        &HalfWhite,
    );
    let status = stack.verdict.channel(Channel::BaseColor);
    assert!(
        status.is_computable() && status.reasons().iter().any(|r| r.contains("planar")),
        "{status:?}"
    );
    placed.sources = vec![bitmap(BASE, "resource://shelf/x?version=1")];
    let stack = import(vec![layer(1, "Shelf", vec![PainterEffect::Fill(placed)], vec![])]);
    assert!(!stack.verdict.channel(Channel::BaseColor).is_computable());
}

/// The grey values of the first mask effect of layer `layer` (a painted image), row by row.
fn mask_values(stack: &ImportedStack, layer: usize) -> Vec<f32> {
    let m = stack.layers[layer].mask.as_ref().unwrap();
    let EffectKind::Paint(p) = &m.effects[0].kind else {
        panic!("{m:?}")
    };
    let (w, h) = p.image.size();
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| p.image.get(x, y)[0])
        .collect()
}

#[test]
fn volume_mask_bitmaps_are_laid_on_over_the_surface() {
    use crate::translate::layers::{BlendTable, LayerImport, LazySurface, ProjectionTable};
    let mut fill = mask_fill(bitmap(0, "/Logo?version=abc"), 2, 1.0);
    if let PainterEffect::Fill(f) = &mut fill {
        f.projection = 1; // planar
        f.placement.position = Some([0.25, 0.0, 0.0]);
    }
    let layers = base_and(top(mask(MaskInitial::Black, vec![fill])));
    let mesh = super::native::quad();
    let surface = LazySurface::new(&mesh, 0, 0, SIZE);
    let table = BlendTable::painter();
    let import = LayerImport {
        table: &table,
        projections: &ProjectionTable::painter(),
        images: &HalfWhite,
        plan: &Default::default(),
        replay: Default::default(),
        replay_cache: None,
    };
    let on = import.import_on(&set(layers.clone()), &channels(), SIZE, None, Some(&surface));
    let off = import.import(&set(layers), &channels(), SIZE);
    let (on, off) = (mask_values(&on, 1), mask_values(&off, 1));
    assert_eq!(
        off.iter().filter(|v| **v > 0.99).count(),
        32,
        "without the mesh: in texture space"
    );
    assert_ne!(on, off, "the volume's offset moves the bitmap");
    let mean = on.iter().sum::<f32>() / on.len() as f32;
    assert!(
        (mean - 0.5).abs() < 0.1,
        "a repeating half-white bitmap stays half white: {on:?}"
    );
}
