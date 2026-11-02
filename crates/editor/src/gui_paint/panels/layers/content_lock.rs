//! Whether a layer's content can be edited: the editor's side of `paint::edit`'s rule for locked layers, so the
//! panels show a locked layer's fill, projection, effects and mask read-only instead of offering edits the model
//! refuses. Visibility, opacity, blending, the name and the mask switch stay editable on a locked layer.

use materialcraft::paint::edit::LayerRef;
use materialcraft::paint::{Document, LayerLock};

/// The lock that keeps `r`'s content (base, fill and projection, effects, mask, pixels) from being edited, if any.
pub fn content_lock<'a>(doc: &'a Document, r: &LayerRef) -> Option<&'a LayerLock> {
    doc.texture_set(&r.set)?.layer(&r.layer)?.lock.as_ref()
}

/// True if `r`'s content can be edited as far as locks go (also for a layer that isn't there: nothing to refuse).
pub fn content_editable(doc: &Document, r: &LayerRef) -> bool {
    content_lock(doc, r).is_none()
}

/// The one-line note shown above a locked layer's read-only properties.
pub const NOTE: &str = "This layer is locked: its content can't be edited. Right-click it in the layer list to unlock it.";

#[cfg(test)]
mod tests {
    use super::*;
    use materialcraft::paint::{Fill, Layer, LayerKind, LockReason, TextureSet};

    #[test]
    fn only_a_locked_layer_refuses_content_edits() {
        let mut set = TextureSet::new("s", "Body", (4, 4));
        let mut locked = Layer::new("locked", "Locked", LayerKind::Fill(Fill::uniform(vec![])));
        locked.lock = Some(LayerLock::new("import", vec![LockReason::new("substance:mg_dirt", "mask generator")]));
        let free = Layer::new("free", "Free", LayerKind::Fill(Fill::uniform(vec![])));
        set.layers = vec![locked, free];
        let doc = Document { texture_sets: vec![set], ..Document::default() };
        assert!(content_lock(&doc, &LayerRef::new("s", "locked")).is_some_and(|l| l.source == "import"));
        assert!(!content_editable(&doc, &LayerRef::new("s", "locked")));
        assert!(content_editable(&doc, &LayerRef::new("s", "free")));
        assert!(content_editable(&doc, &LayerRef::new("s", "missing")) && content_editable(&doc, &LayerRef::new("zz", "free")));
    }
}
