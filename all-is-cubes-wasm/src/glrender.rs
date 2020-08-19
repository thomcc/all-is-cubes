// Copyright 2020 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <http://opensource.org/licenses/MIT>.

//! OpenGL-based graphics rendering.

use cgmath::{Vector3};
use luminance_derive::{Semantics, Vertex, UniformInterface};
use luminance_front::face_culling::{FaceCulling, FaceCullingMode, FaceCullingOrder};
use luminance_front::context::GraphicsContext;
use luminance_front::framebuffer::Framebuffer;
use luminance_front::pipeline::PipelineState;
use luminance_front::render_state::RenderState;
use luminance_front::shader::{BuiltProgram, Program, ProgramError, StageError, Uniform};
use luminance_front::tess::{Interleaved, Mode, Tess, VerticesMut};
use luminance_front::tess_gate::TessGate;
use luminance_front::texture::Dim2;
use luminance_front::Backend;
use std::time::Duration;
use wasm_bindgen::prelude::*;
use web_sys::console;

use all_is_cubes::camera::{Camera, ProjectionHelper};
use all_is_cubes::math::{FreeCoordinate};
use all_is_cubes::space::{PackedLight, Space};
use all_is_cubes::raycast::Raycaster;
use all_is_cubes::triangulator::{BlockVertex, BlocksRenderData, ToGfxVertex, triangulate_blocks, triangulate_space};

use crate::js_bindings::{CanvasHelper};

const SHADER_COMMON: &str = include_str!("shaders/common.glsl");
const SHADER_FRAGMENT: &str = include_str!("shaders/fragment.glsl");
const SHADER_VERTEX_BLOCK: &str = include_str!("shaders/vertex-block.glsl");
const SHADER_VERTEX_COMMON: &str = include_str!("shaders/vertex-common.glsl");

pub struct GLRenderer<C> where C: GraphicsContext<Backend = Backend> {
    canvas_helper: CanvasHelper,
    surface: C,
    back_buffer: Framebuffer<Dim2, (), ()>,
    proj: ProjectionHelper,
    block_program: Program<VertexSemantics, (), ShaderInterface>,
    block_data_cache: Option<BlocksRenderData<BlockVertex>>,  // TODO: quick hack, needs an invalidation strategy
    chunk: Chunk,
    fake_time: Duration,
}

impl<C> GLRenderer<C> where C: GraphicsContext<Backend = Backend> {
    pub fn new(mut surface: C, canvas_helper: CanvasHelper) -> Self {
        let program_attempt: Result<BuiltProgram<_, _, _>, ProgramError> = surface
            .new_shader_program::<VertexSemantics, (), ShaderInterface>()
            .from_strings(
                &(SHADER_COMMON.to_owned()
                    + "#line 1 1\n" + SHADER_VERTEX_COMMON
                    + "#line 1 2\n" + SHADER_VERTEX_BLOCK),
                None,
                None,
                &(SHADER_COMMON.to_owned()
                    + "#line 1 1\n" + SHADER_FRAGMENT));
        let BuiltProgram {program: block_program, warnings} = program_attempt
            .unwrap_or_else(|error| {
                // Extract text and send to console so we get less quoting
                let error_text = match error {
                    ProgramError::CreationFailed(text) => text,
                    ProgramError::StageError(StageError::CompilationFailed(_, text)) => text,
                    ProgramError::LinkFailed(text) => text,
                    _ => format!("{:?}", error),
                };
                console::error_1(&JsValue::from_str(&format!("GLSL error:\n{}", error_text)));
                panic!("shader compilation failed");
            });
        for warning in warnings {
            console::warn_1(&JsValue::from_str(&format!("GLSL warning: {:?}", warning)));
        }

        let proj = ProjectionHelper::new(1.0, canvas_helper.viewport_px());
        let back_buffer = luminance::framebuffer::Framebuffer::back_buffer(&mut surface, canvas_helper.viewport_dev()).unwrap();  // TODO error handling
        Self {
            canvas_helper,
            surface,
            back_buffer,
            proj,
            block_program,
            block_data_cache: None,
            chunk: Chunk::new(),
            fake_time: Duration::default(),
        }
    }

    pub fn update_viewport(&mut self) {
        self.proj.set_viewport(self.canvas_helper.viewport_px());
        self.back_buffer = luminance::framebuffer::Framebuffer::back_buffer(&mut self.surface, self.canvas_helper.viewport_dev()).unwrap();  // TODO error handling
    }

    pub fn render_frame(&mut self, space: &Space, camera: &Camera) {
        // Not used directly for rendering, but used for cursor.
        self.proj.set_view_matrix(camera.view());

        let surface = &mut self.surface;
        let block_program = &mut self.block_program;
        let projection_matrix = self.proj.projection();

        // TODO: quick hack; we need actual invalidation, not memoization
        let block_render_data = self.block_data_cache
            .get_or_insert_with(|| triangulate_blocks(space));

        self.fake_time += Duration::from_millis(1000/60);  // TODO

        self.chunk.update(surface, &space, block_render_data);
        let ct = &self.chunk;

        let render_state = RenderState::default()
            .set_face_culling(FaceCulling{order: FaceCullingOrder::CCW, mode: FaceCullingMode::Back});

        let render = surface.new_pipeline_gate().pipeline(
            &self.back_buffer,
            // TODO: port skybox cube map code
            &PipelineState::default().set_clear_color([0.6, 0.7, 1.0, 1.]),
            |_, mut shading_gate| {
                shading_gate.shade(block_program, |mut shader_iface, u, mut render_gate| {
                    let pm: [[f32; 4]; 4] = projection_matrix.cast::<f32>().unwrap().into();
                    shader_iface.set(&u.projection_matrix0, pm[0]);
                    shader_iface.set(&u.projection_matrix1, pm[1]);
                    shader_iface.set(&u.projection_matrix2, pm[2]);
                    shader_iface.set(&u.projection_matrix3, pm[3]);

                    let vm: [[f32; 4]; 4] = camera.view().cast::<f32>().unwrap().into();
                    shader_iface.set(&u.view_matrix0, vm[0]);
                    shader_iface.set(&u.view_matrix1, vm[1]);
                    shader_iface.set(&u.view_matrix2, vm[2]);
                    shader_iface.set(&u.view_matrix3, vm[3]);

                    render_gate.render(&render_state, |mut tess_gate| {
                         ct.render(&mut tess_gate)?;
                         Ok(())
                    })
                })
            },
        ).assume();  // TODO error handlint

        if !render.is_ok() {
            panic!("not ok");  // TODO what good error handling goes here?
        }

        // There is no swap_buffers operation because WebGL implicitly does so.
    }

    pub fn cursor_raycaster(&self, /* TODO: offset ... or move this function */) -> Raycaster {
        let (origin, direction) = self.proj.project_ndc_into_world(0.0, 0.0);
        Raycaster::new(origin, direction)
    }
}

#[derive(Debug, UniformInterface)]
struct ShaderInterface {
    // TODO: Passing matrices as four vectors due to bug
    //     https://github.com/phaazon/luminance-rs/issues/434
    #[uniform(unbound)] projection_matrix0: Uniform<[f32; 4]>,
    #[uniform(unbound)] projection_matrix1: Uniform<[f32; 4]>,
    #[uniform(unbound)] projection_matrix2: Uniform<[f32; 4]>,
    #[uniform(unbound)] projection_matrix3: Uniform<[f32; 4]>,

    #[uniform(unbound)] view_matrix0: Uniform<[f32; 4]>,
    #[uniform(unbound)] view_matrix1: Uniform<[f32; 4]>,
    #[uniform(unbound)] view_matrix2: Uniform<[f32; 4]>,
    #[uniform(unbound)] view_matrix3: Uniform<[f32; 4]>,
}

/// Defines vertex array structure for luminance framework.
/// Note that each "wrapper" names a new type generated by the derive(Semantics).
/// Also the macro requires the enum to be `pub` for some reason.
#[derive(Copy, Clone, Debug, Semantics)]
pub enum VertexSemantics {
    // TODO: revisit compact representations
    #[sem(name = "a_position", repr = "[f32; 3]", wrapper = "VertexPosition")]
    Position,
    #[sem(name = "a_normal", repr = "[f32; 3]", wrapper = "VertexNormal")]
    Normal,
    #[sem(name = "a_color", repr = "[f32; 4]", wrapper = "VertexRGBA")]
    Color,
    // TODO: look into packed repr for lighting
    #[sem(name = "a_lighting", repr = "[f32; 3]", wrapper = "VertexLighting")]
    Lighting,
}

#[derive(Clone, Copy, Debug, Vertex)]
#[vertex(sem = "VertexSemantics")]
struct Vertex {
    #[allow(dead_code)]  // read by shader
    position: VertexPosition,

    #[allow(dead_code)]  // read by shader
    normal: VertexNormal,

    #[allow(dead_code)]  // read by shader
    color: VertexRGBA,

    #[allow(dead_code)]  // read by shader
    lighting: VertexLighting,
}

impl ToGfxVertex<Vertex> for BlockVertex {
    // TODO: Stop using BlockVertex as our block vertex type, because it stores FreeCoordinate
    // and we want f32, forcing us to translate at every instantiation.

    fn instantiate(&self, offset: Vector3<FreeCoordinate>, lighting: PackedLight) -> Vertex {
        Vertex {
            position: VertexPosition::new((self.position + offset).cast::<f32>().unwrap().into()),
            normal: VertexNormal::new(self.normal.cast::<f32>().unwrap().into()),
            color: {
                let mut color_attribute = VertexRGBA::new(self.color.into());
                color_attribute[3] = 1.0;  // Force alpha to 1 until we have a better answer.
                color_attribute
            },
            lighting: VertexLighting::new(lighting.into()),
        }
    }
}

/// Not yet actually chunked rendering, but bundles the data that would be used in one.
struct Chunk {
    // bounds: Grid,
    vertices: Vec<Vertex>,
    tess: Option<Tess<Vertex>>,
}

impl Chunk {
    fn new() -> Self {
        Chunk { vertices: Vec::new(), tess: None }
    }

    fn update<C: GraphicsContext<Backend = Backend>>(
        &mut self,
        context: &mut C,
        space: &Space,
        blocks_render_data: &BlocksRenderData<BlockVertex>,
    ) {
        triangulate_space(space, blocks_render_data, &mut self.vertices);

        if let Some(tess) = self.tess.as_mut() {
            if tess.vert_nb() == self.vertices.len() {
                // Same length; reuse existing buffer.
                // TODO: Generalize this to be able to shrink buffers via degenerate triangles.
                let mut buffer_slice: VerticesMut<Vertex, (), (), Interleaved, Vertex> =
                    tess.vertices_mut().expect("failed to map vertices for copying");
                buffer_slice.copy_from_slice(&*self.vertices);
                return;
            }
        }

        // Failed to reuse; make a new buffer
        self.tess = Some(context
            .new_tess()
            .set_vertices(self.vertices.clone())
            .set_mode(Mode::Triangle)
            .build()
            .unwrap());  // TODO need any error handling?
    }

    fn render<E>(&self, tess_gate: &mut TessGate) -> Result<(), E> {
        if let Some(tess) = self.tess.as_ref() {
            tess_gate.render(tess)?;
        }
        Ok(())
    }
}