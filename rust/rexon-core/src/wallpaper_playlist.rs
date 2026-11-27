//! Playlist of Rexon's own wallpapers: one rotator for all our kinds.
//!
//! WHAT IS INCLUDED. Video, web pages and Flash (the `rexon-wallpaper-host`
//! host), pictures and GIF (including those downloaded in «HD Wallpapers»
//! from the KDE Store and Wallhaven) and Razer Axon library wallpapers. Kinds
//! can be mixed in one list. Wallpaper Engine wallpapers (scenes, WE web,
//! application wallpapers) are NOT included: they are shown by the waywallen
//! daemon with its own playlists, and a second owner for them would be a
//! source of a "who is on screen now" war.
//!
//! HOW IT WORKS.
//! * The setting is the [`SETTINGS_KEY`] key in `~/.config/rexon/settings.json`
//!   (the window writes it). No key or a corrupted one — an empty disabled
//!   playlist; unfamiliar fields and kinds are skipped rather than breaking
//!   the parse (backward compatibility both ways).
//! * The live state (what is shown, the rest of the shuffle round, history,
//!   pause, how much time has ticked) — `~/.local/state/rexon/wallpaper-playlist.json`.
//!   Only this module writes it, the window does not touch it.
//! * Decisions are pure functions over (setting, state, time): [`choose`],
//!   [`tick`], [`login_due`], [`taken_over`]. The executor is [`Applier`]:
//!   live, [`LiveApplier`] applies by THE SAME path as the «Apply» button
//!   (`wallpaper_exec::run_in_slot` → `apply_with_audio`; rule #12), in the
//!   checks — a fake one.
//! * The timer lives in the long-lived Rexon process (the main window with
//!   the tray icon, which is also started at login with `--tray`): [`spawn_runner`].
//!
//! WHO OWNS THE SCREEN. One owner of the "what is on screen now" decision:
//! * while the playlist is enabled and not empty ([`owns_wallpaper`]), the
//!   wallpaper pair of the «Light and dark» block (`day_night`) does NOT change
//!   the wallpaper — it still switches themes and schemes;
//! * the person set another wallpaper themselves (with any «Apply» button in
//!   this same process or a Wallpaper Engine wallpaper) — the playlist pauses
//!   and says so, rather than overriding the person's choice N minutes later;
//! * the wallpaper is auto-paused by a condition (a full-screen window, the
//!   lock, the battery, GameMode — the shared `wallpaper_audio` model) — the
//!   playlist timer stands still: there is no point changing an invisible
//!   wallpaper.
//!
//! MONITORS. The playlist is bound to a [`Target`]: currently only
//! [`Target::All`] («on all screens») is executed. The "one monitor" target
//! is already in the model and in the store; when the per-monitor wallpaper
//! core provides applying to one screen, it is connected with one edit in
//! [`LiveApplier::apply`] (the `Screen` branch).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::i18n::{n_, ntr, tr, trf};

/// The setting key in `settings.json`.
pub const SETTINGS_KEY: &str = "wallpaper_playlist";
/// The live state file in `$XDG_STATE_HOME/rexon/`.
pub const STATE_FILE: &str = "wallpaper-playlist.json";
/// The setting format version.
pub const FORMAT_VERSION: u64 = 1;
/// The executor's timer step, s.
pub const TICK_SECS: u64 = 15;
/// How many seconds one tick can "tick" at most. More means the machine slept
/// or the process stood still: sleep must not count as showing.
pub const MAX_TICK_GAP: i64 = 120;
/// How many past wallpapers «Previous» remembers.
pub const HISTORY_LEN: usize = 50;
/// Default interval, min.
pub const DEFAULT_INTERVAL_MIN: u32 = 30;
/// Interval options for the drop-down list, min (0 — manual only).
pub const INTERVAL_CHOICES: &[u32] = &[0, 1, 5, 10, 15, 30, 60, 120, 180, 360, 720, 1440];

// ============================================================================
// Model
// ============================================================================

/// What to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A file of any of our kinds: `format` — the shared executor's format
    /// (`static`, `animated`, `video`, `web`, `swf`). A directory with `static` is
    /// an «HD Wallpapers» pack: the «Appearance» core chooses the picture from it.
    File { path: String, format: String },
    /// A Razer Axon library wallpaper by number; the file is prepared when shown.
    Axon { id: String },
}

impl Source {
    /// The item key: "current", history and the shuffle round are looked up by it
    /// — it survives reordering and removal of neighbors.
    pub fn key(&self) -> String {
        match self {
            Source::File { path, .. } => format!("file:{path}"),
            Source::Axon { id } => format!("axon:{id}"),
        }
    }
}

/// A playlist item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub source: Source,
    /// Human-readable name (empty — the file name).
    pub name: String,
}

impl Item {
    pub fn key(&self) -> String {
        self.source.key()
    }

    /// Name for the list.
    pub fn title(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.trim().to_string();
        }
        match &self.source {
            Source::File { path, .. } => Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone()),
            Source::Axon { id } => format!("Razer Axon {id}"),
        }
    }
}

/// Change order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// In list order.
    InOrder,
    /// Shuffle: each wallpaper once per round, a new round does not start with
    /// the ones just shown.
    #[default]
    Shuffle,
    /// Random: any each time, except the current one.
    Random,
}

impl Order {
    pub const ALL: [Order; 3] = [Order::InOrder, Order::Shuffle, Order::Random];

    pub fn key(self) -> &'static str {
        match self {
            Order::InOrder => "in_order",
            Order::Shuffle => "shuffle",
            Order::Random => "random",
        }
    }

    /// An unfamiliar value is the default, not a parse error.
    pub fn from_key(key: &str) -> Order {
        Order::ALL.into_iter().find(|o| o.key() == key).unwrap_or_default()
    }

    pub fn label(self) -> String {
        tr(match self {
            // TRANSLATORS: wallpaper playlist order: play the list from top to bottom.
            Order::InOrder => n_("In order"),
            // TRANSLATORS: wallpaper playlist order: every wallpaper once per round, in random order.
            Order::Shuffle => n_("Shuffle"),
            // TRANSLATORS: wallpaper playlist order: any wallpaper each time (repeats possible).
            Order::Random => n_("Random"),
        })
    }
}

/// Which screen the playlist plays on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Target {
    /// One wallpaper on all screens.
    #[default]
    All,
    /// One monitor — the monitor key of the "per-monitor wallpaper" core. For now
    /// the executor applies only [`Target::All`].
    Screen { key: String },
}

impl Target {
    pub fn key(&self) -> String {
        match self {
            Target::All => "all".into(),
            Target::Screen { key } => format!("screen:{key}"),
        }
    }

    fn from_value(v: Option<&Value>) -> Target {
        match v.and_then(Value::as_str) {
            Some(s) if s.starts_with("screen:") && s.len() > 7 => Target::Screen { key: s[7..].to_string() },
            _ => Target::All,
        }
    }
}

/// One playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    /// The playlist plays (a disabled one does not touch the screen at all).
    pub enabled: bool,
    pub items: Vec<Item>,
    pub order: Order,
    /// After how many minutes to change; 0 — manual only.
    pub interval_min: u32,
    /// Change the wallpaper on every login.
    pub on_login: bool,
    pub target: Target,
}

impl Default for Playlist {
    fn default() -> Self {
        Playlist {
            enabled: false,
            items: Vec::new(),
            order: Order::default(),
            interval_min: DEFAULT_INTERVAL_MIN,
            on_login: false,
            target: Target::All,
        }
    }
}

impl Playlist {
    pub fn keys(&self) -> Vec<String> {
        self.items.iter().map(Item::key).collect()
    }

    pub fn find(&self, key: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.key() == key)
    }

    /// Whether it plays: enabled and there is something to show.
    pub fn active(&self) -> bool {
        self.enabled && !self.items.is_empty()
    }

    /// Add if there is no such item yet. `true` — added.
    pub fn add(&mut self, item: Item) -> bool {
        if self.find(&item.key()).is_some() {
            return false;
        }
        self.items.push(item);
        true
    }
}

/// The whole setting: playlists by target (today — one, «on all screens»).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub playlists: Vec<Playlist>,
}

impl Default for Config {
    fn default() -> Self {
        Config { playlists: vec![Playlist::default()] }
    }
}

fn loose_str(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or_default().to_string()
}

fn item_from_value(v: &Value) -> Option<Item> {
    let src = v.get("source")?;
    let source = match src.get("kind").and_then(Value::as_str)? {
        "file" => {
            let path = loose_str(src.get("path"));
            let format = loose_str(src.get("format"));
            if path.is_empty() || !is_own_format(&format) {
                return None;
            }
            Source::File { path, format }
        }
        "axon" => {
            let id = loose_str(src.get("id"));
            if id.is_empty() {
                return None;
            }
            Source::Axon { id }
        }
        _ => return None,
    };
    Some(Item { source, name: loose_str(v.get("name")) })
}

fn item_to_value(i: &Item) -> Value {
    let source = match &i.source {
        Source::File { path, format } => json!({ "kind": "file", "path": path, "format": format }),
        Source::Axon { id } => json!({ "kind": "axon", "id": id }),
    };
    json!({ "source": source, "name": i.name })
}

impl Playlist {
    fn from_value(v: &Value) -> Playlist {
        let d = Playlist::default();
        let mut pl = Playlist {
            enabled: v.get("enabled").and_then(Value::as_bool).unwrap_or(d.enabled),
            items: Vec::new(),
            order: v.get("order").and_then(Value::as_str).map(Order::from_key).unwrap_or(d.order),
            interval_min: v
                .get("interval_min")
                .and_then(Value::as_u64)
                .map(|m| m.min(10 * 24 * 60) as u32)
                .unwrap_or(d.interval_min),
            on_login: v.get("on_login").and_then(Value::as_bool).unwrap_or(d.on_login),
            target: Target::from_value(v.get("target")),
        };
        for it in v.get("items").and_then(Value::as_array).into_iter().flatten() {
            if let Some(item) = item_from_value(it) {
                pl.add(item);
            }
        }
        pl
    }

    fn to_value(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "order": self.order.key(),
            "interval_min": self.interval_min,
            "on_login": self.on_login,
            "target": self.target.key(),
            "items": self.items.iter().map(item_to_value).collect::<Vec<_>>(),
        })
    }
}

impl Config {
    /// From Rexon's settings. No key or it is not an object — the default.
    pub fn from_settings(settings: &Map<String, Value>) -> Config {
        let Some(v) = settings.get(SETTINGS_KEY).filter(|v| v.is_object()) else {
            return Config::default();
        };
        let mut playlists: Vec<Playlist> = Vec::new();
        for p in v.get("playlists").and_then(Value::as_array).into_iter().flatten() {
            let pl = Playlist::from_value(p);
            // One target — one playlist: a repeated target is dropped.
            if !playlists.iter().any(|x| x.target == pl.target) {
                playlists.push(pl);
            }
        }
        if !playlists.iter().any(|p| p.target == Target::All) {
            playlists.insert(0, Playlist::default());
        }
        Config { playlists }
    }

    pub fn to_value(&self) -> Value {
        json!({
            "version": FORMAT_VERSION,
            "playlists": self.playlists.iter().map(Playlist::to_value).collect::<Vec<_>>(),
        })
    }

    /// The «on all screens» playlist (it always exists).
    pub fn main(&self) -> &Playlist {
        self.playlists.iter().find(|p| p.target == Target::All).expect("плейлист «все экраны» есть всегда")
    }

    pub fn main_mut(&mut self) -> &mut Playlist {
        if !self.playlists.iter().any(|p| p.target == Target::All) {
            self.playlists.insert(0, Playlist::default());
        }
        self.playlists.iter_mut().find(|p| p.target == Target::All).expect("только что добавлен")
    }
}

/// The playlist holds the desktop wallpaper: other schedules (the «Light and
/// dark» wallpaper pair) must not touch it.
pub fn owns_wallpaper(settings: &Map<String, Value>) -> bool {
    Config::from_settings(settings).main().active()
}

// ============================================================================
// Wallpaper kinds
// ============================================================================

/// Whether this is our format: only what Rexon shows itself. Scenes and other
/// Wallpaper Engine content belong to the daemon and its playlists.
pub fn is_own_format(fmt: &str) -> bool {
    matches!(fmt, "static" | "animated" | "video" | "web" | "swf")
}

/// The file format by extension; `None` — the playlist does not take such files.
pub fn file_format(path: &Path) -> Option<&'static str> {
    if path.is_dir() {
        return Some("static");
    }
    if let Some(f) = crate::axon_library::apply_format(path) {
        return Some(f);
    }
    match path.extension()?.to_string_lossy().to_ascii_lowercase().as_str() {
        "swf" => Some("swf"),
        "html" | "htm" => Some("web"),
        _ => None,
    }
}

// ============================================================================
// Live state
// ============================================================================

/// The state of one target.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TargetState {
    /// Key of the wallpaper shown now (empty — the playlist has not set anything yet).
    pub current: String,
    /// The rest of the shuffle round in showing order.
    pub round: Vec<String>,
    /// Past wallpapers, the latest at the end.
    pub history: Vec<String>,
    /// How many seconds of showing have ticked since the last change.
    pub elapsed: u64,
    /// The playlist pause (by the button or because the person set another wallpaper).
    pub paused: bool,
    /// Paused because another wallpaper took the screen: «Resume» sets the next
    /// one right away.
    pub yielded: bool,
    /// The path the playlist last handed to the executor.
    pub applied_path: String,
    /// The session in which the playlist last looked at a login.
    pub session: String,
    /// Time of the previous tick (unix, s).
    pub last_tick: i64,
    /// Random number generator state (0 — not seeded yet).
    pub rng: u64,
}

/// The state of all targets.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub targets: BTreeMap<String, TargetState>,
}

impl State {
    pub fn target_mut(&mut self, target: &Target) -> &mut TargetState {
        self.targets.entry(target.key()).or_default()
    }

    pub fn target(&self, target: &Target) -> TargetState {
        self.targets.get(&target.key()).cloned().unwrap_or_default()
    }
}

// ── random numbers: our own, seeded — so the checks are deterministic ──

/// SplitMix64: a generator step.
fn next_rand(st: &mut TargetState) -> u64 {
    if st.rng == 0 {
        st.rng = 0x9E37_79B9_7F4A_7C15;
    }
    st.rng = st.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = st.rng;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Seed if not seeded yet.
pub fn seed(st: &mut TargetState, seed: u64) {
    if st.rng == 0 {
        st.rng = seed.max(1);
    }
}

fn below(st: &mut TargetState, n: usize) -> usize {
    (next_rand(st) % n as u64) as usize
}

/// Fisher–Yates shuffle.
fn shuffled(st: &mut TargetState, keys: &[String]) -> Vec<String> {
    let mut out = keys.to_vec();
    for i in (1..out.len()).rev() {
        let j = below(st, i + 1);
        out.swap(i, j);
    }
    out
}

// ============================================================================
// Decisions
// ============================================================================

/// Direction of a manual change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Next,
    Prev,
}

/// Choose the next (previous) wallpaper and record the choice in the state.
/// The answer is the key of the new "current"; `None` — nothing to choose
/// from (an empty list, «Previous» without history). Any change resets the
/// timer countdown: a manual change is a "kick", the full interval starts over.
pub fn choose(pl: &Playlist, st: &mut TargetState, step: Step) -> Option<String> {
    let keys = pl.keys();
    let n = keys.len();
    if n == 0 {
        return None;
    }
    let pos = keys.iter().position(|k| *k == st.current);
    let picked = match (step, pl.order) {
        (Step::Next, Order::InOrder) => keys[pos.map(|p| (p + 1) % n).unwrap_or(0)].clone(),
        (Step::Prev, Order::InOrder) => keys[pos.map(|p| (p + n - 1) % n).unwrap_or(n - 1)].clone(),
        (Step::Next, Order::Shuffle) => {
            st.round.retain(|k| keys.contains(k) && *k != st.current);
            if st.round.is_empty() {
                let mut round = shuffled(st, &keys);
                // A new round does not start with the ones just shown.
                if n > 1 && round[0] == st.current {
                    round.swap(0, n - 1);
                }
                st.round = round;
            }
            st.round.remove(0)
        }
        (Step::Next, Order::Random) => {
            if n == 1 {
                keys[0].clone()
            } else {
                let others: Vec<&String> = keys.iter().filter(|k| **k != st.current).collect();
                let i = below(st, others.len());
                others[i].clone()
            }
        }
        (Step::Prev, Order::Shuffle | Order::Random) => {
            let back = loop {
                match st.history.pop() {
                    Some(k) if keys.contains(&k) => break Some(k),
                    Some(_) => continue,
                    None => break None,
                }
            }?;
            // Went back — the current one is first in the rest of the round again, and
            // «Next» after «Previous» brings it back.
            if pl.order == Order::Shuffle && pos.is_some() {
                let cur = st.current.clone();
                st.round.retain(|k| *k != cur);
                st.round.insert(0, cur);
            }
            st.current = back.clone();
            st.elapsed = 0;
            return Some(back);
        }
    };
    if step == Step::Next && pos.is_some() && st.current != picked {
        st.history.push(st.current.clone());
        let extra = st.history.len().saturating_sub(HISTORY_LEN);
        st.history.drain(..extra);
    }
    st.current = picked.clone();
    st.elapsed = 0;
    Some(picked)
}

/// A timer tick. `held` — the wallpaper is auto-paused by a condition (then
/// the timer stands still too). The answer is the key of the wallpaper due to be set.
pub fn tick(pl: &Playlist, st: &mut TargetState, now: i64, held: bool) -> Option<String> {
    let gap = if st.last_tick == 0 { 0 } else { (now - st.last_tick).clamp(0, MAX_TICK_GAP) };
    st.last_tick = now;
    if !pl.active() || st.paused || held || pl.interval_min == 0 {
        return None;
    }
    st.elapsed += gap as u64;
    if st.elapsed < u64::from(pl.interval_min) * 60 {
        return None;
    }
    choose(pl, st, Step::Next)
}

/// The first look in a new session: whether it is time to change the
/// wallpaper "on login". The session is remembered in any case — restarting
/// the window in the same session does not change the wallpaper.
pub fn login_due(pl: &Playlist, st: &mut TargetState, session: &str) -> bool {
    if session.is_empty() || st.session == session {
        return false;
    }
    st.session = session.to_string();
    pl.active() && pl.on_login && !st.paused
}

/// Another wallpaper took the screen: this process's executor holds something
/// other than what the playlist set (`on_screen` — its current path, `None` —
/// the executor knows nothing), or Wallpaper Engine wallpapers are alive
/// (`we_running`; our wallpapers stop WE when shown, so WE was set after us).
pub fn taken_over(st: &TargetState, on_screen: Option<&str>, we_running: bool) -> bool {
    if st.applied_path.is_empty() {
        return false;
    }
    we_running || on_screen.is_some_and(|p| p != st.applied_path)
}

/// Toggle the pause. The answer is whether to set the next one right away
/// (resuming after another wallpaper took the screen).
pub fn toggle_pause(st: &mut TargetState) -> bool {
    if st.paused {
        st.paused = false;
        let claim = st.yielded;
        st.yielded = false;
        st.elapsed = 0;
        claim
    } else {
        st.paused = true;
        false
    }
}

// ============================================================================
// Importing the Razer Axon queue (reading Axon files only)
// ============================================================================

/// Move the Axon queue into the playlist: wallpapers of the screen selected
/// in Axon (otherwise the first non-empty one) are appended at the end
/// without repeats. Order and timer are taken from Axon only if the playlist
/// was empty — the import does not overwrite the person's own settings. The
/// answer is how many were added.
pub fn import_axon(
    pl: &mut Playlist,
    setting: &crate::axon_library::PlayListSetting,
    title_of: &dyn Fn(&str) -> Option<String>,
) -> usize {
    let queue = setting
        .playlists
        .iter()
        .find(|p| p.selected && !p.wallpaper_ids.is_empty())
        .or_else(|| setting.playlists.iter().find(|p| !p.wallpaper_ids.is_empty()));
    let Some(queue) = queue else { return 0 };
    let was_empty = pl.items.is_empty();
    let mut added = 0;
    for id in &queue.wallpaper_ids {
        // Only what is in the library: wallpapers deleted in Axon would leave an
        // eternal showing error in the playlist.
        let Some(name) = title_of(id) else { continue };
        if pl.add(Item { source: Source::Axon { id: id.clone() }, name }) {
            added += 1;
        }
    }
    if was_empty && added > 0 {
        pl.order = match setting.play_order {
            crate::axon_library::PlayOrder::Random => Order::Shuffle,
            crate::axon_library::PlayOrder::Ordered => Order::InOrder,
        };
        pl.interval_min = if setting.switch_interval_seconds <= 0 {
            0
        } else {
            ((setting.switch_interval_seconds + 59) / 60) as u32
        };
    }
    added
}

// ============================================================================
// Execution
// ============================================================================

/// What to set wallpapers with and what to ask the machine.
pub trait Applier {
    /// Set a wallpaper; `Ok` — the path handed to the wallpaper executor.
    fn apply(&mut self, target: &Target, item: &Item) -> Result<String, String>;
    /// Path of what is set now according to this process's executor.
    fn on_screen(&mut self) -> Option<String>;
    /// Whether Wallpaper Engine wallpapers are alive.
    fn we_running(&mut self) -> bool;
}

/// What the executor step did — for the journal and the notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Applied { key: String, path: String },
    /// Not a single wallpaper from the list got set (each attempt is a line).
    Failed(Vec<String>),
    /// Another wallpaper took the screen — the playlist paused.
    Yielded,
}

/// Set the chosen one; if it did not get set — try the next ones, but no more
/// than a round (otherwise a broken list would spin forever).
fn apply_from(pl: &Playlist, st: &mut TargetState, first: String, ap: &mut dyn Applier) -> Outcome {
    let mut key = first;
    let mut errors = Vec::new();
    for _ in 0..pl.items.len() {
        let Some(item) = pl.find(&key) else { break };
        match ap.apply(&pl.target, item) {
            Ok(path) => {
                st.applied_path = path.clone();
                return Outcome::Applied { key, path };
            }
            Err(e) => errors.push(format!("{}: {e}", item.title())),
        }
        match choose(pl, st, Step::Next) {
            Some(k) => key = k,
            None => break,
        }
    }
    Outcome::Failed(errors)
}

/// What was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// A timer tick (`held` — auto-pause by a condition).
    Tick { held: bool },
    /// The first tick of the process: check "on login" in session `session`.
    Start,
    Next,
    Prev,
    TogglePause,
}

/// One executor step over the playlist and its state.
pub fn step(
    pl: &Playlist,
    st: &mut TargetState,
    cmd: Command,
    now: i64,
    session: &str,
    ap: &mut dyn Applier,
) -> Outcome {
    seed(st, now as u64 ^ 0x5DEE_CE66_D);
    if !pl.enabled {
        st.last_tick = now;
        return Outcome::Nothing;
    }
    // The screen was taken — pause until the person asks to resume.
    if !st.paused && matches!(cmd, Command::Tick { .. } | Command::Start) {
        let screen = ap.on_screen();
        if taken_over(st, screen.as_deref(), ap.we_running()) {
            st.paused = true;
            st.yielded = true;
            st.last_tick = now;
            return Outcome::Yielded;
        }
    }
    let pick = match cmd {
        Command::Tick { held } => tick(pl, st, now, held),
        Command::Start => {
            let due = login_due(pl, st, session);
            st.last_tick = now;
            if due {
                choose(pl, st, Step::Next)
            } else {
                None
            }
        }
        Command::Next => {
            // A manual change during a yield pause — the person brought the playlist back.
            if st.yielded {
                st.paused = false;
                st.yielded = false;
            }
            choose(pl, st, Step::Next)
        }
        Command::Prev => {
            if st.yielded {
                st.paused = false;
                st.yielded = false;
            }
            choose(pl, st, Step::Prev)
        }
        Command::TogglePause => {
            if toggle_pause(st) {
                choose(pl, st, Step::Next)
            } else {
                None
            }
        }
    };
    match pick {
        Some(key) => apply_from(pl, st, key, ap),
        None => Outcome::Nothing,
    }
}

// ============================================================================
// Live executor
// ============================================================================

/// The real machine: the same process wallpaper executor as the «Apply»
/// buttons in all tabs.
pub struct LiveApplier;

fn screen_size_live() -> (u64, u64) {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let session = crate::wallpaper_engine::detect_session_type(&env("XDG_SESSION_TYPE"), &env("WAYLAND_DISPLAY"), &env("DISPLAY"));
    let outputs = crate::wallpaper_exec::live_screen_outputs(&session);
    crate::wallpaper_exec::largest_output(&outputs).and_then(|o| o.size()).unwrap_or((0, 0))
}

fn run_exec(path: String, format: String, name: String, audio: bool) -> Result<String, String> {
    use crate::wallpaper_exec::{live_slot, run_in_slot, LiveSys, WallpaperExec};
    let shown = path.clone();
    let res = run_in_slot(&live_slot(), move |exec: &mut WallpaperExec<LiveSys>| {
        exec.apply_with_audio(&path, &format, &name, audio)
    });
    if res.ok {
        Ok(shown)
    } else {
        Err(res.message)
    }
}

impl Applier for LiveApplier {
    fn apply(&mut self, target: &Target, item: &Item) -> Result<String, String> {
        // ⏳ Per-monitor wallpapers: applying to one screen goes right here, when the
        // per-monitor assignment core provides it.
        if !matches!(target, Target::All) {
            return Err(tr("Wallpapers for a single screen are not supported yet."));
        }
        match &item.source {
            Source::File { path, format } => {
                let p = Path::new(path);
                if p.is_dir() {
                    let snap = crate::appearance_installer::install_tree_snapshot(path, &[]);
                    let images = crate::appearance_installer::pack_images(&snap, path);
                    let (w, h) = screen_size_live();
                    let chosen = crate::appearance_installer::pick_pack_image(&images, w, h)
                        .ok_or_else(|| trf("Wallpaper file not found: %1", &[&path]))?;
                    return run_exec(chosen, "static".into(), String::new(), false);
                }
                if !p.exists() && format != "web" {
                    return Err(trf("Wallpaper file not found: %1", &[&path]));
                }
                // Video sound — as with «Apply» without a choice: the shared wallpaper
                // sound settings decide by themselves.
                run_exec(path.clone(), format.clone(), String::new(), true)
            }
            Source::Axon { id } => {
                let env = crate::axon_prepare::live_env().0;
                let lib = crate::axon_library::load_library(&env);
                let w = lib.get(id).cloned().ok_or_else(|| crate::axon_library::prepare_failed_text(id))?;
                let extractor = crate::axon_library::default_extractor();
                let ready = crate::axon_prepare::prepare_for_apply(&w, Some(extractor.as_ref()), &env)?;
                run_exec(ready.path.to_string_lossy().into_owned(), ready.format.to_string(), ready.name, ready.audio)
            }
        }
    }

    fn on_screen(&mut self) -> Option<String> {
        use crate::wallpaper_exec::{live_slot, try_with_slot, LiveSys, WallpaperExec};
        try_with_slot(&live_slot(), |exec: &mut WallpaperExec<LiveSys>| exec.current_path().map(str::to_string)).flatten()
    }

    fn we_running(&mut self) -> bool {
        crate::renderer_watch::renderers_running()
    }
}

/// Paths of one user.
fn settings_path() -> std::path::PathBuf {
    crate::settings::settings_file()
}

fn state_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_STATE_HOME").ok().filter(|v| !v.is_empty());
    Path::new(&crate::utils::state_home(xdg.as_deref(), &home)).join("rexon").join(STATE_FILE)
}

pub fn load_config() -> Config {
    Config::from_settings(&crate::settings::load_settings(&settings_path()))
}

pub fn load_state() -> State {
    std::fs::read_to_string(state_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_state(st: &State) {
    let path = state_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = serde_json::to_string_pretty(st).unwrap_or_default();
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Write the setting (the window).
pub fn save_config(cfg: &Config) -> Result<(), String> {
    let mut m = crate::settings::SettingsManager::new().map_err(|e| e.to_string())?;
    m.set(SETTINGS_KEY, cfg.to_value()).map_err(|e| e.to_string())
}

/// The login session: boot ID + logind session (a restarted window in the
/// same session has the same one).
pub fn live_session() -> String {
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap_or_default();
    let session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
    format!("{}/{}", boot.trim(), session.trim())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// One executor per process: the timer and the buttons must not apply at
/// the same time.
fn run_lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

/// The auto-pause poll: `active` — the playlist needs an answer (otherwise the
/// watcher can be turned off); the answer — the wallpaper is paused by a condition.
pub type HoldProbe = Box<dyn FnMut(bool) -> bool + Send>;

fn hold_probe() -> &'static Mutex<Option<HoldProbe>> {
    static PROBE: Mutex<Option<HoldProbe>> = Mutex::new(None);
    &PROBE
}

/// Install the auto-pause poll (the Qt window — that is where the shared
/// `wallpaper_audio` watcher lives).
pub fn set_hold_probe(probe: HoldProbe) {
    if let Ok(mut slot) = hold_probe().lock() {
        *slot = Some(probe);
    }
}

fn held_now(active: bool) -> bool {
    hold_probe()
        .lock()
        .ok()
        .and_then(|mut slot| slot.as_mut().map(|p| p(active)))
        .unwrap_or(false)
}

/// A live step: read the setting and the state, decide, set, save. For the
/// timer and for the buttons (window, tray) — call outside the GUI thread.
pub fn live_command(cmd: Command) -> Outcome {
    let _guard = run_lock().lock().unwrap_or_else(|e| e.into_inner());
    let cfg = load_config();
    let mut state = load_state();
    let before = state.clone();
    let pl = cfg.main();
    let cmd = match cmd {
        Command::Tick { .. } => {
            let st = state.target(&pl.target);
            let counting = pl.active() && !st.paused && pl.interval_min > 0;
            Command::Tick { held: held_now(counting) }
        }
        other => other,
    };
    let st = state.target_mut(&pl.target);
    let out = step(pl, st, cmd, now_secs(), &live_session(), &mut LiveApplier);
    match &out {
        Outcome::Applied { key, path } => crate::log_info!("wallpaper", "плейлист: поставлены {key} ({path})"),
        Outcome::Failed(errs) => crate::log_warn!("wallpaper", "плейлист: обои не встали: {}", errs.join("; ")),
        Outcome::Yielded => {
            crate::log_info!("wallpaper", "плейлист: экран заняли другие обои — пауза");
            crate::utils::notify_live(&notice_title(), &yielded_text());
        }
        Outcome::Nothing => {}
    }
    // A tick mark by itself is no reason to write the file every 15 s.
    let changed = {
        let mut a = state.clone();
        let mut b = before;
        for t in a.targets.values_mut().chain(b.targets.values_mut()) {
            t.last_tick = 0;
            t.elapsed = t.elapsed / 60;
        }
        a != b
    };
    if changed || !matches!(cmd, Command::Tick { .. }) {
        save_state(&state);
    }
    out
}

/// Start the playlist timer once per process.
pub fn spawn_runner() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new().name("wallpaper-playlist".into()).spawn(|| {
            live_command(Command::Start);
            loop {
                std::thread::sleep(std::time::Duration::from_secs(TICK_SECS));
                live_command(Command::Tick { held: false });
            }
        });
        if let Err(e) = spawned {
            crate::log_warn!("wallpaper", "таймер плейлиста не запущен: {e}");
        }
    });
}

/// What to show in the tray: (the playlist plays, paused).
pub fn tray_state() -> (bool, bool) {
    let cfg = load_config();
    let pl = cfg.main();
    (pl.active(), load_state().target(&pl.target).paused)
}

// ============================================================================
// Texts
// ============================================================================

/// Tooltip of the «⚙️ Settings» button of the «HD Wallpapers» section.
pub fn settings_tooltip() -> String {
    tr("A playlist of Rexon's own wallpapers: change them by a timer, at login or by hand.")
}

pub fn notice_title() -> String {
    tr("Wallpaper playlist")
}

pub fn yielded_text() -> String {
    tr("The playlist is paused because you set other wallpapers. Choose “Resume Playlist” in the Rexon tray icon menu to continue.")
}

/// Interval label.
pub fn interval_label(min: u32) -> String {
    match min {
        // TRANSLATORS: wallpaper playlist interval: change only by hand (tray or buttons).
        0 => tr("Only by hand"),
        m if m % 60 == 0 => ntr("Every %n hour", "Every %n hours", u64::from(m / 60)),
        m => ntr("Every %n minute", "Every %n minutes", u64::from(m)),
    }
}

/// Item kind for the person.
pub fn kind_label(item: &Item) -> String {
    match &item.source {
        Source::Axon { .. } => "Razer Axon".to_string(),
        Source::File { format, .. } => crate::wallpaper_engine::format_title(format),
    }
}

/// Labels of the settings block.
pub fn texts() -> Value {
    json!({
        "title": tr("Wallpaper playlist"),
        "description": tr("Change Rexon's own wallpapers by themselves: videos, web pages, Flash, pictures from “HD Wallpapers” and Razer Axon wallpapers — mixed in one list. Wallpaper Engine wallpapers keep their own playlist in the Wallpaper Engine tab."),
        "enabled": tr("Play the playlist"),
        "order": tr("Order"),
        "interval": tr("Change"),
        "onLogin": tr("New wallpaper at every login"),
        "addFiles": tr("Add files…"),
        "addFilesTitle": tr("Choose wallpapers for the playlist"),
        "addHd": tr("Add all “HD Wallpapers”"),
        "importAxon": tr("Import the Razer Axon queue"),
        "importAxonTooltip": tr("Adds the wallpapers of the Razer Axon queue. Razer Axon's own files are only read, never changed."),
        "next": tr("Next Wallpaper"),
        "prev": tr("Previous Wallpaper"),
        "pause": tr("Pause Playlist"),
        "resume": tr("Resume Playlist"),
        "remove": tr("Remove from the playlist"),
        "up": tr("Move up"),
        "down": tr("Move down"),
        "addToPlaylist": tr("Add to the playlist"),
        "clear": tr("Clear the list"),
        "clearConfirm": tr("Yes, clear the list"),
        "empty": tr("The playlist is empty. Add files, all “HD Wallpapers” or the Razer Axon queue."),
        "dayNightNote": tr("While the playlist plays, the “Light and dark” schedule does not change the wallpaper (themes and colours still switch)."),
    })
}

/// The status line under the heading.
fn status_line(pl: &Playlist, st: &TargetState) -> String {
    if pl.items.is_empty() {
        return String::new();
    }
    if !pl.enabled {
        return ntr("%n wallpaper in the list. The playlist is off.", "%n wallpapers in the list. The playlist is off.", pl.items.len() as u64);
    }
    if st.yielded {
        return tr("Paused: other wallpapers were set.");
    }
    if st.paused {
        return tr("Paused.");
    }
    let now = pl.find(&st.current).map(Item::title);
    let left = (u64::from(pl.interval_min) * 60).saturating_sub(st.elapsed).div_ceil(60);
    match (now, pl.interval_min) {
        (Some(name), 0) => trf("Now: %1.", &[&name]),
        (Some(name), _) => trf("Now: %1. Next in about %2 min.", &[&name, &left.max(1)]),
        (None, 0) => tr("Press “Next Wallpaper” to start."),
        (None, _) => trf("The first change in about %1 min.", &[&left.max(1)]),
    }
}

/// A snapshot of the block for the window.
pub fn view(cfg: &Config, state: &State, axon_queue: bool, day_night_pair: bool) -> Value {
    let pl = cfg.main();
    let st = state.target(&pl.target);
    json!({
        "enabled": pl.enabled,
        "order": pl.order.key(),
        "orders": Order::ALL.iter().map(|o| json!({"id": o.key(), "label": o.label()})).collect::<Vec<_>>(),
        "interval": pl.interval_min,
        "intervals": INTERVAL_CHOICES.iter().map(|m| json!({"id": m, "label": interval_label(*m)})).collect::<Vec<_>>(),
        "onLogin": pl.on_login,
        "paused": st.paused,
        "line": status_line(pl, &st),
        "axonQueue": axon_queue,
        "dayNightNote": pl.active() && day_night_pair,
        "items": pl.items.iter().map(|i| json!({
            "key": i.key(),
            "title": i.title(),
            "kind": kind_label(i),
            "current": i.key() == st.current,
        })).collect::<Vec<_>>(),
    })
}

/// A live snapshot of the block.
pub fn live_view() -> Value {
    let env = crate::axon_prepare::live_env().0;
    let lib = crate::axon_library::load_library(&env);
    let axon_queue = lib.play_setting.as_ref().is_some_and(|s| s.playlists.iter().any(|p| !p.wallpaper_ids.is_empty()));
    let dn = crate::day_night::read_config(&crate::day_night::SystemDayNightIo, &crate::day_night::Paths::live());
    let pair = dn.mode != crate::day_night::Mode::Off && dn.wallpaper.on;
    view(&load_config(), &load_state(), axon_queue, pair)
}

/// A setting edit from the window. `op` — `{"op": …}`; the answer is a string
/// for the person (empty — silently).
pub fn edit(cfg: &mut Config, op: &Value, axon: Option<&crate::axon_library::AxonLibrary>, hd_files: &[String]) -> String {
    let pl = cfg.main_mut();
    let s = |k: &str| op.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    match op.get("op").and_then(Value::as_str).unwrap_or_default() {
        "enabled" => pl.enabled = op.get("on").and_then(Value::as_bool).unwrap_or(false),
        "order" => pl.order = Order::from_key(&s("value")),
        "interval" => pl.interval_min = op.get("value").and_then(Value::as_u64).unwrap_or(0).min(10 * 24 * 60) as u32,
        "onLogin" => pl.on_login = op.get("on").and_then(Value::as_bool).unwrap_or(false),
        "remove" => pl.items.retain(|i| i.key() != s("key")),
        "clear" => pl.items.clear(),
        "move" => {
            let key = s("key");
            let delta = op.get("delta").and_then(Value::as_i64).unwrap_or(0);
            if let Some(i) = pl.items.iter().position(|x| x.key() == key) {
                let j = (i as i64 + delta).clamp(0, pl.items.len() as i64 - 1) as usize;
                let item = pl.items.remove(i);
                pl.items.insert(j, item);
            }
        }
        "addFiles" => {
            let mut added = 0usize;
            let mut skipped = 0usize;
            let paths: Vec<String> = op
                .get("paths")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(crate::local_install::file_url_to_path))
                .chain(hd_files.iter().cloned())
                .collect();
            for path in paths {
                match file_format(Path::new(&path)) {
                    Some(fmt) => {
                        if pl.add(Item { source: Source::File { path, format: fmt.into() }, name: String::new() }) {
                            added += 1;
                        }
                    }
                    None => skipped += 1,
                }
            }
            let mut msg = trf("Added to the playlist: %1.", &[&added]);
            if skipped > 0 {
                msg.push(' ');
                msg.push_str(&ntr("%n file is not a wallpaper and was skipped.", "%n files are not wallpapers and were skipped.", skipped as u64));
            }
            return msg;
        }
        "addAxon" => {
            let id = s("id");
            let Some(w) = axon.and_then(|l| l.get(&id)) else { return crate::axon_library::prepare_failed_text(&id) };
            let name = crate::axon_library::display_title(w);
            return if pl.add(Item { source: Source::Axon { id }, name: name.clone() }) {
                trf("“%1” added to the playlist.", &[&name])
            } else {
                trf("“%1” is already in the playlist.", &[&name])
            };
        }
        "importAxon" => {
            let Some(lib) = axon else { return tr("Razer Axon was not found.") };
            let Some(setting) = lib.play_setting.as_ref() else { return tr("The Razer Axon queue is empty.") };
            let added = import_axon(pl, setting, &|id| lib.get(id).map(crate::axon_library::display_title));
            return if added == 0 {
                tr("Nothing new in the Razer Axon queue.")
            } else {
                trf("Added to the playlist: %1.", &[&added])
            };
        }
        _ => {}
    }
    String::new()
}

/// «HD Wallpapers» pictures and packs (`~/.local/share/wallpapers`).
pub fn hd_files_live() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = Path::new(&home).join(".local/share/wallpapers");
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| file_format(p).is_some())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

/// A live edit from the window: write and answer `{"ok", "message"}`.
pub fn live_edit(op_json: &str) -> Value {
    let Ok(op) = serde_json::from_str::<Value>(op_json) else {
        return json!({"ok": false, "message": ""});
    };
    let kind = op.get("op").and_then(Value::as_str).unwrap_or_default().to_string();
    let axon = matches!(kind.as_str(), "addAxon" | "importAxon").then(|| {
        let env = crate::axon_prepare::live_env().0;
        crate::axon_library::load_library(&env)
    });
    let hd = if kind == "addHd" { hd_files_live() } else { Vec::new() };
    let op = if kind == "addHd" { json!({"op": "addFiles", "paths": []}) } else { op };
    let _guard = run_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut cfg = load_config();
    let message = edit(&mut cfg, &op, axon.as_ref().filter(|l| l.found), &hd);
    match save_config(&cfg) {
        Ok(()) => json!({"ok": true, "message": message}),
        Err(e) => json!({"ok": false, "message": e}),
    }
}

// ============================================================================
// Checks
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn file(n: &str) -> Item {
        Item { source: Source::File { path: format!("/w/{n}.mp4"), format: "video".into() }, name: n.into() }
    }

    fn pl(n: usize, order: Order) -> Playlist {
        Playlist {
            enabled: true,
            items: (0..n).map(|i| file(&format!("v{i}"))).collect(),
            order,
            interval_min: 5,
            on_login: true,
            target: Target::All,
        }
    }

    #[derive(Default)]
    struct Fake {
        applied: Vec<String>,
        fail: Vec<String>,
        screen: Option<String>,
        we: bool,
    }

    impl Applier for Fake {
        fn apply(&mut self, _t: &Target, item: &Item) -> Result<String, String> {
            if self.fail.contains(&item.key()) {
                return Err("нет файла".into());
            }
            self.applied.push(item.key());
            let Source::File { path, .. } = &item.source else { return Ok(item.key()) };
            self.screen = Some(path.clone());
            Ok(path.clone())
        }
        fn on_screen(&mut self) -> Option<String> {
            self.screen.clone()
        }
        fn we_running(&mut self) -> bool {
            self.we
        }
    }

    #[test]
    fn in_order_cycles_and_goes_back() {
        let p = pl(3, Order::InOrder);
        let mut st = TargetState::default();
        let got: Vec<String> = (0..4).map(|_| choose(&p, &mut st, Step::Next).unwrap()).collect();
        assert_eq!(got, ["file:/w/v0.mp4", "file:/w/v1.mp4", "file:/w/v2.mp4", "file:/w/v0.mp4"]);
        assert_eq!(choose(&p, &mut st, Step::Prev).unwrap(), "file:/w/v2.mp4");
    }

    #[test]
    fn shuffle_without_repeats_per_cycle_and_deterministic() {
        let p = pl(7, Order::Shuffle);
        let run = |seed_v: u64| {
            let mut st = TargetState::default();
            seed(&mut st, seed_v);
            (0..70).map(|_| choose(&p, &mut st, Step::Next).unwrap()).collect::<Vec<_>>()
        };
        let a = run(42);
        assert_eq!(a, run(42), "одно зерно — один порядок");
        assert_ne!(a, run(43), "другое зерно — другой порядок");
        for round in a.chunks(7) {
            let mut r = round.to_vec();
            r.sort();
            r.dedup();
            assert_eq!(r.len(), 7, "в круге повтор: {round:?}");
        }
        // The junction of rounds: the same wallpaper is not shown twice in a row.
        for w in a.windows(2) {
            assert_ne!(w[0], w[1]);
        }
    }

    #[test]
    fn shuffle_back_and_forward_again() {
        let p = pl(5, Order::Shuffle);
        let mut st = TargetState::default();
        seed(&mut st, 7);
        let a = choose(&p, &mut st, Step::Next).unwrap();
        let b = choose(&p, &mut st, Step::Next).unwrap();
        assert_eq!(choose(&p, &mut st, Step::Prev).unwrap(), a);
        assert_eq!(choose(&p, &mut st, Step::Next).unwrap(), b, "«Следующие» вернули те же");
    }

    #[test]
    fn random_does_not_repeat_current_in_a_row() {
        let p = pl(3, Order::Random);
        let mut st = TargetState::default();
        seed(&mut st, 1);
        let mut prev = String::new();
        for _ in 0..200 {
            let k = choose(&p, &mut st, Step::Next).unwrap();
            assert_ne!(k, prev);
            prev = k;
        }
        let one = pl(1, Order::Random);
        let mut st = TargetState::default();
        assert_eq!(choose(&one, &mut st, Step::Next).unwrap(), "file:/w/v0.mp4");
        assert_eq!(choose(&one, &mut st, Step::Next).unwrap(), "file:/w/v0.mp4");
    }

    #[test]
    fn empty_list_selects_nothing() {
        let p = pl(0, Order::Shuffle);
        let mut st = TargetState::default();
        assert_eq!(choose(&p, &mut st, Step::Next), None);
        assert_eq!(choose(&p, &mut st, Step::Prev), None);
    }

    #[test]
    fn timer_counts_only_display_time() {
        let p = pl(2, Order::InOrder); // 5 minutes
        let mut st = TargetState::default();
        let mut t = 1_000;
        assert_eq!(tick(&p, &mut st, t, false), None);
        // 2 minutes of showing — too early.
        for _ in 0..8 {
            t += 15;
            assert_eq!(tick(&p, &mut st, t, false), None);
        }
        // Auto-pause by a condition: an hour stands still, nothing changes.
        for _ in 0..240 {
            t += 15;
            assert_eq!(tick(&p, &mut st, t, true), None);
        }
        // Machine sleep: a time jump counts for no more than MAX_TICK_GAP.
        t += 3600;
        assert_eq!(tick(&p, &mut st, t, false), None, "сон засчитан за показ: {}", st.elapsed);
        // Made up the fifth minute (2 + 2 for the sleep + 1).
        let mut got = None;
        for _ in 0..4 {
            t += 15;
            got = got.or(tick(&p, &mut st, t, false));
        }
        assert!(got.is_some(), "смены не было: {}", st.elapsed);
        assert_eq!(st.elapsed, 0);
    }

    #[test]
    fn pause_and_zero_interval_hold_timer() {
        let mut p = pl(2, Order::InOrder);
        let mut st = TargetState { paused: true, last_tick: 1, ..Default::default() };
        assert_eq!(tick(&p, &mut st, 100_000, false), None);
        assert_eq!(st.elapsed, 0);
        st.paused = false;
        p.interval_min = 0;
        for i in 0..100 {
            assert_eq!(tick(&p, &mut st, 100_000 + i * 15, false), None);
        }
        p.enabled = false;
        p.interval_min = 1;
        for i in 0..100 {
            assert_eq!(tick(&p, &mut st, 200_000 + i * 15, false), None);
        }
    }

    #[test]
    fn manual_change_resets_countdown() {
        let p = pl(3, Order::InOrder);
        let mut st = TargetState { elapsed: 290, ..Default::default() };
        let mut ap = Fake::default();
        step(&p, &mut st, Command::Next, 10, "s", &mut ap);
        assert_eq!(st.elapsed, 0);
        assert_eq!(ap.applied, ["file:/w/v0.mp4"]);
    }

    #[test]
    fn login_changes_wallpaper_once_per_session() {
        let p = pl(3, Order::InOrder);
        let mut st = TargetState::default();
        let mut ap = Fake::default();
        assert!(matches!(step(&p, &mut st, Command::Start, 10, "boot/1", &mut ap), Outcome::Applied { .. }));
        assert!(matches!(step(&p, &mut st, Command::Start, 20, "boot/1", &mut ap), Outcome::Nothing));
        assert!(matches!(step(&p, &mut st, Command::Start, 30, "boot/2", &mut ap), Outcome::Applied { .. }));
        let mut off = p.clone();
        off.on_login = false;
        assert!(matches!(step(&off, &mut st, Command::Start, 40, "boot/3", &mut ap), Outcome::Nothing));
        assert_eq!(st.session, "boot/3", "сеанс помнится и без смены");
    }

    #[test]
    fn broken_wallpapers_skipped_but_not_forever() {
        let p = pl(3, Order::InOrder);
        let mut st = TargetState::default();
        let mut ap = Fake { fail: vec!["file:/w/v0.mp4".into()], ..Default::default() };
        let out = step(&p, &mut st, Command::Next, 1, "s", &mut ap);
        assert_eq!(out, Outcome::Applied { key: "file:/w/v1.mp4".into(), path: "/w/v1.mp4".into() });
        let mut ap = Fake { fail: p.keys(), ..Default::default() };
        assert!(matches!(step(&p, &mut st, Command::Next, 2, "s", &mut ap), Outcome::Failed(e) if e.len() == 3));
    }

    #[test]
    fn foreign_wallpaper_pauses_playlist_and_resume_restores() {
        let p = pl(3, Order::InOrder);
        let mut st = TargetState::default();
        let mut ap = Fake::default();
        step(&p, &mut st, Command::Next, 1, "s", &mut ap);
        // The person set another wallpaper with the same «Apply» button.
        ap.screen = Some("/home/u/своя.png".into());
        assert_eq!(step(&p, &mut st, Command::Tick { held: false }, 16, "s", &mut ap), Outcome::Yielded);
        assert!(st.paused && st.yielded);
        // The timer no longer overrides the person's choice.
        for i in 0..100 {
            assert_eq!(step(&p, &mut st, Command::Tick { held: false }, 31 + i * 15, "s", &mut ap), Outcome::Nothing);
        }
        // «Resume» sets the next one right away.
        assert!(matches!(step(&p, &mut st, Command::TogglePause, 9_000, "s", &mut ap), Outcome::Applied { .. }));
        assert!(!st.paused && !st.yielded);
        // A Wallpaper Engine wallpaper on top — also a yield.
        ap.we = true;
        assert_eq!(step(&p, &mut st, Command::Tick { held: false }, 9_015, "s", &mut ap), Outcome::Yielded);
    }

    #[test]
    fn button_pause_sets_no_wallpaper() {
        let p = pl(3, Order::InOrder);
        let mut st = TargetState::default();
        let mut ap = Fake::default();
        assert_eq!(step(&p, &mut st, Command::TogglePause, 1, "s", &mut ap), Outcome::Nothing);
        assert!(st.paused);
        assert_eq!(step(&p, &mut st, Command::TogglePause, 2, "s", &mut ap), Outcome::Nothing);
        assert!(!st.paused);
        assert!(ap.applied.is_empty());
    }

    #[test]
    fn setting_backward_compatible() {
        // No key — an empty disabled «on all screens» playlist.
        let cfg = Config::from_settings(&Map::new());
        assert_eq!(cfg, Config::default());
        assert!(!cfg.main().active());
        // A corrupted key — also the default.
        let mut m = Map::new();
        m.insert(SETTINGS_KEY.into(), json!("мусор"));
        assert_eq!(Config::from_settings(&m), Config::default());
        // A future version: unfamiliar fields, kinds and order do not break the parse.
        m.insert(
            SETTINGS_KEY.into(),
            json!({"version": 9, "новое": 1, "playlists": [{
                "enabled": true, "order": "по-новому", "interval_min": 15, "target": "all",
                "items": [
                    {"source": {"kind": "file", "path": "/w/a.mp4", "format": "video"}, "name": "A"},
                    {"source": {"kind": "голограмма", "id": "x"}},
                    {"source": {"kind": "file", "path": "/w/s.pkg", "format": "scene"}},
                    {"source": {"kind": "axon", "id": "123"}, "name": "Axon"},
                    {"source": {"kind": "file", "path": "/w/a.mp4", "format": "video"}}
                ]}]}),
        );
        let cfg = Config::from_settings(&m);
        let p = cfg.main();
        assert!(p.enabled);
        assert_eq!(p.order, Order::Shuffle);
        assert_eq!(p.interval_min, 15);
        assert_eq!(p.keys(), ["file:/w/a.mp4", "axon:123"], "сцены WE и повторы не берутся");
        // There and back — without losses.
        let mut back = Map::new();
        back.insert(SETTINGS_KEY.into(), cfg.to_value());
        assert_eq!(Config::from_settings(&back), cfg);
        assert!(owns_wallpaper(&back));
    }

    #[test]
    fn monitor_target_stored_next_to_shared() {
        let mut cfg = Config::default();
        cfg.playlists.push(Playlist { target: Target::Screen { key: "DEL-U2720-123".into() }, ..pl(1, Order::InOrder) });
        let mut m = Map::new();
        m.insert(SETTINGS_KEY.into(), cfg.to_value());
        let back = Config::from_settings(&m);
        assert_eq!(back.playlists.len(), 2);
        assert_eq!(back.playlists[1].target.key(), "screen:DEL-U2720-123");
        assert!(!back.main().active(), "плейлист монитора не делает общий активным");
        let mut st = State::default();
        st.target_mut(&back.playlists[1].target).current = "x".into();
        assert_eq!(st.target(&Target::All).current, "");
    }

    #[test]
    fn axon_queue_import() {
        use crate::axon_library::{PlayList as AxQ, PlayListSetting, PlayOrder};
        let setting = PlayListSetting {
            switch_interval_seconds: 600,
            play_order: PlayOrder::Random,
            playlists: vec![
                AxQ { monitor: "1".into(), wallpaper_ids: vec!["9".into()], ..Default::default() },
                AxQ { monitor: "2".into(), wallpaper_ids: vec!["1".into(), "2".into(), "удалён".into()], selected: true, ..Default::default() },
            ],
            ..Default::default()
        };
        let title = |id: &str| (id != "удалён").then(|| format!("Обои {id}"));
        let mut p = Playlist::default();
        assert_eq!(import_axon(&mut p, &setting, &title), 2);
        assert_eq!(p.keys(), ["axon:1", "axon:2"]);
        assert_eq!((p.order, p.interval_min), (Order::Shuffle, 10));
        // Importing again duplicates nothing and does not touch the person's own settings.
        p.order = Order::InOrder;
        assert_eq!(import_axon(&mut p, &setting, &title), 0);
        assert_eq!(p.order, Order::InOrder);
    }

    #[test]
    fn edits_from_window() {
        let mut cfg = Config::default();
        edit(&mut cfg, &json!({"op": "enabled", "on": true}), None, &[]);
        edit(&mut cfg, &json!({"op": "order", "value": "random"}), None, &[]);
        edit(&mut cfg, &json!({"op": "interval", "value": 60}), None, &[]);
        let p = cfg.main();
        assert!(p.enabled);
        assert_eq!((p.order, p.interval_min), (Order::Random, 60));
        cfg.main_mut().items = vec![file("a"), file("b"), file("c")];
        edit(&mut cfg, &json!({"op": "move", "key": "file:/w/c.mp4", "delta": -5}), None, &[]);
        assert_eq!(cfg.main().keys()[0], "file:/w/c.mp4");
        edit(&mut cfg, &json!({"op": "remove", "key": "file:/w/a.mp4"}), None, &[]);
        assert_eq!(cfg.main().items.len(), 2);
    }

    #[test]
    fn file_formats() {
        assert_eq!(file_format(Path::new("/x/a.MP4")), Some("video"));
        assert_eq!(file_format(Path::new("/x/a.png")), Some("static"));
        assert_eq!(file_format(Path::new("/x/a.gif")), Some("animated"));
        assert_eq!(file_format(Path::new("/x/a.swf")), Some("swf"));
        assert_eq!(file_format(Path::new("/x/index.html")), Some("web"));
        assert_eq!(file_format(Path::new("/x/scene.pkg")), None);
        assert!(!is_own_format("scene") && !is_own_format("application"));
    }
}
