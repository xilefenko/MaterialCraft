//! [`ViewPicking`]: keeps the CPU view projector's per-mesh half ([`ViewScene`]: a BVH per mesh part) in step with
//! the open document, building it on a background thread, so tools can map between the 3D view and the mesh
//! every frame ([`ViewPicking::projector`]).

use super::document_state::OpenDocument;
use super::job::Job;
use materialcraft::camera::Camera;
use materialcraft::mesh::Mesh;
use materialcraft::paint::Document;
use materialcraft::projection::{CpuViewProjector, ViewScene};
use std::path::PathBuf;
use std::sync::Arc;

/// What a scene was built for: the document's file, the mesh's shape (part, vertex and triangle counts) and which
/// texture sets are shown.
#[derive(Clone, PartialEq, Debug)]
struct SceneKey {
    source: PathBuf,
    triangles: Vec<usize>,
    vertices: Vec<usize>,
    visible: Vec<bool>,
}

impl SceneKey {
    fn of(doc: &OpenDocument) -> SceneKey {
        let parts = &doc.doc().mesh.parts;
        SceneKey {
            source: doc.source().to_path_buf(),
            triangles: parts.iter().map(|p| p.triangles.len()).collect(),
            vertices: parts.iter().map(|p| p.positions.len()).collect(),
            visible: doc.visibility(),
        }
    }
}

#[derive(Default)]
pub struct ViewPicking {
    /// Shared with strokes in flight on their own thread ([`ViewPicking::shared`]).
    scene: Option<Arc<ViewScene>>,
    /// The mesh the scene was built from (a copy of the document's).
    built_from: Option<Arc<Document>>,
    /// Hidden parts changed while a stroke held the scene: applied once it lets go.
    hidden_pending: bool,
    /// Per mesh part: shown by its texture set's visibility (as the scene was built).
    by_sets: Vec<bool>,
    /// Mesh parts hidden on top of that (a geometry mask being edited): neither picked nor occluding.
    hidden: Vec<bool>,
    building: Option<Job<(ViewScene, Arc<Document>)>>,
    key: Option<SceneKey>,
}

impl ViewPicking {
    /// Starts a rebuild when the document's mesh or shown parts changed; takes a finished build. Call once per
    /// frame.
    pub fn poll(&mut self, doc: Option<&OpenDocument>) {
        let Some(doc) = doc else {
            *self = ViewPicking { hidden: std::mem::take(&mut self.hidden), ..ViewPicking::default() };
            return;
        };
        let key = SceneKey::of(doc);
        if self.key.as_ref() != Some(&key) {
            let (shared, visible) = (doc.mesh_view(), key.visible.clone());
            self.building = Some(Job::spawn(move |_, tx| {
                let scene = build(&shared, &visible);
                let _ = tx.send((scene, shared));
            }));
            self.scene = None;
            self.built_from = None;
            self.key = Some(key);
        }
        let mut arrived = None;
        if let Some(job) = &mut self.building {
            arrived = job.poll().pop();
            if job.is_finished() {
                self.building = None;
            }
        }
        if let Some((scene, from)) = arrived {
            self.by_sets = (0..doc.doc().mesh.parts.len()).map(|i| scene.is_shown(i)).collect();
            self.scene = Some(Arc::new(scene));
            self.built_from = Some(from);
            self.apply_hidden();
        } else if self.hidden_pending {
            self.apply_hidden();
        }
    }

    /// Hides mesh parts (`hidden[i]` for part `i`) from picking and occlusion on top of the texture sets'
    /// visibility; kept for later scenes. Cheap: no rebuild.
    pub fn set_hidden_parts(&mut self, hidden: Vec<bool>) {
        if self.hidden != hidden {
            self.hidden = hidden;
            self.apply_hidden();
        }
    }

    fn apply_hidden(&mut self) {
        let Some(arc) = &mut self.scene else { return };
        // a stroke in flight still paints through it: later
        let Some(scene) = Arc::get_mut(arc) else {
            self.hidden_pending = true;
            return;
        };
        self.hidden_pending = false;
        for (i, by_set) in self.by_sets.iter().enumerate() {
            scene.set_shown(i, *by_set && !self.hidden.get(i).copied().unwrap_or(false));
        }
    }

    /// True once the scene for the current document is built.
    pub fn is_ready(&self) -> bool {
        self.scene.is_some()
    }

    /// Screen ↔ mesh through `camera` for `doc` (the document polled last), or `None` while the scene is built.
    pub fn projector<'a>(&'a self, doc: &'a Document, camera: Camera) -> Option<CpuViewProjector<'a>> {
        self.projector_on(&doc.mesh, camera)
    }

    /// [`Self::projector`] given the document's mesh alone (a brush stroke holds the texture sets meanwhile).
    pub fn projector_on<'a>(&'a self, mesh: &'a Mesh, camera: Camera) -> Option<CpuViewProjector<'a>> {
        self.scene.as_ref()?.projector(mesh, camera)
    }

    /// The picking scene, once built (replays of recorded strokes pick through it).
    pub fn scene(&self) -> Option<&ViewScene> {
        self.scene.as_deref()
    }

    /// The scene and the mesh it was built from, to pick on another thread (a stroke stamped on its own thread).
    /// While someone holds them, hiding parts waits.
    pub fn shared(&self) -> Option<(Arc<ViewScene>, Arc<Document>)> {
        Some((self.scene.clone()?, self.built_from.clone()?))
    }

    /// False for mesh parts of hidden texture sets and for hidden parts (they neither show nor hide anything).
    pub fn is_shown(&self, part: usize) -> bool {
        self.scene.as_ref().is_some_and(|s| s.is_shown(part))
    }
}

/// The scene as the viewport shows it: placed with the model's fit, each part picking in the UV set of its texture
/// set, parts of hidden texture sets left out (as `geometry::build` does).
fn build(doc: &Document, visible: &[bool]) -> ViewScene {
    let mut scene = ViewScene::new(&doc.mesh, doc.mesh.fit());
    for (i, part) in doc.mesh.parts.iter().enumerate() {
        let set = doc.texture_sets.iter().position(|s| s.material_slot == part.material);
        if let Some(s) = set {
            scene.set_uv_set(i, doc.texture_sets.get(s).map_or(0, |t| t.uv_set));
            scene.set_shown(i, visible.get(s).copied().unwrap_or(true));
        }
    }
    scene
}
