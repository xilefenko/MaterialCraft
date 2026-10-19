//! Playing a [`StrokeSpec`]: brush strokes across the view through the brush tools' real frame path. Every frame
//! runs what the screen runs: the brush tools' frame (stamping), the recomposite it asks for
//! (`PaintMode::after_painting`: `SceneSync::painted`), the scene's poll (handing maps to the GPU queue). Each
//! recomposite is waited for, so its latency is measured on its own (step 51's stroke benchmark, generalised).

use super::Bench;
use super::measure::{Recomposite, put_all, put_group};
use super::stroke_paced::{PacedStroke, PacedTally};
use crate::gui_paint::input::{PointerSample, StrokeInput};
use crate::gui_paint::panels::layers::LayerEvent;
use crate::gui_paint::shell::tools::Tool;
use crate::gui_paint::stack_editor;
use crate::gui_paint::tools::brush_state::SmudgeMode;
use crate::gui_paint::tools::helpers::SymmetryMode;
use crate::gui_paint::tools::{PaintFrame, Painting};
use crate::gui_paint::view_pick::ViewPicking;
use crate::gui_paint::view_state::ViewState;
use materialcraft::camera::Camera;
use materialcraft::paint::brush::BrushPreset;
use materialcraft::paint::edit::LayerRef;
use materialcraft::paint::edit::ops::{MaskStart, NewLayer};
use materialcraft::paint::{Channel, LayerKind};
use materialcraft::projection::view::ViewProjector;
use materialcraft_brush::{PaintTarget, TargetPart};
use materialcraft_perf::report::{Stat, ms};
use materialcraft_perf::{LayerChoice, PathShape, RunReport, StrokeSpec, StrokeTarget, StrokeTool, Symmetry};
use std::time::{Duration, Instant};

/// The view the strokes are drawn in.
pub const VIEWPORT: [f32; 2] = [1600.0, 900.0];
/// How long to wait for the picking scene and the brush surface.
const READY_TIMEOUT: Duration = Duration::from_secs(600);

/// What the measured strokes did, summed over the repetitions.
#[derive(Default)]
struct Tally {
    stamp: Vec<Duration>,
    ask: Vec<Duration>,
    poll: Vec<Duration>,
    patches: Vec<Recomposite>,
    wholes: Vec<Recomposite>,
    tiles: usize,
    dabs: usize,
    /// Time the stroke's worker spent stamping.
    worker: Duration,
    total: Duration,
}

/// Paints `repeat` strokes as `spec` says; `new_project` (the source was a mesh) picks set 0 by default.
pub fn play(b: &mut Bench, spec: &StrokeSpec, repeat: u32, new_project: bool, r: &mut RunReport) -> Result<(), String> {
    let camera = {
        let state = ViewState::default();
        state.view.camera(&state.display.camera, VIEWPORT)
    };
    let mut picking = ViewPicking::default();
    wait("the picking scene", || {
        picking.poll(Some(&b.doc));
        picking.is_ready()
    })?;
    let set = choose_set(b, &picking, camera, spec, new_project)?;
    let layer = choose_layer(b, set, spec.layer, r)?;
    if spec.target == StrokeTarget::Mask {
        ensure_mask(b, set, &layer, r)?;
    }
    let set_id = b.doc.doc().texture_sets.get(set).ok_or("no such set")?.id.clone();
    let part = if spec.target == StrokeTarget::Mask { TargetPart::Mask } else { TargetPart::Content };
    let target = PaintTarget { layer: LayerRef::new(set_id, layer), part };

    let mut painting = Painting::default();
    // frame by frame, each frame waits for its stamps (as when stamping ran in the frame); in real time it doesn't
    painting.wait_for_stamps = spec.frame_ms.is_none();
    let tool = configure(&mut painting, spec)?;
    wait("the brush surface", || {
        picking.poll(Some(&b.doc));
        painting.surface_ready(&b.doc, set)
    })?;
    if let Some(s) = b.doc.doc().texture_sets.get(set) {
        r.note(format!("set {:?} at {}x{}, layer {:?}, tool {}", s.name, s.resolution.0, s.resolution.1, target.layer.layer, spec.tool.name()));
    }

    let frames = frames(spec);
    if spec.primed {
        let mut ignored = Tally::default();
        stroke(b, &mut painting, &picking, camera, Tool::Paint, &target, set, &frames, &mut ignored);
    }
    if let Some(frame_ms) = spec.frame_ms {
        let mut t = PacedTally::default();
        let mut paced = PacedStroke { painting: &mut painting, picking: &picking, camera, tool, target: &target, set };
        for _ in 0..repeat {
            paced.play(b, spec, frame_ms, |t| point(spec.path, t), &mut t)?;
            b.settle();
        }
        t.report(r);
        r.put("history_steps", b.doc.history().len() as f64);
        return Ok(());
    }
    let mut t = Tally::default();
    for _ in 0..repeat {
        stroke(b, &mut painting, &picking, camera, tool, &target, set, &frames, &mut t);
    }

    r.put("frames", t.stamp.len() as f64);
    r.put("dabs", t.dabs as f64);
    r.put_ms("stroke_total_ms", t.total);
    r.put_stat("stamp_frame_ms", Stat::of_durations(&t.stamp));
    r.put("stamp_dab_ms", ms(t.stamp.iter().sum::<Duration>()) / t.dabs.max(1) as f64);
    r.put("worker_dab_ms", ms(t.worker) / t.dabs.max(1) as f64);
    r.put_stat("ask_ms", Stat::of_durations(&t.ask));
    r.put_stat("poll_ms", Stat::of_durations(&t.poll));
    let all: Vec<Recomposite> = t.patches.iter().chain(&t.wholes).copied().collect();
    put_all(r, &all);
    r.put("patches", t.patches.len() as f64);
    r.put("wholes", t.wholes.len() as f64);
    r.put("tiles", t.tiles as f64);
    put_group(r, "patch", &t.patches);
    put_group(r, "whole", &t.wholes);
    r.put("history_steps", b.doc.history().len() as f64);
    if spec.undo_redo {
        let undo = Recomposite::history(b, |d| d.undo().map(|_| ())).map_err(|e| format!("undo: {e}"))?;
        let redo = Recomposite::history(b, |d| d.redo().map(|_| ())).map_err(|e| format!("redo: {e}"))?;
        put_group(r, "undo", &[undo]);
        put_group(r, "redo", &[redo]);
    }
    if t.dabs == 0 {
        return Err("the stroke painted nothing".into());
    }
    Ok(())
}

/// Polls `ready` until it is true (or a long timeout).
fn wait(what: &str, mut ready: impl FnMut() -> bool) -> Result<(), String> {
    let started = Instant::now();
    while !ready() {
        if started.elapsed() > READY_TIMEOUT {
            return Err(format!("{what} wasn't ready after {} s", READY_TIMEOUT.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

/// The named set; else set 0 of a new project; else the set under the middle of the path.
fn choose_set(b: &Bench, picking: &ViewPicking, camera: Camera, spec: &StrokeSpec, new_project: bool) -> Result<usize, String> {
    let sets = &b.doc.doc().texture_sets;
    if let Some(name) = &spec.set {
        return sets.iter().position(|s| &s.name == name || &s.id == name).ok_or_else(|| format!("no texture set {name:?}"));
    }
    if new_project {
        return Ok(0);
    }
    let doc = b.doc.doc();
    let mid = point(spec.path, 0.5);
    let hit = picking.projector(doc, camera).and_then(|p| p.pick(mid)).ok_or("the stroke's path misses the mesh")?;
    let slot = doc.mesh.parts.get(hit.part).map(|p| p.material).ok_or("picked an unknown part")?;
    let id = doc.texture_set_for_slot(slot).map(|s| s.id.clone()).ok_or("no texture set on the picked part")?;
    sets.iter().position(|s| s.id == id).ok_or_else(|| "no texture set on the picked part".to_string())
}

/// The layer painted: the topmost paint layer of the set, or a new one (made through the layer panel's path).
fn choose_layer(b: &mut Bench, set: usize, choice: LayerChoice, r: &mut RunReport) -> Result<String, String> {
    let s = b.doc.doc().texture_sets.get(set).ok_or("no such set")?;
    if choice == LayerChoice::TopPaint
        && let Some(l) = s.layers.iter().rev().find(|l| matches!(l.kind, LayerKind::Paint(_)))
    {
        return Ok(l.id.clone());
    }
    let ev = LayerEvent::Create { set: s.id.clone(), what: NewLayer::Paint, above: None };
    let applied = stack_editor::apply(&mut b.doc, ev).map_err(|e| format!("new layer: {e}"))?;
    let _ = stack_editor::apply(&mut b.doc, LayerEvent::EndGesture);
    let id = applied.select.and_then(|ids| ids.into_iter().next()).ok_or("the new layer has no id")?;
    let made = Recomposite::refresh(b, applied.set.into_iter().chain(applied.also).collect());
    r.put_ms("new_layer_ms", made.latency);
    Ok(id)
}

/// Gives the layer a white mask if it has none (through the layer panel's path).
fn ensure_mask(b: &mut Bench, set: usize, layer: &str, r: &mut RunReport) -> Result<(), String> {
    let s = b.doc.doc().texture_sets.get(set).ok_or("no such set")?;
    if s.layer(layer).is_some_and(|l| l.mask.is_some()) {
        return Ok(());
    }
    let ev = LayerEvent::AddMask(LayerRef::new(s.id.clone(), layer.to_string()), MaskStart::White);
    let applied = stack_editor::apply(&mut b.doc, ev).map_err(|e| format!("add mask: {e}"))?;
    let _ = stack_editor::apply(&mut b.doc, LayerEvent::EndGesture);
    let made = Recomposite::refresh(b, applied.set.into_iter().chain(applied.also).collect());
    r.put_ms("add_mask_ms", made.latency);
    Ok(())
}

/// Sets the brush up as `spec` says; returns the screen's tool.
fn configure(painting: &mut Painting, spec: &StrokeSpec) -> Result<Tool, String> {
    let brush = &mut painting.brush;
    brush.value_mut(Channel::BaseColor).value = [0.85, 0.12, 0.08, 1.0];
    // a white mask shows the stroke only when it paints black
    brush.mask_value = 0.0;
    let tool = match spec.tool {
        StrokeTool::Paint => Tool::Paint,
        StrokeTool::Erase => Tool::Eraser,
        StrokeTool::Smudge | StrokeTool::Blur => {
            brush.retouch.smudge_mode = if spec.tool == StrokeTool::Blur { SmudgeMode::Blur } else { SmudgeMode::Smudge };
            Tool::Smudge
        }
    };
    if let Some(path) = &spec.preset {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let preset = BrushPreset::from_file_text(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(p) = brush.preset_mut(tool) {
            *p = preset;
        }
    }
    if let (Some(size), Some(p)) = (spec.size, brush.preset_mut(tool)) {
        p.size = size;
    }
    if let (Some(spacing), Some(p)) = (spec.spacing, brush.preset_mut(tool)) {
        p.spacing = spacing;
    }
    let sym = &mut brush.helpers.symmetry;
    match spec.symmetry {
        Symmetry::Off => sym.on = false,
        Symmetry::Mirror { axis } => (sym.on, sym.mode, sym.axis) = (true, SymmetryMode::Mirror, axis),
        Symmetry::Radial { axis, count } => (sym.on, sym.mode, sym.axis, sym.count) = (true, SymmetryMode::Radial, axis, count),
    }
    Ok(tool)
}

/// Where the path is at `t` (0…1), in view pixels.
fn point(shape: PathShape, t: f32) -> [f32; 2] {
    let [w, h] = VIEWPORT;
    match shape {
        PathShape::Line => [w * (0.3 + 0.4 * t), h * 0.5],
        PathShape::Wave => [w * (0.3 + 0.4 * t), h * (0.5 + 0.1 * (t * 9.0).sin())],
        PathShape::Spiral => {
            let (radius, angle) = (h * (0.02 + 0.18 * t), t * 6.0 * std::f32::consts::PI);
            [w * 0.5 + radius * angle.cos(), h * 0.5 + radius * angle.sin()]
        }
    }
}

/// The stroke's input, frame by frame: pen down, `per_frame` moves per frame, pen up; two seconds in all.
fn frames(spec: &StrokeSpec) -> Vec<Vec<StrokeInput>> {
    let n = spec.samples.max(2);
    let sample = |i: usize| {
        let t = i as f32 / (n - 1) as f32;
        PointerSample::mouse(point(spec.path, t), f64::from(t) * 2.0)
    };
    let mut frames: Vec<Vec<StrokeInput>> = vec![vec![StrokeInput::Down(sample(0))]];
    for chunk in (1..n).collect::<Vec<_>>().chunks(spec.per_frame.max(1)) {
        frames.push(chunk.iter().map(|&i| StrokeInput::Move(sample(i))).collect());
    }
    frames.push(vec![StrokeInput::Up]);
    frames
}

/// One stroke, frame by frame, as the screen plays it.
#[allow(clippy::too_many_arguments)]
fn stroke(
    b: &mut Bench,
    painting: &mut Painting,
    picking: &ViewPicking,
    camera: Camera,
    tool: Tool,
    target: &PaintTarget,
    set: usize,
    frames: &[Vec<StrokeInput>],
    t: &mut Tally,
) {
    let mut dabs = 0;
    let mut worker = Duration::ZERO;
    let started = Instant::now();
    for input in frames.iter().cloned() {
        let at = Instant::now();
        let frame = PaintFrame { doc: &mut b.doc, picking, camera: Some(camera), tool, target: Some((target.clone(), set)), uv: None };
        let report = painting.frame(input, frame);
        t.stamp.push(at.elapsed());
        dabs = painting.stroke_dabs().unwrap_or(dabs);
        worker = painting.stroke_worker_busy().unwrap_or(worker);
        if let Some(s) = report.refresh {
            // what the screen does with the report (`PaintMode::after_painting`), then waiting for the result
            let before = b.scene.stats();
            let asked = Instant::now();
            let tiles = report.tiles.as_ref().map_or(0, |p| p.tiles.len());
            b.scene.painted(&mut b.doc, b.view, s, report.tiles);
            t.ask.push(asked.elapsed());
            b.settle();
            let done = Recomposite { latency: asked.elapsed(), did: super::delta(before, b.scene.stats()) };
            if done.did.patches > 0 {
                t.tiles += tiles;
                t.patches.push(done);
            } else {
                t.wholes.push(done);
            }
        }
        t.poll.push(b.poll());
    }
    t.total += started.elapsed();
    t.dabs += dabs;
    t.worker += worker;
}
