//! A stroke stamped on its own thread ([`StrokeWorker`]) gives the document, the stroke record and the undo step
//! a stroke stamped in place gives, bit for bit, however its points are handed over and however often it is
//! polled; pen up completes it exactly.

use materialcraft::camera::{Camera, Lens};
use materialcraft::mesh::{Mesh, MeshPart, SurfaceKey, SurfaceMap};
use materialcraft::paint::brush::{BrushPreset, Jitter, Retouch};
use materialcraft::paint::edit::ops::{NewLayer, create};
use materialcraft::paint::edit::{Edit, LayerRef, Transaction};
use materialcraft::paint::stroke::{StrokeTool, Symmetry, ViewKind};
use materialcraft::paint::{Channel, Document, TextureSet};
use materialcraft::projection::{ViewProjector, ViewScene};
use materialcraft::stamp::TexelIndex;
use materialcraft::symmetry::SymmetrySpec;
use materialcraft_brush::symmetry::to_record;
use materialcraft_brush::{DocumentStroke, InputPoint, PaintTarget, PaintValues, SessionScene, StrokeSetup, StrokeWorker, TargetPart, WorkerScene};
use std::sync::Arc;

const SIZE: u32 = 256;

fn sphere() -> Mesh {
    let m = materialcraft::render3d::build_mesh(0, 48, 1.0, None, 0.0, 0.0);
    Mesh { materials: vec!["m".into()], parts: vec![MeshPart::from_viewport("sphere", 0, &m)] }
}

fn document() -> (Document, String) {
    let mut doc = Document { mesh: sphere(), ..Document::default() };
    doc.texture_sets.push(TextureSet::new("s", "S", (SIZE, SIZE)).with_channel(Channel::BaseColor).with_channel(Channel::Roughness));
    let mut tx = Transaction::new(&mut doc);
    let id = create(&mut tx, "s", NewLayer::Paint, None).unwrap();
    tx.commit();
    (doc, id)
}

fn camera() -> Camera {
    let lens = Lens::Perspective { fov_y: 40f32.to_radians(), near: 0.03, far: 60.0 };
    Camera::looking_at([0.4, 0.3, 3.0], [0.0; 3], [0.0, 1.0, 0.0], &lens, [480.0, 360.0])
}

fn values() -> PaintValues {
    PaintValues { channels: vec![(Channel::BaseColor, [0.9, 0.2, 0.1, 1.0], true), (Channel::Roughness, [0.3, 0.3, 0.3, 1.0], true)], mask: 1.0 }
}

fn setup(mesh: &Mesh, tool: StrokeTool, symmetry: Symmetry) -> StrokeSetup {
    let preset = BrushPreset {
        size: 0.2,
        flow: 0.6,
        opacity: 0.9,
        spacing: 0.05,
        jitter: Jitter { size: 0.3, angle: 0.5, flow: 0.2, position: 0.2 },
        ..BrushPreset::default()
    };
    StrokeSetup {
        preset,
        tool,
        seed: 11,
        symmetry,
        view: ViewKind::Mesh,
        values: values(),
        model: mesh.bounding_sphere().unwrap(),
        occlusion: true,
        retouch: Retouch { strength: 0.7, ..Retouch::default() },
    }
}

/// The surface and the view, owned, as the worker needs them.
struct Owned {
    map: Arc<SurfaceMap>,
    index: Arc<TexelIndex>,
    view: Arc<ViewScene>,
    mesh: Arc<Mesh>,
}

impl WorkerScene for Owned {
    fn with<R>(&self, f: impl FnOnce(&SessionScene) -> R) -> R {
        let projector = self.view.projector(&self.mesh, camera());
        let p = projector.as_ref().map(|p| p as &dyn ViewProjector);
        f(&SessionScene { surface: &self.map, index: &self.index, projector: p, factors: &[], projected: None })
    }
}

impl Clone for Owned {
    fn clone(&self) -> Owned {
        Owned { map: self.map.clone(), index: self.index.clone(), view: self.view.clone(), mesh: self.mesh.clone() }
    }
}

fn points() -> Vec<InputPoint> {
    (0..60)
        .map(|i| {
            let t = i as f32 / 59.0;
            InputPoint { pressure: 0.5 + 0.5 * t, ..InputPoint::at([120.0 + 240.0 * t, 180.0 + 40.0 * (t * 6.0).sin()], f64::from(t)) }
        })
        .collect()
}

/// Starts a stroke on `doc` (prepared and started in place, as at pen down).
fn start(doc: &mut Document, owned: &Owned, target: &PaintTarget, tool: StrokeTool, symmetry: Symmetry) -> DocumentStroke {
    let prepared = DocumentStroke::prepare(doc, target.clone(), &values()).unwrap();
    let Document { mesh, texture_sets, .. } = doc;
    owned.with(|scene| prepared.start(setup(mesh, tool, symmetry), scene, mesh, &|_| true, &mut texture_sets[0]).map_err(|b| b.1).unwrap())
}

/// The stroke stamped in place, a point at a time.
fn in_place(mut doc: Document, owned: &Owned, target: &PaintTarget, tool: StrokeTool, symmetry: Symmetry) -> (Document, Vec<Edit>) {
    let mut stroke = start(&mut doc, owned, target, tool, symmetry);
    for p in points() {
        owned.with(|scene| stroke.add(scene, &[p], &mut doc.texture_sets[0])).unwrap();
    }
    let (ended, result) = owned.with(|scene| stroke.finish(scene, &mut doc.texture_sets[0]));
    result.unwrap();
    let (inverses, problem) = ended.record(&mut doc);
    assert!(problem.is_none());
    (doc, inverses)
}

/// The stroke on a worker: points handed over in chunks of `chunks` (cycled), polled after every chunk.
fn on_worker(mut doc: Document, owned: &Owned, target: &PaintTarget, tool: StrokeTool, symmetry: Symmetry, chunks: &[usize]) -> (Document, Vec<Edit>) {
    let stroke = start(&mut doc, owned, target, tool, symmetry);
    let mut worker = StrokeWorker::spawn(stroke, doc.texture_sets[0].clone(), owned.clone()).unwrap();
    let list = points();
    let (mut at, mut k) = (0, 0);
    let mut painted_midway = false;
    while at < list.len() {
        let n = chunks[k % chunks.len()].min(list.len() - at);
        worker.add(list[at..at + n].to_vec());
        at += n;
        k += 1;
        let p = worker.poll(&mut doc.texture_sets[0]);
        assert!(p.error.is_none() && p.done.is_none());
        painted_midway |= !p.changed.is_empty();
    }
    assert_eq!(worker.samples_sent(), list.len());
    // before pen up, once the worker caught up, its paint is in the document
    while worker.samples_painted() < list.len() {
        std::thread::yield_now();
    }
    painted_midway |= !worker.poll(&mut doc.texture_sets[0]).changed.is_empty();
    worker.finish();
    let done = loop {
        let p = worker.poll(&mut doc.texture_sets[0]);
        assert!(p.error.is_none() && !p.lost);
        if let Some(d) = p.done {
            break d;
        }
        std::thread::yield_now();
    };
    assert!(painted_midway, "polls during the stroke bring paint into the document");
    assert_eq!(worker.samples_painted(), list.len());
    done.result.unwrap();
    let (inverses, problem) = done.ended.record(&mut doc);
    assert!(problem.is_none());
    (doc, inverses)
}

#[test]
fn a_stroke_on_a_worker_equals_one_stamped_in_place() {
    let (doc, layer) = document();
    let map = SurfaceMap::build(&doc.mesh, SurfaceKey::square(&doc.mesh, 0, SIZE)).unwrap();
    let owned = Owned {
        index: Arc::new(TexelIndex::build(&map)),
        map: Arc::new(map),
        view: Arc::new(ViewScene::new(&doc.mesh, doc.mesh.fit())),
        mesh: Arc::new(doc.mesh.clone()),
    };
    let target = PaintTarget { layer: LayerRef::new("s", &layer), part: TargetPart::Content };
    let mirror = to_record(SymmetrySpec::Mirror { axis: 0, offset: 0.0 });
    for (tool, symmetry) in [
        (StrokeTool::Paint, Symmetry::default()),
        (StrokeTool::Paint, mirror),
        (StrokeTool::Eraser, Symmetry::default()),
        (StrokeTool::Smudge, Symmetry::default()),
    ] {
        // something under the stroke for the eraser and the smudge
        let (under, _) = in_place(doc.clone(), &owned, &target, StrokeTool::Paint, Symmetry::default());
        let (want, want_inverses) = in_place(under.clone(), &owned, &target, tool, symmetry);
        for chunks in [&[1][..], &[3, 17, 1, 8], &[60]] {
            let (got, inverses) = on_worker(under.clone(), &owned, &target, tool, symmetry, chunks);
            assert!(got == want, "{tool:?} {symmetry:?} in chunks {chunks:?}: the document differs");
            assert!(inverses == want_inverses, "{tool:?} in chunks {chunks:?}: the undo step differs");
        }
    }
}

#[test]
fn waiting_completes_a_stroke_with_everything_queued() {
    let (mut doc, layer) = document();
    let map = SurfaceMap::build(&doc.mesh, SurfaceKey::square(&doc.mesh, 0, SIZE)).unwrap();
    let owned = Owned {
        index: Arc::new(TexelIndex::build(&map)),
        map: Arc::new(map),
        view: Arc::new(ViewScene::new(&doc.mesh, doc.mesh.fit())),
        mesh: Arc::new(doc.mesh.clone()),
    };
    let target = PaintTarget { layer: LayerRef::new("s", &layer), part: TargetPart::Content };
    let (want, _) = in_place(doc.clone(), &owned, &target, StrokeTool::Paint, Symmetry::default());
    let stroke = start(&mut doc, &owned, &target, StrokeTool::Paint, Symmetry::default());
    let mut worker = StrokeWorker::spawn(stroke, doc.texture_sets[0].clone(), owned.clone()).unwrap();
    for p in points() {
        worker.add(vec![p]);
    }
    // no poll before: pen up at once, then wait
    let done = worker.wait(&mut doc.texture_sets[0]).done.expect("the stroke completes");
    done.ended.record(&mut doc);
    assert!(doc == want);
}
