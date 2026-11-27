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
            weight: i32,
            italic: bool,
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
                    qs::wallpaper_done_warned(res.ok, &res.message, res.warning.as_deref())
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
