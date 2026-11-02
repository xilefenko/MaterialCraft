//! Effect stack edits on a layer's content stack or its mask stack.

use super::tree::content_layer_mut;
use super::{Edit, EditError, LayerRef, StackEffect, StackId};
use crate::paint::doc::Document;
use crate::paint::effect::{ContentEffect, MaskEffect};

/// One of a layer's two stacks, borrowed for an edit.
enum Stack<'a> {
    Content(&'a mut Vec<ContentEffect>),
    Mask(&'a mut Vec<MaskEffect>),
}

fn stack<'a>(doc: &'a mut Document, r: &LayerRef, which: StackId) -> Result<Stack<'a>, EditError> {
    let layer = content_layer_mut(doc, r)?;
    match which {
        StackId::Content => Ok(Stack::Content(&mut layer.effects)),
        StackId::Mask => match &mut layer.mask {
            Some(m) => Ok(Stack::Mask(&mut m.effects)),
            None => Err(EditError::NoMask { set: r.set.clone(), layer: r.layer.clone() }),
        },
    }
}

fn out_of_range<T>(index: usize, v: &[T]) -> EditError {
    EditError::IndexOutOfRange { index, len: v.len() }
}

fn insert_at<T>(v: &mut Vec<T>, index: usize, item: T) -> Result<(), EditError> {
    if index > v.len() {
        return Err(out_of_range(index, v));
    }
    v.insert(index, item);
    Ok(())
}

fn remove_at<T>(v: &mut Vec<T>, index: usize) -> Result<T, EditError> {
    if index >= v.len() {
        return Err(out_of_range(index, v));
    }
    Ok(v.remove(index))
}

fn replace_at<T>(v: &mut [T], index: usize, item: T) -> Result<T, EditError> {
    match v.get_mut(index) {
        Some(slot) => Ok(std::mem::replace(slot, item)),
        None => Err(out_of_range(index, v)),
    }
}

fn move_within<T>(v: &mut Vec<T>, from: usize, to: usize) -> Result<(), EditError> {
    if from >= v.len() {
        return Err(out_of_range(from, v));
    }
    if to >= v.len() {
        return Err(out_of_range(to, v));
    }
    let item = v.remove(from);
    v.insert(to, item);
    Ok(())
}

pub(super) fn insert(doc: &mut Document, r: LayerRef, index: usize, effect: StackEffect) -> Result<Edit, EditError> {
    let which = effect.stack();
    match (stack(doc, &r, which)?, effect) {
        (Stack::Content(v), StackEffect::Content(e)) => insert_at(v, index, e)?,
        (Stack::Mask(v), StackEffect::Mask(e)) => insert_at(v, index, e)?,
        _ => return Err(EditError::WrongStack(which)),
    }
    Ok(Edit::RemoveEffect { layer: r, stack: which, index })
}

pub(super) fn remove(doc: &mut Document, r: LayerRef, which: StackId, index: usize) -> Result<Edit, EditError> {
    let effect = match stack(doc, &r, which)? {
        Stack::Content(v) => StackEffect::Content(remove_at(v, index)?),
        Stack::Mask(v) => StackEffect::Mask(remove_at(v, index)?),
    };
    Ok(Edit::InsertEffect { layer: r, index, effect })
}

pub(super) fn replace(doc: &mut Document, r: LayerRef, index: usize, effect: StackEffect) -> Result<Edit, EditError> {
    let which = effect.stack();
    let old = match (stack(doc, &r, which)?, effect) {
        (Stack::Content(v), StackEffect::Content(e)) => StackEffect::Content(replace_at(v, index, e)?),
        (Stack::Mask(v), StackEffect::Mask(e)) => StackEffect::Mask(replace_at(v, index, e)?),
        _ => return Err(EditError::WrongStack(which)),
    };
    Ok(Edit::ReplaceEffect { layer: r, index, effect: old })
}

pub(super) fn move_to(doc: &mut Document, r: LayerRef, which: StackId, from: usize, to: usize) -> Result<Edit, EditError> {
    match stack(doc, &r, which)? {
        Stack::Content(v) => move_within(v, from, to)?,
        Stack::Mask(v) => move_within(v, from, to)?,
    }
    Ok(Edit::MoveEffect { layer: r, stack: which, from: to, to: from })
}
