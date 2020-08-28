// Copyright 2020 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <http://opensource.org/licenses/MIT>.

//! Algorithms for converting blocks/voxels to triangle-based rendering
//! (as opposed to raytracing, voxel display hardware, or whatever else).
//!
//! All of the algorithms here are independent of graphics API but may presume that
//! one exists and has specific data types to specialize in.
//!
//! Note: Some sources say that “tesselation” would be a better name for this
//! operation than “triangulation”. However, “tesselation” means a specific
//! other operation in OpenGL graphics programming, and “triangulation” seems to
//! be the more commonly used terms.

use cgmath::{EuclideanSpace as _, Point3, Transform as _, Vector3};

use crate::block::{Block};
use crate::math::{Face, FaceMap, FreeCoordinate, GridCoordinate, RGBA};
use crate::lighting::PackedLight;
use crate::space::{Space};
use crate::util::{ConciseDebug as _};

pub type TextureCoordinate = f32;

/// Generic structure of output from triangulator. Implement `GfxVertex`
/// to provide a specialized version.
#[derive(Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct BlockVertex {
    pub position: Point3<FreeCoordinate>,
    pub normal: Vector3<FreeCoordinate>,  // TODO: Use a smaller number type? Storage vs convenience?
    // TODO: Eventually color will be fully replaced with texture coordinates.
    pub coloring: Coloring,
}
#[derive(Clone, Copy, PartialEq)]
pub enum Coloring {
    Solid(RGBA),
    Texture(Vector3<TextureCoordinate>),
}

impl std::fmt::Debug for BlockVertex {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        // Print compactly on single line even if the formatter is in prettyprint mode.
        write!(fmt, "{{ p: {:?} n: {:?} c: {:?} }}",
            self.position.as_concise_debug(),
            self.normal.cast::<i8>().unwrap().as_concise_debug(),  // no decimals!
            self.coloring)
    }
}
impl std::fmt::Debug for Coloring {
    // TODO: test formatting of this
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Coloring::Solid(color) => write!(fmt, "Solid({:?})", color),
            Coloring::Texture(tc) => write!(fmt, "Texture({:?})", tc.as_concise_debug()),
        }
    }
}

/// Implement this trait along with `From<BlockVertex>` to provide a representation
/// of `BlockVertex` suitable for the target graphics system.
pub trait ToGfxVertex<GV>: From<BlockVertex> + Sized {
    fn instantiate(&self, offset: Vector3<FreeCoordinate>, lighting: PackedLight) -> GV;
}

/// Trivial implementation for testing purposes. Discards lighting.
impl ToGfxVertex<BlockVertex> for BlockVertex {
    fn instantiate(&self, offset: Vector3<FreeCoordinate>, _lighting: PackedLight) -> Self {
        Self {
            position: self.position + offset,
            ..*self
        }
    }
}

/// Describes how to draw one `Face` of a `Block`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FaceRenderData<V: From<BlockVertex>> {
    /// Vertices of triangles (i.e. length is a multiple of 3) in counterclockwise order.
    vertices: Box<[V]>,
    /// Whether the block entirely fills its cube, such that nothing can be seen through
    /// it and faces of adjacent blocks may be removed.
    fully_opaque: bool,
}

impl<V: From<BlockVertex>> Default for FaceRenderData<V> {
    fn default() -> Self {
        FaceRenderData {
            vertices: Box::new([]),
            fully_opaque: false,
        }
    }
}

/// Describes how to draw a block. Pass it to `triangulate_space` to use it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockRenderData<V: From<BlockVertex>, A: TextureAllocator> {
    /// Vertices grouped by the face they belong to.
    ///
    /// All triangles which are on the surface of the cube (such that they may be omitted
    /// when a `fully_opaque` block is adjacent) are grouped under the corresponding
    /// face, and all other triangles are grouped under `Face::WITHIN`.
    faces: FaceMap<FaceRenderData<V>>,

    textures_used: Vec<A::Tile>,
}

impl<V: From<BlockVertex>, A: TextureAllocator> Default for BlockRenderData<V, A> {
    fn default() -> Self {
        Self {
            faces: FaceMap::generate(|_| FaceRenderData::default()),
            textures_used: Vec::new(),
        }
    }
}

/// Collection of `BlockRenderData` indexed by a `Space`'s block indices.
/// Pass it to `triangulate_space` to use it.
pub type BlocksRenderData<V, A> = Box<[BlockRenderData<V, A>]>;

const QUAD_VERTICES: &[Point3<FreeCoordinate>; 6] = &[
    // Two-triangle quad.
    // Note that looked at from a X-right Y-up view, these triangles are
    // clockwise, but they're properly counterclockwise from the perspective
    // that we're drawing the face _facing towards negative Z_ (into the screen),
    // which is how cube faces as implicitly defined by Face::matrix work.
    Point3::new(0.0, 0.0, 0.0),
    Point3::new(0.0, 1.0, 0.0),
    Point3::new(1.0, 0.0, 0.0),
    Point3::new(1.0, 0.0, 0.0),
    Point3::new(0.0, 1.0, 0.0),
    Point3::new(1.0, 1.0, 0.0),
];

fn push_quad_solid<V: From<BlockVertex>>(
    vertices: &mut Vec<V>,
    face: Face,
    color: RGBA,
) {
    let transform = face.matrix();
    for &p in QUAD_VERTICES {
        vertices.push(V::from(BlockVertex {
            position: transform.transform_point(p),
            normal: face.normal_vector(),
            coloring: Coloring::Solid(color),
        }));
    }
}

fn push_quad_textured<V: From<BlockVertex>>(
    vertices: &mut Vec<V>,
    face: Face,
    depth: FreeCoordinate,
    texture_index_coordinate: TextureCoordinate,
) {
    let transform = face.matrix();
    for &p in QUAD_VERTICES {
        vertices.push(V::from(BlockVertex {
            position: transform.transform_point(p + Vector3::new(0.0, 0.0, depth)),
            normal: face.normal_vector(),
            coloring: Coloring::Texture(Vector3::new(
                p.x as TextureCoordinate,
                p.y as TextureCoordinate,
                texture_index_coordinate)),
        }));
    }
}

/// Generate `BlockRenderData` for a block.
fn triangulate_block<V: From<BlockVertex>, A: TextureAllocator>(
    // TODO: Arrange to pass in a buffer of old data such that we can reuse existing textures.
    // This will allow for efficient implementation of animated blocks.
    block :&Block,
    texture_allocator: &mut A,
) -> BlockRenderData<V, A> {
    match block {
        Block::Atom(_attributes, color) => {
            let faces = FaceMap::generate(|face| {
                if face == Face::WITHIN {
                    // No interior detail for atom blocks.
                    return FaceRenderData::default();
                }

                let mut face_vertices: Vec<V> = Vec::new();
                let fully_opaque = color.binary_opaque();

                // TODO: Port over pseudo-transparency mechanism, then change this to a
                // within-epsilon-of-zero test. ...conditional on `GfxVertex` specifying support.
                if fully_opaque {
                    push_quad_solid(&mut face_vertices, face, *color);
                }

                FaceRenderData {
                    vertices: face_vertices.into_boxed_slice(),
                    fully_opaque,
                }
            });

            BlockRenderData {
                faces,
                textures_used: vec![],
            }
        }
        Block::Recur(_attributes, space_ref) => {
            let space = &*space_ref.borrow();
            let grid = space.grid();
            let mut vertices_by_face: FaceMap<Vec<V>> = FaceMap::generate(|_| Vec::new());
            let mut textures_used = Vec::new();

            let tile_size: GridCoordinate = grid.size()[0];  // TODO: have a general policy about what if the space is the wrong size.

            for &face in Face::ALL_SIX {
                let vertices = &mut vertices_by_face[face];  // TODO split by interior and not
                let transform = face.matrix();

                for layer in 0..tile_size {
                    // TODO: JS version would detect fully-opaque blocks (a derived property of Block)
                    // and only scan the first and last faces
                    let mut tile_texels: Vec<(u8, u8, u8, u8)> = Vec::with_capacity((tile_size * tile_size) as usize);
                    let mut layer_is_visible_somewhere = false;
                    for t in 0..tile_size {
                        for s in 0..tile_size {
                            // TODO: Matrix4 isn't allowed to be integer. Provide a better strategy.
                            let cube :Point3<GridCoordinate> = transform.transform_point(Point3::new(s as FreeCoordinate, t as FreeCoordinate, layer as FreeCoordinate)).cast::<GridCoordinate>().unwrap();

                            let obscuring_cube = cube + face.normal_vector();
                            let obscured = space[obscuring_cube].color().alpha() >= 1.0;  // TODO formalize test consistency
                            if !obscured {
                                layer_is_visible_somewhere = true;
                            }

                            tile_texels.push(space[cube].color().to_saturating_8bpp());
                        }
                    }
                    if layer_is_visible_somewhere {
                        let mut texture_tile = texture_allocator.allocate();
                        texture_tile.write(tile_texels.as_ref());
                        push_quad_textured(
                            vertices,
                            face, 
                            layer as FreeCoordinate / tile_size as FreeCoordinate,
                            texture_tile.index());
                        textures_used.push(texture_tile);
                    }
                }
            }

            let faces = FaceMap::generate(|face| {
                FaceRenderData {
                    vertices: vertices_by_face[face].drain(..).collect(),  // TODO awkward move
                    fully_opaque: true,  // TODO: actually calculate this
                }
            });
            BlockRenderData { faces, textures_used }
        }
    }
}

/// Precomputes vertices for blocks present in a space.
///
/// The resulting `Vec` is indexed by the `Space`'s internal unstable IDs.
pub fn triangulate_blocks<V: From<BlockVertex>, A: TextureAllocator>(
    space: &Space,
    texture_allocator: &mut A,
) -> BlocksRenderData<V, A> {
    space.distinct_blocks_unfiltered().iter()
        .map(|b| triangulate_block(b, texture_allocator))
        .collect()
}

/// Allocate an output buffer for `triangulate_space`.
pub fn new_space_buffer<V>() -> FaceMap<Vec<V>> {
    FaceMap::generate(|_| Vec::new())
}

/// Computes a triangle-based representation of a `Space` for rasterization.
///
/// `blocks_render_data` should be provided by `triangulate_blocks` and must be up to
/// date (TODO: provide a means to ensure it is up to date).
///
/// The triangles will be written into `output_vertices`, replacing the existing
/// contents. This is intended to avoid memory reallocation in the common case of
/// new geometry being similar to old geometry.
///
/// `output_vertices` is a `FaceMap` dividing the faces according to their normal
/// vectors.
pub fn triangulate_space<BV, GV, A>(
    space: &Space,
    blocks_render_data: &BlocksRenderData<BV, A>,
    output_vertices: &mut FaceMap<Vec<GV>>,
) where
    BV: ToGfxVertex<GV>,
    A: TextureAllocator,
{
    // TODO: take a Grid parameter for chunked rendering

    let empty_render = BlockRenderData::<BV, A>::default();
    let lookup = |cube| {
        match space.get_block_index(cube) {
            // TODO: On out-of-range, draw an obviously invalid block instead of an invisible one.
            Some(index) => &blocks_render_data.get(index as usize).unwrap_or(&empty_render),
            None => &empty_render,
        }
    };

    for &face in Face::ALL_SEVEN.iter() {
        // use the buffer but not the existing data
        output_vertices[face].clear();
    }
    for cube in space.grid().interior_iter() {
        let precomputed = lookup(cube);
        let low_corner = cube.cast::<FreeCoordinate>().unwrap();
        for &face in Face::ALL_SEVEN {
            let adjacent_cube = cube + face.normal_vector();
            if lookup(adjacent_cube).faces[face.opposite()].fully_opaque {
                // Don't draw obscured faces
                continue;
            }

            let lighting = space.get_lighting(adjacent_cube);

            // Copy vertices, offset to the block position and with lighting
            for vertex in precomputed.faces[face].vertices.iter() {
                output_vertices[face].push(
                    vertex.instantiate(low_corner.to_vec(), lighting));
            }
        }
    }
}

pub type Texel = (u8, u8, u8, u8);

/// Allocator of 2D textures to paint block faces into.
pub trait TextureAllocator {
    type Tile: TextureTile;
    fn allocate(&mut self) -> Self::Tile;
}

/// 2D texture to paint block faces into. It is assumed that when this value is dropped,
/// the texture allocation will be released.
pub trait TextureTile {
    /// Value to write into the third texture coordinate component to indicate this tile.
    fn index(&self) -> TextureCoordinate;

    /// Write texture data as RGBA color.
    fn write(&mut self, data: &[Texel]);
}

/// `TextureAllocator` which discards all input; for testing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NullTextureAllocator;
impl TextureAllocator for NullTextureAllocator {
    type Tile = ();
    fn allocate(&mut self) {}
}
impl TextureTile for () {
    fn index(&self) -> TextureCoordinate { 0.0 }
    fn write(&mut self, _data: &[(u8, u8, u8, u8)]) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{MetricSpace as _};
    use crate::block::BlockAttributes;
    use crate::blockgen::make_some_blocks;
    use crate::universe::{Universe};

    #[test]
    fn excludes_interior_faces() {
        let block = make_some_blocks(1).swap_remove(0);
        let mut space = Space::empty_positive(2, 2, 2);
        for cube in space.grid().interior_iter() {
            space.set(cube, &block);
        }

        let mut rendering = new_space_buffer();
        triangulate_space::<BlockVertex, BlockVertex, NullTextureAllocator>(
            &space,
            &triangulate_blocks(&space, &mut NullTextureAllocator),
            &mut rendering);
        let rendering_flattened: Vec<BlockVertex> = rendering.values().iter().flat_map(|r| (*r).clone()).collect();
        assert_eq!(
            Vec::<&BlockVertex>::new(),
            rendering_flattened.iter()
                .filter(|vertex|
                        vertex.position.distance2(Point3::new(1.0, 1.0, 1.0)) < 0.99)
                .collect::<Vec<&BlockVertex>>(),
            "found an interior point");
        assert_eq!(rendering_flattened.len(),
            6 /* vertices per face */
            * 4 /* block faces per exterior side of space */
            * 6 /* sides of space */,
            "wrong number of faces");
    }

    #[test]
    fn no_panic_on_missing_blocks() {
        let block = make_some_blocks(1).swap_remove(0);
        let mut space = Space::empty_positive(2, 1, 1);
        let blocks_render_data: BlocksRenderData<BlockVertex, _> =
            triangulate_blocks(&space, &mut NullTextureAllocator);
        assert_eq!(blocks_render_data.len(), 1);  // check our assumption

        // This should not panic; visual glitches are preferable to failure.
        space.set((0, 0, 0), &block);  // render data does not know about this
        triangulate_space(&space, &blocks_render_data, &mut new_space_buffer());
    }

    #[test]
    fn trivial_subcube_rendering() {
        let mut u = Universe::new();
        let mut inner_block_space = Space::empty_positive(1, 1, 1);
        inner_block_space.set((0, 0, 0), &make_some_blocks(1)[0]);
        let inner_block = Block::Recur(BlockAttributes::default(), u.insert_anonymous(inner_block_space));
        let mut outer_space = Space::empty_positive(1, 1, 1);
        outer_space.set((0, 0, 0), &inner_block);

        let blocks_render_data: BlocksRenderData<BlockVertex, _> =
            triangulate_blocks(&outer_space, &mut NullTextureAllocator);
        let block_render_data: BlockRenderData<_, _> = blocks_render_data[0].clone();

        eprintln!("{:#?}", blocks_render_data);
        let mut space_rendered = new_space_buffer();
        triangulate_space(&outer_space, &blocks_render_data, &mut space_rendered);
        eprintln!("{:#?}", space_rendered);

        assert_eq!(space_rendered, block_render_data.faces.map(|_, frd| frd.vertices.to_vec()));
    }

    // TODO: more tests
}
