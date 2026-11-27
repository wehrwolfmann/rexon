// SoundPreviewActions — "listen" buttons for sound themes (the "Sounds"
// section of the "Appearance" tab and the Store item details).
//
// There is no logic here: the five sounds, finding a file by the
// specification, choosing a player and unpacking a Store archive belong to
// the core (`rexon_core::sound_preview`). The object only translates QML
// calls into core calls. Downloading an archive for listening runs in a
// worker thread, and the result comes with the `storePreviewReady` signal.
//
// A separate object rather than methods of `AppearanceActions`: that way
// the sounds are hooked up in QML with one line, without touching the tab
// object.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type SoundPreviewActions = super::SoundPreviewActionsRust;
    }

    extern "RustQt" {
        /// Five sounds of an installed theme (JSON
        /// `{"theme", "installed", "sounds": [{"key","event","file"}]}`).
        /// `installed: false` means there is no theme with that name on disk.
        #[qinvokable]
        #[cxx_name = "themeSounds"]
        fn theme_sounds(self: &SoundPreviewActions, theme: &QString) -> QString;

        /// Play an event of an installed theme. Empty means it started playing;
        /// otherwise a phrase for the user (nothing to play with, no sound).
        #[qinvokable]
        #[cxx_name = "playThemeSound"]
        fn play_theme_sound(self: &SoundPreviewActions, theme: &QString, event: &QString) -> QString;

        /// Play a file (sounds of a Store theme before installation).
        #[qinvokable]
        #[cxx_name = "playFile"]
        fn play_file(self: &SoundPreviewActions, path: &QString) -> QString;

        /// Download a Store item into a temporary cache and find the five sounds.
        /// The result comes with the `storePreviewReady` signal.
        #[qinvokable]
        #[cxx_name = "previewStoreTheme"]
        fn preview_store_theme(self: Pin<&mut SoundPreviewActions>, content_id: &QString, download_url: &QString);

        /// Lift themes that earlier installations put deeper (the same as the
        /// tab's first scan does). Log lines separated by `\n`.
        #[qinvokable]
        #[cxx_name = "migrateNestedThemes"]
        fn migrate_nested_themes(self: &SoundPreviewActions) -> QString;

        /// Result of `previewStoreTheme` (JSON
        /// `{"contentId","ok","error","themes":[{"theme","root","sounds"}]}`).
        #[qsignal]
        #[cxx_name = "storePreviewReady"]
        fn store_preview_ready(self: Pin<&mut SoundPreviewActions>, payload: QString);

        /// Download progress of `previewStoreTheme`: bytes received and the total
        /// (`0` — the server did not name the size). Sent no more often than the
        /// percentage changes. The numbers are `f64`: QML does not accept a 64-bit
        /// integer (the `qml_int64` guard in `src/lib.rs`), and bytes are far from
        /// 2^53.
        #[qsignal]
        #[cxx_name = "storePreviewProgress"]
        fn store_preview_progress(self: Pin<&mut SoundPreviewActions>, content_id: QString, done: f64, total: f64);
    }

    impl cxx_qt::Threading for SoundPreviewActions {}
}

use core::pin::Pin;

use cxx_qt::Threading;
use cxx_qt_lib::QString;
use rexon_core::qt_settings as qs;
use rexon_core::sound_preview as sp;
use serde_json::json;

#[derive(Default)]
pub struct SoundPreviewActionsRust;

fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

/// Play the argv from the core; `None` means there is no sound or nothing
/// to play with (with a hint which package to install — the same words as
/// in the sounds section of "Settings").
fn play(argv: Option<Vec<String>>, theme: &str, file: &str) -> QString {
    let Some(argv) = argv else {
        if file.is_empty() {
            return QString::from(&qs::msg_no_event_in_theme(theme));
        }
        let family = rexon_core::system::DistroDetector::detect_live()
            .family_live()
            .as_str()
            .to_string();
        let hint = rexon_core::utils::tool_package_hint(
            rexon_core::utils::sound_player_suggestion(file),
            &family,
        );
        return QString::from(&rexon_core::utils::msg_no_sound_player(&hint));
    };
    match sp::live_play(&argv) {
        Ok(()) => QString::default(),
        Err(e) => QString::from(&e),
    }
}

impl qobject::SoundPreviewActions {
    fn theme_sounds(&self, theme: &QString) -> QString {
        let theme = theme.to_string();
        let dirs = qs::live_xdg_sound_theme_dirs();
        let installed = qs::sound_theme_exists(&dirs, &theme);
        let resolve = sp::resolver_for(dirs);
        let sounds = if installed { sp::preview_sounds(&resolve, &theme) } else { Vec::new() };
        QString::from(&json!({ "theme": theme, "installed": installed, "sounds": sounds }).to_string())
    }

    fn play_theme_sound(&self, theme: &QString, event: &QString) -> QString {
        let (theme, event) = (theme.to_string(), event.to_string());
        let dirs = qs::live_xdg_sound_theme_dirs();
        if !qs::sound_theme_exists(&dirs, &theme) {
            return QString::from(&qs::sound_theme_missing_message(&theme));
        }
        let resolve = sp::resolver_for(dirs);
        let file = sp::find_event_file(&resolve, &theme, &event).unwrap_or_default();
        let available = sp::live_available_players();
        play(sp::play_argv(&available, true, &theme, &event, &file), &theme, &file)
    }

    fn play_file(&self, path: &QString) -> QString {
        let path = path.to_string();
        let available = sp::live_available_players();
        play(sp::play_argv(&available, false, "", "", &path), "", &path)
    }

    fn preview_store_theme(self: Pin<&mut Self>, content_id: &QString, download_url: &QString) {
        let (id, url) = (content_id.to_string(), download_url.to_string());
        let handle = self.qt_thread();
        std::thread::spawn(move || {
            let cache = rexon_core::utils::cache_home(std::env::var("XDG_CACHE_HOME").ok().as_deref(), &home());
            let root = sp::preview_cache_root(&cache);
            let progress = handle.clone();
            let progress_id = id.clone();
            let mut last: i64 = -1;
            let mut net = rexon_core::net::UreqNet::new().with_progress(move |done, total| {
                // A percentage if the size is known, otherwise every 256 KiB: we do not
                // flood the main thread's queue on every chunk.
                let step = if total > 0 { done * 100 / total } else { done / (256 * 1024) };
                if step == last {
                    return;
                }
                last = step;
                let cid = progress_id.clone();
                let _ = progress.queue(move |mut object| {
                    object.as_mut().store_preview_progress(
                        QString::from(&cid),
                        crate::qvalue::i64_to_qml(done),
                        crate::qvalue::i64_to_qml(total),
                    );
                });
            });
            let installed = qs::live_xdg_sound_theme_dirs();
            let payload = match sp::prepare_store_preview(&mut net, &root, &id, &url, &installed) {
                Ok(themes) => json!({ "contentId": id, "ok": true, "error": "", "themes": themes }),
                Err(e) => json!({ "contentId": id, "ok": false, "error": e, "themes": [] }),
            }
            .to_string();
            let sent = handle.queue(move |mut object| {
                object.as_mut().store_preview_ready(QString::from(&payload));
            });
            if let Err(e) = sent {
                if !matches!(e, cxx_qt::ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("sounds", "прослушивание темы из Магазина не доставлено: {e:?}");
                }
            }
        });
    }

    fn migrate_nested_themes(&self) -> QString {
        QString::from(&rexon_core::appearance_installer::live_migrate_nested_themes(&home()).join("\n"))
    }
}
