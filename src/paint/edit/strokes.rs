//! Stroke-record edits: a painted stroke is appended to the record of the pixels it painted, so the record keeps
//! describing every stroke behind them (imported strokes stay as they were, ours follow).

use super::tree::content_layer_mut;
use super::{Edit, EditError, LayerRef};
use crate::paint::doc::Document;
use crate::paint::stroke::{Stroke, StrokeData, StrokeOwner, StrokeRecord};

fn slot<'a>(doc: &'a mut Document, r: &LayerRef, owner: StrokeOwner) -> Result<&'a mut Option<StrokeData>, EditError> {
    content_layer_mut(doc, r)?.strokes_mut(owner).ok_or(EditError::NotAccepted("that layer has no paint pixels to record a stroke for"))
}

pub(super) fn append(doc: &mut Document, layer: LayerRef, owner: StrokeOwner, stroke: Stroke) -> Result<Edit, EditError> {
    let slot = slot(doc, &layer, owner)?;
    let created = match slot {
        None => {
            *slot = Some(StrokeData::Record(StrokeRecord { strokes: vec![stroke] }));
            true
        }
        Some(StrokeData::Record(r)) => {
            r.strokes.push(stroke);
            false
        }
        Some(StrokeData::Unreadable(_)) => return Err(EditError::NotAccepted("the stroke record of those pixels can't be read, so no stroke can be added")),
    };
    Ok(Edit::RemoveLastStroke { layer, owner, drop_record: created })
}

pub(super) fn remove_last(doc: &mut Document, layer: LayerRef, owner: StrokeOwner, drop_record: bool) -> Result<Edit, EditError> {
    let slot = slot(doc, &layer, owner)?;
    let Some(StrokeData::Record(r)) = slot else {
        return Err(EditError::NotAccepted("there is no stroke to remove"));
    };
    let stroke = r.strokes.pop().ok_or(EditError::NotAccepted("there is no stroke to remove"))?;
    if drop_record && r.strokes.is_empty() {
        *slot = None;
    }
    Ok(Edit::AppendStroke { layer, owner, stroke: Box::new(stroke) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::stroke::{Camera, StrokeSample, StrokeTool, Symmetry, UnreadableStrokes, ViewKind};
    use crate::paint::{Layer, LayerKind, PaintContent, TextureSet};

    fn stroke(n: usize) -> Stroke {
        Stroke {
            tool: StrokeTool::Paint,
            preset: String::new(),
            view: ViewKind::Uv,
            camera: Camera::default(),
            seed: 3,
            symmetry: Symmetry::default(),
            samples: (0..n).map(|i| StrokeSample::at([i as f32, 0.0], 0.0)).collect(),
        }
    }

    fn doc() -> Document {
        let mut d = Document::default();
        let mut set = TextureSet::new("s", "S", (4, 4));
        set.layers.push(Layer::new("l", "L", LayerKind::Paint(PaintContent::default())));
        d.texture_sets.push(set);
        d
    }

    fn record(d: &Document) -> Option<StrokeData> {
        match &d.texture_sets[0].layers[0].kind {
            LayerKind::Paint(p) => p.strokes.clone(),
            _ => None,
        }
    }

    #[test]
    fn appended_strokes_undo_back_to_no_record_and_redo() {
        let mut d = doc();
        let r = LayerRef::new("s", "l");
        let undo1 = Edit::AppendStroke { layer: r.clone(), owner: StrokeOwner::Paint, stroke: Box::new(stroke(2)) }.apply(&mut d).unwrap();
        let undo2 = Edit::AppendStroke { layer: r.clone(), owner: StrokeOwner::Paint, stroke: Box::new(stroke(5)) }.apply(&mut d).unwrap();
        assert_eq!(record(&d).and_then(|s| s.record().map(|r| r.strokes.len())), Some(2));
        let redo2 = undo2.apply(&mut d).unwrap();
        let redo1 = undo1.apply(&mut d).unwrap();
        assert_eq!(record(&d), None, "the record did not exist before the first stroke");
        redo1.apply(&mut d).unwrap();
        redo2.apply(&mut d).unwrap();
        assert_eq!(record(&d).and_then(|s| s.record().map(|r| r.sample_count())), Some(7));
    }

    #[test]
    fn unreadable_records_and_missing_slots_refuse_strokes() {
        let mut d = doc();
        if let LayerKind::Paint(p) = &mut d.texture_sets[0].layers[0].kind {
            p.strokes = Some(StrokeData::Unreadable(UnreadableStrokes { bytes: vec![1], reason: "newer".into() }));
        }
        let r = LayerRef::new("s", "l");
        let before = d.clone();
        assert!(Edit::AppendStroke { layer: r.clone(), owner: StrokeOwner::Paint, stroke: Box::new(stroke(1)) }.apply(&mut d).is_err());
        assert!(Edit::AppendStroke { layer: r.clone(), owner: StrokeOwner::Mask { effect: 0 }, stroke: Box::new(stroke(1)) }.apply(&mut d).is_err());
        assert!(Edit::RemoveLastStroke { layer: r, owner: StrokeOwner::Paint, drop_record: false }.apply(&mut d).is_err());
        assert_eq!(d, before);
    }

    #[test]
    fn a_locked_layer_refuses_strokes() {
        let mut d = doc();
        d.texture_sets[0].layers[0].lock = Some(crate::paint::LayerLock::new("import", Vec::new()));
        let r = LayerRef::new("s", "l");
        let before = d.clone();
        let refused = Edit::AppendStroke { layer: r, owner: StrokeOwner::Paint, stroke: Box::new(stroke(1)) }.apply(&mut d);
        assert!(matches!(refused, Err(EditError::Locked { .. })), "{refused:?}");
        assert_eq!(d, before);
    }
}
