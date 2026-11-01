//! Исполнитель установки Rexon — в систему, в домашнюю папку и переносной
//! режим. Порт `RexonInstaller.install` / `install_to_system` /
//! `install_user` / `_install_portable` (`rexon/installer.py`) поверх данных
//! ядра ([`crate::installer`], [`crate::system_plan`]). Вход для окна —
//! диспетчер [`install`].
//!
//! Разделение то же, что у соседей [`crate::system_exec`] и
//! [`crate::system_de_exec`]: **ядро решает, исполнитель делает**.
//!   * ЧТО ставить и КУДА — раскладка [`InstallLayout`], корневой скрипт
//!     [`install_script`] и план [`install_plan`];
//!   * ЧЕМ — трейт [`InstallIo`] (надстройка над [`SystemIo`]): в продукте
//!     [`LiveInstallIo`], в тестах — подставной, живая машина не трогается;
//!   * ход дела — в [`DeOpSink`], тем же контрактом, что у операций над
//!     рабочими столами: окно хода `DeOperationDialog` подключается без
//!     правок (мост — пункт G5). Все строки для человека — из раздела
//!     «Тексты мастера» [`crate::installer`] (`InstallStep`,
//!     `InstallFailure`, `WizardOutcome`); своих букв здесь нет.
//!
//! ⭐ ЧТО ПОЧИНЕНО ПРИ ПЕРЕНОСЕ (правило #16), каждое доказано тестом внизу:
//!   1. Любой сбой корневого скрипта выдавался за «Установка отменена
//!      пользователем (pkexec)»: ловился весь `CalledProcessError`, и
//!      упавшая установка зависимостей читалась как отмена. Теперь скрипт
//!      печатает метки шагов, и исход различается по ФАКТУ
//!      ([`classify_script_run`]): меток нет и код 126/127 (или отказ
//!      агента) — пароль не подтверждён, скрипт не запускался; метки есть —
//!      сломался названный шаг, даже если код 127 дала команда внутри.
//!   2. Временные файлы и копия пакета оставались в `/tmp` при ошибке и
//!      отмене (уборка стояла только на пути успеха; у владельца `/tmp` —
//!      ОЗУ). Теперь всё временное лежит в ОДНОМ каталоге, и он убирается
//!      на любом исходе после его создания.
//!   3. Таймаут корневого скрипта 300 с → 600 с
//!      ([`inst::SYSTEM_INSTALL_TIMEOUT_SECS`]).
//!   4. `chmod -R 755` ставил бит запуска ВСЕМ файлам пакета, а
//!      `cp -r pkg/*` терял скрытые файлы. Теперь права раскладываются при
//!      подготовке ([`payload_mode`]: каталоги 755, файлы 644, исполняемые
//!      755), скрипт копирует `каталог/.` с сохранением прав и отдаёт
//!      владение root.
//!   5. Ручная установка ставила ТОЛЬКО Python-пакет и Python-лаунчер.
//!      Теперь состав — тот же, что у пакета дистрибутива: `rexon-main-window`,
//!      `rexon-wallpaper-host`, `rexon-splash-overlay`,
//!      `librexon_qt_filter.so`, каталог QML-модулей и данные продукта
//!      (вендорный QML вкладки обоев, `themes.json`, шрифты, Plasma-плагин
//!      и дополнения GNOME и Cinnamon). Исходников Python нет вовсе
//!      ([`payload_skip`]), команда `rexon` — `sh`-лаунчер на бинарь окна
//!      ([`inst::system_launcher`]), а поставка без этого бинаря отвергается
//!      до первого побочного действия ([`payload_has_window`]).
//!   6. Раскладка установщика расходилась с поиском: пакет клался прямо в
//!      `LIB_DIR`, а Rust-окно искало `<LIB_DIR>/rexon`; каталог QML-модулей
//!      и бинарь окна в `/usr/local` не искались вовсе. Раскладка теперь
//!      одна — та же, что у пакета дистрибутива ([`InstallLayout`]); сторож
//!      `layout_is_one_for_package_and_manual_install` сверяет её с поиском.
//!   7. ⭐⭐ Правило `rules.d` молча давало группе wheel БЕЗ пароля все
//!      `org.rexon.*`, включая установку и удаление ЛЮБЫХ пакетов. Теперь —
//!      отдельный шаг только по явному согласию
//!      ([`InstallRequest::passwordless_rule`], по умолчанию
//!      [`inst::PASSWORDLESS_CONSENT_DEFAULT`]), сужено до
//!      [`plan::PASSWORDLESS_ACTIONS`] и знает группу администраторов
//!      семейства ([`plan::polkit_admin_groups`]).
//!
//! ⭐ ЧТО ПОЧИНЕНО В ДОМАШНЕЙ ПАПКЕ И ПЕРЕНОСНОМ РЕЖИМЕ (G3b), тоже с тестами:
//!   8. Установка в домашнюю папку не создавала папку резервных копий
//!      (system и portable создавали), а переносной режим при пустом
//!      `XDG_DATA_HOME` создавал `rexon/backups` в ТЕКУЩЕМ каталоге. Папки
//!      профиля теперь одни на все режимы ([`profile_dirs`]).
//!   9. Прежняя копия в `~/.local/lib/rexon` удалялась ДО копирования новой:
//!      сбой посреди копии оставлял человека без обеих. Теперь новая копия
//!      собирается рядом, и копии меняются местами переименованием; при сбое
//!      прежняя остаётся на месте.
//!  10. На неизменяемой системе «в домашнюю папку» выбирается ДО установки в
//!      систему ([`install`]), а не отказом внутри неё: фраза итога «часть
//!      изменений могла остаться» была бы неправдой.
//!  11. Поиск не знал раскладку установки в домашнюю папку: бинарь окна
//!      (`installer::main_window_candidates`), QML-модули
//!      (`qml_modules::module_dirs`) и пакет
//!      (`installer::package_data_dir_candidates`).
//!  12. Лаунчер, ярлыки и значки писались ПОВЕРХ существующего пути: если там
//!      ссылка (у разработчика — в дерево), запись ушла бы в её цель. Теперь
//!      ссылка заменяется, а каталог установки, который сам ссылка, не
//!      заменяется вовсе ([`is_redirected`]). Путь с апострофом ломал
//!      Python-лаунчер; значок для pixmaps брался только из 128 px.
//!  13. Запущенный хост обоев прошлой установки перезаписью не открыть
//!      (`ETXTBSY`): бинари ставятся через временное имя и переименование.
//!  14. Кэши меню и значков — одни команды на оба режима
//!      ([`cache_refresh_groups`]); нет программы — не ошибка, а без
//!      `update-desktop-database` человек узнаёт, что ярлык появится позже.
//!
//! ⭐ УДАЛЕНИЕ (G8) — тот же исполнитель, вход [`uninstall`]; у окна меню
//! «Удалить Rexon» своего исполнителя больше нет. Починено, с тестами:
//!  15. Хост обоев и заставка в каталоге команд оставались после удаления
//!      обоих режимов — пути теперь из [`InstallLayout::extra_bin_paths`].
//!  16. Ссылка на месте папки Rexon (у разработчика `/usr/local/lib/rexon`
//!      ведёт в рабочее дерево) сносилась корневым скриптом, ссылка на папку
//!      настроек — тоже. Теперь путь-ссылка не удаляется и не проходится ни
//!      в каком режиме ([`is_redirected`]); перечень в окне вопроса её не
//!      называет, итог говорит, что она осталась.
//!  17. Любой сбой читался одной строкой «Ошибка удаления system: код …».
//!      Отказ пароля, нечем спросить пароль, таймаут и сбой самого удаления —
//!      разные исходы ([`classify_script_run`] по метке начала скрипта),
//!      слова — без жаргона ([`inst::uninstall_failure_line`]).
//!  18. Временный скрипт удаления лежал прямо в `/tmp` и при сбое оставался;
//!      теперь он во временной папке, которая убирается на любом исходе.
//!  19. Когда Rexon не установлен вовсе, удаление всё равно стирало
//!      `~/.config/rexon` и отвечало «Rexon не установлен». Теперь не
//!      трогается ничего.
//!  20. (решение Wehrwolfmann 15.09.2026) «Сохранить» сохраняло только
//!      резервные копии рабочих столов, а настройки сносило — после
//!      переустановки человек начинал с нуля. Теперь вариант
//!      [`UninstallRequest::keep_profile`] не трогает ни настроек (с
//!      журналом установленных тем), ни резервных копий; «удалить всё» —
//!      как прежде, обе папки.
//!  21. (решение Wehrwolfmann 15.09.2026) Ни один выбор не удалял кэш
//!      (`$XDG_CACHE_HOME/rexon`) и установленные обои, темы и дополнения
//!      (`$XDG_DATA_HOME/rexon/*`), да и литеральные `~/.cache/rexon` и
//!      `~/.local/share/rexon`, куда часть ядра пишет мимо XDG. Теперь
//!      кэш уходит при любом выборе, содержимое данных — только при «удалить
//!      всё»; перечень один — [`inst::user_data_roots`]. Ссылку на месте
//!      кэша или содержимого удаляем саму, не проходя по ней.
//!
//! ⛔ Живой [`install`] с [`LiveInstallIo`] в тестах не вызывается никогда:
//! сторож `tests_never_build_live_io`. Тесты домашней папки ходят в
//! настоящую ФС только внутри своего временного каталога — каждый путь
//! сверяется с ним, выход за него роняет тест.

use std::sync::Arc;

use crate::de_progress;
use crate::distro_compat::Lang;
use crate::i18n::{tr, trf};
use crate::installer::{
    self as inst, HelperTmp, IconSet, InstallFailure, InstallMode, InstallStep, LaunchEntry,
    WizardOutcome,
};
use crate::qml_modules as qm;
use crate::system::PkgFamily;
use crate::system_de_exec::{CancelToken, DeOpSink, DeOpsIo, LiveDeOpsIo};
use crate::system_exec::{RunOut, SystemIo};
use crate::system_plan::{self as plan, Cmd, PlanStep, RunStepId};
use crate::utils::{self, RunOutcome};

// ============================================================================
// Раскладка: одна на пакет, ручную установку и архив
// ============================================================================

/// Каталог данных продукта `rexon/` внутри каталога данных Rexon: вендорный
/// QML вкладки обоев, `themes.json`, шрифты, Plasma-плагин обоев и
/// дополнения GNOME и Cinnamon. Кода Python там больше нет ([`payload_skip`]).
pub const PACKAGE_DIR_NAME: &str = "rexon";
/// Каталог QML-модулей внутри каталога данных Rexon.
pub const QML_MODULES_DIR_NAME: &str = "qml_modules";
/// Каталог исполняемых файлов внутри распакованного архива.
pub const BUNDLE_BIN_DIR: &str = "bin";
/// Каталог исполняемых файлов пакета дистрибутива (PKGBUILD кладёт туда
/// хост обоев и заставку).
pub const PACKAGE_BIN_DIR: &str = "/usr/bin";

/// Где что лежит у установленного Rexon. Раскладка ОДНА на все каналы:
///
/// | канал | `lib_dir` | `bin_dir` |
/// |---|---|---|
/// | пакет дистрибутива | `/usr/lib/rexon` | `/usr/bin` |
/// | установка в систему | `/usr/local/lib/rexon` | `/usr/local/bin` |
/// | установка в домашнюю папку | `~/.local/lib/rexon` | `~/.local/bin` |
/// | распакованный архив | `<корень>` | `<корень>/bin` |
///
/// Внутри `lib_dir`: данные продукта `rexon/`, бинарь окна, каталог
/// QML-модулей с нашими `.so` в `waywallen/control`. В `bin_dir`: команда
/// `rexon`, хост обоев и заставка (их ядро ищет по `PATH`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallLayout {
    pub lib_dir: String,
    pub bin_dir: String,
}

impl InstallLayout {
    pub fn new(lib_dir: &str, bin_dir: &str) -> Self {
        InstallLayout {
            lib_dir: lib_dir.trim_end_matches('/').to_string(),
            bin_dir: bin_dir.trim_end_matches('/').to_string(),
        }
    }

    /// Раскладка режима установки (`None` у portable — ставить некуда).
    pub fn for_mode(mode: InstallMode, home: &str, xdg_data_home: Option<&str>) -> Option<Self> {
        let p = inst::install_paths(mode, home, xdg_data_home);
        Some(Self::new(p.lib_dir.as_deref()?, p.install_dir.as_deref()?))
    }

    /// Установка в систему.
    pub fn system() -> Self {
        Self::new(inst::LIB_DIR, inst::INSTALL_DIR)
    }

    /// Пакет дистрибутива.
    pub fn package() -> Self {
        Self::new(inst::package_data_dir(), PACKAGE_BIN_DIR)
    }

    /// Распакованный архив с корнем `root`.
    pub fn bundle(root: &str) -> Self {
        let root = root.trim_end_matches('/');
        Self::new(root, &format!("{root}/{BUNDLE_BIN_DIR}"))
    }

    pub fn package_dir(&self) -> String {
        format!("{}/{PACKAGE_DIR_NAME}", self.lib_dir)
    }

    pub fn main_window(&self) -> String {
        format!("{}/{}", self.lib_dir, crate::window_menu::MAIN_WINDOW_BIN)
    }

    /// Помощник плиток «Оформления» — рядом с окном: там его ищет ядро
    /// (`widget_shot::find_helper`).
    pub fn theme_shot(&self) -> String {
        format!("{}/{}", self.lib_dir, crate::widget_shot::HELPER_BINARY)
    }

    pub fn qml_modules(&self) -> String {
        format!("{}/{QML_MODULES_DIR_NAME}", self.lib_dir)
    }

    /// Каталог наших `.so` рядом с плагином `waywallen.control`.
    pub fn rust_lib_dir(&self) -> String {
        format!("{}/{}", self.qml_modules(), qm::RUST_LIB_SUBDIR)
    }

    pub fn filter_lib(&self) -> String {
        format!("{}/{}", self.rust_lib_dir(), qm::FILTER_LIB_NAME)
    }

    pub fn wallpaper_host(&self) -> String {
        format!("{}/{}", self.bin_dir, crate::wallpaper_engine::WALLPAPER_HOST_BINARY)
    }

    pub fn splash_overlay(&self) -> String {
        format!("{}/{}", self.bin_dir, crate::session_splash::SPLASH_OVERLAY_BINARY)
    }

    /// Команда продукта `rexon` — лаунчер на бинарь окна
    /// ([`inst::system_launcher`]).
    pub fn launcher(&self) -> String {
        format!("{}/{}", self.bin_dir, inst::PYTHON_ENTRY_CMD)
    }

    /// Файлы установки в `bin_dir` сверх лаунчера. Отсюда же их берёт
    /// удаление обоих режимов (`installer::uninstall_system_paths`,
    /// `installer::uninstall_user_paths`) — правда одна.
    pub fn extra_bin_paths(&self) -> Vec<String> {
        vec![self.wallpaper_host(), self.splash_overlay()]
    }
}

// ============================================================================
// Что ставим: источник поставки
// ============================================================================

/// Размеры растровых значков Rexon (`rexon_icon_<N>.png`).
pub const ICON_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256, 512];

/// Откуда брать файлы. `None` — этой части в поставке нет (например,
/// копия без Rust-окна: тогда ярлык ведёт на классическое окно).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadSource {
    /// Пакет `rexon/`.
    pub package_dir: String,
    /// Каталог со значками `rexon_icon_*.png`.
    pub icon_dir: String,
    pub main_window: Option<String>,
    pub wallpaper_host: Option<String>,
    pub splash_overlay: Option<String>,
    /// Каталог QML-модулей (`Qcm/Material`, `waywallen/control`).
    pub qml_modules: Option<String>,
    pub filter_lib: Option<String>,
    /// Помощник сеанса прав `rexon-priv-helper` («один пароль на запуск»).
    /// Нет в поставке — установка его не ставит и снимает прежний: помощник
    /// другой сборки окно всё равно отвергло бы.
    pub priv_helper: Option<String>,
}

impl PayloadSource {
    /// Распакованный архив: раскладка [`InstallLayout::bundle`], значки — в
    /// корне. Раскладка архива — предложение для блока H.
    pub fn from_bundle<IO: SystemIo + ?Sized>(io: &IO, root: &str) -> Self {
        let l = InstallLayout::bundle(root);
        let file = |p: String| io.is_file(&p).then_some(p);
        let qml = l.qml_modules();
        PayloadSource {
            package_dir: l.package_dir(),
            icon_dir: l.lib_dir.clone(),
            main_window: file(l.main_window()),
            wallpaper_host: file(l.wallpaper_host()),
            splash_overlay: file(l.splash_overlay()),
            qml_modules: io.is_dir(&qml).then_some(qml),
            filter_lib: file(l.filter_lib()),
            priv_helper: file(format!("{}/{}", l.bin_dir, plan::SESSION_HELPER_NAME)),
        }
    }

    /// Дерево разработчика: бинари — из каталога сборки рядом с бинарём
    /// окна, QML-модули — из пользовательского каталога, куда их кладут
    /// `tools/build_qml_*.sh`.
    pub fn from_dev_tree<IO: SystemIo + ?Sized>(
        io: &IO,
        root: &str,
        home: &str,
        xdg_data_home: Option<&str>,
    ) -> Self {
        let root = root.trim_end_matches('/');
        let release = inst::DEV_TREE_MAIN_WINDOW_REL
            .rsplit_once('/')
            .map(|(dir, _)| dir)
            .unwrap_or("");
        let built = |name: &str| {
            let p = format!("{root}/{release}/{name}");
            io.is_file(&p).then_some(p)
        };
        let qml_modules = [utils::data_home(xdg_data_home, home), utils::data_home(None, home)]
            .into_iter()
            .map(|base| format!("{base}/{}", qm::MODULES_REL))
            .find(|dir| io.is_file(&format!("{dir}/{}", qm::QCM_MATERIAL_REL)));
        PayloadSource {
            package_dir: format!("{root}/{}", inst::DEV_TREE_DATA_DIR),
            icon_dir: root.to_string(),
            main_window: built(crate::window_menu::MAIN_WINDOW_BIN),
            wallpaper_host: built(crate::wallpaper_engine::WALLPAPER_HOST_BINARY),
            splash_overlay: built(crate::session_splash::SPLASH_OVERLAY_BINARY),
            qml_modules,
            filter_lib: built(qm::FILTER_LIB_NAME),
            priv_helper: built(plan::SESSION_HELPER_NAME),
        }
    }

    /// Поставка того Rexon, что сейчас запущен: дерево разработчика, если
    /// корень дерева известен (`cli::repo_root`), иначе — архив вокруг
    /// своего бинаря.
    pub fn for_running<IO: SystemIo + ?Sized>(
        io: &IO,
        own_exe: &str,
        repo_root: &str,
        home: &str,
        xdg_data_home: Option<&str>,
    ) -> Self {
        if !repo_root.is_empty() {
            return Self::from_dev_tree(io, repo_root, home, xdg_data_home);
        }
        let dir = own_exe.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
        Self::from_bundle(io, dir)
    }
}

/// Бывшая библиотека крейта `rexon-gui` (cdylib, 54 МБ). Её грузил через
/// ctypes хозяин вкладки на Python; он в архиве, крейт теперь только rlib и
/// вшит в окно статически. Пакет и установщик её больше не кладут, но в
/// каталоге QML-модулей разработчика (`~/.local/share/rexon/qml_modules`,
/// откуда берёт поставка [`PayloadSource::from_dev_tree`]) прежняя копия
/// лежит, пока `tools/build_qml_control.sh` не пересоберёт модуль. Без
/// пропуска установка унесла бы мёртвый груз в систему.
const RETIRED_GUI_LIB: &str = "librexon_gui.so";

/// Не копировать в установку: исходники Python, кеши байткода и черновики.
///
/// ⭐ Починка 16.09.2026 (правило #16): ручная установка клала в систему
/// ВЕСЬ пакет Python — код классического окна, которого поставка больше не
/// везёт. Теперь, как и пакет дистрибутива (`packaging/aur/PKGBUILD`:
/// `find "$data" -name '*.py' -delete`), едут только данные продукта:
/// вендорный QML вкладки обоев, `themes.json`, шрифты, Plasma-плагин обоев
/// и дополнения GNOME и Cinnamon. Признак каталога данных
/// (`installer::PACKAGE_MARKER_REL`) — файл QML, а не `.py`, поэтому поиск
/// раскладку по-прежнему узнаёт.
///
/// [`RETIRED_GUI_LIB`] — тоже мимо: см. там.
///
/// ⚠️ Модель перевода описаний (`translate/<пара>/model.….bin` и словарь,
/// 43 МБ) под пропуск НЕ попадает намеренно: каталог данных копируется
/// целиком, и модель обязана доехать вместе с ним — иначе ручная установка
/// молча осталась бы без перевода описаний, хотя пакет дистрибутива модель
/// везёт (`packaging/aur/PKGBUILD`). В дереве разработчика модели нет, брать
/// её оттуда нечего; из распакованного архива она едет сама.
pub fn payload_skip(name: &str) -> bool {
    name == "__pycache__"
        || name == RETIRED_GUI_LIB
        || name.ends_with(".py")
        || name.ends_with(".pyc")
        || name.ends_with(".bak")
        || name.ends_with(".draft")
}

/// Права в установке: каталоги и исполняемые файлы — 755, остальное — 644.
/// `executable` — есть ли бит запуска у источника.
///
/// Починка 4: прежний `chmod -R 755` делал исполняемым каждый `.py`, `.qml`
/// и картинку.
pub fn payload_mode(is_dir: bool, executable: bool) -> u32 {
    if is_dir || executable {
        0o755
    } else {
        0o644
    }
}

// ============================================================================
// Трейт исполнителя
// ============================================================================

/// Побочные действия установки СВЕРХ [`SystemIo`]. Ни одного решения.
pub trait InstallIo: SystemIo {
    /// Создать уникальный временный каталог (0700) в `parent` (`None` —
    /// системный временный каталог) → путь.
    fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String>;
    /// Переименовать файл или каталог (одна файловая система). Ссылку
    /// переименовывает саму, в её цель не заходит.
    fn rename(&self, src: &str, dst: &str) -> Result<(), String>;
    /// Создать каталог со всеми родителями. `mode` — права для каждого
    /// СОЗДАННОГО каталога (`None` — не трогать, как `os.makedirs`).
    fn make_dirs(&self, path: &str, mode: Option<u32>) -> Result<(), String>;
    /// Записать файл целиком и выставить права.
    fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String>;
    /// Скопировать файл (содержимое) и выставить права.
    fn copy_file_mode(&self, src: &str, dst: &str, mode: u32) -> Result<(), String>;
    /// Скопировать дерево. Имена, для которых `skip` истинно, пропускаются
    /// вместе с содержимым; права каждого каталога и файла — `mode(is_dir,
    /// executable)`. Симлинк на файл копируется содержимым, симлинк на
    /// каталог и висячий симлинк пропускаются. → число скопированных файлов.
    fn copy_tree(
        &self,
        src: &str,
        dst: &str,
        skip: &dyn Fn(&str) -> bool,
        mode: &dyn Fn(bool, bool) -> u32,
    ) -> Result<usize, String>;
    /// Удалить файл; ошибки не важны.
    fn remove_file(&self, path: &str);
    /// Удалить каталог целиком; ошибки не важны.
    fn remove_tree(&self, path: &str);
}

// ============================================================================
// Запрос и исход
// ============================================================================

/// Что просят установить.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    pub payload: PayloadSource,
    pub family: PkgFamily,
    /// `ID` из os-release: на неизменяемой системе установка отказывает до
    /// первого побочного действия (выбор «тогда в домашнюю папку» — G3b).
    pub distro_id: String,
    /// Значение `REXON_ENTRY` (откат на классическое окно).
    pub entry_env: Option<String>,
    /// Человек ЯВНО согласился на правило «без повторного пароля».
    pub passwordless_rule: bool,
    /// Язык строк хода и итога.
    pub lang: Lang,
}

impl InstallRequest {
    pub fn new(payload: PayloadSource, family: PkgFamily, distro_id: &str, lang: Lang) -> Self {
        InstallRequest {
            payload,
            family,
            distro_id: distro_id.to_string(),
            entry_env: None,
            passwordless_rule: inst::PASSWORDLESS_CONSENT_DEFAULT,
            lang,
        }
    }
}

/// Чем кончилась установка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallResult {
    /// Итог для страницы итога (`installer::result_texts`). `None` —
    /// отменено человеком до запроса пароля, система не тронута.
    pub outcome: Option<WizardOutcome>,
    /// Какой вход записан в ярлык (есть, если до ярлыка дошло).
    pub entry: Option<LaunchEntry>,
    /// Одна строка итога — для журнала и `(ok, сообщение)`.
    pub message: String,
}

impl InstallResult {
    pub fn ok(&self) -> bool {
        matches!(self.outcome, Some(WizardOutcome::Installed { .. }))
    }

    pub fn cancelled(&self) -> bool {
        self.outcome.is_none()
    }

    pub fn failure(&self) -> Option<&InstallFailure> {
        match &self.outcome {
            Some(WizardOutcome::Failed(f)) => Some(f),
            _ => None,
        }
    }
}

/// Этап подготовки (заголовок окна хода не меняет; заголовки — дело G5).
pub const STAGE_PREPARE: &str = "app_prepare";
/// Этап работы корневого скрипта.
pub const STAGE_INSTALL: &str = "app_install";
/// Этап итога (есть в `de_progress::STAGE_TITLES`).
pub const STAGE_DONE: &str = "done";

// ============================================================================
// Корневой скрипт и план
// ============================================================================

/// Метка начала шага в выводе скрипта (stdout).
pub const STEP_MARK: &str = "REXON-STEP:";
/// Метка сломавшегося шага (stderr, из ловушки ERR).
pub const FAIL_MARK: &str = "REXON-FAILED:";
/// Префикс временного каталога установки.
pub const STAGE_PREFIX: &str = "rexon_install_";
/// Ярлык в меню приложений.
pub const DESKTOP_FILE: &str = "rexon.desktop";
/// Ярлык аварийного отката оформления.
pub const RECOVER_DESKTOP_FILE: &str = "rexon-recover.desktop";

/// Метки шагов корневого скрипта → шаг установки для человека. Правило «без
/// пароля» — ОТДЕЛЬНАЯ метка, хоть для человека это тот же шаг «пароль реже».
pub const SCRIPT_MARKS: &[(&str, InstallStep)] = &[
    ("deps", InstallStep::Dependencies),
    ("files", InstallStep::CopyFiles),
    ("launcher", InstallStep::Launcher),
    ("menu", InstallStep::MenuShortcuts),
    ("icons", InstallStep::Icons),
    ("helpers", InstallStep::FewerPasswords),
    ("rule", InstallStep::FewerPasswords),
    ("caches", InstallStep::MenuRefresh),
    ("extensions", InstallStep::WallpaperAddons),
];

fn mark_step(id: &str) -> Option<InstallStep> {
    SCRIPT_MARKS.iter().find(|(m, _)| *m == id.trim()).map(|(_, s)| *s)
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Раскладка временного каталога установки. Всё временное — внутри одного
/// каталога, поэтому уборка — одно удаление (починка 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagePaths {
    pub root: String,
}

impl StagePaths {
    pub fn new(root: &str) -> Self {
        StagePaths { root: root.trim_end_matches('/').to_string() }
    }

    /// Та же раскладка, что у цели: `lib/` уходит в `lib_dir`, `bin/` — в
    /// `bin_dir`.
    pub fn layout(&self) -> InstallLayout {
        InstallLayout::new(&format!("{}/lib", self.root), &format!("{}/bin", self.root))
    }

    pub fn menu_dir(&self) -> String {
        format!("{}/menu", self.root)
    }

    pub fn desktop(&self) -> String {
        format!("{}/{DESKTOP_FILE}", self.menu_dir())
    }

    pub fn recover_desktop(&self) -> String {
        format!("{}/{RECOVER_DESKTOP_FILE}", self.menu_dir())
    }

    pub fn icons_dir(&self) -> String {
        format!("{}/icons", self.root)
    }

    pub fn polkit_dir(&self) -> String {
        format!("{}/polkit", self.root)
    }

    /// Помощники, политики и правило — под именами их мест назначения.
    pub fn helpers(&self) -> HelperTmp {
        let p = |dest: &str| format!("{}/{}", self.polkit_dir(), basename(dest));
        HelperTmp {
            tmp_helper: p(inst::SYSTEM_HELPER_PATH),
            tmp_policy: p(inst::SYSTEM_POLICY_PATH),
            tmp_de_helper: p(plan::DE_HELPER_PATH),
            tmp_de_policy: p(plan::DE_POLICY_PATH),
            tmp_pkg_helper: p(plan::PKG_HELPER_PATH),
            tmp_pkg_policy: p(plan::PKG_POLICY_PATH),
            tmp_stage_helper: p(plan::STAGE_HELPER_PATH),
            tmp_stage_policy: p(plan::STAGE_POLICY_PATH),
            tmp_de_rules: p(plan::REXON_POLKIT_RULES_PATH),
        }
    }

    /// Помощник сеанса прав во временном каталоге (копия бинаря поставки).
    pub fn priv_helper(&self) -> String {
        format!("{}/{}", self.polkit_dir(), plan::SESSION_HELPER_NAME)
    }

    /// Политика `org.rexon.session` во временном каталоге.
    pub fn session_policy(&self) -> String {
        format!("{}/{}", self.polkit_dir(), basename(plan::SESSION_POLICY_PATH))
    }

    pub fn script(&self) -> String {
        format!("{}/install.sh", self.root)
    }
}

/// Путь временного каталога годен для скрипта без кавычек: блоки значков и
/// помощников ядра подставляют пути как есть.
fn stage_path_is_plain(path: &str) -> bool {
    path.starts_with('/')
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
}

/// Всё, из чего собирается корневой скрипт.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemScriptParams {
    pub target: InstallLayout,
    pub desktop_dir: String,
    /// Корень значков hicolor (для обновления кэша значков).
    pub icon_root: String,
    pub gnome_ext_dir: String,
    pub cinnamon_ext_dir: String,
    pub stage: StagePaths,
    pub with_wallpaper_host: bool,
    pub with_splash_overlay: bool,
    /// В поставке есть помощник сеанса прав ([`PayloadSource::priv_helper`]).
    pub with_priv_helper: bool,
    /// Значки во временном каталоге.
    pub icons: IconSet,
    pub passwordless_rule: bool,
    /// Строки установки зависимостей ([`deps_script_lines`]).
    pub dep_lines: String,
}

/// Строки установки зависимостей для корневого скрипта — порт
/// `_install_deps_cmd`. Разрядов три, и каждый отдельной строкой:
///
///   1. ОБЯЗАТЕЛЬНЫЕ ([`inst::dep_packages`]) — строго, под `set -e`: без
///      Qt6, layer-shell, загрузчика Vulkan и libmpv окно не откроется, и
///      честный отказ лучше молчаливо поломанного продукта.
///   2. ЖЕЛАТЕЛЬНЫЕ ([`inst::optional_dep_packages`]) — с `|| true`: это
///      база (bash, D-Bus, Wayland, GL, OpenSSL, polkit, тема значков),
///      которая на машине с рабочим столом стоит почти всегда, а имена её
///      пакетов в одиннадцати семействах из двенадцати выписаны по правилам
///      именования, а не проверены вживую. Опечатка здесь не должна стоить
///      человеку всей установки.
///   3. ПЕРЕКЛЮЧАТЕЛЬ СТИЛЯ Qt ([`inst::qt_style_install_cmd`]) — тоже с
///      `|| true`: без него Rexon работает.
///
/// ⚠️ Желательные ставятся ровно тогда же, когда обязательные (`deps_present`
/// — один ответ пробы `ldd` на всю поставку): если компоновщик доволен, в
/// системе есть и то, и другое, и звать пакетный менеджер незачем.
pub fn deps_script_lines(family: PkgFamily, deps_present: bool, qt_style_present: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    if !deps_present {
        let cmd = inst::dep_install_cmd(family);
        if !cmd.is_empty() {
            lines.push(cmd);
        }
        let optional = inst::optional_dep_install_cmd(family);
        if !optional.is_empty() {
            lines.push(format!("{optional} || true"));
        }
    }
    if !qt_style_present {
        let cmd = inst::qt_style_install_cmd(family);
        if !cmd.is_empty() {
            lines.push(format!("{cmd} || true"));
        }
    }
    lines.join("\n")
}

/// Каталог расширения GNOME Shell внутри данных продукта `rexon/`.
pub const GNOME_EXTENSION_SRC: &str = "gnome-extension";
/// Каталог расширения Cinnamon внутри данных продукта `rexon/`.
pub const CINNAMON_EXTENSION_SRC: &str = "cinnamon-extension";

/// Пары (каталог расширения в пакете, куда его ставить) — одни на установку
/// в систему и в домашнюю папку. Mutter и Muffin не умеют wlr-layer-shell:
/// обои там рисует расширение; ставится всегда — на случай перехода на
/// GNOME или Cinnamon.
pub fn extension_targets(gnome_ext_dir: &str, cinnamon_ext_dir: &str) -> [(&'static str, String); 2] {
    [
        (GNOME_EXTENSION_SRC, gnome_ext_dir.trim_end_matches('/').to_string()),
        (CINNAMON_EXTENSION_SRC, cinnamon_ext_dir.trim_end_matches('/').to_string()),
    ]
}

/// Программа сборки схем настроек расширения.
pub const GLIB_COMPILE_SCHEMAS: &str = "glib-compile-schemas";
/// Программа обновления базы ярлыков меню.
pub const UPDATE_DESKTOP_DATABASE: &str = "update-desktop-database";
/// Потолок одной команды обновления кэшей, с.
pub const CACHE_TIMEOUT_SECS: u64 = 120;

/// Обновление кэшей меню и значков — одни команды на оба режима. Группа —
/// взаимозаменяемые команды по порядку: первая удачная закрывает группу.
/// Ни одной программы группы нет — не ошибка.
pub fn cache_refresh_groups(desktop_dir: &str, icon_root: &str) -> Vec<Vec<Vec<String>>> {
    let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    vec![
        vec![argv(&[UPDATE_DESKTOP_DATABASE, desktop_dir])],
        vec![
            argv(&["gtk4-update-icon-cache", "-f", "-t", icon_root]),
            argv(&["gtk-update-icon-cache", "-f", "-t", icon_root]),
        ],
        vec![argv(&["kbuildsycoca6", "--noincremental"]), argv(&["kbuildsycoca5", "--noincremental"])],
    ]
}

/// Текст корневого скрипта установки в систему.
pub fn install_script(p: &SystemScriptParams) -> String {
    let q = |s: &str| crate::ext_manager::shlex_quote(s);
    let stage = p.stage.layout();
    let lib = q(&p.target.lib_dir);
    let desktop = q(&p.desktop_dir);
    let mut lines: Vec<String> = Vec::new();
    let mut line = |l: String| lines.push(l);
    let step = |id: &str| format!("\nrexon_step {id}");

    line("#!/bin/bash".into());
    line("# Установка Rexon в систему. Скрипт собран ядром Rexon.".into());
    line("set -eE".into());
    line("umask 022".into());
    // Терминала нет: команда, спросившая ввод, ждала бы вечно.
    line("exec </dev/null".into());
    line("REXON_STEP=start".into());
    line(format!("trap 'echo \"{FAIL_MARK}$REXON_STEP\" >&2' ERR"));
    line(format!("rexon_step() {{ REXON_STEP=\"$1\"; echo \"{STEP_MARK}$1\"; }}"));

    line(step("deps"));
    line(if p.dep_lines.is_empty() {
        "# Нужные программы уже установлены".into()
    } else {
        p.dep_lines.clone()
    });

    line(step("files"));
    line(format!("rm -rf {lib}"));
    line(format!("install -d -m755 {lib}"));
    // `каталог/.` — со скрытыми файлами; права уже разложены при подготовке.
    line(format!("cp -r --preserve=mode {}/. {lib}/", q(&stage.lib_dir)));
    line(format!("chown -R root:root {lib}"));

    line(step("launcher"));
    let mut bins = vec![(stage.launcher(), p.target.launcher())];
    if p.with_wallpaper_host {
        bins.push((stage.wallpaper_host(), p.target.wallpaper_host()));
    }
    if p.with_splash_overlay {
        bins.push((stage.splash_overlay(), p.target.splash_overlay()));
    }
    for (src, dst) in bins {
        line(format!("install -Dm755 {} {}", q(&src), q(&dst)));
    }

    line(step("menu"));
    line(format!("install -Dm644 {} {desktop}/rexon.desktop", q(&p.stage.desktop())));
    line(format!(
        "install -Dm644 {} {desktop}/rexon-recover.desktop",
        q(&p.stage.recover_desktop())
    ));

    line(step("icons"));
    line(inst::icon_install_block(&p.icons));

    let h = p.stage.helpers();
    line(step("helpers"));
    line(inst::helper_install_block(&h));
    // Помощник сеанса прав и его политика (`org.rexon.session`). Бинарь и
    // каталог над ним — root, без записи для группы и всех: иначе окно
    // помощника не поднимет (`privilege::client::check_ownership`). Нет
    // помощника в поставке — снимается прежний: от другой сборки окно его
    // всё равно отвергло бы, а без файла операции идут прежним путём.
    if p.with_priv_helper {
        line(format!("install -Dm755 {} {}", q(&p.stage.priv_helper()), plan::SESSION_HELPER_PATH));
        line(format!("install -Dm644 {} {}", q(&p.stage.session_policy()), plan::SESSION_POLICY_PATH));
    } else {
        line(format!("rm -f {}", plan::SESSION_HELPER_PATH));
    }
    if p.passwordless_rule {
        line(step("rule"));
    }
    line(inst::passwordless_rule_block(&h.tmp_de_rules, p.passwordless_rule));

    line(step("caches"));
    for group in cache_refresh_groups(&p.desktop_dir, &p.icon_root) {
        let alternatives: Vec<String> = group
            .iter()
            .map(|argv| format!("{} 2>/dev/null", argv.iter().map(|a| q(a)).collect::<Vec<_>>().join(" ")))
            .collect();
        line(format!("{} || true", alternatives.join(" || ")));
    }

    line(step("extensions"));
    let pairs: Vec<String> = extension_targets(&p.gnome_ext_dir, &p.cinnamon_ext_dir)
        .iter()
        .map(|(src, dest)| q(&format!("{src}|{dest}")))
        .collect();
    line(format!("for de_pair in {}; do", pairs.join(" ")));
    line("    DE_DIR=\"${de_pair%|*}\"".into());
    line("    DEST_BASE=\"${de_pair#*|}\"".into());
    line(format!("    EXT_SRC={}/\"$DE_DIR\"/{}", q(&p.target.package_dir()), inst::EXT_ID));
    line(format!("    EXT_DEST=\"$DEST_BASE\"/{}", inst::EXT_ID));
    line(format!("    rm -rf \"$DEST_BASE\"/{}", inst::EXT_ID_LEGACY));
    line("    if [ -d \"$EXT_SRC\" ]; then".into());
    line("        rm -rf \"$EXT_DEST\"".into());
    line("        mkdir -p \"$DEST_BASE\"".into());
    line("        cp -r --preserve=mode \"$EXT_SRC\" \"$EXT_DEST\"".into());
    line("        chown -R root:root \"$EXT_DEST\"".into());
    line(
        format!("        if command -v {GLIB_COMPILE_SCHEMAS} >/dev/null 2>&1 && [ -d \"$EXT_DEST/schemas\" ]; then"),
    );
    line(format!("            {GLIB_COMPILE_SCHEMAS} \"$EXT_DEST/schemas/\" 2>/dev/null || true"));
    line("        fi".into());
    line("    fi".into());
    line("done".into());
    lines.join("\n") + "\n"
}

/// Команда запуска корневого скрипта — тот же вид, что у
/// [`inst::system_install_cmd`] (argv и таймаут 600 с).
pub fn install_cmd(script_path: &str) -> Cmd {
    inst::system_install_cmd(script_path)
}

/// План: записать скрипт → запустить под паролем → убрать временный
/// каталог. Уборку исполнитель делает на ЛЮБОМ исходе, а не только после
/// удачного запуска.
pub fn install_plan(p: &SystemScriptParams) -> Vec<PlanStep> {
    let script = p.stage.script();
    vec![
        PlanStep::WriteFile { path: script.clone(), content: install_script(p), mode: 0o700 },
        PlanStep::Run {
            id: RunStepId::AppInstall,
            cmd: install_cmd(&script),
            needs_root: true,
            description: InstallStep::SystemInstall.progress_line(Lang::Ru),
        },
        PlanStep::RemoveFiles { paths: vec![p.stage.root.clone()] },
    ]
}

fn check_plan(steps: &[PlanStep]) -> bool {
    steps.iter().all(|s| {
        matches!(
            s,
            PlanStep::WriteFile { .. }
                | PlanStep::RemoveFiles { .. }
                | PlanStep::Run { id: RunStepId::AppInstall, .. }
        )
    })
}

// ============================================================================
// Разбор исхода скрипта
// ============================================================================

/// Метки: начатые шаги по порядку и шаг из ловушки ERR.
pub fn script_marks(out: &RunOut) -> (Vec<InstallStep>, Option<InstallStep>) {
    let mut started = Vec::new();
    let mut failed = None;
    for l in out.stdout.lines().chain(out.stderr.lines()) {
        let l = l.trim();
        if let Some(id) = l.strip_prefix(STEP_MARK) {
            if let Some(st) = mark_step(id) {
                started.push(st);
            }
        } else if let Some(id) = l.strip_prefix(FAIL_MARK) {
            failed = mark_step(id);
        }
    }
    (started, failed)
}

fn script_started(out: &RunOut) -> bool {
    out.stdout.lines().chain(out.stderr.lines()).any(|l| l.trim().starts_with(STEP_MARK))
}

/// Последние строки вывода без меток — подробности для разработчика.
pub fn output_tail(out: &RunOut, max_lines: usize) -> String {
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .chain(out.stderr.lines())
        .map(str::trim_end)
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with(STEP_MARK) && !t.starts_with(FAIL_MARK)
        })
        .collect();
    let from = lines.len().saturating_sub(max_lines);
    lines[from..].join("\n")
}

fn detail_with_code(code: Option<i32>, tail: &str) -> String {
    let head = match code {
        Some(c) => trf("exit code %1", &[&c]),
        None => tr("no exit code"),
    };
    if tail.is_empty() {
        head
    } else {
        format!("{head}\n{tail}")
    }
}

/// Исход запуска корневого скрипта.
///
/// Починка 1: «пароль не подтверждён» — только если скрипт НЕ начинался
/// (меток нет) и pkexec отказал (код 126/127 или текст агента —
/// [`plan::pkexec_auth_denied`]). Если метки есть — это сбой шага, даже при
/// коде 127 от команды внутри скрипта. Нечем спросить пароль (запуск не
/// состоялся) — [`InstallFailure::PasswordPromptMissing`]; таймаут —
/// [`InstallFailure::TimedOut`].
pub fn classify_script_run(out: &RunOut, timeout_secs: u64) -> Result<(), InstallFailure> {
    match out.outcome {
        RunOutcome::Error => return Err(InstallFailure::PasswordPromptMissing),
        RunOutcome::Timeout => return Err(InstallFailure::TimedOut { secs: timeout_secs }),
        _ => {}
    }
    let code = out.returncode;
    if code == Some(0) {
        return Ok(());
    }
    let tail = output_tail(out, 15);
    if script_started(out) {
        let (started, failed) = script_marks(out);
        let step = failed.or(started.last().copied()).unwrap_or(InstallStep::SystemInstall);
        return Err(InstallFailure::StepFailed { step, detail: detail_with_code(code, &tail) });
    }
    let c = code.unwrap_or(-1);
    if c == 127 || plan::pkexec_auth_denied(c, &out.stderr) {
        return Err(InstallFailure::PasswordDenied);
    }
    Err(InstallFailure::StepFailed {
        step: InstallStep::SystemInstall,
        detail: detail_with_code(code, &tail),
    })
}

// ============================================================================
// Исполнитель
// ============================================================================

/// Программа, показывающая динамические зависимости бинаря.
const LDD: &str = "ldd";
/// Ответ `ldd` о неразрешённой библиотеке (речь английская: запуск идёт с
/// `lang_c`, иначе перевод сообщения увёл бы проверку в сторону).
const LDD_MISSING_MARK: &str = "not found";
/// Потолок одной пробы, с: `ldd` только читает заголовки файла.
const LDD_TIMEOUT_SECS: u64 = 20;

/// Проба «поставка поднимется»: разрешаются ли динамические зависимости у
/// тех самых файлов, которые установка сейчас положит в систему — окна,
/// хоста обоев, заставки входа и библиотеки модели фильтров.
///
/// ⚠️ Прежде здесь спрашивали интерпретатор (`python3 -c "import
/// PySide6.QtWidgets"`) — это был вопрос про классическое окно, которого
/// поставка больше не везёт: Rust-окну PySide6 не нужен вовсе, а сама проба
/// падала бы на машине без интерпретатора и звала пакетный менеджер зря.
/// Спрашиваем ровно то, чем живёт Rust-поставка: библиотеки Qt6, layer-shell,
/// libmpv, Vulkan — всё это видно динамическому компоновщику.
///
/// Нечем проверить (в поставке нет ни одного бинаря либо в системе нет
/// `ldd`) — отвечаем «на месте»: молчаливое «нет» позвало бы пакетный
/// менеджер вслепую, а на машине без сети это оборвало бы установку целиком.
fn runtime_deps_present<IO: InstallIo + ?Sized>(io: &IO, payload: &PayloadSource) -> bool {
    let files: Vec<&String> = [
        payload.main_window.as_ref(),
        payload.wallpaper_host.as_ref(),
        payload.splash_overlay.as_ref(),
        payload.filter_lib.as_ref(),
    ]
    .into_iter()
    .flatten()
    .collect();
    if files.is_empty() || !io.which(LDD) {
        return true;
    }
    files.iter().all(|path| {
        let argv: Vec<String> = vec![LDD.to_string(), (*path).clone()];
        let out = io.run(&argv, Some(LDD_TIMEOUT_SECS), true);
        out.returncode == Some(0)
            && !out.stdout.contains(LDD_MISSING_MARK)
            && !out.stderr.contains(LDD_MISSING_MARK)
    })
}

/// Вход ярлыка ПОСЛЕ установки: правило одно с живой машиной, кандидат —
/// бинарь окна на его будущем месте.
pub fn installed_entry(entry_env: Option<&str>, target: &InstallLayout, has_main_window: bool) -> LaunchEntry {
    inst::choose_launch_entry(entry_env, &[target.main_window()], |_| has_main_window)
}

/// Значки поставки — пути источников. Мастер-копия берётся, только если нет
/// крупных размеров.
fn payload_icons<IO: SystemIo + ?Sized>(io: &IO, icon_dir: &str) -> IconSet {
    let dir = icon_dir.trim_end_matches('/');
    let mut set = IconSet::default();
    for size in ICON_SIZES {
        let src = format!("{dir}/rexon_icon_{size}.png");
        if io.is_file(&src) {
            set.sizes.push((*size, src));
        }
    }
    let svg = format!("{dir}/rexon_icon.svg");
    if io.is_file(&svg) {
        set.svg = Some(svg);
    }
    let has_big = set.sizes.iter().any(|(s, _)| matches!(s, 128 | 256 | 512));
    let master = format!("{dir}/rexon_icon.png");
    if !has_big && io.is_file(&master) {
        set.master = Some(master);
    }
    set
}

/// Значки из поставки для установки в систему: набор во временном каталоге
/// для блока установки ядра + пары (источник, путь во временном каталоге).
fn icon_plan<IO: SystemIo + ?Sized>(
    io: &IO,
    icon_dir: &str,
    stage: &StagePaths,
) -> (IconSet, Vec<(String, String)>) {
    let src = payload_icons(io, icon_dir);
    let mut set = IconSet::default();
    let mut copies = Vec::new();
    let mut stage_copy = |from: &String, name: String| {
        let dst = format!("{}/{name}", stage.icons_dir());
        copies.push((from.clone(), dst.clone()));
        dst
    };
    for (size, from) in &src.sizes {
        let dst = stage_copy(from, format!("rexon_icon_{size}.png"));
        set.sizes.push((*size, dst));
    }
    if let Some(from) = &src.svg {
        set.svg = Some(stage_copy(from, "rexon_icon.svg".into()));
    }
    if let Some(from) = &src.master {
        set.master = Some(stage_copy(from, "rexon_icon_master.png".into()));
    }
    (set, copies)
}

/// Параметры скрипта для этой машины и этого временного каталога.
/// Исполнитель зовёт ровно её — сверки строят тот же текст.
pub fn system_script_params<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &InstallRequest,
    stage: &StagePaths,
) -> SystemScriptParams {
    let sys = inst::install_paths(InstallMode::System, "", None);
    let (icons, _) = icon_plan(io, &req.payload.icon_dir, stage);
    let qt_style_present = io.which("qt6ct") || io.which("qt5ct");
    SystemScriptParams {
        target: InstallLayout::system(),
        desktop_dir: sys.desktop_dir.unwrap_or_default(),
        icon_root: sys.icon_root.unwrap_or_default(),
        gnome_ext_dir: sys.gnome_ext_dir.unwrap_or_default(),
        cinnamon_ext_dir: sys.cinnamon_ext_dir.unwrap_or_default(),
        stage: stage.clone(),
        with_wallpaper_host: req.payload.wallpaper_host.is_some(),
        with_splash_overlay: req.payload.splash_overlay.is_some(),
        with_priv_helper: req.payload.priv_helper.is_some(),
        icons,
        passwordless_rule: req.passwordless_rule,
        dep_lines: deps_script_lines(
            req.family,
            runtime_deps_present(io, &req.payload),
            qt_style_present,
        ),
    }
}

/// Ошибка подготовки шага — сбой этого шага, подробность — текст ошибки.
fn prep<T>(step: InstallStep, r: Result<T, String>) -> Result<T, InstallFailure> {
    r.map_err(|detail| InstallFailure::StepFailed { step, detail })
}

/// Итог установки ЛЮБОГО режима: строка итога в ход дела и проверка, что
/// каталог команды `rexon` в `PATH` (`command_dir` — `None`, когда команды
/// нет, как у переносного режима). Одна на все режимы.
fn finish<IO: InstallIo + ?Sized>(
    io: &IO,
    sink: &dyn DeOpSink,
    lang: Lang,
    mode: InstallMode,
    command_dir: Option<&str>,
    res: Result<Option<LaunchEntry>, Option<InstallFailure>>,
) -> InstallResult {
    match res {
        Ok(entry) => {
            let message = inst::install_done_line(mode, lang);
            sink.progress(STAGE_DONE, &message, Some(1.0));
            let path_env = io.env("PATH");
            let not_in_path = command_dir
                .filter(|dir| !inst::command_dir_in_path(dir, Some(&path_env)))
                .map(str::to_string);
            InstallResult {
                outcome: Some(WizardOutcome::Installed {
                    mode,
                    fell_back: None,
                    command_dir_not_in_path: not_in_path,
                }),
                entry,
                message,
            }
        }
        Err(None) => {
            let message = crate::system_de_exec::cancelled_message();
            sink.progress(crate::system_de_exec::STATUS_CANCELLED, &message, None);
            InstallResult { outcome: None, entry: None, message }
        }
        Err(Some(failure)) => {
            let message = failure.log_line(lang);
            sink.progress(de_progress::STAGE_ERROR, &message, None);
            InstallResult { outcome: Some(WizardOutcome::Failed(failure)), entry: None, message }
        }
    }
}

/// Папки настроек и резервных копий — одни на все режимы (починка 8).
/// Пусто, если домашняя папка не определена: относительный путь смотрел бы
/// в текущий каталог процесса. Пути — из единого перечня
/// [`inst::user_data_roots`]: первые по XDG настройки и резервные копии.
pub fn profile_dirs(home: &str, xdg_config_home: &str, xdg_data_home: &str) -> Vec<String> {
    let roots = inst::user_data_roots(home, xdg_config_home, xdg_data_home, "");
    [inst::UserDataKind::Settings, inst::UserDataKind::Backups]
        .into_iter()
        .filter_map(|kind| roots.iter().find(|(k, _)| *k == kind).map(|(_, p)| p.clone()))
        .collect()
}

/// Создать папки профиля. Не беда, если не вышло: Rust создаёт их сам при
/// первом обращении.
fn make_profile_dirs<IO: InstallIo + ?Sized>(io: &IO) {
    for dir in profile_dirs(&io.env("HOME"), &io.env("XDG_CONFIG_HOME"), &io.env("XDG_DATA_HOME")) {
        let _ = io.make_dirs(&dir, None);
    }
}

/// Файлы программы в `staged.lib_dir` с правильными правами: данные
/// продукта `rexon/` (без исходников Python — [`payload_skip`]), бинарь
/// окна, QML-модули, наша `.so`. Одна подготовка на установку в систему и в
/// домашнюю папку.
fn stage_program_files<IO: InstallIo + ?Sized>(
    io: &IO,
    pay: &PayloadSource,
    staged: &InstallLayout,
) -> Result<(), InstallFailure> {
    use InstallStep::CopyFiles;
    prep(CopyFiles, io.make_dirs(&staged.lib_dir, Some(0o755)))?;
    prep(CopyFiles, io.copy_tree(&pay.package_dir, &staged.package_dir(), &payload_skip, &payload_mode))?;
    if let Some(src) = &pay.main_window {
        prep(CopyFiles, io.copy_file_mode(src, &staged.main_window(), 0o755))?;
        // Помощник снимков стилей собирается и лежит рядом с окном в любой
        // поставке; нет его — плитки получат нарисованные образцы.
        let helper = std::path::Path::new(src).with_file_name(crate::widget_shot::HELPER_BINARY);
        let helper = helper.to_string_lossy();
        if io.is_file(&helper) {
            prep(CopyFiles, io.copy_file_mode(&helper, &staged.theme_shot(), 0o755))?;
        }
    }
    if let Some(src) = &pay.qml_modules {
        prep(CopyFiles, io.copy_tree(src, &staged.qml_modules(), &payload_skip, &payload_mode))?;
    }
    if let Some(src) = &pay.filter_lib {
        prep(CopyFiles, io.make_dirs(&staged.rust_lib_dir(), Some(0o755)))?;
        prep(CopyFiles, io.copy_file_mode(src, &staged.filter_lib(), 0o755))?;
    }
    Ok(())
}

/// Установить Rexon в систему. Окно зовёт не её, а диспетчер [`install`]:
/// он заранее уводит неизменяемую систему в домашнюю папку.
///
/// Порядок: отказ неизменяемой системы (до побочных действий) → временный
/// каталог → подготовка файлов с правильными правами → корневой скрипт под
/// одним паролем → проверка, что Rexon на месте → папки профиля. Временный
/// каталог убирается на ЛЮБОМ исходе после его создания. Отменить можно,
/// пока не запрошен пароль.
pub fn install_to_system<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &InstallRequest,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
) -> InstallResult {
    let lang = req.lang;
    let target = InstallLayout::system();
    let done = |res: Result<LaunchEntry, Option<InstallFailure>>| {
        finish(io, sink.as_ref(), lang, InstallMode::System, Some(target.bin_dir.as_str()), res.map(Some))
    };

    // Неизменяемая система: сюда дойти не должно — «в домашнюю папку»
    // выбирает диспетчер [`install`]. Отказ — до первого побочного действия.
    if let Some(reason) = inst::system_mode_blocked_reason_in(&req.distro_id, lang) {
        return done(Err(Some(InstallFailure::StepFailed {
            step: InstallStep::SystemInstall,
            detail: reason,
        })));
    }
    sink.cancellable(true);
    sink.progress(STAGE_PREPARE, &InstallStep::CopyFiles.progress_line(lang), Some(0.02));
    let root = match io.temp_dir(STAGE_PREFIX, None) {
        Ok(r) => r,
        Err(e) => {
            sink.cancellable(false);
            return done(Err(Some(InstallFailure::StepFailed { step: InstallStep::CopyFiles, detail: e })));
        }
    };
    let res = run_staged(io, req, sink.as_ref(), cancel, &StagePaths::new(&root));
    // Починка 2: уборка на любом исходе — успех, отказ пароля, сбой шага,
    // ошибка подготовки, отмена, таймаут.
    io.remove_tree(&root);
    sink.cancellable(false);
    done(res)
}

fn run_staged<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &InstallRequest,
    sink: &dyn DeOpSink,
    cancel: &CancelToken,
    stage: &StagePaths,
) -> Result<LaunchEntry, Option<InstallFailure>> {
    let lang = req.lang;
    let say = |step: InstallStep, fraction: f64| {
        sink.progress(STAGE_PREPARE, &step.progress_line(lang), Some(fraction));
    };
    if !stage_path_is_plain(&stage.root) {
        return Err(Some(InstallFailure::StepFailed {
            step: InstallStep::CopyFiles,
            detail: trf("the temporary folder path has spaces or special characters: %1", &[&stage.root]),
        }));
    }
    let params = system_script_params(io, req, stage);
    let steps = install_plan(&params);
    if !check_plan(&steps) {
        return Err(Some(InstallFailure::StepFailed {
            step: InstallStep::SystemInstall,
            detail: tr("the installation plan contains an unknown step"),
        }));
    }
    let target = &params.target;
    let staged = stage.layout();
    let pay = &req.payload;
    let entry = installed_entry(req.entry_env.as_deref(), target, pay.main_window.is_some());
    let cancelled = || cancel.is_cancelled();

    // ── Файлы программы ─────────────────────────────────────────────
    use InstallStep::*;
    prep(CopyFiles, io.make_dirs(&staged.bin_dir, Some(0o755)))?;
    stage_program_files(io, pay, &staged)?;
    if cancelled() {
        return Err(None);
    }

    // ── Команды запуска ─────────────────────────────────────────────
    say(Launcher, 0.12);
    for (src, dst) in [
        (&pay.wallpaper_host, staged.wallpaper_host()),
        (&pay.splash_overlay, staged.splash_overlay()),
    ] {
        if let Some(src) = src {
            prep(Launcher, io.copy_file_mode(src, &dst, 0o755))?;
        }
    }
    prep(
        Launcher,
        io.write_file_mode(&staged.launcher(), &inst::system_launcher(&target.lib_dir), 0o755),
    )?;

    // ── Ярлык ───────────────────────────────────────────────────────
    say(MenuShortcuts, 0.18);
    prep(MenuShortcuts, io.make_dirs(&stage.menu_dir(), Some(0o755)))?;
    prep(MenuShortcuts, io.write_file_mode(&stage.desktop(), &inst::desktop_content_for(&entry), 0o644))?;
    prep(
        MenuShortcuts,
        io.write_file_mode(&stage.recover_desktop(), &inst::recover_desktop_content_for(&entry), 0o644),
    )?;

    // ── Значки ──────────────────────────────────────────────────────
    say(Icons, 0.24);
    let (_, icon_copies) = icon_plan(io, &pay.icon_dir, stage);
    if icon_copies.is_empty() {
        sink.progress(de_progress::STAGE_WARNING, &inst::icons_missing_notice(lang), None);
    } else {
        prep(Icons, io.make_dirs(&stage.icons_dir(), Some(0o755)))?;
        for (src, dst) in &icon_copies {
            prep(Icons, io.copy_file_mode(src, dst, 0o644))?;
        }
    }

    // ── Помощники и (по согласию) правило ───────────────────────────
    say(FewerPasswords, 0.3);
    let h = stage.helpers();
    prep(FewerPasswords, io.make_dirs(&stage.polkit_dir(), Some(0o755)))?;
    let texts: [(&str, String, u32); 8] = [
        (&h.tmp_helper, inst::system_helper_script(), 0o755),
        (&h.tmp_policy, inst::system_policy_xml(), 0o644),
        (&h.tmp_de_helper, plan::de_helper_script(), 0o755),
        (&h.tmp_de_policy, plan::de_policy_xml(), 0o644),
        (&h.tmp_pkg_helper, plan::pkg_helper_script(), 0o755),
        (&h.tmp_pkg_policy, plan::pkg_policy_xml(), 0o644),
        (&h.tmp_stage_helper, plan::stage_helper_script(), 0o755),
        (&h.tmp_stage_policy, plan::stage_policy_xml(), 0o644),
    ];
    for (path, text, mode) in &texts {
        prep(FewerPasswords, io.write_file_mode(path, text, *mode))?;
    }
    if let Some(src) = &pay.priv_helper {
        prep(FewerPasswords, io.copy_file_mode(src, &stage.priv_helper(), 0o755))?;
        prep(FewerPasswords, io.write_file_mode(&stage.session_policy(), &plan::session_policy_xml(), 0o644))?;
    }
    if req.passwordless_rule {
        prep(
            FewerPasswords,
            io.write_file_mode(&h.tmp_de_rules, &plan::passwordless_rules_js(req.family), 0o644),
        )?;
    }
    if !params.dep_lines.is_empty() {
        sink.progress(STAGE_PREPARE, &inst::dependencies_with_rexon_notice(lang), None);
    }
    if cancelled() {
        return Err(None);
    }

    // ── План: скрипт → пароль ───────────────────────────────────────
    let mut run_out: Option<(RunOut, u64)> = None;
    for step in &steps {
        match step {
            PlanStep::WriteFile { path, content, mode } => {
                prep(SystemInstall, io.write_file_mode(path, content, *mode))?;
            }
            PlanStep::Run { cmd, .. } => {
                sink.cancellable(false);
                sink.progress(STAGE_INSTALL, &SystemInstall.progress_line(lang), Some(0.45));
                let out = io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
                run_out = Some((out, cmd.timeout_secs.unwrap_or(inst::SYSTEM_INSTALL_TIMEOUT_SECS)));
            }
            // Уборку делает `install_to_system` на любом исходе.
            PlanStep::RemoveFiles { .. } => {}
            _ => {
                return Err(Some(InstallFailure::StepFailed {
                    step: SystemInstall,
                    detail: tr("the installation plan contains an unknown step"),
                }))
            }
        }
    }
    let (out, timeout) = run_out.unwrap_or_else(|| (RunOut::missing(), inst::SYSTEM_INSTALL_TIMEOUT_SECS));
    // Журнал: шаги, которые скрипт прошёл до конца.
    let verdict = classify_script_run(&out, timeout);
    let (started, _) = script_marks(&out);
    let finished = if verdict.is_ok() { started.len() } else { started.len().saturating_sub(1) };
    let mut last: Option<InstallStep> = None;
    for st in started.iter().take(finished) {
        if last != Some(*st) {
            sink.progress(STAGE_INSTALL, &format!("✅ {}", st.progress_line(lang).trim_end_matches('…')), None);
        }
        last = Some(*st);
    }
    verdict.map_err(Some)?;

    // ── Проверка: Rexon на месте ────────────────────────────────────
    let sys = inst::install_paths(InstallMode::System, "", None);
    let in_place = inst::system_or_user_installed(&sys, |p| io.exists(p), |p| io.is_dir(p))
        && match &entry {
            LaunchEntry::MainWindow(path) => io.is_executable(path),
            LaunchEntry::Python => true,
        };
    if !in_place {
        return Err(Some(InstallFailure::StepFailed {
            step: SystemInstall,
            detail: trf(
                "the administrator script finished successfully, but %1 or %2 was not found",
                &[
                    &target.launcher(),
                    &match &entry {
                        LaunchEntry::MainWindow(p) => p.clone(),
                        LaunchEntry::Python => target.package_dir(),
                    },
                ],
            ),
        }));
    }

    // ── Папки профиля (без пароля) ──────────────────────────────────
    sink.progress(STAGE_INSTALL, &UserFolders.progress_line(lang), Some(0.95));
    make_profile_dirs(io);
    Ok(entry)
}

// ============================================================================
// Установка в домашнюю папку, переносной режим и диспетчер
// ============================================================================

/// Префикс временного каталога установки в домашнюю папку: скрытый и рядом
/// с каталогом установки — та же файловая система, обмен одним
/// переименованием (починка 9).
pub const USER_STAGE_PREFIX: &str = ".rexon_install_";

/// Что просят установить в домашнюю папку — часть [`InstallRequest`]:
/// пароля, зависимостей и правила «без пароля» у этого режима нет.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInstallRequest {
    pub payload: PayloadSource,
    /// Значение `REXON_ENTRY` (откат на классическое окно).
    pub entry_env: Option<String>,
    /// Язык строк хода и итога.
    pub lang: Lang,
}

impl UserInstallRequest {
    pub fn new(payload: PayloadSource, lang: Lang) -> Self {
        UserInstallRequest { payload, entry_env: None, lang }
    }
}

impl InstallRequest {
    /// Тот же запрос для установки в домашнюю папку.
    pub fn for_user(&self) -> UserInstallRequest {
        UserInstallRequest {
            payload: self.payload.clone(),
            entry_env: self.entry_env.clone(),
            lang: self.lang,
        }
    }
}

fn parent_dir(path: &str) -> &str {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) => "/",
        Some((dir, _)) => dir,
        None => ".",
    }
}

/// Существующий путь ведёт не туда, где лежит, — это ссылка (у разработчика
/// `~/.local/lib/rexon` бывает ссылкой в дерево). Сравнение через реальный
/// путь РОДИТЕЛЯ: у Bazzite и Silverblue сама `/home` — ссылка на
/// `/var/home`, и это не повод отказывать.
pub fn is_redirected<IO: SystemIo + ?Sized>(io: &IO, path: &str) -> bool {
    let path = path.trim_end_matches('/');
    if !io.exists(path) {
        return false;
    }
    match (io.realpath(path), io.realpath(parent_dir(path))) {
        (Some(real), Some(parent)) => real != format!("{}/{}", parent.trim_end_matches('/'), basename(path)),
        _ => false,
    }
}

/// Поставить файл через временное имя рядом и переименование: запущенный
/// бинарь перезаписью не открыть (`ETXTBSY`), а ссылку на месте
/// переименование заменяет, не заходя в её цель (починки 12, 13).
fn place_file<IO: InstallIo + ?Sized>(io: &IO, src: &str, dst: &str, mode: u32) -> Result<(), String> {
    let tmp = format!("{dst}.rexon-new");
    io.copy_file_mode(src, &tmp, mode).and_then(|()| io.rename(&tmp, dst)).inspect_err(|_| {
        io.remove_file(&tmp);
    })
}

/// Записать текстовый файл на место существующего: прежний путь сперва
/// снимается, чтобы запись не ушла в цель ссылки (починка 12).
fn replace_text<IO: InstallIo + ?Sized>(io: &IO, path: &str, content: &str, mode: u32) -> Result<(), String> {
    io.remove_file(path);
    io.write_file_mode(path, content, mode)
}

/// Кэши меню и значков по [`cache_refresh_groups`]. Нет программы — не
/// ошибка; без `update-desktop-database` человек узнаёт, что ярлык появится
/// после повторного входа.
/// `sink` — `None` у удаления: там «ярлык появится позже» было бы неправдой.
fn refresh_caches<IO: InstallIo + ?Sized>(
    io: &IO,
    sink: Option<&dyn DeOpSink>,
    lang: Lang,
    desktop_dir: &str,
    icon_root: &str,
) {
    for group in cache_refresh_groups(desktop_dir, icon_root) {
        let present: Vec<&Vec<String>> = group.iter().filter(|argv| io.which(&argv[0])).collect();
        if present.is_empty() {
            if let (Some(sink), true) = (sink, group[0][0] == UPDATE_DESKTOP_DATABASE) {
                sink.progress(de_progress::STAGE_WARNING, &inst::menu_refresh_later_notice(lang), None);
            }
            continue;
        }
        for argv in present {
            if io.run(argv, Some(CACHE_TIMEOUT_SECS), false).returncode == Some(0) {
                break;
            }
        }
    }
}

/// Расширение обоев одной оболочки в `dest_base`: прежнее имя (домен чужого
/// проекта) снимается всегда, иначе обои применяются дважды; своего
/// расширения в поставке нет — не ошибка.
fn install_extension<IO: InstallIo + ?Sized>(io: &IO, package_dir: &str, src_name: &str, dest_base: &str) -> Result<(), String> {
    io.remove_tree(&format!("{dest_base}/{}", inst::EXT_ID_LEGACY));
    let src = format!("{}/{src_name}/{}", package_dir.trim_end_matches('/'), inst::EXT_ID);
    if !io.is_dir(&src) {
        return Ok(());
    }
    let dst = format!("{dest_base}/{}", inst::EXT_ID);
    io.make_dirs(dest_base, None)?;
    io.remove_tree(&dst);
    io.copy_tree(&src, &dst, &payload_skip, &payload_mode)?;
    let schemas = format!("{dst}/schemas");
    if io.is_dir(&schemas) && io.which(GLIB_COMPILE_SCHEMAS) {
        io.run(&[GLIB_COMPILE_SCHEMAS.to_string(), schemas], Some(CACHE_TIMEOUT_SECS), false);
    }
    Ok(())
}

/// Установить Rexon в домашнюю папку — порт `RexonInstaller.install_user`.
///
/// Без пароля. Порядок: файлы программы собираются во временном каталоге
/// рядом с каталогом установки → прежняя и новая копии меняются местами →
/// команды запуска, ярлыки, значки, кэши меню, дополнения обоев → проверка,
/// что Rexon на месте → папки профиля. Временный каталог (и прежняя копия в
/// нём) убирается на ЛЮБОМ исходе после его создания. Отменить можно до
/// обмена копий — тогда установленное прежде не тронуто.
pub fn install_for_user<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &UserInstallRequest,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
) -> InstallResult {
    let lang = req.lang;
    let home = io.env("HOME").trim_end_matches('/').to_string();
    let xdg_data = io.env("XDG_DATA_HOME");
    let xdg_data = Some(xdg_data.as_str()).filter(|x| !x.is_empty());
    let target = inst::user_install_lib_dir(&home)
        .and_then(|_| InstallLayout::for_mode(InstallMode::User, &home, xdg_data));
    let Some(target) = target else {
        return finish(
            io,
            sink.as_ref(),
            lang,
            InstallMode::User,
            None,
            Err(Some(InstallFailure::StepFailed {
                step: InstallStep::CopyFiles,
                detail: trf("the home folder is not known (HOME=%1)", &[&format!("{home:?}")]),
            })),
        );
    };
    let paths = inst::install_paths(InstallMode::User, &home, xdg_data);
    let done = |res: Result<LaunchEntry, Option<InstallFailure>>| {
        finish(io, sink.as_ref(), lang, InstallMode::User, Some(target.bin_dir.as_str()), res.map(Some))
    };

    sink.cancellable(true);
    sink.progress(STAGE_PREPARE, &InstallStep::CopyFiles.progress_line(lang), Some(0.02));
    let parent = parent_dir(&target.lib_dir).to_string();
    let root = io.make_dirs(&parent, None).and_then(|()| io.temp_dir(USER_STAGE_PREFIX, Some(&parent)));
    let root = match root {
        Ok(r) => r,
        Err(detail) => {
            sink.cancellable(false);
            return done(Err(Some(InstallFailure::StepFailed { step: InstallStep::CopyFiles, detail })));
        }
    };
    let res = run_user(io, req, sink.as_ref(), cancel, &paths, &target, &StagePaths::new(&root));
    io.remove_tree(&root);
    sink.cancellable(false);
    done(res)
}

fn run_user<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &UserInstallRequest,
    sink: &dyn DeOpSink,
    cancel: &CancelToken,
    paths: &inst::InstallPaths,
    target: &InstallLayout,
    stage: &StagePaths,
) -> Result<LaunchEntry, Option<InstallFailure>> {
    use InstallStep::*;
    let lang = req.lang;
    let pay = &req.payload;
    let say = |step: InstallStep, fraction: f64| {
        sink.progress(STAGE_INSTALL, &step.progress_line(lang), Some(fraction));
    };
    let dir = |d: &Option<String>| d.clone().unwrap_or_default();
    let entry = installed_entry(req.entry_env.as_deref(), target, pay.main_window.is_some());

    if is_redirected(io, &target.lib_dir) {
        return Err(Some(InstallFailure::StepFailed {
            step: CopyFiles,
            detail: trf(
                "%1 is a link to another folder; the installation won't replace it",
                &[&target.lib_dir],
            ),
        }));
    }
    if cancel.is_cancelled() {
        return Err(None);
    }

    // ── Файлы программы: сборка рядом, затем обмен (починка 9) ──────
    let staged = stage.layout();
    stage_program_files(io, pay, &staged)?;
    if cancel.is_cancelled() {
        return Err(None);
    }
    // Дальше домашняя папка меняется — отмена недоступна.
    sink.cancellable(false);
    let previous = format!("{}/previous", stage.root);
    let had_previous = io.exists(&target.lib_dir);
    if had_previous {
        prep(CopyFiles, io.rename(&target.lib_dir, &previous))?;
    }
    if let Err(detail) = io.rename(&staged.lib_dir, &target.lib_dir) {
        if had_previous {
            let _ = io.rename(&previous, &target.lib_dir);
        }
        return Err(Some(InstallFailure::StepFailed { step: CopyFiles, detail }));
    }

    // ── Команды запуска ─────────────────────────────────────────────
    say(Launcher, 0.4);
    prep(Launcher, io.make_dirs(&target.bin_dir, None))?;
    for (src, dst) in [
        (&pay.wallpaper_host, target.wallpaper_host()),
        (&pay.splash_overlay, target.splash_overlay()),
    ] {
        if let Some(src) = src {
            prep(Launcher, place_file(io, src, &dst, 0o755))?;
        }
    }
    let launcher = inst::user_launcher(&target.lib_dir);
    prep(Launcher, replace_text(io, &target.launcher(), &launcher, 0o755))?;

    // ── Ярлыки ──────────────────────────────────────────────────────
    say(MenuShortcuts, 0.5);
    let desktop_dir = dir(&paths.desktop_dir);
    prep(MenuShortcuts, io.make_dirs(&desktop_dir, None))?;
    for (name, text) in [
        (DESKTOP_FILE, inst::desktop_content_for(&entry)),
        (RECOVER_DESKTOP_FILE, inst::recover_desktop_content_for(&entry)),
    ] {
        prep(MenuShortcuts, replace_text(io, &format!("{desktop_dir}/{name}"), &text, 0o644))?;
    }

    // ── Значки ──────────────────────────────────────────────────────
    say(Icons, 0.6);
    let icon_root = dir(&paths.icon_root);
    let icons = inst::icon_targets(&payload_icons(io, &pay.icon_dir), &icon_root, &dir(&paths.pixmap_dir));
    if icons.is_empty() {
        sink.progress(de_progress::STAGE_WARNING, &inst::icons_missing_notice(lang), None);
    }
    for (src, dst) in &icons {
        prep(Icons, io.make_dirs(parent_dir(dst), None))?;
        prep(Icons, place_file(io, src, dst, 0o644))?;
    }

    // ── Кэши меню и значков ─────────────────────────────────────────
    say(MenuRefresh, 0.7);
    refresh_caches(io, Some(sink), lang, &desktop_dir, &icon_root);

    // ── Дополнения обоев GNOME и Cinnamon ───────────────────────────
    say(WallpaperAddons, 0.8);
    for (src_name, dest_base) in extension_targets(&dir(&paths.gnome_ext_dir), &dir(&paths.cinnamon_ext_dir)) {
        prep(WallpaperAddons, install_extension(io, &target.package_dir(), src_name, &dest_base))?;
    }

    // ── Проверка: Rexon на месте ────────────────────────────────────
    let in_place = inst::system_or_user_installed(paths, |p| io.exists(p), |p| io.is_dir(p))
        && match &entry {
            LaunchEntry::MainWindow(path) => io.is_executable(path),
            LaunchEntry::Python => true,
        };
    if !in_place {
        return Err(Some(InstallFailure::StepFailed {
            step: CopyFiles,
            detail: trf("%1 or %2 was not found after the installation", &[&target.launcher(), &target.lib_dir]),
        }));
    }

    // ── Папки профиля ───────────────────────────────────────────────
    say(UserFolders, 0.95);
    make_profile_dirs(io);
    Ok(entry)
}

/// Переносной режим — порт `_install_portable`: ставить нечего, готовятся
/// только папки профиля. Итог — [`inst::portable_no_install_notice`].
pub fn install_portable<IO: InstallIo + ?Sized>(io: &IO, lang: Lang, sink: &dyn DeOpSink) -> InstallResult {
    sink.progress(STAGE_PREPARE, &InstallStep::UserFolders.progress_line(lang), Some(0.5));
    make_profile_dirs(io);
    finish(io, sink, lang, InstallMode::Portable, None, Ok(None))
}

/// Поставки без бинаря окна нет: ставить было бы нечего. Порядок проверки —
/// до первого побочного действия.
///
/// ⭐ Починка 16.09.2026: прежде такая установка проходила «успешно» и
/// уводила ярлык на классическое окно (`LaunchEntry::Python`). Питона в
/// поставке больше нет, а команда `rexon` теперь сама ведёт на бинарь окна
/// ([`inst::system_launcher`]) — установка без него положила бы в систему
/// лаунчер, указывающий в пустоту.
fn payload_has_window(payload: &PayloadSource) -> Result<(), InstallFailure> {
    if payload.main_window.is_some() {
        return Ok(());
    }
    Err(InstallFailure::StepFailed {
        step: InstallStep::CopyFiles,
        detail: trf(
            "the Rexon window is missing from the package: %1 was not found next to it",
            &[&crate::window_menu::MAIN_WINDOW_BIN],
        ),
    })
}

/// Установить Rexon в выбранном режиме — вход для окна мастера.
///
/// Защита от дурака (правило #13): установка в систему там, где системные
/// папки неизменяемы ([`inst::system_mode_blocked_reason_in`] — то же
/// решение, что `first_run_offer` отдаёт окну как `system_blocked`), молча
/// превращается в установку в домашнюю папку ДО первого побочного действия;
/// причина уходит в ход дела и в итог (`fell_back`). Починка 10.
pub fn install<IO: InstallIo + ?Sized>(
    io: &IO,
    mode: InstallMode,
    req: &InstallRequest,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
) -> InstallResult {
    if mode != InstallMode::Portable {
        if let Err(failure) = payload_has_window(&req.payload) {
            return finish(io, sink.as_ref(), req.lang, mode, None, Err(Some(failure)));
        }
    }
    match mode {
        InstallMode::Portable => install_portable(io, req.lang, sink.as_ref()),
        InstallMode::User => install_for_user(io, &req.for_user(), sink, cancel),
        InstallMode::System => match inst::system_mode_blocked_reason_in(&req.distro_id, req.lang) {
            None => install_to_system(io, req, sink, cancel),
            Some(reason) => {
                sink.progress(STAGE_PREPARE, &reason, None);
                let mut result = install_for_user(io, &req.for_user(), sink, cancel);
                if let Some(WizardOutcome::Installed { fell_back, .. }) = &mut result.outcome {
                    *fell_back = Some(reason);
                }
                result
            }
        },
    }
}

// ============================================================================
// Удаление Rexon
// ============================================================================
//
// Порт `RexonUninstaller` (`rexon/installer.py`): пакет дистрибутива →
// установка для всех → установка для себя → настройки и резервные копии.
// Решения — в ядре ([`inst::uninstall_system_paths`],
// [`inst::uninstall_user_paths`], [`inst::get_installed_files`]), здесь —
// исполнение через [`InstallIo`]. Починки 15–19 — в шапке модуля.

/// Потолок запроса «чей это файл» к пакетному менеджеру, с.
pub const OWNER_QUERY_TIMEOUT_SECS: u64 = 10;
/// Префикс временной папки корневого скрипта удаления.
pub const UNINSTALL_STAGE_PREFIX: &str = "rexon_uninstall_";
/// Этап работы удаления.
pub const STAGE_UNINSTALL: &str = "app_uninstall";

/// Пакет дистрибутива, которым поставлен Rexon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistroPackage {
    pub family: PkgFamily,
    pub name: String,
}

/// Порт `_detect_distro_package`: спросить у пакетного менеджера владельца
/// наших файлов. `None` — ручная, переносная или разработческая копия.
///
/// `own_exe` — путь своего исполняемого файла: у Rust нет `__file__`, а
/// владельца надо спрашивать про тот файл, который реально стоит в системе.
pub fn detect_distro_package<IO: SystemIo + ?Sized>(
    io: &IO,
    family: PkgFamily,
    own_exe: Option<&str>,
) -> Option<DistroPackage> {
    let owner = inst::distro_pkg_owner(family)?;
    let tool = owner.first()?.clone();
    if !io.which(&tool) {
        return None;
    }
    let mut candidates: Vec<String> = own_exe.and_then(|p| io.realpath(p)).into_iter().collect();
    for bin in inst::distro_candidate_bins() {
        if io.exists(bin) {
            candidates.extend(io.realpath(bin));
        }
    }
    candidates.into_iter().find_map(|path| {
        let mut argv = owner.clone();
        argv.push(path);
        let out = io.run(&argv, Some(OWNER_QUERY_TIMEOUT_SECS), true);
        if out.returncode != Some(0) {
            return None;
        }
        inst::parse_owner_output(&tool, &out.stdout).map(|name| DistroPackage { family, name })
    })
}

/// Что удалять.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallRequest {
    /// Пакет дистрибутива ([`detect_distro_package`]).
    pub package: Option<DistroPackage>,
    /// Найдена установка для всех пользователей.
    pub system: bool,
    /// Найдена установка в домашнюю папку.
    pub user: bool,
    /// «Удалить, но сохранить настройки и резервные копии» (решение
    /// Wehrwolfmann 15.09.2026): настройки (`$XDG_CONFIG_HOME/rexon`, в ней
    /// же журнал установленных тем `ocs_installed.json`), резервные копии
    /// рабочих столов и установленные обои, темы и дополнения (всё в
    /// `$XDG_DATA_HOME/rexon`) не трогаются, чтобы после повторной установки
    /// продолжить с того же места. Временные файлы (`$XDG_CACHE_HOME/rexon`)
    /// удаляются при любом выборе. Перечень — [`inst::user_data_roots`].
    pub keep_profile: bool,
    /// Язык строк хода и итога.
    pub lang: Lang,
}

/// Пути установки в домашнюю папку; `None` — домашняя папка не определена
/// (удалять по относительному пути нельзя).
fn user_install_paths<IO: SystemIo + ?Sized>(io: &IO) -> Option<inst::InstallPaths> {
    let home = io.env("HOME").trim_end_matches('/').to_string();
    inst::user_install_lib_dir(&home)?;
    let xdg = io.env("XDG_DATA_HOME");
    Some(inst::install_paths(InstallMode::User, &home, Some(xdg.as_str()).filter(|x| !x.is_empty())))
}

fn installed_at<IO: SystemIo + ?Sized>(io: &IO, paths: &inst::InstallPaths) -> bool {
    inst::system_or_user_installed(paths, |p| io.exists(p), |p| io.is_dir(p))
}

impl UninstallRequest {
    /// Что установлено на этой машине — только чтение.
    pub fn detect<IO: SystemIo + ?Sized>(
        io: &IO,
        package: Option<DistroPackage>,
        keep_profile: bool,
        lang: Lang,
    ) -> Self {
        let system = installed_at(io, &inst::install_paths(InstallMode::System, "", None));
        let user = user_install_paths(io).is_some_and(|p| installed_at(io, &p));
        UninstallRequest { package, system, user, keep_profile, lang }
    }

    fn nothing_installed(&self) -> bool {
        self.package.is_none() && !self.system && !self.user
    }
}

/// Пользовательские данные Rexon на этой машине: (категория, путь), только
/// существующее. Корень установленного содержимого развёрнут в то, что в нём
/// лежит, кроме резервных копий; корень, который сам ссылка, остаётся одной
/// строкой (удалится сама ссылка), и резервные копии по ту сторону такой
/// ссылки не называются — это уже чужая папка.
fn user_data_entries<IO: SystemIo + ?Sized>(io: &IO) -> Vec<(inst::UserDataKind, String)> {
    use inst::UserDataKind as K;
    let roots = inst::user_data_roots(
        &io.env("HOME"),
        &io.env("XDG_CONFIG_HOME"),
        &io.env("XDG_DATA_HOME"),
        &io.env("XDG_CACHE_HOME"),
    );
    let linked_data: Vec<String> = roots
        .iter()
        .filter(|(k, p)| *k == K::Content && is_redirected(io, p))
        .map(|(_, p)| format!("{p}/"))
        .collect();
    let mut out = Vec::new();
    for (kind, path) in roots {
        if !io.exists(&path) {
            continue;
        }
        match kind {
            K::Content if !is_redirected(io, &path) => {
                let mut names = io.list_dir(&path).unwrap_or_default();
                names.sort();
                out.extend(
                    names
                        .into_iter()
                        .filter(|n| n != inst::USER_DATA_BACKUPS_DIR)
                        .map(|n| (kind, format!("{path}/{n}"))),
                );
            }
            K::Backups if linked_data.iter().any(|root| path.starts_with(root)) => {}
            _ => out.push((kind, path)),
        }
    }
    out
}

/// Перечень для окна вопроса «Удалить Rexon».
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UninstallListing {
    /// Файлы программы (и пакет дистрибутива) — удаляются при любом выборе.
    pub program: Vec<String>,
    /// Данные человека по категориям; что из них удалится, решает выбор
    /// ([`inst::UserDataKind::removed_when_keeping`]).
    pub user_data: Vec<(inst::UserDataKind, String)>,
}

impl UninstallListing {
    /// Всё, что удалится при этом выборе, — программа, потом данные.
    pub fn removed(&self, keep_profile: bool) -> Vec<String> {
        let data = self
            .user_data
            .iter()
            .filter(|(k, _)| !keep_profile || k.removed_when_keeping())
            .map(|(_, p)| p.clone());
        self.program.iter().cloned().chain(data).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.program.is_empty() && self.user_data.is_empty()
    }
}

/// Всё, что удалит [`uninstall`], — для окна вопроса. Только чтение.
/// Ссылки на месте программы, настроек и резервных копий не называются:
/// удалены они не будут. Ссылка на месте временных файлов или установленного
/// содержимого называется: удалится сама ссылка. Не установлено ничего —
/// перечень пуст (удаление тогда ничего не трогает).
pub fn uninstall_files<IO: SystemIo + ?Sized>(io: &IO, req: &UninstallRequest) -> UninstallListing {
    if req.nothing_installed() {
        return UninstallListing::default();
    }
    let sys = inst::install_paths(InstallMode::System, "", None);
    let usr = user_install_paths(io).filter(|_| req.user);
    // Настройки и копии идут своими категориями, а не путями программы.
    let program = inst::get_installed_files(
        req.package.as_ref().map(|p| p.name.as_str()),
        req.system.then_some(&sys),
        usr.as_ref(),
        "",
        "",
        |p| !p.is_empty() && io.exists(p) && !is_redirected(io, p),
    );
    let user_data = user_data_entries(io)
        .into_iter()
        .filter(|(kind, p)| {
            !matches!(kind, inst::UserDataKind::Settings | inst::UserDataKind::Backups)
                || !is_redirected(io, p)
        })
        .collect();
    UninstallListing { program, user_data }
}

/// Чем кончилось удаление.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UninstallOutcome {
    /// Удалено; `kept_links` — ссылки на чужие папки, которые не трогали.
    Removed { kept_links: Vec<String> },
    /// Удалять нечего — ничего не тронуто.
    NotInstalled,
    /// Отменено до запроса пароля — ничего не тронуто.
    Cancelled,
    /// Пакет, удалять который Rexon не умеет, — ничего не тронуто.
    PackageUnsupported { name: String },
    Failed(InstallFailure),
}

/// Исход и строка итога для человека ([`inst::uninstall_done_line`] и
/// соседи).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallResult {
    pub outcome: UninstallOutcome,
    pub message: String,
}

impl UninstallResult {
    pub fn ok(&self) -> bool {
        matches!(self.outcome, UninstallOutcome::Removed { .. })
    }
}

/// План удаления годен исполнителю: запись скрипта, запуск удаления под
/// паролем, уборка. Чужой шаг — отказ, а не тихое «сделано».
fn check_uninstall_plan(steps: &[PlanStep]) -> bool {
    steps.iter().all(|s| {
        matches!(
            s,
            PlanStep::WriteFile { .. }
                | PlanStep::RemoveFiles { .. }
                | PlanStep::Run { id: RunStepId::AppUninstall, .. }
        )
    })
}

/// (удалять, ссылки на чужие папки). Ссылку не удаляем и не проходим:
/// починка 16.
fn split_links<IO: SystemIo + ?Sized>(io: &IO, paths: Vec<String>) -> (Vec<String>, Vec<String>) {
    paths.into_iter().partition(|p| !is_redirected(io, p))
}

/// Удалить путь: каталог целиком или файл (висячую ссылку — как файл).
fn remove_path<IO: InstallIo + ?Sized>(io: &IO, path: &str) {
    if io.is_dir(path) {
        io.remove_tree(path)
    } else {
        io.remove_file(path)
    }
}

/// Сбой удаления. Шаг у отказа удаления человеку не называется
/// ([`inst::uninstall_failure_line`]), поле нужно только типу.
fn removal_failed(detail: impl Into<String>) -> UninstallOutcome {
    UninstallOutcome::Failed(InstallFailure::StepFailed {
        step: InstallStep::SystemInstall,
        detail: detail.into(),
    })
}

/// Удалить Rexon — вход окна меню «Удалить Rexon».
///
/// Порядок: отказы до побочных действий (нечего удалять, пакет без
/// известного способа удаления, отмена) → пакет средствами его менеджера →
/// установка для всех одним корневым скриптом под паролем → установка для
/// себя без пароля → временные файлы (при любом выборе) → настройки,
/// резервные копии и установленные обои, темы и дополнения (при «сохранить»
/// ничто из этого не трогается).
/// Сбой останавливает удаление: профиль удаляется последним и только после
/// удачи всего остального. Отменить можно до первого запроса пароля.
pub fn uninstall<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &UninstallRequest,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
) -> UninstallResult {
    let lang = req.lang;
    let sink = sink.as_ref();
    let mut unlinked: Vec<String> = Vec::new();
    let outcome = if req.nothing_installed() {
        UninstallOutcome::NotInstalled
    } else {
        sink.cancellable(true);
        let res = run_uninstall(io, req, sink, cancel);
        sink.cancellable(false);
        match res {
            Ok(removal) => {
                unlinked = removal.unlinked;
                UninstallOutcome::Removed { kept_links: removal.kept }
            }
            Err(outcome) => outcome,
        }
    };
    let (stage, message) = match &outcome {
        UninstallOutcome::Removed { kept_links } => {
            let mut message = inst::uninstall_done_line(req.keep_profile, lang);
            for link in kept_links {
                message.push_str("\n\n");
                message.push_str(&inst::uninstall_kept_link_note(link, lang));
            }
            for link in &unlinked {
                message.push_str("\n\n");
                message.push_str(&inst::uninstall_removed_link_note(link, lang));
            }
            (STAGE_DONE, message)
        }
        UninstallOutcome::NotInstalled => (de_progress::STAGE_WARNING, inst::uninstall_nothing_line(lang)),
        UninstallOutcome::Cancelled => (
            crate::system_de_exec::STATUS_CANCELLED,
            crate::system_de_exec::cancelled_message(),
        ),
        UninstallOutcome::PackageUnsupported { name } => {
            (de_progress::STAGE_ERROR, inst::uninstall_package_unsupported_line(name, lang))
        }
        UninstallOutcome::Failed(f) => (de_progress::STAGE_ERROR, inst::uninstall_failure_line(f, lang)),
    };
    let fraction = matches!(outcome, UninstallOutcome::Removed { .. }).then_some(1.0);
    sink.progress(stage, &message, fraction);
    UninstallResult { outcome, message }
}

fn run_uninstall<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &UninstallRequest,
    sink: &dyn DeOpSink,
    cancel: &CancelToken,
) -> Result<Removal, UninstallOutcome> {
    let lang = req.lang;
    let say = |part: inst::UninstallPart, fraction: f64| {
        sink.progress(STAGE_UNINSTALL, &part.progress_line(lang), Some(fraction));
    };
    // ── Отказы до первого побочного действия ────────────────────────
    let package_cmd = match &req.package {
        Some(pkg) => match inst::distro_pkg_remove(pkg.family) {
            Some(remove) => Some(inst::pkg_remove_cmd(&remove, &pkg.name)),
            None => return Err(UninstallOutcome::PackageUnsupported { name: pkg.name.clone() }),
        },
        None => None,
    };
    if cancel.is_cancelled() {
        return Err(UninstallOutcome::Cancelled);
    }
    let mut kept = Vec::new();

    // ── 1. Пакет дистрибутива — только средствами его менеджера ─────
    if let Some(cmd) = package_cmd {
        sink.cancellable(false);
        say(inst::UninstallPart::Package, 0.1);
        let out = io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
        classify_script_run(&out, cmd.timeout_secs.unwrap_or(0)).map_err(UninstallOutcome::Failed)?;
    }

    // ── 2. Установка для всех: один корневой скрипт под паролем ─────
    if req.system {
        // Отмена ещё возможна, только если пароль не спрашивали.
        let cancel = req.package.is_none().then_some(cancel);
        uninstall_system(io, lang, sink, cancel, &mut kept)?;
    }
    sink.cancellable(false);

    // ── 3. Установка в домашнюю папку: без пароля ───────────────────
    if let Some(usr) = user_install_paths(io).filter(|_| req.user) {
        say(inst::UninstallPart::User, 0.7);
        let (remove, links) = split_links(io, inst::uninstall_user_paths(&usr, |p| io.exists(p)));
        kept.extend(links);
        for p in &remove {
            remove_path(io, p);
        }
        let dir = |d: &Option<String>| d.clone().unwrap_or_default();
        refresh_caches(io, None, lang, &dir(&usr.desktop_dir), &dir(&usr.icon_root));
    }

    // ── 3½. Сторожа входа и сеанса Plasma — при ЛЮБОМ выборе ─────────
    // Программы больше нет: вернуть откаченное оформление и снять юнит
    // сейчас, иначе возвращать его станет некому.
    release_session_guard(io);

    // ── 4. Временные файлы — при ЛЮБОМ выборе ────────────────────────
    let data = user_data_entries(io);
    let mut unlinked = Vec::new();
    let (always, on_full): (Vec<_>, Vec<_>) = data.into_iter().partition(|(k, _)| k.removed_when_keeping());
    if !always.is_empty() {
        say(inst::UninstallPart::Cache, 0.8);
        for (_, p) in &always {
            remove_own_data(io, p, &mut unlinked);
        }
    }

    // ── 5. Настройки, резервные копии, установленные обои и темы ────
    // Последними и только при «Удалить всё». «Сохранить» оставляет всё
    // это: человек, который переустановит Rexon, продолжит с того же места.
    if !req.keep_profile && !on_full.is_empty() {
        say(inst::UninstallPart::Profile, 0.9);
        for (kind, p) in &on_full {
            match kind {
                // Как прежде (починка 16): ссылку не удаляем и не проходим.
                inst::UserDataKind::Settings | inst::UserDataKind::Backups => {
                    if is_redirected(io, p) {
                        kept.push(p.clone());
                    } else {
                        remove_path(io, p);
                    }
                }
                _ => remove_own_data(io, p, &mut unlinked),
            }
        }
        // Опустевший корень данных — тоже. Непустой (осталась ссылка на
        // резервные копии) не трогается.
        let home = io.env("HOME");
        for (_, root) in inst::user_data_roots(&home, "", &io.env("XDG_DATA_HOME"), "")
            .into_iter()
            .filter(|(k, _)| *k == inst::UserDataKind::Content)
        {
            if io.is_dir(&root) && !is_redirected(io, &root) && io.list_dir(&root).is_some_and(|l| l.is_empty()) {
                io.remove_tree(&root);
            }
        }
    }
    Ok(Removal { kept, unlinked })
}

/// Удаление Rexon во время отката AeroThemePlasma
/// ([`crate::session_guard::release_for_uninstall`]): эффекты KWin —
/// сразу, раскладка — однократным сценарием при следующем входе; юнит
/// сторожа сеанса снимается. Без root; сбой — только в журнал: удалению
/// программы он не помеха.
fn release_session_guard<IO: InstallIo + ?Sized>(io: &IO) {
    let home = io.env("HOME");
    if !home.starts_with('/') {
        return;
    }
    let abs = |v: String| Some(v).filter(|v| v.starts_with('/'));
    let config = crate::utils::config_home(abs(io.env("XDG_CONFIG_HOME")).as_deref(), &home);
    let state = crate::utils::state_home(abs(io.env("XDG_STATE_HOME")).as_deref(), &home);
    let paths = crate::session_guard::GuardPaths::new(&config, std::path::Path::new(&state).join("rexon/session-guard"));
    match crate::session_guard::release_for_uninstall(&paths) {
        Ok(done) => {
            if done.restored.had_saved {
                crate::log_info!("session-guard", "удаление Rexon: возвращено {done:?}");
            }
            if !done.restored.effects.is_empty() && io.which("dbus-send") {
                io.run(&crate::session_guard::reconfigure_kwin_argv(), Some(5), true);
            }
        }
        Err(e) => crate::log_warn!("session-guard", "удаление Rexon: {e}"),
    }
    let unit = crate::session_watch::unit_path(&config);
    if io.exists(&unit) {
        let has_systemctl = io.which("systemctl");
        for args in crate::session_watch::uninstall_systemctl() {
            if args.first().map(String::as_str) == Some("daemon-reload") {
                io.remove_file(&unit);
            }
            if has_systemctl {
                let mut argv = vec!["systemctl".to_string(), "--user".to_string()];
                argv.extend(args);
                io.run(&argv, Some(30), true);
            }
        }
    }
    // Светлое и тёмное по расписанию: таймер, служба и автозапуск без
    // Rexon будили бы пустоту. Настройку Plasma (её же «Автоматически»)
    // не трогаем — это настройка самой Plasma.
    let dn = crate::day_night::Paths { config_home: config, state_home: state };
    let present: Vec<String> = crate::day_night::uninstall_paths(&dn).into_iter().filter(|p| io.exists(p)).collect();
    if !present.is_empty() {
        let has_systemctl = io.which("systemctl");
        let sc = |args: &[&str]| {
            if has_systemctl {
                let mut argv = vec!["systemctl".to_string(), "--user".to_string()];
                argv.extend(args.iter().map(|a| a.to_string()));
                io.run(&argv, Some(30), true);
            }
        };
        sc(&["disable", "--now", crate::day_night::TIMER_UNIT]);
        for p in &present {
            io.remove_file(p);
        }
        sc(&["daemon-reload"]);
    }
}

/// Что осталось и что удалено ссылкой — для итога.
struct Removal {
    /// Ссылки на месте программы, настроек и копий: не тронуты.
    kept: Vec<String>,
    /// Ссылки на месте временных файлов и установленного: удалена сама ссылка.
    unlinked: Vec<String>,
}

/// Временные файлы и установленное содержимое. Ссылка на другую папку
/// удаляется САМА, по ней не ходим: то, на что она указывает, — не наше.
/// Остальное — целиком; ссылки внутри каталога при этом тоже удаляются
/// как ссылки (удаление дерева по ним не ходит).
fn remove_own_data<IO: InstallIo + ?Sized>(io: &IO, path: &str, unlinked: &mut Vec<String>) {
    if is_redirected(io, path) {
        io.remove_file(path);
        unlinked.push(path.to_string());
    } else {
        remove_path(io, path);
    }
}

/// Установка для всех: скрипт во временной папке → пароль → уборка папки на
/// ЛЮБОМ исходе (починка 18). Ссылки из списка в скрипт не попадают.
fn uninstall_system<IO: InstallIo + ?Sized>(
    io: &IO,
    lang: Lang,
    sink: &dyn DeOpSink,
    cancel: Option<&CancelToken>,
    kept: &mut Vec<String>,
) -> Result<(), UninstallOutcome> {
    let sys = inst::install_paths(InstallMode::System, "", None);
    let (paths, links) = split_links(io, inst::uninstall_system_paths(&sys));
    let root = io.temp_dir(UNINSTALL_STAGE_PREFIX, None).map_err(removal_failed)?;
    let res = run_system_script(io, lang, sink, cancel, &sys, &paths, &root);
    io.remove_tree(&root);
    res?;
    kept.extend(links);
    Ok(())
}

fn run_system_script<IO: InstallIo + ?Sized>(
    io: &IO,
    lang: Lang,
    sink: &dyn DeOpSink,
    cancel: Option<&CancelToken>,
    sys: &inst::InstallPaths,
    paths: &[String],
    root: &str,
) -> Result<(), UninstallOutcome> {
    let foreign_step = || removal_failed(tr("the removal plan contains an unknown step"));
    let script = format!("{}/uninstall.sh", root.trim_end_matches('/'));
    let steps = inst::uninstall_system_plan_for(sys, paths, &script);
    if !check_uninstall_plan(&steps) {
        return Err(foreign_step());
    }
    let mut run_out: Option<(RunOut, u64)> = None;
    for step in &steps {
        match step {
            PlanStep::WriteFile { path, content, mode } => {
                io.write_file_mode(path, content, *mode).map_err(removal_failed)?;
            }
            PlanStep::Run { cmd, .. } => {
                if cancel.is_some_and(|c| c.is_cancelled()) {
                    return Err(UninstallOutcome::Cancelled);
                }
                sink.cancellable(false);
                sink.progress(STAGE_UNINSTALL, &inst::UninstallPart::System.progress_line(lang), Some(0.4));
                run_out = Some((io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c), cmd.timeout_secs.unwrap_or(0)));
            }
            // Уборку делает `uninstall_system` — всю папку, на любом исходе.
            PlanStep::RemoveFiles { .. } => {}
            _ => return Err(foreign_step()),
        }
    }
    let (out, secs) = run_out.unwrap_or_else(|| (RunOut::missing(), 0));
    classify_script_run(&out, secs).map_err(UninstallOutcome::Failed)
}

/// Приёмник хода без окна: строки уходят в журнал Rexon (`applog`). Им пользуется
/// окно меню «Удалить Rexon» — окна хода у него нет, итог приходит сигналом.
pub struct LogSink;

impl DeOpSink for LogSink {
    fn progress(&self, stage: &str, message: &str, _fraction: Option<f64>) {
        crate::log_info!("uninstall", "[{stage}] {message}");
    }
    fn cancellable(&self, _allowed: bool) {}
}

// ============================================================================
// Живой исполнитель
// ============================================================================

/// [`InstallIo`] поверх живой машины — то, чем пользуется продукт.
pub struct LiveInstallIo {
    de: LiveDeOpsIo,
}

impl LiveInstallIo {
    pub fn new() -> Self {
        LiveInstallIo { de: LiveDeOpsIo::new() }
    }
}

impl Default for LiveInstallIo {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemIo for LiveInstallIo {
    fn exists(&self, path: &str) -> bool {
        self.de.exists(path)
    }
    fn is_file(&self, path: &str) -> bool {
        self.de.is_file(path)
    }
    fn is_dir(&self, path: &str) -> bool {
        self.de.is_dir(path)
    }
    fn is_executable(&self, path: &str) -> bool {
        self.de.is_executable(path)
    }
    fn realpath(&self, path: &str) -> Option<String> {
        self.de.realpath(path)
    }
    fn read_text(&self, path: &str) -> Option<String> {
        self.de.read_text(path)
    }
    fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
        self.de.list_dir(dir)
    }
    fn which(&self, name: &str) -> bool {
        self.de.which(name)
    }
    fn run(&self, argv: &[String], timeout_secs: Option<u64>, lang_c: bool) -> RunOut {
        self.de.run(argv, timeout_secs, lang_c)
    }
    fn env(&self, name: &str) -> String {
        self.de.env(name)
    }
    fn which_path(&self, name: &str) -> Option<String> {
        self.de.which_path(name)
    }
    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        self.de.write_text(path, content)
    }
    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        self.de.copy_file(src, dst)
    }
}

fn set_mode(path: &std::path::Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn walk_copy(
    src: &std::path::Path,
    dst: &std::path::Path,
    skip: &dyn Fn(&str) -> bool,
    mode: &dyn Fn(bool, bool) -> u32,
    count: &mut usize,
) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    set_mode(dst, mode(true, false))?;
    let entries = std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if skip(&name) {
            continue;
        }
        let path = entry.path();
        let target = dst.join(&name);
        let Ok(link) = std::fs::symlink_metadata(&path) else { continue };
        // Висячий симлинк — мимо (как `ignore_dangling_symlinks`).
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.is_dir() {
            // Симлинк на каталог — мимо: петля или путь вне поставки.
            if link.file_type().is_symlink() {
                continue;
            }
            walk_copy(&path, &target, skip, mode, count)?;
        } else if meta.is_file() {
            std::fs::copy(&path, &target).map_err(|e| format!("{}: {e}", path.display()))?;
            set_mode(&target, mode(false, meta.permissions().mode() & 0o111 != 0))?;
            *count += 1;
        }
    }
    Ok(())
}

impl InstallIo for LiveInstallIo {
    fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String> {
        DeOpsIo::temp_dir(&self.de, prefix, parent)
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), String> {
        std::fs::rename(src, dst).map_err(|e| format!("{src} → {dst}: {e}"))
    }

    fn make_dirs(&self, path: &str, mode: Option<u32>) -> Result<(), String> {
        let target = std::path::Path::new(path);
        let missing: Vec<&std::path::Path> = target
            .ancestors()
            .take_while(|p| !p.as_os_str().is_empty() && !p.exists())
            .collect();
        std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
        if let Some(mode) = mode {
            for dir in missing.iter().rev() {
                set_mode(dir, mode)?;
            }
        }
        Ok(())
    }

    fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
        DeOpsIo::write_file_mode(&self.de, path, content, mode)
    }

    fn copy_file_mode(&self, src: &str, dst: &str, mode: u32) -> Result<(), String> {
        std::fs::copy(src, dst).map_err(|e| format!("{src}: {e}"))?;
        set_mode(std::path::Path::new(dst), mode)
    }

    fn copy_tree(
        &self,
        src: &str,
        dst: &str,
        skip: &dyn Fn(&str) -> bool,
        mode: &dyn Fn(bool, bool) -> u32,
    ) -> Result<usize, String> {
        let mut count = 0;
        walk_copy(std::path::Path::new(src), std::path::Path::new(dst), skip, mode, &mut count)?;
        Ok(count)
    }

    fn remove_file(&self, path: &str) {
        DeOpsIo::remove_file(&self.de, path)
    }

    fn remove_tree(&self, path: &str) {
        DeOpsIo::remove_tree(&self.de, path)
    }
}

// ============================================================================
// Тесты — ТОЛЬКО подменный исполнитель
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;

    #[derive(Debug, Clone, PartialEq)]
    enum Call {
        TempDir,
        MakeDirs(String, Option<u32>),
        Write(String, String, u32),
        CopyFile(String, String, u32),
        CopyTree(String, String),
        TreeItem(String, u32),
        Run(Vec<String>, Option<u64>),
        Rename(String, String),
        RemoveFile(String),
        RemoveTree(String),
    }

    const STAGE: &str = "/tmp/rexon_install_T";
    const RU: Lang = Lang::Ru;

    type Reply = Box<dyn Fn(&[String]) -> RunOut>;

    struct FakeIo {
        calls: RefCell<Vec<Call>>,
        files: HashSet<String>,
        dirs: HashSet<String>,
        /// Содержимое деревьев: (относительный путь, каталог, исполняемый).
        trees: HashMap<String, Vec<(String, bool, bool)>>,
        env: HashMap<String, String>,
        reply: Reply,
        /// Скрипт «положил» файлы — цель видна после запуска.
        lands: bool,
        landed: Cell<bool>,
        fail_temp_dir: bool,
        fail_copy_tree: bool,
        /// Ссылки: путь → куда ведёт (`realpath`).
        links: HashMap<String, String>,
        /// Программы, которые «есть» (`which`).
        tools: HashSet<String>,
    }

    fn out(code: i32, stdout: &str, stderr: &str) -> RunOut {
        RunOut {
            outcome: if code == 0 { RunOutcome::Ok } else { RunOutcome::Failed },
            returncode: Some(code),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    fn marks(ids: &[&str]) -> String {
        ids.iter().map(|s| format!("{STEP_MARK}{s}\n")).collect()
    }

    fn all_marks() -> String {
        marks(&["deps", "files", "launcher", "menu", "icons", "helpers", "caches", "extensions"])
    }

    impl FakeIo {
        fn new(pkexec: impl Fn() -> RunOut + 'static) -> Self {
            let files: HashSet<String> = [
                "/src/rexon-main-window",
                "/src/rexon-theme-shot",
                "/src/bin/rexon-wallpaper-host",
                "/src/bin/rexon-splash-overlay",
                "/src/bin/rexon-priv-helper",
                "/src/qml_modules/waywallen/control/librexon_qt_filter.so",
                "/src/rexon_icon_16.png",
                "/src/rexon_icon_128.png",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            let mut trees = HashMap::new();
            trees.insert(
                "/src/rexon".to_string(),
                vec![
                    ("app.py".to_string(), false, false),
                    ("themes.json".to_string(), false, false),
                    ("tools".to_string(), true, false),
                    ("tools/run.sh".to_string(), false, true),
                    ("__pycache__".to_string(), true, false),
                    ("__pycache__/app.cpython.pyc".to_string(), false, false),
                    (".hidden.json".to_string(), false, false),
                    ("gnome-extension".to_string(), true, false),
                ],
            );
            trees.insert(
                "/src/qml_modules".to_string(),
                vec![("Qcm/Material/libqml_materialplugin.so".to_string(), false, true)],
            );
            let env: HashMap<String, String> = [
                ("HOME".to_string(), "/scratch/home".to_string()),
                ("PATH".to_string(), "/usr/local/bin:/usr/bin".to_string()),
            ]
            .into_iter()
            .collect();
            FakeIo {
                calls: RefCell::new(Vec::new()),
                files,
                dirs: ["/src/qml_modules".to_string()].into_iter().collect(),
                trees,
                env,
                reply: Box::new(move |argv: &[String]| {
                    if argv.first().map(String::as_str) == Some("pkexec") {
                        pkexec()
                    } else {
                        out(0, "", "")
                    }
                }),
                lands: true,
                landed: Cell::new(false),
                fail_temp_dir: false,
                fail_copy_tree: false,
                links: HashMap::new(),
                tools: HashSet::new(),
            }
        }

        fn ok() -> Self {
            Self::new(|| out(0, &all_marks(), ""))
        }

        fn log(&self, c: Call) {
            self.calls.borrow_mut().push(c);
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.borrow().clone()
        }

        fn runs(&self) -> Vec<(Vec<String>, Option<u64>)> {
            self.calls()
                .into_iter()
                .filter_map(|c| match c {
                    Call::Run(a, t) if a[0] == "pkexec" => Some((a, t)),
                    _ => None,
                })
                .collect()
        }

        fn written(&self, path: &str) -> Option<(String, u32)> {
            self.calls().into_iter().find_map(|c| match c {
                Call::Write(p, text, mode) if p == path => Some((text, mode)),
                _ => None,
            })
        }

        fn target_visible(&self, path: &str) -> bool {
            self.landed.get()
                && (path.starts_with("/usr/local/") || path.starts_with("/usr/share/applications/"))
        }
    }

    impl SystemIo for FakeIo {
        fn exists(&self, path: &str) -> bool {
            self.files.contains(path) || self.dirs.contains(path) || self.target_visible(path)
        }
        fn is_file(&self, path: &str) -> bool {
            self.files.contains(path)
        }
        fn is_dir(&self, path: &str) -> bool {
            self.dirs.contains(path) || self.target_visible(path)
        }
        fn is_executable(&self, path: &str) -> bool {
            self.target_visible(path)
        }
        fn realpath(&self, path: &str) -> Option<String> {
            Some(self.links.get(path).cloned().unwrap_or_else(|| path.to_string()))
        }
        fn read_text(&self, _path: &str) -> Option<String> {
            None
        }
        fn list_dir(&self, _dir: &str) -> Option<Vec<String>> {
            None
        }
        fn which(&self, name: &str) -> bool {
            self.tools.contains(name)
        }
        fn run(&self, argv: &[String], timeout_secs: Option<u64>, _lang_c: bool) -> RunOut {
            self.log(Call::Run(argv.to_vec(), timeout_secs));
            let r = (self.reply)(argv);
            if argv[0] == "pkexec" && r.returncode == Some(0) && self.lands {
                self.landed.set(true);
            }
            r
        }
        fn env(&self, name: &str) -> String {
            self.env.get(name).cloned().unwrap_or_default()
        }
        fn which_path(&self, _name: &str) -> Option<String> {
            None
        }
        fn write_text(&self, path: &str, _content: &str) -> Result<(), String> {
            panic!("write_text без прав исполнитель установки не зовёт: {path}")
        }
        fn copy_file(&self, src: &str, _dst: &str) -> Result<(), String> {
            panic!("copy_file без прав исполнитель установки не зовёт: {src}")
        }
    }

    impl InstallIo for FakeIo {
        fn temp_dir(&self, _prefix: &str, parent: Option<&str>) -> Result<String, String> {
            self.log(Call::TempDir);
            if self.fail_temp_dir {
                Err("нет места".into())
            } else {
                // Каталог внутри `parent`, как у живого; без него — прежний.
                Ok(parent.map_or_else(|| STAGE.to_string(), |p| format!("{p}/{}", basename(STAGE))))
            }
        }
        fn rename(&self, src: &str, dst: &str) -> Result<(), String> {
            self.log(Call::Rename(src.to_string(), dst.to_string()));
            Ok(())
        }
        fn make_dirs(&self, path: &str, mode: Option<u32>) -> Result<(), String> {
            self.log(Call::MakeDirs(path.to_string(), mode));
            Ok(())
        }
        fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
            self.log(Call::Write(path.to_string(), content.to_string(), mode));
            Ok(())
        }
        fn copy_file_mode(&self, src: &str, dst: &str, mode: u32) -> Result<(), String> {
            self.log(Call::CopyFile(src.to_string(), dst.to_string(), mode));
            Ok(())
        }
        fn copy_tree(
            &self,
            src: &str,
            dst: &str,
            skip: &dyn Fn(&str) -> bool,
            mode: &dyn Fn(bool, bool) -> u32,
        ) -> Result<usize, String> {
            self.log(Call::CopyTree(src.to_string(), dst.to_string()));
            if self.fail_copy_tree {
                return Err("диск переполнен".into());
            }
            let mut n = 0;
            for (rel, is_dir, exec) in self.trees.get(src).cloned().unwrap_or_default() {
                if rel.split('/').any(skip) {
                    continue;
                }
                self.log(Call::TreeItem(format!("{dst}/{rel}"), mode(is_dir, exec)));
                n += usize::from(!is_dir);
            }
            Ok(n)
        }
        fn remove_file(&self, path: &str) {
            self.log(Call::RemoveFile(path.to_string()));
        }
        fn remove_tree(&self, path: &str) {
            self.log(Call::RemoveTree(path.to_string()));
        }
    }

    #[derive(Default)]
    struct RecSink {
        lines: Mutex<Vec<(String, String)>>,
        cancellable: Mutex<Vec<bool>>,
    }

    impl RecSink {
        fn has(&self, message: &str) -> bool {
            self.lines.lock().unwrap().iter().any(|(_, m)| m == message)
        }
    }

    impl DeOpSink for RecSink {
        fn progress(&self, stage: &str, message: &str, _fraction: Option<f64>) {
            self.lines.lock().unwrap().push((stage.to_string(), message.to_string()));
        }
        fn cancellable(&self, allowed: bool) {
            self.cancellable.lock().unwrap().push(allowed);
        }
    }

    fn payload() -> PayloadSource {
        PayloadSource {
            package_dir: "/src/rexon".into(),
            icon_dir: "/src".into(),
            main_window: Some("/src/rexon-main-window".into()),
            wallpaper_host: Some("/src/bin/rexon-wallpaper-host".into()),
            splash_overlay: Some("/src/bin/rexon-splash-overlay".into()),
            qml_modules: Some("/src/qml_modules".into()),
            filter_lib: Some("/src/qml_modules/waywallen/control/librexon_qt_filter.so".into()),
            priv_helper: Some("/src/bin/rexon-priv-helper".into()),
        }
    }

    fn request() -> InstallRequest {
        InstallRequest::new(payload(), PkgFamily::Arch, "cachyos", RU)
    }

    fn install(io: &FakeIo, req: &InstallRequest) -> (InstallResult, Arc<RecSink>) {
        let sink = Arc::new(RecSink::default());
        let r = install_to_system(io, req, sink.clone(), &CancelToken::new());
        (r, sink)
    }

    fn script_of(io: &FakeIo) -> String {
        io.written(&StagePaths::new(STAGE).script()).expect("скрипт записан").0
    }

    /// Уборка: временный каталог снят ПОСЛЕ всех побочных действий в нём.
    fn cleaned_last(io: &FakeIo) {
        let calls = io.calls();
        let remove = calls.iter().rposition(|c| *c == Call::RemoveTree(STAGE.into()));
        let last_side = calls.iter().rposition(|c| match c {
            Call::Write(p, ..) | Call::CopyFile(_, p, _) | Call::CopyTree(_, p) | Call::MakeDirs(p, _) => {
                p.starts_with(STAGE)
            }
            Call::Run(a, _) => a[0] == "pkexec",
            _ => false,
        });
        assert!(remove.is_some(), "временный каталог убран: {calls:#?}");
        assert!(remove > last_side, "уборка — после всех действий во временном каталоге");
    }

    fn failure(r: &InstallResult) -> InstallFailure {
        r.failure().cloned().unwrap_or_else(|| panic!("ждали неудачу: {r:?}"))
    }

    // ── Успех ───────────────────────────────────────────────────────

    #[test]
    fn success_runs_script_once_under_password_and_cleans_up() {
        let io = FakeIo::ok();
        let req = request();
        let (r, sink) = install(&io, &req);
        assert!(r.ok(), "{r:?}");
        assert_eq!(r.message, inst::install_done_line(InstallMode::System, RU));
        assert_eq!(
            r.outcome,
            Some(WizardOutcome::Installed {
                mode: InstallMode::System,
                fell_back: None,
                command_dir_not_in_path: None,
            })
        );

        let script = StagePaths::new(STAGE).script();
        assert_eq!(
            io.runs(),
            vec![(vec!["pkexec".to_string(), "bash".into(), script.clone()], Some(600))],
            "один запуск под паролем; починка 3: таймаут 600 с"
        );

        // Записанный скрипт — ровно текст ядра для этих параметров.
        let params = system_script_params(&io, &req, &StagePaths::new(STAGE));
        let (text, mode) = io.written(&script).unwrap();
        assert_eq!(text, install_script(&params));
        assert_eq!(mode, 0o700);

        // Всё, что пишется и копируется, — во временном каталоге или в
        // подставном HOME: ни /usr, ни /etc исполнитель сам не трогает.
        for c in io.calls() {
            let dst = match &c {
                Call::Write(p, ..) | Call::CopyFile(_, p, _) | Call::CopyTree(_, p) | Call::MakeDirs(p, _) => p,
                _ => continue,
            };
            assert!(
                dst.starts_with(STAGE) || dst.starts_with("/scratch/home/"),
                "запись вне временного каталога: {c:?}"
            );
        }
        cleaned_last(&io);

        assert!(sink.has(&format!("✅ {}", InstallStep::CopyFiles.progress_line(RU).trim_end_matches('…'))));
        let lines = sink.lines.lock().unwrap().clone();
        assert_eq!(lines.last().unwrap().0, STAGE_DONE);
        assert_eq!(sink.cancellable.lock().unwrap().first(), Some(&true));
        assert_eq!(sink.cancellable.lock().unwrap().last(), Some(&false));

        let calls = io.calls();
        assert!(calls.contains(&Call::MakeDirs("/scratch/home/.config/rexon".into(), None)));
        assert!(calls.contains(&Call::MakeDirs("/scratch/home/.local/share/rexon/backups".into(), None)));
    }

    #[test]
    fn command_dir_outside_path_is_reported() {
        let mut io = FakeIo::ok();
        io.env.insert("PATH".into(), "/usr/bin".into());
        let (r, _) = install(&io, &request());
        assert!(matches!(
            r.outcome,
            Some(WizardOutcome::Installed { command_dir_not_in_path: Some(ref d), .. }) if d == "/usr/local/bin"
        ));
    }

    // ── Отказ пароля и сбой шага (починка 1) ────────────────────────

    #[test]
    fn closed_password_window_is_password_denied_not_a_failed_install() {
        let io = FakeIo::new(|| out(126, "", "Error executing command as another user: Request dismissed"));
        let (r, _) = install(&io, &request());
        assert_eq!(failure(&r), InstallFailure::PasswordDenied);
        assert_eq!(r.message, InstallFailure::PasswordDenied.log_line(RU));
        assert!(!r.message.contains("pkexec") && !r.message.contains("отменена пользователем"));
        cleaned_last(&io);
    }

    #[test]
    fn denied_password_without_started_script_is_password_denied() {
        let io = FakeIo::new(|| out(127, "", "Error executing command as another user: Not authorized"));
        let (r, _) = install(&io, &request());
        assert_eq!(failure(&r), InstallFailure::PasswordDenied);
        cleaned_last(&io);
    }

    #[test]
    fn failed_dependency_step_is_a_step_failure_not_a_cancel() {
        let io = FakeIo::new(|| {
            out(
                1,
                &marks(&["deps"]),
                &format!("error: target not found: layer-shell-qt\n{FAIL_MARK}deps\n"),
            )
        });
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let (r, sink) = install(&io, &request());
        let f = failure(&r);
        let InstallFailure::StepFailed { step, detail } = &f else { panic!("{f:?}") };
        assert_eq!(*step, InstallStep::Dependencies);
        assert!(
            detail.contains("exit code 1") && detail.contains("target not found: layer-shell-qt"),
            "{detail}"
        );
        assert!(!detail.contains(FAIL_MARK) && !detail.contains(STEP_MARK));
        // ⭐ 16.09.2026: отказ на этом шаге НАЗЫВАЕТ человеку, чего не
        // хватило, и говорит, что система не тронута, — общей фразы «не
        // получилось: установка недостающих компонентов» здесь больше нет.
        assert!(r.message.contains(&inst::dependencies_failed_problem(RU)), "{}", r.message);
        // Слова — английские (исходный язык каталога); русский слово в слово
        // сверяет `tests/installer_parity.rs`.
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let message = inst::dependencies_failed_problem(RU);
        for word in ["Qt6", "wallpaper layer", "video player", "Vulkan loader"] {
            assert!(message.contains(word), "не названо «{word}»: {message}");
        }
        assert!(message.contains("Nothing in the system was changed"), "{message}");
        // Сырое имя пакета — только в подробностях для разработчика.
        assert!(!r.message.contains("layer-shell-qt"), "{}", r.message);
        assert!(!sink.has(&format!("✅ {}", InstallStep::Dependencies.progress_line(RU).trim_end_matches('…'))));
        cleaned_last(&io);
    }

    #[test]
    fn code_127_from_inside_started_script_is_a_step_failure() {
        // «команда не найдена» внутри скрипта тоже даёт 127 — но скрипт шёл.
        let io = FakeIo::new(|| out(127, &marks(&["deps", "files"]), &format!("cp: not found\n{FAIL_MARK}files\n")));
        let (r, sink) = install(&io, &request());
        assert!(matches!(failure(&r), InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. }));
        assert!(sink.has(&format!("✅ {}", InstallStep::Dependencies.progress_line(RU).trim_end_matches('…'))));
        cleaned_last(&io);
    }

    #[test]
    fn timeout_and_missing_password_prompt_are_named_and_cleaned() {
        let io = FakeIo::new(|| RunOut {
            outcome: RunOutcome::Timeout,
            returncode: None,
            stdout: String::new(),
            stderr: String::new(),
        });
        let (r, _) = install(&io, &request());
        assert_eq!(failure(&r), InstallFailure::TimedOut { secs: 600 });
        cleaned_last(&io);

        let io = FakeIo::new(RunOut::missing);
        let (r, _) = install(&io, &request());
        assert_eq!(failure(&r), InstallFailure::PasswordPromptMissing);
        cleaned_last(&io);
    }

    #[test]
    fn script_ok_but_nothing_in_place_is_not_success() {
        let mut io = FakeIo::ok();
        io.lands = false;
        let (r, _) = install(&io, &request());
        assert!(matches!(failure(&r), InstallFailure::StepFailed { step: InstallStep::SystemInstall, .. }));
        cleaned_last(&io);
    }

    #[test]
    fn classify_by_fact_not_by_code_alone() {
        let c = |o: RunOut| classify_script_run(&o, 600);
        assert_eq!(c(out(0, &all_marks(), "")), Ok(()));
        assert_eq!(c(out(126, "", "")), Err(InstallFailure::PasswordDenied));
        assert_eq!(c(out(127, "", "")), Err(InstallFailure::PasswordDenied));
        assert_eq!(c(out(1, "", "Error executing command as another user: Not authorized")), Err(InstallFailure::PasswordDenied));
        assert!(matches!(
            c(out(3, "", "boom")),
            Err(InstallFailure::StepFailed { step: InstallStep::SystemInstall, .. })
        ));
        assert!(matches!(
            c(out(126, &marks(&["deps", "files"]), "")),
            Err(InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. })
        ));
    }

    // ── Уборка при ошибке подготовки и отмене (починка 2) ───────────

    #[test]
    fn prepare_failure_cleans_up_and_never_asks_password() {
        let mut io = FakeIo::ok();
        io.fail_copy_tree = true;
        let (r, _) = install(&io, &request());
        assert_eq!(
            failure(&r),
            InstallFailure::StepFailed { step: InstallStep::CopyFiles, detail: "диск переполнен".into() }
        );
        assert!(io.runs().is_empty(), "пароль не спрашивали");
        cleaned_last(&io);
    }

    #[test]
    fn temp_dir_failure_touches_nothing_else() {
        let mut io = FakeIo::ok();
        io.fail_temp_dir = true;
        let (r, _) = install(&io, &request());
        assert!(matches!(failure(&r), InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. }));
        assert_eq!(io.calls(), vec![Call::TempDir]);
    }

    #[test]
    fn cancel_before_password_cleans_up_and_system_untouched() {
        let io = FakeIo::ok();
        let cancel = CancelToken::new();
        cancel.cancel();
        let r = install_to_system(&io, &request(), Arc::new(RecSink::default()), &cancel);
        assert!(r.cancelled() && !r.ok());
        assert!(io.runs().is_empty());
        cleaned_last(&io);
    }

    #[test]
    fn immutable_system_refuses_before_any_side_effect() {
        let io = FakeIo::ok();
        let mut req = request();
        req.distro_id = "steamos".into();
        let (r, _) = install(&io, &req);
        assert!(!r.ok());
        assert!(io.calls().is_empty(), "{:?}", io.calls());
    }

    // ── Права файлов (починка 4) ────────────────────────────────────

    #[test]
    fn modes_dirs_755_files_644_executables_755() {
        assert_eq!(payload_mode(true, false), 0o755);
        assert_eq!(payload_mode(false, false), 0o644);
        assert_eq!(payload_mode(false, true), 0o755);

        let io = FakeIo::ok();
        install(&io, &request());
        let calls = io.calls();
        let pkg = format!("{STAGE}/lib/rexon");
        let item = |rel: &str| {
            calls.iter().find_map(|c| match c {
                Call::TreeItem(p, m) if *p == format!("{pkg}/{rel}") => Some(*m),
                _ => None,
            })
        };
        assert_eq!(item("themes.json"), Some(0o644));
        assert_eq!(item("app.py"), None, "исходники Python не ставятся");
        assert_eq!(item(".hidden.json"), Some(0o644), "скрытые файлы не теряются");
        assert_eq!(item("tools"), Some(0o755));
        assert_eq!(item("tools/run.sh"), Some(0o755));
        assert_eq!(item("__pycache__"), None, "кеши байткода не ставятся");
        assert_eq!(item("__pycache__/app.cpython.pyc"), None);

        let copied = |dst: &str| {
            calls.iter().find_map(|c| match c {
                Call::CopyFile(_, d, m) if d == dst => Some(*m),
                _ => None,
            })
        };
        let st = StagePaths::new(STAGE);
        assert_eq!(copied(&st.layout().main_window()), Some(0o755));
        assert_eq!(copied(&st.layout().theme_shot()), Some(0o755), "помощник снимков — рядом с окном");
        assert_eq!(copied(&st.layout().filter_lib()), Some(0o755));
        assert_eq!(copied(&st.layout().wallpaper_host()), Some(0o755));
        assert_eq!(copied(&format!("{}/rexon_icon_16.png", st.icons_dir())), Some(0o644));
        assert_eq!(io.written(&st.desktop()).unwrap().1, 0o644);
        assert_eq!(io.written(&st.layout().launcher()).unwrap().1, 0o755);
        let h = st.helpers();
        assert_eq!(io.written(&h.tmp_pkg_helper).unwrap().1, 0o755);
        assert_eq!(io.written(&h.tmp_pkg_policy).unwrap().1, 0o644);
        for c in &calls {
            if let Call::MakeDirs(p, m) = c {
                if p.starts_with(STAGE) {
                    assert_eq!(*m, Some(0o755), "каталог поставки {p}");
                }
            }
        }

        let script = script_of(&io);
        assert!(!script.contains("chmod -R 755"));
        assert!(script.contains(&format!("cp -r --preserve=mode {STAGE}/lib/. /usr/local/lib/rexon/\n")));
        assert!(script.contains("chown -R root:root /usr/local/lib/rexon\n"));
    }

    // ── Rust-окно и откат на Python (починка 5) ─────────────────────

    #[test]
    fn installs_rust_window_and_menu_entry_points_to_it() {
        let io = FakeIo::ok();
        let (r, _) = install(&io, &request());
        let target = InstallLayout::system();
        assert_eq!(r.entry, Some(LaunchEntry::MainWindow(target.main_window())));
        let desktop = io.written(&StagePaths::new(STAGE).desktop()).unwrap().0;
        assert!(desktop.contains(&format!("Exec={}\n", target.main_window())), "{desktop}");
        // 16.09.2026: пункта «классическое окно» в ярлыке больше нет —
        // ставить его нечем, команда `rexon` ведёт на то же Rust-окно.
        assert!(!desktop.contains("Classic"), "{desktop}");

        let script = script_of(&io);
        for dst in [target.launcher(), target.wallpaper_host(), target.splash_overlay()] {
            assert!(script.contains(&format!(" {dst}\n")), "{dst} ставится");
        }
    }

    /// ⭐ 16.09.2026: поставка без бинаря окна — отказ ДО первого побочного
    /// действия. Прежде такая установка проходила «успешно» и уводила ярлык
    /// на классическое окно; питона в поставке больше нет, а команда `rexon`
    /// сама ведёт на бинарь окна — установка положила бы в систему лаунчер,
    /// указывающий в пустоту.
    #[test]
    fn without_the_window_binary_the_install_refuses_and_touches_nothing() {
        for mode in [InstallMode::System, InstallMode::User] {
            let io = FakeIo::ok();
            let mut req = request();
            req.payload.main_window = None;
            let sink = Arc::new(RecSink::default());
            let r = super::install(&io, mode, &req, sink, &CancelToken::new());
            assert!(!r.ok() && !r.cancelled(), "{mode:?}: {r:?}");
            assert!(
                matches!(r.failure(), Some(InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. })),
                "{mode:?}: {r:?}"
            );
            assert_eq!(r.entry, None);
            assert!(io.calls().is_empty(), "{mode:?}: система не тронута");
        }

        // Переносному режиму бинарь не нужен — ставить он ничего и не пытается.
        let io = FakeIo::ok();
        let mut req = request();
        req.payload.main_window = None;
        let sink = Arc::new(RecSink::default());
        let r = super::install(&io, InstallMode::Portable, &req, sink, &CancelToken::new());
        assert!(r.ok(), "{r:?}");
    }

    /// Лаунчер `rexon` ручной установки ведёт на бинарь окна — как у пакета
    /// дистрибутива, но БЕЗ признака пакета: иначе выбор режима счёл бы
    /// установку переносной и мастер больше не открылся бы.
    #[test]
    fn the_command_leads_to_the_window_binary_without_the_package_mark() {
        let io = FakeIo::ok();
        let (_, _) = install(&io, &request());
        let target = InstallLayout::system();
        let text = io.written(&StagePaths::new(STAGE).layout().launcher()).unwrap().0;
        assert_eq!(text, inst::system_launcher(&target.lib_dir));
        assert!(text.starts_with("#!/bin/sh\n"), "{text}");
        assert!(text.contains(&format!("exec {} \"$@\"\n", target.main_window())), "{text}");
        assert!(!text.contains(inst::PACKAGED_ENV), "{text}");
        assert!(!text.contains("python"), "{text}");

        // Переменная отката ярлык уводит, а лаунчер — нет: вести ему больше
        // некуда, классического окна в поставке нет.
        let io = FakeIo::ok();
        let mut req = request();
        req.entry_env = Some("python".into());
        let (r, _) = install(&io, &req);
        assert_eq!(r.entry, Some(LaunchEntry::Python));
        assert_eq!(
            io.written(&StagePaths::new(STAGE).layout().launcher()).unwrap().0,
            inst::system_launcher(&target.lib_dir)
        );
    }

    // ── Раскладка одна (починка 6) ──────────────────────────────────

    #[test]
    fn layout_is_one_for_package_and_manual_install() {
        let pkg = InstallLayout::package();
        assert_eq!(pkg.main_window(), inst::PACKAGE_MAIN_WINDOW);
        assert_eq!(pkg.qml_modules(), qm::SYSTEM_MODULE_DIRS[0]);
        assert!(inst::package_data_dir_candidates("", "/h").contains(&pkg.package_dir()));

        let sys = InstallLayout::system();
        assert_eq!(Some(sys.clone()), InstallLayout::for_mode(InstallMode::System, "/h", None));
        assert!(
            inst::package_data_dir_candidates("", "/h").contains(&sys.package_dir()),
            "Rust-окно находит данные установки в систему"
        );
        assert!(
            inst::main_window_candidates(None, "", &[]).contains(&sys.main_window()),
            "выбор входа находит бинарь окна установки в систему"
        );
        assert!(
            qm::module_dirs(None, "/h", "", &[]).contains(&sys.qml_modules()),
            "QML-модули установки в систему ищутся"
        );
        assert!(
            inst::system_launcher(&sys.lib_dir).contains(&format!("exec {}", sys.main_window())),
            "команда `rexon` ведёт на бинарь окна этой же раскладки"
        );

        // Домашняя папка — та же форма, и поиск её знает (починка 11).
        let user = InstallLayout::for_mode(InstallMode::User, "/h", None).unwrap();
        assert_eq!(user.main_window(), "/h/.local/lib/rexon/rexon-main-window");
        assert_eq!(user.wallpaper_host(), "/h/.local/bin/rexon-wallpaper-host");
        assert_eq!(
            InstallLayout::for_mode(InstallMode::User, "/h", Some("/xdg")),
            Some(user.clone()),
            "XDG_DATA_HOME двигает ярлыки и значки, но не программу"
        );
        assert!(
            inst::main_window_candidates(None, "/h", &[]).contains(&user.main_window()),
            "выбор входа находит бинарь окна установки в домашнюю папку"
        );
        assert!(
            qm::module_dirs(None, "/h", "", &[]).contains(&user.qml_modules()),
            "QML-модули установки в домашнюю папку ищутся"
        );
        assert!(
            inst::package_data_dir_candidates("", "/h").contains(&user.package_dir()),
            "Rust-окно находит данные установки в домашнюю папку"
        );
        assert!(
            !inst::main_window_candidates(None, "", &[]).iter().any(|c| c.contains(".local")),
            "без домашней папки кандидата нет — относительный путь не ищется"
        );
        assert!(InstallLayout::for_mode(InstallMode::Portable, "/h", None).is_none());

        // Пакет кладёт нашу .so туда же (PKGBUILD), а бывшую библиотеку
        // rexon-gui не кладёт вовсе.
        let pkgbuild = include_str!("../../../packaging/aur/PKGBUILD");
        assert!(pkgbuild.contains(&format!("\"$qmod/{}/{}\"", qm::RUST_LIB_SUBDIR, qm::FILTER_LIB_NAME)));
        assert!(!pkgbuild.contains(RETIRED_GUI_LIB));

        // Расширения берутся из пакета внутри <lib>, а не из <lib> напрямую.
        let io = FakeIo::ok();
        install(&io, &request());
        assert!(script_of(&io).contains(&format!("EXT_SRC=/usr/local/lib/rexon/rexon/\"$DE_DIR\"/{}", inst::EXT_ID)));
    }

    #[test]
    fn payload_from_bundle_follows_the_same_layout() {
        let io = FakeIo::ok();
        assert_eq!(PayloadSource::from_bundle(&io, "/src"), payload());
        assert_eq!(PayloadSource::for_running(&io, "/src/rexon-main-window", "", "/h", None), payload());
    }

    // ── Правило без пароля — только по согласию (починка 7) ─────────

    #[test]
    fn no_consent_no_passwordless_rule() {
        let io = FakeIo::ok();
        let req = request();
        assert!(!req.passwordless_rule, "по умолчанию выключено");
        let (r, _) = install(&io, &req);
        assert!(r.ok());
        let st = StagePaths::new(STAGE);
        assert!(io.written(&st.helpers().tmp_de_rules).is_none(), "файла правила нет");
        let script = script_of(&io);
        assert!(!script.contains(&format!("{} {}", st.helpers().tmp_de_rules, plan::REXON_POLKIT_RULES_PATH)));
        assert!(!script.contains("rexon_step rule"));
        assert!(script.contains(&format!("rm -f {}", plan::REXON_POLKIT_RULES_PATH)), "молчаливое прежнее снимается");
        // Помощники при этом ставятся все: восемь файлов прежних и два —
        // сеанса прав (бинарь и политика).
        assert_eq!(script.matches(&format!("{}/", st.polkit_dir())).count(), 10);
    }

    /// ⛔ «Один пароль на запуск» (Б11): ручная установка кладёт помощника
    /// сеанса прав бинарём из поставки (755) и политику `org.rexon.session`
    /// (644) в шаге помощников; политика указывает на путь ручной установки.
    #[test]
    fn installs_session_helper_and_its_policy() {
        let io = FakeIo::ok();
        let (r, _) = install(&io, &request());
        assert!(r.ok());
        let st = StagePaths::new(STAGE);
        let copied = io.calls().into_iter().find_map(|c| match c {
            Call::CopyFile(src, dst, m) if dst == st.priv_helper() => Some((src, m)),
            _ => None,
        });
        assert_eq!(copied, Some(("/src/bin/rexon-priv-helper".to_string(), 0o755)));
        let (policy, mode) = io.written(&st.session_policy()).expect("политика записана");
        assert_eq!(mode, 0o644);
        assert_eq!(policy, plan::session_policy_xml());
        assert!(policy.contains(plan::SESSION_HELPER_PATH));
        let script = script_of(&io);
        let at = script.find("rexon_step helpers").expect("шаг помощников");
        let tail = &script[at..script.find("rexon_step caches").unwrap()];
        assert!(tail.contains(&format!("install -Dm755 {} {}\n", st.priv_helper(), plan::SESSION_HELPER_PATH)), "{tail}");
        assert!(tail.contains(&format!("install -Dm644 {} {}\n", st.session_policy(), plan::SESSION_POLICY_PATH)), "{tail}");
        assert!(!script.contains(&format!("rm -f {}", plan::SESSION_HELPER_PATH)));
    }

    /// Поставка без помощника сеанса: политика не пишется, а прежний
    /// помощник снимается — окно новой сборки его всё равно отвергло бы
    /// («Rexon обновлён»), а без файла операции идут прежним путём.
    #[test]
    fn payload_without_session_helper_removes_the_old_one() {
        let io = FakeIo::ok();
        let mut req = request();
        req.payload.priv_helper = None;
        let (r, _) = install(&io, &req);
        assert!(r.ok());
        let st = StagePaths::new(STAGE);
        assert!(io.written(&st.session_policy()).is_none());
        let script = script_of(&io);
        assert!(script.contains(&format!("rm -f {}\n", plan::SESSION_HELPER_PATH)), "{script}");
        assert!(!script.contains(plan::SESSION_POLICY_PATH), "{script}");
    }

    /// Поставка находит помощника там, куда его кладёт сборка: в дереве
    /// разработчика — рядом с бинарём окна, в архиве — в `bin/`.
    #[test]
    fn payload_finds_session_helper() {
        let mut io = FakeIo::ok();
        let release = inst::DEV_TREE_MAIN_WINDOW_REL.rsplit_once('/').unwrap().0;
        let dev = format!("/repo/{release}/{}", plan::SESSION_HELPER_NAME);
        io.files.insert(dev.clone());
        io.files.insert("/b/bin/rexon-priv-helper".into());
        assert_eq!(PayloadSource::from_dev_tree(&io, "/repo", "/home/u", None).priv_helper, Some(dev));
        assert_eq!(PayloadSource::from_bundle(&io, "/b").priv_helper, Some("/b/bin/rexon-priv-helper".into()));
        assert_eq!(PayloadSource::from_bundle(&io, "/nothing").priv_helper, None);
    }

    #[test]
    fn with_consent_rule_is_a_separate_narrow_step() {
        let io = FakeIo::ok();
        let mut req = request();
        req.passwordless_rule = true;
        let (r, _) = install(&io, &req);
        assert!(r.ok());
        let st = StagePaths::new(STAGE);
        let (rules, mode) = io.written(&st.helpers().tmp_de_rules).expect("правило записано");
        assert_eq!(mode, 0o644);
        assert_eq!(rules, plan::passwordless_rules_js(PkgFamily::Arch));
        let script = script_of(&io);
        let at = script.find("rexon_step rule").expect("отдельный шаг");
        assert!(script[at..].contains(&format!("{} {}", st.helpers().tmp_de_rules, plan::REXON_POLKIT_RULES_PATH)));
        assert!(!script.contains(&format!("rm -f {}", plan::REXON_POLKIT_RULES_PATH)));
    }

    #[test]
    fn passwordless_rule_is_narrow_and_knows_admin_group_of_family() {
        let arch = plan::passwordless_rules_js(PkgFamily::Arch);
        for action in plan::PASSWORDLESS_ACTIONS {
            assert!(arch.contains(&format!("\"{action}\"")));
        }
        assert!(!arch.contains(plan::PKG_ACTION), "пакеты по имени — только с паролем");
        assert!(!arch.contains("indexOf(\"org.rexon.\") === 0"), "не всё семейство org.rexon.*");
        assert!(arch.contains("subject.isInGroup(\"wheel\")") && !arch.contains("\"sudo\""));
        let debian = plan::passwordless_rules_js(PkgFamily::Debian);
        assert!(debian.contains("subject.isInGroup(\"sudo\") || subject.isInGroup(\"admin\")"));
        assert!(!debian.contains("\"wheel\""), "в Debian группы wheel у администраторов нет");
        assert_eq!(plan::polkit_admin_groups(PkgFamily::Solus), &["sudo"]);
        assert_eq!(plan::polkit_admin_groups(PkgFamily::Fedora), &["wheel"]);
        assert_eq!(plan::polkit_admin_groups(PkgFamily::Unknown), &["wheel"]);
    }

    #[test]
    fn classic_installer_script_sets_rule_only_with_consent() {
        let h = StagePaths::new(STAGE).helpers();
        let mut p = inst::InstallScriptParams {
            lib_dir: inst::LIB_DIR.into(),
            install_dir: inst::INSTALL_DIR.into(),
            desktop_dir: inst::DESKTOP_DIR.into(),
            tmp_pkg: "/tmp/p".into(),
            tmp_launcher: "/tmp/l".into(),
            tmp_desktop: "/tmp/d".into(),
            tmp_recover: "/tmp/r".into(),
            icons: IconSet::default(),
            helpers: h.clone(),
            dep_cmd: String::new(),
            passwordless_rule: false,
        };
        let install_line = format!("install -Dm644 {} {}", h.tmp_de_rules, plan::REXON_POLKIT_RULES_PATH);
        assert!(!inst::system_install_script(&p).contains(&install_line));
        p.passwordless_rule = true;
        assert!(inst::system_install_script(&p).contains(&install_line));
        assert_eq!(inst::system_install_cmd("/tmp/x.sh").timeout_secs, Some(600));
    }

    // ── Прочее ──────────────────────────────────────────────────────

    /// Зависимости спрашиваются у динамического компоновщика по файлам самой
    /// поставки, а интерпретатор больше не поднимается ни разу.
    #[test]
    fn runtime_deps_are_asked_of_the_dynamic_linker() {
        let ran = |io: &FakeIo| -> Vec<Vec<String>> {
            io.calls()
                .into_iter()
                .filter_map(|c| match c {
                    Call::Run(a, t) => {
                        assert_eq!(t, Some(LDD_TIMEOUT_SECS), "потолок пробы: {a:?}");
                        Some(a)
                    }
                    _ => None,
                })
                .collect()
        };

        // Нечем проверить: `ldd` в системе нет — молчим и никого не зовём.
        let io = FakeIo::ok();
        assert!(runtime_deps_present(&io, &payload()));
        assert!(io.calls().is_empty());

        // Все библиотеки на месте: спрошены ровно файлы поставки.
        let mut io = FakeIo::ok();
        io.tools.insert(LDD.into());
        io.reply = Box::new(|_| out(0, "\tlibQt6Core.so.6 => /usr/lib/libQt6Core.so.6\n", ""));
        assert!(runtime_deps_present(&io, &payload()));
        assert_eq!(
            ran(&io),
            [
                "/src/rexon-main-window",
                "/src/bin/rexon-wallpaper-host",
                "/src/bin/rexon-splash-overlay",
                "/src/qml_modules/waywallen/control/librexon_qt_filter.so",
            ]
            .map(|p| vec![LDD.to_string(), p.to_string()])
        );

        // Одна неразрешённая библиотека — и зависимости ставятся.
        let mut io = FakeIo::ok();
        io.tools.insert(LDD.into());
        io.reply = Box::new(|argv: &[String]| {
            if argv[1].ends_with("rexon-splash-overlay") {
                out(0, "\tlibmpv.so.2 => not found\n", "")
            } else {
                out(0, "\tlibQt6Core.so.6 => /usr/lib/libQt6Core.so.6\n", "")
            }
        });
        assert!(!runtime_deps_present(&io, &payload()));
        let st = StagePaths::new(STAGE);
        assert!(!system_script_params(&io, &request(), &st).dep_lines.is_empty());

        // В поставке нет ни одного бинаря — проверять нечего.
        let mut io = FakeIo::ok();
        io.tools.insert(LDD.into());
        let empty = PayloadSource {
            main_window: None,
            wallpaper_host: None,
            splash_overlay: None,
            filter_lib: None,
            ..payload()
        };
        assert!(runtime_deps_present(&io, &empty));
        assert!(io.calls().is_empty());
    }

    #[test]
    fn deps_lines_port_install_deps_cmd() {
        assert_eq!(deps_script_lines(PkgFamily::Arch, true, true), "");
        let all = deps_script_lines(PkgFamily::Arch, false, false);
        assert_eq!(all.lines().next(), Some(inst::dep_install_cmd(PkgFamily::Arch).as_str()));
        assert!(all.lines().nth(1).unwrap().ends_with(" || true"));
        assert_eq!(deps_script_lines(PkgFamily::Nix, false, false), "", "NixOS: ставить нечем");
    }

    /// ⭐⭐ Сторож разреза 16.09.2026, и проверяется он У ВСЕХ семейств, а не
    /// у одного Arch: обязательные зависимости уходят СВОЕЙ строкой без
    /// прикрытия (падение обрывает установку — так и задумано), желательные
    /// — своей, с `|| true`. Имена желательных в одиннадцати семействах
    /// выписаны по правилам именования и вживую не проверены: опечатка там
    /// не должна стоить человеку всей установки.
    #[test]
    fn every_family_sends_must_have_and_nice_to_have_in_separate_lines() {
        for family in [
            PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse,
            PkgFamily::Gentoo, PkgFamily::Alpine, PkgFamily::Void, PkgFamily::Nix,
            PkgFamily::Solus, PkgFamily::Clear, PkgFamily::Alt, PkgFamily::Unknown,
        ] {
            let name = family.as_str();
            // Переключатель стиля уже стоит — в тексте остаются только два
            // разряда зависимостей.
            let text = deps_script_lines(family, false, true);
            let must = inst::dep_install_cmd(family);
            let want = inst::optional_dep_install_cmd(family);
            if must.is_empty() {
                // NixOS и «менеджер не опознан» — ставить нечем ни тем, ни
                // другим разрядом; выдумывать команду нельзя.
                assert!(want.is_empty(), "{name}: желательные есть, а обязательных нет");
                assert_eq!(text, "", "{name}: ставить нечем — строк нет");
                continue;
            }
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(lines.len(), 2, "{name}: разрядов не два: {text}");
            assert_eq!(lines[0], must, "{name}: первая строка — обязательные");
            assert!(
                !lines[0].contains("|| true"),
                "{name}: обязательные прикрыты — отказ стал бы молчаливым"
            );
            assert_eq!(lines[1], format!("{want} || true"), "{name}: вторая строка — желательные");
            // Строки не пересекаются: ни одно имя не ставится дважды.
            for pkg in inst::optional_dep_packages(family) {
                assert!(
                    !lines[0].split_whitespace().any(|w| w == pkg),
                    "{name}: «{pkg}» попал и в обязательные"
                );
            }
        }
    }

    /// Желательные едут ровно тогда же, когда обязательные: ответ пробы
    /// `ldd` один на всю поставку, и «компоновщик доволен» значит, что звать
    /// пакетный менеджер незачем ни за тем, ни за другим разрядом.
    #[test]
    fn nice_to_have_packages_ride_with_the_must_have_ones() {
        assert_eq!(deps_script_lines(PkgFamily::Arch, true, true), "");
        // Стоит только переключатель стиля — и он один с `|| true`.
        let only_style = deps_script_lines(PkgFamily::Arch, true, false);
        assert_eq!(only_style, format!("{} || true", inst::qt_style_install_cmd(PkgFamily::Arch)));
        // Всё сразу — три строки, прикрыты две последние.
        let all = deps_script_lines(PkgFamily::Arch, false, false);
        let lines: Vec<&str> = all.lines().collect();
        assert_eq!(lines.len(), 3, "{all}");
        assert!(!lines[0].contains("|| true"));
        assert!(lines[1].ends_with(" || true") && lines[2].ends_with(" || true"));
    }

    #[test]
    fn plan_has_only_known_steps_and_cleans_stage() {
        let st = StagePaths::new(STAGE);
        let io = FakeIo::ok();
        let steps = install_plan(&system_script_params(&io, &request(), &st));
        assert!(check_plan(&steps));
        assert_eq!(steps.last(), Some(&PlanStep::RemoveFiles { paths: vec![STAGE.to_string()] }));
        assert!(!check_plan(&[PlanStep::EnsureGentooOverlay { overlay: "x".into() }]));
        assert!(!stage_path_is_plain("/tmp/with space/x"));
        // Каждая метка скрипта — известный шаг, и все метки из таблицы есть в скрипте.
        let script = install_script(&system_script_params(&io, &request(), &st));
        for (id, _) in SCRIPT_MARKS.iter().filter(|(id, _)| *id != "rule") {
            assert!(script.contains(&format!("\nrexon_step {id}\n")), "{id}");
        }
        // Кэши в скрипте — из той же таблицы, что у установки в домашнюю папку.
        for group in cache_refresh_groups(inst::DESKTOP_DIR, "/usr/share/icons/hicolor") {
            for argv in &group {
                assert!(script.contains(&format!("{} 2>/dev/null", argv.join(" "))), "{argv:?}");
            }
        }
        assert!(script.contains(&format!("'{GNOME_EXTENSION_SRC}|/usr/share/gnome-shell/extensions'")));
    }

    #[test]
    fn progress_lines_come_from_wizard_texts_without_jargon() {
        let io = FakeIo::new(|| out(1, &marks(&["deps"]), &format!("{FAIL_MARK}deps")));
        let (_, sink) = install(&io, &request());
        let io2 = FakeIo::ok();
        let (_, sink2) = install(&io2, &request());
        for (stage, m) in sink.lines.lock().unwrap().iter().chain(sink2.lines.lock().unwrap().iter()) {
            for bad in ["pkexec", "polkit", "sudo", ".desktop", "PySide", "/usr", "rules.d"] {
                assert!(!m.contains(bad), "жаргон «{bad}» в строке хода [{stage}] «{m}»");
            }
        }
    }

    // ── Домашняя папка: настоящая ФС ТОЛЬКО во временном каталоге теста ──

    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SCRATCH_N: AtomicUsize = AtomicUsize::new(0);

    /// Подставной исполнитель с настоящей ФС внутри своего временного
    /// каталога. КАЖДЫЙ путь записи, переименования и удаления сверяется с
    /// корнем — выход за него роняет тест, живой HOME недостижим. Внешние
    /// команды НЕ исполняются, только записываются.
    struct ScratchIo {
        base: String,
        env: HashMap<String, String>,
        tools: HashSet<String>,
        runs: RefCell<Vec<Vec<String>>>,
        /// Сбой копирования дерева, путь назначения которого содержит строку.
        fail_copy_tree_into: Option<String>,
        /// Один сбой переименования В этот путь.
        fail_rename_once_into: RefCell<Option<String>>,
    }

    impl ScratchIo {
        fn new() -> Self {
            let n = SCRATCH_N.fetch_add(1, Ordering::SeqCst);
            let base = std::env::temp_dir().join(format!("rexon-g3b-test-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).expect("временный каталог теста");
            let base = base.canonicalize().unwrap().to_string_lossy().into_owned();
            let home = format!("{base}/home");
            std::fs::create_dir_all(&home).unwrap();
            let env = [
                ("HOME".to_string(), home.clone()),
                ("PATH".to_string(), format!("/usr/bin:{home}/.local/bin")),
            ]
            .into_iter()
            .collect();
            let tools = ["update-desktop-database", "gtk-update-icon-cache", "kbuildsycoca6", GLIB_COMPILE_SCHEMAS]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let io = ScratchIo {
                base,
                env,
                tools,
                runs: RefCell::new(Vec::new()),
                fail_copy_tree_into: None,
                fail_rename_once_into: RefCell::new(None),
            };
            for (rel, text, mode) in [
                ("src/rexon/app.py", "print()", 0o644),
                ("src/rexon/themes.json", "{}", 0o644),
                ("src/rexon/qt_wallpaper_engine/qml_ui/PageHost.qml", "import QtQuick", 0o644),
                ("src/rexon/.hidden.json", "{}", 0o644),
                ("src/rexon/__pycache__/app.cpython.pyc", "x", 0o644),
                ("src/rexon/tools/run.sh", "#!/bin/sh", 0o755),
                ("src/rexon/gnome-extension/rexon-wallpaper@rexon.app/extension.js", "//", 0o644),
                ("src/rexon/gnome-extension/rexon-wallpaper@rexon.app/schemas/org.rexon.gschema.xml", "<x/>", 0o644),
                ("src/rexon/cinnamon-extension/rexon-wallpaper@rexon.app/extension.js", "//", 0o644),
                ("src/rexon-main-window", "ELF", 0o755),
                ("src/bin/rexon-wallpaper-host", "ELF", 0o755),
                ("src/bin/rexon-splash-overlay", "ELF", 0o755),
                ("src/qml_modules/Qcm/Material/libqml_materialplugin.so", "so", 0o755),
                // Залежавшаяся копия бывшей библиотеки — установка её не берёт.
                ("src/qml_modules/waywallen/control/librexon_gui.so", "so", 0o755),
                ("src/qml_modules/waywallen/control/librexon_qt_filter.so", "so", 0o755),
                ("src/rexon_icon_16.png", "png", 0o644),
                ("src/rexon_icon_128.png", "png", 0o644),
                ("src/rexon_icon.svg", "<svg/>", 0o644),
            ] {
                io.put(rel, text, mode);
            }
            io
        }

        fn home(&self) -> String {
            self.env["HOME"].clone()
        }

        fn xdg(&self) -> Option<&str> {
            self.env.get("XDG_DATA_HOME").map(String::as_str).filter(|x| !x.is_empty())
        }

        fn layout(&self) -> InstallLayout {
            InstallLayout::for_mode(InstallMode::User, &self.home(), self.xdg()).unwrap()
        }

        fn paths(&self) -> inst::InstallPaths {
            inst::install_paths(InstallMode::User, &self.home(), self.xdg())
        }

        fn request(&self) -> UserInstallRequest {
            UserInstallRequest::new(PayloadSource::from_bundle(self, &format!("{}/src", self.base)), RU)
        }

        fn guard(&self, path: &str) {
            assert!(
                path.starts_with(&format!("{}/", self.base)) && !path.split('/').any(|s| s == ".."),
                "запись вне временного каталога теста: {path}"
            );
        }

        /// Файл внутри временного каталога теста (путь от корня).
        fn put(&self, rel: &str, text: &str, mode: u32) {
            let path = format!("{}/{rel}", self.base);
            std::fs::create_dir_all(parent_dir(&path)).unwrap();
            std::fs::write(&path, text).unwrap();
            set_mode(Path::new(&path), mode).unwrap();
        }

        fn runs(&self) -> Vec<Vec<String>> {
            self.runs.borrow().clone()
        }
    }

    impl Drop for ScratchIo {
        fn drop(&mut self) {
            if self.base.contains("rexon-g3b-test-") {
                let _ = std::fs::remove_dir_all(&self.base);
            }
        }
    }

    impl SystemIo for ScratchIo {
        fn exists(&self, path: &str) -> bool {
            Path::new(path).exists()
        }
        fn is_file(&self, path: &str) -> bool {
            Path::new(path).is_file()
        }
        fn is_dir(&self, path: &str) -> bool {
            Path::new(path).is_dir()
        }
        fn is_executable(&self, path: &str) -> bool {
            std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        }
        fn realpath(&self, path: &str) -> Option<String> {
            std::fs::canonicalize(path).ok().map(|p| p.to_string_lossy().into_owned())
        }
        fn read_text(&self, path: &str) -> Option<String> {
            std::fs::read_to_string(path).ok()
        }
        fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
            let entries = std::fs::read_dir(dir).ok()?;
            Some(entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        }
        fn which(&self, name: &str) -> bool {
            self.tools.contains(name)
        }
        fn run(&self, argv: &[String], _timeout_secs: Option<u64>, _lang_c: bool) -> RunOut {
            self.runs.borrow_mut().push(argv.to_vec());
            out(0, "", "")
        }
        fn env(&self, name: &str) -> String {
            self.env.get(name).cloned().unwrap_or_default()
        }
        fn which_path(&self, _name: &str) -> Option<String> {
            None
        }
        fn write_text(&self, path: &str, _content: &str) -> Result<(), String> {
            panic!("write_text исполнитель установки не зовёт: {path}")
        }
        fn copy_file(&self, src: &str, _dst: &str) -> Result<(), String> {
            panic!("copy_file исполнитель установки не зовёт: {src}")
        }
    }

    impl InstallIo for ScratchIo {
        fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String> {
            let parent = parent.map(str::to_string).unwrap_or_else(|| format!("{}/tmp", self.base));
            self.guard(&format!("{parent}/{prefix}"));
            std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
            for k in 0.. {
                let path = format!("{parent}/{prefix}{k}");
                if std::fs::create_dir(&path).is_ok() {
                    set_mode(Path::new(&path), 0o700)?;
                    return Ok(path);
                }
            }
            unreachable!()
        }
        fn rename(&self, src: &str, dst: &str) -> Result<(), String> {
            self.guard(src);
            self.guard(dst);
            let mut fail = self.fail_rename_once_into.borrow_mut();
            if fail.as_deref() == Some(dst) {
                *fail = None;
                return Err("переименование не удалось".into());
            }
            std::fs::rename(src, dst).map_err(|e| e.to_string())
        }
        fn make_dirs(&self, path: &str, mode: Option<u32>) -> Result<(), String> {
            self.guard(path);
            std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
            match mode {
                Some(m) => set_mode(Path::new(path), m),
                None => Ok(()),
            }
        }
        fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
            self.guard(path);
            std::fs::write(path, content).map_err(|e| e.to_string())?;
            set_mode(Path::new(path), mode)
        }
        fn copy_file_mode(&self, src: &str, dst: &str, mode: u32) -> Result<(), String> {
            self.guard(dst);
            std::fs::copy(src, dst).map_err(|e| e.to_string())?;
            set_mode(Path::new(dst), mode)
        }
        fn copy_tree(
            &self,
            src: &str,
            dst: &str,
            skip: &dyn Fn(&str) -> bool,
            mode: &dyn Fn(bool, bool) -> u32,
        ) -> Result<usize, String> {
            self.guard(dst);
            if self.fail_copy_tree_into.as_deref().is_some_and(|f| dst.contains(f)) {
                return Err("диск переполнен".into());
            }
            let mut count = 0;
            walk_copy(Path::new(src), Path::new(dst), skip, mode, &mut count)?;
            Ok(count)
        }
        fn remove_file(&self, path: &str) {
            self.guard(path);
            let _ = std::fs::remove_file(path);
        }
        fn remove_tree(&self, path: &str) {
            self.guard(path);
            let _ = std::fs::remove_dir_all(path);
        }
    }

    fn user_install(io: &ScratchIo) -> (InstallResult, Arc<RecSink>) {
        let sink = Arc::new(RecSink::default());
        let r = install_for_user(io, &io.request(), sink.clone(), &CancelToken::new());
        (r, sink)
    }

    /// Файлы и ссылки под каталогом (без самих каталогов), по порядку.
    fn tree(dir: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                if e.file_type().unwrap().is_dir() {
                    stack.push(e.path());
                } else {
                    found.push(e.path().to_string_lossy().into_owned());
                }
            }
        }
        found.sort();
        found
    }

    fn mode_of(path: &str) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn read(path: &str) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// Временный каталог установки убран на любом исходе.
    fn no_stage_left(io: &ScratchIo) {
        let parent = parent_dir(&io.layout().lib_dir).to_string();
        let left: Vec<String> = io
            .list_dir(&parent)
            .unwrap_or_default()
            .into_iter()
            .filter(|n| n.starts_with(USER_STAGE_PREFIX))
            .collect();
        assert!(left.is_empty(), "временный каталог остался: {left:?}");
        assert!(tree(&io.home()).iter().all(|f| !f.ends_with(".rexon-new")), "временные имена файлов убраны");
    }

    fn user_failure(r: &InstallResult) -> InstallFailure {
        r.failure().cloned().unwrap_or_else(|| panic!("ждали неудачу: {r:?}"))
    }

    #[test]
    fn user_install_lays_out_one_tree_and_uninstall_finds_it() {
        let mut io = ScratchIo::new();
        let xdg = format!("{}/xdg-data", io.home());
        io.env.insert("XDG_DATA_HOME".into(), xdg.clone());
        let (r, sink) = user_install(&io);
        assert!(r.ok(), "{r:?}");
        assert_eq!(
            r.outcome,
            Some(WizardOutcome::Installed { mode: InstallMode::User, fell_back: None, command_dir_not_in_path: None })
        );
        assert_eq!(r.message, inst::install_done_line(InstallMode::User, RU));
        let l = io.layout();
        let p = io.paths();
        assert_eq!(r.entry, Some(LaunchEntry::MainWindow(l.main_window())));

        // Программа — по раскладке пакета, права разложены.
        let pkg = l.package_dir();
        assert_eq!(mode_of(&format!("{pkg}/themes.json")), 0o644);
        assert_eq!(mode_of(&format!("{pkg}/tools/run.sh")), 0o755);
        assert!(io.is_file(&format!("{pkg}/.hidden.json")), "скрытые файлы не теряются");
        assert!(!io.exists(&format!("{pkg}/__pycache__")), "кеши байткода не ставятся");
        // Исходников Python в установке нет, а каталог данных всё равно
        // узнаётся — по вендорному QML вкладки обоев.
        assert!(!io.exists(&format!("{pkg}/app.py")), "исходники Python не ставятся");
        assert!(io.is_file(&format!("{pkg}/{}", inst::PACKAGE_MARKER_REL)));
        assert!(
            inst::package_data_dir_candidates("", &io.home()).contains(&pkg),
            "окно ищет данные там, куда установка их положила"
        );
        for exe in [l.main_window(), l.wallpaper_host(), l.splash_overlay(), l.filter_lib()] {
            assert!(io.is_executable(&exe), "{exe}");
        }
        // Залежавшаяся бывшая библиотека rexon-gui в каталоге модулей
        // поставки в установку не едет.
        assert!(!io.exists(&format!("{}/{RETIRED_GUI_LIB}", l.rust_lib_dir())));
        assert!(io.is_file(&format!("{}/{}", l.qml_modules(), qm::QCM_MATERIAL_REL)));

        // Лаунчер и ярлык ведут на бинарь окна этой же установки.
        assert_eq!(read(&l.launcher()), inst::user_launcher(&l.lib_dir));
        assert!(read(&l.launcher()).contains(&format!("exec {} \"$@\"", l.main_window())));
        assert_eq!(mode_of(&l.launcher()), 0o755);
        let desktop_dir = p.desktop_dir.clone().unwrap();
        assert!(desktop_dir.starts_with(&xdg), "ярлыки уважают XDG_DATA_HOME: {desktop_dir}");
        let desktop = read(&format!("{desktop_dir}/{DESKTOP_FILE}"));
        assert!(desktop.contains(&format!("Exec={}\n", l.main_window())), "{desktop}");
        assert!(io.is_file(&format!("{desktop_dir}/{RECOVER_DESKTOP_FILE}")));

        // Значки и pixmap — по общему правилу.
        for rel in ["icons/hicolor/16x16/apps/rexon.png", "icons/hicolor/128x128/apps/rexon.png", "icons/hicolor/scalable/apps/rexon.svg", "pixmaps/rexon.png"] {
            assert!(io.is_file(&format!("{xdg}/{rel}")), "{rel}");
        }

        // Дополнения обоев в пользовательские каталоги.
        let gnome = format!("{}/{}", p.gnome_ext_dir.clone().unwrap(), inst::EXT_ID);
        let cinnamon = format!("{}/{}", p.cinnamon_ext_dir.clone().unwrap(), inst::EXT_ID);
        assert!(io.is_file(&format!("{gnome}/extension.js")) && io.is_file(&format!("{cinnamon}/extension.js")));

        // Починка 8: папка резервных копий есть и у этого режима.
        assert!(io.is_dir(&format!("{}/.config/rexon", io.home())));
        assert!(io.is_dir(&format!("{xdg}/rexon/backups")));

        // Внешние команды — только кэши и схемы, без пароля.
        let icon_root = p.icon_root.clone().unwrap();
        let s = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        assert_eq!(
            io.runs(),
            vec![
                s(&["update-desktop-database", &desktop_dir]),
                s(&["gtk-update-icon-cache", "-f", "-t", &icon_root]),
                s(&["kbuildsycoca6", "--noincremental"]),
                s(&[GLIB_COMPILE_SCHEMAS, &format!("{gnome}/schemas")]),
            ]
        );
        no_stage_left(&io);

        // Установили → удаление находит всё, включая хост обоев и заставку
        // (починка G8). Вне его списка — только папки профиля.
        let removable = inst::uninstall_user_paths(&p, |x| std::fs::symlink_metadata(x).is_ok());
        for extra in l.extra_bin_paths() {
            assert!(removable.contains(&extra), "удаление не знает {extra}");
        }
        let uncovered: Vec<String> = tree(&io.home())
            .into_iter()
            .filter(|f| !removable.iter().any(|r| f == r || f.starts_with(&format!("{r}/"))))
            .collect();
        assert_eq!(uncovered, Vec::<String>::new());
        assert!(inst::system_or_user_installed(&p, |x| io.exists(x), |x| io.is_dir(x)), "первый запуск увидит установку");

        for (stage, m) in sink.lines.lock().unwrap().iter() {
            for bad in ["pkexec", "polkit", ".desktop", "PySide", "/usr", "~/.local", "$PATH", "User-", "Portable"] {
                assert!(!m.contains(bad), "жаргон «{bad}» в строке хода [{stage}] «{m}»");
            }
        }
        assert_eq!(sink.cancellable.lock().unwrap().first(), Some(&true));
        assert_eq!(sink.cancellable.lock().unwrap().last(), Some(&false));
    }

    #[test]
    fn user_reinstall_replaces_previous_copy_whole() {
        let io = ScratchIo::new();
        let l = io.layout();
        let stale = format!("{}/stale.py", l.package_dir());
        io.put(&stale[io.base.len() + 1..], "old", 0o644);
        let (r, _) = user_install(&io);
        assert!(r.ok(), "{r:?}");
        assert!(!io.exists(&stale), "прежняя копия заменена целиком");
        assert!(io.is_file(&format!("{}/themes.json", l.package_dir())));
        no_stage_left(&io);
    }

    #[test]
    fn user_missing_cache_programs_are_not_a_failure() {
        let mut io = ScratchIo::new();
        io.tools.clear();
        let (r, sink) = user_install(&io);
        assert!(r.ok(), "{r:?}");
        assert!(io.runs().is_empty(), "запускать нечего — и не запускали");
        let lines = sink.lines.lock().unwrap().clone();
        assert!(lines.contains(&(de_progress::STAGE_WARNING.to_string(), inst::menu_refresh_later_notice(RU))));
    }

    #[test]
    fn user_command_dir_outside_path_is_reported() {
        let mut io = ScratchIo::new();
        io.env.insert("PATH".into(), "/usr/bin".into());
        let (r, _) = user_install(&io);
        let bin = io.layout().bin_dir;
        let outcome = r.outcome.clone().unwrap();
        assert!(
            matches!(&outcome, WizardOutcome::Installed { command_dir_not_in_path: Some(d), .. } if *d == bin),
            "{outcome:?}"
        );
        assert!(inst::result_texts(&outcome, RU).notes.contains(&inst::command_path_notice(RU)));

        // Косая черта в конце элемента PATH — всё равно «входит».
        let mut io = ScratchIo::new();
        let bin = io.layout().bin_dir;
        io.env.insert("PATH".into(), format!("/usr/bin:{bin}/"));
        let (r, _) = user_install(&io);
        assert!(matches!(r.outcome, Some(WizardOutcome::Installed { command_dir_not_in_path: None, .. })));
    }

    #[test]
    fn user_copy_failure_keeps_previous_copy_cleans_up_and_names_step() {
        let mut io = ScratchIo::new();
        let l = io.layout();
        let old = format!("{}/app.py", l.package_dir());
        io.put(&old[io.base.len() + 1..], "old", 0o644);
        io.fail_copy_tree_into = Some(QML_MODULES_DIR_NAME.into());
        let (r, sink) = user_install(&io);
        let f = user_failure(&r);
        assert_eq!(f, InstallFailure::StepFailed { step: InstallStep::CopyFiles, detail: "диск переполнен".into() });
        assert_eq!(r.message, f.log_line(RU));
        assert_eq!(read(&old), "old", "починка 9: прежняя копия цела");
        assert!(!io.exists(&l.launcher()));
        assert!(io.runs().is_empty());
        assert_eq!(sink.lines.lock().unwrap().last().unwrap().0, de_progress::STAGE_ERROR);
        no_stage_left(&io);

        // Сбой обмена копий — прежняя возвращается на место.
        let io = ScratchIo::new();
        let l = io.layout();
        let old = format!("{}/app.py", l.package_dir());
        io.put(&old[io.base.len() + 1..], "old", 0o644);
        *io.fail_rename_once_into.borrow_mut() = Some(l.lib_dir.clone());
        let (r, _) = user_install(&io);
        assert!(matches!(user_failure(&r), InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. }));
        assert_eq!(read(&old), "old", "прежняя копия возвращена");
        no_stage_left(&io);
    }

    #[test]
    fn user_cancel_before_swap_changes_nothing() {
        let io = ScratchIo::new();
        let l = io.layout();
        let old = format!("{}/app.py", l.package_dir());
        io.put(&old[io.base.len() + 1..], "old", 0o644);
        let cancel = CancelToken::new();
        cancel.cancel();
        let sink = Arc::new(RecSink::default());
        let r = install_for_user(&io, &io.request(), sink.clone(), &cancel);
        assert!(r.cancelled() && !r.ok(), "{r:?}");
        assert_eq!(read(&old), "old");
        assert!(!io.exists(&l.launcher()) && !io.exists(&l.main_window()));
        assert!(io.runs().is_empty());
        assert_eq!(sink.cancellable.lock().unwrap().last(), Some(&false));
        no_stage_left(&io);
    }

    #[test]
    fn user_links_are_replaced_never_written_through() {
        // Каталог установки — ссылка в дерево: не заменяется вовсе.
        let io = ScratchIo::new();
        io.put("devtree/rexon/app.py", "dev", 0o644);
        let l = io.layout();
        std::fs::create_dir_all(parent_dir(&l.lib_dir)).unwrap();
        std::os::unix::fs::symlink(format!("{}/devtree", io.base), &l.lib_dir).unwrap();
        let (r, _) = user_install(&io);
        assert!(matches!(user_failure(&r), InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. }));
        assert!(std::fs::symlink_metadata(&l.lib_dir).unwrap().file_type().is_symlink());
        assert_eq!(read(&format!("{}/devtree/rexon/app.py", io.base)), "dev");
        assert!(tree(&format!("{}/devtree", io.base)).len() == 1, "в дерево ничего не легло");
        no_stage_left(&io);

        // Лаунчер и хост обоев — ссылки: заменяются сами, цель не тронута.
        let io = ScratchIo::new();
        io.put("devtree/launcher", "dev", 0o755);
        io.put("devtree/host", "dev", 0o755);
        let l = io.layout();
        std::fs::create_dir_all(&l.bin_dir).unwrap();
        std::os::unix::fs::symlink(format!("{}/devtree/launcher", io.base), l.launcher()).unwrap();
        std::os::unix::fs::symlink(format!("{}/devtree/host", io.base), l.wallpaper_host()).unwrap();
        let (r, _) = user_install(&io);
        assert!(r.ok(), "{r:?}");
        assert_eq!(read(&format!("{}/devtree/launcher", io.base)), "dev");
        assert_eq!(read(&format!("{}/devtree/host", io.base)), "dev");
        assert!(!std::fs::symlink_metadata(l.launcher()).unwrap().file_type().is_symlink());
        assert_eq!(read(&l.wallpaper_host()), "ELF");

        // Ссылка выше по пути (Bazzite: /home → /var/home) — не повод отказать.
        io.put("realhome/x/y", "", 0o644);
        std::os::unix::fs::symlink(format!("{}/realhome", io.base), format!("{}/linkhome", io.base)).unwrap();
        assert!(!is_redirected(&io, &format!("{}/linkhome/x", io.base)));
        assert!(is_redirected(&io, &format!("{}/linkhome", io.base)));
    }

    #[test]
    fn user_without_home_refuses_before_any_side_effect() {
        let mut io = ScratchIo::new();
        io.env.insert("HOME".into(), String::new());
        let sink = Arc::new(RecSink::default());
        let req = UserInstallRequest::new(payload(), RU);
        let r = install_for_user(&io, &req, sink, &CancelToken::new());
        assert!(matches!(user_failure(&r), InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. }));
        assert!(io.runs().is_empty());
    }

    #[test]
    fn portable_prepares_profile_folders_only() {
        let mut io = ScratchIo::new();
        // Починка 8: пустой XDG_DATA_HOME — «не задан», а не текущий каталог.
        io.env.insert("XDG_DATA_HOME".into(), String::new());
        let req = InstallRequest::new(payload(), PkgFamily::Arch, "cachyos", RU);
        let sink = Arc::new(RecSink::default());
        let r = super::install(&io, InstallMode::Portable, &req, sink.clone(), &CancelToken::new());
        assert!(r.ok(), "{r:?}");
        assert_eq!(
            r.outcome,
            Some(WizardOutcome::Installed { mode: InstallMode::Portable, fell_back: None, command_dir_not_in_path: None })
        );
        assert_eq!(r.message, inst::portable_no_install_notice(RU));
        assert_eq!(r.entry, None);
        let home = io.home();
        assert!(io.is_dir(&format!("{home}/.config/rexon")));
        assert!(io.is_dir(&format!("{home}/.local/share/rexon/backups")));
        assert!(!io.exists(&format!("{home}/.local/lib")) && !io.exists(&format!("{home}/.local/bin")));
        assert!(io.runs().is_empty());
        assert_eq!(profile_dirs("/h", "", ""), vec!["/h/.config/rexon".to_string(), "/h/.local/share/rexon/backups".into()]);
        assert!(profile_dirs("", "", "").is_empty() && profile_dirs("rel", "", "").is_empty());
    }

    #[test]
    fn dispatcher_on_immutable_system_installs_for_user_before_touching_system() {
        let io = ScratchIo::new();
        let payload = PayloadSource::from_bundle(&io, &format!("{}/src", io.base));
        let req = InstallRequest::new(payload, PkgFamily::Arch, "steamos", RU);
        let sink = Arc::new(RecSink::default());
        let r = super::install(&io, InstallMode::System, &req, sink.clone(), &CancelToken::new());
        let reason = inst::system_mode_blocked_reason_in("steamos", RU).unwrap();
        assert_eq!(Some(reason.clone()), inst::system_mode_blocked_reason("steamos"), "то же решение, что у первого запуска");
        assert!(r.ok(), "{r:?}");
        assert_eq!(
            r.outcome,
            Some(WizardOutcome::Installed {
                mode: InstallMode::User,
                fell_back: Some(reason.clone()),
                command_dir_not_in_path: None,
            })
        );
        assert!(io.runs().iter().all(|a| a[0] != "pkexec"), "пароль не спрашивали");
        assert!(sink.has(&reason));
        assert!(inst::result_texts(r.outcome.as_ref().unwrap(), RU).notes.contains(&reason));
        assert!(io.is_executable(&io.layout().main_window()));
        no_stage_left(&io);
    }

    #[test]
    fn dispatcher_routes_system_and_user_modes() {
        let io = FakeIo::ok();
        let r = super::install(&io, InstallMode::System, &request(), Arc::new(RecSink::default()), &CancelToken::new());
        assert!(r.ok(), "{r:?}");
        assert_eq!(io.runs().len(), 1, "обычная система — установка в систему под паролем");

        let io = FakeIo::ok();
        super::install(&io, InstallMode::User, &request(), Arc::new(RecSink::default()), &CancelToken::new());
        assert!(io.runs().is_empty(), "в домашнюю папку — без пароля");
        for c in io.calls() {
            if let Call::Write(p, ..) | Call::CopyFile(_, p, _) | Call::CopyTree(_, p) | Call::MakeDirs(p, _) = &c {
                assert!(p.starts_with("/scratch/home/"), "запись вне домашней папки: {c:?}");
            }
        }
    }

    // ── Удаление (G8) ───────────────────────────────────────────────────
    //
    // Установка для всех — подменный `FakeIo` (ни одной записи и ни одного
    // запуска); домашняя папка — `ScratchIo` (настоящая ФС только внутри
    // временного каталога теста). `UninstallRequest::detect` на `ScratchIo`
    // не зовётся: он смотрел бы на живой `/usr/local`.

    const SYS_LIB: &str = "/usr/local/lib/rexon";
    const CONFIG: &str = "/scratch/home/.config/rexon";
    const BACKUPS: &str = "/scratch/home/.local/share/rexon/backups";

    /// Найдена установка для всех, профиль на месте.
    fn with_system(mut io: FakeIo) -> FakeIo {
        io.files.insert("/usr/local/bin/rexon".into());
        io.files.insert("/usr/share/applications/rexon.desktop".into());
        io.dirs.insert(SYS_LIB.into());
        io.dirs.insert(CONFIG.into());
        io.dirs.insert(BACKUPS.into());
        io
    }

    fn removed_ok() -> RunOut {
        out(0, &marks(&[inst::UNINSTALL_SCRIPT_MARK]), "")
    }

    fn un<IO: InstallIo>(io: &IO, req: &UninstallRequest) -> (UninstallResult, Arc<RecSink>) {
        let sink = Arc::new(RecSink::default());
        let r = uninstall(io, req, sink.clone(), &CancelToken::new());
        (r, sink)
    }

    fn uninstall_script(io: &FakeIo) -> String {
        io.written(&format!("{STAGE}/uninstall.sh")).expect("скрипт удаления записан").0
    }

    fn removed(io: &FakeIo) -> Vec<String> {
        io.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::RemoveTree(p) | Call::RemoveFile(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn system_uninstall_is_one_script_under_password_then_profile_and_cleanup() {
        let io = with_system(FakeIo::new(removed_ok));
        let req = UninstallRequest::detect(&io, None, false, RU);
        assert_eq!((req.system, req.user), (true, false));
        let (r, sink) = un(&io, &req);
        assert_eq!(r.outcome, UninstallOutcome::Removed { kept_links: vec![] }, "{}", r.message);
        assert!(r.ok());
        assert_eq!(r.message, inst::uninstall_done_line(false, RU));

        let script = format!("{STAGE}/uninstall.sh");
        assert_eq!(io.runs(), vec![(inst::uninstall_system_cmd(&script).argv, Some(300))]);
        let text = uninstall_script(&io);
        let sys = inst::install_paths(InstallMode::System, "", None);
        assert_eq!(text, inst::uninstall_system_script(&sys), "ссылок нет — скрипт целиком из ядра");
        let layout = InstallLayout::system();
        // Хост обоев и заставка (починка 15), правило «без пароля», папка программы.
        for must in [
            layout.wallpaper_host(),
            layout.splash_overlay(),
            plan::REXON_POLKIT_RULES_PATH.to_string(),
            SYS_LIB.to_string(),
        ] {
            assert!(text.lines().any(|l| l == format!("rm -rf {must}")), "скрипт не снимает {must}:\n{text}");
        }

        // Уборка временной папки — после запуска; профиль — последним.
        let calls = io.calls();
        let run_at = calls.iter().position(|c| matches!(c, Call::Run(a, _) if a[0] == "pkexec")).unwrap();
        let clean_at = calls.iter().position(|c| *c == Call::RemoveTree(STAGE.into())).unwrap();
        assert!(clean_at > run_at);
        assert_eq!(removed(&io), vec![STAGE.to_string(), CONFIG.into(), BACKUPS.into()]);

        for (stage, m) in sink.lines.lock().unwrap().iter() {
            for bad in ["pkexec", "polkit", "system", "/usr", "код "] {
                assert!(!m.contains(bad), "жаргон «{bad}» в строке хода [{stage}] «{m}»");
            }
        }
        assert_eq!(sink.cancellable.lock().unwrap().first(), Some(&true));
        assert_eq!(sink.cancellable.lock().unwrap().last(), Some(&false));
    }

    /// Решение Wehrwolfmann 15.09.2026: «сохранить» — это настройки И
    /// резервные копии. Прежний тест утверждал обратное (настройки сносятся,
    /// копии остаются) — ровно то поведение, которое отменено.
    #[test]
    fn keep_profile_leaves_settings_and_backups_untouched() {
        let io = with_system(FakeIo::new(removed_ok));
        let (r, sink) = un(&io, &UninstallRequest::detect(&io, None, true, RU));
        assert!(r.ok(), "{r:?}");
        assert_eq!(r.message, inst::uninstall_done_line(true, RU));
        let gone = removed(&io);
        assert_eq!(gone, vec![STAGE.to_string()], "убрана только временная папка: {gone:?}");
        for kept in [CONFIG, BACKUPS] {
            assert!(gone.iter().all(|p| !kept.starts_with(p.as_str())), "задета {kept}: {gone:?}");
        }
        // Раз настройки не удаляются, строки «Удаление настроек…» быть не должно.
        assert!(!sink.has(&inst::UninstallPart::Profile.progress_line(RU)));
        assert!(sink.has(&inst::UninstallPart::System.progress_line(RU)));

        // «Удалить всё» на той же машине убирает обе папки.
        let io = with_system(FakeIo::new(removed_ok));
        let (r, sink) = un(&io, &UninstallRequest::detect(&io, None, false, RU));
        assert_eq!(r.message, inst::uninstall_done_line(false, RU));
        let gone = removed(&io);
        assert!(gone.contains(&CONFIG.to_string()) && gone.contains(&BACKUPS.to_string()), "{gone:?}");
        assert!(sink.has(&inst::UninstallPart::Profile.progress_line(RU)));
    }

    /// ⛔ У владельца `/usr/local/lib/rexon` — ссылка в рабочее дерево.
    #[test]
    fn link_into_developer_tree_is_neither_removed_nor_followed() {
        let mut io = with_system(FakeIo::new(removed_ok));
        io.links.insert(SYS_LIB.into(), "/home/dev/rexon_v5/rexon".into());
        let req = UninstallRequest::detect(&io, None, false, RU);
        assert!(req.system, "установка видна — тем важнее не снести ссылку");
        let listed = uninstall_files(&io, &req).removed(false);
        assert!(!listed.contains(&SYS_LIB.to_string()), "перечень обещает удалить ссылку: {listed:?}");
        assert!(listed.contains(&"/usr/local/bin/rexon".to_string()), "{listed:?}");

        let (r, _) = un(&io, &req);
        assert_eq!(r.outcome, UninstallOutcome::Removed { kept_links: vec![SYS_LIB.to_string()] });
        assert!(r.message.contains(&inst::uninstall_kept_link_note(SYS_LIB, RU)), "{}", r.message);
        let text = uninstall_script(&io);
        assert!(!text.lines().any(|l| l == format!("rm -rf {SYS_LIB}")), "{text}");
        assert!(text.contains("rm -rf /usr/local/lib/rexon-system-helper\n"), "{text}");
        assert!(text.contains(&format!("rm -rf {}\n", InstallLayout::system().wallpaper_host())), "{text}");
        for p in removed(&io) {
            assert!(!p.starts_with("/home/dev") && p != SYS_LIB, "задет {p}");
        }
    }

    #[test]
    fn denied_password_stops_removal_cleans_up_and_keeps_profile() {
        let io = with_system(FakeIo::new(|| out(126, "", "Error executing command as another user: Not authorized")));
        let (r, sink) = un(&io, &UninstallRequest::detect(&io, None, false, RU));
        assert_eq!(r.outcome, UninstallOutcome::Failed(InstallFailure::PasswordDenied));
        assert_eq!(r.message, inst::uninstall_failure_line(&InstallFailure::PasswordDenied, RU));
        assert_eq!(removed(&io), vec![STAGE.to_string()], "убрана только временная папка");
        assert_eq!(sink.lines.lock().unwrap().last().unwrap().0, de_progress::STAGE_ERROR);
    }

    #[test]
    fn failed_removal_is_not_a_denied_password_and_other_outcomes_are_named() {
        // Скрипт начался (метка есть) и упал — даже с кодом 127 это сбой удаления.
        let io = with_system(FakeIo::new(|| {
            out(127, &marks(&[inst::UNINSTALL_SCRIPT_MARK]), "rm: cannot remove: Read-only file system")
        }));
        let (r, _) = un(&io, &UninstallRequest::detect(&io, None, false, RU));
        let UninstallOutcome::Failed(InstallFailure::StepFailed { detail, .. }) = &r.outcome else {
            panic!("ждали сбой удаления: {r:?}")
        };
        assert!(detail.contains("Read-only file system"), "{detail}");
        assert!(r.message.contains("Read-only file system"), "{}", r.message);
        assert_eq!(removed(&io), vec![STAGE.to_string()]);

        let failing = |run: RunOut| {
            let io = with_system(FakeIo::new(move || run.clone()));
            let (r, _) = un(&io, &UninstallRequest::detect(&io, None, false, RU));
            assert_eq!(removed(&io), vec![STAGE.to_string()], "уборка на любом исходе");
            r.outcome
        };
        let blank = |outcome| RunOut { outcome, returncode: None, stdout: String::new(), stderr: String::new() };
        assert_eq!(failing(blank(RunOutcome::Error)), UninstallOutcome::Failed(InstallFailure::PasswordPromptMissing));
        assert_eq!(failing(blank(RunOutcome::Timeout)), UninstallOutcome::Failed(InstallFailure::TimedOut { secs: 300 }));
    }

    #[test]
    fn script_write_failure_is_a_failure_and_asks_no_password() {
        struct NoWrite(FakeIo);
        impl SystemIo for NoWrite {
            fn exists(&self, p: &str) -> bool { self.0.exists(p) }
            fn is_file(&self, p: &str) -> bool { self.0.is_file(p) }
            fn is_dir(&self, p: &str) -> bool { self.0.is_dir(p) }
            fn is_executable(&self, p: &str) -> bool { self.0.is_executable(p) }
            fn realpath(&self, p: &str) -> Option<String> { self.0.realpath(p) }
            fn read_text(&self, p: &str) -> Option<String> { self.0.read_text(p) }
            fn list_dir(&self, p: &str) -> Option<Vec<String>> { self.0.list_dir(p) }
            fn which(&self, n: &str) -> bool { self.0.which(n) }
            fn run(&self, a: &[String], t: Option<u64>, l: bool) -> RunOut { self.0.run(a, t, l) }
            fn env(&self, n: &str) -> String { self.0.env(n) }
            fn which_path(&self, n: &str) -> Option<String> { self.0.which_path(n) }
            fn write_text(&self, p: &str, c: &str) -> Result<(), String> { self.0.write_text(p, c) }
            fn copy_file(&self, s: &str, d: &str) -> Result<(), String> { self.0.copy_file(s, d) }
        }
        impl InstallIo for NoWrite {
            fn temp_dir(&self, p: &str, par: Option<&str>) -> Result<String, String> { self.0.temp_dir(p, par) }
            fn rename(&self, s: &str, d: &str) -> Result<(), String> { self.0.rename(s, d) }
            fn make_dirs(&self, p: &str, m: Option<u32>) -> Result<(), String> { self.0.make_dirs(p, m) }
            fn write_file_mode(&self, _p: &str, _c: &str, _m: u32) -> Result<(), String> { Err("нет места".into()) }
            fn copy_file_mode(&self, s: &str, d: &str, m: u32) -> Result<(), String> { self.0.copy_file_mode(s, d, m) }
            fn copy_tree(&self, s: &str, d: &str, sk: &dyn Fn(&str) -> bool, m: &dyn Fn(bool, bool) -> u32) -> Result<usize, String> { self.0.copy_tree(s, d, sk, m) }
            fn remove_file(&self, p: &str) { self.0.remove_file(p) }
            fn remove_tree(&self, p: &str) { self.0.remove_tree(p) }
        }
        let io = NoWrite(with_system(FakeIo::new(removed_ok)));
        let (r, _) = un(&io, &UninstallRequest::detect(&io, None, false, RU));
        assert!(matches!(&r.outcome, UninstallOutcome::Failed(InstallFailure::StepFailed { detail, .. }) if detail == "нет места"), "{r:?}");
        assert!(io.0.runs().is_empty(), "пароль не спрашивали");
        assert_eq!(removed(&io.0), vec![STAGE.to_string()], "починка 18: временная папка убрана");
    }

    #[test]
    fn cancel_before_password_touches_nothing() {
        let io = with_system(FakeIo::new(removed_ok));
        let cancel = CancelToken::new();
        cancel.cancel();
        let sink = Arc::new(RecSink::default());
        let r = uninstall(&io, &UninstallRequest::detect(&io, None, false, RU), sink.clone(), &cancel);
        assert_eq!(r.outcome, UninstallOutcome::Cancelled);
        assert!(!r.ok());
        assert!(io.calls().is_empty(), "{:?}", io.calls());
        assert_eq!(sink.cancellable.lock().unwrap().first(), Some(&true));
        assert_eq!(sink.cancellable.lock().unwrap().last(), Some(&false));
    }

    #[test]
    fn package_goes_first_through_its_manager_then_system() {
        let io = with_system(FakeIo::new(removed_ok));
        let pkg = DistroPackage { family: PkgFamily::Arch, name: "rexon-git".into() };
        let req = UninstallRequest::detect(&io, Some(pkg), false, RU);
        assert!(uninstall_files(&io, &req).program[0].contains("rexon-git"));
        let (r, sink) = un(&io, &req);
        assert!(r.ok(), "{r:?}");
        let argv: Vec<Vec<String>> = io.runs().into_iter().map(|(a, _)| a).collect();
        assert_eq!(argv.len(), 2, "{argv:?}");
        assert_eq!(argv[0], vec!["pkexec", "pacman", "-R", "--noconfirm", "rexon-git"]);
        assert_eq!(argv[1][..2], ["pkexec", "bash"]);
        assert!(sink.has(&inst::UninstallPart::Package.progress_line(RU)));
    }

    #[test]
    fn denied_password_for_package_stops_before_everything_else() {
        let io = with_system(FakeIo::new(|| out(126, "", "")));
        let pkg = DistroPackage { family: PkgFamily::Arch, name: "rexon".into() };
        let (r, _) = un(&io, &UninstallRequest::detect(&io, Some(pkg), false, RU));
        assert_eq!(r.outcome, UninstallOutcome::Failed(InstallFailure::PasswordDenied));
        assert_eq!(io.runs().len(), 1, "скрипт удаления не запускали");
        assert!(removed(&io).is_empty(), "{:?}", io.calls());
    }

    #[test]
    fn package_without_known_remover_is_refused_before_any_side_effect() {
        let io = with_system(FakeIo::ok());
        let pkg = DistroPackage { family: PkgFamily::Unknown, name: "rexon".into() };
        let (r, _) = un(&io, &UninstallRequest::detect(&io, Some(pkg), false, RU));
        assert_eq!(r.outcome, UninstallOutcome::PackageUnsupported { name: "rexon".into() });
        assert_eq!(r.message, inst::uninstall_package_unsupported_line("rexon", RU));
        assert!(io.calls().is_empty(), "{:?}", io.calls());
    }

    /// Починка 19: прежде стирались настройки и звучало «не установлен».
    #[test]
    fn nothing_installed_touches_nothing_not_even_settings() {
        let mut io = FakeIo::ok();
        io.dirs.insert(CONFIG.into());
        let req = UninstallRequest::detect(&io, None, false, RU);
        assert!(uninstall_files(&io, &req).is_empty());
        let (r, _) = un(&io, &req);
        assert_eq!(r.outcome, UninstallOutcome::NotInstalled);
        assert_eq!(r.message, inst::uninstall_nothing_line(RU));
        assert!(!r.ok());
        assert!(io.calls().is_empty(), "{:?}", io.calls());
    }

    #[test]
    fn foreign_plan_step_is_refused() {
        let sys = inst::install_paths(InstallMode::System, "", None);
        let mut steps = inst::uninstall_system_plan(&sys, "/tmp/x.sh");
        assert!(check_uninstall_plan(&steps));
        steps.push(PlanStep::EnsureGentooOverlay { overlay: "чужой".into() });
        assert!(!check_uninstall_plan(&steps));
        // Запуск установки в плане удаления — тоже чужой шаг.
        let install_run = PlanStep::Run {
            id: RunStepId::AppInstall,
            cmd: inst::uninstall_system_cmd("/tmp/x.sh"),
            needs_root: true,
            description: String::new(),
        };
        assert!(!check_uninstall_plan(&[install_run]));
    }

    #[test]
    fn distro_package_owner_is_asked_of_the_package_manager() {
        let mut io = FakeIo::ok();
        io.tools.insert("pacman".into());
        io.files.insert("/usr/bin/rexon".into());
        io.reply = Box::new(|argv: &[String]| {
            if argv.last().map(String::as_str) == Some("/usr/bin/rexon") {
                out(0, "rexon-git\n", "")
            } else {
                out(1, "", "error: No package owns")
            }
        });
        let got = detect_distro_package(&io, PkgFamily::Arch, Some("/opt/rexon/rexon-main-window"));
        assert_eq!(got, Some(DistroPackage { family: PkgFamily::Arch, name: "rexon-git".into() }));
        let asked: Vec<(Vec<String>, Option<u64>)> = io
            .calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Run(a, t) => Some((a, t)),
                _ => None,
            })
            .collect();
        let q = |p: &str| (vec!["pacman".to_string(), "-Qoq".into(), p.into()], Some(OWNER_QUERY_TIMEOUT_SECS));
        assert_eq!(asked, vec![q("/opt/rexon/rexon-main-window"), q("/usr/bin/rexon")]);

        // Нет менеджера — не спрашиваем; у семейства нет детекта — тоже.
        let io = FakeIo::ok();
        assert_eq!(detect_distro_package(&io, PkgFamily::Arch, Some("/x")), None);
        assert_eq!(detect_distro_package(&io, PkgFamily::Gentoo, Some("/x")), None);
        assert!(io.calls().is_empty());
    }

    fn user_request(keep_profile: bool) -> UninstallRequest {
        UninstallRequest { package: None, system: false, user: true, keep_profile, lang: RU }
    }

    #[test]
    fn user_uninstall_removes_what_install_laid_out_host_and_splash_included() {
        let io = ScratchIo::new();
        assert!(user_install(&io).0.ok());
        io.put("home/.config/rexon/settings.json", "{}", 0o644);
        io.put("home/.local/share/rexon/backups/plasma-panel/1.json", "{}", 0o644);
        let l = io.layout();
        let listed = uninstall_files(&io, &user_request(false)).removed(false);
        for exe in l.extra_bin_paths() {
            assert!(io.is_file(&exe), "{exe}");
            assert!(listed.contains(&exe), "перечень не называет {exe}: {listed:?}");
        }
        let runs_before = io.runs().len();
        let (r, sink) = un(&io, &user_request(false));
        assert_eq!(r.outcome, UninstallOutcome::Removed { kept_links: vec![] }, "{}", r.message);
        assert_eq!(tree(&io.home()), Vec::<String>::new(), "в домашней папке не осталось файлов Rexon");
        assert!(!io.exists(&l.lib_dir));
        let after = io.runs()[runs_before..].to_vec();
        assert!(after.iter().all(|a| a[0] != "pkexec"), "без пароля: {after:?}");
        assert!(after.iter().any(|a| a[0] == UPDATE_DESKTOP_DATABASE), "меню обновлено: {after:?}");
        assert!(sink.has(&inst::UninstallPart::User.progress_line(RU)));
        assert!(!sink.has(&inst::menu_refresh_later_notice(RU)));
    }

    #[test]
    fn uninstall_during_aero_fallback_returns_the_look_and_removes_the_watch_unit() {
        let mut io = ScratchIo::new();
        io.tools.extend(["systemctl".to_string(), "dbus-send".to_string()]);
        assert!(user_install(&io).0.ok());
        let home = io.home();
        let paths = crate::session_guard::GuardPaths::new(format!("{home}/.config"), format!("{home}/.local/state/rexon/session-guard"));
        io.put("home/.config/kwinrc", "[Plugins]\nsmodglowEnabled=true\n", 0o644);
        let plan = crate::session_guard::FallbackPlan { effects: vec!["smodglow".into()], applets: vec![], why: vec![] };
        crate::session_guard::fall_back_to_stock(&paths, &plan, crate::session_guard::FallbackCause::Requested { reason: "x".into() }).unwrap();
        assert!(read(&format!("{home}/.config/kwinrc")).contains("smodglowEnabled=false"));
        let unit = crate::session_watch::unit_path(&format!("{home}/.config"));
        io.put(&format!("home/.config/systemd/user/{}", crate::session_watch::UNIT_NAME), "[Unit]\n", 0o644);
        io.put(&format!("home/.config/plasma-workspace/env/{}", crate::session_guard::ENV_SCRIPT_NAME), "guard", 0o644);

        let (r, _) = un(&io, &user_request(true));
        assert!(r.ok(), "{}", r.message);
        assert!(read(&format!("{home}/.config/kwinrc")).contains("smodglowEnabled=true"), "эффект вернулся");
        assert!(!io.exists(&unit), "юнит сторожа сеанса снят");
        assert!(!paths.env_script().exists(), "раскладка не подменялась — сценарию незачем оставаться");
        let runs = io.runs();
        let sc = |a: &[&str]| runs.iter().any(|r| r.iter().map(String::as_str).eq(a.iter().copied()));
        assert!(sc(&["systemctl", "--user", "disable", crate::session_watch::UNIT_NAME]), "{runs:?}");
        assert!(sc(&["systemctl", "--user", "daemon-reload"]), "{runs:?}");
        assert!(runs.iter().any(|r| r == &crate::session_guard::reconfigure_kwin_argv()), "KWin перечитал настройки: {runs:?}");
    }

    #[test]
    fn user_uninstall_keeping_profile_keeps_settings_and_backups() {
        let io = ScratchIo::new();
        assert!(user_install(&io).0.ok());
        io.put("home/.config/rexon/settings.json", "мои настройки", 0o644);
        io.put("home/.config/rexon/ocs_installed.json", "журнал тем", 0o644);
        io.put("home/.local/share/rexon/backups/plasma-panel/1.json", "копия", 0o644);
        let (r, sink) = un(&io, &user_request(true));
        assert!(r.ok(), "{}", r.message);
        assert_eq!(r.message, inst::uninstall_done_line(true, RU));
        let home = io.home();
        let settings = format!("{home}/.config/rexon/settings.json");
        let journal = format!("{home}/.config/rexon/ocs_installed.json");
        let copy = format!("{home}/.local/share/rexon/backups/plasma-panel/1.json");
        let mut want = vec![settings.clone(), journal.clone(), copy.clone()];
        want.sort();
        assert_eq!(tree(&home), want, "остались настройки и резервная копия, файлов программы нет");
        assert_eq!(read(&settings), "мои настройки");
        assert_eq!(read(&journal), "журнал тем");
        assert_eq!(read(&copy), "копия");
        assert!(!sink.has(&inst::UninstallPart::Profile.progress_line(RU)));
    }

    /// Папки профиля берутся с поправкой на `XDG_CONFIG_HOME` и
    /// `XDG_DATA_HOME`: «сохранить» оставляет именно их, «удалить всё» —
    /// убирает именно их. Литеральный `~/.config/rexon` — тоже НАШ: до
    /// починки 8a45776 настройки писались туда даже при заданном
    /// `XDG_CONFIG_HOME`, поэтому он уходит вместе с XDG-шным. Чужой сосед
    /// рядом в `~/.config` не тронут ни там, ни там.
    #[test]
    fn profile_follows_xdg_dirs_in_both_choices() {
        for keep in [true, false] {
            let mut io = ScratchIo::new();
            io.env.insert("XDG_CONFIG_HOME".into(), format!("{}/xdg-config", io.base));
            io.env.insert("XDG_DATA_HOME".into(), format!("{}/xdg-data", io.base));
            assert!(user_install(&io).0.ok());
            io.put("xdg-config/rexon/settings.json", "мои", 0o644);
            io.put("xdg-data/rexon/backups/plasma-panel/1.json", "копия", 0o644);
            // Настройки от установки, сделанной до починки 8a45776.
            io.put("home/.config/rexon/settings.json", "мои прежние", 0o644);
            io.put("home/.config/not-ours.json", "чужое место", 0o644);
            let config = format!("{}/xdg-config/rexon", io.base);
            let old_config = format!("{}/home/.config/rexon", io.base);
            let backups = format!("{}/xdg-data/rexon/backups", io.base);
            let listed = uninstall_files(&io, &user_request(keep)).removed(false);
            assert!(
                listed.contains(&config) && listed.contains(&old_config) && listed.contains(&backups),
                "{listed:?}"
            );

            let (r, _) = un(&io, &user_request(keep));
            assert!(r.ok(), "{}", r.message);
            assert_eq!(io.exists(&format!("{config}/settings.json")), keep, "настройки, keep={keep}");
            assert_eq!(
                io.exists(&format!("{old_config}/settings.json")),
                keep,
                "прежние настройки, keep={keep}"
            );
            assert_eq!(io.exists(&format!("{backups}/plasma-panel/1.json")), keep, "копии, keep={keep}");
            assert_eq!(read(&format!("{}/home/.config/not-ours.json", io.base)), "чужое место");
            assert!(!io.exists(&io.layout().lib_dir), "программа удалена, keep={keep}");
        }
    }

    /// Данные Rexon у человека: кэш (по XDG и литеральный), настройки,
    /// резервные копии и установленное содержимое, плюс чужие соседи, которых
    /// удаление касаться не должно. Возвращает (что уходит всегда, что
    /// уходит только при «удалить всё», что не наше).
    fn put_user_data(io: &ScratchIo, cache_root: &str, data_root: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
        let b = &io.base;
        let files = [
            (format!("{cache_root}/rexon/youtube/abc.mp4"), true),
            ("home/.cache/rexon/ocs_cache.db".to_string(), true),
            ("home/.config/rexon/settings.json".to_string(), false),
            (format!("{data_root}/rexon/backups/plasma-panel/1.json"), false),
            (format!("{data_root}/rexon/wallpapers/волны/scene.json"), false),
            (format!("{data_root}/rexon/login-themes/тема/Main.qml"), false),
            (format!("{data_root}/rexon/plymouth-themes/p/p.plymouth"), false),
            (format!("{data_root}/rexon/bundles/b.rxn"), false),
            (format!("{data_root}/rexon/extensions/e/metadata.json"), false),
            (format!("{data_root}/rexon/qml_modules/Qcm/qmldir"), false),
            ("home/.local/share/rexon/bin/realesrgan/realesrgan-ncnn-vulkan".to_string(), false),
        ];
        let (mut always, mut full) = (Vec::new(), Vec::new());
        for (rel, is_cache) in files {
            io.put(&rel, "данные", 0o644);
            if is_cache { always.push(format!("{b}/{rel}")) } else { full.push(format!("{b}/{rel}")) }
        }
        let foreign = [
            format!("{cache_root}/other-app/c"),
            "home/.cache/rexon-other/c".to_string(),
            format!("{data_root}/other-app/d"),
            "home/.local/share/applications/other.desktop".to_string(),
            "home/.config/other/o".to_string(),
        ];
        for rel in &foreign {
            io.put(rel, "чужое", 0o644);
        }
        (always, full, foreign.iter().map(|r| format!("{b}/{r}")).collect())
    }

    /// ⭐ Решение 15.09.2026: «сохранить» — кэш удалён, настройки, копии и
    /// установленные обои и темы целы; «удалить всё» — всё удалено, корни
    /// `rexon` в кэше и данных тоже. Чужое не тронуто ни там, ни там.
    #[test]
    fn cache_goes_always_installed_content_only_with_delete_all() {
        for keep in [true, false] {
            let io = ScratchIo::new();
            assert!(user_install(&io).0.ok());
            let (always, full, foreign) = put_user_data(&io, "home/.cache", "home/.local/share");
            let home = io.home();

            let listing = uninstall_files(&io, &user_request(keep));
            use inst::UserDataKind as K;
            let kinds_of = |k: K| -> Vec<String> {
                listing.user_data.iter().filter(|(kind, _)| *kind == k).map(|(_, p)| p.clone()).collect()
            };
            assert_eq!(kinds_of(K::Cache), vec![format!("{home}/.cache/rexon")]);
            assert_eq!(kinds_of(K::Settings), vec![format!("{home}/.config/rexon")]);
            assert_eq!(kinds_of(K::Backups), vec![format!("{home}/.local/share/rexon/backups")]);
            let content: Vec<String> = ["bin", "bundles", "extensions", "login-themes", "plymouth-themes", "qml_modules", "wallpapers"]
                .iter()
                .map(|n| format!("{home}/.local/share/rexon/{n}"))
                .collect();
            assert_eq!(kinds_of(K::Content), content, "содержимое без резервных копий, по имени");
            let removed = listing.removed(keep);
            assert!(removed.contains(&format!("{home}/.cache/rexon")), "кэш уходит при любом выборе");
            assert_eq!(removed.iter().any(|p| p.contains("/.local/share/rexon")), !keep, "{removed:?}");
            assert_eq!(removed.iter().any(|p| p.contains("/.config/rexon")), !keep, "{removed:?}");

            let (r, sink) = un(&io, &user_request(keep));
            assert!(r.ok(), "{}", r.message);
            assert_eq!(r.message, inst::uninstall_done_line(keep, RU));
            for p in &always {
                assert!(!io.exists(p), "кэш остался, keep={keep}: {p}");
            }
            assert!(!io.exists(&format!("{home}/.cache/rexon")), "корень кэша остался");
            for p in &full {
                assert_eq!(io.exists(p), keep, "keep={keep}: {p}");
            }
            for p in &foreign {
                assert_eq!(read(p), "чужое", "задето чужое: {p}");
            }
            assert_eq!(io.exists(&format!("{home}/.local/share/rexon")), keep, "корень данных, keep={keep}");
            assert!(sink.has(&inst::UninstallPart::Cache.progress_line(RU)));
            assert_eq!(sink.has(&inst::UninstallPart::Profile.progress_line(RU)), !keep);
        }
    }

    /// `XDG_CACHE_HOME` и `XDG_DATA_HOME`: корни берутся и по XDG, и
    /// литеральные (часть ядра пишет в `~/.cache/rexon` мимо XDG).
    #[test]
    fn cache_and_content_follow_xdg_and_literal_roots() {
        for keep in [true, false] {
            let mut io = ScratchIo::new();
            io.env.insert("XDG_CACHE_HOME".into(), format!("{}/xdg-cache", io.base));
            io.env.insert("XDG_DATA_HOME".into(), format!("{}/xdg-data", io.base));
            assert!(user_install(&io).0.ok());
            let (always, full, foreign) = put_user_data(&io, "xdg-cache", "xdg-data");
            let listed = uninstall_files(&io, &user_request(keep)).removed(true);
            for root in [format!("{}/xdg-cache/rexon", io.base), format!("{}/.cache/rexon", io.home())] {
                assert!(listed.contains(&root), "{root}: {listed:?}");
            }
            let (r, _) = un(&io, &user_request(keep));
            assert!(r.ok(), "{}", r.message);
            for p in &always {
                assert!(!io.exists(p), "кэш остался, keep={keep}: {p}");
            }
            for p in &full {
                assert_eq!(io.exists(p), keep, "keep={keep}: {p}");
            }
            for p in &foreign {
                assert_eq!(read(p), "чужое", "задето чужое: {p}");
            }
            for root in ["xdg-data/rexon", "home/.local/share/rexon"] {
                assert_eq!(io.exists(&format!("{}/{root}", io.base)), keep, "корень {root}, keep={keep}");
            }
        }
    }

    /// ⛔ Ссылки: на месте кэша, установленного содержимого и всего корня
    /// данных удаляется САМА ссылка; то, на что она указывает, цело. Ссылка
    /// на резервные копии — как прежде, не удаляется и не проходится.
    #[test]
    fn links_in_cache_and_content_are_removed_themselves_never_followed() {
        let io = ScratchIo::new();
        assert!(user_install(&io).0.ok());
        let b = io.base.clone();
        let home = io.home();
        for (rel, text) in [
            ("elsewhere/cache/big.bin", "кэш на другом диске"),
            ("elsewhere/walls/мои.jpg", "обои на другом диске"),
            ("elsewhere/photo.jpg", "фото"),
            ("elsewhere/backups/1.json", "копии на другом диске"),
            ("home/.local/share/rexon/login-themes/t/Main.qml", "тема"),
        ] {
            io.put(rel, text, 0o644);
        }
        let link = |target: &str, at: &str| {
            std::fs::create_dir_all(parent_dir(at)).unwrap();
            // Установка уже создала пустую папку копий — ссылка встаёт на её место.
            let _ = std::fs::remove_dir(at);
            std::os::unix::fs::symlink(format!("{b}/{target}"), at).unwrap()
        };
        let cache = format!("{home}/.cache/rexon");
        let walls = format!("{home}/.local/share/rexon/wallpapers");
        let inner = format!("{home}/.local/share/rexon/login-themes/t/фон.jpg");
        let backups = format!("{home}/.local/share/rexon/backups");
        link("elsewhere/cache", &cache);
        link("elsewhere/walls", &walls);
        link("elsewhere/photo.jpg", &inner);
        link("elsewhere/backups", &backups);

        let listing = uninstall_files(&io, &user_request(false));
        let listed = listing.removed(false);
        assert!(listed.contains(&cache) && listed.contains(&walls), "удалится сама ссылка — назвать: {listed:?}");
        assert!(!listed.contains(&backups), "ссылку на копии не удаляем — не называть: {listed:?}");

        let (r, _) = un(&io, &user_request(false));
        let UninstallOutcome::Removed { kept_links } = &r.outcome else { panic!("{r:?}") };
        assert_eq!(kept_links, &vec![backups.clone()]);
        for gone in [&cache, &walls] {
            assert!(std::fs::symlink_metadata(gone).is_err(), "ссылка {gone} осталась");
            assert!(r.message.contains(&inst::uninstall_removed_link_note(gone, RU)), "{}", r.message);
        }
        assert!(std::fs::symlink_metadata(&inner).is_err(), "каталог с ссылкой внутри удалён");
        assert!(std::fs::symlink_metadata(&backups).unwrap().file_type().is_symlink());
        assert_eq!(read(&format!("{b}/elsewhere/cache/big.bin")), "кэш на другом диске");
        assert_eq!(read(&format!("{b}/elsewhere/walls/мои.jpg")), "обои на другом диске");
        assert_eq!(read(&format!("{b}/elsewhere/photo.jpg")), "фото");
        assert_eq!(read(&format!("{b}/elsewhere/backups/1.json")), "копии на другом диске");
        // Корень данных не пуст (осталась ссылка на копии) — не тронут.
        assert!(io.is_dir(&format!("{home}/.local/share/rexon")));
    }

    /// Весь корень данных — ссылка: удаляется только она, резервные копии и
    /// обои по ту сторону целы и не называются по одному.
    #[test]
    fn linked_data_root_is_unlinked_and_its_backups_are_not_entered() {
        let io = ScratchIo::new();
        assert!(user_install(&io).0.ok());
        let b = io.base.clone();
        io.put("dotfiles/rexon-data/backups/1.json", "копия", 0o644);
        io.put("dotfiles/rexon-data/wallpapers/w.jpg", "обои", 0o644);
        let root = format!("{}/.local/share/rexon", io.home());
        // Установка создала там папку копий — ссылка встаёт на место корня.
        std::fs::remove_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(format!("{b}/dotfiles/rexon-data"), &root).unwrap();

        let listing = uninstall_files(&io, &user_request(false));
        let data: Vec<&String> = listing
            .user_data
            .iter()
            .filter(|(k, _)| matches!(k, inst::UserDataKind::Content | inst::UserDataKind::Backups))
            .map(|(_, p)| p)
            .collect();
        assert_eq!(data, vec![&root], "одна строка — сама ссылка, копии по ту сторону не названы");
        for keep in [true, false] {
            if keep {
                let (r, _) = un(&io, &user_request(true));
                assert!(r.ok() && std::fs::symlink_metadata(&root).is_ok(), "«сохранить» не трогает ссылку");
                continue;
            }
            let (r, _) = un(&io, &user_request(false));
            assert!(r.ok(), "{}", r.message);
            assert!(std::fs::symlink_metadata(&root).is_err(), "ссылка удалена");
            assert_eq!(read(&format!("{b}/dotfiles/rexon-data/backups/1.json")), "копия");
            assert_eq!(read(&format!("{b}/dotfiles/rexon-data/wallpapers/w.jpg")), "обои");
        }
    }

    #[test]
    fn user_uninstall_never_removes_or_follows_links_into_developer_tree() {
        let io = ScratchIo::new();
        io.put("devtree/rexon/app.py", "dev", 0o644);
        io.put("devtree/target/rexon-wallpaper-host", "ELF", 0o755);
        io.put("dotfiles/rexon/settings.json", "мои", 0o644);
        io.put("home/.local/bin/rexon", "#!/bin/sh", 0o755);
        io.put("home/.local/share/applications/rexon.desktop", "[Desktop Entry]", 0o644);
        let l = io.layout();
        let home = io.home();
        let config = format!("{home}/.config/rexon");
        std::fs::create_dir_all(parent_dir(&l.lib_dir)).unwrap();
        std::fs::create_dir_all(parent_dir(&config)).unwrap();
        let link = |target: &str, at: &str| std::os::unix::fs::symlink(format!("{}/{target}", io.base), at).unwrap();
        link("devtree", &l.lib_dir);
        link("devtree/target/rexon-wallpaper-host", &l.wallpaper_host());
        link("dotfiles/rexon", &config);

        let req = user_request(false);
        let listed = uninstall_files(&io, &req).removed(false);
        for k in [&l.lib_dir, &l.wallpaper_host(), &config] {
            assert!(!listed.contains(k), "перечень обещает удалить ссылку {k}: {listed:?}");
        }
        let (r, _) = un(&io, &req);
        let UninstallOutcome::Removed { kept_links } = &r.outcome else { panic!("{r:?}") };
        let mut kept = kept_links.clone();
        kept.sort();
        let mut want = vec![l.lib_dir.clone(), l.wallpaper_host(), config.clone()];
        want.sort();
        assert_eq!(kept, want);
        for k in &want {
            assert!(std::fs::symlink_metadata(k).unwrap().file_type().is_symlink(), "ссылка {k} на месте");
            assert!(r.message.contains(&inst::uninstall_kept_link_note(k, RU)), "{}", r.message);
        }
        assert_eq!(read(&format!("{}/devtree/rexon/app.py", io.base)), "dev");
        assert_eq!(read(&format!("{}/devtree/target/rexon-wallpaper-host", io.base)), "ELF");
        assert_eq!(read(&format!("{}/dotfiles/rexon/settings.json", io.base)), "мои");
        assert!(!io.exists(&l.launcher()), "обычный файл удалён");
    }

    /// ⛔ Живой исполнитель в тестах не создаётся никогда: тест с настоящим
    /// паролем поставил бы Rexon в систему (память: 15.07 тест с подставным
    /// HOME тихо заменил прод-симлинк `/usr/local/lib/rexon`).
    #[test]
    fn tests_never_build_live_io() {
        let whole = include_str!("installer_exec.rs");
        let tests = whole.split_once("#[cfg(test)]").map(|(_, t)| t).unwrap();
        for forbidden in [
            ["Live", "InstallIo::", "new"].concat(),
            ["Live", "InstallIo::", "default"].concat(),
            ["Live", "DeOpsIo"].concat(),
            ["Live", "SystemIo"].concat(),
        ] {
            assert!(!tests.contains(&forbidden), "{forbidden}");
        }
    }
}
