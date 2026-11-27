//! The GTK theme in GTK 4 and libadwaita programs.
//!
//! libadwaita does not read the theme name (`gtk-theme-name`, GNOME key
//! `gtk-theme`): it has its own look and takes a foreign theme only from the
//! user stylesheet `~/.config/gtk-4.0/gtk.css`. That is what everyone who can
//! do this does:
//!
//! * theme installers (Orchis, Colloid, Fluent — `install.sh -l`,
//!   `link_libadwaita` in `core.sh`): remove `gtk-4.0/{assets,gtk.css,
//!   gtk-dark.css}` and put symlinks into the theme's `gtk-4.0` in their place;
//! * nwg-look (`linkGtk4Stuff`, the "export-gtk4-symlinks" toggle, on by
//!   default) — the same three links and `gtk-4.0/settings.ini`; for a theme
//!   without `gtk-4.0` it touches nothing;
//! * KDE (kde-gtk-config, `SettingsIniEditor`) writes the GTK 3 and GTK 4
//!   `settings.ini` (`gtk-theme-name` and the rest), and creates its own
//!   `gtk.css` only for the Breeze colors (`@import 'colors.css'`); it does not
//!   set links to themes;
//! * Gradience writes its own `gtk.css`, puts the previous one next to it as
//!   `gtk.css.bak` and restores it on "Restore"; gnome-tweaks does not touch the
//!   GTK 4 theme at all ("Legacy Applications").
//!
//! Rexon does what the theme installers do, but more carefully: it links ALL
//! stylesheets and directories from the theme's `gtk-4.0` (adw-gtk3's `gtk.css`
//! imports the neighboring `libadwaita.css`, Materia has an `icons` directory:
//! GTK resolves relative paths next to the link, not next to the theme file),
//! does not erase foreign files in those places but moves them to
//! `<name>.rexon-backup` and restores them when Adwaita, a theme without
//! `gtk-4.0` is chosen or the place becomes free. Links into the `gtk-4.0` of
//! any theme (`…/themes/<theme>/gtk-4.0/…` — ours, Orchis, nwg-look) are
//! considered replaceable and are removed without a backup.
//!
//! The decision is a pure function [`steps`] over [`Facts`] gathered in advance
//! ([`facts_live`]); the steps are file steps of the shared executor, under the
//! click's rollback and in the "revert to how it was" journal.

use crate::ext_manager::ExtStep;
use crate::i18n::{tr, trf};
use crate::install_plan::Step;

/// `settings.json` key: whether to apply the GTK theme to GTK 4 and libadwaita
/// programs. No key — yes (as in nwg-look).
pub const SETTING_KEY: &str = "gtk4_apply";

/// Suffix of the name of a backup of a foreign file: `gtk.css.rexon-backup`,
/// the next one `gtk.css.rexon-backup-2`.
pub const BACKUP_SUFFIX: &str = ".rexon-backup";

/// What is in `~/.config/gtk-4.0` under one name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A link into some theme's `gtk-4.0` — replaceable, no backup needed.
    ThemeLink,
    /// A link to nowhere (not into a theme) — nothing to preserve.
    Dangling,
    /// The person's own file (or a link to a file outside themes).
    File,
    /// The person's own directory (or a link to a directory outside themes).
    Dir,
}

/// One entry of the `~/.config/gtk-4.0` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
}

/// The GTK 4 version of a theme.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ThemeGtk4 {
    /// The theme has no `gtk-4.0` directory (or `gtk-4.0/gtk.css`).
    #[default]
    Missing,
    /// Adwaita — the built-in look of GTK 4 and libadwaita: put nothing in place.
    Builtin,
    /// The styles are packed into `gtk.gresource` and imported as
    /// `resource:///…` — GTK registers the resource only when loading the theme by
    /// name; from `~/.config/gtk-4.0` such a stylesheet is empty (WhiteSur).
    Packed,
    /// There is something to put in place: the theme's `gtk-4.0` directory and the
    /// names in it — `*.css` stylesheets and directories (`assets`, `icons`);
    /// `true` — a directory.
    Files { dir: String, names: Vec<(String, bool)> },
}

impl ThemeGtk4 {
    /// Whether there is anything to give GTK 4 programs.
    pub fn linkable(&self) -> bool {
        matches!(self, ThemeGtk4::Files { .. })
    }
}

/// What was gathered from the machine for [`steps`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// `~/.config/gtk-4.0` (honoring `$XDG_CONFIG_HOME`).
    pub config_dir: String,
    /// What is in it now (without `settings.ini`).
    pub entries: Vec<Entry>,
    /// The GTK 4 version of the chosen theme.
    pub theme: ThemeGtk4,
    /// The "Apply to GTK 4/libadwaita programs" toggle.
    pub enabled: bool,
}

/// Name of the backup of `name` with number `n` (1 — without a number).
fn backup_name(name: &str, n: usize) -> String {
    if n <= 1 { format!("{name}{BACKUP_SUFFIX}") } else { format!("{name}{BACKUP_SUFFIX}-{n}") }
}

/// Backup number, if `entry` is a backup of `name`.
fn backup_number(entry: &str, name: &str) -> Option<usize> {
    let rest = entry.strip_prefix(name)?.strip_prefix(BACKUP_SUFFIX)?;
    if rest.is_empty() {
        return Some(1);
    }
    rest.strip_prefix('-')?.parse().ok().filter(|n| *n >= 2)
}

/// Whether this is a backup at all (the plan does not treat such names as places).
fn is_backup(entry: &str) -> bool {
    entry.contains(BACKUP_SUFFIX)
}

/// Whether a link leads into some theme's `gtk-4.0`: `…/themes/<theme>/gtk-4.0/…`
/// or `…/.themes/<theme>/gtk-4.0/…`.
pub fn is_theme_link_target(target: &str) -> bool {
    let Some(at) = target.find("/gtk-4.0/") else { return false };
    let theme_dir = std::path::Path::new(&target[..at]);
    theme_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "themes" || n == ".themes")
}

/// Steps for the chosen theme. Empty — nothing to touch (the toggle is off, the
/// place is already in order).
///
/// Order: remove all links into themes; for every name the theme carries, move
/// the foreign item into a backup and put a link; a place that became free and
/// is not taken by the theme is restored from the latest backup.
pub fn steps(facts: &Facts) -> Vec<Step> {
    if !facts.enabled {
        return Vec::new();
    }
    let dir = facts.config_dir.trim_end_matches('/');
    let at = |name: &str| format!("{dir}/{name}");
    let wanted: &[(String, bool)] = match &facts.theme {
        ThemeGtk4::Files { names, .. } => names,
        _ => &[],
    };
    let mut out: Vec<Step> = Vec::new();
    // Links of the previous theme are removed (the new theme puts its own).
    for e in facts.entries.iter().filter(|e| !is_backup(&e.name)) {
        if matches!(e.kind, EntryKind::ThemeLink) {
            out.push(Step::File(ExtStep::RemoveFile { path: at(&e.name) }));
        }
    }
    let entry = |name: &str| facts.entries.iter().find(|e| e.name == name);
    if let ThemeGtk4::Files { dir: theme_dir, names } = &facts.theme {
        if !names.is_empty() {
            out.insert(0, Step::File(ExtStep::MakeDirs { path: dir.to_string() }));
        }
        for (name, _) in names {
            match entry(name).map(|e| e.kind) {
                Some(EntryKind::File) | Some(EntryKind::Dir) => {
                    let is_dir = entry(name).is_some_and(|e| e.kind == EntryKind::Dir);
                    let free = (1..)
                        .map(|n| backup_name(name, n))
                        .find(|b| entry(b).is_none())
                        .unwrap_or_default();
                    let (src, dest) = (at(name), at(&free));
                    if is_dir {
                        out.push(Step::File(ExtStep::CopyTree { src: src.clone(), dest }));
                        out.push(Step::File(ExtStep::RemoveTree { path: src, safe: false, ignore_errors: false }));
                    } else {
                        out.push(Step::File(ExtStep::CopyFile { src: src.clone(), dest }));
                        out.push(Step::File(ExtStep::RemoveFile { path: src }));
                    }
                }
                Some(EntryKind::Dangling) => out.push(Step::File(ExtStep::RemoveFile { path: at(name) })),
                Some(EntryKind::ThemeLink) | None => {}
            }
            out.push(Step::File(ExtStep::Symlink {
                target: format!("{}/{name}", theme_dir.trim_end_matches('/')),
                link: at(name),
            }));
        }
    }
    // A freed (or previously empty) place that the theme did not take is restored
    // from the latest backup.
    let mut restored: Vec<&str> = Vec::new();
    for e in &facts.entries {
        let Some(base) = e.name.find(BACKUP_SUFFIX).map(|i| &e.name[..i]) else { continue };
        if restored.contains(&base) || wanted.iter().any(|(n, _)| n == base) {
            continue;
        }
        if entry(base).is_some_and(|b| b.kind != EntryKind::ThemeLink) {
            continue;
        }
        let Some((latest, _)) = facts
            .entries
            .iter()
            .filter_map(|b| backup_number(&b.name, base).map(|n| (b, n)))
            .max_by_key(|(_, n)| *n)
        else {
            continue;
        };
        restored.push(base);
        let (src, dest) = (at(&latest.name), at(base));
        if latest.kind == EntryKind::Dir {
            out.push(Step::File(ExtStep::CopyTree { src: src.clone(), dest }));
            out.push(Step::File(ExtStep::RemoveTree { path: src, safe: false, ignore_errors: false }));
        } else {
            out.push(Step::File(ExtStep::CopyFile { src: src.clone(), dest }));
            out.push(Step::File(ExtStep::RemoveFile { path: src }));
        }
    }
    out
}

/// Rexon sets the light/dark mode of libadwaita programs from the GTK theme
/// where the theme is written to the GNOME scheme
/// ([`crate::themes::COLOR_SCHEME_KEY`]).
pub fn mode_follows(de: &str) -> bool {
    crate::themes::resolve_schemas(de)
        .role("gtk")
        .is_some_and(|(s, _)| s == crate::themes::COLOR_SCHEME_KEY.0)
}

/// Append to the steps of GTK theme `base` what GTK 4 needs: `gtk-theme-name`
/// in `gtk-4.0/settings.ini` (as KDE does; if the environment's plan does not
/// write it yet) and the [`steps`] links. For Deepin the plan is a single stop,
/// nothing is added to it.
pub fn extend_press(base: &mut Vec<Step>, theme: &str, de: &str, facts: &Facts) {
    use crate::themes::ApplyStep;
    if de == "deepin" {
        return;
    }
    let ini = format!("{}/settings.ini", facts.config_dir.trim_end_matches('/'));
    let has_ini = base.iter().any(|s| {
        matches!(s, Step::Apply(ApplyStep::GtkIni { path, key, .. }) if *path == ini && key == "gtk-theme-name")
    });
    if !has_ini {
        base.push(Step::Apply(ApplyStep::GtkIni {
            path: ini,
            key: "gtk-theme-name".into(),
            value: theme.to_string(),
        }));
    }
    base.extend(steps(facts));
}

/// Whether this is a built-in GTK 4 theme (Adwaita and its dark variant).
pub fn is_builtin(theme: &str) -> bool {
    theme.trim().to_lowercase().starts_with("adwaita")
}

/// The GTK 4 version of a theme from the theme directory `theme_dir` (`None` —
/// the theme directory was not found). `list` — the names in `gtk-4.0` with a
/// directory flag, `read` — a file's text.
pub fn theme_gtk4(
    theme: &str,
    theme_dir: Option<&str>,
    list: &dyn Fn(&str) -> Option<Vec<(String, bool)>>,
    read: &dyn Fn(&str) -> Option<String>,
) -> ThemeGtk4 {
    if is_builtin(theme) {
        return ThemeGtk4::Builtin;
    }
    let Some(theme_dir) = theme_dir else { return ThemeGtk4::Missing };
    let dir = format!("{}/gtk-4.0", theme_dir.trim_end_matches('/'));
    let Some(all) = list(&dir) else { return ThemeGtk4::Missing };
    if !all.iter().any(|(n, d)| n == "gtk.css" && !d) {
        return ThemeGtk4::Missing;
    }
    let packed = all.iter().any(|(n, _)| n == "gtk.gresource")
        && read(&format!("{dir}/gtk.css")).is_some_and(|css| css.contains("resource:///"));
    if packed {
        return ThemeGtk4::Packed;
    }
    let mut names: Vec<(String, bool)> = all
        .into_iter()
        .filter(|(n, d)| !n.starts_with('.') && (*d || n.ends_with(".css")))
        .collect();
    names.sort();
    ThemeGtk4::Files { dir, names }
}

// ============================================================================
// Labels
// ============================================================================

/// The toggle under the GTK theme tile: `checked` — what will happen on
/// "Apply", `enabled` — whether there is anything to carry over, `note` — an
/// explanation (for a disabled toggle as a visible line, for an enabled one as a
/// tooltip).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Toggle {
    pub visible: bool,
    pub enabled: bool,
    pub checked: bool,
    pub text: String,
    pub note: String,
}

/// The toggle for theme `theme` with version `gtk4`; `on` — the value of the
/// [`SETTING_KEY`] setting; `mode_follows` — Rexon sets the light/dark mode of
/// libadwaita programs from the theme (environments with the GNOME scheme).
pub fn toggle(theme: &str, gtk4: &ThemeGtk4, on: bool, mode_follows: bool) -> Toggle {
    let mode = if mode_follows {
        tr(" Only light or dark mode follows the theme.")
    } else {
        String::new()
    };
    let note = match gtk4 {
        ThemeGtk4::Files { .. } => tr(
            "The theme has its own GTK 4 version: Rexon links its styles into \
             ~/.config/gtk-4.0 (gtk.css, gtk-dark.css, assets), as the theme's own \
             installer does. Your own files there are kept as *.rexon-backup and come \
             back when you choose Adwaita.",
        ),
        ThemeGtk4::Builtin => tr(
            "Adwaita is the built-in look of GTK 4 and libadwaita apps: links to other \
             themes in ~/.config/gtk-4.0 are removed and your own files come back.",
        ),
        ThemeGtk4::Packed => {
            trf(
                "The GTK 4 version of “%1” is packed into gtk.gresource and works only \
                 when GTK loads the theme by name, so GTK 4 and libadwaita apps keep \
                 their own look.",
                &[&theme],
            ) + &mode
        }
        ThemeGtk4::Missing => {
            trf(
                "“%1” has no GTK 4 version, so GTK 4 and libadwaita apps keep their \
                 own look.",
                &[&theme],
            ) + &mode
        }
    };
    let enabled = gtk4.linkable();
    Toggle {
        visible: true,
        enabled,
        checked: enabled && on,
        text: tr("Apply to GTK 4/libadwaita apps"),
        note,
    }
}

// ============================================================================
// The live machine
// ============================================================================

/// Directory of GTK theme `theme` — the first one found in the theme search paths.
pub fn theme_dir_live(theme: &str, home: &str) -> Option<String> {
    let data = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), home);
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    crate::themes::gtk_theme_search_roots(home, &data, &dirs)
        .into_iter()
        .map(|root| format!("{root}/{theme}"))
        .find(|d| std::path::Path::new(d).is_dir())
}

fn list_live(dir: &str) -> Option<Vec<(String, bool)>> {
    let rd = std::fs::read_dir(dir).ok()?;
    Some(
        rd.flatten()
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                Some((name, e.path().is_dir()))
            })
            .collect(),
    )
}

/// The GTK 4 version of a theme on this machine.
pub fn theme_gtk4_live(theme: &str, home: &str) -> ThemeGtk4 {
    let dir = theme_dir_live(theme, home);
    theme_gtk4(theme, dir.as_deref(), &list_live, &|p| std::fs::read_to_string(p).ok())
}

/// What is in directory `dir` (`~/.config/gtk-4.0`), apart from `settings.ini`.
pub fn entries_live(dir: &str) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            if name == "settings.ini" {
                return None;
            }
            let path = e.path();
            let link = std::fs::symlink_metadata(&path).ok()?.file_type().is_symlink();
            let kind = if link {
                let target = std::fs::read_link(&path).ok()?.to_string_lossy().into_owned();
                if is_theme_link_target(&target) {
                    EntryKind::ThemeLink
                } else if !path.exists() {
                    EntryKind::Dangling
                } else if path.is_dir() {
                    EntryKind::Dir
                } else {
                    EntryKind::File
                }
            } else if path.is_dir() {
                EntryKind::Dir
            } else {
                EntryKind::File
            };
            Some(Entry { name, kind })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Toggle value from `settings.json` (no key — yes).
pub fn enabled_live() -> bool {
    crate::settings::load_settings(&crate::settings::settings_file())
        .get(SETTING_KEY)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

/// Write the toggle to `settings.json`.
pub fn set_enabled_live(on: bool) -> std::io::Result<()> {
    crate::settings::SettingsManager::new()?.set(SETTING_KEY, serde_json::Value::from(on))
}

/// Gather [`Facts`] for theme `theme`.
pub fn facts_live(theme: &str, home: &str, xdg_config_home: Option<&str>) -> Facts {
    let config_dir = format!("{}/gtk-4.0", crate::utils::config_home(xdg_config_home, home));
    Facts {
        entries: entries_live(&config_dir),
        theme: theme_gtk4_live(theme, home),
        enabled: enabled_live(),
        config_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = "/h/.config/gtk-4.0";

    fn e(name: &str, kind: EntryKind) -> Entry {
        Entry { name: name.into(), kind }
    }

    fn files(names: &[(&str, bool)]) -> ThemeGtk4 {
        ThemeGtk4::Files {
            dir: "/usr/share/themes/Orchis/gtk-4.0".into(),
            names: names.iter().map(|(n, d)| (n.to_string(), *d)).collect(),
        }
    }

    fn facts(entries: Vec<Entry>, theme: ThemeGtk4) -> Facts {
        Facts { config_dir: DIR.into(), entries, theme, enabled: true }
    }

    fn file(s: ExtStep) -> Step {
        Step::File(s)
    }

    fn link(name: &str) -> Step {
        file(ExtStep::Symlink {
            target: format!("/usr/share/themes/Orchis/gtk-4.0/{name}"),
            link: format!("{DIR}/{name}"),
        })
    }

    fn rm(name: &str) -> Step {
        file(ExtStep::RemoveFile { path: format!("{DIR}/{name}") })
    }

    #[test]
    fn links_every_style_and_directory_of_the_theme() {
        let got = steps(&facts(vec![], files(&[("assets", true), ("gtk-dark.css", false), ("gtk.css", false)])));
        assert_eq!(
            got,
            vec![
                file(ExtStep::MakeDirs { path: DIR.into() }),
                link("assets"),
                link("gtk-dark.css"),
                link("gtk.css"),
            ]
        );
    }

    #[test]
    fn own_file_goes_to_backup_and_theme_link_is_replaced_without_one() {
        let got = steps(&facts(
            vec![e("gtk.css", EntryKind::File), e("assets", EntryKind::ThemeLink), e("colors.css", EntryKind::File)],
            files(&[("assets", true), ("gtk.css", false)]),
        ));
        assert_eq!(
            got,
            vec![
                file(ExtStep::MakeDirs { path: DIR.into() }),
                rm("assets"),
                link("assets"),
                file(ExtStep::CopyFile { src: format!("{DIR}/gtk.css"), dest: format!("{DIR}/gtk.css.rexon-backup") }),
                rm("gtk.css"),
                link("gtk.css"),
            ],
            "colors.css темой не занят — его не трогаем"
        );
    }

    #[test]
    fn second_backup_gets_a_number_and_own_directory_is_copied_as_tree() {
        let got = steps(&facts(
            vec![
                e("assets", EntryKind::Dir),
                e("assets.rexon-backup", EntryKind::Dir),
                e("gtk.css", EntryKind::ThemeLink),
            ],
            files(&[("assets", true), ("gtk.css", false)]),
        ));
        assert_eq!(
            got,
            vec![
                file(ExtStep::MakeDirs { path: DIR.into() }),
                rm("gtk.css"),
                file(ExtStep::CopyTree { src: format!("{DIR}/assets"), dest: format!("{DIR}/assets.rexon-backup-2") }),
                file(ExtStep::RemoveTree { path: format!("{DIR}/assets"), safe: false, ignore_errors: false }),
                link("assets"),
                link("gtk.css"),
            ]
        );
    }

    #[test]
    fn adwaita_removes_theme_links_and_brings_back_latest_backup() {
        let got = steps(&facts(
            vec![
                e("assets", EntryKind::ThemeLink),
                e("gtk.css", EntryKind::ThemeLink),
                e("gtk.css.rexon-backup", EntryKind::File),
                e("gtk.css.rexon-backup-2", EntryKind::File),
            ],
            ThemeGtk4::Builtin,
        ));
        assert_eq!(
            got,
            vec![
                rm("assets"),
                rm("gtk.css"),
                file(ExtStep::CopyFile {
                    src: format!("{DIR}/gtk.css.rexon-backup-2"),
                    dest: format!("{DIR}/gtk.css"),
                }),
                rm("gtk.css.rexon-backup-2"),
            ]
        );
    }

    #[test]
    fn freed_place_the_new_theme_does_not_take_gets_its_backup_back() {
        // The previous theme occupied gtk-dark.css, the new one (Materia-dark) does not
        // carry it — the person's own file comes back.
        let got = steps(&facts(
            vec![
                e("gtk-dark.css", EntryKind::ThemeLink),
                e("gtk-dark.css.rexon-backup", EntryKind::File),
                e("gtk.css", EntryKind::ThemeLink),
            ],
            files(&[("gtk.css", false)]),
        ));
        assert_eq!(
            got,
            vec![
                file(ExtStep::MakeDirs { path: DIR.into() }),
                rm("gtk-dark.css"),
                rm("gtk.css"),
                link("gtk.css"),
                file(ExtStep::CopyFile {
                    src: format!("{DIR}/gtk-dark.css.rexon-backup"),
                    dest: format!("{DIR}/gtk-dark.css"),
                }),
                rm("gtk-dark.css.rexon-backup"),
            ]
        );
    }

    #[test]
    fn own_files_without_backup_stay_and_switch_off_touches_nothing() {
        let own = vec![e("gtk.css", EntryKind::File), e("assets", EntryKind::ThemeLink)];
        // A theme without gtk-4.0: remove the previous theme's link, keep the person's
        // own gtk.css.
        assert_eq!(steps(&facts(own.clone(), ThemeGtk4::Missing)), vec![rm("assets")]);
        let mut off = facts(own, files(&[("gtk.css", false)]));
        off.enabled = false;
        assert!(steps(&off).is_empty());
    }

    #[test]
    fn theme_link_targets() {
        assert!(is_theme_link_target("/usr/share/themes/Orchis/gtk-4.0/gtk.css"));
        assert!(is_theme_link_target("/home/u/.themes/Fluent-Dark/gtk-4.0/assets"));
        assert!(is_theme_link_target("/home/u/.local/share/themes/Colloid/gtk-4.0/gtk-dark.css"));
        assert!(!is_theme_link_target("/home/u/css/gtk-4.0/gtk.css"));
        assert!(!is_theme_link_target("/home/u/my.css"));
    }

    #[test]
    fn theme_version_kinds() {
        let list = |entries: Vec<(&'static str, bool)>| {
            move |_: &str| Some(entries.iter().map(|(n, d)| (n.to_string(), *d)).collect::<Vec<_>>())
        };
        let none = |_: &str| None::<Vec<(String, bool)>>;
        let read = |_: &str| Some("@import url(\"resource:///org/gnome/theme/gtk.css\");".to_string());
        let plain = |_: &str| Some("window {}".to_string());
        assert_eq!(theme_gtk4("Adwaita-dark", Some("/t"), &none, &plain), ThemeGtk4::Builtin);
        assert_eq!(theme_gtk4("Arc", Some("/t/Arc"), &none, &plain), ThemeGtk4::Missing);
        assert_eq!(theme_gtk4("Arc", None, &none, &plain), ThemeGtk4::Missing);
        assert_eq!(
            theme_gtk4("WhiteSur", Some("/t/W"), &list(vec![("gtk.css", false), ("gtk.gresource", false)]), &read),
            ThemeGtk4::Packed
        );
        assert_eq!(
            theme_gtk4(
                "adw-gtk3",
                Some("/t/adw-gtk3"),
                &list(vec![
                    ("gtk.css", false),
                    ("libadwaita.css", false),
                    ("assets", true),
                    ("thumbnail.png", false),
                    ("settings.ini", false),
                ]),
                &plain,
            ),
            ThemeGtk4::Files {
                dir: "/t/adw-gtk3/gtk-4.0".into(),
                names: vec![("assets".into(), true), ("gtk.css".into(), false), ("libadwaita.css".into(), false)],
            }
        );
    }

    #[test]
    fn toggle_is_honest_about_themes_without_gtk4() {
        let t = toggle("Arc", &ThemeGtk4::Missing, true, true);
        assert!(!t.enabled && !t.checked && t.visible);
        assert!(t.note.contains("Arc") && t.note.contains("light or dark"));
        assert!(!toggle("Arc", &ThemeGtk4::Missing, true, false).note.contains("light or dark"));
        let t = toggle("Orchis", &files(&[("gtk.css", false)]), true, false);
        assert!(t.enabled && t.checked);
        assert!(!toggle("Orchis", &files(&[("gtk.css", false)]), false, false).checked);
    }

    #[test]
    fn press_gets_gtk4_settings_ini_once_and_deepin_nothing() {
        use crate::themes::ApplyStep;
        let f = facts(vec![], files(&[("gtk.css", false)]));
        let ini = |v: &str| {
            Step::Apply(ApplyStep::GtkIni {
                path: format!("{DIR}/settings.ini"),
                key: "gtk-theme-name".into(),
                value: v.into(),
            })
        };
        let mut base = Vec::new();
        extend_press(&mut base, "Orchis", "gnome", &f);
        assert_eq!(base, vec![ini("Orchis"), file(ExtStep::MakeDirs { path: DIR.into() }), link("gtk.css")]);
        let mut base = vec![ini("Orchis")];
        extend_press(&mut base, "Orchis", "kde", &f);
        assert_eq!(base.iter().filter(|s| **s == ini("Orchis")).count(), 1);
        let mut base = Vec::new();
        extend_press(&mut base, "Orchis", "deepin", &f);
        assert!(base.is_empty());
    }

    /// A run through the shared executor on a real directory: the person's own
    /// gtk.css goes into a backup and comes back when Adwaita is chosen.
    #[test]
    fn real_run_keeps_own_file_and_brings_it_back() {
        use crate::install_plan::{exec, no_sync_env, Ctx};
        use std::fs;
        let root = std::env::temp_dir().join(format!("rexon-gtk4-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let theme = root.join("share/themes/Orchis");
        fs::create_dir_all(theme.join("gtk-4.0/assets")).unwrap();
        fs::write(theme.join("gtk-4.0/gtk.css"), "orchis").unwrap();
        fs::write(theme.join("gtk-4.0/gtk-dark.css"), "orchis-dark").unwrap();
        let cfg = root.join("config/gtk-4.0");
        fs::create_dir_all(&cfg).unwrap();
        fs::write(cfg.join("gtk.css"), "mine").unwrap();
        fs::write(cfg.join("settings.ini"), "[Settings]\n").unwrap();
        let cfg_s = cfg.to_string_lossy().into_owned();
        let read = |p: &str| fs::read_to_string(p).ok();
        let run_with = |gtk4: ThemeGtk4| {
            let f = Facts { config_dir: cfg_s.clone(), entries: entries_live(&cfg_s), theme: gtk4, enabled: true };
            let no_run = |_: &[String], _: Option<u64>| crate::utils::RunReport::default();
            let ctx = Ctx {
                install: true,
                steamos: false,
                privileged: None,
                run: &no_run,
                have: &|_| false,
                spawn: &|_| {},
                wayland: false,
                sync_env: &no_sync_env,
            };
            exec(&steps(&f), &mut Vec::new(), &ctx).expect("план исполнился");
        };
        run_with(theme_gtk4("Orchis", Some(&theme.to_string_lossy()), &list_live, &read));
        assert_eq!(fs::read_to_string(cfg.join("gtk.css")).unwrap(), "orchis");
        assert!(fs::symlink_metadata(cfg.join("gtk.css")).unwrap().file_type().is_symlink());
        assert!(cfg.join("assets").is_dir());
        assert_eq!(fs::read_to_string(cfg.join("gtk.css.rexon-backup")).unwrap(), "mine");
        assert!(entries_live(&cfg_s).iter().any(|e| e.name == "gtk.css" && e.kind == EntryKind::ThemeLink));
        // Re-applying the same theme does not multiply backups.
        run_with(theme_gtk4("Orchis", Some(&theme.to_string_lossy()), &list_live, &read));
        assert!(!cfg.join("gtk.css.rexon-backup-2").exists());
        run_with(ThemeGtk4::Builtin);
        let left: Vec<String> = entries_live(&cfg_s).into_iter().map(|e| e.name).collect();
        assert_eq!(left, vec!["gtk.css".to_string()]);
        assert_eq!(fs::read_to_string(cfg.join("gtk.css")).unwrap(), "mine");
        assert!(cfg.join("settings.ini").is_file());
        let _ = fs::remove_dir_all(&root);
    }
}
