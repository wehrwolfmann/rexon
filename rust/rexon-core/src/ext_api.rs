//! The public extension API: the pure part of the port of `rexon/extensions/api.py`.
//!
//! `ExtensionAPI` in Python is a thin wrapper over the main window (Qt) and
//! subprocesses (`gsettings`/`xfconf-query`/`notify-send`/`fc-cache`,
//! `shutil.copytree`), so it cannot be moved into the Qt-free core as a whole.
//! The PURE, deterministic part has been ported: building the command that
//! applies a GTK or icon theme for the current DE (`apply_gtk_theme`/
//! `apply_icon_theme`), i.e. mapping the DE to its settings backend (a
//! gsettings schema or an xfconf property). Running the command (`safe_run`)
//! and returning a `bool` from the exit code remain in the GUI layer; here we
//! only decide "which command to run".
//!
//! Verified byte for byte against Python: the command arguments were captured
//! by monkeypatching `safe_run` on a real `ExtensionAPI` (golden test
//! `ext_api_parity.rs`).

/// What is being applied: a GTK theme or an icon theme. The only differences
/// are the gsettings key (`gtk-theme`/`icon-theme`) and the xfconf property name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    /// GTK theme (`apply_gtk_theme`).
    Gtk,
    /// Icon theme (`apply_icon_theme`).
    Icon,
}

impl ThemeKind {
    /// The gsettings key for gnome/cinnamon/mate.
    fn gsettings_key(self) -> &'static str {
        match self {
            ThemeKind::Gtk => "gtk-theme",
            ThemeKind::Icon => "icon-theme",
        }
    }

    /// The name of the xfconf property in the `xsettings` channel for XFCE.
    fn xfconf_property(self) -> &'static str {
        match self {
            ThemeKind::Gtk => "/Net/ThemeName",
            ThemeKind::Icon => "/Net/IconThemeName",
        }
    }
}

/// The interface gsettings schema for a DE (`schema_map` in Python). `None`
/// means the DE does not use the gsettings path.
fn gsettings_schema(de: &str) -> Option<&'static str> {
    match de {
        // Pantheon is themed with the same GNOME keys.
        "gnome" | "pantheon" => Some("org.gnome.desktop.interface"),
        "cinnamon" => Some("org.cinnamon.desktop.interface"),
        "mate" => Some("org.mate.interface"),
        _ => None,
    }
}

/// The command that applies a theme (`kind`) named `name` for the current DE `de`.
///
/// A port of the branching in `apply_gtk_theme`/`apply_icon_theme`:
/// gnome/cinnamon/mate → `gsettings set <schema> <key> <name>`; xfce →
/// `xfconf-query -c xsettings -p <prop> -s <name>`. For other DEs
/// (kde/budgie/…) there is no command → `None` (in Python, `return False`: the
/// theme is not applied through this path).
pub fn theme_apply_command(de: &str, kind: ThemeKind, name: &str) -> Option<Vec<String>> {
    if let Some(schema) = gsettings_schema(de) {
        return Some(vec![
            "gsettings".to_string(),
            "set".to_string(),
            schema.to_string(),
            kind.gsettings_key().to_string(),
            name.to_string(),
        ]);
    }
    if de == "xfce" {
        return Some(vec![
            "xfconf-query".to_string(),
            "-c".to_string(),
            "xsettings".to_string(),
            "-p".to_string(),
            kind.xfconf_property().to_string(),
            "-s".to_string(),
            name.to_string(),
        ]);
    }
    None
}

/// The full target path for copying an appearance directory: the pure part of
/// `copy_theme_files`/`copy_icon_files`, i.e. `os.path.join(base_dir,
/// os.path.basename(src_dir.rstrip("/")))`.
///
/// The destination directory (`~/.themes`, `/usr/share/themes`, the icon
/// directory) is chosen by the shim: it is expanded via `utils.expand_user`,
/// so it depends on `XDG_DATA_HOME`, and computing it here would lose that
/// adjustment.
///
/// `None` means the name is empty (`src_dir` like `"/"`, `"///"` or an empty
/// string). In that case the original got `os.path.join(base, "")`, i.e. the
/// destination directory ITSELF, and went on to delete it
/// (`_safe_rmtree(~/.themes)`): wiping all of the user's themes because of a
/// garbage argument. The port rejects such input (the "foolproofing" rule);
/// on `None` the shim returns `False`.
pub fn copy_target(base_dir: &str, src_dir: &str) -> Option<String> {
    let trimmed = src_dir.trim_end_matches('/');
    let name = trimmed.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        return None;
    }
    Some(if base_dir.is_empty() || base_dir.ends_with('/') {
        format!("{base_dir}{name}")
    } else {
        format!("{base_dir}/{name}")
    })
}
