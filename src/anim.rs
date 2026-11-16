/// 指数平滑速度。~12.0 约 300 ms 达到目标的 95%。
const ANIM_SPEED: f32 = 12.0;

/// 小于此差值则直接贴到目标，避免无尽微抖动。
const ANIM_SNAP_EPSILON: f32 = 0.05;

/// 轻量逐值插值器：无分配、无特征约束、无异步。
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

    /// 按真实经过时间 `dt`（秒）推进；与帧率无关。
    /// `current` 有实质变化时返回 `true`。
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

/// 采样环形数值之间的短线性过渡。
///
/// 新样本从当前显示值改目标，不跳变。
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

    pub fn snap_to(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.start = value;
        self.elapsed = 0.0;
        self.running = false;
    }

    pub fn set_target(&mut self, target: f32) -> bool {
        if self.target == target {
            return self.running;
        }

        if self.current == target {
            self.snap_to(target);
            return false;
        }

        self.target = target;
        self.start = self.current;
        self.elapsed = 0.0;
        self.running = true;
        true
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

/// 布局过渡用的定长标量动画。
///
/// 不同于 `AnimatedValue` 的指数平滑，此动画必在有限时间内结束，
/// 避免窗口展开/收起末段“拖尾”感。插值刻意线性：该值常驱动像素对齐几何；
/// 视觉缓动应另施于透明度或变换。
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
    fn sampled_animation_settles_before_the_next_sample() {
        let mut value = SampledAnimatedValue::new(48.0, 0.22);
        assert!(value.set_target(32.0));
        assert!(value.tick_dt(0.11));
        assert_eq!(value.current, 40.0);
        assert!(!value.tick_dt(0.11));
        assert_eq!(value.current, 32.0);
        assert!(!value.tick_dt(0.78));
        assert!(!value.set_target(32.0));
    }

    #[test]
    fn sampled_animation_snap_cancels_an_in_progress_transition() {
        let mut value = SampledAnimatedValue::new(48.0, 0.22);
        value.set_target(32.0);
        assert!(value.tick_dt(0.05));
        value.snap_to(25.0);
        assert_eq!(value.current, 25.0);
        assert!(!value.tick_dt(0.1));
        assert!(!value.set_target(25.0));
        assert!(value.set_target(30.0));
        assert!(!value.tick_dt(0.22));
        assert_eq!(value.current, 30.0);
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
        // `tick_dt` 在动画完全停稳时恰好返回 `false`。
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
