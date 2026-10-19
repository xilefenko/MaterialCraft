//! [`ActiveStroke`]: the stroke under the pointer, from pen down to pen up: the brush crate's
//! [`DocumentStroke`] plus what it paints through (the set's surface, the camera at pen down), the lazy mouse in
//! front of it, and where it ended (the next straight line's start). A stroke from the 2D view has no camera: its
//! points are texel positions and it paints in texture space (PT-53). Projection strokes (PJ-26) also keep where
//! the projected image lay at pen down and paint its values ([`Through::projection`]).
//!
//! Where it is stamped ([`Stamping`]): on its own thread ([`StrokeWorker`]) whenever what it paints through can be
//! owned there (the surface, the picking scene, the camera at pen down), so the UI thread only hands points over
//! and picks up the paint each frame ([`ActiveStroke::poll`]); pen up then leaves a [`PendingStroke`] that
//! completes once the worker has painted everything. Strokes through the quick mask, a stencil or a projection
//! image (which the screen lends every frame) are stamped in place on the UI thread.

use super::super::document_state::OpenDocument;
use super::super::view_pick::ViewPicking;
use super::screen_image::ScreenImage;
use super::stroke_scene::StrokeScene;
use super::surfaces::Surface;
use crate::gui_paint::input::PointerSample;
use materialcraft::camera::Camera;
use materialcraft::mesh::Mesh;
use materialcraft::mesh::Sphere;
use materialcraft::paint::Document;
use materialcraft::paint::brush::{BrushPreset, Retouch};
use materialcraft::paint::stroke::{StrokeTool, Symmetry, ViewKind};
use materialcraft::projection::ViewScene;
use materialcraft::projection::{CpuViewProjector, ScreenImageTransform, ViewProjector};
use materialcraft::stamp::{ProjectedPaint, TexelFactor};
use materialcraft_brush::{
    Base, BrushError, DocumentStroke, Ended, EngineExtras, InputPoint, LazyMouse, LineAnchor, PaintTarget, PaintValues, Progress, SessionScene, StrokeSetup,
    StrokeWorker, TipAlpha,
};
use std::sync::Arc;

/// How to start a stroke.
pub struct StrokeStart {
    pub target: PaintTarget,
    pub set: usize,
    pub preset: BrushPreset,
    pub tool: StrokeTool,
    pub values: PaintValues,
    pub occlusion: bool,
    pub seed: u32,
    /// The 3D view's camera; `None` for a stroke in the 2D view (points in texels).
    pub camera: Option<Camera>,
    pub surface: Arc<Surface>,
    /// Symmetric copies in the record's form.
    pub symmetry: Symmetry,
    /// The lazy mouse, if on (never for straight lines).
    pub lazy: Option<LazyMouse>,
    /// Smudge and blur strength, clone offset.
    pub retouch: Retouch,
    /// On a see-through layer, the composite below it (the stroke starts from it where the layer has no pixels).
    pub base: Option<Base>,
    /// Where the projected image lies (projection strokes; it does not move during a stroke).
    pub placement: Option<ScreenImageTransform>,
    /// The alpha image of an alpha-image tip (round and procedural tips need none).
    pub tip: Option<TipAlpha>,
}

/// What a stroke paints through besides the surface, lent per call: the factors (quick mask, stencil) and, for
/// projection strokes, the projected image.
#[derive(Clone, Copy)]
pub struct Through<'a> {
    pub factors: &'a [&'a dyn TexelFactor],
    pub projection: Option<&'a ScreenImage>,
}

/// Where a stroke's dabs are stamped.
enum Stamping {
    /// On the UI thread, in each call.
    Here(DocumentStroke),
    /// On its own thread.
    Worker(StrokeWorker),
}

pub struct ActiveStroke {
    stamping: Stamping,
    set: usize,
    camera: Option<Camera>,
    surface: Arc<Surface>,
    /// Pen-down time, so samples are recorded in seconds since the stroke started.
    t0: f64,
    pub label: &'static str,
    lazy: Option<LazyMouse>,
    /// The last point painted (view coordinates).
    last: Option<[f32; 2]>,
    placement: Option<ScreenImageTransform>,
    /// What changed since the last [`ActiveStroke::take_changes`].
    changes: Changes,
    /// The id of the layer painted.
    layer: String,
    /// Pointer samples handed to the stroke so far.
    samples: usize,
}

/// What [`ActiveStroke::end`] left.
pub enum Ending {
    Ended(StrokeEnd),
    /// The stroke's worker is still painting; the stroke completes later.
    Pending(PendingStroke),
}

/// Where a stroke changed the composite of its texture set.
#[derive(Debug, Clone, PartialEq)]
pub enum Changes {
    /// Anywhere (preparing the target changed more than pixels).
    Whole,
    /// In these tiles of the set's grid.
    Tiles(Vec<(u32, u32)>),
}

impl Changes {
    /// Adds `tiles` (nothing to add to `Whole`).
    fn add(&mut self, tiles: Vec<(u32, u32)>) {
        if let Changes::Tiles(t) = self
            && !tiles.is_empty()
        {
            t.extend(tiles);
            t.sort_unstable();
            t.dedup();
        }
    }

    /// What changed since the last call (`None`: nothing).
    fn take(&mut self) -> Option<Changes> {
        match std::mem::replace(self, Changes::Tiles(Vec::new())) {
            Changes::Tiles(t) if t.is_empty() => None,
            c => Some(c),
        }
    }
}

/// How a stroke ended.
pub struct StrokeEnd {
    /// A problem to report.
    pub problem: Option<String>,
    /// Where it ended, for the next straight line.
    pub anchor: Option<LineAnchor>,
}

/// The 3D view's projector for `camera` (`None` for a 2D-view stroke); an error if the 3D view is not ready.
fn projector_for<'a>(picking: &'a ViewPicking, mesh: &'a Mesh, camera: Option<Camera>) -> Result<Option<CpuViewProjector<'a>>, BrushError> {
    match camera {
        Some(c) => picking.projector_on(mesh, c).map(Some).ok_or(BrushError::NoView),
        None => Ok(None),
    }
}

/// The pieces of a document a stroke needs at once: the mesh to look through, the set to paint.
fn split(doc: &mut Document, set: usize) -> Result<(&materialcraft::mesh::Mesh, &mut materialcraft::paint::TextureSet), BrushError> {
    let Document { mesh, texture_sets, .. } = doc;
    let set = texture_sets.get_mut(set).ok_or(BrushError::NotPaintable("the texture set is gone"))?;
    Ok((mesh, set))
}

impl ActiveStroke {
    /// Pen down at `at`: prepares the target, starts the stroke and paints its first sample. On failure after
    /// the target was prepared, what was done is recorded so the history stays whole.
    pub fn begin(doc: &mut OpenDocument, picking: &ViewPicking, start: StrokeStart, at: PointerSample, through: Through) -> Result<ActiveStroke, BrushError> {
        let label = match start.tool {
            StrokeTool::Eraser => "Erase",
            StrokeTool::Smudge => "Smudge",
            StrokeTool::Blur => "Blur",
            StrokeTool::Clone => "Clone",
            StrokeTool::Projection => "Projection paint",
            StrokeTool::Paint => "Paint",
        };
        let model = doc.doc().mesh.bounding_sphere().unwrap_or(Sphere { centre: [0.0; 3], radius: 1.0 });
        let setup = StrokeSetup {
            preset: start.preset,
            tool: start.tool,
            seed: start.seed,
            symmetry: start.symmetry,
            view: if start.camera.is_some() { ViewKind::Mesh } else { ViewKind::Uv },
            values: start.values.clone(),
            model,
            occlusion: start.occlusion,
            retouch: start.retouch,
        };
        let layer = start.target.layer.layer.clone();
        let mut prepared = DocumentStroke::prepare(doc.doc_for_strokes(), start.target, &start.values)?;
        let whole: bool;
        let result = {
            let (mesh, set) = split(doc.doc_for_strokes(), start.set)?;
            if let Some(base) = start.base {
                prepared.see_through(set, base);
            }
            whole = prepared.changed_document();
            let projector = projector_for(picking, mesh, start.camera)?;
            let view = projector.as_ref().map(|p| p as &dyn ViewProjector);
            let projected = view.and_then(|v| projected(through, start.placement, &start.surface, v));
            let scene = SessionScene {
                surface: &start.surface.map,
                index: &start.surface.index,
                projector: view,
                factors: through.factors,
                projected: projected.as_ref(),
            };
            let extras = EngineExtras { tip: start.tip, ..EngineExtras::default() };
            prepared.start(setup, &scene, mesh, &|part| picking.is_shown(part), set).map(|s| s.with_extras(extras))
        };
        let stroke = match result {
            Ok(s) => s,
            Err(failed) => {
                let (ended, e) = *failed;
                record(doc, ended, label);
                return Err(e);
            }
        };
        let off_thread = through.factors.is_empty() && start.placement.is_none();
        let stamping = match off_thread.then(|| owned_view(picking, doc, start.camera)).flatten() {
            Some(view) => {
                let set = doc.doc().texture_sets.get(start.set).cloned().ok_or(BrushError::NotPaintable("the texture set is gone"))?;
                Stamping::Worker(StrokeWorker::spawn(stroke, set, StrokeScene { surface: start.surface.clone(), view })?)
            }
            None => Stamping::Here(stroke),
        };
        let mut active = ActiveStroke {
            stamping,
            set: start.set,
            camera: start.camera,
            surface: start.surface,
            t0: at.time,
            label,
            lazy: start.lazy,
            last: None,
            placement: start.placement,
            changes: if whole { Changes::Whole } else { Changes::Tiles(Vec::new()) },
            layer,
            samples: 0,
        };
        active.add(doc, picking, &[at], through)?;
        Ok(active)
    }

    /// What the stroke changed since the last call, for recompositing (`None`: nothing).
    pub fn take_changes(&mut self) -> Option<Changes> {
        self.changes.take()
    }

    /// True for a stroke from the 2D view.
    pub fn in_uv(&self) -> bool {
        self.camera.is_none()
    }

    /// True for a projection stroke (it paints through the projection image).
    pub fn projects(&self) -> bool {
        self.placement.is_some()
    }

    /// Dabs placed so far (the performance benchmarks count them).
    pub fn dabs(&self) -> usize {
        match &self.stamping {
            Stamping::Here(s) => s.dabs(),
            Stamping::Worker(w) => w.dabs(),
        }
    }

    /// Pointer samples whose dabs are in the document.
    pub fn samples_painted(&self) -> usize {
        match &self.stamping {
            Stamping::Here(_) => self.samples,
            Stamping::Worker(w) => w.samples_painted(),
        }
    }

    /// Time spent stamping on the worker so far (zero for strokes stamped in place).
    pub fn worker_busy(&self) -> std::time::Duration {
        match &self.stamping {
            Stamping::Here(_) => std::time::Duration::ZERO,
            Stamping::Worker(w) => w.busy(),
        }
    }

    /// Waits until the worker has painted every sample handed over (the benchmarks' frame-by-frame mode).
    pub fn wait_painted(&self) {
        if let Stamping::Worker(w) = &self.stamping {
            w.wait_painted();
        }
    }

    /// Puts what the worker painted since the last call into the document; returns whether any pixel changed.
    pub fn poll(&mut self, doc: &mut OpenDocument) -> Result<bool, BrushError> {
        let Stamping::Worker(worker) = &mut self.stamping else { return Ok(false) };
        let set = doc.doc_for_strokes().texture_sets.get_mut(self.set).ok_or(BrushError::NotPaintable("the texture set is gone"))?;
        let progress = worker.poll(set);
        let changed = !progress.changed.is_empty();
        self.changes.add(progress.changed);
        match progress.error {
            Some(e) => Err(e),
            None => Ok(changed),
        }
    }

    /// The id of the layer being painted.
    pub fn layer(&self) -> &str {
        &self.layer
    }

    /// The texture set being painted.
    pub fn set(&self) -> usize {
        self.set
    }

    /// Paints more samples; returns whether any pixel changed.
    pub fn add(&mut self, doc: &mut OpenDocument, picking: &ViewPicking, samples: &[PointerSample], through: Through) -> Result<bool, BrushError> {
        if samples.is_empty() {
            return Ok(false);
        }
        self.samples += samples.len();
        let mut points: Vec<InputPoint> = samples
            .iter()
            .map(|s| InputPoint { pressure: s.pressure, tilt: s.tilt, rotation: s.rotation, ..InputPoint::at(s.pos, (s.time - self.t0).max(0.0)) })
            .collect();
        if let Some(lazy) = &mut self.lazy {
            points = lazy.push(&points);
        }
        let Some(last) = points.last() else { return Ok(false) };
        self.last = Some(last.pos);
        let stroke = match &mut self.stamping {
            Stamping::Here(s) => s,
            Stamping::Worker(w) => {
                w.add(points);
                return Ok(false);
            }
        };
        let (mesh, set) = split(doc.doc_for_strokes(), self.set)?;
        let projector = projector_for(picking, mesh, self.camera)?;
        let view = projector.as_ref().map(|p| p as &dyn ViewProjector);
        let projected = view.and_then(|v| projected(through, self.placement, &self.surface, v));
        let scene =
            SessionScene { surface: &self.surface.map, index: &self.surface.index, projector: view, factors: through.factors, projected: projected.as_ref() };
        let changed = stroke.add(&scene, &points, set)?;
        let any = !changed.is_empty();
        self.changes.add(changed);
        Ok(any)
    }

    /// The lazy mouse's string `(brush, pointer)` while the brush lags behind.
    pub fn string(&self) -> Option<([f32; 2], [f32; 2])> {
        self.lazy.as_ref().and_then(LazyMouse::string)
    }

    /// Pen up: paints the last stamps and records the stroke as one history step; a stroke on a worker completes
    /// later ([`Ending::Pending`]).
    pub fn end(self, doc: &mut OpenDocument, picking: &ViewPicking, through: Through) -> Ending {
        let ActiveStroke { stamping, set, camera, surface, label, last, placement, changes, layer, .. } = self;
        let stroke = match stamping {
            Stamping::Here(s) => s,
            Stamping::Worker(mut worker) => {
                worker.finish();
                let projector = camera.and_then(|c| picking.projector_on(&doc.doc().mesh, c));
                let view = projector.as_ref().map(|p| p as &dyn ViewProjector);
                let anchor = last.map(|end| LineAnchor::at(end, view));
                return Ending::Pending(PendingStroke { worker, set, label, layer, changes, anchor });
            }
        };
        let mut anchor = None;
        let (ended, mut problem) = match split(doc.doc_for_strokes(), set) {
            Ok((mesh, set_ref)) => match projector_for(picking, mesh, camera) {
                Ok(projector) => {
                    let view = projector.as_ref().map(|p| p as &dyn ViewProjector);
                    anchor = last.map(|end| LineAnchor::at(end, view));
                    let projected = view.and_then(|v| projected(through, placement, &surface, v));
                    let scene =
                        SessionScene { surface: &surface.map, index: &surface.index, projector: view, factors: through.factors, projected: projected.as_ref() };
                    let (ended, result) = stroke.finish(&scene, set_ref);
                    (ended, result.err().map(|e| e.to_string()))
                }
                // the mesh changed under the stroke: keep what was painted, without its record
                Err(_) => (stroke.abandon(), Some("the 3D view changed during the stroke".to_string())),
            },
            Err(e) => (stroke.abandon(), Some(e.to_string())),
        };
        if let Some(e) = record(doc, ended, label) {
            problem.get_or_insert(e);
        }
        Ending::Ended(StrokeEnd { problem, anchor })
    }
}

/// The view a worker owns.
type OwnedView = (Arc<ViewScene>, Arc<Document>, Camera);

/// The 3D view as a worker owns it, for a stroke through `camera` (`Some(None)` for a 2D-view stroke); `None` if
/// the picking scene can't be shared with it.
fn owned_view(picking: &ViewPicking, doc: &OpenDocument, camera: Option<Camera>) -> Option<Option<OwnedView>> {
    let Some(camera) = camera else { return Some(None) };
    let (scene, from) = picking.shared()?;
    // the scene must be the one of the document's mesh (as in-place strokes check through `projector_on`)
    (scene.matches(&from.mesh) && scene.matches(&doc.doc().mesh)).then_some(Some((scene, from, camera)))
}

/// The projection a projection stroke paints through `view` (none for other strokes).
fn projected<'a>(
    through: Through<'a>,
    placement: Option<ScreenImageTransform>,
    surface: &'a Surface,
    view: &'a dyn ViewProjector,
) -> Option<ProjectedPaint<'a>> {
    through.projection.zip(placement).map(|(image, p)| image.projected(&surface.map, view, p))
}

/// Records an ended stroke's inverses as one history step; returns a problem to report.
fn record(doc: &mut OpenDocument, ended: Ended, label: &str) -> Option<String> {
    let (inverses, problem) = ended.record(doc.doc_for_strokes());
    doc.record_stroke(label, inverses);
    problem.map(|e| format!("the stroke was painted but not added to the stroke record: {e}"))
}

/// A stroke after pen up whose worker is still painting: it completes ([`PendingStroke::poll`],
/// [`PendingStroke::wait`]) once every queued sample and the last dabs are in the document, and is then recorded
/// as one history step, exactly as a stroke stamped in place.
pub struct PendingStroke {
    worker: StrokeWorker,
    set: usize,
    label: &'static str,
    layer: String,
    changes: Changes,
    anchor: Option<LineAnchor>,
}

impl PendingStroke {
    /// The texture set being painted.
    pub fn set(&self) -> usize {
        self.set
    }

    /// The id of the layer being painted.
    pub fn layer(&self) -> &str {
        &self.layer
    }

    /// Dabs placed so far.
    pub fn dabs(&self) -> usize {
        self.worker.dabs()
    }

    /// Time spent stamping on the worker.
    pub fn worker_busy(&self) -> std::time::Duration {
        self.worker.busy()
    }

    /// What the stroke changed since the last call (`None`: nothing).
    pub fn take_changes(&mut self) -> Option<Changes> {
        self.changes.take()
    }

    /// Takes what the worker painted; once it is done, records the stroke and returns how it ended.
    pub fn poll(&mut self, doc: &mut OpenDocument) -> Option<StrokeEnd> {
        let Some(set) = doc.doc_for_strokes().texture_sets.get_mut(self.set) else {
            return Some(StrokeEnd { problem: Some("the texture set is gone".into()), anchor: None });
        };
        let progress = self.worker.poll(set);
        self.complete(doc, progress)
    }

    /// Waits until the worker is done, then records the stroke and returns how it ended.
    pub fn wait(mut self, doc: &mut OpenDocument) -> StrokeEnd {
        let Some(set) = doc.doc_for_strokes().texture_sets.get_mut(self.set) else {
            return StrokeEnd { problem: Some("the texture set is gone".into()), anchor: None };
        };
        let progress = self.worker.wait(set);
        let lost = StrokeEnd { problem: Some("the stroke stopped unexpectedly".into()), anchor: None };
        self.complete(doc, progress).unwrap_or(lost)
    }

    fn complete(&mut self, doc: &mut OpenDocument, progress: Progress) -> Option<StrokeEnd> {
        self.changes.add(progress.changed);
        let mut problem = progress.error.map(|e| e.to_string());
        if let Some(done) = progress.done {
            if let Err(e) = done.result {
                problem.get_or_insert(e.to_string());
            }
            if let Some(e) = record(doc, done.ended, self.label) {
                problem.get_or_insert(e);
            }
            return Some(StrokeEnd { problem, anchor: self.anchor });
        }
        // a worker that failed: what it painted stays, without a history step
        progress.lost.then(|| StrokeEnd { problem: problem.or(Some("the stroke stopped unexpectedly".into())), anchor: self.anchor })
    }
}
