//! Small data-parallel helpers for the hot paths (stamping, compositing tiles, `crate::tex::par_map`), on scoped
//! std threads.
//!
//! - The calling thread works too, beside at most `cores − 1` helpers, and never more threads than there are runs
//!   of work: a call over a few tiles wakes a few threads, not one per core.
//! - Items are handed out in short runs to whichever thread is free, so uneven items (tiles with many dabs, tiles
//!   with none) balance.
//! - The core count is read once per process (reading it costs system calls).
//!
//! Results are deterministic: each item is visited exactly once with its index, and what a visit does may depend
//! only on its item and index, never on which thread runs it or when.
//!
//! The threads are spawned per call: a persistent pool would have to hand borrowed closures to long-lived threads,
//! which std offers only through `std::thread::scope` (or `unsafe`, which this package doesn't allow outside its
//! two documented modules). Callers therefore batch work into few calls (the stamp stage pulls a whole batch of
//! dabs in one call).
//!
//! Unlike `crate::tex::par_map` these never stop early for a stale editor job, so callers always get complete
//! results.

use std::sync::{Mutex, OnceLock, PoisonError};

/// Threads besides the caller that a call may use: the cores less one.
pub(crate) fn helpers() -> usize {
    static HELPERS: OnceLock<usize> = OnceLock::new();
    *HELPERS.get_or_init(|| std::thread::available_parallelism().map_or(1, |n| n.get()).saturating_sub(1))
}

/// Calls `f(index, item)` for every item, spread over the available cores.
pub(crate) fn for_each_mut<T: Send, F: Fn(usize, &mut T) + Sync>(items: &mut [T], f: F) {
    let run = items.len().div_ceil((helpers() + 1) * 4).max(1);
    for_each_run_mut(items, run, |first, chunk| {
        for (i, it) in chunk.iter_mut().enumerate() {
            f(first + i, it);
        }
    });
}

/// Calls `f(first_index, run)` for consecutive runs of at most `run` items covering `items`, spread over the
/// available cores.
pub(crate) fn for_each_run_mut<T: Send, F: Fn(usize, &mut [T]) + Sync>(items: &mut [T], run: usize, f: F) {
    let run = run.max(1);
    let runs = items.len().div_ceil(run);
    let helpers = helpers().min(runs.saturating_sub(1));
    if helpers == 0 {
        for (ri, chunk) in items.chunks_mut(run).enumerate() {
            f(ri * run, chunk);
        }
        return;
    }
    let queue = Mutex::new(items.chunks_mut(run).enumerate());
    let work = || {
        loop {
            // the lock is held only to take the next run
            let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
            let Some((ri, chunk)) = next else { return };
            f(ri * run, chunk);
        }
    };
    std::thread::scope(|s| {
        for _ in 0..helpers {
            s.spawn(work);
        }
        work();
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn visits_every_item_once_with_its_index() {
        for n in [0, 1, 2, 7, 1000] {
            let mut v = vec![0usize; n];
            super::for_each_mut(&mut v, |i, x| *x += i + 1);
            assert!(v.iter().enumerate().all(|(i, &x)| x == i + 1), "{n} items");
        }
    }

    #[test]
    fn runs_cover_the_items_in_order_and_respect_their_length() {
        for (n, run) in [(10, 3), (10, 10), (10, 64), (4097, 4096), (5, 0)] {
            let mut v = vec![usize::MAX; n];
            super::for_each_run_mut(&mut v, run, |first, chunk| {
                assert!(chunk.len() <= run.max(1));
                chunk.iter_mut().enumerate().for_each(|(i, x)| *x = first + i);
            });
            assert!(v.iter().enumerate().all(|(i, &x)| x == i), "{n} items in runs of {run}");
        }
    }
}
