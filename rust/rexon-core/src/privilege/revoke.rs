//! Revoking the session privilege (item B6 of the "one password per launch"
//! plan).
//!
//! The owner's decision of 28.09: the session privilege expires when
//! 1. the Rexon window is closed, including minimized to the tray;
//! 2. there have been no privileged actions for 15 minutes;
//! 3. the screen is locked;
//! 4. the machine went to sleep.
//!
//! An operation in progress is still carried through to the end, new ones are
//! rejected; the next request asks for the password again (starts a new
//! helper).
//!
//! Structure:
//! * [`Revoker`] — a pure state machine: no threads, no clocks, no system. Time
//!   is passed to it as the `now` argument (monotonic, from any origin), events
//!   as [`Event`]. That way it is fully testable with fake clocks.
//! * [`Clock`] and [`SessionWatch`] — the seams to the outside. The live ones
//!   are [`MonotonicClock`] and [`LogindWatch`].
//! * [`Revoker::poll`] — one step: take the watcher's events, check for
//!   inactivity. The loop with a pause (once per [`POLL_INTERVAL`]) is run by
//!   the owner — the privilege session thread (B4); there are no threads here.
//!
//! How lock and sleep are caught. The project has no D-Bus crate, so we do not
//! listen to logind signals (`Lock`, `PrepareForSleep`) but poll instead:
//! * lock and the session going away — `loginctl show-session <id> -p Active
//!   -p LockedHint` (the same `loginctl` that `login_rescue.rs` parses, through
//!   the shared [`SystemIo::run`]);
//! * sleep — by the divergence of `CLOCK_BOOTTIME` and `CLOCK_MONOTONIC`: the
//!   former runs during sleep, the latter stands still, and the difference grows
//!   by exactly the time slept. The wall clock is no good — NTP and the person
//!   move it, and that would be a false "sleep". Polling would not catch the
//!   `PrepareForSleep` signal at all; the lock screen does not come up after
//!   sleep on every desktop, so we do not rely on `LockedHint` alone.

use std::time::Duration;

use crate::i18n::{n_, tr};
use crate::system_exec::SystemIo;

/// How much inactivity expires the privilege (the owner's decision of 28.09).
pub const IDLE_LIMIT: Duration = Duration::from_secs(15 * 60);

/// How often the owner of [`Revoker`] should call [`Revoker::poll`].
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// How much the BOOTTIME − MONOTONIC difference must grow between polls to count
/// as sleep. By itself it changes only during sleep; the margin covers the fact
/// that the two clocks are not read in a single call.
pub const SLEEP_JUMP: Duration = Duration::from_secs(1);

/// How many polls in a row `loginctl` may stay silent (timeout, failed to
/// start) before the session is considered lost. A clear refusal ("No
/// session") expires the privilege immediately.
pub const SILENT_POLLS_LIMIT: u32 = 2;

/// Path to `loginctl`. The helper runs as root — we take an absolute path rather
/// than a `PATH` lookup.
pub const LOGINCTL: &str = "/usr/bin/loginctl";

// ============================================================================
// Events and reasons
// ============================================================================

/// What happened outside and expires the privilege.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The Rexon window was closed (a GUI event, B13; the helper also treats a
    /// broken socket as closing).
    WindowClosed,
    /// The window was minimized to the tray (a GUI event, B13).
    WindowToTray,
    /// The screen is locked (`LockedHint` became `yes`).
    ScreenLocked,
    /// The machine was asleep.
    Slept,
    /// The window's session is no longer active: the user or VT was switched.
    SessionInactive,
    /// logind does not know the window's session or does not answer about it.
    SessionLost,
    /// The person pressed "Revoke now" themselves.
    RevokeNow,
}

/// Why the privilege expired — for the text shown to the person and for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    WindowClosed,
    WindowToTray,
    Idle,
    ScreenLocked,
    Slept,
    SessionInactive,
    SessionLost,
    RevokedByUser,
}

impl From<Event> for Reason {
    fn from(e: Event) -> Self {
        match e {
            Event::WindowClosed => Reason::WindowClosed,
            Event::WindowToTray => Reason::WindowToTray,
            Event::ScreenLocked => Reason::ScreenLocked,
            Event::Slept => Reason::Slept,
            Event::SessionInactive => Reason::SessionInactive,
            Event::SessionLost => Reason::SessionLost,
            Event::RevokeNow => Reason::RevokedByUser,
        }
    }
}

impl Reason {
    /// Short name for the log (not translated).
    pub fn code(self) -> &'static str {
        match self {
            Reason::WindowClosed => "window-closed",
            Reason::WindowToTray => "window-to-tray",
            Reason::Idle => "idle",
            Reason::ScreenLocked => "screen-locked",
            Reason::Slept => "slept",
            Reason::SessionInactive => "session-inactive",
            Reason::SessionLost => "session-lost",
            Reason::RevokedByUser => "revoked-by-user",
        }
    }

    /// Reason by its short name ([`Reason::code`]); `None` — unknown.
    pub fn from_code(code: &str) -> Option<Self> {
        [
            Reason::WindowClosed,
            Reason::WindowToTray,
            Reason::Idle,
            Reason::ScreenLocked,
            Reason::Slept,
            Reason::SessionInactive,
            Reason::SessionLost,
            Reason::RevokedByUser,
        ]
        .into_iter()
        .find(|r| r.code() == code)
    }

    /// Text for the person: why we are asking for the password again (B13, without
    /// technical words; translation — `po/`).
    pub fn message(self) -> String {
        let msgid = match self {
            // TRANSLATORS: Permission to change the system lasts while the Rexon window is open.
            Reason::WindowClosed => {
                n_("Permission to change the system ended because the Rexon window was closed. Rexon will ask for the password again.")
            }
            Reason::WindowToTray => {
                n_("Permission to change the system ended because Rexon was minimized to the tray. Rexon will ask for the password again.")
            }
            Reason::Idle => {
                n_("Permission to change the system ended after 15 minutes without changes. Rexon will ask for the password again.")
            }
            Reason::ScreenLocked => {
                n_("Permission to change the system ended because the screen was locked. Rexon will ask for the password again.")
            }
            Reason::Slept => {
                n_("Permission to change the system ended because the computer went to sleep. Rexon will ask for the password again.")
            }
            Reason::SessionInactive => {
                n_("Permission to change the system ended because you switched to another user. Rexon will ask for the password again.")
            }
            Reason::SessionLost => {
                n_("Permission to change the system ended because Rexon couldn't check who is signed in. Rexon will ask for the password again.")
            }
            Reason::RevokedByUser => {
                n_("You turned off Rexon's permission to change the system. Rexon will ask for the password again.")
            }
        };
        tr(msgid)
    }
}

// ============================================================================
// State machine
// ============================================================================

/// Monotonic clock: `now` is time from an arbitrary origin that never goes back.
pub trait Clock {
    fn now(&self) -> Duration;
}

/// Session watcher: a single poll returns what happened since the previous poll.
pub trait SessionWatch {
    fn poll(&mut self) -> Vec<Event>;
}

/// Decides whether the session privilege is still alive.
///
/// Created when the password has just been accepted. Inactivity is counted from
/// the end of the last operation; while an operation is running there is no
/// inactivity (a long build does not expire the privilege in the middle of it —
/// and an expired privilege would not interrupt it anyway).
#[derive(Debug, Clone)]
pub struct Revoker {
    idle_limit: Duration,
    revoked: Option<Reason>,
    in_flight: u32,
    last_action: Duration,
}

impl Revoker {
    /// The privilege has just been granted (the password accepted) at moment `now`.
    pub fn granted(now: Duration) -> Self {
        Self::with_idle_limit(now, IDLE_LIMIT)
    }

    /// The same with a different inactivity limit (for tests and debugging).
    pub fn with_idle_limit(now: Duration, idle_limit: Duration) -> Self {
        Revoker { idle_limit, revoked: None, in_flight: 0, last_action: now }
    }

    /// The privilege was granted again (the password was asked once more). Running
    /// operations remain counted.
    pub fn regrant(&mut self, now: Duration) {
        self.revoked = None;
        self.last_action = now;
    }

    /// An external event. It expires a live privilege; an expired one keeps its
    /// first reason — that is what explains to the person what happened.
    pub fn on_event(&mut self, event: Event, now: Duration) {
        self.tick(now);
        self.revoke(event.into());
    }

    /// Check for inactivity at moment `now`.
    pub fn tick(&mut self, now: Duration) {
        if self.revoked.is_none() && self.in_flight == 0 && now.saturating_sub(self.last_action) >= self.idle_limit {
            self.revoke(Reason::Idle);
        }
    }

    /// One step of the owner: the watcher's events, then inactivity.
    pub fn poll(&mut self, watch: &mut impl SessionWatch, clock: &impl Clock) {
        for e in watch.poll() {
            self.on_event(e, clock.now());
        }
        self.tick(clock.now());
    }

    /// Whether a new operation may start. Marks nothing — that is what
    /// [`Revoker::op_started`] is for.
    pub fn allow_new_op(&mut self, now: Duration) -> Result<(), Reason> {
        self.tick(now);
        match self.revoked {
            Some(r) => Err(r),
            None => Ok(()),
        }
    }

    /// An operation is starting. Checks the privilege itself, so
    /// [`Revoker::allow_new_op`] cannot be bypassed.
    pub fn op_started(&mut self, now: Duration) -> Result<(), Reason> {
        self.allow_new_op(now)?;
        self.in_flight += 1;
        self.last_action = now;
        Ok(())
    }

    /// An operation has finished (successfully or not) — inactivity is counted from here.
    pub fn op_finished(&mut self, now: Duration) {
        self.in_flight = self.in_flight.saturating_sub(1);
        self.last_action = now;
    }

    /// Reason for revocation; `None` — the privilege is alive.
    pub fn revoked(&self) -> Option<Reason> {
        self.revoked
    }

    /// How many operations are running.
    pub fn in_flight(&self) -> u32 {
        self.in_flight
    }

    /// Time for the helper to exit: the privilege has expired and nothing is left to finish.
    pub fn may_exit(&self) -> bool {
        self.revoked.is_some() && self.in_flight == 0
    }

    /// When the privilege will expire from inactivity if nothing happens; `None` —
    /// already expired or an operation is running.
    pub fn idle_deadline(&self) -> Option<Duration> {
        (self.revoked.is_none() && self.in_flight == 0).then(|| self.last_action + self.idle_limit)
    }

    fn revoke(&mut self, reason: Reason) {
        if self.revoked.is_none() {
            self.revoked = Some(reason);
        }
    }
}

// ============================================================================
// Live clock and watcher
// ============================================================================

fn clock_read(id: libc::clockid_t) -> Duration {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes only into the structure passed in.
    let rc = unsafe { libc::clock_gettime(id, &mut ts) };
    if rc != 0 {
        return Duration::ZERO;
    }
    Duration::new(ts.tv_sec.max(0) as u64, ts.tv_nsec.clamp(0, 999_999_999) as u32)
}

/// `CLOCK_MONOTONIC`: stands still during sleep, so inactivity does not
/// accumulate while asleep (sleep expires the privilege itself, as a separate
/// event).
#[derive(Debug, Clone, Copy, Default)]
pub struct MonotonicClock;

impl Clock for MonotonicClock {
    fn now(&self) -> Duration {
        clock_read(libc::CLOCK_MONOTONIC)
    }
}

/// How long the machine has slept in total since boot.
pub trait SuspendGauge {
    fn slept_total(&self) -> Duration;
}

/// `CLOCK_BOOTTIME − CLOCK_MONOTONIC`.
#[derive(Debug, Clone, Copy, Default)]
pub struct BootMinusMonotonic;

impl SuspendGauge for BootMinusMonotonic {
    fn slept_total(&self) -> Duration {
        let mono = clock_read(libc::CLOCK_MONOTONIC);
        let boot = clock_read(libc::CLOCK_BOOTTIME);
        boot.saturating_sub(mono)
    }
}

/// logind's answer about the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAnswer {
    /// The session exists.
    Seen { active: bool, locked: bool },
    /// logind clearly said "no such session" (or the answer lacks the needed fields).
    Gone,
    /// There was no answer: timeout, the program failed to start.
    Silent,
}

/// Where the session state comes from — a seam for tests.
pub trait SessionQuery {
    fn query(&self, session: &str) -> SessionAnswer;
}

/// A live request via `loginctl show-session` (read-only).
pub struct Loginctl<I: SystemIo>(pub I);

impl<I: SystemIo> SessionQuery for Loginctl<I> {
    fn query(&self, session: &str) -> SessionAnswer {
        let argv: Vec<String> =
            [LOGINCTL, "show-session", session, "-p", "Active", "-p", "LockedHint"].iter().map(|s| s.to_string()).collect();
        let out = self.0.run(&argv, Some(5), true);
        match out.rc() {
            None => SessionAnswer::Silent,
            Some(0) => parse_active_locked(&out.stdout),
            Some(_) => SessionAnswer::Gone,
        }
    }
}

/// Parsing `loginctl show-session -p Active -p LockedHint` (line order — as
/// logind returns it). Without `Active` the answer is unusable, we consider the
/// session lost. Without `LockedHint` (old systemd) — "not locked": sleep and
/// inactivity will expire the privilege anyway.
pub fn parse_active_locked(text: &str) -> SessionAnswer {
    let mut active = None;
    let mut locked = false;
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim() {
            "Active" => active = Some(v.trim() == "yes"),
            "LockedHint" => locked = v.trim() == "yes",
            _ => {}
        }
    }
    match active {
        Some(active) => SessionAnswer::Seen { active, locked },
        None => SessionAnswer::Gone,
    }
}

/// A logind session id is fit for argv: non-empty, short, only Latin letters
/// and digits (`2`, `c1`). Otherwise it could pose as a `loginctl` option.
pub fn valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Watcher of the window's session: logind + a sleep counter.
///
/// Lock is caught on the edge: `LockedHint` "no → yes". The first poll is the
/// reference point: the password has just been entered, so the screen is not
/// locked, and `LockedHint=yes` there is a stuck flag of the desktop; expiring
/// every new privilege because of it would be wrong. An inactive session, on
/// the contrary, expires it immediately, on the first poll as well: the
/// password was not asked in it.
pub struct LogindWatch<Q: SessionQuery, G: SuspendGauge> {
    query: Q,
    gauge: G,
    session: String,
    was_locked: Option<bool>,
    slept_before: Duration,
    silent: u32,
}

impl<Q: SessionQuery, G: SuspendGauge> LogindWatch<Q, G> {
    /// `None` — the session id is unusable (see [`valid_session_id`]).
    pub fn new(query: Q, gauge: G, session: &str) -> Option<Self> {
        if !valid_session_id(session) {
            return None;
        }
        let slept_before = gauge.slept_total();
        Some(LogindWatch { query, gauge, session: session.to_string(), was_locked: None, slept_before, silent: 0 })
    }
}

impl<I: SystemIo, G: SuspendGauge> LogindWatch<Loginctl<I>, G> {
    /// Live watcher on `loginctl`.
    pub fn live(io: I, gauge: G, session: &str) -> Option<Self> {
        Self::new(Loginctl(io), gauge, session)
    }
}

impl<Q: SessionQuery, G: SuspendGauge> SessionWatch for LogindWatch<Q, G> {
    fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let slept = self.gauge.slept_total();
        if slept.saturating_sub(self.slept_before) >= SLEEP_JUMP {
            events.push(Event::Slept);
        }
        self.slept_before = slept;
        match self.query.query(&self.session) {
            SessionAnswer::Seen { active, locked } => {
                self.silent = 0;
                if !active {
                    events.push(Event::SessionInactive);
                }
                if locked && self.was_locked == Some(false) {
                    events.push(Event::ScreenLocked);
                }
                self.was_locked = Some(locked);
            }
            SessionAnswer::Gone => events.push(Event::SessionLost),
            SessionAnswer::Silent => {
                self.silent += 1;
                if self.silent >= SILENT_POLLS_LIMIT {
                    events.push(Event::SessionLost);
                }
            }
        }
        events
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

    fn min(n: u64) -> Duration {
        Duration::from_secs(n * 60)
    }

    struct FakeClock(Cell<Duration>);
    impl FakeClock {
        fn at(t: Duration) -> Self {
            FakeClock(Cell::new(t))
        }
        fn advance(&self, d: Duration) {
            self.0.set(self.0.get() + d);
        }
    }
    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            self.0.get()
        }
    }

    #[derive(Default)]
    struct FakeWatch(VecDeque<Vec<Event>>);
    impl SessionWatch for FakeWatch {
        fn poll(&mut self) -> Vec<Event> {
            self.0.pop_front().unwrap_or_default()
        }
    }

    // ── inactivity ──

    #[test]
    fn idle_15_minutes_revokes() {
        let c = FakeClock::at(min(100));
        let mut r = Revoker::granted(c.now());
        c.advance(min(15) - Duration::from_secs(1));
        assert_eq!(r.allow_new_op(c.now()), Ok(()));
        c.advance(Duration::from_secs(1));
        assert_eq!(r.allow_new_op(c.now()), Err(Reason::Idle));
        assert!(r.may_exit());
    }

    #[test]
    fn action_resets_idle_timer() {
        let c = FakeClock::at(Duration::ZERO);
        let mut r = Revoker::granted(c.now());
        c.advance(min(14));
        r.op_started(c.now()).unwrap();
        c.advance(min(1));
        r.op_finished(c.now());
        assert_eq!(r.idle_deadline(), Some(min(15) + IDLE_LIMIT));
        c.advance(min(14));
        assert_eq!(r.allow_new_op(c.now()), Ok(()));
        c.advance(min(1));
        assert_eq!(r.allow_new_op(c.now()), Err(Reason::Idle));
    }

    #[test]
    fn long_operation_is_not_idle() {
        let c = FakeClock::at(Duration::ZERO);
        let mut r = Revoker::granted(c.now());
        r.op_started(c.now()).unwrap();
        assert_eq!(r.idle_deadline(), None);
        c.advance(min(40));
        r.tick(c.now());
        assert_eq!(r.revoked(), None);
        r.op_finished(c.now());
        c.advance(min(5));
        assert_eq!(r.allow_new_op(c.now()), Ok(()));
    }

    #[test]
    fn poll_checks_idle_without_events() {
        let c = FakeClock::at(Duration::ZERO);
        let mut r = Revoker::granted(c.now());
        let mut w = FakeWatch::default();
        c.advance(IDLE_LIMIT);
        r.poll(&mut w, &c);
        assert_eq!(r.revoked(), Some(Reason::Idle));
    }

    // ── events in the middle of an operation ──

    fn mid_operation(event: Event, reason: Reason) {
        let c = FakeClock::at(Duration::ZERO);
        let mut r = Revoker::granted(c.now());
        r.op_started(c.now()).unwrap();
        c.advance(Duration::from_secs(30));
        let mut w = FakeWatch(VecDeque::from([vec![event]]));
        r.poll(&mut w, &c);
        // the running one is carried through: the helper does not leave while it is counted
        assert_eq!(r.revoked(), Some(reason));
        assert_eq!(r.in_flight(), 1);
        assert!(!r.may_exit());
        // new ones are rejected
        assert_eq!(r.allow_new_op(c.now()), Err(reason));
        assert_eq!(r.op_started(c.now()), Err(reason));
        assert_eq!(r.in_flight(), 1);
        c.advance(min(3));
        r.op_finished(c.now());
        assert!(r.may_exit());
        assert_eq!(r.allow_new_op(c.now()), Err(reason));
    }

    #[test]
    fn lock_mid_operation() {
        mid_operation(Event::ScreenLocked, Reason::ScreenLocked);
    }

    #[test]
    fn sleep_mid_operation() {
        mid_operation(Event::Slept, Reason::Slept);
    }

    #[test]
    fn window_closed_mid_operation() {
        mid_operation(Event::WindowClosed, Reason::WindowClosed);
    }

    #[test]
    fn tray_mid_operation() {
        mid_operation(Event::WindowToTray, Reason::WindowToTray);
    }

    #[test]
    fn other_events_revoke() {
        for (e, why) in [
            (Event::SessionInactive, Reason::SessionInactive),
            (Event::SessionLost, Reason::SessionLost),
            (Event::RevokeNow, Reason::RevokedByUser),
        ] {
            let mut r = Revoker::granted(Duration::ZERO);
            r.on_event(e, Duration::from_secs(1));
            assert_eq!(r.allow_new_op(Duration::from_secs(2)), Err(why));
        }
    }

    #[test]
    fn first_reason_is_kept() {
        let mut r = Revoker::granted(Duration::ZERO);
        r.on_event(Event::ScreenLocked, Duration::from_secs(1));
        r.on_event(Event::Slept, Duration::from_secs(2));
        r.on_event(Event::WindowClosed, Duration::from_secs(3));
        assert_eq!(r.revoked(), Some(Reason::ScreenLocked));
    }

    #[test]
    fn idle_before_event_wins() {
        // fell asleep after 20 minutes of inactivity: the privilege has already expired from inactivity
        let mut r = Revoker::granted(Duration::ZERO);
        r.on_event(Event::Slept, min(20));
        assert_eq!(r.revoked(), Some(Reason::Idle));
    }

    // ── raising again ──

    #[test]
    fn regrant_after_revoke() {
        let c = FakeClock::at(Duration::ZERO);
        let mut r = Revoker::granted(c.now());
        c.advance(min(16));
        assert_eq!(r.allow_new_op(c.now()), Err(Reason::Idle));
        r.regrant(c.now());
        assert_eq!(r.revoked(), None);
        assert_eq!(r.op_started(c.now()), Ok(()));
        r.op_finished(c.now());
        c.advance(min(14));
        assert_eq!(r.allow_new_op(c.now()), Ok(()));
        r.on_event(Event::ScreenLocked, c.now());
        assert_eq!(r.allow_new_op(c.now()), Err(Reason::ScreenLocked));
        r.regrant(c.now());
        assert_eq!(r.allow_new_op(c.now()), Ok(()));
    }

    #[test]
    fn regrant_keeps_running_operation() {
        let mut r = Revoker::granted(Duration::ZERO);
        r.op_started(Duration::ZERO).unwrap();
        r.on_event(Event::WindowToTray, Duration::from_secs(1));
        r.regrant(Duration::from_secs(2));
        assert_eq!(r.in_flight(), 1);
        r.op_finished(Duration::from_secs(3));
        assert_eq!(r.in_flight(), 0);
        assert!(!r.may_exit());
    }

    #[test]
    fn reasons_have_texts_and_codes() {
        for r in [
            Reason::WindowClosed,
            Reason::WindowToTray,
            Reason::Idle,
            Reason::ScreenLocked,
            Reason::Slept,
            Reason::SessionInactive,
            Reason::SessionLost,
            Reason::RevokedByUser,
        ] {
            assert!(!r.message().is_empty());
            assert!(!r.code().is_empty());
        }
    }

    // ── logind watcher ──

    #[derive(Clone, Default)]
    struct Script(Rc<RefCell<VecDeque<SessionAnswer>>>, Rc<RefCell<Vec<String>>>);
    impl SessionQuery for Script {
        fn query(&self, session: &str) -> SessionAnswer {
            self.1.borrow_mut().push(session.to_string());
            self.0.borrow_mut().pop_front().unwrap_or(SessionAnswer::Silent)
        }
    }

    #[derive(Clone, Default)]
    struct FakeGauge(Rc<Cell<Duration>>);
    impl SuspendGauge for FakeGauge {
        fn slept_total(&self) -> Duration {
            self.0.get()
        }
    }

    fn seen(active: bool, locked: bool) -> SessionAnswer {
        SessionAnswer::Seen { active, locked }
    }

    fn watch(answers: &[SessionAnswer]) -> (LogindWatch<Script, FakeGauge>, Script, FakeGauge) {
        let q = Script::default();
        q.0.borrow_mut().extend(answers.iter().copied());
        let g = FakeGauge::default();
        g.0.set(Duration::from_secs(7)); // sleep before the helper started does not count
        (LogindWatch::new(q.clone(), g.clone(), "3").unwrap(), q, g)
    }

    #[test]
    fn parse_loginctl_output() {
        assert_eq!(parse_active_locked("Active=yes\nState=active\nLockedHint=no\n"), seen(true, false));
        assert_eq!(parse_active_locked("LockedHint=yes\nActive=yes\n"), seen(true, true));
        assert_eq!(parse_active_locked("Active=no\n"), seen(false, false));
        assert_eq!(parse_active_locked(""), SessionAnswer::Gone);
    }

    #[test]
    fn session_id_validation() {
        assert!(valid_session_id("2"));
        assert!(valid_session_id("c1"));
        assert!(!valid_session_id(""));
        assert!(!valid_session_id("-p"));
        assert!(!valid_session_id("3 4"));
        assert!(!valid_session_id(&"1".repeat(33)));
        assert!(LogindWatch::new(Script::default(), FakeGauge::default(), "--help").is_none());
    }

    #[test]
    fn lock_is_caught_on_edge() {
        let (mut w, q, _) = watch(&[seen(true, false), seen(true, false), seen(true, true), seen(true, true)]);
        assert!(w.poll().is_empty());
        assert!(w.poll().is_empty());
        assert_eq!(w.poll(), vec![Event::ScreenLocked]);
        assert!(w.poll().is_empty());
        assert_eq!(q.1.borrow().as_slice(), ["3", "3", "3", "3"]);
    }

    #[test]
    fn stuck_locked_hint_at_start_is_ignored() {
        let (mut w, _, _) = watch(&[seen(true, true), seen(true, true), seen(true, false), seen(true, true)]);
        assert!(w.poll().is_empty());
        assert!(w.poll().is_empty());
        assert!(w.poll().is_empty());
        assert_eq!(w.poll(), vec![Event::ScreenLocked]);
    }

    #[test]
    fn inactive_or_gone_session() {
        let (mut w, _, _) = watch(&[seen(false, false), SessionAnswer::Gone]);
        assert_eq!(w.poll(), vec![Event::SessionInactive]);
        assert_eq!(w.poll(), vec![Event::SessionLost]);
    }

    #[test]
    fn silent_logind_tolerated_once() {
        let (mut w, _, _) =
            watch(&[seen(true, false), SessionAnswer::Silent, seen(true, false), SessionAnswer::Silent, SessionAnswer::Silent]);
        assert!(w.poll().is_empty());
        assert!(w.poll().is_empty());
        assert!(w.poll().is_empty());
        assert!(w.poll().is_empty());
        assert_eq!(w.poll(), vec![Event::SessionLost]);
    }

    #[test]
    fn sleep_is_caught_by_clock_gap() {
        let (mut w, _, g) = watch(&[seen(true, false), seen(true, false), seen(true, false), seen(true, false)]);
        assert!(w.poll().is_empty());
        g.0.set(g.0.get() + Duration::from_millis(3)); // read jitter is not sleep
        assert!(w.poll().is_empty());
        g.0.set(g.0.get() + min(40));
        assert_eq!(w.poll(), vec![Event::Slept]);
        assert!(w.poll().is_empty());
    }

    #[test]
    fn watch_drives_revoker() {
        let c = FakeClock::at(Duration::ZERO);
        let mut r = Revoker::granted(c.now());
        let (mut w, _, g) = watch(&[seen(true, false), seen(true, false)]);
        r.poll(&mut w, &c);
        assert_eq!(r.revoked(), None);
        r.op_started(c.now()).unwrap();
        g.0.set(g.0.get() + min(5));
        c.advance(POLL_INTERVAL);
        r.poll(&mut w, &c);
        assert_eq!(r.revoked(), Some(Reason::Slept));
        assert_eq!(r.in_flight(), 1);
    }

    // ── live clock ──

    #[test]
    fn live_clocks_are_sane() {
        let a = MonotonicClock.now();
        let b = MonotonicClock.now();
        assert!(b >= a);
        assert!(a > Duration::ZERO);
        let _ = BootMinusMonotonic.slept_total();
    }
}
