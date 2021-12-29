// Copyright 2020-2021 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

//! Core data types for graphics code to use.

use cgmath::{EuclideanSpace as _, Point3, Vector3};
use luminance::{Semantics, Vertex};

use crate::math::{Face, FreeCoordinate, GridCoordinate, GridPoint, GridVector, Rgba};
use crate::mesh::{BlockVertex, Coloring, GfxVertex};
use crate::space::PackedLight;

/// Module to isolate the `luminance::backend` traits which have conflicting names.
mod backend {
    use super::*;
    use luminance::backend::{
        framebuffer::FramebufferBackBuffer,
        pipeline::{Pipeline, PipelineTexture},
        render_gate::RenderGate,
        shader::{Shader, Uniformable},
        tess::{IndexSlice, VertexSlice},
        tess_gate::TessGate,
    };
    use luminance::pipeline::TextureBinding;
    use luminance::pixel::{NormRGBA8UI, NormUnsigned, SRGBA8UI};
    use luminance::shader::types::{Mat44, Vec3};
    use luminance::tess::Interleaved;
    use luminance::texture::{Dim2, Dim3};

    /// Trait alias for a [`luminance`] backend which implements all the
    /// functionality that we need.
    pub trait AicLumBackend:
        Sized
        + FramebufferBackBuffer
        + Pipeline<Dim2>
        + PipelineTexture<Dim2, NormRGBA8UI>
        + PipelineTexture<Dim3, NormRGBA8UI>
        + PipelineTexture<Dim3, SRGBA8UI>
        + RenderGate
        + Shader
        + TessGate<(), (), (), Interleaved>
        + TessGate<LinesVertex, (), (), Interleaved>
        + TessGate<LinesVertex, u32, (), Interleaved>
        + TessGate<LumBlockVertex, (), (), Interleaved>
        + TessGate<LumBlockVertex, u32, (), Interleaved>
        + for<'a> Uniformable<'a, f32, Target = f32>
        + for<'a> Uniformable<'a, Vec3<i32>, Target = Vec3<i32>>
        + for<'a> Uniformable<'a, Vec3<f32>, Target = Vec3<f32>>
        + for<'a> Uniformable<'a, Mat44<f32>, Target = Mat44<f32>>
        + for<'a> Uniformable<
            'a,
            TextureBinding<Dim2, NormUnsigned>,
            Target = TextureBinding<Dim2, NormUnsigned>,
        > + for<'a> Uniformable<
            'a,
            TextureBinding<Dim3, NormUnsigned>,
            Target = TextureBinding<Dim3, NormUnsigned>,
        > + for<'a> VertexSlice<'a, LumBlockVertex, u32, (), Interleaved, LumBlockVertex>
        + for<'a> IndexSlice<'a, LumBlockVertex, u32, (), Interleaved>
    //TODO: (): luminance::backend::depth_slot::DepthSlot<Self, Dim2>,
    {
    }
    impl<B> AicLumBackend for B where
        Self: Sized
            + FramebufferBackBuffer
            + Pipeline<Dim2>
            + PipelineTexture<Dim2, NormRGBA8UI>
            + PipelineTexture<Dim3, NormRGBA8UI>
            + PipelineTexture<Dim3, SRGBA8UI>
            + RenderGate
            + Shader
            + TessGate<(), (), (), Interleaved>
            + TessGate<LinesVertex, (), (), Interleaved>
            + TessGate<LinesVertex, u32, (), Interleaved>
            + TessGate<LumBlockVertex, (), (), Interleaved>
            + TessGate<LumBlockVertex, u32, (), Interleaved>
            + for<'a> Uniformable<'a, f32, Target = f32>
            + for<'a> Uniformable<'a, Vec3<i32>, Target = Vec3<i32>>
            + for<'a> Uniformable<'a, Vec3<f32>, Target = Vec3<f32>>
            + for<'a> Uniformable<'a, Mat44<f32>, Target = Mat44<f32>>
            + for<'a> Uniformable<
                'a,
                TextureBinding<Dim2, NormUnsigned>,
                Target = TextureBinding<Dim2, NormUnsigned>,
            > + for<'a> Uniformable<
                'a,
                TextureBinding<Dim3, NormUnsigned>,
                Target = TextureBinding<Dim3, NormUnsigned>,
            > + for<'a> VertexSlice<'a, LumBlockVertex, u32, (), Interleaved, LumBlockVertex>
            + for<'a> IndexSlice<'a, LumBlockVertex, u32, (), Interleaved>
    {
    }
}
pub use backend::AicLumBackend;

/// Defines vertex array structure for luminance framework.
/// Note that each "wrapper" names a new type generated by the `derive(Semantics)`.
#[derive(Copy, Clone, Debug, Semantics)]
#[rustfmt::skip]
pub enum VertexSemantics {
    // TODO: revisit compact representations
    /// Vertex position.
    #[sem(name = "a_position", repr = "[f32; 3]", wrapper = "VertexPosition")]
    Position,
    /// Position of the cube containing the triangle this vertex belongs to.
    /// Note that this is not the same as floor() of `Position`.
    ///
    /// TODO: Once we implement storing chunks in relative coordinates for better
    /// precision, we can reduce this representation size down to i8 or u8.
    #[sem(name = "a_cube", repr = "[f32; 3]", wrapper = "VertexCube")]
    Cube,
    /// Vertex normal (should be length 1).
    #[sem(name = "a_normal", repr = "[f32; 3]", wrapper = "VertexNormal")]
    Normal,
    /// Packed format:
    /// * If `[3]` is in the range 0.0 to 1.0, then the attribute is a solid RGBA color.
    /// * If `[3]` is -1.0, then the first three components are 3D texture coordinates.
    #[sem(name = "a_color_or_texture", repr = "[f32; 4]", wrapper = "VertexColorOrTexture")]
    ColorOrTexture,
    /// Interpolated texture coordinates are clamped to be ≥ this value, to avoid bleeding.
    #[sem(name = "a_clamp_min", repr = "[f32; 3]", wrapper = "VertexClampLow", unbound)]
    ClampLow,
    /// Interpolated texture coordinates are clamped to be ≤ this value, to avoid bleeding.
    #[sem(name = "a_clamp_max", repr = "[f32; 3]", wrapper = "VertexClampHigh")]
    ClampHigh,
}

/// Vertex type sent to shader for rendering blocks (and, for the moment, other geometry,
/// but its attributes are heavily focused on blocks).
/// See [`VertexSemantics`] for the meaning of the fields.
///
/// Note: this struct must be declared `pub` because it is used in the trait bounds of
/// [`AicLumBackend`], but it is not actually accessible outside the crate.
#[derive(Clone, Copy, Debug, PartialEq, Vertex)]
#[vertex(sem = "VertexSemantics")]
pub struct LumBlockVertex {
    position: VertexPosition,
    cube: VertexCube,
    normal: VertexNormal,
    color_or_texture: VertexColorOrTexture,
    clamp_min: VertexClampLow,
    clamp_max: VertexClampHigh,
}

impl LumBlockVertex {
    /// A vertex which will not be rendered.
    pub const DUMMY: Self = Self {
        position: VertexPosition::new([f32::INFINITY, f32::INFINITY, f32::INFINITY]),
        cube: VertexCube::new([0., 0., 0.]),
        normal: VertexNormal::new([0., 0., 0.]),
        color_or_texture: VertexColorOrTexture::new([0., 0., 0., 0.]),
        clamp_min: VertexClampLow::new([0., 0., 0.]),
        clamp_max: VertexClampHigh::new([0., 0., 0.]),
    };

    /// Constructor taking our natural types instead of luminance specialized types.
    #[inline]
    pub fn new_colored(
        position: Point3<FreeCoordinate>,
        normal: Vector3<FreeCoordinate>,
        color: Rgba,
    ) -> Self {
        Self {
            position: VertexPosition::new(position.cast::<f32>().unwrap().into()),
            cube: VertexCube::new(position.map(|s| s.floor() as f32).into()),
            normal: VertexNormal::new(normal.cast::<f32>().unwrap().into()),
            color_or_texture: VertexColorOrTexture::new(color.into()),
            clamp_min: VertexClampLow::new([0., 0., 0.]),
            clamp_max: VertexClampHigh::new([0., 0., 0.]),
        }
    }

    /// Make an axis-aligned rectangle. Convenience for debug code.
    /// Assumes `size` is positive.
    #[allow(dead_code)]
    pub fn rectangle(
        origin: Vector3<f32>,
        size: Vector3<f32>,
        tex_origin: Vector3<f32>,
        tex_size: Vector3<f32>,
    ) -> Box<[Self; 6]> {
        let normal: Vector3<f32> = Vector3::new(0.0, 0.0, 1.0);
        let dx = Vector3::new(size.x, 0.0, 0.0);
        let dy = Vector3::new(0.0, size.y, 0.0);
        let tdx = Vector3::new(tex_size.x, 0.0, 0.0);
        let tdy = Vector3::new(0.0, tex_size.y, 0.0);
        let v = |p: Vector3<f32>, t: Vector3<f32>| Self {
            position: VertexPosition::new(p.into()),
            cube: VertexCube::new(p.map(|c| c.floor() as f32).into()),
            normal: VertexNormal::new(normal.into()),
            color_or_texture: VertexColorOrTexture::new(t.extend(-1.0).into()),
            clamp_min: VertexClampLow::new([0., 0., 0.]),
            clamp_max: VertexClampHigh::new([0., 0., 0.]),
        };
        Box::new([
            v(origin, tex_origin),
            v(origin + dx, tex_origin + tdx),
            v(origin + dx + dy, tex_origin + tdx + tdy),
            v(origin + dx + dy, tex_origin + tdx + tdy),
            v(origin + dy, tex_origin + tdy),
            v(origin, tex_origin),
        ])
    }
}

impl From<BlockVertex> for LumBlockVertex {
    #[inline]
    fn from(vertex: BlockVertex) -> Self {
        let position = vertex.position.cast::<f32>().unwrap().to_vec();
        let cube = VertexCube::new([0., 0., 0.]);
        let normal = VertexNormal::new(vertex.face.normal_vector::<f32>().into());
        match vertex.coloring {
            Coloring::Solid(color) => {
                let mut color_attribute = VertexColorOrTexture::new(color.into());
                // Clamp out-of-range alpha values so they fit into the
                // VertexColorOrTexture protocol (not less than zero).
                color_attribute[3] = color_attribute[3].min(1.).max(0.);
                Self {
                    position: VertexPosition::new(position.into()),
                    cube,
                    normal,
                    color_or_texture: color_attribute,
                    clamp_min: VertexClampLow::new([0., 0., 0.]),
                    clamp_max: VertexClampHigh::new([0., 0., 0.]),
                }
            }
            Coloring::Texture {
                pos: tc,
                clamp_min,
                clamp_max,
            } => Self {
                position: VertexPosition::new(position.into()),
                cube,
                normal,
                color_or_texture: VertexColorOrTexture::new([tc[0], tc[1], tc[2], -1.0]),
                clamp_min: VertexClampLow::new(clamp_min.into()),
                clamp_max: VertexClampHigh::new(clamp_max.into()),
            },
        }
    }
}

impl GfxVertex for LumBlockVertex {
    type Coordinate = f32;
    type BlockInst = Vector3<f32>;
    const WANTS_LIGHT: bool = false;

    #[inline]
    fn instantiate_block(cube: GridPoint) -> Self::BlockInst {
        cube.to_vec().map(|s| s as f32)
    }

    #[inline]
    fn instantiate_vertex(&mut self, cube: Self::BlockInst, _lighting: PackedLight) {
        self.position.repr[0] += cube.x;
        self.position.repr[1] += cube.y;
        self.position.repr[2] += cube.z;
        self.cube.repr = cube.into();
    }

    #[inline]
    fn position(&self) -> Point3<Self::Coordinate> {
        Point3::from(self.position.repr)
    }

    #[inline]
    fn face(&self) -> Face {
        let normal: GridVector = Vector3::from(self.normal.repr).map(|c| c as GridCoordinate);
        Face::try_from(normal).unwrap_or(Face::Within)
    }
}

/// Vertex type for drawing marker and debug shapes.
///
/// Note: this struct must be declared `pub` because it is used in the trait bounds of
/// [`AicLumBackend`], but it is not actually accessible outside the crate.
#[derive(Clone, Copy, Debug, PartialEq, Vertex)]
#[vertex(sem = "VertexSemantics")]
pub struct LinesVertex {
    direct_position: VertexPosition,
    color: VertexColorOrTexture,
}

impl LinesVertex {
    /// Can't use name `new` because luminance::Vertex grabs it
    #[inline]
    pub(crate) fn new_basic(direct_position: Point3<FreeCoordinate>, color: Rgba) -> Self {
        Self {
            direct_position: VertexPosition::new(direct_position.cast::<f32>().unwrap().into()),
            color: VertexColorOrTexture::new(color.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Face, Rgb};
    use cgmath::Vector3;

    #[test]
    fn vertex_dummy() {
        assert!(!LumBlockVertex::DUMMY.position.repr[0].is_finite());
    }

    #[test]
    fn vertex_new_colored() {
        let vertex = LumBlockVertex::new_colored(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(4.0, 5.0, 6.0),
            Rgba::new(7.0, 8.0, 9.0, 0.5),
        );
        assert_eq!(vertex.position.repr, [1.0, 2.0, 3.0]);
        assert_eq!(vertex.normal.repr, [4.0, 5.0, 6.0]);
        assert_eq!(vertex.color_or_texture.repr, [7.0, 8.0, 9.0, 0.5]);
    }

    /// Full path used by normal rendering.
    #[test]
    fn vertex_from_block_vertex() {
        let block_vertex = BlockVertex {
            position: Point3::new(1.0, 2.1, 3.0),
            face: Face::PX,
            coloring: Coloring::Solid(Rgba::new(7.0, 8.0, 9.0, 0.5)),
        };
        let mut vertex = LumBlockVertex::from(block_vertex);
        vertex.instantiate_vertex(
            LumBlockVertex::instantiate_block(Point3::new(10, 20, 30)),
            Rgb::new(1.0, 0.0, 2.0).into(),
        );
        assert_eq!(vertex.position.repr, [11., 22.1, 33.]);
        assert_eq!(vertex.cube.repr, [10., 20., 30.]);
        assert_eq!(vertex.normal.repr, [1.0, 0.0, 0.0]);
        assert_eq!(vertex.color_or_texture.repr, [7.0, 8.0, 9.0, 0.5]);
    }
}
