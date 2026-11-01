// WindowMenuModel — меню главного окна Rexon на Rust: состав, тексты и
// исполнение трёх пунктов, за которыми стоит работа.
//
// ЗАЧЕМ. Меню под кнопкой «☰» — единственное место окна, откуда покупатель
// добирается до «Восстановить тему KDE», «О программе», «Удалить Rexon» и
// «Выход». В Rust-каркасе его не было НИ СТРОКИ: окно на QML открывалось без
// меню вовсе, то есть заменить окно на Python оно не могло в принципе.
//
// Приём тот же, что у объектов побочных действий вкладок: объект не решает
// ничего сам.
//
//   * ЧТО в меню, какими словами и что доступно — `rexon_core::window_menu`;
//   * ЧТО удалять, какой командой и КАК исполнять — ядро: решения в
//     `rexon_core::installer` (тот самый движок, которым пользуется
//     `RexonUninstaller` на Python), исполнение — `installer_exec::uninstall`
//     (детект владельца файла у пакетного менеджера, `pkexec pacman -R` для
//     пакета дистрибутива, скрипт удаления копии для всех, копия в домашней
//     папке, профиль, разбор исхода и текст итога). Своего исполнителя у окна
//     нет: до G8 здесь жила его вторая копия;
//   * здесь — только живые факты (семейство дистрибутива, свой бинарь) и
//     фоновый поток.
//
// ⚠️ УДАЛЕНИЕ ПРОГРАММЫ — ЗДЕСЬ УМЕСТНО. Правило владельца: кнопка удаления
// живёт только в ПРОГРАММАХ, в играх её не делают. Rexon — программа, и в
// продукте эта кнопка стоит ровно здесь.
//
// ⚠️ ПАКЕТ ДИСТРИБУТИВА — ОТДЕЛЬНЫЙ СПОСОБ УСТАНОВКИ. Файлы в /usr/{bin,lib}
// ведёт пакетный менеджер, и ручной детект их не видит вовсе: `rm -rf` по
// таким путям оставил бы в базе pacman/dpkg запись о пакете, чьи файлы
// исчезли. Поэтому сначала спрашиваем у менеджера ВЛАДЕЛЬЦА наших файлов, и
// если владелец есть — удаляем пакет его же средствами (`pkexec pacman -R`).
//
// ⚠️ УДАЛЕНИЕ ИДЁТ В ФОНЕ. За `pkexec` стоит запрос пароля, который ждёт
// сколько угодно; синхронный вызов подвесил бы окно на всё это время. Итог
// приходит сигналом `uninstallFinished`.

/// Мост cxx-qt меню окна.
///
/// Тип объявлен `#[qml_element]`, поэтому и хозяин окна, и разметка меню
/// могут завести его себе сами.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // ── Состав меню: [{id, label, shortcut, separatorBefore, enabled, destructive}, …]
        // в порядке меню. CONSTANT: за время жизни окна меню не меняется —
        // ровно как в продукте, где `_build_menubar` зовётся один раз.
        #[qproperty(QList_QVariant, items, READ = items, CONSTANT)]
        // ── Подпись самой кнопки меню («☰»).
        #[qproperty(QString, button_text, cxx_name = "buttonText",
                    READ = button_text, CONSTANT)]
        type WindowMenuModel = super::WindowMenuModelRust;
    }

    unsafe extern "RustQt" {
        /// Удаление закончилось: `ok` — получилось ли, `title` и `message` —
        /// заголовок и текст сообщения, которое показать человеку.
        #[qsignal]
        #[cxx_name = "uninstallFinished"]
        fn uninstall_finished(
            self: Pin<&mut WindowMenuModel>,
            ok: bool,
            title: QString,
            message: QString,
        );

        /// Снятие пакетов-замен частей Plasma закончилось: то же устройство,
        /// что у `uninstallFinished`.
        #[qsignal]
        #[cxx_name = "forksRemoveFinished"]
        fn forks_remove_finished(
            self: Pin<&mut WindowMenuModel>,
            ok: bool,
            title: QString,
            message: QString,
        );
    }

    extern "RustQt" {
        // Читатели свойств. Объявлены отдельно от вызываемых методов: у
        // свойства нет `#[qinvokable]`, его читает сам Qt.
        fn items(self: &WindowMenuModel) -> QList_QVariant;
        #[cxx_name = "buttonText"]
        fn button_text(self: &WindowMenuModel) -> QString;
    }

    extern "RustQt" {
        // ── «О программе» ─────────────────────────────────────────────────
        #[qinvokable]
        #[cxx_name = "aboutTitle"]
        fn about_title(self: &WindowMenuModel) -> QString;

        #[qinvokable]
        #[cxx_name = "aboutHtml"]
        fn about_html(self: &WindowMenuModel) -> QString;

        /// Журнал для справки: {path, url, location, openLabel, copyLabel, copied}.
        #[qinvokable]
        #[cxx_name = "logInfo"]
        fn log_info(self: &WindowMenuModel) -> QMap_QString_QVariant;

        /// Конец журнала для «Скопировать журнал».
        #[qinvokable]
        #[cxx_name = "logTail"]
        fn log_tail(self: &WindowMenuModel) -> QString;

        // ── «Восстановить тему KDE» ───────────────────────────────────────
        /// Заголовок и текст вопроса перед сбросом плюс имя темы, которая
        /// применится: {title, text, target}.
        #[qinvokable]
        #[cxx_name = "recoverPrompt"]
        fn recover_prompt(self: &WindowMenuModel) -> QMap_QString_QVariant;

        /// Запустить сброс. {ok, title, text} — что сказать человеку.
        #[qinvokable]
        #[cxx_name = "recoverApply"]
        fn recover_apply(self: &WindowMenuModel) -> QMap_QString_QVariant;

        /// Стоят ли пакеты, подменяющие части Plasma, и что о них сказать:
        /// {forks, title, text, removeLabel, themeLabel, cancelLabel}.
        /// Пустой `forks` — замен нет, сразу вопрос о сбросе темы. Только
        /// ЧТЕНИЕ списка пакетов; найденный набор запоминается, и дальше
        /// снимается ровно он.
        #[qinvokable]
        #[cxx_name = "recoverForks"]
        fn recover_forks(self: &WindowMenuModel) -> QMap_QString_QVariant;

        /// Что выполнит снятие замен — БЕЗ запуска (пробы только на
        /// чтение): {ok, title, text, runningTitle, runningText}. `ok=false`
        /// — вернуть стандартные пакеты нечем, `text` объясняет почему.
        #[qinvokable]
        #[cxx_name = "forksNext"]
        fn forks_next(self: &WindowMenuModel, button_id: &QString) -> QString;

        #[qinvokable]
        #[cxx_name = "forksPreview"]
        fn forks_preview(self: &WindowMenuModel) -> QMap_QString_QVariant;

        /// Взяться за снятие замен в фоне. `false` — уже идёт либо снимать
        /// нечего. Итог — сигналом `forksRemoveFinished`.
        #[qinvokable]
        #[cxx_name = "forksRemoveRun"]
        fn forks_remove_run(self: Pin<&mut WindowMenuModel>) -> bool;

        // ── «Удалить Rexon» ───────────────────────────────────────────────
        /// Окно удаления целиком: {title, text, informative, files,
        /// keepLabel, deleteLabel, cancelLabel, defaultId}.
        ///
        /// Только ЧТЕНИЕ файловой системы и опрос пакетного менеджера —
        /// ничего не удаляет.
        #[qinvokable]
        #[cxx_name = "uninstallPrompt"]
        fn uninstall_prompt(self: &WindowMenuModel) -> QMap_QString_QVariant;

        /// Взяться за удаление в фоне. `false` — уже идёт.
        #[qinvokable]
        #[cxx_name = "uninstallRun"]
        fn uninstall_run(self: Pin<&mut WindowMenuModel>, keep_profile: bool) -> bool;

        /// Идёт ли удаление.
        #[qinvokable]
        fn busy(self: &WindowMenuModel) -> bool;

        // ── Общее ─────────────────────────────────────────────────────────
        /// Доступен ли пункт. Спрашивается у ядра, а не решается разметкой.
        #[qinvokable]
        #[cxx_name = "isEnabled"]
        fn is_enabled(self: &WindowMenuModel, id: &QString) -> bool;
    }

    // Объявление трейта обязано быть В МОСТЕ: cxx-qt печатает по нему алиас
    // типа потока, без которого фоновая работа не доедет до сигнала.
    impl cxx_qt::Threading for WindowMenuModel {}
}

use core::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cxx_qt::{CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};

use rexon_core::distro_compat::Lang;
use rexon_core::installer_exec::{self as iexec, LiveInstallIo, LogSink, UninstallRequest};
use rexon_core::system::DistroDetector;
use rexon_core::system_de_exec::CancelToken;
use rexon_core::window_menu as wm;

/// Rust-состояние объекта: флажки «идёт» и набор пакетов-замен Plasma,
/// который показан человеку.
#[derive(Default)]
pub struct WindowMenuModelRust {
    uninstall_busy: Arc<AtomicBool>,
    forks_busy: Arc<AtomicBool>,
    /// Что показали в предупреждении — то и снимаем. Второй опрос между
    /// вопросом и снятием мог бы снять не то, что человек видел.
    shown_forks: Mutex<Vec<String>>,
}

fn qstr(s: &str) -> QVariant {
    QVariant::from(&QString::from(s))
}

fn map(pairs: &[(&str, QVariant)]) -> QMap<QMapPair_QString_QVariant> {
    let mut m = QMap::<QMapPair_QString_QVariant>::default();
    for (k, v) in pairs {
        m.insert(QString::from(*k), v.clone());
    }
    m
}

impl qobject::WindowMenuModel {
    // ── Состав меню ───────────────────────────────────────────────────────

    fn items(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        // На чужом рабочем столе пункта «Восстановить тему KDE» нет вовсе.
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        for item in wm::menu_items_for(&desktop) {
            out.append(QVariant::from(&map(&[
                ("id", qstr(item.id)),
                ("label", qstr(&item.label)),
                ("shortcut", qstr(item.shortcut)),
                ("separatorBefore", QVariant::from(&item.separator_before)),
                ("enabled", QVariant::from(&wm::is_enabled(item.id))),
                ("destructive", QVariant::from(&wm::is_destructive(item.id))),
            ])));
        }
        out
    }

    fn button_text(&self) -> QString {
        QString::from(wm::MENU_BUTTON)
    }

    fn is_enabled(&self, id: &QString) -> bool {
        wm::is_enabled(&id.to_string())
    }

    // ── «О программе» ─────────────────────────────────────────────────────

    fn about_title(&self) -> QString {
        QString::from(&wm::about_title())
    }

    fn about_html(&self) -> QString {
        QString::from(&wm::about_dialog_html())
    }

    fn log_info(&self) -> QMap<QMapPair_QString_QVariant> {
        let path = rexon_core::applog::log_path().to_string_lossy().into_owned();
        map(&[
            ("path", qstr(&path)),
            ("url", qstr(&format!("file://{path}"))),
            ("location", qstr(&wm::log_location_text(&path))),
            ("openLabel", qstr(&wm::log_open_label())),
            ("copyLabel", qstr(&wm::log_copy_label())),
            ("copied", qstr(&wm::log_copied_text())),
        ])
    }

    fn log_tail(&self) -> QString {
        rexon_core::log_info!("about", "журнал скопирован из справки");
        QString::from(&rexon_core::applog::tail(wm::LOG_COPY_BYTES))
    }

    // ── «Восстановить тему KDE» ───────────────────────────────────────────

    fn recover_prompt(&self) -> QMap<QMapPair_QString_QVariant> {
        let target = recover_target();
        // На чужом рабочем столе маршрут откажет — окно говорит это заранее,
        // а не докладывает «запущено» (`allowed=false` + `refusal`).
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        map(&[
            ("title", qstr(&wm::recover_title())),
            ("text", qstr(&wm::recover_question(&target))),
            ("target", qstr(&target)),
            ("allowed", QVariant::from(&wm::recover_allowed(&desktop))),
            ("refusal", qstr(&wm::recover_not_plasma_text(&desktop))),
        ])
    }

    fn recover_forks(&self) -> QMap<QMapPair_QString_QVariant> {
        let forks = rexon_core::system_exec::live().detect_plasma_forks();
        *self
            .rust()
            .shown_forks
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = forks.clone();
        let mut list = QList::<QVariant>::default();
        for f in &forks {
            list.append(qstr(f));
        }
        // Окно и подписи кнопок — те же, что у Python-окна (`forks_prompt`).
        let Some(prompt) = wm::forks_prompt(&forks) else {
            return map(&[("forks", QVariant::from(&list))]);
        };
        let label = |id: &str| {
            prompt
                .buttons
                .iter()
                .find(|b| b.id == id)
                .map(|b| b.label.clone())
                .unwrap_or_default()
        };
        map(&[
            ("forks", QVariant::from(&list)),
            ("title", qstr(&prompt.title)),
            ("text", qstr(&prompt.text)),
            ("removeLabel", qstr(&label(wm::FORKS_BUTTON_REMOVE))),
            ("themeLabel", qstr(&label(wm::FORKS_BUTTON_THEME))),
            ("cancelLabel", qstr(&label(wm::FORKS_BUTTON_CANCEL))),
        ])
    }

    fn forks_next(&self, button_id: &QString) -> QString {
        QString::from(wm::forks_next(&button_id.to_string()).as_str())
    }

    fn forks_preview(&self) -> QMap<QMapPair_QString_QVariant> {
        let forks = self.shown_forks();
        let (ok, message) = rexon_core::system_exec::live().plasma_fork_reset_preview(Some(&forks));
        let step = wm::forks_preview_step(ok, &message);
        map(&[
            ("ok", QVariant::from(&step.ok)),
            ("title", qstr(&step.title)),
            ("text", qstr(&step.text)),
            ("runningTitle", qstr(&step.running_title)),
            ("runningText", qstr(&step.running_text)),
        ])
    }

    fn shown_forks(&self) -> Vec<String> {
        self.rust()
            .shown_forks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn forks_remove_run(mut self: Pin<&mut Self>) -> bool {
        let forks = self.as_ref().get_ref().shown_forks();
        if forks.is_empty() {
            return false;
        }
        let flag = self.as_ref().get_ref().rust().forks_busy.clone();
        if flag.swap(true, Ordering::SeqCst) {
            return false;
        }
        let handle = self.as_mut().qt_thread();
        // В фоне: за `pkexec` стоит запрос пароля, который ждёт сколько
        // угодно, а транзакция пакетов — до минуты. Python на это время
        // вешал окно; здесь оно остаётся живым.
        std::thread::spawn(move || {
            let (ok, message) =
                rexon_core::system_exec::live().remove_plasma_forks(Some(&forks));
            flag.store(false, Ordering::SeqCst);
            let (title, text) = wm::forks_finished(ok, &message);
            let queued = handle.queue(move |mut object| {
                object
                    .as_mut()
                    .forks_remove_finished(ok, QString::from(&title), QString::from(&text));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("window-menu", "итог снятия замен Plasma не доставлен: {e:?}");
                }
            }
        });
        true
    }

    fn recover_apply(&self) -> QMap<QMapPair_QString_QVariant> {
        let target = recover_target();
        // Запустить и забыть — как `subprocess.Popen` в продукте: за флагом
        // стоит переключение темы Plasma и перезапуск plasmashell, ждать его
        // из потока интерфейса нельзя. Зовётся СВОЙ бинарь: маршрут
        // `--recover-kde` он отрабатывает на Rust до подъёма Qt.
        let own_exe = std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().into_owned());
        match spawn_detached(&wm::recover_argv(own_exe.as_deref())) {
            Ok(()) => map(&[
                ("ok", QVariant::from(&true)),
                ("title", qstr(&wm::recover_started_title())),
                ("text", qstr(&wm::recover_started_text(&target))),
            ]),
            Err(e) => map(&[
                ("ok", QVariant::from(&false)),
                ("title", qstr(&wm::error_title())),
                ("text", qstr(&wm::recover_failed_text(&e))),
            ]),
        }
    }

    // ── «Удалить Rexon» ───────────────────────────────────────────────────

    fn uninstall_prompt(&self) -> QMap<QMapPair_QString_QVariant> {
        let io = LiveInstallIo::new();
        let listing = iexec::uninstall_files(&io, &live_uninstall_request(&io, false));
        let mut list = QList::<QVariant>::default();
        for f in &listing.removed(false) {
            list.append(qstr(f));
        }
        map(&[
            ("title", qstr(&wm::uninstall_title())),
            ("text", qstr(&wm::uninstall_text())),
            ("informative", qstr(&wm::uninstall_informative(&listing.program, &listing.user_data))),
            ("files", QVariant::from(&list)),
            ("keepLabel", qstr(&wm::uninstall_keep_label())),
            ("deleteLabel", qstr(&wm::uninstall_delete_label())),
            ("cancelLabel", qstr(&wm::uninstall_cancel_label())),
            ("defaultId", qstr(wm::UNINSTALL_DEFAULT_ID)),
        ])
    }

    fn busy(&self) -> bool {
        self.rust().uninstall_busy.load(Ordering::SeqCst)
    }

    fn uninstall_run(mut self: Pin<&mut Self>, keep_profile: bool) -> bool {
        if self.as_ref().get_ref().busy() {
            return false;
        }
        let flag = self.as_ref().get_ref().rust().uninstall_busy.clone();
        flag.store(true, Ordering::SeqCst);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let io = LiveInstallIo::new();
            let req = live_uninstall_request(&io, keep_profile);
            let result = iexec::uninstall(&io, &req, Arc::new(LogSink), &CancelToken::new());
            let (ok, message) = (result.ok(), result.message);
            flag.store(false, Ordering::SeqCst);
            let (title, text) = if ok {
                (wm::uninstall_done_title(), wm::uninstall_done_text(&message))
            } else {
                (wm::uninstall_failed_title(), message)
            };
            let queued = handle.queue(move |mut object| {
                object
                    .as_mut()
                    .uninstall_finished(ok, QString::from(&title), QString::from(&text));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    // Молчать нельзя: окно осталось бы с «Удаляю…» навсегда.
                    rexon_core::log_warn!("uninstall", "итог удаления не доставлен: {e:?}");
                }
            }
        });
        true
    }
}

// ============================================================================
// «Восстановить тему KDE»: какая тема применится
// ============================================================================

/// Имя темы из резервной копии либо запасная. Читает файл; решение о том, что
/// считать пустотой и чем её заменить, принимает ядро.
fn recover_target() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let path = wm::recover_backup_path(&home, xdg.as_deref().filter(|v| !v.is_empty()));
    let saved = std::fs::read_to_string(&path).ok();
    wm::recover_target(saved.as_deref())
}

/// Запустить и забыть. Ошибку возвращаем текстом: диалог обязан назвать
/// причину, иначе нажатие выглядит как «ничего не произошло».
fn spawn_detached(argv: &[String]) -> Result<(), String> {
    let Some((binary, args)) = argv.split_first() else {
        return Err(rexon_core::i18n::tr("empty command"));
    };
    std::process::Command::new(binary)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ============================================================================
// Удаление: живые факты для исполнителя ядра
// ============================================================================

/// Язык интерфейса парой установщика (`Lang`) — тот же, что у каталога.
fn ui_lang() -> Lang {
    Lang::from_code(&rexon_core::i18n::current_language())
}

/// Что удалять на этой машине. Решения и исполнение — в ядре
/// (`installer_exec::detect_distro_package`, `UninstallRequest::detect`);
/// здесь только живые факты: семейство дистрибутива и путь своего бинаря.
///
/// Язык итога — язык интерфейса (`rexon_core::i18n`): заголовки окна
/// удаления идут через тот же каталог, и итог другим языком дал бы окно на
/// двух языках. Установщик пока выбирает пару «русский/английский» сам
/// (`Lang`), поэтому язык ему передаётся тот же, что у каталога.
fn live_uninstall_request(io: &LiveInstallIo, keep_profile: bool) -> UninstallRequest {
    let family = DistroDetector::detect_live().family_live();
    let own_exe = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let package = iexec::detect_distro_package(io, family, own_exe.as_deref());
    UninstallRequest::detect(io, package, keep_profile, ui_lang())
}

#[cfg(test)]
mod tests {
    /// Ни один текст меню в этом файле не набран заново: все приходят из
    /// ядра. Сторож на копипасту — вторая копия человеческой строки
    /// разошлась бы с продуктом молча (правило #12).
    #[test]
    fn no_human_text_is_typed_twice_in_this_file() {
        // Свой же тестовый модуль не считаем: список запрещённых строк
        // лежит ниже, и без этой отсечки сторож ловил бы сам себя и не мог
        // позеленеть никогда.
        let whole = include_str!("window_menu.rs");
        let src = whole
            .split_once("#[cfg(test)]")
            .map(|(code, _)| code)
            .expect("тестовый модуль на месте");
        for baked in [
            "Восстановить тему KDE",
            "О программе",
            "Удалить Rexon",
            "Выход",
            "Breeze Dark",
            "Сохранить резервные копии",
            "Только сбросить тему",
            "вернуть обычную Plasma",
            "Замены удалены",
            "--recover-kde",
            // Английские исходные тексты каталога — тоже только в ядре.
            "Restore KDE Theme",
            "About Rexon",
            "Uninstall Rexon",
            "Remove Everything",
            "Reset Theme Only",
            "Replacements Removed",
        ] {
            let in_code: Vec<&str> = src
                .lines()
                .filter(|l| l.contains(baked))
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect();
            assert!(
                in_code.is_empty(),
                "человеческий текст «{baked}» набран здесь, а не взят у ядра: {in_code:?}"
            );
        }
    }

    /// G8: исполнитель удаления один — в ядре. Второй копии шагов плана,
    /// запуска команд и удаления путей в окне нет: она разошлась бы с ядром
    /// молча (правило #12). Чужой шаг плана — отказ ядра
    /// (`installer_exec::tests::foreign_plan_step_is_refused`), перечень —
    /// только чтение (`installer_exec::tests::nothing_installed_touches_nothing_not_even_settings`).
    #[test]
    fn window_has_no_uninstall_executor_of_its_own() {
        let whole = include_str!("window_menu.rs");
        let src = whole
            .split_once("#[cfg(test)]")
            .map(|(code, _)| code)
            .expect("тестовый модуль на месте");
        for own in [
            "fn exec_step",
            "fn run_cmd",
            "fn remove_path",
            "remove_dir_all",
            "PlanStep",
            "uninstall_system_plan",
            "\"pkexec\"",
        ] {
            let hits: Vec<&str> = src
                .lines()
                .filter(|l| l.contains(own))
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect();
            assert!(hits.is_empty(), "окно снова исполняет удаление само («{own}»): {hits:?}");
        }
        assert!(src.contains("iexec::uninstall(&io"), "окно не зовёт исполнитель ядра");
        assert!(src.contains("iexec::uninstall_files(&io"), "перечень удаляемого берётся не у ядра");
    }
}
