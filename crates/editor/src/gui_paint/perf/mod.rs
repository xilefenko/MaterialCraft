//! The headless runner behind `mcperf` (roadmap steps 51 and 52): plays a benchmark scenario
//! (`materialcraft_perf::Scenario`) through the Paint screen's real paths: the document and its history, the brush
//! tools, [`SceneSync`] with its maps worker, the GPU scene's upload queue, without a window, waiting for every
//! recomposite so the numbers are repeatable. [`runner`] plays a scenario ([`source`] opens, [`stroke_play`] and
//! [`edit_play`] act, [`measure`] records); `mcperf` starts it through the editor binary's hidden `--perf-run`
//! option and keeps the history (see `MaterialCraft-docs/perf/README.md`).
//!
//! Two ignored local tools live here too: `perf_identity` (incremental results against full recomputes on real
//! documents, bit for bit) and `perf_coverage` (which sets of a corpus list take the tile path). They read corpus
//! files; run them one at a time, under the heavy lock, at low priority:
//!
//! ```text
//! MC_PERF_DOCS=<a.mcpaint:b.mcpaint> flock ../Corpus/.heavy.lock nice -n 10 \
//!     cargo test --release -p materialcraft-editor perf_identity -- --ignored --nocapture --test-threads 1
//! ```

#[cfg(test)]
mod coverage;
mod edit_play;
#[cfg(test)]
mod identity;
mod measure;
pub mod runner;
mod source;
mod stroke_paced;
mod stroke_play;

use super::channel_view::ChannelView;
use super::document_state::OpenDocument;
use super::scene_stats::SceneStats;
use super::scene_sync::SceneSync;
use materialcraft::diag::NullSink;
use materialcraft::gpu3d::{self, Shared};
use materialcraft::paint::Document;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// An open document with its scene, as the Paint screen holds them.
pub struct Bench {
    pub doc: OpenDocument,
    pub scene: SceneSync,
    pub gpu: Shared,
    pub view: ChannelView,
}

impl Bench {
    /// Opens `doc` and waits until every set's maps are on the GPU queue; returns the bench and that time.
    pub fn open(doc: Document, source: PathBuf) -> (Bench, Duration) {
        let mut b = Bench { doc: OpenDocument::new(doc, source, None), scene: SceneSync::default(), gpu: gpu3d::new_shared(), view: ChannelView::Material };
        let started = Instant::now();
        b.scene.rebuild(&mut b.doc, b.view);
        b.settle();
        (b, started.elapsed())
    }

    /// One frame's poll of the scene (the screen polls once per frame); returns its time.
    pub fn poll(&mut self) -> Duration {
        let started = Instant::now();
        self.scene.poll(&mut self.doc, &self.gpu, &NullSink);
        started.elapsed()
    }

    /// Polls until the scene has no work left; returns the time it took.
    pub fn settle(&mut self) -> Duration {
        let started = Instant::now();
        loop {
            self.poll();
            if !self.scene.is_busy() {
                return started.elapsed();
            }
            std::thread::sleep(Duration::from_micros(200));
        }
    }
}

/// What the scene did between two readings of its stats.
pub fn delta(a: SceneStats, b: SceneStats) -> SceneStats {
    SceneStats {
        sets: b.sets - a.sets,
        patches: b.patches - a.patches,
        composite: b.composite - a.composite,
        prepare: b.prepare - a.prepare,
        upload_bytes: b.upload_bytes - a.upload_bytes,
        handoff: b.handoff - a.handoff,
    }
}
