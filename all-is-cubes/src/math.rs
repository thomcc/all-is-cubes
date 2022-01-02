// Copyright 2020-2021 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

//! Mathematical utilities and decisions.

use std::fmt;

use cgmath::{EuclideanSpace as _, Point3, Vector3};
use noise::NoiseFn;
use num_traits::identities::Zero;
pub use ordered_float::{FloatIsNan, NotNan};

use crate::util::{ConciseDebug, CustomFormat};

mod aab;
pub use aab::*;
#[macro_use]
mod color;
pub use color::*;
mod face;
pub use face::*;
mod matrix;
pub use matrix::*;
mod rotation;
pub use rotation::*;

/// Coordinates that are locked to the cube grid.
pub type GridCoordinate = i32;
/// Positions that are locked to the cube grid.
pub type GridPoint = Point3<GridCoordinate>;
/// Vectors that are locked to the cube grid.
pub type GridVector = Vector3<GridCoordinate>;
/// Coordinates that are not locked to the cube grid.
pub type FreeCoordinate = f64;

/// Allows writing a [`NotNan`] value as a constant expression  (which is not currently
/// a feature provided by the [`ordered_float`] crate itself).
///
/// ```
/// use all_is_cubes::{notnan, math::NotNan};
///
/// const X: NotNan<f32> = notnan!(1.234);
/// ```
///
/// ```compile_fail
/// # use all_is_cubes::{notnan, math::NotNan};
/// // Not a literal; will not compile
/// const X: NotNan<f32> = notnan!(f32::NAN);
/// ```
///
/// ```compile_fail
/// # use all_is_cubes::{notnan, math::NotNan};
/// // Not a literal; will not compile
/// const X: NotNan<f32> = notnan!(0.0 / 0.0);
/// ```
///
/// ```compile_fail
/// # use all_is_cubes::{notnan, math::NotNan};
/// const N0N: f32 = f32::NAN;
/// // Not a literal; will not compile
/// const X: NotNan<f32> = notnan!(N0N);
/// ```
///
/// ```compile_fail
/// # use all_is_cubes::{notnan, math::NotNan};
/// // Not a float; will not compile
/// const X: NotNan<char> = notnan!('a');
/// ```
#[doc(hidden)]
#[macro_export] // used by all-is-cubes-content
macro_rules! notnan {
    ($value:literal) => {
        match $value {
            value => {
                // Safety: Only literal values are allowed, which will either be a non-NaN
                // float or (as checked below) a type mismatch.
                let result = unsafe { $crate::math::NotNan::new_unchecked(value) };

                // Ensure that the type is one which could have resulted from a float literal,
                // by requiring type unification with a literal. This prohibits char, &str, etc.
                let _ = if false {
                    // Safety: Statically never NaN, and is also never executed.
                    unsafe { $crate::math::NotNan::new_unchecked(0.0) }
                } else {
                    result
                };

                result
            }
        }
    };
}

#[cfg(feature = "arbitrary")]
pub(crate) fn arbitrary_notnan<
    'a,
    T: num_traits::Float + num_traits::FromPrimitive + arbitrary::Arbitrary<'a>,
>(
    u: &mut arbitrary::Unstructured<'a>,
) -> arbitrary::Result<NotNan<T>> {
    let float = u.arbitrary()?;
    match NotNan::new(float) {
        Ok(nn) => Ok(nn),
        Err(FloatIsNan) => {
            let (mantissa, _exponent, sign) = num_traits::Float::integer_decode(float);
            // If our arbitrary float input was a NaN (encoded by exponent = max value),
            // then replace it with a finite float, reusing the mantissa bits.
            let float = T::from_i64(i64::from(sign) * mantissa as i64).unwrap();
            Ok(NotNan::new(float).unwrap())
        }
    }
}

#[inline]
pub(crate) fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    3. * x.powi(2) - 2. * x.powi(3)
}

/// Compute the squared magnitude of a [`GridVector`].
///
/// [`cgmath::InnerSpace::magnitude2`] would do the same but only for floats.
#[inline]
pub(crate) fn int_magnitude_squared(v: GridVector) -> GridCoordinate {
    v.x * v.x + v.y * v.y + v.z * v.z
}

/// Common features of objects that have a location and shape in space.
pub trait Geometry {
    /// Type of coordinates; generally determines whether this object can be translated by a
    /// non-integer amount.
    type Coord;

    /// Translate (move) this object by the specified offset.
    fn translate(self, offset: impl Into<Vector3<Self::Coord>>) -> Self;

    /// Represent this object as a line drawing, or wireframe.
    ///
    /// The generated points should be in pairs, each pair defining a line segment.
    /// If there are an odd number of vertices, the caller should ignore the last.
    ///
    /// TODO: This should probably return an iterator instead, but defining the type
    /// will be awkward until `type_alias_impl_trait` is stable.
    fn wireframe_points<E>(&self, output: &mut E)
    where
        E: Extend<(Point3<FreeCoordinate>, Option<Rgba>)>;
}

/// Extension trait for [`noise::NoiseFn`] which makes it usable with our [`GridPoint`]s.
pub trait NoiseFnExt: NoiseFn<[f64; 3]> {
    /// Sample the noise at the center of the given cube. That is, convert the integer
    /// vector to `f64`, add 0.5 to all coordinates, and call [`NoiseFn::get`].
    ///
    /// This offset is appropriate for the most resolution-independent sampling, or
    /// symmetric shapes with even-numbered widths.
    fn at_cube(&self, cube: GridPoint) -> f64;

    /// As [`NoiseFn::get`], but converting from integer. Unlike [`NoiseFnExt::at_cube`],
    /// does not apply any offset.
    fn at_grid(&self, point: GridPoint) -> f64;
}
impl<T> NoiseFnExt for T
where
    T: NoiseFn<[f64; 3]> + Sized,
{
    fn at_cube(&self, cube: GridPoint) -> f64
    where
        Self: Sized,
    {
        let point = cube.map(f64::from) + Vector3::new(0.5, 0.5, 0.5);
        NoiseFn::get(&self, point.into())
    }

    fn at_grid(&self, point: GridPoint) -> f64
    where
        Self: Sized,
    {
        let point = point.map(f64::from);
        NoiseFn::get(&self, point.into())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "arbitrary")]
    #[test]
    fn arbitrary_notnan_t() {
        use super::{arbitrary_notnan, NotNan};
        use arbitrary::Unstructured;

        // Exhaustively search all patterns of sign and exponent bits plus a few mantissa bits.
        for high_bytes in 0..=u16::MAX {
            let [h1, h2] = high_bytes.to_be_bytes();

            // Should not panic, and should not need more bytes than given.
            let _: NotNan<f32> =
                arbitrary_notnan(&mut Unstructured::new(&[h1, h2, h1, h2])).expect("f32 failure");
            let _: NotNan<f64> =
                arbitrary_notnan(&mut Unstructured::new(&[h1, h2, h1, h2, h1, h2, h1, h2]))
                    .expect("f64 failure");
        }
    }
}
