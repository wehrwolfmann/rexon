//! Палитра в обе стороны между Rexon и чужим шеллом (Noctalia,
//! DankMaterialShell).
//!
//! ЗАЧЕМ. Оба шелла красят свою панель сами и, что важнее, сами ставят
//! приложениям тёмный или светлый режим: Noctalia — БЕЗУСЛОВНО, при запуске и
//! после каждой смены темы (`gsettings … color-scheme prefer-dark|prefer-light`),
//! DMS — при включённом `syncModeWithPortal`, а он включён по умолчанию.
//! Применённая в Rexon схема с другим режимом продержится до первой смены
//! темы в шелле. Устраивать гонку записей бессмысленно, поэтому человеку
//! даётся выбор «кто ведёт палитру», а две стороны подгоняются друг к другу:
//!
//!   * **Из Rexon в шелл.** Палитра текущей цветовой схемы (цвета, которые
//!     `plasma-apply-colorscheme` кладёт в `kdeglobals`) уходит шеллу его же
//!     штатным путём, и РЕЖИМ шелла выставляется таким же, как у схемы. Тогда
//!     шелл пишет в `color-scheme` то же самое, что подразумевает схема, —
//!     гонки нет, потому что спорить не о чем.
//!       - Noctalia: свой файл `palettes/Rexon.json` (формат «палитры» v5) и в
//!         файле состояния `settings.toml` ключи `[theme] source = "custom"`,
//!         `custom_palette = "Rexon"`, `mode`. Файл состояния грузится
//!         последним и перебивает `~/.config/noctalia/*.toml` — туда же пишут
//!         GUI шелла и его `noctalia msg color-scheme-set / theme-mode-set`.
//!       - DMS: свой файл темы `rexon-palette.json` (формат «своей темы» DMS:
//!         `dark`/`light`) и в `settings.json` ключи `currentThemeName =
//!         "custom"`, `currentThemeCategory = "custom"`, `customThemeFile`;
//!         режим — `isLightMode` в `session.json`. Оба файла шелл
//!         перечитывает сам.
//!   * **Из шелла в Rexon.** Цвета шелла становятся цветовой схемой KDE
//!     (`Rexon — цвета оболочки`) и применяются тем же путём, что любая
//!     схема из раздела «Цветовые схемы».
//!       - DMS после каждой сборки темы сохраняет полную палитру Material
//!         в `~/.cache/DankMaterialShell/dms-colors.json` (нужен `matugen`);
//!         при своей теме — читается её файл.
//!       - Noctalia своей палитры наружу сама не кладёт (встроенные палитры
//!         живут в бинаре, палитра из обоев — в памяти). Отдаёт она её только
//!         через шаблоны, поэтому Rexon с согласия человека подключает ОДИН
//!         свой шаблон `[theme.templates.user.rexon_palette]`: шелл заполняет
//!         его при каждой смене палитры, и файл всегда свежий. При своей
//!         палитре (`source = "custom"`) читается её файл напрямую.
//!
//! Правило записи то же, что у обоев шелла (`desktop_shell`): правятся ТОЛЬКО
//! перечисленные ключи, остальной файл остаётся байт в байт; результат
//! проверяется разбором (иначе отказ, а не порча); перед первой правкой рядом
//! кладётся резервная копия `*.rexon-palette-backup`; запись — подменой имени
//! (`utils::atomic_write`). Что было до Rexon, запоминается в
//! `$XDG_STATE_HOME/rexon/shell-palette.json` ДО записи в файл шелла.
//! «Вернуть как было»: если файл с тех пор никто не трогал — возвращается
//! прежнее содержимое целиком, бит в бит; если шелл успел его переписать —
//! возвращаются только наши ключи, чужие правки остаются.
//!
//! ⚠️ Граница: ни один шелл на машине разработки не установлен. Форматы и
//! ключи сверены по исходникам noctalia v5.2.0 и DMS v1.6.2 (разведка
//! 01.10.2026), поведение — подставными файлами (`tests/shell_palette.rs`).
//! Что шелл действительно перечитывает файлы на лету и что Noctalia
//! принимает шаблон из файла состояния, живьём НЕ проверено.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::i18n::{tr, trf};
use crate::system::DesktopShell;

// ============================================================================
// Цвет
// ============================================================================

/// Цвет без прозрачности.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Разбор `#rgb`, `#rrggbb`, `#rrggbbaa` (прозрачность отбрасывается) и
    /// KDE-вида `r,g,b` / `r,g,b,a`.
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

    /// `r,g,b` — так цвета записаны в схемах KDE.
    pub fn kde(self) -> String {
        format!("{},{},{}", self.0, self.1, self.2)
    }

    /// Относительная яркость по WCAG (0 — чёрный, 1 — белый).
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

    /// Смесь: `t = 0` — этот цвет, `t = 1` — `other`.
    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        let m = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round().clamp(0.0, 255.0) as u8;
        Rgb(m(self.0, other.0), m(self.1, other.1), m(self.2, other.2))
    }

    /// Читаемый текст поверх этого цвета: чёрный или белый, у кого контраст
    /// больше.
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
// Палитра
// ============================================================================

/// Палитра ролями — те же 16 ролей, что у Noctalia (`mPrimary` …
/// `mOnHover`); DMS и схема KDE раскладываются на них без потерь смысла.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Тёмная палитра (фон темнее текста).
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
    /// Роли парами «имя Noctalia (`mPrimary`) → цвет» в порядке её
    /// документации.
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

    /// Собрать по ролям с запасными значениями для тех, что источник не
    /// дал. `get` отвечает по имени Material (`on_primary`, `surface` …).
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
// Схема KDE ⇄ палитра
// ============================================================================

/// Группы INI KDE: `[Colors:Window]` → ключ → значение. Повтор ключа —
/// последний побеждает, как у KConfig.
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

/// Палитра по тексту схемы KDE (`*.colors`) или `kdeglobals` — туда
/// `plasma-apply-colorscheme` кладёт те же группы `[Colors:*]`.
///
/// `None` — в тексте нет цветов окна (схема не применялась ни разу).
pub fn palette_from_kde(text: &str) -> Option<Palette> {
    let g = kde_groups(text);
    let c = |group: &str, key: &str| g.get(group).and_then(|m| m.get(key)).and_then(|v| Rgb::parse(v));
    let surface = c("Colors:Window", "BackgroundNormal")?;
    let on_surface = c("Colors:Window", "ForegroundNormal")?;
    // Акцент: `AccentColor` (Plasma 6, «свой цвет акцента») главнее
    // выделения — выделение схема могла оставить своим.
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

/// Подпись схемы, которую Rexon собирает из цветов шелла.
pub fn shell_scheme_display_name() -> String {
    // TRANSLATORS: name of the KDE color scheme Rexon builds from the desktop shell's colors (shown in KDE's color settings).
    tr("Rexon — Shell Colors")
}

/// Текст схемы KDE по палитре. `id` — имя файла без `.colors` (оно же
/// `ColorScheme`), `name` — подпись в списке схем.
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
    // Выделение: текст поверх акцента, «неактивный» — тот же текст.
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
// Палитры шеллов ⇄ палитра
// ============================================================================

/// Палитра Material, как её кладёт DMS (`dms-colors.json`) и наш шаблон
/// Noctalia: `{"mode": "dark", "colors": {"dark": {...}, "light": {...}}}`
/// с именами `primary`, `on_primary`, `surface_container_high` ….
///
/// `dark_hint` — режим, в котором шелл сейчас (у DMS он хранится отдельно,
/// в `session.json`); `None` — по полю `mode`, без него — тёмный.
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

/// Палитра Noctalia (`palettes/*.json`: `dark`/`light` с `mPrimary` …).
/// Нет нужного варианта — берётся другой, как делает сам шелл.
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

/// Своя тема DMS (`customThemeFile`): либо `dark`/`light`, либо плоский
/// набор. Имена — DMS (`primaryText`, `surfaceText` …).
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

/// Файл палитры Noctalia. Оба варианта — одна и та же палитра: режим
/// шеллу выставляется отдельно, и при любом из них он должен показать
/// именно её.
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

/// Файл своей темы DMS (те же ключи, что у его встроенных тем).
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

/// Роли, которые шаблон Noctalia отдаёт наружу (имена Material).
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

/// Шаблон Noctalia, который пишет палитру в том же виде, что
/// `dms-colors.json` у DMS, — читатель у обоих один
/// ([`palette_from_material`]). Синтаксис — `{{ colors.<имя>.<режим>.hex }}`
/// и `{{ mode }}` (документация Noctalia, «Template Reference»).
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
// Где что лежит
// ============================================================================

/// Имя нашей палитры у Noctalia (`palettes/Rexon.json`).
pub const NOCTALIA_PALETTE_NAME: &str = "Rexon";
/// Имя нашего шаблона Noctalia.
pub const NOCTALIA_BRIDGE_ID: &str = "rexon_palette";
/// Две схемы KDE с цветами шелла. Две — потому что
/// `plasma-apply-colorscheme` ничего не делает, если просят ту же схему,
/// что уже стоит: новые цвета под прежним именем не применились бы.
pub const SHELL_SCHEME_IDS: [&str; 2] = ["RexonShell", "RexonShellAlt"];

/// Домашние корни (пустая строка — переменная не задана).
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

    /// Каталог конфигурации Noctalia (`$NOCTALIA_CONFIG_HOME/noctalia`, иначе
    /// XDG) — все `*.toml` в нём и `palettes/`.
    pub fn noctalia_config_dir(&self) -> PathBuf {
        if self.noctalia_config_home.is_empty() {
            self.config().join("noctalia")
        } else {
            Path::new(&self.noctalia_config_home).join("noctalia")
        }
    }

    /// Файл состояния Noctalia — слой, перебивающий конфигурацию.
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

    /// Памятка Rexon: что было до нас и кто ведёт палитру.
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
    /// `$XDG_STATE_HOME` либо `~/.local/state`.
    pub fn state_dir(&self) -> PathBuf {
        self.state()
    }
    pub fn color_scheme_file(&self, id: &str) -> PathBuf {
        self.data().join("color-schemes").join(format!("{id}.colors"))
    }
}

/// Имя резервной копии рядом с файлом шелла. Своё, не то же, что у обоев
/// шелла (`*.rexon-backup`): копии двух правок одного файла не должны
/// затирать друг друга.
pub fn backup_path(file: &Path) -> PathBuf {
    let mut name = file.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".rexon-palette-backup");
    file.with_file_name(name)
}

// ============================================================================
// Точечная правка TOML (строками) и JSON (членами верхнего уровня)
// ============================================================================

/// Почему правка не сделана.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// Файл не разбирается — трогать его нельзя.
    Unparsable(String),
    /// После правки разбор показал не то, что задумано (ключ записан
    /// иначе, чем мы умеем, — точечными ключами, встроенной таблицей …).
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

/// Имя таблицы из строки-заголовка `[a.b]` (пробелы вокруг точек и
/// внешние кавычки ключей убраны). `[[…]]` и не-заголовки — `None`.
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

/// Ключ строки вида `key = …` (без пути с точками).
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

/// Диапазон строк таблицы `table`: (строка заголовка, конец исключая).
/// Пустое имя — корень (до первого заголовка).
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

/// Строка ключа `key` в таблице `table` как есть (без перевода строки).
pub fn toml_key_line(text: &str, table: &str, key: &str) -> Option<String> {
    let lines = lines_keep_ends(text);
    let (head, end) = toml_table_span(&lines, table)?;
    let from = head.map(|h| h + 1).unwrap_or(0);
    lines[from..end]
        .iter()
        .find(|l| toml_line_key(l) == Some(key))
        .map(|l| l.trim_end_matches(['\n', '\r']).to_string())
}

/// Поставить (`Some(строка)`) или убрать (`None`) строку ключа. Нет таблицы —
/// дописывается в конец файла. Только текст; проверка смысла — у
/// вызывающего ([`toml_apply`]).
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
            // После последней непустой строки таблицы — пустые строки,
            // отделяющие следующую таблицу, остаются на месте.
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

/// Убрать заголовок таблицы, если в ней не осталось ни одной строки,
/// кроме пустых, — вместе с пустой строкой перед ним, которую дописывает
/// [`toml_set_line`].
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

/// Значение по пути `a.b.c` в разобранной таблице.
fn toml_lookup<'a>(root: &'a toml::Table, table: &str, key: &str) -> Option<&'a toml::Value> {
    let mut cur = root;
    if !table.is_empty() {
        for part in table.split('.') {
            cur = cur.get(part)?.as_table()?;
        }
    }
    cur.get(key)
}

/// То же для правки ожидаемого.
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

/// Убрать пустые таблицы — у разбора «нет таблицы» и «пустая таблица»
/// отличаются, а по смыслу для шелла это одно.
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

/// Строка TOML со строковым значением.
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

/// Одна точечная правка: таблица, ключ, новая строка (`None` — убрать).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlEdit {
    pub table: String,
    pub key: String,
    pub line: Option<String>,
}

/// Применить правки к тексту и проверить разбором: новый файл обязан
/// отличаться от старого РОВНО этими ключами. `drop_tables` — таблицы,
/// которые надо убрать, если они опустели (их создавал Rexon).
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

/// Член объекта верхнего уровня: начало ключа и конец значения.
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

/// Сырое значение члена верхнего уровня (текст JSON как в файле).
pub fn json_member_raw(text: &str, key: &str) -> Option<String> {
    let (_, members, _) = json_members(text)?;
    members
        .iter()
        .rev()
        .find(|m| m.key == key)
        .map(|m| text[m.value_start..m.value_end].to_string())
}

/// Поставить (`Some(сырое значение)`) или убрать член верхнего уровня,
/// не трогая остальной текст. Нового члена — в конец объекта, с отступом
/// как у первого.
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
                // Съедаем «ключ … значение, » до начала следующего ключа.
                format!("{}{}", &text[..m.key_start], &text[members[i + 1].key_start..])
            } else {
                // Последний: съедаем от конца предыдущего значения.
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
// Памятка
// ============================================================================

/// Кто ведёт палитру.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lead {
    /// Схема, применённая в Rexon, сразу уходит шеллу вместе с режимом.
    Rexon,
    /// Палитру ведёт шелл: Rexon ему ничего не передаёт, а цвета шелла
    /// берёт по кнопке.
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

/// Прежнее значение одного нашего ключа.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyMemo {
    /// Таблица TOML (`theme`, `theme.templates.user.rexon_palette`); у JSON
    /// пусто.
    pub table: String,
    pub key: String,
    /// Строка ключа TOML либо сырое значение JSON до нас; `None` — ключа
    /// не было.
    pub prev: Option<String>,
}

/// Файл, который правил Rexon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMemo {
    pub path: String,
    /// `toml`, `json` или `own` (файл целиком наш: палитра, шаблон).
    pub kind: String,
    /// Содержимое до первой правки Rexon; `None` — файла не было.
    pub original: Option<String>,
    /// Что Rexon записал последним.
    pub written: String,
    /// Наши ключи (у `own` пусто).
    #[serde(default)]
    pub keys: Vec<KeyMemo>,
    /// Таблицы TOML, которые создал Rexon.
    #[serde(default)]
    pub created_tables: Vec<String>,
}

/// Что Rexon сделал в окружении, когда брал цвета шелла.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvMemo {
    /// Схема KDE до нас (`[General] ColorScheme` в `kdeglobals`).
    pub prev_scheme: Option<String>,
    /// Наши файлы схем.
    #[serde(default)]
    pub scheme_files: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memo {
    #[serde(default)]
    pub lead: Option<Lead>,
    /// Какому шеллу что правили (`as_str`); пусто — ничего.
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
    /// Есть что возвращать.
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

/// Памятка с диска (нет или битая — пустая).
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

/// Запомнить выбор «кто ведёт палитру».
pub fn set_lead(home: &PaletteHome, lead: Lead) -> Result<(), String> {
    let mut memo = read_memo(home);
    if memo.lead == Some(lead) {
        return Ok(());
    }
    memo.lead = Some(lead);
    save_memo(home, &memo)
}

// ============================================================================
// Запись в файлы шелла
// ============================================================================

/// Одна задуманная правка файла.
#[derive(Debug, Clone)]
enum FileChange {
    /// Файл целиком наш.
    Own(String),
    /// Ключи TOML.
    Toml(Vec<TomlEdit>),
    /// Члены JSON верхнего уровня: ключ → сырое значение.
    Json(Vec<(String, String)>),
}

/// Почему не вышло, словами для человека.
fn edit_refused(shell: DesktopShell, path: &Path, reason: &str) -> String {
    trf(
        // TRANSLATORS: %1 is the shell name, %2 a file path, %3 the technical reason. Nothing was changed.
        "Rexon left the settings of %1 alone: %2 is not laid out the way Rexon expects (%3). Nothing was changed.",
        &[&shell.label(), &path.to_string_lossy(), &reason],
    )
}

/// Применить правки: что было — в памятку ДО записи, рядом — резервная
/// копия. Возвращает, изменилось ли хоть что-то.
fn apply_changes(
    home: &PaletteHome,
    shell: DesktopShell,
    changes: &[(PathBuf, FileChange)],
) -> Result<bool, String> {
    let mut memo = read_memo(home);
    // Сначала всё посчитать — отказ любого файла не должен оставить
    // половину правок.
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
    // Памятка — первой.
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
// Режим шелла
// ============================================================================

/// Слои конфигурации Noctalia по порядку: `*.toml` каталога по алфавиту,
/// поверх — файл состояния. (`[include]` не раскрывается: ключи темы туда
/// почти не выносят, а ошибиться здесь стоит лишь лишней правкой режима.)
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

/// Значение `[theme] <key>` Noctalia после слияния слоёв.
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

/// Тёмный ли режим у шелла сейчас. `None` — не узнать (у Noctalia
/// `mode = "auto"`: режим по расписанию солнца).
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
// Из Rexon в шелл
// ============================================================================

/// Палитра текущей цветовой схемы окружения (`kdeglobals`).
pub fn current_rexon_palette(home: &PaletteHome) -> Option<Palette> {
    palette_from_kde(&std::fs::read_to_string(home.kdeglobals()).ok()?)
}

/// Схема KDE, стоящая сейчас (`[General] ColorScheme`).
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
            // Режим по умолчанию — тёмный: ключ ставим, только если он
            // разойдётся со схемой.
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

/// Передать палитру шеллу. `Ok(текст)` — итог для человека (в том числе
/// «менять нечего»), `Err(текст)` — отказ.
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

/// Нет схемы, чью палитру передавать.
pub fn no_rexon_palette_msg() -> String {
    tr("There is no current color scheme to pass on: kdeglobals has no window colors. Apply a color scheme in “Color Schemes” and try again.")
}

// ============================================================================
// Из шелла в Rexon
// ============================================================================

/// Почему цвета шелла не взяты.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullError {
    /// Noctalia: нужен наш шаблон (с согласия человека — [`connect_bridge`]).
    NeedsBridge,
    /// Шаблон подключён, но файл шелл ещё не заполнил.
    BridgePending,
    /// Отказ словами для человека.
    Refused(String),
}

/// Цвета шелла сейчас.
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

/// Наш шаблон Noctalia уже прописан.
pub fn bridge_connected(home: &PaletteHome) -> bool {
    std::fs::read_to_string(home.noctalia_state_file())
        .ok()
        .is_some_and(|t| toml_key_line(&t, &bridge_table(), "output_path").is_some())
}

fn bridge_table() -> String {
    format!("theme.templates.user.{NOCTALIA_BRIDGE_ID}")
}

/// Подключить наш шаблон Noctalia: шелл будет класть свою палитру в
/// [`PaletteHome::bridge_output_file`] при каждой смене.
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

/// Ответ на «Шаблон подключён, ждём шелл».
pub fn bridge_pending_msg(shell: DesktopShell) -> String {
    trf(
        // TRANSLATORS: %1 is the shell name; “Take Shell Colors” is the button name.
        "Rexon asked %1 to share its colors, but the shell has not written them yet. Press “Take Shell Colors” again in a few seconds.",
        &[&shell.label()],
    )
}

/// Схема KDE с цветами шелла: (имя, путь, текст). Имя чередуется между
/// двумя, если наша схема уже стоит (см. [`SHELL_SCHEME_IDS`]); `None` —
/// стоящая схема уже ровно эта, применять нечего.
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

/// Записать схему с цветами шелла и запомнить, какая стояла до нас
/// (только при первом заходе — второй заход не должен затереть «до нас»
/// нашей же схемой).
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

/// Итог «Взять цвета оболочки».
pub fn pulled_msg(shell: DesktopShell) -> String {
    trf(
        // TRANSLATORS: %1 is the shell name; %2 is the name of the color scheme.
        "The colors of %1 are applied as the color scheme “%2”.",
        &[&shell.label(), &shell_scheme_display_name()],
    )
}

/// Повторное «Взять цвета оболочки» без перемен у шелла.
pub fn pulled_same_msg(shell: DesktopShell) -> String {
    trf(
        // TRANSLATORS: %1 is the shell name.
        "The colors of %1 are already applied — nothing to change.",
        &[&shell.label()],
    )
}

// ============================================================================
// Вернуть как было
// ============================================================================

/// Что осталось сделать вызывающему после возврата файлов.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevertOutcome {
    /// Текст для человека.
    pub message: String,
    /// Схема KDE, которую надо применить обратно (стояла до Rexon).
    pub reapply_scheme: Option<String>,
    /// Наши файлы схем — убрать ПОСЛЕ того, как прежняя схема встанет
    /// ([`forget_env`]).
    pub drop_scheme_files: Vec<String>,
    /// Ключи, которые вернуть не удалось (файл переписан до
    /// неузнаваемости), — для честного текста.
    pub skipped: Vec<String>,
}

fn revert_file(fm: &FileMemo) -> Result<Option<String>, String> {
    let path = Path::new(&fm.path);
    let current = read_optional(path)?;
    // Никто не трогал после нас — бит в бит прежнее.
    if current.as_deref() == Some(fm.written.as_str()) {
        match &fm.original {
            Some(orig) => write_text(path, orig)?,
            None => remove_file(path)?,
        }
        let _ = remove_file(&backup_path(path));
        return Ok(None);
    }
    let Some(current) = current else {
        // Файла больше нет — шелл или человек убрал его сам.
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
        // Наш файл кто-то переписал — не наш больше, не трогаем.
        _ => Ok(Some(fm.path.clone())),
    }
}

/// Вернуть файлы шелла к тому, что было до Rexon. Схему окружения
/// вызывающий применяет сам (`reapply_scheme`), затем зовёт [`forget_env`].
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
    // В обратном порядке: последним правили — первым возвращаем.
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
        // Применять обратно нечего — свои схемы убираем сразу.
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

/// Прежняя схема снова стоит — убрать наши файлы схем и запись об
/// окружении.
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
// Что показать человеку
// ============================================================================

/// Полоса «Палитра оболочки» в «Оформлении».
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PaletteBanner {
    pub visible: bool,
    /// `as_str` шелла.
    pub shell: String,
    /// Предупреждение о споре за тёмный/светлый режим.
    pub text: String,
    /// `rexon`, `shell` или пусто (не выбрано).
    pub lead: String,
    pub lead_caption: String,
    pub lead_rexon_label: String,
    pub lead_shell_label: String,
    pub push_label: String,
    pub pull_label: String,
    pub revert_label: String,
    /// Есть что передавать (в `kdeglobals` есть цвета).
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

/// Полоса по готовым наблюдениям.
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

/// Что добавить к итогу применения цветовой схемы в Rexon при живом
/// шелле. Палитру ведёт Rexon — схема сразу уходит шеллу; ведёт шелл —
/// честно сказать, что режим останется за ним; не выбрано — подсказать
/// кнопку.
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
// Живой сеанс и ход действий
// ============================================================================

/// Живой шелл этого сеанса — общим сбором наблюдений модуля обоев шелла
/// (`desktop_shell::LiveFacts`): каталог сеанса, `/proc` своего
/// пользователя и своё окружение, без внешних процессов.
pub fn live_detect() -> Option<DesktopShell> {
    crate::desktop_shell::LiveFacts::collect().detect().map(|d| d.shell)
}

/// Полоса по живому сеансу.
pub fn live_banner() -> PaletteBanner {
    palette_banner(live_detect(), &PaletteHome::live())
}

/// Нет живого шелла — действовать не над кем.
fn no_shell_msg() -> String {
    tr("No running desktop shell (Noctalia, DankMaterialShell) was found in this session.")
}

/// «Передать палитру оболочке».
pub fn push_flow(shell: Option<DesktopShell>, home: &PaletteHome) -> (bool, String) {
    let Some(shell) = shell else { return (false, no_shell_msg()) };
    let Some(p) = current_rexon_palette(home) else { return (false, no_rexon_palette_msg()) };
    match push_palette(shell, &p, home) {
        Ok(m) => (true, m),
        Err(m) => (false, m),
    }
}

/// «Взять цвета оболочки». `nudge` просит шелл заполнить наш шаблон
/// (живьём — `noctalia msg templates-apply`), `wait_ms` — сколько ждать
/// файла после подключения шаблона; `apply` применяет схему KDE по имени
/// тем же путём, что раздел «Цветовые схемы», и отвечает `(ok, текст)`.
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

/// «Вернуть как было»: файлы шелла, затем прежняя схема окружения.
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

/// Попросить Noctalia перерисовать шаблоны сейчас, а не при следующей
/// смене палитры. Нет команды или шелл не ответил — не беда: шаблон всё
/// равно заполнится при следующей смене.
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
// Цветовая схема KDE вне Plasma
// ============================================================================
//
// ЗАЧЕМ. В Plasma схему применяет `plasma-apply-colorscheme`: он вливает
// цвета схемы в `~/.config/kdeglobals`, а палитру программам раздаёт
// платформенная тема Plasma. Вне Plasma (GNOME, Cinnamon, XFCE, MATE, Budgie,
// Pantheon, LXQt, COSMIC, тайловые) этой связки нет, и раздел «Цветовые
// схемы» раньше был только в KDE (правило «настройка одного окружения — во
// всех»). Здесь Rexon делает её сам — тремя слоями, каждый для своих программ:
//
//   * `kdeglobals` — те же группы и ключи, что пишет сама Plasma при
//     применении схемы (сверено с `kcms/colors/colorsapplicator.cpp`
//     `applyScheme` и `plasma-apply-colorscheme.cpp` из plasma-workspace):
//     `[Colors:View|Window|Button|Selection|Tooltip|Complementary|Header]`
//     (12 ключей каждая, `[Colors:Header]` — только если она есть в схеме,
//     подгруппа `[…][Inactive]` — если она есть в схеме), `[WM]` (6 цветов
//     заголовка), `[KDE] contrast / frameContrast`, `[ColorEffects:Inactive
//     |Disabled]`, `[General] ColorScheme / ColorSchemeHash` (SHA-1 файла
//     схемы — по нему startplasma узнаёт, что файл схемы менялся). Эти цвета
//     читают KDE-программы через `KColorScheme`.
//   * `[UiSettings] ColorScheme` в том же `kdeglobals` — без него вне Plasma
//     не обойтись. Программы KDE с меню «Цветовая схема» (Dolphin, Kate,
//     Okular…) создают `KColorSchemeManager`, и тот при платформенной теме не
//     от Plasma САМ ставит программе Breeze или Breeze Dark по светлому/
//     тёмному режиму — цвета `kdeglobals` ему не указ (kcolorscheme,
//     `kcolorschememanager.cpp` `init` / `automaticColorSchemeId`). Схему из
//     `[UiSettings] ColorScheme` он берёт первой, а файл настроек программы
//     каскадом читает и `kdeglobals` — так один ключ доходит до всех.
//     ⚠️ В Plasma этот ключ мешал бы: программы держались бы за схему и не
//     шли за сменой схемы в Параметрах. Поэтому, применяя схему в Plasma,
//     Rexon свой ключ снимает (если его с тех пор никто не менял).
//   * Палитра Qt для остальных Qt-программ:
//       - LXQt — родной ключ: `~/.config/lxqt/lxqt.conf [Palette]`
//         (`window_color`, `base_color`, … — `LXQtPlatformTheme::loadSettings`
//         в lxqt-qtplugin; файл тема платформы перечитывает сама);
//       - остальные — qt6ct (и qt5ct, если он есть): наш файл
//         `<tool>/colors/<схема>.conf` (`[ColorScheme] active_colors /
//         inactive_colors / disabled_colors` — цвета ролей `QPalette` по
//         порядку ролей, `#aarrggbb`; формат — `Qt6CT::loadColorScheme`) и в
//         `<tool>.conf [Appearance] custom_palette=true`,
//         `color_scheme_path=<путь>` (так его записывает страница
//         «Appearance» самого qt6ct). Если сеанс ещё не отдаёт Qt-программы
//         qt6ct, в наш файл переменных `environment.d/rexon-qt-style.conf`
//         (тот же, что у Kvantum и «Стиля приложений») добавляется
//         `QT_QPA_PLATFORMTHEME=qt6ct` — тогда палитра появится после
//         следующего входа. Значков qt6ct до этого не знал — кладём нынешние
//         значки окружения, иначе Qt-программы остались бы с `hicolor`.
//
// Роли палитры раскладываются из наборов цветов схемы так же, как это
// делает KDE для своих программ (Window → окно и текст окна, View → поля
// ввода и списки, Button, Selection → выделение, Tooltip, ссылки и
// замещающий текст — из View). Оттенки рамок (Light/Mid/Dark/Shadow) и вид
// неактивных/выключенных элементов посчитаны проще, чем у KDE: близко по
// смыслу, но не цифра в цифру.
//
// GTK-программы схему KDE не читают вовсе — у них свои цвета в теме GTK.
// Это сказано человеку в оговорке раздела (матрица `de_customization`).
//
// Откат — как у палитры шелла: что было до Rexon, запоминается в
// `$XDG_STATE_HOME/rexon/color-scheme-layer.json` ДО записи. «Вернуть как
// было»: файл с тех пор не трогали — прежнее содержимое бит в бит; трогали —
// только наши ключи возвращаются к прежним значениям, чужие правки остаются.
// Наши файлы палитры qt6ct удаляются; файл переменных возвращается, только
// если его с тех пор никто не переписал (его ведёт и применение Kvantum).
//
// ⚠️ Граница: живьём вне Plasma НЕ проверено — только подставными файлами
// (`tests/shell_palette.rs`); формат и ключи — по исходникам KDE, qt6ct и
// LXQt (см. выше).

/// Наборы цветов, которые Plasma вливает в `kdeglobals` (по порядку
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

/// Ключи набора цветов, которые переносит Plasma.
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

/// Цвета заголовка окна `[WM]`.
const SCHEME_WM_KEYS: &[&str] = &[
    "activeBackground",
    "activeForeground",
    "inactiveBackground",
    "inactiveForeground",
    "activeBlend",
    "inactiveBlend",
];

/// Группы эффектов неактивного и выключенного состояния.
const SCHEME_EFFECT_GROUPS: &[&str] = &["ColorEffects:Inactive", "ColorEffects:Disabled"];

/// Ключи эффектов.
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

/// Файл переменных сеанса Rexon для Qt — общий со стилем Qt.
const QT_ENV_REL: &str = "environment.d/rexon-qt-style.conf";

// ── Правка INI в формате KConfig/QSettings строками ────────────────────────
//
// Чужой файл (`kdeglobals`, `qt6ct.conf`, `lxqt.conf`) правится точечно:
// меняются только наши ключи, остальные строки остаются байт в байт.
// Вложенная группа KConfig `[Colors:Header][Inactive]` здесь называется
// `Colors:Header][Inactive` — как её видит [`kde_groups`].

/// Одна правка: `value = None` — убрать ключ.
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

/// Группа INI: имя (`None` — строки до первой группы) и её строки без
/// заголовка.
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

/// Значение ключа (последнее, как у KConfig). `None` — ключа нет.
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

/// Ключи группы по порядку (без повторов).
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

/// Применить правки по порядку. Новый ключ встаёт в конец своей группы
/// (перед пустыми строками), новая группа — в конец файла. Группа, из
/// которой правкой убран последний ключ, исчезает вместе с заголовком.
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
                        return false; // повтор ключа — лишний
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
                // Отбить новую группу пустой строкой от предыдущей.
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
                    // Группа была последней — пустая строка-разделитель перед
                    // ней больше не нужна.
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

// ── Цвета схемы ────────────────────────────────────────────────────────────

/// Значения по умолчанию для недостающего ключа набора (светлый Breeze —
/// так и KDE достраивает неполную схему).
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

/// Разобранная схема KDE: цвет набора с запасом (нет ключа — тот же ключ
/// окна, нет и его — светлый Breeze).
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
    /// Заголовок окна: набор Header, а без него — Window (так KDE и рисует
    /// схему без собственного заголовка).
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
    /// Доля «выцветания» выключенного текста (`[ColorEffects:Disabled]`,
    /// эффект контраста «к фону»; у KDE по умолчанию 0.65).
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

/// Цвет в форме KConfig (`r,g,b`).
fn kcolor(c: Rgb) -> String {
    c.kde()
}

/// Правки `kdeglobals`, которыми Plasma применяет схему (`applyScheme`), и
/// сверх них — `[UiSettings] ColorScheme` вне Plasma (`pin`).
///
/// Отличие от Plasma намеренное одно: эффекты (`[ColorEffects:*]`), которых
/// в схеме нет, Plasma записывает пустыми строками, а здесь ключ убирается —
/// по смыслу это одно и то же («по умолчанию»), но без пустых значений.
/// Акцентный цвет (`[General] AccentColor`) здесь не подмешивается — его
/// вносит [`crate::accent::scheme_accent_edits`] в [`plan_scheme_layer`].
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
                // Неактивный заголовок Plasma берёт с фона окна.
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

/// Роли `QPalette` по порядку (Qt 6: `WindowText` = 0 … `Accent` = 21).
/// Qt 5 и Qt 6 до 6.6 знают первые 21 роль и лишнюю последнюю пропускают;
/// qt6ct для 21 роли сам достраивает `Accent` из выделения.
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

/// Файл палитры qt6ct/qt5ct по схеме KDE (`[ColorScheme]`, цвета
/// `#aarrggbb`). Неактивное окно — те же цвета, что активное (так выглядят
/// и стандартные схемы KDE: эффект неактивного у них выключен).
pub fn qtct_palette_text(scheme_text: &str) -> String {
    let s = SchemeColors::new(scheme_text);
    let list = |roles: Vec<Rgb>| roles.iter().map(|c| format!("#ff{}", &c.hex()[1..])).collect::<Vec<_>>().join(", ");
    let active = list(qt_palette_roles(&s, false));
    let disabled = list(qt_palette_roles(&s, true));
    format!(
        "[ColorScheme]\nactive_colors={active}\ndisabled_colors={disabled}\ninactive_colors={active}\n"
    )
}

/// Правки `lxqt.conf [Palette]` — родной палитры LXQt.
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

/// SHA-1 файла схемы шестнадцатеричной строкой — `[General] ColorSchemeHash`.
pub fn scheme_hash(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    Sha1::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

// ── Окружение и план ───────────────────────────────────────────────────────

/// Как в этом окружении применяется цветовая схема.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemeWay {
    /// Plasma: `plasma-apply-colorscheme`.
    Plasma,
    /// Вне Plasma — слои Rexon. `lxqt` — палитра Qt в родном `lxqt.conf`,
    /// иначе — qt6ct.
    Layer { lxqt: bool },
    /// Deepin: программы рисуются стилем Chameleon, палитру им даёт сам
    /// Deepin — раздел скрыт с объяснением (`de_customization`).
    Unavailable,
}

/// Путь применения схемы по идентификатору окружения
/// (`system::detect_current_de`). Неизвестное окружение — слои Rexon:
/// они не зависят от оболочки.
pub fn scheme_way(de_id: &str) -> SchemeWay {
    use crate::de_customization::{de_from_id, De};
    match de_from_id(de_id) {
        Some(De::Kde) => SchemeWay::Plasma,
        Some(De::Deepin) => SchemeWay::Unavailable,
        Some(De::Lxqt) => SchemeWay::Layer { lxqt: true },
        _ => SchemeWay::Layer { lxqt: false },
    }
}

/// Где искать и куда писать.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemeCtx {
    pub de_id: String,
    pub home: PaletteHome,
    /// `$XDG_DATA_DIRS` по порядку (пусто — `/usr/local/share`, `/usr/share`).
    pub data_dirs: Vec<String>,
    /// `$QT_QPA_PLATFORMTHEME` этого сеанса.
    pub session_platform_theme: String,
}

impl SchemeCtx {
    pub fn live(de_id: &str) -> SchemeCtx {
        let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
        SchemeCtx {
            de_id: de_id.to_string(),
            home: PaletteHome::live(),
            data_dirs: dirs.split(':').filter(|d| !d.is_empty()).map(str::to_string).collect(),
            session_platform_theme: std::env::var("QT_QPA_PLATFORMTHEME").unwrap_or_default(),
        }
    }

    /// Файл схемы по имени: сначала свои схемы, потом системные.
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

    fn qt_env_file(&self) -> PathBuf {
        self.home.config().join(QT_ENV_REL)
    }

    /// Памятка слоя.
    pub fn layer_memo_file(&self) -> PathBuf {
        layer_memo_path(&self.home)
    }
}

/// Файл памятки слоя цветов.
pub fn layer_memo_path(home: &PaletteHome) -> PathBuf {
    home.state().join("rexon/color-scheme-layer.json")
}

/// Что известно о машине до плана (собирает исполнитель).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayerFacts {
    /// Настройщики Qt, которыми человек может пользоваться: программа есть
    /// в системе или её конфиг уже заведён (`qt6ct`, `qt5ct`).
    pub qt_tools: Vec<String>,
    /// Тема значков окружения сейчас (пусто — неизвестна).
    pub icon_theme: String,
}

/// Одна запись по плану.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerWrite {
    pub path: PathBuf,
    /// `ini` — чужой файл, правятся наши ключи; `own` — файл целиком наш;
    /// `env` — наш файл переменных, общий со стилем Qt.
    pub kind: &'static str,
    pub text: String,
    /// Ключи, которые правка трогает (группа, ключ) — для отката.
    pub keys: Vec<(String, String)>,
}

/// План слоя схемы вне Plasma.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayerPlan {
    pub writes: Vec<LayerWrite>,
    /// В файл переменных добавлен `QT_QPA_PLATFORMTHEME` — палитра qt6ct
    /// появится после следующего входа.
    pub env_added: bool,
    /// Палитра для Qt-программ (qt6ct/qt5ct или LXQt) записана.
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

/// Имя нашего файла палитры в `<tool>/colors`: имя схемы, а если там
/// лежит чужой файл с таким именем — с пометкой.
fn qtct_palette_path(dir: &Path, id: &str, ours: &dyn Fn(&Path) -> bool, exists: &dyn Fn(&Path) -> bool) -> PathBuf {
    let plain = dir.join(format!("{id}.conf"));
    if !exists(&plain) || ours(&plain) {
        return plain;
    }
    dir.join(format!("{id} (Rexon).conf"))
}

/// План слоя схемы вне Plasma. `read` — текст файла (`None` — нет), `ours`
/// — записан ли файл Rexon (по памятке).
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

    // 1. kdeglobals — цвета для KDE-программ и `[UiSettings]` для их
    //    `KColorSchemeManager`.
    let kdeglobals = ctx.home.kdeglobals();
    let current = read(&kdeglobals).unwrap_or_default();
    let mut kg_edits = scheme_kdeglobals_edits(&current, scheme_text, id, hash, true);
    // Свой цвет акцента (`[General] AccentColor`) — поверх схемы, как его
    // вносит `applyScheme` KDE; палитра Qt строится уже из подкрашенной схемы.
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

    // 2. Палитра Qt.
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
        // Значков у qt6ct ещё нет — нынешние значки окружения (иначе
        // Qt-программы под ним останутся с `hicolor`).
        let conf_text = read(&conf).unwrap_or_default();
        if !facts.icon_theme.is_empty() && ini_get(&conf_text, "Appearance", "icon_theme").is_none_or(|v| v.is_empty()) {
            edits.push(IniEdit::set("Appearance", "icon_theme", facts.icon_theme.clone()));
        }
        plan.writes.push(LayerWrite { path: file, kind: "own", text: palette.clone(), keys: Vec::new() });
        plan.writes.push(ini(conf, edits));
        plan.qt_layer = true;
    }

    // 3. Переменная сеанса: Qt-программы должны брать тему платформы qt6ct.
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

// ── Памятка и исполнение ───────────────────────────────────────────────────

/// Что Rexon записал слоем схемы.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerMemo {
    #[serde(default)]
    pub files: Vec<FileMemo>,
    /// Последняя схема, применённая слоем.
    #[serde(default)]
    pub scheme: Option<String>,
}

impl LayerMemo {
    /// Есть что возвращать.
    pub fn has_changes(&self) -> bool {
        !self.files.is_empty()
    }
    fn file(&self, path: &Path) -> Option<&FileMemo> {
        let p = path.to_string_lossy();
        self.files.iter().find(|f| f.path == p)
    }
}

/// Памятка слоя с диска (нет или битая — пустая).
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

/// Итог применения слоя.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayerOutcome {
    pub env_added: bool,
    pub qt_layer: bool,
}

/// Файл схемы и его текст.
fn find_scheme(ctx: &SchemeCtx, id: &str) -> Option<(PathBuf, Vec<u8>)> {
    ctx.scheme_candidates(id).into_iter().find_map(|p| std::fs::read(&p).ok().map(|b| (p, b)))
}

fn scheme_not_found_msg(id: &str) -> String {
    // TRANSLATORS: %1 is the name of a color scheme.
    trf("Couldn't find the color scheme “%1”.", &[&id])
}

/// План нажатия «Применить» схемы слоями Rexon (вне Plasma) — шаги
/// общего плана ([`crate::install_plan`], шаг 5а слияния установщиков).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemePress {
    /// По порядку: `ForceOk`, памятка «что было» (до любой записи слоя),
    /// записи слоёв (одинаковое не переписывается), снятие наших палитр
    /// прежних схем, уведомление «палитра сменилась». Каждая запись — под
    /// общим откатом: сбой посреди нажатия возвращает всё, включая памятку.
    pub steps: Vec<crate::themes::ApplyStep>,
    pub outcome: LayerOutcome,
    /// Палитра Qt — в родном `lxqt.conf`.
    pub lxqt: bool,
}

impl SchemePress {
    /// Текст удачи для человека.
    pub fn applied_msg(&self) -> String {
        layer_applied_msg(&self.outcome, self.lxqt)
    }
}

/// Собрать план нажатия слоями Rexon: всё прочитано и решено здесь, ни
/// одной записи (пробы — только `io.have` и чтение значков окружения).
/// `Err` — отказ до первого шага (схемы нет).
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

    // Памятка: что было до первой правки Rexon.
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
    // Наши файлы палитры прежних схем — больше не нужны.
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
    // Записывать может оказаться нечего (всё уже так) — нажатие всё равно удалось.
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
    // Программам с темой платформы KDE — «палитра сменилась» (как шлёт
    // Plasma). Шины нет — не беда.
    let notify = ["dbus-send", "--session", "--type=signal", "/KGlobalSettings", "org.kde.KGlobalSettings.notifyChange", "int32:0", "int32:0"];
    steps.push(ApplyStep::BestEffort(Cmd { argv: notify.iter().map(|a| a.to_string()).collect(), timeout: None }));
    Ok(SchemePress { steps, outcome: LayerOutcome { env_added: plan.env_added, qt_layer: plan.qt_layer }, lxqt })
}

/// Применить схему слоями Rexon (вне Plasma): план [`scheme_layer_press`]
/// исполняет общий исполнитель, команды — через `io`. Памятка — ДО записи;
/// не удалась запись — всё, что нажатие записало (и памятка), возвращено.
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

/// Тема значков окружения сейчас — те же ключи, куда её пишет Rexon.
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

/// В Plasma снять наш `[UiSettings] ColorScheme` (иначе программы KDE
/// держались бы за схему и не шли за Параметрами) и забыть про `kdeglobals`:
/// цвета в нём теперь ставит сама Plasma, и «вернуть как было» не должно
/// перебить её выбор.
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

/// Текст итога применения слоя.
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

/// Применить схему в этом окружении: Plasma — её инструментом, вне Plasma
/// — слоями Rexon. `(получилось, текст)`.
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

/// [`apply_scheme`] на живой машине: после правки переменных сеанса они
/// передаются и композиторам, которые `environment.d` не читают.
pub fn apply_scheme_live(de_id: &str, id: &str) -> (bool, String) {
    let ctx = SchemeCtx::live(de_id);
    let (ok, msg, out) = apply_scheme(&crate::animations::SystemIo, &ctx, id);
    if out.env_added {
        let _ = crate::compositor_autostart::sync_session_env_live();
    }
    (ok, msg)
}

/// Вернуть файлы, которые правил слой схемы, к виду до Rexon.
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
            // Наш файл переписали (палитру qt6ct — в самом qt6ct, файл
            // переменных — применением Kvantum) — он уже не наш.
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
