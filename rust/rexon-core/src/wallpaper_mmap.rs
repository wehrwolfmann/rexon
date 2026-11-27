//! Pure logic of wallpaper mmap streaming and the X11 fallback — port of
//! `rexon/wallpaper_mmap.py` (the module itself was removed from the tree on
//! 2026-08-23 together with the Python wallpaper host; here is all the
//! surviving logic).
//!
//! The original serves two render paths:
//!   * **mmap** (GNOME/Cinnamon extension): with `REXON_WP_OUTPUT` set,
//!     engines write RGBA8888 frames into a shared file
//!     `/run/user/$UID/rexon-wp-frame.rgba` (`width × height × 4` bytes), the
//!     extension reads it via `GLib.MappedFile`;
//!   * **X11 fallback** (XFCE/MATE/Budgie/Cinnamon-X11): the window is pinned
//!     to the desktop via EWMH — the `_NET_WM_WINDOW_TYPE_DESKTOP` type and
//!     state before showing, requests to the root window after
//!     ([`x11_desktop_plan`]).
//!
//! Here is ONLY the deterministic part (input → output): parsing the ENV
//! screen size, output path and frame rate, computing the frame layout
//! ([`prime_plan`]: file size, row count, a black opaque row, permissions),
//! Wayland session detection, the X11 fallback plan ([`x11_desktop_plan`]:
//! properties before showing, EWMH requests after, the ClientMessage
//! layout), the headless tick interval. Side effects — files, mmap, flock,
//! X server requests, Qt — belong to the wallpaper host
//! (`rexon-gui::wallpaper_host`), which executes what the core decided
//! (paths, sizes, bytes, atoms). There are no layout numbers in the host.
//!
//! Environment observations (the `REXON_WP_*` values, `XDG_SESSION_TYPE`,
//! `WAYLAND_DISPLAY`, the runtime directory) come in as PARAMETERS.
//!
//! Parity with the live Python — the golden `tests/wallpaper_mmap_parity.rs`
//! (the reference was captured read-only: xprop was NOT run — subprocess was
//! replaced by a stub capturing argv; Qt did not start — PySide6 was replaced
//! by stubs; prime files were written only into a temporary directory).
//!
//! A preserved quirk of the original (not a bug): in `env_screen_size` a
//! parse error of EITHER of the two variables resets BOTH to the 1920×1080
//! default (a shared `try/except ValueError` around both `int()` calls) — an
//! atomic fallback.

// ── constants ──────────────────────────────────────────────────────────────

/// Default frame width (`REXON_WP_WIDTH` not set/not parsed).
pub const DEFAULT_WIDTH: i64 = 1920;
/// Default frame height (`REXON_WP_HEIGHT` not set/not parsed).
pub const DEFAULT_HEIGHT: i64 = 1080;
/// Bytes per RGBA8888 pixel.
pub const BYTES_PER_PIXEL: u64 = 4;
/// Name of the frame mmap file in the runtime directory (the same as
/// `FRAME_PATH` in the GNOME/Cinnamon extension.js).
pub const FRAME_FILE_NAME: &str = "rexon-wp-frame.rgba";
/// Permissions of the mmap file — `os.open(..., 0o600)` in `prime_output_file`
/// and `MmapFrameSink.__init__` (per-user tmpfs, nothing for others to read).
pub const FRAME_FILE_MODE: u32 = 0o600;
/// Qt application name of the headless loop (`app.setApplicationName`).
///
/// The source of truth is the entry point table [`crate::qt_app::ENTRY_IDENTITIES`]
/// (`rexon.wallpaper_mmap`); here is a duplicate for those who do not need the
/// whole Qt module. `tests/wallpaper_mmap_parity.rs` checks that they match.
pub const HEADLESS_APP_NAME: &str = "RexonWallpaperHeadless";
/// Default mmap stream FPS (`run_headless_loop(..., fps=30)`,
/// `stream_window_to_mmap(..., fps=30)`, the `_env_fps` fallback).
pub const DEFAULT_FPS: i64 = 30;
/// The lowest meaningful mmap stream frame rate (`1 <= fps` in `_env_fps`).
pub const MIN_FPS: i64 = 1;
/// The highest accepted frame rate (`fps <= 120`): above that is almost
/// certainly a typo in the environment variable, not a wish.
pub const MAX_FPS: i64 = 120;

// ── paths and frame layout ─────────────────────────────────────────────────

/// The canonical path of the frame mmap file: `<runtime_dir>/rexon-wp-frame.rgba`.
///
/// `runtime_dir` is an observation (`/run/user/$UID`, for extensions
/// `GLib.get_user_runtime_dir()`); the Python side put the result into
/// `REXON_WP_OUTPUT` for the engines.
pub fn default_frame_path(runtime_dir: &str) -> String {
    format!("{runtime_dir}/{FRAME_FILE_NAME}")
}

/// Screen size from ENV values — port of `env_screen_size`.
///
/// `width`/`height` — raw `REXON_WP_WIDTH`/`REXON_WP_HEIGHT` values (`None` =
/// the variable is not set → the default as a string). Parsing has Python
/// `int()` semantics: whitespace trimming, a sign; fractions/garbage →
/// ValueError. A parse error of EITHER variable → both become 1920×1080 (a
/// quirk of the original, see the module doc). Then `max(1, …)` on each.
pub fn env_screen_size(width: Option<&str>, height: Option<&str>) -> (i64, i64) {
    let w_raw = width.unwrap_or("1920");
    let h_raw = height.unwrap_or("1080");
    let (w, h) = match (py_int(w_raw), py_int(h_raw)) {
        (Some(w), Some(h)) => (w, h),
        _ => (DEFAULT_WIDTH, DEFAULT_HEIGHT),
    };
    (w.max(1), h.max(1))
}

/// The mmap file path from ENV — port of `env_output_path`:
/// `os.environ.get("REXON_WP_OUTPUT") or None` — not set or an empty string
/// → `None` (the regular QML mode).
pub fn env_output_path(raw: Option<&str>) -> Option<String> {
    match raw {
        Some(s) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

/// Size of the frame mmap file in bytes — `max(1, width) * max(1, height) * 4`
/// (as in `prime_output_file`; `MmapFrameSink` gets already clamped sizes from
/// [`env_screen_size`]).
pub fn frame_size(width: i64, height: i64) -> u64 {
    width.max(1) as u64 * height.max(1) as u64 * BYTES_PER_PIXEL
}

/// One frame row in OPAQUE BLACK for priming — port of
/// `row = b"\x00\x00\x00\xff" * max(1, width)` from `prime_output_file`.
///
/// RGBA8888: R=0 G=0 B=0 A=255. NOT zeros — zero alpha would give a
/// transparent frame and a "flash" at the start of scene/web wallpapers. The
/// Python shim wrote this row [`PrimePlan::rows`] times under `flock(LOCK_EX)`.
pub fn black_row(width: i64) -> Vec<u8> {
    let mut row = Vec::with_capacity(width.max(1) as usize * 4);
    for _ in 0..width.max(1) {
        row.extend_from_slice(&[0x00, 0x00, 0x00, 0xff]);
    }
    row
}

/// What exactly the shim writes into the mmap file when priming — the whole
/// `prime_output_file` computation at once, so that `max(1, height)` does not
/// remain a number in Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimePlan {
    /// Frame width after clamping (`max(1, width)`) — the number of pixels in the
    /// [`PrimePlan::row`] row.
    pub width: i64,
    /// Frame height after clamping (`max(1, height)`), which is also
    /// [`PrimePlan::rows`].
    pub height: i64,
    /// The resulting file size (`os.ftruncate`), bytes.
    pub size: u64,
    /// How many times to write [`PrimePlan::row`] (port of `range(max(1, height))`).
    pub rows: u64,
    /// One frame row — opaque black RGBA8888.
    pub row: Vec<u8>,
    /// File permissions (`os.open(..., mode)`).
    pub mode: u32,
}

/// The frame mmap file priming plan — port of the computational part of
/// `prime_output_file`. Side effects (makedirs/open/ftruncate/flock/
/// write/fsync) stay with the shim, the numbers come from here.
pub fn prime_plan(width: i64, height: i64) -> PrimePlan {
    PrimePlan {
        width: width.max(1),
        height: height.max(1),
        size: frame_size(width, height),
        rows: height.max(1) as u64,
        row: black_row(width),
        mode: FRAME_FILE_MODE,
    }
}

// ── Wayland session detection ──────────────────────────────────────────────

/// Whether the session is Wayland — port of `is_wayland_session` (the choice
/// between LayerShellQt QML and regular QML with the xprop trick).
///
/// `xdg_session_type`/`wayland_display` — values of the ENV variables of the
/// same names (not set → an empty string, as `os.environ.get(..., "")`).
/// True if `XDG_SESSION_TYPE` == "wayland" (case-insensitive) OR
/// `WAYLAND_DISPLAY` is non-empty.
///
/// There is NO implementation of its own here (rule #12): the Wayland session
/// sign is one for all of Rexon and lives in
/// [`crate::wallpaper_backend::is_wayland_session`] — two copies of the same
/// sign had already drifted apart (there surrounding whitespace is trimmed,
/// here it previously was not), and it decides whether wallpapers are drawn
/// as a window or frames are handed to the shell extension.
pub fn is_wayland_session(xdg_session_type: &str, wayland_display: &str) -> bool {
    crate::wallpaper_backend::is_wayland_session(xdg_session_type, wayland_display)
}

// ── X11 fallback: the wallpaper window as the desktop (EWMH) ────────────

// The window used to be sent to the background by two `xprop -set` calls
// AFTER showing. By EWMH only half of that works: the window manager reads
// the window type (`_NET_WM_WINDOW_TYPE`) when the window is shown, and a
// client must not write `_NET_WM_STATE` of a shown window at all — the
// manager does not re-read it and may overwrite it. So now there are two steps:
//   * BEFORE showing — properties right on the window (type, state, "on all
//     desktops"): EWMH allows a client to set them on a window not yet shown;
//   * AFTER showing — requests to the root window (ClientMessage), as EWMH
//     prescribes for changing a shown window's state. The window type is
//     rewritten again: Muffin, Marco and xfwm4 re-read it on a property change.
// Here is only the computation — what and in which order; the X server
// request is made by the wallpaper host (`rexon-gui::wallpaper_host::x11_desktop`).

/// Wallpaper window type: desktop — the WM puts it under all applications.
pub const X11_DESKTOP_WINDOW_TYPE: &str = "_NET_WM_WINDOW_TYPE_DESKTOP";

/// Wallpaper window states: no taskbar and switcher entry, on all workspaces.
///
/// ⚠️ `_NET_WM_STATE_BELOW` (the former `xprop` set it) is not set on a
/// "desktop" type window. Marco and Muffin put such a window into the desktop
/// layer without looking at BELOW (`get_standalone_layer`), while xfwm4 4.20
/// moves the window on BELOW out of the desktop layer (`WIN_LAYER_DESKTOP`,
/// 0) HIGHER — into `WIN_LAYER_BELOW` (2; `clientUpdateLayerState`). There
/// the wallpaper window covered nemo-desktop's icons, and neither
/// ConfigureWindow nor raising the icons window lowered it: the layer matters
/// more than the order (Nemo test bench, 31.10.2026).
pub const X11_DESKTOP_STATES: [&str; 3] = [
    "_NET_WM_STATE_SKIP_TASKBAR",
    "_NET_WM_STATE_SKIP_PAGER",
    "_NET_WM_STATE_STICKY",
];

/// `_NET_WM_DESKTOP` = "on all desktops" (EWMH: 0xFFFFFFFF).
pub const NET_WM_DESKTOP_ALL: u32 = 0xFFFF_FFFF;

/// `_NET_WM_STATE_ADD` — the action in `data.l[0]` of a `_NET_WM_STATE` request.
pub const NET_WM_STATE_ADD: u32 = 1;

/// Request source — a regular application (EWMH: 1; 2 — a pager).
pub const EWMH_SOURCE_APPLICATION: u32 = 1;

/// Atoms of predefined X11 property types (`XA_ATOM`, `XA_CARDINAL`).
pub const XA_ATOM: u32 = 4;
pub const XA_CARDINAL: u32 = 6;

/// Event mask of a request to the root window:
/// `SubstructureRedirectMask | SubstructureNotifyMask` (EWMH, «Root Window
/// Messages»).
pub const ROOT_MESSAGE_EVENT_MASK: u32 = (1 << 20) | (1 << 19);

/// The ClientMessage event code in the X11 protocol.
pub const X11_CLIENT_MESSAGE: u8 = 33;

/// A window property value or a request word: an atom name (its number is
/// learned from the server) or a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11Word {
    Atom(&'static str),
    Num(u32),
}

/// A property set directly on the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11Property {
    /// Property name (`_NET_WM_WINDOW_TYPE`, …).
    pub name: &'static str,
    /// Value type: [`XA_ATOM`] or [`XA_CARDINAL`].
    pub kind: u32,
    /// The value — 32-bit words.
    pub values: Vec<X11Word>,
}

/// A request to the root window about the wallpaper window (EWMH ClientMessage, format 32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11ClientMessage {
    /// The event's `message_type` (`_NET_WM_STATE`, `_NET_WM_DESKTOP`).
    pub message_type: &'static str,
    /// `data.l[0..5]`.
    pub data: [X11Word; 5],
}

/// What to do with the wallpaper window on X11 — before showing and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11DesktopPlan {
    /// Properties for the window BEFORE showing (type first — the WM reads it on showing).
    pub before_map: Vec<X11Property>,
    /// Properties rewritten on the shown window (only the type: a shown window's
    /// state is not changed this way).
    pub after_map_properties: Vec<X11Property>,
    /// Requests to the root window for the shown window.
    pub after_map_messages: Vec<X11ClientMessage>,
}

fn window_type_property() -> X11Property {
    X11Property {
        name: "_NET_WM_WINDOW_TYPE",
        kind: XA_ATOM,
        values: vec![X11Word::Atom(X11_DESKTOP_WINDOW_TYPE)],
    }
}

/// The X11 fallback plan: type, state and "on all desktops" — before showing
/// the window, `_NET_WM_STATE`/`_NET_WM_DESKTOP` requests — after.
///
/// One `_NET_WM_STATE` request carries at most two states (`data.l[1]`,
/// `data.l[2]`), so three states go as two requests; an empty second slot is
/// 0 (EWMH).
pub fn x11_desktop_plan() -> X11DesktopPlan {
    let [skip_taskbar, skip_pager, sticky] = X11_DESKTOP_STATES;
    let state_add = |first: &'static str, second: Option<&'static str>| X11ClientMessage {
        message_type: "_NET_WM_STATE",
        data: [
            X11Word::Num(NET_WM_STATE_ADD),
            X11Word::Atom(first),
            second.map_or(X11Word::Num(0), X11Word::Atom),
            X11Word::Num(EWMH_SOURCE_APPLICATION),
            X11Word::Num(0),
        ],
    };
    X11DesktopPlan {
        before_map: vec![
            window_type_property(),
            X11Property {
                name: "_NET_WM_STATE",
                kind: XA_ATOM,
                values: X11_DESKTOP_STATES.iter().map(|a| X11Word::Atom(*a)).collect(),
            },
            X11Property {
                name: "_NET_WM_DESKTOP",
                kind: XA_CARDINAL,
                values: vec![X11Word::Num(NET_WM_DESKTOP_ALL)],
            },
        ],
        after_map_properties: vec![window_type_property()],
        after_map_messages: vec![
            state_add(skip_taskbar, Some(skip_pager)),
            state_add(sticky, None),
            X11ClientMessage {
                message_type: "_NET_WM_DESKTOP",
                data: [
                    X11Word::Num(NET_WM_DESKTOP_ALL),
                    X11Word::Num(EWMH_SOURCE_APPLICATION),
                    X11Word::Num(0),
                    X11Word::Num(0),
                    X11Word::Num(0),
                ],
            },
        ],
    }
}

/// All atom names the plan needs to learn from the server, without repeats,
/// in order of first mention.
pub fn x11_desktop_plan_atoms(plan: &X11DesktopPlan) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let mut push = |name: &'static str| {
        if !out.contains(&name) {
            out.push(name);
        }
    };
    let props = plan.before_map.iter().chain(&plan.after_map_properties);
    for p in props {
        push(p.name);
        for v in &p.values {
            if let X11Word::Atom(a) = v {
                push(*a);
            }
        }
    }
    for m in &plan.after_map_messages {
        push(m.message_type);
        for v in &m.data {
            if let X11Word::Atom(a) = v {
                push(*a);
            }
        }
    }
    out
}

/// Words into numbers: atoms via `atom` (the number from the server; `None` —
/// the server did not give the atom, and the word must not be set).
pub fn x11_resolve_words(
    words: &[X11Word],
    atom: impl Fn(&str) -> Option<u32>,
) -> Option<Vec<u32>> {
    words
        .iter()
        .map(|w| match w {
            X11Word::Num(n) => Some(*n),
            X11Word::Atom(a) => atom(a),
        })
        .collect()
}

/// 32 bytes of a ClientMessage event (format 32) in machine byte order — the
/// way `xcb_send_event` expects it: code, format, sequence (0, set by the
/// server), window, `message_type`, five `data.l` words.
pub fn x11_client_message_bytes(window: u32, message_type: u32, data: [u32; 5]) -> [u8; 32] {
    let mut ev = [0u8; 32];
    ev[0] = X11_CLIENT_MESSAGE;
    ev[1] = 32;
    ev[4..8].copy_from_slice(&window.to_ne_bytes());
    ev[8..12].copy_from_slice(&message_type.to_ne_bytes());
    for (i, word) in data.iter().enumerate() {
        let at = 12 + i * 4;
        ev[at..at + 4].copy_from_slice(&word.to_ne_bytes());
    }
    ev
}

// ── X11: order of the wallpaper window and the desktop icons window ───────
//
// Desktop icons on X11 are drawn by the file manager in ITS OWN window of the
// same "desktop" type: Caja (MATE), nemo-desktop (Cinnamon), xfdesktop
// (XFCE). The window manager puts all such windows into one bottom layer and
// orders them within the layer by showing time — whoever is shown later is
// higher. The wallpaper window is shown after the icons and covers them
// (MATE 1.28, 31.10.2026: the host window above the Caja window, the icons
// not visible).
//
// Where to put the wallpaper window is decided by the icons window background:
//   * transparent (the window has 32-bit depth — an alpha channel — AND a
//     compositor is running): the wallpaper goes UNDER the icons, it shows
//     through between them. This is always the case with nemo-desktop and
//     with Caja with the `org.mate.caja.desktop transparent-background` key;
//   * opaque (Caja without the key, xfdesktop, any window without a
//     compositor): the wallpaper goes ABOVE it, otherwise it is not visible
//     at all; the icons are hidden meanwhile, and the person is told about
//     it on apply (`wallpaper_backend::prepare_desktop_icons`).
// A window that draws only the background (Cinnamon's `csd-background` under
// XWayland) stays under the wallpaper in any case.
//
// Here is only the decision from observations; the server requests are made
// by the host (`rexon-gui::wallpaper_host::x11_desktop::keep_order`).

/// `WM_CLASS` of windows that draw only the desktop background and lie under the wallpaper.
pub const X11_BACKGROUND_ONLY_CLASSES: [&str; 1] = ["csd-background"];

/// Root properties by which file managers name their desktop window.
pub const X11_ICONS_WINDOW_ROOT_PROPERTIES: [&str; 2] = ["CAJA_DESKTOP_WINDOW_ID", "NAUTILUS_DESKTOP_WINDOW_ID"];

/// The source of a `_NET_RESTACK_WINDOW` request is a pager: a direct action
/// that managers carry out without focus-stealing checks (EWMH).
pub const EWMH_SOURCE_PAGER: u32 = 2;

/// ConfigureWindow's `stack_mode` and `_NET_RESTACK_WINDOW`'s `detail`.
pub const X11_STACK_ABOVE: u32 = 0;
pub const X11_STACK_BELOW: u32 = 1;

/// Depth of a window with an alpha channel (an ARGB visual).
pub const X11_ALPHA_DEPTH: u8 = 32;

/// Observations about a foreign window from `_NET_CLIENT_LIST_STACKING`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct X11ClientFacts {
    /// The window type is `_NET_WM_WINDOW_TYPE_DESKTOP`.
    pub desktop_type: bool,
    /// The root window names it the file manager's desktop window
    /// ([`X11_ICONS_WINDOW_ROOT_PROPERTIES`]).
    pub announced_icons: bool,
    /// `WM_CLASS`: instance name and class.
    pub instance: String,
    pub class: String,
    /// Window depth (bits per pixel); 0 — unknown.
    pub depth: u8,
}

/// What the window is to the wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11StackRole {
    /// Our wallpaper window.
    Own,
    /// The desktop icons window; `see_through` — its background is transparent.
    Icons { see_through: bool },
    /// A window that draws only the desktop background.
    Background,
    /// Any other window — does not affect the wallpaper order.
    Other,
}

/// The role of a foreign window. `compositing` — the screen has a compositor
/// (the owner of the `_NET_WM_CM_Sn` selection): without it window alpha is
/// not blended, and a 32-bit-deep window still covers what is under it.
pub fn x11_stack_role(facts: &X11ClientFacts, compositing: bool) -> X11StackRole {
    let named = |names: &[&str]| {
        names
            .iter()
            .any(|n| facts.instance.eq_ignore_ascii_case(n) || facts.class.eq_ignore_ascii_case(n))
    };
    if !facts.announced_icons && !facts.desktop_type {
        return X11StackRole::Other;
    }
    if !facts.announced_icons && named(&X11_BACKGROUND_ONLY_CLASSES) {
        return X11StackRole::Background;
    }
    X11StackRole::Icons { see_through: compositing && facts.depth == X11_ALPHA_DEPTH }
}

/// Where to restack the wallpaper window relative to the neighbor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11Restack {
    Below(u32),
    Above(u32),
}

impl X11Restack {
    /// The neighbor and `stack_mode` for the request.
    pub fn sibling_and_mode(self) -> (u32, u32) {
        match self {
            X11Restack::Below(w) => (w, X11_STACK_BELOW),
            X11Restack::Above(w) => (w, X11_STACK_ABOVE),
        }
    }
}

/// Whether the wallpaper window `own` must be restacked in the `stack` stack
/// (bottom to top, like `_NET_CLIENT_LIST_STACKING`), and where. `None` — it
/// stands where it should.
///
/// * There is at least one opaque icons window — the wallpaper goes above the
///   topmost icons window (otherwise it is not visible).
/// * All icons windows are transparent — the wallpaper goes under the lowest one.
/// * Windows that draw only the background always stay under the wallpaper.
///
/// Recomputing after a fulfilled request gives `None`: on each check round
/// only what is out of place gets restacked.
pub fn x11_restack_goal(stack: &[(u32, X11StackRole)], own: u32) -> Option<X11Restack> {
    let own_at = stack.iter().position(|(w, role)| *w == own && *role == X11StackRole::Own)?;
    let icons: Vec<(usize, bool)> = stack
        .iter()
        .enumerate()
        .filter_map(|(i, (_, role))| match role {
            X11StackRole::Icons { see_through } => Some((i, *see_through)),
            _ => None,
        })
        .collect();
    let background_top = stack.iter().rposition(|(_, role)| *role == X11StackRole::Background);
    if icons.is_empty() {
        return match background_top {
            Some(bg) if own_at < bg => Some(X11Restack::Above(stack[bg].0)),
            _ => None,
        };
    }
    if icons.iter().any(|(_, see_through)| !see_through) {
        let top = icons.iter().map(|(i, _)| *i).max().unwrap_or(0);
        return (own_at < top).then(|| X11Restack::Above(stack[top].0));
    }
    let lowest = icons.iter().map(|(i, _)| *i).min().unwrap_or(0);
    if own_at > lowest {
        return Some(X11Restack::Below(stack[lowest].0));
    }
    match background_top {
        Some(bg) if own_at < bg => Some(X11Restack::Above(stack[bg].0)),
        _ => None,
    }
}

/// How many times in a row to ask the window manager for the same order with
/// the same stack before admitting a refusal. The manager fulfills a request
/// within milliseconds, and the check runs every 2 s — three rounds cover a
/// slow start with a margin.
pub const X11_RESTACK_TRIES: u32 = 3;

/// What the host should do on this check round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11RestackStep {
    /// Ask the manager for the order.
    Ask(X11Restack),
    /// The requests were not fulfilled [`X11_RESTACK_TRIES`] times — say so in the
    /// journal once and stop asking.
    GiveUp(X11Restack),
    /// The refusal is already admitted, the stack is the same — stay silent.
    Quiet,
}

/// Bookkeeping of order requests for one wallpaper window between check rounds.
///
/// The window manager may silently not fulfill a request: the window is in
/// another layer (xfwm4 with BELOW kept the wallpaper window above the
/// desktop layer), the manager's policy. Then the check would ask for the
/// same thing every 2 s forever and write the same to the journal. A refusal
/// is the same goal with the same stack after [`X11_RESTACK_TRIES`] requests;
/// any change of the bottom layer (the icons window recreated, the manager
/// changed, the goal became different) starts the count anew.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct X11RestackTracker {
    /// The goal and the bottom layer (non-`Other` windows, bottom to top) of the last request.
    last: Option<(X11Restack, Vec<(u32, X11StackRole)>)>,
    tries: u32,
    /// The icons window has already been recreated for this wallpaper window.
    icons_recreated: bool,
}

impl X11RestackTracker {
    /// A check step for the window `own` in the stack `stack` (bottom to top).
    /// `None` — the window stands where it should, nothing to ask.
    pub fn step(&mut self, stack: &[(u32, X11StackRole)], own: u32) -> Option<X11RestackStep> {
        let Some(goal) = x11_restack_goal(stack, own) else {
            self.last = None;
            self.tries = 0;
            return None;
        };
        let layer: Vec<(u32, X11StackRole)> =
            stack.iter().copied().filter(|(_, role)| *role != X11StackRole::Other).collect();
        let same = matches!(&self.last, Some((g, l)) if *g == goal && *l == layer);
        if !same {
            self.last = Some((goal, layer));
            self.tries = 0;
        }
        self.tries = self.tries.saturating_add(1);
        Some(match self.tries {
            n if n <= X11_RESTACK_TRIES => X11RestackStep::Ask(goal),
            n if n == X11_RESTACK_TRIES + 1 => X11RestackStep::GiveUp(goal),
            _ => X11RestackStep::Quiet,
        })
    }

    /// After [`X11RestackStep::GiveUp`]: whether to recreate the icons window.
    ///
    /// xfwm4 does not lower the wallpaper window under the icons window (it
    /// discards order requests for a "desktop" type window), but puts a new icons
    /// window above the wallpaper. So transparent icons shown before the host are
    /// reached by recreating their window (how to do that is decided by
    /// `wallpaper_backend::icons_window_toggle` by the window class). Only for the
    /// "under the icons" goal (a new window will not help above opaque icons) and
    /// once per wallpaper window: if it did not help, we do not poke the icons in
    /// a loop any more.
    pub fn take_icons_recreate(&mut self, goal: X11Restack) -> bool {
        if !matches!(goal, X11Restack::Below(_)) || self.icons_recreated {
            return false;
        }
        self.icons_recreated = true;
        true
    }
}

/// The X screen number from `DISPLAY` (`:0.1` → 1); without a number — 0.
/// Needed for the compositor selection name `_NET_WM_CM_S<number>`.
pub fn x11_screen_number(display: &str) -> u32 {
    let after_colon = display.rsplit_once(':').map(|(_, rest)| rest).unwrap_or("");
    after_colon
        .split_once('.')
        .and_then(|(_, screen)| screen.parse().ok())
        .unwrap_or(0)
}

/// The name of the selection owned by the screen's compositor (EWMH).
pub fn x11_compositor_selection(screen: u32) -> String {
    format!("_NET_WM_CM_S{screen}")
}

// ── headless render-loop ───────────────────────────────────────────────────

/// QTimer tick interval (ms) — port of `max(8, int(1000 / max(fps, 1)))` from
/// `run_headless_loop`. `fps <= 0` → 1000 ms; the top is capped at ~125 Hz (8 ms).
pub fn headless_tick_ms(fps: i64) -> i64 {
    (1000 / fps.max(1)).max(8)
}

/// The mmap stream frame rate from the `REXON_WP_FPS` value — port of
/// `_env_fps` from `video_wallpaper.py`.
///
/// The variable is set by the shell extension. Garbage in it must not leave
/// the person without a wallpaper, so a value not understood or outside
/// [`MIN_FPS`]..=[`MAX_FPS`] falls back to [`DEFAULT_FPS`].
///
/// It lives here, not in `wallpaper.rs`: the variable describes exactly the
/// mmap frame stream, next to [`headless_tick_ms`], which computes the timer
/// interval from it.
pub fn env_fps(raw: Option<&str>) -> i64 {
    let Some(v) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return DEFAULT_FPS;
    };
    match v.parse::<i64>() {
        Ok(fps) if (MIN_FPS..=MAX_FPS).contains(&fps) => fps,
        _ => DEFAULT_FPS,
    }
}

// ── the host's Qt platform in frame stream mode ───────────────────────────

/// A Qt platform without windows and without a graphics card: the host lives
/// on it while handing frames to the extension, if the frame is not drawn by
/// `QQuickRenderControl`.
pub const OFFSCREEN_QPA_PLATFORM: &str = "offscreen";
/// The Qt platform of a Wayland session (GNOME/Cinnamon-Wayland — `WAYLAND_DISPLAY`).
pub const WAYLAND_QPA_PLATFORM: &str = "wayland";
/// The Qt platform of an X11 or XWayland session (`DISPLAY`).
pub const XCB_QPA_PLATFORM: &str = "xcb";

/// How the host captures a frame in frame stream mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamCapture {
    /// The frame is not taken from the Qt Quick scene: for video it comes from
    /// `QVideoSink` (VideoOutput does not draw on `offscreen`), for Flash wgpu
    /// writes it itself. Qt rendering is not needed, the platform stays
    /// `offscreen` — there is no window there by design.
    OwnFrames,
    /// The Qt Quick scene is drawn by `QQuickRenderControl` into a texture and
    /// read back (web). The QML window is NEVER shown, so the platform can be a
    /// real one — and get the graphics card.
    RenderControl,
}

/// How a host mode captures a frame in frame stream mode — ONE table.
pub fn host_stream_capture(mode: crate::wallpaper_engine::HostMode) -> StreamCapture {
    use crate::wallpaper_engine::HostMode;
    match mode {
        HostMode::Web => StreamCapture::RenderControl,
        // The output client never writes into the frame stream; the answer is for
        // the completeness of the table.
        HostMode::Video | HostMode::Swf | HostMode::Waywallen => StreamCapture::OwnFrames,
    }
}

/// What to write into `QT_QPA_PLATFORM` for the wallpaper host BEFORE the Qt
/// application is created. `None` — touch nothing.
///
/// ONE decision for all host modes, rule #12. Previously only the web host
/// made it, and the video host on GNOME/Cinnamon-Wayland brought up a regular
/// window: measured under a Wayland compositor without layer-shell — the
/// client asks for `xdg_surface.get_toplevel` with the title
/// «RexonVideoWallpaper» and hands frames to it, under Xvfb — a visible
/// 1920×1080 window (`IsViewable`). On Mutter this is a black full-screen
/// window over the desktop.
///
/// Web (`StreamCapture::RenderControl`) — on the session's REAL platform
/// (15.09.2026). On `offscreen` Qt has no EGL, and GLX only via `DISPLAY`
/// (qtbase `plugins/platforms/offscreen`), and even with it QtWebEngine 6.11
/// on `offscreen` with a GPU compositor does not put the frame into the scene
/// at all (`render_widget_host_view_qt_delegate_item.cpp`, QTBUG-132682).
/// Measured on the test bench (1920×1080, 30 fps stream): `offscreen` — CSS
/// 17 fps at 157 % CPU, no WebGL; `wayland` + `QQuickRenderControl` — the
/// graphics card (ANGLE, Radeon 780M), CSS 31 fps at 43 % CPU, WebGL 30 fps at
/// 25 % with a live frame; `xcb` — 38 and 26 %. There is no window on screen
/// meanwhile: during the whole run the client created not a single
/// `wl_surface` (0 `create_surface`, 0 `get_toplevel`). Without
/// `WAYLAND_DISPLAY` and `DISPLAY` — `offscreen`, software rendering.
///
/// ⚠️ An already set value is OVERRIDDEN, not respected: in frame stream mode
/// there must be no window on screen on any platform, and GNOME users often
/// have `QT_QPA_PLATFORM=wayland` set for the whole session (that is the
/// common advice for Qt programs) — the video would inherit it from the shell
/// and put a window over the desktop again.
pub fn host_qpa_platform(
    stream_to_file: bool,
    capture: StreamCapture,
    current: Option<&str>,
    wayland_display: &str,
    x_display: &str,
) -> Option<&'static str> {
    if !stream_to_file {
        return None;
    }
    let wanted = stream_qpa_platform(capture, wayland_display, x_display);
    if current == Some(wanted) {
        None
    } else {
        Some(wanted)
    }
}

/// Whether a window may be shown in stream mode for the sake of grabbing a
/// frame. Only on `offscreen`: there the window has no screen by design. On
/// any real platform a shown window on Mutter comes out over the desktop —
/// the very defect for which the frame stream lives without a window at all.
pub fn stream_window_may_show(platform: &str) -> bool {
    platform == OFFSCREEN_QPA_PLATFORM
}

/// The Qt platform of frame stream mode — by itself, without checking against the current one.
pub fn stream_qpa_platform(
    capture: StreamCapture,
    wayland_display: &str,
    x_display: &str,
) -> &'static str {
    match capture {
        StreamCapture::OwnFrames => OFFSCREEN_QPA_PLATFORM,
        StreamCapture::RenderControl if !wayland_display.trim().is_empty() => {
            WAYLAND_QPA_PLATFORM
        }
        StreamCapture::RenderControl if !x_display.trim().is_empty() => XCB_QPA_PLATFORM,
        StreamCapture::RenderControl => OFFSCREEN_QPA_PLATFORM,
    }
}

// ── frame generation counter ───────────────────────────────────────────────

/// The ending of the counter file name next to the frame file
/// (`rexon-wp-frame.rgba` → `rexon-wp-frame.rgba.gen`). The same string stands
/// in the shared read block of both `extension.js` files (guarded by
/// `tests/wallpaper_mmap_parity.rs`).
pub const FRAME_GEN_SUFFIX: &str = ".gen";

/// Counter size: u64 little-endian, there is nothing else in the file.
pub const FRAME_GEN_BYTES: usize = 8;

/// The generation counter path for a frame file.
///
/// Why it exists. The host writes the frame under `flock(LOCK_EX)`, but the
/// shell extension cannot take the lock: GJS has neither `flock` nor `fcntl`.
/// Measured at 1920×1080 and 30 frames: the host holds the lock 6 % of the
/// time (writing a frame ~1.9 ms), the reader's copy takes ~1.7 ms — and 12
/// torn frames out of 180 ticks.
///
/// The contract (seqlock): before writing a frame the host puts an ODD number
/// into the counter, after it — the next even one. The reader takes the
/// counter before and after copying the frame; odd or changed — the frame is
/// skipped (the previous one stays on screen). The layout of the frame file
/// itself does NOT change: exactly width×height×4 bytes without a header, and
/// an extension of an older build reads it as before. No counter (a host of
/// an older build) — the reader behaves the old way.
pub fn frame_generation_path(frame_path: &str) -> String {
    format!("{frame_path}{FRAME_GEN_SUFFIX}")
}

/// Generation counter bytes — u64 little-endian.
pub fn frame_generation_bytes(generation: u64) -> [u8; FRAME_GEN_BYTES] {
    generation.to_le_bytes()
}

// ── internals ──────────────────────────────────────────────────────────────

/// Python `int(str)` semantics for ENV values: whitespace trimming, a sign,
/// integers only. (Python 3.6+ underscore separators are not supported — they
/// do not occur in ENV screen sizes.)
fn py_int(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<i64>().ok()
}
