//! Never-panic checks: every decoder entry point is fed truncated, corrupted and random byte strings and must
//! return (`Ok` or `Err`) without panicking. Inputs come from a tiny deterministic generator, so a failure always
//! reproduces; seeds are the small valid records the unit tests already build, so the corruptions reach deep into
//! each decoder rather than failing on the first byte.

use crate::container::hdf5::Hdf5File;
use crate::container::qds::QDataStream;
use crate::container::qds::test_encoder::QdsWriter as W;
use crate::decode::assbin::test_writer::{TestMesh, compressed_file, file};
use crate::decode::assbin::{AssbinGeometry, AssbinHeader, AssbinScene, MeshMetadata, Normalization};
use crate::decode::document::{ClassCensus, DocumentHeader, DocumentMaterials, DocumentSettings, Visitor};
use crate::decode::image::{ImageHeader, ImageResource};
use crate::decode::resource_toc::ResourceToc;
use crate::decode::settings::ProjectSettings;
use crate::decode::texture_cache::{CacheHeader, CacheTexture};
use crate::model::PainterDocument;
use crate::model::cameras::PainterCameras;
use crate::model::viewer::PainterViewer;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Inputs generated per seed, on top of every truncation.
const VARIANTS_PER_SEED: usize = 300;

/// Knuth's MMIX linear congruential generator; the high bits are the usable ones.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    fn below(&mut self, n: usize) -> usize {
        self.next() as usize % n.max(1)
    }

    fn byte(&mut self) -> u8 {
        self.next() as u8
    }
}

/// Truncations of `seed` (every length for short seeds, evenly spaced ones for long ones), then
/// [`VARIANTS_PER_SEED`] corruptions: a few bytes overwritten, often followed by a cut, with some
/// entirely random strings mixed in.
fn variants(seed: &[u8], rng: &mut Lcg) -> Vec<Vec<u8>> {
    let step = (seed.len() / 256).max(1);
    let mut out: Vec<Vec<u8>> = (0..seed.len()).step_by(step).map(|end| seed[..end].to_vec()).collect();
    for i in 0..VARIANTS_PER_SEED {
        if i % 10 == 0 {
            let len = rng.below(256);
            out.push((0..len).map(|_| rng.byte()).collect());
            continue;
        }
        let mut v = seed.to_vec();
        for _ in 0..1 + rng.below(4) {
            if v.is_empty() {
                break;
            }
            // Half the edits land in the first 4 KiB, where the containers keep their headers.
            let at = match rng.below(2) {
                0 => rng.below(v.len().min(4096)),
                _ => rng.below(v.len()),
            };
            // Mostly small edits, sometimes the extremes that make lengths and counts huge.
            v[at] = match rng.below(4) {
                0 => 0xff,
                1 => 0x00,
                2 => v[at].wrapping_add(1 + rng.byte() % 4),
                _ => rng.byte(),
            };
        }
        if rng.below(2) == 0 {
            v.truncate(rng.below(v.len() + 1));
        }
        out.push(v);
    }
    out
}

/// Runs `decode` on every variant of every seed; fails naming the input if any call panics.
fn never_panics(name: &str, seeds: &[Vec<u8>], decode: impl Fn(&[u8])) {
    // Each entry point gets its own stream, so adding one does not change the others' inputs.
    let mut rng = Lcg(name
        .bytes()
        .fold(0x5eed, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b))));
    for seed in seeds {
        for input in variants(seed, &mut rng) {
            if catch_unwind(AssertUnwindSafe(|| decode(&input))).is_err() {
                panic!(
                    "{name} panicked on {} bytes: {:02x?}",
                    input.len(),
                    &input[..input.len().min(512)]
                );
            }
        }
    }
}

fn int(v: i32) -> Vec<u8> {
    W::encode(|w| w.int(v))
}

fn assbin_seeds() -> Vec<Vec<u8>> {
    let meshes = [
        TestMesh {
            name: "Body",
            material_index: 0,
            vertices: 4,
            triangles: 2,
            uv_sets: 1,
            color_sets: 1,
        },
        TestMesh {
            name: "Trim",
            material_index: 1,
            vertices: 3,
            triangles: 1,
            uv_sets: 2,
            color_sets: 0,
        },
    ];
    vec![
        file(&meshes, &["Body", "Trim"]),
        compressed_file(&meshes, &["Body", "Trim"]),
    ]
}

fn qds_seed() -> Vec<u8> {
    let mut w = W::new();
    w.raw_map(&[
        ("versionAtLastSave/major", int(9)),
        ("versionAtCreation/major", int(8)),
        ("list", W::encode(|w| w.string_list(&["a", "bc"]))),
        ("center", W::encode(|w| w.vector3d([0.0, 1.0, 2.0]))),
        ("radius", W::encode(|w| w.float(2.5))),
        ("origin", W::encode(|w| w.url("file:///C:/a.obj"))),
        ("nested", W::encode(|w| w.map(&[("x", W::encode(|w| w.double(1.0)))]))),
        ("bytes", W::encode(|w| w.byte_array(b"{}"))),
    ])
    .raw_bytes(&W::encode_list(&[int(1), W::encode(|w| w.null_string())]));
    w.finish()
}

fn image_seed() -> Vec<u8> {
    let payload = lz4_flex::block::compress(&(0..8).collect::<Vec<u8>>());
    let mut w = W::new();
    w.raw_map(&[
        ("width", W::encode(|w| w.uint(2))),
        ("version", W::encode(|w| w.string("v2"))),
        ("pitch", W::encode(|w| w.long_long(4))),
        ("height", W::encode(|w| w.uint(2))),
        ("format", W::encode(|w| w.string("LUM16"))),
        ("dataSizeRaw", W::encode(|w| w.ulong_long(8))),
        ("compression", W::encode(|w| w.string("lz4"))),
        ("dataSizeLZ4", W::encode(|w| w.ulong_long(payload.len() as u64))),
    ])
    .raw_bytes(&payload);
    w.finish()
}

fn cache_seed() -> Vec<u8> {
    let payload = lz4_flex::block::compress(&[10, 20, 30, 40]);
    let mut w = W::new();
    w.raw_map(&[
        ("width", W::encode(|w| w.uint(2))),
        ("version", W::encode(|w| w.string("v1"))),
        ("internalFormat", W::encode(|w| w.int(0x822B))),
        ("height", W::encode(|w| w.uint(1))),
        ("compression", W::encode(|w| w.string("lz4"))),
        ("bufferSize", W::encode(|w| w.uint(4))),
    ])
    .raw_u32(payload.len() as u32)
    .raw_bytes(&payload);
    w.finish()
}

fn toc_seed() -> Vec<u8> {
    let mut w = W::new();
    w.raw_u32(2);
    w.raw_string("images")
        .raw_u32(2)
        .raw_string("/a?version=1")
        .raw_string("/b");
    w.raw_string("brushes").raw_u32(0);
    w.finish()
}

/// Counts objects, so the walker does real work for every callback.
struct Counter(usize);

impl Visitor for Counter {
    fn object(&mut self, _class: &str) {
        self.0 += 1;
    }

    fn unknown_tag(&mut self, _tag: u8, _offset: usize) {}
}

#[test]
fn inflate_never_panics() {
    // A fixed block, a stored block and a dynamic block (the deflate test's stream).
    let seeds = [
        vec![0x4b, 0x4c, 0x4a, 0x06, 0x00],
        vec![0x01, 3, 0, 0xfc, 0xff, b'a', b'b', b'c'],
        vec![
            11, 72, 204, 204, 43, 73, 45, 82, 200, 78, 77, 45, 40, 86, 200, 44, 41, 86, 200, 77, 45, 206, 80, 72, 73,
            77, 203, 73, 44, 73, 77, 177, 86, 8, 24, 85, 48, 170, 128, 190, 10, 0,
        ],
    ];
    never_panics("inflate", &seeds, |d| {
        let _ = crate::inflate::inflate(d);
    });
}

#[test]
fn assbin_decoders_never_panic() {
    let seeds = assbin_seeds();
    never_panics("AssbinHeader::parse", &seeds, |d| {
        let _ = AssbinHeader::parse(d);
    });
    never_panics("AssbinScene::parse", &seeds, |d| {
        let _ = AssbinScene::parse(d);
    });
    never_panics("AssbinGeometry::parse", &seeds, |d| {
        let _ = AssbinGeometry::parse(d);
    });
    never_panics("Normalization::of_scene", &seeds, |d| {
        let _ = Normalization::of_scene(d, None);
    });
}

#[test]
fn document_decoders_never_panic() {
    let seeds = [crate::decode::document::test_writer::sample()];
    never_panics("DocumentHeader::parse", &seeds, |d| {
        let _ = DocumentHeader::parse(d);
    });
    never_panics("walk", &seeds, |d| {
        let _ = crate::decode::document::walk(d, &mut Counter(0));
    });
    never_panics("ClassCensus::scan", &seeds, |d| {
        let _ = ClassCensus::scan(d);
    });
    never_panics("DocumentMaterials::scan", &seeds, |d| {
        let _ = DocumentMaterials::scan(d);
    });
    never_panics("DocumentSettings::scan", &seeds, |d| {
        let _ = DocumentSettings::scan(d);
    });
    never_panics("PainterDocument::from_bytes", &seeds, |d| {
        let _ = PainterDocument::from_bytes(d);
    });
    never_panics("PainterCameras::parse", &seeds, |d| {
        let _ = PainterCameras::parse(d);
    });
    never_panics("PainterViewer::parse", &seeds, |d| {
        let _ = PainterViewer::parse(d);
    });
}

#[test]
fn qdatastream_and_record_decoders_never_panic() {
    let seeds = [qds_seed(), image_seed(), cache_seed(), toc_seed()];
    never_panics("QDataStream", &seeds, |d| {
        let _ = QDataStream::new(d).read_variant_map();
        let _ = QDataStream::new(d).read_variant_list();
        let _ = QDataStream::new(d).read_variant();
        let _ = QDataStream::new(d).read_string_list();
        let mut s = QDataStream::new(d);
        while s.read_string().is_ok() && !s.is_at_end() {}
    });
    never_panics("ProjectSettings::parse", &seeds, |d| {
        let _ = ProjectSettings::parse(d);
    });
    never_panics("MeshMetadata::parse", &seeds, |d| {
        let _ = MeshMetadata::parse(d);
    });
    never_panics("ResourceToc::parse", &seeds, |d| {
        let _ = ResourceToc::parse(d);
    });
    never_panics("ImageHeader::parse", &seeds, |d| {
        let _ = ImageHeader::parse(d);
    });
    never_panics("ImageResource", &seeds, |d| {
        if let Ok(image) = ImageResource::parse(d) {
            let _ = image.decompress();
            let _ = image.decompress_padded();
        }
    });
    never_panics("CacheHeader::parse", &seeds, |d| {
        let _ = CacheHeader::parse(d);
    });
    never_panics("CacheTexture", &seeds, |d| {
        if let Ok(texture) = CacheTexture::parse(d) {
            let _ = texture.decompress();
            let _ = texture.decompress_padded();
        }
    });
}

#[test]
fn hdf5_reader_never_panics() {
    let seeds = [include_bytes!("../tests/fixtures/small.h5").to_vec()];
    never_panics("Hdf5File", &seeds, |d| {
        let Ok(file) = Hdf5File::from_memory(d.to_vec()) else {
            return;
        };
        for entry in file.entries().unwrap_or_default() {
            let _ = file.read_prefix(&entry.path, 1 << 20);
        }
    });
}
