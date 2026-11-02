//! Assimp's 4x4 matrices: row-major, applied to column vectors (`p' = M p`), translation in
//! the last column.

pub type Vec3 = [f32; 3];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    m: [[f32; 4]; 4],
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// From 16 values in Assimp's storage order (row by row).
    pub fn from_rows(v: [f32; 16]) -> Self {
        let row = |r: usize| [v[r * 4], v[r * 4 + 1], v[r * 4 + 2], v[r * 4 + 3]];
        Transform {
            m: [row(0), row(1), row(2), row(3)],
        }
    }

    /// `self * other`: applies `other` first.
    pub fn then_local(&self, other: &Transform) -> Transform {
        let mut m = [[0.0; 4]; 4];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = (0..4).map(|k| self.m[r][k] * other.m[k][c]).sum();
            }
        }
        Transform { m }
    }

    pub fn point(&self, p: Vec3) -> Vec3 {
        let v = self.direction(p);
        [v[0] + self.m[0][3], v[1] + self.m[1][3], v[2] + self.m[2][3]]
    }

    /// Applies the upper 3x3 part (for tangents and bitangents).
    pub fn direction(&self, d: Vec3) -> Vec3 {
        let row = |r: usize| self.m[r][0] * d[0] + self.m[r][1] * d[1] + self.m[r][2] * d[2];
        [row(0), row(1), row(2)]
    }

    /// Determinant of the upper 3x3 part; negative when the transform mirrors.
    pub fn determinant(&self) -> f32 {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    /// Transforms a normal by the inverse transpose of the upper 3x3 part, using the cofactor
    /// matrix (inverse transpose times determinant); the result is normalised by the caller.
    pub fn normal(&self, n: Vec3) -> Vec3 {
        let m = &self.m;
        let cof = |r0: usize, r1: usize, c0: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
        let c = [
            [cof(1, 2, 1, 2), -cof(1, 2, 0, 2), cof(1, 2, 0, 1)],
            [-cof(0, 2, 1, 2), cof(0, 2, 0, 2), -cof(0, 2, 0, 1)],
            [cof(0, 1, 1, 2), -cof(0, 1, 0, 2), cof(0, 1, 0, 1)],
        ];
        let sign = if self.determinant() < 0.0 { -1.0 } else { 1.0 };
        let row = |r: usize| sign * (c[r][0] * n[0] + c[r][1] * n[1] + c[r][2] * n[2]);
        [row(0), row(1), row(2)]
    }
}

pub fn normalize(v: Vec3) -> Vec3 {
    let len = dot(v, v).sqrt();
    if len > 0.0 && len.is_finite() {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        v
    }
}

pub fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scale_translate(s: [f32; 3], t: [f32; 3]) -> Transform {
        Transform::from_rows([
            s[0], 0., 0., t[0], 0., s[1], 0., t[1], 0., 0., s[2], t[2], 0., 0., 0., 1.,
        ])
    }

    #[test]
    fn translation_is_in_the_last_column_and_parents_apply_last() {
        let parent = scale_translate([2.0; 3], [0.0, 0.0, 0.0]);
        let child = scale_translate([1.0; 3], [1.0, 0.0, 0.0]);
        assert_eq!(parent.then_local(&child).point([0.0, 1.0, 0.0]), [2.0, 2.0, 0.0]);
    }

    #[test]
    fn normals_use_the_inverse_transpose_and_mirrors_have_negative_determinant() {
        let squash = scale_translate([1.0, 2.0, 1.0], [0.0; 3]);
        let n = normalize(squash.normal(normalize([1.0, 1.0, 0.0])));
        let expected = normalize([2.0, 1.0, 0.0]);
        assert!((0..3).all(|i| (n[i] - expected[i]).abs() < 1e-6), "{n:?}");
        let mirror = scale_translate([-1.0, 1.0, 1.0], [0.0; 3]);
        assert!(mirror.determinant() < 0.0);
        assert_eq!(mirror.normal([1.0, 0.0, 0.0]), [-1.0, 0.0, 0.0]);
    }
}
