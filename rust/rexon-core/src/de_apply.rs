//! Buttons of the environment sections in the "Appearance" tab: "Apply" for
//! themes, "Enable"/"Disable" for add-ons, "Start"/"Stop" for Conky.
//!
//! The owner's decision of 04.10.2026: installing an `.rxn` ONLY puts files
//! on disk; the person enables it with a button. Previously, for matrix
//! types without a section in the tab (xfwm4 and Marco frames,
//! GNOME/Cinnamon/LXQt shell themes, the KDE window switcher, KWin scripts
//! and effects, GNOME extensions, Cinnamon applets/desklets/extensions,
//! Nemo actions, Conky), the installation itself enabled them: "installed
//! it, and you already have different frames", without a choice.
//!
//! There is no second command list here (rule #12). What to write and
//! where is described by the matrix ([`crate::de_customization`]); the
//! enable and disable steps are the same ones the installation did and the
//! removal does ([`crate::ext_manager::de_enable_steps`] /
//! [`de_disable_steps`]). This module adds only what the installation did
//! not have:
//!   * READING what is enabled now (the "Active" tiles and the "Disable"
//!     button must tell the truth rather than remember what was pressed);
//!   * executing the button with a check: after the steps the value is
//!     reread, and "✅" is set only if the environment really accepted it;
//!   * Conky: starting, stopping and autostarting ONE widget (the
//!     installation killed all conky processes at once by process name).
//!
//! The login screen (SDDM) and the boot splash (Plymouth) go through the
//! system helper as root: their "Apply" lives next to the installation in
//! [`crate::appearance_installer`]; here only the call line
//! ([`system_apply_argv`]).
//!
//! [`de_disable_steps`]: crate::ext_manager::de_disable_steps

use crate::animations::LookIo;
use crate::de_customization::{self as dec, Activate, EntryFmt, Payload, Row};
use crate::i18n::{tr, trf};

// ============================================================================
// Button kind
// ============================================================================

/// Which button a section has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// "🎨 Apply": one theme per environment, the choice replaces the previous one.
    Apply,
    /// "▶ Enable" / "⏸ Disable": any number of add-ons can be enabled.
    Toggle,
    /// "▶ Start" / "⏹ Stop": a separate program (Conky).
    Run,
}

/// How an `.rxn` type is enabled with a section button.
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    /// A theme: one value of an environment key (xfwm4, Marco, GNOME, Cinnamon
    /// and LXQt shells, the Alt+Tab layout).
    Theme(&'static Row),
    /// An add-on: an item of the enabled list (KWin scripts and effects, GNOME
    /// extensions, Cinnamon applets/desklets/extensions, Nemo actions).
    Addon(&'static Row),
    /// A Conky widget: a separate program, with starting and autostart.
    Conky,
}

impl Mode {
    pub fn button(self) -> Button {
        match self {
            Mode::Theme(_) => Button::Apply,
            Mode::Addon(_) => Button::Toggle,
            Mode::Conky => Button::Run,
        }
    }
}

/// The Conky type.
pub const CONKY_KIND: &str = "theme-conky";

/// The Nemo actions type: files Nemo picks up by itself; it keeps the
/// disabled ones in its [`NEMO_DISABLED`] list (the native key: Nemo's own
/// "Actions configuration" disables them the same way).
pub const NEMO_KIND: &str = "nemo-action";
pub const NEMO_DISABLED: (&str, &str) = ("org.nemo.plugins", "disabled-actions");

/// The Marco (MATE) window frame type.
pub const MARCO_KIND: &str = "theme-marco";

/// The Openbox and labwc window frame type: `<theme><name>` in the
/// `rc.xml` of the session's window manager ([`session_wm`]).
pub const OPENBOX_KIND: &str = "theme-openbox";

/// The User Themes extension schema: GNOME takes the shell theme through
/// its `name` key. No schema means the extension is not installed, and
/// there is nowhere to write the theme.
pub const USER_THEME_SCHEMA: &str = "org.gnome.shell.extensions.user-theme";

/// How a type is enabled with a button. `None` means the type has no
/// button of this module (types of the common appearance installer, which
/// have their own actions; the Plasma wallpaper plugin, which is chosen
/// only in the desktop settings).
pub fn mode(kind: &str) -> Option<Mode> {
    if kind == CONKY_KIND {
        return Some(Mode::Conky);
    }
    let row = dec::de_managed_row(kind)?;
    match row.activate {
        Activate::KdeConfig { .. }
        | Activate::Gsettings { .. }
        | Activate::Xfconf { .. }
        | Activate::IniKey { .. }
        | Activate::RcXmlTheme => Some(Mode::Theme(row)),
        Activate::KwinPlugin | Activate::GnomeExtension | Activate::GsettingsList { .. } => {
            Some(Mode::Addon(row))
        }
        Activate::Auto if kind == NEMO_KIND => Some(Mode::Addon(row)),
        _ => None,
    }
}

/// The section button by its application kind (`apply_kind` = the section
/// key). Sections this module does not handle get the plain "Apply".
pub fn button_for_apply_kind(apply_kind: &str) -> Button {
    crate::categories::category_kind(apply_kind)
        .and_then(mode)
        .map(Mode::button)
        .unwrap_or(Button::Apply)
}

/// The application kind from a local tile id `local:<kind>:<name>`.
pub fn apply_kind_of_local_id(id: &str) -> Option<&str> {
    id.strip_prefix("local:")?.split_once(':').map(|(k, _)| k)
}

// ============================================================================
// Reading: what is enabled now
// ============================================================================

/// Where to look for the user's files.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub home: String,
    /// `$XDG_CONFIG_HOME`, if set (otherwise `~/.config`).
    pub xdg_config_home: Option<String>,
    /// A Wayland session (for the Conky caveat).
    pub wayland: bool,
    /// The running environment (`mate`, `gnome`…; empty means not recognised),
    /// for the Conky caveat: on Wayland it does not work the same everywhere.
    pub de: String,
}

impl Ctx {
    /// The live session.
    pub fn live() -> Self {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        let home = {
            let h = env("HOME");
            if h.is_empty() { "/root".to_string() } else { h }
        };
        let xdg = env("XDG_CONFIG_HOME");
        let wayland = !env("WAYLAND_DISPLAY").is_empty()
            || env("XDG_SESSION_TYPE").eq_ignore_ascii_case("wayland");
        let de = crate::system::detect_current_de(
            &env("XDG_CURRENT_DESKTOP"),
            &env("DESKTOP_SESSION"),
            !env("WAYLAND_DISPLAY").is_empty(),
        )
        .unwrap_or_default();
        Ctx {
            home,
            xdg_config_home: (!xdg.is_empty()).then_some(xdg),
            wayland,
            de,
        }
    }

    /// Context for home `home`: the live session if this is this process's
    /// home (the session's `$XDG_CONFIG_HOME`), otherwise a foreign tree
    /// without session variables (parity checks on a fake home do not read
    /// live directories).
    pub fn for_home(home: &str) -> Self {
        let live = Ctx::live();
        if live.home.trim_end_matches('/') == home.trim_end_matches('/') {
            live
        } else {
            Ctx { home: home.to_string(), xdg_config_home: None, wayland: false, de: String::new() }
        }
    }

    pub(crate) fn config(&self, rel: &str) -> String {
        match &self.xdg_config_home {
            Some(x) => format!("{x}/{rel}"),
            None => format!("{}/.config/{rel}", self.home),
        }
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}

/// The value of an ini key in a section (`[group]` -> `key=`).
pub fn ini_value(text: &str, group: &str, key: &str) -> Option<String> {
    let mut here = false;
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            here = &s[1..s.len() - 1] == group;
            continue;
        }
        if !here {
            continue;
        }
        if let Some((k, v)) = s.split_once('=') {
            if k.trim() == key {
                let v = v.trim();
                return (!v.is_empty()).then(|| v.to_string());
            }
        }
    }
    None
}

/// The `gsettings get` value without quotes (`'Name'` -> `Name`).
pub fn parse_gsettings_string(out: &str) -> Option<String> {
    let v = out.trim().trim_matches('\'').trim_matches('"').trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// A list of strings from a `gsettings get` answer (`['a', 'b']`, `@as []`).
pub fn parse_gsettings_list(out: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in out.chars() {
        match quote {
            Some(q) if ch == q => {
                items.push(std::mem::take(&mut cur));
                quote = None;
            }
            Some(_) => cur.push(ch),
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None => {}
        }
    }
    items
}

/// The object identifier from an item of the enabled list.
pub fn entry_uuid(entry: &str, fmt: EntryFmt) -> Option<String> {
    let field = match fmt {
        EntryFmt::Uuid => Some(entry),
        // panel1:<zone>:<order>:<uuid>:<number>
        EntryFmt::CinnamonApplet => entry.split(':').nth(3),
        // <uuid>:<number>:<x>:<y>
        EntryFmt::CinnamonDesklet => entry.split(':').next(),
    }?;
    let field = field.trim();
    (!field.is_empty()).then(|| field.to_string())
}

/// Whether a KWin plugin is enabled by the `kwinrc` text
/// (`[Plugins] <id>Enabled=`). No key means `EnabledByDefault` from the
/// plugin metadata decides.
pub fn kwin_plugin_enabled(kwinrc: Option<&str>, id: &str, enabled_by_default: bool) -> bool {
    match kwinrc.and_then(|t| ini_value(t, "Plugins", &format!("{id}Enabled"))) {
        Some(v) => v.eq_ignore_ascii_case("true"),
        None => enabled_by_default,
    }
}

/// `KPlugin.EnabledByDefault` from `metadata.json`.
fn kpackage_enabled_by_default(meta_json: Option<&str>) -> bool {
    meta_json
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| v.get("KPlugin")?.get("EnabledByDefault")?.as_bool())
        .unwrap_or(false)
}

/// The system directory of a type's objects next to the matrix user
/// directory: `.local/share/X` -> `/usr/share/X`, `.themes` ->
/// `/usr/share/themes`. Empty means the type has no system directory
/// (Conky).
pub fn system_dir(row: &Row, usr_root: &str) -> String {
    if let Some(rest) = row.dest.strip_prefix(".local/share/") {
        return format!("{usr_root}/share/{rest}");
    }
    if row.dest == ".themes" {
        return format!("{usr_root}/share/themes");
    }
    String::new()
}

/// Directories holding a type's objects (the user one, then the system one).
fn object_dirs(ctx: &Ctx, row: &Row) -> Vec<String> {
    let mut dirs = vec![row.dest_path(&ctx.home)];
    let sys = system_dir(row, "/usr");
    if !sys.is_empty() {
        dirs.push(sys);
    }
    dirs
}

/// The current theme value (`None` means not set or nothing to read it with).
pub fn theme_value(io: &dyn LookIo, ctx: &Ctx, row: &Row) -> Option<String> {
    match row.activate {
        Activate::KdeConfig { file, group, key, .. } => {
            if io.have("kreadconfig6") {
                let (ok, out) = io.run(&argv(&[
                    "kreadconfig6", "--file", file, "--group", group, "--key", key,
                ]));
                if ok {
                    return parse_gsettings_string(&out);
                }
            }
            ini_value(&io.read(&ctx.config(file))?, group, key)
        }
        Activate::Gsettings { schema, key } => {
            let (ok, out) = io.run(&argv(&["gsettings", "get", schema, key]));
            if ok { parse_gsettings_string(&out) } else { None }
        }
        Activate::Xfconf { channel, prop } => {
            let (ok, out) = io.run(&argv(&["xfconf-query", "-c", channel, "-p", prop]));
            if ok { parse_gsettings_string(&out) } else { None }
        }
        Activate::IniKey { file, group, key } => {
            ini_value(&io.read(&format!("{}/{file}", ctx.home))?, group, key)
        }
        Activate::RcXmlTheme => {
            let wm = session_wm(io, ctx).filter(|w| w.rc_xml_path().is_some())?;
            let (_, text, _) = rc_xml(io, ctx, wm);
            crate::qt_settings::rc_xml_theme_value(&text?, &["name"]).filter(|v| !v.is_empty())
        }
        _ => None,
    }
}

// ============================================================================
// Openbox and labwc frames: the `rc.xml` of the session's window manager
// ============================================================================

/// Who draws the frames in this session: for LXQt by its `session.conf`,
/// in a labwc session labwc (`qt_settings::session_title_wm`).
pub fn session_wm(io: &dyn LookIo, ctx: &Ctx) -> Option<crate::qt_settings::TitleWm> {
    let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
    let home = ctx.config("");
    crate::qt_settings::session_title_wm(&ctx.de, home.trim_end_matches('/'), &dirs, ctx.wayland, &|p| io.read(p))
}

/// The manager's `rc.xml`: the path of our own, the text (our own or the
/// system sample) and whether it was taken from the system one.
fn rc_xml(io: &dyn LookIo, ctx: &Ctx, wm: crate::qt_settings::TitleWm) -> (String, Option<String>, bool) {
    let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
    let home = ctx.config("");
    crate::qt_settings::rc_xml_source(wm, home.trim_end_matches('/'), &dirs, &|p| io.read(p))
}

/// Steps for frame theme `name`: the new `rc.xml` text (only
/// `<theme><name>` changes) and the manager's `--reconfigure`. `Err` is a
/// refusal for the person before the first write: the frames are drawn
/// neither by Openbox nor by labwc, or there is nowhere to write `rc.xml`,
/// or it does not parse.
pub fn rc_theme_steps(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Result<Vec<crate::ext_manager::ExtStep>, String> {
    use crate::qt_settings::TitleWm;
    let refuse = |why: String| Err(trf("Couldn't apply “%1”: %2", &[&name, &why]));
    let wm = match session_wm(io, ctx) {
        Some(wm @ (TitleWm::Openbox | TitleWm::Labwc)) => wm,
        Some(other) => {
            return refuse(trf(
                // TRANSLATORS: %1 is a window manager, e.g. “kwin” or “xfwm4”.
                "window borders in this session are drawn by %1, and Openbox themes only work in Openbox and labwc",
                &[&other.program()],
            ))
        }
        None => {
            return refuse(tr(
                "Openbox themes only work in Openbox and labwc, and this session uses another window manager \
                 (or none is chosen in LXQt Session Settings)",
            ))
        }
    };
    let (own, text, _) = rc_xml(io, ctx, wm);
    let Some(content) = crate::qt_settings::rc_xml_with_theme_value(text.as_deref(), wm, &["name"], name) else {
        return refuse(if text.is_none() {
            trf("%1 doesn't exist, and there is no system settings file to start from", &[&own])
        } else {
            trf("the settings file %1 can't be read, so Rexon leaves it as it is", &[&own])
        });
    };
    Ok(vec![
        crate::ext_manager::ExtStep::WriteFile { path: own, content },
        crate::ext_manager::ExtStep::Run { argv: argv(&[wm.program(), "--reconfigure"]), timeout_secs: Some(10) },
    ])
}

/// Enabled objects of an add-on (directory/file names as on disk).
pub fn enabled_addons(io: &dyn LookIo, ctx: &Ctx, kind: &str, row: &Row) -> Vec<String> {
    if kind == NEMO_KIND {
        let (schema, key) = NEMO_DISABLED;
        let (ok, out) = io.run(&argv(&["gsettings", "get", schema, key]));
        let disabled = if ok { parse_gsettings_list(&out) } else { Vec::new() };
        let suffix = match row.payload {
            Payload::Files { ext } => ext,
            _ => "",
        };
        let mut out = Vec::new();
        for dir in object_dirs(ctx, row) {
            for f in io.list_dir(&dir) {
                if f.ends_with(suffix) && !disabled.contains(&f) && !out.contains(&f) {
                    out.push(f);
                }
            }
        }
        return out;
    }
    match row.activate {
        Activate::KwinPlugin => {
            let kwinrc = io.read(&ctx.config("kwinrc"));
            let mut out = Vec::new();
            for dir in object_dirs(ctx, row) {
                for id in io.list_dir(&dir) {
                    let meta = io.read(&format!("{dir}/{id}/metadata.json"));
                    if kwin_plugin_enabled(
                        kwinrc.as_deref(),
                        &id,
                        kpackage_enabled_by_default(meta.as_deref()),
                    ) && !out.contains(&id)
                    {
                        out.push(id);
                    }
                }
            }
            out
        }
        Activate::GnomeExtension => {
            // In a GNOME session the truth is the shell itself: enabled in its
            // settings (even if it failed to start). The `enabled-extensions`
            // key is the fallback path without the shell.
            if let Some(list) = io.gnome_shell_extensions() {
                return crate::gnome_ext_section::enabled_uuids(&list);
            }
            let (ok, out) = io.run(&argv(&["gsettings", "get", "org.gnome.shell", "enabled-extensions"]));
            if ok { parse_gsettings_list(&out) } else { Vec::new() }
        }
        Activate::GsettingsList { schema, key, entry, .. } => {
            let (ok, out) = io.run(&argv(&["gsettings", "get", schema, key]));
            if !ok {
                return Vec::new();
            }
            let mut v: Vec<String> = Vec::new();
            for e in parse_gsettings_list(&out) {
                if let Some(u) = entry_uuid(&e, entry) {
                    if !v.contains(&u) {
                        v.push(u);
                    }
                }
            }
            v
        }
        _ => Vec::new(),
    }
}

/// What is enabled now for a type: the theme name, the enabled add-ons or
/// the running Conky widgets. Empty means nothing (or nothing to read it
/// with).
pub fn active_names(io: &dyn LookIo, ctx: &Ctx, kind: &str) -> Vec<String> {
    match mode(kind) {
        // A frame theme Marco did not load is not shown by it, however long its
        // name stands in the key: "Active" would be untrue for it.
        Some(Mode::Theme(row)) => theme_value(io, ctx, row)
            .filter(|name| kind != MARCO_KIND || marco_theme_problem(io, ctx, name).is_none())
            .into_iter()
            .collect(),
        // "Active" for a GNOME extension is what the shell started (`state` =
        // enabled), not only what is written in the settings: one that crashed
        // while being enabled is not shown as active.
        Some(Mode::Addon(row)) if kind == crate::gnome_ext_section::KIND => match io.gnome_shell_extensions() {
            Some(list) => crate::gnome_ext_section::active_uuids(&list),
            None => enabled_addons(io, ctx, kind, row),
        },
        Some(Mode::Addon(row)) => enabled_addons(io, ctx, kind, row),
        Some(Mode::Conky) => conky_running_ids(io, ctx),
        None => Vec::new(),
    }
}

/// A GNOME shell call step (`EnableExtension`/`DisableExtension`) through
/// `gdbus`, by the same executor as the other button steps.
fn gnome_shell_call(method: &str, uuid: &str) -> crate::ext_manager::ExtStep {
    crate::ext_manager::ExtStep::Run {
        argv: argv(&[
            "gdbus",
            "call",
            "--session",
            "--dest",
            crate::gnome_shell_ext::BUS_NAME,
            "--object-path",
            crate::gnome_shell_ext::OBJECT_PATH,
            "--method",
            &format!("{}.{method}", crate::gnome_shell_ext::INTERFACE),
            uuid,
        ]),
        timeout_secs: Some(30),
    }
}

// ============================================================================
// Executing a button
// ============================================================================

/// Plan steps from the extension installer: execute them for real.
fn run_steps(io: &dyn LookIo, steps: &[crate::ext_manager::ExtStep]) -> Option<String> {
    let steps: Vec<crate::install_plan::Step> = steps.iter().cloned().map(Into::into).collect();
    match exec_plan(io, &steps, &mut crate::install_plan::Undo::off()) {
        Ok(err) => err,
        Err(f) => Some(crate::install_plan::failure_text(&f)),
    }
}

/// Execute a section button's steps with the COMMON executor
/// ([`crate::install_plan::exec_undoable`]) under the `undo` rollback;
/// commands and fire-and-forget launches go through `io` (`io.run`,
/// `io.spawn`: in checks it is a fake and nothing is launched), so the
/// machine substitution is the same as before.
///
/// ALL steps are executed; a command failure does not abort the run but is
/// returned first (`Ok(Some(text))`). The measure of success is not the
/// command codes but what the environment returns after the write
/// ([`check`]): refreshing the environment (`qdbus … reconfigure`)
/// legitimately fails when KWin is not running, while the key has been
/// written. `Err` is a failure of a file step (writes under rollback,
/// [`crate::themes::ApplyStep::Guard`]).
pub fn exec_plan(
    io: &dyn LookIo,
    steps: &[crate::install_plan::Step],
    undo: &mut crate::install_plan::Undo,
) -> Result<Option<String>, crate::install_plan::Failure> {
    use crate::utils::{RunOutcome, RunReport};
    let first: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
    let run = |argv: &[String], _timeout: Option<u64>| -> RunReport {
        let (ok, out) = io.run(argv);
        // Reading the previous value for the "what was there" snapshot is not a
        // reason for failure: for an xfconf property that does not exist yet it
        // is supposed to fail.
        if !ok && first.borrow().is_none() && !crate::install_plan::is_value_read(argv) {
            let tool = argv.first().map(String::as_str).unwrap_or("");
            let detail = out.trim();
            *first.borrow_mut() = Some(if detail.is_empty() { tool.to_string() } else { format!("{tool}: {detail}") });
        }
        RunReport {
            outcome: if ok { RunOutcome::Ok } else { RunOutcome::Failed },
            returncode: Some(if ok { 0 } else { 1 }),
            stdout: if ok { out.into_bytes() } else { Vec::new() },
            ..Default::default()
        }
    };
    let have = |tool: &str| io.have(tool);
    // Refreshing the environment (`cinnamon --replace`) is launched and not waited for.
    let spawn = |argv: &[String]| {
        io.spawn(argv);
    };
    let ctx = crate::install_plan::Ctx {
        install: true,
        steamos: false,
        privileged: None,
        run: &run,
        have: &have,
        spawn: &spawn,
        wayland: false,
        sync_env: &crate::install_plan::no_sync_env,
    };
    crate::install_plan::exec_undoable(steps, &mut Vec::new(), &ctx, undo)?;
    Ok(first.into_inner())
}

/// What to check after the write: the environment's truth, not what was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// A theme: the environment returns `name`.
    Theme { kind: String, name: String },
    /// The add-on was enabled (`was_on`): after the press it must become the
    /// opposite.
    Addon { kind: String, name: String, was_on: bool },
}

/// Plan of a section button press ([`press_plan`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Refusal before the first step: nothing was written.
    Refused(String),
    /// Steps of the common plan, then the [`check`].
    Steps { steps: Vec<crate::install_plan::Step>, check: Check },
    /// Conky: starting and stopping processes its own way ([`press`]).
    Own,
}

/// Files the section button commands will rewrite (`kwriteconfig6 --file
/// …`, the ini editing script): their "what was there" is taken by the
/// [`crate::themes::ApplyStep::Guard`] step. `gsettings` and
/// `xfconf-query` writes are held by services (dconf, xfconfd), so there
/// is nothing to snapshot there.
fn written_files(ctx: &Ctx, row: &Row) -> Vec<String> {
    match row.activate {
        Activate::KdeConfig { file, .. } => vec![ctx.config(file)],
        Activate::KwinPlugin => vec![ctx.config("kwinrc")],
        Activate::IniKey { file, .. } => vec![format!("{}/{file}", ctx.home)],
        _ => Vec::new(),
    }
}

fn plan_steps(ctx: &Ctx, row: &Row, steps: Vec<crate::ext_manager::ExtStep>, check: Check) -> Plan {
    let guard = written_files(ctx, row);
    let mut out: Vec<crate::install_plan::Step> = Vec::new();
    if !guard.is_empty() && !steps.is_empty() {
        out.push(crate::themes::ApplyStep::Guard { paths: guard }.into());
    }
    out.extend(steps.into_iter().map(Into::into));
    Plan::Steps { steps: out, check }
}

/// Plan of a section button press, without a single write or launch (only
/// probes: whether the add-on is enabled now, whether the User Themes
/// schema exists, whether Marco will load the theme). A theme: enable it;
/// an add-on: toggle it according to the environment's truth; Conky: its
/// own way ([`Plan::Own`]).
pub fn press_plan(io: &dyn LookIo, ctx: &Ctx, kind: &str, name: &str) -> Plan {
    match mode(kind) {
        Some(Mode::Theme(row)) => {
            if kind == MARCO_KIND {
                // The key is not written at all: Marco will not load such a theme
                // and will keep the old frames, while the tab would show "Applied".
                if let Some(problem) = marco_theme_problem(io, ctx, name) {
                    return Plan::Refused(marco_refusal(name, &problem));
                }
            }
            if let Some(reason) = missing_user_themes(io, row) {
                return Plan::Refused(trf("Couldn't apply “%1”: %2", &[&name, &reason]));
            }
            if kind == OPENBOX_KIND {
                // Our own `rc.xml` is written by a file write step, under the press
                // rollback, like a write done by hand.
                return match rc_theme_steps(io, ctx, name) {
                    Ok(steps) => Plan::Steps {
                        steps: steps.into_iter().map(Into::into).collect(),
                        check: Check::Theme { kind: kind.to_string(), name: name.to_string() },
                    },
                    Err(why) => Plan::Refused(why),
                };
            }
            let steps = enable_steps(io, ctx, kind, name);
            plan_steps(ctx, row, steps, Check::Theme { kind: kind.to_string(), name: name.to_string() })
        }
        Some(Mode::Addon(row)) => {
            let was_on = enabled_addons(io, ctx, kind, row).iter().any(|n| n == name);
            let steps = if was_on {
                disable_steps(io, ctx, kind, name)
            } else {
                enable_steps(io, ctx, kind, name)
            };
            plan_steps(ctx, row, steps, Check::Addon { kind: kind.to_string(), name: name.to_string(), was_on })
        }
        Some(Mode::Conky) => Plan::Own,
        None => Plan::Refused(trf("“%1” can't be applied here", &[&name])),
    }
}

/// The check after the steps: what the environment returns, `(success,
/// message)`. `err` is the first command failure ([`exec_plan`]): the
/// refusal names it.
pub fn check(io: &dyn LookIo, ctx: &Ctx, check: &Check, err: Option<String>) -> (bool, String) {
    match check {
        Check::Theme { kind, name } => {
            let Some(Mode::Theme(row)) = mode(kind) else {
                return (false, trf("“%1” can't be applied here", &[name]));
            };
            // The truth is what the environment returns after the write, not what was written.
            match theme_value(io, ctx, row) {
                Some(v) if v == *name => (true, trf("✅ Applied: %1", &[name])),
                _ => (false, trf("Couldn't apply “%1”: %2", &[name, &not_accepted(err)])),
            }
        }
        Check::Addon { kind, name, was_on } => {
            let Some(Mode::Addon(row)) = mode(kind) else {
                return (false, trf("“%1” can't be applied here", &[name]));
            };
            let now_on = enabled_addons(io, ctx, kind, row).iter().any(|n| n == name);
            if now_on == *was_on {
                return (false, trf("Couldn't turn “%1” on or off: %2", &[name, &not_accepted(err)]));
            }
            let mut msg = if now_on {
                trf("▶️ Enabled: %1", &[name])
            } else {
                trf("⏸️ Disabled: %1", &[name])
            };
            // When this becomes visible: the same condition as in the section label.
            if let Some(note) = refresh_note(io, row, ctx, name) {
                msg.push_str(" — ");
                msg.push_str(&note);
            }
            (true, msg)
        }
    }
}

/// The "the environment did not accept it" refusal, with the command's reason if there was one.
fn not_accepted(err: Option<String>) -> String {
    err.unwrap_or_else(|| tr("the desktop didn't accept the setting"))
}

/// The planner environment for the enable steps: only the home and the
/// program lookup are needed from it (there are no copy steps here).
fn with_ext_env<R>(io: &dyn LookIo, ctx: &Ctx, body: impl FnOnce(&crate::ext_manager::ExtEnv) -> R) -> R {
    use crate::ext_manager::{ExtEnv, FsSnap, TmpAlloc};
    let fs = FsSnap::from_json(&serde_json::json!({}));
    let tmp = TmpAlloc::new("/tmp/rexon-de-apply");
    let which = |name: &str| io.have(name).then(|| name.to_string());
    let run_ok = |_: &[String]| false;
    // The running desktop comes ONLY from the context, like the session kind
    // below: both come from one source. Previously, with an unrecognised
    // desktop (`ctx.de` empty), the process's `XDG_CURRENT_DESKTOP` was mixed
    // in while the session kind stayed from the context: on a foreign home
    // (`Ctx::for_home`) run from a Cinnamon session this gave "Cinnamon X11",
    // and the plan got an extra `cinnamon --replace` (the parity check turned
    // red depending on the desktop it was run on). The live path loses
    // nothing from this: `Ctx::live` has already recognised the desktop by
    // the same variable; empty means the desktop is not recognised and there
    // is nothing to restart.
    let xdg = ctx.de.clone();
    // The session kind also comes from the context: `Ctx::live` has already
    // taken into account both `WAYLAND_DISPLAY` and `XDG_SESSION_TYPE`. The
    // process variable here made the plan depend on the desktop it was built
    // on: the check for Cinnamon X11 lost `cinnamon --replace` if it was run
    // from a Wayland session.
    let session = if ctx.wayland { "wayland" } else { "x11" };
    let env = ExtEnv {
        home: &ctx.home,
        fs: &fs,
        which: &which,
        run_ok: &run_ok,
        xdg_current_desktop: &xdg,
        xdg_session_type: session,
        wayland_display: ctx.wayland,
        stage_helper_available: false,
        system_helper_path: "",
        system_helper_exists: false,
        helper_family: "arch",
        tmp: &tmp,
    };
    body(&env)
}

/// Steps that enable an object with the button (visible to parity checks).
pub fn enable_steps(io: &dyn LookIo, ctx: &Ctx, kind: &str, name: &str) -> Vec<crate::ext_manager::ExtStep> {
    if kind == NEMO_KIND {
        let (schema, key) = NEMO_DISABLED;
        return vec![sh(dec::gsettings_list_remove_script(schema, key, &format!("'{name}'")))];
    }
    if kind == OPENBOX_KIND {
        return rc_theme_steps(io, ctx, name).unwrap_or_default();
    }
    // The live shell enables it by itself and immediately; the script is the fallback.
    if kind == crate::gnome_ext_section::KIND && io.gnome_shell_extensions().is_some() {
        return vec![gnome_shell_call("EnableExtension", name)];
    }
    with_ext_env(io, ctx, |env| crate::ext_manager::de_enable_steps(env, kind, name))
}

/// Steps that disable an object with the button (visible to parity checks).
pub fn disable_steps(io: &dyn LookIo, ctx: &Ctx, kind: &str, name: &str) -> Vec<crate::ext_manager::ExtStep> {
    if kind == NEMO_KIND {
        let (schema, key) = NEMO_DISABLED;
        return vec![sh(dec::gsettings_list_add_script(schema, key, name, &format!("'{name}'")))];
    }
    if kind == crate::gnome_ext_section::KIND && io.gnome_shell_extensions().is_some() {
        return vec![gnome_shell_call("DisableExtension", name)];
    }
    with_ext_env(io, ctx, |env| crate::ext_manager::de_disable_steps(env, kind, name))
}

fn sh(script: String) -> crate::ext_manager::ExtStep {
    crate::ext_manager::ExtStep::Run {
        argv: vec!["bash".into(), "-c".into(), script],
        timeout_secs: Some(30),
    }
}

/// Whether the add-on is enabled now (the environment's truth).
pub fn addon_enabled(io: &dyn LookIo, ctx: &Ctx, kind: &str, name: &str) -> bool {
    match mode(kind) {
        Some(Mode::Addon(row)) => enabled_addons(io, ctx, kind, row).iter().any(|n| n == name),
        _ => false,
    }
}

/// Enable or disable an add-on (the "Extensions" tab: the whole package).
pub fn set_addon(io: &dyn LookIo, ctx: &Ctx, kind: &str, name: &str, on: bool) -> Result<(), String> {
    let steps = if on {
        enable_steps(io, ctx, kind, name)
    } else {
        disable_steps(io, ctx, kind, name)
    };
    let err = run_steps(io, &steps);
    if addon_enabled(io, ctx, kind, name) == on {
        Ok(())
    } else {
        Err(format!("{name}: {}", not_accepted(err)))
    }
}

/// A section button press: apply a theme, enable/disable an add-on,
/// start/stop Conky. `(success, message for the person)`.
///
/// For add-ons and Conky the direction is decided NOT by the button but by
/// the environment: enabled now means we disable, otherwise we enable. The
/// tile may be stale (the person may have toggled it in the environment's
/// own settings), and the button must do what is truthfully written on it.
///
/// A theme and an add-on go through the plan ([`press_plan`]), the common
/// executor ([`exec_plan`], here without rollback: the tab's "Apply"
/// executes the same plan under rollback itself) and the check
/// ([`check`]).
pub fn press(io: &dyn LookIo, ctx: &Ctx, kind: &str, name: &str) -> (bool, String) {
    match press_plan(io, ctx, kind, name) {
        Plan::Refused(m) => (false, m),
        Plan::Steps { steps, check: c } => match exec_plan(io, &steps, &mut crate::install_plan::Undo::off()) {
            Ok(err) => check(io, ctx, &c, err),
            Err(f) => (false, trf("Couldn't apply “%1”: %2", &[&name, &crate::install_plan::failure_text(&f)])),
        },
        Plan::Own => {
            if conky_running_ids(io, ctx).iter().any(|i| i == name) {
                conky_stop(io, ctx, name)
            } else {
                conky_start(io, ctx, name)
            }
        }
    }
}

/// A GNOME shell theme without the User Themes extension: there is no
/// [`USER_THEME_SCHEMA`] schema, `gsettings set` refuses with "No such
/// schema", and the person must be told what is missing rather than given
/// a retelling of the gsettings error. `None` means the type is not about
/// the shell, the schema exists, or there is nobody to ask (without
/// gsettings the write fails by itself and names the reason).
fn missing_user_themes(io: &dyn LookIo, row: &Row) -> Option<String> {
    let Activate::Gsettings { schema, .. } = row.activate else { return None };
    if schema != USER_THEME_SCHEMA || !io.have("gsettings") {
        return None;
    }
    if io.run(&argv(&["gsettings", "list-keys", schema])).0 {
        return None;
    }
    // Shell extensions live in an "Appearance" section, not in the
    // "Extensions" tab: that one holds only .rxn packages.
    let place = format!("{} → {}", tr("Appearance"), crate::i18n::trc("appearance section", "GNOME Extensions"));
    Some(trf(
        // TRANSLATORS: %1 is the place in Rexon, e.g. “Appearance → GNOME Extensions”.
        "GNOME Shell themes need the User Themes extension — install it and turn it on in “%1”",
        &[&place],
    ))
}

/// The `metacity-1` directories of theme `name` in Marco's search order:
/// `~/.themes`, `$XDG_DATA_HOME/themes`, then the `$XDG_DATA_DIRS`
/// directories.
pub fn marco_theme_dirs(ctx: &Ctx, name: &str) -> Vec<String> {
    let data_home = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), &ctx.home);
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    let mut roots = vec![format!("{}/.themes", ctx.home), format!("{data_home}/themes")];
    roots.extend(data_dirs.split(':').filter(|d| !d.is_empty()).map(|d| format!("{d}/themes")));
    roots.into_iter().map(|r| format!("{r}/{name}/metacity-1")).collect()
}

/// Why Marco will not load a frame theme
/// ([`crate::local_preview::marco_theme_problem`]); `None` means no obstacle.
pub fn marco_theme_problem(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<crate::local_preview::MarcoProblem> {
    crate::local_preview::marco_theme_problem(&marco_theme_dirs(ctx, name), &|path| io.read(path))
}

/// The frame button label for the person (the exact button function is in
/// parentheses next to it, so for the "un…" pairs the main button's label
/// is enough). The labels of the common buttons are the same as in the
/// title bar buttons settings ([`crate::window_buttons::MANAGED`]).
fn marco_button_label(function: &str) -> String {
    let managed = match function {
        "above" | "unabove" => "above",
        "stick" | "unstick" => "sticky",
        other => other,
    };
    if let Some((_, label)) = crate::window_buttons::MANAGED.iter().find(|(id, _)| *id == managed) {
        return tr(label);
    }
    match function {
        "appmenu" => crate::i18n::trc("window frame button", "Application menu"),
        "shade" | "unshade" => crate::i18n::trc("window frame button", "Roll up into title bar"),
        other => other.to_string(),
    }
}

/// The refusal to apply a frame theme Marco will not load, for the person.
pub fn marco_refusal(name: &str, problem: &crate::local_preview::MarcoProblem) -> String {
    use crate::local_preview::MarcoProblem;
    match problem {
        MarcoProblem::UnknownButton(function) => trf(
            "Marco can't load “%1”: the theme has a “%2” button Marco doesn't know, so MATE would keep the old window borders",
            &[&name, function],
        ),
        MarcoProblem::MissingButton(function) => {
            let label = marco_button_label(function);
            let button = if label == *function { label } else { format!("{label} ({function})") };
            trf(
                // TRANSLATORS: %2 is a window frame button, e.g. “Roll up into title bar (shade)”.
                "Marco doesn't accept “%1”: the theme has no “%2” button, which Marco requires, so MATE would keep the old window borders",
                &[&name, &button],
            )
        }
        MarcoProblem::MissingPart(part) => trf(
            // TRANSLATORS: %2 is a part of the theme file as written there, e.g. “<author>”.
            "Marco doesn't accept “%1”: the theme file lacks the required part %2, so MATE would keep the old window borders",
            &[&name, part],
        ),
    }
}

/// When the change becomes visible, if not right away.
///
/// GNOME enables and disables an extension immediately if it already knows
/// it: it was in place at login (verified on gnome-shell 51.1,
/// 16.10.2026). A re-login on Wayland is needed only for an extension
/// installed after login, which the shell has not seen yet.
fn refresh_note(io: &dyn LookIo, row: &Row, ctx: &Ctx, name: &str) -> Option<String> {
    use dec::Refresh;
    match row.refresh {
        Refresh::GnomeShell if ctx.wayland && !gnome_shell_knows(io, name) => {
            Some(tr("on Wayland GNOME picks this up after you log out and back in"))
        }
        Refresh::CinnamonReplace if ctx.wayland => {
            Some(tr("Cinnamon picks this up after you log out and back in"))
        }
        _ => None,
    }
}

/// Whether the running GNOME shell knows extension `uuid`. No answer means
/// "does not know": an extra caveat about re-login is safer than a false
/// "done".
fn gnome_shell_knows(io: &dyn LookIo, uuid: &str) -> bool {
    let (ok, out) = io.run(&argv(&[
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
    ]));
    // An unknown uuid gives an empty dictionary `(@a{sv} {},)`, a known one comes with `state`.
    ok && out.contains("'state': <")
}

// ============================================================================
// Conky
// ============================================================================

/// The Conky widgets directory (the installation puts them there).
pub fn conky_root(ctx: &Ctx) -> String {
    dec::row(CONKY_KIND)
        .map(|r| r.dest_path(&ctx.home))
        .unwrap_or_else(|| format!("{}/.config/conky", ctx.home))
}

// ── Several widgets in one package ──────────────────────────────────────
//
// A normal Conky package (`theme-conky`, not a set) lies in one directory
// `<root>/<id>`. If its `data/` has no settings file of its own, but there
// are two or more subfolders with settings files (`data/A`, `data/B`),
// these are several widgets: each is its own tile named `<id>/A` (start,
// autostart, removal, "Colours", path repair all go by it). The files lie
// in one place, `<root>/<id>/A`; the former second copy in `<root>/A` is
// no longer placed (defect of 14.11.2026). The decision is made from the
// installed copy of the package (`<configs>/rexon/extensions/<id>`), not
// from what remains in `<root>/<id>`: a removed widget does not change
// the names of the others.

/// A name without a slash and not `.`/`..`.
fn conky_plain_name(n: &str) -> bool {
    !n.is_empty() && n != "." && n != ".." && !n.contains('/')
}

/// Nested widgets of package `top` (`["top/A", "top/B"]`); empty means the
/// package (or a directory not from a package) is one widget `top`.
pub fn conky_nested_widgets(io: &dyn LookIo, ctx: &Ctx, top: &str) -> Vec<String> {
    if !conky_plain_name(top) {
        return Vec::new();
    }
    let pkg = ctx.config(&format!("rexon/extensions/{top}"));
    let Some(manifest) = io
        .read(&format!("{pkg}/manifest.json"))
        .and_then(|t| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&t).ok())
    else {
        return Vec::new();
    };
    if manifest.get("type").and_then(|v| v.as_str()) != Some(CONKY_KIND)
        || manifest.get("id").and_then(|v| v.as_str()) != Some(top)
        || manifest.get(crate::conky_sets::SET_MANIFEST_KEY).is_some_and(|v| v.is_object())
    {
        return Vec::new();
    }
    let data = format!("{pkg}/data");
    let mut names = io.list_dir(&data);
    names.sort();
    let conf_named = |f: &str| {
        f == "conky.conf" || f.ends_with(".conkyrc") || (f.to_lowercase().contains("conky") && f.ends_with(".conf"))
    };
    let top_conf = crate::conky_scan::config_files(io, &data)
        .iter()
        .any(|c| c.rsplit_once('/').is_some_and(|(d, _)| d == data))
        || names.iter().any(|f| conf_named(f) && io.list_dir(&format!("{data}/{f}")).is_empty());
    if top_conf {
        return Vec::new();
    }
    let subs: Vec<String> = names
        .into_iter()
        .filter(|n| conky_plain_name(n) && !n.starts_with('.'))
        .filter(|n| !crate::conky_scan::config_files(io, &format!("{data}/{n}")).is_empty())
        .map(|n| format!("{top}/{n}"))
        .collect();
    if subs.len() < 2 {
        return Vec::new();
    }
    subs
}

/// The widget for a path under the Conky root: `<root>/<name>` is
/// `<name>`, `<root>/<id>/<subfolder>` of a nested widget is
/// `<id>/<subfolder>` ([`conky_nested_widgets`]). Deeper paths give the
/// widget the path belongs to if `deeper`; otherwise `None`.
fn conky_widget_under_root(io: &dyn LookIo, ctx: &Ctx, path: &str, deeper: bool) -> Option<String> {
    let root = format!("{}/", conky_root(ctx));
    let rest = path.trim_end_matches('/').strip_prefix(&root)?;
    let mut parts = rest.split('/');
    let top = parts.next().filter(|t| conky_plain_name(t))?;
    let Some(sub) = parts.next() else { return Some(top.to_string()) };
    let more = parts.next().is_some();
    let nested = format!("{top}/{sub}");
    if conky_plain_name(sub) && conky_nested_widgets(io, ctx, top).contains(&nested) {
        return (deeper || !more).then_some(nested);
    }
    deeper.then(|| top.to_string())
}

/// Whether `path` is a widget folder in the Rexon directory (own or
/// nested): the widget name; otherwise `None`.
pub fn conky_widget_of_dir(io: &dyn LookIo, ctx: &Ctx, path: &str) -> Option<String> {
    conky_widget_under_root(io, ctx, path, false)
}

/// The Rexon widget that settings file `conf` belongs to (by path, without
/// walking the disk except for the package description). `None` means the
/// file is not in a widget folder.
pub fn conky_widget_of_conf(io: &dyn LookIo, ctx: &Ctx, conf: &str) -> Option<String> {
    let root = format!("{}/", conky_root(ctx));
    if !conf.strip_prefix(&root).is_some_and(|r| r.contains('/')) {
        return None;
    }
    conky_widget_under_root(io, ctx, conf, true)
}

/// A widget's settings file: `conky.conf`, `.conkyrc`, `*.conkyrc`, then
/// any `*conky*.conf` one level down: the same order as the installation.
///
/// A widget found on disk outside the Rexon directory (the name is an
/// absolute path, `conky_scan`) and a theme whose file is named anything
/// at all (`Jhalak.conf`, `Gotham`) are found by content
/// ([`crate::conky_scan::config_files`]).
pub fn conky_conf(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Option<String> {
    if crate::conky_scan::is_found_name(id) {
        return crate::conky_scan::widget_confs(io, ctx, id, None).into_iter().next();
    }
    let dir = format!("{}/{id}", conky_root(ctx));
    let mut names = io.list_dir(&dir);
    names.sort();
    if let Some(f) = names
        .iter()
        .find(|f| *f == "conky.conf" || *f == ".conkyrc" || f.ends_with(".conkyrc"))
    {
        return Some(format!("{dir}/{f}"));
    }
    let looks = |f: &str| {
        f.to_lowercase().contains("conky") && (f.ends_with(".conf") || f.ends_with(".conkyrc"))
    };
    if let Some(f) = names.iter().find(|f| looks(f)) {
        return Some(format!("{dir}/{f}"));
    }
    for sub in &names {
        let sub_dir = format!("{dir}/{sub}");
        let mut inner = io.list_dir(&sub_dir);
        inner.sort();
        if let Some(f) = inner.iter().find(|f| looks(f)) {
            return Some(format!("{sub_dir}/{f}"));
        }
    }
    crate::conky_scan::config_files(io, &dir).into_iter().next()
}

/// The settings file from a conky process command line (`-c X`,
/// `--config=X`, `--config X`). `None` means it is not conky or it runs
/// without its own file. A conky started with a Rexon overlay answers with
/// the author's file ([`crate::conky_overlay::original_of_launch`]): for
/// "Stop", "Running" and autostart it is the same widget.
pub fn conky_conf_of_cmdline(cmdline: &str) -> Option<String> {
    let launch = conky_launch_of_cmdline(cmdline)?;
    Some(crate::conky_overlay::original_of_launch(&launch).unwrap_or(launch))
}

/// The file from conky's `-c` command line as is (an overlay stays an overlay).
pub fn conky_launch_of_cmdline(cmdline: &str) -> Option<String> {
    let args: Vec<&str> = cmdline.split('\0').filter(|a| !a.is_empty()).collect();
    let prog = args.first()?.rsplit('/').next()?;
    if prog != "conky" {
        return None;
    }
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if *a == "-c" || *a == "--config" {
            return it.next().map(|s| s.to_string());
        }
        if let Some(v) = a.strip_prefix("--config=") {
            return Some(v.to_string());
        }
        if let Some(v) = a.strip_prefix("-c").filter(|v| !v.is_empty()) {
            return Some(v.to_string());
        }
    }
    None
}

/// conky processes: `(pid, settings file)`.
fn conky_processes(io: &dyn LookIo) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pid in io.list_dir("/proc") {
        if !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Some(cmd) = io.read(&format!("/proc/{pid}/cmdline")) else { continue };
        // A copy on the invisible screenshot screen (`conky_preview`) is not a widget.
        if conky_launch_of_cmdline(&cmd).is_some_and(|l| crate::conky_preview::is_preview_launch(&l)) {
            continue;
        }
        if let Some(conf) = conky_conf_of_cmdline(&cmd) {
            out.push((pid, conf));
        }
    }
    out
}

/// The widget a settings file belongs to (`<root>/<id>/…`).
fn conky_id_of_conf(io: &dyn LookIo, ctx: &Ctx, conf: &str) -> Option<String> {
    conky_widget_of_conf(io, ctx, conf)
}

/// Conky widgets running now.
pub fn conky_running_ids(io: &dyn LookIo, ctx: &Ctx) -> Vec<String> {
    let mut ids = Vec::new();
    for (_, conf) in conky_processes(io) {
        if let Some(id) = conky_id_of_conf(io, ctx, &conf) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    // Those found on disk (`~/.conky`, `~/.conkyrc`…) go by path.
    let extra = crate::conky_scan::extra_dirs_for(ctx);
    // Windows of a theme with a script go by marker: a window file can lie outside its folder.
    for p in crate::conky_procs::all(io) {
        if let Some(dir) = p.widget.as_deref() {
            let id = crate::conky_theme::widget_of_dir(ctx, &extra, dir);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    for owner in crate::conky_scan::running_found(io, ctx, &extra) {
        if !ids.contains(&owner) {
            ids.push(owner);
        }
    }
    ids
}

// ── "Foreign folder" paths in a widget's files ──────────────────────────
//
// Store themes were written for installation into `~/.conky/<Name>/`
// (that is how the Conky manager program's archives and the authors'
// instructions laid them out) and hardcode absolute paths there:
// `lua_load ~/.conky/<Name>/rings.lua`, `${image
// ~/.conky/<Name>/bg.png}`, `$HOME/.conky/<Name>/weather.sh`, or even
// `/home/<author>/.conky/…`. Rexon installs the widget into
// `~/.config/conky/<id>/`, and such paths miss. The repair happens in the
// widget's own files: a path that leads nowhere, but whose "tail" exists
// in the widget folder, is rewritten to that folder. Everything else is
// left alone:
//   * a path that already exists (the person really has
//     `~/.conky/<Name>`) stays as is;
//   * a path that already leads to the widget folder too (a repeated
//     repair changes nothing, so it can be called before every start);
//   * no "tail" in the widget folder: not our file, the edit cannot be
//     guessed;
//   * the substitution would put a space or a quote where there was none,
//     so the line would split into two arguments; we leave it.
// Nothing is created in `~/.conky`: other people's things there are not
// touched, and when the widget is removed there is nothing to clean up.

/// Roots the themes were written for (after `~/`, `$HOME/` or
/// `/home/<someone>/`).
const CONKY_FOREIGN_ROOTS: &[&str] = &[".conky/", ".Conky/", ".config/conky/", ".config/Conky/"];

/// The end of a path in config, Lua or script text.
fn conky_path_end(c: char) -> bool {
    c.is_whitespace() || "'\"`}{)(][,;|<>&$\\=".contains(c)
}

/// The start of a `~/`, `$HOME/`, `${HOME}/` or `/home/<name>/` path at
/// position `i` of string `text`: `(prefix length, absolute home prefix or
/// None meaning "this person's home")`.
fn conky_home_prefix(text: &str, i: usize) -> Option<(usize, Option<String>)> {
    let rest = &text[i..];
    for p in ["~/", "$HOME/", "${HOME}/"] {
        if rest.starts_with(p) {
            return Some((p.len(), None));
        }
    }
    let user = rest.strip_prefix("/home/")?;
    let n = user
        .find(|c: char| !(c.is_ascii_alphanumeric() || "._-".contains(c)))
        .unwrap_or(user.len());
    if n == 0 || !user[n..].starts_with('/') {
        return None;
    }
    Some((6 + n + 1, Some(format!("/home/{}", &user[..n]))))
}

/// Repair the paths of one text. `home` is the home folder, `widget` the
/// widget folder (absolute, without a trailing `/`), `exists`/`is_dir` are
/// disk observations. `None` means nothing to change.
pub fn conky_repair_text(
    text: &str,
    home: &str,
    widget: &str,
    exists: &dyn Fn(&str) -> bool,
    is_dir: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut last = 0;
    let mut i = 0;
    while i < text.len() {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let prev_ok = text[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || "_./~$-".contains(c)));
        let found = prev_ok.then(|| conky_home_prefix(text, i)).flatten();
        let Some((plen, author_home)) = found else {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            continue;
        };
        let after = &text[i + plen..];
        let Some(root) = CONKY_FOREIGN_ROOTS.iter().find(|r| after.starts_with(**r)) else {
            i += plen;
            continue;
        };
        let tail_all = &after[root.len()..];
        // In quotes a path runs up to the closing quote (spaces in it are part
        // of the name); without quotes up to the first separator.
        let quote = text[..i].chars().next_back().filter(|c| *c == '"' || *c == '\'');
        let tail_len = match quote {
            Some(q) => tail_all.find(|c: char| c == q || c == '\n'),
            None => tail_all.find(conky_path_end),
        }
        .unwrap_or(tail_all.len());
        let tail = &tail_all[..tail_len];
        let end = i + plen + root.len() + tail_len;
        let original_home = author_home.clone().unwrap_or_else(|| home.to_string());
        let original = format!("{original_home}/{root}{tail}");
        let parts: Vec<&str> = tail.split('/').filter(|p| !p.is_empty()).collect();
        let inside = |p: &str| p == widget || p.starts_with(&format!("{widget}/"));
        let pick = (!parts.is_empty() && !exists(&original) && !inside(&original))
            .then(|| {
                // First a tail that exists in full, starting from the longest.
                (0..parts.len())
                    .map(|k| parts[k..].join("/"))
                    .find(|s| exists(&format!("{widget}/{s}")))
                    // Then a file the widget writes itself (weather, cache):
                    // it does not exist yet, but its folder does.
                    .or_else(|| {
                        (0..parts.len().saturating_sub(1))
                            .find(|&k| is_dir(&format!("{widget}/{}", parts[k..parts.len() - 1].join("/"))))
                            .map(|k| parts[k..].join("/"))
                    })
            })
            .flatten();
        if let Some(sub) = pick {
            let slash = if tail.ends_with('/') { "/" } else { "" };
            let target = format!("{widget}/{sub}{slash}");
            // The same kind of path start as the author's: `~/…` stays `~/…`.
            let head = &text[i..i + plen];
            let written = match (author_home.is_some(), target.strip_prefix(&format!("{home}/"))) {
                (false, Some(rel)) => format!("{head}{rel}"),
                _ => target.clone(),
            };
            let breaks = |s: &str| s.contains(|c: char| c.is_whitespace() || "'\"".contains(c));
            let fits = match quote {
                Some(q) => !written.contains(q),
                None => !breaks(&written),
            };
            if fits {
                out.push_str(&text[last..i]);
                out.push_str(&written);
                last = end;
                changed = true;
            }
        }
        i = end.max(i + 1);
    }
    if !changed {
        return None;
    }
    out.push_str(&text[last..]);
    Some(out)
}

/// Repair the paths in all text files of widget folder `widget` (see
/// [`conky_repair_text`]), and for Lua scripts written for the old conky
/// hook up the X11 drawing module
/// ([`crate::conky_safety::lua_xlib_compat`]). Returns the changed files.
/// Links are not followed, binary and large files are not read, files
/// outside the widget folder are not touched.
pub fn conky_repair_paths(home: &str, widget: &str) -> Vec<String> {
    const MAX_FILES: usize = 4000;
    const MAX_SIZE: u64 = 1 << 20;
    let exists = |p: &str| std::path::Path::new(p).exists();
    let is_dir = |p: &str| std::path::Path::new(p).is_dir();
    let mut changed = Vec::new();
    let mut stack = vec![(widget.trim_end_matches('/').to_string(), 0usize)];
    let widget = widget.trim_end_matches('/');
    let mut seen = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            seen += 1;
            if seen > MAX_FILES {
                return changed;
            }
            let Ok(ft) = e.file_type() else { continue };
            let path = e.path().to_string_lossy().into_owned();
            if ft.is_dir() && depth < 8 {
                stack.push((path, depth + 1));
                continue;
            }
            if !ft.is_file() || e.metadata().map_or(true, |m| m.len() > MAX_SIZE) {
                continue;
            }
            // The author's installer (`install.sh`) describes WHERE to put the
            // theme; rewritten, it would copy the widget into itself.
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.starts_with("install") || name.starts_with("uninstall") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            if bytes.contains(&0) {
                continue;
            }
            let Ok(text) = String::from_utf8(bytes) else { continue };
            let paths = conky_repair_text(&text, home, widget, &exists, &is_dir);
            // A Lua script for the old conky: X11 drawing as a separate module.
            let lua = name.ends_with(".lua").then(|| crate::conky_safety::lua_xlib_compat(paths.as_deref().unwrap_or(&text))).flatten();
            if let Some(fixed) = lua.or(paths) {
                if std::fs::write(&path, fixed).is_ok() {
                    changed.push(path);
                }
            }
        }
    }
    changed
}

/// conky's working directory for settings file `conf`: its folder. Widgets
/// write the paths to their files relative to it (`lua_load ./x.lua`,
/// `${image ./bg.png}`, `${execi 60 ./weather.sh}`): conky and the
/// commands it runs look for them relative to the process's working
/// directory. The programs the themes were drawn for start a widget the
/// same way (change into the file's folder, then conky).
pub fn conky_work_dir(conf: &str) -> &str {
    match conf.rsplit_once('/') {
        Some(("", _)) => "/",
        Some((dir, _)) => dir,
        None => ".",
    }
}

/// The launch line of the "Start" button: `(argv, working directory)`.
pub fn conky_start_command(conf: &str) -> (Vec<String>, String) {
    (
        ["conky", "-d", "-c", conf].map(String::from).to_vec(),
        conky_work_dir(conf).to_string(),
    )
}

/// The `sh -c` script for autostart at login: `$0` is the settings file;
/// change into its folder and replace the shell with conky on the same
/// file. The path is not substituted into the script: it arrives as a
/// separate argument, so spaces, quotes and `$` in the folder name need no
/// escaping inside the script. After `exec`, conky itself with
/// `-c <file>` remains in `/proc`, and "is the widget running" is
/// detected the same way as for the button.
///
/// There is deliberately no `%` sign in the script: in `Exec=` it is
/// special, and launchers parse `%%` inside quotes differently.
///
/// A switch to XWayland at the start of the script can be set by
/// [`crate::conky_overlay::XWAYLAND_LAUNCH_VAR`]: then conky runs with the
/// launch file through XWayland (a window under the windows), with the
/// same working directory.
pub const CONKY_AUTOSTART_SCRIPT: &str = r#"cd "$(dirname "$0")" && exec conky -c "${rexon_xw:-$0}""#;

/// The same for a widget with a Rexon overlay: `$0` is the overlay (conky
/// is started with it), `$1` the author's file (its folder is the working
/// directory).
pub const CONKY_OVERLAY_AUTOSTART_SCRIPT: &str = r#"cd "$(dirname "$1")" && exec conky -c "${rexon_xw:-$0}""#;

fn conky_start(io: &dyn LookIo, ctx: &Ctx, id: &str) -> (bool, String) {
    let Some(conf) = conky_conf(io, ctx, id) else {
        return (
            false,
            tr("This Conky widget has no settings file (conky.conf) — there's nothing to start."),
        );
    };
    if !io.have("conky") {
        return (false, conky_missing_refusal());
    }
    conky_prepare(ctx, id);
    // The widget runs commands or scripts, and the person has not allowed
    // this yet (or the files have changed since): do not start it. The
    // interface asks the question before the press
    // (`conky_safety::start_question`); starts that bypass it arrive here,
    // such as "Install and apply" from the store. The check comes AFTER the
    // path repair: it edits files, and a fingerprint taken before it would
    // no longer match at the next start.
    if let Some(why) = crate::conky_safety::start_refusal(io, ctx, id) {
        return (false, why);
    }
    // Start from the widget folder, errors go to the log; if it did not
    // start, the reason in conky's words (`conky_procs::start_conf`).
    crate::conky_procs::start_conf(io, ctx, &crate::conky_scan::widget_title(id), &conf)
}

/// Preparing a widget for start: paths under `~/.conky/<Name>/` are
/// pointed at the widget folder (widgets installed before this repair are
/// fixed here too, on the first start). A widget found on disk already
/// lies where its paths point, so its files are not touched. The interface
/// calls it BEFORE the question about the widget's commands too: the
/// permission remembers the files already repaired. A widget allowed
/// BEFORE the repair stays allowed: the edit is our own, by Rexon, and
/// there is no reason to ask about the same commands a second time.
pub fn conky_prepare(ctx: &Ctx, id: &str) {
    if crate::conky_scan::is_found_name(id) {
        return;
    }
    let io = crate::animations::SystemIo;
    let trusted = crate::conky_safety::trusted_as_is(&io, ctx, id);
    let changed = conky_repair_paths(&ctx.home, &format!("{}/{id}", conky_root(ctx)));
    if trusted && !changed.is_empty() {
        crate::conky_safety::remember(&io, ctx, id);
    }
}

/// The "Start" refusal when the Conky program is missing.
pub fn conky_missing_refusal() -> String {
    tr("The widget wasn't started: it needs the Conky program, and Conky isn't installed.")
}

/// The Conky on Wayland caveat for environment `de_id`. `None` means no
/// caveat is needed.
///
/// On Wayland Conky draws the widget on the desktop through
/// wlr-layer-shell: it is known to KWin, wlroots compositors (Sway,
/// Hyprland, labwc, which Budgie 10.10 runs on too), niri and COSMIC.
/// Mutter (GNOME), Muffin (Cinnamon) and Gala (Pantheon) do not implement
/// it: there the widget goes only through XWayland and appears as a normal
/// window, or does not appear at all. The old caveat "on Wayland it goes
/// through XWayland" was outdated for KDE and the tiling ones, and gave
/// false hope for GNOME.
pub fn conky_wayland_note(de_id: &str, wayland: bool) -> Option<String> {
    conky_wayland_note_for(de_id, wayland, None)
}

/// [`conky_wayland_note`] with the conky version. Since 1.25 conky chooses
/// the output itself (`output_backend`) and on a desktop without layers
/// appears as a normal Wayland window: the warning says exactly that
/// ([`crate::conky_overlay::preset_note`]); for older releases and without
/// a version, the old "unreliable".
pub fn conky_wayland_note_for(de_id: &str, wayland: bool, version: Option<(u32, u32)>) -> Option<String> {
    use crate::de_customization::De;
    if !wayland {
        return None;
    }
    if version.is_some_and(|v| v >= (1, 25)) {
        return crate::conky_overlay::preset_note(de_id, wayland).map(|n| format!("⚠️ {n}"));
    }
    match crate::de_customization::de_from_id(de_id)? {
        De::Gnome | De::Cinnamon | De::Pantheon => Some(tr(
            "⚠️ In the Wayland session of this desktop Conky is unreliable: the widget may not \
             appear or may land in the wrong place. The X11 session (chosen on the login screen) \
             works best.",
        )),
        _ => None,
    }
}

/// The condition label of the "Conky widgets" section: no program means
/// saying so up front, not after the "Start" button; an unreliable Wayland
/// too. `installable`: Rexon can install the package itself (not SteamOS,
/// not an immutable system, a known package manager). `None` means all is
/// fine.
pub fn conky_section_note(installed: bool, installable: bool, de_id: &str, wayland: bool) -> Option<String> {
    let mut parts = Vec::new();
    if !installed {
        parts.push(if installable {
            tr("The Conky program isn't installed yet — Rexon will install it when you start a widget.")
        } else {
            tr("The Conky program isn't installed, and Rexon can't install programs on this system by itself: \
                install the conky package with your system's tools.")
        });
    }
    parts.extend(conky_wayland_note(de_id, wayland));
    // Cinnamon on Wayland: it has its own desktop widgets, and they stay on
    // the desktop in any session: show the way to them (24.10.2026).
    if wayland && crate::de_customization::de_from_id(de_id) == Some(crate::de_customization::De::Cinnamon) {
        parts.push(crate::i18n::trf(
            // TRANSLATORS: %1 is the name of the neighbouring section (“Cinnamon Desklets”).
            "Desklets stay on the desktop in any session — they're in the neighbouring “%1” section.",
            &[&crate::i18n::trc("appearance section", "Cinnamon Desklets")],
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// The Conky program package Rexon can install on THIS system. `None`
/// means it cannot: SteamOS, an immutable system (a system package will
/// not install there) or an unknown package manager; then installation is
/// not offered, only a warning.
pub fn live_conky_package() -> Option<&'static str> {
    let detector = crate::system::DistroDetector::detect_live();
    if detector.is_steamos() || detector.is_immutable() {
        return None;
    }
    conky_package(detector.family_live().as_str())
}

/// Whether this is a Conky widget: a store card (section 124) or a local
/// tile `local:conky:<name>`.
pub fn is_conky_item(item: &crate::models::ThemeItem) -> bool {
    apply_kind_of_local_id(&item.id) == Some("conky")
        || crate::categories::section_id_for(item.category_id) == crate::categories::CONKY_SECTION_ID
}

/// An item of the Conky section that is a program, not a widget: EVERY
/// file of it is a distribution package or an installer
/// ([`crate::ocs::DISTRO_PACKAGE_EXTS`]: `.deb`, `.rpm`, `.pkg.tar.zst`,
/// `.AppImage`…). That is how "Conky Manager GTK" and "Conky Manager Qt"
/// sit in the store's "Conky" category: there is nothing to install as a
/// widget, yet they came first at the top of the showcase. A widget has at
/// least one archive or settings file, and it stays even if a package lies
/// next to it.
pub fn is_conky_program(item: &crate::models::ThemeItem) -> bool {
    if !is_conky_item(item) {
        return false;
    }
    let mut names: Vec<String> = item
        .download_links
        .iter()
        .filter_map(|l| l.get("name").and_then(serde_json::Value::as_str))
        .map(str::to_ascii_lowercase)
        .collect();
    if names.is_empty() && !item.download_name.is_empty() {
        names.push(item.download_name.to_ascii_lowercase());
    }
    !names.is_empty()
        && names
            .iter()
            .all(|n| crate::ocs::DISTRO_PACKAGE_EXTS.iter().any(|e| n.ends_with(e)))
}

/// The Conky package Rexon installs before starting a widget BY ITSELF,
/// without a "Yes/No" question (the owner's decision of 24.10: it is easy
/// to press "No" by mistake in the question; the protection is the polkit
/// password window). `None` means there is nothing to install: Conky is
/// installed, or Rexon cannot install it here (then the section warned up
/// front, and "Start" will say [`conky_missing_refusal`]).
///
/// `have_conky` tells whether `conky` is in PATH, `package` is
/// [`live_conky_package`].
pub fn conky_install_package(have_conky: bool, package: Option<&str>) -> Option<String> {
    if have_conky {
        return None;
    }
    package.map(str::to_string)
}

/// Install Conky the same way as all of Rexon's package installation
/// (pkexec + `rexon-pkg-helper`), only from the official repositories
/// (`appearance_installer::live_install_repo_packages`: the AUR is not
/// built without a question), and check the fact: the `conky` binary in
/// PATH. `Err` is the reason for the person (the widget does not start
/// after it).
pub fn install_conky_live() -> Result<(), String> {
    let Some(package) = live_conky_package() else {
        return Err(tr("Rexon can't install programs on this system by itself."));
    };
    crate::appearance_installer::live_install_repo_packages(
        &[package.to_string()],
        crate::privilege::op::PkgSource::Conky,
    )?;
    if !have_conky() {
        return Err(trf("the %1 package was installed, but the conky program didn't appear", &[&package]));
    }
    Ok(())
}

/// Whether the `conky` program is in the process's PATH.
/// `utils::which(_, None)` searches a fallback list of directories, not
/// `$PATH`: it would not see a Conky installed outside `/usr/bin` (or
/// planted by the test bench).
pub fn have_conky() -> bool {
    crate::utils::which("conky", std::env::var("PATH").ok().as_deref()).is_some()
}

/// Result of a failed Conky installation: the widget is not started, and
/// it says why and what to do. A closed password window means "changed my
/// mind", not a breakage: the advice is to repeat "▶️ Start" and enter the
/// password.
pub fn conky_install_failed(why: &str) -> String {
    if why == crate::appearance_installer::password_not_confirmed() {
        return trf(
            // TRANSLATORS: %1 is the label of the widget's start button.
            "The widget wasn't started: it needs the Conky program. Press “%1” again and enter the \
             password to install it.",
            &[&tr("▶️ Start")],
        );
    }
    trf("Couldn't install Conky: %1. The widget wasn't started.", &[&why])
}

/// The Conky program package for a package family
/// (`system::PkgFamily::as_str`). `None` means Rexon does not install it
/// here. The names were checked against the repositories: Arch (extra),
/// Debian/Ubuntu, Fedora and openSUSE all use `conky`.
pub fn conky_package(family: &str) -> Option<&'static str> {
    match family {
        "arch" | "debian" | "fedora" | "suse" => Some("conky"),
        _ => None,
    }
}

fn conky_stop(io: &dyn LookIo, ctx: &Ctx, id: &str) -> (bool, String) {
    // We kill ONLY this widget's processes, by the process number found by
    // its settings file. Other conky processes (the person's own) are left
    // alone. The message has the widget name, not the path found on disk.
    let title = crate::conky_scan::widget_title(id);
    let pids: Vec<String> = if crate::conky_scan::is_found_name(id) {
        crate::conky_procs::pids_where(io, ctx, &|conf| crate::conky_scan::widget_owns(ctx, id, conf))
    } else {
        conky_processes(io)
            .into_iter()
            .filter(|(_, conf)| conky_id_of_conf(io, ctx, conf).as_deref() == Some(id))
            .map(|(pid, _)| pid)
            .collect()
    };
    // A theme with a launch script goes as a whole: the script's loops and
    // the group (`conky_theme`), even if conky itself has already crashed.
    let mut pids = crate::conky_theme::with_companions(io, pids);
    for p in crate::conky_theme::widget_companions(io, &crate::conky_scan::widget_path(ctx, id)) {
        if !pids.contains(&p) {
            pids.push(p);
        }
    }
    if pids.is_empty() {
        return (true, trf("⏹️ Stopped: %1", &[&title]));
    }
    let (ok, out) = crate::conky_theme::kill_targets(io, &pids);
    if !ok {
        return (false, trf("Couldn't stop “%1”: %2", &[&title, &out.trim().to_string()]));
    }
    (true, trf("⏹️ Stopped: %1", &[&title]))
}

/// Before "Remove": kill the widget's processes (by number, like "Stop")
/// and remove its autostart entry; otherwise the removed widget would keep
/// hanging on the desktop, and at login autostart would call conky with a
/// file that does not exist. `widget_dir` is the path from the
/// installation log; if it is not a widget folder (`<Conky root>/<id>`),
/// nothing is done. `Some(id)` means the widget was recognised.
pub fn conky_forget_widget(io: &dyn LookIo, ctx: &Ctx, widget_dir: &str) -> Option<String> {
    let id = conky_widget_of_dir(io, ctx, widget_dir)?;
    // A package with several widgets: each nested one has its own processes
    // and its own autostart (the folder may already be gone: the name comes
    // from the package).
    if !id.contains('/') {
        for nested in conky_nested_widgets(io, ctx, &id) {
            let _ = conky_stop(io, ctx, &nested);
            conky_autostart_forget(ctx, &nested);
            crate::conky_colors::forget(ctx, &crate::conky_scan::widget_path(ctx, &nested));
        }
    }
    let _ = conky_stop(io, ctx, &id);
    conky_autostart_forget(ctx, &id);
    crate::conky_overlay::forget_under(ctx, widget_dir);
    crate::conky_colors::forget(ctx, widget_dir);
    Some(id)
}

/// The widget's autostart file (the name is the same the old installation
/// wrote, so widgets enabled earlier stay enabled).
pub fn conky_autostart_path(ctx: &Ctx, id: &str) -> String {
    ctx.config(&format!("autostart/rexon-conky-{}.desktop", crate::conky_scan::autostart_key(id)))
}

/// The autostart entry of widget `id` for file `conf` with settings `opts`
/// (the compositor lines are set right here, [`crate::conky_autostart::build`]).
fn conky_autostart_built(io: &dyn LookIo, ctx: &Ctx, id: &str, conf: &str, opts: &crate::conky_autostart::Options) -> crate::conky_autostart::Built {
    // A theme with its own `start.sh` is started by it at login too (`conky_theme`).
    if let Some(plan) = crate::conky_theme::script_launch(io, ctx, id) {
        let env = crate::conky_autostart::session_env(ctx);
        let key = crate::conky_scan::autostart_key(id);
        return crate::conky_autostart::build_script(&env, &key, &crate::conky_scan::widget_title(id), &plan, Some(conf), opts);
    }
    let p = crate::conky_overlay::live_paths(ctx, conf);
    let launch = crate::conky_overlay::autostart_launch(io, &p);
    let xwayland = crate::conky_overlay::autostart_xwayland(io, ctx, &p, conf);
    let env = crate::conky_autostart::session_env(ctx);
    let key = crate::conky_scan::autostart_key(id);
    let title = crate::conky_scan::widget_title(id);
    crate::conky_autostart::build(&env, &key, &title, conf, launch.as_deref(), xwayland.as_deref(), opts)
}

/// Remove the widget's autostart: the entry and the compositor lines.
pub fn conky_autostart_forget(ctx: &Ctx, id: &str) {
    let _ = std::fs::remove_file(conky_autostart_path(ctx, id));
    crate::conky_autostart::remove(&crate::conky_autostart::session_env(ctx), &crate::conky_scan::autostart_key(id));
}

/// Whether the widget's autostart is enabled.
///
/// An entry of the old kind (without changing into the widget folder,
/// without `X-KDE-autostart-after`, without the switch to XWayland) is
/// rewritten along the way to the current kind with the same delay and
/// environment: otherwise a widget enabled earlier would keep starting the
/// old way at login. A current entry is not touched.
pub fn conky_autostart(io: &dyn LookIo, ctx: &Ctx, id: &str) -> bool {
    let path = conky_autostart_path(ctx, id);
    let Some(text) = io.read(&path) else { return false };
    // An entry without the switch to XWayland (before 15.11.2026) or without
    // the launch file through it (a window above the windows) is of the old
    // kind too.
    let script = crate::conky_theme::start_script(io, ctx, id).is_some();
    let old_session = !text.contains(crate::conky_overlay::XWAYLAND_SESSION_TYPE)
        || (!script && !text.contains(crate::conky_overlay::XWAYLAND_LAUNCH_VAR));
    // The theme gained (or lost) its own `start.sh`: the entry goes under a
    // different launch path.
    let wrong_path = script != text.contains(crate::conky_theme::MARKER_ENV);
    if text.contains("\nExec=conky -c ") || !text.contains("\nX-KDE-autostart-after=") || old_session || wrong_path {
        if let Some(conf) = conky_conf(io, ctx, id) {
            let opts = crate::conky_autostart::options_of(&text);
            io.write(&path, &conky_autostart_built(io, ctx, id, &conf, &opts).desktop);
        }
    }
    true
}

/// The "Start at login" toggle. `(success, message)`. A new entry gets the
/// [`crate::conky_autostart::DEFAULT_DELAY`] delay.
pub fn set_conky_autostart(io: &dyn LookIo, ctx: &Ctx, id: &str, on: bool) -> (bool, String) {
    let path = conky_autostart_path(ctx, id);
    if !on {
        let ok = match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        };
        crate::conky_autostart::remove(&crate::conky_autostart::session_env(ctx), &crate::conky_scan::autostart_key(id));
        return if ok {
            (true, trf("Conky widget “%1” won't start at login", &[&id]))
        } else {
            (false, trf("Couldn't change autostart of “%1”", &[&id]))
        };
    }
    let opts = io.read(&path).map(|t| crate::conky_autostart::options_of(&t)).unwrap_or(crate::conky_autostart::Options {
        delay: crate::conky_autostart::DEFAULT_DELAY,
        only_in: Vec::new(),
    });
    set_conky_autostart_options(io, ctx, id, &opts)
}

/// Write the widget's autostart with settings `opts` (this enables it).
/// `(success, message)`; in a compositor session where the launch line did
/// not fit in, the message says what to insert by hand.
pub fn set_conky_autostart_options(io: &dyn LookIo, ctx: &Ctx, id: &str, opts: &crate::conky_autostart::Options) -> (bool, String) {
    let path = conky_autostart_path(ctx, id);
    let Some(conf) = conky_conf(io, ctx, id) else {
        return (
            false,
            tr("This Conky widget has no settings file (conky.conf) — there's nothing to start."),
        );
    };
    // At login the widget will start without Rexon, so the paths are repaired in advance.
    conky_prepare(ctx, id);
    let built = conky_autostart_built(io, ctx, id, &conf, opts);
    if !io.write(&path, &built.desktop) {
        return (false, trf("Couldn't change autostart of “%1”", &[&id]));
    }
    let title = crate::conky_scan::widget_title(id);
    match built.refusal {
        Some(why) => (false, trf("Conky widget “%1” is set to start at login, but not in this session: %2", &[&title, &why])),
        None if built.configs.is_empty() => (true, trf("✅ Conky widget “%1” will start when you log in", &[&title])),
        // TRANSLATORS: %1 is a Conky widget name, %2 a settings file of a window manager (sway, Hyprland…).
        None => (true, trf("✅ Conky widget “%1” will start when you log in — the start line is in %2", &[&title, &built.configs.join(", ")])),
    }
}

// ============================================================================
// Login screen and boot splash: the helper call line
// ============================================================================

/// `pkexec <helper> [--lang=…] apply-<kind> <theme> - <family>`: the
/// system helper's "Apply" mode: the theme is already in
/// `/usr/share/<kind>/themes`, it is only selected. The same helper and the
/// same polkit action as the installation, so the password is asked with
/// the same "one password" (`auth_admin_keep`), and no new prompts appear.
pub fn system_apply_argv(
    helper: &str,
    lang_args: &[String],
    kind: &str,
    theme: &str,
    family: &str,
) -> Vec<String> {
    system_mode_argv(helper, lang_args, "apply", kind, theme, family)
}

/// `pkexec <helper> [--lang=…] remove-<kind> <theme> - <family>`: the
/// "Remove" mode of the same helper: remove
/// `/usr/share/<kind>/themes/<theme>`. The helper itself does not touch the
/// active theme or the system package's files; the check before the
/// password window is the caller's
/// ([`crate::appearance_installer::system_theme_remove_with`]).
pub fn system_remove_argv(
    helper: &str,
    lang_args: &[String],
    kind: &str,
    theme: &str,
    family: &str,
) -> Vec<String> {
    system_mode_argv(helper, lang_args, "remove", kind, theme, family)
}

/// The common call line of helper modes that need no directory.
fn system_mode_argv(
    helper: &str,
    lang_args: &[String],
    mode: &str,
    kind: &str,
    theme: &str,
    family: &str,
) -> Vec<String> {
    let mut v = vec!["pkexec".to_string(), helper.to_string()];
    v.extend(lang_args.iter().cloned());
    v.extend([format!("{mode}-{kind}"), theme.to_string(), "-".to_string(), family.to_string()]);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_follow_the_matrix() {
        for k in ["theme-xfwm4", "theme-marco", "theme-gnome-shell", "theme-cinnamon", "theme-lxqt", "kde-window-switcher"] {
            assert_eq!(mode(k).map(Mode::button), Some(Button::Apply), "{k}");
        }
        for k in [
            "kde-kwin-script", "kde-kwin-effect", "gnome-shell-extension", "cinnamon-applet",
            "cinnamon-desklet", "cinnamon-extension", "nemo-action",
        ] {
            assert_eq!(mode(k).map(Mode::button), Some(Button::Toggle), "{k}");
        }
        assert_eq!(mode("theme-conky").map(Mode::button), Some(Button::Run));
        for k in ["kde-wallpaper-plugin", "theme-gtk", "font-pack", "theme-plasmoid"] {
            assert!(mode(k).is_none(), "{k}");
        }
    }

    #[test]
    fn conky_programs_are_told_from_widgets() {
        use crate::models::ThemeItem;
        let item = |names: &[&str]| ThemeItem {
            id: "1".into(),
            category_id: crate::categories::CONKY_SECTION_ID,
            download_links: names.iter().map(|n| serde_json::json!({"url": "u", "name": n})).collect(),
            ..Default::default()
        };
        // "Conky Manager Qt" from the store: only packages and an AppImage.
        assert!(is_conky_program(&item(&[
            "conky-manager-1.0-2-x86-64.AppImage",
            "conky-manager-q-1.0-2-x86-64.deb",
            "conky-manager-q-1.0-2-x86-64.pkg.tar.zst",
            "conky-manager-q-1.0-2.x86-64.rpm",
        ])));
        // Widgets stay: an archive, a bare settings file, an archive next to a package.
        assert!(!is_conky_program(&item(&["Mimosa-AmethystC-v4.0.zip"])));
        assert!(!is_conky_program(&item(&["conky.conf", "readme.txt"])));
        assert!(!is_conky_program(&item(&["3D-Plus.tar.xz"])));
        assert!(!is_conky_program(&item(&["w.zip", "w-1.0.deb"])));
        // No files, no sign.
        assert!(!is_conky_program(&item(&[])));
        // Not the Conky section: none of our business.
        let mut other = item(&["x.deb"]);
        other.category_id = 107;
        assert!(!is_conky_program(&other));
    }

    #[test]
    fn gsettings_lists_parse() {
        assert_eq!(parse_gsettings_list("@as []"), Vec::<String>::new());
        assert_eq!(parse_gsettings_list("['a@b', 'c']"), vec!["a@b", "c"]);
        assert_eq!(
            entry_uuid("panel1:right:3:weather@mockturtl:12", EntryFmt::CinnamonApplet).as_deref(),
            Some("weather@mockturtl")
        );
        assert_eq!(entry_uuid("clock@cinnamon.org:0:100:100", EntryFmt::CinnamonDesklet).as_deref(), Some("clock@cinnamon.org"));
    }

    /// A settings file with any name is recognised by its content (Jhalak has
    /// `Jhalak.conf`, with the visualiser's `cava.conf` next to it), and the
    /// "Found on disk" tile, a path, leads to its own directory; its autostart
    /// is a file without slashes in the name.
    #[test]
    fn conky_conf_by_content_and_for_found_widgets() {
        use crate::animations::SystemIo;
        let base = std::env::temp_dir().join(format!("rexon-conky-conf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let put = |rel: &str, text: &str| {
            let p = base.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        put(".config/conky/Jhalak/cava.conf", "[general]\nbars = 128\n");
        put(".config/conky/Jhalak/Jhalak.conf", "-- JHALAK\nconky.config = {}\nconky.text = [[x]]\n");
        put(".conky/Gotham/Gotham", "TEXT\n${time}\n");
        put(".conkyrc", "TEXT\n");
        let h = base.to_string_lossy().into_owned();
        let ctx = Ctx { home: h.clone(), xdg_config_home: None, wayland: false, de: String::new() };
        assert_eq!(conky_conf(&SystemIo, &ctx, "Jhalak"), Some(format!("{h}/.config/conky/Jhalak/Jhalak.conf")));
        assert_eq!(conky_conf(&SystemIo, &ctx, &format!("{h}/.conky/Gotham")), Some(format!("{h}/.conky/Gotham/Gotham")));
        assert_eq!(conky_conf(&SystemIo, &ctx, &format!("{h}/.conkyrc")), Some(format!("{h}/.conkyrc")));
        let auto = conky_autostart_path(&ctx, &format!("{h}/.conky/Gotham"));
        let file = auto.rsplit('/').next().unwrap();
        assert!(auto.starts_with(&format!("{h}/.config/autostart/rexon-conky-found-")) && file.ends_with(".desktop"), "{auto}");
        assert_eq!(conky_autostart_path(&ctx, "Jhalak"), format!("{h}/.config/autostart/rexon-conky-Jhalak.desktop"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn conky_cmdline() {
        assert_eq!(conky_conf_of_cmdline("conky\0-d\0-c\0/h/.config/conky/w/conky.conf\0").as_deref(), Some("/h/.config/conky/w/conky.conf"));
        assert_eq!(conky_conf_of_cmdline("/usr/bin/conky\0--config=/x\0").as_deref(), Some("/x"));
        assert_eq!(conky_conf_of_cmdline("bash\0-c\0conky\0"), None);
        let ctx = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        let io = crate::animations::SystemIo;
        assert_eq!(conky_id_of_conf(&io, &ctx, "/h/.config/conky/w/sub/conky.conf").as_deref(), Some("w"));
        assert_eq!(conky_id_of_conf(&io, &ctx, "/h/.config/conky/conky.conf"), None);
    }

    /// The "Start" button runs conky from the settings file's folder: widgets
    /// with `lua_load ./x.lua`, `${image ./bg.png}` and `${execi … ./s.sh}`
    /// look for their files relative to the process's working directory.
    #[test]
    fn conky_starts_in_widget_folder() {
        let (argv, dir) = conky_start_command("/h/.config/conky/My w/sub/conky.conf");
        assert_eq!(argv, ["conky", "-d", "-c", "/h/.config/conky/My w/sub/conky.conf"]);
        assert_eq!(dir, "/h/.config/conky/My w/sub");
        // A file in the root gives the root, not an empty string.
        assert_eq!(conky_work_dir("/conky.conf"), "/");
    }

    /// Autostart at login: the same folder as the button, and the launch line
    /// survives spaces, quotes and `$` in the path (previously the path went
    /// in single quotes, and an apostrophe in a widget name broke the line).
    #[test]
    fn conky_autostart_entry_runs_in_widget_folder() {
        let conf = "/h/.config/conky/It's \"my\" $w/conky.conf";
        let entry = crate::ext_manager::conky_desktop_entry("w", conf);
        let value = |key: &str| {
            entry
                .lines()
                .find_map(|l| l.strip_prefix(key))
                .unwrap_or_else(|| panic!("нет {key} в\n{entry}"))
                .to_string()
        };
        let argv = crate::compositor_autostart::exec_argv(&value("Exec=")).expect("строка Exec разбирается");
        let script = format!("{}{CONKY_AUTOSTART_SCRIPT}", crate::conky_overlay::xwayland_shell(false, None));
        assert_eq!(argv, ["sh", "-c", script.as_str(), conf]);
        assert_eq!(value("Path="), "/h/.config/conky/It's \"my\" $w");
    }

    /// The autostart line in practice: `sh` from the entry changes into the
    /// widget folder and is replaced by conky with the same file (a fake conky
    /// records where it was started and with what), so "is the widget
    /// running" by `/proc` still recognises it.
    #[test]
    fn conky_autostart_script_really_changes_directory() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!("rexon-conky-cwd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let bin = tmp.join("bin");
        let widget = tmp.join("It's \"my\" $w");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&widget).unwrap();
        let log = tmp.join("log");
        let fake = bin.join("conky");
        std::fs::write(&fake, format!("#!/bin/sh\npwd > '{}'\nprintf '%s\\n' \"$@\" >> '{}'\n", log.display(), log.display())).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let conf = widget.join("conky.conf").to_string_lossy().into_owned();
        std::fs::write(&conf, "conky.text = [[x]]\n").unwrap();
        let entry = crate::ext_manager::conky_desktop_entry("w", &conf);
        let exec = entry.lines().find_map(|l| l.strip_prefix("Exec=")).unwrap();
        let argv = crate::compositor_autostart::exec_argv(exec).unwrap();
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let st = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir("/")
            .env("PATH", path)
            .status()
            .unwrap();
        assert!(st.success());
        let got = std::fs::read_to_string(&log).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(got, format!("{}\n-c\n{conf}\n", widget.display()));
    }

    /// An autostart enabled before the change into the widget folder is
    /// rewritten at the very first look at the toggle; a current one is not
    /// touched.
    #[test]
    fn old_conky_autostart_entry_is_upgraded() {
        use std::cell::RefCell;
        struct Fake {
            entry: RefCell<String>,
            writes: RefCell<usize>,
        }
        impl LookIo for Fake {
            fn run(&self, _a: &[String]) -> (bool, String) { (true, String::new()) }
            fn have(&self, _t: &str) -> bool { true }
            fn read(&self, p: &str) -> Option<String> {
                (p == "/h/.config/autostart/rexon-conky-w.desktop").then(|| self.entry.borrow().clone())
            }
            fn write(&self, _p: &str, t: &str) -> bool {
                *self.writes.borrow_mut() += 1;
                *self.entry.borrow_mut() = t.to_string();
                true
            }
            fn list_dir(&self, p: &str) -> Vec<String> {
                if p == "/h/.config/conky/w" { vec!["conky.conf".into()] } else { Vec::new() }
            }
        }
        let ctx = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        let old = "[Desktop Entry]\nType=Application\nName=Conky — w\nExec=conky -c '/h/.config/conky/w/conky.conf'\n";
        let io = Fake { entry: RefCell::new(old.into()), writes: RefCell::new(0) };
        assert!(conky_autostart(&io, &ctx, "w"));
        assert_eq!(*io.entry.borrow(), crate::ext_manager::conky_desktop_entry("w", "/h/.config/conky/w/conky.conf"));
        assert!(conky_autostart(&io, &ctx, "w"));
        assert_eq!(*io.writes.borrow(), 1, "нынешняя запись не переписывается");
    }

    /// Paths for installation into `~/.conky/<Name>/`: lines from real
    /// archives of the store's Conky section (victorConky, conky_orange, Conky
    /// Nord, conky_grey_redux): rewritten to the widget folder if their "tail"
    /// is there; existing and foreign paths are not touched.
    #[test]
    fn conky_foreign_paths_point_into_widget() {
        let home = "/h";
        let w = "/h/.config/conky/victorConky1.1";
        let files = [
            "/h/.config/conky/victorConky1.1/victorConky/ArchLarge.png",
            "/h/.config/conky/victorConky1.1/conky_orange.lua",
            "/h/.config/conky/victorConky1.1/nordcore/start_conky.sh",
            "/h/.config/conky/victorConky1.1/lib/rings.lua",
            "/h/.conky/Mine/clock.lua",
        ];
        let dirs = ["/h/.config/conky/victorConky1.1/nordcore", "/h/.config/conky/victorConky1.1/lib"];
        let exists = |p: &str| files.contains(&p) || dirs.contains(&p);
        let is_dir = |p: &str| dirs.contains(&p);
        let fix = |t: &str| conky_repair_text(t, home, w, &exists, &is_dir);

        // An archive wrapper: the whole tail exists in the widget folder.
        assert_eq!(
            fix("${image ~/.conky/victorConky/ArchLarge.png -p 10,0}").as_deref(),
            Some("${image ~/.config/conky/victorConky1.1/victorConky/ArchLarge.png -p 10,0}")
        );
        // A file directly in `~/.conky` (an archive with a folder, while the path has none).
        assert_eq!(
            fix("lua_load ~/.conky/conky_orange.lua").as_deref(),
            Some("lua_load ~/.config/conky/victorConky1.1/conky_orange.lua")
        );
        // A Lua string and `${HOME}`: a file the widget writes itself does not
        // exist yet, but its folder does.
        assert_eq!(
            fix("x = '${HOME}/.conky/nordcore/weather_raw'").as_deref(),
            Some("x = '${HOME}/.config/conky/victorConky1.1/nordcore/weather_raw'")
        );
        // The author's home: an absolute path into this person's home.
        assert_eq!(
            fix("Exec=/home/linuxscoop/.conky/nordcore/start_conky.sh").as_deref(),
            Some("Exec=/h/.config/conky/victorConky1.1/nordcore/start_conky.sh")
        );
        // A foreign name in `~/.config/conky/<Name>/` also misses and is repaired too.
        assert_eq!(
            fix("lua_load = '~/.config/conky/Regulus/lib/rings.lua'").as_deref(),
            Some("lua_load = '~/.config/conky/victorConky1.1/lib/rings.lua'")
        );
        // An existing path, an already-ours path, a plain word and a foreign file: no edit.
        assert_eq!(fix("lua_load ~/.conky/Mine/clock.lua"), None);
        assert_eq!(fix("${image ~/.config/conky/victorConky1.1/victorConky/ArchLarge.png}"), None);
        assert_eq!(fix("Copy the files to ~/.conky/ and run"), None);
        assert_eq!(fix("${image ~/.conky/Other/nothing.png}"), None);
        assert_eq!(fix("x=~/.conky.bak/a"), None);
        // A repeated repair changes nothing.
        let once = fix("${image ~/.conky/victorConky/ArchLarge.png}").unwrap();
        assert_eq!(fix(&once), None);
    }

    /// A widget folder with a space: the substitution would split the argument
    /// in two, so the path stays as it was; in the author's quotes it is
    /// repaired.
    #[test]
    fn conky_path_repair_keeps_arguments_whole() {
        let w = "/h/.config/conky/Rock and Roll";
        let exists = |p: &str| p == "/h/.config/conky/Rock and Roll/bg.png";
        let no = |_: &str| false;
        assert_eq!(conky_repair_text("${image ~/.conky/bg.png}", "/h", w, &exists, &no), None);
        assert_eq!(
            conky_repair_text("${image \"~/.conky/Rock and Roll/bg.png\"}", "/h", w, &exists, &no).as_deref(),
            Some("${image \"~/.config/conky/Rock and Roll/bg.png\"}")
        );
    }

    /// Repair on disk: only text files with such paths change, file
    /// permissions are kept (a script stays executable), `~/.conky` does not
    /// appear.
    #[test]
    fn conky_repair_paths_on_disk() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!("rexon-conky-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let home = tmp.to_string_lossy().into_owned();
        let w = tmp.join(".config/conky/nord");
        std::fs::create_dir_all(w.join("nordcore")).unwrap();
        std::fs::write(w.join("nordcore/clock01_rings.lua"), "-- lua\n").unwrap();
        std::fs::write(w.join("nordcore/conkyrc2core"), "lua_load = '~/.conky/nordcore/clock01_rings.lua',\n").unwrap();
        let sh = w.join("nordcore/start_conky.sh");
        std::fs::write(&sh, "#!/bin/sh\nconky -c ~/.conky/nordcore/conkyrc2core &\n").unwrap();
        std::fs::set_permissions(&sh, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(w.join("bg.png"), b"\x89PNG\0~/.conky/nordcore/x").unwrap();
        std::fs::write(w.join("install.sh"), "cp -r nordcore ~/.conky/nordcore\n").unwrap();
        // A script for the old conky: both the path and the X11 drawing module.
        let viz = w.join("viz.lua");
        std::fs::write(&viz, "require 'cairo'\ndofile('~/.conky/nordcore/clock01_rings.lua')\ncs = cairo_xlib_surface_create(d)\n").unwrap();
        let wd = w.to_string_lossy().into_owned();
        let mut changed = conky_repair_paths(&home, &wd);
        changed.sort();
        let rc = std::fs::read_to_string(w.join("nordcore/conkyrc2core")).unwrap();
        let start = std::fs::read_to_string(&sh).unwrap();
        let mode = std::fs::metadata(&sh).unwrap().permissions().mode() & 0o777;
        let conky_dir = tmp.join(".conky").exists();
        let again = conky_repair_paths(&home, &wd);
        let png = std::fs::read(w.join("bg.png")).unwrap();
        let viz = std::fs::read_to_string(&viz).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(changed.len(), 3, "{changed:?}");
        assert!(viz.starts_with(crate::conky_safety::LUA_COMPAT_MARK), "{viz}");
        assert!(viz.contains("pcall(require, 'cairo_xlib')\nrequire 'cairo'\ndofile('~/.config/conky/nord/nordcore/clock01_rings.lua')"), "{viz}");
        assert_eq!(rc, format!("lua_load = '~/.config/conky/nord/nordcore/clock01_rings.lua',\n"));
        assert_eq!(start, "#!/bin/sh\nconky -c ~/.config/conky/nord/nordcore/conkyrc2core &\n");
        assert_eq!(mode, 0o755);
        assert!(!conky_dir);
        assert!(again.is_empty());
        assert_eq!(png, b"\x89PNG\0~/.conky/nordcore/x");
        assert!(!changed.iter().any(|p| p.ends_with("install.sh")), "установщик автора не трогается");
    }

    /// A widget's "Remove": only its processes are killed (by number), the
    /// autostart entry is removed; a path not from the widgets folder does
    /// nothing.
    #[test]
    fn forgetting_a_widget_stops_it_and_drops_autostart() {
        use std::cell::RefCell;
        struct Fake {
            kills: RefCell<Vec<Vec<String>>>,
        }
        impl LookIo for Fake {
            fn run(&self, a: &[String]) -> (bool, String) {
                self.kills.borrow_mut().push(a.to_vec());
                (true, String::new())
            }
            fn have(&self, _t: &str) -> bool { true }
            fn read(&self, p: &str) -> Option<String> {
                match p {
                    "/proc/42/cmdline" => Some("conky\0-c\0/h/.config/conky/w/conky.conf\0".into()),
                    "/proc/43/cmdline" => Some("conky\0-c\0/h/.config/conky/other/conky.conf\0".into()),
                    _ => None,
                }
            }
            fn write(&self, _p: &str, _t: &str) -> bool { true }
            fn list_dir(&self, p: &str) -> Vec<String> {
                if p == "/proc" { vec!["42".into(), "43".into(), "self".into()] } else { Vec::new() }
            }
        }
        let tmp = std::env::temp_dir().join(format!("rexon-conky-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = tmp.to_string_lossy().into_owned();
        std::fs::create_dir_all(tmp.join("autostart")).unwrap();
        std::fs::write(tmp.join("autostart/rexon-conky-w.desktop"), "x").unwrap();
        std::fs::write(tmp.join("autostart/rexon-conky-other.desktop"), "x").unwrap();
        let ctx = Ctx { home: "/h".into(), xdg_config_home: Some(cfg), wayland: false, de: String::new() };
        let io = Fake { kills: RefCell::new(Vec::new()) };
        assert_eq!(conky_forget_widget(&io, &ctx, "/h/.config/conky/w/").as_deref(), Some("w"));
        assert_eq!(*io.kills.borrow(), vec![vec!["kill".to_string(), "--".to_string(), "42".to_string()]]);
        let gone = !tmp.join("autostart/rexon-conky-w.desktop").exists();
        let kept = tmp.join("autostart/rexon-conky-other.desktop").exists();
        assert_eq!(conky_forget_widget(&io, &ctx, "/h/.local/share/fonts/rexon-conky-w"), None);
        assert_eq!(conky_forget_widget(&io, &ctx, "/h/.config/conky/w/sub"), None);
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(gone && kept);
        assert_eq!(io.kills.borrow().len(), 1);
    }

    #[test]
    fn kwin_default_state() {
        assert!(kwin_plugin_enabled(Some("[Plugins]\nfooEnabled=true\n"), "foo", false));
        assert!(!kwin_plugin_enabled(Some("[Plugins]\nfooEnabled=false\n"), "foo", true));
        assert!(kwin_plugin_enabled(None, "foo", true));
        assert!(!kwin_plugin_enabled(Some("[Other]\nfooEnabled=true\n"), "foo", false));
    }

    #[test]
    fn helper_apply_line() {
        assert_eq!(
            system_apply_argv("/usr/lib/rexon-system-helper", &["--lang=ru".into()], "sddm", "Sugar", "arch"),
            vec!["pkexec", "/usr/lib/rexon-system-helper", "--lang=ru", "apply-sddm", "Sugar", "-", "arch"]
        );
    }

    #[test]
    fn helper_remove_line() {
        assert_eq!(
            system_remove_argv("/usr/lib/rexon-system-helper", &[], "plymouth", "neon", "debian"),
            vec!["pkexec", "/usr/lib/rexon-system-helper", "remove-plymouth", "neon", "-", "debian"]
        );
    }

    /// A frame theme Marco will not load: applying refuses with a reason and
    /// does not touch the key; one already written to the key is not "Active".
    #[test]
    fn marco_theme_marco_cannot_load_is_refused() {
        use std::cell::RefCell;
        struct Fake {
            key: RefCell<String>,
            sets: RefCell<usize>,
        }
        impl LookIo for Fake {
            fn run(&self, argv: &[String]) -> (bool, String) {
                if argv.get(1).map(String::as_str) == Some("set") {
                    *self.sets.borrow_mut() += 1;
                    *self.key.borrow_mut() = argv[4].clone();
                    return (true, String::new());
                }
                (true, format!("'{}'\n", self.key.borrow()))
            }
            fn have(&self, _t: &str) -> bool { true }
            fn read(&self, p: &str) -> Option<String> {
                // A theme Marco accepts, damaged by the theme name.
                let (extra, skip) = match p {
                    "/h/.themes/Shine/metacity-1/metacity-theme-3.xml" => (r#"<button function="unmaximize" state="normal"/>"#, ""),
                    "/h/.themes/Win8/metacity-1/metacity-theme-3.xml" => ("", "shade"),
                    "/h/.themes/Plain/metacity-1/metacity-theme-3.xml" => ("", ""),
                    _ => return None,
                };
                let buttons: String = ["close", "maximize", "minimize", "menu", "shade", "above", "stick", "unshade", "unabove", "unstick"]
                    .iter()
                    .filter(|f| **f != skip)
                    .flat_map(|f| ["normal", "pressed"].map(|st| format!(r#"<button function="{f}" state="{st}"/>"#)))
                    .collect();
                let windows: String = ["normal", "dialog", "modal_dialog", "utility", "menu", "border"]
                    .iter()
                    .map(|t| format!(r#"<window type="{t}" style_set="s"/>"#))
                    .collect();
                Some(format!(
                    "<metacity_theme><info><name>n</name><author>a</author><copyright>c</copyright>\
                     <date>d</date><description>t</description></info>\
                     <frame_style name=\"s\">{buttons}{extra}</frame_style>{windows}</metacity_theme>"
                ))
            }
            fn write(&self, _p: &str, _t: &str) -> bool { false }
            fn list_dir(&self, _p: &str) -> Vec<String> { Vec::new() }
        }
        let ctx = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        let io = Fake { key: RefCell::new("Plain".into()), sets: RefCell::new(0) };

        let (ok, msg) = press(&io, &ctx, MARCO_KIND, "Shine");
        assert!(!ok);
        assert!(msg.contains("Shine") && msg.contains("unmaximize"), "{msg}");
        assert_eq!(*io.sets.borrow(), 0, "ключ не записан");
        // Win8 themes from the store without a shade button: the refusal names the button.
        let (ok, msg) = press(&io, &ctx, MARCO_KIND, "Win8");
        assert!(!ok);
        assert!(msg.contains("Win8") && msg.contains("(shade)"), "{msg}");
        assert_eq!(*io.sets.borrow(), 0, "ключ не записан");
        assert_eq!(active_names(&io, &ctx, MARCO_KIND), vec!["Plain".to_string()]);

        // Written bypassing Rexon: Marco does not show it, and we do not say "Active".
        *io.key.borrow_mut() = "Shine".into();
        assert!(active_names(&io, &ctx, MARCO_KIND).is_empty());

        let (ok, _) = press(&io, &ctx, MARCO_KIND, "Plain");
        assert!(ok);
        assert_eq!(*io.sets.borrow(), 1);
    }

    /// A GNOME shell theme without the User Themes extension: the refusal
    /// names the extension, the key is not written; with the schema it applies
    /// as before.
    #[test]
    fn gnome_shell_theme_without_user_themes_says_so() {
        use std::cell::RefCell;
        struct Fake {
            schema: bool,
            key: RefCell<String>,
            sets: RefCell<usize>,
        }
        impl LookIo for Fake {
            fn run(&self, argv: &[String]) -> (bool, String) {
                let has = self.schema;
                match argv.get(1).map(String::as_str) {
                    Some("list-keys") => (has, if has { "name\n".into() } else { "No such schema\n".into() }),
                    Some("set") if has => {
                        *self.sets.borrow_mut() += 1;
                        *self.key.borrow_mut() = argv[4].clone();
                        (true, String::new())
                    }
                    Some("get") if has => (true, format!("'{}'\n", self.key.borrow())),
                    _ => (false, "No such schema\n".into()),
                }
            }
            fn have(&self, _t: &str) -> bool { true }
            fn read(&self, _p: &str) -> Option<String> { None }
            fn write(&self, _p: &str, _t: &str) -> bool { false }
            fn list_dir(&self, _p: &str) -> Vec<String> { Vec::new() }
        }
        let ctx = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        let io = Fake { schema: false, key: RefCell::new(String::new()), sets: RefCell::new(0) };
        let (ok, msg) = press(&io, &ctx, "theme-gnome-shell", "Dracula");
        assert!(!ok);
        assert!(msg.contains("Dracula") && msg.contains("User Themes"), "{msg}");
        assert_eq!(*io.sets.borrow(), 0);

        let io = Fake { schema: true, key: RefCell::new(String::new()), sets: RefCell::new(0) };
        let (ok, msg) = press(&io, &ctx, "theme-gnome-shell", "Dracula");
        assert!(ok, "{msg}");
        assert_eq!(*io.sets.borrow(), 1);
    }

    /// Enabling an extension the shell already knows is visible right away:
    /// no re-login caveat; if it does not know it or did not answer, the
    /// caveat stays (live check on gnome-shell 51.1, 16.10.2026: Rexon
    /// promised a re-login, while disabling worked immediately).
    #[test]
    fn relogin_note_only_for_extension_the_shell_hasnt_seen() {
        struct Fake {
            answer: Option<&'static str>,
        }
        impl LookIo for Fake {
            fn run(&self, argv: &[String]) -> (bool, String) {
                assert_eq!(argv[0], "gdbus");
                assert_eq!(argv.last().map(String::as_str), Some("x@y"));
                match self.answer {
                    Some(out) => (true, out.to_string()),
                    None => (false, String::new()),
                }
            }
            fn have(&self, _t: &str) -> bool { true }
            fn read(&self, _p: &str) -> Option<String> { None }
            fn write(&self, _p: &str, _t: &str) -> bool { false }
            fn list_dir(&self, _p: &str) -> Vec<String> { Vec::new() }
        }
        let row = dec::row("gnome-shell-extension").expect("строка GNOME");
        let wl = Ctx { home: "/h".into(), xdg_config_home: None, wayland: true, de: String::new() };
        let known = Fake { answer: Some("({'uuid': <'x@y'>, 'state': <2.0>},)\n") };
        assert_eq!(refresh_note(&known, row, &wl, "x@y"), None);
        let unseen = Fake { answer: Some("(@a{sv} {},)\n") };
        assert!(refresh_note(&unseen, row, &wl, "x@y").is_some());
        let silent = Fake { answer: None };
        assert!(refresh_note(&silent, row, &wl, "x@y").is_some());
        let x11 = Ctx { wayland: false, ..wl };
        assert_eq!(refresh_note(&unseen, row, &x11, "x@y"), None);
    }

    /// In a GNOME session "Active" and the button direction come from the
    /// shell: active means only a started one (`state` = 1), one that crashed
    /// while being enabled is enabled but not active; the button calls D-Bus
    /// through `gdbus`, the script with `gnome-extensions` only without the
    /// shell.
    #[test]
    fn gnome_extensions_follow_the_live_shell() {
        use crate::gnome_shell_ext::{ExtInfo, ExtState};
        use std::cell::RefCell;
        struct Fake {
            shell: Option<Vec<ExtInfo>>,
            runs: RefCell<Vec<String>>,
        }
        impl LookIo for Fake {
            fn run(&self, argv: &[String]) -> (bool, String) {
                self.runs.borrow_mut().push(argv.join(" "));
                (true, "['old@example.org']".into())
            }
            fn have(&self, _t: &str) -> bool { true }
            fn read(&self, _p: &str) -> Option<String> { None }
            fn write(&self, _p: &str, _t: &str) -> bool { false }
            fn list_dir(&self, _p: &str) -> Vec<String> { Vec::new() }
            fn gnome_shell_extensions(&self) -> Option<Vec<ExtInfo>> { self.shell.clone() }
        }
        let ext = |uuid: &str, state: ExtState, enabled: bool| ExtInfo {
            uuid: uuid.into(),
            name: uuid.into(),
            description: String::new(),
            state,
            enabled,
            error: String::new(),
            has_prefs: false,
            has_update: false,
            can_change: true,
            is_system: false,
            path: String::new(),
            url: String::new(),
            version: String::new(),
            version_name: String::new(),
            shell_versions: Vec::new(),
            session_modes: Vec::new(),
            donations: Default::default(),
        };
        let ctx = Ctx { home: "/h".into(), xdg_config_home: None, wayland: true, de: "gnome".into() };
        let kind = crate::gnome_ext_section::KIND;
        let live = Fake {
            shell: Some(vec![
                ext("run@example.org", ExtState::Active, true),
                ext("broken@example.org", ExtState::Error, true),
                ext("off@example.org", ExtState::Inactive, false),
            ]),
            runs: RefCell::new(Vec::new()),
        };
        assert_eq!(active_names(&live, &ctx, kind), vec!["run@example.org".to_string()]);
        assert!(addon_enabled(&live, &ctx, kind, "broken@example.org"));
        assert!(live.runs.borrow().is_empty(), "при оболочке gsettings не спрашиваем");
        let on = enable_steps(&live, &ctx, kind, "off@example.org");
        let off = disable_steps(&live, &ctx, kind, "run@example.org");
        let text = |steps: &[crate::ext_manager::ExtStep]| format!("{steps:?}");
        assert!(text(&on).contains("org.gnome.Shell.Extensions.EnableExtension"), "{on:?}");
        assert!(text(&off).contains("org.gnome.Shell.Extensions.DisableExtension"), "{off:?}");
        // No shell: the old path, the key and the script.
        let down = Fake { shell: None, runs: RefCell::new(Vec::new()) };
        assert_eq!(active_names(&down, &ctx, kind), vec!["old@example.org".to_string()]);
        assert!(!text(&enable_steps(&down, &ctx, kind, "off@example.org")).contains("gdbus"));
    }
}
