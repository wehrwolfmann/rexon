// WallpaperPlaylistActions — блок «Плейлист обоев» в «⚙️ Настройках»
// раздела «Обои HD» и кнопка «В плейлист» у обоев Razer Axon.
//
// Логики здесь нет: модель, правки, снимок и подписи — ядро
// (`rexon_core::wallpaper_playlist`). Смена обоев идёт тем же исполнителем,
// что у таймера и лотка, — вне потока GUI; итог приходит сигналом.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type WallpaperPlaylistActions = super::WallpaperPlaylistActionsRust;
    }

    unsafe extern "RustQt" {
        /// Смена обоев закончилась (текст для человека; пусто — молча).
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperPlaylistActions>, message: QString);
    }

    extern "RustQt" {
        /// Подписи блока (JSON).
        #[qinvokable]
        fn texts(self: &WallpaperPlaylistActions) -> QString;

        /// Снимок блока (JSON `wallpaper_playlist::view`).
        #[qinvokable]
        fn state(self: &WallpaperPlaylistActions) -> QString;

        /// Правка (JSON `{"op": …}`). Ответ — `{"ok", "message"}`.
        #[qinvokable]
        fn edit(self: &WallpaperPlaylistActions, op: &QString) -> QString;

        /// `next`, `prev`, `pause` — смена в фоне, итог сигналом `finished`.
        #[qinvokable]
        fn command(self: Pin<&mut WallpaperPlaylistActions>, name: &QString);
    }

    impl cxx_qt::Threading for WallpaperPlaylistActions {}
}

use core::pin::Pin;

use cxx_qt::Threading;
use cxx_qt_lib::QString;
use rexon_core::wallpaper_playlist::{self as wp, Command, Outcome};

#[derive(Default)]
pub struct WallpaperPlaylistActionsRust;

impl qobject::WallpaperPlaylistActions {
    fn texts(&self) -> QString {
        QString::from(&wp::texts().to_string())
    }

    fn state(&self) -> QString {
        QString::from(&wp::live_view().to_string())
    }

    fn edit(&self, op: &QString) -> QString {
        QString::from(&wp::live_edit(&op.to_string()).to_string())
    }

    fn command(self: Pin<&mut Self>, name: &QString) {
        let cmd = match name.to_string().as_str() {
            "next" => Command::Next,
            "prev" => Command::Prev,
            "pause" => Command::TogglePause,
            _ => return,
        };
        let handle = self.qt_thread();
        std::thread::spawn(move || {
            let message = match wp::live_command(cmd) {
                Outcome::Failed(errs) => format!("❌ {}", errs.join("\n")),
                _ => String::new(),
            };
            let _ = handle.queue(move |mut object| {
                object.as_mut().finished(QString::from(&message));
            });
        });
    }
}
