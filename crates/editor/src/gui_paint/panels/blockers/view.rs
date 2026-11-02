//! [`BlockersView`]: what the blockers panel shows, worked out once per document state: the importer's
//! [`BlockerSummary`] grouped by blocker kind, with totals and the texts of each row. No drawing here.

use materialcraft::paint::Document;
use materialcraft_substance::translate::blocker_summary::{BlockerSummary, BlockerUse, LockedLayer};
use materialcraft_substance::translate::blockers::{BlockerKey, BlockerKind};
use materialcraft_substance::translate::catalog::Support;

/// The heading of blockers whose key isn't a blocker key the catalog knows (a lock from another source).
const OTHER: &str = "Other";

/// The blockers of one kind.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// [`BlockerKind::label`], or "Other".
    pub label: &'static str,
    /// Indexes into [`BlockerSummary::blockers`], most layers first.
    pub blockers: Vec<usize>,
}

/// The header line's numbers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub layers: usize,
    pub blockers: usize,
    pub replaceable: usize,
    pub replayable: usize,
    /// Not supported yet, and keys the catalog doesn't know (nothing at import time unlocks those either).
    pub unsupported: usize,
}

impl Totals {
    fn of(summary: &BlockerSummary) -> Totals {
        let mut t = Totals { layers: summary.locked.len(), blockers: summary.blockers.len(), ..Totals::default() };
        for b in &summary.blockers {
            match b.support {
                Some(Support::Replaceable { .. }) => t.replaceable += 1,
                Some(Support::Replayable) => t.replayable += 1,
                Some(Support::Unsupported) | None => t.unsupported += 1,
            }
        }
        t
    }

    /// "12 locked layers, 5 blockers: 2 replaceable, 1 replayable, 2 not supported yet".
    pub fn line(&self) -> String {
        let mut parts = Vec::new();
        for (n, what) in [(self.replaceable, "replaceable"), (self.replayable, "replayable"), (self.unsupported, "not supported yet")] {
            if n > 0 {
                parts.push(format!("{n} {what}"));
            }
        }
        let head = format!("{}, {}", count(self.layers, "locked layer"), count(self.blockers, "blocker"));
        if parts.is_empty() { head } else { format!("{head}: {}", parts.join(", ")) }
    }
}

/// The panel's content for one document state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlockersView {
    pub summary: BlockerSummary,
    /// In [`BlockerKind::ALL`] order, "Other" last; kinds without blockers left out.
    pub groups: Vec<Group>,
    pub totals: Totals,
}

impl BlockersView {
    pub fn of(doc: &Document) -> BlockersView {
        BlockersView::from_summary(BlockerSummary::of(doc).with_texts(doc))
    }

    pub fn from_summary(summary: BlockerSummary) -> BlockersView {
        let kinds: Vec<Option<BlockerKind>> = BlockerKind::ALL.into_iter().map(Some).chain([None]).collect();
        let groups = kinds
            .into_iter()
            .filter_map(|kind| {
                let blockers: Vec<usize> = summary.blockers.iter().enumerate().filter(|(_, b)| b.parsed().map(|k| k.kind) == kind).map(|(i, _)| i).collect();
                let label = kind.map_or(OTHER, BlockerKind::label);
                (!blockers.is_empty()).then_some(Group { label, blockers })
            })
            .collect();
        let totals = Totals::of(&summary);
        BlockersView { summary, groups, totals }
    }

    pub fn blocker(&self, i: usize) -> Option<&BlockerUse> {
        self.summary.blockers.get(i)
    }

    pub fn layer(&self, i: usize) -> Option<&LockedLayer> {
        self.summary.locked.get(i)
    }
}

/// The blocker's name without its kind (`mg_dirt` for `substance:mg_dirt`), or the whole key if it isn't one.
pub fn name(b: &BlockerUse) -> String {
    BlockerKey::parse(&b.key).map_or_else(|| b.key.clone(), |k| k.name)
}

/// "locks 3 layers (1 alone)"; "(none alone)" when every layer it locks has another blocker too.
pub fn uses(b: &BlockerUse) -> String {
    let alone = match b.only {
        0 => "none alone".to_string(),
        n if n == b.layers.len() => "all alone".to_string(),
        n => format!("{n} alone"),
    };
    format!("locks {} ({alone})", count(b.layers.len(), "layer"))
}

/// The catalog status for people ("Unknown" for a key the catalog doesn't know).
pub fn status(b: &BlockerUse) -> &'static str {
    b.support.map_or("Unknown", Support::label)
}

/// What unlocks it, for people.
pub fn action(b: &BlockerUse) -> String {
    match b.support {
        Some(s) => s.action(),
        None => "Not a blocker MaterialCraft knows: unlock the layer to edit it".to_string(),
    }
}

/// "Body / Rust / Dirt", with "(hidden)" for a hidden layer.
pub fn layer_label(l: &LockedLayer) -> String {
    let hidden = if l.visible { "" } else { " (hidden)" };
    format!("{} / {}{hidden}", l.set_name, l.path)
}

fn count(n: usize, what: &str) -> String {
    if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locked(path: &str, keys: &[&str]) -> LockedLayer {
        LockedLayer {
            set: "s".into(),
            set_name: "Body".into(),
            layer: path.to_lowercase(),
            path: path.into(),
            visible: true,
            keys: keys.iter().map(|k| k.to_string()).collect(),
        }
    }

    fn view() -> BlockersView {
        let summary = BlockerSummary::from_locked(vec![
            locked("A", &["substance:no_such_graph", "strokes:painted-mask"]),
            locked("B", &["substance:no_such_graph"]),
            locked("C", &["brush-alpha:charcoal"]),
            locked("D", &["elsewhere:thing"]),
        ]);
        BlockersView::from_summary(summary)
    }

    #[test]
    fn blockers_group_by_kind_in_catalog_order_with_others_last() {
        let v = view();
        let labels: Vec<&str> = v.groups.iter().map(|g| g.label).collect();
        assert_eq!(labels, [BlockerKind::Substance.label(), BlockerKind::BrushAlpha.label(), BlockerKind::Strokes.label(), OTHER]);
        let first = v.groups.first().and_then(|g| g.blockers.first()).and_then(|i| v.blocker(*i)).expect("a substance blocker");
        assert_eq!(first.key, "substance:no_such_graph");
        assert_eq!(name(first), "no_such_graph");
        assert_eq!(uses(first), "locks 2 layers (1 alone)");
        assert_eq!(status(first), "Not supported yet");
    }

    #[test]
    fn totals_count_layers_blockers_and_support() {
        let t = view().totals;
        assert_eq!(t, Totals { layers: 4, blockers: 4, replaceable: 0, replayable: 1, unsupported: 3 });
        assert_eq!(t.line(), "4 locked layers, 4 blockers: 1 replayable, 3 not supported yet");
        assert_eq!(Totals::default().line(), "0 locked layers, 0 blockers");
    }

    #[test]
    fn row_texts() {
        let v = view();
        let strokes = v.summary.blockers.iter().find(|b| b.key.starts_with("strokes:")).expect("strokes");
        assert_eq!(uses(strokes), "locks 1 layer (none alone)");
        assert_eq!(action(strokes), Support::Replayable.action());
        let other = v.summary.blockers.iter().find(|b| b.key == "elsewhere:thing").expect("other");
        assert_eq!((name(other), status(other), uses(other)), ("elsewhere:thing".to_string(), "Unknown", "locks 1 layer (all alone)".to_string()));
        let mut hidden = locked("Rust / Dirt", &[]);
        hidden.visible = false;
        assert_eq!(layer_label(&hidden), "Body / Rust / Dirt (hidden)");
    }
}
