//! The «do not blank the screen» bridge: `org.freedesktop.ScreenSaver` where it is missing.
//!
//! WHY. Browsers and players ask «do not blank the screen, a video is playing» through
//! the `org.freedesktop.ScreenSaver` service (`Inhibit(application, reason)` →
//! cookie, `UnInhibit(cookie)`). In Plasma the session itself provides this service, while in MATE it
//! does not exist at all: the screensaver there answers only to `org.mate.ScreenSaver`,
//! the call goes nowhere (`ServiceUnknown`), and the screen blanks in the middle of a film.
//! Measurement on a live machine 16.10.2026 (MATE 1.28, X11, Firefox 157): the name has
//! no owner on the bus; the `org.freedesktop.portal.Inhibit` portal does not reach
//! `mate-session` (`IsInhibited(8)` stays `false`); a direct
//! `org.gnome.SessionManager.Inhibit(…, 8)` from a live client gives
//! `IsInhibited(8)` = `true`.
//!
//! WHAT IT DOES. Rexon lives for the whole session (tray icon), so it takes
//! the name: if `org.freedesktop.ScreenSaver` has no owner in the session, Rexon places
//! the object on both paths, as Plasma does (`/org/freedesktop/ScreenSaver` and
//! `/ScreenSaver`), and forwards every inhibition to whatever in this environment
//! really holds the screen: in MATE that is `org.gnome.SessionManager.Inhibit`
//! with flag 8 («idle»; mate-session holds it), in XFCE —
//! `org.freedesktop.PowerManagement.Inhibit` (xfce4-power-manager). Where the
//! screen is blanked by a service that listens only to the Wayland protocol (swayidle in sway and
//! Budgie, KIdleTime in LXQt — both via `ext-idle-notify-v1`), there is nothing on the bus
//! to hold it with: the bridge holds an idle inhibition with the Wayland protocol
//! `idle-inhibit-unstable-v1` on its own invisible layer surface
//! ([`WAYLAND_IDLE`]) — but only on compositors that, according to their sources,
//! are known to honor such a surface ([`WAYLAND_IDLE_COMPOSITORS`]).
//! On the others the bridge does NOT take the name: native Firefox, getting no answer on
//! the bus, falls back to Wayland idle-inhibit on its own window by itself, and a name
//! taken by us would stop it at us. An inhibition is released on `UnInhibit` and on its own —
//! when the client disappears from the bus (the browser was closed or crashed): otherwise the screen
//! would never blank again.
//!
//! ⚠️ The session manager attributes the inhibition to Rexon's bus CONNECTION and
//! releases it by itself when that connection closes. So forwarding
//! is only possible from the process's long-lived connection — not from `gdbus` and not
//! from another short-lived process: it would exit, and the inhibition would vanish along with
//! it.
//!
//! BOUNDARY. Only decisions live here: whether the bridge is needed in this environment, where to
//! forward, cookie bookkeeping and what to reply. The bus is on the executor
//! (`rexon-gui/src/screensaver_bridge.rs` over the shim
//! `cpp/rexon_screensaver_bridge.cpp`). Everything below is checked by ordinary core
//! tests, without a bus.
//!
//! The interface description follows the freedesktop specification (the
//! idle-inhibition draft, <https://specifications.freedesktop.org/idle-inhibit-spec/latest/>)
//! and the KDE interface description installed by kscreenlocker
//! (`/usr/share/dbus-1/interfaces/kf6_org.freedesktop.ScreenSaver.xml`).
//! We implement what we can honestly execute: `Inhibit`, `UnInhibit`,
//! `GetActive`, `SetActive`, `GetActiveTime`, `SimulateUserActivity`, `Lock`.
//! `Throttle`/`GetSessionIdleTime` and the `ActiveChanged` signal are not declared:
//! there is nothing to execute them with, and declaring them and staying silent is worse than not declaring them.

use std::collections::BTreeMap;
use std::time::Duration;

/// Service name on the session bus.
pub const SERVICE: &str = "org.freedesktop.ScreenSaver";
/// Interface name.
pub const INTERFACE: &str = "org.freedesktop.ScreenSaver";
/// Object paths: both, as in Plasma. Clients call one or the other
/// (Firefox and Chromium — `/ScreenSaver`, some players —
/// `/org/freedesktop/ScreenSaver`).
pub const OBJECT_PATHS: [&str; 2] = ["/org/freedesktop/ScreenSaver", "/ScreenSaver"];

/// How long to wait after Rexon starts before checking whether the name is free.
///
/// Rexon comes up via autostart at login — together with the session services.
/// The real owner of the name (in environments that have one) may not be up yet, and
/// taking the name before it would mean taking it away from the session. Environments where
/// the owner is known are skipped by the bridge anyway ([`desktop_policy`]); the pause protects
/// those not in the table, and MATE with a screensaver that provides the name
/// itself (since 1.28.1): it takes the name without queueing and without replacement, so a name
/// taken before it will never reach it.
pub const START_DELAY: Duration = Duration::from_secs(30);

/// From this version mate-screensaver holds `org.freedesktop.ScreenSaver` itself
/// on both paths (release 1.28.1 of 18.05.2026) — the bridge is not needed in MATE.
pub const MATE_SCREENSAVER_FDO_SINCE: (u32, u32, u32) = (1, 28, 1);

/// The command that tells us the mate-screensaver version. The client, not the
/// screensaver itself: that one brings up GTK on `--version` and reaches for the accessibility bus.
/// They share one package, one version.
pub const MATE_SCREENSAVER_VERSION_ARGV: [&str; 2] = ["mate-screensaver-command", "--version"];

/// Version from a line like `mate-screensaver-command 1.28.0`.
pub fn parse_version(output: &str) -> Option<(u32, u32, u32)> {
    output.split_whitespace().rev().find_map(|word| {
        let mut parts = word.split('.').map(|p| p.parse::<u32>());
        let major = parts.next()?.ok()?;
        let minor = parts.next()?.ok()?;
        let patch = match parts.next() {
            Some(p) => p.ok()?,
            None => 0,
        };
        Some((major, minor, patch))
    })
}

/// Whether the executor needs to learn the mate-screensaver version for [`decide`].
pub fn wants_mate_screensaver_version(de: Option<&str>) -> bool {
    de == Some("mate")
}

/// The «idle» flag of `org.gnome.SessionManager.Inhibit`: do not consider the session
/// idle — do not start the screensaver and do not blank the screen.
pub const GSM_INHIBIT_IDLE: u32 = 8;

/// D-Bus errors the bridge replies with.
pub const ERROR_UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";
pub const ERROR_INVALID_ARGS: &str = "org.freedesktop.DBus.Error.InvalidArgs";
pub const ERROR_NOT_SUPPORTED: &str = "org.freedesktop.DBus.Error.NotSupported";
pub const ERROR_FAILED: &str = "org.freedesktop.DBus.Error.Failed";

/// Interface description for `Introspect` (only `<interface>`: the node and
/// standard interfaces are added by QtDBus).
pub const INTROSPECTION_XML: &str = r#"<interface name="org.freedesktop.ScreenSaver">
  <method name="Inhibit">
    <arg name="application_name" type="s" direction="in"/>
    <arg name="reason_for_inhibit" type="s" direction="in"/>
    <arg name="cookie" type="u" direction="out"/>
  </method>
  <method name="UnInhibit">
    <arg name="cookie" type="u" direction="in"/>
  </method>
  <method name="GetActive">
    <arg type="b" direction="out"/>
  </method>
  <method name="SetActive">
    <arg name="e" type="b" direction="in"/>
    <arg type="b" direction="out"/>
  </method>
  <method name="GetActiveTime">
    <arg name="seconds" type="u" direction="out"/>
  </method>
  <method name="SimulateUserActivity">
  </method>
  <method name="Lock">
  </method>
</interface>
"#;

/// The standard object description interface.
pub const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

/// The full reply to `Introspect`: a node with the description and our interface.
pub fn introspection_node() -> String {
    format!(
        "<!DOCTYPE node PUBLIC \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\" \
         \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\n<node>\n\
         <interface name=\"{INTROSPECTABLE}\">\n  <method name=\"Introspect\">\n    \
         <arg name=\"xml_data\" type=\"s\" direction=\"out\"/>\n  </method>\n</interface>\n\
         {INTROSPECTION_XML}</node>\n"
    )
}

// ============================================================================
// Environments: where the bridge is needed
// ============================================================================

/// How the environment stands with respect to `org.freedesktop.ScreenSaver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopPolicy {
    /// The environment provides the name itself (named who). The bridge is not needed and is NOT installed,
    /// even if the name is free at this minute: its owner may be restarting,
    /// and it would never get a name taken by us.
    OwnProvider(&'static str),
    /// There is no own owner, but nothing to forward to on the bus either: the screen there is blanked by
    /// a service that looks only at the Wayland idle-inhibit protocol, not
    /// at the session manager. It can only be held with a Wayland inhibition
    /// ([`WAYLAND_IDLE`]); if that is unavailable — we do NOT take the name: the client would get a cookie
    /// while the screen blanks.
    NoBusTarget(&'static str),
    /// The environment has no own owner — the bridge is installed if there is something to
    /// forward to.
    Bridge,
}

/// Environments with their own owner of the name. Key — the core's environment identifier
/// ([`crate::system::detect_current_de`]).
///
/// Sources are in the table of the commit report and in the line comments.
const OWN_PROVIDERS: &[(&str, &str)] = &[
    // Plasma: the lock screen service (libKScreenLocker in ksmserver) and
    // PowerDevil — both paths, `Inhibit` goes to power management.
    ("kde", "ksmserver (kscreenlocker) + PowerDevil"),
    // GNOME: gsd-screensaver-proxy from gnome-settings-daemon, unit
    // `org.gnome.SettingsDaemon.ScreensaverProxy` — forwards to
    // gnome-session with the «idle» flag.
    ("gnome", "gsd-screensaver-proxy"),
    // Cinnamon: csd-screensaver-proxy from cinnamon-settings-daemon,
    // autostart with `OnlyShowIn=X-Cinnamon`, the same forwarding to cinnamon-session.
    ("cinnamon-mint", "csd-screensaver-proxy"),
    // COSMIC: cosmic-idle (started by cosmic-session), both paths, Inhibit and
    // UnInhibit remove the screen blanking and sleep timers.
    ("cosmic", "cosmic-idle"),
    // niri since 0.1.2 holds the name itself on both paths and turns Inhibit into an idle
    // inhibition (src/dbus/freedesktop_screensaver.rs) — in the session instance.
    // Under LXQt niri runs without `--session`, does not take the name, and the environment there
    // is «lxqt», not «niri».
    ("niri", "niri"),
];

/// Environments where nothing on the bus can hold the screen — only the Wayland protocol.
const NO_BUS_TARGET: &[(&str, &str)] = &[
    // Budgie 10.10 (labwc): the screen is blanked by `swayidle -w`, started by
    // budgie-daemon (src/daemon/screenlock.vala); it listens only to Wayland.
    // budgie-session holds org.gnome.SessionManager, but the «idle» flag there
    // changes only Presence, and swayidle does not look at it.
    ("budgie", "swayidle (Wayland idle-inhibit only)"),
];

/// Decision by environment. `de` — the answer of [`crate::system::detect_current_de`].
pub fn desktop_policy(de: Option<&str>) -> DesktopPolicy {
    let Some(de) = de else {
        return DesktopPolicy::Bridge;
    };
    if let Some((_, who)) = OWN_PROVIDERS.iter().find(|(k, _)| *k == de) {
        return DesktopPolicy::OwnProvider(who);
    }
    if let Some((_, who)) = NO_BUS_TARGET.iter().find(|(k, _)| *k == de) {
        return DesktopPolicy::NoBusTarget(who);
    }
    DesktopPolicy::Bridge
}

// ============================================================================
// Where to forward
// ============================================================================

/// The service that really holds the screen in this environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Upstream {
    pub service: &'static str,
    pub path: &'static str,
    pub iface: &'static str,
    pub kind: UpstreamKind,
}

/// Which call the service uses to take and release an inhibition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamKind {
    /// `Inhibit(s app_id, u toplevel_xid, s reason, u flags) → u`, `Uninhibit(u)`.
    GnomeSession,
    /// `Inhibit(s application, s reason) → u`, `UnInhibit(u)`.
    PowerManagement,
    /// A screensaver: we do not take inhibitions through it.
    Companion,
    /// Not a bus service but an idle inhibition via the Wayland protocol: one for all
    /// inhibitions, held by the executor ([`Action::HoldIdle`]).
    WaylandIdleInhibit,
}

/// A session manager of the gnome-session family: mate-session, budgie-session,
/// gnome-session, cinnamon-session — all hold this name.
/// `Inhibit(s app_id, u toplevel_xid, s reason, u flags) → u`, `Uninhibit(u)`.
pub const GNOME_SESSION: Upstream = Upstream {
    service: "org.gnome.SessionManager",
    path: "/org/gnome/SessionManager",
    iface: "org.gnome.SessionManager",
    kind: UpstreamKind::GnomeSession,
};

/// XFCE power management (xfce4-power-manager): `Inhibit` disables both
/// DPMS and the screensaver (it calls org.xfce.ScreenSaver via libxfce4ui).
/// A bare `org.xfce.ScreenSaver.Inhibit` does not touch xfpm's DPMS, so
/// we forward here.
pub const POWER_MANAGEMENT: Upstream = Upstream {
    service: "org.freedesktop.PowerManagement",
    path: "/org/freedesktop/PowerManagement/Inhibit",
    iface: "org.freedesktop.PowerManagement.Inhibit",
    kind: UpstreamKind::PowerManagement,
};

/// Idle inhibition via the Wayland protocol (`zwp_idle_inhibit_manager_v1`) on
/// an invisible 1×1 layer surface from Rexon's own connection to the compositor.
/// There is no bus here: path and interface are empty, the name is for the log.
pub const WAYLAND_IDLE: Upstream = Upstream {
    service: "zwp_idle_inhibit_manager_v1",
    path: "",
    iface: "",
    kind: UpstreamKind::WaylandIdleInhibit,
};

/// Compositor globals without which a Wayland inhibition cannot be set.
pub const WAYLAND_GLOBALS_NEEDED: [&str; 4] =
    ["wl_compositor", "wl_shm", "zwlr_layer_shell_v1", "zwp_idle_inhibit_manager_v1"];

/// Compositors that honor an inhibition on an invisible layer surface —
/// according to their sources. Key — the compositor's process name (`/proc/PID/comm`
/// of the Wayland socket owner), not the environment: under LXQt and Budgie the compositors
/// differ, while `XDG_CURRENT_DESKTOP` is the same.
pub const WAYLAND_IDLE_COMPOSITORS: &[(&str, &str)] = &[
    // sway/desktop/idle_inhibit_v1.c: a layer surface's inhibition is in effect
    // while it is mapped on an enabled output («layer surfaces are always
    // on screen after they have been mapped»).
    ("sway", "sway"),
    // src/idle.c: any live inhibition = idle is inhibited, visibility is not
    // checked. The default compositor of Budgie 10.10 and of the LXQt session.
    ("labwc", "labwc"),
    // src/managers/input/IdleInhibitor.cpp: the inhibition is in effect if the view
    // is mapped and accepts input; a layer surface always has input available
    // (CLayerSurface::focusAvailable). Usually hypridle holds the name there, and the bridge
    // does not come up at all; this is for the case of swayidle instead of it.
    ("Hyprland", "Hyprland"),
];

/// What the executor learned about the Wayland session: the compositor's process name and
/// the globals it announced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaylandFacts {
    pub compositor: String,
    pub globals: Vec<String>,
}

/// Whether the screen can be held here with a Wayland inhibition; `Err` — why not.
pub fn wayland_idle_check(facts: Option<&WaylandFacts>) -> Result<(), String> {
    let Some(f) = facts else {
        return Err("not a Wayland session or the compositor did not answer".into());
    };
    if !WAYLAND_IDLE_COMPOSITORS.iter().any(|(comm, _)| *comm == f.compositor) {
        let name = if f.compositor.is_empty() { "?" } else { f.compositor.as_str() };
        return Err(format!(
            "compositor \"{name}\" not verified: unknown whether it honours an inhibitor on an invisible surface"
        ));
    }
    let missing: Vec<&str> = WAYLAND_GLOBALS_NEEDED
        .iter()
        .copied()
        .filter(|g| !f.globals.iter().any(|have| have == g))
        .collect();
    if !missing.is_empty() {
        return Err(format!("compositor {} lacks {}", f.compositor, missing.join(", ")));
    }
    Ok(())
}

/// Forwarding candidates in order of preference: the session manager holds both
/// the screensaver and the blanking (MATE: mate-screensaver watches Presence,
/// mate-power-manager — `IsInhibited(8)`), so it comes first.
pub const UPSTREAMS: &[Upstream] = &[GNOME_SESSION, POWER_MANAGEMENT];

/// The environment's screensaver — we ask it for `GetActive` and the rest.
/// All below share the interface of the gnome-screensaver 2.x successor: `GetActive() → b`,
/// `GetActiveTime() → u`, `SetActive(b)`, `SimulateUserActivity()`, `Lock()`.
pub const COMPANIONS: &[Upstream] = &[
    Upstream {
        service: "org.mate.ScreenSaver",
        path: "/org/mate/ScreenSaver",
        iface: "org.mate.ScreenSaver",
        kind: UpstreamKind::Companion,
    },
    Upstream {
        service: "org.cinnamon.ScreenSaver",
        path: "/org/cinnamon/ScreenSaver",
        iface: "org.cinnamon.ScreenSaver",
        kind: UpstreamKind::Companion,
    },
    Upstream {
        service: "org.gnome.ScreenSaver",
        path: "/org/gnome/ScreenSaver",
        iface: "org.gnome.ScreenSaver",
        kind: UpstreamKind::Companion,
    },
    Upstream {
        service: "org.xfce.ScreenSaver",
        path: "/org/xfce/ScreenSaver",
        iface: "org.xfce.ScreenSaver",
        kind: UpstreamKind::Companion,
    },
];

/// What to install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub upstream: Upstream,
    pub companion: Option<Upstream>,
}

/// The answer «install the bridge or not».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Start(Plan),
    /// Do not install — with a reason for the log.
    Skip(String),
}

/// Decide whether to install the bridge. `has_owner(name)` — whether the name has an owner on
/// the session bus (`None` — no bus).
/// `mate_screensaver_version` — output of [`MATE_SCREENSAVER_VERSION_ARGV`]
/// (`None` — not asked or no such command).
/// `wayland()` — what is known about the Wayland session (`None` — not Wayland or
/// the compositor did not answer); asked only when nothing on the bus can hold the screen.
pub fn decide(
    de: Option<&str>,
    mate_screensaver_version: Option<&str>,
    has_owner: impl Fn(&str) -> Option<bool>,
    wayland: impl FnOnce() -> Option<WaylandFacts>,
) -> Decision {
    if wants_mate_screensaver_version(de) {
        if let Some(v) = mate_screensaver_version.and_then(parse_version) {
            if v >= MATE_SCREENSAVER_FDO_SINCE {
                return Decision::Skip(format!(
                    "mate-screensaver {}.{}.{} provides {SERVICE} itself",
                    v.0, v.1, v.2
                ));
            }
        }
    }
    let policy = desktop_policy(de);
    if let DesktopPolicy::OwnProvider(who) = policy {
        return Decision::Skip(format!(
            "desktop {} provides {SERVICE} itself ({who})",
            de.unwrap_or("?")
        ));
    }
    match has_owner(SERVICE) {
        None => return Decision::Skip("no session bus".into()),
        Some(true) => return Decision::Skip(format!("{SERVICE} already has an owner")),
        Some(false) => {}
    }
    let bus_upstream = match policy {
        DesktopPolicy::NoBusTarget(_) => None,
        _ => UPSTREAMS.iter().find(|u| has_owner(u.service) == Some(true)),
    };
    if let Some(upstream) = bus_upstream {
        let companion = COMPANIONS
            .iter()
            .find(|c| has_owner(c.service) == Some(true))
            .copied();
        return Decision::Start(Plan {
            upstream: *upstream,
            companion,
        });
    }
    // Nothing on the bus can hold it — the Wayland protocol remains.
    let facts = wayland();
    match wayland_idle_check(facts.as_ref()) {
        Ok(()) => Decision::Start(Plan { upstream: WAYLAND_IDLE, companion: None }),
        Err(why) => Decision::Skip(match policy {
            DesktopPolicy::NoBusTarget(who) => format!(
                "in desktop {} the screen is blanked by {who}, and a Wayland inhibitor cannot be set: {why}",
                de.unwrap_or("?")
            ),
            _ => format!(
                "{SERVICE} is free, but there is nowhere to forward to (no {}), and a Wayland inhibitor cannot be set: {why}",
                UPSTREAMS.iter().map(|u| u.service).collect::<Vec<_>>().join(", ")
            ),
        }),
    }
}

// ============================================================================
// Inhibition bookkeeping
// ============================================================================

/// What the executor should do. Executed in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Reply to the receipt with values matching the signature.
    Reply {
        token: u64,
        signature: &'static str,
        values: Vec<String>,
    },
    /// Reply with an error.
    ReplyError {
        token: u64,
        name: &'static str,
        text: String,
    },
    /// Call a foreign service; return the reply into [`Book::on_call_done`] with `ctx`.
    Call {
        ctx: u64,
        target: Upstream,
        method: &'static str,
        signature: &'static str,
        values: Vec<String>,
        /// How long to wait for the reply, ms; [`CALL_TIMEOUT_DEFAULT`] — the bus timeout.
        timeout_ms: i32,
    },
    /// Hold (`true`) or release (`false`) the idle inhibition via the
    /// Wayland protocol ([`WAYLAND_IDLE`]). One for all inhibitions: «hold» — when
    /// the first appears, «release» — when the last is gone.
    HoldIdle(bool),
    /// Watch for a name disappearing / stop watching.
    Watch(String),
    Unwatch(String),
    /// A line into the log.
    Note(String),
}

/// Forwarding state of one inhibition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    /// The call to the session manager went out, no reply yet.
    Pending,
    /// The session manager issued its own cookie.
    Upstream(u32),
    /// The session manager refused.
    Failed,
    /// Held by the bridge itself with the shared Wayland inhibition — nothing to release at the session.
    Local,
}

#[derive(Debug, Clone)]
struct Entry {
    sender: String,
    app: String,
    held: Held,
}

/// What a call to a foreign service is waiting for.
#[derive(Debug, Clone, Copy)]
enum Purpose {
    /// Forwarding an inhibition with our cookie.
    Inhibit(u32),
    /// Releasing a forwarded inhibition.
    Release,
    /// A question to the screensaver; reply to the client's receipt.
    Companion { token: u64, reply: CompanionReply },
}

/// How to turn the screensaver's reply into a reply to the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompanionReply {
    /// `b` from the reply; on error — `false`.
    Bool,
    /// `u` from the reply; on error — 0.
    Uint,
    /// Success → `true`, error → `false` (`SetActive`).
    Success,
    /// An empty reply in any case (`SimulateUserActivity`).
    Empty,
    /// An empty reply on success, the error goes to the client (`Lock`).
    EmptyOrError,
}

/// The default bus reply timeout (QtDBus: `-1`, about 25 s).
pub const CALL_TIMEOUT_DEFAULT: i32 = -1;
/// The screensaver's reply timeout: the client is waiting for its reply, the service is local.
pub const COMPANION_TIMEOUT_MS: i32 = 2000;

/// The first cookie of the live bridge: derived from the clock so that bridges of different
/// Rexon runs do not share numbers. Never 0 and never 1.
pub fn live_first_cookie() -> u32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let mixed = (now.as_secs() as u32).wrapping_mul(1000).wrapping_add(now.subsec_millis());
    mixed.max(2)
}

/// The bridge's inhibition bookkeeping: cookies, who took them, what the session manager issued.
#[derive(Debug)]
pub struct Book {
    plan: Plan,
    next_cookie: u32,
    next_ctx: u64,
    entries: BTreeMap<u32, Entry>,
    calls: BTreeMap<u64, Purpose>,
    /// Whether the Wayland inhibition is held now (what the executor was last told).
    holding_idle: bool,
}

impl Book {
    pub fn new(plan: Plan) -> Self {
        Self {
            plan,
            next_cookie: 1,
            next_ctx: 1,
            entries: BTreeMap::new(),
            calls: BTreeMap::new(),
            holding_idle: false,
        }
    }

    /// Bookkeeping whose cookies start at `first` ([`live_first_cookie`] for the live
    /// bridge). Starting from one in every process is not allowed: a client that took cookie 1 from
    /// the previous Rexon would, after a restart, release someone else's inhibition in the new one with it.
    pub fn starting_at(plan: Plan, first: u32) -> Self {
        Self { next_cookie: first.max(1), ..Self::new(plan) }
    }

    /// How many inhibitions are alive (for the log and tests).
    pub fn active(&self) -> usize {
        self.entries.len()
    }

    fn alloc_cookie(&mut self) -> u32 {
        loop {
            let c = self.next_cookie;
            self.next_cookie = self.next_cookie.wrapping_add(1).max(1);
            if c != 0 && !self.entries.contains_key(&c) {
                return c;
            }
        }
    }

    fn call(&mut self, purpose: Purpose, target: Upstream, method: &'static str,
            signature: &'static str, values: Vec<String>) -> Action {
        let ctx = self.next_ctx;
        self.next_ctx += 1;
        // The client is waiting for the screensaver's reply — the timeout is short. Nobody waits for
        // forwarding and releasing an inhibition, and a reply dropped on timeout would leave
        // the cookie issued by the session without an owner.
        let timeout_ms = match purpose {
            Purpose::Companion { .. } => COMPANION_TIMEOUT_MS,
            Purpose::Inhibit(_) | Purpose::Release => CALL_TIMEOUT_DEFAULT,
        };
        self.calls.insert(ctx, purpose);
        Action::Call { ctx, target, method, signature, values, timeout_ms }
    }

    /// Wayland inhibition: hold it while at least one inhibition is alive.
    fn sync_idle_hold(&mut self, out: &mut Vec<Action>) {
        if self.plan.upstream.kind != UpstreamKind::WaylandIdleInhibit {
            return;
        }
        let want = !self.entries.is_empty();
        if want != self.holding_idle {
            self.holding_idle = want;
            out.push(Action::HoldIdle(want));
        }
    }

    fn sender_has_entries(&self, sender: &str) -> bool {
        self.entries.values().any(|e| e.sender == sender)
    }

    /// Release the forwarded inhibition, if there is one.
    fn release(&mut self, held: Held, out: &mut Vec<Action>) {
        if let Held::Upstream(up) = held {
            let target = self.plan.upstream;
            // The session manager has `Uninhibit`, power management has
            // `UnInhibit`: the names differ in the case of one letter.
            let method = match target.kind {
                UpstreamKind::GnomeSession => "Uninhibit",
                UpstreamKind::PowerManagement
                | UpstreamKind::Companion
                | UpstreamKind::WaylandIdleInhibit => "UnInhibit",
            };
            out.push(self.call(Purpose::Release, target, method, "u", vec![up.to_string()]));
        }
    }

    /// A call arrived at our object.
    pub fn on_message(
        &mut self,
        token: u64,
        sender: &str,
        iface: &str,
        member: &str,
        signature: &str,
        args: &[String],
    ) -> Vec<Action> {
        let mut out = Vec::new();
        // QtDBus also hands the Introspect call to a QDBusVirtualObject subclass
        // (bench measurement 17.10.2026) — we reply with the node description.
        if iface == INTROSPECTABLE && member == "Introspect" {
            out.push(Action::Reply { token, signature: "s", values: vec![introspection_node()] });
            return out;
        }
        // The interface in a call is optional (D-Bus allows calling by the method
        // name alone); a foreign one is not ours.
        if !iface.is_empty() && iface != INTERFACE {
            out.push(Action::ReplyError {
                token,
                name: ERROR_UNKNOWN_METHOD,
                text: format!("no interface {iface}"),
            });
            return out;
        }
        let expected = match member {
            "Inhibit" => "ss",
            "UnInhibit" => "u",
            "SetActive" => "b",
            "GetActive" | "GetActiveTime" | "SimulateUserActivity" | "Lock" => "",
            _ => {
                out.push(Action::ReplyError {
                    token,
                    name: ERROR_UNKNOWN_METHOD,
                    text: format!("no method {member}"),
                });
                return out;
            }
        };
        if signature != expected || args.len() != expected.len() {
            out.push(Action::ReplyError {
                token,
                name: ERROR_INVALID_ARGS,
                text: format!("{member}: expected signature \"{expected}\", got \"{signature}\""),
            });
            return out;
        }
        match member {
            "Inhibit" => self.inhibit(token, sender, &args[0], &args[1], &mut out),
            "UnInhibit" => {
                let cookie = args[0].parse::<u32>().unwrap_or(0);
                self.uninhibit(token, cookie, &mut out);
            }
            "GetActive" => self.ask_companion(token, "GetActive", "", vec![], CompanionReply::Bool, &mut out),
            "GetActiveTime" => {
                self.ask_companion(token, "GetActiveTime", "", vec![], CompanionReply::Uint, &mut out)
            }
            "SetActive" => self.ask_companion(
                token,
                "SetActive",
                "b",
                vec![args[0].clone()],
                CompanionReply::Success,
                &mut out,
            ),
            "SimulateUserActivity" => self.ask_companion(
                token,
                "SimulateUserActivity",
                "",
                vec![],
                CompanionReply::Empty,
                &mut out,
            ),
            "Lock" => self.ask_companion(token, "Lock", "", vec![], CompanionReply::EmptyOrError, &mut out),
            _ => unreachable!("method checked above"),
        }
        out
    }

    fn inhibit(&mut self, token: u64, sender: &str, app: &str, reason: &str, out: &mut Vec<Action>) {
        let cookie = self.alloc_cookie();
        let first_of_sender = !self.sender_has_entries(sender);
        // The session manager refuses an inhibition without an application name or without
        // a reason — we substitute what we know for sure: the client's bus name and
        // the application name.
        let app_id = if app.trim().is_empty() { sender.to_string() } else { app.to_string() };
        let why = if reason.trim().is_empty() { app_id.clone() } else { reason.to_string() };
        self.entries.insert(
            cookie,
            Entry {
                sender: sender.to_string(),
                app: app_id.clone(),
                held: Held::Pending,
            },
        );
        // Reply to the client right away: it needs a cookie, not the session manager's reply.
        out.push(Action::Reply { token, signature: "u", values: vec![cookie.to_string()] });
        if first_of_sender {
            out.push(Action::Watch(sender.to_string()));
        }
        let target = self.plan.upstream;
        let (signature, values) = match target.kind {
            UpstreamKind::GnomeSession => (
                "susu",
                vec![app_id.clone(), "0".into(), why.clone(), GSM_INHIBIT_IDLE.to_string()],
            ),
            UpstreamKind::PowerManagement | UpstreamKind::Companion => {
                ("ss", vec![app_id.clone(), why.clone()])
            }
            UpstreamKind::WaylandIdleInhibit => ("", vec![]),
        };
        if target.kind == UpstreamKind::WaylandIdleInhibit {
            if let Some(e) = self.entries.get_mut(&cookie) {
                e.held = Held::Local;
            }
            self.sync_idle_hold(out);
        } else {
            out.push(self.call(Purpose::Inhibit(cookie), target, "Inhibit", signature, values));
        }
        out.push(Action::Note(format!(
            "inhibition {cookie}: \"{app_id}\" ({why}) from {sender}"
        )));
    }

    fn uninhibit(&mut self, token: u64, cookie: u32, out: &mut Vec<Action>) {
        // The specification defines no error for a foreign/unknown cookie; like
        // Plasma, we reply empty — the client has nothing to do with it.
        out.push(Action::Reply { token, signature: "", values: vec![] });
        let Some(entry) = self.entries.remove(&cookie) else {
            out.push(Action::Note(format!("release {cookie}: no such cookie")));
            return;
        };
        self.release(entry.held, out);
        self.sync_idle_hold(out);
        if !self.sender_has_entries(&entry.sender) {
            out.push(Action::Unwatch(entry.sender.clone()));
        }
        out.push(Action::Note(format!("inhibition {cookie} (\"{}\") released by the client", entry.app)));
    }

    fn ask_companion(
        &mut self,
        token: u64,
        method: &'static str,
        signature: &'static str,
        values: Vec<String>,
        reply: CompanionReply,
        out: &mut Vec<Action>,
    ) {
        match self.plan.companion {
            Some(c) => out.push(self.call(Purpose::Companion { token, reply }, c, method, signature, values)),
            None => out.push(companion_reply(token, reply, None)),
        }
    }

    /// A foreign service's reply to `Action::Call` arrived.
    pub fn on_call_done(&mut self, ctx: u64, ok: bool, value: &str) -> Vec<Action> {
        let mut out = Vec::new();
        let Some(purpose) = self.calls.remove(&ctx) else {
            return out;
        };
        match purpose {
            Purpose::Inhibit(cookie) => {
                let up = if ok { value.parse::<u32>().ok() } else { None };
                match (self.entries.get_mut(&cookie), up) {
                    (Some(e), Some(up)) => {
                        e.held = Held::Upstream(up);
                        out.push(Action::Note(format!("inhibition {cookie} forwarded to the session (cookie {up})")));
                    }
                    (Some(e), None) => {
                        e.held = Held::Failed;
                        out.push(Action::Note(format!("the session refused inhibition {cookie}: {value}")));
                    }
                    // The client released the inhibition (or vanished) while the session
                    // was replying — release it at the session too, otherwise it will hang.
                    (None, Some(up)) => self.release(Held::Upstream(up), &mut out),
                    (None, None) => {}
                }
            }
            Purpose::Release => {
                if !ok {
                    out.push(Action::Note(format!("the session did not release the inhibition: {value}")));
                }
            }
            Purpose::Companion { token, reply } => {
                out.push(companion_reply(token, reply, ok.then_some(value)));
            }
        }
        out
    }

    /// The name went to another owner (or the bridge is going away): release everything at the session.
    ///
    /// Inhibitions the session has not replied to yet are released on its reply
    /// ([`Book::on_call_done`] sees that the record is already gone) — if the executor
    /// is still alive by then.
    pub fn release_all(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        let all = std::mem::take(&mut self.entries);
        for (cookie, e) in all {
            self.release(e.held, &mut out);
            out.push(Action::Note(format!("inhibition {cookie} (\"{}\") released: the bridge stepped down", e.app)));
        }
        self.sync_idle_hold(&mut out);
        out
    }

    /// The client's name disappeared from the bus: release everything it held.
    pub fn on_vanished(&mut self, name: &str) -> Vec<Action> {
        let mut out = Vec::new();
        let gone: Vec<u32> = self
            .entries
            .iter()
            .filter(|(_, e)| e.sender == name)
            .map(|(c, _)| *c)
            .collect();
        if gone.is_empty() {
            return out;
        }
        for cookie in &gone {
            if let Some(e) = self.entries.remove(cookie) {
                self.release(e.held, &mut out);
                out.push(Action::Note(format!(
                    "inhibition {cookie} (\"{}\") released: {name} left the bus",
                    e.app
                )));
            }
        }
        self.sync_idle_hold(&mut out);
        out.push(Action::Unwatch(name.to_string()));
        out
    }
}

/// Reply to the client based on the screensaver's reply (`None` — an error or no
/// screensaver).
fn companion_reply(token: u64, reply: CompanionReply, value: Option<&str>) -> Action {
    match reply {
        CompanionReply::Bool => Action::Reply {
            token,
            signature: "b",
            values: vec![(value == Some("true")).to_string()],
        },
        CompanionReply::Uint => Action::Reply {
            token,
            signature: "u",
            values: vec![value.and_then(|v| v.parse::<u32>().ok()).unwrap_or(0).to_string()],
        },
        CompanionReply::Success => Action::Reply {
            token,
            signature: "b",
            values: vec![value.is_some().to_string()],
        },
        CompanionReply::Empty => Action::Reply { token, signature: "", values: vec![] },
        CompanionReply::EmptyOrError => match value {
            Some(_) => Action::Reply { token, signature: "", values: vec![] },
            None => Action::ReplyError {
                token,
                name: ERROR_NOT_SUPPORTED,
                text: "the desktop's screensaver did not answer".into(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MATE_SS: Upstream = Upstream {
        service: "org.mate.ScreenSaver",
        path: "/org/mate/ScreenSaver",
        iface: "org.mate.ScreenSaver",
        kind: UpstreamKind::Companion,
    };

    fn plan(companion: Option<Upstream>) -> Plan {
        Plan { upstream: GNOME_SESSION, companion }
    }

    fn s(v: &str) -> String {
        v.to_string()
    }

    /// Call number and its arguments from the action list.
    fn calls(actions: &[Action]) -> Vec<(u64, &'static str, Vec<String>)> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Call { ctx, method, values, .. } => Some((*ctx, *method, values.clone())),
                _ => None,
            })
            .collect()
    }

    fn reply_values(actions: &[Action], tok: u64) -> Option<Vec<String>> {
        actions.iter().find_map(|a| match a {
            Action::Reply { token, values, .. } if *token == tok => Some(values.clone()),
            _ => None,
        })
    }

    #[test]
    fn known_providers_are_left_alone() {
        // Even with a free name and a live session manager: the name owner in
        // these environments is their own, and it must not be taken away.
        for de in ["kde", "gnome", "cinnamon-mint", "cosmic", "budgie"] {
            let d = decide(Some(de), None, |_| Some(false), || None);
            assert!(matches!(d, Decision::Skip(_)), "{de}: {d:?}");
            let owners = ["org.gnome.SessionManager"];
            let d = decide(Some(de), None, |n| Some(owners.contains(&n)), || None);
            assert!(matches!(d, Decision::Skip(_)), "{de}: {d:?}");
        }
    }

    #[test]
    fn xfce_forwards_to_power_management() {
        let owners = ["org.freedesktop.PowerManagement", "org.xfce.ScreenSaver"];
        let d = decide(Some("xfce"), None, |n| Some(owners.contains(&n)), || None);
        let Decision::Start(p) = d else { panic!("{d:?}") };
        assert_eq!(p.upstream, POWER_MANAGEMENT);
        let mut b = Book::new(p);
        let a = b.on_message(1, ":1.2", INTERFACE, "Inhibit", "ss", &[s("Firefox"), s("video")]);
        let c = calls(&a);
        assert_eq!((c[0].1, c[0].2.clone()), ("Inhibit", vec![s("Firefox"), s("video")]));
        b.on_call_done(c[0].0, true, "17");
        let a = b.on_message(2, ":1.2", INTERFACE, "UnInhibit", "u", &[s("1")]);
        let c = calls(&a);
        assert_eq!((c[0].1, c[0].2.clone()), ("UnInhibit", vec![s("17")]));
    }

    #[test]
    fn mate_gets_bridge_to_session_manager() {
        let owners = ["org.gnome.SessionManager", "org.mate.ScreenSaver"];
        let d = decide(Some("mate"), None, |n| Some(owners.contains(&n)), || None);
        assert_eq!(d, Decision::Start(plan(Some(MATE_SS))));
        // Live machine 17.10.2026: 1.28.0 — the bridge is needed.
        let d = decide(Some("mate"), Some("mate-screensaver-command 1.28.0\n"), |n| Some(owners.contains(&n)), || None);
        assert_eq!(d, Decision::Start(plan(Some(MATE_SS))));
    }

    #[test]
    fn mate_screensaver_1_28_1_owns_the_name_itself() {
        // Even if the name is free at this minute (the screensaver is not up yet or
        // is restarting): it takes the name without replacement, so it must not be taken.
        let owners = ["org.gnome.SessionManager", "org.mate.ScreenSaver"];
        for v in ["mate-screensaver-command 1.28.1", "mate-screensaver-command 1.30.0"] {
            let d = decide(Some("mate"), Some(v), |n| Some(owners.contains(&n)), || None);
            assert!(matches!(d, Decision::Skip(_)), "{v}: {d:?}");
        }
        assert_eq!(parse_version("mate-screensaver 1.28"), Some((1, 28, 0)));
        assert_eq!(parse_version("error"), None);
    }

    /// A cookie of the previous Rexon process (the client held it across the restart) does not
    /// release someone else's inhibition in the new bridge: each bridge has its own
    /// numbering, not starting from one.
    #[test]
    fn stale_cookie_of_previous_bridge_releases_nothing() {
        let mut b = Book::starting_at(plan(None), 70_001);
        let a = b.on_message(1, ":1.9", INTERFACE, "Inhibit", "ss", &[s("mpv"), s("video")]);
        b.on_call_done(calls(&a)[0].0, true, "50");
        let a = b.on_message(2, ":1.7", INTERFACE, "UnInhibit", "u", &[s("1")]);
        assert!(calls(&a).is_empty(), "someone else's inhibition released: {a:?}");
        assert_eq!(b.active(), 1);
        // The live bridge starts neither from one nor from zero.
        assert!(live_first_cookie() > 1);
    }

    /// Forwarding an inhibition to the session waits for the reply as long as needed: after
    /// a short timeout the reply used to be dropped while the session still issued a cookie —
    /// the inhibition hung on Rexon until the end of the session. A short timeout is only for
    /// questions to the screensaver, whose reply the client is waiting for.
    #[test]
    fn forwarded_inhibit_waits_for_the_session_answer() {
        let timeouts = |actions: &[Action]| -> Vec<(&'static str, i32)> {
            actions
                .iter()
                .filter_map(|a| match a {
                    Action::Call { method, timeout_ms, .. } => Some((*method, *timeout_ms)),
                    _ => None,
                })
                .collect()
        };
        let mut b = Book::new(plan(Some(GNOME_SESSION)));
        let a = b.on_message(1, ":1.4", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        assert_eq!(timeouts(&a), [("Inhibit", CALL_TIMEOUT_DEFAULT)]);
        b.on_call_done(calls(&a)[0].0, true, "41");
        let a = b.on_message(2, ":1.4", INTERFACE, "UnInhibit", "u", &[s("1")]);
        assert_eq!(timeouts(&a), [("Uninhibit", CALL_TIMEOUT_DEFAULT)]);
        let a = b.on_message(3, ":1.4", INTERFACE, "GetActive", "", &[]);
        assert_eq!(timeouts(&a), [("GetActive", COMPANION_TIMEOUT_MS)]);
    }

    #[test]
    fn lost_name_releases_everything() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.4", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        b.on_call_done(calls(&a)[0].0, true, "41");
        b.on_message(2, ":1.5", INTERFACE, "Inhibit", "ss", &[s("b"), s("r")]);
        let a = b.release_all();
        let c = calls(&a);
        assert_eq!(c.len(), 1, "only what the session issued is released");
        assert_eq!(c[0].2, vec![s("41")]);
        assert_eq!(b.active(), 0);
    }

    #[test]
    fn taken_name_or_no_bus_or_no_upstream_means_nothing() {
        assert!(matches!(decide(Some("mate"), None, |_| Some(true), || None), Decision::Skip(_)));
        assert!(matches!(decide(Some("mate"), None, |_| None, || None), Decision::Skip(_)));
        // The name is free, but nothing can hold the screen — we do NOT take the name: otherwise
        // the client would get «inhibited» while the screen blanks.
        assert!(matches!(decide(Some("sway"), None, |_| Some(false), || None), Decision::Skip(_)));
    }

    #[test]
    fn inhibit_replies_cookie_at_once_and_forwards_idle_flag() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(7, ":1.42", INTERFACE, "Inhibit", "ss", &[s("Firefox"), s("video-playing")]);
        assert_eq!(reply_values(&a, 7), Some(vec![s("1")]));
        assert!(a.contains(&Action::Watch(s(":1.42"))));
        let c = calls(&a);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].1, "Inhibit");
        assert_eq!(c[0].2, vec![s("Firefox"), s("0"), s("video-playing"), s("8")]);
        assert_eq!(b.active(), 1);
    }

    #[test]
    fn uninhibit_releases_upstream_cookie() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.42", "", "Inhibit", "ss", &[s("mpv"), s("play")]);
        let ctx = calls(&a)[0].0;
        b.on_call_done(ctx, true, "555");
        let a = b.on_message(2, ":1.42", INTERFACE, "UnInhibit", "u", &[s("1")]);
        assert_eq!(reply_values(&a, 2), Some(vec![]));
        let c = calls(&a);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].1, c[0].2.clone()), ("Uninhibit", vec![s("555")]));
        assert!(a.contains(&Action::Unwatch(s(":1.42"))));
        assert_eq!(b.active(), 0);
    }

    #[test]
    fn uninhibit_before_session_answered_releases_late_cookie() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.5", INTERFACE, "Inhibit", "ss", &[s("vlc"), s("x")]);
        let ctx = calls(&a)[0].0;
        let a = b.on_message(2, ":1.5", INTERFACE, "UnInhibit", "u", &[s("1")]);
        assert!(calls(&a).is_empty(), "nothing to release at the session yet");
        // The session replied after the release — its cookie must be returned.
        let a = b.on_call_done(ctx, true, "99");
        let c = calls(&a);
        assert_eq!((c[0].1, c[0].2.clone()), ("Uninhibit", vec![s("99")]));
    }

    #[test]
    fn vanished_client_drops_all_its_inhibitors_only() {
        let mut b = Book::new(plan(None));
        let mut ctxs = Vec::new();
        for (tok, who) in [(1, ":1.10"), (2, ":1.10"), (3, ":1.11")] {
            let a = b.on_message(tok, who, INTERFACE, "Inhibit", "ss", &[s("app"), s("r")]);
            ctxs.push(calls(&a)[0].0);
        }
        // A second inhibition from the same client does not ask to watch again.
        for (i, ctx) in ctxs.iter().enumerate() {
            b.on_call_done(*ctx, true, &(100 + i).to_string());
        }
        let a = b.on_vanished(":1.10");
        let mut released: Vec<String> = calls(&a).into_iter().map(|c| c.2[0].clone()).collect();
        released.sort();
        assert_eq!(released, vec![s("100"), s("101")]);
        assert!(a.contains(&Action::Unwatch(s(":1.10"))));
        assert_eq!(b.active(), 1);
        assert!(b.on_vanished(":1.99").is_empty());
    }

    #[test]
    fn second_inhibit_of_same_client_does_not_watch_twice() {
        let mut b = Book::new(plan(None));
        b.on_message(1, ":1.3", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        let a = b.on_message(2, ":1.3", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        assert!(!a.iter().any(|x| matches!(x, Action::Watch(_))));
        assert_eq!(reply_values(&a, 2), Some(vec![s("2")]));
    }

    #[test]
    fn rejected_by_session_is_kept_but_not_released() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.3", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        b.on_call_done(calls(&a)[0].0, false, "org.gnome.SessionManager.GeneralError no");
        let a = b.on_message(2, ":1.3", INTERFACE, "UnInhibit", "u", &[s("1")]);
        assert!(calls(&a).is_empty());
    }

    #[test]
    fn empty_app_and_reason_get_filled() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.8", INTERFACE, "Inhibit", "ss", &[s(""), s(" ")]);
        assert_eq!(calls(&a)[0].2, vec![s(":1.8"), s("0"), s(":1.8"), s("8")]);
    }

    #[test]
    fn cookies_never_zero_and_unique() {
        let mut b = Book::new(plan(None));
        b.next_cookie = u32::MAX;
        let a = b.on_message(1, ":1.1", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        let a2 = b.on_message(2, ":1.1", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        assert_eq!(reply_values(&a, 1), Some(vec![u32::MAX.to_string()]));
        assert_eq!(reply_values(&a2, 2), Some(vec![s("1")]));
    }

    #[test]
    fn bad_calls_get_errors() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.1", INTERFACE, "Throttle", "ss", &[s("a"), s("b")]);
        assert!(matches!(&a[0], Action::ReplyError { name, .. } if *name == ERROR_UNKNOWN_METHOD));
        let a = b.on_message(2, ":1.1", INTERFACE, "Inhibit", "s", &[s("a")]);
        assert!(matches!(&a[0], Action::ReplyError { name, .. } if *name == ERROR_INVALID_ARGS));
        let a = b.on_message(3, ":1.1", "org.freedesktop.DBus.Properties", "Get", "ss", &[s("a"), s("b")]);
        assert!(matches!(&a[0], Action::ReplyError { .. }));
    }

    #[test]
    fn without_companion_answers_are_honest_defaults() {
        let mut b = Book::new(plan(None));
        assert_eq!(reply_values(&b.on_message(1, ":1.1", "", "GetActive", "", &[]), 1), Some(vec![s("false")]));
        assert_eq!(reply_values(&b.on_message(2, ":1.1", "", "GetActiveTime", "", &[]), 2), Some(vec![s("0")]));
        assert_eq!(
            reply_values(&b.on_message(3, ":1.1", "", "SetActive", "b", &[s("true")]), 3),
            Some(vec![s("false")])
        );
        let a = b.on_message(4, ":1.1", "", "Lock", "", &[]);
        assert!(matches!(&a[0], Action::ReplyError { name, .. } if *name == ERROR_NOT_SUPPORTED));
    }

    #[test]
    fn companion_answers_are_forwarded() {
        let mut b = Book::new(plan(Some(MATE_SS)));
        let a = b.on_message(1, ":1.1", "", "GetActive", "", &[]);
        let ctx = calls(&a)[0].0;
        assert_eq!(reply_values(&b.on_call_done(ctx, true, "true"), 1), Some(vec![s("true")]));
        let a = b.on_message(2, ":1.1", "", "SetActive", "b", &[s("true")]);
        let c = calls(&a);
        assert_eq!(c[0].2, vec![s("true")]);
        // mate-screensaver's SetActive returns nothing: success is `true`.
        assert_eq!(reply_values(&b.on_call_done(c[0].0, true, ""), 2), Some(vec![s("true")]));
        let a = b.on_message(3, ":1.1", "", "GetActiveTime", "", &[]);
        assert_eq!(reply_values(&b.on_call_done(calls(&a)[0].0, false, "err"), 3), Some(vec![s("0")]));
    }

    fn wl(compositor: &str) -> Option<WaylandFacts> {
        Some(WaylandFacts {
            compositor: compositor.into(),
            globals: WAYLAND_GLOBALS_NEEDED.iter().map(|g| g.to_string()).collect(),
        })
    }

    /// sway, Budgie and LXQt on labwc: the name is free, nothing on the bus to forward to
    /// — the bridge holds the screen with a Wayland inhibition.
    #[test]
    fn wayland_without_bus_target_holds_idle_inhibit() {
        let wayland_plan = Decision::Start(Plan { upstream: WAYLAND_IDLE, companion: None });
        for (de, comp) in [("sway", "sway"), ("budgie", "labwc"), ("lxqt", "labwc"), ("hyprland", "Hyprland")] {
            assert_eq!(decide(Some(de), None, |_| Some(false), || wl(comp)), wayland_plan, "{de}/{comp}");
        }
        // Budgie: budgie-session holds org.gnome.SessionManager, but swayidle
        // does not look at it — still Wayland.
        let owners = ["org.gnome.SessionManager"];
        assert_eq!(decide(Some("budgie"), None, |n| Some(owners.contains(&n)), || wl("labwc")), wayland_plan);
    }

    /// A compositor whose honoring is unverified, or the needed globals are missing, or
    /// the session is not Wayland — we do NOT take the name: native Firefox would otherwise stop
    /// at us instead of its own Wayland inhibition.
    #[test]
    fn unverified_compositor_or_missing_globals_means_no_bridge() {
        for comp in ["niri", "kwin_wayland", "magpie-wm", "wayfire", "river", ""] {
            let d = decide(Some("lxqt"), None, |_| Some(false), || wl(comp));
            assert!(matches!(d, Decision::Skip(_)), "{comp}: {d:?}");
        }
        let partial = || {
            Some(WaylandFacts { compositor: "sway".into(), globals: vec!["wl_compositor".into(), "wl_shm".into()] })
        };
        let Decision::Skip(why) = decide(Some("sway"), None, |_| Some(false), partial) else { panic!() };
        assert!(why.contains("zwp_idle_inhibit_manager_v1"), "{why}");
        assert!(matches!(decide(Some("budgie"), None, |_| Some(false), || None), Decision::Skip(_)));
    }

    /// There is something to forward to on the bus (XFCE on Wayland) — the bus takes priority,
    /// we do not even ask the compositor.
    #[test]
    fn bus_upstream_wins_over_wayland() {
        let owners = ["org.freedesktop.PowerManagement"];
        let d = decide(Some("xfce"), None, |n| Some(owners.contains(&n)), || panic!("Wayland was asked"));
        assert!(matches!(d, Decision::Start(Plan { upstream: POWER_MANAGEMENT, .. })), "{d:?}");
    }

    /// niri holds the name itself; a taken name and an environment owner — we do not ask
    /// Wayland at all.
    #[test]
    fn own_provider_or_taken_name_never_asks_wayland() {
        assert!(matches!(decide(Some("niri"), None, |_| Some(false), || panic!()), Decision::Skip(_)));
        assert!(matches!(decide(Some("sway"), None, |_| Some(true), || panic!()), Decision::Skip(_)));
    }

    fn holds(actions: &[Action]) -> Vec<bool> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::HoldIdle(on) => Some(*on),
                _ => None,
            })
            .collect()
    }

    /// One Wayland inhibition for all inhibitions: «hold» with the first, «release»
    /// with the last, not a single call to the bus.
    #[test]
    fn wayland_hold_follows_first_and_last_inhibitor() {
        let mut b = Book::new(Plan { upstream: WAYLAND_IDLE, companion: None });
        let a = b.on_message(1, ":1.20", INTERFACE, "Inhibit", "ss", &[s("firefox"), s("video")]);
        assert_eq!(reply_values(&a, 1), Some(vec![s("1")]));
        assert_eq!(holds(&a), [true]);
        assert!(calls(&a).is_empty(), "{a:?}");
        let a = b.on_message(2, ":1.21", INTERFACE, "Inhibit", "ss", &[s("steam"), s("game")]);
        assert!(holds(&a).is_empty(), "the second inhibition keeps the same inhibitor");
        let a = b.on_message(3, ":1.20", INTERFACE, "UnInhibit", "u", &[s("1")]);
        assert!(holds(&a).is_empty() && calls(&a).is_empty(), "{a:?}");
        // The last holder left the bus — release.
        let a = b.on_vanished(":1.21");
        assert_eq!(holds(&a), [false]);
        assert_eq!(b.active(), 0);
        // And again from scratch.
        let a = b.on_message(4, ":1.22", INTERFACE, "Inhibit", "ss", &[s("mpv"), s("play")]);
        assert_eq!(holds(&a), [true]);
        let a = b.on_message(5, ":1.22", INTERFACE, "UnInhibit", "u", &[s("3")]);
        assert_eq!(holds(&a), [false]);
    }

    /// The name was taken — the Wayland inhibition is released together with all inhibitions.
    #[test]
    fn wayland_hold_released_when_bridge_steps_down() {
        let mut b = Book::new(Plan { upstream: WAYLAND_IDLE, companion: None });
        b.on_message(1, ":1.30", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        let a = b.release_all();
        assert_eq!(holds(&a), [false]);
        assert!(calls(&a).is_empty());
        assert!(holds(&b.release_all()).is_empty(), "not released twice");
    }

    #[test]
    fn introspect_call_gets_node() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.1", INTROSPECTABLE, "Introspect", "", &[]);
        let v = reply_values(&a, 1).expect("reply");
        assert!(v[0].contains("<node>") && v[0].contains("name=\"org.freedesktop.ScreenSaver\""));
    }

    #[test]
    fn introspection_declares_only_implemented_methods() {
        for m in ["Inhibit", "UnInhibit", "GetActive", "SetActive", "GetActiveTime", "SimulateUserActivity", "Lock"] {
            assert!(INTROSPECTION_XML.contains(&format!("<method name=\"{m}\">")), "{m}");
        }
        for m in ["Throttle", "GetSessionIdleTime", "ActiveChanged"] {
            assert!(!INTROSPECTION_XML.contains(m), "{m} declared but not implemented");
        }
    }
}
