//! [`IslandTexels`]: the texels of a texture set that lie on its mesh (inside its UV islands, partly covered
//! texels included). Painter renders layer content only there; the rest of its texture cache holds its padding
//! (the island borders dilated outwards), which no layer produces. A UV-projected fill on a set whose islands
//! leave most of the tile empty therefore matches the cache on the islands and nowhere else (on one measured set,
//! 6 % of the tile covered: 0.09 steps there, 11 outside). Cache scores compare these texels.

use materialcraft::mesh::{Mesh, SurfaceKey, SurfaceMap};

#[derive(Debug, Clone, PartialEq)]
pub struct IslandTexels {
    width: usize,
    height: usize,
    inside: Vec<bool>,
}

impl IslandTexels {
    /// The texels the parts on material `slots` cover in UV set `uv_set` at `size`. `None` when nothing could
    /// be rasterised or nothing is covered (a score then compares every texel, as without a mesh).
    pub fn rasterise(mesh: &Mesh, slots: &[usize], uv_set: usize, size: (u32, u32)) -> Option<IslandTexels> {
        let (width, height) = (size.0 as usize, size.1 as usize);
        let mut inside = vec![false; width.checked_mul(height)?];
        for &material in slots {
            let key = SurfaceKey {
                material,
                uv_set,
                width: size.0,
                height: size.1,
                gutter: 0,
                mesh_revision: 0,
            };
            let Ok(map) = SurfaceMap::build(mesh, key) else {
                continue;
            };
            for (dst, c) in inside.iter_mut().zip(map.coverage()) {
                *dst |= c > 0.0;
            }
        }
        inside.contains(&true).then_some(IslandTexels { width, height, inside })
    }

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// True if texel `(x, y)` lies on the mesh (false outside the image).
    #[inline]
    pub fn contains(&self, x: usize, y: usize) -> bool {
        x < self.width && self.inside.get(y * self.width + x).copied().unwrap_or(false)
    }

    /// Share of the texels on the mesh, 0..1.
    pub fn share(&self) -> f32 {
        let n = self.inside.iter().filter(|&&b| b).count();
        n as f32 / self.inside.len().max(1) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use materialcraft::mesh::MeshPart;

    /// One triangle over the top-left half of the left half of the tile, on material 0.
    fn mesh() -> Mesh {
        let part = MeshPart {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            uv_sets: vec![vec![[0.0, 0.0], [0.5, 0.0], [0.0, 1.0]]],
            triangles: vec![[0, 1, 2]],
            ..MeshPart::default()
        };
        Mesh {
            parts: vec![part],
            ..Mesh::default()
        }
    }

    #[test]
    fn covers_the_islands_only() {
        let t = IslandTexels::rasterise(&mesh(), &[0], 0, (8, 8)).unwrap();
        assert_eq!(t.size(), (8, 8));
        assert!(t.contains(0, 0) && t.contains(1, 4));
        assert!(!t.contains(7, 7) && !t.contains(5, 0) && !t.contains(8, 0));
        assert!(t.share() > 0.2 && t.share() < 0.5, "{}", t.share());
        assert_eq!(IslandTexels::rasterise(&mesh(), &[3], 0, (8, 8)), None);
    }
}
