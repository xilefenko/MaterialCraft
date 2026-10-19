//! [`StrokeWorker`]: a document stroke stamped on its own thread, so the thread that takes the input (the editor's
//! UI thread) only hands points over and picks up what was painted, and keeps its frame rate at any spacing and
//! with any burst of input. The stroke may trail the pointer; it never holds the input up.
//!
//! Who owns what while a stroke is in flight:
//! - The worker owns the [`DocumentStroke`] (engine, stamp state, the old tiles for the history) and a working copy
//!   of the texture set it paints, cloned at pen down (images share their tiles, so the copy is cheap).
//! - The document holds the target images as last published. After each batch the worker publishes copies of them
//!   (sharing tiles again) and the tiles they changed; [`StrokeWorker::poll`] puts them into the document, so
//!   everything that reads the document (views, thumbnails, the maps worker through the screen's `Painted`
//!   reports) works as with a stroke stamped in place.
//! - Pen up ([`StrokeWorker::finish`]) paints whatever is still queued and then the stroke's last dabs; once a poll
//!   reports it [`Done`], the final images are in the document and [`Ended::record`] makes the history step, as
//!   for a stroke stamped in place: nothing is dropped.
//! - Nothing else may change the painted layer meanwhile; whoever wants to edit the document first ends the stroke
//!   and waits for it ([`StrokeWorker::wait`]).
//!
//! Batching: the worker takes everything queued whenever it gets to it, so a frame's pointer moves (or several
//! frames' while it trails) reach the engine at once. The engine places the same dabs however its input is split
//! up, and the stamp stage gives the same bits however its dabs are batched (`StampStroke::apply_all`), so the
//! result does not depend on timing.

use crate::BrushError;
use crate::document::{DocumentStroke, Ended};
use crate::engine::InputPoint;
use crate::session::SessionScene;
use crate::target::{PaintTarget, Resolved};
use materialcraft::paint::{TextureSet, TiledImage};
use materialcraft::stamp::StampMode;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// What a worker's stroke paints through, owned so it can live on the worker's thread (the surface, the view
/// scene, the factors), lent to the session per batch.
pub trait WorkerScene: Send + 'static {
    /// Calls `f` with the scene.
    fn with<R>(&self, f: impl FnOnce(&SessionScene) -> R) -> R;
}

/// Where a stroke's target images sit in a texture set.
#[derive(Clone)]
pub(crate) struct TargetSlots {
    pub target: PaintTarget,
    pub resolved: Resolved,
    pub mode: StampMode,
}

impl TargetSlots {
    /// Copies of the target images in `set` (sharing their tiles).
    fn read(&self, set: &mut TextureSet) -> Result<Vec<TiledImage>, BrushError> {
        Ok(self.target.projected_targets(set, &self.resolved, self.mode, None)?.into_iter().map(|t| t.image.clone()).collect())
    }

    /// Puts `images` (from [`TargetSlots::read`] of another copy of the set) into `set`.
    fn write(&self, set: &mut TextureSet, images: Vec<TiledImage>) -> Result<(), BrushError> {
        let targets = self.target.projected_targets(set, &self.resolved, self.mode, None)?;
        if targets.len() != images.len() {
            return Err(BrushError::NotPaintable("the layer's images changed"));
        }
        for (t, image) in targets.into_iter().zip(images) {
            *t.image = image;
        }
        Ok(())
    }
}

enum Command {
    Points(Vec<InputPoint>),
    Finish,
}

/// A finished stroke, ready for [`Ended::record`] (its images are in the set [`StrokeWorker::poll`] was given).
pub struct Done {
    pub ended: Ended,
    /// Every image tile the stroke changed, or why its last dabs could not be painted.
    pub result: Result<Vec<(u32, u32)>, BrushError>,
}

/// What a poll found.
#[derive(Default)]
pub struct Progress {
    /// Tiles changed since the last poll (sorted).
    pub changed: Vec<(u32, u32)>,
    /// Why painting stopped (the stroke then paints nothing more; end it).
    pub error: Option<BrushError>,
    /// The stroke is complete.
    pub done: Option<Done>,
    /// The worker stopped without completing the stroke (it failed): what it painted is in the set, but there is
    /// nothing to record.
    pub lost: bool,
}

/// What the worker publishes after each batch.
#[derive(Default)]
struct Shared {
    images: Option<Vec<TiledImage>>,
    changed: Vec<(u32, u32)>,
    error: Option<BrushError>,
    done: Option<Done>,
    /// Input points painted so far.
    samples: usize,
    dabs: usize,
    /// Time spent painting.
    busy: Duration,
}

type Mailbox = Arc<(Mutex<Shared>, Condvar)>;

fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    // a worker that panicked leaves its last state; reading it is still sound
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A stroke being stamped on its own thread. See the module docs.
pub struct StrokeWorker {
    commands: Option<Sender<Command>>,
    thread: JoinHandle<()>,
    mailbox: Mailbox,
    slots: TargetSlots,
    sent: usize,
    finishing: bool,
    /// A poll handed out [`Done`].
    completed: bool,
}

impl StrokeWorker {
    /// Starts painting `stroke` on a new thread into `set`, a copy of the texture set it paints (taken after the
    /// stroke was prepared and started), through `scene`.
    pub fn spawn<S: WorkerScene>(stroke: DocumentStroke, set: TextureSet, scene: S) -> Result<StrokeWorker, BrushError> {
        let slots = stroke.slots();
        let (tx, rx) = mpsc::channel();
        let mailbox: Mailbox = Arc::default();
        let (theirs, their_slots) = (mailbox.clone(), slots.clone());
        let thread = std::thread::Builder::new()
            .name("stroke".into())
            .spawn(move || run(stroke, set, scene, their_slots, rx, theirs))
            .map_err(|_| BrushError::NotPaintable("no thread could be started for the stroke"))?;
        Ok(StrokeWorker { commands: Some(tx), thread, mailbox, slots, sent: 0, finishing: false, completed: false })
    }

    /// Queues `points` (as for [`DocumentStroke::add`]). Ignored after [`StrokeWorker::finish`].
    pub fn add(&mut self, points: Vec<InputPoint>) {
        if self.finishing || points.is_empty() {
            return;
        }
        self.sent += points.len();
        self.send(Command::Points(points));
    }

    /// Pen up: paints what is queued and the stroke's last dabs; a later poll reports it done.
    pub fn finish(&mut self) {
        if !self.finishing {
            self.finishing = true;
            self.send(Command::Finish);
        }
    }

    fn send(&mut self, c: Command) {
        if self.commands.as_ref().is_some_and(|tx| tx.send(c).is_err()) {
            // the worker is gone; the next poll says so
            self.commands = None;
        }
    }

    /// Puts what was painted since the last poll into `set` (the document's copy of the set) and reports it.
    pub fn poll(&mut self, set: &mut TextureSet) -> Progress {
        // read before the mailbox: a worker that stopped has published everything it will
        let stopped = self.thread.is_finished();
        let (images, mut progress) = {
            let mut s = lock(&self.mailbox.0);
            let images = s.images.take();
            let done = s.done.take();
            let lost = stopped && done.is_none() && !self.completed;
            self.completed |= done.is_some();
            (images, Progress { changed: std::mem::take(&mut s.changed), error: s.error.take(), done, lost })
        };
        if progress.lost {
            progress.error.get_or_insert(BrushError::NotPaintable("the stroke stopped unexpectedly"));
        }
        if let Some(images) = images
            && let Err(e) = self.slots.write(set, images)
        {
            progress.error.get_or_insert(e);
        }
        progress
    }

    /// Ends the stroke ([`StrokeWorker::finish`]) and waits until it is complete; then polls.
    pub fn wait(&mut self, set: &mut TextureSet) -> Progress {
        self.finish();
        self.wait_painted(None);
        self.poll(set)
    }

    /// Waits until every point handed over is painted (after pen up: until the stroke is done), or painting
    /// stopped, or `limit` passed (`None`: no limit); a later poll picks the paint up. True if it got there.
    pub fn wait_painted(&self, limit: Option<Duration>) -> bool {
        let until = limit.map(|l| Instant::now() + l);
        let (m, ready) = &*self.mailbox;
        let mut s = lock(m);
        loop {
            let there = if self.finishing { s.done.is_some() } else { s.samples >= self.sent || s.done.is_some() };
            if there || self.thread.is_finished() {
                return there;
            }
            // a worker that stops without completing (it failed) notifies nobody: look again now and then
            let mut wait = Duration::from_millis(20);
            if let Some(u) = until {
                let left = u.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return false;
                }
                wait = wait.min(left);
            }
            s = ready.wait_timeout(s, wait).map_or_else(|e| e.into_inner().0, |r| r.0);
        }
    }

    /// Input points handed over so far.
    pub fn samples_sent(&self) -> usize {
        self.sent
    }

    /// Input points painted so far.
    pub fn samples_painted(&self) -> usize {
        lock(&self.mailbox.0).samples
    }

    /// Dabs placed so far.
    pub fn dabs(&self) -> usize {
        lock(&self.mailbox.0).dabs
    }

    /// Time the worker spent painting so far.
    pub fn busy(&self) -> Duration {
        lock(&self.mailbox.0).busy
    }
}

/// The worker's thread: paints batches until pen up, publishing after each.
fn run<S: WorkerScene>(mut stroke: DocumentStroke, mut set: TextureSet, scene: S, slots: TargetSlots, rx: Receiver<Command>, mailbox: Mailbox) {
    let mut failed = false;
    loop {
        // everything queued so far, as one batch
        let Ok(first) = rx.recv() else { return };
        let mut points = Vec::new();
        let mut finish = false;
        for c in std::iter::once(first).chain(rx.try_iter()) {
            match c {
                Command::Points(p) => points.extend(p),
                Command::Finish => finish = true,
            }
        }
        let started = Instant::now();
        let n = points.len();
        let mut error = None;
        let mut changed = Vec::new();
        if !failed && !points.is_empty() {
            match scene.with(|sc| stroke.add(sc, &points, &mut set)) {
                Ok(c) => changed = c,
                Err(e) => {
                    failed = true;
                    error = Some(e);
                }
            }
        }
        let images = if changed.is_empty() { None } else { slots.read(&mut set).map_err(|e| error.get_or_insert(e).clone()).ok() };
        let dabs = stroke.dabs();
        publish(&mailbox, |s| {
            if images.is_some() {
                s.images = images;
            }
            s.changed.extend(changed);
            s.changed.sort_unstable();
            s.changed.dedup();
            s.samples += n;
            s.dabs = dabs;
            s.busy += started.elapsed();
            if error.is_some() {
                s.error = error;
            }
        });
        if finish {
            break;
        }
    }
    let started = Instant::now();
    let (ended, result) = scene.with(|sc| stroke.finish(sc, &mut set));
    let images = slots.read(&mut set);
    publish(&mailbox, |s| {
        match images {
            Ok(images) => s.images = Some(images),
            Err(e) => s.error = Some(e),
        }
        if let Ok(t) = &result {
            s.changed.extend(t.iter().copied());
            s.changed.sort_unstable();
            s.changed.dedup();
        }
        s.busy += started.elapsed();
        s.done = Some(Done { ended, result });
    });
}

fn publish(mailbox: &Mailbox, f: impl FnOnce(&mut Shared)) {
    let (m, ready) = &**mailbox;
    f(&mut lock(m));
    ready.notify_all();
}
