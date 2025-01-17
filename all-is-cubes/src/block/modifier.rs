// Copyright 2020-2022 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

use cgmath::Zero;

use crate::block::{
    Block, BlockAttributes, BlockChange, BlockCollision, EvalBlockError, EvaluatedBlock, Evoxel,
    AIR,
};
use crate::drawing::VoxelBrush;
use crate::listen::Listener;
use crate::math::{Face6, GridCoordinate, GridRotation, Rgb, Rgba};
use crate::space::{Grid, GridArray};
use crate::universe::{RefVisitor, VisitRefs};

/// Modifiers can be applied to a [`Block`] to change the result of
/// [`evaluate()`](Block::evaluate)ing it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Modifier {
    /// Suppresses all behaviors of the [`Block`] that might affect the space around it,
    /// (or itself).
    // TODO: figure out how to publicly construct this given that we want to still have
    // the option to add variants
    #[non_exhaustive]
    Quote {
        /// If true, also suppress light and sound effects.
        ambient: bool,
    },

    /// Rotate the block about its cube center by the given rotation.
    Rotate(GridRotation),

    /// Displace the block out of the grid, cropping it. A pair of `Move`s can depict a
    /// block moving between two cubes.
    ///
    /// **Animation:**
    ///
    /// * If the `distance` is zero then the modifier will remove itself, if possible,
    ///   on the next tick.
    /// * If the `distance` and `velocity` are such that the block is out of view and will
    ///   never strt being in view, the block will be replaced with [`AIR`].
    ///
    /// (TODO: Define the conditions for “if possible”.)
    // #[non_exhaustive] // TODO: needs a constructor
    Move {
        /// The direction in which the block is displaced.
        direction: Face6,
        /// The distance, in 1/256ths, by which it is displaced.
        distance: u16,
        /// The velocity **per tick** with which the displacement is changing.
        ///
        /// TODO: "Per tick" is a bad unit.
        velocity: i16,
    },
}

impl Modifier {
    /// Return the given `block` with `self` added as the outermost modifier.
    pub fn attach(self, mut block: Block) -> Block {
        block.modifiers_mut().push(self);
        block
    }

    /// Compute the effect of this modifier.
    ///
    /// * `block` is the original block value (modifiers do not alter it).
    /// * `this_modifier_index` is the index in `block.modifiers()` of `self`.
    /// * `value` is the output of the preceding modifier or primitive, which is what the
    ///   current modifier should be applied to.
    /// * `depth` is the current block evaluation recursion depth (which is *not*
    ///   incremented by modifiers; TODO: define a computation limit strategy).
    pub(crate) fn evaluate(
        &self,
        block: &Block,
        this_modifier_index: usize,
        mut value: EvaluatedBlock,
        depth: u8,
    ) -> Result<EvaluatedBlock, EvalBlockError> {
        Ok(match *self {
            Modifier::Quote { ambient } => {
                value.attributes.tick_action = None;
                if ambient {
                    value.attributes.light_emission = Rgb::ZERO;
                }
                value
            }

            Modifier::Rotate(rotation) => {
                if value.voxels.is_none() && value.voxel_opacity_mask.is_none() {
                    // Skip computation of transforms
                    value
                } else {
                    // TODO: Add a shuffle-in-place rotation operation to GridArray and try implementing this using that, which should have less arithmetic involved than these matrix ops
                    let resolution = value.resolution;
                    let inner_to_outer = rotation.to_positive_octant_matrix(resolution.into());
                    let outer_to_inner = rotation
                        .inverse()
                        .to_positive_octant_matrix(resolution.into());

                    EvaluatedBlock {
                        voxels: value.voxels.map(|voxels| {
                            GridArray::from_fn(
                                voxels.grid().transform(inner_to_outer).unwrap(),
                                |cube| voxels[outer_to_inner.transform_cube(cube)],
                            )
                        }),
                        voxel_opacity_mask: value.voxel_opacity_mask.map(|mask| {
                            GridArray::from_fn(
                                mask.grid().transform(inner_to_outer).unwrap(),
                                |cube| mask[outer_to_inner.transform_cube(cube)],
                            )
                        }),

                        // Unaffected
                        attributes: value.attributes,
                        color: value.color,
                        resolution,
                        opaque: value.opaque,
                        visible: value.visible,
                    }
                }
            }

            Modifier::Move {
                direction,
                distance,
                velocity,
            } => {
                // Apply Quote to ensure that the block's own `tick_action` and other effects
                // don't interfere with movement or cause duplication.
                // (In the future we may want a more nuanced policy that allows internal changes,
                // but that will probably involve refining tick_action processing.)
                value = Modifier::Quote { ambient: false }.evaluate(
                    block,
                    this_modifier_index,
                    value,
                    depth,
                )?;

                let (original_bounds, effective_resolution) = match value.voxels.as_ref() {
                    Some(array) => (array.grid(), value.resolution),
                    // Treat color blocks as having a resolution of 16. TODO: Improve on this hardcoded constant.
                    None => (Grid::for_block(16), 16),
                };

                // For now, our strategy is to work in units of the block's resolution.
                let distance_in_res = GridCoordinate::from(distance)
                    * GridCoordinate::from(effective_resolution)
                    / 256;
                let translation_in_res = direction.normal_vector() * distance_in_res;

                // This will be None if the displacement puts the block entirely out of view.
                let displaced_bounds: Option<Grid> = original_bounds
                    .translate(translation_in_res)
                    .intersection(Grid::for_block(effective_resolution));

                let animation_action = if displaced_bounds.is_none() && velocity >= 0 {
                    // Displaced to invisibility; turn into just plain air.
                    Some(VoxelBrush::single(AIR))
                } else if translation_in_res.is_zero() && velocity == 0
                    || distance == 0 && velocity < 0
                {
                    // Either a stationary displacement which is invisible, or an animated one which has finished its work.
                    assert_eq!(&block.modifiers()[this_modifier_index], self);
                    let mut new_block = block.clone();
                    new_block.modifiers_mut().remove(this_modifier_index); // TODO: What if other modifiers want to do things?
                    Some(VoxelBrush::single(new_block))
                } else if velocity != 0 {
                    // Movement in progress.
                    assert_eq!(&block.modifiers()[this_modifier_index], self);
                    let mut new_block = block.clone();
                    if let Modifier::Move {
                        distance, velocity, ..
                    } = &mut new_block.modifiers_mut()[this_modifier_index]
                    {
                        *distance = i32::from(*distance)
                            .saturating_add(i32::from(*velocity))
                            .clamp(0, i32::from(u16::MAX))
                            .try_into()
                            .unwrap(/* clamped to range */);
                    }
                    Some(VoxelBrush::single(new_block))
                } else {
                    // Stationary displacement; take no action
                    None
                };

                // Used by the solid color case; we have to do this before we move `attributes`
                // out of `value`.
                let voxel = Evoxel::from_block(&value);

                let attributes = BlockAttributes {
                    // Switch to `Recur` collision so that the displacement collides as expected.
                    // TODO: If the collision was `Hard` then we may need to edit the collision
                    // values of the individual voxels to preserve expected behavior.
                    collision: match value.attributes.collision {
                        BlockCollision::None => BlockCollision::None,
                        BlockCollision::Hard | BlockCollision::Recur => {
                            if displaced_bounds.is_some() {
                                BlockCollision::Recur
                            } else {
                                // Recur treats no-voxels as Hard, which is not what we want
                                BlockCollision::None
                            }
                        }
                    },
                    tick_action: animation_action,
                    ..value.attributes
                };

                match displaced_bounds {
                    Some(displaced_bounds) => {
                        let displaced_voxels = match value.voxels.as_ref() {
                            Some(voxels) => GridArray::from_fn(displaced_bounds, |cube| {
                                voxels[cube - translation_in_res]
                            }),
                            None => {
                                // Input block is a solid color; synthesize voxels.
                                GridArray::from_fn(displaced_bounds, |_| voxel)
                            }
                        };
                        EvaluatedBlock::from_voxels(
                            attributes,
                            effective_resolution,
                            displaced_voxels,
                        )
                    }
                    None => EvaluatedBlock::from_color(attributes, Rgba::TRANSPARENT),
                }
            }
        })
    }

    /// Called by [`Block::listen()`]; not designed to be used otherwise.
    pub(crate) fn listen_impl(
        &self,
        _listener: &(impl Listener<BlockChange> + Clone + Send + Sync),
        _depth: u8,
    ) -> Result<(), EvalBlockError> {
        match self {
            Modifier::Quote { .. } => {}
            Modifier::Rotate(_) => {}
            Modifier::Move { .. } => {}
        }
        Ok(())
    }

    /// Create a pair of [`Modifier::Move`]s to displace a block.
    /// The first goes on the block being moved and the second on the air
    /// it's moving into.
    ///
    /// TODO: This is going to need to change again in order to support
    /// moving one block in and another out at the same time.
    pub fn paired_move(direction: Face6, distance: u16, velocity: i16) -> [Modifier; 2] {
        [
            Modifier::Move {
                direction,
                distance,
                velocity,
            },
            Modifier::Move {
                direction: direction.opposite(),
                distance: 256 - distance,
                velocity: -velocity,
            },
        ]
    }
}

impl VisitRefs for Modifier {
    fn visit_refs(&self, _visitor: &mut dyn RefVisitor) {
        match self {
            Modifier::Quote { .. } => {}
            Modifier::Rotate(..) => {}
            Modifier::Move {
                direction: _,
                distance: _,
                velocity: _,
            } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockAttributes, BlockCollision, Evoxel, Primitive, AIR};
    use crate::content::{make_some_blocks, make_some_voxel_blocks};
    use crate::drawing::VoxelBrush;
    use crate::math::{GridPoint, OpacityCategory, Rgba};
    use crate::space::{Grid, Space};
    use crate::time::Tick;
    use crate::universe::Universe;
    use cgmath::EuclideanSpace;
    use pretty_assertions::assert_eq;

    #[test]
    fn quote_evaluation() {
        let l = Rgb::new(1.0, 2.0, 3.0);
        let mut block = Block::builder()
            .light_emission(l)
            .color(Rgba::WHITE)
            .build();
        assert_eq!(block.evaluate().unwrap().attributes.light_emission, l);
        block
            .modifiers_mut()
            .push(Modifier::Quote { ambient: true });
        assert_eq!(
            block.evaluate().unwrap().attributes.light_emission,
            Rgb::ZERO
        );
    }

    // Unlike other tests, this one asserts the entire `EvaluatedBlock` value because
    // a new field is a potential bug.
    #[test]
    fn rotate_evaluation() {
        let resolution = 2;
        let block_grid = Grid::for_block(resolution);
        let rotation = GridRotation::RYXZ;
        let mut universe = Universe::new();
        let color_fn = |cube: GridPoint| {
            Rgba::new(
                cube.x as f32,
                cube.y as f32,
                cube.z as f32,
                if cube.y == 0 { 1.0 } else { 0.0 },
            )
        };
        let rotated_color_fn = |cube: GridPoint| {
            color_fn(
                rotation
                    .to_positive_octant_matrix(resolution.into())
                    .transform_cube(cube),
            )
        };
        let block = Block::builder()
            .voxels_fn(&mut universe, resolution, |cube| {
                // Construct a lower half block with all voxels distinct
                Block::from(color_fn(cube))
            })
            .unwrap()
            .build();
        let rotated = block.clone().rotate(rotation);
        assert_eq!(
            rotated.evaluate().unwrap(),
            EvaluatedBlock {
                attributes: BlockAttributes::default(),
                color: rgba_const!(0.5, 0.5, 0.5, 0.5),
                voxels: Some(GridArray::from_fn(block_grid, |cube| {
                    Evoxel {
                        color: rotated_color_fn(cube),
                        selectable: true,
                        collision: BlockCollision::Hard,
                    }
                })),
                resolution: 2,
                opaque: false,
                visible: true,
                voxel_opacity_mask: Some(GridArray::from_fn(block_grid, |cube| {
                    if cube.x == 0 {
                        OpacityCategory::Opaque
                    } else {
                        OpacityCategory::Invisible
                    }
                })),
            }
        );
    }

    /// Check that [`Block::rotate`]'s pre-composition is consistent with the interpretation
    /// used by evaluating [`Modifier::Rotate`].
    #[test]
    fn rotate_rotated_consistency() {
        let mut universe = Universe::new();
        let [block] = make_some_voxel_blocks(&mut universe);
        assert!(matches!(block.primitive(), Primitive::Recur { .. }));

        // Two rotations not in the same plane, so they are not commutative.
        let rotation_1 = GridRotation::RyXZ;
        let rotation_2 = GridRotation::RXyZ;

        let rotated_twice = block.clone().rotate(rotation_1).rotate(rotation_2);
        let mut two_rotations = block.clone();
        two_rotations
            .modifiers_mut()
            .extend([Modifier::Rotate(rotation_1), Modifier::Rotate(rotation_2)]);
        assert_ne!(rotated_twice, two_rotations, "Oops; test is ineffective");

        assert_eq!(
            rotated_twice.evaluate().unwrap(),
            two_rotations.evaluate().unwrap()
        );
    }

    #[test]
    fn move_atom_block_evaluation() {
        let color = rgba_const!(1.0, 0.0, 0.0, 1.0);
        let original = Block::from(color);
        let modifier = Modifier::Move {
            direction: Face6::PY,
            distance: 128, // distance 1/2 block × scale factor of 256
            velocity: 0,
        };
        let moved = modifier.attach(original.clone());

        let expected_bounds = Grid::new([0, 8, 0], [16, 8, 16]);

        let ev_original = original.evaluate().unwrap();
        assert_eq!(
            moved.evaluate().unwrap(),
            EvaluatedBlock {
                attributes: BlockAttributes {
                    collision: BlockCollision::Recur,
                    ..ev_original.attributes.clone()
                },
                color: color.to_rgb().with_alpha(notnan!(0.5)),
                voxels: GridArray::from_elements(
                    expected_bounds,
                    vec![Evoxel::from_block(&ev_original); expected_bounds.volume()]
                ),
                resolution: 16,
                opaque: false,
                visible: true,
                voxel_opacity_mask: GridArray::from_elements(
                    expected_bounds,
                    vec![OpacityCategory::Opaque; expected_bounds.volume()]
                ),
            }
        );
    }

    #[test]
    fn move_voxel_block_evaluation() {
        let mut universe = Universe::new();
        let resolution = 2;
        let color = rgba_const!(1.0, 0.0, 0.0, 1.0);
        let original = Block::builder()
            .voxels_fn(&mut universe, resolution, |_| Block::from(color))
            .unwrap()
            .build();

        let modifier = Modifier::Move {
            direction: Face6::PY,
            distance: 128, // distance 1/2 block × scale factor of 256
            velocity: 0,
        };
        let moved = modifier.attach(original.clone());

        let expected_bounds = Grid::new([0, 1, 0], [2, 1, 2]);

        let ev_original = original.evaluate().unwrap();
        assert_eq!(
            moved.evaluate().unwrap(),
            EvaluatedBlock {
                attributes: BlockAttributes {
                    collision: BlockCollision::Recur,
                    ..ev_original.attributes.clone()
                },
                color: color.to_rgb().with_alpha(notnan!(0.5)),
                voxels: GridArray::from_elements(
                    expected_bounds,
                    vec![Evoxel::from_block(&ev_original); expected_bounds.volume()]
                ),
                resolution,
                opaque: false,
                visible: true,
                voxel_opacity_mask: GridArray::from_elements(
                    expected_bounds,
                    vec![OpacityCategory::Opaque; expected_bounds.volume()]
                ),
            }
        );
    }

    /// [`Modifier::Move`] incorporates [`Modifier::Quote`] to ensure that no conflicting
    /// effects happen.
    #[test]
    fn move_also_quotes() {
        let original = Block::builder()
            .color(Rgba::WHITE)
            .tick_action(Some(VoxelBrush::single(AIR)))
            .build();
        let moved = Modifier::Move {
            direction: Face6::PY,
            distance: 128,
            velocity: 0,
        }
        .attach(original);

        assert_eq!(moved.evaluate().unwrap().attributes.tick_action, None);
    }

    /// Set up a `Modifier::Move`, let it run, and then allow assertions to be made about the result.
    fn move_block_test(direction: Face6, velocity: i16, checker: impl FnOnce(&Space, &Block)) {
        let [block] = make_some_blocks();
        let mut space =
            Space::builder(Grid::from_lower_upper([-1, -1, -1], [2, 2, 2])).build_empty();
        let [move_out, move_in] = Modifier::paired_move(direction, 0, velocity);
        space
            .set([0, 0, 0], move_out.attach(block.clone()))
            .unwrap();
        space
            .set(
                GridPoint::origin() + direction.normal_vector(),
                move_in.attach(block.clone()),
            )
            .unwrap();
        let mut universe = Universe::new();
        let space = universe.insert_anonymous(space);
        // TODO: We need a "step until idle" function, or for the UniverseStepInfo to convey how many blocks were updated / are waiting
        // TODO: Some tests will want to look at the partial results
        for _ in 0..257 {
            universe.step(Tick::arbitrary());
        }
        checker(&space.borrow(), &block);
    }

    #[test]
    fn move_zero_velocity() {
        move_block_test(Face6::PX, 0, |space, block| {
            assert_eq!(&space[[0, 0, 0]], block);
            assert_eq!(&space[[1, 0, 0]], &AIR);
        });
    }

    #[test]
    fn move_slowly() {
        move_block_test(Face6::PX, 1, |space, block| {
            assert_eq!(&space[[0, 0, 0]], &AIR);
            assert_eq!(&space[[1, 0, 0]], block);
        });
    }

    #[test]
    fn move_instant_velocity() {
        move_block_test(Face6::PX, 256, |space, block| {
            assert_eq!(&space[[0, 0, 0]], &AIR);
            assert_eq!(&space[[1, 0, 0]], block);
        });
    }
}
