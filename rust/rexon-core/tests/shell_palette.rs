//! Сверка палитры в обе стороны между Rexon и чужим шеллом
//! (`rexon_core::shell_palette`).
//!
//! ⛔ Ни Noctalia, ни DankMaterialShell на машине не установлены, и ставить
//! их запрещено. Поэтому сверка идёт на подставных домашних каталогах: в них
//! лежат настройки шеллов ровно того вида, что пишут сами шеллы (форматы
//! сверены по исходникам noctalia v5.2.0 и DMS v1.6.2). Проверяется:
//!   * запись трогает ТОЛЬКО наши ключи, остальной файл байт в байт;
//!   * повтор того же — без изменений (ни записи, ни новой резервной копии);
//!   * «Вернуть как было» возвращает файл бит в бит, а если шелл успел его
//!     переписать — только наши ключи;
//!   * файл, устроенный не так, как мы умеем править, не портится, а
//!     отказ говорится словами;
//!   * обратная сторона: цвета шелла читаются из его файлов и становятся
//!     схемой KDE, повтор не применяет схему заново.
//! Живьём шелл не перечитывал ни одного файла — граница сверки.

mod i18n_pin;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rexon_core::shell_palette::{self as sp, Lead, Palette, PaletteHome, PullError, Rgb};
use rexon_core::system::DesktopShell;

// ── Подставной дом ──────────────────────────────────────────────────────

fn fresh_home(tag: &str) -> (PathBuf, PaletteHome) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("shell-palette-{}-{tag}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let home = PaletteHome { home: root.to_string_lossy().to_string(), ..Default::default() };
    (root, home)
}

fn put(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Все файлы под корнем — снимок «как было» для сверки бит в бит.
fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.push((p.strip_prefix(root).unwrap().to_string_lossy().to_string(), fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// Снимок без памятки Rexon (её «вернуть» не убирает: в ней живёт выбор
/// «кто ведёт палитру»).
fn snapshot_without_memo(root: &Path) -> Vec<(String, Vec<u8>)> {
    snapshot(root).into_iter().filter(|(p, _)| !p.contains("rexon/shell-palette")).collect()
}

/// Схема Breeze Dark в том виде, как её кладёт `plasma-apply-colorscheme`
/// в `kdeglobals` (фрагмент).
const KDEGLOBALS_DARK: &str = "[ColorEffects:Disabled]\nColor=56,56,56\n\n\
[Colors:Button]\nBackgroundNormal=41,44,48\nForegroundNormal=252,252,252\n\n\
[Colors:Selection]\nBackgroundNormal=61,174,233\nForegroundNormal=252,252,252\n\n\
[Colors:View]\nBackgroundNormal=20,22,24\nDecorationFocus=61,174,233\nForegroundNegative=218,68,83\n\n\
[Colors:Window]\nBackgroundAlternate=41,44,48\nBackgroundNormal=32,35,38\nForegroundInactive=161,169,177\n\
ForegroundLink=29,153,243\nForegroundNegative=218,68,83\nForegroundNormal=252,252,252\nForegroundVisited=155,89,182\n\n\
[General]\nColorScheme=BreezeDark\nName=Breeze Dark\n\n[KDE]\nLookAndFeelPackage=org.kde.breezedark.desktop\n";

const KDEGLOBALS_LIGHT: &str = "[Colors:Selection]\nBackgroundNormal=61,174,233\nForegroundNormal=255,255,255\n\n\
[Colors:Window]\nBackgroundNormal=239,240,241\nForegroundNormal=35,38,41\n\n\
[General]\nColorScheme=BreezeLight\n";

fn dark_palette() -> Palette {
    sp::palette_from_kde(KDEGLOBALS_DARK).unwrap()
}

/// Конфигурация Noctalia с комментариями и чужими таблицами — всё это
/// обязано остаться на месте.
const NOCTALIA_STATE: &str = "# written by noctalia settings\n\
[wallpaper]\nenabled = true # keep\n\n[theme]\nsource = \"builtin\" # my choice\nbuiltin = \"Nord\"\n\n\
[bar]\nposition = \"top\"\n";

/// Настройки DMS — как их пишет `JSON.stringify(…, null, 2)`.
const DMS_SETTINGS: &str = "{\n  \"currentThemeName\": \"purple\",\n  \"cornerRadius\": 16,\n  \"screenPreferences\": {\n    \"wallpaper\": [\n      \"all\"\n    ]\n  },\n  \"popupTransparency\": 0.9\n}\n";

// ============================================================================
// Цвета и схема KDE
// ============================================================================

#[test]
fn цвет_разбирается_в_обоих_видах() {
    assert_eq!(Rgb::parse("#3daee9"), Some(Rgb(61, 174, 233)));
    assert_eq!(Rgb::parse("#3daee9ff"), Some(Rgb(61, 174, 233)));
    assert_eq!(Rgb::parse("#fff"), Some(Rgb(255, 255, 255)));
    assert_eq!(Rgb::parse("61,174,233"), Some(Rgb(61, 174, 233)));
    assert_eq!(Rgb::parse(" 61, 174, 233, 255 "), Some(Rgb(61, 174, 233)));
    assert_eq!(Rgb::parse("300,0,0"), None);
    assert_eq!(Rgb::parse("blue"), None);
    assert_eq!(Rgb(61, 174, 233).hex(), "#3daee9");
    assert_eq!(Rgb(61, 174, 233).kde(), "61,174,233");
}

#[test]
fn палитра_из_kdeglobals() {
    let p = dark_palette();
    assert!(p.dark);
    assert_eq!(p.surface, Rgb(32, 35, 38));
    assert_eq!(p.on_surface, Rgb(252, 252, 252));
    assert_eq!(p.primary, Rgb(61, 174, 233), "акцент — цвет выделения");
    assert_eq!(p.error, Rgb(218, 68, 83));
    assert_eq!(p.on_surface_variant, Rgb(161, 169, 177));

    let light = sp::palette_from_kde(KDEGLOBALS_LIGHT).unwrap();
    assert!(!light.dark);

    // Свой цвет акцента Plasma главнее выделения.
    let with_accent = format!("{KDEGLOBALS_DARK}\n[General]\nAccentColor=233,100,61\n");
    assert_eq!(sp::palette_from_kde(&with_accent).unwrap().primary, Rgb(233, 100, 61));

    // Схема не применялась — палитры нет, а не выдуманная.
    assert_eq!(sp::palette_from_kde("[General]\nColorScheme=X\n"), None);
}

#[test]
fn схема_kde_из_палитры_читается_обратно_той_же() {
    let p = dark_palette();
    let text = sp::kde_scheme_text(&p, "RexonShell", "Rexon — Shell Colors");
    assert!(text.contains("[General]\nColorScheme=RexonShell\nName=Rexon — Shell Colors\n"));
    for group in ["Colors:Window", "Colors:View", "Colors:Button", "Colors:Selection", "Colors:Tooltip", "WM"] {
        assert!(text.contains(&format!("[{group}]\n")), "нет группы {group}");
    }
    let back = sp::palette_from_kde(&text).unwrap();
    assert_eq!(back.dark, p.dark);
    assert_eq!(back.surface, p.surface);
    assert_eq!(back.on_surface, p.on_surface);
    assert_eq!(back.primary, p.primary);
    assert_eq!(back.on_primary, p.on_primary);
    assert_eq!(back.error, p.error);
}

#[test]
fn файлы_палитр_шеллов_читаются_обратно() {
    let p = dark_palette();
    let noct = sp::noctalia_palette_json(&p);
    let v: serde_json::Value = serde_json::from_str(&noct).unwrap();
    assert_eq!(v["dark"]["mPrimary"], "#3daee9");
    assert_eq!(v["dark"].as_object().unwrap().len(), 16, "16 ролей Noctalia");
    assert_eq!(v["light"], v["dark"], "режим ставится отдельно — варианты одинаковы");
    let back = sp::palette_from_noctalia(&noct, true).unwrap();
    assert_eq!(back, p);

    let dms = sp::dms_theme_json(&p);
    let back = sp::palette_from_dms_theme(&dms, true).unwrap();
    assert_eq!(back.primary, p.primary);
    assert_eq!(back.on_primary, p.on_primary);
    assert_eq!(back.surface, p.surface);
    assert_eq!(back.on_surface, p.on_surface);
    assert_eq!(back.surface_variant, p.surface_variant);
}

/// Шаблон Noctalia после подстановки — правильный JSON того же вида, что
/// `dms-colors.json`, и читается одним читателем.
#[test]
fn шаблон_noctalia_после_подстановки_читается() {
    let t = sp::noctalia_bridge_template();
    assert!(t.contains("\"primary\": \"{{ colors.primary.dark.hex }}\""));
    assert!(t.contains("\"mode\": \"{{ mode }}\""));
    // Подставляем, как это сделал бы шелл: тёмный режим, светлые — белые.
    let mut out = String::new();
    let mut rest = t.as_str();
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let j = rest[i..].find("}}").unwrap() + i;
        let token = rest[i + 2..j].trim();
        out.push_str(match token {
            "mode" => "light",
            t if t.ends_with(".light.hex") && t.starts_with("colors.surface.") => "#fafafa",
            t if t.ends_with(".light.hex") && t.starts_with("colors.on_surface.") => "#111111",
            t if t.ends_with(".light.hex") && t.starts_with("colors.primary.") => "#6750a4",
            t if t.ends_with(".hex") => "#808080",
            other => panic!("неизвестная метка {other}"),
        });
        rest = &rest[j + 2..];
    }
    out.push_str(rest);
    let p = sp::palette_from_material(&out, None).unwrap();
    assert!(!p.dark, "режим — из поля mode");
    assert_eq!(p.surface, Rgb(0xfa, 0xfa, 0xfa));
    assert_eq!(p.primary, Rgb(0x67, 0x50, 0xa4));
}

// ============================================================================
// Точечная правка
// ============================================================================

#[test]
fn toml_правится_одной_строкой() {
    let edits = [sp::TomlEdit { table: "theme".into(), key: "mode".into(), line: Some("mode = \"light\"".into()) }];
    let out = sp::toml_apply(NOCTALIA_STATE, &edits, &[]).unwrap();
    assert_eq!(
        out,
        NOCTALIA_STATE.replace("builtin = \"Nord\"\n", "builtin = \"Nord\"\nmode = \"light\"\n"),
        "новая строка — в конец своей таблицы, остальное байт в байт"
    );
    // Замена существующей строки — на месте.
    let edits = [sp::TomlEdit { table: "theme".into(), key: "source".into(), line: Some("source = \"custom\"".into()) }];
    let out = sp::toml_apply(NOCTALIA_STATE, &edits, &[]).unwrap();
    assert_eq!(out, NOCTALIA_STATE.replace("source = \"builtin\" # my choice", "source = \"custom\""));
    // Таблицы нет — дописывается в конец.
    let edits = [sp::TomlEdit { table: "theme".into(), key: "mode".into(), line: Some("mode = \"dark\"".into()) }];
    assert_eq!(sp::toml_apply("[bar]\nx = 1", &edits, &[]).unwrap(), "[bar]\nx = 1\n\n[theme]\nmode = \"dark\"\n");
}

#[test]
fn toml_в_чужом_виде_не_правится() {
    // Ключ темы записан точками в корне: наш `[theme]` дал бы второе
    // определение — отказ, а не порча.
    let text = "theme.mode = \"light\"\n";
    let edits = [sp::TomlEdit { table: "theme".into(), key: "source".into(), line: Some("source = \"custom\"".into()) }];
    assert!(sp::toml_apply(text, &edits, &[]).is_err());
    // Битый файл не трогается вовсе.
    assert!(matches!(sp::toml_apply("[theme\n", &edits, &[]), Err(sp::EditError::Unparsable(_))));
}

#[test]
fn json_правится_членом_верхнего_уровня() {
    let out = sp::json_set_member(DMS_SETTINGS, "currentThemeName", Some("\"custom\"")).unwrap();
    assert_eq!(out, DMS_SETTINGS.replace("\"purple\"", "\"custom\""));
    let added = sp::json_set_member(DMS_SETTINGS, "customThemeFile", Some("\"/x/y.json\"")).unwrap();
    assert_eq!(
        added,
        DMS_SETTINGS.replace("\"popupTransparency\": 0.9\n", "\"popupTransparency\": 0.9,\n  \"customThemeFile\": \"/x/y.json\"\n")
    );
    // Убрать добавленное — ровно исходный текст.
    assert_eq!(sp::json_set_member(&added, "customThemeFile", None).unwrap(), DMS_SETTINGS);
    // Вложенный ключ с тем же именем — не наш, правится только верхний.
    let nested = "{\n  \"a\": {\"currentThemeName\": 1},\n  \"currentThemeName\": \"x\"\n}\n";
    let out = sp::json_set_member(nested, "currentThemeName", Some("\"y\"")).unwrap();
    assert_eq!(out, nested.replace("\"x\"", "\"y\""));
    // Пустой объект.
    assert_eq!(sp::json_set_member("{}\n", "k", Some("true")).unwrap(), "{\n  \"k\": true\n}\n");
    // Не объект и битый текст — отказ.
    assert!(sp::json_set_member("[1]", "k", Some("1")).is_err());
    assert!(sp::json_set_member("{\"a\": ", "k", Some("1")).is_err());
}

// ============================================================================
// Из Rexon в шелл
// ============================================================================

#[test]
fn noctalia_запись_повтор_и_возврат_бит_в_бит() {
    let _lang = i18n_pin::en();
    let (root, home) = fresh_home("noct");
    let state = home.noctalia_state_file();
    put(&state, NOCTALIA_STATE);
    put(&home.noctalia_config_dir().join("config.toml"), "[theme]\nmode = \"dark\"\n");
    let before = snapshot(&root);

    let p = sp::palette_from_kde(KDEGLOBALS_LIGHT).unwrap();
    let msg = sp::push_palette(DesktopShell::Noctalia, &p, &home).unwrap();
    assert!(msg.starts_with("Palette passed to Noctalia."), "{msg}");
    assert!(msg.contains("light"), "{msg}");

    // Свой файл палитры и три наших ключа — больше ничего.
    let palette = read(&home.noctalia_palette_file("Rexon")).unwrap();
    assert_eq!(palette, sp::noctalia_palette_json(&p));
    let written = read(&state).unwrap();
    assert_eq!(
        written,
        NOCTALIA_STATE
            .replace("source = \"builtin\" # my choice", "source = \"custom\"")
            .replace("builtin = \"Nord\"\n", "builtin = \"Nord\"\ncustom_palette = \"Rexon\"\nmode = \"light\"\n")
    );
    assert_eq!(read(&sp::backup_path(&state)).as_deref(), Some(NOCTALIA_STATE), "резервная копия — прежний файл");
    assert_eq!(sp::shell_dark(DesktopShell::Noctalia, &home), Some(false), "режим шелла — как у схемы");

    // Повтор — без изменений: ни записи, ни новой копии.
    let after_first = snapshot(&root);
    let again = sp::push_palette(DesktopShell::Noctalia, &p, &home).unwrap();
    assert_eq!(again, "Noctalia already has this palette — nothing to change.");
    assert_eq!(snapshot(&root), after_first);

    // Возврат — бит в бит.
    let out = sp::revert(&home).unwrap();
    assert_eq!(out.message, "The palette of Noctalia is back as it was before Rexon.");
    assert!(out.skipped.is_empty());
    assert_eq!(snapshot_without_memo(&root), before);
    assert!(!sp::read_memo(&home).has_changes());
}

#[test]
fn noctalia_без_файла_состояния_возврат_убирает_файл() {
    let (root, home) = fresh_home("noct-empty");
    let before = snapshot(&root);
    let p = sp::palette_from_kde(KDEGLOBALS_LIGHT).unwrap();
    sp::push_palette(DesktopShell::Noctalia, &p, &home).unwrap();
    let state = read(&home.noctalia_state_file()).unwrap();
    assert_eq!(state, "[theme]\nsource = \"custom\"\ncustom_palette = \"Rexon\"\nmode = \"light\"\n");
    sp::revert(&home).unwrap();
    assert_eq!(snapshot_without_memo(&root), before);
}

/// Тёмная схема и тёмный шелл по умолчанию: ключ режима не пишется вовсе —
/// правим только то, что разошлось.
#[test]
fn noctalia_режим_не_пишется_без_нужды() {
    let (_root, home) = fresh_home("noct-mode");
    sp::push_palette(DesktopShell::Noctalia, &dark_palette(), &home).unwrap();
    let state = read(&home.noctalia_state_file()).unwrap();
    assert!(!state.contains("mode"), "{state}");
}

/// Шелл переписал файл после нас (свой ключ поменял): возврат снимает
/// только наши ключи, чужую правку оставляет.
#[test]
fn noctalia_возврат_после_правки_шелла_только_наши_ключи() {
    let (_root, home) = fresh_home("noct-foreign");
    let state = home.noctalia_state_file();
    put(&state, NOCTALIA_STATE);
    let p = sp::palette_from_kde(KDEGLOBALS_LIGHT).unwrap();
    sp::push_palette(DesktopShell::Noctalia, &p, &home).unwrap();
    let ours = read(&state).unwrap();
    put(&state, &ours.replace("position = \"top\"", "position = \"bottom\""));
    let out = sp::revert(&home).unwrap();
    assert!(out.skipped.is_empty(), "{:?}", out.skipped);
    assert_eq!(read(&state).unwrap(), NOCTALIA_STATE.replace("position = \"top\"", "position = \"bottom\""));
}

#[test]
fn noctalia_чужой_вид_ключей_отказ_без_порчи() {
    let _lang = i18n_pin::en();
    let (root, home) = fresh_home("noct-dotted");
    put(&home.noctalia_state_file(), "theme.builtin = \"Nord\"\n");
    let before = snapshot(&root);
    let err = sp::push_palette(DesktopShell::Noctalia, &dark_palette(), &home).unwrap_err();
    assert!(err.starts_with("Rexon left the settings of Noctalia alone:"), "{err}");
    assert!(err.ends_with("Nothing was changed."), "{err}");
    assert_eq!(snapshot(&root), before, "ни палитры, ни памятки, ни копии");
}

#[test]
fn dms_запись_повтор_и_возврат_бит_в_бит() {
    let _lang = i18n_pin::en();
    let (root, home) = fresh_home("dms");
    let settings = home.dms_settings_file();
    put(&settings, DMS_SETTINGS);
    let session = home.dms_session_file();
    put(&session, "{\n  \"isLightMode\": false,\n  \"wallpaperPath\": \"/w.png\"\n}\n");
    let before = snapshot(&root);

    let p = sp::palette_from_kde(KDEGLOBALS_LIGHT).unwrap();
    sp::push_palette(DesktopShell::DankMaterialShell, &p, &home).unwrap();
    let theme = home.dms_theme_file();
    assert_eq!(read(&theme).unwrap(), sp::dms_theme_json(&p));
    let s = read(&settings).unwrap();
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["currentThemeName"], "custom");
    assert_eq!(v["currentThemeCategory"], "custom");
    assert_eq!(v["customThemeFile"], theme.to_string_lossy().as_ref());
    assert_eq!(v["cornerRadius"], 16);
    assert!(s.starts_with("{\n  \"currentThemeName\": \"custom\",\n  \"cornerRadius\": 16,"), "{s}");
    assert_eq!(
        read(&session).unwrap(),
        "{\n  \"isLightMode\": true,\n  \"wallpaperPath\": \"/w.png\"\n}\n",
        "режим DMS — как у схемы, остальное на месте"
    );

    let after_first = snapshot(&root);
    sp::push_palette(DesktopShell::DankMaterialShell, &p, &home).unwrap();
    assert_eq!(snapshot(&root), after_first, "повтор — без изменений");

    sp::revert(&home).unwrap();
    assert_eq!(snapshot_without_memo(&root), before);
}

#[test]
fn dms_битый_файл_отказ_без_порчи() {
    let (root, home) = fresh_home("dms-bad");
    put(&home.dms_settings_file(), "{ \"currentThemeName\": ");
    let before = snapshot(&root);
    assert!(sp::push_palette(DesktopShell::DankMaterialShell, &dark_palette(), &home).is_err());
    assert_eq!(snapshot(&root), before);
}

/// DMS переписал настройки целиком (у него свой порядок и свой ключ
/// поменян): наши ключи возвращаются по одному, чужое остаётся.
#[test]
fn dms_возврат_после_правки_шелла_только_наши_ключи() {
    let (_root, home) = fresh_home("dms-foreign");
    let settings = home.dms_settings_file();
    put(&settings, DMS_SETTINGS);
    sp::push_palette(DesktopShell::DankMaterialShell, &dark_palette(), &home).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&read(&settings).unwrap()).unwrap();
    v["cornerRadius"] = serde_json::json!(8);
    put(&settings, &(serde_json::to_string_pretty(&v).unwrap() + "\n"));
    let out = sp::revert(&home).unwrap();
    assert!(out.skipped.is_empty());
    let back: serde_json::Value = serde_json::from_str(&read(&settings).unwrap()).unwrap();
    let mut want: serde_json::Value = serde_json::from_str(DMS_SETTINGS).unwrap();
    want["cornerRadius"] = serde_json::json!(8);
    assert_eq!(back, want);
}

// ============================================================================
// Из шелла в Rexon
// ============================================================================

const DMS_COLORS: &str = r##"{
  "mode": "dark",
  "colors": {
    "dark": {"primary": "#d0bcff", "on_primary": "#381e72", "surface": "#141218", "on_surface": "#e6e0e9",
             "secondary": "#ccc2dc", "error": "#f2b8b5", "surface_container_high": "#2b292f"},
    "light": {"primary": "#6750a4", "on_primary": "#ffffff", "surface": "#fef7ff", "on_surface": "#1d1b20"}
  }
}"##;

#[test]
fn dms_цвета_берутся_из_файла_matugen_по_режиму_шелла() {
    let _lang = i18n_pin::en();
    let (_root, home) = fresh_home("dms-pull");
    match sp::shell_palette(DesktopShell::DankMaterialShell, &home) {
        Err(PullError::Refused(m)) => assert!(m.contains("matugen"), "{m}"),
        other => panic!("{other:?}"),
    }
    put(&home.dms_colors_file(), DMS_COLORS);
    let p = sp::shell_palette(DesktopShell::DankMaterialShell, &home).unwrap();
    assert!(p.dark);
    assert_eq!(p.primary, Rgb(0xd0, 0xbc, 0xff));
    assert_eq!(p.hover, Rgb(0x2b, 0x29, 0x2f));
    // Шелл в светлом режиме — светлая половина файла.
    put(&home.dms_session_file(), "{\"isLightMode\": true}");
    let p = sp::shell_palette(DesktopShell::DankMaterialShell, &home).unwrap();
    assert!(!p.dark);
    assert_eq!(p.primary, Rgb(0x67, 0x50, 0xa4));
    // Своя тема DMS главнее файла matugen.
    put(&home.dms_settings_file(), "{\"currentThemeName\": \"custom\", \"customThemeFile\": \"~/t.json\"}");
    put(&Path::new(&home.home).join("t.json"), "{\"light\": {\"primary\": \"#ff0000\", \"surface\": \"#ffffff\", \"surfaceText\": \"#000000\"}}");
    assert_eq!(sp::shell_palette(DesktopShell::DankMaterialShell, &home).unwrap().primary, Rgb(255, 0, 0));
}

#[test]
fn noctalia_цвета_через_шаблон_и_возврат() {
    let (root, home) = fresh_home("noct-pull");
    put(&home.noctalia_state_file(), NOCTALIA_STATE);
    let before = snapshot(&root);
    assert_eq!(sp::shell_palette(DesktopShell::Noctalia, &home), Err(PullError::NeedsBridge));

    sp::connect_bridge(&home).unwrap();
    assert!(sp::bridge_connected(&home));
    let state = read(&home.noctalia_state_file()).unwrap();
    let tpl = home.bridge_template_file();
    let outp = home.bridge_output_file();
    assert_eq!(
        state,
        format!(
            "{NOCTALIA_STATE}\n[theme.templates.user.rexon_palette]\ninput_path = \"{}\"\noutput_path = \"{}\"\n",
            tpl.display(),
            outp.display()
        )
    );
    assert_eq!(read(&tpl).unwrap(), sp::noctalia_bridge_template());
    assert_eq!(sp::shell_palette(DesktopShell::Noctalia, &home), Err(PullError::BridgePending));

    // Повторное подключение — без изменений.
    let snap = snapshot(&root);
    sp::connect_bridge(&home).unwrap();
    assert_eq!(snapshot(&root), snap);

    // Шелл заполнил шаблон.
    put(&outp, DMS_COLORS);
    let p = sp::shell_palette(DesktopShell::Noctalia, &home).unwrap();
    assert_eq!(p.primary, Rgb(0xd0, 0xbc, 0xff));

    // Возврат снимает шаблон бит в бит (выходной файл заполнял шелл — он
    // наш каталог, его не трогаем).
    sp::revert(&home).unwrap();
    let after: Vec<_> = snapshot_without_memo(&root);
    assert_eq!(after, before);
}

#[test]
fn noctalia_своя_палитра_читается_напрямую() {
    let (_root, home) = fresh_home("noct-custom");
    put(&home.noctalia_state_file(), "[theme]\nsource = \"custom\"\ncustom_palette = \"Mine\"\nmode = \"light\"\n");
    let p = dark_palette();
    put(&home.noctalia_palette_file("Mine"), &sp::noctalia_palette_json(&p));
    let got = sp::shell_palette(DesktopShell::Noctalia, &home).unwrap();
    assert!(!got.dark, "режим — шелла");
    assert_eq!(got.primary, p.primary);
}

#[test]
fn схема_с_цветами_шелла_чередует_имя_и_не_повторяется() {
    let (_root, home) = fresh_home("scheme");
    let p = dark_palette();
    put(&home.kdeglobals(), KDEGLOBALS_DARK);
    let (id, path, text) = sp::shell_scheme_file(&home, &p).unwrap();
    assert_eq!(id, "RexonShell");
    sp::write_shell_scheme(&home, &path, &text).unwrap();
    // Как будто `plasma-apply-colorscheme RexonShell` отработал.
    put(&home.kdeglobals(), &KDEGLOBALS_DARK.replace("ColorScheme=BreezeDark", "ColorScheme=RexonShell"));
    assert_eq!(sp::shell_scheme_file(&home, &p), None, "те же цвета — применять нечего");
    let other = Palette { primary: Rgb(1, 2, 3), ..p };
    let (id2, path2, text2) = sp::shell_scheme_file(&home, &other).unwrap();
    assert_eq!(id2, "RexonShellAlt", "та же схема второй раз не применится — другое имя");
    sp::write_shell_scheme(&home, &path2, &text2).unwrap();

    let memo = sp::read_memo(&home);
    let env = memo.env.unwrap();
    assert_eq!(env.prev_scheme.as_deref(), Some("BreezeDark"), "«до нас» не затирается нашей же схемой");
    assert_eq!(env.scheme_files.len(), 2);

    let out = sp::revert(&home).unwrap();
    assert_eq!(out.reapply_scheme.as_deref(), Some("BreezeDark"));
    assert_eq!(out.drop_scheme_files.len(), 2);
    assert!(path.exists(), "наши схемы убираются ПОСЛЕ возврата прежней");
    sp::forget_env(&home).unwrap();
    assert!(!path.exists() && !path2.exists());
    assert!(!sp::read_memo(&home).has_changes());
}

// ============================================================================
// Полоса и «кто ведёт»
// ============================================================================

#[test]
fn полоса_только_при_живом_шелле() {
    let _lang = i18n_pin::en();
    let (_root, home) = fresh_home("banner");
    assert!(!sp::palette_banner(None, &home).visible);
    let b = sp::palette_banner(Some(DesktopShell::Noctalia), &home);
    assert!(b.visible);
    assert_eq!(b.shell, "noctalia");
    assert!(b.text.contains("dark or light mode"), "{}", b.text);
    assert!(!b.can_push, "без цветов в kdeglobals передавать нечего");
    assert!(!b.can_revert);
    assert_eq!(b.lead, "");
    assert_eq!(b.push_label, "Pass Palette to Shell");
    assert_eq!(b.pull_label, "Take Shell Colors");
    assert_eq!(b.revert_label, "Return as It Was");
    put(&home.kdeglobals(), KDEGLOBALS_DARK);
    sp::set_lead(&home, Lead::Shell).unwrap();
    let b = sp::palette_banner(Some(DesktopShell::DankMaterialShell), &home);
    assert!(b.can_push);
    assert_eq!(b.lead, "shell");
    assert!(b.text.contains("sync mode with portal"), "{}", b.text);
}

#[test]
fn после_схемы_rexon_ведёт_значит_передаём_сразу() {
    let _lang = i18n_pin::en();
    let (_root, home) = fresh_home("after");
    assert_eq!(sp::after_scheme_applied(None, &home), None, "шелла нет — добавлять нечего");
    put(&home.kdeglobals(), KDEGLOBALS_LIGHT);
    let hint = sp::after_scheme_applied(Some(DesktopShell::Noctalia), &home).unwrap();
    assert!(hint.contains("Pass Palette to Shell"), "не выбрано — подсказать кнопку: {hint}");
    assert!(!home.noctalia_state_file().exists(), "без выбора ничего не пишем");

    sp::set_lead(&home, Lead::Rexon).unwrap();
    let msg = sp::after_scheme_applied(Some(DesktopShell::Noctalia), &home).unwrap();
    assert!(msg.starts_with("Palette passed to Noctalia."), "{msg}");
    assert_eq!(sp::shell_dark(DesktopShell::Noctalia, &home), Some(false));

    sp::set_lead(&home, Lead::Shell).unwrap();
    let msg = sp::after_scheme_applied(Some(DesktopShell::Noctalia), &home).unwrap();
    assert!(msg.starts_with("The palette is led by Noctalia"), "{msg}");
    // Выбор переживает возврат.
    sp::revert(&home).unwrap();
    assert_eq!(sp::read_memo(&home).lead, Some(Lead::Shell));
}

/// По-русски: подписи берутся из настоящего `po/ru.po`.
#[test]
fn подписи_по_русски() {
    let _lang = i18n_pin::ru();
    assert_eq!(sp::push_label(), "Передать палитру оболочке");
    assert_eq!(sp::pull_label(), "Взять цвета оболочки");
    assert_eq!(sp::revert_label(), "Вернуть как было");
}

// ============================================================================
// Ход действий целиком (применение схемы — подставное)
// ============================================================================

/// «Применить схему» как это делает `plasma-apply-colorscheme`: имя схемы в
/// `kdeglobals`. Возвращает счётчик вызовов.
fn fake_apply(home: &PaletteHome, calls: &mut Vec<String>, scheme: &str) -> (bool, String) {
    calls.push(scheme.to_string());
    let text = fs::read_to_string(home.kdeglobals()).unwrap_or_default();
    let cur = text.lines().find_map(|l| l.strip_prefix("ColorScheme=")).unwrap_or("").to_string();
    put(&home.kdeglobals(), &text.replace(&format!("ColorScheme={cur}"), &format!("ColorScheme={scheme}")));
    (true, "Color scheme applied".into())
}

#[test]
fn взять_цвета_dms_повтор_и_возврат() {
    let _lang = i18n_pin::en();
    let (_root, home) = fresh_home("pull-flow");
    put(&home.kdeglobals(), KDEGLOBALS_DARK);
    put(&home.dms_colors_file(), DMS_COLORS);
    let kdeglobals_before = read(&home.kdeglobals()).unwrap();
    let mut calls = Vec::new();
    let shell = Some(DesktopShell::DankMaterialShell);
    let (ok, msg) = sp::pull_flow(shell, &home, &mut || {}, 0, &mut |s| fake_apply(&home, &mut calls, s));
    assert!(ok, "{msg}");
    assert_eq!(msg, "The colors of DankMaterialShell are applied as the color scheme “Rexon — Shell Colors”.");
    assert_eq!(calls, ["RexonShell"]);
    let scheme = read(&home.color_scheme_file("RexonShell")).unwrap();
    assert_eq!(sp::palette_from_kde(&scheme).unwrap().primary, Rgb(0xd0, 0xbc, 0xff));

    // Повтор без перемен у шелла — схема заново не применяется.
    let (ok, msg) = sp::pull_flow(shell, &home, &mut || {}, 0, &mut |s| fake_apply(&home, &mut calls, s));
    assert!(ok);
    assert_eq!(msg, "The colors of DankMaterialShell are already applied — nothing to change.");
    assert_eq!(calls.len(), 1);

    // Возврат: прежняя схема применяется обратно, наши файлы схем уходят.
    let (ok, msg) = sp::revert_flow(&home, &mut |s| fake_apply(&home, &mut calls, s));
    assert!(ok, "{msg}");
    assert_eq!(calls.last().map(String::as_str), Some("BreezeDark"));
    assert_eq!(read(&home.kdeglobals()).unwrap(), kdeglobals_before);
    assert!(!home.color_scheme_file("RexonShell").exists());
    assert!(!sp::read_memo(&home).has_changes());
}

/// Noctalia без шаблона: подключаем, «шелл» заполняет файл по толчку —
/// и цвета берутся тем же нажатием.
#[test]
fn взять_цвета_noctalia_подключает_шаблон_и_ждёт() {
    let _lang = i18n_pin::en();
    let (_root, home) = fresh_home("pull-noct");
    put(&home.kdeglobals(), KDEGLOBALS_DARK);
    let out = home.bridge_output_file();
    let mut nudged = 0;
    let mut calls = Vec::new();
    let (ok, msg) = sp::pull_flow(
        Some(DesktopShell::Noctalia),
        &home,
        &mut || {
            nudged += 1;
            put(&out, DMS_COLORS);
        },
        1000,
        &mut |s| fake_apply(&home, &mut calls, s),
    );
    assert!(ok, "{msg}");
    assert_eq!(nudged, 1);
    assert!(sp::bridge_connected(&home));

    // Шелл молчит — честно сказать, а не ждать вечно.
    let (_r2, home2) = fresh_home("pull-noct-silent");
    let (ok, msg) = sp::pull_flow(Some(DesktopShell::Noctalia), &home2, &mut || {}, 0, &mut |_| (true, String::new()));
    assert!(!ok);
    assert!(msg.contains("Take Shell Colors"), "{msg}");

    // Шелла нет вовсе.
    let (ok, msg) = sp::pull_flow(None, &home2, &mut || {}, 0, &mut |_| (true, String::new()));
    assert!(!ok);
    assert!(msg.starts_with("No running desktop shell"), "{msg}");
}

#[test]
fn передать_без_схемы_честный_отказ() {
    let _lang = i18n_pin::en();
    let (root, home) = fresh_home("push-flow");
    let before = snapshot(&root);
    let (ok, msg) = sp::push_flow(Some(DesktopShell::Noctalia), &home);
    assert!(!ok);
    assert!(msg.starts_with("There is no current color scheme to pass on"), "{msg}");
    assert_eq!(snapshot(&root), before);
}

/// Немецкий — из настоящего `po/de.po`: кнопки и полоса не остаются
/// английскими.
#[test]
fn подписи_по_немецки() {
    let _lang = i18n_pin::en();
    let po = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../po/de.po");
    let text = fs::read_to_string(po).unwrap();
    for (en, de) in [
        ("Pass Palette to Shell", "Palette an Shell übergeben"),
        ("Take Shell Colors", "Shell-Farben übernehmen"),
        ("Return as It Was", "Zurücksetzen"),
    ] {
        assert!(text.contains(&format!("msgid \"{en}\"\nmsgstr \"{de}\"")), "нет перевода «{en}»");
    }
}

// ── Цветовая схема KDE вне Plasma ──────────────────────────────────────
//
// Подставной дом и подставная машина: ни одна команда не запускается
// (`plasma-apply-colorscheme`, `gsettings`, `dbus-send` только
// записываются), файлы пишутся под `CARGO_TARGET_TMPDIR`.

use rexon_core::animations::LookIo;
use std::cell::RefCell;

struct FakeIo {
    tools: Vec<&'static str>,
    calls: RefCell<Vec<Vec<String>>>,
    run_ok: bool,
    run_out: String,
}

impl FakeIo {
    fn new(tools: &[&'static str]) -> FakeIo {
        FakeIo { tools: tools.to_vec(), calls: RefCell::new(Vec::new()), run_ok: true, run_out: String::new() }
    }
}

impl LookIo for FakeIo {
    fn run(&self, argv: &[String]) -> (bool, String) {
        self.calls.borrow_mut().push(argv.to_vec());
        (self.run_ok, self.run_out.clone())
    }
    fn have(&self, tool: &str) -> bool {
        self.tools.contains(&tool)
    }
    fn read(&self, path: &str) -> Option<String> {
        fs::read_to_string(path).ok()
    }
    fn write(&self, path: &str, text: &str) -> bool {
        put(Path::new(path), text);
        true
    }
    fn list_dir(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}

/// Схема с заголовком и его неактивной подгруппой, без подсказок и
/// без своего `[WM]`.
const NIGHT_SCHEME: &str = "[ColorEffects:Disabled]\nColor=56,56,56\nContrastAmount=0.5\nContrastEffect=1\n\n\
[Colors:Button]\nBackgroundNormal=40,40,50\nForegroundNormal=230,230,230\n\n\
[Colors:Header]\nBackgroundNormal=10,10,20\nForegroundNormal=250,250,250\n\n\
[Colors:Header][Inactive]\nBackgroundNormal=99,99,99\nForegroundNormal=200,200,200\n\n\
[Colors:Selection]\nBackgroundNormal=0,120,215\nForegroundNormal=255,255,255\n\n\
[Colors:View]\nBackgroundNormal=20,20,30\nBackgroundAlternate=25,25,35\nForegroundNormal=240,240,240\n\
ForegroundInactive=150,150,160\nForegroundLink=80,160,255\nForegroundVisited=160,100,200\n\n\
[Colors:Window]\nBackgroundNormal=30,30,40\nForegroundNormal=235,235,235\nForegroundInactive=140,140,150\n\n\
[General]\nName=Night\n\n[KDE]\ncontrast=5\n";

const DAY_SCHEME: &str = "[Colors:Selection]\nBackgroundNormal=200,0,0\nForegroundNormal=255,255,255\n\n\
[Colors:View]\nBackgroundNormal=255,255,255\nForegroundNormal=0,0,0\n\n\
[Colors:Window]\nBackgroundNormal=240,240,240\nForegroundNormal=10,10,10\n\n[General]\nName=Day\n";

fn scheme_home(tag: &str, de: &str) -> (PathBuf, sp::SchemeCtx) {
    let (root, home) = fresh_home(tag);
    put(&root.join(".local/share/color-schemes/Night.colors"), NIGHT_SCHEME);
    put(&root.join(".local/share/color-schemes/Day.colors"), DAY_SCHEME);
    let ctx = sp::SchemeCtx {
        de_id: de.to_string(),
        home,
        // Системных схем в подставном доме нет.
        data_dirs: vec![root.join("no-system").to_string_lossy().to_string()],
        session_platform_theme: String::new(),
    };
    (root, ctx)
}

#[test]
fn ini_edit_keeps_foreign_lines_and_nested_groups() {
    let text = "# comment\n[General]\nfont=Noto Sans,10\nColorScheme=Old\n\n[Colors:Header][Inactive]\nBackgroundNormal=1,2,3\n\n[Icons]\nTheme=Papirus\n";
    let out = sp::ini_apply(
        Some(text),
        &[
            sp::IniEdit { group: "General".into(), key: "ColorScheme".into(), value: Some("New".into()) },
            sp::IniEdit { group: "Colors:Header][Inactive".into(), key: "BackgroundNormal".into(), value: None },
            sp::IniEdit { group: "UiSettings".into(), key: "ColorScheme".into(), value: Some("New".into()) },
        ],
    );
    assert_eq!(
        out,
        "# comment\n[General]\nfont=Noto Sans,10\nColorScheme=New\n\n[Icons]\nTheme=Papirus\n\n[UiSettings]\nColorScheme=New\n"
    );
    assert_eq!(sp::ini_get(&out, "General", "font").as_deref(), Some("Noto Sans,10"));
    // Снять последний ключ последней группы — группа уходит без хвоста.
    let back = sp::ini_apply(Some(&out), &[sp::IniEdit { group: "UiSettings".into(), key: "ColorScheme".into(), value: None }]);
    assert_eq!(back, "# comment\n[General]\nfont=Noto Sans,10\nColorScheme=New\n\n[Icons]\nTheme=Papirus\n");
}

#[test]
fn kdeglobals_gets_the_same_groups_plasma_writes() {
    let before = "[General]\nfont=Noto Sans,10\n\n[Colors:View]\nBackgroundNormal=1,1,1\nStaleKey=9,9,9\n";
    let edits = sp::scheme_kdeglobals_edits(before, NIGHT_SCHEME, "Night", "abc", true);
    let out = sp::ini_apply(Some(before), &edits);
    // Старая группа заменена целиком (лишний ключ ушёл), чужое осталось.
    assert_eq!(sp::ini_get(&out, "Colors:View", "StaleKey"), None);
    assert_eq!(sp::ini_get(&out, "Colors:View", "BackgroundNormal").as_deref(), Some("20,20,30"));
    assert_eq!(sp::ini_get(&out, "General", "font").as_deref(), Some("Noto Sans,10"));
    assert_eq!(sp::ini_get(&out, "General", "ColorScheme").as_deref(), Some("Night"));
    assert_eq!(sp::ini_get(&out, "General", "ColorSchemeHash").as_deref(), Some("abc"));
    assert_eq!(sp::ini_get(&out, "UiSettings", "ColorScheme").as_deref(), Some("Night"));
    // Неактивный заголовок, как у Plasma, — с фона окна, не из схемы.
    assert_eq!(sp::ini_get(&out, "Colors:Header][Inactive", "BackgroundNormal").as_deref(), Some("30,30,40"));
    // [WM] без своего раздела в схеме — из заголовка.
    assert_eq!(sp::ini_get(&out, "WM", "activeBackground").as_deref(), Some("10,10,20"));
    assert_eq!(sp::ini_get(&out, "WM", "inactiveBackground").as_deref(), Some("99,99,99"));
    assert_eq!(sp::ini_get(&out, "KDE", "contrast").as_deref(), Some("5"));
    assert_eq!(sp::ini_get(&out, "KDE", "frameContrast").as_deref(), Some("0.2"));
    assert_eq!(sp::ini_get(&out, "ColorEffects:Disabled", "ContrastAmount").as_deref(), Some("0.5"));
    // Схема без заголовка: группы заголовка нет вовсе.
    let day = sp::ini_apply(Some(&out), &sp::scheme_kdeglobals_edits(&out, DAY_SCHEME, "Day", "", false));
    assert!(!day.contains("[Colors:Header]"), "{day}");
    assert_eq!(sp::ini_get(&day, "UiSettings", "ColorScheme").as_deref(), Some("Night"), "pin=false не трогает ключ");
}

#[test]
fn qtct_palette_follows_kde_roles() {
    let text = sp::qtct_palette_text(NIGHT_SCHEME);
    let roles = |key: &str| -> Vec<String> {
        let line = text.lines().find(|l| l.starts_with(key)).unwrap();
        line.split_once('=').unwrap().1.split(", ").map(str::to_string).collect()
    };
    let active = roles("active_colors=");
    assert_eq!(active.len(), 22);
    assert_eq!(active[0], "#ffebebeb", "WindowText — текст окна");
    assert_eq!(active[9], "#ff14141e", "Base — фон списков (View)");
    assert_eq!(active[10], "#ff1e1e28", "Window");
    assert_eq!(active[12], "#ff0078d7", "Highlight — выделение");
    assert_eq!(active[14], "#ff50a0ff", "Link");
    assert_eq!(active[20], "#ff9696a0", "PlaceholderText — неактивный текст View");
    // Выключенный текст выцветает к фону, фон остаётся.
    let disabled = roles("disabled_colors=");
    assert_ne!(disabled[6], active[6]);
    assert_eq!(disabled[9], active[9]);
    assert_eq!(roles("inactive_colors="), active);
}

#[test]
fn layer_outside_plasma_writes_kde_qt_and_env_and_reverts_bit_for_bit() {
    let (root, ctx) = scheme_home("gnome", "gnome");
    put(&root.join(".config/kdeglobals"), "[General]\nfont=Noto Sans,10\n\n[KDE]\nSingleClick=false\n");
    put(&root.join(".config/qt6ct/qt6ct.conf"), "[Appearance]\nstyle=Fusion\nicon_theme=Papirus\n\n[Fonts]\nfixed=\"Mono,10\"\n");
    let before = snapshot(&root);
    let io = FakeIo::new(&["qt6ct"]);

    let (ok, msg, out) = sp::apply_scheme(&io, &ctx, "Night");
    assert!(ok, "{msg}");
    assert!(out.qt_layer && out.env_added, "{out:?}");
    let kg = read(&root.join(".config/kdeglobals")).unwrap();
    assert_eq!(sp::ini_get(&kg, "KDE", "SingleClick").as_deref(), Some("false"), "чужой ключ на месте");
    assert_eq!(sp::ini_get(&kg, "UiSettings", "ColorScheme").as_deref(), Some("Night"));
    let palette = root.join(".config/qt6ct/colors/Night.conf");
    assert!(read(&palette).unwrap().starts_with("[ColorScheme]\nactive_colors="));
    let conf = read(&root.join(".config/qt6ct/qt6ct.conf")).unwrap();
    assert_eq!(sp::ini_get(&conf, "Appearance", "custom_palette").as_deref(), Some("true"));
    assert_eq!(
        sp::ini_get(&conf, "Appearance", "color_scheme_path").as_deref(),
        Some(palette.to_string_lossy().as_ref())
    );
    assert_eq!(sp::ini_get(&conf, "Appearance", "icon_theme").as_deref(), Some("Papirus"), "свои значки не перебиты");
    assert_eq!(sp::ini_get(&conf, "Fonts", "fixed").as_deref(), Some("\"Mono,10\""));
    assert_eq!(
        read(&root.join(".config/environment.d/rexon-qt-style.conf")).as_deref(),
        Some("QT_QPA_PLATFORMTHEME=qt6ct\n")
    );
    assert!(msg.contains("GTK"), "{msg}");
    // Ни одной команды, кроме уведомления по шине (значки известны из qt6ct
    // не нужны — gsettings спрашивать можно, запускать plasma-* — нельзя).
    assert!(io.calls.borrow().iter().all(|c| c[0] == "dbus-send" || c[0] == "gsettings"), "{:?}", io.calls.borrow());

    // Вторая схема: прежний файл палитры убран, переменная не дублируется.
    let (ok, _, out2) = sp::apply_scheme(&io, &ctx, "Day");
    assert!(ok);
    assert!(!out2.env_added);
    assert!(read(&palette).is_none(), "палитра прежней схемы осталась");
    assert!(read(&root.join(".config/qt6ct/colors/Day.conf")).is_some());

    // Возврат — бит в бит, памятки не остаётся.
    sp::revert_scheme_layer(&ctx.home).unwrap();
    assert_eq!(snapshot(&root), before);
}

/// Шаг 5а: план слоёв собирается без единой записи, а сбой записи посреди
/// нажатия (каталог палитр qt6ct закрыт) возвращает ВСЁ побайтно —
/// `kdeglobals`, `qt6ct.conf`, файл переменных и саму памятку «что было».
#[test]
fn layer_press_plans_without_writes_and_failure_rolls_back_bit_for_bit() {
    use std::os::unix::fs::PermissionsExt;
    let (root, ctx) = scheme_home("press-fail", "xfce");
    put(&root.join(".config/kdeglobals"), "[General]\nfont=Noto Sans,10\n");
    put(&root.join(".config/qt6ct/qt6ct.conf"), "[Appearance]\nstyle=Fusion\n");
    let before = snapshot(&root);
    let io = FakeIo::new(&["qt6ct"]);

    let press = sp::scheme_layer_press(&io, &ctx, false, "Night").expect("план собран");
    assert_eq!(snapshot(&root), before, "сборка плана что-то записала");
    assert!(matches!(press.steps.first(), Some(rexon_core::themes::ApplyStep::ForceOk)), "{:?}", press.steps);
    assert!(press.outcome.qt_layer && press.outcome.env_added, "{:?}", press.outcome);
    assert!(sp::scheme_layer_press(&io, &ctx, false, "Nope").is_err(), "нет схемы — отказ до первого шага");

    let palettes = root.join(".config/qt6ct/colors");
    fs::create_dir_all(&palettes).unwrap();
    let before = snapshot(&root);
    fs::set_permissions(&palettes, fs::Permissions::from_mode(0o555)).unwrap();
    let probe = fs::write(palettes.join(".probe"), "").is_ok();
    if probe {
        let _ = fs::remove_file(palettes.join(".probe"));
        fs::set_permissions(&palettes, fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("НЕ ПРОВЕРЕНО: запись в закрытый каталог проходит (root?)");
        return;
    }
    let (ok, msg, _) = sp::apply_scheme(&io, &ctx, "Night");
    fs::set_permissions(&palettes, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!ok, "сбой записи не стал отказом: {msg}");
    assert!(msg.contains("qt6ct/colors"), "причина не названа: {msg}");
    assert_eq!(snapshot(&root), before, "сбой посреди нажатия оставил записи");
    assert!(!sp::read_layer_memo(&ctx.home).has_changes(), "памятка осталась после сбоя");
}

#[test]
fn layer_revert_after_foreign_edit_returns_only_our_keys() {
    let (root, ctx) = scheme_home("foreign", "hyprland");
    put(&root.join(".config/kdeglobals"), "[General]\nColorScheme=BreezeLight\n");
    let io = FakeIo::new(&[]);
    let (ok, msg, out) = sp::apply_scheme(&io, &ctx, "Night");
    assert!(ok, "{msg}");
    assert!(!out.qt_layer, "без qt6ct палитры Qt нет");
    assert!(msg.contains("qt6ct"), "честно сказано, почему: {msg}");
    // Кто-то дописал свой ключ после нас.
    let kg = root.join(".config/kdeglobals");
    let edited = sp::ini_apply(read(&kg).as_deref(), &[sp::IniEdit { group: "Icons".into(), key: "Theme".into(), value: Some("Papirus".into()) }]);
    put(&kg, &edited);
    sp::revert_scheme_layer(&ctx.home).unwrap();
    let back = read(&kg).unwrap();
    assert_eq!(back, "[General]\nColorScheme=BreezeLight\n\n[Icons]\nTheme=Papirus\n");
}

#[test]
fn lxqt_gets_native_palette_without_qt6ct_or_env() {
    let (root, ctx) = scheme_home("lxqt", "lxqt");
    put(&root.join(".config/lxqt/lxqt.conf"), "[General]\ntheme=frost\n\n[Qt]\nstyle=Fusion\n");
    let io = FakeIo::new(&["qt6ct"]);
    let (ok, msg, out) = sp::apply_scheme(&io, &ctx, "Night");
    assert!(ok, "{msg}");
    assert!(out.qt_layer && !out.env_added);
    let conf = read(&root.join(".config/lxqt/lxqt.conf")).unwrap();
    assert_eq!(sp::ini_get(&conf, "Palette", "window_color").as_deref(), Some("#1e1e28"));
    assert_eq!(sp::ini_get(&conf, "Palette", "highlight_color").as_deref(), Some("#0078d7"));
    assert_eq!(sp::ini_get(&conf, "Qt", "style").as_deref(), Some("Fusion"));
    assert!(read(&root.join(".config/qt6ct/qt6ct.conf")).is_none());
    assert!(read(&root.join(".config/environment.d/rexon-qt-style.conf")).is_none());
}

#[test]
fn plasma_uses_its_tool_and_releases_our_pin() {
    let (root, mut ctx) = scheme_home("plasma", "gnome");
    let io = FakeIo::new(&["qt6ct"]);
    assert!(sp::apply_scheme(&io, &ctx, "Night").0);
    // Тот же дом, теперь сеанс Plasma.
    ctx.de_id = "kde".into();
    io.calls.borrow_mut().clear();
    let (ok, _, _) = sp::apply_scheme(&io, &ctx, "Day");
    assert!(ok);
    assert_eq!(io.calls.borrow()[0], vec!["plasma-apply-colorscheme".to_string(), "Day".to_string()]);
    let kg = read(&root.join(".config/kdeglobals")).unwrap();
    assert_eq!(sp::ini_get(&kg, "UiSettings", "ColorScheme"), None, "в Plasma наш ключ мешал бы Параметрам");
    let memo = sp::read_layer_memo(&ctx.home);
    assert!(memo.files.iter().all(|f| !f.path.ends_with("kdeglobals")), "kdeglobals теперь ведёт Plasma");
}

#[test]
fn unknown_scheme_is_refused_without_writes() {
    let (root, ctx) = scheme_home("missing", "xfce");
    let before = snapshot(&root);
    let (ok, msg, _) = sp::apply_scheme(&FakeIo::new(&["qt6ct"]), &ctx, "Nope");
    assert!(!ok);
    assert!(msg.contains("Nope"), "{msg}");
    assert_eq!(snapshot(&root), before);
}

#[test]
fn colors_section_is_offered_outside_plasma_with_honest_gtk_note() {
    use rexon_core::de_customization::{self as dec, De};
    let row = dec::row("theme-kde-colors").unwrap();
    for de in [De::Kde, De::Gnome, De::Cinnamon, De::Xfce, De::Mate, De::Budgie, De::Pantheon, De::Lxqt, De::Cosmic, De::Wlroots] {
        assert!(row.covers(de), "цвета пропали из {}", de.label());
    }
    assert!(!row.covers(De::Deepin));
    assert!(dec::unavailable_note("theme-kde-colors", De::Deepin, "Deepin").is_some());
    assert!(dec::condition_note("theme-kde-colors", De::Kde).is_none(), "в Plasma оговорки нет");
    let note = dec::condition_note("theme-kde-colors", De::Gnome).unwrap();
    assert!(note.contains("GTK"), "{note}");
    assert_eq!(sp::scheme_way("deepin"), sp::SchemeWay::Unavailable);
    assert_eq!(sp::scheme_way("sway"), sp::SchemeWay::Layer { lxqt: false });
}
