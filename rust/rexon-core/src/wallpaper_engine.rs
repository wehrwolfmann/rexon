//! Wallpaper engine decisions — port of `rexon/wallpaper_engine.py`.
//!
//! The module answers one question: WHICH command line applies the wallpaper
//! in this environment. Hence the layering adopted in the project: here is
//! only the deterministic part (input → plan), while execution (`safe_run`,
//! `subprocess.Popen`, process signals, Pillow, threads) stays in the
//! `wallpaper_engine.py` Python shim, which executes what the core decided.
//!
//! Observations of the environment come in as PARAMETERS: which binaries
//! exist on the system, the monitor name, the screen resolution, the package
//! name hint for the current distribution. The core asks the machine nothing
//! by itself — otherwise it could not be run against a golden file.
//!
//! Parity with the live Python is the reference
//! `rust/rexon-py/tests/fixtures/wallpaper_engine_reference.json`: it was
//! captured from the Python version BEFORE the port (155 scenarios, 156
//! external calls) and verified by mutation — eight corruptions of values in
//! argv, backend preference order, texts and tables break the comparison.
//!
//! ⚠️ A fix during the port (rule #16): the "scene" format launched
//! `python -m rexon.parallax_engine`, and that module has not been in the
//! tree since July — the process started and died right away, while the
//! customer was shown «Scene Engine started». Now it is an honest refusal,
//! as for application wallpapers.

use serde::Serialize;

// File system checks for finding the video wallpaper host binary. The trait
// is shared with the waywallen daemon lookup: two methods (`is_exec_file`,
// `which`) describe exactly the same side effects, and there is no point in
// a second one.
use crate::we_client::DaemonFs;
// The compositor+shell pair is detected in system.rs — in the same place as the compositor.
use crate::i18n::{tr, trc, trf};
use crate::system::DesktopShell;

// ── environment ────────────────────────────────────────────────────────────

/// Compositors without their own "wallpaper setting": the background is set
/// by a separate daemon (swww/hyprpaper/swaybg/wbg), one path for all.
pub const WLROOTS_DE: &[&str] = &[
    "cosmic", "hyprland", "labwc", "niri", "river", "sway", "wayfire",
];

/// The current DE by `XDG_CURRENT_DESKTOP` / `DESKTOP_SESSION`.
///
/// There is no environment table of its own here (rule #12): the answer
/// comes from the shared [`crate::system::detect_current_de`]. The engine's
/// former table searched for substrings, called Cinnamon `cinnamon`
/// (everywhere else in the product it is `cinnamon-mint`) and did not
/// recognize Plasma by the session name. That is why all engine branches
/// below accept both Cinnamon ids: both the core's answer and previously
/// saved values arrive here.
///
/// Not recognized — `generic`: the common path (feh / background daemon), as
/// before. The Wayland flag is deliberately `false`: an unfamiliar compositor
/// must not turn into "an environment named after the first token", the
/// engine has one branch for it — the common one.
pub fn detect_de(desktop: &str, session: &str) -> String {
    crate::system::detect_current_de(desktop, session, false)
        .unwrap_or_else(|| "generic".to_string())
}

/// `wayland` / `x11` / `unknown` by three session variables.
pub fn detect_session_type(
    session_type: &str,
    wayland_display: &str,
    display: &str,
) -> String {
    let st = session_type.to_lowercase();
    if st == "wayland" || st == "x11" {
        return st;
    }
    if !wayland_display.is_empty() {
        return "wayland".to_string();
    }
    if !display.is_empty() {
        return "x11".to_string();
    }
    "unknown".to_string()
}

/// A compositor without its own wallpaper setting?
pub fn is_wlroots_de(de: &str) -> bool {
    WLROOTS_DE.contains(&de)
}

/// Name of the first monitor from a `wlr-randr --json` / `hyprctl monitors -j` answer.
/// Did not parse — `None`, the caller substitutes `*`.
pub fn parse_monitor_json(stdout: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(stdout).ok()?;
    let first = value.as_array()?.first()?;
    Some(
        first
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("*")
            .to_string(),
    )
}

/// Resolution of the active mode from `xrandr --current` (the line with the asterisk).
pub fn parse_xrandr_resolution(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .find(|line| line.contains('*'))
        .and_then(|line| line.trim().split_whitespace().next())
        .map(|s| s.to_string())
}

// ── plan step ──────────────────────────────────────────────────────────────

/// What the shim does with the step.
pub mod step_kind {
    /// `safe_run(argv)`; success is code 0.
    pub const RUN: &str = "run";
    /// `Popen(argv)` — the live wallpaper process (`self._process`).
    pub const SPAWN: &str = "spawn";
    /// `Popen(argv, start_new_session=True)` — the live wallpaper process.
    pub const SPAWN_SESSION: &str = "spawn_session";
    /// `Popen(argv, start_new_session=True)` — the process HOLDING the static
    /// background (swaybg/wbg); it outlives the live wallpaper.
    pub const SPAWN_HOLD: &str = "spawn_hold";
    /// `plasma_evaluate_script(argv[0])`.
    pub const PLASMA_SCRIPT: &str = "plasma_script";
    /// List the xfconf properties and write the path into each `/last-image`.
    pub const XFCONF_ALL: &str = "xfconf_all";
    /// Launch nothing — refuse right away with `err_message`.
    pub const FAIL: &str = "fail";
    /// COSMIC: write the picture into cosmic-bg's native key
    /// ([`super::apply_cosmic_bg`]) — without a command and without a process of our own.
    pub const COSMIC_BG: &str = "cosmic_bg";
}

/// How many seconds a `spawn*` step waits for the process it started to be ALIVE.
///
/// ⚠️ A fix for a whole class of "reporting success about a dead launch". All
/// `spawn`, `spawn_session` and `spawn_hold` steps start a process that draws
/// (or holds) the desktop while it lives; yet success was taken to be the
/// mere fact that the launch did not throw. There were two consequences, both
/// serious:
///
/// 1. The customer got a green «wallpaper started» on an EMPTY desktop: the
///    previous wallpaper is stopped before applying, and the new process died
///    within milliseconds (`python -m rexon.web_wallpaper` — the module is not
///    in the tree at all).
/// 2. The plan's fallback steps became unreachable: `python3` always exists,
///    so the first step "succeeded" — and `surf`/`xwinwrap` on X11 NEVER got
///    their turn.
///
/// The value was chosen by measurement on the developer's machine (CachyOS,
/// 23.08.2026): an instant failure — `python -m` without the module — fits in
/// 10 ms; the slowest observed failure, when the process first loads a heavy
/// stack and only then crashes (GTK+WebKit2 — 190 ms, mpv on a non-existent
/// file — 93 ms), fits in 0.2 s. 0.5 s gives a 2.6x margin and is unnoticeable
/// next to applying the wallpaper itself. Going higher is not allowed: the
/// delay is paid on EVERY format and on every background restore after
/// «Stop»; lower — false success returns on slowly failing backends.
///
/// The wait must not be "sleep for so long" but polling: a dead launch gives
/// a refusal right away, only a live one pays the full delay.
pub const SPAWN_ALIVE_GRACE_SEC: f64 = 0.5;

/// The "is the process alive" poll step within [`SPAWN_ALIVE_GRACE_SEC`], seconds.
///
/// Polling, not sleeping: a dead launch must give a refusal right away, only
/// a live one pays the full grace period. 20 ms is 25 probes for the whole
/// grace period, cheaper than the launch itself and finer than any observed
/// failure (10 ms for `python -m` without the module — the only one that fits
/// in a step, and even it is caught within two probes).
pub const ALIVE_POLL_STEP_SEC: f64 = 0.02;

/// How long to wait for the death of the wallpaper process being stopped ON EACH signal, seconds.
///
/// The stop order is the same for all engine processes: SIGTERM to the group
/// → wait → SIGKILL to the group → wait. Five seconds — the same as Python
/// gave, and it is not arbitrary: on SIGTERM mpv/mpvpaper release the surface
/// and close the output rather than dying instantly, and killing them too
/// early means leaving the garbage of the last frame on screen.
pub const KILL_WAIT_SEC: f64 = 5.0;

/// Timeout of the `evaluateScript` call in plasmashell, seconds.
///
/// The script runs IN THE MAIN THREAD of the shell, so the wait has to be as
/// long as a Plasma busy with repainting makes it.
pub const PLASMA_SCRIPT_TIMEOUT_SEC: f64 = 10.0;

/// Timeout of short desktop commands (icons, plugin reset), seconds.
pub const DESKTOP_CMD_TIMEOUT_SEC: f64 = 5.0;

/// Query of an already running swww daemon, seconds.
pub const SWWW_QUERY_TIMEOUT_SEC: f64 = 3.0;

/// Starting the daemon with the old `swww init` subcommand, seconds.
pub const SWWW_INIT_TIMEOUT_SEC: f64 = 5.0;

/// Pause after launching a separate `swww-daemon`, seconds: it brings up its
/// socket not instantly, and the very next `swww img` without this pause
/// would run into "no connection" out of nowhere.
pub const SWWW_DAEMON_SETTLE_SEC: f64 = 0.5;

/// Text for `{stderr}` when the wallpaper process died right after launch.
///
/// The shim deliberately does not read the process's own output: the pipe
/// would fill up on a live wallpaper and choke the host, and a file would grow
/// the whole time it runs (`/tmp` here is RAM). The exit code answers the
/// "why is it empty" question exactly as much as the customer needs.
pub fn spawn_died_message(code: Option<i32>) -> String {
    match code {
        Some(c) => trf("the process exited right after starting (code %1)", &[&c]),
        None => tr("the process exited right after starting"),
    }
}

/// One wallpaper apply step. Steps go in order; the first successful one ends
/// the apply, a failure leads to the next one — except `terminal`, which is
/// responsible for the outcome itself.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Step {
    /// How to execute — see [`step_kind`].
    pub kind: String,
    /// Backend name for the journal.
    pub backend: String,
    /// Start the swww daemon before the main command.
    pub ensure_swww: bool,
    /// Commands before the main one; their exit code is not looked at.
    pub pre: Vec<Vec<String>>,
    /// The main command (for `plasma_script` the only element is the JS itself).
    pub argv: Vec<String>,
    /// Commands after success; their exit code is not looked at.
    pub post: Vec<Vec<String>>,
    /// Success message.
    pub ok_message: String,
    /// Error message; the shim substitutes `{stderr}`.
    pub err_message: String,
    /// Second error message (for `xfconf_all` — «no monitors»).
    pub alt_message: String,
    /// The format written into the engine state on success.
    pub format: String,
    /// A failure of this step ends the apply with a refusal.
    pub terminal: bool,
    /// `safe_run` returned `None` (timeout/no binary) — count as success.
    pub ok_on_none: bool,
    /// Timeout of the main command, seconds.
    pub timeout: Option<u64>,
    /// A check that the wallpaper is REALLY written: after a successful exit code
    /// the executor reads this file and compares the key with the wallpaper path
    /// ([`ini_check_passes`]). Needed where the exit code proves nothing
    /// (pcmanfm-qt's `--set-wallpaper` always answers zero).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_ini: Option<IniCheck>,
    /// On a successful step the compositor sets the background itself — our
    /// holding process (swaybg/wbg) is no longer needed and is stopped.
    pub drops_holder: bool,
}

/// Where a program keeps the wallpaper path: file (`~/.config/…`), group, key.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IniCheck {
    pub file: String,
    pub section: String,
    pub key: String,
}

/// Value of the `key` key in the `[section]` group of an INI text (Qt
/// `QSettings`: the path may be quoted; spaces around `=` are allowed).
pub fn ini_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_section = t[1..t.len() - 1].trim() == section;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            if k.trim() == key {
                let v = v.trim();
                let v = v
                    .strip_prefix('"')
                    .and_then(|x| x.strip_suffix('"'))
                    .unwrap_or(v);
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Whether the program wrote exactly this wallpaper: the file exists, the key
/// is found and equals the path. No file, no key, another path — NOT written,
/// there will be no «✅».
pub fn ini_check_passes(text: Option<&str>, check: &IniCheck, path: &str) -> bool {
    text.and_then(|t| ini_value(t, &check.section, &check.key))
        .is_some_and(|v| v == path)
}

/// The pcmanfm-qt settings file in the LXQt profile.
pub const PCMANFM_QT_LXQT_SETTINGS: &str = "~/.config/pcmanfm-qt/lxqt/settings.conf";

impl Step {
    fn new(kind: &str, backend: &str, argv: Vec<String>) -> Self {
        Step {
            kind: kind.to_string(),
            backend: backend.to_string(),
            ensure_swww: false,
            pre: Vec::new(),
            argv,
            post: Vec::new(),
            ok_message: String::new(),
            err_message: String::new(),
            alt_message: String::new(),
            format: "static".to_string(),
            terminal: false,
            ok_on_none: false,
            timeout: None,
            verify_ini: None,
            drops_holder: false,
        }
    }

    fn ok(mut self, msg: impl Into<String>) -> Self {
        self.ok_message = msg.into();
        self
    }

    fn err(mut self, msg: impl Into<String>) -> Self {
        self.err_message = msg.into();
        self
    }

    fn fmt(mut self, format: &str) -> Self {
        self.format = format.to_string();
        self
    }

    fn terminal(mut self) -> Self {
        self.terminal = true;
        self
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn fail(message: impl Into<String>) -> Step {
    Step::new(step_kind::FAIL, "нет", Vec::new()).err(message)
}

// ── static wallpapers ──────────────────────────────────────────────────────

/// gsettings schema and key for the environment (empty — not via gsettings).
pub fn gsettings_schema(de: &str) -> Option<(&'static str, &'static str)> {
    match de {
        // Pantheon: the background is drawn by Gala from org.gnome.desktop.background
        // (pantheon-default-settings sets picture-uri and -dark there).
        "gnome" | "budgie" | "pantheon" | "unity" => {
            Some(("org.gnome.desktop.background", "picture-uri"))
        }
        "cinnamon" | "cinnamon-mint" => {
            Some(("org.cinnamon.desktop.background", "picture-uri"))
        }
        "mate" => Some(("org.mate.background", "picture-filename")),
        _ => None,
    }
}

/// Reading the current value of a gsettings key — this is how the engine
/// learns the wallpaper to restore after stopping a live one.
pub fn gsettings_get_argv(schema: &str, key: &str) -> Vec<String> {
    argv(&["gsettings", "get", schema, key])
}

/// The dark wallpaper key next to [`gsettings_schema`]: GNOME and its
/// neighbors with a dark style (`color-scheme = prefer-dark`) show
/// `picture-uri-dark` on the desktop. `None` — the environment has no such key.
pub fn gsettings_dark_key(de: &str) -> Option<(&'static str, &'static str)> {
    match gsettings_schema(de)? {
        ("org.gnome.desktop.background", _) => Some(("org.gnome.desktop.background", "picture-uri-dark")),
        _ => None,
    }
}

/// A gsettings value in `gsettings get` output (`'file:///a b.jpg'`) — the
/// path without quotes and `file://`. Empty — the key is empty.
pub fn gsettings_path_value(out: &str) -> String {
    out.trim().trim_matches(['\'', '"']).replace("file://", "")
}

/// Whether the style is dark, by the output of `gsettings get … color-scheme`.
pub fn prefers_dark(color_scheme_out: &str) -> bool {
    gsettings_path_value(color_scheme_out) == "prefer-dark"
}

/// The xfconf property with the main desktop picture from the output of
/// `xfconf-query -c xfce4-desktop -l`: the first monitor, the first workspace
/// (`…/monitor<name>/workspace0/last-image`), otherwise the first `last-image`.
pub fn xfce_last_image_prop(list: &str) -> Option<String> {
    let props: Vec<&str> = list.lines().map(str::trim).filter(|l| l.ends_with("/last-image")).collect();
    props
        .iter()
        .find(|p| p.contains("/workspace0/"))
        .or_else(|| props.first())
        .map(|p| p.to_string())
}

/// Reading one xfconf property.
pub fn xfconf_get_argv(prop: &str) -> Vec<String> {
    argv(&["xfconf-query", "-c", "xfce4-desktop", "-p", prop])
}

/// The program used to grab a frame and do the faststart remux.
pub const FFMPEG_BIN: &str = "ffmpeg";

/// VLC players: the engine considers VLC available if any of them exists.
pub const VLC_BINS: &[&str] = &["cvlc", "vlc"];

/// Programs for which the engine asks for a package name hint in the current
/// distribution. The names match the fields of [`Hints`].
pub const HINT_TOOLS: &[&str] = &[
    "mpvpaper", "xwinwrap", "cvlc", "webkit2gtk-4.1", "swww", "yt-dlp",
];

/// The result of a successful static wallpaper set — ONE phrase for all paths.
///
/// ⭐ A fix (rule #13): previously the parentheses held the internal name of
/// the path that set the wallpaper — «Wallpaper set (generic)», «(KDE D-Bus)»,
/// «(swaybg)». That word tells the customer nothing, and «generic» also read
/// like an error. Which path set it is known to the journal: the executor
/// writes there the [`Step::backend`] of the step that succeeded.
pub fn wallpaper_set_message() -> String {
    tr("Wallpaper set")
}

// ── Cinnamon: the desktop picture list (Cinnamon 6.8+) ─────────────────────
//
// Since Cinnamon 6.8 the background is drawn by the `cinnamon-background-daemon`
// service, and it takes the picture from the `picture-uri-list` key (`aa{sv}`:
// a dictionary per monitor, in `background-mode = 'mirror'` mode one entry
// for all). The shell now only intercepts the former `picture-uri` (10 s
// after startup): moves it into the list with `picture-options = 'zoom'` and
// clears it right away — so writing it on 6.8 is not allowed (it would break
// the fitting mode), and reading it is useless (it is empty). Old Cinnamon
// does not know the list — it still gets `picture-uri`. Which key to write
// is decided by the script itself from the schema (`gsettings list-keys`);
// the version number is not looked at.

/// The Cinnamon background schema.
pub const CINNAMON_BG_SCHEMA: &str = "org.cinnamon.desktop.background";
/// The picture list key (Cinnamon 6.8+).
pub const CINNAMON_BG_LIST_KEY: &str = "picture-uri-list";
/// The list mode key (`mirror` / `independent`).
pub const CINNAMON_BG_MODE_KEY: &str = "background-mode";

/// A GVariant string in single quotes (`'…'`, `\` and `'` are escaped inside).
pub fn gvariant_quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The Cinnamon static picture script: on 6.8+ one `picture-uri-list` entry
/// (fitting and colors from the previous keys, as the shell itself does on
/// «Set as wallpaper»), `mirror` mode; on the old one — `picture-uri`. The
/// picture comes as arguments (`$1` — the GVariant string, `$2` — the URI)
/// rather than being pasted into the script text: that way the shell does
/// not touch `$`, quotes and `\\` in the file name. The command line is
/// [`cinnamon_static_argv`].
pub const CINNAMON_STATIC_SCRIPT: &str = "s=org.cinnamon.desktop.background
if gsettings list-keys \"$s\" 2>/dev/null | grep -qx picture-uri-list; then
o=$(gsettings get \"$s\" picture-options) || o=\"'zoom'\"
c=$(gsettings get \"$s\" color-shading-type) || c=\"'solid'\"
p=$(gsettings get \"$s\" primary-color) || p=\"'#000000'\"
q=$(gsettings get \"$s\" secondary-color) || q=\"'#000000'\"
gsettings set \"$s\" picture-uri-list \"[{'picture-uri': <$1>, 'picture-options': <$o>, 'color-shading-type': <$c>, 'primary-color': <$p>, 'secondary-color': <$q>}]\" && gsettings set \"$s\" background-mode mirror
else
gsettings set \"$s\" picture-uri \"$2\"
fi
";

/// The [`CINNAMON_STATIC_SCRIPT`] command line for the picture `path`.
pub fn cinnamon_static_argv(path: &str) -> Vec<String> {
    let uri = format!("file://{path}");
    vec![
        "sh".to_string(),
        "-c".to_string(),
        CINNAMON_STATIC_SCRIPT.to_string(),
        "sh".to_string(),
        gvariant_quote(&uri),
        uri,
    ]
}

fn cinnamon_static_step(path: &str) -> Step {
    Step::new(step_kind::RUN, "gsettings", cinnamon_static_argv(path))
        .ok(wallpaper_set_message())
        .err(tr("gsettings error"))
        .terminal()
}

/// The first picture from a `picture-uri-list` value (`gsettings get` output)
/// — the path without `file://`. `None` — the list is empty or not parsed.
pub fn cinnamon_list_first_path(list: &str) -> Option<String> {
    let at = list.find("'picture-uri': <'")? + "'picture-uri': <'".len();
    let rest = &list[at..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => out.push(chars.next()?),
            '\'' => break,
            c => out.push(c),
        }
    }
    let path = out.strip_prefix("file://").unwrap_or(&out).to_string();
    (!path.is_empty()).then_some(path)
}

fn gsettings_step(path: &str, schema: &str, key: &str) -> Step {
    if schema == CINNAMON_BG_SCHEMA {
        return cinnamon_static_step(path);
    }
    // GNOME expects a file:// URI, MATE a plain path.
    let value = if key.contains("picture-uri") {
        format!("file://{path}")
    } else {
        path.to_string()
    };
    let mut step = Step::new(
        step_kind::RUN,
        "gsettings",
        argv(&["gsettings", "set", schema, key, &value]),
    )
    .ok(wallpaper_set_message())
    .err(tr("gsettings error"))
    .terminal();
    // GNOME 42+ keeps a separate picture for the dark theme.
    if key == "picture-uri" {
        step.post
            .push(argv(&["gsettings", "set", schema, "picture-uri-dark", &value]));
    }
    // MATE: the lock screen follows the wallpaper if it still has the stock
    // «stripes».
    if schema == "org.mate.background" {
        step.post.push(mate_lock_follows_desktop_argv());
    }
    step
}

// ── MATE: the lock screen picture ─────────────────────────────────────────
//
// mate-screensaver draws the `org.mate.screensaver picture-filename` picture
// under the password window, and its default is the stock «stripes»
// (`…/backgrounds/mate/desktop/Stripes.png`): you change the wallpaper — the
// lock screen still shows stripes. If there is no file at the key (an empty
// string included), mate-screensaver takes the desktop wallpaper with all its
// settings (`gs-manager.c`: `mate_bg_load_from_preferences`, then
// `mate_bg_set_filename` only when `g_file_test(…, EXISTS)`). So an empty
// value means "like the desktop wallpaper", and it also follows wallpaper
// changes made in MATE itself, not only in Rexon. A picture of the person's
// own choosing is left alone: only the stock default is changed.

/// The MATE lock screen picture schema.
pub const MATE_SCREENSAVER_SCHEMA: &str = "org.mate.screensaver";

/// Path tail of the stock MATE lock picture (the prefix differs between
/// distributions).
pub const MATE_STOCK_LOCK_PICTURE: &str = "/backgrounds/mate/desktop/Stripes.png";

/// Whether the MATE lock screen has the stock picture — by the output of
/// `gsettings get org.mate.screensaver picture-filename`.
pub fn mate_lock_picture_is_stock(gsettings_out: &str) -> bool {
    gsettings_path_value(gsettings_out).ends_with(MATE_STOCK_LOCK_PICTURE)
}

/// The "lock screen like the desktop wallpaper, if the stock picture is there"
/// command: a single one, without reading back in Rust, because it runs as the
/// tail of the wallpaper step.
pub fn mate_lock_follows_desktop_argv() -> Vec<String> {
    let script = format!(
        "v=$(gsettings get {MATE_SCREENSAVER_SCHEMA} picture-filename) || exit 0\n\
         case \"$v\" in *'{MATE_STOCK_LOCK_PICTURE}'\\') gsettings set {MATE_SCREENSAVER_SCHEMA} picture-filename '' ;; esac\n"
    );
    argv(&["sh", "-c", &script])
}

/// The "lock screen like the desktop wallpaper" command without the condition
/// (the caller checks it with [`mate_lock_picture_is_stock`]).
pub fn mate_lock_as_desktop_set_argv() -> Vec<String> {
    argv(&["gsettings", "set", MATE_SCREENSAVER_SCHEMA, "picture-filename", ""])
}

/// Session type for [`static_plan`] for Deepin in a Treeland session (set by
/// the executor from the compositor, [`crate::dde::treeland_session_live`]).
pub const TREELAND_SESSION: &str = "treeland";

/// Deepin — only in a Treeland session: the wallpaper is accepted by the
/// Control Center ([`crate::dde::control_center_wallpaper_argv`]) — the
/// appearance service does not set wallpapers there. A third-party feh/swaybg
/// will not do: the dde-shell background would lie on top. There is nothing to
/// read the result from Treeland with — the message says "handed over", not
/// "set". Outside Treeland — a refusal ([`crate::dde::not_treeland_message`]).
fn deepin_static_steps(path: &str, treeland: bool) -> Vec<Step> {
    if !treeland {
        return vec![fail(crate::dde::not_treeland_message())];
    }
    let mut st = Step::new(step_kind::RUN, "deepin-control-center", crate::dde::control_center_wallpaper_argv(path, false))
        .ok(crate::dde::wallpaper_handed_message());
    st.timeout = Some(70);
    vec![st, fail(crate::dde::control_center_failed_message())]
}

fn kde_static_steps(path: &str, tools: &[String]) -> Vec<Step> {
    if has(tools, "plasma-apply-wallpaperimage") {
        return vec![Step::new(
            step_kind::RUN,
            "plasma-apply-wallpaperimage",
            argv(&["plasma-apply-wallpaperimage", path]),
        )
        .ok(wallpaper_set_message())
        .err({
            // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
            tr("plasma-apply-wallpaperimage error: {stderr}")
        })
        .terminal()];
    }
    vec![Step::new(
        step_kind::PLASMA_SCRIPT,
        "kde-dbus",
        vec![kde_static_script(path)],
    )
    .ok(wallpaper_set_message())
    .err(tr("Couldn't set the wallpaper on KDE"))
    .terminal()]
}

fn xfce_static_steps(path: &str) -> Vec<Step> {
    let mut step = Step::new(
        step_kind::XFCONF_ALL,
        "xfconf-query",
        argv(&["xfconf-query", "-c", "xfce4-desktop", "-l"]),
    )
    .ok(wallpaper_set_message())
    .err(tr("xfconf-query didn't respond"))
    .terminal();
    // `{prop}` is substituted by the shim for each found `/last-image` property.
    step.post.push(argv(&[
        "xfconf-query", "-c", "xfce4-desktop", "-p", "{prop}", "-s", path,
    ]));
    step.alt_message = tr("No Xfce monitors found");
    vec![step]
}

fn wayland_daemon_steps(de: &str, path: &str, tools: &[String], swww_hint: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    // Order: swww (can "set and exit") → hyprpaper (where it is already
    // configured) → swaybg/wbg as a holding process.
    if has(tools, "swww") {
        let mut step = Step::new(step_kind::RUN, "swww", argv(&["swww", "img", path]))
            .ok(wallpaper_set_message());
        step.ensure_swww = true;
        steps.push(step);
    }
    if has(tools, "hyprpaper") {
        let mut step = Step::new(
            step_kind::RUN,
            "hyprpaper",
            argv(&["hyprctl", "hyprpaper", "wallpaper", &format!(",{path}")]),
        )
        .ok(wallpaper_set_message());
        step.pre
            .push(argv(&["hyprctl", "hyprpaper", "preload", path]));
        steps.push(step);
    }
    // swaybg/wbg do not "set and exit" — they HOLD the background while alive.
    // The shim stops the previous such process itself, otherwise every wallpaper
    // change would leave one more live process behind.
    for (tool, cmd) in [
        ("swaybg", argv(&["swaybg", "-i", path, "-m", "fill"])),
        ("wbg", argv(&["wbg", path])),
    ] {
        if has(tools, tool) {
            steps.push(
                Step::new(step_kind::SPAWN_HOLD, tool, cmd)
                    .ok(wallpaper_set_message())
                    .err({
                        // TRANSLATORS: %1 is a program name; keep {stderr} as is:
                        // the program replaces it with the error output.
                        trf("Couldn't start %1: {stderr}", &[&tool])
                    })
                    .terminal(),
            );
            break;
        }
    }
    // The trailing refusal ALWAYS goes, not only when there is nothing to set
    // with: a daemon may be installed and still fail (`swww img` returned
    // non-zero). Then the customer needs the same thing — WHAT to install, not
    // silence. For a successful apply the step is unreachable, so it does not
    // affect the call journal.
    let hint = if swww_hint.is_empty() { "swww" } else { swww_hint };
    // ⭐ A fix (rule #13): a raw engine identifier stood here — «Wallpapers on
    // generic need a background program». The human name and the proper form for
    // an unrecognized desktop come from the shared `place_de_name`.
    steps.push(fail(crate::screenshots::place_de_name(
        // TRANSLATORS: %1 is the desktop name, %2 a hint on how to install swww.
        &trf(
            "Wallpapers on %1 need a background program. \
             Install swww (%2) — or swaybg/hyprpaper/wbg.",
            &[&"%1", &hint],
        ),
        "%1",
        &crate::screenshots::display_name(de),
    )));
    steps
}

fn fallback_static_steps(
    de: &str,
    session_type: &str,
    path: &str,
    tools: &[String],
    swww_hint: &str,
) -> Vec<Step> {
    let mut steps = Vec::new();
    if session_type == "x11" && has(tools, "feh") {
        steps.push(
            Step::new(step_kind::RUN, "feh", argv(&["feh", "--bg-fill", path]))
                .ok(wallpaper_set_message()),
        );
    }
    if session_type == "wayland" {
        // Previously any unrecognized Wayland session went to the GNOME schema
        // gsettings: the command silently finished "successfully", yet the wallpaper
        // did not change, because nobody reads the schema.
        steps.extend(wayland_daemon_steps(de, path, tools, swww_hint));
    } else {
        steps.push(gsettings_step(
            path,
            "org.gnome.desktop.background",
            "picture-uri",
        ));
    }
    steps
}

/// Binaries whose presence the engine plans ask about. The shim tries exactly
/// this list and passes what it found as the `tools` parameter — so that the
/// program names do not have a SECOND copy in Python: there they would quietly
/// drift apart from the plans.
pub const PLAN_TOOLS: &[&str] = &[
    "feh", "hyprpaper", "mpv", "mpvpaper", "pcmanfm-qt",
    "plasma-apply-wallpaperimage", "surf", "swaybg", "swww", "wbg", "xwinwrap",
    "cosmic-bg", "swaymsg",
];

/// Environments whose desktop is configured through Plasma (its desktops
/// config, the wallpaper plugin and `evaluateScript`). A separate question,
/// because it determines three branches at once: what to remember before a
/// wallpaper change, how to cure a stuck plugin and how to restore the desktop
/// after a stop.
pub fn uses_plasma_config(de: &str) -> bool {
    de == "kde"
}

/// Environments whose wallpaper the engine saves before replacing it and
/// restores after a stop. On the others there is nothing to restore: we did
/// not read the background there, and an extra command would change a
/// wallpaper we never touched.
pub const RESTORE_DES: &[&str] = &[
    "budgie", "cinnamon", "cinnamon-mint", "cosmic", "gnome", "kde", "mate", "pantheon", "xfce",
];

/// Plan for restoring the saved wallpaper after stopping a live one.
///
/// It is the same path as a normal static picture set, but only for the
/// [`RESTORE_DES`] environments; the session type is not asked here — none of
/// these branches depends on it.
pub fn restore_static_plan(
    de: &str,
    path: &str,
    tools: &[String],
    swww_hint: &str,
) -> Vec<Step> {
    if !RESTORE_DES.contains(&de) {
        return Vec::new();
    }
    static_plan(de, "", path, tools, swww_hint)
}

/// Plan for applying a static wallpaper: ordered attempts on the current DE.
///
/// `tools` — which binaries were found on the system; `swww_hint` — the hint
/// about the background package for the current distribution
/// (`tool_package_hint("swww")`).
pub fn static_plan(
    de: &str,
    session_type: &str,
    path: &str,
    tools: &[String],
    swww_hint: &str,
) -> Vec<Step> {
    if de == "kde" {
        return kde_static_steps(path, tools);
    }
    if de == "xfce" {
        return xfce_static_steps(path);
    }
    if de == "deepin" {
        return deepin_static_steps(path, session_type == TREELAND_SESSION);
    }
    if let Some((schema, key)) = gsettings_schema(de) {
        return vec![gsettings_step(path, schema, key)];
    }
    if de == "lxqt" {
        // The LXQt desktop is drawn by pcmanfm-qt, which also owns the wallpaper:
        // feh will not do here — the background is redrawn on top and the wallpaper
        // "falls off". No file manager (bare openbox+lxqt-panel) — the common path.
        let mut steps = Vec::new();
        if has(tools, "pcmanfm-qt") {
            let mut step = Step::new(
                step_kind::RUN,
                "pcmanfm-qt",
                argv(&["pcmanfm-qt", "--wallpaper-mode=fit"]),
            )
            .ok(wallpaper_set_message());
            step.pre.push(argv(&["pcmanfm-qt", "--set-wallpaper", path]));
            // `--set-wallpaper` answers zero even with an already running instance;
            // an error is told apart only by the second command's code.
            step.ok_on_none = true;
            // ⚠️ D12: a zero code does not yet mean "the wallpaper is set" — check
            // that pcmanfm-qt wrote the path into its settings. If not, the step
            // failed, and the person will not see a false «✅».
            step.verify_ini = Some(IniCheck {
                file: PCMANFM_QT_LXQT_SETTINGS.to_string(),
                section: "Desktop".to_string(),
                key: "Wallpaper".to_string(),
            });
            steps.push(step);
        }
        steps.extend(fallback_static_steps(de, session_type, path, tools, swww_hint));
        return steps;
    }
    if de == "cosmic" {
        return cosmic_static_steps(path, tools, swww_hint);
    }
    if de == "sway" {
        let mut steps: Vec<Step> = sway_bg_step(path, tools).into_iter().collect();
        steps.extend(wayland_daemon_steps(de, path, tools, swww_hint));
        return steps;
    }
    if is_wlroots_de(de) {
        return wayland_daemon_steps(de, path, tools, swww_hint);
    }
    fallback_static_steps(de, session_type, path, tools, swww_hint)
}

/// sway draws the background itself — with its own swaybg on the `output * bg`
/// command (sway(5)). We ask it instead of starting a second swaybg next to
/// it: the holding process is not available to «Appearance» at all, and the
/// wallpaper tab would hold two backgrounds on one layer. sway passes the path
/// to wordexp: in single quotes (inside double ones — command parsing strips
/// those) it stays one word with a space, an apostrophe and `$`. A double
/// quote, a backslash, `$` (sway would substitute its own variable) and a
/// double space (sway escapes it itself, and the path stops being found)
/// cannot be carried this way — then there is no step, and swaybg holds the
/// background. Verified on sway 1.12 headless.
fn sway_bg_step(path: &str, tools: &[String]) -> Option<Step> {
    if !has(tools, "swaymsg")
        || path.contains("  ")
        || path.chars().any(|c| c.is_control() || matches!(c, '"' | '\\' | '$'))
    {
        return None;
    }
    let quoted = format!("\"'{}'\"", path.replace('\'', "'\\''"));
    let mut step = Step::new(step_kind::RUN, "sway", argv(&["swaymsg", "output", "*", "bg", &quoted, "fill"]))
        .ok(wallpaper_set_message());
    step.drops_holder = true;
    Some(step)
}

// ── COSMIC: cosmic-bg's native key ─────────────────────────────────────────
//
// The COSMIC background is drawn by cosmic-bg — the same Background layer
// surface as swww/swaybg have. A foreign daemon on top of it is a lottery of
// creation order, and after re-login cosmic-bg shows its own again. So we
// write the picture to the same place «COSMIC Settings → Wallpaper» writes
// it, and cosmic-bg picks the key up itself (it is watched by
// `ConfigWatchSource`).
//
// The format was verified against the cosmic-bg sources (`config/src/lib.rs`,
// epoch 1.10): `NAME = "com.system76.CosmicBackground"`, version 1; the `all`
// key is an `Entry` record for all screens (fields `output`, `source`,
// `filter_by_theme`, `rotation_frequency`, `filter_method`, `scaling_mode`,
// `sampling_method`; `deny_unknown_fields`, the first four have no default);
// the `same-on-all` key is a `bool`, with `false` cosmic-bg takes the
// `output.<screen>` records, and `all` only for screens without their own.
// Values are written by `cosmic-config` via `ron::ser::to_string_pretty` —
// we write them in the same form (sample:
// `/usr/share/cosmic/com.system76.CosmicBackground/v1/all`).

/// The cosmic-bg key directory under `$XDG_CONFIG_HOME`.
pub const COSMIC_BG_DIR_REL: &str = "cosmic/com.system76.CosmicBackground/v1";

/// Value of a single-line top-level field of a RON record (`name: value,`).
fn ron_field(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(name)?.trim_start().strip_prefix(':')?;
        Some(rest.trim().trim_end_matches(',').trim().to_string())
    })
}

/// The picture path from a cosmic-bg `Entry` record (`source: Path("…")`); a
/// color, a gradient or an unparsed record — `None`.
pub fn cosmic_bg_source_path(text: &str) -> Option<String> {
    let value = ron_field(text, "source")?;
    let inner = value.strip_prefix("Path(")?.strip_suffix(')')?.trim();
    let quoted = inner.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(chars.next()?);
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// An `Entry` record for all screens with the picture `path`.
///
/// As «COSMIC Settings» does (`config_wallpaper_entry`), foreign fields of the
/// previous `existing` record are kept — scaling method, filter, order and
/// rotation frequency; an unparsed or unfamiliar value is replaced by the
/// cosmic-bg default (`Entry::new`: Zoom, Lanczos, Alphanumeric, 900 s).
/// A path with control characters — `None` (see [`crate::themes::cosmic_ron_string`]).
pub fn cosmic_bg_entry(existing: Option<&str>, path: &str) -> Option<String> {
    let source = crate::themes::cosmic_ron_string(path)?;
    let old = |name: &str| existing.and_then(|t| ron_field(t, name));
    let pick = |name: &str, allowed: &[&str], default: &str| -> String {
        old(name)
            .filter(|v| allowed.contains(&v.as_str()))
            .unwrap_or_else(|| default.to_string())
    };
    // The field is not used by cosmic-bg for a single picture, but is mandatory;
    // `#true` occurs in the sample from /usr/share — we read that form too.
    let filter_by_theme = matches!(old("filter_by_theme").as_deref(), Some("true" | "#true"));
    let rotation = old("rotation_frequency")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(900);
    let scaling = old("scaling_mode")
        .filter(|v| {
            matches!(v.as_str(), "Zoom" | "Stretch")
                || (v.starts_with("Fit((") && v.ends_with("))") && !v.contains('\n'))
        })
        .unwrap_or_else(|| "Zoom".to_string());
    Some(format!(
        "(\n    output: \"all\",\n    source: Path({source}),\n    filter_by_theme: {filter_by_theme},\n    \
         rotation_frequency: {rotation},\n    filter_method: {},\n    scaling_mode: {scaling},\n    \
         sampling_method: {},\n)",
        pick("filter_method", &["Nearest", "Linear", "Lanczos"], "Lanczos"),
        pick("sampling_method", &["Alphanumeric", "Random"], "Alphanumeric"),
    ))
}

/// Set a picture as the COSMIC background: the `all` key + `same-on-all = true`
/// (one picture for all screens, as in Rexon's other environments).
/// `read`/`write` — the executor's I/O (each has its own seam), paths are
/// full. Success means the record was read back and holds exactly this path.
pub fn apply_cosmic_bg(
    config_home: &str,
    path: &str,
    read: &mut dyn FnMut(&str) -> Option<String>,
    write: &mut dyn FnMut(&str, &str) -> bool,
) -> bool {
    let dir = format!("{}/{COSMIC_BG_DIR_REL}", config_home.trim_end_matches('/'));
    let all = format!("{dir}/all");
    let Some(entry) = cosmic_bg_entry(read(&all).as_deref(), path) else {
        return false;
    };
    if !write(&all, &entry) {
        return false;
    }
    let same = format!("{dir}/same-on-all");
    if read(&same).as_deref().map(str::trim) != Some("true") && !write(&same, "true") {
        return false;
    }
    read(&all).and_then(|t| cosmic_bg_source_path(&t)).as_deref() == Some(path)
}

/// Static wallpaper on COSMIC: the native key while cosmic-bg is installed;
/// without it (cosmic-comp with no background of its own) or if the write
/// failed — the common path of external daemons.
fn cosmic_static_steps(path: &str, tools: &[String], swww_hint: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    if has(tools, "cosmic-bg") {
        steps.push(
            Step::new(step_kind::COSMIC_BG, "cosmic-bg", Vec::new()).ok(wallpaper_set_message()),
        );
    }
    steps.extend(wayland_daemon_steps("cosmic", path, tools, swww_hint));
    steps
}

// ── animation (GIF/APNG) ───────────────────────────────────────────────────

/// GIF wallpaper plan. On Wayland swww goes first (it handles GIF natively),
/// then mpvpaper as video; on X11 — xwinwrap + mpv.
///
/// `de` is needed for the same reason as in [`youtube_plan`]: both swww and
/// mpvpaper put the picture with THEIR OWN window via `wlr-layer-shell`, and
/// Mutter (GNOME) and Muffin (Cinnamon) do not have this protocol at all.
/// Previously such a session got either «swww or mpvpaper is not installed»
/// with swww and mpvpaper INSTALLED, or a launch without a picture. The
/// environment decision is one and shared with video and YouTube wallpapers —
/// [`needs_shell_extension`].
pub fn animated_plan(
    de: &str,
    session_type: &str,
    path: &str,
    tools: &[String],
    monitor: &str,
    resolution: &str,
    hints: &Hints,
) -> Vec<Step> {
    if needs_shell_extension(de, session_type) {
        return vec![fail(trf(
            "On %1 there's nothing to show animated wallpapers with: both swww and \
             mpvpaper draw their own window through wlr-layer-shell, which the shell \
             doesn't have. Live wallpapers there are shown by the shell extension — \
             choose a video wallpaper (MP4/WebM).",
            &[&shell_ext_de_label(de)],
        ))];
    }
    if session_type == "wayland" {
        let mut steps = Vec::new();
        if has(tools, "swww") {
            let mut step = Step::new(
                step_kind::RUN,
                "swww",
                argv(&[
                    "swww",
                    "img",
                    path,
                    "--transition-type",
                    "fade",
                    "--transition-duration",
                    "1",
                ]),
            )
            .ok(tr("GIF wallpaper started"))
            .fmt("animated");
            step.ensure_swww = true;
            steps.push(step);
        }
        if has(tools, "mpvpaper") {
            steps.extend(mpvpaper_steps(path, false, 50, None, monitor, tools, hints));
        } else {
            // The trailing refusal is needed also when swww EXISTS but `swww img`
            // failed: then nobody can pick up the GIF, and we tell the customer
            // what to install. When swww succeeds the step is unreachable.
            steps.push(fail(tr(
                "Neither swww nor mpvpaper is installed for GIF wallpapers on Wayland",
            )));
        }
        return steps;
    }
    xwinwrap_steps(path, false, 50, None, resolution, tools, hints)
}

// ── video ──────────────────────────────────────────────────────────────────

/// Package name hints for the current distribution. The shim collects them
/// (`tool_package_hint`), the core only inserts them into the text.
#[derive(Debug, Clone, Default)]
pub struct Hints {
    /// Hint about `mpvpaper`.
    pub mpvpaper: String,
    /// Hint about `xwinwrap`.
    pub xwinwrap: String,
    /// Hint about `cvlc`.
    pub cvlc: String,
    /// Hint about `webkit2gtk-4.1`.
    pub webkit: String,
    /// Hint about `swww`.
    pub swww: String,
    /// Hint about `yt-dlp`.
    pub ytdlp: String,
}

/// No `mpvpaper` — video wallpapers have nothing to draw with under Wayland.
fn mpvpaper_missing_message(hint: &str) -> String {
    // TRANSLATORS: %1 is a hint on how to install mpvpaper.
    trf("The mpvpaper program wasn't found; video wallpapers can't run on Wayland without it. %1", &[&hint])
}

/// No `xwinwrap` — video wallpapers have nothing to draw with under X11.
fn xwinwrap_missing_message(hint: &str) -> String {
    // TRANSLATORS: %1 is a hint on how to install xwinwrap.
    trf("The xwinwrap program wasn't found; video wallpapers can't run on X11 without it. %1", &[&hint])
}

fn mpvpaper_steps(
    path: &str,
    audio: bool,
    volume: i64,
    audio_file: Option<&str>,
    monitor: &str,
    tools: &[String],
    hints: &Hints,
) -> Vec<Step> {
    if !has(tools, "mpvpaper") {
        return vec![fail(mpvpaper_missing_message(&hints.mpvpaper))];
    }
    let audio_opt = if audio { "" } else { " no-audio" };
    let vol_opt = if audio {
        format!(" volume={volume}")
    } else {
        String::new()
    };
    let af_opt = audio_file
        .map(|f| format!(" audio-file={f}"))
        .unwrap_or_default();
    let opts = format!("loop=inf hwdec=auto{audio_opt}{vol_opt}{af_opt}");
    vec![Step::new(
        step_kind::SPAWN,
        "mpvpaper",
        vec![
            "mpvpaper".into(),
            "-o".into(),
            opts,
            monitor.to_string(),
            path.to_string(),
        ],
    )
    .ok(tr("Video wallpaper started"))
    .fmt("video")]
}

/// The output geometry placeholder in plan commands: `WxH+X+Y`.
///
/// `{resolution}` gives only the size, while `xwinwrap` covers the X root
/// window by the whole geometry: without the offset the second monitor's
/// window would land on top of the first. The core executor expands this
/// placeholder into the size TOGETHER with the position of a SPECIFIC
/// output, one placeholder per launch.
pub const GEOMETRY_MARK: &str = "{geometry}";

/// The value for `xwinwrap -g`.
///
/// Given a ready geometry (the [`GEOMETRY_MARK`] placeholder or a string with
/// an offset) — it goes as is. Given a size alone (the way the `rexon-py`
/// bridge calls it) — a zero offset is appended to it, as before: the window
/// goes to the top-left corner of the desktop.
fn xwinwrap_geometry(size_or_geometry: &str) -> String {
    if size_or_geometry == GEOMETRY_MARK || size_or_geometry.contains('+') {
        return size_or_geometry.to_string();
    }
    format!("{size_or_geometry}+0+0")
}

fn xwinwrap_steps(
    path: &str,
    audio: bool,
    volume: i64,
    audio_file: Option<&str>,
    resolution: &str,
    tools: &[String],
    hints: &Hints,
) -> Vec<Step> {
    if !has(tools, "xwinwrap") {
        return vec![fail(xwinwrap_missing_message(&hints.xwinwrap))];
    }
    if !has(tools, "mpv") {
        return vec![fail(tr("mpv isn't installed."))];
    }
    let mut cmd = vec![
        "xwinwrap".to_string(),
        "-g".to_string(),
        xwinwrap_geometry(resolution),
        "-ov".into(),
        "-ni".into(),
        "-s".into(),
        "-nf".into(),
        "--".into(),
        "mpv".into(),
        "--loop=inf".into(),
    ];
    if !audio {
        cmd.push("--no-audio".into());
    } else {
        cmd.push(format!("--volume={volume}"));
    }
    if let Some(f) = audio_file {
        cmd.push(format!("--audio-file={f}"));
    }
    cmd.extend(argv(&[
        "--no-osc",
        "--no-osd-bar",
        "--no-input-default-bindings",
        "--hwdec=auto",
        "--gpu-api=auto",
        "--wid=WID",
    ]));
    cmd.push(path.to_string());
    vec![Step::new(step_kind::SPAWN, "xwinwrap", cmd)
        .ok(tr("Video wallpaper started"))
        .fmt("video")]
}

/// The shell extension schema key — GNOME and Cinnamon have different
/// schemas, the keys are the same. `None` — the environment manages with a
/// regular window.
pub fn shell_ext_schema(de: &str) -> Option<&'static str> {
    match de {
        "gnome" => Some("org.gnome.shell.extensions.rexon-wallpaper"),
        "cinnamon" | "cinnamon-mint" => Some("org.cinnamon.extensions.rexon-wallpaper"),
        _ => None,
    }
}

/// Identifier of our shell extension.
pub const SHELL_EXT_ID: &str = "rexon-wallpaper@rexon.app";

/// Human name of the shell for messages to the customer.
///
/// In the engine wallpapers are told apart by identifier (`gnome`,
/// `cinnamon`), but an identifier has no place in on-screen text (rule #13):
/// «a video cannot be streamed on gnome» reads like a raw string from a
/// config. Both shell extension messages take the name from here so that it
/// is one and the same.
pub fn shell_ext_de_label(de: &str) -> &str {
    match de {
        "gnome" => "GNOME",
        "cinnamon" | "cinnamon-mint" => "Cinnamon",
        other => other,
    }
}

/// A Mutter-family shell (GNOME, Cinnamon) on Wayland — where it has our
/// extension. By itself it does not answer whether the extension path is
/// needed: Cinnamon with layer-shell does not need it
/// ([`needs_shell_extension`]). Separately — for the idle screensaver
/// (`idle_screensaver`): showing it via the extension does not depend on the
/// wallpaper's layer-shell.
pub fn mutter_family_wayland(de: &str, session_type: &str) -> bool {
    session_type == "wayland" && shell_ext_schema(de).is_some()
}

/// Whether the shell extension path is needed instead of our own window.
///
/// Wayland only: Mutter cannot do wlr-layer-shell, a wallpaper cannot be put
/// there as a window; Muffin can since Cinnamon 6.8 — then the wallpaper goes
/// as a layer, as on wlr compositors (the session fact is
/// [`crate::wallpaper_backend::cinnamon_layer_shell_fact`], set by the live
/// process; not set — the extension, as before). On X11 both environments use
/// a regular window with `_NET_WM_WINDOW_TYPE_DESKTOP`.
pub fn needs_shell_extension(de: &str, session_type: &str) -> bool {
    needs_shell_extension_with(de, session_type, crate::wallpaper_backend::cinnamon_layer_shell_fact())
}

/// [`needs_shell_extension`] with an explicit "Cinnamon with layer-shell" fact.
pub fn needs_shell_extension_with(de: &str, session_type: &str, cinnamon_layer_shell: bool) -> bool {
    mutter_family_wayland(de, session_type) && !(cinnamon_layer_shell && matches!(de, "cinnamon" | "cinnamon-mint"))
}

/// When to set the backdrop relative to showing the live wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackdropTiming {
    /// Before showing, as before: the desktop picture here is held by a separate
    /// background-layer surface, and one set LATER would land over ours.
    BeforeShow,
    /// After showing, in the background: changing the desktop picture does not
    /// cover our surface, and the video does not wait for ffmpeg and the settings
    /// write (measured 15.11.2026: ~1.5 s to the first frame).
    AfterShow,
}

/// When to set the backdrop on this desktop — a breakdown by wallpaper output paths.
///
/// * **X11** (MATE, XFCE, Cinnamon-X11, Budgie, GNOME-X11, LXQt, feh) —
///   after. Our `_NET_WM_WINDOW_TYPE_DESKTOP` window already lies over the
///   desktop window; changing the picture repaints the same desktop window
///   (Caja, xfdesktop, pcmanfm-qt) or the shell/root window background under
///   all windows — no new windows appear, the order does not change. With
///   Caja's transparent background the desktop window does not draw the
///   picture at all.
/// * **GNOME and Cinnamon-Wayland before 6.8** (shell extension) — after.
///   On a `picture-uri` change GNOME Shell creates a new `Meta.BackgroundActor`
///   and puts it AT THE VERY BOTTOM of the background group (`background.js`,
///   `set_child_below_sibling(actor, null)`; verified against GNOME Shell 51),
///   our actor stands above it. In Cinnamon our actor is a child of the
///   background window (above its contents), and the extension picks up a new
///   background window by itself.
/// * **Cinnamon 6.8 with layer-shell** — after. The picture is drawn by the
///   background service with a surface of the same layer, but Muffin puts a
///   surface created LATER BELOW the earlier ones, and Rexon restarts the
///   service itself under our wallpaper
///   (`wallpaper_backend::restack_cinnamon_background`) — a picture change
///   does not cover the video. Without a backdrop the Cinnamon lock screen
///   (it takes the picture from `picture-uri-list`) showed the previous, stale
///   picture (live check 16.11.2026). Before showing is not allowed: before
///   the restack the service's surface lies over ours, and the person would
///   see a frame instead of the video.
/// * **KDE Plasma** (layer window) — after: `plasma-apply-wallpaperimage` and
///   the Plasma script change the picture in the already open desktop window,
///   there is no new surface.
/// * **Deepin/Treeland** (layer window) — after: the picture is accepted by the
///   Control Center and drawn by the compositor itself as its background under
///   the shell layers; the Control Center step can be long (up to 70 s), and
///   the video no longer waits for it.
/// * **Other Wayland** (sway, Hyprland, niri, river, labwc, COSMIC and
///   unrecognized) — BEFORE showing. There the picture is held by a separate
///   program with a surface of the same background layer: swaybg/wbg are
///   started anew, `swww` may start its daemon, sway restarts its swaybg,
///   cosmic-bg re-reads the key. On wlroots compositors a surface created
///   later than ours goes above it — the backdrop would cover the video.
pub fn backdrop_timing(de: &str, session_type: &str) -> BackdropTiming {
    if session_type != "wayland"
        || mutter_family_wayland(de, session_type)
        || uses_plasma_config(de)
        || de == "deepin"
    {
        return BackdropTiming::AfterShow;
    }
    BackdropTiming::BeforeShow
}

/// The movie second for the backdrop frame: from the 3rd — past intros; a
/// shorter movie — from the start ([`BACKDROP_FALLBACK_SEC`]).
pub const BACKDROP_FRAME_SEC: f64 = 3.0;
/// Fallback second for the backdrop frame.
pub const BACKDROP_FALLBACK_SEC: f64 = 0.0;

/// Cache key of a movie's still frame: path, size and modification time of
/// the file (the contents of a multi-gigabyte movie are not read — as with
/// [`faststart_key`]) plus the screen size the frame is scaled to.
pub fn video_still_key(path: &str, size: u64, mtime: i64, screen: Option<(u64, u64)>) -> String {
    let screen = screen.map(|(w, h)| format!("{w}x{h}")).unwrap_or_else(|| "native".to_string());
    let digest = sha1_hex(format!("{path}:{size}:{mtime}:{screen}").as_bytes());
    digest[..16].to_string()
}

/// Where a movie's still frame lies: `<cache>/rexon/video-stills/<key>.jpg`.
/// One cache for the backdrop under live wallpapers and for the frame shown
/// instead of a crashing wallpaper.
pub fn video_still_path(cache_home: &str, key: &str) -> String {
    format!("{}/rexon/video-stills/{key}.jpg", cache_home.trim_end_matches('/'))
}

/// Screen size for the backdrop frame: the largest output (the frame is
/// stretched the most on it); on smaller ones the desktop scales it down itself.
pub fn still_screen_size(sizes: &[(u64, u64)]) -> Option<(u64, u64)> {
    sizes.iter().copied().max_by_key(|(w, h)| w * h)
}

/// ffmpeg: a sharp movie frame at second `timestamp`, scaled so as to COVER
/// the `screen` (Lanczos, proportions kept — the desktop crops with its own
/// "fill"); `None` — at the movie's native size. Highest-quality JPEG
/// `-q:v 2`: a PNG of a 4K frame weighs tens of megabytes.
pub fn video_still_argv(path: &str, output: &str, timestamp: f64, screen: Option<(u64, u64)>) -> Vec<String> {
    let mut cmd = vec![
        FFMPEG_BIN.to_string(),
        "-v".into(),
        "error".into(),
        "-y".into(),
        "-ss".into(),
        format_float(timestamp),
        "-i".into(),
        path.to_string(),
        "-frames:v".into(),
        "1".into(),
    ];
    if let Some((w, h)) = screen {
        cmd.push("-vf".into());
        cmd.push(format!("scale={w}:{h}:force_original_aspect_ratio=increase:flags=lanczos"));
    }
    cmd.extend(["-q:v".to_string(), "2".to_string(), output.to_string()]);
    cmd
}

/// Journal line: the backdrop got set `ms` after the video launch.
pub fn backdrop_ready_log_line(ms: u64) -> String {
    format!("подложка готова через {ms} мс после видео")
}

/// Limit of the `rexon/video-stills` frame cache: no more than this many frames…
pub const VIDEO_STILLS_MAX_FILES: usize = 24;
/// …and no more than this many bytes together (a 4K frame in JPEG `-q:v 2` is 2–4 MB).
pub const VIDEO_STILLS_MAX_BYTES: u64 = 96 * 1024 * 1024;

/// What to delete from the frame cache to fit within the limit
/// ([`VIDEO_STILLS_MAX_FILES`], [`VIDEO_STILLS_MAX_BYTES`]). Input is
/// `(path, mtime, bytes)`; the oldest by mtime go first (a frame taken from
/// the cache is refreshed — so these are "long unneeded"). `keep` — the frame
/// being set now is never deleted. Unfinished `*.part.jpg` files do not wait
/// behind the rest: they are always deleted, except `keep`.
pub fn video_still_evictions(files: &[(String, i64, u64)], keep: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut live: Vec<&(String, i64, u64)> = Vec::new();
    for f in files {
        if f.0 == keep {
            continue;
        }
        if f.0.ends_with(".part.jpg") {
            out.push(f.0.clone());
        } else {
            live.push(f);
        }
    }
    let keep_bytes = files.iter().find(|f| f.0 == keep).map(|f| f.2).unwrap_or(0);
    let keep_count = usize::from(files.iter().any(|f| f.0 == keep));
    live.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let mut count = live.len() + keep_count;
    let mut bytes: u64 = live.iter().map(|f| f.2).sum::<u64>() + keep_bytes;
    for f in live {
        if count <= VIDEO_STILLS_MAX_FILES && bytes <= VIDEO_STILLS_MAX_BYTES {
            break;
        }
        out.push(f.0.clone());
        count -= 1;
        bytes = bytes.saturating_sub(f.2);
    }
    out
}

// ── who owns the wallpaper layer ───────────────────────────────────────────
//
// ⚠️ A rule #12 junction. The question "is the background ours in this
// session" comes up in EVERY wallpaper plan — pictures, video, YouTube,
// application wallpapers. If it were decided by a branch in each plan, the
// branches would drift apart exactly the way three copies of
// `detect_current_de` did. So the decision here is one, and everyone calls it.

/// Who draws the background in this compositor + shell pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallpaperOwner {
    /// The background is ours: the Rexon engine puts its own surface (or window) itself.
    Rexon,
    /// The background is drawn by a foreign shell. Fighting it for the layer is
    /// useless: its surface on the Background layer stays on top of ours, the
    /// customer sees the foreign wallpaper, and the report is green.
    Shell(DesktopShell),
}

impl WallpaperOwner {
    /// Whether the background is ours.
    pub fn is_ours(self) -> bool {
        matches!(self, WallpaperOwner::Rexon)
    }

    /// The foreign owning shell, if the owner is foreign.
    pub fn shell(self) -> Option<DesktopShell> {
        match self {
            WallpaperOwner::Rexon => None,
            WallpaperOwner::Shell(s) => Some(s),
        }
    }
}

/// Who owns the wallpaper layer.
///
/// `shell` — the answer of [`crate::system::detect_shell`];
/// `shell_draws_wallpaper` — whether the shell's wallpaper is known to be
/// TURNED OFF (`Some(false)`). `None` means "did not find out": then we
/// assume the shell draws. The cautious side is chosen on purpose — telling
/// once too often how to turn off the foreign wallpaper is cheaper than
/// silently putting ours under someone else's.
///
/// Order of answers:
///   * not Wayland — the background is ours: both shells live only on
///     Wayland, and on X11 wallpapers are put as a window with
///     `_NET_WM_WINDOW_TYPE_DESKTOP`;
///   * GNOME/Cinnamon on Wayland — the background is ours: Mutter and Muffin
///     cannot do wlr-layer-shell, neither they nor we can take the wallpaper
///     layer there, and there is one path — the shell extension
///     ([`needs_shell_extension`]);
///   * no shell or its wallpaper is off — the background is ours;
///   * otherwise the background is foreign.
pub fn wallpaper_layer_owner(
    de: &str,
    session_type: &str,
    shell: Option<DesktopShell>,
    shell_draws_wallpaper: Option<bool>,
) -> WallpaperOwner {
    if session_type != "wayland" {
        return WallpaperOwner::Rexon;
    }
    if needs_shell_extension(de, session_type) {
        return WallpaperOwner::Rexon;
    }
    match shell {
        Some(s) if shell_draws_wallpaper != Some(false) => WallpaperOwner::Shell(s),
        _ => WallpaperOwner::Rexon,
    }
}

/// What to tell the customer when a foreign shell owns the wallpaper layer.
///
/// Keeping quiet is not allowed here: from outside it looks like "Rexon does
/// not work", while in fact the wallpaper would land under a foreign surface.
/// So the text says both what is happening and what to press: the switch is
/// the «Give the wallpaper to Rexon» button on the «Wallpaper Engine» tab
/// (`crate::desktop_shell::hand_over`), not a file path and a key name (rule
/// #13: jargon has no place in the text; which key is where and why is
/// explained in the header of `desktop_shell`). The text is ONE and lives in
/// the core: there must be no second copy in QML.
pub fn shell_owner_message(shell: DesktopShell) -> String {
    trf(
        // TRANSLATORS: %1 is the shell name (Noctalia, DankMaterialShell). “Wallpaper Engine” is the tab name, “Give Wallpaper to Rexon” the button on it.
        "The background is currently drawn by %1: the shell keeps its own full-screen \
         surface on the wallpaper layer, and Rexon's wallpaper would end up underneath it. \
         On the Wallpaper Engine tab, press “Give Wallpaper to Rexon”, then apply the \
         Rexon wallpaper again.",
        &[&shell.label()],
    )
}

/// The command checking "what state our shell extension is in".
///
/// For GNOME we ask the shell itself over D-Bus (`GetExtensionInfo`): the
/// former `gnome-extensions list --enabled` only shows that the extension is
/// ENABLED in the settings — while one that crashed with an error, does not
/// fit the shell version or is not loaded yet (Wayland, no re-login) is also
/// "enabled" there, and Rexon reported «done» on an empty desktop. Parsing is
/// [`parse_shell_ext_state`]. Cinnamon has no such call — we look at the list
/// of enabled extensions in its settings.
pub fn shell_ext_state_argv(de: &str) -> Vec<String> {
    if de == "gnome" {
        gnome_ext_info_argv(SHELL_EXT_ID)
    } else {
        argv(&["gsettings", "get", "org.cinnamon", "enabled-extensions"])
    }
}

/// GNOME Shell's `GetExtensionInfo` for any extension `uuid`; the answer is
/// parsed by [`parse_shell_ext_state`] with `de = "gnome"`.
pub fn gnome_ext_info_argv(uuid: &str) -> Vec<String> {
    argv(&[
        "gdbus",
        "call",
        "--session",
        "--dest",
        "org.gnome.Shell",
        "--object-path",
        "/org/gnome/Shell",
        "--method",
        "org.gnome.Shell.Extensions.GetExtensionInfo",
        uuid,
    ])
}

/// The state of our shell extension as seen by the shell itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellExtState {
    /// Running — the wallpaper will show.
    Enabled,
    /// Disabled in the extension list.
    Disabled,
    /// The shell caught an error in the extension's code.
    Error,
    /// The shell version is not in `shell-version` of our `metadata.json`.
    OutOfDate,
    /// The shell does not know the extension: installed but not loaded (on
    /// Wayland — until logging back into the session).
    NotLoaded,
    /// Asking failed: the shell did not answer or the answer is unclear.
    Unknown,
}

/// Parse the answer to [`shell_ext_state_argv`].
///
/// GNOME: `gdbus` prints an `a{sv}` dictionary, the state is a floating-point
/// number (`'state': <1.0>`: the shell packs all numbers as `d`,
/// `serializeExtension` in `misc/extensionUtils.js`). The `ExtensionState`
/// values are the same in gnome-shell 45–51 (verified against the sources;
/// in 47 ENABLED/DISABLED were renamed to ACTIVE/INACTIVE, same numbers):
/// 1 enabled, 2 disabled, 3 error, 4 wrong version, 5 downloading,
/// 6 loaded but never enabled, 7 disabling, 8 enabling, 99 not installed.
/// An unknown uuid is an empty dictionary `(@a{sv} {},)`.
///
/// Cinnamon: the `enabled-extensions` list from `gsettings`.
///
/// `out = None` — the command did not start or returned an error.
pub fn parse_shell_ext_state(de: &str, out: Option<&str>) -> ShellExtState {
    let Some(out) = out else {
        return ShellExtState::Unknown;
    };
    if de != "gnome" {
        if out.trim().is_empty() {
            return ShellExtState::Unknown;
        }
        return if out.contains(&format!("'{SHELL_EXT_ID}'")) {
            ShellExtState::Enabled
        } else {
            ShellExtState::Disabled
        };
    }
    if !out.trim_start().starts_with('(') {
        return ShellExtState::Unknown;
    }
    let Some(at) = out.find("'state': <") else {
        return ShellExtState::NotLoaded;
    };
    let rest = &out[at + "'state': <".len()..];
    let Some(state) = rest
        .split('>')
        .next()
        .and_then(|v| v.trim().parse::<f64>().ok())
    else {
        return ShellExtState::Unknown;
    };
    match state as i64 {
        1 | 8 => ShellExtState::Enabled,
        2 | 6 | 7 => ShellExtState::Disabled,
        3 => ShellExtState::Error,
        4 => ShellExtState::OutOfDate,
        5 | 99 => ShellExtState::NotLoaded,
        _ => ShellExtState::Unknown,
    }
}

/// The "enable our extension" command for a disabled one: GNOME —
/// `gnome-extensions enable`; Cinnamon — the `enabled-extensions` list with
/// our uuid at the end, foreign uuids are kept (the shell enables the
/// extension on the fly, without re-login). `list_out` — the output of
/// [`shell_ext_state_argv`] on Cinnamon (the current list), not needed on
/// GNOME. Already enabled — `None`.
pub fn shell_ext_enable_argv(de: &str, list_out: Option<&str>) -> Option<Vec<String>> {
    if de == "gnome" {
        return Some(argv(&["gnome-extensions", "enable", SHELL_EXT_ID]));
    }
    shell_ext_schema(de)?;
    let mut list: Vec<String> = list_out
        .unwrap_or_default()
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    if list.iter().any(|u| u == SHELL_EXT_ID) {
        return None;
    }
    list.push(SHELL_EXT_ID.to_string());
    Some(vec![
        "gsettings".to_string(),
        "set".to_string(),
        "org.cinnamon".to_string(),
        "enabled-extensions".to_string(),
        gvariant_string_array(&list),
    ])
}

/// What to tell the person when the wallpaper is handed to the extension, but
/// it is not working.
///
/// `None` — the extension works, nothing to say. Otherwise — whether the
/// handover succeeded, and the text. Not loaded or not answering — the
/// wallpaper is handed over and will appear as soon as the extension starts
/// working (a success with an honest text); disabled (Rexon itself could not
/// enable it), error and wrong version — there is no wallpaper on the desktop
/// (a refusal: there must be no green check mark on an empty desktop, rule
/// #13). For a disabled one — the plan's text (`disabled`).
pub fn shell_ext_state_message(state: ShellExtState, disabled: &str) -> Option<(bool, String)> {
    match state {
        ShellExtState::Enabled => None,
        // Rexon enables a disabled one itself (`shell_ext_enable_argv`); we get
        // here only if enabling failed — there is no wallpaper on the desktop.
        ShellExtState::Disabled => Some((false, disabled.to_string())),
        ShellExtState::Error => Some((
            false,
            tr("The wallpaper was handed to the Rexon shell extension, but GNOME Shell \
                reports an error in the extension, so the wallpaper won't appear. Log out \
                and back in; if that doesn't help, reinstall Rexon."),
        )),
        ShellExtState::OutOfDate => Some((
            false,
            tr("The wallpaper was handed to the Rexon shell extension, but GNOME Shell \
                considers it incompatible with this shell version and doesn't load it. \
                Update Rexon."),
        )),
        ShellExtState::NotLoaded => Some((
            true,
            tr("The wallpaper was handed to the Rexon shell extension, but the shell \
                hasn't loaded the extension yet. Log out and back in — the wallpaper \
                will appear after that."),
        )),
        ShellExtState::Unknown => Some((
            true,
            tr("The wallpaper was handed to the Rexon shell extension, but the shell \
                didn't say whether the extension is working. If the wallpaper doesn't \
                appear, turn on “Rexon Wallpaper Engine” in the extension list or log out and \
                back in."),
        )),
    }
}

/// The extension schema key into which the READY host command line is put.
///
/// The extension is shell code, not our process: `sys.executable` is not
/// available to it, and a bare `python3` from `PATH` for a customer with a
/// package, portable or Flatpak points to the wrong place (or nowhere). So
/// the command line is assembled by whoever knows it — the wallpaper engine —
/// and passed along as data.
pub const SHELL_EXT_DAEMON_KEY: &str = "daemon-argv";

/// Text form of a string array for `gsettings set` (type `as`).
///
/// GVariant format: `["a","b"]`; `\` and `"` are escaped inside strings.
/// The value goes as ONE argv element, the shell does not parse it.
pub fn gvariant_string_array(items: &[String]) -> String {
    let body: Vec<String> = items
        .iter()
        .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("[{}]", body.join(","))
}

/// Texts and commands for handing a wallpaper to the shell extension.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ShellExtPlan {
    /// Write the source (it is also the install check: a foreign schema is an error).
    pub source_argv: Vec<String>,
    /// Write the engine kind.
    pub engine_argv: Vec<String>,
    /// Write the host command line ([`SHELL_EXT_DAEMON_KEY`]).
    ///
    /// ⚠️ The result of this step MUST be checked. The key may be missing from
    /// the schema of an extension left over from a previous version — then
    /// `gsettings` returns an error. It used to be harmless: the extension took
    /// its fallback path `python3 -m rexon.video_wallpaper`. The module was
    /// removed from the tree, there is no fallback path any more — and an
    /// unnoticed error would mean an empty desktop with a green report (rule #13).
    pub daemon_argv: Vec<String>,
    /// No schema — the extension is not installed.
    pub not_installed_message: String,
    /// The [`SHELL_EXT_DAEMON_KEY`] key did not get written — the extension is outdated.
    pub daemon_failed_message: String,
    /// The wallpaper is handed over, but the extension is disabled.
    pub disabled_message: String,
    /// Everything is in place.
    pub ok_message: String,
}

/// Plan for handing a video wallpaper to the GNOME/Cinnamon shell extension.
///
/// The daemon is launched by the extension ITSELF (it also stops it and reads
/// the frames) — so here we only write what to show and WITH WHAT to show it.
///
/// `host`/`has_audio` — the same as in [`video_native_plan`]: the command line
/// for both paths is assembled by one function ([`video_host_argv`]), there
/// must be no second "how to launch the host" decision in the project (rule #12).
pub fn shell_ext_plan(
    de: &str,
    path: &str,
    host: &str,
    has_audio: bool,
) -> Option<ShellExtPlan> {
    let cmd = video_host_argv(host, path, has_audio);
    // TRANSLATORS: %1 is the desktop name (GNOME, Cinnamon).
    let ok = trf("Video wallpaper started (%1, shell extension)", &[&shell_ext_de_label(de)]);
    shell_ext_plan_for(de, path, &cmd, "video", ok)
}

/// Hand a Flash wallpaper to the GNOME/Cinnamon shell extension: the same
/// command line as for our own window ([`swf_host_plan`]) — the host writes
/// frames into a file itself, the extension shows them. The extension only
/// checks the `engine` value for emptiness and writes it to the journal.
pub fn swf_shell_ext_plan(de: &str, path: &str, host: &str) -> Option<ShellExtPlan> {
    let cmd = wallpaper_host_argv(host, HostMode::Swf, path);
    let ok = trf("Flash wallpaper started (%1, shell extension)", &[&shell_ext_de_label(de)]);
    shell_ext_plan_for(de, path, &cmd, HostMode::Swf.as_arg(), ok)
}

/// Where in the Rexon window the shell extension lives: a section of
/// «Appearance», not the «Extensions» tab — that one has only .rxn packages
/// (live check on gnome-shell 51.1, 16.10.2026: the hint pointed to the wrong place).
pub(crate) fn shell_ext_place(de: &str) -> String {
    let section = if de == "gnome" {
        trc("appearance section", "GNOME Extensions")
    } else {
        trc("appearance section", "Cinnamon Extensions")
    };
    format!("{} → {}", tr("Appearance"), section)
}

/// The common plan for handing a wallpaper to the shell extension — ONE for
/// all host modes: the source, `engine`, the daemon command line and the
/// refusal words.
fn shell_ext_plan_for(
    de: &str,
    path: &str,
    cmd: &[String],
    engine: &str,
    ok_message: String,
) -> Option<ShellExtPlan> {
    let schema = shell_ext_schema(de)?;
    Some(ShellExtPlan {
        source_argv: argv(&["gsettings", "set", schema, "source", path]),
        engine_argv: argv(&["gsettings", "set", schema, "engine", engine]),
        daemon_argv: vec![
            "gsettings".to_string(),
            "set".to_string(),
            schema.to_string(),
            SHELL_EXT_DAEMON_KEY.to_string(),
            gvariant_string_array(cmd),
        ],
        not_installed_message: trf(
            // TRANSLATORS: %1 is a desktop id (gnome, cinnamon); %2 is the
            // place in Rexon, e.g. “Appearance → GNOME Extensions”.
            "Live wallpapers on %1 need the Rexon shell extension: it's installed \
             together with the program — turn it on in “%2” and try again.",
            &[&de, &shell_ext_place(de)],
        ),
        daemon_failed_message: trf(
            // TRANSLATORS: %1 is the place in Rexon, e.g. “Appearance → GNOME Extensions”.
            "The Rexon shell extension is left over from an older version and doesn't \
             know how to start the wallpaper. Reinstall it in “%1” and try again.",
            &[&shell_ext_place(de)],
        ),
        disabled_message: trf(
            // TRANSLATORS: %1 is the place in Rexon, e.g. “Appearance → Cinnamon Extensions”.
            "The wallpaper wasn't shown: the Rexon shell extension is turned off and Rexon \
             couldn't turn it on. Turn on “Rexon Wallpaper Engine” in “%1” and try again.",
            &[&shell_ext_place(de)],
        ),
        ok_message,
    })
}

/// The `schemas/` directories of our extension in the order the shell looks
/// for the extension itself: the user copy (`$XDG_DATA_HOME`, otherwise
/// `~/.local/share`) overrides the system ones (`$XDG_DATA_DIRS`, otherwise
/// `/usr/local/share:/usr/share`).
///
/// ⚠️ Why: the extension's schema lies and is compiled in its own directory,
/// and it may be missing from the shared `/usr/share/glib-2.0/schemas` (that
/// was the case in the package before 16.10.2026; with an install into the
/// home directory it is never there). Bare `gsettings set` then does not see
/// it — «Schema … does not exist» — and the wallpaper is not handed to the
/// extension at all. The extension itself reads its settings exactly from
/// the schema in its directory (`getSettings`), so writing through it means
/// writing exactly where it will read from.
/// An empty list — the environment has no extension of ours.
pub fn shell_ext_schema_dirs(
    de: &str,
    home: &str,
    xdg_data_home: &str,
    xdg_data_dirs: &str,
) -> Vec<String> {
    let sub = match shell_ext_schema(de) {
        Some(_) if de == "gnome" => "gnome-shell/extensions",
        Some(_) => "cinnamon/extensions",
        None => return Vec::new(),
    };
    let mut bases = Vec::new();
    if !xdg_data_home.is_empty() {
        bases.push(xdg_data_home.to_string());
    } else if !home.is_empty() {
        bases.push(format!("{home}/.local/share"));
    }
    let system = if xdg_data_dirs.is_empty() { "/usr/local/share:/usr/share" } else { xdg_data_dirs };
    bases.extend(system.split(':').filter(|d| !d.is_empty()).map(str::to_string));
    bases
        .iter()
        .map(|b| format!("{}/{sub}/{SHELL_EXT_ID}/schemas", b.trim_end_matches('/')))
        .collect()
}

/// A `gsettings` command with an explicit schema directory: `--schemadir`
/// goes right after the program name (that is where `gsettings` expects it).
/// Not `gsettings` — the command is returned as is.
pub fn with_schemadir(cmd: &[String], dir: &str) -> Vec<String> {
    if cmd.first().map(String::as_str) != Some("gsettings") {
        return cmd.to_vec();
    }
    let mut out = vec![cmd[0].clone(), "--schemadir".to_string(), dir.to_string()];
    out.extend_from_slice(&cmd[1..]);
    out
}

/// Take the wallpaper away from the shell extension: an empty source = "do not show".
pub fn shell_ext_stop_argv(de: &str) -> Option<Vec<String>> {
    let schema = shell_ext_schema(de)?;
    Some(argv(&["gsettings", "set", schema, "source", ""]))
}

/// Name of the built wallpaper host (the `rexon-gui` crate, a binary) — ONE
/// for all formats Rexon shows with its own process.
///
/// ⭐ On 15.09.2026 there were two hosts (`rexon-wallpaper-host` for video and
/// `rexon-web-host` for web) with two lookups, two command line builders and
/// two lines in the packaging. The owner's decision is not to multiply hosts:
/// the format is chosen by the mode ([`HostMode`]) in the command line of a
/// single binary. The name was kept as the video one: it is already written
/// in the GNOME/Cinnamon shell extension's gsettings on customers' machines.
pub const WALLPAPER_HOST_BINARY: &str = "rexon-wallpaper-host";

/// The mode switch in the wallpaper host command line.
pub const HOST_MODE_FLAG: &str = "--mode";

/// Wallpaper host mode: what the `rexon-wallpaper-host` process shows.
///
/// A new format is added as a variant here, not as a new binary — that is
/// how Flash came in as the third mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostMode {
    /// A video file or a wallpaper directory with video (QtMultimedia).
    Video,
    /// A link, an HTML file or a web wallpaper directory (QtWebEngine).
    Web,
    /// A Flash SWF movie: Ruffle draws via wgpu (Vulkan), the `rexon-swf` crate.
    /// The mode has no QML — the window is placed on the same layer as video and web.
    Swf,
    /// The waywallen daemon's output client on X11: desktop windows in which the
    /// waywallen output element shows the daemon's frames (Wallpaper Engine
    /// scenes). It is launched by the daemon from the output definition
    /// (`wallpaper_backend::rexon_display_manifest`) or by Rexon on «Apply».
    Waywallen,
}

impl HostMode {
    /// All modes — for the host's help and the checks.
    pub const ALL: [HostMode; 4] = [HostMode::Video, HostMode::Web, HostMode::Swf, HostMode::Waywallen];

    /// The mode when the [`HOST_MODE_FLAG`] switch is not in the line — video.
    ///
    /// This is not "detection by content" but compatibility: before the merge
    /// `rexon-wallpaper-host` was the video host, and the shell extension already
    /// keeps such a mode-less line in the customer's gsettings (the
    /// [`SHELL_EXT_DAEMON_KEY`] key) until the next wallpaper apply. The host does
    /// not take it upon itself to guess the format from the path: the caller
    /// knows the format, and the "what kind of wallpaper is this" decision in the
    /// project is one — the engine's.
    pub const DEFAULT: HostMode = HostMode::Video;

    /// The value of the [`HOST_MODE_FLAG`] switch.
    pub fn as_arg(self) -> &'static str {
        match self {
            HostMode::Video => "video",
            HostMode::Web => "web",
            HostMode::Swf => "swf",
            HostMode::Waywallen => "waywallen",
        }
    }

    /// Parsing the value of the [`HOST_MODE_FLAG`] switch; an unfamiliar one is `None`.
    pub fn from_arg(raw: &str) -> Option<HostMode> {
        HostMode::ALL.into_iter().find(|m| m.as_arg() == raw)
    }
}

/// Wallpaper host exit codes — ONE table for all modes. Shared codes mean the
/// same in any mode; a mode's own codes do not overlap.
pub mod host_exit {
    /// All good.
    pub const OK: i32 = 0;
    /// Nothing to show: argument parsing, no file, a foreign wallpaper type.
    pub const NOTHING_TO_SHOW: i32 = 1;
    /// QML did not come up.
    pub const QML_FAILED: i32 = 2;
    /// The frame stream did not start.
    pub const FRAMES_FAILED: i32 = 3;
    /// Video: sound was requested, but the file has no audio track.
    pub const VIDEO_NO_AUDIO: i32 = 4;
    /// Web: the page did not load.
    pub const WEB_PAGE_FAILED: i32 = 5;
    /// Flash: no Vulkan adapter able to draw into the wallpaper window was found,
    /// or the window surface was not created. Codes 7 and 8 match
    /// `rexon_swf::Error::exit_code`: the library and the host speak the same numbers.
    pub const SWF_NO_GPU: i32 = 7;
    /// Flash: the movie could not be read or parsed.
    pub const SWF_MOVIE_FAILED: i32 = 8;
}

// ── One wallpaper host per session ──────────────────────────────────────
//
// Live on 15.11.2026 (Cinnamon 6.8 Wayland): TWO video hosts ran side by
// side — ours as a desktop layer and a second one, started by a leftover
// copy of the shell extension from a previous version using old keys. Frames
// were decoded twice, and one was visible. The display host (video, web,
// Flash) is one for the whole session: it has as many windows as monitors,
// so "one per session" is also "one per monitor". A new host REPLACES the
// previous one (that one gets SIGTERM) rather than standing next to it: the
// new one is the latest decision of the person or the watcher. The lock is
// `flock` on a file in `$XDG_RUNTIME_DIR`, with the owner's PID in the file:
// the OS kernel releases the lock together with the process's death, so an
// abandoned file holds nothing after a crash, and a foreign process with
// the same PID is not harmed (the signal goes only to whoever holds the lock
// and whose command line is our host).

/// The wallpaper display host lock file for the session's display. The
/// display comes from `WAYLAND_DISPLAY`, otherwise `DISPLAY` (two sessions of
/// one person have their own hosts). `None` — there is no session directory,
/// nowhere to put the lock.
pub fn host_session_lock_path(runtime_dir: &str, wayland_display: &str, display: &str) -> Option<String> {
    let dir = runtime_dir.trim().trim_end_matches('/');
    if dir.is_empty() {
        return None;
    }
    let raw = if !wayland_display.trim().is_empty() { wayland_display.trim() } else { display.trim() };
    // The display can be a socket path or `:0` — only safe characters go into the file name.
    let name: String = raw
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
        .collect();
    let name = if name.is_empty() { "nodisplay".to_string() } else { name };
    Some(format!("{dir}/{WALLPAPER_HOST_BINARY}-{name}.lock"))
}

/// Whether this host launch takes the session lock. Only showing a wallpaper
/// on the desktop: the waywallen scene output is a different role (the daemon
/// holds it), and the self-check, the debug window (`--windowed`) and the
/// full-screen page preview (`--preview`) are not wallpapers and must not stop the real ones.
pub fn host_takes_session_lock(mode: HostMode, args: &[String]) -> bool {
    if mode == HostMode::Waywallen {
        return false;
    }
    !args
        .iter()
        .any(|a| a == "-w" || a == "--windowed" || a == "--preview" || a == "--selftest" || a.starts_with("--selftest="))
}

/// Is the process command line (`/proc/<pid>/cmdline`, NULs between words)
/// our wallpaper host? The signal by PID from the lock file goes only to it.
pub fn is_wallpaper_host_cmdline(cmdline: &[u8]) -> bool {
    let exe = cmdline.split(|b| *b == 0).next().unwrap_or_default();
    let exe = String::from_utf8_lossy(exe);
    exe.rsplit('/').next() == Some(WALLPAPER_HOST_BINARY)
}

/// PID from the lock file; garbage — `None`.
pub fn parse_host_lock_pid(text: &str) -> Option<i32> {
    text.trim().parse::<i32>().ok().filter(|p| *p > 1)
}

/// Whether the host refuses showing via frames into a file (that is how the
/// GNOME/Cinnamon shell extension launches it). On Cinnamon with layer-shell
/// Rexon puts the wallpaper with its own layer window, and a host started by
/// the extension is always superfluous: only a copy of the extension from a
/// previous version launches it that way (live on 15.11.2026 — a copy in
/// `~/.local/share/cinnamon/extensions` overrode the packaged one). The
/// refusal is an exit with code 0: an extension of any version takes it as
/// "stopped by itself" and does not complain with a popup.
pub fn stream_host_refused(stream_output: bool, cinnamon_layer_shell: bool) -> bool {
    stream_output && cinnamon_layer_shell
}

/// What to do with our Cinnamon extension when the wallpaper goes as a
/// desktop layer (Cinnamon with layer-shell): it does not show the wallpaper,
/// and everything left in its keys from the old path is a source of a
/// superfluous host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellExtRetire {
    /// Remove our uuid from `org.cinnamon enabled-extensions` (foreign ones stay
    /// as they were). Goes FIRST: a disabled extension will not wake up when its
    /// keys are edited. `None` — the extension is needed (a screen effect is on)
    /// or already disabled, or the list could not be read.
    pub disable_argv: Option<Vec<String>>,
    /// Clear the wallpaper in the extension's keys (the source and the command
    /// line) — commands for the extension schema (the executor fills in the
    /// schema directory).
    pub clear_argvs: Vec<Vec<String>>,
}

/// String value from a `gsettings get` answer (`'…'`); empty — empty.
fn gsettings_str_value(out: &str) -> String {
    let t = out.trim();
    let t = t.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')).unwrap_or(t);
    t.to_string()
}

/// The "disable our Cinnamon extension" command: the `enabled-extensions` list
/// without our uuid, the rest in the previous order. We are not in the list
/// or it is not Cinnamon — `None`.
pub fn shell_ext_disable_argv(de: &str, list_out: &str) -> Option<Vec<String>> {
    if !matches!(de, "cinnamon" | "cinnamon-mint") {
        return None;
    }
    let list: Vec<String> = list_out.split('\'').skip(1).step_by(2).map(str::to_string).collect();
    if !list.iter().any(|u| u == SHELL_EXT_ID) {
        return None;
    }
    let rest: Vec<String> = list.into_iter().filter(|u| u != SHELL_EXT_ID).collect();
    Some(vec![
        "gsettings".to_string(),
        "set".to_string(),
        "org.cinnamon".to_string(),
        "enabled-extensions".to_string(),
        gvariant_string_array(&rest),
    ])
}

/// Plan for taking the extension off the wallpaper on Cinnamon with layer-shell.
///
/// `source_out` — the answer of `gsettings get <schema> source`, `effect_out`
/// — of the `effect-shader` key, `enabled_out` — of `org.cinnamon
/// enabled-extensions`; `None` — asking failed. We disable only if we KNOW for
/// sure there is no screen effect: the effect lives in the same extension.
/// Bringing the extension back on old Cinnamon (without layer-shell) needs
/// nothing special: the wallpaper path via the extension enables a disabled
/// one by itself ([`shell_ext_enable_argv`]).
pub fn shell_ext_retire_plan(
    de: &str,
    source_out: Option<&str>,
    effect_out: Option<&str>,
    enabled_out: Option<&str>,
) -> ShellExtRetire {
    let Some(schema) = shell_ext_schema(de).filter(|_| de != "gnome") else {
        return ShellExtRetire::default();
    };
    let effect_off = effect_out.is_some_and(|o| gsettings_str_value(o).is_empty());
    let disable_argv = if effect_off { enabled_out.and_then(|o| shell_ext_disable_argv(de, o)) } else { None };
    let mut clear_argvs = Vec::new();
    if source_out.is_some_and(|o| !gsettings_str_value(o).is_empty()) {
        clear_argvs.push(argv(&["gsettings", "set", schema, "source", ""]));
        clear_argvs.push(argv(&["gsettings", "set", schema, SHELL_EXT_DAEMON_KEY, "[]"]));
    }
    ShellExtRetire { disable_argv, clear_argvs }
}

#[cfg(test)]
mod host_session_tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn lock_path_per_display() {
        assert_eq!(
            host_session_lock_path("/run/user/1000/", "wayland-0", ":0").as_deref(),
            Some("/run/user/1000/rexon-wallpaper-host-wayland-0.lock")
        );
        assert_eq!(
            host_session_lock_path("/run/user/1000", "", ":1").as_deref(),
            Some("/run/user/1000/rexon-wallpaper-host-_1.lock")
        );
        assert_eq!(
            host_session_lock_path("/run/user/1000", "/run/user/1000/wl-x", "").as_deref(),
            Some("/run/user/1000/rexon-wallpaper-host-wl-x.lock"),
            "путь к сокету — только имя"
        );
        assert_eq!(
            host_session_lock_path("/r", "", "").as_deref(),
            Some("/r/rexon-wallpaper-host-nodisplay.lock")
        );
        assert_eq!(host_session_lock_path("  ", "wayland-0", ""), None);
    }

    #[test]
    fn lock_only_for_desktop_show() {
        assert!(host_takes_session_lock(HostMode::Video, &s(&["/v.mp4", "--no-audio"])));
        assert!(host_takes_session_lock(HostMode::Web, &s(&["https://x"])));
        assert!(host_takes_session_lock(HostMode::Swf, &s(&["/a.swf"])));
        assert!(!host_takes_session_lock(HostMode::Waywallen, &s(&[])), "вывод сцен — другая роль");
        assert!(!host_takes_session_lock(HostMode::Video, &s(&["/v.mp4", "--windowed"])));
        assert!(!host_takes_session_lock(HostMode::Video, &s(&["/v.mp4", "-w"])));
        assert!(!host_takes_session_lock(HostMode::Web, &s(&["/i.html", "--selftest"])));
        assert!(!host_takes_session_lock(HostMode::Video, &s(&["/v.mp4", "--selftest=500"])));
        assert!(!host_takes_session_lock(HostMode::Web, &s(&["--preview", "/i.html"])), "a preview must not stop the wallpaper");
    }

    #[test]
    fn signal_only_our_host() {
        assert!(is_wallpaper_host_cmdline(b"/bin/rexon-wallpaper-host\0--mode\0video\0/v.mp4\0"));
        assert!(is_wallpaper_host_cmdline(b"rexon-wallpaper-host\0"));
        assert!(!is_wallpaper_host_cmdline(b"/usr/bin/cinnamon\0--replace\0"));
        assert!(!is_wallpaper_host_cmdline(b"/x/rexon-wallpaper-host-old\0"));
        assert!(!is_wallpaper_host_cmdline(b""));
        assert_eq!(parse_host_lock_pid("339145\n"), Some(339145));
        assert_eq!(parse_host_lock_pid("1"), None, "init не трогаем");
        assert_eq!(parse_host_lock_pid("мусор"), None);
    }

    #[test]
    fn stream_refused_only_on_cinnamon_layer_shell() {
        assert!(stream_host_refused(true, true));
        assert!(!stream_host_refused(true, false), "GNOME и старый Cinnamon — показ кадрами");
        assert!(!stream_host_refused(false, true), "наш хост слоем стола");
    }

    #[test]
    fn retire_disables_then_clears_keeping_foreign_uuids() {
        let schema = "org.cinnamon.extensions.rexon-wallpaper";
        let plan = shell_ext_retire_plan(
            "cinnamon-mint",
            Some("'/home/u/.cache/rexon/faststart/bb.mp4'\n"),
            Some("''\n"),
            Some("['a@x', 'rexon-wallpaper@rexon.app', 'b@y']\n"),
        );
        assert_eq!(
            plan.disable_argv.unwrap(),
            s(&["gsettings", "set", "org.cinnamon", "enabled-extensions", "[\"a@x\",\"b@y\"]"])
        );
        assert_eq!(
            plan.clear_argvs,
            vec![
                s(&["gsettings", "set", schema, "source", ""]),
                s(&["gsettings", "set", schema, "daemon-argv", "[]"]),
            ]
        );
    }

    #[test]
    fn retire_keeps_extension_for_screen_effect_and_writes_nothing_needless() {
        let plan = shell_ext_retire_plan(
            "cinnamon",
            Some("''\n"),
            Some("'/home/u/.config/rexon/screen-effects/shell-a.glsl'\n"),
            Some("['rexon-wallpaper@rexon.app']\n"),
        );
        assert_eq!(plan, ShellExtRetire::default(), "эффект идёт — расширение нужно; ключи уже пусты");
        // Asking about the effect failed — do not disable blindly.
        let plan = shell_ext_retire_plan("cinnamon", None, None, Some("['rexon-wallpaper@rexon.app']"));
        assert_eq!(plan.disable_argv, None);
        // Already disabled — leave the list alone.
        let plan = shell_ext_retire_plan("cinnamon", Some("''"), Some("''"), Some("['a@x']"));
        assert_eq!(plan.disable_argv, None);
        let plan = shell_ext_retire_plan("cinnamon", Some("''"), Some("''"), Some("@as []"));
        assert_eq!(plan.disable_argv, None);
        // Not Cinnamon — nothing.
        assert_eq!(shell_ext_retire_plan("gnome", Some("'/x'"), Some("''"), Some("['rexon-wallpaper@rexon.app']")), ShellExtRetire::default());
        assert_eq!(shell_ext_retire_plan("kde", Some("'/x'"), Some("''"), None), ShellExtRetire::default());
    }
}

/// Refusal when the video wallpaper host is not found.
///
/// Previously there was a silent fallback to the `rexon.video_wallpaper`
/// Python module here; the module was removed from the tree, and instead the
/// person gets an explanation of WHAT is wrong and what to do about it (rule
/// #13). An empty screen with a green «wallpaper applied» report is exactly
/// the class of breakage that has already been fixed in the project.
pub fn video_host_missing_message() -> String {
    tr("There's nothing to show live wallpapers with: the rexon-wallpaper-host \
        program wasn't found. It's installed together with Rexon — reinstall Rexon. \
        If you work from the source tree, build it: cargo build --release -p rexon-gui.")
}

/// Where to look for the built wallpaper host — in decreasing freshness: the
/// working tree (`rust/target`, release then debug), then the caller's
/// `$PATH` — that is where the install puts the binary. The same set and
/// order as for the splash helper
/// ([`crate::session_splash::splash_overlay_candidates`]).
pub fn host_candidates(repo_root: &str, binary: &str) -> Vec<String> {
    let root = repo_root.trim_end_matches('/');
    vec![
        format!("{root}/rust/target/release/{binary}"),
        format!("{root}/rust/target/debug/{binary}"),
    ]
}

/// The built wallpaper host or `None` — nothing to show with. ONE lookup for
/// all modes (rule #12): while there were two hosts, a diverged order would
/// have meant video from a fresh build and web from yesterday's install.
///
/// `None` does not mean "let's take the Python path": there is no fallback
/// path, and the caller must refuse ([`VIDEO_HOST_MISSING_MESSAGE`],
/// [`WEB_HOST_MISSING`]) rather than pretend the wallpaper was applied. File
/// system checks are outside (the [`DaemonFs`] trait, the same as for the
/// waywallen daemon lookup).
pub fn find_wallpaper_host_binary<FS: DaemonFs>(fs: &mut FS, repo_root: &str) -> Option<String> {
    for cand in host_candidates(repo_root, WALLPAPER_HOST_BINARY) {
        if fs.is_exec_file(&cand) {
            return Some(cand);
        }
    }
    fs.which(WALLPAPER_HOST_BINARY)
}

/// The wallpaper host command line in mode `mode` — ONE for all formats and
/// all display paths: `<host> --mode <mode> <path>`.
///
/// The wallpaper host environment variable: when the executor started the
/// apply (ms since the epoch, the journal's «applying …» line). By it the host
/// measures how long the person waited for the first frame
/// ([`first_frame_log_line`]).
pub const APPLY_STARTED_ENV: &str = "REXON_WP_APPLY_MS";

/// The "first frame on screen" journal line: `what` — what is shown («video»,
/// «web page», «Flash»), `since_apply_ms` — from the start of the apply
/// ([`APPLY_STARTED_ENV`]; `None` — the host was not launched by the
/// executor), `since_host_ms` — from the host process launch.
pub fn first_frame_log_line(what: &str, since_apply_ms: Option<u64>, since_host_ms: u64) -> String {
    match since_apply_ms {
        Some(total) => format!(
            "первый кадр ({what}) на экране через {total} мс после «применяю» (хост — {since_host_ms} мс от запуска)"
        ),
        None => format!("первый кадр ({what}) на экране через {since_host_ms} мс от запуска хоста"),
    }
}

/// How much time has passed since the start of the apply: `env_value` — the
/// [`APPLY_STARTED_ENV`] value, `now_ms` — now. Empty, garbage or a time from
/// the future — `None`.
pub fn since_apply_ms(env_value: &str, now_ms: u64) -> Option<u64> {
    let started: u64 = env_value.trim().parse().ok()?;
    (started > 0 && started <= now_ms).then(|| now_ms - started)
}

/// The mode is always written, including for video, even though a host
/// without the switch shows video anyway ([`HostMode::DEFAULT`]): the command
/// line should make it visible what it launches.
pub fn wallpaper_host_argv(host: &str, mode: HostMode, path: &str) -> Vec<String> {
    vec![
        host.to_string(),
        HOST_MODE_FLAG.to_string(),
        mode.as_arg().to_string(),
        path.to_string(),
    ]
}

/// The host command line in video mode — [`wallpaper_host_argv`] plus the
/// muted-wallpaper flag.
///
/// It launches both our own window ([`video_native_plan`]) and the daemon
/// under the GNOME/Cinnamon shell extension ([`shell_ext_plan`]). `host` is
/// the path to the built binary ([`find_wallpaper_host_binary`]); Python
/// takes part nowhere in the process chain and substitutes for nothing: no
/// binary found — a refusal ([`VIDEO_HOST_MISSING_MESSAGE`]), not another host.
pub fn video_host_argv(host: &str, path: &str, has_audio: bool) -> Vec<String> {
    let mut cmd = wallpaper_host_argv(host, HostMode::Video, path);
    if !has_audio {
        cmd.push("--no-audio".into());
    }
    cmd
}

/// Launching our own video wallpaper host.
///
/// Works on any Wayland with wlr_layer_shell and on X11 via the xprop trick
/// `_NET_WM_WINDOW_TYPE_DESKTOP`.
///
/// `host` is the path to the built binary ([`find_wallpaper_host_binary`]): it
/// becomes the command line, Python does not remain in the chain. No binary —
/// control must not get here: the caller refuses earlier with the
/// [`VIDEO_HOST_MISSING_MESSAGE`] text.
///
/// ⚠️ Volume and "sound on/off" are NO LONGER PASSED here, and that is not an
/// omission. While they travelled as launch switches, the sound setting
/// reached the wallpaper exactly once — at process start — so moving the
/// slider did nothing until the wallpaper was set again. Now both values live
/// in `settings.json`, and the host watches the file and applies the change on
/// the fly (`rexon-core::wallpaper_audio`).
///
/// `has_audio` is NOT the person's setting but a property of the wallpaper
/// itself: "it is meant to have sound". The difference is fundamental: the
/// person changes the setting on the fly, but a muted wallpaper cannot be
/// given sound. The switch is set only in the negative case — the default
/// comes from the wallpaper's `project.json`.
pub fn video_native_plan(host: &str, path: &str, has_audio: bool) -> Vec<Step> {
    let cmd = video_host_argv(host, path, has_audio);
    let name = base_name(path);
    vec![Step::new(step_kind::SPAWN_SESSION, "video_wallpaper", cmd)
        // TRANSLATORS: %1 is a file name.
        .ok(trf("Video wallpaper started: %1", &[&name]))
        .err({
            // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
            tr("Couldn't start video_wallpaper: {stderr}")
        })
        .fmt("video")
        .terminal()]
}

// ── YouTube ────────────────────────────────────────────────────────────────

/// yt-dlp format for the quality; an unknown quality is treated as `1080p`.
pub fn youtube_format(quality: &str) -> String {
    let h = match quality {
        "4k" => 2160,
        "1440p" => 1440,
        "720p" => 720,
        "480p" => 480,
        "best" => {
            return "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best".to_string();
        }
        _ => 1080,
    };
    format!("bestvideo[height<={h}][ext=mp4]+bestaudio[ext=m4a]/best[height<={h}]")
}

/// The YouTube wallpaper mode that can actually be carried out in this session.
///
/// "Stream" is shown by `mpvpaper` (Wayland) or `xwinwrap`+`mpv` (X11) — both
/// put the picture with THEIR OWN window. On GNOME-Wayland and
/// Cinnamon-Wayland there is no own window at all: `mpvpaper` works via
/// wlr-layer-shell, which Mutter and Muffin never have — there the wallpaper
/// is shown by the shell extension, and it takes frames from the host, which
/// needs a file on disk. So in such sessions "stream" silently becomes
/// "download": the customer needs a wallpaper, not a choice between an error
/// and a black screen (rule #13).
///
/// The decision is ONE and shared with video wallpapers — [`needs_shell_extension`].
pub fn youtube_effective_mode(de: &str, session_type: &str, mode: &str) -> &'static str {
    if mode == "download" || needs_shell_extension(de, session_type) {
        "download"
    } else {
        "stream"
    }
}

/// An explanation of why "stream" was replaced by a download in this environment.
/// An empty string — there was no replacement.
pub fn youtube_mode_downgraded_note(de: &str, session_type: &str, mode: &str) -> String {
    if mode == "download" || !needs_shell_extension(de, session_type) {
        return String::new();
    }
    let de = shell_ext_de_label(de);
    trf(
        "On %1 live wallpapers are shown by the shell extension, and it needs a file \
         on disk — the video was downloaded instead of streamed.",
        &[&de],
    )
}

/// YouTube wallpaper plan. On KDE the route is the same as everywhere — by session type.
///
/// `de` is needed so that this plan CANNOT launch `mpvpaper` where it has
/// nothing to draw on (GNOME/Cinnamon-Wayland): such sessions go to the shell
/// extension through [`youtube_effective_mode`], and control must not get
/// here. The check is here in case of a direct call.
pub fn youtube_plan(
    de: &str,
    session_type: &str,
    url: &str,
    audio: bool,
    volume: i64,
    quality: &str,
    tools: &[String],
    monitor: &str,
    resolution: &str,
    hints: &Hints,
) -> Vec<Step> {
    let fmt = youtube_format(quality);
    if needs_shell_extension(de, session_type) {
        return vec![fail(trf(
            "On %1 a YouTube video can't be streamed: wallpapers there are shown by the \
             shell extension, and it needs a file on disk. Choose the “download” mode.",
            &[&shell_ext_de_label(de)],
        ))];
    }
    if session_type == "wayland" {
        if !has(tools, "mpvpaper") {
            return vec![fail(mpvpaper_missing_message(&hints.mpvpaper))];
        }
        let audio_opt = if audio { "" } else { " no-audio" };
        let vol_opt = if audio {
            format!(" volume={volume}")
        } else {
            String::new()
        };
        let opts = format!("loop=inf hwdec=auto ytdl-format={fmt}{audio_opt}{vol_opt}");
        return vec![Step::new(
            step_kind::SPAWN,
            "mpvpaper",
            vec![
                "mpvpaper".into(),
                "-o".into(),
                opts,
                monitor.to_string(),
                url.to_string(),
            ],
        )
        .ok(tr("YouTube wallpaper started"))
        .err({
            // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
            tr("mpvpaper error: {stderr}")
        })
        .fmt("youtube")
        .terminal()];
    }

    if !has(tools, "xwinwrap") {
        return vec![fail(xwinwrap_missing_message(&hints.xwinwrap))];
    }
    if !has(tools, "mpv") {
        return vec![fail(tr("mpv isn't installed."))];
    }
    let mut cmd = vec![
        "xwinwrap".to_string(),
        "-g".to_string(),
        xwinwrap_geometry(resolution),
        "-ov".into(),
        "-ni".into(),
        "-s".into(),
        "-nf".into(),
        "--".into(),
        "mpv".into(),
        "--loop=inf".into(),
        format!("--ytdl-format={fmt}"),
    ];
    if !audio {
        cmd.push("--no-audio".into());
    } else {
        cmd.push(format!("--volume={volume}"));
    }
    cmd.extend(argv(&[
        "--no-osc",
        "--no-osd-bar",
        "--no-input-default-bindings",
        "--hwdec=auto",
        "--gpu-api=auto",
        "--wid=WID",
    ]));
    cmd.push(url.to_string());
    vec![Step::new(step_kind::SPAWN, "xwinwrap", cmd)
        .ok(tr("YouTube wallpaper started"))
        .err({
            // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
            tr("xwinwrap error: {stderr}")
        })
        .fmt("youtube")
        .terminal()]
}

/// No yt-dlp — without it a YouTube video cannot be downloaded.
pub fn youtube_no_ytdlp_message(hint: &str) -> String {
    // TRANSLATORS: %1 is a hint on how to install yt-dlp.
    trf("The yt-dlp program wasn't found; YouTube videos can't be downloaded without it. %1", &[&hint])
}

/// The link is not from a supported site.
pub fn youtube_unsupported_message(url: &str) -> String {
    trf("Unsupported URL: %1", &[&url])
}

/// The download failed.
pub fn youtube_download_failed() -> String {
    tr("Couldn't download the YouTube video. Try another quality or the streaming mode.")
}

/// The same, but adjusted for the environment.
///
/// ⚠️ The bare [`YOUTUBE_DOWNLOAD_FAILED`] on GNOME and Cinnamon-Wayland
/// advises the impossible: there is no stream there at all — the wallpaper is
/// shown by the shell extension, which needs a file on disk, which is why
/// "stream" is replaced by a download ([`youtube_effective_mode`]). A person
/// following the advice would switch the mode to "stream", get a download
/// again — and go round in circles.
pub fn youtube_download_failed_message(de: &str, session_type: &str) -> String {
    if !needs_shell_extension(de, session_type) {
        return youtube_download_failed();
    }
    let de = shell_ext_de_label(de);
    trf(
        "Couldn't download the YouTube video. Try another quality or another link: \
         on %1 the video can't be streamed — wallpapers there are shown by the shell \
         extension, and it needs a file on disk.",
        &[&de],
    )
}

// ── web ────────────────────────────────────────────────────────────────────

/// Is the path/URL a link?
pub fn is_url(path: &str) -> bool {
    path.starts_with("http://") || path.starts_with("https://")
}

/// There is no wallpaper host on the system — an explanation for a customer
/// who chose a web wallpaper.
///
/// ⚠️ The text is exactly this, not «web_wallpaper error: code 127». The
/// refusal is made BEFORE launching, because `None` here does not mean "no
/// path was passed" but "the core has already looked and found it neither in
/// the build tree nor in `$PATH`" ([`find_wallpaper_host_binary`]). Spawning
/// in such a situation would give the person a flash of an empty desktop and
/// an exit code instead of an answer (rule #13). There is no Python fallback:
/// the `rexon.web_wallpaper` module was removed from the tree on 30.07.2026.
pub fn web_host_missing() -> String {
    tr("There's nothing to show web wallpapers with: the `rexon-wallpaper-host` \
        program wasn't found next to Rexon or in PATH. It's built together with Rexon — \
        reinstall the program or build it again.")
}

/// Web wallpaper plan: first our own QtWebEngine host, on X11 also surf.
///
/// ⚠️ `python` no longer takes part in the first step and is left in the
/// signature on purpose: the Python side of the engine knows it, and changing
/// it now is not allowed. Web wallpapers are launched by our binary.
///
/// `host` is the path to it ([`find_wallpaper_host_binary`]); `None` — it is
/// nowhere, and the first step is an honest refusal, not launching something
/// that does not exist.
pub fn web_plan(
    session_type: &str,
    python: &str,
    path: &str,
    abs_path: &str,
    tools: &[String],
    resolution: &str,
    hints: &Hints,
    host: Option<&str>,
) -> Vec<Step> {
    let _ = python;
    let mut steps = vec![match host {
        Some(bin) => Step::new(
            step_kind::SPAWN_SESSION,
            "web_wallpaper",
            wallpaper_host_argv(bin, HostMode::Web, path),
        )
        .ok(tr("Web wallpaper started"))
        .err({
            // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
            tr("web_wallpaper error: {stderr}")
        })
        .fmt("web"),
        None => fail(web_host_missing()).fmt("web"),
    }];

    if session_type == "wayland" {
        // On Wayland our own host is the only path: xwinwrap does not work there.
        steps[0] = steps[0].clone().terminal();
        return steps;
    }
    if has(tools, "surf") && has(tools, "xwinwrap") {
        // surf accepts both a file:// URI and an http(s) URL.
        let uri = if is_url(path) {
            path.to_string()
        } else {
            format!("file://{abs_path}")
        };
        steps.push(
            Step::new(
                step_kind::SPAWN,
                "surf",
                vec![
                    "xwinwrap".into(),
                    "-g".into(),
                    xwinwrap_geometry(resolution),
                    "-ov".into(),
                    "-ni".into(),
                    "-s".into(),
                    "-nf".into(),
                    "--".into(),
                    "surf".into(),
                    "-dfgkKmns".into(),
                    uri,
                ],
            )
            .ok(tr("Web wallpaper started"))
            .err({
                // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
                tr("Couldn't start surf: {stderr}")
            })
            .fmt("web")
            .terminal(),
        );
    } else {
        let hint = if hints.webkit.is_empty() {
            tr("(the webkit2gtk-4.1 package)")
        } else {
            hints.webkit.clone()
        };
        // TRANSLATORS: %1 is a hint on which package to install.
        steps.push(fail(trf("Web wallpapers need the WebKitGTK engine %1", &[&hint])));
    }
    steps
}

/// The HTML file was not found.
pub fn web_missing_message(path: &str) -> String {
    trf("HTML file not found: %1", &[&path])
}

// ── scene ──────────────────────────────────────────────────────────────────

/// Scene wallpapers are no longer supported — exactly like application
/// wallpapers below.
///
/// Rexon has no scene renderer of its own and will not have one:
/// `parallax_engine` (and the whole `scene_gl`/`scene_mesh` class) was removed
/// from the tree deliberately, scenes are drawn by the waywallen engine. The
/// former code still launched `python -m rexon.parallax_engine`: the process
/// died right away, while the customer was told «Scene Engine started».
///
/// The text tells the customer exactly the truth: there is nothing to show
/// our own scene format (`.rxn` layers) with, and Workshop scenes open in the
/// «Wallpaper Engine» tab. Promising that THIS file will work there is not
/// allowed: waywallen applies wallpapers by the identifier of its library
/// (`apply_by_id`), an arbitrary path cannot be passed to it.
pub fn scene_unsupported() -> String {
    tr("Scene wallpapers are no longer supported. Scenes from the Wallpaper Engine \
        Workshop open in the “Wallpaper Engine” tab")
}

// ── Flash (SWF) ────────────────────────────────────────────────────────────

/// Ruffle from a CDN — the fallback path when there is no native binary.
pub const RUFFLE_CDN: &str = "https://unpkg.com/@ruffle-rs/ruffle";

/// The SWF file was not found.
pub fn swf_missing_message(path: &str) -> String {
    trf("SWF file not found: %1", &[&path])
}

/// Flash wallpaper with our own host: `rexon-wallpaper-host --mode swf <file>`
/// in its own session — Ruffle as a library (the `rexon-swf` crate), without
/// an external `ruffle`, without downloading and without an HTML wrapper. The
/// command line is assembled by the same function as for video and web
/// ([`wallpaper_host_argv`]).
///
/// ⚠️ The line carries no sound switches, and that does NOT mean "muted"
/// (since 16.09.2026). Volume and "sound on/off" live in `settings.json`
/// (`wallpaper_audio_*`) — the same keys as for video — and the host watches
/// the file and applies the change on the fly. They cannot be passed as a
/// switch for the same reason as for video: the setting would reach the
/// wallpaper exactly once, at process start. The non-linearity of Ruffle's
/// scale (0.5 there gives a level roughly 10 times quieter than 1.0) is
/// removed by a conversion inside `rexon-swf` and does not leak outside.
pub fn swf_host_plan(host: &str, path: &str) -> Vec<Step> {
    let cmd = wallpaper_host_argv(host, HostMode::Swf, path);
    let name = base_name(path);
    vec![Step::new(step_kind::SPAWN_SESSION, "swf_wallpaper", cmd)
        .ok(trf("Flash wallpaper started: %1", &[&name]))
        .err({
            // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
            tr("Couldn't start the Flash wallpaper: {stderr}")
        })
        .fmt("swf")
        .terminal()]
}

/// Launching an external Ruffle in full-screen mode.
///
/// ⚠️ The CLASSIC window's path (Python `_apply_swf` via the
/// `wpe_swf_native_plan` bridge). The Rust executor shows Flash with its own
/// host — [`swf_host_plan`]. The function lives as long as Python calls it.
pub fn swf_native_plan(ruffle: &str, swf_abs: &str) -> Vec<Step> {
    vec![Step::new(
        step_kind::SPAWN,
        "ruffle",
        argv(&[ruffle, "--fullscreen", swf_abs]),
    )
    .ok(tr("SWF wallpaper started"))
    .fmt("swf")]
}

/// Two xprop commands that make a foreign window a desktop on X11.
pub fn xprop_desktop_argvs(wid: &str) -> Vec<Vec<String>> {
    vec![
        argv(&[
            "xprop", "-id", wid, "-f", "_NET_WM_WINDOW_TYPE", "32a", "-set",
            "_NET_WM_WINDOW_TYPE", "_NET_WM_WINDOW_TYPE_DESKTOP",
        ]),
        argv(&[
            "xprop",
            "-id",
            wid,
            "-f",
            "_NET_WM_STATE",
            "32a",
            "-set",
            "_NET_WM_STATE",
            "_NET_WM_STATE_BELOW,_NET_WM_STATE_SKIP_TASKBAR,\
             _NET_WM_STATE_SKIP_PAGER,_NET_WM_STATE_STICKY",
        ]),
    ]
}

/// Finding a window by PID for the xprop trick.
pub fn xdotool_search_argv(pid: i64) -> Vec<String> {
    vec![
        "xdotool".into(),
        "search".into(),
        "--pid".into(),
        pid.to_string(),
    ]
}

/// Where to get Ruffle Web for the HTML wrapper: our own copy → a copy from
/// .rxn → a CDN (needs the internet).
pub fn swf_ruffle_src(rexon_copy: Option<&str>, local_copy: Option<&str>) -> String {
    if let Some(p) = rexon_copy {
        return format!("file://{p}");
    }
    if let Some(p) = local_copy {
        return format!("file://{p}");
    }
    RUFFLE_CDN.to_string()
}

/// An HTML wrapper around SWF: Ruffle WASM inside our web host.
pub fn swf_wrapper_html(swf_name: &str, ruffle_src: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">
<title>Rexon SWF Wallpaper</title>
<style>
  * {{ margin: 0; padding: 0; overflow: hidden; }}
  html, body {{ width: 100vw; height: 100vh; background: #000; }}
  #player {{
    width: 100vw;
    height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
  }}
  ruffle-embed, ruffle-object, ruffle-player, embed, object {{
    width: 100% !important;
    height: 100% !important;
  }}
</style>
</head><body>
<div id="player">
  <embed src="{swf_name}"
         type="application/x-shockwave-flash"
         width="100%" height="100%"
         quality="high" wmode="transparent"
         allowScriptAccess="never"
         loop="true" play="true"
         scale="showAll">
</div>
<script src="{ruffle_src}"></script>
</body></html>"#
    )
}

// ── KDE: plasmashell scripts and the desktops config ───────────────────────

/// The stock static Plasma wallpaper plugin.
pub const KDE_IMAGE_PLUGIN: &str = "org.kde.image";

/// Rexon's wallpaper plugin, removed on 2026-07-28. The name is needed only
/// to recognize and cure an old version's config: live wallpapers on KDE are
/// drawn by the waywallen engine.
pub const REMOVED_KDE_PLUGIN: &str = "org.rexon.wallpaper";

/// Script that sets a static picture on all desktops.
pub fn kde_static_script(path: &str) -> String {
    format!(
        "\nvar a = desktops();\nfor (var i = 0; i < a.length; i++) {{\n    \
         a[i].wallpaperPlugin = \"org.kde.image\";\n    \
         a[i].currentConfigGroup = [\"Wallpaper\",\"org.kde.image\",\"General\"];\n    \
         a[i].writeConfig(\"Image\", \"file://{path}\");\n}}\n"
    )
}

/// Script that returns desktops to the stock plugin with a clean picture.
///
/// THE ORDER MATTERS: first write the clean picture into the in-memory
/// `org.kde.image` config, THEN switch the plugin — otherwise the plugin
/// loads with the old (blurred frame) Image from plasmashell's memory.
pub fn kde_reset_script(clean_image: &str) -> String {
    let img_js = if clean_image.is_empty() {
        String::new()
    } else {
        format!(
            "  d[i].currentConfigGroup = ['Wallpaper','org.kde.image','General'];\
             \x20 d[i].writeConfig('Image', {});",
            serde_json::Value::String(clean_image.to_string())
        )
    };
    format!(
        "var d = desktops();for (var i = 0; i < d.length; i++) {{{img_js}\
         \x20 d[i].wallpaperPlugin = 'org.kde.image';}}"
    )
}

/// Script that gives the desktop back a foreign wallpaper plugin.
pub fn kde_restore_plugin_script(plugin: &str) -> String {
    format!(
        "var d = desktops();for (var i = 0; i < d.length; i++) {{  \
         d[i].wallpaperPlugin = '{plugin}';}}"
    )
}

/// Script that reads the wallpaper plugins of ALL desktops — live, not from
/// the config: the file on disk lags behind, plasmashell keeps settings in
/// memory and writes them whenever it wants.
///
/// One query answers two engine questions at once: whose desktop this is (to
/// whom to give it back after «Stop») and whether Rexon's removed plugin is
/// still on it. There may be several desktops, so a comma-separated LIST is
/// printed rather than the first one found: previously `d[0]` was taken, and
/// a stuck plugin on the second monitor went unnoticed.
///
/// The containment number is not named here at all — `desktops()` returns
/// exactly the desktops. This is the cure for the class of defect we got
/// burned on with the wallpaper picture: the number differs on every machine,
/// and reading by a hard-coded one returned emptiness instead of an answer.
pub const KDE_READ_PLUGIN_SCRIPT: &str =
    "var d = desktops(); var p = []; for (var i = 0; i < d.length; i++) \
     p.push(d[i].wallpaperPlugin); print(p.join(','));";

/// Script that resets the plugin to the stock one (without a picture) — for recovery.
pub const KDE_PLAIN_RESET_SCRIPT: &str =
    "var allDesktops = desktops();for (var i = 0; i < allDesktops.length; i++) {\
     \x20 allDesktops[i].wallpaperPlugin = 'org.kde.image';}";

/// An `evaluateScript` call via `dbus-send`.
pub fn plasma_dbus_argv(script: &str) -> Vec<String> {
    vec![
        "dbus-send".into(),
        "--session".into(),
        "--dest=org.kde.plasmashell".into(),
        "--type=method_call".into(),
        "/PlasmaShell".into(),
        "org.kde.PlasmaShell.evaluateScript".into(),
        format!("string:{script}"),
    ]
}

/// The same call, but with a reply (`--print-reply`).
pub fn plasma_dbus_reply_argv(script: &str) -> Vec<String> {
    let mut v = plasma_dbus_argv(script);
    v.insert(2, "--print-reply".into());
    v
}

/// A string field from a `dbus-send` reply.
pub fn parse_dbus_string(stdout: &str) -> Option<String> {
    let idx = stdout.find("string \"")?;
    let rest = &stdout[idx + "string \"".len()..];
    let end = rest.find('"')?;
    let name = rest[..end].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// KDE desktop wallpaper plugins from the `dbus-send` reply to
/// [`KDE_READ_PLUGIN_SCRIPT`].
///
/// The only parser of this reply: both "whose desktop" and "is the removed
/// plugin still there" are computed from here, so that the two questions do
/// not drift apart over two ways of reading. An empty reply (plasmashell is
/// silent) is an empty list: silence is not passed off as "no plugins".
pub fn kde_desktop_plugins(dbus_stdout: &str) -> Vec<String> {
    parse_dbus_string(dbus_stdout)
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// A FOREIGN desktop wallpaper plugin — the one the desktop must be given
/// back to after stopping a live wallpaper (`None` if there is nothing to
/// give back).
///
/// TWO names are filtered out, not one. The stock [`KDE_IMAGE_PLUGIN`] —
/// because the stop returns to it anyway. And [`REMOVED_KDE_PLUGIN`] —
/// because there is nothing left in the config to draw it with: having
/// remembered the removed plugin, the stop first honestly reset the desktop
/// to `org.kde.image` and then itself put the dead plugin back, and the
/// desktop stayed empty.
pub fn kde_foreign_wallpaper_plugin(dbus_stdout: &str) -> Option<String> {
    kde_desktop_plugins(dbus_stdout)
        .into_iter()
        .find(|p| p != KDE_IMAGE_PLUGIN && p != REMOVED_KDE_PLUGIN)
}

/// Whether Rexon's REMOVED wallpaper plugin is set on at least one desktop.
///
/// For a user who upgraded from an old version, the desktop may have stayed
/// configured to `org.rexon.wallpaper`: Plasma loads a non-existent plugin,
/// and the desktop is empty. We ask Plasma itself, not `kreadconfig6` by
/// containment number — we already got burned on that kind of reading.
pub fn kde_removed_plugin_selected(dbus_stdout: &str) -> bool {
    kde_desktop_plugins(dbus_stdout)
        .iter()
        .any(|p| p == REMOVED_KDE_PLUGIN)
}

/// Identifiers of the desktop containments from `plasma-…-appletsrc`.
///
/// `wallpaperplugin` is checked EXACTLY on them — otherwise a stuck plugin in
/// an inactive containment gives a false positive. Nothing found — the
/// historical `128`.
pub fn desktop_containment_ids(text: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(id) = line
            .strip_prefix("[Containments][")
            .and_then(|r| r.strip_suffix(']'))
            .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
        {
            current = Some(id.to_string());
        } else if let (Some(cur), true) = (current.as_ref(), line.starts_with("plugin=")) {
            if (line.contains("folder") || line.contains("desktopcontainment"))
                && !ids.contains(cur)
            {
                ids.push(cur.clone());
            }
        }
    }
    if ids.is_empty() {
        ids.push("128".to_string());
    }
    ids
}

/// Wallpaper pictures of ALL KDE desktops from
/// `plasma-org.kde.plasma.desktop-appletsrc`.
///
/// The containment number is NOT guessed here: all
/// `[Containments][N][Wallpaper][org.kde.image][General]` groups with an
/// `Image` key are taken, in file order. This is the same technique as in
/// [`desktop_containment_ids`], and it is more reliable than reading by the
/// hard-coded number `128`: on Wehrwolfmann's machine the desktop has number
/// `216`, and a query by `128` returned an empty string. The engine no longer
/// builds the read-by-hard-coded-number command at all — it went away
/// together with the last one who called it.
///
/// The appearance catalog asks here whether a wallpaper pack is applied right
/// now (`installed_scan::wallpaper_pack_name`).
pub fn kde_wallpaper_images(appletsrc: &str) -> Vec<String> {
    kde_wallpaper_image_entries(appletsrc)
        .into_iter()
        .map(|(_, image)| image)
        .collect()
}

/// Pairs of "containment number → its `org.kde.image` picture", in file order.
///
/// Shared parsing for [`kde_wallpaper_images`] and [`kde_desktop_static_image`]:
/// the first needs all pictures, the second also whose containment it is.
fn kde_wallpaper_image_entries(appletsrc: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in appletsrc.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            current = s
                .strip_prefix("[Containments][")
                .and_then(|r| r.strip_suffix("][Wallpaper][org.kde.image][General]"))
                .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
                .map(|id| id.to_string());
            continue;
        }
        let Some(id) = current.as_ref() else {
            continue;
        };
        if s.starts_with('#') {
            continue;
        }
        if let Some(v) = s.strip_prefix("Image=") {
            let v = v.trim();
            if !v.is_empty() {
                out.push((id.clone(), v.to_string()));
            }
        }
    }
    out
}

/// The static picture of the DESKTOP — what we return to after stopping a
/// live wallpaper.
///
/// A live wallpaper changes the containment's `wallpaperplugin`, but the
/// `org.kde.image` group stays untouched in the config: that is where the
/// user's real wallpaper lies. We take it from the containment that
/// [`desktop_containment_ids`] recognized as a desktop: the panel also gets
/// its own `org.kde.image` group with an `Image` key from Plasma, and "the
/// first picture in the file" would choose it instead of the desktop.
///
/// The `file://` scheme is stripped: Plasma writes a URL into the config,
/// while the wallpaper engine everywhere treats the saved wallpaper as a file
/// system path — checks `os.path.exists` and substitutes it into the command
/// line. With a URL that check did not pass, and the restore after a stop
/// silently did not happen.
pub fn kde_desktop_static_image(appletsrc: &str) -> Option<String> {
    let entries = kde_wallpaper_image_entries(appletsrc);
    let desktops = desktop_containment_ids(appletsrc);
    entries
        .iter()
        .find(|(id, _)| desktops.contains(id))
        .or_else(|| entries.first())
        .map(|(_, image)| image.replace("file://", ""))
}

/// Our cache in the picture path = it is the blurred frame, not the real wallpaper.
pub const CACHE_MARKER: &str = "/.cache/rexon/";

/// Whether a candidate is fit as a "clean" picture to restore after a stop.
///
/// `exists` — the shim's observation: the file (or the wallpaper package directory) is in place.
pub fn clean_image_candidate_ok(candidate: &str, exists: bool) -> bool {
    exists && !candidate.replace("file://", "").contains(CACHE_MARKER)
}

/// Distribution wallpaper directories — a fallback for restoring.
pub const DISTRO_WALLPAPER_DIRS: &[&str] = &[
    "/usr/share/wallpapers/Next",
    "/usr/share/wallpapers/cachyos-wallpapers",
];

/// Where Plasma wallpaper packages lie — the last fallback for restoring,
/// when neither a saved wallpaper nor distribution directories were found.
///
/// A package is a subdirectory with `contents/images` inside: the directory
/// name by itself says nothing, and without pictures Plasma would get an empty
/// desktop — that is, exactly what the restore insures against.
pub const PLASMA_WALLPAPER_ROOT: &str = "/usr/share/wallpapers";

// ── blur and frame ─────────────────────────────────────────────────────────

/// Extensions for which the frame is taken by copying rather than via ffmpeg.
pub const IMAGE_EXTS: &[&str] = &[".png", ".jpg", ".jpeg", ".webp", ".bmp"];

/// Extracting one frame: the default `timestamp` of 3 s skips intros.
pub fn extract_frame_argv(path: &str, output: &str, timestamp: f64) -> Vec<String> {
    vec![
        "ffmpeg".into(),
        "-y".into(),
        "-ss".into(),
        format_float(timestamp),
        "-i".into(),
        path.to_string(),
        "-vframes".into(),
        "1".into(),
        "-q:v".into(),
        "2".into(),
        output.to_string(),
    ]
}

/// Where to put the blurred wallpaper copy: `<cache>/rexon/blur/<name>_blur.jpg`.
///
/// ⭐ A fix: Python wrote the copy NEXT TO the source ([`blur_output_path`]),
/// and distribution wallpapers lie in `/usr/share/…`, which is not writable —
/// and «Blur» did not work on stock wallpapers at all. Besides, the Rexon
/// cache is recognized by [`CACHE_MARKER`]: a stop will not put the blurred
/// copy back on the desktop instead of the real wallpaper.
pub fn blur_cache_output_path(cache_home: &str, source: &str) -> String {
    let name = source.rsplit('/').next().unwrap_or(source);
    let (stem, _) = split_ext(name);
    format!("{}/rexon/blur/{stem}_blur.jpg", cache_home.trim_end_matches('/'))
}

/// Blur a picture and write a JPEG — port of `_blur_image` without Pillow.
///
/// As in the original: a large picture is first shrunk within
/// [`BLUR_MAX_SIZE`] with Lanczos (the blur hides the details anyway), then
/// Gaussian blur with strength `radius`, JPEG of quality [`BLUR_JPEG_QUALITY`].
pub fn blur_image_file(source: &str, output: &str, radius: i64) -> Result<(), String> {
    let img = image::ImageReader::open(source)
        .map_err(|e| trf("couldn't open the image %1: %2", &[&source, &e]))?
        .with_guessed_format()
        .map_err(|e| trf("couldn't read the image %1: %2", &[&source, &e]))?
        .decode()
        .map_err(|e| trf("couldn't decode the image %1: %2", &[&source, &e]))?;
    let (max_w, max_h) = (BLUR_MAX_SIZE.0.max(1) as u32, BLUR_MAX_SIZE.1.max(1) as u32);
    let img = if img.width() > max_w || img.height() > max_h {
        img.resize(max_w, max_h, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let blurred = img.blur(radius.max(1) as f32).to_rgb8();
    if let Some(dir) = std::path::Path::new(output).parent() {
        std::fs::create_dir_all(dir).map_err(|e| trf("couldn't create the cache folder: %1", &[&e]))?;
    }
    let file = std::fs::File::create(output).map_err(|e| trf("couldn't create %1: %2", &[&output, &e]))?;
    let mut writer = std::io::BufWriter::new(file);
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
        &mut writer,
        BLUR_JPEG_QUALITY.clamp(1, 100) as u8,
    );
    blurred
        .write_with_encoder(encoder)
        .map_err(|e| trf("couldn't write %1: %2", &[&output, &e]))
}

/// File name of the blurred copy: `<name>_blur.jpg` next to the source.
pub fn blur_output_path(path: &str) -> String {
    let (stem, _) = split_ext(path);
    format!("{stem}_blur.jpg")
}

/// The maximum size the frame is shrunk to before blurring (the blur hides
/// the details anyway).
pub const BLUR_MAX_SIZE: (i64, i64) = (1920, 1080);
/// JPEG quality of the blurred frame.
pub const BLUR_JPEG_QUALITY: i64 = 85;

// ── faststart ──────────────────────────────────────────────────────────────

/// Extensions for which the position of the `moov` atom is checked.
pub const FASTSTART_EXTS: &[&str] = &[".mp4", ".mov", ".m4v"];

/// Whether `moov` comes BEFORE `mdat` at the start of the file.
///
/// Unknown/error → `true`: the file is left alone. Needed because
/// QtMultimedia gets stuck in `BufferingMedia` on large mp4 files with `moov`
/// at the end and decodes 0 frames — a black screen without sound.
pub fn moov_at_front(header: &[u8]) -> bool {
    let mut off = 0usize;
    for _ in 0..6 {
        if off + 8 > header.len() {
            break;
        }
        let size = u32::from_be_bytes([
            header[off],
            header[off + 1],
            header[off + 2],
            header[off + 3],
        ]) as usize;
        let atom = &header[off + 4..off + 8];
        if atom == b"moov" {
            return true;
        }
        if atom == b"mdat" {
            return false;
        }
        if size < 8 {
            break;
        }
        off += size;
    }
    true
}

/// How many header bytes to read for [`moov_at_front`].
pub const MOOV_HEADER_BYTES: usize = 64;

/// Faststart cache key: path + size + modification time.
pub fn faststart_key(abs_path: &str, size: u64, mtime: i64) -> String {
    let digest = sha1_hex(format!("{abs_path}:{size}:{mtime}").as_bytes());
    digest[..16].to_string()
}

/// A lossless remux: `moov` is moved to the start of the file (~0.3 s,
/// without re-encoding).
pub fn faststart_argv(path: &str, out: &str) -> Vec<String> {
    argv(&[
        "ffmpeg", "-v", "error", "-y", "-i", path, "-c", "copy", "-movflags",
        "+faststart", out,
    ])
}

/// Remux timeout, seconds.
pub const FASTSTART_TIMEOUT_SEC: u64 = 180;

// ── dependencies and validation ────────────────────────────────────────────

/// Binaries the engine reports on in `check_dependencies`.
/// `vlc` is `cvlc` OR `vlc`, `pillow` is counted by Python.
pub const DEP_TOOLS: &[&str] = &[
    "mpv", "mpvpaper", "xwinwrap", "swww", "hyprpaper", "surf", "feh",
    "fc-cache", "yt-dlp", "ffmpeg",
];

/// What is missing for the format. `deps` — the answer of `check_dependencies`.
pub fn missing_for_format(fmt: &str, session_type: &str, deps: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut missing = Vec::new();
    match fmt {
        "video" => {
            if session_type == "wayland" {
                if !deps("mpvpaper") {
                    missing.push("mpvpaper".to_string());
                }
            } else {
                if !deps("xwinwrap") {
                    missing.push("xwinwrap-git (AUR)".to_string());
                }
                if !deps("mpv") {
                    missing.push("mpv".to_string());
                }
            }
        }
        "youtube" => {
            if !deps("yt-dlp") {
                missing.push("yt-dlp".to_string());
            }
            if !deps("ffmpeg") {
                // TRANSLATORS: an item in a list of missing programs.
                missing.push(trf("%1 (recommended)", &[&"ffmpeg"]));
            }
            missing.extend(missing_for_format("video", session_type, deps));
        }
        "animated" => {
            if session_type == "wayland" {
                if !deps("swww") && !deps("mpvpaper") {
                    // TRANSLATORS: an item in a list of missing programs.
                    missing.push(trf("%1 (recommended) or %2", &[&"swww", &"mpvpaper"]));
                }
            } else {
                if !deps("xwinwrap") {
                    missing.push("xwinwrap-git (AUR)".to_string());
                }
                if !deps("mpv") {
                    missing.push("mpv".to_string());
                }
            }
        }
        // Web and Flash are shown by our host (`rexon-wallpaper-host --mode web|swf`,
        // the [`web_plan`] / [`swf_host_plan`] plan): they need no external programs
        // either on Wayland or on X11. surf with xwinwrap for web on X11 is only a
        // fallback path when there is no host, and the absence of the host itself is
        // reported on apply ([`WEB_HOST_MISSING`], [`VIDEO_HOST_MISSING_MESSAGE`]),
        // not as an "install these" list.
        "web" | "swf" => {}
        _ => {}
    }
    missing
}

/// Extensions the engine takes as video.
pub const VALID_VIDEO_EXTS: &[&str] = &[
    ".avi", ".bik", ".bk2", ".flv", ".gif", ".m4v", ".mkv", ".mov", ".mp4",
    ".mpeg", ".mpg", ".webm", ".wmv",
];

/// Checking a video file. `exists` and `size_bytes` are the shim's observations.
/// The wallpaper video file size limit, MB — the same number as in Python
/// (`max_file_size_mb`).
pub const MAX_VIDEO_FILE_MB: i64 = 1024;

pub fn validate_video(
    path: &str,
    exists: bool,
    size_bytes: u64,
    max_mb: i64,
) -> (bool, String) {
    if !exists {
        return (false, tr("File not found"));
    }
    let (_, ext) = split_ext(path);
    let ext = ext.to_lowercase();
    if !VALID_VIDEO_EXTS.contains(&ext.as_str()) {
        return (false, trf("Unsupported format: %1", &[&ext]));
    }
    let size_mb = size_bytes as f64 / (1024.0 * 1024.0);
    if size_mb > max_mb as f64 {
        return (
            false,
            // TRANSLATORS: %1 is the file size, %2 the limit, both in megabytes.
            trf("The file is too large: %1 MB (max. %2 MB)", &[&crate::i18n::format_decimal(size_mb, 1), &crate::i18n::format_decimal(max_mb as f64, 0)]),
        );
    }
    (true, "OK".to_string())
}

/// Application wallpapers (a foreign .exe via Wine/Proton) are no longer
/// supported: a window host of our own for them would be a second wallpaper
/// engine next to waywallen.
pub fn application_unsupported() -> String {
    tr("Application wallpapers are no longer supported")
}

/// Formats that MOVE: they are shown by a separate live process.
///
/// A static picture is not among them — it is simply written into the
/// environment's settings and leaves no process behind.
pub const LIVE_FORMATS: &[&str] = &["animated", "video", "youtube", "web", "swf"];

/// Whether this is a live wallpaper — the only place where the question is decided.
///
/// Both the engine (what to report outside via the bridge) and anything that
/// wants to tell "a live picture is running" from "a static one lies there" ask it.
pub fn is_live_format(fmt: &str) -> bool {
    LIVE_FORMATS.contains(&fmt)
}

/// How many seconds the engine stays silent about live wallpapers after its own notice.
///
/// ⚠️ Protection against bursts. In the showcase wallpapers are browsed by
/// eye, and EVERY click goes through the whole engine: the previous wallpaper
/// is stopped, the new one is set. Without the threshold a dozen clicks in a
/// dozen seconds would leave two dozen files in the bridge's command
/// directory — garbage nobody will read: the reading side has its own memory,
/// and after the very first reply it stays silent for minutes. So only the
/// FIRST notice of a burst makes sense, and the threshold cuts off the rest on
/// the leading edge — the notice goes out right away, the silence comes after.
///
/// Five seconds: meaningful browsing in the showcase is one or two clicks a
/// second, which is what the threshold covers; at the same time it is much
/// shorter than the reading side's own memory, so the threshold eats no
/// notice the other side could have responded to. The `video_wallpaper_active`
/// flag in the bridge state is not affected by the threshold at all — it is
/// always written and stays a fallback path for whoever missed the notices.
pub const BRIDGE_QUIET_SEC: f64 = 5.0;

/// Whether to tell the bridge about live wallpapers right now.
///
/// `told_*` — what the bridge has ALREADY been told by a notice (not what is
/// really going on with the wallpaper: the bridge did not see a replacement
/// or a suppressed burst as a notice). `told_at` — the moment of that notice
/// by the monotonic clock, `None` — never told yet.
///
/// We stay silent in two cases: there is nothing to say (the same state and
/// name) or [`BRIDGE_QUIET_SEC`] has not passed since the last notice.
pub fn bridge_should_announce(
    live: bool,
    name: &str,
    told_live: bool,
    told_name: &str,
    told_at: Option<f64>,
    now: f64,
) -> bool {
    if live == told_live && name == told_name {
        return false;
    }
    match told_at {
        Some(at) => now - at >= BRIDGE_QUIET_SEC,
        None => true,
    }
}

/// Wallpaper format name FOR THE CUSTOMER.
///
/// Internal plan labels (`video`, `swf`, …) are not shown in the window: these
/// are our internal words, not what the person chose. Technology names stay
/// in Latin script — `YouTube`, `GIF`, `Flash`, `SWF` are written like that
/// both in Rexon itself and in the original.
pub fn format_label(fmt: &str) -> String {
    match fmt {
        // TRANSLATORS: wallpaper format, lowercase: it's used inside sentences
        // (“Wallpaper stopped (format was: video)”) and capitalized by the program.
        "static" => trc("wallpaper format", "static image"),
        "animated" => trc("wallpaper format", "GIF animation"),
        "video" => trc("wallpaper format", "video"),
        "youtube" => trc("wallpaper format", "YouTube video"),
        "web" => trc("wallpaper format", "web page"),
        "swf" => "Flash (SWF)".to_string(),
        "scene" => trc("wallpaper format", "scene"),
        "application" => trc("wallpaper format", "application"),
        // A format the core does not know yet: showing it as is is more honest than
        // substituting someone else's name.
        other => other.to_string(),
    }
}

/// Wallpaper format name as a heading — capitalized: the label on a library
/// card («Static picture», «Video», «Web page»).
///
/// The name table is one — [`format_label`]; here only the capital letter, so
/// that no second dictionary of formats appears in the product. The logic
/// (whether to show a preview, what to apply with) still relies on the
/// internal value.
pub fn format_title(fmt: &str) -> String {
    let label = format_label(fmt);
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => label,
    }
}

/// The result of «Stop».
///
/// `fmt` — the format that WAS PLAYING before the stop; `None` — nothing was playing.
///
/// ⚠️ The format must arrive here taken BEFORE the wallpaper restore. The
/// restore sets a static picture with the same plan as a normal apply, and
/// that one writes its own format into the engine state — and the message
/// assembled after the restore said `static` even after video and YouTube
/// wallpapers. It also presented an empty state to the customer as «format
/// was: None».
pub fn stopped_message(fmt: Option<&str>) -> String {
    match fmt.map(str::trim).filter(|f| !f.is_empty()) {
        // TRANSLATORS: %1 is a wallpaper format in lowercase (“video”).
        Some(f) => trf("Wallpaper stopped (format was: %1)", &[&format_label(f)]),
        None => tr("Live wallpaper isn't running — the desktop is back to how it was"),
    }
}

/// Unknown format.
pub fn unknown_format_message(fmt: &str) -> String {
    trf("Unknown format: %1", &[&fmt])
}

/// File not found.
pub fn file_missing_message(path: &str) -> String {
    trf("File not found: %1", &[&path])
}

// ── GPU ────────────────────────────────────────────────────────────────────

/// What could be learned about the graphics card.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct Gpu {
    /// `nvidia` / `amd` / `intel` / `unknown`.
    pub vendor: String,
    /// Kernel driver: `nvidia` / `nouveau` / `amdgpu` / `i915` / `xe` / `?`.
    pub driver: String,
    /// NVIDIA with the proprietary driver on Wayland — a known risk.
    pub wayland_risk: bool,
    /// Warning text or empty.
    pub warning: String,
}

/// Parsing `lspci -k`: vendor and driver of the first graphics card.
pub fn parse_gpu(lspci: &str) -> Gpu {
    let mut vendor = "unknown".to_string();
    let mut driver = "?".to_string();
    let mut vga_found = false;
    for line in lspci.lines() {
        let low = line.to_lowercase();
        if low.contains("vga") || low.contains("3d controller") {
            vga_found = true;
            if low.contains("nvidia") {
                vendor = "nvidia".into();
            } else if low.contains("amd") || low.contains("ati") {
                vendor = "amd".into();
            } else if low.contains("intel") {
                vendor = "intel".into();
            }
        } else if vga_found && low.contains("kernel driver") {
            for name in ["nvidia", "nouveau", "amdgpu", "i915", "xe"] {
                if low.contains(name) {
                    driver = name.to_string();
                    break;
                }
            }
            vga_found = false;
        }
    }
    let wayland_risk = vendor == "nvidia" && driver == "nvidia";
    let warning = if wayland_risk {
        tr("NVIDIA with the proprietary driver on Wayland: hardware acceleration may not \
            work. Suggestions: use the VLC backend, or add __NV_PRIME_RENDER_OFFLOAD=1 \
            to the environment.")
    } else {
        String::new()
    };
    Gpu {
        vendor,
        driver,
        wayland_risk,
        warning,
    }
}

// ── desktop icons ──────────────────────────────────────────────────────────

/// Commands to show/hide desktop icons for the environment.
///
/// A live wallpaper lies over the desktop, and in some environments the icons
/// under it disappear for good — so they are hidden explicitly and explicitly
/// brought back.
pub fn desktop_icons_argvs(de: &str, visible: bool) -> Vec<Vec<String>> {
    let val = if visible { "true" } else { "false" };
    match de {
        // MATE: icons are drawn by Caja, and they should be turned off only if it
        // really owns the desktop (the shim does the check).
        "mate" => vec![argv(&[
            "gsettings",
            "set",
            "org.mate.background",
            "show-desktop-icons",
            val,
        ])],
        "gnome" | "budgie" => vec![
            // Ubuntu Desktop Icons NG
            argv(&[
                "gsettings",
                "set",
                "org.gnome.shell.extensions.ding",
                "show-desktop-icons",
                val,
            ]),
            // Vanilla GNOME: the Desktop Icons extension
            argv(&[
                "gsettings",
                "set",
                "org.gnome.desktop.background",
                "show-desktop-icons",
                val,
            ]),
        ],
        "cinnamon" | "cinnamon-mint" => vec![argv(&[
            "gsettings",
            "set",
            "org.nemo.desktop",
            "show-desktop-icons",
            val,
        ])],
        // style: 0 = none, 2 = files and launchers
        "xfce" => vec![argv(&[
            "xfconf-query",
            "-c",
            "xfce4-desktop",
            "-p",
            "/desktop-icons/style",
            "-s",
            if visible { "2" } else { "0" },
        ])],
        _ => Vec::new(),
    }
}

/// Whether MATE reads the desktop through Caja (otherwise there is nothing to hide).
pub fn caja_desktop_argv() -> Vec<String> {
    argv(&[
        "gsettings",
        "get",
        "org.mate.background",
        "show-desktop-icons",
    ])
}

/// What to ask the system BEFORE hiding the icons, or `None`.
///
/// Only MATE: there icons are drawn by Caja, and hiding them makes sense only
/// if the desktop really belongs to it. Bringing icons back needs no such
/// check — they must be brought back in any case. The answer counts as "yes"
/// if the command printed `true`.
pub fn desktop_icons_probe_argv(de: &str, visible: bool) -> Option<Vec<String>> {
    if de == "mate" && !visible {
        Some(caja_desktop_argv())
    } else {
        None
    }
}

// ── pause on a full-screen window and low battery ──────────────────────────

/// Which tool to detect a full-screen window with. Empty — nothing to detect with.
pub fn fullscreen_backend(de: &str, session_type: &str) -> &'static str {
    if session_type == "x11" {
        return "xdotool";
    }
    match de {
        "kde" => "kdotool",
        "hyprland" => "hyprctl",
        "gnome" | "budgie" => "gdbus",
        _ => "",
    }
}

/// Geometry of the active window on X11.
pub fn xdotool_geometry_argv() -> Vec<String> {
    argv(&[
        "xdotool",
        "getactivewindow",
        "getwindowgeometry",
        "--shell",
    ])
}

/// The active window on KDE Wayland (without kdotool there is nothing to detect with).
pub fn kdotool_active_argv() -> Vec<String> {
    argv(&["kdotool", "getactivewindow"])
}

/// The active Hyprland window.
pub fn hyprctl_active_argv() -> Vec<String> {
    argv(&["hyprctl", "activewindow", "-j"])
}

/// A window state query to GNOME Shell.
pub fn gdbus_fullscreen_argv() -> Vec<String> {
    argv(&[
        "gdbus",
        "call",
        "--session",
        "--dest",
        "org.gnome.Shell",
        "--object-path",
        "/org/gnome/Shell",
        "--method",
        "org.gnome.Shell.Eval",
        "global.display.get_focus_window() && \
         global.display.get_focus_window().is_fullscreen()",
    ])
}

/// X11 window types that do NOT obscure the wallpaper, however large they are.
///
/// ⚠️ 16.09.2026, measured on an X11 test bench with a window manager:
/// `xdotool getactivewindow` named OUR OWN wallpaper window as the active one
/// — it is full-screen, and the "a full-screen window is playing on top" rule
/// always fired. Flash wallpapers gave 4 frames instead of 240 over the same
/// run — they paused because of themselves. The desktop is the background,
/// not a window over the background; a panel and a dock are a strip at the
/// edge, all the more no reason to turn off the wallpaper.
pub const WALLPAPER_SAFE_WINDOW_TYPES: &[&str] = &[
    "_NET_WM_WINDOW_TYPE_DESKTOP",
    "_NET_WM_WINDOW_TYPE_DOCK",
];

/// `NET::WindowType` in the KWin answer: 1 — desktop, 2 — panel/dock.
/// The same idea as [`WALLPAPER_SAFE_WINDOW_TYPES`], in the language of Wayland KWin.
pub const KWIN_SAFE_WINDOW_TYPES: &[i64] = &[1, 2];

/// X11 window properties that show "this is the background, not a window over it".
///
/// We ask in ONE run: the window type and the owner. The type covers foreign
/// desktops and panels, the PID covers our own wallpaper windows (in case the
/// type could not be set).
pub fn xprop_window_info_argv(window_id: i64) -> Vec<String> {
    vec![
        "xprop".into(),
        "-id".into(),
        window_id.to_string(),
        "_NET_WM_WINDOW_TYPE".into(),
        "_NET_WM_PID".into(),
    ]
}

/// Identifier of the active window from the `xdotool … --shell` output (`WINDOW=`).
pub fn parse_xdotool_window_id(stdout: &str) -> Option<i64> {
    stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix("WINDOW="))
        .and_then(|v| v.trim().parse::<i64>().ok())
}

/// Is this window the desktop background (or our own wallpaper window)?
///
/// Input is the output of [`xprop_window_info_argv`]. An answer that was not
/// understood (no property, `xprop` was silent, garbage) gives `false`: the
/// window counts as an ordinary one, that is, the behavior stays UNCHANGED.
/// Answering "yes" here on something unclear would mean "never mute" — the
/// wallpaper would play over a game.
pub fn window_is_wallpaper_layer(props: &str, own_pids: &[i64]) -> bool {
    for line in props.lines() {
        let line = line.trim();
        let Some((name, value)) = line.split_once('=') else {
            // For a missing property `xprop` prints
            // «_NET_WM_PID:  not found.» — there is no equals sign there.
            continue;
        };
        let value = value.trim();
        if name.starts_with("_NET_WM_WINDOW_TYPE")
            && value
                .split(',')
                .any(|t| WALLPAPER_SAFE_WINDOW_TYPES.contains(&t.trim()))
        {
            return true;
        }
        if name.starts_with("_NET_WM_PID") {
            if let Ok(pid) = value.parse::<i64>() {
                if own_pids.contains(&pid) {
                    return true;
                }
            }
        }
    }
    false
}

/// Does the window occupy the whole screen? Parsing the `xdotool … --shell` output.
pub fn parse_xdotool_fullscreen(stdout: &str, screen: &str) -> bool {
    let Some((sw, sh)) = screen.split_once('x') else {
        return false;
    };
    let (Ok(sw), Ok(sh)) = (sw.parse::<i64>(), sh.parse::<i64>()) else {
        return false;
    };
    let mut w = 0i64;
    let mut h = 0i64;
    for line in stdout.lines() {
        if let Some(v) = line.strip_prefix("WIDTH=") {
            w = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("HEIGHT=") {
            h = v.trim().parse().unwrap_or(0);
        }
    }
    w >= sw && h >= sh
}

/// Where to knock for KWin window properties: bus name, object, method.
///
/// The former code asked `org.kde.kwin.Scripting.evaluateScript` — KWin 6 has
/// no such method at all, so pausing the wallpaper on a full-screen window
/// NEVER worked.
pub const KWIN_WINDOW_INFO: &[&str] =
    &["org.kde.KWin", "/KWin", "org.kde.KWin.getWindowInfo"];

/// Parsing GNOME Shell's answer to `Eval` — "is there a full-screen window".
pub fn parse_gdbus_fullscreen(stdout: &str) -> bool {
    stdout.to_lowercase().contains("true")
}

/// Parsing the window property dictionary from KWin (`org.kde.KWin.getWindowInfo`).
///
/// `dbus-send` unfolds the dictionary into a flat "key, value" list, `qdbus`
/// prints «fullscreen: false» — we understand both forms.
pub fn parse_kwin_fullscreen(info: &str) -> bool {
    let lines: Vec<&str> = info.lines().map(str::trim).collect();
    for (i, line) in lines.iter().enumerate() {
        if let Some(v) = line.strip_prefix("fullscreen:") {
            return v.trim() == "true";
        }
        if *line == "fullscreen" && i + 1 < lines.len() {
            return lines[i + 1] == "true";
        }
    }
    false
}

/// Is this KWin window the desktop background, a panel or our wallpaper window?
///
/// The same idea as [`window_is_wallpaper_layer`] on X11, only the properties
/// come from KWin itself (`getWindowInfo`). An answer not understood is
/// `false`, that is, the behavior is unchanged.
pub fn kwin_window_is_wallpaper_layer(info: &str, own_pids: &[i64]) -> bool {
    for line in info.lines() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "type" => {
                if let Ok(t) = value.parse::<i64>() {
                    if KWIN_SAFE_WINDOW_TYPES.contains(&t) {
                        return true;
                    }
                }
            }
            "pid" => {
                if let Ok(pid) = value.parse::<i64>() {
                    if own_pids.contains(&pid) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Is this Hyprland window our own wallpaper window?
///
/// The protocol has no "window type" of its own: layers (layer-shell), which
/// the wallpaper lies on, never get into `activewindow` — what remains is the
/// owner. An answer without `pid` is `false`, the behavior is unchanged.
pub fn hyprctl_window_is_wallpaper_layer(stdout: &str, own_pids: &[i64]) -> bool {
    serde_json::from_str::<serde_json::Value>(stdout)
        .ok()
        .and_then(|v| v.get("pid").and_then(serde_json::Value::as_i64))
        .map(|pid| own_pids.contains(&pid))
        .unwrap_or(false)
}

/// Parsing the `hyprctl activewindow -j` answer.
pub fn parse_hyprctl_fullscreen(stdout: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(stdout)
        .ok()
        .and_then(|v| v.get("fullscreen").cloned())
        .map(|v| v.as_i64().unwrap_or(0) != 0 || v.as_bool().unwrap_or(false))
        .unwrap_or(false)
}

/// Below which battery charge supervision pauses live wallpapers, % — the
/// same number as in Python (`battery_pause_pct`).
pub const BATTERY_PAUSE_PCT: i64 = 20;

/// Where the kernel exposes batteries.
pub const POWER_SUPPLY_DIR: &str = "/sys/class/power_supply";

/// Threshold and rule of the battery pause: charging — no pause.
pub fn battery_should_pause(percent: i64, status: &str, threshold: i64) -> bool {
    if status == "Charging" || status == "Full" {
        return false;
    }
    percent < threshold
}

/// CPU load of the wallpaper process.
pub fn ps_cpu_argv(pid: i64) -> Vec<String> {
    vec![
        "ps".into(),
        "-p".into(),
        pid.to_string(),
        "-o".into(),
        "%cpu".into(),
        "--no-headers".into(),
    ]
}

/// The CPU load threshold at which the wallpaper counts as leaking, %.
pub const CPU_HIGH_THRESHOLD: f64 = 80.0;
/// How many samples in a row above the threshold — a forced stop (×5 s).
pub const CPU_HIGH_STRIKES: i64 = 6;
/// Monitoring loop period, seconds.
pub const MONITOR_INTERVAL_SEC: i64 = 5;

// ── orphans ────────────────────────────────────────────────────────────────

/// Processes that may have survived the previous run. We kill only those
/// whose command line has a Rexon trace.
pub const ORPHAN_NAMES: &[&str] = &["mpvpaper", "xwinwrap", "cvlc"];

/// Whether this is our process — by the command line.
pub fn is_rexon_cmdline(cmdline: &str) -> bool {
    cmdline.to_lowercase().contains("rexon")
}

/// Finding processes by name.
pub fn pgrep_argv(name: &str) -> Vec<String> {
    argv(&["pgrep", "-f", name])
}

/// Name of the wallpaper process PID file.
pub const PIDFILE_NAME: &str = "rexon-wallpaper.pid";

// ── ambient sounds ─────────────────────────────────────────────────────────

/// A separate mpv for each background sound.
pub fn ambient_argv(path: &str, volume: i64, looped: bool) -> Vec<String> {
    let mut cmd = vec![
        "mpv".to_string(),
        "--no-video".into(),
        "--no-terminal".into(),
        format!("--volume={volume}"),
    ];
    if looped {
        cmd.push("--loop=inf".into());
    }
    cmd.push(path.to_string());
    cmd
}

// ── dependency install ─────────────────────────────────────────────────────

/// Package names for installing video dependencies by hand.
pub fn install_deps_packages(family: &str, session_type: &str) -> Vec<String> {
    let mut pkgs = vec!["mpv".to_string()];
    if family == "arch" && session_type == "wayland" {
        pkgs.push("mpvpaper".to_string());
    }
    pkgs
}

/// The command that installs video dependencies.
///
/// There is no table of its own here any more: both the flags and the choice
/// "the named org.rexon.pkg path or raw pkexec" come from the package engine
/// ([`crate::system_plan::install_argv`]) — the same one Rexon installs
/// desktops with. The former copy knew three families out of eleven and had
/// already drifted from the engine: `pacman -S --noconfirm` without
/// `--needed` (reinstalled an already installed mpv) and `apt install -y`
/// instead of `apt-get install -y --no-install-recommends` (an interface
/// unstable for scripts plus a tail of recommended packages). The other eight
/// families got «Unknown package manager» with a perfectly known manager.
///
/// `None` — the family is not recognized (`unknown` or a string outside the
/// table) or there is nothing to install; the caller tells the person what to
/// install by hand ([`install_deps_unknown_message`]).
pub fn install_deps_argv(
    family: &str,
    pkgs: &[String],
    pkg_helper_available: bool,
) -> Option<Vec<String>> {
    let fam = crate::system::PkgFamily::from_value(family)?;
    crate::system_plan::install_argv(fam, pkgs, pkg_helper_available)
}

/// The package manager is not recognized — we say what to install by hand.
pub fn install_deps_unknown_message(missing: &[String]) -> String {
    trf("Unknown package manager. Install manually: %1", &[&missing.join(", ")])
}

/// The package name from the "what is missing" string: strip the «(AUR)»,
/// «(recommended)» and «or …» marks.
///
/// ⚠️ The string is assembled by [`missing_for_format`] in the interface
/// language, so the word «or» is not searched for here: the package name is
/// the first word before the parenthesis (package names have no spaces).
pub fn dep_package_name(item: &str) -> String {
    item.split('(')
        .next()
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string()
}

// ── library of installed wallpapers ────────────────────────────────────────

/// File extensions the library takes from `data/` when the manifest has no
/// explicit wallpaper list.
pub fn library_data_exts() -> Vec<String> {
    let mut exts: Vec<String> = VALID_VIDEO_EXTS.iter().map(|s| s.to_string()).collect();
    for e in [".png", ".jpg", ".jpeg", ".webp", ".gif", ".apng", ".swf"] {
        if !exts.iter().any(|x| x == e) {
            exts.push(e.to_string());
        }
    }
    exts.sort();
    exts
}

/// One wallpaper from an installed wallpaper package — a library tile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibraryItem {
    /// Package identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Path to the wallpaper file.
    pub path: String,
    /// Wallpaper format (`static`, `video`, …).
    pub format: String,
    /// Path to the package's preview picture (the file may be missing).
    pub preview: String,
}

/// Wallpapers of one package from its `manifest.json` — port of the body of
/// `WallpaperApplier.list_installed_wallpapers`. Disk observations come from
/// outside: `exists` and `data_listing` (names in the `data/` directory).
///
/// ⭐ A fix: for a wallpaper with an empty `file` Python got the path of the
/// `data/` directory ITSELF (`os.path.join(data, "")`), it "existed" — and the
/// directory got into the library as a tile that cannot be applied.
pub fn library_items_from_manifest(
    pack_dir: &str,
    dir_name: &str,
    manifest: &serde_json::Value,
    exists: &dyn Fn(&str) -> bool,
    data_listing: &dyn Fn(&str) -> Vec<String>,
) -> Vec<LibraryItem> {
    use serde_json::Value;
    if manifest.get("type").and_then(Value::as_str) != Some("wallpaper-pack") {
        return Vec::new();
    }
    let text = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);
    let id = text(manifest.get("id")).unwrap_or_else(|| dir_name.to_string());
    let pack_name = text(manifest.get("name")).unwrap_or_else(|| dir_name.to_string());
    let pack_format =
        text(manifest.get("wallpaper_format")).unwrap_or_else(|| "static".to_string());
    let preview = format!("{pack_dir}/preview.png");
    let data_dir = format!("{pack_dir}/data");
    let listed = manifest
        .get("wallpapers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::new();
    for wp in &listed {
        let file = text(wp.get("file")).unwrap_or_default();
        if file.trim().is_empty() {
            continue;
        }
        // The path in the manifest is written in different ways: relative to
        // `data/` (our own packages), relative to the package root (a store item
        // puts the picture next to the manifest) and with `data/` in the path itself
        // (importing Windows themes). Previously only the first kind was checked,
        // and «Wallpaper library» stood empty with sets installed (acceptance 10.10.2026).
        let Some(path) = [format!("{data_dir}/{file}"), format!("{pack_dir}/{file}")]
            .into_iter()
            .find(|p| exists(p))
        else {
            continue;
        };
        let format = text(wp.get("format")).unwrap_or_else(|| pack_format.clone());
        // A set of several pictures: each tile has its own picture and its own
        // number. With the package's shared preview eight wallpapers looked like
        // eight identical tiles, and the right one could not be chosen.
        let many = listed.len() > 1;
        let own_picture = many && format == "static";
        out.push(LibraryItem {
            id: id.clone(),
            name: text(wp.get("name")).unwrap_or_else(|| {
                if many {
                    format!("{pack_name} — {}", out.len() + 1)
                } else {
                    pack_name.clone()
                }
            }),
            preview: if own_picture { path.clone() } else { preview.clone() },
            path,
            format,
        });
    }
    // No wallpaper list — take suitable files from `data/`.
    if listed.is_empty() {
        let exts = library_data_exts();
        let mut files = data_listing(&data_dir);
        files.sort();
        for file in files {
            let ext = file.rfind('.').map(|i| file[i..].to_lowercase()).unwrap_or_default();
            if !exts.iter().any(|e| *e == ext) {
                continue;
            }
            out.push(LibraryItem {
                id: id.clone(),
                name: file.clone(),
                path: format!("{data_dir}/{file}"),
                format: pack_format.clone(),
                preview: preview.clone(),
            });
        }
    }
    out
}

/// All installed wallpapers from the extensions directory — a live disk walk.
///
/// ⭐ A fix: the package order is by directory name. In Python it was file
/// system order, and library tiles swapped places from machine to machine. A
/// broken manifest is skipped (the other packages are shown).
pub fn list_installed_wallpapers(ext_dir: &str) -> Vec<LibraryItem> {
    let Ok(read) = std::fs::read_dir(ext_dir) else {
        return Vec::new();
    };
    let mut packs: Vec<(String, String)> = read
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| {
            (
                e.path().to_string_lossy().into_owned(),
                e.file_name().to_string_lossy().into_owned(),
            )
        })
        .collect();
    packs.sort_by(|a, b| a.1.cmp(&b.1));

    let exists = |p: &str| std::path::Path::new(p).exists();
    let listing = |d: &str| -> Vec<String> {
        std::fs::read_dir(d)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter(|e| e.path().is_file())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for (dir, name) in packs {
        let Ok(raw) = std::fs::read_to_string(format!("{dir}/manifest.json")) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        out.extend(library_items_from_manifest(&dir, &name, &manifest, &exists, &listing));
    }
    out
}

// ── small helpers ──────────────────────────────────────────────────────────

fn has(tools: &[String], name: &str) -> bool {
    tools.iter().any(|t| t == name)
}

fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Split a path into "before the extension" and "the extension with the dot"
/// — like `os.path.splitext`.
pub fn split_ext(path: &str) -> (&str, &str) {
    let name = base_name(path);
    match name.rfind('.') {
        // A dot at the start of the name is a hidden file, not an extension.
        Some(i) if i > 0 => {
            let cut = path.len() - name.len() + i;
            (&path[..cut], &path[cut..])
        }
        _ => (path, ""),
    }
}

/// Python's `str(float)` prints «3.0» for whole values, not «3».
fn format_float(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e16 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// SHA-1 in hex — the same cache key Python computed (`hashlib.sha1`).
fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// ⭐ The success text is for the customer: it contains no internal name of
    /// the path that set the wallpaper («generic», «KDE D-Bus», «swaybg», «PID»).
    /// The path goes to the journal (`Step::backend`). The check walks all
    /// environments and program sets, and a new step with a label in parentheses
    /// will turn it red.
    #[test]
    fn success_texts_carry_no_path_labels() {
        let des = [
            "kde", "gnome", "cinnamon", "cinnamon-mint", "mate", "budgie", "pantheon", "unity",
            "xfce", "lxqt", "sway",
            "niri", "river", "wayfire", "labwc", "hyprland", "generic",
        ];
        let tool_sets = [
            t(&[]),
            t(&["plasma-apply-wallpaperimage", "pcmanfm-qt", "feh"]),
            t(&["swww", "hyprpaper", "swaybg"]),
            t(&["wbg"]),
        ];
        let mut steps = Vec::new();
        for de in des {
            for session in ["wayland", "x11", ""] {
                for tools in &tool_sets {
                    steps.extend(static_plan(de, session, "/tmp/a.png", tools, "swww"));
                }
            }
        }
        steps.extend(swf_native_plan("/usr/bin/ruffle", "/tmp/a.swf"));
        let labels = [
            "generic", "KDE D-Bus", "KDE Plasma", "Xfce", "swww", "hyprpaper", "swaybg",
            "wbg", "feh", "pcmanfm", "mpvpaper", "xwinwrap", "QtWebEngine", "surf", "Ruffle",
            "PID", "(",
        ];
        let mut checked = 0;
        for step in steps.iter().filter(|s| !s.ok_message.is_empty()) {
            checked += 1;
            assert!(
                !step.ok_message.contains(&step.backend),
                "в тексте успеха имя пути «{}»: {}",
                step.backend,
                step.ok_message
            );
            for label in labels {
                assert!(
                    !step.ok_message.contains(label),
                    "в тексте успеха служебная метка «{label}»: {}",
                    step.ok_message
                );
            }
        }
        assert!(checked > 10, "сверка ничего не проверила ({checked})");
    }

    /// ⭐ Every wallpaper package format has a human label: the internal word
    /// (`static`, `video`…) is not shown on a library card.
    #[test]
    fn every_wallpaper_format_has_a_title() {
        for fmt in crate::constants::WALLPAPER_FORMATS {
            let title = format_title(fmt);
            assert_ne!(title, *fmt, "у формата «{fmt}» нет подписи");
            assert!(
                title.chars().next().is_some_and(char::is_uppercase),
                "подпись «{title}» не с заглавной буквы"
            );
        }
        assert_eq!(format_title("static"), "Static image");
        assert_eq!(format_title("video"), "Video");
        assert_eq!(format_title("web"), "Web page");
        assert_eq!(format_title("scene"), "Scene");
        // An unfamiliar format — as is, without a made-up name.
        assert_eq!(format_title("нечто"), "Нечто");
    }

    /// The blurred copy goes into the Rexon cache — and is recognized as NOT the
    /// real wallpaper, so that a stop does not put it back on the desktop.
    #[test]
    fn blur_copy_goes_to_the_cache() {
        let out = blur_cache_output_path("/home/u/.cache/", "/usr/share/backgrounds/закат.png");
        assert_eq!(out, "/home/u/.cache/rexon/blur/закат_blur.jpg");
        assert!(!clean_image_candidate_ok(&out, true), "размытая копия сошла за обои");
    }

    /// A real blur: a PNG above the limit is shrunk and written as JPEG.
    #[test]
    fn blur_writes_a_small_jpeg() {
        let dir = std::env::temp_dir().join(format!("rexon-blur-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.png");
        let big = image::RgbImage::from_fn(2400, 1200, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        big.save(&src).unwrap();
        let out = dir.join("кеш/out_blur.jpg");
        blur_image_file(src.to_str().unwrap(), out.to_str().unwrap(), 10).expect("размытие");
        let read = image::ImageReader::open(&out)
            .unwrap()
            .with_guessed_format()
            .unwrap();
        assert_eq!(read.format(), Some(image::ImageFormat::Jpeg));
        let img = read.decode().unwrap();
        assert!(img.width() <= BLUR_MAX_SIZE.0 as u32 && img.height() <= BLUR_MAX_SIZE.1 as u32);
        assert!(blur_image_file("/нет/такой.png", out.to_str().unwrap(), 10).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Library: a package with a wallpaper list, a package without one (files
    /// from `data/`), a foreign type and an empty file name.
    #[test]
    fn library_reads_wallpaper_packs() {
        let exists = |p: &str| !p.ends_with("/нет.mp4");
        let listing = |_: &str| t(&["b.png", "a.jpg", "readme.txt"]);
        let listed = serde_json::json!({
            "type": "wallpaper-pack", "id": "pack1", "name": "Пакет",
            "wallpaper_format": "video",
            "wallpapers": [
                {"file": "закат.mp4", "name": "Закат"},
                {"file": "нет.mp4"},
                {"file": ""},
                {"file": "кадр.png", "format": "static"}
            ]
        });
        let items = library_items_from_manifest("/x/pack1", "pack1", &listed, &exists, &listing);
        assert_eq!(items.len(), 2, "{items:?}");
        assert_eq!(items[0].name, "Закат");
        assert_eq!(items[0].format, "video");
        assert_eq!(items[0].path, "/x/pack1/data/закат.mp4");
        assert_eq!(items[0].preview, "/x/pack1/preview.png");
        // In a set of several wallpapers the fallback name carries a number, and the
        // picture has its own sample tile: otherwise the set's tiles are indistinguishable.
        assert_eq!(items[1].name, "Пакет — 2");
        assert_eq!(items[1].format, "static");
        assert_eq!(items[1].preview, items[1].path);

        // A single file in the set — the package name without a number. The path
        // from the manifest is found both from the package root (a store item) and
        // with `data/` in the path itself (importing Windows themes).
        let root_only = |p: &str| p == "/x/shop/картинка.jpg" || p == "/x/win/data/обои.jpg";
        let shop = serde_json::json!({
            "type": "wallpaper-pack", "name": "Магазин",
            "wallpapers": [{"file": "картинка.jpg", "format": "static"}]
        });
        let got = library_items_from_manifest("/x/shop", "shop", &shop, &root_only, &listing);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!((got[0].name.as_str(), got[0].path.as_str()), ("Магазин", "/x/shop/картинка.jpg"));
        assert_eq!(got[0].preview, "/x/shop/preview.png");
        let win = serde_json::json!({
            "type": "wallpaper-pack", "name": "Windows",
            "wallpapers": [{"file": "data/обои.jpg", "format": "static"}]
        });
        let got = library_items_from_manifest("/x/win", "win", &win, &root_only, &listing);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].path, "/x/win/data/обои.jpg");
        assert!(
            !items.iter().any(|i| i.path.ends_with("/data/")),
            "каталог data/ попал в библиотеку плиткой"
        );

        let bare = serde_json::json!({"type": "wallpaper-pack"});
        let files = library_items_from_manifest("/x/p2", "p2", &bare, &exists, &listing);
        let names: Vec<&str> = files.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["a.jpg", "b.png"], "файлы data/ не по порядку или лишние");
        assert!(files.iter().all(|i| i.id == "p2" && i.format == "static"));

        let other = serde_json::json!({"type": "theme", "wallpapers": [{"file": "a.png"}]});
        assert!(library_items_from_manifest("/x/p3", "p3", &other, &exists, &listing).is_empty());
    }

    /// Live walk: packages in name order, a broken manifest does not get in the
    /// way of the others. Everything is in the check's temporary directory.
    #[test]
    fn library_walk_is_sorted_and_survives_broken_manifests() {
        let root = std::env::temp_dir().join(format!("rexon-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (pack, manifest) in [
            ("b-pack", r#"{"type":"wallpaper-pack","wallpapers":[{"file":"b.png"}]}"#),
            ("a-pack", r#"{"type":"wallpaper-pack","wallpapers":[{"file":"a.png"}]}"#),
            ("broken", "{ это не json"),
        ] {
            let data = root.join(pack).join("data");
            std::fs::create_dir_all(&data).unwrap();
            std::fs::write(root.join(pack).join("manifest.json"), manifest).unwrap();
            std::fs::write(data.join(format!("{}.png", &pack[..1])), b"x").unwrap();
        }
        let items = list_installed_wallpapers(root.to_str().unwrap());
        let _ = std::fs::remove_dir_all(&root);
        let names: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
        assert_eq!(names, ["a-pack", "b-pack"]);
    }

    /// The KDE containment number is not guessed: wallpapers are read from all
    /// `org.kde.image` groups, whatever number the desktop has.
    #[test]
    fn kde_wallpaper_images_reads_every_containment() {
        let conf = "\
[Containments][216]
plugin=org.kde.plasma.folder

[Containments][216][Wallpaper][org.kde.image][General]
Image=/usr/share/wallpapers/Next/
PreviewImage=/usr/share/wallpapers/Next/

[Containments][216][Wallpaper][org.waywallen.kde][General]
Image=/tmp/live.mp4

[Containments][3][Wallpaper][org.kde.image][General]
Image=/home/u/.local/share/wallpapers/Пачка/contents/images/1920x1080.png
";
        assert_eq!(
            kde_wallpaper_images(conf),
            vec![
                "/usr/share/wallpapers/Next/".to_string(),
                "/home/u/.local/share/wallpapers/Пачка/contents/images/1920x1080.png"
                    .to_string(),
            ]
        );
        assert!(kde_wallpaper_images("").is_empty());
    }

    /// One plasmashell answer answers both questions about the desktop plugin.
    #[test]
    fn kde_plugins_answer_both_questions_from_one_read() {
        let reply = |v: &str| format!("method return time=1\n   string \"{v}\"\n");

        // Two monitors: the foreign plugin is found even if the stock one comes first.
        let two = reply("org.kde.image,org.waywallen.kde");
        assert_eq!(
            kde_desktop_plugins(&two),
            vec!["org.kde.image", "org.waywallen.kde"]
        );
        assert_eq!(
            kde_foreign_wallpaper_plugin(&two).as_deref(),
            Some("org.waywallen.kde")
        );
        assert!(!kde_removed_plugin_selected(&two));

        // A removed plugin on the SECOND desktop: the former read took only `d[0]`
        // and skipped such a desktop.
        let stale = reply("org.kde.image,org.rexon.wallpaper");
        assert!(kde_removed_plugin_selected(&stale));
        // ...and it must not be remembered: a stop would put the dead plugin back
        // and leave the desktop empty.
        assert_eq!(kde_foreign_wallpaper_plugin(&stale), None);

        // The stock plugin does not count as foreign: the stop returns to it.
        assert_eq!(kde_foreign_wallpaper_plugin(&reply("org.kde.image")), None);

        // plasmashell is silent — silence is not passed off as "no plugins".
        assert!(kde_desktop_plugins("").is_empty());
        assert!(!kde_removed_plugin_selected(""));
        assert_eq!(kde_foreign_wallpaper_plugin(""), None);
    }

    /// The stop message names the format that WAS PLAYING (the source language is
    /// English; the catalog checks the Russian one).
    #[test]
    fn stopped_message_names_the_format_that_played() {
        assert_eq!(
            stopped_message(Some("youtube")),
            "Wallpaper stopped (format was: YouTube video)"
        );
        assert_eq!(
            stopped_message(Some("swf")),
            "Wallpaper stopped (format was: Flash (SWF))"
        );
        // Nothing was playing — we say exactly that, not «format was: None».
        assert_eq!(
            stopped_message(None),
            "Live wallpaper isn't running — the desktop is back to how it was"
        );
        assert_eq!(stopped_message(Some("")), stopped_message(None));
        // A name unknown to the core is shown as is, not replaced by someone else's.
        assert_eq!(
            stopped_message(Some("шейдер")),
            "Wallpaper stopped (format was: шейдер)"
        );
    }

    /// The current XFCE wallpaper and the GNOME dark wallpaper — read for the
    /// Conky widget snapshot backdrop.
    #[test]
    fn xfce_and_dark_gnome_wallpaper_reading() {
        let list = "/backdrop/screen0/monitorHDMI-1/workspace1/last-image\n\
                    /backdrop/screen0/monitorHDMI-1/workspace0/last-image\n\
                    /backdrop/screen0/monitorHDMI-1/workspace0/image-style\n";
        assert_eq!(
            xfce_last_image_prop(list).as_deref(),
            Some("/backdrop/screen0/monitorHDMI-1/workspace0/last-image")
        );
        assert_eq!(xfce_last_image_prop("/backdrop/single-workspace-mode\n"), None);
        assert_eq!(gsettings_dark_key("gnome"), Some(("org.gnome.desktop.background", "picture-uri-dark")));
        assert_eq!(gsettings_dark_key("mate"), None);
        assert!(prefers_dark("'prefer-dark'\n"));
        assert!(!prefers_dark("'default'"));
        assert_eq!(gsettings_path_value("'file:///usr/share/a b.jpg'\n"), "/usr/share/a b.jpg");
    }

    /// The restore after stopping a live wallpaper takes the DESKTOP picture: the
    /// panel also holds an `org.kde.image` group, and it may come earlier in the file.
    #[test]
    fn kde_desktop_static_image_prefers_desktop_containment() {
        let conf = "\
[Containments][1]
plugin=org.kde.panel

[Containments][1][Wallpaper][org.kde.image][General]
Image=/usr/share/wallpapers/Next/

[Containments][216]
plugin=org.kde.plasma.folder
wallpaperplugin=org.waywallen.kde

[Containments][216][Wallpaper][org.kde.image][General]
Image=file:///home/u/.local/share/wallpapers/мой.png
";
        // The scheme is stripped: next the path goes to the file existence check.
        assert_eq!(
            kde_desktop_static_image(conf).as_deref(),
            Some("/home/u/.local/share/wallpapers/мой.png")
        );
        // No desktops found — any picture is better than the distribution default.
        assert_eq!(
            kde_desktop_static_image(
                "[Containments][9][Wallpaper][org.kde.image][General]\nImage=/w/один.png\n"
            )
            .as_deref(),
            Some("/w/один.png")
        );
        assert!(kde_desktop_static_image("").is_none());
    }

    #[test]
    fn de_detection_order_matters() {
        // budgie contains gnome, mate must come before gnome
        assert_eq!(detect_de("Budgie:GNOME", ""), "budgie");
        assert_eq!(detect_de("ubuntu:GNOME", ""), "gnome");
        assert_eq!(detect_de("", "mate"), "mate");
        assert_eq!(detect_de("KDE", ""), "kde");
        // It used to be «generic»: the engine's former table did not look at the
        // Plasma session name, and wallpapers on KDE without XDG_CURRENT_DESKTOP went
        // the common path.
        assert_eq!(detect_de("", "plasma"), "kde");
        assert_eq!(detect_de("sway", ""), "sway");
        assert_eq!(detect_de("i3", ""), "generic");
    }

    /// Pantheon: the picture is set by the native key that Gala reads — together
    /// with the picture for the dark style; and after a live wallpaper what was
    /// there comes back.
    #[test]
    fn pantheon_static_wallpaper_uses_gnome_background_keys() {
        let plan = static_plan("pantheon", "x11", "/tmp/a.png", &[], "swww");
        assert_eq!(plan.len(), 1);
        assert_eq!(
            plan[0].argv,
            t(&["gsettings", "set", "org.gnome.desktop.background", "picture-uri", "file:///tmp/a.png"])
        );
        assert_eq!(
            plan[0].post,
            vec![t(&[
                "gsettings", "set", "org.gnome.desktop.background", "picture-uri-dark",
                "file:///tmp/a.png",
            ])]
        );
        assert!(RESTORE_DES.contains(&"pantheon"));
        assert_eq!(restore_static_plan("pantheon", "/tmp/a.png", &[], "").len(), 1);
    }

    /// ⭐ The wallpaper engine recognizes the environment with THE SAME answer as
    /// the whole product (rule #12), and loses neither river/wayfire/labwc nor
    /// "don't know". Checked against live XDG_CURRENT_DESKTOP / DESKTOP_SESSION values.
    #[test]
    fn detect_de_is_the_core_answer() {
        let cases: &[(&str, &str, &str)] = &[
            ("KDE", "plasma", "kde"),
            ("", "plasmawayland", "kde"),
            ("GNOME", "gnome", "gnome"),
            ("GNOME-Flashback:GNOME", "gnome-flashback-metacity", "gnome"),
            ("", "gnome-xorg", "gnome"),
            ("X-Cinnamon", "cinnamon", "cinnamon-mint"),
            ("", "cinnamon2d", "cinnamon-mint"),
            ("XFCE", "xfce", "xfce"),
            ("MATE", "mate", "mate"),
            ("Budgie:GNOME", "budgie-desktop", "budgie"),
            ("Pantheon", "pantheon", "pantheon"),
            ("Pantheon", "pantheon-wayland", "pantheon"),
            ("", "pantheon-wayland", "pantheon"),
            ("LXQt", "lxqt", "lxqt"),
            ("pop:COSMIC", "", "cosmic"),
            ("Hyprland", "hyprland", "hyprland"),
            ("sway", "sway", "sway"),
            ("niri", "niri", "niri"),
            ("river", "river", "river"),
            ("", "river", "river"),
            ("wayfire", "wayfire", "wayfire"),
            ("labwc:wlroots", "labwc", "labwc"),
            ("", "labwc", "labwc"),
            ("LXDE", "LXDE", "generic"),
            ("i3", "i3", "generic"),
            ("", "", "generic"),
        ];
        for (xdg, session, want) in cases {
            assert_eq!(detect_de(xdg, session), *want, "{xdg:?} / {session:?}");
            let core = crate::system::detect_current_de(xdg, session, false);
            assert_eq!(core.as_deref().unwrap_or("generic"), *want, "ядро: {xdg:?}");
        }
        // Everything the engine treats as "an external daemon sets the background" is recognized.
        for de in WLROOTS_DE {
            assert_eq!(detect_de(de, ""), *de, "{de} не опознан");
        }
    }

    /// Cinnamon in the product is `cinnamon-mint`, in the engine's old data
    /// `cinnamon`. Every engine branch must understand both.
    #[test]
    fn both_cinnamon_ids_take_the_same_branches() {
        for (a, b) in [("cinnamon", "cinnamon-mint")] {
            assert_eq!(gsettings_schema(a), gsettings_schema(b));
            assert!(gsettings_schema(b).is_some());
            assert_eq!(RESTORE_DES.contains(&a), RESTORE_DES.contains(&b));
            assert_eq!(shell_ext_schema(a), shell_ext_schema(b));
            assert_eq!(shell_ext_de_label(b), "Cinnamon");
            assert!(needs_shell_extension(b, "wayland"));
            for visible in [true, false] {
                assert_eq!(desktop_icons_argvs(a, visible), desktop_icons_argvs(b, visible));
                assert!(!desktop_icons_argvs(b, visible).is_empty());
            }
            let t = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            assert_eq!(
                static_plan(a, "x11", "/w.png", &t(&["feh"]), ""),
                static_plan(b, "x11", "/w.png", &t(&["feh"]), ""),
            );
        }
    }

    #[test]
    fn session_type_falls_back_to_display_vars() {
        assert_eq!(detect_session_type("wayland", "", ""), "wayland");
        assert_eq!(detect_session_type("", "wayland-0", ""), "wayland");
        assert_eq!(detect_session_type("tty", "", ":0"), "x11");
        assert_eq!(detect_session_type("", "", ""), "unknown");
    }

    #[test]
    fn wlroots_prefers_swww_then_hyprpaper_then_holders() {
        let all = t(&["swww", "hyprpaper", "swaybg", "wbg"]);
        let plan = static_plan("sway", "wayland", "/w.png", &all, "");
        assert_eq!(plan[0].backend, "swww");
        assert_eq!(plan[1].backend, "hyprpaper");
        assert_eq!(plan[2].backend, "swaybg");
        // wbg only if there is no swaybg; the tail is always a refusal with a hint
        assert_eq!(plan.len(), 4);
        assert_eq!(plan[3].kind, step_kind::FAIL);
        let only_wbg = static_plan("sway", "wayland", "/w.png", &t(&["wbg"]), "");
        assert_eq!(only_wbg[0].backend, "wbg");
        assert_eq!(only_wbg[0].kind, step_kind::SPAWN_HOLD);
    }

    /// sway draws the background itself (`output * bg`, its own swaybg): Rexon
    /// asks it with a command rather than starting a second swaybg next to it.
    /// Otherwise «Appearance» on sway never set the wallpaper (the holding
    /// process is not available to it), and the wallpaper tab held two
    /// backgrounds on one layer. The path is in single quotes inside double ones:
    /// that way sway passes it to wordexp as one word with a space, an apostrophe
    /// and `$` (verified on sway 1.12 headless).
    #[test]
    fn sway_static_wallpaper_goes_through_swaymsg_first() {
        let tools = t(&["swaymsg", "swaybg"]);
        let plan = static_plan("sway", "wayland", "/w/it's a.png", &tools, "");
        assert_eq!(plan[0].kind, step_kind::RUN);
        assert_eq!(plan[0].backend, "sway");
        assert_eq!(plan[0].argv, argv(&["swaymsg", "output", "*", "bg", "\"'/w/it'\\''s a.png'\"", "fill"]));
        assert!(plan[0].drops_holder);
        assert_eq!(plan[1].backend, "swaybg");
        // sway cannot carry a double quote and a double space — straight to swaybg.
        for bad in ["/w/a\"b.png", "/w/a  b.png", "/w/a\\b.png", "/w/$mod.png"] {
            assert_eq!(static_plan("sway", "wayland", bad, &tools, "")[0].backend, "swaybg", "{bad}");
        }
        // Only sway: niri has no background command of its own.
        assert_eq!(static_plan("niri", "wayland", "/w.png", &tools, "")[0].backend, "swaybg");
        assert!(PLAN_TOOLS.contains(&"swaymsg"));
    }

    #[test]
    fn restore_touches_only_saved_des() {
        assert!(restore_static_plan("hyprland", "/w.png", &[], "").is_empty());
        assert!(restore_static_plan("lxqt", "/w.png", &[], "").is_empty());
        let mate = restore_static_plan("mate", "/w.png", &[], "");
        assert_eq!(mate[0].argv[2], "org.mate.background");
    }

    #[test]
    fn caja_probed_only_before_hiding() {
        assert!(desktop_icons_probe_argv("mate", false).is_some());
        assert!(desktop_icons_probe_argv("mate", true).is_none());
        assert!(desktop_icons_probe_argv("gnome", false).is_none());
    }

    #[test]
    fn empty_wlroots_names_the_package() {
        let plan = static_plan("niri", "wayland", "/w.png", &[], "пакет swww");
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].kind, step_kind::FAIL);
        assert!(plan[0].err_message.contains("пакет swww"));
    }

    #[test]
    fn kde_does_not_fall_through_after_plasma_apply() {
        let plan = static_plan(
            "kde",
            "wayland",
            "/w.png",
            &t(&["plasma-apply-wallpaperimage"]),
            "",
        );
        assert_eq!(plan.len(), 1);
        assert!(plan[0].terminal);
    }

    #[test]
    fn gnome_also_writes_dark_variant() {
        let plan = static_plan("gnome", "wayland", "/w.png", &[], "");
        assert_eq!(plan[0].argv[4], "file:///w.png");
        assert_eq!(plan[0].post[0][3], "picture-uri-dark");
        // MATE takes a plain path, not a URI
        let mate = static_plan("mate", "x11", "/w.png", &[], "");
        assert_eq!(mate[0].argv[4], "/w.png");
        // MATE has no dark variant — the tail has only the lock screen.
        assert_eq!(mate[0].post, vec![mate_lock_follows_desktop_argv()]);
    }

    #[test]
    fn unknown_wayland_goes_to_daemon_not_gsettings() {
        // previously it fell through to GNOME gsettings — the command "succeeded",
        // but the wallpaper did not change
        let plan = static_plan("generic", "wayland", "/w.png", &t(&["swww"]), "");
        assert_eq!(plan[0].backend, "swww");
    }

    #[test]
    fn ini_value_reads_qsettings_forms() {
        let t = "[General]\nWallpaper=/x.png\n[Desktop]\nWallpaperMode=fit\nWallpaper = \"/home/u/a b.png\"\n";
        assert_eq!(ini_value(t, "Desktop", "Wallpaper").as_deref(), Some("/home/u/a b.png"));
        assert_eq!(ini_value(t, "General", "Wallpaper").as_deref(), Some("/x.png"));
        assert_eq!(ini_value(t, "Desktop", "Nope"), None);
        let c = IniCheck { file: String::new(), section: "Desktop".into(), key: "Wallpaper".into() };
        assert!(ini_check_passes(Some(t), &c, "/home/u/a b.png"));
        assert!(!ini_check_passes(Some(t), &c, "/x.png"));
        assert!(!ini_check_passes(None, &c, "/x.png"));
    }

    #[test]
    fn lxqt_falls_back_when_pcmanfm_missing() {
        let plan = static_plan("lxqt", "x11", "/w.png", &t(&["feh"]), "");
        assert_eq!(plan[0].backend, "feh");
        let with_fm = static_plan("lxqt", "x11", "/w.png", &t(&["pcmanfm-qt", "feh"]), "");
        assert_eq!(with_fm[0].backend, "pcmanfm-qt");
        assert!(with_fm[0].ok_on_none);
        assert_eq!(
            with_fm[0].verify_ini.as_ref().map(|c| (c.file.as_str(), c.section.as_str(), c.key.as_str())),
            Some((PCMANFM_QT_LXQT_SETTINGS, "Desktop", "Wallpaper"))
        );
        assert_eq!(with_fm[1].backend, "feh");
    }

    #[test]
    fn video_plan_leaves_volume_to_settings() {
        // The volume is no longer in the command line on purpose: while it was
        // there, moving the slider did nothing until the wallpaper restarted.
        let host = "/opt/rexon/bin/rexon-wallpaper-host";
        let plan = video_native_plan(host, "/v.mp4", true);
        assert!(!plan[0].argv.iter().any(|a| a == "--volume"));
        assert!(!plan[0].argv.iter().any(|a| a == "--audio"));
        // But "this wallpaper is muted" is a property of the wallpaper, and it is passed.
        let mute = video_native_plan(host, "/v.mp4", false);
        assert_eq!(mute[0].argv.last().unwrap(), "--no-audio");
    }

    /// The command line holds the binary and only it: Python left it entirely
    /// together with the removed fallback path.
    #[test]
    fn video_command_is_the_host_binary_only() {
        let host = "/opt/rexon/bin/rexon-wallpaper-host";
        let plan = video_native_plan(host, "/v.mp4", true);
        assert_eq!(plan[0].argv, vec![host, "--mode", "video", "/v.mp4"]);
        let mute = video_native_plan(host, "/v.mp4", false);
        assert_eq!(mute[0].argv, vec![host, "--mode", "video", "/v.mp4", "--no-audio"]);
        // No plan has the right to call Python: there is no fallback host, and
        // nothing left to silently replace the binary with an interpreter.
        for step in plan.iter().chain(mute.iter()) {
            assert!(
                !step.argv.iter().any(|a| a.contains("python")),
                "{:?}",
                step.argv
            );
        }
    }

    /// A refusal instead of a quiet fallback: the text must name both the reason
    /// and what to do (rule #13).
    #[test]
    fn missing_host_message_names_cause_and_remedy() {
        assert!(video_host_missing_message().contains(WALLPAPER_HOST_BINARY));
        // The host is one — and the web refusal names the same binary, not the
        // removed `rexon-web-host`, which the customer no longer has.
        assert!(web_host_missing().contains(WALLPAPER_HOST_BINARY));
        assert!(!web_host_missing().contains("rexon-web-host"));
        assert!(video_host_missing_message().contains("reinstall Rexon"));
        assert!(video_host_missing_message().contains("cargo build"));
    }

    /// The shell extension gets THE SAME command line as our own window: there is
    /// no bare `python3` from `PATH` in it in any case.
    #[test]
    fn shell_ext_gets_same_host_command_as_own_window() {
        let host = "/opt/rexon/bin/rexon-wallpaper-host";
        for de in ["gnome", "cinnamon"] {
            let plan = shell_ext_plan(de, "/v.mp4", host, true)
                .expect("схема расширения известна");
            let native = video_native_plan(host, "/v.mp4", true);
            assert_eq!(
                plan.daemon_argv.last().unwrap(),
                &gvariant_string_array(&native[0].argv)
            );
            assert_eq!(plan.daemon_argv[3], SHELL_EXT_DAEMON_KEY);
            // There is no bare `python3` from `PATH` in the line in any case:
            // the extension's fallback path was removed together with the Python host.
            assert!(!plan.daemon_argv.last().unwrap().contains("python"));
            // The key did not get written — this is a refusal, not a trifle: without it
            // the extension has nothing to launch the daemon with.
            assert!(!plan.daemon_failed_message.is_empty());
        }
        let mute = shell_ext_plan("gnome", "/v.mp4", host, false)
            .expect("схема расширения известна");
        assert!(mute.daemon_argv.last().unwrap().contains("--no-audio"));
        // Environments with their own window do not need the extension.
        assert!(shell_ext_plan("kde", "/v.mp4", host, true).is_none());
    }

    /// Flash in our own window and via the shell extension is one and the same
    /// `--mode swf` host line; there is no external `ruffle` in it.
    #[test]
    fn swf_goes_through_our_host_in_both_ways() {
        let host = "/opt/rexon/bin/rexon-wallpaper-host";
        let native = swf_host_plan(host, "/обои/ролик.swf");
        assert_eq!(native.len(), 1);
        assert_eq!(native[0].kind, step_kind::SPAWN_SESSION);
        assert_eq!(native[0].argv, vec![host, "--mode", "swf", "/обои/ролик.swf"]);
        assert_eq!(native[0].format, "swf");
        assert!(native[0].ok_message.contains("ролик.swf"));
        assert!(!native[0].argv.iter().any(|a| a.contains("ruffle")));
        for de in ["gnome", "cinnamon"] {
            let plan = swf_shell_ext_plan(de, "/обои/ролик.swf", host).expect("схема известна");
            assert_eq!(plan.daemon_argv.last().unwrap(), &gvariant_string_array(&native[0].argv));
            assert_eq!(plan.engine_argv.last().unwrap(), "swf");
            assert!(plan.ok_message.contains("Flash"));
        }
        assert!(swf_shell_ext_plan("kde", "/r.swf", host).is_none());
        // Video through the shared builder has not changed.
        let video = shell_ext_plan("gnome", "/v.mp4", host, true).unwrap();
        assert_eq!(video.engine_argv.last().unwrap(), "video");
        assert!(video.ok_message.contains("Video wallpaper"));
    }

    /// An array write for `gsettings set` is valid GVariant text even when the
    /// path contains a quote or a backslash.
    #[test]
    fn gvariant_array_escapes_quotes_and_backslashes() {
        let items = vec![
            "/usr/bin/python3".to_string(),
            "/home/w/фильм \"тест\".mp4".to_string(),
            "back\\slash".to_string(),
        ];
        assert_eq!(
            gvariant_string_array(&items),
            "[\"/usr/bin/python3\",\"/home/w/фильм \\\"тест\\\".mp4\",\"back\\\\slash\"]"
        );
        assert_eq!(gvariant_string_array(&[]), "[]");
    }

    /// The list of "live" formats must cover exactly those that start a separate
    /// process — and must not count a static picture as live.
    #[test]
    fn live_formats_are_the_ones_that_spawn_a_process() {
        for fmt in ["video", "youtube", "animated", "web", "swf"] {
            assert!(is_live_format(fmt), "{fmt}");
        }
        assert!(!is_live_format("static"));
        assert!(!is_live_format(""));
        // Every live format must have a name for the customer. In English the name
        // «video» coincides with the internal word, so the check is against the name
        // table, not "name ≠ key".
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let names = [
            ("video", "video"),
            ("youtube", "YouTube video"),
            ("animated", "GIF animation"),
            ("web", "web page"),
            ("swf", "Flash (SWF)"),
        ];
        assert_eq!(LIVE_FORMATS.len(), names.len());
        for fmt in LIVE_FORMATS {
            let want = names.iter().find(|(k, _)| k == fmt).map(|(_, v)| *v);
            assert_eq!(Some(format_label(fmt).as_str()), want, "{fmt}");
        }
    }

    /// YouTube "stream" on Mutter/Muffin cannot be carried out: there is neither
    /// an own window nor wlr-layer-shell for mpvpaper — the mode becomes "download".
    #[test]
    fn youtube_stream_routes_to_download_on_shell_ext_sessions() {
        for de in ["gnome", "cinnamon"] {
            assert_eq!(youtube_effective_mode(de, "wayland", "stream"), "download");
            assert!(!youtube_mode_downgraded_note(de, "wayland", "stream").is_empty());
            // X11 on the same environments manages with a regular window — leave it alone.
            assert_eq!(youtube_effective_mode(de, "x11", "stream"), "stream");
            assert!(youtube_mode_downgraded_note(de, "x11", "stream").is_empty());
        }
        // The other environments stay as they were.
        assert_eq!(youtube_effective_mode("kde", "wayland", "stream"), "stream");
        assert_eq!(youtube_effective_mode("kde", "wayland", "download"), "download");
        // An explicitly chosen "download" needs no explanation.
        assert!(youtube_mode_downgraded_note("gnome", "wayland", "download").is_empty());
    }

    /// And a failed download message must not advise "stream" where there is no
    /// stream: the customer would switch the mode, get a download again and go
    /// round in circles.
    #[test]
    fn download_failure_never_advises_stream_where_stream_is_impossible() {
        for de in ["gnome", "cinnamon"] {
            let msg = youtube_download_failed_message(de, "wayland");
            assert!(!msg.contains("streaming mode"), "{de}: {msg}");
            // The shell name is a human one, not an identifier from a config.
            assert!(msg.contains(shell_ext_de_label(de)), "{de}: {msg}");
            assert!(!msg.contains(de), "{de}: сырой идентификатор в тексте");
            // X11 on the same environments can stream — the advice stays the same.
            assert_eq!(
                youtube_download_failed_message(de, "x11"),
                youtube_download_failed()
            );
        }
        assert_eq!(
            youtube_download_failed_message("kde", "wayland"),
            youtube_download_failed()
        );
    }

    /// And the plan itself refuses to launch mpvpaper where it has nothing to
    /// draw on — even if mpvpaper is installed.
    #[test]
    fn youtube_plan_refuses_mpvpaper_on_mutter() {
        let tools = t(&["mpvpaper", "mpv", "xwinwrap"]);
        for de in ["gnome", "cinnamon"] {
            let plan = youtube_plan(
                de, "wayland", "https://youtu.be/x", true, 50, "1080p", &tools,
                "{monitor}", "1920x1080", &Hints::default(),
            );
            assert_eq!(plan[0].kind, step_kind::FAIL, "{de}");
            assert!(plan[0].err_message.contains("download"), "{de}");
        }
        // KDE-Wayland — the former path via mpvpaper.
        let kde = youtube_plan(
            "kde", "wayland", "https://youtu.be/x", true, 50, "1080p", &tools,
            "{monitor}", "1920x1080", &Hints::default(),
        );
        assert_eq!(kde[0].backend, "mpvpaper");
    }

    /// The same class of defect for GIF wallpapers: swww and mpvpaper draw with
    /// their own window via wlr-layer-shell, which Mutter and Muffin do not have —
    /// and the plan did not ask about the environment at all and called them
    /// unconditionally. A customer on GNOME and Cinnamon-Wayland got «swww or
    /// mpvpaper is not installed» with swww and mpvpaper INSTALLED, or a launch
    /// without a picture.
    #[test]
    fn animated_plan_refuses_layer_shell_tools_on_mutter() {
        let tools = t(&["swww", "mpvpaper", "mpv", "xwinwrap"]);
        for de in ["gnome", "cinnamon"] {
            let plan = animated_plan(
                de, "wayland", "/w.gif", &tools, "{monitor}", "1920x1080",
                &Hints::default(),
            );
            assert_eq!(plan.len(), 1, "{de}");
            assert_eq!(plan[0].kind, step_kind::FAIL, "{de}");
            // Neither swww nor mpvpaper may be launched on these environments.
            assert!(!plan.iter().any(|s| s.backend == "swww"), "{de}");
            assert!(!plan.iter().any(|s| s.backend == "mpvpaper"), "{de}");
            // The shell name is a human one, not an identifier from a config.
            assert!(plan[0].err_message.contains(shell_ext_de_label(de)), "{de}");
            assert!(!plan[0].err_message.contains(de), "{de}: сырой идентификатор");
            // X11 on the same environments manages with its own window — the path is unchanged.
            let x11 = animated_plan(
                de, "x11", "/w.gif", &tools, "{monitor}", "1920x1080",
                &Hints::default(),
            );
            assert_eq!(x11[0].backend, "xwinwrap", "{de}");
        }
        // Wayland WITH wlr-layer-shell (KWin and wlroots) — as it was: swww
        // first, mpvpaper next.
        let kde = animated_plan(
            "kde", "wayland", "/w.gif", &tools, "{monitor}", "1920x1080",
            &Hints::default(),
        );
        assert_eq!(kde[0].backend, "swww");
        assert!(kde.iter().any(|s| s.backend == "mpvpaper"));
        // And without swww only mpvpaper remains.
        let only_mpv = animated_plan(
            "hyprland", "wayland", "/w.gif", &t(&["mpvpaper"]), "{monitor}",
            "1920x1080", &Hints::default(),
        );
        assert_eq!(only_mpv[0].backend, "mpvpaper");
    }

    /// ⚠️ Web wallpapers were not shown on ANY desktop since 30.07.2026: the
    /// `rexon/web_wallpaper.py` host was deleted, while the plan kept calling
    /// `python -m rexon.web_wallpaper`. The process died right away, while the
    /// customer was told «Web wallpaper started». This test guards that the
    /// command line never again points to a deleted module.
    #[test]
    fn web_plan_never_calls_the_deleted_python_module() {
        for session in ["wayland", "x11"] {
            let plan = web_plan(
                session, "/usr/bin/python3", "/w/index.html", "/w/index.html",
                &t(&["surf", "xwinwrap"]), "1920x1080", &Hints::default(), None,
            );
            for step in &plan {
                assert!(
                    !step.argv.iter().any(|a| a.contains("web_wallpaper")),
                    "{session}: план всё ещё зовёт удалённый модуль: {:?}",
                    step.argv
                );
            }
        }
    }

    /// The first step is our binary, not Python; a given path to a build wins
    /// over the `$PATH` lookup.
    #[test]
    fn web_plan_spawns_our_host_first() {
        let host = "/дерево/rust/target/release/rexon-wallpaper-host";
        let plan = web_plan(
            "wayland", "/usr/bin/python3", "https://пример.рф", "https://пример.рф",
            &[], "1920x1080", &Hints::default(), Some(host),
        );
        assert_eq!(plan[0].argv, vec![host, "--mode", "web", "https://пример.рф"]);
        // On Wayland our own host is the only path: xwinwrap does not work there.
        assert_eq!(plan.len(), 1);
        assert!(plan[0].terminal);
        // The wallpaper outlives the command, so the step is a session one: the
        // wallpaper engine waits for a plain SPAWN, and the launch would look
        // forever hanging.
        assert_eq!(plan[0].kind, step_kind::SPAWN_SESSION);

        // ⚠️ The binary is nowhere — a refusal BEFORE launching, not launching nothing.
        // `None` gets here after the core has already searched both the build tree
        // and `$PATH`; spawning in such a situation would give the person a flash
        // of an empty desktop and «code 127» instead of an answer.
        let missing = web_plan(
            "wayland", "/usr/bin/python3", "/w/index.html", "/w/index.html",
            &[], "1920x1080", &Hints::default(), None,
        );
        assert_eq!(missing[0].kind, step_kind::FAIL);
        assert!(missing[0].err_message.contains(WALLPAPER_HOST_BINARY));
        assert!(missing[0].argv.is_empty(), "отказ ничего не запускает");
    }

    /// On X11 the fallback path via surf remains — and accepts both a link and a
    /// local file.
    #[test]
    fn web_plan_keeps_surf_fallback_on_x11() {
        // No host — but on X11 the queue must get to surf: the first step's
        // refusal is NOT terminal here.
        let plan = web_plan(
            "x11", "/usr/bin/python3", "/w/index.html", "/w/index.html",
            &t(&["surf", "xwinwrap"]), "1920x1080", &Hints::default(), None,
        );
        assert_eq!(plan.len(), 2);
        assert!(!plan[0].terminal, "отказ не должен обрывать план на X11");
        assert_eq!(plan[1].backend, "surf");
        assert!(plan[1].argv.contains(&"file:///w/index.html".to_string()));

        let link = web_plan(
            "x11", "/usr/bin/python3", "https://a", "https://a",
            &t(&["surf", "xwinwrap"]), "1920x1080", &Hints::default(), None,
        );
        assert!(link[1].argv.contains(&"https://a".to_string()));
    }

    /// xwinwrap window geometry: the executor expands the placeholder itself (one
    /// placeholder per output), while a bare size, as before, gets a zero offset —
    /// that is how the `rexon-py` bridge calls the plan, and its command is the
    /// same as it was.
    #[test]
    fn xwinwrap_takes_the_whole_geometry_or_the_old_zero_offset() {
        let tools = t(&["xwinwrap", "mpv"]);
        let by_mark = animated_plan(
            "generic", "x11", "/w.gif", &tools, "{monitor}", GEOMETRY_MARK,
            &Hints::default(),
        );
        assert_eq!(by_mark[0].backend, "xwinwrap");
        assert!(
            by_mark[0].argv.contains(&GEOMETRY_MARK.to_string()),
            "{:?}", by_mark[0].argv
        );
        let by_size = animated_plan(
            "generic", "x11", "/w.gif", &tools, "{monitor}", "{resolution}",
            &Hints::default(),
        );
        assert!(
            by_size[0].argv.contains(&"{resolution}+0+0".to_string()),
            "{:?}", by_size[0].argv
        );
        // Web via surf goes through the same xwinwrap — and takes the geometry the same way.
        let web = web_plan(
            "x11", "", "/w/index.html", "/w/index.html",
            &t(&["surf", "xwinwrap"]), GEOMETRY_MARK, &Hints::default(), None,
        );
        assert!(
            web[1].argv.contains(&GEOMETRY_MARK.to_string()),
            "{:?}", web[1].argv
        );
    }

    /// The wallpaper host is one: video and web differ only in the mode value in
    /// the command line, the binary and the path position are the same.
    #[test]
    fn one_host_argv_for_every_mode() {
        let host = "/opt/rexon/bin/rexon-wallpaper-host";
        assert_eq!(
            wallpaper_host_argv(host, HostMode::Video, "/v.mp4"),
            vec![host, "--mode", "video", "/v.mp4"]
        );
        assert_eq!(
            wallpaper_host_argv(host, HostMode::Web, "https://a"),
            vec![host, "--mode", "web", "https://a"]
        );
        // The video line for our own window and the web line are built by one function.
        let video = video_native_plan(host, "/v.mp4", true);
        let web = web_plan(
            "wayland", "", "/w/index.html", "/w/index.html", &[], "", &Hints::default(),
            Some(host),
        );
        assert_eq!(video[0].argv[..2], web[0].argv[..2]);
        assert_eq!(video[0].argv[2], "video");
        assert_eq!(web[0].argv[2], "web");
        // The switch value parses back and forth; an unfamiliar one is None.
        for mode in HostMode::ALL {
            assert_eq!(HostMode::from_arg(mode.as_arg()), Some(mode));
        }
        assert_eq!(HostMode::from_arg("swf"), Some(HostMode::Swf));
        assert_eq!(
            wallpaper_host_argv(host, HostMode::Swf, "/r.swf"),
            vec![host, "--mode", "swf", "/r.swf"]
        );
        assert_eq!(HostMode::from_arg("flash"), None);
        assert_eq!(HostMode::from_arg(""), None);
        // Without the switch — video: that is how the former contract called the
        // host, and the shell extension already keeps such a line in the customer's
        // gsettings.
        assert_eq!(HostMode::DEFAULT, HostMode::Video);
        // Mode exit codes do not overlap with the shared ones.
        let codes = [
            host_exit::OK,
            host_exit::NOTHING_TO_SHOW,
            host_exit::QML_FAILED,
            host_exit::FRAMES_FAILED,
            host_exit::VIDEO_NO_AUDIO,
            host_exit::WEB_PAGE_FAILED,
            host_exit::SWF_NO_GPU,
            host_exit::SWF_MOVIE_FAILED,
        ];
        let mut uniq = codes.to_vec();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), codes.len(), "коды возврата хоста пересеклись");
        // The root's trailing slash is not doubled.
        assert!(!host_candidates("/дерево/", WALLPAPER_HOST_BINARY)[0].contains("//rust"));
    }

    /// Binary lookup order: the tree (release → debug) → `$PATH`.
    /// Nothing found — `None`, and that is a refusal, not a switch to another host.
    #[test]
    fn wallpaper_host_lookup_order() {
        struct Fs {
            exists: Vec<String>,
            asked: Vec<String>,
            path_hit: bool,
        }
        impl DaemonFs for Fs {
            fn is_exec_file(&mut self, path: &str) -> bool {
                self.asked.push(path.to_string());
                self.exists.iter().any(|e| e == path)
            }
            fn which(&mut self, name: &str) -> Option<String> {
                self.asked.push(format!("which:{name}"));
                self.path_hit.then(|| format!("/usr/bin/{name}"))
            }
        }
        let release = format!("/дерево/rust/target/release/{WALLPAPER_HOST_BINARY}");
        let debug = format!("/дерево/rust/target/debug/{WALLPAPER_HOST_BINARY}");

        let mut both = Fs {
            exists: vec![release.clone(), debug.clone()],
            asked: Vec::new(),
            path_hit: true,
        };
        assert_eq!(
            find_wallpaper_host_binary(&mut both, "/дерево"),
            Some(release)
        );

        let mut only_debug = Fs {
            exists: vec![debug.clone()],
            asked: Vec::new(),
            path_hit: true,
        };
        assert_eq!(
            find_wallpaper_host_binary(&mut only_debug, "/дерево"),
            Some(debug)
        );

        let mut installed = Fs {
            exists: Vec::new(),
            asked: Vec::new(),
            path_hit: true,
        };
        assert_eq!(
            find_wallpaper_host_binary(&mut installed, "/дерево"),
            Some(format!("/usr/bin/{WALLPAPER_HOST_BINARY}"))
        );

        let mut nothing = Fs {
            exists: Vec::new(),
            asked: Vec::new(),
            path_hit: false,
        };
        assert_eq!(find_wallpaper_host_binary(&mut nothing, "/дерево"), None);
        assert_eq!(nothing.asked.len(), 3);
    }

    #[test]
    fn youtube_quality_maps_to_ytdl_format() {
        assert!(youtube_format("4k").contains("height<=2160"));
        assert!(youtube_format("неизвестно").contains("height<=1080"));
        assert_eq!(
            youtube_format("best"),
            "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best"
        );
    }

    #[test]
    fn moov_position_decides_remux() {
        let mut front = Vec::new();
        front.extend_from_slice(&16u32.to_be_bytes());
        front.extend_from_slice(b"ftyp");
        front.extend_from_slice(&[0u8; 8]);
        front.extend_from_slice(&32u32.to_be_bytes());
        front.extend_from_slice(b"moov");
        assert!(moov_at_front(&front));

        let mut tail = Vec::new();
        tail.extend_from_slice(&16u32.to_be_bytes());
        tail.extend_from_slice(b"ftyp");
        tail.extend_from_slice(&[0u8; 8]);
        tail.extend_from_slice(&32u32.to_be_bytes());
        tail.extend_from_slice(b"mdat");
        assert!(!moov_at_front(&tail));

        // garbage — leave the file alone
        assert!(moov_at_front(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn sha1_matches_hashlib() {
        // hashlib.sha1(b"abc").hexdigest()
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha1_hex(b""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(faststart_key("/a/b.mp4", 10, 5).len(), 16);
    }

    #[test]
    fn video_validation_reports_reason() {
        assert_eq!(validate_video("/v.mp4", false, 0, 1024).1, "File not found");
        assert_eq!(
            validate_video("/v.png", true, 10, 1024).1,
            "Unsupported format: .png"
        );
        let (ok, msg) = validate_video("/v.mp4", true, 2 * 1024 * 1024 * 1024, 1024);
        assert!(!ok && msg.starts_with("The file is too large: 2048.0 MB"));
        assert!(validate_video("/v.mp4", true, 1024, 1024).0);
    }

    #[test]
    fn containments_pick_desktops_only() {
        let text = "[Containments][1]\nplugin=org.kde.panel\n\
                    [Containments][128]\nplugin=org.kde.desktopcontainment\n\
                    [Containments][77]\nplugin=org.kde.plasma.folder\n";
        assert_eq!(desktop_containment_ids(text), vec!["128", "77"]);
        assert_eq!(desktop_containment_ids("[General]\nx=1\n"), vec!["128"]);
    }

    /// English wallpaper messages contain no Russian letters; the package name is
    /// taken from the "what is missing" string the same way as in Russian.
    #[test]
    fn english_wallpaper_texts_have_no_russian() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let cyrillic = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
        let mut texts = vec![
            spawn_died_message(Some(1)),
            spawn_died_message(None),
            wallpaper_set_message(),
            video_host_missing_message(),
            youtube_mode_downgraded_note("gnome", "wayland", "stream"),
            youtube_no_ytdlp_message("Install the “yt-dlp” package"),
            youtube_unsupported_message("https://example.com"),
            youtube_download_failed(),
            youtube_download_failed_message("gnome", "wayland"),
            web_host_missing(),
            web_missing_message("/x/index.html"),
            scene_unsupported(),
            swf_missing_message("/x/a.swf"),
            application_unsupported(),
            stopped_message(Some("video")),
            stopped_message(None),
            unknown_format_message("xyz"),
            file_missing_message("/x/a.mp4"),
            install_deps_unknown_message(&["mpv".to_string()]),
            validate_video("/x/a.mp4", false, 0, 500).1,
            parse_gpu("01:00.0 VGA compatible controller: NVIDIA Corporation\n\tKernel driver in use: nvidia\n").warning,
        ];
        for fmt in ["static", "animated", "video", "youtube", "web", "swf", "scene", "application"] {
            texts.push(format_label(fmt));
            texts.push(format_title(fmt));
            for session in ["wayland", "x11"] {
                for item in missing_for_format(fmt, session, &|_| false) {
                    assert!(!dep_package_name(&item).contains(' '), "{fmt}: {item}");
                    texts.push(item);
                }
            }
        }
        for text in &texts {
            assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
        }
        assert_eq!(format_title("static"), "Static image");
        assert_eq!(dep_package_name("swww (recommended) or mpvpaper"), "swww");
    }

    #[test]
    fn gpu_warns_only_on_proprietary_nvidia() {
        let g = parse_gpu(
            "01:00.0 VGA compatible controller: NVIDIA Corporation\n\
             \tKernel driver in use: nvidia\n",
        );
        assert_eq!((g.vendor.as_str(), g.driver.as_str()), ("nvidia", "nvidia"));
        assert!(g.wayland_risk && !g.warning.is_empty());

        let n = parse_gpu(
            "01:00.0 VGA compatible controller: NVIDIA Corporation\n\
             \tKernel driver in use: nouveau\n",
        );
        assert!(!n.wayland_risk && n.warning.is_empty());
        assert_eq!(parse_gpu("").vendor, "unknown");
    }

    #[test]
    fn missing_deps_are_per_session() {
        let none = |_: &str| false;
        assert_eq!(missing_for_format("video", "wayland", &none), vec!["mpvpaper"]);
        assert_eq!(
            missing_for_format("video", "x11", &none),
            vec!["xwinwrap-git (AUR)", "mpv"]
        );
        // youtube pulls in the video dependencies
        let yt = missing_for_format("youtube", "wayland", &none);
        assert_eq!(yt.last().unwrap(), "mpvpaper");
        assert!(missing_for_format("static", "x11", &none).is_empty());
        // Web and Flash are drawn by our host: nothing to install in any session,
        // and the former «gtk4-layer-shell + webkit2gtk» hint is gone.
        for fmt in ["web", "swf"] {
            for session in ["wayland", "x11", ""] {
                assert!(missing_for_format(fmt, session, &none).is_empty(), "{fmt}/{session}");
            }
        }
    }

    #[test]
    fn scene_no_longer_pretends_to_start() {
        // There is no scene renderer of our own and there will not be; the text must
        // point to the tab, not talk about a vanished module.
        assert!(scene_unsupported().contains("no longer supported"));
        assert!(scene_unsupported().contains("Wallpaper Engine"));
        assert!(!scene_unsupported().contains("parallax"));
    }

    #[test]
    fn splitext_matches_python() {
        assert_eq!(split_ext("/a/b.mp4"), ("/a/b", ".mp4"));
        assert_eq!(split_ext("/a/b"), ("/a/b", ""));
        assert_eq!(split_ext("/a/.hidden"), ("/a/.hidden", ""));
        assert_eq!(split_ext("/a.b/c"), ("/a.b/c", ""));
    }

    #[test]
    fn kde_reset_script_writes_image_before_switching_plugin() {
        let s = kde_reset_script("/usr/share/wallpapers/Next/");
        let img = s.find("writeConfig").unwrap();
        let plug = s.find("wallpaperPlugin").unwrap();
        assert!(img < plug, "картинка обязана записаться до смены плагина");
        assert!(!kde_reset_script("").contains("writeConfig"));
    }

    #[test]
    fn desktop_icons_cover_every_de_that_has_them() {
        assert_eq!(desktop_icons_argvs("gnome", false).len(), 2);
        assert_eq!(desktop_icons_argvs("xfce", true)[0].last().unwrap(), "2");
        assert_eq!(desktop_icons_argvs("xfce", false)[0].last().unwrap(), "0");
        assert!(desktop_icons_argvs("kde", false).is_empty());
        assert_eq!(desktop_icons_argvs("cinnamon-mint", false).len(), 1);
    }

    /// The liveness check grace period must cover the slowest observed failure
    /// (0.2 s) and not grow: EVERY wallpaper format and every background restore
    /// pays for it.
    #[test]
    fn spawn_alive_grace_covers_slow_death_and_stays_short() {
        assert!(SPAWN_ALIVE_GRACE_SEC >= 0.2, "медленный отказ пройдёт как успех");
        assert!(SPAWN_ALIVE_GRACE_SEC <= 1.0, "применение обоев затормозится");
    }

    /// The instant death text names the exit code and does not pretend to be the
    /// process output.
    #[test]
    fn spawn_died_message_names_exit_code() {
        assert!(spawn_died_message(Some(1)).contains("code 1"));
        assert!(!spawn_died_message(None).contains("code"));
    }

    /// The first notice goes out right away, a repeat of the same — never.
    #[test]
    fn bridge_announces_first_change_and_never_repeats_itself() {
        // Never told yet — tell.
        assert!(bridge_should_announce(true, "Ночной город", false, "", None, 0.0));
        // The same as already told — stay silent, however much time has passed.
        assert!(!bridge_should_announce(
            true,
            "Ночной город",
            true,
            "Ночной город",
            Some(0.0),
            1_000.0
        ));
        // A stop after a launch is news, and the threshold passed long ago.
        assert!(bridge_should_announce(false, "", true, "Ночной город", Some(0.0), 1_000.0));
    }

    /// A burst of clicks in the showcase goes through as one notice: the threshold
    /// counts time from OUR previous notice, not from the wallpaper change.
    #[test]
    fn bridge_quiet_period_swallows_a_burst() {
        let told_at = Some(100.0);
        // Another scene right after our notice — stay silent.
        assert!(!bridge_should_announce(true, "Лес", true, "Ночной город", told_at, 100.5));
        // Even a stop within the threshold — stay silent (the flag in the bridge
        // state stays correct, and the reading side recognizes the stop by it).
        assert!(!bridge_should_announce(false, "", true, "Ночной город", told_at, 102.0));
        // Exactly at the threshold — we already tell.
        assert!(bridge_should_announce(
            true,
            "Лес",
            true,
            "Ночной город",
            told_at,
            100.0 + BRIDGE_QUIET_SEC
        ));
    }

    /// The threshold cuts off a burst, but is much shorter than the reading
    /// side's memory (three minutes) — otherwise it would eat notices the other
    /// side could have responded to.
    #[test]
    fn bridge_quiet_period_covers_a_burst_and_stays_short() {
        assert!(BRIDGE_QUIET_SEC >= 2.0, "перебор в витрине пройдёт пачкой");
        assert!(BRIDGE_QUIET_SEC <= 30.0, "порог съест осмысленную весть");
    }

    #[test]
    fn window_id_and_property_argv_from_one_place() {
        let geometry = "WINDOW=10485771\nX=0\nY=0\nWIDTH=1280\nHEIGHT=800\nSCREEN=0\n";
        assert_eq!(parse_xdotool_window_id(geometry), Some(10485771));
        assert_eq!(parse_xdotool_window_id("WIDTH=1\n"), None);
        assert_eq!(parse_xdotool_window_id("WINDOW=не число\n"), None);
        let a = xprop_window_info_argv(10485771);
        assert_eq!(a[0], "xprop");
        assert!(a.contains(&"_NET_WM_WINDOW_TYPE".to_string()));
        assert!(a.contains(&"_NET_WM_PID".to_string()));
        // We ask in ONE run, not two.
        assert_eq!(a.iter().filter(|p| p.as_str() == "-id").count(), 1);
    }

    #[test]
    fn own_window_filter_works_on_wayland_too() {
        // KWin: type 1 — desktop, 2 — panel/dock, plus our own process.
        let kwin = "type: 1\npid: 5\n";
        assert!(kwin_window_is_wallpaper_layer(kwin, &[]));
        assert!(kwin_window_is_wallpaper_layer("type: 2\n", &[]));
        assert!(kwin_window_is_wallpaper_layer("pid: 5\n", &[5]));
        assert!(!kwin_window_is_wallpaper_layer("type: 0\npid: 5\n", &[9]));
        // An answer not understood — the behavior is unchanged.
        assert!(!kwin_window_is_wallpaper_layer("", &[5]));
        assert!(!kwin_window_is_wallpaper_layer("тарабарщина", &[5]));

        // Hyprland: no window type in the answer, the owner remains.
        let hypr = r#"{"pid": 5, "fullscreen": 2}"#;
        assert!(hyprctl_window_is_wallpaper_layer(hypr, &[5]));
        assert!(!hyprctl_window_is_wallpaper_layer(hypr, &[9]));
        assert!(!hyprctl_window_is_wallpaper_layer("{}", &[5]));
        assert!(!hyprctl_window_is_wallpaper_layer("не json", &[5]));
    }

    /// COSMIC: a static picture goes into cosmic-bg's native key, not with our
    /// own daemon on top of it; without cosmic-bg — the former common path.
    #[test]
    fn cosmic_static_wallpaper_goes_to_cosmic_bg() {
        let t = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let plan = static_plan("cosmic", "wayland", "/w.png", &t(&["cosmic-bg", "swaybg"]), "");
        assert_eq!(plan[0].kind, step_kind::COSMIC_BG);
        assert!(!plan[0].terminal, "неудача записи обязана уводить на запасной путь");
        assert_eq!(plan[1].backend, "swaybg");
        let plain = static_plan("cosmic", "wayland", "/w.png", &t(&["swaybg"]), "");
        assert!(plain.iter().all(|s| s.kind != step_kind::COSMIC_BG));
        assert!(PLAN_TOOLS.contains(&"cosmic-bg"));
        // The restore after a live wallpaper — through the same key.
        assert!(RESTORE_DES.contains(&"cosmic"));
        assert_eq!(
            restore_static_plan("cosmic", "/w.png", &t(&["cosmic-bg"]), "")[0].kind,
            step_kind::COSMIC_BG
        );
        // Other environments do not get this step.
        for de in ["sway", "hyprland", "gnome", "kde"] {
            let p = static_plan(de, "wayland", "/w.png", &t(&["cosmic-bg", "swaybg"]), "");
            assert!(p.iter().all(|s| s.kind != step_kind::COSMIC_BG), "{de}");
        }
    }

    /// The `Entry` record — in `ron::ser::to_string_pretty` form, with the
    /// previous record's foreign fields; an unfamiliar value is the cosmic-bg default.
    #[test]
    fn cosmic_bg_entry_keeps_foreign_fields() {
        let shipped = "(\n    output: \"all\",\n    source: Path(\"/usr/share/backgrounds/cosmic/orion_nebula_nasa_heic0601a.jpg\"),\n    filter_by_theme: #true,\n    rotation_frequency: 3600,\n    filter_method: Lanczos,\n    scaling_mode: Zoom,\n    sampling_method: Alphanumeric,\n)\n";
        let fresh = cosmic_bg_entry(None, "/home/u/Обои/a \"b\".png").unwrap();
        assert_eq!(
            fresh,
            "(\n    output: \"all\",\n    source: Path(\"/home/u/Обои/a \\\"b\\\".png\"),\n    filter_by_theme: false,\n    rotation_frequency: 900,\n    filter_method: Lanczos,\n    scaling_mode: Zoom,\n    sampling_method: Alphanumeric,\n)"
        );
        assert_eq!(cosmic_bg_source_path(&fresh).as_deref(), Some("/home/u/Обои/a \"b\".png"));
        let kept = cosmic_bg_entry(Some(shipped), "/w.png").unwrap();
        assert!(kept.contains("filter_by_theme: true,") && kept.contains("rotation_frequency: 3600,"), "{kept}");
        let fit = shipped.replace("scaling_mode: Zoom", "scaling_mode: Fit((0.0, 0.0, 0.0))").replace("Lanczos", "Nearest");
        let kept = cosmic_bg_entry(Some(&fit), "/w.png").unwrap();
        assert!(kept.contains("scaling_mode: Fit((0.0, 0.0, 0.0)),") && kept.contains("filter_method: Nearest,"), "{kept}");
        let odd = shipped.replace("scaling_mode: Zoom", "scaling_mode: Fit((").replace("Alphanumeric", "Shuffle");
        let kept = cosmic_bg_entry(Some(&odd), "/w.png").unwrap();
        assert!(kept.contains("scaling_mode: Zoom,") && kept.contains("sampling_method: Alphanumeric,"), "{kept}");
        // A control character in the path — do not write at all.
        assert!(cosmic_bg_entry(None, "/a\nb.png").is_none());
        // A color instead of a picture — no path.
        assert_eq!(cosmic_bg_source_path("(\n    source: Color(Single((0.1, 0.2, 0.3))),\n)"), None);
    }

    /// Execution: the `all` key and `same-on-all = true` in `$XDG_CONFIG_HOME`;
    /// an already set `true` is not rewritten; did not read back — not a success.
    #[test]
    fn apply_cosmic_bg_writes_both_keys() {
        use std::cell::RefCell;
        use std::collections::HashMap;
        let fs: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
        let writes = RefCell::new(Vec::new());
        let ok = apply_cosmic_bg(
            "/cfg/",
            "/w.png",
            &mut |p| fs.borrow().get(p).cloned(),
            &mut |p, t| {
                writes.borrow_mut().push(p.to_string());
                fs.borrow_mut().insert(p.to_string(), t.to_string());
                true
            },
        );
        assert!(ok);
        let dir = "/cfg/cosmic/com.system76.CosmicBackground/v1";
        assert_eq!(*writes.borrow(), vec![format!("{dir}/all"), format!("{dir}/same-on-all")]);
        assert_eq!(fs.borrow()[&format!("{dir}/same-on-all")], "true");
        writes.borrow_mut().clear();
        assert!(apply_cosmic_bg(
            "/cfg",
            "/v.png",
            &mut |p| fs.borrow().get(p).cloned(),
            &mut |p, t| {
                writes.borrow_mut().push(p.to_string());
                fs.borrow_mut().insert(p.to_string(), t.to_string());
                true
            },
        ));
        assert_eq!(*writes.borrow(), vec![format!("{dir}/all")]);
        // The write "succeeded", but the disk holds something else — an honest refusal.
        assert!(!apply_cosmic_bg("/cfg", "/x.png", &mut |p| fs.borrow().get(p).cloned(), &mut |_, _| true));
    }

    /// Parsing the extension state: the gnome-shell 45–51 `ExtensionState` numbers.
    #[test]
    fn shell_ext_state_is_parsed_from_gdbus_and_gsettings() {
        use ShellExtState::*;
        let gnome = |out: &str| parse_shell_ext_state("gnome", Some(out));
        let info = |state: &str| {
            format!("({{'uuid': <'{SHELL_EXT_ID}'>, 'version': <1.0>, 'state': <{state}>}},)\n")
        };
        assert_eq!(gnome(&info("1.0")), Enabled);
        assert_eq!(gnome(&info("2.0")), Disabled);
        assert_eq!(gnome(&info("3.0")), Error);
        assert_eq!(gnome(&info("4.0")), OutOfDate);
        assert_eq!(gnome(&info("6.0")), Disabled);
        assert_eq!(gnome(&info("99.0")), NotLoaded);
        assert_eq!(gnome(&info("42.0")), Unknown);
        assert_eq!(gnome("(@a{sv} {},)\n"), NotLoaded);
        assert_eq!(gnome(""), Unknown);
        assert_eq!(parse_shell_ext_state("gnome", None), Unknown);

        let cinnamon = |out: &str| parse_shell_ext_state("cinnamon", Some(out));
        assert_eq!(cinnamon(&format!("['a@b', '{SHELL_EXT_ID}']\n")), Enabled);
        assert_eq!(cinnamon("@as []\n"), Disabled);
        assert_eq!(cinnamon(""), Unknown);

        assert!(shell_ext_state_message(Enabled, "x").is_none());
        assert_eq!(shell_ext_state_message(Disabled, "x"), Some((false, "x".to_string())), "выключено — обоев нет, не удача");
        assert!(!shell_ext_state_message(Error, "x").unwrap().0);
        assert!(!shell_ext_state_message(OutOfDate, "x").unwrap().0);
    }

    /// Cinnamon with layer-shell: a live wallpaper goes with our own layer window,
    /// the extension is not needed; GNOME and Cinnamon without layer-shell — as
    /// before; the idle screensaver still needs the extension path
    /// (`mutter_family_wayland`).
    #[test]
    fn cinnamon_with_layer_shell_needs_no_extension() {
        assert!(!needs_shell_extension_with("cinnamon", "wayland", true));
        assert!(needs_shell_extension_with("cinnamon", "wayland", false));
        assert!(needs_shell_extension_with("gnome", "wayland", true), "у Mutter layer-shell нет");
        assert!(!needs_shell_extension_with("cinnamon", "x11", false));
        assert!(mutter_family_wayland("cinnamon", "wayland"));
    }

    /// MATE: after the wallpaper the lock screen is switched to "like the desktop
    /// wallpaper", but only from the stock picture; the person's own picture stays.
    #[test]
    fn mate_lock_follows_desktop_only_from_stock_picture() {
        let plan = static_plan("mate", "x11", "/home/u/a.png", &[], "");
        assert_eq!(plan[0].argv, argv(&["gsettings", "set", "org.mate.background", "picture-filename", "/home/u/a.png"]));
        assert_eq!(plan[0].post, vec![mate_lock_follows_desktop_argv()]);
        let sh = mate_lock_follows_desktop_argv();
        assert_eq!(&sh[..2], ["sh", "-c"]);
        assert!(sh[2].contains("*'/backgrounds/mate/desktop/Stripes.png'\\')"), "{}", sh[2]);
        assert!(sh[2].contains("gsettings set org.mate.screensaver picture-filename ''"), "{}", sh[2]);
        // The gsettings neighbors have no tail.
        assert!(static_plan("gnome", "wayland", "/a.png", &[], "")[0].post.iter().all(|p| p[0] == "gsettings"));

        assert!(mate_lock_picture_is_stock("'/usr/share/backgrounds/mate/desktop/Stripes.png'\n"));
        assert!(!mate_lock_picture_is_stock("''\n"));
        assert!(!mate_lock_picture_is_stock("'/home/u/Мой замок.png'\n"));
        assert_eq!(mate_lock_as_desktop_set_argv(), argv(&["gsettings", "set", "org.mate.screensaver", "picture-filename", ""]));
    }

    /// Cinnamon static picture: the path goes as arguments (not as script text),
    /// the script itself chooses `picture-uri-list` (6.8+) or `picture-uri` by
    /// the schema.
    #[test]
    fn cinnamon_static_picture_goes_to_list_or_legacy_key() {
        let argv = cinnamon_static_argv("/home/u/Мои обои/it's $HOME.png");
        assert_eq!(&argv[..2], ["sh", "-c"]);
        assert_eq!(argv[4], "'file:///home/u/Мои обои/it\\'s $HOME.png'");
        assert_eq!(argv[5], "file:///home/u/Мои обои/it's $HOME.png");
        assert!(argv[2].contains("grep -qx picture-uri-list"));
        assert!(argv[2].contains("picture-uri-list \"[{'picture-uri': <$1>"));
        assert!(argv[2].contains("background-mode mirror"));
        assert!(argv[2].contains("picture-uri \"$2\""));
        assert!(!argv[2].contains("/home/u"), "путь не вклеен в сценарий");
    }

    #[test]
    fn cinnamon_list_first_picture_is_read() {
        let list = "[{'picture-uri': <'file:///a/b c.png'>, 'picture-options': <'zoom'>}]\n";
        assert_eq!(cinnamon_list_first_path(list).as_deref(), Some("/a/b c.png"));
        assert_eq!(cinnamon_list_first_path("[{'picture-uri': <'file:///x/it\\'s.png'>}]").as_deref(), Some("/x/it's.png"));
        assert_eq!(cinnamon_list_first_path("@aa{sv} []"), None);
        assert_eq!(cinnamon_list_first_path("[{'picture-uri': <''>}]"), None);
        assert_eq!(gvariant_quote("a'b\\c"), "'a\\'b\\\\c'");
    }

    /// Enabling our own extension: on Cinnamon foreign uuids stay, ours is
    /// appended at the end; an already enabled one is left alone.
    #[test]
    fn shell_ext_enable_keeps_foreign_cinnamon_extensions() {
        let cmd = shell_ext_enable_argv("cinnamon", Some("['a@b', 'c@d']\n")).expect("выключено — включаем");
        assert_eq!(&cmd[..4], ["gsettings", "set", "org.cinnamon", "enabled-extensions"]);
        assert_eq!(cmd[4], format!("[\"a@b\",\"c@d\",\"{SHELL_EXT_ID}\"]"));
        let empty = shell_ext_enable_argv("cinnamon", Some("@as []\n")).unwrap();
        assert_eq!(empty[4], format!("[\"{SHELL_EXT_ID}\"]"));
        assert!(shell_ext_enable_argv("cinnamon", Some(&format!("['{SHELL_EXT_ID}']"))).is_none());
        assert_eq!(shell_ext_enable_argv("gnome", None).unwrap(), ["gnome-extensions", "enable", SHELL_EXT_ID]);
        assert!(shell_ext_enable_argv("xfce", None).is_none());
    }

    /// Extension schema directories: our own copy before the system ones,
    /// `--schemadir` goes right after `gsettings`.
    #[test]
    fn shell_ext_schema_dir_goes_right_after_gsettings() {
        let dirs = shell_ext_schema_dirs("gnome", "/home/u", "", "");
        assert_eq!(
            dirs,
            vec![
                format!("/home/u/.local/share/gnome-shell/extensions/{SHELL_EXT_ID}/schemas"),
                format!("/usr/local/share/gnome-shell/extensions/{SHELL_EXT_ID}/schemas"),
                format!("/usr/share/gnome-shell/extensions/{SHELL_EXT_ID}/schemas"),
            ]
        );
        let dirs = shell_ext_schema_dirs("cinnamon", "/home/u", "/x/data/", "/opt/share");
        assert_eq!(
            dirs,
            vec![
                format!("/x/data/cinnamon/extensions/{SHELL_EXT_ID}/schemas"),
                format!("/opt/share/cinnamon/extensions/{SHELL_EXT_ID}/schemas"),
            ]
        );
        assert!(shell_ext_schema_dirs("kde", "/home/u", "", "").is_empty());

        let plan = shell_ext_plan("gnome", "/v.mp4", "/h", true).unwrap();
        assert_eq!(
            with_schemadir(&plan.source_argv, "/d"),
            argv(&[
                "gsettings",
                "--schemadir",
                "/d",
                "set",
                "org.gnome.shell.extensions.rexon-wallpaper",
                "source",
                "/v.mp4",
            ])
        );
        let gdbus = shell_ext_state_argv("gnome");
        assert_eq!(with_schemadir(&gdbus, "/d"), gdbus);
    }
}
