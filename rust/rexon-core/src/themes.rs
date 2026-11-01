//! Менеджер GTK-тем, тем иконок и курсоров — порт `rexon/themes.py`
//! (класс `ThemeManager`).
//!
//! Обнаружение / применение GTK-тем, наборов иконок, курсоров (и KDE-тем
//! Plasma) для всех поддерживаемых DE. Каталог популярных тем грузится из
//! встроенного `rexon/themes.json` + пользовательского
//! `~/.config/rexon/themes.json` (перекрывает/дополняет по семантике
//! `dict.update`).
//!
//! Принцип порта (как `system.rs` / `installed_scan.rs` / `defaults.rs`):
//! ВСЯ чистая логика — детерминированные функции вход→выход. Побочные эффекты
//! (реальный `subprocess`/`safe_run` gsettings/xfconf/kwriteconfig/dbus/
//! plasma-apply/hyprctl, чтение каталогов ФС, запись `settings.ini` /
//! `~/.icons/default` / истории) НЕ исполняются в ядре. Они выражаются:
//!   * ПЛАНАМИ команд/записей ([`ApplyStep`], [`Cmd`], [`Plan`],
//!     [`KvantumPlan`]) — исполнитель прогоняет их и сам считает `ok`;
//!   * снимком файловой системы ([`crate::installed_scan::FsSnapshot`]) для
//!     сканеров установленных тем;
//!   * готовыми значениями источников (вывод gsettings/kreadconfig6,
//!     содержимое конфигов) для сборки «текущих тем».
//!
//! Паритет БАЙТ-В-БАЙТ с Python: та же логика слияния каталога (порядок,
//! перекрытие ключей `dict.update`), тот же argv команд и порядок шагов, тот
//! же разбор `index.theme` / `kcminputrc` / `settings.ini`, та же генерация
//! `settings.ini`. Эталоны сняты с ЖИВОГО Python read-only (safe_run/
//! subprocess замоканы, HOME — временный каталог, реальный `rexon/themes.json`
//! как встроенный каталог) — `tests/themes_parity.rs`.
//!
//! Сохранённые квирки оригинала (намеренное расхождение НЕ вводится, паритет):
//!   * [`resolve_schemas`] РЕГИСТРОЗАВИСИМА к ключам SCHEMAS/(«xfce»/«kde»),
//!     а [`is_generic_wm`] — регистронезависима (`de.lower()`); асимметрия
//!     оригинала сохранена;
//!   * `hyprland` НЕ входит в `_NATIVE_DES` → [`is_generic_wm`] для него
//!     `true`: apply пишет `settings.ini` вдобавок к gsettings;
//!   * `lxqt` входит в `_NATIVE_DES`, но не в SCHEMAS → gsettings по общему
//!     GTK-пути, но `settings.ini` НЕ пишется (не generic-wm);
//!   * [`update_gtk_settings_ini`] заменяет строку `{key}=` в ЛЮБОЙ секции
//!     (без проверки `[Settings]`) — квирк оригинала сохранён;
//!   * [`kde_font_to_display`] при непарсибельном размере даёт `10` (как
//!     `except ValueError`).
//!
//! Дополнение 2026-07-30: портированы решения `install_theme_from_archive`
//! ([`pick_archive_dir`], [`archive_install_dest`]), argv и текстовый разбор
//! `_kde_get` ([`kde_read_cmd`], [`kdeglobals_value`]) и все пользовательские
//! сообщения каталога/архива (`msg_*`).
//!
//! ⚠ Правило #16: [`pick_archive_dir`] берёт первую папку архива ПО АЛФАВИТУ —
//! оригинал брал `os.scandir()[0]`, т.е. при нескольких папках устанавливал
//! случайную (порядок задаёт файловая система).
//!
//! НЕ портировано (исполнение — при подключении к живому Python / будущему
//! мосту): реальные `safe_run`/`safe_check_command`, `os.scandir`,
//! `os.makedirs`, чтение/запись `settings.ini` / `~/.icons/default` /
//! `theme_history.json`, сама распаковка архивов тем и `copytree`,
//! вызов `pkg_manager.install`, санитайзер курсоров из живой ФС, логирование.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::i18n::{tr, trc, trf};
use crate::installed_scan::FsSnapshot;

// ============================================================================
// Каталоги поиска (live-значения строятся исполнителем из $HOME)
// ============================================================================

/// `GTK_THEME_DIRS` — где искать GTK-темы (live: usr + `~/.themes`).
pub fn gtk_theme_dirs(home: &str) -> [String; 2] {
    ["/usr/share/themes".to_string(), format!("{home}/.themes")]
}

/// `ICON_DIRS` — где искать иконки/курсоры (live: usr + `~/.local/share/icons`).
pub fn icon_dirs(home: &str) -> [String; 2] {
    [
        "/usr/share/icons".to_string(),
        format!("{home}/.local/share/icons"),
    ]
}

/// Путь встроенного каталога тем — порт `_BUILTIN_JSON` (рядом с `themes.py`).
/// `pkg_dir` — каталог пакета `rexon` (live: каталог `themes.py`).
pub fn builtin_json_path(pkg_dir: &str) -> String {
    format!("{pkg_dir}/themes.json")
}

/// Путь пользовательского каталога тем — порт `_USER_JSON`.
pub fn user_json_path(home: &str) -> String {
    user_json_path_xdg(None, home)
}

/// То же с учётом `$XDG_CONFIG_HOME`.
pub fn user_json_path_xdg(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/rexon/themes.json",
        crate::utils::config_home(xdg_config_home, home)
    )
}

// ============================================================================
// Каталог популярных тем: слияние встроенного + пользовательского JSON
// ============================================================================

/// Порт `_load_themes_catalog`: `themes = {}` → `update(builtin["themes"])`
/// → `update(user["themes"])`. Семантика `dict.update`: существующий ключ
/// заменяется по месту (порядок сохраняется), новый — дописывается в конец.
///
/// `builtin`/`user` — содержимое файлов (None = файла нет). Битый JSON,
/// не-объект верхнего уровня или `themes`, не являющийся объектом, —
/// пропускаются (как `except`/`data.get("themes", {})` в оригинале).
pub fn merge_catalogs(builtin: Option<&str>, user: Option<&str>) -> Map<String, Value> {
    let mut out = Map::new();
    for content in [builtin, user] {
        let Some(text) = content else { continue };
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else {
            continue; // битый JSON / не-объект → пропуск файла
        };
        if let Some(Value::Object(themes)) = root.get("themes") {
            for (k, v) in themes {
                out.insert(k.clone(), v.clone()); // IndexMap: замена по месту / дозапись
            }
        }
    }
    out
}

/// `json.dumps(catalog, indent=2, ensure_ascii=False)` — для сверки байт-в-байт.
pub fn serialize_catalog(catalog: &Map<String, Value>) -> String {
    serde_json::to_string_pretty(&Value::Object(catalog.clone())).expect("Value сериализуется")
}

// ============================================================================
// Разрешение gsettings-схем по DE — порт SCHEMAS / _GENERIC_WM_SCHEMAS /
// _NATIVE_DES / _resolve_schemas / _is_generic_wm
// ============================================================================

/// (role, schema, prop) — роли `gtk`/`icons`/`cursor`/`font`. Порядок ролей
/// = порядок ключей в Python-словаре (важен для `get_current_themes`).
pub type SchemaEntry = (&'static str, &'static str, &'static str);

const GNOME_STACK: &[SchemaEntry] = &[
    ("gtk", "org.gnome.desktop.interface", "gtk-theme"),
    ("icons", "org.gnome.desktop.interface", "icon-theme"),
    ("cursor", "org.gnome.desktop.interface", "cursor-theme"),
    ("font", "org.gnome.desktop.interface", "font-name"),
];

const CINNAMON_STACK: &[SchemaEntry] = &[
    ("gtk", "org.cinnamon.desktop.interface", "gtk-theme"),
    ("icons", "org.cinnamon.desktop.interface", "icon-theme"),
    ("cursor", "org.cinnamon.desktop.interface", "cursor-theme"),
    ("font", "org.cinnamon.desktop.interface", "font-name"),
];

// MATE: курсор — отдельная схема org.mate.peripherals-mouse (не interface!).
const MATE_STACK: &[SchemaEntry] = &[
    ("gtk", "org.mate.interface", "gtk-theme"),
    ("icons", "org.mate.interface", "icon-theme"),
    ("cursor", "org.mate.peripherals-mouse", "cursor-theme"),
    ("font", "org.mate.interface", "font-name"),
];

/// Универсальный GTK-путь для тайловых / wlroots WM — порт `_GENERIC_WM_SCHEMAS`.
const GENERIC_WM_STACK: &[SchemaEntry] = GNOME_STACK;

/// Таблица `SCHEMAS` (де → набор ролей). budgie/cosmic/hyprland опираются на
/// GNOME-стек; cinnamon-mint — алиас cinnamon.
const SCHEMAS: &[(&str, &[SchemaEntry])] = &[
    ("gnome", GNOME_STACK),
    ("cinnamon", CINNAMON_STACK),
    ("cinnamon-mint", CINNAMON_STACK),
    ("mate", MATE_STACK),
    ("budgie", GNOME_STACK),
    // Pantheon читает ключи GNOME (pantheon-default-settings переопределяет
    // именно org.gnome.desktop.interface gtk-theme/icon-theme/cursor-theme/
    // font-name для `:Pantheon`). ⚠️ gtk-theme при входе, смене акцента и —
    // при акценте «авто» — смене обоев перезаписывает io.elementary.
    // settings-daemon; условие названо человеку оговоркой матрицы.
    ("pantheon", GNOME_STACK),
    ("cosmic", GNOME_STACK),
    ("hyprland", GNOME_STACK),
];

/// DE со своим механизмом (не generic-WM) — порт `_NATIVE_DES` (нижний регистр).
const NATIVE_DES: &[&str] = &[
    "kde", "gnome", "cinnamon", "cinnamon-mint", "mate", "budgie", "pantheon",
    "cosmic", "lxqt", "xfce", "deepin",
];

/// Результат `_resolve_schemas`: gsettings-набор ролей или «не-gsettings»
/// бэкенд (xfce → xfconf, kde → kdeglobals, deepin → только чтение DConfig,
/// запись — стоп [`ApplyStep::DdeTreelandGate`], см. [`crate::dde`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schemas {
    /// Набор ролей (native-DE из SCHEMAS либо общий GTK-путь для тайлового WM).
    Gsettings(&'static [SchemaEntry]),
    /// xfce / kde — управляются не через gsettings.
    NonGsettings,
}

impl Schemas {
    /// `schemas["role"]` → (schema, prop). None для роли, которой нет / не-gsettings.
    pub fn role(&self, role: &str) -> Option<(&'static str, &'static str)> {
        match self {
            Schemas::Gsettings(list) => list
                .iter()
                .find(|(r, _, _)| *r == role)
                .map(|(_, s, p)| (*s, *p)),
            Schemas::NonGsettings => None,
        }
    }
}

/// Порт `_resolve_schemas` (РЕГИСТРОЗАВИСИМ, как оригинал `SCHEMAS.get(de)`):
/// набор из SCHEMAS; для «xfce»/«kde» — [`Schemas::NonGsettings`]; иначе —
/// общий GTK-путь для тайловых/wlroots WM.
pub fn resolve_schemas(de: &str) -> Schemas {
    if let Some((_, list)) = SCHEMAS.iter().find(|(k, _)| *k == de) {
        return Schemas::Gsettings(list);
    }
    if de == "xfce" || de == "kde" || de == "deepin" {
        return Schemas::NonGsettings;
    }
    Schemas::Gsettings(GENERIC_WM_STACK)
}

/// Порт `_is_generic_wm` (РЕГИСТРОНЕЗАВИСИМ: `(de or "").lower()`).
pub fn is_generic_wm(de: &str) -> bool {
    let low = de.to_lowercase();
    !NATIVE_DES.contains(&low.as_str())
}

// ============================================================================
// План применения темы — argv/ini/файлы (исполнение при подключении к мосту)
// ============================================================================

/// Команда для `safe_run` — argv + опциональный timeout (сек; None =
/// дефолтные 30с). Порт вызовов `safe_run([...], timeout=...)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cmd {
    pub argv: Vec<String>,
    pub timeout: Option<u64>,
}

fn cmd(argv: &[&str], timeout: Option<u64>) -> Cmd {
    Cmd {
        argv: argv.iter().map(|s| (*s).to_string()).collect(),
        timeout,
    }
}

/// Шаг плана применения. Тег кодирует условность исполнения; сам порядок
/// шагов детерминирован по (theme, de). Исполнитель ведёт флаг `ok`.
/// ⚠️ Теги вариантов и имена полей — это ФОРМА, которую читает исполнитель
/// плана (шим `defaults_set_terminal_plan`/`themes_apply_*_plan` и Python за
/// ним). Она объявлена здесь, у самого типа, а не переписана веткой на
/// каждое поле в мосту: новый вариант доезжает до исполнителя сам.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op")]
pub enum ApplyStep {
    /// Основной вызов, чей returncode определяет `ok`.
    #[serde(rename = "primary")]
    Primary(Cmd),
    /// Вспомогательный вызов, `ok` не меняющий (kde-gtk gsettings и т.п.).
    #[serde(rename = "besteffort")]
    BestEffort(Cmd),
    /// Выполняется, только если `ok` уже true (kde-icon dbus-уведомление).
    #[serde(rename = "ifok")]
    IfOk(Cmd),
    /// Выполняется, только если `ok` ещё false; успех любого делает `ok=true`
    /// (kde-cursor fallback: kwriteconfig6 в kcminputrc/kdeglobals).
    #[serde(rename = "ifnotok")]
    IfNotOk(Cmd),
    /// Обновить `settings.ini` (read-modify-write через
    /// [`update_gtk_settings_ini`]); на `ok` не влияет.
    #[serde(rename = "gtkini")]
    GtkIni { path: String, key: String, value: String },
    /// Hyprland-курсор (только при наличии `hyprctl`): прочитать размер
    /// (`gsettings get org.gnome.desktop.interface cursor-size`, дефолт «24»),
    /// затем `hyprctl setcursor <theme> <size>` и
    /// `hyprctl keyword env XCURSOR_THEME,<theme>`. На `ok` не влияет.
    ///
    /// `size` задан — берётся он, а `gsettings get` не нужен. Удался
    /// `setcursor` — это «сменилось сейчас» в [`CursorReport::live`].
    #[serde(rename = "hyprctl_cursor")]
    HyprctlCursor { theme: String, size: Option<String> },
    /// Команда «сменить в работающем сеансе» (`swaymsg …`): на `ok` не
    /// влияет, её успех идёт в [`CursorReport::live`]. Сеанса нет — команда
    /// просто не удаётся.
    #[serde(rename = "live")]
    Live(Cmd),
    /// Записать НАШ файл (каталог создаётся); удалось — `ok = true`. Для
    /// окружений без своей настройки, где этот файл и есть настоящий эффект
    /// (вместо прежнего безусловного `ForceOk`).
    #[serde(rename = "write_setsok")]
    WriteSetsOk {
        dir: String,
        path: String,
        content: String,
    },
    /// Курсор в конфиг тайлового композитора управляемым блоком — см.
    /// [`WmConfigPlan`]. На `ok` не влияет; итог — в [`CursorReport::wm`].
    #[serde(rename = "wm_config")]
    WmConfig(WmConfigPlan),
    /// `ok = true` безусловно (kde-gtk и тайловые/wlroots WM: `settings.ini`
    /// авторитетен, запись не падает молча).
    #[serde(rename = "forceok")]
    ForceOk,
    /// Deepin: исполнитель здесь останавливает план — `ok = false`, ни
    /// одной записи. В сеансе Treeland темы задаёт только Центр управления,
    /// другие сеансы Deepin Rexon не поддерживает; причина —
    /// [`crate::dde::refusal`].
    #[serde(rename = "dde_treeland_gate")]
    DdeTreelandGate,
    /// Только при `ok`: `makedirs(dir)` + запись `index_path` (для Xorg-курсора
    /// `~/.icons/default/index.theme`).
    #[serde(rename = "write_ifok")]
    WriteIfOk {
        dir: String,
        path: String,
        content: String,
    },
    /// Только при `ok`: правка ключа в ini-файле по месту
    /// ([`crate::defaults::set_ini_value`]) — чужие настройки в файле
    /// сохраняются. Нужна там, где конфиг ведёт не Rexon (`lxqt.conf`,
    /// `qt6ct.conf`). `section` — со скобками (`"[Qt]"`).
    #[serde(rename = "iniset_ifok")]
    IniSetIfOk {
        path: String,
        section: String,
        key: String,
        value: String,
    },
    /// Только при `ok`: правка ключа в чужом ini-файле С УСЛОВИЕМ (см.
    /// [`ini_guarded_text`]): `only_if_file` — файла нет, значит программа
    /// не настроена, и заводить его не нужно; `keep_existing` — ключ уже
    /// задан человеком, не перебивать. Нужна для `qt6ct.conf`.
    #[serde(rename = "iniset_guarded")]
    IniSetGuarded {
        path: String,
        section: String,
        key: String,
        value: String,
        only_if_file: bool,
        keep_existing: bool,
    },
    /// Только при `ok`: убрать ключ из `settings.ini` GTK
    /// ([`remove_gtk_settings_key`]) — тогда GTK берёт значение окружения.
    /// Возврат к стандартному курсору ([`standard_cursor_plan`]).
    #[serde(rename = "gtk_ini_unset_ifok")]
    GtkIniUnsetIfOk { path: String, key: String },
    /// Только при `ok`: удалить НАШ файл (переменные курсора в
    /// `environment.d`); файла нет — не ошибка.
    #[serde(rename = "remove_ifok")]
    RemoveIfOk { path: String },
    /// Файлы `paths` перепишут следующие команды плана (`kwriteconfig6
    /// --file …`, сценарий правки ini): снять копию «что было» под откатом
    /// нажатия, а заведённые командами — отметить после прогона. Так
    /// «Применить», которое пишет командами, оставляет снимок и запись
    /// `applied`, как и запись своими руками. На `ok` не влияет.
    #[serde(rename = "guard")]
    Guard { paths: Vec<String> },
    /// Прочитать обратно (`read`) то, что записали команды: `ok = true`,
    /// только если команда удалась и её ответ (без кавычек `gsettings`)
    /// равен `expect`; иначе `ok = false`. `kwriteconfig6` и `gsettings`
    /// умеют завершаться нулём, ничего не записав.
    #[serde(rename = "readback_setsok")]
    ReadBackSetsOk { read: Cmd, expect: String },
}

/// `settings.ini` GTK без строк ключа `key` (остальное — байт в байт).
/// `None` — ключа нет, переписывать файл незачем.
pub fn remove_gtk_settings_key(existing: &str, key: &str) -> Option<String> {
    let key_eq = format!("{key}=");
    let mut found = false;
    let kept: String = existing
        .split_inclusive('\n')
        .filter(|line| {
            let hit = line.trim().starts_with(&key_eq);
            found |= hit;
            !hit
        })
        .collect();
    found.then_some(kept)
}

/// Есть ли в ini-тексте ключ `key` с непустым значением в секции `section`
/// (со скобками, `"[Appearance]"`).
pub fn ini_has_key(text: &str, section: &str, key: &str) -> bool {
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            inside = t == section;
            continue;
        }
        if inside {
            if let Some((k, v)) = t.split_once('=') {
                if k.trim() == key && !v.trim().is_empty() {
                    return true;
                }
            }
        }
    }
    false
}

/// Решение шага [`ApplyStep::IniSetGuarded`]: новый текст файла или `None`
/// (ничего не писать). `existing` — текущий текст, `None` — файла нет.
pub fn ini_guarded_text(
    existing: Option<&str>,
    section: &str,
    key: &str,
    value: &str,
    only_if_file: bool,
    keep_existing: bool,
) -> Option<String> {
    if only_if_file && existing.is_none() {
        return None;
    }
    if keep_existing && existing.is_some_and(|t| ini_has_key(t, section, key)) {
        return None;
    }
    Some(crate::defaults::set_ini_value(existing, section, key, value))
}

/// Откуда прочитать тему значков, которую окружение отдаёт программам
/// сейчас (её получает новый конфиг qt6ct — см. [`KvantumEnv::icon_theme`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconThemeSource {
    /// Команда чтения: `gsettings get …` или `xfconf-query …`.
    Cmd(Cmd),
    /// `settings.ini` GTK, ключ `gtk-icon-theme-name`.
    GtkIni(String),
}

/// Источник текущей темы значков — те же ключи, в которые её пишет
/// [`apply_icon_plan_xdg`].
pub fn current_icon_theme_source(de: &str, home: &str, xdg_config_home: Option<&str>) -> IconThemeSource {
    match resolve_schemas(de) {
        s @ Schemas::Gsettings(_) => {
            if let Some((schema, key)) = s.role("icons") {
                return IconThemeSource::Cmd(cmd(&["gsettings", "get", schema, key], Some(5)));
            }
        }
        Schemas::NonGsettings if de == "xfce" => {
            return IconThemeSource::Cmd(cmd(
                &["xfconf-query", "-c", "xsettings", "-p", "/Net/IconThemeName"],
                Some(5),
            ));
        }
        Schemas::NonGsettings if de == "deepin" => {
            return IconThemeSource::Cmd(Cmd {
                argv: crate::dde::dconfig_get_argv(crate::dde::ICON_THEME),
                timeout: Some(5),
            });
        }
        Schemas::NonGsettings => {}
    }
    IconThemeSource::GtkIni(gtk_ini_path(home, xdg_config_home, "gtk-3.0"))
}

/// Роли, которые окружение со своим хранилищем оформления держит у себя:
/// по ним карта активных тем спрашивает само окружение
/// ([`native_role_read_cmd`]).
pub const NATIVE_STORE_ROLES: &[&str] = &["gtk", "icons", "cursor"];

/// Команда чтения роли `gtk`/`icons`/`cursor` из РОДНОГО хранилища окружения
/// — того же ключа, в который пишут планы применения
/// ([`apply_gtk_plan_xdg`], [`apply_icon_plan_xdg`], [`apply_cursor_plan_sized`]):
/// gsettings по таблице схем (GNOME, Cinnamon, MATE, Budgie, Pantheon, COSMIC),
/// xfconf у XFCE, DConfig у Deepin.
///
/// `None` — у окружения такого хранилища нет или оно не главное: KDE
/// (kdeglobals читает карта сама), LXQt и тайловые WM (их gsettings — лишь
/// подсказка GTK-программам, главное у них — `settings.ini`, конфиг
/// композитора и переменные окружения).
pub fn native_role_read_cmd(de: &str, role: &str) -> Option<Cmd> {
    if !NATIVE_STORE_ROLES.contains(&role) || is_generic_wm(de) || de == "lxqt" {
        return None;
    }
    match resolve_schemas(de) {
        s @ Schemas::Gsettings(_) => {
            let (schema, key) = s.role(role)?;
            Some(cmd(&["gsettings", "get", schema, key], Some(5)))
        }
        Schemas::NonGsettings if de == "xfce" => {
            let prop = match role {
                "gtk" => "/Net/ThemeName",
                "icons" => "/Net/IconThemeName",
                _ => "/Gtk/CursorThemeName",
            };
            Some(cmd(&["xfconf-query", "-c", "xsettings", "-p", prop], Some(5)))
        }
        Schemas::NonGsettings if de == "deepin" => {
            let key = match role {
                "gtk" => crate::dde::GTK_THEME,
                "icons" => crate::dde::ICON_THEME,
                _ => crate::dde::CURSOR_THEME,
            };
            Some(Cmd { argv: crate::dde::dconfig_get_argv(key), timeout: Some(5) })
        }
        Schemas::NonGsettings => None,
    }
}

/// Команда, которая назовёт СТАНДАРТНУЮ тему роли `role` окружения `de` —
/// ту, что стоит, пока человек ничего не выбирал: умолчание схемы вместе с
/// поправками дистрибутива (`GSETTINGS_BACKEND=memory` читает схему мимо
/// записанных значений). Так это значение и узнаётся, а не нашей таблицей
/// догадок: у Cinnamon на CachyOS значки по умолчанию — Papirus, а не
/// Adwaita из исходной схемы.
///
/// `None` — у окружения нет схемы gsettings для роли (KDE, XFCE, Deepin,
/// LXQt, тайловые WM): там стандартные темы (Breeze, elementary) не скрыты и
/// не попадают под защиту курсоров, особой обработки не нужно.
pub fn standard_theme_read_cmd(de: &str, role: &str) -> Option<Cmd> {
    if !NATIVE_STORE_ROLES.contains(&role) || is_generic_wm(de) || de == "lxqt" {
        return None;
    }
    let (schema, key) = resolve_schemas(de).role(role)?;
    Some(cmd(&["env", "GSETTINGS_BACKEND=memory", "gsettings", "get", schema, key], Some(5)))
}

/// Ответ [`native_role_read_cmd`] → имя темы: кавычки gsettings и подпись
/// типа `dde-dconfig` снимаются (`'Menta'\n` → `Menta`, `s "Papirus"` →
/// `Papirus`).
pub fn parse_native_role_output(de: &str, raw: &str) -> String {
    if de == "deepin" {
        crate::dde::parse_value(raw)
    } else {
        parse_icon_theme_output(raw)
    }
}

/// Ответ команды чтения → имя темы (`'Papirus'\n` → `Papirus`).
pub fn parse_icon_theme_output(raw: &str) -> String {
    raw.trim().trim_matches(|c| c == '\'' || c == '"').trim().to_string()
}

/// Конфиги переключателей стиля Qt, в которые уходит выбор значков:
/// без `icon_theme` Qt-программы под ними берут `hicolor`.
pub const QT_PLATFORM_THEME_TOOLS: &[&str] = &["qt6ct", "qt5ct"];

/// План применения темы: шаги + вид записи истории (`_add_history` kind).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<ApplyStep>,
    pub history_kind: &'static str,
}

/// План темы для Deepin: один стоп до записи ([`ApplyStep::DdeTreelandGate`]) —
/// в Treeland темы задаёт только Центр управления, другие сеансы Deepin
/// Rexon не поддерживает.
fn deepin_plan(history_kind: &'static str) -> Plan {
    Plan { steps: vec![ApplyStep::DdeTreelandGate], history_kind }
}

fn gtk_ini_path(home: &str, xdg_config_home: Option<&str>, gtk_ver: &str) -> String {
    format!(
        "{}/{gtk_ver}/settings.ini",
        crate::utils::config_home(xdg_config_home, home)
    )
}

fn gtk_ini_step(
    home: &str,
    xdg_config_home: Option<&str>,
    gtk_ver: &str,
    key: &str,
    value: &str,
) -> ApplyStep {
    ApplyStep::GtkIni {
        path: gtk_ini_path(home, xdg_config_home, gtk_ver),
        key: key.to_string(),
        value: value.to_string(),
    }
}

/// Правка ключа в конфиге LXQt (`~/.config/lxqt/<file>`) по месту — файл ведёт
/// LXQt, чужие ключи в нём сохраняются.
fn lxqt_ini_step(
    home: &str,
    xdg_config_home: Option<&str>,
    file: &str,
    section: &str,
    key: &str,
    value: &str,
) -> ApplyStep {
    ApplyStep::IniSetIfOk {
        path: format!("{}/lxqt/{file}", crate::utils::config_home(xdg_config_home, home)),
        section: section.to_string(),
        key: key.to_string(),
        value: value.to_string(),
    }
}

/// Порт `apply_gtk_theme`. `home` раскрывает `~`.
pub fn apply_gtk_plan(theme: &str, de: &str, home: &str) -> Plan {
    apply_gtk_plan_xdg(theme, de, home, None)
}

/// То же с учётом `$XDG_CONFIG_HOME` (пути `settings.ini` GTK).
pub fn apply_gtk_plan_xdg(
    theme: &str,
    de: &str,
    home: &str,
    xdg_config_home: Option<&str>,
) -> Plan {
    apply_gtk_plan_scheme(theme, de, home, xdg_config_home, None)
}

/// Схема и ключ светлого/тёмного режима программ: `org.gnome.desktop.
/// interface color-scheme` — его читают libadwaita и портал оформления
/// (`org.freedesktop.appearance color-scheme`). Ставится там, где тема GTK
/// пишется в ту же схему GNOME (GNOME, Budgie, Pantheon, COSMIC, тайловые
/// WM): тёмная тема GTK при светлом режиме оставляла программы libadwaita
/// светлыми (проверка GNOME, 16.10.2026). У Cinnamon, MATE, XFCE и KDE режим
/// ведёт само окружение (KDE — по цветовой схеме), его не трогаем.
pub const COLOR_SCHEME_KEY: (&str, &str) = ("org.gnome.desktop.interface", "color-scheme");

/// Значение [`COLOR_SCHEME_KEY`] для темы: тёмная — `prefer-dark`, светлая —
/// `default` (так и пишут «Настройки GNOME → Оформление»: «Светлый» — это
/// `default`, а не `prefer-light`).
pub fn color_scheme_value(dark: bool) -> &'static str {
    if dark { "prefer-dark" } else { "default" }
}

/// Pantheon: схема и ключ режима у службы io.elementary.settings-daemon —
/// его пишет «Параметры → Рабочий стол → Оформление» (switchboard-plug-
/// pantheon-shell, `Appearance.vala`). Служба раздаёт значение в
/// AccountsService (оттуда режим берут Gala, панель и программы elementary)
/// и в [`COLOR_SCHEME_KEY`]; запись одного ключа GNOME оставляла оболочку
/// светлой, а по расписанию тёмного режима служба возвращала его назад.
pub const PANTHEON_COLOR_SCHEME_KEY: (&str, &str) =
    ("io.elementary.settings-daemon.prefers-color-scheme", "color-scheme");

/// Значение [`PANTHEON_COLOR_SCHEME_KEY`]: «По умолчанию» (светлый) у
/// elementary — `no-preference`.
pub fn pantheon_color_scheme_value(dark: bool) -> &'static str {
    if dark { "prefer-dark" } else { "no-preference" }
}

/// Тёмная ли тема GTK. Первым — цвет фона окна из её таблицы стилей GTK 3
/// (`@define-color theme_bg_color …`, по ссылкам `@bg_color`/
/// `@window_bg_color` в том же файле) общим критерием продукта
/// [`crate::ui_base::is_dark_rgb`]; цвета в файле нет (стили лежат в
/// ресурсе GTK или подключаются `@import`) — по имени: «dark» в нём
/// (`Adwaita-dark`, `WhiteSur-Dark-nord`). Цвет важнее имени: у «Windows
/// XP Royale Dark» фон окна светлый.
pub fn gtk_theme_is_dark(name: &str, gtk_css: Option<&str>) -> bool {
    if let Some((r, g, b)) = gtk_css.and_then(gtk_css_window_bg) {
        return crate::ui_base::is_dark_rgb(r, g, b);
    }
    name.to_lowercase().contains("dark")
}

/// Цвет фона окна из `gtk.css`: `theme_bg_color`, ссылки `@имя` — по
/// `@define-color` того же файла (не глубже 8 шагов, от зацикливания).
fn gtk_css_window_bg(css: &str) -> Option<(u8, u8, u8)> {
    let mut colors: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for line in css.lines() {
        let Some(rest) = line.trim().strip_prefix("@define-color") else { continue };
        let mut parts = rest.trim().splitn(2, char::is_whitespace);
        let (Some(name), Some(value)) = (parts.next(), parts.next()) else { continue };
        let value = value.trim().trim_end_matches(';').trim();
        colors.entry(name).or_insert(value);
    }
    let mut value = *colors.get("theme_bg_color")?;
    for _ in 0..8 {
        match value.strip_prefix('@') {
            Some(alias) => value = colors.get(alias)?,
            None => return crate::ui_base::parse_hex(value),
        }
    }
    None
}

/// Как [`apply_gtk_plan_xdg`], и с режимом программ: `dark` — тёмная ли
/// тема ([`gtk_theme_is_dark`]); `None` — режим не трогать.
pub fn apply_gtk_plan_scheme(
    theme: &str,
    de: &str,
    home: &str,
    xdg_config_home: Option<&str>,
    dark: Option<bool>,
) -> Plan {
    if de == "deepin" {
        return deepin_plan("gtk");
    }
    let mut plan = apply_gtk_plan_steps(theme, de, home, xdg_config_home);
    let gnome_schema = resolve_schemas(de).role("gtk").is_some_and(|(s, _)| s == COLOR_SCHEME_KEY.0);
    if let (Some(dark), true) = (dark, gnome_schema) {
        // Только если тема записалась; на ok не влияет.
        plan.steps.push(ApplyStep::IfOk(cmd(
            &["gsettings", "set", COLOR_SCHEME_KEY.0, COLOR_SCHEME_KEY.1, color_scheme_value(dark)],
            None,
        )));
        if de == "pantheon" {
            // Схемы нет (служба elementary не стоит) — шаг просто не
            // удаётся; на ok он не влияет.
            plan.steps.push(ApplyStep::IfOk(cmd(
                &[
                    "gsettings", "set", PANTHEON_COLOR_SCHEME_KEY.0, PANTHEON_COLOR_SCHEME_KEY.1,
                    pantheon_color_scheme_value(dark),
                ],
                None,
            )));
        }
    }
    plan
}

fn apply_gtk_plan_steps(
    theme: &str,
    de: &str,
    home: &str,
    xdg_config_home: Option<&str>,
) -> Plan {
    let mut steps = Vec::new();
    match resolve_schemas(de) {
        s @ Schemas::Gsettings(_) => {
            if let Some((schema, key)) = s.role("gtk") {
                steps.push(ApplyStep::Primary(cmd(
                    &["gsettings", "set", schema, key, theme],
                    None,
                )));
            }
        }
        Schemas::NonGsettings => {
            if de == "xfce" {
                steps.push(ApplyStep::Primary(cmd(
                    &["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName", "-s", theme],
                    None,
                )));
            } else if de == "kde" {
                for v in ["gtk-3.0", "gtk-4.0"] {
                    steps.push(gtk_ini_step(home, xdg_config_home, v, "gtk-theme-name", theme));
                }
                steps.push(ApplyStep::BestEffort(cmd(
                    &["gsettings", "set", "org.gnome.desktop.interface", "gtk-theme", theme],
                    None,
                )));
                // Модуль Plasma `gtkconfig` (kded) раздаёт тему GTK запущенным
                // программам — через xsettingsd (X11/XWayland) и GSettings. О
                // ручной записи `settings.ini` он не узнаёт: живой kded6
                // оставлял `Net/ThemeName "Breeze"` до перезахода (Д4, проверка
                // окружений 02.10.2026). Тот же вызов делает страница «Стиль
                // приложений → GTK» Параметров системы. Нет kded — шаг просто
                // не удаётся, файлы выше уже записаны.
                steps.push(ApplyStep::BestEffort(cmd(
                    &[
                        "dbus-send", "--session", "--print-reply", "--dest=org.kde.GtkConfig",
                        "/GtkConfig", "org.kde.GtkConfig.setGtkTheme", &format!("string:{theme}"),
                    ],
                    Some(10),
                )));
                steps.push(ApplyStep::ForceOk);
            }
        }
    }
    // LXQt своего хранилища темы GTK не имеет: lxqt-config-appearance пишет
    // `settings.ini` gtk-3.0/gtk-4.0 (и тот же gsettings, что выше). Без
    // xsettingsd GTK-программы на LXQt читают именно файл — он и авторитетен.
    if is_generic_wm(de) || de == "lxqt" {
        for v in ["gtk-3.0", "gtk-4.0"] {
            steps.push(gtk_ini_step(home, xdg_config_home, v, "gtk-theme-name", theme));
        }
        steps.push(ApplyStep::ForceOk);
    }
    Plan { steps, history_kind: "gtk" }
}

/// Порт `apply_icon_theme`. `valid` — результат [`is_valid_icon_theme`]
/// (pre-flight): при `false` Python отказывает — здесь `None` (плана нет).
pub fn apply_icon_plan(theme: &str, de: &str, home: &str, valid: bool) -> Option<Plan> {
    apply_icon_plan_xdg(theme, de, home, None, valid)
}

/// То же с учётом `$XDG_CONFIG_HOME`.
pub fn apply_icon_plan_xdg(
    theme: &str,
    de: &str,
    home: &str,
    xdg_config_home: Option<&str>,
    valid: bool,
) -> Option<Plan> {
    if !valid {
        return None;
    }
    if de == "deepin" {
        return Some(deepin_plan("icons"));
    }
    let mut steps = Vec::new();
    match resolve_schemas(de) {
        s @ Schemas::Gsettings(_) => {
            if let Some((schema, key)) = s.role("icons") {
                steps.push(ApplyStep::Primary(cmd(
                    &["gsettings", "set", schema, key, theme],
                    None,
                )));
            }
        }
        Schemas::NonGsettings => {
            if de == "xfce" {
                steps.push(ApplyStep::Primary(cmd(
                    &["xfconf-query", "-c", "xsettings", "-p", "/Net/IconThemeName", "-s", theme],
                    None,
                )));
            } else if de == "kde" {
                // `--notify`: модуль Plasma `gtkconfig` и запущенные
                // KDE-программы следят за kdeglobals через KConfigWatcher, а
                // сигнал уходит только при записи с уведомлением. Без него
                // GTK-программы под Plasma получали новые значки лишь после
                // перезахода (Д4, проверка окружений 02.10.2026: с `--notify`
                // `org.gnome.desktop.interface icon-theme` сменился сразу).
                steps.push(ApplyStep::Primary(cmd(
                    &[
                        "kwriteconfig6", "--file", "kdeglobals", "--group", "Icons", "--key", "Theme",
                        "--notify", theme,
                    ],
                    None,
                )));
                steps.push(ApplyStep::IfOk(cmd(
                    &["dbus-send", "--type=signal", "/KIconLoader", "org.kde.KIconLoader.iconChanged", "int32:0"],
                    None,
                )));
            }
        }
    }
    if de == "cosmic" {
        // Панель, cosmic-files и прочие программы libcosmic берут значки не
        // из gsettings, а из своего ключа — туда же их пишет «Настройки
        // COSMIC → Оформление» (cosmic-settings `drawer.rs`:
        // `tk_config.set::<String>("icon_theme", …)`). Удалась запись —
        // значки сменились и у них, даже если gsettings выше не ответил.
        if let Some(step) = cosmic_icon_theme_step(theme, home, xdg_config_home) {
            steps.push(step);
        }
    }
    // На любом DE — gtk-3.0 и gtk-4.0 settings.ini.
    for v in ["gtk-3.0", "gtk-4.0"] {
        steps.push(gtk_ini_step(home, xdg_config_home, v, "gtk-icon-theme-name", theme));
    }
    // У тайловых и LXQt авторитетны файлы, а не gsettings: без dconf он
    // падает, и прежде `ForceOk` стоял ПОСЛЕ записей «если ok» — qt6ct и
    // kdeglobals ниже тогда молча пропускались. Поэтому — до них.
    if is_generic_wm(de) || de == "lxqt" {
        steps.push(ApplyStep::ForceOk);
    }
    if de == "lxqt" {
        // Qt-часть LXQt (панель, pcmanfm-qt, lxqt-*) берёт значки из своего
        // `lxqt.conf [General] icon_theme` — туда же пишет lxqt-config-appearance,
        // и LXQt подхватывает файл на лету. gsettings выше — только для GTK.
        steps.push(lxqt_ini_step(home, xdg_config_home, "lxqt.conf", "[General]", "icon_theme", theme));
    }
    let cfg = crate::utils::config_home(xdg_config_home, home);
    if de != "kde" {
        // Программы KDE (Dolphin, Kate, Okular — KIconThemes) и вне Plasma
        // берут набор значков из `kdeglobals [Icons] Theme`; без этой записи
        // они оставались на Breeze в любом окружении, кроме KDE. Тот же ключ
        // пишет Plasma, поэтому файл правится точечно, чужое не трогается.
        steps.push(ApplyStep::IniSetGuarded {
            path: format!("{cfg}/kdeglobals"),
            section: "[Icons]".into(),
            key: "Theme".into(),
            value: theme.into(),
            only_if_file: false,
            keep_existing: false,
        });
    }
    // Qt-программы под qt6ct/qt5ct берут значки из их конфига, а не из
    // окружения: без этой записи выбор значков до Qt не доходил (Д2,
    // проверка окружений 02.10.2026). Только если конфиг уже есть — как у
    // шрифтов: нет файла, значит переключатель не настроен.
    for tool in QT_PLATFORM_THEME_TOOLS {
        steps.push(ApplyStep::IniSetGuarded {
            path: format!("{cfg}/{tool}/{tool}.conf"),
            section: "[Appearance]".into(),
            key: "icon_theme".into(),
            value: theme.into(),
            only_if_file: true,
            keep_existing: false,
        });
    }
    Some(Plan { steps, history_kind: "icons" })
}

/// Ключ `cosmic-config` с темой значков libcosmic — путь от
/// `$XDG_CONFIG_HOME`. Сверено с libcosmic `src/config/mod.rs`
/// (`ID = "com.system76.CosmicTk"`, `#[version = 1]`, поле `icon_theme:
/// String`) и `cosmic-config` (`Config::new`: `<config>/cosmic/<ID>/v<версия>`,
/// один ключ — один файл).
pub const COSMIC_TK_ICON_THEME_REL: &str = "cosmic/com.system76.CosmicTk/v1/icon_theme";

/// Строка в записи RON — так её пишет `cosmic-config`
/// (`ron::ser::to_string_pretty`): в двойных кавычках, `\` и `"`
/// экранированы. Управляющие символы (перевод строки и т.п.) в имени темы
/// или пути — `None`: такого значения COSMIC не прочтёт как задумано, и
/// писать его не будем.
pub fn cosmic_ron_string(value: &str) -> Option<String> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    Some(format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
}

/// Шаг записи темы значков в ключ COSMIC ([`COSMIC_TK_ICON_THEME_REL`]).
fn cosmic_icon_theme_step(theme: &str, home: &str, xdg_config_home: Option<&str>) -> Option<ApplyStep> {
    let content = cosmic_ron_string(theme)?;
    let cfg = crate::utils::config_home(xdg_config_home, home);
    let path = format!("{cfg}/{COSMIC_TK_ICON_THEME_REL}");
    let dir = path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
    Some(ApplyStep::WriteSetsOk { dir, path, content })
}

/// Порт `apply_cursor_theme`. `safe` — результат [`cursor_preflight_ok`]
/// (санитайзер): при `false` Python отказывает — здесь `None`.
pub fn apply_cursor_plan(theme: &str, de: &str, home: &str, safe: bool) -> Option<Plan> {
    apply_cursor_plan_xdg(theme, de, home, None, safe)
}

/// То же с учётом `$XDG_CONFIG_HOME`. Размер курсора не трогается — см.
/// [`apply_cursor_plan_sized`].
pub fn apply_cursor_plan_xdg(
    theme: &str,
    de: &str,
    home: &str,
    xdg_config_home: Option<&str>,
    safe: bool,
) -> Option<Plan> {
    apply_cursor_plan_sized(theme, None, de, home, xdg_config_home, safe)
}

// ============================================================================
// Курсор во всех окружениях: тема + размер
// ============================================================================
//
// Кто что читает (сверено с первоисточниками 30.09.2026):
//   * KDE — `kcminputrc [Mouse] cursorTheme/cursorSize`, пишет их
//     `plasma-apply-cursortheme [--size N]` (справка установленного бинаря);
//   * GNOME/Budgie/Pantheon/COSMIC — `org.gnome.desktop.interface
//     cursor-theme/size` (Gala — тот же Mutter),
//     Cinnamon — `org.cinnamon.desktop.interface`, MATE —
//     `org.mate.peripherals-mouse`; XFCE — xsettings `/Gtk/CursorThemeName`
//     и `/Gtk/CursorThemeSize` (строки xfsettingsd); LXQt —
//     `session.conf [Mouse] cursor_theme/cursor_size` (читает lxqt-session при
//     входе);
//   * sway — `seat <name> xcursor_theme <theme> [<size>]` (sway-input(5));
//     seat0 заодно отдаёт XCURSOR_THEME/XCURSOR_SIZE своим детям и XWayland;
//     `include <path>` (sway(5)) — последний задавший побеждает;
//   * niri — `cursor { xcursor-theme "…"; xcursor-size N }` (вики niri,
//     Configuration: Miscellaneous), `include "file.kdl"` с 25.11
//     позиционный, разделы сливаются; конфиг перечитывается сам, курсор
//     меняется на лету (niri.rs: `cursor_manager.reload`);
//   * Hyprland — свой указатель при старте берёт из HYPRCURSOR_THEME/SIZE, а
//     если такой hyprcursor-темы нет — из XCURSOR_THEME/SIZE
//     (CursorManager.cpp v0.56.2); `hyprctl setcursor` меняет его в текущем
//     сеансе и для XCursor-тем тоже (откат на XCursor с тем же именем).
//     Конфиг 0.56 — `hyprland.lua`, если он есть, иначе старый
//     `hyprland.conf` (Jeremy.cpp v0.56.2); в Lua другие файлы подключаются
//     `require` (вики: Configuring → Core), в старом — `source =`;
//   * всё остальное (Qt без платформенной темы, Electron, XWayland-программы)
//     — XCURSOR_THEME/XCURSOR_SIZE, а без них тема `default`, то есть
//     `~/.icons/default/index.theme`. Переменные кладём в
//     `~/.config/environment.d` тем же механизмом, что у терминала и стиля Qt.

/// Размеры курсора на выбор — как в настройках KDE и GNOME.
pub const CURSOR_SIZES: &[u32] = &[24, 32, 40, 48, 64, 96];

/// Размер, когда его не задал никто.
pub const CURSOR_DEFAULT_SIZE: u32 = 24;

/// Файл переменных окружения Rexon для курсора (рядом с `rexon-qt-style.conf`).
pub const CURSOR_ENV_FILE: &str = "rexon-cursor.conf";

/// Метка управляемого блока Rexon в чужом конфиге композитора.
pub const CURSOR_BLOCK_TAG: &str = "cursor";

/// Тайловый композитор, у которого свой указатель задаётся в его конфиге.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorWm {
    Sway,
    Niri,
    Hyprland,
}

impl CursorWm {
    /// По идентификатору окружения (`system::detect_current_de`).
    pub fn from_de(de: &str) -> Option<Self> {
        match de.to_lowercase().as_str() {
            "sway" => Some(CursorWm::Sway),
            "niri" => Some(CursorWm::Niri),
            "hyprland" => Some(CursorWm::Hyprland),
            _ => None,
        }
    }

    /// Имя для человека.
    pub fn label(self) -> &'static str {
        match self {
            CursorWm::Sway => "sway",
            CursorWm::Niri => "niri",
            CursorWm::Hyprland => "Hyprland",
        }
    }
}

/// Синтаксис конфига композитора — от него зависят комментарий, строка
/// подключения нашего файла и сам наш файл.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WmSyntax {
    Sway,
    Kdl,
    HyprLua,
    HyprConf,
}

impl WmSyntax {
    /// Начало строчного комментария.
    pub fn comment(self) -> &'static str {
        match self {
            WmSyntax::Sway | WmSyntax::HyprConf => "#",
            WmSyntax::Kdl => "//",
            WmSyntax::HyprLua => "--",
        }
    }
}

/// Один возможный конфиг композитора и наш файл рядом с ним.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WmConfigFile {
    /// Конфиг композитора (его ведёт человек; мы добавляем одну строку).
    pub path: String,
    pub syntax: WmSyntax,
    /// Наш файл (его ведём мы и переписываем целиком).
    pub own_path: String,
    pub own_content: String,
    /// Строка подключения нашего файла — единственное, что попадает в
    /// конфиг человека (внутри управляемого блока).
    pub include_line: String,
}

/// Запись курсора в конфиг тайлового композитора — что делает исполнитель
/// [`ApplyStep::WmConfig`]:
///   1. `min_version`: есть бинарь и он старше — ничего не трогаем
///      (`include` у niri появился в 25.11, старый niri конфиг с ним не
///      прочитает);
///   2. берётся первый СУЩЕСТВУЮЩИЙ файл из `candidates` (порядок поиска
///      самого композитора). Нет ни одного: если есть файл из `foreign`
///      (sway читает конфиг i3, а i3 не знает команды `seat`) — не трогаем;
///      иначе, если есть `seed` (системный конфиг sway), он копируется на
///      место личного — так рекомендует сам sway, поведение не меняется;
///      иначе — личного конфига нет, композитор создаст его сам при первом
///      запуске, а курсор до тех пор придёт через `~/.icons/default`;
///   3. пишется наш файл, затем в конфиг — управляемый блок
///      ([`upsert_managed_block`]); до первой правки рядом кладётся копия
///      `<конфиг>.rexon-bak`;
///   4. `validate` (argv, путь конфига дописывается в конец): если конфиг
///      был исправен, а после правки нет — исходный текст возвращается.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WmConfigPlan {
    pub wm: CursorWm,
    pub candidates: Vec<WmConfigFile>,
    pub foreign: Vec<String>,
    pub seed: Option<WmSeed>,
    pub validate: Vec<String>,
    pub min_version: Option<WmVersionGate>,
}

/// Чем засеять отсутствующий личный конфиг.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WmSeed {
    pub from: String,
    /// Индекс в `candidates`, на место которого ложится копия.
    pub candidate: usize,
}

/// Минимальная версия композитора: argv спроса версии и порог (год, месяц).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WmVersionGate {
    pub argv: Vec<String>,
    pub min: (u32, u32),
}

/// Чем кончилась запись в конфиг композитора.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WmConfigOutcome {
    /// Блок на месте (записан сейчас или уже был).
    Written,
    /// Личного конфига нет — композитор заведёт его сам при первом запуске.
    NoConfig,
    /// Конфиг чужой (sway на конфиге i3) — не трогали.
    Foreign,
    /// Композитор слишком старый для `include`.
    TooOld,
    /// Записать не вышло (права, диск, ссылка в только-читаемое хранилище).
    Failed,
    /// Записали, проверка композитора конфиг отвергла — вернули как было.
    RolledBack,
}

/// Итог исполнения плана курсора: общий `ok` плюс то, что нужно, чтобы
/// честно сказать человеку, где и когда курсор сменится.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CursorReport {
    pub ok: bool,
    /// Удалась ли команда «сменить сейчас» (`swaymsg`, `hyprctl setcursor`);
    /// `None` — такой команды в плане не было.
    pub live: Option<bool>,
    /// Запись в конфиг композитора: композитор, итог, путь конфига.
    pub wm: Option<(CursorWm, WmConfigOutcome, String)>,
    /// Почему не применилось, если причина известна точно (Deepin:
    /// [`crate::dde::refusal`]). Её и показываем вместо общего «не применено».
    pub reason: Option<String>,
}

/// Можно ли вписать имя темы в конфиг композитора как есть. Имена тем — это
/// имена каталогов; кавычки, обратная косая, запятая (разделитель `env =` у
/// Hyprland) и управляющие символы в них не встречаются, а встретятся — в
/// конфиг человека мы такое не понесём.
pub fn wm_safe_theme_name(theme: &str) -> bool {
    !theme.is_empty()
        && !theme.starts_with('-')
        && !theme.chars().any(|c| c.is_control() || matches!(c, '"' | '\\' | ',' | '\'' | '$' | '`'))
}

/// Имя темы в команде sway: в кавычках, если в нём есть пробел.
fn sway_quote(theme: &str) -> String {
    if theme.contains(' ') {
        format!("\"{theme}\"")
    } else {
        theme.to_string()
    }
}

/// Путь в `include` sway. Аргумент проходит у sway три обработки: с него
/// снимаются ВСЕ неэкранированные кавычки, потом снимается экранирование
/// `\`, `$имя` подменяется переменной (`$$` — сам `$`), и только после этого
/// путь уходит в wordexp, который режет по пробелам. Поэтому кавычки для
/// wordexp пишутся экранированными (`\'`), а весь аргумент — в двойных
/// кавычках, чтобы sway не разрезал его раньше. Проверено `sway --validate`
/// (1.12) на пути с пробелом, апострофом и `$`.
pub fn sway_path(path: &str) -> String {
    // Буквы любого алфавита (`~/Загрузки`) ни sway, ни wordexp не трогают.
    if path.chars().all(|c| c.is_alphanumeric() || "/._-+".contains(c)) {
        return path.to_string();
    }
    let words = format!("'{}'", path.replace('\'', "'\\''"));
    let escaped = words
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('"', "\\\"")
        .replace('$', "$$");
    format!("\"{escaped}\"")
}

/// Наш файл и строка подключения для конфига композитора.
fn wm_config_file(
    syntax: WmSyntax,
    path: String,
    own_path: String,
    theme: &str,
    size: Option<u32>,
) -> WmConfigFile {
    let head = format!(
        "{} Rexon: cursor theme. This file is rewritten by Rexon; set the cursor in Rexon instead.\n",
        syntax.comment()
    );
    let (own_content, include_line) = match syntax {
        WmSyntax::Sway => {
            let size = size.map(|s| format!(" {s}")).unwrap_or_default();
            (
                format!("{head}seat * xcursor_theme {}{size}\n", sway_quote(theme)),
                format!("include {}", sway_path(&own_path)),
            )
        }
        WmSyntax::Kdl => {
            let size = size.map(|s| format!("    xcursor-size {s}\n")).unwrap_or_default();
            let name = own_path.rsplit('/').next().unwrap_or(&own_path).to_string();
            (
                format!("{head}cursor {{\n    xcursor-theme \"{theme}\"\n{size}}}\n"),
                // Относительно самого config.kdl: наш файл лежит рядом.
                format!("include \"{name}\""),
            )
        }
        WmSyntax::HyprLua => {
            let mut body = format!(
                "{head}hl.env(\"XCURSOR_THEME\", \"{theme}\")\nhl.env(\"HYPRCURSOR_THEME\", \"{theme}\")\n"
            );
            if let Some(s) = size {
                body.push_str(&format!(
                    "hl.env(\"XCURSOR_SIZE\", \"{s}\")\nhl.env(\"HYPRCURSOR_SIZE\", \"{s}\")\n"
                ));
            }
            // `pcall`: пропади наш файл — конфиг человека грузится дальше
            // (голый `require` несуществующего модуля обрывает весь конфиг).
            (body, "pcall(require, \"rexon-cursor\")".to_string())
        }
        WmSyntax::HyprConf => {
            let mut body =
                format!("{head}env = XCURSOR_THEME,{theme}\nenv = HYPRCURSOR_THEME,{theme}\n");
            if let Some(s) = size {
                body.push_str(&format!("env = XCURSOR_SIZE,{s}\nenv = HYPRCURSOR_SIZE,{s}\n"));
            }
            (body, format!("source = {own_path}"))
        }
    };
    WmConfigFile { path, syntax, own_path, own_content, include_line }
}

/// План записи курсора в конфиг композитора (см. [`WmConfigPlan`]).
pub fn wm_cursor_config_plan(
    wm: CursorWm,
    theme: &str,
    size: Option<u32>,
    home: &str,
    xdg_config_home: Option<&str>,
) -> WmConfigPlan {
    let cfg = crate::utils::config_home(xdg_config_home, home);
    let home = home.trim_end_matches('/');
    let file = |syntax, path: String, own: String| wm_config_file(syntax, path, own, theme, size);
    match wm {
        CursorWm::Sway => WmConfigPlan {
            wm,
            // Порядок поиска sway(1): ~/.sway/config, $XDG_CONFIG_HOME/sway/config.
            candidates: vec![
                file(WmSyntax::Sway, format!("{home}/.sway/config"), format!("{home}/.sway/rexon-cursor.conf")),
                file(WmSyntax::Sway, format!("{cfg}/sway/config"), format!("{cfg}/sway/rexon-cursor.conf")),
            ],
            foreign: vec![format!("{home}/.i3/config"), format!("{cfg}/i3/config")],
            seed: Some(WmSeed { from: "/etc/sway/config".into(), candidate: 1 }),
            validate: vec!["sway".into(), "--validate".into(), "--config".into()],
            min_version: None,
        },
        CursorWm::Niri => WmConfigPlan {
            wm,
            candidates: vec![file(
                WmSyntax::Kdl,
                format!("{cfg}/niri/config.kdl"),
                format!("{cfg}/niri/rexon-cursor.kdl"),
            )],
            foreign: Vec::new(),
            seed: None,
            validate: vec!["niri".into(), "validate".into(), "--config".into()],
            min_version: Some(WmVersionGate {
                argv: vec!["niri".into(), "--version".into()],
                min: (25, 11),
            }),
        },
        CursorWm::Hyprland => WmConfigPlan {
            wm,
            // Hyprland 0.56: hyprland.lua, если есть, иначе hyprland.conf.
            candidates: vec![
                file(WmSyntax::HyprLua, format!("{cfg}/hypr/hyprland.lua"), format!("{cfg}/hypr/rexon-cursor.lua")),
                file(WmSyntax::HyprConf, format!("{cfg}/hypr/hyprland.conf"), format!("{cfg}/hypr/rexon-cursor.conf")),
            ],
            foreign: Vec::new(),
            seed: None,
            validate: vec!["Hyprland".into(), "--verify-config".into(), "--config".into()],
            min_version: None,
        },
    }
}

/// Версия «год.месяц» из вывода `niri --version` (`niri 26.04 (8ed0da4)`).
pub fn parse_wm_version(out: &str) -> Option<(u32, u32)> {
    let word = out.split_whitespace().find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let mut parts = word.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()
        .map(|m| m.chars().take_while(char::is_ascii_digit).collect::<String>())
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    Some((major, minor))
}

fn block_markers(comment: &str, tag: &str) -> (String, String) {
    (
        format!("{comment} >>> Rexon: {tag} (managed by Rexon, do not edit inside) >>>"),
        format!("{comment} <<< Rexon: {tag} <<<"),
    )
}

/// Вставить или обновить управляемый блок Rexon в чужом конфиге.
///
/// Блок стоит в КОНЦЕ файла: у sway, niri и Hyprland побеждает последнее
/// заданное значение, а всё, что человек написал до него, остаётся как было.
/// Уже стоящий блок переписывается на месте (повторное применение не плодит
/// строк). Всё вне маркеров не меняется ни на байт.
pub fn upsert_managed_block(existing: &str, comment: &str, tag: &str, body: &str) -> String {
    let (begin, end) = block_markers(comment, tag);
    let block = format!("{begin}\n{}\n{end}\n", body.trim_end());
    if let (Some(b), Some(e)) = (existing.find(&begin), existing.find(&end)) {
        if b < e {
            let after = &existing[e + end.len()..];
            let after = after.strip_prefix('\n').unwrap_or(after);
            return format!("{}{block}{after}", &existing[..b]);
        }
    }
    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&block);
    out
}

/// Откат: убрать управляемый блок Rexon (вместе с пустой строкой, которую
/// добавил [`upsert_managed_block`]). Нет блока — текст как есть.
pub fn remove_managed_block(existing: &str, comment: &str, tag: &str) -> String {
    let (begin, end) = block_markers(comment, tag);
    let (Some(b), Some(e)) = (existing.find(&begin), existing.find(&end)) else {
        return existing.to_string();
    };
    if b > e {
        return existing.to_string();
    }
    let before = &existing[..b];
    let before = before.strip_suffix("\n\n").map(|s| format!("{s}\n")).unwrap_or_else(|| before.to_string());
    let after = &existing[e + end.len()..];
    let after = after.strip_prefix('\n').unwrap_or(after);
    format!("{before}{after}")
}

/// Содержимое `environment.d/rexon-cursor.conf`.
pub fn cursor_env_content(theme: &str, size: Option<u32>) -> String {
    let mut s = format!("XCURSOR_THEME={theme}\n");
    if let Some(n) = size {
        s.push_str(&format!("XCURSOR_SIZE={n}\n"));
    }
    s
}

/// План применения курсора: тема и (если задан) размер — во всех окружениях.
///
/// `size == None` — размер не трогается (так зовёт старый путь Python).
///
/// Где `ok` берётся из родного ключа окружения (KDE, gsettings, xfconf),
/// там он им и остаётся. У тайловых композиторов и прочих окружений без
/// своей настройки курсора прежний безусловный `ForceOk` убран: `ok` даёт
/// запись НАШЕГО файла переменных окружения (он и есть настоящий эффект —
/// после перезахода), а что сменилось сразу и где, исполнитель собирает в
/// [`CursorReport`] для честного сообщения ([`cursor_apply_message`]).
pub fn apply_cursor_plan_sized(
    theme: &str,
    size: Option<u32>,
    de: &str,
    home: &str,
    xdg_config_home: Option<&str>,
    safe: bool,
) -> Option<Plan> {
    if !safe {
        return None;
    }
    if de == "deepin" {
        return Some(deepin_plan("cursor"));
    }
    let cfg = crate::utils::config_home(xdg_config_home, home);
    let size_s = size.map(|n| n.to_string());
    let generic = is_generic_wm(de);
    let mut steps = Vec::new();
    match resolve_schemas(de) {
        s @ Schemas::Gsettings(_) => {
            if let Some((schema, key)) = s.role("cursor") {
                let set_theme = cmd(&["gsettings", "set", schema, key, theme], None);
                // У тайловых gsettings — лишь подсказка GTK-программам через
                // портал; без dconf он падает, а курсор композитора от этого
                // не зависит. Решает не он.
                steps.push(if generic { ApplyStep::BestEffort(set_theme) } else { ApplyStep::Primary(set_theme) });
                if let Some(n) = &size_s {
                    // Размер — в той же схеме, что тема (у MATE это
                    // `org.mate.peripherals-mouse`, а не interface).
                    steps.push(ApplyStep::BestEffort(cmd(&["gsettings", "set", schema, "cursor-size", n], None)));
                }
            }
        }
        Schemas::NonGsettings => {
            if de == "xfce" {
                steps.push(ApplyStep::Primary(cmd(
                    &["xfconf-query", "-c", "xsettings", "-p", "/Gtk/CursorThemeName", "-s", theme],
                    None,
                )));
                if let Some(n) = &size_s {
                    // `-n -t int`: свойства может ещё не быть (размер не меняли).
                    steps.push(ApplyStep::BestEffort(cmd(
                        &["xfconf-query", "-c", "xsettings", "-p", "/Gtk/CursorThemeSize", "-n", "-t", "int", "-s", n],
                        None,
                    )));
                }
            } else if de == "kde" {
                let primary = match &size_s {
                    Some(n) => cmd(&["plasma-apply-cursortheme", "--size", n, theme], None),
                    None => cmd(&["plasma-apply-cursortheme", theme], None),
                };
                steps.push(ApplyStep::Primary(primary));
                steps.push(ApplyStep::IfNotOk(cmd(
                    &["kwriteconfig6", "--file", "kcminputrc", "--group", "Mouse", "--key", "cursorTheme", theme],
                    None,
                )));
                steps.push(ApplyStep::IfNotOk(cmd(
                    &["kwriteconfig6", "--file", "kdeglobals", "--group", "Mouse", "--key", "cursorTheme", theme],
                    None,
                )));
                if let Some(n) = &size_s {
                    steps.push(ApplyStep::IfOk(cmd(
                        &["kwriteconfig6", "--file", "kcminputrc", "--group", "Mouse", "--key", "cursorSize", n],
                        None,
                    )));
                }
            }
        }
    }
    // На любом DE — gtk-3.0 и gtk-4.0 settings.ini.
    for v in ["gtk-3.0", "gtk-4.0"] {
        steps.push(gtk_ini_step(home, xdg_config_home, v, "gtk-cursor-theme-name", theme));
        if let Some(n) = &size_s {
            steps.push(gtk_ini_step(home, xdg_config_home, v, "gtk-cursor-theme-size", n));
        }
    }
    if de == "lxqt" {
        // Родной ключ LXQt — `session.conf [Mouse] cursor_theme` (его пишут
        // lxqt-config-appearance и lxqt-config-input, читает lxqt-session при
        // входе): курсор всего сеанса сменится после перезахода.
        steps.push(ApplyStep::ForceOk);
        steps.push(lxqt_ini_step(home, xdg_config_home, "session.conf", "[Mouse]", "cursor_theme", theme));
        if let Some(n) = &size_s {
            steps.push(lxqt_ini_step(home, xdg_config_home, "session.conf", "[Mouse]", "cursor_size", n));
        }
    }
    // Переменные окружения — всем, кроме KDE: Plasma при входе сама ставит
    // XCURSOR_* из kcminputrc, наш файл там был бы вторым, отстающим
    // источником.
    if de != "kde" {
        let env = (
            format!("{cfg}/environment.d"),
            format!("{cfg}/environment.d/{CURSOR_ENV_FILE}"),
            cursor_env_content(theme, size),
        );
        steps.push(if generic {
            ApplyStep::WriteSetsOk { dir: env.0, path: env.1, content: env.2 }
        } else {
            ApplyStep::WriteIfOk { dir: env.0, path: env.1, content: env.2 }
        });
    }
    if de.eq_ignore_ascii_case("river") {
        // river: своя команда меняет указатель сеанса сейчас и отдаёт
        // XCURSOR_THEME/XCURSOR_SIZE программам, которые он запустит дальше
        // (riverctl(1), `xcursor-theme <тема> [размер]`). При следующем входе
        // river без своей строки в `init` грузит тему `default` — это
        // `~/.icons/default` ниже.
        let mut argv = vec!["riverctl", "xcursor-theme", theme];
        if let Some(n) = &size_s {
            argv.push(n);
        }
        steps.push(ApplyStep::Live(cmd(&argv, Some(5))));
    }
    if de.eq_ignore_ascii_case("wayfire") {
        // Wayfire: родной ключ `[input] cursor_theme`/`cursor_size`; конфиг он
        // перечитывает сам и тут же грузит курсор заново (`config_reloaded` →
        // `init_xcursor` в `seat/cursor.cpp`). Только в существующий конфиг:
        // без него Wayfire берёт тему `default`, то есть `~/.icons/default`.
        // Какой из двух файлов он читает, решает наличие `wayfire/wayfire.ini`
        // — правится тот, что есть.
        for path in [format!("{cfg}/wayfire/wayfire.ini"), format!("{cfg}/wayfire.ini")] {
            let mut keys = vec![("cursor_theme", theme.to_string())];
            if let Some(n) = &size_s {
                keys.push(("cursor_size", n.clone()));
            }
            for (key, value) in keys {
                steps.push(ApplyStep::IniSetGuarded {
                    path: path.clone(),
                    section: "[input]".into(),
                    key: key.into(),
                    value,
                    only_if_file: true,
                    keep_existing: false,
                });
            }
        }
    }
    if let Some(wm) = CursorWm::from_de(de) {
        // Сейчас — в работающем сеансе.
        match wm {
            CursorWm::Sway => {
                let mut argv = vec!["swaymsg", "seat", "*", "xcursor_theme"];
                let quoted = sway_quote(theme);
                argv.push(&quoted);
                if let Some(n) = &size_s {
                    argv.push(n);
                }
                steps.push(ApplyStep::Live(cmd(&argv, Some(5))));
            }
            CursorWm::Hyprland => {
                steps.push(ApplyStep::HyprctlCursor { theme: theme.to_string(), size: size_s.clone() });
            }
            // niri перечитывает конфиг сам — отдельной команды не нужно. С
            // 25.11 (с тех пор, как есть `include`) его сторож раз в 0,5 с
            // сверяет время изменения и ВКЛЮЧЁННЫХ файлов тоже, поэтому
            // переписанного `rexon-cursor.kdl` без касания config.kdl хватает
            // (проверено по исходникам 25.11 и 26.04, `src/utils/watcher.rs`).
            CursorWm::Niri => {}
        }
        // И навсегда — в конфиге композитора.
        if wm_safe_theme_name(theme) {
            steps.push(ApplyStep::WmConfig(wm_cursor_config_plan(wm, theme, size, home, xdg_config_home)));
        }
    }
    // Финал: при ok — ~/.icons/default/index.theme (тема `default`, её берут
    // все, кому никто не сказал другого: X11, XWayland, композитор без
    // своей настройки).
    let dir = format!("{home}/.icons/default");
    steps.push(ApplyStep::WriteIfOk {
        path: format!("{dir}/index.theme"),
        content: format!("[Icon Theme]\nInherits={theme}\n"),
        dir,
    });
    Some(Plan { steps, history_kind: "cursor" })
}

/// Вернуть стандартный курсор окружения — тот, что назвала
/// [`standard_theme_read_cmd`]. Нужен для тем из [`UNSAFE_CURSOR_THEMES`]:
/// Adwaita — курсор по умолчанию в GNOME, Budgie, Cinnamon, и отказ «тема
/// небезопасна» оставлял человека без пути назад (проверки GNOME и
/// Cinnamon, 16.10.2026).
///
/// Защита из-за которой список вводили, сохранена: имя небезопасной темы
/// нигде не пишется явно. Родной ключ окружения СБРАСЫВАЕТСЯ к умолчанию
/// схемы (`gsettings reset` — его ставит само окружение и с ним живёт), а
/// наши прежние явные указания убираются: `gtk-cursor-theme-name` из
/// `settings.ini` GTK (GTK берёт курсор окружения) и наш файл переменных
/// `environment.d` (XCURSOR_* ставит само окружение). `~/.icons/default`
/// не трогается: его мог положить не Rexon, а GTK и окружение берут курсор
/// из своего ключа. `None` — у окружения нет родного ключа курсора в
/// gsettings (KDE, XFCE, тайловые WM): там Adwaita стандартом не является,
/// и прежний отказ остаётся.
pub fn standard_cursor_plan(
    de: &str,
    size: Option<u32>,
    home: &str,
    xdg_config_home: Option<&str>,
) -> Option<Plan> {
    if is_generic_wm(de) {
        return None;
    }
    let s @ Schemas::Gsettings(_) = resolve_schemas(de) else { return None };
    let (schema, key) = s.role("cursor")?;
    let mut steps = vec![ApplyStep::Primary(cmd(&["gsettings", "reset", schema, key], None))];
    if let Some(n) = size {
        steps.push(ApplyStep::BestEffort(cmd(&["gsettings", "set", schema, "cursor-size", &n.to_string()], None)));
    }
    for v in ["gtk-3.0", "gtk-4.0"] {
        steps.push(ApplyStep::GtkIniUnsetIfOk {
            path: gtk_ini_path(home, xdg_config_home, v),
            key: "gtk-cursor-theme-name".into(),
        });
    }
    let cfg = crate::utils::config_home(xdg_config_home, home);
    steps.push(ApplyStep::RemoveIfOk { path: format!("{cfg}/environment.d/{CURSOR_ENV_FILE}") });
    Some(Plan { steps, history_kind: "cursor" })
}

/// Что сказать человеку после применения курсора — правду о том, где он уже
/// сменился, а где сменится после перезахода.
pub fn cursor_apply_message(de: &str, report: &CursorReport) -> String {
    if let (false, Some(reason)) = (report.ok, &report.reason) {
        return reason.clone();
    }
    if !report.ok {
        return tr("Cursor not applied (the theme may be unsafe)");
    }
    if let Some((wm, outcome, path)) = &report.wm {
        let name = wm.label();
        let live = report.live == Some(true);
        return match outcome {
            WmConfigOutcome::Written if *wm == CursorWm::Niri => trf(
                // TRANSLATORS: %1 is the compositor name (niri).
                "Cursor applied and saved in the %1 config — %1 picks it up on its own.",
                &[&name],
            ),
            WmConfigOutcome::Written if live => trf(
                // TRANSLATORS: %1 is the compositor name (sway, Hyprland).
                "Cursor applied and saved in the %1 config.",
                &[&name],
            ),
            WmConfigOutcome::Written => trf(
                // TRANSLATORS: %1 is the compositor name (sway, Hyprland).
                "Cursor saved in the %1 config. It shows up the next time you start %1.",
                &[&name],
            ),
            WmConfigOutcome::NoConfig => trf(
                // TRANSLATORS: %1 is the compositor name.
                "Cursor applied. %1 has no config of its own yet, so its pointer \
                 changes after you log in again.",
                &[&name],
            ),
            WmConfigOutcome::Foreign => trf(
                // TRANSLATORS: %1 is the compositor name (sway), %2 a file path.
                "Cursor applied for apps. %1 reads the i3 config (%2), which Rexon \
                 doesn't change — the pointer of %1 itself changes after you log in \
                 again only if that config doesn't set its own cursor.",
                &[&name, &path],
            ),
            WmConfigOutcome::TooOld => trf(
                // TRANSLATORS: %1 is the compositor name (niri).
                "Cursor applied for apps. This %1 version can't include extra config \
                 files — update %1 to change its own pointer from Rexon.",
                &[&name],
            ),
            WmConfigOutcome::Failed | WmConfigOutcome::RolledBack => trf(
                // TRANSLATORS: %1 is the compositor name, %2 its config file path.
                "Cursor applied for apps, but Rexon couldn't save it in the %1 config \
                 (%2) — the config was left as it was. The pointer of %1 itself \
                 changes after you log in again only if that config doesn't set \
                 its own cursor.",
                &[&name, &path],
            ),
        };
    }
    match de.to_lowercase().as_str() {
        "kde" => tr("Cursor applied"),
        "lxqt" => tr("Cursor applied. LXQt switches the whole session to it after you log in again."),
        // Живая команда композитора удалась (river: `riverctl xcursor-theme`)
        // — сменился и его собственный указатель.
        d if is_generic_wm(d) && report.live == Some(true) => {
            tr("Cursor applied. Some Qt and Electron apps pick it up after you log in again.")
        }
        // COSMIC: своей настройки указателя нет, но cosmic-comp при входе
        // берёт XCURSOR_THEME, а без неё — тему `default` (`~/.icons/default`),
        // и то и другое Rexon уже записал.
        d if d == "cosmic" || is_generic_wm(d) => tr(
            "Cursor applied for apps. The compositor's own pointer and some Qt and \
             Electron apps change after you log in again.",
        ),
        _ => tr("Cursor applied. Some Qt and Electron apps pick it up after you log in again."),
    }
}

/// Откуда прочитать текущую тему или размер курсора — по порядку, первое
/// непустое.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorQuery {
    /// Команда; ответ — её вывод без кавычек и пробелов.
    Cmd(Cmd),
    /// Ключ ini-файла (`section` без скобок).
    Ini { path: String, section: String, key: String },
    /// Переменная из файла `environment.d`.
    EnvFile { path: String, key: String },
    /// Переменная окружения процесса.
    Env(String),
}

/// Где взять текущую тему (`size == false`) или размер (`true`) курсора.
pub fn cursor_queries(de: &str, home: &str, xdg_config_home: Option<&str>, size: bool) -> Vec<CursorQuery> {
    let cfg = crate::utils::config_home(xdg_config_home, home);
    let ini = |path: String, section: &str, key: &str| CursorQuery::Ini {
        path,
        section: section.into(),
        key: key.into(),
    };
    let env_file = CursorQuery::EnvFile {
        path: format!("{cfg}/environment.d/{CURSOR_ENV_FILE}"),
        key: if size { "XCURSOR_SIZE" } else { "XCURSOR_THEME" }.into(),
    };
    let env = CursorQuery::Env(if size { "XCURSOR_SIZE" } else { "XCURSOR_THEME" }.into());
    let gs = |schema: &str| {
        CursorQuery::Cmd(cmd(
            &["gsettings", "get", schema, if size { "cursor-size" } else { "cursor-theme" }],
            Some(5),
        ))
    };
    let mut out = Vec::new();
    match resolve_schemas(de) {
        Schemas::NonGsettings if de == "kde" => {
            out.push(ini(format!("{cfg}/kcminputrc"), "Mouse", if size { "cursorSize" } else { "cursorTheme" }));
            out.push(env);
            return out;
        }
        Schemas::NonGsettings if de == "deepin" => {
            let key = if size { crate::dde::CURSOR_SIZE } else { crate::dde::CURSOR_THEME };
            out.push(CursorQuery::Cmd(Cmd { argv: crate::dde::dconfig_get_argv(key), timeout: Some(5) }));
            // Служба оформления пишет курсор и в settings.ini GTK.
            out.push(ini(
                gtk_ini_path(home, xdg_config_home, "gtk-3.0"),
                "Settings",
                if size { "gtk-cursor-theme-size" } else { "gtk-cursor-theme-name" },
            ));
        }
        Schemas::NonGsettings => {
            out.push(CursorQuery::Cmd(cmd(
                &[
                    "xfconf-query", "-c", "xsettings", "-p",
                    if size { "/Gtk/CursorThemeSize" } else { "/Gtk/CursorThemeName" },
                ],
                Some(5),
            )));
        }
        s @ Schemas::Gsettings(_) => {
            if de == "lxqt" {
                out.push(ini(format!("{cfg}/lxqt/session.conf"), "Mouse", if size { "cursor_size" } else { "cursor_theme" }));
            }
            if is_generic_wm(de) {
                // Последний выбор, сделанный в Rexon, — он же в конфиге
                // композитора.
                out.push(env_file.clone());
            }
            if let Some((schema, _)) = s.role("cursor") {
                out.push(gs(schema));
            }
        }
    }
    out.push(env_file);
    out.push(env);
    if !size {
        out.push(ini(format!("{}/.icons/default/index.theme", home.trim_end_matches('/')), "Icon Theme", "Inherits"));
    }
    out
}

/// Значение из ответа источника: без кавычек, пробелов и типа gsettings
/// (`uint32 24`). Пусто — `None`.
pub fn clean_cursor_value(raw: &str) -> Option<String> {
    let v = raw.trim();
    let v = v.strip_prefix("uint32 ").or_else(|| v.strip_prefix("int32 ")).unwrap_or(v);
    let v = v.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

/// Значение ключа ini-файла в секции (`section` без скобок).
pub fn ini_key_value(content: &str, section: &str, key: &str) -> Option<String> {
    let mut here = false;
    for line in content.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            here = &s[1..s.len() - 1] == section;
            continue;
        }
        if !here || s.starts_with('#') || s.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = s.split_once('=') {
            if k.trim() == key {
                return clean_cursor_value(v);
            }
        }
    }
    None
}

/// Значение переменной из файла `environment.d` (`ИМЯ=значение`).
pub fn env_file_key_value(content: &str, key: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
        .and_then(clean_cursor_value)
}

/// Размер курсора из ответа источника — только разумные числа.
pub fn parse_cursor_size(raw: &str) -> Option<u32> {
    clean_cursor_value(raw)?.parse::<u32>().ok().filter(|n| (8..=512).contains(n))
}

/// argv двух `hyprctl`-вызовов при известном размере — раскрытие
/// [`ApplyStep::HyprctlCursor`] (для сверки; сам `gsettings get` cursor-size
/// исполнитель делает отдельно, когда размер в плане не задан).
pub fn hyprctl_cursor_cmds(theme: &str, size: &str) -> [Cmd; 2] {
    [
        cmd(&["hyprctl", "setcursor", theme, size], None),
        cmd(&["hyprctl", "keyword", "env", &format!("XCURSOR_THEME,{theme}")], None),
    ]
}

/// argv чтения размера курсора — часть [`ApplyStep::HyprctlCursor`].
pub fn cursor_size_get_cmd() -> Cmd {
    cmd(&["gsettings", "get", "org.gnome.desktop.interface", "cursor-size"], None)
}

// ============================================================================
// update_gtk_settings_ini — чистая read-modify-write трансформация
// ============================================================================

/// Порт `_update_gtk_settings_ini`. `existing` — содержимое файла (None =
/// файла нет). КВИРК оригинала сохранён: строка `{key}=` заменяется в ЛЮБОЙ
/// секции (без проверки, что мы в `[Settings]`). Заголовок `[Settings]`
/// дописывается в начало, если его нет; ключ — в конец, если не найден.
pub fn update_gtk_settings_ini(existing: Option<&str>, key: &str, value: &str) -> String {
    // Python читает f.readlines() — строки С завершающим '\n' (кроме, быть
    // может, последней). split_inclusive воспроизводит это.
    let lines: Vec<&str> = match existing {
        Some(text) => text.split_inclusive('\n').collect(),
        None => Vec::new(),
    };

    let mut new_lines: Vec<String> = Vec::new();
    let mut found = false;
    let mut has_header = false;
    let key_eq = format!("{key}=");

    for line in &lines {
        let stripped = line.trim();
        if stripped == "[Settings]" {
            has_header = true;
        }
        if stripped.starts_with(&key_eq) {
            new_lines.push(format!("{key}={value}\n"));
            found = true;
        } else {
            new_lines.push((*line).to_string());
        }
    }

    if !has_header {
        new_lines.insert(0, "[Settings]\n".to_string());
    }
    if !found {
        new_lines.push(format!("{key}={value}\n"));
    }

    new_lines.concat()
}

// ============================================================================
// Обнаружение установленных тем — разбор index.theme (чистые функции)
// ============================================================================

/// Порт `_is_valid_icon_theme`: тема валидна, если её `index.theme` содержит
/// НЕПУСТОЙ `Directories=`. Курсорные темы такого ключа не имеют.
///
/// Решает ПЕРВЫЙ из `icon_dirs`, где есть `{dir}/{name}/index.theme` (как в
/// оригинале). `fs` — снимок ФС; пустое `name` → false.
pub fn is_valid_icon_theme(fs: &FsSnapshot, name: &str, icon_dirs: &[String]) -> bool {
    if name.is_empty() {
        return false;
    }
    for d in icon_dirs {
        let idx = format!("{d}/{name}/index.theme");
        if !fs.is_file(&idx) {
            continue;
        }
        // Первый каталог с index.theme решает исход.
        let Some(content) = fs.read_text(&idx) else {
            return false; // читаем как OSError → False
        };
        for line in content.lines() {
            let s = line.trim();
            if s.starts_with("Directories=") && s != "Directories=" {
                return true;
            }
        }
        return false;
    }
    false
}

/// Порт `list_gtk_themes`: подпапки `GTK_THEME_DIRS` с `gtk-3.0/` или
/// `gtk-2.0/`. Результат отсортирован и дедуплицирован (как `sorted(set)`).
pub fn list_gtk_themes(fs: &FsSnapshot, home: &str) -> Vec<String> {
    let mut themes: Vec<String> = Vec::new();
    for d in gtk_theme_dirs(home) {
        if !fs.is_dir(&d) {
            continue;
        }
        for name in fs.scandir(&d) {
            let base = format!("{d}/{name}");
            if !fs.is_dir(&base) {
                continue;
            }
            if fs.is_dir(&format!("{base}/gtk-3.0")) || fs.is_dir(&format!("{base}/gtk-2.0")) {
                themes.push(name.clone());
            }
        }
    }
    sorted_set(themes)
}

/// Порт `list_icon_themes`: подпапки `ICON_DIRS` с `index.theme`, прошедшие
/// [`is_valid_icon_theme`] (курсорные темы без `Directories=` отсеиваются).
pub fn list_icon_themes(fs: &FsSnapshot, home: &str) -> Vec<String> {
    let dirs = icon_dirs(home);
    let mut icons: Vec<String> = Vec::new();
    for d in &dirs {
        if !fs.is_dir(d) {
            continue;
        }
        for name in fs.scandir(d) {
            let base = format!("{d}/{name}");
            if !fs.is_dir(&base) {
                continue;
            }
            if !fs.is_file(&format!("{base}/index.theme")) {
                continue;
            }
            if !is_valid_icon_theme(fs, name, &dirs) {
                continue;
            }
            icons.push(name.clone());
        }
    }
    sorted_set(icons)
}

/// Порт `list_cursor_themes`: подпапки `ICON_DIRS` с `cursors/`.
pub fn list_cursor_themes(fs: &FsSnapshot, home: &str) -> Vec<String> {
    let mut cursors: Vec<String> = Vec::new();
    for d in icon_dirs(home) {
        if !fs.is_dir(&d) {
            continue;
        }
        for name in fs.scandir(&d) {
            let base = format!("{d}/{name}");
            if fs.is_dir(&base) && fs.is_dir(&format!("{base}/cursors")) {
                cursors.push(name.clone());
            }
        }
    }
    sorted_set(cursors)
}

fn sorted_set(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

// ============================================================================
// KDE Plasma темы: сканеры каталогов + apply-планы
// ============================================================================

/// Каталоги look-and-feel (global) — порт `_KDE_DIRS["global"]`.
pub fn kde_global_dirs(home: &str) -> [String; 2] {
    [
        format!("{home}/.local/share/plasma/look-and-feel"),
        "/usr/share/plasma/look-and-feel".to_string(),
    ]
}

/// Каталоги Plasma Style — порт `_KDE_DIRS["plasma_style"]`.
pub fn kde_plasma_style_dirs(home: &str) -> [String; 2] {
    [
        format!("{home}/.local/share/plasma/desktoptheme"),
        "/usr/share/plasma/desktoptheme".to_string(),
    ]
}

/// Каталоги цветовых схем — порт `_KDE_DIRS["colors"]`.
pub fn kde_color_dirs(home: &str) -> [String; 2] {
    [
        format!("{home}/.local/share/color-schemes"),
        "/usr/share/color-schemes".to_string(),
    ]
}

/// Каталоги Kvantum — порт `_KDE_DIRS["kvantum"]`.
pub fn kde_kvantum_dirs(home: &str) -> [String; 2] {
    kde_kvantum_dirs_xdg(None, home)
}

/// То же с учётом `$XDG_CONFIG_HOME`.
pub fn kde_kvantum_dirs_xdg(xdg_config_home: Option<&str>, home: &str) -> [String; 2] {
    [
        format!("{}/Kvantum", crate::utils::config_home(xdg_config_home, home)),
        "/usr/share/Kvantum".to_string(),
    ]
}

/// Порт `list_kde_global_themes`: подпапки с `metadata.json` ИЛИ
/// `metadata.desktop`. КВИРК приоритета операторов Python сохранён:
/// условие `is_dir and isfile(json) or isfile(desktop)` — при наличии
/// `metadata.desktop` тема добавляется ДАЖЕ если запись не каталог.
pub fn list_kde_global_themes(fs: &FsSnapshot, home: &str) -> Vec<String> {
    kde_lnf_like_scan(fs, &kde_global_dirs(home))
}

/// Порт `list_kde_plasma_styles` (та же логика операторов, что global).
pub fn list_kde_plasma_styles(fs: &FsSnapshot, home: &str) -> Vec<String> {
    kde_lnf_like_scan(fs, &kde_plasma_style_dirs(home))
}

// Общий обход для global/plasma_style: `(is_dir and isfile(A)) or isfile(B)`.
fn kde_lnf_like_scan(fs: &FsSnapshot, dirs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in dirs {
        if !fs.is_dir(d) {
            continue;
        }
        for name in fs.scandir(d) {
            let base = format!("{d}/{name}");
            let is_dir = fs.is_dir(&base);
            let has_json = fs.is_file(&format!("{base}/metadata.json"));
            let has_desktop = fs.is_file(&format!("{base}/metadata.desktop"));
            if (is_dir && has_json) || has_desktop {
                out.push(name.clone());
            }
        }
    }
    sorted_set(out)
}

/// Порт `list_kde_color_schemes`: файлы `*.colors` (без суффикса).
pub fn list_kde_color_schemes(fs: &FsSnapshot, home: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in kde_color_dirs(home) {
        if !fs.is_dir(&d) {
            continue;
        }
        for name in fs.scandir(&d) {
            let base = format!("{d}/{name}");
            if fs.is_file(&base) {
                if let Some(stem) = name.strip_suffix(".colors") {
                    out.push(stem.to_string());
                }
            }
        }
    }
    sorted_set(out)
}

/// Порт `list_kvantum_themes`: подпапки, где есть файл `*.kvconfig`.
/// `kv_files` — для каждой темы список имён файлов внутри (порт
/// `os.listdir(entry.path)` + `f.endswith(".kvconfig")`); отсутствует —
/// считается пустым.
pub fn list_kvantum_themes<F>(fs: &FsSnapshot, home: &str, dir_files: F) -> Vec<String>
where
    F: Fn(&str) -> Vec<String>,
{
    let mut out: Vec<String> = Vec::new();
    for d in kde_kvantum_dirs(home) {
        if !fs.is_dir(&d) {
            continue;
        }
        for name in fs.scandir(&d) {
            let base = format!("{d}/{name}");
            if !fs.is_dir(&base) {
                continue;
            }
            if dir_files(&base).iter().any(|f| f.ends_with(".kvconfig")) {
                out.push(name.clone());
            }
        }
    }
    sorted_set(out)
}

/// План чтения текущих KDE-тем — порт `get_current_kde_themes` (три
/// `kreadconfig6`-вызова; исполнитель читает stdout, `"—"` при пустом/ошибке).
pub fn get_current_kde_cmds() -> [Cmd; 3] {
    [
        cmd(&["kreadconfig6", "--file", "kdeglobals", "--group", "KDE", "--key", "LookAndFeelPackage"], None),
        cmd(&["kreadconfig6", "--file", "plasmarc", "--group", "Theme", "--key", "name"], None),
        cmd(&["kreadconfig6", "--file", "kdeglobals", "--group", "General", "--key", "ColorScheme"], None),
    ]
}

/// Порт `apply_kde_global_theme` (timeout=15, history «kde-global»).
pub fn apply_kde_global_plan(theme: &str) -> Plan {
    Plan {
        steps: vec![ApplyStep::Primary(cmd(&["plasma-apply-lookandfeel", "-a", theme], Some(15)))],
        history_kind: "kde-global",
    }
}

// ── Глобальная тема: «Применить «X»?» — оформление и расположение ─────────
//
// Plasma 6.8 перед применением глобальной темы спрашивает, ЧТО из неё
// взять: «Оформление» (цвета, стили, значки, курсоры, шрифты, заставка,
// обои, рамки) и «Расположение рабочего стола и виджетов» (раскладка
// панелей и виджетов из `contents/layouts`, кнопки заголовка из
// `contents/layouts/defaults`, размещение окон). Расположение по умолчанию
// НЕ отмечено: оно сносит нынешние панели и виджеты.
//
// Расположение есть у ЛЮБОЙ темы с оформлением, как в KDE: недостающие
// файлы темы Plasma берёт из запасного пакета, и это всегда Breeze —
// `org.kde.breeze.desktop`, зашитый в структуру пакета глобальной темы, а не
// тема по умолчанию из kdeglobals. Раскладка панелей у темы без своей —
// раскладка Breeze, и подпись флажка говорит это прямо
// ([`LnfLayout::desktop`] ложно). Отметка плитки «Содержит расположение
// виджетов» — только у темы со своей раскладкой.

/// Расположение, которое несёт глобальная тема.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LnfLayout {
    /// Есть сценарий `contents/layouts/*.js` — своя раскладка панелей и виджетов.
    pub desktop: bool,
    /// Кнопки заголовка (`layouts/defaults`, `[kwinrc][org.kde.kdecoration2]`).
    pub buttons: Option<(String, String)>,
    /// Размещение окон (`defaults`, `[kwinrc][Windows] Placement`).
    pub placement: Option<String>,
    /// `BorderlessMaximizedWindows` из `layouts/defaults`; `Some("")` — файл
    /// есть, ключа нет (Plasma тогда выключает «без рамки»), `None` — файла нет.
    pub borderless: Option<String>,
}

impl LnfLayout {
    pub fn any(&self) -> bool {
        self.desktop || self.buttons.is_some() || self.placement.is_some()
    }
}

/// Часть глобальной темы — флаг `KLookAndFeelManager::ContentFlags`
/// Plasma 6.8 (значения [`LnfPart::bit`] те же). Окно «Выбрать, что
/// применить…» показывает их флажками; обои ([`LnfPart::Wallpaper`]) Plasma
/// 6.8 распознаёт, но не применяет и не показывает.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LnfPart {
    Colors,
    WidgetStyle,
    WindowDecoration,
    BorderSize,
    Icons,
    PlasmaTheme,
    Cursors,
    Fonts,
    WindowSwitcher,
    SplashScreen,
    Wallpaper,
    DesktopLayout,
    TitlebarLayout,
    WindowPlacement,
}

/// Все части оформления (`AppearanceSettings` Plasma).
pub const LNF_APPEARANCE_MASK: u32 = (1 << 12) - 1;
/// Все части расположения (`LayoutSettings` Plasma).
pub const LNF_LAYOUT_MASK: u32 = (1 << 20) - (1 << 16);
/// Части, которые после себя просят KWin перечитать настройки
/// (`KWinSettings` Plasma).
const LNF_KWIN_MASK: u32 = (1 << 7) | (1 << 2) | (1 << 18) | (1 << 17) | (1 << 11);

impl LnfPart {
    /// Порядок окна Plasma 6.8 (`MoreOptions.qml`), обои — после
    /// оформления, расположение — последним.
    pub const ALL: [LnfPart; 14] = [
        LnfPart::Colors,
        LnfPart::WidgetStyle,
        LnfPart::WindowDecoration,
        LnfPart::BorderSize,
        LnfPart::Icons,
        LnfPart::PlasmaTheme,
        LnfPart::Cursors,
        LnfPart::Fonts,
        LnfPart::WindowSwitcher,
        LnfPart::SplashScreen,
        LnfPart::Wallpaper,
        LnfPart::DesktopLayout,
        LnfPart::TitlebarLayout,
        LnfPart::WindowPlacement,
    ];

    /// Флаг Plasma.
    pub fn bit(self) -> u32 {
        match self {
            LnfPart::Colors => 1 << 0,
            LnfPart::WidgetStyle => 1 << 1,
            LnfPart::WindowDecoration => 1 << 2,
            LnfPart::Icons => 1 << 3,
            LnfPart::PlasmaTheme => 1 << 4,
            LnfPart::Cursors => 1 << 5,
            LnfPart::Fonts => 1 << 6,
            LnfPart::WindowSwitcher => 1 << 7,
            LnfPart::SplashScreen => 1 << 8,
            LnfPart::Wallpaper => 1 << 10,
            LnfPart::BorderSize => 1 << 11,
            LnfPart::DesktopLayout => 1 << 16,
            LnfPart::TitlebarLayout => 1 << 17,
            LnfPart::WindowPlacement => 1 << 18,
        }
    }

    pub fn is_layout(self) -> bool {
        self.bit() & LNF_LAYOUT_MASK != 0
    }

    /// Подпись флажка — тексты окна Plasma 6.8.
    pub fn label(self) -> String {
        match self {
            LnfPart::Colors => trc("global theme part", "Colors"),
            LnfPart::WidgetStyle => trc("global theme part", "Application Style"),
            LnfPart::WindowDecoration => trc("global theme part", "Window Decoration Style"),
            LnfPart::BorderSize => trc("global theme part", "Window Decoration Size"),
            LnfPart::Icons => trc("global theme part", "Icons"),
            LnfPart::PlasmaTheme => trc("global theme part", "Plasma Style"),
            LnfPart::Cursors => trc("global theme part", "Pointers"),
            LnfPart::Fonts => trc("global theme part", "Fonts"),
            LnfPart::WindowSwitcher => trc("global theme part", "Task Switcher"),
            LnfPart::SplashScreen => trc("global theme part", "Splash Screen"),
            LnfPart::Wallpaper => trc("global theme part", "Wallpaper"),
            LnfPart::DesktopLayout | LnfPart::WindowPlacement => trc("global theme part", "Desktop layout"),
            LnfPart::TitlebarLayout => trc("global theme part", "Titlebar Buttons layout"),
        }
    }
}

/// Маска списка частей.
pub fn lnf_mask(parts: &[LnfPart]) -> u32 {
    parts.iter().fold(0, |m, p| m | p.bit())
}

/// Части маски по порядку [`LnfPart::ALL`].
pub fn lnf_parts_of(mask: u32) -> Vec<LnfPart> {
    LnfPart::ALL.into_iter().filter(|p| mask & p.bit() != 0).collect()
}

/// Что несёт глобальная тема. `appearance` и `layout` — без подмены
/// недостающего из Breeze (подпись флажка и отметка плитки); `parts` и
/// `values` — как их видит Plasma: файла `defaults`, `layouts/defaults`,
/// заставки или раскладки нет у темы — он берётся из запасного пакета
/// ([`LNF_FALLBACK_PACKAGE`]), если тот есть на диске.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LnfContents {
    /// Хоть одна настройка оформления.
    pub appearance: bool,
    pub layout: LnfLayout,
    /// Части темы (`KLookAndFeelManager::packageContents`).
    pub parts: Vec<LnfPart>,
    /// Значения, которые части пишут: «группа/…/ключ» файла `defaults`
    /// (`kdeglobals/General/ColorScheme`), только непустые.
    pub values: BTreeMap<String, String>,
    /// У темы свой файл цветов (`contents/colors`).
    pub own_colors: bool,
}

impl LnfContents {
    pub fn mask(&self) -> u32 {
        lnf_mask(&self.parts)
    }

    pub fn has(&self, part: LnfPart) -> bool {
        self.parts.contains(&part)
    }

    fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
}

/// Ключи `defaults`, которые читают части (`KLookAndFeelManager::save`).
const LNF_VALUE_KEYS: &[(&str, &str)] = &[
    ("kdeglobals/General", "ColorScheme"),
    ("kdeglobals/KDE", "widgetStyle"),
    ("kdeglobals/Icons", "Theme"),
    ("plasmarc/Theme", "name"),
    ("kcminputrc/Mouse", "cursorTheme"),
    ("kwinrc/WindowSwitcher", "LayoutName"),
    ("kwinrc/org.kde.kdecoration2", "library"),
    ("kwinrc/org.kde.kdecoration2", "theme"),
    ("kwinrc/org.kde.kdecoration2", "NoPlugin"),
    ("kwinrc/org.kde.kdecoration2", "BorderSize"),
    ("kdeglobals/General", "font"),
    ("kdeglobals/General", "fixed"),
    ("kdeglobals/General", "smallestReadableFont"),
    ("kdeglobals/General", "toolBarFont"),
    ("kdeglobals/General", "menuFont"),
    ("kdeglobals/WM", "activeFont"),
    ("ksplashrc/KSplash", "Theme"),
    ("Wallpaper", "Image"),
];

/// Каталог глобальной темы `theme`: сперва свой, потом системный — тот же
/// порядок, что у Plasma. `None` — темы нет.
pub fn lnf_package_dir(io: &dyn crate::animations::LookIo, home: &str, theme: &str) -> Option<String> {
    if theme.is_empty() || theme.contains('/') {
        return None;
    }
    kde_global_dirs(home)
        .into_iter()
        .map(|root| format!("{root}/{theme}"))
        .find(|dir| !io.list_dir(dir).is_empty())
}

/// Значение ключа во вложенной группе файла `defaults` темы: путь групп
/// «kdeglobals/General» — заголовок `[kdeglobals][General]`.
fn lnf_value(text: &str, group_path: &str, key: &str) -> Option<String> {
    let group = group_path.replace('/', "][");
    crate::shell_palette::ini_get(text, &group, key).filter(|v| !v.is_empty())
}

/// Состав глобальной темы в каталоге `dir` — те же признаки, по которым
/// Plasma 6.8 решает, что тема несёт (без запасного пакета).
pub fn lnf_contents(io: &dyn crate::animations::LookIo, dir: &str) -> LnfContents {
    lnf_contents_with(io, dir, None)
}

/// Состав темы `theme` на машине `io`, как его видит Plasma: недостающие
/// файлы — из запасного пакета ([`LNF_FALLBACK_PACKAGE`]). `None` — темы
/// нет на диске.
pub fn lnf_contents_live(io: &dyn crate::animations::LookIo, home: &str, theme: &str) -> Option<LnfContents> {
    let dir = lnf_package_dir(io, home, theme)?;
    let fallback = lnf_package_dir(io, home, LNF_FALLBACK_PACKAGE).filter(|f| *f != dir);
    Some(lnf_contents_with(io, &dir, fallback.as_deref()))
}

/// [`lnf_contents`] с запасным пакетом в каталоге `fallback`.
///
/// Признаки частей — `KLookAndFeelManager::packageContents` Plasma 6.8:
/// цвета — `ColorScheme` или свой файл `colors` (только при `defaults`),
/// рамка — `library` или `NoPlugin`, размер рамки — `BorderSize`, шрифты —
/// любой шрифт `[kdeglobals][General]` или `[kdeglobals][WM] activeFont`
/// (и перепутанные группы, по которым ищет Plasma), заставка — `splash/Splash.qml`, раскладка —
/// каталог `layouts`, кнопки заголовка — `layouts/defaults`.
pub fn lnf_contents_with(io: &dyn crate::animations::LookIo, dir: &str, fallback: Option<&str>) -> LnfContents {
    let contents = format!("{dir}/contents");
    let top = io.list_dir(&contents);
    let has = |name: &str| top.iter().any(|n| n == name);
    let defaults = io.read(&format!("{contents}/defaults"));
    let layout_defaults = io.read(&format!("{contents}/layouts/defaults"));
    // Файл темы, а нет его — запасного пакета (так ищет KPackage).
    let fb = |rel: &str| fallback.and_then(|f| io.read(&format!("{f}/contents/{rel}")));
    let fb_has = |rel: &str, name: &str| {
        fallback.is_some_and(|f| {
            let dir = if rel.is_empty() { format!("{f}/contents") } else { format!("{f}/contents/{rel}") };
            io.list_dir(&dir).iter().any(|n| n == name)
        })
    };
    let pkg_defaults = defaults.clone().or_else(|| fb("defaults"));
    let pkg_layout_defaults = layout_defaults.clone().or_else(|| fb("layouts/defaults"));
    let d = pkg_defaults.as_deref().unwrap_or("");
    let provides = |group: &str, keys: &[&str]| keys.iter().any(|k| lnf_value(d, group, k).is_some());

    let own_splash = io.list_dir(&format!("{contents}/splash")).iter().any(|n| n == "Splash.qml");
    let own_colors = has("colors");
    let mut parts = Vec::new();
    let mut put = |part: LnfPart, on: bool| {
        if on {
            parts.push(part);
        }
    };
    put(LnfPart::SplashScreen, own_splash || fb_has("splash", "Splash.qml"));
    put(LnfPart::DesktopLayout, has("layouts") || fb_has("", "layouts"));
    if let Some(ld) = pkg_layout_defaults.as_deref() {
        put(
            LnfPart::TitlebarLayout,
            ["ButtonsOnLeft", "ButtonsOnRight"].iter().any(|k| lnf_value(ld, "kwinrc/org.kde.kdecoration2", k).is_some()),
        );
    }
    if pkg_defaults.is_some() {
        let deco = "kwinrc/org.kde.kdecoration2";
        put(LnfPart::Colors, provides("kdeglobals/General", &["ColorScheme"]) || own_colors || fb_has("", "colors"));
        put(LnfPart::WidgetStyle, provides("kdeglobals/KDE", &["widgetStyle"]));
        put(LnfPart::Icons, provides("kdeglobals/Icons", &["Theme"]));
        put(LnfPart::PlasmaTheme, provides("plasmarc/Theme", &["name"]));
        put(LnfPart::Wallpaper, provides("Wallpaper", &["Image"]));
        put(LnfPart::Cursors, provides("kcminputrc/Mouse", &["cursorTheme"]));
        put(LnfPart::WindowSwitcher, provides("kwinrc/WindowSwitcher", &["LayoutName"]));
        put(LnfPart::WindowDecoration, provides(deco, &["library", "NoPlugin"]));
        put(LnfPart::BorderSize, provides(deco, &["BorderSize"]));
        // ⭐ Починка: Plasma 6.8 ищет шрифты в перепутанных группах
        // (`[WM] font…`, `[General] activeFont`), а пишет из тех, где они
        // лежат у тем (`[General] font…`, `[WM] activeFont`), — у такой темы
        // флажка «Шрифты» в Plasma нет, и шрифты не применяются. Здесь
        // признаются оба места.
        let fonts = ["font", "fixed", "smallestReadableFont", "toolBarFont", "menuFont"];
        put(
            LnfPart::Fonts,
            provides("kdeglobals/WM", &fonts)
                || provides("kdeglobals/General", &["activeFont"])
                || provides("kdeglobals/General", &fonts)
                || provides("kdeglobals/WM", &["activeFont"]),
        );
        put(LnfPart::WindowPlacement, provides("kwinrc/Windows", &["Placement"]));
    }
    parts.sort_by_key(|p| LnfPart::ALL.iter().position(|q| q == p));
    let values = LNF_VALUE_KEYS
        .iter()
        .filter_map(|(group, key)| lnf_value(d, group, key).map(|v| (format!("{group}/{key}"), v)))
        .collect();

    // Оформление и расположение — по файлам самой темы (подпись флажка
    // «из Breeze», отметка плитки), как прежде.
    let own = defaults.as_deref().unwrap_or("");
    let own_provides = |group: &str, keys: &[&str]| keys.iter().any(|k| lnf_value(own, group, k).is_some());
    let appearance = own_splash
        || own_colors
        || own_provides("kdeglobals/General", &["ColorScheme", "activeFont"])
        || own_provides("kdeglobals/KDE", &["widgetStyle"])
        || own_provides("kdeglobals/Icons", &["Theme"])
        || own_provides("plasmarc/Theme", &["name"])
        || own_provides("Wallpaper", &["Image"])
        || own_provides("kcminputrc/Mouse", &["cursorTheme"])
        || own_provides("kwinrc/WindowSwitcher", &["LayoutName"])
        || own_provides("kwinrc/org.kde.kdecoration2", &["library", "NoPlugin", "BorderSize"])
        || own_provides("kdeglobals/WM", &["font", "fixed", "smallestReadableFont", "toolBarFont", "menuFont"])
        || lnf_mask(&parts) & LNF_APPEARANCE_MASK != 0;

    // Кнопки, «без рамки» и размещение окон — из файлов пакета (с запасным),
    // как их читает `KLookAndFeelManager::save`.
    let ld = pkg_layout_defaults.as_deref().unwrap_or("");
    let deco = "kwinrc/org.kde.kdecoration2";
    let buttons = match (lnf_value(ld, deco, "ButtonsOnLeft"), lnf_value(ld, deco, "ButtonsOnRight")) {
        (None, None) => None,
        (l, r) => Some((l.unwrap_or_default(), r.unwrap_or_default())),
    };
    let layout = LnfLayout {
        desktop: lnf_has_desktop_layout(io, dir),
        buttons,
        placement: lnf_value(d, "kwinrc/Windows", "Placement"),
        borderless: pkg_layout_defaults
            .as_deref()
            .map(|t| lnf_value(t, "kwinrc/Windows", "BorderlessMaximizedWindows").unwrap_or_default()),
    };
    LnfContents { appearance, layout, parts, values, own_colors }
}

/// Есть ли у темы своя раскладка панелей и виджетов — отметка плитки
/// «Содержит расположение виджетов».
///
/// Своя раскладка — сценарий `*.js` в `contents/layouts`: одни кнопки
/// заголовка (`layouts/defaults`, так их несёт тема, сохранённая вне
/// Plasma) раскладкой панелей не считаются.
pub fn lnf_has_desktop_layout(io: &dyn crate::animations::LookIo, dir: &str) -> bool {
    io.list_dir(&format!("{dir}/contents/layouts")).iter().any(|n| n.ends_with(".js"))
}

/// Подпись плитки темы со своей раскладкой.
pub fn lnf_layout_badge() -> String {
    tr("Contains desktop layout")
}

/// Отметка плитки темы с диска: у глобальной темы со своей раскладкой
/// (`item_id` — `local:global:…`, `install_path` — её каталог) —
/// «Содержит расположение виджетов», у прочих — пусто.
pub fn lnf_tile_badge(item_id: &str, install_path: &str) -> String {
    if item_id.starts_with("local:global:")
        && !install_path.is_empty()
        && std::fs::read_dir(std::path::Path::new(install_path).join("contents/layouts"))
            .map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".js")))
            .unwrap_or(false)
    {
        lnf_layout_badge()
    } else {
        String::new()
    }
}

/// Что человек отметил в окне «Применить «X»?».
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct GlobalChoice {
    pub appearance: bool,
    pub layout: bool,
    /// Подробный вид окна («Выбрать, что применить…»): отмеченные части
    /// маской [`LnfPart::bit`]. `None` — простой вид: оформление и
    /// расположение целиком.
    pub parts: Option<u32>,
}

/// Выбор вместе с составом темы — то, из чего собирается план.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GlobalPress {
    pub choice: GlobalChoice,
    pub contents: LnfContents,
    /// Части, которые включает план раздела Rexon, а не ключ темы
    /// ([`lnf_section_parts`]): шаги или причина, почему не вышло. Собирает
    /// живой установщик теми же пробами, что у кнопки раздела.
    #[serde(skip)]
    pub sections: BTreeMap<LnfPart, Result<Vec<ApplyStep>, String>>,
}

/// Строка окна: флажок, строка-пункт без флажка или ничего.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GlobalRow {
    Checkbox,
    Bullet,
    Hidden,
}

/// Флажок части в подробном виде окна.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalPartRow {
    pub id: LnfPart,
    /// Что включает флажок: «Расположение рабочего стола» — вместе с
    /// размещением окон, как в Plasma.
    pub mask: u32,
    pub text: String,
    /// Часть расположения (группа «Расположение:»), иначе оформления.
    pub layout: bool,
    pub checked: bool,
    /// Вне Plasma часть может не переноситься: флажок выключен, `reason` —
    /// почему.
    pub enabled: bool,
    pub reason: String,
}

/// Окно «Применить «X»?» — всё, что рисует страница.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalQuestion {
    pub title: String,
    pub heading: String,
    pub appearance_row: GlobalRow,
    pub appearance_text: String,
    pub appearance_checked: bool,
    pub layout_row: GlobalRow,
    pub layout_text: String,
    pub layout_checked: bool,
    /// Показывается, пока отмечено расположение.
    pub layout_warning: String,
    /// Тема не несёт ничего: окно объясняет, «Применить» выключена.
    pub empty_text: String,
    pub apply_text: String,
    pub cancel_text: String,
    /// Подробный вид: флажки частей, сначала расположение, потом
    /// оформление (как в Plasma); частей, которых в теме нет, в списке нет.
    pub parts: Vec<GlobalPartRow>,
    pub layout_heading: String,
    pub appearance_heading: String,
    /// Кнопка «Выбрать, что применить…» / «Меньше параметров…»; доступна,
    /// когда в теме есть оформление (как в Plasma).
    pub more_text: String,
    pub fewer_text: String,
    pub more_enabled: bool,
    /// Открыть сразу подробный вид — вне Plasma, где видно, что переносится.
    pub detailed: bool,
    /// Вне Plasma: как применяется тема на этом столе.
    pub outside_note: String,
}

/// Запасная глобальная тема Plasma: из неё берётся всё, чего в теме нет,
/// в том числе раскладка панелей и виджетов.
pub const LNF_FALLBACK_PACKAGE: &str = "org.kde.breeze.desktop";

/// Части, доступные в окне: части темы, а у темы с оформлением — ещё и
/// расположение (недостающее даёт Breeze).
pub fn lnf_available_mask(contents: &LnfContents) -> u32 {
    let mut mask = contents.mask();
    if contents.appearance || contents.layout.any() {
        mask |= LnfPart::DesktopLayout.bit();
    }
    mask
}

/// Почему часть темы не переносится на стол `de` (не Plasma); `None` —
/// переносится. Переносится то, что у Rexon есть для любого стола: цвета
/// (слой Qt), стиль приложений, значки, курсоры, шрифты, кнопки заголовка.
pub fn lnf_part_outside(part: LnfPart, contents: &LnfContents, de: &str) -> Option<String> {
    use crate::de_customization::{de_from_id, De};
    let deepin = de_from_id(de) == Some(De::Deepin);
    let chameleon = || tr("Deepin programs are drawn by its own Chameleon style, and Rexon does not replace it");
    match part {
        LnfPart::Colors | LnfPart::WidgetStyle if deepin => Some(chameleon()),
        LnfPart::Colors if contents.value("kdeglobals/General/ColorScheme").is_none() => Some(lnf_own_colors_reason()),
        LnfPart::Fonts if deepin => Some(tr("In Deepin, fonts are changed only in its Control Center")),
        LnfPart::Colors | LnfPart::WidgetStyle | LnfPart::Icons | LnfPart::Cursors | LnfPart::Fonts => None,
        LnfPart::TitlebarLayout => None,
        LnfPart::WindowDecoration | LnfPart::BorderSize => {
            Some(tr("these window frames are drawn only by KWin, the Plasma window manager"))
        }
        LnfPart::WindowSwitcher => Some(tr("this task switcher (Alt+Tab) belongs to KWin, the Plasma window manager")),
        LnfPart::PlasmaTheme => {
            Some(tr("a Plasma Style draws Plasma's own panels and widgets, and this desktop has none of them"))
        }
        LnfPart::SplashScreen => Some(tr("this splash screen is shown only when Plasma starts")),
        LnfPart::Wallpaper => Some(tr("Plasma does not apply the wallpaper of a global theme either")),
        LnfPart::DesktopLayout | LnfPart::WindowPlacement => {
            Some(tr("the desktop and panel layout is a Plasma script that only Plasma can run"))
        }
    }
}

/// Цвета темы — только своим файлом, без имени схемы: их вносит лишь
/// применение темы целиком (`plasma-apply-lookandfeel`).
fn lnf_own_colors_reason() -> String {
    tr("the theme keeps its colors only in its own file, which only applying the whole theme in Plasma brings in")
}

/// Части, которые можно отметить на столе `de`.
fn lnf_allowed_mask(contents: &LnfContents, de: &str) -> u32 {
    if de == "kde" {
        return u32::MAX;
    }
    LnfPart::ALL.into_iter().filter(|p| lnf_part_outside(*p, contents, de).is_none()).fold(0, |m, p| m | p.bit())
}

/// Части, которые включает план раздела Rexon — те же шаги, что у кнопки
/// «Применить» в разделе «Значки», «Курсоры»…: в Plasma — значки и курсоры
/// (их план проверяет тему), вне Plasma — ещё цвета (слой Qt), стиль
/// приложений и кнопки заголовка.
pub fn lnf_section_parts(de: &str) -> &'static [LnfPart] {
    if de == "kde" {
        &[LnfPart::Icons, LnfPart::Cursors]
    } else {
        &[LnfPart::Colors, LnfPart::WidgetStyle, LnfPart::Icons, LnfPart::Cursors, LnfPart::TitlebarLayout]
    }
}

/// Имя, которое часть передаёт плану раздела: схема, стиль, тема значков
/// или курсоров. Стиль `breeze` со строчной — `Breeze`, как поправляет
/// Plasma.
pub fn lnf_section_value(contents: &LnfContents, part: LnfPart) -> Option<String> {
    let key = match part {
        LnfPart::Colors => "kdeglobals/General/ColorScheme",
        LnfPart::WidgetStyle => "kdeglobals/KDE/widgetStyle",
        LnfPart::Icons => "kdeglobals/Icons/Theme",
        LnfPart::Cursors => "kcminputrc/Mouse/cursorTheme",
        _ => return None,
    };
    let v = contents.value(key)?;
    Some(if part == LnfPart::WidgetStyle && v == "breeze" { "Breeze".to_string() } else { v.to_string() })
}

/// Окно для темы `display` с составом `contents` в Plasma. `plasma_locked`
/// — виджеты Plasma закреплены (`org.kde.PlasmaShell.immutable`): флажка
/// расположения тогда нет, как и в Plasma.
pub fn global_question(display: &str, contents: &LnfContents, plasma_locked: bool) -> GlobalQuestion {
    global_question_in(display, contents, plasma_locked, "kde")
}

/// Окно для стола `de`. Вне Plasma окно сразу подробное: части, которые
/// сюда не переносятся, стоят выключенными с причиной.
pub fn global_question_in(display: &str, contents: &LnfContents, plasma_locked: bool, de: &str) -> GlobalQuestion {
    let outside = de != "kde";
    let avail = lnf_available_mask(contents);
    let allowed = lnf_allowed_mask(contents, de);
    let a = contents.appearance;
    // Расположение доступно у любой темы с оформлением (недостающее — из
    // Breeze); тема без оформления — только когда своё расположение есть.
    let l = a || contents.layout.any();
    let carried = avail & allowed;
    let (appearance_row, layout_row) = if outside {
        // Вне Plasma расположение не переносится; оформление — одной
        // строкой: «то, что переносится» (список — в подробном виде).
        (if carried & LNF_APPEARANCE_MASK != 0 { GlobalRow::Bullet } else { GlobalRow::Hidden }, GlobalRow::Hidden)
    } else {
        (
            match (a, l) {
                (true, true) => GlobalRow::Checkbox,
                (true, false) => GlobalRow::Bullet,
                _ => GlobalRow::Hidden,
            },
            match (a, l) {
                (true, true) if !plasma_locked => GlobalRow::Checkbox,
                (false, true) => GlobalRow::Bullet,
                _ => GlobalRow::Hidden,
            },
        )
    };

    // Подробный вид — флажки `MoreOptions.qml` Plasma 6.8.
    let mut parts = Vec::new();
    let has_app_parts = avail & LNF_APPEARANCE_MASK & !LnfPart::Wallpaper.bit() != 0;
    let mut row = |part: LnfPart, mask: u32| {
        if avail & mask == 0 {
            return;
        }
        let reason = if outside { lnf_part_outside(part, contents, de) } else { None };
        let enabled = reason.is_none();
        // По умолчанию — всё, что тема несёт, кроме расположения при
        // оформлении (`resetSelectedContents`).
        let checked = enabled && (!part.is_layout() || !has_app_parts);
        parts.push(GlobalPartRow {
            id: part,
            mask: avail & mask,
            text: part.label(),
            layout: part.is_layout(),
            checked,
            enabled,
            reason: reason.unwrap_or_default(),
        });
    };
    if !(plasma_locked && !outside) {
        row(LnfPart::DesktopLayout, LnfPart::DesktopLayout.bit() | LnfPart::WindowPlacement.bit());
    }
    row(LnfPart::TitlebarLayout, LnfPart::TitlebarLayout.bit());
    for part in LnfPart::ALL.into_iter().filter(|p| !p.is_layout() && *p != LnfPart::Wallpaper) {
        row(part, part.bit());
    }

    let empty_text = if outside {
        if carried == 0 {
            tr("Nothing in this Global Theme can be carried over to this desktop: what it changes exists only in Plasma.")
        } else {
            String::new()
        }
    } else if a || l {
        String::new()
    } else {
        tr("This Global Theme provides no settings that can be applied. It may be broken — \
            please tell its author.")
    };
    GlobalQuestion {
        title: trf("Apply “%1”?", &[&display]),
        heading: tr("The following will be applied by this Global Theme:"),
        appearance_row,
        appearance_text: tr("Appearance settings"),
        // Оформление отмечено всегда, когда оно есть; расположение — только
        // когда кроме него в теме ничего нет.
        appearance_checked: if outside { appearance_row != GlobalRow::Hidden } else { a },
        layout_row,
        layout_text: if contents.layout.desktop {
            tr("Desktop and widget layout")
        } else {
            tr("Desktop and widget layout (from Breeze — the theme has none of its own)")
        },
        layout_checked: !outside && l && !a,
        layout_warning: tr(
            "Applying the layout will delete the current desktops, panels, docks and widgets \
             and replace them with the ones from the theme.",
        ),
        empty_text,
        apply_text: tr("Apply"),
        cancel_text: trc("dialog button", "Cancel"),
        parts,
        layout_heading: tr("Layout settings:"),
        appearance_heading: tr("Appearance settings:"),
        more_text: tr("Choose what to apply…"),
        fewer_text: tr("Show fewer options…"),
        more_enabled: if outside { carried != 0 } else { a },
        detailed: outside && carried != 0,
        outside_note: if outside {
            tr("Outside Plasma, Rexon applies the parts of the theme one by one — the ones this desktop can use.")
        } else {
            String::new()
        },
    }
}

/// Закреплены ли виджеты Plasma: ответ `dbus-send --print-reply` на
/// `org.kde.PlasmaShell.immutable` («boolean true»).
pub fn parse_plasma_immutable(reply: &str) -> bool {
    reply.split_whitespace().last() == Some("true")
}

/// argv вопроса «закреплены ли виджеты».
pub fn plasma_immutable_argv() -> Vec<String> {
    [
        "dbus-send",
        "--session",
        "--print-reply",
        "--dest=org.kde.plasmashell",
        "/PlasmaShell",
        "org.kde.PlasmaShell.immutable",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Окно для темы `theme` на машине `io` и столе `de`. `None` — темы нет
/// на диске.
pub fn global_question_live(
    io: &dyn crate::animations::LookIo,
    home: &str,
    theme: &str,
    display: &str,
    de: &str,
) -> Option<GlobalQuestion> {
    let contents = lnf_contents_live(io, home, theme)?;
    let locked = de == "kde" && io.have("dbus-send") && {
        let (ok, out) = io.run(&plasma_immutable_argv());
        ok && parse_plasma_immutable(&out)
    };
    Some(global_question_in(display, &contents, locked, de))
}

/// Что применить: отмеченное ∩ то, что тема несёт ∩ то, что переносится
/// на стол `de` (как у Plasma, флажок части, которой нет, ничего не
/// включает).
pub fn global_selection(press: &GlobalPress, de: &str) -> u32 {
    let c = &press.choice;
    let want = c.parts.unwrap_or_else(|| {
        (if c.appearance { LNF_APPEARANCE_MASK } else { 0 }) | (if c.layout { LNF_LAYOUT_MASK } else { 0 })
    });
    want & lnf_available_mask(&press.contents) & lnf_allowed_mask(&press.contents, de)
}

/// План по выбору человека и что из отмеченного не вышло (часть — причина).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalPlan {
    pub plan: Plan,
    pub skipped: Vec<String>,
}

/// План «Применить» глобальной темы в Plasma. `None` — не отмечено ничего.
pub fn apply_kde_global_choice_plan(theme: &str, press: &GlobalPress) -> Option<Plan> {
    apply_global_choice_plan(theme, press, "kde").map(|g| g.plan)
}

/// `kwriteconfig6 … --notify` — так части пишет `KLookAndFeelManager`.
fn lnf_kwrite(file: &str, group: &str, key: &str, value: &str, notify: bool) -> Cmd {
    let mut argv = vec!["kwriteconfig6", "--file", file, "--group", group, "--key", key];
    if notify {
        argv.push("--notify");
    }
    argv.push(value);
    cmd(&argv, None)
}

/// Команды подряд: первая решает `ok`, следующие — только при её успехе.
fn lnf_chain(cmds: Vec<Cmd>) -> Vec<ApplyStep> {
    cmds.into_iter()
        .enumerate()
        .map(|(i, c)| if i == 0 { ApplyStep::Primary(c) } else { ApplyStep::IfOk(c) })
        .collect()
}

/// Шаги одной части оформления в Plasma — те же ключи и команды, что у
/// `KLookAndFeelManager::save` Plasma 6.8; значки и курсоры — план раздела
/// ([`GlobalPress::sections`]). `Err` — почему часть не применится.
fn kde_part_steps(theme: &str, press: &GlobalPress, part: LnfPart) -> Result<Vec<ApplyStep>, String> {
    let c = &press.contents;
    let deco = "org.kde.kdecoration2";
    Ok(match part {
        LnfPart::Icons | LnfPart::Cursors => {
            return press.sections.get(&part).cloned().unwrap_or_else(|| Err(tr("not applied")));
        }
        LnfPart::Colors => match c.value("kdeglobals/General/ColorScheme") {
            Some(name) => apply_kde_color_plan(name).steps,
            None => return Err(lnf_own_colors_reason()),
        },
        LnfPart::WidgetStyle => {
            let style = lnf_section_value(c, part).unwrap_or_default();
            let env = KvantumEnv { de_id: "kde", home: "", xdg_config_home: "", platform_theme: "", icon_theme: String::new() };
            apply_qt_style_plan(&style, &env).steps
        }
        LnfPart::PlasmaTheme => apply_kde_plasma_plan(c.value("plasmarc/Theme/name").unwrap_or_default()).steps,
        LnfPart::WindowSwitcher => lnf_chain(vec![lnf_kwrite(
            "kwinrc",
            "TabBox",
            "LayoutName",
            c.value("kwinrc/WindowSwitcher/LayoutName").unwrap_or_default(),
            false,
        )]),
        LnfPart::WindowDecoration => {
            let get = |k: &str, d: &'static str| c.value(&format!("kwinrc/{deco}/{k}")).unwrap_or(d).to_string();
            let no_plugin = if get("NoPlugin", "false") == "true" { "true" } else { "false" };
            lnf_chain(vec![
                lnf_kwrite("kwinrc", deco, "library", &get("library", "org.kde.breeze"), false),
                lnf_kwrite("kwinrc", deco, "theme", &get("theme", "Breeze"), true),
                lnf_kwrite("kwinrc", deco, "NoPlugin", no_plugin, true),
            ])
        }
        LnfPart::BorderSize => lnf_chain(vec![lnf_kwrite(
            "kwinrc",
            deco,
            "BorderSize",
            c.value(&format!("kwinrc/{deco}/BorderSize")).unwrap_or_default(),
            true,
        )]),
        LnfPart::Fonts => {
            let mut cmds: Vec<Cmd> = ["font", "fixed", "smallestReadableFont", "toolBarFont", "menuFont"]
                .into_iter()
                .filter_map(|k| {
                    c.value(&format!("kdeglobals/General/{k}")).map(|v| lnf_kwrite("kdeglobals", "General", k, v, true))
                })
                .collect();
            if let Some(v) = c.value("kdeglobals/WM/activeFont") {
                cmds.push(lnf_kwrite("kdeglobals", "WM", "activeFont", v, true));
            }
            if cmds.is_empty() {
                return Err(tr("the theme names no fonts that Plasma reads"));
            }
            cmds.push(cmd(
                &["dbus-send", "--session", "--type=signal", "/KDEPlatformTheme", "org.kde.KDEPlatformTheme.refreshFonts"],
                None,
            ));
            lnf_chain(cmds)
        }
        LnfPart::SplashScreen => {
            let splash = c.value("ksplashrc/KSplash/Theme").unwrap_or(theme);
            lnf_chain(vec![
                lnf_kwrite("ksplashrc", "KSplash", "Theme", splash, false),
                lnf_kwrite("ksplashrc", "KSplash", "Engine", "KSplashQML", false),
            ])
        }
        // Обои Plasma 6.8 из темы не применяет; части расположения — в
        // [`kde_layout_steps`].
        _ => Vec::new(),
    })
}

/// Шаги расположения `lay` — как в Plasma: раскладка через plasmashell (без
/// своей — Breeze подставит сама Plasma), кнопки заголовка, «без рамки» у
/// развёрнутых, размещение окон.
fn kde_layout_steps(theme: &str, c: &LnfContents, lay: u32) -> Vec<ApplyStep> {
    let lay_bits = &c.layout;
    let kwrite = |file: &str, group: &str, key: &str, value: &str| ApplyStep::IfOk(lnf_kwrite(file, group, key, value, true));
    let has = |p: LnfPart| lay & p.bit() != 0;
    let mut steps = Vec::new();
    if has(LnfPart::DesktopLayout) {
        let arg = format!("string:{theme}");
        steps.push(ApplyStep::IfOk(cmd(
            &[
                "dbus-send",
                "--session",
                "--print-reply",
                "--dest=org.kde.plasmashell",
                "/PlasmaShell",
                "org.kde.PlasmaShell.loadLookAndFeelDefaultLayout",
                &arg,
            ],
            Some(30),
        )));
    }
    let deco = "org.kde.kdecoration2";
    if let (true, Some((left, right))) = (has(LnfPart::TitlebarLayout), &lay_bits.buttons) {
        steps.push(kwrite("kwinrc", deco, "ButtonsOnLeft", left));
        steps.push(kwrite("kwinrc", deco, "ButtonsOnRight", right));
    }
    if let (true, Some(value)) = (has(LnfPart::DesktopLayout), &lay_bits.borderless) {
        let value = if value.is_empty() { "false" } else { value.as_str() };
        steps.push(kwrite("kwinrc", "Windows", "BorderlessMaximizedWindows", value));
    }
    if let (true, Some(p)) = (has(LnfPart::WindowPlacement), &lay_bits.placement) {
        steps.push(kwrite("kwinrc", "Windows", "Placement", p));
    }
    steps
}

/// «KWin, перечитай настройки» — один раз в конце, как у Plasma.
fn kwin_reload_step() -> ApplyStep {
    ApplyStep::IfOk(cmd(&["dbus-send", "--session", "--type=signal", "/KWin", "org.kde.KWin.reloadConfig"], None))
}

/// План «Применить» глобальной темы по выбору человека на столе `de`.
/// `None` — не отмечено ничего.
///
/// Plasma (`apply_kde_global_choice_plan`):
/// * всё оформление темы — `plasma-apply-lookandfeel -a` (тот же
///   `KLookAndFeelManager::save`, что у окна Plasma); всё расположение —
///   плюс `--resetLayout`, часть расположения — её шаги следом;
/// * часть оформления — у `plasma-apply-lookandfeel` такого режима нет,
///   поэтому шаги те же, что делает сама Plasma для каждой отмеченной
///   части: тема записывается выбранной, каждая часть — своими ключами и
///   командами (`plasma-apply-colorscheme`, `plasma-apply-desktoptheme`,
///   значки и курсоры — планом раздела), затем KWin перечитывает настройки.
///
/// Вне Plasma — те же части, что у «Как в дистрибутиве»: цвета — слоем Qt,
/// стиль приложений, значки, курсоры и кнопки заголовка — планом раздела
/// Rexon для этого стола, шрифты — родными ключами стола.
pub fn apply_global_choice_plan(theme: &str, press: &GlobalPress, de: &str) -> Option<GlobalPlan> {
    let sel = global_selection(press, de);
    if sel == 0 {
        return None;
    }
    if de != "kde" {
        return Some(outside_global_plan(press, de, sel));
    }
    let c = &press.contents;
    let avail = lnf_available_mask(c);
    let selectable = avail & LNF_APPEARANCE_MASK & !LnfPart::Wallpaper.bit();
    let app = sel & LNF_APPEARANCE_MASK & !LnfPart::Wallpaper.bit();
    let lay = sel & LNF_LAYOUT_MASK;
    // Всё оформление темы (у темы с одними обоями — и они: Plasma их не
    // применяет, но и `-a` на такой теме ничего лишнего не делает).
    if sel & LNF_APPEARANCE_MASK != 0 && app == selectable {
        let mut plan = apply_kde_global_plan(theme);
        if lay != 0 && lay == avail & LNF_LAYOUT_MASK {
            if let Some(ApplyStep::Primary(c)) = plan.steps.first_mut() {
                c.argv.push("--resetLayout".into());
            }
        } else if lay != 0 {
            plan.steps.extend(kde_layout_steps(theme, c, lay));
            if lay & LNF_KWIN_MASK != 0 {
                plan.steps.push(kwin_reload_step());
            }
        }
        return Some(GlobalPlan { plan, skipped: Vec::new() });
    }
    let mut steps = vec![ApplyStep::Primary(cmd(
        &["kwriteconfig6", "--file", "kdeglobals", "--group", "KDE", "--key", "LookAndFeelPackage", theme],
        None,
    ))];
    steps.extend(kde_layout_steps(theme, c, lay));
    let mut skipped = Vec::new();
    for part in lnf_parts_of(app) {
        match kde_part_steps(theme, press, part) {
            Ok(s) => steps.extend(s),
            Err(why) => skipped.push(format!("{} — {why}", part.label())),
        }
    }
    if sel & LNF_KWIN_MASK != 0 {
        steps.push(kwin_reload_step());
    }
    Some(GlobalPlan { plan: Plan { steps, history_kind: "kde-global" }, skipped })
}

/// Шрифт KDE (`Семейство,10,-1,5,…`) — семейство, кегль и начертание
/// (жирность и наклон из полей QFont).
fn kde_font_parts(raw: &str) -> Option<(String, i32, crate::qt_settings::FontFace)> {
    let (family, size, face) = crate::qt_settings::parse_font_spec_face(raw);
    (!family.is_empty() && size > 0).then_some((family, size, face))
}

/// Вне Plasma: шаги отмеченных частей одна за другой.
fn outside_global_plan(press: &GlobalPress, de: &str, sel: u32) -> GlobalPlan {
    let c = &press.contents;
    let mut steps = Vec::new();
    let mut skipped = Vec::new();
    for part in lnf_parts_of(sel) {
        let got = match part {
            LnfPart::Fonts => {
                // Интерфейс, моноширинный и заголовок — у каждого стола свои
                // ключи ([`crate::qt_settings::font_write_argv_face`]);
                // жирность и наклон уходят туда словами Pango.
                let mut cmds = Vec::new();
                for (key, target) in
                    [("kdeglobals/General/font", "system"), ("kdeglobals/General/fixed", "mono"), ("kdeglobals/WM/activeFont", "title")]
                {
                    let Some((family, size, face)) = c.value(key).and_then(kde_font_parts) else { continue };
                    if !crate::qt_settings::font_target_available(de, target) {
                        continue;
                    }
                    if let Some(argv) = crate::qt_settings::font_write_argv_face(de, target, &family, size, face, "") {
                        cmds.push(Cmd { argv, timeout: None });
                    }
                }
                if cmds.is_empty() {
                    Err(tr("the theme names no fonts this desktop can use"))
                } else {
                    Ok(lnf_chain(cmds))
                }
            }
            _ => press.sections.get(&part).cloned().unwrap_or_else(|| Err(tr("not applied"))),
        };
        match got {
            Ok(s) => steps.extend(s),
            Err(why) => skipped.push(format!("{} — {why}", part.label())),
        }
    }
    GlobalPlan { plan: Plan { steps, history_kind: "kde-global" }, skipped }
}

/// Шаги общего движка «Внешнего вида» ([`crate::animations::LookStep`]) —
/// шагами плана применения: так кнопки заголовка из раздела «Кнопки
/// заголовка» входят в одно нажатие глобальной темы. `Err` — шаг, которого
/// план применения не умеет (подключение к конфигу композитора).
pub fn look_steps_as_apply(steps: &[crate::animations::LookStep], config_home: &str) -> Result<Vec<ApplyStep>, String> {
    use crate::animations::LookStep;
    let mut out = Vec::new();
    for step in steps {
        match step {
            LookStep::Cmd(argv) => out.push(if out.is_empty() {
                ApplyStep::Primary(Cmd { argv: argv.clone(), timeout: None })
            } else {
                ApplyStep::IfOk(Cmd { argv: argv.clone(), timeout: None })
            }),
            LookStep::Notify(variants) => {
                if let Some(argv) = variants.first() {
                    out.push(ApplyStep::BestEffort(Cmd { argv: argv.clone(), timeout: None }));
                }
            }
            LookStep::GtkIni { key, value } => {
                for version in ["3.0", "4.0"] {
                    out.push(ApplyStep::GtkIni {
                        path: crate::gtk_sanitizer::gtk_ini_path(config_home, version),
                        key: key.clone(),
                        value: value.clone(),
                    });
                }
            }
            LookStep::WriteFile { path, content } => {
                let dir = std::path::Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                out.push(ApplyStep::WriteSetsOk { dir, path: path.clone(), content: content.clone() });
            }
            LookStep::Wm(_) => return Err(tr("not applied")),
        }
    }
    Ok(out)
}

/// Кнопки заголовка темы (`ButtonsOnLeft`/`ButtonsOnRight`, буквы KWin) на
/// столе из `ctx` — тот же план, что у раздела «Кнопки заголовка»
/// ([`crate::window_buttons::plan_layout`]). `Err` — почему нет.
pub fn lnf_buttons_steps(
    io: &dyn crate::animations::LookIo,
    ctx: &crate::animations::LookCtx,
    contents: &LnfContents,
) -> Result<Vec<ApplyStep>, String> {
    use crate::window_buttons as wb;
    let Some((left, right)) = &contents.layout.buttons else { return Err(tr("not applied")) };
    let mut wm = ctx.clone();
    wm.de = wb::wm_key(io, ctx);
    if let Some(why) = wb::rc_refusal(io, &wm) {
        return Err(why);
    }
    let layout = wb::parse(wb::Codec::Kde, left, right);
    look_steps_as_apply(&wb::plan_layout(io, &wm, &layout), &ctx.config_home)
}

/// Порт `apply_kde_plasma_style` (timeout=10, history «kde-plasma»).
pub fn apply_kde_plasma_plan(theme: &str) -> Plan {
    Plan {
        steps: vec![ApplyStep::Primary(cmd(&["plasma-apply-desktoptheme", theme], Some(10)))],
        history_kind: "kde-plasma",
    }
}

/// Порт `apply_kde_color_scheme` (timeout=10, history «kde-colors»).
pub fn apply_kde_color_plan(scheme: &str) -> Plan {
    Plan {
        steps: vec![ApplyStep::Primary(cmd(&["plasma-apply-colorscheme", scheme], Some(10)))],
        history_kind: "kde-colors",
    }
}

// ============================================================================
// Kvantum: тема плюс движок стиля Qt
// ============================================================================
//
// `kvantummanager --set` записывает только ВЫБОР Kvantum (kvantum.kvconfig);
// движок стиля Qt он не переключает. Вторым шагом нужно сказать Qt, что стиль
// теперь Kvantum, — и вот тут окружения расходятся. Сам Kvantum при этом
// работает везде: это обычный плагин стилей Qt (`libkvantum.so`), красит любое
// Qt-приложение независимо от оболочки.

/// Чем в этом окружении выбирается движок стиля Qt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QtStyleWay {
    /// KDE: `widgetStyle` в `kdeglobals` плюс уведомление Plasma. Переменную
    /// окружения тут ставить НЕЛЬЗЯ — она перебьёт штатный выбор стиля.
    Kdeglobals,
    /// LXQt: `[Qt] style` в `~/.config/lxqt/lxqt.conf` — то же место, куда
    /// пишет lxqt-config-appearance.
    LxqtConf,
    /// Остальные: своего переключателя стиля Qt нет. Полный путь — `qt6ct`
    /// (стиль, шрифты и значки для Qt-приложений), запасной — переменная
    /// `QT_STYLE_OVERRIDE`.
    PlatformTheme,
    /// Deepin: программы DTK (и прочие Qt-программы через тему платформы
    /// самого Deepin) рисуются его стилем Chameleon. Любая переменная на
    /// весь сеанс — `QT_QPA_PLATFORMTHEME=qt6ct` или `QT_STYLE_OVERRIDE` —
    /// отняла бы Chameleon у Центра управления, файлового менеджера и прочих
    /// программ Deepin. Стиль Qt здесь не переключаем: план пуст, применение
    /// честно не удаётся, а раздел в Deepin скрыт с объяснением
    /// (`de_customization::unavailable_note`).
    Deepin,
}

/// Как в этом рабочем столе переключается стиль Qt.
///
/// Неизвестный идентификатор — общий путь: переменную окружения понимает сам
/// Qt, а не оболочка, поэтому она сработает и там, где мы не узнали стол.
pub fn qt_style_way(de_id: &str) -> QtStyleWay {
    use crate::de_customization::{de_from_id, De};
    match de_from_id(de_id) {
        Some(De::Kde) => QtStyleWay::Kdeglobals,
        Some(De::Lxqt) => QtStyleWay::LxqtConf,
        Some(De::Deepin) => QtStyleWay::Deepin,
        _ => QtStyleWay::PlatformTheme,
    }
}

/// Бинарь полноценного переключателя стиля Qt (он же ключ в таблице пакетов).
pub const QT_STYLE_TOOL: &str = "qt6ct";

/// Имя пакета переключателя стиля Qt для семейства дистрибутива.
///
/// Таблица имён — общая, `utils::TOOL_PACKAGES` (там же живут imagemagick,
/// icoutils и прочие): заводить вторую таблицу «пакет по семейству» было бы
/// ровно тем размножением, которое запрещает правило #12.
///
/// `None` — ставить нечем: в NixOS пакеты ставятся правкой конфигурации, в
/// Clear Linux бандла нет. Это не отказ — тема применится запасным путём.
pub fn qt_style_helper_pkg(pkg_family: &str) -> Option<String> {
    let pkg = crate::utils::tool_package_name(QT_STYLE_TOOL, pkg_family);
    if pkg.is_empty() {
        None
    } else {
        Some(pkg)
    }
}

/// Файл переменных окружения Rexon для стиля Qt (рядом с `rexon-terminal.conf`
/// — тот же механизм `~/.config/environment.d`, что у терминала).
pub const QT_STYLE_ENV_FILE: &str = "rexon-qt-style.conf";

/// Окружение для плана применения Kvantum-темы.
pub struct KvantumEnv<'a> {
    /// Идентификатор рабочего стола (`system::detect_current_de`).
    pub de_id: &'a str,
    pub home: &'a str,
    /// `$XDG_CONFIG_HOME` (пусто — `~/.config`).
    pub xdg_config_home: &'a str,
    /// Имя РАБОТАЮЩЕГО переключателя стиля Qt (`qt6ct`/`qt5ct`), если он в
    /// системе есть. Пусто — идём запасным путём через `QT_STYLE_OVERRIDE`:
    /// человек не остаётся без результата, просто без шрифтов и значков.
    pub platform_theme: &'a str,
    /// Тема значков, которую окружение отдаёт программам сейчас (пусто —
    /// неизвестна). Ложится в конфиг qt6ct, если там своей ещё нет: без неё
    /// Qt-программы под qt6ct теряют значки (Papirus → hicolor, Д2).
    pub icon_theme: String,
}

/// План применения Kvantum-темы — порт `apply_kvantum_theme`, доведённый до
/// всех окружений.
///
/// Первый шаг общий: `kvantummanager --set` (он же определяет `ok` и запись
/// истории). Второй зависит от [`qt_style_way`]:
///   * KDE — `kdeglobals` + уведомление Plasma (как было);
///   * LXQt — `[Qt] style` в его же `lxqt.conf`;
///   * остальные — `~/.config/environment.d`: `QT_QPA_PLATFORMTHEME=qt6ct`
///     плюс стиль в конфиге qt6ct, если он есть, иначе `QT_STYLE_OVERRIDE`.
///
/// ⚠️ Переменные окружения читаются при входе в сеанс — эффект будет после
/// перезахода. Сказать об этом человеку обязательно (`condition_note` матрицы).
pub fn apply_kvantum_plan(theme: &str, env: &KvantumEnv) -> Plan {
    if qt_style_way(env.de_id) == QtStyleWay::Deepin {
        // Выбор темы в Kvantum без переключения стиля ничего не покажет —
        // «Применено» было бы неправдой (см. [`QtStyleWay::Deepin`]).
        return Plan { steps: Vec::new(), history_kind: "kvantum" };
    }
    let mut steps = vec![ApplyStep::Primary(cmd(
        &["kvantummanager", "--set", theme],
        Some(10),
    ))];
    steps.extend(qt_style_switch_steps("kvantum", env, true));
    Plan { steps, history_kind: "kvantum" }
}

/// План выбора стиля Qt по имени (раздел «Стиль приложений»: Breeze, Fusion,
/// kvantum…) — во ВСЕХ окружениях, той же развилкой [`qt_style_way`], что у
/// Kvantum. Раньше раздел писал только `kdeglobals`, а его вне Plasma читают
/// лишь программы с платформенной темой KDE: «Применить» было зелёным, а стиль
/// не менялся.
///
/// `env` — то же окружение, что у Kvantum (имя типа историческое).
/// Вне KDE и LXQt стиль появится после перезахода — как и у Kvantum.
pub fn apply_qt_style_plan(style: &str, env: &KvantumEnv) -> Plan {
    Plan { steps: qt_style_switch_steps(style, env, false), history_kind: "widget-style" }
}

/// Шаги «сказать Qt, какой стиль брать» — общие для Kvantum и «Стиля
/// приложений» (правило #12: одна развилка на всех).
///
/// `after_primary` — перед этими шагами уже был свой `Primary` (у Kvantum —
/// `kvantummanager --set`), и они выполняются только при его успехе. Если нет,
/// первым делом становится сама запись стиля: на KDE — `kwriteconfig6`
/// (`Primary`), в остальных окружениях запись идёт в файл, который ведём
/// мы сами, поэтому `ForceOk`.
fn qt_style_switch_steps(style: &str, env: &KvantumEnv, after_primary: bool) -> Vec<ApplyStep> {
    let cfg = crate::utils::config_home(
        if env.xdg_config_home.is_empty() { None } else { Some(env.xdg_config_home) },
        env.home,
    );
    let mut steps = Vec::new();

    match qt_style_way(env.de_id) {
        QtStyleWay::Kdeglobals => {
            let write = cmd(
                &[
                    "kwriteconfig6", "--file", "kdeglobals", "--group", "KDE",
                    "--key", "widgetStyle", style,
                ],
                None,
            );
            steps.push(if after_primary { ApplyStep::IfOk(write) } else { ApplyStep::Primary(write) });
            // `notifyChange(2, 0)` — StyleChanged (KGlobalSettings::ChangeType:
            // 0 — палитра, 1 — шрифт, 2 — стиль). Раньше слался 0, и
            // запущенные Qt-программы стиль не меняли до перезапуска
            // (Д3, проверка окружений 02.10.2026: после 0 программа осталась
            // на breeze, после 2 — перешла на fusion).
            steps.push(ApplyStep::IfOk(cmd(
                &[
                    "dbus-send", "--type=signal", "/KGlobalSettings",
                    "org.kde.KGlobalSettings.notifyChange", "int32:2", "int32:0",
                ],
                None,
            )));
        }
        QtStyleWay::LxqtConf => {
            if !after_primary {
                steps.push(ApplyStep::ForceOk);
            }
            steps.push(ApplyStep::IniSetIfOk {
                path: format!("{cfg}/lxqt/lxqt.conf"),
                section: "[Qt]".into(),
                key: "style".into(),
                value: style.into(),
            });
        }
        QtStyleWay::PlatformTheme => {
            if !after_primary {
                steps.push(ApplyStep::ForceOk);
            }
            let tool = env.platform_theme;
            let content = if tool.is_empty() {
                // Запасной путь: Qt читает эту переменную сам, без всяких
                // пакетов. Стиль будет, шрифтов и значков — нет.
                format!("QT_STYLE_OVERRIDE={style}\n")
            } else {
                // Полный путь: стиль выбирается в конфиге qt6ct, поэтому
                // QT_STYLE_OVERRIDE не пишем — иначе человек сменит стиль в
                // qt6ct, а переменная его молча перебьёт.
                steps.push(ApplyStep::IniSetIfOk {
                    path: format!("{cfg}/{tool}/{tool}.conf"),
                    section: "[Appearance]".into(),
                    key: "style".into(),
                    value: style.into(),
                });
                // Свой выбор значков qt6ct не перебиваем; нет его — кладём
                // нынешние значки окружения, иначе после перезахода
                // Qt-программы останутся с `hicolor` (Д2).
                if !env.icon_theme.is_empty() {
                    steps.push(ApplyStep::IniSetGuarded {
                        path: format!("{cfg}/{tool}/{tool}.conf"),
                        section: "[Appearance]".into(),
                        key: "icon_theme".into(),
                        value: env.icon_theme.clone(),
                        only_if_file: false,
                        keep_existing: true,
                    });
                }
                format!("QT_QPA_PLATFORMTHEME={tool}\n")
            };
            steps.push(ApplyStep::WriteIfOk {
                dir: format!("{cfg}/environment.d"),
                path: format!("{cfg}/environment.d/{QT_STYLE_ENV_FILE}"),
                content,
            });
        }
        // Ни переменной, ни «успеха» — см. [`QtStyleWay::Deepin`].
        QtStyleWay::Deepin => {}
    }
    steps
}

// Текстов «поставить пакет?» здесь нет намеренно. Покупатель не должен
// видеть вопрос про дополнительную библиотеку и разбираться, почему выбор
// темы просит пароль: это ровно тот вопрос, где ответ «нет» даёт худший
// результат (правило #13, защита от дурака — как SDDM, который при установке
// тайлового стола ставится САМ). Пакет приезжает двумя молчаливыми путями —
// `depends` пакета Rexon и системный установщик
// (`installer::qt_style_install_cmd`), а если его нет в репозиториях вовсе,
// работает `QT_STYLE_OVERRIDE`. Единственное, о чём мы говорим человеку, —
// что изменения появятся после перезахода (оговорка раздела в матрице).

// ============================================================================
// Что тема просит в придачу: набор иконок и курсор (чистая часть)
// ============================================================================
//
// GTK-тема с метатемой (`index.theme`, секция `[X-GNOME-Metatheme]` или
// `[Desktop Entry]`) называет набор иконок (`IconTheme=`) и курсор
// (`CursorTheme=`), с которыми её задумал автор. Окружения сами эти ключи не
// читают: `gsettings set … gtk-theme` меняет только виджеты. Поэтому
// «Применить» у такой темы применяет и их — тем же планом, что кнопки
// разделов «Иконки» и «Курсоры» ([`apply_icon_plan_xdg`],
// [`apply_cursor_plan_sized`]), то есть во всех окружениях сразу.
//
// ⛔ Имя, которого на диске нет, в настройки окружения НЕ пишется: GTK
// молча откатится на hicolor/Adwaita, а Rexon показал бы «применено».
// Нет набора — честное предупреждение, текущие иконки и курсор остаются.

/// Что именно тема просит: набор иконок или курсор.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepRole {
    Icons,
    Cursor,
}

/// Набор иконок и курсор, которые тема просит в придачу (`None` — не просит).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeDeps {
    pub icons: Option<String>,
    pub cursor: Option<String>,
}

/// Где метатема держит свои ключи. Порядок — как у mate-appearance и
/// gnome-control-center прежних лет: основная секция, потом общая.
const DEPS_SECTIONS: [&str; 2] = ["X-GNOME-Metatheme", "Desktop Entry"];

/// Значения, которые просьбой о наборе не считаются: `hicolor` — запасной
/// набор, на который откатывается любой (его же пишет наш импорт
/// `.msstyles`, когда своих иконок у темы нет), `default` — не имя набора, а
/// ссылка на выбранный курсор.
const NOT_A_REQUEST: &[&str] = &["hicolor", "default"];

/// Что тема просит в придачу — разбор её `index.theme`.
pub fn theme_deps(index_theme: &str) -> ThemeDeps {
    let get = |key: &str| {
        DEPS_SECTIONS
            .iter()
            .find_map(|section| ini_key_value(index_theme, section, key))
            .filter(|v| !NOT_A_REQUEST.iter().any(|n| n.eq_ignore_ascii_case(v)))
            // Имя набора — имя каталога, не путь: «../x» из чужого
            // index.theme ушло бы в поиск, скачивание и настройки.
            .filter(|v| !v.contains('/') && v != "." && v != "..")
    };
    ThemeDeps { icons: get("IconTheme"), cursor: get("CursorTheme") }
}

/// Известный набор: под какими именами он встречается и каким пакетом
/// ставится в каждом семействе дистрибутивов (пусто — пакета нет или он
/// нам не известен).
struct KnownSet {
    role: DepRole,
    /// Имена одного и того же набора. Первое — основное. Разные
    /// дистрибутивы кладут один курсор под разными именами: тема просит
    /// «DMZ-White», а пакет Arch `xcursor-vanilla-dmz` ставит «Vanilla-DMZ».
    names: &'static [&'static str],
    /// Набор с вариантами («Papirus-Dark», «WhiteSur-dark»): тот же пакет
    /// приносит и имена `<имя>-<вариант>`.
    variants: bool,
    arch: &'static str,
    debian: &'static str,
    fedora: &'static str,
    suse: &'static str,
    /// Откуда набор можно положить в домашний каталог без пароля и без
    /// сборки ([`dep_home_source`]).
    home: HomeSource,
}

/// Готовый набор, который ставится в `~/.local/share/icons` простой
/// распаковкой — без пароля, без сборки и без чужих сценариев установки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeSource {
    /// Только системный пакет (или ничего).
    None,
    /// Товар магазина KDE (OCS): его номер и имя файла среди ссылок товара.
    /// `names` — какие наборы архив несёт на деле (регистр не важен):
    /// вариант, которого в архиве нет, отсюда не ставится.
    Ocs { content_id: &'static str, file: &'static str, names: &'static [&'static str] },
}

/// Откуда набор ставится в домашний каталог ([`HomeSource`]). `None` —
/// набор нам неизвестен или его так не поставить.
pub fn dep_home_source(role: DepRole, wanted: &str) -> Option<HomeSource> {
    KNOWN_SETS
        .iter()
        .find(|s| known_set_matches(s, role, wanted))
        .map(|s| s.home)
        .filter(|h| match h {
            HomeSource::None => false,
            HomeSource::Ocs { names, .. } => names.iter().any(|n| n.eq_ignore_ascii_case(wanted.trim())),
        })
}

/// Таблица известных наборов. Имена пакетов сверены 16.10.2026: Arch —
/// `pacman -Si` и AUR RPC (`whitesur-*` и `mint-y-icons` на чистом Arch
/// только в AUR, в CachyOS `mint-y-icons` есть в репозитории — путь
/// выбирает исполнитель по факту); Debian/Ubuntu (trixie, noble) —
/// списки файлов packages.debian.org/packages.ubuntu.com; Fedora (f43,
/// f44) — mdapi, `dmz-cursor-themes` оттуда снят; openSUSE (Tumbleweed,
/// Leap 16.0) — rpm из download.opensuse.org. WhiteSur нет ни в одном
/// из трёх.
const KNOWN_SETS: &[KnownSet] = &[
    KnownSet {
        role: DepRole::Cursor,
        names: &["DMZ-White", "Vanilla-DMZ", "dmz"],
        variants: false,
        arch: "xcursor-vanilla-dmz",
        debian: "dmz-cursor-theme",
        fedora: "",
        suse: "dmz-icon-theme-cursors",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Cursor,
        names: &["DMZ-Black", "Vanilla-DMZ-AA", "dmz-aa"],
        variants: false,
        arch: "xcursor-vanilla-dmz-aa",
        debian: "dmz-cursor-theme",
        fedora: "",
        suse: "dmz-icon-theme-cursors",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Cursor,
        names: &["Adwaita"],
        variants: false,
        arch: "adwaita-cursors",
        debian: "adwaita-icon-theme",
        fedora: "adwaita-cursor-theme",
        suse: "adwaita-icon-theme",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Cursor,
        names: &["breeze_cursors"],
        variants: false,
        arch: "breeze",
        debian: "breeze-cursor-theme",
        fedora: "breeze-cursor-theme",
        suse: "breeze6-cursors",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Cursor,
        names: &["WhiteSur-cursors"],
        variants: false,
        arch: "whitesur-cursor-theme-git",
        debian: "",
        fedora: "",
        suse: "",
        // Товар магазина KDE «WhiteSur cursors» (автор тот же): архив
        // WhiteSur-cursors.tar.xz несёт собранную тему WhiteSur-cursors
        // (проверено распаковкой 25.10.2026).
        home: HomeSource::Ocs { content_id: "1411743", file: "WhiteSur-cursors.tar.xz", names: &["WhiteSur-cursors"] },
    },
    // «gnome» — имя набора иконок GNOME 2; в GNOME 3 его сменил Adwaita.
    KnownSet {
        role: DepRole::Icons,
        names: &["Adwaita", "gnome"],
        variants: false,
        arch: "adwaita-icon-theme",
        debian: "adwaita-icon-theme",
        fedora: "adwaita-icon-theme",
        suse: "adwaita-icon-theme",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Icons,
        names: &["breeze"],
        variants: true,
        arch: "breeze-icons",
        debian: "breeze-icon-theme",
        fedora: "breeze-icon-theme",
        suse: "kf6-breeze-icons",
        home: HomeSource::None,
    },
    // В Fedora варианты Papirus — отдельные подпакеты; записи стоят раньше
    // общей, потому что поиск берёт первое совпадение.
    KnownSet {
        role: DepRole::Icons,
        names: &["Papirus-Dark"],
        variants: false,
        arch: "papirus-icon-theme",
        debian: "papirus-icon-theme",
        fedora: "papirus-icon-theme-dark",
        suse: "papirus-icon-theme",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Icons,
        names: &["Papirus-Light"],
        variants: false,
        arch: "papirus-icon-theme",
        debian: "papirus-icon-theme",
        fedora: "papirus-icon-theme-light",
        suse: "papirus-icon-theme",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Icons,
        names: &["Papirus"],
        variants: true,
        arch: "papirus-icon-theme",
        debian: "papirus-icon-theme",
        fedora: "papirus-icon-theme",
        suse: "papirus-icon-theme",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Icons,
        names: &["Mint-Y"],
        variants: true,
        arch: "mint-y-icons",
        debian: "mint-y-icons",
        fedora: "mint-y-icons",
        suse: "mint-y-icon-theme",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Icons,
        names: &["WhiteSur"],
        variants: true,
        arch: "whitesur-icon-theme",
        debian: "",
        fedora: "",
        suse: "",
        // Товар магазина KDE «WhiteSur icon theme» (автор тот же): архив
        // 01-WhiteSur.tar.xz несёт WhiteSur, WhiteSur-dark и WhiteSur-light
        // (проверено распаковкой 17.10.2026). Цветные варианты — другими файлами.
        home: HomeSource::Ocs {
            content_id: "1405756",
            file: "01-WhiteSur.tar.xz",
            names: &["WhiteSur", "WhiteSur-dark", "WhiteSur-light"],
        },
    },
];

/// Подходит ли набор из таблицы под просьбу темы. Регистр не важен: тема
/// WhiteSur просит «WhiteSur-Dark», а пакет ставит «WhiteSur-dark».
fn known_set_matches(set: &KnownSet, role: DepRole, wanted: &str) -> bool {
    if set.role != role {
        return false;
    }
    let w = wanted.to_lowercase();
    set.names.iter().any(|n| {
        let n = n.to_lowercase();
        w == n || (set.variants && w.strip_prefix(&n).is_some_and(|rest| rest.starts_with('-')))
    })
}

/// Под какими именами искать набор на диске: сама просьба, потом другие
/// имена того же набора ([`KNOWN_SETS`]). Без повторов.
pub fn dep_aliases(role: DepRole, wanted: &str) -> Vec<String> {
    let mut out = vec![wanted.to_string()];
    for set in KNOWN_SETS.iter().filter(|s| !s.variants && known_set_matches(s, role, wanted)) {
        for n in set.names {
            if !out.iter().any(|o| o == n) {
                out.push(n.to_string());
            }
        }
    }
    out
}

/// Пакет, который ставит набор, в семействе `pkg_family` (`arch`, `debian`,
/// `fedora`, `suse`). `None` — набор нам неизвестен или пакета нет.
pub fn dep_package(role: DepRole, wanted: &str, pkg_family: &str) -> Option<String> {
    let set = KNOWN_SETS.iter().find(|s| known_set_matches(s, role, wanted))?;
    let pkg = match pkg_family.to_lowercase().as_str() {
        "arch" => set.arch,
        "debian" => set.debian,
        "fedora" => set.fedora,
        "suse" => set.suse,
        _ => "",
    };
    (!pkg.is_empty()).then(|| pkg.to_string())
}

/// Все пакеты наборов таблицы для семейства `pkg_family` — белый список
/// установки помощника пакетов (`system_plan::pkg_helper_install_allowlist`):
/// значки и курсор темы он ставит только отсюда.
pub fn known_set_packages(pkg_family: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for set in KNOWN_SETS {
        let pkg = match pkg_family {
            "arch" => set.arch,
            "debian" => set.debian,
            "fedora" => set.fedora,
            "suse" => set.suse,
            _ => "",
        };
        if !pkg.is_empty() && !out.contains(&pkg) {
            out.push(pkg);
        }
    }
    out
}

/// Каталоги, где ищутся иконки и курсоры, — по спецификации XDG: `~/.icons`,
/// `$XDG_DATA_HOME/icons`, затем `<каждый из $XDG_DATA_DIRS>/icons`.
pub fn dep_search_roots(home: &str, data_home: &str, xdg_data_dirs: &str) -> Vec<String> {
    xdg_search_roots(home, ".icons", data_home, xdg_data_dirs, "icons")
}

/// Каталоги GTK-тем в порядке поиска GTK: `~/.themes`,
/// `$XDG_DATA_HOME/themes`, затем `<каждый из $XDG_DATA_DIRS>/themes`.
pub fn gtk_theme_search_roots(home: &str, data_home: &str, xdg_data_dirs: &str) -> Vec<String> {
    xdg_search_roots(home, ".themes", data_home, xdg_data_dirs, "themes")
}

fn xdg_search_roots(home: &str, legacy: &str, data_home: &str, xdg_data_dirs: &str, sub: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |p: String| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    let home = home.trim_end_matches('/');
    if home.starts_with('/') {
        push(format!("{home}/{legacy}"));
    }
    let data = data_home.trim_end_matches('/');
    if data.starts_with('/') {
        push(format!("{data}/{sub}"));
    }
    for d in crate::plugin_journal::data_dirs(xdg_data_dirs) {
        push(format!("{d}/{sub}"));
    }
    out
}

/// Годен ли каталог `dir` как набор роли `role`: у курсора есть `cursors/`,
/// у иконок — `index.theme` с непустым `Directories=` (то же мерило, что у
/// [`is_valid_icon_theme`]: курсорная тема с одним `index.theme` набором
/// иконок не считается).
fn dep_dir_usable(role: DepRole, dir: &str, is_dir: &dyn Fn(&str) -> bool, read: &dyn Fn(&str) -> Option<String>) -> bool {
    match role {
        DepRole::Cursor => is_dir(&format!("{dir}/cursors")),
        DepRole::Icons => read(&format!("{dir}/index.theme")).is_some_and(|text| {
            text.lines().map(str::trim).any(|s| s.starts_with("Directories=") && s != "Directories=")
        }),
    }
}

/// Имя установленного набора, которое закрывает просьбу темы: точное имя,
/// другое имя того же набора, затем то же без учёта регистра. `None` —
/// ничего подходящего на диске нет.
pub fn find_installed_dep(
    role: DepRole,
    wanted: &str,
    roots: &[String],
    is_dir: &dyn Fn(&str) -> bool,
    list_dir: &dyn Fn(&str) -> Vec<String>,
    read: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let aliases = dep_aliases(role, wanted);
    for name in &aliases {
        if roots.iter().any(|r| dep_dir_usable(role, &format!("{r}/{name}"), is_dir, read)) {
            return Some(name.clone());
        }
    }
    for name in &aliases {
        for root in roots {
            for entry in list_dir(root) {
                if entry.eq_ignore_ascii_case(name) && dep_dir_usable(role, &format!("{root}/{entry}"), is_dir, read) {
                    return Some(entry);
                }
            }
        }
    }
    None
}

/// Состояние одной просьбы темы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepState {
    /// Набор стоит — под этим именем его и применять.
    Installed(String),
    /// Набора нет, но его ставит этот пакет.
    Installable(String),
    /// Набора нет, и где его взять, Rexon не знает.
    Missing,
}

/// Просьба темы и что с ней.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepItem {
    pub role: DepRole,
    pub wanted: String,
    pub state: DepState,
}

/// Что делать с просьбами темы: `find` ищет установленный набор
/// ([`find_installed_dep`]), пакет — по таблице для `pkg_family`.
pub fn plan_theme_deps(
    deps: &ThemeDeps,
    pkg_family: &str,
    find: &dyn Fn(DepRole, &str) -> Option<String>,
) -> Vec<DepItem> {
    let mut out = Vec::new();
    for (role, wanted) in [(DepRole::Icons, &deps.icons), (DepRole::Cursor, &deps.cursor)] {
        let Some(wanted) = wanted else { continue };
        let state = match find(role, wanted) {
            Some(name) => DepState::Installed(name),
            None => match dep_package(role, wanted, pkg_family) {
                Some(pkg) => DepState::Installable(pkg),
                None => DepState::Missing,
            },
        };
        out.push(DepItem { role, wanted: wanted.clone(), state });
    }
    out
}

/// «набор иконок «X»» / «курсор «X»» — для фраз ниже.
pub fn dep_label(role: DepRole, name: &str) -> String {
    match role {
        // TRANSLATORS: part of a sentence, %1 is an icon set name.
        DepRole::Icons => trf("the icon set “%1”", &[&name]),
        // TRANSLATORS: part of a sentence, %1 is a cursor theme name.
        DepRole::Cursor => trf("the cursor “%1”", &[&name]),
    }
}

/// Что ставить перед «Применить» у GTK-темы — САМО, без вопроса «Да/Нет»
/// (решение владельца 24.10; защита — окно пароля polkit): чего нет на
/// диске, но можно поставить пакетом или скачать в домашний каталог.
/// Значки — только когда человек выбрал «Скачать и включить» (`icons_ok`,
/// решение 3а). Чего взять негде ([`DepState::Missing`] без домашнего
/// источника) — не здесь: тема применяется, а итог говорит, чего нет.
pub fn deps_to_prepare(items: &[DepItem], icons_ok: bool) -> Vec<DepItem> {
    items
        .iter()
        .filter(|i| !matches!(i.state, DepState::Installed(_)))
        .filter(|i| i.role != DepRole::Icons || icons_ok)
        .filter(|i| matches!(i.state, DepState::Installable(_)) || dep_home_source(i.role, &i.wanted).is_some())
        .cloned()
        .collect()
}

/// Отказ «Применить»: то, с чем идёт тема, не поставилось («всё или
/// ничего» — тема без своего курсора не применяется). `why` — причина
/// сбоя; окно пароля закрыто ([`crate::appearance_installer::password_not_confirmed`])
/// — «передумал»: совет нажать «Применить» ещё раз и ввести пароль.
pub fn deps_refusal(theme: &str, missing: &[DepItem], why: &str) -> String {
    let what: Vec<String> = missing.iter().map(|i| dep_label(i.role, &i.wanted)).collect();
    if why == crate::appearance_installer::password_not_confirmed() {
        return crate::i18n::fill(
            // TRANSLATORS: %1 is the theme name, %2 a list like “the cursor “X””.
            &crate::i18n::ntr(
                "The theme “%1” wasn't applied: it comes with %2, which isn't installed. Press “Apply” \
                 on the theme again and enter the password to install it.",
                "The theme “%1” wasn't applied: it comes with %2, which aren't installed. Press “Apply” \
                 on the theme again and enter the password to install them.",
                what.len() as u64,
            ),
            &[&theme, &what.join(", ")],
        );
    }
    trf(
        // TRANSLATORS: %1 is the theme name, %2 a list like “the cursor “X””,
        // %3 the reason.
        "The theme “%1” wasn't applied: couldn't install %2 that it comes with: %3.",
        &[&theme, &what.join(", "), &why],
    )
}

// ── Значки темы: спросить, а не включать молча ─────────────────────────────
//
// Решение Wehrwolfmann 20.10.2026 (3а): тема, рассчитанная на свой набор
// значков (образ «как Windows XP»), прежде молча докачивала его и включала
// вместо выбранного человеком. Теперь — один вопрос «Скачать и включить» /
// «Оставить мои» (у стоящего набора — «Включить»). Ответ запоминается для
// этой темы (настройка [`THEME_ICONS_CHOICE_KEY`], по имени темы): при
// следующем применении той же темы не спрашиваем. «Оставить мои» — тема
// применяется целиком, кроме значков.

/// Настройка Rexon: ответы на вопрос о значках темы, `{тема: включать?}`.
pub const THEME_ICONS_CHOICE_KEY: &str = "theme_icons_choice";

/// Запомненный ответ для темы `theme` из значения настройки
/// [`THEME_ICONS_CHOICE_KEY`]; `None` — не спрашивали.
pub fn icons_choice_of(value: Option<&Value>, theme: &str) -> Option<bool> {
    value?.get(theme)?.as_bool()
}

/// Значение настройки с ответом `yes` для темы `theme` (прочие ответы
/// остаются).
pub fn icons_choice_with(value: Option<&Value>, theme: &str, yes: bool) -> Value {
    let mut map = value.and_then(Value::as_object).cloned().unwrap_or_default();
    map.insert(theme.to_string(), Value::Bool(yes));
    Value::Object(map)
}

/// Вопрос о значках темы: заголовок, текст, подписи «да» и «нет».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconsQuestion {
    pub title: String,
    pub text: String,
    pub yes: String,
    pub no: String,
}

/// Спросить ли о значках темы `theme` при её применении. `items` — план
/// [`plan_theme_deps`], `current` — значки, что стоят сейчас, `remembered` —
/// [`icons_choice_of`]. `None` — спрашивать не о чем: тема значков не
/// просит, их нигде нет (о них скажет итог), они и так стоят, либо ответ
/// уже дан.
pub fn icons_question(theme: &str, items: &[DepItem], current: &str, remembered: Option<bool>) -> Option<IconsQuestion> {
    if remembered.is_some() {
        return None;
    }
    let item = items.iter().find(|i| i.role == DepRole::Icons)?;
    let installed = match &item.state {
        DepState::Installed(name) if name == current || item.wanted == current => return None,
        DepState::Installed(_) => true,
        DepState::Installable(_) => false,
        DepState::Missing if dep_home_source(DepRole::Icons, &item.wanted).is_some() => false,
        DepState::Missing => return None,
    };
    let (text, yes) = if installed {
        (
            // TRANSLATORS: %1 is the theme name, %2 is an icon set name.
            trf("The theme “%1” is made for the icons “%2”. Turn them on?", &[&theme, &item.wanted]),
            tr("Turn on"),
        )
    } else {
        (
            // TRANSLATORS: %1 is the theme name, %2 is an icon set name.
            trf("The theme “%1” is made for the icons “%2”. Download and turn them on?", &[&theme, &item.wanted]),
            tr("Download and turn on"),
        )
    };
    Some(IconsQuestion { title: tr("Theme icons"), text, yes, no: tr("Keep mine") })
}

/// Строка итога: значки темы не включены по ответу «Оставить мои».
pub fn dep_icons_kept_line(wanted: &str) -> String {
    // TRANSLATORS: %1 is an icon set name.
    trf("Your icons stay — the theme's icons “%1” were not turned on.", &[&wanted])
}

/// Строка итога: набор применён вместе с темой.
pub fn dep_applied_line(role: DepRole, applied: &str, wanted: &str) -> String {
    if applied == wanted {
        trf("Also applied %1 that the theme comes with.", &[&dep_label(role, applied)])
    } else {
        trf(
            // TRANSLATORS: %1 is e.g. “the cursor “Vanilla-DMZ””, %2 is the name the theme asked for.
            "Also applied %1 that the theme comes with (the theme calls it “%2”).",
            &[&dep_label(role, applied), &wanted],
        )
    }
}

/// Строка итога: набора нет, и взять его негде.
pub fn dep_missing_line(role: DepRole, wanted: &str) -> String {
    let (what, place) = match role {
        DepRole::Icons => (tr("Your current icons stay."), crate::i18n::trc("appearance section", "Icons")),
        DepRole::Cursor => (tr("Your current cursor stays."), crate::i18n::trc("appearance section", "Cursors")),
    };
    trf(
        // TRANSLATORS: %1 is e.g. “the icon set “X””, %2 is “Your current icons stay.”,
        // %3 is a section of Rexon's Appearance tab.
        "⚠️ The theme comes with %1, but it isn't installed and Rexon doesn't know where to get it. %2 You can look for it in “%3”.",
        &[&dep_label(role, wanted), &what, &place],
    )
}

/// Строка итога: набор не поставлен (отказ от установки или сбой).
pub fn dep_not_installed_line(role: DepRole, wanted: &str, failure: Option<&str>) -> String {
    let what = match role {
        DepRole::Icons => tr("Your current icons stay."),
        DepRole::Cursor => tr("Your current cursor stays."),
    };
    match failure {
        None => trf(
            // TRANSLATORS: %1 is e.g. “the icon set “X””, %2 is “Your current icons stay.”
            "⚠️ The theme comes with %1, which isn't installed. %2 To install it, press “Apply” on the theme again.",
            &[&dep_label(role, wanted), &what],
        ),
        Some(why) => trf(
            // TRANSLATORS: %1 is e.g. “the icon set “X””, %2 is the reason, %3 is “Your current icons stay.”
            "⚠️ Couldn't install %1 that the theme comes with: %2. %3",
            &[&dep_label(role, wanted), &why, &what],
        ),
    }
}

/// Строка итога: набор стоит, но применить его не удалось.
pub fn dep_apply_failed_line(role: DepRole, name: &str) -> String {
    trf("⚠️ Couldn't apply %1 that the theme comes with.", &[&dep_label(role, name)])
}

// ============================================================================
// Установка темы из каталога — разрешение имени пакета (чистая часть)
// ============================================================================

/// Порт разрешения пакета из `install_popular_theme`. Возвращает
/// (pkg_name, aur_name) для данного `pkg_family` (нижний регистр). Fallback
/// на `pkg_debian` для не-arch семейств сохранён.
pub fn resolve_install(meta: &Value, pkg_family: &str) -> (String, String) {
    let family = pkg_family.to_lowercase();
    let key = match family.as_str() {
        "arch" => "pkg_arch",
        "debian" => "pkg_debian",
        "fedora" => "pkg_fedora",
        "suse" => "pkg_suse",
        "gentoo" => "pkg_gentoo",
        "alpine" => "pkg_alpine",
        "void" => "pkg_void",
        "solus" => "pkg_solus",
        "clear" => "pkg_clear",
        _ => "",
    };
    let meta_str = |k: &str| meta.get(k).and_then(Value::as_str).unwrap_or("").to_string();

    let mut pkg_name = if key.is_empty() { String::new() } else { meta_str(key) };
    // Fallback: pkg_debian для не-arch (Fedora/SUSE часто совпадают).
    if pkg_name.is_empty() && family != "arch" {
        pkg_name = meta_str("pkg_debian");
    }
    let aur_name = meta_str("aur");
    (pkg_name, aur_name)
}

/// argv установки из AUR — порт `[aur_helper, "-S", "--needed", "--noconfirm", aur]`.
pub fn aur_install_cmd(aur_helper: &str, aur_name: &str) -> Cmd {
    // Одна команда на весь Rexon (права окном polkit, ответы на вопросы
    // помощника) — `aur_install::install_argv`.
    Cmd {
        argv: crate::aur_install::install_argv(aur_helper, &[aur_name.to_string()], false),
        timeout: None,
    }
}

// ============================================================================
// Текущие темы (для отображения) — сборка из готовых значений источников
// ============================================================================

/// Готовые значения источников `get_current_themes` (исполнитель заполняет
/// только ветку своего DE). gsettings/xfconf-значения — уже без кавычек/пробелов
/// (как `r.stdout.strip().strip("'\"")`); None = ключ не получен.
#[derive(Default)]
pub struct CurrentSources<'a> {
    // gsettings-ветка (роли gtk/icons/cursor/font).
    pub gs_gtk: Option<&'a str>,
    pub gs_icons: Option<&'a str>,
    pub gs_cursor: Option<&'a str>,
    pub gs_font: Option<&'a str>,
    // xfce (xfconf-query).
    pub xf_gtk: Option<&'a str>,
    pub xf_icons: Option<&'a str>,
    pub xf_cursor: Option<&'a str>,
    pub xf_font: Option<&'a str>,
    // kde.
    pub kde_widget_style: Option<&'a str>,
    pub kde_icons: Option<&'a str>,
    pub kde_cursor_mouse: Option<&'a str>,
    pub kde_kcminput_cursor: &'a str,
    pub xcursor_env: &'a str,
    pub kde_font: Option<&'a str>,
    /// `_system_font_fallback()` — уже вычисленный шрифт по умолчанию.
    pub font_fallback: &'a str,
}

/// Порт `get_current_themes`: собрать `{gtk, icons, cursor, font}` из готовых
/// значений источников. Ветвление по DE — как в оригинале.
pub fn get_current_themes(de: &str, src: &CurrentSources) -> Map<String, Value> {
    let mut result = Map::new();
    let put = |m: &mut Map<String, Value>, k: &str, v: &str| {
        m.insert(k.to_string(), Value::from(v));
    };
    match resolve_schemas(de) {
        Schemas::Gsettings(list) => {
            for (role, _, _) in list {
                let v = match *role {
                    "gtk" => src.gs_gtk,
                    "icons" => src.gs_icons,
                    "cursor" => src.gs_cursor,
                    "font" => src.gs_font,
                    _ => None,
                };
                put(&mut result, role, v.unwrap_or("—"));
            }
            if result.get("font").and_then(Value::as_str) == Some("—") {
                put(&mut result, "font", src.font_fallback);
            }
        }
        Schemas::NonGsettings if de == "xfce" => {
            put(&mut result, "gtk", src.xf_gtk.unwrap_or("—"));
            put(&mut result, "icons", src.xf_icons.unwrap_or("—"));
            put(&mut result, "cursor", src.xf_cursor.unwrap_or("—"));
            put(&mut result, "font", src.xf_font.unwrap_or(src.font_fallback));
        }
        Schemas::NonGsettings if de == "kde" => {
            put(&mut result, "gtk", src.kde_widget_style.unwrap_or("—"));
            put(&mut result, "icons", src.kde_icons.unwrap_or("—"));
            // cursor: Mouse/cursorTheme → kcminputrc → $XCURSOR_THEME → breeze_cursors
            // (семантика Python `a or b or c or "breeze_cursors"` — пустая строка
            // считается ложью и уходит дальше по цепочке).
            let cursor = match src.kde_cursor_mouse {
                Some(s) if !s.is_empty() => s,
                _ if !src.kde_kcminput_cursor.is_empty() => src.kde_kcminput_cursor,
                _ if !src.xcursor_env.is_empty() => src.xcursor_env,
                _ => "breeze_cursors",
            };
            put(&mut result, "cursor", cursor);
            let font = match src.kde_font {
                Some(raw) if !raw.is_empty() => kde_font_to_display(raw),
                _ => src.font_fallback.to_string(),
            };
            put(&mut result, "font", &font);
        }
        Schemas::NonGsettings => {}
    }
    result
}

/// Порт `_kde_kcminput_cursor`: `cursorTheme=` из секции `[Mouse]` файла
/// `kcminputrc`. `content` — содержимое (None = файла нет) → пусто.
pub fn kde_kcminput_cursor(content: Option<&str>) -> String {
    let Some(text) = content else {
        return String::new();
    };
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
        } else if section == "Mouse" {
            if let Some(rest) = line.strip_prefix("cursorTheme=") {
                return rest.trim().to_string();
            }
        }
    }
    String::new()
}

/// Порт `_kde_font_to_display`: QFont `"Family,Size,-1,5,..."` → `"Family Size"`.
/// Пусто → пусто; без запятой → как есть; непарсибельный размер → 10.
pub fn kde_font_to_display(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    if !raw.contains(',') {
        return raw.to_string();
    }
    let parts: Vec<&str> = raw.split(',').collect();
    let family = parts[0].trim();
    // int(float(parts[1])) if len>1 else 10; ValueError → 10.
    let size = parts
        .get(1)
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|f| f as i64)
        .unwrap_or(10);
    format!("{family} {size}")
}

/// Порт `_system_font_fallback`: `gtk-font-name=` из gtk-4.0, затем gtk-3.0
/// `settings.ini`; иначе вывод `fc-match`; иначе `"Sans 10"`. `gtk4_ini` /
/// `gtk3_ini` — содержимое файлов (None = нет); `fc_match` — stdout (None/
/// пусто = недоступно).
pub fn system_font_fallback(
    gtk4_ini: Option<&str>,
    gtk3_ini: Option<&str>,
    fc_match: Option<&str>,
) -> String {
    for ini in [gtk4_ini, gtk3_ini] {
        let Some(text) = ini else { continue };
        for line in text.lines() {
            let s = line.trim();
            if let Some(rest) = s.strip_prefix("gtk-font-name=") {
                let val = rest.trim();
                if !val.is_empty() {
                    return val.to_string();
                }
            }
        }
    }
    if let Some(out) = fc_match {
        let out = out.trim();
        if !out.is_empty() {
            return out.to_string();
        }
    }
    "Sans 10".to_string()
}

// ============================================================================
// Санитайзер курсоров — порт gtk_sanitizer (pre-flight apply_cursor_theme)
// ============================================================================

/// Небезопасные курсорные темы — порт `UNSAFE_CURSOR_THEMES` (GTK4 4.22
/// SIGSEGV: нет `cursor.theme`, Inherits в icon-тему без `cursors/`).
pub const UNSAFE_CURSOR_THEMES: &[&str] = &["Adwaita", "AdwaitaLegacy"];

/// Порт `_is_cursor_theme_chain_safe`: цепочка Inherits не уводит GTK4 в
/// рекурсию. `theme_dir` — каталог темы; `parent_has_cursors` — есть ли у
/// родителя `cursors/` (проверяется исполнителем по /usr/share/icons и
/// ~/.local/share/icons). Безопасна, если: есть `cursor.theme`; нет
/// `index.theme`; нет `Inherits=`; или хоть один не-`hicolor` родитель имеет
/// `cursors/`.
pub fn is_cursor_theme_chain_safe<F>(
    fs: &FsSnapshot,
    theme_dir: &str,
    parent_has_cursors: F,
) -> bool
where
    F: Fn(&str) -> bool,
{
    if fs.is_file(&format!("{theme_dir}/cursor.theme")) {
        return true;
    }
    let idx = format!("{theme_dir}/index.theme");
    if !fs.is_file(&idx) {
        return true; // самодостаточная, без наследования
    }
    let Some(content) = fs.read_text(&idx) else {
        return true; // OSError при чтении → безопасна (как оригинал)
    };
    let mut parents: Vec<String> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Inherits=") {
            parents = rest.split(',').map(|p| p.trim().to_string()).collect();
            break;
        }
    }
    if parents.is_empty() {
        return true;
    }
    for parent in &parents {
        if parent == "hicolor" {
            continue; // терминатор для иконок, тупик для курсоров
        }
        if parent_has_cursors(parent) {
            return true;
        }
    }
    false
}

/// Порт pre-flight `apply_cursor_theme`: отказать unsafe-теме. `theme` в
/// [`UNSAFE_CURSOR_THEMES`] → false; иначе для первого каталога (из
/// `/usr/share/icons`, `~/.local/share/icons`), где есть `<theme>/cursors/`,
/// проверить [`is_cursor_theme_chain_safe`]. `parent_has_cursors` — тот же
/// колбэк. Возвращает `true` = применять можно.
pub fn cursor_preflight_ok<F>(
    fs: &FsSnapshot,
    theme: &str,
    home: &str,
    parent_has_cursors: F,
) -> bool
where
    F: Fn(&str) -> bool,
{
    if UNSAFE_CURSOR_THEMES.contains(&theme) {
        return false;
    }
    for base in ["/usr/share/icons".to_string(), format!("{home}/.local/share/icons")] {
        let d = format!("{base}/{theme}");
        if fs.is_dir(&d) && fs.is_dir(&format!("{d}/cursors")) {
            if !is_cursor_theme_chain_safe(fs, &d, &parent_has_cursors) {
                return false;
            }
            break;
        }
    }
    true
}

// ============================================================================
// История применений — порт _add_history / _save_history / get_recent
// ============================================================================

/// Порт записи `_add_history`: `{"kind","theme","de","ts"}`. `ts` (isoformat
/// timespec=seconds) недетерминирован → подаётся параметром.
pub fn history_entry(kind: &str, theme: &str, de: &str, ts: &str) -> Value {
    let mut m = Map::new();
    m.insert("kind".into(), Value::from(kind));
    m.insert("theme".into(), Value::from(theme));
    m.insert("de".into(), Value::from(de));
    m.insert("ts".into(), Value::from(ts));
    Value::Object(m)
}

/// Порт `_add_history`: вставить в начало, обрезать до 50.
pub fn push_history(history: &[Value], entry: Value) -> Vec<Value> {
    let mut out = Vec::with_capacity(history.len() + 1);
    out.push(entry);
    out.extend(history.iter().cloned());
    out.truncate(50);
    out
}

/// `json.dump(history, indent=2, ensure_ascii=False)` — порт `_save_history`.
pub fn serialize_history(history: &[Value]) -> String {
    serde_json::to_string_pretty(&Value::Array(history.to_vec())).expect("Value сериализуется")
}

/// Порт `get_recent`: `history[:n]`.
pub fn get_recent(history: &[Value], n: usize) -> Vec<Value> {
    history.iter().take(n).cloned().collect()
}

// ══════════════════════════════════════════════════════════════════════════
//  Установка темы из архива (порт `install_theme_from_archive`)
// ══════════════════════════════════════════════════════════════════════════

/// Куда распаковывать тему из архива: `base` — корень («~/.themes» или
/// «~/.local/share/icons»), `dest` — конечный каталог темы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDest {
    pub base: String,
    pub dest: String,
}

/// Порт выбора места установки в `install_theme_from_archive`: `kind == "gtk"`
/// → `~/.themes` (вне XDG — там его читают GTK/Qt), иначе — каталог иконок
/// в XDG-корне данных.
pub fn archive_install_dest(
    kind: &str,
    name: &str,
    home: &str,
    xdg_data_home: Option<&str>,
) -> ArchiveDest {
    let base = if kind == "gtk" {
        crate::utils::expand_user_xdg("~/.themes", home, None, xdg_data_home)
    } else {
        crate::utils::expand_user_xdg(
            "~/.local/share/icons",
            home,
            None,
            xdg_data_home,
        )
    };
    let dest = format!("{}/{name}", base.trim_end_matches('/'));
    ArchiveDest { base, dest }
}

/// Папка темы внутри распакованного архива.
///
/// ⚠ Правило #16: оригинал брал `[e for e in os.scandir(tmp) if e.is_dir()][0]`
/// — порядок `scandir` задаёт ФС, поэтому у архива с несколькими папками
/// устанавливалась СЛУЧАЙНАЯ. Порт берёт первую по алфавиту: тот же результат
/// для нормального архива (одна папка) и предсказуемый — для остальных.
pub fn pick_archive_dir(names: &[String]) -> Option<String> {
    names.iter().min().cloned()
}

/// `install_theme_from_archive`: в архиве нет ни одной папки.
pub fn msg_archive_no_dirs() -> String {
    crate::i18n::tr("The archive contains no folders")
}
/// Начало сообщения о неудачной распаковке (дальше — текст исключения).
pub fn msg_extract_failed_prefix() -> String {
    format!("{}: ", crate::i18n::tr("Unpacking error"))
}
/// `install_popular_theme`: темы нет во встроенном каталоге.
pub fn msg_theme_not_in_catalog_prefix() -> String {
    format!("{}: ", crate::i18n::tr("Theme not found in the catalog"))
}
/// `install_popular_theme`: поставили из репозитория дистрибутива.
pub fn msg_installed_from_repo_prefix() -> String {
    format!("{}: ", crate::i18n::tr("Theme installed from the repository"))
}
/// `install_popular_theme`: поставили из AUR.
pub fn msg_installed_from_aur_prefix() -> String {
    format!("{}: ", crate::i18n::tr("Theme installed from AUR"))
}
/// `install_popular_theme`: для этого дистрибутива пакета нет.
pub fn msg_no_package_prefix() -> String {
    format!("{}: ", crate::i18n::tr("No package for this distribution"))
}
/// `install_popular_theme`: установка не удалась, дальше — что пробовать руками.
pub fn msg_install_failed_prefix() -> String {
    format!("{}: ", crate::i18n::tr("Couldn't install. Try it manually"))
}

// ══════════════════════════════════════════════════════════════════════════
//  Чтение kdeglobals (порт `_kde_get`)
// ══════════════════════════════════════════════════════════════════════════

/// argv `kreadconfig6` для ключа kdeglobals — путь через KDE, чтобы подхватить
/// системные defaults и активный Look-and-Feel (не только файл в ~/.config).
pub fn kde_read_cmd(section: &str, key: &str) -> Cmd {
    Cmd {
        argv: vec![
            "kreadconfig6".into(),
            "--file".into(),
            "kdeglobals".into(),
            "--group".into(),
            section.to_string(),
            "--key".into(),
            key.to_string(),
            "--default".into(),
            String::new(),
        ],
        timeout: None,
    }
}

/// Фолбэк `_kde_get`: значение ключа прямо из текста kdeglobals.
///
/// Повторяет `configparser(strict=False)` на данных KDE: последнее вхождение
/// ключа в секции побеждает (дубликаты в kdeglobals — норма), значение
/// обрезается по краям, комментарии `#`/`;` вне значений пропускаются,
/// интерполяция `%(...)s` НЕ выполняется (как `configparser.get` без vars —
/// у KDE-значений вида `%d` она и так не сработала бы).
pub fn kdeglobals_value(content: &str, section: &str, key: &str) -> Option<String> {
    let mut cur = String::new();
    let mut found: Option<String> = None;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            cur = line[1..line.len() - 1].to_string();
            continue;
        }
        if cur != section {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            // configparser сравнивает ключи без учёта регистра (optionxform).
            if k.trim().eq_ignore_ascii_case(key) {
                found = Some(v.trim().to_string());
            }
        }
    }
    found
}

// ============================================================================
// «Настроить…» у стиля приложений — как в модуле KDE «Стиль приложений»
// ============================================================================
//
// KDE (plasma-workspace 6.8.2, `kcms/style`): у стиля есть свои настройки,
// когда в его описании `kstyle/themes/<стиль>.themerc` стоит
// `[Misc] ConfigPage=<плагин>` (Breeze — `kstyle_config/breezestyleconfig`,
// Oxygen — `kstyle_config/kstyle_oxygen_config`); модуль грузит этот плагин
// (`allocate_kstyle_config`) и показывает его окном «Настроить …». Только у
// таких стилей кнопка и есть. Тот же плагин без модуля показывают отдельные
// программы самих стилей — `breeze-settings6` и `oxygen-settings6`; их Rexon
// и зовёт, они работают и вне KDE. Плагин без своей программы открывается
// из самого модуля (`kcmshell6 kcm_style`). Kvantum — свой стиль без
// `.themerc`: его настройки — `kvantummanager`; у `qt6ct-style`/`qt5ct-style`
// — сами qt6ct/qt5ct.

/// Программы, которые показывают плагин настроек стиля без модуля KDE.
const STYLE_CONFIG_PROGRAMS: &[(&str, &str)] = &[
    ("kstyle_config/breezestyleconfig", "breeze-settings6"),
    ("kstyle_config/kstyle_oxygen_config", "oxygen-settings6"),
];

/// Чем открываются настройки стиля.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StyleConfigButton {
    /// Показывать ли кнопку (только у стилей приложений и тем Kvantum).
    pub visible: bool,
    /// Можно ли нажать: есть чем открыть.
    pub enabled: bool,
    pub text: String,
    /// Что откроется — или честная причина, почему нечем.
    pub tooltip: String,
    /// Команда (пусто — нажать нельзя).
    pub argv: Vec<String>,
}

/// `ConfigPage` стилей по описаниям `.themerc`: ключ — имя стиля
/// (`[KDE] WidgetStyle`) в нижнем регистре.
pub fn style_config_pages(themercs: &[String]) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for text in themercs {
        let style = ini_value_ci(text, "KDE", "WidgetStyle");
        let page = ini_value_ci(text, "Misc", "ConfigPage");
        if let (Some(style), Some(page)) = (style, page) {
            if !style.is_empty() && !page.is_empty() {
                out.insert(style.to_lowercase(), page);
            }
        }
    }
    out
}

/// Каталоги `kstyle/themes` по XDG (свой первым, как у QStandardPaths).
pub fn style_themerc_dirs(home: &str, xdg_data_home: Option<&str>, xdg_data_dirs: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let data_home = xdg_data_home
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{home}/.local/share"));
    out.push(format!("{data_home}/kstyle/themes"));
    let dirs = xdg_data_dirs.filter(|s| !s.trim().is_empty()).unwrap_or("/usr/local/share:/usr/share");
    for d in dirs.split(':').filter(|d| !d.is_empty()) {
        out.push(format!("{}/kstyle/themes", d.trim_end_matches('/')));
    }
    out
}

/// Кнопка «Настроить…» плитки раздела «Стиль приложений». `kind` — вид
/// плитки (`widgetstyle` — стиль Qt, `kvantum` — тема Kvantum), `name` —
/// имя стиля или темы, `pages` — [`style_config_pages`], `has` — есть ли
/// программа в PATH.
pub fn style_config_button(
    kind: &str,
    name: &str,
    pages: &std::collections::HashMap<String, String>,
    has: &dyn Fn(&str) -> bool,
) -> StyleConfigButton {
    let hidden = StyleConfigButton {
        visible: false,
        enabled: false,
        text: String::new(),
        tooltip: String::new(),
        argv: Vec::new(),
    };
    if kind != "widgetstyle" && kind != "kvantum" {
        return hidden;
    }
    let low = name.trim().to_lowercase();
    let open = |argv: Vec<String>, what: String| StyleConfigButton {
        visible: true,
        enabled: true,
        text: tr("Configure…"),
        tooltip: trf("Opens %1", &[&what]),
        argv,
    };
    let missing = |reason: String| StyleConfigButton {
        visible: true,
        enabled: false,
        text: tr("Configure…"),
        tooltip: reason,
        argv: Vec::new(),
    };
    // Kvantum: и сам движок (`kvantum`, `kvantum-dark`), и его темы.
    if kind == "kvantum" || low.starts_with("kvantum") {
        return if has("kvantummanager") {
            open(vec!["kvantummanager".into()], tr("Kvantum Manager"))
        } else {
            missing(tr("Kvantum settings are made in Kvantum Manager, and it isn't installed (package kvantum)."))
        };
    }
    for (style, program, title) in [("qt6ct-style", "qt6ct", "qt6ct"), ("qt5ct-style", "qt5ct", "qt5ct")] {
        if low == style {
            return if has(program) {
                open(vec![program.into()], title.to_string())
            } else {
                missing(trf("This style is set up in %1, and it isn't installed.", &[&title]))
            };
        }
    }
    let Some(page) = pages.get(&low) else {
        return missing(tr("This style has no settings of its own."));
    };
    if let Some((_, program)) = STYLE_CONFIG_PROGRAMS.iter().find(|(p, _)| p == page) {
        if has(program) {
            return open(vec![(*program).into()], trf("settings of the %1 style", &[&name.trim()]));
        }
    }
    if has("kcmshell6") {
        return open(
            vec!["kcmshell6".into(), "kcm_style".into()],
            tr("the KDE “Application Style” module — its “Configure…” button is next to the style"),
        );
    }
    missing(tr("The settings of this style open only from KDE System Settings, and they aren't installed."))
}

/// [`style_config_button`] на живой системе: описания стилей с диска,
/// программы — по PATH.
pub fn style_config_button_live(kind: &str, name: &str) -> StyleConfigButton {
    let home = std::env::var("HOME").unwrap_or_default();
    let dirs = style_themerc_dirs(
        &home,
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("XDG_DATA_DIRS").ok().as_deref(),
    );
    let mut texts = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "themerc") {
                if let Ok(t) = std::fs::read_to_string(&p) {
                    texts.push(t);
                }
            }
        }
    }
    let pages = style_config_pages(&texts);
    let path = std::env::var("PATH").ok();
    style_config_button(kind, name, &pages, &|p| crate::utils::which(p, path.as_deref()).is_some())
}

/// Значение ключа INI без учёта регистра имени группы и ключа.
fn ini_value_ci(text: &str, group: &str, key: &str) -> Option<String> {
    let mut here = false;
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            here = s[1..s.len() - 1].eq_ignore_ascii_case(group);
            continue;
        }
        if here {
            if let Some((k, v)) = s.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod deps_tests {
    use super::*;

    /// Решение 3а: о значках темы — один вопрос; запомненный ответ, стоящие
    /// уже значки и набор, которого нет нигде, вопроса не дают; у стоящего
    /// набора — «Включить», у недостающего — «Скачать и включить».
    #[test]
    fn theme_icons_are_asked_once_and_remembered() {
        let icons = |state: DepState| vec![DepItem { role: DepRole::Icons, wanted: "Windows XP".into(), state }];
        let pkg = icons(DepState::Installable("xp-icons".into()));
        let q = icons_question("Luna", &pkg, "breeze", None).expect("спросить");
        assert_eq!(q.yes, tr("Download and turn on"));
        assert_eq!(q.no, tr("Keep mine"));
        assert!(q.text.contains("Luna") && q.text.contains("Windows XP"), "{}", q.text);
        // Ответ запомнен — не спрашивать, каким бы он ни был.
        assert_eq!(icons_question("Luna", &pkg, "breeze", Some(true)), None);
        assert_eq!(icons_question("Luna", &pkg, "breeze", Some(false)), None);
        // Набор уже стоит — тот же вопрос, но «Включить».
        let here = icons(DepState::Installed("Windows XP".into()));
        assert_eq!(icons_question("Luna", &here, "breeze", None).expect("спросить").yes, tr("Turn on"));
        // Он и так включён — спрашивать не о чем.
        assert_eq!(icons_question("Luna", &here, "Windows XP", None), None);
        // Нет нигде — о нём скажет итог, не вопрос.
        assert_eq!(icons_question("Luna", &icons(DepState::Missing), "breeze", None), None);
        // Тема о значках не просит.
        let cursor = vec![DepItem { role: DepRole::Cursor, wanted: "DMZ-White".into(), state: DepState::Missing }];
        assert_eq!(icons_question("Luna", &cursor, "breeze", None), None);

        // Память — по имени темы, прочие ответы не теряются.
        let v = icons_choice_with(None, "Luna", false);
        assert_eq!(icons_choice_of(Some(&v), "Luna"), Some(false));
        let v = icons_choice_with(Some(&v), "Royale", true);
        assert_eq!(icons_choice_of(Some(&v), "Luna"), Some(false));
        assert_eq!(icons_choice_of(Some(&v), "Royale"), Some(true));
        assert_eq!(icons_choice_of(Some(&v), "Other"), None);
        assert_eq!(icons_choice_of(None, "Luna"), None);
    }

    /// Домашний архив WhiteSur несёт только WhiteSur, -dark и -light:
    /// цветной вариант из него не достать — его и не качаем (иначе три
    /// ненужных набора в ~/.local/share/icons и «не принесла» при каждом
    /// «Применить»).
    #[test]
    fn home_archive_is_offered_only_for_names_it_brings() {
        for name in ["WhiteSur", "WhiteSur-Dark", "whitesur-light"] {
            assert!(dep_home_source(DepRole::Icons, name).is_some(), "{name}");
        }
        for name in ["WhiteSur-purple-dark", "WhiteSur-nord"] {
            assert_eq!(dep_home_source(DepRole::Icons, name), None, "{name}");
        }
        // Пакет при этом по-прежнему знает варианты.
        assert!(KNOWN_SETS.iter().any(|s| known_set_matches(s, DepRole::Icons, "WhiteSur-nord")));
    }

    /// Имя набора из чужого index.theme — одно имя каталога, не путь.
    #[test]
    fn theme_deps_ignore_path_like_names() {
        let d = theme_deps("[Desktop Entry]\nIconTheme=WhiteSur-/../../x\nCursorTheme=../c\n");
        assert_eq!(d, ThemeDeps::default());
    }
    use std::collections::{HashMap, HashSet};

    /// Мнимый диск: каталоги и файлы с текстом.
    struct Disk {
        dirs: HashSet<String>,
        files: HashMap<String, String>,
    }

    impl Disk {
        fn new(dirs: &[&str], files: &[(&str, &str)]) -> Self {
            Disk {
                dirs: dirs.iter().map(|d| d.to_string()).collect(),
                files: files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect(),
            }
        }
        fn find(&self, role: DepRole, wanted: &str, roots: &[String]) -> Option<String> {
            let list = |root: &str| -> Vec<String> {
                let prefix = format!("{root}/");
                self.dirs
                    .iter()
                    .filter_map(|d| d.strip_prefix(&prefix))
                    .filter(|rest| !rest.contains('/'))
                    .map(str::to_string)
                    .collect()
            };
            find_installed_dep(
                role,
                wanted,
                roots,
                &|p| self.dirs.contains(p),
                &list,
                &|p| self.files.get(p).cloned(),
            )
        }
    }

    const LUNA: &str = "[Desktop Entry]\nType=X-GNOME-Metatheme\nName=Windows XP Luna\n\n\
        [X-GNOME-Metatheme]\nGtkTheme=Windows XP Luna\nMetacityTheme=Windows XP Luna\n\
        IconTheme=Windows XP\nCursorTheme=DMZ-White\n";

    fn roots() -> Vec<String> {
        dep_search_roots("/h", "/h/.local/share", "")
    }

    #[test]
    fn deps_read_from_metatheme_and_desktop_entry() {
        assert_eq!(
            theme_deps(LUNA),
            ThemeDeps { icons: Some("Windows XP".into()), cursor: Some("DMZ-White".into()) }
        );
        // Ключи в [Desktop Entry] (так пишут часть тем) — тоже просьба.
        let de = theme_deps("[Desktop Entry]\nName=X\nIconTheme=Papirus-Dark\n");
        assert_eq!(de.icons.as_deref(), Some("Papirus-Dark"));
        assert_eq!(de.cursor, None);
        // Пустое значение, hicolor (наш импорт .msstyles) и чужая секция — не просьба.
        let none = theme_deps("[X-GNOME-Metatheme]\nIconTheme=hicolor\nCursorTheme=\n[Other]\nIconTheme=Papirus\n");
        assert_eq!(none, ThemeDeps::default());
    }

    #[test]
    fn search_roots_follow_xdg() {
        assert_eq!(
            dep_search_roots("/h", "/h/.local/share", "/opt/share:/usr/share/"),
            vec!["/h/.icons", "/h/.local/share/icons", "/opt/share/icons", "/usr/share/icons"]
        );
        // Без XDG_DATA_DIRS — значение по спецификации.
        assert_eq!(
            gtk_theme_search_roots("/h", "/d", ""),
            vec!["/h/.themes", "/d/themes", "/usr/local/share/themes", "/usr/share/themes"]
        );
    }

    #[test]
    fn dmz_white_is_vanilla_dmz() {
        // Тема просит «DMZ-White», пакет Arch поставил «Vanilla-DMZ».
        let disk = Disk::new(&["/usr/share/icons/Vanilla-DMZ", "/usr/share/icons/Vanilla-DMZ/cursors"], &[]);
        assert_eq!(disk.find(DepRole::Cursor, "DMZ-White", &roots()).as_deref(), Some("Vanilla-DMZ"));
        // Чёрный DMZ — другой набор: белым его не закрыть.
        assert_eq!(disk.find(DepRole::Cursor, "DMZ-Black", &roots()), None);
        // Точное имя выигрывает у псевдонима.
        let both = Disk::new(
            &[
                "/usr/share/icons/Vanilla-DMZ",
                "/usr/share/icons/Vanilla-DMZ/cursors",
                "/h/.icons/DMZ-White",
                "/h/.icons/DMZ-White/cursors",
            ],
            &[],
        );
        assert_eq!(both.find(DepRole::Cursor, "DMZ-White", &roots()).as_deref(), Some("DMZ-White"));
    }

    #[test]
    fn icons_need_directories_and_case_is_forgiven() {
        let disk = Disk::new(
            &["/h/.local/share/icons/WhiteSur-dark", "/h/.icons/Cursorish", "/h/.icons/Cursorish/cursors"],
            &[
                ("/h/.local/share/icons/WhiteSur-dark/index.theme", "[Icon Theme]\nName=WhiteSur-dark\nDirectories=48x48/apps\n"),
                // Курсорная тема с index.theme без Directories= — не набор иконок.
                ("/h/.icons/Cursorish/index.theme", "[Icon Theme]\nName=Cursorish\n"),
            ],
        );
        assert_eq!(disk.find(DepRole::Icons, "WhiteSur-Dark", &roots()).as_deref(), Some("WhiteSur-dark"));
        assert_eq!(disk.find(DepRole::Icons, "Cursorish", &roots()), None);
        assert_eq!(disk.find(DepRole::Cursor, "Cursorish", &roots()).as_deref(), Some("Cursorish"));
    }

    #[test]
    fn plan_installed_installable_missing() {
        let disk = Disk::new(
            &["/h/.local/share/icons/Windows XP"],
            &[("/h/.local/share/icons/Windows XP/index.theme", "[Icon Theme]\nDirectories=16x16/apps\n")],
        );
        let find = |role: DepRole, wanted: &str| disk.find(role, wanted, &roots());
        let plan = plan_theme_deps(&theme_deps(LUNA), "arch", &find);
        assert_eq!(
            plan,
            vec![
                DepItem { role: DepRole::Icons, wanted: "Windows XP".into(), state: DepState::Installed("Windows XP".into()) },
                DepItem {
                    role: DepRole::Cursor,
                    wanted: "DMZ-White".into(),
                    state: DepState::Installable("xcursor-vanilla-dmz".into()),
                },
            ]
        );
        // Ничего не стоит, семейство без пакета — честное «нет».
        let empty = Disk::new(&[], &[]);
        let find = |role: DepRole, wanted: &str| empty.find(role, wanted, &roots());
        let plan = plan_theme_deps(&theme_deps(LUNA), "gentoo", &find);
        assert!(plan.iter().all(|i| i.state == DepState::Missing));
        // Ставится само только то, что можно поставить.
        assert!(deps_to_prepare(&plan, true).iter().all(|i| dep_home_source(i.role, &i.wanted).is_some()));
    }

    /// «Применить» у GTK-темы: курсор ставится сам, без вопроса; значки — по
    /// ответу «Скачать и включить»; не встало — тема не применяется.
    #[test]
    fn theme_deps_install_without_question_all_or_nothing() {
        let cursor = DepItem { role: DepRole::Cursor, wanted: "DMZ-White".into(), state: DepState::Installable("xcursor-vanilla-dmz".into()) };
        let icons = DepItem { role: DepRole::Icons, wanted: "Mint-Y".into(), state: DepState::Installable("mint-y-icons".into()) };
        let have = DepItem { role: DepRole::Icons, wanted: "Adwaita".into(), state: DepState::Installed("Adwaita".into()) };
        let nowhere = DepItem { role: DepRole::Cursor, wanted: "No-Such-Cursor".into(), state: DepState::Missing };
        let items = vec![cursor.clone(), icons.clone(), have, nowhere];
        assert_eq!(deps_to_prepare(&items, false), vec![cursor.clone()]);
        assert_eq!(deps_to_prepare(&items, true), vec![cursor.clone(), icons.clone()]);
        // Пароль не подтверждён — совет повторить; причина не повторяется.
        let pw = crate::appearance_installer::password_not_confirmed();
        let text = deps_refusal("Luna", &[cursor.clone()], &pw);
        assert!(text.contains("Luna") && text.contains("DMZ-White") && !text.contains(&pw), "{text}");
        // Пакета нет в официальных репозиториях (только AUR) — тема не
        // применяется, и сказано, чего нет.
        let text = deps_refusal("Luna", &[icons], "not in the official repositories: mint-y-icons");
        assert!(text.contains("Mint-Y") && text.contains("mint-y-icons"), "{text}");
    }

    #[test]
    fn packages_by_family_and_variants() {
        assert_eq!(dep_package(DepRole::Cursor, "DMZ-White", "debian").as_deref(), Some("dmz-cursor-theme"));
        assert_eq!(dep_package(DepRole::Icons, "Mint-Y", "arch").as_deref(), Some("mint-y-icons"));
        assert_eq!(dep_package(DepRole::Icons, "WhiteSur-Dark", "arch").as_deref(), Some("whitesur-icon-theme"));
        assert_eq!(dep_package(DepRole::Cursor, "WhiteSur-cursors", "arch").as_deref(), Some("whitesur-cursor-theme-git"));
        assert_eq!(dep_package(DepRole::Icons, "Papirus-Dark", "fedora").as_deref(), Some("papirus-icon-theme-dark"));
        assert_eq!(dep_package(DepRole::Icons, "Papirus-Dark", "debian").as_deref(), Some("papirus-icon-theme"));
        assert_eq!(dep_package(DepRole::Icons, "Papirus", "fedora").as_deref(), Some("papirus-icon-theme"));
        assert_eq!(dep_package(DepRole::Cursor, "DMZ-White", "fedora"), None);
        assert_eq!(dep_package(DepRole::Cursor, "DMZ-White", "suse").as_deref(), Some("dmz-icon-theme-cursors"));
        assert_eq!(dep_package(DepRole::Icons, "Windows XP", "arch"), None);
        assert_eq!(dep_package(DepRole::Icons, "Mint-Y-Dark", "debian").as_deref(), Some("mint-y-icons"));
        assert_eq!(dep_package(DepRole::Icons, "WhiteSur", "debian"), None);
        // «Papirusoid» — не вариант Papirus.
        assert_eq!(dep_package(DepRole::Icons, "Papirusoid", "arch"), None);
    }
}
