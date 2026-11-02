//! [`Verification`]: the results of `mcsub verify-layers` as text: one block per set (strategy, then each
//! channel with its status and score), then totals, the worst channels, the blend fits and the projection fits.

use super::fit::FitTally;
use super::kinds::KindTally;
use super::{ChannelResult, SetResult};
use crate::translate::layers::cache_score::EXACT_STEPS;
use crate::translate::layers::replay::REPLAYED;
use crate::translate::layers::{Computability, StackStrategy};
use std::fmt;

/// Whether a status is of one kind.
type StatusTest = fn(&Computability) -> bool;

/// How many of the worst compared channels the summary lists.
const WORST: usize = 20;

#[derive(Default)]
pub struct Verification {
    pub sets: Vec<SetResult>,
    pub fits: FitTally,
    /// Compared and reproduced channels per native effect kind.
    pub kinds: KindTally,
    pub projection_fits: super::ProjectionFitTally,
    /// Projects that could not be read, with the reason.
    pub failures: Vec<(String, String)>,
}

fn strategy_name(s: StackStrategy) -> &'static str {
    match s {
        StackStrategy::RealStack => "real stack",
        StackStrategy::Replacements => "real stack with replacements",
        StackStrategy::Corrected => "Painter correction on top",
        StackStrategy::CacheDisagrees => "Painter result on top (cache disagrees)",
        StackStrategy::BestEffort => "best effort",
    }
}

fn status_name(s: &Computability) -> &'static str {
    match s {
        Computability::Exact => "exact",
        Computability::Replaced(_) => "replaced",
        Computability::Approximated(_) => "approx",
        Computability::NotComputable(_) => "not computable",
    }
}

fn channel_line(c: &ChannelResult) -> String {
    let score = c
        .score
        .map_or("–".to_string(), |s| format!("mean {:.2} max {:.1}", s.mean, s.max));
    let reasons = c.status.reasons().join("; ");
    format!("{:<11} {:<15} {score:<22} {reasons}", c.label, status_name(&c.status))
}

impl Verification {
    fn compared(&self) -> impl Iterator<Item = (&SetResult, &ChannelResult)> {
        self.sets
            .iter()
            .flat_map(|s| s.channels.iter().map(move |c| (s, c)))
            .filter(|(_, c)| c.score.is_some())
    }

    fn write_totals(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = |s: StackStrategy| self.sets.iter().filter(|r| r.strategy == s).count();
        writeln!(
            f,
            "\n{} texture sets with a texture cache: {} real stack, {} real stack with replacements, {} Painter result on top, {} of them because the cache disagrees",
            self.sets.len(),
            count(StackStrategy::RealStack),
            count(StackStrategy::Replacements),
            count(StackStrategy::Corrected) + count(StackStrategy::CacheDisagrees),
            count(StackStrategy::CacheDisagrees)
        )?;
        let all: usize = self.sets.iter().map(|s| s.channels.len()).sum();
        let kinds: [(&str, StatusTest); 3] = [
            ("exact", |s| matches!(s, Computability::Exact)),
            ("replaced", |s| matches!(s, Computability::Replaced(_))),
            ("approximated", |s| matches!(s, Computability::Approximated(_))),
        ];
        for (name, is) in kinds {
            let of: Vec<_> = self.compared().filter(|(_, c)| is(&c.status)).collect();
            let ok = of
                .iter()
                .filter(|(_, c)| c.score.is_some_and(|s| s.reproduced()))
                .count();
            writeln!(
                f,
                "{name} channels compared: {}, reproduced (mean ≤ {EXACT_STEPS} steps): {ok}",
                of.len()
            )?;
        }
        // channels the replayed strokes made computable (alone or with replacements)
        let replays = |c: &ChannelResult| c.status.reasons().iter().any(|r| r.starts_with(REPLAYED));
        let with: Vec<_> = self
            .sets
            .iter()
            .flat_map(|s| &s.channels)
            .filter(|c| c.status.is_computable() && replays(c))
            .collect();
        let compared = with.iter().filter(|c| c.score.is_some()).count();
        let ok = with.iter().filter(|c| c.score.is_some_and(|s| s.reproduced())).count();
        let only = with
            .iter()
            .filter(|c| c.status.reasons().iter().all(|r| r.starts_with(REPLAYED)))
            .count();
        writeln!(
            f,
            "channels with replayed strokes: {} computable ({only} apart from the replay alone), {compared} compared, {ok} reproduced",
            with.len()
        )?;
        let computable = self
            .sets
            .iter()
            .filter(|s| s.channels.iter().all(|c| c.score.is_some()))
            .count();
        let reproduced = self
            .sets
            .iter()
            .filter(|s| !s.channels.is_empty() && s.channels.iter().all(|c| c.score.is_some_and(|x| x.reproduced())))
            .count();
        writeln!(
            f,
            "channels in total: {all}; sets with every channel compared: {computable}, all reproduced: {reproduced}"
        )?;
        let mut worst: Vec<_> = self.compared().collect();
        let mean = |c: &ChannelResult| c.score.map_or(0.0, |s| s.mean);
        worst.sort_by(|a, b| mean(b.1).total_cmp(&mean(a.1)));
        writeln!(f, "worst compared channels:")?;
        for (set, c) in worst.iter().take(WORST) {
            writeln!(f, "  {} / {}: {}", set.project, set.set, channel_line(c))?;
        }
        Ok(())
    }
}

impl fmt::Display for Verification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for set in &self.sets {
            writeln!(f, "{} / {}: {}", set.project, set.set, strategy_name(set.strategy))?;
            for c in &set.channels {
                writeln!(f, "  {}", channel_line(c))?;
            }
        }
        self.write_totals(f)?;
        write!(f, "{}", self.kinds)?;
        write!(f, "{}", self.fits)?;
        write!(f, "{}", self.projection_fits)?;
        for (project, e) in &self.failures {
            writeln!(f, "FAILED {project}: {e}")?;
        }
        Ok(())
    }
}
