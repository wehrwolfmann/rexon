//! A shot of real widgets for an «Appearance» tile: a Qt style (Breeze,
//! Fusion, Oxygen, Kvantum with its theme) and a GTK 3 theme.
//!
//! The KDE «Application Style» window does not draw the sample itself: every tile is a live
//! form of real widgets (tabs, button, check box, radio button,
//! list, field, spin box, slider, progress bar, scroll bar)
//! that is assigned the chosen style, with the palette of the current color scheme. Rexon
//! does the same, but as a shot in a SEPARATE process — the helper
//! [`HELPER_BINARY`]: a foreign style module (or GTK theme engine) is not loaded
//! into the Rexon process, a crash or hang costs one shot, the process
//! is killed on timeout, and the tile gets the previous drawn picture.
//!
//! * Qt — the `offscreen` platform, no screen is needed at all; widgets are captured with
//!   `QWidget::grab()`.
//! * GTK 3 — `GtkOffscreenWindow`: the window never appears on screen, but
//!   GTK 3 requires a display connection (it has no headless output),
//!   so a GTK shot is taken only in a session with `WAYLAND_DISPLAY` or
//!   `DISPLAY`.
//!
//! Everything that is decided lives here: what to shoot for a tile, the command line and
//! environment of the helper, the palette from the KDE scheme, the cache key, parsing of the
//! helper's own arguments. The helper (`rexon-gui`, `src/bin/theme_shot/`) only
//! executes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Helper name. In the developer tree it lies next to the window binary, in
/// the package — in `/usr/lib/rexon` ([`PACKAGE_HELPER`]).
pub const HELPER_BINARY: &str = "rexon-theme-shot";
/// Where the helper lies after the package is installed.
pub const PACKAGE_HELPER: &str = "/usr/lib/rexon/rexon-theme-shot";
/// How long to wait for a shot, seconds.
pub const SHOT_TIMEOUT_SECS: u64 = 15;
/// Size of the widget form in logical points. The tile
/// ([`crate::local_preview::PREVIEW_WIDTH`]×[`crate::local_preview::PREVIEW_HEIGHT`])
/// is too small for real widgets: the form is one and a half times larger. It is drawn
/// [`render_scale`] times larger and shrunk to a [`SHOT_WIDTH`]×
/// [`SHOT_HEIGHT`] frame keeping the aspect ratio.
pub const FORM_WIDTH: i64 = 450;
/// See [`FORM_WIDTH`].
pub const FORM_HEIGHT: i64 = 255;
/// Size of the finished frame — the size of the picture in the theme details
/// ([`crate::appearance_page::DETAIL_PREVIEW_W`]×[`crate::appearance_page::DETAIL_PREVIEW_H`],
/// exactly twice the tile). A tile-sized frame was stretched there
/// twofold and came out blurry; the tile simply shrinks a large frame.
pub const SHOT_WIDTH: i32 = crate::appearance_page::DETAIL_PREVIEW_W;
/// See [`SHOT_WIDTH`].
pub const SHOT_HEIGHT: i32 = crate::appearance_page::DETAIL_PREVIEW_H;
/// Mark of the shot rules in the cache key: when it changes, all shots are retaken.
pub const SHOT_STYLE: &str = "w2";

/// The cache-key part every widget shot shares: the shot rules and the form
/// labels. The labels are translated ([`sample_texts`]), so a shot taken in one
/// interface language is retaken after the language changes instead of showing
/// the old language's words.
pub fn style_tag() -> String {
    format!("{SHOT_STYLE}\u{1}{}", sample_texts().join("\u{1}"))
}

/// How many times larger to draw form `form` so that frame `size` is
/// only shrunk from it, never stretched: an integer from 1 to 4. A 450×255 form into
/// a 600×340 frame — twofold (900×510 → 600×340).
pub fn render_scale(form: (i32, i32), size: (i32, i32)) -> i32 {
    if form.0 <= 0 || form.1 <= 0 {
        return 1;
    }
    let k = (size.0 as f64 / form.0 as f64).max(size.1 as f64 / form.1 as f64);
    (k.ceil() as i32).clamp(1, 4)
}

/// Qt palette roles by KDE scheme groups — the same mapping as the KDE
/// application palette: window — group `Colors:Window`, fields and lists —
/// `Colors:View`, buttons — `Colors:Button`, selection — `Colors:Selection`,
/// tooltips — `Colors:Tooltip`. Role names are `QPalette::ColorRole` names.
pub const PALETTE_ROLES: &[(&str, &str, &str)] = &[
    ("Window", "Colors:Window", "BackgroundNormal"),
    ("WindowText", "Colors:Window", "ForegroundNormal"),
    ("Base", "Colors:View", "BackgroundNormal"),
    ("AlternateBase", "Colors:View", "BackgroundAlternate"),
    ("Text", "Colors:View", "ForegroundNormal"),
    ("Button", "Colors:Button", "BackgroundNormal"),
    ("ButtonText", "Colors:Button", "ForegroundNormal"),
    ("Highlight", "Colors:Selection", "BackgroundNormal"),
    ("HighlightedText", "Colors:Selection", "ForegroundNormal"),
    ("ToolTipBase", "Colors:Tooltip", "BackgroundNormal"),
    ("ToolTipText", "Colors:Tooltip", "ForegroundNormal"),
    ("Link", "Colors:View", "ForegroundLink"),
    ("LinkVisited", "Colors:View", "ForegroundVisited"),
    ("PlaceholderText", "Colors:View", "ForegroundInactive"),
];
/// Text roles that fade towards their background on disabled widgets.
const DISABLED_TEXT: &[(&str, &str)] =
    &[("WindowText", "Window"), ("Text", "Base"), ("ButtonText", "Button")];
/// Role prefix of the "disabled" group in the `--color` line.
pub const DISABLED_PREFIX: &str = "Disabled:";

/// Form labels in order: two tabs, the main and second button, check box,
/// radio button, list, field text. The helper takes them by index.
pub fn sample_texts() -> Vec<String> {
    use crate::i18n::tr;
    vec![
        tr("Preview"),
        tr("Settings"),
        tr("Apply"),
        tr("Cancel"),
        tr("Check box"),
        tr("Radio button"),
        tr("Combo box"),
        tr("Text"),
    ]
}

/// What to shoot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShotKind {
    /// A Qt style by its `QStyleFactory` key; for a Kvantum theme — also its name and
    /// directory (Kvantum takes the theme from its settings file, there is no
    /// environment variable for it — the helper is given its own settings directory).
    Qt { style: String, kvantum: Option<(String, String)> },
    /// A GTK 3 theme: name, directory, dark variant.
    Gtk { theme: String, dir: String, dark: bool },
}

/// A shot order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotRequest {
    pub kind: ShotKind,
    /// Qt palette: `(role, #rrggbb)`; roles of the "disabled" group carry
    /// [`DISABLED_PREFIX`]. Empty — the style's native palette.
    pub palette: Vec<(String, String)>,
}

/// KConfig color "r,g,b" → `#rrggbb`.
fn kconfig_hex(value: &str) -> Option<(u8, u8, u8)> {
    let parts: Vec<u8> = value.split(',').map(|p| p.trim().parse::<u8>().ok()).collect::<Option<_>>()?;
    match parts[..] {
        [r, g, b] | [r, g, b, _] => Some((r, g, b)),
        _ => None,
    }
}

fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Qt palette from the text of `kdeglobals` (or a `.colors` scheme file): the
/// [`PALETTE_ROLES`] roles and the faded text of disabled widgets (midway between
/// the text and its background). No scheme (outside KDE `kdeglobals` has no colors) —
/// empty: the style takes its own native palette, as a Qt program would see it without
/// the KDE platform theme.
pub fn palette_from_kdeglobals(text: &str) -> Vec<(String, String)> {
    let ini = crate::gtk_sanitizer::read_kde_ini(Some(text));
    let mut got: HashMap<&str, (u8, u8, u8)> = HashMap::new();
    for (role, group, key) in PALETTE_ROLES {
        if let Some(c) = kconfig_hex(&crate::gtk_sanitizer::kde_value(&ini, group, key)) {
            got.insert(role, c);
        }
    }
    // Without the window color and its text there is no scheme: we do not mix fragments
    // with the style's foreign native palette.
    if !got.contains_key("Window") || !got.contains_key("WindowText") {
        return Vec::new();
    }
    let mut out: Vec<(String, String)> = PALETTE_ROLES
        .iter()
        .filter_map(|(role, _, _)| got.get(role).map(|c| (role.to_string(), hex(*c))))
        .collect();
    for (text_role, back_role) in DISABLED_TEXT {
        if let (Some(t), Some(b)) = (got.get(text_role), got.get(back_role)) {
            let mid = |x: u8, y: u8| ((x as u16 + y as u16) / 2) as u8;
            out.push((format!("{DISABLED_PREFIX}{text_role}"), hex((mid(t.0, b.0), mid(t.1, b.1), mid(t.2, b.2)))));
        }
    }
    out
}

/// Whether a GTK theme directory has a GTK 3 style sheet (`gtk-3.0/gtk.css` or
/// `gtk-3.<N>/gtk.css`). Without it GTK 3 silently draws Adwaita — such a shot is
/// worse than the drawn sample.
pub fn has_gtk3(dir: &Path) -> bool {
    gtk3_css(dir, "gtk.css").is_some()
}

/// File `name` in the GTK 3 subdirectory of the theme.
fn gtk3_css(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join("gtk-3.0").join(name);
    if direct.is_file() {
        return Some(direct);
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("gtk-3."))
        .map(|e| e.path().join(name))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.pop()
}

/// Whether the person asks for the dark variant of GTK themes (GTK 3 `settings.ini`,
/// `gtk-application-prefer-dark-theme`).
pub fn prefers_dark(settings_ini: &str) -> bool {
    settings_ini.lines().any(|l| {
        let mut kv = l.splitn(2, '=');
        kv.next().map(str::trim) == Some("gtk-application-prefer-dark-theme")
            && matches!(kv.next().map(|v| v.trim().to_ascii_lowercase()).as_deref(), Some("1" | "true"))
    })
}

/// What to shoot for tile `local_id` (`local:<kind>:<name>`). `None` — the tile
/// is not a Qt style, not a Kvantum theme and not a GTK 3 theme, or there is nothing to shoot with.
pub fn shot_kind(local_id: &str, install_path: &str, gtk_display: bool, gtk_dark: bool) -> Option<ShotKind> {
    let rest = local_id.strip_prefix("local:")?;
    let (kind, name) = rest.split_once(':')?;
    if name.is_empty() {
        return None;
    }
    match kind {
        "widgetstyle" => Some(ShotKind::Qt { style: name.to_string(), kvantum: None }),
        "kvantum" if !install_path.is_empty() => {
            Some(ShotKind::Qt { style: "kvantum".into(), kvantum: Some((name.to_string(), install_path.to_string())) })
        }
        "gtk" if gtk_display && has_gtk3(Path::new(install_path)) => {
            let dir = Path::new(install_path);
            let theme = dir.file_name()?.to_string_lossy().into_owned();
            let dark = gtk_dark && gtk3_css(dir, "gtk-dark.css").is_some();
            Some(ShotKind::Gtk { theme, dir: install_path.to_string(), dark })
        }
        _ => None,
    }
}

/// File timestamp, seconds (no file — 0).
fn stamp(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// What is known about shots on this machine — collected once per batch
/// of tiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotSetup {
    /// The helper ([`find_helper`]); `None` — nothing to shoot with.
    pub helper: Option<String>,
    /// Palette of the current KDE scheme ([`palette_from_kdeglobals`]).
    pub palette: Vec<(String, String)>,
    /// Timestamp of `kdeglobals` — the palette in the cache key.
    pub colors_stamp: i64,
    /// Whether there is a display for GTK.
    pub gtk_display: bool,
    /// Dark variant of GTK themes.
    pub gtk_dark: bool,
}

/// The helper: next to the window binary (developer tree, the package's
/// `/usr/lib/rexon`), then [`PACKAGE_HELPER`], then `$PATH`.
pub fn find_helper(exe_dir: Option<&Path>, is_exec: &dyn Fn(&Path) -> bool, path_var: &str) -> Option<String> {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(d) = exe_dir {
        cands.push(d.join(HELPER_BINARY));
    }
    cands.push(PathBuf::from(PACKAGE_HELPER));
    cands.extend(path_var.split(':').filter(|d| d.starts_with('/')).map(|d| Path::new(d).join(HELPER_BINARY)));
    cands.into_iter().find(|p| is_exec(p)).map(|p| p.to_string_lossy().into_owned())
}

fn is_exec_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

impl ShotSetup {
    /// Live machine: the helper, `kdeglobals` and `gtk-3.0/settings.ini` from
    /// `$XDG_CONFIG_HOME` (otherwise `~/.config`), the session display.
    pub fn live(home: &str) -> Self {
        let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
        let helper = find_helper(exe_dir.as_deref(), &is_exec_file, &std::env::var("PATH").unwrap_or_default());
        let cfg = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|d| d.starts_with('/'))
            .unwrap_or_else(|| format!("{}/.config", home.trim_end_matches('/')));
        let kdeglobals = Path::new(&cfg).join("kdeglobals");
        let palette = std::fs::read_to_string(&kdeglobals).map(|t| palette_from_kdeglobals(&t)).unwrap_or_default();
        let set = |k: &str| std::env::var(k).is_ok_and(|v| !v.trim().is_empty());
        let gtk_display = set("WAYLAND_DISPLAY") || set("DISPLAY");
        let gtk_dark = std::fs::read_to_string(Path::new(&cfg).join("gtk-3.0/settings.ini"))
            .map(|t| prefers_dark(&t))
            .unwrap_or(false);
        ShotSetup { helper, palette, colors_stamp: stamp(&kdeglobals), gtk_display, gtk_dark }
    }

    /// What to shoot for a tile (`None` — wrong kind or nothing to shoot with).
    pub fn request(&self, local_id: &str, install_path: &str) -> Option<ShotRequest> {
        self.helper.as_ref()?;
        let kind = shot_kind(local_id, install_path, self.gtk_display, self.gtk_dark)?;
        let palette = match kind {
            ShotKind::Qt { .. } => self.palette.clone(),
            ShotKind::Gtk { .. } => Vec::new(),
        };
        Some(ShotRequest { kind, palette })
    }

    /// Addition to the cache key of the tile picture: the frame depends on whether there is
    /// something to shoot with, on the scheme palette (for Qt styles), on the Kvantum theme
    /// settings files and GTK style sheets (the theme directory timestamp changes only when
    /// files are added and removed) and on the GTK dark variant. Empty — the
    /// tile is not made with a shot.
    pub fn cache_tag(&self, local_id: &str, install_path: &str) -> String {
        let Some(kind) = shot_kind(local_id, install_path, self.gtk_display, self.gtk_dark) else {
            return String::new();
        };
        if self.helper.is_none() {
            return "noshot".into();
        }
        let size = format!("{FORM_WIDTH}x{FORM_HEIGHT}");
        match kind {
            ShotKind::Qt { kvantum: None, .. } => format!("{}\u{0}{size}\u{0}{}", style_tag(), self.colors_stamp),
            ShotKind::Qt { kvantum: Some((name, dir)), .. } => {
                let cfg = stamp(&Path::new(&dir).join(format!("{name}.kvconfig")));
                let svg = stamp(&Path::new(&dir).join(format!("{name}.svg")));
                format!("{}\u{0}{size}\u{0}{}\u{0}{cfg}\u{0}{svg}", style_tag(), self.colors_stamp)
            }
            ShotKind::Gtk { dir, dark, .. } => {
                let css = gtk3_css(Path::new(&dir), "gtk.css").map_or(0, |p| stamp(&p));
                format!("{}\u{0}{size}\u{0}{css}\u{0}{dark}", style_tag())
            }
        }
    }
}

// ── helper command line ──────────────────────────────────────────────────

/// Helper mode: Qt.
pub const MODE_QT: &str = "qt";
/// Helper mode: GTK.
pub const MODE_GTK: &str = "gtk";

/// Helper argv: the mode, then `--out`, `--size`, `--form`, for Qt `--style`
/// and `--color role=#color`, and the `--text` labels in [`sample_texts`] order.
pub fn helper_argv(helper: &str, req: &ShotRequest, texts: &[String], out: &str) -> Vec<String> {
    helper_argv_sized(helper, req, texts, out, (SHOT_WIDTH, SHOT_HEIGHT))
}

/// [`helper_argv`] for a finished frame of `size` (width, height) instead of
/// [`SHOT_WIDTH`]×[`SHOT_HEIGHT`].
pub fn helper_argv_sized(helper: &str, req: &ShotRequest, texts: &[String], out: &str, size: (i32, i32)) -> Vec<String> {
    let mut argv = vec![helper.to_string()];
    match &req.kind {
        ShotKind::Qt { style, .. } => {
            argv.extend([MODE_QT.to_string(), "--style".into(), style.clone()]);
        }
        ShotKind::Gtk { .. } => argv.push(MODE_GTK.into()),
    }
    argv.extend([
        "--out".into(),
        out.to_string(),
        "--size".into(),
        format!("{}x{}", size.0, size.1),
        "--form".into(),
        format!("{FORM_WIDTH}x{FORM_HEIGHT}"),
    ]);
    for (role, color) in &req.palette {
        argv.extend(["--color".into(), format!("{role}={color}")]);
    }
    for t in texts {
        argv.extend(["--text".into(), t.clone()]);
    }
    argv
}

/// Helper environment: what to set and what to remove. `work` is the shot's own temporary
/// directory (it holds the substitute Kvantum settings and the GTK themes
/// directory).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HelperEnv {
    pub set: Vec<(String, String)>,
    pub remove: Vec<String>,
    /// Symbolic links to create before the launch: (link, target).
    pub links: Vec<(PathBuf, PathBuf)>,
    /// Files to write before the launch: (path, text).
    pub files: Vec<(PathBuf, String)>,
}

/// Helper environment for order `req` with temporary directory `work`.
pub fn helper_env(req: &ShotRequest, work: &Path) -> HelperEnv {
    let mut env = HelperEnv::default();
    let set = |env: &mut HelperEnv, k: &str, v: &str| env.set.push((k.to_string(), v.to_string()));
    match &req.kind {
        ShotKind::Qt { kvantum, .. } => {
            // No screen and scale 1: otherwise the frame comes out twice as large.
            set(&mut env, "QT_QPA_PLATFORM", "offscreen");
            set(&mut env, "QT_SCALE_FACTOR", "1");
            // The platform theme (KDE, qt6ct) and the process style would replace
            // the palette and font of the form: the sample is the style with the scheme palette and
            // nothing else. The shot needs no input method, and the session IBus
            // can delay it until the timeout (see
            // `kdecoration::SHOT_ENV_REMOVE`).
            for k in [
                "QT_QPA_PLATFORMTHEME",
                "QT_STYLE_OVERRIDE",
                "QT_SCREEN_SCALE_FACTORS",
                "QT_AUTO_SCREEN_SCALE_FACTOR",
                "QT_IM_MODULE",
                "QT_IM_MODULES",
            ] {
                env.remove.push(k.to_string());
            }
            if let Some((name, dir)) = kvantum {
                // Kvantum takes the theme from `$XDG_CONFIG_HOME/Kvantum/
                // kvantum.kvconfig` (`[General] theme=`), and looks for the theme itself first
                // in `$XDG_CONFIG_HOME/Kvantum/<name>/`: our own settings
                // directory and a link to the theme directory give exactly this theme,
                // wherever it lies, and the person's settings are not touched.
                let cfg = work.join("config");
                env.files.push((cfg.join("Kvantum/kvantum.kvconfig"), format!("[General]\ntheme={name}\n")));
                env.links.push((cfg.join("Kvantum").join(name), PathBuf::from(dir)));
                set(&mut env, "XDG_CONFIG_HOME", &cfg.to_string_lossy());
            }
        }
        ShotKind::Gtk { theme, dir, dark } => {
            // GTK looks a theme up by name first in `$XDG_DATA_HOME/themes`: a link
            // there gives exactly this directory, even if a theme with the same name also lies
            // elsewhere.
            let data = work.join("data");
            env.links.push((data.join("themes").join(theme), PathBuf::from(dir)));
            set(&mut env, "XDG_DATA_HOME", &data.to_string_lossy());
            let value = if *dark { format!("{theme}:dark") } else { theme.clone() };
            set(&mut env, "GTK_THEME", &value);
            set(&mut env, "GDK_SCALE", "1");
            set(&mut env, "GDK_DPI_SCALE", "1");
            // No accessibility and no third-party modules: the shot does not need them, and
            // the application menu module pulls in the bus and slows down the launch.
            set(&mut env, "NO_AT_BRIDGE", "1");
            set(&mut env, "GTK_A11Y", "none");
            env.remove.push("GTK_MODULES".into());
        }
    }
    env
}

/// Shoot the form into `out` (PNG [`SHOT_WIDTH`]×[`SHOT_HEIGHT`]). `work_root` is the directory where
/// the shot's temporary directory is created (next to the picture cache). A process that
/// does not finish within [`SHOT_TIMEOUT_SECS`] is killed. `false` — no shot
/// (the style or theme did not load, the helper crashed, the frame has the wrong size);
/// an unfinished file is erased.
pub fn take_shot(helper: &str, work_root: &str, req: &ShotRequest, out: &str) -> bool {
    take_shot_sized(helper, work_root, req, out, (SHOT_WIDTH, SHOT_HEIGHT))
}

/// [`take_shot`] into a frame of `size` (width, height): the window contents of
/// a whole-theme picture ([`crate::local_preview::WHOLE_SHOT_SIZE`]).
pub fn take_shot_sized(helper: &str, work_root: &str, req: &ShotRequest, out: &str, size: (i32, i32)) -> bool {
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let work = Path::new(work_root).join(format!(
        "wshot-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&work);
    if std::fs::create_dir_all(&work).is_err() {
        return false;
    }
    let env = helper_env(req, &work);
    let prepared = env.files.iter().all(|(p, text)| {
        p.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(p, text).is_ok()
    }) && env.links.iter().all(|(link, target)| {
        link.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok())
            && std::os::unix::fs::symlink(target, link).is_ok()
    });
    let _ = std::fs::remove_file(out);
    let ok = prepared && {
        let argv = helper_argv_sized(helper, req, &sample_texts(), out, size);
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        for (k, v) in &env.set {
            cmd.env(k, v);
        }
        for k in &env.remove {
            cmd.env_remove(k);
        }
        match cmd.spawn() {
            Err(_) => false,
            Ok(mut child) => {
                let deadline = Instant::now() + Duration::from_secs(SHOT_TIMEOUT_SECS);
                loop {
                    match child.try_wait() {
                        Ok(Some(status)) => break status.success(),
                        Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
                        _ => {
                            let _ = child.kill();
                            let _ = child.wait();
                            break false;
                        }
                    }
                }
            }
        }
    };
    // Links are only links: remove_dir_all does not dereference them.
    let _ = std::fs::remove_dir_all(&work);
    let want = (size.0 as f64, size.1 as f64);
    let good = ok && crate::local_preview::image_size(Path::new(out)) == Some(want);
    if !good {
        let _ = std::fs::remove_file(out);
    }
    good
}

// ── parsing helper arguments ─────────────────────────────────────────────

/// The parsed helper command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperArgs {
    /// [`MODE_QT`] or [`MODE_GTK`].
    pub mode: String,
    /// Qt style key (empty for GTK).
    pub style: String,
    pub out: String,
    /// Size of the finished frame.
    pub size: (i32, i32),
    /// Form size in logical points; it is drawn [`render_scale`]
    /// times larger.
    pub form: (i32, i32),
    pub palette: Vec<(String, String)>,
    /// Labels in [`sample_texts`] order; missing ones are English.
    pub texts: Vec<String>,
}

fn parse_size(v: &str) -> Option<(i32, i32)> {
    let (w, h) = v.split_once('x')?;
    let (w, h) = (w.parse::<i32>().ok()?, h.parse::<i32>().ok()?);
    ((16..=4096).contains(&w) && (16..=4096).contains(&h)).then_some((w, h))
}

/// Parsing of helper arguments (without the program name). `Err` — text for
/// the log.
pub fn parse_helper_args(args: &[String]) -> Result<HelperArgs, String> {
    let mut it = args.iter();
    let mode = it.next().cloned().unwrap_or_default();
    if mode != MODE_QT && mode != MODE_GTK {
        return Err(format!("режим «{mode}» неизвестен: нужен {MODE_QT} или {MODE_GTK}"));
    }
    let mut out = HelperArgs {
        mode,
        style: String::new(),
        out: String::new(),
        size: (SHOT_WIDTH, SHOT_HEIGHT),
        form: (FORM_WIDTH as i32, FORM_HEIGHT as i32),
        palette: Vec::new(),
        texts: Vec::new(),
    };
    while let Some(key) = it.next() {
        let value = it.next().ok_or_else(|| format!("у «{key}» нет значения"))?;
        match key.as_str() {
            "--style" => out.style = value.clone(),
            "--out" => out.out = value.clone(),
            "--size" => out.size = parse_size(value).ok_or_else(|| format!("размер «{value}»"))?,
            "--form" => out.form = parse_size(value).ok_or_else(|| format!("размер формы «{value}»"))?,
            "--color" => {
                let (role, color) = value.split_once('=').ok_or_else(|| format!("цвет «{value}»"))?;
                out.palette.push((role.to_string(), color.to_string()));
            }
            "--text" => out.texts.push(value.clone()),
            _ => return Err(format!("ключ «{key}» неизвестен")),
        }
    }
    if out.out.is_empty() {
        return Err("не сказано, куда сохранить (--out)".into());
    }
    if out.mode == MODE_QT && out.style.is_empty() {
        return Err("не назван стиль (--style)".into());
    }
    const ENGLISH: [&str; 8] = ["Preview", "Settings", "Apply", "Cancel", "Check box", "Radio button", "Combo box", "Text"];
    for t in ENGLISH.iter().skip(out.texts.len()) {
        out.texts.push(t.to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-wshot-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const BREEZE_DARK: &str = "[Colors:Window]\nBackgroundNormal=32,35,38\nForegroundNormal=252,252,252\n\
        [Colors:View]\nBackgroundNormal=20,22,24\nBackgroundAlternate=29,31,34\nForegroundNormal=252,252,252\n\
        ForegroundLink=29,153,243\n[Colors:Button]\nBackgroundNormal=41,44,48\nForegroundNormal=252,252,252\n\
        [Colors:Selection]\nBackgroundNormal=61,174,233\nForegroundNormal=255,255,255\n";

    /// Palette from the KDE scheme: roles by group, disabled text is midway
    /// between text and background; no scheme — the style's native palette (empty).
    #[test]
    fn palette_follows_kde_scheme() {
        let p = palette_from_kdeglobals(BREEZE_DARK);
        let get = |r: &str| p.iter().find(|(k, _)| k == r).map(|(_, v)| v.as_str());
        assert_eq!(get("Window"), Some("#202326"));
        assert_eq!(get("Base"), Some("#141618"));
        assert_eq!(get("Highlight"), Some("#3daee9"));
        assert_eq!(get("Link"), Some("#1d99f3"));
        assert_eq!(get("ToolTipBase"), None, "в схеме нет — роль не задаётся");
        // (252+32)/2 = 142 = 0x8e, (252+35)/2 = 143, (252+38)/2 = 145.
        assert_eq!(get("Disabled:WindowText"), Some("#8e8f91"));
        assert!(palette_from_kdeglobals("[General]\nwidgetStyle=Fusion\n").is_empty());
        assert!(palette_from_kdeglobals("[Colors:Window]\nBackgroundNormal=1,2,3\n").is_empty(), "без текста окна схемы нет");
        assert!(palette_from_kdeglobals("[Colors:Window]\nBackgroundNormal=1,2,300\nForegroundNormal=0,0,0\n").is_empty());
    }

    /// What is shot: a Qt style, a Kvantum theme, a GTK 3 theme when there is a display; a
    /// GTK 2-only theme and other kinds — not.
    #[test]
    fn kinds_that_are_shot() {
        let root = tmp("kinds");
        let gtk3 = root.join("Nord");
        std::fs::create_dir_all(gtk3.join("gtk-3.0")).unwrap();
        std::fs::write(gtk3.join("gtk-3.0/gtk.css"), "").unwrap();
        let gtk2 = root.join("Old");
        std::fs::create_dir_all(gtk2.join("gtk-2.0")).unwrap();
        let new_dir = root.join("Neo");
        std::fs::create_dir_all(new_dir.join("gtk-3.20")).unwrap();
        std::fs::write(new_dir.join("gtk-3.20/gtk.css"), "").unwrap();
        std::fs::write(new_dir.join("gtk-3.20/gtk-dark.css"), "").unwrap();
        let s = |p: &Path| p.to_string_lossy().into_owned();

        assert_eq!(
            shot_kind("local:widgetstyle:Breeze", "", false, false),
            Some(ShotKind::Qt { style: "Breeze".into(), kvantum: None })
        );
        assert_eq!(
            shot_kind("local:kvantum:KvArc", "/usr/share/Kvantum/KvArc", false, false),
            Some(ShotKind::Qt { style: "kvantum".into(), kvantum: Some(("KvArc".into(), "/usr/share/Kvantum/KvArc".into())) })
        );
        assert_eq!(
            shot_kind("local:gtk:Nord", &s(&gtk3), true, true),
            Some(ShotKind::Gtk { theme: "Nord".into(), dir: s(&gtk3), dark: false }),
            "тёмного файла нет — тёмный вариант не просится"
        );
        assert_eq!(
            shot_kind("local:gtk:Neo", &s(&new_dir), true, true),
            Some(ShotKind::Gtk { theme: "Neo".into(), dir: s(&new_dir), dark: true })
        );
        assert_eq!(shot_kind("local:gtk:Nord", &s(&gtk3), false, false), None, "без дисплея GTK не снимается");
        assert_eq!(shot_kind("local:gtk:Old", &s(&gtk2), true, false), None);
        assert_eq!(shot_kind("local:colors:BreezeDark", "/x", true, false), None);
        assert_eq!(shot_kind("local:widgetstyle:", "", true, false), None);
        assert_eq!(shot_kind("store:123", "", true, false), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dark_preference_from_settings_ini() {
        assert!(prefers_dark("[Settings]\ngtk-application-prefer-dark-theme=1\n"));
        assert!(prefers_dark("[Settings]\ngtk-application-prefer-dark-theme = true\n"));
        assert!(!prefers_dark("[Settings]\ngtk-application-prefer-dark-theme=0\n"));
        assert!(!prefers_dark("[Settings]\ngtk-theme-name=Adwaita\n"));
    }

    /// The helper: next to the window binary, then the package path, then PATH.
    #[test]
    fn helper_lookup_order() {
        let all = |_: &Path| true;
        assert_eq!(
            find_helper(Some(Path::new("/tree/rust/target/release")), &all, "/usr/bin").as_deref(),
            Some("/tree/rust/target/release/rexon-theme-shot")
        );
        let pkg = |p: &Path| p == Path::new(PACKAGE_HELPER);
        assert_eq!(find_helper(Some(Path::new("/x")), &pkg, "/usr/bin").as_deref(), Some(PACKAGE_HELPER));
        let path = |p: &Path| p == Path::new("/opt/r/bin/rexon-theme-shot");
        assert_eq!(find_helper(None, &path, "relative:/opt/r/bin").as_deref(), Some("/opt/r/bin/rexon-theme-shot"));
        assert_eq!(find_helper(None, &|_: &Path| false, "/usr/bin"), None);
    }

    /// The command line and its parsing by the helper agree: what the core wrote is
    /// what the helper reads.
    #[test]
    fn argv_round_trip() {
        let req = ShotRequest {
            kind: ShotKind::Qt { style: "Fusion".into(), kvantum: None },
            palette: vec![("Window".into(), "#202326".into()), ("Disabled:Text".into(), "#808080".into())],
        };
        let texts: Vec<String> = ["A", "B"].iter().map(|s| s.to_string()).collect();
        let argv = helper_argv("/h/rexon-theme-shot", &req, &texts, "/c/x.png");
        assert_eq!(argv[0], "/h/rexon-theme-shot");
        let got = parse_helper_args(&argv[1..]).unwrap();
        assert_eq!(got.mode, MODE_QT);
        assert_eq!(got.style, "Fusion");
        assert_eq!(got.out, "/c/x.png");
        assert_eq!(got.size, (SHOT_WIDTH, SHOT_HEIGHT));
        assert_eq!(got.size, (600, 340), "кадр — размер картинки в подробностях");
        assert_eq!(got.form, (FORM_WIDTH as i32, FORM_HEIGHT as i32));
        assert_eq!(got.palette, req.palette);
        assert_eq!(got.texts.len(), 8, "недостающие подписи — английские");
        assert_eq!(&got.texts[..3], ["A", "B", "Apply"]);

        let gtk = ShotRequest { kind: ShotKind::Gtk { theme: "T".into(), dir: "/t".into(), dark: false }, palette: vec![] };
        let got = parse_helper_args(&helper_argv("h", &gtk, &[], "/o.png")[1..]).unwrap();
        assert_eq!(got.mode, MODE_GTK);
        assert!(got.style.is_empty());

        assert!(parse_helper_args(&["svg".to_string()]).is_err());
        assert!(parse_helper_args(&["qt".into(), "--out".into(), "/o".into()]).is_err(), "без стиля");
        assert!(parse_helper_args(&["gtk".into(), "--out".into()]).is_err(), "ключ без значения");
        assert!(parse_helper_args(&["gtk".into(), "--out".into(), "/o".into(), "--size".into(), "9x9".into()]).is_err());
    }

    /// The form is drawn larger exactly enough for the frame to be only
    /// shrunk from it.
    #[test]
    fn render_scale_never_upscales() {
        assert_eq!(render_scale((FORM_WIDTH as i32, FORM_HEIGHT as i32), (SHOT_WIDTH, SHOT_HEIGHT)), 2);
        assert_eq!(render_scale((450, 255), (300, 170)), 1);
        assert_eq!(render_scale((100, 100), (1000, 1000)), 4, "не больше четырёх");
        assert_eq!(render_scale((0, 10), (300, 170)), 1);
    }

    /// Environment: Qt — no screen and no platform theme, for Kvantum — its own
    /// settings directory with the chosen theme and a link to its directory; GTK — the theme
    /// by name through our own themes directory.
    #[test]
    fn helper_environment() {
        let work = Path::new("/c/wshot-1");
        let qt = helper_env(&ShotRequest { kind: ShotKind::Qt { style: "Breeze".into(), kvantum: None }, palette: vec![] }, work);
        assert!(qt.set.contains(&("QT_QPA_PLATFORM".into(), "offscreen".into())));
        assert!(qt.remove.contains(&"QT_QPA_PLATFORMTHEME".to_string()));
        assert!(qt.links.is_empty() && qt.files.is_empty());

        let kv = helper_env(
            &ShotRequest {
                kind: ShotKind::Qt { style: "kvantum".into(), kvantum: Some(("KvArc".into(), "/home/u/.config/Kvantum/KvArc".into())) },
                palette: vec![],
            },
            work,
        );
        assert!(kv.set.contains(&("XDG_CONFIG_HOME".into(), "/c/wshot-1/config".into())));
        assert_eq!(kv.files, vec![(PathBuf::from("/c/wshot-1/config/Kvantum/kvantum.kvconfig"), "[General]\ntheme=KvArc\n".into())]);
        assert_eq!(
            kv.links,
            vec![(PathBuf::from("/c/wshot-1/config/Kvantum/KvArc"), PathBuf::from("/home/u/.config/Kvantum/KvArc"))]
        );

        let gtk = helper_env(
            &ShotRequest { kind: ShotKind::Gtk { theme: "Nord".into(), dir: "/usr/share/themes/Nord".into(), dark: true }, palette: vec![] },
            work,
        );
        assert!(gtk.set.contains(&("GTK_THEME".into(), "Nord:dark".into())));
        assert!(gtk.set.contains(&("XDG_DATA_HOME".into(), "/c/wshot-1/data".into())));
        assert_eq!(gtk.links, vec![(PathBuf::from("/c/wshot-1/data/themes/Nord"), PathBuf::from("/usr/share/themes/Nord"))]);
        assert!(!gtk.set.iter().any(|(k, _)| k == "QT_QPA_PLATFORM"));
    }

    /// Cache key: empty for tiles not made by a shot; without the helper — its own key (so that
    /// the frame is retaken after the helper is installed); changes together with the palette.
    #[test]
    fn cache_tag_tracks_inputs() {
        let mut setup = ShotSetup {
            helper: Some("/h".into()),
            palette: vec![],
            colors_stamp: 5,
            gtk_display: true,
            gtk_dark: false,
        };
        assert_eq!(setup.cache_tag("local:colors:X", "/x"), "");
        let a = setup.cache_tag("local:widgetstyle:Fusion", "");
        // The translated form labels are part of the key: a new interface
        // language retakes the shot.
        assert!(a.contains(&sample_texts().join("\u{1}")));
        setup.colors_stamp = 6;
        assert_ne!(a, setup.cache_tag("local:widgetstyle:Fusion", ""));
        assert!(setup.request("local:widgetstyle:Fusion", "").is_some());
        setup.helper = None;
        assert_eq!(setup.cache_tag("local:widgetstyle:Fusion", ""), "noshot");
        assert!(setup.request("local:widgetstyle:Fusion", "").is_none());
    }

    /// No helper — no shot, no file left behind, the temporary directory is removed.
    #[test]
    fn missing_helper_leaves_nothing() {
        let root = tmp("take");
        let out = root.join("x.png");
        std::fs::write(&out, b"old").unwrap();
        let req = ShotRequest { kind: ShotKind::Qt { style: "Fusion".into(), kvantum: None }, palette: vec![] };
        assert!(!take_shot("/нет/такого/помощника", &root.to_string_lossy(), &req, &out.to_string_lossy()));
        assert!(!out.exists());
        let left: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().collect();
        assert!(left.is_empty(), "временный каталог убран: {left:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
