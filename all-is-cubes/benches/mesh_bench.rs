// Copyright 2020-2022 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

use all_is_cubes::camera::GraphicsOptions;
use all_is_cubes::math::Rgba;
use all_is_cubes::rgba_const;
use all_is_cubes::universe::Universe;
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

use all_is_cubes::block::{Block, AIR};

use all_is_cubes::mesh::{
    triangulate_block, triangulate_blocks, triangulate_space, BlockMeshes, BlockVertex,
    MeshOptions, SpaceMesh, TestTextureAllocator, TestTextureTile,
};
use all_is_cubes::space::{Grid, Space};

criterion_group!(benches, mesh_benches);
criterion_main!(benches);

fn mesh_benches(c: &mut Criterion) {
    let options = &MeshOptions::new(&GraphicsOptions::default(), false);

    c.bench_function("block, checkerboard", |b| {
        let mut universe = Universe::new();
        let block = checkerboard_block(&mut universe, [AIR, Block::from(Rgba::WHITE)]);
        let ev = block.evaluate().unwrap();

        b.iter_batched_ref(
            || (),
            |()| {
                triangulate_block::<BlockVertex, _>(&ev, &mut TestTextureAllocator::new(), options)
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("block, opaque", |b| {
        let mut universe = Universe::new();
        let block = checkerboard_block(
            &mut universe,
            [Block::from(Rgba::BLACK), Block::from(Rgba::WHITE)],
        );
        let ev = block.evaluate().unwrap();

        b.iter_batched_ref(
            || (),
            |()| {
                triangulate_block::<BlockVertex, _>(&ev, &mut TestTextureAllocator::new(), options)
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("space, checkerboard, new buffer", |b| {
        // The Space and block meshes are not mutated, so we can construct them once.
        // This also ensures we're not timing dropping them.
        let (space, block_meshes) = checkerboard_space_bench_setup(options, false);
        // This benchmark actually has no use for iter_batched_ref -- it could be
        // iter_with_large_drop -- but I want to make sure any quirks of the measurement
        // are shared between all cases.
        b.iter_batched_ref(
            || (),
            |()| triangulate_space(&space, space.grid(), options, &*block_meshes),
            BatchSize::SmallInput,
        );
    });

    c.bench_function("space, checkerboard, reused buffer", |b| {
        let (space, block_meshes) = checkerboard_space_bench_setup(options, false);
        b.iter_batched_ref(
            || {
                let mut buffer = SpaceMesh::new();
                buffer.compute(&space, space.grid(), options, &*block_meshes);
                // Sanity check that we're actually rendering as much as we expect.
                assert_eq!(buffer.vertices().len(), 6 * 4 * (16 * 16 * 16) / 2);
                buffer
            },
            |buffer: &mut SpaceMesh<BlockVertex, TestTextureTile>| {
                // As of this writing, this benchmark is really just "what if we don't allocate
                // a new Vec". Later, the buffers will hopefully become cleverer and we'll be
                // able to reuse some work (or at least send only part of the buffer to the GPU),
                // and so this will become a meaningful benchmark of how much CPU time we're
                // spending or saving on that.
                buffer.compute(&space, space.grid(), options, &*block_meshes)
            },
            BatchSize::SmallInput,
        );
    });

    let mut slow_group = c.benchmark_group("slow");
    slow_group.sample_size(10);

    slow_group.bench_function("space, transparent checkerboard, new buffer", |b| {
        let (space, block_meshes) = checkerboard_space_bench_setup(options, true);
        b.iter_batched_ref(
            || (),
            |()| triangulate_space(&space, space.grid(), options, &*block_meshes),
            BatchSize::SmallInput,
        );
    });
}

fn checkerboard_space_bench_setup(
    options: &MeshOptions,
    transparent: bool,
) -> (Space, BlockMeshes<BlockVertex, TestTextureTile>) {
    let space = checkerboard_space([
        AIR,
        Block::from(if transparent {
            rgba_const!(0.5, 0.5, 0.5, 0.5)
        } else {
            Rgba::WHITE
        }),
    ]);

    let block_meshes = triangulate_blocks(&space, &mut TestTextureAllocator::new(), options);

    (space, block_meshes)
}

fn checkerboard_block(universe: &mut Universe, voxels: [Block; 2]) -> Block {
    Block::builder()
        .voxels_ref(16, universe.insert_anonymous(checkerboard_space(voxels)))
        .build()
}

fn checkerboard_space(blocks: [Block; 2]) -> Space {
    let grid = Grid::new([0, 0, 0], [16, 16, 16]);
    let mut space = Space::empty(grid);
    space
        .fill(grid, |p| {
            Some(&blocks[((p.x + p.y + p.z) as usize).rem_euclid(blocks.len())])
        })
        .unwrap();
    space
}
