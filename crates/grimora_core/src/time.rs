use std::time::{Duration, Instant};

/// Frame timing. `delta` is clamped so a debugger pause or window drag
/// never produces a huge simulation step.
#[derive(Debug, Clone)]
pub struct Time {
    start: Instant,
    last: Instant,
    delta: f32,
    elapsed: f32,
    frame: u64,
}

impl Time {
    pub const MAX_DELTA: f32 = 0.1;

    pub fn new() -> Self {
        let now = Instant::now();
        Self { start: now, last: now, delta: 0.0, elapsed: 0.0, frame: 0 }
    }

    /// Advances the clock; called once per frame by the runner.
    pub fn tick(&mut self) {
        let now = Instant::now();
        self.delta = (now - self.last).as_secs_f32().min(Self::MAX_DELTA);
        self.last = now;
        self.elapsed = (now - self.start).as_secs_f32();
        self.frame += 1;
    }

    /// Seconds since the previous frame.
    pub fn delta(&self) -> f32 {
        self.delta
    }

    /// Seconds since the engine started.
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    pub fn elapsed_duration(&self) -> Duration {
        self.last - self.start
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
