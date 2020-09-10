// Copyright 2020 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <http://opensource.org/licenses/MIT>.

//! Key data types for graphics code to use.

use cgmath::{EuclideanSpace as _, Point3, Vector3};
use luminance_derive::{Semantics, Vertex};

use crate::math::{FreeCoordinate, RGBA};
use crate::space::PackedLight;
use crate::triangulator::{BlockVertex, Coloring, ToGfxVertex};

/// Defines vertex array structure for luminance framework.
/// Note that each "wrapper" names a new type generated by the derive(Semantics).
#[derive(Copy, Clone, Debug, Semantics)]
#[rustfmt::skip]
pub enum VertexSemantics {
    // TODO: revisit compact representations
    #[sem(name = "a_position", repr = "[f32; 3]", wrapper = "VertexPosition")]
    Position,
    #[sem(name = "a_normal", repr = "[f32; 3]", wrapper = "VertexNormal")]
    Normal,
    /// Packed format:
    /// * If [3] is in the range 0.0 to 1.0, then the attribute is a solid RGBA color.
    /// * If [3] is -1.0, then the first three components are array texture coordinates.
    #[sem(name = "a_color_or_texture", repr = "[f32; 4]", wrapper = "VertexColorOrTexture")]
    ColorOrTexture,
    // TODO: look into packed repr for lighting
    #[sem(name = "a_lighting", repr = "[f32; 3]", wrapper = "VertexLighting")]
    Lighting,
}

/// Vertex type sent to GPU. See also `GLBlockVertex`
#[derive(Clone, Copy, Debug, Vertex)]
#[vertex(sem = "VertexSemantics")]
pub struct Vertex {
    #[allow(dead_code)] // read by shader
    position: VertexPosition,

    #[allow(dead_code)] // read by shader
    normal: VertexNormal,

    #[allow(dead_code)] // read by shader
    color_or_texture: VertexColorOrTexture,

    #[allow(dead_code)] // read by shader
    lighting: VertexLighting,
}

impl Vertex {
    /// A vertex which will not be rendered.
    pub const DUMMY: Vertex = Vertex {
        position: VertexPosition::new([f32::INFINITY, f32::INFINITY, f32::INFINITY]),
        normal: VertexNormal::new([0.0, 0.0, 0.0]),
        color_or_texture: VertexColorOrTexture::new([0.0, 0.0, 0.0, 0.0]),
        lighting: VertexLighting::new([0.0, 0.0, 0.0]),
    };

    /// Constructor taking our natural types instead of luminance specialized types.
    pub fn new_colored(
        position: Point3<FreeCoordinate>,
        normal: Vector3<FreeCoordinate>,
        color: RGBA,
    ) -> Self {
        Self {
            position: VertexPosition::new(position.cast::<f32>().unwrap().into()),
            normal: VertexNormal::new(normal.cast::<f32>().unwrap().into()),
            color_or_texture: VertexColorOrTexture::new(color.into()),
            lighting: VertexLighting::new([1.0, 1.0, 1.0]),
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
        let v = |p: Vector3<f32>, t: Vector3<f32>| Vertex {
            position: VertexPosition::new(p.into()),
            normal: VertexNormal::new(normal.into()),
            color_or_texture: VertexColorOrTexture::new(t.extend(-1.0).into()),
            lighting: VertexLighting::new([1.0, 1.0, 1.0]),
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

/// Vertex format for `triangulate_blocks` that is as close as possible to
/// `Vertex` without containing unnecessary parts.
pub struct GLBlockVertex {
    position: Vector3<f32>,
    normal: VertexNormal,
    color_or_texture: VertexColorOrTexture,
}

impl From<BlockVertex> for GLBlockVertex {
    fn from(vertex: BlockVertex) -> Self {
        Self {
            position: vertex.position.cast::<f32>().unwrap().to_vec(),
            normal: VertexNormal::new(vertex.normal.cast::<f32>().unwrap().into()),
            color_or_texture: match vertex.coloring {
                Coloring::Solid(color) => {
                    let mut color_attribute = VertexColorOrTexture::new(color.into());
                    // Force alpha to 1 until we have a better transparency answer. When we do,
                    // also make sure to clamp it to meet the VertexColorOrTexture protocol.
                    color_attribute[3] = 1.0;
                    color_attribute
                }
                Coloring::Texture(tc) => VertexColorOrTexture::new([tc[0], tc[1], tc[2], -1.0]),
            },
        }
    }
}

impl ToGfxVertex<Vertex> for GLBlockVertex {
    type Coordinate = f32;

    fn instantiate(&self, offset: Vector3<Self::Coordinate>, lighting: PackedLight) -> Vertex {
        Vertex {
            position: VertexPosition::new((self.position + offset).into()),
            normal: self.normal,
            color_or_texture: self.color_or_texture,
            lighting: VertexLighting::new(lighting.into()),
        }
    }
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;
    use cgmath::Vector3;

    #[test]
    fn vertex_dummy() {
        assert!(!Vertex::DUMMY.position.repr[0].is_finite());
    }

    #[test]
    fn vertex_new_colored() {
        let vertex = Vertex::new_colored(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(4.0, 5.0, 6.0),
            RGBA::new(7.0, 8.0, 9.0, 0.5),
        );
        assert_eq!(vertex.position.repr, [1.0, 2.0, 3.0]);
        assert_eq!(vertex.normal.repr, [4.0, 5.0, 6.0]);
        assert_eq!(vertex.color_or_texture.repr, [7.0, 8.0, 9.0, 0.5]);
        assert_eq!(vertex.lighting.repr, [1.0, 1.0, 1.0]);
    }

    /// Full path used by normal rendering.
    #[test]
    fn vertex_from_block_vertex() {
        let block_vertex = BlockVertex {
            position: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(4.0, 5.0, 6.0),
            coloring: Coloring::Solid(RGBA::new(7.0, 8.0, 9.0, 0.5)),
        };
        let vertex = GLBlockVertex::from(block_vertex).instantiate(
            Vector3::new(0.1, 0.2, 0.3),
            PackedLight::INITIAL,
        );
        assert_eq!(vertex.position.repr, [1.1, 2.2, 3.3]);
        assert_eq!(vertex.normal.repr, [4.0, 5.0, 6.0]);
        assert_eq!(vertex.color_or_texture.repr, [7.0, 8.0, 9.0, 1.0]);  // alpha is currently locked to 1
        assert_eq!(vertex.lighting.repr, [1.0, 1.0, 1.0]);
    }
}