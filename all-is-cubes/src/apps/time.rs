// Copyright 2020-2021 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

use instant::{Duration, Instant}; // wasm-compatible replacement for std::time::Instant

/// Algorithm for deciding how to execute simulation and rendering frames.
/// Platform-independent; does not consult any clocks, only makes decisions
/// given the provided information.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameClock {
    last_absolute_time: Option<Instant>,
    /// Whether there was a step and we should therefore draw a frame.
    /// TODO: This might go away in favor of actual dirty-notifications.
    render_dirty: bool,
    accumulated_step_time: Duration,
}

impl FrameClock {
    const STEP_LENGTH_MICROS: u64 = 1_000_000 / 60;
    const STEP_LENGTH: Duration = Duration::from_micros(Self::STEP_LENGTH_MICROS);
    /// Number of steps per frame to permit.
    /// This sets how low the frame rate can go below STEP_LENGTH before game time
    /// slows down.
    pub(crate) const CATCH_UP_STEPS: u8 = 2;
    const ACCUMULATOR_CAP: Duration =
        Duration::from_micros(Self::STEP_LENGTH_MICROS * Self::CATCH_UP_STEPS as u64);

    /// Constructs a new [`FrameClock`].
    ///
    /// This operation is independent of the system clock.
    pub fn new() -> Self {
        Self {
            last_absolute_time: None,
            render_dirty: true,
            accumulated_step_time: Duration::default(),
        }
    }

    /// Advance the clock using a source of absolute time.
    ///
    /// This cannot be meaningfully used in combination with
    /// [`FrameClock::request_frame()`].
    pub fn advance_to(&mut self, instant: Instant) {
        if let Some(last_absolute_time) = self.last_absolute_time {
            let delta = instant - last_absolute_time;
            self.accumulated_step_time += delta;
            self.cap_step_time();
        }
        self.last_absolute_time = Some(instant);
    }

    /// Reacts to a callback from the environment requesting drawing a frame ASAP if
    /// we're going to (i.e. `requestAnimationFrame` on the web). Drives the simulation
    /// clock based on this input (it will not advance if no requests are made).
    ///
    /// Returns whether a frame should actually be rendered now. The caller should also
    /// consult [`FrameClock::should_step()`] afterward to schedule game state steps.
    ///
    /// This cannot be meaningfully used in combination with [`FrameClock::advance_to()`].
    #[must_use]
    pub fn request_frame(&mut self, time_since_last_frame: Duration) -> bool {
        let result = self.should_draw();
        self.did_draw();

        self.accumulated_step_time += time_since_last_frame;
        self.cap_step_time();

        result
    }

    /// Indicates whether a new frame should be drawn, given the amount of time that this
    /// [`FrameClock`] has been informed has passed.
    ///
    /// When a frame *is* drawn, [`FrameClock::did_draw`]] must be called; otherwise, this
    /// will always return true.
    pub fn should_draw(&self) -> bool {
        self.render_dirty
    }

    /// Informs the [`FrameClock`] that a frame was just drawn.
    pub fn did_draw(&mut self) {
        self.render_dirty = false;
    }

    /// Indicates whether [`Universe::step`](crate::universe::Universe::step) should be performed,
    /// given the amount of time that this [`FrameClock`] has been informed has passed.
    ///
    /// When a step *is* performd, [`FrameClock::did_step`] must be called; otherwise, this
    /// will always return true.
    pub fn should_step(&self) -> bool {
        self.accumulated_step_time >= Self::STEP_LENGTH
    }

    /// Informs the [`FrameClock`] that a step was just performed.
    pub fn did_step(&mut self) {
        self.accumulated_step_time -= Self::STEP_LENGTH;
        self.render_dirty = true;
    }

    /// The timestep value that should be passed to
    /// [`Universe::step`](crate::universe::Universe::step)
    /// when stepping in response to [`FrameClock::should_step`] returning true.
    #[must_use] // avoid confusion with side-effecting methods
    pub fn tick(&self) -> Tick {
        Tick {
            delta_t: Self::STEP_LENGTH,
            paused: false,
        }
    }

    fn cap_step_time(&mut self) {
        if self.accumulated_step_time > Self::ACCUMULATOR_CAP {
            self.accumulated_step_time = Self::ACCUMULATOR_CAP;
        }
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Information to pass from the [`FrameClock`] or other timing mechanism to
/// the [`Universe`](crate::universe::Universe) and other game objects having `step` methods.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Tick {
    // TODO: Replace this with a rational-number-based system so that we can
    // (1) step in exact 60ths or other frame rate fractions
    // (2) have a standard subdivision for slower-than-every-frame events
    pub(crate) delta_t: Duration,

    paused: bool,
}

impl Tick {
    pub const fn arbitrary() -> Self {
        Self {
            delta_t: Duration::from_secs(1),
            paused: false,
        }
    }

    pub fn from_seconds(dt: f64) -> Self {
        Self {
            delta_t: Duration::from_micros((dt * 1e6) as u64),
            paused: false,
        }
    }

    /// Set the paused flag. See [`Tick::paused`] for more information.
    #[must_use]
    pub fn pause(self) -> Self {
        Self {
            paused: true,
            ..self
        }
    }

    /// Returns the "paused" state of this Tick. If true, then step operations should
    /// not perform any changes that reflect "in-game" time passing. They should still
    /// take care of the side effects of other mutations/transactions, particularly where
    /// not doing so might lead to a stale or inconsistent view.
    pub fn paused(&self) -> bool {
        self.paused
    }
}
