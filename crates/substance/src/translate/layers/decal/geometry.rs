//! [`SetGeometry`]: the mesh parts of one texture set as polygon-fill picks need them: corners in UV space
//! (Painter's convention, V up) and in 3D, a BVH for clicks in the 3D view, and per part the face groups, connected
//! pieces and UV islands a picked triangle grows to. Topology and BVH are built on first use and kept, so a set
//! re-imported for blend fits picks from the same structures.

use materialcraft::mesh::{Bvh, FaceGroups, Mesh, MeshPart, Sphere, Topology, Triangles, Vec3};
use std::sync::OnceLock;

/// One part of the set.
pub struct GeometryPart<'m> {
    pub part: &'m MeshPart,
    /// Source polygons (Painter's per-face flag); one face per triangle when unknown.
    pub faces: FaceGroups,
    topology: OnceLock<Topology>,
}

impl GeometryPart<'_> {
    pub fn name(&self) -> &str {
        &self.part.name
    }

    /// The corner positions of triangle `t`, if its indices are valid.
    pub fn corners(&self, t: usize) -> Option<[Vec3; 3]> {
        let tri = self.part.triangles.get(t)?;
        let p = |i: u32| self.part.positions.get(i as usize).copied();
        Some([p(tri[0])?, p(tri[1])?, p(tri[2])?])
    }

    /// The corner UVs of triangle `t` in Painter's convention (origin bottom-left, V up): the mesh stores glTF's
    /// (V down), see `decode::assbin::part`.
    pub fn uvs(&self, t: usize) -> Option<[[f32; 2]; 3]> {
        let tri = self.part.triangles.get(t)?;
        let uvs = self.part.uv_sets.first()?;
        let uv = |i: u32| uvs.get(i as usize).map(|uv| [uv[0], 1.0 - uv[1]]);
        Some([uv(tri[0])?, uv(tri[1])?, uv(tri[2])?])
    }

    pub fn topology(&self) -> &Topology {
        self.topology.get_or_init(|| Topology::of_part(self.part, 0))
    }
}

/// Every triangle of the set as one list, for the BVH.
/// A triangle of a part: (part index, triangle index, corners).
type PartTriangle = (usize, usize, [Vec3; 3]);

struct AllTriangles<'a>(&'a [PartTriangle]);

impl Triangles for AllTriangles<'_> {
    fn triangle_count(&self) -> usize {
        self.0.len()
    }

    fn corners(&self, i: usize) -> [Vec3; 3] {
        self.0.get(i).map_or([[0.0; 3]; 3], |t| t.2)
    }
}

pub struct SetGeometry<'m> {
    pub parts: Vec<GeometryPart<'m>>,
    /// The bounding sphere of the whole mesh (every set), which Painter's model-sized brushes are measured
    /// against.
    pub scene: Option<Sphere>,
    /// Every valid triangle, and the BVH over them.
    triangles: OnceLock<(Vec<PartTriangle>, Bvh)>,
}

impl<'m> SetGeometry<'m> {
    /// The parts of `mesh` on material slot `slots` (all slots Painter collapsed into the set); `faces` are the
    /// parts' face groups by part index (missing ones count one face per triangle).
    pub fn new(mesh: &'m Mesh, faces: &[FaceGroups], slots: &[usize]) -> SetGeometry<'m> {
        let parts = mesh
            .parts
            .iter()
            .enumerate()
            .filter(|(_, p)| slots.contains(&p.material))
            .map(|(i, part)| GeometryPart {
                part,
                faces: faces
                    .get(i)
                    .filter(|f| f.triangle_count() == part.triangles.len())
                    .cloned()
                    .unwrap_or_else(|| FaceGroups::per_triangle(part.triangles.len())),
                topology: OnceLock::new(),
            })
            .collect();
        SetGeometry {
            parts,
            scene: mesh.bounding_sphere(),
            triangles: OnceLock::new(),
        }
    }

    /// The nearest triangle the ray `origin + t * dir` (t > 0) hits: (part, triangle, distance).
    pub fn ray_hit(&self, origin: Vec3, dir: Vec3) -> Option<(usize, usize, f32)> {
        let (list, bvh) = self.triangles.get_or_init(|| {
            let mut list = Vec::new();
            for (p, part) in self.parts.iter().enumerate() {
                for t in 0..part.part.triangles.len() {
                    if let Some(c) = part.corners(t) {
                        list.push((p, t, c));
                    }
                }
            }
            let bvh = Bvh::build(&AllTriangles(&list));
            (list, bvh)
        });
        let hit = bvh.intersect(&AllTriangles(list), origin, dir, f32::MAX)?;
        let (p, t, _) = list.get(hit.tri as usize)?;
        Some((*p, *t, hit.t))
    }
}
