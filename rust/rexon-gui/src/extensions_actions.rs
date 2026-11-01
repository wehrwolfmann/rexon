// ExtensionsActions — объект побочных действий вкладки «Расширения» на Rust.
//
// ЗАЧЕМ. Каждая QML-страница продукта ждёт свойством `actions` объект, который
// умеет то, что меняет мир: поставить .rxn, снять расширение, включить его,
// открыть каталог. Когда писался этот объект, такой был ОДИН на Rust — у
// «Сравнения DE», и даже он останавливался на границе исполнения (сейчас его
// место занял `DesktopsActions`). У «Расширений» объект жил на PySide6
// (`rexon/qt_extensions/window.py`, класс `ExtensionsActions`, 259 строк вместе
// с хозяином окна), и пока он там, страница получала `actions` от Python — то
// есть уйти с PySide6 вкладка не могла в принципе.
//
// Здесь тот же договор — те же СЕМЬ методов и те же два сигнала — исполняет
// Rust. Приём тот же, что у объектов действий других вкладок: объект ничего не решает
// сам, все решения берёт ядро.
//
//   * ЧТО делать при установке/удалении оформления решает планировщик
//     `rexon_core::ext_manager` (он был готов давно);
//   * ДЕЛАЕТ это исполнитель `rexon_core::ext_exec` — дописан вместе с этим
//     объектом, потому что настоящей файловой системы у планировщика не было
//     вовсе (вход у него — снимок `FsSnap`);
//   * ЧТО сказать человеку — тексты ядра (`rexon_core::qt_extensions`), те же
//     самые, что показывает виджетная вкладка.
//
// ⚠️ ФОНОВОСТЬ СОХРАНЕНА, И ЭТО НЕ УКРАШЕНИЕ. У виджетной вкладки установка и
// удаление шли в `QThread`, и окно во время распаковки .rxn не подвисало.
// Синхронный вызов, как у «Сравнения DE», был бы здесь потерей: распаковка
// большой темы занимает секунды, а `pkexec` ждёт пароля сколько угодно.
// Поэтому `installPath`/`uninstall` только БЕРУТСЯ за дело и сразу возвращают
// управление, а итог приходит сигналом `installFinished`/`uninstallFinished` —
// его страница уже слушает блоком `Connections`.
//
// ⚠️ ПЕРЕКЛЮЧЕНИЕ — НАОБОРОТ, СИНХРОННОЕ: это запись одного файла состояния, и
// ответ нужен странице немедленно. Возвращается строка «on» / «off» /
// «!текст ошибки», как и у Python: пары «получилось ли + новое состояние»
// одним значением не выразить, а два вызова подряд разошлись бы с диском.
//
// ⚠️ ДИАЛОГИ. `installDialog` и `confirm` на Python были однострочниками из
// QtWidgets (`QFileDialog`, `QMessageBox`), а в QML-процессе виджетов нет
// вовсе. Разбор решения и его цена — в шапке `cpp/rexon_ext_dialogs.h`;
// коротко: диалог поднимается штатным QML-типом `QtQuick.Dialogs`, а
// синхронность договора с ней мирит вложенный цикл событий, как у
// `QDialog::exec()`.

/// Мост cxx-qt объекта действий.
///
/// Тип объявлен `#[qml_element]`, поэтому страница может завести его себе сама
/// (`ExtensionsActions { }`), а хозяин окна — подставить свой. Регистрации в
/// `main_window.rs` для этого не требуется: там лежит карта разделов, а не
/// список побочных объектов.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    // Диалоги — см. шапку `cpp/rexon_ext_dialogs.h`.
    unsafe extern "C++" {
        include!("rexon_ext_dialogs.h");

        #[rust_name = "pick_file"]
        fn rexon_ext_pick_file(title: &QString, filter: &QString, folder: &QString) -> QString;

        /// Окно сохранения файла (экспорт группы шрифтов в «Настройках»).
        #[rust_name = "save_file"]
        fn rexon_ext_save_file(title: &QString, filter: &QString, path: &QString) -> QString;

        #[rust_name = "ask_confirm_skinned"]
        fn rexon_ext_confirm(title: &QString, text: &QString, skin: &QString) -> bool;

        #[rust_name = "open_folder_url"]
        fn rexon_ext_open_folder(path: &QString) -> bool;

        /// Окно выбора шрифта. Живёт здесь же, потому что шим диалогов в
        /// проекте один: им пользуются и «Настройки» (`settings_actions.rs`),
        /// как уже пользуются `pick_file` для импорта тем Windows.
        #[rust_name = "pick_font"]
        fn rexon_ext_pick_font(
            title: &QString,
            family: &QString,
            size: i32,
            outcome: &mut i32,
        ) -> QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type ExtensionsActions = super::ExtensionsActionsRust;
    }

    unsafe extern "RustQt" {
        /// Установка закончилась: `ok` — получилось ли, `message` — имя
        /// поставленного расширения либо текст ошибки.
        #[qsignal]
        #[cxx_name = "installFinished"]
        fn install_finished(self: Pin<&mut ExtensionsActions>, ok: bool, message: QString);

        /// Удаление закончилось: `ok` — получилось ли, `message` —
        /// идентификатор снятого расширения либо текст ошибки.
        #[qsignal]
        #[cxx_name = "uninstallFinished"]
        fn uninstall_finished(self: Pin<&mut ExtensionsActions>, ok: bool, message: QString);

        /// «Применить» закончилось: `message` — готовая строка итога.
        #[qsignal]
        #[cxx_name = "applyFinished"]
        fn apply_finished(self: Pin<&mut ExtensionsActions>, message: QString);
    }

    extern "RustQt" {
        /// Идёт ли уже установка или удаление. Один ответ на оба вопроса: у
        /// виджетной вкладки были две проверки, но обе вели к одному —
        /// «дождись, потом жми».
        #[qinvokable]
        fn busy(self: &ExtensionsActions) -> bool;

        /// Окно выбора .rxn. Пустая строка — человек передумал.
        #[qinvokable]
        #[cxx_name = "installDialog"]
        fn install_dialog(self: &ExtensionsActions) -> QString;

        /// Взяться за установку файла в фоне. `false` — не взялись.
        #[qinvokable]
        #[cxx_name = "installPath"]
        fn install_path(self: Pin<&mut ExtensionsActions>, path: &QString) -> bool;

        /// Вопрос «да/нет» перед необратимым действием.
        #[qinvokable]
        fn confirm(self: &ExtensionsActions, title: &QString, text: &QString) -> bool;

        /// Взяться за удаление расширения в фоне. `false` — не взялись.
        #[qinvokable]
        fn uninstall(self: Pin<&mut ExtensionsActions>, ext_id: &QString) -> bool;

        /// Переключить расширение. «on» / «off» / «!ошибка».
        #[qinvokable]
        fn toggle(self: Pin<&mut ExtensionsActions>, ext_id: &QString) -> QString;

        /// Открыть каталог расширения в файловом менеджере.
        #[qinvokable]
        #[cxx_name = "openFolder"]
        fn open_folder(self: &ExtensionsActions, path: &QString) -> bool;

        /// Подпись кнопки «Применить» — пустая, если это расширение применить
        /// отсюда нельзя (не набор обоев либо в наборе нет файлов).
        #[qinvokable]
        #[cxx_name = "applyText"]
        fn apply_text(self: &ExtensionsActions, ext_id: &QString) -> QString;

        /// Поставить обои набора на рабочий стол (первые из набора; остальные
        /// выбираются в «Настройки → Библиотека обоев»). Возвращает строку
        /// «применяю…»; итог придёт сигналом `applyFinished`.
        #[qinvokable]
        fn apply(self: Pin<&mut ExtensionsActions>, ext_id: &QString) -> QString;

        /// Библиотека Razer Axon (окно «Razer Axon (тест)»): JSON
        /// `{found, note, dirs: [путь], items: [{id, title, author, line,
        /// downloaded, folder}]}` — найденное в префиксах Wine плюс папки,
        /// указанные вручную (`rexon_core::axon_library`).
        #[qinvokable]
        #[cxx_name = "axonTexts"]
        fn axon_texts(self: &ExtensionsActions) -> QString;

        #[qinvokable]
        #[cxx_name = "axonList"]
        fn axon_list(self: &ExtensionsActions) -> QString;

        /// Запомнить папку с обоями Axon (`WallpaperSource`), выбранную
        /// человеком (`file://…` или путь). JSON `{ok, message}`.
        #[qinvokable]
        #[cxx_name = "axonAddFolder"]
        fn axon_add_folder(self: &ExtensionsActions, url: &QString) -> QString;

        /// Подготовить обои Axon `id` и поставить их на рабочий стол общим
        /// исполнителем обоев. Возвращает строку «готовлю…»; итог — сигналом
        /// `applyFinished`.
        #[qinvokable]
        #[cxx_name = "axonApply"]
        fn axon_apply(self: Pin<&mut ExtensionsActions>, id: &QString) -> QString;

        /// Подготовка обоев Axon заранее: JSON `{line, running, enabled}` —
        /// строка «Подготовлено: N, осталось: M» (`rexon_core::axon_prepare`).
        #[qinvokable]
        #[cxx_name = "axonPrepareStatus"]
        fn axon_prepare_status(self: &ExtensionsActions) -> QString;

        /// Переключатель «Готовить обои заранее»: запомнить в настройках и,
        /// если включили, сразу запустить фоновую подготовку.
        #[qinvokable]
        #[cxx_name = "axonSetPrepareAhead"]
        fn axon_set_prepare_ahead(self: &ExtensionsActions, on: bool);

        /// «Вернуть оригинальные описания»: выключает подготовку заранее и
        /// возвращает ResourceConfig.txt из резервных копий. Итог — сигналом
        /// `applyFinished`.
        #[qinvokable]
        #[cxx_name = "axonRestoreOriginals"]
        fn axon_restore_originals(self: Pin<&mut ExtensionsActions>);
    }

    // Объявление трейта обязано быть В МОСТЕ: cxx-qt печатает по нему алиас
    // типа потока, без которого фоновая работа не доедет до сигнала.
    impl cxx_qt::Threading for ExtensionsActions {}
}

use core::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::QString;

use rexon_core::ext_exec;
use rexon_core::qt_extensions as qe;

/// Rust-состояние объекта действий: два флажка занятости, по одному на вид
/// работы. Не один общий: установка и удаление идут разными потоками, и общий
/// флажок гасился бы первым же завершившимся.
#[derive(Default)]
pub struct ExtensionsActionsRust {
    install_busy: Arc<AtomicBool>,
    uninstall_busy: Arc<AtomicBool>,
}

/// Цвета действующего скина и подписи кнопок для окна вопроса — в порядке,
/// которого ждёт шим (`rexon_ext_confirm`): фон, карточка, текст, акцент,
/// рамка, «Да», «Нет»; разделитель U+001F.
pub fn confirm_skin() -> String {
    confirm_skin_labeled(&rexon_core::i18n::tr("Yes"), &rexon_core::i18n::tr("No"))
}

/// [`confirm_skin`] со своими подписями кнопок.
fn confirm_skin_labeled(yes: &str, no: &str) -> String {
    let colors = rexon_core::ui_base::active_skin_colors();
    let tone = |key: &str| colors.get(key).cloned().unwrap_or_default();
    [
        tone("bg_primary"),
        tone("bg_card"),
        tone("text_primary"),
        tone("accent"),
        tone("border"),
        yes.to_string(),
        no.to_string(),
    ]
    .join("\u{1f}")
}

/// Вопрос «да/нет» окном в скине Rexon. `false` — «Нет», закрытое окно и
/// окно, которое не поднялось.
pub fn ask_confirm(title: &QString, text: &QString) -> bool {
    qobject::ask_confirm_skinned(title, text, &QString::from(&confirm_skin()))
}

/// [`ask_confirm`] со своими подписями кнопок («Скачать и включить» /
/// «Оставить мои»). `false` — вторая кнопка, закрытое окно и окно, которое
/// не поднялось.
pub fn ask_confirm_labeled(title: &str, text: &str, yes: &str, no: &str) -> bool {
    qobject::ask_confirm_skinned(&QString::from(title), &QString::from(text), &QString::from(&confirm_skin_labeled(yes, no)))
}

impl qobject::ExtensionsActions {
    fn busy(&self) -> bool {
        let state = self.rust();
        state.install_busy.load(Ordering::SeqCst) || state.uninstall_busy.load(Ordering::SeqCst)
    }

    fn install_dialog(&self) -> QString {
        // Заголовок и фильтр приходят из ядра: странице они уже известны
        // (`fileDialogTitle` / `fileDialogFilter`), и второй их копии здесь
        // быть не должно.
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        qobject::pick_file(
            &QString::from(&qe::file_dialog_title()),
            &QString::from(&qe::file_dialog_filter()),
            &QString::from(&home),
        )
    }

    fn confirm(&self, title: &QString, text: &QString) -> bool {
        ask_confirm(title, text)
    }

    fn open_folder(&self, path: &QString) -> bool {
        qobject::open_folder_url(path)
    }

    fn install_path(mut self: Pin<&mut Self>, path: &QString) -> bool {
        if self.as_ref().get_ref().busy() {
            return false;
        }
        let flag = self.as_ref().get_ref().rust().install_busy.clone();
        flag.store(true, Ordering::SeqCst);
        let path = path.to_string();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // Итог установки — имя расширения И предупреждения о пропущенных
            // шагах (нет конвертера, нечем измерить обои). Предупреждение,
            // не дошедшее до строки состояния, равно его отсутствию.
            let outcome = ext_exec::install_from_file(&path)
                .map(|done| qe::installed_with_notes(&done.name, &done.notes));
            flag.store(false, Ordering::SeqCst);
            report(handle, outcome, true);
        });
        true
    }

    fn uninstall(mut self: Pin<&mut Self>, ext_id: &QString) -> bool {
        if self.as_ref().get_ref().busy() {
            return false;
        }
        let flag = self.as_ref().get_ref().rust().uninstall_busy.clone();
        flag.store(true, Ordering::SeqCst);
        let ext_id = ext_id.to_string();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let outcome = ext_exec::uninstall(&ext_id).map(|()| ext_id.clone());
            flag.store(false, Ordering::SeqCst);
            report(handle, outcome, false);
        });
        true
    }

    fn apply_text(&self, ext_id: &QString) -> QString {
        if pack_wallpapers(&ext_id.to_string()).is_empty() {
            QString::default()
        } else {
            QString::from(&rexon_core::qt_settings::text("lib.apply", &[]).unwrap_or_default())
        }
    }

    fn apply(mut self: Pin<&mut Self>, ext_id: &QString) -> QString {
        use rexon_core::{qt_settings as qs, wallpaper_engine, wallpaper_exec};
        let Some(first) = pack_wallpapers(&ext_id.to_string()).into_iter().next() else {
            return QString::from(&qs::text("lib.no_file", &[]).unwrap_or_default());
        };
        let basename = std::path::Path::new(&first.path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let applying = qs::library_applying_status(&basename, &wallpaper_engine::format_label(&first.format));
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // Та же дверь ядра, что у «Библиотеки обоев» в «Настройках»: один
            // исполнитель на процесс, надзор за живыми обоями — его забота.
            let status = wallpaper_exec::run_in_slot(
                &wallpaper_exec::live_slot(),
                move |exec: &mut wallpaper_exec::WallpaperExec<wallpaper_exec::LiveSys>| {
                    let res = exec.apply(&first.path, &first.format, "");
                    qs::wallpaper_done(res.ok, &res.message)
                },
            );
            if let Err(e) = handle.queue(move |mut object| {
                object.as_mut().apply_finished(QString::from(&status));
            }) {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("extensions", "итог применения обоев не доставлен: {e:?}");
                }
            }
        });
        QString::from(&applying)
    }

    /// Подписи окна «Razer Axon (тест)» одним JSON — тексты ядра.
    fn axon_texts(&self) -> QString {
        use rexon_core::{axon_library as ax, axon_prepare as ap};
        let apply = rexon_core::qt_settings::text("lib.apply", &[]).unwrap_or_default();
        QString::from(
            &serde_json::json!({
                "button": ax::entry_button_text(),
                "title": ax::window_title(),
                "pick": ax::pick_folder_text(),
                "pickTooltip": ax::pick_folder_tooltip(),
                "pickTitle": ax::pick_folder_title(),
                "open": ax::open_folder_text(),
                "openTooltip": ax::open_folder_tooltip(),
                "apply": apply,
                "close": rexon_core::i18n::tr("Close"),
                "prepareAhead": ap::toggle_text(),
                "prepareAheadTooltip": ap::toggle_tooltip(),
                "restore": ap::restore_button_text(),
                "restoreWarning": ap::restore_warning_text(),
                "restoreConfirm": ap::restore_confirm_text(),
                "cancel": rexon_core::i18n::tr("Cancel"),
            })
            .to_string(),
        )
    }

    fn axon_list(&self) -> QString {
        use rexon_core::axon_library as ax;
        let lib = ax::load_library(&axon_env());
        for p in &lib.problems {
            rexon_core::log_warn!("axon", "библиотека не прочитана: {p}");
        }
        let note = if !lib.found {
            ax::not_found_text()
        } else if lib.wallpapers.is_empty() {
            ax::empty_text()
        } else {
            String::new()
        };
        let items: Vec<serde_json::Value> = lib
            .wallpapers
            .iter()
            .map(|w| {
                serde_json::json!({
                    "id": w.id,
                    "title": ax::display_title(w),
                    "author": w.author,
                    "line": ax::tile_line(w),
                    "downloaded": w.downloaded,
                    "folder": w.resource_dir.as_ref().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
                })
            })
            .collect();
        let dirs: Vec<String> = lib.source_dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect();
        QString::from(&serde_json::json!({"found": lib.found, "note": note, "dirs": dirs, "items": items}).to_string())
    }

    fn axon_add_folder(&self, url: &QString) -> QString {
        use rexon_core::axon_library as ax;
        let raw = url.to_string();
        let path = std::path::PathBuf::from(rexon_core::local_install::file_url_to_path(&raw));
        let answer = |ok: bool, message: String| {
            QString::from(&serde_json::json!({"ok": ok, "message": message}).to_string())
        };
        let dir = match ax::check_source_dir(&path) {
            Ok(d) => d,
            Err(why) => return answer(false, why),
        };
        let count = ax::read_source_dir(&dir).map(|l| l.len()).unwrap_or(0);
        let saved = match rexon_core::settings::SettingsManager::new() {
            Ok(mut m) => {
                let list = ax::with_source_dir(&ax::saved_source_dirs(&m.settings), &dir);
                m.set(ax::SETTINGS_KEY, list).map_err(|e| e.to_string())
            }
            Err(e) => Err(e.to_string()),
        };
        if let Err(e) = saved {
            rexon_core::log_warn!("axon", "папка {} не сохранена в настройках: {e}", dir.display());
        }
        // Новая папка — подготовить и её обои заранее (выключено — поток
        // сразу выйдет).
        rexon_core::axon_prepare::spawn_background(std::time::Duration::ZERO);
        answer(true, ax::folder_added_text(count))
    }

    fn axon_apply(mut self: Pin<&mut Self>, id: &QString) -> QString {
        use rexon_core::{axon_library as ax, qt_settings as qs, wallpaper_exec};
        let env = axon_env();
        let lib = ax::load_library(&env);
        let Some(w) = lib.get(&id.to_string()).cloned() else {
            return QString::from(&ax::prepare_failed_text(&id.to_string()));
        };
        let preparing = ax::preparing_text(&ax::display_title(&w));
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let extractor = ax::default_extractor();
            // Подготовленные заранее — прямо из `Resource/`, прочие — через кэш.
            let status = match rexon_core::axon_prepare::prepare_for_apply(&w, Some(extractor.as_ref()), &env) {
                Ok(ready) => {
                    rexon_core::log_info!("axon", "обои {} готовы: {}", w.id, ready.path.display());
                    let path = ready.path.to_string_lossy().into_owned();
                    // Та же дверь ядра, что у «Применить» набора обоев: один
                    // исполнитель на процесс, вывод под окружение решает он.
                    wallpaper_exec::run_in_slot(
                        &wallpaper_exec::live_slot(),
                        move |exec: &mut wallpaper_exec::WallpaperExec<wallpaper_exec::LiveSys>| {
                            let res = exec.apply_with_audio(&path, ready.format, &ready.name, ready.audio);
                            qs::wallpaper_done(res.ok, &res.message)
                        },
                    )
                }
                Err(why) => format!("❌ {why}"),
            };
            if let Err(e) = handle.queue(move |mut object| {
                object.as_mut().apply_finished(QString::from(&status));
            }) {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("axon", "итог применения обоев не доставлен: {e:?}");
                }
            }
        });
        QString::from(&preparing)
    }

    fn axon_prepare_status(&self) -> QString {
        use rexon_core::{axon_library as ax, axon_prepare as ap};
        let (env, settings) = ap::live_env();
        let lib = ax::load_library(&env);
        let counts = ap::counts(&lib, &env.originals_root());
        let running = ap::is_running();
        let line = if counts.prepared + counts.remaining == 0 {
            String::new()
        } else {
            ap::status_text(counts, running)
        };
        QString::from(
            &serde_json::json!({"line": line, "running": running, "enabled": ap::enabled(&settings)}).to_string(),
        )
    }

    fn axon_set_prepare_ahead(&self, on: bool) {
        use rexon_core::axon_prepare as ap;
        match rexon_core::settings::SettingsManager::new() {
            Ok(mut m) => {
                if let Err(e) = m.set(ap::PREPARE_AHEAD_KEY, serde_json::Value::Bool(on)) {
                    rexon_core::log_warn!("axon", "настройка подготовки заранее не сохранена: {e}");
                }
            }
            Err(e) => rexon_core::log_warn!("axon", "настройки не открылись: {e}"),
        }
        if on {
            ap::spawn_background(std::time::Duration::ZERO);
        } else {
            ap::request_stop();
        }
    }

    fn axon_restore_originals(mut self: Pin<&mut Self>) {
        use rexon_core::axon_prepare as ap;
        self.axon_set_prepare_ahead(false);
        let root = axon_env().originals_root();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let report = ap::restore_originals(&root);
            rexon_core::log_info!("axon", "возврат исходных описаний: {report:?}");
            let status = ap::restore_done_text(report);
            if let Err(e) = handle.queue(move |mut object| {
                object.as_mut().apply_finished(QString::from(&status));
            }) {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("axon", "итог возврата описаний не доставлен: {e:?}");
                }
            }
        });
    }

    fn toggle(self: Pin<&mut Self>, ext_id: &QString) -> QString {
        let id = ext_id.to_string();
        match ext_exec::toggle(&id) {
            Ok(true) => QString::from("on"),
            Ok(false) => QString::from("off"),
            // Приставка «!» — часть договора со страницей: по ней она отличает
            // ошибку от нового состояния.
            Err(text) => QString::from(&format!("!{text}")),
        }
    }
}

/// Окружение библиотеки Axon: процесс плюс папки из настроек Rexon.
fn axon_env() -> rexon_core::axon_library::AxonEnv {
    rexon_core::axon_prepare::live_env().0
}

/// Обои набора `ext_id` из каталога расширений — те же записи, что
/// показывает «Библиотека обоев» в «Настройках». Пусто — расширение не набор
/// обоев либо файлов в нём нет.
fn pack_wallpapers(ext_id: &str) -> Vec<rexon_core::wallpaper_engine::LibraryItem> {
    let dir = qe::extensions_dir();
    rexon_core::wallpaper_engine::list_installed_wallpapers(&dir.to_string_lossy())
        .into_iter()
        .filter(|item| item.id == ext_id)
        .collect()
}

/// Доставить итог фоновой работы в поток интерфейса.
///
/// Ни `unwrap()`, ни предварительной проверки «жив ли объект»: между проверкой
/// и постановкой страница успевает закрыться, и это не ошибка — просто отчёт
/// показывать уже некому.
fn report(
    handle: cxx_qt::CxxQtThread<qobject::ExtensionsActions>,
    outcome: Result<String, String>,
    install: bool,
) {
    let (ok, message) = match outcome {
        Ok(text) => (true, text),
        Err(text) => (false, text),
    };
    let queued = handle.queue(move |mut object| {
        let text = QString::from(&message);
        if install {
            object.as_mut().install_finished(ok, text);
        } else {
            object.as_mut().uninstall_finished(ok, text);
        }
    });
    if let Err(e) = queued {
        if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
            // Молчать нельзя: итог установки пропал, и вкладка осталась бы со
            // строкой «Устанавливаю…» навсегда.
            rexon_core::log_warn!("extensions", "итог действия «Расширений» не доставлен: {e:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Договор со страницей: ответ переключения — «on», «off» либо текст с
    /// приставкой «!». Страница разбирает его именно так
    /// (`answer.substring(1)`), и третьей формы у ответа быть не может.
    #[test]
    fn toggle_answer_of_unknown_extension_is_marked_as_error() {
        let answer = match ext_exec::toggle("rexon-нет-такого-расширения") {
            Ok(true) => "on".to_string(),
            Ok(false) => "off".to_string(),
            Err(text) => format!("!{text}"),
        };
        assert!(
            answer.starts_with('!'),
            "неизвестное расширение обязано ответить ошибкой, а не «выключено»: {answer}"
        );
    }
}
