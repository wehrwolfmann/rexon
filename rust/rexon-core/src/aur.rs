//! AUR RPC v5 client for the "Appearance" catalog: a port of
//! `rexon/appearance/aur_client.py` IN FULL, both response parsing and the
//! network layer.
//!
//! Parsing: RPC JSON response -> models (`parse`), computed package
//! attributes (`id`/`aur_url`/`preview_url`/`icon`), RPC error check,
//! sorting by votes and orchestration over an ALREADY received response:
//! the multi-word search filter (`search_filter`) and the category
//! dedup+pin (`dedup_category`).
//!
//! Network: [`http_json`] (a port of `_http_json`) plus the URL builders and
//! [`fetch_curated`]/[`fetch_for_category`]/[`search_aur`] themselves. The
//! call is hidden behind the [`NetIo`] trait (`net.rs`): it is the only way
//! to test the branches at all: `aur.archlinux.org` is unreachable from the
//! development environment (a filter breaks TLS), so the tests run on
//! RECORDED responses. The former decision "HTTP stays in Python" is
//! cancelled: the goal is to remove Python from everywhere except the
//! interface.
//!
//! Parity with Python: the golden files were taken from `aur_client.py`
//! itself (monkeypatched `_http_json`/`urlopen` -> a canonical response,
//! the real `search_aur`/`fetch_for_category`/`_parse`/attributes called),
//! and Rust must return the same list and the same strings. Golden tests:
//! `tests/aur_parity.rs` (parsing) and `tests/net_parity.rs` (network
//! branches).

use crate::net::{self, NetError, NetIo};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Base URL of RPC v5 (matches `RPC_BASE`).
pub const RPC_BASE: &str = "https://aur.archlinux.org/rpc/v5";
/// Package page template (`AUR_PKG_URL`).
pub const AUR_PKG_URL_PREFIX: &str = "https://aur.archlinux.org/packages/";
/// User-Agent of the requests (`USER_AGENT`).
pub const USER_AGENT: &str = "Rexon/5.0";

/// One AUR package (a subset of RPC v5 fields), a port of the `AurPackage`
/// dataclass.
///
/// Field order/defaults repeat Python. `category` is the catalog category
/// key ("" = free search). The `installed` runtime flag is set by the UI
/// layer from `installed_set` (it plays no part here).
///
/// serde derives are for the PyO3 bridge (rexon-py): the field names match
/// the dataclass, so a dict is built/parsed through the bridge directly.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AurPackage {
    pub name: String,
    pub description: String,
    pub version: String,
    pub votes: i64,
    pub popularity: f64,
    pub maintainer: String,
    pub out_of_date: bool,
    pub upstream_url: String,
    pub installed: bool,
    pub category: String,
    pub last_modified: i64,
}

impl AurPackage {
    /// `id` for the grid mechanics (`it.id`): matches the package name.
    pub fn id(&self) -> &str {
        &self.name
    }

    /// The package page on aur.archlinux.org (`aur_url`): the name is
    /// percent-encoded like `urllib.parse.quote` (safe by default = only `/`).
    pub fn aur_url(&self) -> String {
        format!("{AUR_PKG_URL_PREFIX}{}", url_quote(&self.name))
    }

    /// The tile picture source: a REAL picture of the item or "".
    ///
    /// * a font: the file of this font from Google Fonts (the `google-font:`
    ///   marker, [`GOOGLE_FONT_SCHEME`]): the tile draws a text sample with it;
    /// * a project on GitHub: the first README picture (the `github-readme:`
    ///   marker, [`README_PREVIEW_SCHEME`]): for wallpaper, icon and theme sets
    ///   it is a screenshot of the set itself;
    /// * otherwise "": the tile honestly says there is no picture
    ///   ([`no_preview_note`]).
    ///
    /// ⛔ The repository OpenGraph card (`opengraph.githubassets.com`: name,
    /// stars, owner avatar) is no longer used here: on wallpaper and font
    /// tiles it passed itself off as a preview of the item (live check
    /// 16.11.2026), and the gallery shows only real screenshots (rule #13).
    /// The GitHub address is parsed with the regexp
    /// `https?://(?:www\.)?github\.com/(owner)/(repo)`, cutting off `.git` and
    /// the tail `[/#?].*`.
    pub fn preview_url(&self) -> String {
        if self.is_font() {
            return google_font_family(&self.name)
                .map(|family| format!("{GOOGLE_FONT_SCHEME}{family}"))
                .unwrap_or_default();
        }
        match github_owner_repo(&self.upstream_url) {
            Some((owner, repo)) => format!("{README_PREVIEW_SCHEME}{owner}/{repo}"),
            None => String::new(),
        }
    }

    /// A font package: the "Fonts" section or a name following the Arch
    /// convention (`ttf-*`, `otf-*`): this way it is recognised in the results
    /// of combined sections too.
    pub fn is_font(&self) -> bool {
        self.category == "fonts" || is_font_package_name(&self.name)
    }

    /// The card emoji (`icon`): by category, otherwise a heuristic on the
    /// package name. The order of checks repeats Python exactly.
    pub fn icon(&self) -> &'static str {
        if let Some(ic) = category_icon(&self.category) {
            return ic;
        }
        let n = self.name.to_lowercase();
        if n.contains("cursor") {
            "🖱️"
        } else if n.contains("icon") {
            "📁"
        } else if n.contains("sddm") {
            "🔐"
        } else if n.contains("plymouth") {
            "⚡"
        } else if n.starts_with("ttf-") || n.starts_with("otf-") || n.contains("font") {
            "🔤"
        } else if n.contains("kvantum") {
            "🔲"
        } else if n.contains("wallpaper") {
            "🖼️"
        } else {
            "💎"
        }
    }
}

/// A font package name by the Arch convention: `ttf-*`, `otf-*`, `woff2-*`.
pub fn is_font_package_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["ttf-", "otf-", "woff-", "woff2-"].iter().any(|p| n.starts_with(p))
}

/// The marker "the tile picture is a font sample from Google Fonts"
/// (`google-font:Fira Sans`): the loader resolves it
/// ([`resolve_google_font`]) into the font file address.
pub const GOOGLE_FONT_SCHEME: &str = "google-font:";

/// Package name suffixes unrelated to the font name.
const FONT_NAME_TAILS: &[&str] = &[
    "git", "bin", "variable", "vf", "ttf", "otf", "font", "fonts", "static", "latest", "full",
];

/// The font family name for Google Fonts from the package name:
/// `ttf-fira-sans-git` -> `Fira Sans`. `None` means not a font package
/// ([`is_font_package_name`]) or no name is left.
///
/// Google Fonts is case-sensitive (`JetBrains Mono` exists, `Jetbrains
/// Mono` does not), and the package loses case: words are capitalised, and
/// families with capitals inside a word are honestly not found; the tile
/// then says there is no sample.
pub fn google_font_family(name: &str) -> Option<String> {
    if !is_font_package_name(name) {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    let rest = lower.split_once('-').map(|(_, r)| r).unwrap_or("");
    let mut words: Vec<&str> = rest.split(['-', '_']).filter(|w| !w.is_empty()).collect();
    while words.last().is_some_and(|w| FONT_NAME_TAILS.contains(w)) {
        words.pop();
    }
    if words.is_empty() {
        return None;
    }
    let title = |w: &str| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
    };
    Some(words.into_iter().map(title).collect::<Vec<_>>().join(" "))
}

/// Address of the family CSS request at Google Fonts (`css2?family=Fira+Sans`).
pub fn google_font_css_url(family: &str) -> String {
    format!("https://fonts.googleapis.com/css2?family={}", quote_plus(family))
}

/// The first font file address from the Google Fonts CSS response
/// (`src: url(…)`). To a simple client Google returns TrueType as one file.
pub fn parse_google_font_css(css: &str) -> Option<String> {
    let at = css.find("url(")?;
    let rest = &css[at + 4..];
    let end = rest.find(')')?;
    let url = rest[..end].trim().trim_matches(['\'', '"']);
    url.starts_with("https://").then(|| url.to_string())
}

/// Find the font file for the `google-font:Family` marker. `None` means
/// Google Fonts has no such family (a 400 response) or the network did not
/// answer.
pub fn resolve_google_font(net: &mut dyn NetIo, marker: &str) -> Option<String> {
    let family = marker.strip_prefix(GOOGLE_FONT_SCHEME)?;
    let body = net.get(&google_font_css_url(family), USER_AGENT, TIMEOUT_SECS).ok()?;
    parse_google_font_css(&String::from_utf8_lossy(&body))
}

/// The label in place of the tile picture when there is no picture: what
/// kind of item this is and why there is no screenshot. The line is true
/// while the picture is loading too (a wallpaper set carries its pictures
/// inside the package). Empty means no label is needed.
pub fn no_preview_note(pkg: &AurPackage) -> String {
    use crate::i18n::tr;
    if pkg.is_font() {
        return tr("Font sample — after installation");
    }
    if pkg.category == "wallpapers" {
        return tr("Wallpaper set — the pictures come inside the package");
    }
    if pkg.preview_url().is_empty() {
        return tr("The author gave no picture");
    }
    String::new()
}

/// Text sample on a font tile.
pub fn font_sample_text() -> String {
    crate::i18n::tr("Aa Bb Cc 123")
}

/// Category emoji by key (`CATEGORY_ICONS`). `None` means no such key.
pub fn category_icon(key: &str) -> Option<&'static str> {
    Some(match key {
        "global" => "💎",
        "plasma" => "🖥️",
        "colors" => "🌈",
        "aurorae" => "🪟",
        "icons" => "📁",
        "cursor" => "🖱️",
        "kvantum" => "🔲",
        "wallpapers" => "🖼️",
        "wallpaper_plugins" => "🎞️",
        "fonts" => "🔤",
        "sounds" => "🔔",
        "plasmoids" => "🧊",
        "sddm" => "🔐",
        "plymouth" => "⚡",
        // Environment sections: an icon of their own type, as in the sidebar.
        _ => return crate::categories::category_kind(key).and_then(crate::constants::type_icon),
    })
}

/// The curated list (`CURATED`): package names for `fetch_curated`.
///
/// The order of `aerothemeplasma-*` sets the order of pinning at the top
/// of the selection: shell, icons, sounds. Sounds were added on 03.10.2026:
/// without them the AeroThemePlasma kit in the selection was incomplete,
/// and the package itself dropped out of the results (`curated_order` pins
/// only what is listed here). In the catalog CATEGORIES the sounds live in
/// "Sounds", not in "Global themes": a section answers the question "what
/// does this install", and the package installs sound themes (see
/// [`is_relevant`]).
pub const CURATED: &[&str] = &[
    "aerothemeplasma-desktop-git",
    "aerothemeplasma-icons-git",
    "aerothemeplasma-sounds-git",
    "numix-icon-theme-git",
    "numix-circle-icon-theme-git",
    "bibata-cursor-theme",
    "paper-icon-theme",
    "klassy",
    "dracula-gtk-theme",
    "flat-remix",
    "la-capitaine-icon-theme",
    "tela-icon-theme",
    "nordic-theme",
    "vimix-gtk-themes",
    "candy-icons-git",
    "catppuccin-gtk-theme-mocha",
];

/// The catalog section in which a curated package stands at the top.
///
/// The selection is shown not as a separate showcase but by pinning in ITS
/// OWN section of the AUR source (without a search query): the
/// AeroThemePlasma shell in "Global themes", its icons in "Icons", the
/// sounds in "Sounds". The table is explicit rather than based on
/// [`is_relevant`]: for some sections the relevance rule lets everything
/// through and would pin, say, icons in "Plasma styles". `None` means the
/// package has no section of its own in the catalog (`klassy` is a window
/// decoration module, not an Aurorae theme); such a package is not pinned
/// anywhere.
pub fn curated_category(name: &str) -> Option<&'static str> {
    Some(match name {
        "aerothemeplasma-desktop-git" => "global",
        "aerothemeplasma-icons-git"
        | "numix-icon-theme-git"
        | "numix-circle-icon-theme-git"
        | "paper-icon-theme"
        | "flat-remix"
        | "la-capitaine-icon-theme"
        | "tela-icon-theme"
        | "candy-icons-git" => "icons",
        "aerothemeplasma-sounds-git" => "sounds",
        "bibata-cursor-theme" => "cursor",
        "dracula-gtk-theme" | "nordic-theme" | "vimix-gtk-themes" | "catppuccin-gtk-theme-mocha" => "gtk",
        _ => return None,
    })
}

/// Whether section `key` has curated packages (otherwise we do not fetch the selection).
pub fn has_curated(key: &str) -> bool {
    CURATED.iter().any(|n| curated_category(n) == Some(key))
}

/// Pin the curated packages of section `key` at the top of the section's
/// results.
///
/// The order of pinned packages is the order of [`CURATED`]; a package
/// already found in the results moves to the top (no duplicate). Packages
/// of other sections and those missing from the `/info` response (removed
/// from the AUR) are not added. The category key of the pinned packages is
/// `key`: the tile icon depends on it.
pub fn pin_curated(key: &str, curated: &[AurPackage], list: Vec<AurPackage>) -> Vec<AurPackage> {
    let mut pinned: Vec<AurPackage> = Vec::new();
    for name in CURATED {
        if curated_category(name) != Some(key) {
            continue;
        }
        let found = list.iter().find(|p| p.name == *name).or_else(|| curated.iter().find(|p| p.name == *name));
        if let Some(p) = found {
            let mut p = p.clone();
            p.category = key.to_string();
            pinned.push(p);
        }
    }
    let rest: Vec<AurPackage> =
        list.into_iter().filter(|p| !pinned.iter().any(|q| q.name == p.name)).collect();
    pinned.extend(rest);
    pinned
}

/// An AUR source section without a search query: [`fetch_for_category`]
/// with this section's curated packages on top ([`pin_curated`]).
///
/// The selection is a decoration, not a condition: if `/info` did not
/// answer, the section is still shown with its own results. The virtual
/// `popular`/`new` do not pin the selection (they have no section of their
/// own).
pub fn fetch_category_with_curated<IO: NetIo + ?Sized>(
    io: &mut IO,
    key: &str,
) -> Result<Vec<AurPackage>, AurError> {
    let list = fetch_for_category(io, key)?;
    if key == "popular" || key == "new" || !has_curated(key) {
        return Ok(list);
    }
    let names: Vec<&str> = CURATED.iter().copied().filter(|n| curated_category(n) == Some(key)).collect();
    match rpc_call(io, &info_url(&names)) {
        Ok(data) => Ok(pin_curated(key, &parse_results(&data), list)),
        Err(_) => Ok(list),
    }
}

/// Aggregate categories for the virtual popular/new (`AGGREGATE_KEYS`).
pub const AGGREGATE_KEYS: &[&str] = &["global", "plasma", "colors", "icons", "cursor"];

/// AUR search queries by category key (`CATEGORY_QUERIES`).
/// `(keyword, by)`: `by`=name (a name substring) / name-desc (name+description).
pub fn category_queries(key: &str) -> &'static [(&'static str, &'static str)] {
    match key {
        "global" => &[
            ("aerothemeplasma", "name"),
            ("plasma-theme", "name"),
            ("look and feel", "name-desc"),
            ("global theme", "name-desc"),
        ],
        "plasma" => &[("plasma-theme", "name")],
        "colors" => &[("colorscheme", "name"), ("color-scheme", "name")],
        "aurorae" => &[("aurorae", "name")],
        "icons" => &[("icon-theme", "name"), ("icons", "name")],
        "cursor" => &[("cursor-theme", "name"), ("cursors", "name"), ("xcursor", "name")],
        "kvantum" => &[("kvantum", "name")],
        // "Application style": Kvantum themes and ready Qt styles. Styles have
        // no naming convention of their own (AUR snapshot 17.10.2026: `darkly`,
        // `lightly-qt`, `klassy`, `qt5-styleplugins`, `plastikstyle-qt6`), so
        // the queries go by family names; [`is_relevant`] cuts off the rest.
        "widgetstyle" => &[
            ("kvantum", "name"),
            ("darkly", "name"),
            ("lightly", "name"),
            ("klassy", "name"),
            ("styleplugin", "name"),
            ("plastikstyle", "name"),
        ],
        // GTK themes. Snapshot 04.10.2026: names `*-gtk-theme(s)*`/`gtk-theme-*`
        // are 236 packages, `gtk3-theme-*` 4 more. Classic sets are named
        // differently (`zuki-themes`, `vertex-themes`, `ubuntu-themes`,
        // `xfce-theme-greybird`), but their GTK2 part is drawn by the Murrine
        // engine, and the package depends on `gtk-engine-murrine` (110 packages).
        // A broad "gtk theme" by description picks up `flat-remix-gtk`,
        // `darkly-gtk` and themes without "gtk" in the name; [`is_relevant`] cuts
        // icons, cursors, Fluxbox styles and switcher programs out of these two
        // queries.
        "gtk" => &[
            ("gtk-theme", "name"),
            ("gtk3-theme", "name"),
            ("gtk-engine-murrine", "depends"),
            ("gtk theme", "name-desc"),
        ],
        "wallpapers" => &[("wallpaper", "name")],
        "wallpaper_plugins" => {
            &[("plasma6-wallpapers", "name"), ("plasma wallpaper plugin", "name-desc")]
        }
        "fonts" => &[("ttf-", "name"), ("otf-", "name")],
        // Not every package has a "*-sound-theme" name: sets like
        // `aerothemeplasma-sounds-git` and `xde-sounds` are named differently,
        // and "sound theme" is in their description. [`is_relevant`] cuts the
        // extras out of the two broad queries.
        "sounds" => &[
            ("sound-theme", "name"),
            ("sound theme", "name-desc"),
            ("sounds", "name"),
        ],
        "plasmoids" => &[("plasma6-applet", "name"), ("plasmoid", "name")],
        "sddm" => &[("sddm-theme", "name")],
        "plymouth" => &[("plymouth-theme", "name")],
        // KWin packages in the AUR are named `kwin-script(s)-*` and
        // `kwin-effect(s)-*` (snapshot 04.10.2026: 17 and 38 packages). Window
        // switchers have no packages of their own, so the section does without
        // the AUR.
        "kwinscripts" => &[("kwin-script", "name")],
        "kwineffects" => &[("kwin-effect", "name")],
        _ => &[],
    }
}

/// Whether the AUR has a shelf for the section: its own queries
/// ([`category_queries`]) or the combined "Popular"/"New" showcase
/// ([`AGGREGATE_KEYS`]).
pub fn has_category(key: &str) -> bool {
    key == "popular" || key == "new" || !category_queries(key).is_empty()
}

// ── Category relevance filter ──────────────────────────────────────────────
//
// RPC search is a substring of the name or description, and broad queries
// bring in unrelated things. A live snapshot on 03.10.2026: "look and
// feel" by name-desc pulled into "Global themes" the cool-retro-term-git
// terminal, the qemacs-git editor, the windowmaker-git and labwc-git
// window managers, a Second Life client and the jgoodies-looks Java
// library: all of them have "look and feel" in the description, but they
// have nothing to do with a Plasma theme. Colour schemes had the same
// tails (vim, Konsole, alacritty schemes), as did icons (ttf-ionicons
// fonts, akonadiconsole), cursors (emacs-multiple-cursors), wallpapers
// (background installer programs) and sounds (FreeSWITCH voice packages,
// VST plugins). One decision for all categories: the package must be a
// THEME of this kind, not merely match a word.

/// Packages known not to be themes that broad queries bring in: whole names.
///
/// This is a safety net on top of the [`is_relevant`] rules: a rule may
/// change, but these packages must not return to the catalog with any
/// change of it.
pub const NOT_THEMES: &[&str] = &[
    "cool-retro-term-git",
    "qemacs-git",
    "windowmaker-git",
    "labwc",
    "labwc-git",
    "labwc-im",
    "labwc-im-git",
    "labwc-hdr-metadata-git",
    "singularityviewer-alpha",
    "jgoodies-looks",
    "plasma-theme-master",
    "plasmoidviewer-debug",
    "aimp-skin-aurorae",
];

/// Prefixes of library packages and editor add-ons: such a package is
/// never a desktop appearance theme, whatever category it surfaces in.
const NOT_THEME_PREFIXES: &[&str] = &[
    "vim-", "neovim-", "nvim-", "emacs-", "python-", "python2-", "perl-",
    "ruby-", "nodejs-", "lua-", "haskell-", "rust-", "go-", "lib32-",
    "pidgin-", "geany-", "freeswitch-", "svxlink-",
    // Snapshot 04.10.2026: GNOME Shell extensions ("desktop icons", "Bing
    // wallpaper"), Android library builds and Themix theme builder add-ons
    // surfaced in "Icons", "Cursors" and "Wallpapers".
    "gnome-shell-extension-", "android-", "themix-",
];

/// Name words that make a package a program rather than a set (for wallpapers and sounds).
const TOOL_NAME_TOKENS: &[&str] = &[
    "cli", "gui", "engine", "wallpaperengine", "switch", "changer", "setter",
    "picker", "manager", "editor", "generator", "plugin", "player", "daemon",
    "lv2", "vst", "vst3", "bin-gui", "viewer", "fetcher", "downloader", "splitter",
    "bing", "importer",
];

/// Description words that make a package a program rather than a set.
const TOOL_DESC_WORDS: &[&str] = &[
    "setter", "setting utility", "wallpaper manager", "utility", "utilities", "a script",
    "bash script", "simple script", "gui for", "renderer", "runtime",
    "integration", "plugin", "daemon", "tool", "application", "client",
    "implementation of", "a program", "manage ", "fetches", "player",
    "editor", "engine", "voice recordings", "language pack", "effects app",
    // Wallpaper changing and downloading programs (snapshot 04.10.2026).
    "chooser", "change your", "changes your", "changes the desktop",
    "change wallpaper", "downloader", "download of", "fetch", "creator",
    "maker", "split and span", "show up in", "powered by", "wallpaper helper",
    "photo of the day", "use images", "as wallpaper", "live wallpaper",
    "animated", "automatically set", "beautify",
];

/// Description words that make a package from the GTK section a program
/// or an add-on rather than a theme: theme switchers, settings bridges,
/// theme builder add-ons. The general [`TOOL_DESC_WORDS`] will not do
/// here: "engine" appears in the description of real themes ("theme for
/// Clearlooks GTK+ engine").
const GTK_TOOL_DESC_WORDS: &[&str] = &[
    "chooser", "switcher", "theme manager", "gui for", "a gui ", "plugin",
    "syncs ", "a tool", "utility", "extension",
];

/// Name parts split by `-`/`_`, in lower case.
fn name_tokens(name: &str) -> Vec<String> {
    name.to_lowercase()
        .split(['-', '_'])
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether a package fits a catalog category: see the section above.
///
/// An unknown category (and fonts, SDDM, Plymouth, Kvantum, widgets: their
/// queries follow a strict naming convention) is checked only against the
/// common stop lists.
pub fn is_relevant(key: &str, pkg: &AurPackage) -> bool {
    let name = pkg.name.to_lowercase();
    let desc = pkg.description.to_lowercase();
    if NOT_THEMES.contains(&name.as_str())
        || NOT_THEME_PREFIXES.iter().any(|p| name.starts_with(p))
    {
        return false;
    }
    let tokens = name_tokens(&name);
    let has_token = |t: &str| tokens.iter().any(|x| x == t);
    let tool_name = TOOL_NAME_TOKENS.iter().any(|t| has_token(t));
    let tool_desc = TOOL_DESC_WORDS.iter().any(|w| desc.contains(w));
    let plasma_ish = desc.contains("plasma") || desc.contains("kde") || name.contains("plasma");
    match key {
        "global" => {
            // AeroThemePlasma icons and sounds have their own sections ("Icons",
            // "Sounds"): in "Global themes" the icons card would promise a
            // global theme the package does not contain.
            let family = name.starts_with("aerothemeplasma") || name.starts_with("vistathemeplasma");
            if family {
                return !(has_token("icons") || has_token("sounds"));
            }
            const NAME_MARKS: &[&str] =
                &["look-and-feel", "lookandfeel", "global-theme", "plasma-theme"];
            if NAME_MARKS.iter().any(|m| name.contains(m)) {
                return true;
            }
            let lnf = desc.contains("look and feel")
                || desc.contains("look-and-feel")
                || desc.contains("global theme");
            lnf && plasma_ish
        }
        "plasma" => true,
        // Application style is a Kvantum theme or a Qt style: the description
        // says "style" or "Kvantum". Namesakes of the families with another job
        // (the GTK theme `darkly-gtk-git`, the editor `darkly-art-bin`, the KWin
        // effect `lightlyshaders-git`) do not carry this word.
        "widgetstyle" => {
            let kvantum = name.contains("kvantum") || desc.contains("kvantum");
            let style = desc.contains("style") && !has_token("gtk");
            !tool_name && (kvantum || style)
        }
        // The catalog's colour schemes are KDE schemes (`.colors`); vim, Konsole,
        // terminal and TTY schemes have nothing to do with desktop appearance.
        "colors" => plasma_ish && !desc.contains("konsole"),
        "aurorae" => {
            name.contains("aurorae-theme") || desc.contains("aurorae theme") || plasma_ish
        }
        // "icons" as a substring catches akonadiconsole and icon fonts.
        "icons" => {
            let font = ["ttf-", "otf-", "woff-", "woff2-"].iter().any(|p| name.starts_with(p));
            let named = has_token("icons") || has_token("icon") || name.contains("icon-theme");
            let described = desc.contains("icon theme") || desc.contains("icons theme");
            // File manager add-ons, stencils, icons in the terminal.
            const ADDON_WORDS: &[&str] = &[
                "extension that", "nautilus extension", "caja extension", "stencil",
                "in terminal", "file manager", "emoticons", "browser addon",
            ];
            let addon = ADDON_WORDS.iter().any(|w| desc.contains(w));
            !font && !tool_name && !addon && (named || described)
        }
        "cursor" => {
            !name.contains("multiple-cursors") && !desc.contains("selection") && !tool_name
        }
        // Wallpapers are sets of pictures; background programs and Plasma
        // plugins live in "Wallpaper plugins" or not in the catalog at all.
        "wallpapers" => {
            !["plasma5-wallpapers-", "plasma6-wallpapers-", "plasma6-applets-", "plasma-wallpaper-"]
                .iter()
                .any(|p| name.starts_with(p))
                // A GTK theme variant with wallpapers inside belongs in "GTK themes".
                && !name.contains("gtk-theme")
                && !tool_name
                && !tool_desc
        }
        "sounds" => {
            if tool_name || tool_desc || name.starts_with("lib") {
                return false;
            }
            name.contains("sound-theme")
                || desc.contains("sound theme")
                || desc.contains("desktop sounds")
                || (desc.contains("sounds for") && (plasma_ish || desc.contains("desktop")))
        }
        // GTK themes. The section's queries ([`category_queries`]) bring in
        // unrelated things too: icons and cursors "for a GTK theme", Fluxbox
        // styles, a tint2rc file, theme switchers and configurators, engines.
        "gtk" => {
            let gtk_tool = GTK_TOOL_DESC_WORDS.iter().any(|w| desc.contains(w))
                || tokens.iter().any(|t| t.starts_with("switch") || t == "config" || t == "bridge");
            if tool_name || gtk_tool || name.starts_with("lib") {
                return false;
            }
            const OTHER_PARTS: &[&str] =
                &["icon", "icons", "cursor", "cursors", "folders", "aurorae", "xfwm4", "fluxbox", "tint2rc"];
            if OTHER_PARTS.iter().any(|t| has_token(t)) || desc.contains("fluxbox") {
                return false;
            }
            // A set for Plasma (a style, a global theme) is not a GTK theme,
            // even if it has a GTK part; `gtk-theme-plasma` is a GTK theme.
            if has_token("plasma") && !name.contains("gtk") {
                return false;
            }
            const GTK_DESKTOPS: &[&str] =
                &["gtk", "gnome", "xfce", "mate", "cinnamon", "budgie", "metacity", "openbox", "unity"];
            // `*-themes` without "gtk" in the name and description are classic
            // sets found by the query on the Murrine dependency
            // (`ubuntu-themes`, `numix-frost-themes`).
            let gtk_ish = name.contains("gtk")
                || has_token("themes")
                || GTK_DESKTOPS.iter().any(|w| desc.contains(w));
            let theme_ish = name.contains("theme") || desc.contains("theme");
            gtk_ish && theme_ish
        }
        _ => true,
    }
}

/// Words for the section's kind of item: a package from SEARCH must carry
/// one of them in the name or description. An empty list means the
/// section's [`is_relevant`] rule already requires the kind itself
/// ("Global themes", "Icons", "GTK themes", "Application style").
///
/// Why. The section list arrives by naming convention
/// ([`category_queries`]: "wallpaper" in the name), and the [`is_relevant`]
/// rules only have to cut the tails. Search, however, goes across the
/// whole AUR: for "girl" in "HD wallpapers" fcitx dictionaries, fonts and a
/// game engine came back (live check 16.11.2026): they have none of the
/// forbidden words, but no word "wallpaper" either. RPC search does not
/// return a package's keywords and groups (only `info` does), so the
/// measure is the name and description.
pub fn kind_markers(key: &str) -> &'static [&'static str] {
    match key {
        "wallpapers" => &["wallpaper", "background", "backdrop"],
        "wallpaper_plugins" => &["wallpaper"],
        "fonts" => &["font", "ttf-", "otf-", "typeface"],
        "cursor" => &["cursor"],
        "sounds" => &["sound"],
        "plasmoids" => &["plasmoid", "applet", "widget"],
        "sddm" => &["sddm"],
        "plymouth" => &["plymouth"],
        "kwinscripts" | "kwineffects" => &["kwin"],
        "kvantum" => &["kvantum"],
        "colors" => &["color", "colour"],
        "aurorae" => &["aurorae"],
        "plasma" => &["plasma"],
        _ => &[],
    }
}

/// Whether a package from SEARCH fits the section: the section rule
/// ([`is_relevant`]) and a word of its kind ([`kind_markers`]). "Popular"
/// and "New" are combined: a package fits if it fits any of
/// [`AGGREGATE_KEYS`].
pub fn search_fits(key: &str, pkg: &AurPackage) -> bool {
    search_kind(key, pkg).is_some()
}

/// The section a package from search fits ([`search_fits`]): `key` itself,
/// for combined ones the first fitting key from [`AGGREGATE_KEYS`] (the
/// tile icon depends on it). `None` means it does not fit.
pub fn search_kind<'a>(key: &'a str, pkg: &AurPackage) -> Option<&'a str> {
    if key == "popular" || key == "new" {
        return AGGREGATE_KEYS.iter().copied().find(|k| search_fits(k, pkg));
    }
    if !is_relevant(key, pkg) {
        return None;
    }
    let markers = kind_markers(key);
    if markers.is_empty() {
        return Some(key);
    }
    let hay = format!("{} {}", pkg.name, pkg.description).to_lowercase();
    markers.iter().any(|m| hay.contains(m)).then_some(key)
}

/// AUR search within a section: [`search_aur`], then only what fits the
/// section ([`search_kind`]), with the section key on the package.
pub fn search_in_category<IO: NetIo + ?Sized>(
    io: &mut IO,
    query: &str,
    key: &str,
) -> Result<Vec<AurPackage>, AurError> {
    let found = search_aur(io, query, None)?;
    Ok(filter_search_for_category(found, key))
}

/// Filtering search results for a section: the pure part of
/// [`search_in_category`]. A section without an AUR shelf
/// ([`has_category`]) gets the results unfiltered.
pub fn filter_search_for_category(found: Vec<AurPackage>, key: &str) -> Vec<AurPackage> {
    if !has_category(key) {
        return found;
    }
    found
        .into_iter()
        .filter_map(|mut p| {
            let kind = search_kind(key, &p)?.to_string();
            p.category = kind;
            Some(p)
        })
        .collect()
}

/// Parse one RPC record into an `AurPackage`: a port of `_parse`.
///
/// ⚠️ Numeric fields are parsed TOLERANTLY of a foreign schema
/// (`int_loose`/`float_loose`/`truthy`): RPC returns
/// `NumVotes`/`LastModified` as integers, `Popularity` as a float,
/// `OutOfDate` as `null` or an epoch, but AUR mirrors and proxy wrappers
/// sometimes deliver numbers AS STRINGS. The original crashed on this
/// (`int("3")` still works, but `int("")`/`int("n/a")` is a ValueError
/// inside the catalog worker), so here the string is parsed and garbage
/// gives the default: the card loses a counter, but the AUR source does
/// not fall apart entirely.
pub fn parse_record(rec: &Value) -> AurPackage {
    AurPackage {
        name: str_or_empty(rec, "Name"),
        description: str_or_empty(rec, "Description"),
        version: str_or_empty(rec, "Version"),
        votes: int_loose(rec.get("NumVotes")),
        popularity: float_loose(rec.get("Popularity")),
        maintainer: str_or_empty(rec, "Maintainer"),
        // bool(OutOfDate): null/0/"" -> false, a non-zero epoch -> true.
        out_of_date: truthy(rec.get("OutOfDate")),
        upstream_url: str_or_empty(rec, "URL"),
        installed: false,
        category: String::new(),
        last_modified: int_loose(rec.get("LastModified")),
    }
}

/// The `results` records of a response: objects only.
///
/// A missing `results`, `null`, a string instead of an array, individual
/// non-objects inside the array: all of this gives empty/truncated results
/// rather than garbage packages with an empty name. The original crashed
/// with a TypeError on `"results": null` (iterating over None) and with an
/// AttributeError in `_parse` on a string; fixed in `aur_client.py` too
/// (`_results`).
fn results_of(data: &Value) -> impl Iterator<Item = &Value> {
    data.get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|r| r.is_object())
}

/// All `results` of a response -> packages. Missing `results` -> an empty list.
pub fn parse_results(data: &Value) -> Vec<AurPackage> {
    results_of(data).map(parse_record).collect()
}

/// RPC error check (`_check_rpc_error`): `type=="error"` or a truthy
/// `error` -> `Err(text)`.
///
/// The port takes into account that Python checks `data.get("error")` for
/// TRUTHINESS, not for "a non-empty string": a response like
/// `{"error": ["too many results"]}` (some proxies in front of the AUR
/// answer like this) was an error in the original too. As the text of such
/// an error we take the compact JSON of the value: the only difference
/// from Python, where `str(RuntimeError(list))` would give the Python repr
/// of the list.
pub fn check_rpc_error(data: &Value) -> Result<(), String> {
    let is_err_type = data.get("type").and_then(Value::as_str) == Some("error");
    let err = data.get("error");
    if is_err_type || truthy(err) {
        let msg = match err {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            Some(v) if truthy(Some(v)) => v.to_string(),
            _ => "AUR RPC error".to_string(),
        };
        return Err(msg);
    }
    Ok(())
}

/// Post-fetch logic of `fetch_curated` over the `/rpc/v5/info` response:
/// records into a dictionary by name (the last one with the same name wins,
/// as in a Python dict), `aerothemeplasma*` at the top in [`CURATED`]
/// order, the rest by votes (descending, stable, in order of appearance in
/// the response).
///
/// ⚠️ Fixed a bug of the original: it built the dictionary via `r["Name"]`
/// and crashed with `KeyError` on a record without `Name` (RPC returns such
/// records when it truncates a response at the limit). Here records
/// without a name are skipped.
pub fn curated_order(data: &Value) -> Vec<AurPackage> {
    // Insertion order + last-value-wins: like a dict comprehension in Python.
    let mut order: Vec<String> = Vec::new();
    let mut by_name: std::collections::HashMap<String, AurPackage> =
        std::collections::HashMap::new();
    for rec in results_of(data) {
        let name = str_or_empty(rec, "Name");
        if name.is_empty() {
            continue;
        }
        if by_name.insert(name.clone(), parse_record(rec)).is_none() {
            order.push(name);
        }
    }

    let mut pinned: Vec<AurPackage> = Vec::new();
    for name in CURATED {
        if name.starts_with("aerothemeplasma") {
            if let Some(p) = by_name.get(*name) {
                pinned.push(p.clone());
            }
        }
    }
    let mut rest: Vec<AurPackage> = order
        .iter()
        .filter(|n| !n.starts_with("aerothemeplasma"))
        .filter_map(|n| by_name.get(n).cloned())
        .collect();
    // key=lambda p: -p.votes: votes only; out_of_date does NOT take part here
    // (unlike sort_votes); the sort is stable, like sorted() in Python.
    rest.sort_by_key(|p| std::cmp::Reverse(p.votes));

    pinned.into_iter().chain(rest).collect()
}

/// Sort by votes (`_sort_votes`): key `(out_of_date, -votes)`, stable
/// (outdated ones at the end, ties keep the original order).
pub fn sort_votes(pkgs: &mut [AurPackage]) {
    pkgs.sort_by(|a, b| {
        a.out_of_date
            .cmp(&b.out_of_date)
            .then_with(|| b.votes.cmp(&a.votes))
    });
}

/// The "new" sort (the `key=="new"` branch): `(out_of_date, -last_modified)`.
pub fn sort_new(pkgs: &mut [AurPackage]) {
    pkgs.sort_by(|a, b| {
        a.out_of_date
            .cmp(&b.out_of_date)
            .then_with(|| b.last_modified.cmp(&a.last_modified))
    });
}

/// Post-fetch logic of `search_aur` over an ALREADY received RPC response
/// (for the longest word): a local AND filter on the remaining words over
/// name+description, then `sort_votes`. `data` is the RPC response (an
/// object with `results`).
///
/// `limit` is as in Python (`None` = the whole list).
pub fn search_filter(data: &Value, query: &str, limit: Option<usize>) -> Vec<AurPackage> {
    let terms: Vec<String> = query
        .trim()
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }
    // key = the longest word; Python's max() takes the FIRST on equal length
    // (Rust's max_by_key would return the last), so we look for the first
    // with the maximum length.
    let max_len = terms.iter().map(|t| t.chars().count()).max().unwrap();
    let key = terms
        .iter()
        .find(|t| t.chars().count() == max_len)
        .cloned()
        .unwrap();
    let rest: Vec<&String> = terms.iter().filter(|t| **t != key).collect();

    let mut out: Vec<AurPackage> = parse_results(data)
        .into_iter()
        .filter(|p| {
            if rest.is_empty() {
                return true;
            }
            let hay = format!("{} {}", p.name, p.description).to_lowercase();
            rest.iter().all(|t| hay.contains(t.as_str()))
        })
        .collect();
    sort_votes(&mut out);
    match limit {
        Some(n) => out.into_iter().take(n).collect(),
        None => out,
    }
}

/// Post-fetch logic of `fetch_for_category` (the non-virtual branch):
/// merging the responses of the category queries with dedup by name (the
/// first wins), filtering out unrelated packages ([`is_relevant`]), setting
/// `category`, `sort_votes` and pinning `aerothemeplasma` at the top for
/// `global`. `responses` are RPC responses in `category_queries(key)` order.
pub fn dedup_category(responses: &[&Value], key: &str) -> Vec<AurPackage> {
    let mut seen: Vec<AurPackage> = Vec::new();
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for data in responses {
        for rec in results_of(data) {
            let name = str_or_empty(rec, "Name");
            if name.is_empty() || names.contains(&name) {
                continue;
            }
            let mut p = parse_record(rec);
            names.insert(name);
            if !is_relevant(key, &p) {
                continue;
            }
            p.category = key.to_string();
            seen.push(p);
        }
    }
    sort_votes(&mut seen);
    if key == "global" {
        let (pinned, rest): (Vec<_>, Vec<_>) = seen
            .into_iter()
            .partition(|p| p.name.starts_with("aerothemeplasma"));
        seen = pinned.into_iter().chain(rest).collect();
    }
    seen
}

// ── helpers ───────────────────────────────────────────────────────────────

/// `rec.get(key)` as a string; null/missing/non-string -> "". Repeats
/// `rec.get(k) or ""` (and `rec.get("Name", "")`).
fn str_or_empty(rec: &Value, key: &str) -> String {
    rec.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// A port of `int(v or 0)`, tolerant of a foreign schema: a number -> its
/// integer part (truncation toward zero, like `int()`), a string with a
/// number -> the parsed value, `true` -> 1, everything else (null, a
/// garbage string, an object) -> 0. The original crashed with a ValueError
/// on a garbage string; here it is the default (see `parse_record`).
fn int_loose(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.trunc() as i64))
            .unwrap_or(0),
        Some(Value::String(s)) => {
            let t = s.trim();
            t.parse::<i64>()
                .ok()
                .or_else(|| t.parse::<f64>().ok().map(|f| f.trunc() as i64))
                .unwrap_or(0)
        }
        Some(Value::Bool(b)) => i64::from(*b),
        _ => 0,
    }
}

/// A port of `float(v or 0.0)` with the same tolerance as [`int_loose`].
fn float_loose(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse::<f64>().unwrap_or(0.0),
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => 0.0,
    }
}

/// A port of Python value truthiness: null/missing/`false`/0/""/an empty
/// list/an empty object -> `false`, everything else -> `true`.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Percent-encode like `urllib.parse.quote(s)`: safe = ALPHA/DIGIT/`_.-~/`.
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

/// Parse a GitHub upstream into `(owner, repo)` by the Python regexp
/// `https?://(?:www\.)?github\.com/([\w.-]+)/([\w.-]+?)(?:\.git)?(?:[/#?].*)?$`.
/// Returns `None` if this is not a github URL of the required form.
fn github_owner_repo(url: &str) -> Option<(String, String)> {
    // Character class `[\w.-]`: a Unicode word character (like Python \w) plus `.`/`-`.
    fn cls(c: char) -> bool {
        c.is_alphanumeric() || c == '_' || c == '.' || c == '-'
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let rest = rest.strip_prefix("github.com/")?;

    // owner = the longest prefix from the class before '/'; the '/' must exist.
    let owner_end = rest.find(|c: char| !cls(c))?;
    if rest.as_bytes().get(owner_end) != Some(&b'/') {
        return None; // the character after owner is not '/' -> the form did not match
    }
    let owner = &rest[..owner_end];
    if owner.is_empty() {
        return None;
    }
    let after = &rest[owner_end + 1..];

    // repo = the longest prefix from the class; after it only the end of the
    // string or a tail starting with '/'/'#'/'?' is allowed.
    let seg_end = after.find(|c: char| !cls(c)).unwrap_or(after.len());
    let tail = &after[seg_end..];
    if !tail.is_empty() && !matches!(tail.as_bytes()[0], b'/' | b'#' | b'?') {
        return None;
    }
    let mut seg = &after[..seg_end];
    // Cut off the `.git` suffix (the non-greedy repo does not capture it).
    if let Some(stripped) = seg.strip_suffix(".git") {
        if !stripped.is_empty() {
            seg = stripped;
        }
    }
    if seg.is_empty() {
        return None;
    }
    Some((owner.to_string(), seg.to_string()))
}

// ── Network layer ─────────────────────────────────────────────────────────
//
// A port of `_http_json` and the network functions
// `fetch_curated`/`fetch_for_category`/`search_aur`. The call itself is
// behind the [`NetIo`] trait (`net.rs`).

/// RPC request timeout, seconds (`TIMEOUT`).
pub const TIMEOUT_SECS: u64 = 25;

/// A failure talking to the AUR RPC: a port of the exceptions `aur_client`
/// does NOT catch: they fly up to the catalog worker and are shown to the
/// customer (`_AurWorker.failed.emit(str(e))`).
///
/// So [`Display`](std::fmt::Display) gives exactly the same text as
/// `str(e)` in Python.
#[derive(Debug, Clone, PartialEq)]
pub enum AurError {
    /// A network failure: `_http_json` catches NOTHING, the urllib exception
    /// goes up as is.
    Net(NetError),
    /// The response did not parse as JSON (an HTML error page, a proxy captcha).
    NotJson(String),
    /// The response parsed, but it is not an object: all readers further on
    /// call `.get(...)`, and previously this surfaced as an obscure
    /// `AttributeError` deep in parsing. Inside is the type name the Python
    /// way (`list`, `str`, …).
    NotObject(String),
    /// RPC returned an error (`type == "error"` or a non-empty `error`).
    Rpc(String),
}

impl std::fmt::Display for AurError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AurError::Net(e) => write!(f, "{e}"),
            AurError::NotJson(detail) => {
                f.write_str(&crate::i18n::trf("AUR RPC returned something other than JSON: %1", &[detail]))
            }
            AurError::NotObject(ty) => {
                f.write_str(&crate::i18n::trf("AUR RPC returned a %1 instead of an object", &[ty]))
            }
            AurError::Rpc(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for AurError {}

/// The value's type name the Python way, for the text of
/// [`AurError::NotObject`] (`type(data).__name__`). Integers and floats
/// are told apart the same way `json.loads` tells them apart.
fn python_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `urllib.parse.quote_plus`: safe = ALPHA/DIGIT/`_.-~`, space -> `+`.
fn quote_plus(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b == b' ' {
            out.push('+');
        } else if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// URL of exact data for a list of names: a port of the building in
/// `fetch_curated` (`urlencode([("arg[]", n) for n in names])`; the `arg[]`
/// key is percent-encoded too -> `arg%5B%5D`).
pub fn info_url(names: &[&str]) -> String {
    let q = names
        .iter()
        .map(|n| format!("arg%5B%5D={}", quote_plus(n)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{RPC_BASE}/info?{q}")
}

/// Search URL: a port of `f"{RPC_BASE}/search/{quote(kw)}?by={by}"`. The
/// keyword is escaped with `urllib.parse.quote` (safe by default is `/`),
/// `by` is inserted as is (the values are our own, from
/// [`category_queries`]).
pub fn search_url(keyword: &str, by: &str) -> String {
    format!("{RPC_BASE}/search/{}?by={by}", url_quote(keyword))
}

/// The RPC response as an object: a port of `_http_json`.
///
/// The type check is mandatory: all readers further on call `.get(...)`,
/// and RPC (or a proxy/captcha in front of it) can answer with an array or
/// a string. Non-JSON (an HTML error page) also produces an error upwards,
/// so that the catalog worker shows the reason instead of "the AUR source
/// is empty".
///
/// The body is decoded as UTF-8 with broken bytes replaced: a port of
/// `resp.read().decode("utf-8", "replace")`.
///
/// ⚠️ A divergence from the original (rule #12): a transient failure IS
/// RETRIED, see [`crate::net::with_retry`]. The AUR shelf has the same
/// class of failures as the OCS catalog: a single dropped connection
/// showed the customer an error instead of the package list.
pub fn http_json<IO: NetIo + ?Sized>(io: &mut IO, url: &str) -> Result<Value, AurError> {
    let what = format!("AUR RPC {url}");
    let raw = net::with_retry(io, net::Retry::IDEMPOTENT, &what, |io| {
        io.get(url, USER_AGENT, TIMEOUT_SECS)
    })
    .map_err(AurError::Net)?;
    let text = String::from_utf8_lossy(&raw);
    let data: Value =
        serde_json::from_str(&text).map_err(|e| AurError::NotJson(e.to_string()))?;
    if data.is_object() {
        Ok(data)
    } else {
        Err(AurError::NotObject(python_type_name(&data).to_string()))
    }
}

/// One RPC request with a protocol-level error check: the common tail of
/// all three network functions (`_http_json` + `_check_rpc_error`).
fn rpc_call<IO: NetIo + ?Sized>(io: &mut IO, url: &str) -> Result<Value, AurError> {
    let data = http_json(io, url)?;
    check_rpc_error(&data).map_err(AurError::Rpc)?;
    Ok(data)
}

/// The curated list in one `/info` request: a port of `fetch_curated`.
/// AeroThemePlasma at the top, the rest by votes (see [`curated_order`]).
pub fn fetch_curated<IO: NetIo + ?Sized>(io: &mut IO) -> Result<Vec<AurPackage>, AurError> {
    let data = rpc_call(io, &info_url(CURATED))?;
    Ok(curated_order(&data))
}

/// AUR search (name+description): a port of `search_aur`.
///
/// RPC searches for ONE literal substring ("aero plasma" -> 0 results), so
/// a multi-word query goes to RPC with the longest word, and the other
/// words become a local AND filter (see [`search_filter`]). An empty query
/// does not touch the network. `limit = None` means the whole list (the
/// interface cuts the pages).
pub fn search_aur<IO: NetIo + ?Sized>(
    io: &mut IO,
    query: &str,
    limit: Option<usize>,
) -> Result<Vec<AurPackage>, AurError> {
    let lowered = query.trim().to_lowercase();
    let terms: Vec<&str> = lowered.split_whitespace().collect();
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    // Python's max() takes the FIRST word on equal length.
    let max_len = terms.iter().map(|t| t.chars().count()).max().unwrap();
    let key = terms.iter().find(|t| t.chars().count() == max_len).unwrap();
    let data = rpc_call(io, &search_url(key, "name-desc"))?;
    Ok(search_filter(&data, query, limit))
}

/// AUR packages of a catalog category: a port of `fetch_for_category`.
///
/// A normal category: merging searches by naming convention
/// ([`category_queries`]) with dedup by name. The virtual `popular`/`new`:
/// aggregation of [`AGGREGATE_KEYS`] with dedup (the first wins); packages
/// keep THEIR OWN category key (the card emoji depends on it); `new` is
/// sorted by modification date, `popular` by votes. A category without
/// queries -> an empty list (an honest "nothing in the AUR"), and the
/// network is not touched.
pub fn fetch_for_category<IO: NetIo + ?Sized>(
    io: &mut IO,
    key: &str,
) -> Result<Vec<AurPackage>, AurError> {
    if key == "popular" || key == "new" {
        let mut seen: Vec<AurPackage> = Vec::new();
        let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for sub in AGGREGATE_KEYS {
            for p in fetch_for_category(io, sub)? {
                if names.insert(p.name.clone()) {
                    seen.push(p);
                }
            }
        }
        if key == "new" {
            sort_new(&mut seen);
        } else {
            sort_votes(&mut seen);
        }
        return Ok(seen);
    }
    let queries = category_queries(key);
    if queries.is_empty() {
        return Ok(Vec::new());
    }
    let mut responses: Vec<Value> = Vec::with_capacity(queries.len());
    for (kw, by) in queries {
        responses.push(rpc_call(io, &search_url(kw, by))?);
    }
    let refs: Vec<&Value> = responses.iter().collect();
    Ok(dedup_category(&refs, key))
}

// ═══════════════════════════════════════════════════════════════════════
//  Package card: everything the AUR knows about a package, inside the program
// ═══════════════════════════════════════════════════════════════════════
//
// An AUR tile opens a detailed card, like a store item, not a page in the
// browser. The data comes from two places:
//
// * RPC v5 `info`: the package passport: version, maintainer, votes,
//   popularity, dates, licence, dependencies, keywords, the "out of date"
//   flag (RPC documentation: https://wiki.archlinux.org/title/Aurweb_RPC_interface);
// * the package HTML page: what RPC does not return at all:
//   co-maintainers and comments. The AUR has no comments API, so the page
//   is parsed; if an "are you a robot" check (Anubis) arrived instead of
//   it, the card honestly says "comments are on the site" and offers a
//   button.
//
// Network goes through the same [`NetIo`]; parsing is pure functions over
// recorded responses (`tests/aur_detail.rs`).

/// The package passport from RPC `info`.
///
/// `out_of_date` is the time of the "out of date" flag (epoch); 0 means not flagged.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AurInfo {
    pub name: String,
    pub package_base: String,
    pub version: String,
    pub description: String,
    pub url: String,
    pub maintainer: String,
    pub submitter: String,
    pub votes: i64,
    pub popularity: f64,
    pub first_submitted: i64,
    pub last_modified: i64,
    pub out_of_date: i64,
    pub license: Vec<String>,
    pub keywords: Vec<String>,
    pub groups: Vec<String>,
    pub depends: Vec<String>,
    pub make_depends: Vec<String>,
    pub check_depends: Vec<String>,
    pub opt_depends: Vec<String>,
    pub conflicts: Vec<String>,
    pub provides: Vec<String>,
    pub replaces: Vec<String>,
}

impl AurInfo {
    /// A tile from the passport, for installation and the common display path.
    pub fn as_package(&self) -> AurPackage {
        AurPackage {
            name: self.name.clone(),
            description: self.description.clone(),
            version: self.version.clone(),
            votes: self.votes,
            popularity: self.popularity,
            maintainer: self.maintainer.clone(),
            out_of_date: self.out_of_date != 0,
            upstream_url: self.url.clone(),
            installed: false,
            category: String::new(),
            last_modified: self.last_modified,
        }
    }
}

/// Strings of a field's array; non-strings and empty strings are skipped
/// (tolerant of a foreign schema, like [`parse_record`]).
fn str_list(rec: &Value, key: &str) -> Vec<String> {
    rec.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// One `info` record -> a passport.
pub fn parse_info_record(rec: &Value) -> AurInfo {
    AurInfo {
        name: str_or_empty(rec, "Name"),
        package_base: str_or_empty(rec, "PackageBase"),
        version: str_or_empty(rec, "Version"),
        description: str_or_empty(rec, "Description"),
        url: str_or_empty(rec, "URL"),
        maintainer: str_or_empty(rec, "Maintainer"),
        submitter: str_or_empty(rec, "Submitter"),
        votes: int_loose(rec.get("NumVotes")),
        popularity: float_loose(rec.get("Popularity")),
        first_submitted: int_loose(rec.get("FirstSubmitted")),
        last_modified: int_loose(rec.get("LastModified")),
        out_of_date: int_loose(rec.get("OutOfDate")),
        license: str_list(rec, "License"),
        keywords: str_list(rec, "Keywords"),
        groups: str_list(rec, "Groups"),
        depends: str_list(rec, "Depends"),
        make_depends: str_list(rec, "MakeDepends"),
        check_depends: str_list(rec, "CheckDepends"),
        opt_depends: str_list(rec, "OptDepends"),
        conflicts: str_list(rec, "Conflicts"),
        provides: str_list(rec, "Provides"),
        replaces: str_list(rec, "Replaces"),
    }
}

/// The passport of package `name` from an `info` response. `None` means
/// the response has no such package (removed from the AUR, a typo).
pub fn parse_info(data: &Value, name: &str) -> Option<AurInfo> {
    results_of(data)
        .find(|r| r.get("Name").and_then(Value::as_str) == Some(name))
        .map(parse_info_record)
}

/// The passport of one package via an `info` request. `Ok(None)` means the AUR does not know it.
pub fn fetch_info<IO: NetIo + ?Sized>(io: &mut IO, name: &str) -> Result<Option<AurInfo>, AurError> {
    let data = rpc_call(io, &info_url(&[name]))?;
    Ok(parse_info(&data, name))
}

// ── Package page: co-maintainers and comments ─────────────────────────────

/// How many comments per site page (`?O=` steps by 10).
pub const PAGE_COMMENTS: i64 = 10;

/// How long to keep an AUR response in memory before asking again, in
/// seconds. The card gets opened and closed repeatedly, and the site must
/// not suffer from it.
pub const DETAIL_CACHE_SECS: u64 = 15 * 60;

/// Pause between requests to AUR HTML pages, milliseconds. A page is
/// heavier than RPC, and the site's bot protection triggers precisely on
/// frequent requests in a row.
pub const PAGE_PAUSE_MS: u64 = 2_000;

/// The package page with comments; `page` is the comment page number,
/// starting from zero.
pub fn package_page_url(name: &str, page: i64) -> String {
    let base = format!("{AUR_PKG_URL_PREFIX}{}", url_quote(name));
    if page > 0 {
        format!("{base}?O={}", page * PAGE_COMMENTS)
    } else {
        base
    }
}

/// How much longer to wait before the next page request: `since_last_ms`
/// is how much time has passed since the previous one (`None` means there
/// were no requests).
pub fn page_pause_left_ms(since_last_ms: Option<u64>) -> u64 {
    match since_last_ms {
        Some(ms) if ms < PAGE_PAUSE_MS => PAGE_PAUSE_MS - ms,
        _ => 0,
    }
}

/// A comment from the package page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AurComment {
    pub author: String,
    /// As on the site: `2023-09-30 14:14 (UTC)`.
    pub date: String,
    /// Text without markup, paragraphs as newlines.
    pub text: String,
    /// Pinned by the maintainer (shown first).
    pub pinned: bool,
}

/// What could be taken from the package page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AurPage {
    pub comaintainers: Vec<String>,
    /// Pinned ones first, then the latest, as on the site.
    pub comments: Vec<AurComment>,
    /// There is a next page of comments.
    pub has_more: bool,
}

/// Why the page cannot be parsed.
#[derive(Debug, Clone, PartialEq)]
pub enum PageError {
    /// Something other than the package page arrived, usually an "are you a
    /// robot" check. It must not be bypassed: the comments are on the site.
    Blocked,
    /// No network, or the server answered with an error.
    Net(NetError),
}

/// Whether this is a package page at all: a real one has a details table.
pub fn is_package_page(html: &str) -> bool {
    html.contains("id=\"pkgdetails\"")
}

/// Parse the package page.
pub fn parse_package_page(html: &str) -> Result<AurPage, PageError> {
    if !is_package_page(html) {
        return Err(PageError::Blocked);
    }
    let mut out = AurPage {
        comaintainers: parse_comaintainers(html),
        ..AurPage::default()
    };
    // The comment sections follow each other: "Pinned Comments", then
    // "Latest Comments". A section boundary is the start of the next one.
    const SECTION: &str = "<div class=\"comments package-comments\">";
    let starts: Vec<usize> = html.match_indices(SECTION).map(|(i, _)| i).collect();
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(html.len());
        let section = &html[start..end];
        let title = between(section, "<span class=\"text\">", "</span>").unwrap_or("");
        let pinned = title.trim().eq_ignore_ascii_case("Pinned Comments");
        if !pinned && section_has_next(section) {
            out.has_more = true;
        }
        out.comments.extend(parse_comments(section, pinned));
    }
    Ok(out)
}

/// Co-maintainers: the "Maintainer:" line carries them in parentheses after
/// the main one, comma-separated.
fn parse_comaintainers(html: &str) -> Vec<String> {
    let Some(row) = between(html, "<tr class=\"pkgmaint\">", "</tr>") else {
        return Vec::new();
    };
    let cell = between(row, "<td>", "</td>").unwrap_or("");
    let text = strip_tags(cell);
    let Some(open) = text.find('(') else {
        return Vec::new();
    };
    let close = text[open..].find(')').map(|c| open + c).unwrap_or(text.len());
    text[open + 1..close]
        .split(',')
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Whether there is a "Next ›" link in the section navigation.
fn section_has_next(section: &str) -> bool {
    section.contains(">Next ›<") || section.contains(">Next &rsaquo;<")
}

/// Comments of one section.
fn parse_comments(section: &str, pinned: bool) -> Vec<AurComment> {
    const HEAD: &str = "class=\"comment-header\"";
    let mut out = Vec::new();
    let mut rest = section;
    while let Some(pos) = rest.find(HEAD) {
        let after = &rest[pos..];
        let Some(head_end) = after.find("</h4>") else { break };
        let head = &after[..head_end];
        let body_from = &after[head_end..];
        // The body is the first `article-content` block after the header.
        let text = match body_from.find("class=\"article-content\"") {
            Some(b) => {
                let block = &body_from[b..];
                let open_end = block.find('>').map(|e| e + 1).unwrap_or(0);
                let inner = div_inner(&block[open_end..]);
                html_to_text(inner)
            }
            None => String::new(),
        };
        // "nick commented on <date>": the nick comes before " commented on ",
        // the date is the text of the link with the `date` class. An edit
        // ("edited on …") does not belong to the date. `head` starts inside the
        // opening `<h4 …>` tag; its tail up to `>` is cut off.
        let head_body = head.split_once('>').map(|(_, b)| b).unwrap_or(head);
        let head_text =
            strip_tags(head_body.split("<span class=\"edited\">").next().unwrap_or(head_body));
        let author = head_text
            .split(" commented on ")
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let date = between(head, "class=\"date\">", "</a>")
            .map(|d| strip_tags(d).trim().to_string())
            .unwrap_or_default();
        out.push(AurComment { author, date, text, pinned });
        rest = &after[head_end..];
    }
    out
}

/// Contents of a `<div>` up to its matching closing tag: the body may have
/// nested `<div>`s, and the very first `</div>` would cut the text.
fn div_inner(s: &str) -> &str {
    let mut depth = 1i32;
    let mut i = 0;
    let lower = s.to_ascii_lowercase();
    while i < s.len() {
        let Some(rel) = lower[i..].find("<") else { break };
        let at = i + rel;
        if lower[at..].starts_with("</div") {
            depth -= 1;
            if depth == 0 {
                return &s[..at];
            }
        } else if lower[at..].starts_with("<div") {
            depth += 1;
        }
        i = at + 1;
    }
    s
}

/// The substring between the first `open` and the `close` that follows it.
fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let a = s.find(open)? + open.len();
    let b = s[a..].find(close)? + a;
    Some(&s[a..b])
}

/// Tags removed, entities turned into characters. Whitespace is left alone.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    crate::translator::html_unescape(&out)
}

/// Comment markup (Markdown rendered by the site) -> plain text: paragraphs
/// and lines as newlines, list items as "• ", code in `<pre>` as is, with
/// its newlines. A link stays as its text.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    let mut in_pre = false;
    let bytes = html.as_bytes();
    while i < html.len() {
        if bytes[i] == b'<' {
            let end = html[i..].find('>').map(|e| i + e + 1).unwrap_or(html.len());
            let tag = html[i..end].to_ascii_lowercase();
            let name: String = tag
                .trim_start_matches('<')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '/')
                .collect();
            match name.as_str() {
                "br" | "br/" => out.push('\n'),
                "p" | "/p" | "div" | "/div" | "ul" | "/ul" | "ol" | "/ol" | "blockquote"
                | "/blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "/h1" | "/h2"
                | "/h3" | "/h4" | "/h5" | "/h6" | "hr" => out.push_str("\n\n"),
                "li" => out.push_str("\n• "),
                "/li" => out.push('\n'),
                "pre" => {
                    out.push_str("\n\n");
                    in_pre = true;
                }
                "/pre" => {
                    out.push_str("\n\n");
                    in_pre = false;
                }
                _ => {}
            }
            i = end;
            continue;
        }
        let next = html[i..].find('<').map(|n| i + n).unwrap_or(html.len());
        let chunk = crate::translator::html_unescape(&html[i..next]);
        if in_pre {
            out.push_str(&chunk);
        } else {
            // Outside `<pre>` a source newline is just a space.
            let flat = chunk.replace(['\n', '\r', '\t'], " ");
            let mut prev_space = out.ends_with([' ', '\n']) || out.is_empty();
            for c in flat.chars() {
                if c == ' ' {
                    if !prev_space {
                        out.push(' ');
                    }
                    prev_space = true;
                } else {
                    out.push(c);
                    prev_space = false;
                }
            }
        }
        i = next;
    }
    // Trailing spaces of lines and extra empty lines removed.
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    let mut text = String::new();
    let mut blank = 0;
    for line in lines {
        if line.trim().is_empty() {
            blank += 1;
            continue;
        }
        if !text.is_empty() {
            text.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        text.push_str(line);
    }
    text
}

/// Download and parse the package page. The pause between requests is the
/// caller's concern ([`page_pause_left_ms`]).
pub fn fetch_package_page<IO: NetIo + ?Sized>(
    io: &mut IO,
    name: &str,
    page: i64,
) -> Result<AurPage, PageError> {
    let raw = io
        .get(&package_page_url(name, page), USER_AGENT, TIMEOUT_SECS)
        .map_err(PageError::Net)?;
    parse_package_page(&String::from_utf8_lossy(&raw))
}

// ── Card picture ─────────────────────────────────────────────────────────

/// The marker "the picture must be taken from the project page's
/// `og:image`": the loader resolves it ([`resolve_og_image`]), not the
/// image cache.
pub const OG_IMAGE_SCHEME: &str = "og-image:";

/// Hosts where the project page carries `og:image` (GitLab and Forgejo).
fn og_image_host(host: &str) -> bool {
    host.contains("gitlab")
        || matches!(
            host,
            "codeberg.org" | "invent.kde.org" | "framagit.org" | "salsa.debian.org"
        )
}

/// The host of an address (`https://host/...` -> `host`), lower case.
fn url_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Package card pictures in display order.
///
/// * GitHub: the first README picture (the `github-readme:` marker, looked
///   up by the loader; the same as on the tile). The repository OpenGraph
///   card (name and stars) is not used: it is not a screenshot of the item
///   (16.11.2026);
/// * GitLab/Codeberg: the project page's `og:image` (the `og-image:`
///   marker);
/// * otherwise nothing: the card shows the same icon as the tile.
pub fn detail_image_sources(upstream_url: &str) -> Vec<String> {
    if let Some((owner, repo)) = github_owner_repo(upstream_url) {
        return vec![format!("{}{owner}/{repo}", README_PREVIEW_SCHEME)];
    }
    match url_host(upstream_url) {
        Some(host) if og_image_host(&host) => vec![format!("{OG_IMAGE_SCHEME}{upstream_url}")],
        _ => Vec::new(),
    }
}

/// `og:image` from the page HTML; a relative address is resolved against `page_url`.
pub fn parse_og_image(html: &str, page_url: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find("<meta") {
        let start = from + rel;
        let end = lower[start..].find('>').map(|e| start + e).unwrap_or(lower.len());
        let tag = &html[start..end];
        let tag_l = &lower[start..end];
        let is_og = ["property=\"og:image\"", "property='og:image'", "name=\"og:image\""]
            .iter()
            .any(|m| tag_l.contains(m));
        if is_og {
            if let Some(v) = attr_value(tag, tag_l, "content") {
                let v = crate::translator::html_unescape(v.trim());
                if !v.is_empty() {
                    return Some(absolute_url(&v, page_url));
                }
            }
        }
        from = end;
    }
    None
}

/// The value of a tag attribute (in quotes of any kind).
fn attr_value<'a>(tag: &'a str, tag_l: &str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=");
    let mut search = 0;
    while let Some(rel) = tag_l[search..].find(&key) {
        let at = search + rel;
        // The start of an attribute name, not the tail of another one (`data-content=`).
        let ok_start = at == 0 || tag_l.as_bytes()[at - 1].is_ascii_whitespace();
        let val = &tag[at + key.len()..];
        if ok_start {
            return match val.chars().next() {
                Some(q @ ('"' | '\'')) => val[1..].split(q).next(),
                _ => val.split([' ', '>', '/']).next(),
            };
        }
        search = at + key.len();
    }
    None
}

/// A relative picture address -> an absolute one relative to the page.
fn absolute_url(link: &str, page_url: &str) -> String {
    if link.starts_with("https://") || link.starts_with("http://") {
        return link.to_string();
    }
    if let Some(rest) = link.strip_prefix("//") {
        return format!("https://{rest}");
    }
    let scheme_host = match url_host(page_url) {
        Some(h) => format!("https://{h}"),
        None => return link.to_string(),
    };
    if link.starts_with('/') {
        format!("{scheme_host}{link}")
    } else {
        format!("{}/{link}", page_url.trim_end_matches('/'))
    }
}

/// Find the picture for the `og-image:<page address>` marker.
pub fn resolve_og_image(net: &mut dyn NetIo, marker: &str) -> Option<String> {
    let page = marker.strip_prefix(OG_IMAGE_SCHEME)?;
    let body = net.get(page, USER_AGENT, TIMEOUT_SECS).ok()?;
    parse_og_image(&String::from_utf8_lossy(&body), page)
}

/// The marker "the picture must be found in the GitHub repository README"
/// (`github-readme:owner/name`): the loader resolves it
/// ([`resolve_readme_preview`]), not the image cache.
pub const README_PREVIEW_SCHEME: &str = "github-readme:";

/// Badges and other small stuff that is not a picture of the project.
const BADGE_MARKERS: &[&str] = &[
    "shields.io", "badge", "badgen", "/workflows/", "travis-ci", "circleci", "codecov",
    "ko-fi.com/img", "buymeacoffee", "liberapay", "paypal", "sponsor", "opencollective",
    "star-history", "contrib.rocks", "visitor", "hits.", "count.getloli", "komarev",
    "avatars.githubusercontent",
];

/// All picture links in the README text in order: `![…](address)` and
/// `<img src="address">`.
fn image_links(md: &str) -> Vec<String> {
    let mut found: Vec<(usize, String)> = Vec::new();
    let mut i = 0;
    while let Some(p) = md[i..].find("![") {
        let start = i + p;
        let rest = &md[start..];
        if let Some(close) = rest.find("](") {
            let after = &rest[close + 2..];
            if let Some(end) = after.find(')') {
                let inner = after[..end].trim();
                let link = inner.split_whitespace().next().unwrap_or("").trim_matches(['<', '>']);
                if !link.is_empty() {
                    found.push((start, link.to_string()));
                }
            }
        }
        i = start + 2;
    }
    let lower = md.to_lowercase();
    let mut j = 0;
    while let Some(p) = lower[j..].find("<img") {
        let start = j + p;
        let tag_end = lower[start..].find('>').map(|e| start + e).unwrap_or(lower.len());
        let tag = &md[start..tag_end];
        let tag_l = &lower[start..tag_end];
        if let Some(sp) = tag_l.find("src=") {
            let val = &tag[sp + 4..];
            let link = match val.chars().next() {
                Some(q @ ('"' | '\'')) => val[1..].split(q).next().unwrap_or(""),
                _ => val.split([' ', '>']).next().unwrap_or(""),
            };
            if !link.is_empty() {
                found.push((start, link.to_string()));
            }
        }
        j = start + 4;
    }
    found.sort_by_key(|(pos, _)| *pos);
    found.into_iter().map(|(_, l)| l).collect()
}

/// A link from the README -> a direct picture address: a relative one goes
/// to the branch's `raw.githubusercontent.com`, `github.com/…/blob/…` to
/// its "raw" form. Other addresses as is.
pub fn absolutize(link: &str, full_name: &str, branch: &str) -> Option<String> {
    let link = link.trim();
    if link.is_empty() || link.starts_with('#') || link.starts_with("data:") {
        return None;
    }
    if let Some(rest) = link.strip_prefix("https://github.com/").or_else(|| link.strip_prefix("http://github.com/")) {
        // owner/repo/blob/ref/path → raw
        let parts: Vec<&str> = rest.splitn(5, '/').collect();
        if parts.len() == 5 && (parts[2] == "blob" || parts[2] == "raw") {
            let path = parts[4].split(['?', '#']).next().unwrap_or("");
            return Some(format!("https://raw.githubusercontent.com/{}/{}/{}/{}", parts[0], parts[1], parts[3], path));
        }
        return Some(link.to_string());
    }
    if link.starts_with("https://") || link.starts_with("http://") {
        return Some(link.to_string());
    }
    if link.starts_with("//") {
        return Some(format!("https:{link}"));
    }
    let path = link.trim_start_matches("./").trim_start_matches('/');
    let path = path.split(['?', '#']).next().unwrap_or("");
    if path.is_empty() || path.starts_with("../") {
        return None;
    }
    Some(format!("https://raw.githubusercontent.com/{full_name}/{branch}/{path}"))
}

/// The first README picture that is not a badge: `None` means there are no
/// pictures. SVG is taken only if there is nothing else (not every loader
/// shows them, and projects usually have a PNG or GIF).
pub fn readme_first_image(md: &str, full_name: &str, branch: &str) -> Option<String> {
    let all: Vec<String> = image_links(md)
        .into_iter()
        .filter(|l| {
            let l = l.to_lowercase();
            !BADGE_MARKERS.iter().any(|b| l.contains(b))
        })
        .filter_map(|l| absolutize(&l, full_name, branch))
        .collect();
    let is_svg = |u: &String| u.to_lowercase().split(['?', '#']).next().unwrap_or("").ends_with(".svg");
    all.iter().find(|u| !is_svg(u)).or_else(|| all.first()).cloned()
}

/// Find the picture for the `github-readme:owner/name` marker: read the
/// README of the default branch (`HEAD`) and take the first picture.
pub fn resolve_readme_preview(net: &mut dyn NetIo, marker: &str) -> Option<String> {
    let full = marker.strip_prefix(README_PREVIEW_SCHEME)?;
    let branch = "HEAD";
    for name in ["README.md", "readme.md", "README.MD", "Readme.md"] {
        let url = format!("https://raw.githubusercontent.com/{full}/{branch}/{name}");
        if let Ok(body) = net.get(&url, USER_AGENT, TIMEOUT_SECS) {
            return readme_first_image(&String::from_utf8_lossy(&body), full, branch);
        }
    }
    None
}

// ── Card lines ───────────────────────────────────────────────────────────

/// A card details line: label, value, warning, link.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AurFact {
    pub label: String,
    pub value: String,
    /// Paint as a warning ("flagged out of date").
    pub warn: bool,
    /// The value is an address that can be followed.
    pub link: bool,
}

/// The day from an epoch, UTC: `2026-06-19`. 0 and below give an empty string.
pub fn epoch_day(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    crate::engine_update::now_iso(ts).chars().take(10).collect()
}

/// An optional dependency: "package: why" -> "package — why".
pub fn opt_depend_line(raw: &str) -> String {
    match raw.split_once(':') {
        Some((pkg, why)) if !why.trim().is_empty() => format!("{} — {}", pkg.trim(), why.trim()),
        Some((pkg, _)) => pkg.trim().to_string(),
        None => raw.trim().to_string(),
    }
}

/// All card details lines in order. Empty fields are not shown.
pub fn info_facts(info: &AurInfo, comaintainers: &[String]) -> Vec<AurFact> {
    use crate::i18n::{tr, trf};
    let mut out: Vec<AurFact> = Vec::new();
    let mut push = |label: String, value: String| {
        if !value.trim().is_empty() {
            out.push(AurFact { label, value, warn: false, link: false });
        }
    };
    let list = |v: &[String]| v.join(", ");
    if info.out_of_date > 0 {
        // On the first line: this is the main thing to know before building.
        push(tr("Status"), trf("⚠️ Flagged out-of-date since %1", &[&epoch_day(info.out_of_date)]));
    }
    push(tr("Version"), info.version.clone());
    push(
        tr("Maintainer"),
        if info.maintainer.is_empty() { tr("none (orphaned package)") } else { info.maintainer.clone() },
    );
    push(tr("Co-maintainers"), list(comaintainers));
    push(tr("Votes"), info.votes.to_string());
    push(tr("Popularity"), crate::i18n::format_decimal(info.popularity, 2));
    push(tr("First submitted"), epoch_day(info.first_submitted));
    push(tr("Last updated"), epoch_day(info.last_modified));
    push(tr("License"), list(&info.license));
    push(tr("Project page"), info.url.clone());
    push(tr("Depends on"), list(&info.depends));
    push(tr("Build dependencies"), list(&info.make_depends));
    push(tr("Test dependencies"), list(&info.check_depends));
    push(
        tr("Optional"),
        info.opt_depends.iter().map(|d| opt_depend_line(d)).collect::<Vec<_>>().join("\n"),
    );
    push(tr("Conflicts with"), list(&info.conflicts));
    push(tr("Provides"), list(&info.provides));
    push(tr("Replaces"), list(&info.replaces));
    push(tr("Keywords"), list(&info.keywords));
    if !info.package_base.is_empty() && info.package_base != info.name {
        push(tr("Package base"), info.package_base.clone());
    }
    let project = tr("Project page");
    let status = tr("Status");
    for f in &mut out {
        if f.label == project {
            f.link = true;
        }
        if f.label == status && info.out_of_date > 0 {
            f.warn = true;
        }
    }
    out
}

/// The pinned comment marker before the nick.
pub const PINNED_MARK: &str = "📌 ";

/// The label instead of comments when the package page cannot be obtained.
pub fn comments_on_site_text() -> String {
    crate::i18n::tr("AUR didn't let us read the comments here (bot protection or no connection) — they are on the website.")
}

/// The "Details" button on a package tile.
pub fn details_btn() -> String {
    crate::i18n::tr("📄 Details")
}

/// Its tooltip.
pub fn details_tooltip() -> String {
    crate::i18n::tr("Package card: description, dependencies, comments")
}

/// The package was not found in the AUR (removed or renamed).
pub fn info_missing_text() -> String {
    crate::i18n::tr("AUR doesn't know this package anymore — it may have been removed or renamed.")
}
