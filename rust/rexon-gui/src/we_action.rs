// Action — QML-синглтон общих действий вкладки Wallpaper Engine.
//
// Порт
// `ui/src/action.cpp` waywallen (MIT, (c) hypengw; атрибуция в NOTICE).
//
// ⚠️ Две починки по сравнению с нашим Python-мостом — эталон ОРИГИНАЛ:
//
//   1. `toast` у оригинала — СИГНАЛ с дефолтными аргументами, его слушает
//      вендорный `Window.qml:86` (`function onToast(text, duration, flags,
//      action)`). Python-мост переименовал сигнал в `toasted`, а `toast`
//      сделал слотом, и обработчик вендора не срабатывал НИ РАЗУ: тосты
//      видел только наш шелл. Здесь `toast` вернулся сигналом с теми же
//      дефолтами — он живёт в moc'аемой базе `cpp/rexon_we_action_base.h`
//      (почему именно так — разобрано в шапке того файла).
//   2. `copyToClipboard` в Python-мосте НЕ БЫЛО ВОВСЕ, а вендорный
//      `Global.qml:44` зовёт его на кнопке «Скопировать» в тосте ошибки:
//      нажатие падало в TypeError и текст в буфер не попадал.
//
// Третья, мелкая: `enterWallpaperSelect` у оригинала шлёт
// `wallpaperSelectStorageChanged` ТОЛЬКО при смене хранилища, наш слал
// всегда (лишний пересчёт привязок на каждом входе в режим выбора).

/// Мост cxx-qt: синглтон `Action`.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("rexon_we_action_base.h");
        /// База с сигналом `toast` и его дефолтными аргументами.
        ///
        /// ⚠️ Без `#[qobject]` — на каждый `#[qobject]`-тип в `extern "C++"`
        /// cxx-qt печатает пару шимов приведения по имени ТИПА, и второй
        /// такой мост в крейте дал бы `duplicate symbol` на линковке (та же
        /// грабля, что с базой моделей).
        type RexonWeActionBase;
    }

    unsafe extern "C++" {
        // QObject приходится объявить руками: cxx-qt печатает объявление сам
        // только для QObject'ов без базы, а у Action база своя (см. выше).
        #[namespace = ""]
        type QObject = cxx_qt::QObject;

        include!("rexon_we_singletons.h");

        /// Запомнить хранилище выбора под QPointer'ом.
        ///
        /// Не поле структуры: у оригинала здесь `QPointer<QObject>`, и это
        /// не украшение — хранилище живёт в QML-странице и умирает вместе с
        /// ней, а синглтон переживает страницу. Голый указатель после этого
        /// стал бы висячим, и первое же чтение свойства из QML уронило бы
        /// вкладку.
        ///
        /// # Safety
        /// `storage` — либо nullptr, либо живой QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_we_action_set_storage(storage: *mut QObject);
        fn rexon_we_action_storage() -> *mut QObject;

        /// Текст в буфер обмена (QGuiApplication::clipboard()).
        fn rexon_we_copy_to_clipboard(text: &QString);
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[base = RexonWeActionBase]
        #[qproperty(*mut QObject, wallpaper_select_storage,
                    cxx_name = "wallpaperSelectStorage",
                    READ = wallpaper_select_storage,
                    NOTIFY = wallpaper_select_storage_changed)]
        type Action = super::ActionRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "wallpaperSelectStorageChanged"]
        fn wallpaper_select_storage_changed(self: Pin<&mut Action>);

        #[qsignal]
        #[cxx_name = "wallpaperSelectEntered"]
        unsafe fn wallpaper_select_entered(self: Pin<&mut Action>, storage: *mut QObject);
    }

    unsafe extern "RustQt" {
        #[cxx_name = "wallpaperSelectStorage"]
        fn wallpaper_select_storage(self: &Action) -> *mut QObject;

        /// Войти в режим выбора обоей с этим хранилищем.
        #[qinvokable]
        #[cxx_name = "enterWallpaperSelect"]
        unsafe fn enter_wallpaper_select(self: Pin<&mut Action>, storage: *mut QObject);
    }

    extern "RustQt" {
        /// Скопировать текст в буфер обмена (кнопка тоста ошибки).
        #[qinvokable]
        #[cxx_name = "copyToClipboard"]
        fn copy_to_clipboard(self: Pin<&mut Action>, text: &QString);
    }
}

use core::pin::Pin;
use cxx_qt_lib::QString;

/// Своего состояния у синглтона нет: единственное поле оригинала —
/// `QPointer` на хранилище выбора, а QPointer живёт в шиме (см. шапку).
#[derive(Default)]
pub struct ActionRust;

impl qobject::Action {
    pub fn wallpaper_select_storage(&self) -> *mut qobject::QObject {
        qobject::rexon_we_action_storage()
    }

    /// # Safety
    /// `storage` — либо nullptr, либо живой QObject: шим кладёт его под
    /// `QPointer`, а тот читает мета-объект по этому адресу.
    pub unsafe fn enter_wallpaper_select(mut self: Pin<&mut Self>, storage: *mut qobject::QObject) {
        // Сигнал смены — только при РЕАЛЬНОЙ смене (ветка оригинала).
        if qobject::rexon_we_action_storage() != storage {
            unsafe { qobject::rexon_we_action_set_storage(storage) };
            self.as_mut().wallpaper_select_storage_changed();
        }
        // unsafe: аргумент — сырой указатель; сам вызов ничего не разыменовывает.
        unsafe { self.as_mut().wallpaper_select_entered(storage) };
    }

    pub fn copy_to_clipboard(self: Pin<&mut Self>, text: &QString) {
        qobject::rexon_we_copy_to_clipboard(text);
    }
}
