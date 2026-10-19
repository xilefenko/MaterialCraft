//! Playing a stroke in real time ([`StrokeSpec::frame_ms`]): pointer samples arrive at a steady rate, `per_frame`
//! per frame period, as a pen or a fast mouse delivers them, and each frame takes every sample that arrived by its
//! start. A frame that runs long makes the next one bigger, as in the window (the snowball of the 2026-10-18
//! trace). Frames start on the next period boundary after the previous frame ended, like vsync.
//!
//! What is measured: the UI thread's time per frame (the brush tools' frame, asking for the recomposite, the
//! scene's poll), the dabs placed per second from pen down until the stroke is complete, how far the stamping
//! trails the pointer (from a sample's arrival to the end of the frame after which its dabs are in the document)
//! and how long pen up takes to complete the stroke. Recomposites are not waited for; they run alongside.

use super::Bench;
use crate::gui_paint::input::{PointerSample, StrokeInput};
use crate::gui_paint::shell::tools::Tool;
use crate::gui_paint::tools::{PaintFrame, Painting};
use crate::gui_paint::view_pick::ViewPicking;
use materialcraft::camera::Camera;
use materialcraft_brush::PaintTarget;
use materialcraft_perf::report::{Stat, ms};
use materialcraft_perf::{RunReport, StrokeSpec};
use std::time::{Duration, Instant};

/// How long a stroke may take to complete after pen up before the run fails.
const COMPLETE_TIMEOUT: Duration = Duration::from_secs(120);

/// What the paced strokes did, summed over the repetitions.
#[derive(Default)]
pub struct PacedTally {
    ui: Vec<Duration>,
    trail: Vec<Duration>,
    pen_up: Vec<Duration>,
    /// Samples per frame as delivered.
    delivered: Vec<f64>,
    dabs: usize,
    /// Time the strokes' workers spent stamping.
    worker: Duration,
    painting_time: Duration,
}

impl PacedTally {
    /// Puts the measurements into `r`.
    pub fn report(&self, r: &mut RunReport) {
        r.put("frames", self.ui.len() as f64);
        r.put("dabs", self.dabs as f64);
        r.put_stat("ui_frame_ms", Stat::of_durations(&self.ui));
        r.put("ui_frames_over_16ms", self.ui.iter().filter(|d| ms(**d) > 16.0).count() as f64);
        r.put_stat("samples_per_frame", Stat::of(&self.delivered));
        r.put("dabs_per_s", self.dabs as f64 / self.painting_time.as_secs_f64().max(1e-9));
        r.put("worker_dab_ms", ms(self.worker) / self.dabs.max(1) as f64);
        r.put_stat("trail_ms", Stat::of_durations(&self.trail));
        r.put_stat("pen_up_ms", Stat::of_durations(&self.pen_up));
    }
}

/// One input sample with its arrival time (from pen down).
struct Arrival {
    at: Duration,
    input: StrokeInput,
}

/// The stroke's samples and their arrival times: pen down at 0, `per_frame` samples per `frame` along the path,
/// pen up right after the last sample. The pressure swings like a pen's, so pressure-driven size varies.
fn arrivals(spec: &StrokeSpec, frame: Duration, point: impl Fn(f32) -> [f32; 2]) -> Vec<Arrival> {
    let n = spec.samples.max(2);
    let step = frame / spec.per_frame.max(1) as u32;
    let sample = |i: usize| {
        let t = i as f32 / (n - 1) as f32;
        let at = step * i as u32;
        let pressure = 0.8 + 0.2 * (t * 23.0).sin();
        (at, PointerSample { pressure, ..PointerSample::mouse(point(t), at.as_secs_f64()) })
    };
    let mut out = Vec::with_capacity(n + 1);
    let (at, first) = sample(0);
    out.push(Arrival { at, input: StrokeInput::Down(first) });
    for i in 1..n {
        let (at, s) = sample(i);
        out.push(Arrival { at, input: StrokeInput::Move(s) });
    }
    out.push(Arrival { at: step * n as u32, input: StrokeInput::Up });
    out
}

/// The screen's frame around the brush tools, as it plays during a paced stroke.
pub struct PacedStroke<'a> {
    pub painting: &'a mut Painting,
    pub picking: &'a ViewPicking,
    pub camera: Camera,
    pub tool: Tool,
    pub target: &'a PaintTarget,
    pub set: usize,
}

impl PacedStroke<'_> {
    /// Plays one stroke in real time; adds what it measured to `t`.
    pub fn play(&mut self, b: &mut Bench, spec: &StrokeSpec, frame_ms: f64, point: impl Fn(f32) -> [f32; 2], t: &mut PacedTally) -> Result<(), String> {
        let frame = Duration::from_secs_f64(frame_ms / 1000.0);
        let list = arrivals(spec, frame, point);
        let started = Instant::now();
        let mut next = 0;
        // samples whose dabs are in the document (counted from pen down; pen up isn't a sample)
        let mut credited = 0;
        let mut boundary = started;
        let mut up_at = None;
        let mut dabs = 0;
        let mut worker = Duration::ZERO;
        loop {
            // the next frame starts on the first period boundary not yet passed
            let now = Instant::now();
            while boundary < now {
                boundary += frame;
            }
            std::thread::sleep(boundary - now);
            let frame_start = Instant::now();
            let due = frame_start - started;
            let mut input = Vec::new();
            while let Some(a) = list.get(next).filter(|a| a.at <= due) {
                input.push(a.input);
                next += 1;
            }
            let samples = input.iter().filter(|i| !matches!(i, StrokeInput::Up)).count();
            if input.iter().any(|i| matches!(i, StrokeInput::Up)) {
                up_at = Some(Instant::now());
            }
            let f = PaintFrame {
                doc: &mut b.doc,
                picking: self.picking,
                camera: Some(self.camera),
                tool: self.tool,
                target: Some((self.target.clone(), self.set)),
                uv: None,
            };
            let report = self.painting.frame(input, f);
            dabs = self.painting.stroke_dabs().unwrap_or(dabs);
            worker = self.painting.stroke_worker_busy().unwrap_or(worker);
            if let Some(s) = report.refresh {
                b.scene.painted(&mut b.doc, b.view, s, report.tiles);
            }
            b.poll();
            let end = Instant::now();
            t.ui.push(end - frame_start);
            t.delivered.push(samples as f64);
            let stroking = self.painting.is_stroking();
            let stamped = if stroking { self.painting.samples_stamped() } else { next.min(list.len() - 1) };
            for a in list.iter().take(stamped.min(next)).skip(credited) {
                t.trail.push(end.duration_since(started + a.at));
            }
            credited = credited.max(stamped.min(next));
            if let (Some(up), false) = (up_at, stroking) {
                t.pen_up.push(end - up);
                t.painting_time += end - started;
                t.dabs += dabs;
                t.worker += worker;
                return Ok(());
            }
            if up_at.is_some_and(|u| u.elapsed() > COMPLETE_TIMEOUT) {
                return Err("the stroke didn't complete after pen up".into());
            }
        }
    }
}
