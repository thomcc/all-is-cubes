// Copyright 2020-2021 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

//! Projection and view matrices, viewport and aspect ratio, visibility,
//! raycasting into the scene, etc.

use cgmath::{
    Deg, EuclideanSpace as _, InnerSpace as _, Matrix as _, Matrix4, Point2, Point3, SquareMatrix,
    Transform, Vector2, Vector3, Vector4,
};
use itertools::Itertools as _;
use ordered_float::NotNan;

use crate::math::{Aab, FreeCoordinate};
use crate::raycast::Ray;
use crate::space::Grid;

type M = Matrix4<FreeCoordinate>;

/// Defines a viewpoint in/of the world: a viewport (aspect ratio), projection matrix,
/// and view matrix.
#[derive(Clone, Debug)]
pub struct Camera {
    /// Caller-provided options. Always validated by [`GraphicsOptions::repair`].
    options: GraphicsOptions,

    /// Caller-provided viewport.
    viewport: Viewport,

    /// Caller-provided view matrix.
    view_matrix: M,

    /// Projection matrix derived from viewport and options.
    /// Calculated by [`Self::compute_matrices`].
    projection: M,

    /// View point derived from view matrix.
    /// Calculated by [`Self::compute_matrices`].
    view_position: Point3<FreeCoordinate>,

    /// Inverse of `projection * view_matrix`.
    /// Calculated by [`Self::compute_matrices`].
    inverse_projection_view: M,
    view_frustum_corners: [Point3<FreeCoordinate>; 8],

    /// Position of mouse pointer or other input device in normalized device coordinates
    /// (range -1 to 1 upward and rightward). If there is no such input device, zero may
    /// be used to designate the center of the screen.
    ///
    /// TODO: This doesn't really belong here. Look for better data flow routes.
    pub cursor_ndc_position: Option<Vector2<FreeCoordinate>>,
}

#[allow(clippy::cast_lossless)]
impl Camera {
    pub fn new(options: GraphicsOptions, viewport: Viewport) -> Self {
        let mut new_self = Self {
            options: options.repair(),
            viewport,
            view_matrix: M::identity(),
            cursor_ndc_position: None,

            // Overwritten immediately by compute_matrices
            projection: M::identity(),
            view_position: Point3::origin(),
            inverse_projection_view: M::identity(),
            view_frustum_corners: [Point3::origin(); 8],
        };
        new_self.compute_matrices();
        new_self
    }

    /// Returns the viewport value last provided.
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Returns the [`GraphicsOptions`] value last provided (possibly with adjusted values).
    pub fn options(&self) -> &GraphicsOptions {
        &self.options
    }

    pub fn set_options(&mut self, options: GraphicsOptions) {
        self.options = options.repair();
    }

    /// Sets the contained viewport value, and recalculates matrices to be suitable for
    /// the new viewport's aspect ratio.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        if viewport != self.viewport {
            self.viewport = viewport;
            self.compute_matrices();
        }
    }

    /// Returns the field of view, expressed in degrees on the vertical axis (that is, the
    /// horizontal field of view depends on the viewport's aspect ratio).
    /// This differs from the value in [`GraphicsOptions`] by being clamped to valid values.
    pub fn fov_y(&self) -> Deg<FreeCoordinate> {
        Deg(self.options.fov_y.into_inner())
    }

    /// Returns the view distance; the far plane of the projection matrix, or the distance
    /// at which rendering may be truncated.
    pub fn view_distance(&self) -> FreeCoordinate {
        self.options.view_distance.into_inner()
    }

    /// Set the current cursor position. In the same pixel units as `set_viewport`.
    pub fn set_cursor_position(&mut self, position: Option<Point2<usize>>) {
        self.cursor_ndc_position = position.map(|p| self.viewport.normalize_nominal_point(p));
    }

    /// Sets the view matrix.
    ///
    /// This matrix is used to determine world coordinates for purposes of
    /// [`view_position`](Self::view_position),
    /// [`project_ndc_into_world`](Self::project_ndc_into_world),
    /// and [`project_cursor_into_world`](Self::project_cursor_into_world).
    /// to determine what world coordinates are.
    pub fn set_view_matrix(&mut self, view_matrix: M) {
        if view_matrix != self.view_matrix {
            self.view_matrix = view_matrix;
            self.compute_matrices();
        }
    }

    /// Returns a projection matrix suitable for OpenGL use.
    pub fn projection(&self) -> M {
        self.projection
    }

    /// Returns a view matrix suitable for OpenGL use.
    pub fn view_matrix(&self) -> M {
        self.view_matrix
    }

    /// Returns the eye position in world coordinates, as set by [`Self::set_view_matrix()`].
    pub fn view_position(&self) -> Point3<FreeCoordinate> {
        self.view_position
    }

    /// Converts a screen position in normalized device coordinates (as produced by
    /// [`Viewport::normalize_nominal_point`]) into a ray in world space.
    /// Uses the view transformation given by [`set_view_matrix`](Self::set_view_matrix).
    pub fn project_ndc_into_world(&self, ndc_x: FreeCoordinate, ndc_y: FreeCoordinate) -> Ray {
        let ndc_near = Vector4::new(ndc_x, ndc_y, -1.0, 1.0);
        let ndc_far = Vector4::new(ndc_x, ndc_y, 1.0, 1.0);
        // World-space endpoints of the ray.
        let world_near = Point3::from_homogeneous(self.inverse_projection_view * ndc_near);
        let world_far = Point3::from_homogeneous(self.inverse_projection_view * ndc_far);
        let direction = world_far - world_near;
        Ray {
            origin: world_near,
            direction,
        }
    }

    fn project_point_into_world(&self, p: Point3<FreeCoordinate>) -> Point3<FreeCoordinate> {
        Point3::from_homogeneous(self.inverse_projection_view * p.to_homogeneous())
    }

    /// Converts the cursor position into a ray in world space.
    /// Uses the view transformation given by [`set_view_matrix`](Self::set_view_matrix).
    pub fn project_cursor_into_world(&self) -> Option<Ray> {
        self.cursor_ndc_position
            .map(|pos| self.project_ndc_into_world(pos.x, pos.y))
    }

    /// Determine whether the given `Aab` is visible in this projection+view.
    pub(crate) fn aab_in_view(&self, aab: Aab) -> bool {
        // Check for the AAB being outside the viewport, using the separating axis theorem.
        // First, check if the view frustum's corner points lie outside the AAB. This is
        // the simpler case since it is axis-aligned.
        for &axis in &[Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()] {
            if Self::separated_along(
                self.view_frustum_corners.iter().copied(),
                aab.corner_points(),
                axis,
            ) {
                return false;
            }
        }

        // Now use the viewport's projected planes to try more separation axes.
        // TODO: The correctness of this depends on Aab::corner_points's point ordering,
        // which is not yet nailed down.
        let [lbf, rbf, ltf, rtf, lbn, rbn, ltn, rtn] = self.view_frustum_corners;
        for &(p1, p2, p3) in &[
            (lbn, lbf, ltf), // left
            (rtn, rtf, rbf), // right
            (ltn, ltf, rtf), // top
            (rbn, rbf, lbf), // bottom
            // Testing against the near plane (lbn, ltn, rtn) is not worthwhile since the
            // frustum is nearly a pyramid — the volume removed is less than a single cube.
            (lbf, rbf, ltf), // far
        ] {
            let normal = (p2 - p1).cross(p3 - p1);
            if Self::separated_along(
                self.view_frustum_corners.iter().copied(),
                aab.corner_points(),
                normal,
            ) {
                return false;
            }
        }
        true
    }

    /// Helper for aab_in_view; finds if two sets of points' projections onto a line intersect.
    #[inline]
    fn separated_along(
        points1: impl IntoIterator<Item = Point3<FreeCoordinate>>,
        points2: impl IntoIterator<Item = Point3<FreeCoordinate>>,
        axis: Vector3<FreeCoordinate>,
    ) -> bool {
        let (min1, max1) = Self::projected_range(points1, axis);
        let (min2, max2) = Self::projected_range(points2, axis);
        let intersection_min = min1.max(min2);
        let intersection_max = max1.min(max2);
        intersection_max < intersection_min
    }

    /// Helper for aab_in_view; projects a set of points onto a line.
    #[inline]
    fn projected_range(
        points: impl IntoIterator<Item = Point3<FreeCoordinate>>,
        axis: Vector3<FreeCoordinate>,
    ) -> (FreeCoordinate, FreeCoordinate) {
        points
            .into_iter()
            .map(|p| p.to_vec().dot(axis))
            .minmax()
            .into_option()
            .unwrap()
    }

    fn compute_matrices(&mut self) {
        self.projection = cgmath::perspective(
            self.fov_y(),
            self.viewport.nominal_aspect_ratio(),
            /* near: */ 1. / 32., // half a voxel at resolution=16
            /* far: */ self.view_distance(),
        );
        self.view_position = Point3::from_vec(
            self.view_matrix
                .inverse_transform()
                .expect("view matrix was not invertible")
                .transpose()
                .row(3)
                .truncate(),
        );
        self.inverse_projection_view = (self.projection * self.view_matrix)
            .inverse_transform()
            .expect("projection and view matrix was not invertible");

        // Compute the view frustum's corner points.
        let mut view_frustum_corners: [Point3<FreeCoordinate>; 8] = [Point3::origin(); 8];
        for (i, point) in Aab::new(-1., 1., -1., 1., -1., 1.)
            .corner_points()
            .map(|p| self.project_point_into_world(p))
            .enumerate()
        {
            view_frustum_corners[i] = point;
        }
        self.view_frustum_corners = view_frustum_corners;
    }
}

/// Viewport dimensions for rendering and UI layout with the correct resolution and
/// aspect ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// Viewport dimensions to use for determining aspect ratio and interpreting
    /// pointer events.
    pub nominal_size: Vector2<FreeCoordinate>,
    /// Viewport dimensions to use for framebuffer configuration.
    /// This aspect ratio may differ to represent non-square pixels.
    pub framebuffer_size: Vector2<u32>,
}

impl Viewport {
    #![allow(clippy::cast_lossless)] // lossiness depends on size of usize

    /// Calculates the aspect ratio (width divided by height) of the `nominal_size` of this
    /// viewport.
    #[inline]
    pub fn nominal_aspect_ratio(&self) -> FreeCoordinate {
        self.nominal_size.x / self.nominal_size.y
    }

    /// Convert an *x* coordinate from the range `0..self.framebuffer_size.x` (upper exclusive)
    /// to OpenGL normalized device coordinates, range -1 to 1 (at pixel centers).
    #[inline]
    pub fn normalize_fb_x(&self, x: usize) -> FreeCoordinate {
        (x as FreeCoordinate + 0.5) / FreeCoordinate::from(self.framebuffer_size.x) * 2.0 - 1.0
    }

    /// Convert a *y* coordinate from the range `0..self.framebuffer_size.y` (upper exclusive)
    /// to OpenGL normalized device coordinates, range -1 to 1 (at pixel centers) and flipped.
    #[inline]
    pub fn normalize_fb_y(&self, y: usize) -> FreeCoordinate {
        -((y as FreeCoordinate + 0.5) / FreeCoordinate::from(self.framebuffer_size.y) * 2.0 - 1.0)
    }

    /// Convert a point in the `self.nominal_size` coordinate system to
    /// to OpenGL normalized device coordinates, range -1 to 1 (at pixel centers) with Y flipped.
    ///
    /// TODO: this should take float input, probably
    #[inline]
    pub fn normalize_nominal_point(&self, nominal_point: Point2<usize>) -> Vector2<FreeCoordinate> {
        Vector2::new(
            (nominal_point.x as FreeCoordinate + 0.5) / self.nominal_size.x * 2.0 - 1.0,
            -((nominal_point.y as FreeCoordinate + 0.5) / self.nominal_size.y * 2.0 - 1.0),
        )
    }

    // TODO: Maybe have a validate() that checks if the data is not fit for producing an
    // invertible transform.
}

/// User/debug options for rendering (i.e. not affecting gameplay except informationally).
/// Not all of these options are applicable to all renderers.
///
/// TODO: This may not be the best module location. Possibly it should get its own module.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct GraphicsOptions {
    /// Field of view, in degrees from top to bottom edge of the viewport.
    pub fov_y: NotNan<FreeCoordinate>,

    /// Distance, in unit cubes, from the camera to the farthest visible point.
    pub view_distance: NotNan<FreeCoordinate>,

    /// Draw collision boxes for some objects.
    pub debug_collision_boxes: bool,

    /// Draw the light rays that contribute to the selected block.
    pub debug_light_rays_at_cursor: bool,
    // TODO: Add fog options.
}

impl GraphicsOptions {
    /// Constrain fields to valid/practical values.
    pub fn repair(mut self) -> Self {
        self.fov_y = self
            .fov_y
            .max(NotNan::new(1.0).unwrap())
            .min(NotNan::new(189.0).unwrap());
        self.view_distance = self
            .view_distance
            .max(NotNan::new(1.0).unwrap())
            .min(NotNan::new(10000.0).unwrap());
        self
    }
}

impl Default for GraphicsOptions {
    fn default() -> Self {
        Self {
            fov_y: NotNan::new(90.).unwrap(),
            view_distance: NotNan::new(200.).unwrap(),
            debug_collision_boxes: false,
            debug_light_rays_at_cursor: false,
        }
    }
}

/// Calculate an “eye position” (camera position) to view the entire given `grid`.
///
/// `direction` points in the direction the camera should be relative to the space.
///
/// TODO: This function does not yet consider the effects of field-of-view,
/// and it will need additional parameters to do so.
pub fn eye_for_look_at(grid: Grid, direction: Vector3<FreeCoordinate>) -> Point3<FreeCoordinate> {
    let mut space_radius: FreeCoordinate = 0.0;
    for axis in 0..3 {
        space_radius = space_radius.max(grid.size()[axis].into());
    }
    grid.center() + direction.normalize() * space_radius // TODO: allow for camera FoV
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMMY_VIEWPORT: Viewport = Viewport {
        nominal_size: Vector2::new(2.0, 2.0),
        framebuffer_size: Vector2::new(2, 2),
    };

    #[test]
    fn camera_bad_viewport_doesnt_panic() {
        Camera::new(
            GraphicsOptions::default(),
            Viewport {
                nominal_size: Vector2::new(0.0, 0.0),
                framebuffer_size: Vector2::new(0, 0),
            },
        );
    }

    #[test]
    fn camera_view_position() {
        let mut camera = Camera::new(GraphicsOptions::default(), DUMMY_VIEWPORT);
        let pos = Point3::new(1.0, 2.0, 3.0);
        camera.set_view_matrix(
            Matrix4::from_angle_x(Deg(45.0)) * Matrix4::from_translation(-pos.to_vec()),
        );
        assert_eq!(camera.view_position(), pos);
    }
}
