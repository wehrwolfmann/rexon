//! Data models of the appearance catalog — port of `rexon/appearance/models.py`.
//!
//! `ThemeItem`     — one OCS content item (theme/icons/cursor/…).
//! `ThemeCategory` — a catalog category (cat_id, install path, apply command).
//!
//! The field order of `ThemeItem` matches the dataclass declaration — the key
//! order in `favorites.json` (`dataclasses.asdict`) depends on it, so it is
//! load-bearing and checked byte for byte (golden test `models_parity.rs`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One content item from the OCS API.
///
/// All fields have defaults (`#[serde(default)]`): on deserialization missing
/// ones are filled in as in the Python dataclass, and serde silently drops
/// unknown keys — an exact analogue of
/// `ThemeItem(**{k: v ... if k in _FIELDS})`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeItem {
    /// content_id on store.kde.org
    pub id: String,
    pub name: String,
    pub description: String,
    /// translation (argos, after the MVP)
    pub description_translated: String,
    pub author: String,
    pub license: String,
    pub version: String,
    /// 0-100 (OCS score)
    pub rating: i64,
    pub downloads: i64,
    /// URL of the large preview
    pub preview_url: String,
    /// ALL screenshots previewpic1..12 (gallery of the detail view)
    pub preview_urls: Vec<String>,
    /// path to the downloaded preview (cache)
    pub preview_local: String,
    /// URL for downloading the archive
    pub download_url: String,
    /// archive file name
    pub download_name: String,
    /// archive size in KiB — as the store reports it (`downloadsize1`)
    pub download_size: i64,
    /// OCS typeid (cat_id)
    pub category_id: i64,
    /// creation date (ISO)
    pub created: String,
    /// update date (ISO)
    pub updated: String,
    /// installed locally?
    pub installed: bool,
    /// Whether it is applied RIGHT NOW (matches the active theme of its kind).
    /// Installed and applied are DIFFERENT states: a theme can be downloaded with
    /// the "Download only" button and not enabled.
    pub applied: bool,
    /// install path (if installed)
    pub install_path: String,
    /// All downloadable files (for choosing among multiple archives)
    pub download_links: Vec<Value>,

    /// The runtime attribute `_local_kind` from Python: not serialized (the
    /// analogue of an underscored field that is absent from `dataclasses.fields`).
    /// Filled in by `favorites::list_favorite_items` for local themes.
    #[serde(skip)]
    pub local_kind: Option<String>,

    /// Name of the theme on disk WITHOUT translation (`Name=` without a language):
    /// "High Contrast" for the card «Высококонтрастная». Needed only by search — a
    /// person searches both by what they see and by what the theme is called in
    /// the original. Empty when it matches the displayed name or there is no theme
    /// on disk. Not written to files (the field order of `favorites.json` is
    /// load-bearing); between the tab's threads it travels as a separate
    /// `sourceName` key.
    #[serde(skip)]
    pub source_name: String,
}

impl ThemeItem {
    /// Description for display: the translation if there is one, otherwise the original.
    pub fn display_description(&self) -> &str {
        if self.description_translated.is_empty() {
            &self.description
        } else {
            &self.description_translated
        }
    }

    /// Rating 0-100 → 0.0-5.0 stars, like Python's `round(rating / 20.0, 1)`.
    ///
    /// Python's round rounds to nearest with ties to even based on the TRUE value
    /// of the double (e.g. rating=25 → 1.25 → 1.2, while rating=1 → 0.05, which as
    /// a double is slightly above 0.05 → 0.1). Rust's `format!("{:.1}", …)` uses
    /// the same semantics (round-half-to-even on the true value), so formatting
    /// followed by parsing back gives byte-for-byte the same numbers — checked on
    /// all 101 values (golden test).
    pub fn rating_stars(&self) -> f64 {
        Self::stars_of(self.rating)
    }

    /// Stars if the product has a rating; `None` — there are no votes
    /// ([`crate::ocs::UNRATED_SCORE`]).
    pub fn rated_stars(&self) -> Option<f64> {
        (self.rating != crate::ocs::UNRATED_SCORE).then(|| self.rating_stars())
    }

    fn stars_of(rating: i64) -> f64 {
        format!("{:.1}", rating as f64 / 20.0)
            .parse()
            .expect("формат {:.1} всегда парсится обратно в f64")
    }
}

/// A category of the appearance catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeCategory {
    /// OCS cat_id (0 = virtual)
    pub id: i64,
    /// internal key (popular/new/global/…)
    pub key: String,
    /// human-readable name (RU)
    pub name: String,
    /// emoji for the sidebar
    pub icon: String,
    /// install path (~ is expanded)
    pub install_dir: String,
    /// apply type (global/plasma/colors/…)
    pub apply_kind: String,
    /// whether pkexec is needed (SDDM/Plymouth)
    pub requires_root: bool,
    /// Virtual sidebar categories (popular/new/active/downloaded/system) have no
    /// cat_id of their own and do not go to the network.
    ///
    /// In Python the field is called `virtual` (reserved in Rust) — on
    /// serialization we rename it back so that the `asdict` output matches byte for
    /// byte (golden test `categories_parity.rs`).
    #[serde(rename = "virtual")]
    pub virtual_: bool,
    /// Where the section gets its online catalog from ([`CatalogSource`]). The
    /// section number (`id`) no longer says anything about it: the "Window borders
    /// (XFCE)" section has number 906 (the journal and favorites rely on it), while
    /// its catalog is store category 138.
    pub store: CatalogSource,
    /// For popular/new — the default sort: new|down|high|alpha
    pub default_sort: String,
}

impl ThemeCategory {
    /// Create a category with the required fields; the rest are dataclass defaults
    /// (empty paths/flags, `default_sort = "new"`).
    pub fn new(id: i64, key: &str, name: &str, icon: &str) -> Self {
        Self {
            id,
            key: key.to_string(),
            name: name.to_string(),
            icon: icon.to_string(),
            install_dir: String::new(),
            apply_kind: String::new(),
            requires_root: false,
            virtual_: false,
            store: CatalogSource::Ocs(vec![id]),
            default_sort: "new".to_string(),
        }
    }

    /// A section without an online catalog: it shows only what is on disk.
    pub fn local_only(&self) -> bool {
        self.store == CatalogSource::None
    }

    /// Numbers of the OCS store categories the section's catalog is made of
    /// (several — an "umbrella": OCS accepts "id1,id2"). Empty — the catalog is not
    /// from OCS or there is none.
    pub fn ocs_cats(&self) -> &[i64] {
        match &self.store {
            CatalogSource::Ocs(cats) => cats,
            _ => &[],
        }
    }
}

/// Source of the online catalog of an "Appearance" section.
///
/// AUR is not a section source but an additional shelf INSIDE a section that
/// has a catalog (the "Store:" switch, `appearance_page::source_keys_for`).
///
/// Cinnamon Spices is used by five Cinnamon sections (`crate::cinnamon_spices`),
/// extensions.gnome.org by the "GNOME Extensions" section (`crate::gnome_ego`).
/// Warning: cosmic-themes.org is only named for now: the page has no client for
/// it yet, and no section gets it. The tab would consider a section with such a
/// source a network one — it may only be added together with the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSource {
    /// No catalog — disk only.
    None,
    /// OCS store (KDE Store, pling): category numbers.
    Ocs(Vec<i64>),
    /// The combined "Popular"/"New" storefront: the commonly used OCS categories of
    /// this environment (`categories::aggregate_cats`).
    OcsShowcase,
    /// Cinnamon Spices (`cinnamon-spices.linuxmint.com`): the catalog kind —
    /// `themes`, `applets`, `desklets`, `extensions`, `actions`.
    Spices(String),
    /// extensions.gnome.org (`crate::gnome_ego`).
    GnomeExtensions,
    /// cosmic-themes.org.
    CosmicThemes,
}

/// Serialization to JSON with a 1-space indent — like Python's
/// `json.dumps(obj, ensure_ascii=False, indent=1)` (the `favorites.json` format).
///
/// serde_json's `to_string_pretty` hard-codes 2 spaces, while the favorites
/// store is written with `indent=1`, so `PrettyFormatter::with_indent(b" ")` is
/// needed. The key-value separator `": "` and the item separator `,\n` in serde
/// match Python with an indent set (Cyrillic is not escaped —
/// `ensure_ascii=False`).
pub fn to_json_indent1(value: &Value) -> String {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value
        .serialize(&mut ser)
        .expect("serde_json::Value всегда сериализуется");
    String::from_utf8(buf).expect("serde_json пишет валидный UTF-8")
}
