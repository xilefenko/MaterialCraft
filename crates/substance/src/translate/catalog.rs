//! The support catalog: for every [`BlockerKey`], what MaterialCraft can do about it today. This is the one place
//! that answers "can this be unlocked?": the import review, the blockers panel, the import report and
//! `mcsub blockers` all ask [`support`], so adding support for something (a replacement in
//! [`super::replacements`], a brush the replay can paint) shows up everywhere at once.
//!
//! | Status | Meaning | What unlocks it |
//! |---|---|---|
//! | [`Support::Replaceable`] | MaterialCraft has its own stand-in | choosing "replace" in the import review |
//! | [`Support::Replayable`] | our brush can paint the strokes again | importing with stroke replay on |
//! | [`Support::Unsupported`] | nothing yet | adding support; until then the layer stays locked |
//!
//! A blocker that's [`Support::Replaceable`] or [`Support::Replayable`] still blocked the import it was met in
//! because of a choice (the procedural was kept, replay was off); the catalog says what the other choice gives.

use super::blockers::{BlockerKey, BlockerKind};
use super::replacements;

/// What MaterialCraft can do about a blocker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Our generator, texture or filter `by` stands in for it; `note` says what it leaves out.
    Replaceable { by: &'static str, note: &'static str },
    /// Our brush paints the strokes again when the import replays strokes.
    Replayable,
    /// Not supported yet: what it touches stays locked.
    Unsupported,
}

impl Support {
    /// Stable id for files and JSON: `replaceable`, `replayable`, `unsupported`.
    pub fn id(self) -> &'static str {
        match self {
            Support::Replaceable { .. } => "replaceable",
            Support::Replayable => "replayable",
            Support::Unsupported => "unsupported",
        }
    }

    /// Short label for people.
    pub fn label(self) -> &'static str {
        match self {
            Support::Replaceable { .. } => "Replaceable",
            Support::Replayable => "Replayable",
            Support::Unsupported => "Not supported yet",
        }
    }

    /// What unlocks it, for people.
    pub fn action(self) -> String {
        match self {
            Support::Replaceable { by, .. } => {
                format!("Choose \"replace\" in the import review (MaterialCraft's {by})")
            }
            Support::Replayable => "Import with stroke replay on".to_string(),
            Support::Unsupported => "Stays locked until MaterialCraft supports it".to_string(),
        }
    }

    /// True if a choice at import time can unlock it today.
    pub fn is_supported(self) -> bool {
        !matches!(self, Support::Unsupported)
    }
}

/// What MaterialCraft can do about `key` today.
pub fn support(key: &BlockerKey) -> Support {
    match key.kind {
        BlockerKind::Substance => match replacements::find(&key.name) {
            Some(r) => Support::Replaceable {
                by: r.generator,
                note: r.note,
            },
            None => Support::Unsupported,
        },
        BlockerKind::Strokes => Support::Replayable,
        BlockerKind::BrushAlpha | BlockerKind::Brush | BlockerKind::Feature | BlockerKind::Class => {
            Support::Unsupported
        }
    }
}

/// [`support`] for a key written as text (`kind:name`); `None` for a key that isn't a blocker key.
pub fn support_of(key: &str) -> Option<Support> {
    BlockerKey::parse(key).map(|k| support(&k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substance_graphs_follow_the_replacement_table() {
        let with = replacements::entries()
            .next()
            .and_then(|r| r.painter.first())
            .expect("the table has entries");
        let key = BlockerKey::new(BlockerKind::Substance, *with);
        assert!(matches!(support(&key), Support::Replaceable { .. }), "{key}");
        assert_eq!(
            support(&BlockerKey::new(BlockerKind::Substance, "no_such_graph")),
            Support::Unsupported
        );
    }

    #[test]
    fn strokes_are_replayable_and_the_rest_unsupported() {
        assert_eq!(support_of("strokes:brush"), Some(Support::Replayable));
        assert_eq!(support_of("brush-alpha:charcoal_brush"), Some(Support::Unsupported));
        assert_eq!(support_of("feature:folder-effects"), Some(Support::Unsupported));
        assert_eq!(support_of("not a key"), None);
        assert!(Support::Replayable.is_supported() && !Support::Unsupported.is_supported());
    }
}
