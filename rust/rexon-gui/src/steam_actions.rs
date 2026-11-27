// SteamActions — the side-actions object of the "Steam" tab in Rust.
//
// WHY. `SteamPage.qml` expects, as the `actions` property, an object with
// eight methods: the showcase snapshot (`scan`), the picker window for your
// own clip (`pickFile`), choosing (`select`), the star (`setFavorite`),
// three toggles (`setSound`, `setRotation`, `setAutostart`) and the
// full-screen show (`play`). While there was no such object in Rust, the
// "Steam" section under the Rust window host opened ALIVE IN LOOKS ONLY:
// the showcase was empty, the buttons pressed and did nothing. Here the
// object appears, and the section starts working.
//
// Warning: the ninth method, `webapiToken()`, has not existed since
// 13.10.2026. It went to the network from the interface thread, and since
// 08.09.2026 it was moreover not visible from QML (`#[qinvokable]` was
// lost), so the page silently lived without a token and without animated
// backgrounds. The token is obtained by the inventory background task
// (`steam_workers.rs`, `start_inventory`).
//
// The contract with the page is taken from the Python `SteamActions` class
// (`rexon/qt_steam/qml_window.py`) TO THE LETTER — the same names, the same
// answer types. So the page markup did not change by a single line.
//
// WHERE THE LOGIC LIVES. There is none of our own here at all:
//   * WHAT to do with the splash — the `rexon_core::session_splash::Splash`
//     flow (scanning Steam clips, config, favourites, rotation, the KSplash
//     hybrid, autostart, all three run paths);
//   * it is DONE by the executor `rexon_core::splash_exec::LiveSplashIo` —
//     the very one the live Python facade works with too;
//   * WHAT to tell the user — the core's texts (`rexon_core::qt_steam`), the
//     same ones the widget tab shows;
//
// Warning: WHAT HAD TO BE MOVED FOR THIS OBJECT (both changes are not a side
// effect but a condition for no second copy of the rules appearing):
//   1. The live splash executor moved from `rexon-py` into the core
//      (`splash_exec.rs`). In `rexon-py` it was unreachable: that crate is
//      built only as a `cdylib` for Python.
//   2. Exchanging the Steam client cookie for a `webapi_token` was added to
//      the core (`steam_token.rs`). It alone remained on Python's `urllib`,
//      although the core's network layer has long existed and the tab uses
//      it for the inventory too.
//
// Warning: ALL METHODS ARE SYNCHRONOUS — as in the Python original. Each of
// them either reads a ready snapshot or writes one file, and the page needs
// the answer immediately; long jobs (network, thumbnails, preview frames)
// are run by the tab itself as tokio tasks (`steam_workers.rs`). Running in
// the background, as in "Extensions", would not be an improvement here but
// a breach of the contract: the page reads the answer on the same line it
// made the call on.

/// cxx-qt bridge of the actions object.
///
/// The type is declared `#[qml_element]`, so the window host creates it by
/// itself (`SteamActions { }`) and hands it to the page as the `actions`
/// property. No registration in `main_window.rs` is needed for this: that
/// file holds the section map, not a list of side objects.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type SteamActions = super::SteamActionsRust;
    }

    extern "RustQt" {
        /// The splash showcase and the state of the toggles as ONE snapshot:
        /// `{movies, current, favorites, enabled, rotate, sound}`.
        ///
        /// An empty snapshot is not a breakage: a machine without Steam has no
        /// clips, and the page will honestly say so in the core's words.
        #[qinvokable]
        fn scan(self: &SteamActions) -> QVariant;

        /// Picker window for your own video. An empty string means the user changed
        /// their mind.
        #[qinvokable]
        #[cxx_name = "pickFile"]
        fn pick_file(self: &SteamActions) -> QString;

        /// Remember the chosen clip in the splash config.
        #[qinvokable]
        fn select(self: &SteamActions, path: &QString) -> bool;

        /// Toggle the star. Returns the CURRENT favourites list rather than "did it
        /// work": the page keeps the favourites on its side, and two calls in a row
        /// ("toggle", "now read it") could disagree with the disk.
        #[qinvokable]
        #[cxx_name = "setFavorite"]
        fn set_favorite(self: &SteamActions, path: &QString) -> QVariant;

        /// Play the splash full screen. "" means it started, otherwise the reason.
        #[qinvokable]
        fn play(self: &SteamActions, path: &QString, sound: bool) -> QString;

        /// Where Steam is — the same answer the model finds by itself. Exposed for
        /// the comparison: a divergence means the page has been left without a root
        /// and the backgrounds section will be empty.
        #[qinvokable]
        #[cxx_name = "steamRoot"]
        fn steam_root(self: &SteamActions) -> QString;

        /// Splash autostart at login. "" means it worked, otherwise the reason.
        #[qinvokable]
        #[cxx_name = "setAutostart"]
        fn set_autostart(self: &SteamActions, on: bool) -> QString;

        /// A random splash on every login.
        #[qinvokable]
        #[cxx_name = "setRotation"]
        fn set_rotation(self: &SteamActions, on: bool) -> bool;

        /// Splash sound. Written to the config together with the chosen clip.
        #[qinvokable]
        #[cxx_name = "setSound"]
        fn set_sound(self: &SteamActions, on: bool) -> bool;
    }
}

use cxx_qt_lib::{QString, QVariant};
use serde_json::{Map, Value};

use rexon_core::i18n::tr;
use rexon_core::qt_steam as qs;
use rexon_core::session_splash::{self as ss, Splash};
use rexon_core::splash_exec::LiveSplashIo;

/// Rust state of the object: empty, and this is not an oversight.
///
/// The splash executor is created FOR ONE call — just as the Python facade
/// does it (`session_splash._io()`). Keeping it as a field is not possible:
/// it holds descriptors of its own processes and X11 layers inside, while
/// the actions object lives as long as the window.
#[derive(Default)]
pub struct SteamActionsRust;

impl qobject::SteamActions {
    fn steam_root(&self) -> QString {
        let mut io = live_io();
        let home = home();
        QString::from(
            &Splash::new(&mut io, &home)
                .steam_root()
                .unwrap_or_default(),
        )
    }

    fn scan(&self) -> QVariant {
        let mut io = live_io();
        let mut splash = Splash::new(&mut io, &home());

        let cfg = splash.load_config();
        let current = cfg
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let mut movies = splash.scan_startup_movies();
        // Whether to append OUR OWN clip is decided by the core; from here only the
        // observation "the file exists on disk" comes. Otherwise the customer's
        // choice would disappear from view without getting into the Steam scan.
        let scanned: Vec<String> = movies
            .iter()
            .map(|m| m.get("path").and_then(Value::as_str).unwrap_or("").to_string())
            .collect();
        let exists = !current.is_empty() && std::path::Path::new(&current).is_file();
        if qs::needs_custom_movie(&current, &scanned, exists) {
            let (basename, stem) = name_and_stem(&current);
            movies.push(ss::describe_custom(&current, &basename, &stem));
        }

        let favorites = splash.favorites();
        let enabled = splash.is_enabled();
        let rotate = splash.rotation_enabled();
        let sound = cfg
            .get("sound")
            .map(|v| v.as_bool().unwrap_or(true))
            .unwrap_or(true);

        crate::qvalue::json_to_variant(&snapshot(
            movies, &current, &favorites, enabled, rotate, sound,
        ))
    }

    fn pick_file(&self) -> QString {
        // Title and filter come from the core: the same as the widget tab showed.
        // The initial directory is the Steam root, and if there is none, home.
        let mut io = live_io();
        let start = Splash::new(&mut io, &home())
            .steam_root()
            .unwrap_or_else(home);
        // The dialog is brought up by the crate's SHARED shim
        // (`cpp/rexon_ext_dialogs.h`): a synchronous answer, a nested event loop,
        // the desktop portal on Wayland. There is deliberately no second
        // declaration of the same C++ function in this bridge — cxx emits the shim
        // by the function name, and a duplicate would give a `duplicate symbol` at
        // link time.
        crate::extensions_actions::qobject::pick_file(
            &QString::from(&qs::sv_pick_dialog_title()),
            &QString::from(&qs::sv_pick_filter()),
            &QString::from(&start),
        )
    }

    fn select(&self, path: &QString) -> bool {
        let path = path.to_string();
        if path.is_empty() {
            return false;
        }
        let mut io = live_io();
        let mut splash = Splash::new(&mut io, &home());
        // The sound is taken from the config, not invented: the page toggles it
        // with a separate action, and a default of our own would silently reset
        // the customer's choice.
        let sound = splash
            .load_config()
            .get("sound")
            .map(|v| v.as_bool().unwrap_or(true))
            .unwrap_or(true);
        splash.save_config(&path, sound).is_none()
    }

    fn set_favorite(&self, path: &QString) -> QVariant {
        let path = path.to_string();
        let mut io = live_io();
        let mut splash = Splash::new(&mut io, &home());
        // Warning: a fix made during the port (rule #16): an empty path returns
        // what IS ON DISK, not an empty list as Python did. The list goes straight
        // into `page.setFavorites(...)`, and an empty answer would wipe all the
        // stars at once — on screen that is indistinguishable from "the user
        // removed them".
        if path.is_empty() {
            return crate::qvalue::json_to_variant(&strings_to_json(&splash.favorites()));
        }
        let current = splash.favorites();
        let on = !current.iter().any(|p| p == &path);
        let (favorites, err) = splash.set_favorite(&path, on);
        // Warning: a second fix of the same kind: if the config was not written, we
        // return the PREVIOUS list. The core returns the desired one in this case,
        // and the star would light up on screen without surviving a restart.
        let favorites = if err.is_some() { current } else { favorites };
        crate::qvalue::json_to_variant(&strings_to_json(&favorites))
    }

    fn play(&self, path: &QString, sound: bool) -> QString {
        let path = path.to_string();
        if path.is_empty() {
            return QString::from(&tr(qs::SV_TEST_NO_ITEM));
        }
        let mut io = live_io();
        // `blocking=false` — control returns at once: this is a check from the
        // interface, not a login. `hybrid=false` — the ksplash→mpv relay has
        // nothing to do with it, as in the widget tab.
        let (ok, msg) = Splash::new(&mut io, &home()).play(
            Some(&path),
            Some(sound),
            false,
            Some(false),
            &repo_root(),
        );
        QString::from(&reason(ok, msg))
    }

    fn set_autostart(&self, on: bool) -> QString {
        let mut io = live_io();
        let mut splash = Splash::new(&mut io, &home());
        let (ok, msg) = if on {
            splash.install_autostart()
        } else {
            splash.uninstall_autostart()
        };
        QString::from(&reason(ok, msg))
    }

    fn set_rotation(&self, on: bool) -> bool {
        let mut io = live_io();
        Splash::new(&mut io, &home()).set_rotation(on).is_none()
    }

    fn set_sound(&self, on: bool) -> bool {
        let mut io = live_io();
        let mut splash = Splash::new(&mut io, &home());
        let path = splash
            .load_config()
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if path.is_empty() {
            // No clip is chosen yet — there is nowhere to write the sound; the choice
            // will save it together with the path.
            return false;
        }
        splash.save_config(&path, on).is_none()
    }
}

// ============================================================================
// Observations the core does not have
// ============================================================================

/// Home directory (the original's `os.path.expanduser("~")`).
fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

/// The live splash executor — FOR ONE call, as in the Python facade.
///
/// Never: no interpreter is passed or implied: since 16.09.2026 (block H) no
/// splash path calls it. The autostart line and the unit's `ExecStart=` are
/// built by `rexon_core::app::launch_program` — the window binary for a Rust
/// entry, the `rexon` launcher with its full path as a fallback, and NOTHING
/// (a refusal out loud) if neither was found. The constructor's second
/// argument remained only for the sake of the signature held by the
/// `rexon-py` bridge (`splash_io::PyLiveSplashIo`); it will go away together
/// with the bridge.
fn live_io() -> LiveSplashIo {
    LiveSplashIo::new(&home(), "")
}

/// Root of the development tree: the core looks for the built
/// `rexon-splash-overlay` there.
///
/// Warning: it is needed by exactly one run path — the blocking Wayland show
/// at login, which never happens from the tab (`play` is called with
/// `blocking=false`). The value is neither invented nor derived here: the
/// rule "what counts as a developer tree" is one for the whole product and
/// lives in the core (rule #12). If it was not derived — an empty string,
/// and the core will fall back to searching `PATH`, as it knows how.
fn repo_root() -> String {
    rexon_core::cli::repo_root()
}

// ============================================================================
// Translation odds and ends
// ============================================================================

/// `(file name, name without extension)` — what Python computed with
/// `os.path.basename` and `os.path.splitext` before `describe_custom`.
fn name_and_stem(path: &str) -> (String, String) {
    let p = std::path::Path::new(path);
    let basename = p
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    (basename, stem)
}

/// The answer of an action that either worked or named the reason.
///
/// An empty string means success. A refusal without words is replaced with
/// the core's generic reason: a silent button reads as a breakage (rule
/// #13).
fn reason(ok: bool, message: String) -> String {
    if ok {
        String::new()
    } else if message.is_empty() {
        qs::sv_no_reason()
    } else {
        message
    }
}

fn strings_to_json(items: &[String]) -> Value {
    Value::Array(items.iter().map(|s| Value::from(s.as_str())).collect())
}

/// Snapshot keys the page reads with a dot (`snapshot.movies`,
/// `snapshot.rotate`…). The list is named separately not for looks: a
/// forgotten key arrives in QML as `undefined`, and `undefined` in a toggle
/// is a silent "off". Such a divergence is visible neither at build time
/// nor to the eye.
const SNAPSHOT_KEYS: [&str; 6] = ["movies", "current", "favorites", "enabled", "rotate", "sound"];

/// Build the snapshot of the showcase and the toggles — the only place where
/// the composition of the `scan()` answer is written down.
fn snapshot(
    movies: Vec<Value>,
    current: &str,
    favorites: &[String],
    enabled: bool,
    rotate: bool,
    sound: bool,
) -> Value {
    // The map is built FROM THE LIST of keys, not with scattered inserts: that
    // way the composition of the answer is written exactly once, and the guard
    // below checks the very list the page is filled from.
    let values = [
        Value::Array(movies),
        Value::from(current),
        strings_to_json(favorites),
        Value::from(enabled),
        Value::from(rotate),
        Value::from(sound),
    ];
    let mut out = Map::new();
    for (key, value) in SNAPSHOT_KEYS.into_iter().zip(values) {
        out.insert(key.to_string(), value);
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contract with the page: a refusal ALWAYS comes with words. The page
    /// understands an empty string as "it worked" and writes "▶ Playing…" into
    /// the state — that is, a silent refusal would show the user a success.
    #[test]
    fn refusal_is_never_wordless() {
        assert_eq!(reason(true, String::new()), "");
        assert_eq!(reason(true, "лишнее".into()), "");
        assert_eq!(reason(false, String::new()), qs::sv_no_reason());
        assert_eq!(reason(false, "нет файла".into()), "нет файла");
    }

    /// Our own clip's name for the showcase is computed from the FILE, not from
    /// the whole path: the card is labelled with `title`, and the whole path
    /// would not fit into it.
    #[test]
    fn custom_movie_is_named_by_its_file() {
        let (basename, stem) = name_and_stem("/дом/видео/Мой ролик.webm");
        assert_eq!(basename, "Мой ролик.webm");
        assert_eq!(stem, "Мой ролик");
        let (basename, stem) = name_and_stem("");
        assert_eq!(basename, "");
        assert_eq!(stem, "");
    }

    /// The snapshot must name ALL six keys — on an empty machine too. The page
    /// reads them with a dot, and a forgotten key would arrive in it as
    /// `undefined`, that is, the toggle would silently go to "off".
    ///
    /// Warning: the names here are written out ANEW, not taken from
    /// `SNAPSHOT_KEYS`. Comparing with the list that is itself being checked
    /// would go green on any renaming of it — that is, it would check nothing.
    /// The real source of these six words is `qml/SteamPage.qml`
    /// (`snapshot.movies`, `snapshot.rotate`…), and they can only be rewritten
    /// together with it.
    #[test]
    fn snapshot_names_every_key_the_page_reads() {
        let wanted = ["movies", "current", "favorites", "enabled", "rotate", "sound"];
        let empty = snapshot(Vec::new(), "", &[], false, false, true);
        let map = empty.as_object().expect("снимок обязан быть объектом");
        for key in wanted {
            assert!(map.contains_key(key), "в снимке нет ключа «{key}»");
        }
        assert_eq!(map.len(), wanted.len(), "в снимке появился лишний ключ");
    }

    /// The field types of the snapshot hold the markup: `movies` and
    /// `favorites` are lists, the three toggles are true/false. If the string
    /// "false" arrived here, QML would take it for true (a non-empty string),
    /// and the toggle would go to "on" by itself.
    #[test]
    fn snapshot_field_types_are_what_qml_expects() {
        let full = snapshot(
            vec![Value::Object(Map::new())],
            "/дом/ролик.webm",
            &["/дом/ролик.webm".to_string()],
            true,
            false,
            true,
        );
        assert!(full["movies"].is_array());
        assert!(full["favorites"].is_array());
        assert_eq!(full["current"], Value::from("/дом/ролик.webm"));
        assert!(full["enabled"].is_boolean());
        assert!(full["rotate"].is_boolean());
        assert!(full["sound"].is_boolean());
        assert_eq!(full["favorites"][0], Value::from("/дом/ролик.webm"));
    }
}
