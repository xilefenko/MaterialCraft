//! A project's cameras → the document's camera list.
//!
//! - Painter's viewer camera as the project was saved (`/editor/camera.ini` `defaultCamera`) becomes our default
//!   camera, so the document opens with Painter's view, and an imported camera "Saved view" that keeps it.
//! - The mesh's cameras: the scene cameras of `camera.ini`, or, when that has none, the cameras block of the
//!   embedded `scene.assbin`. Imported under their own names.
//! - Painter's active camera stays active.
//!
//! Painter's cameras live in its normalised mesh space, which is the document mesh's space, and are moved into the
//! viewport's fit like the mesh ([`imported_cameras`]). Lens: a perspective camera's vertical field of view comes from
//! its focal length and film height, `2 · atan(film height / 2 / focal length)`, kept for any viewport shape
//! (Inferred: the viewer camera's 16.668 mm on its 15.5448 mm film give exactly 50°, a round vertical field of view;
//! no screenshot of a saved view was found to confirm how Painter fits the film to a wide viewport). An orthographic
//! camera keeps `orthographicHeight`. The viewer camera's film is no frame (it fills the viewport), a scene camera's
//! film gives its aspect. `camera_tests` compares the result with this model of Painter's viewer.

use crate::decode::assbin::AssbinCamera;
use crate::model::cameras::{CAMERA_INI, PainterCamera, PainterCameras, PainterProjection};
use crate::project::SppProject;
use materialcraft::camera::{SceneCamera, SceneLens};
use materialcraft::mesh::Mesh;
use materialcraft::paint::camera_import::imported_cameras;
use materialcraft::paint::saved_view::{SavedCamera, SavedView};

/// Name of the imported camera that keeps Painter's saved view.
pub const SAVED_VIEW: &str = "Saved view";
/// Sources of the imported cameras (see `CameraOrigin::SOURCES`).
pub const VIEWER_SOURCE: &str = "painter-viewer";
pub const SCENE_SOURCE: &str = "painter-scene";

/// The cameras one project brings.
#[derive(Debug, Default)]
pub struct ProjectCameras {
    pub painter: PainterCameras,
    /// The `scene.assbin` cameras, used when `camera.ini` lists no scene cameras.
    pub assbin: Vec<AssbinCamera>,
}

impl ProjectCameras {
    /// Reads the cameras of `project` (`scene` is its `scene.assbin`). What cannot be read is left out and said in the
    /// returned notes; a project without `camera.ini` is no problem.
    pub fn read(project: &SppProject, scene: &[u8]) -> (ProjectCameras, Vec<String>) {
        let mut notes = Vec::new();
        let painter = match project.read(CAMERA_INI) {
            Ok(data) => PainterCameras::parse(&data).unwrap_or_else(|e| {
                notes.push(format!("viewer cameras unreadable ({e})"));
                PainterCameras::default()
            }),
            Err(_) => PainterCameras::default(),
        };
        let assbin = AssbinCamera::read_all(scene).unwrap_or_else(|e| {
            notes.push(format!("mesh cameras unreadable ({e})"));
            Vec::new()
        });
        (ProjectCameras { painter, assbin }, notes)
    }

    /// The document's view with these cameras (exposure and environment at their defaults; the editor shows a
    /// freshly imported project with the user's), or `None` when there is no camera at all.
    pub fn view(&self, mesh: &Mesh) -> Option<SavedView> {
        let import = |c: Option<SceneCamera>, source: &str| c.and_then(|c| imported_cameras(mesh, &[c], source).pop());
        let viewer = import(
            self.painter
                .viewer
                .as_ref()
                .and_then(|c| scene_camera(c, SAVED_VIEW, false)),
            VIEWER_SOURCE,
        );
        // (Painter's uid, camera) of every usable scene camera
        let scene: Vec<(Option<u64>, SavedCamera)> = if self.painter.scene.is_empty() {
            self.assbin
                .iter()
                .filter_map(|c| Some((None, import(assbin_camera(c), SCENE_SOURCE)?)))
                .collect()
        } else {
            self.painter
                .scene
                .iter()
                .filter_map(|c| Some((c.uid, import(scene_camera(c, &c.label, true), SCENE_SOURCE)?)))
                .collect()
        };
        if viewer.is_none() && scene.is_empty() {
            return None;
        }
        let default = viewer.as_ref().map_or_else(SavedCamera::default, |v| {
            SavedCamera::own(SavedCamera::DEFAULT_NAME, v.values())
        });
        let mut cameras = vec![default];
        cameras.extend(viewer);
        let selected = self
            .painter
            .selected
            .and_then(|uid| scene.iter().position(|(u, _)| *u == Some(uid)))
            .map(|i| cameras.len() + i);
        cameras.extend(scene.into_iter().map(|(_, c)| c));
        Some(SavedView {
            cameras,
            active_camera: selected.unwrap_or(0),
            ..SavedView::default()
        })
    }
}

/// A camera of `camera.ini` as a scene camera named `name` (with its film's aspect when `framed`).
fn scene_camera(c: &PainterCamera, name: &str, framed: bool) -> Option<SceneCamera> {
    let [film_w, film_h] = c.film_size;
    let lens = match c.projection {
        PainterProjection::Orthographic => SceneLens::Orthographic {
            height: c.orthographic_height,
        },
        _ => SceneLens::Perspective {
            fov_y: 2.0 * (0.5 * film_h / c.focal_length).atan(),
        },
    };
    let usable = match lens {
        SceneLens::Perspective { fov_y } => fov_y.is_finite() && fov_y > 0.0,
        SceneLens::Orthographic { height } => height.is_finite() && height > 0.0,
    };
    let aspect = (film_w / film_h)
        .is_finite()
        .then_some(film_w / film_h)
        .filter(|a| framed && *a > 0.0);
    let name = if name.trim().is_empty() { "Camera" } else { name };
    SceneCamera::from_rotation(name, c.position, c.rotation, lens, aspect).filter(|_| usable)
}

/// A camera of the assbin scene (horizontal field of view and aspect → vertical field of view).
fn assbin_camera(c: &AssbinCamera) -> Option<SceneCamera> {
    let aspect = Some(c.aspect).filter(|a| a.is_finite() && *a > 0.0);
    let fov_y = 2.0 * ((0.5 * c.horizontal_fov).tan() / aspect.unwrap_or(1.0)).atan();
    if !(fov_y.is_finite() && fov_y > 0.0) {
        return None;
    }
    let unit = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        (l.is_finite() && l > 0.0).then(|| v.map(|x| x / l))
    };
    let (forward, up) = (unit(c.look_at)?, unit(c.up)?);
    let name = if c.name.trim().is_empty() {
        "Camera".to_string()
    } else {
        c.name.clone()
    };
    Some(SceneCamera {
        name,
        eye: c.position,
        forward,
        up,
        lens: SceneLens::Perspective { fov_y },
        aspect,
    })
}
