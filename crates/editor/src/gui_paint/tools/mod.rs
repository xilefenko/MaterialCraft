//! The brush tools at work (PT-01, PT-20, PT-54, PT-55): [`Painting`] turns the viewport's stroke gesture into
//! strokes of the paint, eraser, smudge (or blur) and clone tools on the selected paintable target, one history step
//! each. The tool choice lives in the shell ([`super::shell::tools`]); the settings in [`BrushTools`]; the work in
//! `materialcraft-brush`.
//!
//! State machine: idle → (pen down on a paintable target with a brush tool and a ready surface) painting → (pen
//! up) finishing → idle; any document change ends a stroke at once (waiting for it). While painting, the tiles
//! painted are recomposited every frame; at pen up the set once more. A frame's pointer moves go to the stroke at
//! once. Strokes are stamped on their own thread where they can be ([`active`]): the frame hands the samples over
//! and picks up what was painted, so it keeps its pace at any spacing; the stroke may trail the pointer and
//! completes after pen up (finishing). A straight-line click (PT-31) is a whole stroke at once, from where the previous stroke ended
//! ([`LineAnchor`], re-projected into the current view) to the click; without a previous stroke it paints one
//! stamp. Strokes go through the lazy mouse and symmetry of [`helpers::StrokeHelpers`]; [`overlay`] draws their
//! visual aids.
//!
//! Quick mask (PT-42): while it is edited, paint and eraser strokes go into it ([`mask_stroke`]); while it is
//! applied, every document stroke is multiplied by it. The polygon fill tool ([`polygon_fill`]) turns the same
//! gesture into picks.
//!
//! Retouch tools ([`retouch`], PT-23…PT-30): smudge and blur run local kernels, clone copies from the source marked
//! with its key (aligned or fixed). Strokes on a see-through layer start from the composite below it; the layer is
//! painted again from its record whenever the layers below it change.
//!
//! Stencil and projection (PT-15, PJ-26, [`screen_image`]): a stencil laid over the view masks every document
//! stroke; the projection tool paints the projection image's channels through the paint tool's brush. Strokes
//! through either have no symmetry (ADR 0008 mc2-q-pt-04c).
//!
//! The 2D view (PT-53): its gestures arrive with [`PaintFrame::uv`] set. Brush strokes paint in texture space (the
//! points become texel positions of the target set, the brush size counts texels, symmetry is mirrored through the
//! mesh as in 3D); the polygon fill picks in UV space. The stencil and the projection image belong to the 3D view
//! and leave 2D strokes alone; the quick mask is edited in the 3D view. The material picker ([`material_picker`]) is handled
//! by the screen, which owns the composite's fill context.

mod active;
mod brush_cursor;
pub mod brush_state;
mod events;
pub mod helpers;
mod mask_stroke;
pub mod material_picker;
mod overlay;
pub mod polygon_fill;
mod retouch;
pub mod screen_image;
mod stroke_scene;
mod surfaces;
mod uv_overlay;
#[cfg(test)]
mod uv_tests;

use super::document_state::OpenDocument;
use super::input::{PointerSample, StrokeInput};
use super::shell::tools::Tool;
use super::view_pick::ViewPicking;
use super::view2d::UvMap;
use active::{ActiveStroke, Changes, Ending, PendingStroke, StrokeEnd, StrokeStart, Through};
pub use brush_state::BrushTools;
pub use events::ToolEvent;
use mask_stroke::{ActiveMaskStroke, MaskStrokeStart};
use materialcraft::camera::Camera;
use materialcraft::paint::brush::Tip;
use materialcraft::paint::stroke::StrokeTool;
use materialcraft::projection::ViewProjector;
use materialcraft::stamp::{Stencil, TexelFactor};
use materialcraft::symmetry::SymmetrySpec;
use materialcraft_brush::stroke::line::SNAP_DEGREES;
use materialcraft_brush::stroke::{line_points, snap_angle};
use materialcraft_brush::symmetry::to_record;
use materialcraft_brush::{FolderAlphas, LineAnchor, PaintTarget, QuickMask, alpha_for};
use screen_image::ScreenImages;
use std::time::{Duration, Instant};
use surfaces::SurfaceCache;

/// How often the painted set is recomposited during a quick mask stroke (which recomposites it whole); document
/// strokes update the tiles they painted every frame.
const REFRESH: Duration = Duration::from_millis(120);

/// How long a frame waits for the stroke's worker before it shows what is painted so far: a stroke that keeps up
/// shows in the frame of its input, one that can't trails without holding the frame up.
const STAMP_WAIT: Duration = Duration::from_millis(4);

/// What one frame of painting asks of the screen.
#[derive(Debug, Default, PartialEq)]
pub struct PaintReport {
    /// Recomposite this texture set.
    pub refresh: Option<usize>,
    /// Only these tiles of its grid changed (`None`: recomposite it whole).
    pub tiles: Option<Painted>,
    /// Something to tell the user.
    pub message: Option<String>,
    /// True while a stroke is in progress (keep frames coming).
    pub busy: bool,
    /// The quick mask changed (its tint is out of date).
    pub quick_mask: bool,
}

/// Where a frame of a stroke painted.
#[derive(Debug, Clone, PartialEq)]
pub struct Painted {
    /// The layer whose content or mask was painted (its id).
    pub layer: String,
    /// The tiles of the set's grid that changed.
    pub tiles: Vec<(u32, u32)>,
}

/// What the tools work with this frame.
pub struct PaintFrame<'a> {
    pub doc: &'a mut OpenDocument,
    pub picking: &'a ViewPicking,
    /// The view's camera (screen positions relative to the view's corner).
    pub camera: Option<Camera>,
    pub tool: Tool,
    /// The selected paintable target and its texture set's index.
    pub target: Option<(PaintTarget, usize)>,
    /// The input comes from the 2D view, laid out like this (view positions are turned into UV and texels).
    pub uv: Option<UvMap>,
}

/// The brush tools' session state.
#[derive(Default)]
pub struct Painting {
    pub brush: BrushTools,
    surfaces: SurfaceCache,
    active: Option<ActiveStroke>,
    /// A stroke after pen up whose worker is still painting, and whether it was painted in the 2D view.
    finishing: Option<(PendingStroke, bool)>,
    /// Wait for the stroke's worker in every frame (the benchmarks' frame-by-frame mode): stamping then counts
    /// in the frame, as before it had a thread of its own.
    pub wait_for_stamps: bool,
    last_refresh: Option<Instant>,
    strokes: u32,
    /// Where the last stroke ended (the next straight line's start).
    anchor: Option<LineAnchor>,
    /// The last stroke was painted in the 2D view (its anchor is a texel position).
    anchor_uv: bool,
    guides: overlay::GuideCache,
    /// The session's quick mask (PT-42), honoured by every document stroke while applied.
    pub quick_mask: QuickMask,
    /// A stroke into the quick mask while it is edited.
    mask_active: Option<ActiveMaskStroke>,
    /// The polygon fill pick gesture in progress.
    gesture: Option<polygon_fill::PickGesture>,
    topologies: polygon_fill::TopologyCache,
    /// The clone source and the see-through layers' cache.
    retouch: retouch::RetouchState,
    /// The stencil and the projection tool's image over the view.
    pub images: ScreenImages,
    /// Where alpha-image tips are found: the asset library folders (AS-06).
    pub alphas: FolderAlphas,
}

impl Painting {
    /// Handles this frame's stroke input.
    pub fn frame(&mut self, input: Vec<StrokeInput>, mut f: PaintFrame) -> PaintReport {
        let mut report = PaintReport::default();
        self.surfaces.poll();
        if let Some(e) = self.surfaces.error.take() {
            report.message = Some(format!("The brush can't paint this texture set: {e}."));
        }
        let tool = self.brush.stroke_tool(f.tool);
        // see-through layers follow the layers below them (PT-25)
        if !self.is_stroking()
            && let Some((_, set)) = &f.target
        {
            self.keep_see_through(f.doc, f.picking, *set, &mut report);
        }
        if let Some(camera) = f.camera {
            self.images.place(camera.viewport);
        }
        if f.tool == Tool::PolygonFill {
            let mut polygon = self.polygon_frame(input, f);
            polygon.message = polygon.message.or(report.message);
            polygon.refresh = polygon.refresh.or(report.refresh);
            return polygon;
        }
        let pressed = input.iter().any(|ev| matches!(ev, StrokeInput::Down(_) | StrokeInput::Line { .. }));
        let input = match f.uv {
            Some(_) if self.quick_mask.is_editing() || f.tool == Tool::Projection => {
                if pressed {
                    report.message = Some(match f.tool {
                        Tool::Projection => "The projection tool paints in the 3D view.".into(),
                        _ => "The quick mask is edited in the 3D view.".into(),
                    });
                }
                return report;
            }
            Some(map) => match f.target.as_ref().and_then(|(_, set)| f.doc.doc().texture_sets.get(*set)) {
                Some(s) => to_texels(input, map, s.resolution),
                None => input,
            },
            None => input,
        };
        if let (true, Some(tool)) = (self.quick_mask.is_editing(), tool) {
            if !matches!(tool, StrokeTool::Paint | StrokeTool::Eraser) {
                if input.iter().any(|ev| matches!(ev, StrokeInput::Down(_))) {
                    report.message = Some("Only the paint and eraser tools work on the quick mask while it is edited.".into());
                }
                return report;
            }
            return self.mask_frame(input, f, tool, report);
        }
        // keep the selected set's surface warm, so the first stroke does not wait
        if let (Some(_), Some((_, set))) = (tool, &f.target) {
            let _ = self.surfaces.get(f.doc, *set);
        }
        let mut painted = None;
        // a frame's moves go to the stroke at once
        let mut moves: Vec<PointerSample> = Vec::new();
        for ev in input {
            if let StrokeInput::Move(at) = ev {
                moves.push(at);
                continue;
            }
            self.moves(&mut f, std::mem::take(&mut moves), &mut painted, &mut report);
            match ev {
                StrokeInput::Down(at) => {
                    self.end(f.doc, f.picking, &mut report);
                    self.complete(f.doc, &mut report);
                    if let Some(set) = self.begin(&mut f, tool, at, true, &mut report) {
                        painted = Some(set);
                    }
                }
                StrokeInput::Line { to, snap } => {
                    self.end(f.doc, f.picking, &mut report);
                    self.complete(f.doc, &mut report);
                    let start = self.line_start(&f);
                    let begin_at = PointerSample { pos: start.unwrap_or(to.pos), ..to };
                    if let Some(set) = self.begin(&mut f, tool, begin_at, false, &mut report) {
                        painted = Some(set);
                    }
                    if let (Some(from), Some(a)) = (start, &mut self.active) {
                        let end = if snap { snap_angle(from, to.pos, SNAP_DEGREES) } else { to.pos };
                        let samples: Vec<PointerSample> = line_points(from, end, to.pressure)
                            .iter()
                            .skip(1)
                            .map(|p| PointerSample { pos: p.pos, pressure: p.pressure, time: to.time + p.time, ..to })
                            .collect();
                        let (factor, stencil) = (factor_for(&self.quick_mask, f.doc, a.set()), stencil_of(&self.images, a.in_uv()));
                        let fs = factors(&factor, &stencil);
                        if let Err(e) = a.add(f.doc, f.picking, &samples, through(&fs, &self.images, f.tool)) {
                            report.message = Some(format!("The line stopped: {e}."));
                        }
                    }
                    self.end(f.doc, f.picking, &mut report);
                }
                StrokeInput::Move(_) => {}
                StrokeInput::Up => self.end(f.doc, f.picking, &mut report),
            }
        }
        self.moves(&mut f, moves, &mut painted, &mut report);
        // what the stroke's worker painted meanwhile
        if let Some(a) = &mut self.active {
            // the paint shows in this frame if the worker gets there within a short wait, else it trails
            a.wait_painted(if self.wait_for_stamps { None } else { Some(STAMP_WAIT) });
            match a.poll(f.doc) {
                Ok(true) => painted = Some(a.set()),
                Ok(false) => {}
                Err(e) => {
                    report.message = Some(format!("The stroke stopped: {e}."));
                    self.end(f.doc, f.picking, &mut report);
                }
            }
        }
        if self.wait_for_stamps {
            self.complete(f.doc, &mut report);
        }
        self.poll_finishing(f.doc, &mut painted, &mut report);
        // every frame, the tiles the stroke painted (a stroke that ended recomposites its set whole)
        if report.refresh.is_none()
            && let Some(set) = painted
        {
            match self.changes_of(set) {
                Some((_, None)) => {}
                Some((layer, Some(Changes::Tiles(tiles)))) => {
                    report.refresh = Some(set);
                    report.tiles = Some(Painted { layer, tiles });
                }
                Some((_, Some(Changes::Whole))) | None => report.refresh = Some(set),
            }
        }
        report.busy = self.active.is_some() || self.finishing.is_some() || self.surfaces.is_building();
        report
    }

    /// Hands `moves` (a frame's pointer moves, in order) to the stroke in progress at once.
    fn moves(&mut self, f: &mut PaintFrame, moves: Vec<PointerSample>, painted: &mut Option<usize>, report: &mut PaintReport) {
        let Some(a) = &mut self.active else { return };
        if moves.is_empty() {
            return;
        }
        let (factor, stencil) = (factor_for(&self.quick_mask, f.doc, a.set()), stencil_of(&self.images, a.in_uv()));
        let fs = factors(&factor, &stencil);
        match a.add(f.doc, f.picking, &moves, through(&fs, &self.images, f.tool)) {
            Ok(true) => *painted = Some(a.set()),
            Ok(false) => {}
            Err(e) => {
                report.message = Some(format!("The stroke stopped: {e}."));
                self.end(f.doc, f.picking, report);
            }
        }
    }

    /// The layer painted in `set` and what changed there since the last call: the stroke in progress, else the
    /// one finishing.
    fn changes_of(&mut self, set: usize) -> Option<(String, Option<Changes>)> {
        if let Some(a) = self.active.as_mut().filter(|a| a.set() == set) {
            return Some((a.layer().to_string(), a.take_changes()));
        }
        let (p, _) = self.finishing.as_mut().filter(|(p, _)| p.set() == set)?;
        Some((p.layer().to_string(), p.take_changes()))
    }

    /// Takes what a finishing stroke's worker painted; records the stroke once it is done.
    fn poll_finishing(&mut self, doc: &mut OpenDocument, painted: &mut Option<usize>, report: &mut PaintReport) {
        let Some((p, _)) = &mut self.finishing else { return };
        let set = p.set();
        p.wait_done(STAMP_WAIT);
        match p.poll(doc) {
            Some(end) => {
                let uv = self.finishing.take().is_some_and(|(_, uv)| uv);
                self.ended(end, set, uv, report);
            }
            None => *painted = Some(set),
        }
    }

    /// Waits for a finishing stroke and records it (before anything else may change the document).
    fn complete(&mut self, doc: &mut OpenDocument, report: &mut PaintReport) {
        if let Some((p, uv)) = self.finishing.take() {
            let set = p.set();
            let end = p.wait(doc);
            self.ended(end, set, uv, report);
        }
    }

    /// Starts a stroke at `at` (with the lazy mouse if `lazy` and it is on); returns the set it painted or tried.
    fn begin(&mut self, f: &mut PaintFrame, tool: Option<StrokeTool>, at: PointerSample, lazy: bool, report: &mut PaintReport) -> Option<usize> {
        let (Some(tool), Some((target, set))) = (tool, f.target.clone()) else { return None };
        // a 2D-view stroke has no camera
        let camera = if f.uv.is_some() { None } else { Some(f.camera?) };
        let picking = f.picking;
        let doc = &mut *f.doc;
        let Some(surface) = self.surfaces.get(doc, set) else {
            report.message = Some("The brush is getting this texture set ready; paint again in a moment.".into());
            return None;
        };
        let retouch = match self.retouch_for(doc, picking, camera, tool, at, set) {
            Ok(r) => r,
            Err(why) => {
                report.message = Some(format!("Nothing was painted: {why}."));
                return Some(set);
            }
        };
        // a see-through layer is brought up to date first, and the stroke works on the composite below it
        let base = self.see_through_base(doc, picking, &surface, &target, set);
        let set_ref = doc.doc().texture_sets.get(set)?;
        let preset = self.brush.preset(f.tool).cloned().unwrap_or_default();
        let tip = alpha_for(&preset.tip, &self.alphas);
        if let (Tip::Alpha(path), None) = (&preset.tip, &tip) {
            report.message = Some(format!("The tip {path} was not found in the asset libraries; the brush paints round."));
        }
        let mut values = self.brush.values_for(set_ref);
        let placement = match (tool, &mut self.images.projection, camera) {
            (StrokeTool::Projection, Some(image), Some(camera)) => {
                // only the channels the projection provides are painted
                let provided = image.channels();
                values.channels.iter_mut().for_each(|(c, _, on)| *on &= provided.contains(c));
                Some(image.placement(camera.viewport))
            }
            (StrokeTool::Projection, Some(_), None) => {
                report.message = Some("The projection tool paints in the 3D view.".into());
                return None;
            }
            (StrokeTool::Projection, None, _) => {
                report.message = Some("Load a projection image or drop an image or material from the assets onto the view first.".into());
                return None;
            }
            _ => None,
        };
        self.strokes = self.strokes.wrapping_add(1);
        let helpers = &self.brush.helpers;
        // projection painting and stencils have no symmetry (ADR 0008 mc2-q-pt-04c)
        let stencilled = camera.is_some() && self.images.stencil.is_some();
        let symmetry = if placement.is_some() || stencilled { SymmetrySpec::None } else { helpers.symmetry.spec() };
        let start = StrokeStart {
            target,
            set,
            preset,
            tool,
            values,
            occlusion: self.brush.occlusion,
            seed: self.strokes,
            camera,
            surface,
            symmetry: to_record(symmetry),
            lazy: if lazy { helpers.lazy.start() } else { None },
            retouch,
            base,
            placement,
            tip,
        };
        let (factor, stencil) = (factor_for(&self.quick_mask, doc, set), self.stencil_for(camera.is_none()));
        let fs = factors(&factor, &stencil);
        match ActiveStroke::begin(doc, picking, start, at, through(&fs, &self.images, f.tool)) {
            Ok(a) => {
                self.active = Some(a);
            }
            Err(e) => report.message = Some(format!("Nothing was painted: {e}.")),
        }
        Some(set)
    }

    /// Where a straight line starts in the current view: the previous stroke's end, re-projected (PT-33); in the
    /// 2D view the previous 2D stroke's end texel.
    fn line_start(&self, f: &PaintFrame) -> Option<[f32; 2]> {
        let anchor = self.anchor?;
        if f.uv.is_some() || self.anchor_uv {
            return (f.uv.is_some() && self.anchor_uv).then(|| anchor.start(None)).flatten();
        }
        let camera = f.camera?;
        let projector = f.picking.projector_on(&f.doc.doc().mesh, camera);
        anchor.start(projector.as_ref().map(|p| p as &dyn ViewProjector))
    }

    /// Ends a stroke in progress and waits until it is complete (before anything else changes the document).
    pub fn finish_stroke(&mut self, doc: &mut OpenDocument, picking: &ViewPicking) -> PaintReport {
        let mut report = PaintReport::default();
        self.end(doc, picking, &mut report);
        self.complete(doc, &mut report);
        report
    }

    /// Texture set `set`'s surface once built (starts building it otherwise), for other users of the painted set's
    /// surface (the projection gizmo's UV handle).
    pub fn surface_of(&mut self, doc: &OpenDocument, set: usize) -> Option<std::sync::Arc<surfaces::Surface>> {
        self.surfaces.get(doc, set)
    }

    /// True once texture set `set`'s surface is built (starts building it otherwise).
    pub fn surface_ready(&mut self, doc: &OpenDocument, set: usize) -> bool {
        self.surfaces.get(doc, set).is_some()
    }

    /// Dabs the stroke in progress (or finishing) has placed so far (the performance benchmarks count them).
    pub fn stroke_dabs(&self) -> Option<usize> {
        self.active.as_ref().map(ActiveStroke::dabs).or_else(|| self.finishing.as_ref().map(|(p, _)| p.dabs()))
    }

    /// Time the stroke in progress (or finishing) spent stamping on its worker (the benchmarks' per-dab cost).
    pub fn stroke_worker_busy(&self) -> Option<Duration> {
        self.active.as_ref().map(ActiveStroke::worker_busy).or_else(|| self.finishing.as_ref().map(|(p, _)| p.worker_busy()))
    }

    /// Pointer samples of the stroke in progress whose dabs are in the document.
    pub fn samples_stamped(&self) -> usize {
        self.active.as_ref().map_or(0, ActiveStroke::samples_painted)
    }

    /// True while a stroke (also one finishing after pen up) or a pick gesture is in progress.
    pub fn is_stroking(&self) -> bool {
        self.active.is_some() || self.finishing.is_some() || self.mask_active.is_some() || self.gesture.is_some()
    }

    /// Forgets the document's state (another document was opened or it was closed).
    pub fn forget_document(&mut self) {
        self.active = None;
        self.finishing = None;
        self.anchor = None;
        self.anchor_uv = false;
        self.surfaces.clear();
        self.mask_active = None;
        self.gesture = None;
        self.quick_mask.forget();
        self.topologies = Default::default();
        self.retouch.clear();
    }

    /// Paints the see-through layers of `set` again if the layers below them changed (once its surface is ready).
    fn keep_see_through(&mut self, doc: &mut OpenDocument, picking: &ViewPicking, set: usize, report: &mut PaintReport) {
        let has_any = doc.doc().texture_sets.get(set).is_some_and(|s| !materialcraft::paint::see_through::see_through_layers(s).is_empty());
        if !has_any {
            return;
        }
        let Some(surface) = self.surfaces.get(doc, set) else { return };
        let (repainted, problem) = self.refresh_see_through(doc, picking, &surface, set);
        if repainted {
            report.refresh = Some(set);
        }
        if problem.is_some() {
            report.message = problem;
        }
    }

    fn end(&mut self, doc: &mut OpenDocument, picking: &ViewPicking, report: &mut PaintReport) {
        self.gesture = None;
        self.end_mask(doc, picking, report);
        let Some(a) = self.active.take() else { return };
        // one stroke finishing at a time
        self.complete(doc, report);
        let set = a.set();
        let (factor, stencil) = (factor_for(&self.quick_mask, doc, set), self.stencil_for(a.in_uv()));
        let fs = factors(&factor, &stencil);
        let projection = a.projects().then_some(self.images.projection.as_ref()).flatten();
        let a_in_uv = a.in_uv();
        match a.end(doc, picking, Through { factors: &fs, projection }) {
            Ending::Ended(end) => self.ended(end, set, a_in_uv, report),
            Ending::Pending(p) => self.finishing = Some((p, a_in_uv)),
        }
    }

    /// A stroke into `set` is complete and recorded: its set is recomposited whole.
    fn ended(&mut self, end: StrokeEnd, set: usize, uv: bool, report: &mut PaintReport) {
        if let Some(problem) = end.problem {
            report.message = Some(format!("The stroke had a problem: {problem}."));
        }
        if end.anchor.is_some() {
            self.anchor = end.anchor;
            self.anchor_uv = uv;
        }
        report.refresh = Some(set);
        self.last_refresh = Some(Instant::now());
    }
}

/// The input of a 2D-view gesture with its positions turned into texel positions of a `resolution` texture.
fn to_texels(input: Vec<StrokeInput>, map: UvMap, resolution: (u32, u32)) -> Vec<StrokeInput> {
    let at = |s: PointerSample| PointerSample { pos: map.texel(s.pos, resolution), ..s };
    input
        .into_iter()
        .map(|ev| match ev {
            StrokeInput::Down(s) => StrokeInput::Down(at(s)),
            StrokeInput::Move(s) => StrokeInput::Move(at(s)),
            StrokeInput::Line { to, snap } => StrokeInput::Line { to: at(to), snap },
            StrokeInput::Up => StrokeInput::Up,
        })
        .collect()
}

impl Painting {
    /// The stencil's factor for a stroke (none for a 2D-view stroke: the stencil lies over the 3D view).
    fn stencil_for(&self, uv: bool) -> Option<Stencil<'_>> {
        stencil_of(&self.images, uv)
    }
}

fn stencil_of(images: &ScreenImages, uv: bool) -> Option<Stencil<'_>> {
    if uv { None } else { images.stencil_factor() }
}

/// The quick mask's factor for strokes into texture set `set`, while it is applied.
fn factor_for<'a>(mask: &'a QuickMask, doc: &OpenDocument, set: usize) -> Option<materialcraft::stamp::QuickMask<'a>> {
    let s = doc.doc().texture_sets.get(set)?;
    mask.factor(&s.id, s.resolution)
}

/// The factors of a document stroke: the quick mask while applied, the stencil while there is one.
fn factors<'a>(factor: &'a Option<materialcraft::stamp::QuickMask<'a>>, stencil: &'a Option<Stencil<'a>>) -> Vec<&'a dyn TexelFactor> {
    let mask = factor.iter().map(|f| f as &dyn TexelFactor);
    mask.chain(stencil.iter().map(|s| s as &dyn TexelFactor)).collect()
}

/// What a stroke of `tool` paints through.
fn through<'a>(factors: &'a [&'a dyn TexelFactor], images: &'a ScreenImages, tool: Tool) -> Through<'a> {
    Through { factors, projection: if tool == Tool::Projection { images.projection.as_ref() } else { None } }
}

impl Painting {
    /// This frame's brush input while the quick mask is edited: strokes paint the mask (PT-42).
    fn mask_frame(&mut self, input: Vec<StrokeInput>, f: PaintFrame, tool: StrokeTool, mut report: PaintReport) -> PaintReport {
        let Some((_, set)) = f.target.clone() else { return report };
        let mut painted = false;
        for ev in input {
            match ev {
                StrokeInput::Down(at) | StrokeInput::Line { to: at, .. } => {
                    self.end_mask(f.doc, f.picking, &mut report);
                    let Some(camera) = f.camera else { continue };
                    let Some(surface) = self.surfaces.get(f.doc, set) else {
                        report.message = Some("The brush is getting this texture set ready; paint again in a moment.".into());
                        continue;
                    };
                    self.strokes = self.strokes.wrapping_add(1);
                    let start = MaskStrokeStart {
                        set,
                        preset: self.brush.preset(f.tool).cloned().unwrap_or_default(),
                        tool,
                        occlusion: self.brush.occlusion,
                        seed: self.strokes,
                        camera,
                        surface,
                        symmetry: to_record(self.brush.helpers.symmetry.spec()),
                    };
                    match ActiveMaskStroke::begin(&mut self.quick_mask, f.doc, f.picking, start, at) {
                        Ok(a) => {
                            self.mask_active = Some(a);
                            painted = true;
                        }
                        Err(e) => report.message = Some(format!("Nothing was painted into the quick mask: {e}.")),
                    }
                }
                StrokeInput::Move(at) => {
                    let Some(a) = &mut self.mask_active else { continue };
                    match a.add(&mut self.quick_mask, f.doc, f.picking, &[at]) {
                        Ok(changed) => painted |= changed,
                        Err(e) => {
                            report.message = Some(format!("The stroke stopped: {e}."));
                            self.end_mask(f.doc, f.picking, &mut report);
                        }
                    }
                }
                StrokeInput::Up => self.end_mask(f.doc, f.picking, &mut report),
            }
        }
        if painted && self.last_refresh.is_none_or(|t| t.elapsed() >= REFRESH) {
            report.refresh = Some(set);
            report.quick_mask = true;
            self.last_refresh = Some(Instant::now());
        }
        report.busy = self.mask_active.is_some() || self.surfaces.is_building();
        report
    }

    fn end_mask(&mut self, doc: &OpenDocument, picking: &ViewPicking, report: &mut PaintReport) {
        let Some(a) = self.mask_active.take() else { return };
        let set = a.set();
        if let Some(problem) = a.end(&mut self.quick_mask, doc, picking) {
            report.message = Some(format!("The quick-mask stroke had a problem: {problem}."));
        }
        report.refresh = Some(set);
        report.quick_mask = true;
        self.last_refresh = Some(Instant::now());
    }
}
