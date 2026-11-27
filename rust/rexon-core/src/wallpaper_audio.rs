//! Wallpaper sound: one model, one policy, one source of values.
//!
//! Before this module wallpaper sound lived in three unrelated pieces:
//!
//!   * the «Video wallpaper sound» check box in our settings — saved nowhere
//!     and reset to "yes" every time the tab was opened;
//!   * our host's volume — a constant `50` in a Python object's memory, there
//!     was no control in the interface at all;
//!   * the foreign engine's volume, "mute when other audio plays" and "mute
//!     when…" table — a separate map that went to its daemon and had nothing
//!     to do with the first two.
//!
//! Because of this, a person who set the volume to 30 % in the foreign
//! engine's tab got 50 % for our video wallpapers, and the "no sound" switch
//! covered only the foreign engine. Rule #12: fix the shared mechanism, not
//! both copies separately — so the settings model, the defaults and the
//! "play/mute/pause" decision are computed HERE, and both sides only read
//! from here:
//!
//!   * our wallpaper host `rexon-wallpaper-host` — [`AudioSettings::from_map`]
//!     over `~/.config/rexon/settings.json` and [`AudioSettings::decide`];
//!   * the foreign engine — the same structure laid out into its global
//!     settings map ([`AudioSettings::write_engine_global`]).
//!
//! There is one store — `settings.json`. There is deliberately no separate
//! state file: live application works from the same file the settings window
//! writes, so "saved" and "heard" cannot diverge.

use serde_json::{Map, Value};

// ── Store keys ─────────────────────────────────────────────────────────────

/// Whether wallpaper sound plays at all.
pub const KEY_ENABLED: &str = "wallpaper_audio_enabled";
/// Wallpaper volume, percent 0..100.
pub const KEY_VOLUME: &str = "wallpaper_audio_volume";
/// Mute when someone else is playing sound.
pub const KEY_MUTE_OTHER: &str = "wallpaper_audio_mute_when_other";
/// What to do in each window state — a "condition → action" object.
pub const KEY_ACTIONS: &str = "wallpaper_audio_actions";
/// Mute fade smoothness, milliseconds.
pub const KEY_FADE_MS: &str = "wallpaper_audio_fade_ms";

// ── Foreign engine keys ────────────────────────────────────────────────────

/// The same values in the foreign engine's global settings map.
pub const ENGINE_KEY_ENABLED: &str = "renderer.enable_audio";
pub const ENGINE_KEY_VOLUME: &str = "renderer.volume";
pub const ENGINE_KEY_MUTE_OTHER: &str = "muteWhenOtherAudio";
pub const ENGINE_KEY_FADE_MS: &str = "audioFadeMs";
pub const ENGINE_KEY_ACTIONS: &str = "autoReplay";

// ── Audio stream properties ────────────────────────────────────────────────

/// The role of the wallpaper stream.
///
/// ⚠️ NOT `music`. Qt Multimedia hard-labels its streams `Music` — there is no
/// public way to change that, the value is baked into the library itself.
/// Because of this wallpapers end up in the music players' bucket: the role
/// determines the volume memory slot (shared with ALL music programs — the
/// wallpaper volume overwrites the player's volume and vice versa), and
/// `module-role-cork` and `module-role-ducking` work by it too.
///
/// Why exactly `Movie` and not "something about animation". Verified by
/// measurement on PipeWire 1.6: it has its own role dictionary (`Movie`,
/// `Music`, `Camera`, `Screen`, `Communication`, `Game`, `Notification`,
/// `DSP`, `Production`, `Accessibility`, `Test`), and PulseAudio roles are
/// translated into it — `animation` collapses into `Movie` and comes out as
/// `video`. That is, "giving wallpapers the animation role" is physically
/// unreachable: the choice is not between `animation` and `video` but between
/// `Music` and everything else. `Movie` describes video wallpapers honestly
/// and, unlike `Screen` (which is about screen capture), does not lie about
/// the stream's origin.
pub const STREAM_ROLE: &str = "Movie";

/// Node name in the sound system — Latin letters and no spaces: it is a key,
/// not a label.
pub const STREAM_NODE: &str = "rexon-wallpaper";

/// The key by which the sound system remembers the wallpaper volume
/// separately from everything else. It, not the role, is responsible for
/// volume memory on PipeWire.
pub const STREAM_APP_ID: &str = "com.rexon.wallpaper";

/// The name under which the wallpaper is visible in the system mixer. Human
/// and in the interface language: in the KDE mixer the line sits next to
/// «Firefox» and «Steam», and `RexonVideoWallpaper` reads there as a glitch,
/// not as a program (rule #13).
///
/// ⚠️ This is a LABEL, not a key: the sound system recognizes its own stream
/// by [`STREAM_APP_ID`] and [`STREAM_NODE`] (see [`OWN_STREAM_MARKERS`]), and
/// volume memory is kept on `application.id`. A double quote is cut out of
/// the translation — the name sits inside the quotes of the properties string.
pub fn stream_description() -> String {
    // TRANSLATORS: name of the wallpaper's audio stream in the system volume mixer
    crate::i18n::tr("Rexon Wallpaper").replace('"', "")
}

/// The stream name before the interface was translated. No longer a label but
/// a sign: people still have it in the sound system's volume memory slots.
const LEGACY_STREAM_DESCRIPTION: &str = "Обои Rexon";

/// The `PIPEWIRE_PROPS` string for the wallpaper process.
///
/// Why exactly the environment and not a Qt call: Qt Multimedia does not
/// expose the stream role. But the Qt 6.11 player goes to the sound system
/// with a native PipeWire client, and that one reads `PIPEWIRE_PROPS` when
/// creating the context, that is, already after process start: it is enough
/// to set the variable in our own `main()` before the first sound operation.
pub fn pipewire_props() -> String {
    let description = stream_description();
    format!(
        "{{ media.role = {STREAM_ROLE} media.category = Playback \
         application.id = {STREAM_APP_ID} node.name = {STREAM_NODE} \
         application.name = \"{description}\" \
         media.name = \"{description}\" }}"
    )
}

/// The same request for the PulseAudio client — on machines without PipeWire.
///
/// There the role is written in the PulseAudio dictionary (`video`), and the
/// stream properties are set by the program itself, so the variable does not
/// override everything; it does no harm, and on stacks where the role is set
/// on the context it hits the target.
pub fn pulse_props() -> String {
    format!(
        "media.role=video media.category=Playback application.id={STREAM_APP_ID}"
    )
}

/// The same request for the ALSA client — Flash goes to the sound system
/// through it (Ruffle via cpal).
///
/// ⚠️ Without it `PIPEWIRE_PROPS` carries the role and `application.id`, but
/// the NAME is rewritten by the ALSA plugin: in the system mixer the line
/// comes out as «PipeWire ALSA [rexon-wallpaper-host]» (live measurement
/// 16.09.2026). For the customer this is not a program name but a glitch
/// (rule #13), and for us too it is handier to recognize the stream by its
/// own name rather than by someone else's wrapper.
pub fn alsa_props() -> String {
    let description = stream_description();
    format!(
        "{{ node.name = {STREAM_NODE} application.name = \"{description}\" \
         media.name = \"{description}\" media.role = {STREAM_ROLE} \
         application.id = {STREAM_APP_ID} }}"
    )
}

/// All environment variables at once — what the host sets for itself in `main()`.
///
/// Three, not one: each sound client has its own way of reading stream
/// properties. Qt Multimedia (video) goes with a native PipeWire client — it
/// listens to `PIPEWIRE_PROPS`; on machines without PipeWire
/// `PULSE_PROP_OVERRIDE` works; Ruffle (Flash) goes via cpal and ALSA — there
/// `PIPEWIRE_ALSA` decides. An extra variable does no harm: a foreign client
/// simply does not read it.
pub fn stream_env() -> Vec<(&'static str, String)> {
    vec![
        ("PIPEWIRE_PROPS", pipewire_props()),
        ("PULSE_PROP_OVERRIDE", pulse_props()),
        ("PIPEWIRE_ALSA", alsa_props()),
    ]
}

// ── Actions ────────────────────────────────────────────────────────────────

/// What to do with the wallpaper when the condition is met.
///
/// The numbers match the foreign engine protocol's `AutoAction` (`NONE`,
/// `MUTE`, `PAUSE`, `STOP`) — on purpose: the same setting must not mean
/// different things in two places.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Action {
    /// Do nothing.
    #[default]
    None = 0,
    /// Mute the sound, keep the picture.
    Mute = 1,
    /// Stop playback (both sound and picture).
    Pause = 2,
    /// The same as pause, plus going back to the start.
    Stop = 3,
}

impl Action {
    /// From a protocol number; anything unclear is "do nothing", not accidental
    /// silence.
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => Action::Mute,
            2 => Action::Pause,
            3 => Action::Stop,
            _ => Action::None,
        }
    }

    /// The protocol number.
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    /// Whether this action mutes the sound.
    pub fn mutes(self) -> bool {
        self != Action::None
    }

    /// Whether this action stops playback.
    pub fn pauses(self) -> bool {
        matches!(self, Action::Pause | Action::Stop)
    }
}

/// Condition names — the same for both stores.
///
/// The last two (`gamemode`, `onBattery`) are named exactly as the foreign
/// engine's protocol calls them (the GameMode pause reason and battery
/// power): the table goes into its map through the same
/// [`AutoActions::to_map`], and a second name of our own for the same
/// condition would mean the person's choice never reaches the daemon.
pub const CONDITIONS: [&str; 8] = [
    "anyWindow",
    "focused",
    "maximized",
    "fullscreen",
    "sessionLocked",
    "sessionInactive",
    "gamemode",
    "onBattery",
];

/// The "condition → action" table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoActions {
    /// Some window is open.
    pub any_window: Action,
    /// A window over the wallpaper has focus.
    pub focused: Action,
    /// A window is maximized to the whole screen (the wallpaper is not visible).
    pub maximized: Action,
    /// A full-screen window — a game or a video.
    pub fullscreen: Action,
    /// The session is locked.
    pub session_locked: Action,
    /// The session is inactive (switched to another one).
    pub session_inactive: Action,
    /// A game is running: game mode is on (Feral GameMode) or gamescope is
    /// running.
    pub gamemode: Action,
    /// The computer runs on battery. Never happens on a machine without a
    /// battery.
    pub on_battery: Action,
}

impl Default for AutoActions {
    /// The product defaults, not an empty table.
    ///
    /// The complaint this module started with was "the wallpaper blares over
    /// games and videos", so the safe behavior is on right away (rule #13: do
    /// not ask where a wrong answer spoils the impression). A full-screen window
    /// is a game or a movie: the wallpaper is not visible there at all, so it has
    /// no reason to work — pause. A window maximized to the whole screen — the
    /// wallpaper is not visible either, but a pause would cause a jerk on every
    /// minimize, so only silence.
    ///
    /// Battery and game — pause, not "nothing" (the foreign engine has "nothing"
    /// there). The difference is deliberate, Wehrwolfmann's decision of
    /// 15.11.2026: a live wallpaper on a laptop on battery means noticeably
    /// shorter time away from the outlet, and during a game — frames taken away
    /// from the game by the same graphics card. A person who does not know this
    /// will not go fix it — they will decide the "laptop is weak" (rule #13: safe
    /// behavior by default). Pause, not "stop": back at the outlet, the wallpaper
    /// continues from the same place, without a jump to the start.
    fn default() -> Self {
        Self {
            any_window: Action::None,
            focused: Action::None,
            maximized: Action::Mute,
            fullscreen: Action::Pause,
            session_locked: Action::Stop,
            session_inactive: Action::Stop,
            gamemode: Action::Pause,
            on_battery: Action::Pause,
        }
    }
}

impl AutoActions {
    /// Lay out into a map with condition names (both sides — one layout).
    pub fn to_map(self) -> Map<String, Value> {
        let mut m = Map::new();
        for (name, action) in CONDITIONS.iter().zip(self.list()) {
            m.insert((*name).to_string(), Value::from(action.as_i64()));
        }
        m
    }

    /// Build from a map; whatever is missing is taken from the defaults.
    pub fn from_map(m: &Map<String, Value>) -> Self {
        let pick = |name: &str, fallback: Action| -> Action {
            m.get(name)
                .and_then(Value::as_i64)
                .map(Action::from_i64)
                .unwrap_or(fallback)
        };
        let d = Self::default();
        Self {
            any_window: pick("anyWindow", d.any_window),
            focused: pick("focused", d.focused),
            maximized: pick("maximized", d.maximized),
            fullscreen: pick("fullscreen", d.fullscreen),
            session_locked: pick("sessionLocked", d.session_locked),
            session_inactive: pick("sessionInactive", d.session_inactive),
            gamemode: pick("gamemode", d.gamemode),
            on_battery: pick("onBattery", d.on_battery),
        }
    }

    /// Actions in [`CONDITIONS`] order.
    pub fn list(self) -> [Action; 8] {
        [
            self.any_window,
            self.focused,
            self.maximized,
            self.fullscreen,
            self.session_locked,
            self.session_inactive,
            self.gamemode,
            self.on_battery,
        ]
    }

    /// Whether to ask about windows at all: at least one of the "window"
    /// conditions does something. Querying the window manager is a process on
    /// every tick, and there is no point running it just for the battery.
    pub fn windows_matter(self) -> bool {
        [self.any_window, self.focused, self.maximized, self.fullscreen]
            .iter()
            .any(|a| *a != Action::None)
    }

    /// Whether to ask logind about the session (locked / inactive).
    pub fn session_matters(self) -> bool {
        self.session_locked != Action::None || self.session_inactive != Action::None
    }
}

// ── Observed desktop state ─────────────────────────────────────────────────

/// What is happening around the wallpaper right now. Filled in by the host's
/// observer; here is only the decision based on these facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Conditions {
    pub any_window: bool,
    pub focused: bool,
    pub maximized: bool,
    pub fullscreen: bool,
    pub session_locked: bool,
    pub session_inactive: bool,
    /// A game is running: game mode is on or gamescope is running.
    pub gamemode: bool,
    /// Running on battery.
    pub on_battery: bool,
    /// Someone else (not us) is playing sound.
    pub other_audio: bool,
}

impl Conditions {
    /// Flags in [`CONDITIONS`] order.
    pub fn list(self) -> [bool; 8] {
        [
            self.any_window,
            self.focused,
            self.maximized,
            self.fullscreen,
            self.session_locked,
            self.session_inactive,
            self.gamemode,
            self.on_battery,
        ]
    }
}

// ── Settings ───────────────────────────────────────────────────────────────

/// Default volume, percent.
///
/// The original has 100, we have 50: the wallpaper is a background, and the
/// first time it is switched on must not deafen. A person can raise it to 100
/// in one move, but "got scared and turned it off forever" cannot be undone.
pub const DEFAULT_VOLUME_PCT: i64 = 50;
/// Default mute fade smoothness, ms (as in the foreign engine).
pub const DEFAULT_FADE_MS: i64 = 500;

/// The complete wallpaper sound settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioSettings {
    pub enabled: bool,
    pub volume_pct: i64,
    pub mute_when_other_audio: bool,
    pub fade_ms: i64,
    pub actions: AutoActions,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            volume_pct: DEFAULT_VOLUME_PCT,
            // The foreign engine has this off by default, we have it on. The
            // difference is deliberate and has the same meaning as in `AutoActions`:
            // a wallpaper shouting over a film is perceived by a person as a broken
            // product, not as a setting they forgot to turn on.
            mute_when_other_audio: true,
            fade_ms: DEFAULT_FADE_MS,
            actions: AutoActions::default(),
        }
    }
}

/// Clamp the volume to 0..100.
pub fn clamp_volume_pct(v: i64) -> i64 {
    v.clamp(0, 100)
}

impl AudioSettings {
    /// Read from Rexon's settings map (`settings.json`).
    pub fn from_map(m: &Map<String, Value>) -> Self {
        let d = Self::default();
        let actions = match m.get(KEY_ACTIONS) {
            Some(Value::Object(o)) => AutoActions::from_map(o),
            _ => d.actions,
        };
        Self {
            enabled: m.get(KEY_ENABLED).and_then(Value::as_bool).unwrap_or(d.enabled),
            volume_pct: clamp_volume_pct(
                m.get(KEY_VOLUME)
                    .and_then(Value::as_i64)
                    .unwrap_or(d.volume_pct),
            ),
            mute_when_other_audio: m
                .get(KEY_MUTE_OTHER)
                .and_then(Value::as_bool)
                .unwrap_or(d.mute_when_other_audio),
            fade_ms: m
                .get(KEY_FADE_MS)
                .and_then(Value::as_i64)
                .unwrap_or(d.fade_ms)
                .clamp(0, 5000),
            actions,
        }
    }

    /// Lay out into Rexon's settings map.
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert(KEY_ENABLED.into(), Value::Bool(self.enabled));
        m.insert(KEY_VOLUME.into(), Value::from(self.volume_pct));
        m.insert(
            KEY_MUTE_OTHER.into(),
            Value::Bool(self.mute_when_other_audio),
        );
        m.insert(KEY_FADE_MS.into(), Value::from(self.fade_ms));
        m.insert(KEY_ACTIONS.into(), Value::Object(self.actions.to_map()));
        m
    }

    /// Read from the FOREIGN engine's global settings map.
    ///
    /// Exactly the same values under its names — so that "wallpaper volume"
    /// means the same thing on both sides and cannot be split apart by editing
    /// one of the copies.
    pub fn from_engine_global(g: &Map<String, Value>) -> Self {
        let d = Self::default();
        let actions = match g.get(ENGINE_KEY_ACTIONS) {
            Some(Value::Object(o)) => AutoActions::from_map(o),
            _ => d.actions,
        };
        Self {
            enabled: g
                .get(ENGINE_KEY_ENABLED)
                .and_then(Value::as_bool)
                .unwrap_or(d.enabled),
            volume_pct: clamp_volume_pct(
                g.get(ENGINE_KEY_VOLUME)
                    .and_then(Value::as_i64)
                    .unwrap_or(d.volume_pct),
            ),
            mute_when_other_audio: g
                .get(ENGINE_KEY_MUTE_OTHER)
                .and_then(Value::as_bool)
                .unwrap_or(d.mute_when_other_audio),
            fade_ms: g
                .get(ENGINE_KEY_FADE_MS)
                .and_then(Value::as_i64)
                .unwrap_or(d.fade_ms),
            actions,
        }
    }

    /// Write the values into the foreign engine's global settings map.
    ///
    /// ⚠️ Exactly write into the existing one, not build a new one: that engine
    /// saves settings by replacing the whole map, and a key lost on the way is
    /// erased from the person's settings for good.
    pub fn write_engine_global(&self, g: &mut Map<String, Value>) {
        g.insert(ENGINE_KEY_ENABLED.into(), Value::Bool(self.enabled));
        g.insert(ENGINE_KEY_VOLUME.into(), Value::from(self.volume_pct));
        g.insert(
            ENGINE_KEY_MUTE_OTHER.into(),
            Value::Bool(self.mute_when_other_audio),
        );
        g.insert(ENGINE_KEY_FADE_MS.into(), Value::from(self.fade_ms));
        g.insert(
            ENGINE_KEY_ACTIONS.into(),
            Value::Object(self.actions.to_map()),
        );
    }

    /// Volume as a fraction of one — the form the player expects.
    pub fn volume01(&self) -> f64 {
        clamp_volume_pct(self.volume_pct) as f64 / 100.0
    }

    /// The decision for the current situation.
    pub fn decide(&self, c: &Conditions) -> Decision {
        // The strictest of the triggered actions wins: "stop" is stronger than
        // "pause", pause is stronger than silence. The order is given by the order
        // of the `Action` variants, so a new table row gets into the decision by
        // itself, without editing this function.
        let mut worst = Action::None;
        for (action, active) in self.actions.list().iter().zip(c.list()) {
            if active && *action > worst {
                worst = *action;
            }
        }
        let muted_by_other = self.mute_when_other_audio && c.other_audio;
        Decision {
            // Sound is given only when it was requested AND nothing says to be silent.
            audio_on: self.enabled && !worst.mutes() && !muted_by_other,
            paused: worst.pauses(),
            rewind: worst == Action::Stop,
            volume01: self.volume01(),
        }
    }
}

/// The simple form of the "stop the wallpaper when it is not visible" setting.
///
/// In the product's interface a "six conditions × four actions" table is a
/// screen for someone who already knows what they want (rule #13: the
/// customer needs "easy and simple"). So one check box goes outside, and the
/// core expands it into the table — so that the check box value and the
/// table values cannot diverge.
pub fn actions_for_hide_when_covered(on: bool) -> AutoActions {
    actions_for_choices(SimpleChoices {
        hide_when_covered: on,
        ..SimpleChoices::default()
    })
}

/// The reverse question: whether this check box is on right now.
pub fn hide_when_covered(actions: &AutoActions) -> bool {
    actions.fullscreen != Action::None || actions.maximized != Action::None
}

/// Three check boxes of the «Wallpaper» section instead of the "condition × action" table.
///
/// Each check box is expanded into table rows here, in the core — so that the
/// settings window and the wallpaper host cannot understand the same check
/// box differently (rule #12). The default is all three on: the same as
/// [`AutoActions::default`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimpleChoices {
    /// «Stop the wallpaper when it is hidden behind a window».
    pub hide_when_covered: bool,
    /// «Pause the wallpaper when running on battery».
    pub pause_on_battery: bool,
    /// «Pause the wallpaper while gaming».
    pub pause_in_game: bool,
}

impl Default for SimpleChoices {
    fn default() -> Self {
        choices_of(&AutoActions::default())
    }
}

/// Check boxes → table.
pub fn actions_for_choices(c: SimpleChoices) -> AutoActions {
    let d = AutoActions::default();
    let covered = |on: bool, action: Action| if on { action } else { Action::None };
    AutoActions {
        any_window: covered(c.hide_when_covered, d.any_window),
        focused: covered(c.hide_when_covered, d.focused),
        maximized: covered(c.hide_when_covered, d.maximized),
        fullscreen: covered(c.hide_when_covered, d.fullscreen),
        // A locked and an inactive session are not switched off by the check boxes:
        // playing the wallpaper in someone else's session or on the lock screen is
        // not "a matter of taste" but a drained battery and sound out of nowhere.
        session_locked: d.session_locked,
        session_inactive: d.session_inactive,
        gamemode: if c.pause_in_game { Action::Pause } else { Action::None },
        on_battery: if c.pause_on_battery { Action::Pause } else { Action::None },
    }
}

/// Table → check boxes (what to show in the settings window).
pub fn choices_of(actions: &AutoActions) -> SimpleChoices {
    SimpleChoices {
        hide_when_covered: hide_when_covered(actions),
        pause_on_battery: actions.on_battery != Action::None,
        pause_in_game: actions.gamemode != Action::None,
    }
}

/// What the host must do right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decision {
    /// Keep the audio stream open.
    ///
    /// ⚠️ Exactly "there is a stream / there is no stream", not "volume 0". An
    /// open silent stream hangs in the mixer the whole session as a separate
    /// slider that plays nothing, and keeps the sound card from sleeping;
    /// besides, it makes it impossible to tell "works quietly" from "does not
    /// work at all" — and exactly on this indistinguishability the wallpaper
    /// self-check once already went green where nothing reached the sound card.
    pub audio_on: bool,
    /// Keep playback paused.
    pub paused: bool,
    /// Rewind to the start (the "stop" action).
    pub rewind: bool,
    /// Volume as a fraction of one.
    pub volume01: f64,
}

// ── Foreign sound ──────────────────────────────────────────────────────────

/// By which stream properties it can be recognized at all.
///
/// ⚠️ `application.id` is FIRST in the list for a reason: it is the only
/// property that reaches the sound system unchanged along BOTH roads — for
/// video (the PipeWire client of Qt Multimedia) and for Flash (the ALSA client
/// of cpal). The ALSA plugin rewrites the others in its own way.
pub const STREAM_ID_KEYS: [&str; 5] = [
    "application.id",
    "node.name",
    "application.name",
    "media.name",
    "application.process.binary",
];

/// Signs of OUR wallpaper stream — the same for all wallpaper kinds and both
/// host modes.
///
/// ⭐ A fix of 16.09.2026, found by measurement. The list lived in the host
/// and was compared by EXACT equality, so it recognized only video. Flash
/// goes to the sound system with an ALSA client (cpal), and PipeWire's ALSA
/// plugin labels the stream in its own way:
/// `node.name = "alsa_playback.rexon-wallpaper-host"`,
/// `application.name = "PipeWire ALSA [rexon-wallpaper-host]"`. There is no
/// exact match with any former name there — and Flash wallpapers, with the
/// "mute when sound comes from another program" setting on by default, took
/// THEMSELVES for another program and muted themselves. In the journal this
/// looked like "another program is playing sound" on an empty desktop, and to
/// a person simply like "Flash has no sound".
///
/// Hence: the signs are compared by containment, not equality, and live in
/// the core, not in the host — so that a third wallpaper kind does not start
/// a third copy of the list (rule #12).
pub const OWN_STREAM_MARKERS: [&str; 4] = [
    STREAM_APP_ID,
    STREAM_NODE,
    LEGACY_STREAM_DESCRIPTION,
    // The former video wallpaper stream name: people still have it in the sound
    // system's volume memory slots.
    "RexonVideoWallpaper",
];

/// The command that asks the sound system for the list of streams.
///
/// ⚠️ `LANG=C` is mandatory: under a Russian locale `pactl` translates field
/// names, and the parser below stops finding `Corked`.
pub fn sink_inputs_argv() -> Vec<String> {
    ["pactl", "list", "sink-inputs"]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

/// Whether someone other than us is playing sound.
///
/// Parses the output of [`sink_inputs_argv`]. Our own streams are recognized
/// by the [`OWN_STREAM_MARKERS`] signs in the [`STREAM_ID_KEYS`] properties;
/// stopped ones (`Corked: yes`) do not count — otherwise any paused player
/// would pass for "foreign sound", and the wallpaper would always be silent.
///
/// ⚠️ The sign is searched by CONTAINMENT. Exact equality here already cost
/// us self-muting Flash wallpapers: see [`OWN_STREAM_MARKERS`].
pub fn other_audio_playing(stdout: &str, own_names: &[&str]) -> bool {
    let mut corked = false;
    let mut ours = false;
    let mut any = false;
    // Block-wise parsing: a block starts with the «Sink Input #N» line.
    let finish = |any: bool, ours: bool, corked: bool| any && !ours && !corked;
    for line in stdout.lines() {
        let t = line.trim();
        if t.starts_with("Sink Input #") {
            if finish(any, ours, corked) {
                return true;
            }
            corked = false;
            ours = false;
            any = true;
            continue;
        }
        if let Some(v) = t.strip_prefix("Corked:") {
            corked = v.trim().eq_ignore_ascii_case("yes");
        }
        if STREAM_ID_KEYS.iter().any(|k| t.starts_with(k)) {
            if let Some((_, value)) = t.split_once('=') {
                let value = value.trim().trim_matches('"');
                if own_names.iter().any(|n| value.contains(*n)) {
                    ours = true;
                }
            }
        }
    }
    finish(any, ours, corked)
}

// ── Polling the situation ──────────────────────────────────────────────────

/// Whether the session is locked — one question to the screen saver.
pub fn screensaver_argv() -> Vec<String> {
    [
        "qdbus6",
        "org.freedesktop.ScreenSaver",
        "/ScreenSaver",
        "org.freedesktop.ScreenSaver.GetActive",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// Parsing the screen saver's answer.
pub fn parse_screensaver_active(stdout: &str) -> bool {
    stdout.trim().eq_ignore_ascii_case("true")
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}

/// A D-Bus property — via a `Properties.Get` call, not `busctl get-property`.
///
/// ⚠️ `--auto-start=no` is mandatory, and that is exactly why `call`: the game
/// mode service is started by the bus on first access. Polling "is a game
/// running" every couple of seconds would by itself start the game mode
/// daemon on every machine where it is installed but not used. The
/// `--auto-start` switch of `busctl` works only for `call`.
fn dbus_property_argv(bus: &str, service: &str, path: &str, iface: &str, prop: &str) -> Vec<String> {
    argv(&[
        "busctl",
        bus,
        "--auto-start=no",
        "call",
        service,
        path,
        "org.freedesktop.DBus.Properties",
        "Get",
        "ss",
        iface,
        prop,
    ])
}

/// Whether the machine runs on battery — the `OnBattery` property of the
/// UPower service (system bus). On a computer without a battery UPower answers `false`.
pub fn upower_on_battery_argv() -> Vec<String> {
    dbus_property_argv(
        "--system",
        "org.freedesktop.UPower",
        "/org/freedesktop/UPower",
        "org.freedesktop.UPower",
        "OnBattery",
    )
}

/// How many programs requested game mode — the `ClientCount` property of the
/// Feral GameMode service (session bus). Above zero — a game is running.
pub fn gamemode_client_count_argv() -> Vec<String> {
    dbus_property_argv(
        "--user",
        "com.feralinteractive.GameMode",
        "/com/feralinteractive/GameMode",
        "com.feralinteractive.GameMode",
        "ClientCount",
    )
}

/// The last word of the `busctl call … Properties.Get` answer: `v b true`,
/// `v i 2`. Empty — no answer.
fn dbus_value(stdout: &str) -> Option<&str> {
    let mut words = stdout.split_whitespace();
    // The answer starts with the variant signature `v` and the value type.
    if words.next()? != "v" {
        return None;
    }
    words.nth(1)
}

/// Parsing a boolean property (`v b true`). An unclear answer — `None`.
pub fn parse_dbus_bool(stdout: &str) -> Option<bool> {
    match dbus_value(stdout)? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Parsing an integer property (`v i 2`, `v u 2`). An unclear answer — `None`.
pub fn parse_dbus_int(stdout: &str) -> Option<i64> {
    dbus_value(stdout)?.parse().ok()
}

/// The battery fallback when there is no UPower or it is silent: the battery
/// state from `/sys/class/power_supply/BAT*/status`. On battery — when at
/// least one is discharging. No batteries — never.
pub fn sysfs_on_battery(statuses: &[String]) -> bool {
    statuses
        .iter()
        .any(|s| s.trim().eq_ignore_ascii_case("Discharging"))
}

/// gamescope process names in `/proc/PID/comm`. A separate gamescope on the
/// desktop is a game running in it (Steam launches games like that with the
/// `gamescope %command%` parameter), even when game mode is not on.
pub const GAMESCOPE_COMMS: [&str; 2] = ["gamescope", "gamescope-wl"];

/// A process with such a `comm` is gamescope.
pub fn is_gamescope_comm(comm: &str) -> bool {
    let comm = comm.trim();
    GAMESCOPE_COMMS.iter().any(|g| *g == comm)
}

/// Session state — one question to logind: whether it is active and whether it is locked.
///
/// `session_id` — the process's `XDG_SESSION_ID`. If there is none (the host
/// was started by a user service, outside a session) or it is unfit for a
/// command line, we ask about `auto` — that is what logind calls the caller's
/// session, and without one, the graphical session of the same user.
pub fn logind_session_argv(session_id: &str) -> Vec<String> {
    let id = if crate::privilege::revoke::valid_session_id(session_id) {
        session_id
    } else {
        "auto"
    };
    argv(&["loginctl", "show-session", id, "-p", "Active", "-p", "LockedHint"])
}

/// Parsing logind's answer into `(inactive, locked)`.
///
/// ⚠️ An unclear answer (no logind, the session was not found) is
/// `(false, false)`: a "session inactive" caused by a polling error would
/// leave the person with a forever-stopped wallpaper, and they would not go
/// fix it — they would decide the wallpaper is broken.
pub fn parse_logind_session(stdout: &str) -> (bool, bool) {
    use crate::privilege::revoke::{parse_active_locked, SessionAnswer};
    match parse_active_locked(stdout) {
        SessionAnswer::Seen { active, locked } => (!active, locked),
        _ => (false, false),
    }
}

/// Parsing the X11 active window geometry into observed conditions.
///
/// The twin of [`kwin_window_conditions`] for X11, and their filtering is
/// SHARED ([`crate::wallpaper_engine::window_is_wallpaper_layer`]), so "a
/// window over the wallpaper" means the same thing on both sides of the
/// session (rule #12).
///
/// ⚠️ Measurement of 16.09.2026 on an X11 test bench with a window manager:
/// `xdotool getactivewindow` named THE WALLPAPER ITSELF as the active window —
/// a full-screen window with `_NET_WM_WINDOW_TYPE_DESKTOP`. The "a full-screen
/// window is playing on top" rule always fired, and Flash wallpapers gave 4
/// frames instead of 240 over the same run.
///
/// `geometry` — the output of `xdotool … --shell`, `screen` — the screen
/// yardstick, `own_pids` — the processes whose windows are the wallpaper.
/// `window_props` is called LAZILY and only when a window was named at all;
/// `None` (no `xprop`, it did not answer) and an unclear answer keep the
/// UNCHANGED behavior — the window counts as an ordinary one. Neither "always
/// mute" nor "never mute".
pub fn x11_window_conditions(
    geometry: &str,
    screen: &str,
    own_pids: &[i64],
    window_props: impl FnOnce(i64) -> Option<String>,
) -> Conditions {
    use crate::wallpaper_engine as we;
    if geometry.trim().is_empty() {
        return Conditions::default();
    }
    if let Some(id) = we::parse_xdotool_window_id(geometry) {
        if let Some(props) = window_props(id) {
            if we::window_is_wallpaper_layer(&props, own_pids) {
                return Conditions::default();
            }
        }
    }
    // On X11 there is no separate "maximized" flag without extra questions:
    // we compute it from the geometry by the same rule as on Wayland.
    let covers = we::parse_xdotool_fullscreen(geometry, screen);
    Conditions {
        any_window: true,
        focused: true,
        maximized: covers,
        fullscreen: covers,
        ..Conditions::default()
    }
}

/// Parsing the KWin window property dictionary into observed conditions.
///
/// KWin names `fullscreen` itself; "maximized" and "there is a window on top"
/// are computed from the geometry — the same way as on X11, so the behavior
/// is the same on both sides of the session (rule #12). The screen size comes
/// from the wallpaper window: it is exactly full-screen.
///
/// `own_pids` — the processes whose windows are the wallpaper: our own window
/// does not count as "a window over the wallpaper". Which window types are
/// filtered out is decided by the engine core
/// ([`crate::wallpaper_engine::kwin_window_is_wallpaper_layer`]) — the list is
/// one per product, and it also works on X11.
pub fn kwin_window_conditions(
    info: &str,
    screen_w: i64,
    screen_h: i64,
    own_pids: &[i64],
) -> Conditions {
    let mut c = Conditions::default();
    let mut w = 0i64;
    let mut h = 0i64;
    let mut minimized = false;
    let mut have_window = false;
    for line in info.lines() {
        let t = line.trim();
        let Some((key, value)) = t.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "fullscreen" => {
                have_window = true;
                c.fullscreen = value == "true";
            }
            "minimized" => minimized = value == "true",
            "width" => w = value.parse().unwrap_or(0),
            "height" => h = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    if !have_window
        || minimized
        || crate::wallpaper_engine::kwin_window_is_wallpaper_layer(info, own_pids)
    {
        return Conditions::default();
    }
    c.any_window = true;
    c.focused = true;
    c.maximized = screen_w > 0 && screen_h > 0 && w >= screen_w && h >= screen_h;
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(json: &str) -> Map<String, Value> {
        match serde_json::from_str::<Value>(json).unwrap() {
            Value::Object(o) => o,
            _ => panic!("нужен объект"),
        }
    }

    #[test]
    fn роль_потока_не_музыкальная() {
        // Exactly the substitution this module was written for: a wallpaper with
        // the `music` role ends up in the shared volume restore slot with all
        // music players.
        assert_ne!(STREAM_ROLE.to_lowercase(), "music");
        let pw = pipewire_props();
        assert!(pw.contains("media.role = Movie"));
        assert!(pw.contains(&stream_description()), "в микшере — по-человечески");
        assert!(pw.contains(STREAM_APP_ID), "ключ памяти громкости обязателен");
        assert!(pw.contains(STREAM_NODE));
        // The role is named differently in the PulseAudio dictionary, and the
        // substitution must travel in both forms — otherwise on a machine without
        // PipeWire it is music again.
        assert!(pulse_props().contains("media.role=video"));
        // And for the ALSA client — Flash goes to the sound system through it;
        // without this request the ALSA plugin rewrites the stream name.
        assert!(alsa_props().contains(&stream_description()));
        assert!(alsa_props().contains(STREAM_NODE));
        assert_eq!(stream_env().len(), 3);
    }

    #[test]
    fn умолчания_не_дают_обоям_перекрикивать_игру() {
        let s = AudioSettings::default();
        assert!(s.enabled);
        assert_eq!(s.volume_pct, 50);
        assert!(s.mute_when_other_audio);
        assert_eq!(s.actions.fullscreen, Action::Pause);
        assert_eq!(s.actions.maximized, Action::Mute);
        // A calm desktop — sound plays.
        let d = s.decide(&Conditions::default());
        assert!(d.audio_on && !d.paused);
        assert_eq!(d.volume01, 0.5);
    }

    #[test]
    fn полноэкранное_окно_ставит_на_паузу_и_гасит_звук() {
        let s = AudioSettings::default();
        let c = Conditions {
            any_window: true,
            focused: true,
            maximized: true,
            fullscreen: true,
            ..Conditions::default()
        };
        let d = s.decide(&c);
        assert!(d.paused, "полноэкранное окно — обоев не видно");
        assert!(!d.audio_on, "пауза обязана снимать и звук");
        assert!(!d.rewind, "перематывает только «остановить»");
    }

    #[test]
    fn развёрнутое_окно_только_глушит() {
        let s = AudioSettings::default();
        let c = Conditions {
            any_window: true,
            focused: true,
            maximized: true,
            ..Conditions::default()
        };
        let d = s.decide(&c);
        assert!(!d.audio_on);
        assert!(!d.paused, "рывок при каждом разворачивании окна не нужен");
    }

    #[test]
    fn чужой_звук_глушит_обои() {
        let s = AudioSettings::default();
        let c = Conditions {
            other_audio: true,
            ..Conditions::default()
        };
        assert!(!s.decide(&c).audio_on);
        // The setting is off — the wallpaper plays on top, as it was asked to.
        let s = AudioSettings {
            mute_when_other_audio: false,
            ..AudioSettings::default()
        };
        assert!(s.decide(&c).audio_on);
    }

    #[test]
    fn строгое_действие_побеждает_мягкое() {
        let s = AudioSettings {
            actions: AutoActions {
                maximized: Action::Mute,
                fullscreen: Action::Stop,
                ..AutoActions::default()
            },
            ..AudioSettings::default()
        };
        let c = Conditions {
            maximized: true,
            fullscreen: true,
            ..Conditions::default()
        };
        let d = s.decide(&c);
        assert!(d.paused && d.rewind && !d.audio_on);
    }

    #[test]
    fn выключенный_звук_не_включается_ничем() {
        let s = AudioSettings {
            enabled: false,
            ..AudioSettings::default()
        };
        assert!(!s.decide(&Conditions::default()).audio_on);
    }

    #[test]
    fn обе_стороны_читают_одни_значения() {
        // A value set in our settings, laid out for the foreign engine and read
        // back, must match itself — otherwise "wallpaper volume" again means
        // different things in two places.
        let s = AudioSettings {
            enabled: false,
            volume_pct: 30,
            mute_when_other_audio: false,
            fade_ms: 250,
            actions: AutoActions {
                focused: Action::Mute,
                ..AutoActions::default()
            },
        };
        let mut engine = map(r#"{"queueMode":"sequential"}"#);
        s.write_engine_global(&mut engine);
        assert_eq!(AudioSettings::from_engine_global(&engine), s);
        // And the foreign keys are in place: saving settings there replaces the
        // whole map, a lost key is erased from the person's settings.
        assert_eq!(engine.get("queueMode"), Some(&Value::from("sequential")));
        // Our store — the same round trip.
        let ours = s.to_map();
        assert_eq!(AudioSettings::from_map(&ours), s);
    }

    #[test]
    fn мусор_в_настройках_не_оставляет_без_звука() {
        let m = map(r#"{"wallpaper_audio_volume": 4000, "wallpaper_audio_enabled": "да"}"#);
        let s = AudioSettings::from_map(&m);
        assert_eq!(s.volume_pct, 100, "громкость зажимается, а не улетает");
        assert!(s.enabled, "нечисловое значение — умолчание, не тишина");
    }

    #[test]
    fn чужой_звук_узнаётся_по_выводу_pactl() {
        let ours = "\
Sink Input #353
\tMute: no
\tCorked: no
\tProperties:
\t\tmedia.role = \"animation\"
\t\tnode.name = \"RexonVideoWallpaper\"
";
        assert!(
            !other_audio_playing(ours, &["RexonVideoWallpaper"]),
            "свой поток чужим звуком не считается"
        );
        let firefox = format!(
            "{ours}Sink Input #400\n\tCorked: no\n\t\tapplication.name = \"Firefox\"\n"
        );
        assert!(other_audio_playing(&firefox, &["RexonVideoWallpaper"]));
        let paused = format!(
            "{ours}Sink Input #400\n\tCorked: yes\n\t\tapplication.name = \"Firefox\"\n"
        );
        assert!(
            !other_audio_playing(&paused, &["RexonVideoWallpaper"]),
            "остановленный проигрыватель — не играющий звук"
        );
        assert!(!other_audio_playing("", &["RexonVideoWallpaper"]));
    }

    #[test]
    fn поток_flash_обоев_узнаётся_своим() {
        // ⭐ Live measurement of 16.09.2026 (`pactl list sink-inputs`, the lines
        // below are copied verbatim). Flash goes to the sound system with an ALSA
        // client, and PipeWire's ALSA plugin labels the stream in its own words —
        // there is no exact match with any of our names. While the signs were
        // compared by equality, Flash wallpapers, with the "mute when sound comes
        // from another program" setting on by default, took THEMSELVES for another
        // program and muted themselves.
        let flash = "\
Sink Input #72
\tCorked: no
\tProperties:
\t\tapplication.name = \"PipeWire ALSA [rexon-wallpaper-host]\"
\t\tnode.name = \"alsa_playback.rexon-wallpaper-host\"
\t\tmedia.name = \"ALSA Playback\"
\t\tapplication.id = \"com.rexon.wallpaper\"
";
        assert!(
            !other_audio_playing(flash, &OWN_STREAM_MARKERS),
            "поток Flash-обоев обязан быть узнан своим"
        );
        // `application.id` by itself must already be enough: it is the same for
        // video and Flash and reaches the sound system along both roads.
        let only_id = "Sink Input #1\n\tCorked: no\n\t\tapplication.id = \"com.rexon.wallpaper\"\n";
        assert!(!other_audio_playing(only_id, &OWN_STREAM_MARKERS));

        // A foreign player labeled by the same ALSA plugin stays foreign: the
        // containment is searched by OUR signs, not by the word «alsa».
        let alien = "Sink Input #1\n\tCorked: no\n\t\t                     application.name = \"PipeWire ALSA [mpv]\"\n\t\t                     node.name = \"alsa_playback.mpv\"\n";
        assert!(other_audio_playing(alien, &OWN_STREAM_MARKERS));
        assert_eq!(OWN_STREAM_MARKERS[0], STREAM_APP_ID);
    }

    #[test]
    fn состояние_окна_kwin_разбирается() {
        let maximized = "\
caption: ~ — Konsole
fullscreen: false
height: 1080
minimized: false
type: 0
width: 1920
";
        let c = kwin_window_conditions(maximized, 1920, 1080, &[]);
        assert!(c.any_window && c.focused && c.maximized && !c.fullscreen);

        let full = maximized.replace("fullscreen: false", "fullscreen: true");
        assert!(kwin_window_conditions(&full, 1920, 1080, &[]).fullscreen);

        // The desktop itself is not a window over the wallpaper.
        let desktop = maximized.replace("type: 0", "type: 1");
        assert_eq!(
            kwin_window_conditions(&desktop, 1920, 1080, &[]),
            Conditions::default()
        );

        // A panel/dock is not "a window over the wallpaper" either: a strip at the
        // screen edge does not cover the wallpaper, and the former filter knew only
        // the desktop.
        let dock = maximized.replace("type: 0", "type: 2");
        assert_eq!(
            kwin_window_conditions(&dock, 1920, 1080, &[]),
            Conditions::default()
        );

        // A minimized window does not cover the wallpaper.
        let mini = maximized.replace("minimized: false", "minimized: true");
        assert_eq!(
            kwin_window_conditions(&mini, 1920, 1080, &[]),
            Conditions::default()
        );

        // No active window — an empty answer, not "full-screen".
        assert_eq!(
            kwin_window_conditions("", 1920, 1080, &[]),
            Conditions::default()
        );

        // A window smaller than the screen — the wallpaper is visible.
        let small = maximized.replace("width: 1920", "width: 800");
        assert!(!kwin_window_conditions(&small, 1920, 1080, &[]).maximized);
    }

    /// The `xdotool … --shell` answer for a 1280×800 full-screen window.
    fn окно_во_весь_экран(id: i64) -> String {
        format!("WINDOW={id}\nX=0\nY=0\nWIDTH=1280\nHEIGHT=800\nSCREEN=0\n")
    }

    #[test]
    fn окно_рабочего_стола_не_ставит_обои_на_паузу() {
        // ⭐ Measurement of 16.09.2026 on an X11 test bench with a window manager:
        // THE WALLPAPER ITSELF was named the active window — a full-screen window of
        // type DESKTOP. Flash wallpapers gave 4 frames instead of 240 over the same run.
        let g = окно_во_весь_экран(10485771);
        let desktop = "_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_DESKTOP\n\
                       _NET_WM_PID(CARDINAL) = 2742555\n";
        assert_eq!(
            x11_window_conditions(&g, "1280x800", &[], |_| Some(desktop.to_string())),
            Conditions::default(),
            "рабочий стол"
        );

        // A panel/dock is not "a window over the wallpaper" either.
        let dock = "_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_DOCK\n";
        assert_eq!(
            x11_window_conditions(&g, "1280x800", &[], |_| Some(dock.to_string())),
            Conditions::default(),
            "панель"
        );

        // Our own wallpaper window — all the more so, even if the type could not be set.
        let no_type = "_NET_WM_WINDOW_TYPE:  not found.\n\
                       _NET_WM_PID(CARDINAL) = 2742555\n";
        assert_eq!(
            x11_window_conditions(&g, "1280x800", &[2742555], |_| Some(
                no_type.to_string()
            )),
            Conditions::default(),
            "свои обои"
        );

        // A real full-screen game still turns the wallpaper off.
        let game = "_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_NORMAL\n\
                    _NET_WM_PID(CARDINAL) = 999\n";
        let c = x11_window_conditions(&g, "1280x800", &[2742555], |_| {
            Some(game.to_string())
        });
        assert!(c.fullscreen && c.maximized && c.any_window && c.focused, "игра");
    }

    #[test]
    fn отсев_окон_рабочего_стола_защищён_от_дурака() {
        let g = окно_во_весь_экран(10485771);

        // `xprop` is not installed or did not answer — the behavior is UNCHANGED: a
        // full-screen window counts as a full-screen window. Neither "always mute"
        // nor "never mute".
        assert!(x11_window_conditions(&g, "1280x800", &[], |_| None).fullscreen);

        // There is an answer, but an unclear one — also the unchanged behavior.
        assert!(
            x11_window_conditions(&g, "1280x800", &[], |_| Some(
                "тарабарщина".to_string()
            ))
            .fullscreen
        );
        assert!(
            x11_window_conditions(&g, "1280x800", &[], |_| Some(String::new())).fullscreen
        );

        // No `WINDOW=` in the answer — nothing to ask about, the unchanged behavior,
        // and no extra process is started.
        let без_id = "X=0\nY=0\nWIDTH=1280\nHEIGHT=800\n";
        assert!(
            x11_window_conditions(без_id, "1280x800", &[], |_| panic!(
                "свойства спросили, хотя окно не назвали"
            ))
            .fullscreen
        );

        // No active window at all — the wallpaper plays.
        assert_eq!(
            x11_window_conditions("", "1280x800", &[], |_| panic!(
                "свойства спросили, хотя окна нет"
            )),
            Conditions::default()
        );

        // A window SMALLER than the screen: there is a window, but it does not cover the wallpaper.
        let маленькое = "WINDOW=7\nWIDTH=800\nHEIGHT=600\n";
        let c = x11_window_conditions(маленькое, "1280x800", &[], |_| Some(String::new()));
        assert!(c.any_window && c.focused && !c.maximized && !c.fullscreen);

        // The screen yardstick did not parse — as before, "not full-screen".
        assert!(
            !x11_window_conditions(&g, "мусор", &[], |_| Some(String::new())).fullscreen
        );
    }

    #[test]
    fn своё_окно_обоев_не_считается_окном_поверх_обоев() {
        // KWin may not name the window type at all (no `type` in the answer) — then
        // the window's owner remains. The process drawing the wallpaper cannot cover
        // it for itself.
        let own = "\
caption: Rexon Wallpaper
fullscreen: true
height: 1080
minimized: false
pid: 4242
width: 1920
";
        assert_eq!(
            kwin_window_conditions(own, 1920, 1080, &[4242]),
            Conditions::default()
        );
        // A foreign process with the same answer covers the wallpaper, as before.
        assert!(kwin_window_conditions(own, 1920, 1080, &[7]).fullscreen);
        // We know nothing about our own — the behavior is unchanged.
        assert!(kwin_window_conditions(own, 1920, 1080, &[]).fullscreen);
    }

    #[test]
    fn галочка_разворачивается_в_таблицу_и_обратно() {
        let on = actions_for_hide_when_covered(true);
        assert!(hide_when_covered(&on));
        assert_eq!(on, AutoActions::default());
        let off = actions_for_hide_when_covered(false);
        assert!(!hide_when_covered(&off));
        // The lock and a foreign session are not switched off by the check box.
        assert_eq!(off.session_locked, Action::Stop);
        assert_eq!(off.session_inactive, Action::Stop);
    }

    #[test]
    fn умолчания_новых_условий_берегут_батарею_и_игру() {
        let a = AutoActions::default();
        assert_eq!(a.on_battery, Action::Pause, "от батареи — пауза");
        assert_eq!(a.gamemode, Action::Pause, "игра — пауза");
        assert!(a.session_inactive.pauses(), "чужой сеанс — обои стоят");
        assert_eq!(CONDITIONS.len(), a.list().len());
        assert_eq!(CONDITIONS.len(), Conditions::default().list().len());
        // The names are exactly as in the foreign engine's protocol.
        assert_eq!(&CONDITIONS[6..], &["gamemode", "onBattery"]);
    }

    /// Every new condition × every action: the decision is what the action
    /// promises, and only when the condition is met.
    #[test]
    fn новые_условия_на_все_действия() {
        type Set = fn(&mut AutoActions, Action);
        type Flag = fn(&mut Conditions);
        let rows: [(&str, Set, Flag); 3] = [
            ("батарея", |a, x| a.on_battery = x, |c| c.on_battery = true),
            ("игра", |a, x| a.gamemode = x, |c| c.gamemode = true),
            ("сеанс неактивен", |a, x| a.session_inactive = x, |c| c.session_inactive = true),
        ];
        for (name, set, flag) in rows {
            for action in [Action::None, Action::Mute, Action::Pause, Action::Stop] {
                // The other conditions are off — one row is checked.
                let mut actions = actions_for_choices(SimpleChoices {
                    hide_when_covered: false,
                    pause_on_battery: false,
                    pause_in_game: false,
                });
                actions.session_locked = Action::None;
                actions.session_inactive = Action::None;
                set(&mut actions, action);
                let s = AudioSettings { actions, ..AudioSettings::default() };
                let mut c = Conditions::default();
                flag(&mut c);
                let d = s.decide(&c);
                assert_eq!(d.audio_on, !action.mutes(), "{name} × {action:?}: звук");
                assert_eq!(d.paused, action.pauses(), "{name} × {action:?}: пауза");
                assert_eq!(d.rewind, action == Action::Stop, "{name} × {action:?}: в начало");
                // The condition is not met — the action does not fire.
                let calm = s.decide(&Conditions::default());
                assert!(calm.audio_on && !calm.paused, "{name} × {action:?}: спокойно");
            }
        }
    }

    #[test]
    fn батарея_и_игра_по_умолчанию_ставят_на_паузу() {
        let s = AudioSettings::default();
        for c in [
            Conditions { on_battery: true, ..Conditions::default() },
            Conditions { gamemode: true, ..Conditions::default() },
            Conditions { session_inactive: true, ..Conditions::default() },
        ] {
            let d = s.decide(&c);
            assert!(d.paused && !d.audio_on, "{c:?}");
        }
        // An inactive session is stricter than the battery: "stop" beats "pause".
        let both = Conditions { on_battery: true, session_inactive: true, ..Conditions::default() };
        assert!(s.decide(&both).rewind);
    }

    #[test]
    fn галочки_разворачиваются_в_таблицу_и_обратно() {
        for bits in 0..8u8 {
            let c = SimpleChoices {
                hide_when_covered: bits & 1 != 0,
                pause_on_battery: bits & 2 != 0,
                pause_in_game: bits & 4 != 0,
            };
            let a = actions_for_choices(c);
            assert_eq!(choices_of(&a), c, "{c:?}");
            // The lock and a foreign session are never switched off by the check boxes.
            assert_eq!(a.session_locked, Action::Stop);
            assert_eq!(a.session_inactive, Action::Stop);
        }
        assert_eq!(SimpleChoices::default(), choices_of(&AutoActions::default()));
        assert_eq!(actions_for_choices(SimpleChoices::default()), AutoActions::default());
        // The old "when not visible" check box does not touch the new rows.
        let off = actions_for_hide_when_covered(false);
        assert_eq!(off.on_battery, Action::Pause);
        assert_eq!(off.gamemode, Action::Pause);
    }

    #[test]
    fn новые_условия_доезжают_до_протокола_чужого_движка() {
        let s = AudioSettings {
            actions: AutoActions {
                gamemode: Action::Stop,
                on_battery: Action::Mute,
                ..AutoActions::default()
            },
            ..AudioSettings::default()
        };
        let mut g = Map::new();
        s.write_engine_global(&mut g);
        let Some(Value::Object(policy)) = g.get(ENGINE_KEY_ACTIONS) else {
            panic!("нет таблицы")
        };
        // The same map that the daemon protocol's layout parses.
        let pb = crate::we_settings::map_to_auto_replay(policy);
        assert_eq!(pb.gamemode, Action::Stop.as_i64() as i32);
        assert_eq!(pb.on_battery, Action::Mute.as_i64() as i32);
        assert_eq!(pb.session_inactive, Action::Stop.as_i64() as i32);
        assert_eq!(AudioSettings::from_engine_global(&g), s);
        // A former record without the new keys — defaults, not "nothing".
        let old = map(r#"{"wallpaper_audio_actions": {"fullscreen": 0}}"#);
        let a = AudioSettings::from_map(&old).actions;
        assert_eq!((a.on_battery, a.gamemode), (Action::Pause, Action::Pause));
    }

    #[test]
    fn ответы_upower_и_gamemode_разбираются() {
        // Copied verbatim from `busctl call … Properties.Get`.
        assert_eq!(parse_dbus_bool("v b false\n"), Some(false));
        assert_eq!(parse_dbus_bool("v b true\n"), Some(true));
        assert_eq!(parse_dbus_int("v i 0\n"), Some(0));
        assert_eq!(parse_dbus_int("v i 2\n"), Some(2));
        // The service is not running: busctl writes an error to stderr, stdout is empty.
        assert_eq!(parse_dbus_bool(""), None);
        assert_eq!(parse_dbus_int(""), None);
        assert_eq!(parse_dbus_bool("b true"), None, "без подписи варианта — не наш ответ");
        assert_eq!(parse_dbus_int("v i много"), None);
        // We do not start the service via the bus: game mode does not start from polling.
        for argv in [upower_on_battery_argv(), gamemode_client_count_argv()] {
            assert_eq!(argv[0], "busctl");
            assert!(argv.iter().any(|a| a == "--auto-start=no"), "{argv:?}");
            assert!(argv.iter().any(|a| a == "call"));
        }
        assert!(upower_on_battery_argv().contains(&"--system".to_string()));
        assert!(gamemode_client_count_argv().contains(&"--user".to_string()));
        assert_eq!(upower_on_battery_argv().last().map(String::as_str), Some("OnBattery"));
        assert_eq!(gamemode_client_count_argv().last().map(String::as_str), Some("ClientCount"));
    }

    #[test]
    fn батарея_из_sys_и_gamescope() {
        let st = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(sysfs_on_battery(&st(&["Discharging\n"])));
        assert!(!sysfs_on_battery(&st(&["Charging\n"])));
        assert!(!sysfs_on_battery(&st(&["Full\n", "Not charging\n"])));
        assert!(!sysfs_on_battery(&[]), "ПК без батареи — никогда");
        assert!(is_gamescope_comm("gamescope\n"));
        assert!(is_gamescope_comm("gamescope-wl\n"));
        assert!(!is_gamescope_comm("gamescope-session"));
        assert!(!is_gamescope_comm("steam"));
    }

    #[test]
    fn сеанс_logind_разбирается() {
        assert_eq!(parse_logind_session("Active=yes\nLockedHint=no\n"), (false, false));
        assert_eq!(parse_logind_session("Active=no\nLockedHint=no\n"), (true, false));
        assert_eq!(parse_logind_session("Active=yes\nLockedHint=yes\n"), (false, true));
        // logind is silent or there is no session — the wallpaper does not stop forever.
        assert_eq!(parse_logind_session(""), (false, false));
        assert_eq!(parse_logind_session("мусор"), (false, false));
        assert_eq!(logind_session_argv("2")[2], "2");
        assert_eq!(logind_session_argv("")[2], "auto", "вне сеанса — свой графический сеанс");
        assert_eq!(logind_session_argv("--help")[2], "auto", "ключ под видом номера");
    }

    #[test]
    fn хранитель_экрана_разбирается() {
        assert!(parse_screensaver_active("true\n"));
        assert!(!parse_screensaver_active("false\n"));
        assert!(!parse_screensaver_active(""));
    }
}
