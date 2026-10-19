//! [`DocumentStroke`]: a stroke into a paint document, with everything the history needs to make it one undo step
//! (PT-54): the edit that prepared the target (a mask's new paint effect), the old tiles of every image it painted
//! and the stroke appended to the target's stroke record.
//!
//! The document is lent in pieces so the caller can hold a view of the mesh while the texture sets are painted:
//! [`DocumentStroke::prepare`] (whole document) → [`Prepared::start`], [`DocumentStroke::add`],
//! [`DocumentStroke::finish`] (the texture set) → [`Ended::record`] (whole document, returns the inverses).
//!
//! On see-through content ([`Prepared::see_through`]) the stroke starts from the composite below where the layer has
//! no pixels yet, and the tiles it did not touch are given back before the step is recorded, so the step holds
//! only what the stroke changed.

use crate::BrushError;
use crate::engine::InputPoint;
use crate::session::{Finished, SessionScene, StrokeSession, StrokeSetup};
use crate::target::{PaintTarget, Resolved, TargetPart, mode_for};
use materialcraft::mesh::Mesh;
use materialcraft::paint::edit::{Edit, EditError, TileCapture};
use materialcraft::paint::see_through::{fill_from, trim_to};
use materialcraft::paint::stroke::StrokeOwner;
use materialcraft::paint::{Channel, Document, LayerKind, TextureSet, TiledImage};
use materialcraft::stamp::StampMode;

/// The composite below a see-through layer, per channel (see [`materialcraft::paint::see_through`]).
pub type Base = Vec<(Channel, TiledImage)>;

/// A target made ready for painting: preparation applied, images resolved, old tiles held.
pub struct Prepared {
    target: PaintTarget,
    resolved: Resolved,
    inverses: Vec<Edit>,
    captures: Vec<TileCapture>,
    base: Option<Base>,
}

/// A stroke in progress on a document.
pub struct DocumentStroke {
    target: PaintTarget,
    resolved: Resolved,
    mode: StampMode,
    inverses: Vec<Edit>,
    captures: Vec<TileCapture>,
    base: Option<Base>,
    session: StrokeSession,
}

/// A stroke whose painting is done; [`Ended::record`] completes it.
pub struct Ended {
    target: PaintTarget,
    owner: StrokeOwner,
    inverses: Vec<Edit>,
    captures: Vec<TileCapture>,
    base: Option<Base>,
    finished: Option<Finished>,
}

impl DocumentStroke {
    /// Makes `target` ready in `doc` for painting `values`. On error `doc` is as it was.
    pub fn prepare(doc: &mut Document, target: PaintTarget, values: &crate::target::PaintValues) -> Result<Prepared, BrushError> {
        let set = doc.texture_set(&target.layer.set).ok_or(BrushError::NotPaintable("the texture set is gone"))?;
        let mut inverses = Vec::new();
        if let Some(edit) = target.preparation(set)? {
            inverses.push(edit.apply(doc).map_err(BrushError::Edit)?);
        }
        let ready = Self::resolve(doc, &target, values);
        match ready {
            Ok((resolved, captures)) => Ok(Prepared { target, resolved, inverses, captures, base: None }),
            Err(e) => {
                for inv in inverses.into_iter().rev() {
                    // putting back what was just done; it applied a moment ago, so it applies now
                    let _ = inv.apply(doc);
                }
                Err(e)
            }
        }
    }

    fn resolve(doc: &Document, target: &PaintTarget, values: &crate::target::PaintValues) -> Result<(Resolved, Vec<TileCapture>), BrushError> {
        let set = doc.texture_set(&target.layer.set).ok_or(BrushError::NotPaintable("the texture set is gone"))?;
        let resolved = target.resolve(set, values)?;
        if resolved.images.is_empty() {
            return Err(BrushError::NotPaintable("no channel the layer has is switched on"));
        }
        let captures =
            resolved.images.iter().map(|i| TileCapture::begin(doc, i.image.clone())).collect::<Result<Vec<_>, EditError>>().map_err(BrushError::Edit)?;
        Ok((resolved, captures))
    }

    /// Gives up the stroke without painting more; [`Ended::record`] still records what was painted (without a
    /// stroke record entry).
    pub fn abandon(self) -> Ended {
        let DocumentStroke { target, resolved, inverses, captures, base, .. } = self;
        Ended { owner: resolved.owner, target, inverses, captures, base, finished: None }
    }

    /// Builder, before the first [`DocumentStroke::add`]: paint with `extras` (an alpha-image tip,
    /// [`StrokeSession::with_extras`]).
    pub fn with_extras(self, extras: crate::engine::EngineExtras) -> DocumentStroke {
        DocumentStroke { session: self.session.with_extras(extras), ..self }
    }

    /// Paints `points` (see [`StrokeSession::add`]); returns the changed tiles of the set's images.
    pub fn add(&mut self, scene: &SessionScene, points: &[InputPoint], set: &mut TextureSet) -> Result<Vec<(u32, u32)>, BrushError> {
        let mut targets = self.target.projected_targets(set, &self.resolved, self.mode, scene.projected)?;
        self.session.add(scene, points, &mut targets)
    }

    /// Paints the stroke's last dabs. The result's [`Ended::record`] must follow, also after an error here (the
    /// pixels painted so far stay and are recorded).
    pub fn finish(self, scene: &SessionScene, set: &mut TextureSet) -> (Ended, Result<Vec<(u32, u32)>, BrushError>) {
        let DocumentStroke { target, resolved, mode, inverses, captures, base, session } = self;
        let result = target.projected_targets(set, &resolved, mode, scene.projected).and_then(|mut t| session.finish(scene, &mut t));
        let (finished, out) = match result {
            Ok(f) => {
                let touched = f.touched.clone();
                (Some(f), Ok(touched))
            }
            Err(e) => (None, Err(e)),
        };
        (Ended { owner: resolved.owner, target, inverses, captures, base, finished }, out)
    }

    /// Dabs placed so far.
    pub fn dabs(&self) -> usize {
        self.session.dabs()
    }

    /// Where the stroke's images sit in its texture set.
    pub(crate) fn slots(&self) -> crate::worker::TargetSlots {
        crate::worker::TargetSlots { target: self.target.clone(), resolved: self.resolved.clone(), mode: self.mode }
    }

    /// The stroke's camera (3D-view strokes).
    pub fn camera(&self) -> Option<materialcraft::camera::Camera> {
        self.session.camera()
    }
}

impl Prepared {
    /// Starts painting with `setup` through `scene`; `mesh` and `shown` draw the occlusion depth.
    pub fn start(
        self,
        setup: StrokeSetup,
        scene: &SessionScene,
        mesh: &Mesh,
        shown: &dyn Fn(usize) -> bool,
        set: &mut TextureSet,
    ) -> Result<DocumentStroke, Box<(Ended, BrushError)>> {
        let mode = mode_for(setup.tool, &setup.retouch);
        let Prepared { target, resolved, inverses, captures, base } = self;
        let session = target.projected_targets(set, &resolved, mode, scene.projected).and_then(|t| StrokeSession::begin(setup, scene, mesh, shown, &t));
        match session {
            Ok(session) => Ok(DocumentStroke { target, resolved, mode, inverses, captures, base, session }),
            Err(e) => Err(Box::new((Ended { owner: resolved.owner, target, inverses, captures, base, finished: None }, e))),
        }
    }

    /// What the stroke resolved to (images painted, channels the layer lacks).
    pub fn resolved(&self) -> &Resolved {
        &self.resolved
    }

    /// True if preparing the target changed the document beyond pixels (a mask got a paint effect, a see-through
    /// layer took the composite below it): the composite may have changed anywhere, not only where the stroke paints.
    pub fn changed_document(&self) -> bool {
        !self.inverses.is_empty() || self.base.is_some()
    }

    /// The target is see-through content and `base` the composite below it
    /// ([`materialcraft::paint::see_through::base_images`], taken after the layer was brought up to date): the
    /// layer's images take the base's tiles where they have none, so the stroke works on the composite below.
    pub fn see_through(&mut self, set: &mut TextureSet, base: Base) {
        if self.target.part != TargetPart::Content {
            return;
        }
        if let Some(LayerKind::Paint(p)) = set.layer_mut(&self.target.layer.layer).map(|l| &mut l.kind) {
            for (c, img) in p.images.iter_mut() {
                if let Some((_, b)) = base.iter().find(|(bc, _)| bc == c) {
                    fill_from(img, b);
                }
            }
        }
        self.base = Some(base);
    }
}

impl Ended {
    /// Appends the stroke to the target's stroke record and returns every inverse in the order the edits were made,
    /// for one history step. A record that can't take the stroke (unreadable) is left as it is; the error comes
    /// back with the inverses so the pixels are still undoable.
    pub fn record(self, doc: &mut Document) -> (Vec<Edit>, Option<EditError>) {
        let Ended { target, owner, mut inverses, captures, base, finished } = self;
        if let Some(base) = &base {
            give_back_untouched(doc, &target, base);
        }
        let mut problem = None;
        for c in captures {
            match c.finish(doc) {
                Ok(Edit::SwapTiles { tiles, .. }) if tiles.is_empty() => {}
                Ok(inv) => inverses.push(inv),
                Err(e) => problem = Some(e),
            }
        }
        if let Some(f) = finished.filter(|f| f.dabs > 0) {
            match (Edit::AppendStroke { layer: target.layer.clone(), owner, stroke: Box::new(f.stroke) }).apply(doc) {
                Ok(inv) => inverses.push(inv),
                Err(e) => problem = Some(e),
            }
        }
        (inverses, problem)
    }
}

/// Frees the see-through layer's tiles that still hold the base (no stroke wrote them).
fn give_back_untouched(doc: &mut Document, target: &PaintTarget, base: &Base) {
    let Some(set) = doc.texture_sets.iter_mut().find(|s| s.id == target.layer.set) else { return };
    let Some(LayerKind::Paint(p)) = set.layer_mut(&target.layer.layer).map(|l| &mut l.kind) else { return };
    for (c, img) in p.images.iter_mut() {
        if let Some((_, b)) = base.iter().find(|(bc, _)| bc == c) {
            trim_to(img, b);
        }
    }
}
