//! Layer tree edits: insert, remove, move, and property changes, plus the lookups the other edits share.

use super::{Edit, EditError, LayerProperty, LayerRef, LayerSlot};
use crate::paint::doc::Document;
use crate::paint::layer::{Layer, LayerKind, LayerWalk};
use crate::paint::texture_set::TextureSet;
use std::mem::replace;

pub(super) fn set_mut<'a>(doc: &'a mut Document, set: &str) -> Result<&'a mut TextureSet, EditError> {
    doc.texture_set_mut(set).ok_or_else(|| EditError::NoTextureSet(set.to_string()))
}

pub(super) fn layer_mut<'a>(doc: &'a mut Document, r: &LayerRef) -> Result<&'a mut Layer, EditError> {
    set_mut(doc, &r.set)?.layer_mut(&r.layer).ok_or_else(|| no_layer(r))
}

/// [`layer_mut`] for changing the layer's parameters: refused on an instance, whose parameters are its source's (IN-02).
pub(super) fn own_layer_mut<'a>(doc: &'a mut Document, r: &LayerRef) -> Result<&'a mut Layer, EditError> {
    let l = layer_mut(doc, r)?;
    if matches!(l.kind, LayerKind::Instance(_)) {
        return Err(EditError::InstanceReadOnly { set: r.set.clone(), layer: r.layer.clone() });
    }
    Ok(l)
}

/// [`own_layer_mut`] for changing the layer's content (its base, effects, masks, pixels and strokes): also refused on a
/// locked layer (see [`crate::paint::lock`]).
pub(super) fn content_layer_mut<'a>(doc: &'a mut Document, r: &LayerRef) -> Result<&'a mut Layer, EditError> {
    let l = own_layer_mut(doc, r)?;
    if let Some(lock) = &l.lock {
        return Err(EditError::Locked { set: r.set.clone(), layer: r.layer.clone(), why: lock.summary() });
    }
    Ok(l)
}

/// The first instance in the subtree of layer `r` that depends on itself.
fn cycle_in(doc: &Document, r: &LayerRef) -> Option<String> {
    let set = doc.texture_set(&r.set)?;
    let top = set.layer(&r.layer)?;
    LayerWalk::new(std::slice::from_ref(top))
        .filter(|(_, l)| matches!(l.kind, LayerKind::Instance(_)))
        .find(|(_, l)| doc.instance_on_cycle(&r.set, &l.id))
        .map(|(_, l)| l.id.clone())
}

fn no_layer(r: &LayerRef) -> EditError {
    EditError::NoLayer { set: r.set.clone(), layer: r.layer.clone() }
}

/// The children list a slot points into.
fn children_mut<'a>(set: &'a mut TextureSet, parent: Option<&str>) -> Result<&'a mut Vec<Layer>, EditError> {
    let Some(p) = parent else {
        return Ok(&mut set.layers);
    };
    let sid = set.id.clone();
    let layer = set.layer_mut(p).ok_or_else(|| EditError::NoLayer { set: sid.clone(), layer: p.to_string() })?;
    match &mut layer.kind {
        LayerKind::Group(children) => Ok(children),
        _ => Err(EditError::NotAGroup { set: sid, layer: p.to_string() }),
    }
}

/// Where layer `id` sits.
pub(super) fn locate(layers: &[Layer], id: &str, parent: Option<&str>) -> Option<LayerSlot> {
    for (index, l) in layers.iter().enumerate() {
        if l.id == id {
            return Some(LayerSlot { parent: parent.map(str::to_string), index });
        }
        if let Some(found) = locate(l.children(), id, Some(&l.id)) {
            return Some(found);
        }
    }
    None
}

fn check_insert_index(index: usize, len: usize) -> Result<(), EditError> {
    if index > len { Err(EditError::IndexOutOfRange { index, len }) } else { Ok(()) }
}

pub(super) fn insert(doc: &mut Document, set_id: &str, at: LayerSlot, layer: Layer) -> Result<Edit, EditError> {
    let set = set_mut(doc, set_id)?;
    let mut new_ids: Vec<&str> = Vec::new();
    for (_, l) in LayerWalk::new(std::slice::from_ref(&layer)) {
        if l.id.is_empty() || new_ids.contains(&l.id.as_str()) || set.layer(&l.id).is_some() {
            return Err(EditError::DuplicateId { set: set_id.to_string(), id: l.id.clone() });
        }
        new_ids.push(&l.id);
    }
    let children = children_mut(set, at.parent.as_deref())?;
    check_insert_index(at.index, children.len())?;
    let id = layer.id.clone();
    children.insert(at.index, layer);
    let r = LayerRef::new(set_id, id);
    if let Some(looping) = cycle_in(doc, &r) {
        remove(doc, r)?;
        return Err(EditError::InstanceCycle { set: set_id.to_string(), layer: looping });
    }
    Ok(Edit::RemoveLayer(r))
}

/// Takes the layer out of its tree; returns it with the slot it had.
fn take(set: &mut TextureSet, r: &LayerRef) -> Result<(Layer, LayerSlot), EditError> {
    let slot = locate(&set.layers, &r.layer, None).ok_or_else(|| no_layer(r))?;
    let children = children_mut(set, slot.parent.as_deref())?;
    if slot.index >= children.len() {
        return Err(no_layer(r));
    }
    Ok((children.remove(slot.index), slot))
}

pub(super) fn remove(doc: &mut Document, r: LayerRef) -> Result<Edit, EditError> {
    let (layer, at) = take(set_mut(doc, &r.set)?, &r)?;
    Ok(Edit::InsertLayer { set: r.set, at, layer: Box::new(layer) })
}

pub(super) fn move_to(doc: &mut Document, r: LayerRef, to: LayerSlot) -> Result<Edit, EditError> {
    let set = set_mut(doc, &r.set)?;
    let from = locate(&set.layers, &r.layer, None).ok_or_else(|| no_layer(&r))?;
    // Check everything before taking the layer out, so a failed move leaves the tree as it was.
    if let Some(p) = &to.parent
        && set.layer(&r.layer).is_some_and(|l| LayerWalk::new(std::slice::from_ref(l)).any(|(_, d)| &d.id == p))
    {
        return Err(EditError::IntoItself { set: r.set.clone(), layer: r.layer.clone() });
    }
    let len = children_mut(set, to.parent.as_deref())?.len().saturating_sub(usize::from(from.parent == to.parent));
    check_insert_index(to.index, len)?;
    let (layer, from) = take(set, &r)?;
    children_mut(set, to.parent.as_deref())?.insert(to.index, layer);
    if let Some(looping) = cycle_in(doc, &r) {
        // Put it back where it was (no checks: that is the state before the edit).
        let set = set_mut(doc, &r.set)?;
        let (layer, _) = take(set, &r)?;
        let children = children_mut(set, from.parent.as_deref())?;
        check_insert_index(from.index, children.len())?;
        children.insert(from.index, layer);
        return Err(EditError::InstanceCycle { set: r.set, layer: looping });
    }
    Ok(Edit::MoveLayer { layer: r, to: from })
}

pub(super) fn set_property(doc: &mut Document, r: LayerRef, property: LayerProperty) -> Result<Edit, EditError> {
    let l = match property {
        LayerProperty::Name(_) | LayerProperty::Visible(_) | LayerProperty::Lock(_) => layer_mut(doc, &r)?,
        LayerProperty::Opacity(_) | LayerProperty::Blend(_) | LayerProperty::MaskEnabled(_) => own_layer_mut(doc, &r)?,
        LayerProperty::Mask(_) | LayerProperty::MaskInitial(_) | LayerProperty::GeometryMask(_) | LayerProperty::Fill(_) => content_layer_mut(doc, &r)?,
    };
    let old = match property {
        LayerProperty::Name(v) => LayerProperty::Name(replace(&mut l.name, v)),
        LayerProperty::Visible(v) => LayerProperty::Visible(replace(&mut l.visible, v)),
        LayerProperty::Opacity(v) => LayerProperty::Opacity(replace(&mut l.opacity, v)),
        LayerProperty::Blend(v) => LayerProperty::Blend(replace(&mut l.blend, v)),
        LayerProperty::Mask(v) => LayerProperty::Mask(replace(&mut l.mask, v)),
        LayerProperty::MaskEnabled(v) => {
            let mask = l.mask.as_mut().ok_or_else(|| EditError::NoMask { set: r.set.clone(), layer: r.layer.clone() })?;
            LayerProperty::MaskEnabled(replace(&mut mask.enabled, v))
        }
        LayerProperty::MaskInitial(v) => {
            let mask = l.mask.as_mut().ok_or_else(|| EditError::NoMask { set: r.set.clone(), layer: r.layer.clone() })?;
            LayerProperty::MaskInitial(replace(&mut mask.initial, v))
        }
        LayerProperty::GeometryMask(v) => LayerProperty::GeometryMask(replace(&mut l.geometry_mask, v)),
        LayerProperty::Lock(v) => LayerProperty::Lock(replace(&mut l.lock, v)),
        LayerProperty::Fill(v) => match &mut l.kind {
            LayerKind::Fill(f) => LayerProperty::Fill(replace(f, v)),
            _ => return Err(EditError::WrongLayerKind { set: r.set.clone(), layer: r.layer.clone() }),
        },
    };
    Ok(Edit::SetLayer { layer: r, property: old })
}
