//! "Edit…" and "Create based on…" for a color scheme, as in the KDE "Colors"
//! module.
//!
//! What KDE does (checked against plasma-workspace 6.8.2, `kcms/colors`,
//! `kcm_colors.so` and `kcolorschemeeditor --help`, 31.10.2026): the tile's
//! "Edit" button calls `KCMColors::editScheme`, which starts the separate
//! program `kcolorschemeeditor <scheme>` (service
//! `org.kde.kcolorschemeeditor`). For the person's own scheme (it lives in
//! `~/.local/share/color-schemes` and can be deleted) `--overwrite` is added,
//! and the editor shows "Apply", which saves over it; a system scheme only has
//! "Save As…", i.e. editing a system scheme is exactly "create a new one based
//! on it". Hence here:
//!
//! * "Edit…" — `kcolorschemeeditor <scheme>` (+ `--overwrite` for one's own);
//! * "Create based on…" — `kcolorschemeeditor <scheme>` without `--overwrite`;
//! * no program (not KDE, plasma-workspace not installed) — Rexon's own simple
//!   editor: window background and text, view areas, buttons, selection,
//!   tooltips and the window title; the result is a scheme in
//!   `~/.local/share/color-schemes`. Outside Plasma, schemes are applied by
//!   the Rexon layer ([`crate::shell_palette`]), which reads the same keys, so
//!   the editor works there too.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::{n_, tr, trf};
use crate::shell_palette::{ini_apply, ini_get, IniEdit, Rgb};

/// The KDE color scheme editor program.
pub const KDE_EDITOR: &str = "kcolorschemeeditor";

/// What the button will do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    /// "Edit…": one's own scheme is edited in place.
    Edit,
    /// "Create based on…": always a new scheme.
    NewFrom,
}

impl EditMode {
    pub fn from_key(key: &str) -> EditMode {
        if key == "new" {
            EditMode::NewFrom
        } else {
            EditMode::Edit
        }
    }
}

/// Whether the scheme is the person's own: it lies in the person's scheme
/// directory (and can be deleted, like `RemovableRole` in the KDE module).
pub fn is_user_scheme(path: &str, home: &str) -> bool {
    let dir = format!("{}/.local/share/color-schemes/", home.trim_end_matches('/'));
    path.starts_with(&dir)
}

/// The KDE editor command for scheme `name` (the file name without
/// `.colors`), as in `KCMColors::editScheme`. `None` means the program is
/// missing (`has_editor`); then Rexon's own editor is used.
pub fn kde_editor_argv(name: &str, user_scheme: bool, mode: EditMode, has_editor: bool) -> Option<Vec<String>> {
    if !has_editor || name.trim().is_empty() {
        return None;
    }
    let mut argv = vec![KDE_EDITOR.to_string(), name.to_string()];
    if mode == EditMode::Edit && user_scheme {
        argv.push("--overwrite".to_string());
    }
    Some(argv)
}

/// The scheme of a local tile: (file name without `.colors`, path to the
/// file). A scheme tile has the id `local:colors:<name>`, and `install_path`
/// is the path without the extension (`installed_scan::mk_item`).
pub fn scheme_of_item(item_id: &str, install_path: &str) -> Option<(String, String)> {
    let name = item_id.strip_prefix("local:colors:")?;
    if name.is_empty() || install_path.trim().is_empty() {
        return None;
    }
    let file = if install_path.ends_with(".colors") {
        install_path.to_string()
    } else {
        format!("{install_path}.colors")
    };
    Some((name.to_string(), file))
}

/// What the button opens: the KDE editor or our own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditOpen {
    /// `kde` means the KDE editor has been started (`argv`); `own` means the Rexon window.
    pub mode: String,
    pub argv: Vec<String>,
    /// Our own window: the suggested name, the roles, the base file and the name
    /// of the file that may be overwritten (`overwrite`; empty means new scheme only).
    pub name: String,
    pub roles: Vec<SchemeRole>,
    pub base: String,
    pub overwrite: String,
}

/// Decides what to open for a scheme tile. `display` is the scheme's
/// human-readable name, `base_text` is the contents of its file (needed only
/// by our own window).
pub fn edit_open(
    item_id: &str,
    install_path: &str,
    display: &str,
    home: &str,
    mode: EditMode,
    has_kde_editor: bool,
    base_text: &str,
) -> Option<EditOpen> {
    let (name, file) = scheme_of_item(item_id, install_path)?;
    let user = is_user_scheme(&file, home);
    if let Some(argv) = kde_editor_argv(&name, user, mode, has_kde_editor) {
        return Some(EditOpen {
            mode: "kde".into(),
            argv,
            name: String::new(),
            roles: Vec::new(),
            base: file,
            overwrite: String::new(),
        });
    }
    let shown = if display.trim().is_empty() { name.as_str() } else { display.trim() };
    Some(EditOpen {
        mode: "own".into(),
        argv: Vec::new(),
        name: suggested_name(shown, mode, user),
        roles: read_roles(base_text),
        base: file,
        overwrite: if mode == EditMode::Edit && user { name } else { String::new() },
    })
}

// ============================================================================
// Our own editor: roles
// ============================================================================

/// A scheme role edited by our own editor.
struct RoleDef {
    key: &'static str,
    label: &'static str,
    group: &'static str,
    bg_key: &'static str,
    fg_key: &'static str,
}

/// The main roles. The window title uses the active window frame colors
/// (`[WM]`); the others use the "normal" background and text of their
/// `Colors:*` group.
const ROLES: &[RoleDef] = &[
    RoleDef { key: "window", label: n_("Window"), group: "Colors:Window", bg_key: "BackgroundNormal", fg_key: "ForegroundNormal" },
    RoleDef { key: "view", label: n_("View (lists and text fields)"), group: "Colors:View", bg_key: "BackgroundNormal", fg_key: "ForegroundNormal" },
    RoleDef { key: "button", label: n_("Button"), group: "Colors:Button", bg_key: "BackgroundNormal", fg_key: "ForegroundNormal" },
    RoleDef { key: "selection", label: n_("Selection"), group: "Colors:Selection", bg_key: "BackgroundNormal", fg_key: "ForegroundNormal" },
    RoleDef { key: "tooltip", label: n_("Tooltip"), group: "Colors:Tooltip", bg_key: "BackgroundNormal", fg_key: "ForegroundNormal" },
    RoleDef { key: "titlebar", label: n_("Window title bar"), group: "WM", bg_key: "activeBackground", fg_key: "activeForeground" },
];

/// A role with its colors for the editor window (`#rrggbb`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemeRole {
    pub key: String,
    #[serde(default)]
    pub label: String,
    pub background: String,
    pub foreground: String,
}

/// The role colors of scheme `text`. When a key is missing, the window role
/// takes "Window", and so do the others (the way KColorScheme substitutes an
/// omitted key), with light Breeze as a last resort.
pub fn read_roles(text: &str) -> Vec<SchemeRole> {
    let get = |group: &str, key: &str| ini_get(text, group, key).and_then(|v| Rgb::parse(&v));
    let win_bg = get("Colors:Window", "BackgroundNormal").unwrap_or(Rgb(239, 240, 241));
    let win_fg = get("Colors:Window", "ForegroundNormal").unwrap_or(Rgb(35, 38, 41));
    ROLES
        .iter()
        .map(|r| SchemeRole {
            key: r.key.to_string(),
            label: tr(r.label),
            background: get(r.group, r.bg_key).unwrap_or(win_bg).hex(),
            foreground: get(r.group, r.fg_key).unwrap_or(win_fg).hex(),
        })
        .collect()
}

/// The file name of a new scheme from its human-readable name, as in "Save
/// As…" of the KDE editor: letters and digits are kept (spaces and punctuation
/// are dropped). Empty gives `None`.
pub fn file_stem_for(name: &str) -> Option<String> {
    let stem: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
    (!stem.is_empty()).then_some(stem)
}

/// The suggested name of a new scheme based on `base`.
pub fn suggested_name(base: &str, mode: EditMode, user_scheme: bool) -> String {
    if mode == EditMode::Edit && user_scheme {
        base.to_string()
    } else {
        trf("%1 (copy)", &[&base])
    }
}

/// The text of a new scheme: the base scheme `base` with colors `roles` and
/// name `name` (`[General] Name`; translations of the base name are removed,
/// otherwise a Russian menu would still show the old name). An empty `base`
/// means a scheme from scratch.
pub fn build_scheme(base: &str, name: &str, stem: &str, roles: &[SchemeRole]) -> String {
    // Translations of the base name (`Name[ru]=…`) go away.
    let cleaned: String = base
        .split_inclusive('\n')
        .filter(|l| !l.trim_start().starts_with("Name["))
        .collect();
    let mut edits = vec![
        IniEdit { group: "General".into(), key: "Name".into(), value: Some(name.to_string()) },
        IniEdit { group: "General".into(), key: "ColorScheme".into(), value: Some(stem.to_string()) },
    ];
    for role in roles {
        let Some(def) = ROLES.iter().find(|r| r.key == role.key) else { continue };
        for (key, color) in [(def.bg_key, &role.background), (def.fg_key, &role.foreground)] {
            if let Some(c) = Rgb::parse(color) {
                edits.push(IniEdit { group: def.group.into(), key: key.into(), value: Some(c.kde()) });
            }
        }
    }
    ini_apply(Some(&cleaned), &edits)
}

/// The result of saving.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveResult {
    pub ok: bool,
    pub message: String,
    /// The file of the new scheme (on success).
    pub path: String,
}

/// Saves our own editor's scheme into `~/.local/share/color-schemes`.
/// `base_path` is the base scheme file; `overwrite` is the file name of the
/// person's own scheme that may be overwritten ("Edit…" on one's own scheme,
/// same name).
pub fn save_scheme(
    home: &str,
    base_path: &str,
    name: &str,
    roles: &[SchemeRole],
    overwrite: Option<&str>,
    system_dirs: &[String],
) -> SaveResult {
    let fail = |message: String| SaveResult { ok: false, message, path: String::new() };
    let name = name.trim();
    let Some(stem) = file_stem_for(name) else {
        return fail(tr("Enter a name for the color scheme."));
    };
    let dir = PathBuf::from(home).join(".local/share/color-schemes");
    let target = dir.join(format!("{stem}.colors"));
    let taken_by_user = target.exists() && overwrite != Some(stem.as_str());
    // A system scheme with the same file would be shadowed by our own — that is not allowed either.
    let taken_by_system = system_dirs
        .iter()
        .any(|d| Path::new(d).join(format!("{stem}.colors")).exists());
    if taken_by_user || taken_by_system {
        return fail(trf("A color scheme named “%1” already exists — choose another name.", &[&stem]));
    }
    let base = std::fs::read_to_string(base_path).unwrap_or_default();
    let text = build_scheme(&base, name, &stem, roles);
    let wrote = std::fs::create_dir_all(&dir).and_then(|_| {
        let tmp = dir.join(format!(".{stem}.colors.rexon-tmp"));
        std::fs::write(&tmp, text.as_bytes())?;
        std::fs::rename(&tmp, &target)
    });
    match wrote {
        Ok(()) => SaveResult {
            ok: true,
            message: trf("Color scheme “%1” saved.", &[&name]),
            path: target.to_string_lossy().into_owned(),
        },
        Err(e) => fail(trf("Couldn't save the color scheme: %1", &[&e])),
    }
}

/// System scheme directories (`$XDG_DATA_DIRS/color-schemes`).
pub fn system_scheme_dirs(xdg_data_dirs: Option<&str>) -> Vec<String> {
    let dirs = xdg_data_dirs.filter(|s| !s.trim().is_empty()).unwrap_or("/usr/local/share:/usr/share");
    dirs.split(':')
        .filter(|d| !d.is_empty())
        .map(|d| format!("{}/color-schemes", d.trim_end_matches('/')))
        .collect()
}

// ============================================================================
// Labels
// ============================================================================

/// Labels of the buttons and the window of our own editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditorTexts {
    pub edit: String,
    pub edit_tooltip: String,
    pub new_from: String,
    pub new_from_tooltip: String,
    pub title: String,
    pub name_label: String,
    pub background: String,
    pub foreground: String,
    pub save: String,
    pub cancel: String,
    pub hint: String,
}

pub fn editor_texts(has_kde_editor: bool) -> EditorTexts {
    EditorTexts {
        edit: tr("Edit…"),
        edit_tooltip: if has_kde_editor {
            tr("Open the scheme in the KDE color scheme editor")
        } else {
            tr("Change the main colors of the scheme")
        },
        new_from: tr("New Based on This…"),
        new_from_tooltip: tr("Make a new color scheme from this one; the original stays as it is"),
        title: tr("Color Scheme Editor"),
        name_label: tr("Name:"),
        background: tr("Background"),
        foreground: tr("Text"),
        save: tr("Save"),
        cancel: tr("Cancel"),
        hint: tr("The scheme is saved to your color schemes and appears in the list; apply it as usual."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_drops_spaces_and_signs() {
        assert_eq!(file_stem_for("My Dark 2!").as_deref(), Some("MyDark2"));
        assert_eq!(file_stem_for("  ").as_deref(), None);
    }
}
