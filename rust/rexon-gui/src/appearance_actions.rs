// AppearanceActions — the side-actions object of the «Appearance» tab in Rust.
//
// WHY. `AppearancePage.qml` expects, in its `actions` property, an object
// with seventeen methods: the machine snapshot, the section contents, tile
// pictures and their cancellation, background warm-up, item details,
// carousel frames, comments, search suggestions, installing from the store
// and from AUR, applying a theme from disk, removal, the star, the page in
// the browser and a one-off cleanup of translations. While there was no
// such object in Rust, the «Appearance» section under the Rust window host
// opened looking ALIVE: the sidebar in place, but the showcase empty and
// the stars unlit — there was nobody to ask the network and the disk.
//
// The contract with the page is taken from the Python `AppearanceActions`
// class (`rexon/appearance/qml_window.py`) TO THE LETTER — the same method
// names, the same fourteen signals, the same JSON strings. So the page
// markup did not change by a single line (except for two lines with which
// it creates this object for itself — see `qml/AppearancePage.qml`).
//
// WHERE THE LOGIC LIVES. There is none of our own here at all:
//   * WHAT to show and with what words — `rexon_core::appearance_page`;
//   * THE STORE — `rexon_core::ocs::OcsClient` over the live executor
//     `rexon_core::ocs::LiveOcsIo` (the `net::UreqNet` network + the SQLite
//     cache `appearance_cache::LiveCache`); both halves were written
//     together with this object, because there was no live `OcsCacheIo`
//     executor in the product at all;
//   * AUR — `rexon_core::aur` (the RPC requests were already in Rust);
//   * THE DISK — `rexon_core::installed_scan::LiveScan`;
//   * INSTALLATION, APPLYING, REMOVAL —
//     `rexon_core::appearance_installer::LiveInstaller`;
//   * FAVOURITES — `rexon_core::favorites`; SETTINGS — `rexon_core::settings`.
//
// Warning: WHAT IS SYNCHRONOUS AND WHAT IS NOT, AND WHY IT MATTERS. Only the
// five methods whose answer the page reads ON THE SAME line it called them
// answer synchronously: `environment`, `remember`, `toggleFavorite`,
// `favorites`, `openUrl`. Everything else goes to the network or the disk
// and therefore answers with a SIGNAL from its own thread — a synchronous
// trip to the network would freeze the window for the time of the store's
// answer (measured in the original: a detailed answer takes up to 20 s).
//
// Warning: THE CANCELLATION IS REAL, NOT "THROW THE ANSWER AWAY". When the
// section changes, the catalogue and picture requests in flight are ABORTED
// via `net::CancelToken`, as `QNetworkReply::abort` aborted them in Python.
// Without this six pictures of the previous section would keep occupying
// the channel the opened one is waiting for (measured in the original on
// 31.07.2026: the answer went from 0.95 s to 12.3 s).

/// cxx-qt bridge of the action object.
///
/// The type is declared `#[qml_element]`, so the page creates it for itself
/// (`AppearanceActions { id: ownActions }`), while the Python host still
/// substitutes its own through an initial property. No registration in
/// `main_window.rs` is required for this: there is the section map, not a
/// list of side objects.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    // Available Qt styles: the static `QStyleFactory::keys()` is called
    // directly (cxx can do static methods), there is no shim. The crate already
    // links QtWidgets (the tray), and the process is a QApplication.
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        include!(<QtWidgets/QStyleFactory>);
        type QStyleFactory;

        #[Self = "QStyleFactory"]
        fn keys() -> QStringList;
    }

    // The item page in the browser — the crate's shared shim
    // (`cpp/rexon_ext_dialogs.h`). There is deliberately no second declaration
    // of `rexon_ext_pick_file`/`rexon_ext_confirm` here: cxx emits a shim by
    // the function name, and a duplicate would give a `duplicate symbol` at
    // link time.
    unsafe extern "C++" {
        include!("rexon_ext_dialogs.h");

        #[rust_name = "open_url_external"]
        fn rexon_ext_open_url(url: &QString) -> bool;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type AppearanceActions = super::AppearanceActionsRust;
    }

    unsafe extern "RustQt" {
        /// The section contents (a JSON string). The page will discard an answer
        /// with someone else's key itself — the key travels back untouched.
        /// A Conky widget appeared on or disappeared from disk (`~/.conky`,
        /// `~/.conkyrc`, our own folders): the section re-reads what is installed.
        #[qsignal]
        #[cxx_name = "conkyDiskChanged"]
        fn conky_disk_changed(self: Pin<&mut AppearanceActions>);

        /// New Conky widget snapshots for the tiles are ready (`conky_preview`):
        /// the section re-reads what is installed.
        #[qsignal]
        #[cxx_name = "conkyPreviewsReady"]
        fn conky_previews_ready(self: Pin<&mut AppearanceActions>);

        #[qsignal]
        #[cxx_name = "catalogReady"]
        fn catalog_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// The answer of the background warm-up: the request fields plus `items`.
        #[qsignal]
        #[cxx_name = "prefetchReady"]
        fn prefetch_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Full item details (a JSON string).
        #[qsignal]
        #[cxx_name = "detailReady"]
        fn detail_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// The SECOND pass of the details panel: the translated description (a JSON
        /// string — `itemId` and `translated`) following the card already shown.
        /// Arrives only when a translation actually happened: without a model there
        /// is no signal at all, and the user sees the original without a single
        /// complaint.
        #[qsignal]
        #[cxx_name = "descriptionReady"]
        fn description_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// A page of item comments.
        #[qsignal]
        #[cxx_name = "commentsReady"]
        fn comments_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// An AUR package card (a JSON string): the RPC `info` passport —
        /// `{itemId, info}` (or `{itemId, missing}` / `{itemId, error}`). The card's
        /// pictures arrive as frames (`galleryReady`), the comments as
        /// `commentsReady`, the description translation as `descriptionReady`.
        #[qsignal]
        #[cxx_name = "aurDetailReady"]
        fn aur_detail_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Check of an AUR package source (a JSON string): `{itemId, entry}` — an
        /// `aur_trust::CacheEntry` record, or `{itemId, failed}`.
        #[qsignal]
        #[cxx_name = "aurTrustReady"]
        fn aur_trust_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// The package's PKGBUILD (a JSON string): `{itemId, text}` or
        /// `{itemId, error}`.
        #[qsignal]
        #[cxx_name = "aurPkgbuildReady"]
        fn aur_pkgbuild_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// The SECOND pass of the comments page: translations (a JSON string —
        /// `itemId`, `page` and `rows` with "original → translation" pairs)
        /// following the already shown list. Only the comments whose translation
        /// actually happened get into `rows`: Russian ones and nothing-to-translate
        /// ones are not mentioned in the answer at all, so the "show original" link
        /// does not appear above them. No model — no signal either.
        #[qsignal]
        #[cxx_name = "commentTranslationsReady"]
        fn comment_translations_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Suggestions for the search line.
        #[qsignal]
        #[cxx_name = "suggestReady"]
        fn suggest_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// An item from a link in third-party text was assembled
        /// (`openLinkedCard`): a JSON string `{item}` (the item), `{aur}` (the
        /// package name) or `{failed, url, status}`.
        #[qsignal]
        #[cxx_name = "linkedCardReady"]
        fn linked_card_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// The request's picture was written into the cache at path `path`.
        #[qsignal]
        #[cxx_name = "previewDone"]
        fn preview_done(self: Pin<&mut AppearanceActions>, job: QString, path: QString);

        /// The request failed: `status` is the server's answer as a numeric string,
        /// empty (there was no answer) or `canceled` (we aborted it ourselves).
        #[qsignal]
        #[cxx_name = "previewFailed"]
        fn preview_failed(self: Pin<&mut AppearanceActions>, job: QString, status: QString);

        /// A ready carousel frame: the number as a string and the path on disk.
        #[qsignal]
        #[cxx_name = "galleryReady"]
        fn gallery_ready(self: Pin<&mut AppearanceActions>, index: QString, path: QString);

        /// The frame did not arrive.
        #[qsignal]
        #[cxx_name = "galleryFailed"]
        fn gallery_failed(self: Pin<&mut AppearanceActions>, index: QString);

        /// Walking the carousel links is finished: how many frames there really
        /// are.
        #[qsignal]
        #[cxx_name = "galleryFinished"]
        fn gallery_finished(self: Pin<&mut AppearanceActions>, count: QString);

        /// The installation progress share, 0–100.
        #[qsignal]
        #[cxx_name = "installProgress"]
        fn install_progress(self: Pin<&mut AppearanceActions>, pct: i32);

        /// The next line of the installation log.
        #[qsignal]
        #[cxx_name = "installLog"]
        fn install_log(self: Pin<&mut AppearanceActions>, text: QString);

        /// A message for the status line.
        #[qsignal]
        #[cxx_name = "statusMessage"]
        fn status_message(self: Pin<&mut AppearanceActions>, text: QString);

        /// The outcome of installing/applying/removing.
        #[qsignal]
        #[cxx_name = "actionDone"]
        fn action_done(self: Pin<&mut AppearanceActions>, text: QString, ok: bool);

        /// A theme from disk was marked for removal (`removeLocalById`): the text
        /// of the pop-up line with "Undo". The page hides the tile by rescanning.
        #[qsignal]
        #[cxx_name = "removalPending"]
        fn removal_pending(self: Pin<&mut AppearanceActions>, text: QString);

        /// The marked themes were removed (or a system theme was removed by the
        /// journal): the outcome in words for the status line; the page re-reads
        /// the section.
        #[qsignal]
        #[cxx_name = "removalDone"]
        fn removal_done(self: Pin<&mut AppearanceActions>, text: QString);

        /// The outcome of "Install from file…" beyond `actionDone` (a JSON string
        /// `local_install::LocalOutcome`): which section to offer to open
        /// (`openKey`, `openText`) and whether to re-read the section from disk
        /// (`reload`).
        #[qsignal]
        #[cxx_name = "localInstallDone"]
        fn local_install_done(self: Pin<&mut AppearanceActions>, payload: QString);

        /// The AUR installation queue changed: a JSON string
        /// `aur_install::AurQueue::snapshot_json` (`{jobs, status}`).
        #[qsignal]
        #[cxx_name = "aurJobsChanged"]
        fn aur_jobs_changed(self: Pin<&mut AppearanceActions>, payload: QString);

        /// The answer of the login screen probe (a JSON string
        /// `{plasmaLogin, loginManager, steamos, canOpen}` and the "Login manager"
        /// section: `managers`, `backend`, `switchable`, `note`, `pending`,
        /// `rollback` — `system_exec::LoginScreenView`) — see `probeLoginScreen`.
        #[qsignal]
        #[cxx_name = "loginScreenReady"]
        fn login_screen_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// The outcome of "Make default" (`makeDefaultLoginManager`): whether it
        /// worked and the text for the user — "takes effect after a reboot" and how
        /// to restore the previous one, or the reason for the refusal. A fresh probe
        /// follows (`loginScreenReady`).
        #[qsignal]
        #[cxx_name = "loginManagerSwitched"]
        fn login_manager_switched(self: Pin<&mut AppearanceActions>, ok: bool, message: QString);

        /// The outcome of an action of the "Login screen background and look" block
        /// (SDDM and Plasma Login Manager): the background, transferring the
        /// desktop look or "Reset". A fresh probe follows (`loginScreenReady`).
        #[qsignal]
        #[cxx_name = "loginLookDone"]
        fn login_look_done(self: Pin<&mut AppearanceActions>, ok: bool, message: QString);

        /// The login screen preview opened (`running`) or closed; `message` — why
        /// it did not show (empty — all is fine).
        #[qsignal]
        #[cxx_name = "loginPreviewChanged"]
        fn login_preview_changed(self: Pin<&mut AppearanceActions>, running: bool, message: QString);

        /// The outcome of an action of the "Shell palette" strip (hand over, take,
        /// revert): the text for the user and whether it worked.
        #[qsignal]
        #[cxx_name = "shellPaletteDone"]
        fn shell_palette_done(self: Pin<&mut AppearanceActions>, text: QString, ok: bool);

        /// The outcome of a GNOME extension tile action (`gnomeExtensionAction`): a
        /// JSON string `{ok, action, uuid, message, folder?, path?, details?}`
        /// (`gnome_ext_section::run_action`).
        #[qsignal]
        #[cxx_name = "gnomeExtensionDone"]
        fn gnome_extension_done(self: Pin<&mut AppearanceActions>, payload: QString);

        /// How many GNOME extension updates the shell has already downloaded: a
        /// JSON string `{count, text, logoutLabel}`
        /// (`gnome_ext_section::updates_answer`). `text` is the section's status
        /// line, empty means there are no updates.
        #[qsignal]
        #[cxx_name = "gnomeUpdatesChanged"]
        fn gnome_updates_changed(self: Pin<&mut AppearanceActions>, payload: QString);
    }

    extern "RustQt" {
        /// A machine snapshot the page cannot get anywhere else: the list of Qt
        /// styles, the active application style, whether there is an AUR helper,
        /// the running and the remembered desktop. A JSON string.
        #[qinvokable]
        fn environment(self: &AppearanceActions) -> QString;

        /// Remember the chosen desktop between launches. An empty `seen` means
        /// "ask the machine yourself which desktop is running now".
        #[qinvokable]
        fn remember(self: &AppearanceActions, de_id: &QString, seen: &QString) -> bool;

        /// Start on the section contents. The answer will come with the
        /// `catalogReady` signal.
        #[qinvokable]
        #[cxx_name = "loadCatalog"]
        fn load_catalog(self: Pin<&mut AppearanceActions>, request: &QString);

        /// Warm up the cache of another page. There is nothing to show it on.
        #[qinvokable]
        #[cxx_name = "loadPrefetch"]
        fn load_prefetch(self: Pin<&mut AppearanceActions>, request: &QString);

        /// One picture per request of the core's queue. The answer comes back with
        /// the request NUMBER, not the address: otherwise a live request and a
        /// background request for the same address could not be told apart.
        #[qinvokable]
        #[cxx_name = "loadPreview"]
        fn load_preview(
            self: Pin<&mut AppearanceActions>,
            job: &QString,
            item_id: &QString,
            url: &QString,
            low: bool,
        );

        /// The section changed: whatever is flying for the previous one's pictures
        /// is no longer relevant.
        #[qinvokable]
        #[cxx_name = "cancelPreviews"]
        fn cancel_previews(self: &AppearanceActions);

        /// Full item details and its carousel frames.
        #[qinvokable]
        #[cxx_name = "loadDetail"]
        fn load_detail(self: Pin<&mut AppearanceActions>, item_id: &QString);

        /// A single carousel frame — the one the user paged to themselves.
        #[qinvokable]
        #[cxx_name = "loadGalleryFrame"]
        fn load_gallery_frame(
            self: Pin<&mut AppearanceActions>,
            index: &QString,
            url: &QString,
        );

        /// A page of item comments from the store website.
        #[qinvokable]
        #[cxx_name = "loadComments"]
        fn load_comments(self: Pin<&mut AppearanceActions>, item_id: &QString, page: &QString);

        /// An AUR package card: the passport, pictures, description translation.
        #[qinvokable]
        #[cxx_name = "loadAurDetail"]
        fn load_aur_detail(self: Pin<&mut AppearanceActions>, pkg_name: &QString);

        /// A page of AUR package comments (parsing the website page; the answer is
        /// `commentsReady` with the fields `pinned`, `hasMore`, `blocked`,
        /// `comaintainers`).
        #[qinvokable]
        #[cxx_name = "loadAurComments"]
        fn load_aur_comments(self: Pin<&mut AppearanceActions>, pkg_name: &QString, page: &QString);

        /// The AUR package's PKGBUILD — shown inside the program. `pkg_base` is the
        /// package's git branch in AUR (the base), `pkg_name` is for the answer.
        #[qinvokable]
        #[cxx_name = "loadAurPkgbuild"]
        fn load_aur_pkgbuild(self: Pin<&mut AppearanceActions>, pkg_name: &QString, pkg_base: &QString);

        /// Suggestions for the search line: the same store search, but brief.
        #[qinvokable]
        #[cxx_name = "loadSuggestions"]
        fn load_suggestions(self: Pin<&mut AppearanceActions>, request: &QString);

        /// Install a theme from the store (and apply it if asked).
        /// `file_name` is the item file chosen by hand in the details panel (a name
        /// from `detailFiles`); empty means the automatic choice, the same as the
        /// tile's.
        #[qinvokable]
        #[cxx_name = "installById"]
        fn install_by_id(
            self: Pin<&mut AppearanceActions>,
            item_id: &QString,
            apply_after: bool,
            file_name: &QString,
        );

        /// The store item button (the tile and the main button of the details), as
        /// in KDE: not installed — only install; installed — apply what is
        /// installed. Decided by the core (`ap::store_card_action`); the outcome is
        /// `actionDone`.
        #[qinvokable]
        #[cxx_name = "actOnStoreItem"]
        fn act_on_store_item(self: Pin<&mut AppearanceActions>, item_id: &QString);

        /// Build an AUR package with the helper — the same one DE packages are
        /// installed with. The installation joins the application's SHARED queue
        /// (one at a time); the progress and the outcome come with the
        /// `aurJobsChanged` signal. The same call is "Retry".
        #[qinvokable]
        #[cxx_name = "installAur"]
        fn install_aur(self: Pin<&mut AppearanceActions>, pkg_name: &QString);

        /// The AUR installation queue right now (the same string as
        /// `aurJobsChanged`). It also subscribes the object to its changes: the
        /// queue lives longer than the page, and a reopened page picks up a running
        /// installation.
        #[qinvokable]
        #[cxx_name = "aurJobs"]
        fn aur_jobs(self: Pin<&mut AppearanceActions>) -> QString;

        /// Open the full installation log of an AUR package.
        #[qinvokable]
        #[cxx_name = "openAurLog"]
        fn open_aur_log(self: &AppearanceActions, pkg_name: &QString) -> bool;

        /// "Install from file…": `paths_json` is an array of addresses from the
        /// selection window (`file:///…`) or paths, `section` is the key of the
        /// section it is installed from. What it is and where it goes is the
        /// core's (`local_install`); the outcome is `actionDone` and
        /// `localInstallDone`.
        #[qinvokable]
        #[cxx_name = "installFiles"]
        fn install_files(self: Pin<&mut AppearanceActions>, paths_json: &QString, section: &QString);

        /// Apply a theme that already lies on disk.
        #[qinvokable]
        #[cxx_name = "applyLocalById"]
        fn apply_local_by_id(self: Pin<&mut AppearanceActions>, item_id: &QString);

        /// The "Apply "X"?" window of a Plasma global theme (a JSON string —
        /// `themes::GlobalQuestion` plus `itemId`). Empty means this tile has no
        /// window: not a global theme, not installed yet or of the old format — the
        /// button behaves as before.
        #[qinvokable]
        #[cxx_name = "globalApplyQuestion"]
        fn global_apply_question(self: &AppearanceActions, item_id: &QString) -> QString;

        /// Apply a global theme with the window's choice: `appearance` — the look,
        /// `layout` — the desktop and widget layout; `parts` — the parts mask of the
        /// detailed view ("Choose what to apply…"), `-1` — the simple view. The
        /// outcome is `actionDone`, as for `applyLocalById`.
        #[qinvokable]
        #[cxx_name = "applyGlobalById"]
        fn apply_global_by_id(
            self: Pin<&mut AppearanceActions>,
            item_id: &QString,
            appearance: bool,
            layout: bool,
            parts: i32,
        );

        /// The current cursor size in this environment, 0 — nobody set it. Fast (a
        /// file or one `gsettings get`) — the answer is synchronous.
        #[qinvokable]
        #[cxx_name = "cursorSize"]
        fn cursor_size(self: &AppearanceActions) -> i32;

        /// Change the cursor size of the current theme. The outcome comes with the
        /// `actionDone` signal, as for applying a theme.
        #[qinvokable]
        #[cxx_name = "applyCursorSize"]
        fn apply_cursor_size(self: Pin<&mut AppearanceActions>, size: i32);

        /// A GNOME extension tile action: `toggle`, `enable`, `disable`, `prefs`,
        /// `uninstall` (only our own), `folder` (our own — open the folder, a system
        /// one — copy the path), `errorDetails`. With the shell running — through
        /// it, without it — the fallback path. The outcome is `gnomeExtensionDone`
        /// (and `actionDone` for toggling and removal).
        #[qinvokable]
        #[cxx_name = "gnomeExtensionAction"]
        fn gnome_extension_action(self: Pin<&mut AppearanceActions>, item_id: &QString, action: &QString);

        /// The downloaded GNOME extension updates right now (the same string as
        /// `gnomeUpdatesChanged`).
        #[qinvokable]
        #[cxx_name = "gnomeUpdates"]
        fn gnome_updates(self: &AppearanceActions) -> QString;

        /// Labels of the "GNOME extensions" section (JSON
        /// `gnome_ext_section::page_texts`).
        #[qinvokable]
        #[cxx_name = "gnomeExtTexts"]
        fn gnome_ext_texts(self: &AppearanceActions) -> QString;

        /// "Log out" so that the downloaded extension updates are applied:
        /// `gnome-session-quit --logout`, GNOME shows the confirmation. `false`
        /// means it could not be launched.
        #[qinvokable]
        #[cxx_name = "logOutForUpdates"]
        fn log_out_for_updates(self: &AppearanceActions) -> bool;

        /// Remove an installed theme. A store item — by the installed journal; a
        /// theme from disk (`local:…`) — like `removeLocalById`.
        #[qinvokable]
        fn uninstall(self: Pin<&mut AppearanceActions>, item_json: &QString);

        /// "Remove" for a theme from disk (`theme_removal`): our own — mark for
        /// removal (the `removalPending` signal, `commitRemovals` removes it); a
        /// system one installed by Rexon — at once with the helper; an enabled one
        /// and other system ones — a refusal with a reason (`actionDone`).
        #[qinvokable]
        #[cxx_name = "removeLocalById"]
        fn remove_local_by_id(self: Pin<&mut AppearanceActions>, item_id: &QString);

        /// The pop-up line closed or expired: remove what is marked. The outcome is
        /// `actionDone`, then `removalDone`.
        #[qinvokable]
        #[cxx_name = "commitRemovals"]
        fn commit_removals(self: Pin<&mut AppearanceActions>);

        /// "Undo": remove all marks. Returns how many themes came back.
        #[qinvokable]
        #[cxx_name = "undoRemovals"]
        fn undo_removals(self: &AppearanceActions) -> i32;

        /// Toggle the star. Returns the CURRENT set of favourites, not "whether it
        /// worked": the page keeps the favourites itself, and two calls in a row
        /// would diverge from what is on disk.
        #[qinvokable]
        #[cxx_name = "toggleFavorite"]
        fn toggle_favorite(self: &AppearanceActions, item_id: &QString) -> QString;

        /// The set of favourites on opening the tab (the stars on the tiles).
        #[qinvokable]
        fn favorites(self: &AppearanceActions) -> QString;

        /// The item page in the browser.
        #[qinvokable]
        #[cxx_name = "openUrl"]
        fn open_url(self: &AppearanceActions, url: &QString) -> bool;

        /// A link to a store item from third-party text (`{card, url}`, `card` is
        /// `link_policy::StoreRef`): assemble the item in the background, the
        /// answer with the `linkedCardReady` signal.
        #[qinvokable]
        #[cxx_name = "openLinkedCard"]
        fn open_linked_card(self: Pin<&mut AppearanceActions>, request: &QString);

        /// "Demonstrate" a KDE splash: `ksplashqml <theme> --test`, as in the KDE
        /// module. Returns an error for the user (empty — the show started); a
        /// failure of the program itself is reported by `statusMessage`.
        #[qinvokable]
        #[cxx_name = "demoSplash"]
        fn demo_splash(self: Pin<&mut AppearanceActions>, theme: &QString) -> QString;

        /// Whether autostart of the Conky widget of the local tile `itemId` is on.
        #[qinvokable]
        #[cxx_name = "conkyAutostart"]
        fn conky_autostart(self: &AppearanceActions, item_id: &QString) -> bool;

        /// «Start at login» for a Conky widget. The outcome comes with the
        /// `actionDone` signal; returns the state after toggling.
        #[qinvokable]
        #[cxx_name = "setConkyAutostart"]
        fn set_conky_autostart(self: Pin<&mut AppearanceActions>, item_id: &QString, on: bool) -> bool;

        /// Autostart settings of a Conky widget (JSON `conky_autostart::View`): the
        /// delay, "only here".
        #[qinvokable]
        #[cxx_name = "conkyAutostartOptions"]
        fn conky_autostart_options(self: &AppearanceActions, item_id: &QString) -> QString;

        /// Write the delay and "only here". The outcome comes with the `actionDone`
        /// signal; returns the settings anew.
        #[qinvokable]
        #[cxx_name = "setConkyAutostartOptions"]
        fn set_conky_autostart_options(
            self: Pin<&mut AppearanceActions>,
            item_id: &QString,
            delay: i32,
            only_here: bool,
        ) -> QString;

        /// Parts of the Conky widget of tile `itemId` — JSON
        /// `[{conf, label, running}]` (`conky_procs::parts`).
        #[qinvokable]
        #[cxx_name = "conkyParts"]
        fn conky_parts(self: &AppearanceActions, item_id: &QString) -> QString;

        /// Start or stop one part of the widget. The outcome comes with the
        /// `actionDone` signal; returns the parts anew (JSON, like `conkyParts`).
        #[qinvokable]
        #[cxx_name = "pressConkyPart"]
        fn press_conky_part(self: Pin<&mut AppearanceActions>, item_id: &QString, conf: &QString) -> QString;

        /// The last lines of the widget's log; empty means Rexon did not start it.
        #[qinvokable]
        #[cxx_name = "conkyLog"]
        fn conky_log(self: &AppearanceActions, item_id: &QString) -> QString;

        /// "Open folder" of a Conky widget in the file manager; `false` means the
        /// widget is no longer on disk or there is nothing to open it with.
        #[qinvokable]
        #[cxx_name = "openConkyFolder"]
        fn open_conky_folder(self: &AppearanceActions, item_id: &QString) -> bool;

        /// The tile's theme was made in Conky Studio and it is on the machine — the
        /// card has an "Edit in Conky Studio" button.
        #[qinvokable]
        #[cxx_name = "conkyStudioEditable"]
        fn conky_studio_editable(self: &AppearanceActions, item_id: &QString) -> bool;

        /// "Edit in Conky Studio": open the theme in it. The outcome comes with the
        /// `actionDone` signal.
        #[qinvokable]
        #[cxx_name = "editConkyInStudio"]
        fn edit_conky_in_studio(self: Pin<&mut AppearanceActions>, item_id: &QString);

        /// "Open settings file" — the `conf` part of the widget in the default
        /// editor; someone else's path (not a part of the widget) does not open.
        #[qinvokable]
        #[cxx_name = "openConkyFile"]
        fn open_conky_file(self: &AppearanceActions, item_id: &QString, conf: &QString) -> bool;

        /// Settings of a Conky widget (the `conf` file of tile `itemId`) — JSON
        /// `conky_overlay::SettingsView`: values, choices, labels.
        #[qinvokable]
        #[cxx_name = "conkySettings"]
        fn conky_settings(self: &AppearanceActions, item_id: &QString, conf: &QString) -> QString;

        /// Change one setting (`key` is the choice field, `net:<name>` is the
        /// network; `value` is JSON, `null` means as the author has it). The outcome
        /// comes with the `actionDone` signal; returns the settings anew.
        #[qinvokable]
        #[cxx_name = "setConkySetting"]
        fn set_conky_setting(
            self: Pin<&mut AppearanceActions>,
            item_id: &QString,
            conf: &QString,
            key: &QString,
            value: &QString,
        ) -> QString;

        /// "As the author has it": remove the overlay. Returns the settings anew.
        #[qinvokable]
        #[cxx_name = "resetConkySettings"]
        fn reset_conky_settings(self: Pin<&mut AppearanceActions>, item_id: &QString, conf: &QString) -> QString;

        /// The colour palette of the Conky widget of tile `itemId` — JSON
        /// `conky_colors::View`: colour circles with a counter of places, caveats.
        #[qinvokable]
        #[cxx_name = "conkyColours"]
        fn conky_colours(self: &AppearanceActions, item_id: &QString) -> QString;

        /// Change colour `from` to `to` (`#rrggbb`) in all places of the widget's
        /// files. The outcome comes with the `actionDone` signal; returns the
        /// palette anew.
        #[qinvokable]
        #[cxx_name = "setConkyColour"]
        fn set_conky_colour(self: Pin<&mut AppearanceActions>, item_id: &QString, from: &QString, to: &QString) -> QString;

        /// "Restore colours": the widget's files — as before the first colour
        /// change (if edited after Rexon too — a question first). Returns the
        /// palette anew.
        #[qinvokable]
        #[cxx_name = "revertConkyColours"]
        fn revert_conky_colours(self: Pin<&mut AppearanceActions>, item_id: &QString) -> QString;

        /// Labels of the "Conky widgets" section (JSON `conky_procs::SectionTexts`).
        #[qinvokable]
        #[cxx_name = "conkyTexts"]
        fn conky_texts(self: &AppearanceActions) -> QString;

        /// The "On the desktop now" ribbon: the running Conky widgets — JSON
        /// `[{item_id, title, picture, confs}]` (`conky_procs::ribbon`).
        #[qinvokable]
        #[cxx_name = "conkyRibbon"]
        fn conky_ribbon(self: &AppearanceActions) -> QString;

        /// The ribbon's cross: stop widget `itemId` (its running files). The
        /// outcome comes with the `actionDone` signal.
        #[qinvokable]
        #[cxx_name = "stopConkyRibbonItem"]
        fn stop_conky_ribbon_item(self: Pin<&mut AppearanceActions>, item_id: &QString) -> bool;

        /// Conky widget sets — JSON `{texts, rows}` (`conky_sets::texts`,
        /// `conky_sets::rows`).
        #[qinvokable]
        #[cxx_name = "conkySets"]
        fn conky_sets(self: &AppearanceActions) -> QString;

        /// "Save running ones as a set" with name `name`. The outcome comes with the
        /// `actionDone` signal; `true` — saved.
        #[qinvokable]
        #[cxx_name = "saveConkySet"]
        fn save_conky_set(self: Pin<&mut AppearanceActions>, name: &QString) -> bool;

        /// Apply a set: a question about the widgets' commands (one per set), then
        /// launching in the background. The outcome comes with the `actionDone`
        /// signal.
        #[qinvokable]
        #[cxx_name = "applyConkySet"]
        fn apply_conky_set(self: Pin<&mut AppearanceActions>, name: &QString);

        /// "Save set to file…": the save window, a question if something personal
        /// was found in the widgets, then `.rxn`. The outcome comes with the
        /// `actionDone` signal; `true` — saved.
        #[qinvokable]
        #[cxx_name = "exportConkySet"]
        fn export_conky_set(self: Pin<&mut AppearanceActions>, name: &QString) -> bool;

        /// Remove our own set after a question. The outcome comes with the
        /// `actionDone` signal.
        #[qinvokable]
        #[cxx_name = "removeConkySet"]
        fn remove_conky_set(self: Pin<&mut AppearanceActions>, name: &QString) -> bool;

        /// "Stop all": a question with a list, then stopping by process numbers.
        /// The outcome comes with the `actionDone` signal; `false` means they
        /// declined or there is nothing to stop.
        #[qinvokable]
        #[cxx_name = "stopAllConky"]
        fn stop_all_conky(self: Pin<&mut AppearanceActions>) -> bool;

        /// Start watching the places of Conky widgets (once per object): a change
        /// comes with the `conkyDiskChanged` signal.
        #[qinvokable]
        #[cxx_name = "watchConkyDisk"]
        fn watch_conky_disk(self: Pin<&mut AppearanceActions>);

        /// The primary screen for the invisible screen of Conky widget snapshots:
        /// physical points and the logical density (dpi). On Wayland it cannot be
        /// found out otherwise.
        #[qinvokable]
        #[cxx_name = "setConkyPreviewScreen"]
        fn set_conky_preview_screen(self: &AppearanceActions, width: i32, height: i32, dpi: i32);

        /// The screen mini-map: the running Conky widgets and the `conf` file of
        /// tile `itemId` (empty — without a card) — JSON `conky_layout::MapView`.
        /// `screens` are the window's screens (JSON
        /// `[{name, x, y, width, height}]`).
        #[qinvokable]
        #[cxx_name = "conkyScreenMap"]
        fn conky_screen_map(self: &AppearanceActions, item_id: &QString, conf: &QString, screens: &QString) -> QString;

        /// A widget was dragged on the mini-map: the window of the `conf` file is
        /// now at `x, y, w, h` (screen points); `snap` — snap to the edges. The
        /// outcome comes with the `actionDone` signal.
        #[qinvokable]
        #[cxx_name = "moveConkyWidget"]
        fn move_conky_widget(
            self: Pin<&mut AppearanceActions>,
            item_id: &QString,
            conf: &QString,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            snap: bool,
            screens: &QString,
        ) -> bool;

        /// "Undo" on the mini-map: the position of the `conf` file is `saved` (JSON
        /// from the map).
        #[qinvokable]
        #[cxx_name = "restoreConkyPosition"]
        fn restore_conky_position(self: Pin<&mut AppearanceActions>, item_id: &QString, conf: &QString, saved: &QString) -> bool;

        /// The "no preview — no Xvfb" hint of the Conky section (JSON
        /// `conky_preview::XvfbHint`).
        #[qinvokable]
        #[cxx_name = "conkyXvfbHint"]
        fn conky_xvfb_hint(self: &AppearanceActions) -> QString;

        /// Close the Xvfb hint forever.
        #[qinvokable]
        #[cxx_name = "closeConkyXvfbHint"]
        fn close_conky_xvfb_hint(self: &AppearanceActions);

        /// Install Xvfb in the background by the shared path for missing programs
        /// (the password window). The outcome comes with the `actionDone` signal,
        /// then `conkyDiskChanged`, so that the tiles are re-shot.
        #[qinvokable]
        #[cxx_name = "installConkyXvfb"]
        fn install_conky_xvfb(self: Pin<&mut AppearanceActions>);

        /// A one-off cleanup of stale translations: space in the cache, nothing
        /// more.
        #[qinvokable]
        #[cxx_name = "purgeTranslations"]
        fn purge_translations(self: Pin<&mut AppearanceActions>);

        /// What draws the login screen (Plasma Login Manager or not), whether this
        /// is SteamOS, whether there is something to open the login screen settings
        /// with and which login screens are installed (the default one, the running
        /// one, whether it can be made default and why not). Read-only. The answer
        /// comes with the `loginScreenReady` signal: the probe may call
        /// `systemctl`, and it has no place in the interface thread.
        #[qinvokable]
        #[cxx_name = "probeLoginScreen"]
        fn probe_login_screen(self: Pin<&mut AppearanceActions>);

        /// The last answer of the login screen probe (the same JSON string as
        /// `loginScreenReady`), empty means the probe has not answered yet. The
        /// page loads asynchronously, and the signal managed to arrive before its
        /// `Connections` attached to the object (live bench of 28.09: the probe
        /// answered, but it did not reach the page). So the page, once connected,
        /// also asks this way.
        #[qinvokable]
        #[cxx_name = "loginScreen"]
        fn login_screen(self: &AppearanceActions) -> QString;

        /// Open the login screen settings of Plasma Login Manager. `false` means
        /// there was nothing to launch.
        #[qinvokable]
        #[cxx_name = "openLoginScreenSettings"]
        fn open_login_screen_settings(self: &AppearanceActions) -> bool;

        /// "Make default" login screen `unit` (a unit without `.service`) with
        /// greeter `greeter` (LightDM only; empty — without a choice). In a worker
        /// thread: the core checks everything again against the live system and
        /// refuses before the password, otherwise one `pkexec` of the `switch-dm`
        /// helper. The answer is `loginManagerSwitched`. While the previous call is
        /// running, a new one is rejected with the same signal.
        #[qinvokable]
        #[cxx_name = "makeDefaultLoginManager"]
        fn make_default_login_manager(self: Pin<&mut AppearanceActions>, unit: &QString, greeter: &QString);

        /// "Make Plasma (Wayland) the default session": the login screen itself
        /// opens or offers the X11 session `x11` (from the core's table
        /// `system_plan::X11_SESSION_SUCCESSORS`) — move only the session key to its
        /// pair. In a worker thread, through the window's privilege session; the
        /// root helper checks everything itself. The answer is
        /// `loginManagerSwitched`, followed by a fresh probe.
        #[qinvokable]
        #[cxx_name = "makeWaylandDefaultSession"]
        fn make_wayland_default_session(self: Pin<&mut AppearanceActions>, x11: &QString);

        /// The background of the current SDDM theme — picture `path` (empty —
        /// restore the theme's own background), like "Change background" of the KDE
        /// module; with Plasma Login Manager — the background of its login screen
        /// through its helper (`plasma_login::background_live`). In a worker thread,
        /// through the window's privilege session
        /// (`system_plan::sddm_background_live`). The answer is `loginLookDone`,
        /// followed by a fresh probe.
        #[qinvokable]
        #[cxx_name = "setLoginBackground"]
        fn set_login_background(self: Pin<&mut AppearanceActions>, path: &QString);

        /// Transfer this desktop's look to the SDDM or Plasma Login Manager login
        /// screen (`reset` — "Reset"), like "Apply Plasma settings" of the KDE
        /// modules. The answer is `loginLookDone`, followed by a fresh probe.
        #[qinvokable]
        #[cxx_name = "applyLoginLook"]
        fn apply_login_look(self: Pin<&mut AppearanceActions>, reset: bool);

        /// "Show clock" on the login screen (SDDM — for the current theme, Plasma
        /// Login Manager — its own key), like the checkbox of the KDE modules. The
        /// answer is `loginLookDone`, followed by a fresh probe.
        #[qinvokable]
        #[cxx_name = "setLoginClock"]
        fn set_login_clock(self: Pin<&mut AppearanceActions>, show: bool);

        /// Live preview of the login screen (the greeter's test mode, without
        /// logging out of the session): `token` is `sddm:<theme>` from a tile or
        /// empty (what the login screen will show now). Returns a refusal for the
        /// user (empty — the show started); the progress is
        /// `loginPreviewChanged`.
        #[qinvokable]
        #[cxx_name = "previewLoginScreen"]
        fn preview_login_screen(self: Pin<&mut AppearanceActions>, token: &QString) -> QString;

        /// Close the open login screen preview.
        #[qinvokable]
        #[cxx_name = "closeLoginPreview"]
        fn close_login_preview(self: &AppearanceActions);

        /// The label of the show button on a tile (`token` is the `splashDemo`
        /// sign): "Demonstrate splash" or "Preview" of an SDDM theme.
        #[qinvokable]
        #[cxx_name = "tileDemoLabel"]
        fn tile_demo_label(self: &AppearanceActions, token: &QString) -> QString;

        /// The tooltip of the same button.
        #[qinvokable]
        #[cxx_name = "tileDemoTooltip"]
        fn tile_demo_tooltip(self: &AppearanceActions, token: &QString) -> QString;

        /// The "Shell palette" strip: a JSON string of the core's ready decision
        /// (`shell_palette::PaletteBanner`); `visible: false` means there is no
        /// live shell. Synchronous: collecting is the session directory, `/proc`
        /// and a couple of files.
        #[qinvokable]
        #[cxx_name = "shellPalette"]
        fn shell_palette(self: &AppearanceActions) -> QString;

        /// Remember who drives the palette (`rexon` / `shell`).
        #[qinvokable]
        #[cxx_name = "setPaletteLead"]
        fn set_palette_lead(self: &AppearanceActions, lead: &QString) -> bool;

        /// "Revert to what it was" of section `section` by the "Apply" snapshot
        /// (`ap::revert_applied_section`; for colours, first the layer's memo). The
        /// answer is a JSON string `{ok, text}`. Synchronous: only profile files, no
        /// commands. The scan cache is reset — the "Active" badge moves on
        /// re-reading.
        #[qinvokable]
        #[cxx_name = "revertApplied"]
        fn revert_applied(self: &AppearanceActions, section: &QString) -> QString;

        /// "Hand the palette over to the shell". The outcome comes with the
        /// `shellPaletteDone` signal.
        #[qinvokable]
        #[cxx_name = "pushShellPalette"]
        fn push_shell_palette(self: Pin<&mut AppearanceActions>);

        /// "Take the shell's colours". The outcome comes with the
        /// `shellPaletteDone` signal.
        #[qinvokable]
        #[cxx_name = "pullShellPalette"]
        fn pull_shell_palette(self: Pin<&mut AppearanceActions>);

        /// "Revert to what it was". The outcome comes with the `shellPaletteDone`
        /// signal.
        #[qinvokable]
        #[cxx_name = "revertShellPalette"]
        fn revert_shell_palette(self: Pin<&mut AppearanceActions>);
    }

    // The trait declaration must be IN THE BRIDGE: cxx-qt emits the thread type
    // alias from it, without which background work would not reach the
    // signal.
    impl cxx_qt::Threading for AppearanceActions {}
}

use core::pin::Pin;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use cxx_qt::{CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::{json, Map, Value};

use rexon_core::appearance_installer::{self as ainst, LiveInstaller};
use rexon_core::appearance_page as ap;
use rexon_core::aur::{self, AurPackage};
use rexon_core::aur_install;
use rexon_core::categories;
use rexon_core::cinnamon_spices as cs;
use rexon_core::gnome_ego as ego;
use rexon_core::gnome_ext_section as gext;
use rexon_core::wallhaven as wh;
use rexon_core::de_apply;
use rexon_core::favorites;
use rexon_core::i18n;
use rexon_core::installed_scan::{self as scan, LiveScan};
use rexon_core::models::ThemeItem;
use rexon_core::net::{CancelToken, UreqNet};
use rexon_core::ocs::{LiveOcsIo, OcsClient};
use rexon_core::session_splash::{self as ss, Splash};
use rexon_core::settings::SettingsManager;
use rexon_core::shell_palette as palette;
use rexon_core::splash_exec::LiveSplashIo;
use rexon_core::system;
use rexon_core::system_plan;
use rexon_core::theme_removal::{self as removal, LiveRemoval, RemovalQueue, Verdict};
use rexon_core::translator_engine as te;
use rexon_core::ui_base;
use rexon_core::utils;

/// The object's answer to the page.
type Handle = CxxQtThread<qobject::AppearanceActions>;

/// Rust state of the action object.
///
/// Everything lies behind `Arc<Mutex<…>>`, and this is not
/// over-cautiousness: the page asks for an action BY the item NUMBER, while
/// the installer needs the item itself — so what the object once handed up
/// is later read by worker threads.
#[derive(Default)]
pub struct AppearanceActionsRust {
    state: Arc<State>,
}

#[derive(Default)]
struct State {
    /// Watching the places of Conky widgets is already running.
    conky_watch: std::sync::atomic::AtomicBool,
    /// Everything the object has ever handed up (number → item).
    items: Mutex<HashMap<String, ThemeItem>>,
    /// The same for AUR packages: they have their own button and their own
    /// installer.
    packages: Mutex<HashMap<String, AurPackage>>,
    /// The disk scan cache for the session: re-entering the section is
    /// instant. It is reset after a successful installation — otherwise the
    /// «✅ Active» badge would stay on the previous theme.
    scan_cache: Mutex<HashMap<String, Vec<ThemeItem>>>,
    /// The theme directories stamp at which [`State::scan_cache`] was taken
    /// (`installed_scan::roots_stamp`): a different one throws the cache away.
    scan_stamp: Mutex<String>,
    /// The AUR list arrives whole (the RPC has no pages) — we cut it ourselves.
    aur_cache: Mutex<HashMap<String, Vec<AurPackage>>>,
    /// The AUR helper, asked once per tab session.
    aur_helper: Mutex<Option<Option<String>>>,
    /// Cancellation flags of the live catalogue requests (request key → token).
    catalog_cancels: Mutex<HashMap<String, CancelToken>>,
    /// The same for pictures (request number → token).
    preview_cancels: Mutex<HashMap<String, CancelToken>>,
    /// How many comments an item has — by the store's answer.
    ///
    /// As a separate map rather than an item field: `comment_count` lives on
    /// `ocs::OcsContent`, while the page keeps a `ThemeItem`, and this number
    /// does not pass through the catalogue cache at all (there is no column in
    /// the database). Without it the comments branch would not know whether
    /// there are more pages.
    comment_counts: Mutex<HashMap<String, i64>>,
    /// The opened item: nobody needs the carousel frames of a closed one.
    detail_id: Mutex<String>,
    /// The shown comments LIST: the item and the list walk number.
    ///
    /// The number grows every time the list is built anew (a request for the
    /// FIRST page or another item), and does not change from "Show more": pages
    /// 1, 2, 3 are appended to the same list and stay on screen. The watcher of
    /// the second pass (translating the comments) needs exactly this — "the
    /// same comments are still shown" — not "the last requested page". Count it
    /// otherwise, and a click on "Show more" would cancel the translation of a
    /// page that has not gone anywhere.
    comments_view: Mutex<(String, u64)>,
    /// The last answer of the login screen probe (`probeLoginScreen`).
    login_screen: Mutex<String>,
    /// "Make default" is running: a second call is not let in until the first
    /// ends.
    login_switching: std::sync::atomic::AtomicBool,
    /// Available Qt styles taken in the interface thread (`environment`): the
    /// "Application style" scan runs in a worker thread and takes them from
    /// here.
    qt_style_keys: Mutex<Vec<String>>,
    /// AUR package cards for the session: the passport (`None` — AUR does not
    /// know the package) with the moment of the answer; older than
    /// `aur::DETAIL_CACHE_SECS` — ask again.
    aur_info: Mutex<HashMap<String, (std::time::Instant, Option<aur::AurInfo>)>>,
    /// Package pages (name, comments page number) → the parse.
    aur_pages: Mutex<HashMap<(String, i64), (std::time::Instant, aur::AurPage)>>,
    /// Found card pictures: a marker (`github-readme:`/`og-image:`) → an address
    /// (`None` — there is no picture).
    aur_images: Mutex<HashMap<String, Option<String>>>,
    /// Themes from disk marked for removal while the "Undo" line is open
    /// (`theme_removal`): their tiles do not get into the scan answers.
    removals: Mutex<RemovalQueue>,
    /// The desktop of the last disk scan: by it the removal checks whether the
    /// theme is enabled in the environment's section.
    last_de: Mutex<String>,
    /// Checking for GNOME extension updates has already been requested in this
    /// session (`CheckForUpdates()` — once, on the first opening of the
    /// section).
    gnome_updates_checked: std::sync::atomic::AtomicBool,
    /// How many extension updates the shell has already downloaded.
    gnome_updates: Mutex<usize>,
}

/// The page closed (or the program exited) while the "Undo" line was still
/// hanging: remove what is marked, as an expired line would have — the user
/// saw "removed". If we did not get here (a crash), the themes stayed in
/// place.
impl Drop for State {
    fn drop(&mut self) {
        let pending = self.removals.get_mut().map(|q| q.take_all()).unwrap_or_default();
        if pending.is_empty() {
            return;
        }
        let de = self.last_de.get_mut().map(|d| d.clone()).unwrap_or_default();
        let (ok, msg) = commit_pending(pending, &de);
        rexon_core::log_info!("theme", "удаление на закрытии: {} — {msg}", if ok { "удалось" } else { "не всё" });
    }
}

// ============================================================================
// Synchronous answers
// ============================================================================

impl qobject::AppearanceActions {
    fn environment(&self) -> QString {
        let state = self.rust().state.clone();
        // The list of Qt styles — the live `QStyleFactory::keys()`, as in the
        // original: the built-in styles plus the installed plugins. The core
        // replaces an empty answer with a fallback (`available_widget_styles`).
        let live_keys: Vec<String> = qobject::QStyleFactory::keys()
            .iter()
            .map(|k| k.to_string())
            .collect();
        let styles = scan::available_widget_styles(&live_keys);
        if let Ok(mut keys) = state.qt_style_keys.lock() {
            *keys = styles.clone();
        }

        let mut live = LiveScan::new(Vec::new(), HashMap::new());
        // From here only the application style is needed, and this is the
        // interface thread: we set the answer about the login screen in advance so
        // that the map of active themes does not go asking `systemctl` (the SDDM
        // theme is not read from here).
        live.plasma_login = Some(false);
        let widget_style = live
            .active_map()
            .get("widgetstyle")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let detected = system::detect_current_de(
            &env_var("XDG_CURRENT_DESKTOP"),
            &env_var("DESKTOP_SESSION"),
            !env_var("WAYLAND_DISPLAY").is_empty(),
        )
        .unwrap_or_default();

        let (saved, seen) = match SettingsManager::new() {
            Ok(s) => (
                s.get("appearance_de")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                s.get("appearance_de_seen")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
            Err(_) => (String::new(), String::new()),
        };

        let payload = json!({
            "qtStyles": styles,
            "widgetStyle": widget_style,
            "aurHelper": aur_helper(&state).is_some(),
            "detectedDe": detected,
            "savedDe": saved,
            "seenDe": seen,
        });
        // Warning: the decision "which desktop to remember" is made by the core
        // and executed by the object — and AFTER the snapshot is taken, so that the
        // core on the page side sees the previous values and makes the same
        // decision.
        let choice = ap::initial_de(&detected, &saved, &seen);
        if let Some((de_id, seen)) = choice.remember {
            remember_de(&de_id, &seen);
        }
        QString::from(&payload.to_string())
    }

    fn remember(&self, de_id: &QString, seen: &QString) -> bool {
        remember_de(&de_id.to_string(), &seen.to_string())
    }

    fn toggle_favorite(&self, item_id: &QString) -> QString {
        let id = item_id.to_string();
        let item = self
            .rust()
            .state
            .items
            .lock()
            .ok()
            .and_then(|m| m.get(&id).cloned());
        if let Some(item) = item {
            let path = favorites::default_path();
            let mut store = favorites::load_raw(&path);
            favorites::toggle(&mut store, &item);
            if let Err(e) = favorites::save_raw(&path, &store) {
                // Silence is not allowed: the star on screen would light up without
                // surviving a restart.
                rexon_core::log_warn!("appearance", "избранное не сохранено: {e}");
            }
        }
        QString::from(&favorite_ids())
    }

    fn favorites(&self) -> QString {
        QString::from(&favorite_ids())
    }

    fn conky_autostart(&self, item_id: &QString) -> bool {
        let id = item_id.to_string();
        let name = ap::local_theme_name(&id, "");
        rexon_core::de_apply::conky_autostart(
            &rexon_core::animations::SystemIo,
            &rexon_core::de_apply::Ctx::live(),
            &name,
        )
    }

    fn set_conky_autostart(mut self: Pin<&mut Self>, item_id: &QString, on: bool) -> bool {
        let id = item_id.to_string();
        let name = ap::local_theme_name(&id, "");
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        if !on {
            let (ok, msg) = rexon_core::de_apply::set_conky_autostart(&io, &ctx, &name, false);
            self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
            return rexon_core::de_apply::conky_autostart(&io, &ctx, &name);
        }
        // At login the widget will start without Rexon — check it now by the same
        // path as "▶️ Start": permission for the commands (the question is in the
        // window's thread), then the programs "all or nothing" (installation in
        // the background, without a question). If it is not ready, autostart is not
        // enabled, and it is said why; the toggle is re-read by `actionDone`.
        if ask_conky_safety(&name, None) == Some(false) {
            self.as_mut()
                .action_done(QString::from(&rexon_core::conky_safety::declined_text(&name)), false);
            return rexon_core::de_apply::conky_autostart(&io, &ctx, &name);
        }
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        let widget = name.clone();
        std::thread::spawn(move || {
            use rexon_core::conky_safety as cs;
            let name = widget;
            let note = match cs::live_prepare_start(std::slice::from_ref(&name), cs::Retry::Autostart).pop() {
                Some(Err(text)) => {
                    finish_action(&state, &handle, text, false);
                    return;
                }
                Some(Ok(note)) => note,
                None => None,
            };
            let io = rexon_core::animations::SystemIo;
            let ctx = rexon_core::de_apply::Ctx::live();
            let (ok, mut msg) = rexon_core::de_apply::set_conky_autostart(&io, &ctx, &name, true);
            if let Some(n) = note.filter(|_| ok) {
                msg = format!("{msg} {n}");
            }
            finish_action(&state, &handle, ap::action_result_status(ok, &msg), ok);
        });
        rexon_core::de_apply::conky_autostart(&io, &ctx, &name)
    }

    fn conky_autostart_options(&self, item_id: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let v = rexon_core::conky_autostart::widget_view(
            &rexon_core::animations::SystemIo,
            &rexon_core::de_apply::Ctx::live(),
            &name,
        );
        QString::from(&serde_json::to_string(&v).unwrap_or_default())
    }

    fn set_conky_autostart_options(mut self: Pin<&mut Self>, item_id: &QString, delay: i32, only_here: bool) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        let (ok, msg) = rexon_core::conky_autostart::set_widget(&io, &ctx, &name, delay.max(0) as u32, only_here);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        self.conky_autostart_options(item_id)
    }

    fn conky_parts(&self, item_id: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let parts = rexon_core::conky_procs::parts(
            &rexon_core::animations::SystemIo,
            &rexon_core::de_apply::Ctx::live(),
            &name,
        );
        QString::from(&serde_json::to_string(&parts).unwrap_or_else(|_| "[]".into()))
    }

    fn press_conky_part(mut self: Pin<&mut Self>, item_id: &QString, conf: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        if ask_conky_safety(&name, Some(&conf.to_string())) == Some(false) {
            let msg = rexon_core::conky_safety::declined_text(&name);
            self.as_mut().action_done(QString::from(&ap::action_result_status(false, &msg)), false);
            return self.conky_parts(item_id);
        }
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        let conf = conf.to_string();
        // "All or nothing", as for the tile: a part without the needed programs
        // does not start. Packages are not installed from here (the part's button
        // works in the interface thread) — the tile's "Start" will install them.
        let starting = !rexon_core::conky_procs::running_confs(&io, &ctx).iter().any(|c| *c == conf);
        let (family, installable) = rexon_core::conky_safety::live_family();
        let h = rexon_core::conky_safety::health(&io, &ctx, &name, &family);
        if starting {
            if let Some(text) = rexon_core::conky_safety::part_refusal(&name, &h, installable) {
                self.as_mut().action_done(QString::from(&ap::action_result_status(false, &text)), false);
                return self.conky_parts(item_id);
            }
        }
        let (ok, mut msg) = rexon_core::conky_procs::press_part(&io, &ctx, &name, &conf);
        if ok && starting {
            if let Some(n) = rexon_core::conky_safety::files_note(&h) {
                msg = format!("{msg} {n}");
            }
        }
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        self.conky_parts(item_id)
    }

    fn conky_log(&self, item_id: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        QString::from(&rexon_core::conky_procs::log_tail(
            &rexon_core::animations::SystemIo,
            &rexon_core::de_apply::Ctx::live(),
            &name,
        ))
    }

    fn open_conky_folder(&self, item_id: &QString) -> bool {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let io = rexon_core::animations::SystemIo;
        match rexon_core::conky_procs::widget_folder(&io, &rexon_core::de_apply::Ctx::live(), &name) {
            Some(dir) => crate::extensions_actions::qobject::open_folder_url(&QString::from(&dir)),
            None => false,
        }
    }

    fn conky_studio_editable(&self, item_id: &QString) -> bool {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        rexon_core::conky_theme::studio_edit_argv(&rexon_core::animations::SystemIo, &rexon_core::de_apply::Ctx::live(), &name)
            .is_some()
    }

    fn edit_conky_in_studio(mut self: Pin<&mut Self>, item_id: &QString) {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let (ok, msg) =
            rexon_core::conky_theme::edit_in_studio(&rexon_core::animations::SystemIo, &rexon_core::de_apply::Ctx::live(), &name);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
    }

    fn open_conky_file(&self, item_id: &QString, conf: &QString) -> bool {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        match rexon_core::conky_procs::part_file(&io, &ctx, &name, &conf.to_string()) {
            Some(file) => crate::extensions_actions::qobject::open_folder_url(&QString::from(&file)),
            None => false,
        }
    }

    fn conky_settings(&self, item_id: &QString, conf: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let view = rexon_core::conky_overlay::live_settings_view(&name, &conf.to_string());
        QString::from(&serde_json::to_string(&view).unwrap_or_default())
    }

    fn set_conky_setting(
        mut self: Pin<&mut Self>,
        item_id: &QString,
        conf: &QString,
        key: &QString,
        value: &QString,
    ) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let value: Value = serde_json::from_str(&value.to_string()).unwrap_or(Value::Null);
        let (ok, msg) = rexon_core::conky_overlay::live_set(&name, &conf.to_string(), &key.to_string(), &value);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        self.conky_settings(item_id, conf)
    }

    fn reset_conky_settings(mut self: Pin<&mut Self>, item_id: &QString, conf: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let (ok, msg) = rexon_core::conky_overlay::live_reset(&name, &conf.to_string());
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        self.conky_settings(item_id, conf)
    }

    fn conky_colours(&self, item_id: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        QString::from(&serde_json::to_string(&rexon_core::conky_colors::live_view(&name)).unwrap_or_default())
    }

    fn set_conky_colour(mut self: Pin<&mut Self>, item_id: &QString, from: &QString, to: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let (ok, msg) = rexon_core::conky_colors::live_set(&name, &from.to_string(), &to.to_string());
        if !msg.is_empty() {
            self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        }
        self.conky_colours(item_id)
    }

    fn revert_conky_colours(mut self: Pin<&mut Self>, item_id: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        // The files were edited after Rexon too: reverting will erase those edits
        // as well — a question first.
        if let Some((title, text)) = rexon_core::conky_colors::live_revert_question(&name) {
            if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                return self.conky_colours(item_id);
            }
        }
        let (ok, msg) = rexon_core::conky_colors::live_revert(&name);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        self.conky_colours(item_id)
    }

    fn conky_texts(&self) -> QString {
        QString::from(&serde_json::to_string(&rexon_core::conky_procs::section_texts()).unwrap_or_default())
    }

    fn conky_ribbon(&self) -> QString {
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        let extra = rexon_core::conky_scan::extra_dirs_for(&ctx);
        let r = rexon_core::conky_procs::ribbon(&io, &ctx, &extra);
        QString::from(&serde_json::to_string(&r).unwrap_or_else(|_| "[]".into()))
    }

    fn stop_conky_ribbon_item(mut self: Pin<&mut Self>, item_id: &QString) -> bool {
        use rexon_core::conky_procs;
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        let extra = rexon_core::conky_scan::extra_dirs_for(&ctx);
        let id = item_id.to_string();
        let Some(it) = conky_procs::ribbon(&io, &ctx, &extra).into_iter().find(|i| i.item_id == id) else {
            return false;
        };
        let (ok, msg) = conky_procs::stop_confs(&io, &ctx, &it.title, &it.confs);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        ok
    }

    fn conky_sets(&self) -> QString {
        let v = serde_json::json!({
            "texts": rexon_core::conky_sets::texts(),
            "rows": rexon_core::conky_sets::live_rows(),
        });
        QString::from(&v.to_string())
    }

    fn save_conky_set(mut self: Pin<&mut Self>, name: &QString) -> bool {
        let (ok, msg) = rexon_core::conky_sets::live_save(&name.to_string());
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        ok
    }

    fn apply_conky_set(mut self: Pin<&mut Self>, name: &QString) {
        use rexon_core::conky_sets as sets;
        let (plan, question) = match sets::live_plan(&name.to_string()) {
            Ok(x) => x,
            Err(why) => {
                self.as_mut().action_done(QString::from(&ap::action_result_status(false, &why)), false);
                return;
            }
        };
        // The question about commands — in the window's thread, BEFORE the
        // background work; "No" means the set is not applied at all (half a set is
        // worse than none).
        let trusted = match question {
            Some((title, text)) => {
                if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                    return;
                }
                true
            }
            None => false,
        };
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let (ok, msg) = sets::live_apply(&plan, trusted);
            finish_action(&state, &handle, msg, ok);
        });
    }

    fn export_conky_set(mut self: Pin<&mut Self>, name: &QString) -> bool {
        use rexon_core::conky_sets as sets;
        let name = name.to_string();
        let (title, filter) = sets::export_dialog();
        let chosen = crate::extensions_actions::qobject::save_file(
            &QString::from(&title),
            &QString::from(&filter),
            &QString::from(&sets::export_suggested(&home(), &name)),
        )
        .to_string();
        if chosen.is_empty() {
            return false;
        }
        let dest = sets::export_path(&chosen);
        let mut result = sets::live_export(&name, &dest, false);
        if let Err(sets::ExportError::Private(found)) = &result {
            let (q_title, q_text) = sets::export_private_question(&name, found);
            if !crate::extensions_actions::ask_confirm(&QString::from(&q_title), &QString::from(&q_text)) {
                return false;
            }
            result = sets::live_export(&name, &dest, true);
        }
        let (ok, msg) = match result {
            Ok(done) => (true, sets::export_message(&done)),
            Err(sets::ExportError::Failed(why)) => (false, why),
            Err(sets::ExportError::Private(_)) => (false, String::new()),
        };
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        ok
    }

    fn remove_conky_set(mut self: Pin<&mut Self>, name: &QString) -> bool {
        let name = name.to_string();
        let (title, text) = rexon_core::conky_sets::remove_question(&name);
        if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
            return false;
        }
        let (ok, msg) = rexon_core::conky_sets::live_remove(&name);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        ok
    }

    fn stop_all_conky(mut self: Pin<&mut Self>) -> bool {
        use rexon_core::conky_procs;
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        let extra = rexon_core::conky_scan::extra_dirs_for(&ctx);
        let procs = conky_procs::all(&io);
        // The question comes with a list of what will be stopped; exactly the
        // shown numbers are stopped (the core will check once more that it is
        // conky).
        let Some((title, text)) = conky_procs::stop_all_question(&ctx, &extra, &procs) else {
            let (ok, msg) = conky_procs::stop_all(&io, &[]);
            self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
            return false;
        };
        if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
            return false;
        }
        let pids: Vec<String> = procs.into_iter().map(|p| p.pid).collect();
        let (ok, msg) = conky_procs::stop_all(&io, &pids);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        ok
    }

    fn watch_conky_disk(mut self: Pin<&mut Self>) {
        let state = self.as_ref().get_ref().rust().state.clone();
        if state.conky_watch.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let handle = self.as_mut().qt_thread();
        // Widget snapshots for the tiles — in the background, on an invisible
        // screen; when ready — a signal to the section.
        rexon_core::conky_preview::enable();
        let preview_handle = handle.clone();
        rexon_core::conky_preview::watch(Box::new(move || {
            !matches!(
                preview_handle.queue(|mut o| o.as_mut().conky_previews_ready()),
                Err(cxx_qt::ThreadingQueueError::ObjectDestroyed)
            )
        }));
        let ctx = rexon_core::de_apply::Ctx::live();
        let targets = rexon_core::conky_scan::watch_targets(&ctx, &rexon_core::conky_scan::extra_dirs_for(&ctx));
        std::thread::spawn(move || {
            let Some(mut watch) = rexon_core::conky_scan::Watch::new(targets) else {
                rexon_core::log_warn!("appearance", "слежка за виджетами Conky не запущена: inotify недоступен");
                return;
            };
            loop {
                if !watch.wait(-1) {
                    continue;
                }
                // Unpacking a theme is hundreds of entries in a row: say it once, after a
                // second of silence.
                while watch.wait(1000) {}
                if handle.queue(|mut o| o.as_mut().conky_disk_changed()).is_err() {
                    return; // the page is gone
                }
            }
        });
    }

    fn set_conky_preview_screen(&self, width: i32, height: i32, dpi: i32) {
        rexon_core::conky_preview::set_screen(width.max(0) as u32, height.max(0) as u32, dpi.max(0) as u32);
    }

    fn conky_screen_map(&self, item_id: &QString, conf: &QString, screens: &QString) -> QString {
        let id = item_id.to_string();
        let name = if id.is_empty() { String::new() } else { ap::local_theme_name(&id, "") };
        let map = rexon_core::conky_layout::live_map(&name, &conf.to_string(), &screens.to_string());
        QString::from(&serde_json::to_string(&map).unwrap_or_default())
    }

    #[allow(clippy::too_many_arguments)]
    fn move_conky_widget(
        mut self: Pin<&mut Self>,
        item_id: &QString,
        conf: &QString,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        snap: bool,
        screens: &QString,
    ) -> bool {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let r = rexon_core::conky_layout::Rect { x, y, w, h };
        let (ok, msg) = rexon_core::conky_layout::live_move(&name, &conf.to_string(), r, snap, &screens.to_string());
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        ok
    }

    fn restore_conky_position(mut self: Pin<&mut Self>, item_id: &QString, conf: &QString, saved: &QString) -> bool {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let saved: Value = serde_json::from_str(&saved.to_string()).unwrap_or(Value::Null);
        let (ok, msg) = rexon_core::conky_layout::live_restore(&name, &conf.to_string(), &saved);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        ok
    }

    fn conky_xvfb_hint(&self) -> QString {
        let hint = rexon_core::conky_preview::live_xvfb_hint();
        QString::from(&serde_json::to_string(&hint).unwrap_or_default())
    }

    fn close_conky_xvfb_hint(&self) {
        rexon_core::conky_preview::close_xvfb_hint();
    }

    fn install_conky_xvfb(mut self: Pin<&mut Self>) {
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let (ok, msg) = rexon_core::conky_preview::install_xvfb_live();
            let _ = handle.queue(move |mut o| {
                o.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
                if ok {
                    o.as_mut().conky_disk_changed();
                }
            });
        });
    }

    fn open_url(&self, url: &QString) -> bool {
        if url.to_string().is_empty() {
            return false;
        }
        qobject::open_url_external(url)
    }

    fn open_linked_card(mut self: Pin<&mut Self>, request: &QString) {
        let Ok(req) = serde_json::from_str::<Value>(&request.to_string()) else {
            return;
        };
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let url = s(&req, "url").to_string();
            let card = req.get("card").cloned().unwrap_or(Value::Null);
            let payload = match resolve_linked_card(&state, &card) {
                Ok(v) => v,
                Err(why) => {
                    rexon_core::log_warn!("link", "карточка по ссылке {url}: {why}");
                    json!({
                        "failed": true,
                        "url": url,
                        "status": rexon_core::link_policy::card_failed_status(),
                    })
                }
            }
            .to_string();
            post(&handle, move |mut object| {
                object.as_mut().linked_card_ready(QString::from(&payload));
            });
        });
    }

    fn demo_splash(mut self: Pin<&mut Self>, theme: &QString) -> QString {
        use std::sync::atomic::{AtomicBool, Ordering};
        use rexon_core::appearance_installer as ainst;
        // An SDDM theme tile: its button is the login screen preview.
        if theme.to_string().starts_with(ap::LOGIN_PREVIEW_PREFIX) {
            return self.preview_login_screen(theme);
        }
        // One show at a time, as in the KDE module (the button waits for the end
        // of the show).
        static RUNNING: AtomicBool = AtomicBool::new(false);
        let Some(argv) = ainst::splash_demo_argv(&theme.to_string()) else {
            return QString::default();
        };
        if RUNNING.swap(true, Ordering::SeqCst) {
            return QString::default();
        }
        let child = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn();
        let child = match child {
            Ok(c) => c,
            Err(e) => {
                RUNNING.store(false, Ordering::SeqCst);
                rexon_core::log_warn!("theme", "показ заставки: {e}");
                return QString::from(&if e.kind() == std::io::ErrorKind::NotFound {
                    ainst::splash_demo_missing()
                } else {
                    ainst::splash_demo_failed(&e.to_string())
                });
            }
        };
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let out = child.wait_with_output();
            RUNNING.store(false, Ordering::SeqCst);
            let failed = match out {
                Ok(o) if o.status.success() => None,
                Ok(o) => Some(String::from_utf8_lossy(&o.stderr).trim().to_string()),
                Err(e) => Some(e.to_string()),
            };
            if let Some(detail) = failed {
                rexon_core::log_warn!("theme", "показ заставки не удался: {detail}");
                let text = QString::from(&ainst::splash_demo_failed(&detail));
                let _ = handle.queue(move |mut o| o.as_mut().status_message(text));
            }
        });
        QString::default()
    }

    fn cancel_previews(&self) {
        let Ok(mut jobs) = self.rust().state.preview_cancels.lock() else {
            return;
        };
        for (_, token) in jobs.drain() {
            token.cancel();
        }
    }
}

// ============================================================================
// Long work: the catalogue
// ============================================================================

impl qobject::AppearanceActions {
    fn load_catalog(mut self: Pin<&mut Self>, request: &QString) {
        let Ok(req) = serde_json::from_str::<Value>(&request.to_string()) else {
            return;
        };
        let key = s(&req, "key").to_string();
        let kind = s(&req, "kind").to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        // The previous section's requests are no longer needed — we abort them at
        // once without waiting for an answer: the channel is freed for what was
        // opened.
        abort_catalog(&state, &key);

        let handle = self.as_mut().qt_thread();
        // The cancellable transport is set up BEFORE the thread: the page may
        // change the section before the thread gets to the first request.
        let token = CancelToken::new();
        if let Ok(mut cancels) = state.catalog_cancels.lock() {
            cancels.insert(key.clone(), token.clone());
        }
        std::thread::spawn(move || {
            if s(&req, "categoryKey") == gext::SECTION_KEY {
                start_gnome_update_check(&state, &handle);
            }
            let answer = match kind.as_str() {
                "combined" => fetch_combined(&state, &req, &token),
                other => fetch_kind(&state, &req, &token, other),
            };
            if let Ok(mut cancels) = state.catalog_cancels.lock() {
                cancels.remove(&key);
            }
            post(&handle, move |mut object| {
                object.as_mut().catalog_ready(QString::from(&answer));
            });
        });
    }

    fn load_prefetch(mut self: Pin<&mut Self>, request: &QString) {
        let Ok(req) = serde_json::from_str::<Value>(&request.to_string()) else {
            return;
        };
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let mut answer = req.clone();
            let mut items: Vec<Value> = Vec::new();
            let io = LiveOcsIo::new(&home());
            let preview_dir = io.preview_dir();
            let mut client = OcsClient::new(io, &preview_dir);
            // The warm-up warms up the same topped-up page that "Forward" will open:
            // all the server pages it is assembled from will land in the cache.
            let found = store_page(&mut client, &req).items;
            for c in &found {
                items.push(serde_json::to_value(&c.item).unwrap_or(Value::Null));
            }
            if let Some(map) = answer.as_object_mut() {
                map.insert("items".to_string(), Value::Array(items));
            }
            let payload = answer.to_string();
            post(&handle, move |mut object| {
                object.as_mut().prefetch_ready(QString::from(&payload));
            });
        });
    }
}

// ============================================================================
// Long work: pictures
// ============================================================================

impl qobject::AppearanceActions {
    fn load_preview(
        mut self: Pin<&mut Self>,
        job: &QString,
        _item_id: &QString,
        url: &QString,
        low: bool,
    ) {
        let job = job.to_string();
        let url = url.to_string();
        if url.is_empty() {
            self.as_mut()
                .preview_failed(QString::from(&job), QString::default());
            return;
        }
        // The core asks only about what it does not have, but the picture may have
        // arrived through a neighbouring request for the same address.
        let dest = ap::preview_cache_path(&home(), &url);
        if std::path::Path::new(&dest).is_file() {
            self.as_mut()
                .preview_done(QString::from(&job), QString::from(&dest));
            return;
        }

        let state = self.as_ref().get_ref().rust().state.clone();
        let token = CancelToken::new();
        if let Ok(mut jobs) = state.preview_cancels.lock() {
            jobs.insert(job.clone(), token.clone());
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // Warning: the low priority of a background request (`low`) is NOT
            // expressed here, and deliberately so: in Python it was set by
            // `QNetworkRequest::LowPriority` — a queue property of one
            // `QNetworkAccessManager`, which does not exist in the Rust path. The
            // request stays honest: how many pictures fly at once and in what order is
            // decided by the core's queue anyway (`PREVIEW_MAX_INFLIGHT` and
            // `PREVIEW_MAX_INFLIGHT_LOW` — two different ceilings), and a priority
            // inside an already selected batch does not change its decisions.
            let _ = low;
            // An AUR package tile carries a marker, not an address: the first README
            // picture, a Google Fonts font sample (`aur::preview_url`). It is resolved
            // once per session (`aur_image_url`).
            let real = aur_image_url(&state, &url);
            let net = UreqNet::new().with_cancel(token.clone());
            let io = LiveOcsIo::with_net(net, &home());
            let preview_dir = io.preview_dir();
            let mut client = OcsClient::new(io, &preview_dir);
            let outcome = real
                .and_then(|real| client.get_preview_image(&real))
                .filter(|p| usable_preview(p, &preview_dir));
            if let Ok(mut jobs) = state.preview_cancels.lock() {
                jobs.remove(&job);
            }
            let cancelled = token.is_cancelled();
            post(&handle, move |mut object| {
                match outcome {
                    Some(path) => object.as_mut().preview_done(
                        QString::from(&job),
                        QString::from(&path),
                    ),
                    // The core's queue does NOT repeat a cancellation: we changed the section
                    // ourselves, and a repeat would pull the previous section's pictures.
                    None if cancelled => object
                        .as_mut()
                        .preview_failed(QString::from(&job), QString::from("canceled")),
                    None => object
                        .as_mut()
                        .preview_failed(QString::from(&job), QString::default()),
                }
            });
        });
    }
}

// ============================================================================
// Long work: the details panel
// ============================================================================

impl qobject::AppearanceActions {
    fn load_detail(mut self: Pin<&mut Self>, item_id: &QString) {
        let id = item_id.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        let Some(item) = state.items.lock().ok().and_then(|m| m.get(&id).cloned()) else {
            return;
        };
        if let Ok(mut open) = state.detail_id.lock() {
            *open = id.clone();
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            fetch_detail(&state, &handle, item);
        });
    }

    fn load_gallery_frame(mut self: Pin<&mut Self>, index: &QString, url: &QString) {
        let index = index.to_string();
        let url = url.to_string();
        if url.is_empty() {
            self.as_mut().gallery_failed(QString::from(&index));
            return;
        }
        if index.parse::<i64>().is_err() {
            return;
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let io = LiveOcsIo::new(&home());
            let preview_dir = io.preview_dir();
            let mut client = OcsClient::new(io, &preview_dir);
            emit_frame(&handle, &mut client, &index, &url);
        });
    }

    /// The comments page: first THE LIST ITSELF, then the translations
    /// following it.
    ///
    /// ⭐ The order is the same as for the details panel: showing does not wait
    /// for anything that can be sent later. Python translated the comments
    /// BEFORE sending the list up, one by one, and all that time the user looked
    /// at an empty thread.
    fn load_comments(mut self: Pin<&mut Self>, item_id: &QString, page: &QString) {
        let id = item_id.to_string();
        let page: i64 = page.to_string().parse().unwrap_or(0);
        let state = self.as_ref().get_ref().rust().state.clone();
        // The list walk is marked HERE, in the interface thread, not in the worker
        // one: otherwise two requests in a row ("Show more" twice) could renumber
        // each other in reverse order.
        let view = mark_comments_view(&state, &id, page);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let io = LiveOcsIo::new(&home());
            let preview_dir = io.preview_dir();
            let mut client = OcsClient::new(io, &preview_dir);
            // `None` means the item has no comments section at all; in this case the
            // page must not show an empty thread.
            let Some(comments) = client.comments(&id, page, ap::COMMENTS_PAGESIZE as i64) else {
                return;
            };
            let mut rows: Vec<Value> = Vec::with_capacity(comments.len());
            for c in &comments {
                rows.push(json!({
                    "author": c.author,
                    "date": c.date,
                    "subject": c.subject,
                    "text": c.text,
                    "score": c.score,
                    "depth": c.depth,
                    // The translation is empty HERE deliberately: an empty string means "show
                    // the original", and the list waits for nothing from the translation. The
                    // translations catch up with it in a second pass
                    // (`translate_comment_page` below) and fix the already shown rows.
                    "translated": "",
                }));
            }
            let mut answer = Map::new();
            answer.insert("itemId".to_string(), Value::from(id.as_str()));
            answer.insert("page".to_string(), Value::from(page));
            answer.insert("comments".to_string(), Value::Array(rows));
            let count = state
                .comment_counts
                .lock()
                .ok()
                .and_then(|m| m.get(&id).copied())
                .unwrap_or(0);
            if count > 0 {
                answer.insert("total".to_string(), Value::from(count));
            }
            let payload = Value::Object(answer).to_string();
            post(&handle, move |mut object| {
                object.as_mut().comments_ready(QString::from(&payload));
            });

            let texts: Vec<String> = comments.iter().map(|c| c.text.clone()).collect();
            translate_comment_page(&state, &handle, &id, page, view, &texts);
        });
    }

    fn load_suggestions(mut self: Pin<&mut Self>, request: &QString) {
        let Ok(req) = serde_json::from_str::<Value>(&request.to_string()) else {
            return;
        };
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let io = LiveOcsIo::new(&home());
            let preview_dir = io.preview_dir();
            let mut client = OcsClient::new(io, &preview_dir);
            let (found, _total) = client.search(
                s(&req, "categoryId"),
                num(&req, "page"),
                i64::from(ap::PAGE_SIZE),
                s(&req, "sort"),
                s(&req, "query"),
                true,
            );
            let items: Vec<ThemeItem> = found.iter().map(|c| c.item.clone()).collect();
            remember_items(&state, &items);
            let payload = json!({
                "generation": num(&req, "generation"),
                "items": items,
            })
            .to_string();
            post(&handle, move |mut object| {
                object.as_mut().suggest_ready(QString::from(&payload));
            });
        });
    }
}

// ============================================================================
// Long work: installation, applying, removal
// ============================================================================

impl qobject::AppearanceActions {
    fn install_by_id(mut self: Pin<&mut Self>, item_id: &QString, apply_after: bool, file_name: &QString) {
        let id = item_id.to_string();
        let file_name = file_name.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        let Some(item) = state.items.lock().ok().and_then(|m| m.get(&id).cloned()) else {
            self.as_mut().action_done(QString::default(), false);
            return;
        };
        if ego::is_item_id(&item.id) {
            self.install_gnome_extension(item);
            return;
        }
        // The login splash slot is one for everyone — the dispute over it is
        // settled BEFORE the background work: the dialog lives in the interface
        // thread.
        let (apply_after, keep_video, note) = if apply_after {
            match splash_slot_answer(&item) {
                // The place stays with the clip. We do NOT cancel the installation though:
                // the customer asked for the theme for themselves, we refused only the
                // applying — and said so in words.
                ss::SplashSlotPlan::Stop(text) => (false, None, text),
                ss::SplashSlotPlan::Apply { keep_video, note } => (true, keep_video, note),
            }
        } else {
            (false, None, String::new())
        };
        // A Conky widget without the program itself: it is installed together with
        // the widget without a question (the protection is the password window).
        // If it did not install, the widget is installed but does not start.
        let conky = apply_after && conky_needs_install(&item);
        // The item file is 7z, and there is nothing to unpack it with: bsdtar is
        // installed without a question (the protection is the password window). If
        // it did not install, the item is not installed.
        let seven_zip = seven_zip_needs_install(&item, &file_name);
        rexon_core::log_info!(
            "theme",
            "установка «{}» (id {}, категория {}){}",
            item.name,
            item.id,
            item.category_id,
            if apply_after { " с применением" } else { "" }
        );
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            if seven_zip {
                if let Err(why) = rexon_core::appearance_installer::install_seven_zip_tool_live() {
                    rexon_core::log_warn!("theme", "bsdtar для «{}» не встал: {why}", item.name);
                    finish_action(&state, &handle, rexon_core::appearance_installer::seven_zip_install_failed(&why), false);
                    return;
                }
            }
            let mut apply_after = apply_after;
            let mut conky_note = None;
            if conky {
                if let Err(why) = de_apply::install_conky_live() {
                    apply_after = false;
                    conky_note = Some(de_apply::conky_install_failed(&why));
                }
            }
            let (mut msg, ok) = install_theme(&handle, item, apply_after, &file_name);
            finish_splash_slot(keep_video, &note, ok, &mut msg);
            if let Some(line) = conky_note {
                msg = format!("{msg}. ⚠️ {line}");
            }
            finish_action(&state, &handle, msg, ok);
        });
    }

    fn install_aur(mut self: Pin<&mut Self>, pkg_name: &QString) {
        let name = pkg_name.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        // What to install is decided by the core (`aur_install::plan_install`): a
        // fork package like AeroThemePlasma is installed as a set and only after a
        // warning. The question is synchronous, in the interface thread (the
        // dialog lives here); the list of what is installed is needed only for the
        // set.
        let plan = if aur_install::kit_for(&name).is_some() {
            let have = installed_packages();
            aur_install::plan_install(&name, &|p| have.contains(p))
        } else {
            aur_install::plan_install(&name, &|_| false)
        };
        let Some(helper) = aur_helper(&state) else {
            self.as_mut().action_done(QString::from(&ap::aur_no_builder()), false);
            return;
        };
        if let Some(question) = &plan.question {
            let yes = crate::extensions_actions::ask_confirm(
                &QString::from(&question.title),
                &QString::from(&question.text),
            );
            if !yes {
                return;
            }
        }
        let handle = self.as_mut().qt_thread();
        aur_subscribe(handle, state);
        let log_path = aur_install::log_path(&home(), &name);
        let queued = match aur_runner().queue.lock() {
            Ok(mut q) => q.enqueue(&name, plan, &helper, &log_path),
            Err(_) => return,
        };
        rexon_core::log_info!(
            "aur",
            "в очередь: «{name}» помощником {helper}{}",
            if queued.start_worker { ", очередь была пуста" } else { "" }
        );
        aur_broadcast();
        if queued.start_worker {
            std::thread::spawn(aur_worker);
        }
    }

    fn aur_jobs(mut self: Pin<&mut Self>) -> QString {
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        aur_subscribe(handle, state);
        QString::from(&aur_snapshot())
    }

    fn open_aur_log(&self, pkg_name: &QString) -> bool {
        let path = aur_install::log_path(&home(), &pkg_name.to_string());
        if !std::path::Path::new(&path).is_file() {
            return false;
        }
        qobject::open_url_external(&QString::from(&format!("file://{path}")))
    }

    /// Install (update) a Cinnamon spice: download from the website, verify,
    /// lay out — in a thread, with a progress bar.
    fn install_spice(mut self: Pin<&mut Self>, item: ThemeItem) {
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let progress_handle = handle.clone();
            let progress: Box<dyn FnMut(i64, i64) + Send> = Box::new(move |done, total| {
                if total > 0 {
                    let pct = (done * 100 / total) as i32;
                    post(&progress_handle, move |mut object| {
                        object.as_mut().install_progress(pct);
                    });
                }
            });
            rexon_core::log_info!("theme", "установка из Cinnamon Spices: {}", item.id);
            let (ok, msg) = cs::live_install(&item, log_sink(&handle), Some(progress));
            finish_action(&state, &handle, msg, ok);
        });
    }

    /// Install a GNOME extension: through the shell (its confirmation window,
    /// works at once), without it — the release zip for this GNOME version
    /// (will work at the next login). If it worked — an entry in the installed
    /// journal (`provider_id = ego`).
    fn install_gnome_extension(mut self: Pin<&mut Self>, item: ThemeItem) {
        let Some(uuid) = ego::parse_item_id(&item.id).map(str::to_string) else {
            self.as_mut().action_done(QString::default(), false);
            return;
        };
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        rexon_core::log_info!("gnome-ext", "установка: {uuid}");
        std::thread::spawn(move || {
            let data_home = rexon_core::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), &home());
            let mut io = rexon_core::gnome_shell_ext::LiveShell::new();
            let shell = ego_shell_version();
            let token = CancelToken::new();
            let mut fetch = || {
                let work = mkdtemp("rexon-gext-").ok_or_else(|| ap::install_download_failed())?;
                gext::live_fetch_zip(&uuid, shell.as_ref(), std::path::Path::new(&work), &token)
            };
            let name = if item.name.is_empty() { uuid.clone() } else { item.name.clone() };
            let (ok, msg) = match gext::install(&mut io, &uuid, &data_home, &mut fetch) {
                Ok(outcome) => {
                    if outcome != rexon_core::gnome_shell_ext::InstallOutcome::Cancelled {
                        let version = rexon_core::gnome_shell_ext::extension_info(&mut io, &uuid)
                            .ok()
                            .flatten()
                            .map(|i| i.version)
                            .unwrap_or_else(|| item.version.clone());
                        let entry = gext::journal_item(&uuid, &name, &version, &item.author);
                        let now = rexon_core::engine_update::now_iso(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs() as i64)
                                .unwrap_or(0),
                        );
                        if let Err(e) = gext::live_journal_edit(&home(), |log| {
                            gext::journal_record(log, &entry, &data_home, &uuid, &now)
                        }) {
                            rexon_core::log_warn!("gnome-ext", "журнал установленного: {e}");
                        }
                    }
                    gext::install_message(&name, outcome)
                }
                Err(e) => (false, e.message()),
            };
            finish_action(&state, &handle, msg, ok);
        });
    }

    fn gnome_extension_action(mut self: Pin<&mut Self>, item_id: &QString, action: &QString) {
        let id = item_id.to_string();
        let (Some(uuid), Some(act)) = (gext::uuid_of_id(&id).map(str::to_string), gext::Action::parse(&action.to_string()))
        else {
            self.as_mut().action_done(QString::default(), false);
            return;
        };
        let state = self.as_ref().get_ref().rust().state.clone();
        let disk_path = state
            .items
            .lock()
            .ok()
            .and_then(|m| m.get(&id).map(|it| it.install_path.clone()))
            .unwrap_or_default();
        let handle = self.as_mut().qt_thread();
        rexon_core::log_info!("gnome-ext", "действие {act:?}: {uuid}");
        std::thread::spawn(move || {
            let data_home = rexon_core::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), &home());
            let mut io = rexon_core::gnome_shell_ext::LiveShell::new();
            let answer = gext::run_action(&mut io, act, &uuid, &data_home, &disk_path);
            let ok = answer["ok"].as_bool().unwrap_or(false);
            let msg = answer["message"].as_str().unwrap_or("").to_string();
            if ok && act == gext::Action::Uninstall {
                if let Err(e) = gext::live_journal_edit(&home(), |log| {
                    gext::journal_forget(log, &uuid);
                }) {
                    rexon_core::log_warn!("gnome-ext", "журнал установленного: {e}");
                }
            }
            if matches!(act, gext::Action::Toggle | gext::Action::Enable | gext::Action::Disable | gext::Action::Uninstall) {
                finish_action(&state, &handle, msg, ok);
            } else if !ok {
                rexon_core::log_warn!("gnome-ext", "{act:?} {uuid}: {msg}");
            }
            let folder = answer["folder"].as_str().unwrap_or("").to_string();
            let path = answer["path"].as_str().unwrap_or("").to_string();
            let payload = answer.to_string();
            post(&handle, move |mut object| {
                match folder.as_str() {
                    "open" => {
                        qobject::open_url_external(&QString::from(&format!("file://{path}")));
                    }
                    "copy" => crate::we_action::qobject::rexon_we_copy_to_clipboard(&QString::from(&path)),
                    _ => {}
                }
                object.as_mut().gnome_extension_done(QString::from(&payload));
            });
        });
    }

    fn gnome_ext_texts(&self) -> QString {
        QString::from(&gext::page_texts().to_string())
    }

    fn gnome_updates(&self) -> QString {
        let n = self.rust().state.gnome_updates.lock().map(|n| *n).unwrap_or(0);
        QString::from(&gext::updates_answer(n).to_string())
    }

    fn log_out_for_updates(&self) -> bool {
        let (program, args) = gext::LOGOUT_ARGV.split_first().expect("команда выхода");
        match std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => true,
            Err(e) => {
                rexon_core::log_warn!("gnome-ext", "выход из сеанса: {e}");
                false
            }
        }
    }

    fn install_files(mut self: Pin<&mut Self>, paths_json: &QString, section: &QString) {
        let paths: Vec<String> = serde_json::from_str(&paths_json.to_string()).unwrap_or_default();
        let section = section.to_string();
        if paths.is_empty() {
            self.as_mut().action_done(QString::default(), false);
            return;
        }
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        rexon_core::log_info!("theme", "установка из файла в раздел «{section}»: {} шт.", paths.len());
        std::thread::spawn(move || {
            let outcome = rexon_core::local_install::live_install(&paths, &section, log_sink(&handle));
            let payload = serde_json::to_string(&outcome).unwrap_or_default();
            // Some files got installed, some did not: the outcome is "failed", but new
            // themes are already on disk — reset the scan cache anyway.
            if outcome.installed && !outcome.ok {
                if let Ok(mut cache) = state.scan_cache.lock() {
                    cache.clear();
                }
            }
            finish_action(&state, &handle, outcome.message.clone(), outcome.ok);
            post(&handle, move |mut object| {
                object.as_mut().local_install_done(QString::from(&payload));
            });
        });
    }

    fn apply_local_by_id(mut self: Pin<&mut Self>, item_id: &QString) {
        let id = item_id.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        let Some(item) = state.items.lock().ok().and_then(|m| m.get(&id).cloned()) else {
            self.as_mut().action_done(QString::default(), false);
            return;
        };
        let name = ap::local_theme_name(&item.id, &item.name);
        self.apply_item(item, name);
    }

    fn global_apply_question(&self, item_id: &QString) -> QString {
        let id = item_id.to_string();
        let item = self.rust().state.items.lock().ok().and_then(|m| m.get(&id).cloned());
        let Some((item, name)) = item.and_then(|item| global_target(&item).map(|n| (item, n))) else {
            return QString::default();
        };
        let display = ap::card_name(&item.name);
        let display = if display.is_empty() { name.as_str() } else { display };
        let io = rexon_core::animations::SystemIo;
        // Outside Plasma the window says which parts are transferred to this
        // desktop.
        let de = LiveInstaller::new("", Box::new(|_: &str| {})).de_id().to_string();
        match rexon_core::themes::global_question_live(&io, &home(), &name, display, &de) {
            Some(q) => {
                let mut v = serde_json::to_value(q).unwrap_or_default();
                v["itemId"] = json!(id);
                QString::from(&v.to_string())
            }
            None => QString::default(),
        }
    }

    fn apply_global_by_id(mut self: Pin<&mut Self>, item_id: &QString, appearance: bool, layout: bool, parts: i32) {
        let id = item_id.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        let item = state.items.lock().ok().and_then(|m| m.get(&id).cloned());
        let Some((item, name)) = item.and_then(|item| global_target(&item).map(|n| (item, n))) else {
            self.as_mut().action_done(QString::default(), false);
            return;
        };
        let parts = u32::try_from(parts).ok();
        let choice = rexon_core::themes::GlobalChoice { appearance, layout, parts };
        self.apply_item_with(item, name, Some(choice));
    }

    fn act_on_store_item(mut self: Pin<&mut Self>, item_id: &QString) {
        let id = item_id.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        let Some(item) = state.items.lock().ok().and_then(|m| m.get(&id).cloned()) else {
            self.as_mut().action_done(QString::default(), false);
            return;
        };
        if item.id.starts_with(cs::ID_PREFIX) {
            // A Spices theme in the GTK or window borders section: if installed and
            // not outdated, apply it by the same path as a theme from disk (a GTK
            // theme, xfwm4 or Marco borders — by the card's section).
            if let Some(name) = cs::live_part_apply_name(&item) {
                self.apply_item(item, name);
                return;
            }
            self.install_spice(item);
            return;
        }
        // A GNOME extension is installed by the shell itself (the card has no
        // archive links — the shared installation path does not suit them).
        if ego::is_item_id(&item.id) {
            self.install_gnome_extension(item);
            return;
        }
        let home = home();
        let installed = ap::live_logged_theme_name(&home, &item.id);
        let path = std::path::Path::new(&home);
        // An installed Conky widget or add-on — start/stop or enable/disable by the
        // same path as for an installed tile (the widget's command permission
        // window, missing programs).
        let can_apply = ap::store_item_can_apply(path, &item) || ap::store_item_switch(path, &item).is_some();
        match (ap::store_card_action(installed.is_some(), can_apply), installed) {
            (ap::StoreCardAction::ApplyInstalled, Some(name)) => self.apply_item(item, name),
            _ => self.install_by_id(item_id, false, &QString::default()),
        }
    }

    /// Apply a theme that is already on disk (`name` is the directory/file
    /// name): a local tile and an installed store item are one path, with the
    /// same questions (the splash, the icons and cursor of a GTK theme, the
    /// Conky program).
    fn apply_item(self: Pin<&mut Self>, item: ThemeItem, name: String) {
        self.apply_item_with(item, name, None);
    }

    /// [`Self::apply_item`] with the answer of the "Apply "X"?" window of a
    /// global theme (`None` — there was no window).
    fn apply_item_with(
        mut self: Pin<&mut Self>,
        item: ThemeItem,
        name: String,
        global: Option<rexon_core::themes::GlobalChoice>,
    ) {
        let state = self.as_ref().get_ref().rust().state.clone();
        // The "old format theme" refusal is instant and synchronous: while it went
        // through background work, that work managed to die before the delivery,
        // and the user did not see the reason at all.
        if let Some(name) = stale_global(&item, &name) {
            self.as_mut()
                .action_done(QString::from(&ap::stale_global_msg(&name)), false);
            return;
        }
        // The same dispute over the splash slot as on installing from the store,
        // and synchronous for the same reason. Here "keep the clip" has no second
        // outcome: the theme is already on disk, applying is the only thing asked
        // for.
        let (keep_video, note) = match splash_slot_answer(&item) {
            ss::SplashSlotPlan::Stop(text) => {
                self.as_mut().action_done(QString::from(&text), false);
                return;
            }
            ss::SplashSlotPlan::Apply { keep_video, note } => (keep_video, note),
        };
        // The theme is designed for its own icons: ask once whether to enable them
        // (decision 3a; the answer is remembered for the theme). "Keep mine" — the
        // theme is applied entirely except for the icons.
        let probe = LiveInstaller::new("", Box::new(|_: &str| {}));
        let icons_choice = probe.theme_icons_question(&item, &name).map(|q| {
            let yes = crate::extensions_actions::ask_confirm_labeled(&q.title, &q.text, &q.yes, &q.no);
            rexon_core::theme_watch::remember_icons_choice(&name, yes);
            yes
        });
        // A Conky widget without the program itself: it is installed without a
        // question (the protection is the password window). If it did not install,
        // the widget does not start, and it is said why.
        let conky = conky_needs_install(&item);
        // The widget executes commands or scripts: show what exactly, and start
        // only with permission (`conky_safety`).
        if de_apply::is_conky_item(&item) && ask_conky_safety(&name, None) == Some(false) {
            self.as_mut()
                .action_done(QString::from(&rexon_core::conky_safety::declined_text(&name)), false);
            return;
        }
        // What the widget lacks: programs with a package in the repositories are
        // installed without a question (the protection is the polkit password
        // window); for what cannot be installed, the widget does not start.
        let check_programs = de_apply::is_conky_item(&item) && conky_will_start(&name);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            if conky {
                if let Err(why) = de_apply::install_conky_live() {
                    finish_action(&state, &handle, de_apply::conky_install_failed(&why), false);
                    return;
                }
            }
            // "All or nothing": missing programs from the repositories are installed
            // by themselves (the protection is the password window); if they did not
            // install or Rexon cannot install them, the widget does not start, and the
            // outcome says what to do. A shared path with "Apply" of a set
            // (`conky_safety::prepare_start`).
            let mut files_note = None;
            if check_programs {
                use rexon_core::conky_safety as cs;
                match cs::live_prepare_start(std::slice::from_ref(&name), cs::Retry::Tile).pop() {
                    Some(Err(text)) => {
                        finish_action(&state, &handle, text, false);
                        return;
                    }
                    Some(Ok(note)) => files_note = note,
                    None => {}
                }
            }
            let logger = log_sink(&handle);
            let mut installer = LiveInstaller::new("", logger);
            if icons_choice.is_some() {
                installer.set_icons_choice(icons_choice);
            }
            installer.set_global_choice(global);
            // A GTK theme comes with its own cursor (and icons, if it was chosen to
            // enable them), but they are missing: they are installed without a
            // question (the protection is the password window). If they did not
            // install, the theme is not applied, and it is said why.
            if let Err(why) = installer.prepare_theme_deps(&item, &name) {
                rexon_core::log_warn!("theme", "наборы темы «{name}» не встали: {why}");
                finish_action(&state, &handle, why, false);
                return;
            }
            rexon_core::log_info!("theme", "применение «{name}» (id {}, вид {:?})", item.id, item.local_kind);
            let (ok, mut msg) = installer.apply(&item, &name);
            finish_splash_slot(keep_video, &note, ok, &mut msg);
            if ok {
                add_shell_palette_note(&item, &mut msg);
                if let Some(n) = files_note {
                    msg = format!("{msg} {n}");
                }
            }
            finish_action(&state, &handle, msg, ok);
        });
    }

    fn cursor_size(&self) -> i32 {
        let installer = LiveInstaller::new("", Box::new(|_: &str| {}));
        installer.cursor_size().map(|n| n as i32).unwrap_or(0)
    }

    fn apply_cursor_size(mut self: Pin<&mut Self>, size: i32) {
        let Ok(size) = u32::try_from(size) else {
            return;
        };
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let logger = log_sink(&handle);
            let mut installer = LiveInstaller::new("", logger);
            let (ok, msg) = installer.apply_cursor_size(size);
            finish_action(&state, &handle, msg, ok);
        });
    }

    fn uninstall(mut self: Pin<&mut Self>, item_json: &QString) {
        let Ok(raw) = serde_json::from_str::<Value>(&item_json.to_string()) else {
            return;
        };
        let id = s(&raw, "id").to_string();
        if id.is_empty() {
            return;
        }
        // A theme from disk is not listed in the installed journal: its "Remove"
        // goes by the path from the scan.
        if id.starts_with("local:") {
            self.remove_local_by_id(&QString::from(&id));
            return;
        }
        // The purchase folder contains separately installed parts (window borders
        // in a GTK theme) — removal will take them too: ask.
        if let Some((title, text)) = removal::live_uninstall_question(&id) {
            if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                return;
            }
        }
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        spawn_uninstall(state, handle, id, false);
    }

    fn remove_local_by_id(mut self: Pin<&mut Self>, item_id: &QString) {
        let id = item_id.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        if self.as_mut().mark_gnome_removal(&state, &id) {
            return;
        }
        let Some(item) = state.items.lock().ok().and_then(|m| m.get(&id).cloned()) else {
            self.as_mut().action_done(QString::default(), false);
            return;
        };
        let live = LiveRemoval::new();
        match live.verdict(&item) {
            Verdict::Local { path } => {
                // A theme folder with separately installed borders inside — ask.
                if let Some((title, text)) = live.separate_parts_question(&path, &item.name) {
                    if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                        return;
                    }
                }
                rexon_core::log_info!("theme", "к удалению: {path}");
                let text = match state.removals.lock() {
                    Ok(mut queue) => queue.mark(&path, &item.name, &item.id),
                    Err(_) => return,
                };
                self.as_mut().removal_pending(QString::from(&text));
            }
            Verdict::Journal { content_id } => {
                let handle = self.as_mut().qt_thread();
                spawn_uninstall(state, handle, content_id, true);
            }
            Verdict::Loose { path } => {
                let handle = self.as_mut().qt_thread();
                std::thread::spawn(move || {
                    let logger = log_sink(&handle);
                    let mut installer = LiveInstaller::new("", logger);
                    rexon_core::log_info!("theme", "удаление системной темы без пакета: {path}");
                    let (ok, msg) = installer.remove_loose_system_theme(&path);
                    finish_action(&state, &handle, msg.clone(), ok);
                    post(&handle, move |mut object| object.as_mut().removal_done(QString::from(&msg)));
                });
            }
            Verdict::FoundConky { path } => {
                // A widget not installed through Rexon: ask (for the user's own folder —
                // twice) right here, in the interface thread; remove it in the background
                // (the processes stop, the folder goes to the trash).
                let questions = match rexon_core::conky_scan::live_found_removal(&path) {
                    Ok(Some(r)) => rexon_core::conky_scan::found_removal_questions(&r),
                    Ok(None) => return,
                    Err(reason) => {
                        self.as_mut().action_done(QString::from(&reason), false);
                        return;
                    }
                };
                for (title, text) in questions {
                    if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                        return;
                    }
                }
                let handle = self.as_mut().qt_thread();
                std::thread::spawn(move || {
                    rexon_core::log_info!("theme", "виджет Conky не из Rexon — в корзину: {path}");
                    let (ok, msg) = rexon_core::conky_scan::live_remove_found(&path);
                    if let Ok(mut cache) = state.scan_cache.lock() {
                        cache.clear();
                    }
                    finish_action(&state, &handle, msg.clone(), ok);
                    post(&handle, move |mut object| object.as_mut().removal_done(QString::from(&msg)));
                });
            }
            Verdict::RunningConky { path } => {
                // Our own widget is running: ask "stop and remove?"; "No" means touch
                // nothing. Removing goes in the background.
                let (title, text) = rexon_core::conky_scan::running_removal_question(&path);
                if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                    return;
                }
                let handle = self.as_mut().qt_thread();
                let item_id = item.id.clone();
                std::thread::spawn(move || {
                    rexon_core::log_info!("theme", "запущенный виджет Conky — остановить и в корзину: {path}");
                    let (ok, msg) = removal::live_remove_running_conky(&path, &item_id);
                    if let Ok(mut cache) = state.scan_cache.lock() {
                        cache.clear();
                    }
                    finish_action(&state, &handle, msg.clone(), ok);
                    post(&handle, move |mut object| object.as_mut().removal_done(QString::from(&msg)));
                });
            }
            Verdict::Blocked { reason } => {
                self.as_mut().action_done(QString::from(&reason), false);
            }
            Verdict::Hidden => {}
        }
    }

    /// "🗑" for a GNOME extension the shell knows (both from disk and from the
    /// showcase): a question with its name, after "Yes" — a mark for removal and
    /// an "Undo" line, as for themes; the shell will remove it
    /// (`commit_pending`). A system one is not removed — a refusal in words.
    /// `false` means not an extension or there is no shell: the ordinary path of
    /// a theme from disk.
    fn mark_gnome_removal(mut self: Pin<&mut Self>, state: &Arc<State>, id: &str) -> bool {
        let Some(uuid) = gext::uuid_of_id(id) else {
            return false;
        };
        let Some(info) = gext::live_snapshot().and_then(|l| l.into_iter().find(|e| e.uuid == uuid)) else {
            return false;
        };
        if info.is_system {
            let msg = rexon_core::gnome_shell_ext::ShellExtError::SystemExtension.message();
            self.as_mut().action_done(QString::from(&msg), false);
            return true;
        }
        let name = state
            .items
            .lock()
            .ok()
            .and_then(|m| m.get(id).map(|it| it.name.clone()))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| if info.name.is_empty() { uuid.to_string() } else { info.name.clone() });
        let (title, text) = gext::removal_question(&name);
        if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
            return true;
        }
        rexon_core::log_info!("gnome-ext", "к удалению: {uuid}");
        let text = match state.removals.lock() {
            Ok(mut queue) => queue.mark(&info.path, &name, id),
            Err(_) => return true,
        };
        self.as_mut().removal_pending(QString::from(&text));
        true
    }

    fn commit_removals(mut self: Pin<&mut Self>) {
        let state = self.as_ref().get_ref().rust().state.clone();
        let pending = state.removals.lock().map(|mut q| q.take_all()).unwrap_or_default();
        if pending.is_empty() {
            return;
        }
        let de = state.last_de.lock().map(|d| d.clone()).unwrap_or_default();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let (ok, msg) = commit_pending(pending, &de);
            // What was removed disappears from disk, and what was not removed (enabled
            // while the line was hanging) must come back into the grid: the scan cache
            // starts anew.
            if let Ok(mut cache) = state.scan_cache.lock() {
                cache.clear();
            }
            finish_action(&state, &handle, msg.clone(), ok);
            post(&handle, move |mut object| object.as_mut().removal_done(QString::from(&msg)));
        });
    }

    fn undo_removals(&self) -> i32 {
        let n = self.rust().state.removals.lock().map(|mut q| q.undo_all()).unwrap_or(0);
        i32::try_from(n).unwrap_or(i32::MAX)
    }

    fn probe_login_screen(mut self: Pin<&mut Self>) {
        let handle = self.as_mut().qt_thread();
        let state = self.rust().state.clone();
        std::thread::spawn(move || probe_login_screen_into(&state, &handle));
    }

    fn login_screen(&self) -> QString {
        let last = self
            .rust()
            .state
            .login_screen
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default();
        QString::from(&last)
    }

    fn make_default_login_manager(mut self: Pin<&mut Self>, unit: &QString, greeter: &QString) {
        use std::sync::atomic::Ordering;
        let handle = self.as_mut().qt_thread();
        let state = self.rust().state.clone();
        let (unit, greeter) = (unit.to_string(), greeter.to_string());
        if state.login_switching.swap(true, Ordering::SeqCst) {
            let msg = rexon_core::system_plan::login_manager_switch_failed(&rexon_core::i18n::tr(
                "the previous change isn't finished yet",
            ));
            post(&handle, move |mut object| {
                object.as_mut().login_manager_switched(false, QString::from(&msg));
            });
            return;
        }
        std::thread::spawn(move || {
            let greeter = (!greeter.trim().is_empty()).then_some(greeter.trim());
            let (ok, msg) = rexon_core::system_exec::live().switch_login_manager(&unit, greeter);
            let first = msg.lines().next().unwrap_or("");
            if ok {
                rexon_core::log_info!("login", "экран входа {unit}: сделан основным — {first}");
            } else {
                rexon_core::log_warn!("login", "экран входа {unit}: не вышло — {first}");
            }
            state.login_switching.store(false, Ordering::SeqCst);
            post(&handle, move |mut object| {
                object.as_mut().login_manager_switched(ok, QString::from(&msg));
            });
            // The section is re-read: the default one changed (or not — then the probe
            // will show the previous one, and that is the truth too).
            probe_login_screen_into(&state, &handle);
        });
    }

    fn make_wayland_default_session(mut self: Pin<&mut Self>, x11: &QString) {
        use std::sync::atomic::Ordering;
        let handle = self.as_mut().qt_thread();
        let state = self.rust().state.clone();
        let x11 = x11.to_string();
        let Some(session) = system_plan::X11_SESSION_SUCCESSORS.iter().find(|s| s.x11 == x11.trim()) else {
            return;
        };
        if state.login_switching.swap(true, Ordering::SeqCst) {
            let msg = system_plan::login_manager_switch_failed(&rexon_core::i18n::tr(
                "the previous change isn't finished yet",
            ));
            post(&handle, move |mut object| {
                object.as_mut().login_manager_switched(false, QString::from(&msg));
            });
            return;
        }
        std::thread::spawn(move || {
            let (ok, msg) = match system_plan::make_wayland_default_live(session) {
                Ok(text) => (true, text),
                Err(why) => (false, why),
            };
            if ok {
                rexon_core::log_info!("login", "сеанс по умолчанию: {} → {}", session.x11, session.wayland);
            } else {
                rexon_core::log_warn!("login", "сеанс по умолчанию не сменён ({}): {msg}", session.x11);
            }
            state.login_switching.store(false, Ordering::SeqCst);
            post(&handle, move |mut object| {
                object.as_mut().login_manager_switched(ok, QString::from(&msg));
            });
            probe_login_screen_into(&state, &handle);
        });
    }

    fn set_login_background(mut self: Pin<&mut Self>, path: &QString) {
        let path = path.to_string();
        let path = path.strip_prefix("file://").map(str::to_string).unwrap_or(path);
        self.as_mut().run_login_look(move || {
            let image = (!path.trim().is_empty()).then_some(path.trim());
            // Plasma Login Manager: its own helper (KAuth) writes, the password window
            // is polkit.
            if system_plan::is_plasma_login(&ainst::active_login_manager()) {
                return rexon_core::plasma_login::background_live(image);
            }
            let Some(theme) = system_plan::sddm_current_theme_live() else {
                return Err(rexon_core::i18n::tr("Couldn't change the login screen background."));
            };
            system_plan::sddm_background_live(&theme, image)
        });
    }

    fn apply_login_look(mut self: Pin<&mut Self>, reset: bool) {
        let state = self.rust().state.clone();
        self.as_mut().run_login_look(move || {
            if system_plan::is_plasma_login(&ainst::active_login_manager()) {
                let de = (!reset).then(|| greeter_desktop(&state));
                return rexon_core::plasma_login::look_live(de.as_deref());
            }
            if reset {
                return system_plan::sddm_look_live(None);
            }
            let de = greeter_desktop(&state);
            let look = system_plan::desktop_greeter_look_live(&de);
            system_plan::sddm_look_live(Some(&look))
        });
    }

    fn set_login_clock(mut self: Pin<&mut Self>, show: bool) {
        self.as_mut().run_login_look(move || {
            if system_plan::is_plasma_login(&ainst::active_login_manager()) {
                return rexon_core::plasma_login::clock_live(show);
            }
            let Some(theme) = system_plan::sddm_current_theme_live() else {
                return Err(rexon_core::i18n::tr("Couldn't change the login screen clock."));
            };
            system_plan::sddm_clock_live(&theme, show)
        });
    }

    fn preview_login_screen(mut self: Pin<&mut Self>, token: &QString) -> QString {
        use std::sync::atomic::Ordering;
        // One preview at a time; a click while it is open closes it (the block's
        // button at that time is "Close preview").
        if LOGIN_PREVIEW_RUNNING.load(Ordering::SeqCst) {
            LOGIN_PREVIEW_STOP.store(true, Ordering::SeqCst);
            return QString::default();
        }
        let argv = match system_plan::login_preview_argv_live(&token.to_string()) {
            Ok(a) => a,
            Err(e) => return QString::from(&e),
        };
        if LOGIN_PREVIEW_RUNNING.swap(true, Ordering::SeqCst) {
            return QString::default();
        }
        LOGIN_PREVIEW_STOP.store(false, Ordering::SeqCst);
        rexon_core::log_info!("login", "превью экрана входа: {}", argv.join(" "));
        self.as_mut().login_preview_changed(true, QString::default());
        let handle = self.as_mut().qt_thread();
        // The thread lives until the end of the show: with it (PR_SET_PDEATHSIG)
        // the greeter also goes away if Rexon is closed in the middle of the
        // preview.
        std::thread::spawn(move || {
            let max = std::time::Duration::from_secs(system_plan::LOGIN_PREVIEW_MAX_SECS);
            let out = system_plan::run_greeter_preview(&argv, &LOGIN_PREVIEW_STOP, max);
            LOGIN_PREVIEW_RUNNING.store(false, Ordering::SeqCst);
            let msg = match out {
                Ok(()) => String::new(),
                Err(text) => {
                    rexon_core::log_warn!("login", "превью экрана входа не удалось: {text}");
                    text
                }
            };
            post(&handle, move |mut o| {
                if !msg.is_empty() {
                    o.as_mut().status_message(QString::from(&msg));
                }
                o.as_mut().login_preview_changed(false, QString::from(&msg));
            });
        });
        QString::default()
    }

    fn close_login_preview(&self) {
        LOGIN_PREVIEW_STOP.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn tile_demo_label(&self, token: &QString) -> QString {
        QString::from(&if token.to_string().starts_with(ap::LOGIN_PREVIEW_PREFIX) {
            system_plan::greeter_preview_view(false, None).label
        } else {
            ainst::splash_demo_label()
        })
    }

    fn tile_demo_tooltip(&self, token: &QString) -> QString {
        QString::from(&if token.to_string().starts_with(ap::LOGIN_PREVIEW_PREFIX) {
            system_plan::greeter_preview_view(false, None).tooltip
        } else {
            ainst::splash_demo_tooltip()
        })
    }

    fn open_login_screen_settings(&self) -> bool {
        // The first command that started. The process lives its own life; we wait
        // for it in a separate thread only so as not to leave a zombie.
        for argv in login_settings_commands() {
            let Some((bin, args)) = argv.split_first() else {
                continue;
            };
            if let Ok(mut child) = std::process::Command::new(bin)
                .args(args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return true;
            }
        }
        false
    }

    fn shell_palette(&self) -> QString {
        let banner = palette::live_banner();
        QString::from(&serde_json::to_string(&banner).unwrap_or_default())
    }

    fn set_palette_lead(&self, lead: &QString) -> bool {
        match palette::Lead::parse(&lead.to_string()) {
            Some(l) => palette::set_lead(&palette::PaletteHome::live(), l).is_ok(),
            None => false,
        }
    }

    fn revert_applied(&self, section: &QString) -> QString {
        let rexon = utils::rexon_config_dir();
        let (ok, text) = ap::revert_applied_section(&rexon, &palette::PaletteHome::live(), &section.to_string());
        if let Ok(mut cache) = self.rust().state.scan_cache.lock() {
            cache.clear();
        }
        QString::from(&json!({ "ok": ok, "text": text }).to_string())
    }

    fn push_shell_palette(mut self: Pin<&mut Self>) {
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let (ok, msg) = palette::push_flow(palette::live_detect(), &palette::PaletteHome::live());
            post_palette(&handle, msg, ok);
        });
    }

    fn pull_shell_palette(mut self: Pin<&mut Self>) {
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let logger = log_sink(&handle);
            let mut installer = LiveInstaller::new("", logger);
            let mut apply = |scheme: &str| apply_color_scheme(&mut installer, scheme);
            let (ok, msg) = palette::pull_flow(
                palette::live_detect(),
                &palette::PaletteHome::live(),
                &mut palette::nudge_noctalia_live,
                3000,
                &mut apply,
            );
            if ok {
                // The active scheme changed — so did the "Active" badge in the section.
                if let Ok(mut cache) = state.scan_cache.lock() {
                    cache.clear();
                }
            }
            post_palette(&handle, msg, ok);
        });
    }

    fn revert_shell_palette(mut self: Pin<&mut Self>) {
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let logger = log_sink(&handle);
            let mut installer = LiveInstaller::new("", logger);
            let mut apply = |scheme: &str| apply_color_scheme(&mut installer, scheme);
            let (ok, msg) = palette::revert_flow(&palette::PaletteHome::live(), &mut apply);
            if let Ok(mut cache) = state.scan_cache.lock() {
                cache.clear();
            }
            post_palette(&handle, msg, ok);
        });
    }

    fn purge_translations(mut self: Pin<&mut Self>) {
        // Cleaning up stale entries of the shared translation cache. The Rust tab
        // now translates everything itself — both the descriptions
        // (`translate_detail_description`) and the comments
        // (`translate_comment_page`) — and its database is ONE with the widget
        // tab: entries translated by that one do not go anywhere, and the cleaner
        // here is also one for both.
        std::thread::spawn(move || {
            let mut cache = rexon_core::appearance_cache::LiveCache::open(&home());
            cache.purge_stale_translations();
        });
        // `self` is not needed here, but the method signature must be the same as
        // for the other long actions: the page calls it with a timer.
        let _ = self.as_mut();
    }
}

/// What to open the login screen settings with on this machine is the
/// core's decision (`appearance_page::login_settings_commands`), here there
/// is only finding the programs.
fn login_settings_commands() -> Vec<Vec<String>> {
    ap::login_settings_commands(&|bin| utils::which(bin, None).is_some())
}

// ============================================================================
// Section contents
// ============================================================================

/// One catalogue source by the kind of request.
fn fetch_kind(state: &Arc<State>, req: &Value, token: &CancelToken, kind: &str) -> String {
    match kind {
        "local" => scan_local(state, req),
        "aur" => fetch_aur(state, req, token),
        "spices" => fetch_spices(state, req),
        "wallhaven" => fetch_wallhaven(state, req, token),
        "ego" => fetch_ego(state, req, token),
        _ => fetch_ocs(state, req, token),
    }
}

/// A section with installed items: on top the installed ones (only on the
/// first store page), under them the section's store page — in one answer.
/// If the store did not answer (no network), its error goes to the status
/// line, and the installed ones are still visible.
fn fetch_combined(state: &Arc<State>, req: &Value, token: &CancelToken) -> String {
    let store: Value = serde_json::from_str(&fetch_kind(state, req, token, s(req, "storeKind")))
        .unwrap_or_else(|_| json!({}));
    let mut answer = match store {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    answer.insert("key".to_string(), Value::from(s(req, "key")));
    if num(req, "page") == 0 {
        let local: Value = serde_json::from_str(&scan_local(state, req)).unwrap_or_else(|_| json!({}));
        let mut items: Vec<Value> = local.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        if cs::type_for_section(s(req, "categoryKey")).is_some() {
            mark_spice_danger(&mut items);
        }
        if s(req, "categoryKey") == gext::SECTION_KEY {
            stamp_gnome_installed(state, req, &mut items, &mut answer);
            // The showcase snapshot — with the same installed items (the showcase page
            // already came above, the picture is only from the preview cache, without
            // the network).
            let home = home();
            gext::stamp_ego_pictures(&mut items, &|uuid| {
                let url = ego::live_pictures().lock().ok()?.get(uuid)?.to_string();
                let path = ap::preview_cache_path(&home, &url);
                std::path::Path::new(&path).is_file().then_some(path)
            });
        }
        if let Some(Value::Array(rest)) = answer.get("items") {
            items.extend(rest.iter().cloned());
        }
        answer.insert("items".to_string(), Value::Array(items));
    }
    Value::Object(answer).to_string()
}

/// A store page (plus the AUR tail in "All" mode) — a port of `_fetch_ocs`.
fn fetch_ocs(state: &Arc<State>, req: &Value, token: &CancelToken) -> String {
    let mut answer = Map::new();
    answer.insert("key".to_string(), Value::from(s(req, "key")));

    let net = UreqNet::new().with_cancel(token.clone());
    let io = LiveOcsIo::with_net(net, &home());
    let preview_dir = io.preview_dir();
    let mut client = OcsClient::new(io, &preview_dir);
    let (page, failure) = store_page_checked(&mut client, req);
    let ap::StreamPage { items: found, total, has_more } = page;
    let mut items: Vec<ThemeItem> = found.iter().map(|c| c.item.clone()).collect();
    // The store always answers "not installed": "✓ Installed"/"✓ Applied" are
    // restored by the installation journal (the core's decision).
    ap::stamp_installed_live(&home(), &mut items);
    remember_items(state, &items);
    let rows: Vec<Value> = items
        .iter()
        .map(|it| serde_json::to_value(it).unwrap_or(Value::Null))
        .collect();

    let first_page = num(req, "page") == 0;
    // The installed items are no longer glued here: for the splash, full lock
    // themes and KDE sections they open with their own grid, and the store
    // with the "Get new…" button (`CatalogView::installed_view`), and the
    // request for it goes with the ordinary section scan (`scan_local`).
    // The store did not give the page (a refusal, a connection failure) — that
    // is not "nothing found": the status line says the store does not respond.
    // The section shows the installed items as before (`fetch_combined`).
    if rows.is_empty() {
        if let Some(f) = &failure {
            answer.insert("error".to_string(), Value::from(ap::store_failure_text(f)));
        }
    }
    answer.insert("items".to_string(), Value::Array(rows));
    answer.insert("total".to_string(), Value::from(total));
    answer.insert("hasMore".to_string(), Value::from(has_more));

    // "All": the AUR cards are appended BELOW the store ones and only on the
    // first page — further on only the store is paged.
    if s(req, "kind") == "both" && first_page {
        match aur_packages(state, req) {
            Ok(pkgs) => {
                let top: Vec<AurPackage> =
                    pkgs.into_iter().take(page_size(req) as usize).collect();
                remember_packages(state, &top);
                answer.insert(
                    "packages".to_string(),
                    serde_json::to_value(&top).unwrap_or(Value::Array(Vec::new())),
                );
            }
            // AUR does not break the store: the showcase is already assembled, and an
            // RPC refusal is no reason to show the user an empty section.
            Err(e) => rexon_core::log_warn!("aur", "режим «Все»: {e}"),
        }
    }
    Value::Object(answer).to_string()
}

/// A store page topped up to full (`ap::stream_page`): the KDE Store hides
/// some works, and a server page arrives shorter than requested. The server
/// pages are taken through the `ocs_cache` cache, so when paging in a row
/// only the new ones go to the network.
fn store_page<IO: rexon_core::ocs::OcsIo>(
    client: &mut OcsClient<IO>,
    req: &Value,
) -> ap::StreamPage<rexon_core::ocs::OcsContent> {
    store_page_checked(client, req).0
}

/// [`store_page`] and the first reason why the store did not give the page.
fn store_page_checked<IO: rexon_core::ocs::OcsIo>(
    client: &mut OcsClient<IO>,
    req: &Value,
) -> (ap::StreamPage<rexon_core::ocs::OcsContent>, Option<rexon_core::ocs::StoreFailure>) {
    let size = page_size(req).clamp(1, i64::from(u32::MAX)) as u32;
    let mut failure = None;
    // A server page is always `PAGE_SIZE`: the store cache key does not know
    // the size, and the Rexon page (whole grid rows) is cut from the stream.
    let page = ap::stream_page(num(req, "page"), size, ap::PAGE_SIZE, |k| {
        let (mut chunk, total) = match client.search_checked(
            s(req, "categoryId"),
            k,
            i64::from(ap::PAGE_SIZE),
            s(req, "sort"),
            s(req, "query"),
            true,
        ) {
            Ok(found) => found,
            Err(e) => {
                failure.get_or_insert(e);
                (Vec::new(), 0)
            }
        };
        // Programs in the Conky widgets section (Conky Manager) are not widgets;
        // the page is topped up from the next server page.
        chunk.retain(|c| !de_apply::is_conky_program(&c.item));
        (chunk, total)
    });
    (page, failure)
}

/// A section from AUR: a selection by section or a search over all of AUR —
/// a port of `_fetch_aur`.
fn fetch_aur(state: &Arc<State>, req: &Value, _token: &CancelToken) -> String {
    let mut answer = Map::new();
    answer.insert("key".to_string(), Value::from(s(req, "key")));
    answer.insert("items".to_string(), Value::Array(Vec::new()));
    answer.insert("total".to_string(), Value::from(0));

    let query = s(req, "query");
    if ap::aur_query_too_short(query) {
        answer.insert(
            "error".to_string(),
            Value::from(ap::aur_query_too_short_msg()),
        );
        return Value::Object(answer).to_string();
    }
    let pkgs = match aur_packages(state, req) {
        Ok(p) => p,
        Err(e) => {
            answer.insert(
                "error".to_string(),
                Value::from(ui_base::humanize_network_error(&e, "AUR").as_str()),
            );
            return Value::Object(answer).to_string();
        }
    };
    // The RPC has no pages — it gives the whole list; we cut it ourselves.
    let size = page_size(req).max(1) as usize;
    let page = num(req, "page").max(0) as usize;
    let chunk: Vec<AurPackage> = pkgs.iter().skip(page * size).take(size).cloned().collect();
    remember_packages(state, &chunk);
    answer.insert(
        "packages".to_string(),
        serde_json::to_value(&chunk).unwrap_or(Value::Array(Vec::new())),
    );
    answer.insert("total".to_string(), Value::from(pkgs.len() as i64));
    Value::Object(answer).to_string()
}

/// The section's Cinnamon Spices store: the site index from the cache (the
/// network — once a day with a conditional request or on "Refresh"), the
/// page and the search go by it. Whether it is installed and whether there
/// is an update goes by the disk (`last-edited` in `metadata.json`): this
/// way what was installed by the Cinnamon settings window is visible too.
fn fetch_spices(state: &Arc<State>, req: &Value) -> String {
    let mut answer = Map::new();
    answer.insert("key".to_string(), Value::from(s(req, "key")));
    let section = s(req, "categoryKey");
    // Cinnamon sections are their own spice type; "GTK themes" and window
    // borders are themes selected by contents
    // (`cinnamon_spices::PART_SECTIONS`).
    let part = cs::is_part_section(section).then_some(section);
    let Some(t) = cs::type_for_section(section).or_else(|| part.and_then(|_| cs::type_for_key("theme"))) else {
        answer.insert("items".to_string(), Value::Array(Vec::new()));
        answer.insert("total".to_string(), Value::from(0));
        return Value::Object(answer).to_string();
    };
    let force = req.get("force").and_then(Value::as_bool).unwrap_or(false);
    let mut refresh = cs::live_refresh(t, force);
    let parts = part.map(|_| cs::live_refresh_parts(force));
    if let (Some(sec), Some(p)) = (part, parts.as_ref()) {
        refresh.spices = cs::themes_for_section(&refresh.spices, &p.parts, sec);
        if !p.note.is_empty() {
            refresh.note = if refresh.note.is_empty() { p.note.clone() } else { format!("{} {}", refresh.note, p.note) };
        }
    }
    let lang = rexon_core::i18n::current_language();
    let size = page_size(req).max(1) as usize;
    let page = num(req, "page").max(0) as usize;
    let (chunk, total) = cs::filter_sort_page(&refresh.spices, s(req, "query"), s(req, "sort"), page, size, &lang);
    let env = cs::SpiceEnv::live();
    let mut items: Vec<ThemeItem> = Vec::with_capacity(chunk.len());
    let mut rows: Vec<Value> = Vec::with_capacity(chunk.len());
    for sp in &chunk {
        let mut it = match (part, parts.as_ref()) {
            (Some(sec), Some(p)) => {
                let paths = p.parts.get(&sp.key).map(Vec::as_slice).unwrap_or(&[]);
                cs::section_theme_item(sp, &lang, sec, s(req, "de"), paths)
            }
            _ => cs::theme_item(t, sp, &lang),
        };
        let mark = cs::installed_state(&env, t, &sp.key);
        it.installed = mark.is_some();
        let danger = it.installed && cs::code_dir(&env, t, &sp.key).is_some_and(|d| cs::scan_for_danger(std::path::Path::new(&d)));
        let mut v = serde_json::to_value(&it).unwrap_or(Value::Null);
        if let Some(m) = v.as_object_mut() {
            m.insert("spicesScore".to_string(), Value::from(sp.score));
            m.insert("spicesEdited".to_string(), Value::from(sp.last_edited));
            m.insert("spicesUpdate".to_string(), Value::from(mark.is_some_and(|m| cs::has_update(m, sp.last_edited))));
            m.insert("spicesPage".to_string(), Value::from(cs::page_url(t, &sp.spices_id)));
            m.insert("spicesDanger".to_string(), Value::from(danger));
            // In the GTK or borders section the card itself enables an installed theme
            // — "Apply".
            m.insert("spicesApply".to_string(), Value::from(part.is_some()));
        }
        rows.push(v);
        items.push(it);
    }
    remember_items(state, &items);
    if rows.is_empty() && refresh.spices.is_empty() && !refresh.note.is_empty() {
        answer.insert("error".to_string(), Value::from(refresh.note.clone()));
    } else if !refresh.note.is_empty() {
        answer.insert("note".to_string(), Value::from(refresh.note.clone()));
    }
    answer.insert("items".to_string(), Value::Array(rows));
    answer.insert("total".to_string(), Value::from(total as i64));
    Value::Object(answer).to_string()
}

/// The Wallhaven store of the "HD Wallpapers" section: a Rexon page from API
/// pages (`wallhaven::live_page` — its own counter of 45 requests per minute
/// and the session's answer memory; "Refresh" bypasses the memory). The
/// cards are ordinary items of the wallpaper section: "installed" goes by
/// the shared installed journal.
fn fetch_wallhaven(state: &Arc<State>, req: &Value, token: &CancelToken) -> String {
    let mut answer = Map::new();
    answer.insert("key".to_string(), Value::from(s(req, "key")));
    let force = req.get("force").and_then(Value::as_bool).unwrap_or(false);
    let size = page_size(req).max(1);
    let page = match wh::live_page(s(req, "query"), s(req, "sort"), num(req, "page"), size, force, token) {
        Ok(p) => p,
        Err(e) => {
            rexon_core::log_warn!("wallhaven", "страница каталога: {e:?}");
            answer.insert("items".to_string(), Value::Array(Vec::new()));
            answer.insert("total".to_string(), Value::from(0));
            answer.insert("error".to_string(), Value::from(e.message()));
            return Value::Object(answer).to_string();
        }
    };
    let mut items: Vec<ThemeItem> = page.items.iter().map(wh::theme_item).collect();
    ap::stamp_installed_live(&home(), &mut items);
    remember_items(state, &items);
    let rows: Vec<Value> = items.iter().zip(&page.items).map(|(it, w)| wh::catalog_row(it, w)).collect();
    answer.insert("items".to_string(), Value::Array(rows));
    answer.insert("total".to_string(), Value::from(page.total));
    answer.insert("hasMore".to_string(), Value::from(page.has_more));
    Value::Object(answer).to_string()
}

/// The GNOME Shell version of this machine in the extensions site form
/// (`51`), once per process: for a running shell — from the bus, otherwise
/// `gnome-shell --version` (`gnome_shell_ext::detect_shell_version`).
/// `None` means there is no GNOME here, and the showcase shows everything
/// without checking.
///
/// The memory is not forever: a version taken not from the bus (Rexon
/// started before logging into GNOME or outside it) is asked again as soon
/// as the shell appears on the bus — otherwise the showcase would stay
/// without filtering by version.
fn ego_shell_version() -> Option<ego::ShellVersion> {
    /// (the version, whether it was taken from the running shell).
    static VERSION: Mutex<Option<(Option<ego::ShellVersion>, bool)>> = Mutex::new(None);
    let mut io = rexon_core::gnome_shell_ext::LiveShell::new();
    let mut memo = VERSION.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((v, from_bus)) = memo.as_ref() {
        if *from_bus || !rexon_core::gnome_shell_ext::ShellExtIo::shell_running(&mut io) {
            return v.clone();
        }
    }
    let from_bus = rexon_core::gnome_shell_ext::ShellExtIo::shell_running(&mut io);
    let v = rexon_core::gnome_shell_ext::detect_shell_version(&mut io)
        .as_deref()
        .and_then(ego::parse_shell_version);
    *memo = Some((v.clone(), from_bus));
    v
}

/// Remove what is marked for removal: GNOME extensions — by the shell
/// (without it — our own directory; the installed journal entry goes away),
/// themes — by the shared folder removal (`theme_removal::live_commit`). The
/// outcome is one line per part.
fn commit_pending(pending: Vec<removal::PendingRemoval>, de: &str) -> (bool, String) {
    let (gnome, rest) = gext::split_pending(pending);
    let mut ok = true;
    let mut lines: Vec<String> = Vec::new();
    if !rest.is_empty() {
        let (done, msg) = removal::live_commit(rest, de);
        ok &= done;
        if !msg.is_empty() {
            lines.push(msg);
        }
    }
    if !gnome.is_empty() {
        let data_home = rexon_core::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), &home());
        let mut io = rexon_core::gnome_shell_ext::LiveShell::new();
        for (uuid, p) in gnome {
            match gext::uninstall_own(&mut io, &uuid, &data_home) {
                Ok(()) => {
                    if let Err(e) = gext::live_journal_edit(&home(), |log| {
                        gext::journal_forget(log, &uuid);
                    }) {
                        rexon_core::log_warn!("gnome-ext", "журнал установленного: {e}");
                    }
                    lines.push(rexon_core::i18n::trf("🗑️ Removed: %1", &[&p.name]));
                }
                Err(e) => {
                    ok = false;
                    lines.push(e.message());
                }
            }
        }
    }
    (ok, lines.join("\n"))
}

/// Once per session, on the first opening of the "GNOME extensions"
/// section: ask the shell to check for updates (`CheckForUpdates()` — it
/// downloads them itself) and broadcast the number downloaded
/// (`gnomeUpdatesChanged`).
fn start_gnome_update_check(state: &Arc<State>, handle: &Handle) {
    use std::sync::atomic::Ordering;
    let mut probe = rexon_core::gnome_shell_ext::LiveShell::new();
    if !rexon_core::gnome_shell_ext::ShellExtIo::shell_running(&mut probe) {
        return;
    }
    if state.gnome_updates_checked.swap(true, Ordering::SeqCst) {
        return;
    }
    let state = state.clone();
    let handle = handle.clone();
    std::thread::spawn(move || {
        let mut io = rexon_core::gnome_shell_ext::LiveShell::new();
        let mut wait = || {
            std::thread::sleep(std::time::Duration::from_secs(gext::UPDATE_RECOUNT_SECS));
            true
        };
        let mut report = |n: usize| {
            if let Ok(mut c) = state.gnome_updates.lock() {
                *c = n;
            }
            let payload = gext::updates_answer(n).to_string();
            post(&handle, move |mut object| object.as_mut().gnome_updates_changed(QString::from(&payload)));
        };
        if let Err(e) = gext::check_updates(&mut io, gext::UPDATE_RECOUNTS, &mut wait, &mut report) {
            rexon_core::log_warn!("gnome-ext", "проверка обновлений: {e:?}");
        }
    });
}

/// The extensions.gnome.org store of the "GNOME extensions" section —
/// modelled on [`fetch_wallhaven`]: a Rexon page is one API page
/// (`gnome_ego::live_page`, the session's answer memory and a back-off after
/// 5xx; "Refresh" bypasses the memory). Under the "Only for your GNOME
/// version" switch (on by default) the site itself selects compatible ones;
/// when it is off — a listing without filtering, and incompatible ones are
/// marked by a second request (checking uuid batches). If the check failed —
/// tiles without marks: the showcase matters more.
fn fetch_ego(state: &Arc<State>, req: &Value, token: &CancelToken) -> String {
    let mut answer = Map::new();
    answer.insert("key".to_string(), Value::from(s(req, "key")));
    let force = req.get("force").and_then(Value::as_bool).unwrap_or(false);
    let only_compatible = req.get("egoCompatibleOnly").and_then(Value::as_bool).unwrap_or(true);
    let shell = ego_shell_version();
    let shell = shell.as_ref();
    let site_shell = shell.map(ego::site_shell_version);
    let filter = if only_compatible { site_shell.as_deref() } else { None };
    let size = page_size(req).max(1);
    let page = match ego::live_page(s(req, "query"), s(req, "sort"), num(req, "page"), size, filter, force, token) {
        Ok(p) => p,
        Err(e) => {
            rexon_core::log_warn!("gnome-ego", "страница каталога: {e:?}");
            answer.insert("items".to_string(), Value::Array(Vec::new()));
            answer.insert("total".to_string(), Value::from(0));
            answer.insert("error".to_string(), Value::from(e.message()));
            return Value::Object(answer).to_string();
        }
    };
    let unsupported: std::collections::HashSet<String> = match (shell, site_shell.as_deref()) {
        (Some(_), Some(site)) if !only_compatible && !page.items.is_empty() => {
            let shown: Vec<&str> = page.items.iter().map(|e| e.uuid.as_str()).collect();
            match ego::live_compatible(&shown, site, token) {
                Ok(ok) => ego::unsupported_uuids(&shown, &ok),
                Err(e) => {
                    rexon_core::log_warn!("gnome-ego", "сверка совместимости: {e:?}");
                    Default::default()
                }
            }
        }
        _ => Default::default(),
    };
    let shell_text = shell.map(ego::shell_label).unwrap_or_default();
    let mut items: Vec<ThemeItem> = page.items.iter().map(ego::theme_item).collect();
    ap::stamp_installed_live(&home(), &mut items);
    remember_items(state, &items);
    let rows: Vec<Value> = items
        .iter()
        .zip(&page.items)
        .map(|(it, e)| ego::catalog_row(it, e, unsupported.contains(&e.uuid).then_some(shell_text.as_str())))
        .collect();
    // What is already installed — according to the shell (including what was
    // installed bypassing Rexon).
    let mut rows = rows;
    if let Some(list) = gext::live_snapshot() {
        gext::stamp_rows(&mut rows, &list, &shell_text);
    }
    answer.insert("items".to_string(), Value::Array(rows));
    answer.insert("total".to_string(), Value::from(page.total));
    answer.insert("hasMore".to_string(), Value::from(page.has_more));
    Value::Object(answer).to_string()
}

/// The installed items of the "GNOME extensions" section in a GNOME session
/// — according to the shell: to the tiles from disk is added what only it
/// knows, each with marks ("Error", "Does not support GNOME N", "Update —
/// after re-login") and what can be done. Without the shell the disk
/// overview remains, as before. Along the way — the number of downloaded
/// updates (`gnomeUpdates` in the answer).
fn stamp_gnome_installed(state: &Arc<State>, req: &Value, items: &mut Vec<Value>, answer: &mut Map<String, Value>) {
    let Some(list) = gext::live_snapshot() else { return };
    let known: std::collections::HashSet<String> =
        items.iter().filter_map(|v| v.get("id").and_then(Value::as_str)).map(str::to_string).collect();
    let extra: Vec<ThemeItem> = gext::merge_installed(Vec::new(), &list)
        .into_iter()
        .filter(|it| !known.contains(&it.id))
        .filter(|it| {
            let q = s(req, "query").trim().to_lowercase();
            q.is_empty() || it.name.to_lowercase().contains(&q) || it.id.to_lowercase().contains(&q)
        })
        .collect();
    // What is marked for removal is not shown while "Undo" is hanging.
    let mut extra = extra;
    if let Ok(queue) = state.removals.lock() {
        queue.retain_visible(&mut extra);
    }
    if !extra.is_empty() {
        remember_items(state, &extra);
        // Before the showcase cards: the installed items go on top.
        let local = items.iter().take_while(|v| {
            v.get("id").and_then(Value::as_str).is_some_and(|id| id.starts_with("local:"))
        });
        let at = local.count();
        let rows = local_rows(&extra, s(req, "which"));
        items.splice(at..at, rows);
    }
    let shell = ego_shell_version().map(|v| ego::shell_label(&v)).unwrap_or_default();
    gext::stamp_rows(items, &list, &shell);
    let pending = rexon_core::gnome_shell_ext::pending_updates(&list);
    if let Ok(mut c) = state.gnome_updates.lock() {
        *c = pending;
    }
    answer.insert("gnomeUpdates".to_string(), gext::updates_answer(pending));
}

/// Installed tiles of a Cinnamon section get the `spicesDanger` mark if the
/// spice's code calls blocking functions (`cinnamon_spices::scan_for_danger`).
fn mark_spice_danger(items: &mut [Value]) {
    for v in items.iter_mut() {
        let path = v.get("install_path").and_then(Value::as_str).unwrap_or("").to_string();
        if path.is_empty() || !std::path::Path::new(&path).is_dir() {
            continue;
        }
        if cs::scan_for_danger(std::path::Path::new(&path)) {
            if let Some(m) = v.as_object_mut() {
                m.insert("spicesDanger".to_string(), Value::from(true));
            }
        }
    }
}

/// The list of AUR packages for the query, cached for the tab session.
fn aur_packages(state: &Arc<State>, req: &Value) -> Result<Vec<AurPackage>, String> {
    let key = s(req, "aurKey").to_string();
    if let Some(cached) = state.aur_cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Ok(cached);
    }
    let mut net = UreqNet::new();
    let query = s(req, "query");
    // Without a query — the whole section, with its curated packages on top
    // (`aur::CURATED`): AeroThemePlasma in "Global themes", its icons in
    // "Icons", its sounds in "Sounds".
    let mut pkgs = if query.is_empty() {
        aur::fetch_category_with_curated(&mut net, s(req, "categoryKey"))
    } else {
        // The search — only what fits the section: for "girl" in "HD Wallpapers"
        // fcitx dictionaries and fonts used to come otherwise
        // (`aur::search_in_category`).
        aur::search_in_category(&mut net, query, s(req, "categoryKey"))
    }
    .map_err(|e| e.to_string())?;
    let installed = installed_packages();
    for p in &mut pkgs {
        p.installed = installed.contains(&p.name);
    }
    if let Ok(mut cache) = state.aur_cache.lock() {
        cache.insert(key, pkgs.clone());
    }
    Ok(pkgs)
}

/// The disk overview: active, downloaded, system ones, favourites, styles —
/// a port of `_scan_local`.
fn scan_local(state: &Arc<State>, req: &Value) -> String {
    let which = s(req, "which").to_string();
    // The desktop the tab looks at: the environment sections in the overviews
    // and which tiles are applicable here at all depend on it.
    let de = s(req, "de").to_string();
    if let Ok(mut last) = state.last_de.lock() {
        *last = de.clone();
    }
    let cache_key = format!("{which}@{de}");
    let mut answer = Map::new();
    answer.insert("key".to_string(), Value::from(s(req, "key")));

    let mut items: Vec<ThemeItem> = if which == "favorites" {
        // Favourites are store items from the cache: their installation state is
        // the same as in the store grid and is taken from the same journal.
        let mut fav = favorite_items();
        ap::stamp_installed_live(&home(), &mut fav);
        fav
    } else {
        if req.get("fresh").and_then(Value::as_bool).unwrap_or(false) {
            if let Ok(mut cache) = state.scan_cache.lock() {
                cache.remove(&cache_key);
            }
        }
        // A theme was put or removed bypassing Rexon (in `~/.themes`, `~/.icons`…)
        // with the window open: the previous scans are good for no section.
        let stamp = scan::roots_stamp(&home(), "/usr");
        if let (Ok(mut last), Ok(mut cache)) = (state.scan_stamp.lock(), state.scan_cache.lock()) {
            if *last != stamp {
                cache.clear();
                *last = stamp;
            }
        }
        let cached = state
            .scan_cache
            .lock()
            .ok()
            .and_then(|c| c.get(&cache_key).cloned());
        match cached {
            Some(list) => list,
            None => match scan_disk(state, &which, &de) {
                Some(list) => {
                    let mut list = ap::local_items_for_de(list, std::path::Path::new(&home()), &de);
                    // Small subdirectory thumbnails (120×35) do not cover the whole tile: the
                    // window will assemble a frame for such a tile.
                    rexon_core::local_preview::drop_small_thumbnails(&mut list);
                    // A GTK theme without its own icons and cursor — say it on the tile.
                    let deps = LiveInstaller::new("", Box::new(|_: &str| {}));
                    scan::stamp_missing_deps(&mut list, &|name| deps.theme_deps_plan(name));
                    if let Ok(mut cache) = state.scan_cache.lock() {
                        cache.insert(cache_key.clone(), list.clone());
                    }
                    list
                }
                None => {
                    // System themes are not classified reliably (`pacman -Qm` failed): they
                    // cannot be shown — AUR themes would leak through. We do not write the
                    // cache, and on the next visit we try again.
                    answer.insert("items".to_string(), Value::Array(Vec::new()));
                    answer.insert("total".to_string(), Value::from(0));
                    answer.insert(
                        "error".to_string(),
                        Value::from(ap::status_system_scan_unavailable()),
                    );
                    return Value::Object(answer).to_string();
                }
            },
        }
    };

    // The search over the disk overview — on the spot, without the network.
    // The yardstick "does the theme stay in view" is NOT computed here on our
    // own: it is one for all places where the page shows themes from disk, and
    // lives in the core.
    let query = s(req, "query");
    if !query.trim().is_empty() {
        items = ap::local_filter(&items, query);
    }
    // Those marked for removal are not shown while the "Undo" line is hanging.
    if let Ok(queue) = state.removals.lock() {
        queue.retain_visible(&mut items);
    }
    remember_items(state, &items);
    let rows: Vec<Value> = local_rows(&items, &which);
    answer.insert("total".to_string(), Value::from(items.len() as i64));
    answer.insert("items".to_string(), Value::Array(rows));
    Value::Object(answer).to_string()
}

/// One disk scan. `None` means the package source is not determined (only
/// "System").
///
/// Warning: the "scan kind → gatherer" table is left HERE deliberately: on
/// its right side stand the live disk walks and the package manager query,
/// that is, execution. The knowledge ABOUT THE FORMAT of the key is below —
/// the core assembles and parses it (`local_scan_plan`).
fn scan_disk(state: &Arc<State>, which: &str, de: &str) -> Option<Vec<ThemeItem>> {
    let (kind, style_name) = ap::local_scan_plan(which);
    let live = live_scan().with_de(de);
    let active = live.active_map();
    let items = match kind.as_str() {
        "active" => live.active(&active),
        "downloaded" => live.downloaded(&active),
        "system" => match live.system(&active) {
            Ok(items) => items,
            Err(_) => return None,
        },
        // "Application style" as one grid: Qt styles (the same live list taken in
        // `environment`; if not taken — the core's fallback) and Kvantum themes.
        // The second element of the plan is the filter (`ap::STYLE_FILTER_*`).
        "widgetstyle" => {
            let filter = style_name.as_str();
            let mut items = Vec::new();
            if filter != ap::STYLE_FILTER_KVANTUM {
                let known = state
                    .qt_style_keys
                    .lock()
                    .map(|k| k.clone())
                    .unwrap_or_default();
                let styles = ap::shown_qt_styles(&scan::available_widget_styles(&known));
                items.extend(live.widget_styles(&active, &styles, true));
            }
            if filter != ap::STYLE_FILTER_QT {
                items.extend(live.kvantum(&active));
            }
            items
        }
        "splash" | "lockscreen" => live.lnf(&kind, &active),
        // An environment section: the sub-key is the section itself
        // (`local_scan_plan`).
        "section" => live.section(&style_name, &active),
        _ => Vec::new(),
    };
    Some(items)
}

/// Rows of local themes with the "Remove" decision (`theme_removal`): the
/// journal and the directories are read once per scan.
fn local_rows(items: &[ThemeItem], which: &str) -> Vec<Value> {
    let removal = LiveRemoval::new();
    items
        .iter()
        .map(|it| {
            let mut row = local_row(it, which);
            if it.id.starts_with("local:") {
                stamp_removal(&mut row, &removal.verdict(it));
            }
            row
        })
        .collect()
}

/// The "Remove" fields in a tile row: `removeVisible`, `removeEnabled`,
/// `removeTooltip` (the page reads them, `RemovalUi`).
fn stamp_removal(row: &mut Value, verdict: &Verdict) {
    if let Some(map) = row.as_object_mut() {
        map.insert("removeVisible".to_string(), Value::from(verdict.visible()));
        map.insert("removeEnabled".to_string(), Value::from(verdict.enabled()));
        map.insert("removeTooltip".to_string(), Value::from(verdict.tooltip()));
    }
}

/// A local theme for the page: it has its own tile kind and its own button.
/// Which kind exactly is decided by the core (`scan_local_kind`).
fn local_row(item: &ThemeItem, which: &str) -> Value {
    let mut raw = serde_json::to_value(item).unwrap_or(Value::Null);
    let kind = item
        .local_kind
        .clone()
        .unwrap_or_else(|| ap::scan_local_kind(which, &item.author).to_string());
    if let Some(map) = raw.as_object_mut() {
        map.insert("localKind".to_string(), Value::from(kind));
        // The field is not written into files (`#[serde(skip)]`), but the search on
        // the page also needs the original name — we carry it alongside, like the
        // tile kind.
        if !item.source_name.is_empty() {
            map.insert("sourceName".to_string(), Value::from(item.source_name.clone()));
        }
    }
    raw
}

// ============================================================================
// Details panel
// ============================================================================

/// The description translation (in its own thread), the carousel frames,
/// then the full details, then the remaining frames.
///
/// ⭐ PICTURES first, then text: the screenshot links are known already at
/// the moment of the click, while the store's detailed answer took up to
/// 20 s — all that time the user looked at "Loading screenshot".
///
/// ⭐ The translation is a separate signal rather than a `detail_ready`
/// field: showing the card must not wait for anything that can be sent
/// later. But the translation itself does NOT wait for the pictures either:
/// until 06.11 it went last in this same thread, and for theme 2380824
/// (eight screenshots) it arrived after a minute, while the card showed the
/// English text all that time. Now the tile's description goes to be
/// translated in its own thread at once, and the full details in a second
/// round, only if their description is different (an identical one will be
/// taken by the cache, and the page keeps the translation, `detail_answer`).
fn fetch_detail(state: &Arc<State>, handle: &Handle, item: ThemeItem) {
    let io = LiveOcsIo::new(&home());
    let preview_dir = io.preview_dir();
    let mut client = OcsClient::new(io, &preview_dir);
    let id = item.id.clone();

    // The tile's description — translate at once, without waiting for either
    // the pictures or the details (in the KDE Store it is already complete in
    // the list).
    let tile_description = item.description.clone();
    if !tile_description.trim().is_empty() {
        spawn_description_translation(state, handle, &item);
    }

    let mut seen: HashMap<usize, String> = HashMap::new();
    for (i, url) in ap::gallery_urls(&item.preview_urls, &item.preview_url)
        .into_iter()
        .enumerate()
    {
        if !still_open(state, &id) {
            return; // the item was closed — no reason to download
        }
        if emit_frame(handle, &mut client, &i.to_string(), &url) {
            seen.insert(i, url);
        }
    }

    // Fresh (up to a day) full details — from the cache, without waiting for
    // the store; the term is one for both versions of the tab
    // (`ocs::DETAIL_TTL_SECS`). Installation does not go this way: it needs
    // fresh archive links.
    // A Cinnamon spice does not need the KDE store: everything the site has is
    // already in the card from the index.
    // A GNOME extension does not need the KDE store either: the extension
    // site's details are pulled in by the details panel (a QML step), here is
    // the card from the showcase.
    let mut full = if id.starts_with(cs::ID_PREFIX) || ego::is_item_id(&id) {
        item
    } else if let Some(wid) = wh::parse_item_id(&id) {
        // Wallhaven: the uploader and the tags exist only in the picture info (one
        // API request; if it fails — the card as in the grid).
        match wh::live_detail(wid, &CancelToken::new()) {
            Ok(w) => wh::theme_item(&w),
            Err(e) => {
                rexon_core::log_warn!("wallhaven", "сведения о {wid}: {e:?}");
                item
            }
        }
    } else {
        client
            .get_item_cached(&id, rexon_core::ocs::DETAIL_TTL_SECS)
            .map(|c| c.item)
            .unwrap_or(item)
    };
    // The store's full details also come as "not installed" — without
    // checking against the journal they would reset the "✓ Applied" of the
    // opened item.
    ap::stamp_installed_live(&home(), std::slice::from_mut(&mut full));
    remember_items(state, std::slice::from_ref(&full));
    if !still_open(state, &id) {
        return;
    }
    let mut payload = serde_json::to_value(&full).unwrap_or(Value::Null);
    // "Remove" for an installed item: a theme enabled right now cannot be
    // removed — the same check as for an installed tile (`theme_removal`).
    if full.installed && full.local_kind.is_none() {
        let de = state.last_de.lock().map(|d| d.clone()).unwrap_or_default();
        let verdict = match removal::live_store_blocked(&id, &de) {
            Some(reason) => Verdict::Blocked { reason },
            None => Verdict::Journal { content_id: id.clone() },
        };
        stamp_removal(&mut payload, &verdict);
    }
    let payload = payload.to_string();
    let handle_detail = handle.clone();
    post(&handle_detail, move |mut object| {
        object.as_mut().detail_ready(QString::from(&payload));
    });

    // The full details brought a DIFFERENT description (or the tile had none)
    // — we translate it too, in its own thread, in parallel with the rest of
    // the gallery.
    if full.description != tile_description || tile_description.trim().is_empty() {
        spawn_description_translation(state, handle, &full);
    }

    // The detailed answer knows MORE screenshots than the list: we walk the
    // full set, skipping what was already downloaded from the same address.
    let mut got = 0usize;
    for (i, url) in ap::gallery_urls(&full.preview_urls, &full.preview_url)
        .into_iter()
        .enumerate()
    {
        if !still_open(state, &id) {
            return;
        }
        if seen.get(&i).map(|u| u == &url).unwrap_or(false) {
            got += 1;
            continue;
        }
        if emit_frame(handle, &mut client, &i.to_string(), &url) {
            got += 1;
        }
    }
    let count = got.to_string();
    post(handle, move |mut object| {
        object.as_mut().gallery_finished(QString::from(&count));
    });
}

/// A store item from a link in third-party text — by the same path that
/// this store's section assembles it: KDE Store — by number, Wallhaven and
/// GNOME Extensions — by a request to the site, Cinnamon Spices — from the
/// site index, AUR — by name (the card will request the passport itself).
fn resolve_linked_card(state: &Arc<State>, card: &Value) -> Result<Value, String> {
    let token = CancelToken::new();
    let mut item = match s(card, "source") {
        "aur" => {
            let name = s(card, "name");
            if name.is_empty() {
                return Err("нет имени пакета".to_string());
            }
            return Ok(json!({ "aur": name }));
        }
        "ocs" => {
            let io = LiveOcsIo::new(&home());
            let preview_dir = io.preview_dir();
            let mut client = OcsClient::new(io, &preview_dir);
            client
                .get_item_cached(s(card, "id"), rexon_core::ocs::DETAIL_TTL_SECS)
                .map(|c| c.item)
                .ok_or_else(|| "магазин не отдал товар".to_string())?
        }
        "wallhaven" => wh::live_detail(s(card, "id"), &token)
            .map(|w| wh::theme_item(&w))
            .map_err(|e| format!("{e:?}"))?,
        "ego" => {
            let pk = card.get("pk").and_then(Value::as_u64).unwrap_or(0);
            let uuid = ego::live_uuid_by_pk(pk, &token).map_err(|e| format!("{e:?}"))?;
            ego::live_detail(&uuid, &token)
                .map(|e| ego::theme_item(&e))
                .map_err(|e| format!("{e:?}"))?
        }
        "spices" => {
            let t = cs::type_for_key(s(card, "kind")).ok_or_else(|| "неизвестный тип спайса".to_string())?;
            let wanted = cs::page_url(t, &Value::from(s(card, "spices_id")));
            let refresh = cs::live_refresh(t, false);
            let sp = refresh
                .spices
                .iter()
                .find(|sp| cs::page_url(t, &sp.spices_id) == wanted)
                .ok_or_else(|| "спайса нет в индексе".to_string())?;
            let mut it = cs::theme_item(t, sp, &i18n::current_language());
            it.installed = cs::installed_state(&cs::SpiceEnv::live(), t, &sp.key).is_some();
            it
        }
        other => return Err(format!("незнакомый магазин «{other}»")),
    };
    ap::stamp_installed_live(&home(), std::slice::from_mut(&mut item));
    remember_items(state, std::slice::from_ref(&item));
    Ok(json!({ "item": item }))
}

/// Translate the description in its own thread: the window does not need to
/// wait for either the gallery or the source check — the translation
/// arrives as soon as it is ready.
fn spawn_description_translation(state: &Arc<State>, handle: &Handle, item: &ThemeItem) {
    let (state, handle, item) = (state.clone(), handle.clone(), item.clone());
    std::thread::spawn(move || translate_detail_description(&state, &handle, &item));
}

/// After how long to show the «Translating the description…» note.
///
/// ⭐ A threshold rather than "at once" — so that the note does not flicker.
/// Measurements of 06.11 on a live model (theme 2380824, 51 text nodes): a
/// translation from the cache takes milliseconds, a translation by a warm
/// model 0.27–0.31 s, and a cold one (bringing up the model 0.3–1 s +
/// translating) up to ~1.3 s. 0.7 s is above a warm translation with a
/// twofold margin, so with the cache and a warm model there is no note at
/// all, while on a cold start and with long descriptions it appears — where
/// the wait is already noticeable to the eye (within a second attention
/// drifts, and silence looks like "it is not translating").
const TRANSLATING_NOTE_DELAY: std::time::Duration = std::time::Duration::from_millis(700);

/// The second pass: translate the description of the opened item and send
/// it to the panel.
///
/// The work runs in ITS OWN thread ([`spawn_description_translation`]): the
/// window does not wait for it, and it does not wait for the gallery. One
/// door of the core faces outward,
/// [`rexon_core::translator_engine::translate_live_guarded`]: it takes the
/// target language from the locale itself, tells prose from technical text,
/// goes to the cache and brings up the engine.
///
/// Three quiet exits, and none of the three is an error:
///   * the description is empty — there is nothing to translate;
///   * the item was already closed (the user moved on to another one) —
///     another card does not need the translation, and `still_open` is asked
///     TWICE: before the long work and after it;
///   * `was_translated == false` — there is no model in the bundle, the text
///     is already in the customer's language, or it is technical text. No
///     complaint, the panel keeps the original: translation is a
///     convenience, not a condition for the tab to work. If the
///     «Translating the description…» note was already shown, it is removed
///     (`pending: false`).
///
/// The note is by the [`TRANSLATING_NOTE_DELAY`] threshold: a short
/// translation (the cache, a warm model) arrives earlier, and the note does
/// not flicker.
fn translate_detail_description(state: &Arc<State>, handle: &Handle, item: &ThemeItem) {
    if item.description.trim().is_empty() || !still_open(state, &item.id) {
        return;
    }
    let locale = rexon_core::translator::interface_locale();
    // The «Translating the description…» note — only if a translation is
    // possible at all (not Russian text for a Russian customer) and only if it
    // takes longer than the threshold. The lock is shared by the timer and the
    // outcome: the note cannot arrive AFTER the answer and hang over a ready
    // translation.
    let note = Arc::new(Mutex::new(NoteState::default()));
    if te::translation_may_follow(&item.description, &locale) {
        let (note, state, handle) = (note.clone(), state.clone(), handle.clone());
        let (id, source) = (item.id.clone(), item.description.clone());
        std::thread::spawn(move || {
            std::thread::sleep(TRANSLATING_NOTE_DELAY);
            let Ok(mut note) = note.lock() else { return };
            if note.finished || !still_open(&state, &id) {
                return;
            }
            note.shown = true;
            post_description(&handle, json!({ "itemId": id, "source": source, "pending": true }));
        });
    }
    let mut cache = rexon_core::appearance_cache::LiveCache::open(&home());
    // The theme name and the author name are not given to the engine: without
    // this "Sweet Mars" becomes "sweet Mars" in Russian, and "Nordic"
    // "Scandinavian". The core itself discards empty and too short names, so
    // there is nothing to check here.
    let done = te::translate_live_guarded(
        &item.description,
        &locale,
        &te::LiveEngine::new(),
        &mut cache,
        &[item.name.as_str(), item.author.as_str()],
    );
    let Ok(mut note) = note.lock() else { return };
    note.finished = true;
    let translated = done.was_translated && still_open(state, &item.id);
    if translated {
        post_description(
            handle,
            json!({ "itemId": item.id, "source": item.description, "translated": done.text }),
        );
    } else if note.shown {
        // There will be no translation — remove the note.
        post_description(
            handle,
            json!({ "itemId": item.id, "source": item.description, "pending": false }),
        );
    }
}

/// What has already been told to the page about the translation of one
/// description.
#[derive(Default)]
struct NoteState {
    /// The translation finished (with a result or without).
    finished: bool,
    /// The «Translating the description…» note was sent.
    shown: bool,
}

/// Send the page the answer about the description translation
/// (`descriptionReady`).
fn post_description(handle: &Handle, payload: Value) {
    let payload = payload.to_string();
    post(handle, move |mut object| {
        object.as_mut().description_ready(QString::from(&payload));
    });
}

/// Mark a new walk of the comments list and return its number.
///
/// The number changes only when the list is built ANEW: the first page or
/// another item. "Show more" (pages 1, 2, 3) does not reset the walk — those
/// comments stay on screen, and the translation already being computed for
/// them is still needed.
fn mark_comments_view(state: &Arc<State>, id: &str, page: i64) -> u64 {
    let Ok(mut view) = state.comments_view.lock() else {
        return 0;
    };
    if page == 0 || view.0 != id {
        *view = (id.to_string(), view.1 + 1);
    }
    view.1
}

/// The same comments are still shown: the same item is open and the list
/// has not been built anew since.
fn comments_shown(state: &Arc<State>, id: &str, view: u64) -> bool {
    still_open(state, id)
        && state
            .comments_view
            .lock()
            .map(|now| now.0 == id && now.1 == view)
            .unwrap_or(false)
}

/// The second pass of the comments page: translate it WHOLE in one trip to
/// the engine and send the translations to the already shown list.
///
/// ⭐ In ONE batch, not per comment:
/// [`rexon_core::translator_engine::translate_live_batch`] collects sentence
/// blocks ACROSS the texts. Comments are short (one or two phrases) and
/// there are twenty per page — one by one these would be twenty almost
/// empty blocks, that is, the engine's batch path idling. The decision
/// "whether to translate" stays individual for EACH comment, and a Russian
/// one among English ones does not go to the engine at all.
///
/// The quiet exits are the same as for the description, and none of them is
/// an error:
///   * there is nothing to translate (all comments are empty);
///   * the list has already changed — the [`comments_shown`] watcher is asked
///     TWICE, before the long work and after it;
///   * not a single translation happened (no model, the comments are
///     already in the customer's language) — no signal, no complaint, the
///     originals stay on screen.
///
/// Only the comments whose translation actually happened go into `rows`:
/// the page will not see the rest, and the "show original" link will not
/// appear above them.
fn translate_comment_page(
    state: &Arc<State>,
    handle: &Handle,
    id: &str,
    page: i64,
    view: u64,
    comments: &[String],
) {
    // Only the texts: the same second pass for store comments and for comments
    // from an AUR package page.
    let texts: Vec<&str> = comments.iter().map(String::as_str).collect();
    if texts.iter().all(|t| t.trim().is_empty()) || !comments_shown(state, id, view) {
        return;
    }
    let mut cache = rexon_core::appearance_cache::LiveCache::open(&home());
    // People in comments call the theme by name ("Sweet Mars looks great"),
    // and without protection the name gets translated just as in the
    // description. We take the item from the same store as the other branches
    // of the tab; if not found, we simply translate without protection — that
    // is no reason to give up translating.
    let item = state.items.lock().ok().and_then(|m| m.get(id).cloned());
    let guarded: Vec<&str> = item
        .as_ref()
        .map(|it| vec![it.name.as_str(), it.author.as_str()])
        .unwrap_or_default();
    let done = te::translate_live_batch_guarded(
        &texts,
        &rexon_core::translator::interface_locale(),
        &te::LiveEngine::new(),
        &mut cache,
        &guarded,
    );
    if !comments_shown(state, id, view) {
        return;
    }
    // A "what it was → what came out" pair, not a row number: by this time the
    // list may have grown by a page, and a comment's place in it is the page's
    // knowledge, not ours. By the text the page will find its rows itself.
    let rows: Vec<Value> = comments
        .iter()
        .zip(done.iter())
        .filter(|(_, tr)| tr.was_translated)
        .map(|(c, tr)| json!({ "text": c, "translated": tr.text }))
        .collect();
    if rows.is_empty() {
        return;
    }
    let payload = json!({ "itemId": id, "page": page, "rows": rows }).to_string();
    post(handle, move |mut object| {
        object
            .as_mut()
            .comment_translations_ready(QString::from(&payload));
    });
}

/// One carousel frame: from the cache or from the network, and straight to
/// the panel.
fn emit_frame(
    handle: &Handle,
    client: &mut OcsClient<LiveOcsIo>,
    index: &str,
    url: &str,
) -> bool {
    let preview_dir = rexon_core::appearance_cache::preview_dir(&home());
    match client.get_preview_image(url).filter(|p| usable_preview(p, &preview_dir)) {
        Some(local) => {
            let (index, local) = (index.to_string(), local);
            post(handle, move |mut object| {
                object
                    .as_mut()
                    .gallery_ready(QString::from(&index), QString::from(&local));
            });
            true
        }
        None => {
            // Silence is not allowed: without a refusal the panel would show "Loading
            // screenshot…" forever — it saw the successes but not the refusals.
            let index = index.to_string();
            post(handle, move |mut object| {
                object.as_mut().gallery_failed(QString::from(&index));
            });
            false
        }
    }
}

/// Whether bsdtar has to be installed before installing an item whose file
/// is a 7z archive and there is nothing to unpack it with (without a
/// question — the protection is the polkit password window).
fn seven_zip_needs_install(item: &ThemeItem, file_name: &str) -> bool {
    use rexon_core::appearance_installer as ai;
    let idx = rexon_core::ocs::file_index_for(item, file_name);
    let file = rexon_core::ocs::file_name_at(item, idx).unwrap_or_else(|| item.download_name.clone());
    ai::seven_zip_install_package(&file, ai::have_seven_zip_tool(), ai::live_seven_zip_package()).is_some()
}

/// Whether Conky has to be installed before starting the widget (without a
/// question — the protection is the polkit password window). `false` means
/// not a Conky widget, Conky is installed or it cannot be installed here.
fn conky_needs_install(item: &ThemeItem) -> bool {
    de_apply::is_conky_item(item)
        && de_apply::conky_install_package(rexon_core::de_apply::have_conky(), de_apply::live_conky_package()).is_some()
}

/// The question "allow the Conky widget to execute commands?" before
/// starting — in the interface thread, BEFORE the background work. `part`
/// is the file of a widget part (the part's button), `None` the whole
/// widget. `None` in the answer means there is nothing to ask about: the
/// button will stop a running widget, there are no commands, or the widget
/// is already allowed in its current form; `Some(answer)` is what the user
/// said, and "yes" is remembered at once.
fn ask_conky_safety(name: &str, part: Option<&str>) -> Option<bool> {
    use rexon_core::conky_safety as cs;
    let io = rexon_core::animations::SystemIo;
    let ctx = de_apply::Ctx::live();
    let running = match part {
        Some(conf) => rexon_core::conky_procs::running_confs(&io, &ctx).iter().any(|c| c == conf),
        None => de_apply::conky_running_ids(&io, &ctx).iter().any(|i| i == name),
    };
    if running {
        return None;
    }
    // Repairing the paths edits the widget's files — before the fingerprint,
    // not after.
    de_apply::conky_prepare(&ctx, name);
    let (title, text) = cs::start_question(&io, &ctx, name)?;
    let yes = crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text));
    if yes && !cs::remember(&io, &ctx, name) {
        rexon_core::log_warn!("appearance", "разрешение виджета Conky «{name}» не записалось: {}", cs::trust_path(&ctx));
    }
    Some(yes)
}

/// Whether the button will start the Conky widget (rather than stop a
/// running one) — then the background work first checks the widget's
/// programs (`conky_safety::live_prepare_start`). Looked at in the interface
/// thread, before the press outcome.
fn conky_will_start(name: &str) -> bool {
    let io = rexon_core::animations::SystemIo;
    let ctx = de_apply::Ctx::live();
    !de_apply::conky_running_ids(&io, &ctx).iter().any(|i| i == name)
}

// ============================================================================
// Installation
// ============================================================================

/// A port of `_install`: refresh the details, download the archive, install
/// (and apply). `file_name` is the file chosen by hand in the details panel;
/// if empty or the item has no such file — the automatic choice
/// (`ocs::file_index_for`).
fn install_theme(handle: &Handle, item: ThemeItem, apply_after: bool, file_name: &str) -> (String, bool) {
    let mut item = item;
    // Grid tiles and cache entries arrive WITHOUT the file list, and an old
    // cache may have kept an external link instead of an archive: we refresh
    // the details to choose a real archive for Linux.
    if item.download_links.is_empty() {
        let io = LiveOcsIo::new(&home());
        let preview_dir = io.preview_dir();
        let mut client = OcsClient::new(io, &preview_dir);
        if let Some(fresh) = client.get_item(&item.id) {
            item = fresh.item;
        }
    }

    // The name is that of the file that will be downloaded: a theme without
    // its own folder is named by it, and the `download_name` written into the
    // tile may have stayed from a previous choice.
    let file_index = rexon_core::ocs::file_index_for(&item, file_name);
    let name = rexon_core::ocs::file_name_at(&item, file_index).unwrap_or_else(|| {
        if item.download_name.is_empty() {
            format!("{}.archive", item.id)
        } else {
            item.download_name.clone()
        }
    });
    let Some(work) = mkdtemp("rexon-dl-") else {
        return (ap::install_download_failed(), false);
    };
    let dest = format!("{work}/{name}");

    // The labels of the step and the refusal come from the core: they are one
    // for both versions of the tab, and "Downloading…" is moreover one for the
    // whole product.
    say_log(handle, &rexon_core::we_window::download_start_text(""));
    // The item has several files — the log shows which one is taken.
    if let Some(note) = rexon_core::ocs::file_note_at(&item, file_index) {
        say_log(handle, &note);
    }
    let progress_handle = handle.clone();
    let net = UreqNet::new().with_progress(move |done, total| {
        if total > 0 {
            let pct = (done * 100 / total) as i32;
            post(&progress_handle, move |mut object| {
                object.as_mut().install_progress(pct);
            });
        }
    });
    let io = LiveOcsIo::with_net(net, &home());
    let preview_dir = io.preview_dir();
    let mut client = OcsClient::new(io, &preview_dir);
    let downloaded = client.download(&item, &dest, file_index, true);
    if !downloaded {
        let _ = std::fs::remove_dir_all(&work);
        return (ap::install_download_failed(), false);
    }
    post(handle, |mut object| {
        object.as_mut().install_progress(100);
    });

    let logger = log_sink(handle);
    let mut installer = LiveInstaller::new("", logger);
    let outcome = if apply_after {
        let (ok, mut msg) = installer.install_and_apply(&item, &dest);
        if ok {
            add_shell_palette_note(&item, &mut msg);
        }
        (ok, msg)
    } else {
        let (ok, msg, _names) = installer.install(&item, &dest);
        (ok, msg)
    };
    // Warning: a fix during the port: the temporary directory of the download
    // is ALWAYS removed. In Python it stayed in `/tmp` after every installation
    // (`mkdtemp` without cleanup), and over a session the directory filled up
    // with theme archives of a dozen megabytes each.
    let _ = std::fs::remove_dir_all(&work);
    (outcome.1, outcome.0)
}

// ============================================================================
// The AUR installation queue — one for the whole application
// ============================================================================
//
// The decisions (one at a time, who is next, the stage labels) are the
// core's (`aur_install::AurQueue`, `stage_of_line`). Here there is one
// executor thread per process and broadcasting the state to all live
// «Appearance» pages: the queue survives both a section change and closing
// the page (live case of 05.10.2026 — three parallel helpers, three
// password windows, a dependency race).

struct AurRunner {
    queue: Mutex<aur_install::AurQueue>,
    /// Pages to tell about the progress (closed ones fall off by themselves —
    /// by a delivery error).
    listeners: Mutex<Vec<(Handle, Arc<State>)>>,
}

fn aur_runner() -> &'static AurRunner {
    static RUNNER: std::sync::OnceLock<AurRunner> = std::sync::OnceLock::new();
    RUNNER.get_or_init(|| AurRunner {
        queue: Mutex::new(aur_install::AurQueue::new()),
        listeners: Mutex::new(Vec::new()),
    })
}

fn aur_subscribe(handle: Handle, state: Arc<State>) {
    if let Ok(mut list) = aur_runner().listeners.lock() {
        if !list.iter().any(|(_, s)| Arc::ptr_eq(s, &state)) {
            list.push((handle, state));
        }
    }
}

fn aur_snapshot() -> String {
    aur_runner().queue.lock().map(|q| q.snapshot_json()).unwrap_or_default()
}

/// Broadcast the queue state to all pages.
fn aur_broadcast() {
    let payload = aur_snapshot();
    if let Ok(mut list) = aur_runner().listeners.lock() {
        list.retain(|(handle, _)| {
            let payload = payload.clone();
            handle
                .queue(move |mut object| {
                    object.as_mut().aur_jobs_changed(QString::from(&payload));
                })
                .is_ok()
        });
    }
}

/// The executor thread: takes installations one at a time until the queue
/// is empty. The "executor is working" flag is removed by the queue itself
/// (`take_next`) under its own lock — there is no "queued, but nobody to
/// pick it up" race.
fn aur_worker() {
    loop {
        let job = match aur_runner().queue.lock() {
            Ok(mut q) => q.take_next(),
            Err(_) => return,
        };
        let Some(job) = job else {
            aur_broadcast();
            return;
        };
        aur_broadcast();
        rexon_core::log_info!(
            "aur",
            "сборка «{}»: пакеты {} (помощник {}, журнал {})",
            job.name,
            job.plan.packages.join(" "),
            job.helper,
            job.log_path
        );
        let started = std::time::Instant::now();
        let (ok, message, short) = run_aur_job(&job);
        if ok {
            rexon_core::log_info!("aur", "«{}» установлен за {:.0} с", job.name, started.elapsed().as_secs_f64());
        } else {
            rexon_core::log_warn!(
                "aur",
                "«{}» не установлен за {:.0} с: {}",
                job.name,
                started.elapsed().as_secs_f64(),
                if short.is_empty() { message.lines().next().unwrap_or("") } else { short.as_str() }
            );
        }
        if ok {
            // The new package brought themes: a cached scan would show the old state.
            if let Ok(list) = aur_runner().listeners.lock() {
                for (_, state) in list.iter() {
                    if let Ok(mut cache) = state.scan_cache.lock() {
                        cache.clear();
                    }
                }
            }
        }
        if let Ok(mut q) = aur_runner().queue.lock() {
            q.finish(&job.name, ok, &message, &short);
        }
        aur_broadcast();
    }
}

/// A new stage of a running installation — into the queue and, if it
/// changed, to the pages.
fn aur_stage(name: &str, stage: Option<aur_install::AurStage>, current: Option<&str>) {
    let changed = aur_runner()
        .queue
        .lock()
        .map(|mut q| q.set_stage(name, stage, current))
        .unwrap_or(false);
    if changed {
        aur_broadcast();
    }
}

/// Building a package (or a set) with the helper:
/// `(succeeded, outcome, brief)`.
///
/// The command, parsing the refusal and the texts are the core's
/// (`aur_install`). Here are the database lock, launching with output into a
/// file, the stages by output lines and saving the log to
/// `~/.cache/rexon/aur/<package>.log`.
fn run_aur_job(job: &aur_install::AurJob) -> (bool, String, String) {
    let name = job.name.as_str();
    let plan = &job.plan;
    let helper = job.helper.as_str();
    // A neighbouring pacman (or a snapshot hook) holds the database lock — we
    // wait, as the shared package installation waits. Warning: the wait has to
    // be real: snap-pac holds `db.lck` for about thirty seconds AFTER the end
    // of someone else's transaction, and a build started right away would fail
    // with "database busy".
    let family = system::DistroDetector::detect_live().family_live();
    // The pacman lock comes from the real `DBPath` (`pacman-conf`), not a
    // hard-coded path.
    let paths = system_plan::lock_paths_live(family);
    let mut waiter = system_plan::LiveLockWait::new(|_stage: &str, _message: &str| {});
    let (free, why) = system_plan::wait_for_pkg_lock(&mut waiter, &paths, PKG_LOCK_TIMEOUT_SECS);
    if !free {
        return (false, why.clone(), why);
    }
    aur_stage(name, Some(aur_install::AurStage::Downloading), None);
    let argv = aur_install::install_argv(helper, &plan.packages, plan.replace_conflicts);
    let log_path = job.log_path.clone();
    if let Some(dir) = std::path::Path::new(&log_path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let job_name = name.to_string();
    let mut tail = system_plan::tail_log(&log_path, system_plan::LogTailKind::Raw, move |_s, line| {
        if let Some(info) = aur_install::stage_of_line(line) {
            aur_stage(&job_name, info.stage, info.package.as_deref());
        }
    });
    utils::run(
        &utils::RunSpec {
            argv,
            output: utils::RunOutput::File(log_path.clone()),
            timeout_secs: Some(3600),
            // The output is parsed by the core (`aur_install::failure_cause`).
            lang_c: true,
            env: Vec::new(),
            stderr_to_stdout: true,
        },
        None,
    );
    tail.stop();
    // The reliable check is by the fact of the package's presence: the helper
    // sometimes returns a non-zero code even on success.
    if installed_packages().contains(name) {
        let msg = match plan.kit {
            Some(kit) => aur_install::kit_done_message(kit),
            None => i18n::trf("“%1” is installed", &[&name]),
        };
        return (true, msg, String::new());
    }
    let log = std::fs::read_to_string(&log_path).unwrap_or_default();
    // "Not authorized" from pkexec is both a closed window and a window the
    // agent did not even open; the agent writes the real reason only into the
    // system journal.
    let cause = aur_install::refine_with_agent_journal(
        aur_install::failure_cause(&log),
        &journal_since(started),
    );
    // The command for the terminal is only the package itself: the helper
    // will collect the dependencies itself, and the line stays short.
    let msg = aur_install::failure_message_for(
        name,
        helper,
        &[name.to_string()],
        &log,
        &log_path,
        cause.as_ref(),
    );
    (false, msg, aur_install::failure_short(cause.as_ref()))
}

/// The system and session journal since `since` (epoch seconds), only the
/// message text. Empty means there is no journalctl or nothing to read.
fn journal_since(since: u64) -> String {
    if since == 0 || utils::which("journalctl", None).is_none() {
        return String::new();
    }
    let mut out = String::new();
    for user in [false, true] {
        let mut argv = vec!["journalctl".to_string()];
        if user {
            argv.push("--user".to_string());
        }
        argv.extend([
            "--no-pager".to_string(),
            "-o".to_string(),
            "cat".to_string(),
            "-n".to_string(),
            "5000".to_string(),
            format!("--since=@{since}"),
        ]);
        let report = utils::run(
            &utils::RunSpec {
                argv,
                output: utils::RunOutput::Capture,
                timeout_secs: Some(15),
                lang_c: true,
                env: Vec::new(),
                stderr_to_stdout: false,
            },
            None,
        );
        out.push_str(&String::from_utf8_lossy(&report.stdout));
    }
    out
}

// ============================================================================
// The dispute over the login splash slot
// ============================================================================
//
// The Rexon login video splash (the "KSplash hybrid") and a KDE splash are
// ONE AND THE SAME place: at login only one of them is shown. Moreover the
// hybrid heals itself on every login, so silently applying another splash
// would mean promising the customer something that will not happen: the
// next login would bring ours back.
//
// The decision, the conditions and all the texts are in the core
// (`session_splash::splash_slot_*`). Here there is only showing the
// question with the same dialog that "Extensions" asks with
// (`rexon_ext_confirm` — the crate's shared shim) and executing the
// answer. The question is asked SYNCHRONOUSLY, before the background work:
// the dialog lives in the interface thread, and it cannot be shown from a
// worker thread.

/// Ask the customer if applying will take the login splash slot.
///
/// The "Yes/No → what to do" branching is in the core
/// (`Splash::resolve_splash_slot`); the Python host of the live tab makes
/// the same call. Here there is only the dialog.
///
/// Called ONLY from the interface thread.
fn splash_slot_answer(item: &ThemeItem) -> ss::SplashSlotPlan {
    let home = home();
    let kind = categories::category_by_id(std::path::Path::new(&home), item.category_id)
        .map(|c| c.apply_kind)
        .unwrap_or_default();
    let theme = ap::local_theme_name(&item.id, &item.name);
    let mut io = LiveSplashIo::new(&home, rexon_core::installer::PYTHON_FALLBACK_EXE);
    Splash::new(&mut io, &home).resolve_splash_slot(&kind, &theme, |conflict| {
        crate::extensions_actions::ask_confirm(
            &QString::from(conflict.title),
            &QString::from(&conflict.question),
        )
    })
}

/// Carry the plan through after applying (the core:
/// `Splash::finish_splash_slot`).
fn finish_splash_slot(keep_video: Option<String>, note: &str, ok: bool, msg: &mut String) {
    let home = home();
    let mut io = LiveSplashIo::new(&home, rexon_core::installer::PYTHON_FALLBACK_EXE);
    *msg = Splash::new(&mut io, &home).finish_splash_slot(keep_video.as_deref(), note, ok, msg);
}

/// The name of the Plasma global theme that this tile's "Apply" will
/// enable — the same choice as in [`AppearanceActions::apply_local_by_id`]
/// and [`AppearanceActions::act_on_store_item`]. `None` means the tile is
/// not a global theme, the item is not installed yet, or the theme is of the
/// old format (its refusal comes by the previous path, without a window).
fn global_target(item: &ThemeItem) -> Option<String> {
    if item.id.starts_with(cs::ID_PREFIX) {
        return None;
    }
    let home = home();
    let path = std::path::Path::new(&home);
    let cat = categories::category_by_id(path, item.category_id)?;
    if ainst::apply_action_for(Some(&cat), "") != ainst::ApplyAction::KdeGlobal {
        return None;
    }
    let name = if item.id.starts_with("local:") {
        ap::local_theme_name(&item.id, &item.name)
    } else {
        let installed = ap::live_logged_theme_name(&home, &item.id);
        let can_apply = ap::store_item_can_apply(path, item);
        match (ap::store_card_action(installed.is_some(), can_apply), installed) {
            (ap::StoreCardAction::ApplyInstalled, Some(name)) => name,
            _ => return None,
        }
    };
    if stale_global(item, &name).is_some() {
        return None;
    }
    Some(name)
}

/// A full theme of the old Plasma 5 format — Plasma 6 will not apply it.
/// `Some(name)` means it will not apply.
fn stale_global(item: &ThemeItem, name: &str) -> Option<String> {
    let home = home();
    let cat = categories::category_by_id(std::path::Path::new(&home), item.category_id)?;
    if cat.apply_kind != "global" {
        return None;
    }
    if ainst::live_global_theme_missing_metadata_json(&home, name) {
        Some(name.to_string())
    } else {
        None
    }
}

/// The outcome of an action of the "Shell palette" strip.
fn post_palette(handle: &Handle, msg: String, ok: bool) {
    post(handle, move |mut object| {
        object.as_mut().shell_palette_done(QString::from(&msg), ok);
    });
}

/// Apply a colour scheme by name BY THE SAME path as the "Colour schemes"
/// section (`LiveInstaller::apply` with the schemes category): the shell
/// palette has no second road of its own.
fn apply_color_scheme(installer: &mut LiveInstaller, scheme: &str) -> (bool, String) {
    let Some(cat) = categories::category_by_kind(std::path::Path::new(&home()), "colors") else {
        return (false, String::new());
    };
    let item = ThemeItem {
        id: scheme.to_string(),
        name: scheme.to_string(),
        category_id: cat.id,
        ..Default::default()
    };
    installer.apply(&item, scheme)
}

/// A colour scheme was applied with a live shell — say what will happen to
/// its palette (and if Rexon drives the palette, hand it over to the shell
/// at once).
fn add_shell_palette_note(item: &ThemeItem, msg: &mut String) {
    let is_colors = categories::category_by_id(std::path::Path::new(&home()), item.category_id)
        .is_some_and(|c| c.apply_kind == "colors");
    if !is_colors {
        return;
    }
    if let Some(note) = palette::after_scheme_applied(palette::live_detect(), &palette::PaletteHome::live()) {
        if !msg.is_empty() {
            msg.push_str("\n");
        }
        msg.push_str(&note);
    }
}

/// The outcome of a long action: reset the scan cache and report to the
/// page.
/// Remove store item `content_id` by the installed journal (a system theme —
/// with the root helper). `reload` — re-read the section after the outcome
/// (removed from a tile of a theme from disk).
fn spawn_uninstall(state: Arc<State>, handle: Handle, content_id: String, reload: bool) {
    std::thread::spawn(move || {
        let logger = log_sink(&handle);
        let mut installer = LiveInstaller::new("", logger);
        rexon_core::log_info!("theme", "удаление: {content_id}");
        // We do not remove a theme enabled right now here either: the details
        // button may be stale (the theme was enabled after the card was opened).
        if !content_id.starts_with("local:") {
            let de = state.last_de.lock().map(|d| d.clone()).unwrap_or_default();
            if let Some(reason) = removal::live_store_blocked(&content_id, &de) {
                finish_action(&state, &handle, reason, false);
                return;
            }
        }
        // A Cinnamon spice is removed entirely by its own plan: disable it, the
        // directory, translations, settings (`cinnamon_spices::plan_uninstall`).
        let (ok, msg) = if content_id.starts_with(cs::ID_PREFIX) {
            drop(installer);
            cs::live_uninstall(&content_id, log_sink(&handle))
        } else if let Some(uuid) = ego::parse_item_id(&content_id).map(str::to_string) {
            // A GNOME extension is removed by the shell (without it — our own
            // directory); a system one is not removed.
            drop(installer);
            let data_home = rexon_core::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), &home());
            let mut io = rexon_core::gnome_shell_ext::LiveShell::new();
            match gext::uninstall_own(&mut io, &uuid, &data_home) {
                Ok(()) => {
                    if let Err(e) = gext::live_journal_edit(&home(), |log| {
                        gext::journal_forget(log, &uuid);
                    }) {
                        rexon_core::log_warn!("gnome-ext", "журнал установленного: {e}");
                    }
                    (true, rexon_core::i18n::trf("🗑️ Removed: %1", &[&uuid]))
                }
                Err(e) => (false, e.message()),
            }
        } else {
            installer.uninstall(&content_id)
        };
        finish_action(&state, &handle, msg.clone(), ok);
        if reload {
            post(&handle, move |mut object| object.as_mut().removal_done(QString::from(&msg)));
        }
    });
}

/// The login screen probe (a worker thread): what draws the login screen,
/// whether this is SteamOS and the "Login manager" section — the installed
/// ones, the default one, the running one, the buttons and texts
/// (`system_exec::login_screen_view`). The answer is remembered and goes out
/// with the `loginScreenReady` signal.
fn probe_login_screen_into(state: &Arc<State>, handle: &Handle) {
    let unit = ainst::active_login_manager();
    let steamos = rexon_core::distro_compat::running_on_steamos();
    let view = rexon_core::system_exec::live().login_screen_view();
    let mut answer = json!({
        "plasmaLogin": system_plan::is_plasma_login(&unit),
        "loginManager": unit,
        "steamos": steamos,
        "canOpen": !login_settings_commands().is_empty(),
        // The login screen itself opens or offers the outgoing X11 session — a
        // strip with the "Make … the default session" button (on SteamOS the
        // section is closed entirely).
        "waylandDefault": (!steamos)
            .then(system_plan::x11_login_default_live)
            .flatten()
            .map(system_plan::wayland_default_offer),
    });
    if let (Some(obj), Ok(Value::Object(extra))) = (answer.as_object_mut(), serde_json::to_value(&view)) {
        // On SteamOS the buttons are locked in the core too: it is immutable
        // (`DistroDetector::is_immutable`), and the section is closed entirely
        // there.
        obj.extend(extra);
    }
    // The "Login screen background and look" block: with SDDM — through the
    // window's privilege session, with Plasma Login Manager — through its own
    // helper (KAuth), like its "System Settings".
    if system_plan::is_plasma_login(&unit) {
        let de = greeter_desktop(state);
        let greeter = rexon_core::plasma_login::greeter_view_live(&de);
        if let (Some(obj), Ok(v)) = (answer.as_object_mut(), serde_json::to_value(&greeter)) {
            obj.insert("plmGreeter".into(), v);
        }
    }
    if unit == "sddm" {
        let de = greeter_desktop(state);
        let look = system_plan::desktop_greeter_look_live(&de);
        let greeter = system_plan::sddm_greeter_view_live(&de, &look);
        if let (Some(obj), Ok(v)) = (answer.as_object_mut(), serde_json::to_value(&greeter)) {
            obj.insert("sddmGreeter".into(), v);
        }
    }
    let answer = answer.to_string();
    if let Ok(mut last) = state.login_screen.lock() {
        *last = answer.clone();
    }
    post(handle, move |mut object| {
        object.as_mut().login_screen_ready(QString::from(&answer));
    });
}

/// The desktop from which the look is transferred to the login screen: the
/// one the tab looks at; before the first overview — the one running now.
fn greeter_desktop(state: &Arc<State>) -> String {
    let de = state.last_de.lock().map(|d| d.clone()).unwrap_or_default();
    if !de.is_empty() {
        return de;
    }
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    rexon_core::system::detect_current_de(
        &env("XDG_CURRENT_DESKTOP"),
        &env("DESKTOP_SESSION"),
        !env("WAYLAND_DISPLAY").is_empty(),
    )
    .unwrap_or_default()
}

impl qobject::AppearanceActions {
    /// An action of the "Login screen background and look" block: in a worker
    /// thread, one at a time (the same lock as "Make default"), the outcome is
    /// `loginLookDone`, followed by a fresh login screen probe.
    fn run_login_look(self: Pin<&mut Self>, work: impl FnOnce() -> Result<String, String> + Send + 'static) {
        use std::sync::atomic::Ordering;
        let mut this = self;
        let handle = this.as_mut().qt_thread();
        let state = this.rust().state.clone();
        if state.login_switching.swap(true, Ordering::SeqCst) {
            let msg = rexon_core::i18n::tr("The previous change isn't finished yet.");
            post(&handle, move |mut object| object.as_mut().login_look_done(false, QString::from(&msg)));
            return;
        }
        std::thread::spawn(move || {
            let (ok, msg) = match work() {
                Ok(t) => (true, t),
                Err(t) => (false, t),
            };
            let first = msg.lines().next().unwrap_or("");
            if ok {
                rexon_core::log_info!("login", "фон и оформление экрана входа: {first}");
            } else {
                rexon_core::log_warn!("login", "фон и оформление экрана входа, не вышло: {first}");
            }
            state.login_switching.store(false, Ordering::SeqCst);
            post(&handle, move |mut object| object.as_mut().login_look_done(ok, QString::from(&msg)));
            probe_login_screen_into(&state, &handle);
        });
    }
}

fn finish_action(state: &Arc<State>, handle: &Handle, msg: String, ok: bool) {
    // The outcome of installing/applying/removing a theme — as one line in the
    // log.
    let first = msg.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if ok {
        rexon_core::log_info!("theme", "итог: удалось — {first}");
    } else {
        rexon_core::log_warn!("theme", "итог: не удалось — {first}");
    }
    if ok {
        // The theme folder is new, and there is a new picture next to it: a cached
        // scan would show the old state, without a preview and with someone else's
        // badge.
        if let Ok(mut cache) = state.scan_cache.lock() {
            cache.clear();
        }
    }
    post(handle, move |mut object| {
        object
            .as_mut()
            .action_done(QString::from(&msg), ok);
    });
}

// ============================================================================
// Small things
// ============================================================================

/// Deliver work into the interface thread.
///
/// Neither `unwrap()` nor a preceding "is the object alive" check: between
/// the check and the queueing the page may close, and that is not an error
/// — there is simply nobody to report to any more.
/// The login screen preview is open (one per process).
static LOGIN_PREVIEW_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The request to close the preview ("Close preview").
static LOGIN_PREVIEW_STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn post(
    handle: &Handle,
    work: impl FnOnce(Pin<&mut qobject::AppearanceActions>) + Send + 'static,
) {
    if let Err(e) = handle.queue(work) {
        if !matches!(e, cxx_qt::ThreadingQueueError::ObjectDestroyed) {
            rexon_core::log_warn!("appearance", "ответ вкладки «Оформление» не доставлен: {e:?}");
        }
    }
}

/// An installation log line into the interface.
fn say_log(handle: &Handle, text: &str) {
    let text = text.to_string();
    post(handle, move |mut object| {
        object.as_mut().install_log(QString::from(&text));
    });
}

/// The log receiver for the live installer — the same `installLog` signal.
fn log_sink(handle: &Handle) -> Box<dyn FnMut(&str) + Send> {
    let handle = handle.clone();
    Box::new(move |text: &str| say_log(&handle, text))
}

/// How long to wait for the package database lock to be released — the
/// same number the shared package installation waits with
/// (`system_plan::wait_lock_step`). There must not be one of our own here:
/// two different patiences for one and the same queue.
const PKG_LOCK_TIMEOUT_SECS: u64 = 300;

fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// A live disk overview with the store cache's raw material: previews of
/// installed themes come from there when a theme has no file of its own.
fn live_scan() -> LiveScan {
    let mut cache = rexon_core::appearance_cache::LiveCache::open(&home());
    let ocs_cache = cache.all_items_min();
    let previews: HashMap<String, String> = cache.all_previews_min().into_iter().collect();
    let sections = cache.all_preview_categories();
    LiveScan::new(ocs_cache, previews).with_store_records(sections)
}

/// Build an AUR package with THE SAME shared queue as «Install» of the AUR
/// tiles (one build per application), and wait for the outcome. For places
/// that need the package before the next step (parts of the MATE panel
/// layout). Do not call from the interface thread.
pub(crate) fn aur_install_wait(name: &str) -> Result<(), String> {
    let family = system::DistroDetector::detect_live().family_live();
    let Some(helper) = system_plan::pick_aur_helper(family, &|bin| utils::which(bin, None).is_some()) else {
        return Err(ap::aur_no_builder());
    };
    let plan = aur_install::plan_install(name, &|_| false);
    let log_path = aur_install::log_path(&home(), name);
    let queued = match aur_runner().queue.lock() {
        Ok(mut q) => q.enqueue(name, plan, &helper, &log_path),
        Err(_) => return Err(ap::aur_no_builder()),
    };
    aur_broadcast();
    if queued.start_worker {
        std::thread::spawn(aur_worker);
    }
    // A build takes up to an hour (`run_aur_job`); we wait a little longer.
    for _ in 0..(2 * 3600) {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let done = aur_runner().queue.lock().ok().and_then(|q| {
            q.jobs()
                .iter()
                .rev()
                .find(|j| j.name == name)
                .filter(|j| !j.stage.active())
                .map(|j| (j.stage == aur_install::AurStage::Done, j.message.clone()))
        });
        match done {
            Some((true, _)) => return Ok(()),
            Some((false, msg)) => return Err(msg.lines().next().unwrap_or("").to_string()),
            None => {}
        }
    }
    Err(i18n::trf("“%1” wasn't built in time", &[&name]))
}

/// The AUR helper, asked ONCE per tab session (`shutil.which` is an
/// expensive operation on a slow disk, and the answer does not change during
/// a session).
fn aur_helper(state: &Arc<State>) -> Option<String> {
    if let Ok(known) = state.aur_helper.lock() {
        if let Some(value) = known.as_ref() {
            return value.clone();
        }
    }
    let found = system_plan::pick_aur_helper(
        system::DistroDetector::detect_live().family_live(),
        &|bin| utils::which(bin, None).is_some(),
    );
    if let Ok(mut known) = state.aur_helper.lock() {
        *known = Some(found.clone());
    }
    found
}

/// Names of installed packages — a port of `aur_client.installed_set`.
fn installed_packages() -> std::collections::HashSet<String> {
    let family = system::DistroDetector::detect_live().family_live();
    let Some(query) = system_plan::installed_packages_query(family) else {
        return std::collections::HashSet::new();
    };
    if utils::which(&query.binary, None).is_none() {
        return std::collections::HashSet::new();
    }
    let report = utils::run(
        &utils::RunSpec {
            argv: query.cmd.argv.clone(),
            output: utils::RunOutput::Capture,
            timeout_secs: query.cmd.timeout_secs,
            lang_c: query.cmd.lang_c,
            env: Vec::new(),
            stderr_to_stdout: false,
        },
        None,
    );
    let ok = report.returncode == Some(0);
    system_plan::parse_installed_packages(ok, &utils::decode_text(&report.stdout))
        .into_iter()
        .collect()
}

/// Favourites — from their own storage, without the network and without a
/// disk scan.
fn favorite_items() -> Vec<ThemeItem> {
    favorites::list_items(&favorites::load_raw(&favorites::default_path()))
}

/// The set of favourites as a JSON string (a list of numbers).
fn favorite_ids() -> String {
    let ids: Vec<String> = favorite_items().into_iter().map(|it| it.id).collect();
    serde_json::to_string(&ids).unwrap_or_else(|_| "[]".to_string())
}

/// Remember the chosen desktop between launches.
///
/// Together with the choice, the desktop RUNNING at the moment of the choice
/// is written too: by it the core later tells the user's choice from an
/// inherited one (`initial_de`).
fn remember_de(de_id: &str, seen: &str) -> bool {
    let Ok(mut settings) = SettingsManager::new() else {
        return false;
    };
    let seen = if seen.is_empty() {
        system::detect_current_de(
            &env_var("XDG_CURRENT_DESKTOP"),
            &env_var("DESKTOP_SESSION"),
            !env_var("WAYLAND_DISPLAY").is_empty(),
        )
        .unwrap_or_default()
    } else {
        seen.to_string()
    };
    if settings
        .set("appearance_de", Value::from(de_id))
        .and_then(|()| settings.set("appearance_de_seen", Value::from(seen.as_str())))
        .is_err()
    {
        return false;
    }
    true
}

/// Remember the items handed up (for actions by number).
fn remember_items(state: &Arc<State>, items: &[ThemeItem]) {
    let Ok(mut known) = state.items.lock() else {
        return;
    };
    for it in items {
        if !it.id.is_empty() {
            known.insert(it.id.clone(), it.clone());
        }
    }
}

fn remember_packages(state: &Arc<State>, pkgs: &[AurPackage]) {
    let Ok(mut known) = state.packages.lock() else {
        return;
    };
    for p in pkgs {
        if !p.name.is_empty() {
            known.insert(p.name.clone(), p.clone());
        }
    }
}

/// Is the downloaded preview really a picture? Instead of a picture the
/// server sometimes gives an error page; such a file is deleted (the
/// preview cache checks that the file exists and will not give it a second
/// time), and the tile takes the item's next picture.
///
/// We judge only files of OUR OWN preview cache: someone else's file (a
/// local theme's picture) is not checked, let alone deleted.
pub(crate) fn usable_preview(path: &str, cache_dir: &str) -> bool {
    use std::io::Read;
    let ours = !cache_dir.is_empty()
        && std::path::Path::new(path).starts_with(std::path::Path::new(cache_dir));
    if !ours {
        return true;
    }
    let mut head = [0u8; 64];
    let n = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .unwrap_or(0);
    // A font file is a fit "picture" too: the font tile draws a sample with it.
    if ap::looks_like_image(&head[..n]) || (ap::is_font_file(path) && ap::looks_like_font(&head[..n])) {
        return true;
    }
    let _ = std::fs::remove_file(path);
    false
}

/// Abort the catalogue requests in flight, except the request for `keep`.
fn abort_catalog(state: &Arc<State>, keep: &str) {
    let Ok(mut cancels) = state.catalog_cancels.lock() else {
        return;
    };
    let stale: Vec<String> = cancels.keys().filter(|k| *k != keep).cloned().collect();
    for key in stale {
        if let Some(token) = cancels.remove(&key) {
            token.cancel();
        }
    }
}

/// Whether the same item is still open.
fn still_open(state: &Arc<State>, id: &str) -> bool {
    state
        .detail_id
        .lock()
        .map(|open| *open == id)
        .unwrap_or(false)
}

/// A temporary directory with a prefix — what Python called
/// `tempfile.mkdtemp`.
fn mkdtemp(prefix: &str) -> Option<String> {
    let base = std::env::temp_dir();
    for attempt in 0..64u32 {
        let name = format!(
            "{prefix}{}-{attempt}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let path = base.join(name);
        if std::fs::create_dir(&path).is_ok() {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
}

/// A string field of the request (empty — there is no field).
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// A numeric field of the request.
fn num(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

/// The request's page size; the object has no default of its own — the
/// core has it.
fn page_size(v: &Value) -> i64 {
    v.get("pageSize")
        .and_then(Value::as_i64)
        .unwrap_or(i64::from(ap::PAGE_SIZE))
}

// ============================================================================
// AUR package card
// ============================================================================
//
// What to show and how to parse it is the core's (`aur::fetch_info`,
// `aur::parse_package_page`, `aur::detail_image_sources`). Here are the
// session cache, the pause between requests to the site's pages and
// delivering the answers to the page with the same signals as for a store
// item (frames, comments, description translation).

impl qobject::AppearanceActions {
    fn load_aur_detail(mut self: Pin<&mut Self>, pkg_name: &QString) {
        let name = pkg_name.to_string();
        if name.is_empty() {
            return;
        }
        let state = self.as_ref().get_ref().rust().state.clone();
        if let Ok(mut open) = state.detail_id.lock() {
            *open = name.clone();
        }
        let tile = state.packages.lock().ok().and_then(|m| m.get(&name).cloned());
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let (info, error) = aur_info_cached(&state, &name);
            if !still_open(&state, &name) {
                return;
            }
            let payload = match (&info, &error) {
                (Some(i), _) => json!({ "itemId": name, "info": i }),
                (None, Some(e)) => json!({ "itemId": name, "error": e }),
                (None, None) => json!({ "itemId": name, "missing": true }),
            }
            .to_string();
            post(&handle, move |mut object| {
                object.as_mut().aur_detail_ready(QString::from(&payload));
            });

            // The source check — in its own thread: it makes requests to the AUR site
            // (with pauses) and to GitHub, and the pictures and the translation do not
            // wait for it.
            spawn_aur_trust(&state, &handle, &name, info.clone());

            // The description translation — by the same pass as for a store item, and
            // likewise in its own thread BEFORE the pictures: project pages give
            // screenshots slowly, and the translation does not wait for them. The
            // package and maintainer names are not given to the engine.
            let base = info.as_ref().map(|i| i.as_package()).or_else(|| tile.clone());
            if let Some(pkg) = base {
                let item = ThemeItem {
                    id: name.clone(),
                    name: pkg.name.clone(),
                    author: pkg.maintainer.clone(),
                    description: pkg.description.clone(),
                    ..Default::default()
                };
                // The name protection when translating comments takes the item from here.
                remember_items(&state, std::slice::from_ref(&item));
                spawn_description_translation(&state, &handle, &item);
            }

            // Pictures: the project address from the passport, without it — from the
            // tile.
            let upstream = info
                .as_ref()
                .map(|i| i.url.clone())
                .or_else(|| tile.as_ref().map(|p| p.upstream_url.clone()))
                .unwrap_or_default();
            let sources = aur::detail_image_sources(&upstream);
            if !sources.is_empty() {
                let io = LiveOcsIo::new(&home());
                let preview_dir = io.preview_dir();
                let mut client = OcsClient::new(io, &preview_dir);
                let mut got = 0usize;
                for src in &sources {
                    if !still_open(&state, &name) {
                        return;
                    }
                    let Some(url) = aur_image_url(&state, src) else { continue };
                    if emit_frame(&handle, &mut client, &got.to_string(), &url) {
                        got += 1;
                    }
                }
                let count = got.to_string();
                post(&handle, move |mut object| {
                    object.as_mut().gallery_finished(QString::from(&count));
                });
            }

        });
    }

    fn load_aur_pkgbuild(mut self: Pin<&mut Self>, pkg_name: &QString, pkg_base: &QString) {
        let name = pkg_name.to_string();
        let base = pkg_base.to_string();
        let base = if base.is_empty() { name.clone() } else { base };
        if name.is_empty() {
            return;
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            static TEXTS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
            let cached = TEXTS.lock().ok().and_then(|m| m.as_ref()?.get(&base).cloned());
            let answer = match cached {
                Some(t) => Ok(t),
                None => {
                    aur_site_turn();
                    let mut net = UreqNet::new();
                    let got = rexon_core::aur_trust::fetch_pkgbuild(&mut net, &base);
                    if let (Ok(t), Ok(mut m)) = (&got, TEXTS.lock()) {
                        m.get_or_insert_with(HashMap::new).insert(base.clone(), t.clone());
                    }
                    got
                }
            };
            let payload = match answer {
                Ok(text) => json!({ "itemId": name, "text": text }),
                Err(e) => json!({ "itemId": name, "error": e }),
            }
            .to_string();
            post(&handle, move |mut object| {
                object.as_mut().aur_pkgbuild_ready(QString::from(&payload));
            });
        });
    }

    fn load_aur_comments(mut self: Pin<&mut Self>, pkg_name: &QString, page: &QString) {
        let name = pkg_name.to_string();
        let page: i64 = page.to_string().parse().unwrap_or(0);
        let state = self.as_ref().get_ref().rust().state.clone();
        let view = mark_comments_view(&state, &name, page);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let answer = aur_page_cached(&state, &name, page);
            if !comments_shown(&state, &name, view) {
                return;
            }
            let mut out = Map::new();
            out.insert("itemId".to_string(), Value::from(name.as_str()));
            out.insert("page".to_string(), Value::from(page));
            let texts: Vec<String> = match &answer {
                Some(p) => {
                    let rows: Vec<Value> = p
                        .comments
                        .iter()
                        // Pinned ones repeat on every page of the site — we show them once, at the
                        // top of the first.
                        .filter(|c| page == 0 || !c.pinned)
                        .map(|c| {
                            json!({
                                "author": c.author,
                                "date": c.date,
                                "text": c.text,
                                "depth": 0,
                                "pinned": c.pinned,
                                "translated": "",
                            })
                        })
                        .collect();
                    out.insert("comments".to_string(), Value::Array(rows));
                    out.insert("hasMore".to_string(), Value::from(p.has_more));
                    out.insert("comaintainers".to_string(), json!(p.comaintainers));
                    p.comments
                        .iter()
                        .filter(|c| page == 0 || !c.pinned)
                        .map(|c| c.text.clone())
                        .collect()
                }
                None => {
                    // The page was not given (bot protection, no network): an honest
                    // "comments are on the site", not "there are no comments".
                    out.insert("comments".to_string(), Value::Array(Vec::new()));
                    out.insert("blocked".to_string(), Value::from(true));
                    Vec::new()
                }
            };
            let payload = Value::Object(out).to_string();
            post(&handle, move |mut object| {
                object.as_mut().comments_ready(QString::from(&payload));
            });
            translate_comment_page(&state, &handle, &name, page, view, &texts);
        });
    }
}

/// Checking the package source (`aur_trust`) in its own thread: the disk
/// cache for a day, otherwise the AUR site (through the shared page queue)
/// and the GitHub API without logging in. Without the RPC passport there is
/// nothing to check — the card learns that the check is over. An answer
/// about a closed card is not sent.
fn spawn_aur_trust(
    state: &Arc<State>,
    handle: &Handle,
    name: &str,
    info: Option<aur::AurInfo>,
) {
    let state = state.clone();
    let handle = handle.clone();
    let name = name.to_string();
    std::thread::spawn(move || {
        let entry = info.map(|info| {
            let mut aur_net = UreqNet::new();
            let mut gh_net = UreqNet::new().with_headers(rexon_core::aur_trust::github_api_headers());
            let mut pause = aur_site_turn;
            let mut f = rexon_core::aur_trust::Fetchers {
                aur: &mut aur_net,
                github: &mut gh_net,
                pause: &mut pause,
            };
            let path = rexon_core::aur_trust::live_cache_path();
            rexon_core::aur_trust::check_cached(&path, &mut f, &info, rexon_core::aur_trust::now_secs())
        });
        if !still_open(&state, &name) {
            return;
        }
        let payload = match entry {
            Some(e) => json!({ "itemId": name, "entry": e }),
            None => json!({ "itemId": name, "failed": true }),
        }
        .to_string();
        post(&handle, move |mut object| {
            object.as_mut().aur_trust_ready(QString::from(&payload));
        });
    });
}

/// A fresh cache entry of an AUR card.
fn aur_fresh(at: std::time::Instant) -> bool {
    at.elapsed().as_secs() < aur::DETAIL_CACHE_SECS
}

/// The package passport: from the session cache or by an RPC request.
/// `(passport, error)`: both empty means AUR does not know the package.
fn aur_info_cached(state: &Arc<State>, name: &str) -> (Option<aur::AurInfo>, Option<String>) {
    if let Some((at, info)) = state.aur_info.lock().ok().and_then(|m| m.get(name).cloned()) {
        if aur_fresh(at) {
            return (info, None);
        }
    }
    let mut net = UreqNet::new();
    match aur::fetch_info(&mut net, name) {
        Ok(info) => {
            if let Ok(mut m) = state.aur_info.lock() {
                m.insert(name.to_string(), (std::time::Instant::now(), info.clone()));
            }
            (info, None)
        }
        Err(e) => (None, Some(e.to_string())),
    }
}

/// The queue to the AUR site's pages (comments, the log and `.SRCINFO` for
/// the source check, PKGBUILD): no more often than once per
/// `aur::PAGE_PAUSE_MS` for the whole process. The lock is held during the
/// pause too: two requests in a row queue up rather than going to the site
/// at once.
fn aur_site_turn() {
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    let since = last.map(|t| t.elapsed().as_millis() as u64);
    let wait = aur::page_pause_left_ms(since);
    if wait > 0 {
        std::thread::sleep(std::time::Duration::from_millis(wait));
    }
    *last = Some(std::time::Instant::now());
}

/// A package page: from the session cache or from the site — no more often
/// than once per `aur::PAGE_PAUSE_MS` for the whole process. `None` means
/// the page was not given.
fn aur_page_cached(state: &Arc<State>, name: &str, page: i64) -> Option<aur::AurPage> {
    let key = (name.to_string(), page);
    if let Some((at, p)) = state.aur_pages.lock().ok().and_then(|m| m.get(&key).cloned()) {
        if aur_fresh(at) {
            return Some(p);
        }
    }
    aur_site_turn();
    let mut net = UreqNet::new();
    match aur::fetch_package_page(&mut net, name, page) {
        Ok(p) => {
            if let Ok(mut m) = state.aur_pages.lock() {
                m.insert(key, (std::time::Instant::now(), p.clone()));
            }
            Some(p)
        }
        Err(e) => {
            rexon_core::log_warn!("aur", "страница пакета AUR «{name}» не получена: {e:?}");
            None
        }
    }
}

/// A direct address of a card picture: the README and `og:image` markers are
/// resolved (once per session), an ordinary address is taken as is.
fn aur_image_url(state: &Arc<State>, src: &str) -> Option<String> {
    let is_marker = src.starts_with(aur::README_PREVIEW_SCHEME)
        || src.starts_with(aur::OG_IMAGE_SCHEME)
        || src.starts_with(aur::GOOGLE_FONT_SCHEME);
    if !is_marker {
        return Some(src.to_string());
    }
    if let Some(found) = state.aur_images.lock().ok().and_then(|m| m.get(src).cloned()) {
        return found;
    }
    let mut net = UreqNet::new();
    let found = if src.starts_with(aur::README_PREVIEW_SCHEME) {
        aur::resolve_readme_preview(&mut net, src)
    } else if src.starts_with(aur::GOOGLE_FONT_SCHEME) {
        aur::resolve_google_font(&mut net, src)
    } else {
        aur::resolve_og_image(&mut net, src)
    };
    if let Ok(mut m) = state.aur_images.lock() {
        m.insert(src.to_string(), found.clone());
    }
    found
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The contract with the page: the catalogue answer MUST have a request key.
    /// The core compares it with the current one (`requests.answer`) and
    /// silently discards an answer without a key — the showcase would stay empty
    /// with a live host.
    #[test]
    fn catalog_answer_always_carries_the_request_key() {
        let req = json!({
            "key": "splash:0:new::",
            "kind": "local",
            "which": "нет-такого-скана",
            "query": "",
        });
        let state = Arc::new(State::default());
        let answer: Value = serde_json::from_str(&scan_local(&state, &req)).unwrap();
        assert_eq!(answer["key"], Value::from("splash:0:new::"));
        assert!(answer["items"].is_array(), "витрина обязана быть списком");
        assert_eq!(answer["total"], Value::from(0));
    }

    /// The AUR refusal for a short query is named IN THE CORE'S WORDS, not with
    /// our own copy of the text: the label is one for both versions of the tab.
    #[test]
    fn short_aur_query_is_refused_with_core_words() {
        // The process language is shared, and the crate's neighbouring
        // comparisons set the source one: without pinning, on a Russian locale the
        // label changed between two `tr` calls and the comparison was flaky.
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let req = json!({"key": "aur:0", "kind": "aur", "query": "п", "aurKey": "x"});
        let state = Arc::new(State::default());
        let answer: Value = serde_json::from_str(&fetch_aur(&state, &req, &CancelToken::new()))
            .unwrap();
        assert_eq!(answer["error"], Value::from(ap::aur_query_too_short_msg()));
        // Warning: there must be no network here at all: a short query is cut off
        // BEFORE the trip to the RPC, otherwise every letter in the search line
        // would be a request outside.
        assert_eq!(answer["total"], Value::from(0));
    }

    /// A local theme arrives on the page WITH ITS OWN kind (`localKind`): by it
    /// it gets a different button and a different badge. Forget it, and a theme
    /// from disk would show an «Install» button that it does not have.
    #[test]
    fn local_card_names_its_kind() {
        // The process language is shared, and the crate's neighbouring
        // comparisons set the source one: without pinning, on a Russian locale the
        // label changed between two `tr` calls and the comparison was flaky.
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let item = ThemeItem {
            id: "local:active:Breeze".to_string(),
            name: "Breeze".to_string(),
            author: ap::local_active_badge(),
            ..Default::default()
        };
        let row = local_row(&item, "splash");
        assert_eq!(row["localKind"], Value::from("active"));
        // The "Downloaded" kind of the same scan — when there is no badge in the
        // label.
        let plain = ThemeItem {
            id: "local:splash:Другая".to_string(),
            name: "Другая".to_string(),
            ..Default::default()
        };
        assert_eq!(local_row(&plain, "splash")["localKind"], Value::from("downloaded"));
        // The name without translation is not written into files, but the page's
        // search needs it — it travels as a separate key, and only when it exists.
        assert!(local_row(&plain, "splash").get("sourceName").is_none());
        let translated = ThemeItem {
            id: "local:gtk:HighContrast".to_string(),
            name: "Высококонтрастная".to_string(),
            source_name: "High Contrast".to_string(),
            ..Default::default()
        };
        assert_eq!(local_row(&translated, "system")["sourceName"], Value::from("High Contrast"));
    }

    /// The set of favourites is a list of STRINGS, and an empty storage gives
    /// `[]`, not an empty string: the page parses the answer as JSON and on an
    /// empty string would lose all stars at once.
    #[test]
    fn favorites_answer_is_always_a_json_list() {
        let raw = favorite_ids();
        let parsed: Value = serde_json::from_str(&raw).expect("избранное обязано быть JSON");
        assert!(parsed.is_array(), "избранное обязано быть списком: {raw}");
        for id in parsed.as_array().unwrap() {
            assert!(id.is_string(), "номер избранного обязан быть строкой");
        }
    }
}
