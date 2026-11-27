//! DE screenshots — port of the deterministic parts of `rexon/screenshots.py`
//! (`DEScreenshotManager`).
//!
//! ONLY pure input→output pieces are ported (checked by the golden test
//! `screenshots_parity.rs` against Python itself):
//!   * the tables `DE_COLORS`, `DE_DISPLAY_NAMES`, `SCREENSHOT_URLS`, the map of
//!     Wikimedia files (`filename_for_de`);
//!   * generation of the built-in SVG placeholder (`generate_svg`) — pure text,
//!     checked byte for byte for every DE;
//!   * choosing the argv of a capture tool by availability (`screenshot_tools`,
//!     `select_screenshot_tool`) — availability is supplied by a closure, the subprocess itself
//!     is NOT executed;
//!   * file names and path candidates (`auto_name`, `custom_name`,
//!     `placeholder_name`, `screenshot_path_candidates`, `extra_candidates`,
//!     `save_ext`/`save_dest`);
//!   * URL helpers (`official_screenshot_urls`, `online_url`, `online_urls`);
//!   * resolving the real CDN URL via the Wikimedia API (`wikimedia_api_url`,
//!     `parse_wikimedia_imageinfo` — port of `_resolve_via_api`);
//!   * accepting a downloaded response (`validate_image_response`, `retry_wait_secs`);
//!   * the auto-download plan (`auto_download_plan` — port of building `tasks` in
//!     `auto_download_all`, FS observations are supplied by closures).
//!
//! Side effects are NOT ported (a thin wrapper over the FS/subprocesses/network
//! on top of these pure functions — as in `backup.rs`/`distro_compat.rs`):
//!   * actually taking a screenshot (`subprocess.run`) — the core hands out argv/plan,
//!     execution happens when connected to live Python;
//!   * reading/writing the screenshot directory, `shutil.copy2`, `os.remove`;
//!   * downloading over HTTP/Wikimedia API, the parallel `auto_download_all`.
//!
//! Quirks of the original are kept for parity (marked below).

use serde_json::Value;

use crate::i18n::{n_, tr, trc, trf};
use crate::net::{self, LogLevel, NetError, NetIo};

/// Screenshot subdirectory under `$HOME` with the STANDARD config root
/// (`~/.config/rexon/screenshots`). Not suitable for building a path — only
/// for comparison with the Python reference recorded without `$XDG_CONFIG_HOME`; the real
/// path is given by [`screenshot_dir_xdg`].
pub const SCREENSHOT_SUBDIR: &str = ".config/rexon/screenshots";

/// Absolute path of the screenshot directory with the standard config root.
///
/// ⚠️ Kept for the sake of the PyO3 bridge signature. Product callers need
/// [`screenshot_dir_xdg`]: without `$XDG_CONFIG_HOME` the path drifts away from
/// what Python reads (`DEScreenshotManager.SCREENSHOT_DIR` is built with
/// `expand_user("~/.config/rexon/screenshots")`, i.e. XDG-aware).
pub fn screenshot_dir(home: &str) -> String {
    screenshot_dir_xdg(None, home)
}

/// Screenshot directory honoring `$XDG_CONFIG_HOME` — port of
/// `expand_user("~/.config/rexon/screenshots")`.
///
/// ⚠️ Fix: the path used to be glued as a literal from a bare `$HOME`. Python
/// meanwhile expanded it via `expand_user`, and on a machine with a non-standard
/// `XDG_CONFIG_HOME` the carousel of the «Desktops» tab looked into ONE
/// folder while taken screenshots landed in ANOTHER: the shot was taken, yet the
/// card still showed the drawn placeholder. Built through a single point,
/// [`crate::utils::rexon_config_dir_at`] — like all Rexon stores.
pub fn screenshot_dir_xdg(xdg_config_home: Option<&str>, home: &str) -> String {
    format!("{}/screenshots", crate::utils::rexon_config_dir_at(xdg_config_home, home))
}

/// Default gradient colors for an unknown DE (the `DE_COLORS.get` fallback).
pub const FALLBACK_COLORS: (&str, &str) = ("#607D8B", "#37474F");

/// Gradient color table of the SVG placeholder (`DEScreenshotManager.DE_COLORS`).
/// Order = declaration order in the original (matters for generators/comparison).
pub const DE_COLORS: &[(&str, &str, &str)] = &[
    ("cinnamon-mint", "#4CAF50", "#2E7D32"),
    ("cinnamon", "#8BC34A", "#558B2F"),
    ("xfce", "#2196F3", "#1565C0"),
    ("mate", "#4CAF50", "#1B5E20"),
    ("gnome", "#3F51B5", "#1A237E"),
    ("kde", "#1565C0", "#0D47A1"),
    ("budgie", "#5E35B1", "#311B92"),
    ("pantheon", "#3689E6", "#0D52BF"),
    ("deepin", "#0081FF", "#0050A0"),
    ("cosmic", "#E64A19", "#BF360C"),
    ("hyprland", "#00BCD4", "#006064"),
    ("lxqt", "#03A9F4", "#01579B"),
    ("niri", "#EC407A", "#AD1457"),
    ("sway", "#26A69A", "#00695C"),
    ("river", "#5C6BC0", "#283593"),
    ("wayfire", "#FF7043", "#BF360C"),
];

/// Human-readable DE names (`DEScreenshotManager.DE_DISPLAY_NAMES`).
pub const DE_DISPLAY_NAMES: &[(&str, &str)] = &[
    ("cinnamon-mint", "Cinnamon Mint"),
    ("cinnamon", "Cinnamon"),
    ("xfce", "XFCE"),
    ("mate", "MATE"),
    ("gnome", "GNOME"),
    ("kde", "KDE Plasma"),
    ("budgie", "Budgie"),
    ("pantheon", "Pantheon"),
    ("deepin", "Deepin"),
    ("cosmic", "COSMIC"),
    ("hyprland", "Hyprland"),
    ("lxqt", "LXQt"),
    ("niri", "niri"),
    ("sway", "Sway"),
    ("river", "River"),
    ("wayfire", "Wayfire"),
];

/// What Rexon calls a desktop it did not recognize. One name for the whole
/// product: it is returned by both [`display_name`] and `qt_settings::de_display`.
///
/// ⚠️ Put it into a phrase ONLY via [`place_de_name`]: desktop names
/// («KDE», «Sway») do not decline, but this one does (in Russian), and plain substitution
/// produced the ungrammatical «for YOUR DESKTOP» in the nominative case.
///
/// This is the `msgid`; the translation is shown — [`unknown_de_name`].
pub const UNKNOWN_DE_NAME: &str = n_("YOUR DESKTOP");

/// [`UNKNOWN_DE_NAME`] in the interface language — the name itself, without a preposition
/// («Current environment: <b>YOUR DESKTOP</b>»).
pub fn unknown_de_name() -> String {
    // TRANSLATORS: name shown for a desktop environment Rexon couldn't
    // recognize, e.g. “Current desktop: YOUR DESKTOP”. Capitals match how
    // known names (KDE, GNOME) are shown.
    tr(UNKNOWN_DE_NAME)
}

/// The unrecognized desktop inside a phrase, after a preposition («for your desktop»).
///
/// For languages where the name does not decline: one form for all prepositions. Russian
/// declines it itself — by the case table in [`place_de_name`].
fn unknown_de_in_sentence() -> String {
    // TRANSLATORS: the unrecognized desktop inside a sentence, after a
    // preposition: “Restored 2 apps for your desktop”.
    trc("unknown desktop in a sentence", "your desktop")
}

/// Is the name the unrecognized desktop (in any interface language)?
fn is_unknown_de_name(name: &str) -> bool {
    name == UNKNOWN_DE_NAME || name == unknown_de_name()
}

/// Is the shown desktop name the placeholder for an unrecognized one (`msgid` or translation)?
/// For callers that have a whole phrase of their own for an unrecognized desktop.
pub fn is_unknown_de_display(name: &str) -> bool {
    is_unknown_de_name(name)
}

/// Whether the desktop is unrecognized: empty, `unknown` (environment detection) or `generic`
/// (wallpaper engine detection).
pub fn is_unknown_de(de: &str) -> bool {
    let de = de.trim();
    de.is_empty() || de.eq_ignore_ascii_case("unknown") || de.eq_ignore_ascii_case("generic")
}

/// The form of [`UNKNOWN_DE_NAME`] after a word that requires a case, or
/// `None` — the word requires no case (a colon, a tag, a noun).
///
/// Russian cases — only after a Russian word (in a Russian template). After
/// an English preposition («for %1», «on %1») — the single «inside a phrase» form from
/// the catalog: English has no use for a case table. Other places
/// («Restore %1 Apps») carry no preposition — they need a whole phrase at the
/// caller, not gluing.
fn unknown_de_form(word: &str) -> Option<String> {
    const EN_PREPOSITIONS: &[&str] = &[
        "for", "on", "in", "at", "of", "with", "to", "from", "by", "under", "about", "into",
        "onto", "without",
    ];
    if EN_PREPOSITIONS.contains(&word) {
        return Some(unknown_de_in_sentence());
    }
    russian_unknown_de_form(word).map(str::to_string)
}

/// The Russian form of «your desktop» by the preceding word.
fn russian_unknown_de_form(word: &str) -> Option<&'static str> {
    match word {
        // genitive
        "для" | "у" | "из" | "от" | "без" | "до" | "около" | "после" | "кроме" | "среди"
        | "окружения" | "приложения" => Some("вашего рабочего стола"),
        // prepositional
        "на" | "в" | "во" | "о" | "об" | "при" => Some("вашем рабочем столе"),
        // dative
        "к" | "ко" | "по" => Some("вашему рабочему столу"),
        // instrumental
        "с" | "со" | "под" | "над" | "перед" | "за" | "между" => Some("вашим рабочим столом"),
        _ => None,
    }
}

/// Put the desktop name in place of `slot` in the template — grammatically.
///
/// A known name is substituted as is. [`UNKNOWN_DE_NAME`] after a word
/// requiring a case is replaced by its form in that case: Russian «for {0}» → «for
/// your desktop» in the genitive, «on {de}» → «on your desktop» in the prepositional. A quoted name
/// after the word «environment» goes away together with that word: «For the environment
/// «{0}»» → «For your desktop». Elsewhere («Current
/// environment: <b>{0}</b>») the nominative case stays.
pub fn place_de_name(template: &str, slot: &str, name: &str) -> String {
    if slot.is_empty() || !is_unknown_de_name(name) {
        return template.replace(slot, name);
    }
    let name = unknown_de_name();
    let name = name.as_str();
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(pos) = rest.find(slot) {
        let head = &rest[..pos];
        let mut after = &rest[pos + slot.len()..];
        // Quotes in a Russian template are «», in an English one “”.
        let quote = ['«', '“'].into_iter().find(|q| head.ends_with(*q));
        let quoted = quote.is_some();
        let bare = quote.and_then(|q| head.strip_suffix(q)).unwrap_or(head);
        let form = bare.strip_suffix(' ').and_then(|before| {
            let word_len: usize = before
                .chars()
                .rev()
                .take_while(|c| c.is_alphabetic())
                .map(char::len_utf8)
                .sum();
            let word = &before[before.len() - word_len..];
            let form = unknown_de_form(&word.to_lowercase())?;
            Some((before, word, form))
        });
        match form {
            Some((before, word, form)) => {
                if word.to_lowercase() == "окружения" && quoted {
                    // «For the environment «X»» — the word goes away together with the quotes.
                    out.push_str(&before[..before.len() - word.len()]);
                } else {
                    // «for “%1”» → «for your desktop»: the preposition stays,
                    // the quotes (if any) go away.
                    out.push_str(before);
                    out.push(' ');
                }
                out.push_str(&form);
                if quoted {
                    after = after.strip_prefix(['»', '”']).unwrap_or(after);
                }
            }
            None => {
                out.push_str(head);
                out.push_str(name);
            }
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Gradient colors for a DE — `DE_COLORS.get(de, FALLBACK)`.
pub fn de_colors(de: &str) -> (&'static str, &'static str) {
    DE_COLORS
        .iter()
        .find(|(k, _, _)| *k == de)
        .map(|(_, a, b)| (*a, *b))
        .unwrap_or(FALLBACK_COLORS)
}

/// Display name of a DE — `DE_DISPLAY_NAMES.get(de, de.upper())`.
///
/// Quirk kept: for an unknown DE the fallback = `de.upper()` (Python
/// `str.upper()`), in Rust `to_uppercase()` — equivalent for ASCII ids
/// (the intentional difference is kept for parity: all DE ids are ASCII).
pub fn display_name(de: &str) -> String {
    // ⭐ Fix (rule #13): an unrecognized desktop was called «UNKNOWN» or
    // an empty string — and the tab label read «For the environment «»».
    if is_unknown_de(de) {
        return unknown_de_name();
    }
    table_display_name(de)
}

/// The table name without substituting the unrecognized desktop — for the placeholder picture
/// [`generate_svg`]: it is compared with the original byte for byte, and for an empty id
/// its caption is empty. Texts for people use [`display_name`].
fn table_display_name(de: &str) -> String {
    DE_DISPLAY_NAMES
        .iter()
        .find(|(k, _)| *k == de)
        .map(|(_, v)| (*v).to_string())
        .unwrap_or_else(|| de.to_uppercase())
}

/// Built-in SVG placeholder for a DE (`_generate_svg`) — pure text,
/// byte for byte like the original. No trailing newline.
pub fn generate_svg(de: &str) -> String {
    generate_svg_for(de, PlaceholderState::Loading)
}

/// A placeholder with a caption matching the download state. Differs from [`generate_svg`]
/// only in the bottom line: for [`PlaceholderState::Loading`] the bytes are the same as
/// the original's.
pub fn generate_svg_for(de: &str, state: PlaceholderState) -> String {
    let caption = state.caption();
    let (bg1, bg2) = de_colors(de);
    let display = table_display_name(de);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"480\" height=\"160\" viewBox=\"0 0 480 160\">\n  \
<defs>\n    \
<linearGradient id=\"bg\" x1=\"0%\" y1=\"0%\" x2=\"100%\" y2=\"100%\">\n      \
<stop offset=\"0%\" style=\"stop-color:{bg1}\"/>\n      \
<stop offset=\"100%\" style=\"stop-color:{bg2}\"/>\n    \
</linearGradient>\n  \
</defs>\n  \
<rect width=\"480\" height=\"160\" rx=\"10\" fill=\"url(#bg)\"/>\n  \
<rect y=\"0\" width=\"480\" height=\"24\" fill=\"rgba(0,0,0,0.25)\"/>\n  \
<rect x=\"8\" y=\"6\" width=\"10\" height=\"10\" rx=\"5\" fill=\"#FF5F56\"/>\n  \
<rect x=\"26\" y=\"6\" width=\"10\" height=\"10\" rx=\"5\" fill=\"#FFBD2E\"/>\n  \
<rect x=\"44\" y=\"6\" width=\"10\" height=\"10\" rx=\"5\" fill=\"#27C93F\"/>\n  \
<rect x=\"60\" y=\"42\" width=\"360\" height=\"98\" rx=\"8\" fill=\"rgba(255,255,255,0.10)\" stroke=\"rgba(255,255,255,0.18)\" stroke-width=\"1\"/>\n  \
<text x=\"240\" y=\"98\" font-family=\"sans-serif\" font-size=\"28\" font-weight=\"bold\" fill=\"white\" text-anchor=\"middle\">{display}</text>\n  \
<text x=\"240\" y=\"148\" font-family=\"sans-serif\" font-size=\"11\" fill=\"rgba(255,255,255,0.45)\" text-anchor=\"middle\">{caption}</text>\n\
</svg>"
    )
}

// ── File names ─────────────────────────────────────────────────────────────

/// Name of an auto-taken screenshot — `{de}_auto.png`.
pub fn auto_name(de: &str) -> String {
    format!("{de}_auto.png")
}

/// Name of a user screenshot — `{de}.png` (`set_custom_screenshot`).
pub fn custom_name(de: &str) -> String {
    format!("{de}.png")
}

/// File name of the SVG placeholder — `{de}_placeholder.svg`.
pub fn placeholder_name(de: &str) -> String {
    format!("{de}_placeholder.svg")
}

// ── Placeholder state ──────────────────────────────────────────────────────
//
// The placeholder used to always promise «Screenshot is loading…» — and without a network
// it promised forever: the user waited for something that would never come (rule #13). Now
// the caption is honest and depends on what the core knows about the download.

/// What the placeholder says instead of a screenshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderState {
    /// Screenshots of this desktop are being downloaded.
    Loading,
    /// The download came to nothing: no network or the source did not answer.
    NoInternet,
    /// Nowhere to download from — the desktop has no screenshot address at all.
    NoScreenshot,
}

/// Placeholder caption while downloading — the original's text.
pub const CAPTION_LOADING: &str = n_("Loading screenshot…");
/// Placeholder caption when the download failed.
pub const CAPTION_NO_INTERNET: &str = n_("The screenshot will appear once you're online");
/// Placeholder caption of a desktop without a screenshot source.
pub const CAPTION_NO_SCREENSHOT: &str = n_("No screenshot of this desktop yet");

impl PlaceholderState {
    /// The placeholder's bottom line in the interface language (`CAPTION_*` are `msgid`s).
    pub fn caption(self) -> String {
        tr(match self {
            PlaceholderState::Loading => CAPTION_LOADING,
            PlaceholderState::NoInternet => CAPTION_NO_INTERNET,
            PlaceholderState::NoScreenshot => CAPTION_NO_SCREENSHOT,
        })
    }
}

/// Placeholder file name in the given state. A separate name for each state —
/// because QML caches the picture by address: rewrite one file, and the window
/// would show the old caption. `Loading` keeps the old name
/// ([`placeholder_name`]), shared with Python.
pub fn placeholder_name_for(de: &str, state: PlaceholderState) -> String {
    match state {
        PlaceholderState::Loading => placeholder_name(de),
        PlaceholderState::NoInternet => format!("{de}_placeholder_offline.svg"),
        PlaceholderState::NoScreenshot => format!("{de}_placeholder_none.svg"),
    }
}

/// Whether the desktop has somewhere to download screenshots from — the [`SCREENSHOT_URLS`] list
/// used by auto-download.
pub fn has_download_source(de: &str) -> bool {
    SCREENSHOT_URLS.iter().any(|(d, urls)| *d == de && !urls.is_empty())
}

/// Placeholder state: `pending` — whether the desktop is waiting for its download batch.
pub fn placeholder_state(de: &str, pending: bool) -> PlaceholderState {
    if !has_download_source(de) {
        PlaceholderState::NoScreenshot
    } else if pending {
        PlaceholderState::Loading
    } else {
        PlaceholderState::NoInternet
    }
}

// ── Path candidates (no FS checks; checks are an effect in the wrapper) ─────

/// Suffix order when looking for a screenshot (`get_screenshot_path`).
pub const PATH_SUFFIXES: &[&str] = &["", "_auto"];
/// Extension order when looking for a screenshot.
pub const PATH_EXTS: &[&str] = &[".png", ".jpg", ".webp"];
/// Minimum size of a candidate file in bytes — accepted when `size > 100`.
pub const MIN_SIZE: u64 = 100;

/// Path candidates of the main screenshot in search order
/// (`get_screenshot_path`): `(suffix, ext)` for `suffix ∈ {"", "_auto"}` and
/// `ext ∈ {.png, .jpg, .webp}`. The live search takes the first existing one with
/// `size > MIN_SIZE`. `delete_screenshot` uses the same candidate list
/// (but without the size filter — it deletes any existing one).
pub fn screenshot_path_candidates(dir: &str, de: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(PATH_SUFFIXES.len() * PATH_EXTS.len());
    for suffix in PATH_SUFFIXES {
        for ext in PATH_EXTS {
            out.push(format!("{dir}/{de}{suffix}{ext}"));
        }
    }
    out
}

/// Candidates of extra carousel screenshots (`get_screenshot_paths`):
/// `{de}_1..{de}_5` × `{.png, .jpg, .webp}`, as a flat list in search
/// order. The live search takes the FIRST existing extension for each index
/// (`break`) — this FS semantics lives in the wrapper, here only the candidates.
pub fn extra_candidates(dir: &str, de: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(5 * PATH_EXTS.len());
    for i in 1..=5 {
        for ext in PATH_EXTS {
            out.push(format!("{dir}/{de}_{i}{ext}"));
        }
    }
    out
}

// ── Taking a screenshot (argv plan; subprocess is an effect in the wrapper) ──

/// Message about missing capture tools (`take_screenshot`).
pub fn no_tools_msg() -> String {
    tr("No screenshot program found (grim, spectacle, gnome-screenshot, scrot, maim)")
}

/// Screenshot tool candidates in priority order
/// (`take_screenshot`): each is a full argv writing to `{dir}/{de}_auto.png`.
/// Order kept: scrot → gnome-screenshot → maim → spectacle → import.
pub fn screenshot_tools(dir: &str, de: &str) -> Vec<Vec<String>> {
    let out = format!("{dir}/{de}_auto.png");
    vec![
        // Wayland first: on sway/Hyprland/niri (goal #4 of the platform list)
        // grim is the only working option, while the X11 tools there are either
        // missing or capture a black frame through XWayland.
        vec!["grim".into(), out.clone()],
        vec!["spectacle".into(), "--nonotify".into(), "-b".into(),
             "-o".into(), out.clone()],
        vec!["gnome-screenshot".into(), "-f".into(), out.clone()],
        vec!["scrot".into(), "-z".into(), out.clone()],
        vec!["maim".into(), out.clone()],
        vec!["import".into(), "-window".into(), "root".into(), out],
    ]
}

/// Choose the argv of the first available capture tool (`take_screenshot` without
/// running the subprocess): availability of the command `cmd[0]` is supplied by the closure
/// `is_available` (port of `safe_check_command` = `shutil.which(cmd) is not None`).
/// Returns the argv of the chosen tool, or None if none is available —
/// the caller (Python) then runs the subprocess and checks the result.
pub fn select_screenshot_tool<F>(dir: &str, de: &str, is_available: F) -> Option<Vec<String>>
where
    F: Fn(&str) -> bool,
{
    screenshot_tools(dir, de)
        .into_iter()
        .find(|cmd| is_available(&cmd[0]))
}

// ── Saving downloaded data (`_save_screenshot_data`) ───────────────────────

/// File extension by Content-Type (`_save_screenshot_data`): webp → `.webp`,
/// jpeg/jpg → `.jpg`, otherwise `.png`. `None` is treated as an empty string.
pub fn save_ext(content_type: Option<&str>) -> &'static str {
    let ct = content_type.unwrap_or("");
    if ct.contains("webp") {
        ".webp"
    } else if ct.contains("jpeg") || ct.contains("jpg") {
        ".jpg"
    } else {
        ".png"
    }
}

/// Save path of a downloaded screenshot — `{dir}/{de}{ext}` by Content-Type.
pub fn save_dest(dir: &str, de: &str, content_type: Option<&str>) -> String {
    format!("{dir}/{de}{}", save_ext(content_type))
}

// ── Wikimedia names and screenshot URLs ────────────────────────────────────

/// File name on Wikimedia Commons for a DE (`_filename_for_de`), None if none.
/// Quirk: there is no entry for `cinnamon` (unlike `cinnamon-mint`).
/// `niri` has no entry on purpose: its only screenshot on Commons has a
/// German interface, and the fallback path would replace the main screenshot with it.
pub fn filename_for_de(de: &str) -> Option<&'static str> {
    let m: &[(&str, &str)] = &[
        ("kde", "KDE_neon_6_Breeze_Desktop_English.png"),
        ("gnome", "Fedora_Workstation_39_Desktop_English.png"),
        ("xfce", "Xubuntu_24.04_LTS_default_desktop_-_English.png"),
        (
            "mate",
            "Ubuntu_MATE_24.04_LTS_default_desktop_-_English.png",
        ),
        ("cinnamon-mint", "LinuxMint22-Wilma-English.png"),
        ("budgie", "Ubuntu_Budgie_22.04_Desktop.png"),
        ("pantheon", "Elementary_OS_8.0_default_desktop_-_English.png"),
        ("deepin", "Deepin_Linux_23.1_default_desktop_-_English.png"),
        (
            "cosmic",
            "Pop!_OS_24.04_Alpha_7_default_desktop_-_English.png",
        ),
        ("hyprland", "Omarchy_Tokyo_Night_theme.png"),
        (
            "lxqt",
            "Lubuntu_24.04_LTS_default_desktop_-_English.jpg",
        ),
        ("sway", "Sway_1.4_Window_Manager.png"),
    ];
    m.iter().find(|(k, _)| *k == de).map(|(_, v)| *v)
}

/// Direct screenshot URLs by DE (`SCREENSHOT_URLS`), order kept.
pub const SCREENSHOT_URLS: &[(&str, &[&str])] = &[
    (
        "cinnamon-mint",
        &[
            "https://www.linuxmint.com/web/img/screenshots/c1.jpg",
            "https://www.linuxmint.com/web/img/screenshots/c2.jpg",
            "https://commons.wikimedia.org/wiki/Special:FilePath/LinuxMint22-Wilma-English.png",
        ],
    ),
    (
        "xfce",
        &[
            "https://cdn.xfce.org/about/screenshots/4.20-1.png",
            "https://cdn.xfce.org/about/screenshots/4.18-1.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Xubuntu_24.04_LTS_default_desktop_-_English.png",
        ],
    ),
    (
        "mate",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Ubuntu_MATE_24.04_LTS_default_desktop_-_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Ubuntu_MATE_24.04_LTS_menu_-_English.png",
            "https://mate-desktop.org/screens/screenshot.jpg",
        ],
    ),
    (
        "gnome",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Fedora_Workstation_39_Desktop_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Fedora_35_GNOME_Activities_Menu_en.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Fedora_41_With_Gnome_47.png",
        ],
    ),
    (
        "kde",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/KDE_neon_6_Breeze_Desktop_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Arco_Linux_24.05.11_Plasma_menu_-_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Arco_Linux_24.05.11_Plasma_Dolphin_24.02.2_Breeze_-_English.png",
        ],
    ),
    (
        "budgie",
        &[
            "https://buddiesofbudgie.org/images/blog/2023/01/MainDesktop.jpg",
            "https://buddiesofbudgie.org/images/blog/2023/01/BudgieMenu.png",
            "https://buddiesofbudgie.org/images/blog/2023/01/Raven-WidgetView.jpg",
        ],
    ),
    (
        "pantheon",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Elementary_OS_8.0_default_desktop_-_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Elementary_OS_8.0_menu_1_-_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Elementary_OS_8.0_Settings_-_English.png",
        ],
    ),
    (
        "deepin",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Deepin_Linux_23.1_default_desktop_-_English.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Deepin_Linux_25_Alpha_default_desktop_-_English.png",
        ],
    ),
    (
        "cosmic",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Pop!_OS_24.04_Alpha_7_default_desktop_-_English.png",
            "https://cdn11.bigcommerce.com/s-pywjnxrcr2/images/stencil/original/image-manager/cosmic-screensr20.png",
            "https://cdn11.bigcommerce.com/s-pywjnxrcr2/images/stencil/original/image-manager/themingr5.png",
        ],
    ),
    // Hyprland: the first screenshot is the Tokyo Night theme from Omarchy (Commons, GPLv2+,
    // author David Heinemeier Hansson). The former `Hyprland_screen.png` showed
    // the Russian Wikipedia across half the screen.
    (
        "hyprland",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Omarchy_Tokyo_Night_theme.png",
            "https://github.com/YAWTUUYAS/hyprland-riced/raw/main/screenshots/desktop.png",
            "https://github.com/YAWTUUYAS/hyprland-riced/raw/main/screenshots/wofi.png",
        ],
    ),
    (
        "lxqt",
        &[
            "https://lxqt-project.org/images/screenshots/light.png",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Lubuntu_24.04_LTS_default_desktop_-_English.jpg",
            "https://lxqt-project.org/images/screenshots/pcmanfm-qt.png",
        ],
    ),
    // niri: the second screenshot is from the project wiki (GPL-3.0); the Commons screenshot had
    // a German interface.
    (
        "niri",
        &[
            "https://github.com/user-attachments/assets/dea5909e-1859-4aaa-9d88-d37f9663e00b",
            "https://raw.githubusercontent.com/niri-wm/niri/main/docs/wiki/img/blur.png",
        ],
    ),
    (
        "sway",
        &[
            "https://commons.wikimedia.org/wiki/Special:FilePath/Sway_1.4_Window_Manager.png?width=1200",
            "https://commons.wikimedia.org/wiki/Special:FilePath/Sway_window_manager_on_two_displays.jpg?width=1200",
        ],
    ),
    // River: neither river-classic nor Wikimedia Commons has a screenshot
    // (Commons has only the logo), so there is no line: River screenshots are
    // our own, shipped in the package (`BUNDLED_SUBDIR`), not downloaded.
    // Wayfire: the wf-shell gallery (MIT; the default panel and background are exactly the
    // look Rexon installs) and effect screenshots from the official Wayfire wiki
    // (desktop cube, window switcher). Commons has only the logo.
    (
        "wayfire",
        &[
            "https://raw.githubusercontent.com/WayfireWM/wf-shell/master/gallery/default.png",
            "https://raw.githubusercontent.com/WayfireWM/wf-shell/master/gallery/legacy.png",
            "https://raw.githubusercontent.com/WayfireWM/wayfire-wiki/master/images/cube-normal.png",
            "https://raw.githubusercontent.com/WayfireWM/wayfire-wiki/master/images/switcher.png",
        ],
    ),
];

/// All screenshot URLs of a DE (`get_online_screenshot_urls` = `list(...)`).
pub fn online_urls(de: &str) -> Vec<&'static str> {
    SCREENSHOT_URLS
        .iter()
        .find(|(k, _)| *k == de)
        .map(|(_, v)| v.to_vec())
        .unwrap_or_default()
}

/// The first URL of each DE with a non-empty list (`OFFICIAL_SCREENSHOT_URLS`).
pub fn official_screenshot_urls() -> Vec<(&'static str, &'static str)> {
    SCREENSHOT_URLS
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (*k, v[0]))
        .collect()
}

// ── Wikimedia API: resolving the real CDN URL (`_resolve_via_api`) ─────────

/// User-Agent of requests to Wikimedia (`ua_bot` in `download_screenshot_from_url`).
/// Wikimedia requires an informative UA and cuts anonymous downloads.
pub const WIKIMEDIA_USER_AGENT: &str = "Rexon/5.0 (Linux DE Manager; \
https://github.com/rexon-app/rexon; rexon@proton.me) Python/urllib";

/// URL of the `imageinfo` request to the Commons API — port of `api_url` from `_resolve_via_api`.
/// `iiurlwidth=800` asks for a thumbnail of the needed width (`thumburl` in the reply).
pub fn wikimedia_api_url(filename: &str) -> String {
    format!(
        "https://commons.wikimedia.org/w/api.php?action=query&titles=File:{}\
&prop=imageinfo&iiprop=url&iiurlwidth=800&format=json&formatversion=2",
        url_quote(filename)
    )
}

/// The real CDN URL of the picture from the `imageinfo` reply — port of the parsing in
/// `_resolve_via_api`: `thumburl` (800px thumbnail), otherwise `url` (the original),
/// otherwise `None`.
///
/// Also `None` on any «wrong» reply: no `query.pages`, the page is marked
/// `missing` (the file is not on Commons), `imageinfo` is absent or empty.
///
/// ⚠️ Fragile spots of the original are fixed (`pages[0].get("imageinfo", [{}])[0]`):
/// an empty `imageinfo` list gave `IndexError`, and a reply in the old
/// `formatversion=1` (where `pages` is an object, not a list) gave `KeyError`. Both
/// exceptions were swallowed by an outer `except`, so «no file» could not be told apart from
/// «parsing broke». Here both cases are an honest `None`.
pub fn parse_wikimedia_imageinfo(data: &Value) -> Option<String> {
    let pages = data.get("query")?.get("pages")?;
    // formatversion=2 → array; formatversion=1 → object {pageid: page}.
    let page = match pages {
        Value::Array(a) => a.first()?,
        Value::Object(o) => o.values().next()?,
        _ => return None,
    };
    // `missing` is checked for TRUTHINESS, as in Python: formatversion=2 sets
    // it only on missing files, but an explicit `"missing": false` must also
    // read as «the file exists».
    if json_truthy(page.get("missing")) {
        return None;
    }
    let ii = page.get("imageinfo")?.as_array()?.first()?;
    for key in ["thumburl", "url"] {
        if let Some(u) = ii.get(key).and_then(Value::as_str) {
            if !u.is_empty() {
                return Some(u.to_string());
            }
        }
    }
    None
}

// ── Checking the downloaded reply (`_download_url`) ────────────────────────

/// Minimum size of an accepted picture in bytes (`len(data) < 512`).
pub const MIN_DOWNLOAD_BYTES: usize = 512;
/// Number of download attempts on HTTP 429 (`retries=3`).
pub const DOWNLOAD_RETRIES: u32 = 3;
/// Message after attempts are exhausted on 429.
pub fn rate_limit_msg() -> String {
    tr("Too many requests (429)")
}

/// Whether the reply is fit as a picture — port of the checks of `_download_url`:
/// `Content-Type` must start with `image/`, and the body must be no smaller than
/// [`MIN_DOWNLOAD_BYTES`]. `Err` — the reason text exactly as in the original (it
/// goes to the UI and the log).
///
/// Cuts off the main practical «wrong reply» case: an HTML error page
/// (403/404/captcha) instead of a picture — it has `Content-Type: text/html`.
pub fn validate_image_response(content_type: Option<&str>, body_len: usize) -> Result<(), String> {
    let ct = content_type.unwrap_or("");
    if !ct.starts_with("image/") {
        // TRANSLATORS: download check failed; %1 is the content type the server sent.
        return Err(trf("Not an image: %1", &[&ct]));
    }
    if body_len < MIN_DOWNLOAD_BYTES {
        return Err(tr("The file is too small"));
    }
    Ok(())
}

/// Pause before a retry on HTTP 429 — port of `wait = (attempt + 1) * 3`
/// (`attempt` counts from zero).
///
/// The value is no longer local: it comes from the SHARED retry mechanism
/// ([`crate::net::retry_delay_ms`]), where the same 3 s / 6 s pair is pinned by
/// the constant [`crate::net::RATE_LIMIT_DELAY_MS`]. The function is kept for
/// comparison with the original — one rule, no more copies.
pub fn retry_wait_secs(attempt: u32) -> u64 {
    let rate_limited = NetError::Status { code: 429, reason: String::new() };
    net::retry_delay_ms(attempt + 1, &rate_limited) / 1000
}

// ── Downloading a screenshot (`download_screenshot_from_url`) ──────────────
//
// The ported part used to end here: Python did the HTTP itself
// (`urllib`) with its own retry loop for HTTP 429 alone. A dropped
// connection — the same class of failure that killed catalog previews —
// was not retried at all. Now the request goes through the core's shared network layer
// (`net.rs`), so screenshot retries are exactly the same as for the theme catalog
// (rule #12), and no Python copy of the urllib logic remains.

/// Timeout for downloading the picture itself — port of `urlopen(req, timeout=20)`.
pub const DOWNLOAD_TIMEOUT_SECS: u64 = 20;
/// Timeout of the Commons API request — port of `urlopen(req, timeout=15)`.
pub const API_TIMEOUT_SECS: u64 = 15;

/// The failure reason in human text — what the original returns as the second
/// tuple element and shows in the progress line.
fn why(e: &NetError) -> String {
    match e {
        // The response code is printed as in the original (`f"HTTP {e.code}"`), except
        // 429: after retries are exhausted it means exactly «rate limit».
        NetError::Status { code: 429, .. } => rate_limit_msg(),
        NetError::Status { code, .. } => format!("HTTP {code}"),
        // The original's raw `str(e.reason)` («[Errno -2] Name or service not
        // known») tells the user nothing — rule #13.
        NetError::Transport { .. } => e.human(),
    }
}

/// Download a picture from one URL — port of `_download_url`.
///
/// `Ok((bytes, Content-Type))` or the failure reason. The reply type is checked BEFORE
/// reading the body, as in the original: an HTML error page can be megabytes.
/// Retries are shared ([`net::with_retry`]): dropped connection, timeout, 5xx and 429; «no
/// file» (4xx) and «not an image» are not retried.
pub fn download_image<IO: NetIo + ?Sized>(
    io: &mut IO,
    url: &str,
    user_agent: &str,
) -> Result<(Vec<u8>, String), String> {
    let what = format!("Снимок рабочего стола {url}");
    // The outer layer is a network failure (`with_retry` decides to retry it),
    // the inner one is «the reply arrived but it is the wrong one»: that is NOT a network failure, a second
    // request returns the same page, so such a refusal is final.
    let res = net::with_retry(io, net::Retry::IDEMPOTENT, &what, |io| {
        io.open(url, user_agent, DOWNLOAD_TIMEOUT_SECS)?;
        let ctype = io.content_type();
        // We pass a length that is surely large enough: this step checks
        // ONLY the type — there is no body yet and no reason to download it.
        if let Err(msg) = validate_image_response(Some(&ctype), MIN_DOWNLOAD_BYTES) {
            io.close();
            return Ok(Err(msg));
        }
        let mut body = Vec::new();
        loop {
            match io.read(DOWNLOAD_CHUNK) {
                Ok(c) if c.is_empty() => break,
                Ok(c) => body.extend_from_slice(&c),
                Err(e) => {
                    io.close();
                    return Err(e);
                }
            }
        }
        io.close();
        Ok(validate_image_response(Some(&ctype), body.len()).map(|()| (body, ctype)))
    });
    match res {
        Ok(inner) => inner,
        Err(e) => Err(why(&e)),
    }
}

/// Read chunk of a screenshot body. The same size as in the theme catalog.
const DOWNLOAD_CHUNK: usize = 64 * 1024;

/// Download a screenshot of desktop `de` — port of `download_screenshot_from_url`
/// in full: the direct URL, on failure — resolving the real CDN address via
/// the Commons API and a second attempt with it.
///
/// `Ok((bytes, Content-Type))` — what to write to disk (the path is computed by
/// [`save_dest`]); `Err` — a ready string for the user.
pub fn download_screenshot<IO: NetIo + ?Sized>(
    io: &mut IO,
    de: &str,
    url: &str,
) -> Result<(Vec<u8>, String), String> {
    let direct = match download_image(io, url, WIKIMEDIA_USER_AGENT) {
        Ok(got) => return Ok(got),
        Err(info) => info,
    };
    io.log(
        LogLevel::Info,
        &format!("Прямой URL не сработал ({direct}), пробуем API…"),
    );
    let Some(filename) = filename_for_de(de) else {
        return Err(trf("Couldn't download the screenshot: %1", &[&direct]));
    };
    let api = wikimedia_api_url(filename);
    let cdn = net::with_retry(io, net::Retry::IDEMPOTENT, &format!("Commons API {api}"), |io| {
        io.get(&api, WIKIMEDIA_USER_AGENT, API_TIMEOUT_SECS)
    })
    .ok()
    .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
    .as_ref()
    .and_then(parse_wikimedia_imageinfo);
    let Some(cdn) = cdn else {
        // In this branch the original reports the reason of the FIRST failure, not of
        // the API failure: what matters to the user is that there is nowhere to take the screenshot from.
        return Err(trf("Couldn't download the screenshot: %1", &[&direct]));
    };
    download_image(io, &cdn, WIKIMEDIA_USER_AGENT).map_err(|info| format!("CDN: {info}"))
}

// ── Auto-download plan (`auto_download_all`) ───────────────────────────────

/// One auto-download task: which DE it belongs to, under which tag to
/// save (`{de}` for the first URL, `{de}_{idx}` for the rest) and what to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDownloadTask {
    pub de: String,
    pub tag: String,
    pub url: String,
}

/// Auto-download plan: what to download and which DEs already have everything in place.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoDownloadPlan {
    pub tasks: Vec<AutoDownloadTask>,
    /// `(de, how many files already exist)` — for these the original immediately writes
    /// «already present (N items)» and sets the result to `True` without downloading anything.
    pub already_complete: Vec<(String, usize)>,
}

/// File tag for the i-th URL of a DE's list — port of `f"{de_name}{suffix}"`,
/// where `suffix = "" if idx == 0 else f"_{idx}"`.
pub fn auto_download_tag(de: &str, idx: usize) -> String {
    if idx == 0 {
        de.to_string()
    } else {
        format!("{de}_{idx}")
    }
}

/// Path candidates for an auto-download tag — `{dir}/{tag}{.png,.jpg,.webp}`.
/// The executor checks their existence and size (`> MIN_SIZE`).
pub fn tag_candidates(dir: &str, tag: &str) -> Vec<String> {
    PATH_EXTS.iter().map(|ext| format!("{dir}/{tag}{ext}")).collect()
}

/// Auto-download plan — port of building `tasks` in `auto_download_all`.
///
/// The FS is not touched: `existing_count(de)` gives the number of the DE's screenshots already found
/// (port of `len(get_screenshot_paths(de))`), `tag_exists(tag)` — whether a fit
/// file exists under this tag (port of iterating [`tag_candidates`] with the size filter).
/// DEs are traversed in [`SCREENSHOT_URLS`] order, as Python does over the dict.
pub fn auto_download_plan<C, E>(existing_count: C, tag_exists: E) -> AutoDownloadPlan
where
    C: Fn(&str) -> usize,
    E: Fn(&str) -> bool,
{
    let mut plan = AutoDownloadPlan::default();
    for (de, urls) in SCREENSHOT_URLS {
        let have = existing_count(de);
        // No fewer screenshots than URLs → everything is downloaded for this DE.
        if have >= urls.len() {
            plan.already_complete.push(((*de).to_string(), have));
            continue;
        }
        for (idx, url) in urls.iter().enumerate() {
            let tag = auto_download_tag(de, idx);
            if tag_exists(&tag) {
                continue;
            }
            plan.tasks.push(AutoDownloadTask {
                de: (*de).to_string(),
                tag,
                url: (*url).to_string(),
            });
        }
    }
    plan
}

// ── Live auto-download executor (`auto_download_all`) ──────────────────────
//
// Until now the core held only the download PLAN, and the Python host executed it
// (`DEScreenshotManager.auto_download_all`, the tab's QThread worker). The Rust window
// does not call Python, and on a fresh machine the user would see only
// placeholders in the cards — forever. The executor moved here in full: one copy for both
// windows (rule #12), the directory, tags and file names are exactly the same as
// Python's, so the two windows share the cache.

/// Whether a screenshot file is fit: it exists and is larger than [`MIN_SIZE`] — port of
/// `os.path.exists(p) and os.path.getsize(p) > 100`.
pub fn usable_file(path: &str) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.len() > MIN_SIZE)
        .unwrap_or(false)
}

/// The desktop's screenshots on disk in carousel order — port of `get_screenshot_paths`:
/// the main one (the first fit candidate), then `_1`…`_5`, by the first fit
/// extension per index.
pub fn screenshot_paths_on_disk(dir: &str, de: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(main) = screenshot_path_candidates(dir, de)
        .into_iter()
        .find(|p| usable_file(p))
    {
        out.push(main);
    }
    let extra = extra_candidates(dir, de);
    for group in extra.chunks(PATH_EXTS.len()) {
        if let Some(found) = group.iter().find(|p| usable_file(p)) {
            out.push(found.clone());
        }
    }
    out
}

// ── Shipped screenshots ────────────────────────────────────────────────────
//
// Where neither the project nor Wikimedia Commons has a desktop screenshot (River), Rexon
// ships its own screenshots in the package — in the product data directory
// (`data/assets/screenshots/` in the tree, `/usr/lib/rexon/rexon/assets/
// screenshots/` in the package). The names are the same as for downloaded ones: `{de}.png`,
// `{de}_1.png`…, so the search is shared ([`screenshot_paths_on_disk`]).

/// Subdirectory of shipped screenshots inside the Rexon data directory.
pub const BUNDLED_SUBDIR: &str = "assets/screenshots";

/// Directories of shipped screenshots in decreasing precision — the same layouts as
/// for the product data (`installer::package_data_dir_candidates`: the developer
/// tree, the package, a system install and a home-folder install).
pub fn bundled_screenshot_dirs(repo_root: &str, home: &str) -> Vec<String> {
    crate::installer::package_data_dir_candidates(repo_root, home)
        .into_iter()
        .map(|dir| format!("{dir}/{BUNDLED_SUBDIR}"))
        .collect()
}

/// Desktop screenshots for the carousel: shipped ones (from the first directory where the desktop
/// has them) come first, followed by those downloaded into `dir`. Empty — show
/// the placeholder ([`placeholder_on_disk`]).
pub fn carousel_paths_on_disk(dir: &str, bundled_dirs: &[String], de: &str) -> Vec<String> {
    let mut out = bundled_dirs
        .iter()
        .map(|b| screenshot_paths_on_disk(b, de))
        .find(|found| !found.is_empty())
        .unwrap_or_default();
    out.extend(screenshot_paths_on_disk(dir, de));
    out
}

// ── Changing a screenshot source ───────────────────────────────────────────
//
// The cache lays out screenshots by tag (`hyprland.png`, `xfce_2.png`), not by
// address, and a warm cache is not downloaded again. So a screenshot replaced in
// [`SCREENSHOT_URLS`] would stay with the user forever without a special measure.
// The measure is an inventory next to the screenshots: tag → the address the file was downloaded from. A tag whose
// address in the table changed counts as outdated: its files are deleted, and
// the download fetches the screenshot again.

/// Inventory of downloaded screenshot sources: lines `tag<TAB>address`.
pub const SOURCES_FILE: &str = ".sources";

/// Tags whose source changed BEFORE the inventory appeared: files with these tags
/// that are not in the inventory were downloaded from the old address. Other files without an inventory entry
/// are accepted as downloaded from the current address.
///
/// 08.11.2026: `hyprland` — the screenshot had the Russian Wikipedia open;
/// `xfce_2` — the Xfce interface in Spanish; `niri_1` — in German.
pub const REPLACED_BEFORE_SOURCES: &[&str] = &["hyprland", "xfce_2", "niri_1"];

/// Parse the inventory: `(tag, address)` in line order; malformed lines are skipped.
pub fn parse_sources(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(tag, url)| !tag.is_empty() && !url.is_empty())
        .map(|(tag, url)| (tag.to_string(), url.to_string()))
        .collect()
}

/// Inventory text from `(tag, address)` pairs.
pub fn render_sources(entries: &[(String, String)]) -> String {
    entries.iter().map(|(tag, url)| format!("{tag}\t{url}\n")).collect()
}

/// Which tags are outdated — a decision without the FS. `have_file(tag)` — whether a fit
/// file lies under the tag; `recorded` — the inventory. Returns the outdated tags and the inventory
/// supplemented with accepted files that had no record.
pub fn retired_tags<H>(recorded: &[(String, String)], have_file: H) -> (Vec<String>, Vec<(String, String)>)
where
    H: Fn(&str) -> bool,
{
    let mut retired = Vec::new();
    let mut sources: Vec<(String, String)> = Vec::new();
    for (de, urls) in SCREENSHOT_URLS {
        for (idx, url) in urls.iter().enumerate() {
            let tag = auto_download_tag(de, idx);
            if !have_file(&tag) {
                continue;
            }
            let stale = match recorded.iter().find(|(t, _)| *t == tag) {
                Some((_, was)) => was != url,
                None => REPLACED_BEFORE_SOURCES.contains(&tag.as_str()),
            };
            if stale {
                retired.push(tag);
            } else {
                sources.push((tag, (*url).to_string()));
            }
        }
    }
    (retired, sources)
}

/// Remove from the `dir` cache the screenshots whose source changed in the table, and
/// rewrite the inventory. Returns the removed tags.
pub fn drop_retired_on_disk(dir: &str) -> Vec<String> {
    let path = format!("{dir}/{SOURCES_FILE}");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let recorded = parse_sources(&text);
    let (retired, sources) =
        retired_tags(&recorded, |tag| tag_candidates(dir, tag).iter().any(|p| usable_file(p)));
    for tag in &retired {
        for p in tag_candidates(dir, tag) {
            let _ = std::fs::remove_file(p);
        }
    }
    let fresh = render_sources(&sources);
    if fresh != text && (!fresh.is_empty() || !text.is_empty()) {
        let _ = crate::utils::atomic_write(&path, &fresh);
    }
    retired
}

/// Write the addresses of just downloaded screenshots into the `dir` inventory.
fn record_sources(dir: &str, saved: &[(String, String)]) {
    if saved.is_empty() {
        return;
    }
    let path = format!("{dir}/{SOURCES_FILE}");
    let mut entries = parse_sources(&std::fs::read_to_string(&path).unwrap_or_default());
    for (tag, url) in saved {
        entries.retain(|(t, _)| t != tag);
        entries.push((tag.clone(), url.clone()));
    }
    let _ = crate::utils::atomic_write(&path, &render_sources(&entries));
}

/// Auto-download plan over the real `dir` directory — the same observations that
/// the original's `auto_download_all` gathers. Screenshots with a changed source
/// are removed beforehand ([`drop_retired_on_disk`]) and enter the plan again.
pub fn auto_download_plan_on_disk(dir: &str) -> AutoDownloadPlan {
    drop_retired_on_disk(dir);
    auto_download_plan(
        |de| screenshot_paths_on_disk(dir, de).len(),
        |tag| tag_candidates(dir, tag).iter().any(|p| usable_file(p)),
    )
}

/// How many screenshots are downloaded at once — `ThreadPoolExecutor(max_workers=4)`.
pub const AUTO_DOWNLOAD_WORKERS: usize = 4;

/// Result of one auto-download task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDownloadOutcome {
    pub de: String,
    pub tag: String,
    /// Path of the written file or the failure reason in human text.
    pub result: Result<String, String>,
    /// This is the last task of its desktop: its whole batch has finished.
    pub de_finished: bool,
    /// How many of the desktop's files have been downloaded by now (including this task).
    pub de_downloaded: usize,
}

/// Write the downloaded bytes — port of `_save_screenshot_data`: the path by
/// Content-Type is computed by [`save_dest`].
///
/// ⚠️ Fix of the original (rule #16): Python wrote straight into the final file, and
/// an interrupted write (the window closed mid-download) left a stub larger than
/// 100 bytes — it was taken for a screenshot and never downloaded again, while the carousel
/// showed a broken picture. Here the bytes go through the shared door
/// [`crate::utils::atomic_write_bytes`]: into a temporary file next to it and into place in one
/// step.
pub fn save_screenshot(dir: &str, tag: &str, data: &[u8], content_type: &str) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| trf("Write error: %1", &[&e]))?;
    let dest = save_dest(dir, tag, Some(content_type));
    crate::utils::atomic_write_bytes(&dest, data).map_err(|e| trf("Write error: %1", &[&e]))?;
    Ok(dest)
}

/// Execute the auto-download plan — port of `auto_download_all` in full.
///
/// The transport is provided by `make_io` (one per worker thread: connections
/// are reused within a batch); `on_task` is called for every finished
/// task IN THE CALLER'S THREAD — it does not need to be `Sync`, and the window can
/// redraw the card right away. Returns `(de, whether it has screenshots)` in
/// [`SCREENSHOT_URLS`] order — the original's `results` dict. No network — tasks simply
/// end with a refusal; panics and errors do not leak out.
pub fn run_auto_download<IO, F, R>(dir: &str, make_io: F, on_task: R) -> Vec<(String, bool)>
where
    IO: NetIo,
    F: Fn() -> IO + Sync,
    R: FnMut(&AutoDownloadOutcome),
{
    run_plan(dir, auto_download_plan_on_disk(dir), make_io, on_task)
}

/// Execute an already built plan — the body of [`run_auto_download`].
fn run_plan<IO, F, R>(dir: &str, plan: AutoDownloadPlan, make_io: F, mut on_task: R) -> Vec<(String, bool)>
where
    IO: NetIo,
    F: Fn() -> IO + Sync,
    R: FnMut(&AutoDownloadOutcome),
{
    // `(de, downloaded, finished, total)` — the original's `de_counts` plus a count of
    // finished ones, so the window knows when the desktop's batch is over.
    let mut downloaded: Vec<(String, usize, usize, usize)> = Vec::new();
    for task in &plan.tasks {
        match downloaded.iter_mut().find(|(de, ..)| *de == task.de) {
            Some(slot) => slot.3 += 1,
            None => downloaded.push((task.de.clone(), 0, 0, 1)),
        }
    }
    if !plan.tasks.is_empty() {
        let next = std::sync::atomic::AtomicUsize::new(0);
        let (tx, rx) = std::sync::mpsc::channel::<AutoDownloadOutcome>();
        std::thread::scope(|scope| {
            let workers = AUTO_DOWNLOAD_WORKERS.min(plan.tasks.len());
            for _ in 0..workers {
                let tx = tx.clone();
                let (next, tasks, make_io) = (&next, &plan.tasks, &make_io);
                scope.spawn(move || {
                    let mut io = make_io();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(task) = tasks.get(i) else { break };
                        let result = download_screenshot(&mut io, &task.tag, &task.url)
                            .and_then(|(body, ctype)| save_screenshot(dir, &task.tag, &body, &ctype));
                        let outcome = AutoDownloadOutcome {
                            de: task.de.clone(),
                            tag: task.tag.clone(),
                            result,
                            de_finished: false,
                            de_downloaded: 0,
                        };
                        if tx.send(outcome).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(tx);
            for mut outcome in rx {
                // The inventory is written right away: if the window is closed mid-batch, what is already
                // downloaded will not be considered outdated.
                if outcome.result.is_ok() {
                    if let Some(task) = plan.tasks.iter().find(|t| t.tag == outcome.tag) {
                        record_sources(dir, &[(task.tag.clone(), task.url.clone())]);
                    }
                }
                if let Some(slot) = downloaded.iter_mut().find(|(de, ..)| *de == outcome.de) {
                    slot.1 += usize::from(outcome.result.is_ok());
                    slot.2 += 1;
                    outcome.de_downloaded = slot.1;
                    outcome.de_finished = slot.2 == slot.3;
                }
                on_task(&outcome);
            }
        });
    }
    let mut results = Vec::new();
    for (de, _) in SCREENSHOT_URLS {
        if plan.already_complete.iter().any(|(d, _)| d == de) {
            results.push(((*de).to_string(), true));
        } else if let Some((_, n, ..)) = downloaded.iter().find(|(d, ..)| d == de) {
            results.push(((*de).to_string(), *n > 0));
        }
    }
    results
}

/// [`run_auto_download`] on the core's production transport (`net::UreqNet`).
pub fn auto_download_all_live<R>(dir: &str, on_task: R) -> Vec<(String, bool)>
where
    R: FnMut(&AutoDownloadOutcome),
{
    run_auto_download(dir, net::UreqNet::new, on_task)
}

/// This process's live download: `Some(desktops waiting for their batch)` while it
/// runs. The placeholder uses it to decide whether to promise «loading».
static LIVE_RUN: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

/// Placeholder state of a desktop according to the process's live download.
pub fn live_placeholder_state(de: &str) -> PlaceholderState {
    let pending = LIVE_RUN
        .lock()
        .map(|run| run.as_ref().is_some_and(|p| p.iter().any(|d| d == de)))
        .unwrap_or(false);
    placeholder_state(de, pending)
}

/// The desktop's placeholder on disk for the current state: the file is written if it is missing
/// or its caption is outdated. `None` — the directory is not writable.
pub fn placeholder_on_disk(dir: &str, de: &str) -> Option<String> {
    placeholder_on_disk_for(dir, de, live_placeholder_state(de))
}

/// The desktop's placeholder on disk for a GIVEN state. For an executor that
/// runs the download itself and knows whether the desktop is waiting for its batch (the Python
/// widget tab via the bridge): it gets the state from [`placeholder_state`], while the
/// file name, caption and writing come from here, the same as for the Rust page.
pub fn placeholder_on_disk_for(dir: &str, de: &str, state: PlaceholderState) -> Option<String> {
    let path = format!("{dir}/{}", placeholder_name_for(de, state));
    let svg = generate_svg_for(de, state);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(svg.as_str()) {
        std::fs::create_dir_all(dir).ok()?;
        crate::utils::atomic_write(&path, &svg).ok()?;
    }
    Some(path)
}

/// Start auto-download in its own thread and track placeholder state with it.
/// Desktops that have something to download are marked «waiting» BEFORE returning
/// — the very first card will honestly say «loading». A desktop is taken off
/// waiting before `on_task` is called with `de_finished`, so a window
/// that redraws on it already sees the final caption.
///
/// One download per process: while the first is running, a second does not start (`None`) —
/// otherwise two pages would write into the same files. Once it is over — it can run again.
pub fn start_auto_download<IO, F, R>(
    dir: String,
    make_io: F,
    mut on_task: R,
) -> Option<std::thread::JoinHandle<()>>
where
    IO: NetIo,
    F: Fn() -> IO + Send + Sync + 'static,
    R: FnMut(&AutoDownloadOutcome) + Send + 'static,
{
    let plan = auto_download_plan_on_disk(&dir);
    {
        let mut run = LIVE_RUN.lock().ok()?;
        if run.is_some() {
            return None;
        }
        let mut pending: Vec<String> = Vec::new();
        for task in &plan.tasks {
            if !pending.contains(&task.de) {
                pending.push(task.de.clone());
            }
        }
        *run = Some(pending);
    }
    let finish = || {
        if let Ok(mut run) = LIVE_RUN.lock() {
            *run = None;
        }
    };
    let spawned = std::thread::Builder::new()
        .name("rexon-scr".into())
        .spawn(move || {
            run_plan(&dir, plan, make_io, |outcome| {
                if outcome.de_finished {
                    if let Ok(mut run) = LIVE_RUN.lock() {
                        if let Some(pending) = run.as_mut() {
                            pending.retain(|d| *d != outcome.de);
                        }
                    }
                }
                on_task(outcome);
            });
            finish();
        });
    match spawned {
        Ok(handle) => Some(handle),
        Err(_) => {
            finish();
            None
        }
    }
}

/// [`start_auto_download`] on the core's production transport. `false` — a download is already
/// running or the thread did not come up.
pub fn start_auto_download_live<R>(dir: &str, on_task: R) -> bool
where
    R: FnMut(&AutoDownloadOutcome) + Send + 'static,
{
    start_auto_download(dir.to_string(), net::UreqNet::new, on_task).is_some()
}

/// Python truthiness of a JSON value: null/missing/`false`/0/""/empty
/// list/empty object → `false`.
fn json_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Percent-encode like `urllib.parse.quote(s)` — safe = ALPHA/DIGIT/`_.-~/`.
/// A local copy following the neighbouring core modules (`aur.rs`, `ocs.rs`):
/// there is no shared url helper in `utils` yet.
fn url_quote(s: &str) -> String {
    fn safe(b: u8) -> bool {
        b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~' | b'/')
    }
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if safe(b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

/// URL of the first DE screenshot (`get_online_screenshot_url`): first the direct
/// `SCREENSHOT_URLS[0]`, otherwise Wikimedia `Special:FilePath/{fname}?width=800`,
/// otherwise an empty string. Quirk kept: returns `""` (not None) when missing.
pub fn online_url(de: &str) -> String {
    let urls = online_urls(de);
    if let Some(first) = urls.first() {
        return (*first).to_string();
    }
    match filename_for_de(de) {
        Some(fname) => {
            format!("https://commons.wikimedia.org/wiki/Special:FilePath/{fname}?width=800")
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod live_download_tests {
    use super::*;

    /// Fake network: xfce screenshots are served as a PNG picture, everything else is
    /// «no network» (a dropped connection on every request, the Commons API too).
    #[derive(Default)]
    struct FakeNet {
        body: Vec<u8>,
        ctype: String,
    }

    fn offline() -> NetError {
        NetError::Transport { detail: "сети нет".into() }
    }

    impl NetIo for FakeNet {
        fn get(&mut self, _: &str, _: &str, _: u64) -> Result<Vec<u8>, NetError> {
            Err(offline())
        }
        fn open(&mut self, url: &str, _: &str, _: u64) -> Result<i64, NetError> {
            // xfce addresses come from the table: the third one (Xubuntu from Commons) does not
            // contain the word «xfce».
            if !online_urls("xfce").contains(&url) {
                return Err(offline());
            }
            self.body = vec![0x89; 2048];
            self.ctype = if url.ends_with(".png") { "image/png" } else { "image/jpeg" }.into();
            Ok(self.body.len() as i64)
        }
        fn read(&mut self, _: usize) -> Result<Vec<u8>, NetError> {
            Ok(std::mem::take(&mut self.body))
        }
        fn close(&mut self) {}
        fn makedirs(&mut self, _: &str) {}
        fn create(&mut self, _: &str) -> Result<(), NetError> {
            Ok(())
        }
        fn write(&mut self, _: &[u8]) -> Result<(), NetError> {
            Ok(())
        }
        fn close_file(&mut self) {}
        fn exists(&mut self, _: &str) -> bool {
            false
        }
        fn remove(&mut self, _: &str) {}
        fn progress(&mut self, _: i64, _: i64) {}
        fn log(&mut self, _: LogLevel, _: &str) {}
        fn sleep(&mut self, _: u64) {}
        fn content_type(&mut self) -> String {
            self.ctype.clone()
        }
    }

    fn scratch(tag: &str) -> String {
        let dir = std::env::temp_dir().join(format!("rexon_scr_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn empty_cache_fills_with_python_names_and_offline_is_quiet() {
        let dir = scratch("fill");
        let mut outcomes = Vec::new();
        let results = run_auto_download(&dir, FakeNet::default, |o| outcomes.push(o.clone()));

        // The names are the same as Python writes: the tag `{de}`/`{de}_{idx}` plus
        // the extension by Content-Type. No temporary `.part` files remain.
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(files, [SOURCES_FILE, "xfce.png", "xfce_1.png", "xfce_2.png"]);
        // The inventory remembers where every file came from.
        let recorded = parse_sources(&std::fs::read_to_string(format!("{dir}/{SOURCES_FILE}")).unwrap());
        assert_eq!(recorded.len(), 3);
        for (idx, url) in online_urls("xfce").iter().enumerate() {
            assert!(recorded.contains(&(auto_download_tag("xfce", idx), (*url).to_string())));
        }
        assert_eq!(screenshot_paths_on_disk(&dir, "xfce").len(), 3);

        // The result is the original's `results` dict: xfce has screenshots, the
        // others do not, and no panic without a network.
        assert!(results.contains(&("xfce".to_string(), true)));
        assert!(results.contains(&("kde".to_string(), false)));
        assert_eq!(results.len(), SCREENSHOT_URLS.len());

        // Each desktop reports the end of its batch exactly once.
        for (de, urls) in SCREENSHOT_URLS {
            let mine: Vec<_> = outcomes.iter().filter(|o| o.de == *de).collect();
            assert_eq!(mine.len(), urls.len(), "{de}");
            assert_eq!(mine.iter().filter(|o| o.de_finished).count(), 1, "{de}");
        }
        let xfce_last = outcomes.iter().find(|o| o.de == "xfce" && o.de_finished).unwrap();
        assert_eq!(xfce_last.de_downloaded, 3);
        let kde_last = outcomes.iter().find(|o| o.de == "kde" && o.de_finished).unwrap();
        assert_eq!(kde_last.de_downloaded, 0);

        // Warm cache: xfce is not downloaded again.
        let plan = auto_download_plan_on_disk(&dir);
        assert!(plan.tasks.iter().all(|t| t.de != "xfce"));
        assert!(plan.already_complete.contains(&("xfce".to_string(), 3)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replaced_source_drops_cached_shot() {
        let dir = scratch("retire");
        std::fs::create_dir_all(&dir).unwrap();
        let big = vec![0x89u8; 2048];
        // Cache before the inventory: a Hyprland screenshot from the old address, kde is fit.
        std::fs::write(format!("{dir}/hyprland.png"), &big).unwrap();
        std::fs::write(format!("{dir}/kde.png"), &big).unwrap();
        // A screenshot whose address in the inventory does not match the table.
        std::fs::write(format!("{dir}/gnome.png"), &big).unwrap();
        std::fs::write(format!("{dir}/{SOURCES_FILE}"), "gnome\thttps://example.org/old.png\n").unwrap();

        let plan = auto_download_plan_on_disk(&dir);
        assert!(!std::path::Path::new(&format!("{dir}/hyprland.png")).exists());
        assert!(!std::path::Path::new(&format!("{dir}/gnome.png")).exists());
        assert!(std::path::Path::new(&format!("{dir}/kde.png")).exists());
        assert!(plan.tasks.iter().any(|t| t.tag == "hyprland"));
        assert!(plan.tasks.iter().any(|t| t.tag == "gnome"));
        assert!(!plan.tasks.iter().any(|t| t.tag == "kde"));

        // The accepted file is recorded in the inventory with the current address — a repeated
        // pass does not touch it.
        let recorded = parse_sources(&std::fs::read_to_string(format!("{dir}/{SOURCES_FILE}")).unwrap());
        assert_eq!(recorded, [("kde".to_string(), online_urls("kde")[0].to_string())]);
        assert!(drop_retired_on_disk(&dir).is_empty());
        assert!(std::path::Path::new(&format!("{dir}/kde.png")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn placeholder_caption_follows_download_state() {
        // The «loading» caption is the original's bytes, the others differ
        // only in the bottom line.
        let loading = generate_svg_for("kde", PlaceholderState::Loading);
        assert_eq!(loading, generate_svg("kde"));
        let offline = generate_svg_for("kde", PlaceholderState::NoInternet);
        assert_eq!(offline, loading.replace(CAPTION_LOADING, CAPTION_NO_INTERNET));
        assert!(placeholder_state("нет-такого", true) == PlaceholderState::NoScreenshot);

        let dir = scratch("live");
        // The transport gives out not a single byte until the test releases it: that way the
        // «download in progress» state is visible.
        static GO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let make_io = || {
            while !GO.load(std::sync::atomic::Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            FakeNet::default()
        };
        let finished = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = finished.clone();
        let handle = start_auto_download(dir.clone(), make_io, move |o| {
            if o.de_finished {
                // The desktop is already off waiting when the window gets the news.
                seen.lock().unwrap().push((o.de.clone(), live_placeholder_state(&o.de)));
            }
        })
        .expect("первая закачка стартует");

        assert_eq!(live_placeholder_state("kde"), PlaceholderState::Loading);
        let path = placeholder_on_disk(&dir, "kde").unwrap();
        assert!(path.ends_with("/kde_placeholder.svg"));
        assert!(std::fs::read_to_string(&path).unwrap().contains(CAPTION_LOADING));
        assert!(
            start_auto_download(dir.clone(), FakeNet::default, |_| {}).is_none(),
            "вторая закачка во время первой не стартует"
        );

        GO.store(true, std::sync::atomic::Ordering::Release);
        handle.join().unwrap();

        assert_eq!(live_placeholder_state("kde"), PlaceholderState::NoInternet);
        let path = placeholder_on_disk(&dir, "kde").unwrap();
        assert!(path.ends_with("/kde_placeholder_offline.svg"));
        assert!(std::fs::read_to_string(&path).unwrap().contains(CAPTION_NO_INTERNET));
        assert!(finished
            .lock()
            .unwrap()
            .contains(&("kde".to_string(), PlaceholderState::NoInternet)));
        assert_eq!(screenshot_paths_on_disk(&dir, "xfce").len(), 3);

        // A given state (the caller runs the download — the widget tab
        // via the bridge): a desktop without a source gets its own caption and its own file.
        let state = placeholder_state("нет-такого", false);
        let path = placeholder_on_disk_for(&dir, "нет-такого", state).unwrap();
        assert!(path.ends_with("/нет-такого_placeholder_none.svg"));
        assert!(std::fs::read_to_string(&path).unwrap().contains(CAPTION_NO_SCREENSHOT));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bundled_screenshots_come_first_then_downloaded() {
        let root = scratch("bundled");
        let cache = format!("{root}/cache");
        let missing = format!("{root}/missing");
        let bundled = format!("{root}/bundled");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::create_dir_all(&bundled).unwrap();
        let big = vec![0u8; 200];
        for name in ["river.png", "river_1.png", "river_2.jpg"] {
            std::fs::write(format!("{bundled}/{name}"), &big).unwrap();
        }
        std::fs::write(format!("{cache}/river_1.webp"), &big).unwrap();
        // A dummy does not count — the same size filter as for downloaded ones.
        std::fs::write(format!("{bundled}/river_3.png"), b"x").unwrap();
        let dirs = vec![missing, bundled.clone()];
        assert_eq!(
            carousel_paths_on_disk(&cache, &dirs, "river"),
            vec![
                format!("{bundled}/river.png"),
                format!("{bundled}/river_1.png"),
                format!("{bundled}/river_2.jpg"),
                format!("{cache}/river_1.webp"),
            ]
        );
        // A desktop without shipped ones — exactly the downloaded ones.
        assert!(carousel_paths_on_disk(&cache, &dirs, "kde").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_dirs_follow_product_data_layouts() {
        let dirs = bundled_screenshot_dirs("/src/rexon", "/home/u");
        assert_eq!(dirs[0], "/src/rexon/data/assets/screenshots");
        assert!(dirs.contains(&"/usr/lib/rexon/rexon/assets/screenshots".to_string()));
    }

    /// River has no screenshots from the project or on Commons — the tree must ship
    /// its own (otherwise the card will again show «no screenshot yet»), and Wayfire must
    /// have several addresses, like the other desktops.
    #[test]
    fn river_ships_own_screenshots_and_wayfire_has_several() {
        let data = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/assets/screenshots");
        let river = screenshot_paths_on_disk(data, "river");
        assert!(river.len() >= 3, "поставляемых снимков River: {river:?}");
        for path in &river {
            let size = std::fs::metadata(path).unwrap().len();
            assert!(size <= 500 * 1024, "{path}: {size} байт — слишком крупно для пакета");
        }
        assert!(online_urls("wayfire").len() >= 3);
    }
}
