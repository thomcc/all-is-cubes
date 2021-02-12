// Copyright 2020-2021 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

//! [`Face`] type and related items.
//! This module is private but reexported by its parent.

use cgmath::{BaseNum, Vector3};
pub use ordered_float::{FloatIsNan, NotNan};
use std::convert::TryFrom;
use std::ops::{Index, IndexMut};

use crate::math::*;

/// Identifies a face of a cube or an orthogonal unit vector, except for
/// [`WITHIN`](Face::WITHIN) meaning “zero distance and undefined direction”.
///
/// So far, nearly every usage of Face has a use for [`WITHIN`](Face::WITHIN), but we
/// should keep an eye out for uses of the ‘true’ 6-face version.
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
#[repr(u8)]
pub enum Face {
    /// The interior volume of a cube, or an undefined direction. Corresponds to the vector `(0, 0, 0)`.
    WITHIN = 0,
    /// Negative X; the face whose normal vector is `(-1, 0, 0)`.
    NX,
    /// Negative Y; the face whose normal vector is `(0, -1, 0)`; downward.
    NY,
    /// Negative Z; the face whose normal vector is `(0, 0, -1)`.
    NZ,
    /// Positive X; the face whose normal vector is `(1, 0, 0)`.
    PX,
    /// Positive Y; the face whose normal vector is `(0, 1, 0)`; upward.
    PY,
    /// Positive Z; the face whose normal vector is `(0, 0, 1)`.
    PZ,
}

impl Face {
    /// All the values of [`Face`] except for [`Face::WITHIN`].
    pub const ALL_SIX: &'static [Face; 6] =
        &[Face::NX, Face::NY, Face::NZ, Face::PX, Face::PY, Face::PZ];
    /// All the values of [`Face`], with [`Face::WITHIN`] listed first.
    pub const ALL_SEVEN: &'static [Face; 7] = &[
        Face::WITHIN,
        Face::NX,
        Face::NY,
        Face::NZ,
        Face::PX,
        Face::PY,
        Face::PZ,
    ];

    /// Returns which axis this face's normal vector is parallel to, with the numbering
    /// X = 0, Y = 1, Z = 2. Panics if given [`Face::WITHIN`].
    pub fn axis_number(self) -> usize {
        match self {
            Face::WITHIN => panic!("WITHIN has no axis number"),
            Face::NX | Face::PX => 0,
            Face::NY | Face::PY => 1,
            Face::NZ | Face::PZ => 2,
        }
    }

    /// Returns whether this face is a “positive” face: one whose unit vector's nonzero
    /// coordinate is positive.
    ///
    /// ```
    /// use all_is_cubes::math::Face;
    ///
    /// assert_eq!(Face::PX.is_positive(), true);
    /// assert_eq!(Face::NX.is_positive(), false);
    /// assert_eq!(Face::WITHIN.is_positive(), false);
    /// ```
    pub fn is_positive(self) -> bool {
        matches!(self, Face::PX | Face::PY | Face::PZ)
    }

    /// Returns whether this face is a negative face: one whose unit vector's nonzero
    /// coordinate is negative.
    ///
    /// ```
    /// use all_is_cubes::math::Face;
    ///
    /// assert_eq!(Face::PX.is_negative(), false);
    /// assert_eq!(Face::NX.is_negative(), true);
    /// assert_eq!(Face::WITHIN.is_negative(), false);
    /// ```
    pub fn is_negative(self) -> bool {
        matches!(self, Face::NX | Face::NY | Face::NZ)
    }

    /// Returns the opposite face (maps [`PX`](Self::PX) to [`NX`](Self::NX) and so on).
    #[inline]
    pub const fn opposite(self) -> Face {
        match self {
            Face::WITHIN => Face::WITHIN,
            Face::NX => Face::PX,
            Face::NY => Face::PY,
            Face::NZ => Face::PZ,
            Face::PX => Face::NX,
            Face::PY => Face::NY,
            Face::PZ => Face::NZ,
        }
    }

    /// Returns the face whose normal is the cross product of these faces' normals.
    ///
    /// ```
    /// use all_is_cubes::math::Face;
    ///
    /// for &face1 in Face::ALL_SEVEN {
    ///     for &face2 in Face::ALL_SEVEN {
    ///         // Cross product of faces is identical to cross product of vectors.
    ///         assert_eq!(
    ///             face1.cross(face2).normal_vector::<f64>(),
    ///             face1.normal_vector().cross(face2.normal_vector()),
    ///             "{:?} cross {:?}", face1, face2,
    ///         );
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn cross(self, other: Self) -> Self {
        use Face::*;
        match (self, other) {
            // Zero input
            (WITHIN, _) => WITHIN,
            (_, WITHIN) => WITHIN,

            // Equal vectors
            (Face::NX, Face::NX) => WITHIN,
            (Face::NY, Face::NY) => WITHIN,
            (Face::NZ, Face::NZ) => WITHIN,
            (Face::PX, Face::PX) => WITHIN,
            (Face::PY, Face::PY) => WITHIN,
            (Face::PZ, Face::PZ) => WITHIN,

            // Opposite vectors
            (Face::NX, Face::PX) => WITHIN,
            (Face::NY, Face::PY) => WITHIN,
            (Face::NZ, Face::PZ) => WITHIN,
            (Face::PX, Face::NX) => WITHIN,
            (Face::PY, Face::NY) => WITHIN,
            (Face::PZ, Face::NZ) => WITHIN,

            (Face::NX, Face::NY) => PZ,
            (Face::NX, Face::NZ) => NY,
            (Face::NX, Face::PY) => NZ,
            (Face::NX, Face::PZ) => PY,

            (Face::NY, Face::NX) => NZ,
            (Face::NY, Face::NZ) => PX,
            (Face::NY, Face::PX) => PZ,
            (Face::NY, Face::PZ) => NX,

            (Face::NZ, Face::NX) => PY,
            (Face::NZ, Face::NY) => NX,
            (Face::NZ, Face::PX) => NY,
            (Face::NZ, Face::PY) => PX,

            (Face::PX, Face::NY) => NZ,
            (Face::PX, Face::NZ) => PY,
            (Face::PX, Face::PY) => PZ,
            (Face::PX, Face::PZ) => NY,

            (Face::PY, Face::NX) => PZ,
            (Face::PY, Face::NZ) => NX,
            (Face::PY, Face::PX) => NZ,
            (Face::PY, Face::PZ) => PX,

            (Face::PZ, Face::NX) => NY,
            (Face::PZ, Face::NY) => PX,
            (Face::PZ, Face::PX) => PY,
            (Face::PZ, Face::PY) => NX,
        }
    }

    /// Returns the vector normal to this face. [`WITHIN`](Self::WITHIN) is assigned the
    /// zero vector.
    #[inline]
    pub fn normal_vector<S>(self) -> Vector3<S>
    where
        S: BaseNum + std::ops::Neg<Output = S>,
    {
        match self {
            Face::WITHIN => Vector3::new(S::zero(), S::zero(), S::zero()),
            Face::NX => Vector3::new(-S::one(), S::zero(), S::zero()),
            Face::NY => Vector3::new(S::zero(), -S::one(), S::zero()),
            Face::NZ => Vector3::new(S::zero(), S::zero(), -S::one()),
            Face::PX => Vector3::new(S::one(), S::zero(), S::zero()),
            Face::PY => Vector3::new(S::zero(), S::one(), S::zero()),
            Face::PZ => Vector3::new(S::zero(), S::zero(), S::one()),
        }
    }

    /// Returns a homogeneous transformation matrix which, if given points on the square
    /// with x ∈ [0, scale], y ∈ [0, scale] and z = 0, converts them to points that lie
    /// on the faces of the cube with x ∈ [0, scale], y ∈ [0, scale], and z ∈ [0, scale].
    ///
    /// Specifically, `Face::NZ.gmatrix()` is the identity matrix and all others are
    /// consistent with that. Note that there are arbitrary choices in the rotation
    /// of all other faces. (TODO: Document those choices and test them.)
    ///
    /// To work with floating-point coordinates, use `.matrix(1).to_free()`.
    #[rustfmt::skip]
    pub const fn matrix(self, scale: GridCoordinate) -> GridMatrix {
        match self {
            Face::WITHIN => GridMatrix::ZERO,
            Face::NX => GridMatrix::new(
                0, 1, 0,
                0, 0, 1,
                1, 0, 0,
                0, 0, 0,
            ),
            Face::NY => GridMatrix::new(
                0, 0, 1,
                1, 0, 0,
                0, 1, 0,
                0, 0, 0,
            ),
            Face::NZ => GridMatrix::new(
                // Z face leaves X and Y unchanged!
                1, 0, 0,
                0, 1, 0,
                0, 0, 1,
                0, 0, 0,
            ),
            // Positives are same as negatives but with translation and an arbitrary choice of rotation.
            // PX rotates about Y.
            Face::PX => GridMatrix::new(
                0, -1, 0,
                0, 0, 1,
                -1, 0, 0,
                scale, scale, 0,
            ),
            // PY rotates about X.
            Face::PY => GridMatrix::new(
                0, 0, 1,
                -1, 0, 0,
                0, -1, 0,
                scale, scale, 0,
            ),
            // PZ rotates about Y.
            Face::PZ => GridMatrix::new(
                1, 0, 0,
                0, -1, 0,
                0, 0, -1,
                0, scale, scale,
            ),
        }
    }
}

impl TryFrom<GridVector> for Face {
    /// Returns the original vector on failure.
    /// (An error message would probably be too lacking context to be helpful.)
    type Error = GridVector;

    /// Recovers a `Face` from its corresponding unit normal vector. All other vectors
    /// are rejected.
    ///
    /// ```
    /// use all_is_cubes::math::{Face, GridVector};
    /// use std::convert::TryFrom;
    ///
    /// // A Face may be converted from its normal vector.
    /// for &face in Face::ALL_SEVEN {
    ///     assert_eq!(Face::try_from(face.normal_vector()), Ok(face));
    /// }
    ///
    /// // If the vector does not correspond to any Face, it is returned.
    /// let v = GridVector::new(1, 2, 3);
    /// assert_eq!(Face::try_from(v), Err(v));
    /// ```
    fn try_from(value: GridVector) -> Result<Self, Self::Error> {
        use Face::*;
        match value {
            GridVector { x: 0, y: 0, z: 0 } => Ok(WITHIN),
            GridVector { x: 1, y: 0, z: 0 } => Ok(PX),
            GridVector { x: 0, y: 1, z: 0 } => Ok(PY),
            GridVector { x: 0, y: 0, z: 1 } => Ok(PZ),
            GridVector { x: -1, y: 0, z: 0 } => Ok(NX),
            GridVector { x: 0, y: -1, z: 0 } => Ok(NY),
            GridVector { x: 0, y: 0, z: -1 } => Ok(NZ),
            not_unit_vector => Err(not_unit_vector),
        }
    }
}

/// Container for values keyed by [`Face`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaceMap<V> {
    /// The value whose key is `Face::WITHIN`.
    pub within: V,
    /// The value whose key is `Face::NX`.
    pub nx: V,
    /// The value whose key is `Face::NY`.
    pub ny: V,
    /// The value whose key is `Face::NZ`.
    pub nz: V,
    /// The value whose key is `Face::PX`.
    pub px: V,
    /// The value whose key is `Face::PY`.
    pub py: V,
    /// The value whose key is `Face::PZ`.
    pub pz: V,
}

impl<V> FaceMap<V> {
    /// Compute and store a value for each [`Face`] enum variant.
    pub fn generate(mut f: impl FnMut(Face) -> V) -> Self {
        Self {
            within: f(Face::WITHIN),
            nx: f(Face::NX),
            ny: f(Face::NY),
            nz: f(Face::NZ),
            px: f(Face::PX),
            py: f(Face::PY),
            pz: f(Face::PZ),
        }
    }

    /// Access all of the values.
    /// TODO: Return an iterator instead; right now the problem is the iterator won't
    /// own the data until we implement a custom iterator.
    #[rustfmt::skip]
    pub const fn values(&self) -> [&V; 7] {
        [&self.within, &self.nx, &self.ny, &self.nz, &self.px, &self.py, &self.pz]
    }

    /// Transform values.
    ///
    /// TODO: Should wr do this in terms of iterators?
    pub fn map<U>(self, mut f: impl FnMut(Face, V) -> U) -> FaceMap<U> {
        FaceMap {
            within: f(Face::WITHIN, self.within),
            nx: f(Face::NX, self.nx),
            ny: f(Face::NY, self.ny),
            nz: f(Face::NZ, self.nz),
            px: f(Face::PX, self.px),
            py: f(Face::PY, self.py),
            pz: f(Face::PZ, self.pz),
        }
    }

    // TODO: provide more convenience methods for iteration & transformation
}

impl<V> Index<Face> for FaceMap<V> {
    type Output = V;
    fn index(&self, face: Face) -> &V {
        match face {
            Face::WITHIN => &self.within,
            Face::NX => &self.nx,
            Face::NY => &self.ny,
            Face::NZ => &self.nz,
            Face::PX => &self.px,
            Face::PY => &self.py,
            Face::PZ => &self.pz,
        }
    }
}

impl<V> IndexMut<Face> for FaceMap<V> {
    fn index_mut(&mut self, face: Face) -> &mut V {
        match face {
            Face::WITHIN => &mut self.within,
            Face::NX => &mut self.nx,
            Face::NY => &mut self.ny,
            Face::NZ => &mut self.nz,
            Face::PX => &mut self.px,
            Face::PY => &mut self.py,
            Face::PZ => &mut self.pz,
        }
    }
}

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
/// The combination of a `GridPoint` identifying a unit cube and a `Face` identifying
/// one face of it. This pattern recurs in selection and collision detection.
pub struct CubeFace {
    pub cube: GridPoint,
    pub face: Face,
}

impl CubeFace {
    #[inline]
    pub fn new(cube: impl Into<GridPoint>, face: Face) -> Self {
        Self {
            cube: cube.into(),
            face,
        }
    }

    /// Computes the cube that is adjacent in the direction of `self.face`.
    /// Equal to `self.cube` if the face is [`Face::WITHIN`].
    #[inline]
    pub fn adjacent(self) -> GridPoint {
        self.cube + self.face.normal_vector()
    }
}

impl std::fmt::Debug for CubeFace {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            fmt,
            "CubeFace({:?}, {:?})",
            self.cube.as_concise_debug(),
            self.face,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::SquareMatrix as _;

    #[test]
    fn face_matrix_does_not_scale_or_reflect() {
        for &face in Face::ALL_SIX {
            assert_eq!(1.0, face.matrix(7).to_free().determinant());
        }
    }

    // TODO: More tests of face.matrix()

    #[test]
    fn face_map_values_in_enum_order() {
        // TODO: Maybe generalize this to _all_ the Face/FaceMap methods that have an ordering?
        let map = FaceMap::generate(|f| f);
        assert_eq!(
            Face::ALL_SEVEN.to_vec(),
            map.values().iter().copied().copied().collect::<Vec<_>>(),
        )
    }

    // TODO: More Tests of FaceMap

    #[test]
    fn cubeface_format() {
        let cube_face = CubeFace {
            cube: GridPoint::new(1, 2, 3),
            face: Face::NY,
        };
        assert_eq!(&format!("{:#?}", cube_face), "CubeFace((+1, +2, +3), NY)");
    }
}
