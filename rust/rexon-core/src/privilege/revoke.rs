//! Отзыв права сеанса (пункт Б6 плана «один пароль на запуск»).
//!
//! Решение владельца 28.09: право сеанса гаснет, когда
//! 1. окно Rexon закрыто, в том числе свёрнуто в лоток;
//! 2. 15 минут не было привилегированных действий;
//! 3. экран заблокирован;
//! 4. машина уснула.
//!
//! Идущая операция при этом доводится до конца, новые отвергаются; следующий
//! запрос снова спрашивает пароль (поднимает нового помощника).
//!
//! Устройство:
//! * [`Revoker`] — чистая машина состояний: ни потоков, ни часов, ни
//!   системы. Время ей приносят аргументом `now` (монотонное, от любого
//!   начала), события — [`Event`]. Так она целиком проверяется тестами на
//!   поддельных часах.
//! * [`Clock`] и [`SessionWatch`] — швы наружу. Живые:
//!   [`MonotonicClock`] и [`LogindWatch`].
//! * [`Revoker::poll`] — один шаг: забрать события наблюдателя, проверить
//!   простой. Цикл с паузой (раз в [`POLL_INTERVAL`]) крутит владелец —
//!   поток сеанса прав (Б4), здесь потоков нет.
//!
//! Как ловится блокировка и сон. D-Bus крейта в проекте нет,
//! поэтому сигналы logind (`Lock`, `PrepareForSleep`) не слушаем, а опрашиваем:
//! * блокировка и уход сеанса — `loginctl show-session <id> -p Active
//!   -p LockedHint` (тот же `loginctl`, что разбирает `login_rescue.rs`,
//!   через общий [`SystemIo::run`]);
//! * сон — по расхождению `CLOCK_BOOTTIME` и `CLOCK_MONOTONIC`: первые часы
//!   идут во сне, вторые стоят, разница растёт ровно на время сна. Настенные
//!   часы не годятся — их двигают NTP и человек, а это ложный «сон». Опрос
//!   сигнала `PrepareForSleep` не поймал бы вовсе; экран блокировки после сна
//!   поднимается не на каждом столе, так что на `LockedHint` одного не
//!   полагаемся.

use std::time::Duration;

use crate::i18n::{n_, tr};
use crate::system_exec::SystemIo;

/// Сколько простоя гасит право (решение владельца 28.09).
pub const IDLE_LIMIT: Duration = Duration::from_secs(15 * 60);

/// Как часто владельцу [`Revoker`] звать [`Revoker::poll`].
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Насколько должна вырасти разница BOOTTIME − MONOTONIC между опросами,
/// чтобы счесть это сном. Сама по себе она меняется только во сне; запас —
/// на то, что двое часов читаются не одним вызовом.
pub const SLEEP_JUMP: Duration = Duration::from_secs(1);

/// Сколько опросов подряд `loginctl` может молчать (тайм-аут, не
/// запустился), прежде чем считать сеанс потерянным. Внятный отказ
/// («No session») гасит право сразу.
pub const SILENT_POLLS_LIMIT: u32 = 2;

/// Путь к `loginctl`. Помощник работает от root — берём абсолютный путь, а
/// не поиск по `PATH`.
pub const LOGINCTL: &str = "/usr/bin/loginctl";

// ============================================================================
// События и причины
// ============================================================================

/// Что случилось снаружи и гасит право.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Окно Rexon закрыто (событие GUI, Б13; обрыв сокета помощник тоже
    /// понимает как закрытие).
    WindowClosed,
    /// Окно свернуто в лоток (событие GUI, Б13).
    WindowToTray,
    /// Экран заблокирован (`LockedHint` стал `yes`).
    ScreenLocked,
    /// Машина спала.
    Slept,
    /// Сеанс окна перестал быть активным: переключили пользователя или VT.
    SessionInactive,
    /// logind не знает сеанса окна или не отвечает о нём.
    SessionLost,
    /// Человек сам нажал «Запретить сейчас».
    RevokeNow,
}

/// Почему право погашено — для текста человеку и журнала.
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
    /// Короткое имя для журнала (не переводится).
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

    /// Причина по короткому имени ([`Reason::code`]); `None` — незнакомое.
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

    /// Текст человеку: почему снова спрашиваем пароль (Б13, без служебных
    /// слов; перевод — `po/`).
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
// Машина состояний
// ============================================================================

/// Монотонные часы: `now` — время от любого начала, не идущее назад.
pub trait Clock {
    fn now(&self) -> Duration;
}

/// Наблюдатель сеанса: за один опрос отдаёт случившееся с прошлого опроса.
pub trait SessionWatch {
    fn poll(&mut self) -> Vec<Event>;
}

/// Решает, живо ли ещё право сеанса.
///
/// Создаётся, когда пароль только что принят. Простой считается от конца
/// последней операции; пока операция идёт, простоя нет (долгая сборка не
/// гасит право посреди себя — да и погашенное право её не прервало бы).
#[derive(Debug, Clone)]
pub struct Revoker {
    idle_limit: Duration,
    revoked: Option<Reason>,
    in_flight: u32,
    last_action: Duration,
}

impl Revoker {
    /// Право только что выдано (пароль принят) в момент `now`.
    pub fn granted(now: Duration) -> Self {
        Self::with_idle_limit(now, IDLE_LIMIT)
    }

    /// То же с другим пределом простоя (для тестов и отладки).
    pub fn with_idle_limit(now: Duration, idle_limit: Duration) -> Self {
        Revoker { idle_limit, revoked: None, in_flight: 0, last_action: now }
    }

    /// Право выдано заново (пароль спросили ещё раз). Идущие операции
    /// остаются в счёте.
    pub fn regrant(&mut self, now: Duration) {
        self.revoked = None;
        self.last_action = now;
    }

    /// Внешнее событие. Гасит живое право; у погашенного остаётся первая
    /// причина — она и объясняет человеку, что случилось.
    pub fn on_event(&mut self, event: Event, now: Duration) {
        self.tick(now);
        self.revoke(event.into());
    }

    /// Проверить простой на момент `now`.
    pub fn tick(&mut self, now: Duration) {
        if self.revoked.is_none() && self.in_flight == 0 && now.saturating_sub(self.last_action) >= self.idle_limit {
            self.revoke(Reason::Idle);
        }
    }

    /// Один шаг владельца: события наблюдателя, затем простой.
    pub fn poll(&mut self, watch: &mut impl SessionWatch, clock: &impl Clock) {
        for e in watch.poll() {
            self.on_event(e, clock.now());
        }
        self.tick(clock.now());
    }

    /// Можно ли начать новую операцию. Ничего не отмечает — для этого
    /// [`Revoker::op_started`].
    pub fn allow_new_op(&mut self, now: Duration) -> Result<(), Reason> {
        self.tick(now);
        match self.revoked {
            Some(r) => Err(r),
            None => Ok(()),
        }
    }

    /// Операция начинается. Проверяет право сам, так что обойти
    /// [`Revoker::allow_new_op`] нельзя.
    pub fn op_started(&mut self, now: Duration) -> Result<(), Reason> {
        self.allow_new_op(now)?;
        self.in_flight += 1;
        self.last_action = now;
        Ok(())
    }

    /// Операция закончилась (успехом или нет) — простой считается отсюда.
    pub fn op_finished(&mut self, now: Duration) {
        self.in_flight = self.in_flight.saturating_sub(1);
        self.last_action = now;
    }

    /// Причина отзыва; `None` — право живо.
    pub fn revoked(&self) -> Option<Reason> {
        self.revoked
    }

    /// Сколько операций идёт.
    pub fn in_flight(&self) -> u32 {
        self.in_flight
    }

    /// Помощнику пора выходить: право погашено и доводить нечего.
    pub fn may_exit(&self) -> bool {
        self.revoked.is_some() && self.in_flight == 0
    }

    /// Когда право погаснет по простою, если ничего не случится; `None` —
    /// уже погашено или идёт операция.
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
// Живые часы и наблюдатель
// ============================================================================

fn clock_read(id: libc::clockid_t) -> Duration {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime пишет только в переданную структуру.
    let rc = unsafe { libc::clock_gettime(id, &mut ts) };
    if rc != 0 {
        return Duration::ZERO;
    }
    Duration::new(ts.tv_sec.max(0) as u64, ts.tv_nsec.clamp(0, 999_999_999) as u32)
}

/// `CLOCK_MONOTONIC`: во сне стоит, так что простой во сне не копится (сон
/// гасит право сам, отдельным событием).
#[derive(Debug, Clone, Copy, Default)]
pub struct MonotonicClock;

impl Clock for MonotonicClock {
    fn now(&self) -> Duration {
        clock_read(libc::CLOCK_MONOTONIC)
    }
}

/// Сколько всего машина проспала с загрузки.
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

/// Ответ logind о сеансе.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAnswer {
    /// Сеанс есть.
    Seen { active: bool, locked: bool },
    /// logind внятно сказал «нет такого сеанса» (или ответ без нужных полей).
    Gone,
    /// Ответа не было: тайм-аут, программа не запустилась.
    Silent,
}

/// Откуда берётся состояние сеанса — шов для тестов.
pub trait SessionQuery {
    fn query(&self, session: &str) -> SessionAnswer;
}

/// Живой запрос через `loginctl show-session` (только чтение).
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

/// Разбор `loginctl show-session -p Active -p LockedHint` (порядок строк —
/// как отдаст logind). Без `Active` — ответ негоден, считаем сеанс
/// потерянным. Без `LockedHint` (старый systemd) — «не заблокирован»: сон и
/// простой право всё равно погасят.
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

/// Номер сеанса logind годится в argv: непустой, короткий, только латиница
/// и цифры (`2`, `c1`). Иначе он мог бы прикинуться ключом `loginctl`.
pub fn valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Наблюдатель сеанса окна: logind + счётчик сна.
///
/// Блокировка ловится по фронту: `LockedHint` «нет → да». Первый опрос —
/// точка отсчёта: пароль только что ввели, значит экран не заблокирован, и
/// `LockedHint=yes` там — залипший признак стола; гасить по нему каждое
/// новое право нельзя. Неактивный сеанс, напротив, гасит сразу, и на
/// первом опросе тоже: пароль спрашивали не в нём.
pub struct LogindWatch<Q: SessionQuery, G: SuspendGauge> {
    query: Q,
    gauge: G,
    session: String,
    was_locked: Option<bool>,
    slept_before: Duration,
    silent: u32,
}

impl<Q: SessionQuery, G: SuspendGauge> LogindWatch<Q, G> {
    /// `None` — номер сеанса негоден (см. [`valid_session_id`]).
    pub fn new(query: Q, gauge: G, session: &str) -> Option<Self> {
        if !valid_session_id(session) {
            return None;
        }
        let slept_before = gauge.slept_total();
        Some(LogindWatch { query, gauge, session: session.to_string(), was_locked: None, slept_before, silent: 0 })
    }
}

impl<I: SystemIo, G: SuspendGauge> LogindWatch<Loginctl<I>, G> {
    /// Живой наблюдатель на `loginctl`.
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
// Тесты
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

    // ── простой ──

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

    // ── события посреди операции ──

    fn mid_operation(event: Event, reason: Reason) {
        let c = FakeClock::at(Duration::ZERO);
        let mut r = Revoker::granted(c.now());
        r.op_started(c.now()).unwrap();
        c.advance(Duration::from_secs(30));
        let mut w = FakeWatch(VecDeque::from([vec![event]]));
        r.poll(&mut w, &c);
        // идущая доводится: помощник не уходит, пока она в счёте
        assert_eq!(r.revoked(), Some(reason));
        assert_eq!(r.in_flight(), 1);
        assert!(!r.may_exit());
        // новые — отвергаются
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
        // уснули через 20 минут простоя: право уже погасло по простою
        let mut r = Revoker::granted(Duration::ZERO);
        r.on_event(Event::Slept, min(20));
        assert_eq!(r.revoked(), Some(Reason::Idle));
    }

    // ── повторное поднятие ──

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

    // ── наблюдатель logind ──

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
        g.0.set(Duration::from_secs(7)); // сон до запуска помощника не в счёт
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
        g.0.set(g.0.get() + Duration::from_millis(3)); // дрожь чтения — не сон
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

    // ── живые часы ──

    #[test]
    fn live_clocks_are_sane() {
        let a = MonotonicClock.now();
        let b = MonotonicClock.now();
        assert!(b >= a);
        assert!(a > Duration::ZERO);
        let _ = BootMinusMonotonic.slept_total();
    }
}
