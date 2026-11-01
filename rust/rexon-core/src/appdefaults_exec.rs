//! Живой исполнитель маршрута `--apply-defaults` — приложения по умолчанию
//! при входе в сеанс.
//!
//! ЗАЧЕМ. Решения этого маршрута давно на Rust: таблицы «окружение →
//! программы», разбор `$XDG_CURRENT_DESKTOP`, текст per-DE `mimeapps.list`,
//! DE-специфичные команды терминала и весь поток проверки комплектности
//! ([`crate::appdefaults`], [`crate::compliance`]). Но ДЕЙСТВИЯ не было
//! ни одного: порядок шагов и запись файлов держал Python
//! (`rexon/app.py:625` → `DEAppSwitcher` + `ComplianceCheck`). Из-за этого
//! автозапуск при входе в систему обязан был поднимать интерпретатор с
//! колесом моста, а Rust-процессу маршрут был недоступен вовсе.
//!
//! Разделение здесь такое же, как у соседних исполнителей — `wallpaper_exec`,
//! `splash_exec`, `ext_exec`, `imp_live`: **ядро решает, исполнитель делает**.
//!   * ЧТО делать — [`crate::appdefaults`] (исход `generate_profile`, план
//!     `apply_de_specific`, argv `xdg-mime`/`busctl`, тексты автозапуска) и
//!     [`crate::compliance`] (весь поток проверок);
//!   * ЧЕМ делать — трейт [`AppDefaultsIo`]: каждый метод ровно один побочный
//!     вызов или наблюдение. В продукте его исполняет [`LiveAppDefaultsIo`]
//!     (настоящие ФС и подпроцессы), в сверках — подставной исполнитель, и
//!     живая машина при этом не меняется вовсе;
//!   * ПОРЯДОК шагов — [`DeAppSwitcher`] и [`apply_defaults_route`].
//!
//! Проверке комплектности исполнитель нужен свой ([`LiveComplianceIo`],
//! реализация [`crate::compliance::ComplianceIo`]): до сих пор он жил только
//! в мосте `rexon-py` поверх Python-объекта, и вкладка «Настройки» на Rust
//! честно писала, что запустить проверку не может. Теперь исполнитель один
//! на продукт и лежит рядом с маршрутом, который его и заводит; кнопка
//! «Проверить и доустановить» идёт через `system_de_exec::complete_de`.
//!
//! ⭐ ЧТО ПОЧИНЕНО ПРИ ПЕРЕНОСЕ (правило #16 — дефекты оригинала не
//! консервируем):
//!
//!   1. **Автозапуск больше не указывает в пустоту.** Оригинал, не найдя
//!      `rexon` в `PATH`, писал в автозапуск путь
//!      `/usr/local/lib/rexon/rexon_apply_defaults.sh` и пытался создать там
//!      bash-обёртку. Обычный пользовательский процесс в `/usr/local/lib`
//!      писать не может: `_create_apply_script` ловил ошибку, писал строку в
//!      журнал — и автозапуск оставался с `Exec=` на НЕСУЩЕСТВУЮЩИЙ файл.
//!      Дефолты приложений после этого не применялись никогда, а узнать об
//!      этом было неоткуда: следующий вход в сеанс молча ничего не делал.
//!      Здесь запасной путь — САМ работающий бинарь Rexon
//!      ([`AppDefaultsIo::self_exe`]): он и обрабатывает `--apply-defaults`,
//!      посредник не нужен. Не нашлось и его — автозапуск НЕ ставится и
//!      говорит почему (правило #13), а не рапортует успех.
//!   2. **Пропуск окружения перестал называться созданием файла.** Оригинал
//!      в `apply_for_current_de` печатал «📝 Создан kde-mimeapps.list» на
//!      ЛЮБОМ успехе `generate_profile` — включая исход «окружение не
//!      установлено, пропуск», когда файла не появлялось. Исход теперь
//!      различим по типу ([`GenerateOutcome`]), и в лог идёт то, что было на
//!      самом деле.
//!   3. **Неудачная запись `helpers.rc` больше не проглатывается.** У XFCE
//!      оригинал писал ошибку записи в отладочный журнал и возвращал логи
//!      так, будто настройка применилась. Теперь неудача — строка в том же
//!      логе, который человек видит при входе.
//!   4. **`~/.local/share/dbus-1/services` уважает `$XDG_DATA_HOME`.**
//!      В оригинале путь override'а FileManager1 раскрывался помощником,
//!      который знает обе XDG-ветки, но в самом модуле лежал константой,
//!      посчитанной на импорте, — то есть до того, как окружение процесса
//!      могло стать известным. Здесь путь строится от окружения запуска.
//!   5. **«Открыть содержащую папку» перестало зависеть от того, кто успел
//!      первым.** Оригинал считал, что дефолты — это mimeapps, и на этом
//!      останавливался. Но кнопка «показать файл в папке» идёт по имени
//!      `org.freedesktop.FileManager1` на пользовательской шине, и пока имя
//!      держит ЖИВОЙ чужой демон, D-Bus не смотрит ни на один `.service` —
//!      ни вендорный, ни наш override. Пойманный случай (09.09.2026, живая
//!      машина владельца): `xdg-mime query default inode/directory` →
//!      `org.kde.dolphin.desktop`, а папки открывал Thunar, поднятый KDE из
//!      сохранённой сессии (`ksmserverrc`, `program1=Thunar`,
//!      `restartCommand1=Thunar,…,--daemon`). Маршрут теперь проверяет слот
//!      при каждом входе и возвращает его назначенному менеджеру
//!      ([`DeAppSwitcher::ensure_filemanager1`]). Заодно чинится и прежняя
//!      попытка выгнать владельца: она сравнивала `/proc/<pid>/comm` со
//!      строчными короткими именами, а у Thunar `comm` — `Thunar`, так что
//!      на нём не срабатывала никогда.
//!
//! Bash-обёртки автозапуска на python3 больше нет нигде: Python-фасад ушёл
//! в архив (02.10.2026), вместе с ним из ядра снят и её текст
//! (`apply_script_content`) — см. пункт 1.

use std::path::Path;

use crate::appdefaults::{self as ad, ProfileOutcome};
use crate::i18n::{tr, trf};
use crate::compliance::{self as comp, ComplianceIo, RunResult, ScanEntry};
use crate::system::{DistroDetector, PkgFamily};
use crate::system_plan;
use crate::utils::{self, RunOutcome, RunOutput, RunSpec};

// ============================================================================
// Трейт исполнителя
// ============================================================================

/// Итог `safe_run(argv, capture_output=True)`: `ok` = код возврата 0.
/// `None` у [`AppDefaultsIo::run`] — команда не запустилась либо не уложилась
/// в таймаут (тот же безопасный дефолт, что `safe_run` → `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOut {
    pub ok: bool,
    pub stdout: String,
}

/// Каталог vendor-файлов D-Bus активации: оттуда берётся текст сервиса
/// FileManager1, который перекрывается пользовательской копией.
pub const VENDOR_DBUS_SERVICES: &str = "/usr/share/dbus-1/services";

/// Каталоги, где ищется `.desktop` — порт `_DESKTOP_DIRS` (порядок важен:
/// пользовательский слой перекрывает системный, flatpak идёт последним).
const DESKTOP_DIRS: &[&str] = &[
    "~/.local/share/applications",
    "/usr/share/applications",
    "/usr/local/share/applications",
    "/var/lib/flatpak/exports/share/applications",
    "~/.local/share/flatpak/exports/share/applications",
];

/// Побочные действия и наблюдения маршрута — ровно те, что делал Python.
///
/// Методы берут `&self`, а не `&mut self` (как у соседнего
/// [`ComplianceIo`]), намеренно: решающий слой [`crate::appdefaults`]
/// принимает наблюдения замыканиями `&dyn Fn(&str) -> bool`, и с
/// `&mut self` каждый такой вызов упирался бы в двойное заимствование
/// исполнителя. Подставным исполнителям это ничего не стоит — счётчики
/// и журнал они держат в `Cell`/`RefCell`.
pub trait AppDefaultsIo {
    /// `os.path.exists`.
    fn exists(&self, path: &str) -> bool;
    /// `safe_check_command(name)` — есть ли программа (с учётом синонимов).
    fn which(&self, name: &str) -> bool;
    /// `shutil.which(name)` — путь к программе.
    fn which_path(&self, name: &str) -> Option<String>;
    /// Чтение текстового файла; `None` — не прочли (аналог `except OSError`).
    fn read_text(&self, path: &str) -> Option<String>;
    /// Запись текстового файла целиком.
    fn write_text(&self, path: &str, content: &str) -> Result<(), String>;
    /// `os.makedirs(path, exist_ok=True)`.
    fn makedirs(&self, path: &str) -> Result<(), String>;
    /// Имена файлов каталога (несуществующий каталог — пусто).
    fn list_dir(&self, dir: &str) -> Vec<String>;
    /// `safe_run(argv, capture_output=True)`.
    fn run(&self, argv: &[String]) -> Option<RunOut>;
    /// Содержимое `/proc/<pid>/comm` без перевода строки.
    fn proc_comm(&self, pid: i64) -> Option<String>;
    /// PID процессов, у которых есть ОКНА на экране. `None` — узнать не
    /// удалось; это НЕ «окон нет», и решение тогда принимается осторожно
    /// (см. [`crate::appdefaults::fm1_verdict`]).
    fn window_pids(&self) -> Option<Vec<i64>>;
    /// `os.kill(pid, SIGTERM)`.
    fn terminate(&self, pid: i64);
    /// Путь к работающему бинарю Rexon (`/proc/self/exe`).
    fn self_exe(&self) -> Option<String>;
    /// Какой вход продукта сейчас выбран — у живого исполнителя это
    /// [`crate::installer::launch_entry_live`], то же правило, что у ярлыка.
    fn launch_entry(&self) -> crate::installer::LaunchEntry;
    /// Строка автозапуска в конфигах композиторов, которые `.desktop` не
    /// исполняют (`crate::compositor_autostart`): поставить для записи
    /// `desktop` и вернуть окончательный текст записи (с `NotShowIn=`) и
    /// отказ для текущего сеанса. По умолчанию — ничего не трогать
    /// (подставные исполнители сверок живых конфигов не касаются).
    fn login_lines(&self, desktop: &str) -> (String, Option<String>) {
        (desktop.to_string(), None)
    }
}

// ============================================================================
// Окружение запуска
// ============================================================================

/// Значения окружения, от которых зависит маршрут. Читаются ОДИН раз на
/// входе ([`AppDefaultsEnv::from_process`]) и дальше едут значениями: так
/// весь исполнитель прогоняется на подставном `$XDG_CONFIG_HOME`, не трогая
/// живой сеанс.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppDefaultsEnv {
    pub home: String,
    pub xdg_config_home: Option<String>,
    pub xdg_data_home: Option<String>,
    pub xdg_current_desktop: String,
    pub desktop_session: String,
    pub has_wayland: bool,
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

impl AppDefaultsEnv {
    /// Окружение текущего процесса.
    pub fn from_process() -> Self {
        AppDefaultsEnv {
            home: std::env::var("HOME").unwrap_or_default(),
            xdg_config_home: env_nonempty("XDG_CONFIG_HOME"),
            xdg_data_home: env_nonempty("XDG_DATA_HOME"),
            xdg_current_desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            desktop_session: std::env::var("DESKTOP_SESSION").unwrap_or_default(),
            has_wayland: env_nonempty("WAYLAND_DISPLAY").is_some(),
        }
    }
}

/// Склейка пути (`os.path.join` для наших случаев).
fn join(a: &str, b: &str) -> String {
    format!("{}/{}", a.trim_end_matches('/'), b.trim_start_matches('/'))
}

// ============================================================================
// Исход генерации профиля
// ============================================================================

/// Что вышло из `generate_profile` — порт его пары `(ok, message)`, но с
/// РАЗЛИЧИМЫМ пропуском: оригинал отдавал «пропущено» и «записано» одним и
/// тем же `True`, и вход в сеанс докладывал о создании файла, которого не
/// появилось (см. пункт 2 в шапке модуля).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerateOutcome {
    /// Файл записан; поле — полный путь.
    Wrote(String),
    /// Окружение не установлено — файл не нужен.
    Skipped(String),
    /// Не вышло: нет ни одного приложения либо ошибка записи.
    Failed(String),
}

impl GenerateOutcome {
    /// Пара `(ok, message)` оригинала — для мест, где различие не нужно.
    pub fn ok_msg(&self) -> (bool, String) {
        match self {
            GenerateOutcome::Wrote(p) => (true, p.clone()),
            GenerateOutcome::Skipped(m) => (true, m.clone()),
            GenerateOutcome::Failed(m) => (false, m.clone()),
        }
    }
}

// ============================================================================
// DEAppSwitcher
// ============================================================================

/// Порт класса `DEAppSwitcher` — переключение приложений по умолчанию при
/// смене окружения.
///
/// Каталога `~/.config/rexon` здесь, в отличие от оригинала, никто не
/// создаёт: конструктор Python делал это на каждом создании объекта, а сам
/// класс в тот каталог ни разу не писал.
pub struct DeAppSwitcher<'a, IO: AppDefaultsIo + ?Sized> {
    io: &'a IO,
    env: AppDefaultsEnv,
    cfg_dir: String,
}

impl<'a, IO: AppDefaultsIo + ?Sized> DeAppSwitcher<'a, IO> {
    pub fn new(io: &'a IO, env: AppDefaultsEnv) -> Self {
        let cfg_dir = utils::config_home(env.xdg_config_home.as_deref(), &env.home);
        DeAppSwitcher { io, env, cfg_dir }
    }

    /// Каталог пользовательских конфигов сеанса (`$XDG_CONFIG_HOME`).
    pub fn config_dir(&self) -> &str {
        &self.cfg_dir
    }

    fn expand(&self, path: &str) -> String {
        utils::expand_user_xdg(
            path,
            &self.env.home,
            self.env.xdg_config_home.as_deref(),
            self.env.xdg_data_home.as_deref(),
        )
    }

    /// `_desktop_exists` — есть ли `.desktop` хоть в одном из каталогов.
    pub fn desktop_exists(&self, desktop_id: &str) -> bool {
        DESKTOP_DIRS
            .iter()
            .any(|d| self.io.exists(&join(&self.expand(d), desktop_id)))
    }

    // ── 1. Генерация per-DE mimeapps.list ────────────────────────────────

    /// Порт `generate_profile`: ядро строит исход, здесь — запись.
    pub fn generate_profile(&self, de_id: &str) -> GenerateOutcome {
        let outcome = ad::generate_profile(de_id, &|p| self.desktop_exists(p));
        match outcome {
            ProfileOutcome::Skip { message } => GenerateOutcome::Skipped(message),
            ProfileOutcome::NoApps { message } => GenerateOutcome::Failed(message),
            ProfileOutcome::Write { filename, content, .. } => {
                let filepath = join(&self.cfg_dir, &filename);
                match self.io.write_text(&filepath, &content) {
                    Ok(()) => GenerateOutcome::Wrote(filepath),
                    Err(e) => {
                        GenerateOutcome::Failed(trf("Couldn't write %1: %2", &[&filepath, &e]))
                    }
                }
            }
        }
    }

    /// Порт `generate_all_profiles`: `{de_id: сообщение}` + строки лога.
    pub fn generate_all_profiles(
        &self,
        installed_des: Option<&[String]>,
        logs: &mut Vec<String>,
    ) -> Vec<(String, String)> {
        let targets: Vec<String> = match installed_des {
            Some(list) => list.to_vec(),
            None => self.installed_des(),
        };
        let mut results = Vec::new();
        for de_id in targets {
            let (ok, msg) = self.generate_profile(&de_id).ok_msg();
            let status = if ok { "✅" } else { "⚠️" };
            logs.push(format!("  {status} {de_id}: {msg}"));
            results.push((de_id, msg));
        }
        results
    }

    // ── 2. DE-специфичные настройки (терминал и т.д.) ────────────────────

    /// Порт `apply_de_specific`: ядро строит план (argv + текст
    /// `helpers.rc` + логи), здесь — запуск команд и запись.
    pub fn apply_de_specific(&self, de_id: &str) -> Vec<String> {
        // XFCE читается ТЕМ ЖЕ раскрытием пути, каким ниже пишется: иначе
        // при нестандартном `$XDG_CONFIG_HOME` к старому файлу из `~/.config`
        // подмешивались бы новые строки, а результат ложился в другой
        // каталог — настройка молча не доезжала.
        let helpers_path = self.expand("~/.config/xfce4/helpers.rc");
        let xfce_existing = if de_id == "xfce" && self.io.exists(&helpers_path) {
            self.io.read_text(&helpers_path)
        } else {
            None
        };

        let plan = ad::apply_de_specific(
            de_id,
            &|p| self.desktop_exists(p),
            &|c| self.io.which(c),
            xfce_existing.as_deref(),
        );
        for cmd in &plan.commands {
            self.io.run(cmd);
        }
        let mut logs = plan.logs;
        if let Some(text) = plan.helpers_rc.as_deref() {
            let dir = self.expand("~/.config/xfce4");
            let written = self
                .io
                .makedirs(&dir)
                .and_then(|()| self.io.write_text(&helpers_path, text));
            if let Err(e) = written {
                // Оригинал прятал это в отладочный журнал и возвращал логи
                // так, будто настройка применилась (правило #13).
                logs.push(trf("⚠️ XFCE: couldn't write %1: %2", &[&helpers_path, &e]));
            }
        }
        // Правки чужих конфигов по месту (LXQt `lxqt-qtxdg.conf`): те же
        // шаги общей таблицы терминала, что у вкладки «Настройки».
        for step in &plan.file_steps {
            if let crate::defaults::TermStep::SetIni { path, section, key, value } = step {
                let path = self.expand(path);
                let existing = self.io.read_text(&path);
                let text = crate::defaults::set_ini_value(existing.as_deref(), section, key, value);
                let dir = Path::new(&path).parent().map(|d| d.to_string_lossy().into_owned());
                let written = match dir {
                    Some(d) => self.io.makedirs(&d),
                    None => Ok(()),
                }
                .and_then(|()| self.io.write_text(&path, &text));
                if let Err(e) = written {
                    logs.push(format!("⚠️ {}", trf("Couldn't write %1: %2", &[&path, &e])));
                }
            }
        }
        logs
    }

    // ── 3. Полное применение для текущего окружения ──────────────────────

    /// Порт `apply_for_current_de` — то, что зовётся при входе в сеанс.
    pub fn apply_for_current_de(&self) -> (bool, Vec<String>) {
        let Some(de_id) = ad::detect_current_de(
            &self.env.xdg_current_desktop,
            &self.env.desktop_session,
            self.env.has_wayland,
        ) else {
            return (false, vec![tr("⚠️ Couldn't detect the current desktop")]);
        };

        // TRANSLATORS: %1 is a desktop environment id (kde, gnome…).
        let mut logs = vec![trf("🖥️ Current desktop: %1", &[&de_id])];

        // 1. Убедиться, что per-DE mimeapps.list существует. Имя есть у
        //    ЛЮБОГО окружения (незнакомое даёт `<id>-mimeapps.list`), поэтому
        //    ветки «имени нет» здесь, в отличие от оригинала, не бывает.
        let filename = ad::resolve_mimeapps_filename(&de_id);
        let filepath = join(&self.cfg_dir, &filename);
        if self.io.exists(&filepath) {
            logs.push(trf("✅ %1 already exists", &[&filename]));
        } else {
            match self.generate_profile(&de_id) {
                GenerateOutcome::Wrote(_) => logs.push(trf("📝 Created %1", &[&filename])),
                GenerateOutcome::Skipped(m) => logs.push(format!("ℹ️ {m}")),
                GenerateOutcome::Failed(m) => logs.push(format!("⚠️ {m}")),
            }
        }

        // 2. DE-специфичные настройки.
        logs.extend(self.apply_de_specific(&de_id));

        // 3. Слот «Показать в папке». Идёт ПОСЛЕ mimeapps намеренно: вопрос
        //    решается не там, и порядок делает это видимым в журнале —
        //    сначала «дефолты записаны», потом «а имя на шине держит вот кто».
        logs.extend(self.ensure_filemanager1(&de_id));

        (true, logs)
    }

    // ── 4. При переключении окружения через Rexon ────────────────────────

    /// Порт `on_switch_de`: профиль целевого окружения + его настройки +
    /// автозапуск.
    pub fn on_switch_de(&self, target_de: &str, logs: &mut Vec<String>) -> bool {
        logs.push(trf("🔄 Switching default apps to %1…", &[&target_de]));

        let (ok, msg) = self.generate_profile(target_de).ok_msg();
        let status = if ok { "✅" } else { "⚠️" };
        logs.push(format!("  {status} {msg}"));

        for line in self.apply_de_specific(target_de) {
            logs.push(format!("  {line}"));
        }
        // Слот «Показать в папке» — часть тех же дефолтов (правило #12: одно
        // место решает, а не «на входе чиним, при переключении нет»).
        for line in self.ensure_filemanager1(target_de) {
            logs.push(format!("  {line}"));
        }

        let (_, notes) = self.install_autostart();
        logs.extend(notes);
        ok
    }

    // ── 5. Автозапуск ────────────────────────────────────────────────────

    /// Порт `install_autostart`: `.desktop` в автозапуске сеанса.
    ///
    /// Чем запускать — решает вход продукта ([`ad::autostart_exec_for`]):
    /// Rust-вход — бинарь окна; Python-вход (откат) — `rexon` из `PATH`,
    /// иначе САМ работающий бинарь. Пути в `/usr/local/lib` здесь нет
    /// вовсе — см. пункт 1 в шапке модуля.
    pub fn install_autostart(&self) -> (bool, Vec<String>) {
        let mut notes = Vec::new();
        let entry = self.io.launch_entry();
        let exec_path = match entry {
            crate::installer::LaunchEntry::MainWindow(_) => ad::autostart_exec_for(&entry, None, None),
            crate::installer::LaunchEntry::Python => ad::autostart_exec_for(
                &entry,
                self.io.which_path(crate::installer::PYTHON_ENTRY_CMD).as_deref(),
                self.io.self_exe().as_deref(),
            ),
        };
        let Some(exec_path) = exec_path else {
            notes.push(tr(
                "⚠️ Default apps autostart not set up: couldn't find how to launch Rexon \
                 (no `rexon` in PATH and its own binary path is unknown)",
            ));
            return (false, notes);
        };
        let autostart_dir = join(&self.cfg_dir, "autostart");
        let autostart_path = join(&autostart_dir, "rexon-appdefaults.desktop");
        let content = ad::autostart_desktop_content(&exec_path);
        let (content, refusal) = self.io.login_lines(&content);
        let written = self
            .io
            .makedirs(&autostart_dir)
            .and_then(|()| self.io.write_text(&autostart_path, &content));
        match written {
            Ok(()) => match refusal {
                // Запись легла, но в этом сеансе её не исполнят.
                Some(msg) => {
                    notes.push(format!("⚠️ {msg}"));
                    (false, notes)
                }
                None => {
                    notes.push(trf("✅ Autostart set up: %1", &[&autostart_path]));
                    (true, notes)
                }
            },
            Err(e) => {
                notes.push(trf("⚠️ Autostart not set up: %1: %2", &[&autostart_path, &e]));
                (false, notes)
            }
        }
    }

    // ── 6. Информация ────────────────────────────────────────────────────

    /// Порт `get_de_apps_info` — `[(категория, .desktop | None)]`.
    pub fn de_apps_info(&self, de_id: &str) -> Vec<(String, Option<String>)> {
        ad::de_apps_info(de_id, &|p| self.desktop_exists(p))
    }

    /// Порт `get_installed_des`.
    pub fn installed_des(&self) -> Vec<String> {
        ad::installed_des(&|p| self.desktop_exists(p))
    }

    // ── 7. Файловый менеджер по умолчанию ────────────────────────────────

    /// Каталог пользовательских файлов D-Bus активации.
    fn user_dbus_services(&self) -> String {
        self.expand("~/.local/share/dbus-1/services")
    }

    /// Порт `_override_filemanager1`: перекрыть D-Bus активацию
    /// `org.freedesktop.FileManager1` пользовательской копией.
    ///
    /// «Показать в папке» идёт НЕ через mimeapps, а через это имя на шине;
    /// системным файлом на Arch владеет пакет nautilus, и без override'а
    /// GNOME Files перехватывает такие вызовы в любом окружении.
    ///
    /// ⚠️ Одного override'а МАЛО: он решает, кого ЗАПУСТИТЬ по имени, и не
    /// действует вовсе, пока имя держит живой процесс. Второй половиной
    /// занимается [`Self::reclaim_filemanager1`].
    pub fn override_filemanager1(&self, binary: &str, notes: &mut Vec<String>) -> bool {
        let mut vendor_txt: Option<String> = None;
        for name in self.io.list_dir(VENDOR_DBUS_SERVICES) {
            if !name.ends_with(".service") {
                continue;
            }
            let Some(txt) = self.io.read_text(&join(VENDOR_DBUS_SERVICES, &name)) else {
                continue;
            };
            if ad::fm1_matches(&txt, binary) {
                vendor_txt = Some(txt);
                break;
            }
        }
        let Some(vendor_txt) = vendor_txt else {
            notes.push(trf(
                "FileManager1: %1 has no vendor service file — not installing an override",
                &[&binary],
            ));
            return false;
        };

        let dir = self.user_dbus_services();
        let target = join(&dir, &format!("{}.service", ad::FM1_NAME));
        if let Err(e) = self
            .io
            .makedirs(&dir)
            .and_then(|()| self.io.write_text(&target, &vendor_txt))
        {
            notes.push(trf("FileManager1: couldn't write %1: %2", &[&target, &e]));
            return false;
        }

        self.io.run(&ad::busctl_reload_cmd());
        true
    }

    /// Кто СЕЙЧАС держит имя `org.freedesktop.FileManager1` — и есть ли у
    /// него окна.
    ///
    /// Один вопрос шине (`busctl status`) отдаёт сразу всё нужное: `PID`,
    /// `Comm`, `Exe` и `CommandLine`. Прежний разбор брал оттуда ОДИН PID и
    /// добирал имя из `/proc/<pid>/comm` — чтения остались запасным ходом на
    /// случай скупого вывода старой шины.
    fn fm1_owner(&self) -> Option<ad::Fm1Owner> {
        let res = self.io.run(&ad::busctl_status_cmd(ad::FM1_NAME))?;
        if !res.ok {
            return None;
        }
        let mut facts = ad::busctl_owner(&res.stdout)?;
        if facts.comm.is_empty() && facts.exe.is_empty() {
            facts.comm = self.io.proc_comm(facts.pid).unwrap_or_default();
        }
        let has_windows = self.io.window_pids().map(|pids| pids.contains(&facts.pid));
        Some(ad::Fm1Owner { facts, has_windows })
    }

    /// Убрать менеджер из сохранённой сессии KDE, чтобы при следующем входе
    /// он не поднялся демоном снова и не отобрал имя обратно.
    ///
    /// Файла нет (не KDE) — молча ничего не делаем: решение о содержимом
    /// принимает ядро ([`ad::ksmserver_forget`]), здесь только чтение и
    /// запись.
    fn forget_in_kde_session(&self, short: &str, notes: &mut Vec<String>) {
        let path = join(&self.cfg_dir, ad::KSMSERVERRC_NAME);
        let Some(text) = self.io.read_text(&path) else {
            return;
        };
        let Some(updated) = ad::ksmserver_forget(&text, short) else {
            return;
        };
        match self.io.write_text(&path, &updated) {
            Ok(()) => notes.push(format!(
                "  {}",
                trf("✅ %1 removed from the saved KDE session — it won't come back at the next login", &[&short])
            )),
            Err(e) => notes.push(format!("  {}", trf("⚠️ couldn't rewrite %1: %2", &[&path, &e]))),
        }
    }

    /// Сказать вслух, кому досталось имя после лечения.
    fn confirm_filemanager1(&self, desired_short: &str, notes: &mut Vec<String>) {
        match ad::fm1_verdict(self.fm1_owner().as_ref(), desired_short) {
            ad::Fm1Verdict::Free => notes.push(format!(
                "  {}",
                trf("✅ the name is free — “Show in Folder” will open %1", &[&desired_short])
            )),
            ad::Fm1Verdict::Owned { short, .. } => {
                notes.push(format!("  {}", trf("✅ the name passed to %1", &[&short])))
            }
            ad::Fm1Verdict::Busy { short, pid }
            | ad::Fm1Verdict::Evict { short, pid, .. } => notes.push(format!(
                "  {}",
                trf(
                    "⚠️ %1 (pid %2) still holds the name: until you log out, “Show in Folder” will open it",
                    &[&short, &pid]
                )
            )),
            ad::Fm1Verdict::Unknown { process, pid } => notes.push(format!(
                "  {}",
                trf("⚠️ %1 (pid %2) holds the name", &[&process, &pid])
            )),
        }
    }

    /// Вернуть слот `org.freedesktop.FileManager1` назначенному менеджеру.
    ///
    /// ⛔ Гасится ТОЛЬКО чужой демон и ТОЛЬКО тот процесс, чей PID назвала
    /// шина: ни поиска по имени, ни `pkill`. Владелец с окнами не трогается
    /// вовсе — у человека там открытая работа.
    ///
    /// Возвращает `true`, если слот пришлось отбирать.
    pub fn reclaim_filemanager1(
        &self,
        desired_short: &str,
        notes: &mut Vec<String>,
    ) -> bool {
        match ad::fm1_verdict(self.fm1_owner().as_ref(), desired_short) {
            ad::Fm1Verdict::Free => {
                notes.push(tr(
                    "✅ “Show in Folder”: the name org.freedesktop.FileManager1 is free — the chosen file manager will get it",
                ));
                false
            }
            ad::Fm1Verdict::Owned { short, .. } => {
                notes.push(trf(
                    "✅ “Show in Folder”: %1 holds the name — it's the chosen default",
                    &[&short],
                ));
                false
            }
            ad::Fm1Verdict::Unknown { process, pid } => {
                notes.push(trf(
                    "ℹ️ “Show in Folder”: %1 (pid %2) holds the name — it isn't a file manager, leaving it alone",
                    &[&process, &pid],
                ));
                false
            }
            ad::Fm1Verdict::Busy { short, pid } => {
                notes.push(trf(
                    "⚠️ %1 (pid %2) took over “Show in Folder”, but it has open windows — not closing it so your work isn't lost",
                    &[&short, &pid],
                ));
                false
            }
            ad::Fm1Verdict::Evict { short, pid, quit } => {
                notes.push(trf(
                    "⚠️ %1 (pid %2, service mode) took over “Show in Folder” instead of %3 — taking it back",
                    &[&short, &pid, &desired_short],
                ));
                let mut stopped = false;
                if !quit.is_empty() {
                    stopped = self.io.run(&quit).map(|r| r.ok).unwrap_or(false);
                    if stopped {
                        notes.push(format!(
                            "  {}",
                            trf("✅ %1 quit normally: %2", &[&short, &quit.join(" ")])
                        ));
                    } else {
                        notes.push(format!(
                            "  {}",
                            trf("⚠️ normal quit didn't work (%1), stopping the process itself", &[&quit.join(" ")])
                        ));
                    }
                }
                if !stopped {
                    self.io.terminate(pid);
                    notes.push(format!(
                        "  {}",
                        trf("✅ %1 (pid %2) was sent the stop signal", &[&short, &pid])
                    ));
                }
                self.forget_in_kde_session(&short, notes);
                self.io.run(&ad::busctl_reload_cmd());
                self.confirm_filemanager1(desired_short, notes);
                true
            }
        }
    }

    /// Короткое имя файлового менеджера, который Rexon считает умолчанием
    /// для `inode/directory`: сперва то, что реально записано в mime, иначе
    /// родной менеджер окружения.
    pub fn desired_file_manager(&self, de_id: &str) -> String {
        if let Some(res) = self.io.run(&ad::xdg_mime_query_cmd()) {
            if res.ok {
                let desktop = res.stdout.trim().lines().next().unwrap_or("").trim();
                if desktop.ends_with(".desktop") {
                    return ad::fm_short_name(desktop, desktop);
                }
            }
        }
        ad::default_file_manager_for_de(Some(de_id), &|c| self.io.which(c))
    }

    /// Полная проверка «Показать в папке» для окружения: наш override
    /// D-Bus-активации на месте, и имя на шине принадлежит тому менеджеру,
    /// который назначен умолчанием.
    ///
    /// Зовётся при каждом входе в сеанс: верных `mimeapps.list` для этой
    /// кнопки НЕ ДОСТАТОЧНО (почему — см. раздел про слот FileManager1 в
    /// [`crate::appdefaults`]).
    pub fn ensure_filemanager1(&self, de_id: &str) -> Vec<String> {
        let mut notes = Vec::new();
        let desired = self.desired_file_manager(de_id);
        self.override_filemanager1(&desired, &mut notes);
        self.reclaim_filemanager1(&desired, &mut notes);
        notes
    }

    /// Порт `set_file_manager_default`: `fm_id` — короткое имя из настроек
    /// («dolphin», «nemo») либо готовый `.desktop`-id.
    pub fn set_file_manager_default(
        &self,
        fm_id: &str,
        de: Option<&str>,
    ) -> (bool, Vec<String>) {
        let mut notes = Vec::new();
        let Some(desktop) = ad::fm_default_target(fm_id, &|p| self.desktop_exists(p))
        else {
            notes.push(trf(
                "⚠️ File manager %1 isn't installed: its .desktop file wasn't found",
                &[&fm_id],
            ));
            return (false, notes);
        };
        let res = self.io.run(&ad::xdg_mime_default_cmd(&desktop));
        if !res.map(|r| r.ok).unwrap_or(false) {
            notes.push(trf("⚠️ xdg-mime default %1 failed", &[&desktop]));
            return (false, notes);
        }
        let short = ad::fm_short_name(fm_id, &desktop);
        self.override_filemanager1(&short, &mut notes);
        self.reclaim_filemanager1(&short, &mut notes);

        // Проверка: что реально записалось.
        let chk = self.io.run(&ad::xdg_mime_query_cmd());
        let applied = chk
            .map(|r| r.ok && r.stdout.contains(&desktop))
            .unwrap_or(false);
        notes.push(format!(
            "set_file_manager_default({fm_id}, de={}) → {desktop}, applied={applied}",
            de.unwrap_or("—")
        ));
        (applied, notes)
    }
}

// ============================================================================
// Маршрут `--apply-defaults` целиком
// ============================================================================

/// Что маршруту нужно знать о машине для проверки комплектности. Добывается
/// снаружи ([`ComplianceEnv::from_live_machine`]), чтобы сверки гоняли
/// маршрут на подставных значениях.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplianceEnv {
    pub pkg_family: PkgFamily,
    pub pkg_helper_available: bool,
    pub stage_helper_available: bool,
}

impl ComplianceEnv {
    pub fn from_live_machine() -> Self {
        Self::probe(
            DistroDetector::detect_live().family_live(),
            utils::which("pkexec", path_env().as_deref()).is_some(),
            &system_plan::is_exec_file,
            &|p| Path::new(p).exists(),
        )
    }

    /// Та же диагностика на поданных пробах: `is_exec` выбирает раскладку
    /// помощников (пакет `/usr/lib` или ручная установка `/usr/local/lib`),
    /// `exists` отвечает про файлы. Сверки гоняют ОБЕ раскладки, не трогая
    /// живую машину.
    pub fn probe(
        pkg_family: PkgFamily,
        has_pkexec: bool,
        is_exec: &dyn Fn(&str) -> bool,
        exists: &dyn Fn(&str) -> bool,
    ) -> Self {
        ComplianceEnv {
            pkg_family,
            pkg_helper_available: helper_available(
                system_plan::PKG_HELPER_NAME,
                system_plan::PKG_POLICY_PATH,
                has_pkexec,
                is_exec,
                exists,
            ),
            stage_helper_available: helper_available(
                system_plan::STAGE_HELPER_NAME,
                system_plan::STAGE_POLICY_PATH,
                has_pkexec,
                is_exec,
                exists,
            ),
        }
    }
}

/// Развёрнут ли привилегированный helper и его polkit-политика — порт
/// `system._helper_available` (один вопрос на все helper'ы Rexon).
///
/// ⚠️ Помощник спрашивается ПО ИМЕНИ, а путь выбирает проба
/// ([`system_plan::pick_helper_path`]): раскладок две — пакет дистрибутива
/// кладёт помощников в `/usr/lib`, ручная установка в `/usr/local/lib`.
/// Прибитая константа `/usr/local/lib` заставляла диагностику маршрута
/// `--apply-defaults` говорить «помощников нет» на машине с пакетом, где
/// они есть. Политика у обеих раскладок одна — `/usr/share/polkit-1`.
///
/// Пробы приходят снаружи: решение остаётся проверяемым на обеих раскладках.
fn helper_available(
    name: &str,
    policy_path: &str,
    has_pkexec: bool,
    is_exec: &dyn Fn(&str) -> bool,
    exists: &dyn Fn(&str) -> bool,
) -> bool {
    has_pkexec && exists(&system_plan::pick_helper_path(name, is_exec)) && exists(policy_path)
}

/// Маршрут `--apply-defaults` целиком — порт ветки `rexon/app.py:625`.
///
/// Порядок шагов тот же, что у оригинала, и он важен: сначала применяются
/// дефолты (их результат и решает код возврата), потом идёт ДИАГНОСТИКА
/// комплектности. Проверка зовётся `allow_privileged = false`: при входе в
/// систему Rexon не спрашивает пароль и не ходит в сеть — недостающее
/// доустанавливается человеком по кнопке в настройках.
///
/// `out` — приёмник строк (в продукте `println!`): строки идут ровно в том
/// порядке, в каком их печатал оригинал.
pub fn apply_defaults_route<IO, C>(
    io: &IO,
    env: &AppDefaultsEnv,
    compliance_io: &mut C,
    compliance_env: &ComplianceEnv,
    out: &mut dyn FnMut(&str),
) -> i32
where
    IO: AppDefaultsIo + ?Sized,
    C: ComplianceIo,
{
    let switcher = DeAppSwitcher::new(io, env.clone());
    let (ok, logs) = switcher.apply_for_current_de();
    for line in &logs {
        out(line);
    }

    let mut check_logs: Vec<String> = Vec::new();
    {
        let mut check = comp::Compliance::with_xdg(
            compliance_io,
            &env.home,
            env.xdg_config_home.as_deref(),
            compliance_env.pkg_family,
            false, // вход в систему: только диагностика
            compliance_env.pkg_helper_available,
            compliance_env.stage_helper_available,
        );
        check.run_for_current_de(&env.xdg_current_desktop, &mut check_logs);
    }
    for line in &check_logs {
        out(line);
    }

    if ok {
        0
    } else {
        1
    }
}

/// Тот же маршрут на ЖИВОЙ машине: исполнители настоящие, окружение — своё.
pub fn apply_defaults_route_live(out: &mut dyn FnMut(&str)) -> i32 {
    let io = LiveAppDefaultsIo;
    let env = AppDefaultsEnv::from_process();
    let mut cio = LiveComplianceIo;
    let cenv = ComplianceEnv::from_live_machine();
    apply_defaults_route(&io, &env, &mut cio, &cenv, out)
}

// ============================================================================
// Живые исполнители
// ============================================================================

fn path_env() -> Option<String> {
    std::env::var("PATH").ok()
}

/// `safe_run(argv, capture_output=True)` на Rust: единая точка запуска ядра.
/// `None` — команда не запустилась либо не уложилась в таймаут.
fn live_run(argv: &[String], timeout_secs: Option<u64>) -> Option<(bool, String)> {
    if argv.is_empty() {
        return None;
    }
    let spec = RunSpec {
        argv: argv.to_vec(),
        output: RunOutput::Capture,
        timeout_secs,
        ..RunSpec::default()
    };
    let report = utils::run(&spec, path_env().as_deref());
    match report.outcome {
        RunOutcome::Ok | RunOutcome::Failed => Some((
            report.returncode == Some(0),
            utils::decode_text(&report.stdout),
        )),
        RunOutcome::Timeout | RunOutcome::Error => None,
    }
}

/// Текст файла с заменой негодных байт; `None` — не прочли.
fn read_text_lossy(path: &str) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|raw| String::from_utf8_lossy(&raw).into_owned())
}

/// Запись файла целиком общей дверью ядра, с созданием каталога.
///
/// Каталог создаётся здесь, а не у вызывающего: оригинал в
/// `generate_profile` писал в `~/.config` предполагая, что каталог есть, и
/// на свежем `$XDG_CONFIG_HOME` (наш же случай на стенде) запись падала.
///
/// Сюда уходят `mimeapps.list`, `kdeglobals` и строка автозапуска — файлы
/// покупателя, а не наши. Половина `mimeapps.list` — это ссылки, картинки и
/// документы, которые перестали открываться; милости «битое читается как
/// пустое», как у эталона, тут нет.
fn write_text_live(path: &str, content: &str) -> Result<(), String> {
    if let Some(dir) = Path::new(path).parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    crate::utils::atomic_write_str_err(path, content)
}

/// Настоящий исполнитель маршрута: файловая система, подпроцессы, сигналы.
pub struct LiveAppDefaultsIo;

impl AppDefaultsIo for LiveAppDefaultsIo {
    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn which(&self, name: &str) -> bool {
        // Порт `safe_check_command`: у имён с синонимами (`kwriteconfig6` →
        // `kwriteconfig5`) ответ даётся по ВСЕМ вариантам, иначе проверка и
        // последующий запуск расходятся.
        let path = path_env();
        if utils::tool_has_variants(name) {
            return utils::tool_argv(name, &[], path.as_deref()).is_some();
        }
        utils::lookup_tool(&[name], &[], path.as_deref()).is_some()
    }

    fn which_path(&self, name: &str) -> Option<String> {
        utils::which(name, path_env().as_deref())
    }

    fn read_text(&self, path: &str) -> Option<String> {
        read_text_lossy(path)
    }

    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        write_text_live(path, content)
    }

    fn makedirs(&self, path: &str) -> Result<(), String> {
        std::fs::create_dir_all(path).map_err(|e| e.to_string())
    }

    fn list_dir(&self, dir: &str) -> Vec<String> {
        // Имена сортируются: `glob` в Python отдаёт порядок файловой
        // системы, и «первый подходящий vendor-файл» иначе зависел бы от
        // машины (тот же довод, что у соседних исполнителей).
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .collect();
        names.sort();
        names
    }

    fn run(&self, argv: &[String]) -> Option<RunOut> {
        live_run(argv, None).map(|(ok, stdout)| RunOut { ok, stdout })
    }

    fn proc_comm(&self, pid: i64) -> Option<String> {
        read_text_lossy(&format!("/proc/{pid}/comm")).map(|s| s.trim().to_string())
    }

    fn terminate(&self, pid: i64) {
        if pid <= 0 {
            return; // 0 и отрицательные — это ГРУППА процессов, не наш случай
        }
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }

    fn window_pids(&self) -> Option<Vec<i64>> {
        // Спросить список окон ВМЕСТЕ с PID умеет только X11 (`wmctrl -lp`
        // читает `_NET_CLIENT_LIST` + `_NET_WM_PID`). У Wayland такого
        // протокола нет вовсе, и врать «окон нет» нельзя — это привело бы к
        // гашению менеджера с открытой у человека работой. Отвечаем честное
        // «не знаю», решение примут по командной строке владельца.
        if env_nonempty("WAYLAND_DISPLAY").is_some() || env_nonempty("DISPLAY").is_none() {
            return None;
        }
        let argv = vec!["wmctrl".to_string(), "-lp".to_string()];
        let (ok, stdout) = live_run(&argv, Some(5))?;
        if !ok {
            return None;
        }
        // Столбцы wmctrl -lp: id, рабочий стол, PID, машина, заголовок.
        Some(
            stdout
                .lines()
                .filter_map(|l| l.split_whitespace().nth(2)?.parse::<i64>().ok())
                .collect(),
        )
    }

    fn self_exe(&self) -> Option<String> {
        std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    fn launch_entry(&self) -> crate::installer::LaunchEntry {
        crate::installer::launch_entry_live()
    }

    fn login_lines(&self, desktop: &str) -> (String, Option<String>) {
        use crate::compositor_autostart as ca;
        ca::mirror_desktop_live(ca::ID_APPDEFAULTS, desktop)
    }
}

/// Настоящий исполнитель проверки комплектности — порт
/// `_LiveComplianceIo` из `rexon/compliance.py`.
///
/// Каждый метод — ровно один побочный вызов оригинала; на ошибке даётся тот
/// же безопасный дефолт (`safe_run` → `None`, `OSError` → `None`/`False`).
/// Порядок вызовов задаёт ядро ([`comp::Compliance`]).
pub struct LiveComplianceIo;

/// Уникальное имя во временном каталоге (`tempfile` без крейта tempfile).
fn tmp_name(prefix: &str, suffix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}{}_{stamp}_{n}{suffix}", std::process::id())
}

impl ComplianceIo for LiveComplianceIo {
    fn run(&mut self, argv: &[String], timeout: Option<u64>) -> Option<RunResult> {
        live_run(argv, timeout).map(|(ok, stdout)| RunResult { ok, stdout })
    }

    fn which(&mut self, name: &str) -> bool {
        utils::which(name, path_env().as_deref()).is_some()
    }

    fn env(&mut self, name: &str) -> String {
        std::env::var(name).unwrap_or_default()
    }

    fn exists(&mut self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn is_file(&mut self, path: &str) -> bool {
        Path::new(path).is_file()
    }

    fn is_dir(&mut self, path: &str) -> bool {
        Path::new(path).is_dir()
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        read_text_lossy(path)
    }

    fn scandir(&mut self, path: &str) -> Option<Vec<ScanEntry>> {
        let entries = std::fs::read_dir(path).ok()?;
        let mut out: Vec<ScanEntry> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                Some(ScanEntry {
                    is_dir: e.path().is_dir(),
                    name,
                })
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Some(out)
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(path) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .collect();
        names.sort();
        names
    }

    fn makedirs(&mut self, path: &str) {
        let _ = std::fs::create_dir_all(path);
    }

    fn write_text(&mut self, path: &str, content: &str) {
        let _ = write_text_live(path, content);
    }

    fn replace(&mut self, src: &str, dst: &str) {
        let _ = std::fs::rename(src, dst);
    }

    fn realpath(&mut self, path: &str) -> String {
        std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string())
    }

    fn mkdtemp(&mut self, prefix: &str, dir: Option<&str>) -> String {
        use std::os::unix::fs::PermissionsExt;
        let base = match dir {
            Some(d) if !d.is_empty() => std::path::PathBuf::from(d),
            _ => std::env::temp_dir(),
        };
        let mut last = base.join(prefix);
        for _ in 0..64 {
            last = base.join(tmp_name(prefix, ""));
            match std::fs::create_dir_all(&last) {
                Ok(()) => {
                    let _ = std::fs::set_permissions(
                        &last,
                        std::fs::Permissions::from_mode(0o700),
                    );
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => break,
            }
        }
        last.to_string_lossy().into_owned()
    }

    fn mkstemp(&mut self, prefix: &str, suffix: &str) -> String {
        use std::os::unix::fs::OpenOptionsExt;
        let base = std::env::temp_dir();
        let mut last = base.join(tmp_name(prefix, suffix));
        for _ in 0..64 {
            last = base.join(tmp_name(prefix, suffix));
            let opened = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&last);
            match opened {
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => break,
            }
        }
        last.to_string_lossy().into_owned()
    }

    fn chmod(&mut self, path: &str, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }

    fn remove_file(&mut self, path: &str) {
        let _ = std::fs::remove_file(path);
    }

    fn remove_tree(&mut self, path: &str) {
        let _ = std::fs::remove_dir_all(path);
    }

    fn copy_file(&mut self, src: &str, dst: &str) {
        let _ = std::fs::copy(src, dst);
    }
}
