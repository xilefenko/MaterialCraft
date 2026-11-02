//! Turns the layer panel's [`LayerEvent`]s into history steps on the open document: each event is one
//! `paint::edit::ops` operation inside [`OpenDocument::edit`], so it can be undone as a whole.

use super::document_state::OpenDocument;
use super::instance_reach::Reach;
use super::panels::layers::LayerEvent;
use materialcraft::paint::edit::ops::{self, DropOutcome};
use materialcraft::paint::edit::{Edit, EditError, LayerProperty, LayerRef, Transaction};
use materialcraft::paint::{BlendMode, Channel, Layer};

/// What an applied event changed.
#[derive(Debug, Default, PartialEq)]
pub struct Applied {
    /// Index of the texture set whose stack changed (its maps need recompositing).
    pub set: Option<usize>,
    /// Other sets the change reaches through layer instances (IN-13); they need recompositing too.
    pub also: Vec<usize>,
    /// Layers to select afterwards (new layers, copies, a new folder).
    pub select: Option<Vec<String>>,
}

/// Applies `ev` to `doc` as one history step. `Isolate`, `EditGeometryMask` (view state) and `SaveAsset` (a file) are not
/// handled here.
pub fn apply(doc: &mut OpenDocument, ev: LayerEvent) -> Result<Applied, EditError> {
    let set_id = match set_of(&ev) {
        Some(s) => s.to_string(),
        None => {
            if ev == LayerEvent::EndGesture {
                doc.close_step();
            }
            return Ok(Applied::default());
        }
    };
    let mut reach = Reach::before(doc.doc(), &ev);
    let select = run(doc, ev.clone())?;
    reach.after(doc.doc(), &ev, select.as_deref().unwrap_or_default());
    let set = doc.doc().texture_sets.iter().position(|s| s.id == set_id);
    let also = reach.also(doc.doc(), set);
    Ok(Applied { set, also, select })
}

fn set_of(ev: &LayerEvent) -> Option<&str> {
    use LayerEvent::*;
    Some(match ev {
        Create { set, .. } | Duplicate { set, .. } | Delete { set, .. } | Group { set, .. } | Move { set, .. } => set,
        Rename(r, _) | SetVisible(r, _) | Unlock(r) | AddMask(r, _) | RemoveMask(r) | SetMaskEnabled(r, _) | InvertMask(r) | SetGeometryMask(r, _) => &r.set,
        SetOpacity { layer, .. } | SetBlendMode { layer, .. } | AddEffect { layer, .. } | RemoveEffect { layer, .. } | SetEffectEnabled { layer, .. } => {
            &layer.set
        }
        Drop { target, .. } => target.set(),
        Instantiate { source, .. } => &source.set,
        PasteInstance { set, .. } => set,
        EndGesture | Isolate(_) | EditGeometryMask(_) | SaveAsset(_) => return None,
    })
}

fn run(doc: &mut OpenDocument, ev: LayerEvent) -> Result<Option<Vec<String>>, EditError> {
    use LayerEvent::*;
    let none = |r: Result<(), EditError>| r.map(|_| None);
    match ev {
        Create { set, what, above } => doc.edit(create_label(what), None, |tx| ops::create(tx, &set, what, above.as_deref()).map(|id| Some(vec![id]))),
        Duplicate { set, ids } => doc.edit("Duplicate", None, |tx| ops::duplicate(tx, &set, &ids).map(Some)),
        Delete { set, ids } => none(doc.edit("Delete", None, |tx| ops::delete(tx, &set, &ids))),
        Group { set, ids } => doc.edit("Group into folder", None, |tx| ops::group(tx, &set, &ids).map(|id| Some(vec![id]))),
        Move { set, ids, to, copy: false } => none(doc.edit("Move layers", None, |tx| ops::move_layers(tx, &set, &ids, to))),
        Move { set, ids, to, copy: true } => doc.edit("Duplicate and place", None, |tx| ops::duplicate_to(tx, &set, &ids, to).map(Some)),
        Rename(r, name) => none(doc.edit("Rename", None, |tx| tx.apply(Edit::SetLayer { layer: r, property: LayerProperty::Name(name) }))),
        SetVisible(r, v) => {
            none(doc.edit(if v { "Show layer" } else { "Hide layer" }, None, |tx| tx.apply(Edit::SetLayer { layer: r, property: LayerProperty::Visible(v) })))
        }
        Unlock(r) => none(doc.edit("Unlock layer", None, |tx| tx.apply(Edit::SetLayer { layer: r, property: LayerProperty::Lock(None) }))),
        SetOpacity { layer, channel, opacity, key } => none(doc.edit("Opacity", Some(key), |tx| set_blend(tx, &layer, channel, None, Some(opacity)))),
        SetBlendMode { layer, channel, mode } => none(doc.edit("Blend mode", None, |tx| set_blend(tx, &layer, channel, Some(mode), None))),
        AddEffect { layer, stack, what } => none(doc.edit("Add effect", None, |tx| {
            let set = tx.doc().texture_set(&layer.set).ok_or_else(|| EditError::NoTextureSet(layer.set.clone()))?;
            let effect = ops::new_effect(set, what, stack);
            ops::add_effect(tx, &layer, effect, None).map(|_| ())
        })),
        RemoveEffect { layer, stack, index } => none(doc.edit("Delete effect", None, |tx| tx.apply(Edit::RemoveEffect { layer, stack, index }))),
        SetEffectEnabled { layer, stack, index, enabled } => {
            none(doc.edit(if enabled { "Enable effect" } else { "Disable effect" }, None, |tx| ops::set_effect_enabled(tx, &layer, stack, index, enabled)))
        }
        AddMask(r, start) => none(doc.edit("Add mask", None, |tx| ops::add_mask(tx, &r, start))),
        RemoveMask(r) => none(doc.edit("Remove mask", None, |tx| ops::remove_mask(tx, &r))),
        SetMaskEnabled(r, on) => none(doc.edit(if on { "Enable mask" } else { "Disable mask" }, None, |tx| ops::set_mask_enabled(tx, &r, on))),
        InvertMask(r) => none(doc.edit("Invert mask", None, |tx| ops::invert_mask(tx, &r))),
        SetGeometryMask(r, mask) => {
            none(doc.edit("Geometry mask", None, |tx| tx.apply(Edit::SetLayer { layer: r, property: LayerProperty::GeometryMask(mask) })))
        }
        Drop { target, payload, replace } => doc.edit("Drop asset", None, |tx| {
            ops::drop_onto(tx, &target, &payload, replace).map(|out| match out {
                DropOutcome::NewLayer(id) => Some(vec![id]),
                DropOutcome::Filled(r) => Some(vec![r.layer]),
                DropOutcome::Effects { .. } => None,
            })
        }),
        Instantiate { source, sets } => none(doc.edit("Instantiate across texture sets", None, |tx| ops::instantiate(tx, &source, &sets).map(|_| ()))),
        PasteInstance { source, set, above } => {
            doc.edit("Paste as instance", None, |tx| ops::paste_instance(tx, &source, &set, above.as_deref()).map(|id| Some(vec![id])))
        }
        EndGesture | Isolate(_) | EditGeometryMask(_) | SaveAsset(_) => Ok(None),
    }
}

fn create_label(what: ops::NewLayer) -> &'static str {
    match what {
        ops::NewLayer::Paint => "New paint layer",
        ops::NewLayer::Fill => "New fill layer",
        ops::NewLayer::Folder => "New folder",
        ops::NewLayer::SeeThrough => "New see-through layer",
    }
}

/// Changes the mode and/or opacity of the layer's entry for `channel` (the panel's view channel).
fn set_blend(tx: &mut Transaction, r: &LayerRef, channel: Channel, mode: Option<BlendMode>, opacity: Option<f32>) -> Result<(), EditError> {
    let layer: &Layer =
        tx.doc().texture_set(&r.set).and_then(|s| s.layer(&r.layer)).ok_or_else(|| EditError::NoLayer { set: r.set.clone(), layer: r.layer.clone() })?;
    let mut blend = layer.blend.clone();
    match blend.iter_mut().find(|b| b.channel == channel) {
        Some(b) => {
            b.mode = mode.unwrap_or(b.mode);
            b.opacity = opacity.unwrap_or(b.opacity).clamp(0.0, 1.0);
        }
        None => return Err(EditError::NotAccepted("the layer does not affect this channel")),
    }
    tx.apply(Edit::SetLayer { layer: r.clone(), property: LayerProperty::Blend(blend) })
}

#[cfg(test)]
#[path = "stack_editor_tests.rs"]
mod tests;
