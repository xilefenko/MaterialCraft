//! Turns a [`RawTree`] into a [`PainterDocument`]. Never fails: what it does not know becomes a
//! [`RawEntry`] and is counted in the [`ModelReport`]. Split by level: texture sets, layers, effects, sources,
//! strokes.

mod brush;
mod decal;
mod effect;
mod fill_projection;
mod layer;
mod source;
mod stroke;
mod texture_set;

use super::document::PainterDocument;
use super::raw::{RawObject, RawTree};
use super::raw_entry::RawEntry;
use super::report::ModelReport;

/// Deepest nesting of folders, effect groups and procedural inputs the builder follows; real files stay
/// far below (folders 4, effect groups 2).
const MAX_NESTING: usize = 64;

/// Builds the model and keeps the tally.
pub struct ModelBuilder {
    report: ModelReport,
}

impl ModelBuilder {
    pub fn build(tree: &RawTree) -> (PainterDocument, ModelReport) {
        let mut b = ModelBuilder {
            report: ModelReport::with_totals(&tree.classes),
        };
        b.report.unknown_tags = tree.unknown_tags;
        let texture_sets = match &tree.root {
            Some(root) if root.class == "DataDocument" => {
                b.seen(root);
                root.objects("materials").map(|m| b.texture_set(m)).collect()
            }
            Some(other) => {
                b.raw(other);
                Vec::new()
            }
            None => Vec::new(),
        };
        let doc = PainterDocument {
            format_version: tree.format_version,
            texture_sets,
        };
        (doc, b.report)
    }

    /// Marks an object as turned into a typed value.
    fn seen(&mut self, object: &RawObject) {
        self.report.recognise(&object.class);
    }

    /// Keeps an object the model does not understand.
    fn raw(&mut self, object: &RawObject) -> RawEntry {
        self.report.keep_raw(&object.class);
        RawEntry::of(object)
    }

    /// The nesting limit was hit: keep the object raw and note it.
    fn too_deep(&mut self, object: &RawObject) -> RawEntry {
        self.report.depth_limited += 1;
        self.raw(object)
    }
}
