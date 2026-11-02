//! Painter's scene geometry ([`SceneMesh`], from `spp-reader`) as MaterialCraft's [`Mesh`]: the same arrays, with each
//! part's polygon starts as its [`FaceGroups`] (polygon fill picks whole source polygons).

use crate::decode::DecodeError;
use crate::decode::assbin::{AssbinGeometry, SceneMesh, ScenePart};
use materialcraft::mesh::{FaceGroups, Mesh, MeshPart};

/// The scene of a `scene.assbin` record as our mesh, plus each part's face groups beside it.
pub fn parse_mesh(data: &[u8]) -> Result<(Mesh, Vec<FaceGroups>), DecodeError> {
    AssbinGeometry::parse(data).map(mesh_of)
}

/// `scene` as our mesh, plus each part's face groups (also kept on the part).
pub fn mesh_of(scene: SceneMesh) -> (Mesh, Vec<FaceGroups>) {
    let (parts, faces): (Vec<MeshPart>, Vec<FaceGroups>) = scene
        .parts
        .into_iter()
        .map(|p| {
            let part = part_of(p);
            let faces = part.faces.clone();
            (part, faces)
        })
        .unzip();
    (
        Mesh {
            materials: scene.materials,
            parts,
        },
        faces,
    )
}

fn part_of(p: ScenePart) -> MeshPart {
    let faces = match p.polygon_starts.is_empty() {
        true => FaceGroups::per_triangle(p.triangles.len()),
        false => FaceGroups::from_polygon_starts(p.polygon_starts),
    };
    MeshPart {
        name: p.name,
        material: p.material,
        positions: p.positions,
        normals: p.normals,
        tangents: p.tangents,
        uv_sets: p.uv_sets,
        triangles: p.triangles,
        faces,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_starts_become_face_groups() {
        let part = ScenePart {
            positions: vec![[0.0; 3]; 4],
            uv_sets: vec![vec![[0.0; 2]; 4]],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            polygon_starts: vec![true, false],
            ..ScenePart::default()
        };
        let scene = SceneMesh {
            materials: vec!["m".into()],
            parts: vec![
                part.clone(),
                ScenePart {
                    polygon_starts: Vec::new(),
                    ..part
                },
            ],
        };
        let (mesh, faces) = mesh_of(scene);
        assert_eq!(mesh.validate(), Ok(()));
        assert_eq!(faces[0], FaceGroups::from_polygon_starts([true, false]), "one quad");
        assert_eq!(
            faces[1],
            FaceGroups::per_triangle(2),
            "unknown polygons: a face per triangle"
        );
        assert_eq!(mesh.parts[0].faces, faces[0]);
    }
}
