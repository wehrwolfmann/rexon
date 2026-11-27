// WallpaperPlaylistActions — the "Wallpaper playlist" block in "⚙️
// Settings" of the "HD Wallpapers" section and the "To playlist" button of
// Razer Axon wallpapers.
//
// There is no logic here: the model, edits, snapshot and labels come from
// the core (`rexon_core::wallpaper_playlist`). Changing the wallpaper goes
// through the same executor as for the timer and the tray — outside the
// GUI thread; the result arrives as a signal.

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
        /// The wallpaper change has finished (text for the user; empty — silently).
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperPlaylistActions>, message: QString);
    }

    extern "RustQt" {
        /// Block labels (JSON).
        #[qinvokable]
        fn texts(self: &WallpaperPlaylistActions) -> QString;

        /// Block snapshot (JSON `wallpaper_playlist::view`).
        #[qinvokable]
        fn state(self: &WallpaperPlaylistActions) -> QString;

        /// An edit (JSON `{"op": …}`). The answer is `{"ok", "message"}`.
        #[qinvokable]
        fn edit(self: &WallpaperPlaylistActions, op: &QString) -> QString;

        /// `next`, `prev`, `pause` — the change happens in the background, the
        /// result comes with the `finished` signal.
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
