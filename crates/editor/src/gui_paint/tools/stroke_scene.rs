//! [`StrokeScene`]: what a stroke stamped on its own thread paints through, owned (the set's surface, the picking
//! scene and the mesh it was built from, the camera at pen down). Strokes through the quick mask, a stencil or a
//! projection image are stamped in place instead (those borrow the screen's images every frame).

use super::surfaces::Surface;
use materialcraft::camera::Camera;
use materialcraft::paint::Document;
use materialcraft::projection::{ViewProjector, ViewScene};
use materialcraft_brush::{SessionScene, WorkerScene};
use std::sync::Arc;

pub struct StrokeScene {
    pub surface: Arc<Surface>,
    /// The 3D view: the picking scene, the document copy holding the mesh it was built from, the camera. `None`
    /// for a 2D-view stroke.
    pub view: Option<(Arc<ViewScene>, Arc<Document>, Camera)>,
}

impl WorkerScene for StrokeScene {
    fn with<R>(&self, f: impl FnOnce(&SessionScene) -> R) -> R {
        let projector = self.view.as_ref().and_then(|(scene, doc, camera)| scene.projector(&doc.mesh, *camera));
        let view = projector.as_ref().map(|p| p as &dyn ViewProjector);
        f(&SessionScene { surface: &self.surface.map, index: &self.surface.index, projector: view, factors: &[], projected: None })
    }
}
