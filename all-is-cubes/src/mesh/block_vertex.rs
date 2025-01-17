// Copyright 2020-2022 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

//! Mesh vertices.

use std::fmt;

use cgmath::{EuclideanSpace as _, Point3, Vector3};

use crate::math::{Face6, Face7, FreeCoordinate, GridPoint, Rgba};
use crate::space::PackedLight;
use crate::util::{ConciseDebug, CustomFormat as _};

/// Numeric type used to store texture coordinates in vertices.
pub type TextureCoordinate = f32;

/// Basic vertex data type from [`triangulate_block`].
/// Implement <code>[`From`]&lt;[`BlockVertex`]&gt;</code> (and usually [`GfxVertex`])
/// to provide a specialized version fit for the target graphics API.
///
/// [`triangulate_block`]: super::triangulate_block
#[allow(clippy::exhaustive_structs)]
#[derive(Clone, Copy, PartialEq)]
pub struct BlockVertex {
    /// Vertex position.
    pub position: Point3<FreeCoordinate>,
    /// Vertex normal, always axis-aligned.
    pub face: Face6,
    /// Surface color or texture coordinate.
    pub coloring: Coloring,
}

impl BlockVertex {
    /// Remove the clamp information for the sake of tidier tests of one thing at a time.
    #[cfg(test)]
    pub(crate) fn remove_clamps(mut self) -> Self {
        self.coloring = match self.coloring {
            Coloring::Texture {
                pos,
                clamp_min: _,
                clamp_max: _,
            } => Coloring::Texture {
                pos,
                clamp_min: pos,
                clamp_max: pos,
            },
            other => other,
        };
        self
    }
}

/// Describes the two ways a [`BlockVertex`] may be colored; by a solid color or by a texture.
#[allow(clippy::exhaustive_enums)]
#[derive(Clone, Copy, PartialEq)]
pub enum Coloring {
    /// Solid color.
    Solid(Rgba),
    /// Texture coordinates provided by the [`TextureAllocator`](super::TextureAllocator)
    /// for this vertex.
    Texture {
        /// Texture coordinates for this vertex.
        pos: Vector3<TextureCoordinate>,
        /// Lower bounds for clamping the entire surface's texture coordinates.
        /// Used to avoid texture bleed.
        clamp_min: Vector3<TextureCoordinate>,
        /// Upper bounds for clamping the entire surface's texture coordinates.
        /// Used to avoid texture bleed.
        clamp_max: Vector3<TextureCoordinate>,
    },
}

impl fmt::Debug for BlockVertex {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Print compactly on single line even if the formatter is in prettyprint mode.
        write!(
            fmt,
            "{{ p: {:?} n: {:?} c: {:?} }}",
            self.position.custom_format(ConciseDebug),
            self.face,
            self.coloring
        )
    }
}
impl fmt::Debug for Coloring {
    // TODO: test formatting of this
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Coloring::Solid(color) => write!(fmt, "Solid({:?})", color),
            Coloring::Texture { pos, .. } => {
                write!(fmt, "Texture({:?})", pos.custom_format(ConciseDebug))
            }
        }
    }
}

/// A custom representation of [`BlockVertex`] suitable for a specific graphics system.
///
/// The life cycle of a [`GfxVertex`]:
///
/// * First, it is constructed by [`triangulate_block`](super::triangulate_block)
///   for a particular [`Block`] value, and stored in a [`BlockMesh`].
/// * Then, wherever that block appears in a [`Space`], the block vertices are copied
///   to become the [`SpaceMesh`]’s vertices, and [`GfxVertex::instantiate_vertex`] is
///   called on each copy to position it at the particular block's location.
///
/// [`Block`]: crate::block::Block
/// [`BlockMesh`]: crate::mesh::BlockMesh
/// [`Space`]: crate::space::Space
/// [`SpaceMesh`]: crate::mesh::SpaceMesh
pub trait GfxVertex: From<BlockVertex> + Copy + Sized {
    /// Number type for the vertex position coordinates.
    type Coordinate: cgmath::BaseFloat;

    /// Type of the data carried from [`Self::instantiate_block`] to
    /// [`Self::instantiate_vertex`].
    type BlockInst: Copy;

    /// Whether the triangulator should compute light values for this vertex type.
    /// If this is false, the `lighting` parameter to `instantiate_vertex` will be a
    /// constant unrelated to actual light conditions.
    const WANTS_LIGHT: bool;

    /// Prepare the information needed to instantiate vertices of one block.
    /// Currently, this constitutes the location of that block, and hence this function
    /// is responsible for any necessary numeric conversion.
    fn instantiate_block(cube: GridPoint) -> Self::BlockInst;

    /// Transforms a vertex belonging to a general model of a block to its instantiation
    /// in a specific location in space and lighting conditions.
    fn instantiate_vertex(&mut self, block: Self::BlockInst, lighting: PackedLight);

    /// Returns the position of this vertex.
    ///
    /// Note: This is used to perform depth sorting for transparent vertices.
    fn position(&self) -> Point3<Self::Coordinate>;

    /// Returns the normal of this vertex, expressed as a [`Face7`],
    /// with [`Face7::Within`] representing any error cases.
    fn face(&self) -> Face7;
}

/// Trivial implementation of [`GfxVertex`] for testing purposes. Discards lighting.
impl GfxVertex for BlockVertex {
    type Coordinate = FreeCoordinate;
    type BlockInst = Vector3<FreeCoordinate>;
    const WANTS_LIGHT: bool = false;

    fn position(&self) -> Point3<FreeCoordinate> {
        self.position
    }

    #[inline]
    fn instantiate_block(cube: GridPoint) -> Self::BlockInst {
        cube.to_vec().map(FreeCoordinate::from)
    }

    #[inline]
    fn instantiate_vertex(&mut self, offset: Self::BlockInst, _lighting: PackedLight) {
        self.position += offset;
    }

    #[inline]
    fn face(&self) -> Face7 {
        self.face.into()
    }
}
