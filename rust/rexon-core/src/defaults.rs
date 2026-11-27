//! Protection of default applications when the DE changes, a port of
//! `rexon/defaults.py` (the `SystemDefaultsProtector` class).
//!
//! The module saves/restores the "default applications" (file manager,
//! text editor, browser, mail, viewers, players, archiver, terminal) when a
//! neighbouring DE is installed: a snapshot of the current defaults through
//! `xdg-mime`, restoring the old choice after installation + a question to
//! the user "a new application appeared, switch to it?".
//!
//! Port principle (like `system_plan.rs` / `installed_scan.rs`): all the
//! PURE logic is deterministic input->output functions; side effects (a
//! real `xdg-mime`/`gsettings`/`kwriteconfig6` subprocess, reading/writing
//! `~/.config`) are NOT executed here. They are expressed either as PLANS
//! of commands/writes ([`TermStep`], [`SetDefault`], [`ProtectDeOutcome`])
//! or arrive as ready values/callbacks (file contents,
//! `desktop_file_exists`, `xdg-mime`/`gsettings` output). Live execution
//! stays on the Python side / in a future bridge.
//!
//! BYTE-FOR-BYTE parity with Python: the same serialisation of
//! snapshot/rules/pending (`json.dump(indent=2, ensure_ascii=False)`
//! matches `serde_json::to_string_pretty` with the `preserve_order`
//! feature), the same command argv, the same generation of
//! `*-mimeapps.list`, the same parsing of
//! `xdg-terminals.list`/`kdeglobals`/`mimeapps.list`. The golden files were
//! taken from live Python read-only (`tests/golden/defaults_cases.json`).
//!
//! Quirks of the original kept (no deliberate divergence introduced, for
//! parity):
//!   * `TerminalService` in `kdeglobals` is returned "as is", while
//!     `TerminalApplication` gets the `.desktop` suffix appended: the
//!     original's asymmetry is kept;
//!   * `binary = terminal_desktop.replace(".desktop", "")` erases ALL
//!     occurrences of the `.desktop` substring, then the tail after the
//!     last dot is taken: kept;
//!   * `action = "updated" if existing else "created"` in
//!     `protect_de_mimeapps` looks at whether the parsed dictionary is
//!     NON-EMPTY, not at whether the file exists: kept.
//!
//! NOT ported (execution happens when connected to live Python):
//!   real `subprocess.run`/`safe_run` (`xdg-mime query/default`,
//!   `gsettings`, `kwriteconfig6`), real reading/writing of the files
//!   `~/.config/rexon/*.json`, `~/.config/xdg-terminals.list`,
//!   `~/.config/kdeglobals`, `~/.config/environment.d/rexon-terminal.conf`,
//!   `~/.config/*-mimeapps.list`, `os.makedirs`, logging.

use crate::i18n::{n_, tr};
use serde::Serialize;
use serde_json::{Map, Value};

// ============================================================================
// Constant tables (order = as in the Python dict, matters for serialisation)
// ============================================================================

/// `SystemDefaultsProtector.MIME_TYPES`: application type -> MIME.
pub const MIME_TYPES: &[(&str, &str)] = &[
    ("file_manager", "inode/directory"),
    ("text_editor", "text/plain"),
    ("browser", "x-scheme-handler/http"),
    ("email", "x-scheme-handler/mailto"),
    ("image_viewer", "image/jpeg"),
    ("video_player", "video/mp4"),
    ("pdf_viewer", "application/pdf"),
    ("archive_manager", "application/zip"),
    ("music_player", "audio/mpeg"),
];

/// MIME types that belong to the web browser. Rexon NEVER writes them into a
/// `<desktop>-mimeapps.list`: the browser is the person's choice and the same
/// in every desktop. A live case (owner's machine, 2026): the KDE table carried
/// `text/html=org.kde.kate.desktop`, and every web page saved to disk or opened
/// from another program went to the text editor instead of Firefox. Lines with
/// these types that are already in a file are left exactly as they are.
pub const BROWSER_MIMES: &[&str] = &[
    "text/html",
    "application/xhtml+xml",
    "x-scheme-handler/http",
    "x-scheme-handler/https",
    "x-scheme-handler/about",
    "x-scheme-handler/unknown",
];

/// Whether `mime` belongs to the browser ([`BROWSER_MIMES`]).
pub fn is_browser_mime(mime: &str) -> bool {
    BROWSER_MIMES.contains(&mime.trim())
}

/// MIME for an application type (`MIME_TYPES.get`).
pub fn mime_for(app_type: &str) -> Option<&'static str> {
    MIME_TYPES.iter().find(|(k, _)| *k == app_type).map(|(_, v)| *v)
}

/// Human names of application types: what the customer reads in
/// "Settings". The terminal comes first: it is the only one not configured
/// through MIME, and the interface names it the same way as the rest.
/// The labels are `msgid`s, translated by [`app_type_label`].
pub const APP_TYPE_LABELS: &[(&str, &str)] = &[
    ("terminal", n_("🖥️ Terminal")),
    ("file_manager", n_("📁 File Manager")),
    ("text_editor", n_("📝 Text Editor")),
    ("browser", n_("🌐 Web Browser")),
    ("email", n_("📧 Email Client")),
    ("image_viewer", n_("🖼️ Image Viewer")),
    ("video_player", n_("🎬 Video Player")),
    ("pdf_viewer", n_("📄 PDF Viewer")),
    ("archive_manager", n_("📦 Archive Manager")),
    ("music_player", n_("🎵 Music Player")),
];

/// Human name of an application type. An unknown type is returned as is,
/// just as `APP_TYPE_LABELS.get(key, key)` did.
pub fn app_type_label(app_type: &str) -> String {
    APP_TYPE_LABELS
        .iter()
        .find(|(k, _)| *k == app_type)
        .map(|(_, v)| tr(v))
        .unwrap_or_else(|| app_type.to_string())
}

/// Default rules key order + values, a port of `self.rules`.
pub const DEFAULT_RULES: &[(&str, bool)] = &[
    ("enabled", true),
    ("terminal", true),
    ("file_manager", true),
    ("text_editor", true),
    ("browser", false), // the browser is left alone by default
    ("email", false),   // email is left alone
    ("image_viewer", true),
    ("video_player", true),
    ("pdf_viewer", true),
    ("archive_manager", true),
    ("music_player", true),
];

/// The `*-mimeapps.list` file name of a desktop. ONE naming table for the whole
/// product: [`crate::appdefaults::resolve_mimeapps_filename`] (the first element
/// of the desktop's `XDG_CURRENT_DESKTOP` in lowercase, as GLib and `xdg-mime`
/// look it up). The former separate table here named Cinnamon `cinnamon-…`,
/// while `XDG_CURRENT_DESKTOP=X-Cinnamon` makes everyone read
/// `x-cinnamon-mimeapps.list`: the list written at desktop install was never read.
fn mimeapps_filename_for(de_id: &str) -> String {
    crate::appdefaults::resolve_mimeapps_filename(de_id)
}

/// Rexon's own per-desktop files of `de_id` in `$XDG_CONFIG_HOME` (file
/// names): its `<desktop>-mimeapps.list` and `<desktop>-xdg-terminals.list`.
/// Removed after the desktop itself is removed — only those carrying a Rexon
/// mark ([`is_rexon_desktop_file`]).
pub fn desktop_owned_files(de_id: &str) -> Vec<String> {
    let mut out = vec![mimeapps_filename_for(de_id)];
    if let Some(t) = crate::appdefaults::terminals_list_filename(de_id) {
        out.push(t);
    }
    out
}

/// Whether a per-desktop file is Rexon's: the mimeapps marker (or the old
/// header) or one of the terminal list marks.
pub fn is_rexon_desktop_file(text: &str) -> bool {
    is_rexon_mimeapps(text)
        || text
            .lines()
            .any(|l| l.trim() == TERMINALS_LIST_CHOSEN_MARK || l.trim() == TERMINALS_LIST_NATIVE_MARK)
}

/// Lists older versions wrote under a name no desktop reads: (wrong, right).
pub const MISNAMED_MIMEAPPS: &[(&str, &str)] =
    &[("cinnamon-mimeapps.list", "x-cinnamon-mimeapps.list")];

/// What to do with a list written under a wrong name ([`MISNAMED_MIMEAPPS`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MisnamedFix {
    /// Nothing to do: no old file, or it is not Rexon's (the person's own file
    /// is never touched).
    Nothing,
    /// The right file does not exist: write this text there, remove the old one.
    Move { content: String },
    /// The right file already exists: the old one is unread junk, remove it.
    DropOld,
}

/// The decision for one misnamed list. `old`/`right` are the texts of the
/// wrong-name and right-name files (`None` — no file).
pub fn misnamed_mimeapps_fix(old: Option<&str>, right: Option<&str>) -> MisnamedFix {
    match old {
        Some(text) if is_rexon_mimeapps(text) => match right {
            None => MisnamedFix::Move { content: text.to_string() },
            Some(_) => MisnamedFix::DropOld,
        },
        _ => MisnamedFix::Nothing,
    }
}

/// Native applications of each DE, a port of `DE_NATIVE_APPS`. The order of
/// pairs within a DE matters (the order valid_entries are built in before
/// sorting on write).
pub const DE_NATIVE_APPS: &[(&str, &[(&str, &str)])] = &[
    (
        "kde",
        &[
            ("inode/directory", "org.kde.dolphin.desktop"),
            ("text/plain", "org.kde.kate.desktop"),
            ("application/pdf", "okularApplication_pdf.desktop"),
            ("image/jpeg", "org.kde.gwenview.desktop"),
            ("image/png", "org.kde.gwenview.desktop"),
            ("image/gif", "org.kde.gwenview.desktop"),
            ("image/webp", "org.kde.gwenview.desktop"),
            ("image/svg+xml", "org.kde.gwenview.desktop"),
            ("image/avif", "org.kde.gwenview.desktop"),
            ("image/heif", "org.kde.gwenview.desktop"),
            ("image/tiff", "org.kde.gwenview.desktop"),
            ("image/bmp", "org.kde.gwenview.desktop"),
            ("image/jxl", "org.kde.gwenview.desktop"),
            ("video/mp4", "org.kde.haruna.desktop"),
            ("video/x-matroska", "org.kde.haruna.desktop"),
            ("video/webm", "org.kde.haruna.desktop"),
            ("video/mpeg", "org.kde.haruna.desktop"),
            ("video/quicktime", "org.kde.haruna.desktop"),
            ("video/x-msvideo", "org.kde.haruna.desktop"),
            ("audio/mpeg", "org.kde.elisa.desktop"),
            ("audio/x-flac", "org.kde.elisa.desktop"),
            ("audio/flac", "org.kde.elisa.desktop"),
            ("audio/ogg", "org.kde.elisa.desktop"),
            ("audio/x-wav", "org.kde.elisa.desktop"),
            ("audio/aac", "org.kde.elisa.desktop"),
            ("audio/mp4", "org.kde.elisa.desktop"),
            ("application/zip", "org.kde.ark.desktop"),
            ("application/gzip", "org.kde.ark.desktop"),
            ("application/x-7z-compressed", "org.kde.ark.desktop"),
            ("application/vnd.rar", "org.kde.ark.desktop"),
            ("application/x-tar", "org.kde.ark.desktop"),
            ("application/x-compressed-tar", "org.kde.ark.desktop"),
            ("application/x-bzip2-compressed-tar", "org.kde.ark.desktop"),
            ("application/x-xz-compressed-tar", "org.kde.ark.desktop"),
            ("application/epub+zip", "org.kde.okular.desktop"),
            ("image/vnd.djvu", "org.kde.okular.desktop"),
            ("application/oxps", "org.kde.okular.desktop"),
            ("text/markdown", "org.kde.kate.desktop"),
            ("application/json", "org.kde.kate.desktop"),
            ("application/x-yaml", "org.kde.kate.desktop"),
            ("text/xml", "org.kde.kate.desktop"),
            ("text/calendar", "org.kde.merkuro.calendar.desktop"),
            ("text/vcard", "org.kde.kaddressbook.desktop"),
            ("x-scheme-handler/mailto", "org.kde.kmail2.desktop"),
        ],
    ),
    (
        "gnome",
        &[
            ("inode/directory", "org.gnome.Nautilus.desktop"),
            ("text/plain", "org.gnome.TextEditor.desktop"),
            ("application/pdf", "org.gnome.Papers.desktop"),
            ("image/jpeg", "org.gnome.Loupe.desktop"),
            ("image/png", "org.gnome.Loupe.desktop"),
            ("image/gif", "org.gnome.Loupe.desktop"),
            ("image/webp", "org.gnome.Loupe.desktop"),
            ("image/svg+xml", "org.gnome.Loupe.desktop"),
            ("image/avif", "org.gnome.Loupe.desktop"),
            ("image/heif", "org.gnome.Loupe.desktop"),
            ("image/heic", "org.gnome.Loupe.desktop"),
            ("image/tiff", "org.gnome.Loupe.desktop"),
            ("image/bmp", "org.gnome.Loupe.desktop"),
            ("image/jxl", "org.gnome.Loupe.desktop"),
            ("video/mp4", "org.gnome.Showtime.desktop"),
            ("video/x-matroska", "org.gnome.Showtime.desktop"),
            ("video/webm", "org.gnome.Showtime.desktop"),
            ("video/mpeg", "org.gnome.Showtime.desktop"),
            ("video/ogg", "org.gnome.Showtime.desktop"),
            ("audio/mpeg", "org.gnome.Decibels.desktop"),
            ("audio/flac", "org.gnome.Decibels.desktop"),
            ("audio/ogg", "org.gnome.Decibels.desktop"),
            ("audio/mp4", "org.gnome.Decibels.desktop"),
            ("audio/x-wav", "org.gnome.Decibels.desktop"),
            ("application/zip", "org.gnome.FileRoller.desktop"),
            ("application/gzip", "org.gnome.FileRoller.desktop"),
            ("application/x-7z-compressed", "org.gnome.FileRoller.desktop"),
            ("application/x-rar", "org.gnome.FileRoller.desktop"),
            ("application/x-tar", "org.gnome.FileRoller.desktop"),
            ("application/x-compressed-tar", "org.gnome.FileRoller.desktop"),
            ("application/epub+zip", "org.gnome.Papers.desktop"),
            ("image/vnd.djvu", "org.gnome.Papers.desktop"),
            ("text/calendar", "org.gnome.Calendar.desktop"),
            ("x-scheme-handler/webcal", "org.gnome.Calendar.desktop"),
            ("text/vcard", "org.gnome.Contacts.desktop"),
            ("font/ttf", "org.gnome.font-viewer.desktop"),
            ("font/otf", "org.gnome.font-viewer.desktop"),
        ],
    ),
    (
        "cinnamon-mint",
        &[
            ("inode/directory", "nemo.desktop"),
            ("text/plain", "xed.desktop"),
            ("application/pdf", "xreader.desktop"),
            ("image/jpeg", "xviewer.desktop"),
            ("image/png", "xviewer.desktop"),
            ("image/gif", "xviewer.desktop"),
            ("image/webp", "xviewer.desktop"),
            ("image/svg+xml", "xviewer.desktop"),
            ("video/mp4", "xplayer.desktop"),
            ("video/x-matroska", "xplayer.desktop"),
            ("video/webm", "xplayer.desktop"),
            ("application/epub+zip", "xreader.desktop"),
            ("image/vnd.djvu", "xreader.desktop"),
        ],
    ),
    (
        "xfce",
        &[
            ("inode/directory", "thunar.desktop"),
            ("text/plain", "org.xfce.mousepad.desktop"),
            ("image/jpeg", "org.xfce.ristretto.desktop"),
            ("image/png", "org.xfce.ristretto.desktop"),
            ("image/gif", "org.xfce.ristretto.desktop"),
            ("image/webp", "org.xfce.ristretto.desktop"),
            ("image/svg+xml", "org.xfce.ristretto.desktop"),
            ("image/avif", "org.xfce.ristretto.desktop"),
            ("image/tiff", "org.xfce.ristretto.desktop"),
            ("video/mp4", "org.xfce.Parole.desktop"),
            ("video/x-matroska", "org.xfce.Parole.desktop"),
            ("video/webm", "org.xfce.Parole.desktop"),
            ("audio/mpeg", "org.xfce.Parole.desktop"),
            ("audio/flac", "org.xfce.Parole.desktop"),
            ("audio/ogg", "org.xfce.Parole.desktop"),
            ("application/json", "org.xfce.mousepad.desktop"),
            ("text/xml", "org.xfce.mousepad.desktop"),
            ("text/markdown", "org.xfce.mousepad.desktop"),
        ],
    ),
    (
        "mate",
        &[
            ("inode/directory", "caja-folder-handler.desktop"),
            ("text/plain", "pluma.desktop"),
            ("application/pdf", "atril.desktop"),
            ("image/jpeg", "eom.desktop"),
            ("image/png", "eom.desktop"),
            ("image/gif", "eom.desktop"),
            ("image/bmp", "eom.desktop"),
            ("image/tiff", "eom.desktop"),
            ("image/svg+xml", "eom.desktop"),
            ("application/zip", "engrampa.desktop"),
            ("application/gzip", "engrampa.desktop"),
            ("application/x-7z-compressed", "engrampa.desktop"),
            ("application/x-rar", "engrampa.desktop"),
            ("application/x-tar", "engrampa.desktop"),
            ("application/x-compressed-tar", "engrampa.desktop"),
            ("application/epub+zip", "atril.desktop"),
            ("image/vnd.djvu", "atril.desktop"),
        ],
    ),
    (
        "cosmic",
        &[
            ("inode/directory", "com.system76.CosmicFiles.desktop"),
            ("text/plain", "com.system76.CosmicEdit.desktop"),
            ("video/mp4", "com.system76.CosmicPlayer.desktop"),
            ("video/x-matroska", "com.system76.CosmicPlayer.desktop"),
            ("video/webm", "com.system76.CosmicPlayer.desktop"),
            ("audio/mpeg", "com.system76.CosmicPlayer.desktop"),
            ("audio/flac", "com.system76.CosmicPlayer.desktop"),
        ],
    ),
    // Budgie: the programs of Ubuntu Budgie 26.04 per its
    // `/etc/budgie-desktop/defaults.list` (installed by the `budgie` set in
    // `system::DE_DATABASE`). Music is Lollypop throughout, as in
    // `appdefaults` (Ubuntu Budgie opens MP3/M4A/WAV with VLC).
    (
        "budgie",
        &[
            ("inode/directory", "nemo.desktop"),
            ("text/plain", "org.gnome.gedit.desktop"),
            ("application/pdf", "atril.desktop"),
            ("image/jpeg", "org.gnome.gThumb.desktop"),
            ("image/png", "org.gnome.gThumb.desktop"),
            ("image/gif", "org.gnome.gThumb.desktop"),
            ("image/svg+xml", "org.gnome.gThumb.desktop"),
            ("image/heif", "org.gnome.gThumb.desktop"),
            ("image/tiff", "org.gnome.gThumb.desktop"),
            ("image/bmp", "org.gnome.gThumb.desktop"),
            ("image/jxl", "org.gnome.gThumb.desktop"),
            ("video/mp4", "vlc.desktop"),
            ("video/x-matroska", "vlc.desktop"),
            ("video/webm", "vlc.desktop"),
            ("video/mpeg", "vlc.desktop"),
            ("video/ogg", "vlc.desktop"),
            ("video/quicktime", "vlc.desktop"),
            ("audio/mpeg", "org.gnome.Lollypop.desktop"),
            ("audio/flac", "org.gnome.Lollypop.desktop"),
            ("audio/ogg", "org.gnome.Lollypop.desktop"),
            ("audio/mp4", "org.gnome.Lollypop.desktop"),
            ("audio/x-wav", "org.gnome.Lollypop.desktop"),
            ("application/zip", "org.gnome.FileRoller.desktop"),
            ("application/gzip", "org.gnome.FileRoller.desktop"),
            ("application/x-7z-compressed", "org.gnome.FileRoller.desktop"),
            ("application/x-rar", "org.gnome.FileRoller.desktop"),
            ("application/x-tar", "org.gnome.FileRoller.desktop"),
            ("application/x-compressed-tar", "org.gnome.FileRoller.desktop"),
            ("image/vnd.djvu", "atril.desktop"),
            ("text/xml", "org.gnome.gedit.desktop"),
        ],
    ),
    (
        "pantheon",
        &[
            ("inode/directory", "io.elementary.files.desktop"),
            ("text/plain", "io.elementary.code.desktop"),
            ("text/markdown", "io.elementary.code.desktop"),
            ("application/json", "io.elementary.code.desktop"),
            ("image/jpeg", "io.elementary.photos.viewer.desktop"),
            ("image/png", "io.elementary.photos.viewer.desktop"),
            ("image/gif", "io.elementary.photos.viewer.desktop"),
            ("image/webp", "io.elementary.photos.viewer.desktop"),
            ("image/tiff", "io.elementary.photos.viewer.desktop"),
            ("video/mp4", "io.elementary.videos.desktop"),
            ("video/x-matroska", "io.elementary.videos.desktop"),
            ("video/webm", "io.elementary.videos.desktop"),
            ("audio/mpeg", "io.elementary.music.desktop"),
            ("audio/flac", "io.elementary.music.desktop"),
            ("audio/ogg", "io.elementary.music.desktop"),
        ],
    ),
    // Deepin: programs of the `deepin` / `deepin-extra` groups (`.desktop`
    // names checked against their linuxdeepin repositories).
    (
        "deepin",
        &[
            ("inode/directory", "dde-file-manager.desktop"),
            ("text/plain", "deepin-editor.desktop"),
            ("text/markdown", "deepin-editor.desktop"),
            ("application/json", "deepin-editor.desktop"),
            ("image/jpeg", "deepin-image-viewer.desktop"),
            ("image/png", "deepin-image-viewer.desktop"),
            ("image/gif", "deepin-image-viewer.desktop"),
            ("image/webp", "deepin-image-viewer.desktop"),
            ("image/tiff", "deepin-image-viewer.desktop"),
            ("video/mp4", "deepin-movie.desktop"),
            ("video/x-matroska", "deepin-movie.desktop"),
            ("video/webm", "deepin-movie.desktop"),
            ("application/zip", "deepin-compressor.desktop"),
            ("application/x-7z-compressed", "deepin-compressor.desktop"),
        ],
    ),
    ("hyprland", &[]), // the user chooses themselves
];

fn native_apps_for(de_id: &str) -> Option<&'static [(&'static str, &'static str)]> {
    DE_NATIVE_APPS.iter().find(|(k, _)| *k == de_id).map(|(_, v)| *v)
}

// ============================================================================
// Rules: an ordered bool dictionary (Python dict semantics)
// ============================================================================

/// Protection rules: ordered pairs (like a Python dict). `get` gives None
/// if the key is missing; callers substitute the default (`enabled`->true,
/// others->false).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules(pub Vec<(String, bool)>);

impl Rules {
    /// Default rules, a port of the `self.rules` initialisation.
    pub fn defaults() -> Rules {
        Rules(DEFAULT_RULES.iter().map(|(k, v)| (k.to_string(), *v)).collect())
    }

    /// `self.rules.get(key)`.
    pub fn get(&self, key: &str) -> Option<bool> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }

    /// `self.rules.get("enabled", True)`.
    pub fn enabled(&self) -> bool {
        self.get("enabled").unwrap_or(true)
    }

    /// `self.rules.get(app_type, False)`.
    pub fn protect(&self, app_type: &str) -> bool {
        self.get(app_type).unwrap_or(false)
    }

    /// `self.rules.update(loaded)`: replace by position + append new keys at
    /// the end (a port of `_load_rules`). `loaded` is a JSON object of bool
    /// values (rules are always bool).
    pub fn updated(&self, loaded: &Map<String, Value>) -> Rules {
        let mut out = self.0.clone();
        for (k, v) in loaded {
            let b = v.as_bool().unwrap_or(false);
            if let Some(e) = out.iter_mut().find(|(ek, _)| ek == k) {
                e.1 = b;
            } else {
                out.push((k.clone(), b));
            }
        }
        Rules(out)
    }

    /// `json.dumps(self.rules, indent=2, ensure_ascii=False)`.
    pub fn serialize(&self) -> String {
        let mut m = Map::new();
        for (k, v) in &self.0 {
            m.insert(k.clone(), Value::Bool(*v));
        }
        to_pretty(&Value::Object(m))
    }
}

/// `json.dumps(obj, indent=2, ensure_ascii=False)` (Cyrillic is not
/// escaped, 2-space indent; matches with the `preserve_order` feature).
fn to_pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).expect("Value сериализуется")
}

// ============================================================================
// Snapshot / pending: serialisation and loading
// ============================================================================

/// `_save_snapshot`: `{"protected_apps": {...}, "last_updated": ts}` ->
/// json.dump(indent=2, ensure_ascii=False). `last_updated` (isoformat) is
/// non-deterministic and is passed as a parameter.
pub fn serialize_snapshot(
    protected_apps: &[(String, Option<String>)],
    last_updated: &str,
) -> String {
    let mut inner = Map::new();
    for (k, v) in protected_apps {
        inner.insert(k.clone(), opt_to_value(v));
    }
    let mut outer = Map::new();
    outer.insert("protected_apps".to_string(), Value::Object(inner));
    outer.insert("last_updated".to_string(), Value::String(last_updated.to_string()));
    to_pretty(&Value::Object(outer))
}

/// `_save_pending`: `{app_type: {"old":.., "new":..}}` → json.dump(indent=2).
pub fn serialize_pending(pending: &[(String, (String, String))]) -> String {
    let mut m = Map::new();
    for (app_type, (old, new)) in pending {
        let mut choice = Map::new();
        choice.insert("old".to_string(), Value::String(old.clone()));
        choice.insert("new".to_string(), Value::String(new.clone()));
        m.insert(app_type.clone(), Value::Object(choice));
    }
    to_pretty(&Value::Object(m))
}

fn opt_to_value(v: &Option<String>) -> Value {
    match v {
        Some(s) => Value::String(s.clone()),
        None => Value::Null,
    }
}

/// `_load_snapshot`: extract `protected_apps` from the parsed JSON (key
/// order as in the file). Missing -> empty.
pub fn load_protected_apps(parsed: &Value) -> Vec<(String, Option<String>)> {
    parsed
        .get("protected_apps")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().map(String::from)))
                .collect()
        })
        .unwrap_or_default()
}

// ============================================================================
// Detecting the current terminal: a port of _get_current_terminal
// ============================================================================

/// `val + ".desktop"` if it does not end with `.desktop` yet (otherwise as is).
fn maybe_desktop(val: &str) -> String {
    if val.ends_with(".desktop") {
        val.to_string()
    } else {
        format!("{val}.desktop")
    }
}

/// Which gsettings schema to read the current terminal from (the `exec`
/// key), by the raw `XDG_CURRENT_DESKTOP`; `None` means the environment
/// keeps it outside gsettings (KDE, XFCE…) or has no place for it at all.
///
/// One decision for the product (rule #12): the same chain "gnome |
/// cinnamon | budgie -> the GNOME schema, mate -> the MATE schema" used to
/// live in three copies: here, in the desktop switch executor and in the
/// "Settings" tab. Cinnamon is read from the GNOME schema on purpose: the
/// common layer of [`set_terminal_plan`] always writes it.
pub fn terminal_exec_schema(desktop: &str) -> Option<&'static str> {
    let de = desktop.to_lowercase();
    if de.contains("kde") || de.contains("plasma") {
        return None;
    }
    if de.split(':').any(|t| t == "dde" || t == "deepin") {
        Some(DEEPIN_TERMINAL_SCHEMA)
    } else if ["gnome", "cinnamon", "budgie", "pantheon"].iter().any(|x| de.contains(x)) {
        Some("org.gnome.desktop.default-applications.terminal")
    } else if de.contains("mate") {
        Some("org.mate.applications-terminal")
    } else {
        None
    }
}

/// Port of `_get_current_terminal`. All side sources are passed as values:
///   * `xdg_terminals_list`: contents of `~/.config/xdg-terminals.list`;
///   * `desktop`: the raw `XDG_CURRENT_DESKTOP` (lowercased inside);
///   * `kdeglobals`: contents of `~/.config/kdeglobals` (KDE branch only);
///   * `gsettings_exec`: stdout of the matching `gsettings get ... exec`
///     (the schema is chosen by [`terminal_exec_schema`]).
pub fn get_current_terminal(
    xdg_terminals_list: Option<&str>,
    desktop: &str,
    kdeglobals: Option<&str>,
    gsettings_exec: Option<&str>,
) -> Option<String> {
    // 1. xdg-terminals.list: the freedesktop standard, independent of the DE.
    if let Some(content) = xdg_terminals_list {
        let first = content.lines().next().unwrap_or("").trim();
        if !first.is_empty() {
            return Some(first.to_string());
        }
    }

    let de = desktop.to_lowercase();

    // 2. KDE: kdeglobals.
    if de.contains("kde") || de.contains("plasma") {
        if let Some(kd) = kdeglobals {
            for line in kd.lines() {
                let s = line.trim();
                if let Some(rest) = s.strip_prefix("TerminalService=") {
                    let val = rest.trim();
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
                if let Some(rest) = s.strip_prefix("TerminalApplication=") {
                    let val = rest.trim();
                    if !val.is_empty() {
                        return Some(maybe_desktop(val));
                    }
                }
            }
        }
        return Some("org.kde.konsole.desktop".to_string());
    }

    // 3-4. GNOME / Cinnamon / Budgie / Pantheon use the GNOME schema, MATE its
    // own: which one to read is decided by [`terminal_exec_schema`], and its
    // answer arrives here.
    if terminal_exec_schema(desktop).is_some() {
        if let Some(out) = gsettings_exec {
            let val = out.trim().trim_matches(|c| c == '\'' || c == '"');
            if !val.is_empty() {
                return Some(maybe_desktop(val));
            }
        }
    }

    // 5. XFCE.
    if de.contains("xfce") {
        return Some("xfce4-terminal.desktop".to_string());
    }

    None
}

// ============================================================================
// Terminal list for one's own desktop: `<desktop>-xdg-terminals.list`
// ============================================================================
//
// `xdg-terminal-exec` reads the lists in order: first
// `~/.config/<desktop>-xdg-terminals.list` (desktop is each element of
// `XDG_CURRENT_DESKTOP` in lower case), and only then the common
// `~/.config/xdg-terminals.list`. The own desktop's list OVERRIDES the
// common one. A caught case (08.11.2026, the owner's live machine): in
// MATE a program with `Terminal=true` opened in GNOME Terminal: a foreign
// `mate-xdg-terminals.list` named it, while the common list held konsole,
// written once from Plasma. So Rexon maintains the own desktop's list too:
// a deliberate choice in the "Settings" tab is written there with the
// [`TERMINALS_LIST_CHOSEN_MARK`] mark, and at login a desktop with a
// native terminal gets it with the [`TERMINALS_LIST_NATIVE_MARK`] mark,
// unless the person chose a terminal for this desktop themselves.

/// Mark of the own desktop's list: the terminal was chosen by the person in
/// Rexon. Logging in does not rewrite such a list. A comment line:
/// `xdg-terminal-exec` skips lines with `#`.
pub const TERMINALS_LIST_CHOSEN_MARK: &str = "# Rexon: chosen by the user";

/// Mark of the own desktop's list: the desktop's native terminal, written at login.
pub const TERMINALS_LIST_NATIVE_MARK: &str = "# Rexon: native terminal of this desktop";

/// Text of the own desktop's list. The terminal is on the FIRST line: that
/// is what [`get_current_terminal`] reads.
pub fn desktop_terminals_list_text(terminal_desktop: &str, chosen: bool) -> String {
    let mark = if chosen { TERMINALS_LIST_CHOSEN_MARK } else { TERMINALS_LIST_NATIVE_MARK };
    format!("{terminal_desktop}\n{mark}\n")
}

/// The terminal chosen by the person for this desktop (a list with the
/// [`TERMINALS_LIST_CHOSEN_MARK`] mark); `None` means no choice or a
/// foreign list.
pub fn chosen_terminal_in_list(content: &str) -> Option<String> {
    if !content.lines().any(|l| l.trim() == TERMINALS_LIST_CHOSEN_MARK) {
        return None;
    }
    let first = content.lines().next().unwrap_or("").trim();
    (!first.is_empty() && !first.starts_with('#')).then(|| first.to_string())
}

/// Which list to read as "the current terminal": the own desktop's list if
/// it has a first line (`xdg-terminal-exec` takes it before the common
/// one), otherwise the common `xdg-terminals.list`.
pub fn effective_terminals_list(desktop_list: Option<String>, general: Option<String>) -> Option<String> {
    desktop_list
        .filter(|c| c.lines().next().is_some_and(|l| !l.trim().is_empty()))
        .or(general)
}

// ============================================================================
// Our own terminal search (where no source said anything)
// ============================================================================
//
// ⚠️ Why this is needed at all. The system-wide terminal choice in
// freedesktop works through the `xdg-terminal-exec` utility: GLib calls it
// first, and it reads `~/.config/xdg-terminals.list`, where Rexon writes.
// But the official Arch repositories do not have it AT ALL (only the AUR),
// Fedora got it only in 42, and Debian/Ubuntu/openSUSE have it, but it is
// installed neither by default nor as a dependency. So "out of the box" it
// is nowhere, and its presence cannot be taken for granted. Below is our
// own search over candidates: it gives a meaningful answer to "which
// terminal does the person have" even when no settings source said
// anything.

/// Terminal candidates: binary -> possible names of its `.desktop`.
///
/// The order goes from the "native" terminals of the big desktops to the
/// universal ones. There can be several `.desktop` names, because
/// distributions name the same file differently (Arch:
/// `org.gnome.Terminal.desktop`, Debian: `gnome-terminal.desktop`); which
/// one really exists is checked by the executor and passed as a list.
pub const TERMINAL_CANDIDATES: &[(&str, &[&str])] = &[
    ("konsole", &["org.kde.konsole.desktop", "konsole.desktop"]),
    ("kgx", &["org.gnome.Console.desktop", "kgx.desktop"]),
    (
        "gnome-terminal",
        &["org.gnome.Terminal.desktop", "gnome-terminal.desktop"],
    ),
    ("xfce4-terminal", &["xfce4-terminal.desktop"]),
    ("mate-terminal", &["mate-terminal.desktop"]),
    ("lxterminal", &["lxterminal.desktop"]),
    ("qterminal", &["qterminal.desktop"]),
    ("deepin-terminal", &["deepin-terminal.desktop"]),
    ("cosmic-term", &["com.system76.CosmicTerm.desktop"]),
    ("io.elementary.terminal", &["io.elementary.terminal.desktop"]),
    ("alacritty", &["Alacritty.desktop", "alacritty.desktop"]),
    ("kitty", &["kitty.desktop"]),
    ("foot", &["foot.desktop"]),
    ("wezterm", &["org.wezfurlong.wezterm.desktop"]),
    ("ghostty", &["com.mitchellh.ghostty.desktop"]),
    ("terminator", &["terminator.desktop"]),
    ("tilix", &["com.gexperts.Tilix.desktop"]),
    ("urxvt", &["rxvt-unicode.desktop", "urxvt.desktop"]),
    ("st", &["st.desktop"]),
    ("xterm", &["xterm.desktop", "debian-xterm.desktop"]),
];

/// The first installed terminal from [`TERMINAL_CANDIDATES`].
///
/// `available` are the binary names found in PATH; `desktops_present` are
/// the names of `.desktop` files actually present in the application
/// directories. If none of a candidate's names was found, we take the first
/// (the canonical freedesktop id): better to write it than to write
/// nothing.
pub fn pick_terminal(available: &[String], desktops_present: &[String]) -> Option<String> {
    for (binary, desktop_ids) in TERMINAL_CANDIDATES {
        if !available.iter().any(|b| b == binary) {
            continue;
        }
        let chosen = desktop_ids
            .iter()
            .find(|id| desktops_present.iter().any(|p| p == *id))
            .unwrap_or(&desktop_ids[0]);
        return Some((*chosen).to_string());
    }
    None
}

/// An honest warning for the UI when the system has no `xdg-terminal-exec`.
///
/// Without it the terminal choice stays written (Rexon writes it to all
/// three places), but some programs that launch a terminal through GLib
/// will open their own. Rule #13: we say so in plain words rather than stay
/// silent.
pub fn terminal_exec_missing_note() -> String {
    tr("Some apps will open their own terminal instead of the one you chose: a \
        system-wide choice needs the xdg-terminal-exec utility, and it isn't \
        installed. Rexon has already saved your choice — install the \
        “xdg-terminal-exec” package and it will work everywhere.")
}

/// Terminal status text for the UI: empty if there is nothing to warn about.
pub fn terminal_status_note(xdg_terminal_exec_present: bool) -> String {
    if xdg_terminal_exec_present {
        String::new()
    } else {
        terminal_exec_missing_note()
    }
}

// ============================================================================
// _set_ini_value: a pure INI transformation (kdeglobals and the like)
// ============================================================================

/// Port of `_set_ini_value`. `existing` is the file contents (None = no
/// file). `section` comes with square brackets (e.g. `"[General]"`).
/// Returns the new file contents.
///
/// An empty `section` means a file without sections (`xfce4/helpers.rc`):
/// the key is edited or appended before the first `[…]` header, if there is
/// one at all.
pub fn set_ini_value(existing: Option<&str>, section: &str, key: &str, value: &str) -> String {
    if section.is_empty() {
        return set_sectionless_value(existing, key, value);
    }
    // Python reads f.readlines(): lines WITH the trailing '\n' (except perhaps
    // the last one). split_inclusive reproduces this.
    let lines: Vec<&str> = match existing {
        Some(text) => text.split_inclusive('\n').collect(),
        None => Vec::new(),
    };

    let mut in_section = false;
    let mut found = false;
    let mut new_lines: Vec<String> = Vec::new();
    // `key = value` with spaces is the same key (that is how, for example,
    // `wayfire.ini` is written): otherwise the key would be appended a second
    // time next to the old one.
    let is_key = |s: &str| s.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='));

    for line in &lines {
        let stripped = line.trim();
        if stripped.starts_with('[') {
            if in_section && !found {
                new_lines.push(format!("{key}={value}\n"));
                found = true;
            }
            in_section = stripped == section;
        }
        if in_section && is_key(stripped) {
            new_lines.push(format!("{key}={value}\n"));
            found = true;
            continue;
        }
        new_lines.push((*line).to_string());
    }

    if !found {
        if !in_section {
            new_lines.push(format!("\n{section}\n"));
        }
        new_lines.push(format!("{key}={value}\n"));
    }

    new_lines.concat()
}

/// [`set_ini_value`] for a key outside sections: edit in place before the
/// first header, otherwise the line goes before it (or at the end of a file
/// without sections).
fn set_sectionless_value(existing: Option<&str>, key: &str, value: &str) -> String {
    let lines: Vec<&str> = existing.map(|t| t.split_inclusive('\n').collect()).unwrap_or_default();
    let is_key = |s: &str| s.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='));
    let new_line = format!("{key}={value}\n");
    let mut out: Vec<String> = Vec::new();
    let mut done = false;
    let mut in_head = true;
    for line in &lines {
        let stripped = line.trim();
        if in_head && stripped.starts_with('[') {
            in_head = false;
            if !done {
                out.push(new_line.clone());
                done = true;
            }
        }
        if in_head && !done && is_key(stripped) {
            out.push(new_line.clone());
            done = true;
            continue;
        }
        out.push(if line.ends_with('\n') { (*line).to_string() } else { format!("{line}\n") });
    }
    if !done {
        out.push(new_line);
    }
    out.concat()
}

// ============================================================================
// _set_terminal: the terminal restore plan
// ============================================================================

/// A step of the terminal restore plan (a port of `_set_terminal`).
/// Execution happens when connected to live Python.
/// ⚠️ The variant tags are the FORM for the plan executor (see [`crate::themes::ApplyStep`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op")]
pub enum TermStep {
    /// `open(path, "w").write(content)`.
    #[serde(rename = "write")]
    WriteFile { path: String, content: String },
    /// `os.makedirs(path, exist_ok=True)`.
    #[serde(rename = "makedirs")]
    MakeDirs { path: String },
    /// `subprocess.run(argv, check=False, capture_output=True)`.
    #[serde(rename = "run")]
    Run { argv: Vec<String> },
    /// `_set_ini_value(path, section, key, value)` (read-modify-write).
    #[serde(rename = "setini")]
    SetIni { path: String, section: String, key: String, value: String },
}

/// Where the environment keeps ITS OWN default terminal: one table for the
/// whole of Rexon (rule #12). It is read both by the "Settings" tab /
/// application protection ([`set_terminal_plan`]) and by the desktop switch
/// ([`crate::appdefaults::apply_de_specific`]); previously they had two
/// different copies, and the first did not write Cinnamon, XFCE and LXQt
/// at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalHome {
    /// `kdeglobals [General] TerminalApplication/TerminalService`.
    Kde,
    /// `org.gnome.desktop.default-applications.terminal` (GNOME, Budgie,
    /// Pantheon: the io.elementary.* schemas have no terminal key of their own).
    Gnome,
    /// `org.cinnamon.desktop.default-applications.terminal`: Nemo's "Open in
    /// Terminal" and Ctrl+Alt+T take it.
    Cinnamon,
    /// `org.mate.applications-terminal`.
    Mate,
    /// `~/.config/xfce4/helpers.rc TerminalEmulator=` — `exo-open --launch
    /// TerminalEmulator`.
    Xfce,
    /// `~/.config/lxqt-qtxdg.conf TerminalEmulator=<id>.desktop`: libqtxdg
    /// `XdgDefaultApps::setTerminal`, file `<first of XDG_CURRENT_DESKTOP>-qtxdg`.
    Lxqt,
    /// `com.deepin.desktop.default-applications.terminal` `app-id`/`exec`/
    /// `exec-arg`: Deepin's Ctrl+Alt+T and "Open in Terminal" take it
    /// (dde-daemon `bin/default-terminal`, `keybinding1`).
    Deepin,
    /// No place of its own (COSMIC, tiling ones): only the common layer.
    None,
}

/// Deepin terminal schema (deepin-desktop-schemas).
pub const DEEPIN_TERMINAL_SCHEMA: &str = "com.deepin.desktop.default-applications.terminal";

/// Environment -> terminal place. Accepts both the raw
/// `XDG_CURRENT_DESKTOP` (`X-Cinnamon`, `Budgie:GNOME`, `LXQt`) and a Rexon
/// identifier (`cinnamon-mint`, `budgie`). There is no environment table of
/// its own here: the answer comes from the shared
/// [`crate::de_customization::de_from_id`] and
/// [`crate::system::detect_current_de`].
pub fn terminal_home(desktop: &str) -> TerminalHome {
    use crate::de_customization::{de_from_id, De};
    let de = de_from_id(desktop).or_else(|| {
        crate::system::detect_current_de(desktop, "", false).and_then(|id| de_from_id(&id))
    });
    match de {
        Some(De::Kde) => TerminalHome::Kde,
        Some(De::Gnome | De::Budgie | De::Pantheon) => TerminalHome::Gnome,
        Some(De::Cinnamon) => TerminalHome::Cinnamon,
        Some(De::Mate) => TerminalHome::Mate,
        Some(De::Xfce) => TerminalHome::Xfce,
        Some(De::Lxqt) => TerminalHome::Lxqt,
        Some(De::Deepin) => TerminalHome::Deepin,
        _ => TerminalHome::None,
    }
}

/// Terminal places beyond the native [`terminal_home`]: they are read by
/// the programs of the desktop's set, not by the desktop itself. Budgie is
/// installed with Nemo (as in Ubuntu Budgie), and Nemo's "Open in Terminal"
/// takes the terminal from the Cinnamon schema (`eel-gnome-extensions.c`);
/// Ubuntu Budgie sets the same terminal there
/// (`25_budgie-desktop-environment.gschema.override`). Without this Nemo
/// would call `gnome-terminal`, which is not in the Budgie set.
pub fn terminal_extra_homes(desktop: &str) -> &'static [TerminalHome] {
    use crate::de_customization::{de_from_id, De};
    let de = de_from_id(desktop).or_else(|| {
        crate::system::detect_current_de(desktop, "", false).and_then(|id| de_from_id(&id))
    });
    match de {
        Some(De::Budgie) => &[TerminalHome::Cinnamon],
        _ => &[],
    }
}

/// The terminal program name from its `.desktop`: for `exec` in gsettings,
/// KDE's `TerminalApplication` and the XFCE helper. Where the binary name
/// does not match the identifier's tail, an explicit table; otherwise the
/// tail after the last dot in lower case (`org.kde.konsole` -> `konsole`,
/// `Alacritty` -> `alacritty`).
pub fn terminal_exec_name(terminal_desktop: &str) -> String {
    match terminal_desktop {
        "org.gnome.Console.desktop" => "kgx".to_string(),
        "org.gnome.Terminal.desktop" => "gnome-terminal".to_string(),
        "com.system76.CosmicTerm.desktop" => "cosmic-term".to_string(),
        // The tail after the dot would give "terminal", and there is no such binary.
        "io.elementary.terminal.desktop" => "io.elementary.terminal".to_string(),
        _ => {
            let s = terminal_desktop.replace(".desktop", "");
            s.rsplit('.').next().unwrap_or(&s).to_lowercase()
        }
    }
}

/// The "run a command" key (`exec-arg`) for gsettings schemas: for
/// gnome-terminal it is `--` (its `-e` is deprecated), for mate-terminal
/// `-x`, for the others the conventional `-e`.
pub fn terminal_exec_arg(exec_name: &str) -> &'static str {
    match exec_name {
        "gnome-terminal" => "--",
        "mate-terminal" => "-x",
        // The elementary terminal's `-e` takes ONE string per flag, while "the
        // rest of the line as a command" is `-x` (`--commandline`,
        // src/Application.vala).
        "io.elementary.terminal" => "-x",
        _ => "-e",
    }
}

fn gsettings_terminal_steps(schema: &str, exec: &str) -> Vec<TermStep> {
    ["exec", "exec-arg"]
        .into_iter()
        .zip([exec, terminal_exec_arg(exec)])
        .map(|(key, val)| TermStep::Run {
            argv: vec!["gsettings".into(), "set".into(), schema.into(), key.into(), val.into()],
        })
        .collect()
}

// ============================================================================
// exo helpers in XFCE (`helpers.rc`)
// ============================================================================
//
// `exo-open --launch TerminalEmulator` (and Thunar's "Open in Terminal")
// takes the name from `~/.config/xfce4/helpers.rc` and looks for the
// helper FILE `xfce4/helpers/<name>.desktop` in the XDG data directories.
// Not every program has a system helper (the xfce4-settings package):
// konsole, foot, kgx, mate-terminal, cosmic-term, wezterm, ghostty,
// ptyxis, Dolphin and Nemo have none, and a bare name in `helpers.rc`
// gives a "choose a terminal" window (D7, environment check of 02.10.2026,
// captured live). For such programs OUR OWN helper
// `~/.local/share/xfce4/helpers/custom-<Category>.desktop` is written:
// the same name and the same kind of file xfce4-settings itself creates
// when the person chooses "another program" (the strings of the installed
// `xfce4-mime-helper`).

/// Kind of XFCE helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XfceHelper {
    Terminal,
    FileManager,
}

impl XfceHelper {
    /// The `helpers.rc` key, also the `X-XFCE-Category` of the helper file.
    pub fn key(self) -> &'static str {
        match self {
            XfceHelper::Terminal => "TerminalEmulator",
            XfceHelper::FileManager => "FileManager",
        }
    }

    /// Name of our own helper (`custom-TerminalEmulator`).
    pub fn custom_id(self) -> String {
        format!("custom-{}", self.key())
    }
}

/// System helper directories (`$XDG_DATA_DIRS` by default).
pub const XFCE_SYSTEM_HELPER_DIRS: &[&str] = &["/usr/local/share/xfce4/helpers", "/usr/share/xfce4/helpers"];

/// Whether the system has a `<name>.desktop` helper.
pub fn xfce_system_helper_exists(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && XFCE_SYSTEM_HELPER_DIRS
            .iter()
            .any(|d| std::path::Path::new(&format!("{d}/{name}.desktop")).is_file())
}

/// Text of our own helper for the program `binary`.
pub fn xfce_custom_helper_text(kind: XfceHelper, binary: &str) -> String {
    let with_param = match kind {
        XfceHelper::Terminal => format!("{binary} {} %s", terminal_exec_arg(binary)),
        XfceHelper::FileManager => format!("{binary} %s"),
    };
    format!(
        "[Desktop Entry]\nNoDisplay=true\nVersion=1.0\nEncoding=UTF-8\nType=X-XFCE-Helper\n\
         X-XFCE-Category={}\nX-XFCE-Binaries={binary};\nX-XFCE-Commands={binary};\n\
         X-XFCE-CommandsWithParameter={with_param};\nIcon={binary}\nName={binary}\n",
        kind.key()
    )
}

/// The data directory next to the config directory: `<home>/.config` ->
/// `<home>/.local/share` (and `~/.config` -> `~/.local/share`).
fn data_home_beside(cfg: &str) -> String {
    let cfg = cfg.trim_end_matches('/');
    match cfg.strip_suffix("/.config") {
        Some(base) => format!("{base}/.local/share"),
        None => format!("{cfg}/../.local/share"),
    }
}

/// Steps "program `binary` is the `kind` helper in XFCE": the name in
/// `helpers.rc`, and if the system has no helper for it
/// (`has_system_helper == false`), first our own `custom-…` helper in
/// `~/.local/share/xfce4/helpers`.
pub fn xfce_helper_steps(kind: XfceHelper, binary: &str, cfg: &str, has_system_helper: bool) -> Vec<TermStep> {
    let mut steps = Vec::new();
    let value = if has_system_helper {
        binary.to_string()
    } else {
        let dir = format!("{}/xfce4/helpers", data_home_beside(cfg));
        steps.push(TermStep::MakeDirs { path: dir.clone() });
        steps.push(TermStep::WriteFile {
            path: format!("{dir}/{}.desktop", kind.custom_id()),
            content: xfce_custom_helper_text(kind, binary),
        });
        kind.custom_id()
    };
    steps.push(TermStep::SetIni {
        path: format!("{}/xfce4/helpers.rc", cfg.trim_end_matches('/')),
        section: String::new(),
        key: kind.key().into(),
        value,
    });
    steps
}

/// Terminal writes into the environment's NATIVE place (without the common
/// layer). `cfg` is the config directory (`~/.config` or `$XDG_CONFIG_HOME`).
pub fn terminal_home_steps(home: TerminalHome, terminal_desktop: &str, cfg: &str) -> Vec<TermStep> {
    terminal_home_steps_with(home, terminal_desktop, cfg, &xfce_system_helper_exists)
}

/// The same, with a fake answer to "is there a system helper" (for parity checks).
pub fn terminal_home_steps_with(
    home: TerminalHome,
    terminal_desktop: &str,
    cfg: &str,
    xfce_helper_exists: &dyn Fn(&str) -> bool,
) -> Vec<TermStep> {
    let exec = terminal_exec_name(terminal_desktop);
    match home {
        TerminalHome::Kde => {
            let kd = format!("{cfg}/kdeglobals");
            let pairs = [("TerminalApplication", exec.as_str()), ("TerminalService", terminal_desktop)];
            let mut steps: Vec<TermStep> = pairs
                .iter()
                .map(|(key, val)| TermStep::SetIni {
                    path: kd.clone(),
                    section: "[General]".into(),
                    key: (*key).into(),
                    value: (*val).into(),
                })
                .collect();
            steps.extend(pairs.iter().map(|(key, val)| TermStep::Run {
                argv: vec![
                    "kwriteconfig6".into(), "--file".into(), "kdeglobals".into(),
                    "--group".into(), "General".into(), "--key".into(),
                    (*key).into(), (*val).into(),
                ],
            }));
            steps
        }
        TerminalHome::Gnome => {
            gsettings_terminal_steps("org.gnome.desktop.default-applications.terminal", &exec)
        }
        TerminalHome::Cinnamon => {
            gsettings_terminal_steps("org.cinnamon.desktop.default-applications.terminal", &exec)
        }
        TerminalHome::Mate => gsettings_terminal_steps("org.mate.applications-terminal", &exec),
        TerminalHome::Xfce => {
            let has = xfce_helper_exists(&exec);
            xfce_helper_steps(XfceHelper::Terminal, &exec, cfg, has)
        }
        TerminalHome::Lxqt => vec![TermStep::SetIni {
            path: format!("{cfg}/lxqt-qtxdg.conf"),
            section: "[General]".into(),
            key: "TerminalEmulator".into(),
            value: terminal_desktop.to_string(),
        }],
        TerminalHome::Deepin => {
            // `app-id` is the `.desktop` name without the extension: by it the
            // Deepin service launches the terminal through the application manager;
            // `exec` is the fallback.
            let app_id = terminal_desktop.trim_end_matches(".desktop");
            let mut steps = vec![TermStep::Run {
                argv: vec![
                    "gsettings".into(), "set".into(), DEEPIN_TERMINAL_SCHEMA.into(),
                    "app-id".into(), app_id.into(),
                ],
            }];
            steps.extend(gsettings_terminal_steps(DEEPIN_TERMINAL_SCHEMA, &exec));
            steps
        }
        TerminalHome::None => Vec::new(),
    }
}

/// Port of `_set_terminal`. `home` is the expansion of `~` (the base
/// directory `~/.config` is built as `{home}/.config`).
///
/// The common layer is always written (programs outside their own
/// environment see it): `xdg-terminals.list`, the GNOME schema,
/// `environment.d/rexon-terminal.conf`. Then the environment's native place
/// from [`terminal_home_steps`].
pub fn set_terminal_plan(terminal_desktop: &str, desktop: &str, home: &str) -> Vec<TermStep> {
    set_terminal_plan_with(terminal_desktop, desktop, home, &xfce_system_helper_exists)
}

/// The same, with a fake answer to "does the system have an XFCE helper".
pub fn set_terminal_plan_with(
    terminal_desktop: &str,
    desktop: &str,
    home: &str,
    xfce_helper_exists: &dyn Fn(&str) -> bool,
) -> Vec<TermStep> {
    let exec = terminal_exec_name(terminal_desktop);
    let cfg = format!("{home}/.config");
    let de_home = terminal_home(desktop);

    let mut steps = Vec::new();

    // ── COMMON LAYER ──
    // 1. xdg-terminals.list.
    steps.push(TermStep::WriteFile {
        path: format!("{cfg}/xdg-terminals.list"),
        content: format!("{terminal_desktop}\n"),
    });
    // 1a. The own desktop's list: it overrides the common one (see the section
    // on `<desktop>-xdg-terminals.list`); marked "chosen by the person", so
    // that logging in does not replace the choice with the native terminal.
    if let Some(name) = crate::appdefaults::terminals_list_filename(desktop) {
        steps.push(TermStep::WriteFile {
            path: format!("{cfg}/{name}"),
            content: desktop_terminals_list_text(terminal_desktop, true),
        });
    }
    // 2. The GNOME schema: GLib programs read it in any environment.
    steps.extend(terminal_home_steps(TerminalHome::Gnome, terminal_desktop, &cfg));
    // 3. environment.d/rexon-terminal.conf.
    let env_dir = format!("{cfg}/environment.d");
    steps.push(TermStep::MakeDirs { path: env_dir.clone() });
    steps.push(TermStep::WriteFile {
        path: format!("{env_dir}/rexon-terminal.conf"),
        content: format!("TERMINAL={exec}\n"),
    });

    // ── NATIVE PLACE OF THE ENVIRONMENT ── (GNOME/Budgie are already written by the common layer.)
    if de_home != TerminalHome::Gnome {
        steps.extend(terminal_home_steps_with(de_home, terminal_desktop, &cfg, xfce_helper_exists));
    }
    for extra in terminal_extra_homes(desktop) {
        steps.extend(terminal_home_steps_with(*extra, terminal_desktop, &cfg, xfce_helper_exists));
    }

    steps
}

// ============================================================================
// _set_default: setting defaults
// ============================================================================

/// Result of `_set_default` (port). `Terminal`: the executor runs
/// `set_terminal_plan`; `Xdg`: a list of `xdg-mime default …` argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetDefault {
    Terminal(String),
    Xdg(Vec<Vec<String>>),
}

/// Port of `_set_default`. An unknown type without MIME -> an empty `Xdg`
/// (Python `if not mime: return`). The browser additionally gets
/// `x-scheme-handler/https`.
pub fn set_default(app_type: &str, desktop: &str) -> SetDefault {
    if app_type == "terminal" {
        return SetDefault::Terminal(desktop.to_string());
    }
    let Some(mime) = mime_for(app_type) else {
        return SetDefault::Xdg(Vec::new());
    };
    let mut cmds = vec![vec![
        "xdg-mime".to_string(),
        "default".to_string(),
        desktop.to_string(),
        mime.to_string(),
    ]];
    if app_type == "browser" {
        cmds.push(vec![
            "xdg-mime".to_string(),
            "default".to_string(),
            desktop.to_string(),
            "x-scheme-handler/https".to_string(),
        ]);
    }
    SetDefault::Xdg(cmds)
}

// ============================================================================
// snapshot(): updating protected_apps
// ============================================================================

/// Port of `snapshot`: `for key,val in current: if val and rules.get(key,False):
/// protected[key]=val`. `current` are ordered pairs (MIME_TYPES + terminal).
/// `existing` are the already saved protected_apps (order is kept, new keys
/// are appended).
pub fn compute_snapshot(
    current: &[(String, Option<String>)],
    rules: &Rules,
    existing: &[(String, Option<String>)],
) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = existing.to_vec();
    for (key, val) in current {
        let truthy = val.as_ref().is_some_and(|s| !s.is_empty());
        if truthy && rules.protect(key) {
            if let Some(e) = out.iter_mut().find(|(k, _)| k == key) {
                e.1 = val.clone();
            } else {
                out.push((key.clone(), val.clone()));
            }
        }
    }
    out
}

/// The decision "take the protection baseline at window startup or not".
///
/// The "Default application protection" section is on by default, but
/// until the first baseline is taken there is nothing to compare a
/// substitution against: Rexon remembers nothing and silently does not
/// protect. So the baseline is taken BY ITSELF on the very first launch.
///
/// ⭐ The condition is based on the FACT, not on a separate "first launch"
/// flag key: protection is on AND nothing is remembered. This way it heals
/// itself if the baseline was deleted by hand or lost with the profile, and
/// the rules do not get one more key nobody will ever reset. The same
/// meaning of "not remembered" as in the tab's status line
/// (`qt_settings::protection_status_text`): empty means nothing to compare
/// against.
///
/// An already taken baseline is NOT retaken: otherwise a second launch
/// would write over it, as "correct", a value substituted by someone else's
/// installer, exactly what the protection guards against. Retaking stays
/// with the "Remember now" button, i.e. with a deliberate action by the
/// person.
pub fn should_snapshot_at_startup(
    rules: &Rules,
    remembered: &[(String, Option<String>)],
) -> bool {
    rules.enabled() && remembered.is_empty()
}

// ============================================================================
// restore_defaults(): the restore decision
// ============================================================================

/// Result of `restore_defaults` (the pure part). `results` are ordered by
/// the traversal of protected_apps; `set_calls` are the
/// `_set_default(app_type, desktop)` calls the executor must make (in
/// order); `pending` is the resulting pending_choices (original + added).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreOutcome {
    pub results: Vec<(String, String)>,
    pub set_calls: Vec<(String, String)>,
    pub pending: Vec<(String, (String, String))>,
}

fn lookup<'a>(pairs: &'a [(String, Option<String>)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.as_deref())
}

/// Port of `restore_defaults`. `desktop_file_exists` is the `.desktop`
/// presence callback. `existing_pending` is the current pending_choices
/// (usually empty).
pub fn restore_defaults(
    protected_apps: &[(String, Option<String>)],
    rules: &Rules,
    current: &[(String, Option<String>)],
    de_switched: bool,
    existing_pending: &[(String, (String, String))],
    desktop_file_exists: &dyn Fn(&str) -> bool,
) -> RestoreOutcome {
    let mut outcome = RestoreOutcome {
        results: Vec::new(),
        set_calls: Vec::new(),
        pending: existing_pending.to_vec(),
    };

    if !rules.enabled() {
        // Python: returns {} and does NOT save pending (in-memory is untouched).
        return outcome;
    }

    for (app_type, saved) in protected_apps {
        if !rules.protect(app_type) {
            outcome.results.push((app_type.clone(), "disabled".into()));
            continue;
        }
        let saved_desktop = match saved {
            Some(s) if !s.is_empty() => s.as_str(),
            _ => {
                outcome.results.push((app_type.clone(), "absent".into()));
                continue;
            }
        };
        if !desktop_file_exists(saved_desktop) {
            outcome.results.push((app_type.clone(), "absent".into()));
            continue;
        }

        let cur = lookup(current, app_type);
        let changed = cur.is_some_and(|c| !c.is_empty() && c != saved_desktop);

        if !changed {
            outcome.results.push((app_type.clone(), "unchanged".into()));
            continue;
        }
        if de_switched {
            outcome.results.push((app_type.clone(), "skipped".into()));
            continue;
        }

        // The DE is NOT switched: restore the old one + remember the new one.
        outcome.set_calls.push((app_type.clone(), saved_desktop.to_string()));
        outcome.results.push((app_type.clone(), "restored".into()));

        if let Some(cur_desktop) = cur {
            if !cur_desktop.is_empty() && desktop_file_exists(cur_desktop) {
                let choice = (saved_desktop.to_string(), cur_desktop.to_string());
                if let Some(e) = outcome.pending.iter_mut().find(|(k, _)| k == app_type) {
                    e.1 = choice;
                } else {
                    outcome.pending.push((app_type.clone(), choice));
                }
            }
        }
    }

    outcome
}

// ============================================================================
// should_protect / resolve_choice
// ============================================================================

/// Port of `should_protect`.
pub fn should_protect(
    app_type: &str,
    rules: &Rules,
    protected_apps: &[(String, Option<String>)],
    desktop_file_exists: &dyn Fn(&str) -> bool,
) -> bool {
    if !rules.enabled() {
        return false;
    }
    if !rules.protect(app_type) {
        return false;
    }
    match lookup(protected_apps, app_type) {
        Some(d) if !d.is_empty() => desktop_file_exists(d),
        _ => false,
    }
}

/// Result of `resolve_choice` (the pure part).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveOutcome {
    /// `_set_default(app_type, new_desktop)` + an update of protected_apps:
    /// only with use_new and a choice present.
    pub set_call: Option<(String, String)>,
    pub protected_apps: Vec<(String, Option<String>)>,
    pub pending: Vec<(String, (String, String))>,
}

/// Port of `resolve_choice`. `pop` semantics: the choice is removed from
/// pending regardless of use_new; with use_new, set_default(new) +
/// protected[app]=new.
pub fn resolve_choice(
    app_type: &str,
    use_new: bool,
    protected_apps: &[(String, Option<String>)],
    pending: &[(String, (String, String))],
) -> ResolveOutcome {
    let mut out = ResolveOutcome {
        set_call: None,
        protected_apps: protected_apps.to_vec(),
        pending: pending.to_vec(),
    };
    let Some(idx) = out.pending.iter().position(|(k, _)| k == app_type) else {
        // No choice: Python returns right away.
        return out;
    };
    let (_, (_, new_desktop)) = out.pending.remove(idx);
    if use_new {
        out.set_call = Some((app_type.to_string(), new_desktop.clone()));
        if let Some(e) = out.protected_apps.iter_mut().find(|(k, _)| k == app_type) {
            e.1 = Some(new_desktop);
        } else {
            out.protected_apps.push((app_type.to_string(), Some(new_desktop)));
        }
    }
    out
}

// ============================================================================
// get_status_summary
// ============================================================================

/// Port of `get_status_summary`. Returns a JSON array of rows (like the
/// Python list of dicts). `saved`/`current`: the value or `"—"`; `changed`:
/// bool or null (`saved and cur and saved != cur`).
pub fn get_status_summary(
    protected_apps: &[(String, Option<String>)],
    current: &[(String, Option<String>)],
    rules: &Rules,
) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let all_types: Vec<&str> = MIME_TYPES
        .iter()
        .map(|(k, _)| *k)
        .chain(std::iter::once("terminal"))
        .collect();
    for app_type in all_types {
        let saved = lookup(protected_apps, app_type);
        let cur = lookup(current, app_type);
        // changed = saved and cur and saved != cur -> null if either is missing
        let changed = match (saved, cur) {
            (Some(s), Some(c)) => Value::Bool(s != c),
            _ => Value::Null,
        };
        let mut row = Map::new();
        row.insert("type".into(), Value::String(app_type.to_string()));
        row.insert("saved".into(), Value::String(saved.unwrap_or("—").to_string()));
        row.insert("current".into(), Value::String(cur.unwrap_or("—").to_string()));
        row.insert("protected".into(), Value::Bool(rules.protect(app_type)));
        row.insert("changed".into(), changed);
        rows.push(Value::Object(row));
    }
    Value::Array(rows)
}

// ============================================================================
// _get_exec_from_desktop
// ============================================================================

/// Port of `_get_exec_from_desktop`: the first `Exec=` line -> the first
/// word after `=`. `content` is the `.desktop` contents (None = no file).
/// Searches line by line.
pub fn get_exec_from_desktop(content: Option<&str>) -> Option<String> {
    let text = content?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Exec=") {
            // rest.strip().split()[0] is the first word after '=' (splitting on
            // whitespace already ignores the edges; a separate strip/trim is not
            // needed).
            return rest.split_whitespace().next().map(String::from);
        }
    }
    None
}

/// `.desktop` search paths, a port of `_desktop_file_exists`. `home` expands `~`.
pub fn desktop_search_paths(desktop: &str, home: &str) -> [String; 2] {
    [
        format!("/usr/share/applications/{desktop}"),
        format!("{home}/.local/share/applications/{desktop}"),
    ]
}

// ============================================================================
// mimeapps.list: parsing / generation / per-DE protection
// ============================================================================

/// Port of `_parse_mimeapps_list`: the `[Default Applications]` section ->
/// {mime: desktop} (desktop up to the first `;`). Returns ordered pairs
/// (file order).
pub fn parse_mimeapps_list(content: &str) -> Vec<(String, String)> {
    let mut result: Vec<(String, String)> = Vec::new();
    let mut in_section = false;
    for line in content.lines() {
        let line = line.trim();
        if line == "[Default Applications]" {
            in_section = true;
            continue;
        }
        if line.starts_with('[') && in_section {
            break; // another section
        }
        if in_section {
            if let Some(eq) = line.find('=') {
                let mime = line[..eq].trim().to_string();
                let desktop =
                    line[eq + 1..].trim().split(';').next().unwrap_or("").to_string();
                // dict[mime] = desktop (overwriting keeps the position).
                if let Some(e) = result.iter_mut().find(|(m, _)| *m == mime) {
                    e.1 = desktop;
                } else {
                    result.push((mime, desktop));
                }
            }
        }
    }
    result
}

/// First line of every `<desktop>-mimeapps.list` Rexon writes: by it Rexon
/// recognises its own file (to follow the person's choice into it, to clean it
/// up when the desktop or Rexon is removed). A file without it belongs to the
/// person and is never rewritten or removed.
pub const MIMEAPPS_MARK: &str = "# Rexon: per-desktop defaults";

/// Whether a `<desktop>-mimeapps.list` text was written by Rexon: the current
/// marker, or the header of versions before 25.11.2026 (Russian, see below).
pub fn is_rexon_mimeapps(text: &str) -> bool {
    text.lines().take(3).any(|l| {
        let l = l.trim();
        l == MIMEAPPS_MARK || is_legacy_mimeapps_header(l)
    })
}

/// The header older versions wrote (in Russian, before the rule "files and
/// logs in English"): `# <words> Rexon <words> DE`. Recognised by its ASCII
/// skeleton so that the old Russian words do not have to live in the code.
fn is_legacy_mimeapps_header(line: &str) -> bool {
    line.starts_with("# ")
        && line.ends_with(" DE")
        && line.contains(" Rexon ")
        && line.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// Port of `_write_mimeapps_list`: header + `[Default Applications]` +
/// entries SORTED by MIME. `timestamp` = `datetime.now().isoformat(
/// timespec='seconds')`, passed as a parameter.
///
/// The header is English and starts with [`MIMEAPPS_MARK`] (the original wrote
/// a Russian line here; files written by Rexon are read by people of any
/// language, and logs and file comments are English).
pub fn serialize_mimeapps_list(entries: &[(String, String)], timestamp: &str) -> String {
    let mut out = String::new();
    out.push_str(MIMEAPPS_MARK);
    out.push('\n');
    // No clock at hand (the login route) — no "Updated" line.
    if !timestamp.is_empty() {
        out.push_str(&format!("# Updated: {timestamp}\n"));
    }
    out.push('\n');
    out.push_str("[Default Applications]\n");
    let mut sorted: Vec<&(String, String)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (mime, desktop) in sorted {
        out.push_str(&format!("{mime}={desktop}\n"));
    }
    out
}

/// Merge a desktop's native programs into its existing list.
///
/// ⭐ What is already in the file WINS while its program is installed: it is
/// either the person's choice (followed into the file by
/// [`follow_choice_in_desktop_list`] / [`adopt_session_choices`]) or a native
/// program written earlier. The original did `merged.update(native)`, and
/// every next desktop install put the native programs back over what the
/// person had chosen in that desktop. Only missing types are added, and a line
/// whose program is gone is replaced. Browser types are never added.
pub fn merge_native_entries(
    existing: &[(String, String)],
    native: &[(String, String)],
    desktop_file_exists: &dyn Fn(&str) -> bool,
) -> Vec<(String, String)> {
    let mut merged = existing.to_vec();
    for (m, d) in native {
        if is_browser_mime(m) {
            continue;
        }
        match merged.iter_mut().find(|(mm, _)| mm == m) {
            Some(e) if !e.1.is_empty() && desktop_file_exists(&e.1) => {}
            Some(e) => e.1 = d.clone(),
            None => merged.push((m.clone(), d.clone())),
        }
    }
    merged
}

/// `<desktop>-mimeapps.list` names a session reads, in its order: every
/// element of `XDG_CURRENT_DESKTOP` in lowercase (`Budgie:GNOME` →
/// `budgie-…`, `gnome-…`).
pub fn session_desktop_lists(xdg_current_desktop: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in xdg_current_desktop.split(':') {
        let t = token.trim().to_lowercase();
        if t.is_empty() {
            continue;
        }
        let name = format!("{t}-mimeapps.list");
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Follow a choice made for the whole session (`xdg-mime default` writes only
/// the shared `~/.config/mimeapps.list`) into a desktop's own list.
///
/// WHY. By the specification `<desktop>-mimeapps.list` is read BEFORE the
/// shared file, so a type present in it hides the choice: the person picks
/// "always open with X" in Nemo or Dolphin (or Rexon's «Restore as it was»
/// does), and for that type nothing changes. So the line in the desktop list
/// takes the same program.
///
/// Only Rexon's own list ([`is_rexon_mimeapps`]) is edited, only a type that is
/// already in it (a missing type falls through to the shared file anyway),
/// never a browser type. The text is edited in place: other lines and
/// sections stay byte for byte. `None` — nothing to change.
pub fn follow_choice_in_desktop_list(text: &str, mime: &str, desktop: &str) -> Option<String> {
    if !is_rexon_mimeapps(text) || is_browser_mime(mime) || desktop.trim().is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len() + desktop.len());
    let mut in_section = false;
    let mut changed = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        if t.starts_with('[') {
            in_section = t == "[Default Applications]";
        } else if in_section {
            if let Some((k, v)) = t.split_once('=') {
                if k.trim() == mime {
                    let current = v.trim().split(';').next().unwrap_or("").trim();
                    if current != desktop {
                        let nl = if line.ends_with('\n') { "\n" } else { "" };
                        out.push_str(&format!("{mime}={desktop}{nl}"));
                        changed = true;
                        continue;
                    }
                }
            }
        }
        out.push_str(line);
    }
    changed.then_some(out)
}

/// What the shared `mimeapps.list` looked like at the last session login, and
/// which desktop list that session read (`~/.config/rexon/mimeapps_seen.json`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChoicesSeen {
    /// `<desktop>-mimeapps.list` of that session.
    pub list: String,
    /// `[Default Applications]` of the shared file then.
    pub general: Vec<(String, String)>,
}

/// Name of the [`ChoicesSeen`] file inside `~/.config/rexon`.
pub const CHOICES_SEEN_FILE: &str = "mimeapps_seen.json";

pub fn serialize_choices_seen(seen: &ChoicesSeen) -> String {
    let mut general = Map::new();
    for (m, d) in &seen.general {
        general.insert(m.clone(), Value::String(d.clone()));
    }
    let mut root = Map::new();
    root.insert("list".into(), Value::String(seen.list.clone()));
    root.insert("general".into(), Value::Object(general));
    serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default() + "\n"
}

/// `None` — no file or not ours/broken: then nothing is adopted this time.
pub fn parse_choices_seen(text: &str) -> Option<ChoicesSeen> {
    let v: Value = serde_json::from_str(text).ok()?;
    let list = v.get("list")?.as_str()?.to_string();
    if !list.ends_with("-mimeapps.list") || list.contains('/') {
        return None;
    }
    let general = v
        .get("general")?
        .as_object()?
        .iter()
        .filter_map(|(k, d)| Some((k.clone(), d.as_str()?.to_string())))
        .collect();
    Some(ChoicesSeen { list, general })
}

/// The choices made during the previous session, carried into its desktop's
/// list: every type whose program in the shared `mimeapps.list` changed since
/// that session's login (`before`) is set to the new program in `text`
/// (only types already in our list, never browser types — see
/// [`follow_choice_in_desktop_list`]). The person chose it in that desktop,
/// so there it must win. `None` — nothing to change.
pub fn adopt_session_choices(
    text: &str,
    before: &[(String, String)],
    after: &[(String, String)],
) -> Option<String> {
    let mut cur = text.to_string();
    let mut changed = false;
    for (mime, desktop) in after {
        let was = before.iter().find(|(m, _)| m == mime).map(|(_, d)| d.as_str());
        if was == Some(desktop.as_str()) {
            continue;
        }
        if let Some(next) = follow_choice_in_desktop_list(&cur, mime, desktop) {
            cur = next;
            changed = true;
        }
    }
    changed.then_some(cur)
}

/// Result of `protect_de_mimeapps` (the pure part).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectDeOutcome {
    /// Ordered by the traversal of installed_des: (de_id, "created"/"updated"/
    /// "skipped"/"empty").
    pub results: Vec<(String, String)>,
    /// Files to write: (absolute path, contents), in processing order.
    pub writes: Vec<(String, String)>,
    /// Parallel to `writes`: how many NATIVE DE entries are actually installed
    /// (`len(valid_entries)` in the original's log; merging with user entries
    /// does not increase this number).
    pub valid_counts: Vec<usize>,
}

/// Port of `protect_de_mimeapps` (the pure part). Side effects via
/// callbacks:
///   * `desktop_file_exists(desktop)`: whether the `.desktop` is installed;
///   * `read_existing(path)`: contents of an existing `*-mimeapps.list`
///     (None = no file).
///
/// `config_dir` is the expanded `~/.config`; `timestamp` is for the header.
/// `current_de` is not used in the original (kept in the signature for
/// parity).
pub fn protect_de_mimeapps(
    _current_de: &str,
    installed_des: &[String],
    config_dir: &str,
    timestamp: &str,
    desktop_file_exists: &dyn Fn(&str) -> bool,
    read_existing: &dyn Fn(&str) -> Option<String>,
) -> ProtectDeOutcome {
    let mut outcome = ProtectDeOutcome {
        results: Vec::new(),
        writes: Vec::new(),
        valid_counts: Vec::new(),
    };

    for de_id in installed_des {
        let native = match native_apps_for(de_id) {
            Some(n) if !n.is_empty() => n,
            _ => {
                outcome.results.push((de_id.clone(), "empty".into()));
                continue;
            }
        };

        let path = format!("{config_dir}/{}", mimeapps_filename_for(de_id));

        // Only the actually installed .desktop files (native order). Browser
        // types are never ours to write, whatever a table says.
        let valid: Vec<(String, String)> = native
            .iter()
            .filter(|(m, _)| !is_browser_mime(m))
            .filter(|(_, d)| desktop_file_exists(d))
            .map(|(m, d)| (m.to_string(), d.to_string()))
            .collect();

        if valid.is_empty() {
            outcome.results.push((de_id.clone(), "skipped".into()));
            continue;
        }

        // Existing file -> parse (for the merge and for the action).
        let existing: Vec<(String, String)> = match read_existing(&path) {
            Some(content) => parse_mimeapps_list(&content),
            None => Vec::new(),
        };

        // The person's line wins (see [`merge_native_entries`]); the output order
        // does not matter: serialise sorts by MIME.
        let merged = merge_native_entries(&existing, &valid, desktop_file_exists);

        let content = serialize_mimeapps_list(&merged, timestamp);
        outcome.writes.push((path, content));
        outcome.valid_counts.push(valid.len());

        // Quirk: "updated" if the PARSED dictionary is non-empty (not whether the file exists).
        let action = if existing.is_empty() { "created" } else { "updated" };
        outcome.results.push((de_id.clone(), action.to_string()));
    }

    outcome
}
