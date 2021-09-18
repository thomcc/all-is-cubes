// Copyright 2020-2021 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

//! Get from [`Space`] to [`Tess`].

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex, Weak};

use cgmath::{EuclideanSpace as _, Matrix4, Point3, Transform as _, Vector3, Zero as _};
use luminance::backend::shader::Uniformable;
use luminance::blending::{Blending, Equation, Factor};
use luminance::context::GraphicsContext;
use luminance::depth_test::DepthWrite;
use luminance::face_culling::{FaceCulling, FaceCullingMode, FaceCullingOrder};
use luminance::pipeline::{BoundTexture, Pipeline, PipelineError, TextureBinding};
use luminance::pixel::{NormRGBA8UI, NormUnsigned};
use luminance::render_state::RenderState;
use luminance::shading_gate::ShadingGate;
use luminance::tess::View as _;
use luminance::tess::{Mode, Tess};
use luminance::tess_gate::TessGate;
use luminance::texture::{Dim2, Dim3, GenMipmaps, Sampler, Texture, TextureError};

use crate::camera::Camera;
use crate::chunking::ChunkPos;
use crate::content::palette;
use crate::listen::Listener;
use crate::lum::block_texture::{BlockTexture, BoundBlockTexture, LumAtlasAllocator};
use crate::lum::shading::BlockPrograms;
use crate::lum::types::{AicLumBackend, LumBlockVertex};
use crate::lum::{wireframe_vertices, GraphicsResourceError};
use crate::math::{Aab, FaceMap, FreeCoordinate, GridCoordinate, GridPoint, Rgb};
use crate::raycast::Face;
use crate::space::{Grid, Space, SpaceChange};
use crate::triangulator::{
    ChunkTriangulation, ChunkedSpaceTriangulation, DepthOrdering, SpaceTriangulation,
};
use crate::universe::URef;
use crate::util::{CustomFormat, StatusText};

use super::block_texture::AtlasFlushInfo;

const CHUNK_SIZE: GridCoordinate = 16;

type ChunkData<Backend> = Option<Tess<Backend, LumBlockVertex, u32>>;

/// Manages cached data and GPU resources for drawing a single [`Space`] and
/// following its changes.
pub struct SpaceRenderer<Backend: AicLumBackend> {
    /// Note that `self.cst` has its own todo listener.
    todo: Arc<Mutex<SpaceRendererTodo>>,
    block_texture: Option<LumAtlasAllocator<Backend>>,
    light_texture: Option<SpaceLightTexture<Backend>>,
    cst: ChunkedSpaceTriangulation<
        ChunkData<Backend>,
        LumBlockVertex,
        LumAtlasAllocator<Backend>,
        CHUNK_SIZE,
    >,
    debug_chunk_boxes_tess: Option<Tess<Backend, LumBlockVertex>>,
}

impl<Backend: AicLumBackend> SpaceRenderer<Backend> {
    /// Constructs a [`SpaceRenderer`] for the given [`Space`].
    ///
    /// Note that the actual geometry for the [`Space`] will be computed over several
    /// frames after construction. There is not currently a specific way to wait for
    /// completion.
    pub fn new(space: URef<Space>) -> Self {
        let space_borrowed = space.borrow();

        let todo = SpaceRendererTodo::default();
        let todo_rc = Arc::new(Mutex::new(todo));
        space_borrowed.listen(TodoListener(Arc::downgrade(&todo_rc)));

        Self {
            todo: todo_rc,
            block_texture: None,
            light_texture: None,
            cst: ChunkedSpaceTriangulation::new(space),
            debug_chunk_boxes_tess: None,
        }
    }

    /// Returns a reference to the [`Space`] this draws.
    pub fn space(&self) -> &URef<Space> {
        self.cst.space()
    }

    /// Prepare to draw a frame, performing the steps that must be done while holding a
    /// `&mut C`; the returned [`SpaceRendererOutput`] is then for use within the
    /// luminance pipeline.
    pub(super) fn prepare_frame<'a, C>(
        &'a mut self,
        context: &mut C,
        camera: &Camera,
    ) -> Result<SpaceRendererOutput<'a, Backend>, GraphicsResourceError>
    where
        C: GraphicsContext<Backend = Backend>,
    {
        let graphics_options = camera.options();
        let mut todo = self.todo.lock().unwrap();

        let space = &*self
            .cst
            .space()
            .try_borrow()
            .expect("TODO: return a trivial result instead of panic.");

        if self.block_texture.is_none() {
            self.block_texture = Some(LumAtlasAllocator::new(context)?);
        }
        let block_texture_allocator = self.block_texture.as_mut().unwrap();

        if self.light_texture.is_none() {
            todo.light = None; // signal to update everything
            self.light_texture = Some(SpaceLightTexture::new(context, space.grid())?);
        }
        let light_texture = self.light_texture.as_mut().unwrap();

        // Update light texture
        if let Some(set) = &mut todo.light {
            // TODO: work in larger, ahem, chunks
            for cube in set.drain() {
                light_texture.update(space, Grid::new(cube, [1, 1, 1]))?;
            }
        } else {
            light_texture.update_all(space)?;
            todo.light = Some(HashSet::new());
        }

        // Update chunks
        let (cst_info, view_chunk) = self.cst.update_blocks_and_some_chunks(
            camera,
            block_texture_allocator,
            |triangulation, render_data| {
                update_chunk_tess(context, triangulation, render_data);
            },
            |triangulation, render_data| {
                if let Some(tess) = render_data {
                    let range = triangulation.transparent_range(DepthOrdering::Within);
                    tess.indices_mut()
                        .expect("failed to map indices for depth sorting")[range.clone()]
                    .copy_from_slice(&triangulation.indices()[range]);
                }
            },
        );

        // Flush all texture updates to GPU.
        // This must happen after `cst.update_blocks_and_some_chunks`.
        let texture_info = block_texture_allocator.flush()?;

        if graphics_options.debug_chunk_boxes {
            if self.debug_chunk_boxes_tess.is_none() {
                let mut v = Vec::new();
                for chunk in self.cst.chunk_chart().chunks(view_chunk) {
                    wireframe_vertices(&mut v, palette::DEBUG_CHUNK_MAJOR, Aab::from(chunk.grid()));
                }

                // Frame the nearest chunk in detail
                for face in Face::ALL_SIX {
                    let m = face.matrix(CHUNK_SIZE);
                    for i in 1..CHUNK_SIZE {
                        let mut push = |p| {
                            v.push(LumBlockVertex::new_colored(
                                m.transform_point(p).map(FreeCoordinate::from),
                                Vector3::zero(),
                                palette::DEBUG_CHUNK_MINOR,
                            ));
                        };
                        push(Point3::new(i, 0, 0));
                        push(Point3::new(i, CHUNK_SIZE, 0));
                        push(Point3::new(0, i, 0));
                        push(Point3::new(CHUNK_SIZE, i, 0));
                    }
                }

                // TODO: Allocate some vertices for dynamic debug info
                // (or maybe use separate Tesses per chunk)
                // * Signal chunk updates
                // * Mark which chunks are nonempty, opaque, transparent, view angle

                self.debug_chunk_boxes_tess = Some(
                    context
                        .new_tess()
                        .set_vertices(v)
                        .set_mode(Mode::Line)
                        .build()?,
                );
            }
        } else {
            self.debug_chunk_boxes_tess = None;
        }

        Ok(SpaceRendererOutput {
            data: SpaceRendererOutputData {
                camera: camera.clone(),
                cst: &self.cst,
                debug_chunk_boxes_tess: &self.debug_chunk_boxes_tess,
                view_chunk,
                info: SpaceRenderInfo {
                    chunk_update_count: cst_info.chunk_update_count,
                    block_update_count: cst_info.block_update_count,
                    chunks_drawn: 0,
                    squares_drawn: 0, // filled later
                    texture_info,
                },
                sky_color: space.physics().sky_color,
            },
            block_texture: &mut block_texture_allocator.texture,
            light_texture,
        })
    }
}

/// Ingredients to actually draw the [`Space`] inside a luminance pipeline, produced by
/// [`SpaceRenderer::prepare_frame`].
pub(super) struct SpaceRendererOutput<'a, Backend: AicLumBackend> {
    pub(super) data: SpaceRendererOutputData<'a, Backend>,
    block_texture: &'a mut BlockTexture<Backend>,
    light_texture: &'a mut SpaceLightTexture<Backend>,
}

/// The portion of [`SpaceRendererOutput`] which does not vary with the pipeline progress.
pub(super) struct SpaceRendererOutputData<'a, Backend: AicLumBackend> {
    pub(super) camera: Camera,
    cst: &'a ChunkedSpaceTriangulation<
        ChunkData<Backend>,
        LumBlockVertex,
        LumAtlasAllocator<Backend>,
        CHUNK_SIZE,
    >,
    debug_chunk_boxes_tess: &'a Option<Tess<Backend, LumBlockVertex>>,
    view_chunk: ChunkPos<CHUNK_SIZE>,
    info: SpaceRenderInfo,

    /// Space's sky color, to be used as background color (clear color / fog).
    pub(super) sky_color: Rgb,
}

/// As [`SpaceRendererOutput`], but past the texture-binding stage of the pipeline.
/// Note: This must be public to satisfy luminance derive macros' public requirements.
pub(super) struct SpaceRendererBound<'a, Backend: AicLumBackend> {
    pub(super) data: SpaceRendererOutputData<'a, Backend>,

    /// Block texture to pass to the shader.
    pub(super) bound_block_texture: BoundBlockTexture<'a, Backend>,
    /// Block texture to pass to the shader.
    pub(super) bound_light_texture: SpaceLightTextureBound<'a, Backend>,
}

impl<'a, Backend: AicLumBackend> SpaceRendererOutput<'a, Backend> {
    /// Bind texture, in preparation for using the
    /// [`ShadingGate`](luminance::shading_gate::ShadingGate).
    pub fn bind(
        self,
        pipeline: &'a Pipeline<'a, Backend>,
    ) -> Result<SpaceRendererBound<'a, Backend>, PipelineError> {
        Ok(SpaceRendererBound {
            data: self.data,
            bound_block_texture: pipeline.bind_texture(self.block_texture)?,
            bound_light_texture: self.light_texture.bind(pipeline)?,
        })
    }
}
impl<'a, Backend: AicLumBackend> SpaceRendererOutputData<'a, Backend> {
    fn cull(&self, chunk: ChunkPos<CHUNK_SIZE>) -> bool {
        self.camera.options().use_frustum_culling && !self.camera.aab_in_view(chunk.grid().into())
    }
}
impl<'a, Backend: AicLumBackend> SpaceRendererBound<'a, Backend>
where
    f32: Uniformable<Backend>,
    [i32; 3]: Uniformable<Backend>,
    [f32; 3]: Uniformable<Backend>,
    [[f32; 4]; 4]: Uniformable<Backend>,
    TextureBinding<Dim2, NormUnsigned>: Uniformable<Backend>,
    TextureBinding<Dim3, NormUnsigned>: Uniformable<Backend>,
{
    /// Use a [`ShadingGate`] to actually draw the space.
    pub(crate) fn render<E>(
        &self,
        shading_gate: &mut ShadingGate<'_, Backend>,
        block_programs: &mut BlockPrograms<Backend>,
    ) -> Result<SpaceRenderInfo, E> {
        let mut chunks_drawn = 0;
        let mut squares_drawn = 0;

        // These two blocks are *almost* identical but the iteration order is reversed,
        // the shader is different, and we only count the chunks once.
        shading_gate.shade(
            &mut block_programs.opaque,
            |ref mut program_iface, u, mut render_gate| {
                u.initialize(program_iface, self);
                let pass = SpaceRendererPass::Opaque;
                render_gate.render(&pass.render_state(), |mut tess_gate| {
                    for p in self.data.cst.chunk_chart().chunks(self.data.view_chunk) {
                        if let Some(chunk) = self.data.cst.chunk(p) {
                            if self.data.cull(p) {
                                continue;
                            }
                            chunks_drawn += 1;
                            squares_drawn +=
                                render_chunk_tess(chunk, &mut tess_gate, pass, DepthOrdering::Any)?;
                        }
                        // TODO: If the chunk is missing, draw a blocking shape, possibly?
                    }
                    Ok(())
                })?;

                u.set_view_matrix(
                    program_iface,
                    self.data.camera.view_matrix()
                        * Matrix4::from_translation(
                            (self.data.view_chunk.0 * CHUNK_SIZE)
                                .to_vec()
                                .map(FreeCoordinate::from),
                        ),
                );
                render_gate.render(&pass.render_state(), |mut tess_gate| {
                    if let Some(debug_tess) = self.data.debug_chunk_boxes_tess {
                        tess_gate.render(debug_tess)?;
                    }
                    Ok(())
                })?;

                Ok(())
            },
        )?;

        if self.data.camera.options().transparency.will_output_alpha() {
            shading_gate.shade(
                &mut block_programs.transparent,
                |ref mut program_iface, u, mut render_gate| {
                    u.initialize(program_iface, self);
                    let pass = SpaceRendererPass::Transparent;
                    render_gate.render(&pass.render_state(), |mut tess_gate| {
                        for p in self
                            .data
                            .cst
                            .chunk_chart()
                            .chunks(self.data.view_chunk)
                            .rev()
                        {
                            if let Some(chunk) = self.data.cst.chunk(p) {
                                if self.data.cull(p) {
                                    continue;
                                }
                                squares_drawn += render_chunk_tess(
                                    chunk,
                                    &mut tess_gate,
                                    pass,
                                    // TODO: avoid adding and then subtracting view_chunk
                                    DepthOrdering::from_view_direction(
                                        p.0 - self.data.view_chunk.0,
                                    ),
                                )?;
                            }
                        }
                        Ok(())
                    })
                },
            )?;
        }

        Ok(SpaceRenderInfo {
            chunks_drawn,
            squares_drawn,
            ..self.data.info.clone()
        })
    }
}

/// Performance info from a [`SpaceRenderer`] drawing one frame.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpaceRenderInfo {
    /// How many chunks were recomputed this frame.
    pub chunk_update_count: usize,
    /// How many block triangulations were recomputed this time.
    pub block_update_count: usize,
    pub chunks_drawn: usize,
    /// How many squares (quadrilaterals; sets of 2 triangles = 6 vertices) were used
    /// to draw this frame.
    pub squares_drawn: usize,
    /// Status of the texture atlas.
    pub texture_info: AtlasFlushInfo,
}

impl CustomFormat<StatusText> for SpaceRenderInfo {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>, _: StatusText) -> fmt::Result {
        writeln!(
            fmt,
            "Chunk updates: {:3} Block updates: {:3}",
            self.chunk_update_count, self.block_update_count,
        )?;
        writeln!(
            fmt,
            "Chunks drawn: {:3} Quads drawn: {:3}",
            self.chunks_drawn, self.squares_drawn,
        )?;
        write!(fmt, "{:#?}", self.texture_info.custom_format(StatusText))?;
        Ok(())
    }
}

// Which drawing pass we're doing.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SpaceRendererPass {
    Opaque,
    Transparent,
}
impl SpaceRendererPass {
    /// Returns the [`RenderState`] to use for this pass.
    pub fn render_state(self) -> RenderState {
        let base = RenderState::default().set_face_culling(FaceCulling {
            order: FaceCullingOrder::CCW,
            mode: FaceCullingMode::Back,
        });
        match self {
            SpaceRendererPass::Opaque => base,
            SpaceRendererPass::Transparent => {
                base.set_depth_write(DepthWrite::Off) // Unnecessary
                    .set_blending(Some(Blending {
                        // Note that this blending configuration is for premultiplied alpha.
                        // The fragment shaders are responsible for producing premultiplied alpha outputs.
                        equation: Equation::Additive,
                        src: Factor::One,
                        dst: Factor::SrcAlphaComplement,
                    }))
            }
        }
    }
}

/// Render chunk and return the number of quads drawn.
fn render_chunk_tess<Backend: AicLumBackend, E>(
    chunk: &ChunkTriangulation<
        ChunkData<Backend>,
        LumBlockVertex,
        LumAtlasAllocator<Backend>,
        CHUNK_SIZE,
    >,
    tess_gate: &mut TessGate<'_, Backend>,
    pass: SpaceRendererPass,
    ordering: DepthOrdering,
) -> Result<usize, E> {
    let mut count = 0;
    if let Some(tess) = &chunk.render_data {
        let range = match pass {
            SpaceRendererPass::Opaque => chunk.triangulation().opaque_range(),
            SpaceRendererPass::Transparent => chunk.triangulation().transparent_range(ordering),
        };
        if range.is_empty() {
            return Ok(0);
        }
        count += (range.end - range.start) / 6;
        tess_gate.render(
            tess.view(range)
                .expect("can't happen: inconsistent chunk index range"),
        )?;
    }
    Ok(count)
}

fn update_chunk_tess<C>(
    context: &mut C,
    new_triangulation: &SpaceTriangulation<LumBlockVertex>,
    tess_option: &mut ChunkData<C::Backend>,
) where
    C: GraphicsContext,
    C::Backend: AicLumBackend,
{
    let existing_tess_size_ok = if let Some(tess) = tess_option.as_ref() {
        tess.vert_nb() == new_triangulation.vertices().len()
            && tess.idx_nb() == new_triangulation.indices().len()
    } else {
        false
    };
    if !existing_tess_size_ok {
        // Existing buffer, if any, is not the right length. Discard it.
        *tess_option = None;
    }

    // TODO: replace unwrap()s with an error logging/flagging mechanism that can do partial drawing
    if new_triangulation.is_empty() {
        // Render zero vertices by not rendering anything.
        *tess_option = None;
    } else if let Some(tess) = tess_option.as_mut() {
        // We already have a buffer, and it is a matching length.
        tess.vertices_mut()
            .expect("failed to map vertices for copying")
            .copy_from_slice(new_triangulation.vertices());
        tess.indices_mut()
            .expect("failed to map indices for copying")
            .copy_from_slice(new_triangulation.indices());
    } else {
        // Allocate and populate new buffer.
        *tess_option = Some(
            context
                .new_tess()
                .set_vertices(new_triangulation.vertices())
                .set_indices(new_triangulation.indices())
                .set_mode(Mode::Triangle)
                .build()
                .unwrap(),
        );
    }
}

/// [`SpaceRenderer`]'s set of things that need recomputing.
#[derive(Debug, Default)]
struct SpaceRendererTodo {
    /// Blocks whose light texels should be updated.
    /// None means do a full space reupload.
    ///
    /// TODO: experiment with different granularities of light invalidation (chunks, dirty rects, etc.)
    light: Option<HashSet<GridPoint>>,
}

/// [`Listener`] adapter for [`SpaceRendererTodo`].
struct TodoListener(Weak<Mutex<SpaceRendererTodo>>);

impl Listener<SpaceChange> for TodoListener {
    fn receive(&self, message: SpaceChange) {
        if let Some(cell) = self.0.upgrade() {
            if let Ok(mut todo) = cell.lock() {
                match message {
                    SpaceChange::EveryBlock => {
                        todo.light = None;
                    }
                    SpaceChange::Lighting(p) => {
                        // None means we're already at "update everything"
                        if let Some(set) = &mut todo.light {
                            set.insert(p);
                        }
                    }
                    SpaceChange::Block(..) => {}
                    SpaceChange::Number(..) => {}
                    SpaceChange::BlockValue(..) => {}
                }
            }
        }
    }

    fn alive(&self) -> bool {
        self.0.strong_count() > 0
    }
}

/// Keeps a 3D [`Texture`] up to date with the light data from a [`Space`].
///
/// The texels are in [`PackedLight`] form.
/// The alpha component is unused.
/// TODO: Use alpha component to communicate block opacity.
struct SpaceLightTexture<Backend: AicLumBackend> {
    texture: Texture<Backend, Dim3, NormRGBA8UI>,
    /// The region of cube coordinates for which there are valid texels.
    texture_grid: Grid,
}

impl<Backend: AicLumBackend> SpaceLightTexture<Backend> {
    /// Construct a new `SpaceLightTexture` for the specified size of [`Space`],
    /// with no data.
    pub fn new<C>(context: &mut C, grid: Grid) -> Result<Self, TextureError>
    where
        C: GraphicsContext<Backend = Backend>,
    {
        // Boundary of 1 extra cube automatically captures sky light.
        let texture_grid = grid.expand(FaceMap {
            px: 1,
            py: 1,
            pz: 1,
            nx: 0,
            ny: 0,
            nz: 0,
            within: 0,
        });
        let texture = context.new_texture_no_texels(
            texture_grid.unsigned_size().into(),
            /* mipmaps= */ 0,
            Sampler::default(), // sampler options don't matter because we're using texelFetch()
        )?;
        Ok(Self {
            texture,
            texture_grid,
        })
    }

    /// Copy the specified region of light data.
    pub fn update(&mut self, space: &Space, region: Grid) -> Result<(), TextureError> {
        let mut data = Vec::with_capacity(region.volume());
        // TODO: Enable circular operation and eliminate the need for the offset of the
        // coordinates (texture_grid.lower_bounds() and light_offset in the shader)
        // by doing a coordinate wrap-around -- the shader and the Space will agree
        // on coordinates modulo the texture size, and this upload will need to be broken
        // into up to 8 pieces.
        for z in region.z_range() {
            for y in region.y_range() {
                for x in region.x_range() {
                    data.push(space.get_lighting([x, y, z]).as_texel());
                }
            }
        }
        self.texture.upload_part(
            GenMipmaps::No,
            (region.lower_bounds() - self.texture_grid.lower_bounds())
                .map(|s| s as u32)
                .into(),
            region.unsigned_size().into(),
            &data,
        )
    }

    pub fn update_all(&mut self, space: &Space) -> Result<(), TextureError> {
        self.update(space, self.texture_grid)
    }

    fn bind<'a>(
        &'a mut self,
        pipeline: &'a Pipeline<'a, Backend>,
    ) -> Result<SpaceLightTextureBound<'a, Backend>, PipelineError> {
        Ok(SpaceLightTextureBound {
            texture: pipeline.bind_texture(&mut self.texture)?,
            offset: -self.texture_grid.lower_bounds().to_vec(),
        })
    }
}

pub(crate) struct SpaceLightTextureBound<'a, Backend: AicLumBackend> {
    pub(crate) texture: BoundTexture<'a, Backend, Dim3, NormRGBA8UI>,
    pub(crate) offset: Vector3<GridCoordinate>,
}

#[cfg(test)]
mod tests {
    // TODO: Arrange, somehow, to test the parts that need a GraphicsContext
}
