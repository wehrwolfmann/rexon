//! Порт чистой части точек входа Qt — `rexon/qt_main/app.py` и четырёх
//! мелких (`qt_extensions/app.py`, `qt_settings/app.py`, `qt_desktops/app.py`,
//! `qt_compare/app.py`).
//!
//! Маршрутизация флагов и аварийный откат KDE живут в [`crate::app`] (порт
//! `rexon/app.py`) — сюда переехало то, чего там нет: **отчёт о крахе** и
//! **идентичность приложения Qt**.
//!
//! ГРАНИЦА. В ядре — решения, тексты и форматы:
//!   * шапка отчёта, имя файла, ротация каталога, содержимое диалога,
//!     решение «печатать/сохранять/показывать» по типу исключения и потоку;
//!   * таблица `applicationName`/`applicationDisplayName`/`organizationName`/
//!     `desktopFileName` для каждой точки входа.
//!
//! За Qt-слоем остаётся исполнение: сам `QApplication` и вызовы сеттеров,
//! `QMessageBox` (сборка окна по [`CrashDialog`]), буфер обмена, запуск
//! `xdg-open`, `faulthandler`/`SIGUSR1`, guard от SEGV нативных диалогов
//! (KDE 521894), логгер сообщений Qt, скин и прозрачная иконка окна. Ядро не
//! пишет на диск: файловые операции отчёта идут через трейт [`CrashIo`]
//! (как `ImpIo` в `importers`), поэтому тесты реальную ФС не трогают.
//!
//! ГРАБЛИ ОРИГИНАЛА ПОЧИНЕНЫ (правило #16, каждая помечена `Починка:` по
//! месту):
//!   1. `_crash_header()` звался ДВАЖДЫ — отдельно для файла и отдельно для
//!      диалога, поэтому строка `Time:` в сохранённом логе и в тексте окна
//!      расходились (а по ним потом сопоставляют отчёт покупателя с логом);
//!      здесь шапка строится один раз и передаётся обоим.
//!   2. `SystemExit` не был исключением-«молчуном». В рабочем потоке
//!      `sys.exit()` доходит до `threading.excepthook` (проверено на CPython
//!      3.14), и штатный выход воркера показывал покупателю «Rexon
//!      неожиданно завершился» и писал отчёт о крахе; здесь `SystemExit`
//!      приравнен к `KeyboardInterrupt`.
//!   3. Диалог строился прямо в том потоке, где случилось исключение:
//!      `threading.excepthook` → `QMessageBox` вне потока GUI = abort Qt.
//!      Показ диалога не отменён (иначе покупатель Steam опять ничего не
//!      увидит) — решение несёт флаг
//!      [`CrashDecision::dialog_needs_main_thread_hop`], по которому Qt-слой
//!      обязан перебросить показ в поток GUI.
//!   4. Путь сохранённого лога терялся, если после успешной записи падал
//!      обход каталога: ротация была внутри общего `try`, и функция
//!      возвращала `None` при уже существующем файле — диалог тогда молчал
//!      про сохранённый отчёт. Здесь ошибка ротации путь не отменяет.
//!
//! Эталон — `tests/golden/qt_app_cases.json`, снятый прогоном ЖИВОГО
//! `rexon.qt_main.app`: `_crash_header` с подменёнными источниками времени и
//! окружения, `_save_crash_log` во временном каталоге, `_show_crash_dialog` и
//! `_install_excepthook` с записывающими заглушками Qt. Сверка —
//! `tests/qt_app_parity.rs`.

use crate::ext_utils::py_repr_str;
use crate::i18n::{tr, trf};

// ============================================================================
// Платформа Qt
// ============================================================================

/// Надо ли снять `QT_QPA_PLATFORM` из окружения.
///
/// Под KDE Plasma 6 на Wayland ПУСТАЯ переменная мешает Qt выбрать нативный
/// wayland-плагин, поэтому её удаляют. Непустое значение (в том числе `xcb`)
/// уважается: форсировать `xcb` нельзя — KWin не рисует SSD для xcb-окон
/// через Xwayland, окно остаётся без заголовка и кнопок.
pub fn should_unset_qt_platform(value: Option<&str>) -> bool {
    value == Some("")
}

// ============================================================================
// Идентичность приложения Qt
// ============================================================================

/// Организация — общая на все точки входа.
pub const ORGANIZATION_NAME: &str = "OpenAxon";

/// Имя `.desktop`-файла без расширения. KDE Plasma 6 / Wayland по нему
/// связывает процесс с `/usr/share/applications/rexon.desktop` и берёт оттуда
/// иконку для панели задач; без него app_id пустой и плазма рисует дефолтный
/// логотип Wayland.
pub const DESKTOP_FILE_NAME: &str = "rexon";

/// Что точка входа сообщает о себе `QApplication`. `None` — сеттер в
/// оригинале не вызывается вовсе (это НЕ то же самое, что пустая строка).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QtIdentity {
    pub application_name: &'static str,
    pub display_name: Option<&'static str>,
    pub organization_name: Option<&'static str>,
    pub desktop_file_name: Option<&'static str>,
}

/// Идентичность по имени Python-модуля точки входа. Ключи совпадают со
/// значениями [`crate::app::SINGLE_TAB_FLAGS`], чтобы маршрут и идентичность
/// не могли разойтись (правило #12).
///
/// Первые шесть — окна с вкладками. Остальные пять — хосты движков обоев
/// (класс D плана ухода с PySide6): они ставят только имя приложения и
/// `desktopFileName`, организацию и отображаемое имя не трогают.
pub const ENTRY_IDENTITIES: &[(&str, QtIdentity)] = &[
    (
        "rexon.qt_main.app",
        QtIdentity {
            application_name: "Rexon",
            // Пустая строка — НАМЕРЕННО, а не «не задано»: KWin/Qt клеит
            // displayName в хвост заголовка («<title> — <displayName>»), а все
            // окна и так начинаются с «Rexon — …», и «Rexon» дублировался бы.
            display_name: Some(""),
            organization_name: Some(ORGANIZATION_NAME),
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.appearance_page",
        QtIdentity {
            application_name: "Rexon Appearance",
            display_name: Some("Rexon — Оформление"),
            organization_name: Some(ORGANIZATION_NAME),
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.qt_extensions.app",
        QtIdentity {
            application_name: "Rexon Extensions",
            display_name: Some("Rexon — Расширения"),
            organization_name: Some(ORGANIZATION_NAME),
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.qt_compare.app",
        QtIdentity {
            application_name: "Rexon Compare",
            display_name: Some("Rexon — Сравнение рабочих столов"),
            organization_name: Some(ORGANIZATION_NAME),
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.qt_settings.app",
        QtIdentity {
            application_name: "Rexon Settings",
            display_name: Some("Rexon — Настройки"),
            organization_name: Some(ORGANIZATION_NAME),
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.qt_desktops.app",
        QtIdentity {
            application_name: "Rexon Desktops",
            display_name: Some("Rexon — Рабочие столы"),
            organization_name: Some(ORGANIZATION_NAME),
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.web_wallpaper",
        QtIdentity {
            application_name: "RexonWebWallpaper",
            display_name: None,
            organization_name: None,
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.video_wallpaper",
        QtIdentity {
            application_name: "RexonVideoWallpaper",
            display_name: None,
            organization_name: None,
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.scene_wallpaper",
        QtIdentity {
            application_name: "RexonSceneWallpaper",
            display_name: None,
            organization_name: None,
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.app_wallpaper",
        QtIdentity {
            application_name: "RexonAppWallpaper",
            display_name: None,
            organization_name: None,
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.parallax_engine",
        QtIdentity {
            application_name: "RexonSceneEngine",
            display_name: None,
            organization_name: None,
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.wallpaper_mmap",
        QtIdentity {
            application_name: "RexonWallpaperHeadless",
            display_name: None,
            organization_name: None,
            desktop_file_name: None,
        },
    ),
];

/// Идентичность точки входа по имени Python-модуля.
pub fn identity(module: &str) -> Option<QtIdentity> {
    ENTRY_IDENTITIES
        .iter()
        .find(|(name, _)| *name == module)
        .map(|(_, id)| *id)
}

// ============================================================================
// Время отчёта
// ============================================================================

/// Момент времени без часового пояса — ровно то, что оригинал берёт из
/// `datetime.datetime.now()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Timestamp {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub microsecond: u32,
}

impl Timestamp {
    /// МЕСТНОЕ время прямо сейчас — порт `datetime.datetime.now()`.
    ///
    /// `None` — часовой пояс не читается (сломанный `/etc/localtime`,
    /// урезанный контейнер). Это НЕ повод падать: вызывающий решает сам, чем
    /// заменить метку.
    ///
    /// ⚠️ Именно МЕСТНОЕ, а не UTC: этой меткой называются копии панели и
    /// файлы отчётов, и человек ищет их глазами по своему времени. Второго
    /// обращения к `localtime_r` в продукте быть не должно (правило #12) —
    /// всё, чему нужны местные часы, зовёт эту функцию.
    pub fn now_local() -> Option<Self> {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        let t = epoch.as_secs() as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
            return None;
        }
        Some(Timestamp {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
            microsecond: epoch.subsec_micros(),
        })
    }

    /// Порт `datetime.isoformat()`: дробная часть печатается ТОЛЬКО когда
    /// микросекунды не нулевые (шесть знаков, без округления до миллисекунд).
    pub fn isoformat(&self) -> String {
        let base = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        );
        if self.microsecond == 0 {
            base
        } else {
            format!("{base}.{:06}", self.microsecond)
        }
    }

    /// Порт `strftime("%Y%m%d_%H%M%S")` — основа имени файла отчёта. Формат
    /// выбран так, что сортировка имён по алфавиту = сортировка по времени,
    /// на этом держится ротация.
    pub fn file_stamp(&self) -> String {
        format!(
            "{:04}{:02}{:02}_{:02}{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

// ============================================================================
// Шапка отчёта
// ============================================================================

/// Длина разделительной линии в шапке (символ `─`).
pub const CRASH_HEADER_RULE_WIDTH: usize = 60;

/// Источники шапки. Всё, что оригинал читает из `platform`, `PySide6` и
/// окружения, — снаружи: ядро ни к системе, ни к Qt не обращается.
#[derive(Debug, Clone, Default)]
pub struct CrashHeaderInput<'a> {
    pub now: Timestamp,
    /// `platform.python_version()`.
    pub python_version: &'a str,
    /// `PySide6.__version__`; `None` — импорт не удался (в шапке `?`).
    pub pyside_version: Option<&'a str>,
    /// `platform.platform()`.
    pub os_platform: &'a str,
    pub xdg_current_desktop: Option<&'a str>,
    pub desktop_session: Option<&'a str>,
    pub xdg_session_type: Option<&'a str>,
    pub lang: Option<&'a str>,
}

/// Шапка отчёта о крахе — байт-в-байт как в оригинале.
///
/// ⚠️ Два поля живут по РАЗНЫМ правилам, и это не описка оригинала:
/// рабочий стол берётся через `or` (пустая строка проваливается в
/// `DESKTOP_SESSION`, потом в `?`), а тип сессии и локаль — через
/// `get(..., "?")`, где `?` подставляется только при ОТСУТСТВИИ переменной, а
/// пустая строка печатается пустой.
pub fn crash_header(input: &CrashHeaderInput) -> String {
    let pyside = input.pyside_version.unwrap_or("?");
    crash_header_with(
        &input.now,
        &[("Python", input.python_version), ("PySide6", pyside)],
        &CrashHeaderEnv {
            os_platform: input.os_platform,
            xdg_current_desktop: input.xdg_current_desktop,
            desktop_session: input.desktop_session,
            xdg_session_type: input.xdg_session_type,
            lang: input.lang,
        },
    )
}

/// Ширина подписи поля шапки вместе с двоеточием и отбивкой: `Time:     `.
/// Значения всех полей начинаются с одной колонки — отчёты обоих входов
/// читаются глазами одинаково.
const CRASH_HEADER_LABEL_WIDTH: usize = 10;

/// Окружение, общее для шапок обоих входов (Python-хозяин и Rust-окно).
#[derive(Debug, Clone, Copy, Default)]
pub struct CrashHeaderEnv<'a> {
    /// Строка ОС в виде `platform.platform()` — см. [`os_platform`].
    pub os_platform: &'a str,
    pub xdg_current_desktop: Option<&'a str>,
    pub desktop_session: Option<&'a str>,
    pub xdg_session_type: Option<&'a str>,
    pub lang: Option<&'a str>,
}

/// Общий построитель шапки: `runtime` — строки «чем собран процесс» между
/// `Time:` и `OS:` (у Python-хозяина это Python и PySide6, у Rust-окна —
/// версия Rexon и Qt). Всё прочее — одно на оба входа (правило #12).
pub fn crash_header_with(now: &Timestamp, runtime: &[(&str, &str)], env: &CrashHeaderEnv) -> String {
    let de = first_nonempty(&[env.xdg_current_desktop, env.desktop_session]).unwrap_or("?");
    let sess = env.xdg_session_type.unwrap_or("?");
    let lang = env.lang.unwrap_or("?");
    let rule: String = "─".repeat(CRASH_HEADER_RULE_WIDTH);
    let field = |label: &str, value: &str| {
        format!("{:<w$}{value}\n", format!("{label}:"), w = CRASH_HEADER_LABEL_WIDTH)
    };
    let mut out = String::from("Rexon crash report\n");
    out.push_str(&field("Time", &now.isoformat()));
    for (label, value) in runtime {
        out.push_str(&field(label, value));
    }
    out.push_str(&field("OS", env.os_platform));
    out.push_str(&field("DE", &format!("{de} ({sess})")));
    out.push_str(&field("Locale", lang));
    out.push_str(&rule);
    out.push_str("\n\n");
    out
}

/// Источники шапки отчёта Rust-окна. Всё, что читается из системы, —
/// снаружи (`rexon-gui::crash_report`).
#[derive(Debug, Clone, Default)]
pub struct RustCrashHeaderInput<'a> {
    pub now: Timestamp,
    /// Версия Rexon (`CARGO_PKG_VERSION` крейта окна).
    pub rexon_version: &'a str,
    /// Имя исполняемого файла, в котором случилась паника.
    pub entry: &'a str,
    /// `qVersion()` загруженной Qt; `None` — неизвестна (в шапке `?`).
    pub qt_version: Option<&'a str>,
    pub env: CrashHeaderEnv<'a>,
}

/// Шапка отчёта Rust-окна: те же поля и та же раскладка, что у
/// Python-хозяина, отличаются только строки среды выполнения
/// (`Rexon:`/`Qt:` вместо `Python:`/`PySide6:`).
pub fn rust_crash_header(input: &RustCrashHeaderInput) -> String {
    let rexon = if input.entry.is_empty() {
        input.rexon_version.to_string()
    } else {
        format!("{} ({})", input.rexon_version, input.entry)
    };
    crash_header_with(
        &input.now,
        &[("Rexon", &rexon), ("Qt", input.qt_version.unwrap_or("?"))],
        &input.env,
    )
}

/// Строка ОС в виде `platform.platform()` Python на Linux:
/// `Linux-7.2.3-1-cachyos-x86_64-with-glibc2.44`. Одна запись для обоих
/// входов — чтобы строки `OS:` в отчётах сравнивались как есть.
///
/// `libc` — `(имя, версия)`, например `("glibc", "2.44")`; `None` — хвоста
/// `-with-…` нет (musl и прочие, как у Python).
pub fn os_platform(system: &str, release: &str, machine: &str, libc: Option<(&str, &str)>) -> String {
    let mut parts: Vec<String> = [system, release, machine]
        .iter()
        .map(|s| s.trim().replace(' ', "_"))
        .filter(|s| !s.is_empty())
        .collect();
    if let Some((name, ver)) = libc {
        parts.push("with".to_string());
        parts.push(format!("{name}{ver}"));
    }
    parts.join("-")
}

/// Первое непустое значение — порт цепочки `a or b`.
fn first_nonempty<'a>(values: &[Option<&'a str>]) -> Option<&'a str> {
    values
        .iter()
        .find_map(|v| v.filter(|s| !s.is_empty()))
}

// ============================================================================
// Сохранение отчёта и ротация каталога
// ============================================================================

/// Сколько файлов остаётся в каталоге отчётов после ротации.
pub const CRASH_KEEP: usize = 20;

/// Префикс имени файла отчёта; по нему же отбираются кандидаты на удаление,
/// поэтому чужие файлы в каталоге (README и прочее) ротация не трогает.
pub const CRASH_PREFIX: &str = "crash_";

/// Каталог отчётов: `~/.config/rexon/crashes` с поправкой на
/// `$XDG_CONFIG_HOME` (оригинал строит его через `utils.expand_user`).
pub fn crash_dir(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/rexon/crashes",
        crate::utils::config_home(xdg_config_home, home)
    )
}

/// Имя файла отчёта для указанного момента времени.
pub fn crash_log_name(ts: &Timestamp) -> String {
    format!("{CRASH_PREFIX}{}.log", ts.file_stamp())
}

/// Какие имена уходят при ротации: из отсортированного списка отчётов
/// выкидывается всё, кроме последних `keep`. Файлы без префикса не
/// рассматриваются вовсе.
pub fn rotation_victims(names: &[String], keep: usize) -> Vec<String> {
    let mut files: Vec<String> = names
        .iter()
        .filter(|n| n.starts_with(CRASH_PREFIX))
        .cloned()
        .collect();
    files.sort();
    let cut = files.len().saturating_sub(keep);
    files.truncate(cut);
    files
}

/// Файловые операции отчёта. Каждый метод — ровно один побочный вызов
/// оригинала, в порядке исполнения; `false`/`None` = исключение поймано
/// (в Python весь блок обёрнут в `try/except Exception`).
pub trait CrashIo {
    /// `os.makedirs(path, exist_ok=True)`.
    fn makedirs(&mut self, path: &str) -> bool;
    /// `open(path, "w", encoding="utf-8").write(text)`.
    fn write_text(&mut self, path: &str, text: &str) -> bool;
    /// `os.listdir(path)`.
    fn list_dir(&mut self, path: &str) -> Option<Vec<String>>;
    /// `os.remove(path)`; ошибка глотается самим оригиналом.
    fn remove(&mut self, path: &str);
}

/// Сохранить отчёт с ротацией. Возвращает путь к файлу либо `None`, если
/// каталог не создался или запись не удалась.
///
/// `header` приходит снаружи и тем же значением уходит в диалог — см. починку
/// №1 в шапке модуля.
///
/// Починка №4: ошибка обхода каталога больше не отменяет уже записанный путь.
pub fn save_crash_log(
    io: &mut dyn CrashIo,
    dir: &str,
    ts: &Timestamp,
    header: &str,
    crash_text: &str,
) -> Option<String> {
    if !io.makedirs(dir) {
        return None;
    }
    let path = format!("{dir}/{}", crash_log_name(ts));
    if !io.write_text(&path, &format!("{header}{crash_text}")) {
        return None;
    }
    if let Some(names) = io.list_dir(dir) {
        for victim in rotation_victims(&names, CRASH_KEEP) {
            io.remove(&format!("{dir}/{victim}"));
        }
    }
    Some(path)
}

/// Живой исполнитель [`CrashIo`] — ОДИН на оба входа (Python-мост и
/// Rust-окно), поэтому живёт здесь, а не в каждом из них.
///
/// Запись атомарная: текст уходит во временный файл рядом
/// (`.<имя>.part-<pid>`, без префикса [`CRASH_PREFIX`] — ротация его не
/// видит), сбрасывается на диск и только потом переименовывается в
/// окончательное имя. Отчёт пишется в момент краха, и процесс может умереть
/// посреди записи (паника через границу FFI — это `abort` сразу после
/// перехватчика): при записи по месту в каталоге остался бы обрезанный
/// `crash_*.log`, который выглядит как настоящий. Здесь файл с таким именем
/// либо полный, либо его нет.
pub struct AtomicCrashIo;

impl CrashIo for AtomicCrashIo {
    fn makedirs(&mut self, path: &str) -> bool {
        std::fs::create_dir_all(path).is_ok()
    }

    fn write_text(&mut self, path: &str, text: &str) -> bool {
        atomic_write(path, text).is_ok()
    }

    fn list_dir(&mut self, path: &str) -> Option<Vec<String>> {
        let rd = std::fs::read_dir(path).ok()?;
        Some(
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
        )
    }

    fn remove(&mut self, path: &str) {
        let _ = std::fs::remove_file(path);
    }
}

/// Атомарная запись — ОДНА на весь продукт, живёт в [`crate::utils`].
///
/// Здесь она только переобъявлена под прежним именем: отчёт о крахе был
/// первым, кто её получил, и зовут её отсюда оба окна (`rexon-gui`
/// и мост в Python). Тело переехало в общее место, когда выяснилось, что
/// мимо этой двери в профиль покупателя пишет ещё полтора десятка мест.
pub use crate::utils::atomic_write;

// ============================================================================
// Диалог о крахе
// ============================================================================

/// Заголовок окна (на языке интерфейса).
pub fn crash_dialog_title() -> String {
    // TRANSLATORS: title of the window shown after Rexon crashes.
    tr("Rexon — An Error Occurred")
}

/// Куда покупатель отправляет отчёт о крахе. Одна точка на ОБА окна —
/// Rust-окно (`rexon-gui::crash_report`) и Python-окно (`qt_main/app.py`
/// через мост), — чтобы они не разошлись текстом.
///
/// `None` — публичного трекера ещё нет: окно ничего и никого не называет,
/// оно лишь сохраняет отчёт и даёт его скопировать. Так и должно быть, пока
/// репозитория нет: звать человека «на GitHub», которого не существует, —
/// прямой путь к тому, что он никуда не напишет (правило #13).
///
/// `Some("https://github.com/<владелец>/<репозиторий>/issues")` — в окне
/// появляется кнопка [`crash_button_report`], а текст зовёт сообщить о
/// проблеме на GitHub. Больше менять нечего: подписи, тексты и кнопка
/// собираются отсюда сами.
pub const ISSUE_TRACKER_URL: Option<&str> = None;

// Текст окна собирается из двух частей: «что случилось» (зависит от того,
// выжил ли процесс) и «что с этим делать» (зависит от того, задан ли
// трекер). Части — отдельные строки каталога, склеенные пустой строкой;
// имена кнопок в них подставляются из подписей самих кнопок, чтобы текст и
// кнопка не разошлись ни на одном языке.

/// «Что случилось»: `fatal` — процесс не выживет.
pub fn crash_lead(fatal: bool) -> String {
    if fatal {
        tr("Rexon quit unexpectedly.")
    } else {
        tr("An error occurred in Rexon. The program is still running, but the last \
            action may not have completed. If something stops responding, close Rexon \
            and open it again.")
    }
}

/// Хвост «что делать», когда адрес трекера не задан (с пустой строкой
/// впереди — он дописывается к [`crash_lead`]).
pub fn crash_tail_plain() -> String {
    format!(
        "\n\n{}",
        trf(
            // TRANSLATORS: %1 is the label of the “Copy Report” button.
            "You can copy the error report with the “%1” button and attach it to your \
             message if you contact the developer.",
            &[&crash_button_copy()],
        )
    )
}

/// Хвост «что делать», когда адрес трекера задан.
pub fn crash_tail_tracker() -> String {
    format!(
        "\n\n{}",
        trf(
            // TRANSLATORS: %1 is the label of the “Report a Problem” button, %2 of the “Copy Report” button.
            "Help fix this error: click “%1”, describe on GitHub what happened and paste \
             the report into your message — the “%2” button copies it.",
            &[&crash_button_report(), &crash_button_copy()],
        )
    )
}

/// Подпись кнопки «скопировать».
pub fn crash_button_copy() -> String {
    tr("Copy Report")
}
/// Подпись кнопки «открыть папку».
pub fn crash_button_open() -> String {
    tr("Open Folder")
}
/// Подпись кнопки «закрыть».
pub fn crash_button_close() -> String {
    tr("Close")
}
/// Подпись кнопки «сообщить о проблеме». Кнопки нет, пока не задан
/// [`ISSUE_TRACKER_URL`], — вести человека некуда.
pub fn crash_button_report() -> String {
    tr("Report a Problem")
}

/// Адрес трекера, пригодный к показу: пустая строка — это «не задан».
pub fn issue_tracker_url(url: Option<&str>) -> Option<&str> {
    url.map(str::trim).filter(|u| !u.is_empty())
}

/// Кнопка «Сообщить о проблеме»: подпись и команда открытия адреса.
/// `None` — адрес не задан, кнопки в окне нет.
///
/// Rust-окно открывает адрес само (`QDesktopServices` через
/// `Qt.openUrlExternally`), Python-окну нужна команда — отсюда argv.
pub fn crash_report_issue_button_for(url: Option<&str>) -> Option<(String, Vec<String>)> {
    issue_tracker_url(url)
        .map(|u| (crash_button_report(), vec!["xdg-open".to_string(), u.to_string()]))
}

/// То же по адресу продукта ([`ISSUE_TRACKER_URL`]).
pub fn crash_report_issue_button() -> Option<(String, Vec<String>)> {
    crash_report_issue_button_for(ISSUE_TRACKER_URL)
}

/// Роль кнопки в `QMessageBox`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonRole {
    Action,
    Reject,
}

impl ButtonRole {
    /// Имя элемента `QMessageBox.ButtonRole` для Qt-слоя.
    pub fn as_str(self) -> &'static str {
        match self {
            ButtonRole::Action => "ActionRole",
            ButtonRole::Reject => "RejectRole",
        }
    }
}

/// Какую кнопку нажали. Порядок совпадает с порядком добавления в окно.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashButton {
    Copy,
    OpenFolder,
    Close,
}

/// Признаки кнопок окна в порядке добавления. Подписи — у [`crash_buttons`]:
/// порядок и роли от языка не зависят, а подписи — зависят.
pub const CRASH_BUTTON_ORDER: &[(CrashButton, ButtonRole)] = &[
    (CrashButton::Copy, ButtonRole::Action),
    (CrashButton::OpenFolder, ButtonRole::Action),
    (CrashButton::Close, ButtonRole::Reject),
];

impl CrashButton {
    /// Подпись кнопки на языке интерфейса.
    pub fn label(self) -> String {
        match self {
            CrashButton::Copy => crash_button_copy(),
            CrashButton::OpenFolder => crash_button_open(),
            CrashButton::Close => crash_button_close(),
        }
    }
}

/// Кнопки окна в порядке добавления: признак, подпись, роль.
pub fn crash_buttons() -> Vec<(CrashButton, String, ButtonRole)> {
    CRASH_BUTTON_ORDER.iter().map(|&(b, role)| (b, b.label(), role)).collect()
}

/// Содержимое окна о крахе.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashDialog {
    pub title: String,
    pub text: String,
    /// Процесс не выживет: от этого зависит «что случилось» в тексте, и по
    /// этому признаку (а не по сравнению текста) системное окно собирает
    /// свой вариант.
    pub fatal: bool,
    /// `None` — лог сохранить не удалось, строка не показывается.
    pub informative: Option<String>,
    /// Полный текст отчёта (шапка + traceback) — он же уходит в буфер обмена.
    pub detailed: String,
}

/// Собрать окно о крахе. `header` — та же шапка, что легла в файл.
pub fn crash_dialog(header: &str, crash_text: &str, log_path: Option<&str>) -> CrashDialog {
    CrashDialog {
        title: crash_dialog_title(),
        text: crash_dialog_text(true),
        fatal: true,
        informative: log_path.map(msg_report_saved),
        detailed: format!("{header}{crash_text}"),
    }
}

/// «Отчёт сохранён» с путём на следующей строке.
fn msg_report_saved(path: &str) -> String {
    // TRANSLATORS: %1 is the path of the crash report file (shown on its own line).
    trf("Report saved:\n%1", &[&path])
}

/// Что делать по нажатой кнопке.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrashAction {
    /// Положить текст в буфер обмена.
    Copy(String),
    /// Запустить команду (открытие папки с логом).
    Run(Vec<String>),
    /// Ничего.
    None,
}

/// Реакция на кнопку. «Открыть папку» без сохранённого лога — пустышка: в
/// оригинале ветка требует и кнопку, и путь.
pub fn crash_button_action(
    dialog: &CrashDialog,
    button: CrashButton,
    log_path: Option<&str>,
) -> CrashAction {
    match button {
        CrashButton::Copy => CrashAction::Copy(dialog.detailed.clone()),
        CrashButton::OpenFolder => match log_path {
            Some(p) => CrashAction::Run(crash_open_folder_argv(p)),
            None => CrashAction::None,
        },
        CrashButton::Close => CrashAction::None,
    }
}

/// argv открытия каталога с логом в файловом менеджере.
pub fn crash_open_folder_argv(log_path: &str) -> Vec<String> {
    vec!["xdg-open".to_string(), dirname(log_path)]
}

/// Порт `os.path.dirname`: часть до последнего `/`, у корня слэш остаётся,
/// у имени без каталога — пустая строка.
fn dirname(path: &str) -> String {
    match path.rfind('/') {
        None => String::new(),
        Some(i) => {
            let head = &path[..=i];
            let trimmed = head.trim_end_matches('/');
            if trimmed.is_empty() {
                "/".to_string()
            } else {
                trimmed.to_string()
            }
        }
    }
}

// ============================================================================
// Решение перехватчика исключений
// ============================================================================

/// Исключения, о которых покупателю сообщать НЕ надо: это штатные способы
/// завершения, а не крах.
///
/// `SystemExit` добавлен здесь — починка №2, см. шапку модуля.
pub const SILENT_EXCEPTIONS: &[&str] = &["KeyboardInterrupt", "SystemExit"];

/// Что делать с пойманным исключением.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashDecision {
    /// Отдать traceback штатному `sys.__excepthook__` (для терминала).
    pub print_stderr: bool,
    /// Записать отчёт в каталог отчётов.
    pub save_log: bool,
    /// Показать окно о крахе.
    pub show_dialog: bool,
    /// Показ обязан быть переброшен в поток GUI: исключение поймано в рабочем
    /// потоке, а строить `QMessageBox` по месту нельзя (починка №3).
    pub dialog_needs_main_thread_hop: bool,
}

/// Решение по имени класса исключения и потоку, где оно поймано.
///
/// В главном потоке «молчун» всё равно печатается в stderr (так делает
/// оригинал: `Ctrl+C` из терминала должен оставить след). В рабочем потоке
/// «молчун» не делает вообще ничего.
pub fn crash_decision(exc_type_name: &str, main_thread: bool) -> CrashDecision {
    let silent = SILENT_EXCEPTIONS.contains(&exc_type_name);
    if silent {
        return CrashDecision {
            print_stderr: main_thread,
            save_log: false,
            show_dialog: false,
            dialog_needs_main_thread_hop: false,
        };
    }
    CrashDecision {
        print_stderr: true,
        save_log: true,
        show_dialog: true,
        dialog_needs_main_thread_hop: !main_thread,
    }
}

// ============================================================================
// Паника Rust-окна
// ============================================================================
//
// У Rust-входа (`rexon-main-window`) вместо исключений — паники, и
// перехватчик паники зовётся на КАЖДУЮ из них, в том числе на пойманную
// выше по стеку (`catch_unwind`, задача tokio). Отсюда два отличия от
// решения для Python:
//   * отчёт и окно — один раз на процесс: вторая паника (чаще всего —
//     следствие первой) уходит только в stderr, иначе человек получил бы
//     пачку окон подряд;
//   * паника в главном потоке смертельна всегда (выход из `main` или
//     `abort` на границе с Qt), а в рабочем потоке процесс, как правило,
//     живёт дальше — и окно обязано сказать правду: «продолжает работать»,
//     а не «неожиданно завершился». Оригинал говорил «завершился» и там, где
//     умер только рабочий поток (починка по правилу #16).

/// Подписи раскрытия технического текста. Подробности свёрнуты по
/// умолчанию: покупателю трассировка ни о чём не говорит (правило #13),
/// она нужна тому, кому он отправит отчёт.
pub fn crash_details_show() -> String {
    tr("Show Details")
}
pub fn crash_details_hide() -> String {
    tr("Hide Details")
}

/// Строка после нажатия «Скопировать отчёт». Окно при этом НЕ закрывается:
/// на Wayland и X11 буфер обмена живёт, пока жив владелец, и закрытие окна
/// сразу после копирования уносило бы скопированное с собой (если в сеансе
/// нет менеджера буфера).
pub fn crash_copied() -> String {
    tr("Report copied — paste it into your message to the developer.")
}

/// Основной текст окна по тому, умер ли процесс и задан ли адрес трекера.
pub fn crash_dialog_text_for(fatal: bool, url: Option<&str>) -> String {
    let tail = if issue_tracker_url(url).is_some() {
        crash_tail_tracker()
    } else {
        crash_tail_plain()
    };
    format!("{}{tail}", crash_lead(fatal))
}

/// То же по адресу продукта ([`ISSUE_TRACKER_URL`]).
pub fn crash_dialog_text(fatal: bool) -> String {
    crash_dialog_text_for(fatal, ISSUE_TRACKER_URL)
}

/// Что делать с паникой.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanicDecision {
    /// Записать отчёт в каталог отчётов.
    pub save_log: bool,
    /// Показать окно о крахе (отдельным процессом).
    pub show_dialog: bool,
    /// Процесс после этой паники не выживет — от этого зависит текст окна.
    pub fatal: bool,
}

/// Решение по потоку паники и по тому, был ли уже отчёт в этом процессе.
/// В stderr паника печатается ВСЕГДА — это не решается.
pub fn panic_decision(main_thread: bool, already_reported: bool) -> PanicDecision {
    PanicDecision {
        save_log: !already_reported,
        show_dialog: !already_reported,
        fatal: main_thread,
    }
}

/// Тело отчёта о панике — то, что у Python-хозяина занимает трассировка.
pub fn panic_crash_text(message: &str, thread: &str, location: Option<&str>, backtrace: &str) -> String {
    let location = location.unwrap_or("?");
    let backtrace = backtrace.trim_end();
    format!(
        "Rust panic: {message}\n\
         Thread:   {thread}\n\
         Location: {location}\n\n\
         Backtrace:\n{backtrace}\n"
    )
}

/// Окно о крахе по сохранённому файлу отчёта (его читает процесс окна).
///
/// «Отчёт сохранён» показывается, только если файл лежит прямо в каталоге
/// отчётов: если сохранить туда не удалось, отчёт едет к окну через
/// временный файл ([`crash_transport_path`]), и называть его человеку как
/// сохранённый было бы неправдой — он пропадёт при выходе из сеанса.
pub fn crash_dialog_for_report(report: &str, report_path: &str, crash_dir: &str, fatal: bool) -> CrashDialog {
    let saved = dirname(report_path) == crash_dir.trim_end_matches('/');
    CrashDialog {
        title: crash_dialog_title(),
        text: crash_dialog_text(fatal),
        fatal,
        informative: saved.then(|| msg_report_saved(report_path)),
        detailed: report.to_string(),
    }
}

/// Куда положить отчёт для окна, если в каталог отчётов записать не
/// удалось. `runtime_dir` — `$XDG_RUNTIME_DIR` (или временный каталог).
pub fn crash_transport_path(runtime_dir: &str, pid: u32) -> String {
    format!("{}/rexon-crash-{pid}.log", runtime_dir.trim_end_matches('/'))
}

/// Есть ли куда показать окно о крахе. Без дисплея (выход из сеанса,
/// запуск из консоли) любое окно — наше и kdialog — падает с `abort`: это
/// дамп памяти и чужое окно о крахе вместо нашего, а показать всё равно
/// нечего. Тогда отчёт остаётся файлом и строкой в stderr.
pub fn crash_dialog_can_show(wayland_display: Option<&str>, display: Option<&str>) -> bool {
    [wayland_display, display]
        .iter()
        .any(|v| v.is_some_and(|s| !s.is_empty()))
}

/// Поднимется ли Qt в этом процессе вообще. Дисплей есть
/// ([`crash_dialog_can_show`]) — да. Нет — да только тогда, когда
/// `QT_QPA_PLATFORM` явно называет платформу без экрана (`offscreen`,
/// `minimal`, `eglfs`, `linuxfb`, `vnc`…): у `wayland*` и `xcb` без
/// дисплея `QGuiApplication` кончается `abort`. Список платформ через `;`
/// (запасные) и параметры после `:` Qt понимает так же.
///
/// Один ответ на тот же класс, что окно о крахе: «Qt без дисплея падает
/// с abort» — спрашивать его обязан любой вход, который создаёт
/// `QGuiApplication` и может быть запущен из консоли.
pub fn qt_can_start(wayland_display: Option<&str>, display: Option<&str>, qpa_platform: Option<&str>) -> bool {
    if crash_dialog_can_show(wayland_display, display) {
        return true;
    }
    qpa_platform.unwrap_or_default().split(';').any(|p| {
        let name = p.split(':').next().unwrap_or_default().trim();
        !name.is_empty() && name != "xcb" && !name.starts_with("wayland")
    })
}

/// Хвост «что делать» для системного окна без адреса трекера: наших кнопок
/// там нет, поэтому вместо «скопируйте кнопкой» — «приложите файл».
pub fn crash_fallback_file_hint() -> String {
    format!(
        "\n\n{}",
        tr("If you contact the developer, attach the report file to your message — its \
            path is below.")
    )
}

/// Хвост «что делать» для системного окна: кнопок нет, поэтому адрес
/// трекера называется прямо в тексте, а отчёт предлагается приложить файлом
/// (если он сохранён — иначе прикладывать нечего, и про файл молчим).
fn crash_fallback_tail(url: Option<&str>, saved: bool) -> String {
    match (issue_tracker_url(url), saved) {
        (None, true) => crash_fallback_file_hint(),
        (None, false) => String::new(),
        (Some(u), true) => format!(
            "\n\n{}",
            trf(
                // TRANSLATORS: %1 is the web address of the issue tracker.
                "Help fix this error: report it on GitHub (%1) and attach the report file \
                 to your message — its path is below.",
                &[&u],
            )
        ),
        (Some(u), false) => format!(
            "\n\n{}",
            // TRANSLATORS: %1 is the web address of the issue tracker.
            trf("Help fix this error: report it on GitHub — %1", &[&u])
        ),
    }
}

/// Системные окна на случай, когда окно Rexon не поднялось (Qt не
/// стартует, бинарь не запускается). Порядок — порядок попыток; каждое
/// окно сообщает то же самое, что окно Rexon, кроме кнопок.
///
/// kdialog первым: он умеет «подробности» под раскрытием; у zenity только
/// основной текст (трассировка в нём нечитаема), зато путь к файлу отчёта
/// есть в обоих.
pub fn crash_fallback_argvs(dialog: &CrashDialog) -> Vec<Vec<String>> {
    crash_fallback_argvs_for(dialog, ISSUE_TRACKER_URL)
}

/// То же с явным адресом трекера (продуктовый — [`ISSUE_TRACKER_URL`]).
pub fn crash_fallback_argvs_for(dialog: &CrashDialog, url: Option<&str>) -> Vec<Vec<String>> {
    // Наших кнопок у системных окон нет, поэтому хвост «что делать»
    // подменяется целиком: советы про кнопки заменяются файлом отчёта и
    // адресом трекера. «Что случилось» берётся по признаку, а не отрезанием
    // хвоста от текста: сравнение текста зависело бы от языка.
    let lead = crash_lead(dialog.fatal);
    let mut body = format!(
        "{lead}{}",
        crash_fallback_tail(url, dialog.informative.is_some())
    );
    if let Some(info) = &dialog.informative {
        body.push_str("\n\n");
        body.push_str(info);
    }
    let s = |v: &str| v.to_string();
    vec![
        vec![
            s("kdialog"),
            s("--title"),
            dialog.title.clone(),
            s("--detailederror"),
            body.clone(),
            dialog.detailed.clone(),
        ],
        vec![
            s("zenity"),
            s("--error"),
            s("--no-markup"),
            s("--width=520"),
            s("--title"),
            dialog.title.clone(),
            s("--text"),
            body,
        ],
    ]
}

// ============================================================================
// Диагностические строки запуска
// ============================================================================

/// Строка «какая платформа Qt выбрана» — печатается сразу после создания
/// `QApplication`. `!r` оригинала: строки в кавычках, отсутствующая
/// переменная окружения — `None`.
pub fn qt_platform_banner(platform_name: &str, qt_qpa_platform: Option<&str>) -> String {
    let qpa = match qt_qpa_platform {
        Some(v) => py_repr_str(v),
        None => "None".to_string(),
    };
    format!(
        "[QT] platform={}, QT_QPA_PLATFORM={qpa}",
        py_repr_str(platform_name)
    )
}

/// Строка «с какими флагами и геометрией встало окно» — печатается после
/// `show()`. Геометрия печатается кортежем Python.
pub fn qt_window_banner(flags: u64, frame_geometry: (i32, i32, i32, i32)) -> String {
    let (x, y, w, h) = frame_geometry;
    format!("[QT] flags=0x{flags:x}, frameGeom=({x}, {y}, {w}, {h})")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Шапка настоящего отчёта Python-хозяина (`crash_20260907_220608.log`
    /// с этой машины) — эталон раскладки для обоих входов.
    const PY_SAMPLE_HEADER: &str = "Rexon crash report\n\
        Time:     2026-09-07T22:06:08.454403\n\
        Python:   3.14.7\n\
        PySide6:  6.11.2\n\
        OS:       Linux-7.2.3-1-cachyos-x86_64-with-glibc2.44\n\
        DE:       KDE (wayland)\n\
        Locale:   ru_RU.UTF-8\n\
        ────────────────────────────────────────────────────────────\n\n";

    fn sample_now() -> Timestamp {
        Timestamp { year: 2026, month: 9, day: 7, hour: 22, minute: 6, second: 8, microsecond: 454403 }
    }

    fn sample_env(os: &str) -> CrashHeaderEnv<'_> {
        CrashHeaderEnv {
            os_platform: os,
            xdg_current_desktop: Some("KDE"),
            desktop_session: Some("plasma"),
            xdg_session_type: Some("wayland"),
            lang: Some("ru_RU.UTF-8"),
        }
    }

    /// Каталог для записи — в $HOME, а не в /tmp (там ОЗУ, и там живут
    /// чужие файлы). Уникален на прогон.
    fn home_tmp(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap())
            .join(".cache")
            .join(format!("rexon-test-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn python_header_still_matches_real_sample() {
        let os = os_platform("Linux", "7.2.3-1-cachyos", "x86_64", Some(("glibc", "2.44")));
        let e = sample_env(&os);
        let got = crash_header(&CrashHeaderInput {
            now: sample_now(),
            python_version: "3.14.7",
            pyside_version: Some("6.11.2"),
            os_platform: e.os_platform,
            xdg_current_desktop: e.xdg_current_desktop,
            desktop_session: e.desktop_session,
            xdg_session_type: e.xdg_session_type,
            lang: e.lang,
        });
        assert_eq!(got, PY_SAMPLE_HEADER);
    }

    #[test]
    fn rust_header_has_python_layout() {
        let os = os_platform("Linux", "7.2.3-1-cachyos", "x86_64", Some(("glibc", "2.44")));
        let got = rust_crash_header(&RustCrashHeaderInput {
            now: sample_now(),
            rexon_version: "0.0.1",
            entry: "rexon-main-window",
            qt_version: Some("6.11.2"),
            env: sample_env(&os),
        });
        let expected = PY_SAMPLE_HEADER
            .replace("Python:   3.14.7\n", "Rexon:    0.0.1 (rexon-main-window)\n")
            .replace("PySide6:  6.11.2\n", "Qt:       6.11.2\n");
        assert_eq!(got, expected);
    }

    #[test]
    fn rust_header_unknowns() {
        let got = rust_crash_header(&RustCrashHeaderInput::default());
        assert!(got.contains("\nQt:       ?\n"), "{got}");
        assert!(got.contains("\nDE:       ? (?)\n"), "{got}");
        assert!(got.contains("\nRexon:    \n"), "{got}");
    }

    #[test]
    fn os_platform_like_python() {
        assert_eq!(
            os_platform("Linux", "7.2.3-1-cachyos", "x86_64", Some(("glibc", "2.44"))),
            "Linux-7.2.3-1-cachyos-x86_64-with-glibc2.44"
        );
        assert_eq!(os_platform("Linux", "6.1", "aarch64", None), "Linux-6.1-aarch64");
        assert_eq!(os_platform("Linux", "", "x86_64", None), "Linux-x86_64");
    }

    #[test]
    fn panic_reported_once_and_fatal_only_on_main() {
        assert_eq!(
            panic_decision(true, false),
            PanicDecision { save_log: true, show_dialog: true, fatal: true }
        );
        let worker = panic_decision(false, false);
        assert!(worker.save_log && worker.show_dialog && !worker.fatal);
        let again = panic_decision(true, true);
        assert!(!again.save_log && !again.show_dialog);
    }

    #[test]
    fn panic_text_layout() {
        let t = panic_crash_text("boom", "main", Some("src/x.rs:1:2"), "   0: f\n   1: g\n\n");
        assert_eq!(
            t,
            "Rust panic: boom\nThread:   main\nLocation: src/x.rs:1:2\n\nBacktrace:\n   0: f\n   1: g\n"
        );
        assert!(panic_crash_text("x", "w", None, "").contains("Location: ?\n"));
    }

    #[test]
    fn dialog_for_report_names_only_saved_file() {
        // Язык исходный: русский текст сторожит `tests/qt_app_parity.rs`.
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let dir = "/home/u/.config/rexon/crashes";
        let saved = crash_dialog_for_report("R", &format!("{dir}/crash_1.log"), dir, true);
        assert_eq!(saved.title, crash_dialog_title());
        assert_eq!(saved.text, crash_dialog_text(true));
        assert!(saved.fatal);
        assert_eq!(saved.informative.as_deref(), Some("Report saved:\n/home/u/.config/rexon/crashes/crash_1.log"));
        assert_eq!(saved.detailed, "R");
        // Тот же вид, что у окна Python-хозяина для той же пары шапка+текст.
        let py = crash_dialog("", "R", Some(&format!("{dir}/crash_1.log")));
        assert_eq!(saved, py);

        let moved = crash_dialog_for_report("R", &crash_transport_path("/run/user/1000/", 42), dir, false);
        assert_eq!(moved.informative, None);
        assert_eq!(moved.text, crash_dialog_text(false));
        assert!(!moved.fatal);
        assert_eq!(crash_transport_path("/run/user/1000/", 42), "/run/user/1000/rexon-crash-42.log");
    }

    #[test]
    fn dialog_needs_some_display() {
        assert!(crash_dialog_can_show(Some("wayland-0"), None));
        assert!(crash_dialog_can_show(None, Some(":0")));
        assert!(!crash_dialog_can_show(None, None));
        assert!(!crash_dialog_can_show(Some(""), Some("")));
    }

    #[test]
    fn qt_without_display_only_on_screenless_platform() {
        assert!(qt_can_start(Some("wayland-0"), None, None));
        assert!(qt_can_start(None, Some(":0"), Some("xcb")));
        assert!(!qt_can_start(None, None, None));
        assert!(!qt_can_start(Some(""), Some(""), Some("")));
        assert!(!qt_can_start(None, None, Some("wayland")));
        assert!(!qt_can_start(None, None, Some("wayland-egl;xcb")));
        assert!(qt_can_start(None, None, Some("offscreen")));
        assert!(qt_can_start(None, None, Some("wayland;offscreen")));
        assert!(qt_can_start(None, None, Some("vnc:size=800x600")));
    }

    #[test]
    fn fallback_dialogs_carry_path_and_title() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let d = crash_dialog_for_report("DETAILS", "/c/crash_1.log", "/c", true);
        let argvs = crash_fallback_argvs(&d);
        assert_eq!(argvs[0][0], "kdialog");
        assert_eq!(argvs[0][3], "--detailederror");
        assert!(argvs[0][4].ends_with("Report saved:\n/c/crash_1.log"));
        assert_eq!(argvs[0][5], "DETAILS");
        assert_eq!(argvs[1][0], "zenity");
        assert!(argvs[1].iter().any(|a| a == "--no-markup"));
        assert!(argvs[1].last().unwrap().ends_with("/c/crash_1.log"));
        // Совета про несуществующую кнопку в системном окне нет.
        for fatal in [true, false] {
            let tail = crash_tail_plain();
            assert!(
                crash_dialog_text(fatal).ends_with(&tail),
                "хвост текста не найден — подмена в системном окне молча отключилась"
            );
            let hint = crash_fallback_file_hint();
            let saved = crash_fallback_argvs(&crash_dialog_for_report("D", "/c/crash_1.log", "/c", fatal));
            assert!(!saved[0][4].contains(&tail) && saved[0][4].contains(&hint));
            assert!(saved[0][4].starts_with(&crash_lead(fatal)), "{}", saved[0][4]);
            let unsaved = crash_fallback_argvs(&crash_dialog_for_report("D", "/run/rexon-crash-1.log", "/c", fatal));
            assert!(!unsaved[1][7].contains(&tail) && !unsaved[1][7].contains(&hint));
        }
        for a in &argvs {
            assert!(a.iter().any(|x| *x == crash_dialog_title()));
        }
    }

    /// Адрес трекера решает всё: пока его нет — окно никого не называет и
    /// кнопки «Сообщить о проблеме» не показывает; как только задан —
    /// появляется и кнопка, и упоминание GitHub. Оба окна, Rust и Python,
    /// берут это отсюда, поэтому сверяется сама развилка.
    #[test]
    fn issue_tracker_switches_text_and_button() {
        const URL: &str = "https://github.com/wehrwolfmann/rexon/issues";
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let (copy, report) = (crash_button_copy(), crash_button_report());

        // Пусто — состояние продукта на сегодня.
        assert_eq!(crash_report_issue_button_for(None), None);
        assert_eq!(crash_report_issue_button_for(Some("   ")), None);
        for fatal in [true, false] {
            let text = crash_dialog_text_for(fatal, None);
            assert!(
                !text.contains("GitHub") && !text.contains("Discord"),
                "адресата нет — называть его нечем: {text}"
            );
            assert!(text.contains(&copy));
            assert!(!text.contains(&report));
        }

        // Задан — кнопка и текст с GitHub.
        let (label, argv) = crash_report_issue_button_for(Some(URL)).unwrap();
        assert_eq!(label, report);
        assert_eq!(argv, vec!["xdg-open".to_string(), URL.to_string()]);
        for fatal in [true, false] {
            let text = crash_dialog_text_for(fatal, Some(URL));
            assert!(text.contains("GitHub"), "текст не зовёт на трекер: {text}");
            assert!(text.contains(&report) && text.contains(&copy));
        }

        // Подписи кнопок названы в тексте — сверяем, что они не разъехались
        // с самими кнопками.
        assert!(crash_tail_plain().contains(&copy));
        assert!(crash_tail_tracker().contains(&report));
        assert!(crash_tail_tracker().contains(&copy));

        // Системное окно кнопок не имеет: адрес уходит прямо в текст.
        let d = crash_dialog_for_report("D", "/c/crash_1.log", "/c", true);
        let with = crash_fallback_argvs_for(&d, Some(URL));
        assert!(with[0][4].contains(URL) && !with[0][4].contains(&copy));
        let without = crash_fallback_argvs_for(&d, None);
        assert!(!without[0][4].contains("GitHub") && without[0][4].contains(&crash_fallback_file_hint()));
        let moved = crash_dialog_for_report("D", "/run/rexon-crash-1.log", "/c", false);
        assert!(crash_fallback_argvs_for(&moved, Some(URL))[1][7].contains(URL));

        // Продуктовые обёртки берут ровно эту константу.
        assert_eq!(crash_dialog_text(true), crash_dialog_text_for(true, ISSUE_TRACKER_URL));
        assert_eq!(crash_report_issue_button(), crash_report_issue_button_for(ISSUE_TRACKER_URL));
    }

    fn part_files(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".part-"))
            .collect()
    }

    #[test]
    fn atomic_write_replaces_whole_file_and_leaves_no_part() {
        let dir = home_tmp("atomic");
        let path = dir.join("crash_x.log");
        std::fs::write(&path, "старый длинный-длинный текст, который обязан исчезнуть целиком").unwrap();
        atomic_write(path.to_str().unwrap(), "новый").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "новый");
        assert!(part_files(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_failure_keeps_target_and_cleans_part() {
        let dir = home_tmp("atomic-fail");
        // На месте файла — непустой каталог: rename обязан отказать.
        let target = dir.join("crash_y.log");
        std::fs::create_dir_all(target.join("inner")).unwrap();
        assert!(atomic_write(target.to_str().unwrap(), "x").is_err());
        assert!(target.join("inner").is_dir());
        assert!(part_files(&dir).is_empty());
        // Каталога нет вовсе — ошибка, и ничего не создано.
        let missing = dir.join("nope/crash_z.log");
        assert!(atomic_write(missing.to_str().unwrap(), "x").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn live_save_is_atomic_and_rotates() {
        let root = home_tmp("save");
        let dir = root.join("rexon/crashes");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..22 {
            std::fs::write(dir.join(format!("crash_2000010{}_0000{:02}.log", i / 10, i)), "old").unwrap();
        }
        std::fs::write(dir.join("README"), "чужой").unwrap();
        let ts = Timestamp { year: 2026, month: 9, day: 11, hour: 12, minute: 0, second: 1, microsecond: 0 };
        let path = save_crash_log(&mut AtomicCrashIo, dir.to_str().unwrap(), &ts, "H\n", "T\n").unwrap();
        assert!(path.ends_with("/crash_20260911_120001.log"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "H\nT\n");
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.iter().filter(|n| n.starts_with(CRASH_PREFIX)).count(), CRASH_KEEP);
        assert!(names.contains(&"README".to_string()));
        assert!(part_files(&dir).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
