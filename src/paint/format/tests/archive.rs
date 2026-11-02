//! The archive is readable with standard tools: entry names, methods, pretty JSON, valid glTF.

use super::fixtures::full_document;
use super::round_trip::to_bytes;
use crate::json;
use crate::mesh::Mesh;
use crate::zip::{Method, ZipReader};
use std::io::Cursor;

#[test]
fn entries_and_methods() {
    let doc = full_document();
    let bytes = to_bytes(&doc);
    if let Some(dir) = std::env::var_os("MATERIALCRAFT_MCPAINT_OUT") {
        std::fs::write(std::path::Path::new(&dir).join("full.mcpaint"), &bytes).unwrap();
    }
    let mut zip = ZipReader::open(Cursor::new(bytes)).unwrap();
    let names: Vec<String> = zip.entries().iter().map(|e| e.name.clone()).collect();
    assert_eq!(names[..3], ["document.json", "import-report.json", "mesh/model.glb"]);
    assert_eq!(names.last().map(String::as_str), Some("thumbnail.png"));
    assert!(names.contains(&"textures/body/layers/p0/base_color.png".to_string()));
    assert!(names.contains(&"textures/body/layers/p2/user3.exr".to_string()));
    assert!(names.contains(&"textures/body/layers/p0/mask-0.png".to_string()));
    assert!(names.contains(&"textures/body/baked/curvature-2.png".to_string()));
    assert!(names.contains(&"textures/odd_.._id/layers/x_y-2/roughness.png".to_string()));
    assert!(names.contains(&"foreign/substance/document.bin".to_string()));
    for e in zip.entries() {
        if e.name.ends_with(".png") || e.name.ends_with(".exr") {
            assert_eq!(e.method(), Some(Method::Stored), "{}", e.name);
        }
    }
    assert_eq!(zip.find("document.json").unwrap().method(), Some(Method::Deflate));

    let text = String::from_utf8(zip.read("document.json").unwrap()).unwrap();
    assert!(text.starts_with("{\n  \"format\": \"mcpaint\",\n  \"format_version\": \"0.18\",\n"));
    assert!(text.contains("\n  \"mesh\": \"mesh/model.glb\",\n  \"height_filter\": \"sobel\",\n"));
    assert!(text.ends_with("}\n"));
    let root = json::parse(&text).unwrap();
    assert_eq!(root["texture_sets"][0]["resolution"].to_string(), "[150,130]");
    assert_eq!(root["texture_sets"][0]["layers"][0]["opacity"].as_f64(), Some(0.37));
    // the glb holds the triangles; the source polygons are in the manifest (`mesh_faces`)
    let mut triangles_only = doc.mesh.clone();
    triangles_only.parts.iter_mut().for_each(|p| p.faces = Default::default());
    assert_eq!(Mesh::from_glb(&zip.read("mesh/model.glb").unwrap()).unwrap(), triangles_only);
    assert_eq!(root["mesh_faces"][0]["runs"].to_string(), "[[1,2]]");
    // 16-bit samples are big-endian in the PNG: an independent decoder sees the model's values.
    let png = crate::png::decode(&zip.read("textures/body/layers/p1/height.png").unwrap()).unwrap();
    let layer = doc.texture_sets[0].layer("p1").unwrap();
    let model = layer.images()[0].1;
    assert_eq!(model.format().id(), "gray16");
    for (x, y) in [(0, 0), (3, 1), (149, 129)] {
        let (a, b) = (png.px[y * 150 + x][0], model.get(x as u32, y as u32)[0]);
        assert!((a - b).abs() < 1e-6, "{x},{y}: {a} vs {b}");
    }
    let thumb = crate::png::decode_raw(&zip.read("thumbnail.png").unwrap()).unwrap();
    assert_eq!((thumb.width, thumb.height), (150, 130));
}
