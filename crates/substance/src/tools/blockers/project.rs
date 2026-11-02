//! [`ProjectBlockers`]: what one imported project contributes to the census: its size, its locked layers by
//! blocker ([`BlockerSummary`]) and how much of each set the correction layer still covers.

use crate::translate::blocker_summary::BlockerSummary;
use crate::translate::layers::correction::{CORRECTION_ID, coverage_of};
use crate::translate::{DEFAULTS_LAYER_ID, PAINTER_RESULT_ID};
use materialcraft::paint::{Channel, Document};
use std::path::PathBuf;

/// The correction's coverage of one texture set.
#[derive(Debug, Clone, PartialEq)]
pub struct SetCoverage {
    pub set: String,
    /// Share (0..1) of the set's texels per channel where Painter's result is still needed.
    pub channels: Vec<(Channel, f32)>,
}

/// One imported project.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectBlockers {
    /// As shown (relative to the corpus folder).
    pub path: PathBuf,
    pub sets: usize,
    /// Painter's layers and folders, without the layers the import adds (defaults, correction, reference).
    pub layers: usize,
    pub summary: BlockerSummary,
    /// Per set that has a correction layer.
    pub coverage: Vec<SetCoverage>,
    /// Import time.
    pub seconds: f64,
}

impl ProjectBlockers {
    /// The record of `doc`, imported from `path` in `seconds`.
    pub fn of(path: PathBuf, doc: &Document, seconds: f64) -> ProjectBlockers {
        let added = |id: &str| id == DEFAULTS_LAYER_ID || id == PAINTER_RESULT_ID || id == CORRECTION_ID;
        let layers = doc
            .texture_sets
            .iter()
            .map(|s| s.walk_layers().filter(|(_, l)| !added(&l.id)).count())
            .sum();
        let coverage = doc
            .texture_sets
            .iter()
            .filter_map(|s| {
                let channels = s.layers.iter().find_map(coverage_of)?;
                Some(SetCoverage {
                    set: s.name.clone(),
                    channels,
                })
            })
            .collect();
        ProjectBlockers {
            path,
            sets: doc.texture_sets.len(),
            layers,
            summary: BlockerSummary::of(doc).with_texts(doc),
            coverage,
            seconds,
        }
    }

    /// Locked layers.
    pub fn locked(&self) -> usize {
        self.summary.locked.len()
    }

    /// Per channel (in the order first met) the mean share over the sets that have it.
    pub fn mean_coverage(&self) -> Vec<(Channel, f32)> {
        let mut sums: Vec<(Channel, f32, usize)> = Vec::new();
        for (channel, share) in self.coverage.iter().flat_map(|s| &s.channels) {
            match sums.iter_mut().find(|(c, _, _)| c == channel) {
                Some((_, sum, n)) => {
                    *sum += share;
                    *n += 1;
                }
                None => sums.push((*channel, *share, 1)),
            }
        }
        sums.into_iter().map(|(c, sum, n)| (c, sum / n as f32)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_is_averaged_per_channel_over_the_sets_that_have_it() {
        let p = ProjectBlockers {
            path: PathBuf::from("a.spp"),
            sets: 2,
            layers: 0,
            summary: BlockerSummary::default(),
            coverage: vec![
                SetCoverage {
                    set: "A".into(),
                    channels: vec![(Channel::BaseColor, 0.2), (Channel::Roughness, 0.5)],
                },
                SetCoverage {
                    set: "B".into(),
                    channels: vec![(Channel::BaseColor, 0.4)],
                },
            ],
            seconds: 0.0,
        };
        let mean = p.mean_coverage();
        assert_eq!(mean.len(), 2);
        assert_eq!(mean[0].0, Channel::BaseColor);
        assert!((mean[0].1 - 0.3).abs() < 1e-6);
        assert!((mean[1].1 - 0.5).abs() < 1e-6);
    }
}
