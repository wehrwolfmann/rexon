//! Часы ролика и темп рендера — чистая логика без GPU (тестируется синтетическими Instant).

use std::time::{Duration, Instant};

/// Один шаг часов.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    /// На сколько двигать ролик: реальное время с прошлого шага, но не больше `dt_cap`.
    pub dt: Duration,
    /// Реальный разрыв, если он больше `dt_cap` (SIGSTOP/SIGCONT надзора, сон, зависание хоста).
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
    /// `max_fps` > 0 — рендер не чаще этого; 0 — по готовности кадров ролика.
    pub fn new(max_fps: f64, dt_cap: Duration) -> Self {
        let period = (max_fps > 0.0 && max_fps.is_finite()).then(|| Duration::from_secs_f64(1.0 / max_fps));
        Self { period, dt_cap: dt_cap.max(Duration::from_millis(1)), last: None, next: None }
    }

    pub fn period(&self) -> Option<Duration> {
        self.period
    }

    /// Сдвинуть часы к `now`. Первый вызов даёт dt = 0.
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

    /// После паузы внутри процесса: время паузы в ролик не идёт вовсе, рендер — сразу.
    pub fn resume(&mut self, now: Instant) {
        self.last = Some(now);
        self.next = Some(now);
    }

    /// Пора ли рисовать. `movie_wants` — `Player::needs_render()` (используется без потолка fps).
    pub fn due(&self, now: Instant, movie_wants: bool) -> bool {
        match self.period {
            Some(_) => self.next.is_none_or(|n| now >= n),
            None => movie_wants,
        }
    }

    /// Отметить отрисованный кадр. Отставание больше периода не копится (кадры не догоняются).
    pub fn rendered(&mut self, now: Instant) {
        if let Some(p) = self.period {
            let n = self.next.unwrap_or(now) + p;
            self.next = Some(if n < now { now + p } else { n });
        }
    }

    /// Когда хосту звать `tick` снова. `movie_hint` — `Player::time_til_next_frame()`.
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
        // отстали на секунду: следующий кадр через период от «сейчас», а не пачка догоняющих
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
