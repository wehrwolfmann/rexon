//! Autologin and login screen sound when switching the display manager.
//!
//! Defect of 15.11.2026: the `switch-dm` helper ("Make default") changed the
//! login screen, and autologin silently disappeared — every display manager
//! keeps this setting in its own place, and Rexon did not carry it over. Here
//! are the pure decisions: where each login screen keeps autologin, what is
//! currently enabled in the previous one, what to write and where for the new
//! one, and what to tell the person honestly BEFORE switching if it cannot be
//! carried over. The helper (`privilege::helper::switch_dm`) executes it in
//! the same plan and with the same rollback.
//!
//! Where autologin lives (according to the man pages and the strings of the
//! programs themselves on the machine):
//! * SDDM — `[Autologin] User= Session= Relogin=`; files
//!   `/usr/lib/sddm/sddm.conf.d/*.conf`, `/etc/sddm.conf.d/*.conf`, and
//!   `/etc/sddm.conf` last (later overrides earlier).
//! * Plasma Login Manager — the same keys; `/usr/lib/plasmalogin/plasmalogin.conf.d`,
//!   `/etc/plasmalogin.conf.d`, and `/etc/plasmalogin.conf` last (the System
//!   Settings module rewrites it entirely, so Rexon puts its own file into the
//!   directory).
//! * DDM 0.4 (Deepin) — `[Autologin] User= Session=` (without `Relogin`);
//!   `/usr/lib/ddm/ddm.conf.d`, `/etc/ddm.conf.d`, and `/etc/ddm.conf` last.
//! * GDM — `[daemon] AutomaticLoginEnable= AutomaticLogin=` in a single file
//!   (`/etc/gdm/custom.conf`; Debian's `gdm3` uses `/etc/gdm3/daemon.conf`,
//!   Ubuntu `/etc/gdm3/custom.conf`). GDM's autologin desktop cannot be set:
//!   it takes the last one selected in AccountsService.
//! * LightDM — `[Seat:*] autologin-user= autologin-session=` (the old group
//!   name is `[SeatDefaults]`, the specific `[Seat:seat0]` is stronger);
//!   `/etc/lightdm/lightdm.conf` is read last. On Arch the
//!   `lightdm-autologin` PAM service lets in without a password only members
//!   of the `autologin` group.
//! * greetd (and cosmic-greeter on top of it) — `[initial_session] user=
//!   command=`: a desktop launch command, not a session name. We read it but
//!   do not write it.
//!
//! Login screen sound: the greeter has its own user and its own PipeWire.
//! Without saved state, WirePlumber sets the hardware mixer to the default
//! volume (40 %), and after login the person's volume drops. The workaround is
//! masking five sound units in `~/.config/systemd/user` of the greeter user
//! (links to `/dev/null`). If the previous login screen has such masks, the
//! helper sets them for the new one's user too.

use crate::i18n::trf;
use crate::login_rescue::ini_value;
use crate::system_plan::{rewrite_ini_values, sddm_conf_read_order, SessionEdit, SessionKey};

/// How a login screen stores autologin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// `[Autologin] User= Session=`, with or without `Relogin=`.
    Autologin { relogin: bool },
    /// `[daemon] AutomaticLoginEnable= AutomaticLogin=` (GDM).
    Gdm,
    /// `[Seat:*] autologin-user= autologin-session=` (LightDM).
    LightDm,
    /// `[initial_session] user= command=` (greetd), read-only.
    Greetd,
}

/// Where one login screen keeps autologin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutologinSpec {
    /// The unit without `.service` (as in `system_plan::LOGIN_MANAGER_SPECS`).
    pub unit: &'static str,
    pub layout: Layout,
    /// `*.conf` directories in reading order (within each, by name).
    pub dirs: &'static [&'static str],
    /// Files after the directories. For `Autologin`/`LightDm` all of them are read
    /// in order (later overrides earlier); for `Gdm`/`Greetd` the first existing
    /// one (the program reads a single file).
    pub files: &'static [&'static str],
    /// Rexon's own file: a key that is missing from the other `/etc` files is
    /// appended there. Empty means the file the program reads.
    pub own: &'static str,
    /// The autologin PAM service.
    pub pam: &'static str,
}

/// Login screens for which Rexon knows where autologin lives.
pub const AUTOLOGIN_SPECS: &[AutologinSpec] = &[
    AutologinSpec {
        unit: "plasmalogin",
        layout: Layout::Autologin { relogin: true },
        dirs: &["/usr/lib/plasmalogin/plasmalogin.conf.d", "/etc/plasmalogin.conf.d"],
        files: &["/etc/plasmalogin.conf"],
        own: "/etc/plasmalogin.conf.d/zz-rexon-autologin.conf",
        pam: "plasmalogin-autologin",
    },
    AutologinSpec {
        unit: "sddm",
        layout: Layout::Autologin { relogin: true },
        dirs: &["/usr/lib/sddm/sddm.conf.d", "/etc/sddm.conf.d"],
        files: &["/etc/sddm.conf"],
        own: "/etc/sddm.conf.d/zz-rexon-autologin.conf",
        pam: "sddm-autologin",
    },
    AutologinSpec {
        unit: "ddm",
        layout: Layout::Autologin { relogin: false },
        dirs: &["/usr/lib/ddm/ddm.conf.d", "/etc/ddm.conf.d"],
        files: &["/etc/ddm.conf"],
        own: "/etc/ddm.conf.d/zz-rexon-autologin.conf",
        pam: "ddm-autologin",
    },
    AutologinSpec {
        unit: "gdm",
        layout: Layout::Gdm,
        dirs: &[],
        files: &["/etc/gdm/custom.conf"],
        own: "",
        pam: "gdm-autologin",
    },
    AutologinSpec {
        unit: "gdm3",
        layout: Layout::Gdm,
        dirs: &[],
        files: &["/etc/gdm3/daemon.conf", "/etc/gdm3/custom.conf"],
        own: "",
        pam: "gdm-autologin",
    },
    AutologinSpec {
        unit: "lightdm",
        layout: Layout::LightDm,
        dirs: &[
            "/usr/share/lightdm/lightdm.conf.d",
            "/etc/xdg/lightdm/lightdm.conf.d",
            "/etc/lightdm/lightdm.conf.d",
        ],
        files: &["/etc/lightdm/lightdm.conf"],
        // The main file is read last, so nothing overrides a key in it.
        own: "",
        pam: "lightdm-autologin",
    },
    AutologinSpec {
        unit: "greetd",
        layout: Layout::Greetd,
        dirs: &[],
        files: &[crate::system_plan::GREETD_DEFAULT_CONFIG],
        own: "",
        pam: "greetd",
    },
    AutologinSpec {
        unit: "cosmic-greeter",
        layout: Layout::Greetd,
        dirs: &[],
        files: &["/etc/greetd/cosmic-greeter.toml", crate::system_plan::GREETD_DEFAULT_CONFIG],
        own: "",
        pam: "greetd",
    },
];

/// Where a login screen keeps autologin; `None` means Rexon does not know it.
pub fn autologin_spec(unit: &str) -> Option<&'static AutologinSpec> {
    let unit = unit.trim();
    AUTOLOGIN_SPECS.iter().find(|s| s.unit == unit)
}

/// An enabled autologin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Autologin {
    pub user: String,
    /// The session file name without `.desktop`; empty means the last one selected.
    pub session: String,
    /// Log in again after logging out (`Relogin=true`; game mode relies on it).
    pub relogin: bool,
}

/// The login screen's autologin right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutologinState {
    Off,
    On(Autologin),
    /// Rexon does not know where this login screen keeps autologin.
    Unknown,
}

type Read<'a> = &'a dyn Fn(&str) -> Option<String>;
type List<'a> = &'a dyn Fn(&str) -> Vec<String>;

/// Settings groups in ascending order of strength.
fn groups(layout: Layout) -> &'static [&'static str] {
    match layout {
        Layout::Autologin { .. } => &["Autologin"],
        Layout::Gdm => &["daemon"],
        Layout::LightDm => &["SeatDefaults", "Seat:*", "Seat:seat0"],
        Layout::Greetd => &["initial_session"],
    }
}

/// The group Rexon appends the key to.
fn write_group(layout: Layout) -> &'static str {
    match layout {
        Layout::LightDm => "Seat:*",
        other => groups(other)[0],
    }
}

/// The files the login screen reads, in its order.
pub fn config_files(spec: &AutologinSpec, read: Read<'_>, list: List<'_>) -> Vec<String> {
    match spec.layout {
        Layout::Gdm | Layout::Greetd => {
            let first = spec.files.iter().find(|f| read(f).is_some()).or(spec.files.first());
            first.map(|f| vec![f.to_string()]).unwrap_or_default()
        }
        _ => {
            let mut out = Vec::new();
            for dir in spec.dirs {
                out.extend(sddm_conf_read_order(&list(dir)).iter().map(|n| format!("{dir}/{n}")));
            }
            out.extend(spec.files.iter().map(|f| f.to_string()));
            out
        }
    }
}

/// The resulting value of a key for the login screen (a stronger group overrides a weaker one).
fn effective(spec: &AutologinSpec, key: &str, texts: &[String]) -> Option<String> {
    groups(spec.layout).iter().filter_map(|g| ini_value(texts, g, key)).last()
}

fn truthy(v: Option<&str>) -> bool {
    matches!(v.map(|s| s.trim().to_ascii_lowercase()).as_deref(), Some("true" | "1" | "yes"))
}

/// The session name without the directory and `.desktop`.
pub fn session_name(value: &str) -> String {
    crate::update_guard::session_file_name(value)
}

/// The autologin the login screen will perform with these settings.
pub fn read_autologin(unit: &str, read: Read<'_>, list: List<'_>) -> AutologinState {
    let Some(spec) = autologin_spec(unit) else {
        return AutologinState::Unknown;
    };
    let files = config_files(spec, read, list);
    let texts: Vec<String> = files.iter().filter_map(|p| read(p)).collect();
    let get = |k: &str| effective(spec, k, &texts).map(|v| v.trim().to_string()).unwrap_or_default();
    let (user, session, relogin) = match spec.layout {
        Layout::Autologin { relogin } => (get("User"), session_name(&get("Session")), relogin && truthy(Some(&get("Relogin")))),
        Layout::Gdm => {
            let user = if truthy(Some(&get("AutomaticLoginEnable"))) { get("AutomaticLogin") } else { String::new() };
            (user, String::new(), false)
        }
        Layout::LightDm => (get("autologin-user"), session_name(&get("autologin-session")), false),
        Layout::Greetd => {
            let text = texts.first().map(String::as_str).unwrap_or_default();
            let user = crate::system_plan::toml_string_value(text, "initial_session", "user").unwrap_or_default();
            (user.trim().to_string(), String::new(), false)
        }
    };
    if user.is_empty() {
        AutologinState::Off
    } else {
        AutologinState::On(Autologin { user, session, relogin })
    }
}

/// A flag value: empty and `false` are the same; not a flag gives `None`.
fn flag(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "" | "false" | "0" | "no" => Some(false),
        "true" | "1" | "yes" => Some(true),
        _ => None,
    }
}

/// Whether the values are the same for the login screen: a missing key equals
/// empty, flags are compared case-insensitively (`True`/`true`), names strictly.
fn same(a: Option<&str>, b: &str) -> bool {
    let a = a.unwrap_or("").trim();
    let b = b.trim();
    a == b || matches!((flag(a), flag(b)), (Some(x), Some(y)) if x == y)
}

/// The keys and values to be achieved in the target login screen.
fn desired(layout: Layout, want: Option<&Autologin>) -> Vec<(&'static str, String)> {
    match (layout, want) {
        (Layout::Autologin { relogin }, Some(a)) => {
            let mut v = vec![
                ("User", a.user.clone()),
                ("Session", if a.session.is_empty() { String::new() } else { format!("{}.desktop", a.session) }),
            ];
            if relogin {
                v.push(("Relogin", (if a.relogin { "true" } else { "false" }).to_string()));
            }
            v
        }
        (Layout::Autologin { .. }, None) => vec![("User", String::new())],
        (Layout::Gdm, Some(a)) => vec![("AutomaticLoginEnable", "True".into()), ("AutomaticLogin", a.user.clone())],
        (Layout::Gdm, None) => vec![("AutomaticLoginEnable", "False".into())],
        (Layout::LightDm, Some(a)) => {
            vec![("autologin-user", a.user.clone()), ("autologin-session", a.session.clone())]
        }
        (Layout::LightDm, None) => vec![("autologin-user", String::new())],
        (Layout::Greetd, _) => Vec::new(),
    }
}

/// File edits after which the resulting key values of login screen `spec`
/// equal `want`. Existing lines in `/etc` files are changed first (other lines
/// stay byte for byte), missing keys are appended to our own file. `None`
/// means the result would differ anyway (the value is set by a package file
/// over `/etc`), and then nothing is touched.
fn plan_keys(spec: &AutologinSpec, want: &[(&'static str, String)], read: Read<'_>, list: List<'_>) -> Option<Vec<SessionEdit>> {
    let files = config_files(spec, read, list);
    let mut edits: Vec<SessionEdit> = Vec::new();
    let current = |edits: &[SessionEdit], p: &str| edits.iter().find(|e| e.path == p).map(|e| e.text.clone()).or_else(|| read(p));
    let put = |edits: &mut Vec<SessionEdit>, path: &str, text: String| match edits.iter_mut().find(|e| e.path == path) {
        Some(e) => e.text = text,
        None => edits.push(SessionEdit { path: path.to_string(), text }),
    };
    for (key, value) in want {
        let keys: Vec<SessionKey> = groups(spec.layout).iter().map(|g| SessionKey { group: g, key: *key }).collect();
        for path in files.iter().filter(|p| p.starts_with("/etc/")) {
            let Some(text) = current(&edits, path) else { continue };
            if let Some(new) = rewrite_ini_values(&text, &keys, &|v| !same(Some(v), value), &|_| value.clone()) {
                put(&mut edits, path, new);
            }
        }
    }
    let own = if spec.own.is_empty() { files.last().cloned().unwrap_or_default() } else { spec.own.to_string() };
    if !own.starts_with("/etc/") {
        return None;
    }
    let all_files = |edits: &[SessionEdit]| {
        let mut f = files.clone();
        for e in edits {
            if !f.contains(&e.path) {
                // Our own directory file goes into its place by name.
                let pos = f.iter().position(|x| x.as_str() > e.path.as_str() && x.rsplit_once('/').map(|p| p.0) == e.path.rsplit_once('/').map(|p| p.0));
                match pos {
                    Some(i) => f.insert(i, e.path.clone()),
                    None => {
                        // The directory is not in the list yet: before the files that come after the directories.
                        let i = f.iter().position(|x| spec.files.contains(&x.as_str())).unwrap_or(f.len());
                        f.insert(i, e.path.clone());
                    }
                }
            }
        }
        f
    };
    let value_now = |edits: &[SessionEdit], key: &str| {
        let texts: Vec<String> = all_files(edits).iter().filter_map(|p| current(edits, p)).collect();
        effective(spec, key, &texts)
    };
    for (key, value) in want {
        if same(value_now(&edits, key).as_deref(), value) {
            continue;
        }
        let base = current(&edits, &own);
        let group = format!("[{}]", write_group(spec.layout));
        let text = crate::defaults::set_ini_value(base.as_deref(), &group, key, value);
        put(&mut edits, &own, text);
    }
    let ok = want.iter().all(|(k, v)| same(value_now(&edits, k).as_deref(), v));
    ok.then_some(edits)
}

/// Observations about the target login screen needed for the plan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetFacts {
    /// The text of the target login screen's autologin PAM service.
    pub pam_autologin: Option<String>,
    /// Group members (from `getent group`) if PAM requires a group; `None` means
    /// there is no such group.
    pub group_members: Option<Vec<String>>,
    /// The greeter user of the target login screen (`LOGIN_MANAGER_SPECS`).
    pub greeter_user: Option<String>,
}

/// The group without which the autologin PAM service will ask for a password
/// (`pam_succeed_if.so … user ingroup <group>` in an `auth` line).
pub fn pam_autologin_group(pam: &str) -> Option<String> {
    pam.lines().map(str::trim).filter(|l| !l.starts_with('#') && l.starts_with("auth")).find_map(|l| {
        if !l.contains("pam_succeed_if") {
            return None;
        }
        let words: Vec<&str> = l.split_whitespace().collect();
        let i = words.iter().position(|w| *w == "ingroup")?;
        let g = words.get(i + 1)?;
        // Several groups separated by ":" make no sense for autologin; only one.
        (!g.contains(':')).then(|| g.to_string())
    })
}

/// What to tell the person about carrying it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// The user's autologin is carried over.
    Moved { user: String },
    /// No session is set: the last one selected will open.
    LastSession,
    /// GDM does not set the autologin desktop.
    GdmSession,
    /// The new one cannot log in again after logout.
    ReloginLost,
    /// The user is added to the group for PAM.
    Group { user: String, group: String },
    /// Autologin previously configured in the target is turned off.
    TurnedOff { user: String },
    /// Cannot carry over: the target's autologin is a launch command (greetd).
    LostCommand { user: String },
    /// Cannot carry over: Rexon does not know where the target keeps autologin.
    LostUnknown { user: String },
    /// Cannot carry over: the value is set by a package file.
    LostElsewhere { user: String },
    /// The autologin user is the greeter user itself.
    LostGreeterUser { user: String },
}

/// The decision about carrying autologin over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    /// What to write (empty means nothing).
    pub edits: Vec<SessionEdit>,
    /// Add the user to this group: `(user, group, the group does not exist yet)`.
    pub group: Option<(String, String, bool)>,
    pub notes: Vec<Note>,
}

impl Transfer {
    fn nothing() -> Self {
        Transfer { edits: Vec::new(), group: None, notes: Vec::new() }
    }
    /// Nothing to write and nothing to say.
    pub fn is_nothing(&self) -> bool {
        self.edits.is_empty() && self.group.is_none() && self.notes.is_empty()
    }
    /// Autologin will be lost.
    pub fn is_lost(&self) -> bool {
        self.notes.iter().any(|n| {
            matches!(n, Note::LostCommand { .. } | Note::LostUnknown { .. } | Note::LostElsewhere { .. } | Note::LostGreeterUser { .. })
        })
    }
}

/// What to do with autologin when switching the login screen `from` → `to`.
///
/// Carrying over means "as in the previous one": if there was autologin, the
/// new one gets the same user, the same session and the same `Relogin`; if
/// there was none, it is turned off in the new one (otherwise after switching
/// the login screen the computer would suddenly start logging in by itself).
/// The previous login screen is not touched: if a rollback brings it back, its
/// autologin comes back too.
pub fn plan_transfer(from: &str, to: &str, read: Read<'_>, list: List<'_>, facts: &TargetFacts) -> Transfer {
    let source = read_autologin(from, read, list);
    let Some(spec) = autologin_spec(to) else {
        return match source {
            AutologinState::On(a) => Transfer { notes: vec![Note::LostUnknown { user: a.user }], ..Transfer::nothing() },
            _ => Transfer::nothing(),
        };
    };
    let target = read_autologin(to, read, list);
    let want = match source {
        AutologinState::Unknown => return Transfer::nothing(),
        AutologinState::Off => match target {
            AutologinState::On(t) if spec.layout != Layout::Greetd => {
                let Some(edits) = plan_keys(spec, &desired(spec.layout, None), read, list) else {
                    return Transfer::nothing();
                };
                return Transfer { edits, group: None, notes: vec![Note::TurnedOff { user: t.user }] };
            }
            _ => return Transfer::nothing(),
        },
        AutologinState::On(a) => a,
    };
    if spec.layout == Layout::Greetd {
        return Transfer { notes: vec![Note::LostCommand { user: want.user }], ..Transfer::nothing() };
    }
    if facts.greeter_user.as_deref() == Some(want.user.as_str()) {
        return Transfer { notes: vec![Note::LostGreeterUser { user: want.user }], ..Transfer::nothing() };
    }
    let Some(edits) = plan_keys(spec, &desired(spec.layout, Some(&want)), read, list) else {
        return Transfer { notes: vec![Note::LostElsewhere { user: want.user }], ..Transfer::nothing() };
    };
    let group = facts.pam_autologin.as_deref().and_then(pam_autologin_group).and_then(|g| match &facts.group_members {
        Some(m) if m.iter().any(|u| *u == want.user) => None,
        Some(_) => Some((want.user.clone(), g, false)),
        None => Some((want.user.clone(), g, true)),
    });
    // Everything is already in place: the new one has the same autologin.
    if edits.is_empty() && group.is_none() {
        return Transfer::nothing();
    }
    let mut notes = vec![Note::Moved { user: want.user.clone() }];
    if spec.layout == Layout::Gdm {
        if !want.session.is_empty() {
            notes.push(Note::GdmSession);
        }
    } else if want.session.is_empty() {
        notes.push(Note::LastSession);
    }
    if want.relogin && !matches!(spec.layout, Layout::Autologin { relogin: true }) {
        notes.push(Note::ReloginLost);
    }
    if let Some((u, g, _)) = &group {
        notes.push(Note::Group { user: u.clone(), group: g.clone() });
    }
    Transfer { edits, group, notes }
}

/// The text for the "Make default" confirmation dialog, BEFORE the password.
/// `None` means nothing changes about autologin.
pub fn transfer_note(to_label: &str, t: &Transfer) -> Option<String> {
    let mut parts = Vec::new();
    for n in &t.notes {
        parts.push(match n {
            // TRANSLATORS: %1 is a user name, %2 a login screen name.
            Note::Moved { user } => trf("Automatic sign-in for %1 moves to %2 as well.", &[user, &to_label]),
            // TRANSLATORS: %1 is a login screen name.
            Note::LastSession => trf("%1 will open the desktop chosen at the last sign-in.", &[&to_label]),
            Note::GdmSession => trf(
                // TRANSLATORS: %1 is a login screen name (GDM).
                "%1 can't be told which desktop to open on automatic sign-in: it opens the one chosen at the last sign-in through %1.",
                &[&to_label],
            ),
            Note::ReloginLost => trf(
                // TRANSLATORS: %1 is a login screen name. Game mode is Rexon's "like Steam Machine" mode.
                "%1 can't sign you back in by itself after you sign out, and game mode relies on that.",
                &[&to_label],
            ),
            Note::Group { user, group } => trf(
                // TRANSLATORS: %1 is a login screen name, %2 a user name, %3 a system group name.
                "To let %1 sign in without a password, Rexon adds %2 to the group %3.",
                &[&to_label, user, group],
            ),
            Note::TurnedOff { user } => trf(
                // TRANSLATORS: %1 is a login screen name, %2 a user name.
                "%1 has automatic sign-in for %2 turned on. You don't sign in automatically now, so Rexon turns it off there too.",
                &[&to_label, user],
            ),
            Note::LostCommand { user } => trf(
                // TRANSLATORS: %1 is a user name, %2 a login screen name.
                "Automatic sign-in for %1 will be lost: %2 sets it as the desktop's start command in its own settings, and Rexon doesn't write that. Turn it on in the settings of %2 after the restart.",
                &[user, &to_label],
            ),
            Note::LostUnknown { user } => trf(
                // TRANSLATORS: %1 is a user name, %2 a login screen name.
                "Automatic sign-in for %1 will be lost: Rexon doesn't know where %2 keeps this setting.",
                &[user, &to_label],
            ),
            Note::LostElsewhere { user } => trf(
                // TRANSLATORS: %1 is a user name, %2 a login screen name.
                "Automatic sign-in for %1 will be lost: %2 takes this setting from a file of its package, which Rexon doesn't change.",
                &[user, &to_label],
            ),
            Note::LostGreeterUser { user } => trf(
                // TRANSLATORS: %1 is a user name, %2 a login screen name.
                "Automatic sign-in for %1 will be lost: %2 runs its own screen as this user.",
                &[user, &to_label],
            ),
        });
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// After switching: is the new one's autologin as intended?
pub fn transfer_result_note(to_unit: &str, to_label: &str, want: &AutologinState, read: Read<'_>, list: List<'_>) -> Option<String> {
    let AutologinState::On(a) = want else { return None };
    match read_autologin(to_unit, read, list) {
        AutologinState::On(now) if now.user == a.user => {
            // TRANSLATORS: %1 is a user name, %2 a login screen name.
            Some(trf("Automatic sign-in for %1 moved to %2.", &[&a.user, &to_label]))
        }
        // TRANSLATORS: %1 is a login screen name.
        _ => Some(trf("Automatic sign-in wasn't moved to %1 — turn it on in the settings of %1.", &[&to_label])),
    }
}

// ── Login screen sound ───────────────────────────────────────────────────

/// The greeter user's sound units that the "volume drops to 40 % after login"
/// workaround turns off.
pub const GREETER_AUDIO_UNITS: &[&str] =
    &["pipewire.service", "pipewire.socket", "pipewire-pulse.service", "pipewire-pulse.socket", "wireplumber.service"];

/// The user unit directory in the greeter user's home.
pub fn greeter_user_unit_dir(home: &str) -> String {
    format!("{}/.config/systemd/user", home.trim_end_matches('/'))
}

/// Whether a unit is masked: a link to `/dev/null` or an empty file (that is
/// how systemd sees it: systemd.unit(5)).
pub fn unit_masked(link_target: Option<&str>, text: Option<&str>) -> bool {
    link_target == Some("/dev/null") || text.is_some_and(|t| t.is_empty())
}

/// Units to mask for the new one: the previous one has sound turned off (at
/// least one unit), and the new one has no such unit at all yet (we do not
/// overwrite the greeter user's own setting).
pub fn audio_masks_to_add(old_masked: &[&str], new_present: &dyn Fn(&str) -> bool) -> Vec<&'static str> {
    if old_masked.is_empty() {
        return Vec::new();
    }
    GREETER_AUDIO_UNITS.iter().copied().filter(|u| !new_present(u)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fs(files: &[(&str, &str)]) -> BTreeMap<String, String> {
        files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()
    }

    fn io(m: &BTreeMap<String, String>) -> (impl Fn(&str) -> Option<String> + '_, impl Fn(&str) -> Vec<String> + '_) {
        let read = move |p: &str| m.get(p).cloned();
        let list = move |d: &str| {
            let pre = format!("{d}/");
            m.keys().filter_map(|k| k.strip_prefix(&pre)).filter(|r| !r.contains('/')).map(str::to_string).collect()
        };
        (read, list)
    }

    fn on(user: &str, session: &str, relogin: bool) -> AutologinState {
        AutologinState::On(Autologin { user: user.into(), session: session.into(), relogin })
    }

    /// Applies the edits on top of the "disk".
    fn apply(m: &BTreeMap<String, String>, t: &Transfer) -> BTreeMap<String, String> {
        let mut out = m.clone();
        for e in &t.edits {
            out.insert(e.path.clone(), e.text.clone());
        }
        out
    }

    #[test]
    fn читает_автовход_каждого_экрана_входа() {
        let m = fs(&[
            ("/usr/lib/sddm/sddm.conf.d/default.conf", "[Autologin]\nRelogin=false\nSession=\nUser=\n"),
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nRelogin=true\nSession=plasma\nUser=alice\n"),
            ("/etc/sddm.conf", "[Autologin]\nSession=gamescope-session-steam.desktop\n"),
            ("/etc/plasmalogin.conf", "[Autologin]\nSession=plasma\n"),
            ("/etc/ddm.conf.d/a.conf", "[Autologin]\nUser=bob\nSession=/usr/share/wayland-sessions/treeland.desktop\n"),
            ("/etc/gdm/custom.conf", "[daemon]\nAutomaticLoginEnable=True\nAutomaticLogin=carol\n"),
            ("/etc/gdm3/custom.conf", "[daemon]\nAutomaticLoginEnable=false\nAutomaticLogin=dave\n"),
            ("/usr/share/lightdm/lightdm.conf.d/50-x.conf", "[SeatDefaults]\nautologin-user=eve\n"),
            ("/etc/lightdm/lightdm.conf", "[Seat:*]\n#autologin-user=\nautologin-session=xfce\n"),
            ("/etc/greetd/config.toml", "[default_session]\ncommand = \"tuigreet\"\n[initial_session]\nuser = \"frank\"\ncommand = \"sway\"\n"),
        ]);
        let (read, list) = io(&m);
        assert_eq!(read_autologin("sddm", &read, &list), on("alice", "gamescope-session-steam", true));
        assert_eq!(read_autologin("plasmalogin", &read, &list), AutologinState::Off);
        assert_eq!(read_autologin("ddm", &read, &list), on("bob", "treeland", false));
        assert_eq!(read_autologin("gdm", &read, &list), on("carol", "", false));
        assert_eq!(read_autologin("gdm3", &read, &list), AutologinState::Off, "AutomaticLoginEnable=false");
        assert_eq!(read_autologin("lightdm", &read, &list), on("eve", "xfce", false));
        assert_eq!(read_autologin("greetd", &read, &list), on("frank", "", false));
        assert_eq!(read_autologin("ly", &read, &list), AutologinState::Unknown);
    }

    #[test]
    fn lightdm_частная_группа_места_сильнее() {
        let m = fs(&[(
            "/etc/lightdm/lightdm.conf",
            "[Seat:seat0]\nautologin-user=\n[Seat:*]\nautologin-user=eve\n",
        )]);
        let (read, list) = io(&m);
        assert_eq!(read_autologin("lightdm", &read, &list), AutologinState::Off);
    }

    /// A machine like Wehrwolfmann's: Plasma Login with autologin → SDDM.
    #[test]
    fn plasma_login_в_sddm_переносит_пользователя_сеанс_и_relogin() {
        let m = fs(&[
            ("/etc/plasmalogin.conf", "[Autologin]\nRelogin=true\nSession=plasma\nUser=alice\n"),
            ("/usr/lib/sddm/sddm.conf.d/default.conf", "[Autologin]\nRelogin=false\nSession=\nUser=\n"),
            ("/etc/sddm.conf", "[Autologin]\nSession=plasmax11\n"),
            ("/etc/sddm.conf.d/kde_settings.conf", "[Theme]\nCurrent=breeze\n"),
        ]);
        let (read, list) = io(&m);
        let t = plan_transfer("plasmalogin", "sddm", &read, &list, &TargetFacts::default());
        assert!(!t.is_lost());
        assert_eq!(t.notes, vec![Note::Moved { user: "alice".into() }]);
        // The foreign Session= line in /etc/sddm.conf is rewritten in place, the rest goes into our own file.
        let sddm_conf = t.edits.iter().find(|e| e.path == "/etc/sddm.conf").expect("sddm.conf");
        assert_eq!(sddm_conf.text, "[Autologin]\nSession=plasma.desktop\n");
        let own = t.edits.iter().find(|e| e.path == "/etc/sddm.conf.d/zz-rexon-autologin.conf").expect("свой файл");
        assert!(own.text.contains("User=alice") && own.text.contains("Relogin=true"), "{}", own.text);
        assert!(t.edits.iter().all(|e| e.path.starts_with("/etc/")), "файлы пакета не правятся");
        // The result is read the same way SDDM reads it.
        let after = apply(&m, &t);
        let (read2, list2) = io(&after);
        assert_eq!(read_autologin("sddm", &read2, &list2), on("alice", "plasma", true));
        // A repeated plan: nothing to do.
        assert!(plan_transfer("plasmalogin", "sddm", &read2, &list2, &TargetFacts::default()).is_nothing());
    }

    #[test]
    fn в_gdm_пишется_custom_conf_и_честно_про_стол() {
        let m = fs(&[
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nSession=plasma\nUser=alice\n"),
            ("/etc/gdm/custom.conf", "# GDM configuration storage\n\n[daemon]\n\n[security]\n"),
        ]);
        let (read, list) = io(&m);
        let t = plan_transfer("sddm", "gdm", &read, &list, &TargetFacts::default());
        assert_eq!(t.notes, vec![Note::Moved { user: "alice".into() }, Note::GdmSession]);
        assert_eq!(t.edits.len(), 1);
        assert_eq!(t.edits[0].path, "/etc/gdm/custom.conf");
        let after = apply(&m, &t);
        let text = &after["/etc/gdm/custom.conf"];
        assert!(text.starts_with("# GDM configuration storage\n"), "комментарий цел: {text}");
        assert!(text.contains("[security]"));
        let (read2, list2) = io(&after);
        assert_eq!(read_autologin("gdm", &read2, &list2), on("alice", "", false));
        let note = transfer_note("GDM", &t).unwrap();
        assert!(note.contains("alice") && note.contains("GDM"), "{note}");
    }

    #[test]
    fn в_lightdm_пишется_главный_файл_и_группа_autologin() {
        let m = fs(&[
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nRelogin=true\nSession=plasma\nUser=alice\n"),
            ("/etc/lightdm/lightdm.conf", "[LightDM]\n[Seat:*]\n#autologin-user=\n[XDMCPServer]\n"),
        ]);
        let (read, list) = io(&m);
        let pam = "auth        required    pam_env.so\nauth        [success=1 default=ignore]  pam_succeed_if.so user ingroup autologin\n";
        let facts = TargetFacts { pam_autologin: Some(pam.into()), group_members: None, greeter_user: Some("lightdm".into()) };
        let t = plan_transfer("sddm", "lightdm", &read, &list, &facts);
        assert_eq!(t.group, Some(("alice".into(), "autologin".into(), true)), "группы нет — создать");
        assert!(t.notes.contains(&Note::ReloginLost));
        assert!(t.notes.contains(&Note::Group { user: "alice".into(), group: "autologin".into() }));
        let after = apply(&m, &t);
        let text = &after["/etc/lightdm/lightdm.conf"];
        assert!(text.contains("#autologin-user=\n"), "закомментированная строка не тронута: {text}");
        let seat = text.find("[Seat:*]").unwrap();
        let xdmcp = text.find("[XDMCPServer]").unwrap();
        let user = text.find("autologin-user=alice").unwrap();
        assert!(seat < user && user < xdmcp, "ключ в своей группе: {text}");
        let (read2, list2) = io(&after);
        assert_eq!(read_autologin("lightdm", &read2, &list2), on("alice", "plasma", false));
        // Already in the group: the group is not touched.
        let member = TargetFacts { group_members: Some(vec!["bob".into(), "alice".into()]), ..facts };
        assert_eq!(plan_transfer("sddm", "lightdm", &read, &list, &member).group, None);
    }

    #[test]
    fn в_ddm_свой_файл_в_каталоге_и_без_relogin() {
        let m = fs(&[("/etc/plasmalogin.conf", "[Autologin]\nRelogin=true\nSession=treeland-user.desktop\nUser=alice\n")]);
        let (read, list) = io(&m);
        let t = plan_transfer("plasmalogin", "ddm", &read, &list, &TargetFacts::default());
        assert_eq!(t.edits.len(), 1);
        assert_eq!(t.edits[0].path, "/etc/ddm.conf.d/zz-rexon-autologin.conf");
        assert!(!t.edits[0].text.contains("Relogin"), "у DDM нет Relogin: {}", t.edits[0].text);
        assert!(t.notes.contains(&Note::ReloginLost));
        let after = apply(&m, &t);
        let (read2, list2) = io(&after);
        assert_eq!(read_autologin("ddm", &read2, &list2), on("alice", "treeland-user", false));
    }

    #[test]
    fn без_сеанса_откроется_последний_выбранный() {
        let m = fs(&[("/etc/gdm/custom.conf", "[daemon]\nAutomaticLoginEnable=true\nAutomaticLogin=carol\n")]);
        let (read, list) = io(&m);
        let t = plan_transfer("gdm", "plasmalogin", &read, &list, &TargetFacts::default());
        assert_eq!(t.notes, vec![Note::Moved { user: "carol".into() }, Note::LastSession]);
        let after = apply(&m, &t);
        let (read2, list2) = io(&after);
        assert_eq!(read_autologin("plasmalogin", &read2, &list2), on("carol", "", false));
    }

    #[test]
    fn автовхода_не_было_у_нового_он_выключается() {
        let m = fs(&[
            ("/etc/plasmalogin.conf", "[Autologin]\nSession=plasma\n"),
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nSession=plasma\nUser=bob\n"),
        ]);
        let (read, list) = io(&m);
        let t = plan_transfer("plasmalogin", "sddm", &read, &list, &TargetFacts::default());
        assert_eq!(t.notes, vec![Note::TurnedOff { user: "bob".into() }]);
        let after = apply(&m, &t);
        assert_eq!(after["/etc/sddm.conf.d/kde_settings.conf"], "[Autologin]\nSession=plasma\nUser=\n");
        let (read2, list2) = io(&after);
        assert_eq!(read_autologin("sddm", &read2, &list2), AutologinState::Off);
        // Nowhere: nothing.
        let empty = fs(&[]);
        let (r, l) = io(&empty);
        assert!(plan_transfer("plasmalogin", "sddm", &r, &l, &TargetFacts::default()).is_nothing());
    }

    #[test]
    fn нельзя_перенести_честно_говорится() {
        let m = fs(&[
            ("/etc/plasmalogin.conf", "[Autologin]\nSession=plasma\nUser=alice\n"),
            // A package file read AFTER /etc by LightDM is the specific group of the place.
            ("/usr/share/lightdm/lightdm.conf.d/99-x.conf", "[Seat:seat0]\nautologin-user=\n"),
        ]);
        let (read, list) = io(&m);
        let greetd = plan_transfer("plasmalogin", "greetd", &read, &list, &TargetFacts::default());
        assert!(greetd.is_lost() && greetd.edits.is_empty());
        assert!(transfer_note("greetd", &greetd).unwrap().contains("alice"));
        let unknown = plan_transfer("plasmalogin", "ly", &read, &list, &TargetFacts::default());
        assert_eq!(unknown.notes, vec![Note::LostUnknown { user: "alice".into() }]);
        let me = TargetFacts { greeter_user: Some("alice".into()), ..TargetFacts::default() };
        assert!(plan_transfer("plasmalogin", "sddm", &read, &list, &me).is_lost());
        // [Seat:seat0] from the package directory overrides [Seat:*] of the main file.
        let elsewhere = plan_transfer("plasmalogin", "lightdm", &read, &list, &TargetFacts::default());
        assert_eq!(elsewhere.notes, vec![Note::LostElsewhere { user: "alice".into() }]);
        assert!(elsewhere.edits.is_empty(), "при «нельзя» ничего не пишется");
    }

    #[test]
    fn неизвестный_прежний_ничего_не_меняет() {
        let m = fs(&[("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nUser=bob\n")]);
        let (read, list) = io(&m);
        assert!(plan_transfer("ly", "sddm", &read, &list, &TargetFacts::default()).is_nothing());
    }

    #[test]
    fn pam_группа_автовхода() {
        let arch = "#%PAM-1.0\nauth        required    pam_env.so\nauth        [success=1 default=ignore]  pam_succeed_if.so user ingroup autologin\nauth        required    pam_unix.so\n";
        assert_eq!(pam_autologin_group(arch).as_deref(), Some("autologin"));
        assert_eq!(pam_autologin_group("auth required pam_permit.so\n"), None);
        assert_eq!(pam_autologin_group("# auth sufficient pam_succeed_if.so user ingroup x\n"), None);
        assert_eq!(pam_autologin_group("account required pam_succeed_if.so user ingroup x\n"), None);
    }

    #[test]
    fn маски_звука_переносятся_только_если_были() {
        assert!(audio_masks_to_add(&[], &|_| false).is_empty());
        let all = audio_masks_to_add(&["wireplumber.service"], &|_| false);
        assert_eq!(all, GREETER_AUDIO_UNITS.to_vec());
        let some = audio_masks_to_add(&["pipewire.service"], &|u| u == "pipewire.socket");
        assert!(!some.contains(&"pipewire.socket") && some.len() == GREETER_AUDIO_UNITS.len() - 1);
        assert!(unit_masked(Some("/dev/null"), None));
        assert!(unit_masked(None, Some("")));
        assert!(!unit_masked(Some("/usr/lib/systemd/user/pipewire.service"), Some("[Unit]\n")));
        assert_eq!(greeter_user_unit_dir("/var/lib/sddm/"), "/var/lib/sddm/.config/systemd/user");
    }

    #[test]
    fn каждый_экран_входа_из_таблицы_с_автовходом_пишется_в_etc() {
        for s in AUTOLOGIN_SPECS {
            if s.layout == Layout::Greetd {
                continue;
            }
            let own = if s.own.is_empty() { *s.files.last().unwrap() } else { s.own };
            assert!(own.starts_with("/etc/"), "{}: {own}", s.unit);
            assert!(crate::system_plan::login_manager_spec(s.unit).is_some(), "{} нет в LOGIN_MANAGER_SPECS", s.unit);
        }
    }
}
