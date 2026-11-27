// The hidden "Developer" page of the "Wallpaper Engine" tab in QML — a port
// of `DeveloperPageWidget` from `rexon/qt_wallpaper_engine/window.py`.
//
// Why this file was needed. The tab moved to QML as a shell (the rail and
// the page area, `we_page.rs`), while "Developer" was not ported at all
// back then: the rail item was created with an EMPTY QML file name, and the
// owner, opening it, saw an empty page with the single word `dev`. Among
// other things, the waywallen update is installed through it — that is,
// the button was unreachable.
//
// What is here and what is not:
//   * there IS the page state: the check boxes (they are also settings
//     keys), the version label, the targets' status lines, the operation
//     progress, the changelog panel, button availability, the parity
//     report and badge;
//   * there is NOT a single human-readable string or layout number. All
//     labels, colours, font sizes, paddings and verdicts are computed by
//     the core (`rexon_core::we_window`), and parsing the engine's answers
//     and comparing the schema — by `rexon_core::we_query` and
//     `rexon_core::we_parity`;
//   * there is no network or file system either: the three background tasks
//     (checking upstream, installing the AppImage, installing the OWE
//     plugin) have already moved to Rust as the separate
//     `we_update_workers` module and talk through an event queue that the
//     page drains with a timer — the same technique as in the "Steam" tab.
//
// Warning: fixes of the original (rule #16 — bugs are fixed during the port,
// not preserved), each marked `Fix:` in place:
//   4. THE VERSION LABEL FROZE THE INTERFACE. `_refresh_version` called
//      `current_version()` RIGHT IN THE GUI THREAD on every visit to the
//      page and after every installation, and that goes to the binary's
//      `--version` and to `busctl` — up to ten seconds of a motionless
//      window. Now the version is computed by a background task
//      (`start_version_refresh`), and the label arrives as an event. Number
//      4 continues the numbering of this page's fixes: 1-3 are recorded in
//      `rexon_core::we_window`, three more in `we_update_workers`;
//   6. THE VERSION LABEL DEPENDED ON A LIVE DAEMON, although it has nothing
//      to do with it. The widget page was loaded by the window's shared lazy
//      path (`_ensure_page_loaded`), which silently returns while the socket
//      is closed — and with an engine that did not come up the label stayed
//      "waywallen version: —" forever, although the version is read from
//      DISK (the binary's `--version` and the state). Here the label is
//      always updated; this path is NOT extended to the parity
//      reconciliation — that one really asks the daemon, and its auto-run
//      with a closed socket still stays silent (`reloadIfConnected`);
//   5. THE REFUSAL GATE LEFT THE BUTTON DISABLED. In the original the
//      compatibility gate was computed BEFORE `setEnabled(False)`, so a
//      refusal left the button live. The gate moved into the task (fix 1 in
//      `we_update_workers`), and the button goes off before the answer — so
//      on a refusal it must be brought back, otherwise there would be no way
//      to retry.
//
// Never: do NOT run the engine installation live: it is ~200 MiB and system
// paths.

/// cxx-qt bridge: the "Developer" page and its parity reconciliation query.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

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
        // ── Ready labels and numbers of the page. CONSTANT: both are core
        // constants and do not change during the tab's lifetime.
        #[qproperty(QMap_QString_QVariant, texts, READ = texts, CONSTANT)]
        #[qproperty(QMap_QString_QVariant, metrics, READ = metrics, CONSTANT)]
        // Palette of the active skin (rule #17); the skin changes on the fly.
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        // Opacity of muted text — the same share the widget version paints
        // labels with.
        #[qproperty(f64, variant_alpha, cxx_name = "variantAlpha",
                    READ = variant_alpha, CONSTANT)]

        // ── Check boxes (they are also the product's settings keys) ───
        #[qproperty(bool, auto_upstream, cxx_name = "autoUpstream",
                    READ = auto_upstream, WRITE = set_auto_upstream,
                    NOTIFY = flags_changed)]
        #[qproperty(bool, auto_download, cxx_name = "autoDownload",
                    READ = auto_download, WRITE = set_auto_download,
                    NOTIFY = flags_changed)]
        #[qproperty(bool, auto_parity, cxx_name = "autoParity",
                    READ = auto_parity, WRITE = set_auto_parity,
                    NOTIFY = flags_changed)]

        // ── "Upstream updates" section ────────────────────────────────
        #[qproperty(QString, version_label, cxx_name = "versionLabel",
                    READ = version_label, NOTIFY = upstream_changed)]
        /// Status lines of the targets: entries `{key, text, color, bold}`.
        #[qproperty(QList_QVariant, targets, READ = targets,
                    NOTIFY = upstream_changed)]
        #[qproperty(QString, update_status, cxx_name = "updateStatus",
                    READ = update_status, NOTIFY = upstream_changed)]
        #[qproperty(QString, changelog, READ = changelog,
                    NOTIFY = upstream_changed)]
        #[qproperty(bool, check_enabled, cxx_name = "checkEnabled",
                    READ = check_enabled, NOTIFY = upstream_changed)]
        #[qproperty(bool, update_enabled, cxx_name = "updateEnabled",
                    READ = update_enabled, NOTIFY = upstream_changed)]
        #[qproperty(bool, owe_enabled, cxx_name = "oweEnabled",
                    READ = owe_enabled, NOTIFY = upstream_changed)]
        #[qproperty(bool, seen_enabled, cxx_name = "seenEnabled",
                    READ = seen_enabled, NOTIFY = upstream_changed)]
        #[qproperty(bool, copy_enabled, cxx_name = "copyEnabled",
                    READ = copy_enabled, NOTIFY = upstream_changed)]
        type WeDeveloperModel = super::WeDeveloperModelRust;

        // ── Parity reconciliation query ───────────────────────────────
        //
        // A separate QObject rather than a page method: the reconciliation goes
        // over the daemon socket, and all of the tab's talk with it is done by
        // queries with a shared base (`impl_query_base!`). There is no reason to
        // create a second way of asking the engine here.
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError",
                    READ = forward_error, WRITE = set_forward_error,
                    NOTIFY = forward_error_changed)]
        /// The version of the LIVE daemon; it is named by the page (which knows the
        /// bus singleton), not by the query.
        #[qproperty(QString, daemon_version, cxx_name = "daemonVersion",
                    READ = daemon_version, WRITE = set_daemon_version,
                    NOTIFY = daemon_version_changed)]
        #[qproperty(QString, report_text, cxx_name = "reportText",
                    READ = report_text, NOTIFY = report_changed)]
        #[qproperty(QString, badge_text, cxx_name = "badgeText",
                    READ = badge_text, NOTIFY = report_changed)]
        #[qproperty(QString, badge_color, cxx_name = "badgeColor",
                    READ = badge_color, NOTIFY = report_changed)]
        #[qproperty(bool, badge_bold, cxx_name = "badgeBold",
                    READ = badge_bold, NOTIFY = report_changed)]
        #[qproperty(bool, drift, READ = drift, NOTIFY = report_changed)]
        type WeParityQuery = super::WeParityQueryRust;
    }

    // ── Signals ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "paletteChanged"]
        fn palette_changed(self: Pin<&mut WeDeveloperModel>);

        /// The page's check boxes (they are also product settings) changed.
        #[qsignal]
        #[cxx_name = "flagsChanged"]
        fn flags_changed(self: Pin<&mut WeDeveloperModel>);

        /// Something in the "Upstream updates" section changed. There is ONE
        /// signal for the whole section: its fields change in batches (a check
        /// answer arrived — the statuses, the changelog and four buttons changed at
        /// once), and a signal per field would mean a dozen pushes per event.
        #[qsignal]
        #[cxx_name = "upstreamChanged"]
        fn upstream_changed(self: Pin<&mut WeDeveloperModel>);

        /// The plugin archive is verified and waits to be installed THROUGH THE
        /// DAEMON: the markup passes the path to the `PluginInstallQuery` request
        /// and starts the answer guard. The attempt number is for the guard (core
        /// fix 2).
        #[qsignal]
        #[cxx_name = "oweIpcRequested"]
        fn owe_ipc_requested(self: Pin<&mut WeDeveloperModel>, zip_path: QString,
                             attempt: i32);

        /// Parity divergence mark for the "Dev" button in the rail.
        #[qsignal]
        #[cxx_name = "parityDriftChanged"]
        fn parity_drift_changed(self: Pin<&mut WeDeveloperModel>, drift: bool);

        // ── Base of the reconciliation query ──────────────────────────
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WeParityQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WeParityQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WeParityQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WeParityQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WeParityQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WeParityQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WeParityQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut WeParityQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "daemonVersionChanged"]
        fn daemon_version_changed(self: Pin<&mut WeParityQuery>);
        #[qsignal]
        #[cxx_name = "reportChanged"]
        fn report_changed(self: Pin<&mut WeParityQuery>);
        /// The reconciliation has finished: `drift` — whether to show the mark on
        /// "Dev".
        #[qsignal]
        fn checked(self: Pin<&mut WeParityQuery>, drift: bool);
    }

    // ── Property reading ──────────────────────────────────────────────────
    extern "RustQt" {
        fn texts(self: &WeDeveloperModel) -> QMap_QString_QVariant;
        fn metrics(self: &WeDeveloperModel) -> QMap_QString_QVariant;
        fn palette(self: &WeDeveloperModel) -> QMap_QString_QVariant;
        #[cxx_name = "variantAlpha"]
        fn variant_alpha(self: &WeDeveloperModel) -> f64;
        #[cxx_name = "autoUpstream"]
        fn auto_upstream(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "setAutoUpstream"]
        fn set_auto_upstream(self: Pin<&mut WeDeveloperModel>, value: bool);
        #[cxx_name = "autoDownload"]
        fn auto_download(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "setAutoDownload"]
        fn set_auto_download(self: Pin<&mut WeDeveloperModel>, value: bool);
        #[cxx_name = "autoParity"]
        fn auto_parity(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "setAutoParity"]
        fn set_auto_parity(self: Pin<&mut WeDeveloperModel>, value: bool);
        #[cxx_name = "versionLabel"]
        fn version_label(self: &WeDeveloperModel) -> QString;
        fn targets(self: &WeDeveloperModel) -> QList_QVariant;
        #[cxx_name = "updateStatus"]
        fn update_status(self: &WeDeveloperModel) -> QString;
        fn changelog(self: &WeDeveloperModel) -> QString;
        #[cxx_name = "checkEnabled"]
        fn check_enabled(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "updateEnabled"]
        fn update_enabled(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "oweEnabled"]
        fn owe_enabled(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "seenEnabled"]
        fn seen_enabled(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "copyEnabled"]
        fn copy_enabled(self: &WeDeveloperModel) -> bool;

        fn status(self: &WeParityQuery) -> i32;
        fn error(self: &WeParityQuery) -> QString;
        fn querying(self: &WeParityQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WeParityQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WeParityQuery>, value: bool);
        #[cxx_name = "daemonVersion"]
        fn daemon_version(self: &WeParityQuery) -> QString;
        #[cxx_name = "setDaemonVersion"]
        fn set_daemon_version(self: Pin<&mut WeParityQuery>, value: &QString);
        #[cxx_name = "reportText"]
        fn report_text(self: &WeParityQuery) -> QString;
        #[cxx_name = "badgeText"]
        fn badge_text(self: &WeParityQuery) -> QString;
        #[cxx_name = "badgeColor"]
        fn badge_color(self: &WeParityQuery) -> QString;
        #[cxx_name = "badgeBold"]
        fn badge_bold(self: &WeParityQuery) -> bool;
        fn drift(self: &WeParityQuery) -> bool;
    }

    // ── API for QML ───────────────────────────────────────────────────────
    extern "RustQt" {
        /// Drain the background task event queue. Called by the page timer: a
        /// queue rather than callbacks — there is one thread boundary.
        #[qinvokable]
        fn pump(self: Pin<&mut WeDeveloperModel>);

        /// The page was opened: update the version label.
        ///
        /// `daemon_version` is the `Version` property of the live daemon; only the
        /// Qt layer can ask for it, so the markup names it.
        #[qinvokable]
        #[cxx_name = "refreshVersion"]
        fn refresh_version(self: Pin<&mut WeDeveloperModel>, daemon_version: &QString);

        /// The "Check now" button and the automatic check at start-up.
        #[qinvokable]
        #[cxx_name = "startCheck"]
        fn start_check(self: Pin<&mut WeDeveloperModel>, daemon_version: &QString);

        /// Text of the question before installing the engine; empty means there is
        /// nothing to install.
        #[qinvokable]
        #[cxx_name = "confirmUpdateText"]
        fn confirm_update_text(self: &WeDeveloperModel) -> QString;

        /// Consent to install the engine was received.
        #[qinvokable]
        #[cxx_name = "startEngineInstall"]
        fn start_engine_install(self: Pin<&mut WeDeveloperModel>);

        /// Text of the question before installing the OWE plugin; empty means there
        /// is nothing to install.
        #[qinvokable]
        #[cxx_name = "confirmOweText"]
        fn confirm_owe_text(self: &WeDeveloperModel) -> QString;

        /// Consent to install the OWE plugin was received.
        #[qinvokable]
        #[cxx_name = "startOweInstall"]
        fn start_owe_install(self: Pin<&mut WeDeveloperModel>, daemon_version: &QString);

        /// The "OWE: mark as reviewed" button.
        #[qinvokable]
        #[cxx_name = "markOweSeen"]
        fn mark_owe_seen(self: Pin<&mut WeDeveloperModel>);

        /// The "Copy all" button under the changelog panel.
        #[qinvokable]
        #[cxx_name = "copyChangelog"]
        fn copy_changelog(self: Pin<&mut WeDeveloperModel>);

        /// The daemon accepted the plugin (the `plugin_install` answer).
        #[qinvokable]
        #[cxx_name = "oweIpcInstalled"]
        fn owe_ipc_installed(self: Pin<&mut WeDeveloperModel>, plugin_id: &QString,
                             needs_restart: bool);

        /// The daemon rejected the plugin installation.
        #[qinvokable]
        #[cxx_name = "oweIpcRefused"]
        fn owe_ipc_refused(self: Pin<&mut WeDeveloperModel>, message: &QString);

        /// The guard of the daemon's answer to `plugin_install` fired.
        #[qinvokable]
        #[cxx_name = "oweIpcTimeout"]
        fn owe_ipc_timeout(self: Pin<&mut WeDeveloperModel>, attempt: i32);

        /// The tab is being closed: stop the background tasks and wait for them.
        #[qinvokable]
        fn shutdown(self: Pin<&mut WeDeveloperModel>);

        #[qinvokable]
        fn reload(self: Pin<&mut WeParityQuery>);

        /// Auto-reconciliation on opening the page: STAYS SILENT without a live
        /// daemon.
        ///
        /// Exactly the same order as in the widget version: it loaded the dev page
        /// only with an open socket (`_ensure_page_loaded`), so with a dead daemon
        /// the user saw not a refusal but the ordinary placeholder "press
        /// "Reconcile"". A manual button press, on the contrary, must explain why
        /// nothing worked — that remains `reload`.
        #[qinvokable]
        #[cxx_name = "reloadIfConnected"]
        fn reload_if_connected(self: Pin<&mut WeParityQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WeParityQuery>, token: i64, frame: &QByteArray);
    }

    impl cxx_qt::Initialize for WeDeveloperModel {}
    impl cxx_qt::Threading for WeDeveloperModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QList, QMap, QMapPair_QString_QVariant, QString, QVariant};
use prost::Message;
use std::sync::Arc;

use rexon_core::engine_update as eu;
use rexon_core::proto::control as pb;
use rexon_core::we_parity;
use rexon_core::we_query::{self, QuerySignal, QueryState};
use rexon_core::we_window as ww;
use rexon_core::{settings, ui_base};

use crate::impl_query_base;
use crate::query_base::decode_failed;
use crate::we_env;
use crate::we_update_workers::{self, UpdateEnv, UpdateEvent, WeUpdateTasks};

// ============================================================================
// Translation odds and ends in Qt
// ============================================================================

fn qstr(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

fn vmap(pairs: Vec<(&str, QVariant)>) -> QMap<QMapPair_QString_QVariant> {
    let mut out = QMap::<QMapPair_QString_QVariant>::default();
    for (key, value) in pairs {
        out.insert(QString::from(key), value);
    }
    out
}

/// A status line in the form the markup reads it.
fn line_map(key: &str, line: &ww::StatusLine) -> QVariant {
    QVariant::from(&vmap(vec![
        ("key", qstr(key)),
        ("text", qstr(&line.text)),
        ("color", qstr(&line.color)),
        ("bold", QVariant::from(&line.bold)),
    ]))
}

// ============================================================================
// Page state
// ============================================================================

pub struct WeDeveloperModelRust {
    /// Entry number in the shared skin change registry (`crate::skin`).
    skin_watch: u64,
    auto_upstream: bool,
    auto_download: bool,
    auto_parity: bool,
    /// Background tasks; `None` means the runtime did not come up, and the
    /// buttons stay silent.
    tasks: Option<Arc<WeUpdateTasks>>,
    /// The engine version label in full (composed by the core).
    version_label: String,
    /// Status lines of the targets in the order of `we_window::STATUS_TARGETS`.
    lines: Vec<(String, ww::StatusLine)>,
    /// The latest check answers — `latest`/`title` are taken from them.
    statuses: Vec<eu::TargetStatus>,
    update_status: String,
    changelog: String,
    check_enabled: bool,
    update_enabled: bool,
    owe_enabled: bool,
    seen_enabled: bool,
    copy_enabled: bool,
    /// The available waywallen and OWE update (what the buttons install).
    pending_update: Option<eu::TargetStatus>,
    pending_owe: Option<eu::TargetStatus>,
    /// Auto-installation — no more than once per session, as in the original.
    auto_install_done: bool,
    auto_owe_done: bool,
    /// Version of the live daemon named by the markup at the last operation.
    daemon_version: String,
    /// The temporary plugin archive waiting for the daemon's answer, and its
    /// version.
    owe_ipc_zip: String,
    owe_ipc_version: String,
    /// Number of the IPC installation attempt (core fix 2: the guard remembers
    /// ITS OWN).
    owe_ipc_attempt: u64,
}

impl Default for WeDeveloperModelRust {
    fn default() -> Self {
        let settings = settings::SettingsManager::new().ok();
        let flag = |key: &str, fallback: bool| -> bool {
            settings
                .as_ref()
                .and_then(|s| s.get(key))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(fallback)
        };
        Self {
            skin_watch: 0,
            auto_upstream: flag(ww::KEY_AUTO_UPSTREAM, ww::DEFAULT_AUTO_UPSTREAM),
            auto_download: flag(ww::KEY_AUTO_DOWNLOAD, ww::DEFAULT_AUTO_DOWNLOAD),
            auto_parity: flag(ww::KEY_AUTO_PARITY, ww::DEFAULT_AUTO_PARITY),
            tasks: WeUpdateTasks::new(UpdateEnv::from_process()).map(Arc::new),
            version_label: ww::version_label_initial().to_string(),
            lines: ww::STATUS_TARGETS
                .iter()
                .map(|(key, name)| {
                    ((*key).to_string(), ww::target_status_initial_parts(name))
                })
                .collect(),
            statuses: Vec::new(),
            update_status: String::new(),
            changelog: String::new(),
            // Checking is possible at once, installing only after a check: before it
            // neither what to install nor where from is known. Exactly the same
            // initial values as in the widget version.
            check_enabled: true,
            update_enabled: false,
            owe_enabled: false,
            seen_enabled: false,
            copy_enabled: false,
            pending_update: None,
            pending_owe: None,
            auto_install_done: false,
            auto_owe_done: false,
            daemon_version: String::new(),
            owe_ipc_zip: String::new(),
            owe_ipc_version: String::new(),
            owe_ipc_attempt: 0,
        }
    }
}

impl Drop for WeDeveloperModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
        // The tasks are stopped here too, not only on `shutdown()` from the
        // markup: the tab may be closed without asking the page.
        if let Some(tasks) = self.tasks.take() {
            tasks.shutdown();
        }
    }
}

impl qobject::WeDeveloperModel {
    // ── Labels, numbers and styling ───────────────────────────────────────

    /// All page labels as one map: the markup reads them by name.
    ///
    /// As a map rather than two dozen separate properties: the labels are core
    /// constants, each would have its own getter and its own line in the
    /// bridge, while they are all read the same way.
    fn texts(&self) -> QMap<QMapPair_QString_QVariant> {
        vmap(vec![
            ("title", qstr(&ww::dev_title())),
            ("subtitle", qstr(&ww::dev_subtitle())),
            ("sectionUpstream", qstr(&ww::section_upstream())),
            ("sectionParity", qstr(&ww::section_parity())),
            ("cbAutoUpstream", qstr(&ww::cb_auto_upstream())),
            ("cbAutoDownload", qstr(&ww::cb_auto_download())),
            ("cbAutoParity", qstr(&ww::cb_auto_parity())),
            ("btnCheck", qstr(&ww::btn_check())),
            ("btnUpdate", qstr(&ww::btn_update())),
            ("btnOwe", qstr(&ww::btn_owe())),
            ("btnSeen", qstr(&ww::btn_seen())),
            ("btnCopy", qstr(&ww::btn_copy())),
            ("btnParity", qstr(&ww::btn_parity())),
            ("labelChangelog", qstr(&ww::label_changelog())),
            ("placeholderChangelog", qstr(&ww::placeholder_changelog())),
            ("placeholderParity", qstr(&ww::placeholder_parity())),
            ("confirmUpdateTitle", qstr(&ww::confirm_update_title())),
            ("confirmOweTitle", qstr(&ww::confirm_owe_title())),
            ("dialogYes", qstr(&ww::dialog_yes())),
            ("dialogNo", qstr(&ww::dialog_no())),
        ])
    }

    /// All layout numbers as one map — by the same argument as the labels.
    fn metrics(&self) -> QMap<QMapPair_QString_QVariant> {
        let num = |v: i32| QVariant::from(&v);
        vmap(vec![
            ("titleFontSize", num(ww::DEV_TITLE_FONT_SIZE)),
            ("titleFontWeight", num(ww::DEV_TITLE_FONT_WEIGHT)),
            ("sectionFontSize", num(ww::DEV_SECTION_FONT_SIZE)),
            ("sectionFontWeight", num(ww::DEV_SECTION_FONT_WEIGHT)),
            ("monoFontSize", num(ww::DEV_MONO_FONT_SIZE)),
            ("mutedFontSize", num(ww::DEV_MUTED_FONT_SIZE)),
            ("statusFontSize", num(ww::DEV_STATUS_FONT_SIZE)),
            ("changelogLabelWeight", num(ww::DEV_CHANGELOG_LABEL_WEIGHT)),
            ("pageMargin", num(ww::DEV_PAGE_MARGIN)),
            ("columnSpacing", num(ww::DEV_COLUMN_SPACING)),
            ("panePadding", num(ww::DEV_PANE_PADDING)),
            ("paneSpacing", num(ww::DEV_PANE_SPACING)),
            ("rowSpacing", num(ww::DEV_ROW_SPACING)),
            ("changelogMinHeight", num(ww::DEV_CHANGELOG_MIN_HEIGHT)),
            ("changelogMaxHeight", num(ww::DEV_CHANGELOG_MAX_HEIGHT)),
            ("startupDelayMs", num(ww::STARTUP_DELAY_MS)),
            ("oweIpcTimeoutMs", num(ww::OWE_IPC_TIMEOUT_MS)),
        ])
    }

    /// Palette of the ACTIVE skin (rule #17) — the same way as for the tab
    /// shell: the page defines no colours of its own.
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        out
    }

    fn variant_alpha(&self) -> f64 {
        ww::VARIANT_ALPHA as f64
    }

    // ── Check boxes ───────────────────────────────────────────────────────

    fn auto_upstream(&self) -> bool {
        self.rust().auto_upstream
    }

    fn auto_download(&self) -> bool {
        self.rust().auto_download
    }

    fn auto_parity(&self) -> bool {
        self.rust().auto_parity
    }

    fn set_auto_upstream(mut self: Pin<&mut Self>, value: bool) {
        if self.rust().auto_upstream == value {
            return;
        }
        self.as_mut().rust_mut().auto_upstream = value;
        self.as_mut().store_flag(ww::KEY_AUTO_UPSTREAM, value);
        self.as_mut().flags_changed();
    }

    fn set_auto_download(mut self: Pin<&mut Self>, value: bool) {
        if self.rust().auto_download == value {
            return;
        }
        self.as_mut().rust_mut().auto_download = value;
        self.as_mut().store_flag(ww::KEY_AUTO_DOWNLOAD, value);
        self.as_mut().flags_changed();
    }

    fn set_auto_parity(mut self: Pin<&mut Self>, value: bool) {
        if self.rust().auto_parity == value {
            return;
        }
        self.as_mut().rust_mut().auto_parity = value;
        self.as_mut().store_flag(ww::KEY_AUTO_PARITY, value);
        self.as_mut().flags_changed();
    }

    /// Write a check box into the product settings. A write failure stays
    /// silent: it is no reason to cancel the toggle itself, and the page works
    /// without settings.
    ///
    /// Warning: the settings manager is taken ANEW for every write rather than
    /// kept as a page field. Saving writes the WHOLE settings map to disk, so a
    /// long-lived snapshot returned the file to the state it had when the page
    /// was opened: the skin, the wallpaper volume and the tutorial, changed in
    /// "Settings" since then, were silently rolled back by one click of a check
    /// box here, and keys added in the meantime disappeared altogether. All the
    /// other writers of this file do the same
    /// (`settings_actions::save_setting`, `tray`, `appearance_actions`).
    fn store_flag(self: Pin<&mut Self>, key: &str, value: bool) {
        let _ = settings::SettingsManager::new()
            .and_then(|mut m| m.set(key, serde_json::Value::Bool(value)));
    }

    // ── "Upstream updates" section ────────────────────────────────────────

    fn version_label(&self) -> QString {
        QString::from(&self.rust().version_label)
    }

    fn targets(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (key, line) in &self.rust().lines {
            out.append(line_map(key, line));
        }
        out
    }

    fn update_status(&self) -> QString {
        QString::from(&self.rust().update_status)
    }

    fn changelog(&self) -> QString {
        QString::from(&self.rust().changelog)
    }

    fn check_enabled(&self) -> bool {
        self.rust().check_enabled
    }

    fn update_enabled(&self) -> bool {
        self.rust().update_enabled
    }

    fn owe_enabled(&self) -> bool {
        self.rust().owe_enabled
    }

    fn seen_enabled(&self) -> bool {
        self.rust().seen_enabled
    }

    fn copy_enabled(&self) -> bool {
        self.rust().copy_enabled
    }

    /// Set a target's status line. No such target — silence: the set of
    /// targets is named by the core, and an extra key here is not the page's
    /// error.
    fn set_line(mut self: Pin<&mut Self>, key: &str, line: ww::StatusLine) {
        let mut me = self.as_mut().rust_mut();
        if let Some(slot) = me.lines.iter_mut().find(|(k, _)| k == key) {
            slot.1 = line;
        }
    }

    // ── Entry from the markup ─────────────────────────────────────────────

    fn refresh_version(mut self: Pin<&mut Self>, daemon_version: &QString) {
        let version = daemon_version.to_string();
        self.as_mut().rust_mut().daemon_version = version.clone();
        // Warning: fix 4: the version is computed by a BACKGROUND task. In the
        // original this call went to the binary's `--version` and to `busctl`
        // right in the GUI thread.
        if let Some(tasks) = self.rust().tasks.clone() {
            tasks.start_version_refresh(version);
        }
    }

    fn start_check(mut self: Pin<&mut Self>, daemon_version: &QString) {
        let version = daemon_version.to_string();
        self.as_mut().rust_mut().daemon_version = version.clone();
        let Some(tasks) = self.rust().tasks.clone() else {
            return;
        };
        // The original's early return: a check is already running.
        if !tasks.start_check(version) {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            me.check_enabled = false;
            me.update_status = ww::text_check_running().to_string();
        }
        self.as_mut().upstream_changed();
    }

    fn confirm_update_text(&self) -> QString {
        let Some(st) = self.rust().pending_update.as_ref() else {
            return QString::default();
        };
        QString::from(&ww::confirm_update_text(&st.latest, &st.asset_url))
    }

    fn start_engine_install(mut self: Pin<&mut Self>) {
        let Some(st) = self.rust().pending_update.clone() else {
            return;
        };
        let Some(tasks) = self.rust().tasks.clone() else {
            return;
        };
        if !tasks.start_engine_install(st.asset_url, st.latest) {
            return;
        }
        self.as_mut().rust_mut().update_enabled = false;
        self.as_mut().upstream_changed();
    }

    fn confirm_owe_text(&self) -> QString {
        let Some(st) = self.rust().pending_owe.as_ref() else {
            return QString::default();
        };
        QString::from(&ww::confirm_owe_text(&st.latest, &st.asset_url))
    }

    fn start_owe_install(mut self: Pin<&mut Self>, daemon_version: &QString) {
        let version = daemon_version.to_string();
        self.as_mut().rust_mut().daemon_version = version.clone();
        let Some(st) = self.rust().pending_owe.clone() else {
            return;
        };
        let Some(tasks) = self.rust().tasks.clone() else {
            return;
        };
        // The daemon is connected → hot-install through its own IPC; otherwise
        // unpacking into the plugin directory inside the task. The core decides
        // which way.
        let fs_install = ww::plugin_fs_install(crate::discover_queries::connected());
        if !tasks.start_owe_install(st.asset_url, st.latest, fs_install, version) {
            return;
        }
        self.as_mut().rust_mut().owe_enabled = false;
        self.as_mut().upstream_changed();
    }

    fn mark_owe_seen(mut self: Pin<&mut Self>) {
        let Some(st) = self
            .rust()
            .statuses
            .iter()
            .find(|s| s.key == "owe")
            .cloned()
        else {
            return;
        };
        if st.latest.is_empty() {
            return;
        }
        let Some(tasks) = self.rust().tasks.clone() else {
            return;
        };
        tasks.mark_seen("owe", &st.latest);
        let line = ww::owe_seen_parts(&st.title, &st.latest);
        self.as_mut().set_line("owe", line);
        self.as_mut().rust_mut().seen_enabled = false;
        self.as_mut().upstream_changed();
    }

    fn copy_changelog(mut self: Pin<&mut Self>) {
        let text = self.rust().changelog.clone();
        if text.is_empty() {
            return;
        }
        // Warning: the shim function is called THROUGH the bridge of the `Action`
        // singleton rather than declared here a second time: cxx emits the adapter
        // by the function name, and two declarations of the same one give a
        // duplicate symbol at link time
        // (`duplicate symbol: cxxbridge1$…$rexon_we_copy_to_clipboard`).
        crate::we_action::qobject::rexon_we_copy_to_clipboard(&QString::from(&text));
        self.as_mut().rust_mut().update_status = ww::text_changelog_copied().to_string();
        self.as_mut().upstream_changed();
    }

    fn shutdown(mut self: Pin<&mut Self>) {
        if let Some(tasks) = self.as_mut().rust_mut().tasks.take() {
            tasks.shutdown();
        }
    }

    // ── The daemon's answer to the plugin installation ────────────────────

    fn owe_ipc_installed(
        mut self: Pin<&mut Self>,
        plugin_id: &QString,
        needs_restart: bool,
    ) {
        self.as_mut().drop_ipc_zip();
        let version = self.rust().owe_ipc_version.clone();
        if let Some(tasks) = self.rust().tasks.clone() {
            tasks.mark_installed_tag("owe", &version);
        }
        let text = ww::owe_ipc_success_text(&plugin_id.to_string(), needs_restart);
        self.finish_owe_install(true, &text);
    }

    fn owe_ipc_refused(mut self: Pin<&mut Self>, message: &QString) {
        self.as_mut().drop_ipc_zip();
        let text = message.to_string();
        let text = if text.is_empty() {
            ww::text_owe_ipc_refused().to_string()
        } else {
            text
        };
        self.finish_owe_install(false, &text);
    }

    /// Guard of the daemon's answer. It fires only if it belongs to the CURRENT
    /// attempt and there is still no answer — core fix 2.
    fn owe_ipc_timeout(mut self: Pin<&mut Self>, attempt: i32) {
        let fires = ww::owe_ipc_timeout_fires(
            attempt.max(0) as u64,
            self.rust().owe_ipc_attempt,
            !self.rust().owe_ipc_zip.is_empty(),
        );
        if !fires {
            return;
        }
        self.as_mut().drop_ipc_zip();
        self.finish_owe_install(false, &ww::text_owe_ipc_timeout());
    }

    /// Remove the temporary archive that was waiting for the daemon's answer.
    fn drop_ipc_zip(mut self: Pin<&mut Self>) {
        let zip = std::mem::take(&mut self.as_mut().rust_mut().owe_ipc_zip);
        we_update_workers::drop_tmp(&zip);
    }

    // ── Background task event queue ───────────────────────────────────────

    fn pump(mut self: Pin<&mut Self>) {
        loop {
            let Some(tasks) = self.rust().tasks.clone() else {
                return;
            };
            let Some(event) = tasks.poll_event() else {
                return;
            };
            self.as_mut().handle_event(event);
        }
    }
}

// ============================================================================
// Internals: receiving events and recomputations
// ============================================================================

impl qobject::WeDeveloperModel {
    fn handle_event(mut self: Pin<&mut Self>, event: UpdateEvent) {
        match event {
            UpdateEvent::EngineVersion(version) => {
                self.as_mut().rust_mut().version_label = ww::version_label(&version);
                self.as_mut().upstream_changed();
            }
            UpdateEvent::Progress(message) => {
                self.as_mut().rust_mut().update_status = message;
                self.as_mut().upstream_changed();
            }
            UpdateEvent::CheckDone(statuses) => self.as_mut().on_check_done(statuses),
            UpdateEvent::EngineDone { ok, message } => {
                self.as_mut().on_engine_done(ok, &message);
            }
            UpdateEvent::OweDone {
                ok,
                message,
                zip_path,
            } => self.as_mut().on_owe_done(ok, &message, &zip_path),
            UpdateEvent::OweGateRefused(text) => {
                {
                    let mut me = self.as_mut().rust_mut();
                    me.update_status = text;
                    // Warning: fix 5: we bring the button back. The gate is now computed by
                    // the task, and the button managed to go off before its answer — leaving
                    // it dead would take away the retry.
                    me.owe_enabled = me.pending_owe.is_some();
                }
                self.as_mut().upstream_changed();
            }
        }
    }

    /// A port of `_on_upstream_results`.
    fn on_check_done(mut self: Pin<&mut Self>, statuses: Vec<eu::TargetStatus>) {
        self.as_mut().rust_mut().check_enabled = true;
        if statuses.is_empty() {
            // Network/rate limit — a quiet status, no crashes.
            self.as_mut().rust_mut().update_status = ww::text_check_failed().to_string();
            self.as_mut().upstream_changed();
            return;
        }
        let mut pending_update = None;
        let mut pending_owe = None;
        let mut seen_enabled = self.rust().seen_enabled;
        for st in &statuses {
            let line = ww::target_status_parts(st.ok, &st.title, &st.note, st.update_available);
            self.as_mut().set_line(&st.key, line);
            if !st.ok {
                continue;
            }
            if st.key == "waywallen" && st.update_available && !st.asset_url.is_empty() {
                pending_update = Some(st.clone());
            }
            if st.key == "owe" {
                seen_enabled = st.update_available;
                if st.update_available && !st.asset_url.is_empty() {
                    pending_owe = Some(st.clone());
                }
            }
        }
        let with_changelog: Vec<eu::TargetStatus> = statuses
            .iter()
            .filter(|s| !s.changelog.is_empty())
            .cloned()
            .collect();
        let rendered = eu::render_changelog_text(&with_changelog);
        {
            let mut me = self.as_mut().rust_mut();
            me.statuses = statuses;
            me.update_enabled = pending_update.is_some();
            me.owe_enabled = pending_owe.is_some();
            me.pending_update = pending_update;
            me.pending_owe = pending_owe;
            me.seen_enabled = seen_enabled;
            me.changelog = ww::changelog_panel_text(&rendered);
            me.copy_enabled = !rendered.is_empty();
            me.update_status = ww::text_check_done().to_string();
        }
        self.as_mut().upstream_changed();

        // Auto-installation: only with the check box (default OFF) and once per
        // session. The verdict is the core's.
        let auto = self.rust().auto_download;
        let want_engine = ww::should_auto_install(
            self.rust().pending_update.is_some(),
            auto,
            self.rust().auto_install_done,
        );
        if want_engine {
            self.as_mut().rust_mut().auto_install_done = true;
            self.as_mut().start_engine_install();
        }
        let want_owe = ww::should_auto_install(
            self.rust().pending_owe.is_some(),
            auto,
            self.rust().auto_owe_done,
        );
        if want_owe {
            self.as_mut().rust_mut().auto_owe_done = true;
            let version = QString::from(&self.rust().daemon_version);
            self.as_mut().start_owe_install(&version);
        }
    }

    /// A port of `_on_install_done`.
    fn on_engine_done(mut self: Pin<&mut Self>, ok: bool, message: &str) {
        {
            let mut me = self.as_mut().rust_mut();
            me.update_status = ww::install_done_text(ok, message);
            // The installation succeeded — there is nothing more to install.
            me.update_enabled = !ok;
        }
        if ok {
            let line = ww::waywallen_installed_parts();
            self.as_mut().set_line("waywallen", line);
        }
        self.as_mut().upstream_changed();
        // The version label is re-read IN THE BACKGROUND (fix 4).
        let version = QString::from(&self.rust().daemon_version);
        self.as_mut().refresh_version(&version);
    }

    /// A port of `_on_owe_worker_done`.
    fn on_owe_done(mut self: Pin<&mut Self>, ok: bool, message: &str, zip_path: &str) {
        if !ok {
            {
                let mut me = self.as_mut().rust_mut();
                me.update_status = ww::owe_done_text(false, message);
                me.owe_enabled = true;
            }
            self.as_mut().upstream_changed();
            return;
        }
        if zip_path.is_empty() {
            // The task has already done the file system installation.
            self.finish_owe_install(true, message);
            return;
        }
        if !crate::discover_queries::connected() {
            // The daemon died while downloading — the file system fallback (fast: no
            // network).
            let tag = self
                .rust()
                .pending_owe
                .as_ref()
                .map(|s| s.latest.clone())
                .unwrap_or_default();
            let (ok2, text) = match self.rust().tasks.clone() {
                Some(tasks) => tasks.install_plugin_zip_now(zip_path, &tag),
                None => (false, ww::text_owe_ipc_refused().to_string()),
            };
            we_update_workers::drop_tmp(zip_path);
            self.finish_owe_install(ok2, &text);
            return;
        }
        // Hot-install through the daemon: it unpacks the archive itself.
        let attempt = {
            let mut me = self.as_mut().rust_mut();
            me.update_status = ww::text_owe_ipc_running().to_string();
            me.owe_ipc_zip = zip_path.to_string();
            me.owe_ipc_version = me
                .pending_owe
                .as_ref()
                .map(|s| s.latest.clone())
                .unwrap_or_default();
            me.owe_ipc_attempt += 1;
            me.owe_ipc_attempt
        };
        self.as_mut().upstream_changed();
        // The socket is touched by the markup through `PluginInstallQuery`, and it
        // also starts the answer guard: a second way of talking to the daemon of
        // our own is not needed here.
        let zip = QString::from(zip_path);
        self.as_mut()
            .owe_ipc_requested(zip, attempt.min(i32::MAX as u64) as i32);
    }

    /// A port of `_finish_owe_install`.
    fn finish_owe_install(mut self: Pin<&mut Self>, ok: bool, message: &str) {
        {
            let mut me = self.as_mut().rust_mut();
            me.update_status = ww::owe_done_text(ok, message);
            me.owe_enabled = !ok;
        }
        if ok {
            let tag_version = eu::version_tag_to_state(&self.rust().owe_ipc_version);
            let fs_version = self
                .rust()
                .tasks
                .as_ref()
                .map(|t| we_update_workers::owe_installed_version(t.env()))
                .unwrap_or_default();
            let version = if fs_version.is_empty() {
                tag_version
            } else {
                fs_version
            };
            let line = ww::owe_installed_parts(&version);
            self.as_mut().set_line("owe", line);
            self.as_mut().rust_mut().seen_enabled = false;
        }
        self.as_mut().upstream_changed();
    }
}

impl cxx_qt::Initialize for qobject::WeDeveloperModel {
    fn initialize(mut self: Pin<&mut Self>) {
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::WeDeveloperModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
    }
}

// ============================================================================
// WeParityQuery — reconciling the renderer settings schema with the UI registry
// ============================================================================

#[derive(Default)]
pub struct WeParityQueryRust {
    base: QueryState,
    data: we_query::ParitySchemaData,
    pending: std::collections::BTreeSet<i64>,
    /// Version of the LIVE daemon — named by the page.
    daemon_version: String,
    /// Text of the report panel.
    report_text: String,
    /// Badge next to the "Reconcile with the engine" button.
    badge: Option<ww::StatusLine>,
    /// Whether to show the mark on the "Dev" button.
    drift: bool,
}

impl_query_base!(WeParityQuery);

impl qobject::WeParityQuery {
    pub fn daemon_version(&self) -> QString {
        QString::from(&self.daemon_version)
    }

    pub fn set_daemon_version(mut self: Pin<&mut Self>, value: &QString) {
        let next = value.to_string();
        if self.daemon_version == next {
            return;
        }
        self.as_mut().rust_mut().daemon_version = next;
        self.daemon_version_changed();
    }

    pub fn report_text(&self) -> QString {
        QString::from(&self.report_text)
    }

    pub fn badge_text(&self) -> QString {
        QString::from(self.badge.as_ref().map_or("", |b| b.text.as_str()))
    }

    pub fn badge_color(&self) -> QString {
        QString::from(self.badge.as_ref().map_or("", |b| b.color.as_str()))
    }

    pub fn badge_bold(&self) -> bool {
        self.badge.as_ref().is_some_and(|b| b.bold)
    }

    pub fn drift(&self) -> bool {
        self.drift
    }

    /// The "Reconcile with the engine" button and auto-reconciliation on opening
    /// the page.
    ///
    /// No daemon — the core's own short line rather than the base's generic "no
    /// connection" error: in the widget version the check stood BEFORE sending,
    /// and that is exactly what the user read.
    pub fn reload(mut self: Pin<&mut Self>) {
        if !crate::discover_queries::connected() {
            self.as_mut().rust_mut().report_text = ww::text_parity_no_daemon().to_string();
            self.as_mut().report_changed();
            return;
        }
        let payload = pb::RendererPluginListRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("renderer_plugin_list", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    /// Auto-reconciliation on opening the page (see the bridge).
    pub fn reload_if_connected(self: Pin<&mut Self>) {
        if !crate::discover_queries::connected() {
            return;
        }
        self.reload();
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_query::apply_parity_schema(&mut rust.base, &mut rust.data, frame.as_slice())
        };
        let Ok(signals) = parsed else {
            let signals = decode_failed(&mut self.as_mut().rust_mut().base);
            self.dispatch(signals);
            return;
        };
        self.as_mut().dispatch(signals);
        self.as_mut().rebuild_report();
    }

    /// Rebuild the panel, the badge and the mark from the last answer.
    fn rebuild_report(mut self: Pin<&mut Self>) {
        // The engine answered with an error — its text, or the core's text if it
        // is empty.
        if self.base.status == we_query::STATUS_ERROR {
            let text = if self.base.error.is_empty() {
                ww::text_parity_request_failed().to_string()
            } else {
                self.base.error.clone()
            };
            self.as_mut().rust_mut().report_text = text;
            self.as_mut().report_changed();
            return;
        }
        let Some(report) = self.rust().data.report.clone() else {
            return;
        };
        let daemon_version = self.rust().daemon_version.clone();
        // Tag of our protocol snapshot: on a mismatch with the live daemon's
        // version the badge honestly says that the schema was reconciled against
        // an OUTDATED protocol.
        // Warning: the tag is looked up in the TAB's directory, not in the
        // `rexon/` package: the path between them is known by the core. A direct
        // `pkg_dir` did not find the file, and the report wrote "(tag not
        // recorded)" (bench snapshot 06.09).
        let tag = we_parity::read_proto_upstream_tag_from_pkg(&we_env::pkg_dir());
        let proto_match = we_parity::daemon_matches_snapshot(&tag, &daemon_version);
        let text = ww::parity_report_header(&daemon_version, &tag)
            + &we_parity::render_report_text(&report);
        let badge = ww::parity_badge_parts(
            proto_match,
            &daemon_version,
            &tag,
            report.has_drift(),
            report.issues.len(),
            report.warning_count(),
            report.unchecked.iter().map(|(_, keys)| keys.len()).sum(),
        );
        let drift = ww::parity_drift_flag(report.has_drift(), proto_match);
        {
            let mut me = self.as_mut().rust_mut();
            me.report_text = text;
            me.badge = Some(badge);
            me.drift = drift;
        }
        self.as_mut().report_changed();
        self.as_mut().checked(drift);
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            // The panel, badge and mark are rebuilt by `rebuild_report` — it also
            // needs the snapshot tag, which the core does not have.
            "report" => self.as_mut().report_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Status lines are created exactly by the core's list of targets and in its
    /// order: the markup draws them in sequence, not by name.
    #[test]
    fn initial_lines_follow_core_targets() {
        let me = WeDeveloperModelRust::default();
        assert_eq!(me.lines.len(), ww::STATUS_TARGETS.len());
        for ((key, line), (want_key, want_name)) in me.lines.iter().zip(ww::STATUS_TARGETS) {
            assert_eq!(key, want_key);
            assert_eq!(line.text, ww::target_status_initial_parts(want_name).text);
            assert!(line.color.is_empty(), "у начальной строки краски нет");
        }
    }

    /// Install buttons are locked until the first check: there is nothing to
    /// install and nowhere to install from — the same initial state as in the
    /// widget version.
    #[test]
    fn install_buttons_locked_before_first_check() {
        let me = WeDeveloperModelRust::default();
        assert!(me.check_enabled);
        assert!(!me.update_enabled);
        assert!(!me.owe_enabled);
        assert!(!me.seen_enabled);
        assert!(!me.copy_enabled);
        assert_eq!(me.version_label, ww::version_label_initial());
    }
}
