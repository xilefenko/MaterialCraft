//! [`LayerLock`]: a layer whose content stands for something MaterialCraft cannot evaluate or edit yet (an
//! imported layer built on a compiled procedural, a filter or brush work it cannot compute), so its content is
//! kept as it came and protected from edits that would silently turn it into something else.
//!
//! A locked layer still takes part in the stack like any other: it can be shown or hidden, renamed, moved,
//! deleted, and its opacity, blending and mask switch can change. What is refused is everything that edits its
//! content: the base (pixels, fill, strokes), the content and mask effect stacks, the mask itself and the
//! geometry mask (see [`crate::paint::edit`]). Removing the lock is an edit of its own (undoable), for a user who
//! wants to edit the layer anyway.

/// Why a layer is locked and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerLock {
    /// What locked it, e.g. `Substance 3D Painter import`.
    pub source: String,
    /// Human-readable reasons, e.g. `procedural "Dirt" has no replacement yet`; may be empty.
    pub reasons: Vec<String>,
}

impl LayerLock {
    pub fn new(source: impl Into<String>, reasons: Vec<String>) -> LayerLock {
        LayerLock { source: source.into(), reasons }
    }

    /// One line for a tooltip or a log: the source and the reasons.
    pub fn summary(&self) -> String {
        if self.reasons.is_empty() { format!("Locked ({})", self.source) } else { format!("Locked ({}): {}", self.source, self.reasons.join("; ")) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_names_source_and_reasons() {
        assert_eq!(LayerLock::new("import", Vec::new()).summary(), "Locked (import)");
        let l = LayerLock::new("import", vec!["procedural \"Dirt\"".into(), "strokes".into()]);
        assert_eq!(l.summary(), "Locked (import): procedural \"Dirt\"; strokes");
    }
}
