//! Port of `rexon/qt_settings/window.py`: the logic of the «Settings» tab.
//!
//! The tab is large and varied (event sounds, fonts, wallpapers, application
//! protection, the Plasma panel), but everything that is COMPUTED and DECIDED
//! in it needs neither a widget nor an event loop: where sound themes live,
//! which command reads the font in the current environment, what
//! "20260620_011005" turns into before the customer's eyes. Exactly that lives
//! here, and the Qt side remains drawing.
//!
//! The boundaries are the same as in the neighbouring core modules: the core
//! ONLY assembles commands (`*_argv`) and parses their output, the Python
//! executor runs them. The core reads the file system itself: sound theme
//! directories are walked here.
//!
//! ⚠️ Fixes to the original (rule #16: bugs found while porting are fixed, not
//! preserved), each marked `Fix` in place:
//!   1. the user sound theme directory was built from `~/.local/share`,
//!      bypassing `XDG_DATA_HOME`;
//!   2. `_patch_gtk_ini` inserted the key by the index "the line after the
//!      first `[Settings]`", and without the section at the start of the file
//!      it landed in a foreign section; now the text is assembled by the shared
//!      engine [`crate::gtk_sanitizer::write_ini_setting`];
//!   3. the Russian declension of "file/files" was a fourth copy; now the
//!      plural forms come from the translation catalog ([`crate::i18n::ntr`]).
//!
//! Parity check: `rust/rexon-core/tests/qt_settings_parity.rs`.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::i18n::{n_, ntr, tr, trc, trf};

// ============================================================================
// Wallpapers: file format by extension
// ============================================================================

/// Extensions the tab treats as a static picture.
pub const STATIC_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "tiff", "tif"];

/// Extensions played through the video path (GIF included).
pub const VIDEO_EXTS: &[&str] =
    &["mp4", "webm", "mkv", "avi", "mov", "m4v", "wmv", "mpg", "mpeg", "gif"];

/// Flash wallpaper extensions (the engine plays them through Ruffle).
///
/// The list is kept here by rule #12, next to [`STATIC_EXTS`] and
/// [`VIDEO_EXTS`], from which both the format parsing and the picker filter
/// are built. It does not contradict the second copy (`SUPPORTED_FLASH_FORMATS`
/// in `constants.rs`): there the extensions have a dot, for parsing file extensions.
pub const FLASH_EXTS: &[&str] = &["swf"];

/// File extension in lower case, without the dot.
fn ext_of(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext.to_lowercase(),
        _ => String::new(),
    }
}

/// Wallpaper format by path extension: `"static"`, `"video"` or `"swf"`.
///
/// An unknown extension gives `"static"`: showing a picture is safer than
/// trying to start a player on an incomprehensible file.
///
/// `.swf` was added here because before, the `swf` format was known to the
/// engine (`WallpaperApplier.apply` handles `case "swf"`) but not to the tab:
/// parsing returned `"static"`, and a flash movie the person chose silently
/// went off to be shown as a static picture.
pub fn detect_wallpaper_format(path: &str) -> &'static str {
    let ext = ext_of(path);
    if VIDEO_EXTS.contains(&ext.as_str()) {
        "video"
    } else if FLASH_EXTS.contains(&ext.as_str()) {
        "swf"
    } else {
        "static"
    }
}

/// The file picker filter by the same extension list as the format parsing:
/// «Images (*.jpg *.jpeg …)».
///
/// Rule #12: the project has one set of extension lists. Before, the picker
/// knew its own set and `_detect_wallpaper_format` its own, and `.tif` was not
/// in the filter: a file the engine accepted was simply not shown to the customer.
pub fn wallpaper_dialog_filter(kind: &str) -> String {
    // The dialog opens on the "native" group of the pressed button: what the
    // customer saw before, they see now. Then come «All wallpapers» and the other
    // groups: without them a Flash movie was unreachable at all. The tab has two
    // buttons («Picture…», «Video…»), there is no third one for .swf, and adding
    // one would require the person to assign their file to one of our categories
    // in advance. The format is computed from the chosen file's extension anyway
    // ([`detect_wallpaper_format`]), so switching the group in the dialog breaks
    // nothing: a .swf chosen from under the «Picture…» button applies as Flash,
    // not as a picture.
    // Groups are compared as WHOLE strings of the same translation, so the
    // interface language does not affect the choice of the "native" group.
    let all = || filter_group(&tr("All wallpapers"), &all_wallpaper_exts());
    let images = || filter_group(&tr("Images"), STATIC_EXTS);
    let video = || filter_group(&tr("Video"), VIDEO_EXTS);
    let flash = || filter_group("Flash", FLASH_EXTS);
    let own = match kind {
        "video" => video(),
        "swf" => flash(),
        "static" => images(),
        // `auto` is the "choose wallpaper" dialog: all applicable files in one list.
        _ => all(),
    };
    let mut out = vec![own.clone()];
    for g in [all(), images(), video(), flash()] {
        if g != own {
            out.push(g);
        }
    }
    out.join(";;")
}

/// One Qt filter group: `Video (*.mp4 *.webm)`.
fn filter_group(title: &str, exts: &[&str]) -> String {
    let masks: Vec<String> = exts.iter().map(|e| format!("*.{e}")).collect();
    format!("{title} ({})", masks.join(" "))
}

/// All extensions the tab can apply as wallpaper.
///
/// The order is pictures, video, Flash: the same order the groups go in the filter.
pub fn all_wallpaper_exts() -> Vec<&'static str> {
    STATIC_EXTS
        .iter()
        .chain(VIDEO_EXTS)
        .chain(FLASH_EXTS)
        .copied()
        .collect()
}

// ============================================================================
// Default file manager
// ============================================================================

/// Icon and caption of a file manager for the choice list.
///
/// The SET of managers itself is held by [`crate::appdefaults::FM_SHORT_NAMES`];
/// here is only how to name them to a person. The check "every one has a
/// caption" is in `qt_settings_parity.rs`: a new manager added to the core
/// will not appear in the list without a name.
///
/// The caption is a `msgid` (program names have nothing to translate,
/// «Automatic» does); the place that shows it translates it, [`fm_choices`].
pub fn fm_label(short: &str) -> (&'static str, &'static str) {
    match short {
        "auto" => ("⚙️", n_("Automatic (follows the desktop)")),
        // Budgie has no file manager of its own: it takes Nemo, as Ubuntu Budgie
        // does (the `budgie` package set in `system::DE_DATABASE`).
        "nemo" => ("🍃", "Nemo (Cinnamon, Budgie)"),
        "nautilus" => ("🦶", "Nautilus (GNOME)"),
        "thunar" => ("🐭", "Thunar (XFCE)"),
        "caja" => ("🦣", "Caja (MATE)"),
        "dolphin" => ("💎", "Dolphin (KDE)"),
        "pcmanfm" => ("📂", "PCManFM (LXDE)"),
        "pcmanfm-qt" => ("🗂️", "PCManFM-Qt (LXQt)"),
        "cosmic-files" => ("🪐", "COSMIC Files (COSMIC)"),
        "io.elementary.files" => ("🏛️", "Pantheon Files (Pantheon)"),
        "dde-file-manager" => ("🌊", "Deepin File Manager (Deepin)"),
        _ => ("📁", ""),
    }
}

/// The order of items in the list is the DISPLAY order, not preference.
///
/// It does not match [`crate::appdefaults::FM_SHORT_NAMES`] (that is the
/// auto-selection order) and is kept exactly as the customer saw it. The set,
/// however, must match: the check "the list has exactly the managers the core
/// knows" is in `qt_settings_parity.rs`, so a manager added to the core does
/// not get lost on the way to the window.
pub const FM_DISPLAY_ORDER: &[&str] = &[
    "nemo",
    "nautilus",
    "thunar",
    "caja",
    "dolphin",
    "pcmanfm",
    "pcmanfm-qt",
    "cosmic-files",
    "io.elementary.files",
    "dde-file-manager",
];

/// Items of the «File manager» list: (short name, ready caption).
/// «Automatic» first, then [`FM_DISPLAY_ORDER`].
pub fn fm_choices() -> Vec<(String, String)> {
    let row = |short: &str| {
        let (icon, label) = fm_label(short);
        let label = if label.is_empty() {
            short.to_string()
        } else {
            tr(label)
        };
        (short.to_string(), format!("{icon}  {label}"))
    };
    let mut out = vec![row("auto")];
    out.extend(FM_DISPLAY_ORDER.iter().map(|s| row(s)));
    out
}

// ============================================================================
// Sound themes: where they are, what they contain
// ============================================================================

/// Directories that are NOT sound themes, although they lie next to them.
pub const NOT_SOUND_THEMES: &[&str] = &["alsa", "speech-dispatcher", "sox"];

/// Sound file extensions of a theme.
pub const SOUND_EXTS: &[&str] = &["oga", "ogg", "wav"];

/// The theme substituted when there is none of its own or it could not be read.
pub const DEFAULT_SOUND_THEME: &str = "freedesktop";

/// Sound theme directories: system and user.
///
/// Fix: the user path was built as `~/.local/share/sounds`, bypassing
/// `XDG_DATA_HOME`: on systems where the data directory is overridden, own
/// themes did not appear in the list. `data_home` comes from [`crate::utils::data_home`].
pub fn sound_theme_dirs(data_home: &str) -> Vec<String> {
    vec![
        "/usr/share/sounds".to_string(),
        format!("{}/sounds", data_home.trim_end_matches('/')),
    ]
}

/// Sound theme search directories by the XDG specification (freedesktop
/// sound-theme): `$XDG_DATA_HOME/sounds`, then `<each of
/// $XDG_DATA_DIRS>/sounds`; an empty `XDG_DATA_DIRS` means `/usr/local/share:/usr/share`.
/// Repeats are removed, the order is kept (the first wins, as in libcanberra).
pub fn xdg_sound_theme_dirs(data_home: &str, data_dirs: Option<&str>) -> Vec<String> {
    let dirs = match data_dirs {
        Some(v) if !v.trim().is_empty() => v.to_string(),
        _ => "/usr/local/share:/usr/share".to_string(),
    };
    let mut out: Vec<String> = Vec::new();
    let first = format!("{}/sounds", data_home.trim_end_matches('/'));
    for d in std::iter::once(first).chain(
        dirs.split(':')
            .filter(|d| !d.is_empty())
            .map(|d| format!("{}/sounds", d.trim_end_matches('/'))),
    ) {
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// [`xdg_sound_theme_dirs`] from the process environment.
pub fn live_xdg_sound_theme_dirs() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data_home = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), &home);
    xdg_sound_theme_dirs(&data_home, std::env::var("XDG_DATA_DIRS").ok().as_deref())
}

/// The directory is a sound theme by the specification: it has an `index.theme`
/// with a `[Sound Theme]` section. That is exactly how KDE (the «System Sounds»
/// kcm) and libcanberra look for a theme; a directory without it is not a theme for them.
pub fn is_sound_theme_dir(dir: &Path) -> bool {
    fs::read_to_string(dir.join("index.theme"))
        .map(|t| t.lines().any(|l| l.trim() == "[Sound Theme]"))
        .unwrap_or(false)
}

/// Whether the theme `name` exists at the TOP level of at least one of `dirs`.
/// A name with `/` or an empty one is not a theme name.
pub fn sound_theme_exists(dirs: &[String], name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return false;
    }
    dirs.iter().any(|d| is_sound_theme_dir(&Path::new(d).join(name)))
}

/// A refusal to apply a theme that is not on disk (instead of "theme applied":
/// the system silently replaces a written nonexistent name with `freedesktop`).
pub fn sound_theme_missing_message(name: &str) -> String {
    // TRANSLATORS: %1 is a sound theme name.
    trf(
        "Sound theme “%1” not found: there is no folder with its index.theme. Nothing was changed.",
        &[&name],
    )
}

/// Installed sound themes: only real themes ([`is_sound_theme_dir`]:
/// `index.theme` with a `[Sound Theme]` section), exactly as KDE and
/// libcanberra see them.
///
/// ⚠️ Until 03.10.2026 any directory with sound files inside was also counted
/// as a theme. The list got the GNOME, MATE and Budgie sets without
/// `index.theme` (`/usr/share/sounds/gnome`, `…/mate`, `…/budgie`) and archive
/// wrappers: they could be chosen, while the system silently replaces such a
/// name with `freedesktop`: "theme applied", but something else sounds.
///
/// An empty result is replaced with [`DEFAULT_SOUND_THEME`]: a drop-down list
/// without a single item is of no use to the customer.
pub fn list_sound_themes(dirs: &[String]) -> Vec<String> {
    let mut themes: BTreeSet<String> = BTreeSet::new();
    for base in dirs {
        let Ok(entries) = fs::read_dir(base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if NOT_SOUND_THEMES.contains(&name.as_str()) {
                continue;
            }
            if is_sound_theme_dir(&path) {
                themes.insert(name);
            }
        }
    }
    if themes.is_empty() {
        return vec![DEFAULT_SOUND_THEME.to_string()];
    }
    themes.into_iter().collect()
}

/// Walking a theme tree: calls `visit` on each sound file (name without
/// extension, full path). `visit` returns `false` to stop.
fn walk_sounds(dir: &Path, visit: &mut dyn FnMut(&str, &str) -> bool) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return true;
    };
    let mut subdirs = Vec::new();
    let mut files: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let Some((stem, ext)) = name.rsplit_once('.') else {
            continue;
        };
        if !SOUND_EXTS.contains(&ext.to_lowercase().as_str()) {
            continue;
        }
        files.push((stem.to_string(), path.to_string_lossy().to_string()));
    }
    // The order within a directory from `read_dir` is undefined: we sort, otherwise
    // "which file was found first" would depend on the file system.
    files.sort();
    for (stem, path) in files {
        if !visit(&stem, &path) {
            return false;
        }
    }
    subdirs.sort();
    for sub in subdirs {
        if !walk_sounds(&sub, visit) {
            return false;
        }
    }
    true
}

/// Path to an event file in the theme (recursively), or `None`.
///
/// The search order is the same as in the original: first directory by
/// directory, within a directory extension by extension ([`SOUND_EXTS`]), and
/// only then deeper. So `bell.oga` beats `bell.wav`, wherever that one lies in
/// the same theme.
pub fn find_sound_file(dirs: &[String], theme: &str, event_id: &str) -> Option<String> {
    // An empty theme name would give the WHOLE themes directory, and the search
    // would pull out a sound of an arbitrary other theme. No theme set means freedesktop.
    let theme = effective_sound_theme(theme);
    for base in dirs {
        let theme_dir = Path::new(base).join(theme);
        if !theme_dir.is_dir() {
            continue;
        }
        for want_ext in SOUND_EXTS {
            let mut found: Option<String> = None;
            walk_sounds(&theme_dir, &mut |stem, path| {
                if stem == event_id && ext_of(path) == *want_ext {
                    found = Some(path.to_string());
                    return false;
                }
                true
            });
            if found.is_some() {
                return found;
            }
        }
    }
    None
}

/// All events of a theme (file names without extension), alphabetically.
///
/// No theme set means the events of `freedesktop`: the "listen" buttons must
/// play the same as the system plays (see [`effective_sound_theme`]), not pile
/// up the events of all installed themes at once.
pub fn list_theme_events(dirs: &[String], theme: &str) -> Vec<String> {
    let theme = effective_sound_theme(theme);
    let mut events: BTreeSet<String> = BTreeSet::new();
    for base in dirs {
        let theme_dir = Path::new(base).join(theme);
        if !theme_dir.is_dir() {
            continue;
        }
        walk_sounds(&theme_dir, &mut |stem, _path| {
            events.insert(stem.to_string());
            true
        });
    }
    events.into_iter().collect()
}

/// Events shown as "listen" buttons: recognizable ones first, the rest are
/// added alphabetically up to `limit`.
pub const SOUND_PREFERRED_EVENTS: &[&str] = &[
    "bell",
    "complete",
    "dialog-error",
    "dialog-warning",
    "dialog-information",
    "message-new-instant",
    "desktop-login",
    "desktop-logout",
    "audio-volume-change",
    "device-added",
];

/// Icon and human caption of an event: (id, icon, caption).
///
/// The caption is a `msgid`; [`event_meta`] translates it.
pub const SOUND_EVENT_META: &[(&str, &str, &str)] = &[
    ("bell", "🔔", n_("Bell")),
    ("bell-window-system", "🔔", n_("System bell")),
    ("complete", "✅", n_("Done")),
    ("dialog-error", "🔴", n_("Error")),
    ("dialog-warning", "⚠️", n_("Warning")),
    ("dialog-information", "ℹ️", n_("Notification")),
    ("alarm-clock-elapsed", "⏰", n_("Alarm clock")),
    ("battery-low", "🔋", n_("Battery low")),
    ("battery-caution", "🪫", n_("Battery critical")),
    ("message-new-instant", "💬", n_("New message")),
    ("message", "💬", n_("Message")),
    ("service-login", "🔓", n_("Service login")),
    ("desktop-login", "✅", n_("Desktop login")),
    ("desktop-logout", "🔒", n_("Desktop logout")),
    ("audio-volume-change", "🔊", n_("Volume")),
    ("device-added", "🔌", n_("Device connected")),
    ("device-removed", "❌", n_("Device disconnected")),
    ("trash-empty", "🗑️", n_("Emptying the trash")),
    ("screen-capture", "📸", n_("Screenshot")),
];

/// Which events to show as buttons: first the recognizable ones from
/// [`SOUND_PREFERRED_EVENTS`], then the rest, up to `limit` of them.
pub fn showcase_events(events: &[String], limit: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for want in SOUND_PREFERRED_EVENTS {
        if out.len() >= limit {
            return out;
        }
        if events.iter().any(|e| e == want) {
            out.push((*want).to_string());
        }
    }
    for ev in events {
        if out.len() >= limit {
            break;
        }
        if !out.contains(ev) {
            out.push(ev.clone());
        }
    }
    out
}

/// Icon and caption of an event. An unknown event is labeled with its own id
/// in a human way: `message-new-instant` → «Message new instant».
pub fn event_meta(event_id: &str) -> (String, String) {
    for (id, emoji, label) in SOUND_EVENT_META {
        if *id == event_id {
            return ((*emoji).to_string(), tr(label));
        }
    }
    let words: String = event_id.replace(['-', '_'], " ");
    let mut chars = words.chars();
    let label = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    ("▶️".to_string(), label)
}

// ============================================================================
// Sound themes: how to read and write them in the current environment
// ============================================================================

/// What is configured: the theme itself or the fact "event sounds are on".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundSetting {
    Theme,
    EventSounds,
}

/// Where the setting goes in this environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundBackend {
    /// `gsettings get|set <schema> <key>`
    GSettings { schema: String, key: String },
    /// `xfconf-query -c <channel> -p <prop> [-s <value>]`
    Xfconf { channel: String, prop: String },
    /// `kreadconfig6|kwriteconfig6 --file <file> --group <group> --key <key>`
    KConfig {
        file: String,
        group: String,
        key: String,
    },
    /// Deepin ([`crate::dde`]): writing is a ready DDE service command, the value
    /// is appended as the last argument; reading is `dde-dconfig get` of the key
    /// the service puts the same value into.
    Dde {
        write: Vec<String>,
        read: crate::dde::DConfigKey,
    },
}

/// A DDE service command without the last argument (the value): the template
/// of a [`SoundBackend::Dde`] write.
fn dde_write_prefix(mut argv: Vec<String>) -> Vec<String> {
    argv.pop();
    argv
}

/// Where the sound setting lives for environment `de`.
///
/// Tiling and wlroots sessions (Hyprland, niri, sway, "unknown") go by the
/// shared `org.gnome.desktop.sound` branch: libcanberra applications read it
/// even without GNOME. One shared branch instead of a per-WM list: rule #12.
pub fn sound_backend(de: &str, setting: SoundSetting) -> SoundBackend {
    let key = match setting {
        SoundSetting::Theme => "theme-name",
        SoundSetting::EventSounds => "event-sounds",
    };
    let gsettings = |schema: &str| SoundBackend::GSettings {
        schema: schema.to_string(),
        key: key.to_string(),
    };
    if de == "deepin" {
        // The DDE appearance service does not accept the sound theme through `Set`:
        // it is held by the DConfig key `Sound_Theme`, watched by the sound service
        // (dde-daemon `soundeffect1`). The master switch is the sound service's
        // `Enabled` property: its handler also passes the value to the player.
        return match setting {
            SoundSetting::Theme => SoundBackend::Dde {
                write: dde_write_prefix(crate::dde::dconfig_set_argv(crate::dde::SOUND_THEME, "")),
                read: crate::dde::SOUND_THEME,
            },
            SoundSetting::EventSounds => SoundBackend::Dde {
                write: dde_write_prefix(crate::dde::event_sounds_argv(true)),
                read: crate::dde::EVENT_SOUNDS,
            },
        };
    }
    match de {
        // Pantheon: pantheon-default-settings sets `theme-name='elementary'` exactly
        // in org.gnome.desktop.sound (the `:Pantheon` override).
        "gnome" | "budgie" | "pantheon" | "cosmic" => gsettings("org.gnome.desktop.sound"),
        "cinnamon" | "cinnamon-mint" => gsettings("org.cinnamon.desktop.sound"),
        "mate" => gsettings("org.mate.sound"),
        "xfce" => SoundBackend::Xfconf {
            channel: "xsettings".to_string(),
            prop: match setting {
                SoundSetting::Theme => "/Net/SoundThemeName".to_string(),
                SoundSetting::EventSounds => "/Net/EnableEventSounds".to_string(),
            },
        },
        "kde" => SoundBackend::KConfig {
            file: "kdeglobals".to_string(),
            group: "Sounds".to_string(),
            key: match setting {
                SoundSetting::Theme => "Theme".to_string(),
                // KF6 Notifications reads `[Sounds] Enable` (bool, default true):
                // knotifications `src/notifybyaudio.cpp:42,48`
                // `group.readEntry("Enable", true)`. The former
                // `EnableNotificationSounds` is read by nobody: the switch did not work in
                // KDE at all.
                SoundSetting::EventSounds => "Enable".to_string(),
            },
        },
        _ => gsettings("org.gnome.desktop.sound"),
    }
}

/// argv of reading the setting. `kde_tool` is the found `kreadconfig6` (empty
/// means no utility, nothing to read with: `None`).
pub fn sound_read_argv(backend: &SoundBackend, kde_tool: &str) -> Option<Vec<String>> {
    Some(match backend {
        SoundBackend::GSettings { schema, key } => {
            vec![
                "gsettings".to_string(),
                "get".to_string(),
                schema.clone(),
                key.clone(),
            ]
        }
        SoundBackend::Xfconf { channel, prop } => vec![
            "xfconf-query".to_string(),
            "-c".to_string(),
            channel.clone(),
            "-p".to_string(),
            prop.clone(),
        ],
        SoundBackend::KConfig { file, group, key } => {
            if kde_tool.is_empty() {
                return None;
            }
            vec![
                kde_tool.to_string(),
                "--file".to_string(),
                file.clone(),
                "--group".to_string(),
                group.clone(),
                "--key".to_string(),
                key.clone(),
                "--default".to_string(),
                String::new(),
            ]
        }
        SoundBackend::Dde { read, .. } => crate::dde::dconfig_get_argv(*read),
    })
}

/// Type of the xfconf property the tab writes: the sound switch is `bool`;
/// font antialiasing, hinting and DPI are `int` (that is how
/// `xfce4-appearance-settings` creates them: `-1` means "not set" for them);
/// the rest (sound theme, fonts, `/Xft/HintStyle`, `/Xft/RGBA`) is a string.
fn xfconf_value_type(prop: &str) -> &'static str {
    match prop {
        "/Net/EnableEventSounds" => "bool",
        "/Xft/Antialias" | "/Xft/Hinting" | "/Xft/DPI" => "int",
        // xfdesktop: the icon caption size and its switch.
        XFDESKTOP_FONT_SIZE_PROP => "double",
        XFDESKTOP_USE_SIZE_PROP => "bool",
        _ => "string",
    }
}

/// argv of writing the setting. `kde_tool` is the found `kwriteconfig6`.
pub fn sound_write_argv(
    backend: &SoundBackend,
    value: &str,
    kde_tool: &str,
) -> Option<Vec<String>> {
    Some(match backend {
        SoundBackend::GSettings { schema, key } => vec![
            "gsettings".to_string(),
            "set".to_string(),
            schema.clone(),
            key.clone(),
            value.to_string(),
        ],
        // `-n -t <type>`: on a profile where the setting was never changed there is
        // no property in xfconf (the xfwm4 default lies in /usr/share/xfwm4/defaults,
        // not in the channel), and a bare `-s` refuses with code 1: the title font
        // was not applied in XFCE at all (D6, environment check 02.10.2026).
        // The type is mandatory on creation; for an existing property of the same
        // type it changes nothing.
        SoundBackend::Xfconf { channel, prop } => vec![
            "xfconf-query".to_string(),
            "-c".to_string(),
            channel.clone(),
            "-p".to_string(),
            prop.clone(),
            "-n".to_string(),
            "-t".to_string(),
            xfconf_value_type(prop).to_string(),
            "-s".to_string(),
            value.to_string(),
        ],
        SoundBackend::KConfig { file, group, key } => {
            if kde_tool.is_empty() {
                return None;
            }
            // `--notify`: running KDE programs watch kdeglobals through KConfigWatcher
            // (that is how KF6 Notifications learns about `[Sounds]`,
            // `notifybyaudio.cpp:32-43`), and the change signal is sent only when
            // writing with notification. Without it the setting would reach only newly
            // started programs.
            vec![
                kde_tool.to_string(),
                "--file".to_string(),
                file.clone(),
                "--group".to_string(),
                group.clone(),
                "--key".to_string(),
                key.clone(),
                "--notify".to_string(),
                value.to_string(),
            ]
        }
        // No write (Deepin fonts): nothing to write with.
        SoundBackend::Dde { write, .. } if write.is_empty() => return None,
        SoundBackend::Dde { write, .. } => {
            let mut argv = write.clone();
            argv.push(value.to_string());
            argv
        }
    })
}

/// argv of reading the sound setting directly by environment name, so the
/// executor does not have to assemble a [`SoundBackend`] by hand.
///
/// KDE has no `[Sounds] Enable` key on a clean profile, and KF6 Notifications
/// treats a missing key as enabled (`readEntry("Enable", true)`), so we also
/// read with the default `true`; otherwise the tab would show "off" while the
/// sounds are playing.
pub fn sound_read_argv_for(de: &str, setting: SoundSetting, kde_tool: &str) -> Option<Vec<String>> {
    let backend = sound_backend(de, setting);
    let mut argv = sound_read_argv(&backend, kde_tool)?;
    if setting == SoundSetting::EventSounds && matches!(backend, SoundBackend::KConfig { .. }) {
        if let Some(last) = argv.last_mut() {
            *last = event_sounds_value(true).to_string();
        }
    }
    Some(argv)
}

/// argv of writing the sound setting directly by environment name.
pub fn sound_write_argv_for(
    de: &str,
    setting: SoundSetting,
    value: &str,
    kde_tool: &str,
) -> Option<Vec<String>> {
    sound_write_argv(&sound_backend(de, setting), value, kde_tool)
}

/// The full plan of changing a sound setting (the theme or "event sounds
/// on/off"): what to write with and what to verify with.
///
/// The main mechanism is the environment's own ([`sound_backend`]). A shared
/// layer is added to it so that the setting reaches programs of a FOREIGN
/// toolkit in any environment:
/// * `org.gnome.desktop.sound` (`theme-name` / `event-sounds`): read by
///   libcanberra, that is GTK programs; without it, in KDE sounds would change
///   only for KDE itself;
/// * `kdeglobals [Sounds] Theme / Enable`: read by KF6 Notifications, that is
///   KDE/Qt programs under GNOME, XFCE and others (written only if
///   `kwriteconfig6` exists).
///
/// No duplicate is added if the main mechanism is that branch. In KDE, when
/// writing "event sounds", the junk `EnableNotificationSounds` that older
/// Rexon versions wrote instead of `Enable` is removed.
///
/// Verification is mandatory: a command may finish with code 0 and write
/// nothing (no kwriteconfig6, no gsettings schema). Applying counts as
/// successful only if the MAIN mechanism returned what was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundThemePlan {
    /// Write commands: `[0]` is the main one (success is decided by it), the rest
    /// are auxiliary (best-effort).
    pub write: Vec<Vec<String>>,
    /// Read command of the main mechanism (None means nothing to read with).
    pub verify: Option<Vec<String>>,
}

/// Build a [`SoundThemePlan`] for the setting `setting` with value `value`
/// (a theme name or [`event_sounds_value`]). `kwrite`/`kread` are the found
/// kwriteconfig6 / kreadconfig6 (empty means no utility); `has_gsettings` is
/// whether `gsettings` was found (if not, the shared GNOME branch is not offered).
pub fn sound_plan(
    de: &str,
    setting: SoundSetting,
    value: &str,
    kwrite: &str,
    kread: &str,
    has_gsettings: bool,
) -> SoundThemePlan {
    let backend = sound_backend(de, setting);
    let mut write = Vec::new();
    if let Some(argv) = sound_write_argv(&backend, value, kwrite) {
        write.push(argv);
    }
    let gnome = sound_backend("gnome", setting);
    if has_gsettings && backend != gnome {
        if let Some(argv) = sound_write_argv(&gnome, value, "") {
            write.push(argv);
        }
    }
    let kde = sound_backend("kde", setting);
    if backend != kde {
        if let Some(argv) = sound_write_argv(&kde, value, kwrite) {
            write.push(argv);
        }
    }
    // Deepin: the XSettings service hands the sound theme to GTK programs from its
    // own key, separate from the appearance service's `Sound_Theme`.
    if de == "deepin" && setting == SoundSetting::Theme {
        write.push(crate::dde::dconfig_set_argv(crate::dde::XS_SOUND_THEME, value));
    }
    if setting == SoundSetting::EventSounds && !kwrite.is_empty() {
        write.push(vec![
            kwrite.to_string(),
            "--file".to_string(),
            "kdeglobals".to_string(),
            "--group".to_string(),
            "Sounds".to_string(),
            "--key".to_string(),
            "EnableNotificationSounds".to_string(),
            "--delete".to_string(),
        ]);
    }
    SoundThemePlan {
        verify: sound_read_argv(&backend, kread),
        write,
    }
}

/// The sound theme change plan: [`sound_plan`] for [`SoundSetting::Theme`].
pub fn sound_theme_plan(
    de: &str,
    name: &str,
    kwrite: &str,
    kread: &str,
    has_gsettings: bool,
) -> SoundThemePlan {
    sound_plan(de, SoundSetting::Theme, name, kwrite, kread, has_gsettings)
}

/// Theme name from the command output: `'freedesktop'\n` → `freedesktop`.
///
/// An empty answer means exactly one thing, the theme is NOT SET, and it is
/// returned as an empty string. Previously [`DEFAULT_SOUND_THEME`] was
/// substituted here, and on a clean profile the «Settings» tab assured the
/// person they had `freedesktop`, although nothing was set: the «— not set —»
/// line (caption `snd.theme_unset`) was NEVER reachable.
///
/// The default is needed not by the caption but by the sound: a system
/// without a set theme plays `freedesktop`. That second path is separated:
/// [`effective_sound_theme`], and it lives where the sound is LOOKED UP on
/// disk, not where the theme is shown to a person.
pub fn parse_sound_theme(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c| c == '\'' || c == '"')
        .trim()
        .to_string()
}

/// The theme whose sounds the system will ACTUALLY use: the set one, and when
/// there is none, [`DEFAULT_SOUND_THEME`] (libcanberra does the same).
///
/// Not suitable for a caption: about an unset theme the person must be told
/// that it is not set. Suitable where the sound is looked up and played.
pub fn effective_sound_theme(theme: &str) -> &str {
    let name = theme.trim();
    if name.is_empty() {
        DEFAULT_SOUND_THEME
    } else {
        name
    }
}

/// Whether event sounds are on, by the command's answer.
pub fn parse_event_sounds(raw: &str) -> bool {
    let value = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    matches!(value.to_lowercase().as_str(), "true" | "1" | "yes")
}

/// The "on/off" value in the form all three settings storages understand.
pub fn event_sounds_value(enabled: bool) -> &'static str {
    if enabled {
        "true"
    } else {
        "false"
    }
}

/// The answer to "the theme has no such sound": a ready phrase for a person.
pub fn msg_no_event_in_theme(theme: &str) -> String {
    // TRANSLATORS: %1 is the name of a sound theme.
    trf("The “%1” theme has no sound for this event", &[&theme])
}

// ============================================================================
// System fonts
// ============================================================================

/// What is configured, in the order of the Plasma 6 «Fonts» page: the
/// interface font, monospace, small, toolbar, menu, window titles; after them,
/// what Plasma does not have but GNOME, Cinnamon and MATE do: the document
/// font ([`DOCUMENT_FONT_GNOME`]) and the desktop icon caption font ([`DESKTOP_FONT_DES`]).
pub const FONT_TARGETS: &[&str] = &["system", "mono", "small", "toolbar", "menu", "title", "document", "desktop"];

/// The GNOME document font: `org.gnome.desktop.interface document-font-name`
/// (gsettings-desktop-schemas, "Name of the default font used for reading
/// documents"). The Cinnamon fonts page writes it too (`cs_fonts.py`:
/// `org.cinnamon.desktop.interface` has no such key of its own). It is read
/// by individual GTK programs (document viewers, mail, the GNOME browser),
/// not by GTK itself; Qt and KDE programs do not know it at all. So the key is
/// written in any environment: GTK programs will see it in KDE too.
pub const DOCUMENT_FONT_GNOME: (&str, &str) = ("org.gnome.desktop.interface", "document-font-name");

/// Environments whose desktop program has its own icon caption font: Cinnamon
/// with Nemo (`org.nemo.desktop font`, written by `cs_fonts.py` as "Desktop
/// font"), MATE with Caja (`org.mate.caja.desktop font`), XFCE with
/// xfdesktop, where only the size is set ([`XFDESKTOP_FONT_SIZE_PROP`]), LXQt
/// with pcmanfm-qt (`[Desktop] Font` in its `settings.conf`, with protection of
/// the desktop settings, [`pcmanfm_desktop_font_apply`]). KDE Plasma (the
/// «Desktop Folder» takes the interface font), GNOME, Budgie
/// (`budgie-desktop-view`: icon size, no font), Pantheon, COSMIC and tiling
/// sessions have no key of their own; there the item is honestly disabled
/// ([`desktop_font_note`]).
pub const DESKTOP_FONT_DES: &[&str] = &["cinnamon", "cinnamon-mint", "mate", "xfce", "lxqt"];

/// The xfdesktop xfconf channel.
pub const XFDESKTOP_CHANNEL: &str = "xfce4-desktop";
/// xfdesktop icon caption size (type `double`, in points).
pub const XFDESKTOP_FONT_SIZE_PROP: &str = "/desktop-icons/font-size";
/// The switch of the own size: with `false` xfdesktop takes the size of the
/// interface font, and [`XFDESKTOP_FONT_SIZE_PROP`] has no effect.
pub const XFDESKTOP_USE_SIZE_PROP: &str = "/desktop-icons/use-custom-font-size";

/// Fonts whose native key exists only in KDE (`kdeglobals [General]
/// smallestReadableFont`, `toolBarFont`, `menuFont`). Neither GNOME nor XFCE,
/// Cinnamon, MATE, LXQt, COSMIC keep separate fonts for small text, toolbars
/// and menus (their schemas and settings pages were checked 04.10.2026).
/// There Rexon's shared layer carries them: our own block in
/// `gtk-{3,4}.0/gtk.css` for GTK programs and our own qt6ct/qt5ct stylesheet
/// for Qt programs ([`font_plan`], [`ExtraFontDialect`]).
pub const EXTRA_FONT_TARGETS: &[&str] = &["small", "toolbar", "menu"];

/// Where the font is written in this environment by a command (gsettings /
/// xfconf / kwriteconfig6): the same breakdown as for sounds, but with its own keys.
///
/// The keys were checked against the primary sources (30.09.2026):
/// * Cinnamon: its own settings module `cinnamon-settings/modules/cs_fonts.py`
///   writes `org.cinnamon.desktop.interface font-name` (line 32),
///   `org.gnome.desktop.interface monospace-font-name` (line 41: Cinnamon has
///   no monospace key of its own; the `org.cinnamon.desktop.interface` schema
///   indeed lacks it) and `org.cinnamon.desktop.wm.preferences titlebar-font` (line 44).
/// * MATE: `mate-control-center/capplets/appearance/appearance.h`:
///   `INTERFACE_SCHEMA "org.mate.interface"`, `GTK_FONT_KEY "font-name"`,
///   `MONOSPACE_FONT_KEY "monospace-font-name"`,
///   `MARCO_SCHEMA "org.mate.Marco.general"`,
///   `WINDOW_TITLE_FONT_KEY "titlebar-font"` (lines 57–75).
/// * Budgie reads the shared GNOME keys: the `labwc_bridge.py` bridge takes
///   `org.gnome.desktop.wm.preferences titlebar-font` (lines 434, 1309), the
///   panel `org.gnome.desktop.interface`.
/// * Pantheon: GNOME keys: pantheon-default-settings sets for `:Pantheon`
///   `org.gnome.desktop.interface font-name`/`monospace-font-name`, and the
///   titles of windows without CSD are drawn by Gala (Mutter) by
///   `org.gnome.desktop.wm.preferences titlebar-font`.
/// * Tiling sessions and an unrecognized environment: GNOME keys too, GTK
///   itself reads them.
///
/// COSMIC and LXQt keep the font not via a command but in a file; their native
/// storage is described by [`font_plan`]; here the GNOME keys remain for them,
/// read by the GTK programs of these environments.
pub fn font_backend(de: &str, target: &str) -> SoundBackend {
    // Deepin: read-only, the current font from the appearance service's DConfig.
    // No write: in a Treeland session only the Control Center sets fonts, and
    // Rexon does not support other Deepin sessions (the picker does not open, the
    // reason is [`crate::dde::refusal_live`]). Deepin has no title font.
    if de == "deepin" && matches!(target, "system" | "mono") {
        let read = if target == "mono" { crate::dde::FONT_MONOSPACE } else { crate::dde::FONT_STANDARD };
        return SoundBackend::Dde { write: Vec::new(), read };
    }
    // The document font is a gsettings key in any environment, KDE and XFCE
    // included: they have no such key of their own, and GTK programs read the GNOME key.
    if target == "document" {
        let (schema, key) = font_gsettings_key(de, target);
        return SoundBackend::GSettings { schema: schema.to_string(), key: key.to_string() };
    }
    match de {
        "kde" => {
            let (group, key) = match target {
                "mono" => ("General", "fixed"),
                "small" => ("General", "smallestReadableFont"),
                "toolbar" => ("General", "toolBarFont"),
                "menu" => ("General", "menuFont"),
                "title" => ("WM", "activeFont"),
                _ => ("General", "font"),
            };
            SoundBackend::KConfig {
                file: "kdeglobals".to_string(),
                group: group.to_string(),
                key: key.to_string(),
            }
        }
        "xfce" => {
            let (channel, prop) = match target {
                "mono" => ("xsettings", "/Gtk/MonospaceFontName"),
                "title" => ("xfwm4", "/general/title_font"),
                "desktop" => (XFDESKTOP_CHANNEL, XFDESKTOP_FONT_SIZE_PROP),
                _ => ("xsettings", "/Gtk/FontName"),
            };
            SoundBackend::Xfconf {
                channel: channel.to_string(),
                prop: prop.to_string(),
            }
        }
        _ => {
            let (schema, key) = font_gsettings_key(de, target);
            SoundBackend::GSettings {
                schema: schema.to_string(),
                key: key.to_string(),
            }
        }
    }
}

/// The gsettings font key for environments that keep it in dconf.
fn font_gsettings_key(de: &str, target: &str) -> (&'static str, &'static str) {
    match (de, target) {
        // MATE keeps the document font in its own schema (that is how
        // `mate-appearance-properties` writes it), the others in the GNOME schema.
        ("mate", "document") => ("org.mate.interface", "document-font-name"),
        (_, "document") => DOCUMENT_FONT_GNOME,
        ("cinnamon" | "cinnamon-mint", "desktop") => ("org.nemo.desktop", "font"),
        ("mate", "desktop") => ("org.mate.caja.desktop", "font"),
        ("cinnamon" | "cinnamon-mint", "mono") => ("org.gnome.desktop.interface", "monospace-font-name"),
        ("cinnamon" | "cinnamon-mint", "title") => ("org.cinnamon.desktop.wm.preferences", "titlebar-font"),
        ("cinnamon" | "cinnamon-mint", _) => ("org.cinnamon.desktop.interface", "font-name"),
        ("mate", "mono") => ("org.mate.interface", "monospace-font-name"),
        ("mate", "title") => ("org.mate.Marco.general", "titlebar-font"),
        ("mate", _) => ("org.mate.interface", "font-name"),
        (_, "mono") => ("org.gnome.desktop.interface", "monospace-font-name"),
        (_, "title") => ("org.gnome.desktop.wm.preferences", "titlebar-font"),
        _ => ("org.gnome.desktop.interface", "font-name"),
    }
}

/// The file the environment keeps the font in (path relative to `$XDG_CONFIG_HOME`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontFile {
    /// A `libcosmic` key in `cosmic-config`: the file
    /// `cosmic/com.system76.CosmicTk/v1/<key>` with a `FontConfig` RON structure
    /// (libcosmic `src/config/mod.rs`: `ID = "com.system76.CosmicTk"`,
    /// `#[version = 1]`, fields `interface_font` / `monospace_font`, `FontConfig`
    /// has `family`, `weight`, `stretch`, `style`; `cosmic-config` writes it
    /// through `ron::ser::to_string_pretty`).
    CosmicTk { key: String },
    /// A QSettings ini file key: a quoted QFont string (without quotes QSettings
    /// reads the commas as a list, and `toString()` returns empty).
    /// `create = false` means do not create the file if it is missing: it belongs
    /// to a foreign program the person may not use (qt6ct).
    QtIni {
        path: String,
        group: String,
        key: String,
        create: bool,
    },
    /// Our font block in `gtk-<version>/gtk.css` (the shared layer for GTK
    /// programs where the environment has no key of its own): `target` is
    /// `small`/`toolbar`/`menu`, see [`extra_fonts_block`].
    GtkCss { version: String, target: String },
    /// Our stylesheet `<tool>/qss/rexon-fonts.qss` and its line in
    /// `<tool>/<tool>.conf [Interface] stylesheets` (`tool` is `qt6ct` or
    /// `qt5ct`). Written only if the person uses this program: its `.conf`
    /// already exists.
    QtCtQss { tool: String, target: String },
    /// The window title font in the `rc.xml` of Openbox or labwc: the elements
    /// `<theme><font place="ActiveWindow">` and `place="InactiveWindow"`
    /// ([`rc_xml_with_title_font`]).
    RcXml { wm: TitleWm },
    /// The sway title font: our file `sway/rexon-title-font.conf` with a
    /// `font pango:…` line, included with a managed block into the sway config.
    /// It is written not by [`FontFile::file_writes`] but by
    /// [`sway_title_font_apply`]: the include, seeding the personal config from the
    /// system one and the `sway --validate` check are the shared compositor
    /// mechanism ([`crate::animations::wm_include`]), the same as the accent's.
    SwayConf,
    /// The LXQt desktop icon caption font: `[Desktop] Font` in
    /// `pcmanfm-qt/lxqt/settings.conf`. It is written not by [`FontFile::file_writes`]
    /// but by [`pcmanfm_desktop_font_apply`]: close the live desktop, back up,
    /// seed the own file from the system one, edit one line, start the desktop.
    PcmanfmDesktop,
}

/// One file write of a font plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFileWrite {
    /// Path relative to `$XDG_CONFIG_HOME`.
    pub rel_path: String,
    pub text: String,
    /// A symbolic link lies in place of the file (a GTK theme included by a link):
    /// remove it and put a regular file; writing THROUGH it is not allowed, that
    /// would edit a foreign theme.
    pub replace_link: bool,
}

/// How a font plan reads files: the text by path relative to `$XDG_CONFIG_HOME`
/// and the symbolic link target (an absolute path) if there is a link in that place.
pub trait FontFileIo {
    fn read(&self, rel_path: &str) -> Option<String>;
    fn link_target(&self, rel_path: &str) -> Option<String>;
    /// The same path in the system config directories (`$XDG_CONFIG_DIRS`,
    /// otherwise `/etc/xdg`): the sample an own file is created from.
    fn read_system(&self, _rel_path: &str) -> Option<String> {
        None
    }
}

impl FontFile {
    /// File path relative to `$XDG_CONFIG_HOME`.
    pub fn rel_path(&self) -> String {
        match self {
            FontFile::CosmicTk { key } => format!("cosmic/com.system76.CosmicTk/v1/{key}"),
            FontFile::QtIni { path, .. } => path.clone(),
            FontFile::GtkCss { version, .. } => format!("gtk-{version}/gtk.css"),
            FontFile::QtCtQss { tool, .. } => format!("{tool}/qss/{QTCT_QSS_NAME}"),
            FontFile::RcXml { wm } => wm.rc_xml_path().unwrap_or_default().to_string(),
            FontFile::SwayConf => format!("sway/{SWAY_FONT_STEM}.conf"),
            FontFile::PcmanfmDesktop => format!("{PCMANFM_PROFILE_REL}/settings.conf"),
        }
    }

    /// Whether the file may be created if it does not exist yet.
    pub fn may_create(&self) -> bool {
        match self {
            FontFile::CosmicTk { .. } | FontFile::GtkCss { .. } | FontFile::QtCtQss { .. } | FontFile::SwayConf => true,
            // The own pcmanfm-qt file shadows the system one entirely: it is created only
            // as a copy of the system one ([`pcmanfm_desktop_font_apply`]).
            FontFile::PcmanfmDesktop => false,
            FontFile::QtIni { create, .. } => *create,
            // Openbox reads its own rc.xml INSTEAD of the system one: created with just a
            // font, it would leave windows without key and mouse bindings. Without an own
            // one, a copy of the system one is taken ([`FontFileIo::read_system`]).
            FontFile::RcXml { wm } => *wm == TitleWm::Labwc,
        }
    }

    /// The font string from the file text, in a form [`parse_font_spec_face`]
    /// understands (empty means not set).
    pub fn read_spec(&self, text: &str) -> String {
        match self {
            FontFile::CosmicTk { .. } => cosmic_font_family(text),
            FontFile::QtIni { group, key, .. } => ini_value(text, group, key)
                .map(|v| v.trim_matches('"').to_string())
                .unwrap_or_default(),
            // The style in Pango words («Noto Sans Bold Italic 9»), as for rc.xml: the
            // string is read by [`parse_font_spec_face`].
            FontFile::GtkCss { target, .. } | FontFile::QtCtQss { target, .. } => extra_fonts_parse(text)
                .into_iter()
                .find(|f| f.target == *target)
                .map(|f| font_face_spec(&f.family, f.size, f.face))
                .unwrap_or_default(),
            // The style in Pango words («Sans Bold 10»): the string is read by
            // [`parse_font_spec_face`].
            FontFile::RcXml { .. } => rc_xml_title_font_face(text)
                .map(|(family, size, face)| font_face_spec(&family, size, face))
                .unwrap_or_default(),
            FontFile::PcmanfmDesktop => pcmanfm_desktop_font_raw(text)
                .map(|v| v.trim_matches('"').to_string())
                .unwrap_or_default(),
            FontFile::SwayConf => sway_font_value(text)
                .map(|desc| {
                    let (family, size, face) = parse_pango_font_desc(&desc);
                    font_face_spec(&family, size, face)
                })
                .unwrap_or_default(),
        }
    }

    /// The new file text with this font. `existing = None` means no file. If the
    /// file must not be touched (our block in `gtk.css` is broken by hand), the
    /// previous text is returned as is.
    pub fn new_text(&self, existing: Option<&str>, family: &str, size: i32) -> String {
        self.try_new_text(existing, family, size)
            .unwrap_or_else(|| existing.unwrap_or_default().to_string())
    }

    /// The new file text; `None` means the file must not be touched.
    pub fn try_new_text(&self, existing: Option<&str>, family: &str, size: i32) -> Option<String> {
        self.try_new_text_face(existing, family, size, FontFace::default())
    }

    /// The same with the style from the picker (weight, slant): it is kept by
    /// COSMIC's `FontConfig`, a QFont record (LXQt, qt6ct), the `rc.xml` of
    /// Openbox and labwc (`<weight>`, `<slant>`), the sway `font` line and our
    /// block in `gtk.css`/`.qss` (the mark line and `font-weight`/`font-style`,
    /// [`extra_fonts_block`]). If no style is chosen ([`FontFace::default`]), the
    /// block keeps the previous one, as `rc.xml` does.
    pub fn try_new_text_face(&self, existing: Option<&str>, family: &str, size: i32, face: FontFace) -> Option<String> {
        match self {
            FontFile::CosmicTk { .. } => Some(cosmic_font_ron_face(family, face)),
            FontFile::QtIni { group, key, .. } => Some(crate::gtk_sanitizer::write_ini_setting_in(
                existing,
                group,
                key,
                &format!("\"{}\"", qt_font_spec_face(family, size, face)),
            )),
            FontFile::GtkCss { version, target } => {
                let dialect = if version == "4.0" { ExtraFontDialect::Gtk4 } else { ExtraFontDialect::Gtk3 };
                let text = existing.unwrap_or_default();
                let fonts = with_extra_font(extra_fonts_parse(text), target, family, size, face);
                text_with_extra_fonts_block(existing, &extra_fonts_block(dialect, &fonts))
            }
            FontFile::QtCtQss { target, .. } => {
                // The file is entirely ours: it has only the block.
                let fonts = with_extra_font(extra_fonts_parse(existing.unwrap_or_default()), target, family, size, face);
                Some(extra_fonts_block(ExtraFontDialect::Qss, &fonts))
            }
            FontFile::RcXml { wm } => rc_xml_with_title_font_face(existing, *wm, family, size, face),
            FontFile::SwayConf => Some(sway_title_font_content(family, size, face)),
            FontFile::PcmanfmDesktop => {
                Some(pcmanfm_with_desktop_font(existing, &pcmanfm_font_value(family, size, face)))
            }
        }
    }

    /// All file writes a font write into this storage requires. `None` means
    /// writing is not allowed: the file is missing and must not be created (qt6ct
    /// is not set up, its `.conf` is missing), or our block is broken by hand.
    ///
    /// * A `gtk.css` link to a foreign theme is not edited through the link: in
    ///   its place a regular file appears that includes the same theme
    ///   (`@import url("file://…")`), followed by our block.
    /// * qt6ct/qt5ct: next to the `.qss` itself, its path is entered into
    ///   `[Interface] stylesheets` (that is how their own «Style Sheets» page
    ///   includes it); foreign stylesheets in the list stay.
    pub fn file_writes(
        &self,
        config_home: &str,
        io: &dyn FontFileIo,
        family: &str,
        size: i32,
    ) -> Option<Vec<FontFileWrite>> {
        self.file_writes_face(config_home, io, family, size, FontFace::default())
    }

    /// [`FontFile::file_writes`] with the style from the picker.
    pub fn file_writes_face(
        &self,
        config_home: &str,
        io: &dyn FontFileIo,
        family: &str,
        size: i32,
        face: FontFace,
    ) -> Option<Vec<FontFileWrite>> {
        let rel = self.rel_path();
        let mut out = Vec::new();
        match self {
            // One file is not enough: a block in the sway config checked by sway itself
            // is needed too; [`sway_title_font_apply`] does that.
            FontFile::SwayConf => return None,
            // First close the live desktop and make a backup;
            // [`pcmanfm_desktop_font_apply`] does that.
            FontFile::PcmanfmDesktop => return None,
            FontFile::QtCtQss { tool, .. } => {
                let conf_rel = format!("{tool}/{tool}.conf");
                let conf = io.read(&conf_rel)?;
                let existing = io.read(&rel);
                out.push(FontFileWrite {
                    text: self.try_new_text_face(existing.as_deref(), family, size, face)?,
                    rel_path: rel.clone(),
                    replace_link: false,
                });
                let qss_abs = format!("{}/{rel}", config_home.trim_end_matches('/'));
                if let Some(text) = qtct_conf_with_stylesheet(&conf, &qss_abs) {
                    out.push(FontFileWrite { rel_path: conf_rel, text, replace_link: false });
                }
            }
            FontFile::GtkCss { .. } => {
                let (existing, replace_link) = match io.link_target(&rel) {
                    Some(target) => (Some(format!("@import url(\"{}\");\n", css_file_url(&target))), true),
                    None => (io.read(&rel), false),
                };
                out.push(FontFileWrite {
                    text: self.try_new_text_face(existing.as_deref(), family, size, face)?,
                    rel_path: rel,
                    replace_link,
                });
            }
            _ => {
                let existing = io.read(&rel).or_else(|| match self {
                    FontFile::RcXml { .. } => io.read_system(&rel),
                    _ => None,
                });
                if existing.is_none() && !self.may_create() {
                    return None;
                }
                out.push(FontFileWrite {
                    text: self.try_new_text_face(existing.as_deref(), family, size, face)?,
                    rel_path: rel,
                    replace_link: false,
                });
            }
        }
        Some(out)
    }
}

// ── Shared layer of the "small / toolbar / menu" fonts ────────────────────
//
// Only KDE has a native key for these fonts. To make them work in any
// environment (the rule "a setting of one environment works in all"), Rexon
// itself puts them where programs take them from:
//
// * GTK 3 and GTK 4: the user stylesheet `gtk-<version>/gtk.css`: GTK
//   includes it with user priority (above the theme), and font properties in
//   it are inherited down the node tree. Nodes and classes follow the GTK
//   documentation (CSS node names of `GtkMenu`/`GtkMenuItem`/`GtkMenuBar`/
//   `GtkToolbar`/`GtkStatusbar`/`GtkPopoverMenu` in GTK 3; `menubar > item`,
//   `popover.menu`, `modelbutton` and the `.toolbar`, `.caption` classes in GTK 4).
//   `font-family` and `font-size` are written, not the `font` shorthand, so as
//   not to reset the weight the theme sets, for example, for `.caption-heading`;
//   `font-weight`/`font-style` only for an unusual style (bold, thin,
//   slanted); a regular one does not touch the theme's weight.
// * Qt widget programs outside KDE: the qt6ct/qt5ct stylesheet
//   (`[Interface] stylesheets`): the qt6ct platform theme reads it at program
//   start and puts it before the program's own stylesheet
//   (`Qt6CTPlatformTheme::applySettings`). The classes are the same ones Qt
//   hands the platform theme fonts out by: `QMenu`/`QMenuBar` (MenuFont),
//   `QToolButton` (ToolButtonFont); Qt has no separate class for the small
//   font, the `QStatusBar` status lines carry it.
// * KDE programs started with the Plasma platform theme outside Plasma:
//   `kdeglobals [General]` (read by `KDEPlasmaPlatformTheme6`).

/// Name of our stylesheet in the `qss` directory of qt6ct/qt5ct.
pub const QTCT_QSS_NAME: &str = "rexon-fonts.qss";

/// Qt configuration programs outside KDE that have their own stylesheets.
pub const QTCT_TOOLS: &[&str] = &["qt6ct", "qt5ct"];

const EXTRA_BLOCK_BEGIN: &str = "/* >>> Rexon: fonts (managed by Rexon, edits are overwritten) */";
const EXTRA_BLOCK_BEGIN_PREFIX: &str = "/* >>> Rexon: fonts";
const EXTRA_BLOCK_END: &str = "/* <<< Rexon: fonts */";
const EXTRA_ENTRY_PREFIX: &str = "/* rexon:font ";

/// Which stylesheet the block is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtraFontDialect {
    Gtk3,
    Gtk4,
    Qss,
}

/// Selectors of the font `target` in the stylesheet `dialect`.
pub fn extra_font_selectors(dialect: ExtraFontDialect, target: &str) -> &'static str {
    match (dialect, target) {
        (ExtraFontDialect::Gtk3, "small") => "statusbar, .caption, .caption-heading",
        (ExtraFontDialect::Gtk3, "toolbar") => "toolbar, toolbar button",
        (ExtraFontDialect::Gtk3, _) => {
            "menubar, menubar > menuitem, menu, menuitem, popover.menu, popover.menu modelbutton"
        }
        (ExtraFontDialect::Gtk4, "small") => ".caption, .caption-heading",
        (ExtraFontDialect::Gtk4, "toolbar") => ".toolbar, .toolbar button",
        (ExtraFontDialect::Gtk4, _) => "menubar, menubar > item, popover.menu, popover.menu modelbutton",
        (ExtraFontDialect::Qss, "small") => "QStatusBar, QStatusBar QLabel",
        (ExtraFontDialect::Qss, "toolbar") => "QToolButton",
        (ExtraFontDialect::Qss, _) => "QMenuBar, QMenu",
    }
}

/// One font of our block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraFont {
    pub target: String,
    pub family: String,
    pub size: i32,
    /// The style; a regular one (`weight` 0 or 400 without slant) is not written
    /// into the block.
    pub face: FontFace,
}

/// Whether the style is regular: weight not chosen or 400, no slant. Such a
/// style is not written into the block, so as not to override the weight the theme sets.
fn face_is_plain(face: FontFace) -> bool {
    (face.weight <= 0 || face.weight == 400) && !face.italic
}

/// Weight for CSS/QSS: a step 100–900; `None` means regular (do not write).
fn extra_font_css_weight(face: FontFace) -> Option<i32> {
    (face.weight > 0 && face.weight != 400).then(|| ((face.weight as f64 / 100.0).round() as i32).clamp(1, 9) * 100)
}

/// The style field in the mark line: `face=<weight>,<slant>`.
const EXTRA_FACE_PREFIX: &str = "face=";

/// A CSS string: quotes and backslashes are escaped, font names never contain
/// line breaks.
fn css_string(value: &str) -> String {
    let clean: String = value.chars().filter(|c| !c.is_control()).collect();
    format!("\"{}\"", clean.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A `file://` URL of a file for `@import url(...)`.
fn css_file_url(path: &str) -> String {
    let mut out = String::from("file://");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Our font block: for each font a mark line (the block is read back by it)
/// and a rule.
///
/// The mark is `/* rexon:font <target> <size> [face=<weight>,<slant>]
/// <family> */`; the style field exists only for an unusual style, and marks
/// without it (older versions wrote those) are read as regular. An unusual
/// style goes into the rule as the `font-weight` (step 100–900) and
/// `font-style: italic` properties: both GTK 3/4 CSS and Qt stylesheets understand them.
pub fn extra_fonts_block(dialect: ExtraFontDialect, fonts: &[ExtraFont]) -> String {
    let mut out = format!("{EXTRA_BLOCK_BEGIN}\n");
    for target in EXTRA_FONT_TARGETS {
        let Some(f) = fonts.iter().find(|f| f.target == *target) else { continue };
        let label: String = f.family.chars().filter(|c| !c.is_control()).collect::<String>().replace("*/", "* /");
        let face_label = if face_is_plain(f.face) {
            String::new()
        } else {
            format!("{EXTRA_FACE_PREFIX}{},{} ", f.face.weight.max(0), i32::from(f.face.italic))
        };
        out.push_str(&format!("{EXTRA_ENTRY_PREFIX}{target} {} {face_label}{label} */\n", f.size));
        let mut face_rules = String::new();
        if let Some(weight) = extra_font_css_weight(f.face) {
            face_rules.push_str(&format!("  font-weight: {weight};\n"));
        }
        if f.face.italic {
            face_rules.push_str("  font-style: italic;\n");
        }
        out.push_str(&format!(
            "{} {{\n  font-family: {};\n  font-size: {}pt;\n{face_rules}}}\n",
            extra_font_selectors(dialect, target),
            css_string(&f.family),
            f.size
        ));
    }
    out.push_str(EXTRA_BLOCK_END);
    out.push('\n');
    out
}

/// The fonts of our block in the stylesheet text (nothing outside the block is
/// read: foreign rules are not ours).
pub fn extra_fonts_parse(text: &str) -> Vec<ExtraFont> {
    let Some(begin) = text.find(EXTRA_BLOCK_BEGIN_PREFIX) else { return Vec::new() };
    let rest = &text[begin..];
    let block = match rest.find(EXTRA_BLOCK_END) {
        Some(end) => &rest[..end],
        None => rest,
    };
    let mut out: Vec<ExtraFont> = Vec::new();
    for line in block.lines() {
        let Some(entry) = line.trim().strip_prefix(EXTRA_ENTRY_PREFIX) else { continue };
        let entry = entry.trim_end_matches("*/").trim();
        let mut parts = entry.splitn(3, ' ');
        let (Some(target), Some(size), Some(family)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let (Some(target), Ok(size)) = (EXTRA_FONT_TARGETS.iter().find(|t| **t == target), size.parse::<i32>())
        else {
            continue;
        };
        // The style is an optional field before the family (marks of older versions
        // do not have it).
        let family = family.trim();
        let (face, family) = match family.strip_prefix(EXTRA_FACE_PREFIX).and_then(|r| r.split_once(' ')) {
            Some((spec, rest)) => match spec.split_once(',') {
                Some((w, i)) if w.parse::<i32>().is_ok() && matches!(i, "0" | "1") => (
                    FontFace { weight: w.parse::<i32>().unwrap_or(0).clamp(0, 1000), italic: i == "1" },
                    rest.trim(),
                ),
                _ => (FontFace::default(), family),
            },
            None => (FontFace::default(), family),
        };
        out.retain(|f| f.target != *target);
        out.push(ExtraFont { target: target.to_string(), family: family.to_string(), size, face });
    }
    out
}

/// The block's fonts with a new font for the target `target`. If no style is
/// chosen ([`FontFace::default`], old calls without the picker), the previous
/// style of this target stays.
fn with_extra_font(mut fonts: Vec<ExtraFont>, target: &str, family: &str, size: i32, face: FontFace) -> Vec<ExtraFont> {
    let face = if face == FontFace::default() {
        fonts.iter().find(|f| f.target == target).map(|f| f.face).unwrap_or_default()
    } else {
        face
    };
    fonts.retain(|f| f.target != target);
    fonts.push(ExtraFont { target: target.to_string(), family: family.to_string(), size, face });
    fonts
}

/// The stylesheet text with our block: our previous block is removed, the new
/// one goes at the end (later means stronger at equal weight); foreign rules
/// and `@import` stay in place. `None` means the block start is there but the
/// end is not (edited by hand): guessing where ours ends is not allowed.
pub fn text_with_extra_fonts_block(existing: Option<&str>, block: &str) -> Option<String> {
    let mut body = existing.unwrap_or_default().to_string();
    if let Some(begin) = body.find(EXTRA_BLOCK_BEGIN_PREFIX) {
        let end_rel = body[begin..].find(EXTRA_BLOCK_END)?;
        let mut end = begin + end_rel + EXTRA_BLOCK_END.len();
        if body[end..].starts_with('\n') {
            end += 1;
        }
        body.replace_range(begin..end, "");
    }
    let body = body.trim_end();
    Some(if body.is_empty() { block.to_string() } else { format!("{body}\n\n{block}") })
}

/// Lines of the qt6ct `stylesheets=` list (QSettings notation: comma-separated,
/// entries containing a comma quoted; an empty list is `@Invalid()`).
fn qsettings_list(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "@Invalid()" {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ',' if !quoted => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out.retain(|s| !s.is_empty());
    out
}

fn qsettings_list_value(items: &[String]) -> String {
    items
        .iter()
        .map(|s| {
            if s.contains(',') || s.contains('"') {
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                s.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Text of `qt6ct.conf` with our stylesheet in `[Interface] stylesheets`; `None`
/// means it is already there. Foreign stylesheets in the list stay, ours goes
/// last (later means stronger).
pub fn qtct_conf_with_stylesheet(conf: &str, qss_path: &str) -> Option<String> {
    let mut list = qsettings_list(&ini_value(conf, "Interface", "stylesheets").unwrap_or_default());
    if list.iter().any(|p| p == qss_path) {
        return None;
    }
    list.push(qss_path.to_string());
    Some(crate::gtk_sanitizer::write_ini_setting_in(
        Some(conf),
        "Interface",
        "stylesheets",
        &qsettings_list_value(&list),
    ))
}

/// Stylesheet paths from `qt6ct.conf`, for checking.
pub fn qtct_conf_stylesheets(conf: &str) -> Vec<String> {
    qsettings_list(&ini_value(conf, "Interface", "stylesheets").unwrap_or_default())
}

/// What "small", "toolbar" and "menu" affect in environment `de`: one line
/// under the font buttons.
pub fn extra_fonts_scope_note(de: &str) -> String {
    let mut text = match de {
        "kde" => tr("Small, toolbar and menu fonts also reach GTK apps: Rexon writes them to its own block \
                     in gtk.css."),
        "lxqt" => tr("LXQt has no small, toolbar and menu fonts of its own, so Rexon applies them to GTK apps \
                      (its own block in gtk.css). LXQt's Qt apps don't take them: LXQt passes them only the \
                      interface and monospace fonts."),
        "cosmic" => tr("COSMIC has no small, toolbar and menu fonts of its own, so Rexon applies them to GTK \
                        apps (its own block in gtk.css) and to Qt apps styled by qt6ct or qt5ct. In COSMIC apps \
                        only the interface and monospace fonts change, without the size: text size there \
                        follows the interface density."),
        _ => tr("This desktop has no small, toolbar and menu fonts of its own, so Rexon applies them to GTK \
                 apps (its own block in gtk.css) and to Qt apps styled by qt6ct or qt5ct."),
    };
    match de {
        "gnome" => {
            text.push(' ');
            text.push_str(&tr("The panel and menus of GNOME Shell itself don't take them."));
        }
        "cinnamon" | "cinnamon-mint" => {
            text.push(' ');
            text.push_str(&tr("The panel and menus of Cinnamon itself don't take them."));
        }
        _ => {}
    }
    text.push(' ');
    text.push_str(&tr("Electron apps and browsers don't take them either; open apps pick the change up after a \
                       restart."));
    text
}

/// One font storage: a command or a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontStore {
    Cmd(SoundBackend),
    File(FontFile),
}

/// The full plan of changing a font in an environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontPlan {
    /// The environment's native storage: the current font is read from it, and
    /// success is decided by the write into it.
    pub primary: FontStore,
    /// Auxiliary storages of the same font (best-effort): so that GTK and Qt
    /// programs that do not read the environment's native key see it.
    pub also: Vec<FontStore>,
    /// Ready companion commands (best-effort), for example "window titles not in
    /// the system font": without it the chosen title font silently has no effect.
    pub extra: Vec<Vec<String>>,
    /// Commands AFTER the write (best-effort): the window manager rereads its
    /// settings (`openbox --reconfigure`, `labwc --reconfigure`).
    pub notify: Vec<Vec<String>>,
    /// Whether to carry the font into `gtk-{3,4}.0/settings.ini`.
    pub gtk_ini: bool,
}

/// Environments whose window manager draws titles with the font from the
/// setting [`font_backend`] writes. In the others (COSMIC, where programs draw
/// titles themselves with the interface font; Hyprland/niri, which have no
/// titles) there is nowhere to write the title font, and the button is not
/// shown instead of pretending to apply it ([`title_font_note`] says why). In
/// LXQt, labwc and sway titles are drawn by the session manager: [`title_wm_for`], [`font_plan_wm`].
const TITLE_FONT_DES: &[&str] =
    &["kde", "xfce", "gnome", "budgie", "pantheon", "cinnamon", "cinnamon-mint", "mate"];

/// The window manager that draws window titles where the environment has no
/// manager of its own: in LXQt (the manager is chosen in «Session Settings»)
/// and in bare labwc and sway sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleWm {
    Openbox,
    Labwc,
    Xfwm4,
    Marco,
    KWin,
    /// sway: the `font` line in our file, included with a managed block into its
    /// config ([`sway_title_font_steps`]).
    Sway,
}

impl TitleWm {
    /// Program name (for the `--reconfigure` command).
    pub fn program(self) -> &'static str {
        match self {
            TitleWm::Openbox => "openbox",
            TitleWm::Labwc => "labwc",
            TitleWm::Xfwm4 => "xfwm4",
            TitleWm::Marco => "marco",
            TitleWm::KWin => "kwin",
            TitleWm::Sway => "sway",
        }
    }

    /// `rc.xml` relative to `$XDG_CONFIG_HOME`, for Openbox and labwc.
    ///
    /// * Openbox in LXQt since 0.17 reads the ordinary `openbox/rc.xml`:
    ///   `lxqt-session` starts the manager from `window_manager` without
    ///   `--config-file` (`LXQtModuleManager::startWm`), the separate
    ///   `lxqt-rc.xml` is a thing of the past.
    /// * labwc in LXQt Wayland: `labwc/rc.xml`: `startlxqtwayland` puts its sample
    ///   there if the directory is missing and starts labwc with the defaults
    ///   (labwc-config(5)).
    pub fn rc_xml_path(self) -> Option<&'static str> {
        match self {
            TitleWm::Openbox => Some("openbox/rc.xml"),
            TitleWm::Labwc => Some("labwc/rc.xml"),
            _ => None,
        }
    }

    /// The root element of `rc.xml`.
    fn rc_xml_root(self) -> &'static str {
        if self == TitleWm::Labwc { "labwc_config" } else { "openbox_config" }
    }
}

/// Who draws titles in this session, for environments without a window
/// manager of their own. `lxqt_session_confs` are the texts of
/// `lxqt/session.conf` by precedence (the own one, then the system ones from
/// `$XDG_CONFIG_DIRS`): the first one where `[General] window_manager` on X11
/// or `compositor` on Wayland is set applies (`lxqt-config-session` writes
/// them; the value may be a full path or have options, the program name is
/// taken). Not set or an unknown manager gives `None`: no title font is offered.
pub fn title_wm_for(de: &str, lxqt_session_confs: &[&str], wayland: bool) -> Option<TitleWm> {
    match de {
        "labwc" => Some(TitleWm::Labwc),
        "sway" => Some(TitleWm::Sway),
        "lxqt" => {
            let key = if wayland { "compositor" } else { "window_manager" };
            let value = lxqt_session_confs.iter().find_map(|t| ini_value(t, "General", key))?;
            let program = value.split_whitespace().next()?.rsplit('/').next()?.to_ascii_lowercase();
            match program.as_str() {
                "openbox" if !wayland => Some(TitleWm::Openbox),
                "labwc" if wayland => Some(TitleWm::Labwc),
                "xfwm4" if !wayland => Some(TitleWm::Xfwm4),
                "marco" if !wayland => Some(TitleWm::Marco),
                "kwin_x11" | "kwin" if !wayland => Some(TitleWm::KWin),
                "kwin_wayland" if wayland => Some(TitleWm::KWin),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Why there is no title font button in this environment: one phrase for the
/// explanation under the font buttons; `None` means the button exists.
pub fn title_font_note(de: &str, wm: Option<TitleWm>) -> Option<String> {
    if font_target_available_wm(de, "title", wm) {
        return None;
    }
    Some(match de {
        "lxqt" => tr("The title bar font in LXQt belongs to its window manager: Rexon changes it for Openbox, \
                      labwc, xfwm4, Marco and KWin, and this session uses another one (or none is chosen in \
                      Session Settings)."),
        "cosmic" => tr("COSMIC has no separate title bar font: title bars use the interface font."),
        "hyprland" | "niri" | "river" => tr("This window manager draws no title bars of its own (only with \
                                              plugins, which Rexon doesn't configure)."),
        "deepin" => tr("Deepin has no title bar font setting."),
        _ => tr("Rexon doesn't know where this desktop keeps its title bar font."),
    })
}

/// What the document font affects: one phrase for the explanation under the
/// font buttons, in all environments: only individual GTK programs read the
/// key ([`DOCUMENT_FONT_GNOME`]).
pub fn document_font_note() -> String {
    tr("The document font is used by GNOME and GTK apps that show documents: document viewers, mail, the \
        GNOME web browser. KDE and Qt apps don't use it.")
}

/// Why the desktop icon caption font is disabled or not set entirely: a phrase
/// for the explanation; `None` means it is set fully (Cinnamon, MATE).
pub fn desktop_font_note(de: &str) -> Option<String> {
    Some(match de {
        "cinnamon" | "cinnamon-mint" | "mate" | "lxqt" => return None,
        "xfce" => tr("In XFCE only the size of desktop icon labels can be set: the typeface comes from the \
                      interface font."),
        "kde" => tr("KDE Plasma has no separate font for desktop icon labels: they use the interface font."),
        _ => tr("The font of desktop icon labels can't be set in this desktop: it has no such setting."),
    })
}

/// argv of reading the xfdesktop own-size switch
/// ([`XFDESKTOP_USE_SIZE_PROP`]).
pub fn xfdesktop_use_size_read_argv() -> Vec<String> {
    sound_read_argv(
        &SoundBackend::Xfconf { channel: XFDESKTOP_CHANNEL.to_string(), prop: XFDESKTOP_USE_SIZE_PROP.to_string() },
        "",
    )
    .expect("xfconf читается без kde_tool")
}

/// The effective xfdesktop icon caption font as a Pango description: family
/// and style from the interface font (`interface`, the xsettings Pango
/// description), the size its own (`size`, an xfconf answer like
/// "12.000000") if `use_custom` is on ("true"). Otherwise xfdesktop takes the
/// interface font entirely, and that is what is shown.
pub fn xfdesktop_font_spec(use_custom: &str, size: &str, interface: &str) -> String {
    let interface = interface.trim().trim_matches(['\'', '"']).trim();
    let size = size.trim().parse::<f64>().ok().filter(|s| s.is_finite() && *s > 0.0);
    match (use_custom.trim() == "true", size) {
        (true, Some(size)) => {
            let (family, _, face) = parse_font_spec_face(interface);
            if family.is_empty() {
                return String::new();
            }
            pango_font_desc(&family, size.round() as i32, face)
        }
        _ => interface.to_string(),
    }
}

// ── LXQt desktop icon captions (pcmanfm-qt) ──────────────────────────────
//
// The LXQt desktop is drawn by pcmanfm-qt (`pcmanfm-qt --desktop --profile=lxqt`,
// autostart `lxqt-desktop.desktop`, a `lxqt-session` module). The icon caption
// font is `[Desktop] Font` in `<profile>/settings.conf`: a quoted
// `QFont::toString` record (`Font="Sans Serif,12,-1,5,400,0,…"`). From the
// pcmanfm-qt 2.5 sources (`settings.cpp`, `application.cpp`):
//
// * `Settings::profileDir(profile, useFallback)`: if the DIRECTORY
//   `$XDG_CONFIG_HOME/pcmanfm-qt/lxqt` exists, only it is read, even without a
//   `settings.conf` inside (then the built-in defaults apply: no wallpaper,
//   the interface font). Without the directory, the first existing
//   `<dir>/pcmanfm-qt/lxqt` from `QStandardPaths::standardLocations(ConfigLocation)`,
//   that is `$XDG_CONFIG_DIRS`. The package puts the system file at
//   `/usr/share/pcmanfm-qt/lxqt/settings.conf` (`startlxqt` appends `/usr/share`
//   to `$XDG_CONFIG_DIRS`), with wallpaper, shortcuts, colors in it.
//   Files are not merged: the own directory shadows the system one entirely.
// * `Settings::save` writes ALL values into the own directory and is called on
//   exit (`aboutToQuit`; on SIGTERM its own handler, also through the exit).
//   pcmanfm-qt cannot reread the file on the fly: an edit while the desktop
//   is live is not visible, and on its exit it is overwritten by what is in its memory.
//
// Hence the protection: the live desktop is closed first (D-Bus `quit` on
// `org.pcmanfm.PCManFM`, it saves its settings entirely by itself), then the
// single `Font` line is edited (the other bytes stay as they were), then the
// desktop is started again (`startModule` of `lxqt-session`). If there is no
// own file, it is created as a copy of what pcmanfm-qt read before us (the
// system one), not from scratch: created with just a font, it would leave the
// desktop without wallpaper. Before the first edit, a backup and an "as it was"
// record ([`PcmanfmFontRecord`]); restoring puts back the previous `Font` line
// or removes the file we created together with the directories.

/// pcmanfm-qt profile of the LXQt desktop (relative to the config directory).
pub const PCMANFM_PROFILE_REL: &str = "pcmanfm-qt/lxqt";
/// D-Bus name of the live pcmanfm-qt (`application.cpp`, `serviceName`).
pub const PCMANFM_BUS_NAME: &str = "org.pcmanfm.PCManFM";
/// The desktop module in `lxqt-session` (the pcmanfm-qt autostart file).
pub const LXQT_DESKTOP_MODULE: &str = "lxqt-desktop.desktop";
/// D-Bus name of `lxqt-session` (`sessionapplication.cpp`).
pub const LXQT_SESSION_BUS_NAME: &str = "org.lxqt.session";

/// Where the pcmanfm-qt desktop settings and the Rexon backup live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmanfmPaths {
    /// `$XDG_CONFIG_HOME`.
    pub config_home: String,
    /// System directories by precedence ([`pcmanfm_config_dirs`]).
    pub config_dirs: Vec<String>,
    /// Backup directory ([`pcmanfm_backup_dir`]).
    pub backup_dir: String,
}

/// System pcmanfm-qt config directories: `$XDG_CONFIG_DIRS`, then `/etc/xdg`
/// and `/usr/share`: packages put them there, and `startlxqt` appends the
/// same, even if Rexon runs with a trimmed environment.
pub fn pcmanfm_config_dirs(xdg_config_dirs: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in xdg_config_dirs.split(':').chain(["/etc/xdg", "/usr/share"]) {
        let d = d.trim_end_matches('/');
        if !d.is_empty() && !out.iter().any(|o| o == d) {
            out.push(d.to_string());
        }
    }
    out
}

/// Backup directory of the LXQt desktop settings.
pub fn pcmanfm_backup_dir(data_home: &str) -> String {
    format!("{}/rexon/backups/lxqt-desktop-font", data_home.trim_end_matches('/'))
}

impl PcmanfmPaths {
    /// From the process environment.
    pub fn from_env() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let xdg_data = std::env::var("XDG_DATA_HOME").ok();
        PcmanfmPaths {
            config_home: crate::utils::config_home_env(),
            config_dirs: pcmanfm_config_dirs(&std::env::var("XDG_CONFIG_DIRS").unwrap_or_default()),
            backup_dir: pcmanfm_backup_dir(&crate::utils::data_home(xdg_data.as_deref(), &home)),
        }
    }

    /// The own profile directory.
    pub fn own_dir(&self) -> String {
        format!("{}/{PCMANFM_PROFILE_REL}", self.config_home.trim_end_matches('/'))
    }

    /// The own `settings.conf`.
    pub fn own_file(&self) -> String {
        format!("{}/settings.conf", self.own_dir())
    }

    fn record_path(&self) -> String {
        format!("{}/record.json", self.backup_dir.trim_end_matches('/'))
    }
}

/// Where pcmanfm-qt takes the desktop settings from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PcmanfmSource {
    /// The own directory exists (it may have no file in it).
    Own,
    /// No own directory: the system `settings.conf` at this path.
    System(String),
    /// Neither an own nor a system directory: built-in defaults.
    Builtin,
}

/// What pcmanfm-qt will read at start: the source and the text (`None` means
/// no file, the built-in defaults apply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmanfmSeed {
    pub source: PcmanfmSource,
    pub text: Option<String>,
}

/// Port of `Settings::profileDir(profile, true)`: the own directory shadows
/// the system ones entirely.
pub fn pcmanfm_seed(paths: &PcmanfmPaths) -> PcmanfmSeed {
    if Path::new(&paths.own_dir()).is_dir() {
        return PcmanfmSeed { source: PcmanfmSource::Own, text: fs::read_to_string(paths.own_file()).ok() };
    }
    pcmanfm_system_seed(paths)
}

/// What pcmanfm-qt would read without the own directory: the first existing
/// system profile directory.
fn pcmanfm_system_seed(paths: &PcmanfmPaths) -> PcmanfmSeed {
    for dir in &paths.config_dirs {
        let profile = format!("{}/{PCMANFM_PROFILE_REL}", dir.trim_end_matches('/'));
        if Path::new(&profile).is_dir() {
            let file = format!("{profile}/settings.conf");
            return PcmanfmSeed { text: fs::read_to_string(&file).ok(), source: PcmanfmSource::System(file) };
        }
    }
    PcmanfmSeed { source: PcmanfmSource::Builtin, text: None }
}

/// Lines with their endings: editing one line leaves the other bytes as they
/// were (both `\r\n` and a missing `\n` at the end).
fn ini_raw_lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn ini_raw_body(line: &str) -> &str {
    line.trim_end_matches(['\n', '\r'])
}

fn ini_raw_section(line: &str) -> Option<&str> {
    let t = ini_raw_body(line).trim();
    (t.starts_with('[') && t.ends_with(']')).then(|| &t[1..t.len() - 1])
}

fn ini_raw_key(line: &str) -> Option<&str> {
    ini_raw_body(line).split_once('=').map(|(k, _)| k.trim())
}

/// The raw `[Desktop] Font` value (as in the file, with quotes); `None` means no key.
pub fn pcmanfm_desktop_font_raw(text: &str) -> Option<String> {
    let mut in_desktop = false;
    for line in ini_raw_lines(text) {
        if let Some(name) = ini_raw_section(line) {
            in_desktop = name == "Desktop";
            continue;
        }
        if in_desktop && ini_raw_key(line) == Some("Font") {
            return ini_raw_body(line).split_once('=').map(|(_, v)| v.trim().to_string());
        }
    }
    None
}

/// Text of `settings.conf` with `[Desktop] Font=<raw>`: one line changes, the
/// other bytes stay as they were. Without the key the line goes right after
/// the `[Desktop]` header; without the section it is appended at the end.
pub fn pcmanfm_with_desktop_font(existing: Option<&str>, raw: &str) -> String {
    let text = existing.unwrap_or_default();
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let new_line = format!("Font={raw}");
    let mut out = String::with_capacity(text.len() + new_line.len() + 16);
    let mut in_desktop = false;
    let mut after_header: Option<usize> = None;
    let mut replaced = false;
    for line in ini_raw_lines(text) {
        if let Some(name) = ini_raw_section(line) {
            in_desktop = name == "Desktop";
            out.push_str(line);
            if in_desktop && after_header.is_none() {
                if !line.ends_with('\n') {
                    out.push_str(eol);
                }
                after_header = Some(out.len());
            }
            continue;
        }
        if in_desktop && !replaced && ini_raw_key(line) == Some("Font") {
            out.push_str(&new_line);
            out.push_str(&line[ini_raw_body(line).len()..]);
            replaced = true;
            continue;
        }
        out.push_str(line);
    }
    if replaced {
        return out;
    }
    match after_header {
        Some(at) => out.insert_str(at, &format!("{new_line}{eol}")),
        None => {
            if !out.is_empty() {
                if !out.ends_with('\n') {
                    out.push_str(eol);
                }
                out.push_str(eol);
            }
            out.push_str(&format!("[Desktop]{eol}{new_line}{eol}"));
        }
    }
    out
}

/// The text without the `[Desktop] Font` line (pcmanfm-qt then takes the
/// interface font); the other bytes stay as they were.
pub fn pcmanfm_without_desktop_font(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_desktop = false;
    for line in ini_raw_lines(text) {
        if let Some(name) = ini_raw_section(line) {
            in_desktop = name == "Desktop";
        } else if in_desktop && ini_raw_key(line) == Some("Font") {
            continue;
        }
        out.push_str(line);
    }
    out
}

/// The `Font` value in pcmanfm-qt notation: a quoted `QFont::toString`; without
/// quotes QSettings reads the commas as a list, and the font is lost.
pub fn pcmanfm_font_value(family: &str, size: i32, face: FontFace) -> String {
    format!("\"{}\"", qt_font_spec_face(family, size, face))
}

/// What was there before Rexon's first edit, for restoring
/// ([`pcmanfm_desktop_font_restore`]): `record.json` in [`pcmanfm_backup_dir`]
/// next to the copy of the own `settings.conf` (if there was one).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PcmanfmFontRecord {
    /// Whether there was an own `settings.conf`.
    pub had_file: bool,
    /// The `Font` line pcmanfm-qt read before us (from the own or the system
    /// file); `None` means there was no key.
    pub font: Option<String>,
    /// Directories Rexon created itself (removed on restore if empty).
    pub created_dirs: Vec<String>,
    /// The last text Rexon wrote: if it matches on restore, the file is untouched
    /// since our edit and can be removed entirely.
    pub written: String,
}

/// The "as it was" record, if Rexon has already edited the desktop font.
pub fn pcmanfm_font_record(paths: &PcmanfmPaths) -> Option<PcmanfmFontRecord> {
    serde_json::from_str(&fs::read_to_string(paths.record_path()).ok()?).ok()
}

fn pcmanfm_save_record(paths: &PcmanfmPaths, record: &PcmanfmFontRecord) -> Result<(), String> {
    fs::create_dir_all(&paths.backup_dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
    crate::utils::atomic_write_str_err(&paths.record_path(), &text)
}

/// The live pcmanfm-qt desktop (a fake in the checks).
pub trait PcmanfmBus {
    /// Whether pcmanfm-qt is running (whether its D-Bus name is taken).
    fn running(&self) -> bool;
    /// Ask it to quit (it saves its settings itself) → whether the request was sent.
    fn quit(&self) -> bool;
    /// Start the desktop again → whether it worked.
    fn start(&self) -> bool;
    /// Pause between checks whether it has closed.
    fn pause(&self);
}

/// Result of applying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmanfmApplied {
    /// The desktop was running and has been started again: the font is already visible.
    pub restarted: bool,
}

/// How many [`PcmanfmBus::pause`] periods to wait for pcmanfm-qt to quit.
const PCMANFM_QUIT_POLLS: usize = 50;

/// Close the live desktop before the edit → whether it was running. If it did
/// not close, that is an error: the file must not be edited, it would overwrite it on exit.
fn pcmanfm_stop(bus: &dyn PcmanfmBus) -> Result<bool, String> {
    if !bus.running() {
        return Ok(false);
    }
    if bus.quit() {
        for _ in 0..PCMANFM_QUIT_POLLS {
            if !bus.running() {
                return Ok(true);
            }
            bus.pause();
        }
    }
    Err(tr("PCManFM-Qt, which draws the LXQt desktop, didn't close, so its settings file wasn't changed: on \
            exit it would have overwritten the change."))
}

fn pcmanfm_no_settings_text() -> String {
    tr("The font of desktop icon labels in LXQt can't be set: there is no PCManFM-Qt desktop settings file \
        (pcmanfm-qt/lxqt/settings.conf), neither yours nor a system one.")
}

/// Why the LXQt icon caption font cannot be set, before the picker; `None`
/// means it can. It cannot if pcmanfm-qt is not running and there are no
/// desktop settings, neither own nor system: creating them from scratch means guessing the defaults.
pub fn pcmanfm_refusal(paths: &PcmanfmPaths, bus: &dyn PcmanfmBus) -> Option<String> {
    (pcmanfm_seed(paths).source == PcmanfmSource::Builtin && !bus.running()).then(pcmanfm_no_settings_text)
}

/// Write the LXQt desktop icon caption font while protecting the desktop
/// settings: close the live pcmanfm-qt, make a backup on the first edit,
/// change the single `Font` line (without an own file, create it as a copy
/// of the system one), start the desktop again.
pub fn pcmanfm_desktop_font_apply(
    paths: &PcmanfmPaths,
    bus: &dyn PcmanfmBus,
    family: &str,
    size: i32,
    face: FontFace,
) -> Result<PcmanfmApplied, String> {
    let was_running = pcmanfm_stop(bus)?;
    let result = pcmanfm_write_font(paths, &pcmanfm_font_value(family, size, face));
    // The desktop is started again even if writing fails: we closed it.
    let restarted = was_running && bus.start();
    result.map(|()| PcmanfmApplied { restarted })
}

fn pcmanfm_backup_failed(detail: &str) -> String {
    trf("Couldn't save a backup copy of the LXQt desktop settings, so nothing was changed: %1", &[&detail])
}

fn pcmanfm_write_font(paths: &PcmanfmPaths, raw: &str) -> Result<(), String> {
    let seed = pcmanfm_seed(paths);
    if seed.source == PcmanfmSource::Builtin {
        return Err(pcmanfm_no_settings_text());
    }
    let own_file = paths.own_file();
    let mut record = match pcmanfm_font_record(paths) {
        Some(record) => record,
        None => {
            // First edit: a copy of the own file and the "as it was" record.
            let own_text = fs::read_to_string(&own_file).ok();
            if let Some(text) = &own_text {
                fs::create_dir_all(&paths.backup_dir).map_err(|e| pcmanfm_backup_failed(&e.to_string()))?;
                crate::utils::atomic_write_str_err(&format!("{}/settings.conf", paths.backup_dir), text)
                    .map_err(|e| pcmanfm_backup_failed(&e))?;
            }
            let app_dir = format!("{}/pcmanfm-qt", paths.config_home.trim_end_matches('/'));
            PcmanfmFontRecord {
                had_file: own_text.is_some(),
                font: seed.text.as_deref().and_then(pcmanfm_desktop_font_raw),
                created_dirs: [app_dir, paths.own_dir()].into_iter().filter(|d| !Path::new(d).is_dir()).collect(),
                written: String::new(),
            }
        }
    };
    let text = pcmanfm_with_desktop_font(seed.text.as_deref(), raw);
    record.written = text.clone();
    // The record comes before the edit: without it there would be nothing to restore with.
    pcmanfm_save_record(paths, &record).map_err(|e| pcmanfm_backup_failed(&e))?;
    fs::create_dir_all(paths.own_dir()).map_err(|e| e.to_string())?;
    crate::utils::atomic_write_str_err(&own_file, &text)
}

/// Restore the LXQt desktop icon caption font as it was before Rexon. A file
/// we created and that has been untouched since is removed together with the
/// created directories; otherwise the previous `Font` line is put into the
/// current file (or removed if there was none), and everything else, including
/// what the person changed after us, stays. `Ok(false)` means nothing to restore.
pub fn pcmanfm_desktop_font_restore(paths: &PcmanfmPaths, bus: &dyn PcmanfmBus) -> Result<bool, String> {
    let Some(record) = pcmanfm_font_record(paths) else {
        return Ok(false);
    };
    let was_running = pcmanfm_stop(bus)?;
    let result = pcmanfm_undo(paths, &record);
    if was_running {
        bus.start();
    }
    result?;
    let _ = fs::remove_dir_all(&paths.backup_dir);
    Ok(true)
}

fn pcmanfm_undo(paths: &PcmanfmPaths, record: &PcmanfmFontRecord) -> Result<(), String> {
    let own_file = paths.own_file();
    let Ok(current) = fs::read_to_string(&own_file) else {
        return Ok(());
    };
    if !record.had_file && current == record.written {
        fs::remove_file(&own_file).map_err(|e| e.to_string())?;
        // Deeper ones first. `remove_dir` does not remove a non-empty directory
        // (pcmanfm-qt put its own things there); then it shadows the system one, and
        // it needs the text pcmanfm-qt read before us.
        for dir in record.created_dirs.iter().rev() {
            let _ = fs::remove_dir(dir);
        }
        // The directory existed before us (empty) and stays so: pcmanfm-qt read the
        // built-in defaults and will read them again.
        let own_dir = paths.own_dir();
        if record.created_dirs.contains(&own_dir) && Path::new(&own_dir).is_dir() {
            if let Some(text) = pcmanfm_system_seed(paths).text {
                crate::utils::atomic_write_str_err(&own_file, &text)?;
            }
        }
        return Ok(());
    }
    let text = match &record.font {
        Some(raw) => pcmanfm_with_desktop_font(Some(&current), raw),
        None => pcmanfm_without_desktop_font(&current),
    };
    if text != current {
        crate::utils::atomic_write_str_err(&own_file, &text)?;
    }
    Ok(())
}

/// The icon caption font the LXQt desktop draws with now, as a QFont record;
/// empty means `Font` is not set (pcmanfm-qt takes the interface font).
pub fn pcmanfm_desktop_font_spec(paths: &PcmanfmPaths) -> String {
    pcmanfm_seed(paths)
        .text
        .as_deref()
        .and_then(pcmanfm_desktop_font_raw)
        .map(|v| v.trim_matches('"').to_string())
        .unwrap_or_default()
}

/// A phrase for the apply result: the desktop was restarted, so the font is
/// already visible; it was not running, so it applies when it starts.
pub fn pcmanfm_applied_note(applied: PcmanfmApplied) -> String {
    if applied.restarted {
        tr("To apply it, Rexon restarted the LXQt desktop (PCManFM-Qt); its open windows were closed.")
    } else {
        tr("It applies the next time the LXQt desktop starts.")
    }
}

/// The restore button: (caption, tooltip).
pub fn pcmanfm_restore_texts() -> (String, String) {
    (
        tr("↺ Desktop font as before"),
        tr("Return the font of LXQt desktop icon labels to what it was before Rexon changed it. The wallpaper and \
            other desktop settings stay as they are."),
    )
}

/// Result of restoring.
pub fn pcmanfm_restore_status(result: &Result<bool, String>) -> String {
    match result {
        Ok(true) => tr("The font of desktop icon labels is back to what it was."),
        Ok(false) => tr("Rexon didn't change the font of desktop icon labels: there's nothing to return."),
        Err(why) => trf("The font of desktop icon labels wasn't returned: %1", &[why]),
    }
}

fn pcmanfm_argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// argv of asking the bus whether the name `name` is taken (`NameHasOwner`).
pub fn dbus_name_has_owner_argv(name: &str) -> Vec<String> {
    pcmanfm_argv(&[
        "dbus-send",
        "--session",
        "--print-reply",
        "--reply-timeout=3000",
        "--dest=org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus.NameHasOwner",
        &format!("string:{name}"),
    ])
}

/// argv of quitting pcmanfm-qt (`org.pcmanfm.Application.quit`).
pub fn pcmanfm_quit_argv() -> Vec<String> {
    pcmanfm_argv(&[
        "dbus-send",
        "--session",
        "--print-reply",
        "--reply-timeout=5000",
        &format!("--dest={PCMANFM_BUS_NAME}"),
        "/Application",
        "org.pcmanfm.Application.quit",
    ])
}

/// argv of starting the desktop module in `lxqt-session`. `startModule` sends no
/// reply (`Q_NOREPLY`), hence no `--print-reply`: otherwise `dbus-send` would
/// wait for it until the timeout.
pub fn lxqt_start_desktop_argv() -> Vec<String> {
    pcmanfm_argv(&[
        "dbus-send",
        "--session",
        "--type=method_call",
        &format!("--dest={LXQT_SESSION_BUS_NAME}"),
        "/LXQtSession",
        "org.lxqt.session.startModule",
        &format!("string:{LXQT_DESKTOP_MODULE}"),
    ])
}

/// argv of starting the desktop without `lxqt-session`: the line from
/// `lxqt-desktop.desktop`, detached.
pub fn pcmanfm_desktop_spawn_argv() -> Vec<String> {
    pcmanfm_argv(&["setsid", "-f", "pcmanfm-qt", "--desktop", "--profile=lxqt"])
}

/// The live pcmanfm-qt through `dbus-send`.
pub struct LivePcmanfmBus;

fn pcmanfm_run(argv: &[String]) -> Option<std::process::Output> {
    let (exe, args) = argv.split_first()?;
    std::process::Command::new(exe).args(args).stdin(std::process::Stdio::null()).output().ok()
}

fn dbus_name_has_owner(name: &str) -> bool {
    pcmanfm_run(&dbus_name_has_owner_argv(name))
        .is_some_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).contains("boolean true"))
}

impl PcmanfmBus for LivePcmanfmBus {
    fn running(&self) -> bool {
        dbus_name_has_owner(PCMANFM_BUS_NAME)
    }
    fn quit(&self) -> bool {
        pcmanfm_run(&pcmanfm_quit_argv()).is_some_and(|o| o.status.success())
    }
    fn start(&self) -> bool {
        let argv = if dbus_name_has_owner(LXQT_SESSION_BUS_NAME) {
            lxqt_start_desktop_argv()
        } else {
            pcmanfm_desktop_spawn_argv()
        };
        // Output goes nowhere, not into pipes: `setsid -f` would leave them open in
        // the live desktop, and waiting for the end of output would never finish.
        let Some((exe, args)) = argv.split_first() else { return false };
        std::process::Command::new(exe)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    fn pause(&self) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

// ── sway title font ──────────────────────────────────────────────────────
//
// sway draws titles with the font from the `font [pango:]<Pango description>`
// line of its config (sway(5)); the last top-level line wins (`font` inside
// `bar { … }` is the bar font, not the title one). Rexon puts the line into
// its own file and includes it with a managed block at the END of the config,
// by the same mechanism as the accent color border (`crate::accent`).

/// Name of the managed title font block in the sway config.
pub const SWAY_FONT_TAG: &str = "title bar font";
/// Our title font file next to the sway config (without `.conf`).
pub const SWAY_FONT_STEM: &str = "rexon-title-font";
/// The sway title font if the config has none (sway(5), `font`).
pub const SWAY_DEFAULT_FONT: &str = "monospace 10";

/// Weight name in a Pango description (steps 100–900); regular has no word.
fn pango_weight_name(weight: i32) -> Option<&'static str> {
    if weight <= 0 {
        return None;
    }
    match ((weight as f64 / 100.0).round() as i32).clamp(1, 9) {
        1 => Some("Thin"),
        2 => Some("Ultra-Light"),
        3 => Some("Light"),
        5 => Some("Medium"),
        6 => Some("Semi-Bold"),
        7 => Some("Bold"),
        8 => Some("Ultra-Bold"),
        9 => Some("Heavy"),
        _ => None,
    }
}

/// A Pango font description "Family [weight] [Italic] size".
///
/// No comma separator of the family list is used: in sway commands a comma
/// separates commands, and `swaymsg font …` would cut off the size.
pub fn pango_font_desc(family: &str, size: i32, face: FontFace) -> String {
    let mut parts = vec![family.trim().to_string()];
    if let Some(weight) = pango_weight_name(face.weight) {
        parts.push(weight.to_string());
    }
    if face.italic {
        parts.push("Italic".to_string());
    }
    parts.push(size.max(1).to_string());
    parts.join(" ")
}

/// The font string for reading back ([`parse_font_spec_face`]) and for showing
/// to a person: a Pango description "Family [Style] Size" ([`pango_font_desc`]),
/// and without a size (`0` means not set) without a number, so as not to
/// invent size 1.
pub fn font_face_spec(family: &str, size: i32, face: FontFace) -> String {
    if size > 0 {
        return pango_font_desc(family, size, face);
    }
    let desc = pango_font_desc(family, 1, face);
    desc.strip_suffix(" 1").unwrap_or(&desc).to_string()
}

/// Weight by a style word (Pango, Qt style name): a step 100–900, `Some(0)`
/// means regular, `None` means the word is not about weight. The word is
/// lowercase and without hyphens.
fn face_weight_word(word: &str) -> Option<i32> {
    match word {
        "thin" | "hairline" => Some(100),
        "ultralight" | "extralight" => Some(200),
        "light" | "semilight" | "demilight" => Some(300),
        "book" | "regular" | "normal" => Some(0),
        "medium" => Some(500),
        "semibold" | "demibold" => Some(600),
        "bold" => Some(700),
        "ultrabold" | "extrabold" => Some(800),
        "heavy" | "black" | "ultrablack" | "ultraheavy" => Some(900),
        _ => None,
    }
}

/// A Pango description (with or without `pango:`, quoted or not) → family, size
/// and style. Style words at the end of the family are stripped; the first of
/// the family list is taken. Without a size, 10.
pub fn parse_pango_font_desc(desc: &str) -> (String, i32, FontFace) {
    let desc = desc.trim().trim_matches(['"', '\'']).trim();
    let desc = desc.strip_prefix("pango:").unwrap_or(desc);
    let mut words: Vec<&str> = desc.split_whitespace().collect();
    let mut size = 10;
    if let Some(last) = words.last() {
        if let Ok(v) = last.trim_end_matches("px").parse::<f64>() {
            size = (v.round() as i32).max(1);
            words.pop();
        }
    }
    let mut face = FontFace::default();
    while words.len() > 1 {
        let word = words[words.len() - 1].to_ascii_lowercase().replace('-', "");
        if let Some(w) = face_weight_word(&word) {
            if face.weight == 0 {
                face.weight = w;
            }
        } else if matches!(word.as_str(), "italic" | "oblique") {
            face.italic = true;
        } else if !matches!(
            word.as_str(),
            "roman" | "smallcaps" | "ultracondensed" | "extracondensed" | "condensed" | "semicondensed"
                | "semiexpanded" | "expanded" | "extraexpanded" | "ultraexpanded"
        ) {
            break;
        }
        words.pop();
    }
    let joined = words.join(" ");
    let family = joined.split(',').map(str::trim).find(|f| !f.is_empty()).unwrap_or("").to_string();
    (family, size, face)
}

/// The last top-level `font` value in the sway config text (`pango:` is left
/// as is, [`parse_pango_font_desc`] strips it). Lines inside `{ … }` blocks
/// (`bar`, `mode`, `output`) and comments are skipped.
pub fn sway_font_value(text: &str) -> Option<String> {
    let mut depth = 0i32;
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if depth == 0 {
            let mut words = line.split_whitespace();
            if words.next() == Some("font") {
                let rest: Vec<&str> = words.filter(|w| !w.starts_with("--")).collect();
                let value = rest.join(" ").trim_matches(['"', '\'']).trim().to_string();
                if !value.is_empty() {
                    found = Some(value);
                }
            }
        }
        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        depth = depth.max(0);
    }
    found
}

/// Our sway title font file.
pub fn sway_title_font_content(family: &str, size: i32, face: FontFace) -> String {
    format!(
        "# Rexon: title bar font. This file is rewritten by Rexon; change the title bar font in Rexon instead.\n\
         font pango:{}\n",
        pango_font_desc(family, size, face)
    )
}

/// Steps of writing the sway title font: our file and the block with an
/// `include` in the config ([`crate::animations::wm_include`]), then `swaymsg font …`.
///
/// Not `swaymsg reload`: after rereading the config sway restarts its swaybg on
/// top of the Rexon wallpaper (the accent reasons the same way). `font` is a
/// command of both the config and the live sway (sway(5)); if it is not
/// running, there is nobody to notify, which is not an error.
pub fn sway_title_font_steps(
    ctx: &crate::animations::LookCtx,
    family: &str,
    size: i32,
    face: FontFace,
) -> Vec<crate::animations::LookStep> {
    use crate::animations::{argv, wm_include, LookStep};
    let content = sway_title_font_content(family, size, face);
    let desc = pango_font_desc(family, size, face);
    vec![
        LookStep::Wm(wm_include(ctx, "sway", SWAY_FONT_TAG, SWAY_FONT_STEM, &move |_| content.clone())),
        LookStep::Notify(vec![argv(&["swaymsg", &format!("font pango:{desc}")])]),
    ]
}

/// Write the sway title font; `Err` is the reason for a person.
pub fn sway_title_font_apply(
    io: &dyn crate::animations::LookIo,
    ctx: &crate::animations::LookCtx,
    family: &str,
    size: i32,
    face: FontFace,
) -> Result<(), String> {
    crate::animations::run_steps(io, ctx, &sway_title_font_steps(ctx, family, size, face))
}

/// The title font sway draws with now, as a Pango description: the config is
/// looked up in sway's own order (sway(5): `~/.sway/config`,
/// `$XDG_CONFIG_HOME/sway/config`, then the i3 configs and the system one). If
/// it has our block, our file applies (the block is at the end), otherwise the
/// last `font` line; with neither, [`SWAY_DEFAULT_FONT`].
pub fn sway_title_font_current(io: &dyn crate::animations::LookIo, ctx: &crate::animations::LookCtx) -> String {
    let candidates = [
        format!("{}/.sway/config", ctx.home),
        ctx.cfg("sway/config"),
        format!("{}/.i3/config", ctx.home),
        ctx.cfg("i3/config"),
        "/etc/sway/config".to_string(),
        "/etc/i3/config".to_string(),
    ];
    let Some((path, text)) = candidates.iter().find_map(|p| io.read(p).map(|t| (p.clone(), t))) else {
        return SWAY_DEFAULT_FONT.to_string();
    };
    let targets = crate::animations::wm_targets(ctx, "sway", SWAY_FONT_STEM, &|_| String::new());
    let ours = targets
        .iter()
        .find(|t| t.config == path && text.lines().any(|l| l.trim() == t.include_line))
        .and_then(|t| io.read(&t.own_path))
        .and_then(|own| sway_font_value(&own));
    ours.or_else(|| sway_font_value(&text)).unwrap_or_else(|| SWAY_DEFAULT_FONT.to_string())
}

/// [`sway_title_font_current`] as a "Family [Style] size" string (as in GNOME;
/// [`parse_font_spec_face`] understands it).
pub fn sway_title_font_spec(io: &dyn crate::animations::LookIo, ctx: &crate::animations::LookCtx) -> String {
    let (family, size, face) = parse_pango_font_desc(&sway_title_font_current(io, ctx));
    font_face_spec(&family, size, face)
}

/// [`sway_title_font_spec`] on a live machine.
pub fn sway_title_font_spec_live() -> String {
    sway_title_font_spec(&crate::animations::SystemIo, &crate::animations::LookCtx::from_env())
}

/// [`sway_title_font_apply`] on a live machine → whether it worked.
pub fn sway_title_font_apply_live(family: &str, size: i32, face: FontFace) -> bool {
    sway_title_font_apply(&crate::animations::SystemIo, &crate::animations::LookCtx::from_env(), family, size, face)
        .is_ok()
}

/// The tag search must not find tags inside `<!-- … -->` comments: the same
/// bytes, but the comment contents (with delimiters) become spaces.
fn xml_mask_comments(text: &str) -> String {
    let mut bytes = text.as_bytes().to_vec();
    let mut from = 0;
    while let Some(start) = text[from..].find("<!--").map(|i| i + from) {
        let end = text[start + 4..].find("-->").map(|i| start + 4 + i + 3).unwrap_or(text.len());
        bytes[start..end].fill(b' ');
        from = end;
    }
    // Only whole comments were replaced, their delimiters are ASCII: UTF-8 stays intact.
    String::from_utf8(bytes).unwrap_or_else(|_| text.to_string())
}

/// Start of the tag `<name` (followed by a space, `>` or `/`) in `masked[from..to]`.
fn xml_find_tag(masked: &str, from: usize, to: usize, name: &str) -> Option<usize> {
    let open = format!("<{name}");
    let mut at = from;
    while let Some(i) = masked.get(at..to)?.find(&open) {
        let start = at + i;
        match masked.as_bytes().get(start + open.len()) {
            Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/') => return Some(start),
            _ => at = start + open.len(),
        }
    }
    None
}

/// Value of the `name` attribute in the opening tag `tag` (`<font place="…">`).
fn xml_attr(tag: &str, name: &str) -> Option<(usize, usize)> {
    let mut at = 0;
    while let Some(i) = tag[at..].find(name) {
        let start = at + i;
        let before_ok = start > 0 && tag.as_bytes()[start - 1].is_ascii_whitespace();
        let rest = tag[start + name.len()..].trim_start();
        if before_ok && rest.starts_with('=') {
            let after_eq = rest[1..].trim_start();
            let quote = after_eq.chars().next()?;
            if quote == '"' || quote == '\'' {
                let value_start = tag.len() - after_eq.len() + 1;
                let value_end = value_start + tag[value_start..].find(quote)?;
                return Some((value_start, value_end));
            }
        }
        at = start + name.len();
    }
    None
}

fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn xml_escape_text(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A `<font>` element in `<theme>`: bounds of the element and of the opening
/// tag, and the `place` value (empty means no `place`, for labwc "all places").
struct RcXmlFont {
    start: usize,
    tag_end: usize,
    end: usize,
    self_closing: bool,
    place: String,
}

/// Bounds of `<theme>…</theme>` (after the opening tag and the start of the
/// closing one) and all `<font>` elements in it.
fn rc_xml_theme_fonts(text: &str) -> Option<((usize, usize), Vec<RcXmlFont>)> {
    let masked = xml_mask_comments(text);
    let theme = xml_find_tag(&masked, 0, masked.len(), "theme")?;
    let open_end = theme + masked[theme..].find('>')? + 1;
    if masked.as_bytes()[open_end - 2] == b'/' {
        return None;
    }
    let close = open_end + masked[open_end..].find("</theme>")?;
    let mut fonts = Vec::new();
    let mut at = open_end;
    while let Some(start) = xml_find_tag(&masked, at, close, "font") {
        let tag_end = start + masked[start..].find('>')? + 1;
        let self_closing = masked.as_bytes()[tag_end - 2] == b'/';
        let end = if self_closing { tag_end } else { tag_end + masked[tag_end..close].find("</font>")? + "</font>".len() };
        let tag = &text[start..tag_end];
        let place = xml_attr(tag, "place").map(|(a, b)| tag[a..b].trim().to_string()).unwrap_or_default();
        fonts.push(RcXmlFont { start, tag_end, end, self_closing, place });
        at = end;
    }
    Some(((open_end, close), fonts))
}

/// Value of the field `field` of a `<font>` element: a child `<field>…</field>`
/// (Openbox, labwc) or an attribute (in labwc attributes and elements are
/// equivalent, labwc-config(5)).
fn rc_xml_font_field(text: &str, font: &RcXmlFont, field: &str) -> Option<String> {
    let tag = &text[font.start..font.tag_end];
    if let Some((a, b)) = xml_attr(tag, field) {
        return Some(xml_unescape(tag[a..b].trim()));
    }
    if font.self_closing {
        return None;
    }
    let masked = xml_mask_comments(text);
    let open = xml_find_tag(&masked, font.tag_end, font.end, field)?;
    let value_start = open + masked[open..].find('>')? + 1;
    let value_end = value_start + masked[value_start..font.end].find(&format!("</{field}>"))?;
    Some(xml_unescape(text[value_start..value_end].trim()))
}

/// The active window title font from `rc.xml`: `place="ActiveWindow"`,
/// otherwise a `<font>` without `place` (in labwc, for all places). Size `0`
/// means not set.
pub fn rc_xml_title_font(text: &str) -> Option<(String, i32)> {
    rc_xml_title_font_face(text).map(|(family, size, _)| (family, size))
}

/// [`rc_xml_title_font`] with the style: `<weight>` (Openbox: `Bold` or
/// `Normal`; labwc: Pango words `semibold`, `bold`, …) and `<slant>`
/// (`italic`, `oblique`). Regular weight and an unknown word give `0`, as in
/// [`parse_pango_font_desc`].
pub fn rc_xml_title_font_face(text: &str) -> Option<(String, i32, FontFace)> {
    let (_, fonts) = rc_xml_theme_fonts(text)?;
    let font = fonts
        .iter()
        .find(|f| f.place.eq_ignore_ascii_case("ActiveWindow"))
        .or_else(|| fonts.iter().find(|f| f.place.is_empty()))?;
    let family = rc_xml_font_field(text, font, "name")?;
    if family.is_empty() {
        return None;
    }
    let size = rc_xml_font_field(text, font, "size").and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let word = |field| rc_xml_font_field(text, font, field).unwrap_or_default().to_ascii_lowercase().replace('-', "");
    let face = FontFace {
        weight: face_weight_word(&word("weight")).unwrap_or(0),
        italic: matches!(word("slant").as_str(), "italic" | "oblique"),
    };
    Some((family, size, face))
}

/// `<weight>` and `<slant>` words for `rc.xml`. Openbox distinguishes only
/// `Bold` (everything else is regular weight for it, `parse_theme` in its
/// `config.c`), so semibold and heavier are `Bold` for it, lighter ones
/// `Normal`. labwc understands Pango steps (labwc-config(5): normal, thin,
/// ultralight, light, semilight, book, medium, semibold, bold, ultrabold,
/// heavy, ultraheavy).
fn rc_xml_face_words(wm: TitleWm, face: FontFace) -> (&'static str, &'static str) {
    if wm == TitleWm::Labwc {
        let weight = match ((face.weight.max(1) as f64 / 100.0).round() as i32).clamp(1, 9) {
            1 => "thin",
            2 => "ultralight",
            3 => "light",
            5 => "medium",
            6 => "semibold",
            7 => "bold",
            8 => "ultrabold",
            9 => "heavy",
            _ => "normal",
        };
        let weight = if face.weight <= 0 { "normal" } else { weight };
        (weight, if face.italic { "italic" } else { "normal" })
    } else {
        (if face.weight >= 600 { "Bold" } else { "Normal" }, if face.italic { "Italic" } else { "Normal" })
    }
}

/// Title places that receive the chosen font.
const RC_XML_TITLE_PLACES: [&str; 2] = ["ActiveWindow", "InactiveWindow"];

/// A new `rc.xml` with the title font `family`/`size` for the active and the
/// inactive window. The other fields of the element (`weight`, `slant`) and
/// everything else in the file stay as they were; without the element it is
/// appended at the end of `<theme>`, without `<theme>` that is appended to the root.
///
/// `existing = None` (no own file and no system sample either): for labwc a
/// file with a single `<theme>` is created (labwc takes the rest from its
/// defaults), for Openbox `None`: its `rc.xml` replaces the system one
/// entirely, and windows would be left without bindings. Also `None` when the
/// file does not parse (no root): we do not spoil what is not ours.
pub fn rc_xml_with_title_font(existing: Option<&str>, wm: TitleWm, family: &str, size: i32) -> Option<String> {
    rc_xml_with_title_font_face(existing, wm, family, size, FontFace::default())
}

/// [`rc_xml_with_title_font`] with the style from the picker: it goes into
/// `<weight>` and `<slant>` of both places ([`rc_xml_face_words`]). If no style
/// is chosen ([`FontFace::default`]), these fields stay as they were.
pub fn rc_xml_with_title_font_face(
    existing: Option<&str>,
    wm: TitleWm,
    family: &str,
    size: i32,
    face: FontFace,
) -> Option<String> {
    let root = wm.rc_xml_root();
    let mut text = match existing {
        Some(t) if !t.trim().is_empty() => t.to_string(),
        _ if wm == TitleWm::Labwc => format!("<?xml version=\"1.0\"?>\n<{root}>\n</{root}>\n"),
        _ => return None,
    };
    xml_find_tag(&xml_mask_comments(&text), 0, text.len(), root)?;
    let family_xml = xml_escape_text(family);
    let mut fields = vec![("name", family_xml.clone()), ("size", size.to_string())];
    if face != FontFace::default() {
        let (weight, slant) = rc_xml_face_words(wm, face);
        fields.push(("weight", weight.to_string()));
        fields.push(("slant", slant.to_string()));
    }
    if rc_xml_theme_fonts(&text).is_none() {
        let masked = xml_mask_comments(&text);
        let close = masked.rfind(&format!("</{root}>"))?;
        text.insert_str(close, "  <theme>\n  </theme>\n");
    }
    for place in RC_XML_TITLE_PLACES {
        let ((_, close), fonts) = rc_xml_theme_fonts(&text)?;
        match fonts.iter().find(|f| f.place.eq_ignore_ascii_case(place)) {
            Some(font) => {
                let mut element = text[font.start..font.end].to_string();
                for (field, value) in &fields {
                    element = rc_xml_set_font_field(&element, font.self_closing, field, value)?;
                }
                text.replace_range(font.start..font.end, &element);
            }
            None => {
                // Before `</theme>`, one step deeper than its indent.
                let line_start = text[..close].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let lead = &text[line_start..close];
                let (at, indent, prefix) =
                    if lead.trim().is_empty() { (line_start, lead.to_string(), "") } else { (close, String::new(), "\n") };
                let i = format!("{indent}  ");
                let body: String = fields.iter().map(|(f, v)| format!("{i}  <{f}>{v}</{f}>\n")).collect();
                let block = format!("{prefix}{i}<font place=\"{place}\">\n{body}{i}</font>\n");
                text.insert_str(at, &block);
            }
        }
    }
    Some(text)
}

/// The field `field` of a `<font>` element = `value` (already escaped): an
/// attribute if the field is set as an attribute or the element is
/// self-closing, otherwise a child element (replace the value or append before `</font>`).
fn rc_xml_set_font_field(element: &str, self_closing: bool, field: &str, value: &str) -> Option<String> {
    let tag_end = element.find('>')? + 1;
    let tag = &element[..tag_end];
    if let Some((a, b)) = xml_attr(tag, field) {
        return Some(format!("{}{value}{}", &element[..a], &element[b..]));
    }
    if self_closing {
        let slash = tag.rfind('/')?;
        return Some(format!("{} {field}=\"{value}\" {}", element[..slash].trim_end(), &element[slash..]));
    }
    let masked = xml_mask_comments(element);
    if let Some(open) = xml_find_tag(&masked, tag_end, masked.len(), field) {
        let value_start = open + masked[open..].find('>')? + 1;
        if masked.as_bytes()[value_start - 2] != b'/' {
            let value_end = value_start + masked[value_start..].find(&format!("</{field}>"))?;
            return Some(format!("{}{value}{}", &element[..value_start], &element[value_end..]));
        }
        let open_end = value_start;
        return Some(format!("{}<{field}>{value}</{field}>{}", &element[..open], &element[open_end..]));
    }
    let close = masked.rfind("</font>")?;
    let line_start = element[..close].rfind('\n').map(|i| i + 1).unwrap_or(close);
    if element[line_start..close].trim().is_empty() {
        let indent = &element[line_start..close];
        Some(format!("{}{indent}  <{field}>{value}</{field}>\n{}", &element[..line_start], &element[line_start..]))
    } else {
        Some(format!("{}<{field}>{value}</{field}>{}", &element[..close], &element[close..]))
    }
}

// ── `<theme>` nodes in rc.xml: frame theme and button layout ───────────
//
// With the same parsing as the title font ([`rc_xml_with_title_font`]): the
// file is not rebuilt, only the text of the wanted node changes, comments and
// foreign nodes stay byte for byte. Nodes are looked up among DIRECT children:
// the theme's `<name>` is not the same as `<name>` inside `<font>` of the same `<theme>`.

/// A direct child of an element: bounds of the element, end of the opening tag
/// and start of the closing one (for a self-closing one both are its end).
struct XmlChild {
    name: String,
    start: usize,
    open_end: usize,
    close: usize,
    end: usize,
    self_closing: bool,
}

/// Direct children in `masked[from..to]` (`from` is right after the parent's
/// opening tag, `to` is the start of its closing one). `None` means the tags
/// do not match (the file is broken: we do not touch what is not ours).
fn xml_children(masked: &str, from: usize, to: usize) -> Option<Vec<XmlChild>> {
    let mut out = Vec::new();
    let mut stack: Vec<(String, usize, usize)> = Vec::new();
    let mut at = from;
    while let Some(i) = masked.get(at..to)?.find('<') {
        let lt = at + i;
        let gt = lt + masked.get(lt..to)?.find('>')?;
        let inner = &masked[lt + 1..gt];
        at = gt + 1;
        if inner.starts_with('?') || inner.starts_with('!') {
            continue;
        }
        if let Some(name) = inner.strip_prefix('/') {
            let (open, start, open_end) = stack.pop()?;
            if open != name.trim() {
                return None;
            }
            if stack.is_empty() {
                out.push(XmlChild { name: open, start, open_end, close: lt, end: at, self_closing: false });
            }
            continue;
        }
        let name: String = inner.chars().take_while(|c| !c.is_whitespace() && *c != '/').collect();
        if inner.trim_end().ends_with('/') {
            if stack.is_empty() {
                out.push(XmlChild { name, start: lt, open_end: at, close: at, end: at, self_closing: true });
            }
        } else {
            stack.push((name, lt, at));
        }
    }
    stack.is_empty().then_some(out)
}

/// The root element of the file (the first top-level element).
fn xml_root(masked: &str) -> Option<XmlChild> {
    xml_children(masked, 0, masked.len())?.into_iter().find(|c| !c.self_closing)
}

/// Text of the node `<theme>/<path…>` from `rc.xml`; `None` means no node. An
/// empty node gives an empty string.
pub fn rc_xml_theme_value(text: &str, path: &[&str]) -> Option<String> {
    let masked = xml_mask_comments(text);
    let mut node = xml_root(&masked)?;
    for comp in std::iter::once("theme").chain(path.iter().copied()) {
        if node.self_closing {
            return None;
        }
        node = xml_children(&masked, node.open_end, node.close)?.into_iter().find(|c| c.name == comp)?;
    }
    Some(if node.self_closing { String::new() } else { xml_unescape(text[node.open_end..node.close].trim()) })
}

/// Indent of the line on which `at` starts.
fn xml_line_indent(text: &str, at: usize) -> String {
    let line_start = text[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
    text[line_start..at].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

/// Nested elements `comps` with the value `value` in the deepest one; each
/// level deeper is two spaces more than `indent`.
fn xml_nested(comps: &[&str], value: &str, indent: &str) -> String {
    match comps {
        [] => value.to_string(),
        [one] => format!("<{one}>{value}</{one}>"),
        [first, rest @ ..] => {
            let inner = format!("{indent}  ");
            format!("<{first}>\n{inner}{}\n{indent}</{first}>", xml_nested(rest, value, &inner))
        }
    }
}

/// A new `rc.xml` where the node `<theme>/<path…>` = `value`. If the node exists,
/// only its text changes; if not, the missing elements go first into the
/// nearest existing parent (without `<theme>` it goes into the root).
/// Everything else in the file stays as it was.
///
/// `existing = None` (neither an own file nor a system sample): for labwc a
/// file with a single `<theme>` is created, for Openbox `None`, as for the font
/// ([`rc_xml_with_title_font`]): its `rc.xml` replaces the system one entirely.
/// Also `None` for a file that does not parse: we do not spoil what is not ours.
pub fn rc_xml_with_theme_value(existing: Option<&str>, wm: TitleWm, path: &[&str], value: &str) -> Option<String> {
    let root = wm.rc_xml_root();
    let mut text = match existing {
        Some(t) if !t.trim().is_empty() => t.to_string(),
        _ if wm == TitleWm::Labwc => format!("<?xml version=\"1.0\"?>\n<{root}>\n</{root}>\n"),
        _ => return None,
    };
    let value = xml_escape_text(value);
    let full: Vec<&str> = std::iter::once("theme").chain(path.iter().copied()).collect();
    let masked = xml_mask_comments(&text);
    let mut node = xml_root(&masked)?;
    if node.self_closing {
        return None;
    }
    for (i, comp) in full.iter().enumerate() {
        let found = xml_children(&masked, node.open_end, node.close)?.into_iter().find(|c| c.name == *comp);
        match found {
            Some(child) if child.self_closing => {
                let indent = xml_line_indent(&text, child.start);
                text.replace_range(child.start..child.end, &xml_nested(&full[i..], &value, &indent));
                return Some(text);
            }
            Some(child) if i + 1 == full.len() => {
                text.replace_range(child.open_end..child.close, &value);
                return Some(text);
            }
            Some(child) => node = child,
            None => {
                let indent = format!("{}  ", xml_line_indent(&text, node.start));
                let block = format!("\n{indent}{}", xml_nested(&full[i..], &value, &indent));
                // A one-line parent (`<theme></theme>`): the closing tag moves to its own line.
                let tail = if text[node.open_end..node.close].contains('\n') {
                    String::new()
                } else {
                    format!("\n{}", xml_line_indent(&text, node.start))
                };
                text.insert_str(node.open_end, &format!("{block}{tail}"));
                return Some(text);
            }
        }
    }
    None
}

/// Paths of the manager's system `rc.xml` files (`$XDG_CONFIG_DIRS`, then
/// `/etc/xdg`): the sample an own file is created from (Openbox and labwc
/// read the FIRST `rc.xml` found, they do not merge them).
pub fn rc_xml_system_paths(wm: TitleWm, xdg_config_dirs: &str) -> Vec<String> {
    let Some(rel) = wm.rc_xml_path() else { return Vec::new() };
    let mut dirs: Vec<&str> = xdg_config_dirs.split(':').filter(|d| !d.is_empty()).collect();
    dirs.push("/etc/xdg");
    let mut out: Vec<String> = Vec::new();
    for d in dirs {
        let p = format!("{}/{rel}", d.trim_end_matches('/'));
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Paths of `lxqt/session.conf` by precedence: the own one, then the system ones
/// (`$XDG_CONFIG_DIRS`, `/etc/xdg`, `/usr/share`, as `startlxqt` looks them up).
pub fn lxqt_session_conf_paths(config_home: &str, xdg_config_dirs: &str) -> Vec<String> {
    let mut dirs = vec![config_home.to_string()];
    dirs.extend(xdg_config_dirs.split(':').filter(|d| !d.is_empty()).map(str::to_string));
    dirs.extend(["/etc/xdg".to_string(), "/usr/share".to_string()]);
    dirs.iter().map(|d| format!("{}/lxqt/session.conf", d.trim_end_matches('/'))).collect()
}

/// [`title_wm_for`] reading `lxqt/session.conf` through `read` (a fake in the
/// checks, files on a live machine).
pub fn session_title_wm(
    de: &str,
    config_home: &str,
    xdg_config_dirs: &str,
    wayland: bool,
    read: &dyn Fn(&str) -> Option<String>,
) -> Option<TitleWm> {
    let confs: Vec<String> = if de == "lxqt" {
        lxqt_session_conf_paths(config_home, xdg_config_dirs).iter().filter_map(|p| read(p)).collect()
    } else {
        Vec::new()
    };
    let refs: Vec<&str> = confs.iter().map(String::as_str).collect();
    title_wm_for(de, &refs, wayland)
}

/// The manager's own `rc.xml`, and without it the first system one: `(own path,
/// text, whether from the system one)`. No text means neither exists.
pub fn rc_xml_source(
    wm: TitleWm,
    config_home: &str,
    xdg_config_dirs: &str,
    read: &dyn Fn(&str) -> Option<String>,
) -> (String, Option<String>, bool) {
    let own = format!("{}/{}", config_home.trim_end_matches('/'), wm.rc_xml_path().unwrap_or_default());
    if let Some(text) = read(&own) {
        return (own, Some(text), false);
    }
    let system = rc_xml_system_paths(wm, xdg_config_dirs).iter().find_map(|p| read(p));
    let from_system = system.is_some();
    (own, system, from_system)
}

/// Why fonts do not change in this environment at all, before the picker:
/// Deepin (in Treeland only the Control Center sets them, Rexon does not
/// support other Deepin sessions, [`crate::dde::refusal_live`]). `None` means
/// they change.
pub fn font_refusal_live(de: &str) -> Option<String> {
    (de == "deepin").then(crate::dde::refusal_live)
}

/// Whether environment `de` has a place to apply the font `target` to.
pub fn font_target_available(de: &str, target: &str) -> bool {
    font_target_available_wm(de, target, None)
}

/// The same with the session's window manager ([`title_wm_for`]): in LXQt and
/// labwc the title applies if the manager is recognized.
pub fn font_target_available_wm(de: &str, target: &str, wm: Option<TitleWm>) -> bool {
    match target {
        "title" => TITLE_FONT_DES.contains(&de) || wm.is_some(),
        "desktop" => DESKTOP_FONT_DES.contains(&de),
        _ => true,
    }
}

/// Whether only the size is set for the target's font: xfdesktop icon captions
/// (their family comes from the interface font, there is no family property).
pub fn font_target_size_only(de: &str, target: &str) -> bool {
    de == "xfce" && target == "desktop"
}

/// Font targets that can be applied in environment `de`.
pub fn font_targets_for(de: &str) -> Vec<&'static str> {
    font_targets_for_wm(de, None)
}

/// [`font_targets_for`] with the session's window manager.
pub fn font_targets_for_wm(de: &str, wm: Option<TitleWm>) -> Vec<&'static str> {
    FONT_TARGETS
        .iter()
        .copied()
        .filter(|t| font_target_available_wm(de, t, wm))
        .collect()
}

/// Font targets the page shows in environment `de`, marked whether they can be
/// applied. The window title is not shown at all where it is drawn by a window
/// manager we cannot configure; the other targets apply everywhere ("small",
/// "toolbar" and "menu" outside KDE through Rexon's shared layer).
pub fn font_targets_shown(de: &str) -> Vec<(&'static str, bool)> {
    font_targets_shown_wm(de, None)
}

/// [`font_targets_shown`] with the session's window manager.
pub fn font_targets_shown_wm(de: &str, wm: Option<TitleWm>) -> Vec<(&'static str, bool)> {
    FONT_TARGETS
        .iter()
        .copied()
        .filter(|t| *t != "title" || font_target_available_wm(de, t, wm))
        .map(|t| (t, font_target_available_wm(de, t, wm)))
        .collect()
}

/// Build a [`FontPlan`] for environment `de` and target `target`.
///
/// * The environment's native key is [`font_backend`]; for COSMIC and LXQt it is a file:
///   COSMIC `com.system76.CosmicTk` `interface_font`/`monospace_font`;
///   LXQt `lxqt/lxqt.conf` `[Qt] font` (as `lxqt-config-appearance` writes it,
///   `fontsconfig.cpp`) and `[Qt] fixedFont` (read by the platform theme
///   `lxqt-qtplugin`, `lxqtplatformtheme.cpp:267`).
/// * The companion for GTK is `org.gnome.desktop.interface`: GTK on Wayland and
///   GNOME programs in any environment read it (LXQt itself does the same,
///   `configothertoolkits.cpp:85`). Not written in COSMIC: `cosmic-settings-daemon`
///   carries the font there from `CosmicTk` itself (`src/theme.rs:298-303`) and
///   would overwrite ours.
/// * The companion for Qt is `qt6ct/qt6ct.conf` `[Fonts] general|fixed`, only if
///   the file already exists (the person uses qt6ct); in KDE and LXQt Qt
///   programs take the font from the native storage.
/// * The title in GNOME/Cinnamon/MATE/Budgie: together with `titlebar-font`,
///   `titlebar-uses-system-font` is cleared: with `true` the font key is
///   ignored (the key description in the schemas), and in Arch Cinnamon has it
///   on by default (`30_org.archlinux.cinnamon.gschema.override`).
pub fn font_plan(de: &str, target: &str) -> FontPlan {
    font_plan_wm(de, target, None)
}

/// [`font_plan`] with the session's window manager ([`title_wm_for`]): the
/// title font in LXQt, labwc and sway is written where that manager takes it
/// from. Openbox and labwc: `rc.xml` and `--reconfigure`; xfwm4, Marco and
/// KWin under LXQt: the same keys as in their native environments (XFCE,
/// MATE, KDE); sway: [`FontFile::SwayConf`] (sway itself learns the font from
/// the `swaymsg font …` command of [`sway_title_font_steps`]).
pub fn font_plan_wm(de: &str, target: &str, wm: Option<TitleWm>) -> FontPlan {
    if target == "title" && !TITLE_FONT_DES.contains(&de) {
        match wm {
            Some(TitleWm::Xfwm4) => return font_plan_wm("xfce", target, None),
            Some(TitleWm::Marco) => return font_plan_wm("mate", target, None),
            Some(TitleWm::KWin) => return font_plan_wm("kde", target, None),
            Some(TitleWm::Sway) => {
                return FontPlan {
                    primary: FontStore::File(FontFile::SwayConf),
                    also: Vec::new(),
                    extra: Vec::new(),
                    notify: Vec::new(),
                    gtk_ini: false,
                };
            }
            Some(wm @ (TitleWm::Openbox | TitleWm::Labwc)) => {
                return FontPlan {
                    primary: FontStore::File(FontFile::RcXml { wm }),
                    also: Vec::new(),
                    extra: Vec::new(),
                    notify: vec![vec![wm.program().to_string(), "--reconfigure".to_string()]],
                    gtk_ini: false,
                };
            }
            None => {}
        }
    }
    let native_file = match (de, target) {
        ("cosmic", "system") => Some(FontFile::CosmicTk {
            key: "interface_font".to_string(),
        }),
        ("cosmic", "mono") => Some(FontFile::CosmicTk {
            key: "monospace_font".to_string(),
        }),
        ("lxqt", "desktop") => Some(FontFile::PcmanfmDesktop),
        ("lxqt", "system" | "mono") => Some(FontFile::QtIni {
            path: "lxqt/lxqt.conf".to_string(),
            group: "Qt".to_string(),
            key: if target == "mono" { "fixedFont" } else { "font" }.to_string(),
            create: true,
        }),
        // Outside KDE these fonts have no native key: our block in the GTK 3 gtk.css
        // serves as the native one (the font is read back from it).
        (_, "small" | "toolbar" | "menu") if de != "kde" => Some(FontFile::GtkCss {
            version: "3.0".to_string(),
            target: target.to_string(),
        }),
        _ => None,
    };
    let backend = font_backend(de, target);
    let primary = match native_file {
        Some(file) => FontStore::File(file),
        None => FontStore::Cmd(backend.clone()),
    };

    let mut also = Vec::new();
    // The MATE document font: next to it, the GNOME key, which GNOME programs
    // (document viewer, browser) read in MATE too.
    if target == "document" {
        let shared = SoundBackend::GSettings {
            schema: DOCUMENT_FONT_GNOME.0.to_string(),
            key: DOCUMENT_FONT_GNOME.1.to_string(),
        };
        if primary != FontStore::Cmd(shared.clone()) {
            also.push(FontStore::Cmd(shared));
        }
    }
    if matches!(target, "system" | "mono") {
        let (schema, key) = font_gsettings_key("gnome", target);
        let shared = SoundBackend::GSettings {
            schema: schema.to_string(),
            key: key.to_string(),
        };
        if de != "cosmic" && primary != FontStore::Cmd(shared.clone()) {
            also.push(FontStore::Cmd(shared));
        }
        if de != "kde" && de != "lxqt" {
            also.push(FontStore::File(FontFile::QtIni {
                path: "qt6ct/qt6ct.conf".to_string(),
                group: "Fonts".to_string(),
                key: if target == "mono" { "fixed" } else { "general" }.to_string(),
                create: false,
            }));
        }
    }
    if EXTRA_FONT_TARGETS.contains(&target) {
        // The shared layer: GTK 3 (outside KDE it is already native), GTK 4,
        // qt6ct/qt5ct outside KDE and LXQt (there Qt programs take fonts from their
        // environment, and the LXQt platform theme does not read stylesheets),
        // kdeglobals outside KDE, for KDE programs with the Plasma platform theme.
        let gtk3 = FontFile::GtkCss { version: "3.0".to_string(), target: target.to_string() };
        if primary != FontStore::File(gtk3.clone()) {
            also.push(FontStore::File(gtk3));
        }
        also.push(FontStore::File(FontFile::GtkCss { version: "4.0".to_string(), target: target.to_string() }));
        if de != "kde" && de != "lxqt" {
            for tool in QTCT_TOOLS {
                also.push(FontStore::File(FontFile::QtCtQss { tool: tool.to_string(), target: target.to_string() }));
            }
        }
        if de != "kde" {
            also.push(FontStore::Cmd(font_backend("kde", target)));
        }
    }

    let mut extra = Vec::new();
    // xfdesktop: its own size applies only if its switch is on.
    if font_target_size_only(de, target) {
        extra.push(
            sound_write_argv(
                &SoundBackend::Xfconf {
                    channel: XFDESKTOP_CHANNEL.to_string(),
                    prop: XFDESKTOP_USE_SIZE_PROP.to_string(),
                },
                "true",
                "",
            )
            .expect("xfconf пишется без kde_tool"),
        );
    }
    if let FontStore::Cmd(SoundBackend::GSettings { schema, key }) = &primary {
        if key == "titlebar-font" {
            extra.push(vec![
                "gsettings".to_string(),
                "set".to_string(),
                schema.clone(),
                "titlebar-uses-system-font".to_string(),
                "false".to_string(),
            ]);
        }
    }

    FontPlan {
        primary,
        also,
        extra,
        notify: Vec::new(),
        gtk_ini: font_syncs_to_gtk(target),
    }
}

/// argv of writing a font to a command storage; for a file, `None`.
pub fn font_store_write_argv(
    store: &FontStore,
    family: &str,
    size: i32,
    kde_tool: &str,
) -> Option<Vec<String>> {
    font_store_write_argv_face(store, family, size, FontFace::default(), kde_tool)
}

/// [`font_store_write_argv`] with the style: KDE gets it as QFont fields
/// ([`kde_font_spec_face`]), gsettings and xfconf as words of a Pango
/// description («Sans Bold Italic 10», [`pango_font_desc`]): that is how GNOME,
/// MATE, Cinnamon, Budgie and XFCE write and read them themselves.
pub fn font_store_write_argv_face(
    store: &FontStore,
    family: &str,
    size: i32,
    face: FontFace,
    kde_tool: &str,
) -> Option<Vec<String>> {
    let FontStore::Cmd(backend) = store else {
        return None;
    };
    let value = match backend {
        // The xfdesktop icon caption size is a number, without a family.
        SoundBackend::Xfconf { prop, .. } if prop == XFDESKTOP_FONT_SIZE_PROP => size.to_string(),
        SoundBackend::KConfig { .. } => kde_font_spec_face(family, size, face),
        SoundBackend::GSettings { .. } | SoundBackend::Xfconf { .. } => pango_font_desc(family, size, face),
        SoundBackend::Dde { .. } => gnome_font_spec(family, size),
    };
    sound_write_argv(backend, &value, kde_tool)
}

/// Value of the key `key` in the `[group]` section of an ini text.
fn ini_value(text: &str, group: &str, key: &str) -> Option<String> {
    let mut in_group = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_group = &line[1..line.len() - 1] == group;
            continue;
        }
        if in_group {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// The COSMIC `FontConfig` RON record, in the form `cosmic-config` writes it
/// (`to_string_pretty` with `PrettyConfig::new()`). COSMIC has no size at all:
/// text size there is set by the interface density, not by the font.
pub fn cosmic_font_ron(family: &str) -> String {
    cosmic_font_ron_face(family, FontFace::default())
}

/// The style from the font picker: weight on the Qt 6 scale (100–900; `0`
/// means not chosen) and slant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FontFace {
    pub weight: i32,
    pub italic: bool,
}

/// The style from the picker's answer "Family,size,weight,slant" (the third and
/// fourth fields; without them no style is chosen).
pub fn parse_font_face(spec: &str) -> FontFace {
    let mut parts = spec.trim().split(',').skip(2);
    let weight = parts.next().and_then(|w| w.trim().parse::<f64>().ok()).map(|w| w.round() as i32).unwrap_or(0);
    let italic = matches!(parts.next().map(str::trim), Some("1") | Some("true"));
    FontFace { weight: weight.clamp(0, 1000), italic }
}

/// Weight in `iced::font::Weight` notation (used by COSMIC's `FontConfig`): the
/// nearest step of the 100–900 scale.
pub fn cosmic_weight_name(weight: i32) -> &'static str {
    const NAMES: [&str; 9] =
        ["Thin", "ExtraLight", "Light", "Normal", "Medium", "Semibold", "Bold", "ExtraBold", "Black"];
    if weight <= 0 {
        return "Normal";
    }
    let step = ((weight as f64 / 100.0).round() as i32).clamp(1, 9);
    NAMES[(step - 1) as usize]
}

/// The COSMIC `FontConfig` RON record with the style: `weight` is an
/// `iced::font::Weight` step, `style` is `Italic` when slanted; the width
/// (`stretch`) is not set by the picker and stays `Normal`. The first line is
/// the [`COSMIC_FONT_MARK`] mark: the font reset recognizes its file by it.
pub fn cosmic_font_ron_face(family: &str, face: FontFace) -> String {
    let family = family.replace('\\', "\\\\").replace('"', "\\\"");
    let weight = cosmic_weight_name(face.weight);
    let style = if face.italic { "Italic" } else { "Normal" };
    format!(
        "{COSMIC_FONT_MARK}\n(\n    family: \"{family}\",\n    weight: {weight},\n    stretch: Normal,\n    style: {style},\n)"
    )
}

/// Weight and slant from a COSMIC `FontConfig` RON record, for checking what
/// was written: `(weight step, style)`.
pub fn cosmic_font_face_fields(text: &str) -> (String, String) {
    let field = |name: &str| {
        text.lines()
            .filter_map(|l| l.trim().strip_prefix(name))
            .map(|v| v.trim().trim_end_matches(',').trim().to_string())
            .next()
            .unwrap_or_default()
    };
    (field("weight:"), field("style:"))
}

/// The family from a COSMIC `FontConfig` RON record; empty means not found.
pub fn cosmic_font_family(text: &str) -> String {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("family:") {
            let value = rest.trim().trim_end_matches(',').trim();
            if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                return inner.replace("\\\"", "\"").replace("\\\\", "\\");
            }
        }
    }
    String::new()
}

/// Family and size from a font string. Understands both notations: GNOME
/// «Noto Sans 11» and KDE «Noto Sans,11,-1,5,Regular,0,0,0,0,0».
/// Size `0` means "it was not in the string".
pub fn parse_font_spec(raw: &str) -> (String, i32) {
    let raw = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    if raw.is_empty() {
        return (String::new(), 0);
    }
    if let Some((family, tail)) = raw.split_once(',') {
        let size = tail
            .split(',')
            .next()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .map(|v| v as i32)
            .unwrap_or(10);
        return (family.trim().to_string(), size);
    }
    if let Some((head, tail)) = raw.rsplit_once(' ') {
        if let Ok(size) = tail.trim().parse::<f64>() {
            return (head.trim().to_string(), size as i32);
        }
    }
    (raw.to_string(), 0)
}

/// A font string in GNOME/XFCE notation: «Noto Sans 11».
pub fn gnome_font_spec(family: &str, size: i32) -> String {
    format!("{family} {size}")
}

/// A QFont string in Qt 6 notation (`QFont::toString`, 16 fields): family,
/// size, pixel size, styleHint, weight (400 is regular), style and flags.
///
/// ⚠️ Previously there was a ten-field record with "Regular" in the weight
/// field here. With ten fields Qt 6 reads the weight on the old scale and
/// parses "Regular" as `0` → weight 100 (Thin): checked with `QFont::fromString`
/// from PySide6 on 30.09.2026: `…,5,Regular,0,0,0,0,0` gives weight 100, this
/// record gives 400. Plasma 6 itself writes the same
/// (`activeFont=Segoe UI,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,,0,0`).
pub fn qt_font_spec(family: &str, size: i32) -> String {
    qt_font_spec_face(family, size, FontFace::default())
}

/// [`qt_font_spec`] with the style, the way Plasma itself writes it: weight is
/// the fifth field on the Qt 6 scale (400 regular, 600 semibold, 700 bold;
/// not chosen means 400), slant is the sixth (`1` is italic). The style name
/// (the seventeenth field) is not written: then Qt picks the style by weight,
/// not by the name, which differs per family («SemiBold», «Semibold», «DemiBold»).
pub fn qt_font_spec_face(family: &str, size: i32, face: FontFace) -> String {
    let weight = if face.weight > 0 { face.weight.clamp(1, 1000) } else { 400 };
    let style = i32::from(face.italic);
    format!("{family},{size},-1,5,{weight},{style},0,0,0,0,0,0,0,0,0,1")
}

/// A font string in KDE notation: a QFont serialization, see [`qt_font_spec`].
pub fn kde_font_spec(family: &str, size: i32) -> String {
    qt_font_spec(family, size)
}

/// [`kde_font_spec`] with the style, see [`qt_font_spec_face`].
pub fn kde_font_spec_face(family: &str, size: i32, face: FontFace) -> String {
    qt_font_spec_face(family, size, face)
}

/// Qt 5 weight (scale 0–99: 50 regular, 63 semibold, 75 bold) to the Qt 6
/// scale, by the nearest step (like `qt_legacyToOpenTypeWeight` in Qt).
fn qt5_weight_to_qt6(weight: i32) -> i32 {
    const STEPS: [(i32, i32); 9] =
        [(0, 100), (12, 200), (25, 300), (50, 400), (57, 500), (63, 600), (75, 700), (81, 800), (87, 900)];
    STEPS.iter().min_by_key(|(old, _)| (old - weight).abs()).map(|(_, new)| *new).unwrap_or(400)
}

/// A QFont serialization (`QFont::toString`: ten or eleven fields in Qt 5,
/// sixteen or more in Qt 6) → family, size and style. `None` means it is not
/// a QFont record. A size in pixels (no points) gives `0`.
///
/// Weight is the fifth field: in a Qt 6 record on the 100–900 scale, in an old
/// one on the Qt 5 scale (`QFont::fromString` reads it the same way). A word
/// instead of a number ("Regular", as old Rexon versions wrote) and an empty
/// weight with a style name set are read by the style words. Slant is the
/// sixth field (`1` italic, `2` oblique) or a word in the style name.
pub fn parse_qt_font(raw: &str) -> Option<(String, i32, FontFace)> {
    let raw = raw.trim().trim_matches(['\'', '"']).trim();
    let fields: Vec<&str> = raw.split(',').map(str::trim).collect();
    if fields.len() < 10 || fields[0].is_empty() {
        return None;
    }
    let size = fields[1].parse::<f64>().ok().map(|v| v.round() as i32).filter(|v| *v > 0).unwrap_or(0);
    let qt6 = fields.len() >= 16;
    let style_name = fields.get(if qt6 { 16 } else { 10 }).copied().unwrap_or("");
    let words: Vec<String> = style_name.split_whitespace().map(|w| w.to_ascii_lowercase().replace('-', "")).collect();
    let mut face = FontFace::default();
    face.weight = match fields[4].parse::<f64>() {
        Ok(w) if qt6 => (w.round() as i32).clamp(0, 1000),
        Ok(w) => qt5_weight_to_qt6(w.round() as i32),
        Err(_) => face_weight_word(&fields[4].to_ascii_lowercase().replace('-', "")).unwrap_or(0),
    };
    if face.weight == 0 || (face.weight == 400 && !words.is_empty()) {
        if let Some(w) = words.iter().find_map(|w| face_weight_word(w)).filter(|w| *w > 0) {
            face.weight = w;
        }
    }
    if face.weight == 0 {
        face.weight = 400;
    }
    face.italic = matches!(fields[5], "1" | "2") || words.iter().any(|w| w == "italic" || w == "oblique");
    Some((fields[0].to_string(), size, face))
}

/// Family, size and style from a font string in any notation: a QFont
/// serialization ([`parse_qt_font`]) or a Pango description "Family [Style]
/// Size" ([`parse_pango_font_desc`]), which GNOME/MATE/Cinnamon/Budgie gsettings
/// and xfconf store fonts in. Size `0` means "it was not in the string"; no
/// style named means [`FontFace::default`].
pub fn parse_font_spec_face(raw: &str) -> (String, i32, FontFace) {
    let raw = raw.trim().trim_matches(['\'', '"']).trim();
    if raw.is_empty() {
        return (String::new(), 0, FontFace::default());
    }
    if let Some(qt) = parse_qt_font(raw) {
        return qt;
    }
    if raw.contains(',') {
        let (family, size) = parse_font_spec(raw);
        return (family, size, FontFace::default());
    }
    let has_size = raw.split_whitespace().last().is_some_and(|w| w.trim_end_matches("px").parse::<f64>().is_ok());
    let (family, size, face) = parse_pango_font_desc(raw);
    (family, if has_size { size } else { 0 }, face)
}

/// argv of reading a font with an environment command. `kde_tool` is the found
/// `kreadconfig6` (empty means nothing to read with). The native storage of
/// COSMIC and LXQt is a file, read through [`font_plan`].
pub fn font_read_argv(de: &str, target: &str, kde_tool: &str) -> Option<Vec<String>> {
    sound_read_argv(&font_backend(de, target), kde_tool)
}

/// argv of writing a font with an environment command. The font string is
/// taken in the notation of the storage written to: KDE understands a QFont
/// serialization, the others "Family Size".
pub fn font_write_argv(
    de: &str,
    target: &str,
    family: &str,
    size: i32,
    kde_tool: &str,
) -> Option<Vec<String>> {
    font_write_argv_face(de, target, family, size, FontFace::default(), kde_tool)
}

/// [`font_write_argv`] with the style ([`font_store_write_argv_face`]).
pub fn font_write_argv_face(
    de: &str,
    target: &str,
    family: &str,
    size: i32,
    face: FontFace,
    kde_tool: &str,
) -> Option<Vec<String>> {
    font_store_write_argv_face(&FontStore::Cmd(font_backend(de, target)), family, size, face, kde_tool)
}

/// The `settings.ini` key the interface font is carried into. Monospace and
/// title fonts have no keys of their own in GTK: only `system` is carried over.
pub const GTK_FONT_KEY: &str = "gtk-font-name";

/// Whether this font should be carried into `gtk-{3,4}.0/settings.ini`.
pub fn font_syncs_to_gtk(target: &str) -> bool {
    target == "system"
}

/// The `settings.ini` files the interface font is carried into for GTK, one per
/// version from `gtk_sanitizer::GTK_VERSIONS`.
///
/// Fix 02.10.2026: the window passed the name "gtk-3.0" to `gtk_ini_path`,
/// which prepends "gtk-" itself, and the font went to `~/.config/gtk-gtk-3.0/`,
/// a directory GTK does not read. The path is now built here, in one place,
/// from the same versions as the sanitizer's.
pub fn gtk_font_ini_paths(config_home: &str) -> Vec<String> {
    crate::gtk_sanitizer::GTK_VERSIONS
        .iter()
        .map(|v| crate::gtk_sanitizer::gtk_ini_path(config_home, v))
        .collect()
}

/// Directories the former buggy font write created (`gtk-gtk-3.0`,
/// `gtk-gtk-4.0`), each with the path of its `settings.ini`.
pub fn stray_gtk_font_dirs(config_home: &str) -> Vec<(String, String)> {
    let base = config_home.trim_end_matches('/');
    crate::gtk_sanitizer::GTK_VERSIONS
        .iter()
        .map(|v| {
            let dir = format!("{base}/gtk-gtk-{v}");
            let ini = format!("{dir}/settings.ini");
            (dir, ini)
        })
        .collect()
}

/// Whether a junk directory of the former write may be removed: it has exactly
/// one `settings.ini` file (`entries` are the names in the directory), and the
/// file has exactly what that write put there: a `[Settings]` section with the
/// single `gtk-font-name` key. Anything else means the directory is someone's, it is not touched.
pub fn stray_gtk_font_dir_removable(entries: &[String], ini_text: Option<&str>) -> bool {
    if entries.len() != 1 || entries[0] != "settings.ini" {
        return false;
    }
    let Some(text) = ini_text else { return false };
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines.len() == 2
        && lines[0] == "[Settings]"
        && lines[1].starts_with(&format!("{GTK_FONT_KEY}="))
}

// ============================================================================
// Font rendering: antialiasing, subpixels, hinting
// ============================================================================
//
// The reference set is the Plasma 6 «Fonts» page: antialiasing on/off with a
// "do not antialias" size range, subpixel rendering and the hinting level.
// Here the same setting is for ANY environment:
//
// * the shared layer is our own block in `~/.config/fontconfig/fonts.conf`:
//   this file is read by all programs that pick fonts through fontconfig
//   (GTK, Qt, Electron, Firefox, Chromium), in any environment, on both X11 and Wayland;
// * on top, the environment's native keys (its own settings show and change
//   them, and GTK/Qt take values from there on the fly): KDE `kdeglobals
//   [General] XftAntialias/XftHintStyle/XftSubPixel`, XFCE the `xsettings`
//   channel `/Xft/*`, Cinnamon `org.cinnamon.settings-daemon.plugins.
//   xsettings`, MATE `org.mate.font-rendering`, the others the GNOME keys
//   `org.gnome.desktop.interface font-antialiasing/font-hinting/
//   font-rgba-order` (GTK itself reads them on Wayland too). The keys were
//   checked against the gsettings schemas and the environments' settings sources (04.10.2026).
//
// ⚠️ The block in `fonts.conf` is written in the same form the KDE «Fonts»
// page keeps this file in: one `<match target="pattern">` with one
// `<edit mode="assign">` per property, the no-antialiasing range as two
// `<test>`s on `size` and `pixelsize`. Then KDE, opening the file, recognizes
// our entries as its own and edits them in place instead of creating second ones.

/// The screen's subpixel order, as fontconfig names it (`rgba`).
pub const SUBPIXEL_ORDERS: &[&str] = &["none", "rgb", "bgr", "vrgb", "vbgr"];

/// Hinting levels (`hintstyle` without the `hint` prefix).
pub const HINT_STYLES: &[&str] = &["none", "slight", "medium", "full"];

/// The "do not antialias" range offered while the person has not set one (the
/// same default as in KDE).
pub const AA_EXCLUDE_DEFAULT: (i32, i32) = (8, 15);

/// Size bounds in the "do not antialias" range.
pub const AA_EXCLUDE_LIMITS: (i32, i32) = (1, 99);

/// The DPI the pixel size is computed from if the person has not set their
/// own (the X11 and Wayland reference value).
pub const FONT_BASE_DPI: i32 = 96;

/// The whole font rendering setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontRendering {
    pub antialias: bool,
    /// Sizes (pt) without antialiasing, inclusive; `None` means no exceptions.
    pub exclude: Option<(i32, i32)>,
    /// One of [`SUBPIXEL_ORDERS`].
    pub subpixel: String,
    /// One of [`HINT_STYLES`].
    pub hinting: String,
}

impl Default for FontRendering {
    /// Defaults as in KDE: antialiasing on, RGB subpixels, slight hinting.
    fn default() -> Self {
        FontRendering {
            antialias: true,
            exclude: None,
            subpixel: "rgb".to_string(),
            hinting: "slight".to_string(),
        }
    }
}

impl FontRendering {
    /// Normalize: unknown values go to the default, range bounds are ordered and
    /// within [`AA_EXCLUDE_LIMITS`]. Without antialiasing there are no subpixels
    /// (KDE behaves the same way).
    pub fn normalized(mut self) -> Self {
        let def = FontRendering::default();
        if !SUBPIXEL_ORDERS.contains(&self.subpixel.as_str()) {
            self.subpixel = def.subpixel;
        }
        if !HINT_STYLES.contains(&self.hinting.as_str()) {
            self.hinting = def.hinting;
        }
        if let Some((a, b)) = self.exclude {
            let (lo, hi) = AA_EXCLUDE_LIMITS;
            let (a, b) = (a.clamp(lo, hi), b.clamp(lo, hi));
            self.exclude = Some((a.min(b), a.max(b)));
        }
        if !self.antialias {
            self.subpixel = "none".to_string();
        }
        self
    }

    /// GNOME `font-antialiasing` (and Cinnamon/MATE `antialiasing`): one value for
    /// "antialiasing" and "subpixels" together.
    pub fn gnome_antialiasing(&self) -> &'static str {
        if !self.antialias {
            "none"
        } else if self.subpixel == "none" {
            "grayscale"
        } else {
            "rgba"
        }
    }

    /// GNOME `font-rgba-order`: the order without "none" (the enum does not have it).
    pub fn gnome_rgba_order(&self) -> &str {
        if self.subpixel == "none" {
            "rgb"
        } else {
            &self.subpixel
        }
    }

    /// Hinting level in Xft/fontconfig notation: `hintslight`.
    pub fn xft_hintstyle(&self) -> String {
        format!("hint{}", self.hinting)
    }
}

/// Size in points → in pixels at the given DPI (rounded to the nearest).
pub fn point_to_pixel(points: i32, dpi: i32) -> i32 {
    ((points as f64 * dpi as f64) / 72.0).round() as i32
}

/// Which rendering property the key stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderKey {
    Antialias,
    /// Hinting on/off (XFCE has it; KDE and GNOME derive it from the level).
    Hinting,
    HintStyle,
    SubPixel,
}

/// Notation of values in the environment storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderDialect {
    /// `kdeglobals`: `true/false`, `hintslight`, `rgb`.
    Kde,
    /// GNOME, Cinnamon and MATE gsettings enums: `rgba/grayscale/none`,
    /// `slight`, `rgb`.
    Gnome,
    /// The XFCE `xsettings` channel: `1/0`, `hintslight`, `rgb`.
    Xfce,
}

/// The environment's native font rendering storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderStore {
    pub dialect: RenderDialect,
    pub keys: Vec<(RenderKey, SoundBackend)>,
}

fn render_gsettings(schema: &str, prefix: &str) -> RenderStore {
    let key = |k: RenderKey, name: &str| {
        (
            k,
            SoundBackend::GSettings {
                schema: schema.to_string(),
                key: format!("{prefix}{name}"),
            },
        )
    };
    RenderStore {
        dialect: RenderDialect::Gnome,
        keys: vec![
            key(RenderKey::Antialias, "antialiasing"),
            key(RenderKey::HintStyle, "hinting"),
            key(RenderKey::SubPixel, "rgba-order"),
        ],
    }
}

/// GNOME keys: a layer shared by GTK in any environment.
fn render_gnome_store() -> RenderStore {
    render_gsettings("org.gnome.desktop.interface", "font-")
}

/// The native storage of environment `de`. `None` means the environment has no
/// setting of its own, and `fonts.conf` itself serves as its native one (LXQt
/// writes there with its «Fonts» page; COSMIC, Deepin and tiling sessions have
/// no such page at all).
pub fn render_native_store(de: &str) -> Option<RenderStore> {
    let kde = |key: &str| SoundBackend::KConfig {
        file: "kdeglobals".to_string(),
        group: "General".to_string(),
        key: key.to_string(),
    };
    let xfce = |prop: &str| SoundBackend::Xfconf {
        channel: "xsettings".to_string(),
        prop: prop.to_string(),
    };
    match de {
        "kde" => Some(RenderStore {
            dialect: RenderDialect::Kde,
            keys: vec![
                (RenderKey::Antialias, kde("XftAntialias")),
                (RenderKey::HintStyle, kde("XftHintStyle")),
                (RenderKey::SubPixel, kde("XftSubPixel")),
            ],
        }),
        "xfce" => Some(RenderStore {
            dialect: RenderDialect::Xfce,
            keys: vec![
                (RenderKey::Antialias, xfce("/Xft/Antialias")),
                (RenderKey::Hinting, xfce("/Xft/Hinting")),
                (RenderKey::HintStyle, xfce("/Xft/HintStyle")),
                (RenderKey::SubPixel, xfce("/Xft/RGBA")),
            ],
        }),
        "cinnamon" | "cinnamon-mint" => {
            Some(render_gsettings("org.cinnamon.settings-daemon.plugins.xsettings", ""))
        }
        "mate" => Some(render_gsettings("org.mate.font-rendering", "")),
        "gnome" | "budgie" | "pantheon" => Some(render_gnome_store()),
        _ => None,
    }
}

/// Value of the property `key` in the notation of storage `dialect`.
pub fn render_value(dialect: RenderDialect, key: RenderKey, r: &FontRendering) -> String {
    match (dialect, key) {
        (RenderDialect::Kde, RenderKey::Antialias) => bool_word(r.antialias).to_string(),
        (RenderDialect::Xfce, RenderKey::Antialias) => if r.antialias { "1" } else { "0" }.to_string(),
        (RenderDialect::Gnome, RenderKey::Antialias) => r.gnome_antialiasing().to_string(),
        (_, RenderKey::Hinting) => if r.hinting == "none" { "0" } else { "1" }.to_string(),
        (RenderDialect::Gnome, RenderKey::HintStyle) => r.hinting.clone(),
        (_, RenderKey::HintStyle) => r.xft_hintstyle(),
        (RenderDialect::Gnome, RenderKey::SubPixel) => r.gnome_rgba_order().to_string(),
        (_, RenderKey::SubPixel) => r.subpixel.clone(),
    }
}

/// Parts of the setting found in one place (`None` means not set there).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderingParts {
    pub antialias: Option<bool>,
    pub subpixel: Option<String>,
    pub hinting: Option<String>,
    pub exclude: Option<(i32, i32)>,
    /// DPI from the `fonts.conf` block (`<edit name="dpi">`).
    pub dpi: Option<i32>,
}

fn clean_answer(raw: &str) -> String {
    raw.trim().trim_matches(|c| c == '\'' || c == '"').trim().to_string()
}

/// Parse the storage answers (`(key, output of the read command)`).
pub fn parse_render_answers(dialect: RenderDialect, answers: &[(RenderKey, String)]) -> RenderingParts {
    let mut out = RenderingParts::default();
    let get = |k: RenderKey| {
        answers
            .iter()
            .find(|(key, _)| *key == k)
            .map(|(_, v)| clean_answer(v))
            .filter(|v| !v.is_empty())
    };
    let hint = get(RenderKey::HintStyle).map(|h| h.strip_prefix("hint").unwrap_or(&h).to_string());
    out.hinting = hint.filter(|h| HINT_STYLES.contains(&h.as_str()));
    if dialect == RenderDialect::Xfce && get(RenderKey::Hinting).as_deref() == Some("0") {
        out.hinting = Some("none".to_string());
    }
    let sub = get(RenderKey::SubPixel).filter(|s| SUBPIXEL_ORDERS.contains(&s.as_str()) || s == "rgba");
    match dialect {
        RenderDialect::Gnome => match get(RenderKey::Antialias).as_deref() {
            Some("none") => {
                out.antialias = Some(false);
                out.subpixel = Some("none".to_string());
            }
            Some("grayscale") => {
                out.antialias = Some(true);
                out.subpixel = Some("none".to_string());
            }
            Some("rgba") => {
                out.antialias = Some(true);
                // The "rgba" nick of the order enum means "as the screen has it": the most
                // common RGB order.
                out.subpixel = Some(match sub.as_deref() {
                    Some("rgba") | Some("none") | None => "rgb".to_string(),
                    Some(s) => s.to_string(),
                });
            }
            _ => {}
        },
        RenderDialect::Kde | RenderDialect::Xfce => {
            out.antialias = match get(RenderKey::Antialias).as_deref() {
                Some("true") | Some("1") => Some(true),
                Some("false") | Some("0") => Some(false),
                _ => None,
            };
            out.subpixel = sub.filter(|s| s != "rgba");
        }
    }
    out
}

/// The resulting setting: the native storage outranks `fonts.conf` (the
/// environment itself shows and changes it), the "do not antialias" range
/// lives only in `fonts.conf`; whatever is nowhere gets the default.
pub fn merge_rendering(native: &RenderingParts, fontconfig: &RenderingParts) -> FontRendering {
    let def = FontRendering::default();
    FontRendering {
        antialias: native.antialias.or(fontconfig.antialias).unwrap_or(def.antialias),
        exclude: fontconfig.exclude,
        subpixel: native.subpixel.clone().or_else(|| fontconfig.subpixel.clone()).unwrap_or(def.subpixel),
        hinting: native.hinting.clone().or_else(|| fontconfig.hinting.clone()).unwrap_or(def.hinting),
    }
    .normalized()
}

/// The font rendering write plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderingPlan {
    /// Commands of the native storage: success is decided by them (and by the
    /// `fonts.conf` write).
    pub primary: Vec<Vec<String>>,
    /// The shared layer for GTK programs (best-effort).
    pub also: Vec<Vec<String>>,
}

fn render_store_argv(store: &RenderStore, r: &FontRendering, kwrite: &str) -> Vec<Vec<String>> {
    store
        .keys
        .iter()
        .filter_map(|(key, backend)| sound_write_argv(backend, &render_value(store.dialect, *key, r), kwrite))
        .collect()
}

/// Build a [`RenderingPlan`]. `kwrite` is the found `kwriteconfig6` (empty
/// means none); `has_gsettings` is whether `gsettings` exists.
pub fn rendering_plan(de: &str, r: &FontRendering, kwrite: &str, has_gsettings: bool) -> RenderingPlan {
    let native = render_native_store(de);
    let primary = native.as_ref().map(|s| render_store_argv(s, r, kwrite)).unwrap_or_default();
    let gnome = render_gnome_store();
    let also = if has_gsettings && native.as_ref() != Some(&gnome) {
        render_store_argv(&gnome, r, "")
    } else {
        Vec::new()
    };
    RenderingPlan { primary, also }
}

/// Read commands of the native storage: `(key, argv)`.
pub fn rendering_read_argv(de: &str, kread: &str) -> Option<(RenderDialect, Vec<(RenderKey, Vec<String>)>)> {
    let store = render_native_store(de)?;
    let reads = store
        .keys
        .iter()
        .filter_map(|(key, backend)| sound_read_argv(backend, kread).map(|argv| (*key, argv)))
        .collect();
    Some((store.dialect, reads))
}

// ── Old GNOME: settings daemon keys ─────────────────────────────────────
//
// Before the move to `org.gnome.desktop.interface` (`font-antialiasing`,
// `font-hinting`, `font-rgba-order`, gsettings-desktop-schemas 40+) GNOME kept
// font rendering in the schema of its settings daemon
// `org.gnome.settings-daemon.plugins.xsettings`: `antialiasing`
// (`none|grayscale|rgba`), `hinting` (`none|slight|medium|full`),
// `rgba-order` (`rgb|bgr|vrgb|vbgr`), the same enums as the new keys. The
// daemon hands out Xft/* to X11 programs from them. In recent releases the
// schema no longer has these keys, so they are written only where the schema
// still holds them ([`legacy_render_probe_argv`]), in any environment: nobody
// but the GNOME daemon itself reads them.

/// The old GNOME settings daemon schema.
pub const LEGACY_XSETTINGS_SCHEMA: &str = "org.gnome.settings-daemon.plugins.xsettings";

/// Old GNOME rendering keys.
pub fn legacy_render_store() -> RenderStore {
    render_gsettings(LEGACY_XSETTINGS_SCHEMA, "")
}

/// argv of checking that the old GNOME keys exist: `gsettings writable` with a
/// nonexistent key ends with an error, with an existing one it prints
/// `true`/`false`.
pub fn legacy_render_probe_argv() -> Vec<String> {
    ["gsettings", "writable", LEGACY_XSETTINGS_SCHEMA, "antialiasing"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Whether the old GNOME keys exist and can be written, by the answer of
/// [`legacy_render_probe_argv`] (empty means no key).
pub fn legacy_render_available(probe_answer: &str) -> bool {
    probe_answer.trim() == "true"
}

/// Commands writing rendering into the old GNOME keys.
pub fn legacy_rendering_argv(r: &FontRendering) -> Vec<Vec<String>> {
    render_store_argv(&legacy_render_store(), r, "")
}

/// Commands reading the old GNOME keys: `(key, argv)`.
pub fn legacy_rendering_read_argv() -> (RenderDialect, Vec<(RenderKey, Vec<String>)>) {
    let store = legacy_render_store();
    let reads = store
        .keys
        .iter()
        .filter_map(|(key, backend)| sound_read_argv(backend, "").map(|argv| (*key, argv)))
        .collect();
    (store.dialect, reads)
}

/// Whether the old GNOME keys replace the native keys of environment `de`:
/// environments on the GNOME settings daemon (GNOME, Budgie, Pantheon) have no
/// new keys at all in old releases, and the old ones decide write success.
pub fn legacy_render_is_native(de: &str) -> bool {
    render_native_store(de).as_ref() == Some(&render_gnome_store())
}

/// Whether to take rendering from the old GNOME keys: the environment's native
/// storage gave nothing (old GNOME has no new keys), while the old ones exist.
pub fn rendering_needs_legacy(native: &RenderingParts, legacy_available: bool) -> bool {
    legacy_available && native.antialias.is_none() && native.hinting.is_none() && native.subpixel.is_none()
}

// ── Block in fonts.conf ─────────────────────────────────────────────────

/// Path of the user fontconfig file relative to `$XDG_CONFIG_HOME`.
pub const FONTCONFIG_REL_PATH: &str = "fontconfig/fonts.conf";

const FC_BLOCK_BEGIN: &str = "<!-- >>> Rexon: font rendering (managed by Rexon, edits are overwritten) -->";
const FC_BLOCK_BEGIN_PREFIX: &str = "<!-- >>> Rexon: font rendering";
const FC_BLOCK_END: &str = "<!-- <<< Rexon: font rendering -->";

/// Why Rexon did not touch `fonts.conf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontconfigRefusal {
    /// The file does not parse as fontconfig XML: appending to it means guessing,
    /// and an error in this file breaks fonts for all programs.
    Malformed,
    /// Our block start is there but the end is not: someone edited the file by hand.
    BrokenBlock,
}

fn fc_single(name: &str, value_tag: &str, value: &str) -> String {
    format!(
        "  <match target=\"pattern\">\n    <edit name=\"{name}\" mode=\"assign\">\n      <{value_tag}>{value}</{value_tag}>\n    </edit>\n  </match>\n"
    )
}

fn fc_range(prop: &str, from: i32, to: i32) -> String {
    format!(
        "  <match target=\"pattern\">\n    <test qual=\"any\" name=\"{prop}\" compare=\"more_eq\">\n      <double>{from}</double>\n    </test>\n    <test qual=\"any\" name=\"{prop}\" compare=\"less_eq\">\n      <double>{to}</double>\n    </test>\n    <edit name=\"antialias\" mode=\"assign\">\n      <bool>false</bool>\n    </edit>\n  </match>\n"
    )
}

/// Our block for `fonts.conf`. `dpi` is the forced font DPI (`None` means not
/// set): the "do not antialias" range in pixels is computed from it too.
pub fn fontconfig_block(r: &FontRendering, dpi: Option<i32>) -> String {
    let r = r.clone().normalized();
    let mut out = String::new();
    out.push_str("  ");
    out.push_str(FC_BLOCK_BEGIN);
    out.push('\n');
    out.push_str(&fc_single("antialias", "bool", bool_word(r.antialias)));
    out.push_str(&fc_single("hinting", "bool", bool_word(r.hinting != "none")));
    out.push_str(&fc_single("hintstyle", "const", &r.xft_hintstyle()));
    out.push_str(&fc_single("rgba", "const", &r.subpixel));
    if let Some((from, to)) = r.exclude.filter(|_| r.antialias) {
        let px_dpi = dpi.unwrap_or(FONT_BASE_DPI);
        out.push_str(&fc_range("size", from, to));
        out.push_str(&fc_range("pixelsize", point_to_pixel(from, px_dpi), point_to_pixel(to, px_dpi)));
    }
    if let Some(dpi) = dpi {
        out.push_str(&fc_single("dpi", "double", &dpi.to_string()));
    }
    out.push_str("  ");
    out.push_str(FC_BLOCK_END);
    out.push('\n');
    out
}

/// An XML node: name, attributes, text, children.
#[derive(Debug, Default)]
struct FcNode {
    tag: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<FcNode>,
}

impl FcNode {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// Parse `fonts.conf`. `None` means the file is not XML, the root is not
/// `<fontconfig>` or it is not closed.
fn fc_parse(text: &str) -> Option<FcNode> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut stack: Vec<FcNode> = vec![FcNode::default()];
    let open = |e: &quick_xml::events::BytesStart| -> Option<FcNode> {
        let mut node = FcNode {
            tag: String::from_utf8_lossy(e.name().as_ref()).into_owned(),
            ..Default::default()
        };
        for attr in e.attributes() {
            let attr = attr.ok()?;
            node.attrs.push((
                String::from_utf8_lossy(attr.key.as_ref()).into_owned(),
                attr.unescape_value().ok()?.into_owned(),
            ));
        }
        Some(node)
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => stack.push(open(&e)?),
            Ok(Event::Empty(e)) => {
                let node = open(&e)?;
                stack.last_mut()?.children.push(node);
            }
            Ok(Event::End(_)) => {
                if stack.len() < 2 {
                    return None;
                }
                let node = stack.pop()?;
                stack.last_mut()?.children.push(node);
            }
            Ok(Event::Text(e)) => {
                let t = e.decode().ok()?;
                stack.last_mut()?.text.push_str(&t);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    if stack.len() != 1 {
        return None;
    }
    let mut doc = stack.pop()?;
    let roots: Vec<FcNode> = doc.children.drain(..).collect();
    let mut roots = roots.into_iter();
    let root = roots.next()?;
    if root.tag != "fontconfig" || roots.next().is_some() {
        return None;
    }
    Some(root)
}

/// Rendering values set in `fonts.conf`: the top-level `<match>` elements of the
/// same form we and KDE write are looked at; on a repeat the last one is
/// taken, as fontconfig also applies them in order.
pub fn fontconfig_rendering(text: &str) -> RenderingParts {
    let mut out = RenderingParts::default();
    let Some(root) = fc_parse(text) else { return out };
    for m in root.children.iter().filter(|n| n.tag == "match") {
        if !matches!(m.attr("target"), Some("pattern") | Some("font") | None) {
            continue;
        }
        let tests: Vec<&FcNode> = m.children.iter().filter(|n| n.tag == "test").collect();
        let edits: Vec<&FcNode> = m.children.iter().filter(|n| n.tag == "edit").collect();
        if edits.len() != 1 || m.children.len() != tests.len() + 1 {
            continue;
        }
        let edit = edits[0];
        if !matches!(edit.attr("mode"), Some("assign") | Some("append")) {
            continue;
        }
        let Some(value) = edit.children.first().map(|v| v.text.trim().to_string()) else {
            continue;
        };
        let name = edit.attr("name").unwrap_or("");
        if tests.is_empty() {
            match name {
                "antialias" => out.antialias = Some(value != "false"),
                "rgba" if SUBPIXEL_ORDERS.contains(&value.as_str()) => out.subpixel = Some(value),
                "hintstyle" => {
                    let style = value.strip_prefix("hint").unwrap_or(&value).to_string();
                    if HINT_STYLES.contains(&style.as_str()) {
                        out.hinting = Some(style);
                    }
                }
                "dpi" => out.dpi = value.parse::<f64>().ok().map(|v| v.round() as i32).filter(|v| *v > 0),
                _ => {}
            }
        } else if tests.len() == 2 && name == "antialias" && value == "false" {
            let mut from = None;
            let mut to = None;
            for t in &tests {
                if t.attr("name") != Some("size") {
                    continue;
                }
                let v = t.children.first().and_then(|v| v.text.trim().parse::<f64>().ok());
                match t.attr("compare") {
                    Some("more_eq") | Some("more") => from = v,
                    Some("less_eq") | Some("less") => to = v,
                    _ => {}
                }
            }
            if let (Some(a), Some(b)) = (from, to) {
                let (a, b) = (a.round() as i32, b.round() as i32);
                out.exclude = Some((a.min(b), a.max(b)));
            }
        }
    }
    out
}

/// Text of `fonts.conf` with our block: foreign entries stay where they were,
/// our previous block is removed, the new one goes before `</fontconfig>`,
/// last, so that it is the one in effect. `existing = None` means no file.
pub fn fontconfig_with_block(existing: Option<&str>, block: &str) -> Result<String, FontconfigRefusal> {
    fc_with_managed_block(existing, block, FC_BLOCK_BEGIN_PREFIX, FC_BLOCK_END)
}

/// A shared routine for all our `fonts.conf` blocks (rendering, disabled
/// fonts): the block starting with `begin_prefix` and ending with `block_end`
/// is removed and, if `block` is not empty, put again before `</fontconfig>`.
fn fc_with_managed_block(
    existing: Option<&str>,
    block: &str,
    begin_prefix: &str,
    block_end: &str,
) -> Result<String, FontconfigRefusal> {
    let text = existing.unwrap_or("");
    if text.trim().is_empty() {
        return Ok(format!(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n<fontconfig>\n{block}</fontconfig>\n"
        ));
    }
    if fc_parse(text).is_none() {
        return Err(FontconfigRefusal::Malformed);
    }
    let mut body = text.to_string();
    if let Some(begin) = body.find(begin_prefix) {
        let Some(end_rel) = body[begin..].find(block_end) else {
            return Err(FontconfigRefusal::BrokenBlock);
        };
        let line_start = body[..begin].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let start = if body[line_start..begin].trim().is_empty() { line_start } else { begin };
        let mut end = begin + end_rel + block_end.len();
        if body[end..].starts_with('\n') {
            end += 1;
        }
        body.replace_range(start..end, "");
    }
    let new_text = if let Some(close) = body.rfind("</fontconfig>") {
        let line_start = body[..close].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let (at, lead) = if body[line_start..close].trim().is_empty() {
            (line_start, "")
        } else {
            (close, "\n")
        };
        format!("{}{lead}{block}{}", &body[..at], &body[at..].trim_start_matches([' ', '\t']))
    } else if let Some(pos) = body.find("<fontconfig/>").or_else(|| body.find("<fontconfig />")) {
        let len = if body[pos..].starts_with("<fontconfig/>") { 13 } else { 14 };
        format!("{}<fontconfig>\n{block}</fontconfig>{}", &body[..pos], &body[pos + len..])
    } else {
        return Err(FontconfigRefusal::Malformed);
    };
    if fc_parse(&new_text).is_none() {
        return Err(FontconfigRefusal::Malformed);
    }
    Ok(new_text)
}

/// `true`/`false`: both `kdeglobals` and fontconfig write booleans this way.
fn bool_word(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

/// Subpixel rendering options for the list: `(value, caption)`.
pub fn subpixel_choices() -> Vec<(&'static str, String)> {
    SUBPIXEL_ORDERS
        .iter()
        .map(|id| {
            let label = match *id {
                "none" => trc("sub-pixel rendering", "None"),
                "rgb" => tr("RGB"),
                "bgr" => tr("BGR"),
                "vrgb" => tr("Vertical RGB"),
                _ => tr("Vertical BGR"),
            };
            (*id, label)
        })
        .collect()
}

/// Hinting options for the list: `(value, caption)`.
pub fn hinting_choices() -> Vec<(&'static str, String)> {
    HINT_STYLES
        .iter()
        .map(|id| {
            let label = match *id {
                "none" => trc("font hinting", "None"),
                "slight" => trc("font hinting", "Slight"),
                "medium" => trc("font hinting", "Medium"),
                _ => trc("font hinting", "Full"),
            };
            (*id, label)
        })
        .collect()
}

/// Ready-made rendering presets, like the four samples of MATE's «Appearance →
/// Fonts → Rendering» (and old GNOME 2), which neither KDE, nor GNOME, nor XFCE
/// have: antialiasing, subpixels and hinting with a single click. The values
/// were checked against `mate-control-center/capplets/appearance/
/// appearance-font.c` (`setup_font_pair`, 14.11.2026): «Monochrome» means no
/// antialiasing, full hinting; «Best shapes» grayscale, medium; «Best
/// contrast» grayscale, full; «Subpixel smoothing» RGBA, slight. They are
/// written by the same shared path as the manual setting ([`FontRendering`]),
/// which means in all environments.
pub const RENDER_PRESETS: &[&str] = &["monochrome", "best_shapes", "best_contrast", "subpixel"];

/// Rendering by a ready-made preset on top of the current one: the "no
/// antialiasing" range stays as it was, and "Subpixel" gets the current
/// subpixel order (MATE does not touch the order either) if it is set, otherwise RGB.
pub fn render_preset(id: &str, current: &FontRendering) -> Option<FontRendering> {
    let (antialias, subpixel, hinting) = match id {
        "monochrome" => (false, "none", "full"),
        "best_shapes" => (true, "none", "medium"),
        "best_contrast" => (true, "none", "full"),
        "subpixel" => {
            let order = if current.subpixel == "none" { "rgb" } else { current.subpixel.as_str() };
            (true, order, "slight")
        }
        _ => return None,
    };
    Some(
        FontRendering {
            antialias,
            exclude: current.exclude,
            subpixel: subpixel.to_string(),
            hinting: hinting.to_string(),
        }
        .normalized(),
    )
}

/// Which ready-made preset the rendering matches (`None` means its own, none).
/// The "no antialiasing" range and the subpixel order do not distinguish presets.
pub fn render_preset_of(r: &FontRendering) -> Option<&'static str> {
    RENDER_PRESETS
        .iter()
        .copied()
        .find(|id| render_preset(id, r).is_some_and(|p| p == *r))
}

/// Ready-made presets for the button row: `(value, caption, tooltip)`.
pub fn render_preset_choices() -> Vec<(&'static str, String, String)> {
    RENDER_PRESETS
        .iter()
        .map(|id| {
            let (label, tip) = match *id {
                "monochrome" => (
                    trc("font rendering preset", "Monochrome"),
                    tr("No smoothing, full hinting: sharp pixel-exact letters"),
                ),
                "best_shapes" => (
                    trc("font rendering preset", "Best shapes"),
                    tr("Grayscale smoothing, medium hinting: letters closest to their drawing"),
                ),
                "best_contrast" => (
                    trc("font rendering preset", "Best contrast"),
                    tr("Grayscale smoothing, full hinting: the sharpest smoothed text"),
                ),
                _ => (
                    trc("font rendering preset", "Subpixel smoothing (LCDs)"),
                    tr("Sub-pixel smoothing, slight hinting: the clearest text on flat screens"),
                ),
            };
            (*id, label, tip)
        })
        .collect()
}

/// How writing the font rendering ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderingOutcome {
    /// Written both to `fonts.conf` and to the environment's native keys.
    Saved,
    /// `fonts.conf` is untouched: it does not parse ([`FontconfigRefusal`]).
    /// The string is the file path.
    FontconfigRefused(String),
    /// `fonts.conf` could not be written (no permission, no space).
    FontconfigFailed(String),
    /// The environment's native keys could not be written.
    NativeFailed,
}

/// Result of writing the font rendering, for a person.
pub fn rendering_status(outcome: &RenderingOutcome) -> String {
    match outcome {
        RenderingOutcome::Saved => t("font.render_saved"),
        RenderingOutcome::FontconfigRefused(path) => tf("font.fc_refused", &[path]),
        RenderingOutcome::FontconfigFailed(path) => tf("font.fc_failed", &[path]),
        RenderingOutcome::NativeFailed => t("font.render_native_failed"),
    }
}

// ============================================================================
// «Adjust All Fonts» and forced font DPI
// ============================================================================

/// What «Adjust All Fonts» changes: family and size, family only, size only
/// (in KDE the same is chosen with checkboxes in the font picker).
pub const CHANGE_ALL_MODES: &[&str] = &["both", "family", "size"];

/// "What to change" options for the list: `(value, caption)`.
pub fn change_all_mode_choices() -> Vec<(&'static str, String)> {
    CHANGE_ALL_MODES
        .iter()
        .map(|id| {
            let label = match *id {
                "family" => tr("Family only"),
                "size" => tr("Size only"),
                _ => tr("Family and size"),
            };
            (*id, label)
        })
        .collect()
}

/// Caption of the «Adjust All Fonts» button, its tooltip and the picker title.
pub fn change_all_texts() -> (String, String, String) {
    (
        tr("🔤 Change All Fonts…"),
        tr("Set one family and size for every font at once"),
        tr("All Fonts"),
    )
}

/// One font in "adjust all": the target, the current family and size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontNow {
    pub target: String,
    pub family: String,
    pub size: i32,
}

/// What to write to each font on «Adjust All Fonts».
///
/// The rules are the same as on the KDE «Fonts» page: the small font comes
/// out 2 pt smaller than the chosen one (if the chosen one is at least 9 pt,
/// otherwise small text would become unreadable), the monospace font does not
/// get a family that is not monospace itself (`family_is_mono`): only its
/// size changes. The second value is whether the monospace font kept its previous font.
pub fn change_all_fonts(now: &[FontNow], family: &str, size: i32, mode: &str, family_is_mono: bool) -> (Vec<FontNow>, bool) {
    let set_family = mode != "size";
    let set_size = mode != "family";
    let mut mono_kept = false;
    let out = now
        .iter()
        .filter_map(|cur| {
            let mut fam = if set_family { family.to_string() } else { cur.family.clone() };
            if cur.target == "mono" && set_family && !family_is_mono {
                // The monospace font stays its own; if it has none, it is not touched at
                // all, so as not to write a regular font there.
                if cur.family.is_empty() {
                    return None;
                }
                fam = cur.family.clone();
                mono_kept = true;
            }
            if fam.is_empty() {
                // Size only, while the font has no family (not set): the interface font is
                // taken, as the environment itself does.
                fam = now
                    .iter()
                    .find(|n| n.target == "system")
                    .map(|n| n.family.clone())
                    .filter(|f| !f.is_empty())?;
            }
            let base = if cur.size > 0 { cur.size } else { 10 };
            let sz = if !set_size {
                base
            } else if cur.target == "small" && size >= 9 {
                size - 2
            } else {
                size
            };
            Some(FontNow {
                target: cur.target.clone(),
                family: fam,
                size: sz,
            })
        })
        .collect();
    (out, mono_kept)
}

/// argv of the "is the family monospace" check: `fc-list` returns `spacing`
/// of each style of the family (100 is monospace, 90 is dual width).
pub fn fc_spacing_argv(family: &str) -> Vec<String> {
    vec![
        "fc-list".to_string(),
        format!(":family={}", family.replace('\\', "\\\\").replace(':', "\\:").replace('-', "\\-")),
        "spacing".to_string(),
    ]
}

/// Whether the family is monospace by the output of [`fc_spacing_argv`]: all
/// styles are `spacing=100` or `90`. An empty answer (unknown family) means no.
pub fn parse_fc_spacing(out: &str) -> bool {
    let values: Vec<&str> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| l.trim_start_matches(':').trim().trim_start_matches("spacing=").trim())
        .collect();
    !values.is_empty() && values.iter().all(|v| *v == "100" || *v == "90")
}

/// Result of «Adjust All Fonts».
pub fn change_all_status(failed: &[String], family: &str, size: i32, mode: &str, mono_kept: Option<&str>) -> String {
    if !failed.is_empty() {
        // TRANSLATORS: %1 is a comma-separated list of font kinds (“Menu Font, Small Font”).
        return trf("⚠️ All fonts: could not apply %1", &[&failed.join(", ")]);
    }
    let what = match mode {
        "family" => family.to_string(),
        "size" => trf("%1 pt", &[&size]),
        _ => trf("%1 %2 pt", &[&family, &size]),
    };
    // TRANSLATORS: %1 is the chosen font, e.g. “Noto Sans 11 pt”.
    let mut text = trf("✅ All fonts: %1", &[&what]);
    if let Some(kept) = mono_kept {
        text.push_str(". ");
        // TRANSLATORS: %1 is the monospace font family that was kept.
        text.push_str(&trf("The monospace font stays %1: the chosen one isn't monospaced", &[&kept]));
    }
    text
}

/// Bounds of the forced DPI and the value the field starts from.
pub const FONT_DPI_RANGE: (i32, i32) = (48, 480);

/// Where the environment keeps the font DPI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontDpiStore {
    /// `kcmfonts [General] forceFontDPI` (`0` means not set). Plasma 6.7 and
    /// earlier carry it into `Xft.dpi` at an X11 session login; in Wayland
    /// Plasma has no font DPI: fonts follow the screen scale.
    Kde,
    /// XFCE `xsettings /Xft/DPI` (`-1` means compute from the screen).
    Xfce,
    /// MATE `org.mate.font-rendering dpi` (`0` means from the screen).
    Mate,
    /// The environment has no font DPI of its own, only a text size multiplier
    /// (`text-scaling-factor`), the very one from which the GNOME settings
    /// daemon computes the `Xft/DPI` it gives GTK (96 × multiplier). So DPI N
    /// is the multiplier N/96.
    TextScaling { schema: String },
    /// DPI in our own `fonts.conf` block: that is how the LXQt «Fonts» page
    /// stores it.
    Fontconfig,
    /// `Xft.dpi` in our own `~/.Xresources` block: an X11 session without a
    /// settings daemon that would hand out DPI itself (i3, Openbox and other
    /// window managers without an environment). At login this file is merged
    /// into the X resource database by session scripts (`startx`/`xinitrc`, the
    /// Xsession of SDDM, LightDM, GDM), and `Xft.dpi` from it is taken by GTK
    /// (if there is no XSETTINGS), Qt and Xft programs.
    Xresources,
}

/// The font DPI storage of environment `de`. `None` means there is nothing to
/// set it with (Plasma in a Wayland session).
pub fn font_dpi_store(de: &str, wayland: bool) -> Option<FontDpiStore> {
    let scaling = |schema: &str| Some(FontDpiStore::TextScaling { schema: schema.to_string() });
    match de {
        "kde" if wayland => None,
        "kde" => Some(FontDpiStore::Kde),
        "xfce" => Some(FontDpiStore::Xfce),
        "mate" => Some(FontDpiStore::Mate),
        "cinnamon" | "cinnamon-mint" => scaling("org.cinnamon.desktop.interface"),
        "lxqt" => Some(FontDpiStore::Fontconfig),
        // Deepin outside Wayland is an X11 session, which Rexon does not support
        // ([`font_dpi_note_for`] says exactly that).
        "deepin" if !wayland => None,
        // Environments with their own settings daemon (it hands out Xft/DPI in an
        // X11 session) and Wayland sessions: the GNOME text multiplier.
        "gnome" | "budgie" | "pantheon" | "deepin" | "cosmic" => scaling("org.gnome.desktop.interface"),
        _ if !wayland => Some(FontDpiStore::Xresources),
        _ => scaling("org.gnome.desktop.interface"),
    }
}

/// Whether to also write DPI into `~/.Xresources` on top of the native storage:
/// LXQt in an X11 session keeps DPI only in `fonts.conf` and does not put
/// `Xft.dpi` into the X resource database, so Qt and Xft programs do not see it without this.
pub fn font_dpi_xresources_also(de: &str, wayland: bool) -> bool {
    de == "lxqt" && !wayland
}

/// The user's X resources file (relative to the home directory).
pub const XRESOURCES_REL_PATH: &str = ".Xresources";

const XR_BLOCK_BEGIN: &str = "! >>> Rexon: font DPI (managed by Rexon, edits are overwritten)";
const XR_BLOCK_BEGIN_PREFIX: &str = "! >>> Rexon: font DPI";
const XR_BLOCK_END: &str = "! <<< Rexon: font DPI";

/// The font DPI resource line.
pub fn xft_dpi_line(dpi: i32) -> String {
    format!("Xft.dpi: {dpi}\n")
}

/// Text of `~/.Xresources` with our `Xft.dpi` block (`dpi = None` removes the
/// block). Foreign lines stay, our block goes at the end (for xrdb the last of
/// two identical resources wins). `None` means the block start is there but
/// the end is not (edited by hand).
pub fn xresources_with_dpi(existing: Option<&str>, dpi: Option<i32>) -> Option<String> {
    let mut body = existing.unwrap_or_default().to_string();
    if let Some(begin) = body.find(XR_BLOCK_BEGIN_PREFIX) {
        let end_rel = body[begin..].find(XR_BLOCK_END)?;
        let mut end = begin + end_rel + XR_BLOCK_END.len();
        if body[end..].starts_with('\n') {
            end += 1;
        }
        body.replace_range(begin..end, "");
    }
    let body = body.trim_end();
    let Some(dpi) = dpi else {
        return Some(if body.is_empty() { String::new() } else { format!("{body}\n") });
    };
    let block = format!("{XR_BLOCK_BEGIN}\n{}{XR_BLOCK_END}\n", xft_dpi_line(dpi));
    Some(if body.is_empty() { block } else { format!("{body}\n\n{block}") })
}

/// `Xft.dpi` from the text of `~/.Xresources`: the last value (xrdb decides
/// the same way); `None` means not set.
pub fn xresources_dpi(text: &str) -> Option<i32> {
    let mut out = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('!') || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else { continue };
        let name = name.trim();
        if name == "Xft.dpi" || name == "Xft*dpi" || name == "*dpi" {
            out = value.trim().parse::<f64>().ok().map(|v| v.round() as i32).filter(|v| *v > 0);
        }
    }
    out
}

/// argv of merging the resources file into the live X database (without the
/// preprocessor: our file has one line).
pub fn xrdb_merge_argv(path: &str) -> Vec<String> {
    vec!["xrdb".to_string(), "-nocpp".to_string(), "-merge".to_string(), path.to_string()]
}

fn dpi_backend(store: &FontDpiStore) -> Option<SoundBackend> {
    Some(match store {
        FontDpiStore::Kde => SoundBackend::KConfig {
            file: "kcmfonts".to_string(),
            group: "General".to_string(),
            key: "forceFontDPI".to_string(),
        },
        FontDpiStore::Xfce => SoundBackend::Xfconf {
            channel: "xsettings".to_string(),
            prop: "/Xft/DPI".to_string(),
        },
        FontDpiStore::Mate => SoundBackend::GSettings {
            schema: "org.mate.font-rendering".to_string(),
            key: "dpi".to_string(),
        },
        FontDpiStore::TextScaling { schema } => SoundBackend::GSettings {
            schema: schema.clone(),
            key: "text-scaling-factor".to_string(),
        },
        FontDpiStore::Fontconfig | FontDpiStore::Xresources => return None,
    })
}

/// Text size multiplier for DPI `dpi` (within the schema's 0.5–3.0).
pub fn text_scaling_for_dpi(dpi: i32) -> f64 {
    ((dpi as f64 / FONT_BASE_DPI as f64) * 100.0).round().clamp(50.0, 300.0) / 100.0
}

/// The DPI value in the storage entry; `None` means remove it (back to "from the screen").
pub fn font_dpi_value(store: &FontDpiStore, dpi: Option<i32>) -> String {
    match (store, dpi) {
        (FontDpiStore::Kde, Some(d)) | (FontDpiStore::Xfce, Some(d)) => d.to_string(),
        (FontDpiStore::Kde, None) => "0".to_string(),
        (FontDpiStore::Xfce, None) => "-1".to_string(),
        (FontDpiStore::Mate, Some(d)) => format!("{d}.0"),
        (FontDpiStore::Mate, None) => "0.0".to_string(),
        (FontDpiStore::TextScaling { .. }, Some(d)) => format!("{:.2}", text_scaling_for_dpi(d)),
        (FontDpiStore::TextScaling { .. }, None) => "1.0".to_string(),
        (FontDpiStore::Fontconfig, d) | (FontDpiStore::Xresources, d) => {
            d.map(|d| d.to_string()).unwrap_or_default()
        }
    }
}

/// argv of writing DPI; for `fonts.conf` it is `None` (that is a file).
pub fn font_dpi_write_argv(store: &FontDpiStore, dpi: Option<i32>, kwrite: &str) -> Option<Vec<String>> {
    sound_write_argv(&dpi_backend(store)?, &font_dpi_value(store, dpi), kwrite)
}

/// argv of reading DPI; for `fonts.conf` it is `None`.
pub fn font_dpi_read_argv(store: &FontDpiStore, kread: &str) -> Option<Vec<String>> {
    sound_read_argv(&dpi_backend(store)?, kread)
}

/// The set DPI from the storage's answer; `None` means not set (from the screen).
pub fn parse_font_dpi(store: &FontDpiStore, raw: &str) -> Option<i32> {
    let v: f64 = clean_answer(raw).parse().ok()?;
    match store {
        FontDpiStore::TextScaling { .. } => {
            if (v - 1.0).abs() < 0.005 || v <= 0.0 {
                None
            } else {
                Some((v * FONT_BASE_DPI as f64).round() as i32)
            }
        }
        _ => (v > 0.0).then(|| v.round() as i32),
    }
}

/// The explanation under the DPI field: what exactly takes effect and when.
pub fn font_dpi_note(store: Option<&FontDpiStore>) -> String {
    match store {
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        None => tr("In the Plasma Wayland session fonts follow the display scale: change it in System \
                    Settings → Display"),
        Some(FontDpiStore::Kde) => tr("Takes effect after you log in again"),
        Some(FontDpiStore::TextScaling { .. }) => tr("This desktop has no separate font DPI, so Rexon \
                    sets its text scaling factor instead (96 DPI = 100%)"),
        Some(FontDpiStore::Fontconfig) => tr("Written to the shared font settings file, as LXQt itself \
                    does; apps that are already open pick it up after a restart"),
        Some(FontDpiStore::Xresources) => tr("This session has no settings service of its own, so Rexon \
                    writes Xft.dpi to ~/.Xresources: apps started from now on pick it up, all of them after you \
                    log in again"),
        Some(FontDpiStore::Xfce) | Some(FontDpiStore::Mate) => String::new(),
    }
}

/// [`font_dpi_note`] adjusted for the environment: Deepin has no storage outside
/// Wayland, that is an X11 session, and the caption says Rexon does not support it.
pub fn font_dpi_note_for(de: &str, store: Option<&FontDpiStore>) -> String {
    match (de, store) {
        ("deepin", None) => crate::dde::not_treeland_message(),
        _ => font_dpi_note(store),
    }
}

/// Result of changing the font DPI.
pub fn font_dpi_status(ok: bool, dpi: Option<i32>) -> String {
    match (ok, dpi) {
        (false, _) => tr("⚠️ Could not change the font DPI"),
        // TRANSLATORS: %1 is a number of dots per inch, e.g. 120.
        (true, Some(d)) => trf("✅ Font DPI: %1", &[&d]),
        (true, None) => tr("✅ Font DPI follows the screen again"),
    }
}

// ============================================================================
// Removing Rexon's font layers (resetting fonts to the defaults)
// ============================================================================
//
// Besides the environments' native keys, the «Fonts» page puts its layers
// into foreign files, each in a marked block or in a file that is entirely ours:
//
// * `gtk-3.0/gtk.css`, `gtk-4.0/gtk.css`: the block of menu, toolbar and
//   small caption fonts (in place of a link to the theme, a file with an
//   `@import` of the same theme and our block);
// * `qt6ct/qss/rexon-fonts.qss`, `qt5ct/qss/rexon-fonts.qss`: our
//   stylesheet and its line in `[Interface] stylesheets`;
// * `fontconfig/fonts.conf`: the antialiasing, hinting and DPI block;
// * `~/.Xresources`: the `Xft.dpi` block;
// * COSMIC `com.system76.CosmicTk/v1/interface_font` and `monospace_font`:
//   the whole file, with our mark as the first line.
//
// Resetting fonts removes exactly these blocks: the foreign content of the
// files stays as it was. A file with nothing left after the removal is
// deleted; a `gtk.css` with only our link to the theme left becomes a
// symbolic link to it again. A block broken by hand (a start without an
// end) is not touched: guessing where ours ends is not allowed.

/// The first-line mark in COSMIC font RON files written by Rexon. RON allows
/// comments, `cosmic-config` skips them; when COSMIC «Settings» rewrites the
/// file, the mark goes away together with our value.
pub const COSMIC_FONT_MARK: &str = "// Rexon: font (managed by Rexon)";

/// COSMIC font keys Rexon writes.
pub const COSMIC_FONT_KEYS: &[&str] = &["interface_font", "monospace_font"];

/// What to do with one file when resetting fonts. Paths are full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontLayerUndo {
    /// Rewrite the file with this text (our block removed, the foreign content left).
    Write { path: String, text: String },
    /// Delete the file: nothing but ours is left in it.
    Remove { path: String },
    /// Put a symbolic link to the theme (`target`) back in place of the file.
    Relink { path: String, target: String },
}

/// How the reset plan looks at files: the text by full path and whether it is
/// a link (we do not write through a link to a foreign theme, so there is
/// nothing to remove in it either).
pub trait FontLayerFs {
    fn read(&self, path: &str) -> Option<String>;
    fn is_link(&self, path: &str) -> bool;
}

/// The stylesheet text without our font block. `None` means there is no block,
/// or it is broken by hand.
pub fn text_without_extra_fonts_block(text: &str) -> Option<String> {
    let begin = text.find(EXTRA_BLOCK_BEGIN_PREFIX)?;
    let end_rel = text[begin..].find(EXTRA_BLOCK_END)?;
    let mut end = begin + end_rel + EXTRA_BLOCK_END.len();
    if text[end..].starts_with('\n') {
        end += 1;
    }
    let head = text[..begin].trim_end();
    let tail = text[end..].trim_start_matches('\n');
    let mut out = head.to_string();
    if !out.is_empty() && !tail.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(tail);
    let out = out.trim_end();
    Some(if out.is_empty() { String::new() } else { format!("{out}\n") })
}

/// The theme the `gtk.css` link pointed to, if exactly our line
/// `@import url("file://…");` is left of the file (Rexon puts it in place of
/// the link, see [`FontFile::file_writes`]).
pub fn gtk_css_import_only(text: &str) -> Option<String> {
    let url = text
        .trim()
        .strip_prefix("@import url(\"file://")?
        .strip_suffix("\");")?;
    if url.contains(['"', '\n', ')']) {
        return None;
    }
    let bytes = url.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok().filter(|p| p.starts_with('/'))
}

/// Text of `qt6ct.conf` without our stylesheet in `[Interface] stylesheets`.
/// `None` means it is not there. Recognized by name and place
/// (`<tool>/qss/rexon-fonts.qss`), not by the whole path: the settings directory may have moved.
pub fn qtct_conf_without_stylesheet(conf: &str, tool: &str) -> Option<String> {
    let ours = format!("/{tool}/qss/{QTCT_QSS_NAME}");
    let list = qsettings_list(&ini_value(conf, "Interface", "stylesheets").unwrap_or_default());
    let kept: Vec<String> = list.iter().filter(|p| !p.ends_with(&ours)).cloned().collect();
    if kept.len() == list.len() {
        return None;
    }
    // QSettings writes an empty list the same way qt6ct itself does.
    let value = if kept.is_empty() { "@Invalid()".to_string() } else { qsettings_list_value(&kept) };
    Some(crate::gtk_sanitizer::write_ini_setting_in(Some(conf), "Interface", "stylesheets", &value))
}

/// Text of `fonts.conf` without our block. `None` means no block, or it is broken.
pub fn fontconfig_without_block(text: &str) -> Option<String> {
    let begin = text.find(FC_BLOCK_BEGIN_PREFIX)?;
    let end_rel = text[begin..].find(FC_BLOCK_END)?;
    let line_start = text[..begin].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let start = if text[line_start..begin].trim().is_empty() { line_start } else { begin };
    let mut end = begin + end_rel + FC_BLOCK_END.len();
    if text[end..].starts_with('\n') {
        end += 1;
    }
    Some(format!("{}{}", &text[..start], &text[end..]))
}

/// Whether `fonts.conf` is empty: besides the XML declaration, DOCTYPE and an
/// empty `<fontconfig>` root there is nothing in it (not even a comment);
/// Rexon itself creates such a file when it writes the first block.
pub fn fontconfig_is_bare(text: &str) -> bool {
    let mut rest = text.trim();
    if let Some(r) = rest.strip_prefix("<?xml") {
        match r.find("?>") {
            Some(i) => rest = r[i + 2..].trim_start(),
            None => return false,
        }
    }
    if let Some(r) = rest.strip_prefix("<!DOCTYPE") {
        match r.find('>') {
            Some(i) => rest = r[i + 1..].trim_start(),
            None => return false,
        }
    }
    if rest == "<fontconfig/>" || rest == "<fontconfig />" {
        return true;
    }
    rest.strip_prefix("<fontconfig>")
        .and_then(|r| r.trim().strip_suffix("</fontconfig>"))
        .is_some_and(|inner| inner.trim().is_empty())
}

/// Whether a COSMIC font RON file was written by Rexon itself (by the mark).
pub fn cosmic_font_is_ours(text: &str) -> bool {
    text.trim_start().starts_with(COSMIC_FONT_MARK)
}

/// The font reset plan: which files to restore and how, so that nothing of
/// Rexon's layers remains. Files without ours, links and blocks broken by hand
/// do not get into the plan.
pub fn font_layers_reset_plan(config_home: &str, home: &str, fs: &dyn FontLayerFs) -> Vec<FontLayerUndo> {
    let base = config_home.trim_end_matches('/');
    let mut plan = Vec::new();
    // Remove our block; a file with nothing left is deleted.
    let mut strip = |path: String, without: &dyn Fn(&str) -> Option<String>, bare: &dyn Fn(&str) -> bool| {
        if fs.is_link(&path) {
            return;
        }
        let Some(text) = fs.read(&path) else { return };
        let Some(rest) = without(&text) else { return };
        plan.push(if bare(&rest) {
            FontLayerUndo::Remove { path }
        } else {
            FontLayerUndo::Write { path, text: rest }
        });
    };

    for version in ["3.0", "4.0"] {
        strip(format!("{base}/gtk-{version}/gtk.css"), &text_without_extra_fonts_block, &|r| r.trim().is_empty());
    }
    strip(format!("{base}/{FONTCONFIG_REL_PATH}"), &fontconfig_without_block, &fontconfig_is_bare);
    strip(
        format!("{}/{XRESOURCES_REL_PATH}", home.trim_end_matches('/')),
        &|t| t.contains(XR_BLOCK_BEGIN_PREFIX).then(|| xresources_with_dpi(Some(t), None)).flatten(),
        &|r| r.trim().is_empty(),
    );

    // A gtk.css with only our link to the theme left goes back to a symbolic
    // link (if the theme is gone, it is simply removed).
    for undo in plan.iter_mut() {
        let FontLayerUndo::Write { path, text } = undo else { continue };
        if !path.ends_with("/gtk.css") {
            continue;
        }
        if let Some(target) = gtk_css_import_only(text) {
            *undo = if fs.read(&target).is_some() {
                FontLayerUndo::Relink { path: path.clone(), target }
            } else {
                FontLayerUndo::Remove { path: path.clone() }
            };
        }
    }

    for tool in QTCT_TOOLS {
        let qss = format!("{base}/{tool}/qss/{QTCT_QSS_NAME}");
        if !fs.is_link(&qss) && fs.read(&qss).is_some_and(|t| t.contains(EXTRA_BLOCK_BEGIN_PREFIX)) {
            plan.push(FontLayerUndo::Remove { path: qss });
        }
        let conf = format!("{base}/{tool}/{tool}.conf");
        if fs.is_link(&conf) {
            continue;
        }
        if let Some(text) = fs.read(&conf).and_then(|t| qtct_conf_without_stylesheet(&t, tool)) {
            plan.push(FontLayerUndo::Write { path: conf, text });
        }
    }

    for key in COSMIC_FONT_KEYS {
        let path = format!("{base}/{}", FontFile::CosmicTk { key: key.to_string() }.rel_path());
        if !fs.is_link(&path) && fs.read(&path).is_some_and(|t| cosmic_font_is_ours(&t)) {
            plan.push(FontLayerUndo::Remove { path });
        }
    }
    plan
}

// ============================================================================
// Font management: installed fonts, installing from a file, removal
// ============================================================================
//
// The reference is KDE's «Font Management» page: all installed fonts by
// family, a sample of each style, search, installing and removing your own;
// system ones are view-only (the package manager installs them, and removing
// them from a settings window means breaking packages).
//
// The list source is fontconfig itself (`fc-list`): exactly the fonts the
// programs see. Own fonts live in the directories fontconfig scans for the
// user: `$XDG_DATA_HOME/fonts` (where Rexon installs too) and the obsolete
// `~/.fonts`. After any change the directory cache is rebuilt with
// `fc-cache -f <directory>`.

/// Font file extensions the picker offers. Which of them this machine
/// understands is decided by the FreeType fontconfig is built with (WOFF with
/// zlib, WOFF2 with brotli), so before installing each file is checked with
/// [`fc_query_families_argv`] instead of trusting the extension.
pub const FONT_INSTALL_EXTS: &[&str] =
    &["ttf", "otf", "ttc", "otc", "woff", "woff2", "pfb", "pfa", "pcf", "pcf.gz", "bdf"];

/// The fonts directory "for all users": the same place KDE's «Font Management»
/// installs to for the «System» group. Not `/usr/share/fonts`: package files
/// live there, and the package manager would know nothing about ours. This
/// directory is scanned by fontconfig by default (the distributions' `fonts.conf`).
/// Only the root helper installs and removes here
/// ([`crate::privilege::op::PrivOp::FontInstall`] / `FontRemove`), and its check
/// looks here too ([`crate::privilege::validate::check_font_remove`]).
pub const SHARED_FONT_DIR: &str = "/usr/local/share/fonts";

/// One installed font (a style in a file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledFont {
    pub family: String,
    pub style: String,
    pub file: String,
    /// Lies in the user's fonts directory: may be removed.
    pub user: bool,
    /// Lies in [`SHARED_FONT_DIR`] (installed "for all users"): removed with the
    /// administrator password. Package fonts are neither `user` nor `shared`: only
    /// the package manager removes them.
    pub shared: bool,
    /// Disabled by Rexon (`<rejectfont>` in our `fonts.conf` block): the file is in
    /// place, but programs do not see it.
    pub disabled: bool,
    /// Foundry (fontconfig `foundry`) after [`font_foundry_norm`].
    pub foundry: String,
    /// Scripts of the style: values of [`FONT_SCRIPTS`] (without "other": it comes
    /// from the absence of the rest, see [`font_scripts_of`]).
    pub scripts: Vec<&'static str>,
}

/// A family in the «Font Management» list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFamilyGroup {
    pub family: String,
    pub styles: Vec<InstalledFont>,
}

impl FontFamilyGroup {
    /// Whether the family has styles that can be removed.
    pub fn has_user(&self) -> bool {
        self.styles.iter().any(|f| f.user)
    }

    /// Whether there are system styles.
    pub fn has_system(&self) -> bool {
        self.styles.iter().any(|f| !f.user)
    }

    /// Whether there are styles installed for all users
    /// ([`SHARED_FONT_DIR`]).
    pub fn has_shared(&self) -> bool {
        self.styles.iter().any(|f| f.shared)
    }

    /// Whether there are styles from system packages (neither own nor "for all").
    pub fn has_packaged(&self) -> bool {
        self.styles.iter().any(|f| !f.user && !f.shared)
    }

    /// Whether all styles of the family are disabled.
    pub fn all_disabled(&self) -> bool {
        !self.styles.is_empty() && self.styles.iter().all(|f| f.disabled)
    }

    /// Whether at least one style is disabled.
    pub fn any_disabled(&self) -> bool {
        self.styles.iter().any(|f| f.disabled)
    }
}

/// The user's font directories that fontconfig scans.
pub fn font_user_dirs(home: &str, data_home: &str) -> Vec<String> {
    vec![
        format!("{}/fonts", data_home.trim_end_matches('/')),
        format!("{}/.fonts", home.trim_end_matches('/')),
    ]
}

/// The directory where Rexon installs fonts.
pub fn font_install_dir(data_home: &str) -> String {
    format!("{}/fonts", data_home.trim_end_matches('/'))
}

/// Whether the file lies in one of the user's font directories (`..` in the
/// path means no: such a path could leave the directory).
pub fn font_file_is_user(file: &str, user_dirs: &[String]) -> bool {
    if file.split('/').any(|part| part == "..") {
        return false;
    }
    user_dirs
        .iter()
        .map(|d| d.trim_end_matches('/'))
        .filter(|d| !d.is_empty())
        .any(|d| file.strip_prefix(d).is_some_and(|rest| rest.starts_with('/') && rest.len() > 1))
}

/// Whether the file lies in the "for all users" fonts directory
/// ([`SHARED_FONT_DIR`]). For the window this is a hint (the «Remove» button);
/// the root helper's check decides by the real path.
pub fn font_file_is_shared(file: &str) -> bool {
    font_file_is_user(file, &[SHARED_FONT_DIR.to_string()])
}

/// The `fc-list`/`fc-query` format string for the font list: family, style,
/// file, foundry and languages (separated by `|`). The file is third, as it
/// was before the filters: the old three-field output parses the same way.
pub const FC_FONT_FORMAT: &str = "%{family[0]}\t%{style[0]}\t%{file}\t%{foundry}\t%{lang}\n";

/// argv of the list of installed fonts ([`FC_FONT_FORMAT`]).
pub fn fc_list_fonts_argv() -> Vec<String> {
    vec!["fc-list".to_string(), "--format".to_string(), FC_FONT_FORMAT.to_string()]
}

/// argv of the same list for one file: a disabled font that `fc-list` no
/// longer sees.
pub fn fc_query_fonts_argv(file: &str) -> Vec<String> {
    vec!["fc-query".to_string(), "--format".to_string(), FC_FONT_FORMAT.to_string(), file.to_string()]
}

/// Parse the output of [`fc_list_fonts_argv`]. Repeats (one style found
/// twice) are removed; a style without a name is "Regular".
///
/// fontconfig lists a variable font twice: the whole file itself (without a
/// style name) and each named style of the same file. The entry without a
/// name is skipped for such a file; otherwise the family got an extra
/// "Regular", which drew an upright sample for an italic file, and its "✕"
/// removed the italic.
pub fn parse_fc_list_fonts(out: &str, user_dirs: &[String]) -> Vec<InstalledFont> {
    let named_files: BTreeSet<&str> = out
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let (_, style, file) = (parts.next()?, parts.next()?, parts.next()?);
            (!style.trim().is_empty()).then_some(file.trim())
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut fonts = Vec::new();
    for line in out.lines() {
        let mut parts = line.split('\t');
        let (Some(family), Some(style), Some(file)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let foundry = parts.next().unwrap_or("");
        let langs = parts.next().unwrap_or("");
        let (family, file) = (family.trim(), file.trim());
        if family.is_empty() || file.is_empty() {
            continue;
        }
        // Internal X11 bitmap fonts (`cursor.pcf`, `deccurs.pcf`…) have no name:
        // fontconfig returns the file name instead. Text cannot be typed with them,
        // they do not belong in the font list.
        let family_lower = family.to_lowercase();
        if [".pcf", ".pcf.gz", ".bdf"].iter().any(|ext| family_lower.ends_with(ext)) {
            continue;
        }
        if style.trim().is_empty() && named_files.contains(file) {
            continue;
        }
        let style = if style.trim().is_empty() { "Regular" } else { style.trim() };
        if !seen.insert((family.to_string(), style.to_string(), file.to_string())) {
            continue;
        }
        let user = font_file_is_user(file, user_dirs);
        fonts.push(InstalledFont {
            family: family.to_string(),
            style: style.to_string(),
            file: file.to_string(),
            user,
            shared: !user && font_file_is_shared(file),
            disabled: false,
            foundry: font_foundry_norm(foundry),
            scripts: font_scripts_of(langs),
        });
    }
    fonts
}

/// Style weight for the order within a family: from thin to bold, upright
/// before slanted.
fn style_rank(style: &str) -> (i32, bool, String) {
    let s = style.to_lowercase();
    let weight = [
        ("thin", 100),
        ("hairline", 100),
        ("extralight", 200),
        ("ultralight", 200),
        ("light", 300),
        ("book", 400),
        ("regular", 400),
        ("normal", 400),
        ("medium", 500),
        ("semibold", 600),
        ("demibold", 600),
        ("extrabold", 800),
        ("ultrabold", 800),
        ("bold", 700),
        ("black", 900),
        ("heavy", 900),
    ]
    .iter()
    .find(|(k, _)| s.replace([' ', '-'], "").contains(k))
    .map(|(_, w)| *w)
    .unwrap_or(400);
    (weight, s.contains("italic") || s.contains("oblique"), s)
}

/// Group fonts by family; families alphabetically, case-insensitive, styles
/// from thin to bold.
pub fn group_font_families(fonts: &[InstalledFont]) -> Vec<FontFamilyGroup> {
    let mut groups: Vec<FontFamilyGroup> = Vec::new();
    for f in fonts {
        match groups.iter_mut().find(|g| g.family == f.family) {
            Some(g) => g.styles.push(f.clone()),
            None => groups.push(FontFamilyGroup { family: f.family.clone(), styles: vec![f.clone()] }),
        }
    }
    for g in &mut groups {
        g.styles.sort_by_key(|f| style_rank(&f.style));
    }
    groups.sort_by(|a, b| a.family.to_lowercase().cmp(&b.family.to_lowercase()).then(a.family.cmp(&b.family)));
    groups
}

/// Whether the family matches the search string (case-insensitive, by any
/// word of the string).
pub fn font_family_matches(family: &str, query: &str) -> bool {
    let family = family.to_lowercase();
    query.split_whitespace().all(|w| family.contains(&w.to_lowercase()))
}

/// Whether the file fits by extension.
pub fn font_install_ext_ok(path: &str) -> bool {
    let lower = path.to_lowercase();
    FONT_INSTALL_EXTS.iter().any(|e| lower.ends_with(&format!(".{e}")))
}

/// Filter of the font file picker.
pub fn font_install_filter() -> String {
    let mask: Vec<String> = FONT_INSTALL_EXTS.iter().map(|e| format!("*.{e}")).collect();
    // TRANSLATORS: file dialog filter; %1 is the list of file masks.
    format!("{};;{}", trf("Font files (%1)", &[&mask.join(" ")]), tr("All files (*)"))
}

/// Title of the font file picker.
pub fn font_install_dialog_title() -> String {
    tr("Install Font")
}

/// argv of checking a font file: which families fontconfig sees in it.
pub fn fc_query_families_argv(path: &str) -> Vec<String> {
    vec!["fc-query".to_string(), "--format".to_string(), "%{family[0]}\n".to_string(), path.to_string()]
}

/// Families from the output of [`fc_query_families_argv`] (without repeats, in
/// order). Empty means fontconfig does not consider this file a font.
pub fn parse_fc_query_families(out: &str) -> Vec<String> {
    let mut out_list: Vec<String> = Vec::new();
    for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !out_list.iter().any(|f| f == line) {
            out_list.push(line.to_string());
        }
    }
    out_list
}

/// Where the file `source` is installed: `<fonts directory>/<file name>`. `None`
/// means the path has no name.
pub fn font_install_dest(data_home: &str, source: &str) -> Option<String> {
    let name = Path::new(source).file_name()?.to_string_lossy().into_owned();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(format!("{}/{name}", font_install_dir(data_home)))
}

/// argv of rebuilding the fontconfig cache for a directory.
pub fn fc_cache_argv(dir: &str) -> Vec<String> {
    vec!["fc-cache".to_string(), "-f".to_string(), dir.to_string()]
}

/// Where to install a font: values of the "for me / for all" choice.
pub const FONT_INSTALL_SCOPES: &[&str] = &["user", "all"];

/// Whether fonts can be installed "for all" on this system: on NixOS the
/// system fonts are described by its configuration, on SteamOS the system
/// partition is read-only; the root helper would refuse there, and the window does not offer the choice.
pub fn font_shared_supported(family: crate::system::PkgFamily, steamos: bool) -> bool {
    family != crate::system::PkgFamily::Nix && !steamos
}

/// Options for whom the font is installed: `(value, caption)`. Without
/// `shared_ok` ([`font_shared_supported`]) only "for me".
pub fn font_install_scope_choices(shared_ok: bool) -> Vec<(&'static str, String)> {
    FONT_INSTALL_SCOPES
        .iter()
        .filter(|id| shared_ok || !font_install_scope_is_all(id))
        .map(|id| {
            let label = match *id {
                // TRANSLATORS: who a new font is installed for.
                "all" => trc("font install scope", "For all users"),
                // TRANSLATORS: who a new font is installed for.
                _ => trc("font install scope", "Just for me"),
            };
            (*id, label)
        })
        .collect()
}

/// Whether to install for all users (an unknown value means for me: no
/// password and no touching the system).
pub fn font_install_scope_is_all(scope: &str) -> bool {
    scope == "all"
}

/// Where a font "for all" goes: `<SHARED_FONT_DIR>/<file name>`. `None` means
/// the path has no name.
pub fn font_shared_dest(source: &str) -> Option<String> {
    let name = Path::new(source).file_name()?.to_string_lossy().into_owned();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(format!("{SHARED_FONT_DIR}/{name}"))
}

/// How a font install ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontInstallOutcome {
    /// Installed; the families from the file.
    Installed(Vec<String>),
    /// Installed for all users; the families from the file.
    InstalledForAll(Vec<String>),
    /// Not installed for all: a ready text for a person (the helper's check
    /// refused, the password was not entered, there is no helper).
    NotForAll(String),
    /// A file with this name is already installed (the very same one).
    AlreadyInstalled(String),
    /// A file with this name is already installed, but a different one.
    NameTaken(String),
    /// fontconfig does not consider the file a font (or this kind of font is not built in).
    NotAFont(String),
    /// Not copied (no space, no permission).
    CopyFailed(String),
}

/// Result of the install, for a person.
pub fn font_install_status(outcome: &FontInstallOutcome) -> String {
    match outcome {
        // TRANSLATORS: %1 is a font family name or a comma-separated list of them.
        FontInstallOutcome::Installed(families) => trf("✅ Font installed: %1", &[&families.join(", ")]),
        // TRANSLATORS: %1 is a font family name or a comma-separated list of them.
        FontInstallOutcome::InstalledForAll(families) => {
            trf("✅ Font installed for all users: %1", &[&families.join(", ")])
        }
        FontInstallOutcome::NotForAll(text) => format!("⚠️ {text}"),
        // TRANSLATORS: %1 is a font file name.
        FontInstallOutcome::AlreadyInstalled(name) => trf("%1 is already installed", &[name]),
        // TRANSLATORS: %1 is a font file name.
        FontInstallOutcome::NameTaken(name) => trf(
            "⚠️ A different font file named %1 is already installed — remove it first or rename the new one",
            &[name],
        ),
        // TRANSLATORS: %1 is a file name.
        FontInstallOutcome::NotAFont(name) => {
            trf("⚠️ %1 is not a font this system can read — nothing was installed", &[name])
        }
        // TRANSLATORS: %1 is a file name.
        FontInstallOutcome::CopyFailed(name) => trf("⚠️ Could not copy %1 to your fonts folder", &[name]),
    }
}

/// The question before removal: `(heading, text)`. `families` are the families
/// of the files being removed (a `.ttc` may hold several in one file, and
/// they go together).
pub fn font_remove_confirm(family: &str, files: usize, families: &[String]) -> (String, String) {
    let mut text = crate::i18n::fill(
        &ntr(
            // TRANSLATORS: %1 is a font family, %n the number of font files.
            "Remove the font %1 (%n file)?",
            "Remove the font %1 (%n files)?",
            files as u64,
        ),
        &[&family],
    );
    let others: Vec<&String> = families.iter().filter(|f| *f != family).collect();
    if !others.is_empty() {
        text.push(' ');
        let list: Vec<&str> = others.iter().map(|s| s.as_str()).collect();
        // TRANSLATORS: %1 is a comma-separated list of font families.
        text.push_str(&trf("The same files also hold %1 — they will be removed too.", &[&list.join(", ")]));
    }
    text.push(' ');
    text.push_str(&tr("Apps that use it will fall back to another font."));
    (tr("Remove Font"), text)
}

/// Result of the removal.
pub fn font_remove_status(family: &str, removed: usize, failed: usize) -> String {
    if failed > 0 {
        // TRANSLATORS: %1 is a font family.
        return trf("⚠️ Could not remove some files of %1", &[&family]);
    }
    if removed == 0 {
        return tr(
            "Only fonts installed from a file can be removed here; fonts that came with the system or a \
             package are removed together with that package",
        );
    }
    // TRANSLATORS: %1 is a font family.
    trf("✅ Font removed: %1", &[&family])
}

/// An addition to the question before removal if some of the files were
/// installed for all users.
pub fn font_remove_shared_note() -> String {
    tr("It is installed for all users, so Rexon will ask for the administrator password.")
}

/// Captions of the «Font Management» section.
pub fn font_manager_texts() -> Vec<(&'static str, String)> {
    vec![
        ("open", tr("🗂️ Manage Fonts…")),
        ("open_tip", tr("Installed fonts: preview, install from a file, remove your own")),
        ("close", tr("Hide Font List")),
        ("search", tr("Search fonts…")),
        ("install", tr("➕ Install from File…")),
        ("install_tip", tr("Copy a font file to your fonts folder so every app can use it")),
        (
            "install_all_tip",
            tr("Install the font for everyone who uses this computer; Rexon will ask for the administrator password"),
        ),
        ("scope_tip", tr("Who the new font is for")),
        ("remove", tr("Remove")),
        ("remove_tip", tr("Remove this font from your fonts folder")),
        ("system", tr("System font — view only: it came with the system or a package")),
        ("shared", tr("Installed for all users — removing it asks for the administrator password")),
        ("user", tr("Installed by you")),
        ("empty", tr("No fonts match the search")),
        ("pick", tr("Choose a font on the left to see its styles")),
        ("sample", tr("The quick brown fox jumps over the lazy dog 0123456789")),
        ("no_fclist", tr("fontconfig (fc-list) isn't available, so the font list can't be shown")),
        ("disable", trc("font action", "Disable")),
        ("disable_tip", tr("Hide this font from all apps without deleting the file; you can turn it back on any time")),
        ("enable", trc("font action", "Enable")),
        ("enable_tip", tr("Let apps use this font again")),
        // TRANSLATORS: short badge next to a disabled font in the list.
        ("off", trc("font state", "off")),
        ("disabled_note", tr("Disabled: apps don't see this font, the file stays in place")),
        // TRANSLATORS: %1 is the sample text size in points; keep %1.
        ("sample_size", tr("Sample size: %1 pt")),
        ("duplicates", tr("🔍 Find Duplicates")),
        ("duplicates_tip", tr("Show font styles that are installed in more than one file")),
        ("duplicates_close", tr("Back to Font List")),
        (
            "duplicates_note",
            tr("Apps pick any one of the copies. Your extra copies can be removed; system copies belong to the system and stay."),
        ),
        ("copy_system", tr("System copy")),
        ("copy_shared", tr("Copy for all users")),
        ("filter_foundry_tip", tr("Foundry: the company or project that made the font")),
        ("filter_type_tip", tr("Font file type")),
        ("filter_script_tip", tr("Writing system: show only fonts that can write in it")),
        ("sample_text_tip", tr("Your own text for the samples; Rexon remembers it. Leave empty for the default text.")),
        ("sample_reset", tr("Default Text")),
        // TRANSLATORS: switch in the font preview: text samples of every style.
        ("view_sample", trc("font preview", "Samples")),
        // TRANSLATORS: switch in the font preview: table of the font's characters.
        ("view_chars", trc("font preview", "Characters")),
        ("chars_style_tip", tr("Style whose characters are shown")),
        ("chars_block_tip", tr("Unicode block")),
        ("chars_none", tr("No characters to show: fontconfig couldn't read this font file")),
        ("char_pick", tr("Click a character to see its code and copy it")),
        ("groups_tip", tr("Pick a group to show its fonts. Drag a font from the list onto one of your groups to add it")),
        ("group_new", tr("➕ New Group…")),
        ("group_new_tip", tr("Your own group of fonts, shared with KDE Font Management")),
        ("group_name", tr("Group name")),
        ("group_create", tr("Create")),
        ("group_cancel", tr("Cancel")),
        ("group_rename", tr("✏️ Rename…")),
        ("group_rename_save", tr("Rename")),
        ("group_remove", tr("🗑️ Remove Group")),
        ("group_remove_tip", tr("Removes only the group — the fonts stay installed")),
        ("group_enable", tr("Enable Fonts in Group")),
        ("group_disable", tr("Disable Fonts in Group")),
        ("group_toggle_tip", tr("Turn every font of this group on or off at once, without removing anything")),
        ("group_export", tr("📦 Export…")),
        ("group_export_tip", tr("Save the font files of this group into a ZIP archive")),
        ("group_add", tr("➕ Add to Group…")),
        ("group_add_tip", tr("Add this font family to one of your groups")),
        ("group_take", tr("➖ Remove from Group")),
        ("group_take_tip", tr("Take this font family out of the group; the font stays installed")),
        (
            "group_empty",
            tr("No fonts in this group match the filters yet. Add fonts with “Add to Group…” or drag them onto the group"),
        ),
    ]
}

/// Summary above the list: how many families, how many of them are own.
pub fn font_manager_summary(families: usize, user_families: usize) -> String {
    crate::i18n::fill(
        &ntr(
            // TRANSLATORS: %n is the number of font families, %1 how many of them the user installed.
            "%n font family, yours: %1",
            "%n font families, yours: %1",
            families as u64,
        ),
        &[&user_families],
    )
}

// ── «All / Personal / System» filter, search, sample size ──────────────

/// Values of the font list filter by location: all, personal (the user's
/// font directories), system (everything else fontconfig sees).
pub const FONT_ORIGINS: &[&str] = &["all", "user", "system"];

/// Filter options for the list: `(value, caption)`.
pub fn font_origin_choices() -> Vec<(&'static str, String)> {
    FONT_ORIGINS
        .iter()
        .map(|id| {
            let label = match *id {
                "user" => trc("font origin", "Personal"),
                "system" => trc("font origin", "System"),
                _ => trc("font origin", "All"),
            };
            (*id, label)
        })
        .collect()
}

/// Sample size in «Font Management»: smallest, largest and initial (points),
/// as in the KDE preview.
pub const FONT_SAMPLE_SIZE: (i32, i32, i32) = (12, 48, 16);

/// What to show in the list: styles matching the filter `origin` (`FONT_ORIGINS`;
/// an unknown one means "all") and the search string. A search word that is
/// not in the family name is looked for in the style name: "inter italic"
/// leaves only the slanted styles of Inter. A family without matching styles is not shown.
pub fn font_group_view(groups: &[FontFamilyGroup], origin: &str, query: &str) -> Vec<FontFamilyGroup> {
    let filter = FontListFilter { origin: origin.to_string(), query: query.to_string(), ..Default::default() };
    font_group_view_filtered(groups, &filter)
}

// ── List filters: foundry, file type, script ────────────────────────────
//
// The set is as in KDE's «Font Management» filter: foundry (fontconfig
// `foundry`), file type (by extension: TrueType, OpenType, collection, Type 1,
// PCF, BDF; Rexon also has WOFF, which its install dialog installs), script
// (fontconfig `lang` languages; where fontconfig does not know the language
// or knows it not for all fonts, a probe with one character through
// `fc-list :charset=…`). The filters combine with each other, with the
// location («All / Personal / System») and with the search string instead of
// replacing each other: "Cyrillic + OpenType" is a common request.

/// A script and the fontconfig language it is recognized by: a font matches if
/// its `lang` has this language with any territory (`zh-tw` matches
/// "Simplified Chinese" too: fontconfig likewise considers languages equal up
/// to the territory).
const FONT_SCRIPT_LANGS: &[(&str, &str)] = &[
    ("latin", "en"),
    ("greek", "el"),
    ("cyrillic", "ru"),
    ("armenian", "hy"),
    ("hebrew", "he"),
    ("arabic", "ar"),
    ("syriac", "syr"),
    ("thaana", "div"),
    ("devanagari", "hi"),
    ("bengali", "bn"),
    ("gurmukhi", "pa"),
    ("gujarati", "gu"),
    ("oriya", "or"),
    ("tamil", "ta"),
    ("telugu", "te"),
    ("kannada", "kn"),
    ("malayalam", "ml"),
    ("sinhala", "si"),
    ("thai", "th"),
    ("lao", "lo"),
    ("tibetan", "bo"),
    ("myanmar", "my"),
    ("georgian", "ka"),
    ("khmer", "km"),
    ("chinese_simplified", "zh-cn"),
    ("chinese_traditional", "zh-tw"),
    ("japanese", "ja"),
    ("korean", "ko"),
    ("vietnamese", "vi"),
];

/// Scripts recognized by one character (`fc-list :charset=<code>`): Ogham and
/// Runic have no language in fontconfig at all, the others have one not in all versions.
pub const FONT_SCRIPT_PROBES: &[(&str, u32)] = &[
    ("telugu", 0x0C15),
    ("kannada", 0x0C95),
    ("malayalam", 0x0D15),
    ("sinhala", 0x0D9A),
    ("myanmar", 0x1000),
    ("ogham", 0x1681),
    ("runic", 0x16A0),
];

/// All values of the script filter, in the order stored on a font.
/// "other" is "symbols and other": a font that matched none.
pub const FONT_SCRIPTS: &[&str] = &[
    "latin",
    "greek",
    "cyrillic",
    "armenian",
    "hebrew",
    "arabic",
    "syriac",
    "thaana",
    "devanagari",
    "bengali",
    "gurmukhi",
    "gujarati",
    "oriya",
    "tamil",
    "telugu",
    "kannada",
    "malayalam",
    "sinhala",
    "thai",
    "lao",
    "tibetan",
    "myanmar",
    "georgian",
    "khmer",
    "chinese_simplified",
    "chinese_traditional",
    "japanese",
    "korean",
    "vietnamese",
    "ogham",
    "runic",
    "other",
];

/// The main part of a language tag: `zh-tw` → `zh`.
fn lang_primary(lang: &str) -> &str {
    lang.split('-').next().unwrap_or(lang)
}

/// Scripts by the `%{lang}` field (languages separated by `|`).
pub fn font_scripts_of(langs: &str) -> Vec<&'static str> {
    let have: BTreeSet<String> = langs
        .split('|')
        .map(|l| lang_primary(l.trim()).to_ascii_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
    FONT_SCRIPT_LANGS
        .iter()
        .filter(|(_, lang)| have.contains(lang_primary(lang)))
        .map(|(id, _)| *id)
        .collect()
}

/// argv of a script probe: font files that contain the character `ch`.
pub fn fc_list_char_argv(ch: u32) -> Vec<String> {
    vec![
        "fc-list".to_string(),
        "--format".to_string(),
        "%{file}\n".to_string(),
        format!(":charset={ch:x}"),
    ]
}

/// Add the script `script` to the fonts whose files the probe found
/// ([`fc_list_char_argv`]). The order of scripts on a font is as in
/// [`FONT_SCRIPTS`], without repeats.
pub fn font_add_probed_script(fonts: &mut [InstalledFont], script: &str, probe_out: &str) {
    let Some(id) = FONT_SCRIPTS.iter().find(|s| **s == script && **s != "other") else {
        return;
    };
    let files: BTreeSet<&str> = probe_out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    for f in fonts.iter_mut().filter(|f| files.contains(f.file.as_str())) {
        if !f.scripts.contains(id) {
            f.scripts.push(id);
            f.scripts.sort_by_key(|s| FONT_SCRIPTS.iter().position(|x| x == s));
        }
    }
}

/// Caption of a script.
pub fn font_script_label(id: &str) -> String {
    match id {
        "latin" => tr("Latin"),
        "greek" => tr("Greek"),
        "cyrillic" => tr("Cyrillic"),
        "armenian" => tr("Armenian"),
        "hebrew" => tr("Hebrew"),
        "arabic" => tr("Arabic"),
        "syriac" => tr("Syriac"),
        "thaana" => tr("Thaana"),
        "devanagari" => tr("Devanagari"),
        "bengali" => tr("Bengali"),
        "gurmukhi" => tr("Gurmukhi"),
        "gujarati" => tr("Gujarati"),
        "oriya" => tr("Oriya"),
        "tamil" => tr("Tamil"),
        "telugu" => tr("Telugu"),
        "kannada" => tr("Kannada"),
        "malayalam" => tr("Malayalam"),
        "sinhala" => tr("Sinhala"),
        "thai" => tr("Thai"),
        "lao" => tr("Lao"),
        "tibetan" => tr("Tibetan"),
        "myanmar" => tr("Myanmar"),
        "georgian" => tr("Georgian"),
        "khmer" => tr("Khmer"),
        "chinese_simplified" => tr("Simplified Chinese"),
        "chinese_traditional" => tr("Traditional Chinese"),
        "japanese" => tr("Japanese"),
        "korean" => tr("Korean"),
        "vietnamese" => tr("Vietnamese"),
        "ogham" => tr("Ogham"),
        "runic" => tr("Runic"),
        _ => tr("Symbols and other"),
    }
}

/// File types of the filter and their extensions (case-insensitive).
pub const FONT_FILE_TYPES: &[(&str, &[&str])] = &[
    ("truetype", &["ttf"]),
    ("opentype", &["otf"]),
    ("collection", &["ttc", "otc"]),
    ("type1", &["pfa", "pfb", "t1", "gsf", "pfa.gz", "pfb.gz"]),
    ("pcf", &["pcf", "pcf.gz"]),
    ("bdf", &["bdf", "bdf.gz"]),
    ("woff", &["woff", "woff2"]),
];

/// Font file type by extension (`None` means none of [`FONT_FILE_TYPES`]).
pub fn font_file_type(file: &str) -> Option<&'static str> {
    let name = file.rsplit('/').next().unwrap_or(file).to_lowercase();
    FONT_FILE_TYPES
        .iter()
        .find(|(_, exts)| exts.iter().any(|e| name.ends_with(&format!(".{e}"))))
        .map(|(id, _)| *id)
}

/// Caption of a file type.
pub fn font_file_type_label(id: &str) -> String {
    match id {
        "truetype" => tr("TrueType"),
        "opentype" => tr("OpenType"),
        "collection" => tr("Font collection (TTC/OTC)"),
        "type1" => tr("Type 1 (PostScript)"),
        "pcf" => tr("PCF bitmap font"),
        "bdf" => tr("BDF bitmap font"),
        "woff" => tr("Web font (WOFF)"),
        _ => id.to_string(),
    }
}

/// The "unspecified" foundry value.
pub const FONT_FOUNDRY_UNKNOWN: &str = "unknown";

/// The foundry as it is to be shown and compared: without spaces at the edges
/// (the OS/2 vendor code is exactly 4 characters, short ones are padded with
/// spaces); empty ones and placeholders (`unknown`, `UKWN`, `NONE`) are "unspecified".
pub fn font_foundry_norm(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() || ["unknown", "ukwn", "none"].contains(&t.to_ascii_lowercase().as_str()) {
        FONT_FOUNDRY_UNKNOWN.to_string()
    } else {
        t.to_string()
    }
}

/// The whole font list filter. An empty field means "any".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontListFilter {
    /// [`FONT_ORIGINS`]; empty and unknown mean "all".
    pub origin: String,
    pub query: String,
    /// Foundry (case-insensitive) or [`FONT_FOUNDRY_UNKNOWN`].
    pub foundry: String,
    /// A value of [`FONT_FILE_TYPES`].
    pub file_type: String,
    /// A value of [`FONT_SCRIPTS`].
    pub script: String,
}

impl FontListFilter {
    /// From the page's JSON: `{"origin", "query", "foundry", "type", "script"}`;
    /// whatever is missing or not a string means "any".
    pub fn from_json(text: &str) -> Self {
        let v: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
        let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        FontListFilter {
            origin: get("origin"),
            query: get("query"),
            foundry: get("foundry"),
            file_type: get("type"),
            script: get("script"),
        }
    }

    /// Whether a style matches the foundry, type and script.
    pub fn matches_font(&self, f: &InstalledFont) -> bool {
        let foundry_ok = self.foundry.is_empty() || f.foundry.eq_ignore_ascii_case(&self.foundry);
        let type_ok = self.file_type.is_empty() || font_file_type(&f.file) == Some(self.file_type.as_str());
        let script_ok = match self.script.as_str() {
            "" => true,
            "other" => f.scripts.is_empty(),
            s => f.scripts.contains(&s),
        };
        foundry_ok && type_ok && script_ok
    }
}

/// [`font_group_view`] with the foundry, type and script filters.
pub fn font_group_view_filtered(groups: &[FontFamilyGroup], filter: &FontListFilter) -> Vec<FontFamilyGroup> {
    let words: Vec<String> = filter.query.split_whitespace().map(str::to_lowercase).collect();
    groups
        .iter()
        .filter_map(|g| {
            let family = g.family.to_lowercase();
            let rest: Vec<&String> = words.iter().filter(|w| !family.contains(w.as_str())).collect();
            let styles: Vec<InstalledFont> = g
                .styles
                .iter()
                .filter(|f| match filter.origin.as_str() {
                    "user" => f.user,
                    "system" => !f.user,
                    _ => true,
                })
                .filter(|f| {
                    let style = f.style.to_lowercase();
                    rest.iter().all(|w| style.contains(w.as_str()))
                })
                .filter(|f| filter.matches_font(f))
                .cloned()
                .collect();
            (!styles.is_empty()).then(|| FontFamilyGroup { family: g.family.clone(), styles })
        })
        .collect()
}

/// Options of the three filters by what is installed: `(value, caption)`,
/// first "any" with an empty value. There are no empty options: a type and a
/// script that no font has are not offered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontFilterChoices {
    pub foundries: Vec<(String, String)>,
    pub file_types: Vec<(String, String)>,
    pub scripts: Vec<(String, String)>,
}

/// Build [`FontFilterChoices`]. Foundries alphabetically, case-insensitive
/// (identical up to case means one option), "unspecified" last; types in the
/// order of [`FONT_FILE_TYPES`]; scripts by caption, "symbols and other" last.
pub fn font_filter_choices(fonts: &[InstalledFont]) -> FontFilterChoices {
    let mut foundries: Vec<String> = Vec::new();
    let mut unknown = false;
    for f in fonts {
        if f.foundry == FONT_FOUNDRY_UNKNOWN {
            unknown = true;
        } else if !foundries.iter().any(|x| x.eq_ignore_ascii_case(&f.foundry)) {
            foundries.push(f.foundry.clone());
        }
    }
    foundries.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)));
    let mut foundry_choices = vec![(String::new(), tr("Any foundry"))];
    foundry_choices.extend(foundries.into_iter().map(|f| (f.clone(), f)));
    if unknown {
        foundry_choices.push((FONT_FOUNDRY_UNKNOWN.to_string(), tr("Foundry not specified")));
    }

    let mut type_choices = vec![(String::new(), tr("Any file type"))];
    for (id, _) in FONT_FILE_TYPES {
        if fonts.iter().any(|f| font_file_type(&f.file) == Some(*id)) {
            type_choices.push((id.to_string(), font_file_type_label(id)));
        }
    }

    let mut scripts: Vec<(String, String)> = FONT_SCRIPTS
        .iter()
        .filter(|id| **id != "other" && fonts.iter().any(|f| f.scripts.contains(id)))
        .map(|id| (id.to_string(), font_script_label(id)))
        .collect();
    scripts.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    let mut script_choices = vec![(String::new(), tr("Any writing system"))];
    script_choices.extend(scripts);
    if fonts.iter().any(|f| f.scripts.is_empty()) {
        script_choices.push(("other".to_string(), font_script_label("other")));
    }
    FontFilterChoices { foundries: foundry_choices, file_types: type_choices, scripts: script_choices }
}

// ── Character table and a custom sample text ────────────────────────────
//
// As KDE's «Preview» in "Unicode block" mode: the characters present in the
// font file (fontconfig `%{charset}`), by Unicode block. The blocks are the
// whole basic plane and those supplementary blocks that occur in ordinary
// fonts (Latin and Cyrillic, emoji, math, cards, private icon areas);
// everything else the font has goes into one «Other characters» block,
// nothing is lost.

/// Unicode blocks (Blocks.txt, Unicode 17.0): start, end, name.
/// In ascending order of start: the block lookup is a binary search.
pub const UNICODE_BLOCKS: &[(u32, u32, &str)] = &[
    (0x0000, 0x007F, n_("Basic Latin")),
    (0x0080, 0x00FF, n_("Latin-1 Supplement")),
    (0x0100, 0x017F, n_("Latin Extended-A")),
    (0x0180, 0x024F, n_("Latin Extended-B")),
    (0x0250, 0x02AF, n_("IPA Extensions")),
    (0x02B0, 0x02FF, n_("Spacing Modifier Letters")),
    (0x0300, 0x036F, n_("Combining Diacritical Marks")),
    (0x0370, 0x03FF, n_("Greek and Coptic")),
    (0x0400, 0x04FF, n_("Cyrillic")),
    (0x0500, 0x052F, n_("Cyrillic Supplement")),
    (0x0530, 0x058F, n_("Armenian")),
    (0x0590, 0x05FF, n_("Hebrew")),
    (0x0600, 0x06FF, n_("Arabic")),
    (0x0700, 0x074F, n_("Syriac")),
    (0x0750, 0x077F, n_("Arabic Supplement")),
    (0x0780, 0x07BF, n_("Thaana")),
    (0x07C0, 0x07FF, n_("NKo")),
    (0x0800, 0x083F, n_("Samaritan")),
    (0x0840, 0x085F, n_("Mandaic")),
    (0x0860, 0x086F, n_("Syriac Supplement")),
    (0x0870, 0x089F, n_("Arabic Extended-B")),
    (0x08A0, 0x08FF, n_("Arabic Extended-A")),
    (0x0900, 0x097F, n_("Devanagari")),
    (0x0980, 0x09FF, n_("Bengali")),
    (0x0A00, 0x0A7F, n_("Gurmukhi")),
    (0x0A80, 0x0AFF, n_("Gujarati")),
    (0x0B00, 0x0B7F, n_("Oriya")),
    (0x0B80, 0x0BFF, n_("Tamil")),
    (0x0C00, 0x0C7F, n_("Telugu")),
    (0x0C80, 0x0CFF, n_("Kannada")),
    (0x0D00, 0x0D7F, n_("Malayalam")),
    (0x0D80, 0x0DFF, n_("Sinhala")),
    (0x0E00, 0x0E7F, n_("Thai")),
    (0x0E80, 0x0EFF, n_("Lao")),
    (0x0F00, 0x0FFF, n_("Tibetan")),
    (0x1000, 0x109F, n_("Myanmar")),
    (0x10A0, 0x10FF, n_("Georgian")),
    (0x1100, 0x11FF, n_("Hangul Jamo")),
    (0x1200, 0x137F, n_("Ethiopic")),
    (0x1380, 0x139F, n_("Ethiopic Supplement")),
    (0x13A0, 0x13FF, n_("Cherokee")),
    (0x1400, 0x167F, n_("Unified Canadian Aboriginal Syllabics")),
    (0x1680, 0x169F, n_("Ogham")),
    (0x16A0, 0x16FF, n_("Runic")),
    (0x1700, 0x171F, n_("Tagalog")),
    (0x1720, 0x173F, n_("Hanunoo")),
    (0x1740, 0x175F, n_("Buhid")),
    (0x1760, 0x177F, n_("Tagbanwa")),
    (0x1780, 0x17FF, n_("Khmer")),
    (0x1800, 0x18AF, n_("Mongolian")),
    (0x18B0, 0x18FF, n_("Unified Canadian Aboriginal Syllabics Extended")),
    (0x1900, 0x194F, n_("Limbu")),
    (0x1950, 0x197F, n_("Tai Le")),
    (0x1980, 0x19DF, n_("New Tai Lue")),
    (0x19E0, 0x19FF, n_("Khmer Symbols")),
    (0x1A00, 0x1A1F, n_("Buginese")),
    (0x1A20, 0x1AAF, n_("Tai Tham")),
    (0x1AB0, 0x1AFF, n_("Combining Diacritical Marks Extended")),
    (0x1B00, 0x1B7F, n_("Balinese")),
    (0x1B80, 0x1BBF, n_("Sundanese")),
    (0x1BC0, 0x1BFF, n_("Batak")),
    (0x1C00, 0x1C4F, n_("Lepcha")),
    (0x1C50, 0x1C7F, n_("Ol Chiki")),
    (0x1C80, 0x1C8F, n_("Cyrillic Extended-C")),
    (0x1C90, 0x1CBF, n_("Georgian Extended")),
    (0x1CC0, 0x1CCF, n_("Sundanese Supplement")),
    (0x1CD0, 0x1CFF, n_("Vedic Extensions")),
    (0x1D00, 0x1D7F, n_("Phonetic Extensions")),
    (0x1D80, 0x1DBF, n_("Phonetic Extensions Supplement")),
    (0x1DC0, 0x1DFF, n_("Combining Diacritical Marks Supplement")),
    (0x1E00, 0x1EFF, n_("Latin Extended Additional")),
    (0x1F00, 0x1FFF, n_("Greek Extended")),
    (0x2000, 0x206F, n_("General Punctuation")),
    (0x2070, 0x209F, n_("Superscripts and Subscripts")),
    (0x20A0, 0x20CF, n_("Currency Symbols")),
    (0x20D0, 0x20FF, n_("Combining Diacritical Marks for Symbols")),
    (0x2100, 0x214F, n_("Letterlike Symbols")),
    (0x2150, 0x218F, n_("Number Forms")),
    (0x2190, 0x21FF, n_("Arrows")),
    (0x2200, 0x22FF, n_("Mathematical Operators")),
    (0x2300, 0x23FF, n_("Miscellaneous Technical")),
    (0x2400, 0x243F, n_("Control Pictures")),
    (0x2440, 0x245F, n_("Optical Character Recognition")),
    (0x2460, 0x24FF, n_("Enclosed Alphanumerics")),
    (0x2500, 0x257F, n_("Box Drawing")),
    (0x2580, 0x259F, n_("Block Elements")),
    (0x25A0, 0x25FF, n_("Geometric Shapes")),
    (0x2600, 0x26FF, n_("Miscellaneous Symbols")),
    (0x2700, 0x27BF, n_("Dingbats")),
    (0x27C0, 0x27EF, n_("Miscellaneous Mathematical Symbols-A")),
    (0x27F0, 0x27FF, n_("Supplemental Arrows-A")),
    (0x2800, 0x28FF, n_("Braille Patterns")),
    (0x2900, 0x297F, n_("Supplemental Arrows-B")),
    (0x2980, 0x29FF, n_("Miscellaneous Mathematical Symbols-B")),
    (0x2A00, 0x2AFF, n_("Supplemental Mathematical Operators")),
    (0x2B00, 0x2BFF, n_("Miscellaneous Symbols and Arrows")),
    (0x2C00, 0x2C5F, n_("Glagolitic")),
    (0x2C60, 0x2C7F, n_("Latin Extended-C")),
    (0x2C80, 0x2CFF, n_("Coptic")),
    (0x2D00, 0x2D2F, n_("Georgian Supplement")),
    (0x2D30, 0x2D7F, n_("Tifinagh")),
    (0x2D80, 0x2DDF, n_("Ethiopic Extended")),
    (0x2DE0, 0x2DFF, n_("Cyrillic Extended-A")),
    (0x2E00, 0x2E7F, n_("Supplemental Punctuation")),
    (0x2E80, 0x2EFF, n_("CJK Radicals Supplement")),
    (0x2F00, 0x2FDF, n_("Kangxi Radicals")),
    (0x2FF0, 0x2FFF, n_("Ideographic Description Characters")),
    (0x3000, 0x303F, n_("CJK Symbols and Punctuation")),
    (0x3040, 0x309F, n_("Hiragana")),
    (0x30A0, 0x30FF, n_("Katakana")),
    (0x3100, 0x312F, n_("Bopomofo")),
    (0x3130, 0x318F, n_("Hangul Compatibility Jamo")),
    (0x3190, 0x319F, n_("Kanbun")),
    (0x31A0, 0x31BF, n_("Bopomofo Extended")),
    (0x31C0, 0x31EF, n_("CJK Strokes")),
    (0x31F0, 0x31FF, n_("Katakana Phonetic Extensions")),
    (0x3200, 0x32FF, n_("Enclosed CJK Letters and Months")),
    (0x3300, 0x33FF, n_("CJK Compatibility")),
    (0x3400, 0x4DBF, n_("CJK Unified Ideographs Extension A")),
    (0x4DC0, 0x4DFF, n_("Yijing Hexagram Symbols")),
    (0x4E00, 0x9FFF, n_("CJK Unified Ideographs")),
    (0xA000, 0xA48F, n_("Yi Syllables")),
    (0xA490, 0xA4CF, n_("Yi Radicals")),
    (0xA4D0, 0xA4FF, n_("Lisu")),
    (0xA500, 0xA63F, n_("Vai")),
    (0xA640, 0xA69F, n_("Cyrillic Extended-B")),
    (0xA6A0, 0xA6FF, n_("Bamum")),
    (0xA700, 0xA71F, n_("Modifier Tone Letters")),
    (0xA720, 0xA7FF, n_("Latin Extended-D")),
    (0xA800, 0xA82F, n_("Syloti Nagri")),
    (0xA830, 0xA83F, n_("Common Indic Number Forms")),
    (0xA840, 0xA87F, n_("Phags-pa")),
    (0xA880, 0xA8DF, n_("Saurashtra")),
    (0xA8E0, 0xA8FF, n_("Devanagari Extended")),
    (0xA900, 0xA92F, n_("Kayah Li")),
    (0xA930, 0xA95F, n_("Rejang")),
    (0xA960, 0xA97F, n_("Hangul Jamo Extended-A")),
    (0xA980, 0xA9DF, n_("Javanese")),
    (0xA9E0, 0xA9FF, n_("Myanmar Extended-B")),
    (0xAA00, 0xAA5F, n_("Cham")),
    (0xAA60, 0xAA7F, n_("Myanmar Extended-A")),
    (0xAA80, 0xAADF, n_("Tai Viet")),
    (0xAAE0, 0xAAFF, n_("Meetei Mayek Extensions")),
    (0xAB00, 0xAB2F, n_("Ethiopic Extended-A")),
    (0xAB30, 0xAB6F, n_("Latin Extended-E")),
    (0xAB70, 0xABBF, n_("Cherokee Supplement")),
    (0xABC0, 0xABFF, n_("Meetei Mayek")),
    (0xAC00, 0xD7AF, n_("Hangul Syllables")),
    (0xD7B0, 0xD7FF, n_("Hangul Jamo Extended-B")),
    (0xE000, 0xF8FF, n_("Private Use Area")),
    (0xF900, 0xFAFF, n_("CJK Compatibility Ideographs")),
    (0xFB00, 0xFB4F, n_("Alphabetic Presentation Forms")),
    (0xFB50, 0xFDFF, n_("Arabic Presentation Forms-A")),
    (0xFE00, 0xFE0F, n_("Variation Selectors")),
    (0xFE10, 0xFE1F, n_("Vertical Forms")),
    (0xFE20, 0xFE2F, n_("Combining Half Marks")),
    (0xFE30, 0xFE4F, n_("CJK Compatibility Forms")),
    (0xFE50, 0xFE6F, n_("Small Form Variants")),
    (0xFE70, 0xFEFF, n_("Arabic Presentation Forms-B")),
    (0xFF00, 0xFFEF, n_("Halfwidth and Fullwidth Forms")),
    (0xFFF0, 0xFFFF, n_("Specials")),
    (0x10780, 0x107BF, n_("Latin Extended-F")),
    (0x1D100, 0x1D1FF, n_("Musical Symbols")),
    (0x1D400, 0x1D7FF, n_("Mathematical Alphanumeric Symbols")),
    (0x1DF00, 0x1DFFF, n_("Latin Extended-G")),
    (0x1E030, 0x1E08F, n_("Cyrillic Extended-D")),
    (0x1F000, 0x1F02F, n_("Mahjong Tiles")),
    (0x1F030, 0x1F09F, n_("Domino Tiles")),
    (0x1F0A0, 0x1F0FF, n_("Playing Cards")),
    (0x1F100, 0x1F1FF, n_("Enclosed Alphanumeric Supplement")),
    (0x1F200, 0x1F2FF, n_("Enclosed Ideographic Supplement")),
    (0x1F300, 0x1F5FF, n_("Miscellaneous Symbols and Pictographs")),
    (0x1F600, 0x1F64F, n_("Emoticons")),
    (0x1F650, 0x1F67F, n_("Ornamental Dingbats")),
    (0x1F680, 0x1F6FF, n_("Transport and Map Symbols")),
    (0x1F700, 0x1F77F, n_("Alchemical Symbols")),
    (0x1F780, 0x1F7FF, n_("Geometric Shapes Extended")),
    (0x1F800, 0x1F8FF, n_("Supplemental Arrows-C")),
    (0x1F900, 0x1F9FF, n_("Supplemental Symbols and Pictographs")),
    (0x1FA00, 0x1FA6F, n_("Chess Symbols")),
    (0x1FA70, 0x1FAFF, n_("Symbols and Pictographs Extended-A")),
    (0x1FB00, 0x1FBFF, n_("Symbols for Legacy Computing")),
    (0xF0000, 0xFFFFF, n_("Supplementary Private Use Area-A")),
    (0x100000, 0x10FFFF, n_("Supplementary Private Use Area-B")),
];

/// argv of a file's character set: the style and the `charset` of each face.
pub fn fc_query_charset_argv(file: &str) -> Vec<String> {
    vec![
        "fc-query".to_string(),
        "--format".to_string(),
        "%{style[0]}\t%{charset}\n".to_string(),
        file.to_string(),
    ]
}

/// Parse a fontconfig `charset`: hexadecimal codes and ranges separated by
/// spaces (`20-7e a0 401-44f`). Anything unparseable is skipped.
pub fn parse_fc_charset(text: &str) -> Vec<(u32, u32)> {
    text.split_whitespace()
        .filter_map(|tok| {
            let (a, b) = tok.split_once('-').unwrap_or((tok, tok));
            let (a, b) = (u32::from_str_radix(a, 16).ok()?, u32::from_str_radix(b, 16).ok()?);
            (a <= b && b <= 0x10FFFF).then_some((a, b))
        })
        .collect()
}

/// The character set of the wanted style from the output of [`fc_query_charset_argv`]:
/// the face with the same style name (case-insensitive), otherwise the first one.
pub fn fc_charset_for_style(out: &str, style: &str) -> Vec<(u32, u32)> {
    let faces: Vec<(&str, &str)> = out.lines().filter_map(|l| l.split_once('\t')).collect();
    let face = faces
        .iter()
        .find(|(s, _)| s.trim().eq_ignore_ascii_case(style.trim()))
        .or_else(|| faces.first());
    face.map(|(_, cs)| parse_fc_charset(cs)).unwrap_or_default()
}

/// Whether a character is shown in the table: control characters, surrogates
/// and Unicode "noncharacters" are not (there is nothing to draw).
pub fn font_char_printable(cp: u32) -> bool {
    !(cp < 0x20
        || (0x7F..=0x9F).contains(&cp)
        || (0xD800..=0xDFFF).contains(&cp)
        || (0xFDD0..=0xFDEF).contains(&cp)
        || cp & 0xFFFE == 0xFFFE
        || cp > 0x10FFFF)
}

/// One block of the font's character table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharBlock {
    /// Start of the block; for «Other characters» it is `None`.
    pub start: Option<u32>,
    pub name: String,
    pub chars: Vec<u32>,
}

/// Index of the character's block in [`UNICODE_BLOCKS`].
fn unicode_block_index(cp: u32) -> Option<usize> {
    let i = UNICODE_BLOCKS.partition_point(|(start, _, _)| *start <= cp);
    (i > 0 && cp <= UNICODE_BLOCKS[i - 1].1).then(|| i - 1)
}

/// The font's characters by block: only blocks where the font has at least one
/// shown character, in Unicode order; «Other characters» last.
pub fn font_char_blocks(ranges: &[(u32, u32)]) -> Vec<CharBlock> {
    let mut by_block: Vec<Vec<u32>> = vec![Vec::new(); UNICODE_BLOCKS.len()];
    let mut other: Vec<u32> = Vec::new();
    let mut seen = BTreeSet::new();
    for &(a, b) in ranges {
        for cp in a..=b.min(0x10FFFF) {
            if !font_char_printable(cp) || !seen.insert(cp) {
                continue;
            }
            match unicode_block_index(cp) {
                Some(i) => by_block[i].push(cp),
                None => other.push(cp),
            }
        }
    }
    let mut out: Vec<CharBlock> = by_block
        .into_iter()
        .enumerate()
        .filter(|(_, chars)| !chars.is_empty())
        .map(|(i, mut chars)| {
            chars.sort_unstable();
            CharBlock { start: Some(UNICODE_BLOCKS[i].0), name: tr(UNICODE_BLOCKS[i].2), chars }
        })
        .collect();
    if !other.is_empty() {
        other.sort_unstable();
        out.push(CharBlock { start: None, name: tr("Other characters"), chars: other });
    }
    out
}

/// Key of the custom sample text in the Rexon settings (`settings.json`).
pub const FONT_SAMPLE_TEXT_KEY: &str = "font_sample_text";

/// The largest length of a custom sample text (in characters).
pub const FONT_SAMPLE_TEXT_MAX: usize = 300;

/// The default sample, in the interface language.
pub fn font_sample_default() -> String {
    tr("The quick brown fox jumps over the lazy dog 0123456789")
}

/// The sample text: the custom one from the settings or, if there is none, the default.
pub fn font_sample_text(stored: Option<&str>) -> String {
    match stored.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.chars().take(FONT_SAMPLE_TEXT_MAX).collect(),
        None => font_sample_default(),
    }
}

/// What to write to the settings from the input field: one line without spaces
/// at the edges, no longer than [`FONT_SAMPLE_TEXT_MAX`]. Empty, or exactly the
/// default sample, gives an empty string: then after the interface language
/// changes the sample changes along with it instead of getting stuck in the previous language.
pub fn font_sample_text_to_store(input: &str) -> String {
    let one_line: String = input.chars().map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c }).collect();
    let text: String = one_line.trim().chars().take(FONT_SAMPLE_TEXT_MAX).collect();
    if text == font_sample_default() {
        String::new()
    } else {
        text
    }
}

// ── Enabling and disabling fonts: `<rejectfont>` in fonts.conf ──────────
//
// A font is disabled without removal: its file goes into our separate block
// of the user `fonts.conf` (fonts-conf(5), `<selectfont>` → `<rejectfont>` →
// `<glob>` with the full file path). fontconfig excludes such files from the
// font set "as if they were not in the system": neither `fc-list` nor
// programs see them; the file stays in place, root is not needed, only this
// user is affected. Enabling means removing the path from the block.
// An empty block is removed entirely; a file with nothing left after that is
// deleted: the system returns exactly to its previous state.
//
// The block is our own, separate from the rendering block: antialiasing
// rewrites its own block entirely and knows nothing about disabled fonts.
// Resetting fonts (`font_layers_reset_plan`) does not remove this block:
// disabling is the person's choice, like removal, not a look setting.

const FC_REJECT_BEGIN: &str = "<!-- >>> Rexon: disabled fonts (managed by Rexon, edits are overwritten) -->";
const FC_REJECT_BEGIN_PREFIX: &str = "<!-- >>> Rexon: disabled fonts";
const FC_REJECT_END: &str = "<!-- <<< Rexon: disabled fonts -->";

/// Whether a file can be disabled with a `<glob>` entry: the path is full,
/// without `..` and without `*`/`?` (fontconfig reads them as a pattern, and the entry would disable too much).
pub fn font_glob_safe(file: &str) -> bool {
    file.starts_with('/') && !file.contains(['*', '?', '\n', '\r']) && !file.split('/').any(|part| part == "..")
}

/// The inverse of `system_l10n::xml_escape`; `&amp;` goes last.
fn fc_xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Our block of disabled fonts. An empty list gives an empty string (no block).
pub fn fontconfig_reject_block(files: &[String]) -> String {
    let files: Vec<&String> = files.iter().filter(|f| font_glob_safe(f)).collect();
    if files.is_empty() {
        return String::new();
    }
    let mut out = format!("  {FC_REJECT_BEGIN}\n  <selectfont>\n    <rejectfont>\n");
    for f in files {
        out.push_str(&format!("      <glob>{}</glob>\n", crate::system_l10n::xml_escape(f)));
    }
    out.push_str(&format!("    </rejectfont>\n  </selectfont>\n  {FC_REJECT_END}\n"));
    out
}

/// Files disabled by our block (foreign `<rejectfont>` entries are not ours,
/// they are not here). A broken block gives an empty list.
pub fn fontconfig_disabled_files(text: &str) -> Vec<String> {
    let Some(begin) = text.find(FC_REJECT_BEGIN_PREFIX) else { return Vec::new() };
    let Some(end_rel) = text[begin..].find(FC_REJECT_END) else { return Vec::new() };
    let mut body = &text[begin..begin + end_rel];
    let mut out: Vec<String> = Vec::new();
    while let Some(at) = body.find("<glob>") {
        let rest = &body[at + "<glob>".len()..];
        let Some(close) = rest.find("</glob>") else { break };
        let file = fc_xml_unescape(rest[..close].trim());
        if font_glob_safe(&file) && !out.contains(&file) {
            out.push(file);
        }
        body = &rest[close + "</glob>".len()..];
    }
    out
}

/// Text of `fonts.conf` with the disabled `files`. If a bare file is left in the
/// end ([`fontconfig_is_bare`]), it must be deleted, not written.
pub fn fontconfig_with_disabled(existing: Option<&str>, files: &[String]) -> Result<String, FontconfigRefusal> {
    fc_with_managed_block(existing, &fontconfig_reject_block(files), FC_REJECT_BEGIN_PREFIX, FC_REJECT_END)
}

/// Files the toggle affects: the whole family (`file` is empty) or one style.
/// Without repeats, alphabetically.
pub fn font_toggle_files(fonts: &[InstalledFont], family: &str, file: &str) -> Vec<String> {
    let mut files: Vec<String> = fonts
        .iter()
        .filter(|f| f.family == family && (file.is_empty() || f.file == file))
        .map(|f| f.file.clone())
        .collect();
    files.sort();
    files.dedup();
    files
}

/// The new list of disabled files: `files` added (`enable = false`) or
/// removed. Alphabetically, without repeats.
pub fn font_disabled_after(current: &[String], files: &[String], enable: bool) -> Vec<String> {
    let mut out: Vec<String> = current.iter().filter(|f| !files.contains(f)).cloned().collect();
    if !enable {
        out.extend(files.iter().cloned());
    }
    out.sort();
    out.dedup();
    out
}

/// How enabling or disabling a font ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontToggleOutcome {
    /// Disabled (family).
    Disabled(String),
    /// Enabled (family).
    Enabled(String),
    /// The family is chosen as a system font: it cannot be disabled.
    InUse(String),
    /// All fonts at all would be disabled.
    LastFonts,
    /// The file name would be read as a pattern (`*`, `?`).
    UnsafeName(String),
    /// The font was not found (the list is stale).
    NotFound(String),
    /// `fonts.conf` does not parse, or our block in it is broken by hand.
    Refused(String),
    /// `fonts.conf` could not be written.
    Failed(String),
}

/// The ban on disabling, if there is one: a family chosen as a system font
/// (`in_use` are the families from the environment settings) would disappear
/// entirely; or not a single enabled font would remain. `disabled` is the
/// list of disabled ones AFTER the toggle.
pub fn font_disable_refusal(fonts: &[InstalledFont], disabled: &[String], in_use: &[String]) -> Option<FontToggleOutcome> {
    let gone = |f: &InstalledFont| disabled.contains(&f.file);
    for name in in_use.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        let faces: Vec<&InstalledFont> = fonts.iter().filter(|f| f.family.eq_ignore_ascii_case(name)).collect();
        if !faces.is_empty() && faces.iter().all(|f| gone(f)) && faces.iter().any(|f| !f.disabled) {
            return Some(FontToggleOutcome::InUse(faces[0].family.clone()));
        }
    }
    if !fonts.is_empty() && fonts.iter().all(gone) {
        return Some(FontToggleOutcome::LastFonts);
    }
    None
}

/// Result of the toggle, for a person.
pub fn font_toggle_status(outcome: &FontToggleOutcome) -> String {
    match outcome {
        // TRANSLATORS: %1 is a font family.
        FontToggleOutcome::Disabled(family) => trf(
            "⏸️ Font disabled: %1. Apps stop seeing it after a restart; you can turn it back on here",
            &[family],
        ),
        // TRANSLATORS: %1 is a font family.
        FontToggleOutcome::Enabled(family) => trf("✅ Font enabled: %1. Apps will see it after a restart", &[family]),
        // TRANSLATORS: %1 is a font family.
        FontToggleOutcome::InUse(family) => trf(
            "⚠️ %1 is set as a system font right now — choose another one first, then disable this one",
            &[family],
        ),
        FontToggleOutcome::LastFonts => tr("⚠️ You can't disable every font — text in apps would become unreadable"),
        // TRANSLATORS: %1 is a font file name.
        FontToggleOutcome::UnsafeName(name) => trf(
            "⚠️ The file name %1 contains * or ?, which fontconfig reads as a wildcard and could disable other fonts too — nothing was changed",
            &[name],
        ),
        // TRANSLATORS: %1 is a font family.
        FontToggleOutcome::NotFound(family) => trf("⚠️ Font %1 wasn't found — the list has been refreshed", &[family]),
        FontToggleOutcome::Refused(path) => tf("font.fc_refused", &[path]),
        // TRANSLATORS: %1 is the path of the fontconfig file (fonts.conf).
        FontToggleOutcome::Failed(path) => trf("⚠️ Could not write %1", &[path]),
    }
}

/// The question before disabling, only if the same files hold other families
/// too (`.ttc`): they will be disabled together. Otherwise `None`: disabling is
/// reversible, there is no need to ask.
pub fn font_disable_confirm(family: &str, families: &[String]) -> Option<(String, String)> {
    let others: Vec<&str> = families.iter().filter(|f| *f != family).map(String::as_str).collect();
    if others.is_empty() {
        return None;
    }
    // TRANSLATORS: %1 is a font family, %2 a comma-separated list of other font families.
    let text = trf(
        "Disable the font %1? The same files also hold %2 — they will be disabled too.",
        &[&family, &others.join(", ")],
    );
    Some((tr("Disable Font"), text))
}

// ── Custom font groups: `fontgroups.xml`, shared with KDE ───────────────
//
// Groups are stored in the same place and the same form as in KDE Plasma's
// «Font Management»: `$XDG_CONFIG_HOME/fontgroups.xml`, a `<groups>` root, in
// it `<group name="…">` with a list of `<family>…</family>`. A group is only
// family names: font files are not touched, and a group created in Rexon is
// visible in KDE and vice versa. A family that is not in the system right
// now is not thrown out of a group: it may have been removed for a while or installed later.
//
// Built-in groups («All», «Personal», «System», «Unclassified») are not
// written to the file: they are list filters. «Unclassified» are families
// not in any custom group; as in KDE, they are offered only when there is at
// least one custom group.
//
// The file is reread before each edit: the groups may have been changed in
// KDE, and writing from a stale list would erase someone else's work. A file
// that does not parse is not rewritten at all.

/// The groups file relative to `$XDG_CONFIG_HOME`.
pub const FONT_GROUPS_REL_PATH: &str = "fontgroups.xml";

/// Prefix of the "location" filter value for a custom group: `group:Name`.
pub const FONT_GROUP_ORIGIN_PREFIX: &str = "group:";

/// The longest group name (characters).
pub const FONT_GROUP_NAME_MAX: usize = 100;

/// A custom font group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontGroup {
    pub name: String,
    /// Families alphabetically, without repeats.
    pub families: BTreeSet<String>,
}

/// Parse `fontgroups.xml`. `Err` means the file is not XML: such a file is not
/// rewritten, so as not to erase the groups that were in it. A group without a
/// name is skipped, groups with the same name are merged (as when KDE reads them).
pub fn parse_font_groups(text: &str) -> Result<Vec<FontGroup>, ()> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut groups: Vec<FontGroup> = Vec::new();
    // Depth: 1 is the root, 2 is `<group>`, 3 is `<family>`.
    let mut depth = 0usize;
    let mut group: Option<usize> = None;
    let mut family: Option<String> = None;
    let mut seen_root = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                depth += 1;
                if depth == 1 {
                    seen_root = true;
                }
                let name = e.name();
                if depth == 2 && name.as_ref() == b"group" {
                    group = font_group_attr_name(&e)?.map(|n| font_group_slot(&mut groups, n));
                } else if depth == 3 && group.is_some() && name.as_ref() == b"family" {
                    family = Some(String::new());
                }
            }
            Ok(Event::Empty(e)) => {
                if depth == 0 {
                    seen_root = true;
                } else if depth == 1 && e.name().as_ref() == b"group" {
                    if let Some(n) = font_group_attr_name(&e)? {
                        font_group_slot(&mut groups, n);
                    }
                }
            }
            Ok(Event::End(_)) => {
                if depth == 3 {
                    if let (Some(g), Some(f)) = (group, family.take()) {
                        let f = f.trim();
                        if !f.is_empty() {
                            groups[g].families.insert(f.to_string());
                        }
                    }
                } else if depth == 2 {
                    group = None;
                }
                depth = depth.checked_sub(1).ok_or(())?;
            }
            Ok(Event::Text(e)) => {
                if let Some(f) = family.as_mut() {
                    f.push_str(&e.decode().map_err(|_| ())?);
                }
            }
            Ok(Event::CData(e)) => {
                if let Some(f) = family.as_mut() {
                    f.push_str(&String::from_utf8_lossy(e.as_ref()));
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if let Some(f) = family.as_mut() {
                    let ch = match e.resolve_char_ref().map_err(|_| ())? {
                        Some(c) => c,
                        None => match e.decode().map_err(|_| ())?.as_ref() {
                            "lt" => '<',
                            "gt" => '>',
                            "amp" => '&',
                            "quot" => '"',
                            "apos" => '\'',
                            _ => return Err(()),
                        },
                    };
                    f.push(ch);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(()),
        }
    }
    if depth != 0 || (!seen_root && !text.trim().is_empty()) {
        return Err(());
    }
    Ok(groups)
}

/// The `name` attribute of `<group>` (an empty name means no name).
fn font_group_attr_name(e: &quick_xml::events::BytesStart) -> Result<Option<String>, ()> {
    for attr in e.attributes() {
        let attr = attr.map_err(|_| ())?;
        if attr.key.as_ref() == b"name" {
            let v = attr.unescape_value().map_err(|_| ())?;
            return Ok((!v.is_empty()).then(|| v.into_owned()));
        }
    }
    Ok(None)
}

/// Index of the group `name` in the list; if there is none, it is created.
fn font_group_slot(groups: &mut Vec<FontGroup>, name: String) -> usize {
    match groups.iter().position(|g| g.name == name) {
        Some(i) => i,
        None => {
            groups.push(FontGroup { name, families: BTreeSet::new() });
            groups.len() - 1
        }
    }
}

/// Text of `fontgroups.xml`, in the form KDE writes it.
pub fn font_groups_xml(groups: &[FontGroup]) -> String {
    let esc = crate::system_l10n::xml_escape;
    let mut out = String::from("<groups>\n");
    for g in groups {
        out.push_str(&format!(" <group name=\"{}\">\n", esc(&g.name)));
        for f in &g.families {
            out.push_str(&format!("  <family>{}</family>\n", esc(f)));
        }
        out.push_str(" </group>\n");
    }
    out.push_str("</groups>\n");
    out
}

/// A group name as the person typed it, in the form for the file: line breaks
/// and tabs become spaces, spaces at the edges and repeated ones are removed,
/// the length is no more than [`FONT_GROUP_NAME_MAX`].
pub fn font_group_name_clean(raw: &str) -> String {
    let joined = raw.split(|c: char| c.is_whitespace() || c.is_control()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    joined.chars().take(FONT_GROUP_NAME_MAX).collect()
}

/// Names of the built-in groups: a custom group cannot take them (in KDE
/// neither: there built-in groups are in one list with custom ones).
fn font_group_reserved(name: &str) -> bool {
    let mut names: Vec<String> = ["All", "Personal", "System", "Unclassified"].iter().map(|s| s.to_string()).collect();
    names.extend(font_origin_choices().into_iter().map(|(_, label)| label));
    names.push(trc("font origin", "Unclassified"));
    names.iter().any(|n| n == name)
}

/// An edit of custom groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontGroupEdit {
    Create(String),
    Rename { from: String, to: String },
    Remove(String),
    /// Add families to a group.
    Add { group: String, families: Vec<String> },
    /// Remove families from a group (the fonts themselves stay).
    Take { group: String, families: Vec<String> },
}

/// How a group edit ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontGroupOutcome {
    Created(String),
    Renamed { from: String, to: String },
    Removed(String),
    /// So many families added.
    Added { group: String, count: usize },
    /// So many families removed.
    Taken { group: String, count: usize },
    /// All the families were already in the group.
    AlreadyIn(String),
    /// None of the families were in the group.
    NotIn(String),
    EmptyName,
    Exists(String),
    Reserved(String),
    NotFound(String),
    /// `fontgroups.xml` does not parse: untouched.
    Refused(String),
    /// `fontgroups.xml` could not be written.
    Failed(String),
}

impl FontGroupOutcome {
    /// Whether the edit changed the group list (it must be written).
    pub fn changed(&self) -> bool {
        matches!(
            self,
            FontGroupOutcome::Created(_)
                | FontGroupOutcome::Renamed { .. }
                | FontGroupOutcome::Removed(_)
                | FontGroupOutcome::Added { .. }
                | FontGroupOutcome::Taken { .. }
        )
    }
}

/// Apply an edit: the new group list and the result. On refusal the list is the same.
/// A new group goes at the end, a renamed one stays in its place.
pub fn font_groups_apply(groups: &[FontGroup], edit: &FontGroupEdit) -> (Vec<FontGroup>, FontGroupOutcome) {
    let mut out = groups.to_vec();
    let find = |out: &Vec<FontGroup>, name: &str| out.iter().position(|g| g.name == name);
    let outcome = match edit {
        FontGroupEdit::Create(raw) => {
            let name = font_group_name_clean(raw);
            if name.is_empty() {
                FontGroupOutcome::EmptyName
            } else if font_group_reserved(&name) {
                FontGroupOutcome::Reserved(name)
            } else if find(&out, &name).is_some() {
                FontGroupOutcome::Exists(name)
            } else {
                out.push(FontGroup { name: name.clone(), families: BTreeSet::new() });
                FontGroupOutcome::Created(name)
            }
        }
        FontGroupEdit::Rename { from, to } => {
            let to = font_group_name_clean(to);
            match find(&out, from) {
                None => FontGroupOutcome::NotFound(from.clone()),
                Some(_) if to.is_empty() => FontGroupOutcome::EmptyName,
                Some(_) if to == *from => FontGroupOutcome::Renamed { from: from.clone(), to },
                Some(_) if font_group_reserved(&to) => FontGroupOutcome::Reserved(to),
                Some(_) if find(&out, &to).is_some() => FontGroupOutcome::Exists(to),
                Some(i) => {
                    out[i].name = to.clone();
                    FontGroupOutcome::Renamed { from: from.clone(), to }
                }
            }
        }
        FontGroupEdit::Remove(name) => match find(&out, name) {
            None => FontGroupOutcome::NotFound(name.clone()),
            Some(i) => {
                out.remove(i);
                FontGroupOutcome::Removed(name.clone())
            }
        },
        FontGroupEdit::Add { group, families } => match find(&out, group) {
            None => FontGroupOutcome::NotFound(group.clone()),
            Some(i) => {
                let count = families
                    .iter()
                    .map(|f| f.trim())
                    .filter(|f| !f.is_empty())
                    .filter(|f| out[i].families.insert(f.to_string()))
                    .count();
                if count == 0 {
                    FontGroupOutcome::AlreadyIn(group.clone())
                } else {
                    FontGroupOutcome::Added { group: group.clone(), count }
                }
            }
        },
        FontGroupEdit::Take { group, families } => match find(&out, group) {
            None => FontGroupOutcome::NotFound(group.clone()),
            Some(i) => {
                let count = families.iter().filter(|f| out[i].families.remove(f.trim())).count();
                if count == 0 {
                    FontGroupOutcome::NotIn(group.clone())
                } else {
                    FontGroupOutcome::Taken { group: group.clone(), count }
                }
            }
        },
    };
    if outcome.changed() {
        (out, outcome)
    } else {
        (groups.to_vec(), outcome)
    }
}

/// Result of a group edit, for a person.
pub fn font_group_status(outcome: &FontGroupOutcome) -> String {
    match outcome {
        // TRANSLATORS: %1 is the name of a font group.
        FontGroupOutcome::Created(name) => trf("✅ Group %1 created — drag fonts onto it or use “Add to Group”", &[name]),
        // TRANSLATORS: %1 is the old name of a font group, %2 the new one.
        FontGroupOutcome::Renamed { from, to } => trf("✅ Group %1 renamed to %2", &[from, to]),
        // TRANSLATORS: %1 is the name of a font group.
        FontGroupOutcome::Removed(name) => trf("🗑️ Group %1 removed; its fonts stay installed", &[name]),
        FontGroupOutcome::Added { group, count } => crate::i18n::fill(
            // TRANSLATORS: %n is the number of font families, %1 the name of a font group.
            &ntr("✅ %n font family added to %1", "✅ %n font families added to %1", *count as u64),
            &[group],
        ),
        FontGroupOutcome::Taken { group, count } => crate::i18n::fill(
            // TRANSLATORS: %n is the number of font families, %1 the name of a font group.
            &ntr(
                "✅ %n font family removed from %1; the font stays installed",
                "✅ %n font families removed from %1; the fonts stay installed",
                *count as u64,
            ),
            &[group],
        ),
        // TRANSLATORS: %1 is the name of a font group.
        FontGroupOutcome::AlreadyIn(name) => trf("ℹ️ Already in group %1", &[name]),
        // TRANSLATORS: %1 is the name of a font group.
        FontGroupOutcome::NotIn(name) => trf("ℹ️ Not in group %1", &[name]),
        FontGroupOutcome::EmptyName => tr("⚠️ Type a name for the group"),
        // TRANSLATORS: %1 is the name of a font group.
        FontGroupOutcome::Exists(name) => trf("⚠️ A group named %1 already exists", &[name]),
        // TRANSLATORS: %1 is a name the user typed for a new font group.
        FontGroupOutcome::Reserved(name) => trf("⚠️ %1 is a built-in group — choose another name", &[name]),
        // TRANSLATORS: %1 is the name of a font group.
        FontGroupOutcome::NotFound(name) => trf("⚠️ Group %1 wasn't found — the list has been refreshed", &[name]),
        // TRANSLATORS: %1 is the path of the font groups file.
        FontGroupOutcome::Refused(path) => trf(
            "⚠️ %1 can't be read (it may have been edited by hand) — groups were left untouched",
            &[path],
        ),
        // TRANSLATORS: %1 is the path of the font groups file.
        FontGroupOutcome::Failed(path) => trf("⚠️ Could not write %1", &[path]),
    }
}

/// The question before deleting a group: `(heading, text)`.
pub fn font_group_remove_confirm(name: &str) -> (String, String) {
    (
        tr("Remove Group"),
        // TRANSLATORS: %1 is the name of a font group.
        trf("Remove the group %1? Only the group goes away — its fonts stay installed.", &[&name]),
    )
}

/// Options of the "location" filter together with custom groups: `(value,
/// caption, whether it is a custom group)`. Built-in ones first; «Unclassified»
/// and custom groups (alphabetically, case-insensitive) only if there is at
/// least one custom group.
pub fn font_origin_choices_with_groups(groups: &[FontGroup]) -> Vec<(String, String, bool)> {
    let mut out: Vec<(String, String, bool)> =
        font_origin_choices().into_iter().map(|(id, label)| (id.to_string(), label, false)).collect();
    if groups.is_empty() {
        return out;
    }
    out.push(("unclassified".to_string(), trc("font origin", "Unclassified"), false));
    let mut names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)));
    out.extend(names.into_iter().map(|n| (format!("{FONT_GROUP_ORIGIN_PREFIX}{n}"), n.to_string(), true)));
    out
}

/// A custom group from a "location" filter value (`group:Name`).
pub fn font_group_of_origin(origin: &str) -> Option<&str> {
    origin.strip_prefix(FONT_GROUP_ORIGIN_PREFIX)
}

/// Custom groups a family belongs to (alphabetically).
pub fn font_groups_of_family(groups: &[FontGroup], family: &str) -> Vec<String> {
    let mut out: Vec<String> = groups.iter().filter(|g| g.families.contains(family)).map(|g| g.name.clone()).collect();
    out.sort_by_key(|n| n.to_lowercase());
    out
}

/// [`font_group_view_filtered`] with custom groups: "location" may also be
/// `unclassified` (families outside all custom groups) and `group:Name`
/// (families of this group, with all styles). An unknown group gives an empty list.
pub fn font_group_view_in(groups: &[FontFamilyGroup], filter: &FontListFilter, custom: &[FontGroup]) -> Vec<FontFamilyGroup> {
    let keep = |family: &str| -> bool {
        if filter.origin == "unclassified" {
            return !custom.iter().any(|g| g.families.contains(family));
        }
        match font_group_of_origin(&filter.origin) {
            Some(name) => custom.iter().any(|g| g.name == name && g.families.contains(family)),
            None => true,
        }
    };
    let custom_origin = filter.origin == "unclassified" || font_group_of_origin(&filter.origin).is_some();
    let inner = if custom_origin { FontListFilter { origin: "all".to_string(), ..filter.clone() } } else { filter.clone() };
    let picked: Vec<FontFamilyGroup> = groups.iter().filter(|g| keep(&g.family)).cloned().collect();
    font_group_view_filtered(&picked, &inner)
}

/// Font files of a group: for enabling, disabling and export. Alphabetically,
/// without repeats.
pub fn font_group_files(fonts: &[InstalledFont], group: &FontGroup) -> Vec<String> {
    let mut files: Vec<String> =
        fonts.iter().filter(|f| group.families.contains(&f.family)).map(|f| f.file.clone()).collect();
    files.sort();
    files.dedup();
    files
}

/// Enabling or disabling a whole group's fonts, for a person.
/// `families` is how many installed families of the group are affected.
pub fn font_group_toggle_status(group: &str, families: usize, enabled: bool) -> String {
    let template = if enabled {
        // TRANSLATORS: %n is the number of font families, %1 the name of a font group.
        ntr(
            "✅ Group %1: %n font family enabled. Apps will see it after a restart",
            "✅ Group %1: %n font families enabled. Apps will see them after a restart",
            families as u64,
        )
    } else {
        // TRANSLATORS: %n is the number of font families, %1 the name of a font group.
        ntr(
            "⏸️ Group %1: %n font family disabled. Apps stop seeing it after a restart; you can turn it back on here",
            "⏸️ Group %1: %n font families disabled. Apps stop seeing them after a restart; you can turn them back on here",
            families as u64,
        )
    };
    crate::i18n::fill(&template, &[&group])
}

/// The group has not a single installed font.
pub fn font_group_empty_status(group: &str) -> String {
    // TRANSLATORS: %1 is the name of a font group.
    trf("ℹ️ Group %1 has no installed fonts yet", &[&group])
}

/// The question before disabling a group, only if the same files hold
/// families not from the group (`.ttc`): they will be disabled together.
pub fn font_group_disable_confirm(group: &str, others: &[String]) -> Option<(String, String)> {
    if others.is_empty() {
        return None;
    }
    // TRANSLATORS: %1 is the name of a font group, %2 a comma-separated list of font families.
    let text = trf(
        "Disable the fonts of group %1? The same files also hold %2 — they will be disabled too.",
        &[&group, &others.join(", ")],
    );
    Some((tr("Disable Fonts in Group"), text))
}

/// What to put into a group archive: `(name in the archive, file)`. Type 1
/// fonts take the metric files next to them along (`.afm`, `.pfm`, the same
/// name), if they exist (`exists`). Identical names from different
/// directories do not overwrite each other: the second file gets a `-2`
/// suffix, the third `-3`…
pub fn font_group_export_entries(files: &[String], exists: &dyn Fn(&str) -> bool) -> Vec<(String, String)> {
    let mut paths: Vec<String> = Vec::new();
    for file in files {
        if !paths.contains(file) {
            paths.push(file.clone());
        }
        let lower = file.to_lowercase();
        if lower.ends_with(".pfa") || lower.ends_with(".pfb") {
            let stem = &file[..file.len() - 4];
            for ext in ["afm", "AFM", "pfm", "PFM"] {
                let side = format!("{stem}.{ext}");
                if !paths.contains(&side) && exists(&side) {
                    paths.push(side);
                }
            }
        }
    }
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    for path in paths {
        let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let (stem, ext) = match name.find('.') {
            Some(dot) if dot > 0 => (name[..dot].to_string(), name[dot..].to_string()),
            _ => (name.clone(), String::new()),
        };
        let mut entry = name.clone();
        let mut n = 2;
        while taken.contains(&entry.to_lowercase()) {
            entry = format!("{stem}-{n}{ext}");
            n += 1;
        }
        taken.insert(entry.to_lowercase());
        out.push((entry, path));
    }
    out
}

/// The default group archive name: the group name without `/` and control
/// characters, `.zip`.
pub fn font_group_export_name(group: &str) -> String {
    let clean: String = group.chars().map(|c| if c == '/' || c.is_control() { '-' } else { c }).collect();
    let clean = clean.trim().trim_start_matches('.');
    format!("{}.zip", if clean.is_empty() { "fonts" } else { clean })
}

/// The path chosen in the save dialog, with `.zip` at the end.
pub fn font_group_export_path(chosen: &str) -> String {
    if chosen.to_lowercase().ends_with(".zip") {
        chosen.to_string()
    } else {
        format!("{chosen}.zip")
    }
}

/// Title and filter of the group archive save dialog.
pub fn font_group_export_dialog() -> (String, String) {
    (tr("Export Group"), format!("{} (*.zip)", tr("ZIP archive")))
}

/// Write a group archive: first to a temporary file next to it, then by
/// rename, so an unfinished archive does not remain under the wanted name.
/// Returns how many files went into the archive.
pub fn write_font_group_zip(entries: &[(String, String)], dest: &Path) -> std::io::Result<usize> {
    use std::io::Write as _;
    let part = dest.with_extension("zip.part");
    let result = (|| -> std::io::Result<usize> {
        let mut zip = zip::ZipWriter::new(fs::File::create(&part)?);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, path) in entries {
            let data = fs::read(path)?;
            // The date of a file in the archive is its modification time (without it,
            // it would be January 1, 1980).
            let mtime = fs::metadata(path).and_then(|m| m.modified()).ok().and_then(zip_date_of);
            let options = match mtime {
                Some(t) => options.last_modified_time(t),
                None => options,
            };
            zip.start_file(name.as_str(), options).map_err(std::io::Error::other)?;
            zip.write_all(&data)?;
        }
        zip.finish().map_err(std::io::Error::other)?;
        Ok(entries.len())
    })();
    match result {
        Ok(n) => {
            fs::rename(&part, dest)?;
            Ok(n)
        }
        Err(e) => {
            let _ = fs::remove_file(&part);
            Err(e)
        }
    }
}

/// File time for a ZIP entry: local, as archivers do (ZIP stores the date
/// without a time zone). Outside 1980–2107 it is `None`.
fn zip_date_of(t: std::time::SystemTime) -> Option<zip::DateTime> {
    let secs = libc::time_t::try_from(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs()).ok()?;
    // SAFETY: `localtime_r` writes only into the passed structure.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&secs, &mut tm).is_null() {
            return None;
        }
        tm
    };
    zip::DateTime::from_date_and_time(
        u16::try_from(tm.tm_year + 1900).ok()?,
        u8::try_from(tm.tm_mon + 1).ok()?,
        u8::try_from(tm.tm_mday).ok()?,
        u8::try_from(tm.tm_hour).ok()?,
        u8::try_from(tm.tm_min).ok()?,
        u8::try_from(tm.tm_sec).ok()?,
    )
    .ok()
}

/// Result of a group export, for a person. `Ok(n)` is how many files are in the archive.
pub fn font_group_export_status(group: &str, dest: &str, result: &Result<usize, String>) -> String {
    match result {
        Ok(n) => crate::i18n::fill(
            // TRANSLATORS: %n is the number of font files, %1 the name of a font group, %2 the archive path.
            // Also: arrows ← → point the way they should in your language: in right-to-left languages swap them.
            &ntr("📦 Group %1 exported: %n font file → %2", "📦 Group %1 exported: %n font files → %2", *n as u64),
            &[&group, &dest],
        ),
        // TRANSLATORS: %1 is the archive path, %2 the error from the system.
        Err(e) => trf("⚠️ Could not write %1: %2", &[&dest, e]),
    }
}

// ── Duplicates ──────────────────────────────────────────────────────────

/// argv of the list for finding duplicates: scalable fonts only. Bitmap
/// families deliberately lie in many files (size, encoding:
/// `misc/7x13.pcf.gz` and `misc/8x13.pcf.gz` for «Misc Fixed Regular»), these
/// are not copies.
pub fn fc_list_scalable_argv() -> Vec<String> {
    let mut argv = fc_list_fonts_argv();
    argv.insert(1, ":scalable=true".to_string());
    argv
}

/// One style found in several files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontDuplicate {
    pub family: String,
    pub style: String,
    /// Copies: system ones first, then own; within that, by path.
    pub copies: Vec<InstalledFont>,
}

/// Find duplicates: one family and style in several DIFFERENT files
/// (`real_path` resolves links: two paths to one file are not copies).
/// Disabled ones do not count: programs do not see them anyway.
pub fn find_font_duplicates(fonts: &[InstalledFont], real_path: &dyn Fn(&str) -> String) -> Vec<FontDuplicate> {
    let mut out: Vec<FontDuplicate> = Vec::new();
    for f in fonts.iter().filter(|f| !f.disabled) {
        match out.iter_mut().find(|d| d.family == f.family && d.style == f.style) {
            Some(d) => d.copies.push(f.clone()),
            None => out.push(FontDuplicate { family: f.family.clone(), style: f.style.clone(), copies: vec![f.clone()] }),
        }
    }
    for d in &mut out {
        let mut seen = BTreeSet::new();
        d.copies.retain(|c| seen.insert(real_path(&c.file)));
        d.copies.sort_by(|a, b| a.user.cmp(&b.user).then(a.file.cmp(&b.file)));
    }
    out.retain(|d| d.copies.len() > 1);
    out.sort_by(|a, b| {
        a.family
            .to_lowercase()
            .cmp(&b.family.to_lowercase())
            .then(style_rank(&a.style).cmp(&style_rank(&b.style)))
    });
    out
}

/// Summary above the list of duplicates.
pub fn font_duplicates_summary(found: usize) -> String {
    if found == 0 {
        return tr("No duplicates: every font style is installed once");
    }
    ntr(
        // TRANSLATORS: %n is the number of font styles found in more than one file.
        "%n style is installed more than once",
        "%n styles are installed more than once",
        found as u64,
    )
}

// ============================================================================
// Default application protection and Plasma panel copies
// ============================================================================

/// A readable name from a shortcut id: `org.kde.dolphin.desktop` → `Dolphin`.
/// An empty string means "nothing to show".
pub fn pretty_desktop_id(app_id: &str) -> String {
    let app_id = app_id.trim();
    if app_id.is_empty() || app_id == "—" || app_id == "-" {
        return String::new();
    }
    let stem = app_id.split(".desktop").next().unwrap_or(app_id);
    let name = stem.rsplit('.').next().unwrap_or(stem);
    if name.is_empty() {
        return app_id.to_string();
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => app_id.to_string(),
    }
}

/// A human-readable copy timestamp: `20260620_011005` → «20.06.2026 at 01:10»
/// (the translation decides the order of the date parts). If it does not
/// parse, it is returned as is.
pub fn pretty_backup_ts(ts: &str) -> String {
    let bytes = ts.as_bytes();
    if bytes.len() != 15 || bytes[8] != b'_' {
        return ts.to_string();
    }
    let digits = |from: usize, to: usize| -> Option<&str> {
        let part = ts.get(from..to)?;
        if part.bytes().all(|b| b.is_ascii_digit()) {
            Some(part)
        } else {
            None
        }
    };
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(s)) = (
        digits(0, 4),
        digits(4, 6),
        digits(6, 8),
        digits(9, 11),
        digits(11, 13),
        digits(13, 15),
    ) else {
        return ts.to_string();
    };
    // Parsing is strict, like `datetime.strptime`: a nonexistent date stays as
    // is instead of turning into "32.13.2026". The month length is checked too:
    // February 30 is not a date.
    let year = y.parse::<u32>().unwrap_or(0);
    let month = mo.parse::<u32>().unwrap_or(0);
    let day = d.parse::<u32>().unwrap_or(0);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => 0,
    };
    let ok = days_in_month > 0
        && (1..=days_in_month).contains(&day)
        && h.parse::<u32>().unwrap_or(99) <= 23
        && mi.parse::<u32>().unwrap_or(99) <= 59
        && s.parse::<u32>().unwrap_or(99) <= 60;
    if !ok {
        return ts.to_string();
    }
    // TRANSLATORS: date and time of a saved copy: %1 day, %2 month, %3 year,
    // %4 hours, %5 minutes (all with leading zeros). Reorder them if needed.
    trf("%1.%2.%3 at %4:%5", &[&d, &mo, &y, &h, &mi])
}

/// Captions of the reasons a Plasma panel copy was made.
pub fn backup_reason_label(reason: &str) -> String {
    match reason {
        // TRANSLATORS: why a copy of the Plasma panel was made.
        "before_install" => tr("automatically, before installing"),
        // TRANSLATORS: why a copy of the Plasma panel was made.
        "manual" => tr("manually"),
        other => other.to_string(),
    }
}

/// The line of one panel copy: «📁 20.06.2026 at 01:10 · manual · 3 files».
pub fn backup_row_label(ts: &str, reason: &str, files: i64) -> String {
    format!(
        "📁 {}  ·  {}  ·  {}",
        pretty_backup_ts(ts),
        backup_reason_label(reason),
        // TRANSLATORS: number of files in a saved copy of the Plasma panel.
        ntr("%n file", "%n files", files.max(0) as u64)
    )
}

// ============================================================================
// Desktop completeness: the check result in words
// ============================================================================

/// The completeness check result in one line for a person.
///
/// `results` are "check → status" pairs (`fixed` / `failed` / `skipped` /
/// `needs_action` / anything else). An empty set means "no checks".
pub fn compliance_summary(results: &[(String, String)]) -> String {
    if results.is_empty() {
        return tr("No checks for this desktop yet");
    }
    let count = |what: &str| results.iter().filter(|(_, v)| v == what).count();
    let fixed = count("fixed");
    let failed = count("failed");
    let skipped = count("skipped");
    let needs = count("needs_action");

    let mut parts: Vec<String> = Vec::new();
    if fixed > 0 {
        // TRANSLATORS: part of a list joined with commas; the first letter is
        // capitalized by the program. %1 is a number.
        parts.push(trf("installed: %1", &[&fixed]));
    }
    if failed > 0 {
        // TRANSLATORS: part of a list joined with commas. %1 is a number.
        parts.push(trf("failed: %1", &[&failed]));
    }
    if needs > 0 {
        // TRANSLATORS: part of a list joined with commas. %1 is a number.
        parts.push(trf("still to install: %1", &[&needs]));
    }
    if skipped > 0 {
        // TRANSLATORS: part of a list joined with commas.
        parts.push(tr("the package check is only available on Arch/CachyOS"));
    }
    if parts.is_empty() {
        return trf("✅ Everything is in place (items checked: %1)", &[&results.len()]);
    }
    let joined = parts.join(", ");
    // The first letter is capital: the phrase starts with it.
    let mut chars = joined.chars();
    let joined = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => joined,
    };
    if failed > 0 || needs > 0 {
        format!("⚠️ {joined}")
    } else {
        format!("✅ {joined}")
    }
}

// ============================================================================
// YouTube wallpapers: drop-down list values
// ============================================================================

/// Quality list items: (caption, value for the engine).
///
/// The caption is a `msgid` (there is nothing to translate in numbers); the
/// place that shows it translates it: [`yt_qualities`] or the progress line
/// [`youtube_starting_status`].
pub const YT_QUALITIES: &[(&str, &str)] = &[
    ("1080p", "1080p"),
    ("720p", "720p"),
    ("1440p", "1440p"),
    ("4K", "4k"),
    // TRANSLATORS: YouTube video quality: the best one available.
    (n_("Best available"), "best"),
];

/// Mode list items: (caption, value for the engine). The caption is a `msgid`.
pub const YT_MODES: &[(&str, &str)] = &[
    // TRANSLATORS: YouTube wallpaper mode: play the video online.
    (n_("Stream"), "stream"),
    // TRANSLATORS: YouTube wallpaper mode: download the video first.
    (n_("Download"), "download"),
];

/// The quality list for showing: (translated caption, value for the engine).
pub fn yt_qualities() -> Vec<(String, &'static str)> {
    YT_QUALITIES.iter().map(|(l, v)| (tr(l), *v)).collect()
}

/// The mode list for showing: (translated caption, value for the engine).
pub fn yt_modes() -> Vec<(String, &'static str)> {
    YT_MODES.iter().map(|(l, v)| (tr(l), *v)).collect()
}

// ============================================================================
// Name of the current environment for captions
// ============================================================================

/// How to name the environment in a section caption.
///
/// `system.py` does not recognize niri/sway and returns "unknown"; then the
/// token from `XDG_CURRENT_DESKTOP`/`DESKTOP_SESSION` is used, so the customer
/// does not read "UNKNOWN" (rule #13). If there is nothing there either, «YOUR
/// DESKTOP»: the abbreviation "WM" means nothing to the customer (rule #13).
pub fn de_display(de: &str, xdg_current_desktop: &str, desktop_session: &str) -> String {
    if !de.is_empty() && de != "unknown" {
        // The desktop name is the same as on the «Desktops» tab and in the other
        // sections («KDE Plasma», not «KDE»): one environment under two names in one
        // window reads as two different ones.
        let known = crate::qt_desktops::display_name(de);
        return if known == de { de.to_uppercase() } else { known.to_string() };
    }
    let raw = if !xdg_current_desktop.is_empty() {
        xdg_current_desktop
    } else {
        desktop_session
    };
    let token = raw.split(':').next().unwrap_or("").trim();
    if token.is_empty() {
        // One name per product; it gets into a phrase only through
        // `screenshots::place_de_name`, otherwise the grammatical case breaks.
        crate::screenshots::unknown_de_name()
    } else {
        token.to_uppercase()
    }
}

// ============================================================================
// Tab texts
// ============================================================================

/// Button caption and title of the font picker dialog: (icon+button caption,
/// tooltip, dialog title).
pub fn font_target_texts(target: &str) -> (String, String, String) {
    match target {
        "mono" => (
            tr("🔠 Monospace…"),
            tr("Font for the terminal and code editors"),
            tr("Monospace Font"),
        ),
        "small" => (
            tr("🔡 Small…"),
            tr("Font for the smallest text: captions, hints, status bars"),
            tr("Small Font"),
        ),
        "toolbar" => (
            tr("🧰 Toolbar…"),
            tr("Font of toolbar button labels"),
            tr("Toolbar Font"),
        ),
        "menu" => (
            tr("📋 Menu…"),
            tr("Font of menu bars and menus"),
            tr("Menu Font"),
        ),
        "title" => (
            tr("🪟 Window Titles…"),
            tr("Font in window title bars"),
            tr("Window Title Font"),
        ),
        "document" => (
            tr("📄 Documents…"),
            tr("Font for reading documents in GNOME and GTK apps"),
            tr("Document Font"),
        ),
        "desktop" => (
            tr("🖥 Desktop…"),
            tr("Font of icon labels on the desktop"),
            tr("Desktop Font"),
        ),
        _ => (
            tr("🔤 System…"),
            tr("Font for buttons, menus and text in apps"),
            tr("System Font"),
        ),
    }
}

/// The line "what is on the desktop now" for the wallpaper section.
///
/// `running` is whether live wallpapers are playing; `format` and `path` are
/// what exactly. `None` means "the engine said nothing about it at all" (no
/// key in the snapshot); only then does a "?" appear in the line.
///
/// Fix (rule #13): the format was shown as the engine's internal word: the
/// customer read «Live wallpaper active: video». The name for a person comes
/// from the shared [`crate::wallpaper_engine::format_label`], which also
/// labels the other wallpaper messages; the tab has no second table of names.
pub fn wallpaper_status_text(running: bool, format: Option<&str>, path: Option<&str>) -> String {
    if !running {
        return tr("<b>Current wallpaper:</b> static (or system)");
    }
    let name = path
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string();
    let kind = match format {
        Some(fmt) => crate::wallpaper_engine::format_label(fmt),
        None => "?".to_string(),
    };
    // TRANSLATORS: %1 is the wallpaper kind (video, Flash…), %2 is the file name.
    trf("<b>▶️ Live wallpaper is on:</b> %1 — %2", &[&kind, &name])
}

/// Result of restoring the environment's native applications.
///
/// `restored` is how many were restored; `missing` are captions of categories
/// for which the system had not a single option. They must not be kept
/// silent: before, the person saw «✅ Restored 2 apps» and did not guess that
/// the terminal was not restored at all (rule #13).
pub fn restore_native_message(restored: usize, missing: &[String], de_display: &str) -> String {
    // Fix (rule #13): the count had one ending for all numbers: the customer
    // read «Restored 1 apps». The form for the number is chosen by the
    // translation catalog ([`crate::i18n::ntr`]).
    let msg = crate::screenshots::place_de_name(
        // TRANSLATORS: %n is how many default apps were restored, %1 is the
        // name of the current desktop environment.
        &ntr("Restored %n app for %1", "Restored %n apps for %1", restored as u64),
        "%1",
        de_display,
    );
    if missing.is_empty() {
        return msg;
    }
    // TRANSLATORS: %1 is the sentence about restored apps, %2 is a
    // comma-separated list of app types that were not found.
    trf("%1, not found on the system: %2", &[&msg, &missing.join(", ")])
}

/// Status line of the default application protection.
///
/// `remembered` is how many applications are already remembered (the
/// reference the check compares with); `changed` are triples (category
/// caption, what is remembered, what is now).
///
/// ⚠️ Fix (rule #13): previously the line treated an EMPTY list of differences
/// as success and announced «✅ Everything is in place» on a clean profile,
/// although nothing was remembered and there was nothing to compare with.
/// The customer read the green check as "the protection works", while they
/// were not protected at all. Now the green answer is given ONLY when the
/// reference has been taken; without a reference it says plainly that there
/// is nothing to notice a replacement by, and names the button that fixes it.
/// The button caption is taken from the tab catalog: there must be no second
/// copy of the letters here, or the advice will diverge from the window.
pub fn protection_status_text(
    remembered: usize,
    changed: &[(String, String, String)],
) -> String {
    if remembered == 0 {
        let button = t("prot.snapshot");
        // TRANSLATORS: %1 is the label of the “Remember Now” button.
        return trf("<b>⚠️ Nothing to compare with yet</b><br>Rexon hasn't yet remembered \
                    which apps you use by default, so it won't notice if they get replaced. \
                    Click “%1”.", &[&button]);
    }
    if changed.is_empty() {
        return tr("<b>✅ Everything is in place</b><br>Your default apps match the remembered ones");
    }
    let names: Vec<String> = changed
        .iter()
        .map(|(label, saved, current)| format!("{label}: {saved} → {current}"))
        .collect();
    // TRANSLATORS: %1 is a list like “Browser: firefox → chromium”.
    trf("<b>⚠️ Default apps have changed:</b><br>%1", &[&names.join(";  ")])
}

/// Caption of the active background sound line and its id by the file path.
pub fn ambient_sound_id(path: &str) -> String {
    format!("manual_{}", path.rsplit('/').next().unwrap_or(path))
}

/// Filter of the audio file picker for background sounds.
pub fn ambient_file_filter() -> String {
    format!(
        "{} (*.ogg *.oga *.mp3 *.wav *.flac *.opus *.m4a);;{} (*)",
        // TRANSLATORS: file type group in the “Choose an audio file” window.
        tr("Audio files"),
        // TRANSLATORS: file type group in a file chooser window.
        tr("All files"),
    )
}

// ============================================================================
// Tab sections: display order, headings, descriptions
// ============================================================================

/// All tab sections in display order: (frame name, heading, description).
///
/// The order is a decision about what the customer sees first, and it lives
/// here, not in a chain of Qt calls: before, it could be read only from the
/// body of `__init__`, and a section could be moved only by editing the window.
///
/// `%1` in a description is the name of the current environment (see [`section_texts`]).
/// Headings and descriptions are English `msgid`s; [`section_texts`] translates them.
/// The heading of the onboarding section is taken from the onboarding itself
/// ([`crate::onboarding::settings_label`]), so it is empty in the table.
pub const SECTIONS: &[(&str, &str, &str)] = &[
    (
        "langGroup",
        n_("🌐 Language"),
        n_("The language of Rexon's windows, menus and messages"),
    ),
    (
        "skinGroup",
        n_("🎨 Interface Skin"),
        n_("The look of the Rexon app itself (window colors). Your desktop's theme stays \
            as it is — only Rexon's own look changes."),
    ),
    (
        "obGroup",
        "",
        n_("Short tips on top of the window: where things are and where to start"),
    ),
    (
        "trayGroup",
        n_("🔔 System Tray Icon"),
        n_("Rexon keeps an icon next to the clock: wallpapers, sections and quitting — \
            without opening the window. While the icon is there, the window's close \
            button hides Rexon to the tray instead of quitting it."),
    ),
    (
        "linksGroup",
        n_("🔗 Links to Websites"),
        n_("Links in item descriptions and comments open in your browser only after Rexon \
            asks. Here are the websites you chose to open without asking."),
    ),
    (
        "fmGroup",
        n_("📁 File Manager"),
        n_("The program that opens folders — both in Rexon itself and across the whole \
            system"),
    ),
    (
        "quickFix",
        n_("🔧 Quick App Restore"),
        n_("After you install another desktop environment, your default apps (terminal, \
            file manager, text editor) may be switched to its own. This button brings \
            back the native apps of the current desktop."),
    ),
    (
        "completeness",
        n_("🧩 Desktop Completeness"),
        n_("Rexon checks that everything the current desktop needs is in place: the right \
            themes, fonts and add-ons. At login the check only looks and installs \
            nothing, so it doesn't ask for a password at startup. Here you can install \
            what's missing (the system will ask for a password)."),
    ),
    (
        "wpGroup",
        n_("🎬 Wallpaper (image / video / YouTube / web)"),
        n_("Set a still image, a local video, YouTube/Vimeo or a web page as your desktop \
            wallpaper"),
    ),
    (
        "libraryGroup",
        n_("📚 Wallpaper Library"),
        n_("Wallpaper packs installed in Rexon (.rxn files). “Apply” puts the wallpaper \
            on your desktop; the packs themselves come from the OCS Theme Store or the \
            “Extensions” tab"),
    ),
    (
        "importGroup",
        n_("🪟 Import Windows Themes"),
        n_("Bring over the look of your old Windows PC: wallpapers, icons, cursors and \
            visual styles. Rexon converts them to its own format and installs them."),
    ),
    (
        "blurGroup",
        n_("🌫️ Wallpaper Blur"),
        n_("Blur an image wallpaper so icons and windows on top of it are easier to read. \
            The original image is kept — the blur comes off with one click"),
    ),
    (
        "ambientGroup",
        n_("🔊 Ambient Sounds"),
        n_("Sounds that play on a loop: rain, wind, music. Change the volume here or in \
            the system mixer"),
    ),
    (
        "soundThemeGroup",
        n_("🔔 Event Sounds"),
        // TRANSLATORS: %1 is the name of the current desktop environment.
        n_("System sound theme for %1: clicks, notifications, errors. Rexon applies it \
            with the desktop's own tools"),
    ),
    (
        "fontsGroup",
        n_("🔤 System Fonts"),
        n_("Fonts of the interface, the terminal, small captions, toolbars, menus and window \
            titles, and how text is smoothed. Applies to the current desktop and to apps of \
            other toolkits as well"),
    ),
    (
        "protGroup",
        n_("🔒 Default App Protection"),
        n_("Makes sure your usual programs (browser, player, etc.) stay your default apps \
            when a new desktop is installed"),
    ),
    (
        "plasmaGroup",
        n_("🛡️ KDE Plasma Panel Protection"),
        n_("Saves the panel layout, widgets and keyboard shortcuts — and brings them back \
            if something breaks after installing another desktop"),
    ),
    (
        "aeroGroup",
        n_("🪟 Desktop Appearance"),
        n_("The desktop appearance is built for the system version that was installed at \
            the time. After a system update it has to be rebuilt — otherwise the panel, \
            menu and effects disappear without warning"),
    ),
];

/// Section frame names in display order.
pub fn section_order() -> Vec<&'static str> {
    SECTIONS.iter().map(|(id, _, _)| *id).collect()
}

/// Section heading and description in the interface language. `de_display`
/// is substituted for `%1`.
/// An unknown frame name gives `None` (the caller must say so out loud, not
/// draw an empty frame).
pub fn section_texts(id: &str, de_display: &str) -> Option<(String, String)> {
    let (_, title, desc) = SECTIONS.iter().find(|(name, _, _)| *name == id)?;
    let title = if title.is_empty() {
        // The onboarding section is labeled by the onboarding itself: one feature
        // must not have two wordings of "what this is".
        format!("🎓 {}", crate::onboarding::settings_label())
    } else {
        tr(title)
    };
    // ⭐ Fix: «System sound theme for YOUR DESKTOP»: direct substitution did not
    // inflect the name of an unrecognized desktop.
    Some((title, crate::screenshots::place_de_name(&tr(desc), "%1", de_display)))
}

// ============================================================================
// Catalog of tab captions and messages
// ============================================================================

/// All constant strings of the tab: captions, tooltips, status messages.
///
/// One catalog instead of eighty one-off functions (rule #12): all these
/// strings share one fate: Qt reads them and shows them as is, while separate
/// functions below decide "which one exactly". `%1`, `%2` are placeholders,
/// substituted by [`text`].
///
/// TRANSLATION. The templates are English `msgid`s under the `n_` marker:
/// `xgettext` reads the table, and [`text_template`] translates it at display
/// time. Russian lives in `po/ru.po` as the previous text verbatim. Strings
/// without words (icons, a bare `%1`, a theme icon name) carry no marker:
/// there is nothing to translate in them, and the translator does not see
/// them. The key is an attribute for the code, the caption is only for a
/// person: the code compares KEYS, not text.
pub const TEXTS: &[(&str, &str)] = &[
    // window and shared part
    ("window.title", n_("Rexon — Settings")),
    // «⚠️ Error: …» is one wording for the whole tab: both for a wallpaper
    // engine failure and for an exception while writing a setting.
    ("status.error", n_("⚠️ Error: %1")),
    // A bare warning: the engine message already explains what failed, and the
    // word «Error» in front of it would be a second one.
    ("status.warn", "⚠️ %1"),
    ("page.title", n_("⚙️ Settings")),
    // Answers of the confirmation dialog. The widget tab took them from Qt
    // (`QMessageBox::Yes/No`), but the QML page shows the confirmation INSIDE
    // itself, with a dimmed background and a focus chain, as the Big Picture
    // canon requires (operating system dialogs are inconvenient there). So the
    // answer captions live where the other tab captions are.
    ("dialog.yes", n_("Yes")),
    ("dialog.no", n_("No")),
    ("polkit.banner", "⚠️ %1"),
    // The right to change the system (B13): the same captions as in the tray
    // icon menu (`tray`, keys `admin.*`), one and the same state.
    // The «🔓» icon is set by the page: the same as on the tray menu item.
    (
        "admin.info",
        // TRANSLATORS: shown while Rexon may change the system without asking for the password again.
        n_("Rexon can change the system until the window is closed"),
    ),
    // TRANSLATORS: button/menu item: stop letting Rexon change the system; next time it asks for the password.
    ("admin.revoke", n_("Revoke Now")),
    // interface language
    ("lang.row", n_("<b>Language:</b>")),
    (
        "lang.system",
        // TRANSLATORS: first entry of the language list; %1 is the name of the
        // language the system is set to, in that language (“Deutsch”).
        n_("Same as system (%1)"),
    ),
    ("lang.restart_hint", n_("The language will change after Rexon restarts.")),
    ("lang.restart", n_("Restart Rexon")),
    (
        "lang.saved",
        // TRANSLATORS: %1 is the chosen language, in that language (“English”).
        n_("🌐 Language saved: %1"),
    ),
    ("lang.restart_failed", n_("⚠️ Could not restart Rexon: %1")),
    // interface skin
    ("skin.row", n_("<b>Skin:</b>")),
    (
        "skin.hint",
        n_("💡 “System” uses your system's look as is. The other skins set their own \
            colors regardless of the system theme."),
    ),
    (
        "skin.applied",
        // TRANSLATORS: %1 is the skin name.
        n_("🎨 Interface skin applied: %1"),
    ),
    // onboarding
    ("ob.check", n_("Show the tour on first launch")),
    ("ob.on", n_("🎓 Tour turned on")),
    ("ob.off", n_("🎓 Tour turned off")),
    (
        "ob.no_main_window",
        n_("🎓 The tour is only available in the main Rexon window — open the full window, \
            not a single tab"),
    ),
    ("ob.started", n_("🎓 Tour started")),
    // links to websites (`link_policy`)
    ("links.reset", n_("Ask again for all websites")),
    // file manager
    ("fm.installed", n_("✅ installed")),
    ("fm.missing", n_("❌ not installed")),
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    ("fm.auto_hint", n_("← follows the desktop")),
    (
        "fm.wm_note",
        n_("Hyprland and other window managers have no file manager of their own — any \
            one from the list will do (people usually install Dolphin or Thunar)."),
    ),
    ("fm.apply", n_("Apply File Manager Settings")),
    ("fm.current", n_("Current file manager: <b>%1</b>")),
    ("fm.unknown", "?"),
    ("fm.applied", n_("✅ File manager applied")),
    ("fm.not_applied", n_("⚠️ Could not apply")),
    // quick restore of native applications
    (
        "quickfix.current",
        // TRANSLATORS: %1 is the name of the current desktop environment.
        n_("Current desktop: <b>%1</b>"),
    ),
    (
        "quickfix.button",
        // TRANSLATORS: %1 is the name of the current desktop environment, e.g. KDE.
        n_("Restore %1 Apps"),
    ),
    (
        "quickfix.unknown_de",
        // TRANSLATORS: %1 is the name of the current desktop environment.
        n_("The native app set for “%1” is unknown. Choose apps manually in the “Default \
            App Protection” section below."),
    ),
    ("quickfix.notify_title", "Rexon"),
    ("quickfix.notify_icon", "preferences-desktop-default-applications"),
    // desktop completeness
    ("compl.idle", n_("The check hasn't run yet")),
    ("compl.button", n_("Check and Install Missing")),
    ("compl.running", n_("Checking…")),
    ("compl.busy", n_("⏳ A check is already running")),
    ("compl.failed_log", n_("Check failed: %1")),
    // wallpapers
    ("wp.no_engine", n_("⚠️ The wallpaper engine didn't load — this section is unavailable")),
    ("wp.stop", n_("⏹️ Stop Live Wallpaper")),
    ("wp.from_file", n_("<b>From a file</b>")),
    ("wp.file_hint", n_("Image: JPG, PNG, WebP.  Video: MP4, WebM, MKV, AVI, WMV, MPEG, GIF.  Flash: SWF.")),
    // ⚠️ Without listing kinds: the checkbox and the slider control the sound of
    // ALL wallpapers of our host (video, web, Flash share the same
    // `wallpaper_audio_*` keys), and a list in parentheses went stale with every
    // new kind (15.11: «video and Flash» while web wallpapers had live sound).
    // Wallpapers of the Wallpaper Engine tab (waywallen) have their own volume;
    // the tooltip `wp.audio_hint` says so.
    ("wp.video_audio", n_("🔊 Wallpaper sound")),
    ("wp.pick_image", n_("🖼️ Image…")),
    ("wp.pick_video", n_("🎬 Video…")),
    ("wp.busy", n_("⚠️ Wallpaper is already being applied")),
    (
        "wp.applying",
        // TRANSLATORS: %1 is a file name, %2 is the wallpaper kind.
        n_("⏳ Applying: %1 (%2)…"),
    ),
    ("wp.applied", n_("✅ Wallpaper applied: %1")),
    // TRANSLATORS: %1 is the wallpaper result ("Video wallpaper set: Forest"),
    // %2 is a whole sentence explaining what is still wrong.
    ("wp.applied_warn", n_("⚠️ Wallpaper applied: %1. %2")),
    ("wp.stopped", n_("⏹️ Live wallpaper stopped")),
    (
        "wp.audio_live",
        // TRANSLATORS: %1 is “on” or “off”.
        n_("🔊 Wallpaper sound: %1"),
    ),
    (
        "wp.audio_volume",
        // TRANSLATORS: %1 is the volume in percent.
        n_("🔉 Wallpaper volume: %1%"),
    ),
    ("wp.audio_mute_other", n_("Mute when another app is playing sound")),
    ("wp.audio_mute_covered", n_("Pause the wallpaper when windows hide it")),
    ("wp.pause_battery", n_("Pause the wallpaper when running on battery")),
    (
        "wp.pause_game",
        // TRANSLATORS: “game mode” is Feral GameMode or a game started in gamescope.
        n_("Pause the wallpaper while a game is running (game mode)"),
    ),
    (
        "wp.audio_hint",
        n_("Wallpaper sound changes on the fly — no need to reapply the wallpaper. One \
            volume for all live wallpapers except those from the Wallpaper Engine tab: \
            they have their own, in that tab's settings."),
    ),
    (
        "wp.audio_on",
        // TRANSLATORS: Sound state, shown as “Wallpaper sound: on”.
        n_("on"),
    ),
    (
        "wp.audio_off",
        // TRANSLATORS: Sound state, shown as “Wallpaper sound: off”.
        n_("off"),
    ),
    ("wp.working", "⏳…"),
    // YouTube / Vimeo
    ("yt.header", "<b>▶️ YouTube / Vimeo</b>"),
    ("yt.url_label", n_("Link:")),
    ("yt.url_hint", n_("https://youtube.com/watch?v=…  or a Vimeo link")),
    ("yt.quality_label", n_("Quality:")),
    ("yt.mode_label", n_("Mode:")),
    ("yt.apply", n_("▶️ Apply")),
    ("yt.audio", n_("🔊 YouTube video sound")),
    (
        "yt.audio_next",
        // TRANSLATORS: %1 is “on” or “off”.
        n_("🔊 YouTube sound: %1 (takes effect the next time a video starts)"),
    ),
    ("yt.no_url", n_("⚠️ Paste a video link")),
    ("yt.no_ytdlp", n_("⚠️ yt-dlp is not installed — click “Install yt-dlp”")),
    (
        "yt.starting",
        // TRANSLATORS: %1 is the video quality, %2 is the mode, %3 is “on” or “off” for sound.
        n_("⏳ YouTube (%1, %2, sound %3) — this may take a while…"),
    ),
    ("yt.applied", n_("✅ YouTube wallpaper: %1")),
    // yt-dlp
    ("ytdlp.have", "yt-dlp: ✅ <tt>v%1</tt>"),
    ("ytdlp.missing", n_("⚠️ yt-dlp is not installed — it's needed for YouTube wallpapers")),
    ("ytdlp.install", n_("📦 Install yt-dlp")),
    ("ytdlp.update", n_("🔄 Update yt-dlp")),
    ("ytdlp.installing", n_("Installing")),
    ("ytdlp.updating", n_("Updating")),
    ("ytdlp.working", "⏳ %1…"),
    (
        "ytdlp.password",
        // TRANSLATORS: %1 is “Installing” or “Updating”.
        n_("⏳ %1 yt-dlp (you'll be asked for a password)…"),
    ),
    ("ytdlp.done", "✅ yt-dlp: %1"),
    // web wallpapers
    ("web.header", n_("<b>🌐 Web Wallpaper</b>")),
    ("web.hint", n_("A link to a web page or the path to an HTML file on this computer.")),
    // Fix (rule #13): the path example was an English «/path/to/…»: the customer
    // read a foreign language and an internal word in the input field instead of
    // a clear example.
    ("web.url_hint", n_("https://example.com  or  /home/name/page.html")),
    ("web.apply", n_("🌐 Apply")),
    ("web.no_url", n_("⚠️ Paste a link or the path to an HTML file")),
    ("web.loading", n_("⏳ Loading web wallpaper: %1…")),
    ("web.applied", n_("✅ Web wallpaper: %1")),
    // wallpaper library
    ("lib.no_engine", n_("The wallpaper engine is unavailable")),
    (
        "lib.empty",
        n_("Nothing here yet. Wallpaper packs are installed from the OCS Theme Store, and \
            you can drop your own .rxn file into the “Extensions” tab."),
    ),
    ("lib.unnamed", n_("Untitled")),
    ("lib.apply", n_("Apply")),
    ("lib.preview", "👁️"),
    ("lib.preview_tip", n_("Open the file in the system viewer")),
    ("lib.no_file", n_("⚠️ The wallpaper file is unavailable")),
    (
        "lib.applying",
        // TRANSLATORS: %1 is a wallpaper name, %2 is the wallpaper kind.
        n_("⏳ Applying “%1” (%2)…"),
    ),
    // Windows theme import
    (
        "imp.formats",
        n_("Supported: themes (.themepack, .theme), visual styles (.msstyles), icons \
            (.ico, .icl), cursors (.cur, .ani), DreamScene video wallpapers (.dream, \
            .wmv)."),
    ),
    ("imp.pick", n_("📥 Choose Theme File…")),
    ("imp.dialog_title", n_("Choose a Windows theme file")),
    (
        "imp.unavailable_note",
        n_("⚠️ Theme import is currently unavailable in this Rexon build."),
    ),
    ("imp.unavailable", n_("⚠️ Theme import is currently unavailable in this build")),
    ("imp.busy", n_("⚠️ An import is already running — please wait")),
    ("imp.working", n_("⏳ Importing the theme…")),
    ("imp.working_file", n_("⏳ Importing the theme: %1…")),
    ("imp.working_status", n_("⏳ Importing the theme — this may take a while…")),
    (
        "imp.default_name",
        // TRANSLATORS: Used in place of a theme name when the theme has none.
        n_("theme"),
    ),
    ("imp.installed", n_("✅ “%1” is installed — you'll find it in the “Extensions” tab.")),
    ("imp.installed_status", n_("✅ Theme installed: %1")),
    ("imp.not_found", n_("file not found")),
    ("imp.broken", n_("the file is damaged or its format is not supported")),
    ("imp.failed", n_("⚠️ Could not import: %1")),
    // wallpaper blur
    (
        "blur.no_pillow",
        n_("⚠️ Blur is unavailable: the Pillow library is missing (package \
            <tt>python-pillow</tt>)"),
    ),
    ("blur.strength", n_("Strength:")),
    // The strength slider value. Fix (rule #13): it was «20px»: the customer has
    // no need to know about pixels, a blur radius tells them nothing. What
    // remains is the scale division itself next to the «Strength:» caption, as
    // with any slider. The key is kept: the window and the page read it, and a
    // person does not see it.
    ("blur.px", "%1"),
    ("blur.apply", n_("🌫️ Blur Wallpaper")),
    ("blur.remove", n_("🔆 Remove Blur")),
    ("blur.busy", n_("⚠️ A blur operation is already running")),
    (
        "blur.applying",
        // TRANSLATORS: %1 is the blur strength (a number).
        n_("⏳ Blurring the wallpaper (strength %1)…"),
    ),
    ("blur.removing", n_("⏳ Removing the blur and restoring the previous wallpaper…")),
    ("blur.done", n_("The current wallpaper is blurred")),
    (
        "blur.failed",
        n_("Could not blur: there's no image wallpaper right now, or it couldn't be read"),
    ),
    ("blur.removed", n_("Blur removed, wallpaper restored")),
    ("blur.nothing_to_restore", n_("No saved wallpaper to restore")),
    // background sounds
    (
        "amb.no_manager",
        n_("⚠️ Ambient sounds are unavailable: the wallpaper engine didn't load"),
    ),
    (
        "amb.no_mpv",
        n_("⚠️ Ambient sounds are unavailable: the <tt>mpv</tt> package is not installed"),
    ),
    ("amb.add", n_("➕ Add File…")),
    ("amb.add_tip", n_("Play the chosen audio file on a loop in the background")),
    ("amb.dialog_title", n_("Choose an audio file")),
    ("amb.volume", n_("🔊 Volume:")),
    ("amb.percent", "%1%"),
    ("amb.stop_all", n_("⏹️ Stop All")),
    (
        "amb.volume_hint",
        n_("The slider sets the volume of a <b>new</b> sound. To change the volume of one \
            that's already playing, click 🔊 in its row."),
    ),
    ("amb.empty", n_("No ambient sounds are playing. Click “Add File…”.")),
    ("amb.row_name", "▶️ <b>%1</b>"),
    ("amb.row_volume", "🔊 %1%"),
    ("amb.row_unknown", "?"),
    ("amb.setvol", "🔊"),
    ("amb.setvol_tip", n_("Set this sound's volume from the slider")),
    ("amb.stop", "⏹️"),
    ("amb.stop_tip", n_("Stop this sound")),
    (
        "amb.started",
        // TRANSLATORS: %1 is a file name, %2 is the volume in percent.
        n_("✅ Started: %1 (%2%)"),
    ),
    ("amb.start_failed", n_("⚠️ Could not start (mpv is required)")),
    (
        "amb.start_unplayable",
        n_("⚠️ The sound couldn't be played: the file can't be read or there's no audio \
            output device"),
    ),
    ("amb.start_error", n_("⚠️ Error starting the sound: %1")),
    (
        "amb.volume_set",
        // TRANSLATORS: %1 is a sound name, %2 is the volume in percent.
        // Also: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        n_("🔊 Volume of “%1” → %2%"),
    ),
    ("amb.not_playing", n_("⚠️ This sound is no longer playing")),
    ("amb.stopped", n_("⏹️ Stopped: %1")),
    ("amb.stopped_all", n_("⏹️ All ambient sounds stopped")),
    // event sounds
    ("snd.theme_label", n_("<b>Sound theme:</b>")),
    // The choice line when the system theme is not set at all or is named so
    // that it is not among the installed ones. Without it the list showed the
    // FIRST installed theme, and the person read someone else's value as their own.
    ("snd.theme_unset", n_("— not set —")),
    // The system has a theme selected that is not on disk (it was removed, but
    // the reference remained): say that, not "not set", or the person will not
    // learn why there are no sounds. %1 is the theme name from the system settings.
    ("snd.theme_missing", n_("“%1” — theme not found")),
    ("snd.events_check", n_("Turn on event sounds (clicks, notifications, errors)")),
    ("snd.test_label", n_("<b>Sound test:</b>")),
    (
        "snd.themes_hint",
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        n_("💡 Not many themes? There are more in Appearance → Sounds."),
    ),
    ("snd.no_events", n_("This theme has no sounds to test")),
    ("snd.event_tip", n_("Event sound: %1")),
    ("snd.theme_applied", n_("✅ Sound theme: %1")),
    ("snd.theme_failed", n_("⚠️ Could not apply the theme: %1")),
    (
        "snd.events_state",
        // TRANSLATORS: %1 is “turned on” or “turned off”.
        n_("✅ Event sounds %1"),
    ),
    // "on/off" is a separate entry: the word is inserted into the result of
    // toggling event sounds.
    (
        "state.on",
        // TRANSLATORS: Shown as “Event sounds turned on”, “Desktop icons turned on”.
        n_("turned on"),
    ),
    (
        "state.off",
        // TRANSLATORS: Shown as “Event sounds turned off”, “Desktop icons turned off”.
        n_("turned off"),
    ),
    ("snd.events_failed", n_("⚠️ Could not switch event sounds")),
    ("snd.cannot_play", "🔇 %1"),
    // system fonts
    // No font set means the desktop's default font is in effect; "not set" read
    // as if there were no font at all.
    ("font.unset", n_("<i>(default)</i>")),
    ("font.value", "<i>%1</i>"),
    ("font.placeholder", "—"),
    (
        "font.de_hint",
        // TRANSLATORS: %1 is the name of the current desktop environment.
        n_("Current desktop: <b>%1</b>"),
    ),
    ("font.applied", "✅ %1: %2 %3pt"),
    (
        "font.failed",
        // TRANSLATORS: %1 is the font kind exactly as titled in the list (“System Font”, “Monospace Font”…), keep it at the start.
        n_("⚠️ %1: could not apply"),
    ),
    ("font.all_mode", n_("Change:")),
    ("font.dpi", n_("Force font DPI:")),
    ("font.render_title", n_("<b>Text smoothing</b>")),
    ("font.aa", n_("Anti-aliasing")),
    ("font.aa_exclude", n_("Don't smooth sizes from")),
    (
        "font.aa_to",
        // TRANSLATORS: between two font sizes: “Don't smooth sizes from [8] to [15] pt”.
        n_("to"),
    ),
    (
        "font.pt",
        // TRANSLATORS: abbreviation of “points”, the unit of font size.
        n_("pt"),
    ),
    ("font.render_presets", n_("Quick choice:")),
    ("font.subpixel", n_("Sub-pixel rendering:")),
    ("font.hinting", n_("Hinting:")),
    (
        "font.render_hint",
        n_("Saved to the shared font settings file that apps on GTK, Qt and Electron and web \
            browsers read, and to this desktop's own settings"),
    ),
    (
        "font.render_saved",
        n_("✅ Text smoothing saved. Apps that are already open may need a restart"),
    ),
    (
        "font.render_native_failed",
        n_("⚠️ Text smoothing: the desktop's own settings could not be changed"),
    ),
    (
        "font.fc_refused",
        // TRANSLATORS: %1 is the path of the fontconfig file (fonts.conf).
        n_("⚠️ The font settings file %1 can't be read safely, so Rexon left it untouched. \
            Fix or remove it and try again"),
    ),
    (
        "font.fc_failed",
        // TRANSLATORS: %1 is the path of the fontconfig file (fonts.conf).
        n_("⚠️ Text smoothing: could not write %1"),
    ),
    // default application protection
    ("prot.enable", n_("Turn on protection  (recommended)")),
    (
        "prot.remembered",
        // TRANSLATORS: %1 is an app type (e.g. Browser), %2 is the remembered app.
        n_("%1   ·  remembered: %2"),
    ),
    (
        "prot.not_remembered",
        // TRANSLATORS: %1 is an app type (e.g. Browser).
        n_("%1   ·  not remembered yet"),
    ),
    ("prot.desktop_file", n_("Shortcut file: %1")),
    ("prot.note", "ℹ️ %1"),
    ("prot.snapshot_title", n_("<b>Remember the current choice</b>")),
    (
        "prot.snapshot_sub",
        n_("Rexon will note which apps are your defaults right now and bring exactly \
            those back if they get replaced"),
    ),
    ("prot.snapshot", n_("📌 Remember Now")),
    ("prot.restore", n_("↩️ Restore Previous")),
    ("prot.snapshot_ok", n_("✅ Default apps saved")),
    // Fix: it was «⚠️ Error snapshot: …», jargon in the customer's face
    // (rule #13). The same meaning, in human words.
    ("prot.snapshot_error", n_("⚠️ Could not remember the choice: %1")),
    ("prot.restore_ok", n_("✅ Default apps restored")),
    // terminal colors after a theme change (`terminal_guard`)
    ("prot.terminal_colors", n_("Don't let themes spoil terminal colors  (recommended)")),
    (
        "prot.terminal_colors_tip",
        n_("Some terminals take their text and background colors from the theme, and a dark \
            theme can leave them dark gray on black. After a theme, color scheme or accent \
            changes, Rexon gives such terminals readable colors of their own. Colors you \
            picked yourself are left alone, and “Return as It Was” brings the old ones back."),
    ),
    // Plasma panel
    ("plasma.backup_title", n_("<b>Save the current panel layout</b>")),
    (
        "plasma.backup_sub",
        n_("Rexon will make a copy of the panel layout, widgets and keyboard shortcuts — \
            you can bring it back in one click"),
    ),
    (
        "plasma.backup_tip",
        n_("Settings files that get copied: plasma-org.kde.plasma.desktop-appletsrc, \
            plasmashellrc, kglobalshortcutsrc"),
    ),
    ("plasma.backup", n_("📦 Save Copy")),
    ("plasma.backups_header", n_("<b>Saved copies (%1):</b>")),
    ("plasma.no_backups", n_("No saved copies yet")),
    ("plasma.restore", n_("Restore")),
    ("plasma.backup_dir", n_("Copy folder: %1")),
    ("plasma.confirm_title", n_("Restore?")),
    (
        "plasma.confirm_text",
        // TRANSLATORS: %1 is the date and time of the saved copy.
        n_("Restore the Plasma panel from the copy made %1?\n\nThe current panel layout \
            will be replaced."),
    ),
    // Fix: it was «⚠️ Error backup: …», the same jargon word.
    ("plasma.backup_error", n_("⚠️ Could not save a copy: %1")),
    // desktop look (AeroShell)
    ("aero.check", n_("Check the desktop appearance when Rexon starts")),
    (
        "aero.check_tip",
        n_("Rexon compares the version of the desktop appearance with the system version \
            and offers to restore it if they no longer match. A rebuild NEVER starts \
            until you click"),
    ),
    ("aero.check_now", n_("🔍 Check Now")),
    ("aero.on", n_("🪟 The desktop appearance will be checked at startup")),
    ("aero.off", n_("🪟 Desktop appearance check turned off")),
    ("aero.up_to_date", n_("✅ The desktop appearance matches the current system version")),
    // ── Tray icon ───────────────────────────────────────────────────────
    //
    // ⚠️ The checkbox caption is the same string as in the icon's own menu
    // (`crate::tray`, key `autostart`), and that is no coincidence: one setting,
    // two doors into it. A divergence is caught by the `tray::tests` test.
    ("tray.autostart", n_("Start at login")),
    (
        "tray.autostart_hint",
        n_("Rexon will start in the tray when you log in, without showing the window"),
    ),
    ("tray.autostart_on", n_("✅ Rexon will start when you log in")),
    ("tray.autostart_off", n_("Start at login turned off")),
    ("tray.autostart_failed", n_("⚠️ Could not change autostart — see the log for details")),
    (
        "tray.missing",
        n_("⚠️ There's no tray in this session: the icon wasn't added, and closing the \
            window quits Rexon as usual"),
    ),
    // fetching icons and cursor for a theme (`theme_watch`)
    ("tray.theme_deps", n_("Download icons and cursors for the theme automatically")),
    (
        "tray.theme_deps_hint",
        n_("When a theme you pick — in Rexon or in your desktop's own settings — needs icons \
            or a cursor that aren't installed, Rexon downloads them into your home folder and \
            applies them. Turned off, it asks first. Sets that only come as system packages \
            are always asked about."),
    ),
    ("tray.theme_deps_on", n_("✅ Icons and cursors for the theme will be downloaded automatically")),
    ("tray.theme_deps_off", n_("Rexon will ask before downloading icons and cursors for the theme")),
];

/// The source (English) template by catalog key: the `msgid`, untranslated.
///
/// Needed by checks that keep one wording for two doors (the autostart
/// caption in the tray menu and in «Settings»): the sources must be
/// compared, not what the machine's language chose.
pub fn text_msgid(key: &str) -> Option<&'static str> {
    TEXTS.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// The string template by catalog key, translated into the interface language.
pub fn text_template(key: &str) -> Option<String> {
    text_msgid(key).map(tr)
}

/// A ready catalog string: `%1`, `%2` are replaced by `args` in order.
/// An unknown key gives `None`: the caller must fail with a clear error, not
/// show the customer an empty label.
pub fn text(key: &str, args: &[String]) -> Option<String> {
    // A phrase where the desktop name does not follow a preposition («Restore %1
    // Apps»): for an unrecognized desktop a whole phrase of its own, not a splice.
    if key == "quickfix.button"
        && args.first().is_some_and(|a| crate::screenshots::is_unknown_de_display(a))
    {
        // TRANSLATORS: button: restore the default apps of a desktop
        // environment Rexon couldn't recognize.
        return Some(tr("Restore Your Desktop's Apps"));
    }
    let mut out = text_template(key)?;
    for (i, arg) in args.iter().enumerate() {
        // Any placeholder may receive the name of an unrecognized desktop
        // («quickfix.unknown_de», «quickfix.button»): the correct form after a
        // preposition is chosen by the shared `place_de_name`, other values it
        // substitutes as is.
        out = crate::screenshots::place_de_name(&out, &format!("%{}", i + 1), arg);
    }
    Some(out)
}

/// Truncating a long engine message by characters (not by bytes, otherwise
/// Cyrillic text breaks in the middle of a letter).
pub fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

/// A ready catalog string without substitutions: the short form of [`text`].
fn t(key: &str) -> String {
    text_template(key).unwrap_or_default()
}

/// A catalog string with substitutions: the short form for core code.
fn tf(key: &str, args: &[&str]) -> String {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    text(key, &owned).unwrap_or_default()
}

// ============================================================================
// Tab decisions: which string exactly to show and what to do next
// ============================================================================

/// The "installed / not installed" caption for the file manager line.
pub fn fm_badge(installed: bool) -> String {
    if installed {
        t("fm.installed")
    } else {
        t("fm.missing")
    }
}

/// Result of applying the file manager.
pub fn fm_apply_status(ok: bool) -> String {
    if ok {
        t("fm.applied")
    } else {
        t("fm.not_applied")
    }
}

/// The yt-dlp line and button by the current system state:
/// (label, button caption, button action: `install` or `update`).
///
/// Fix (it was in the original before the port): the state was read once
/// when the section was built, and after installing the button stayed
/// «Install». The decision is here so that both sides (building and refreshing
/// after the install) read one rule.
pub fn ytdlp_row(installed: bool, version: &str) -> (String, String, String) {
    if installed {
        (
            tf("ytdlp.have", &[version]),
            t("ytdlp.update"),
            "update".to_string(),
        )
    } else {
        (
            t("ytdlp.missing"),
            t("ytdlp.install"),
            "install".to_string(),
        )
    }
}

/// Captions for the yt-dlp install/update progress: (button caption, status).
pub fn ytdlp_working(action: &str) -> (String, String) {
    let verb = if action == "update" {
        t("ytdlp.updating")
    } else {
        t("ytdlp.installing")
    };
    (
        tf("ytdlp.working", &[&verb]),
        tf("ytdlp.password", &[&verb]),
    )
}

/// Result of installing/updating yt-dlp.
pub fn ytdlp_done(ok: bool, msg: &str) -> String {
    let msg = clip(msg, 180);
    if ok {
        tf("ytdlp.done", &[&msg])
    } else {
        tf("status.warn", &[&msg])
    }
}

/// Message about toggling the wallpaper sound, applied ON THE FLY.
///
/// The former «🔊 Video sound: %1 (applies to the next video)» and
/// «⏳ %1 live wallpaper sound…» were removed on 15.11.2026 together with
/// their functions: the wallpaper host picks up the setting itself without
/// restarting anything, so both phrases were untrue, and there was nobody
/// left to show them anyway.
pub fn wallpaper_audio_live_status(on: bool) -> String {
    tf("wp.audio_live", &[&audio_word(on)])
}

/// Caption of the wallpaper volume slider.
pub fn wallpaper_volume_label(percent: i64) -> String {
    tf(
        "wp.audio_volume",
        &[&crate::wallpaper_audio::clamp_volume_pct(percent).to_string()],
    )
}

/// Message about toggling the YouTube wallpaper sound.
pub fn youtube_audio_status(on: bool) -> String {
    tf("yt.audio_next", &[&audio_word(on)])
}

/// "on" or "off": one wording for the whole tab.
fn audio_word(on: bool) -> String {
    if on {
        t("wp.audio_on")
    } else {
        t("wp.audio_off")
    }
}

/// Status of starting to apply wallpaper from a file.
pub fn wallpaper_applying_status(basename: &str, format: &str) -> String {
    tf("wp.applying", &[basename, format])
}

/// Result of applying wallpaper (shared by file, library and sound toggling).
pub fn wallpaper_done(ok: bool, msg: &str) -> String {
    wallpaper_done_warned(ok, msg, None)
}

/// Result of applying wallpaper with the executor's warning
/// (`WallpaperResult::warning`). The wallpaper is set, but there is a problem:
/// a warning icon instead of a green check (rule #13). Only the success
/// message is truncated (it holds the long wallpaper name), the warning is
/// kept whole: the status line wraps text, and truncated advice is useless.
pub fn wallpaper_done_warned(ok: bool, msg: &str, warning: Option<&str>) -> String {
    done_or_warned(ok, msg, warning, |ok, msg| {
        let msg = clip(msg, 160);
        if ok {
            tf("wp.applied", &[&msg])
        } else {
            tf("status.error", &[&msg])
        }
    })
}

/// The shared fork of apply results (wallpaper, YouTube, web): no warning or
/// a refusal gives the regular result `plain`; success with a warning gives
/// the line «⚠️ Wallpaper applied: … . <warning>» for any wallpaper kind.
pub fn done_or_warned(
    ok: bool,
    msg: &str,
    warning: Option<&str>,
    plain: impl FnOnce(bool, &str) -> String,
) -> String {
    match warning.map(str::trim).filter(|w| !w.is_empty()) {
        Some(warning) if ok => {
            let msg = clip(msg, 160);
            let msg = msg.trim_end_matches(['.', ' ']);
            tf("wp.applied_warn", &[msg, warning])
        }
        _ => plain(ok, msg),
    }
}

/// Whether the result line warns (starts with the «⚠» sign: a warning or an
/// error); then the status line is painted with [`COLOR_WARN`]. Icons enter
/// catalog strings untranslated, so the rule is the same for all languages.
pub fn status_is_warning(text: &str) -> bool {
    text.trim_start().starts_with('⚠')
}

/// Title of the wallpaper file picker: «Choose wallpaper» / «Choose a video»
/// (the Russian translation keeps the former wording).
pub fn wallpaper_dialog_title(kind: &str) -> String {
    match kind {
        "video" => tr("Choose a video"),
        "swf" => tr("Choose a Flash file"),
        "static" => tr("Choose an image"),
        _ => tr("Choose wallpaper"),
    }
}

/// Status of starting to apply YouTube wallpaper.
pub fn youtube_starting_status(quality: &str, mode: &str, audio: bool) -> String {
    // ⭐ Fix: the line showed internal values ("stream", "best"). The captions
    // come from the same lists as the tab's drop-down menus; an unknown value is
    // shown as is.
    let label = |table: &[(&str, &str)], value: &str| {
        table
            .iter()
            .find(|(_, v)| *v == value)
            .map(|(l, _)| tr(l))
            .unwrap_or_else(|| value.to_string())
    };
    tf(
        "yt.starting",
        &[&label(YT_QUALITIES, quality), &label(YT_MODES, mode), &audio_word(audio)],
    )
}

/// Result of applying YouTube wallpaper.
pub fn youtube_done(ok: bool, msg: &str) -> String {
    let msg = clip(msg, 180);
    if ok {
        tf("yt.applied", &[&msg])
    } else {
        tf("status.error", &[&msg])
    }
}

/// A web wallpaper URL in the form the engine understands: a local path
/// becomes `file://`. `exists` is whether such a file exists (an observation).
pub fn web_wallpaper_url(url: &str, exists: bool) -> String {
    let url = url.trim();
    if url.starts_with('/') && exists {
        return format!("file://{url}");
    }
    url.to_string()
}

/// Status of starting to load web wallpaper.
pub fn web_loading_status(url: &str) -> String {
    tf("web.loading", &[&clip(url, 80)])
}

/// Result of applying web wallpaper.
pub fn web_done(ok: bool, msg: &str) -> String {
    let msg = clip(msg, 180);
    if ok {
        tf("web.applied", &[&msg])
    } else {
        tf("status.error", &[&msg])
    }
}

/// Result of stopping live wallpaper.
pub fn wallpaper_stop_status(ok: bool, msg: &str) -> String {
    if ok {
        t("wp.stopped")
    } else {
        tf("status.warn", &[msg])
    }
}

// ── Wallpaper library ───────────────────────────────────────────────

/// Whether a tile shows the "open in the system" preview button.
///
/// Fix: the original compared the format against its own list
/// `("video","gif","swf","webm","mkv","mp4")`, which mixes wallpaper formats
/// and file extensions, so `.avi`, `.mov`, `.m4v` did not get the button,
/// although the engine plays these videos. Now the decision reads the shared
/// list [`VIDEO_EXTS`] plus flash (`swf`), which the engine also hands out.
pub fn library_shows_preview(format: &str) -> bool {
    let fmt = format.trim().to_lowercase();
    fmt == "video" || fmt == "swf" || VIDEO_EXTS.contains(&fmt.as_str())
}

/// How many ROWS of tiles to show in the library section.
pub const LIBRARY_ROWS: usize = 3;

/// Status of starting to apply wallpaper from the library.
pub fn library_applying_status(basename: &str, format: &str) -> String {
    tf("lib.applying", &[basename, format])
}

// ── Windows theme import ────────────────────────────────────────────

/// A clear reason why the theme was not imported.
///
/// `missing` means the file was not found; otherwise the engine message is
/// taken, and an empty one is replaced by a general wording ("" tells a person nothing).
pub fn import_failure_reason(missing: bool, raw: &str) -> String {
    if missing {
        return t("imp.not_found");
    }
    let raw = raw.trim();
    if raw.is_empty() {
        t("imp.broken")
    } else {
        raw.to_string()
    }
}

/// Section label and status after a failed import: (label, status).
pub fn import_failure_texts(reason: &str) -> (String, String) {
    (
        tf("imp.failed", &[reason]),
        tf("imp.failed", &[&clip(reason, 160)]),
    )
}

/// Section label and status after a successful import: (label, status).
pub fn import_success_texts(name: &str, notes: &[String]) -> (String, String) {
    let shown = if name.trim().is_empty() {
        t("imp.default_name")
    } else {
        name.trim().to_string()
    };
    (
        // Warnings about skipped steps go into the EXPLANATION (it stays on the
        // page), not into the transient status line: the line disappears, while
        // the person needs to read "why there is no cover" to the end.
        crate::importers::with_skipped_notes(&tf("imp.installed", &[&shown]), notes),
        tf("imp.installed_status", &[&shown]),
    )
}

// ── Wallpaper blur ──────────────────────────────────────────────────

/// Blur strength slider: (minimum, maximum, step, default value).
pub const BLUR_RANGE: (i32, i32, i32, i32) = (5, 50, 5, 20);

/// Result of blurring/removing the blur: the message of the operation itself.
pub fn blur_result(action: &str, ok: bool) -> String {
    match (action, ok) {
        ("blur", true) => t("blur.done"),
        ("blur", false) => t("blur.failed"),
        (_, true) => t("blur.removed"),
        (_, false) => t("blur.nothing_to_restore"),
    }
}

/// Blur result for the status line.
pub fn blur_done(ok: bool, msg: &str) -> String {
    let msg = clip(msg, 160);
    if ok {
        format!("✅ {msg}")
    } else {
        format!("⚠️ {msg}")
    }
}

// ── Background sounds ───────────────────────────────────────────────

/// Default volume of a new background sound, %.
pub const AMBIENT_DEFAULT_VOLUME: i32 = 50;

/// Line of the active background sound: (name, volume).
pub fn ambient_row_texts(name: Option<&str>, volume: Option<&str>) -> (String, String) {
    let unknown = t("amb.row_unknown");
    (
        tf("amb.row_name", &[name.unwrap_or(&unknown)]),
        tf("amb.row_volume", &[volume.unwrap_or(&unknown)]),
    )
}

/// Result of starting a background sound.
pub fn ambient_start_status(ok: bool, basename: &str, volume: i32) -> String {
    if ok {
        tf("amb.started", &[basename, &volume.to_string()])
    } else {
        t("amb.start_failed")
    }
}

/// A refusal to start a background sound, with the real reason.
///
/// ⭐ Fix: the former answer "mpv is needed" sounded on ANY refusal, including
/// with mpv installed, when the file itself does not play, and the person went
/// to install what they already had. `has_mpv` is whether mpv is in the system (an observation).
pub fn ambient_start_failed_status(has_mpv: bool) -> String {
    if has_mpv {
        t("amb.start_unplayable")
    } else {
        t("amb.start_failed")
    }
}

/// Result of changing a background sound's volume.
///
/// `name` is the sound name for a person, `None` means the sound no longer
/// exists (finished or removed). `ok` is whether the sound restarted with the new volume.
///
/// ⭐ Two fixes: Python showed the internal sound identifier (a volume line
/// naming «manual_rain.ogg»), and did not look at the restart result at all:
/// a sound that did not survive the volume change was reported as "volume set".
pub fn ambient_volume_status(name: Option<&str>, volume: i64, ok: bool, has_mpv: bool) -> String {
    match (name, ok) {
        (None, _) => t("amb.not_playing"),
        (Some(name), true) => tf("amb.volume_set", &[name, &volume.to_string()]),
        (Some(_), false) => ambient_start_failed_status(has_mpv),
    }
}

/// Result of stopping one background sound. `name` is the sound name for a
/// person, `None` means the sound no longer exists.
///
/// ⭐ Fix: Python wrote the internal sound identifier («Stopped:
/// manual_rain.ogg») and reported stopping even a sound that no longer existed.
pub fn ambient_stopped_status(name: Option<&str>) -> String {
    match name {
        Some(name) => tf("amb.stopped", &[name]),
        None => t("amb.not_playing"),
    }
}

// ── Event sounds ────────────────────────────────────────────────────

/// How many "listen" buttons to show in the sounds section.
pub const SOUND_PREVIEW_BUTTONS: usize = 5;

/// Result of changing the sound theme.
pub fn sound_theme_status(ok: bool, theme: &str) -> String {
    if ok {
        tf("snd.theme_applied", &[theme])
    } else {
        tf("snd.theme_failed", &[theme])
    }
}

/// Result of toggling event sounds.
pub fn event_sounds_status(ok: bool, enabled: bool) -> String {
    if !ok {
        return t("snd.events_failed");
    }
    tf("snd.events_state", &[&state_word(enabled)])
}

/// "on" or "off": a shared word for both toggle sections.
fn state_word(on: bool) -> String {
    if on {
        t("state.on")
    } else {
        t("state.off")
    }
}

// The «MATE: desktop icons» section is gone (08.11.2026): under live wallpaper
// Rexon arranges the Caja icons itself, `wallpaper_backend::prepare_desktop_icons`.

// ── Fonts ───────────────────────────────────────────────────────────

/// The label under a font button: what is set in the system now.
///
/// ⚠️ The NAME AND SIZE are shown, not how the font is written in the config.
/// KDE stores a QFont serialization of ten fields
/// (`Segoe UI,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,,0,0`), and before, this line
/// went to the customer as is: a raw technical record right in the settings
/// window (rule #13). It is parsed by the same [`parse_font_spec_face`] the tab
/// uses to open the font picker, so the label and the preselected font say
/// the same thing, including the style («Sans Bold 10»: Pango words, as GNOME
/// and XFCE show them).
///
/// A string from which the family could not be extracted is shown as is: that
/// is better than "not set" where something is actually written.
pub fn font_value_text(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return t("font.unset");
    }
    let (family, size, face) = parse_font_spec_face(raw);
    let shown = if family.is_empty() { raw.to_string() } else { font_face_spec(&family, size, face) };
    tf("font.value", &[&shown])
}

/// Result of applying a font.
pub fn font_apply_status(ok: bool, title: &str, family: &str, size: i32) -> String {
    if ok {
        tf("font.applied", &[title, family, &size.to_string()])
    } else {
        // The name as in the list, without lowercasing: in German nouns are
        // capitalized, and `to_lowercase` broke them.
        tf("font.failed", &[title])
    }
}

// ── Default application protection ──────────────────────────────────

/// Application types the protection section shows, in display order.
///
/// The same order the core computes the state in
/// ([`crate::defaults::get_status_summary`]): "terminal" goes last because it
/// is not configured through MIME. Before, the list was built in the window
/// from `MIME_TYPES.keys()`, a second assembly of the same order.
pub fn protection_types() -> Vec<&'static str> {
    crate::defaults::MIME_TYPES
        .iter()
        .map(|(k, _)| *k)
        .chain(std::iter::once("terminal"))
        .collect()
}

/// Caption of an application type checkbox and its tooltip: (caption, tooltip).
/// An empty tooltip means nothing to show.
pub fn protection_row_texts(app_type: &str, remembered: &str) -> (String, String) {
    let label = crate::defaults::app_type_label(app_type);
    let label = label.as_str();
    let pretty = pretty_desktop_id(remembered);
    if pretty.is_empty() {
        return (tf("prot.not_remembered", &[label]), String::new());
    }
    (
        tf("prot.remembered", &[label, &pretty]),
        tf("prot.desktop_file", &[remembered]),
    )
}

// ── Plasma panel ────────────────────────────────────────────────────

/// How many of the latest panel copies to show in the section.
pub const PLASMA_BACKUPS_SHOWN: usize = 5;

/// Heading of the panel copy list.
pub fn plasma_backups_header(total: usize) -> String {
    tf("plasma.backups_header", &[&total.to_string()])
}

/// The question before restoring a copy: (dialog title, text).
///
/// Fix (rule #13): the question had a machine timestamp («20.06.2026 at
/// 01:10» turned into «20260620_011005»). In the list above the same copy is
/// labeled in a human way ([`backup_row_label`]), and the person did not
/// recognize in the question the line they had just clicked. The timestamp is
/// made human by the same [`pretty_backup_ts`]: there must be no second date
/// table here.
pub fn plasma_restore_confirm(ts: &str) -> (String, String) {
    (
        t("plasma.confirm_title"),
        tf("plasma.confirm_text", &[&pretty_backup_ts(ts)]),
    )
}

/// The copy engine's answer as a status line: an icon by success + its message.
pub fn plasma_status(ok: bool, msg: &str) -> String {
    if ok {
        format!("✅ {msg}")
    } else {
        format!("⚠️ {msg}")
    }
}

// ── Other results ───────────────────────────────────────────────────

/// Status line after restoring the environment's native applications:
/// the icon depends on whether everything was found.
pub fn restore_native_status(msg: &str, has_missing: bool) -> String {
    if has_missing {
        format!("⚠️ {msg}")
    } else {
        format!("✅ {msg}")
    }
}

/// Completeness check result for the popup line: without checks, «ℹ️».
pub fn compliance_status(msg: &str, has_results: bool) -> String {
    if has_results {
        msg.to_string()
    } else {
        format!("ℹ️ {msg}")
    }
}

/// Message of the onboarding toggle.
pub fn onboarding_toggle_status(enabled: bool) -> String {
    if enabled {
        t("ob.on")
    } else {
        t("ob.off")
    }
}

/// Caption of the "take the onboarding again" button: taken from the onboarding itself.
pub fn onboarding_replay_button() -> String {
    format!("▶️ {}", crate::onboarding::settings_replay())
}

/// Message of the look check toggle.
pub fn aeroshell_toggle_status(enabled: bool) -> String {
    if enabled {
        t("aero.on")
    } else {
        t("aero.off")
    }
}

// ============================================================================
// Interface language
// ============================================================================
//
// The list and captions are here, the mechanism is in `i18n`: which languages
// exist (from `po/LINGUAS` and the catalogs found), what their speakers call
// them, which language a choice gives. The language changes after a restart,
// not on the fly; for why, see `i18n::restart_needed`.

/// Items of the «Language» list: (value of the `ui_language` setting, caption).
///
/// First "as in the system" with the language it gives now; then English and
/// the languages of the built catalogs, each under its own native name.
pub fn language_choices() -> Vec<(String, String)> {
    let system = crate::i18n::native_name(&crate::i18n::language_for_setting(None));
    let mut out = vec![(
        crate::i18n::SYSTEM_SETTING.to_string(),
        text("lang.system", &[system]).unwrap_or_default(),
    )];
    for code in crate::i18n::available_languages() {
        let label = crate::i18n::native_name(&code);
        out.push((code, label));
    }
    out
}

/// Whether the value is known to the list (there is no point saving an unknown
/// one: garbage in the setting would give English without explanation).
pub fn language_choice_valid(value: &str) -> bool {
    language_choices().iter().any(|(code, _)| code == value)
}

/// The answer to choosing a language: "saved: …". The caption is the native
/// name; for "as in the system", the same string as in the list.
pub fn language_saved_status(value: &str) -> String {
    let label = language_choices()
        .into_iter()
        .find(|(code, _)| code == value)
        .map(|(_, label)| label)
        .unwrap_or_else(|| value.to_string());
    tf("lang.saved", &[label.as_str()])
}

/// Refusal of the «Restart Rexon» button, with the reason.
pub fn language_restart_failed(reason: &str) -> String {
    tf("lang.restart_failed", &[reason])
}

/// Arguments of the new process when restarting for the language (`args`
/// without the program name).
///
/// The window opens again on «Settings»: the person was just there and should
/// see that the language changed. So the previous section (`--section=`,
/// `--page=`) and `--tray` are dropped (otherwise the window would go to the
/// tray, and the restart would look like closing), and the internal
/// `--selftest` is not repeated at all.
pub fn restart_args(args: &[String]) -> Vec<String> {
    let mut out: Vec<String> = args
        .iter()
        .filter(|a| {
            *a != crate::tray::START_HIDDEN_FLAG
                && *a != "--selftest"
                && !a.starts_with(crate::cli::SECTION_FLAG)
                && !a.starts_with(crate::app::PAGE_FLAG)
        })
        .cloned()
        .collect();
    out.push(format!("{}settings", crate::cli::SECTION_FLAG));
    out
}

// ============================================================================
// Section set: which of them exist on this machine at all
// ============================================================================

/// Observed facts about the machine that the tab's section set depends on.
///
/// The decision "show the section or not" lives HERE, not in the window body.
/// Before, it was smeared across building the sections (`if not available():
/// return`, `if self._de != "mate": return`, two Plasma checks), and a section
/// added to [`SECTIONS`] could silently fail to appear, or appear where it had
/// nothing to show. Rule #12 in its second sense: entities are enumerated in
/// one list, not by a set of conditions at the drawing site.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SectionFacts {
    /// Onboarding is available at all (the wheel with its API is built).
    pub onboarding_available: bool,
    /// Plasma is running, OR panel copies from it remain.
    pub plasma_available: bool,
    /// A Plasma look fork is installed in the system (AeroShell and kin).
    pub aeroshell_installed: bool,
}

impl SectionFacts {
    /// Facts from the answers of live probes, in the order the window collects them.
    pub fn new(
        onboarding_available: bool,
        plasma_available: bool,
        aeroshell_installed: bool,
    ) -> Self {
        Self {
            onboarding_available,
            plasma_available,
            aeroshell_installed,
        }
    }
}

/// Whether to show the section on this machine.
///
/// Sections not named here are always shown: they have no condition under
/// which they have nothing to show (inside they say "the wallpaper engine did
/// not start", "Pillow is not installed" and so on themselves: that is the
/// CONTENT of the section, not its absence).
pub fn section_visible(id: &str, facts: &SectionFacts) -> bool {
    match id {
        "obGroup" => facts.onboarding_available,
        "plasmaGroup" => facts.plasma_available,
        "aeroGroup" => facts.aeroshell_installed,
        _ => true,
    }
}

/// Sections the tab will show on this machine, in the order of [`SECTIONS`].
pub fn visible_sections(facts: &SectionFacts) -> Vec<&'static str> {
    section_order()
        .into_iter()
        .filter(|id| section_visible(id, facts))
        .collect()
}

// ============================================================================
// Status colors
// ============================================================================
//
// One color, one decision. QML takes it bare, the number itself lives here,
// and the recorded reference is held by the guard
// `qt_settings_parity::status_colors_are_frozen` (the layout and colors of the
// `styles` map by `layout_and_colors_are_frozen`). The former comparison with
// the QSS of the Python widget tab has been removed: since 02.10.2026 Python
// is only an archive.

/// "Everything is in place": an installed file manager, a successful check.
pub const COLOR_OK: &str = "#4ade80";

/// "Not installed / stop": the absence mark and the stop buttons.
pub const COLOR_BAD: &str = "#ef4444";

/// "Attention": the engine did not start, no Pillow, no mpv, import unavailable.
pub const COLOR_WARN: &str = "#f59e0b";

/// Fill of a red button («⏹️ Stop Live Wallpaper», «Stop all sounds») at rest
/// and under the cursor.
///
/// The numbers are carried over one to one from the widget tab's QSS
/// (`rgba(220,38,38,0.18)` → `rgba(220,38,38,0.32)`): the shade itself is set
/// by [`COLOR_BAD`], here only its opacity. The QML page paints with the same
/// values through the palette, so both tabs dim the button the same way.
pub const DANGER_FILL_ALPHA: f64 = 0.18;
pub const DANGER_FILL_HOVER_ALPHA: f64 = 0.32;

/// Color of the "installed / not installed" mark next to the file manager: the
/// pair to [`fm_badge`], which returns the mark itself.
pub fn fm_badge_color(installed: bool) -> &'static str {
    if installed {
        COLOR_OK
    } else {
        COLOR_BAD
    }
}

// ============================================================================
// Layout and styling not present in the skin palette
// ============================================================================
//
// Numbers and colors of the widget tab scattered over its layouts and
// stylesheets (`rexon/qt_settings/window.py`). The QML markup introduces none
// of its own: everything comes from here as the `styles` map
// (rust/rexon-gui/src/settings_page.rs), the same set the widget version is
// painted with. Numbers that match neighbouring tabs are taken from them by
// reference, not as a second copy (rule #12).

/// Page margins and the step between its blocks: the same as in «Extensions».
pub const PAGE_MARGIN: i32 = crate::qt_extensions::PAGE_MARGIN;
pub const PAGE_SPACING: i32 = crate::qt_extensions::PAGE_SPACING;

/// Section group: margins, row step and corner radius, shared with the Steam page.
pub const GROUP_MARGIN_H: i32 = crate::qt_steam::GROUP_MARGIN_H;
pub const GROUP_MARGIN_V: i32 = crate::qt_steam::GROUP_MARGIN_V;
pub const GROUP_SPACING: i32 = crate::qt_steam::GROUP_SPACING;
pub const GROUP_RADIUS: i32 = crate::qt_steam::GROUP_RADIUS;

/// The "background, border" pair of a section group.
///
/// A neutral gray with transparency, not white as on the Steam page: it reads
/// equally on a dark and a light skin, and the tab has no "is the theme dark"
/// fork of its own (`neutral_rgba`).
pub fn group_colors() -> (String, String) {
    (
        crate::ui_base::neutral_rgba(0.04),
        crate::ui_base::neutral_rgba(0.14),
    )
}

/// The "no polkit" warning strip: corner radius, padding around the text and
/// the "background, border" pair.
pub const POLKIT_BANNER_RADIUS: i32 = 8;
pub const POLKIT_BANNER_PADDING: i32 = 10;
pub fn polkit_banner_colors() -> (String, String) {
    (
        crate::ui_base::neutral_rgba(0.10),
        crate::ui_base::neutral_rgba(0.25),
    )
}

/// Blue action button («Apply», «Restore», «Blur»…): padding around the
/// caption and corner radius. The caption is bold.
pub const PRIMARY_BTN_PADDING_V: i32 = 6;
pub const PRIMARY_BTN_PADDING_H: i32 = 14;
pub const PRIMARY_BTN_RADIUS: i32 = crate::qt_steam::BTN_RADIUS;

/// Colors of the blue action button.
pub struct PrimaryButtonColors {
    pub background: String,
    pub hover: String,
    pub pressed: String,
    pub border: String,
    pub disabled_background: String,
    pub disabled_border: String,
    pub disabled_text: String,
}

/// `rgba(r,g,b,a)` of the product accent: the same notation as in widget QSS.
fn accent_rgba(alpha: &str) -> String {
    let (r, g, b) = crate::qt_steam::ACCENT_RGB;
    format!("rgba({r},{g},{b},{alpha})")
}

/// Blue action button: the same light blue as in the other tabs, but more
/// transparent (0.30 / 0.45 / 0.60) and with its own 0.45 border: «Compare»
/// has no border, Steam has a 0.85 fill. A disabled button is gray, not
/// pale: background, border and text are neutral 0.15 / 0.25 / 0.7.
pub fn primary_button_colors() -> PrimaryButtonColors {
    PrimaryButtonColors {
        background: accent_rgba("0.30"),
        hover: accent_rgba("0.45"),
        pressed: accent_rgba("0.60"),
        border: accent_rgba("0.45"),
        disabled_background: crate::ui_base::neutral_rgba(0.15),
        disabled_border: crate::ui_base::neutral_rgba(0.25),
        disabled_text: crate::ui_base::neutral_rgba(0.7),
    }
}

/// Red stop button («⏹️ Stop Live Wallpaper», «⏹️ Stop all»): padding around
/// the caption; the caption is [`COLOR_BAD`] in bold, the fill is
/// [`DANGER_FILL_ALPHA`] / [`DANGER_FILL_HOVER_ALPHA`].
pub const DANGER_BTN_PADDING_V: i32 = 6;
pub const DANGER_BTN_PADDING_H: i32 = 12;

/// The "background, text" pair of a DISABLED red button: gray, not pale red.
pub fn danger_button_disabled_colors() -> (String, String) {
    (
        crate::ui_base::neutral_rgba(0.12),
        crate::ui_base::neutral_rgba(0.6),
    )
}

/// File manager choice row: margins, step, corner radius and the focus strip
/// (a translucent light blue highlight under the focused radio button).
pub const FM_ROW_MARGIN_H: i32 = 8;
pub const FM_ROW_MARGIN_V: i32 = 4;
pub const FM_ROW_SPACING: i32 = 8;
pub const FM_ROW_RADIUS: i32 = 4;
pub fn fm_row_focus_color() -> String {
    accent_rgba("0.22")
}

/// Minimum widths of captions before controls, so that controls in adjacent
/// rows line up in one column: «Skin:» and «Sound theme:», the wallpaper
/// volume caption, «URL:», «Strength:», the blur strength value, the «NN %» value.
pub const FIELD_LABEL_MIN_WIDTH: i32 = 150;
pub const VOLUME_LABEL_MIN_WIDTH: i32 = 190;
pub const URL_LABEL_MIN_WIDTH: i32 = 60;
pub const BLUR_LABEL_MIN_WIDTH: i32 = 60;
pub const BLUR_VALUE_MIN_WIDTH: i32 = 48;
pub const AMBIENT_PERCENT_MIN_WIDTH: i32 = 42;

/// Maximum widths of the volume sliders: wallpaper and background sounds.
pub const VOLUME_SLIDER_MAX_WIDTH: i32 = 200;
pub const AMBIENT_SLIDER_MAX_WIDTH: i32 = 160;

/// List of playing background sounds: top margin, row step; a row has
/// margins, step, corner radius, the width of the «🔊» and «⏹» buttons.
pub const AMBIENT_LIST_TOP_MARGIN: i32 = 4;
pub const AMBIENT_LIST_SPACING: i32 = 4;
pub const AMBIENT_ROW_MARGIN_H: i32 = 8;
pub const AMBIENT_ROW_MARGIN_V: i32 = 4;
pub const AMBIENT_ROW_SPACING: i32 = 8;
pub const AMBIENT_ROW_RADIUS: i32 = 4;
pub const AMBIENT_ROW_BUTTON_WIDTH: i32 = 40;

/// Background of a playing sound row.
///
/// Fix to the original (rule #16): in the widgets the row was painted white
/// with transparency under ANY theme, invisible on a light one. The light
/// theme gets black of the same transparency.
pub fn ambient_row_color(dark: bool) -> String {
    if dark {
        "rgba(255,255,255,0.05)".to_string()
    } else {
        "rgba(0,0,0,0.05)".to_string()
    }
}

/// The «⏹» button in a sound row: the same red as the remove buttons of
/// «Extensions» (220,80,80), but its own fill 0.25 / 0.4 and padding 4 / 6.
pub const AMBIENT_STOP_COLOR: &str = crate::qt_extensions::DANGER_COLOR;
pub const AMBIENT_STOP_FILL_ALPHA: f64 = 0.25;
pub const AMBIENT_STOP_FILL_HOVER_ALPHA: f64 = 0.4;
pub const AMBIENT_STOP_PADDING_V: i32 = 4;
pub const AMBIENT_STOP_PADDING_H: i32 = 6;

/// Wallpaper library: grid step and its top margin; a tile has corner radius,
/// padding and step of the caption block, button gap, width of the «▶»
/// button; font size of the placeholder icon; padding around the "empty" label.
pub const LIB_GRID_SPACING: i32 = 8;
pub const LIB_GRID_TOP_MARGIN: i32 = 4;
pub const LIB_CARD_RADIUS: i32 = 6;
pub const LIB_INFO_MARGIN_H: i32 = 10;
pub const LIB_INFO_MARGIN_V: i32 = 8;
pub const LIB_INFO_SPACING: i32 = 2;
pub const LIB_BUTTON_SPACING: i32 = 4;
pub const LIB_PREVIEW_BUTTON_WIDTH: i32 = 36;
pub const LIB_PLACEHOLDER_POINT_SIZE: i32 = 26;
pub const LIB_EMPTY_PADDING: i32 = 8;

/// The "border, hover border" pair of a library tile: the same 0.12 / 0.30 as
/// the tiles of «Extensions» and Steam cards; the tile has no background.
pub fn lib_card_colors(dark: bool) -> (String, String) {
    let (_bg, border, hover) = crate::qt_extensions::card_colors(dark);
    (border, hover)
}

/// Background of the placeholder in place of a tile preview (the same fill as the sound row).
pub fn lib_placeholder_color(dark: bool) -> String {
    ambient_row_color(dark)
}

/// The row of sound theme "listen" buttons, the row and column of font
/// buttons, the Plasma panel copy row; the "heading + caption" block before
/// a button («Remember current choice», «Save current panel layout») has no step.
pub const SOUND_PREVIEW_SPACING: i32 = 6;
pub const FONT_ROW_SPACING: i32 = 6;
pub const FONT_COL_SPACING: i32 = 2;
pub const PLASMA_ROW_SPACING: i32 = 6;
pub const INFO_BOX_SPACING: i32 = 0;
