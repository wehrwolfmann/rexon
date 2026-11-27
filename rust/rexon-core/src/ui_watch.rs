//! Watchdog of the window's main thread and the journal of the person's actions.
//!
//! WHY. A live case of 15.11.2026: the Rexon window hung, Cinnamon showed
//! «the application is not responding — wait/close», the owner pressed «Close»,
//! and the process was killed (SIGKILL). `rexon.log` held NOTHING about it:
//! the last entry was two minutes before, neither what the person was doing nor
//! where the main thread stood. SIGKILL cannot be caught, so the evidence must
//! land in the journal IN ADVANCE — while the process is still alive but no
//! longer responding:
//!
//!   * the action journal: every button press, toggle, choice in a list,
//!     menu item and section change is one INFO line; an action that held
//!     the main thread longer than [`SLOW_ACTION`] gets a second line with
//!     the duration;
//!   * the watchdog: a separate thread watches the event loop's heartbeat (a
//!     Qt timer on the main thread beats every [`HEARTBEAT_MS`] ms). No
//!     heartbeat for longer than [`STALL_AFTER`] — WARN «window not responding»
//!     with the current action and the main thread's stack; longer than
//!     [`SEVERE_AFTER`] — one ERROR line; the heartbeat is back — INFO
//!     «window responds again, hung for N s». At most three lines per hang.
//!
//! Journal lines go straight to the file (`applog` writes without a buffer,
//! with a `write` call): the data is already in the kernel and survives a
//! SIGKILL of the process.
//!
//! There is no Qt here: the heartbeat timer and walking the button's ancestors
//! live in `rexon-gui` (`action_journal.rs`), while the decisions, texts and
//! the watchdog are here, under tests without Qt.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::applog::{self, Level};

/// Target of the journal lines.
pub const AREA: &str = "ui";

/// How often the main thread beats the heartbeat.
pub const HEARTBEAT_MS: u64 = 250;
/// How often the watchdog looks at the heartbeat.
pub const POLL: Duration = Duration::from_millis(250);
/// No heartbeat for longer than this — the window is «not responding» (WARN).
pub const STALL_AFTER: Duration = Duration::from_secs(2);
/// No heartbeat for longer than this — one ERROR line.
pub const SEVERE_AFTER: Duration = Duration::from_secs(10);
/// An action held the main thread longer than this — a line with the duration.
pub const SLOW_ACTION: Duration = Duration::from_millis(500);
/// The longest label in a journal line, in characters.
const LABEL_MAX: usize = 80;

// ============================================================================
// Action journal
// ============================================================================

#[derive(Clone)]
struct Action {
    id: u64,
    what: String,
    started: Instant,
}

#[derive(Default)]
struct Activity {
    /// The open window section (an identifier from `qt_main::SECTIONS`).
    section: String,
    /// An action whose handler has not yet returned control to the event loop.
    current: Option<Action>,
    /// The last started action — even if it has already finished.
    last: Option<Action>,
    /// The last recorded value of each input field (place + placeholder).
    entered: std::collections::HashMap<String, String>,
}

static ACTIVITY: Mutex<Option<Activity>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn with_activity<R>(f: impl FnOnce(&mut Activity) -> R) -> R {
    let mut guard = ACTIVITY.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Activity::default))
}

/// A label for a journal line: one line, no extra spaces, no longer than
/// [`LABEL_MAX`] characters. An empty one becomes «(без подписи)»: an icon
/// button is an action too, and empty quotes would read as a journal error.
pub fn clean_label(label: &str) -> String {
    let one: String = label.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.is_empty() {
        return "(без подписи)".to_string();
    }
    let cut: String = one.chars().take(LABEL_MAX).collect();
    if cut.chars().count() < one.chars().count() {
        format!("{cut}…")
    } else {
        cut
    }
}

/// A button's place in the window, from the chain of its ancestors.
///
/// `chain` is the class names of the ancestors from nearest to farthest,
/// separated by `/`, as Qt gives them (`ScreenEffectsSection_QMLTYPE_42/QQuickColumnLayout/…`).
/// Only components declared by OUR QML files are taken (they carry the
/// suffix `_QMLTYPE_N` or `_QML_N`), except the shared `Rexon*` controls and
/// the window root; the two nearest remain, from outer to inner. In front is
/// the open window section.
///
/// Example: section `appearance`, chain
/// `RexonButton_QMLTYPE_3/QQuickRowLayout/ScreenEffectsSection_QMLTYPE_9/AppearancePage_QMLTYPE_2/MainWindow_QMLTYPE_0`
/// → `appearance ▸ AppearancePage ▸ ScreenEffectsSection`.
pub fn area_from_chain(section: &str, chain: &str) -> String {
    let mut names: Vec<&str> = Vec::new();
    for raw in chain.split('/') {
        let Some(pos) = raw.find("_QML") else { continue };
        let name = &raw[..pos];
        if name.is_empty() || name.starts_with("Rexon") || name == "MainWindow" {
            continue;
        }
        if names.last() != Some(&name) {
            names.push(name);
        }
        if names.len() == 2 {
            break;
        }
    }
    names.reverse();
    let mut parts: Vec<&str> = Vec::new();
    if !section.trim().is_empty() {
        parts.push(section.trim());
    }
    parts.extend(names);
    if parts.is_empty() {
        "окно".to_string()
    } else {
        parts.join(" ▸ ")
    }
}

/// What the person did — the body of the journal line.
#[derive(Debug, Clone, PartialEq)]
pub enum Deed<'a> {
    /// Pressed a button with a label.
    Click { label: &'a str },
    /// Toggled a checkbox with a label.
    Toggle { label: &'a str, on: bool },
    /// Picked a value in a drop-down list.
    Choose { value: &'a str },
    /// Finished typing in the field with placeholder `field`: `value` is what was
    /// typed (a password field never gets here at all, see [`note_entered`]).
    Entered { field: &'a str, value: &'a str },
}

/// A journal line about an action: `действие: <place> → «<label>»…`.
pub fn deed_line(area: &str, deed: &Deed) -> String {
    match deed {
        Deed::Click { label } => format!("действие: {area} → «{}»", clean_label(label)),
        Deed::Toggle { label, on } => format!(
            "действие: {area} → «{}»: {}",
            clean_label(label),
            if *on { "включено" } else { "выключено" }
        ),
        Deed::Choose { value } => format!("действие: {area} → выбрано «{}»", clean_label(value)),
        Deed::Entered { field, value } => {
            if value.trim().is_empty() {
                format!("действие: {area} → поле «{}» очищено", clean_label(field))
            } else {
                format!("действие: {area} → поле «{}»: «{}»", clean_label(field), clean_label(value))
            }
        }
    }
}

/// A line about a long action: `действие заняло 1234 мс: <what>`.
pub fn slow_line(what: &str, took: Duration) -> String {
    format!("действие заняло {} мс: {what}", took.as_millis())
}

/// A line about a change of the window section.
pub fn section_line(section: &str) -> String {
    format!("раздел: {}", clean_label(section))
}

/// Section `section` is open — a journal line and the place for the next
/// lines. A repeat of the same section is not written.
pub fn note_section(section: &str) {
    let changed = with_activity(|a| {
        if a.section == section {
            return false;
        }
        a.section = section.to_string();
        true
    });
    if changed {
        applog::write(Level::Info, AREA, &section_line(section));
    }
}

/// The open window section (empty — none opened yet).
pub fn current_section() -> String {
    with_activity(|a| a.section.clone())
}

/// The person did `deed` in place `area`: a journal line, and the action
/// becomes «current» until its handler returns control
/// ([`finish_deed`]). The answer is the number for [`finish_deed`].
pub fn begin_deed(area: &str, deed: &Deed) -> u64 {
    let what = deed_line(area, deed);
    applog::write(Level::Info, AREA, &what);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let action = Action { id, what, started: Instant::now() };
    with_activity(|a| {
        a.current = Some(action.clone());
        a.last = Some(action);
    });
    id
}

/// The handler of action `id` returned control to the event loop. If it held
/// the main thread longer than [`SLOW_ACTION`] — a line with the duration.
pub fn finish_deed(id: u64) {
    let done = with_activity(|a| match &a.current {
        Some(c) if c.id == id => a.current.take(),
        _ => None,
    });
    if let Some(action) = done {
        let took = action.started.elapsed();
        if took >= SLOW_ACTION {
            let what = action.what.strip_prefix("действие: ").unwrap_or(&action.what);
            applog::write(Level::Info, AREA, &slow_line(what, took));
        }
    }
}

/// What the window is doing right now — for the watchdog's line.
pub fn activity_report() -> String {
    with_activity(|a| {
        let now = Instant::now();
        let section = if a.section.is_empty() { "—".to_string() } else { a.section.clone() };
        match (&a.current, &a.last) {
            (Some(c), _) => format!(
                "выполняется «{}» (начато {:.1} с назад); раздел {section}",
                c.what,
                now.duration_since(c.started).as_secs_f64()
            ),
            (None, Some(l)) => format!(
                "текущего действия нет; последнее — «{}» ({:.1} с назад); раздел {section}",
                l.what,
                now.duration_since(l.started).as_secs_f64()
            ),
            (None, None) => format!("действий ещё не было; раздел {section}"),
        }
    })
}

// ============================================================================
// Mouse and keyboard input: which action it is
// ============================================================================
//
// Presses are caught by ONE application event filter (`rexon-gui`,
// `action_journal.rs` + the shim `rexon_install_input_journal`): buttons,
// checkboxes, menu and drop-down items, store cards with
// `TapHandler`/`MouseArea` — everything that took a mouse press (or
// Space/Enter on a focused button). The shim decides nothing: it hands over
// the chain «who took the press → its ancestors» as lines, and what of this
// is an action and how to name it is decided here.

/// One link of the chain from the input filter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InputItem {
    /// The Qt class name (`RexonButton_QMLTYPE_3`, `QQuickTapHandler`).
    pub class: String,
    /// Flags (letters), see [`parse_input_chain`].
    pub flags: String,
    /// `objectName`.
    pub name: String,
    /// Its own label (`text`, for a list — `displayText`, for a window — `title`).
    /// For input fields the shim does NOT take it: it may be a password.
    pub text: String,
    /// The first caption (`Text`) inside — for cards and icon buttons.
    pub inner: String,
}

impl InputItem {
    fn has(&self, flag: char) -> bool {
        self.flags.contains(flag)
    }
}

/// Parses the description from the shim: one link per line, from the one that
/// took the press to the far ancestors; fields separated by tabs — class,
/// flags, `objectName`, label, caption inside. Flags: `b` — button
/// (`AbstractButton`, including a menu item and a list row), `k` — checkable,
/// `K` — currently checked, `r` — radio button, `x` — drop-down list, `p` —
/// popup, `i` — input field, `h` — pointer handler (`TapHandler`), `m` —
/// `MouseArea`, `f` — scrollable area, `s` — slider/scroll bar, `t` — a
/// `TabButton` tab.
pub fn parse_input_chain(desc: &str) -> Vec<InputItem> {
    desc.lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut f = line.split('\t');
            let mut next = || f.next().unwrap_or("").to_string();
            InputItem { class: next(), flags: next(), name: next(), text: next(), inner: next() }
        })
        .collect()
}

/// The action of a press — with owned strings (the journal line goes through [`Deed`]).
#[derive(Debug, Clone, PartialEq)]
pub enum InputDeed {
    Click(String),
    Toggle(String, bool),
    Choose(String),
}

/// What the person did with this press; `None` — not an action (scrolling, a
/// click in an input field, opening a drop-down list — the choice in it will
/// be a separate line, a tab of the top bar — the «section» line writes it).
/// `via_key` — Space/Enter on the focused element: an action only for buttons.
/// The answer is the place (see [`area_from_chain`]) and the action.
pub fn deed_from_input(section: &str, items: &[InputItem], via_key: bool) -> Option<(String, InputDeed)> {
    let first = items.first()?;
    if first.has('i') || first.has('f') || first.has('s') || first.has('x') {
        return None;
    }
    if via_key && !first.has('b') {
        return None;
    }
    if !(first.has('b') || first.has('m') || first.has('h')) {
        return None;
    }
    let chain: Vec<&str> = items.iter().map(|i| i.class.as_str()).collect();
    let chain = chain.join("/");
    if first.has('t') && area_from_chain("", &chain) == "окно" {
        // A tab of the section bar: the section change is written by `note_section`.
        return None;
    }
    let area = area_from_chain(section, &chain);
    let label = std::iter::once(first.text.as_str())
        .chain(items.iter().take(3).map(|i| i.inner.as_str()))
        .chain(std::iter::once(first.name.as_str()))
        .find(|t| !t.trim().is_empty())
        .unwrap_or("")
        .to_string();
    // A row of a `ComboBox` popup list: above it is the popup window, higher
    // still is the list itself.
    if first.has('b') {
        if let Some(popup) = items.iter().skip(1).position(|i| i.has('p')) {
            if items.iter().skip(2 + popup).any(|i| i.has('x')) {
                return Some((area, InputDeed::Choose(label)));
            }
        }
    }
    if first.has('k') {
        let on = first.has('r') || !first.has('K');
        return Some((area, InputDeed::Toggle(label, on)));
    }
    Some((area, InputDeed::Click(label)))
}

/// A press from the input filter: a journal line and the «current action» if
/// it is an action. → the number for [`finish_deed`], 0 — not an action.
pub fn begin_input(desc: &str, via_key: bool) -> u64 {
    let items = parse_input_chain(desc);
    let Some((area, deed)) = deed_from_input(&current_section(), &items, via_key) else {
        return 0;
    };
    match &deed {
        InputDeed::Click(label) => begin_deed(&area, &Deed::Click { label }),
        InputDeed::Toggle(label, on) => begin_deed(&area, &Deed::Toggle { label, on: *on }),
        InputDeed::Choose(value) => begin_deed(&area, &Deed::Choose { value }),
    }
}

// ============================================================================
// Input fields
// ============================================================================
//
// An input field is the shared control `RexonTextField`: the journal does
// not need every key press (noise), but it does need the last thing typed
// (if a search hangs, you can see what was searched). So typing is only
// marked as the «last action» without a line, and a line is written when the
// input is finished (Enter or focus leaving) and the value differs from the
// one recorded before.
//
// ⛔ A password field (`echoMode` not `Normal`) is NEVER written — neither a
// line nor a mark with the text: QML does not even pass its text here, and
// the core, just in case, does not look at it either.

/// The person is typing in field `field` (the field's placeholder) of place
/// `area`: a mark for the watchdog, without a journal line.
pub fn note_typing(area: &str, field: &str, value: &str, secret: bool) {
    let what = if secret {
        format!("ввод в поле «{}» ({area}), текст скрыт", clean_label(field))
    } else {
        format!("ввод в поле «{}» ({area}): «{}»", clean_label(field), clean_label(value))
    };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    with_activity(|a| a.last = Some(Action { id, what, started: Instant::now() }));
}

/// Input in the field is finished. A journal line — if the value differs from
/// the one recorded before for this field (and is not empty the first time).
/// A password is not written. → the number for [`finish_deed`], 0 — no line.
pub fn note_entered(area: &str, field: &str, value: &str, secret: bool) -> u64 {
    if secret {
        return 0;
    }
    let key = format!("{area}\u{1f}{field}");
    let value_one = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let fresh = with_activity(|a| {
        let prev = a.entered.get(&key);
        let changed = match prev {
            Some(p) => *p != value_one,
            None => !value_one.is_empty(),
        };
        if changed {
            a.entered.insert(key.clone(), value_one.clone());
        }
        changed
    });
    if !fresh {
        return 0;
    }
    begin_deed(area, &Deed::Entered { field, value: &value_one })
}

// ============================================================================
// Watchdog: the decision
// ============================================================================

/// The watchdog's thresholds.
#[derive(Debug, Clone, Copy)]
pub struct WatchConfig {
    pub stall_after: Duration,
    pub severe_after: Duration,
    pub poll: Duration,
}

impl Default for WatchConfig {
    fn default() -> Self {
        WatchConfig { stall_after: STALL_AFTER, severe_after: SEVERE_AFTER, poll: POLL }
    }
}

/// What the watchdog saw on its latest look.
#[derive(Debug, Clone, PartialEq)]
pub enum WatchEvent {
    /// No heartbeat for `silent` — the window is not responding.
    Stalled { silent: Duration },
    /// No heartbeat for longer than the severe threshold.
    Severe { silent: Duration },
    /// The heartbeat is back; the window hung for `hung`.
    Recovered { hung: Duration },
}

/// The watchdog's state between looks.
#[derive(Debug, Default, Clone)]
pub struct WatchState {
    /// The mark of the last heartbeat at which the window hung.
    stalled_at: Option<u64>,
    severe_said: bool,
}

/// One look of the watchdog. `now_ms` and `beat_ms` are milliseconds from a
/// common origin; `beat_ms == 0` — there has been no heartbeat yet (the event
/// loop has not started), and there is nothing to judge.
pub fn watch_step(state: &mut WatchState, now_ms: u64, beat_ms: u64, cfg: &WatchConfig) -> Option<WatchEvent> {
    if beat_ms == 0 {
        return None;
    }
    let silent = Duration::from_millis(now_ms.saturating_sub(beat_ms));
    match state.stalled_at {
        None => {
            if silent > cfg.stall_after {
                state.stalled_at = Some(beat_ms);
                state.severe_said = false;
                return Some(WatchEvent::Stalled { silent });
            }
            None
        }
        Some(at) if beat_ms != at => {
            // A new heartbeat: the hang is over. It hung from the previous heartbeat to
            // the new one (the watchdog looks once per `poll`, its lateness does not count).
            state.stalled_at = None;
            state.severe_said = false;
            Some(WatchEvent::Recovered { hung: Duration::from_millis(beat_ms.saturating_sub(at)) })
        }
        Some(_) => {
            if !state.severe_said && silent > cfg.severe_after {
                state.severe_said = true;
                return Some(WatchEvent::Severe { silent });
            }
            None
        }
    }
}

/// The WARN line about a hang.
pub fn stall_line(silent: Duration, activity: &str, stack: Option<&str>) -> String {
    let mut s = format!("окно не отвечает {:.1} с; {activity}", silent.as_secs_f64());
    match stack {
        Some(st) if !st.trim().is_empty() => {
            s.push_str("\nстек главного потока:\n");
            s.push_str(st.trim_end());
        }
        _ => s.push_str("\nстек главного потока снять не удалось"),
    }
    s
}

/// The ERROR line about a long hang.
pub fn severe_line(silent: Duration, activity: &str) -> String {
    format!(
        "окно не отвечает уже {:.0} с (дольше {} с); {activity}",
        silent.as_secs_f64(),
        SEVERE_AFTER.as_secs()
    )
}

/// The INFO line about recovery.
pub fn recovered_line(hung: Duration) -> String {
    format!("окно снова отвечает, висело {:.1} с", hung.as_secs_f64())
}

// ============================================================================
// Watchdog: the thread
// ============================================================================

/// Where the watchdog writes its lines (in the product — `applog::write`).
pub type Sink = Arc<dyn Fn(Level, &str) + Send + Sync>;
/// Take the stack of the watched thread (in the product — [`stack::capture`]).
pub type StackFn = Arc<dyn Fn() -> Option<String> + Send + Sync>;
/// What is being done right now (in the product — [`activity_report`]).
pub type ActivityFn = Arc<dyn Fn() -> String + Send + Sync>;

/// The heartbeat of one thread and the watchdog over it.
pub struct Watchdog {
    origin: Instant,
    beat: AtomicU64,
    stop: AtomicBool,
}

impl Watchdog {
    pub fn new() -> Arc<Watchdog> {
        Arc::new(Watchdog { origin: Instant::now(), beat: AtomicU64::new(0), stop: AtomicBool::new(false) })
    }

    fn now_ms(&self) -> u64 {
        // +1: zero is reserved for «no heartbeat yet».
        self.origin.elapsed().as_millis() as u64 + 1
    }

    /// A heartbeat — from the watched thread.
    pub fn beat(&self) {
        self.beat.store(self.now_ms(), Ordering::Relaxed);
    }

    /// Stop the watchdog thread (test stands; in the product it lives until exit).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Start the watchdog thread.
    pub fn spawn(
        self: &Arc<Self>,
        cfg: WatchConfig,
        sink: Sink,
        activity: ActivityFn,
        stack: StackFn,
    ) -> Option<std::thread::JoinHandle<()>> {
        let me = Arc::clone(self);
        std::thread::Builder::new()
            .name("rexon-ui-watch".into())
            .spawn(move || {
                let mut state = WatchState::default();
                while !me.stop.load(Ordering::Relaxed) {
                    std::thread::sleep(cfg.poll);
                    let beat = me.beat.load(Ordering::Relaxed);
                    match watch_step(&mut state, me.now_ms(), beat, &cfg) {
                        Some(WatchEvent::Stalled { silent }) => {
                            let what = activity();
                            let st = stack();
                            sink(Level::Warn, &stall_line(silent, &what, st.as_deref()));
                        }
                        Some(WatchEvent::Severe { silent }) => sink(Level::Error, &severe_line(silent, &activity())),
                        Some(WatchEvent::Recovered { hung }) => sink(Level::Info, &recovered_line(hung)),
                        None => {}
                    }
                }
            })
            .ok()
    }
}

static MAIN: OnceLock<Arc<Watchdog>> = OnceLock::new();

/// A heartbeat of the main thread (the Qt timer in `rexon-gui`).
pub extern "C" fn main_heartbeat() {
    if let Some(w) = MAIN.get() {
        w.beat();
    }
}

/// Start the main thread's watchdog. Call FROM THE MAIN thread (its stack is
/// what gets taken), before the event loop; a repeated call does nothing.
pub fn install_main_watchdog() {
    if MAIN.get().is_some() {
        return;
    }
    let dog = Watchdog::new();
    if MAIN.set(Arc::clone(&dog)).is_err() {
        return;
    }
    let stack_ok = stack::watch_current_thread();
    let sink: Sink = Arc::new(|level, msg| applog::write(level, AREA, msg));
    let stack_fn: StackFn = Arc::new(move || if stack_ok { stack::capture(Duration::from_millis(300)) } else { None });
    if dog.spawn(WatchConfig::default(), sink, Arc::new(activity_report), stack_fn).is_none() {
        applog::write(Level::Warn, AREA, "сторож окна не запустился: поток не создан");
    }
}

// ============================================================================
// Stack of the main thread
// ============================================================================

/// The stack of the watched thread: the watchdog sends it a signal, the
/// handler (on that thread) collects the return addresses of glibc's
/// `backtrace()` into a shared buffer, and the names for the addresses are
/// looked up by the watchdog itself (`dladdr`) — only a safe minimum is
/// allowed inside a signal handler.
///
/// Library functions (Qt, glibc) have names — they show where the thread
/// stands («QProcess::waitForFinished», «QQmlComponent::create»). Rexon's own
/// functions are not in the dynamic table: for them — the file name and the
/// offset, which `addr2line -e <binary> <offset>` expands.
pub mod stack {
    use std::os::raw::{c_int, c_void};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    const MAX_FRAMES: usize = 48;
    /// How many frames to show in the journal.
    const SHOW_FRAMES: usize = 28;

    extern "C" {
        fn backtrace(buffer: *mut *mut c_void, size: c_int) -> c_int;
    }

    static TARGET: AtomicUsize = AtomicUsize::new(0);
    static READY: AtomicBool = AtomicBool::new(false);
    static FRAMES: [AtomicUsize; MAX_FRAMES] = [const { AtomicUsize::new(0) }; MAX_FRAMES];
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    static REQUEST: AtomicU32 = AtomicU32::new(0);
    static DONE: AtomicU32 = AtomicU32::new(0);

    /// The stack-capture signal is one of the user real-time signals: Qt and
    /// glibc have none of their own on them, while SIGPROF/SIGUSR* may be taken.
    fn stack_signal() -> c_int {
        libc::SIGRTMIN() + 6
    }

    extern "C" fn on_stack_signal(_sig: c_int) {
        // Do not clobber the errno of the interrupted code.
        let saved = unsafe { *libc::__errno_location() };
        let mut buf = [std::ptr::null_mut::<c_void>(); MAX_FRAMES];
        let n = unsafe { backtrace(buf.as_mut_ptr(), MAX_FRAMES as c_int) }.max(0) as usize;
        for (slot, addr) in FRAMES.iter().zip(buf.iter().take(n)) {
            slot.store(*addr as usize, Ordering::Relaxed);
        }
        COUNT.store(n, Ordering::Relaxed);
        DONE.store(REQUEST.load(Ordering::Acquire), Ordering::Release);
        unsafe { *libc::__errno_location() = saved };
    }

    /// Watch the current thread. `false` — the handler was not installed.
    pub fn watch_current_thread() -> bool {
        // The first `backtrace` call loads libgcc_s (`dlopen`) — that is not
        // allowed in a signal handler, so the warm-up happens here.
        let mut warm = [std::ptr::null_mut::<c_void>(); 4];
        unsafe { backtrace(warm.as_mut_ptr(), warm.len() as c_int) };
        unsafe {
            let mut act: libc::sigaction = std::mem::zeroed();
            act.sa_sigaction = on_stack_signal as extern "C" fn(c_int) as usize;
            // SA_RESTART: an interrupted call (read, waitpid) resumes by itself.
            // WITHOUT SA_ONSTACK: Rust's alternate signal stack is too small for the walk.
            act.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut act.sa_mask);
            if libc::sigaction(stack_signal(), &act, std::ptr::null_mut()) != 0 {
                return false;
            }
            TARGET.store(libc::pthread_self() as usize, Ordering::SeqCst);
        }
        READY.store(true, Ordering::SeqCst);
        true
    }

    /// Take the stack of the watched thread; `None` — the thread did not accept
    /// the signal within `timeout` (for example, it is in an uninterruptible disk wait).
    pub fn capture(timeout: Duration) -> Option<String> {
        capture_frames(timeout).map(|f| describe(&f))
    }

    /// Raw frame addresses.
    pub fn capture_frames(timeout: Duration) -> Option<Vec<usize>> {
        if !READY.load(Ordering::SeqCst) {
            return None;
        }
        let n = REQUEST.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        let target = TARGET.load(Ordering::SeqCst) as libc::pthread_t;
        if unsafe { libc::pthread_kill(target, stack_signal()) } != 0 {
            return None;
        }
        let deadline = Instant::now() + timeout;
        while DONE.load(Ordering::Acquire) != n {
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let count = COUNT.load(Ordering::Relaxed).min(MAX_FRAMES);
        Some(FRAMES.iter().take(count).map(|a| a.load(Ordering::Relaxed)).collect())
    }

    type Demangle =
        unsafe extern "C" fn(*const libc::c_char, *mut libc::c_char, *mut usize, *mut c_int) -> *mut libc::c_char;

    /// `__cxa_demangle` from libstdc++, if it is in the process (in the Qt window
    /// it always is; in core tests it may not be, then the name stays as is).
    fn demangle(name: &str) -> String {
        static SYM: &[u8] = b"__cxa_demangle\0";
        let f = unsafe { libc::dlsym(libc::RTLD_DEFAULT, SYM.as_ptr().cast()) };
        if f.is_null() || !name.starts_with("_Z") {
            return name.to_string();
        }
        let Ok(c) = std::ffi::CString::new(name) else { return name.to_string() };
        let f: Demangle = unsafe { std::mem::transmute(f) };
        let mut status: c_int = -1;
        let out = unsafe { f(c.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), &mut status) };
        if out.is_null() || status != 0 {
            return name.to_string();
        }
        let s = unsafe { std::ffi::CStr::from_ptr(out) }.to_string_lossy().into_owned();
        unsafe { libc::free(out.cast()) };
        s
    }

    fn frame_text(addr: usize) -> (String, Option<String>) {
        let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
        if unsafe { libc::dladdr(addr as *const c_void, &mut info) } == 0 {
            return (format!("0x{addr:x}"), None);
        }
        let file = if info.dli_fname.is_null() {
            "?".to_string()
        } else {
            let full = unsafe { std::ffi::CStr::from_ptr(info.dli_fname) }.to_string_lossy().into_owned();
            full.rsplit('/').next().unwrap_or(&full).to_string()
        };
        if !info.dli_sname.is_null() {
            let raw = unsafe { std::ffi::CStr::from_ptr(info.dli_sname) }.to_string_lossy().into_owned();
            let off = addr.saturating_sub(info.dli_saddr as usize);
            let mut name = demangle(&raw);
            if name.chars().count() > 160 {
                name = name.chars().take(160).collect::<String>() + "…";
            }
            return (format!("{file}: {name}+0x{off:x}"), Some(raw));
        }
        let off = addr.saturating_sub(info.dli_fbase as usize);
        (format!("{file}+0x{off:x}"), None)
    }

    /// Frames as text, one per line, without the frames of the handler itself.
    pub fn describe(frames: &[usize]) -> String {
        let texts: Vec<(String, Option<String>)> = frames.iter().map(|a| frame_text(*a)).collect();
        // Frames before the signal trampoline (`__restore_rt`) are the handler.
        let start = texts
            .iter()
            .position(|(_, raw)| raw.as_deref() == Some("__restore_rt"))
            .map(|i| i + 1)
            .unwrap_or(1.min(texts.len()));
        let shown: Vec<String> = texts[start..]
            .iter()
            .take(SHOW_FRAMES)
            .enumerate()
            .map(|(i, (t, _))| format!("#{i} {t}"))
            .collect();
        let mut out = shown.join("\n");
        if texts.len() - start > SHOW_FRAMES {
            out.push_str(&format!("\n… ещё {} кадров", texts.len() - start - SHOW_FRAMES));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands that read the shared «current action» run one at a time.
    static SHARED_ACTIVITY: Mutex<()> = Mutex::new(());

    #[test]
    fn подпись_одной_строкой_и_с_пределом() {
        assert_eq!(clean_label("  ✓  Применить\n«Сепия» "), "✓ Применить «Сепия»");
        assert_eq!(clean_label(""), "(без подписи)");
        let long = "я".repeat(100);
        let c = clean_label(&long);
        assert_eq!(c.chars().count(), LABEL_MAX + 1);
        assert!(c.ends_with('…'));
    }

    #[test]
    fn место_по_цепочке_предков() {
        let chain = "RexonButton_QMLTYPE_3/QQuickRowLayout/ScreenEffectsSection_QMLTYPE_9/QQuickColumnLayout/AppearancePage_QMLTYPE_2/QQuickLoader/MainWindow_QMLTYPE_0";
        assert_eq!(area_from_chain("appearance", chain), "appearance ▸ AppearancePage ▸ ScreenEffectsSection");
        // Only the two nearest components.
        let deep = "RexonButton_QMLTYPE_1/ConkySets_QMLTYPE_7/ConkySettings_QMLTYPE_5/AppearancePage_QMLTYPE_2";
        assert_eq!(area_from_chain("appearance", deep), "appearance ▸ ConkySettings ▸ ConkySets");
        // An inline component (`_QML_N`) and a repeated name.
        assert_eq!(area_from_chain("", "Card_QML_12/Card_QML_12/QQuickItem"), "Card");
        assert_eq!(area_from_chain("", "QQuickItem/QQuickPopupItem"), "окно");
        assert_eq!(area_from_chain("steam", ""), "steam");
    }

    #[test]
    fn строки_действий() {
        let area = "appearance ▸ ScreenEffectsSection";
        assert_eq!(
            deed_line(area, &Deed::Click { label: "Применить" }),
            "действие: appearance ▸ ScreenEffectsSection → «Применить»"
        );
        assert_eq!(
            deed_line(area, &Deed::Toggle { label: "Включить", on: true }),
            "действие: appearance ▸ ScreenEffectsSection → «Включить»: включено"
        );
        assert_eq!(
            deed_line(area, &Deed::Toggle { label: "Включить", on: false }),
            "действие: appearance ▸ ScreenEffectsSection → «Включить»: выключено"
        );
        assert_eq!(
            deed_line(area, &Deed::Choose { value: "Сепия" }),
            "действие: appearance ▸ ScreenEffectsSection → выбрано «Сепия»"
        );
        assert_eq!(
            deed_line("extensions", &Deed::Entered { field: "Поиск", value: "dash to dock" }),
            "действие: extensions → поле «Поиск»: «dash to dock»"
        );
        assert_eq!(
            deed_line("extensions", &Deed::Entered { field: "Поиск", value: "" }),
            "действие: extensions → поле «Поиск» очищено"
        );
        assert_eq!(section_line("steam"), "раздел: steam");
        assert_eq!(
            slow_line("appearance → «Применить»", Duration::from_millis(1234)),
            "действие заняло 1234 мс: appearance → «Применить»"
        );
    }

    #[test]
    fn текущее_действие_видно_сторожу_до_конца_обработчика() {
        let _serial = SHARED_ACTIVITY.lock().unwrap_or_else(|e| e.into_inner());
        let id = begin_deed("тест ▸ Стенд", &Deed::Click { label: "Висеть" });
        let during = activity_report();
        assert!(during.contains("выполняется «действие: тест ▸ Стенд → «Висеть»»"), "{during}");
        finish_deed(id);
        let after = activity_report();
        assert!(after.contains("текущего действия нет; последнее — «действие: тест ▸ Стенд → «Висеть»»"), "{after}");
        // Someone else's number does not close the current action.
        let id2 = begin_deed("тест", &Deed::Click { label: "Ещё" });
        finish_deed(id2 + 1000);
        assert!(activity_report().starts_with("выполняется"));
        finish_deed(id2);
    }

    fn chain(lines: &[&str]) -> Vec<InputItem> {
        parse_input_chain(&lines.join("\n"))
    }

    #[test]
    fn нажатие_кнопки_флажка_и_строки_списка() {
        // A button of the shared control: its own label.
        let b = chain(&[
            "RexonButton_QMLTYPE_3\tb\t\tПрименить\tПрименить",
            "QQuickRowLayout\t\t\t\t",
            "ScreenEffectsSection_QMLTYPE_9\t\t\t\t",
            "AppearancePage_QMLTYPE_2\t\t\t\t",
            "MainWindow_QMLTYPE_0\t\t\t\t",
        ]);
        assert_eq!(
            deed_from_input("appearance", &b, false),
            Some(("appearance ▸ AppearancePage ▸ ScreenEffectsSection".into(), InputDeed::Click("Применить".into())))
        );
        // A checkbox: the shim sees the state BEFORE the click — the new one is written.
        let c = chain(&["RexonCheckBox_QMLTYPE_4\tbkK\t\tВключить\t", "SettingsPage_QMLTYPE_1\t\t\t\t"]);
        assert_eq!(
            deed_from_input("settings", &c, false),
            Some(("settings ▸ SettingsPage".into(), InputDeed::Toggle("Включить".into(), false)))
        );
        let r = chain(&["QQuickRadioButton\tbkKr\t\tТёмная\t", "SettingsPage_QMLTYPE_1\t\t\t\t"]);
        assert_eq!(deed_from_input("settings", &r, false).unwrap().1, InputDeed::Toggle("Тёмная".into(), true));
        // A drop-down list row: row → popup → list.
        let d = chain(&[
            "QQuickItemDelegate\tb\t\tСепия\tСепия",
            "QQuickListView\tf\t\t\t",
            "QQuickPopupItem\tp\t\t\t",
            "QQuickPopup\t\t\t\t",
            "RexonComboBox_QMLTYPE_7\tx\t\tОттенки серого\t",
            "ScreenEffectsSection_QMLTYPE_9\t\t\t\t",
        ]);
        assert_eq!(
            deed_from_input("appearance", &d, false),
            Some(("appearance ▸ ScreenEffectsSection".into(), InputDeed::Choose("Сепия".into())))
        );
        // A window menu item — a popup without a list: an ordinary press.
        let m = chain(&[
            "QQuickMenuItem\tb\t\tПерезапустить\t",
            "QQuickPopupItem\tp\t\t\t",
            "QQuickMenu\t\t\t\t",
            "WindowMenu_QMLTYPE_5\t\twindowMenu\t\t",
        ]);
        assert_eq!(deed_from_input("", &m, false).unwrap(), ("WindowMenu".into(), InputDeed::Click("Перезапустить".into())));
    }

    #[test]
    fn карточка_магазина_и_что_не_действие() {
        // A card's TapHandler: the label is the card's first caption.
        let t = chain(&[
            "QQuickTapHandler\th\t\t\t",
            "QQuickRectangle\t\t\t\tGruvbox Plasma",
            "AppearancePage_QMLTYPE_2\t\t\t\t",
        ]);
        assert_eq!(
            deed_from_input("appearance", &t, false),
            Some(("appearance ▸ AppearancePage".into(), InputDeed::Click("Gruvbox Plasma".into())))
        );
        // An icon button without captions — by objectName, and without that — no label.
        let i = chain(&["QQuickButton\tb\tcloseButton\t\t"]);
        assert_eq!(deed_from_input("", &i, false).unwrap().1, InputDeed::Click("closeButton".into()));
        // Not actions.
        for first in [
            "RexonTextField_QMLTYPE_1\ti\t\t\t",
            "QQuickListView\tf\t\t\t",
            "QQuickScrollBar\ts\t\t\t",
            "RexonComboBox_QMLTYPE_7\tx\t\tСепия\t",
            "QQuickRectangle\t\t\t\tТекст",
        ] {
            assert_eq!(deed_from_input("x", &chain(&[first]), false), None, "{first}");
        }
        // A tab of the section bar is written as a «section» line, while a tab
        // inside a page is an action.
        let top = chain(&["QQuickTabButton\tbt\t\tОформление\t", "QQuickTabBar\t\t\t\t", "MainWindow_QMLTYPE_0\t\t\t\t"]);
        assert_eq!(deed_from_input("steam", &top, false), None);
        let inner = chain(&["QQuickTabButton\tbt\t\tСцены\t", "QQuickTabBar\t\t\t\t", "WePage_QMLTYPE_3\t\t\t\t"]);
        assert_eq!(deed_from_input("we", &inner, false).unwrap().1, InputDeed::Click("Сцены".into()));
        // A key — only on a button.
        let ma = chain(&["QQuickMouseArea\tm\t\t\tКарточка"]);
        assert!(deed_from_input("", &ma, false).is_some());
        assert_eq!(deed_from_input("", &ma, true), None);
    }

    #[test]
    fn поле_ввода_одна_строка_и_никаких_паролей() {
        let _serial = SHARED_ACTIVITY.lock().unwrap_or_else(|e| e.into_inner());
        let area = "стенд-поле ▸ ExtensionsPage";
        // Empty the first time (focus left, nothing typed) — stay silent.
        assert_eq!(note_entered(area, "Поиск", "", false), 0);
        let id = note_entered(area, "Поиск", "dash  to dock", false);
        assert!(id > 0);
        finish_deed(id);
        // A repeat of the same (Enter, then focus leaving) — stay silent.
        assert_eq!(note_entered(area, "Поиск", "dash to dock", false), 0);
        // Cleared — a line.
        let id = note_entered(area, "Поиск", "", false);
        assert!(id > 0);
        finish_deed(id);
        // A password — never.
        assert_eq!(note_entered(area, "Пароль", "secret", true), 0);
        note_typing(area, "Пароль", "secret", true);
        let rep = activity_report();
        assert!(!rep.contains("secret"), "{rep}");
        assert!(rep.contains("текст скрыт"), "{rep}");
        note_typing(area, "Поиск", "gruv", false);
        assert!(activity_report().contains("«Поиск» (стенд-поле ▸ ExtensionsPage): «gruv»"));
    }

    #[test]
    fn сторож_решает_один_раз_на_зависание() {
        let cfg = WatchConfig {
            stall_after: Duration::from_secs(2),
            severe_after: Duration::from_secs(10),
            poll: Duration::from_millis(250),
        };
        let mut st = WatchState::default();
        // No heartbeat yet — stay silent, however much time passes.
        assert_eq!(watch_step(&mut st, 60_000, 0, &cfg), None);
        // The heartbeat is beating — stay silent.
        assert_eq!(watch_step(&mut st, 1_250, 1_000, &cfg), None);
        // Exactly 2 s is not a hang yet.
        assert_eq!(watch_step(&mut st, 3_000, 1_000, &cfg), None);
        assert_eq!(
            watch_step(&mut st, 3_250, 1_000, &cfg),
            Some(WatchEvent::Stalled { silent: Duration::from_millis(2_250) })
        );
        // Further on, no repeats until the severe threshold.
        for t in (3_500..=11_000).step_by(250) {
            assert_eq!(watch_step(&mut st, t, 1_000, &cfg), None, "t={t}");
        }
        assert_eq!(
            watch_step(&mut st, 11_250, 1_000, &cfg),
            Some(WatchEvent::Severe { silent: Duration::from_millis(10_250) })
        );
        assert_eq!(watch_step(&mut st, 30_000, 1_000, &cfg), None);
        // The heartbeat is back: it hung from the previous heartbeat to the new one.
        assert_eq!(
            watch_step(&mut st, 30_300, 30_100, &cfg),
            Some(WatchEvent::Recovered { hung: Duration::from_millis(29_100) })
        );
        assert_eq!(watch_step(&mut st, 30_600, 30_350, &cfg), None);
    }

    #[test]
    fn строки_сторожа() {
        let s = stall_line(Duration::from_millis(2_250), "раздел steam", Some("#0 libc.so.6: poll+0x10\n"));
        assert_eq!(s, "окно не отвечает 2.2 с; раздел steam\nстек главного потока:\n#0 libc.so.6: poll+0x10");
        let s = stall_line(Duration::from_millis(3_000), "раздел steam", None);
        assert!(s.ends_with("стек главного потока снять не удалось"));
        assert_eq!(severe_line(Duration::from_millis(10_400), "x"), "окно не отвечает уже 10 с (дольше 10 с); x");
        assert_eq!(recovered_line(Duration::from_millis(3_460)), "окно снова отвечает, висело 3.5 с");
    }

    /// A live run: a thread with a heartbeat artificially «hangs» in sleep — the
    /// watchdog writes a WARN with the stack, one ERROR line and an INFO on recovery.
    #[test]
    fn сторож_ловит_зависший_поток_и_снимает_его_стек() {
        let lines: Arc<Mutex<Vec<(Level, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_lines = Arc::clone(&lines);
        let sink: Sink = Arc::new(move |l, m| sink_lines.lock().unwrap().push((l, m.to_string())));
        let cfg = WatchConfig {
            stall_after: Duration::from_millis(600),
            severe_after: Duration::from_millis(1_500),
            poll: Duration::from_millis(50),
        };
        let dog = Watchdog::new();
        // The watched thread is this one, like the window's main thread in the product.
        assert!(stack::watch_current_thread());
        let handle = dog
            .spawn(
                cfg,
                sink,
                Arc::new(|| "выполняется «действие: стенд → «Висеть»»".to_string()),
                Arc::new(|| stack::capture(Duration::from_millis(500))),
            )
            .expect("поток сторожа");
        for _ in 0..6 {
            dog.beat();
            std::thread::sleep(Duration::from_millis(50));
        }
        // A hang: the thread does not beat the heartbeat for 2 s.
        std::thread::sleep(Duration::from_millis(2_000));
        for _ in 0..8 {
            dog.beat();
            std::thread::sleep(Duration::from_millis(50));
        }
        dog.stop();
        handle.join().unwrap();

        let got = lines.lock().unwrap().clone();
        let warns: Vec<_> = got.iter().filter(|(l, _)| *l == Level::Warn).collect();
        let errors: Vec<_> = got.iter().filter(|(l, _)| *l == Level::Error).collect();
        let infos: Vec<_> = got.iter().filter(|(l, _)| *l == Level::Info).collect();
        assert_eq!((warns.len(), errors.len(), infos.len()), (1, 1, 1), "{got:#?}");
        let warn = &warns[0].1;
        assert!(warn.starts_with("окно не отвечает 0."), "{warn}");
        assert!(warn.contains("«Висеть»"), "{warn}");
        // The thread stood in sleep — the stack has glibc frames.
        assert!(warn.contains("стек главного потока:\n#0 "), "{warn}");
        assert!(warn.contains("libc.so"), "{warn}");
        assert!(errors[0].1.starts_with("окно не отвечает уже "), "{}", errors[0].1);
        let info = &infos[0].1;
        assert!(info.starts_with("окно снова отвечает, висело 2."), "{info}");
    }
}
