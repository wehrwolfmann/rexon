//! `rexon-a11y-robot` — drives the Rexon window from OUTSIDE, through the
//! accessibility tree (AT-SPI2 over D-Bus), the way a person or a screen
//! reader would.
//!
//! The second path next to `rexon-main-window --tour`: the tour walks the
//! window from inside (it calls the pages itself); the robot only sees what
//! the toolkit publishes to assistive technology — roles, names, states,
//! actions — and acts through it: switches section tabs, types into search
//! fields, presses search buttons, walks the keyboard focus with Tab. What it
//! cannot find by name and role, a blind user cannot find either, so the same
//! run is an accessibility audit.
//!
//! Isolation (nothing on the live desktop is touched):
//!   * its own X server (`Xvfb`, an unused display number), so the window is
//!     invisible and key presses go nowhere else;
//!   * its own session bus (`dbus-daemon --session`), on which the AT-SPI bus
//!     launcher and registry are activated, so nothing talks to the live
//!     Plasma/tray/notification daemons;
//!   * its own HOME / XDG_* / XDG_RUNTIME_DIR in a temporary folder: the
//!     window's settings, lock and log live there and are removed afterwards;
//!   * the first-run wizard and the tutorial are switched off for that
//!     profile (`REXON_FIRST_RUN=never`, `onboarding_enabled=false`).
//!
//! Safety: every action goes through [`action_allowed`] — section tabs,
//! search fields and search buttons only; anything whose name looks like
//! install/remove/apply/... is refused even if it also says "search".
//!
//! Usage:
//! ```text
//! rust/target/release/rexon-a11y-robot [--out DIR] [--window PATH]
//!     [--sections a,b] [--query WORD] [--settle SECONDS] [--tabs N]
//! ```
//! Exit codes: 0 — the run finished (findings are in the report, they do not
//! fail the run); 1 — the stand could not be set up or the window never
//! appeared on the accessibility bus; 2 — bad arguments.
//!
//! Requirements (looked up in PATH, nothing is installed): `Xvfb`,
//! `dbus-daemon`, at-spi2-core (`at-spi-bus-launcher`, `at-spi2-registryd`,
//! D-Bus activated).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

/// An AT-SPI object reference: (bus name, object path) — D-Bus `(so)`.
type ObjRef = (String, OwnedObjectPath);

const IFACE_ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const IFACE_ACTION: &str = "org.a11y.atspi.Action";
const IFACE_EDITABLE_TEXT: &str = "org.a11y.atspi.EditableText";
const IFACE_TEXT: &str = "org.a11y.atspi.Text";
const REGISTRY: &str = "org.a11y.atspi.Registry";
const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";

// AtspiStateType bit numbers (at-spi2-core `atspi-constants.h`).
const STATE_CHECKED: u32 = 4;
const STATE_FOCUSABLE: u32 = 11;
const STATE_FOCUSED: u32 = 12;
const STATE_SELECTED: u32 = 23;
const STATE_SENSITIVE: u32 = 24;
const STATE_SHOWING: u32 = 25;
const STATE_VISIBLE: u32 = 30;

// AtspiKeySynthType::ATSPI_KEY_SYM and the X keysyms we send.
const KEY_SYM: u32 = 3;
const KEYSYM_TAB: i32 = 0xff09;
const KEYSYM_RETURN: i32 = 0xff0d;
const KEYSYM_SPACE: i32 = 0x20;

/// Limits of one tree walk: a runaway model must not hang the run.
const MAX_NODES: usize = 6000;
const MAX_DEPTH: usize = 80;

/// Roles a person interacts with. An element of such a role must have a
/// name (a screen reader announces it) and must be focusable.
const INTERACTIVE_ROLES: &[&str] = &[
    "push button",
    "toggle button",
    "check box",
    "radio button",
    "page tab",
    "combo box",
    "text",
    "entry",
    "password text",
    "slider",
    "spin button",
    "menu item",
    "check menu item",
    "radio menu item",
    "link",
    "button",
];

/// Words that mark a search control (English and Russian UI).
const SEARCH_WORDS: &[&str] = &["search", "find", "поиск", "найти", "искать"];

/// Words that mark a control that changes the system or the profile. A
/// match refuses the action even when a search word is present too.
const DENY_WORDS: &[&str] = &[
    "install", "remove", "uninstall", "delete", "apply", "reset", "restore", "update",
    "upgrade", "enable", "disable", "download", "save", "import", "export", "reboot",
    "log out", "logout", "clear", "set as", "switch to", "установ", "удал", "примен",
    "сброс", "восстанов", "обнов", "включ", "выключ", "скача", "загруз", "сохран",
    "импорт", "экспорт", "перезагр", "выйти", "выход", "очист", "сделать",
];

/// What the robot wants to do with an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Intent {
    /// An AT-SPI action by name ("Press", "SetFocus", ...).
    Action,
    /// Replace the text of an editable element.
    SetText,
}

/// The single safety door: may this action be performed on this element?
/// `Err` carries the reason for the report.
fn action_allowed(role: &str, name: &str, intent: Intent, action: &str) -> Result<(), String> {
    let lower = name.to_lowercase();
    if let Some(w) = DENY_WORDS.iter().find(|w| lower.contains(*w)) {
        // Section tabs only switch pages; their labels may contain any word.
        if !(role == "page tab" && intent == Intent::Action && matches!(action, "Press" | "SetFocus" | "Set Focus")) {
            return Err(format!("запрещённое слово «{w}» в имени"));
        }
    }
    let searchy = SEARCH_WORDS.iter().any(|w| lower.contains(w));
    match intent {
        Intent::Action if action == "SetFocus" || action == "Set Focus" => Ok(()),
        Intent::Action if role == "page tab" && action == "Press" => Ok(()),
        Intent::Action if matches!(role, "push button" | "button") && action == "Press" && searchy => Ok(()),
        Intent::SetText if matches!(role, "text" | "entry") && searchy => Ok(()),
        Intent::SetText => Err("ввод текста только в поле поиска".to_string()),
        Intent::Action => Err(format!("действие «{action}» для роли «{role}» не в белом списке")),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AT-SPI access
// ─────────────────────────────────────────────────────────────────────────────

struct Atspi {
    bus: Connection,
}

/// One element of the accessibility tree, as read in one walk.
#[derive(Clone, Debug, Default)]
struct Node {
    obj: Option<ObjRef>,
    depth: usize,
    role: String,
    name: String,
    description: String,
    states: u64,
    actions: Vec<String>,
    editable: bool,
    /// Names of the ancestors (non-empty only), outermost first.
    path: Vec<String>,
    /// The last named element before this one in reading order: where a
    /// person finds the element (containers in QML seldom have names).
    near: String,
}

impl Node {
    fn has(&self, bit: u32) -> bool {
        self.states & (1u64 << bit) != 0
    }
    fn interactive(&self) -> bool {
        INTERACTIVE_ROLES.contains(&self.role.as_str())
    }
    fn where_(&self) -> String {
        let inside = self.path.last().cloned().unwrap_or_else(|| "(корень окна)".to_string());
        if self.near.is_empty() {
            inside
        } else {
            format!("{inside}, после «{}»", self.near)
        }
    }
    /// Name and description together: what the safety door and the search
    /// look at (Qt puts a field's placeholder into the description).
    fn label(&self) -> String {
        if self.description.is_empty() || self.description == self.name {
            self.name.clone()
        } else {
            format!("{} {}", self.name, self.description)
        }
    }
}

impl Atspi {
    fn call<B, R>(&self, obj: &ObjRef, iface: &str, method: &str, body: &B) -> zbus::Result<R>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
        R: serde::de::DeserializeOwned + zbus::zvariant::Type,
    {
        let msg = self.bus.call_method(Some(obj.0.as_str()), obj.1.as_str(), Some(iface), method, body)?;
        let body = msg.body();
        let out = body.deserialize::<R>();
        out
    }

    fn prop(&self, obj: &ObjRef, iface: &str, name: &str) -> Option<OwnedValue> {
        self.call::<_, OwnedValue>(obj, "org.freedesktop.DBus.Properties", "Get", &(iface, name)).ok()
    }

    fn prop_string(&self, obj: &ObjRef, iface: &str, name: &str) -> String {
        self.prop(obj, iface, name).and_then(|v| String::try_from(v).ok()).unwrap_or_default()
    }

    fn children(&self, obj: &ObjRef) -> Vec<ObjRef> {
        self.call::<_, Vec<ObjRef>>(obj, IFACE_ACCESSIBLE, "GetChildren", &()).unwrap_or_default()
    }

    fn read(&self, obj: &ObjRef, depth: usize, path: &[String]) -> Node {
        let role = self.call::<_, String>(obj, IFACE_ACCESSIBLE, "GetRoleName", &()).unwrap_or_default();
        let states = self
            .call::<_, Vec<u32>>(obj, IFACE_ACCESSIBLE, "GetState", &())
            .map(|v| v.first().copied().unwrap_or(0) as u64 | (v.get(1).copied().unwrap_or(0) as u64) << 32)
            .unwrap_or(0);
        let ifaces = self.call::<_, Vec<String>>(obj, IFACE_ACCESSIBLE, "GetInterfaces", &()).unwrap_or_default();
        let actions = if ifaces.iter().any(|i| i == IFACE_ACTION) {
            self.call::<_, Vec<(String, String, String)>>(obj, IFACE_ACTION, "GetActions", &())
                .map(|v| v.into_iter().map(|a| a.0).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Node {
            obj: Some(obj.clone()),
            depth,
            role,
            near: String::new(),
            name: self.prop_string(obj, IFACE_ACCESSIBLE, "Name"),
            description: self.prop_string(obj, IFACE_ACCESSIBLE, "Description"),
            states,
            actions,
            editable: ifaces.iter().any(|i| i == IFACE_EDITABLE_TEXT),
            path: path.to_vec(),
        }
    }

    /// Depth-first walk under `root`. With `showing_only`, subtrees that are
    /// not on screen are not entered (cheap walk for the focus check).
    fn walk(&self, root: &ObjRef, showing_only: bool) -> Vec<Node> {
        let mut out = Vec::new();
        let mut stack: Vec<(ObjRef, usize, Vec<String>)> = vec![(root.clone(), 0, Vec::new())];
        while let Some((obj, depth, path)) = stack.pop() {
            if out.len() >= MAX_NODES {
                break;
            }
            let mut node = self.read(&obj, depth, &path);
            node.near = out
                .iter()
                .rev()
                .find(|n: &&Node| !n.name.trim().is_empty())
                .map(|n| short(&n.name, 40))
                .unwrap_or_default();
            let descend = depth < MAX_DEPTH && (!showing_only || depth == 0 || node.has(STATE_SHOWING));
            let mut child_path = path;
            if !node.name.trim().is_empty() {
                child_path.push(short(&node.name, 40));
            }
            out.push(node);
            if descend {
                for c in self.children(&obj).into_iter().rev() {
                    stack.push((c, depth + 1, child_path.clone()));
                }
            }
        }
        out
    }

    fn do_action(&self, node: &Node, action: &str) -> Result<(), String> {
        action_allowed(&node.role, &node.label(), Intent::Action, action)?;
        let obj = node.obj.as_ref().ok_or("нет ссылки")?;
        let idx = node
            .actions
            .iter()
            .position(|a| a == action)
            .ok_or_else(|| format!("у элемента нет действия «{action}»"))?;
        match self.call::<_, bool>(obj, IFACE_ACTION, "DoAction", &(idx as i32)) {
            Ok(true) => Ok(()),
            Ok(false) => Err("DoAction вернул false".to_string()),
            Err(e) => Err(format!("DoAction: {e}")),
        }
    }

    fn focus(&self, node: &Node) -> Result<(), String> {
        let a = if node.actions.iter().any(|a| a == "SetFocus") { "SetFocus" } else { "Set Focus" };
        self.do_action(node, a)
    }

    fn set_text(&self, node: &Node, text: &str) -> Result<String, String> {
        action_allowed(&node.role, &node.label(), Intent::SetText, "")?;
        let obj = node.obj.as_ref().ok_or("нет ссылки")?;
        self.call::<_, bool>(obj, IFACE_EDITABLE_TEXT, "SetTextContents", &(text,))
            .map_err(|e| format!("SetTextContents: {e}"))?;
        // Read back: what a screen reader would hear in the field now.
        let n = self
            .prop(obj, IFACE_TEXT, "CharacterCount")
            .and_then(|v| i32::try_from(v).ok())
            .unwrap_or(-1);
        Ok(self.call::<_, String>(obj, IFACE_TEXT, "GetText", &(0i32, n)).unwrap_or_default())
    }

    /// A key press through the registry (XTest on the robot's own X server).
    fn key(&self, keysym: i32) -> Result<(), String> {
        let dec = (REGISTRY.to_string(), OwnedObjectPath::try_from("/org/a11y/atspi/registry/deviceeventcontroller").unwrap());
        self.call::<_, ()>(&dec, "org.a11y.atspi.DeviceEventController", "GenerateKeyboardEvent", &(keysym, "", KEY_SYM))
            .map_err(|e| format!("GenerateKeyboardEvent: {e}"))
    }

    fn pid_of(&self, bus_name: &str) -> Option<u32> {
        let dbus = ("org.freedesktop.DBus".to_string(), OwnedObjectPath::try_from("/org/freedesktop/DBus").unwrap());
        self.call::<_, u32>(&dbus, "org.freedesktop.DBus", "GetConnectionUnixProcessID", &(bus_name,)).ok()
    }
}

fn short(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= max {
        s
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Audit
// ─────────────────────────────────────────────────────────────────────────────

/// Accessibility findings of one screen.
#[derive(Default)]
struct Audit {
    total: usize,
    interactive: usize,
    /// Interactive, no name and no description: a screen reader says only the role.
    unnamed: Vec<String>,
    /// Interactive, no name, but a description (read late, after the role).
    described_only: Vec<String>,
    /// Interactive, the name is only symbols ("🔍", "✕"): a screen reader
    /// reads the emoji name or nothing, not what the button does.
    symbol_only: Vec<String>,
    /// Interactive and enabled, but cannot take keyboard focus.
    not_focusable: Vec<String>,
    /// Has actions but an unknown/filler role: a screen reader cannot tell what it is.
    roleless: Vec<String>,
}

fn audit(nodes: &[Node]) -> Audit {
    // Only what is on screen now: pages of other sections stay in the tree
    // (hidden), and they are audited when the robot opens them.
    let shown: Vec<&Node> = nodes.iter().filter(|n| n.has(STATE_SHOWING)).collect();
    let mut a = Audit { total: shown.len(), ..Default::default() };
    for n in shown {
        let label = || format!("[{}] в {}", n.role, n.where_());
        let acts = !n.actions.is_empty() && !n.actions.iter().all(|x| x == "SetFocus" || x == "Set Focus");
        if acts && matches!(n.role.as_str(), "unknown" | "filler" | "invalid" | "redundant object" | "") {
            a.roleless.push(format!("{} {{{}}} «{}»", label(), n.actions.join(","), short(&n.name, 40)));
        }
        if !n.interactive() {
            continue;
        }
        a.interactive += 1;
        if n.name.trim().is_empty() {
            if n.description.trim().is_empty() {
                a.unnamed.push(label());
            } else {
                a.described_only.push(format!("{} (описание: «{}»)", label(), short(&n.description, 50)));
            }
        } else if !n.name.chars().any(char::is_alphanumeric) {
            a.symbol_only.push(format!("{} «{}»", label(), n.name.trim()));
        }
        if n.has(STATE_SENSITIVE) && !n.has(STATE_FOCUSABLE) && n.role != "page tab" {
            a.not_focusable.push(format!("{} «{}»", label(), short(&n.name, 40)));
        }
    }
    a
}

fn dump_tree(nodes: &[Node]) -> String {
    let mut s = String::new();
    for n in nodes {
        let mut st = String::new();
        for (bit, ch) in [
            (STATE_SHOWING, 'S'),
            (STATE_SENSITIVE, 's'),
            (STATE_FOCUSABLE, 'F'),
            (STATE_FOCUSED, '*'),
            (STATE_CHECKED, 'c'),
            (STATE_SELECTED, 'x'),
        ] {
            if n.has(bit) {
                st.push(ch);
            }
        }
        let _ = write!(s, "{}[{}] «{}»", "  ".repeat(n.depth), n.role, short(&n.name, 70));
        if !n.description.is_empty() {
            let _ = write!(s, " ({})", short(&n.description, 60));
        }
        if !n.actions.is_empty() {
            let _ = write!(s, " {{{}}}", n.actions.join(","));
        }
        if n.editable {
            s.push_str(" ✎");
        }
        let _ = writeln!(s, " <{st}>");
    }
    s
}

// ─────────────────────────────────────────────────────────────────────────────
// Stand: Xvfb + private session bus + temporary profile
// ─────────────────────────────────────────────────────────────────────────────

struct Stand {
    xvfb: Child,
    dbus: Child,
    window: Option<Child>,
    profile: PathBuf,
    runtime: PathBuf,
    env: Vec<(String, String)>,
}

impl Stand {
    fn start(out: &Path) -> Result<Stand, String> {
        let display = (140..240)
            .find(|n| !Path::new(&format!("/tmp/.X11-unix/X{n}")).exists() && !Path::new(&format!("/tmp/.X{n}-lock")).exists())
            .ok_or("нет свободного номера дисплея")?;
        let xvfb = Command::new("Xvfb")
            .args([format!(":{display}"), "-screen".into(), "0".into(), "1600x1000x24".into(), "-nolisten".into(), "tcp".into()])
            .stdout(Stdio::null())
            .stderr(log_file(out, "xvfb.log")?)
            .process_group(0)
            .spawn()
            .map_err(|e| format!("Xvfb не запустился: {e}"))?;
        let sock = PathBuf::from(format!("/tmp/.X11-unix/X{display}"));
        let t = Instant::now();
        while !sock.exists() {
            if t.elapsed() > Duration::from_secs(10) {
                return Err("Xvfb не поднял сокет за 10 с".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        // A temporary profile. Short path in the temp dir: XDG_RUNTIME_DIR holds sockets.
        let profile = std::env::temp_dir().join(format!("rexon-a11y-{}", std::process::id()));
        let dirs = ["home", "config", "data", "cache", "state", "runtime"];
        for d in dirs {
            std::fs::create_dir_all(profile.join(d)).map_err(|e| format!("профиль: {e}"))?;
        }
        // XDG_RUNTIME_DIR holds sockets (108-byte limit, GTK helpers on the bus
        // fail with "Socket name too long" under a deep TMPDIR): a short path
        // in /tmp when possible, otherwise inside the profile.
        let short_runtime = PathBuf::from(format!("/tmp/rxa11y-{}", std::process::id()));
        let runtime = if std::fs::create_dir(&short_runtime).is_ok() { short_runtime } else { profile.join("runtime") };
        std::fs::set_permissions(&runtime, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .map_err(|e| format!("профиль: {e}"))?;
        let rexon_cfg = profile.join("config").join("rexon");
        std::fs::create_dir_all(&rexon_cfg).map_err(|e| format!("профиль: {e}"))?;
        let mut settings = serde_json::Map::new();
        settings.insert(rexon_core::onboarding::KEY_ENABLED.to_string(), serde_json::Value::Bool(false));
        let settings = serde_json::Value::Object(settings);
        std::fs::write(rexon_cfg.join("settings.json"), settings.to_string()).map_err(|e| format!("профиль: {e}"))?;

        let p = |d: &str| profile.join(d).to_string_lossy().into_owned();
        let mut env: Vec<(String, String)> = vec![
            ("DISPLAY".into(), format!(":{display}")),
            ("HOME".into(), p("home")),
            ("XDG_CONFIG_HOME".into(), p("config")),
            ("XDG_DATA_HOME".into(), p("data")),
            ("XDG_CACHE_HOME".into(), p("cache")),
            ("XDG_STATE_HOME".into(), p("state")),
            ("XDG_RUNTIME_DIR".into(), runtime.to_string_lossy().into_owned()),
            ("XDG_SESSION_TYPE".into(), "x11".into()),
            ("QT_QPA_PLATFORM".into(), "xcb".into()),
            ("QT_LINUX_ACCESSIBILITY_ALWAYS_ON".into(), "1".into()),
            ("QT_ACCESSIBILITY".into(), "1".into()),
            ("LIBGL_ALWAYS_SOFTWARE".into(), "1".into()),
            (rexon_core::installer::FIRST_RUN_ENV.into(), rexon_core::installer::FIRST_RUN_NEVER.into()),
        ];
        // Everything else from the live environment stays out: SESSION_MANAGER
        // (Qt would register with the live session manager), the live buses,
        // WAYLAND_DISPLAY, the SSH/GPG agents. Kept: locale, PATH, the system
        // XDG dirs and the desktop NAME (detection only, it connects nowhere).
        let inherited: Vec<(String, String)> = std::env::vars().filter(|(k, _)| inherit_var(k)).collect();
        env.splice(0..0, inherited);
        let mut cmd = Command::new("dbus-daemon");
        cmd.args(["--session", "--nofork", "--print-address=1"])
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdout(Stdio::piped())
            .stderr(log_file(out, "dbus.log")?)
            .process_group(0);
        let mut dbus = cmd.spawn().map_err(|e| format!("dbus-daemon не запустился: {e}"))?;
        let mut line = String::new();
        BufReader::new(dbus.stdout.take().ok_or("нет вывода dbus-daemon")?)
            .read_line(&mut line)
            .map_err(|e| format!("адрес шины: {e}"))?;
        let addr = line.trim().to_string();
        if addr.is_empty() {
            return Err("dbus-daemon не напечатал адрес".into());
        }
        env.push(("DBUS_SESSION_BUS_ADDRESS".into(), addr));
        Ok(Stand { xvfb, dbus, window: None, profile, runtime, env })
    }

    fn session_address(&self) -> &str {
        &self.env.iter().find(|(k, _)| k == "DBUS_SESSION_BUS_ADDRESS").unwrap().1
    }

    /// Activates the AT-SPI bus on the private session bus, switches
    /// accessibility on and returns a connection to the AT-SPI bus.
    fn atspi(&mut self) -> Result<Atspi, String> {
        let session = zbus::blocking::connection::Builder::address(self.session_address())
            .and_then(|b| b.method_timeout(Duration::from_secs(15)).build())
            .map_err(|e| format!("сессионная шина: {e}"))?;
        let bus_obj = ("org.a11y.Bus", "/org/a11y/bus");
        let _ = session.call_method(
            Some(bus_obj.0),
            bus_obj.1,
            Some("org.freedesktop.DBus.Properties"),
            "Set",
            &("org.a11y.Status", "IsEnabled", Value::from(true)),
        );
        let reply = session
            .call_method(Some(bus_obj.0), bus_obj.1, Some("org.a11y.Bus"), "GetAddress", &())
            .map_err(|e| format!("org.a11y.Bus.GetAddress (at-spi2-core установлен?): {e}"))?;
        let addr: String = reply.body().deserialize().map_err(|e| format!("адрес a11y: {e}"))?;
        // Exported to the window as well: Qt may otherwise race the launcher
        // (the KDE runner does the same).
        self.env.push(("AT_SPI_BUS_ADDRESS".into(), addr.clone()));
        let bus = zbus::blocking::connection::Builder::address(addr.as_str())
            .and_then(|b| b.method_timeout(Duration::from_secs(5)).build())
            .map_err(|e| format!("шина a11y: {e}"))?;
        Ok(Atspi { bus })
    }

    fn launch(&mut self, window: &Path, out: &Path) -> Result<u32, String> {
        let child = Command::new(window)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdout(log_file(out, "window.log")?)
            .stderr(log_file(out, "window.log")?)
            .process_group(0)
            .spawn()
            .map_err(|e| format!("{}: {e}", window.display()))?;
        let pid = child.id();
        self.window = Some(child);
        Ok(pid)
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        // Process groups by pid of our own children only — never by name.
        let mut kids: Vec<&mut Child> = Vec::new();
        if let Some(w) = self.window.as_mut() {
            kids.push(w);
        }
        kids.push(&mut self.dbus);
        kids.push(&mut self.xvfb);
        for c in kids {
            let pid = c.id() as i32;
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(5) {
                if matches!(c.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if !matches!(c.try_wait(), Ok(Some(_))) {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
                let _ = c.wait();
            }
        }
        let _ = std::fs::remove_dir_all(&self.profile);
        let _ = std::fs::remove_dir_all(&self.runtime);
    }
}

/// Live environment variables the stand passes on (see [`Stand::start`]).
fn inherit_var(name: &str) -> bool {
    const KEEP: &[&str] = &[
        "PATH", "LANG", "LANGUAGE", "USER", "LOGNAME", "SHELL", "TERM", "TMPDIR", "XDG_DATA_DIRS",
        "XDG_CONFIG_DIRS", "XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "DESKTOP_SESSION",
        "KDE_FULL_SESSION", "KDE_SESSION_VERSION", "QT_QPA_PLATFORMTHEME",
    ];
    KEEP.contains(&name) || name.starts_with("LC_")
}

fn log_file(out: &Path, name: &str) -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out.join(name))
        .map_err(|e| format!("{name}: {e}"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Run
// ─────────────────────────────────────────────────────────────────────────────

struct Args {
    out: PathBuf,
    window: PathBuf,
    sections: Vec<String>,
    query: String,
    settle: Duration,
    tabs: usize,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let mut a = Args {
        out: PathBuf::from(home).join(".cache/rexon/a11y"),
        window: exe_dir.join("rexon-main-window"),
        sections: Vec::new(),
        query: "girl".into(),
        settle: Duration::from_millis(2500),
        tabs: 30,
    };
    let mut it = args.iter();
    while let Some(k) = it.next() {
        let (key, inline) = match k.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (k.as_str(), None),
        };
        let mut val = || inline.clone().or_else(|| it.next().cloned()).ok_or(format!("{key}: нужно значение"));
        match key {
            "--out" => a.out = PathBuf::from(val()?),
            "--window" => a.window = PathBuf::from(val()?),
            "--sections" => a.sections = val()?.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect(),
            "--query" => a.query = val()?,
            "--settle" => a.settle = Duration::from_secs_f64(val()?.parse().map_err(|_| "--settle: число секунд")?),
            "--tabs" => a.tabs = val()?.parse().map_err(|_| "--tabs: число")?,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("неизвестный ключ {other}")),
        }
    }
    Ok(a)
}

fn find_window(at: &Atspi, pid: u32, timeout: Duration) -> Option<ObjRef> {
    let root: ObjRef = (REGISTRY.into(), OwnedObjectPath::try_from(ROOT_PATH).unwrap());
    let t = Instant::now();
    while t.elapsed() < timeout {
        for app in at.children(&root) {
            if at.pid_of(&app.0) != Some(pid) {
                continue;
            }
            // The process may publish several top-level windows (hidden helper
            // windows too); the main one carries the product name.
            let shown: Vec<(ObjRef, String)> = at
                .children(&app)
                .into_iter()
                .map(|w| {
                    let n = at.read(&w, 0, &[]);
                    (w, n.name, n.states)
                })
                .filter(|(_, _, st)| st & (1u64 << STATE_VISIBLE | 1u64 << STATE_SHOWING) != 0)
                .map(|(w, name, _)| (w, name))
                .collect();
            if let Some((w, _)) = shown.iter().find(|(_, n)| n.contains("Rexon")).or(shown.first()) {
                return Some(w.clone());
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    None
}

/// Switches to a section tab: the AT-SPI "Press" action when the tab offers
/// one, otherwise as a keyboard user does it — focus, then Space.
fn activate_tab(at: &Atspi, tab: &Node) -> Result<String, String> {
    at.focus(tab)?;
    if tab.actions.iter().any(|a| a == "Press") {
        at.do_action(tab, "Press")?;
        return Ok("действием Press".into());
    }
    std::thread::sleep(Duration::from_millis(200));
    at.key(KEYSYM_SPACE)?;
    Ok("клавишей Пробел (у вкладки нет действия Press)".into())
}

fn push_audit(r: &mut String, a: &Audit) {
    let _ = writeln!(
        r,
        "  элементов: {}, управляющих: {}, без имени: {}, только описание: {}, имя из одних значков: {}, без фокуса с клавиатуры: {}, действие без роли: {}",
        a.total,
        a.interactive,
        a.unnamed.len(),
        a.described_only.len(),
        a.symbol_only.len(),
        a.not_focusable.len(),
        a.roleless.len()
    );
    for (title, list) in [
        ("без имени", &a.unnamed),
        ("только описание", &a.described_only),
        ("имя из значков", &a.symbol_only),
        ("без фокуса", &a.not_focusable),
        ("действие без роли", &a.roleless),
    ] {
        // Collapse repeats (a grid of identical cards) into one line with a count.
        let mut counted: BTreeMap<&str, usize> = BTreeMap::new();
        for x in list {
            *counted.entry(x.as_str()).or_default() += 1;
        }
        for (x, n) in counted {
            let times = if n > 1 { format!(" ×{n}") } else { String::new() };
            let _ = writeln!(r, "    {title}: {x}{times}");
        }
    }
}

fn run(a: &Args) -> Result<String, String> {
    std::fs::create_dir_all(&a.out).map_err(|e| format!("{}: {e}", a.out.display()))?;
    for f in std::fs::read_dir(&a.out).map_err(|e| e.to_string())?.flatten() {
        let name = f.file_name().to_string_lossy().into_owned();
        if name.ends_with(".log") || name.starts_with("tree-") || name == "report.txt" {
            let _ = std::fs::remove_file(f.path());
        }
    }
    if !a.window.exists() {
        return Err(format!("окно не найдено: {} (cargo build-app)", a.window.display()));
    }
    let mut stand = Stand::start(&a.out)?;
    let at = stand.atspi()?;
    let pid = stand.launch(&a.window, &a.out)?;
    let win = find_window(&at, pid, Duration::from_secs(60))
        .ok_or("окно Rexon не появилось на шине доступности за 60 с (см. window.log)")?;
    std::thread::sleep(a.settle);

    let mut r = String::new();
    let mut actions_log = String::new();
    let mut totals = Audit::default();
    let _ = writeln!(r, "Робот доступности Rexon (AT-SPI2, rexon-a11y-robot)");
    let _ = writeln!(r, "окно: {} (pid {pid}), запрос поиска: «{}»\n", a.window.display(), a.query);

    let first = at.walk(&win, false);
    std::fs::write(a.out.join("tree-00-start.txt"), dump_tree(&first)).map_err(|e| e.to_string())?;
    let tabs: Vec<Node> = first.iter().filter(|n| n.role == "page tab").cloned().collect();
    let _ = writeln!(r, "== Вкладки разделов (роль page tab) ==");
    if tabs.is_empty() {
        let _ = writeln!(r, "  НЕ НАЙДЕНО ни одной: робот не может переключать разделы");
    }
    for t in &tabs {
        let _ = writeln!(r, "  «{}» {{{}}}", t.name, t.actions.join(","));
    }

    let mut not_found: Vec<String> = a.sections.iter().filter(|s| !tabs.iter().any(|t| t.name.to_lowercase().contains(s.as_str()))).cloned().collect();
    for (i, tab) in tabs.iter().enumerate() {
        if !a.sections.is_empty() && !a.sections.iter().any(|s| tab.name.to_lowercase().contains(s.as_str())) {
            continue;
        }
        let _ = writeln!(r, "\n== Раздел {}: «{}» ==", i + 1, tab.name);
        let pressed = activate_tab(&at, tab);
        let _ = writeln!(actions_log, "Press вкладка «{}»: {}", tab.name, pressed.as_ref().map_or_else(|e| e.clone(), |how| how.clone()));
        match &pressed {
            Err(e) => {
                let _ = writeln!(r, "  переключить не удалось: {e}");
                continue;
            }
            Ok(how) => {
                let _ = writeln!(r, "  переключено: {how}");
            }
        }
        std::thread::sleep(a.settle);
        let nodes = at.walk(&win, false);
        let selected = nodes
            .iter()
            .any(|n| n.role == "page tab" && n.name == tab.name && (n.has(STATE_SELECTED) || n.has(STATE_CHECKED)));
        let verdict = if selected { "да" } else { "НЕ подтверждено (у вкладки нет состояния selected/checked)" };
        let _ = writeln!(r, "  вкладка выбрана: {verdict}");
        let au = audit(&nodes);
        push_audit(&mut r, &au);
        totals.total += au.total;
        totals.interactive += au.interactive;
        totals.unnamed.extend(au.unnamed.iter().cloned());
        totals.described_only.extend(au.described_only.iter().cloned());
        totals.symbol_only.extend(au.symbol_only.iter().cloned());
        totals.not_focusable.extend(au.not_focusable.iter().cloned());
        totals.roleless.extend(au.roleless.iter().cloned());

        // Search: the first editable field whose name says "search".
        let fields: Vec<&Node> = nodes.iter().filter(|n| n.editable && n.has(STATE_SHOWING)).collect();
        let search = fields.iter().find(|n| action_allowed(&n.role, &n.label(), Intent::SetText, "").is_ok());
        let _ = writeln!(r, "  полей ввода на экране: {}", fields.len());
        match search {
            None => {
                let _ = writeln!(r, "  поле поиска по имени не найдено");
                for f in &fields {
                    let _ = writeln!(r, "    поле [{}] «{}» ({}) в {}", f.role, f.name, short(&f.description, 50), f.where_());
                }
            }
            Some(f) => {
                let res = at.set_text(f, &a.query);
                let _ = writeln!(actions_log, "SetText «{}» ← «{}»: {:?}", f.name, a.query, res);
                match res {
                    Ok(back) => {
                        let _ = writeln!(r, "  поиск: «{}» ← «{}», в поле теперь «{back}»", f.label().trim(), a.query);
                        // Confirm with Enter, like a person: focus the field first.
                        let enter = at.focus(f).and_then(|_| {
                            std::thread::sleep(Duration::from_millis(300));
                            at.key(KEYSYM_RETURN)
                        });
                        let _ = writeln!(actions_log, "Enter в «{}»: {:?}", f.name, enter);
                    }
                    Err(e) => {
                        let _ = writeln!(r, "  поиск: ввод не удался: {e}");
                    }
                }
                std::thread::sleep(a.settle);
                let after = at.walk(&win, false);
                if let Some(b) = after.iter().find(|n| {
                    n.has(STATE_SHOWING) && n.actions.iter().any(|x| x == "Press") && matches!(n.role.as_str(), "push button" | "button") && action_allowed(&n.role, &n.label(), Intent::Action, "Press").is_ok()
                }) {
                    let res = at.do_action(b, "Press");
                    let _ = writeln!(actions_log, "Press кнопка «{}»: {res:?}", b.name);
                    let _ = writeln!(r, "  кнопка поиска «{}»: {}", b.name, res.map_or_else(|e| e, |_| "нажата".into()));
                    std::thread::sleep(a.settle);
                }
                let after = at.walk(&win, false);
                let _ = writeln!(r, "  после поиска элементов: {} (до: {})", after.len(), nodes.len());
                std::fs::write(a.out.join(format!("tree-{:02}-search.txt", i + 1)), dump_tree(&after)).map_err(|e| e.to_string())?;
            }
        }
        std::fs::write(a.out.join(format!("tree-{:02}.txt", i + 1)), dump_tree(&nodes)).map_err(|e| e.to_string())?;
    }

    // Keyboard: where does Tab take the focus, starting from the first tab?
    if a.tabs > 0 {
        let _ = writeln!(r, "\n== Клавиатура: {} нажатий Tab ==", a.tabs);
        if let Some(t) = tabs.first() {
            let _ = activate_tab(&at, t);
            std::thread::sleep(a.settle);
        }
        let mut stops: Vec<String> = Vec::new();
        for _ in 0..a.tabs {
            if let Err(e) = at.key(KEYSYM_TAB) {
                let _ = writeln!(r, "  нажатие не удалось: {e}");
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
            // Containers report "focused" too when the focus is inside them:
            // the stop is the deepest focused element.
            let focused = at.walk(&win, true).into_iter().filter(|n| n.has(STATE_FOCUSED) && n.depth > 0).max_by_key(|n| n.depth);
            let stop = focused.map_or("(фокус потерян или не виден через AT-SPI)".to_string(), |n| {
                format!("[{}] «{}»{}", n.role, short(&n.name, 50), if n.name.trim().is_empty() { " ← БЕЗ ИМЕНИ" } else { "" })
            });
            stops.push(stop);
        }
        for (i, s) in stops.iter().enumerate() {
            let _ = writeln!(r, "  {:2}. {s}", i + 1);
        }
        let unnamed = stops.iter().filter(|s| s.contains("БЕЗ ИМЕНИ")).count();
        let lost = stops.iter().filter(|s| s.starts_with('(')).count();
        let _ = writeln!(r, "  остановок без имени: {unnamed}, без видимого фокуса: {lost}");
    }

    let mut head = String::new();
    let _ = writeln!(head, "== Итог ==");
    let _ = writeln!(head, "  разделов найдено: {}, пройдено: {}", tabs.len(), r.matches("\n== Раздел").count());
    if !not_found.is_empty() {
        not_found.sort();
        let _ = writeln!(head, "  НЕ найдены разделы по --sections: {}", not_found.join(", "));
    }
    // Shared chrome (the tab strip, the window menu) is seen in every
    // section: the summary counts each distinct element once.
    for list in [&mut totals.unnamed, &mut totals.described_only, &mut totals.symbol_only, &mut totals.not_focusable, &mut totals.roleless] {
        list.sort();
        list.dedup();
    }
    let _ = writeln!(head, "  (элементов/управляющих — сумма по разделам; списки — без повторов между разделами)");
    push_audit(&mut head, &totals);
    let _ = writeln!(head, "\n== Действия робота ==\n{actions_log}");
    Ok(format!("{r}\n{head}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = match parse_args(&args) {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("робот: {e}");
            }
            eprintln!("использование: rexon-a11y-robot [--out ПАПКА] [--window ПУТЬ] [--sections a,b] [--query СЛОВО] [--settle С] [--tabs N]");
            std::process::exit(2);
        }
    };
    match run(&a) {
        Ok(report) => {
            let path = a.out.join("report.txt");
            if let Err(e) = std::fs::write(&path, &report) {
                eprintln!("робот: {}: {e}", path.display());
                std::process::exit(1);
            }
            print!("{report}");
            println!("отчёт: {}", path.display());
        }
        Err(e) => {
            eprintln!("робот: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_switch_whatever_their_label() {
        assert!(action_allowed("page tab", "Рабочие столы", Intent::Action, "Press").is_ok());
        assert!(action_allowed("page tab", "Установка", Intent::Action, "Press").is_ok());
    }

    #[test]
    fn only_search_buttons_are_pressed() {
        assert!(action_allowed("push button", "Найти", Intent::Action, "Press").is_ok());
        assert!(action_allowed("push button", "Search", Intent::Action, "Press").is_ok());
        assert!(action_allowed("push button", "Применить", Intent::Action, "Press").is_err());
        assert!(action_allowed("push button", "OK", Intent::Action, "Press").is_err());
        assert!(action_allowed("check box", "Поиск", Intent::Action, "Toggle").is_err());
    }

    #[test]
    fn deny_word_beats_search_word() {
        assert!(action_allowed("push button", "Найти и установить", Intent::Action, "Press").is_err());
        assert!(action_allowed("push button", "Search and install", Intent::Action, "Press").is_err());
        assert!(action_allowed("text", "Поиск и удаление", Intent::SetText, "").is_err());
    }

    #[test]
    fn text_goes_only_into_search_fields() {
        assert!(action_allowed("text", "Поиск тем…", Intent::SetText, "").is_ok());
        assert!(action_allowed("entry", "Search", Intent::SetText, "").is_ok());
        assert!(action_allowed("text", "Имя профиля", Intent::SetText, "").is_err());
        assert!(action_allowed("password text", "Search", Intent::SetText, "").is_err());
    }

    #[test]
    fn focus_is_always_harmless() {
        assert!(action_allowed("push button", "Удалить", Intent::Action, "SetFocus").is_err(),
            "a deny word refuses even focus: Enter/Space after it would act");
        assert!(action_allowed("push button", "OK", Intent::Action, "SetFocus").is_ok());
    }

    #[test]
    fn audit_counts_unnamed_and_unfocusable() {
        let on = |bits: &[u32]| bits.iter().fold(0u64, |s, b| s | 1u64 << b);
        let nodes = vec![
            Node { role: "push button".into(), states: on(&[STATE_SENSITIVE, STATE_SHOWING, STATE_FOCUSABLE]), ..Default::default() },
            Node { role: "push button".into(), name: "OK".into(), states: on(&[STATE_SENSITIVE, STATE_SHOWING]), ..Default::default() },
            Node { role: "check box".into(), description: "Тёмная тема".into(), states: on(&[STATE_SHOWING]), ..Default::default() },
            Node { role: "unknown".into(), actions: vec!["Press".into()], states: on(&[STATE_SHOWING]), ..Default::default() },
            Node { role: "label".into(), states: on(&[STATE_SHOWING]), ..Default::default() },
            Node { role: "push button".into(), name: "🔍".into(), states: on(&[STATE_SHOWING]), ..Default::default() },
            // A page of another section: in the tree, but not on screen.
            Node { role: "push button".into(), ..Default::default() },
        ];
        let a = audit(&nodes);
        assert_eq!((a.total, a.interactive), (6, 4));
        assert_eq!(a.symbol_only.len(), 1);
        assert_eq!(a.unnamed.len(), 1);
        assert_eq!(a.described_only.len(), 1);
        assert_eq!(a.not_focusable.len(), 1);
        assert_eq!(a.roleless.len(), 1);
    }

    #[test]
    fn args_inline_and_split() {
        let a = parse_args(&["--sections=оформ,steam".into(), "--query".into(), "cat".into(), "--tabs=5".into()]).unwrap();
        assert_eq!(a.sections, vec!["оформ", "steam"]);
        assert_eq!(a.query, "cat");
        assert_eq!(a.tabs, 5);
        assert!(parse_args(&["--bogus".into()]).is_err());
    }
}
