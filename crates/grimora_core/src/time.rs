use std::time::{Duration, Instant};

/// Frame timing. `delta` is clamped so a debugger pause or window drag
/// never produces a huge simulation step.
///
/// Two clocks run side by side. `delta` and `elapsed` are real time and never
/// stop, which is what menus and HUD fades want. `world_delta` and
/// `world_elapsed` are the game world's time and stand still while paused
/// ([`Time::set_paused`]).
#[derive(Debug, Clone)]
pub struct Time {
    start: Instant,
    last: Instant,
    delta: f32,
    elapsed: f32,
    frame: u64,
    paused: bool,
    world_delta: f32,
    world_elapsed: f32,
    /// Real seconds spent paused, so `world_elapsed` equals `elapsed` if never paused.
    paused_for: f32,
    scale: f32,
}

impl Time {
    pub const MAX_DELTA: f32 = 0.1;

    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last: now,
            delta: 0.0,
            elapsed: 0.0,
            frame: 0,
            paused: false,
            world_delta: 0.0,
            world_elapsed: 0.0,
            paused_for: 0.0,
            scale: 1.0,
        }
    }

    /// Makes the world's clock run this many times as fast as the real one: slow
    /// motion, for looking at an animation.
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale.max(0.0);
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Advances the clock; called once per frame by the runner.
    pub fn tick(&mut self) {
        let now = Instant::now();
        let real = (now - self.last).as_secs_f32();
        self.last = now;
        self.elapsed = (now - self.start).as_secs_f32();
        self.advanced(real);
    }

    /// Advances the clock by exactly `seconds`, for runs without a real clock (tests).
    pub fn advance(&mut self, seconds: f32) {
        self.last += Duration::from_secs_f32(seconds.max(0.0));
        self.elapsed += seconds;
        self.advanced(seconds);
    }

    fn advanced(&mut self, real: f32) {
        self.delta = real.min(Self::MAX_DELTA);
        if self.paused {
            self.world_delta = 0.0;
            self.paused_for += real;
        } else {
            self.world_delta = self.delta * self.scale;
            // Slowed down, part of each real second counts as paused.
            self.paused_for += real * (1.0 - self.scale);
            self.world_elapsed = self.elapsed - self.paused_for;
        }
        self.frame += 1;
    }

    /// Stops or restarts the world's clock, starting with this frame.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if paused {
            self.world_delta = 0.0;
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Real seconds since the previous frame.
    pub fn delta(&self) -> f32 {
        self.delta
    }

    /// Real seconds since the engine started.
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    pub fn elapsed_duration(&self) -> Duration {
        self.last - self.start
    }

    /// Seconds of world time since the previous frame: 0 while paused.
    pub fn world_delta(&self) -> f32 {
        self.world_delta
    }

    /// Seconds the world has been running: `elapsed` minus the time spent paused.
    pub fn world_elapsed(&self) -> f32 {
        self.world_elapsed
    }

    /// Number of frames ticked so far.
    pub fn frame(&self) -> u64 {
        self.frame
    }
}

impl Default for Time {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_world_clock_stops_while_paused() {
        let mut time = Time::new();
        time.advance(0.5);
        assert_eq!((time.delta(), time.world_delta()), (Time::MAX_DELTA, Time::MAX_DELTA), "both clamped");
        assert_eq!((time.elapsed(), time.world_elapsed()), (0.5, 0.5));

        time.set_paused(true);
        assert_eq!(time.world_delta(), 0.0, "stops within the frame");
        time.advance(0.05);
        assert_eq!((time.delta(), time.world_delta()), (0.05, 0.0));
        assert_eq!(time.elapsed(), 0.55);
        assert_eq!(time.world_elapsed(), 0.5);

        time.set_paused(false);
        time.advance(0.05);
        assert_eq!((time.delta(), time.world_delta()), (0.05, 0.05));
        assert!((time.world_elapsed() - 0.55).abs() < 1e-5, "no jump on resume");
        assert_eq!(time.frame(), 3);
    }
}
