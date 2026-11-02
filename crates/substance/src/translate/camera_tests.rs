//! Imported cameras against Painter's own camera model.
//!
//! The reference ([`painter_projection`]) projects a point the way Painter's viewer does from its stored camera:
//! into the camera's frame by the inverse of its rotation, then through a pinhole whose vertical field of view comes
//! from the film height and focal length (or a parallel view `orthographicHeight` tall), fitted to the viewport's
//! height. Ours goes the importer's way: Painter's camera → scene camera → viewport fit → orbit pose and settings →
//! `CameraSettings::camera`. The corpus check (ignored; `SPP_CORPUS=<.spp files or folders, ';'-separated>`) compares
//! both for every vertex of each project's mesh at 1920 × 1080 and prints the differences in pixels; with
//! `CAMERA_SHOTS=<dir>` (and optionally `CAMERA_SIZE=<w>x<h>`) it also writes the mesh's silhouette seen through
//! our imported "Saved view" camera as `<project>.png`, to lay over screenshots of Painter's viewport.

use super::cameras::{ProjectCameras, SAVED_VIEW, SCENE_SOURCE, VIEWER_SOURCE};
use crate::decode::assbin::AssbinCamera;
use crate::model::cameras::{PainterCamera, PainterCameras, PainterProjection};
use crate::project::SppProject;
use materialcraft::mesh::{Mesh, MeshPart};
use materialcraft::paint::saved_view::{CameraOrigin, SavedCamera};

/// Screen position of `p` in a `size` viewport as Painter's viewer shows it through `c`; `None` behind the camera.
fn painter_projection(c: &PainterCamera, p: [f32; 3], size: [f32; 2]) -> Option<[f32; 2]> {
    let [x, y, z, w] = c.rotation;
    let n = (x * x + y * y + z * z + w * w).sqrt();
    // the inverse rotation: the conjugate of the unit quaternion
    let q = [-x / n, -y / n, -z / n, w / n];
    let d = [p[0] - c.position[0], p[1] - c.position[1], p[2] - c.position[2]];
    let v = rotate(q, d);
    let aspect = size[0] / size[1];
    let (sx, sy) = match c.projection {
        PainterProjection::Orthographic => {
            let half = 0.5 * c.orthographic_height;
            (v[0] / (half * aspect), v[1] / half)
        }
        _ => {
            if v[2] >= -1e-6 {
                return None;
            }
            let t = 0.5 * c.film_size[1] / c.focal_length;
            (v[0] / (-v[2] * t * aspect), v[1] / (-v[2] * t))
        }
    };
    Some([(sx + 1.0) * 0.5 * size[0], (1.0 - sy) * 0.5 * size[1]])
}

fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let (u, w) = ([q[0], q[1], q[2]], q[3]);
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let uv = cross(u, v);
    let uuv = cross(u, uv);
    std::array::from_fn(|i| v[i] + 2.0 * (w * uv[i] + uuv[i]))
}

/// Differences between Painter's and our projection of `points` (both in front, the reference inside the frame):
/// (count, mean, max) in pixels.
fn framing_error(painter: &PainterCamera, ours: &SavedCamera, mesh: &Mesh, size: [f32; 2]) -> (usize, f32, f32) {
    let fit = mesh.fit();
    let camera = ours.settings.camera(&ours.orbit, size);
    let (mut n, mut sum, mut max) = (0usize, 0.0f32, 0.0f32);
    for p in mesh.parts.iter().flat_map(|part| &part.positions) {
        let Some(a) = painter_projection(painter, *p, size) else {
            continue;
        };
        if !(0.0..=size[0]).contains(&a[0]) || !(0.0..=size[1]).contains(&a[1]) {
            continue;
        }
        let Some(b) = camera.project(fit.apply(*p)) else {
            continue;
        };
        let e = (a[0] - b.screen[0]).hypot(a[1] - b.screen[1]);
        n += 1;
        sum += e;
        max = max.max(e);
    }
    (n, if n > 0 { sum / n as f32 } else { 0.0 }, max)
}

fn painter_camera(
    label: &str,
    uid: u64,
    position: [f32; 3],
    rotation: [f32; 4],
    projection: PainterProjection,
    film: [f32; 2],
) -> PainterCamera {
    PainterCamera {
        uid: Some(uid),
        label: label.into(),
        projection,
        position,
        rotation,
        focal_length: 16.667965,
        film_size: film,
        orthographic_height: 1.3,
        focus_distance: Some(2.5),
    }
}

/// A box off the origin, in Painter's normalised space.
fn mesh() -> Mesh {
    let mut positions = Vec::new();
    for i in 0..27 {
        let (x, y, z) = ((i % 3) as f32, ((i / 3) % 3) as f32, (i / 9) as f32);
        positions.push([0.1 + 0.4 * x, -0.3 + 0.35 * y, 0.2 - 0.5 * z]);
    }
    Mesh {
        materials: vec!["m".into()],
        parts: vec![MeshPart {
            positions,
            triangles: vec![[0, 1, 2]],
            ..MeshPart::default()
        }],
    }
}

#[test]
fn the_saved_view_and_scene_cameras_frame_the_mesh_like_painter() {
    // the viewer camera of VASE (rolled about a degree) and the scene camera of Staff, both from the corpus
    let viewer = painter_camera(
        "Default camera",
        8,
        [3.091177, 1.9695756, 1.6316663],
        [0.20727485, -0.49212113, -0.109586366, -0.8383584],
        PainterProjection::Perspective,
        [20.7264, 15.5448],
    );
    let mut scene = painter_camera(
        "Camera",
        9,
        [3.9545298, 2.543086, 3.599668],
        [-0.20997299, 0.38577998, 0.09062841, 0.8937962],
        PainterProjection::Perspective,
        [36.0, 20.25],
    );
    scene.focal_length = 50.0;
    let top = painter_camera(
        "Top",
        10,
        [0.3, 6.0, -0.1],
        [-0.70710677, 0.0, 0.0, 0.70710677],
        PainterProjection::Orthographic,
        [36.0, 24.0],
    );
    let cams = ProjectCameras {
        painter: PainterCameras {
            viewer: Some(viewer.clone()),
            scene: vec![scene.clone(), top.clone()],
            selected: Some(9),
        },
        assbin: Vec::new(),
    };
    let mesh = mesh();
    let view = cams.view(&mesh).unwrap();
    let names: Vec<&str> = view.cameras.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, [SavedCamera::DEFAULT_NAME, SAVED_VIEW, "Camera", "Top"]);
    assert_eq!(view.active_camera, 2, "Painter's selected camera stays active");
    assert!(matches!(&view.cameras[1].origin, CameraOrigin::Imported { source, .. } if source == VIEWER_SOURCE));
    assert!(matches!(&view.cameras[2].origin, CameraOrigin::Imported { source, .. } if source == SCENE_SOURCE));
    assert_eq!(
        view.cameras[0].values(),
        view.cameras[1].values(),
        "the default camera opens with the saved view"
    );
    assert_eq!(
        (
            view.cameras[1].aspect,
            view.cameras[2].aspect.map(|a| (a * 1e4).round())
        ),
        (None, Some(17778.0))
    );
    for (painter, ours) in [
        (&viewer, &view.cameras[1]),
        (&scene, &view.cameras[2]),
        (&top, &view.cameras[3]),
    ] {
        let (n, mean, max) = framing_error(painter, ours, &mesh, [1920.0, 1080.0]);
        assert!(
            n > 10 && max < 0.05,
            "{}: {n} points, mean {mean} px, max {max} px",
            ours.name
        );
    }
}

#[test]
fn mesh_cameras_are_the_fallback_and_nothing_gives_no_view() {
    let mesh = mesh();
    assert!(ProjectCameras::default().view(&mesh).is_none());
    let c = AssbinCamera {
        name: "Shot".into(),
        position: [3.9545298, 2.543086, 3.599668],
        // as stored: directions divided by the normalisation radius
        look_at: [-0.33929, -0.23188, -0.31984],
        up: [-0.16873, 0.46648, -0.15905],
        horizontal_fov: 0.6911112,
        aspect: 1.7777778,
    };
    let view = ProjectCameras {
        painter: PainterCameras::default(),
        assbin: vec![c],
    }
    .view(&mesh)
    .unwrap();
    assert_eq!(view.cameras.len(), 2);
    assert_eq!((view.cameras[1].name.as_str(), view.active_camera), ("Shot", 0));
    // the same camera as Painter lists it in camera.ini: 50 mm on a 36 × 20.25 mm film
    let mut painter = painter_camera(
        "Shot",
        9,
        c_position(),
        [-0.20997299, 0.38577998, 0.09062841, 0.8937962],
        PainterProjection::Perspective,
        [36.0, 20.25],
    );
    painter.focal_length = 50.0;
    let (n, _, max) = framing_error(&painter, &view.cameras[1], &mesh, [1920.0, 1080.0]);
    assert!(n > 10 && max < 1.5, "{n} points, max {max} px");
}

fn c_position() -> [f32; 3] {
    [3.9545298, 2.543086, 3.599668]
}

/// Every `.spp` below the `;`-separated paths.
fn corpus_files(list: &str) -> Vec<std::path::PathBuf> {
    fn walk(p: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        if p.is_file() {
            out.push(p.to_path_buf());
        } else if let Ok(entries) = std::fs::read_dir(p) {
            for e in entries.flatten() {
                let path = e.path();
                if path.is_dir() || path.extension().is_some_and(|x| x.eq_ignore_ascii_case("spp")) {
                    walk(&path, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    list.split(';')
        .filter(|s| !s.is_empty())
        .for_each(|s| walk(std::path::Path::new(s), &mut out));
    out.sort();
    out
}

#[test]
#[ignore]
fn corpus_saved_views_reproduce_painter_framing() {
    let Ok(list) = std::env::var("SPP_CORPUS") else { return };
    let shots = std::env::var_os("CAMERA_SHOTS").map(std::path::PathBuf::from);
    let shot_size = std::env::var("CAMERA_SIZE")
        .ok()
        .and_then(|s| {
            s.split_once('x')
                .and_then(|(w, h)| Some([w.parse::<usize>().ok()?, h.parse::<usize>().ok()?]))
        })
        .unwrap_or([1920, 1080]);
    let size = [1920.0, 1080.0];
    let mut worst = 0.0f32;
    for path in corpus_files(&list) {
        let project = SppProject::open(&path).unwrap();
        let scene = project.read("/paint/scene.assbin").unwrap();
        let (mesh, _) = super::mesh::parse_mesh(&scene).unwrap();
        let (cams, notes) = ProjectCameras::read(&project, &scene);
        let view = cams.view(&mesh).unwrap();
        let viewer = cams.painter.viewer.as_ref().unwrap();
        let saved = view.cameras.iter().find(|c| c.name == SAVED_VIEW).unwrap();
        let (n, mean, max) = framing_error(viewer, saved, &mesh, size);
        let kind = if viewer.projection == PainterProjection::Orthographic {
            "ortho"
        } else {
            "persp"
        };
        println!(
            "{}\t{kind}\troll {:.2}°\t{n} vertices\tmean {mean:.4} px\tmax {max:.4} px\t{} cameras{}",
            path.display(),
            saved.orbit.roll.to_degrees(),
            view.cameras.len(),
            if notes.is_empty() {
                String::new()
            } else {
                format!("\t{}", notes.join("; "))
            }
        );
        for (painter, ours) in cams.painter.scene.iter().zip(view.cameras.iter().skip(2)) {
            let (n, mean, max) = framing_error(painter, ours, &mesh, size);
            println!(
                "  scene camera {}: {n} vertices, mean {mean:.4} px, max {max:.4} px",
                ours.name
            );
            worst = worst.max(max);
        }
        worst = worst.max(max);
        if let Some(dir) = &shots {
            let png = silhouette(saved, &mesh, shot_size);
            let name = path
                .file_stem()
                .map_or("project".into(), |s| s.to_string_lossy().into_owned());
            std::fs::write(dir.join(format!("{name}.png")), png).unwrap();
        }
    }
    println!("worst difference: {worst:.4} px");
}

/// The mesh's silhouette through `camera` as a greyscale PNG (white = mesh).
fn silhouette(camera: &SavedCamera, mesh: &Mesh, [w, h]: [usize; 2]) -> Vec<u8> {
    let fit = mesh.fit();
    let cam = camera.settings.camera(&camera.orbit, [w as f32, h as f32]);
    let mut px = vec![[0.0f32, 0.0, 0.0, 1.0]; w * h];
    for part in &mesh.parts {
        let screen: Vec<Option<[f32; 2]>> = part
            .positions
            .iter()
            .map(|p| cam.project(fit.apply(*p)).map(|q| q.screen))
            .collect();
        for t in &part.triangles {
            let [Some(a), Some(b), Some(c)] = t.map(|i| screen[i as usize]) else {
                continue;
            };
            let (x0, x1) = (
                a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize,
                (a[0].max(b[0]).max(c[0]).ceil() as usize).min(w),
            );
            let (y0, y1) = (
                a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize,
                (a[1].max(b[1]).max(c[1]).ceil() as usize).min(h),
            );
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if area.abs() < 1e-9 {
                continue;
            }
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = [x as f32 + 0.5, y as f32 + 0.5];
                    let e = |u: [f32; 2], v: [f32; 2]| {
                        ((v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0])) / area
                    };
                    if e(a, b) >= 0.0 && e(b, c) >= 0.0 && e(c, a) >= 0.0 {
                        px[y * w + x] = [1.0, 1.0, 1.0, 1.0];
                    }
                }
            }
        }
    }
    materialcraft::png::encode(w, h, &px, 8)
}
