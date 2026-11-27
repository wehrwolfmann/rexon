//! "Animation" of the "Settings" tab: animation speed, "no animations" and
//! per-event effects (opening and closing windows, minimizing, switching
//! desktops…) — on ALL desktops, with each one's native key.
//!
//! The reference is the "Animation" module of KDE System Settings (kcm_animations, KWin
//! 6.7): the speed slider writes `kdeglobals [KDE] AnimationDurationFactor`
//! (the value series 4 · 2 · 1.5 · 1 · 0.75 · 0.5, the extreme "instant" = 0),
//! effects are the keys `kwinrc [Plugins] <effect>Enabled`. Within each group
//! ("opening and closing windows" and so on) at most one effect is enabled —
//! the group comes from the effect's metadata (`X-KWin-Exclusive-Category` for
//! scripted effects, `org.kde.kwin.effect/exclusiveGroup` for built-in ones).
//! Effect names and their defaults are checked against the KWin
//! 6.7.5 sources (the same release as installed on the system) and a live `kwinrc`. A key
//! equal to the default is deleted rather than written — the KDE module itself
//! does the same, and `kwinrc` does not accumulate extra lines.
//!
//! Other desktops:
//! * GNOME, Budgie, Pantheon — `org.gnome.desktop.interface enable-animations`
//!   (there is no speed there — we say so honestly);
//! * Cinnamon — the main switch `org.cinnamon desktop-effects-workspace`
//!   plus `org.cinnamon.desktop.interface enable-animations`, speed
//!   `window-effect-speed` (three steps), effects `desktop-effects-*`;
//! * MATE — `org.mate.interface enable-animations` and `gtk-enable-animations`
//!   (Marco has no window animations);
//! * XFCE — `xsettings /Gtk/EnableAnimations` (xfwm4 has no window animations);
//! * Hyprland — our own file `rexon-animations.lua|conf` next to the config and
//!   the line including it in a managed block: `animations.enabled`,
//!   speed — a multiplier on Hyprland's standard animation set, kinds of
//!   window and workspace animations;
//! * niri — our own `rexon-animations.kdl`: `off`, `slowdown`, turning off
//!   individual animations;
//! * sway, LXQt, labwc, COSMIC… — the compositor has no animations of its own (or no
//!   settings for them); the switch affects animations inside
//!   GTK programs (`gtk-enable-animations` in `settings.ini`). We say exactly that.
//!
//! KDE additionally: the "Desktop Effects" list — all other KWin effects
//! (blur, overview, zoom, cube and third-party ones) with search, groups and
//! a "Configure…" button ([`desktop_effects_state`]); the same button is on
//! "Animation" rows whose effect has a settings page ([`configure_effect`]).
//! Outside KDE this section does not exist: effects are a KWin property.
//!
//! GTK programs outside the GNOME family take the switch from
//! `~/.config/gtk-{3,4}.0/settings.ini` — a shared layer we write everywhere
//! the desktop has no xsettings service of its own.
//!
//! Design: the core does nothing to the machine by itself — everything goes through [`LookIo`].
//! The snapshot ([`read_state`]) and the write plan ([`plan_change`]) are verified by
//! checks on a fake `LookIo`; the executor [`run_steps`] is shared by
//! the button, the `de_matrix_stand` test bench and the "Window title
//! buttons" section (`window_buttons`).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::i18n::{n_, tr, trf};
use crate::themes::{remove_managed_block, upsert_managed_block, WmSyntax};

// ═══════════════════════════════════════════════════════════════════════════
// I/O and executor — shared with `window_buttons`
// ═══════════════════════════════════════════════════════════════════════════

/// Everything the module needs from the machine. The real implementation is [`SystemIo`];
/// checks substitute their own.
pub trait LookIo {
    /// Run a command: (exit code 0?, output). On success the output is stdout; on
    /// failure — the reason for the person ([`run_failure_text`]: stderr, or
    /// stdout without it), so the message names the reason and not just the program name.
    fn run(&self, argv: &[String]) -> (bool, String);
    /// Whether a program is in PATH.
    fn have(&self, tool: &str) -> bool;
    /// Read a text file.
    fn read(&self, path: &str) -> Option<String>;
    /// Read a file as bytes (Qt modules: effect metadata is embedded in
    /// the `.so`). By default — via [`LookIo::read`], which is enough for fakes.
    fn read_bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.read(path).map(String::into_bytes)
    }
    /// Write a whole file (directories are created).
    fn write(&self, path: &str, text: &str) -> bool;
    /// Names in a directory (empty — no directory).
    fn list_dir(&self, path: &str) -> Vec<String>;
    /// Start a program without waiting (restarting the shell: `cinnamon
    /// --replace`). By default it starts NOTHING: test fakes are safe by
    /// themselves. On 25.10.2026 a test with a fake lacking this method
    /// ran `cinnamon --replace` on a live MATE screen and brought the machine down.
    fn spawn(&self, _argv: &[String]) -> bool {
        false
    }
    /// Delete a whole directory (our outdated copy of a shell extension,
    /// [`crate::screen_effects`]). By default it deletes NOTHING — test fakes
    /// are safe by themselves, as with [`LookIo::spawn`].
    fn remove_tree(&self, _path: &str) -> bool {
        false
    }
    /// The desktop of the RUNNING session (`system::detect_current_de`), so a
    /// section knows whether a program it starts lands on the screen in front
    /// of the person or the desktop chosen in "Current desktop" is only being
    /// prepared for the next login. By default — `None` (unknown): test fakes
    /// start nothing live, as with [`LookIo::spawn`].
    fn session_desktop(&self) -> Option<String> {
        None
    }
    /// GNOME extensions as reported by the running shell (D-Bus
    /// `ListExtensions`). `None` — no shell. Defaults to `None`:
    /// test fakes do not touch the bus.
    fn gnome_shell_extensions(&self) -> Option<Vec<crate::gnome_shell_ext::ExtInfo>> {
        None
    }
}

/// The real machine.
pub struct SystemIo;

impl LookIo for SystemIo {
    fn gnome_shell_extensions(&self) -> Option<Vec<crate::gnome_shell_ext::ExtInfo>> {
        crate::gnome_ext_section::live_snapshot()
    }
    fn session_desktop(&self) -> Option<String> {
        Some(LookCtx::from_env().de).filter(|de| !de.is_empty())
    }
    fn spawn(&self, argv: &[String]) -> bool {
        let Some(program) = argv.first() else {
            return false;
        };
        std::process::Command::new(program)
            .args(&argv[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
    }

    fn run(&self, argv: &[String]) -> (bool, String) {
        if argv.is_empty() {
            return (false, String::new());
        }
        let report = crate::utils::run(
            &crate::utils::RunSpec {
                argv: argv.to_vec(),
                output: crate::utils::RunOutput::Capture,
                timeout_secs: Some(30),
                lang_c: true,
                ..Default::default()
            },
            None,
        );
        if report.returncode == Some(0) {
            return (true, crate::utils::decode_text(&report.stdout));
        }
        (false, run_failure_text(&argv[0], &report))
    }

    fn have(&self, tool: &str) -> bool {
        crate::utils::which(tool, std::env::var("PATH").ok().as_deref()).is_some()
    }

    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn remove_tree(&self, path: &str) -> bool {
        // Only an absolute path no shorter than `/home/<who>/<what>/<name>`: the root
        // and the home directory can never get here, whatever the plan error.
        let p = std::path::Path::new(path);
        if !p.is_absolute() || p.components().count() < 5 {
            return false;
        }
        match std::fs::remove_dir_all(p) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        }
    }

    fn read_bytes(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn write(&self, path: &str, text: &str) -> bool {
        if let Some(dir) = std::path::Path::new(path).parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return false;
            }
        }
        crate::utils::atomic_write(path, text).is_ok()
    }

    fn list_dir(&self, path: &str) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(path)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        out.sort();
        out
    }
}

/// The reason a command failed, for the person. Previously only stdout went out, and
/// for failed `gsettings`/`bash -c` it is empty: the person read "Could not
/// apply "Dracula": gsettings" without a single word about what happened
/// (GNOME check, 16.10.2026). The last non-empty stderr line is taken —
/// programs write the error itself there (`bash` puts the line number before it,
/// tracebacks — the stack); without stderr — stdout; if it never reached the code — a timeout or
/// a launch error.
pub fn run_failure_text(program: &str, report: &crate::utils::RunReport) -> String {
    use crate::utils::RunOutcome;
    let last_line = |bytes: &[u8]| {
        crate::utils::decode_text(bytes)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .last()
            .map(str::to_string)
    };
    if let Some(line) = last_line(&report.stderr).or_else(|| last_line(&report.stdout)) {
        return line;
    }
    match report.outcome {
        RunOutcome::Timeout => crate::utils::msg_timeout(program, &report.timeout_secs.to_string()),
        RunOutcome::Error if !report.error.is_empty() => report.error.clone(),
        _ => String::new(),
    }
}

/// Where to look for the person's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookCtx {
    /// The desktop as the core calls it (`system::detect_current_de`).
    pub de: String,
    pub home: String,
    /// `$XDG_CONFIG_HOME` or `~/.config`.
    pub config_home: String,
    /// `$XDG_DATA_HOME` or `~/.local/share`.
    pub data_home: String,
    /// System data directories (`$XDG_DATA_DIRS`).
    pub data_dirs: Vec<String>,
    /// Qt 6 plugin roots (`$QT_PLUGIN_PATH` and the usual places).
    pub plugin_dirs: Vec<String>,
    /// Wayland session: in LXQt it decides whose window manager to read
    /// (`window_manager` on X11 or `compositor` on Wayland,
    /// [`crate::qt_settings::title_wm_for`]).
    pub wayland: bool,
}

impl LookCtx {
    /// Data and plugin directories — the specification defaults; the process
    /// environment is substituted by [`LookCtx::from_env`].
    pub fn new(de: &str, home: &str, xdg_config_home: Option<&str>) -> Self {
        LookCtx {
            de: de.to_string(),
            home: home.trim_end_matches('/').to_string(),
            config_home: crate::utils::config_home(xdg_config_home, home),
            data_home: crate::utils::data_home(None, home),
            data_dirs: crate::plugin_journal::data_dirs(""),
            plugin_dirs: crate::plugin_journal::qt_plugin_dirs(""),
            wayland: false,
        }
    }

    /// From the process environment (the window, the test bench).
    pub fn from_env() -> Self {
        let env = |n: &str| std::env::var(n).unwrap_or_default();
        let de = crate::system::detect_current_de(
            &env("XDG_CURRENT_DESKTOP"),
            &env("DESKTOP_SESSION"),
            std::env::var("WAYLAND_DISPLAY").is_ok(),
        )
        .unwrap_or_default();
        let xdg = env("XDG_CONFIG_HOME");
        let mut ctx = LookCtx::new(&de, &env("HOME"), if xdg.is_empty() { None } else { Some(&xdg) });
        let data = env("XDG_DATA_HOME");
        ctx.data_home = crate::utils::data_home(if data.is_empty() { None } else { Some(&data) }, &ctx.home);
        ctx.data_dirs = crate::plugin_journal::data_dirs(&env("XDG_DATA_DIRS"));
        ctx.plugin_dirs = crate::plugin_journal::qt_plugin_dirs(&env("QT_PLUGIN_PATH"));
        ctx.wayland = crate::wallpaper_mmap::is_wayland_session(&env("XDG_SESSION_TYPE"), &env("WAYLAND_DISPLAY"));
        ctx
    }

    pub fn cfg(&self, rel: &str) -> String {
        format!("{}/{rel}", self.config_home.trim_end_matches('/'))
    }

    /// Context for the desktop `chosen` selected in "Current desktop" of the
    /// "Appearance" tab: empty, "detect automatically" or the same
    /// desktop — the context as is; another one — its name and ITS session kind
    /// ([`crate::screen_effects::KNOWN_DESKTOPS`]; an unknown one — the kind stays).
    pub fn for_chosen(self, chosen: &str) -> Self {
        let chosen = chosen.trim();
        if chosen.is_empty() || chosen == crate::appearance_page::DE_AUTO_ID || chosen == self.de {
            return self;
        }
        let mut ctx = self;
        if let Some((_, wayland)) = crate::screen_effects::KNOWN_DESKTOPS.iter().find(|(de, _)| *de == chosen) {
            ctx.wayland = *wayland;
        }
        ctx.de = chosen.to_string();
        ctx
    }

    /// Context of the "Appearance" tab sections: the process environment, but the desktop is
    /// the one selected in "Current desktop" ([`set_chosen_desktop`]). Previously the sections
    /// ("Animation", "Window title buttons", "Screen effects", "Accent"…)
    /// took the session's desktop, and with another desktop selected by hand they talked about and
    /// wrote to the running one (08.11.2026: "Tiling and Wayfire" in a KDE session —
    /// a warning about KWin). Outside the tab no choice is set — there it is
    /// exactly [`LookCtx::from_env`].
    pub fn for_look() -> Self {
        Self::from_env().for_chosen(&chosen_desktop())
    }
}

/// The desktop selected in "Current desktop" of the "Appearance" tab (empty — not
/// selected, the session is used). One per process: the page sets it, the sections
/// read it through [`LookCtx::for_look`].
static CHOSEN_DESKTOP: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Remember the "Current desktop" choice (a core identifier or
/// [`crate::appearance_page::DE_AUTO_ID`]).
pub fn set_chosen_desktop(de: &str) {
    if let Ok(mut slot) = CHOSEN_DESKTOP.write() {
        *slot = de.trim().to_string();
    }
}

/// The "Current desktop" choice (empty — not selected).
pub fn chosen_desktop() -> String {
    CHOSEN_DESKTOP.read().map(|s| s.clone()).unwrap_or_default()
}

/// One possible compositor config and our file next to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WmTarget {
    /// The compositor config — the person maintains it.
    pub config: String,
    pub syntax: WmSyntax,
    /// Our file — rewritten entirely.
    pub own_path: String,
    pub own_content: String,
    /// The line including our file (inside the managed block).
    pub include_line: String,
}

/// Our file in the compositor config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WmInclude {
    /// Name of the managed block (`>>> Rexon: <tag>`).
    pub tag: String,
    /// Compositor name for the person.
    pub wm: String,
    /// Possible configs in the compositor's own lookup order.
    pub targets: Vec<WmTarget>,
    /// Config check by the compositor itself (the path is appended at the end).
    pub validate: Vec<String>,
    /// Minimum version (`niri --version`), (year, month).
    pub min_version: Option<(Vec<String>, (u32, u32))>,
    /// No personal config: what to seed it with (the system config) and in place of
    /// which of `targets`. Only for sway — unlike niri and
    /// Hyprland, it does not create its config on first start.
    pub seed: Option<(String, usize)>,
    /// Configs the compositor reads instead of its own (sway — the i3
    /// config): if one exists, seeding is forbidden, our file would shadow it.
    pub foreign: Vec<String>,
}

/// A write step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum LookStep {
    /// A command that must succeed (the native key).
    Cmd(Vec<String>),
    /// Notify the live desktop: the first variant that works; a failure
    /// is not an error (on the test bench and outside a session there is nobody to notify).
    Notify(Vec<Vec<String>>),
    /// A key in `[Settings]` of `gtk-3.0/settings.ini` and `gtk-4.0/settings.ini`.
    GtkIni { key: String, value: String },
    /// A whole file (COSMIC stores one key per file).
    WriteFile { path: String, content: String },
    /// Our file + a managed block in the compositor config.
    Wm(WmInclude),
    /// Remove a whole directory (an outdated copy of our own extension).
    RemoveTree { path: String },
}

/// Short program name from argv — for the message to the person.
fn tool_name(argv: &[String]) -> String {
    argv.first().map(|a| a.rsplit('/').next().unwrap_or(a).to_string()).unwrap_or_default()
}

/// Execute the plan. An error is a ready sentence for the person.
pub fn run_steps(io: &dyn LookIo, ctx: &LookCtx, steps: &[LookStep]) -> Result<(), String> {
    for step in steps {
        match step {
            LookStep::Cmd(argv) => {
                if !io.have(&argv[0]) {
                    return Err(trf("The program %1 is not installed.", &[&tool_name(argv)]));
                }
                // One retry: on the test bench under load kwriteconfig6
                // once (1 of 4 runs) returned an error on a fresh profile,
                // while a repeat of the same command succeeds.
                if !io.run(argv).0 && !io.run(argv).0 {
                    return Err(trf("Could not change the setting: %1 failed.", &[&tool_name(argv)]));
                }
            }
            LookStep::Notify(variants) => {
                for argv in variants {
                    if io.have(&argv[0]) && io.run(argv).0 {
                        break;
                    }
                }
            }
            LookStep::GtkIni { key, value } => {
                for version in ["3.0", "4.0"] {
                    let path = crate::gtk_sanitizer::gtk_ini_path(&ctx.config_home, version);
                    let before = io.read(&path);
                    let after = crate::gtk_sanitizer::write_ini_setting_in(before.as_deref(), "Settings", key, value);
                    if before.as_deref() != Some(after.as_str()) && !io.write(&path, &after) {
                        return Err(trf("Could not write %1.", &[&path]));
                    }
                }
            }
            LookStep::WriteFile { path, content } => {
                if io.read(path).as_deref() != Some(content.as_str()) && !io.write(path, content) {
                    return Err(trf("Could not write %1.", &[path]));
                }
            }
            LookStep::Wm(inc) => run_wm_include(io, inc)?,
            LookStep::RemoveTree { path } => {
                if !io.remove_tree(path) {
                    return Err(trf("Couldn't remove %1.", &[path]));
                }
            }
        }
    }
    Ok(())
}

/// Our file + the include line, with a copy of the config before the first edit and
/// a rollback if the compositor's check rejected the result.
fn run_wm_include(io: &dyn LookIo, inc: &WmInclude) -> Result<(), String> {
    if let Some((argv, min)) = &inc.min_version {
        if io.have(&argv[0]) {
            let (_, out) = io.run(argv);
            if let Some(v) = crate::themes::parse_wm_version(&out) {
                if v < *min {
                    return Err(trf("%1 is too old for this (version %2.%3 or newer is needed).", &[
                        &inc.wm,
                        &min.0,
                        &format!("{:02}", min.1),
                    ]));
                }
            }
        }
    }
    let not_found = || trf("The %1 config was not found. Start %1 once so that it creates its config.", &[&inc.wm]);
    let target = match inc.targets.iter().find(|t| io.read(&t.config).is_some()) {
        Some(t) => t,
        None => {
            // sway without a personal config reads the system one: a copy in place of
            // the personal one does not change behavior (as sway(5) advises).
            let seeded = match &inc.seed {
                Some((from, idx)) if !inc.foreign.iter().any(|f| io.read(f).is_some()) => {
                    match (io.read(from), inc.targets.get(*idx)) {
                        (Some(text), Some(t)) => {
                            if !io.write(&t.config, &text) {
                                return Err(trf("Could not write %1.", &[&t.config]));
                            }
                            Some(t)
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            seeded.ok_or_else(not_found)?
        }
    };
    if io.read(&target.own_path).as_deref() != Some(target.own_content.as_str())
        && !io.write(&target.own_path, &target.own_content)
    {
        return Err(trf("Could not write %1.", &[&target.own_path]));
    }
    let before = io.read(&target.config).unwrap_or_default();
    let after = upsert_managed_block(&before, target.syntax.comment(), &inc.tag, &target.include_line);
    let validate = |path: &str| -> bool {
        let mut argv = inc.validate.clone();
        argv.push(path.to_string());
        io.run(&argv).0
    };
    let validator = inc.validate.first().is_some_and(|t| io.have(t));
    if after == before {
        // The include is already there — only our file changed. If the compositor
        // did not accept it, there is nothing to restore our previous file from (we do not keep it),
        // so we check first, then speak.
        if validator && !validate(&target.config) {
            return Err(trf("%1 rejected the change. Check the config: %2", &[&inc.wm, &target.own_path]));
        }
        return Ok(());
    }
    let backup = format!("{}.rexon-bak", target.config);
    if io.read(&backup).is_none() && !io.write(&backup, &before) {
        return Err(trf("Could not write %1.", &[&backup]));
    }
    let was_valid = validator && validate(&target.config);
    if !io.write(&target.config, &after) {
        return Err(trf("Could not write %1.", &[&target.config]));
    }
    if was_valid && !validate(&target.config) {
        let _ = io.write(&target.config, &before);
        return Err(trf("%1 rejected the change; the config is restored as it was.", &[&inc.wm]));
    }
    Ok(())
}

/// Remove our managed block (rollback to "as it was before Rexon"). Text only —
/// the caller writes it; needed by the checks.
pub fn without_block(existing: &str, syntax: WmSyntax, tag: &str) -> String {
    remove_managed_block(existing, syntax.comment(), tag)
}

/// Compositor config candidates: Hyprland 0.56 reads `hyprland.lua` if
/// it exists, otherwise `hyprland.conf`; niri — `config.kdl`; sway —
/// `~/.sway/config`, then `$XDG_CONFIG_HOME/sway/config`.
pub fn wm_targets(
    ctx: &LookCtx,
    wm: &str,
    stem: &str,
    content: &dyn Fn(WmSyntax) -> String,
) -> Vec<WmTarget> {
    let t = |syntax: WmSyntax, config: String, own: String| {
        let include_line = match syntax {
            WmSyntax::HyprLua => format!("pcall(require, \"{stem}\")"),
            WmSyntax::HyprConf => format!("source = {own}"),
            WmSyntax::Kdl => format!("include \"{stem}.kdl\""),
            WmSyntax::Sway => format!("include {}", crate::themes::sway_path(&own)),
        };
        WmTarget { config, syntax, own_content: content(syntax), own_path: own, include_line }
    };
    match wm {
        "hyprland" => vec![
            t(WmSyntax::HyprLua, ctx.cfg("hypr/hyprland.lua"), ctx.cfg(&format!("hypr/{stem}.lua"))),
            t(WmSyntax::HyprConf, ctx.cfg("hypr/hyprland.conf"), ctx.cfg(&format!("hypr/{stem}.conf"))),
        ],
        "niri" => vec![t(WmSyntax::Kdl, ctx.cfg("niri/config.kdl"), ctx.cfg(&format!("niri/{stem}.kdl")))],
        "sway" => vec![
            t(WmSyntax::Sway, format!("{}/.sway/config", ctx.home), format!("{}/.sway/{stem}.conf", ctx.home)),
            t(WmSyntax::Sway, ctx.cfg("sway/config"), ctx.cfg(&format!("sway/{stem}.conf"))),
        ],
        _ => Vec::new(),
    }
}

/// Config check by the compositor itself and the version threshold.
pub fn wm_include(ctx: &LookCtx, wm: &str, tag: &str, stem: &str, content: &dyn Fn(WmSyntax) -> String) -> WmInclude {
    let (name, validate, min_version) = match wm {
        "hyprland" => ("Hyprland", vec!["Hyprland", "--verify-config", "--config"], None),
        "niri" => (
            "niri",
            vec!["niri", "validate", "--config"],
            // niri got `include` in 25.11.
            Some((vec!["niri".to_string(), "--version".to_string()], (25, 11))),
        ),
        _ => ("sway", vec!["sway", "--validate", "--config"], None),
    };
    let (seed, foreign) = if name == "sway" {
        // The second of `wm_targets` is `$XDG_CONFIG_HOME/sway/config`.
        (
            Some(("/etc/sway/config".to_string(), 1)),
            vec![format!("{}/.i3/config", ctx.home), ctx.cfg("i3/config")],
        )
    } else {
        (None, Vec::new())
    };
    WmInclude {
        tag: tag.to_string(),
        wm: name.to_string(),
        targets: wm_targets(ctx, wm, stem, content),
        validate: validate.into_iter().map(String::from).collect(),
        min_version,
        seed,
        foreign,
    }
}

/// Our compositor file that is currently included (the first existing
/// config among the candidates) — its text, if it exists.
pub fn read_own_wm_file(io: &dyn LookIo, ctx: &LookCtx, wm: &str, stem: &str) -> Option<(WmSyntax, String)> {
    let targets = wm_targets(ctx, wm, stem, &|_| String::new());
    let target = targets.iter().find(|t| io.read(&t.config).is_some())?;
    Some((target.syntax, io.read(&target.own_path)?))
}

/// The state line in the first line of our file: `rexon-state: k=v k=v`.
pub fn parse_state_line(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(line) = text.lines().find(|l| l.contains("rexon-state:")) {
        let rest = line.split("rexon-state:").nth(1).unwrap_or("");
        for pair in rest.split_whitespace() {
            if let Some((k, v)) = pair.split_once('=') {
                out.insert(k.to_string(), v.to_string());
            }
        }
    }
    out
}

pub fn state_line(comment: &str, values: &BTreeMap<String, String>) -> String {
    let pairs: Vec<String> = values.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{comment} rexon-state: {}\n", pairs.join(" "))
}

/// A gsettings value without quotes and the type prefix (`uint32 1` → `1`).
pub fn clean_gvariant(raw: &str) -> String {
    let s = raw.trim();
    let s = s.rsplit(' ').next().filter(|_| s.starts_with("uint") || s.starts_with("int")).unwrap_or(s);
    s.trim_matches('\'').to_string()
}

pub fn gsettings_get(io: &dyn LookIo, schema: &str, key: &str) -> Option<String> {
    if !io.have("gsettings") {
        return None;
    }
    let (ok, out) = io.run(&argv(&["gsettings", "get", schema, key]));
    ok.then(|| clean_gvariant(&out))
}

pub fn gsettings_set(schema: &str, key: &str, gvariant: &str) -> LookStep {
    LookStep::Cmd(argv(&["gsettings", "set", schema, key, gvariant]))
}

pub fn xfconf_get(io: &dyn LookIo, channel: &str, prop: &str) -> Option<String> {
    if !io.have("xfconf-query") {
        return None;
    }
    let (ok, out) = io.run(&argv(&["xfconf-query", "-c", channel, "-p", prop]));
    ok.then(|| out.trim().to_string())
}

/// `-n -t`: create the property if it does not exist yet (a clean profile does not have it).
pub fn xfconf_set(channel: &str, prop: &str, kind: &str, value: &str) -> LookStep {
    LookStep::Cmd(argv(&["xfconf-query", "-c", channel, "-p", prop, "-n", "-t", kind, "-s", value]))
}

/// `kwriteconfig6`: a value or deleting the key (back to the default).
pub fn kwrite(file: &str, group: &str, key: &str, value: Option<&str>, bool_type: bool) -> LookStep {
    let mut a = argv(&["kwriteconfig6", "--file", file, "--group", group, "--key", key, "--notify"]);
    match value {
        None => {
            a.push("--delete".into());
            a.push(String::new());
        }
        Some(v) => {
            if bool_type {
                a.push("--type".into());
                a.push("bool".into());
            }
            a.push(v.to_string());
        }
    }
    LookStep::Cmd(a)
}

/// A KConfig value from the file `$XDG_CONFIG_HOME/<file>`; empty — no key.
pub fn kconfig_value(io: &dyn LookIo, ctx: &LookCtx, file: &str, group: &str, key: &str) -> Option<String> {
    let text = io.read(&ctx.cfg(file));
    let cfg = crate::gtk_sanitizer::read_kde_ini(text.as_deref());
    let v = crate::gtk_sanitizer::kde_value(&cfg, group, key);
    (!v.is_empty()).then_some(v)
}

/// Ask the live KWin to reread its config (on the test bench and outside KDE — silently skipped).
pub fn kwin_reconfigure() -> LookStep {
    let (dest, obj, method) = crate::utils::KWIN_RECONFIGURE;
    LookStep::Notify(vec![
        crate::utils::dbus_call_argv("dbus-send", "dbus-send", dest, obj, method, &[]),
        crate::utils::dbus_call_argv("qdbus", "qdbus6", dest, obj, method, &[]),
    ])
}

/// The `KGlobalSettings.notifyChange` signal — as in the KDE "Animation" module.
fn kglobals_notify() -> LookStep {
    LookStep::Notify(vec![argv(&[
        "dbus-send",
        "--session",
        "--type=signal",
        "/KGlobalSettings",
        "org.kde.KGlobalSettings.notifyChange",
        "int32:3",
        "int32:0",
    ])])
}

pub fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Is the GTK `settings.ini` key written for this desktop? Only where
/// GTK does not get the setting from an xsettings service / the GNOME-family portal.
pub fn uses_gtk_ini(de: &str) -> bool {
    !matches!(de, "gnome" | "budgie" | "pantheon" | "cinnamon-mint" | "mate" | "xfce")
}

/// Desktop name for the person.
pub fn de_label(de: &str) -> String {
    match de {
        "kde" => "KDE Plasma".into(),
        "gnome" => "GNOME".into(),
        "cinnamon-mint" => "Cinnamon".into(),
        "mate" => "MATE".into(),
        "xfce" => "Xfce".into(),
        "budgie" => "Budgie".into(),
        "pantheon" => "Pantheon".into(),
        "deepin" => "Deepin".into(),
        "lxqt" => "LXQt".into(),
        "cosmic" => "COSMIC".into(),
        "hyprland" => "Hyprland".into(),
        "niri" => "niri".into(),
        "sway" => "sway".into(),
        "openbox" => "Openbox".into(),
        "" => tr("this desktop"),
        other => other.to_string(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Animation: the model
// ═══════════════════════════════════════════════════════════════════════════

/// Speed slider steps — the same as KDE's (slow → fast).
pub const SPEED_STEPS: &[f64] = &[4.0, 2.0, 1.5, 1.0, 0.75, 0.5];

/// A choice option in a row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnimOption {
    pub id: String,
    pub label: String,
    /// The effect has its own settings page ("Configure…", KWin only).
    pub configurable: bool,
}

/// An "event → effect" row (a choice) or an "effect on/off" row (a checkbox).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EffectRow {
    pub id: String,
    pub label: String,
    /// `choice` | `toggle`.
    pub kind: String,
    pub options: Vec<AnimOption>,
    /// The selected option; for a checkbox `true`/`false`.
    pub value: String,
    /// A checkbox effect has its own settings page (for a choice — see
    /// the option's `configurable`).
    pub configurable: bool,
}

/// Section snapshot for the page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnimState {
    pub de: String,
    /// Animations are enabled.
    pub enabled: bool,
    /// There is a speed slider.
    pub has_speed: bool,
    /// Slider steps and the selected one (index into [`SPEED_STEPS`]).
    pub speed_steps: Vec<f64>,
    pub speed_index: usize,
    pub rows: Vec<EffectRow>,
    /// An honest explanation: where the setting does not reach.
    pub note: String,
}

/// What to change.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimChange {
    Enabled(bool),
    /// Duration multiplier (a step of [`SPEED_STEPS`]).
    Speed(f64),
    Row { id: String, value: String },
    /// Everything to defaults.
    Defaults,
}

/// Internal snapshot: the switch, the multiplier, row values.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimSettings {
    pub enabled: bool,
    pub factor: f64,
    pub values: BTreeMap<String, String>,
}

impl AnimSettings {
    fn get(&self, id: &str) -> &str {
        self.values.get(id).map(String::as_str).unwrap_or("")
    }
    fn on(&self, id: &str) -> bool {
        self.get(id) == "true"
    }
}

/// The slider step nearest to the multiplier.
pub fn speed_index(factor: f64) -> usize {
    let mut best = 3;
    let mut dist = f64::MAX;
    for (i, s) in SPEED_STEPS.iter().enumerate() {
        let d = (s - factor).abs();
        if d < dist {
            dist = d;
            best = i;
        }
    }
    best
}

fn fmt_factor(f: f64) -> String {
    let s = format!("{f:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

// ── Labels (translation catalog) ───────────────────────────────────────────

const L_OPEN_CLOSE: &str = n_("Window open and close");
const L_OPEN: &str = n_("Opening windows");
const L_CLOSE: &str = n_("Closing windows");
const L_MAXIMIZE: &str = n_("Window maximize");
const L_RESIZE: &str = n_("Resizing and tiling windows");
const L_MINIMIZE: &str = n_("Window minimize");
const L_FULLSCREEN: &str = n_("Window full screen");
const L_PEEK: &str = n_("Peek at desktop");
const L_DESKTOP_SWITCH: &str = n_("Virtual desktop switching");
const L_FADING_POPUPS: &str = n_("Fading popups");
const L_SLIDING_POPUPS: &str = n_("Sliding popups");
const L_SLIDING_NOTIFICATIONS: &str = n_("Sliding notifications");
const L_LOGIN: &str = n_("Login");
const L_LOGOUT: &str = n_("Logout");
const L_MENUS: &str = n_("Effects on menus");
const L_DIALOGS: &str = n_("Effects on dialog boxes");
const L_LAYERS: &str = n_("Panels and popups");
const L_FADE_WINDOWS: &str = n_("Fading windows in and out");

const O_NONE: &str = n_("Do not use");
const O_FADE: &str = n_("Fade");
const O_GLIDE: &str = n_("Glide");
const O_SCALE: &str = n_("Scale");
const O_STRETCH: &str = n_("Stretch");
const O_SQUASH: &str = n_("Squash");
const O_MAGIC_LAMP: &str = n_("Magic Lamp");
const O_APERTURE: &str = n_("Window Aperture");
const O_EYE: &str = n_("Eye on Screen");
const O_SLIDE: &str = n_("Slide");
const O_TRADITIONAL: &str = n_("Traditional");
const O_FLY: &str = n_("Fly");
const O_MOVE: &str = n_("Move");
const O_POPIN: &str = n_("Pop in");
const O_GNOMED: &str = n_("GNOME style");
const O_SLIDE_VERT: &str = n_("Slide vertically");

fn opt(id: &str, label: &str) -> AnimOption {
    AnimOption { id: id.into(), label: tr(label), configurable: false }
}

fn choice(id: &str, label: &str, options: Vec<AnimOption>) -> EffectRow {
    EffectRow { id: id.into(), label: tr(label), kind: "choice".into(), options, value: String::new(), configurable: false }
}

fn toggle(id: &str, label: &str) -> EffectRow {
    EffectRow { id: id.into(), label: tr(label), kind: "toggle".into(), options: Vec::new(), value: String::new(), configurable: false }
}

// ═══════════════════════════════════════════════════════════════════════════
// KDE: KWin effects
// ═══════════════════════════════════════════════════════════════════════════

/// A built-in KWin effect: module name, label, whether it is enabled by default.
#[derive(Debug, Clone, PartialEq)]
pub struct KwinEffect {
    pub id: String,
    pub label: String,
    pub default_on: bool,
}

/// KWin 6.7 "Animation" groups: (row, KWin exclusive group, effects).
/// Order and contents — as in kcm_animations (`animationsdata.cpp`); names and
/// defaults — from the effects' `metadata.json` of the same release.
const KWIN_GROUPS: &[(&str, &str, &str, &[(&str, &str, bool)])] = &[
    ("openClose", L_OPEN_CLOSE, "toplevel-open-close-animation", &[
        ("fade", O_FADE, false),
        ("glide", O_GLIDE, false),
        ("scale", O_SCALE, true),
    ]),
    ("maximize", L_MAXIMIZE, "maximize", &[("maximize", O_STRETCH, true)]),
    ("minimize", L_MINIMIZE, "minimize", &[("squash", O_SQUASH, true), ("magiclamp", O_MAGIC_LAMP, false)]),
    ("fullscreen", L_FULLSCREEN, "fullscreen", &[("fullscreen", O_STRETCH, true)]),
    ("peekDesktop", L_PEEK, "show-desktop", &[
        ("windowaperture", O_APERTURE, true),
        ("eyeonscreen", O_EYE, false),
    ]),
    ("desktopSwitch", L_DESKTOP_SWITCH, "desktop-animations", &[
        ("slide", O_SLIDE, true),
        ("fadedesktop", O_FADE, false),
    ]),
];

/// Individual KWin checkbox effects (like the "other" ones in kcm_animations).
const KWIN_TOGGLES: &[(&str, &str, bool)] = &[
    ("fadingpopups", L_FADING_POPUPS, true),
    ("slidingpopups", L_SLIDING_POPUPS, true),
    ("slidingnotifications", L_SLIDING_NOTIFICATIONS, true),
    ("login", L_LOGIN, true),
    ("logout", L_LOGOUT, true),
];

/// Effects with an exclusive group from "Animation" (built-in ones outside the
/// [`KWIN_GROUPS`] table, scripts from the KDE store, `.so` modules) — the same records
/// as the "Desktop Effects" list ([`scan_kwin_effects`]).
fn third_party_effects(io: &dyn LookIo, ctx: &LookCtx) -> Vec<(String, KwinEffect)> {
    scan_kwin_effects(io, ctx)
        .into_iter()
        .filter(|m| !m.internal && !m.group.is_empty())
        .filter(|m| !KWIN_GROUPS.iter().any(|g| g.3.iter().any(|e| e.0 == m.id)))
        .map(|m| (m.group, KwinEffect { id: m.id, label: m.name, default_on: m.default_on }))
        .collect()
}

/// Effects of a group: built-in + third-party ones of the same exclusive group.
fn kwin_group_effects(group: &(&str, &str, &str, &[(&str, &str, bool)]), extra: &[(String, KwinEffect)]) -> Vec<KwinEffect> {
    let mut v: Vec<KwinEffect> = group
        .3
        .iter()
        .map(|(id, label, def)| KwinEffect { id: id.to_string(), label: tr(label), default_on: *def })
        .collect();
    v.extend(extra.iter().filter(|(g, _)| g == group.2).map(|(_, e)| e.clone()));
    v
}

fn kwin_enabled(io: &dyn LookIo, ctx: &LookCtx, id: &str, default_on: bool) -> bool {
    match kconfig_value(io, ctx, "kwinrc", "Plugins", &format!("{id}Enabled")) {
        Some(v) => v == "true",
        None => default_on,
    }
}

/// Writing `<effect>Enabled` by the KDE module's rule: equal to the default —
/// the key is deleted, otherwise it is written.
fn kwin_write(steps: &mut Vec<LookStep>, io: &dyn LookIo, ctx: &LookCtx, e: &KwinEffect, want: bool) {
    let key = format!("{}Enabled", e.id);
    let now = kconfig_value(io, ctx, "kwinrc", "Plugins", &key);
    if want == e.default_on {
        if now.is_some() {
            steps.push(kwrite("kwinrc", "Plugins", &key, None, true));
        }
    } else if now.as_deref() != Some(if want { "true" } else { "false" }) {
        steps.push(kwrite("kwinrc", "Plugins", &key, Some(if want { "true" } else { "false" }), true));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// KDE: "Desktop Effects" — all KWin effects
// ═══════════════════════════════════════════════════════════════════════════
//
// The reference is the KDE "Desktop Effects" module (kcm_kwin_effects, KWin 6.8:
// `src/kcms/common/effectsmodel.cpp`). Effects there are collected from three places,
// and we read the same three:
// * built-in — `kwin/builtin-effects/*.json` in the first data directory where
//   it exists (`QStandardPaths::locate`); the id is the file name;
// * scripts (`KWin/Effect` packages) — `kwin/effects/<id>/metadata.json` in
//   all data directories, the personal one first;
// * modules — `<Qt plugin root>/kwin/effects/plugins/*.so`, the metadata is
//   embedded in the module (CBOR, parsed by `plugin_journal`).
// A repeated id — the first one wins (like `shouldStore`). Internal effects
// (`internal`) are not shown. Effects already in "Animation"
// (six exclusive groups and five checkboxes) are excluded by the KDE module too —
// no point repeating them here.
//
// Enabling — `kwinrc [Plugins] <id>Enabled` by the same rule as
// "Animation" ([`kwin_write`]); KWin watches `kwinrc` (`KConfigWatcher`,
// `EffectsHandler::configChanged`) and loads and unloads the effect itself, while
// `reconfigure` over D-Bus is the fallback. In an exclusive group an enabled
// effect turns off its neighbors — like a radio button in KDE.
//
// "Configure…" — the same settings module KDE opens
// (`EffectsModel::requestConfigure`): `kwin/effects/configs/<module>`;
// from outside it is opened by `kcmshell6` (checked: a path with a subdirectory
// is accepted, a bare module name is not). A script with the shared module
// `kcm_kwin4_genericscripted` needs its `contents/config/main.xml` and
// `contents/ui/config.ui` and the arguments `<id> KWin/Effect` — as in KDE.

/// The shared settings module for scripted effects.
const GENERIC_SCRIPTED_KCM: &str = "kcm_kwin4_genericscripted";

/// A KWin effect as the "Desktop Effects" module sees it.
#[derive(Debug, Clone, PartialEq)]
struct KwinMeta {
    id: String,
    name: String,
    description: String,
    /// Category without translation (`Appearance`, `Accessibility`…).
    category: String,
    /// Exclusive group; empty — the effect stands alone.
    group: String,
    default_on: bool,
    internal: bool,
    /// The settings module and its arguments.
    config: Option<(String, Vec<String>)>,
}

/// Keys by interface language: `Name[ru_RU]`, `Name[ru]`, `Name` — and the same
/// for `Description`.
fn meta_keys(base: &str, lang: &str) -> Vec<String> {
    crate::installed_scan::locale_name_keys(lang)
        .into_iter()
        .map(|k| k.replacen("Name", base, 1))
        .collect()
}

fn localized(kplugin: &serde_json::Value, base: &str, lang: &str) -> String {
    meta_keys(base, lang)
        .iter()
        .find_map(|k| kplugin.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()))
        .unwrap_or("")
        .to_string()
}

/// A record from metadata. `top` — the whole `metadata.json` (for a `.so` module —
/// assembled from the embedded keys), `fallback_id` — the file or directory name,
/// `lang` — the interface language (`ru`, `pt-BR`).
fn kwin_meta(top: &serde_json::Value, fallback_id: &str, scripted: bool, lang: &str) -> KwinMeta {
    let kplugin = top.get("KPlugin").cloned().unwrap_or_default();
    let id = kplugin.get("Id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or(fallback_id).to_string();
    let effect = top.get("org.kde.kwin.effect").cloned().unwrap_or_default();
    let flag = |v: Option<&serde_json::Value>| match v {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) => s == "true",
        _ => false,
    };
    let (group, internal) = if scripted {
        (
            top.get("X-KWin-Exclusive-Category").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            flag(top.get("X-KWin-Internal")),
        )
    } else {
        (effect.get("exclusiveGroup").and_then(|v| v.as_str()).unwrap_or("").to_string(), flag(effect.get("internal")))
    };
    let name = localized(&kplugin, "Name", lang);
    KwinMeta {
        name: if name.is_empty() { id.clone() } else { name },
        description: localized(&kplugin, "Description", lang),
        category: kplugin.get("Category").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        group,
        default_on: flag(kplugin.get("EnabledByDefault")),
        internal,
        config: top
            .get("X-KDE-ConfigModule")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|m| (m.to_string(), Vec::new())),
        id,
    }
}

/// Data directories in `QStandardPaths` order: the personal one, then the system ones.
fn data_roots(ctx: &LookCtx) -> Vec<String> {
    let mut v = vec![ctx.data_home.trim_end_matches('/').to_string()];
    for d in &ctx.data_dirs {
        if !v.contains(d) {
            v.push(d.clone());
        }
    }
    v
}

/// The first data directory containing the file `rel` (`QStandardPaths::locate`).
fn locate_data(io: &dyn LookIo, ctx: &LookCtx, rel: &str) -> Option<String> {
    data_roots(ctx).into_iter().map(|d| format!("{d}/{rel}")).find(|p| io.read(p).is_some())
}

/// The settings module is installed (`kwin/effects/configs/<module>.so`).
fn config_module_installed(io: &dyn LookIo, ctx: &LookCtx, module: &str) -> bool {
    let file = format!("{module}.so");
    ctx.plugin_dirs.iter().any(|d| io.list_dir(&format!("{d}/kwin/effects/configs")).contains(&file))
}

/// All KWin effects on the machine (without the "supported" check).
fn scan_kwin_effects(io: &dyn LookIo, ctx: &LookCtx) -> Vec<KwinMeta> {
    let mut out: Vec<KwinMeta> = Vec::new();
    let push = |m: KwinMeta, out: &mut Vec<KwinMeta>| {
        if !out.iter().any(|e| e.id == m.id) {
            out.push(m);
        }
    };
    let roots = data_roots(ctx);
    let lang = crate::i18n::current_language();
    // Built-in: only the first directory where `builtin-effects` exists.
    for root in &roots {
        let dir = format!("{root}/kwin/builtin-effects");
        let files: Vec<String> = io.list_dir(&dir).into_iter().filter(|f| f.ends_with(".json")).collect();
        if files.is_empty() {
            continue;
        }
        for f in files {
            let Some(text) = io.read(&format!("{dir}/{f}")) else { continue };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
            push(kwin_meta(&json, f.trim_end_matches(".json"), false, &lang), &mut out);
        }
        break;
    }
    // Scripts.
    for root in &roots {
        let dir = format!("{root}/kwin/effects");
        for name in io.list_dir(&dir) {
            let Some(text) = io.read(&format!("{dir}/{name}/metadata.json")) else { continue };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
            let mut m = kwin_meta(&json, &name, true, &lang);
            if m.config.as_ref().is_some_and(|(module, _)| module == GENERIC_SCRIPTED_KCM) {
                let base = format!("kwin/effects/{}/contents", m.id);
                let has = locate_data(io, ctx, &format!("{base}/config/main.xml")).is_some()
                    && locate_data(io, ctx, &format!("{base}/ui/config.ui")).is_some();
                m.config = has.then(|| (GENERIC_SCRIPTED_KCM.to_string(), vec![m.id.clone(), "KWin/Effect".to_string()]));
            }
            push(m, &mut out);
        }
    }
    // `.so` modules.
    for root in &ctx.plugin_dirs {
        let dir = format!("{root}/kwin/effects/plugins");
        for file in io.list_dir(&dir) {
            let Some(stem) = file.strip_suffix(".so") else { continue };
            let Some(data) = io.read_bytes(&format!("{dir}/{file}")) else { continue };
            use crate::plugin_journal::embedded_value;
            let mut top = serde_json::Map::new();
            if let Some(k) = embedded_value(&data, "KPlugin", |v| v.is_object()) {
                top.insert("KPlugin".into(), k);
            }
            if let Some(e) = embedded_value(&data, "org.kde.kwin.effect", |v| v.is_object()) {
                top.insert("org.kde.kwin.effect".into(), e);
            }
            if let Some(c) = embedded_value(&data, "X-KDE-ConfigModule", |v| v.is_string()) {
                top.insert("X-KDE-ConfigModule".into(), c);
            }
            push(kwin_meta(&serde_json::Value::Object(top), stem, false, &lang), &mut out);
        }
    }
    // The settings module (not the shared one for scripts) must be installed on the machine.
    for m in &mut out {
        if m.config.as_ref().is_some_and(|(module, args)| args.is_empty() && !config_module_installed(io, ctx, module)) {
            m.config = None;
        }
    }
    out
}

/// Which of the effects the live KWin can show (`areEffectsSupported`).
/// None — KWin did not answer (another desktop, the test bench): we assume all.
fn kwin_supported(io: &dyn LookIo, ids: &[String]) -> Option<Vec<bool>> {
    if ids.is_empty() || !io.have("dbus-send") {
        return None;
    }
    let (ok, out) = io.run(&argv(&[
        "dbus-send",
        "--session",
        "--print-reply",
        "--dest=org.kde.KWin",
        "/Effects",
        "org.kde.kwin.Effects.areEffectsSupported",
        &format!("array:string:{}", ids.join(",")),
    ]));
    if !ok {
        return None;
    }
    let v: Vec<bool> = out
        .lines()
        .filter_map(|l| l.trim().strip_prefix("boolean "))
        .map(|b| b.trim() == "true")
        .collect();
    (v.len() == ids.len()).then_some(v)
}

/// Category label — the same eight KDE knows; others as is.
fn effect_category(raw: &str) -> String {
    const KNOWN: &[(&str, &str)] = &[
        ("Accessibility", n_("Accessibility")),
        ("Appearance", n_("Appearance")),
        ("Focus", n_("Focus")),
        ("Show Desktop Animation", n_("Peek at Desktop Animation")),
        ("Tools", n_("Tools")),
        ("Virtual Desktop Switching Animation", n_("Virtual Desktop Switching Animation")),
        ("Window Management", n_("Window Management")),
        ("Window Open/Close Animation", n_("Window Open/Close Animation")),
    ];
    match KNOWN.iter().find(|(k, _)| *k == raw) {
        Some((_, label)) => tr(label),
        None => raw.to_string(),
    }
}

/// An effect in the "Desktop Effects" list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesktopEffect {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Group header in the list.
    pub category: String,
    /// Exclusive group: at most one is enabled.
    pub group: String,
    pub enabled: bool,
    /// There is a settings page ("Configure…").
    pub configurable: bool,
    /// The search string (name, description, category) in lowercase.
    pub search: String,
}

/// List snapshot for the page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesktopEffectsState {
    /// The section exists (KDE only: effects belong to KWin).
    pub available: bool,
    pub effects: Vec<DesktopEffect>,
}

/// Effects the "Desktop Effects" list shows: without
/// internal ones and without those already shown in "Animation".
fn listed_effects(io: &dyn LookIo, ctx: &LookCtx) -> Vec<KwinMeta> {
    let mut v: Vec<KwinMeta> = scan_kwin_effects(io, ctx)
        .into_iter()
        .filter(|m| !m.internal)
        .filter(|m| !KWIN_GROUPS.iter().any(|g| g.2 == m.group))
        .filter(|m| !KWIN_TOGGLES.iter().any(|t| t.0 == m.id))
        .collect();
    let ids: Vec<String> = v.iter().map(|m| m.id.clone()).collect();
    if let Some(supported) = kwin_supported(io, &ids) {
        let mut keep = supported.into_iter();
        v.retain(|_| keep.next().unwrap_or(true));
    }
    v
}

/// Snapshot of the "Desktop Effects" list.
pub fn desktop_effects_state(io: &dyn LookIo, ctx: &LookCtx) -> DesktopEffectsState {
    if ctx.de != "kde" {
        return DesktopEffectsState { available: false, effects: Vec::new() };
    }
    let can_configure = io.have("kcmshell6");
    let mut effects: Vec<DesktopEffect> = listed_effects(io, ctx)
        .into_iter()
        .map(|m| {
            let category = effect_category(&m.category);
            DesktopEffect {
                enabled: kwin_enabled(io, ctx, &m.id, m.default_on),
                configurable: can_configure && m.config.is_some(),
                search: format!("{}\n{}\n{}", m.name, m.description, category).to_lowercase(),
                id: m.id,
                name: m.name,
                description: m.description,
                category,
                group: m.group,
            }
        })
        .collect();
    // KDE order: category, exclusive group, name.
    effects.sort_by(|a, b| (&a.category, &a.group, &a.name).cmp(&(&b.category, &b.group, &b.name)));
    DesktopEffectsState { available: true, effects }
}

/// Plan: enable or disable an effect of the list.
pub fn plan_desktop_effect(io: &dyn LookIo, ctx: &LookCtx, id: &str, on: bool) -> Vec<LookStep> {
    if ctx.de != "kde" {
        return Vec::new();
    }
    let all = listed_effects(io, ctx);
    let Some(me) = all.iter().find(|m| m.id == id) else { return Vec::new() };
    let mut steps = Vec::new();
    let as_effect = |m: &KwinMeta| KwinEffect { id: m.id.clone(), label: m.name.clone(), default_on: m.default_on };
    if on && !me.group.is_empty() {
        for other in all.iter().filter(|m| m.group == me.group && m.id != me.id) {
            if kwin_enabled(io, ctx, &other.id, other.default_on) {
                kwin_write(&mut steps, io, ctx, &as_effect(other), false);
            }
        }
    }
    kwin_write(&mut steps, io, ctx, &as_effect(me), on);
    if !steps.is_empty() {
        steps.push(kwin_reconfigure());
    }
    steps
}

/// Plan: all effects of the list to defaults.
pub fn plan_desktop_effects_defaults(io: &dyn LookIo, ctx: &LookCtx) -> Vec<LookStep> {
    if ctx.de != "kde" {
        return Vec::new();
    }
    let mut steps = Vec::new();
    for m in listed_effects(io, ctx) {
        let e = KwinEffect { id: m.id.clone(), label: m.name.clone(), default_on: m.default_on };
        kwin_write(&mut steps, io, ctx, &e, m.default_on);
    }
    if !steps.is_empty() {
        steps.push(kwin_reconfigure());
    }
    steps
}

fn run_effect_steps(io: &dyn LookIo, ctx: &LookCtx, steps: &[LookStep]) -> String {
    if steps.is_empty() {
        return String::new();
    }
    match run_steps(io, ctx, steps) {
        Ok(()) => tr("Desktop effect settings applied."),
        Err(e) => e,
    }
}

/// Enable or disable an effect: the sentence for the popup message.
pub fn set_desktop_effect(io: &dyn LookIo, ctx: &LookCtx, id: &str, on: bool) -> String {
    run_effect_steps(io, ctx, &plan_desktop_effect(io, ctx, id, on))
}

/// "Defaults" for the list.
pub fn desktop_effects_defaults(io: &dyn LookIo, ctx: &LookCtx) -> String {
    run_effect_steps(io, ctx, &plan_desktop_effects_defaults(io, ctx))
}

/// The command that opens an effect's settings page; None — no page.
/// It is searched among all effects, not only the list: "Configure…" is also on
/// "Animation" rows (glide, magic lamp, login…).
pub fn effect_config_argv(io: &dyn LookIo, ctx: &LookCtx, id: &str) -> Option<Vec<String>> {
    if ctx.de != "kde" {
        return None;
    }
    let (module, args) = scan_kwin_effects(io, ctx).into_iter().find(|m| m.id == id)?.config?;
    let mut a = vec!["kcmshell6".to_string()];
    if !args.is_empty() {
        // `kcmshell6` splits `--args` by spaces with shell quoting.
        let quoted: Vec<String> = args
            .iter()
            .map(|x| if x.contains(|c: char| c.is_whitespace() || c == '\'' || c == '"') { format!("'{}'", x.replace('\'', "'\\''")) } else { x.clone() })
            .collect();
        a.push("--args".into());
        a.push(quoted.join(" "));
    }
    a.push(format!("kwin/effects/configs/{module}"));
    Some(a)
}

/// Effects with their own settings page (for "Animation" rows).
fn configurable_ids(io: &dyn LookIo, ctx: &LookCtx) -> Vec<String> {
    if ctx.de != "kde" || !io.have("kcmshell6") {
        return Vec::new();
    }
    scan_kwin_effects(io, ctx).into_iter().filter(|m| m.config.is_some()).map(|m| m.id).collect()
}

/// "Configure…": open the effect's settings page. Empty — opened.
pub fn configure_effect(io: &dyn LookIo, ctx: &LookCtx, id: &str) -> String {
    let Some(a) = effect_config_argv(io, ctx, id) else {
        return tr("This effect has no settings of its own.");
    };
    if !io.have(&a[0]) {
        return trf("The program %1 is not installed.", &[&a[0]]);
    }
    if io.spawn(&a) {
        String::new()
    } else {
        trf("Could not open the settings: %1 did not start.", &[&a[0]])
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Hyprland and niri: our file
// ═══════════════════════════════════════════════════════════════════════════

pub const ANIM_TAG: &str = "animations";
pub const ANIM_STEM: &str = "rexon-animations";

/// Hyprland 0.56 standard animation set (`/usr/share/hypr/hyprland.lua`):
/// (leaf, speed in deciseconds, curve).
const HYPR_LEAVES: &[(&str, f64, &str)] = &[
    ("global", 10.0, "default"),
    ("border", 5.39, "rexonQuint"),
    ("windows", 4.79, "rexonEasy"),
    ("windowsIn", 4.1, "rexonEasy"),
    ("windowsOut", 1.49, "rexonLinear"),
    ("fadeIn", 1.73, "rexonAlmostLinear"),
    ("fadeOut", 1.46, "rexonAlmostLinear"),
    ("fade", 3.03, "rexonQuick"),
    ("layers", 3.81, "rexonQuint"),
    ("layersIn", 4.0, "rexonQuint"),
    ("layersOut", 1.5, "rexonLinear"),
    ("fadeLayersIn", 1.79, "rexonAlmostLinear"),
    ("fadeLayersOut", 1.39, "rexonAlmostLinear"),
    ("workspaces", 1.94, "rexonAlmostLinear"),
    ("workspacesIn", 1.21, "rexonAlmostLinear"),
    ("workspacesOut", 1.94, "rexonAlmostLinear"),
    ("zoomFactor", 7.0, "rexonQuick"),
];

const HYPR_BEZIERS: &[(&str, [f64; 4])] = &[
    ("rexonQuint", [0.23, 1.0, 0.32, 1.0]),
    ("rexonLinear", [0.0, 0.0, 1.0, 1.0]),
    ("rexonAlmostLinear", [0.5, 0.5, 0.75, 1.0]),
    ("rexonQuick", [0.15, 0.0, 0.1, 1.0]),
];

/// Default Hyprland row values (as in the standard config).
fn hypr_defaults() -> BTreeMap<String, String> {
    [("openClose", "popin"), ("desktopSwitch", "fade"), ("layers", "true"), ("fade", "true")]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn niri_defaults() -> BTreeMap<String, String> {
    [("openClose", "true"), ("maximize", "true"), ("desktopSwitch", "true")]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn n(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Our Hyprland file.
pub fn hypr_content(s: &AnimSettings, syntax: WmSyntax) -> String {
    let lua = syntax == WmSyntax::HyprLua;
    let c = syntax.comment();
    let mut state = s.values.clone();
    state.insert("enabled".into(), s.enabled.to_string());
    state.insert("factor".into(), fmt_factor(s.factor));
    let mut out = state_line(c, &state);
    out.push_str(&format!("{c} Rexon: animations. This file is rewritten by Rexon; change animations in Rexon instead.\n"));
    out.push_str(&if lua {
        format!("hl.config({{ animations = {{ enabled = {} }} }})\n", s.enabled)
    } else {
        format!("animations {{\n    enabled = {}\n}}\n", s.enabled)
    });
    let defaults = hypr_defaults();
    let custom = (s.factor - 1.0).abs() > 1e-9 || defaults.iter().any(|(k, v)| s.get(k) != v);
    if !s.enabled || !custom {
        return out;
    }
    for (name, p) in HYPR_BEZIERS {
        out.push_str(&if lua {
            format!(
                "hl.curve(\"{name}\", {{ type = \"bezier\", points = {{ {{{}, {}}}, {{{}, {}}} }} }})\n",
                n(p[0]), n(p[1]), n(p[2]), n(p[3])
            )
        } else {
            format!("bezier = {name}, {}, {}, {}, {}\n", n(p[0]), n(p[1]), n(p[2]), n(p[3]))
        });
    }
    if lua {
        out.push_str("hl.curve(\"rexonEasy\", { type = \"spring\", mass = 1, stiffness = 238.1191, dampening = 24.21279333 })\n");
    }
    let windows = s.get("openClose").to_string();
    let workspaces = s.get("desktopSwitch").to_string();
    for (leaf, speed, curve) in HYPR_LEAVES {
        let on = match *leaf {
            "windows" | "windowsIn" | "windowsOut" => windows != "none",
            "workspaces" | "workspacesIn" | "workspacesOut" => workspaces != "none",
            "layers" | "layersIn" | "layersOut" | "fadeLayersIn" | "fadeLayersOut" => s.on("layers"),
            "fadeIn" | "fadeOut" | "fade" => s.on("fade"),
            _ => true,
        };
        let style = match *leaf {
            "windowsIn" | "windowsOut" => match windows.as_str() {
                "slide" => "slide",
                "gnomed" => "gnomed",
                _ => "popin 87%",
            },
            "workspaces" | "workspacesIn" | "workspacesOut" => match workspaces.as_str() {
                "slide" => "slide",
                "slidevert" => "slidevert",
                _ => "fade",
            },
            "layersIn" | "layersOut" => "fade",
            _ => "",
        };
        let speed = n((speed * s.factor).max(0.01));
        // hyprland.conf has no springs — windows there follow the same curve as the border.
        let curve = if !lua && *curve == "rexonEasy" { "rexonQuint" } else { curve };
        out.push_str(&if lua {
            if !on {
                format!("hl.animation({{ leaf = \"{leaf}\", enabled = false }})\n")
            } else {
                let kind = if curve == "rexonEasy" { "spring" } else { "bezier" };
                let style = if style.is_empty() { String::new() } else { format!(", style = \"{style}\"") };
                format!("hl.animation({{ leaf = \"{leaf}\", enabled = true, speed = {speed}, {kind} = \"{curve}\"{style} }})\n")
            }
        } else if !on {
            format!("animation = {leaf}, 0\n")
        } else {
            let style = if style.is_empty() { String::new() } else { format!(", {style}") };
            format!("animation = {leaf}, 1, {speed}, {curve}{style}\n")
        });
    }
    out
}

/// Our niri file.
pub fn niri_content(s: &AnimSettings) -> String {
    let mut state = s.values.clone();
    state.insert("enabled".into(), s.enabled.to_string());
    state.insert("factor".into(), fmt_factor(s.factor));
    let mut out = state_line("//", &state);
    out.push_str("// Rexon: animations. This file is rewritten by Rexon; change animations in Rexon instead.\n");
    out.push_str("animations {\n");
    if !s.enabled {
        out.push_str("    off\n");
    } else {
        if (s.factor - 1.0).abs() > 1e-9 {
            out.push_str(&format!("    slowdown {}\n", n(s.factor)));
        }
        if !s.on("openClose") {
            out.push_str("    window-open { off; }\n    window-close { off; }\n");
        }
        if !s.on("maximize") {
            out.push_str("    window-resize { off; }\n");
        }
        if !s.on("desktopSwitch") {
            out.push_str("    workspace-switch { off; }\n");
        }
    }
    out.push_str("}\n");
    out
}

fn settings_from_state(map: &BTreeMap<String, String>, defaults: BTreeMap<String, String>) -> AnimSettings {
    let mut values = defaults;
    for (k, v) in map {
        if values.contains_key(k) {
            values.insert(k.clone(), v.clone());
        }
    }
    AnimSettings {
        enabled: map.get("enabled").map(|v| v != "false").unwrap_or(true),
        factor: map.get("factor").and_then(|v| v.parse().ok()).filter(|f: &f64| *f > 0.0).unwrap_or(1.0),
        values,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Snapshot
// ═══════════════════════════════════════════════════════════════════════════

/// Rows (without values) for a desktop.
fn row_defs(io: &dyn LookIo, ctx: &LookCtx) -> Vec<EffectRow> {
    match ctx.de.as_str() {
        "kde" => {
            let extra = third_party_effects(io, ctx);
            let mut rows: Vec<EffectRow> = KWIN_GROUPS
                .iter()
                .map(|g| {
                    let mut options = vec![opt("none", O_NONE)];
                    options.extend(
                        kwin_group_effects(g, &extra).into_iter().map(|e| AnimOption { id: e.id, label: e.label, configurable: false }),
                    );
                    choice(g.0, g.1, options)
                })
                .collect();
            rows.extend(KWIN_TOGGLES.iter().map(|(id, label, _)| toggle(id, label)));
            rows
        }
        "cinnamon-mint" => vec![
            choice("open", L_OPEN, vec![opt("none", O_NONE), opt("traditional", O_TRADITIONAL), opt("fly", O_FLY), opt("move", O_MOVE)]),
            choice("close", L_CLOSE, vec![opt("none", O_NONE), opt("traditional", O_TRADITIONAL), opt("fly", O_FLY)]),
            choice("minimize", L_MINIMIZE, vec![opt("none", O_NONE), opt("traditional", O_TRADITIONAL), opt("fly", O_FLY), opt("fade", O_FADE)]),
            toggle("resize", L_RESIZE),
            toggle("menus", L_MENUS),
            toggle("dialogs", L_DIALOGS),
            toggle("login", L_LOGIN),
        ],
        "hyprland" => vec![
            choice("openClose", L_OPEN_CLOSE, vec![opt("none", O_NONE), opt("popin", O_POPIN), opt("slide", O_SLIDE), opt("gnomed", O_GNOMED)]),
            choice("desktopSwitch", L_DESKTOP_SWITCH, vec![opt("none", O_NONE), opt("fade", O_FADE), opt("slide", O_SLIDE), opt("slidevert", O_SLIDE_VERT)]),
            toggle("layers", L_LAYERS),
            toggle("fade", L_FADE_WINDOWS),
        ],
        "niri" => vec![toggle("openClose", L_OPEN_CLOSE), toggle("maximize", L_RESIZE), toggle("desktopSwitch", L_DESKTOP_SWITCH)],
        _ => Vec::new(),
    }
}

/// Cinnamon: row → `org.cinnamon` key.
const CINNAMON_KEYS: &[(&str, &str)] = &[
    ("open", "desktop-effects-map"),
    ("close", "desktop-effects-close"),
    ("minimize", "desktop-effects-minimize"),
    ("resize", "desktop-effects-change-size"),
    ("menus", "desktop-effects-on-menus"),
    ("dialogs", "desktop-effects-on-dialogs"),
    ("login", "startup-animation"),
];

/// Cinnamon `window-effect-speed` (0 slower … 2 faster) ↔ multiplier.
fn cinnamon_speed_to_factor(v: &str) -> f64 {
    match v {
        "0" => 2.0,
        "2" => 0.5,
        _ => 1.0,
    }
}

fn factor_to_cinnamon_speed(f: f64) -> &'static str {
    if f >= 1.5 {
        "0"
    } else if f <= 0.75 {
        "2"
    } else {
        "1"
    }
}

fn gtk_ini_enabled(io: &dyn LookIo, ctx: &LookCtx) -> bool {
    let path = crate::gtk_sanitizer::gtk_ini_path(&ctx.config_home, "3.0");
    let map = crate::gtk_sanitizer::read_ini_settings(io.read(&path).as_deref());
    map.get("gtk-enable-animations").map(|v| v != "false" && v != "0").unwrap_or(true)
}

fn gnome_schema(de: &str) -> &'static str {
    match de {
        "mate" => "org.mate.interface",
        "cinnamon-mint" => "org.cinnamon.desktop.interface",
        _ => "org.gnome.desktop.interface",
    }
}

/// The settings as they are now.
pub fn read_settings(io: &dyn LookIo, ctx: &LookCtx) -> AnimSettings {
    let mut s = AnimSettings { enabled: true, factor: 1.0, values: BTreeMap::new() };
    match ctx.de.as_str() {
        "kde" => {
            let f = kconfig_value(io, ctx, "kdeglobals", "KDE", "AnimationDurationFactor")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0);
            s.enabled = f > 0.0;
            s.factor = if f > 0.0 { f } else { 1.0 };
            let extra = third_party_effects(io, ctx);
            for g in KWIN_GROUPS {
                let value = kwin_group_effects(g, &extra)
                    .into_iter()
                    .find(|e| kwin_enabled(io, ctx, &e.id, e.default_on))
                    .map(|e| e.id)
                    .unwrap_or_else(|| "none".into());
                s.values.insert(g.0.into(), value);
            }
            for (id, _, def) in KWIN_TOGGLES {
                s.values.insert(id.to_string(), kwin_enabled(io, ctx, id, *def).to_string());
            }
        }
        "gnome" | "budgie" | "pantheon" | "mate" => {
            s.enabled = gsettings_get(io, gnome_schema(&ctx.de), "enable-animations").as_deref() != Some("false");
        }
        "cinnamon-mint" => {
            s.enabled = gsettings_get(io, "org.cinnamon", "desktop-effects-workspace").as_deref() != Some("false");
            s.factor = cinnamon_speed_to_factor(&gsettings_get(io, "org.cinnamon", "window-effect-speed").unwrap_or_default());
            for (id, key) in CINNAMON_KEYS {
                let v = gsettings_get(io, "org.cinnamon", key).unwrap_or_else(|| {
                    if matches!(*id, "open" | "close" | "minimize") { "traditional".into() } else { "true".into() }
                });
                s.values.insert(id.to_string(), v);
            }
        }
        "xfce" => {
            s.enabled = xfconf_get(io, "xsettings", "/Gtk/EnableAnimations").as_deref() != Some("false");
        }
        "hyprland" | "niri" => {
            let own = read_own_wm_file(io, ctx, &ctx.de, ANIM_STEM).map(|(_, t)| parse_state_line(&t)).unwrap_or_default();
            let defaults = if ctx.de == "hyprland" { hypr_defaults() } else { niri_defaults() };
            s = settings_from_state(&own, defaults);
        }
        _ => {
            s.enabled = gtk_ini_enabled(io, ctx);
        }
    }
    s
}

/// Whether the desktop's window manager has window animations that Rexon
/// configures. Where there are none (Marco, xfwm4 and desktops with no animation
/// settings of their own at all — [`note`]), all that is left of the "Animation" section is a single
/// "No animations" checkbox for programs: there is no such section in the sidebar,
/// and the checkbox is the "⚙️ Settings" view of the "GTK themes" section
/// (`categories::SECTION_SETTINGS`, Wehrwolfmann's decision of 08.11.2026).
pub fn has_window_animations(de: &str) -> bool {
    matches!(de, "kde" | "gnome" | "budgie" | "pantheon" | "cinnamon-mint" | "cinnamon" | "hyprland" | "niri")
}

/// Whether there is a speed slider.
pub fn has_speed(de: &str) -> bool {
    matches!(de, "kde" | "cinnamon-mint" | "hyprland" | "niri")
}

/// The explanation under the section.
pub fn note(de: &str) -> String {
    match de {
        "kde" => tr("Not every application follows the speed: GTK applications can only turn animations on or off."),
        "gnome" | "budgie" | "pantheon" => trf("%1 has no animation speed setting — animations can only be turned on or off.", &[&de_label(de)]),
        "cinnamon-mint" => tr("Cinnamon has three speeds; the slider picks the nearest one."),
        "mate" => tr("The Marco window manager has no window animations; the switch turns off animations inside applications."),
        "xfce" => tr("The Xfwm4 window manager has no window animations; the switch turns off animations inside applications."),
        "hyprland" => tr("Speed and styles rewrite the standard Hyprland animation set: your own animation curves in the config are overridden."),
        "niri" => String::new(),
        other => trf("%1 has no animation settings of its own; the switch turns off animations inside GTK applications.", &[&de_label(other)]),
    }
}

/// Snapshot for the page.
pub fn read_state(io: &dyn LookIo, ctx: &LookCtx) -> AnimState {
    let s = read_settings(io, ctx);
    let mut rows = row_defs(io, ctx);
    let configurable = configurable_ids(io, ctx);
    for r in &mut rows {
        r.value = s.get(&r.id).to_string();
        if r.kind == "toggle" {
            r.configurable = configurable.contains(&r.id);
        }
        for o in &mut r.options {
            o.configurable = configurable.contains(&o.id);
        }
    }
    AnimState {
        de: ctx.de.clone(),
        enabled: s.enabled,
        has_speed: has_speed(&ctx.de),
        speed_steps: SPEED_STEPS.to_vec(),
        speed_index: speed_index(s.factor),
        rows,
        note: note(&ctx.de),
    }
}

/// The desktop's default values.
pub fn default_settings(io: &dyn LookIo, ctx: &LookCtx) -> AnimSettings {
    let mut values = BTreeMap::new();
    match ctx.de.as_str() {
        "kde" => {
            let extra = third_party_effects(io, ctx);
            for g in KWIN_GROUPS {
                let def = kwin_group_effects(g, &extra).into_iter().find(|e| e.default_on).map(|e| e.id);
                values.insert(g.0.to_string(), def.unwrap_or_else(|| "none".into()));
            }
            for (id, _, def) in KWIN_TOGGLES {
                values.insert(id.to_string(), def.to_string());
            }
        }
        "cinnamon-mint" => {
            for (id, _) in CINNAMON_KEYS {
                let v = if matches!(*id, "open" | "close" | "minimize") { "traditional" } else { "true" };
                values.insert(id.to_string(), v.to_string());
            }
        }
        "hyprland" => values = hypr_defaults(),
        "niri" => values = niri_defaults(),
        _ => {}
    }
    AnimSettings { enabled: true, factor: 1.0, values }
}

// ═══════════════════════════════════════════════════════════════════════════
// Write plan
// ═══════════════════════════════════════════════════════════════════════════

/// The new state after a change.
pub fn changed(io: &dyn LookIo, ctx: &LookCtx, old: &AnimSettings, change: &AnimChange) -> AnimSettings {
    let mut s = old.clone();
    match change {
        AnimChange::Enabled(on) => s.enabled = *on,
        AnimChange::Speed(f) => {
            s.factor = *f;
            s.enabled = true;
        }
        AnimChange::Row { id, value } => {
            s.values.insert(id.clone(), value.clone());
        }
        AnimChange::Defaults => s = default_settings(io, ctx),
    }
    s
}

/// The plan for going from `old` to `new`. Only what changes is written:
/// reapplying the same value gives an empty plan.
pub fn plan_settings(io: &dyn LookIo, ctx: &LookCtx, old: &AnimSettings, new: &AnimSettings) -> Vec<LookStep> {
    let mut steps = Vec::new();
    let bool_s = |b: bool| if b { "true" } else { "false" };
    match ctx.de.as_str() {
        "kde" => {
            let factor_of = |s: &AnimSettings| if s.enabled { s.factor } else { 0.0 };
            if (factor_of(old) - factor_of(new)).abs() > 1e-9 {
                let f = factor_of(new);
                let value = if (f - 1.0).abs() < 1e-9 { None } else { Some(fmt_factor(f)) };
                steps.push(kwrite("kdeglobals", "KDE", "AnimationDurationFactor", value.as_deref(), false));
                steps.push(kglobals_notify());
            }
            let extra = third_party_effects(io, ctx);
            let mut kwin = Vec::new();
            for g in KWIN_GROUPS {
                if old.get(g.0) == new.get(g.0) {
                    continue;
                }
                for e in kwin_group_effects(g, &extra) {
                    kwin_write(&mut kwin, io, ctx, &e, e.id == new.get(g.0));
                }
            }
            for (id, label, def) in KWIN_TOGGLES {
                if old.get(id) != new.get(id) {
                    let e = KwinEffect { id: id.to_string(), label: label.to_string(), default_on: *def };
                    kwin_write(&mut kwin, io, ctx, &e, new.on(id));
                }
            }
            if !kwin.is_empty() {
                steps.extend(kwin);
                steps.push(kwin_reconfigure());
            }
            if old.enabled != new.enabled {
                steps.push(LookStep::GtkIni { key: "gtk-enable-animations".into(), value: bool_s(new.enabled).into() });
            }
        }
        "gnome" | "budgie" | "pantheon" | "mate" => {
            if old.enabled != new.enabled {
                steps.push(gsettings_set(gnome_schema(&ctx.de), "enable-animations", bool_s(new.enabled)));
                if ctx.de == "mate" {
                    steps.push(gsettings_set("org.mate.interface", "gtk-enable-animations", bool_s(new.enabled)));
                }
            }
        }
        "cinnamon-mint" => {
            if old.enabled != new.enabled {
                steps.push(gsettings_set("org.cinnamon", "desktop-effects-workspace", bool_s(new.enabled)));
                steps.push(gsettings_set("org.cinnamon.desktop.interface", "enable-animations", bool_s(new.enabled)));
            }
            if factor_to_cinnamon_speed(old.factor) != factor_to_cinnamon_speed(new.factor) {
                steps.push(gsettings_set("org.cinnamon", "window-effect-speed", factor_to_cinnamon_speed(new.factor)));
            }
            for (id, key) in CINNAMON_KEYS {
                if old.get(id) != new.get(id) {
                    let v = new.get(id);
                    let gv = if v == "true" || v == "false" { v.to_string() } else { format!("'{v}'") };
                    steps.push(gsettings_set("org.cinnamon", key, &gv));
                }
            }
        }
        "xfce" => {
            if old.enabled != new.enabled {
                steps.push(xfconf_set("xsettings", "/Gtk/EnableAnimations", "bool", bool_s(new.enabled)));
            }
        }
        "hyprland" | "niri" => {
            // Our file describes the WHOLE state: compare with what is there.
            let de = ctx.de.clone();
            let new2 = new.clone();
            let content = move |syntax: WmSyntax| {
                if de == "hyprland" { hypr_content(&new2, syntax) } else { niri_content(&new2) }
            };
            let inc = wm_include(ctx, &ctx.de, ANIM_TAG, ANIM_STEM, &content);
            let current = inc.targets.iter().find(|t| io.read(&t.config).is_some());
            let unchanged = current.is_some_and(|t| {
                io.read(&t.own_path).as_deref() == Some(t.own_content.as_str())
                    && io.read(&t.config).is_some_and(|c| c.contains(&t.include_line))
            });
            if !unchanged {
                steps.push(LookStep::Wm(inc));
            }
            if old.enabled != new.enabled {
                steps.push(LookStep::GtkIni { key: "gtk-enable-animations".into(), value: bool_s(new.enabled).into() });
            }
        }
        _ => {
            if old.enabled != new.enabled {
                steps.push(LookStep::GtkIni { key: "gtk-enable-animations".into(), value: bool_s(new.enabled).into() });
            }
        }
    }
    steps
}

/// The plan of one change.
pub fn plan_change(io: &dyn LookIo, ctx: &LookCtx, change: &AnimChange) -> Vec<LookStep> {
    let old = read_settings(io, ctx);
    let new = changed(io, ctx, &old, change);
    plan_settings(io, ctx, &old, &new)
}

/// Apply a change: the ready sentence for the popup message.
pub fn apply_change(io: &dyn LookIo, ctx: &LookCtx, change: &AnimChange) -> String {
    let steps = plan_change(io, ctx, change);
    if steps.is_empty() {
        return String::new();
    }
    match run_steps(io, ctx, &steps) {
        Ok(()) => tr("Animation settings applied."),
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {

    /// The "Current desktop" choice drives the "Appearance" sections: the desktop and the session
    /// kind are the selected one's; "auto", empty and the same desktop — the session as is.
    #[test]
    fn chosen_desktop_leads_the_look_sections() {
        let mut kde = LookCtx::new("kde", "/h", None);
        kde.wayland = true;
        let mate = kde.clone().for_chosen("mate");
        assert_eq!((mate.de.as_str(), mate.wayland, mate.home.as_str()), ("mate", false, "/h"));
        for same in ["", "auto", "kde"] {
            assert_eq!(kde.clone().for_chosen(same), kde, "{same:?}");
        }
        set_chosen_desktop(" wayfire ");
        assert_eq!(chosen_desktop(), "wayfire");
        assert_eq!(LookCtx::for_look().de, "wayfire");
        set_chosen_desktop("");
        assert_eq!(LookCtx::for_look(), LookCtx::from_env());
    }
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A fake machine: files in memory, gsettings/xfconf/kwriteconfig6 —
    /// in the same memory, compositor validators always say "yes".
    #[derive(Default)]
    pub struct FakeIo {
        pub files: RefCell<HashMap<String, String>>,
        pub gs: RefCell<HashMap<String, String>>,
        pub ran: RefCell<Vec<Vec<String>>>,
        pub cfg: String,
        /// Binary files (`.so` modules).
        pub bins: RefCell<HashMap<String, Vec<u8>>>,
        /// Started without waiting (`kcmshell6`) — recorded only.
        pub spawned: RefCell<Vec<Vec<String>>>,
    }

    impl LookIo for FakeIo {
        fn run(&self, a: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(a.to_vec());
            match a[0].as_str() {
                "gsettings" => {
                    let k = format!("{} {}", a[2], a[3]);
                    if a[1] == "get" {
                        let v = self.gs.borrow().get(&k).cloned();
                        return (v.is_some(), v.unwrap_or_default());
                    }
                    self.gs.borrow_mut().insert(k, a[4].clone());
                    (true, String::new())
                }
                "xfconf-query" => {
                    let k = format!("{} {}", a[2], a[4]);
                    if a.len() == 5 {
                        let v = self.gs.borrow().get(&k).cloned();
                        return (v.is_some(), v.unwrap_or_default());
                    }
                    self.gs.borrow_mut().insert(k, a.last().unwrap().clone());
                    (true, String::new())
                }
                "kwriteconfig6" => {
                    let path = format!("{}/{}", self.cfg, a[2]);
                    let (group, key) = (&a[4], &a[6]);
                    let text = self.files.borrow().get(&path).cloned();
                    let new = if a.contains(&"--delete".to_string()) {
                        text.unwrap_or_default()
                            .lines()
                            .filter(|l| !l.starts_with(&format!("{key}=")))
                            .map(|l| format!("{l}\n"))
                            .collect()
                    } else {
                        crate::gtk_sanitizer::write_ini_setting_in(text.as_deref(), group, key, a.last().unwrap())
                    };
                    self.files.borrow_mut().insert(path, new);
                    (true, String::new())
                }
                _ => (true, String::new()),
            }
        }
        fn have(&self, _: &str) -> bool {
            true
        }
        fn read(&self, p: &str) -> Option<String> {
            self.files.borrow().get(p).cloned()
        }
        fn write(&self, p: &str, t: &str) -> bool {
            self.files.borrow_mut().insert(p.into(), t.into());
            true
        }
        fn read_bytes(&self, p: &str) -> Option<Vec<u8>> {
            self.bins.borrow().get(p).cloned().or_else(|| self.read(p).map(String::into_bytes))
        }
        /// Names directly in the directory — from the keys of in-memory files.
        fn list_dir(&self, dir: &str) -> Vec<String> {
            let prefix = format!("{}/", dir.trim_end_matches('/'));
            let mut v: Vec<String> = self
                .files
                .borrow()
                .keys()
                .chain(self.bins.borrow().keys())
                .filter_map(|k| k.strip_prefix(&prefix))
                .map(|rest| rest.split('/').next().unwrap_or("").to_string())
                .collect();
            v.sort();
            v.dedup();
            v
        }
        fn spawn(&self, a: &[String]) -> bool {
            self.spawned.borrow_mut().push(a.to_vec());
            true
        }
    }

    fn ctx(de: &str) -> LookCtx {
        LookCtx::new(de, "/h", Some("/h/.config"))
    }

    fn fake() -> FakeIo {
        FakeIo { cfg: "/h/.config".into(), ..Default::default() }
    }

    fn apply(io: &FakeIo, c: &LookCtx, ch: AnimChange) {
        let steps = plan_change(io, c, &ch);
        run_steps(io, c, &steps).unwrap();
    }

    #[test]
    fn kde_speed_effects_repeat_and_revert() {
        crate::i18n::set_language(Some("en"));
        let io = fake();
        let c = ctx("kde");
        io.files.borrow_mut().insert("/h/.config/kwinrc".into(), "[Plugins]\nscaleEnabled=false\n".into());
        let before = read_settings(&io, &c);
        assert_eq!(before.get("openClose"), "none");
        assert_eq!(before.get("minimize"), "squash");
        apply(&io, &c, AnimChange::Speed(0.5));
        assert!(io.files.borrow()["/h/.config/kdeglobals"].contains("AnimationDurationFactor=0.5"));
        apply(&io, &c, AnimChange::Row { id: "openClose".into(), value: "glide".into() });
        let kwinrc = io.files.borrow()["/h/.config/kwinrc"].clone();
        assert!(kwinrc.contains("glideEnabled=true"), "{kwinrc}");
        assert!(kwinrc.contains("scaleEnabled=false"), "{kwinrc}");
        // A repeat — an empty plan.
        assert!(plan_change(&io, &c, &AnimChange::Row { id: "openClose".into(), value: "glide".into() }).is_empty());
        assert!(plan_change(&io, &c, &AnimChange::Speed(0.5)).is_empty());
        // "No animations" → 0, back → the previous multiplier.
        apply(&io, &c, AnimChange::Enabled(false));
        assert!(io.files.borrow()["/h/.config/kdeglobals"].contains("AnimationDurationFactor=0"));
        assert!(io.files.borrow()["/h/.config/gtk-3.0/settings.ini"].contains("gtk-enable-animations=false"));
        // Rollback: return to the original snapshot.
        let back = plan_settings(&io, &c, &read_settings(&io, &c), &before);
        run_steps(&io, &c, &back).unwrap();
        let after = read_settings(&io, &c);
        assert_eq!(after.values, before.values);
        assert_eq!(after.factor, before.factor);
        assert!(!io.files.borrow()["/h/.config/kwinrc"].contains("glideEnabled"));
        assert!(!io.files.borrow()["/h/.config/kdeglobals"].contains("AnimationDurationFactor"));
    }

    #[test]
    fn gnome_cinnamon_xfce_keys() {
        let io = fake();
        apply(&io, &ctx("gnome"), AnimChange::Enabled(false));
        assert_eq!(io.gs.borrow()["org.gnome.desktop.interface enable-animations"], "false");
        let io = fake();
        let c = ctx("cinnamon-mint");
        apply(&io, &c, AnimChange::Speed(2.0));
        apply(&io, &c, AnimChange::Row { id: "minimize".into(), value: "fade".into() });
        assert_eq!(io.gs.borrow()["org.cinnamon window-effect-speed"], "0");
        assert_eq!(io.gs.borrow()["org.cinnamon desktop-effects-minimize"], "'fade'");
        assert!(plan_change(&io, &c, &AnimChange::Speed(2.0)).is_empty());
        let io = fake();
        apply(&io, &ctx("xfce"), AnimChange::Enabled(false));
        assert_eq!(io.gs.borrow()["xsettings /Gtk/EnableAnimations"], "false");
    }

    #[test]
    fn sway_include_survives_space_in_home() {
        let c = LookCtx::new("sway", "/home/Иван Петров", None);
        let t = wm_targets(&c, "sway", "rexon-borders", &|_| String::new());
        assert_eq!(t[1].include_line, r#"include "\'/home/Иван Петров/.config/sway/rexon-borders.conf\'""#);
        let c = LookCtx::new("sway", "/h", None);
        assert_eq!(wm_targets(&c, "sway", "rexon-borders", &|_| String::new())[1].include_line, "include /h/.config/sway/rexon-borders.conf");
    }

    #[test]
    fn compositor_files() {
        let io = fake();
        let c = ctx("niri");
        io.files.borrow_mut().insert("/h/.config/niri/config.kdl".into(), "layout { gaps 16; }\n".into());
        apply(&io, &c, AnimChange::Speed(2.0));
        apply(&io, &c, AnimChange::Row { id: "desktopSwitch".into(), value: "false".into() });
        let own = io.files.borrow()["/h/.config/niri/rexon-animations.kdl"].clone();
        assert!(own.contains("slowdown 2") && own.contains("workspace-switch { off; }"), "{own}");
        assert!(io.files.borrow()["/h/.config/niri/config.kdl"].contains("include \"rexon-animations.kdl\""));
        assert!(plan_change(&io, &c, &AnimChange::Speed(2.0)).is_empty());
        let s = read_settings(&io, &c);
        assert_eq!(s.factor, 2.0);
        assert!(!s.on("desktopSwitch"));

        let io = fake();
        let c = ctx("hyprland");
        io.files.borrow_mut().insert("/h/.config/hypr/hyprland.lua".into(), "-- mine\n".into());
        apply(&io, &c, AnimChange::Enabled(true));
        let own = io.files.borrow()["/h/.config/hypr/rexon-animations.lua"].clone();
        assert!(own.contains("enabled = true") && !own.contains("hl.animation"), "{own}");
        apply(&io, &c, AnimChange::Row { id: "openClose".into(), value: "slide".into() });
        let own = io.files.borrow()["/h/.config/hypr/rexon-animations.lua"].clone();
        assert!(own.contains("leaf = \"windowsIn\", enabled = true, speed = 4.1, spring = \"rexonEasy\", style = \"slide\""), "{own}");
    }

    /// A command failure names the reason: the last stderr line, without it —
    /// stdout, without both — a timeout or a launch error.
    #[test]
    fn run_failure_text_names_reason() {
        use crate::utils::{RunOutcome, RunReport};
        let failed = |out: &str, err: &str| RunReport {
            outcome: RunOutcome::Failed,
            returncode: Some(1),
            stdout: out.as_bytes().to_vec(),
            stderr: err.as_bytes().to_vec(),
            ..Default::default()
        };
        let r = failed("", "No such schema \u{201c}org.gnome.shell.extensions.user-theme\u{201d}\n");
        assert_eq!(run_failure_text("gsettings", &r), "No such schema \u{201c}org.gnome.shell.extensions.user-theme\u{201d}");
        let r = failed("", "bash: line 1: warning\n\nbash: line 2: gsettings: command not found\n");
        assert_eq!(run_failure_text("bash", &r), "bash: line 2: gsettings: command not found");
        assert_eq!(run_failure_text("x", &failed("only stdout\n", "  \n")), "only stdout");
        assert_eq!(run_failure_text("x", &failed("", "")), "");
        let t = RunReport { outcome: RunOutcome::Timeout, timeout_secs: 30, ..Default::default() };
        assert!(run_failure_text("gsettings", &t).contains("30"));
        let e = RunReport { outcome: RunOutcome::Error, error: "No such file or directory".into(), ..Default::default() };
        assert_eq!(run_failure_text("gsettings", &e), "No such file or directory");
    }

    /// A machine with KWin effects: built-in, scripts (personal and system),
    /// a `.so` module and settings modules.
    fn kwin_machine() -> FakeIo {
        let io = fake();
        let mut f = io.files.borrow_mut();
        let b = "/usr/share/kwin/builtin-effects";
        f.insert(format!("{b}/blur.json"), r#"{"KPlugin":{"Category":"Appearance","Name":"Blur","Name[ru]":"Размытие","Description":"Blurs the background","EnabledByDefault":true},"X-KDE-ConfigModule":"kwin_blur_config","org.kde.kwin.effect":{"enabledByDefaultMethod":true}}"#.into());
        f.insert(format!("{b}/wobblywindows.json"), r#"{"KPlugin":{"Category":"Appearance","Name":"Wobbly Windows","EnabledByDefault":false},"X-KDE-ConfigModule":"kwin_wobblywindows_config"}"#.into());
        f.insert(format!("{b}/glide.json"), r#"{"KPlugin":{"Category":"Window Open/Close Animation","Name":"Glide","EnabledByDefault":false},"X-KDE-ConfigModule":"kwin_glide_config","org.kde.kwin.effect":{"exclusiveGroup":"toplevel-open-close-animation"}}"#.into());
        f.insert(format!("{b}/zoom.json"), r#"{"KPlugin":{"Category":"Accessibility","Name":"Zoom","EnabledByDefault":true},"org.kde.kwin.effect":{"exclusiveGroup":"magnifiers","internal":true}}"#.into());
        f.insert(format!("{b}/slidingpopups.json"), r#"{"KPlugin":{"Category":"Appearance","Name":"Sliding popups","EnabledByDefault":true}}"#.into());
        let u = "/h/.local/share/kwin/effects";
        f.insert(format!("{u}/myfx/metadata.json"), r#"{"KPackageStructure":"KWin/Effect","KPlugin":{"Id":"myfx","Category":"Tools","Name":"My FX","EnabledByDefault":true},"X-KDE-ConfigModule":"kcm_kwin4_genericscripted","X-KWin-Exclusive-Category":"mine"}"#.into());
        f.insert(format!("{u}/myfx/contents/config/main.xml"), "<kcfg/>".into());
        f.insert(format!("{u}/myfx/contents/ui/config.ui"), "<ui/>".into());
        f.insert(format!("{u}/otherfx/metadata.json"), r#"{"KPlugin":{"Id":"otherfx","Category":"Tools","Name":"Other FX","EnabledByDefault":false},"X-KWin-Exclusive-Category":"mine"}"#.into());
        let s = "/usr/share/kwin/effects";
        f.insert(format!("{s}/translucency/metadata.json"), r#"{"KPlugin":{"Id":"translucency","Category":"Appearance","Name":"Translucency"},"X-KDE-ConfigModule":"kcm_kwin4_genericscripted"}"#.into());
        f.insert(format!("{s}/translucency/contents/config/main.xml"), "<kcfg/>".into());
        f.insert(format!("{s}/login/metadata.json"), r#"{"KPlugin":{"Id":"login","Category":"Appearance","Name":"Login","EnabledByDefault":true},"X-KDE-ConfigModule":"kcm_kwin4_genericscripted"}"#.into());
        f.insert(format!("{s}/login/contents/config/main.xml"), "<kcfg/>".into());
        f.insert(format!("{s}/login/contents/ui/config.ui"), "<ui/>".into());
        drop(f);
        let q = "/usr/lib/qt6/plugins/kwin/effects";
        let mut bins = io.bins.borrow_mut();
        for m in ["kwin_blur_config", "kwin_glide_config", "kcm_kwin4_genericscripted"] {
            bins.insert(format!("{q}/configs/{m}.so"), vec![0]);
        }
        // A `.so` module with embedded metadata (CBOR), as in Qt 6.
        let mut so = b"\x7fELF junk ".to_vec();
        so.extend_from_slice(b"\xa3\x67KPlugin\xa3\x62Id\x68smodglow\x64Name\x69SMOD Glow\x68Category\x6aAppearance");
        so.extend_from_slice(b"\x73org.kde.kwin.effect\xa1\x6eexclusiveGroup\x64mine");
        so.extend_from_slice(b"\x72X-KDE-ConfigModule\x6ekwin_smod_conf");
        bins.insert(format!("{q}/plugins/libsmod.so"), so);
        drop(bins);
        io
    }

    #[test]
    fn desktop_effects_list_like_kde() {
        crate::i18n::set_language(Some("en"));
        let io = kwin_machine();
        let c = ctx("kde");
        let st = desktop_effects_state(&io, &c);
        assert!(st.available);
        let ids: Vec<&str> = st.effects.iter().map(|e| e.id.as_str()).collect();
        // No internal zoom, no glide and slidingpopups — they are in "Animation".
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(sorted, ["blur", "myfx", "otherfx", "smodglow", "translucency", "wobblywindows"], "{ids:?}");
        let get = |id: &str| st.effects.iter().find(|e| e.id == id).unwrap().clone();
        assert!(get("blur").enabled && get("blur").configurable);
        assert_eq!(get("blur").category, "Appearance");
        assert!(get("blur").search.contains("blurs the background"));
        // The settings module is not on the machine — no button.
        assert!(!get("wobblywindows").configurable && !get("wobblywindows").enabled);
        // A script without config.ui — KDE also has no "Configure…" for it.
        assert!(!get("translucency").configurable);
        assert!(get("myfx").configurable && get("myfx").group == "mine");
        // A `.so` module: name, group and settings module — from the embedded metadata;
        // the settings module itself is not on the machine.
        let glow = get("smodglow");
        assert_eq!((glow.name.as_str(), glow.group.as_str(), glow.configurable), ("SMOD Glow", "mine", false));
        // KDE order: category, group, name.
        assert_eq!(ids.first(), Some(&"blur"));
        assert_eq!(ids.last(), Some(&"otherfx"));
        assert_eq!(
            effect_config_argv(&io, &c, "myfx").unwrap(),
            argv(&["kcmshell6", "--args", "myfx KWin/Effect", "kwin/effects/configs/kcm_kwin4_genericscripted"])
        );
        assert_eq!(effect_config_argv(&io, &c, "blur").unwrap(), argv(&["kcmshell6", "kwin/effects/configs/kwin_blur_config"]));
        assert_eq!(configure_effect(&io, &c, "blur"), "");
        assert_eq!(io.spawned.borrow().len(), 1);
        assert!(!configure_effect(&io, &c, "wobblywindows").is_empty());
        assert_eq!(io.spawned.borrow().len(), 1);
    }

    #[test]
    fn effect_names_follow_language() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"KPlugin":{"Name":"Blur","Name[ru]":"Размытие","Name[pt_BR]":"Borrar","Description":"Blurs","Description[ru]":"Размывает"}}"#,
        )
        .unwrap();
        let m = kwin_meta(&json, "blur", false, "ru");
        assert_eq!((m.id.as_str(), m.name.as_str(), m.description.as_str()), ("blur", "Размытие", "Размывает"));
        assert_eq!(kwin_meta(&json, "blur", false, "pt-BR").name, "Borrar");
        assert_eq!(kwin_meta(&json, "blur", false, "en").description, "Blurs");
    }

    #[test]
    fn desktop_effects_toggle_group_and_defaults() {
        crate::i18n::set_language(Some("en"));
        let io = kwin_machine();
        let c = ctx("kde");
        let kwinrc = || io.files.borrow().get("/h/.config/kwinrc").cloned().unwrap_or_default();
        assert_eq!(set_desktop_effect(&io, &c, "wobblywindows", true), "Desktop effect settings applied.");
        assert!(kwinrc().contains("wobblywindowsEnabled=true"), "{}", kwinrc());
        // A repeat — nothing is written.
        assert!(plan_desktop_effect(&io, &c, "wobblywindows", true).is_empty());
        set_desktop_effect(&io, &c, "blur", false);
        assert!(kwinrc().contains("blurEnabled=false"));
        // Exclusive group: otherfx was enabled — myfx (and the .so module) are disabled.
        set_desktop_effect(&io, &c, "otherfx", true);
        let k = kwinrc();
        assert!(k.contains("otherfxEnabled=true") && k.contains("myfxEnabled=false"), "{k}");
        assert!(!k.contains("smodglowEnabled"), "{k}");
        let st = desktop_effects_state(&io, &c);
        assert!(!st.effects.iter().find(|e| e.id == "myfx").unwrap().enabled);
        // "Animation" effects are not touched from here.
        assert!(plan_desktop_effect(&io, &c, "glide", true).is_empty());
        // "Defaults" — the keys are deleted.
        desktop_effects_defaults(&io, &c);
        let k = kwinrc();
        assert!(!k.contains("Enabled="), "{k}");
        assert!(plan_desktop_effects_defaults(&io, &c).is_empty());
    }

    #[test]
    fn animation_rows_know_configurable_effects() {
        crate::i18n::set_language(Some("en"));
        let io = kwin_machine();
        let st = read_state(&io, &ctx("kde"));
        let row = |id: &str| st.rows.iter().find(|r| r.id == id).unwrap().clone();
        let open = row("openClose");
        let opt = |id: &str| open.options.iter().find(|o| o.id == id).unwrap().configurable;
        assert!(opt("glide") && !opt("fade") && !opt("none"));
        assert!(row("login").configurable && !row("logout").configurable);
        // Outside KDE — no buttons anywhere.
        let st = read_state(&io, &ctx("cinnamon-mint"));
        assert!(st.rows.iter().all(|r| !r.configurable && r.options.iter().all(|o| !o.configurable)));
    }

    #[test]
    fn desktop_effects_only_in_kde() {
        crate::i18n::set_language(Some("en"));
        let io = kwin_machine();
        for de in ["gnome", "cinnamon-mint", "hyprland", "mate", ""] {
            let c = ctx(de);
            let st = desktop_effects_state(&io, &c);
            assert!(!st.available && st.effects.is_empty(), "{de}");
            assert!(plan_desktop_effect(&io, &c, "blur", false).is_empty());
            assert!(plan_desktop_effects_defaults(&io, &c).is_empty());
            assert!(!configure_effect(&io, &c, "blur").is_empty());
        }
        assert!(io.spawned.borrow().is_empty());
    }

    #[test]
    fn unsupported_effects_hidden_when_kwin_answers() {
        struct Answering(FakeIo);
        impl LookIo for Answering {
            fn run(&self, a: &[String]) -> (bool, String) {
                if a[0] == "dbus-send" {
                    let ids = a.last().unwrap().trim_start_matches("array:string:").split(',').count();
                    let mut out = String::from("method return\n   array [\n");
                    for i in 0..ids {
                        out += if i == 0 { "      boolean false\n" } else { "      boolean true\n" };
                    }
                    return (true, out + "   ]\n");
                }
                self.0.run(a)
            }
            fn have(&self, t: &str) -> bool {
                self.0.have(t)
            }
            fn read(&self, p: &str) -> Option<String> {
                self.0.read(p)
            }
            fn read_bytes(&self, p: &str) -> Option<Vec<u8>> {
                self.0.read_bytes(p)
            }
            fn write(&self, p: &str, t: &str) -> bool {
                self.0.write(p, t)
            }
            fn list_dir(&self, p: &str) -> Vec<String> {
                self.0.list_dir(p)
            }
        }
        crate::i18n::set_language(Some("en"));
        let io = Answering(kwin_machine());
        let c = ctx("kde");
        let all = listed_effects(&io.0, &c);
        let shown = desktop_effects_state(&io, &c);
        assert_eq!(shown.effects.len(), all.len() - 1);
        assert!(!shown.effects.iter().any(|e| e.id == all[0].id));
    }
}
