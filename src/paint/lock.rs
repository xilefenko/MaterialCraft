//! [`LayerLock`]: a layer whose content stands for something MaterialCraft cannot evaluate or edit yet (an
//! imported layer built on a compiled procedural, a filter or brush work it cannot compute), so its content is
//! kept as it came and protected from edits that would silently turn it into something else.
//!
//! A locked layer still takes part in the stack like any other: it can be shown or hidden, renamed, moved,
//! deleted, and its opacity, blending and mask switch can change. What is refused is everything that edits its
//! content: the base (pixels, fill, strokes), the content and mask effect stacks, the mask itself and the
//! geometry mask (see [`crate::paint::edit`]). Removing the lock is an edit of its own (undoable), for a user who
//! wants to edit the layer anyway.

/// One reason a layer is locked: a key that names what blocks it, stable across documents (opaque here; the
/// source that locked the layer defines it, e.g. `substance:mg_dirt` from the Painter import), and the text for people.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockReason {
    /// May be empty for a reason without a key.
    pub key: String,
    pub text: String,
}

impl LockReason {
    pub fn new(key: impl Into<String>, text: impl Into<String>) -> LockReason {
        LockReason { key: key.into(), text: text.into() }
    }
}

/// Why a layer is locked and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerLock {
    /// What locked it, e.g. `Substance 3D Painter import`.
    pub source: String,
    /// What blocks it; may be empty.
    pub reasons: Vec<LockReason>,
}

impl LayerLock {
    pub fn new(source: impl Into<String>, reasons: Vec<LockReason>) -> LayerLock {
        LayerLock { source: source.into(), reasons }
    }

    /// One line for a tooltip or a log: the source and the reasons.
    pub fn summary(&self) -> String {
        let texts: Vec<&str> = self.reasons.iter().map(|r| r.text.as_str()).collect();
        if texts.is_empty() { format!("Locked ({})", self.source) } else { format!("Locked ({}): {}", self.source, texts.join("; ")) }
    }

    /// The distinct keys of the reasons, in order (reasons without a key left out).
    pub fn keys(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = Vec::new();
        for r in &self.reasons {
            if !r.key.is_empty() && !keys.contains(&r.key.as_str()) {
                keys.push(&r.key);
            }
        }
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_names_source_and_reasons() {
        assert_eq!(LayerLock::new("import", Vec::new()).summary(), "Locked (import)");
        let dirt = LockReason::new("substance:mg_dirt", "mask generator \"mg_dirt\"");
        let again = LockReason::new("substance:mg_dirt", "mask filter \"mg_dirt\"");
        let l = LayerLock::new("import", vec![dirt, again, LockReason::new("", "strokes")]);
        assert_eq!(l.summary(), "Locked (import): mask generator \"mg_dirt\"; mask filter \"mg_dirt\"; strokes");
        assert_eq!(l.keys(), ["substance:mg_dirt"]);
    }
}
