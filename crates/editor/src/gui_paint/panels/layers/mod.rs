//! The layer panel (roadmap step 11): the focused texture set's stack with thumbnails, the view channel,
//! effects, mask states, create/duplicate/delete/group/rename, drag-and-drop reordering with a landing marker
//! and multi-selection. It changes nothing itself: every request is a [`LayerEvent`] that `PaintMode` applies
//! through `paint::edit` and the history.
//!
//! - [`rows`]: the listed rows and pointer → drop target; [`selection`]: the selection and paint target;
//! - [`row_ui`], [`bars`], [`menus`]: drawing; [`thumbnails`]: thumbnails off the UI thread;
//! - [`instance_icon`], [`instance_list`], [`instantiate_dialog`]: layer instances (IN-05 … IN-10): row badges, the
//!   list of a source's instances, the dialog that instances a layer into other texture sets;
//! - [`list`]: the scrolling list with drags; [`StackDropTarget`]: what the asset tray calls while dragging
//!   an asset over the stack (LS-18, LS-19).

mod bars;
pub mod events;
mod geometry_icon;
mod instance_icon;
mod instance_list;
pub mod instantiate_dialog;
mod list;
mod lock_icon;
pub mod lock_text;
mod menus;
mod row_ui;
pub mod rows;
pub mod selection;
pub mod thumbnails;

#[cfg(test)]
mod instance_tests;
#[cfg(test)]
mod tests;

pub use events::{LayerEvent, SaveAsset};

use super::PanelEvent;
use crate::gui_paint::document_state::OpenDocument;
use crate::gui_paint::fill_surfaces::FillSurfaces;
use eframe::egui::{Pos2, Ui};
use instantiate_dialog::{InstantiateDialog, Outcome};
use materialcraft::history::CoalesceKey;
use materialcraft::paint::edit::LayerRef;
use materialcraft::paint::edit::ops::{DropPayload, DropTarget, FillAt, FillRef, NewLayer};
use materialcraft::paint::instance::InstanceIndex;
use materialcraft::paint::{Channel, Document, GeometryMask, LayerKind, TextureSet};
use menus::Local;
use rows::{Folding, Rows};
use selection::{Part, Selection};
use std::sync::Arc;
use thumbnails::Thumbnails;

/// Keyboard actions on the selection (from the shortcut table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerAction {
    New(NewLayer),
    Duplicate,
    Group,
    Delete,
    Rename,
    /// Remember the focused layer for "Paste as instance".
    Copy,
    /// An instance of the copied layer above the focus (IN-05).
    PasteInstance,
}

/// What the asset tray (step 15) calls while it drags an asset: where it would land and showing that.
// Nothing drags assets yet; the tray is built in step 15. Tested in `tests`.
#[allow(dead_code)]
pub trait StackDropTarget {
    /// The drop target under the screen position `pos`, if it is over the list and the drop would do
    /// something.
    fn target_at(&self, doc: &Document, pos: Pos2, payload: &DropPayload) -> Option<DropTarget>;
    /// Shows the landing marker for `payload` while it is dragged (`None` when the drag ends).
    fn hover(&mut self, payload: Option<DropPayload>);
}

/// Panel state.
#[derive(Default)]
pub struct LayerPanel {
    set: Option<String>,
    channel: Option<Channel>,
    pub selection: Selection,
    folding: Folding,
    thumbs: Thumbnails,
    list: list::ListState,
    /// The running opacity drag's history key.
    drag_key: Option<CoalesceKey>,
    next_key: u64,
    /// The layer whose mask the viewport shows alone.
    pub isolated: Option<LayerRef>,
    /// The layer whose geometry mask is being edited (GM-03): the viewport hides what it excludes.
    pub geometry_editing: Option<LayerRef>,
    /// The asset being dragged over the stack from elsewhere.
    external: Option<DropPayload>,
    /// The layer copied for "Paste as instance".
    copied: Option<LayerRef>,
    /// The open "Instantiate across texture sets" dialog.
    instantiate: Option<InstantiateDialog>,
}

impl LayerPanel {
    /// The texture set shown, falling back to the first one.
    pub fn current_set<'a>(&self, doc: &'a Document) -> Option<&'a TextureSet> {
        self.set.as_deref().and_then(|id| doc.texture_set(id)).or_else(|| doc.texture_sets.first())
    }

    /// The channel the thumbnails show and the blend controls edit.
    fn view_channel(&self, set: &TextureSet) -> Option<Channel> {
        self.channel.filter(|c| set.has_channel(*c)).or_else(|| set.channels.first().map(|c| c.channel))
    }

    /// Whether the tools can paint: the focused layer's mask, or a paint layer's content (ADR 0008,
    /// mc2-q-pt-02c).
    pub fn paintable(&self, doc: &Document) -> bool {
        let Some(layer) = self.current_set(doc).and_then(|s| s.layer(self.selection.focus()?)) else {
            return false;
        };
        match self.selection.part {
            Part::Mask => layer.mask.is_some(),
            Part::Content => matches!(layer.kind, LayerKind::Paint(_)),
        }
    }

    /// What the brush tools paint into, with its texture set's index: the selection when [`Self::paintable`].
    pub fn paint_target(&self, doc: &Document) -> Option<(materialcraft_brush::PaintTarget, usize)> {
        if !self.paintable(doc) {
            return None;
        }
        let set = self.current_set(doc)?;
        let index = doc.texture_sets.iter().position(|s| s.id == set.id)?;
        let part = match self.selection.part {
            Part::Mask => materialcraft_brush::TargetPart::Mask,
            Part::Content => materialcraft_brush::TargetPart::Content,
        };
        Some((materialcraft_brush::PaintTarget { layer: LayerRef::new(set.id.clone(), self.selection.focus()?), part }, index))
    }

    /// The fill whose projection the projection panel and gizmos edit: a picked fill effect, else the focused fill
    /// layer's fill (with its content, not its mask, selected).
    pub fn selected_fill(&self, doc: &Document) -> Option<FillRef> {
        let set = self.current_set(doc)?;
        let focus = self.selection.focus()?;
        let r = FillRef::new(
            LayerRef::new(set.id.clone(), focus),
            match self.selection.effect() {
                Some(i) => FillAt::Effect(i),
                None if self.selection.part == Part::Content => FillAt::Layer,
                None => return None,
            },
        );
        r.fill(doc).is_some().then_some(r)
    }

    /// Selects `ids` (new layers after an edit).
    pub fn select(&mut self, ids: Vec<String>) {
        self.selection.set(ids);
    }

    /// Shows texture set `set` with `layer` selected and every folder above it unfolded (a log entry's
    /// target, LG-10). The list does not scroll to it yet.
    pub fn reveal(&mut self, set: &TextureSet, layer: &str) {
        if set.layer(layer).is_none() {
            return;
        }
        self.set = Some(set.id.clone());
        let mut slot = materialcraft::paint::edit::ops::slot_of(set, layer);
        while let Some(parent) = slot.and_then(|s| s.parent) {
            self.folding.folded.remove(&parent);
            slot = materialcraft::paint::edit::ops::slot_of(set, &parent);
        }
        self.selection.set(vec![layer.to_string()]);
    }

    /// The surface maps the thumbnails composite fills with (those the view's maps use). Call before drawing.
    pub fn set_fill_surfaces(&mut self, surfaces: Option<Arc<FillSurfaces>>) {
        self.thumbs.set_fill_surfaces(surfaces);
    }

    /// Forgets everything about the previous document.
    pub fn reset(&mut self) {
        *self = LayerPanel::default();
    }

    /// A keyboard action on the selection.
    pub fn action(&mut self, doc: &Document, action: LayerAction) -> Option<LayerEvent> {
        let set = self.current_set(doc)?.id.clone();
        let ids = self.selection.ids().to_vec();
        Some(match action {
            LayerAction::New(what) => LayerEvent::Create { set, what, above: self.selection.focus().map(str::to_string) },
            LayerAction::Duplicate => LayerEvent::Duplicate { set, ids },
            LayerAction::Group => LayerEvent::Group { set, ids },
            LayerAction::Delete => LayerEvent::Delete { set, ids },
            LayerAction::Rename => {
                let id = self.selection.focus()?.to_string();
                let name = doc.texture_set(&set)?.layer(&id)?.name.clone();
                self.list.start_rename(id, name);
                return None;
            }
            LayerAction::Copy => {
                self.copied = Some(LayerRef::new(set, self.selection.focus()?));
                return None;
            }
            LayerAction::PasteInstance => LayerEvent::PasteInstance { source: self.copied.clone()?, set, above: self.selection.focus().map(str::to_string) },
        })
    }

    /// Draws the panel.
    pub fn show(&mut self, ui: &mut Ui, open: &OpenDocument, events: &mut Vec<PanelEvent>) {
        let doc = open.doc();
        let Some(set) = self.current_set(doc) else {
            ui.weak("This document has no texture sets.");
            return;
        };
        self.selection.retain(|id| set.layer(id).is_some());
        let channel = self.view_channel(set);
        let mut out: Vec<LayerEvent> = Vec::new();
        let (new_set, new_channel) = bars::pickers(ui, doc, set, channel);
        let instances = InstanceIndex::of(doc);
        let mut reveal: Option<LayerRef> = None;
        if let Some(focus) = self.selection.focus()
            && let (Some(layer), Some(c)) = (set.layer(focus), channel)
        {
            let r = LayerRef::new(set.id.clone(), focus);
            if matches!(layer.kind, LayerKind::Instance(_)) {
                ui.weak(format!("{} is an instance: its blending, mask and effects are its source's.", layer.name));
            } else {
                bars::blending(ui, &r, layer, c, &mut self.drag_key, &mut self.next_key, &mut out);
            }
            let entries = instance_list::entries(doc, &instances, &r);
            if !entries.is_empty() {
                reveal = instance_list::show(ui, &entries, &r);
            }
        }
        ui.separator();
        let bottom = 30.0;
        let rows = Rows::build(set, &self.folding);
        let height = (ui.available_height() - bottom).max(60.0);
        if let Some(c) = channel {
            self.thumbs.begin(ui.ctx(), &set.id, c);
        }
        let parts = GeometryMask::set_parts(&doc.mesh, set.material_slot);
        let geometry_editing = self.geometry_editing.as_ref();
        let index = doc.texture_sets.iter().position(|s| s.id == set.id);
        let resolved = index.and_then(|i| doc.resolved_set(i)).unwrap_or(std::borrow::Cow::Borrowed(set));
        let ctx = list::ListCtx {
            set,
            rows: &rows,
            channel,
            height,
            external: self.external.as_ref(),
            parts: &parts,
            geometry_editing,
            resolved: &resolved,
            instances: &instances,
            copied: self.copied.as_ref(),
        };
        self.list.show(ui, ctx, &mut self.selection, &mut self.folding, &mut self.thumbs, self.isolated.as_ref(), &mut out);
        match self.list.take_request() {
            Some((Local::Copy, r)) => self.copied = Some(r),
            Some((Local::Instantiate, r)) => self.instantiate = Some(InstantiateDialog::new(r)),
            Some((Local::GoToSource, r)) => {
                let link = doc.texture_set(&r.set).and_then(|s| s.layer(&r.layer)).and_then(|l| l.instance_of()).and_then(|l| doc.instance_source(l));
                reveal = link.map(|l| LayerRef::new(l.set, l.layer));
            }
            Some((Local::Rename, _)) | None => {}
        }
        if let Some(dialog) = &mut self.instantiate {
            match dialog.show(ui.ctx(), doc) {
                Outcome::Open => {}
                Outcome::Cancelled => self.instantiate = None,
                Outcome::Chosen(sets) => {
                    out.push(LayerEvent::Instantiate { source: dialog.source().clone(), sets });
                    self.instantiate = None;
                }
            }
        }
        if let Some(c) = channel {
            self.thumbs.end(ui.ctx(), set, c);
        }
        ui.separator();
        let focus = self.selection.focus().and_then(|id| set.layer(id).map(|l| (LayerRef::new(set.id.clone(), id), l)));
        bars::buttons(ui, &set.id, focus.as_ref().map(|(r, l)| (r, *l)), self.selection.ids(), &mut out);
        if let Some(s) = new_set {
            self.set = Some(s);
            self.selection.clear();
        }
        if new_channel.is_some() {
            self.channel = new_channel;
        }
        if let Some(r) = reveal
            && let Some(target) = doc.texture_set(&r.set)
        {
            self.reveal(target, &r.layer);
        }
        events.extend(out.into_iter().map(PanelEvent::Layers));
    }
}

impl StackDropTarget for LayerPanel {
    fn target_at(&self, doc: &Document, pos: Pos2, payload: &DropPayload) -> Option<DropTarget> {
        let set = self.current_set(doc)?;
        let rows = Rows::build(set, &self.folding);
        let landing = self.list.landing_at(&rows, pos)?;
        materialcraft::paint::edit::ops::accepts(doc, &landing.target, payload).then_some(landing.target)
    }

    fn hover(&mut self, payload: Option<DropPayload>) {
        self.external = payload;
    }
}
