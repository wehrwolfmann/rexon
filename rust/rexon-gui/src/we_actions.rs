// WeActions — the side-actions object of the "Wallpaper Engine" tab in
// Rust.
//
// WHY. Every QML page of the product expects, as the `actions` property, an
// object that can do what changes the world. For wallpapers the contract is
// the narrowest of the seven tabs — EXACTLY ONE method `retryDaemon`: bring
// up the daemon on the "Retry start" button of the "Daemon unavailable"
// screen. The contract is taken from the markup itself
// (`qml/WallpaperEnginePage.qml`), and it is held by the guard at the end
// of this file, so that "the page asked for a second method" is not
// discovered on a live window.
//
// Until now this object lived on PySide6 (`WallpaperEngineActions` in
// `rexon/qt_wallpaper_engine/qml_window.py`), and as long as it was there,
// the page got its actions from the Python host: without it the button was
// disabled, and the tab could not leave PySide6 in principle. The approach
// repeats `ExtensionsActions` (`src/extensions_actions.rs`) word for word:
// the object decides nothing by itself, the core decides everything.
//
// Warning: WHAT `retryDaemon` DOES AND WHY IN THIS ORDER. The Python original
// (`WallpaperEngineQmlWindow.retry_daemon`) took two steps: switching the
// model to the "connecting…" screen and `DaemonControl.ensure_running()`.
// Both are already in Rust: the autostart decisions are in the core
// (`we_client::ensure_running_decision`), the execution in
// `we_socket::ensure_running` (D-Bus, starting in a separate session,
// polling the bus). Only their order remained here, and it is not
// cosmetic: `ensure_running` can refuse SYNCHRONOUSLY (there is no daemon
// binary at all) and take the tab to the refusal screen right away. Swap
// the steps, and "connecting…" would overwrite the refusal reason already
// shown, leaving the customer looking at an eternal spinner instead of an
// explanation.
//
// The "connecting…" screen is set not by a direct model call but by the
// `retryStarted` signal, which the page handles with a `Connections`
// block. The reason is that the actions object has and must have no
// pointer to the page model: they are connected by the markup, not by
// Rust, exactly as `ExtensionsActions` with its
// `installFinished`/`uninstallFinished`. This also works when the page
// host substituted ITS OWN actions object: the old Python one does not
// send such a signal, `Connections` silently does not wait for it
// (`ignoreUnknownSignals`), and the "connecting…" screen is still set by
// the host itself.
//
// ── STITCHING THE DAEMON SIGNALS: NOW HERE ───────────────────────────────────
//
// The method is the smaller half of the work of the `qml_window.py` host.
// The larger half is the seams between the daemon client and the page
// model: the daemon came up → open the socket on the port it named; did
// not come up → `showNoDaemon`; the socket is open → `showConnecting` and a
// `health` request with a guard; the answer → `applyHealth`; silence →
// `showHealthTimeout`; a drop → `showConnecting`.
//
// Without them the Rust window (`rexon-main-window`, there is no Python
// there at all) opened the tab on "connecting…" and stayed there forever:
// nobody called the daemon on opening, and "Retry" returned to the same
// place.
//
// How it is done:
//   * events are produced by the transport (`we_socket.rs`) and handed out
//     to a LIST of subscribers (`we_socket::subscribe`) — next to the
//     Python slot, not instead of it. The Python tab does not lose its
//     events;
//   * this object subscribes ONLY when the page really uses it
//     (`connectDaemon`, called by the page on opening if the host did not
//     substitute its own actions). With the Python host the object sits in
//     the page idle and does not touch the daemon — otherwise two parties
//     would open the socket;
//   * an event arrives in the object's queue (`CxxQtThread::queue`) and is
//     relayed to the page by a signal, through the same channel as
//     `retryStarted`; there is still no pointer to the page model here;
//   * the `health` answer guard is the shim's timer
//     (`we_socket::request_health`), the timeout is the core's
//     (`we_window::HEALTH_TIMEOUT_MS`), and so is the start delay;
//   * the "there already was a connection" flag is one — the transport's
//     `WAS_CONNECTED`;
//   * preparing the wallpaper display on KDE moved into the same transport
//     (`we_socket::prepare_display_backend`) and works in both processes.

/// cxx-qt bridge of the actions object.
///
/// The type is declared `#[qml_element]`, so the page creates it for itself
/// (`WeActions { }`) and is complete without a host; the window host may
/// substitute its own.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

    }

    // The press pass-through shim. QObject does not need to be declared —
    // cxx-qt knows it by itself.
    unsafe extern "C++" {
        include!("rexon_gui_factory.h");
        /// # Safety
        /// `area` must be a live QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_pass_outside_presses(area: *mut QObject);
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // The KDE wallpaper plugin: a message for the user, an offer to install
        // Rexon's version and the "installing" flag. The decision is the core's
        // (`waywallen_kde_plugin::plan_plugin`), the delivery the transport's.
        #[qproperty(QString, kde_plugin_notice, cxx_name = "kdePluginNotice")]
        #[qproperty(bool, kde_plugin_can_install, cxx_name = "kdePluginCanInstall")]
        #[qproperty(bool, kde_plugin_installing, cxx_name = "kdePluginInstalling")]
        // The engine is starting: the transport's start guard waits for the bus
        // (`we_client::StartWatch`). While it runs, the shared waiting screen
        // guard is paused — the start has its own timeout.
        #[qproperty(bool, engine_starting, cxx_name = "engineStarting")]
        // Explanation under "Starting…" when the start is long but the process is
        // alive (`we_window::text_connecting_slow`); empty means there is nothing
        // to explain.
        #[qproperty(QString, engine_start_note, cxx_name = "engineStartNote")]
        // The banner button label: it follows the notice (`PluginNotice::button_label`
        // — Rexon's version, the packaged version, the plugin package).
        #[qproperty(QString, kde_plugin_install_label, cxx_name = "kdePluginInstallLabel")]
        type WeActions = super::WeActionsRust;
    }

    unsafe extern "RustQt" {
        /// A restart of the daemon was started.
        ///
        /// On it the page switches the model to the "connecting…" screen — the
        /// actions object itself does not and must not know the model.
        #[qsignal]
        #[cxx_name = "retryStarted"]
        fn retry_started(self: Pin<&mut WeActions>);

        /// Waiting for the engine: the socket opened and a readiness check is
        /// running, or the socket dropped and the transport reconnects by itself.
        #[qsignal]
        fn connecting(self: Pin<&mut WeActions>);

        /// The daemon did not come up. `reason` is the raw reason; the human
        /// label is composed from it by the core (the model's `showNoDaemon`).
        #[qsignal]
        #[cxx_name = "daemonFailed"]
        fn daemon_failed(self: Pin<&mut WeActions>, reason: QString);

        /// The `health` answer: the `state` field. The verdict is given by the core
        /// (`applyHealth`).
        #[qsignal]
        #[cxx_name = "healthReceived"]
        fn health_received(self: Pin<&mut WeActions>, state: QString);

        /// The engine connected and is silent on the readiness check.
        #[qsignal]
        #[cxx_name = "healthTimedOut"]
        fn health_timed_out(self: Pin<&mut WeActions>);
    }

    extern "RustQt" {
        /// The "Retry start" button on the "Daemon unavailable" screen.
        #[qinvokable]
        #[cxx_name = "retryDaemon"]
        fn retry_daemon(self: Pin<&mut WeActions>);

        /// Opening the tab: subscribe to daemon events and bring it up.
        #[qinvokable]
        #[cxx_name = "connectDaemon"]
        fn connect_daemon(self: Pin<&mut WeActions>);

        /// The "Install Rexon's version" button: consent to replace the KDE
        /// wallpaper plugin (a foreign one of unknown version, or one not matching
        /// the daemon).
        #[qinvokable]
        #[cxx_name = "installKdePlugin"]
        fn install_kde_plugin(self: Pin<&mut WeActions>);

        /// Mouse presses outside `area` (the wallpaper tab) reach the window even
        /// with a window open on top — otherwise the Rexon sections above the tab
        /// stopped being clickable (the `rexon_pass_outside_presses` shim).
        ///
        /// # Safety
        /// `area` is a live scene item.
        #[qinvokable]
        #[cxx_name = "passOutsidePresses"]
        unsafe fn pass_outside_presses(self: &WeActions, area: *mut QObject);
    }

    // A transport event is queued in the object — the thread trait must be
    // declared IN THE BRIDGE.
    impl cxx_qt::Threading for WeActions {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

use rexon_core::we_window as ww;

use crate::we_socket::{self, DaemonEvent};

/// Rust state of the actions object: exactly the subscription number.
///
/// The whole daemon autostart sequence — bus polling, the attempt counter,
/// the "there already was a connection" flag — lives in `we_socket.rs` as
/// one instance per thread. A second place storing the same would silently
/// drift away from the first (rule #12).
#[derive(Default)]
pub struct WeActionsRust {
    /// Number in the transport's subscriber list; 0 means not subscribed.
    subscription: u64,
    /// Subscription number for the KDE wallpaper plugin state; 0 means not
    /// subscribed.
    kde_subscription: u64,
    kde_plugin_notice: QString,
    kde_plugin_can_install: bool,
    kde_plugin_installing: bool,
    kde_plugin_install_label: QString,
    engine_starting: bool,
    engine_start_note: QString,
}

/// Unsubscribe together with the object: otherwise the transport would call
/// into the queue of a destroyed page (harmless, but it would pile up on
/// every opening).
impl Drop for WeActionsRust {
    fn drop(&mut self) {
        we_socket::unsubscribe(self.subscription);
        we_socket::unsubscribe_kde_plugin(self.kde_subscription);
    }
}

impl qobject::WeActions {
    /// # Safety
    /// `area` is a live scene item (passed by the markup).
    unsafe fn pass_outside_presses(&self, area: *mut qobject::QObject) {
        if !area.is_null() {
            // SAFETY: the pointer is not null and came from the QML engine.
            unsafe { qobject::rexon_pass_outside_presses(area) };
        }
    }

    fn retry_daemon(mut self: Pin<&mut Self>) {
        self.as_mut().subscribe_once();
        // First the waiting screen, then the attempt: `ensure_running` can refuse
        // synchronously, and its refusal screen must stay last. The order is
        // analysed in the file header.
        self.as_mut().retry_started();
        we_socket::ensure_running();
    }

    fn connect_daemon(mut self: Pin<&mut Self>) {
        self.as_mut().subscribe_once();
        we_socket::ensure_running_later(ww::STARTUP_DELAY_MS);
    }

    /// Subscribe to transport events once per lifetime of the object.
    fn subscribe_once(mut self: Pin<&mut Self>) {
        if self.rust().subscription != 0 {
            return;
        }
        let thread = self.qt_thread();
        let id = we_socket::subscribe(move |event| {
            let event = event.clone();
            // The object may have died — that is `ObjectDestroyed`, not a reason to
            // crash.
            let _ = thread.queue(move |obj| obj.on_daemon_event(event));
        });
        self.as_mut().rust_mut().subscription = id;

        // The KDE plugin state comes from the background preparation thread —
        // into the object's queue, like the daemon events.
        let thread = self.qt_thread();
        let kde = we_socket::subscribe_kde_plugin(move |state| {
            let state = state.clone();
            let _ = thread.queue(move |obj| obj.on_kde_plugin_state(state));
        });
        self.as_mut().rust_mut().kde_subscription = kde;
    }

    fn install_kde_plugin(self: Pin<&mut Self>) {
        we_socket::install_kde_plugin_with_consent();
    }

    /// Relay the state of the KDE wallpaper plugin to the page.
    fn on_kde_plugin_state(mut self: Pin<&mut Self>, state: we_socket::KdePluginState) {
        let text = match (&state.failure, &state.notice) {
            (Some(reason), _) => rexon_core::waywallen_kde_plugin::install_failed_text(reason),
            (None, Some(notice)) => notice.text(),
            (None, None) => String::new(),
        };
        let label = state.notice.as_ref().and_then(|n| n.button_label());
        let can_install = !state.installing && label.is_some();
        self.as_mut().set_kde_plugin_notice(QString::from(&text));
        self.as_mut().set_kde_plugin_install_label(QString::from(&label.unwrap_or_default()));
        self.as_mut().set_kde_plugin_can_install(can_install);
        self.as_mut().set_kde_plugin_installing(state.installing);
    }

    /// Relay a transport event to the page.
    fn on_daemon_event(mut self: Pin<&mut Self>, event: DaemonEvent) {
        // An engine start ends with a "ready" answer or a refusal.
        if matches!(event, DaemonEvent::Ready(_) | DaemonEvent::Failed(_)) {
            self.as_mut().set_engine_starting(false);
            self.as_mut().set_engine_start_note(QString::default());
        }
        match event {
            DaemonEvent::Starting { slow } => {
                self.as_mut().set_engine_starting(true);
                let note = if slow { ww::text_connecting_slow() } else { String::new() };
                self.as_mut().set_engine_start_note(QString::from(&note));
                self.as_mut().connecting();
            }
            DaemonEvent::Ready(status) => {
                if we_socket::is_open() {
                    // The socket is already open (the page was opened a second time in the
                    // same process): there will be no `connected` signal, so we ask right away.
                    self.as_mut().connecting();
                    we_socket::request_health(ww::HEALTH_TIMEOUT_MS);
                } else {
                    we_socket::open(i32::try_from(status.ws_port).unwrap_or(0));
                }
            }
            DaemonEvent::Failed(reason) => {
                self.as_mut().daemon_failed(QString::from(&reason));
            }
            DaemonEvent::Connected => {
                self.as_mut().connecting();
                we_socket::request_health(ww::HEALTH_TIMEOUT_MS);
            }
            DaemonEvent::Health(state) => {
                self.as_mut().health_received(QString::from(&state));
            }
            DaemonEvent::HealthTimeout => self.as_mut().health_timed_out(),
            // Messages for the user are shown by App as a toast (`errorOccurred`).
            DaemonEvent::Notice(_) => {}
            // Before the first connection the waiting screen is in place anyway.
            DaemonEvent::Disconnected { was_connected } => {
                if was_connected {
                    self.as_mut().connecting();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// The tab markup is the only source of the contract.
    const PAGE_QML: &str = include_str!("../qml/WallpaperEnginePage.qml");

    /// The names the page asks of the actions object.
    fn asked_methods(qml: &str) -> Vec<String> {
        const CALL: &str = "root.actions.";
        let mut found: Vec<String> = qml
            .match_indices(CALL)
            .map(|(at, _)| {
                qml[at + CALL.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect::<String>()
            })
            .filter(|name| !name.is_empty())
            .collect();
        found.sort();
        found.dedup();
        found
    }

    /// Contract guard: the page must ask for EXACTLY what the bridge can do.
    ///
    /// It is not there for looks. A divergence here is invisible entirely: both
    /// the build and `cargo test` are green — QML resolves a call only at run
    /// time, and an unknown method turns into a log line and a dead button for
    /// the customer. If the page gets a second request, either it is added
    /// here or this guard goes red.
    #[test]
    fn page_asks_only_what_the_bridge_provides() {
        assert_eq!(
            asked_methods(PAGE_QML),
            vec!["retryDaemon".to_string()],
            "договор страницы «Wallpaper Engine» с объектом действий разошёлся \
             с мостом src/we_actions.rs: допишите недостающий #[qinvokable] \
             (и уберите лишний), иначе вызов молча умрёт в работе"
        );
    }

    /// The reverse half of the same contract: someone must listen to the
    /// bridge's signal.
    ///
    /// Without a `retryStarted` handler the "Retry start" button would work,
    /// but the "connecting…" screen would not appear at all — the tab would
    /// look stuck on the refusal while the daemon is already coming up.
    #[test]
    fn page_listens_to_the_retry_signal() {
        assert!(
            PAGE_QML.contains("function onRetryStarted()"),
            "страница «Wallpaper Engine» перестала слушать сигнал retryStarted \
             объекта действий: экран «подключение…» после нажатия кнопки \
             «Повторить запуск» не появится"
        );
    }

    /// Every daemon signal must reach the page model.
    ///
    /// If the page loses any of the handlers, the tab gets stuck on one screen:
    /// without `onHealthReceived` forever on "connecting…" (exactly the defect
    /// for which the stitching moved here), without `onDaemonFailed` — on the
    /// same screen instead of an explanation.
    #[test]
    fn page_listens_to_every_daemon_signal() {
        for handler in [
            "function onConnecting()",
            "function onDaemonFailed(reason)",
            "function onHealthReceived(state)",
            "function onHealthTimedOut()",
        ] {
            assert!(
                PAGE_QML.contains(handler),
                "страница «Wallpaper Engine» не слушает `{handler}` объекта действий"
            );
        }
    }

    /// The shared waiting screen guard must be paused while the engine is
    /// starting, and the page must show the long-start explanation. Otherwise
    /// the first 25 s of a long (but honest) first AppImage start would end
    /// with a refusal — the defect of 30.10.2026 (r1895), only with another
    /// timer.
    #[test]
    fn page_pauses_screen_watchdog_while_engine_starts() {
        assert!(
            PAGE_QML.contains("!ownActions.engineStarting"),
            "сторож экрана ожидания не смотрит на engineStarting"
        );
        assert!(
            PAGE_QML.contains("ownActions.engineStartNote"),
            "страница не показывает пояснение долгого запуска"
        );
    }

    /// The page calls the daemon by itself on opening — but only with ITS OWN
    /// object.
    ///
    /// A foreign actions object (the Python host) drives the daemon by itself,
    /// and a second driver would open a second socket. A call through
    /// `root.actions` does not fit here also because the foreign object has no
    /// such method.
    #[test]
    fn page_connects_daemon_only_with_own_actions() {
        assert!(
            PAGE_QML.contains("if (root.actions === ownActions)")
                && PAGE_QML.contains("ownActions.connectDaemon()"),
            "страница «Wallpaper Engine» не зовёт демона при открытии: вкладка \
             останется на «подключение…» навсегда"
        );
    }
}
