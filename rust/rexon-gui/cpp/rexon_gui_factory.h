// Factory of child QObjects for the cxx-qt bridge.
//
// Why a shim: cxx-qt cannot create an instance of ITS OWN QObject from Rust
// — the generated class exists only in C++, and `new` is there. Exactly
// this operation is required by the architecture of the vendor's QML: a
// query owns a model and exposes it as a property
// (`Q_PROPERTY(Model* model READ model CONSTANT)`), as done in waywallen and
// in the Python bridge (RemoteSearchQuery.model).
//
// The shim is generic in meaning but not in type: every new owned object
// adds one function here — that is cheaper than dragging templates through
// cxx.
#pragma once

#include <cstddef>

class QObject;

/// Create a RemoteListModel. parent may be nullptr — then ownership stays
/// with the caller and the object must be removed with
/// rexon_delete_object(). A null parent is needed by the late callbacks
/// bench: it deliberately deletes the model while an answer is still in
/// flight.
QObject* rexon_new_remote_list_model(QObject* parent);

/// The same as rexon_new_remote_list_model, but under another name — for
/// the Discover queries bridge.
///
/// Warning: the duplication is forced and must NOT be "simplified": cxx emits
/// a shim wrapper for every `extern "C++"` declaration, and the shim name is
/// built from the C++ function name. The same function declared in TWO
/// bridges of one crate gives a `duplicate symbol` at link time — exactly
/// the same class of trap as with the `#[qobject]` QAbstractListModel base.
QObject* rexon_new_search_result_model(QObject* parent);

/// Let presses of the mouse OUTSIDE `area` through to the window when the
/// Material modal layer (`Qcm.Material.Overlay`) would eat them.
///
/// Why a shim: the Material layer is one per window and blocks presses with
/// an event filter on the window itself — before any scene item sees them.
/// For waywallen the window is the whole interface, while in our window the
/// Rexon sections stand above the wallpaper tab: a press on them with a
/// window open on top ("Plugins", "Settings", "About", any modal question)
/// closed that window and was itself lost. A foreign event filter can only
/// be overtaken with a filter of our own — that cannot be done from QML or
/// from Rust. There are no decisions here: "outside `area`" is geometry, the
/// layer does the rest itself.
///
/// `area` is a scene item (`QQuickItem`) outside of which presses are let
/// through; a repeated call is safe (the filter is moved to the front of the
/// queue — it must stand AFTER the layer's filter, and that one is set up
/// lazily).
void rexon_pass_outside_presses(QObject* area);

/// A QVariant value as JSON — the way QML will see it.
///
/// A shim rather than parsing in Rust: `QVariant::value<T>()` in cxx-qt-lib
/// checks `canConvert`, and in Qt that is permissive — a string "converts"
/// both to bool and to a character list, so the REAL type of the value
/// cannot be determined with it. QJsonDocument::fromVariant does exactly
/// what is needed, and does it by Qt's own rules.
class QByteArray;
class QVariant;
QByteArray rexon_variant_to_json(const QVariant& value);

/// Create the Discover page queries (batch 3). parent may be nullptr — then
/// the object is removed with rexon_delete_object(); from QML they are
/// created by the engine itself, and the factory is needed by the bench and
/// by a future registration shim.
QObject* rexon_new_remote_availability_query(QObject* parent);
QObject* rexon_new_remote_search_query(QObject* parent);
QObject* rexon_new_remote_details_query(QObject* parent);
QObject* rexon_new_remote_download_query(QObject* parent);

/// Single-shot deferred reload timer of a query (qextra query.cpp: 100 ms).
/// When it fires it calls the owner's Q_INVOKABLE method `reload()`, so
/// consecutive property changes are glued into ONE request.
///
/// A shim rather than Rust: cxx-qt-lib 0.9 has no QTimer wrapper at all
/// (core/ has QDate/QTime but no timer), and creating our own #[qobject]
/// timer for a debounce costs more than four lines of C++.
QObject* rexon_new_reload_debounce(QObject* owner, int interval_ms);

/// The same two functions under other names — for the "Wallpapers" page
/// bridge.
///
/// Warning: the duplication is forced and must NOT be "simplified": cxx emits
/// a shim wrapper for every `extern "C++"` declaration, and the shim name is
/// built from the C++ function name. The same function declared in TWO
/// bridges of one crate gives a `duplicate symbol` at link time — the same
/// class of trap as with the results model factory.
QObject* rexon_new_wallpaper_debounce(QObject* owner, int interval_ms);
void rexon_restart_wallpaper_debounce(QObject* timer);

/// A third pair of names — for the bridge of the "Wallpapers" page action
/// queries.
QObject* rexon_new_action_debounce(QObject* owner, int interval_ms);
void rexon_restart_action_debounce(QObject* timer);

/// A fourth pair — for the playlists bridge (`PlaylistDetailQuery`, 0.3.7).
QObject* rexon_new_playlist_debounce(QObject* owner, int interval_ms);
void rexon_restart_playlist_debounce(QObject* timer);

/// Restart the debounce timer (start() of a single-shot QTimer resets the
/// countdown — exactly what is needed: we count from the LAST property
/// change).
void rexon_restart_debounce(QObject* timer);

/// The wallpaper library model, or nullptr if the object is something else.
///
/// A checked cast (qobject_cast), not a bare pointer cast: the `model`
/// property of the selection storage is written by QML, and any QObject can
/// be slipped in there. The original does the same (qextra
/// select_storage.cpp:16).
QObject* rexon_as_wallpaper_list_model(QObject* object);

/// Create a WallpaperListModel as a child of the owner query.
QObject* rexon_new_wallpaper_list_model(QObject* parent);

/// The same two functions under other names — for the playlists bridge
/// (`PlaylistDetailQuery` owns its contents model) and for the bridge of the
/// playlist contents selection storage. The reason for the duplication is
/// the same as for the pairs above: the cxx shim name is built from the C++
/// function name, and the same declaration in two bridges gives a
/// `duplicate symbol` at link time.
QObject* rexon_new_playlist_wallpaper_list_model(QObject* parent);
QObject* rexon_as_playlist_wallpaper_list_model(QObject* object);

/// Eight objects of the "Wallpapers" page for the golden bench
/// (src/wallpaper_page_selftest.rs). In the live Rexon they are created by
/// QML — there the factory is not needed, while the bench needs the same
/// classes without a window and without QML.
QObject* rexon_new_wallpaper_list_query(QObject* parent);
QObject* rexon_new_wallpaper_get_query(QObject* parent);
QObject* rexon_new_wallpaper_apply_query(QObject* parent);
QObject* rexon_new_wallpaper_remove_query(QObject* parent);
QObject* rexon_new_wallpaper_scan_query(QObject* parent);
QObject* rexon_new_wallpaper_property_set_query(QObject* parent);
QObject* rexon_new_wallpaper_layout_set_query(QObject* parent);
QObject* rexon_new_wallpaper_apply_via_portal_query(QObject* parent);
/// waywallen 0.4.4: "hide wallpaper" — the v0.4.4 check of the same bench.
QObject* rexon_new_wallpaper_hide_query(QObject* parent);
/// waywallen 0.5.0: playlist queries — the v0.5.0 check of the same bench
/// (`saveSmart`, answer generations, silence without a number).
QObject* rexon_new_playlist_list_query(QObject* parent);
QObject* rexon_new_playlist_detail_query(QObject* parent);
QObject* rexon_new_playlist_mutation_query(QObject* parent);

/// Delete a QObject created without a parent. With delete, not
/// deleteLater(): the bench needs PRECISELY an immediate death of the
/// object, so that a late callback arrives at a known-dead address.
void rexon_delete_object(QObject* object);

/// Spin the event loop for no longer than ms milliseconds. cxx-qt-lib gives
/// no wrapper for QCoreApplication::processEvents, while the late callbacks
/// bench needs exactly it: the CxxQtThread queue is drained by the event
/// loop.
void rexon_process_events(int ms);

// ── Entry point: what cxx-qt-lib 0.9 does not have at all ───────────────
//
// Both functions have C linkage on purpose: the main window binary calls
// them directly (`extern "C"` in Rust), without a cxx bridge. A bridge here
// would be a redundant link — no QObjects or Qt types cross the boundary,
// only strings and numbers.
extern "C" {

/// The translations directory of Qt ITSELF (`QLibraryInfo::TranslationsPath`)
/// in UTF-8.
///
/// Writes no more than `len - 1` bytes and a NUL into `buf`; → the full
/// length of the path (more than `len - 1` means the buffer is too small,
/// call again with the needed size).
///
/// A shim rather than Rust: cxx-qt-lib 0.9 has no QLibraryInfo wrapper.
/// Which files to load from this directory is decided by the core
/// (`rexon_core::cli::qt_translation_files` by the Rexon language), not by
/// QLocale::system().
int rexon_qt_translations_dir(char* buf, int len);

/// Enable the translation catalogues of Qt ITSELF — the buttons Qt draws
/// ("OK", "Cancel", "Show Details…", the edit menu in text fields).
///
/// `paths` are the full `.qm` paths named by the core, in load order;
/// `count == 0` means English: only remove the previous ones. The previous
/// set is always removed (a language change). Neither the language nor a
/// fallback is chosen here: that is the core's decision, and ready paths
/// arrive here. → how many loaded (-1 — there is no application yet).
///
/// Never: the translator must be a Qt object, not a Python one: a Python
/// translator is called from the QML loading thread and deadlocks the
/// threads through the GIL (the PySide6 canon). There is none here — only
/// C++.
int rexon_install_qt_translations(const char* const* paths, int count);

/// Receiver of a translated string: `target` is what the translator passed,
/// `utf8` and `len` are the ready translation (not NUL-terminated).
typedef void (*RexonTextSink)(void* target, const char* utf8, int len);

/// Looking up a QML string in the core catalogue
/// (`rexon_core::i18n::qt_translate`): `disambiguation` is the
/// disambiguation (`nullptr` — none), `n` is the number (`< 0` — none). If
/// found — hand it to `sink(target, …)` and return true; false means there
/// is no translation (Qt will show the source).
typedef bool (*RexonAppTranslate)(const char* disambiguation, const char* source, int n,
                                  RexonTextSink sink, void* target);

/// The translator of Rexon ITSELF — for `qsTranslate("rexon", …)` in QML.
///
/// It has NO catalogue (`.qm`) of its own: it asks the core through `lookup`
/// for every string of context `context`, and the core looks it up in the
/// same `.mo` as for the Rust strings. The translation is stored in one
/// place and cannot diverge. Foreign contexts (the vendored waywallen, Qt
/// itself) are passed on without going into Rust.
///
/// There is exactly one translator: a repeated call removes the previous
/// one (together with its cache) and installs a new one — a language
/// change. `lookup == nullptr` means only remove: English (the source) has
/// no catalogue. → false if there is no application.
///
/// Qt calls it from any thread (QQmlThread too): `lookup` must be
/// thread-safe, and the string cache here is under a lock.
bool rexon_install_app_translator(const char* context, RexonAppTranslate lookup);

/// Link colour of formatted text across the whole application — the `Link`
/// and `LinkVisited` roles of the application palette (the decision is the
/// core's, `ui_base::skin_link_rgb`). `set == false` means remove: the roles
/// are taken from the system theme again. The other roles are not set and
/// still come from the system. Call from the main thread.
/// Warning: Qt does not repaint already parsed text: the colour goes into
/// the document when the HTML is parsed, so call BEFORE loading QML and on a
/// skin change.
void rexon_set_app_link_color(bool set, unsigned char r, unsigned char g, unsigned char b);

/// Fallback font of a script for the whole application
/// (`QFontDatabase::addApplicationFallbackFontFamily`). `script` is a number
/// from the core's list `qt_app::FallbackScript` (0 — Arabic script, 1 —
/// Hebrew), `family` is UTF-8 of length `len`. Which families is decided by
/// the core (`qt_app::RTL_FALLBACK_FONTS`). An unknown number — do nothing.
/// Call from the main thread after the application is created.
void rexon_add_script_fallback_font(unsigned char script, const char* family, std::size_t len);

/// Drain Qt's SHARED thread pool for no longer than ms milliseconds.
/// → how many tasks were NOT waited for (0 — the pool is empty).
///
/// QThreadPool worker threads are created by C++, and our own background
/// task counter (`rexon_core::exit_guard`) cannot see them in principle —
/// hence a separate count. The pool holds Qt Quick tasks (asynchronous
/// image and delegate loaders): in core dump 2292278 eight of its threads
/// sat in QWaitCondition::wait.
///
/// Warning: the outcome MUST get into the answer. Waiting for the pool as a
/// side effect while answering only with the number of our own tasks means
/// saying "the background is empty" while the background works (measured
/// 22.08.2026 on the Python part): the exit guard then does not cut the
/// process off, and exit hangs until the task ends.
int rexon_drain_thread_pool(int ms);

/// Stop and unload ALL players (`QMediaPlayer`, in QML — `MediaPlayer`) in
/// all windows of the process. → how many were found.
///
/// Called from the GUI thread after leaving the event loop and BEFORE
/// dropping the QML engine. Unloading (`setSource(QUrl())`) is synchronous:
/// Qt destroys the playback engine and waits for its decoding threads — by
/// the return from here not a single FFmpeg thread is running any more. Why
/// this cannot be left to the destructors is in `main_window.rs` at the
/// call.
int rexon_stop_media_players();

/// Main thread heartbeat for the window watchdog (`rexon_core::ui_watch`): a
/// Qt timer on the GUI thread calls `tick` every `ms` milliseconds. A shim
/// rather than Rust: cxx-qt-lib 0.9 has no QTimer wrapper. Call from the GUI
/// thread after the application is created; a repeated call does nothing.
/// → false means there is no application yet.
bool rexon_start_ui_heartbeat(int ms, void (*tick)());

/// Names of ancestor classes of an object from QML — for the action journal
/// (`rexon_core::ui_watch::area_from_chain`). `variant` is a pointer to a
/// QVariant with the object (as QML passed it); ancestors go from the
/// nearest to the farthest through `/`, by the QObject parent, and without
/// one — by the visual parent. Writes no more than `len - 1` bytes of UTF-8
/// and a NUL into `buf`; → the length written. There are no decisions here:
/// which names matter is decided by the core.
int rexon_qml_ancestry(const void* variant, char* buf, int len);

/// Action journal: the input filter of the whole application. A left click
/// (without a drag), Enter and Space on the focused item → `begin` with the
/// description "who took the press and its ancestors" (parsed by
/// `rexon_core::ui_watch::parse_input_chain`) and a key flag; a non-zero
/// answer → `finish` with it once the event has been distributed. A shim
/// rather than Rust: an event filter is a QObject subclass with a virtual
/// `eventFilter`. Call from the GUI thread after the application is
/// created; a repeated call does nothing.
bool rexon_install_input_journal(unsigned long long (*begin)(const char*, int, int),
                                 void (*finish)(unsigned long long));

}  // extern "C"
