//! Two-way palette between Rexon and a third-party shell (Noctalia,
//! DankMaterialShell).
//!
//! WHY. Both shells color their own panel and, more importantly, set the
//! dark or light mode for applications themselves: Noctalia — UNCONDITIONALLY, at startup and
//! after every theme change (`gsettings … color-scheme prefer-dark|prefer-light`),
//! DMS — with `syncModeWithPortal` enabled, and it is enabled by default.
//! A scheme applied in Rexon with a different mode would last until the first theme
//! change in the shell. Staging a write race is pointless, so the person
//! gets the choice «who leads the palette», and the two sides are fitted to each other:
//!
//!   * **From Rexon to the shell.** The palette of the current color scheme (the colors
//!     `plasma-apply-colorscheme` puts into `kdeglobals`) goes to the shell by its own
//!     standard path, and the shell's MODE is set to the same as the scheme's. Then
//!     the shell writes into `color-scheme` exactly what the scheme implies —
//!     there is no race because there is nothing to argue about.
//!       - Noctalia: our own file `palettes/Rexon.json` (the v5 «palette» format) and in
//!         the state file `settings.toml` the keys `[theme] source = "custom"`,
//!         `custom_palette = "Rexon"`, `mode`. The state file is loaded
//!         last and overrides `~/.config/noctalia/*.toml` — the shell's GUI and its
//!         `noctalia msg color-scheme-set / theme-mode-set` write there too.
//!       - DMS: our own theme file `rexon-palette.json` (the DMS «custom theme» format:
//!         `dark`/`light`) and in `settings.json` the keys `currentThemeName =
//!         "custom"`, `currentThemeCategory = "custom"`, `customThemeFile`;
//!         the mode is `isLightMode` in `session.json`. The shell rereads both
//!         files itself.
//!   * **From the shell to Rexon.** The shell's colors become a KDE color scheme
//!     (`Rexon — Shell Colors`) and are applied the same way as any
//!     scheme from the «Color Schemes» section.
//!       - DMS saves the full Material palette after every theme build
//!         into `~/.cache/DankMaterialShell/dms-colors.json` (needs `matugen`);
//!         with a custom theme its file is read.
//!       - Noctalia does not put its palette outside by itself (built-in palettes
//!         live in the binary, the palette from the wallpaper — in memory). It hands it out only
//!         through templates, so Rexon, with the person's consent, adds ONE
//!         template of its own `[theme.templates.user.rexon_palette]`: the shell fills
//!         it on every palette change, and the file is always fresh. With a custom
//!         palette (`source = "custom"`) its file is read directly.
//!
//! The write rule is the same as for shell wallpapers (`desktop_shell`): ONLY the
//! listed keys are edited, the rest of the file stays byte for byte; the result
//! is verified by parsing (otherwise a refusal, not corruption); before the first edit a
//! backup `*.rexon-palette-backup` is put next to it; writing is by renaming
//! (`utils::atomic_write`). What existed before Rexon is remembered in
//! `$XDG_STATE_HOME/rexon/shell-palette.json` BEFORE writing to the shell's file.
//! «Return as It Was»: if nobody has touched the file since — the previous
//! content is restored in full, bit for bit; if the shell has rewritten it —
//! only our keys are restored, other edits stay.
//!
//! ⚠️ Boundary: neither shell is installed on the development machine. Formats and
//! keys were checked against the sources of noctalia v5.2.0 and DMS v1.6.2 (research
//! 01.10.2026), behavior — with fake files (`tests/shell_palette.rs`).
//! That the shell really rereads the files on the fly and that Noctalia
//! accepts a template from the state file has NOT been verified live.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::i18n::{tr, trf};
use crate::system::DesktopShell;

// ============================================================================
// Color
// ============================================================================

/// A color without transparency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Parse `#rgb`, `#rrggbb`, `#rrggbbaa` (transparency is dropped) and
    /// the KDE form `r,g,b` / `r,g,b,a`.
    pub fn parse(text: &str) -> Option<Rgb> {
        let t = text.trim();
        if let Some(hex) = t.strip_prefix('#') {
            let full: String = match hex.len() {
                3 => hex.chars().flat_map(|c| [c, c]).collect(),
                6 | 8 => hex[..6].to_string(),
                _ => return None,
            };
            let v = u32::from_str_radix(&full, 16).ok()?;
            return Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8));
        }
        let parts: Vec<&str> = t.split(',').map(str::trim).collect();
        if parts.len() == 3 || parts.len() == 4 {
            let c = |s: &str| s.parse::<u8>().ok();
            return Some(Rgb(c(parts[0])?, c(parts[1])?, c(parts[2])?));
        }
        None
    }

    /// `#rrggbb`.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// `r,g,b` — that is how colors are written in KDE schemes.
    pub fn kde(self) -> String {
        format!("{},{},{}", self.0, self.1, self.2)
    }

    /// Relative luminance per WCAG (0 — black, 1 — white).
    pub fn luminance(self) -> f64 {
        let lin = |c: u8| {
            let c = c as f64 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }

    /// Mix: `t = 0` — this color, `t = 1` — `other`.
    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        let m = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round().clamp(0.0, 255.0) as u8;
        Rgb(m(self.0, other.0), m(self.1, other.1), m(self.2, other.2))
    }

    /// Readable text over this color: black or white, whichever has the higher
    /// contrast.
    pub fn readable_on(self) -> Rgb {
        let l = self.luminance();
        if (l + 0.05) / 0.05 >= 1.05 / (l + 0.05) {
            Rgb(0, 0, 0)
        } else {
            Rgb(255, 255, 255)
        }
    }
}

const BLACK: Rgb = Rgb(0, 0, 0);

// ============================================================================
// Palette
// ============================================================================

/// A palette by roles — the same 16 roles as Noctalia's (`mPrimary` …
/// `mOnHover`); DMS and a KDE scheme map onto them without losing meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// A dark palette (background darker than text).
    pub dark: bool,
    pub primary: Rgb,
    pub on_primary: Rgb,
    pub secondary: Rgb,
    pub on_secondary: Rgb,
    pub tertiary: Rgb,
    pub on_tertiary: Rgb,
    pub error: Rgb,
    pub on_error: Rgb,
    pub surface: Rgb,
    pub on_surface: Rgb,
    pub surface_variant: Rgb,
    pub on_surface_variant: Rgb,
    pub outline: Rgb,
    pub shadow: Rgb,
    pub hover: Rgb,
    pub on_hover: Rgb,
}

impl Palette {
    /// Roles as pairs «Noctalia name (`mPrimary`) → color» in the order of its
    /// documentation.
    pub fn noctalia_roles(&self) -> [(&'static str, Rgb); 16] {
        [
            ("mPrimary", self.primary),
            ("mOnPrimary", self.on_primary),
            ("mSecondary", self.secondary),
            ("mOnSecondary", self.on_secondary),
            ("mTertiary", self.tertiary),
            ("mOnTertiary", self.on_tertiary),
            ("mError", self.error),
            ("mOnError", self.on_error),
            ("mSurface", self.surface),
            ("mOnSurface", self.on_surface),
            ("mSurfaceVariant", self.surface_variant),
            ("mOnSurfaceVariant", self.on_surface_variant),
            ("mOutline", self.outline),
            ("mShadow", self.shadow),
            ("mHover", self.hover),
            ("mOnHover", self.on_hover),
        ]
    }

    /// Assemble by roles with fallback values for those the source did not
    /// give. `get` answers by Material name (`on_primary`, `surface` …).
    fn from_roles(dark_hint: Option<bool>, get: impl Fn(&str) -> Option<Rgb>) -> Option<Palette> {
        let surface = get("surface")?;
        let on_surface = get("on_surface")?;
        let primary = get("primary")?;
        let dark = dark_hint.unwrap_or(surface.luminance() < on_surface.luminance());
        let secondary = get("secondary").unwrap_or(primary);
        let tertiary = get("tertiary").unwrap_or(secondary);
        let error = get("error").unwrap_or(Rgb(0xda, 0x44, 0x53));
        let surface_variant = get("surface_variant").unwrap_or(surface.mix(on_surface, 0.12));
        let on_surface_variant = get("on_surface_variant").unwrap_or(on_surface.mix(surface, 0.3));
        Some(Palette {
            dark,
            primary,
            on_primary: get("on_primary").unwrap_or(primary.readable_on()),
            secondary,
            on_secondary: get("on_secondary").unwrap_or(secondary.readable_on()),
            tertiary,
            on_tertiary: get("on_tertiary").unwrap_or(tertiary.readable_on()),
            error,
            on_error: get("on_error").unwrap_or(error.readable_on()),
            surface,
            on_surface,
            surface_variant,
            on_surface_variant,
            outline: get("outline").unwrap_or(on_surface.mix(surface, 0.55)),
            shadow: get("shadow").unwrap_or(BLACK),
            hover: get("hover").unwrap_or(surface.mix(on_surface, 0.08)),
            on_hover: get("on_hover").unwrap_or(on_surface),
        })
    }
}

// ============================================================================
// KDE scheme ⇄ palette
// ============================================================================

/// KDE INI groups: `[Colors:Window]` → key → value. A repeated key —
/// the last one wins, as in KConfig.
fn kde_groups(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut group = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            group = line[1..line.len() - 1].to_string();
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.entry(group.clone())
                .or_default()
                .insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

/// Palette from the text of a KDE scheme (`*.colors`) or `kdeglobals` — that is where
/// `plasma-apply-colorscheme` puts the same `[Colors:*]` groups.
///
/// `None` — the text has no window colors (the scheme was never applied).
pub fn palette_from_kde(text: &str) -> Option<Palette> {
    let g = kde_groups(text);
    let c = |group: &str, key: &str| g.get(group).and_then(|m| m.get(key)).and_then(|v| Rgb::parse(v));
    let surface = c("Colors:Window", "BackgroundNormal")?;
    let on_surface = c("Colors:Window", "ForegroundNormal")?;
    // Accent: `AccentColor` (Plasma 6, «custom accent color») outranks
    // the selection — the scheme may have kept its own selection.
    let primary = c("General", "AccentColor")
        .or_else(|| c("Colors:Selection", "BackgroundNormal"))
        .or_else(|| c("Colors:View", "DecorationFocus"))?;
    let on_primary = c("Colors:Selection", "ForegroundNormal").unwrap_or(primary.readable_on());
    let get = |role: &str| -> Option<Rgb> {
        match role {
            "surface" => Some(surface),
            "on_surface" => Some(on_surface),
            "primary" => Some(primary),
            "on_primary" => Some(on_primary),
            "secondary" => c("Colors:View", "DecorationFocus").filter(|x| *x != primary)
                .or_else(|| c("Colors:Window", "ForegroundLink")),
            "tertiary" => c("Colors:Window", "ForegroundVisited"),
            "error" => c("Colors:Window", "ForegroundNegative"),
            "surface_variant" => c("Colors:Button", "BackgroundNormal")
                .or_else(|| c("Colors:Window", "BackgroundAlternate")),
            "on_surface_variant" => c("Colors:Window", "ForegroundInactive"),
            _ => None,
        }
    };
    Palette::from_roles(None, get)
}

/// Label of the scheme Rexon assembles from the shell's colors.
pub fn shell_scheme_display_name() -> String {
    // TRANSLATORS: name of the KDE color scheme Rexon builds from the desktop shell's colors (shown in KDE's color settings).
    tr("Rexon — Shell Colors")
}

/// KDE scheme text from a palette. `id` — the file name without `.colors` (also
/// `ColorScheme`), `name` — the label in the scheme list.
pub fn kde_scheme_text(p: &Palette, id: &str, name: &str) -> String {
    let bg = p.surface;
    let fg = p.on_surface;
    let alt = p.surface_variant;
    let inactive = p.on_surface_variant;
    let accent = p.primary;
    let view_bg = if p.dark { bg.mix(BLACK, 0.12) } else { bg.mix(Rgb(255, 255, 255), 0.5) };
    let positive = if p.dark { Rgb(0x27, 0xae, 0x60) } else { Rgb(0x1e, 0x8f, 0x4e) };
    let neutral = if p.dark { Rgb(0xf6, 0x74, 0x00) } else { Rgb(0xc8, 0x5c, 0x00) };
    let group = |name: &str, back: Rgb, back_alt: Rgb, front: Rgb| {
        format!(
            "[{name}]\nBackgroundAlternate={}\nBackgroundNormal={}\nDecorationFocus={}\n\
             DecorationHover={}\nForegroundActive={}\nForegroundInactive={}\nForegroundLink={}\n\
             ForegroundNegative={}\nForegroundNeutral={}\nForegroundNormal={}\n\
             ForegroundPositive={}\nForegroundVisited={}\n\n",
            back_alt.kde(),
            back.kde(),
            accent.kde(),
            accent.kde(),
            accent.kde(),
            inactive.kde(),
            accent.kde(),
            p.error.kde(),
            neutral.kde(),
            front.kde(),
            positive.kde(),
            p.tertiary.kde(),
        )
    };
    let mut out = String::new();
    out.push_str(&group("Colors:Button", alt, bg, fg));
    out.push_str(&group("Colors:Complementary", bg, alt, fg));
    out.push_str(&group("Colors:Header", bg, alt, fg));
    out.push_str(&group("Colors:Header][Inactive", bg, alt, fg));
    // Selection: text over the accent, «inactive» is the same text.
    out.push_str(&format!(
        "[Colors:Selection]\nBackgroundAlternate={}\nBackgroundNormal={}\nDecorationFocus={}\n\
         DecorationHover={}\nForegroundActive={}\nForegroundInactive={}\nForegroundLink={}\n\
         ForegroundNegative={}\nForegroundNeutral={}\nForegroundNormal={}\n\
         ForegroundPositive={}\nForegroundVisited={}\n\n",
        accent.mix(bg, 0.5).kde(),
        accent.kde(),
        accent.kde(),
        accent.kde(),
        p.on_primary.kde(),
        p.on_primary.mix(accent, 0.3).kde(),
        p.on_primary.kde(),
        p.error.kde(),
        neutral.kde(),
        p.on_primary.kde(),
        positive.kde(),
        p.on_primary.kde(),
    ));
    out.push_str(&group("Colors:Tooltip", bg, alt, fg));
    out.push_str(&group("Colors:View", view_bg, alt, fg));
    out.push_str(&group("Colors:Window", bg, alt, fg));
    out.push_str(&format!(
        "[General]\nColorScheme={id}\nName={name}\nshadeSortColumn=true\n\n\
         [KDE]\ncontrast=4\n\n\
         [WM]\nactiveBackground={}\nactiveBlend={}\nactiveForeground={}\n\
         inactiveBackground={}\ninactiveBlend={}\ninactiveForeground={}\n",
        bg.kde(),
        fg.kde(),
        fg.kde(),
        bg.kde(),
        inactive.kde(),
        inactive.kde(),
    ));
    out
}

// ============================================================================
// Shell palettes ⇄ palette
// ============================================================================

/// A Material palette as DMS writes it (`dms-colors.json`) and our Noctalia
/// template: `{"mode": "dark", "colors": {"dark": {...}, "light": {...}}}`
/// with names `primary`, `on_primary`, `surface_container_high` ….
///
/// `dark_hint` — the mode the shell is in now (DMS stores it separately,
/// in `session.json`); `None` — by the `mode` field, without it — dark.
pub fn palette_from_material(json: &str, dark_hint: Option<bool>) -> Option<Palette> {
    let v: Value = serde_json::from_str(json).ok()?;
    let dark = dark_hint.unwrap_or_else(|| v.get("mode").and_then(Value::as_str) != Some("light"));
    let mode = if dark { "dark" } else { "light" };
    let colors = v.get("colors")?.get(mode)?;
    let get = |role: &str| -> Option<Rgb> {
        let key = match role {
            "hover" => "surface_container_high",
            other => other,
        };
        colors.get(key).and_then(Value::as_str).and_then(Rgb::parse)
    };
    Palette::from_roles(Some(dark), get)
}

/// A Noctalia palette (`palettes/*.json`: `dark`/`light` with `mPrimary` …).
/// If the needed variant is missing, the other one is taken, as the shell itself does.
pub fn palette_from_noctalia(json: &str, dark: bool) -> Option<Palette> {
    let v: Value = serde_json::from_str(json).ok()?;
    let first = if dark { "dark" } else { "light" };
    let second = if dark { "light" } else { "dark" };
    let variant = v.get(first).or_else(|| v.get(second))?;
    let get = |role: &str| -> Option<Rgb> {
        let mut key = String::from("m");
        for part in role.split('_') {
            let mut ch = part.chars();
            if let Some(f) = ch.next() {
                key.extend(f.to_uppercase());
                key.push_str(ch.as_str());
            }
        }
        variant.get(&key).and_then(Value::as_str).and_then(Rgb::parse)
    };
    Palette::from_roles(Some(dark), get)
}

/// A DMS custom theme (`customThemeFile`): either `dark`/`light` or a flat
/// set. Names are DMS ones (`primaryText`, `surfaceText` …).
pub fn palette_from_dms_theme(json: &str, dark: bool) -> Option<Palette> {
    let v: Value = serde_json::from_str(json).ok()?;
    let mode = if dark { "dark" } else { "light" };
    let colors = if v.get("dark").is_some() || v.get("light").is_some() {
        v.get(mode).or_else(|| v.get("dark")).or_else(|| v.get("light"))?
    } else {
        &v
    };
    let get = |role: &str| -> Option<Rgb> {
        let key = match role {
            "on_primary" => "primaryText",
            "on_surface" => "surfaceText",
            "surface_variant" => "surfaceVariant",
            "on_surface_variant" => "surfaceVariantText",
            "hover" => "surfaceContainerHigh",
            "primary" | "secondary" | "tertiary" | "surface" | "error" | "outline" => role,
            _ => return None,
        };
        colors.get(key).and_then(Value::as_str).and_then(Rgb::parse)
    };
    Palette::from_roles(Some(dark), get)
}

/// Noctalia palette file. Both variants are the same palette: the mode
/// is set for the shell separately, and in either of them it must show
/// exactly this palette.
pub fn noctalia_palette_json(p: &Palette) -> String {
    let mut variant = serde_json::Map::new();
    for (k, c) in p.noctalia_roles() {
        variant.insert(k.to_string(), Value::String(c.hex()));
    }
    let mut root = serde_json::Map::new();
    root.insert("dark".into(), Value::Object(variant.clone()));
    root.insert("light".into(), Value::Object(variant));
    pretty(&Value::Object(root))
}

/// DMS custom theme file (the same keys as its built-in themes).
pub fn dms_theme_json(p: &Palette) -> String {
    let surface_container = |t: f64| p.surface.mix(p.on_surface, t).hex();
    let mut variant = serde_json::Map::new();
    let mut put = |k: &str, v: String| {
        variant.insert(k.to_string(), Value::String(v));
    };
    put("name", "Rexon".to_string());
    put("primary", p.primary.hex());
    put("primaryText", p.on_primary.hex());
    put("primaryContainer", p.primary.mix(p.surface, 0.6).hex());
    put("secondary", p.secondary.hex());
    put("tertiary", p.tertiary.hex());
    put("surface", p.surface.hex());
    put("surfaceText", p.on_surface.hex());
    put("surfaceVariant", p.surface_variant.hex());
    put("surfaceVariantText", p.on_surface_variant.hex());
    put("surfaceTint", p.primary.hex());
    put("background", p.surface.hex());
    put("backgroundText", p.on_surface.hex());
    put("outline", p.outline.hex());
    put("surfaceContainerLowest", p.surface.mix(BLACK, 0.2).hex());
    put("surfaceContainerLow", surface_container(0.03));
    put("surfaceContainer", surface_container(0.06));
    put("surfaceContainerHigh", p.hover.hex());
    put("surfaceContainerHighest", surface_container(0.14));
    put("error", p.error.hex());
    let mut root = serde_json::Map::new();
    root.insert("dark".into(), Value::Object(variant.clone()));
    root.insert("light".into(), Value::Object(variant));
    pretty(&Value::Object(root))
}

/// Roles the Noctalia template exports (Material names).
const BRIDGE_TOKENS: &[&str] = &[
    "primary",
    "on_primary",
    "secondary",
    "on_secondary",
    "tertiary",
    "on_tertiary",
    "error",
    "on_error",
    "surface",
    "on_surface",
    "surface_variant",
    "on_surface_variant",
    "outline",
    "shadow",
    "surface_container_high",
];

/// A Noctalia template that writes the palette in the same form as DMS's
/// `dms-colors.json` — both share one reader
/// ([`palette_from_material`]). Syntax — `{{ colors.<name>.<mode>.hex }}`
/// and `{{ mode }}` (Noctalia documentation, «Template Reference»).
pub fn noctalia_bridge_template() -> String {
    let variant = |mode: &str| {
        BRIDGE_TOKENS
            .iter()
            .map(|t| format!("      \"{t}\": \"{{{{ colors.{t}.{mode}.hex }}}}\""))
            .collect::<Vec<_>>()
            .join(",\n")
    };
    format!(
        "{{\n  \"mode\": \"{{{{ mode }}}}\",\n  \"colors\": {{\n    \"dark\": {{\n{}\n    }},\n    \"light\": {{\n{}\n    }}\n  }}\n}}\n",
        variant("dark"),
        variant("light")
    )
}

fn pretty(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_default();
    s.push('\n');
    s
}

// ============================================================================
// Where things live
// ============================================================================

/// Name of our palette in Noctalia (`palettes/Rexon.json`).
pub const NOCTALIA_PALETTE_NAME: &str = "Rexon";
/// Name of our Noctalia template.
pub const NOCTALIA_BRIDGE_ID: &str = "rexon_palette";
/// Two KDE schemes with the shell's colors. Two — because
/// `plasma-apply-colorscheme` does nothing if asked for the same scheme
/// that is already set: new colors under the old name would not be applied.
pub const SHELL_SCHEME_IDS: [&str; 2] = ["RexonShell", "RexonShellAlt"];

/// Home roots (empty string — the variable is not set).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaletteHome {
    pub home: String,
    pub xdg_config_home: String,
    pub xdg_state_home: String,
    pub xdg_cache_home: String,
    pub xdg_data_home: String,
    pub noctalia_config_home: String,
    pub noctalia_state_home: String,
}

impl PaletteHome {
    pub fn from_env(get: impl Fn(&str) -> String) -> PaletteHome {
        PaletteHome {
            home: get("HOME"),
            xdg_config_home: get("XDG_CONFIG_HOME"),
            xdg_state_home: get("XDG_STATE_HOME"),
            xdg_cache_home: get("XDG_CACHE_HOME"),
            xdg_data_home: get("XDG_DATA_HOME"),
            noctalia_config_home: get("NOCTALIA_CONFIG_HOME"),
            noctalia_state_home: get("NOCTALIA_STATE_HOME"),
        }
    }

    pub fn live() -> PaletteHome {
        PaletteHome::from_env(|k| std::env::var(k).unwrap_or_default())
    }

    fn root(&self, value: &str, fallback: &str) -> PathBuf {
        if value.is_empty() {
            Path::new(&self.home).join(fallback)
        } else {
            PathBuf::from(value)
        }
    }

    fn config(&self) -> PathBuf {
        self.root(&self.xdg_config_home, ".config")
    }
    fn state(&self) -> PathBuf {
        self.root(&self.xdg_state_home, ".local/state")
    }
    fn cache(&self) -> PathBuf {
        self.root(&self.xdg_cache_home, ".cache")
    }
    fn data(&self) -> PathBuf {
        self.root(&self.xdg_data_home, ".local/share")
    }

    /// Noctalia configuration directory (`$NOCTALIA_CONFIG_HOME/noctalia`, otherwise
    /// XDG) — all `*.toml` in it and `palettes/`.
    pub fn noctalia_config_dir(&self) -> PathBuf {
        if self.noctalia_config_home.is_empty() {
            self.config().join("noctalia")
        } else {
            Path::new(&self.noctalia_config_home).join("noctalia")
        }
    }

    /// Noctalia state file — the layer that overrides the configuration.
    pub fn noctalia_state_file(&self) -> PathBuf {
        let root = if self.noctalia_state_home.is_empty() {
            self.state()
        } else {
            PathBuf::from(&self.noctalia_state_home)
        };
        root.join("noctalia/settings.toml")
    }

    pub fn noctalia_palette_file(&self, name: &str) -> PathBuf {
        self.noctalia_config_dir().join("palettes").join(format!("{name}.json"))
    }

    pub fn dms_settings_file(&self) -> PathBuf {
        self.config().join("DankMaterialShell/settings.json")
    }
    pub fn dms_session_file(&self) -> PathBuf {
        self.state().join("DankMaterialShell/session.json")
    }
    pub fn dms_colors_file(&self) -> PathBuf {
        self.cache().join("DankMaterialShell/dms-colors.json")
    }
    pub fn dms_theme_file(&self) -> PathBuf {
        self.config().join("DankMaterialShell/rexon-palette.json")
    }

    /// Rexon's memo: what existed before us and who leads the palette.
    pub fn memo_file(&self) -> PathBuf {
        self.state().join("rexon/shell-palette.json")
    }
    pub fn bridge_template_file(&self) -> PathBuf {
        self.state().join("rexon/shell-palette/noctalia-bridge.json.in")
    }
    pub fn bridge_output_file(&self) -> PathBuf {
        self.state().join("rexon/shell-palette/noctalia-colors.json")
    }

    pub fn kdeglobals(&self) -> PathBuf {
        self.config().join("kdeglobals")
    }
    /// `$XDG_STATE_HOME` or `~/.local/state`.
    pub fn state_dir(&self) -> PathBuf {
        self.state()
    }
    pub fn color_scheme_file(&self, id: &str) -> PathBuf {
        self.data().join("color-schemes").join(format!("{id}.colors"))
    }
}

/// Name of the backup next to the shell's file. Our own, not the same as for shell
/// wallpapers (`*.rexon-backup`): copies of two edits of one file must not
/// overwrite each other.
pub fn backup_path(file: &Path) -> PathBuf {
    let mut name = file.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".rexon-palette-backup");
    file.with_file_name(name)
}

// ============================================================================
// Targeted edits of TOML (by lines) and JSON (by top-level members)
// ============================================================================

/// Why an edit was not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The file does not parse — it must not be touched.
    Unparsable(String),
    /// After the edit parsing showed something other than intended (the key is written
    /// differently from what we can handle — dotted keys, an inline table …).
    Unexpected(String),
}

impl EditError {
    pub fn reason(&self) -> &str {
        match self {
            EditError::Unparsable(r) | EditError::Unexpected(r) => r,
        }
    }
}

fn lines_keep_ends(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// Table name from a header line `[a.b]` (spaces around dots and
/// outer key quotes removed). `[[…]]` and non-headers — `None`.
fn toml_header(line: &str) -> Option<String> {
    let t = line.trim();
    let t = match t.find('#') {
        Some(i) if !t[..i].contains('"') => t[..i].trim(),
        _ => t,
    };
    if !t.starts_with('[') || t.starts_with("[[") || !t.ends_with(']') {
        return None;
    }
    let inner = &t[1..t.len() - 1];
    Some(
        inner
            .split('.')
            .map(|p| p.trim().trim_matches('"').trim_matches('\''))
            .collect::<Vec<_>>()
            .join("."),
    )
}

/// Key of a line of the form `key = …` (without a dotted path).
fn toml_line_key(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if t.starts_with('#') || t.starts_with('[') {
        return None;
    }
    let (k, _) = t.split_once('=')?;
    let k = k.trim().trim_matches('"');
    if k.is_empty() || k.contains('.') {
        None
    } else {
        Some(k)
    }
}

/// Line range of table `table`: (header line, end exclusive).
/// An empty name is the root (before the first header).
fn toml_table_span(lines: &[&str], table: &str) -> Option<(Option<usize>, usize)> {
    if table.is_empty() {
        let end = lines.iter().position(|l| toml_header(l).is_some()).unwrap_or(lines.len());
        return Some((None, end));
    }
    let start = lines.iter().position(|l| toml_header(l).as_deref() == Some(table))?;
    let end = lines[start + 1..]
        .iter()
        .position(|l| toml_header(l).is_some() || l.trim_start().starts_with("[["))
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());
    Some((Some(start), end))
}

/// The line of key `key` in table `table` as is (without the newline).
pub fn toml_key_line(text: &str, table: &str, key: &str) -> Option<String> {
    let lines = lines_keep_ends(text);
    let (head, end) = toml_table_span(&lines, table)?;
    let from = head.map(|h| h + 1).unwrap_or(0);
    lines[from..end]
        .iter()
        .find(|l| toml_line_key(l) == Some(key))
        .map(|l| l.trim_end_matches(['\n', '\r']).to_string())
}

/// Set (`Some(line)`) or remove (`None`) a key line. No table —
/// it is appended at the end of the file. Text only; checking the meaning is up to
/// the caller ([`toml_apply`]).
pub fn toml_set_line(text: &str, table: &str, key: &str, line: Option<&str>) -> String {
    let lines = lines_keep_ends(text);
    let Some((head, end)) = toml_table_span(&lines, table) else {
        let Some(line) = line else { return text.to_string() };
        let mut out = text.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("[{table}]\n{line}\n"));
        return out;
    };
    let from = head.map(|h| h + 1).unwrap_or(0);
    let found = (from..end).find(|&i| toml_line_key(lines[i]) == Some(key));
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    match (found, line) {
        (Some(i), Some(new)) => {
            let ending = if out[i].ends_with("\r\n") {
                "\r\n"
            } else if out[i].ends_with('\n') {
                "\n"
            } else {
                ""
            };
            out[i] = format!("{new}{ending}");
        }
        (Some(i), None) => {
            out.remove(i);
        }
        (None, Some(new)) => {
            // After the last non-empty line of the table — the empty lines
            // separating the next table stay in place.
            let mut at = end;
            while at > from && out[at - 1].trim().is_empty() {
                at -= 1;
            }
            if at > 0 && !out[at - 1].ends_with('\n') {
                out[at - 1].push('\n');
            }
            out.insert(at, format!("{new}\n"));
        }
        (None, None) => {}
    }
    out.concat()
}

/// Remove a table header if not a single line other than empty ones is left in it —
/// together with the empty line before it that
/// [`toml_set_line`] appends.
fn toml_drop_empty_table(text: &str, table: &str) -> String {
    let lines = lines_keep_ends(text);
    let Some((Some(head), end)) = toml_table_span(&lines, table) else {
        return text.to_string();
    };
    if lines[head + 1..end].iter().any(|l| !l.trim().is_empty()) {
        return text.to_string();
    }
    let mut from = head;
    if from > 0 && lines[from - 1].trim().is_empty() {
        from -= 1;
    }
    let mut out: Vec<&str> = lines[..from].to_vec();
    out.extend_from_slice(&lines[end..]);
    out.concat()
}

fn toml_parse(text: &str) -> Result<toml::Table, EditError> {
    text.parse::<toml::Table>().map_err(|e| EditError::Unparsable(e.to_string()))
}

/// Value at path `a.b.c` in a parsed table.
fn toml_lookup<'a>(root: &'a toml::Table, table: &str, key: &str) -> Option<&'a toml::Value> {
    let mut cur = root;
    if !table.is_empty() {
        for part in table.split('.') {
            cur = cur.get(part)?.as_table()?;
        }
    }
    cur.get(key)
}

/// The same for editing the expected one.
fn toml_put(root: &mut toml::Table, table: &str, key: &str, value: Option<toml::Value>) {
    let mut cur = root;
    if !table.is_empty() {
        for part in table.split('.') {
            let entry = cur.entry(part.to_string()).or_insert_with(|| toml::Value::Table(toml::Table::new()));
            let Some(t) = entry.as_table_mut() else { return };
            cur = t;
        }
    }
    match value {
        Some(v) => {
            cur.insert(key.to_string(), v);
        }
        None => {
            cur.remove(key);
        }
    }
}

/// Remove empty tables — for the parser «no table» and «empty table»
/// differ, but for the shell they mean the same.
fn toml_prune(t: &mut toml::Table) {
    let keys: Vec<String> = t.keys().cloned().collect();
    for k in keys {
        if let Some(toml::Value::Table(inner)) = t.get_mut(&k) {
            toml_prune(inner);
            if inner.is_empty() {
                t.remove(&k);
            }
        }
    }
}

/// A TOML line with a string value.
pub fn toml_string_line(key: &str, value: &str) -> String {
    let mut esc = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => esc.push_str("\\\\"),
            '"' => esc.push_str("\\\""),
            '\n' => esc.push_str("\\n"),
            '\t' => esc.push_str("\\t"),
            c if (c as u32) < 0x20 => esc.push_str(&format!("\\u{:04x}", c as u32)),
            c => esc.push(c),
        }
    }
    format!("{key} = \"{esc}\"")
}

/// One targeted edit: table, key, new line (`None` — remove).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlEdit {
    pub table: String,
    pub key: String,
    pub line: Option<String>,
}

/// Apply edits to the text and verify by parsing: the new file must
/// differ from the old one in EXACTLY these keys. `drop_tables` — tables
/// to remove if they became empty (Rexon created them).
pub fn toml_apply(text: &str, edits: &[TomlEdit], drop_tables: &[String]) -> Result<String, EditError> {
    let before = toml_parse(text)?;
    let mut out = text.to_string();
    let mut expected = before.clone();
    for e in edits {
        out = toml_set_line(&out, &e.table, &e.key, e.line.as_deref());
        let value = match &e.line {
            Some(line) => {
                let one = toml_parse(line)?;
                Some(one.get(&e.key).cloned().ok_or_else(|| EditError::Unexpected(line.clone()))?)
            }
            None => None,
        };
        toml_put(&mut expected, &e.table, &e.key, value);
    }
    for t in drop_tables {
        out = toml_drop_empty_table(&out, t);
    }
    let mut after = toml_parse(&out).map_err(|e| EditError::Unexpected(e.reason().to_string()))?;
    toml_prune(&mut after);
    toml_prune(&mut expected);
    if after != expected {
        return Err(EditError::Unexpected(
            "the keys are written in a form Rexon does not edit".to_string(),
        ));
    }
    Ok(out)
}

// ── JSON ────────────────────────────────────────────────────────────────

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

fn string_end(b: &[u8], mut i: usize) -> Option<usize> {
    // b[i] == '"'
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

fn value_end(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'"' => string_end(b, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'"' => {
                        j = string_end(b, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            None
        }
        _ => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r') {
                j += 1;
            }
            Some(j)
        }
    }
}

/// A member of the top-level object: start of the key and end of the value.
#[derive(Debug, Clone)]
struct Member {
    key: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
}

fn json_members(text: &str) -> Option<(usize, Vec<Member>, usize)> {
    let b = text.as_bytes();
    let open = skip_ws(b, 0);
    if b.get(open) != Some(&b'{') {
        return None;
    }
    let mut out = Vec::new();
    let mut i = skip_ws(b, open + 1);
    if b.get(i) == Some(&b'}') {
        return Some((open, out, i));
    }
    loop {
        if b.get(i) != Some(&b'"') {
            return None;
        }
        let key_end = string_end(b, i)?;
        let key: String = serde_json::from_str(&text[i..key_end]).ok()?;
        let colon = skip_ws(b, key_end);
        if b.get(colon) != Some(&b':') {
            return None;
        }
        let vs = skip_ws(b, colon + 1);
        let ve = value_end(b, vs)?;
        out.push(Member { key, key_start: i, value_start: vs, value_end: ve });
        let next = skip_ws(b, ve);
        match b.get(next) {
            Some(b',') => i = skip_ws(b, next + 1),
            Some(b'}') => return Some((open, out, next)),
            _ => return None,
        }
    }
}

/// Raw value of a top-level member (JSON text as in the file).
pub fn json_member_raw(text: &str, key: &str) -> Option<String> {
    let (_, members, _) = json_members(text)?;
    members
        .iter()
        .rev()
        .find(|m| m.key == key)
        .map(|m| text[m.value_start..m.value_end].to_string())
}

/// Set (`Some(raw value)`) or remove a top-level member
/// without touching the rest of the text. A new member goes at the end of the object, indented
/// like the first one.
pub fn json_set_member(text: &str, key: &str, raw: Option<&str>) -> Result<String, EditError> {
    let before: Value = serde_json::from_str(text).map_err(|e| EditError::Unparsable(e.to_string()))?;
    let Value::Object(mut expected) = before else {
        return Err(EditError::Unparsable("not a JSON object".to_string()));
    };
    let (open, members, close) =
        json_members(text).ok_or_else(|| EditError::Unparsable("not a JSON object".to_string()))?;
    let idx = members.iter().rposition(|m| m.key == key);
    let out = match (idx, raw) {
        (Some(i), Some(v)) => {
            let m = &members[i];
            format!("{}{}{}", &text[..m.value_start], v, &text[m.value_end..])
        }
        (Some(i), None) => {
            let m = &members[i];
            if members.len() == 1 {
                format!("{}{}", &text[..open + 1], &text[close..])
            } else if i + 1 < members.len() {
                // Eat «key … value, » up to the start of the next key.
                format!("{}{}", &text[..m.key_start], &text[members[i + 1].key_start..])
            } else {
                // The last one: eat from the end of the previous value.
                format!("{}{}", &text[..members[i - 1].value_end], &text[m.value_end..])
            }
        }
        (None, Some(v)) => {
            let key_json = serde_json::to_string(key).unwrap_or_default();
            if let Some(last) = members.last() {
                let first = &members[0];
                let indent_start = text[..first.key_start].rfind('\n').map(|p| p + 1).unwrap_or(first.key_start);
                let indent = &text[indent_start..first.key_start];
                let sep = if text[..first.key_start].contains('\n') { format!("\n{indent}") } else { " ".to_string() };
                format!("{},{sep}{key_json}: {v}{}", &text[..last.value_end], &text[last.value_end..])
            } else {
                format!("{}\n  {key_json}: {v}\n{}", &text[..open + 1], &text[close..])
            }
        }
        (None, None) => text.to_string(),
    };
    match raw {
        Some(v) => {
            let value: Value = serde_json::from_str(v).map_err(|e| EditError::Unexpected(e.to_string()))?;
            expected.insert(key.to_string(), value);
        }
        None => {
            expected.remove(key);
        }
    }
    let after: Value = serde_json::from_str(&out).map_err(|e| EditError::Unexpected(e.to_string()))?;
    if after != Value::Object(expected) {
        return Err(EditError::Unexpected("the member did not change as intended".to_string()));
    }
    Ok(out)
}

// ============================================================================
// Memo
// ============================================================================

/// Who leads the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lead {
    /// A scheme applied in Rexon goes to the shell right away together with the mode.
    Rexon,
    /// The shell leads the palette: Rexon passes it nothing and takes the shell's
    /// colors on a button press.
    Shell,
}

impl Lead {
    pub fn as_str(self) -> &'static str {
        match self {
            Lead::Rexon => "rexon",
            Lead::Shell => "shell",
        }
    }
    pub fn parse(s: &str) -> Option<Lead> {
        match s {
            "rexon" => Some(Lead::Rexon),
            "shell" => Some(Lead::Shell),
            _ => None,
        }
    }
}

/// Previous value of one of our keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyMemo {
    /// TOML table (`theme`, `theme.templates.user.rexon_palette`); empty for
    /// JSON.
    pub table: String,
    pub key: String,
    /// The TOML key line or the raw JSON value before us; `None` — the key
    /// did not exist.
    pub prev: Option<String>,
}

/// A file Rexon edited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMemo {
    pub path: String,
    /// `toml`, `json` or `own` (the whole file is ours: palette, template).
    pub kind: String,
    /// Content before Rexon's first edit; `None` — the file did not exist.
    pub original: Option<String>,
    /// What Rexon wrote last.
    pub written: String,
    /// Our keys (empty for `own`).
    #[serde(default)]
    pub keys: Vec<KeyMemo>,
    /// TOML tables Rexon created.
    #[serde(default)]
    pub created_tables: Vec<String>,
}

/// What Rexon did in the environment when taking the shell's colors.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvMemo {
    /// The KDE scheme before us (`[General] ColorScheme` in `kdeglobals`).
    pub prev_scheme: Option<String>,
    /// Our scheme files.
    #[serde(default)]
    pub scheme_files: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memo {
    #[serde(default)]
    pub lead: Option<Lead>,
    /// Which shell had what edited (`as_str`); empty — nothing.
    #[serde(default)]
    pub shell: String,
    #[serde(default)]
    pub files: Vec<FileMemo>,
    #[serde(default)]
    pub env: Option<EnvMemo>,
}

impl Memo {
    fn file(&self, path: &Path) -> Option<&FileMemo> {
        let p = path.to_string_lossy();
        self.files.iter().find(|f| f.path == p)
    }
    /// There is something to restore.
    pub fn has_changes(&self) -> bool {
        !self.files.is_empty() || self.env.is_some()
    }
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    crate::utils::atomic_write(&path.to_string_lossy(), text).map_err(|e| e.to_string())
}

fn remove_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// The memo from disk (missing or broken — empty).
pub fn read_memo(home: &PaletteHome) -> Memo {
    read_optional(&home.memo_file())
        .ok()
        .flatten()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_memo(home: &PaletteHome, memo: &Memo) -> Result<(), String> {
    let text = serde_json::to_string_pretty(memo).map_err(|e| e.to_string())? + "\n";
    write_text(&home.memo_file(), &text)
}

/// Remember the «who leads the palette» choice.
pub fn set_lead(home: &PaletteHome, lead: Lead) -> Result<(), String> {
    let mut memo = read_memo(home);
    if memo.lead == Some(lead) {
        return Ok(());
    }
    memo.lead = Some(lead);
    save_memo(home, &memo)
}

// ============================================================================
// Writing to the shell's files
// ============================================================================

/// One intended file edit.
#[derive(Debug, Clone)]
enum FileChange {
    /// The whole file is ours.
    Own(String),
    /// TOML keys.
    Toml(Vec<TomlEdit>),
    /// Top-level JSON members: key → raw value.
    Json(Vec<(String, String)>),
}

/// Why it did not work, in words for the person.
fn edit_refused(shell: DesktopShell, path: &Path, reason: &str) -> String {
    trf(
        // TRANSLATORS: %1 is the shell name, %2 a file path, %3 the technical reason. Nothing was changed.
        "Rexon left the settings of %1 alone: %2 is not laid out the way Rexon expects (%3). Nothing was changed.",
        &[&shell.label(), &path.to_string_lossy(), &reason],
    )
}

/// Apply the edits: what existed goes into the memo BEFORE writing, a backup
/// next to the file. Returns whether anything changed at all.
fn apply_changes(
    home: &PaletteHome,
    shell: DesktopShell,
    changes: &[(PathBuf, FileChange)],
) -> Result<bool, String> {
    let mut memo = read_memo(home);
    // Compute everything first — a refusal of any file must not leave
    // half of the edits.
    let mut planned: Vec<(PathBuf, Option<String>, String, FileChange)> = Vec::new();
    for (path, change) in changes {
        let current = read_optional(path).map_err(|e| edit_refused(shell, path, &e))?;
        let new_text = match change {
            FileChange::Own(text) => text.clone(),
            FileChange::Toml(edits) => {
                let base = current.clone().unwrap_or_default();
                toml_apply(&base, edits, &[]).map_err(|e| edit_refused(shell, path, e.reason()))?
            }
            FileChange::Json(members) => {
                let mut text = current.clone().unwrap_or_else(|| "{}\n".to_string());
                for (k, v) in members {
                    text = json_set_member(&text, k, Some(v)).map_err(|e| edit_refused(shell, path, e.reason()))?;
                }
                text
            }
        };
        planned.push((path.clone(), current, new_text, change.clone()));
    }
    let any = planned.iter().any(|(_, cur, new, _)| cur.as_deref() != Some(new.as_str()));
    if !any {
        return Ok(false);
    }
    // The memo goes first.
    for (path, current, new_text, change) in &planned {
        if current.as_deref() == Some(new_text.as_str()) && memo.file(path).is_none() {
            continue;
        }
        let p = path.to_string_lossy().to_string();
        let idx = match memo.files.iter().position(|f| f.path == p) {
            Some(i) => i,
            None => {
                memo.files.push(FileMemo {
                    path: p.clone(),
                    kind: match change {
                        FileChange::Own(_) => "own",
                        FileChange::Toml(_) => "toml",
                        FileChange::Json(_) => "json",
                    }
                    .to_string(),
                    original: current.clone(),
                    written: String::new(),
                    keys: Vec::new(),
                    created_tables: Vec::new(),
                });
                memo.files.len() - 1
            }
        };
        let fm = &mut memo.files[idx];
        let base = current.clone().unwrap_or_default();
        match change {
            FileChange::Own(_) => {}
            FileChange::Toml(edits) => {
                for e in edits {
                    if !fm.keys.iter().any(|k| k.table == e.table && k.key == e.key) {
                        fm.keys.push(KeyMemo {
                            table: e.table.clone(),
                            key: e.key.clone(),
                            prev: toml_key_line(&base, &e.table, &e.key),
                        });
                    }
                    let lines = lines_keep_ends(&base);
                    if toml_table_span(&lines, &e.table).is_none() && !fm.created_tables.contains(&e.table) {
                        fm.created_tables.push(e.table.clone());
                    }
                }
            }
            FileChange::Json(members) => {
                for (k, _) in members {
                    if !fm.keys.iter().any(|m| m.key == *k) {
                        fm.keys.push(KeyMemo {
                            table: String::new(),
                            key: k.clone(),
                            prev: current.as_deref().and_then(|t| json_member_raw(t, k)),
                        });
                    }
                }
            }
        }
        fm.written = new_text.clone();
    }
    memo.shell = shell.as_str().to_string();
    save_memo(home, &memo)?;
    for (path, current, new_text, _) in &planned {
        if current.as_deref() == Some(new_text.as_str()) {
            continue;
        }
        if let Some(orig) = current {
            let bak = backup_path(path);
            if !bak.exists() {
                write_text(&bak, orig).map_err(|e| edit_refused(shell, path, &e))?;
            }
        }
        write_text(path, new_text).map_err(|e| edit_refused(shell, path, &e))?;
    }
    Ok(true)
}

// ============================================================================
// Shell mode
// ============================================================================

/// Noctalia configuration layers in order: the directory's `*.toml` alphabetically,
/// the state file on top. (`[include]` is not expanded: theme keys are
/// rarely moved there, and a mistake here costs only an extra mode edit.)
fn noctalia_layers(home: &PaletteHome) -> Vec<toml::Table> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(home.noctalia_config_dir())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files.push(home.noctalia_state_file());
    files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|t| t.parse::<toml::Table>().ok())
        .collect()
}

/// Value of Noctalia's `[theme] <key>` after merging the layers.
fn noctalia_theme_value(home: &PaletteHome, key: &str) -> Option<String> {
    noctalia_layers(home)
        .iter()
        .rev()
        .find_map(|t| toml_lookup(t, "theme", key).and_then(|v| v.as_str().map(str::to_string)))
}

fn json_file_value(path: &Path, key: &str) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get(key).cloned()
}

/// Whether the shell is in dark mode now. `None` — cannot tell (Noctalia's
/// `mode = "auto"`: mode follows the sun schedule).
pub fn shell_dark(shell: DesktopShell, home: &PaletteHome) -> Option<bool> {
    match shell {
        DesktopShell::Noctalia => match noctalia_theme_value(home, "mode").as_deref() {
            None | Some("dark") => Some(true),
            Some("light") => Some(false),
            _ => None,
        },
        DesktopShell::DankMaterialShell => {
            let light = json_file_value(&home.dms_session_file(), "isLightMode")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(!light)
        }
    }
}

// ============================================================================
// From Rexon to the shell
// ============================================================================

/// Palette of the environment's current color scheme (`kdeglobals`).
pub fn current_rexon_palette(home: &PaletteHome) -> Option<Palette> {
    palette_from_kde(&std::fs::read_to_string(home.kdeglobals()).ok()?)
}

/// The KDE scheme currently set (`[General] ColorScheme`).
pub fn current_scheme_name(home: &PaletteHome) -> Option<String> {
    let text = std::fs::read_to_string(home.kdeglobals()).ok()?;
    kde_groups(&text)
        .get("General")
        .and_then(|g| g.get("ColorScheme"))
        .filter(|s| !s.is_empty())
        .cloned()
}

fn push_changes(shell: DesktopShell, p: &Palette, home: &PaletteHome) -> Vec<(PathBuf, FileChange)> {
    let mode = if p.dark { "dark" } else { "light" };
    match shell {
        DesktopShell::Noctalia => {
            let mut edits = Vec::new();
            let mut want = |key: &str, value: &str| {
                if noctalia_theme_value(home, key).as_deref() != Some(value) {
                    edits.push(TomlEdit {
                        table: "theme".into(),
                        key: key.into(),
                        line: Some(toml_string_line(key, value)),
                    });
                }
            };
            want("source", "custom");
            want("custom_palette", NOCTALIA_PALETTE_NAME);
            // The default mode is dark: we set the key only if it
            // would differ from the scheme.
            if shell_dark(shell, home) != Some(p.dark) {
                edits.push(TomlEdit {
                    table: "theme".into(),
                    key: "mode".into(),
                    line: Some(toml_string_line("mode", mode)),
                });
            }
            let mut out = vec![(
                home.noctalia_palette_file(NOCTALIA_PALETTE_NAME),
                FileChange::Own(noctalia_palette_json(p)),
            )];
            if !edits.is_empty() {
                out.push((home.noctalia_state_file(), FileChange::Toml(edits)));
            }
            out
        }
        DesktopShell::DankMaterialShell => {
            let theme = home.dms_theme_file();
            let s = |v: &str| serde_json::to_string(v).unwrap_or_default();
            let settings = home.dms_settings_file();
            let mut members = Vec::new();
            for (k, v) in [
                ("currentThemeName", "custom".to_string()),
                ("currentThemeCategory", "custom".to_string()),
                ("customThemeFile", theme.to_string_lossy().to_string()),
            ] {
                if json_file_value(&settings, k).as_ref().and_then(Value::as_str) != Some(v.as_str()) {
                    members.push((k.to_string(), s(&v)));
                }
            }
            let mut out = vec![(theme, FileChange::Own(dms_theme_json(p)))];
            if !members.is_empty() {
                out.push((settings, FileChange::Json(members)));
            }
            if shell_dark(shell, home) != Some(p.dark) {
                out.push((
                    home.dms_session_file(),
                    FileChange::Json(vec![("isLightMode".into(), (!p.dark).to_string())]),
                ));
            }
            out
        }
    }
}

/// Pass the palette to the shell. `Ok(text)` — the result for the person (including
/// «nothing to change»), `Err(text)` — a refusal.
pub fn push_palette(shell: DesktopShell, p: &Palette, home: &PaletteHome) -> Result<String, String> {
    let changed = apply_changes(home, shell, &push_changes(shell, p, home))?;
    if !changed {
        return Ok(trf(
            // TRANSLATORS: %1 is the shell name.
            "%1 already has this palette — nothing to change.",
            &[&shell.label()],
        ));
    }
    let mode = if p.dark { crate::i18n::trc("theme mode", "dark") } else { crate::i18n::trc("theme mode", "light") };
    Ok(trf(
        // TRANSLATORS: %1 is the shell name, %2 is “dark” or “light”.
        "Palette passed to %1. The shell's mode is set to %2, the same as the color scheme, so the two no longer fight over the dark/light setting of applications.",
        &[&shell.label(), &mode],
    ))
}

/// There is no scheme whose palette could be passed on.
pub fn no_rexon_palette_msg() -> String {
    tr("There is no current color scheme to pass on: kdeglobals has no window colors. Apply a color scheme in “Color Schemes” and try again.")
}

// ============================================================================
// From the shell to Rexon
// ============================================================================

/// Why the shell's colors were not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullError {
    /// Noctalia: our template is needed (with the person's consent — [`connect_bridge`]).
    NeedsBridge,
    /// The template is connected, but the shell has not filled the file yet.
    BridgePending,
    /// A refusal in words for the person.
    Refused(String),
}

/// The shell's colors now.
pub fn shell_palette(shell: DesktopShell, home: &PaletteHome) -> Result<Palette, PullError> {
    match shell {
        DesktopShell::Noctalia => {
            let dark = shell_dark(shell, home);
            if noctalia_theme_value(home, "source").as_deref() == Some("custom") {
                let name = noctalia_theme_value(home, "custom_palette").unwrap_or_default();
                if !name.is_empty() {
                    if let Some(p) = std::fs::read_to_string(home.noctalia_palette_file(&name))
                        .ok()
                        .and_then(|t| palette_from_noctalia(&t, dark.unwrap_or(true)))
                    {
                        return Ok(p);
                    }
                }
            }
            let bridged = bridge_connected(home);
            match std::fs::read_to_string(home.bridge_output_file()) {
                Ok(t) if bridged => palette_from_material(&t, dark).ok_or_else(|| {
                    PullError::Refused(trf(
                        // TRANSLATORS: %1 is a file path.
                        "The color file %1 could not be read.",
                        &[&home.bridge_output_file().to_string_lossy()],
                    ))
                }),
                _ if bridged => Err(PullError::BridgePending),
                _ => Err(PullError::NeedsBridge),
            }
        }
        DesktopShell::DankMaterialShell => {
            let dark = shell_dark(shell, home).unwrap_or(true);
            let settings = home.dms_settings_file();
            let theme_name = json_file_value(&settings, "currentThemeName")
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            if theme_name == "custom" {
                let file = json_file_value(&settings, "customThemeFile")
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
                let file = file.strip_prefix("~/").map(|r| format!("{}/{r}", home.home)).unwrap_or(file);
                if let Some(p) = std::fs::read_to_string(&file)
                    .ok()
                    .and_then(|t| palette_from_dms_theme(&t, dark))
                {
                    return Ok(p);
                }
            }
            match std::fs::read_to_string(home.dms_colors_file()) {
                Ok(t) => palette_from_material(&t, Some(dark)).ok_or_else(|| {
                    PullError::Refused(trf(
                        "The color file %1 could not be read.",
                        &[&home.dms_colors_file().to_string_lossy()],
                    ))
                }),
                Err(_) => Err(PullError::Refused(tr(
                    "DankMaterialShell has not saved its colors to a file: it needs the matugen package for that. Install matugen and change the theme in the shell once.",
                ))),
            }
        }
    }
}

/// Our Noctalia template is already registered.
pub fn bridge_connected(home: &PaletteHome) -> bool {
    std::fs::read_to_string(home.noctalia_state_file())
        .ok()
        .is_some_and(|t| toml_key_line(&t, &bridge_table(), "output_path").is_some())
}

fn bridge_table() -> String {
    format!("theme.templates.user.{NOCTALIA_BRIDGE_ID}")
}

/// Connect our Noctalia template: the shell will put its palette into
/// [`PaletteHome::bridge_output_file`] on every change.
pub fn connect_bridge(home: &PaletteHome) -> Result<(), String> {
    let table = bridge_table();
    let state = home.noctalia_state_file();
    let current = read_optional(&state).ok().flatten().unwrap_or_default();
    let mut edits = Vec::new();
    for (key, path) in [
        ("input_path", home.bridge_template_file()),
        ("output_path", home.bridge_output_file()),
    ] {
        let line = toml_string_line(key, &path.to_string_lossy());
        if toml_key_line(&current, &table, key).as_deref() != Some(line.as_str()) {
            edits.push(TomlEdit { table: table.clone(), key: key.into(), line: Some(line) });
        }
    }
    let mut changes = vec![(home.bridge_template_file(), FileChange::Own(noctalia_bridge_template()))];
    if !edits.is_empty() {
        changes.push((state, FileChange::Toml(edits)));
    }
    apply_changes(home, DesktopShell::Noctalia, &changes).map(|_| ())
}

/// The reply «Template connected, waiting for the shell».
pub fn bridge_pending_msg(shell: DesktopShell) -> String {
    trf(
        // TRANSLATORS: %1 is the shell name; “Take Shell Colors” is the button name.
        "Rexon asked %1 to share its colors, but the shell has not written them yet. Press “Take Shell Colors” again in a few seconds.",
        &[&shell.label()],
    )
}

/// A KDE scheme with the shell's colors: (name, path, text). The name alternates between
/// two if our scheme is already set (see [`SHELL_SCHEME_IDS`]); `None` —
/// the scheme set is exactly this one already, nothing to apply.
pub fn shell_scheme_file(home: &PaletteHome, p: &Palette) -> Option<(String, PathBuf, String)> {
    let current = current_scheme_name(home);
    let name = shell_scheme_display_name();
    if let Some(cur) = current.as_deref() {
        if SHELL_SCHEME_IDS.contains(&cur) {
            let path = home.color_scheme_file(cur);
            let text = kde_scheme_text(p, cur, &name);
            if std::fs::read_to_string(&path).ok().as_deref() == Some(text.as_str()) {
                return None;
            }
            let other = SHELL_SCHEME_IDS.iter().find(|s| **s != cur).copied().unwrap_or(SHELL_SCHEME_IDS[0]);
            let path = home.color_scheme_file(other);
            return Some((other.to_string(), path, kde_scheme_text(p, other, &name)));
        }
    }
    let id = SHELL_SCHEME_IDS[0];
    Some((id.to_string(), home.color_scheme_file(id), kde_scheme_text(p, id, &name)))
}

/// Write the scheme with the shell's colors and remember which one was set before us
/// (only on the first pass — a second pass must not overwrite «before us»
/// with our own scheme).
pub fn write_shell_scheme(home: &PaletteHome, path: &Path, text: &str) -> Result<(), String> {
    let mut memo = read_memo(home);
    let prev = current_scheme_name(home);
    let env = memo.env.get_or_insert_with(|| EnvMemo {
        prev_scheme: prev.filter(|s| !SHELL_SCHEME_IDS.contains(&s.as_str())),
        scheme_files: Vec::new(),
    });
    let p = path.to_string_lossy().to_string();
    if !env.scheme_files.contains(&p) {
        env.scheme_files.push(p);
    }
    save_memo(home, &memo)?;
    write_text(path, text)
}

/// Result of «Take Shell Colors».
pub fn pulled_msg(shell: DesktopShell) -> String {
    trf(
        // TRANSLATORS: %1 is the shell name; %2 is the name of the color scheme.
        "The colors of %1 are applied as the color scheme “%2”.",
        &[&shell.label(), &shell_scheme_display_name()],
    )
}

/// A repeated «Take Shell Colors» without changes in the shell.
pub fn pulled_same_msg(shell: DesktopShell) -> String {
    trf(
        // TRANSLATORS: %1 is the shell name.
        "The colors of %1 are already applied — nothing to change.",
        &[&shell.label()],
    )
}

// ============================================================================
// Return as it was
// ============================================================================

/// What the caller still has to do after the files are restored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevertOutcome {
    /// Text for the person.
    pub message: String,
    /// The KDE scheme to apply back (the one set before Rexon).
    pub reapply_scheme: Option<String>,
    /// Our scheme files — remove AFTER the previous scheme is in place
    /// ([`forget_env`]).
    pub drop_scheme_files: Vec<String>,
    /// Keys that could not be restored (the file was rewritten beyond
    /// recognition) — for an honest text.
    pub skipped: Vec<String>,
}

fn revert_file(fm: &FileMemo) -> Result<Option<String>, String> {
    let path = Path::new(&fm.path);
    let current = read_optional(path)?;
    // Nobody touched it after us — the previous content bit for bit.
    if current.as_deref() == Some(fm.written.as_str()) {
        match &fm.original {
            Some(orig) => write_text(path, orig)?,
            None => remove_file(path)?,
        }
        let _ = remove_file(&backup_path(path));
        return Ok(None);
    }
    let Some(current) = current else {
        // The file is gone — the shell or the person removed it.
        return Ok(None);
    };
    match fm.kind.as_str() {
        "toml" => {
            let edits: Vec<TomlEdit> = fm
                .keys
                .iter()
                .map(|k| TomlEdit { table: k.table.clone(), key: k.key.clone(), line: k.prev.clone() })
                .collect();
            match toml_apply(&current, &edits, &fm.created_tables) {
                Ok(text) => {
                    if text != current {
                        write_text(path, &text)?;
                    }
                    Ok(None)
                }
                Err(e) => Ok(Some(format!("{} ({})", fm.path, e.reason()))),
            }
        }
        "json" => {
            let mut text = current.clone();
            for k in &fm.keys {
                match json_set_member(&text, &k.key, k.prev.as_deref()) {
                    Ok(t) => text = t,
                    Err(e) => return Ok(Some(format!("{} ({})", fm.path, e.reason()))),
                }
            }
            if text != current {
                write_text(path, &text)?;
            }
            Ok(None)
        }
        // Someone rewrote our file — it is no longer ours, leave it alone.
        _ => Ok(Some(fm.path.clone())),
    }
}

/// Return the shell's files to what they were before Rexon. The caller applies the
/// environment scheme itself (`reapply_scheme`), then calls [`forget_env`].
pub fn revert(home: &PaletteHome) -> Result<RevertOutcome, String> {
    let mut memo = read_memo(home);
    if !memo.has_changes() {
        return Ok(RevertOutcome {
            message: tr("Nothing to return: Rexon has not changed the shell's palette."),
            ..Default::default()
        });
    }
    let shell_label = match memo.shell.as_str() {
        "noctalia" => DesktopShell::Noctalia.label(),
        "dms" => DesktopShell::DankMaterialShell.label(),
        _ => "",
    };
    let mut out = RevertOutcome::default();
    // In reverse order: edited last — restored first.
    for fm in memo.files.iter().rev() {
        if let Some(skip) = revert_file(fm)? {
            out.skipped.push(skip);
        }
    }
    memo.files.clear();
    if let Some(env) = &memo.env {
        out.reapply_scheme = env.prev_scheme.clone();
        out.drop_scheme_files = env.scheme_files.clone();
    }
    if out.reapply_scheme.is_none() {
        // Nothing to apply back — remove our schemes right away.
        for f in &out.drop_scheme_files {
            let _ = remove_file(Path::new(f));
        }
        out.drop_scheme_files.clear();
        memo.env = None;
    }
    save_memo(home, &memo)?;
    out.message = if out.skipped.is_empty() {
        if shell_label.is_empty() {
            tr("The colors are back as they were before Rexon.")
        } else {
            trf(
                // TRANSLATORS: %1 is the shell name.
                "The palette of %1 is back as it was before Rexon.",
                &[&shell_label],
            )
        }
    } else {
        trf(
            // TRANSLATORS: %1 is a list of files.
            "Mostly returned. These files were changed by someone else since, and Rexon left them as they are: %1",
            &[&out.skipped.join(", ")],
        )
    };
    Ok(out)
}

/// The previous scheme is set again — remove our scheme files and the record about
/// the environment.
pub fn forget_env(home: &PaletteHome) -> Result<(), String> {
    let mut memo = read_memo(home);
    if let Some(env) = memo.env.take() {
        for f in &env.scheme_files {
            let _ = remove_file(Path::new(f));
        }
    }
    save_memo(home, &memo)
}

// ============================================================================
// What to show the person
// ============================================================================

/// The «Shell palette» bar in «Appearance».
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PaletteBanner {
    pub visible: bool,
    /// The shell's `as_str`.
    pub shell: String,
    /// Warning about the dispute over dark/light mode.
    pub text: String,
    /// `rexon`, `shell` or empty (not chosen).
    pub lead: String,
    pub lead_caption: String,
    pub lead_rexon_label: String,
    pub lead_shell_label: String,
    pub push_label: String,
    pub pull_label: String,
    pub revert_label: String,
    /// There is something to pass on (`kdeglobals` has colors).
    pub can_push: bool,
    pub can_revert: bool,
}

pub fn push_label() -> String {
    // TRANSLATORS: button; sends the current color scheme's palette (and dark/light mode) to the desktop shell.
    tr("Pass Palette to Shell")
}
pub fn pull_label() -> String {
    // TRANSLATORS: button; builds a color scheme from the desktop shell's colors and applies it.
    tr("Take Shell Colors")
}
pub fn revert_label() -> String {
    // TRANSLATORS: button; undoes what Rexon changed in the shell's palette settings.
    tr("Return as It Was")
}

/// The bar from ready observations.
pub fn palette_banner(detected: Option<DesktopShell>, home: &PaletteHome) -> PaletteBanner {
    let Some(shell) = detected else {
        return PaletteBanner::default();
    };
    let memo = read_memo(home);
    let text = match shell {
        DesktopShell::Noctalia => trf(
            // TRANSLATORS: %1 is the shell name (Noctalia).
            "%1 paints its own panels and, at every start and theme change, sets the dark or light mode of applications itself. So that the mode does not jump back and forth, choose who leads the palette: Rexon passes its color scheme to the shell together with the mode, or Rexon takes the shell's colors.",
            &[&shell.label()],
        ),
        DesktopShell::DankMaterialShell => trf(
            // TRANSLATORS: %1 is the shell name (DankMaterialShell).
            "%1 paints its own panels and, with its default “sync mode with portal” setting, sets the dark or light mode of applications itself at every theme change. So that the mode does not jump back and forth, choose who leads the palette: Rexon passes its color scheme to the shell together with the mode, or Rexon takes the shell's colors.",
            &[&shell.label()],
        ),
    };
    PaletteBanner {
        visible: true,
        shell: shell.as_str().to_string(),
        text,
        lead: memo.lead.map(|l| l.as_str().to_string()).unwrap_or_default(),
        lead_caption: tr("Palette led by:"),
        // TRANSLATORS: choice chip after “Palette led by:”.
        lead_rexon_label: crate::i18n::trc("palette lead", "Rexon"),
        lead_shell_label: trc_shell(),
        push_label: push_label(),
        pull_label: pull_label(),
        revert_label: revert_label(),
        can_push: current_rexon_palette(home).is_some(),
        can_revert: memo.has_changes(),
    }
}

fn trc_shell() -> String {
    // TRANSLATORS: choice chip after “Palette led by:” — the desktop shell (Noctalia, DankMaterialShell) leads.
    crate::i18n::trc("palette lead", "Shell")
}

/// What to add to the result of applying a color scheme in Rexon with a live
/// shell. Rexon leads the palette — the scheme goes to the shell right away; the shell leads —
/// honestly say that the mode stays with it; not chosen — point to the
/// button.
pub fn after_scheme_applied(detected: Option<DesktopShell>, home: &PaletteHome) -> Option<String> {
    let shell = detected?;
    match read_memo(home).lead {
        Some(Lead::Rexon) => Some(match current_rexon_palette(home) {
            Some(p) => match push_palette(shell, &p, home) {
                Ok(m) | Err(m) => m,
            },
            None => no_rexon_palette_msg(),
        }),
        Some(Lead::Shell) => Some(trf(
            // TRANSLATORS: %1 is the shell name.
            "The palette is led by %1: the scheme is not passed to it, and the shell keeps its own dark/light mode for applications.",
            &[&shell.label()],
        )),
        None => Some(trf(
            // TRANSLATORS: %1 is the shell name; “Pass Palette to Shell” is the button name.
            "%1 paints with its own palette. To give it this scheme, press “Pass Palette to Shell” at the top of the page.",
            &[&shell.label()],
        )),
    }
}

// ============================================================================
// Live session and the course of actions
// ============================================================================

/// The live shell of this session — via the shared observation gathering of the shell wallpaper module
/// (`desktop_shell::LiveFacts`): the session directory, `/proc` of our own
/// user and our own environment, without external processes.
pub fn live_detect() -> Option<DesktopShell> {
    crate::desktop_shell::LiveFacts::collect().detect().map(|d| d.shell)
}

/// The bar from the live session.
pub fn live_banner() -> PaletteBanner {
    palette_banner(live_detect(), &PaletteHome::live())
}

/// No live shell — nobody to act on.
fn no_shell_msg() -> String {
    tr("No running desktop shell (Noctalia, DankMaterialShell) was found in this session.")
}

/// «Pass Palette to Shell».
pub fn push_flow(shell: Option<DesktopShell>, home: &PaletteHome) -> (bool, String) {
    let Some(shell) = shell else { return (false, no_shell_msg()) };
    let Some(p) = current_rexon_palette(home) else { return (false, no_rexon_palette_msg()) };
    match push_palette(shell, &p, home) {
        Ok(m) => (true, m),
        Err(m) => (false, m),
    }
}

/// «Take Shell Colors». `nudge` asks the shell to fill our template
/// (live — `noctalia msg templates-apply`), `wait_ms` — how long to wait for
/// the file after connecting the template; `apply` applies a KDE scheme by name
/// the same way as the «Color Schemes» section and answers `(ok, text)`.
pub fn pull_flow(
    shell: Option<DesktopShell>,
    home: &PaletteHome,
    nudge: &mut dyn FnMut(),
    wait_ms: u64,
    apply: &mut dyn FnMut(&str) -> (bool, String),
) -> (bool, String) {
    let Some(shell) = shell else { return (false, no_shell_msg()) };
    let mut got = shell_palette(shell, home);
    if matches!(got, Err(PullError::NeedsBridge) | Err(PullError::BridgePending)) {
        if got == Err(PullError::NeedsBridge) {
            if let Err(m) = connect_bridge(home) {
                return (false, m);
            }
        }
        nudge();
        let step = 200;
        let mut waited = 0;
        loop {
            got = shell_palette(shell, home);
            if got != Err(PullError::BridgePending) || waited >= wait_ms {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(step));
            waited += step;
        }
    }
    let p = match got {
        Ok(p) => p,
        Err(PullError::Refused(m)) => return (false, m),
        Err(_) => return (false, bridge_pending_msg(shell)),
    };
    let Some((id, path, text)) = shell_scheme_file(home, &p) else {
        return (true, pulled_same_msg(shell));
    };
    if let Err(e) = write_shell_scheme(home, &path, &text) {
        return (false, e);
    }
    let (ok, msg) = apply(&id);
    if ok {
        (true, pulled_msg(shell))
    } else {
        (false, msg)
    }
}

/// «Return as It Was»: the shell's files, then the environment's previous scheme.
pub fn revert_flow(home: &PaletteHome, apply: &mut dyn FnMut(&str) -> (bool, String)) -> (bool, String) {
    let out = match revert(home) {
        Ok(o) => o,
        Err(e) => return (false, e),
    };
    if let Some(prev) = &out.reapply_scheme {
        let (ok, msg) = apply(prev);
        if !ok {
            return (false, msg);
        }
        if let Err(e) = forget_env(home) {
            return (false, e);
        }
    }
    (out.skipped.is_empty(), out.message)
}

/// Ask Noctalia to redraw the templates now rather than at the next
/// palette change. No command or the shell did not answer — no problem: the template will
/// be filled at the next change anyway.
pub fn nudge_noctalia_live() {
    if crate::utils::which("noctalia", None).is_none() {
        return;
    }
    let _ = crate::utils::run(
        &crate::utils::RunSpec {
            argv: vec!["noctalia".into(), "msg".into(), "templates-apply".into()],
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(5),
            ..Default::default()
        },
        None,
    );
}

// ============================================================================
// KDE color scheme outside Plasma
// ============================================================================
//
// WHY. In Plasma the scheme is applied by `plasma-apply-colorscheme`: it merges
// the scheme's colors into `~/.config/kdeglobals`, and the Plasma platform theme
// hands the palette to programs. Outside Plasma (GNOME, Cinnamon, XFCE, MATE, Budgie,
// Pantheon, LXQt, COSMIC, tiling ones) this link does not exist, and the «Color
// Schemes» section used to exist only in KDE (rule «a setting of one environment — in
// all of them»). Here Rexon builds the link itself — in three layers, each for its own programs:
//
//   * `kdeglobals` — the same groups and keys Plasma itself writes when
//     applying a scheme (checked against `kcms/colors/colorsapplicator.cpp`
//     `applyScheme` and `plasma-apply-colorscheme.cpp` from plasma-workspace):
//     `[Colors:View|Window|Button|Selection|Tooltip|Complementary|Header]`
//     (12 keys each, `[Colors:Header]` — only if the scheme has it,
//     the subgroup `[…][Inactive]` — if the scheme has it), `[WM]` (6 title
//     colors), `[KDE] contrast / frameContrast`, `[ColorEffects:Inactive
//     |Disabled]`, `[General] ColorScheme / ColorSchemeHash` (SHA-1 of the scheme
//     file — by it startplasma learns that the scheme file changed). These colors
//     are read by KDE programs via `KColorScheme`.
//   * `[UiSettings] ColorScheme` in the same `kdeglobals` — outside Plasma there is no
//     way around it. KDE programs with a «Color Scheme» menu (Dolphin, Kate,
//     Okular…) create a `KColorSchemeManager`, and with a non-Plasma platform theme it
//     sets Breeze or Breeze Dark for the program BY ITSELF according to the light/
//     dark mode — the `kdeglobals` colors mean nothing to it (kcolorscheme,
//     `kcolorschememanager.cpp` `init` / `automaticColorSchemeId`). It takes the scheme from
//     `[UiSettings] ColorScheme` first, and the program's settings file
//     cascades into `kdeglobals` too — so one key reaches everyone.
//     ⚠️ In Plasma this key would get in the way: programs would hold on to the scheme and not
//     follow a scheme change in System Settings. So when applying a scheme in Plasma,
//     Rexon removes its key (if nobody has changed it since).
//   * The Qt palette for the remaining Qt programs:
//       - LXQt — the native key: `~/.config/lxqt/lxqt.conf [Palette]`
//         (`window_color`, `base_color`, … — `LXQtPlatformTheme::loadSettings`
//         in lxqt-qtplugin; the platform theme rereads the file itself);
//       - the rest — qt6ct (and qt5ct if present): our file
//         `<tool>/colors/<scheme>.conf` (`[ColorScheme] active_colors /
//         inactive_colors / disabled_colors` — `QPalette` role colors in
//         role order, `#aarrggbb`; format — `Qt6CT::loadColorScheme`) and in
//         `<tool>.conf [Appearance] custom_palette=true`,
//         `color_scheme_path=<path>` (that is how qt6ct's own «Appearance» page
//         writes it). If the session does not hand Qt programs to
//         qt6ct yet, our variables file `environment.d/rexon-qt-style.conf`
//         (the same one used by Kvantum and «Application Style»; while Plasma,
//         LXQt or Deepin is installed — `rexon/session-env/rexon-qt-style.conf`,
//         see `themes::DESKTOP_BOUND_ENV_DIR`) gets
//         `QT_QPA_PLATFORMTHEME=qt6ct` — then the palette appears after the
//         next login. qt6ct did not know the icons before that — we put the current
//         environment icons, otherwise Qt programs would be left with `hicolor`.
//
// Palette roles are derived from the scheme's color sets the same way
// KDE does for its programs (Window → window and window text, View → input
// fields and lists, Button, Selection → selection, Tooltip, links and
// placeholder text — from View). Frame shades (Light/Mid/Dark/Shadow) and the look of
// inactive/disabled elements are computed more simply than in KDE: close in
// meaning, but not digit for digit.
//
// GTK programs do not read the KDE scheme at all — they have their own colors in the GTK theme.
// The person is told this in the section's caveat (the `de_customization` matrix).
//
// Rollback — as for the shell palette: what existed before Rexon is remembered in
// `$XDG_STATE_HOME/rexon/color-scheme-layer.json` BEFORE writing. «Return as
// It Was»: if the file has not been touched since — the previous content bit for bit; if it was touched —
// only our keys return to their previous values, other edits stay.
// Our qt6ct palette files are deleted; the variables file is restored only
// if nobody has rewritten it since (applying Kvantum maintains it too).
//
// ⚠️ Boundary: NOT verified live outside Plasma — only with fake files
// (`tests/shell_palette.rs`); format and keys follow the sources of KDE, qt6ct and
// LXQt (see above).

/// Color sets Plasma merges into `kdeglobals` (in the order of
/// `applyScheme`).
pub const SCHEME_COLOR_SETS: &[&str] = &[
    "Colors:View",
    "Colors:Window",
    "Colors:Button",
    "Colors:Selection",
    "Colors:Tooltip",
    "Colors:Complementary",
    "Colors:Header",
];

/// Color set keys that Plasma carries over.
pub const SCHEME_COLOR_KEYS: &[&str] = &[
    "BackgroundNormal",
    "BackgroundAlternate",
    "ForegroundNormal",
    "ForegroundInactive",
    "ForegroundActive",
    "ForegroundLink",
    "ForegroundVisited",
    "ForegroundNegative",
    "ForegroundNeutral",
    "ForegroundPositive",
    "DecorationFocus",
    "DecorationHover",
];

/// Window title colors `[WM]`.
const SCHEME_WM_KEYS: &[&str] = &[
    "activeBackground",
    "activeForeground",
    "inactiveBackground",
    "inactiveForeground",
    "activeBlend",
    "inactiveBlend",
];

/// Effect groups of the inactive and disabled states.
const SCHEME_EFFECT_GROUPS: &[&str] = &["ColorEffects:Inactive", "ColorEffects:Disabled"];

/// Effect keys.
const SCHEME_EFFECT_KEYS: &[&str] = &[
    "Enable",
    "ChangeSelectionColor",
    "IntensityEffect",
    "IntensityAmount",
    "ColorEffect",
    "ColorAmount",
    "Color",
    "ContrastEffect",
    "ContrastAmount",
];

// ── Editing INI in KConfig/QSettings format by lines ──────────────────────
//
// A foreign file (`kdeglobals`, `qt6ct.conf`, `lxqt.conf`) is edited in place:
// only our keys change, the other lines stay byte for byte.
// The nested KConfig group `[Colors:Header][Inactive]` is called
// `Colors:Header][Inactive` here — as [`kde_groups`] sees it.

/// One edit: `value = None` — remove the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniEdit {
    pub group: String,
    pub key: String,
    pub value: Option<String>,
}

impl IniEdit {
    fn set(group: &str, key: &str, value: impl Into<String>) -> IniEdit {
        IniEdit { group: group.to_string(), key: key.to_string(), value: Some(value.into()) }
    }
    fn unset(group: &str, key: &str) -> IniEdit {
        IniEdit { group: group.to_string(), key: key.to_string(), value: None }
    }
}

/// An INI group: name (`None` — lines before the first group) and its lines without
/// the header.
struct IniSection {
    name: Option<String>,
    header: String,
    lines: Vec<String>,
}

fn ini_header_name(line: &str) -> Option<String> {
    let t = line.trim();
    (t.len() >= 2 && t.starts_with('[') && t.ends_with(']')).then(|| t[1..t.len() - 1].to_string())
}

fn ini_line_key(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
        return None;
    }
    t.split_once('=').map(|(k, _)| k.trim())
}

fn ini_sections(text: &str) -> Vec<IniSection> {
    let mut out = vec![IniSection { name: None, header: String::new(), lines: Vec::new() }];
    for line in text.lines() {
        if let Some(name) = ini_header_name(line) {
            out.push(IniSection { name: Some(name), header: line.to_string(), lines: Vec::new() });
        } else {
            out.last_mut().expect("есть хотя бы преамбула").lines.push(line.to_string());
        }
    }
    out
}

fn ini_render(sections: &[IniSection]) -> String {
    let mut out = String::new();
    for s in sections {
        if s.name.is_some() {
            out.push_str(&s.header);
            out.push('\n');
        }
        for l in &s.lines {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

/// Key value (the last one, as in KConfig). `None` — no key.
pub fn ini_get(text: &str, group: &str, key: &str) -> Option<String> {
    let mut found = None;
    for s in ini_sections(text) {
        if s.name.as_deref() != Some(group) {
            continue;
        }
        for l in &s.lines {
            if ini_line_key(l) == Some(key) {
                found = l.split_once('=').map(|(_, v)| v.trim().to_string());
            }
        }
    }
    found
}

/// Group keys in order (no repeats).
fn ini_keys(text: &str, group: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in ini_sections(text) {
        if s.name.as_deref() != Some(group) {
            continue;
        }
        for l in &s.lines {
            if let Some(k) = ini_line_key(l) {
                if !out.iter().any(|x| x == k) {
                    out.push(k.to_string());
                }
            }
        }
    }
    out
}

/// Apply edits in order. A new key goes at the end of its group
/// (before empty lines), a new group — at the end of the file. A group from
/// which an edit removed the last key disappears together with its header.
pub fn ini_apply(text: Option<&str>, edits: &[IniEdit]) -> String {
    let mut sections = ini_sections(text.unwrap_or_default());
    for e in edits {
        let pos = sections.iter().position(|s| s.name.as_deref() == Some(e.group.as_str()));
        match (&e.value, pos) {
            (Some(v), Some(i)) => {
                let line = format!("{}={v}", e.key);
                let s = &mut sections[i];
                let mut replaced = false;
                s.lines.retain_mut(|l| {
                    if ini_line_key(l) != Some(e.key.as_str()) {
                        return true;
                    }
                    if replaced {
                        return false; // a repeated key is redundant
                    }
                    *l = line.clone();
                    replaced = true;
                    true
                });
                if !replaced {
                    let at = s.lines.iter().rposition(|l| !l.trim().is_empty()).map(|p| p + 1).unwrap_or(0);
                    s.lines.insert(at, line);
                }
            }
            (Some(v), None) => {
                // Separate the new group from the previous one with an empty line.
                if let Some(last) = sections.last_mut() {
                    let has_text = last.name.is_some() || last.lines.iter().any(|l| !l.trim().is_empty());
                    if has_text && last.lines.last().is_none_or(|l| !l.trim().is_empty()) {
                        last.lines.push(String::new());
                    }
                }
                sections.push(IniSection {
                    name: Some(e.group.clone()),
                    header: format!("[{}]", e.group),
                    lines: vec![format!("{}={v}", e.key)],
                });
            }
            (None, Some(i)) => {
                let s = &mut sections[i];
                let before = s.lines.len();
                s.lines.retain(|l| ini_line_key(l) != Some(e.key.as_str()));
                if s.lines.len() != before && s.lines.iter().all(|l| l.trim().is_empty()) {
                    sections.remove(i);
                    // The group was the last one — the separator empty line before
                    // it is no longer needed.
                    if i == sections.len() {
                        if let Some(prev) = sections.last_mut() {
                            while prev.lines.last().is_some_and(|l| l.trim().is_empty()) {
                                prev.lines.pop();
                            }
                        }
                    }
                }
            }
            (None, None) => {}
        }
    }
    ini_render(&sections)
}

// ── Scheme colors ──────────────────────────────────────────────────────────

/// Default values for a missing set key (light Breeze —
/// that is how KDE completes an incomplete scheme too).
fn default_scheme_color(key: &str) -> Rgb {
    match key {
        "BackgroundAlternate" => Rgb(227, 229, 231),
        "ForegroundNormal" => Rgb(35, 38, 41),
        "ForegroundInactive" => Rgb(112, 125, 138),
        "ForegroundActive" | "DecorationFocus" => Rgb(61, 174, 233),
        "ForegroundLink" => Rgb(41, 128, 185),
        "ForegroundVisited" => Rgb(127, 140, 141),
        "ForegroundNegative" => Rgb(218, 68, 83),
        "ForegroundNeutral" => Rgb(246, 116, 0),
        "ForegroundPositive" => Rgb(39, 174, 96),
        "DecorationHover" => Rgb(147, 206, 233),
        _ => Rgb(239, 240, 241),
    }
}

/// A parsed KDE scheme: a set color with fallback (no key — the same key of
/// the window set, no that either — light Breeze).
struct SchemeColors {
    groups: BTreeMap<String, BTreeMap<String, String>>,
}

impl SchemeColors {
    fn new(text: &str) -> SchemeColors {
        SchemeColors { groups: kde_groups(text) }
    }
    fn raw(&self, group: &str, key: &str) -> Option<&String> {
        self.groups.get(group).and_then(|m| m.get(key))
    }
    fn has_group(&self, group: &str) -> bool {
        self.groups.contains_key(group)
    }
    fn color(&self, set: &str, key: &str) -> Rgb {
        let group = format!("Colors:{set}");
        self.raw(&group, key)
            .and_then(|v| Rgb::parse(v))
            .or_else(|| self.raw("Colors:Window", key).and_then(|v| Rgb::parse(v)))
            .unwrap_or_else(|| default_scheme_color(key))
    }
    /// Window title: the Header set, and without it — Window (that is how KDE draws
    /// a scheme without its own header).
    fn header(&self, key: &str, inactive: bool) -> Rgb {
        if inactive {
            if let Some(c) = self.raw("Colors:Header][Inactive", key).and_then(|v| Rgb::parse(v)) {
                return c;
            }
        }
        if self.has_group("Colors:Header") {
            self.color("Header", key)
        } else {
            self.color("Window", key)
        }
    }
    /// The «fading» share of disabled text (`[ColorEffects:Disabled]`,
    /// the «toward background» contrast effect; KDE's default is 0.65).
    fn disabled_fade(&self) -> f64 {
        let g = "ColorEffects:Disabled";
        let effect = self.raw(g, "ContrastEffect").and_then(|v| v.parse::<i32>().ok()).unwrap_or(1);
        if effect == 0 {
            return 0.0;
        }
        self.raw(g, "ContrastAmount")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.65)
            .clamp(0.0, 1.0)
    }
}

/// A color in KConfig form (`r,g,b`).
fn kcolor(c: Rgb) -> String {
    c.kde()
}

/// `kdeglobals` edits with which Plasma applies a scheme (`applyScheme`), and
/// on top of them — `[UiSettings] ColorScheme` outside Plasma (`pin`).
///
/// There is one deliberate difference from Plasma: effects (`[ColorEffects:*]`) that
/// the scheme lacks are written by Plasma as empty strings, while here the key is removed —
/// in meaning it is the same («default»), but without empty values.
/// The accent color (`[General] AccentColor`) is not mixed in here — it is
/// added by [`crate::accent::scheme_accent_edits`] in [`plan_scheme_layer`].
pub fn scheme_kdeglobals_edits(kdeglobals: &str, scheme_text: &str, id: &str, hash: &str, pin: bool) -> Vec<IniEdit> {
    let s = SchemeColors::new(scheme_text);
    let mut edits = Vec::new();
    for set in SCHEME_COLOR_SETS {
        let inactive = format!("{set}][Inactive");
        for group in [set.to_string(), inactive.clone()] {
            for k in ini_keys(kdeglobals, &group) {
                edits.push(IniEdit::unset(&group, &k));
            }
        }
        if *set == "Colors:Header" && !s.has_group(set) {
            continue;
        }
        for key in SCHEME_COLOR_KEYS {
            if let Some(v) = s.raw(set, key) {
                edits.push(IniEdit::set(set, key, v.clone()));
            }
        }
        if s.has_group(&inactive) {
            for key in SCHEME_COLOR_KEYS {
                // Plasma takes the inactive title from the window background.
                let src = if *set == "Colors:Header" { s.raw("Colors:Window", key) } else { s.raw(&inactive, key) };
                if let Some(v) = src {
                    edits.push(IniEdit::set(&inactive, key, v.clone()));
                }
            }
        }
    }
    for key in SCHEME_WM_KEYS {
        let value = s.raw("WM", key).cloned().unwrap_or_else(|| {
            kcolor(match *key {
                "activeBackground" | "activeBlend" => s.header("BackgroundNormal", false),
                "activeForeground" => s.header("ForegroundNormal", false),
                "inactiveBackground" | "inactiveBlend" => s.header("BackgroundNormal", true),
                _ => s.header("ForegroundNormal", true),
            })
        });
        edits.push(IniEdit::set("WM", key, value));
    }
    edits.push(IniEdit::set(
        "KDE",
        "frameContrast",
        s.raw("KDE", "frameContrast").cloned().unwrap_or_else(|| "0.2".to_string()),
    ));
    edits.push(IniEdit::set("KDE", "contrast", s.raw("KDE", "contrast").cloned().unwrap_or_else(|| "7".to_string())));
    for group in SCHEME_EFFECT_GROUPS {
        for key in SCHEME_EFFECT_KEYS {
            edits.push(match s.raw(group, key) {
                Some(v) => IniEdit::set(group, key, v.clone()),
                None => IniEdit::unset(group, key),
            });
        }
    }
    edits.push(IniEdit::set("General", "ColorScheme", id));
    if !hash.is_empty() {
        edits.push(IniEdit::set("General", "ColorSchemeHash", hash));
    }
    if pin {
        edits.push(IniEdit::set("UiSettings", "ColorScheme", id));
    }
    edits
}

/// `QPalette` roles in order (Qt 6: `WindowText` = 0 … `Accent` = 21).
/// Qt 5 and Qt 6 before 6.6 know the first 21 roles and skip the extra last one;
/// for 21 roles qt6ct derives `Accent` from the selection itself.
fn qt_palette_roles(s: &SchemeColors, disabled: bool) -> Vec<Rgb> {
    let window = s.color("Window", "BackgroundNormal");
    let view = s.color("View", "BackgroundNormal");
    let button = s.color("Button", "BackgroundNormal");
    let selection = s.color("Selection", "BackgroundNormal");
    let tooltip = |k: &str| if s.has_group("Colors:Tooltip") { s.color("Tooltip", k) } else { s.color("Window", k) };
    let fade = if disabled { s.disabled_fade() } else { 0.0 };
    let fg = |c: Rgb, bg: Rgb| if fade > 0.0 { c.mix(bg, fade) } else { c };
    let white = Rgb(255, 255, 255);
    vec![
        fg(s.color("Window", "ForegroundNormal"), window), // WindowText
        button,                                            // Button
        window.mix(white, 0.35),                           // Light
        window.mix(white, 0.15),                           // Midlight
        window.mix(BLACK, 0.45),                           // Dark
        window.mix(BLACK, 0.25),                           // Mid
        fg(s.color("View", "ForegroundNormal"), view),     // Text
        white,                                             // BrightText
        fg(s.color("Button", "ForegroundNormal"), button), // ButtonText
        view,                                              // Base
        window,                                            // Window
        window.mix(BLACK, 0.75),                           // Shadow
        selection,                                         // Highlight
        fg(s.color("Selection", "ForegroundNormal"), selection), // HighlightedText
        fg(s.color("View", "ForegroundLink"), view),       // Link
        fg(s.color("View", "ForegroundVisited"), view),    // LinkVisited
        s.color("View", "BackgroundAlternate"),            // AlternateBase
        BLACK,                                             // NoRole
        tooltip("BackgroundNormal"),                       // ToolTipBase
        tooltip("ForegroundNormal"),                       // ToolTipText
        fg(s.color("View", "ForegroundInactive"), view),   // PlaceholderText
        selection,                                         // Accent
    ]
}

/// qt6ct/qt5ct palette file from a KDE scheme (`[ColorScheme]`, colors
/// `#aarrggbb`). The inactive window has the same colors as the active one (that is how
/// the standard KDE schemes look too: their inactive effect is turned off).
pub fn qtct_palette_text(scheme_text: &str) -> String {
    let s = SchemeColors::new(scheme_text);
    let list = |roles: Vec<Rgb>| roles.iter().map(|c| format!("#ff{}", &c.hex()[1..])).collect::<Vec<_>>().join(", ");
    let active = list(qt_palette_roles(&s, false));
    let disabled = list(qt_palette_roles(&s, true));
    format!(
        "[ColorScheme]\nactive_colors={active}\ndisabled_colors={disabled}\ninactive_colors={active}\n"
    )
}

/// Edits of `lxqt.conf [Palette]` — LXQt's native palette.
pub fn lxqt_palette_edits(scheme_text: &str) -> Vec<IniEdit> {
    let s = SchemeColors::new(scheme_text);
    let tooltip = |k: &str| if s.has_group("Colors:Tooltip") { s.color("Tooltip", k) } else { s.color("Window", k) };
    [
        ("window_color", s.color("Window", "BackgroundNormal")),
        ("base_color", s.color("View", "BackgroundNormal")),
        ("highlight_color", s.color("Selection", "BackgroundNormal")),
        ("window_text_color", s.color("Window", "ForegroundNormal")),
        ("text_color", s.color("View", "ForegroundNormal")),
        ("highlighted_text_color", s.color("Selection", "ForegroundNormal")),
        ("link_color", s.color("View", "ForegroundLink")),
        ("link_visited_color", s.color("View", "ForegroundVisited")),
        ("tooltip_base_color", tooltip("BackgroundNormal")),
        ("tooltip_text_color", tooltip("ForegroundNormal")),
    ]
    .into_iter()
    .map(|(k, c)| IniEdit::set("Palette", k, c.hex()))
    .collect()
}

/// SHA-1 of the scheme file as a hex string — `[General] ColorSchemeHash`.
pub fn scheme_hash(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    Sha1::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

// ── Environment and plan ───────────────────────────────────────────────────

/// How a color scheme is applied in this environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemeWay {
    /// Plasma: `plasma-apply-colorscheme`.
    Plasma,
    /// Outside Plasma — Rexon's layers. `lxqt` — the Qt palette in the native `lxqt.conf`,
    /// otherwise qt6ct.
    Layer { lxqt: bool },
    /// Deepin: programs are drawn with the Chameleon style, Deepin itself gives them
    /// the palette — the section is hidden with an explanation (`de_customization`).
    Unavailable,
}

/// The scheme application path by environment identifier
/// (`system::detect_current_de`). An unknown environment — Rexon's layers:
/// they do not depend on the shell.
pub fn scheme_way(de_id: &str) -> SchemeWay {
    use crate::de_customization::{de_from_id, De};
    match de_from_id(de_id) {
        Some(De::Kde) => SchemeWay::Plasma,
        Some(De::Deepin) => SchemeWay::Unavailable,
        Some(De::Lxqt) => SchemeWay::Layer { lxqt: true },
        _ => SchemeWay::Layer { lxqt: false },
    }
}

/// Where to look and where to write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemeCtx {
    pub de_id: String,
    pub home: PaletteHome,
    /// `$XDG_DATA_DIRS` in order (empty — `/usr/local/share`, `/usr/share`).
    pub data_dirs: Vec<String>,
    /// `$QT_QPA_PLATFORMTHEME` of this session.
    pub session_platform_theme: String,
    /// A desktop with its own Qt integration is installed
    /// ([`crate::themes::own_qt_desktop_installed`]): the variable then goes
    /// into Rexon's desktop-bound directory, not the shared `environment.d`
    /// that Plasma imports — the same decision as the Qt style's.
    pub qt_desktop_installed: bool,
}

impl SchemeCtx {
    pub fn live(de_id: &str) -> SchemeCtx {
        let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
        SchemeCtx {
            de_id: de_id.to_string(),
            home: PaletteHome::live(),
            data_dirs: dirs.split(':').filter(|d| !d.is_empty()).map(str::to_string).collect(),
            session_platform_theme: std::env::var("QT_QPA_PLATFORMTHEME").unwrap_or_default(),
            qt_desktop_installed: {
                let path = std::env::var("PATH").ok();
                crate::themes::own_qt_desktop_installed(&|b| crate::utils::which(b, path.as_deref()).is_some())
            },
        }
    }

    /// Scheme file by name: own schemes first, then system ones.
    pub fn scheme_candidates(&self, id: &str) -> Vec<PathBuf> {
        let mut out = vec![self.home.color_scheme_file(id)];
        let dirs: Vec<String> = if self.data_dirs.is_empty() {
            vec!["/usr/local/share".into(), "/usr/share".into()]
        } else {
            self.data_dirs.clone()
        };
        for d in dirs {
            out.push(Path::new(&d).join("color-schemes").join(format!("{id}.colors")));
        }
        out
    }

    /// Our Qt variables file: the shared one, or — while a desktop with its own
    /// Qt integration is installed — the desktop-bound one
    /// ([`crate::themes::qt_style_env_paths`]).
    fn qt_env_file(&self) -> PathBuf {
        let [shared, bound] = crate::themes::qt_style_env_paths(&self.home.config().to_string_lossy());
        PathBuf::from(if self.qt_desktop_installed { bound } else { shared })
    }

    /// The layer memo.
    pub fn layer_memo_file(&self) -> PathBuf {
        layer_memo_path(&self.home)
    }
}

/// The color layer memo file.
pub fn layer_memo_path(home: &PaletteHome) -> PathBuf {
    home.state().join("rexon/color-scheme-layer.json")
}

/// What is known about the machine before the plan (gathered by the executor).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayerFacts {
    /// Qt configurators the person may be using: the program exists
    /// on the system or its config already exists (`qt6ct`, `qt5ct`).
    pub qt_tools: Vec<String>,
    /// The environment's icon theme now (empty — unknown).
    pub icon_theme: String,
}

/// One write according to the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerWrite {
    pub path: PathBuf,
    /// `ini` — a foreign file, our keys are edited; `own` — the whole file is ours;
    /// `env` — our variables file, shared with the Qt style.
    pub kind: &'static str,
    pub text: String,
    /// Keys the edit touches (group, key) — for rollback.
    pub keys: Vec<(String, String)>,
}

/// Plan of the scheme layer outside Plasma.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayerPlan {
    pub writes: Vec<LayerWrite>,
    /// `QT_QPA_PLATFORMTHEME` was added to the variables file — the qt6ct palette
    /// will appear after the next login.
    pub env_added: bool,
    /// The palette for Qt programs (qt6ct/qt5ct or LXQt) was written.
    pub qt_layer: bool,
}

fn edit_keys(edits: &[IniEdit]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for e in edits {
        if !out.iter().any(|(g, k)| *g == e.group && *k == e.key) {
            out.push((e.group.clone(), e.key.clone()));
        }
    }
    out
}

/// Name of our palette file in `<tool>/colors`: the scheme name, and if a foreign file
/// with that name lies there — with a mark.
fn qtct_palette_path(dir: &Path, id: &str, ours: &dyn Fn(&Path) -> bool, exists: &dyn Fn(&Path) -> bool) -> PathBuf {
    let plain = dir.join(format!("{id}.conf"));
    if !exists(&plain) || ours(&plain) {
        return plain;
    }
    dir.join(format!("{id} (Rexon).conf"))
}

/// Plan of the scheme layer outside Plasma. `read` — file text (`None` — missing), `ours`
/// — whether Rexon wrote the file (per the memo).
pub fn plan_scheme_layer(
    ctx: &SchemeCtx,
    lxqt: bool,
    id: &str,
    scheme_text: &str,
    hash: &str,
    facts: &LayerFacts,
    read: &dyn Fn(&Path) -> Option<String>,
    ours: &dyn Fn(&Path) -> bool,
) -> LayerPlan {
    let mut plan = LayerPlan::default();
    let ini = |path: PathBuf, edits: Vec<IniEdit>| {
        let current = read(&path);
        LayerWrite { text: ini_apply(current.as_deref(), &edits), keys: edit_keys(&edits), path, kind: "ini" }
    };

    // 1. kdeglobals — colors for KDE programs and `[UiSettings]` for their
    //    `KColorSchemeManager`.
    let kdeglobals = ctx.home.kdeglobals();
    let current = read(&kdeglobals).unwrap_or_default();
    let mut kg_edits = scheme_kdeglobals_edits(&current, scheme_text, id, hash, true);
    // A custom accent color (`[General] AccentColor`) — on top of the scheme, as KDE's
    // `applyScheme` adds it; the Qt palette is built from the already tinted scheme.
    let accented;
    let scheme_text = match crate::accent::accent_from_kdeglobals(&current) {
        Some(accent) => {
            kg_edits.extend(crate::accent::scheme_accent_edits(scheme_text, accent));
            accented = crate::accent::accented_scheme(scheme_text, accent);
            accented.as_str()
        }
        None => scheme_text,
    };
    plan.writes.push(ini(kdeglobals, kg_edits));

    // 2. The Qt palette.
    if lxqt {
        plan.writes.push(ini(ctx.home.config().join("lxqt/lxqt.conf"), lxqt_palette_edits(scheme_text)));
        plan.qt_layer = true;
        return plan;
    }
    let palette = qtct_palette_text(scheme_text);
    for tool in &facts.qt_tools {
        let dir = ctx.home.config().join(tool);
        let file = qtct_palette_path(&dir.join("colors"), id, ours, &|p| read(p).is_some());
        let mut edits = vec![
            IniEdit::set("Appearance", "custom_palette", "true"),
            IniEdit::set("Appearance", "color_scheme_path", file.to_string_lossy()),
        ];
        let conf = dir.join(format!("{tool}.conf"));
        // qt6ct has no icons yet — the environment's current icons (otherwise
        // Qt programs under it would be left with `hicolor`).
        let conf_text = read(&conf).unwrap_or_default();
        if !facts.icon_theme.is_empty() && ini_get(&conf_text, "Appearance", "icon_theme").is_none_or(|v| v.is_empty()) {
            edits.push(IniEdit::set("Appearance", "icon_theme", facts.icon_theme.clone()));
        }
        plan.writes.push(LayerWrite { path: file, kind: "own", text: palette.clone(), keys: Vec::new() });
        plan.writes.push(ini(conf, edits));
        plan.qt_layer = true;
    }

    // 3. Session variable: Qt programs must use the qt6ct platform theme.
    if facts.qt_tools.iter().any(|t| t == "qt6ct") && !matches!(ctx.session_platform_theme.as_str(), "qt6ct" | "qt5ct") {
        let path = ctx.qt_env_file();
        let current = read(&path).unwrap_or_default();
        let has = current.lines().any(|l| l.trim_start().starts_with("QT_QPA_PLATFORMTHEME="));
        if !has {
            let mut text = current.clone();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str("QT_QPA_PLATFORMTHEME=qt6ct\n");
            plan.writes.push(LayerWrite {
                path,
                kind: "env",
                text,
                keys: vec![(String::new(), "QT_QPA_PLATFORMTHEME".into())],
            });
            plan.env_added = true;
        }
    }
    plan
}

// ── Memo and execution ─────────────────────────────────────────────────────

/// What Rexon wrote with the scheme layer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerMemo {
    #[serde(default)]
    pub files: Vec<FileMemo>,
    /// The last scheme applied by the layer.
    #[serde(default)]
    pub scheme: Option<String>,
}

impl LayerMemo {
    /// There is something to restore.
    pub fn has_changes(&self) -> bool {
        !self.files.is_empty()
    }
    fn file(&self, path: &Path) -> Option<&FileMemo> {
        let p = path.to_string_lossy();
        self.files.iter().find(|f| f.path == p)
    }
}

/// The layer memo from disk (missing or broken — empty).
pub fn read_layer_memo(home: &PaletteHome) -> LayerMemo {
    read_optional(&layer_memo_path(home))
        .ok()
        .flatten()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_layer_memo(home: &PaletteHome, memo: &LayerMemo) -> Result<(), String> {
    let path = layer_memo_path(home);
    if !memo.has_changes() {
        return remove_file(&path);
    }
    let text = serde_json::to_string_pretty(memo).map_err(|e| e.to_string())? + "\n";
    write_text(&path, &text)
}

/// Result of applying the layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayerOutcome {
    pub env_added: bool,
    pub qt_layer: bool,
}

/// The scheme file and its text.
fn find_scheme(ctx: &SchemeCtx, id: &str) -> Option<(PathBuf, Vec<u8>)> {
    ctx.scheme_candidates(id).into_iter().find_map(|p| std::fs::read(&p).ok().map(|b| (p, b)))
}

fn scheme_not_found_msg(id: &str) -> String {
    // TRANSLATORS: %1 is the name of a color scheme.
    trf("Couldn't find the color scheme “%1”.", &[&id])
}

/// Plan of pressing «Apply» for a scheme with Rexon's layers (outside Plasma) — steps of
/// the shared plan ([`crate::install_plan`], step 5a of merging the installers).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemePress {
    /// In order: `ForceOk`, the «what existed» memo (before any layer write),
    /// layer writes (identical content is not rewritten), removing our palettes of
    /// previous schemes, the «palette changed» notification. Each write is under
    /// the shared rollback: a failure mid-press restores everything, including the memo.
    pub steps: Vec<crate::themes::ApplyStep>,
    pub outcome: LayerOutcome,
    /// The Qt palette goes into the native `lxqt.conf`.
    pub lxqt: bool,
}

impl SchemePress {
    /// Success text for the person.
    pub fn applied_msg(&self) -> String {
        layer_applied_msg(&self.outcome, self.lxqt)
    }
}

/// Build the press plan with Rexon's layers: everything is read and decided here, not
/// a single write (probes are only `io.have` and reading the environment's icons).
/// `Err` — a refusal before the first step (no scheme).
pub fn scheme_layer_press(
    io: &dyn crate::animations::LookIo,
    ctx: &SchemeCtx,
    lxqt: bool,
    id: &str,
) -> Result<SchemePress, String> {
    use crate::themes::{ApplyStep, Cmd};
    let Some((_, bytes)) = find_scheme(ctx, id) else {
        return Err(scheme_not_found_msg(id));
    };
    let scheme_text = String::from_utf8_lossy(&bytes).into_owned();
    let mut memo = read_layer_memo(&ctx.home);
    let mut facts = LayerFacts::default();
    if !lxqt {
        for tool in crate::qt_settings::QTCT_TOOLS {
            let conf = ctx.home.config().join(tool).join(format!("{tool}.conf"));
            if io.have(tool) || conf.exists() {
                facts.qt_tools.push(tool.to_string());
            }
        }
        if !facts.qt_tools.is_empty() {
            facts.icon_theme = live_icon_theme(io, ctx);
        }
    }
    let read = |p: &Path| read_optional(p).ok().flatten();
    let own_paths: Vec<String> = memo.files.iter().filter(|f| f.kind == "own").map(|f| f.path.clone()).collect();
    let ours = |p: &Path| own_paths.iter().any(|o| Path::new(o) == p);
    let plan = plan_scheme_layer(ctx, lxqt, id, &scheme_text, &scheme_hash(&bytes), &facts, &read, &ours);

    // Memo: what existed before Rexon's first edit.
    for w in &plan.writes {
        let current = read(&w.path);
        let p = w.path.to_string_lossy().to_string();
        let idx = match memo.files.iter().position(|f| f.path == p) {
            Some(i) => i,
            None => {
                memo.files.push(FileMemo {
                    path: p.clone(),
                    kind: w.kind.to_string(),
                    original: current.clone(),
                    written: String::new(),
                    keys: Vec::new(),
                    created_tables: Vec::new(),
                });
                memo.files.len() - 1
            }
        };
        let fm = &mut memo.files[idx];
        let base = current.unwrap_or_default();
        for (g, k) in &w.keys {
            if !fm.keys.iter().any(|m| m.table == *g && m.key == *k) {
                let prev = if w.kind == "env" { None } else { ini_get(&base, g, k) };
                fm.keys.push(KeyMemo { table: g.clone(), key: k.clone(), prev });
            }
        }
        fm.written = w.text.clone();
    }
    // Our palette files of previous schemes — no longer needed.
    let new_paths: Vec<String> = plan.writes.iter().map(|w| w.path.to_string_lossy().to_string()).collect();
    let stale: Vec<FileMemo> = memo
        .files
        .iter()
        .filter(|f| f.kind == "own" && !new_paths.contains(&f.path))
        .cloned()
        .collect();
    memo.files.retain(|f| !(f.kind == "own" && !new_paths.contains(&f.path)));
    memo.scheme = Some(id.to_string());

    let write = |path: &Path, content: String| {
        let dir = path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        ApplyStep::WriteSetsOk { dir, path: path.to_string_lossy().into_owned(), content }
    };
    // There may be nothing to write (everything is already so) — the press still succeeded.
    let mut steps = vec![ApplyStep::ForceOk];
    let memo_path = layer_memo_path(&ctx.home);
    if memo.has_changes() {
        let text = serde_json::to_string_pretty(&memo).map_err(|e| e.to_string())? + "\n";
        if read(&memo_path).as_deref() != Some(text.as_str()) {
            steps.push(write(&memo_path, text));
        }
    } else if memo_path.exists() {
        steps.push(ApplyStep::RemoveIfOk { path: memo_path.to_string_lossy().into_owned() });
    }
    for w in &plan.writes {
        if read(&w.path).as_deref() != Some(w.text.as_str()) {
            steps.push(write(&w.path, w.text.clone()));
        }
    }
    for f in stale {
        let path = Path::new(&f.path);
        if read(path).as_deref() == Some(f.written.as_str()) {
            steps.push(ApplyStep::RemoveIfOk { path: f.path.clone() });
        }
    }
    // To programs with the KDE platform theme — «palette changed» (as
    // Plasma sends). No bus — no problem.
    let notify = ["dbus-send", "--session", "--type=signal", "/KGlobalSettings", "org.kde.KGlobalSettings.notifyChange", "int32:0", "int32:0"];
    steps.push(ApplyStep::BestEffort(Cmd { argv: notify.iter().map(|a| a.to_string()).collect(), timeout: None }));
    Ok(SchemePress { steps, outcome: LayerOutcome { env_added: plan.env_added, qt_layer: plan.qt_layer }, lxqt })
}

/// Apply a scheme with Rexon's layers (outside Plasma): the [`scheme_layer_press`] plan
/// is executed by the shared executor, commands go through `io`. The memo comes BEFORE writing;
/// if a write fails, everything the press wrote (and the memo) is restored.
pub fn apply_scheme_layer(
    io: &dyn crate::animations::LookIo,
    ctx: &SchemeCtx,
    lxqt: bool,
    id: &str,
) -> Result<LayerOutcome, String> {
    use crate::install_plan::{exec, Ctx, Failure, Step};
    let press = scheme_layer_press(io, ctx, lxqt, id)?;
    let steps: Vec<Step> = press.steps.into_iter().map(Step::from).collect();
    let run = |argv: &[String], _: Option<u64>| {
        let (ok, out) = io.run(argv);
        crate::utils::RunReport { returncode: Some(if ok { 0 } else { 1 }), stdout: out.into_bytes(), ..Default::default() }
    };
    let have = |tool: &str| io.have(tool);
    let spawn = |argv: &[String]| {
        let _ = io.spawn(argv);
    };
    let ectx = Ctx { install: true, steamos: false, privileged: None, run: &run, have: &have, spawn: &spawn, wayland: false, sync_env: &crate::install_plan::no_sync_env };
    match exec(&steps, &mut Vec::new(), &ectx) {
        Ok(_) => Ok(press.outcome),
        Err(Failure::Write { path, err }) => Err(apply_failed_msg(Path::new(&path), &err)),
        Err(f) => Err(crate::install_plan::failure_text(&f)),
    }
}

fn apply_failed_msg(path: &Path, err: &str) -> String {
    // TRANSLATORS: %1 is a file path, %2 the technical reason.
    trf("Couldn't apply the scheme: %1 could not be written (%2).", &[&path.to_string_lossy(), &err])
}

/// The environment's current icon theme — the same keys Rexon writes it into.
fn live_icon_theme(io: &dyn crate::animations::LookIo, ctx: &SchemeCtx) -> String {
    use crate::themes::IconThemeSource as S;
    let xdg = (!ctx.home.xdg_config_home.is_empty()).then_some(ctx.home.xdg_config_home.as_str());
    match crate::themes::current_icon_theme_source(&ctx.de_id, &ctx.home.home, xdg) {
        S::Cmd(c) => {
            if c.argv.is_empty() || !io.have(&c.argv[0]) {
                return String::new();
            }
            let (ok, out) = io.run(&c.argv);
            if ok {
                crate::themes::parse_icon_theme_output(&out)
            } else {
                String::new()
            }
        }
        S::GtkIni(path) => crate::gtk_sanitizer::read_ini_settings(io.read(&path).as_deref())
            .get("gtk-icon-theme-name")
            .cloned()
            .unwrap_or_default(),
    }
}

/// In Plasma remove our `[UiSettings] ColorScheme` (otherwise KDE programs
/// would hold on to the scheme and not follow System Settings) and forget about `kdeglobals`:
/// Plasma itself sets the colors in it now, and «Return as It Was» must not
/// override its choice.
pub fn release_kdeglobals_to_plasma(home: &PaletteHome) -> Result<(), String> {
    let mut memo = read_layer_memo(home);
    let path = home.kdeglobals();
    let Some(fm) = memo.file(&path).cloned() else { return Ok(()) };
    let ours = ini_get(&fm.written, "UiSettings", "ColorScheme");
    if let Some(current) = read_optional(&path)? {
        if ours.is_some() && ini_get(&current, "UiSettings", "ColorScheme") == ours {
            let prev = fm.keys.iter().find(|k| k.table == "UiSettings" && k.key == "ColorScheme").and_then(|k| k.prev.clone());
            let text = ini_apply(Some(&current), &[IniEdit { group: "UiSettings".into(), key: "ColorScheme".into(), value: prev }]);
            write_text(&path, &text)?;
        }
    }
    memo.files.retain(|f| f.path != fm.path);
    save_layer_memo(home, &memo)
}

/// Result text of applying the layer.
pub fn layer_applied_msg(out: &LayerOutcome, lxqt: bool) -> String {
    let mut parts = vec![if out.qt_layer {
        tr("Color scheme applied to KDE and Qt apps. Apps that are already open pick it up after a restart.")
    } else {
        tr("Color scheme applied to KDE apps (Dolphin, Kate, Okular…). Other Qt apps won't get it: qt6ct isn't installed. \
            Apps that are already open pick it up after a restart.")
    }];
    if out.env_added {
        parts.push(tr("Other Qt apps switch to it after your next login."));
    }
    if !lxqt {
        parts.push(tr("GTK apps keep the colors of their GTK theme."));
    }
    parts.join(" ")
}

/// Apply a scheme in this environment: Plasma — with its own tool, outside Plasma
/// — with Rexon's layers. `(succeeded, text)`.
pub fn apply_scheme(io: &dyn crate::animations::LookIo, ctx: &SchemeCtx, id: &str) -> (bool, String, LayerOutcome) {
    match scheme_way(&ctx.de_id) {
        SchemeWay::Plasma => {
            let (ok, _) = io.run(&["plasma-apply-colorscheme".to_string(), id.to_string()]);
            if !ok {
                return (false, tr("Couldn't apply the scheme"), LayerOutcome::default());
            }
            let _ = release_kdeglobals_to_plasma(&ctx.home);
            (true, tr("Color scheme applied"), LayerOutcome::default())
        }
        SchemeWay::Unavailable => (false, tr("Couldn't apply the scheme"), LayerOutcome::default()),
        SchemeWay::Layer { lxqt } => match apply_scheme_layer(io, ctx, lxqt, id) {
            Ok(out) => (true, layer_applied_msg(&out, lxqt), out),
            Err(e) => (false, e, LayerOutcome::default()),
        },
    }
}

/// [`apply_scheme`] on the live machine: after editing the session variables they
/// are also passed to compositors that do not read `environment.d`.
pub fn apply_scheme_live(de_id: &str, id: &str) -> (bool, String) {
    let ctx = SchemeCtx::live(de_id);
    let (ok, msg, out) = apply_scheme(&crate::animations::SystemIo, &ctx, id);
    if out.env_added {
        let _ = crate::compositor_autostart::sync_session_env_live();
    }
    (ok, msg)
}

/// Return the files the scheme layer edited to their pre-Rexon state.
pub fn revert_scheme_layer(home: &PaletteHome) -> Result<String, String> {
    let memo = read_layer_memo(home);
    if !memo.has_changes() {
        return Ok(tr("Nothing to return: Rexon has not changed the colors outside Plasma."));
    }
    let mut skipped = Vec::new();
    for fm in memo.files.iter().rev() {
        let path = Path::new(&fm.path);
        let Some(current) = read_optional(path)? else { continue };
        if current == fm.written {
            match &fm.original {
                Some(orig) => write_text(path, orig)?,
                None => remove_file(path)?,
            }
            continue;
        }
        match fm.kind.as_str() {
            "ini" => {
                let edits: Vec<IniEdit> = fm
                    .keys
                    .iter()
                    .map(|k| IniEdit { group: k.table.clone(), key: k.key.clone(), value: k.prev.clone() })
                    .collect();
                let text = ini_apply(Some(&current), &edits);
                if text != current {
                    write_text(path, &text)?;
                }
            }
            // Our file was rewritten (the qt6ct palette — in qt6ct itself, the variables
            // file — by applying Kvantum) — it is no longer ours.
            _ => skipped.push(fm.path.clone()),
        }
    }
    save_layer_memo(home, &LayerMemo::default())?;
    Ok(if skipped.is_empty() {
        tr("The colors are back as they were before Rexon.")
    } else {
        trf(
            // TRANSLATORS: %1 is a list of files.
            "Mostly returned. These files were changed by someone else since, and Rexon left them as they are: %1",
            &[&skipped.join(", ")],
        )
    })
}
