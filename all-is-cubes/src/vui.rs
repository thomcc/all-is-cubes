// Copyright 2020 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <http://opensource.org/licenses/MIT>.

//! VUI stands for Voxel User Interface.
//!
//! We've got all this rendering and interaction code, so let's reuse it for the
//! GUI as well as the game.

use cgmath::Vector2;
use embedded_graphics::geometry::Point;
use embedded_graphics::prelude::{Drawable, Primitive};
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::style::PrimitiveStyleBuilder;
use std::time::Duration;

use crate::block::{Block, BlockAttributes, AIR};
use crate::drawing::{VoxelBrush, VoxelDisplayAdapter};
use crate::math::{FreeCoordinate, GridCoordinate, GridPoint, RGBA};
use crate::space::{Grid, Space};
use crate::universe::{URef, Universe, UniverseStepInfo};

/// `Vui` builds user interfaces out of voxels. It owns a `Universe` dedicated to the
/// purpose and draws into spaces to form the HUD and menus.
#[derive(Debug)] // TODO: probably not very informative Debug as derived
pub(crate) struct Vui {
    universe: Universe,
    current_space: URef<Space>,
    hud_space: URef<Space>,
    aspect_ratio: FreeCoordinate,
}

impl Vui {
    pub fn new() -> Self {
        let mut universe = Universe::new();
        let hud_space = HudLayout::default().new_space(&mut universe);

        Self {
            universe,
            current_space: hud_space.clone(),
            hud_space,
            aspect_ratio: 4. / 3., // arbitrary placeholder assumption
        }
    }

    // TODO: It'd be more encapsulating if we could provide a _read-only_ reference...
    pub fn current_space(&self) -> &URef<Space> {
        &self.current_space
    }

    pub fn step(&mut self, timestep: Duration) -> UniverseStepInfo {
        self.universe.step(timestep)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct HudLayout {
    size: Vector2<GridCoordinate>,
    toolbar_positions: usize,
}

// TODO: This will probably not make sense once we have aspect ratio adaptations
impl Default for HudLayout {
    fn default() -> Self {
        Self {
            size: Vector2::new(24, 18),
            toolbar_positions: 10,
        }
    }
}

impl HudLayout {
    fn grid(&self) -> Grid {
        Grid::new((-1, -1, 0), (self.size.x + 2, self.size.y + 2, 10))
    }

    // TODO: taking the entire Universe doesn't seem like the best interface
    // but we want room to set up new blocks. Figure out a route for that.
    // TODO: validate this doesn't crash on wonky sizes.
    fn new_space(&self, universe: &mut Universe) -> URef<Space> {
        let Vector2 { x: w, y: h } = self.size;
        let grid = self.grid();
        let mut space = Space::empty(grid);

        if true {
            // Visualization of the bounds of the space we're drawing.
            let frame_block = Block::from(RGBA::new(0.0, 1.0, 1.0, 1.0));
            let mut add_frame = |z| {
                space
                    .fill(&Grid::new((-1, -1, z), (w + 2, h + 2, 1)), |_| {
                        Some(&frame_block)
                    })
                    .unwrap();
                space
                    .fill(&Grid::new((0, 0, z), (w, h, 1)), |_| Some(&AIR))
                    .unwrap();
            };
            add_frame(0);
            add_frame(grid.upper_bounds().z - 1);
        }

        for (index, block) in crate::blockgen::make_some_blocks(self.toolbar_positions)
            .into_iter()
            .enumerate()
        {
            // TODO: don't let the out of bounds error we might be ignoring arise
            let _ = space.set(self.tool_icon_position(index), &block);
        }

        universe.insert_anonymous(space)
    }

    fn tool_icon_position(&self, index: usize) -> GridPoint {
        let stepping = 2;
        let x_start =
            (self.size.x - (self.toolbar_positions as GridCoordinate - 1) * stepping + 1) / 2;
        // TODO: set depth sensibly
        GridPoint::new(x_start + (index as GridCoordinate) * stepping, 0, 9)
    }
}

#[allow(unused)] // TODO: not yet used for real
pub(crate) fn draw_background(space: &mut Space) {
    let grid = *space.grid();
    let background_rect = Rectangle::new(
        Point::new(grid.lower_bounds().x, -grid.upper_bounds().y + 1),
        Point::new(grid.upper_bounds().x - 1, -grid.lower_bounds().y),
    );

    let display = &mut VoxelDisplayAdapter::new(space, GridPoint::new(0, 0, grid.lower_bounds().z));

    let background_block = Block::Atom(BlockAttributes::default(), RGBA::new(0.5, 0.5, 0.5, 1.0));
    let background = VoxelBrush::single(&background_block);
    let frame_block = Block::Atom(BlockAttributes::default(), RGBA::new(0.95, 0.95, 0.95, 1.0));
    let frame = VoxelBrush::single(&frame_block).translate((0, 0, 1));

    background_rect
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_width(1)
                .stroke_color(frame)
                .fill_color(background)
                .build(),
        )
        .draw(display)
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vui_smoke_test() {
        let _ = Vui::new();
    }

    #[test]
    fn background_smoke_test() {
        let mut space = Space::empty_positive(100, 100, 10);
        draw_background(&mut space);
    }
}