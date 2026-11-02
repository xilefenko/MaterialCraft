//! [`LockDiff`]: what changed in the locks between two imports of the same project (a document and its
//! re-import): layers newly unlocked, newly locked and still locked, and the layers per blocker key before and
//! after.
//!
//! Layers are matched by texture set id and layer id. Imported layer ids come from Painter's layer uids
//! (`layer-<uid>`, see `translate::layers::stack_builder`), so the same Painter layer has the same id in every
//! import of the project; the few layers without a uid are numbered in stack order, which only holds while the
//! project's stacks don't change.

use crate::translate::blocker_summary::{BlockerSummary, LockedLayer};
use materialcraft::paint::Document;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// One layer whose lock is compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerLocks {
    /// Texture set id and layer id.
    pub set: String,
    pub layer: String,
    /// `Set / Folder / Layer` (as named in the newer import when the layer is in both).
    pub path: String,
    /// Blocker keys before and after (empty: not locked, or not there).
    pub before: Vec<String>,
    pub after: Vec<String>,
}

/// Layers per blocker key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCount {
    pub key: String,
    pub before: usize,
    pub after: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LockDiff {
    /// Locked before, not after (the layer is still there).
    pub unlocked: Vec<LayerLocks>,
    /// Locked after, not before (or not there before).
    pub locked: Vec<LayerLocks>,
    /// Locked in both (their keys may differ).
    pub still_locked: Vec<LayerLocks>,
    /// Locked before and no longer there after.
    pub gone: Vec<LayerLocks>,
    /// Every key locked layers had before or after, by key.
    pub keys: Vec<KeyCount>,
}

impl LockDiff {
    /// The diff from document `before` to `after`.
    pub fn of(before: &Document, after: &Document) -> LockDiff {
        let present: BTreeSet<(String, String)> = after
            .texture_sets
            .iter()
            .flat_map(|s| s.walk_layers().map(move |(_, l)| (s.id.clone(), l.id.clone())))
            .collect();
        LockDiff::between(&BlockerSummary::of(before), &BlockerSummary::of(after), |set, layer| {
            present.contains(&(set.to_string(), layer.to_string()))
        })
    }

    /// The diff of two summaries; `in_after` says whether a layer (set id, layer id) exists in the newer document,
    /// to tell an unlocked layer from one that is gone.
    pub fn between(before: &BlockerSummary, after: &BlockerSummary, in_after: impl Fn(&str, &str) -> bool) -> LockDiff {
        let index = |s: &BlockerSummary| -> BTreeMap<(String, String), LockedLayer> {
            s.locked
                .iter()
                .map(|l| ((l.set.clone(), l.layer.clone()), l.clone()))
                .collect()
        };
        let (old, new) = (index(before), index(after));
        let mut diff = LockDiff::default();
        for (id, b) in &old {
            let entry = |after: &[String], path: &LockedLayer| LayerLocks {
                set: id.0.clone(),
                layer: id.1.clone(),
                path: format!("{} / {}", path.set_name, path.path),
                before: b.keys.clone(),
                after: after.to_vec(),
            };
            match new.get(id) {
                Some(a) => diff.still_locked.push(entry(&a.keys, a)),
                None if in_after(&id.0, &id.1) => diff.unlocked.push(entry(&[], b)),
                None => diff.gone.push(entry(&[], b)),
            }
        }
        for (id, a) in new.iter().filter(|(id, _)| !old.contains_key(*id)) {
            diff.locked.push(LayerLocks {
                set: id.0.clone(),
                layer: id.1.clone(),
                path: format!("{} / {}", a.set_name, a.path),
                before: Vec::new(),
                after: a.keys.clone(),
            });
        }
        let mut keys: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for b in &before.blockers {
            keys.entry(&b.key).or_default().0 = b.layers.len();
        }
        for b in &after.blockers {
            keys.entry(&b.key).or_default().1 = b.layers.len();
        }
        diff.keys = keys
            .into_iter()
            .map(|(key, (before, after))| KeyCount {
                key: key.to_string(),
                before,
                after,
            })
            .collect();
        diff
    }

    /// Locked layers before and after.
    pub fn totals(&self) -> (usize, usize) {
        let before = self.unlocked.len() + self.still_locked.len() + self.gone.len();
        (before, self.locked.len() + self.still_locked.len())
    }

    /// One line for a log: `5 layers unlocked, 0 newly locked, 3 still locked (8 → 3 locked)`.
    pub fn headline(&self) -> String {
        let (before, after) = self.totals();
        let mut line = format!(
            "{} unlocked, {} newly locked, {} still locked ({before} → {after} locked layers)",
            self.unlocked.len(),
            self.locked.len(),
            self.still_locked.len()
        );
        if !self.gone.is_empty() {
            line.push_str(&format!("; {} locked layers no longer there", self.gone.len()));
        }
        line
    }
}

impl fmt::Display for LockDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "locks: {}", self.headline())?;
        let groups = [
            ("unlocked", &self.unlocked),
            ("newly locked", &self.locked),
            ("still locked", &self.still_locked),
            ("no longer there (were locked)", &self.gone),
        ];
        for (title, layers) in groups.into_iter().filter(|(_, l)| !l.is_empty()) {
            writeln!(f, "\n{title} ({}):", layers.len())?;
            for l in layers {
                write!(f, "  {}  [{}]", l.path, l.layer)?;
                match (l.before.is_empty(), l.after.is_empty()) {
                    (false, true) => writeln!(f, "  was: {}", l.before.join(", "))?,
                    (true, false) => writeln!(f, "  by: {}", l.after.join(", "))?,
                    _ if l.before == l.after => writeln!(f, "  by: {}", l.after.join(", "))?,
                    _ => writeln!(f, "  by: {} (was: {})", l.after.join(", "), l.before.join(", "))?,
                }
            }
        }
        if !self.keys.is_empty() {
            writeln!(f, "\nblockers (locked layers before → after):")?;
            let width = self.keys.iter().map(|k| k.key.chars().count()).max().unwrap_or(0);
            for k in &self.keys {
                let mark = match k.after.cmp(&k.before) {
                    std::cmp::Ordering::Less => "  fewer",
                    std::cmp::Ordering::Greater => "  more",
                    std::cmp::Ordering::Equal => "",
                };
                let line = format!("  {:<width$}  {:>4} → {:<4}{mark}", k.key, k.before, k.after);
                writeln!(f, "{}", line.trim_end())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(id: &str, keys: &[&str]) -> LockedLayer {
        LockedLayer {
            set: "s".into(),
            set_name: "Body".into(),
            layer: id.into(),
            path: id.to_uppercase(),
            visible: true,
            keys: keys.iter().map(|k| k.to_string()).collect(),
        }
    }

    #[test]
    fn layers_are_matched_by_id_and_keys_counted() {
        let before = BlockerSummary::from_locked(vec![
            layer("a", &["substance:mg_dirt"]),
            layer("b", &["substance:mg_dirt", "strokes:brush"]),
            layer("c", &["substance:blur"]),
            layer("gone", &["substance:blur"]),
        ]);
        let after = BlockerSummary::from_locked(vec![layer("b", &["strokes:brush"]), layer("d", &["substance:warp"])]);
        let diff = LockDiff::between(&before, &after, |_, l| l != "gone");
        let ids = |v: &[LayerLocks]| v.iter().map(|l| l.layer.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&diff.unlocked), ["a", "c"]);
        assert_eq!(ids(&diff.locked), ["d"]);
        assert_eq!(ids(&diff.still_locked), ["b"]);
        assert_eq!(ids(&diff.gone), ["gone"]);
        assert_eq!(diff.still_locked[0].before.len(), 2);
        assert_eq!(diff.unlocked[0].path, "Body / A");
        let count = |k: &str| diff.keys.iter().find(|c| c.key == k).map(|c| (c.before, c.after));
        assert_eq!(count("substance:mg_dirt"), Some((2, 0)));
        assert_eq!(count("substance:blur"), Some((2, 0)));
        assert_eq!(count("strokes:brush"), Some((1, 1)));
        assert_eq!(count("substance:warp"), Some((0, 1)));
        assert_eq!(diff.totals(), (4, 2));
        assert_eq!(
            diff.headline(),
            "2 unlocked, 1 newly locked, 1 still locked (4 → 2 locked layers); 1 locked layers no longer there"
        );
        let text = diff.to_string();
        assert!(
            text.contains("unlocked (2):\n  Body / A  [a]  was: substance:mg_dirt\n"),
            "{text}"
        );
        assert!(
            text.contains("by: strokes:brush (was: substance:mg_dirt, strokes:brush)"),
            "{text}"
        );
    }

    #[test]
    fn same_locks_are_no_change() {
        let s = BlockerSummary::from_locked(vec![layer("a", &["substance:mg_dirt"])]);
        let diff = LockDiff::between(&s, &s, |_, _| true);
        assert!(diff.unlocked.is_empty() && diff.locked.is_empty() && diff.gone.is_empty());
        assert_eq!(
            diff.headline(),
            "0 unlocked, 0 newly locked, 1 still locked (1 → 1 locked layers)"
        );
        assert!(LockDiff::of(&Document::default(), &Document::default()).keys.is_empty());
    }
}
