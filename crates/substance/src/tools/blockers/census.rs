//! [`Census`]: the projects' blocker summaries added up over a corpus and ranked by what supporting each
//! blocker would unlock. Pure aggregation: no files are read here.

use super::project::ProjectBlockers;
use crate::translate::blocker_summary::LockedLayer;
use crate::translate::blockers::{BlockerKey, BlockerKind};
use crate::translate::catalog::{Support, support};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// One blocker over the corpus.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyTotal {
    /// `kind:name`.
    pub key: String,
    /// `None` for a key that isn't a blocker key.
    pub kind: Option<BlockerKind>,
    pub support: Option<Support>,
    /// Projects it locks a layer in.
    pub projects: usize,
    /// Locked layers it is among the blockers of.
    pub layers: usize,
    /// Locked layers it is the only blocker of: supporting it unlocks them.
    pub alone: usize,
    /// Of `layers`, the visible ones in the imported document. The import hides layers whose mask it can't
    /// compute (the correction shows Painter's result there), so this is not Painter's visibility.
    pub visible: usize,
    /// The first text it was met with.
    pub example: String,
}

impl KeyTotal {
    /// The replacement that would stand in for it, if any.
    pub fn replacement(&self) -> Option<&'static str> {
        match self.support {
            Some(Support::Replaceable { by, .. }) => Some(by),
            _ => None,
        }
    }
}

/// What it takes to unlock a locked layer, judged from all its blockers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unlock {
    /// Every blocker has a replacement: choosing "replace" unlocks it.
    Replace,
    /// Every blocker is strokes: replaying unlocks it.
    Replay,
    /// Every blocker is replaceable or replayable, some of each: both choices unlock it.
    Both,
    /// At least one blocker isn't supported yet.
    NeedsSupport,
}

impl Unlock {
    pub fn of(layer: &LockedLayer) -> Unlock {
        let (mut replace, mut replay) = (false, false);
        for key in &layer.keys {
            match BlockerKey::parse(key).map(|k| support(&k)) {
                Some(Support::Replaceable { .. }) => replace = true,
                Some(Support::Replayable) => replay = true,
                Some(Support::Unsupported) | None => return Unlock::NeedsSupport,
            }
        }
        match (replace, replay) {
            (true, true) => Unlock::Both,
            (true, false) => Unlock::Replace,
            (false, true) => Unlock::Replay,
            // a locked layer always has a key; without one there is nothing to choose
            (false, false) => Unlock::NeedsSupport,
        }
    }
}

/// Sums over the corpus.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Totals {
    pub projects: usize,
    pub failed: usize,
    pub sets: usize,
    pub layers: usize,
    pub locked: usize,
    pub visible_locked: usize,
    /// Projects without a locked layer.
    pub unlocked_projects: usize,
    /// Locked layers by what unlocks them ([`Unlock`]).
    pub by_replace: usize,
    pub by_replay: usize,
    pub by_both: usize,
    pub needs_support: usize,
    /// Distinct blocker keys.
    pub keys: usize,
}

impl Totals {
    /// Locked layers a choice at import time unlocks today.
    pub fn by_choice(&self) -> usize {
        self.by_replace + self.by_replay + self.by_both
    }
}

/// A project that could not be imported.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub path: PathBuf,
    pub error: String,
}

/// Every project's result, in corpus order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Census {
    pub projects: Vec<ProjectBlockers>,
    pub failures: Vec<Failure>,
    /// Wall time of the whole run.
    pub seconds: f64,
}

impl Census {
    /// Every blocker over the corpus, most layers unlocked by supporting it first, then most layers locked,
    /// then most projects, then by key.
    pub fn ranked(&self) -> Vec<KeyTotal> {
        let mut by_key: BTreeMap<&str, KeyTotal> = BTreeMap::new();
        for p in &self.projects {
            for b in &p.summary.blockers {
                let t = by_key.entry(b.key.as_str()).or_insert_with(|| KeyTotal {
                    key: b.key.clone(),
                    kind: b.parsed().map(|k| k.kind),
                    support: b.support,
                    projects: 0,
                    layers: 0,
                    alone: 0,
                    visible: 0,
                    example: String::new(),
                });
                t.projects += 1;
                t.layers += b.layers.len();
                t.alone += b.only;
                t.visible += b
                    .layers
                    .iter()
                    .filter(|&&i| p.summary.locked.get(i).is_some_and(|l| l.visible))
                    .count();
                if t.example.is_empty() {
                    t.example.clone_from(&b.text);
                }
            }
        }
        let mut ranked: Vec<KeyTotal> = by_key.into_values().collect();
        ranked.sort_by(|a, b| {
            b.alone
                .cmp(&a.alone)
                .then(b.layers.cmp(&a.layers))
                .then(b.projects.cmp(&a.projects))
                .then(a.key.cmp(&b.key))
        });
        ranked
    }

    pub fn totals(&self) -> Totals {
        let mut t = Totals {
            projects: self.projects.len(),
            failed: self.failures.len(),
            ..Totals::default()
        };
        let mut keys: Vec<&str> = Vec::new();
        for p in &self.projects {
            t.sets += p.sets;
            t.layers += p.layers;
            t.locked += p.locked();
            t.unlocked_projects += usize::from(p.summary.is_empty());
            for l in &p.summary.locked {
                t.visible_locked += usize::from(l.visible);
                match Unlock::of(l) {
                    Unlock::Replace => t.by_replace += 1,
                    Unlock::Replay => t.by_replay += 1,
                    Unlock::Both => t.by_both += 1,
                    Unlock::NeedsSupport => t.needs_support += 1,
                }
            }
            keys.extend(p.summary.blockers.iter().map(|b| b.key.as_str()));
        }
        keys.sort_unstable();
        keys.dedup();
        t.keys = keys.len();
        t
    }

    /// Locked layers per blocker kind (layers with blockers of several kinds count once per kind), and how
    /// many of them a kind locks alone, in [`BlockerKind::ALL`] order; kinds that lock nothing are left out.
    pub fn by_kind(&self) -> Vec<(BlockerKind, usize, usize)> {
        let mut out: Vec<(BlockerKind, usize, usize)> = BlockerKind::ALL.iter().map(|k| (*k, 0, 0)).collect();
        for l in self.projects.iter().flat_map(|p| &p.summary.locked) {
            let kinds: Vec<BlockerKind> = l
                .keys
                .iter()
                .filter_map(|k| BlockerKey::parse(k))
                .map(|k| k.kind)
                .collect();
            for (kind, layers, alone) in &mut out {
                if kinds.contains(kind) {
                    *layers += 1;
                    *alone += usize::from(kinds.iter().all(|k| k == kind));
                }
            }
        }
        out.retain(|(_, layers, _)| *layers > 0);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::blocker_summary::BlockerSummary;
    use crate::translate::replacements;

    fn layer(id: &str, visible: bool, keys: &[&str]) -> LockedLayer {
        LockedLayer {
            set: "s".into(),
            set_name: "S".into(),
            layer: id.into(),
            path: id.into(),
            visible,
            keys: keys.iter().map(|k| (*k).to_string()).collect(),
        }
    }

    fn project(name: &str, locked: Vec<LockedLayer>) -> ProjectBlockers {
        let mut summary = BlockerSummary::from_locked(locked);
        for b in &mut summary.blockers {
            b.text = format!("{name}: {}", b.key);
        }
        ProjectBlockers {
            path: PathBuf::from(name),
            sets: 1,
            layers: 10,
            summary,
            coverage: Vec::new(),
            seconds: 0.0,
        }
    }

    /// A Substance graph the replacement table knows.
    fn replaceable() -> String {
        let graph = replacements::entries()
            .next()
            .and_then(|r| r.painter.first())
            .expect("the table has entries");
        format!("substance:{graph}")
    }

    fn census() -> Census {
        let rep = replaceable();
        Census {
            projects: vec![
                project(
                    "a.spp",
                    vec![
                        layer("1", true, &["substance:no_such_graph"]),
                        layer("2", false, &["substance:no_such_graph", "strokes:brush"]),
                        layer("3", true, &["strokes:brush"]),
                        layer("4", true, &[&rep, "strokes:brush"]),
                    ],
                ),
                project(
                    "b.spp",
                    vec![
                        layer("1", true, &["brush-alpha:chalk"]),
                        layer("2", true, &["brush-alpha:chalk"]),
                        layer("3", true, &[&rep]),
                        layer("4", true, &["substance:no_such_graph"]),
                    ],
                ),
                project("c.spp", Vec::new()),
            ],
            failures: vec![Failure {
                path: PathBuf::from("d.spp"),
                error: "broken".into(),
            }],
            seconds: 1.0,
        }
    }

    #[test]
    fn keys_add_up_over_projects_and_rank_by_what_they_unlock_alone() {
        let ranked = census().ranked();
        let keys: Vec<&str> = ranked.iter().map(|k| k.key.as_str()).collect();
        let rep = replaceable();
        // alone: no_such_graph 2, chalk 2 (but fewer layers), strokes 1, replaceable 1 (fewer layers)
        assert_eq!(
            keys,
            [
                "substance:no_such_graph",
                "brush-alpha:chalk",
                "strokes:brush",
                rep.as_str()
            ]
        );
        let graph = &ranked[0];
        assert_eq!((graph.projects, graph.layers, graph.alone, graph.visible), (2, 3, 2, 2));
        assert_eq!(graph.kind, Some(BlockerKind::Substance));
        assert_eq!(graph.support, Some(Support::Unsupported));
        assert_eq!(
            graph.example, "a.spp: substance:no_such_graph",
            "the first project's text"
        );
        let strokes = &ranked[2];
        assert_eq!(
            (strokes.projects, strokes.layers, strokes.alone, strokes.visible),
            (1, 3, 1, 2)
        );
        assert_eq!(strokes.support, Some(Support::Replayable));
        assert!(ranked[3].replacement().is_some() && graph.replacement().is_none());
    }

    #[test]
    fn totals_split_locked_layers_by_what_unlocks_them() {
        let t = census().totals();
        assert_eq!((t.projects, t.failed, t.sets, t.layers), (3, 1, 3, 30));
        assert_eq!((t.locked, t.visible_locked, t.unlocked_projects), (8, 7, 1));
        assert_eq!((t.by_replace, t.by_replay, t.by_both, t.needs_support), (1, 1, 1, 5));
        assert_eq!(t.by_choice(), 3);
        assert_eq!(t.keys, 4);
    }

    #[test]
    fn kinds_count_each_layer_once_and_what_they_lock_alone() {
        let kinds = census().by_kind();
        assert_eq!(
            kinds,
            vec![
                (BlockerKind::Substance, 5, 3),
                (BlockerKind::BrushAlpha, 2, 2),
                (BlockerKind::Strokes, 3, 1),
            ]
        );
    }

    #[test]
    fn unknown_keys_need_support() {
        assert_eq!(Unlock::of(&layer("x", true, &["not a key"])), Unlock::NeedsSupport);
        assert_eq!(Unlock::of(&layer("x", true, &[])), Unlock::NeedsSupport);
        assert!(Census::default().ranked().is_empty());
        assert_eq!(Census::default().totals(), Totals::default());
    }
}
