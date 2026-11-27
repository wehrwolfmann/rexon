//! Movie clock and render pacing — pure logic without the GPU (tested with synthetic Instants).

use std::time::{Duration, Instant};

/// One clock step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    /// How far to advance the movie: real time since the last step, but no more than `dt_cap`.
    pub dt: Duration,
    /// The real gap, if it is larger than `dt_cap` (supervision SIGSTOP/SIGCONT, sleep, a host hang).
    pub gap: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct Pacer {
    period: Option<Duration>,
    dt_cap: Duration,
    last: Option<Instant>,
    next: Option<Instant>,
}

impl Pacer {
    /// `max_fps` > 0 — render no more often than this; 0 — whenever the movie frames are ready.
    pub fn new(max_fps: f64, dt_cap: Duration) -> Self {
        let period = (max_fps > 0.0 && max_fps.is_finite()).then(|| Duration::from_secs_f64(1.0 / max_fps));
        Self { period, dt_cap: dt_cap.max(Duration::from_millis(1)), last: None, next: None }
    }

    pub fn period(&self) -> Option<Duration> {
        self.period
    }

    /// Move the clock to `now`. The first call gives dt = 0.
    pub fn advance(&mut self, now: Instant) -> Step {
        let step = match self.last {
            None => Step { dt: Duration::ZERO, gap: None },
            Some(last) => {
                let wall = now.saturating_duration_since(last);
                Step { dt: wall.min(self.dt_cap), gap: (wall > self.dt_cap).then_some(wall) }
            }
        };
        self.last = Some(now);
        step
    }

    /// After a pause inside the process: the paused time does not go into the movie at all, render — right away.
    pub fn resume(&mut self, now: Instant) {
        self.last = Some(now);
        self.next = Some(now);
    }

    /// Whether it is time to draw. `movie_wants` — `Player::needs_render()` (used without an fps cap).
    pub fn due(&self, now: Instant, movie_wants: bool) -> bool {
        match self.period {
            Some(_) => self.next.is_none_or(|n| now >= n),
            None => movie_wants,
        }
    }

    /// Mark a drawn frame. A lag longer than a period does not accumulate (frames are not caught up).
    pub fn rendered(&mut self, now: Instant) {
        if let Some(p) = self.period {
            let n = self.next.unwrap_or(now) + p;
            self.next = Some(if n < now { now + p } else { n });
        }
    }

    /// When the host should call `tick` again. `movie_hint` — `Player::time_til_next_frame()`.
    pub fn next_wake(&self, now: Instant, movie_hint: Duration) -> Instant {
        match (self.period, self.next) {
            (Some(_), Some(n)) => n.max(now),
            (Some(_), None) => now,
            (None, _) => now + movie_hint,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_step_is_zero_then_real_time() {
        let t0 = Instant::now();
        let mut p = Pacer::new(60.0, Duration::from_millis(100));
        assert_eq!(p.advance(t0), Step { dt: Duration::ZERO, gap: None });
        let s = p.advance(t0 + Duration::from_millis(16));
        assert_eq!(s.dt, Duration::from_millis(16));
        assert_eq!(s.gap, None);
    }

    #[test]
    fn long_stop_is_capped_and_reported() {
        let t0 = Instant::now();
        let mut p = Pacer::new(60.0, Duration::from_millis(100));
        p.advance(t0);
        let s = p.advance(t0 + Duration::from_millis(4355));
        assert_eq!(s.dt, Duration::from_millis(100), "SIGSTOP на 4,4 с двигает ролик только на dt_cap");
        assert_eq!(s.gap, Some(Duration::from_millis(4355)));
        let s = p.advance(t0 + Duration::from_millis(4371));
        assert_eq!((s.dt, s.gap), (Duration::from_millis(16), None), "после разрыва — обычный шаг");
    }

    #[test]
    fn resume_after_in_process_pause_adds_no_time() {
        let t0 = Instant::now();
        let mut p = Pacer::new(0.0, Duration::from_millis(100));
        p.advance(t0);
        p.resume(t0 + Duration::from_secs(30));
        assert_eq!(p.advance(t0 + Duration::from_secs(30)).dt, Duration::ZERO);
    }

    #[test]
    fn fps_cap_schedules_and_does_not_catch_up() {
        let t0 = Instant::now();
        let mut p = Pacer::new(50.0, Duration::from_millis(100));
        assert!(p.due(t0, false), "первый кадр — сразу");
        p.rendered(t0);
        assert!(!p.due(t0 + Duration::from_millis(10), true));
        assert!(p.due(t0 + Duration::from_millis(20), false));
        assert_eq!(p.next_wake(t0 + Duration::from_millis(5), Duration::ZERO), t0 + Duration::from_millis(20));
        // fell behind by a second: the next frame comes one period from "now", not a burst of catch-up frames
        let late = t0 + Duration::from_secs(1);
        p.rendered(late);
        assert_eq!(p.next_wake(late, Duration::ZERO), late + Duration::from_millis(20));
    }

    #[test]
    fn no_cap_follows_movie() {
        let t0 = Instant::now();
        let p = Pacer::new(0.0, Duration::from_millis(100));
        assert!(!p.due(t0, false));
        assert!(p.due(t0, true));
        assert_eq!(p.next_wake(t0, Duration::from_millis(41)), t0 + Duration::from_millis(41));
        assert_eq!(Pacer::new(f64::NAN, Duration::ZERO).period(), None);
    }
}
