//! Manual check of geometry and UV orientation on one real project (kept outside the repository).
//! `SPP_EXPORT=<file.spp> SPP_EXPORT_OUT=<dir> cargo test --release -p spp-reader
//! visual -- --ignored --nocapture` writes `<dir>/<stem>.obj` and, per texture set, a PPM image of the
//! baked AO with the UV wireframe drawn on top, and prints how well the baked position map,
//! sampled at each vertex's UV, correlates with the vertex positions for both V conventions.

use super::scene_mesh::{SceneMesh as Mesh, ScenePart as MeshPart};
use super::*;
use crate::decode::image::{ImageResource, PixelFormat};
use crate::project::SppProject;
use std::fmt::Write as _;
use std::path::Path;

/// Writes an OBJ; OBJ UVs are OpenGL-style, so V is flipped back.
fn write_obj(mesh: &Mesh, path: &Path) {
    let mut s = String::new();
    let mut base = 1;
    for part in &mesh.parts {
        let _ = writeln!(s, "o {}\nusemtl {}", part.name, mesh.materials[part.material]);
        for p in &part.positions {
            let _ = writeln!(s, "v {} {} {}", p[0], p[1], p[2]);
        }
        for t in &part.uv_sets[0] {
            let _ = writeln!(s, "vt {} {}", t[0], 1.0 - t[1]);
        }
        for n in &part.normals {
            let _ = writeln!(s, "vn {} {} {}", n[0], n[1], n[2]);
        }
        for t in &part.triangles {
            let [a, b, c] = t.map(|i| i as usize + base);
            let _ = writeln!(s, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}");
        }
        base += part.positions.len();
    }
    std::fs::write(path, s).unwrap();
}

/// Pixels as RGBA floats in stored row order.
fn pixels(image: &ImageResource) -> Vec<[f32; 4]> {
    let raw = image.decompress().unwrap();
    let u16_at = |c: &[u8], i: usize| u16::from_le_bytes([c[2 * i], c[2 * i + 1]]);
    match &image.format {
        PixelFormat::Lum8 => raw.iter().map(|&v| [v as f32 / 255.0; 4]).collect(),
        PixelFormat::Lum16 => raw
            .chunks_exact(2)
            .map(|c| [u16_at(c, 0) as f32 / 65535.0; 4])
            .collect(),
        PixelFormat::Rgba8 => raw
            .chunks_exact(4)
            .map(|c| [0, 1, 2, 3].map(|i| c[i] as f32 / 255.0))
            .collect(),
        PixelFormat::Rgba16 => raw
            .chunks_exact(8)
            .map(|c| [0, 1, 2, 3].map(|i| u16_at(c, i) as f32 / 65535.0))
            .collect(),
        PixelFormat::Rgba16F => raw
            .chunks_exact(8)
            .map(|c| [0, 1, 2, 3].map(|i| f16_to_f32(u16_at(c, i))))
            .collect(),
        PixelFormat::Rgba32F => raw
            .chunks_exact(16)
            .map(|c| [0, 1, 2, 3].map(|i| f32::from_le_bytes(c[4 * i..4 * i + 4].try_into().unwrap())))
            .collect(),
        other => panic!("unsupported format {}", other.name()),
    }
}

/// The image resource whose table-of-contents name is `name`.
fn find_image(project: &SppProject, name: &str) -> Option<(u32, u32, Vec<[f32; 4]>)> {
    let toc = project.resource_toc().ok()??;
    let version = toc.entries.iter().find(|e| e.name == name)?.version.clone()?;
    let resource = project.resources().into_iter().find(|r| r.file_name == version)?;
    let data = project.read(&resource.path).ok()?;
    let image = ImageResource::parse(&data).ok()?;
    Some((image.width, image.height, pixels(&image)))
}

fn pearson(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f32;
    let (ma, mb) = (a.iter().sum::<f32>() / n, b.iter().sum::<f32>() / n);
    let cov: f32 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let va: f32 = a.iter().map(|x| (x - ma).powi(2)).sum();
    let vb: f32 = b.iter().map(|y| (y - mb).powi(2)).sum();
    cov / (va * vb).sqrt()
}

/// For each position axis, the best |correlation| with any channel of the map sampled at the
/// vertex UVs; `top_down` reads row `v * h` (glTF V on rows stored top first), else `(1 - v) * h`.
fn position_correlation(part: &MeshPart, (w, h, px): &(u32, u32, Vec<[f32; 4]>), top_down: bool) -> [f32; 3] {
    let sample = |uv: &[f32; 2]| {
        let v = if top_down { uv[1] } else { 1.0 - uv[1] };
        let x = ((uv[0].fract() * *w as f32) as usize).min(*w as usize - 1);
        let y = ((v.clamp(0.0, 1.0) * *h as f32) as usize).min(*h as usize - 1);
        px[y * *w as usize + x]
    };
    let samples: Vec<[f32; 4]> = part.uv_sets[0].iter().map(sample).collect();
    [0, 1, 2].map(|axis| {
        let pos: Vec<f32> = part.positions.iter().map(|p| p[axis]).collect();
        (0..3)
            .map(|c| pearson(&pos, &samples.iter().map(|s| s[c]).collect::<Vec<_>>()).abs())
            .fold(0.0, f32::max)
    })
}

/// The AO map with the UV wireframe in red (glTF convention, rows top first), at most 1024 wide.
fn uv_plot(part: &MeshPart, (w, h, px): &(u32, u32, Vec<[f32; 4]>), path: &Path) {
    let step = (*w as usize).div_ceil(1024).max(1);
    let (ow, oh) = (*w as usize / step, *h as usize / step);
    let mut out: Vec<[f32; 4]> = (0..ow * oh)
        .map(|i| px[(i / ow) * step * *w as usize + (i % ow) * step])
        .collect();
    out.iter_mut().for_each(|p| p[3] = 1.0);
    let uv = &part.uv_sets[0];
    for t in &part.triangles {
        for e in 0..3 {
            let (a, b) = (uv[t[e] as usize], uv[t[(e + 1) % 3] as usize]);
            for s in 0..=32 {
                let f = s as f32 / 32.0;
                let x = ((a[0] + (b[0] - a[0]) * f) * ow as f32) as isize;
                let y = ((a[1] + (b[1] - a[1]) * f) * oh as f32) as isize;
                if (0..ow as isize).contains(&x) && (0..oh as isize).contains(&y) {
                    out[y as usize * ow + x as usize] = [1.0, 0.0, 0.0, 1.0];
                }
            }
        }
    }
    let mut ppm = format!("P6\n{ow} {oh}\n255\n").into_bytes();
    ppm.extend(
        out.iter()
            .flat_map(|p| [p[0], p[1], p[2]].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)),
    );
    std::fs::write(path, ppm).unwrap();
}

#[test]
#[ignore]
fn visual_check_of_one_project() {
    let (Ok(spp), Ok(out)) = (std::env::var("SPP_EXPORT"), std::env::var("SPP_EXPORT_OUT")) else {
        return;
    };
    let project = SppProject::open(Path::new(&spp)).unwrap();
    let mesh = AssbinGeometry::parse(&project.read("/paint/scene.assbin").unwrap()).unwrap();
    let stem = Path::new(&spp).file_stem().unwrap().to_string_lossy().replace(' ', "_");
    write_obj(&mesh, &Path::new(&out).join(format!("{stem}.obj")));
    for part in &mesh.parts {
        let set = &mesh.materials[part.material];
        if let Some(map) = find_image(&project, &format!("Position {set}")) {
            let down = position_correlation(part, &map, true);
            let up = position_correlation(part, &map, false);
            println!(
                "{} ({set}): position map correlation, V flipped {down:?}, not flipped {up:?}",
                part.name
            );
        }
        if let Some(ao) = find_image(&project, &format!("Ambient Occlusion Map from Mesh {set}")) {
            let name = format!("{stem}_{}_uv_on_ao.ppm", part.name.replace(['/', ' '], "_"));
            uv_plot(part, &ao, &Path::new(&out).join(name));
        }
    }
}

/// IEEE half to single precision (the baked position maps are half floats).
fn f16_to_f32(h: u16) -> f32 {
    let sign = ((h >> 15) as u32) << 31;
    let exp = ((h >> 10) & 0x1f) as u32;
    let man = (h & 0x3ff) as u32;
    let bits = match (exp, man) {
        (0, 0) => sign,
        (0, _) => {
            let shift = man.leading_zeros() - 21;
            sign | ((113 - shift) << 23) | (((man << shift) & 0x3ff) << 13)
        }
        (0x1f, 0) => sign | 0x7f80_0000,
        (0x1f, _) => sign | 0x7fc0_0000 | (man << 13),
        _ => sign | ((exp + 112) << 23) | (man << 13),
    };
    f32::from_bits(bits)
}
