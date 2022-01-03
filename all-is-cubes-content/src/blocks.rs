// Copyright 2020-2021 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

//! Block definitions that are specific to the demo/initial content and not fundamental
//! or UI.

use all_is_cubes::util::YieldProgress;
use noise::Seedable as _;

use all_is_cubes::block::{Block, BlockCollision, Resolution, RotationPlacementRule, AIR};
use all_is_cubes::cgmath::{ElementWise as _, EuclideanSpace as _, InnerSpace, Vector3};
use all_is_cubes::drawing::embedded_graphics::{
    prelude::Point,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
};
use all_is_cubes::linking::{BlockModule, BlockProvider, GenError, InGenError};
use all_is_cubes::math::{
    cube_to_midpoint, Face, FreeCoordinate, GridCoordinate, GridMatrix, GridPoint, GridRotation,
    GridVector, NoiseFnExt as _, NotNan, Rgb, Rgba,
};
use all_is_cubes::space::{Grid, Space};
use all_is_cubes::universe::Universe;
use all_is_cubes::{rgb_const, rgba_const};

use crate::int_magnitude_squared;
use crate::landscape::install_landscape_blocks;
use crate::palette;

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, strum::Display, strum::EnumIter)]
#[strum(serialize_all = "kebab-case")]
#[non_exhaustive]
pub enum DemoBlocks {
    GlassBlock,
    Lamp,
    LamppostSegment,
    LamppostBase,
    LamppostTop,
    Sconce,
    Arrow,
    Road,
    Curb,
    CurbCorner,
    ExhibitBackground,
    Signboard,
}
impl BlockModule for DemoBlocks {
    fn namespace() -> &'static str {
        "all-is-cubes/demo-blocks"
    }
}

/// Add to `universe` demo-content blocks: all of [`DemoBlocks`] and [`LandscapeBlocks`].
///
/// [`LandscapeBlocks`]: crate::landscape::LandscapeBlocks
pub async fn install_demo_blocks(
    universe: &mut Universe,
    p: YieldProgress,
) -> Result<(), GenError> {
    let resolution = 16;
    let resolution_g = GridCoordinate::from(resolution);

    // In order to have consistent radii from the center point, we need to work with
    // doubled coordinates to allow for center vs. edge of block distinctions, and
    // note when we mean to refer to a cube center.
    // TODO: The whole premise of how to procedurally generate blocks like this ought
    // to be made more convenient, though.
    let one_diagonal = GridVector::new(1, 1, 1);
    let center_point_doubled = GridPoint::from_vec(one_diagonal * resolution_g);

    install_landscape_blocks(universe, resolution)?;

    // TODO: In order to split this into better pieces we need to make BlockProvider::new
    // async.
    p.progress(0.5).await;

    let road_color: Block = Rgba::new(0.157, 0.130, 0.154, 1.0).into();
    let curb_color: Block = Rgba::new(0.788, 0.765, 0.741, 1.0).into();
    let road_noise_v = noise::Value::new().set_seed(0x52b19f6a);
    let road_noise = noise::ScaleBias::new(&road_noise_v)
        .set_bias(1.0)
        .set_scale(0.12);

    let curb_fn = |cube: GridPoint| {
        let width = resolution_g / 3;
        if int_magnitude_squared(
            (cube - GridPoint::new(width / 2 + 2, 0, 0)).mul_element_wise(GridVector::new(1, 2, 0)),
        ) < width.pow(2)
        {
            scale_color(curb_color.clone(), road_noise.at_grid(cube), 0.02)
        } else {
            AIR
        }
    };

    let lamp_globe = Block::from(Rgba::WHITE);
    let lamppost_metal = Block::from(palette::ALMOST_BLACK);
    let lamppost_edge = Block::from(palette::ALMOST_BLACK * 1.12);

    use DemoBlocks::*;
    BlockProvider::<DemoBlocks>::new(|key| {
        Ok(match key {
            GlassBlock => {
                let glass_densities = [
                    //Block::from(rgba_const!(1.0, 0.0, 0.0, 1.0)),
                    Block::from(rgba_const!(0.95, 1.0, 0.95, 0.9)),
                    Block::from(rgba_const!(0.95, 0.95, 1.0, 0.65)),
                    Block::from(rgba_const!(0.95, 1.0, 0.95, 0.3)),
                    Block::from(rgba_const!(0.95, 0.95, 1.0, 0.07)),
                    Block::from(rgba_const!(0.95, 1.0, 0.95, 0.05)),
                ];

                Block::builder()
                    .display_name("Glass Block")
                    .voxels_fn(universe, resolution, |cube| {
                        let unit_radius_point = cube_to_midpoint(cube)
                            .map(|c| c / (FreeCoordinate::from(resolution_g) / 2.0) - 1.0);
                        let power = 7.;
                        let r = unit_radius_point
                            .to_vec()
                            .map(|c| c.abs().powf(power))
                            .dot(Vector3::new(1.0, 1.0, 1.0));
                        gradient_lookup(&glass_densities, (1.0 - r as f32) * 2.0)
                    })?
                    .build()
            }

            Road => Block::builder()
                .display_name("Road")
                .voxels_fn(universe, resolution, |cube| {
                    scale_color(road_color.clone(), road_noise.at_grid(cube), 0.02)
                })?
                .build(),

            Lamp => Block::builder()
                .display_name("Lamp")
                .light_emission(Rgb::new(20.0, 20.0, 20.0))
                .collision(BlockCollision::Recur)
                .voxels_fn(universe, resolution, |p| {
                    if int_magnitude_squared(p * 2 + one_diagonal - center_point_doubled)
                        <= resolution_g.pow(2)
                    {
                        &lamp_globe
                    } else {
                        &AIR
                    }
                })?
                .build(),

            LamppostSegment => Block::builder()
                .display_name("Lamppost")
                .light_emission(Rgb::new(3.0, 3.0, 3.0))
                .collision(BlockCollision::Recur)
                .rotation_rule(RotationPlacementRule::Attach { by: Face::NY })
                .voxels_fn(universe, resolution, |cube| {
                    if int_magnitude_squared(
                        (cube * 2 + one_diagonal - center_point_doubled)
                            .mul_element_wise(GridVector::new(1, 0, 1)),
                    ) <= 4i32.pow(2)
                    {
                        &lamppost_metal
                    } else {
                        &AIR
                    }
                })?
                .build(),

            LamppostBase => Block::builder()
                .display_name("Lamppost Base")
                .collision(BlockCollision::Recur)
                .rotation_rule(RotationPlacementRule::Attach { by: Face::NY })
                .voxels_fn(universe, resolution, |cube| {
                    let shape: [GridCoordinate; 16] =
                        [8, 8, 7, 7, 6, 6, 6, 5, 5, 5, 6, 6, 5, 4, 4, 3];
                    let [radius, secondary] = square_radius(resolution, cube);
                    if radius < shape.get(cube.y as usize).copied().unwrap_or(0) {
                        if secondary == radius {
                            &lamppost_edge
                        } else {
                            &lamppost_metal
                        }
                    } else {
                        &AIR
                    }
                })?
                .build(),

            LamppostTop => Block::builder()
                .display_name("Lamppost Top")
                .collision(BlockCollision::Recur)
                .rotation_rule(RotationPlacementRule::Attach { by: Face::NY })
                .voxels_fn(universe, resolution, |cube| {
                    let shape: [GridCoordinate; 16] =
                        [4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 6, 7, 8];
                    let r2 = int_magnitude_squared(
                        (cube * 2 + one_diagonal - center_point_doubled)
                            .mul_element_wise(GridVector::new(1, 0, 1)),
                    );
                    if r2 < shape.get(cube.y as usize).copied().unwrap_or(0).pow(2) {
                        &lamppost_metal
                    } else {
                        &AIR
                    }
                })?
                .build(),

            Sconce => Block::builder()
                .display_name("Sconce")
                .light_emission(Rgb::new(8.0, 7.0, 6.0))
                .collision(BlockCollision::Recur)
                .rotation_rule(RotationPlacementRule::Attach { by: Face::NZ })
                .voxels_fn(universe, resolution, |p| {
                    // TODO: fancier/tidier appearance; this was just some tinkering from the original `Lamp` sphere
                    let r2 = int_magnitude_squared(
                        (p * 2 + one_diagonal
                            - center_point_doubled.mul_element_wise(GridPoint::new(1, 1, 0)))
                        .mul_element_wise(GridVector::new(3, 1, 1)),
                    );
                    if r2 <= (resolution_g - 2).pow(2) {
                        &lamp_globe
                    } else if r2 <= (resolution_g + 4).pow(2) && p.z == 0 {
                        &lamppost_metal
                    } else {
                        &AIR
                    }
                })?
                .build(),

            Arrow => {
                let mut space = Space::for_block(resolution).build_empty();

                // Support legs, identifying the down / -Y direction.
                let leg = Block::from(palette::STEEL);
                space.fill_uniform(
                    Grid::new(
                        [resolution_g / 2 - 1, 0, resolution_g / 2 - 1],
                        [2, resolution_g / 2, 2],
                    ),
                    &leg,
                )?;

                // Arrow body
                let base_body_color = rgb_const!(0.5, 0.5, 0.5);
                space.fill(space.grid(), |p| {
                    // TODO: We really need better procgen tools
                    let p2 = p * 2 + one_diagonal - center_point_doubled;
                    let r = p2
                        .mul_element_wise(Vector3::new(1, 1, 0))
                        .map(|c| f64::from(c) / 2.0)
                        .magnitude();
                    let body_radius = if p.z < 6 {
                        f64::from(p.z) / 1.5 + 1.0
                    } else {
                        2.0
                    };
                    if r < body_radius {
                        // Shade the body with an axis-indicating color.
                        Some(Block::from(Rgb::new(
                            base_body_color.red().into_inner()
                                + (p2.x as f32 / resolution_g as f32),
                            base_body_color.green().into_inner()
                                + (p2.y as f32 / resolution_g as f32),
                            base_body_color.red().into_inner(),
                        )))
                    } else {
                        None
                    }
                })?;

                Block::builder()
                    .display_name("Arrow")
                    .collision(BlockCollision::Recur)
                    .rotation_rule(RotationPlacementRule::Attach { by: Face::NY })
                    .voxels_ref(resolution, universe.insert_anonymous(space))
                    .build()
            }

            Curb => Block::builder()
                .display_name("Curb")
                .collision(BlockCollision::Recur)
                // TODO: rotation should specify curb line direction
                .rotation_rule(RotationPlacementRule::Attach { by: Face::NY })
                .voxels_fn(universe, resolution, curb_fn)?
                .build(),

            CurbCorner => Block::builder()
                .display_name("Curb Corner")
                .collision(BlockCollision::Recur)
                .rotation_rule(RotationPlacementRule::Attach { by: Face::NY })
                .voxels_fn(universe, resolution, |cube| {
                    // TODO: rework so this isn't redoing the rotation calculations for every single voxel
                    // We should have tools for composing blocks instead...
                    for rot in GridRotation::CLOCKWISE.iterate() {
                        let block = curb_fn(
                            rot.to_positive_octant_matrix(resolution.into())
                                .transform_cube(cube),
                        );
                        if block != AIR {
                            return block;
                        }
                    }
                    AIR
                })?
                .build(),

            ExhibitBackground => {
                let colors = [
                    Block::from(Rgba::new(0.825, 0.825, 0.825, 1.0)),
                    Block::from(Rgba::new(0.75, 0.75, 0.75, 1.0)),
                ];
                Block::builder()
                    .display_name("Exhibit Background")
                    .voxels_fn(universe, 4, |cube| {
                        &colors[(cube.x + cube.y + cube.z).rem_euclid(2) as usize]
                    })?
                    .build()
            }

            Signboard => {
                let sign_board = Block::from(palette::PLANK);
                let sign_post = Block::from(palette::STEEL);

                // This shape has to coordinate with the name-drawing code in city::demo_city.
                // Haven't thought of a good way to abstract/combine it yet.
                let resolution = 16;
                let top_edge = 10;

                let mut space = Space::for_block(resolution).build_empty();

                // Sign board
                {
                    // space.fill_uniform(
                    //     Grid::from_lower_upper([0, 8, 15], [16, 16, 16]),
                    //     &sign_backing,
                    // )?;
                    let mut plane = space.draw_target(GridMatrix::from_translation([
                        0,
                        0,
                        (resolution - 1).into(),
                    ]));
                    Rectangle::with_corners(
                        Point::new(0, (resolution / 4).into()),
                        Point::new((resolution - 1).into(), top_edge),
                    )
                    .draw_styled(&PrimitiveStyle::with_fill(&sign_board), &mut plane)?;
                }

                // Support posts
                let mut post = |x| -> Result<(), InGenError> {
                    let mut plane = space.draw_target(
                        GridMatrix::from_translation([x, 0, 0])
                            * GridRotation::RZYX.to_rotation_matrix(),
                    );
                    let style = &PrimitiveStyle::with_stroke(&sign_post, 2);
                    let z = (resolution - 3).into();
                    // Vertical post
                    Line::new(Point::new(z, 0), Point::new(z, top_edge - 1))
                        .draw_styled(style, &mut plane)?;
                    // Diagonal brace
                    Line::new(Point::new(z - 4, 0), Point::new(z, 4))
                        .draw_styled(style, &mut plane)?;
                    Ok(())
                };
                post(2)?;
                post((resolution - 3).into())?;

                Block::builder()
                    .display_name("Signboard")
                    .collision(BlockCollision::Recur)
                    .rotation_rule(RotationPlacementRule::Attach { by: Face::NY })
                    .voxels_ref(resolution, universe.insert_anonymous(space))
                    .build()
            }
        })
    })?
    .install(universe)?;

    p.progress(1.0).await;
    Ok(())
}

/// Generate a copy of a [`Block::Atom`] with its color scaled by the given scalar.
///
/// The scalar is rounded to steps of `quantization`, to reduce the number of distinct
/// block types generated.
///
/// If the computation is NaN or the block is not an atom, it is returned unchanged.
pub(crate) fn scale_color(block: Block, scalar: f64, quantization: f64) -> Block {
    let scalar = (scalar / quantization).round() * quantization;
    match (block, NotNan::new(scalar as f32)) {
        (Block::Atom(attributes, color), Ok(scalar)) => Block::Atom(
            attributes,
            (color.to_rgb() * scalar).with_alpha(color.alpha()),
        ),
        (block, _) => block,
    }
}

/// Subdivide the range 0.0 to 1.0 into `gradient.len()` parts and return the [`Block`]
/// which the value falls into.
///
/// Panics if `gradient.len() == 0`.
pub(crate) fn gradient_lookup(gradient: &[Block], value: f32) -> &Block {
    &gradient[((value * gradient.len() as f32) as usize).clamp(0, gradient.len() - 1)]
}

/// Compute the cube's distance from the midpoint of the Y axis of the block grid.
///
/// If the resolution is even, then the centermost 4 cubes all have a distance of 1.
/// If the resolution is odd, then there is only 1 cube that has a distance of 1.
/// (No cube has a distance of 0, so 0 can be used in a comparison for “never”.)
///
/// The first returned number is the "radius" value and the second is the distance
/// on the lesser axis, which may be used for distance from the center or corner along
/// the surface.
fn square_radius(resolution: Resolution, cube: GridPoint) -> [GridCoordinate; 2] {
    let distances_vec = cube.map(|c| (c * 2 + 1 - GridCoordinate::from(resolution)).abs() / 2 + 1);
    if distances_vec.x > distances_vec.z {
        [distances_vec.x, distances_vec.z]
    } else {
        [distances_vec.z, distances_vec.x]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use all_is_cubes::content::make_some_blocks;

    #[test]
    pub fn install_demo_blocks_test() {
        let mut universe = Universe::new();
        futures_executor::block_on(async {
            install_demo_blocks(&mut universe, YieldProgress::noop())
                .await
                .unwrap()
        });
        // TODO: assert what entries were created, once Universe has iteration
    }

    #[test]
    fn gradient_lookup_cases() {
        let blocks = make_some_blocks::<4>();
        let inputs_and_output_indices = [
            (-f32::INFINITY, 0),
            (-10.0, 0),
            (-0.1, 0),
            (0.0, 0),
            (0.24, 0),
            (0.25, 1),
            (0.26, 1),
            (0.49, 1),
            (0.50, 2),
            (0.51, 2),
            (0.9, 3),
            (1.0, 3),
            (1.1, 3),
            (10.0, 3),
            (f32::INFINITY, 3),
            (f32::NAN, 0),
        ];
        assert_eq!(
            inputs_and_output_indices.map(|(i, _)| gradient_lookup(&blocks, i)),
            inputs_and_output_indices.map(|(_, o)| &blocks[o]),
        );
    }

    #[test]
    fn square_radius_cases() {
        assert_eq!(
            [6, 7, 8, 9].map(|x| square_radius(16, GridPoint::new(x, 2, 8))[0]),
            [2, 1, 1, 2]
        );

        // TODO: Make this work
        assert_eq!(
            [3, 4, 5, 6].map(|x| square_radius(9, GridPoint::new(x, 2, 4))[0]),
            [2, 1, 2, 3]
        );
    }
}
