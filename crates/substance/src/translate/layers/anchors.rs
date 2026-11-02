//! [`AnchorTable`]: how exactly what each anchor of a set holds can be computed, so the effects that read an anchor
//! (reference fills in content and masks) inherit it. An anchor in a layer's content holds that content up to the
//! anchor (every channel counts), one in a mask the mask up to the anchor. Anchors read through other anchors are
//! resolved in a second pass; deeper chains stay not computable.

use super::computability::Computability;
use super::content::ContentEvaluator;
use super::effect_data::AnchorIds;
use super::mask::MaskEvaluator;
use crate::blockers::Blocker;
use crate::model::{LayerKind, PainterEffect, PainterLayer, PainterMask};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnchorTable {
    status: BTreeMap<u64, Computability>,
}

impl AnchorTable {
    /// The status of the anchor with `uid`; not computable when the set has no such anchor.
    pub fn status(&self, uid: Option<u64>) -> Computability {
        uid.and_then(|u| self.status.get(&u).cloned()).unwrap_or_else(|| {
            Computability::not_computable(Blocker::feature(
                "missing-anchor",
                "reference to an anchor that is not in the set",
            ))
        })
    }

    /// Every anchor of `layers` (the whole tree) with the status of what it holds. `content` and `masks` evaluate
    /// with the table of the previous pass.
    pub fn measure(layers: &[PainterLayer], content: &ContentEvaluator, masks: &MaskEvaluator) -> AnchorTable {
        let mut table = AnchorTable::default();
        table.walk(layers, content, masks);
        table
    }

    fn walk(&mut self, layers: &[PainterLayer], content: &ContentEvaluator, masks: &MaskEvaluator) {
        for l in layers {
            for (i, e) in l.content.iter().enumerate() {
                if let PainterEffect::Anchor(h) = e
                    && let Some(uid) = h.uid
                {
                    let mut status = Computability::Exact;
                    for c in content.evaluate(l.content.get(..i).unwrap_or_default()) {
                        status.and(&c.status);
                    }
                    self.status.insert(uid, status);
                }
            }
            if let Some(mask) = &l.mask {
                for (i, e) in mask.effects.iter().enumerate() {
                    if let PainterEffect::Anchor(h) = e
                        && let Some(uid) = h.uid
                    {
                        let below = PainterMask {
                            enabled: true,
                            effects: mask.effects.get(..i).unwrap_or_default().to_vec(),
                            ..mask.clone()
                        };
                        let status = masks.evaluate(Some(&below), &mut AnchorIds::default()).status;
                        self.status.insert(uid, status);
                    }
                }
            }
            if let LayerKind::Group(children) = &l.kind {
                self.walk(children, content, masks);
            }
        }
    }
}
