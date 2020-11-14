// Copyright 2020 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <http://opensource.org/licenses/MIT>.

//! Continuously moving objects and collision.

use cgmath::{EuclideanSpace as _, InnerSpace as _, Point3, Vector3, Zero};
use std::collections::HashSet;
use std::time::Duration;

use crate::math::{Face, FreeCoordinate, Geometry as _, GridPoint, AAB};
use crate::raycast::{RaycastStep, Raycaster};
use crate::space::Space;
use crate::util::ConciseDebug as _;

/// Close-but-not-intersecting objects are set to this separation.
const POSITION_EPSILON: FreeCoordinate = 1e-6 * 1e-6;
/// Velocities shorter than this are treated as zero, to allow things to come to unchanging rest sooner.
const VELOCITY_EPSILON_SQUARED: FreeCoordinate = 1e-6 * 1e-6;

/// An object with a position, velocity, and (TODO) collision volume.
#[derive(Clone)]
#[non_exhaustive]
pub struct Body {
    // TODO: pub space: Option<URef<Space>>   --- or maybe backwards?
    /// Position.
    pub position: Point3<FreeCoordinate>,
    /// Velocity, in position units per second.
    pub velocity: Vector3<FreeCoordinate>,

    /// Collision volume, defined with `position` as the origin.
    // Thought for the future: switching to a "cylinder" representation (height + radius)
    // would allow for simultaneous collision with multiple spaces with different axes.
    pub collision_box: AAB,

    /// Yaw of the camera look direction, in degrees clockwise from looking towards -Z.
    ///
    /// The preferred range is 0 inclusive to 360 exclusive.
    ///
    /// This does not affect the behavior of the `Body` itself; it has nothing to do with
    /// the direction of the velocity.
    pub yaw: FreeCoordinate,

    /// Pitch of the camera look direction, in degrees downward from looking horixontally.
    ///
    /// The preferred range is -90 to 90, inclusive.
    ///
    /// This does not affect the behavior of the `Body` itself; it has nothing to do with
    /// the direction of the velocity.
    pub pitch: FreeCoordinate,
}

impl std::fmt::Debug for Body {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.debug_struct("Body")
            .field("position", &self.position.as_concise_debug())
            .field("velocity", &self.velocity.as_concise_debug())
            .field("yaw", &self.yaw)
            .field("pitch", &self.pitch)
            .finish()
    }
}

impl Body {
    /// Constructs a `Body` requiring only information that can't be reasonably defaulted.
    pub fn new_minimal(
        position: impl Into<Point3<FreeCoordinate>>,
        collision_box: impl Into<AAB>,
    ) -> Self {
        Self {
            position: position.into(),
            velocity: Vector3::zero(),
            collision_box: collision_box.into(),
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    /// Advances time for the body.
    ///
    /// If `colliding_space` is present then the body may collide with blocks in that space
    /// (constraining possible movement) and `collision_callback` will be called with all
    /// such blocks. It is not guaranteed that `collision_callback` will be called only once
    /// per block.
    pub fn step<CC>(
        &mut self,
        duration: Duration,
        colliding_space: Option<&Space>,
        mut collision_callback: CC,
    ) -> BodyStepInfo
    where
        CC: FnMut(GridPoint),
    {
        let dt = duration.as_secs_f64();
        let mut move_segments = [MoveSegment::default(); 3];

        // TODO: Reset any non-finite values found to allow recovery from glitches.

        // TODO: this.velocity += GRAVITY * dt;
        if self.velocity.magnitude2() <= VELOCITY_EPSILON_SQUARED {
            return BodyStepInfo {
                quiescent: true,
                move_segments,
            };
        }

        let unobstructed_delta_position = self.velocity * dt;

        // Do collision detection and resolution.
        if let Some(space) = colliding_space {
            // TODO: Axis-aligned box collision volume.
            let mut i = 0;
            let mut delta_position = unobstructed_delta_position;
            while delta_position != Vector3::zero() {
                assert!(i < 3, "sliding collision loop did not finish");
                // Each call to collide_and_advance will zero at least one axis of delta_position.
                // The nonzero axes are for sliding movement.
                let (new_delta_position, segment) =
                    self.collide_and_advance(space, &mut collision_callback, delta_position);
                delta_position = new_delta_position;
                move_segments[i] = segment;

                i += 1;
            }
        } else {
            self.position += unobstructed_delta_position;
            move_segments[0] = MoveSegment {
                delta_position: unobstructed_delta_position,
                stopped_by: None,
            };
        }

        // TODO: after gravity, falling-below-the-world protection

        BodyStepInfo {
            quiescent: false,
            move_segments,
        }
    }

    /// Perform a single straight-line position change, stopping at the first obstacle.
    /// Returns the remainder of `delta_position` that should be retried for sliding movement.
    fn collide_and_advance<CC>(
        &mut self,
        space: &Space,
        collision_callback: &mut CC,
        mut delta_position: Vector3<FreeCoordinate>,
    ) -> (Vector3<FreeCoordinate>, MoveSegment)
    where
        CC: FnMut(GridPoint),
    {
        let mut already_colliding: HashSet<GridPoint> = HashSet::new();
        for ray_step in Raycaster::new(self.position, delta_position).within_grid(*space.grid()) {
            if ray_step.t_distance >= 1.0 {
                // Movement is unobstructed in this timestep.
                break;
            }
            if ray_step.face == Face::WITHIN {
                // If we are intersecting a block, we are allowed to leave it; pretend
                // it doesn't exist.
                // TODO: already_colliding is currently useless but will become useful
                // when we generalize to colliding as a box instead of a point.
                already_colliding.insert(ray_step.cube);
                continue;
            }
            if space.get_evaluated(ray_step.cube).attributes.solid
                && !already_colliding.contains(&ray_step.cube)
            {
                // We hit something.
                collision_callback(ray_step.cube);

                // Advance however much straight-line distance is available.
                // But a little bit back from that, to avoid floating point error pushing us
                // into being already colliding next frame.
                let unobstructed_distance_along_ray =
                    (ray_step.t_distance - POSITION_EPSILON / delta_position.magnitude()).max(0.0);
                let unobstructed_delta_position = delta_position * unobstructed_distance_along_ray;
                self.position += unobstructed_delta_position;
                // Figure the distance we have have left.
                delta_position -= unobstructed_delta_position;
                // Convert it to sliding movement for the axes we didn't collide in.
                delta_position[ray_step.face.axis_number()] = 0.0;

                // Absorb velocity in that direction.
                self.velocity[ray_step.face.axis_number()] = 0.0;

                return (
                    delta_position,
                    MoveSegment {
                        delta_position: unobstructed_delta_position,
                        stopped_by: Some(ray_step),
                    },
                );
            }
        }

        // We did not hit anything for the length of the raycast. Proceed unobstructed.
        self.position += delta_position;
        (
            Vector3::zero(),
            MoveSegment {
                delta_position,
                stopped_by: None,
            },
        )
    }

    /// Returns the body's collision box in world coordinates
    /// (`collision_box` translated by `position`).
    ///
    /// ```
    /// use all_is_cubes::math::AAB;
    /// use all_is_cubes::physics::Body;
    ///
    /// let body = Body::new_minimal(
    ///     (0.0, 20.0, 0.0),
    ///     AAB::new(-1.0, 1.0, -2.0, 2.0, -3.0, 3.0)
    /// );
    /// assert_eq!(body.collision_box_abs(), AAB::new(-1.0, 1.0, 18.0, 22.0, -3.0, 3.0));
    /// ```
    pub fn collision_box_abs(&self) -> AAB {
        self.collision_box.translate(self.position.to_vec())
    }

    /// Changes `yaw` and `pitch` to look directly towards the given point within the same
    /// coordinate system as `self.position`.
    pub fn look_at(&mut self, point: impl Into<Point3<FreeCoordinate>>) {
        let direction = point.into() - self.position;
        let horizontal_distance = direction.x.hypot(direction.z);

        self.yaw = (180.0 - (direction.x).atan2(direction.z).to_degrees()).rem_euclid(360.0);
        self.pitch = -(direction.y).atan2(horizontal_distance).to_degrees();
    }
}

/// Diagnostic data returned by `Body::step`. The exact contents of this structure
/// are unstable; use only `Debug` formatting to examine its contents unless you have
/// a specific need for one of the values.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct BodyStepInfo {
    /// Whether movement computation was skipped due to approximately zero velocity.
    pub quiescent: bool,
    /// Details on movement and collision. A single frame's movement may have up to three
    /// segments as differently oriented faces are collided with.
    pub move_segments: [MoveSegment; 3],
}

/// One of the individual straight-line movement segments of a `BodyStepInfo`.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct MoveSegment {
    /// The change in position.
    pub delta_position: Vector3<FreeCoordinate>,
    /// What movement stopped this segment from continuing further.
    /// Note that `self.stopped_by.cube` is not necessarily the cube that
    /// contained an obstruction, as that cube may be off to the side relative to
    /// the ray.
    pub stopped_by: Option<RaycastStep>,
}

impl Default for MoveSegment {
    fn default() -> Self {
        Self {
            delta_position: Vector3::zero(),
            stopped_by: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_physics() {
        let mut body = Body {
            position: Point3::new(0.0, 1.0, 0.0),
            velocity: Vector3::new(2.0, 0.0, 0.0),
            collision_box: AAB::new(-0.5, 0.5, -0.5, 0.5, -0.5, 0.5),
            yaw: 0.0,
            pitch: 0.0,
        };
        body.step(Duration::from_secs(2), None, |_| ());
        assert_eq!(body.position, Point3::new(4.0, 1.0, 0.0));
    }

    // TODO: test having all 3 move segments

    // TODO: test collision

    #[test]
    fn look_at() {
        let do_test = |direction, yaw, pitch| {
            let mut body = Body::new_minimal((10., 0., 0.), AAB::ZERO);
            body.look_at(Point3::new(10., 0., 0.) + Vector3::from(direction));
            println!("{:?} {} {}", direction, yaw, pitch);
            assert_eq!(body.yaw, yaw);
            assert_eq!(body.pitch, pitch);
        };

        do_test((0., 0., -1.), 0., 0.);
        do_test((1., 0., -1.), 45., 0.);
        do_test((1., 0., 0.), 90., 0.);
        do_test((0., 0., 1.), 180., 0.);
        do_test((-1., 0., 0.), 270., 0.);

        // TODO: would be tidier if this is 0 instead; revisit the math
        let exactly_vertical_yaw = 180.;
        do_test((0., 1., 0.), exactly_vertical_yaw, -90.);
        do_test((0., 1., -1.), 0., -45.);
        do_test((0., 0., -1.), 0., 0.);
        do_test((0., -1., -1.), 0., 45.);
        do_test((0., -1., 0.), exactly_vertical_yaw, 90.);
    }

    // TODO: more tests
}
