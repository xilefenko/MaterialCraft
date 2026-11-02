//! [`Reach`]: which texture sets a layer edit reaches beyond its own (IN-13). An edit of a layer changes every set
//! with an instance reading that layer, directly, through a source folder holding it, or through further
//! instances; only those sets are composited again, the others keep their maps.

use super::panels::layers::LayerEvent;
use materialcraft::paint::Document;
use materialcraft::paint::edit::LayerRef;
use materialcraft::paint::edit::ops::{DropTarget, slot_of};

/// The layers an edit changes (texture set id, layer id), each with the folder holding it, and sets it adds
/// layers to directly.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Reach {
    layers: Vec<(String, String)>,
    sets: Vec<String>,
}

impl Reach {
    /// One layer (a fill whose projection changed, …).
    pub fn of_layer(doc: &Document, r: &LayerRef) -> Reach {
        let mut reach = Reach::default();
        reach.add(doc, &r.set, &r.layer);
        reach
    }

    /// The layers `ev` changes where they sit before it is applied.
    pub fn before(doc: &Document, ev: &LayerEvent) -> Reach {
        let mut reach = Reach::default();
        reach.add_event(doc, ev);
        reach
    }

    /// Adds where the layers `ev` changed sit after it was applied, and the layers it made (`made`, ids in the event's
    /// set), so moves out of and into source folders are both seen.
    pub fn after(&mut self, doc: &Document, ev: &LayerEvent, made: &[String]) {
        self.add_event(doc, ev);
        if let Some(set) = event_set(ev) {
            for id in made {
                self.add(doc, set, id);
            }
        }
    }

    /// The indices of the sets besides `edited` the edit reaches, ascending.
    pub fn also(&self, doc: &Document, edited: Option<usize>) -> Vec<usize> {
        let changed: Vec<(&str, &str)> = self.layers.iter().map(|(s, l)| (s.as_str(), l.as_str())).collect();
        let mut out = doc.instance_dependents(&changed);
        out.extend(self.sets.iter().filter_map(|id| doc.texture_sets.iter().position(|s| s.id == *id)));
        out.retain(|i| Some(*i) != edited);
        out.sort_unstable();
        out.dedup();
        out
    }

    fn add(&mut self, doc: &Document, set: &str, id: &str) {
        let mut push = |layer: &str| {
            let entry = (set.to_string(), layer.to_string());
            if !self.layers.contains(&entry) {
                self.layers.push(entry);
            }
        };
        push(id);
        if let Some(parent) = doc.texture_set(set).and_then(|s| slot_of(s, id)).and_then(|slot| slot.parent) {
            push(&parent);
        }
    }

    fn add_event(&mut self, doc: &Document, ev: &LayerEvent) {
        use LayerEvent::*;
        let one = |r: &LayerRef| (r.set.clone(), vec![r.layer.clone()]);
        let (set, ids) = match ev {
            Create { set, above, .. } | PasteInstance { set, above, .. } => (set.clone(), above.iter().cloned().collect()),
            Duplicate { set, ids } | Delete { set, ids } | Group { set, ids } => (set.clone(), ids.clone()),
            Move { set, ids, to, .. } => (set.clone(), ids.iter().cloned().chain(to.parent.clone()).collect()),
            Rename(r, _) | SetVisible(r, _) | Unlock(r) | AddMask(r, _) | RemoveMask(r) | SetMaskEnabled(r, _) | InvertMask(r) | SetGeometryMask(r, _) => {
                one(r)
            }
            SetOpacity { layer, .. } | SetBlendMode { layer, .. } | AddEffect { layer, .. } | RemoveEffect { layer, .. } | SetEffectEnabled { layer, .. } => {
                one(layer)
            }
            Drop { target: DropTarget::OntoLayer(r) | DropTarget::OntoMask(r), .. } => one(r),
            Drop { target: DropTarget::Between { set, at }, .. } => (set.clone(), at.parent.iter().cloned().collect()),
            Instantiate { sets, .. } => {
                self.sets.extend(sets.iter().cloned());
                return;
            }
            EndGesture | Isolate(_) | EditGeometryMask(_) | SaveAsset(_) => return,
        };
        for id in &ids {
            self.add(doc, &set, id);
        }
    }
}

/// The set an event adds layers to.
fn event_set(ev: &LayerEvent) -> Option<&str> {
    use LayerEvent::*;
    match ev {
        Create { set, .. } | PasteInstance { set, .. } | Duplicate { set, .. } | Group { set, .. } | Move { set, .. } => Some(set),
        Drop { target, .. } => Some(target.set()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "instance_reach_tests.rs"]
mod tests;
