/// Exponential smoothing speed.  ~12.0 ⇒ ~300 ms to reach 95 % of target.
const ANIM_SPEED: f32 = 12.0;

/// Below this delta the value snaps to target (avoids endless micro-ticks).
const ANIM_SNAP_EPSILON: f32 = 0.05;

/// Lightweight per-value interpolator — no allocations, no traits, no async.
#[derive(Clone, Debug)]
pub struct AnimatedValue {
    pub current: f32,
    pub target: f32,
}

impl AnimatedValue {
    pub const fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
        }
    }

    /// Advance by real elapsed time `dt` (seconds); frame-rate independent.
    /// Returns `true` when `current` moved meaningfully.
    #[inline]
    pub fn tick_dt(&mut self, dt: f32) -> bool {
        let diff = self.target - self.current;
        if diff.abs() < ANIM_SNAP_EPSILON {
            let moved = self.current != self.target;
            self.current = self.target;
            return moved;
        }
        self.current += diff * (1.0 - (-ANIM_SPEED * dt).exp());
        true
    }
}

/// Linear interpolation between periodically sampled values.
///
/// A new sample retargets from the currently displayed value, so consecutive
/// samples form one continuous motion instead of independent ease-out bursts.
#[derive(Clone, Debug)]
pub struct SampledAnimatedValue {
    pub current: f32,
    target: f32,
    start: f32,
    elapsed: f32,
    duration: f32,
    running: bool,
}

impl SampledAnimatedValue {
    pub const fn new(value: f32, duration: f32) -> Self {
        Self {
            current: value,
            target: value,
            start: value,
            elapsed: 0.0,
            duration,
            running: false,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        if self.target == target {
            return;
        }

        self.target = target;
        if self.current == target {
            self.running = false;
            return;
        }

        self.start = self.current;
        self.elapsed = 0.0;
        self.running = true;
    }

    pub fn tick_dt(&mut self, dt: f32) -> bool {
        if !self.running {
            return false;
        }

        self.elapsed += dt;
        let t = (self.elapsed / self.duration.max(f32::EPSILON)).clamp(0.0, 1.0);
        self.current = self.start + (self.target - self.start) * t;
        if t >= 1.0 {
            self.current = self.target;
            self.running = false;
        }
        self.running
    }
}

/// Fixed-duration scalar animation for layout transitions.
///
/// Unlike `AnimatedValue`'s exponential smoothing, this always completes in a
/// bounded time and avoids the perceptual "slow tail" that makes window
/// expand/collapse feel like it stalls near the end. The interpolation is
/// intentionally linear because this value often drives pixel-rounded geometry;
/// visual easing should be applied separately to opacity or transforms.
#[derive(Clone, Debug)]
pub struct TimedAnimatedValue {
    pub current: f32,
    pub target: f32,
    start: f32,
    elapsed: f32,
    duration: f32,
    running: bool,
}

impl TimedAnimatedValue {
    pub const fn new(value: f32, duration: f32) -> Self {
        Self {
            current: value,
            target: value,
            start: value,
            elapsed: 0.0,
            duration,
            running: false,
        }
    }

    pub fn snap_to(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.start = value;
        self.elapsed = 0.0;
        self.running = false;
    }

    pub fn set_target(&mut self, target: f32) {
        if (self.current - target).abs() < ANIM_SNAP_EPSILON {
            self.snap_to(target);
            return;
        }

        self.start = self.current;
        self.target = target;
        self.elapsed = 0.0;
        self.running = true;
    }

    pub fn tick_dt(&mut self, dt: f32) -> bool {
        if !self.running {
            return false;
        }

        self.elapsed += dt;
        let t = (self.elapsed / self.duration.max(f32::EPSILON)).clamp(0.0, 1.0);
        self.current = self.start + (self.target - self.start) * t;

        if t >= 1.0 {
            self.snap_to(self.target);
            return false;
        }

        true
    }
}

#[inline]
pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampled_animation_spans_the_sampling_interval() {
        let mut value = SampledAnimatedValue::new(0.0, 1.0);
        value.set_target(100.0);
        assert!(value.tick_dt(0.5));
        assert_eq!(value.current, 50.0);
        assert!(!value.tick_dt(0.5));
        assert_eq!(value.current, 100.0);
    }

    #[test]
    fn sampled_animation_retargets_without_jumping() {
        let mut value = SampledAnimatedValue::new(0.0, 1.0);
        value.set_target(100.0);
        assert!(value.tick_dt(0.5));
        value.set_target(70.0);
        assert_eq!(value.current, 50.0);
        assert!(value.tick_dt(0.5));
        assert_eq!(value.current, 60.0);
    }

    #[test]
    fn sampled_animation_ignores_an_unchanged_target() {
        let mut value = SampledAnimatedValue::new(0.0, 1.0);
        value.set_target(100.0);
        assert!(value.tick_dt(0.25));
        value.set_target(100.0);
        assert!(value.tick_dt(0.25));
        assert_eq!(value.current, 50.0);
    }

    #[test]
    fn timed_animation_finishes_at_target() {
        let mut value = TimedAnimatedValue::new(0.0, 0.2);
        value.set_target(1.0);
        assert!(value.tick_dt(0.1));
        assert_eq!(value.current, 0.5);
        // `tick_dt` returns `false` exactly when the animation has settled.
        assert!(!value.tick_dt(0.1));
        assert_eq!(value.current, 1.0);
        assert_eq!(value.current, value.target);
    }

    #[test]
    fn timed_animation_retargets_from_current_value() {
        let mut value = TimedAnimatedValue::new(0.0, 0.2);
        value.set_target(1.0);
        assert!(value.tick_dt(0.1));
        let mid = value.current;
        value.set_target(0.0);
        assert_eq!(value.start, mid);
        assert!(value.tick_dt(0.1));
        assert!(value.current < mid);
    }
}
